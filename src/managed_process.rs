//! Canonical execution service for scheduler-owned argv processes.
//!
//! This module deliberately accepts argv rather than an implicit shell command.
//! The one explicit exception is the Windows `cmd.exe /C` route, whose raw
//! command-line grammar is carried in a typed request field. It is the common
//! boundary for non-interactive managed processes: the environment is rebuilt
//! from an allowlist, output is drained without unbounded growth, and
//! cancellation/timeout cleanup targets the whole process session on Unix.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
#[allow(dead_code)]
mod windows_acl_lease;

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
pub fn windows_acl_lease_dacl_snapshot(path: &Path) -> std::io::Result<Vec<u8>> {
    windows_acl_lease::dacl_snapshot(path)
}

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
        let inherited: BTreeSet<OsString> = codegg_git::ALLOWED_ENV_VARS
            .iter()
            .map(OsString::from)
            .collect();
        #[cfg(windows)]
        let inherited = {
            let mut inherited = inherited;
            inherited.extend(
                ["SystemRoot", "WINDIR", "TEMP", "TMP"]
                    .into_iter()
                    .map(OsString::from),
            );
            inherited
        };
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

    pub(crate) fn apply(&self, command: &mut Command) {
        command.env_clear();
        for (name, value) in self.entries() {
            command.env(name, value);
        }
    }

    fn entries(&self) -> BTreeMap<OsString, OsString> {
        let mut entries = BTreeMap::new();
        for name in &self.inherited {
            if self.denied.contains(name) {
                continue;
            }
            if let Some(value) = std::env::var_os(name) {
                entries.insert(name.clone(), value);
            }
        }
        for (name, value) in &self.overrides {
            if !self.denied.contains(name) {
                entries.insert(name.clone(), value.clone());
            }
        }
        // Keep managed jobs noninteractive and deterministic. These are
        // applied after caller variables so the service owns these invariants.
        for (name, value) in [
            ("CI", "1"),
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_PAGER", "cat"),
            ("PAGER", "cat"),
        ] {
            entries.insert(name.into(), value.into());
        }
        entries
    }
}

/// Overflow behavior for a bounded output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Keep draining the pipe, retaining only the configured head/tail.
    ContinueDrain,
    /// Terminate the owned process tree as soon as either stream exceeds its cap.
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
    /// Host-authenticated model session, when this process belongs to an
    /// interactive or delegated model execution. Never populated from argv.
    pub session_id: Option<String>,
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
            session_id: None,
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
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

    #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
    fn apply_environment(&self, environment: &mut BTreeMap<OsString, OsString>) {
        if !self.job_id.is_empty() {
            environment.insert("CODEGG_JOB_ID".into(), self.job_id.clone().into());
            environment.insert("CODEGG_SCHEDULER_JOB_ID".into(), self.job_id.clone().into());
        }
        if !self.attempt_id.is_empty() {
            environment.insert("CODEGG_ATTEMPT_ID".into(), self.attempt_id.clone().into());
            environment.insert(
                "CODEGG_SCHEDULER_ATTEMPT_ID".into(),
                self.attempt_id.clone().into(),
            );
        }
        environment.insert("CODEGG_MANAGED_PROCESS".into(), "1".into());
    }
}

/// Request to run one managed process. `argv` is passed directly to the child;
/// `raw_command` is reserved for Windows `cmd.exe /C` shell text.
#[derive(Debug, Clone)]
pub struct ManagedProcessRequest {
    pub executable: OsString,
    pub argv: Vec<OsString>,
    /// An uninterpreted Windows `cmd.exe /C` command appended after argv.
    /// This is shell text and must only be set for an explicitly selected cmd shell.
    pub raw_command: Option<OsString>,
    pub cwd: PathBuf,
    pub environment_policy: EnvironmentPolicy,
    pub stdin: StdinPolicy,
    pub timeout: Option<Duration>,
    pub cancellation: CancellationToken,
    pub output_policy: OutputPolicy,
    pub sandbox: SandboxRequest,
    pub provenance: ProcessProvenance,
    /// Accepted authorization provenance for the invocation, when the
    /// process is launched from an agent tool call. This remains local
    /// plumbing and is not projected into child environment variables.
    pub execution_authorization: Option<crate::tool::authorization::ExecutionAuthorizationContext>,
}

impl ManagedProcessRequest {
    pub fn new(argv: Vec<OsString>, cwd: PathBuf, provenance: ProcessProvenance) -> Self {
        let mut argv = argv.into_iter();
        let executable = argv.next().unwrap_or_default();
        Self {
            executable,
            argv: argv.collect(),
            raw_command: None,
            cwd,
            environment_policy: EnvironmentPolicy::default(),
            stdin: StdinPolicy::Null,
            timeout: None,
            cancellation: CancellationToken::new(),
            output_policy: OutputPolicy::default(),
            sandbox: SandboxRequest::Disabled,
            provenance,
            execution_authorization: None,
        }
    }

    pub fn with_execution_authorization(
        mut self,
        authorization: crate::tool::authorization::ExecutionAuthorizationContext,
    ) -> Self {
        self.execution_authorization = Some(authorization);
        self
    }

    /// Append raw command text to a Windows cmd invocation without applying
    /// Windows argv quoting to shell syntax.
    pub fn with_raw_command(mut self, command: impl Into<OsString>) -> Self {
        self.raw_command = Some(command.into());
        self
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
    /// OS containment is in force. `backend` names the mechanism that
    /// applied it and `abi` is that backend's reported version (`None` for
    /// mechanisms without one). `guarantees`/`limits` carry the obtained
    /// facts so an audit record never reads stronger than reality.
    Enforced {
        backend: crate::security::sandbox::BackendId,
        abi: Option<u32>,
        guarantees: Vec<String>,
        limits: Vec<String>,
    },
    /// A debug-only native AppContainer exercise. This proves process identity
    /// and Job ownership for tests, but deliberately carries no filesystem
    /// enforcement claim and is unavailable in release builds.
    #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
    TestOnlyAppContainer {
        token_verified: bool,
        job_verified: bool,
        status: String,
    },
}

impl SandboxExecutionOutcome {
    /// True only when OS filesystem containment was actually applied.
    pub fn is_contained(&self) -> bool {
        matches!(self, Self::Enforced { .. })
    }

    /// Audit/authorization token distinguishing contained from disabled.
    pub fn audit_token(&self) -> &'static str {
        match self {
            Self::Disabled => "sandbox_disabled",
            Self::Enforced { .. } => "sandbox_enforced",
            #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
            Self::TestOnlyAppContainer { .. } => "sandbox_test_only_appcontainer",
        }
    }
}

/// Diagnostics from process-tree cleanup. Errors are retained on the result
/// so a target exit is never confused with cleanup failure.
#[derive(Debug, Clone, Default)]
pub struct CleanupDiagnostics {
    pub process_group_established: bool,
    pub job_object_established: bool,
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
    if request
        .execution_authorization
        .as_ref()
        .is_some_and(|authorization| {
            authorization
                .snapshot
                .sandbox_profile()
                .requires_filesystem_containment()
                && matches!(&request.sandbox, SandboxRequest::Disabled)
        })
    {
        return Err(ManagedProcessError::SandboxFailed(
            "accepted execution policy requires filesystem containment, but the launch request disabled it".into(),
        ));
    }

    #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
    if matches!(&request.sandbox, SandboxRequest::Required(_)) {
        return run_windows_test_restricted(request, output_tx).await;
    }

    let ManagedProcessRequest {
        executable,
        argv,
        raw_command,
        cwd,
        mut environment_policy,
        stdin,
        timeout: process_timeout,
        cancellation,
        output_policy,
        sandbox,
        provenance,
        execution_authorization,
    } = request;

    if executable.is_empty() {
        return Err(ManagedProcessError::EmptyArgv);
    }
    if executable.to_string_lossy().contains('\0') {
        return Err(ManagedProcessError::InvalidArgument(
            "executable".to_string(),
        ));
    }
    #[cfg(windows)]
    if let Some(raw_command) = raw_command.as_deref() {
        if raw_command.to_string_lossy().contains('\0') {
            return Err(ManagedProcessError::InvalidArgument(
                "raw command".to_string(),
            ));
        }
        let executable_path = Path::new(&executable);
        if !executable_path.is_absolute()
            || !executable_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
        {
            return Err(ManagedProcessError::InvalidArgument(
                "raw command requires an absolute cmd.exe executable".to_string(),
            ));
        }
    }
    #[cfg(not(windows))]
    if raw_command.is_some() {
        return Err(ManagedProcessError::InvalidArgument(
            "raw command is only supported for Windows cmd.exe".to_string(),
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

    // Session identity is stamped by the model tool execution context. The
    // child environment is rebuilt, then these trusted values are applied by
    // the canonical process owner so ambient TMP overrides cannot win.
    let scratch_scope = execution_authorization
        .as_ref()
        .and_then(|authorization| authorization.scratch_scope.as_ref());
    let session_scratch = if let Some(scope) = scratch_scope {
        Some(scope.private_root.clone())
    } else {
        provenance
            .session_id
            .as_deref()
            .map(crate::scratch::session_scratch_dir)
            .transpose()
            .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?
    };
    if let Some(path) = session_scratch.as_ref() {
        environment_policy = environment_policy
            .with_var("CODEGG_SCRATCH_DIR", path.as_os_str())
            .with_var("TMPDIR", path.as_os_str())
            .with_var("TMP", path.as_os_str())
            .with_var("TEMP", path.as_os_str());
    }
    if let Some(scope) = scratch_scope {
        for shared in &scope.shared {
            let key = format!(
                "CODEGG_SHARED_SCRATCH_{}",
                shared.alias.to_ascii_uppercase().replace('-', "_")
            );
            environment_policy = environment_policy.with_var(key, shared.path.as_os_str());
        }
    }

    // Every constrained launch receives a collision-resistant 0700 staging
    // directory that remains owned by this future until the process tree is
    // reaped. Model sessions additionally receive their durable private root
    // above; both roots are explicit in the Required launch spec.
    let mut sandbox_scratch = None;
    let mut sandbox = sandbox;
    if let SandboxRequest::Required(spec) = &mut sandbox {
        crate::security::sandbox::policy::validate_denied_paths(&spec.deny_paths)
            .map_err(ManagedProcessError::SandboxFailed)?;
        let scratch = tempfile::Builder::new()
            .prefix("codegg-run-")
            .tempdir()
            .map_err(|error| {
                ManagedProcessError::SandboxFailed(format!("create private scratch: {error}"))
            })?;
        #[cfg(unix)]
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).map_err(
            |error| ManagedProcessError::SandboxFailed(format!("secure private scratch: {error}")),
        )?;
        let scratch_path = std::fs::canonicalize(scratch.path()).map_err(|error| {
            ManagedProcessError::SandboxFailed(format!("resolve private scratch: {error}"))
        })?;
        spec.read_paths.push(scratch_path.clone());
        spec.write_paths.push(scratch_path.clone());
        if let Some(session_scratch) = session_scratch.as_ref() {
            spec.read_paths.push(session_scratch.clone());
            spec.write_paths.push(session_scratch.clone());
        } else {
            environment_policy = environment_policy
                .with_var("CODEGG_SCRATCH_DIR", scratch_path.as_os_str())
                .with_var("TMPDIR", scratch_path.as_os_str())
                .with_var("TMP", scratch_path.as_os_str())
                .with_var("TEMP", scratch_path.as_os_str());
        }
        if let Some(scope) = scratch_scope {
            for shared in &scope.shared {
                spec.read_paths.push(shared.path.clone());
                spec.write_paths.push(shared.path.clone());
            }
        }
        sandbox_scratch = Some(scratch);
    }

    let (
        launch_argv,
        _sandbox_file,
        status_reader,
        status_writer,
        status_ack_reader,
        status_ack_writer,
    ) = prepare_launch_argv(
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
    #[cfg(windows)]
    if let Some(raw_command) = raw_command.as_deref() {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().raw_arg(raw_command);
    }
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
        configure_status_fds(
            &mut command,
            writer.as_raw_fd(),
            status_ack_reader.as_ref().map(AsRawFd::as_raw_fd),
        );
    }

    #[cfg(windows)]
    let mut process_job = Some(windows::WindowsJob::create().map_err(ManagedProcessError::Spawn)?);

    let mut child = command.spawn().map_err(ManagedProcessError::Spawn)?;
    drop(status_writer);
    drop(status_ack_reader);

    #[cfg(windows)]
    if let Err(error) = process_job
        .as_mut()
        .expect("Windows launches always own a Job Object")
        .assign_and_resume(&child)
    {
        // The target's primary thread is still suspended on setup failure.
        // Close the Job first (which kills an assigned tree), then request a
        // direct kill as well in case assignment itself failed. Child has
        // kill_on_drop enabled, so dropping its owned process handle remains
        // a final cleanup path if Windows reports a transient kill failure.
        drop(process_job.take());
        let _ = child.start_kill();
        drop(child);
        return Err(ManagedProcessError::Spawn(error));
    }

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
    let status_task = match (status_reader, status_ack_writer, &sandbox) {
        (Some(reader), Some(ack_writer), SandboxRequest::Required(spec))
            if spec.linux_protected_mount_view =>
        {
            Some(tokio::spawn(read_bubblewrap_status_with_ack(
                reader, ack_writer,
            )))
        }
        (Some(reader), _, _) => Some(tokio::spawn(read_status(reader))),
        (None, _, _) => None,
    };
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
        #[cfg(windows)]
        process_job.as_ref(),
    )
    .await?;
    // A root process may exit while descendants remain. Closing this
    // kill-on-close Job before joining pipes terminates them and releases
    // inherited writers.
    #[cfg(windows)]
    drop(process_job.take());
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
    let sandbox =
        interpret_sandbox_status_for_termination(sandbox_status.as_deref(), &sandbox, termination)?;
    drop(sandbox_scratch);

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

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
async fn run_windows_test_restricted(
    request: ManagedProcessRequest,
    output_tx: Option<mpsc::Sender<ManagedProcessOutputChunk>>,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    run_windows_test_restricted_inner(request, output_tx, false).await
}

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
#[cfg(test)]
async fn run_windows_test_restricted_forensic(
    request: ManagedProcessRequest,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    run_windows_test_restricted_inner(request, None, true).await
}

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
async fn run_windows_test_restricted_inner(
    request: ManagedProcessRequest,
    output_tx: Option<mpsc::Sender<ManagedProcessOutputChunk>>,
    forensic_populated_deny_roots: bool,
) -> Result<ManagedProcessResult, ManagedProcessError> {
    use std::os::windows::ffi::OsStrExt;

    // The AppContainer cannot reach runner-user TEMP without changing the
    // user's profile ACLs. C014 uses this reserved test-only path to request
    // a fresh projection under the package profile's own local TEMP instead.
    let profile_temp_projection = Path::new(r"C:\__codegg_test_profile_temp__");

    let ManagedProcessRequest {
        executable,
        argv,
        raw_command,
        cwd,
        environment_policy,
        stdin,
        timeout: process_timeout,
        cancellation,
        output_policy,
        sandbox,
        provenance,
        execution_authorization: _execution_authorization,
    } = request;
    if executable.is_empty() {
        return Err(ManagedProcessError::EmptyArgv);
    }
    if cancellation.is_cancelled() {
        return Err(ManagedProcessError::CancelledBeforeSpawn);
    }
    if raw_command.is_some()
        || executable.to_string_lossy().contains('\0')
        || argv.iter().any(|arg| arg.to_string_lossy().contains('\0'))
    {
        return Err(ManagedProcessError::InvalidArgument(
            "restricted AppContainer launch requires NUL-free argv without raw shell text".into(),
        ));
    }
    let SandboxRequest::Required(spec) = sandbox else {
        return Err(ManagedProcessError::SandboxFailed(
            "test-only restricted launcher requires an explicit sandbox specification".into(),
        ));
    };
    let encoded_spec = serde_json::to_vec(&spec)
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    if encoded_spec.len() > MAX_SANDBOX_SPEC_BYTES {
        return Err(ManagedProcessError::SandboxFailed(
            "restricted sandbox specification exceeds 64 KiB".into(),
        ));
    }
    let requested_args = spec
        .args
        .iter()
        .map(|value| OsString::from(value.as_str()))
        .collect::<Vec<_>>();
    if spec.target.as_os_str() != Path::new(&executable).as_os_str()
        || requested_args != argv
        || !spec.target.is_absolute()
        || !cwd.is_absolute()
        || cwd.as_os_str().encode_wide().any(|unit| unit == 0)
    {
        return Err(ManagedProcessError::SandboxFailed(
            "restricted launch executable, argv, and cwd must match the immutable request".into(),
        ));
    }

    let started = Instant::now();
    let mut acl_leases = Vec::new();
    let mut roots =
        std::collections::BTreeMap::<String, (PathBuf, windows_acl_lease::LeaseAccess)>::new();
    for (path, access) in spec
        .read_paths
        .iter()
        .map(|path| (path.clone(), windows_acl_lease::LeaseAccess::Read))
        .chain(
            spec.write_paths
                .iter()
                .map(|path| (path.clone(), windows_acl_lease::LeaseAccess::ReadWrite)),
        )
        .chain(
            spec.deny_paths
                .iter()
                .map(|path| (path.clone(), windows_acl_lease::LeaseAccess::Deny)),
        )
    {
        if path == profile_temp_projection {
            continue;
        }
        windows_acl_lease::validate_input_directory(&path).map_err(|error| {
            ManagedProcessError::SandboxFailed(format!(
                "ACL lease root is unavailable or unsupported: {error}"
            ))
        })?;
        if !forensic_populated_deny_roots {
            windows_acl_lease::validate_empty_lease_root(&path).map_err(|error| {
                ManagedProcessError::SandboxFailed(format!(
                    "ACL lease root is unavailable or populated: {error}"
                ))
            })?;
        }
        let canonical = std::fs::canonicalize(&path).map_err(|error| {
            ManagedProcessError::SandboxFailed(format!("ACL lease root is unavailable: {error}"))
        })?;
        let key = canonical.to_string_lossy().to_lowercase();
        roots
            .entry(key)
            .and_modify(|(_, current)| {
                if access == windows_acl_lease::LeaseAccess::Deny
                    || (*current != windows_acl_lease::LeaseAccess::Deny
                        && access == windows_acl_lease::LeaseAccess::ReadWrite)
                {
                    *current = access;
                }
            })
            .or_insert((canonical, access));
    }
    let daemon_paths = codegg_client::LocalDaemonPaths::resolve();
    daemon_paths.ensure_root().map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("prepare daemon-owned ACL journal root: {error}"),
        ))
    })?;
    #[cfg(test)]
    if forensic_populated_deny_roots {
        eprintln!("C013 stage: daemon journal root ready");
    }
    let journal_root = daemon_paths.root.join("sandbox").join("acl-leases");
    let conflicts = windows_acl_lease::recover_all(&journal_root).map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("recover Windows ACL lease journal: {error}"),
        ))
    })?;
    #[cfg(test)]
    if forensic_populated_deny_roots {
        eprintln!("C013 stage: existing ACL journals recovered");
    }
    if !conflicts.is_empty() {
        return Err(ManagedProcessError::SandboxFailed(format!(
            "{} stale Windows ACL lease journal(s) need operator inspection",
            conflicts.len()
        )));
    }
    let profile = windows::WindowsAppContainerProfile::create().map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("create restricted AppContainer profile: {error}"),
        ))
    })?;
    #[cfg(test)]
    if forensic_populated_deny_roots {
        eprintln!("C013 stage: AppContainer profile created");
    }
    let local_app_data = profile.local_app_data_path().map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("resolve AppContainer local application data: {error}"),
        ))
    })?;
    let temporary_directory = local_app_data.join("Temp");
    std::fs::create_dir_all(&temporary_directory).map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("create AppContainer temporary directory: {error}"),
        ))
    })?;
    if cwd == profile_temp_projection {
        windows_acl_lease::validate_empty_lease_root(&temporary_directory).map_err(|error| {
            ManagedProcessError::SandboxFailed(format!(
                "AppContainer profile projection root is unavailable or populated: {error}"
            ))
        })?;
        let canonical = std::fs::canonicalize(&temporary_directory).map_err(|error| {
            ManagedProcessError::SandboxFailed(format!(
                "canonicalize AppContainer profile projection root: {error}"
            ))
        })?;
        roots.insert(
            canonical.to_string_lossy().to_lowercase(),
            (canonical, windows_acl_lease::LeaseAccess::ReadWrite),
        );
    }
    let lease_run_id = format!("{}:{}", provenance.job_id, provenance.attempt_id);
    for (path, access) in roots.into_values() {
        acl_leases.push(
            windows_acl_lease::WindowsAclLease::acquire(
                &path,
                &journal_root,
                &lease_run_id,
                profile.sid(),
                access,
            )
            .map_err(|error| {
                ManagedProcessError::Spawn(std::io::Error::new(
                    error.kind(),
                    format!(
                        "acquire {access:?} Windows ACL lease for {}: {error}",
                        path.display()
                    ),
                ))
            })?,
        );
        #[cfg(test)]
        if forensic_populated_deny_roots {
            eprintln!(
                "C013 stage: {:?} ACL lease applied to {}",
                access,
                path.display()
            );
        }
    }
    #[cfg(test)]
    if forensic_populated_deny_roots {
        for path in spec
            .write_paths
            .iter()
            .chain(spec.deny_paths.iter())
            .take(8)
        {
            if spec.deny_paths.contains(path) {
                let child_created_after_lease = path.join("created-after-lease.txt");
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&child_created_after_lease)
                    .and_then(|mut file| {
                        use std::io::Write;
                        file.write_all(b"synthetic-c013-created-after-lease")
                    })
                    .map_err(ManagedProcessError::Spawn)?;
            }
            windows_acl_lease::forensic_tree_snapshots(path, 64)
                .map_err(ManagedProcessError::Spawn)?
                .into_iter()
                .for_each(|snapshot| eprintln!("C013 post-lease object: {snapshot}"));
        }
        eprintln!("C013 stage: post-lease object snapshots complete");
    }
    #[cfg(test)]
    if forensic_populated_deny_roots {
        eprintln!("C013 stage: AppContainer temp directory ready");
    }
    let mut environment = environment_policy.entries();
    provenance.apply_environment(&mut environment);
    if cwd == profile_temp_projection {
        environment.insert(
            "CODEGG_C014_PROJECTION".into(),
            temporary_directory.as_os_str().to_owned(),
        );
    }
    let launch_cwd = if cwd == profile_temp_projection {
        // Keep the shell's startup directory on the already-readable OS
        // runtime. C014 addresses the granted projection by its explicit
        // path; C016 separately qualifies a staged projection as cwd.
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32")
    } else {
        cwd.clone()
    };
    let mut environment_block = windows_environment_block(
        environment,
        &launch_cwd,
        &local_app_data,
        &temporary_directory,
    )?;
    let mut child = windows::WindowsTestRestrictedProcess::launch(
        &spec.target,
        &requested_args,
        &profile,
        &launch_cwd,
        &mut environment_block,
        &cancellation,
    )
    .map_err(|error| {
        ManagedProcessError::Spawn(std::io::Error::new(
            error.kind(),
            format!("launch restricted AppContainer child: {error}"),
        ))
    })?;
    #[cfg(test)]
    if forensic_populated_deny_roots {
        eprintln!("C013 stage: restricted child created");
    }
    let job_verified = child.job_established();
    if !job_verified {
        let _ = child.terminate(1);
        let _ = child.wait().await;
        child.close_job();
        return Err(ManagedProcessError::Spawn(io::Error::other(
            "restricted child Job membership verification failed",
        )));
    }

    // The status pipe is private to the service. The target cannot forge its
    // contents through stdout/stderr, and this fixture reports SetupError so
    // the test-only ACL lease proof cannot be mistaken for production Enforced.
    let status_text =
        "debug-only AppContainer token, Job, and disposable ACL leases verified; production filesystem rights remain unqualified";
    let status_bytes =
        crate::security::sandbox::encode_sandbox_status(SandboxLaunchOutcome::SetupError {
            reason: status_text.to_string(),
        })
        .map_err(ManagedProcessError::SandboxFailed)?;
    if status_bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err(ManagedProcessError::SandboxFailed(
            "restricted launch status exceeded its private channel bound".into(),
        ));
    }
    let (status_reader, mut status_writer) = windows::create_test_status_pipe()
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    status_writer
        .write_all(&status_bytes)
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    drop(status_writer);

    let (overflow_tx, mut overflow_rx) = mpsc::channel(2);
    let stdout_task = tokio::spawn(read_bounded(
        child.stdout.take().expect("stdout pipe is present"),
        output_policy.stdout_limit,
        OutputStream::Stdout,
        overflow_tx.clone(),
        output_policy.overflow,
        output_tx.clone(),
    ));
    let stderr_task = tokio::spawn(read_bounded(
        child.stderr.take().expect("stderr pipe is present"),
        output_policy.stderr_limit,
        OutputStream::Stderr,
        overflow_tx,
        output_policy.overflow,
        output_tx,
    ));
    let status_task = tokio::spawn(read_status(status_reader));
    let stdin_task = match stdin {
        StdinPolicy::Null => {
            child.stdin.take();
            None
        }
        StdinPolicy::Bytes(bytes) => child.stdin.take().map(|mut writer| {
            tokio::spawn(async move {
                let result = writer.write_all(&bytes).await;
                drop(writer);
                result
            })
        }),
    };
    let (exit_status, termination, mut cleanup) = wait_for_test_restricted_child(
        &child,
        &cancellation,
        process_timeout,
        &mut overflow_rx,
        output_policy.overflow,
    )
    .await?;
    cleanup.job_object_established = true;
    child.close_job();
    drop(child.stdin.take());

    let stdout = join_output(stdout_task).await?;
    let stderr = join_output(stderr_task).await?;
    if let Some(task) = stdin_task {
        if let Err(error) = tokio::time::timeout(TERMINATION_GRACE, task).await {
            tracing::debug!(
                "restricted stdin task did not finish in grace period: {}",
                error
            );
        }
    }
    let status = tokio::time::timeout(STATUS_READ_TIMEOUT, status_task)
        .await
        .map_err(|_| {
            ManagedProcessError::SandboxFailed("private status channel did not close".into())
        })?
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    let status = decode_sandbox_status(&status).map_err(ManagedProcessError::SandboxFailed)?;
    let SandboxLaunchOutcome::SetupError { reason } = status else {
        return Err(ManagedProcessError::SandboxFailed(
            "test-only status channel reported an unexpected enforcement outcome".into(),
        ));
    };
    for lease in acl_leases.into_iter().rev() {
        lease
            .release()
            .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    }
    drop(profile);

    Ok(ManagedProcessResult {
        exit_status,
        stdout,
        stderr,
        duration: started.elapsed(),
        termination,
        sandbox: SandboxExecutionOutcome::TestOnlyAppContainer {
            token_verified: true,
            job_verified,
            status: reason,
        },
        cleanup,
    })
}

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
fn windows_environment_block(
    environment: BTreeMap<OsString, OsString>,
    cwd: &Path,
    local_app_data: &Path,
    temporary_directory: &Path,
) -> Result<Vec<u16>, ManagedProcessError> {
    use std::os::windows::ffi::OsStrExt;
    let mut environment = environment;
    // AppContainer process creation needs the package-local profile paths to
    // build its isolated environment. Never pass the host user's profile or
    // temporary directories through this restricted launch path.
    environment.insert("LOCALAPPDATA".into(), local_app_data.as_os_str().to_owned());
    environment.insert("TEMP".into(), temporary_directory.as_os_str().to_owned());
    environment.insert("TMP".into(), temporary_directory.as_os_str().to_owned());
    let mut environment = environment.into_iter().collect::<Vec<_>>();
    environment.sort_by_key(|(name, _)| name.to_string_lossy().to_ascii_uppercase());
    let mut block = Vec::new();

    // A supplied Windows environment block does not inherit the special
    // per-drive current-directory entries from the parent. Preserve those
    // entries because CreateProcessW and child libraries use them when
    // resolving drive-relative paths. Override the requested working drive
    // so it agrees with lpCurrentDirectory.
    let mut drive_directories = windows::inherited_drive_directories()
        .map_err(|error| ManagedProcessError::SandboxFailed(error.to_string()))?;
    let cwd_wide = cwd.as_os_str().encode_wide().collect::<Vec<_>>();
    let drive = cwd_wide.first().copied();
    if cwd_wide.len() >= 3
        && cwd_wide[1] == b':' as u16
        && (cwd_wide[2] == b'\\' as u16 || cwd_wide[2] == b'/' as u16)
        && drive.is_some_and(|drive| {
            (b'A' as u16..=b'Z' as u16).contains(&drive)
                || (b'a' as u16..=b'z' as u16).contains(&drive)
        })
    {
        let drive = drive.expect("validated drive letter");
        let drive = if (b'a' as u16..=b'z' as u16).contains(&drive) {
            drive - (b'a' - b'A') as u16
        } else {
            drive
        };
        let mut name = vec![b'=' as u16, drive, b':' as u16];
        drive_directories.retain(|(existing, _)| existing != &name);
        name.push(b'=' as u16);
        drive_directories.push((name, cwd_wide));
    }
    drive_directories.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (name, value) in drive_directories {
        block.extend(name);
        block.extend(value);
        block.push(0);
    }

    for (name, value) in environment {
        if name
            .to_string_lossy()
            .chars()
            .any(|c| c == '=' || c == '\0')
            || value.to_string_lossy().contains('\0')
        {
            return Err(ManagedProcessError::SandboxFailed(
                "restricted launch environment contains an invalid name or value".into(),
            ));
        }
        block.extend(name.encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

#[cfg(all(windows, feature = "windows-sandbox-test-support"))]
async fn wait_for_test_restricted_child(
    child: &windows::WindowsTestRestrictedProcess,
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
        status = child.wait() => Ok((status.map_err(ManagedProcessError::Wait)?, TerminationReason::Exited, CleanupDiagnostics::default())),
        _ = cancellation.cancelled() => {
            child.terminate(1).map_err(ManagedProcessError::Wait)?;
            let status = child.wait().await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::Cancelled, CleanupDiagnostics { job_object_established: true, forced_signal_sent: true, ..CleanupDiagnostics::default() }))
        }
        _ = &mut timeout_future => {
            child.terminate(1).map_err(ManagedProcessError::Wait)?;
            let status = child.wait().await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::TimedOut, CleanupDiagnostics { job_object_established: true, forced_signal_sent: true, ..CleanupDiagnostics::default() }))
        }
        stream = overflow_rx.recv(), if overflow_policy == OverflowPolicy::Terminate => {
            let stream = stream.unwrap_or(OutputStream::Stdout);
            child.terminate(1).map_err(ManagedProcessError::Wait)?;
            let status = child.wait().await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::OutputLimitExceeded { stream }, CleanupDiagnostics { job_object_established: true, forced_signal_sent: true, ..CleanupDiagnostics::default() }))
        }
    }
}

type PreparedLaunchArgv = (
    Vec<OsString>,
    Option<tempfile::NamedTempFile>,
    Option<tokio::fs::File>,
    Option<SandboxStatusWriter>,
    Option<SandboxStatusWriter>,
    Option<SandboxStatusWriter>,
);

fn prepare_launch_argv(
    executable: &OsString,
    argv: &[OsString],
    _cwd: &Path,
    sandbox: &SandboxRequest,
    #[cfg(test)] helper_override: Option<&Path>,
) -> Result<PreparedLaunchArgv, ManagedProcessError> {
    let full_argv = std::iter::once(executable.clone())
        .chain(argv.iter().cloned())
        .collect::<Vec<_>>();
    match sandbox {
        SandboxRequest::Disabled => Ok((full_argv, None, None, None, None, None)),
        SandboxRequest::Required(spec) => {
            #[cfg(not(unix))]
            {
                let _ = spec;
                return Err(ManagedProcessError::SandboxFailed(
                    "sandbox helper is unavailable on this platform".to_string(),
                ));
            }
            #[cfg(all(unix, not(target_os = "linux")))]
            if spec.linux_protected_mount_view {
                return Err(ManagedProcessError::SandboxFailed(
                    "protected mount view is available only on Linux".into(),
                ));
            }
            #[cfg(unix)]
            {
                #[cfg(target_os = "linux")]
                {
                    use crate::security::sandbox::linux_launch::{
                        LinuxLaunchAdapter, LinuxLaunchPlan, LinuxLaunchRequest,
                        ProductionLinuxLaunchAdapter,
                    };
                    let launch_request = LinuxLaunchRequest::legacy_filesystem(spec);
                    match ProductionLinuxLaunchAdapter
                        .plan(&launch_request)
                        .map_err(|reason| {
                            ManagedProcessError::SandboxFailed(format!(
                                "invalid Linux launch request: {reason}"
                            ))
                        })? {
                        LinuxLaunchPlan::LegacyPrivateHelper
                        | LinuxLaunchPlan::ProtectedMountView => {}
                        LinuxLaunchPlan::EnhancedUnavailable { dimensions } => {
                            return Err(ManagedProcessError::SandboxFailed(format!(
                                "Linux launch dimensions unavailable: {}",
                                dimensions.join(", ")
                            )));
                        }
                    }
                }
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
                let (status_reader, status_writer, status_ack_reader, status_ack_writer) =
                    create_status_pipe(spec.linux_protected_mount_view)
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
                    status_ack_reader,
                    status_ack_writer,
                ))
            }
        }
    }
}

const STATUS_READ_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(unix)]
#[allow(unsafe_code)]
fn create_status_pipe(
    with_ack: bool,
) -> io::Result<(tokio::fs::File, OwnedFd, Option<OwnedFd>, Option<OwnedFd>)> {
    fn make_pipe() -> io::Result<[OwnedFd; 2]> {
        let mut fds = [0_i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        for fd in fds {
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                let error = io::Error::last_os_error();
                unsafe {
                    libc::close(fds[0]);
                    libc::close(fds[1]);
                }
                return Err(error);
            }
        }
        Ok(unsafe { [OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])] })
    }

    let [reader, writer] = make_pipe()?;
    let (ack_reader, ack_writer) = if with_ack {
        let [reader, writer] = make_pipe()?;
        (Some(reader), Some(writer))
    } else {
        (None, None)
    };
    Ok((
        tokio::fs::File::from_std(std::fs::File::from(reader)),
        writer,
        ack_reader,
        ack_writer,
    ))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn configure_status_fds(command: &mut Command, source_status_fd: i32, source_ack_fd: Option<i32>) {
    unsafe {
        command.pre_exec(move || {
            // Duplicate to temporary descriptors first so source fds 3 or 4
            // cannot be clobbered by the fixed descriptor assignment.
            let status_copy = libc::fcntl(source_status_fd, libc::F_DUPFD_CLOEXEC, 10);
            if status_copy < 0 {
                return Err(io::Error::last_os_error());
            }
            let ack_copy = if let Some(source_ack_fd) = source_ack_fd {
                let fd = libc::fcntl(source_ack_fd, libc::F_DUPFD_CLOEXEC, 10);
                if fd < 0 {
                    libc::close(status_copy);
                    return Err(io::Error::last_os_error());
                }
                Some(fd)
            } else {
                None
            };
            if libc::dup2(status_copy, crate::security::sandbox::SANDBOX_STATUS_FD) < 0 {
                libc::close(status_copy);
                if let Some(fd) = ack_copy {
                    libc::close(fd);
                }
                return Err(io::Error::last_os_error());
            }
            libc::close(status_copy);
            if let Some(fd) = ack_copy {
                if libc::dup2(fd, crate::security::sandbox::SANDBOX_ACK_FD) < 0 {
                    libc::close(fd);
                    return Err(io::Error::last_os_error());
                }
                libc::close(fd);
                if libc::fcntl(crate::security::sandbox::SANDBOX_ACK_FD, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
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

#[cfg(unix)]
async fn read_bubblewrap_status_with_ack(
    mut reader: tokio::fs::File,
    ack_writer: OwnedFd,
) -> Result<Vec<u8>, io::Error> {
    let mut bytes = Vec::new();
    let mut first = [0_u8; 1];
    if reader.read(&mut first).await? == 0 {
        drop(ack_writer);
        return Ok(bytes);
    }
    bytes.push(first[0]);

    let mut ack_error = None;
    if first[0] == b'{' {
        while bytes.last() != Some(&b'\n') {
            if bytes.len() >= MAX_SANDBOX_STATUS_BYTES {
                drop(ack_writer);
                return Ok(bytes);
            }
            let mut byte = [0_u8; 1];
            if reader.read(&mut byte).await? == 0 {
                drop(ack_writer);
                return Ok(bytes);
            }
            bytes.push(byte[0]);
        }
        let ready = decode_bubblewrap_json_status(&bytes).is_ok_and(|_| {
            bytes
                .windows(b"\"child-pid\"".len())
                .any(|window| window == b"\"child-pid\"")
        });
        if ready {
            let mut writer = tokio::fs::File::from_std(std::fs::File::from(ack_writer));
            if let Err(error) = writer.write_all(&[0xA5]).await {
                ack_error = Some(error);
            } else if let Err(error) = writer.flush().await {
                ack_error = Some(error);
            }
        }
    } else {
        drop(ack_writer);
    }

    let remaining = (MAX_SANDBOX_STATUS_BYTES + 1).saturating_sub(bytes.len());
    let mut tail = Vec::new();
    reader.take(remaining as u64).read_to_end(&mut tail).await?;
    bytes.extend_from_slice(&tail);
    if bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sandbox status stream exceeds 16 KiB",
        ));
    }
    if let Some(error) = ack_error {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "Bubblewrap setup ACK failed: {error}; status stream: {}",
                String::from_utf8_lossy(&bytes)
            ),
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
    // The child has already exited, so its pipes are at EOF and this join
    // completes immediately — unless a surviving descendant inherited the
    // write end and is still holding it open. That is reachable: the group
    // kill covers processes that stayed in the session, but a descendant that
    // called `setsid` escapes it and can keep the pipe open indefinitely.
    // An unbounded join there hangs the caller forever, which surfaced as a
    // tool stuck on "waiting for command/tool output..." with no error at all.
    // Bound the join and abort the reader instead; whatever the reader already
    // pushed onto the bounded output channel has been delivered to the consumer.
    match tokio::time::timeout(TERMINATION_GRACE, task).await {
        Ok(Ok(Ok(output))) => Ok(output),
        Ok(Ok(Err(error))) => Err(ManagedProcessError::ReadOutput(error)),
        Ok(Err(error)) => Err(ManagedProcessError::OutputReaderTask(error.to_string())),
        Err(_) => Err(ManagedProcessError::ReadOutput(io::Error::other(
            "output reader did not reach EOF after the process exited \
             (a surviving descendant still holds the pipe open)",
        ))),
    }
}

async fn wait_for_child(
    child: &mut Child,
    cancellation: &CancellationToken,
    process_timeout: Option<Duration>,
    overflow_rx: &mut mpsc::Receiver<OutputStream>,
    overflow_policy: OverflowPolicy,
    #[cfg(windows)] process_job: Option<&windows::WindowsJob>,
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
                .map(|status| {
                    // The reader truncates on its first chunk and queues the
                    // overflow notice, so an overflow and the child's exit
                    // routinely become ready in the same poll. `select!` picks
                    // randomly when that happens, and the loser branch is
                    // dropped, so `terminate_on_overflow` could report the
                    // exit instead of the overflow that policy says to act on.
                    // Drain the notice here so the queued signal always wins
                    // over the coincident exit.
                    let overflow = match overflow_policy {
                        OverflowPolicy::Terminate => overflow_rx.try_recv().ok(),
                        OverflowPolicy::ContinueDrain => None,
                    };
                    let cleanup = CleanupDiagnostics::default();
                    #[cfg(windows)]
                    let cleanup = {
                        let mut cleanup = cleanup;
                        cleanup.job_object_established = process_job.is_some();
                        cleanup
                    };
                    let termination = match overflow {
                        Some(stream) => TerminationReason::OutputLimitExceeded { stream },
                        None => TerminationReason::Exited,
                    };
                    (status, termination, cleanup)
                })
                .map_err(ManagedProcessError::Wait)
        }
        _ = cancellation.cancelled() => {
            let (status, cleanup) = terminate_child(
                child,
                #[cfg(windows)]
                process_job,
            ).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::Cancelled, cleanup))
        }
        _ = &mut timeout_future => {
            let (status, cleanup) = terminate_child(
                child,
                #[cfg(windows)]
                process_job,
            ).await.map_err(ManagedProcessError::Wait)?;
            Ok((status, TerminationReason::TimedOut, cleanup))
        }
        stream = overflow_rx.recv(), if overflow_policy == OverflowPolicy::Terminate => {
            let stream = stream.unwrap_or(OutputStream::Stdout);
            let (status, cleanup) = terminate_child(
                child,
                #[cfg(windows)]
                process_job,
            ).await.map_err(ManagedProcessError::Wait)?;
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
fn configure_process_session(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command
            .as_std_mut()
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    #[cfg(not(windows))]
    let _ = command;
}

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
async fn terminate_child(
    child: &mut Child,
    #[cfg(windows)] process_job: Option<&windows::WindowsJob>,
) -> io::Result<(ExitStatus, CleanupDiagnostics)> {
    #[cfg(windows)]
    let cleanup = if let Some(job) = process_job {
        job.terminate(1)?;
        CleanupDiagnostics {
            job_object_established: true,
            forced_signal_sent: true,
            ..CleanupDiagnostics::default()
        }
    } else {
        child.kill().await?;
        CleanupDiagnostics::default()
    };
    #[cfg(not(windows))]
    let cleanup = {
        child.kill().await?;
        CleanupDiagnostics::default()
    };
    child.wait().await.map(|status| (status, cleanup))
}

#[cfg(test)]
fn interpret_sandbox_status(
    status_bytes: Option<&[u8]>,
    sandbox: &SandboxRequest,
) -> Result<SandboxExecutionOutcome, ManagedProcessError> {
    interpret_sandbox_status_for_termination(status_bytes, sandbox, TerminationReason::Exited)
}

fn interpret_sandbox_status_for_termination(
    status_bytes: Option<&[u8]>,
    sandbox: &SandboxRequest,
    termination: TerminationReason,
) -> Result<SandboxExecutionOutcome, ManagedProcessError> {
    if matches!(sandbox, SandboxRequest::Disabled) {
        return Ok(SandboxExecutionOutcome::Disabled);
    }
    let status = status_bytes.ok_or_else(|| {
        ManagedProcessError::SandboxFailed("sandbox status channel was not created".to_string())
    })?;
    #[cfg(target_os = "linux")]
    if let SandboxRequest::Required(spec) = sandbox {
        if spec.linux_protected_mount_view {
            return interpret_bubblewrap_status(status, spec, termination);
        }
    }
    let decoded = decode_sandbox_status(status).map_err(ManagedProcessError::SandboxFailed)?;
    #[cfg(target_os = "linux")]
    if let SandboxRequest::Required(spec) = sandbox {
        let request =
            crate::security::sandbox::linux_launch::LinuxLaunchRequest::legacy_filesystem(spec);
        crate::security::sandbox::linux_launch::observe_legacy_outcome(&request, &decoded)
            .map_err(ManagedProcessError::SandboxFailed)?;
    }
    match decoded {
        SandboxLaunchOutcome::Enforced {
            backend,
            abi,
            guarantees,
            limits,
        } => Ok(SandboxExecutionOutcome::Enforced {
            backend,
            abi,
            guarantees,
            limits,
        }),
        // A helper that reports Unavailable under a `Required` request is a
        // genuine containment loss between decision and launch: stay
        // fail-closed rather than falling through to the degraded path.
        SandboxLaunchOutcome::Unavailable { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox unavailable: {reason}"),
        )),
        SandboxLaunchOutcome::SetupError { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox setup failed: {reason}"),
        )),
        SandboxLaunchOutcome::ExecError { reason } => Err(ManagedProcessError::SandboxFailed(
            format!("sandbox target exec failed: {reason}"),
        )),
        SandboxLaunchOutcome::BubblewrapReady => Err(ManagedProcessError::SandboxFailed(
            "legacy sandbox helper reported an unexpected Bubblewrap setup receipt".into(),
        )),
    }
}

#[cfg(target_os = "linux")]
fn interpret_bubblewrap_status(
    status: &[u8],
    spec: &SandboxLaunchSpec,
    termination: TerminationReason,
) -> Result<SandboxExecutionOutcome, ManagedProcessError> {
    if status
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'{')
    {
        let has_exit_status =
            decode_bubblewrap_json_status(status).map_err(ManagedProcessError::SandboxFailed)?;
        if !has_exit_status && termination == TerminationReason::Exited {
            return Err(ManagedProcessError::SandboxFailed(
                "Bubblewrap exited without confirming successful target execution".into(),
            ));
        }
    } else {
        match decode_sandbox_status(status).map_err(ManagedProcessError::SandboxFailed)? {
            SandboxLaunchOutcome::Unavailable { reason }
            | SandboxLaunchOutcome::SetupError { reason } => {
                return Err(ManagedProcessError::SandboxFailed(format!(
                    "Bubblewrap setup failed: {reason}"
                )));
            }
            outcome => {
                return Err(ManagedProcessError::SandboxFailed(format!(
                    "Bubblewrap helper returned an unexpected setup outcome: {outcome:?}"
                )));
            }
        }
    }

    let mut guarantees = crate::security::sandbox::linux_mount_view::enforcement_scope(spec)
        .map_err(ManagedProcessError::SandboxFailed)?;
    guarantees.insert(
        0,
        "Bubblewrap block-fd barrier releases the target only after mount setup".into(),
    );
    Ok(SandboxExecutionOutcome::Enforced {
        backend: crate::security::sandbox::BackendId::BUBBLEWRAP,
        abi: None,
        guarantees,
        limits: vec![
            "network isolation not requested".into(),
            "PID/IPC isolation and seccomp are not claimed by this launch".into(),
            "resource limits are not claimed by this launch".into(),
        ],
    })
}

#[cfg(target_os = "linux")]
fn decode_bubblewrap_json_status(bytes: &[u8]) -> Result<bool, String> {
    if bytes.is_empty() {
        return Err("Bubblewrap produced no JSON status".into());
    }
    if bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err("Bubblewrap status stream exceeds 16 KiB".into());
    }

    let mut child_pid_seen = false;
    let mut exit_code_seen = false;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value = serde_json::from_slice(line)
            .map_err(|error| format!("Bubblewrap status JSON is malformed: {error}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| "Bubblewrap status record is not a JSON object".to_string())?;
        if let Some(pid) = object.get("child-pid") {
            if child_pid_seen || exit_code_seen || pid.as_u64().is_none_or(|pid| pid == 0) {
                return Err("Bubblewrap child PID status is invalid or duplicated".into());
            }
            child_pid_seen = true;
        }
        if let Some(exit_code) = object.get("exit-code") {
            if !child_pid_seen || exit_code_seen || exit_code.as_i64().is_none() {
                return Err("Bubblewrap exit status is invalid or out of order".into());
            }
            exit_code_seen = true;
        }
    }
    if !child_pid_seen {
        return Err("Bubblewrap did not report its sandbox child".into());
    }
    Ok(exit_code_seen)
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
    #[tokio::test(flavor = "current_thread")]
    async fn accepted_constrained_context_refuses_disabled_process_sandbox_before_spawn() {
        let root = tempfile::tempdir().expect("process marker directory");
        let marker = root.path().join("must-not-exist");
        let mut process = request(&format!("touch {}", marker.display()));
        process.execution_authorization =
            Some(crate::tool::authorization::ExecutionAuthorizationContext {
                snapshot: codegg_core::approval::ExecutionPolicySnapshot::capture(
                    codegg_core::approval::ApprovalMode::Yolo,
                    codegg_core::approval::SandboxProfile::WorkspaceWrite,
                    None,
                    Some("session".into()),
                    Some("agent".into()),
                    Some("policy".into()),
                    None,
                ),
                receipt: crate::permission::PermissionDecisionReceipt::allowed(
                    "yolo",
                    Some("policy".into()),
                ),
                command_preflight: None,
                scratch_scope: None,
            });

        let error = run(process)
            .await
            .expect_err("constrained authorization cannot use a disabled sandbox");
        assert!(matches!(error, ManagedProcessError::SandboxFailed(_)));
        assert!(!marker.exists());
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
    #[tokio::test(flavor = "current_thread")]
    async fn delegated_scratch_keeps_temp_private_and_exposes_only_granted_aliases() {
        let run_id = codegg_core::identity::AgentRunId::new();
        let session_id = format!("delegated-test-{run_id}");
        let private_root = crate::scratch::agent_run_scratch_dir(&session_id, &run_id)
            .expect("run private scratch");
        let scratch_id = codegg_core::identity::ScratchId::new();
        let shared_root =
            crate::scratch::shared_scratch_dir(&session_id, &scratch_id).expect("shared scratch");
        let scope = crate::scratch::AgentScratchScope {
            session_id,
            run_id,
            private_root: private_root.clone(),
            shared: vec![crate::scratch::AuthorizedSharedScratchRoot {
                scratch_id,
                alias: "handoff".into(),
                path: shared_root.clone(),
            }],
        };
        let script = "test \"$TMPDIR\" = \"$CODEGG_SCRATCH_DIR\" && test \"$TMPDIR\" = \"$EXPECTED_PRIVATE\" && printf handed-off > \"$CODEGG_SHARED_SCRATCH_HANDOFF/item\"";
        let mut process = request(script);
        process.environment_policy =
            EnvironmentPolicy::sanitized().with_var("EXPECTED_PRIVATE", private_root.as_os_str());
        process.execution_authorization =
            Some(crate::tool::authorization::ExecutionAuthorizationContext {
                snapshot: codegg_core::approval::ExecutionPolicySnapshot::capture(
                    codegg_core::approval::ApprovalMode::Interactive,
                    codegg_core::approval::SandboxProfile::FullHost,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                receipt: crate::permission::PermissionDecisionReceipt::allowed("test", None),
                command_preflight: None,
                scratch_scope: Some(scope),
            });
        let result = run(process).await.expect("managed child process succeeds");
        assert!(
            result.exit_status.success(),
            "{}",
            result.stderr.to_string_lossy()
        );
        assert_eq!(
            std::fs::read_to_string(shared_root.join("item")).unwrap(),
            "handed-off"
        );
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
        std::fs::write(
            &helper,
            b"#!/bin/sh\nprintf 'CODEGG_SANDBOX_ENFORCED abi=9\\n' >&2\nexit 0\n",
        )
        .expect("fake helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("fake helper executable permissions");
        let marker = directory.path().join("target-ran");
        let mut request = request("true");
        request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
            linux_protected_mount_view: false,
            protected_readonly_paths: Vec::new(),
            path_identities: Vec::new(),
            deny_paths: Vec::new(),
            target: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), format!("touch {}", marker.display())],
            read_paths: Vec::new(),
            write_paths: Vec::new(),
        });

        let error = run_inner(request, None, Some(&helper))
            .await
            .expect_err("missing status must fail closed");
        assert!(
            matches!(error, ManagedProcessError::SandboxFailed(reason) if reason.contains("status"))
        );
        assert!(
            !marker.exists(),
            "target must not run without helper status"
        );
    }
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn hardlinked_denied_path_fails_before_target_launch() {
        let directory = tempfile::tempdir().expect("hardlink fixture directory");
        let denied = directory.path().join("credential");
        let alias = directory.path().join("workspace-alias");
        std::fs::write(&denied, "secret").expect("credential fixture");
        std::fs::hard_link(&denied, &alias).expect("credential hardlink alias");
        let marker = directory.path().join("target-ran");
        let mut request = request("true");
        request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
            linux_protected_mount_view: false,
            protected_readonly_paths: Vec::new(),
            path_identities: Vec::new(),
            target: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), format!("touch {}", marker.display())],
            read_paths: vec![directory.path().to_path_buf()],
            write_paths: vec![directory.path().to_path_buf()],
            deny_paths: vec![denied],
        });

        let error = run(request)
            .await
            .expect_err("path-based policy cannot safely exclude a hardlinked file");
        assert!(
            matches!(error, ManagedProcessError::SandboxFailed(ref reason) if reason.contains("hardlinked denied file")),
            "unexpected hardlink policy result: {error:?}"
        );
        assert!(
            !marker.exists(),
            "target ran despite hardlink preflight refusal"
        );
    }

    #[test]
    fn required_request_with_helper_unavailable_still_fails_closed() {
        // A `Required` request that loses containment mid-flight must fail
        // closed rather than falling through to any uncontained path.
        let outcome = interpret_sandbox_status(
            None,
            &SandboxRequest::Required(SandboxLaunchSpec {
                linux_protected_mount_view: false,
                protected_readonly_paths: Vec::new(),
                path_identities: Vec::new(),
                deny_paths: Vec::new(),
                target: PathBuf::from("/bin/sh"),
                args: vec!["-c".to_string(), "true".to_string()],
                read_paths: Vec::new(),
                write_paths: Vec::new(),
            }),
        );
        assert!(matches!(
            outcome,
            Err(ManagedProcessError::SandboxFailed(reason)) if reason.contains("status")
        ));
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

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn protected_mount_view_real_managed_process_handoff() {
        if std::env::var("CODEGG_RUN_C006_BWRAP_NATIVE").as_deref() != Ok("1") {
            eprintln!(
                "skipped: set CODEGG_RUN_C006_BWRAP_NATIVE=1 for native Bubblewrap qualification"
            );
            return;
        }

        use crate::security::sandbox::{BackendId, SandboxConfig, SandboxMode};

        fn shell_quote(path: &Path) -> String {
            format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
        }

        let workspace = tempfile::tempdir().expect("writable workspace");
        let outside = tempfile::tempdir().expect("outside directory");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(workspace.path())
            .status()
            .expect("run git init fixture");
        assert!(
            initialized.success(),
            "git init fixture failed: {initialized}"
        );
        let configured = std::process::Command::new("git")
            .args(["-C"])
            .arg(workspace.path())
            .args(["config", "--local", "c006.fixture", "protected config"])
            .status()
            .expect("write fixture Git config");
        assert!(
            configured.success(),
            "Git config fixture failed: {configured}"
        );
        std::fs::write(
            workspace.path().join(".git/hooks/pre-commit"),
            "protected hook",
        )
        .expect("Git hook fixture");
        std::fs::write(workspace.path().join(".secret"), "synthetic secret")
            .expect("denied secret fixture");
        let outside_secret = outside.path().join("outside.secret");
        std::fs::write(&outside_secret, "outside secret").expect("outside secret fixture");
        std::os::unix::fs::symlink(&outside_secret, workspace.path().join("outside-link"))
            .expect("external symlink fixture");
        let future_secret = workspace.path().join("future-secret");

        let script = format!(
            "set -eu\nworkspace={}\nprintf allowed > \"$workspace/allowed.txt\"\n\
             git -C \"$workspace\" --no-optional-locks status --porcelain >/dev/null\n\
             test \"$(git -C \"$workspace\" config --local c006.fixture)\" = 'protected config'\n\
             if printf changed > \"$workspace/.git/config\" 2>/dev/null; then exit 71; fi\n\
             if printf changed > \"$workspace/.git/hooks/pre-commit\" 2>/dev/null; then exit 72; fi\n\
             if grep -q 'synthetic secret' \"$workspace/.secret\" 2>/dev/null; then exit 73; fi\n\
             if printf changed > \"$workspace/.secret\" 2>/dev/null; then exit 74; fi\n\
             if printf changed > {}; then exit 75; fi\n\
             if cat \"$workspace/outside-link\" >/dev/null 2>&1; then exit 76; fi\n\
             if printf future > {}; then exit 77; fi\n\
             test ! -e /proc/self/root && test ! -e /sys/kernel\n\
             test \"$(python3 -c 'print(\"python-ok\")')\" = python-ok\n\
             printf target-ran > \"$workspace/marker\"",
            shell_quote(workspace.path()),
            shell_quote(&outside_secret),
            shell_quote(&future_secret),
        );
        let config = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![workspace.path().to_string_lossy().into_owned()])
            .with_deny_paths(vec![
                workspace.path().join(".secret").display().to_string(),
                future_secret.display().to_string(),
            ])
            .with_linux_protected_mount_view(true);
        let spec = config
            .launch_spec("/bin/sh", &["-c".into(), script], Some(workspace.path()))
            .expect("protected mount launch lease");
        let mut argv = vec![spec.target.as_os_str().to_owned()];
        argv.extend(spec.args.iter().map(OsString::from));
        let mut request = ManagedProcessRequest::new(
            argv,
            workspace.path().to_owned(),
            ProcessProvenance::new("c006-native", "managed-process-handoff"),
        );
        request.timeout = Some(Duration::from_secs(15));
        request.sandbox = SandboxRequest::Required(spec.clone());

        let test_exe = std::env::current_exe().expect("unit test executable");
        let helper = test_exe
            .parent()
            .and_then(Path::parent)
            .expect("Cargo target directory")
            .join(crate::install::sandbox_helper_name());
        assert!(
            helper.is_file(),
            "built sandbox helper missing: {}",
            helper.display()
        );

        let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(64);
        let result = ManagedProcessService::run_streaming(request, output_tx).await;
        let mut setup_stderr = Vec::new();
        while let Ok(chunk) = output_rx.try_recv() {
            if chunk.stream == OutputStream::Stderr {
                setup_stderr.extend_from_slice(&chunk.bytes);
            }
        }
        let result = result.unwrap_or_else(|error| {
            panic!(
                "real Bubblewrap protected mount launch failed: {error}; child stderr: {}",
                String::from_utf8_lossy(&setup_stderr)
            )
        });
        assert!(
            result.exit_status.success(),
            "child stderr: {}",
            result.stderr.to_string_lossy()
        );
        assert!(matches!(
            result.sandbox,
            SandboxExecutionOutcome::Enforced { backend, .. } if backend == BackendId::BUBBLEWRAP
        ));
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("allowed.txt")).unwrap(),
            "allowed"
        );
        let git_config = std::process::Command::new("git")
            .args(["-C"])
            .arg(workspace.path())
            .args(["config", "--local", "c006.fixture"])
            .output()
            .expect("read host Git fixture config");
        assert!(git_config.status.success());
        assert_eq!(
            String::from_utf8_lossy(&git_config.stdout).trim(),
            "protected config"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(".git/hooks/pre-commit")).unwrap(),
            "protected hook"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("marker")).unwrap(),
            "target-ran"
        );
        assert_eq!(
            std::fs::read_to_string(&outside_secret).unwrap(),
            "outside secret"
        );

        let hardlink_marker = workspace.path().join("hardlink-must-not-run");
        let hardlink_alias = workspace.path().join("git-config-alias");
        std::fs::hard_link(workspace.path().join(".git/config"), &hardlink_alias)
            .expect("hardlink protected Git metadata");
        let mut hardlink_spec = spec.clone();
        hardlink_spec.args = vec![
            "-c".into(),
            format!("touch {}", shell_quote(&hardlink_marker)),
        ];
        let mut hardlink_argv = vec![hardlink_spec.target.as_os_str().to_owned()];
        hardlink_argv.extend(hardlink_spec.args.iter().map(OsString::from));
        let mut hardlink_request = ManagedProcessRequest::new(
            hardlink_argv,
            workspace.path().to_owned(),
            ProcessProvenance::new("c006-native", "hardlink-refusal"),
        );
        hardlink_request.sandbox = SandboxRequest::Required(hardlink_spec);
        assert!(ManagedProcessService::run(hardlink_request).await.is_err());
        assert!(
            !hardlink_marker.exists(),
            "target ran with aliased Git metadata"
        );
        std::fs::remove_file(hardlink_alias).expect("remove hardlink fixture");

        let nested_repo = workspace.path().join("nested-repo");
        std::fs::create_dir(&nested_repo).expect("nested repository directory");
        std::os::unix::fs::symlink(outside.path(), nested_repo.join(".git"))
            .expect("symlinked Git metadata fixture");
        let symlink_marker = workspace.path().join("symlink-must-not-run");
        let mut symlink_spec = spec.clone();
        symlink_spec.args = vec![
            "-c".into(),
            format!("touch {}", shell_quote(&symlink_marker)),
        ];
        let mut symlink_argv = vec![symlink_spec.target.as_os_str().to_owned()];
        symlink_argv.extend(symlink_spec.args.iter().map(OsString::from));
        let mut symlink_request = ManagedProcessRequest::new(
            symlink_argv,
            workspace.path().to_owned(),
            ProcessProvenance::new("c006-native", "symlink-refusal"),
        );
        symlink_request.sandbox = SandboxRequest::Required(symlink_spec);
        assert!(ManagedProcessService::run(symlink_request).await.is_err());
        assert!(
            !symlink_marker.exists(),
            "target ran with symlinked Git metadata"
        );
        std::fs::remove_file(nested_repo.join(".git")).expect("remove symlink fixture");
        std::fs::remove_dir(nested_repo).expect("remove nested repository fixture");

        let marker = workspace.path().join("must-not-run");
        let mut invalid_spec = spec;
        let identity = invalid_spec
            .path_identities
            .iter_mut()
            .find(|identity| identity.path == workspace.path())
            .expect("workspace identity snapshot");
        identity.inode = identity.inode.saturating_add(1);
        invalid_spec.args = vec!["-c".into(), format!("touch {}", shell_quote(&marker))];
        let mut invalid_argv = vec![invalid_spec.target.as_os_str().to_owned()];
        invalid_argv.extend(invalid_spec.args.iter().map(OsString::from));
        let mut invalid_request = ManagedProcessRequest::new(
            invalid_argv,
            workspace.path().to_owned(),
            ProcessProvenance::new("c006-native", "identity-failure"),
        );
        invalid_request.sandbox = SandboxRequest::Required(invalid_spec);
        let error = ManagedProcessService::run(invalid_request)
            .await
            .expect_err("changed grant identity must fail closed");
        assert!(matches!(error, ManagedProcessError::SandboxFailed(_)));
        assert!(!marker.exists(), "target ran after grant identity failure");

        let timeout_marker = workspace.path().join("timeout-must-not-run");
        let timeout_config = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![workspace.path().to_string_lossy().into_owned()])
            .with_linux_protected_mount_view(true);
        let timeout_script = format!("sleep 5; touch {}", shell_quote(&timeout_marker));
        let timeout_spec = timeout_config
            .launch_spec(
                "/bin/sh",
                &["-c".into(), timeout_script],
                Some(workspace.path()),
            )
            .expect("timeout launch lease");
        let mut timeout_argv = vec![timeout_spec.target.as_os_str().to_owned()];
        timeout_argv.extend(timeout_spec.args.iter().map(OsString::from));
        let mut timeout_request = ManagedProcessRequest::new(
            timeout_argv,
            workspace.path().to_owned(),
            ProcessProvenance::new("c006-native", "timeout-cleanup"),
        );
        timeout_request.sandbox = SandboxRequest::Required(timeout_spec);
        timeout_request.timeout = Some(Duration::from_millis(200));
        let timed_out = ManagedProcessService::run(timeout_request)
            .await
            .expect("timed out child is reaped");
        assert_eq!(timed_out.termination, TerminationReason::TimedOut);
        assert!(
            !timeout_marker.exists(),
            "target continued after managed timeout cleanup"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread")]
    async fn constrained_launch_gets_private_tmpdir_removed_after_reap() {
        if crate::security::sandbox::probe_landlock().is_err() {
            return;
        }
        let workspace = tempfile::tempdir().expect("workspace");
        let command =
            "printf '%s|%s' \"$TMPDIR\" \"$(stat -c %a \"$TMPDIR\")\"; test -w \"$TMPDIR\"";
        let config = crate::security::sandbox::SandboxConfig::new()
            .with_enabled(true)
            .with_mode(crate::security::sandbox::SandboxMode::ReadOnly)
            .with_allowed_paths(vec![workspace.path().to_string_lossy().to_string()]);
        let mut spec = config
            .launch_spec(
                "/bin/sh",
                &["-c".to_string(), command.to_string()],
                Some(workspace.path()),
            )
            .expect("launch spec");
        // This test isolates scratch creation and cleanup. Credential deny
        // fixtures are covered by the sandbox policy/helper tests; avoiding
        // host-home inspection here keeps the test portable to containers
        // where HOME points at a directory the runner cannot inspect.
        spec.deny_paths.clear();
        let mut request = ManagedProcessRequest::new(
            vec!["/bin/sh".into(), "-c".into(), command.into()],
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

        let result = run_inner(request, None, Some(&helper))
            .await
            .expect("contained process succeeds");
        assert!(
            result.exit_status.success(),
            "{}",
            result.stderr.to_string_lossy()
        );
        assert!(matches!(
            result.sandbox,
            SandboxExecutionOutcome::Enforced { .. }
        ));
        let stdout = result.stdout.to_string_lossy();
        let (scratch, mode) = stdout.split_once('|').expect("TMPDIR and mode");
        let scratch_root = Path::new(scratch)
            .parent()
            .expect("private scratch has a parent");
        assert_eq!(
            std::fs::canonicalize(scratch_root).expect("scratch parent canonicalizes"),
            std::fs::canonicalize(std::env::temp_dir()).expect("system temp root canonicalizes"),
            "scratch must be created under the system temporary root: {scratch}"
        );
        assert_eq!(mode, "700", "scratch permissions must be private: {stdout}");
        assert!(
            !Path::new(scratch).exists(),
            "private scratch should be removed after the target is reaped"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread")]
    async fn constrained_launch_gets_private_tmpdir_removed_after_reap_on_macos() {
        let workspace = tempfile::tempdir().expect("workspace");
        let command =
            "printf '%s|%s' \"$TMPDIR\" \"$(stat -f %Lp \"$TMPDIR\")\"; test -w \"$TMPDIR\"";
        let config = crate::security::sandbox::SandboxConfig::new()
            .with_enabled(true)
            .with_mode(crate::security::sandbox::SandboxMode::ReadOnly)
            .with_allowed_paths(vec![workspace.path().to_string_lossy().to_string()]);
        let mut spec = config
            .launch_spec(
                "/bin/sh",
                &["-c".to_string(), command.to_string()],
                Some(workspace.path()),
            )
            .expect("launch spec");
        // The helper sandbox tests cover credential denials. This lifecycle
        // probe isolates the managed scratch contract from host-home access.
        spec.deny_paths.clear();
        let mut request = ManagedProcessRequest::new(
            vec!["/bin/sh".into(), "-c".into(), command.into()],
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

        let result = run_inner(request, None, Some(&helper))
            .await
            .expect("contained process succeeds");
        assert!(
            result.exit_status.success(),
            "{}",
            result.stderr.to_string_lossy()
        );
        assert!(matches!(
            result.sandbox,
            SandboxExecutionOutcome::Enforced { .. }
        ));
        let stdout = result.stdout.to_string_lossy();
        let (scratch, mode) = stdout.split_once('|').expect("TMPDIR and mode");
        let scratch_root = Path::new(scratch)
            .parent()
            .expect("private scratch has a parent");
        assert_eq!(
            std::fs::canonicalize(scratch_root).expect("scratch parent canonicalizes"),
            std::fs::canonicalize(std::env::temp_dir()).expect("system temp root canonicalizes"),
            "scratch must be created under the system temporary root: {scratch}"
        );
        assert_eq!(mode, "700", "scratch permissions must be private: {stdout}");
        assert!(
            !Path::new(scratch).exists(),
            "private scratch should be removed after the target is reaped"
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

    #[cfg(all(windows, feature = "windows-sandbox-test-support"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn c013_forensic_existing_object_acl_and_alias_probe() {
        let fixture = tempfile::tempdir().expect("C013 disposable fixture");
        let workspace = fixture.path().join("workspace");
        let credentials = fixture.path().join("credentials");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&credentials).unwrap();
        let allowed_file = workspace.join("allowed-workspace-file.txt");
        std::fs::write(&allowed_file, "synthetic-workspace-file").unwrap();
        let credential = credentials.join("synthetic-secret.txt");
        let protected = credentials.join("protected-secret.txt");
        std::fs::write(&credential, "synthetic-c013-secret").unwrap();
        std::fs::write(&protected, "synthetic-protected-secret").unwrap();

        let protect = std::process::Command::new("icacls.exe")
            .arg(&protected)
            .arg("/inheritance:d")
            .output()
            .expect("icacls is available on the native Windows runner");
        assert!(
            protect.status.success(),
            "failed to make the synthetic child DACL protected: {}",
            String::from_utf8_lossy(&protect.stderr)
        );

        let hardlink = workspace.join("hardlink-secret.txt");
        std::fs::hard_link(&credential, &hardlink).expect("NTFS hardlink fixture");
        let alternate_stream = PathBuf::from(format!("{}:c013", credential.display()));
        std::fs::write(&alternate_stream, "synthetic-c013-ads").expect("NTFS ADS fixture");
        let junction = workspace.join("credential-junction");
        let junction_result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&credentials)
            .output()
            .expect("cmd.exe is present on the native Windows runner");
        assert!(
            junction_result.status.success(),
            "failed to create disposable junction: {}",
            String::from_utf8_lossy(&junction_result.stderr)
        );

        let executable = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32")
            .join("cmd.exe");
        let script = [
            r#"if defined C013_START_MARKER echo started>"%C013_START_MARKER%""#,
            r#"type "%C013_ALLOWED%" >nul 2>nul && echo allowed=readable || echo allowed=denied"#,
            r#"type "%C013_DIRECT%" >nul 2>nul && echo direct=leaked || echo direct=denied"#,
            r#"type "%C013_JUNCTION%" >nul 2>nul && echo junction=leaked || echo junction=denied"#,
            r#"type "%C013_HARDLINK%" >nul 2>nul && echo hardlink=leaked || echo hardlink=denied"#,
            r#"type "%C013_PROTECTED%" >nul 2>nul && echo protected=leaked || echo protected=denied"#,
            r#"type "%C013_ADS%" >nul 2>nul && echo ads=leaked || echo ads=denied"#,
            r#"type "%C013_DENY_ROOT%\created-after-lease.txt" >nul 2>nul && echo postLease=leaked || echo postLease=denied"#,
            r#"type nul > "%C013_DENY_ROOT%\created-after-launch.txt" 2>nul && echo new=created || echo new=creation-denied"#,
        ]
        .join("\r\n");
        let probe_script = workspace.join("c013-probe.cmd");
        std::fs::write(&probe_script, script).expect("write disposable C013 probe script");
        let observed_objects = [
            &credentials,
            &credential,
            &protected,
            &hardlink,
            &junction,
            &allowed_file,
            &probe_script,
        ];
        let before = observed_objects
            .iter()
            .map(|path| windows_acl_lease::forensic_object_snapshot(path).unwrap())
            .collect::<Vec<_>>();
        for snapshot in &before {
            eprintln!("C013 pre-lease object: {snapshot}");
        }
        let args = vec![
            OsString::from("/d"),
            OsString::from("/c"),
            probe_script.as_os_str().to_owned(),
        ];
        let request_for = |include_deny_root: bool| {
            let mut request = ManagedProcessRequest::new(
                std::iter::once(executable.clone().into_os_string())
                    .chain(args.iter().cloned())
                    .collect(),
                std::env::var_os("SystemRoot")
                    .map(PathBuf::from)
                    .expect("SystemRoot is set")
                    .join("System32"),
                ProcessProvenance::new("windows-c013-forensic", "existing-object-access"),
            );
            request.timeout = Some(Duration::from_secs(15));
            request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
                linux_protected_mount_view: false,
                protected_readonly_paths: Vec::new(),
                path_identities: Vec::new(),
                target: executable.clone(),
                args: args
                    .iter()
                    .map(|argument| argument.to_string_lossy().into_owned())
                    .collect(),
                read_paths: Vec::new(),
                write_paths: vec![workspace.clone()],
                deny_paths: include_deny_root
                    .then(|| credentials.clone())
                    .into_iter()
                    .collect(),
            });
            request.environment_policy = EnvironmentPolicy::sanitized()
                .with_var("C013_ALLOWED", allowed_file.as_os_str())
                .with_var("C013_DIRECT", credential.as_os_str())
                .with_var(
                    "C013_JUNCTION",
                    junction.join("synthetic-secret.txt").as_os_str(),
                )
                .with_var("C013_HARDLINK", hardlink.as_os_str())
                .with_var("C013_PROTECTED", protected.as_os_str())
                .with_var("C013_ADS", alternate_stream.as_os_str())
                .with_var("C013_DENY_ROOT", credentials.as_os_str());
            request
        };

        let start_marker = fixture.path().join("unexpected-child-start.txt");
        let mut populated_workspace_request = request_for(false);
        populated_workspace_request.environment_policy = populated_workspace_request
            .environment_policy
            .clone()
            .with_var("C013_START_MARKER", start_marker.as_os_str());
        let error = run_windows_test_restricted(populated_workspace_request, None)
            .await
            .expect_err("populated workspace lease root must fail closed before spawn");
        assert!(matches!(error, ManagedProcessError::SandboxFailed(_)));
        assert!(
            !start_marker.exists(),
            "populated workspace child did not start"
        );
        let after_rejection = observed_objects
            .iter()
            .map(|path| windows_acl_lease::forensic_object_snapshot(path).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            after_rejection, before,
            "rejected populated root must not mutate ACLs"
        );

        let grant_only_result = run_windows_test_restricted_forensic(request_for(false))
            .await
            .expect("grant-only C013 fixture completes");
        assert!(
            grant_only_result.exit_status.success(),
            "C013 grant-only child exited {:?}; stdout={:?}; stderr={:?}",
            grant_only_result.exit_status,
            grant_only_result.stdout.to_string_lossy(),
            grant_only_result.stderr.to_string_lossy()
        );
        let grant_only = grant_only_result.stdout.to_string_lossy();
        eprintln!("C013 grant-only candidate results: {grant_only}");
        assert!(grant_only.contains("allowed=readable") || grant_only.contains("allowed=denied"));
        for label in ["direct", "junction", "hardlink", "protected", "ads"] {
            assert!(
                grant_only.contains(&format!("{label}=denied")),
                "grant-only C013 read leaked through {label}: {grant_only}"
            );
        }
        assert!(matches!(
            grant_only_result.sandbox,
            SandboxExecutionOutcome::TestOnlyAppContainer {
                token_verified: true,
                job_verified: true,
                ..
            }
        ));

        let result = run_windows_test_restricted_forensic(request_for(true))
            .await
            .expect("isolated C013 forensic child completes");
        assert!(
            result.exit_status.success(),
            "C013 deny-lease child exited {:?}; stdout={:?}; stderr={:?}",
            result.exit_status,
            result.stdout.to_string_lossy(),
            result.stderr.to_string_lossy()
        );
        let observations = result.stdout.to_string_lossy();
        eprintln!("C013 restricted-child access results: {observations}");
        assert!(
            observations.contains("allowed=readable") || observations.contains("allowed=denied")
        );
        for label in [
            "direct",
            "junction",
            "hardlink",
            "protected",
            "ads",
            "postLease",
        ] {
            assert!(
                observations.contains(&format!("{label}=denied")),
                "deny-lease C013 read leaked through {label}: {observations}"
            );
        }
        assert!(observations.contains("new=creation-denied"));
        assert!(matches!(
            result.sandbox,
            SandboxExecutionOutcome::TestOnlyAppContainer {
                token_verified: true,
                job_verified: true,
                ..
            }
        ));

        let after = observed_objects
            .iter()
            .map(|path| windows_acl_lease::forensic_object_snapshot(path).unwrap())
            .collect::<Vec<_>>();
        for snapshot in &after {
            eprintln!("C013 post-restore object: {snapshot}");
        }
        if after != before {
            eprintln!(
                "C013 finding: root-level DACL restoration left descendant descriptor/control drift"
            );
        }
        assert_ne!(
            after, before,
            "C013 diagnostic expects to reproduce the current root-only restoration limitation"
        );
    }
}
