//! Hardened, shell-free Git process construction.
//!
//! This is the generic Git execution boundary. It owns the environment
//! policy and the process plumbing shared by structured read operations and
//! callers that need a synchronous Git command. It does not know about
//! CodeGG sessions, runs, jobs, permissions, or projections.

use crate::EgggitError;
use std::io::Read;
use std::path::Path;
use std::process::{Command as StdCommand, Output as StdOutput, Stdio};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Default wall-clock bound for a single structured Git read.
///
/// `run` executes a synchronous child inside `spawn_blocking`, and a
/// blocking task cannot be cancelled: wrapping the call in
/// `tokio::time::timeout` would only abandon the task while the Git process
/// kept running and kept holding a blocking thread. The deadline therefore
/// has to be enforced around the child *inside* the blocking closure, where
/// exceeding it kills the process. 30s matches `GitExecutionService`'s
/// default so both halves of a `git_query` call share one budget.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll interval while waiting on the child inside [`run_command`].
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Environment variables restored for local, non-interactive Git commands.
pub const ALLOWED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "LANG",
    "LC_ALL",
    "LC_MESSAGES",
    "TZ",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "LANGUAGE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "GIT_SSL_CAPATH",
];

/// Command-bearing variables never inherited by a CodeGG-owned local Git
/// process. Network callers may explicitly overlay a reviewed allowlist, but
/// these hard-deny entries are always removed afterwards.
pub const ALWAYS_STRIPPED_ENV_VARS: &[&str] = &[
    "GIT_ASKPASS",
    "GIT_SSH_COMMAND",
    "GIT_SSH_VARIANT",
    "GIT_PROXY_COMMAND",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_KEY_1",
    "GIT_CONFIG_KEY_2",
    "GIT_CONFIG_KEY_3",
    "GIT_CONFIG_KEY_4",
    "GIT_CONFIG_KEY_5",
    "GIT_CONFIG_VALUE_0",
    "GIT_CONFIG_VALUE_1",
    "GIT_CONFIG_VALUE_2",
    "GIT_CONFIG_VALUE_3",
    "GIT_CONFIG_VALUE_4",
    "GIT_CONFIG_VALUE_5",
    "GIT_CONFIG_PARAMETERS",
    "SSH_ASKPASS",
    "GIT_TOOL",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_PAGER",
    "PAGER",
];

/// Base policy for local Git operations.
#[derive(Debug, Clone)]
pub struct GitEnvPolicy {
    pub terminal_prompt_disabled: bool,
    pub pin_editor: bool,
    pub strip_editors: bool,
    pub strip_command_bearers: bool,
}

impl Default for GitEnvPolicy {
    fn default() -> Self {
        Self {
            terminal_prompt_disabled: true,
            pin_editor: true,
            strip_editors: true,
            strip_command_bearers: true,
        }
    }
}

impl GitEnvPolicy {
    /// Construct an async command from a complete argv (`git` included).
    pub fn apply(&self, argv: &[String], cwd: &Path) -> Command {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        apply_environment(&mut cmd, self);
        cmd.kill_on_drop(true);
        cmd
    }

    /// Construct a synchronous command from a complete argv (`git` included).
    pub fn apply_sync(&self, argv: &[String], cwd: &Path) -> StdCommand {
        let mut cmd = StdCommand::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(cwd);
        apply_environment(&mut cmd, self);
        cmd
    }
}

fn apply_environment<C>(cmd: &mut C, policy: &GitEnvPolicy)
where
    C: CommandEnvironment,
{
    cmd.env_clear();
    for key in ALLOWED_ENV_VARS {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    if policy.strip_command_bearers {
        for key in ALWAYS_STRIPPED_ENV_VARS {
            cmd.env_remove(key);
        }
    }
    if policy.terminal_prompt_disabled {
        cmd.env("GIT_TERMINAL_PROMPT", "0".into());
    }
    if policy.pin_editor {
        cmd.env("GIT_EDITOR", "true".into());
        cmd.env("GIT_SEQUENCE_EDITOR", "true".into());
    }
    if policy.strip_editors {
        cmd.env_remove("EDITOR");
        cmd.env_remove("VISUAL");
    }
    cmd.env("GPG_TTY", "".into());
    cmd.env("GIT_PAGER", "cat".into());
    cmd.env("PAGER", "cat".into());
}

trait CommandEnvironment {
    fn env_clear(&mut self);
    fn env(&mut self, key: &str, value: std::ffi::OsString);
    fn env_remove(&mut self, key: &str);
}

impl CommandEnvironment for Command {
    fn env_clear(&mut self) {
        self.env_clear();
    }

    fn env(&mut self, key: &str, value: std::ffi::OsString) {
        self.env(key, value);
    }

    fn env_remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl CommandEnvironment for StdCommand {
    fn env_clear(&mut self) {
        self.env_clear();
    }

    fn env(&mut self, key: &str, value: std::ffi::OsString) {
        self.env(key, value);
    }

    fn env_remove(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Run a Git command whose `args` omit the executable name.
///
/// The child runs under [`DEFAULT_READ_TIMEOUT`]; exceeding it kills the
/// process rather than leaking it.
pub async fn run(args: &[String], cwd: &Path) -> Result<StdOutput, EgggitError> {
    run_with_timeout(args, cwd, DEFAULT_READ_TIMEOUT).await
}

/// Run a Git command whose `args` omit the executable name, bounded by an
/// explicit `timeout`.
pub async fn run_with_timeout(
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<StdOutput, EgggitError> {
    if !cwd.exists() {
        return Err(EgggitError::NotARepository(cwd.display().to_string()));
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend_from_slice(args);
    let cwd = cwd.to_path_buf();
    let rendered = argv.join(" ");
    tokio::task::spawn_blocking(move || {
        let mut cmd = GitEnvPolicy::default().apply_sync(&argv, &cwd);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_command(cmd, timeout, &rendered)
    })
    .await
    .map_err(|error| EgggitError::Join(error.to_string()))?
}

/// Spawn `cmd`, drain stdout/stderr on their own threads so a large-output
/// child cannot deadlock on a full pipe, and enforce `timeout` against the
/// running process.
///
/// The deadline is enforced here rather than around the `spawn_blocking`
/// join: a blocking task is not cancellable, so an outer
/// `tokio::time::timeout` would return to the caller while the Git process
/// kept running and kept holding a blocking-pool thread.
fn run_command(
    mut cmd: StdCommand,
    timeout: Duration,
    rendered: &str,
) -> Result<StdOutput, EgggitError> {
    let mut child = cmd
        .spawn()
        .map_err(|error| EgggitError::Io(error.to_string()))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_handle = stdout.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr_handle = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Kill the whole process group: `git` spawns helpers
                    // (pager, credential helper, filter driver) that would
                    // otherwise outlive the direct child.
                    kill_process_tree(&mut child);
                    return Err(EgggitError::Timeout(format!(
                        "git command timed out after {}s: {}",
                        timeout.as_secs(),
                        rendered
                    )));
                }
                std::thread::sleep(CHILD_POLL_INTERVAL);
            }
            Err(error) => {
                kill_process_tree(&mut child);
                return Err(EgggitError::Io(format!("failed to wait on git: {error}")));
            }
        }
    };

    let stdout_bytes = stdout_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr_bytes = stderr_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();

    Ok(StdOutput {
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        status,
    })
}

/// Kill the Git child and any helper processes it spawned.
///
/// `git` frequently delegates to a pager, credential helper, or external
/// diff/merge driver. Killing only the direct child leaves those helpers
/// holding the repository lock, which is what turns one slow read into a
/// cascade of stuck ones.
fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // Signal the child's process group so helpers spawned by `git` die
        // with it. A positive `pid` is required: signalling group 0 would
        // hit our own process group.
        let pid = child.id();
        if pid > 0 {
            #[cfg(target_os = "macos")]
            let pgid = pid as i32;
            #[cfg(not(target_os = "macos"))]
            let pgid = unsafe { libc::getpgid(pid as i32) };
            if pgid > 0 {
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run a read-only Git command with a hard stdout byte limit and the
/// default wall-clock bound. The child is killed as soon as it exceeds
/// either bound; stderr is discarded to avoid an unbounded diagnostic buffer.
pub async fn run_bounded(
    args: &[String],
    cwd: &Path,
    max_stdout_bytes: usize,
) -> Result<StdOutput, EgggitError> {
    run_bounded_with_timeout(args, cwd, max_stdout_bytes, DEFAULT_READ_TIMEOUT).await
}

/// Run a read-only Git command bounded by both a stdout byte limit and an
/// explicit wall-clock timeout.
///
/// Stdout is read under a `Take` bound so an oversized child is killed at
/// the limit instead of being allowed to fill memory. Reading on the calling
/// thread (rather than `wait_with_output`) is deliberate: it lets the byte
/// limit trip before the process exits.
pub async fn run_bounded_with_timeout(
    args: &[String],
    cwd: &Path,
    max_stdout_bytes: usize,
    timeout: Duration,
) -> Result<StdOutput, EgggitError> {
    if !cwd.exists() {
        return Err(EgggitError::NotARepository(cwd.display().to_string()));
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("git".to_owned());
    argv.extend_from_slice(args);
    let cwd = cwd.to_path_buf();
    let rendered = argv.join(" ");
    tokio::task::spawn_blocking(move || {
        let mut cmd = GitEnvPolicy::default().apply_sync(&argv, &cwd);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|error| EgggitError::Io(error.to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| EgggitError::Io("missing git stdout".into()))?;

        // Read one extra byte so the caller can detect truncation without
        // over-allocating.
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(max_stdout_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| EgggitError::Io(error.to_string()))?;
        if bytes.len() > max_stdout_bytes {
            kill_process_tree(&mut child);
            return Err(EgggitError::OutputTooLarge);
        }

        // stdout is drained, so only the deadline remains: reap promptly
        // instead of blocking until the child exits on its own.
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(StdOutput {
                        status,
                        stdout: bytes,
                        stderr: Vec::new(),
                    })
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_process_tree(&mut child);
                        return Err(EgggitError::Timeout(format!(
                            "git command timed out after {}s: {}",
                            timeout.as_secs(),
                            rendered
                        )));
                    }
                    std::thread::sleep(CHILD_POLL_INTERVAL);
                }
                Err(error) => {
                    kill_process_tree(&mut child);
                    return Err(EgggitError::Io(format!("failed to wait on git: {error}")));
                }
            }
        }
    })
    .await
    .map_err(|error| EgggitError::Join(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn policy_tables_are_disjoint_and_unique() {
        let allowed = ALLOWED_ENV_VARS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let stripped = ALWAYS_STRIPPED_ENV_VARS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(allowed.len(), ALLOWED_ENV_VARS.len());
        assert_eq!(stripped.len(), ALWAYS_STRIPPED_ENV_VARS.len());
        assert!(allowed.is_disjoint(&stripped));
    }

    #[test]
    fn default_read_timeout_is_bounded() {
        // The regression that motivated this constant: structured reads used
        // an unbounded `spawn_blocking` wait, so a stuck git could hang a
        // `git_query` call indefinitely.
        assert!(DEFAULT_READ_TIMEOUT <= Duration::from_secs(60));
    }

    /// Drive a command through `run_command` with a short deadline.
    ///
    /// `git` itself cannot be made to hang reliably from a test without a
    /// hostile hook, so the deadline logic is exercised directly against a
    /// long-running child.
    fn run_sleeping_command(timeout: Duration) -> Result<std::process::Output, EgggitError> {
        let mut cmd = StdCommand::new("sleep");
        cmd.arg("30");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_command(cmd, timeout, "sleep 30")
    }

    #[test]
    fn run_command_times_out_instead_of_waiting_forever() {
        let started = Instant::now();
        let result = run_sleeping_command(Duration::from_millis(300));

        assert!(
            matches!(result, Err(EgggitError::Timeout(_))),
            "a child outliving its deadline must surface as Timeout, got {:?}",
            result.err()
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "the deadline must be enforced promptly, took {elapsed:?}"
        );
    }

    #[test]
    fn run_command_returns_output_for_a_fast_child() {
        let mut cmd = StdCommand::new("sh");
        cmd.arg("-c").arg("printf hello");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = run_command(cmd, Duration::from_secs(10), "printf hello")
            .expect("a fast child must succeed");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello");
    }

    #[test]
    fn run_command_drains_stderr_without_deadlocking() {
        // A child writing more than a pipe buffer holds must not deadlock the
        // wait loop; stdout/stderr are drained concurrently for that reason.
        let mut cmd = StdCommand::new("sh");
        cmd.arg("-c")
            .arg("i=0; while [ $i -lt 2000 ]; do echo 'x' >&2; i=$((i+1)); done; printf done");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = run_command(cmd, Duration::from_secs(20), "large stderr")
            .expect("draining stderr must not deadlock");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "done");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_with_timeout_rejects_a_missing_directory() {
        let missing = PathBuf::from("/nonexistent-codegg-egggit-test");
        let result =
            run_with_timeout(&["status".to_string()], &missing, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(EgggitError::NotARepository(_))));
    }
}
