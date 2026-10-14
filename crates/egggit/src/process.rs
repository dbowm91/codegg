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

/// Executable authority that a Git request may obtain from repository data or
/// its transport. Callers use these facts to select a real contained backend;
/// the list is never treated as a reason to manufacture a process error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitExecutionCapability {
    RepositoryAlias,
    CleanSmudgeFilter,
    MergeDriver,
    Hook,
    ExternalDiffOrTextconv,
    RemoteHelper,
    Network,
    Credential,
    DestructiveRepositoryChange,
}

/// Policy classification for one shell-free Git argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitExecutionPolicyOutcome {
    AllowedTrustedNonExecutable,
    RequiresContainedExecution {
        capabilities: Vec<GitExecutionCapability>,
    },
    RequiresAdditionalAuthorization {
        capabilities: Vec<GitExecutionCapability>,
    },
    DeniedExplicitAuthority {
        reason: String,
    },
    InvalidRequest {
        reason: String,
    },
    BackendUnavailable {
        capabilities: Vec<GitExecutionCapability>,
    },
}

impl std::fmt::Display for GitExecutionPolicyOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllowedTrustedNonExecutable => formatter.write_str("trusted non-executable Git"),
            Self::RequiresContainedExecution { capabilities } => {
                write!(
                    formatter,
                    "contained Git execution required: {capabilities:?}"
                )
            }
            Self::RequiresAdditionalAuthorization { capabilities } => {
                write!(
                    formatter,
                    "additional Git authorization required: {capabilities:?}"
                )
            }
            Self::DeniedExplicitAuthority { reason } => write!(formatter, "Git denied: {reason}"),
            Self::InvalidRequest { reason } => write!(formatter, "invalid Git request: {reason}"),
            Self::BackendUnavailable { capabilities } => {
                write!(formatter, "Git backend unavailable for {capabilities:?}")
            }
        }
    }
}

impl std::error::Error for GitExecutionPolicyOutcome {}

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
    pub fn apply(&self, argv: &[String], cwd: &Path) -> Result<Command, GitExecutionPolicyOutcome> {
        ensure_trusted_git_request(argv)?;
        let worktree = canonical_worktree(cwd)?;
        let mut cmd = Command::new("git");
        add_trusted_git_overrides(&mut cmd, argv, &worktree);
        cmd.current_dir(cwd);
        apply_environment(&mut cmd, self, &worktree, argv);
        cmd.kill_on_drop(true);
        Ok(cmd)
    }

    /// Construct a synchronous command from a complete argv (`git` included).
    pub fn apply_sync(
        &self,
        argv: &[String],
        cwd: &Path,
    ) -> Result<StdCommand, GitExecutionPolicyOutcome> {
        ensure_trusted_git_request(argv)?;
        let worktree = canonical_worktree(cwd)?;
        let mut cmd = StdCommand::new("git");
        add_trusted_git_overrides(&mut cmd, argv, &worktree);
        cmd.current_dir(cwd);
        isolate_child_process_group(&mut cmd);
        apply_environment(&mut cmd, self, &worktree, argv);
        Ok(cmd)
    }
}

/// Classify a shell-free Git invocation before it crosses the process
/// boundary. Contained/authorization outcomes are typed handoffs; they never
/// become fabricated executable paths or generic spawn failures.
pub fn classify_git_request(argv: &[String]) -> GitExecutionPolicyOutcome {
    if argv.is_empty() || argv[0] != "git" {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "expected complete argv beginning with git".into(),
        };
    }

    // Global config and repository selectors change the authority context
    // after policy evaluation. CodeGG's typed renderers do not need them.
    if argv
        .iter()
        .skip(1)
        .take_while(|arg| arg.starts_with('-'))
        .any(|arg| {
            arg == "-c"
                || arg.starts_with("-c") && arg.len() > 2
                || arg == "--config-env"
                || arg.starts_with("--config-env=")
                || arg == "--git-dir"
                || arg.starts_with("--git-dir=")
                || arg == "--work-tree"
                || arg.starts_with("--work-tree=")
                || arg == "-C"
                || arg.starts_with("-C") && arg.len() > 2
                || arg == "--exec-path"
                || arg.starts_with("--exec-path=")
        })
    {
        return GitExecutionPolicyOutcome::DeniedExplicitAuthority {
            reason: "caller supplied global Git configuration or repository redirection".into(),
        };
    }

    let Some(subcommand) = argv.get(1).map(String::as_str) else {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "missing Git operation".into(),
        };
    };
    if subcommand.starts_with('-') {
        return GitExecutionPolicyOutcome::InvalidRequest {
            reason: "missing Git operation before operation arguments".into(),
        };
    }

    if matches!(subcommand, "diff" | "diff-tree" | "show" | "log" | "blame")
        && argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "--ext-diff" | "--textconv"))
    {
        return requires_containment(vec![GitExecutionCapability::ExternalDiffOrTextconv]);
    }

    // Operations that only inspect trusted repository state. Diff extension
    // execution paths are disabled explicitly by add_trusted_git_overrides.
    if matches!(
        subcommand,
        "fetch" | "pull" | "push" | "clone" | "ls-remote"
    ) {
        return remote_git_requires_containment();
    }

    if subcommand == "remote" {
        let operation = argv.get(2).map(String::as_str).unwrap_or_default();
        if matches!(operation, "update" | "prune")
            || operation == "show"
                && !argv
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-n" | "--no-fetch"))
        {
            return remote_git_requires_containment();
        }
        return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
    }

    if matches!(subcommand, "reset" | "clean") {
        // A dry-run clean (`-n`/`--dry-run`) changes nothing: it is the
        // read-only preview operators approve against, so it must not
        // require the destructive capability. `reset` has no dry-run mode
        // and a real clean still gates below.
        if subcommand == "clean"
            && argv
                .iter()
                .any(|arg| matches!(arg.as_str(), "-n" | "--dry-run"))
        {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        return GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
            capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange],
        };
    }

    let capabilities = match subcommand {
        "add" => vec![GitExecutionCapability::CleanSmudgeFilter],
        "checkout" | "restore" | "switch" => {
            vec![GitExecutionCapability::CleanSmudgeFilter]
        }
        "merge" | "cherry-pick" | "rebase" | "revert" => vec![
            GitExecutionCapability::CleanSmudgeFilter,
            GitExecutionCapability::MergeDriver,
            GitExecutionCapability::Hook,
        ],
        "commit" => return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable,
        "grep" if argv.iter().any(|arg| arg == "--textconv") => {
            vec![GitExecutionCapability::ExternalDiffOrTextconv]
        }
        "stash" if argv.get(2).is_some_and(|operation| operation == "list") => {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        "stash" => vec![
            GitExecutionCapability::CleanSmudgeFilter,
            GitExecutionCapability::Hook,
        ],
        "worktree" if argv.get(2).is_some_and(|operation| operation == "list") => {
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        "worktree" => vec![GitExecutionCapability::CleanSmudgeFilter],
        "am" | "bisect" => vec![GitExecutionCapability::Hook],
        "apply" | "blame" | "branch" | "cat-file" | "check-attr" | "config" | "diff"
        | "diff-tree" | "for-each-ref" | "grep" | "init" | "log" | "ls-files" | "ls-tree"
        | "merge-base" | "mv" | "notes" | "reflog" | "rev-parse" | "rev-list" | "rm" | "show"
        | "status" | "tag" | "update-ref" => {
            if subcommand == "cat-file" && argv.iter().any(|arg| arg == "--filters") {
                return requires_containment(vec![GitExecutionCapability::CleanSmudgeFilter]);
            }
            return GitExecutionPolicyOutcome::AllowedTrustedNonExecutable;
        }
        _ => vec![GitExecutionCapability::RepositoryAlias],
    };
    requires_containment(capabilities)
}

fn requires_containment(capabilities: Vec<GitExecutionCapability>) -> GitExecutionPolicyOutcome {
    GitExecutionPolicyOutcome::RequiresContainedExecution { capabilities }
}

fn remote_git_requires_containment() -> GitExecutionPolicyOutcome {
    requires_containment(vec![
        GitExecutionCapability::RemoteHelper,
        GitExecutionCapability::Network,
        GitExecutionCapability::Credential,
    ])
}

fn ensure_trusted_git_request(argv: &[String]) -> Result<(), GitExecutionPolicyOutcome> {
    match classify_git_request(argv) {
        GitExecutionPolicyOutcome::AllowedTrustedNonExecutable => Ok(()),
        GitExecutionPolicyOutcome::RequiresContainedExecution { capabilities } => {
            Err(GitExecutionPolicyOutcome::BackendUnavailable { capabilities })
        }
        other => Err(other),
    }
}

fn add_trusted_git_overrides<C>(cmd: &mut C, argv: &[String], worktree: &Path)
where
    C: CommandArguments,
{
    // These options take precedence over repository-local config and prevent
    // Git from promoting config values into child processes. Network routing
    // remains possible through the separately reviewed environment policy.
    for (key, value) in [
        ("core.fsmonitor", "false"),
        ("core.hooksPath", "/dev/null"),
        ("core.pager", "cat"),
        ("core.sshCommand", ""),
        ("credential.helper", ""),
        ("diff.external", ""),
        ("commit.gpgSign", "false"),
        ("tag.gpgSign", "false"),
        ("gpg.program", ""),
        ("core.askPass", ""),
        ("protocol.ext.allow", "never"),
    ] {
        cmd.arg("-c");
        cmd.arg(format!("{key}={value}"));
    }
    // The daemon supplies the repository root as cwd. Pin Git's worktree to
    // that selected root so repository-local `core.worktree` cannot redirect
    // a trusted read into an arbitrary host path. A newly created nested
    // repository is consequently data inside this root; it cannot retarget
    // an operation that was admitted for the root repository.
    cmd.arg(format!("--work-tree={}", worktree.display()));
    cmd.arg("--no-pager");
    if argv.get(1).is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "diff" | "diff-tree" | "show" | "log" | "blame"
        )
    }) {
        cmd.arg(argv[1].as_str());
        cmd.arg("--no-ext-diff");
        cmd.arg("--no-textconv");
        cmd.args(&argv[2..]);
    } else {
        cmd.args(&argv[1..]);
    }
}

trait CommandArguments {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self;
    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>;
}

impl CommandArguments for Command {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self {
        self.arg(arg)
    }

    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.args(args)
    }
}

impl CommandArguments for StdCommand {
    fn arg<S: AsRef<std::ffi::OsStr>>(&mut self, arg: S) -> &mut Self {
        self.arg(arg)
    }

    fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.args(args)
    }
}

/// Put commands that may need tree cleanup in a private process group.
///
/// `kill_process_tree` signals the child's process group on Unix. Without
/// making the child its own group leader first, `getpgid(child)` can return
/// the test runner or application process group and timeout cleanup would
/// kill unrelated processes along with the child.
fn isolate_child_process_group(cmd: &mut StdCommand) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

fn apply_environment<C>(cmd: &mut C, policy: &GitEnvPolicy, worktree: &Path, argv: &[String])
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
    cmd.env("GIT_CONFIG_NOSYSTEM", "1".into());
    cmd.env("GIT_CONFIG_GLOBAL", null_config_path().into());
    // Git normally walks parent directories to discover a repository. The
    // typed service has already selected `worktree`; do not let a missing
    // or newly replaced root marker silently select an enclosing repository.
    if let Some(parent) = worktree.parent() {
        cmd.env("GIT_CEILING_DIRECTORIES", parent.as_os_str().to_owned());
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

    apply_commit_identity(cmd, argv, worktree);
}

/// Preserve Git's user identity preference without loading arbitrary global
/// configuration into the trusted operation. Local values retain normal Git
/// precedence; only missing `user.name`/`user.email` values are copied from
/// global config into command-scoped configuration data. Helpers, hooks,
/// aliases, and other global settings remain disabled.
fn apply_commit_identity<C>(cmd: &mut C, argv: &[String], worktree: &Path)
where
    C: CommandEnvironment,
{
    if !needs_commit_identity(argv) {
        return;
    }

    let mut config = Vec::new();
    for key in ["user.name", "user.email"] {
        let value = read_config_value(worktree, "--local", key)
            .or_else(|| read_config_value(worktree, "--global", key));
        let Some(value) = value.filter(|value| valid_identity_value(value)) else {
            continue;
        };
        config.push((key, value));
    }

    if config.is_empty() {
        return;
    }
    cmd.env("GIT_CONFIG_COUNT", config.len().to_string().into());
    for (index, (key, value)) in config.into_iter().enumerate() {
        let key_env = format!("GIT_CONFIG_KEY_{index}");
        let value_env = format!("GIT_CONFIG_VALUE_{index}");
        cmd.env(&key_env, key.to_owned().into());
        cmd.env(&value_env, value.into());
    }
}

fn needs_commit_identity(argv: &[String]) -> bool {
    match argv.get(1).map(String::as_str) {
        Some("commit" | "merge" | "cherry-pick" | "revert" | "rebase" | "am") => true,
        Some("tag") => argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "-a" | "--annotate")),
        _ => false,
    }
}

fn valid_identity_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value
            .chars()
            .any(|character| character == '\0' || character == '\n' || character == '\r')
}

fn read_config_value(worktree: &Path, scope: &str, key: &str) -> Option<String> {
    let mut command = StdCommand::new("git");
    command
        .env_clear()
        .current_dir(worktree)
        .args(["config", scope, "--null", "--get", key])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat");
    for name in ["PATH", "HOME", "XDG_CONFIG_HOME", "GIT_CONFIG_GLOBAL"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut output = Vec::with_capacity(4097);
    let stdout = child.stdout.take()?;
    stdout.take(4097).read_to_end(&mut output).ok()?;
    if output.len() > 4096 {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    let value = std::str::from_utf8(&output).ok()?;
    let value = value.strip_suffix('\0').unwrap_or(value);
    valid_identity_value(value).then(|| value.to_owned())
}

fn canonical_worktree(cwd: &Path) -> Result<std::path::PathBuf, GitExecutionPolicyOutcome> {
    let worktree = cwd
        .canonicalize()
        .map_err(|_| GitExecutionPolicyOutcome::InvalidRequest {
            reason: "trusted Git worktree could not be canonicalized".into(),
        })?;
    if !worktree.is_dir() || worktree.to_str().is_none() {
        return Err(GitExecutionPolicyOutcome::InvalidRequest {
            reason: "trusted Git worktree must be an existing UTF-8 directory".into(),
        });
    }
    Ok(worktree)
}

#[cfg(windows)]
fn null_config_path() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_config_path() -> &'static str {
    "/dev/null"
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
        let mut cmd = GitEnvPolicy::default()
            .apply_sync(&argv, &cwd)
            .map_err(EgggitError::Policy)?;
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
    // `run_command` is also exercised with directly constructed commands in
    // tests, so enforce isolation here as well as in `GitEnvPolicy`.
    isolate_child_process_group(&mut cmd);
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
        let mut cmd = GitEnvPolicy::default()
            .apply_sync(&argv, &cwd)
            .map_err(EgggitError::Policy)?;
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

    #[test]
    fn trusted_git_policy_overrides_execution_bearing_repository_config() {
        let cwd = Path::new(".");
        let argv = vec!["git".to_string(), "diff".to_string()];
        let cmd = GitEnvPolicy::default().apply_sync(&argv, cwd).unwrap();
        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        for required in [
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "core.sshCommand=",
            "credential.helper=",
            "diff.external=",
            "commit.gpgSign=false",
            "tag.gpgSign=false",
            "--no-pager",
            "--no-ext-diff",
            "--no-textconv",
        ] {
            assert!(
                args.iter().any(|arg| arg == required),
                "missing {required:?}"
            );
        }
        assert!(args.iter().any(|arg| arg.starts_with("--work-tree=")));
    }

    #[test]
    fn git_policy_returns_execution_classes_without_fabricated_spawns() {
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            classify_git_request(&argv(&["git", "status"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "branch", "--show-current"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "add", "file"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![GitExecutionCapability::CleanSmudgeFilter]
            }
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "fetch", "origin"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![
                    GitExecutionCapability::RemoteHelper,
                    GitExecutionCapability::Network,
                    GitExecutionCapability::Credential,
                ]
            }
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "remote", "get-url", "origin"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert!(matches!(
            classify_git_request(&argv(&["git", "remote", "show", "origin"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution { .. }
        ));
        assert!(matches!(
            GitEnvPolicy::default().apply_sync(&argv(&["git", "add", "file"]), Path::new(".")),
            Err(GitExecutionPolicyOutcome::BackendUnavailable { .. })
        ));
        assert!(GitEnvPolicy::default()
            .apply_sync(&argv(&["git", "status"]), Path::new("."))
            .is_ok());
        assert!(matches!(
            classify_git_request(&argv(&["git", "show", "--textconv"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution { .. }
        ));
        assert_eq!(
            classify_git_request(&argv(&["git", "possibly-an-alias"])),
            GitExecutionPolicyOutcome::RequiresContainedExecution {
                capabilities: vec![GitExecutionCapability::RepositoryAlias]
            }
        );
        assert!(matches!(
            classify_git_request(&argv(&["not-git", "status"])),
            GitExecutionPolicyOutcome::InvalidRequest { .. }
        ));
    }

    #[test]
    fn git_clean_preview_is_allowed_but_real_clean_requires_destructive_authorization() {
        // C001 corrective: changing this classification is a policy change,
        // not a test update. Dry-run preview (`-n`/`--dry-run`) is read-only
        // and stays allowed; any real clean gates on an explicit
        // `DestructiveRepositoryChange` capability even inside a temp repo.
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "-n"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "-n", "-d"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        assert_eq!(
            classify_git_request(&argv(&["git", "clean", "--dry-run"])),
            GitExecutionPolicyOutcome::AllowedTrustedNonExecutable
        );
        for real in [
            argv(&["git", "clean", "-f"]),
            argv(&["git", "clean", "-f", "-d"]),
            argv(&["git", "clean", "-f", "-d", "--", "garbage-no-grant.txt"]),
        ] {
            assert_eq!(
                classify_git_request(&real),
                GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
                    capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange]
                },
                "real clean must gate on DestructiveRepositoryChange: {real:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&real, Path::new(".")),
                Err(GitExecutionPolicyOutcome::RequiresAdditionalAuthorization { .. })
            ));
        }
    }

    #[test]
    fn git_reset_modes_require_destructive_authorization() {
        // Reset/remote-test-contract C001: changing reset gating is a policy
        // change, not a test update. Reset has no dry-run mode: every real
        // reset gates on an explicit `DestructiveRepositoryChange`
        // capability even inside a temp repo.
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        for reset in [
            argv(&["git", "reset", "--hard", "HEAD~1"]),
            argv(&["git", "reset", "--soft", "HEAD~1"]),
            argv(&["git", "reset", "--mixed", "HEAD~1"]),
            argv(&["git", "reset", "--merge", "HEAD"]),
            argv(&["git", "reset", "--keep", "HEAD"]),
            argv(&["git", "reset", "HEAD"]),
        ] {
            assert_eq!(
                classify_git_request(&reset),
                GitExecutionPolicyOutcome::RequiresAdditionalAuthorization {
                    capabilities: vec![GitExecutionCapability::DestructiveRepositoryChange]
                },
                "reset must gate on DestructiveRepositoryChange: {reset:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&reset, Path::new(".")),
                Err(GitExecutionPolicyOutcome::RequiresAdditionalAuthorization { .. })
            ));
        }
    }

    #[test]
    fn git_push_and_pull_require_contained_remote_backend() {
        // Reset/remote-test-contract C002: remote gating covers every
        // transport, including local paths — a `file://` remote is not a
        // safety grant. Changing this classification is a policy change
        // owned by Linux C003, not a test update. (`fetch` is already
        // pinned in the execution-classes fixture above.)
        let argv = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        for remote in [
            argv(&["git", "push", "origin", "main"]),
            argv(&["git", "push", "--force-with-lease", "origin", "main"]),
            argv(&["git", "pull", "origin", "main"]),
            argv(&["git", "pull", "--ff-only", "origin", "main"]),
            argv(&["git", "clone", "/tmp/some-local-remote"]),
        ] {
            assert_eq!(
                classify_git_request(&remote),
                GitExecutionPolicyOutcome::RequiresContainedExecution {
                    capabilities: vec![
                        GitExecutionCapability::RemoteHelper,
                        GitExecutionCapability::Network,
                        GitExecutionCapability::Credential,
                    ]
                },
                "remote Git must require the contained backend: {remote:?}"
            );
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&remote, Path::new(".")),
                Err(GitExecutionPolicyOutcome::BackendUnavailable { .. })
            ));
        }
    }

    #[test]
    fn trusted_git_policy_denies_caller_config_and_repository_redirects() {
        let cwd = Path::new(".");
        for extra in [
            vec![
                "-c".to_string(),
                "core.fsmonitor=true".to_string(),
                "status".to_string(),
            ],
            vec![
                "--config-env=core.fsmonitor=GIT_CONFIG_COUNT".to_string(),
                "status".to_string(),
            ],
            vec!["-C/other/repo".to_string(), "status".to_string()],
            vec![
                "--git-dir=/other/repo/.git".to_string(),
                "status".to_string(),
            ],
        ] {
            let mut argv = vec!["git".to_string()];
            argv.extend(extra);
            assert!(matches!(
                GitEnvPolicy::default().apply_sync(&argv, cwd),
                Err(GitExecutionPolicyOutcome::DeniedExplicitAuthority { .. })
            ));
        }

        // `-c` is also a valid subcommand option (for example `git grep -c`);
        // only global configuration overrides before the subcommand are denied.
        let grep_count = vec!["git".to_string(), "grep".to_string(), "-c".to_string()];
        assert!(GitEnvPolicy::default().apply_sync(&grep_count, cwd).is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_status_does_not_run_repository_fsmonitor() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(root)
            .output()
            .expect("git init");
        assert!(init.status.success());

        let marker = root.join("fsmonitor-ran");
        let hook = root.join("fsmonitor.sh");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\ntouch '{}'\nprintf 'token\\n'\n",
                marker.display()
            ),
        )
        .expect("write fsmonitor hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make fsmonitor executable");
        let config = StdCommand::new("git")
            .args([
                "config",
                "core.fsmonitor",
                hook.to_str().expect("hook path"),
            ])
            .current_dir(root)
            .status()
            .expect("set hostile fsmonitor");
        assert!(config.success());

        let status = run(&["status".to_string()], root)
            .await
            .expect("trusted status");
        assert!(status.status.success());
        assert!(!marker.exists(), "repository fsmonitor must not execute");
    }

    #[tokio::test]
    async fn trusted_git_status_cannot_be_redirected_by_local_core_worktree() {
        let fixture = tempfile::tempdir().expect("fixture");
        let root = fixture.path().join("repo");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).expect("repo root");
        std::fs::create_dir(&outside).expect("outside root");
        let init = StdCommand::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&root)
            .output()
            .expect("git init");
        assert!(init.status.success());
        std::fs::write(outside.join("host-secret-name.txt"), "outside\n").expect("outside marker");
        let config = StdCommand::new("git")
            .args(["config", "core.worktree"])
            .arg(&outside)
            .current_dir(&root)
            .status()
            .expect("set hostile worktree");
        assert!(config.success());

        let status = run(&["status".to_string()], &root)
            .await
            .expect("trusted status");
        assert!(status.status.success());
        let stdout = String::from_utf8_lossy(&status.stdout);
        assert!(
            !stdout.contains("host-secret-name.txt"),
            "trusted Git must stay bound to the daemon-selected worktree: {stdout}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_commit_does_not_run_repository_hooks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let run = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .expect("git command")
        };
        assert!(run(&["init", "--initial-branch=main"]).status.success());
        assert!(run(&["config", "user.email", "test@example.com"])
            .status
            .success());
        assert!(run(&["config", "user.name", "Test"]).status.success());

        let marker = root.join("hook-ran");
        let hook_dir = root.join(".git/hooks");
        let hook = hook_dir.join("post-commit");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
            .expect("write post-commit hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make hook executable");
        std::fs::write(root.join("tracked.txt"), "content\n").expect("write file");
        assert!(run(&["add", "tracked.txt"]).status.success());

        let output = super::run(
            &[
                "commit".to_string(),
                "-m".to_string(),
                "initial".to_string(),
            ],
            root,
        )
        .await
        .expect("trusted commit");
        assert!(output.status.success());
        assert!(
            !marker.exists(),
            "repository post-commit hook must not execute"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trusted_git_commit_uses_global_identity_without_loading_global_hooks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("repo");
        let global = dir.path().join("user.gitconfig");
        let hooks = dir.path().join("global-hooks");
        std::fs::create_dir_all(&root).expect("repo dir");
        std::fs::create_dir_all(&hooks).expect("hooks dir");
        let marker = dir.path().join("global-hook-ran");
        let hook = hooks.join("post-commit");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
            .expect("write global post-commit hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make global hook executable");
        std::fs::write(
            &global,
            format!(
                "[user]\n\tname = Safe Test User\n\temail = safe@example.invalid\n[core]\n\thooksPath = {}\n",
                hooks.display()
            ),
        )
        .expect("write isolated global config");

        let previous_global = std::env::var_os("GIT_CONFIG_GLOBAL");
        // The repository's nextest profile isolates environment-mutating
        // tests by process. Restore the setting even if an assertion fails.
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
        let _restore_global = RestoreEnvVar::new("GIT_CONFIG_GLOBAL", previous_global);

        let run = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("git command")
        };
        assert!(run(&["init", "--initial-branch=main"]).status.success());
        std::fs::write(root.join("tracked.txt"), "content\n").expect("write tracked file");
        assert!(run(&["add", "tracked.txt"]).status.success());

        let output = super::run(&["commit".into(), "-m".into(), "initial".into()], &root)
            .await
            .expect("trusted commit with global identity");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!marker.exists(), "global hooks must remain disabled");

        let log = super::run(
            &["log".into(), "-1".into(), "--format=%an <%ae>".into()],
            &root,
        )
        .await
        .expect("read committed identity");
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim(),
            "Safe Test User <safe@example.invalid>"
        );
    }

    struct RestoreEnvVar {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl RestoreEnvVar {
        fn new(name: &'static str, previous: Option<std::ffi::OsString>) -> Self {
            Self { name, previous }
        }
    }

    impl Drop for RestoreEnvVar {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
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
