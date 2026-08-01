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
use std::path::{Path, PathBuf};

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

impl ChildJobConfig {
    /// Validate the program-facing portion of a child request before it is
    /// translated into a scheduler payload.  This is deliberately stricter
    /// than the general managed-argv executor: a Tool Program is not allowed
    /// to turn a typed build/test operation into an arbitrary shell escape.
    pub fn validate(&self) -> Result<(), String> {
        const MAX_ARGV: usize = 32;
        const MAX_ARG_BYTES: usize = 4096;

        fn validate_argv(
            argv: &[String],
            op: ChildJobOp,
            max_args: usize,
            max_bytes: usize,
        ) -> Result<(), String> {
            if argv.is_empty() || argv.len() > max_args {
                return Err(format!("{op} argv must contain 1..={max_args} arguments"));
            }
            let bytes: usize = argv.iter().map(String::len).sum();
            if bytes > max_bytes {
                return Err(format!("{op} argv exceeds {max_bytes} bytes"));
            }
            if argv.iter().any(|arg| {
                arg.is_empty()
                    || arg.bytes().any(|byte| {
                        matches!(
                            byte,
                            b'\0' | b'\n' | b'\r' | b'|' | b'&' | b';' | b'<' | b'>'
                        )
                    })
            }) {
                return Err(format!("{op} argv contains an invalid argument"));
            }
            if argv[0] != "cargo" {
                return Err(format!("{op} only permits the cargo executable"));
            }
            let allowed_subcommand = match op {
                ChildJobOp::Build => {
                    matches!(argv.get(1).map(String::as_str), Some("build" | "check"))
                }
                ChildJobOp::Lint => {
                    matches!(argv.get(1).map(String::as_str), Some("clippy" | "check"))
                }
                ChildJobOp::Format => {
                    matches!(argv.get(1).map(String::as_str), Some("fmt"))
                        && argv.iter().any(|arg| arg == "--check")
                }
                ChildJobOp::Test => matches!(argv.get(1).map(String::as_str), Some("test")),
            };
            if !allowed_subcommand {
                return Err(format!("{op} command is not in the typed allowlist"));
            }
            if matches!(
                op,
                ChildJobOp::Build | ChildJobOp::Lint | ChildJobOp::Format
            ) && argv
                .iter()
                .any(|arg| matches!(arg.as_str(), "install" | "add" | "remove" | "publish"))
            {
                return Err(format!("{op} cannot install or publish dependencies"));
            }
            Ok(())
        }

        match self {
            Self::Test(config) => {
                let argv = ["cargo".to_string(), "test".to_string()];
                validate_argv(&argv, ChildJobOp::Test, MAX_ARGV, MAX_ARG_BYTES)?;
                validate_cwd(config.cwd.as_deref())?;
                validate_positive_timeout(config.timeout_secs, "test timeout")?;
                validate_positive_timeout(config.stall_timeout_secs, "test stall timeout")?;
                if let Some(scope) = &config.scope {
                    if !matches!(
                        scope.as_str(),
                        "workspace" | "package" | "file" | "previous_failures" | "custom"
                    ) {
                        return Err("test scope is not in the typed allowlist".into());
                    }
                }
            }
            Self::Build(config) => {
                let default = ["cargo".to_string(), "build".to_string()];
                let argv = config.argv.as_deref().unwrap_or(&default);
                validate_argv(argv, ChildJobOp::Build, MAX_ARGV, MAX_ARG_BYTES)?;
                validate_cwd(config.cwd.as_deref())?;
                validate_positive_timeout(config.timeout_secs, "build timeout")?;
            }
            Self::Lint(config) => {
                let default = ["cargo".to_string(), "clippy".to_string()];
                validate_argv(
                    config.argv.as_deref().unwrap_or(&default),
                    ChildJobOp::Lint,
                    MAX_ARGV,
                    MAX_ARG_BYTES,
                )?;
                validate_cwd(config.cwd.as_deref())?;
                validate_positive_timeout(config.timeout_secs, "lint timeout")?;
            }
            Self::Format(config) => {
                let default = [
                    "cargo".to_string(),
                    "fmt".to_string(),
                    "--".to_string(),
                    "--check".to_string(),
                ];
                validate_argv(
                    config.argv.as_deref().unwrap_or(&default),
                    ChildJobOp::Format,
                    MAX_ARGV,
                    MAX_ARG_BYTES,
                )?;
                validate_cwd(config.cwd.as_deref())?;
                validate_positive_timeout(config.timeout_secs, "format timeout")?;
            }
        }
        Ok(())
    }

    /// Resolve a workspace-relative cwd without allowing a program to escape
    /// the workspace authority granted to its parent job.
    pub fn resolve_cwd(&self, workspace_root: &Path) -> Result<Option<String>, String> {
        let requested = match self {
            Self::Test(c) => c.cwd.as_deref(),
            Self::Build(c) => c.cwd.as_deref(),
            Self::Lint(c) => c.cwd.as_deref(),
            Self::Format(c) => c.cwd.as_deref(),
        };
        let Some(requested) = requested else {
            return Ok(None);
        };
        let candidate = if Path::new(requested).is_absolute() {
            PathBuf::from(requested)
        } else {
            workspace_root.join(requested)
        };
        let root = workspace_root
            .canonicalize()
            .map_err(|e| format!("workspace root is unavailable: {e}"))?;
        let canonical = candidate
            .canonicalize()
            .map_err(|e| format!("child cwd is unavailable: {e}"))?;
        if !canonical.starts_with(&root) {
            return Err("child cwd escapes the workspace authority".into());
        }
        Ok(Some(canonical.to_string_lossy().into_owned()))
    }
}

fn validate_cwd(cwd: Option<&str>) -> Result<(), String> {
    if cwd.is_some_and(|value| value.is_empty() || value.contains('\0') || value.len() > 4096) {
        Err("child cwd is invalid".into())
    } else {
        Ok(())
    }
}

fn validate_positive_timeout(value: Option<u64>, label: &str) -> Result<(), String> {
    if value == Some(0) {
        Err(format!("{label} must be greater than zero"))
    } else {
        Ok(())
    }
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

    #[test]
    fn typed_commands_reject_shell_and_dependency_installation() {
        let shell = ChildJobConfig::Build(BuildJobConfig {
            argv: Some(vec![
                "sh".into(),
                "-c".into(),
                "cargo build; rm -rf .".into(),
            ]),
            ..Default::default()
        });
        assert!(shell.validate().is_err());

        let install = ChildJobConfig::Build(BuildJobConfig {
            argv: Some(vec!["cargo".into(), "install".into(), "evil".into()]),
            ..Default::default()
        });
        assert!(install.validate().is_err());
    }

    #[test]
    fn format_is_check_only() {
        let write = ChildJobConfig::Format(FormatJobConfig {
            argv: Some(vec!["cargo".into(), "fmt".into()]),
            ..Default::default()
        });
        assert!(write.validate().is_err());

        let check = ChildJobConfig::Format(FormatJobConfig {
            argv: Some(vec![
                "cargo".into(),
                "fmt".into(),
                "--".into(),
                "--check".into(),
            ]),
            ..Default::default()
        });
        assert!(check.validate().is_ok());
    }

    #[test]
    fn cwd_resolution_rejects_workspace_escape() {
        let config = ChildJobConfig::Build(BuildJobConfig {
            cwd: Some("..".into()),
            ..Default::default()
        });
        let root = std::env::current_dir().unwrap();
        assert!(config.resolve_cwd(&root).is_err());
    }
}
