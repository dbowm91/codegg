//! Durable Tool Program domain model, source/IR storage, and call ledger.
//!
//! This module defines the persistent representation of a Tool Program:
//! its identity, lifecycle state, frozen capability manifest,
//! content-addressed source/IR references, checkpoints, nested-call
//! records, and terminal results.
//!
//! # Invariants
//!
//! - Program source and compiled IR are immutable and content-addressed.
//! - A capability manifest is frozen at submission and cannot expand
//!   while running.
//! - Nested-call arguments/results are bounded, redactable, and
//!   artifact-backed when large.
//! - Storage does not contain credentials or hidden reasoning.
//! - Unknown future variants remain inspectable but never execute
//!   under older code.
//! - State transitions are intent-named and validated; generic
//!   arbitrary state mutation is prohibited.
//! - Program storage cannot become a second scheduler or RunStore.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod content_store;
pub mod store;

pub use content_store::{ContentAddressedStore, ContentStoreError, InMemoryContentStore};
pub use store::{
    InMemoryToolProgramStore, ProgramStoreError, ProgramStoreQuery, ProgramSummary,
    ToolProgramStore,
};

/// Opaque, stable identifier for a durable Tool Program.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolProgramId(String);

impl ToolProgramId {
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolProgramId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, stable identifier for a nested call within a Tool Program.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProgramCallId(String);

impl ProgramCallId {
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProgramCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ─── Program language ─────────────────────────────────────────────

/// Language used for a Tool Program source. Forward-compatible:
/// unknown variants are persisted but block execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramLanguage {
    /// Restricted Python subset (M004+).
    RestrictedPython,
    /// Unknown future language; persisted but never executed.
    #[serde(other)]
    Unknown,
}

impl ProgramLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProgramLanguage::RestrictedPython => "restricted_python",
            ProgramLanguage::Unknown => "unknown",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "restricted_python" => ProgramLanguage::RestrictedPython,
            _ => ProgramLanguage::Unknown,
        }
    }
}

// ─── Program state ────────────────────────────────────────────────

/// Logical state of a Tool Program. Terminal states never regress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProgramState {
    /// Program record created; not yet submitted to scheduler.
    Submitted,
    /// Submitted and waiting for scheduler admission.
    Queued,
    /// Compiling source to IR (M004+).
    Compiling,
    /// Actively executing under scheduler ownership.
    Running,
    /// Waiting for a child job or external dependency.
    Waiting,
    /// In backoff before a retry attempt.
    RetryBackoff,
    /// Successfully completed.
    Completed,
    /// Partially completed; some calls may have succeeded.
    Incomplete,
    /// Execution failed.
    Failed,
    /// Cancelled by user or parent.
    Cancelled,
    /// Execution exceeded deadline.
    TimedOut,
    /// Heartbeat expired; possible worker loss.
    Stalled,
    /// Interrupted by daemon generation recovery.
    Interrupted,
    /// Blocked: unknown version, unavailable executor, or dependency.
    Blocked,
}

impl ToolProgramState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Queued => "queued",
            Self::Compiling => "compiling",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::RetryBackoff => "retry_backoff",
            Self::Completed => "completed",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Stalled => "stalled",
            Self::Interrupted => "interrupted",
            Self::Blocked => "blocked",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "submitted" => Self::Submitted,
            "queued" => Self::Queued,
            "compiling" => Self::Compiling,
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "retry_backoff" => Self::RetryBackoff,
            "completed" => Self::Completed,
            "incomplete" => Self::Incomplete,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "timed_out" => Self::TimedOut,
            "stalled" => Self::Stalled,
            "interrupted" => Self::Interrupted,
            "blocked" => Self::Blocked,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Incomplete
                | Self::Failed
                | Self::Cancelled
                | Self::TimedOut
                | Self::Blocked
        )
    }
}

/// Validate a program state transition. Returns `Ok(())` if the
/// transition is allowed, `Err(from, to)` otherwise.
pub fn validate_program_transition(
    from: ToolProgramState,
    to: ToolProgramState,
) -> Result<(), (ToolProgramState, ToolProgramState)> {
    let allowed = matches!(
        (from, to),
        // Initial submission
        (ToolProgramState::Submitted, ToolProgramState::Queued)
            | (ToolProgramState::Submitted, ToolProgramState::Blocked)
            | (ToolProgramState::Submitted, ToolProgramState::Failed)
            | (ToolProgramState::Submitted, ToolProgramState::Cancelled)
            // Queued
            | (ToolProgramState::Queued, ToolProgramState::Compiling)
            | (ToolProgramState::Queued, ToolProgramState::Running)
            | (ToolProgramState::Queued, ToolProgramState::Blocked)
            | (ToolProgramState::Queued, ToolProgramState::Failed)
            | (ToolProgramState::Queued, ToolProgramState::Cancelled)
            // Compiling
            | (ToolProgramState::Compiling, ToolProgramState::Running)
            | (ToolProgramState::Compiling, ToolProgramState::Failed)
            | (ToolProgramState::Compiling, ToolProgramState::Cancelled)
            // Running
            | (ToolProgramState::Running, ToolProgramState::Waiting)
            | (ToolProgramState::Running, ToolProgramState::RetryBackoff)
            | (ToolProgramState::Running, ToolProgramState::Completed)
            | (ToolProgramState::Running, ToolProgramState::Incomplete)
            | (ToolProgramState::Running, ToolProgramState::Failed)
            | (ToolProgramState::Running, ToolProgramState::Cancelled)
            | (ToolProgramState::Running, ToolProgramState::TimedOut)
            | (ToolProgramState::Running, ToolProgramState::Stalled)
            | (ToolProgramState::Running, ToolProgramState::Interrupted)
            // Waiting
            | (ToolProgramState::Waiting, ToolProgramState::Running)
            | (ToolProgramState::Waiting, ToolProgramState::RetryBackoff)
            | (ToolProgramState::Waiting, ToolProgramState::Completed)
            | (ToolProgramState::Waiting, ToolProgramState::Incomplete)
            | (ToolProgramState::Waiting, ToolProgramState::Failed)
            | (ToolProgramState::Waiting, ToolProgramState::Cancelled)
            | (ToolProgramState::Waiting, ToolProgramState::TimedOut)
            | (ToolProgramState::Waiting, ToolProgramState::Stalled)
            | (ToolProgramState::Waiting, ToolProgramState::Interrupted)
            // RetryBackoff
            | (ToolProgramState::RetryBackoff, ToolProgramState::Running)
            | (ToolProgramState::RetryBackoff, ToolProgramState::Failed)
            | (ToolProgramState::RetryBackoff, ToolProgramState::Cancelled)
            // Stalled
            | (ToolProgramState::Stalled, ToolProgramState::Running)
            | (ToolProgramState::Stalled, ToolProgramState::Failed)
            | (ToolProgramState::Stalled, ToolProgramState::TimedOut)
            | (ToolProgramState::Stalled, ToolProgramState::Interrupted)
            // Interrupted (daemon recovery)
            | (ToolProgramState::Interrupted, ToolProgramState::Queued)
            | (ToolProgramState::Interrupted, ToolProgramState::Failed)
    );
    if allowed {
        Ok(())
    } else {
        Err((from, to))
    }
}

// ─── Call state ───────────────────────────────────────────────────

/// State of a single nested call within a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramCallState {
    /// Call slot reserved but not yet dispatched.
    Reserved,
    /// Call is actively executing.
    Running,
    /// Call completed successfully.
    Completed,
    /// Call failed.
    Failed,
    /// Call was cancelled.
    Cancelled,
    /// Call timed out.
    TimedOut,
}

impl ProgramCallState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "reserved" => Self::Reserved,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "timed_out" => Self::TimedOut,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

// ─── Failure and recovery ─────────────────────────────────────────

/// Classification of program failures for retry and recovery policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramFailureClass {
    /// Input validation or schema mismatch.
    Validation,
    /// Tool returned an error.
    ToolError,
    /// Authority or permission denied.
    Permission,
    /// Resource limit exceeded (budget, memory, etc).
    ResourceExhausted,
    /// Infrastructure failure (storage, network, process).
    Infrastructure,
    /// Unknown/unclassified failure.
    Unknown,
}

impl ProgramFailureClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::ToolError => "tool_error",
            Self::Permission => "permission",
            Self::ResourceExhausted => "resource_exhausted",
            Self::Infrastructure => "infrastructure",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "validation" => Self::Validation,
            "tool_error" => Self::ToolError,
            "permission" => Self::Permission,
            "resource_exhausted" => Self::ResourceExhausted,
            "infrastructure" => Self::Infrastructure,
            _ => Self::Unknown,
        }
    }
}

/// Disposition of a completed or failed call on retry/restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayDisposition {
    /// Call completed; replay the result from the ledger.
    Replay,
    /// Call was non-idempotent or failed; must re-execute.
    Reexecute,
    /// Call was cancelled; skip on replay.
    Skip,
}

impl ReplayDisposition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Reexecute => "reexecute",
            Self::Skip => "skip",
        }
    }
}

// ─── Source and IR references ─────────────────────────────────────

/// Content-addressed reference to immutable program source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSourceRef {
    /// SHA-256 hex digest of the source content.
    pub digest: String,
    /// Byte length of the source content.
    pub byte_length: u64,
    /// Schema/format version of the source encoding.
    pub schema_version: u32,
    /// Opaque content location (store key or path).
    pub content_location: String,
}

/// Content-addressed reference to compiled IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramIrRef {
    /// SHA-256 hex digest of the IR content.
    pub digest: String,
    /// Byte length of the IR content.
    pub byte_length: u64,
    /// IR format version (incremented on IR format changes).
    pub ir_version: u32,
    /// Opaque content location (store key or path).
    pub content_location: String,
}

// ─── Capability manifest ──────────────────────────────────────────

/// Frozen snapshot of callable tool contracts for a program.
/// Resolved at submission time from the ToolBroker catalog; immutable
/// for the program's lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramCapabilityManifest {
    /// Version of the manifest format.
    pub manifest_version: u32,
    /// Tool contracts available to this program, keyed by tool name.
    /// Each entry is a JSON-serialized `ToolContract`.
    pub tools: HashMap<String, serde_json::Value>,
    /// Maximum number of concurrent inline tool calls.
    pub max_concurrent_calls: u32,
    /// Maximum total tool calls across the program's lifetime.
    pub max_total_calls: u32,
    /// Authority digest: hash of the submitting session/agent
    /// authority context. Programs cannot escalate beyond this.
    pub authority_digest: String,
    /// Whether mutation-capable tools are permitted (version 1: false).
    pub allow_mutations: bool,
    /// Resource budget snapshot at submission time.
    pub resource_limits: ProgramLimitsSnapshot,
}

/// Persisted budget limits for a program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramLimitsSnapshot {
    /// Maximum execution time in seconds.
    pub max_timeout_secs: u64,
    /// Maximum memory in megabytes.
    pub max_memory_mb: u64,
    /// Maximum number of steps/instructions.
    pub max_steps: u64,
    /// Maximum source code bytes.
    pub max_source_bytes: u64,
    /// Maximum IR bytes.
    pub max_ir_bytes: u64,
    /// Maximum checkpoint size in bytes.
    pub max_checkpoint_bytes: u64,
    /// Maximum single call result bytes.
    pub max_call_result_bytes: u64,
    /// Maximum total result bytes.
    pub max_total_result_bytes: u64,
}

impl Default for ProgramLimitsSnapshot {
    fn default() -> Self {
        Self {
            max_timeout_secs: 300,
            max_memory_mb: 512,
            max_steps: 100_000,
            max_source_bytes: 64 * 1024,
            max_ir_bytes: 256 * 1024,
            max_checkpoint_bytes: 64 * 1024,
            max_call_result_bytes: 1024 * 1024,
            max_total_result_bytes: 10 * 1024 * 1024,
        }
    }
}

// ─── Checkpoint ───────────────────────────────────────────────────

/// Deterministic interpreter position, preserved for restart replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramCheckpoint {
    /// IR version this checkpoint was created from.
    pub ir_version: u32,
    /// Hash of the IR this checkpoint was created from.
    pub ir_hash: String,
    /// Instruction cursor (byte offset or instruction index).
    pub instruction_cursor: u64,
    /// Nested loop frame stack (depth, iteration counter).
    pub loop_frames: Vec<LoopFrame>,
    /// Number of calls completed so far (used as replay cursor).
    pub completed_call_cursor: u32,
    /// Remaining step budget.
    pub remaining_steps: u64,
    /// Remaining time budget in milliseconds.
    pub remaining_time_ms: u64,
    /// Deterministic local variable snapshot (bounded, redactable).
    pub local_values: HashMap<String, serde_json::Value>,
}

/// A single loop frame in the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopFrame {
    /// Nesting depth (0 = top level).
    pub depth: u32,
    /// Current iteration index.
    pub iteration: u64,
    /// Maximum iterations (from static bounds).
    pub max_iterations: u64,
}

// ─── Call record ──────────────────────────────────────────────────

/// Durable record of a single nested tool call within a program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramCallRecord {
    /// Unique call identifier.
    pub call_id: ProgramCallId,
    /// Monotonically increasing sequence within the program.
    pub sequence: u32,
    /// Tool name invoked.
    pub tool_name: String,
    /// Hash of the frozen tool contract at call time.
    pub tool_contract_hash: String,
    /// SHA-256 hash of the normalized input arguments.
    pub normalized_input_hash: String,
    /// Current call state.
    pub state: ProgramCallState,
    /// Number of execution attempts for this call.
    pub attempts: u32,
    /// Optional child job ID (for scheduler-dispatched calls).
    pub child_job_id: Option<String>,
    /// Optional child run ID (for artifact-backed calls).
    pub child_run_id: Option<String>,
    /// Artifact handles for large result bodies.
    pub result_artifacts: Vec<CallArtifactRef>,
    /// Bounded inline result projection (truncated if large).
    pub result_projection: Option<String>,
    /// Failure classification (when state is terminal-failure).
    pub failure_class: Option<ProgramFailureClass>,
    /// Error message (when state is terminal-failure).
    pub error_message: Option<String>,
    /// Replay disposition for restart/retry.
    pub replay_disposition: ReplayDisposition,
    /// When the call was created.
    pub created_at: DateTime<Utc>,
    /// When the call last transitioned.
    pub updated_at: DateTime<Utc>,
    /// When the call reached a terminal state.
    pub terminal_at: Option<DateTime<Utc>>,
}

/// Reference to an artifact produced by a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallArtifactRef {
    /// Artifact identifier.
    pub artifact_id: String,
    /// Content type (MIME).
    pub content_type: String,
    /// Byte length.
    pub byte_length: u64,
    /// Content digest.
    pub digest: Option<String>,
}

// ─── Program result ───────────────────────────────────────────────

/// Terminal, incomplete, or failed result of a Tool Program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResult {
    /// Terminal type of the result.
    pub terminal_type: ProgramTerminalType,
    /// Schema version of the result format.
    pub schema_version: u32,
    /// Primary result value (bounded, redactable).
    pub value: Option<serde_json::Value>,
    /// Artifact handles for large result bodies.
    pub artifacts: Vec<CallArtifactRef>,
    /// Whether partial results are available (for Incomplete state).
    pub has_partial_results: bool,
    /// Failure summary (when terminal_type is failure-derived).
    pub failure_summary: Option<ProgramFailureSummary>,
    /// Budget usage snapshot.
    pub budget_usage: ProgramBudgetUsage,
    /// When the result was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// Terminal type classification for program results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramTerminalType {
    Success,
    Incomplete,
    Failed,
    Cancelled,
    TimedOut,
    Blocked,
}

/// Failure summary for non-success terminals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramFailureSummary {
    pub failure_class: ProgramFailureClass,
    pub message: String,
    /// The call that caused the terminal failure, if applicable.
    pub failing_call_id: Option<ProgramCallId>,
}

/// Budget usage at program completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramBudgetUsage {
    /// Steps consumed.
    pub steps_used: u64,
    /// Wall-clock time in milliseconds.
    pub elapsed_ms: u64,
    /// Peak memory in megabytes.
    pub peak_memory_mb: u64,
    /// Total tool calls attempted.
    pub total_calls: u32,
    /// Total artifact bytes produced.
    pub artifact_bytes: u64,
}

// ─── Full program record ──────────────────────────────────────────

/// Complete durable record of a Tool Program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgramRecord {
    /// Program identity.
    pub program_id: ToolProgramId,
    /// Workspace this program belongs to.
    pub workspace_id: String,
    /// Session that submitted the program.
    pub session_id: Option<String>,
    /// Turn that submitted the program.
    pub turn_id: Option<String>,
    /// Language of the source.
    pub language: ProgramLanguage,
    /// Current state.
    pub state: ToolProgramState,
    /// Source reference (immutable, content-addressed).
    pub source_ref: ProgramSourceRef,
    /// Compiled IR reference (set after compilation, immutable).
    pub ir_ref: Option<ProgramIrRef>,
    /// Frozen capability manifest.
    pub manifest: ProgramCapabilityManifest,
    /// Latest checkpoint (set during execution for restart recovery).
    pub checkpoint: Option<ProgramCheckpoint>,
    /// Terminal result (set when state is terminal).
    pub result: Option<ProgramResult>,
    /// Linked scheduler job ID.
    pub job_id: Option<String>,
    /// Submission fingerprint (session|tool|ordinal) for idempotency.
    pub submission_key: String,
    /// Labels for compact projection/audit (must not contain source,
    /// manifest bodies, credentials, or unbounded output).
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// When the program was created.
    pub created_at: DateTime<Utc>,
    /// When the program last transitioned.
    pub updated_at: DateTime<Utc>,
    /// When the program reached a terminal state.
    pub terminal_at: Option<DateTime<Utc>>,
}

// ─── Query DTOs ───────────────────────────────────────────────────

/// Query parameters for listing programs.
#[derive(Debug, Clone, Default)]
pub struct ProgramListQuery {
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub states: Vec<ToolProgramState>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_roundtrip() {
        let id = ToolProgramId::new_unchecked("prog-1");
        assert_eq!(id.as_str(), "prog-1");
        assert_eq!(format!("{}", id), "prog-1");
        let json = serde_json::to_string(&id).unwrap();
        let back: ToolProgramId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn call_id_roundtrip() {
        let id = ProgramCallId::new_unchecked("call-1");
        assert_eq!(id.as_str(), "call-1");
        let json = serde_json::to_string(&id).unwrap();
        let back: ProgramCallId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn program_language_roundtrip() {
        assert_eq!(
            ProgramLanguage::from_str_lossy("restricted_python"),
            ProgramLanguage::RestrictedPython
        );
        assert_eq!(
            ProgramLanguage::from_str_lossy("future_lang"),
            ProgramLanguage::Unknown
        );
        assert_eq!(
            ProgramLanguage::RestrictedPython.as_str(),
            "restricted_python"
        );
    }

    #[test]
    fn program_state_terminal() {
        assert!(ToolProgramState::Completed.is_terminal());
        assert!(ToolProgramState::Incomplete.is_terminal());
        assert!(ToolProgramState::Failed.is_terminal());
        assert!(ToolProgramState::Cancelled.is_terminal());
        assert!(ToolProgramState::TimedOut.is_terminal());
        assert!(ToolProgramState::Blocked.is_terminal());
        assert!(!ToolProgramState::Running.is_terminal());
        assert!(!ToolProgramState::Queued.is_terminal());
        assert!(!ToolProgramState::Submitted.is_terminal());
    }

    #[test]
    fn program_state_roundtrip() {
        for state in [
            ToolProgramState::Submitted,
            ToolProgramState::Queued,
            ToolProgramState::Compiling,
            ToolProgramState::Running,
            ToolProgramState::Waiting,
            ToolProgramState::RetryBackoff,
            ToolProgramState::Completed,
            ToolProgramState::Incomplete,
            ToolProgramState::Failed,
            ToolProgramState::Cancelled,
            ToolProgramState::TimedOut,
            ToolProgramState::Stalled,
            ToolProgramState::Interrupted,
            ToolProgramState::Blocked,
        ] {
            assert_eq!(ToolProgramState::from_str_lossy(state.as_str()), state);
        }
    }

    #[test]
    fn call_state_terminal() {
        assert!(ProgramCallState::Completed.is_terminal());
        assert!(ProgramCallState::Failed.is_terminal());
        assert!(ProgramCallState::Cancelled.is_terminal());
        assert!(ProgramCallState::TimedOut.is_terminal());
        assert!(!ProgramCallState::Reserved.is_terminal());
        assert!(!ProgramCallState::Running.is_terminal());
    }

    #[test]
    fn program_transition_valid() {
        assert!(
            validate_program_transition(ToolProgramState::Submitted, ToolProgramState::Queued)
                .is_ok()
        );
        assert!(
            validate_program_transition(ToolProgramState::Queued, ToolProgramState::Running)
                .is_ok()
        );
        assert!(validate_program_transition(
            ToolProgramState::Running,
            ToolProgramState::Completed
        )
        .is_ok());
    }

    #[test]
    fn program_transition_invalid() {
        assert!(validate_program_transition(
            ToolProgramState::Completed,
            ToolProgramState::Running
        )
        .is_err());
        assert!(validate_program_transition(
            ToolProgramState::Submitted,
            ToolProgramState::Completed
        )
        .is_err());
        assert!(
            validate_program_transition(ToolProgramState::Failed, ToolProgramState::Queued)
                .is_err()
        );
    }

    #[test]
    fn program_transition_terminal_immutable() {
        let terminals = [
            ToolProgramState::Completed,
            ToolProgramState::Incomplete,
            ToolProgramState::Failed,
            ToolProgramState::Cancelled,
            ToolProgramState::TimedOut,
            ToolProgramState::Blocked,
        ];
        let targets = [
            ToolProgramState::Queued,
            ToolProgramState::Running,
            ToolProgramState::Completed,
            ToolProgramState::Failed,
        ];
        for from in &terminals {
            for to in &targets {
                if from != to {
                    assert!(
                        validate_program_transition(*from, *to).is_err(),
                        "terminal state {:?} should not transition to {:?}",
                        from,
                        to
                    );
                }
            }
        }
    }

    #[test]
    fn failure_class_roundtrip() {
        for fc in [
            ProgramFailureClass::Validation,
            ProgramFailureClass::ToolError,
            ProgramFailureClass::Permission,
            ProgramFailureClass::ResourceExhausted,
            ProgramFailureClass::Infrastructure,
            ProgramFailureClass::Unknown,
        ] {
            assert_eq!(ProgramFailureClass::from_str_lossy(fc.as_str()), fc);
        }
    }

    #[test]
    fn replay_disposition_roundtrip() {
        for rd in [
            ReplayDisposition::Replay,
            ReplayDisposition::Reexecute,
            ReplayDisposition::Skip,
        ] {
            let json = serde_json::to_string(&rd).unwrap();
            let back: ReplayDisposition = serde_json::from_str(&json).unwrap();
            assert_eq!(rd, back);
        }
    }

    #[test]
    fn program_result_serialization_roundtrip() {
        let result = ProgramResult {
            terminal_type: ProgramTerminalType::Success,
            schema_version: 1,
            value: Some(serde_json::json!({"output": "done"})),
            artifacts: vec![],
            has_partial_results: false,
            failure_summary: None,
            budget_usage: ProgramBudgetUsage {
                steps_used: 42,
                elapsed_ms: 1500,
                peak_memory_mb: 128,
                total_calls: 3,
                artifact_bytes: 1024,
            },
            recorded_at: Utc::now(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ProgramResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result.terminal_type, back.terminal_type);
        assert_eq!(result.budget_usage.steps_used, back.budget_usage.steps_used);
    }

    #[test]
    fn program_record_serialization_roundtrip() {
        let record = ToolProgramRecord {
            program_id: ToolProgramId::new_unchecked("p1"),
            workspace_id: "w1".to_string(),
            session_id: Some("s1".to_string()),
            turn_id: None,
            language: ProgramLanguage::RestrictedPython,
            state: ToolProgramState::Submitted,
            source_ref: ProgramSourceRef {
                digest: "abc123".to_string(),
                byte_length: 100,
                schema_version: 1,
                content_location: "store:p1/src".to_string(),
            },
            ir_ref: None,
            manifest: ProgramCapabilityManifest {
                manifest_version: 1,
                tools: HashMap::new(),
                max_concurrent_calls: 1,
                max_total_calls: 100,
                authority_digest: "auth1".to_string(),
                allow_mutations: false,
                resource_limits: ProgramLimitsSnapshot::default(),
            },
            checkpoint: None,
            result: None,
            job_id: None,
            submission_key: "s1|read|0".to_string(),
            labels: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            terminal_at: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: ToolProgramRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.program_id, back.program_id);
        assert_eq!(record.state, back.state);
    }

    #[test]
    fn limits_snapshot_defaults_are_safe() {
        let limits = ProgramLimitsSnapshot::default();
        assert!(limits.max_timeout_secs > 0);
        assert!(limits.max_steps > 0);
        assert!(limits.max_source_bytes > 0);
        assert!(limits.max_call_result_bytes > 0);
    }
}
