//! Typed child-job request/result types for Tool Programs.
//!
//! Programs submit scheduler-owned build, test, lint, and format jobs
//! through the [`BrokerCallback::submit_child_job`] method. The broker
//! adapter translates these typed requests into canonical [`NewJob`]
//! submissions and waits for completion.
//!
//! # Invariants
//!
//! - Raw shell commands and arbitrary argv are never accepted.
//! - Child jobs inherit parent authority, workspace, and deadlines.
//! - Resource requests and exclusivity keys cannot be weakened by
//!   program-supplied input.
//! - Native typed projectors are preferred; RTK is a bounded fallback.

use serde::{Deserialize, Serialize};

/// The kind of child job a program may submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildJobOp {
    /// Canonical test execution (cargo test, pytest, etc.).
    Test,
    /// Build/compile operation (cargo build, make, etc.).
    Build,
    /// Lint/check operation (clippy, eslint, etc.).
    Lint,
    /// Format/check-format operation (cargo fmt --check, etc.).
    Format,
}

impl std::fmt::Display for ChildJobOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Build => write!(f, "build"),
            Self::Lint => write!(f, "lint"),
            Self::Format => write!(f, "format"),
        }
    }
}

impl std::str::FromStr for ChildJobOp {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "test" => Ok(Self::Test),
            "build" => Ok(Self::Build),
            "lint" => Ok(Self::Lint),
            "format" => Ok(Self::Format),
            _ => Err(format!(
                "unknown child job operation '{}': expected test, build, lint, or format",
                s
            )),
        }
    }
}

/// Configuration for a test child job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestJobConfig {
    /// Test scope: "workspace", "package", "file", "previous_failures", or "custom".
    #[serde(default)]
    pub scope: Option<String>,
    /// Working directory override.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Wall-clock timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Stall timeout in seconds.
    #[serde(default)]
    pub stall_timeout_secs: Option<u64>,
    /// Maximum report bytes.
    #[serde(default)]
    pub max_report_bytes: Option<usize>,
}

/// Configuration for a build child job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildJobConfig {
    /// Explicit argv for the build command (e.g. ["cargo", "build"]).
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    /// Working directory override.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Wall-clock timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Configuration for a lint child job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintJobConfig {
    /// Explicit argv for the lint command (e.g. ["cargo", "clippy"]).
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    /// Working directory override.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Wall-clock timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Configuration for a format child job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatJobConfig {
    /// Explicit argv for the format command (e.g. ["cargo", "fmt", "--check"]).
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    /// Working directory override.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Wall-clock timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Typed configuration for a child job operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildJobConfig {
    Test(TestJobConfig),
    Build(BuildJobConfig),
    Lint(LintJobConfig),
    Format(FormatJobConfig),
}

/// A typed child-job submission request.
///
/// Constructed by the interpreter from a `submit_job()` call and
/// passed to `BrokerCallback::submit_child_job`. The broker adapter
/// translates this into a canonical `NewJob` and submits it through
/// the `JobSubmissionService`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildJobRequest {
    /// Deterministic instruction sequence within the parent program attempt.
    /// This separates deliberate repeated operations from replay of one call.
    #[serde(default)]
    pub sequence: u32,
    /// The operation kind.
    pub op: ChildJobOp,
    /// Operation-specific configuration.
    pub config: ChildJobConfig,
}

/// Result of a completed child job.
///
/// Returned by `BrokerCallback::submit_child_job`. Contains the
/// structured status and operation-specific result data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildJobResult {
    /// Whether the child job completed successfully.
    pub success: bool,
    /// Exit code (if available).
    pub exit_code: Option<i32>,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Operation-specific result details.
    pub details: ChildJobDetails,
    /// Artifact handles for stdout/stderr/logs.
    pub artifacts: Vec<String>,
    /// Error message if the job failed.
    pub error: Option<String>,
}

/// Operation-specific result details for a child job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildJobDetails {
    /// Test-specific result data.
    Test(TestJobResult),
    /// Build-specific result data.
    Build(BuildJobResult),
    /// Lint-specific result data.
    Lint(LintJobResult),
    /// Format-specific result data.
    Format(FormatJobResult),
}

/// Test-specific result data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestJobResult {
    /// Test status: "passed", "failed", "skipped", "error".
    pub status: String,
    /// Test framework/runner name.
    pub framework: Option<String>,
    /// Total tests run.
    pub total: Option<u32>,
    /// Tests passed.
    pub passed: Option<u32>,
    /// Tests failed.
    pub failed: Option<u32>,
    /// Tests skipped.
    pub skipped: Option<u32>,
    /// Names of failed tests (bounded).
    #[serde(default)]
    pub failed_tests: Vec<String>,
    /// Concise failure evidence (bounded).
    #[serde(default)]
    pub failure_evidence: Vec<String>,
    /// Whether the test was cancelled.
    pub cancelled: bool,
    /// Whether stall/timeout occurred.
    pub timed_out: bool,
}

/// Build-specific result data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildJobResult {
    /// Build status: "success", "failure", "error".
    pub status: String,
    /// Command identity (argv joined).
    pub command: Option<String>,
    /// Diagnostics summary (errors/warnings count).
    pub diagnostics_errors: Option<u32>,
    pub diagnostics_warnings: Option<u32>,
    /// Changed files.
    #[serde(default)]
    pub changed_files: Vec<String>,
}

/// Lint-specific result data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintJobResult {
    /// Lint status: "clean", "warnings", "errors".
    pub status: String,
    /// Command identity.
    pub command: Option<String>,
    /// Lint diagnostics count.
    pub diagnostics_errors: Option<u32>,
    pub diagnostics_warnings: Option<u32>,
}

/// Format-specific result data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatJobResult {
    /// Format status: "clean", "needs_formatting", "error".
    pub status: String,
    /// Command identity.
    pub command: Option<String>,
    /// Whether files would change (check-only mode).
    pub would_change: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_job_op_parse() {
        assert_eq!("test".parse::<ChildJobOp>().unwrap(), ChildJobOp::Test);
        assert_eq!("build".parse::<ChildJobOp>().unwrap(), ChildJobOp::Build);
        assert_eq!("lint".parse::<ChildJobOp>().unwrap(), ChildJobOp::Lint);
        assert_eq!("format".parse::<ChildJobOp>().unwrap(), ChildJobOp::Format);
        assert!("unknown".parse::<ChildJobOp>().is_err());
    }

    #[test]
    fn child_job_request_serializes() {
        let req = ChildJobRequest {
            sequence: 7,
            op: ChildJobOp::Test,
            config: ChildJobConfig::Test(TestJobConfig {
                scope: Some("workspace".into()),
                timeout_secs: Some(120),
                ..Default::default()
            }),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["op"], "test");
        // Tagged enum: the config is under the "test" key (snake_case rename)
        assert_eq!(json["config"]["test"]["scope"], "workspace");
    }

    #[test]
    fn child_job_result_roundtrip() {
        let result = ChildJobResult {
            success: true,
            exit_code: Some(0),
            duration_ms: 1500,
            details: ChildJobDetails::Test(TestJobResult {
                status: "passed".into(),
                framework: Some("cargo".into()),
                total: Some(42),
                passed: Some(42),
                failed: Some(0),
                skipped: Some(0),
                failed_tests: vec![],
                failure_evidence: vec![],
                cancelled: false,
                timed_out: false,
            }),
            artifacts: vec!["ctx://logs/test-run-1".into()],
            error: None,
        };
        let json_str = serde_json::to_string(&result).unwrap();
        let back: ChildJobResult = serde_json::from_str(&json_str).unwrap();
        assert!(back.success);
        assert_eq!(back.exit_code, Some(0));
    }
}
