use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Capability profiles ─────────────────────────────────────────────

/// Requirement level for OS-level sandbox enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SandboxRequirement {
    /// No sandbox required (policy-only enforcement).
    None,
    /// Sandbox preferred but not mandatory; portable fallback acceptable.
    Preferred,
    /// OS-level sandbox required; deny if unavailable.
    Required,
}

/// Rule for允许的 subprocess invocations in Verify mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutableRule {
    /// Binary name or path prefix (e.g. "cargo", "pytest", "python3").
    pub command: String,
    /// Optional argument prefix constraints. Empty means any args allowed.
    pub arg_prefixes: Vec<String>,
    /// Human-readable reason this rule exists.
    pub reason: String,
}

impl ExecutableRule {
    pub fn new(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            arg_prefixes: Vec::new(),
            reason: reason.into(),
        }
    }

    pub fn with_arg_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.arg_prefixes.push(prefix.into());
        self
    }

    /// Check whether (command_name, first_arg) matches this rule.
    pub fn matches(&self, cmd: &str, first_arg: Option<&str>) -> bool {
        if cmd != self.command {
            return false;
        }
        if self.arg_prefixes.is_empty() {
            return true;
        }
        match first_arg {
            Some(arg) => self.arg_prefixes.iter().any(|p| arg.starts_with(p.as_str())),
            None => false,
        }
    }
}

/// Canonical capability profile for a Python script execution.
///
/// Profiles are deterministic given (mode, risk, context) and cannot be
/// silently widened by risk analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonCapabilityProfile {
    pub mode: PythonExecutionMode,
    /// Directories the script may read from.
    pub read_roots: Vec<PathBuf>,
    /// Directories the script may write to.
    pub write_roots: Vec<PathBuf>,
    /// Whether subprocess execution is allowed at all.
    pub allow_subprocess: bool,
    /// Specific subprocess rules (only checked when allow_subprocess is true).
    pub allowed_subprocesses: Vec<ExecutableRule>,
    /// Whether network access is allowed.
    pub allow_network: bool,
    /// Environment variables the script may access beyond the minimal allowlist.
    pub allow_env: Vec<String>,
    /// Whether dependency installation (pip/conda) is allowed.
    pub allow_dependency_install: bool,
    /// Whether destructive filesystem ops (unlink, rmdir, rmtree) are allowed.
    pub allow_destructive_fs: bool,
    /// Required sandbox enforcement level.
    pub sandbox_requirement: SandboxRequirement,
}

impl PythonCapabilityProfile {
    /// Build the default profile for Analyze mode.
    pub fn analyze(workspace_root: &PathBuf) -> Self {
        Self {
            mode: PythonExecutionMode::Analyze,
            read_roots: vec![workspace_root.clone()],
            write_roots: vec![], // no writes except codegg-managed temp
            allow_subprocess: false,
            allowed_subprocesses: vec![],
            allow_network: false,
            allow_env: vec![],
            allow_dependency_install: false,
            allow_destructive_fs: false,
            sandbox_requirement: SandboxRequirement::Preferred,
        }
    }

    /// Build the default profile for Transform mode.
    pub fn transform(workspace_root: &PathBuf) -> Self {
        Self {
            mode: PythonExecutionMode::Transform,
            read_roots: vec![workspace_root.clone()],
            write_roots: vec![workspace_root.clone()],
            allow_subprocess: false,
            allowed_subprocesses: vec![],
            allow_network: false,
            allow_env: vec![],
            allow_dependency_install: false,
            allow_destructive_fs: false,
            sandbox_requirement: SandboxRequirement::Preferred,
        }
    }

    /// Build the default profile for Verify mode.
    pub fn verify(workspace_root: &PathBuf) -> Self {
        Self {
            mode: PythonExecutionMode::Verify,
            read_roots: vec![workspace_root.clone()],
            write_roots: vec![], // no workspace writes
            allow_subprocess: true,
            allowed_subprocesses: vec![
                ExecutableRule::new("cargo", "cargo test runner"),
                ExecutableRule::new("cargo-test", "cargo test binary"),
                ExecutableRule::new("pytest", "pytest test runner"),
                ExecutableRule::new("python3", "python -m pytest")
                    .with_arg_prefix("-m"),
                ExecutableRule::new("go", "go test runner")
                    .with_arg_prefix("test"),
                ExecutableRule::new("make", "make test/build")
                    .with_arg_prefix("test"),
                ExecutableRule::new("make", "make build")
                    .with_arg_prefix("build"),
            ],
            allow_network: false,
            allow_env: vec![],
            allow_dependency_install: false,
            allow_destructive_fs: false,
            sandbox_requirement: SandboxRequirement::Preferred,
        }
    }

    /// Build profile from mode and workspace root, then deny capabilities
    /// flagged by risk analysis. Risk analysis can only narrow, never widen.
    pub fn from_mode_risk_and_context(
        mode: PythonExecutionMode,
        workspace_root: &PathBuf,
        risk: &PythonRiskAssessment,
    ) -> Self {
        let mut profile = match mode {
            PythonExecutionMode::Analyze => Self::analyze(workspace_root),
            PythonExecutionMode::Transform => Self::transform(workspace_root),
            PythonExecutionMode::Verify => Self::verify(workspace_root),
        };

        // Risk analysis can only deny capabilities, never grant
        if risk.has_network {
            profile.allow_network = false;
        }
        if risk.has_subprocess && mode != PythonExecutionMode::Verify {
            profile.allow_subprocess = false;
            profile.allowed_subprocesses.clear();
        }
        if risk.has_destructive_ops {
            profile.allow_destructive_fs = false;
        }
        if risk.has_dynamic_execution && mode == PythonExecutionMode::Verify {
            // Dynamic execution in verify mode is risky; deny subprocess
            profile.allow_subprocess = false;
            profile.allowed_subprocesses.clear();
        }

        profile
    }
}

// ── Sandbox enforcement ─────────────────────────────────────────────

/// Which enforcement backend is active for a given execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SandboxBackend {
    /// Landlock filesystem sandbox (Linux only).
    Landlock,
    /// Portable fallback: cwd containment, env clearing, snapshots.
    PortableFallback,
    /// No sandboxing active.
    None,
}

impl std::fmt::Display for SandboxBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Landlock => write!(f, "landlock"),
            Self::PortableFallback => write!(f, "portable_fallback"),
            Self::None => write!(f, "none"),
        }
    }
}

/// A specific capability that was denied by policy resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityViolation {
    /// Name of the denied capability (e.g. "network", "subprocess", "write_workspace").
    pub capability: String,
    /// Reason for denial.
    pub reason: String,
}

/// Result of the policy resolution step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonPolicyDecision {
    /// The resolved capability profile.
    pub profile: PythonCapabilityProfile,
    /// Capabilities denied by policy (before execution).
    pub denied: Vec<CapabilityViolation>,
    /// Non-fatal warnings from policy resolution.
    pub warnings: Vec<String>,
    /// Which enforcement backend is active.
    pub enforcement_backend: SandboxBackend,
    /// Whether OS-level filesystem isolation is active.
    pub os_filesystem_isolation: bool,
    /// Whether network isolation is active (always false until Landlock network support).
    pub os_network_isolation: bool,
}

/// Execution mode for Python scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PythonExecutionMode {
    /// Read-only analysis. May not write workspace files.
    Analyze,
    /// Workspace-limited write mode. Captures diffs and changed files.
    Transform,
    /// Read mode plus controlled subprocess capability.
    Verify,
}

impl PythonExecutionMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Analyze => "analyze",
            Self::Transform => "transform",
            Self::Verify => "verify",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Analyze => "Read-only analysis of data or code",
            Self::Transform => "Mutating script that may change workspace files",
            Self::Verify => "Test/verification script with controlled subprocess",
        }
    }
}

impl std::fmt::Display for PythonExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// How the script source is provided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonScriptSource {
    Inline(String),
    FilePath(PathBuf),
}

impl PythonScriptSource {
    pub fn code(&self) -> &str {
        match self {
            Self::Inline(code) => code,
            Self::FilePath(_) => "",
        }
    }
}

/// Capability envelope derived from mode + static risk analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonCapabilityEnvelope {
    pub read_workspace: bool,
    pub write_workspace: bool,
    pub read_outside_workspace: bool,
    pub write_outside_workspace: bool,
    pub subprocess: bool,
    pub network: bool,
    pub env_access: bool,
    pub dependency_install: bool,
    pub destructive_fs: bool,
}

impl PythonCapabilityEnvelope {
    /// Default envelope for Analyze mode: read workspace only.
    pub fn analyze() -> Self {
        Self {
            read_workspace: true,
            write_workspace: false,
            read_outside_workspace: false,
            write_outside_workspace: false,
            subprocess: false,
            network: false,
            env_access: false,
            dependency_install: false,
            destructive_fs: false,
        }
    }

    /// Default envelope for Transform mode: read + write workspace.
    pub fn transform() -> Self {
        Self {
            read_workspace: true,
            write_workspace: true,
            read_outside_workspace: false,
            write_outside_workspace: false,
            subprocess: false,
            network: false,
            env_access: false,
            dependency_install: false,
            destructive_fs: false,
        }
    }

    /// Default envelope for Verify mode: read workspace + supervised subprocess.
    pub fn verify() -> Self {
        Self {
            read_workspace: true,
            write_workspace: false,
            read_outside_workspace: false,
            write_outside_workspace: false,
            subprocess: true,
            network: false,
            env_access: false,
            dependency_install: false,
            destructive_fs: false,
        }
    }

    /// Build envelope from mode, then deny capabilities flagged by risk analysis.
    pub fn from_mode_and_risk(mode: PythonExecutionMode, risk: &PythonRiskAssessment) -> Self {
        let mut env = match mode {
            PythonExecutionMode::Analyze => Self::analyze(),
            PythonExecutionMode::Transform => Self::transform(),
            PythonExecutionMode::Verify => Self::verify(),
        };

        // Deny capabilities that risk analysis flagged
        if risk.has_network {
            env.network = false;
        }
        if risk.has_subprocess && mode != PythonExecutionMode::Verify {
            env.subprocess = false;
        }
        if risk.has_destructive_ops {
            env.destructive_fs = false;
        }
        // In Analyze mode, file reads are allowed (read_workspace=true), but writes are denied
        if risk.has_file_write && mode == PythonExecutionMode::Analyze {
            env.write_workspace = false;
        }

        env
    }

    /// Returns true if any denied capability is needed by the risk analysis.
    pub fn has_denied_capabilities(&self, risk: &PythonRiskAssessment) -> Vec<String> {
        let mut denied = Vec::new();
        if risk.has_network && !self.network {
            denied.push("network".to_string());
        }
        if risk.has_subprocess && !self.subprocess {
            denied.push("subprocess".to_string());
        }
        if risk.has_destructive_ops && !self.destructive_fs {
            denied.push("destructive_fs".to_string());
        }
        if risk.has_file_read && !self.read_workspace {
            denied.push("read_workspace".to_string());
        }
        if risk.has_file_write && !self.write_workspace {
            denied.push("write_workspace".to_string());
        }
        denied
    }
}

/// Risk level from static analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PythonRiskLevel {
    Safe,
    Low,
    Medium,
    High,
}

/// Which scanner produced this risk assessment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PythonRiskScanner {
    /// String/line scanning (fallback).
    Fallback,
    /// AST-aware scanning via `python3 -I`.
    Ast,
}

/// Static risk assessment of a Python script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonRiskAssessment {
    pub level: PythonRiskLevel,
    pub reasons: Vec<String>,
    pub has_file_io: bool,
    pub has_file_read: bool,
    pub has_file_write: bool,
    pub has_subprocess: bool,
    pub has_network: bool,
    pub has_destructive_ops: bool,
    pub has_dynamic_execution: bool,
    pub imports: Vec<String>,
    pub scanner: PythonRiskScanner,
}

impl PythonRiskAssessment {
    pub fn safe() -> Self {
        Self {
            level: PythonRiskLevel::Safe,
            reasons: vec![],
            has_file_io: false,
            has_file_read: false,
            has_file_write: false,
            has_subprocess: false,
            has_network: false,
            has_destructive_ops: false,
            has_dynamic_execution: false,
            imports: vec![],
            scanner: PythonRiskScanner::Fallback,
        }
    }

    pub fn requires_permission(&self) -> bool {
        matches!(self.level, PythonRiskLevel::Medium | PythonRiskLevel::High)
    }
}

/// Request to execute a Python script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonScriptRequest {
    pub code: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    #[serde(default)]
    pub workspace_root: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
    pub session_id: Option<String>,
    pub intent: Option<String>,
}

/// Status of a completed Python run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonRunStatus {
    Success,
    Failed(i32),
    TimedOut,
    SpawnError,
}

impl PythonRunStatus {
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Success => Some(0),
            Self::Failed(code) => Some(*code),
            _ => None,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed(_) => "failed",
            Self::TimedOut => "timed_out",
            Self::SpawnError => "spawn_error",
        }
    }
}

/// Result of executing a Python script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonRunResult {
    pub status: PythonRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: std::time::Duration,
    pub mode: PythonExecutionMode,
    pub script_length: usize,
    pub risk: PythonRiskAssessment,
    pub capabilities: PythonCapabilityEnvelope,
    pub changed_files: Vec<PathBuf>,
    pub interpreter: String,
    /// Textual diff for Transform mode changed files (unified diff format).
    pub diff: Option<String>,
    /// SHA-256 hex digest of the script source body.
    pub script_body_hash: Option<String>,
    /// Pseudo-local label for stdout (not registered in any artifact store; non-resolvable).
    pub stdout_label: Option<String>,
    /// Pseudo-local label for stderr (not registered in any artifact store; non-resolvable).
    pub stderr_label: Option<String>,
    /// Pseudo-local label for diff (not registered in any artifact store; non-resolvable).
    pub diff_label: Option<String>,
    // ── Enforcement evidence (Phase 06) ──────────────────────────────
    /// The policy decision that governed this execution.
    #[serde(default)]
    pub policy_decision: Option<PythonPolicyDecision>,
    /// Capabilities that were denied before execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_capabilities: Vec<String>,
    /// Whether OS-level filesystem isolation was active.
    #[serde(default)]
    pub os_filesystem_isolation: bool,
    /// Whether network isolation was active.
    #[serde(default)]
    pub os_network_isolation: bool,
    /// Allowed read roots for this execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_read_roots: Vec<PathBuf>,
    /// Allowed write roots for this execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_write_roots: Vec<PathBuf>,
    /// Subprocess rules that were active (Verify mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_subprocesses: Vec<ExecutableRule>,
    /// Enforcement warnings from policy resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforcement_warnings: Vec<String>,
}

impl PythonRunResult {
    pub fn exit_code(&self) -> Option<i32> {
        self.status.exit_code()
    }

    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Format a compact model-facing summary.
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("mode: {}", self.mode),
            format!("status: {}", self.status.label()),
        ];
        if let Some(code) = self.exit_code() {
            parts.push(format!("exit: {code}"));
        }
        parts.push(format!("duration: {:.1?}", self.duration));
        if !self.risk.reasons.is_empty() {
            parts.push(format!("risk: {}", self.risk.reasons.join(", ")));
        }
        if !self.changed_files.is_empty() {
            parts.push(format!("changed: {} files", self.changed_files.len()));
        }
        if let Some(ref hash) = self.script_body_hash {
            parts.push(format!("script_hash: {hash}"));
        }
        if self.diff.is_some() {
            parts.push("diff: available".to_string());
        }
        // Enforcement evidence
        if !self.denied_capabilities.is_empty() {
            parts.push(format!("denied: {}", self.denied_capabilities.join(", ")));
        }
        if self.os_filesystem_isolation {
            parts.push("sandbox: os".to_string());
        } else if self.policy_decision.is_some() {
            parts.push("sandbox: portable".to_string());
        }
        if !self.enforcement_warnings.is_empty() {
            parts.push(format!(
                "warnings: {}",
                self.enforcement_warnings.join(", ")
            ));
        }
        parts.join(", ")
    }
}
