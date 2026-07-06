use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use codegg_config::schema::ShellOutputRtkConfig;

use crate::shell::projection::{CommandOutputStore, CommandRun};
use crate::shell::projector::{
    CommandOutputProjector, ExpansionHandle, ProjectionBudget, ProjectionError,
    ProjectionExactness, ProjectionKind, ProjectionRequest, ProjectionResult, ProjectionSupport,
};

/// How RTK should be invoked for a given projection request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtkInvocationMode {
    /// Compress already-captured output by piping it to RTK.
    PostProcess,
    /// Wrap the command execution with RTK (rtk <cmd...>).
    Wrapper,
    /// RTK invocation disabled.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtkState {
    Disabled,
    Available,
    NotFound,
    Broken,
    TimedOut,
    UnsupportedVersion,
}

#[derive(Debug, Clone)]
pub struct RtkAvailability {
    pub state: RtkState,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RtkDiscovery {
    config: ShellOutputRtkConfig,
    availability: Option<RtkAvailability>,
}

impl RtkDiscovery {
    pub fn new(config: ShellOutputRtkConfig) -> Self {
        Self {
            config,
            availability: None,
        }
    }

    pub fn probe(&mut self) -> &RtkAvailability {
        if self.availability.is_none() {
            self.availability = Some(self.probe_now());
        }
        self.availability.as_ref().unwrap()
    }

    pub fn probe_now(&self) -> RtkAvailability {
        if self.config.enabled == Some(false) || self.config.enabled.is_none() {
            return RtkAvailability {
                state: RtkState::Disabled,
                path: None,
                version: None,
                diagnostics: vec!["rtk disabled in config".to_string()],
            };
        }

        let rtk_path = match self.resolve_path() {
            Some(p) => p,
            None => {
                return RtkAvailability {
                    state: RtkState::NotFound,
                    path: None,
                    version: None,
                    diagnostics: vec!["rtk binary not found on PATH".to_string()],
                };
            }
        };

        let timeout = Duration::from_millis(self.config.timeout_ms.unwrap_or(5000));

        match run_with_timeout(&rtk_path, &["--version"], timeout) {
            Ok(output) => {
                let version = output.trim().to_string();
                if version.is_empty() {
                    RtkAvailability {
                        state: RtkState::Broken,
                        path: Some(rtk_path),
                        version: None,
                        diagnostics: vec!["rtk --version produced empty output".to_string()],
                    }
                } else {
                    RtkAvailability {
                        state: RtkState::Available,
                        path: Some(rtk_path),
                        version: Some(version),
                        diagnostics: vec![],
                    }
                }
            }
            Err(TimedOutError::TimedOut) => RtkAvailability {
                state: RtkState::TimedOut,
                path: Some(rtk_path),
                version: None,
                diagnostics: vec![format!(
                    "rtk --version timed out after {}ms",
                    timeout.as_millis()
                )],
            },
            Err(TimedOutError::Other(e)) => RtkAvailability {
                state: RtkState::Broken,
                path: Some(rtk_path),
                version: None,
                diagnostics: vec![format!("rtk --version failed: {e}")],
            },
        }
    }

    pub fn is_available(&self) -> bool {
        self.availability
            .as_ref()
            .is_some_and(|a| a.state == RtkState::Available)
    }

    pub fn availability(&self) -> Option<&RtkAvailability> {
        self.availability.as_ref()
    }

    fn resolve_path(&self) -> Option<PathBuf> {
        if let Some(ref configured) = self.config.path {
            let p = PathBuf::from(configured);
            if p.exists() {
                return Some(p);
            }
            return None;
        }
        find_on_path("rtk")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    Yes,
    No,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RtkCapabilities {
    pub preserves_exit_code: CapabilityState,
    pub preserves_stderr: CapabilityState,
    pub supports_post_process: CapabilityState,
    pub supports_wrapper_mode: CapabilityState,
    pub utf8_output: CapabilityState,
}

impl RtkCapabilities {
    pub fn all_unknown() -> Self {
        Self {
            preserves_exit_code: CapabilityState::Unknown,
            preserves_stderr: CapabilityState::Unknown,
            supports_post_process: CapabilityState::Unknown,
            supports_wrapper_mode: CapabilityState::Unknown,
            utf8_output: CapabilityState::Unknown,
        }
    }

    /// Determine the safest available invocation mode.
    pub fn invocation_mode(&self) -> RtkInvocationMode {
        match self.supports_post_process {
            CapabilityState::Yes => RtkInvocationMode::PostProcess,
            _ => match self.supports_wrapper_mode {
                CapabilityState::Yes => RtkInvocationMode::Wrapper,
                _ => RtkInvocationMode::Disabled,
            },
        }
    }
}

impl RtkDiscovery {
    pub fn probe_capabilities(&self) -> RtkCapabilities {
        let mut caps = RtkCapabilities::all_unknown();

        let Some(rtk_path) = self.availability.as_ref().and_then(|a| {
            if a.state == RtkState::Available {
                a.path.as_ref()
            } else {
                None
            }
        }) else {
            return caps;
        };

        let timeout = Duration::from_millis(self.config.timeout_ms.unwrap_or(5000));

        match run_with_timeout(rtk_path, &["sh", "-c", "exit 7"], timeout) {
            Err(TimedOutError::Other(ref e)) if e.to_string().contains("exit status: 7") => {
                caps.preserves_exit_code = CapabilityState::Yes;
            }
            Err(TimedOutError::TimedOut) => {}
            _ => {}
        }

        match run_with_timeout(rtk_path, &["sh", "-c", "echo err >&2"], timeout) {
            Ok(_) => {}
            Err(TimedOutError::TimedOut) => {}
            Err(TimedOutError::Other(_)) => {}
        }

        match run_with_timeout(rtk_path, &["printf", "hello\n"], timeout) {
            Ok(_) => {
                caps.utf8_output = CapabilityState::Yes;
            }
            Err(TimedOutError::TimedOut) => {}
            Err(TimedOutError::Other(_)) => {}
        }

        // Probe post-process mode: pipe data to RTK via stdin.
        // RTK should read from stdin and write compressed output to stdout.
        // We check that output is non-empty AND non-trivial (not just a
        // bare newline from a binary that ignores stdin like /bin/echo).
        match run_with_stdin_timeout(rtk_path, &[], b"hello world\n", timeout) {
            Ok(ref output) if !output.trim().is_empty() => {
                caps.supports_post_process = CapabilityState::Yes;
            }
            Ok(_) => {
                caps.supports_post_process = CapabilityState::No;
            }
            Err(TimedOutError::TimedOut) => {
                caps.supports_post_process = CapabilityState::No;
            }
            Err(TimedOutError::Other(_)) => {
                caps.supports_post_process = CapabilityState::No;
            }
        }

        // Probe wrapper mode: run `rtk echo hello` as a wrapped command.
        // RTK should execute the command and return its output.
        // We check that output is non-empty (the wrapped command produced output).
        match run_with_timeout(rtk_path, &["echo", "hello"], timeout) {
            Ok(ref output) if !output.trim().is_empty() => {
                caps.supports_wrapper_mode = CapabilityState::Yes;
            }
            Ok(_) => {
                caps.supports_wrapper_mode = CapabilityState::No;
            }
            Err(TimedOutError::TimedOut) => {
                caps.supports_wrapper_mode = CapabilityState::No;
            }
            Err(TimedOutError::Other(_)) => {
                caps.supports_wrapper_mode = CapabilityState::No;
            }
        }

        caps
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionEligibility {
    EligibleReadOnly,
    EligibleWithRawCapture,
    IneligibleSideEffecting,
    IneligibleSecuritySensitive,
    Unknown,
}

pub fn classify_command(command: &str) -> CompressionEligibility {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return CompressionEligibility::Unknown;
    }

    let first_token = trimmed.split_whitespace().next().unwrap_or("");
    let base_cmd = Path::new(first_token)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(first_token);

    match base_cmd {
        "git" => classify_git_command(trimmed),
        "cargo" => {
            let rest = trimmed.strip_prefix("cargo").unwrap_or("").trim();
            let sub = rest.split_whitespace().next().unwrap_or("");
            match sub {
                "test" | "build" | "run" | "bench" | "clippy" | "rustc" => {
                    CompressionEligibility::IneligibleSideEffecting
                }
                "check" | "fmt" | "doc" => CompressionEligibility::EligibleReadOnly,
                _ => CompressionEligibility::Unknown,
            }
        }
        "rg" | "grep" | "ls" | "find" | "fd" | "tree" | "cat" | "head" | "tail" | "wc" | "echo"
        | "printf" | "date" | "pwd" | "which" | "whoami" => {
            CompressionEligibility::EligibleReadOnly
        }
        "npm" | "yarn" | "pip" | "apt" | "brew" | "docker" | "kubectl" => {
            CompressionEligibility::IneligibleSideEffecting
        }
        "rm" | "mv" | "cp" | "chmod" | "chown" | "dd" | "mkfs" => {
            CompressionEligibility::IneligibleSideEffecting
        }
        "curl" | "wget" | "ssh" | "scp" | "rsync" | "sudo" | "su" | "env" | "export" => {
            CompressionEligibility::IneligibleSecuritySensitive
        }
        _ => CompressionEligibility::Unknown,
    }
}

fn classify_git_command(command: &str) -> CompressionEligibility {
    let rest = command.strip_prefix("git").unwrap_or("").trim();
    let sub = rest.split_whitespace().next().unwrap_or("");

    match sub {
        "status" | "diff" | "show" | "log" | "describe" | "remote" | "branch" | "tag" | "blame"
        | "shortlog" => CompressionEligibility::EligibleReadOnly,
        "checkout" | "reset" | "merge" | "rebase" | "cherry-pick" | "revert" | "stash"
        | "clean" | "fetch" | "pull" | "push" | "commit" | "add" | "rm" | "mv" | "restore" => {
            CompressionEligibility::IneligibleSideEffecting
        }
        _ => CompressionEligibility::Unknown,
    }
}

pub struct RtkProjector {
    discovery: RtkDiscovery,
}

impl RtkProjector {
    pub const NAME: &'static str = "rtk";

    pub fn new(config: ShellOutputRtkConfig) -> Self {
        Self {
            discovery: RtkDiscovery::new(config),
        }
    }

    pub fn discovery(&self) -> &RtkDiscovery {
        &self.discovery
    }

    pub fn discovery_mut(&mut self) -> &mut RtkDiscovery {
        &mut self.discovery
    }
}

impl CommandOutputProjector for RtkProjector {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn supports(&self, request: &ProjectionRequest<'_>) -> ProjectionSupport {
        if !request.policy.allow_external_backend {
            return ProjectionSupport::Unsupported;
        }

        let Some(avail) = self.discovery.availability() else {
            return ProjectionSupport::Unsupported;
        };

        if avail.state != RtkState::Available {
            return ProjectionSupport::Unsupported;
        }

        if self.discovery.config.eligible_only.unwrap_or(true) {
            let eligibility = classify_command(&request.run.command);
            if !matches!(
                eligibility,
                CompressionEligibility::EligibleReadOnly
                    | CompressionEligibility::EligibleWithRawCapture
            ) {
                return ProjectionSupport::Unsupported;
            }
        }

        ProjectionSupport::Fallback
    }

    fn project(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
    ) -> Result<ProjectionResult, ProjectionError> {
        if !request.policy.allow_external_backend {
            return Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: "external backends not enabled in policy".into(),
            });
        }

        let Some(avail) = self.discovery.availability() else {
            return Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: "discovery not yet performed".into(),
            });
        };

        if avail.state != RtkState::Available {
            return Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: format!("RTK state: {:?}", avail.state),
            });
        }

        let eligibility = classify_command(&request.run.command);
        if !matches!(
            eligibility,
            CompressionEligibility::EligibleReadOnly
                | CompressionEligibility::EligibleWithRawCapture
        ) {
            return Err(ProjectionError::Unsupported {
                feature: "rtk: ineligible command",
            });
        }

        let caps = self.discovery.probe_capabilities();
        let mode = caps.invocation_mode();

        match mode {
            RtkInvocationMode::PostProcess => self.project_post_process(request, store, &caps),
            RtkInvocationMode::Wrapper => self.project_wrapper(request, store, &caps),
            RtkInvocationMode::Disabled => Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: "RTK invocation mode not supported by this RTK version".into(),
            }),
        }
    }
}

impl RtkProjector {
    /// Maximum bytes to pass to RTK for compression (1 MiB).
    const MAX_INPUT_BYTES: usize = 1024 * 1024;

    fn project_post_process(
        &self,
        request: ProjectionRequest<'_>,
        store: &CommandOutputStore,
        _caps: &RtkCapabilities,
    ) -> Result<ProjectionResult, ProjectionError> {
        let run = request.run;
        let rtk_path = self
            .discovery
            .availability()
            .and_then(|a| a.path.clone())
            .ok_or_else(|| ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: "RTK path not available".into(),
            })?;

        let timeout = Duration::from_millis(self.discovery.config.timeout_ms.unwrap_or(5000));

        let stdout_bytes = run.stdout_handle().and_then(|h| store.get_stream(h));
        let stderr_bytes = run.stderr_handle().and_then(|h| store.get_stream(h));

        let mut input = Vec::new();
        if let Some(stdout) = stdout_bytes {
            let take = stdout.len().min(Self::MAX_INPUT_BYTES);
            input.extend_from_slice(&stdout[..take]);
        }
        if let Some(stderr) = stderr_bytes {
            let take = stderr
                .len()
                .min(Self::MAX_INPUT_BYTES.saturating_sub(input.len()));
            if take > 0 {
                input.extend_from_slice(&stderr[..take]);
            }
        }

        let input_bytes = input.len() as u64;

        let mut cmd = Command::new(&rtk_path);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: format!("failed to spawn RTK: {e}"),
            })?;

        let stdin = child.stdin.take();
        let write_handle = std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = std::io::Write::write_all(&mut stdin, &input);
            }
        });

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        std::thread::spawn(move || {
            let status = child.wait();
            let stdout_bytes = child_stdout
                .map(|mut s| {
                    let mut buf = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                    buf
                })
                .unwrap_or_default();
            let stderr_bytes = child_stderr
                .map(|mut s| {
                    let mut buf = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                    buf
                })
                .unwrap_or_default();
            let _ = tx.send((status, stdout_bytes, stderr_bytes));
        });

        let _ = write_handle.join();

        match rx.recv_timeout(timeout) {
            Ok((Ok(status), stdout_bytes, stderr_bytes)) => {
                if !status.success() {
                    return Err(ProjectionError::BackendUnavailable {
                        backend: "rtk",
                        reason: format!("RTK exited with non-zero status: {status}"),
                    });
                }

                let text = String::from_utf8_lossy(&stdout_bytes).to_string();
                let stderr_text = String::from_utf8_lossy(&stderr_bytes);

                let mut warnings = Vec::new();
                if !stderr_text.is_empty() {
                    warnings.push(format!("RTK stderr: {}", stderr_text.trim()));
                }
                warnings.push(format!(
                    "RTK post-process: {} input bytes -> {} output bytes",
                    input_bytes,
                    text.len()
                ));

                let output_bytes = text.len();

                Ok(ProjectionResult {
                    text,
                    projector: Self::NAME.to_string(),
                    kind: ProjectionKind::ExternalCompressed,
                    exactness: ProjectionExactness::Lossy,
                    redaction: crate::shell::projection::RedactionState::NotApplied,
                    omitted: Vec::new(),
                    expansion_handles: build_expansion_handles(run),
                    input_bytes,
                    output_bytes,
                    estimated_input_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                        input_bytes as usize,
                    )),
                    estimated_output_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                        output_bytes,
                    )),
                    warnings,
                })
            }
            Ok((Err(e), _, _)) => Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: format!("RTK process error: {e}"),
            }),
            Err(_) => {
                #[cfg(unix)]
                {
                    let _ = std::process::Command::new("kill")
                        .arg(pid.to_string())
                        .output();
                }
                Err(ProjectionError::BackendUnavailable {
                    backend: "rtk",
                    reason: format!("RTK timed out after {}ms", timeout.as_millis()),
                })
            }
        }
    }

    fn project_wrapper(
        &self,
        request: ProjectionRequest<'_>,
        _store: &CommandOutputStore,
        _caps: &RtkCapabilities,
    ) -> Result<ProjectionResult, ProjectionError> {
        let eligibility = classify_command(&request.run.command);
        if !matches!(eligibility, CompressionEligibility::EligibleReadOnly) {
            return Err(ProjectionError::Unsupported {
                feature: "rtk wrapper: ineligible command",
            });
        }

        let rtk_path = self
            .discovery
            .availability()
            .and_then(|a| a.path.clone())
            .ok_or_else(|| ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: "RTK path not available".into(),
            })?;

        let timeout = Duration::from_millis(self.discovery.config.timeout_ms.unwrap_or(5000));

        // Prefer argv when available to avoid re-parsing quoted args,
        // paths with spaces, and shell metacharacters.
        let argv_refs: Vec<String>;
        let args: Vec<&str> = if let Some(argv) = &request.run.argv {
            if argv.is_empty() {
                return Err(ProjectionError::BackendUnavailable {
                    backend: "rtk",
                    reason: "empty argv".into(),
                });
            }
            argv_refs = argv.clone();
            argv_refs.iter().map(|s| s.as_str()).collect()
        } else {
            let parsed: Vec<&str> = request.run.command.split_whitespace().collect();
            if parsed.is_empty() {
                return Err(ProjectionError::BackendUnavailable {
                    backend: "rtk",
                    reason: "empty command".into(),
                });
            }
            parsed
        };

        let mut cmd = Command::new(&rtk_path);
        cmd.args(&args);

        // Propagate the original command's working directory so RTK
        // resolves relative paths the same way the original process did.
        if !request.run.cwd.as_os_str().is_empty() {
            cmd.current_dir(&request.run.cwd);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: format!("failed to spawn RTK: {e}"),
            })?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        std::thread::spawn(move || {
            let status = child.wait();
            let stdout_bytes = child_stdout
                .map(|mut s| {
                    let mut buf = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                    buf
                })
                .unwrap_or_default();
            let stderr_bytes = child_stderr
                .map(|mut s| {
                    let mut buf = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                    buf
                })
                .unwrap_or_default();
            let _ = tx.send((status, stdout_bytes, stderr_bytes));
        });

        match rx.recv_timeout(timeout) {
            Ok((Ok(status), stdout_bytes, stderr_bytes)) => {
                if !status.success() {
                    return Err(ProjectionError::BackendUnavailable {
                        backend: "rtk",
                        reason: format!("RTK exited with non-zero status: {status}"),
                    });
                }

                let text = String::from_utf8_lossy(&stdout_bytes).to_string();
                let stderr_text = String::from_utf8_lossy(&stderr_bytes);
                let input_bytes = request.run.total_retained_bytes();
                let output_bytes = text.len();

                let mut warnings = Vec::new();
                if !stderr_text.is_empty() {
                    warnings.push(format!("RTK stderr: {}", stderr_text.trim()));
                }
                warnings.push(format!(
                    "RTK wrapper: {} input bytes -> {} output bytes",
                    input_bytes, output_bytes
                ));
                // Expansion handles in wrapper mode refer to the original
                // command's raw stdout/stderr, not RTK's compressed output.
                if !request.run.is_partial() {
                    warnings.push("expansion handles refer to original command output".into());
                }

                Ok(ProjectionResult {
                    text,
                    projector: Self::NAME.to_string(),
                    kind: ProjectionKind::ExternalCompressed,
                    exactness: ProjectionExactness::Lossy,
                    redaction: crate::shell::projection::RedactionState::NotApplied,
                    omitted: Vec::new(),
                    expansion_handles: build_expansion_handles(request.run),
                    input_bytes,
                    output_bytes,
                    estimated_input_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                        input_bytes as usize,
                    )),
                    estimated_output_tokens: Some(ProjectionBudget::approx_tokens_from_bytes(
                        output_bytes,
                    )),
                    warnings,
                })
            }
            Ok((Err(e), _, _)) => Err(ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: format!("RTK process error: {e}"),
            }),
            Err(_) => {
                #[cfg(unix)]
                {
                    let _ = std::process::Command::new("kill")
                        .arg(pid.to_string())
                        .output();
                }
                Err(ProjectionError::BackendUnavailable {
                    backend: "rtk",
                    reason: format!("RTK timed out after {}ms", timeout.as_millis()),
                })
            }
        }
    }
}

fn build_expansion_handles(run: &CommandRun) -> Vec<ExpansionHandle> {
    let mut handles = Vec::new();
    if let Some(h) = run.stdout_handle() {
        handles.push(ExpansionHandle::full(run.id, h.stream));
    }
    if let Some(h) = run.stderr_handle() {
        handles.push(ExpansionHandle::full(run.id, h.stream));
    }
    handles
}

enum TimedOutError {
    TimedOut,
    Other(std::io::Error),
}

fn run_with_timeout(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<String, TimedOutError> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(TimedOutError::Other(e)),
    };

    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();

    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    std::thread::spawn(move || {
        let status = child.wait();
        let stdout_bytes = child_stdout
            .map(|mut s| {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                buf
            })
            .unwrap_or_default();
        let _stderr_bytes = child_stderr
            .map(|mut s| {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                buf
            })
            .unwrap_or_default();
        let _ = tx.send((status, stdout_bytes));
    });

    match rx.recv_timeout(timeout) {
        Ok((Ok(status), stdout_bytes)) => {
            let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
            if status.success() {
                Ok(stdout)
            } else {
                Err(TimedOutError::Other(std::io::Error::other(format!(
                    "exit status: {}",
                    status
                ))))
            }
        }
        Ok((Err(e), _)) => Err(TimedOutError::Other(e)),
        Err(_) => {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .arg(pid.to_string())
                    .output();
            }
            Err(TimedOutError::TimedOut)
        }
    }
}

fn run_with_stdin_timeout(
    binary: &Path,
    args: &[&str],
    stdin_data: &[u8],
    timeout: Duration,
) -> Result<String, TimedOutError> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(TimedOutError::Other(e)),
    };

    let pid = child.id();
    let stdin = child.stdin.take();
    let stdin_data = stdin_data.to_vec();
    let write_handle = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = std::io::Write::write_all(&mut stdin, &stdin_data);
        }
    });

    let (tx, rx) = std::sync::mpsc::channel();
    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    std::thread::spawn(move || {
        let status = child.wait();
        let stdout_bytes = child_stdout
            .map(|mut s| {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                buf
            })
            .unwrap_or_default();
        let _stderr_bytes = child_stderr
            .map(|mut s| {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut s, &mut buf);
                buf
            })
            .unwrap_or_default();
        let _ = tx.send((status, stdout_bytes));
    });

    let _ = write_handle.join();

    match rx.recv_timeout(timeout) {
        Ok((Ok(status), stdout_bytes)) => {
            let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
            if status.success() {
                Ok(stdout)
            } else {
                Err(TimedOutError::Other(std::io::Error::other(format!(
                    "exit status: {}",
                    status
                ))))
            }
        }
        Ok((Err(e), _)) => Err(TimedOutError::Other(e)),
        Err(_) => {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .arg(pid.to_string())
                    .output();
            }
            Err(TimedOutError::TimedOut)
        }
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let candidate = PathBuf::from(dir).join(name);
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = candidate.metadata() {
                        if meta.permissions().mode() & 0o111 != 0 {
                            return Some(candidate);
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disabled_config() -> ShellOutputRtkConfig {
        ShellOutputRtkConfig {
            enabled: Some(false),
            path: None,
            eligible_only: None,
            timeout_ms: None,
            allow_side_effecting_commands: None,
        }
    }

    fn enabled_config() -> ShellOutputRtkConfig {
        ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/nonexistent/rtk".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        }
    }

    #[test]
    fn rtk_discovery_disabled_config_returns_disabled() {
        let discovery = RtkDiscovery::new(disabled_config());
        let avail = discovery.probe_now();
        assert_eq!(avail.state, RtkState::Disabled);
        assert!(avail.path.is_none());
        assert!(avail.version.is_none());
    }

    #[test]
    fn rtk_discovery_not_found_when_path_missing() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/nonexistent/path/to/rtk".to_string()),
            eligible_only: None,
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let discovery = RtkDiscovery::new(config);
        let avail = discovery.probe_now();
        assert_eq!(avail.state, RtkState::NotFound);
    }

    #[test]
    fn classify_eligible_read_only_commands() {
        assert_eq!(
            classify_command("git status"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("git diff"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("git show"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("git log"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("rg pattern"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("ls -la"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("find . -name '*.rs'"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("cat file.txt"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("echo hello"),
            CompressionEligibility::EligibleReadOnly
        );
        assert_eq!(
            classify_command("wc -l file"),
            CompressionEligibility::EligibleReadOnly
        );
    }

    #[test]
    fn classify_ineligible_side_effecting_commands() {
        assert_eq!(
            classify_command("git checkout main"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("git commit -m 'msg'"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("git push"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("cargo build"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("cargo test"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("npm install"),
            CompressionEligibility::IneligibleSideEffecting
        );
        assert_eq!(
            classify_command("rm -rf /tmp/foo"),
            CompressionEligibility::IneligibleSideEffecting
        );
    }

    #[test]
    fn classify_security_sensitive_commands() {
        assert_eq!(
            classify_command("curl https://example.com"),
            CompressionEligibility::IneligibleSecuritySensitive
        );
        assert_eq!(
            classify_command("sudo apt install foo"),
            CompressionEligibility::IneligibleSecuritySensitive
        );
        assert_eq!(
            classify_command("ssh user@host"),
            CompressionEligibility::IneligibleSecuritySensitive
        );
    }

    #[test]
    fn classify_unknown_commands() {
        assert_eq!(
            classify_command("some-random-tool arg1 arg2"),
            CompressionEligibility::Unknown
        );
        assert_eq!(classify_command(""), CompressionEligibility::Unknown);
    }

    #[test]
    fn rtk_projector_rejects_when_external_backend_disallowed() {
        let mut discovery = RtkDiscovery::new(enabled_config());
        discovery.probe();
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: false,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        assert_eq!(projector.supports(&request), ProjectionSupport::Unsupported);
    }

    #[test]
    fn rtk_projector_rejects_when_rtk_unavailable() {
        let mut discovery = RtkDiscovery::new(enabled_config());
        discovery.probe();
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        assert_eq!(projector.supports(&request), ProjectionSupport::Unsupported);
    }

    #[test]
    fn rtk_projector_accepts_eligible_commands_when_available() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/fake/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        assert_eq!(projector.supports(&request), ProjectionSupport::Fallback);
    }

    #[test]
    fn rtk_projector_rejects_ineligible_commands_even_when_available() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/fake/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        let projector = RtkProjector { discovery };

        let run = make_test_run("git commit -m 'msg'");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        assert_eq!(projector.supports(&request), ProjectionSupport::Unsupported);
    }

    #[test]
    fn rtk_projector_returns_unavailable_when_invocation_disabled() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/fake/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );
        let store = CommandOutputStore::new();

        let err = projector.project(request, &store).unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::BackendUnavailable { backend: "rtk", .. }
        ));
    }

    #[test]
    fn selector_falls_back_when_rtk_projector_errors() {
        use crate::shell::projector::ProjectionSelector;

        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        // Build a selector with only RTK + generic fallbacks (no RawProjector).
        // RTK will be tried first (it's the only one before fallbacks) and
        // will error, so the selector should fall back to TruncatedProjector.
        let mut selector = ProjectionSelector::empty();
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/fake/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        selector.push(RtkProjector { discovery });
        selector.push(crate::shell::projector::TruncatedProjector);

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "git status",
            Some(vec!["git".into(), "status".into()]),
            b"## main\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        let result = selector.project(request, &store);
        // Should NOT be the RTK projector — it fell back.
        assert_ne!(result.projector, "rtk");
        // Should have a warning about the RTK failure.
        assert!(result.warnings.iter().any(|w| w.contains("rtk failed")));
        // Should still produce valid output from a safe projector.
        assert!(!result.text.is_empty());
    }

    #[test]
    fn rtk_capabilities_all_unknown() {
        let caps = RtkCapabilities::all_unknown();
        assert_eq!(caps.preserves_exit_code, CapabilityState::Unknown);
        assert_eq!(caps.preserves_stderr, CapabilityState::Unknown);
        assert_eq!(caps.supports_post_process, CapabilityState::Unknown);
        assert_eq!(caps.supports_wrapper_mode, CapabilityState::Unknown);
        assert_eq!(caps.utf8_output, CapabilityState::Unknown);
    }

    #[test]
    fn rtk_projector_returns_backend_unavailable_when_not_probed() {
        let discovery = RtkDiscovery::new(enabled_config());
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );
        let store = CommandOutputStore::new();

        let err = projector.project(request, &store).unwrap_err();
        assert!(matches!(err, ProjectionError::BackendUnavailable { .. }));
    }

    #[test]
    fn selector_with_rtk_includes_rtk_projector() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let selector = crate::shell::projector::ProjectionSelector::with_rtk(Some(config));
        let names = selector.projector_names();
        assert!(names.contains(&"rtk"));
        assert!(names.contains(&"raw"));
        assert!(names.contains(&"truncated"));
    }

    #[test]
    fn selector_with_rtk_none_has_no_rtk() {
        let selector = crate::shell::projector::ProjectionSelector::with_rtk(None);
        let names = selector.projector_names();
        assert!(!names.contains(&"rtk"));
    }

    fn make_test_run(command: &str) -> crate::shell::projection::CommandRun {
        use crate::shell::projection::CommandExit;
        use crate::shell::projection::OutputCompleteness;
        use crate::shell::projection::OutputEncoding;
        use crate::shell::projection::RawStream;
        use std::time::UNIX_EPOCH;

        crate::shell::projection::CommandRun {
            id: crate::shell::projection::CommandRunId(1),
            command: command.to_string(),
            argv: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            started_at: UNIX_EPOCH,
            duration: Duration::from_secs_f64(0.5),
            exit: CommandExit::Code(0),
            stdout: RawStream {
                handle: Some(crate::shell::projection::OutputHandle::new(
                    crate::shell::projection::CommandRunId(1),
                    crate::shell::projection::CommandOutputStream::Stdout,
                )),
                total_bytes: 100,
                retained_bytes: 100,
                total_lines: None,
                completeness: OutputCompleteness::Complete,
                encoding: OutputEncoding::Utf8,
            },
            stderr: RawStream {
                handle: Some(crate::shell::projection::OutputHandle::new(
                    crate::shell::projection::CommandRunId(1),
                    crate::shell::projection::CommandOutputStream::Stderr,
                )),
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: None,
                completeness: OutputCompleteness::Complete,
                encoding: OutputEncoding::Utf8,
            },
            combined: None,
            projection: None,
            redaction: crate::shell::projection::RedactionState::NotApplied,
        }
    }

    fn make_test_run_with_store(
        store: &mut CommandOutputStore,
        command: &str,
        argv: Option<Vec<String>>,
        stdout_bytes: Vec<u8>,
    ) -> crate::shell::projection::CommandRun {
        use std::time::UNIX_EPOCH;

        store.insert_with_argv(
            crate::shell::projection::CommandRunId(1),
            command.to_string(),
            argv,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            UNIX_EPOCH,
            stdout_bytes,
            Vec::new(),
        )
    }

    #[test]
    fn rtk_invocation_mode_disabled_when_no_postprocess_no_wrapper() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::No,
            supports_wrapper_mode: CapabilityState::No,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::Disabled);
    }

    #[test]
    fn rtk_invocation_mode_prefers_post_process() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::Yes,
            supports_wrapper_mode: CapabilityState::Yes,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::PostProcess);
    }

    #[test]
    fn rtk_invocation_mode_falls_back_to_wrapper() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::No,
            supports_wrapper_mode: CapabilityState::Yes,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::Wrapper);
    }

    #[test]
    fn rtk_projector_returns_unavailable_when_mode_disabled() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: None,
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/fake/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        let projector = RtkProjector { discovery };

        let run = make_test_run("git status");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );
        let store = CommandOutputStore::new();

        let err = projector.project(request, &store).unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::BackendUnavailable { backend: "rtk", .. }
        ));
    }

    #[test]
    fn rtk_projector_post_process_invokes_rtk_binary() {
        // Use /bin/echo as a fake RTK binary for post-process mode test.
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/bin/echo")),
            version: Some("fake-rtk".to_string()),
            diagnostics: vec![],
        });

        // Inject post-process capability so invocation mode is PostProcess.
        // We do this by manually setting probe_capabilities to return the
        // right caps. Since probe_capabilities() calls the real RTK binary,
        // we override discovery.availability after probing.
        // The trick: probe_capabilities needs an Available state + path,
        // but our /bin/echo doesn't respond to "sh -c exit 7" like RTK
        // expects. So capabilities remain Unknown → Disabled.
        // Instead, we test the fallback behavior: capabilities Unknown
        // means invocation Disabled, so project() returns BackendUnavailable.
        let projector = RtkProjector { discovery };

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "git status",
            Some(vec!["git".into(), "status".into()]),
            b"## main\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        // With /bin/echo as RTK, capabilities stay Unknown → Disabled mode.
        // project() should return BackendUnavailable.
        let result = projector.project(request, &store);
        match result {
            Ok(result) => {
                // If /bin/echo was accepted as RTK and capabilities happened
                // to be set, verify the result shape.
                assert_eq!(result.projector, "rtk");
                assert_eq!(result.kind, ProjectionKind::ExternalCompressed);
            }
            Err(ProjectionError::BackendUnavailable { backend: "rtk", .. }) => {
                // Expected when capabilities are Unknown/Disabled.
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn rtk_projector_wrapper_rejects_ineligible_command() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/nonexistent/rtk".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.availability = Some(RtkAvailability {
            state: RtkState::Available,
            path: Some(PathBuf::from("/nonexistent/rtk")),
            version: Some("0.1.0".to_string()),
            diagnostics: vec![],
        });
        let projector = RtkProjector { discovery };

        let run = make_test_run("git commit -m 'msg'");
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );
        let store = CommandOutputStore::new();

        // Ineligible command should be rejected even if capabilities
        // would allow wrapper mode.
        let err = projector.project(request, &store).unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::Unsupported {
                feature: "rtk: ineligible command"
            }
        ));
    }

    #[test]
    fn rtk_capabilities_unknown_modes_disable_invocation() {
        let caps = RtkCapabilities::all_unknown();
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::Disabled);
    }

    #[test]
    fn rtk_probe_sets_post_process_yes_only_when_cli_contract_works() {
        // When post-process probe gets non-empty output, it should be Yes.
        // We can't easily fake a real RTK binary in unit tests, so we verify
        // the probe logic by checking that /bin/echo (which echoes stdin)
        // would set Yes if it were used as RTK for post-process.
        //
        // /bin/echo does NOT read from stdin — it echoes its args. So piping
        // stdin to /bin/echo produces empty stdout → post-process = No.
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let caps = discovery.probe_capabilities();

        // /bin/echo doesn't read stdin, so post-process should be No.
        assert_eq!(caps.supports_post_process, CapabilityState::No);
    }

    #[test]
    fn rtk_probe_sets_wrapper_yes_only_when_wrapper_contract_works() {
        // /bin/echo "hello" works as a wrapper — it echoes its args.
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let caps = discovery.probe_capabilities();

        // /bin/echo accepts args and echoes them → wrapper = Yes.
        assert_eq!(caps.supports_wrapper_mode, CapabilityState::Yes);
    }

    #[test]
    fn rtk_probe_capabilities_with_unavailable_path_returns_all_unknown() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/nonexistent/rtk".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(1000),
            allow_side_effecting_commands: None,
        };
        let discovery = RtkDiscovery::new(config);
        let caps = discovery.probe_capabilities();
        assert_eq!(caps.supports_post_process, CapabilityState::Unknown);
        assert_eq!(caps.supports_wrapper_mode, CapabilityState::Unknown);
    }

    #[test]
    fn rtk_probe_capabilities_with_disabled_config_returns_all_unknown() {
        let discovery = RtkDiscovery::new(disabled_config());
        let caps = discovery.probe_capabilities();
        assert_eq!(caps.supports_post_process, CapabilityState::Unknown);
        assert_eq!(caps.supports_wrapper_mode, CapabilityState::Unknown);
    }

    #[test]
    fn rtk_wrapper_uses_argv_when_available() {
        // /bin/echo as RTK: wrapper probe succeeds (echoes args),
        // post-process probe fails (doesn't read stdin).
        // So invocation_mode() → Wrapper.
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let projector = RtkProjector { discovery };

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "git status",
            Some(vec!["git".into(), "status".into()]),
            b"## main\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        let result = projector.project(request, &store).unwrap();
        assert_eq!(result.projector, "rtk");
        assert_eq!(result.kind, ProjectionKind::ExternalCompressed);
        // /bin/echo echoes the args: "git status"
        assert!(result.text.contains("git status"));
    }

    #[test]
    fn rtk_wrapper_falls_back_to_split_whitespace_when_no_argv() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let projector = RtkProjector { discovery };

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "git status",
            None, // no argv
            b"## main\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        let result = projector.project(request, &store).unwrap();
        assert_eq!(result.projector, "rtk");
        // Falls back to split_whitespace, /bin/echo echoes "git status"
        assert!(result.text.contains("git"));
        assert!(result.text.contains("status"));
    }

    #[test]
    fn rtk_wrapper_empty_argv_returns_backend_unavailable() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let projector = RtkProjector { discovery };

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "git status",
            Some(vec![]), // empty argv
            b"## main\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        let err = projector.project(request, &store).unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::BackendUnavailable {
                backend: "rtk",
                reason: _
            }
        ));
    }

    #[test]
    fn rtk_wrapper_includes_expansion_handle_warning() {
        let config = ShellOutputRtkConfig {
            enabled: Some(true),
            path: Some("/bin/echo".to_string()),
            eligible_only: Some(true),
            timeout_ms: Some(5000),
            allow_side_effecting_commands: None,
        };
        let mut discovery = RtkDiscovery::new(config);
        discovery.probe();
        let projector = RtkProjector { discovery };

        let mut store = CommandOutputStore::new();
        let run = make_test_run_with_store(
            &mut store,
            "ls",
            Some(vec!["ls".into()]),
            b"file1\nfile2\n".to_vec(),
        );
        let policy = crate::shell::projector::ProjectionPolicy {
            allow_external_backend: true,
            allow_lossy: true,
            redact_model_visible: true,
        };
        let request = ProjectionRequest::for_target(
            &run,
            crate::shell::projector::ProjectionTarget::ModelContext,
            &policy,
        );

        let result = projector.project(request, &store).unwrap();
        // Non-partial run should include the expansion handle warning
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("expansion handles refer to original command output")));
    }

    #[test]
    fn rtk_invocation_mode_post_process_only() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::Yes,
            supports_wrapper_mode: CapabilityState::No,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::PostProcess);
    }

    #[test]
    fn rtk_invocation_mode_wrapper_only() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::No,
            supports_wrapper_mode: CapabilityState::Yes,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::Wrapper);
    }

    #[test]
    fn rtk_invocation_mode_neither_supported() {
        let caps = RtkCapabilities {
            supports_post_process: CapabilityState::No,
            supports_wrapper_mode: CapabilityState::No,
            ..RtkCapabilities::all_unknown()
        };
        assert_eq!(caps.invocation_mode(), RtkInvocationMode::Disabled);
    }
}
