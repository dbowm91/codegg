//! Canonical execution service for scheduler-owned argv processes.
//!
//! This module deliberately accepts argv rather than a shell command.  It is
//! the common boundary for non-interactive managed processes: the environment
//! is rebuilt from an allowlist, output is drained without unbounded growth,
//! and cancellation/timeout cleanup targets the whole process session on Unix.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

#[cfg(unix)]
type SandboxStatusWriter = OwnedFd;
#[cfg(not(unix))]
type SandboxStatusWriter = ();

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use nix::unistd::setsid;

use crate::security::sandbox::{
    decode_sandbox_status, SandboxLaunchOutcome, SandboxLaunchSpec, MAX_SANDBOX_SPEC_BYTES,
    MAX_SANDBOX_STATUS_BYTES,
};

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

    /// Construct a policy that inherits the current environment while still
    /// removing variables known to alter command execution or credential
    /// lookup. This is used only for explicitly human-originated commands
    /// whose shell semantics require the user's environment.
    pub fn inherited() -> Self {
        let denied = codegg_git::ALWAYS_STRIPPED_ENV_VARS
            .iter()
            .map(OsString::from)
            .collect();
        Self {
            inherited: std::env::vars_os().map(|(name, _)| name).collect(),
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

/// Overflow behavior for a bounded output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Keep draining the pipe, retaining only the configured head/tail.
    ContinueDrain,
    /// Terminate the process group as soon as either stream exceeds its cap.
    Terminate,
}

/// Which output stream exceeded its configured limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// A bounded output chunk emitted by the streaming adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedProcessOutputChunk {
    pub stream: OutputStream,
    pub bytes: Vec<u8>,
}

/// Bounded capture settings for stdout and stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputPolicy {
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub overflow: OverflowPolicy,
}

impl OutputPolicy {
    pub const fn new(max_bytes: usize) -> Self {
        Self {
            stdout_limit: max_bytes,
            stderr_limit: max_bytes,
            overflow: OverflowPolicy::ContinueDrain,
        }
    }

    pub const fn with_limits(stdout_limit: usize, stderr_limit: usize) -> Self {
        Self {
            stdout_limit,
            stderr_limit,
            overflow: OverflowPolicy::ContinueDrain,
        }
    }

    pub const fn terminate_on_overflow(mut self) -> Self {
        self.overflow = OverflowPolicy::Terminate;
        self
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
    pub tail: VecDeque<u8>,
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
            tail: VecDeque::with_capacity(tail_cap),
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
            self.tail.extend(&bytes[head_take..]);
            let excess = self.tail.len().saturating_sub(tail_cap);
            self.tail.drain(..excess);
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
        bytes.extend(self.tail.iter().copied());
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

/// Stdin behavior for a finite managed process.
#[derive(Debug, Clone, Default)]
pub enum StdinPolicy {
    #[default]
    Null,
    Bytes(Vec<u8>),
}

/// Sandbox request passed to the canonical executor. The helper specification
/// and status channel are deliberately local process plumbing.
#[derive(Debug, Clone, Default)]
pub enum SandboxRequest {
    #[default]
    Disabled,
    Required(SandboxLaunchSpec),
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
    pub executable: OsString,
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment_policy: EnvironmentPolicy,
    pub stdin: StdinPolicy,
    pub timeout: Option<Duration>,
    pub cancellation: CancellationToken,
    pub output_policy: OutputPolicy,
    pub sandbox: SandboxRequest,
    pub provenance: ProcessProvenance,
}

impl ManagedProcessRequest {
    pub fn new(argv: Vec<OsString>, cwd: PathBuf, provenance: ProcessProvenance) -> Self {
        let mut argv = argv.into_iter();
        let executable = argv.next().unwrap_or_default();
        Self {
            executable,
            argv: argv.collect(),
            cwd,
            environment_policy: EnvironmentPolicy::default(),
            stdin: StdinPolicy::Null,
            timeout: None,
            cancellation: CancellationToken::new(),
            output_policy: OutputPolicy::default(),
            sandbox: SandboxRequest::Disabled,
            provenance,
        }
    }

    pub fn full_argv(&self) -> Vec<OsString> {
        std::iter::once(self.executable.clone())
            .chain(self.argv.iter().cloned())
            .collect()
    }
}

/// Why the child stopped running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimitExceeded { stream: OutputStream },
}

/// Sandbox result observed by the canonical process service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxExecutionOutcome {
    Disabled,
    Enforced { abi: u32 },
}

/// Diagnostics from process-group cleanup. Errors are retained on the result
/// so a target exit is never confused with cleanup failure.
#[derive(Debug, Clone, Default)]
pub struct CleanupDiagnostics {
    pub process_group_established: bool,
    pub graceful_signal_sent: bool,
    pub forced_signal_sent: bool,
    pub errors: Vec<String>,
}

/// Captured result from a managed process.
#[derive(Debug, Clone)]
pub struct ManagedProcessResult {
    pub exit_status: ExitStatus,
    pub stdout: BoundedOutput,
    pub stderr: BoundedOutput,
    pub duration: Duration,
    pub termination: TerminationReason,
    pub sandbox: SandboxExecutionOutcome,
    pub cleanup: CleanupDiagnostics,
}

#[derive(Debug, Error)]
pub enum ManagedProcessError {
    #[error("managed process argv must not be empty")]
    EmptyArgv,
    #[error("managed process executable or argument contains an interior NUL: {0}")]
    InvalidArgument(String),
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
    #[error("managed process sandbox failed: {0}")]
    SandboxFailed(String),
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

    /// Execute a managed process from a synchronous adapter while retaining
    /// the same async lifecycle implementation and safety guarantees.
    pub fn run_blocking(
        request: ManagedProcessRequest,
    ) -> Result<ManagedProcessResult, ManagedProcessError> {
        std::thread::Builder::new()
            .name("codegg-managed-process".to_string())
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| ManagedProcessError::Wait(io::Error::other(error)))?
                    .block_on(run(request))
            })
            .map_err(|error| ManagedProcessError::Wait(io::Error::other(error)))?
            .join()
            .map_err(|_| {
                ManagedProcessError::Wait(io::Error::other(
                    "managed process blocking adapter panicked",
                ))
            })?
    }

    /// Execute a managed process while forwarding bounded stdout/stderr
    /// chunks to a caller-owned channel. The final result remains authoritative
    /// for exit, timeout, cancellation, truncation, and cleanup state.
    pub async fn run_streaming(
        request: ManagedProcessRequest,
        output_tx: mpsc::Sender<ManagedProcessOutputChunk>,
    ) -> Result<ManagedProcessResult, ManagedProcessError> {
        run_inner(request, Some(output_tx), None).await
    }
}

pub async fn run(
    request: ManagedProcessRequest,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    run_inner(request, None, None).await
}

async fn run_inner(
    request: ManagedProcessRequest,
    output_tx: Option<mpsc::Sender<ManagedProcessOutputChunk>>,
    #[cfg(test)] helper_override: Option<&Path>,
    #[cfg(not(test))] _helper_override: Option<&Path>,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    let ManagedProcessRequest {
        executable,
        argv,
        cwd,
        environment_policy,
        stdin,
        timeout: process_timeout,
        cancellation,
        output_policy,
        sandbox,
        provenance,
    } = request;

    if executable.is_empty() {
        return Err(ManagedProcessError::EmptyArgv);
    }
    if executable.to_string_lossy().contains('\0') {
        return Err(ManagedProcessError::InvalidArgument(
            "executable".to_string(),
        ));
    }
    if let Some(index) = argv
        .iter()
        .position(|arg| arg.to_string_lossy().contains('\0'))
    {
        return Err(ManagedProcessError::InvalidArgument(format!(
            "argument {index}"
        )));
    }
    if cancellation.is_cancelled() {
        return Err(ManagedProcessError::CancelledBeforeSpawn);
    }

    let (launch_argv, _sandbox_file, status_reader, status_writer) = prepare_launch_argv(
        &executable,
        &argv,
        &cwd,
        &sandbox,
        #[cfg(test)]
        helper_override,
    )?;
    let mut launch_argv = launch_argv.into_iter();
    let launch_executable = launch_argv.next().ok_or(ManagedProcessError::EmptyArgv)?;
    let mut command = Command::new(launch_executable);
    command
        .args(launch_argv)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    match &stdin {
        StdinPolicy::Null => {
            command.stdin(std::process::Stdio::null());
        }
        StdinPolicy::Bytes(_) => {
            command.stdin(std::process::Stdio::piped());
        }
    }
    environment_policy.apply(&mut command);
    provenance.apply(&mut command);
    configure_process_session(&mut command);
    #[cfg(unix)]
    if let Some(writer) = status_writer.as_ref() {
        configure_status_writer(&mut command, writer.as_raw_fd());
    }

    let mut child = command.spawn().map_err(ManagedProcessError::Spawn)?;
    drop(status_writer);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ManagedProcessError::ReadOutput(io::Error::other("stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ManagedProcessError::ReadOutput(io::Error::other("stderr was not piped")))?;

    let (overflow_tx, mut overflow_rx) = mpsc::channel(2);
    let stdout_task = tokio::spawn(read_bounded(
        stdout,
        output_policy.stdout_limit,
        OutputStream::Stdout,
        overflow_tx.clone(),
        output_policy.overflow,
        output_tx.clone(),
    ));
    let stderr_task = tokio::spawn(read_bounded(
        stderr,
        output_policy.stderr_limit,
        OutputStream::Stderr,
        overflow_tx,
        output_policy.overflow,
        output_tx,
    ));
    let status_task = status_reader.map(|reader| tokio::spawn(read_status(reader)));
    let stdin_task = match &stdin {
        StdinPolicy::Null => None,
        StdinPolicy::Bytes(bytes) => child.stdin.take().map(|mut stdin| {
            let bytes = bytes.clone();
            tokio::spawn(async move {
                let result = stdin.write_all(&bytes).await;
                drop(stdin);
                result
            })
        }),
    };
    let started = Instant::now();

    let (exit_status, termination, cleanup) = wait_for_child(
        &mut child,
        &cancellation,
        process_timeout,
        &mut overflow_rx,
        output_policy.overflow,
    )
    .await?;
    let stdout = join_output(stdout_task).await?;
    let stderr = join_output(stderr_task).await?;
    if let Some(task) = stdin_task {
        if let Err(error) = tokio::time::timeout(TERMINATION_GRACE, task).await {
            tracing::debug!("stdin-drain task did not finish in grace period: {}", error);
        }
    }

    let sandbox_status = if let Some(task) = status_task {
        match tokio::time::timeout(STATUS_READ_TIMEOUT, task).await {
            Ok(Ok(result)) => Some(result.map_err(|error| {
                ManagedProcessError::SandboxFailed(format!(
                    "sandbox status channel read failed: {error}"
                ))
            })?),
            Ok(Err(error)) => {
                return Err(ManagedProcessError::SandboxFailed(format!(
                    "sandbox status reader task failed: {error}"
                )));
            }
            Err(_) => {
                return Err(ManagedProcessError::SandboxFailed(
                    "sandbox status channel did not close".to_string(),
                ));
            }
        }
    } else {
        None
    };
    let sandbox = interpret_sandbox_status(sandbox_status.as_deref(), &sandbox)?;

    Ok(ManagedProcessResult {
        exit_status,
        stdout,
        stderr,
        duration: started.elapsed(),
        termination,
        sandbox,
        cleanup,
    })
}

type PreparedLaunchArgv = (
    Vec<OsString>,
    Option<tempfile::NamedTempFile>,
    Option<tokio::fs::File>,
    Option<SandboxStatusWriter>,
);

fn prepare_launch_argv(
    executable: &OsString,
    argv: &[OsString],
    _cwd: &PathBuf,
    sandbox: &SandboxRequest,
    #[cfg(test)] helper_override: Option<&Path>,
) -> Result<PreparedLaunchArgv, ManagedProcessError> {
    let full_argv = std::iter::once(executable.clone())
        .chain(argv.iter().cloned())
        .collect::<Vec<_>>();
    match sandbox {
        SandboxRequest::Disabled => Ok((full_argv, None, None, None)),
        SandboxRequest::Required(spec) => {
            #[cfg(not(unix))]
            {
                let _ = spec;
                return Err(ManagedProcessError::SandboxFailed(
                    "sandbox helper is unavailable on this platform".to_string(),
                ));
            }
            #[cfg(unix)]
            {
                let helper = {
                    #[cfg(test)]
                    if let Some(helper) = helper_override {
                        helper.to_path_buf()
                    } else {
                        crate::security::sandbox::sandbox_helper_path()
                            .map_err(ManagedProcessError::SandboxFailed)?
                    }
                    #[cfg(not(test))]
                    {
                        crate::security::sandbox::sandbox_helper_path()
                            .map_err(ManagedProcessError::SandboxFailed)?
                    }
                };
                let mut file = tempfile::NamedTempFile::new()
                    .map_err(|e| ManagedProcessError::SandboxFailed(e.to_string()))?;
                let bytes = serde_json::to_vec(&spec)
                    .map_err(|e| ManagedProcessError::SandboxFailed(e.to_string()))?;
                if bytes.len() > MAX_SANDBOX_SPEC_BYTES {
                    return Err(ManagedProcessError::SandboxFailed(
                        "sandbox specification exceeds 64 KiB".to_string(),
                    ));
                }
                file.write_all(&bytes)
                    .map_err(|e| ManagedProcessError::SandboxFailed(e.to_string()))?;
                #[cfg(unix)]
                let (status_reader, status_writer) = create_status_pipe()
                    .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
                Ok((
                    vec![
                        helper.into_os_string(),
                        OsString::from("--spec"),
                        file.path().as_os_str().to_os_string(),
                        OsString::from("--status-fd"),
                        OsString::from("3"),
                    ],
                    Some(file),
                    Some(status_reader),
                    Some(status_writer),
                ))
            }
        }
    }
}

const STATUS_READ_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(unix)]
#[allow(unsafe_code)]
fn create_status_pipe() -> io::Result<(tokio::fs::File, OwnedFd)> {
    // The parent reader and writer are both close-on-exec. The child-side
    // pre-exec hook duplicates only the writer to the helper's fixed fd.
    let mut fds = [0_i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(io::Error::last_os_error());
        }
    }
    let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((
        tokio::fs::File::from_std(std::fs::File::from(reader)),
        writer,
    ))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn configure_status_writer(command: &mut Command, source_fd: i32) {
    unsafe {
        command.pre_exec(move || {
            if source_fd != crate::security::sandbox::SANDBOX_STATUS_FD {
                if libc::dup2(source_fd, crate::security::sandbox::SANDBOX_STATUS_FD) < 0 {
                    return Err(io::Error::last_os_error());
                }
                libc::close(source_fd);
            }
            if libc::fcntl(
                crate::security::sandbox::SANDBOX_STATUS_FD,
                libc::F_SETFD,
                0,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

async fn read_status(mut reader: tokio::fs::File) -> Result<Vec<u8>, io::Error> {
    let mut bytes = Vec::new();
    let mut limited = (&mut reader).take((MAX_SANDBOX_STATUS_BYTES + 1) as u64);
    limited.read_to_end(&mut bytes).await?;
    if bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sandbox status stream exceeds 16 KiB",
        ));
    }
    Ok(bytes)
}

async fn read_bounded<R>(
    mut reader: R,
    cap: usize,
    stream: OutputStream,
    overflow_tx: mpsc::Sender<OutputStream>,
    overflow_policy: OverflowPolicy,
    output_tx: Option<mpsc::Sender<ManagedProcessOutputChunk>>,
) -> Result<BoundedOutput, io::Error>
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
        if let Some(output_tx) = output_tx.as_ref() {
            let stream_len = cap.saturating_sub(output.total_bytes).min(read);
            if stream_len > 0 {
                let chunk = ManagedProcessOutputChunk {
                    stream,
                    bytes: buffer[..stream_len].to_vec(),
                };
                if output_tx.try_send(chunk).is_err() {
                    tracing::debug!(?stream, "managed process output chunk dropped");
                }
            }
        }
        let was_truncated = output.is_truncated();
        output.append(&buffer[..read], cap);
        if !was_truncated && output.is_truncated() && overflow_policy == OverflowPolicy::Terminate {
            // Cap enforcement does not depend on this send; termination is
            // driven by the truncated flag. Log drops for observability.
            if overflow_tx.try_send(stream).is_err() {
                tracing::debug!("output-cap overflow notice dropped (no receiver)");
            }
        }
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
    overflow_rx: &mut mpsc::Receiver<OutputStream>,
    overflow_policy: OverflowPolicy,
) -> Result<(ExitStatus, TerminationReason, CleanupDiagnostics), ManagedProcessError> {
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
                .map(|status| (status, TerminationReason::Exited, CleanupDiagnostics::default()))
                .map_err(ManagedProcessError::Wait)
        }
        _ = cancellation.cancelled() => {
            let (status, cleanup) = terminate_child(child).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::Cancelled, cleanup))
        }
        _ = &mut timeout_future => {
            let (status, cleanup) = terminate_child(child).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::TimedOut, cleanup))
        }
        stream = overflow_rx.recv(), if overflow_policy == OverflowPolicy::Terminate => {
            let stream = stream.unwrap_or(OutputStream::Stdout);
            let (status, cleanup) = terminate_child(child).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::OutputLimitExceeded { stream }, cleanup))
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
async fn terminate_child(child: &mut Child) -> io::Result<(ExitStatus, CleanupDiagnostics)> {
    let pid = child.id();
    let mut cleanup = CleanupDiagnostics {
        process_group_established: pid.is_some(),
        ..CleanupDiagnostics::default()
    };
    if let Some(pid) = pid {
        signal_process_group(pid, libc::SIGTERM, &mut cleanup);
        let waited = tokio::time::timeout(TERMINATION_GRACE, child.wait()).await;
        if let Ok(status) = waited {
            // The direct child can exit while descendants keep the session
            // alive. Always complete the grace interval and issue the forced
            // group signal before returning from cancellation/timeout.
            sleep(TERMINATION_GRACE).await;
            signal_process_group(pid, libc::SIGKILL, &mut cleanup);
            return status.map(|status| (status, cleanup));
        }
        signal_process_group(pid, libc::SIGKILL, &mut cleanup);
    } else {
        child.start_kill()?;
    }
    child.wait().await.map(|status| (status, cleanup))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn signal_process_group(pid: u32, signal: libc::c_int, cleanup: &mut CleanupDiagnostics) {
    let pid = pid as libc::pid_t;
    let group = unsafe { libc::getpgid(pid) };
    if group == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        // The session leader may already have exited while its descendants
        // keep the process group alive. The group was established by the
        // successful setsid pre-exec hook, so the retained PGID remains a
        // safe cleanup target; do not mistake a missing leader for a missing
        // process group.
        let result = unsafe { libc::kill(-pid, signal) };
        if result == 0 {
            if signal == libc::SIGTERM {
                cleanup.graceful_signal_sent = true;
            } else {
                cleanup.forced_signal_sent = true;
            }
        }
        return;
    }
    if group != pid {
        cleanup.errors.push(format!(
            "refused process-group signal: child pgid {group} did not match child pid {pid}"
        ));
        return;
    }
    let result = unsafe { libc::kill(-pid, signal) };
    if result == 0 {
        if signal == libc::SIGTERM {
            cleanup.graceful_signal_sent = true;
        } else {
            cleanup.forced_signal_sent = true;
        }
    } else if io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
        cleanup.errors.push(format!(
            "process-group signal {signal} failed: {}",
            io::Error::last_os_error()
        ));
    }
}

#[cfg(not(unix))]
async fn terminate_child(child: &mut Child) -> io::Result<(ExitStatus, CleanupDiagnostics)> {
    child.kill().await?;
    child
        .wait()
        .await
        .map(|status| (status, CleanupDiagnostics::default()))
}

fn interpret_sandbox_status(
    status_bytes: Option<&[u8]>,
    sandbox: &SandboxRequest,
) -> Result<SandboxExecutionOutcome, ManagedProcessError> {
    if matches!(sandbox, SandboxRequest::Disabled) {
        return Ok(SandboxExecutionOutcome::Disabled);
    }
    let status = status_bytes.ok_or_else(|| {
        ManagedProcessError::SandboxFailed("sandbox status channel was not created".to_string())
    })?;
    match decode_sandbox_status(status).map_err(ManagedProcessError::SandboxFailed)? {
        SandboxLaunchOutcome::Enforced { abi } => Ok(SandboxExecutionOutcome::Enforced { abi }),
        SandboxLaunchOutcome::Unavailable { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox unavailable: {reason}"),
        )),
        SandboxLaunchOutcome::SetupError { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox setup failed: {reason}"),
        )),
        SandboxLaunchOutcome::ExecError { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox target exec failed: {reason}"),
        )),
    }
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

    #[tokio::test(flavor = "multi_thread")]
    async fn interior_nul_is_rejected_before_spawn() {
        let mut request = request("true");
        request.argv = vec![OsString::from("bad\0argument")];

        let error = run(request).await.expect_err("NUL must be rejected");
        assert!(matches!(
            error,
            ManagedProcessError::InvalidArgument(field) if field == "argument 0"
        ));
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
    async fn target_stderr_markers_are_preserved_as_target_output() {
        let result = run(request(
            "printf 'CODEGG_SANDBOX_ENFORCED abi=9\\nCODEGG_SANDBOX_ERROR setup: forged\\n' >&2",
        ))
        .await
        .expect("managed process succeeds");

        assert_eq!(
            result.stderr.to_string_lossy(),
            "CODEGG_SANDBOX_ENFORCED abi=9\nCODEGG_SANDBOX_ERROR setup: forged\n"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn missing_private_status_fails_closed_with_test_only_helper_injection() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("helper fixture directory");
        let helper = directory.path().join("fake-helper");
        std::fs::write(&helper, b"#!/bin/sh\nexit 0\n").expect("fake helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("fake helper executable permissions");
        let mut request = request("true");
        request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
            target: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "true".to_string()],
            read_paths: Vec::new(),
            write_paths: Vec::new(),
        });

        let error = run_inner(request, None, Some(&helper))
            .await
            .expect_err("missing status must fail closed");
        assert!(
            matches!(error, ManagedProcessError::SandboxFailed(reason) if reason.contains("status"))
        );
    }

    #[cfg(all(test, target_os = "linux"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn private_status_channel_supports_read_only_cwd_and_keeps_target_output() {
        if crate::security::sandbox::probe_landlock().is_err() {
            return;
        }
        let workspace = tempfile::tempdir().expect("workspace");
        let config = crate::security::sandbox::SandboxConfig::new()
            .with_enabled(true)
            .with_allowed_paths(vec![workspace.path().to_string_lossy().to_string()]);
        let spec = config
            .launch_spec(
                "/bin/sh",
                &[
                    "-c".to_string(),
                    "printf 'CODEGG_SANDBOX_ENFORCED abi=9\\n' >&2".to_string(),
                ],
                Some(workspace.path()),
            )
            .expect("launch spec");
        let mut request = ManagedProcessRequest::new(
            vec![
                OsString::from("/bin/sh"),
                OsString::from("-c"),
                OsString::from("printf 'CODEGG_SANDBOX_ENFORCED abi=9\\n' >&2"),
            ],
            workspace.path().to_path_buf(),
            ProcessProvenance::default(),
        );
        request.sandbox = SandboxRequest::Required(spec);
        let helper = std::env::current_exe()
            .expect("test executable")
            .parent()
            .and_then(|path| path.parent())
            .expect("target debug directory")
            .join("codegg-sandbox-helper");
        let permissions = std::fs::metadata(workspace.path())
            .expect("workspace metadata")
            .permissions();
        let mut readonly = permissions;
        readonly.set_readonly(true);
        std::fs::set_permissions(workspace.path(), readonly).expect("read-only workspace");

        let result = run_inner(request, None, Some(&helper))
            .await
            .expect("sandbox target succeeds in read-only cwd");
        assert!(matches!(
            result.sandbox,
            SandboxExecutionOutcome::Enforced { .. }
        ));
        assert_eq!(
            result.stderr.to_string_lossy(),
            "CODEGG_SANDBOX_ENFORCED abi=9\n"
        );
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

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn stdout_and_stderr_have_independent_limits_and_invalid_utf8_is_lossy() {
        let mut request = request("printf '\\377\\376'; head -c 4096 /dev/zero; printf 'e' >&2; head -c 4096 /dev/zero >&2");
        request.output_policy = OutputPolicy::with_limits(32, 48);
        let result = run(request).await.expect("dual-stream result");

        assert!(result.stdout.is_truncated());
        assert!(result.stderr.is_truncated());
        assert!(result.stdout.retained_bytes() <= 32);
        assert!(result.stderr.retained_bytes() <= 48);
        assert!(result.stdout.to_string_lossy().contains('\u{fffd}'));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn terminate_on_overflow_reports_the_stream() {
        let mut request = request("head -c 100000 /dev/zero");
        request.output_policy = OutputPolicy::new(64).terminate_on_overflow();
        let result = run(request).await.expect("overflow result");
        assert_eq!(
            result.termination,
            TerminationReason::OutputLimitExceeded {
                stream: OutputStream::Stdout
            }
        );
        assert!(result.stdout.retained_bytes() <= 64);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn stdin_is_written_by_the_managed_service() {
        let mut request = request("read value; printf '%s' \"$value\"");
        request.stdin = StdinPolicy::Bytes(b"managed-input\n".to_vec());
        let result = run(request).await.expect("stdin result");
        assert_eq!(result.stdout.to_string_lossy(), "managed-input");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn streaming_forwards_bounded_output_and_retains_final_result() {
        let mut request = request("printf 'stdout'; printf 'stderr' >&2");
        request.output_policy = OutputPolicy::new(64);
        let (tx, mut rx) = mpsc::channel(16);
        let result = ManagedProcessService::run_streaming(request, tx)
            .await
            .expect("streaming process result");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        while let Some(chunk) = rx.recv().await {
            match chunk.stream {
                OutputStream::Stdout => stdout.extend(chunk.bytes),
                OutputStream::Stderr => stderr.extend(chunk.bytes),
            }
        }
        assert_eq!(stdout, b"stdout");
        assert_eq!(stderr, b"stderr");
        assert_eq!(result.stdout.to_string_lossy(), "stdout");
        assert_eq!(result.stderr.to_string_lossy(), "stderr");
    }

    #[test]
    fn bounded_output_retains_latest_tail_without_shifting() {
        let mut output = BoundedOutput::with_capacity(64);
        let bytes: Vec<u8> = (0..=255).cycle().take(1_000_000).collect();
        for chunk in bytes.chunks(8192) {
            output.append(chunk, 64);
        }

        assert_eq!(output.retained_bytes(), 64);
        assert_eq!(output.head, bytes[..32]);
        assert_eq!(
            output.tail.iter().copied().collect::<Vec<_>>(),
            bytes[bytes.len() - 32..]
        );
        assert_eq!(output.omitted_bytes, bytes.len() - 64);
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
