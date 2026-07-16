//! Canonical execution service for scheduler-owned argv processes.
//!
//! This module deliberately accepts argv rather than a shell command.  It is
//! the common boundary for non-interactive managed processes: the environment
//! is rebuilt from an allowlist, output is drained without unbounded growth,
//! and cancellation/timeout cleanup targets the whole process session on Unix.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use nix::unistd::setsid;

/// Default maximum number of bytes retained per output stream.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 256 * 1024;

const TERMINATION_GRACE: Duration = Duration::from_millis(250);

/// The environment policy applied before a managed process is spawned.
///
/// The default clears the parent environment and restores only the reviewed
/// common-process allowlist from `codegg-git`. Callers may add explicitly
/// required variables, but command-bearing variables remain denied.
#[derive(Debug, Clone)]
pub struct EnvironmentPolicy {
    inherited: BTreeSet<OsString>,
    overrides: BTreeMap<OsString, OsString>,
    denied: BTreeSet<OsString>,
}

impl Default for EnvironmentPolicy {
    fn default() -> Self {
        Self::sanitized()
    }
}

impl EnvironmentPolicy {
    /// Construct the default sanitized environment policy.
    pub fn sanitized() -> Self {
        let inherited = codegg_git::ALLOWED_ENV_VARS
            .iter()
            .map(OsString::from)
            .collect();
        let denied = codegg_git::ALWAYS_STRIPPED_ENV_VARS
            .iter()
            .map(OsString::from)
            .collect();
        Self {
            inherited,
            overrides: BTreeMap::new(),
            denied,
        }
    }

    /// Add a variable to the inherited allowlist.
    pub fn allow_inherited_var(mut self, name: impl Into<OsString>) -> Self {
        self.inherited.insert(name.into());
        self
    }

    /// Deny a variable even if it is in the inherited allowlist.
    pub fn deny_var(mut self, name: impl Into<OsString>) -> Self {
        self.denied.insert(name.into());
        self
    }

    /// Set an explicit variable for the child. Denied variables cannot be
    /// reintroduced through this method.
    pub fn with_var(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let name = name.into();
        if !self.denied.contains(&name) {
            self.overrides.insert(name, value.into());
        }
        self
    }

    fn apply(&self, command: &mut Command) {
        command.env_clear();

        for name in &self.inherited {
            if self.denied.contains(name) {
                continue;
            }
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &self.overrides {
            if !self.denied.contains(name) {
                command.env(name, value);
            }
        }

        // Keep managed jobs noninteractive and deterministic. These are
        // applied after caller variables so the service owns these invariants.
        command
            .env("CI", "1")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat");
    }
}

/// Bounded capture settings for stdout and stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputPolicy {
    pub max_bytes: usize,
}

impl OutputPolicy {
    pub const fn new(max_bytes: usize) -> Self {
        Self { max_bytes }
    }
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_OUTPUT_BYTES)
    }
}

/// Head/tail output capture that never retains more than the configured cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub omitted_bytes: usize,
    pub total_bytes: usize,
    pub total_lines: usize,
}

impl BoundedOutput {
    fn with_capacity(cap: usize) -> Self {
        let head_cap = cap.div_ceil(2);
        let tail_cap = cap.saturating_sub(head_cap);
        Self {
            head: Vec::with_capacity(head_cap),
            tail: Vec::with_capacity(tail_cap),
            omitted_bytes: 0,
            total_bytes: 0,
            total_lines: 0,
        }
    }

    fn append(&mut self, bytes: &[u8], cap: usize) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        self.total_lines = self
            .total_lines
            .saturating_add(bytes.iter().filter(|&&byte| byte == b'\n').count());

        let head_cap = cap.div_ceil(2);
        let tail_cap = cap.saturating_sub(head_cap);
        let head_remaining = head_cap.saturating_sub(self.head.len());
        let head_take = head_remaining.min(bytes.len());
        self.head.extend_from_slice(&bytes[..head_take]);

        if head_take < bytes.len() && tail_cap > 0 {
            self.tail.extend_from_slice(&bytes[head_take..]);
            if self.tail.len() > tail_cap {
                let excess = self.tail.len() - tail_cap;
                self.tail.drain(..excess);
            }
        }

        self.omitted_bytes = self
            .total_bytes
            .saturating_sub(self.head.len().saturating_add(self.tail.len()));
    }

    pub fn is_truncated(&self) -> bool {
        self.omitted_bytes > 0
    }

    pub fn retained_bytes(&self) -> usize {
        self.head.len().saturating_add(self.tail.len())
    }

    /// Return the retained bytes in display order. When truncated, the
    /// omitted middle is intentionally not reconstructed.
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.retained_bytes());
        bytes.extend_from_slice(&self.head);
        bytes.extend_from_slice(&self.tail);
        bytes
    }

    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.as_bytes()).into_owned()
    }
}

/// Job and attempt identity made available to the child for audit and
/// diagnostics. Secret material must not be placed in this type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessProvenance {
    pub job_id: String,
    pub attempt_id: String,
}

impl ProcessProvenance {
    pub fn new(job_id: impl Into<String>, attempt_id: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            attempt_id: attempt_id.into(),
        }
    }

    fn apply(&self, command: &mut Command) {
        if !self.job_id.is_empty() {
            command
                .env("CODEGG_JOB_ID", &self.job_id)
                .env("CODEGG_SCHEDULER_JOB_ID", &self.job_id);
        }
        if !self.attempt_id.is_empty() {
            command
                .env("CODEGG_ATTEMPT_ID", &self.attempt_id)
                .env("CODEGG_SCHEDULER_ATTEMPT_ID", &self.attempt_id);
        }
        command.env("CODEGG_MANAGED_PROCESS", "1");
    }
}

/// Request to run one non-shell argv process.
#[derive(Debug, Clone)]
pub struct ManagedProcessRequest {
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment_policy: EnvironmentPolicy,
    pub timeout: Option<Duration>,
    pub cancellation: CancellationToken,
    pub output_policy: OutputPolicy,
    pub provenance: ProcessProvenance,
}

impl ManagedProcessRequest {
    pub fn new(argv: Vec<OsString>, cwd: PathBuf, provenance: ProcessProvenance) -> Self {
        Self {
            argv,
            cwd,
            environment_policy: EnvironmentPolicy::default(),
            timeout: None,
            cancellation: CancellationToken::new(),
            output_policy: OutputPolicy::default(),
            provenance,
        }
    }
}

/// Why the child stopped running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    Exited,
    TimedOut,
    Cancelled,
}

/// Captured result from a managed process.
#[derive(Debug, Clone)]
pub struct ManagedProcessResult {
    pub exit_status: ExitStatus,
    pub stdout: BoundedOutput,
    pub stderr: BoundedOutput,
    pub duration: Duration,
    pub termination: TerminationReason,
}

#[derive(Debug, Error)]
pub enum ManagedProcessError {
    #[error("managed process argv must not be empty")]
    EmptyArgv,
    #[error("managed process was cancelled before spawn")]
    CancelledBeforeSpawn,
    #[error("failed to spawn managed process: {0}")]
    Spawn(#[source] io::Error),
    #[error("failed waiting for managed process: {0}")]
    Wait(#[source] io::Error),
    #[error("failed reading managed process output: {0}")]
    ReadOutput(#[source] io::Error),
    #[error("managed process output reader task failed: {0}")]
    OutputReaderTask(String),
}

/// Stateless entry point for scheduler-owned process execution.
#[derive(Debug, Default, Clone, Copy)]
pub struct ManagedProcessService;

impl ManagedProcessService {
    pub async fn run(
        request: ManagedProcessRequest,
    ) -> Result<ManagedProcessResult, ManagedProcessError> {
        run(request).await
    }

    pub async fn execute(
        request: ManagedProcessRequest,
    ) -> Result<ManagedProcessResult, ManagedProcessError> {
        run(request).await
    }
}

pub async fn run(
    request: ManagedProcessRequest,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    let ManagedProcessRequest {
        argv,
        cwd,
        environment_policy,
        timeout: process_timeout,
        cancellation,
        output_policy,
        provenance,
    } = request;

    let executable = argv.first().ok_or(ManagedProcessError::EmptyArgv)?;
    if cancellation.is_cancelled() {
        return Err(ManagedProcessError::CancelledBeforeSpawn);
    }

    let mut command = Command::new(executable);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    environment_policy.apply(&mut command);
    provenance.apply(&mut command);
    configure_process_session(&mut command);

    let mut child = command.spawn().map_err(ManagedProcessError::Spawn)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ManagedProcessError::ReadOutput(io::Error::other("stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ManagedProcessError::ReadOutput(io::Error::other("stderr was not piped")))?;

    let max_output_bytes = output_policy.max_bytes;
    let stdout_task = tokio::spawn(read_bounded(stdout, max_output_bytes));
    let stderr_task = tokio::spawn(read_bounded(stderr, max_output_bytes));
    let started = Instant::now();

    let (exit_status, termination) =
        wait_for_child(&mut child, &cancellation, process_timeout).await?;
    let stdout = join_output(stdout_task).await?;
    let stderr = join_output(stderr_task).await?;

    Ok(ManagedProcessResult {
        exit_status,
        stdout,
        stderr,
        duration: started.elapsed(),
        termination,
    })
}

async fn read_bounded<R>(mut reader: R, cap: usize) -> Result<BoundedOutput, io::Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = BoundedOutput::with_capacity(cap);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }
        output.append(&buffer[..read], cap);
    }
}

async fn join_output(
    task: tokio::task::JoinHandle<Result<BoundedOutput, io::Error>>,
) -> Result<BoundedOutput, ManagedProcessError> {
    match task.await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(ManagedProcessError::ReadOutput(error)),
        Err(error) => Err(ManagedProcessError::OutputReaderTask(error.to_string())),
    }
}

async fn wait_for_child(
    child: &mut Child,
    cancellation: &CancellationToken,
    process_timeout: Option<Duration>,
) -> Result<(ExitStatus, TerminationReason), ManagedProcessError> {
    let timeout_future = async move {
        match process_timeout {
            Some(duration) => sleep(duration).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(timeout_future);

    tokio::select! {
        status = child.wait() => {
            status
                .map(|status| (status, TerminationReason::Exited))
                .map_err(ManagedProcessError::Wait)
        }
        _ = cancellation.cancelled() => {
            let status = terminate_child(child).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::Cancelled))
        }
        _ = &mut timeout_future => {
            let status = terminate_child(child).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::TimedOut))
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn configure_process_session(command: &mut Command) {
    // A new session makes the child the process-group leader. This lets
    // timeout and cancellation cleanup reach descendants without signaling
    // unrelated scheduler or daemon processes.
    unsafe {
        command.pre_exec(|| {
            setsid().map_err(|error| io::Error::other(format!("setsid failed: {error}")))?;
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_session(_command: &mut Command) {}

#[cfg(unix)]
#[allow(unsafe_code)]
async fn terminate_child(child: &mut Child) -> io::Result<ExitStatus> {
    if let Some(pid) = child.id() {
        // The child owns this process group because configure_process_session
        // called setsid before exec. Negative pid addresses the group.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    } else {
        child.start_kill()?;
    }

    match timeout(TERMINATION_GRACE, child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            } else {
                child.start_kill()?;
            }
            child.wait().await
        }
    }
}

#[cfg(not(unix))]
async fn terminate_child(child: &mut Child) -> io::Result<ExitStatus> {
    child.kill().await?;
    child.wait().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn request(script: &str) -> ManagedProcessRequest {
        ManagedProcessRequest::new(
            vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from(script),
            ],
            std::env::current_dir().expect("test cwd"),
            ProcessProvenance::new("job-test", "attempt-test"),
        )
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn success_captures_output_and_provenance() {
        let result = run(request(
            "printf 'out'; printf 'err' >&2; test \"$CODEGG_JOB_ID\" = job-test && test \"$CODEGG_ATTEMPT_ID\" = attempt-test",
        ))
        .await
        .expect("managed process succeeds");

        assert!(result.exit_status.success());
        assert_eq!(result.termination, TerminationReason::Exited);
        assert_eq!(result.stdout.to_string_lossy(), "out");
        assert_eq!(result.stderr.to_string_lossy(), "err");
        assert!(result.duration < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn timeout_and_cancellation_kill_the_process_group() {
        let mut timed_out = request("sleep 10");
        timed_out.timeout = Some(Duration::from_millis(50));
        let result = run(timed_out).await.expect("timeout result");
        assert_eq!(result.termination, TerminationReason::TimedOut);
        assert!(!result.exit_status.success());

        let cancellation = CancellationToken::new();
        let mut cancelled = request("sleep 10");
        cancelled.cancellation = cancellation.clone();
        let task = tokio::spawn(run(cancelled));
        sleep(Duration::from_millis(50)).await;
        cancellation.cancel();
        let result = task
            .await
            .expect("join cancellation task")
            .expect("cancel result");
        assert_eq!(result.termination, TerminationReason::Cancelled);
        assert!(!result.exit_status.success());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn output_is_bounded_while_the_pipe_is_drained() {
        let mut request = request("head -c 100000 /dev/zero");
        request.output_policy = OutputPolicy::new(64);
        let result = run(request).await.expect("bounded output result");

        assert_eq!(result.stdout.total_bytes, 100_000);
        assert!(result.stdout.is_truncated());
        assert!(result.stdout.retained_bytes() <= 64);
        assert_eq!(
            result.stdout.omitted_bytes + result.stdout.retained_bytes(),
            100_000
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_argv_is_rejected_before_spawn() {
        let request = ManagedProcessRequest::new(
            Vec::new(),
            std::env::current_dir().expect("test cwd"),
            ProcessProvenance::default(),
        );
        assert!(matches!(
            run(request).await,
            Err(ManagedProcessError::EmptyArgv)
        ));
    }
}
