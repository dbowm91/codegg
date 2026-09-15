//! Deterministic security scanning primitives.
//!
//! The crate exposes a small, self-contained API for classifying shell
//! commands, scanning text/files for secret and unsafe-code patterns,
//! classifying dependency files, and producing structured findings. All
//! scanners are deterministic library primitives: the same input bytes
//! produce the same findings and stable finding IDs, with no network,
//! clock, or host-service dependency.
//!
//! ## Stable identifiers
//!
//! Downstream consumers must match on the typed values, not on human
//! diagnostic strings:
//!
//! - [`finding::SecurityCategory`], [`finding::Severity`],
//!   [`finding::Confidence`], [`finding::FindingSource`],
//!   [`finding::FindingMode`], [`command::CommandRisk`],
//!   [`dependency::DependencyEcosystem`], and [`profile::SecurityProfile`]
//!   serialize as `snake_case` strings. Those strings (and
//!   [`finding::SecurityCategory::label`]/[`profile::SecurityProfile::as_str`])
//!   are the stable wire identifiers.
//! - `evidence`, `reasons`, `recommendation`, and `summary` are human
//!   diagnostics and are not identifiers.
//! - Finding `id`s from [`finding::make_finding_id`] and
//!   [`finding::SecurityFinding::deterministic_id`] are stable hashes over
//!   `(prefix, category, evidence/context, line)`.
//!
//! ## Versioning
//!
//! Adding a new category, severity-adjacent rule, ecosystem, or profile
//! variant is a minor change. Renaming or removing an existing wire
//! identifier, or changing the meaning of an existing risk mapping, is a
//! major change. Rule-pattern refinements that keep the same identifiers
//! are patch-level.
//!
//! Host orchestration (tools, gates, approvals, daemon wiring) lives
//! outside this crate. One consumer wires these primitives behind a
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
pub use profile::{ProfileConfig, ProfileRunner, SecurityProfile};
pub use scanner::{inspect_file, inspect_text};

use thiserror::Error;

/// Errors returned by the `eggsentry` API. Converted to `ToolError` at the
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
