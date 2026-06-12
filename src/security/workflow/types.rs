use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Finding model enums
// ---------------------------------------------------------------------------

/// Severity level for evidence-based security findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for SecuritySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Confidence level for evidence-based security findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SecurityConfidence {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for SecurityConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

/// The source kind of a piece of evidence supporting a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityEvidenceKind {
    ChangedHunk,
    RiskMarker,
    Diagnostic,
    CallPath,
    Preflight,
    CodeReasoning,
    TruncationNotice,
}

/// A single piece of structured evidence supporting a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredSecurityEvidence {
    pub kind: SecurityEvidenceKind,
    pub file_path: Option<PathBuf>,
    pub line: Option<u32>,
    pub summary: String,
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Why a file/location was selected as a security review target.
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityTargetReason {
    ChangedHunk,
    DependencyMetadata,
    RiskMarker,
    PublicBoundary,
    UnsafeCode,
    ProcessExecution,
    FilesystemAccess,
    NetworkBoundary,
    AuthOrSecretHandling,
    Unknown,
}

/// A file/location selected for security review.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewTarget {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub preset: String,
    pub reason: SecurityTargetReason,
}

impl SecurityReviewTarget {
    /// Human-readable string for the target reason.
    pub fn reason_str(&self) -> &str {
        match self.reason {
            SecurityTargetReason::ChangedHunk => "changed hunk",
            SecurityTargetReason::DependencyMetadata => "dependency metadata",
            SecurityTargetReason::RiskMarker => "risk marker",
            SecurityTargetReason::PublicBoundary => "public boundary",
            SecurityTargetReason::UnsafeCode => "unsafe code",
            SecurityTargetReason::ProcessExecution => "process execution",
            SecurityTargetReason::FilesystemAccess => "filesystem access",
            SecurityTargetReason::NetworkBoundary => "network boundary",
            SecurityTargetReason::AuthOrSecretHandling => "auth/secret handling",
            SecurityTargetReason::Unknown => "unknown",
        }
    }
}

/// A parsed hunk from a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedHunk {
    pub file_path: PathBuf,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    /// Parsed diff lines (when available from full diff text parsing).
    pub lines: Vec<DiffLine>,
}

/// A single parsed line from a unified diff hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

/// Kind of a parsed diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineKind {
    Added,
    Removed,
    Context,
}

/// A compact hunk context for TUI display, carrying parsed diff lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityReviewHunkRef {
    pub file_path: PathBuf,
    pub old_start: Option<u32>,
    pub old_lines: Option<u32>,
    pub new_start: Option<u32>,
    pub new_lines: Option<u32>,
    pub header: String,
    pub lines: Vec<SecurityReviewHunkLine>,
}

/// A single line within a hunk, with line numbers and kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityReviewHunkLine {
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: SecurityReviewHunkLineKind,
    pub text: String,
    pub is_focus: bool,
}

/// Whether a hunk line is added, removed, or unchanged context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityReviewHunkLineKind {
    Added,
    Removed,
    Context,
}

/// Structured preflight evidence with file path and optional line number.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityPreflightEvidence {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub summary: String,
    pub detail: Option<String>,
}

/// Deterministic preflight check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityPreflightResult {
    pub check_name: String,
    pub status: PreflightStatus,
    pub evidence: Vec<String>,
    pub structured_evidence: Vec<SecurityPreflightEvidence>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PreflightStatus {
    Pass,
    Fail,
    Warn,
    Skipped,
}

/// Legacy evidence type (kept for backward compatibility with existing tests).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvidence {
    pub location: String,
    pub description: String,
}

/// An evidence-based security finding produced by conservative synthesis.
///
/// Risk markers alone never produce findings — additional evidence is
/// required.  Severity and confidence are deterministic enums.  Findings
/// are defensive review outputs, not proof of exploitability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityReviewFinding {
    pub severity: SecuritySeverity,
    pub confidence: SecurityConfidence,
    pub title: String,
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub category: Option<String>,
    pub evidence: Vec<StructuredSecurityEvidence>,
    pub reasoning: String,
    pub recommendation: String,
    pub tests: Vec<String>,
}

/// A review prompt derived from risk markers (not a confirmed finding).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewPrompt {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub preset: String,
    pub category: Option<String>,
    pub title: String,
    pub rationale: String,
    pub evidence: Vec<String>,
}

/// Simplified risk marker used by the workflow (avoids importing LSP types).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRiskMarkerFromWorkflow {
    pub category: String,
    pub label: String,
    pub file_path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub matched_text: String,
    pub rationale: String,
}

/// Placeholder for future finding synthesis.  Empty in this vertical slice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewFindingStub {
    pub title: String,
    pub note: String,
}

/// Stable output shape for the security review workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewReport {
    pub targets: Vec<SecurityReviewTarget>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub findings: Vec<SecurityReviewFindingStub>,
    pub notes: Vec<String>,
}

/// Complete output from the full security review workflow (includes
/// preflight results and evidence-based findings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityReviewOutput {
    pub targets: Vec<SecurityReviewTarget>,
    pub findings: Vec<SecurityReviewFinding>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub preflight_results: Vec<SecurityPreflightResult>,
    pub notes: Vec<String>,
    /// Parsed diff hunks with line-level detail, for TUI hunk display.
    pub hunks: Vec<SecurityReviewHunkRef>,
}

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
pub use SecurityPreflightResult as PreflightResult;
#[allow(unused_imports)]
pub use SecurityReviewFinding as ReviewFinding;
#[allow(unused_imports)]
pub use SecurityReviewOutput as ReviewOutput;
#[allow(unused_imports)]
pub use SecurityReviewPrompt as ReviewPrompt;
#[allow(unused_imports)]
pub use SecurityReviewTarget as ReviewTarget;
#[allow(unused_imports)]
pub use SecurityTargetReason as TargetReason;
#[allow(unused_imports)]
pub use StructuredSecurityEvidence as Evidence;
