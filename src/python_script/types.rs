use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
        if risk.has_file_io && mode == PythonExecutionMode::Analyze {
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
        if risk.has_file_io && !self.write_workspace {
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
        parts.join(", ")
    }
}
