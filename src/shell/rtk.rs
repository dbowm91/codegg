use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use codegg_config::schema::ShellOutputRtkConfig;

use crate::shell::projection::CommandOutputStore;
use crate::shell::projector::{
    CommandOutputProjector, ProjectionError, ProjectionRequest, ProjectionResult, ProjectionSupport,
};

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
        _store: &CommandOutputStore,
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

        // Phase 5 skeleton: RTK compression is not yet implemented.
        // Return a recoverable error so the selector falls back to safe
        // native/generic projection rather than producing fake output.
        Err(ProjectionError::BackendUnavailable {
            backend: "rtk",
            reason: "RTK compression not yet implemented (Phase 5 skeleton)".into(),
        })
    }
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
    fn rtk_projector_skeleton_returns_backend_unavailable() {
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
        // Verify no fake output is produced — the error message must not
        // contain the old placeholder text.
        let err_str = err.to_string();
        assert!(!err_str.contains("[RTK backend skeleton]"));
        assert!(err_str.contains("not yet implemented"));
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
}
