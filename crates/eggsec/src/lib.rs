//! Deterministic security scanning primitives.
//!
//! The crate exposes a small, self-contained API for classifying shell
//! commands, scanning text/files for secret and unsafe-code patterns, and
//! producing structured findings. Codegg wires this behind its native
//! `security` tool and gate policy.

pub mod command;
pub mod dependency;
pub mod finding;
pub mod profile;
pub mod scanner;

pub use command::{
    classify_bash_command, classify_git_subcommand, classify_tool_call, CommandClassification,
    CommandRisk,
};
pub use dependency::{detect_dependency_file, recommended_audit_commands, DependencyEcosystem};
pub use finding::{
    Confidence, FindingMode, FindingSource, SecurityCategory, SecurityFinding, SecurityReport,
    Severity,
};
pub use profile::{ProfileRunner, ProfileConfig, SecurityProfile};
pub use scanner::{inspect_file, inspect_text};

use thiserror::Error;

/// Errors returned by the `eggsec` API. Converted to `ToolError` at the
/// Codegg boundary.
#[derive(Debug, Error)]
pub enum EggsecError {
    #[error("io error: {0}")]
    Io(String),

    #[error("file too large: {0} bytes (max {1})")]
    FileTooLarge(u64, usize),

    #[error("task join error: {0}")]
    Join(String),
}
