//! Durable project-level work-order domain (Project Work Orders M001).
//!
//! A `WorkOrder` is project intent: it records that some future work
//! should happen in a project, under which release conditions, in which
//! sequence lane, and with which requested model/policy snapshot. It is
//! not scheduler execution authority (scheduler `Job`), not a durable
//! rule that materializes jobs (scheduler `Schedule`), not delegated
//! child intent (`AgentTask`), not within-session completion state
//! (`WorkPlan`), and not a conversation (`Session`).
//!
//! Long-term references: `plans/000-long-term-specification.md`
//! (`#4`, `#6`, `#13`, `#17`, `#22`, `#24`, `#26`, `#29`) and ADR-0005
//! (`plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`).
//!
//! Ownership: `codegg-core::work_order` owns these types and their
//! validation. Storage lives in `super::store` against tables created by
//! migration v60. Sessions, jobs, schedules, worktrees, and provider
//! selections are consumed by reference only; this module never creates
//! a session row, submits a job, allocates a worktree, or invokes a
//! model. Release evaluation and occurrence materialization arrive in
//! M002; M001 persists, validates, and projects waiting work only.
//!
//! Occurrence indexing is explicit and 0-based: occurrence 0 is the
//! first execution of a work order; a `repeat_count` of N means N total
//! occurrences (indices `0..N`). There is no indefinite repetition.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{PrincipalId, ProjectId, SequenceLaneId, WorkOrderId, WorkOrderOccurrenceId};

// ── Bounds (enforced in domain validation, not just UI) ─────────────────

/// Maximum UTF-8 characters accepted for a work-order title.
pub const MAX_WORK_ORDER_TITLE_CHARS: usize = 256;
/// Maximum UTF-8 bytes accepted for a work-order prompt.
pub const MAX_WORK_ORDER_PROMPT_BYTES: usize = 32 * 1024;
/// Maximum items accepted in one atomic batch creation.
pub const MAX_WORK_ORDER_BATCH_ITEMS: usize = 32;
/// Maximum page size served by list operations.
pub const MAX_WORK_ORDER_LIST_LIMIT: u32 = 100;
/// Default page size when the caller passes no limit.
pub const DEFAULT_WORK_ORDER_LIST_LIMIT: u32 = 50;
/// Hard maximum total occurrences for one work order (`repeat_count`).
/// Finite and bounded: indefinite repetition has no representation.
pub const MAX_REPEAT_COUNT: u32 = 256;
/// Maximum lanes retained per project.
pub const MAX_LANES_PER_PROJECT: usize = 64;
/// Maximum members in one sequence lane.
pub const MAX_LANE_MEMBERS: usize = 1024;
/// Maximum UTF-8 characters accepted for a lane label.
pub const MAX_LANE_LABEL_CHARS: usize = 128;
/// Maximum UTF-8 characters accepted for a requested model identity.
pub const MAX_MODEL_ID_CHARS: usize = 256;
/// Maximum UTF-8 bytes accepted for an occurrence diagnostic.
pub const MAX_DIAGNOSTIC_CHARS: usize = 1024;
/// Maximum UTF-8 bytes accepted for an idempotency/batch key.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;
/// Maximum serialized bytes accepted for a release-gate set.
pub const MAX_GATE_SPEC_BYTES: usize = 4096;
/// Maximum delay-gate duration (one year, in seconds).
pub const MAX_DELAY_SECS: i64 = 366 * 24 * 60 * 60;
/// Maximum `not_before` timestamp accepted (2100-01-01T00:00:00Z, ms).
pub const MAX_NOT_BEFORE_MS: i64 = 4_102_444_800_000;
/// Maximum UTF-8 characters accepted for a trigger locator reference.
pub const MAX_TRIGGER_REF_CHARS: usize = 128;
/// Maximum UTF-8 characters accepted for a session/turn locator reference.
pub const MAX_PARENT_REF_CHARS: usize = 128;

// ── Lifecycle ──────────────────────────────────────────────────────────

/// Work-order (template) lifecycle.
///
/// `Active`/`Paused` are the only mutable states; `Completed` and
/// `Cancelled` are terminal for execution and may only move to
/// `Archived`. Terminal transitions are one-way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOrderState {
    Active,
    Paused,
    Completed,
    Cancelled,
    Archived,
}

impl WorkOrderState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Archived)
    }
}

/// `true` when `from -> to` is a legal work-order transition.
/// Terminal transitions are one-way; `NeedsAttention` has no template
/// analogue (attention lives on occurrences).
pub const fn can_transition_work_order(from: WorkOrderState, to: WorkOrderState) -> bool {
    use WorkOrderState::{Active, Archived, Cancelled, Completed, Paused};
    matches!(
        (from, to),
        (Active, Paused)
            | (Active, Completed)
            | (Active, Cancelled)
            | (Paused, Active)
            | (Paused, Cancelled)
            | (Completed, Archived)
            | (Cancelled, Archived)
    )
}

/// One execution instance lifecycle.
///
/// M001 persists and validates this matrix so M002 can claim
/// occurrences against it, but M001 leaves the runtime-only states
/// (`Ready`, `Claiming`, `Running`, ...) unused: every M001-created
/// occurrence starts `Waiting`. `NeedsAttention` resumes explicitly to
/// `Waiting`; terminal states never transition out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceState {
    Waiting,
    Ready,
    Claiming,
    Running,
    NeedsAttention,
    Completed,
    Failed,
    Cancelled,
}

impl OccurrenceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Ready => "ready",
            Self::Claiming => "claiming",
            Self::Running => "running",
            Self::NeedsAttention => "needs_attention",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "waiting" => Some(Self::Waiting),
            "ready" => Some(Self::Ready),
            "claiming" => Some(Self::Claiming),
            "running" => Some(Self::Running),
            "needs_attention" => Some(Self::NeedsAttention),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// `true` once an occurrence has been claimed for execution. Edits
    /// to execution-shaping work-order fields are rejected afterwards.
    pub const fn is_claimed(self) -> bool {
        matches!(
            self,
            Self::Claiming | Self::Running | Self::NeedsAttention | Self::Completed | Self::Failed
        )
    }
}

/// `true` when `from -> to` is a legal occurrence transition.
pub const fn can_transition_occurrence(from: OccurrenceState, to: OccurrenceState) -> bool {
    use OccurrenceState::{
        Cancelled, Claiming, Completed, Failed, NeedsAttention, Ready, Running, Waiting,
    };
    matches!(
        (from, to),
        (Waiting, Ready)
            | (Waiting, NeedsAttention)
            | (Waiting, Cancelled)
            | (Ready, Claiming)
            | (Ready, NeedsAttention)
            | (Ready, Cancelled)
            | (Claiming, Running)
            | (Claiming, NeedsAttention)
            | (Claiming, Failed)
            | (Claiming, Cancelled)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, NeedsAttention)
            | (Running, Cancelled)
            | (NeedsAttention, Waiting)
            | (NeedsAttention, Cancelled)
    )
}

// ── Release gates ──────────────────────────────────────────────────────

/// One closed release-gate kind. M001 persists and validates the closed
/// set; evaluation arrives in M002. There is no expression language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Immediate,
    Delay,
    NotBefore,
    SequenceReady,
    ExternalTrigger,
}

impl GateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Delay => "delay",
            Self::NotBefore => "not_before",
            Self::SequenceReady => "sequence_ready",
            Self::ExternalTrigger => "external_trigger",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "immediate" => Some(Self::Immediate),
            "delay" => Some(Self::Delay),
            "not_before" => Some(Self::NotBefore),
            "sequence_ready" => Some(Self::SequenceReady),
            "external_trigger" => Some(Self::ExternalTrigger),
            _ => None,
        }
    }
}

/// How multiple enabled gates combine. Exactly one join applies per
/// gate set; absent input decodes to `All`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateJoin {
    All,
    Any,
}

impl GateJoin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

/// One release-gate description. Only the payload for the gate's own
/// kind is meaningful; other payloads must be absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSpec {
    pub kind: GateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane_id: Option<SequenceLaneId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_ref: Option<String>,
}

/// The validated release-gate set for one work order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseGateSet {
    pub join: GateJoin,
    pub gates: Vec<GateSpec>,
}

impl ReleaseGateSet {
    /// The default gate set: immediate/zero-delay eligibility, which
    /// behaves like a normal newly started session once M002 exists.
    pub fn immediate() -> Self {
        Self {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::Immediate,
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            }],
        }
    }

    pub fn to_dto(&self) -> (Vec<codegg_protocol::work_order::WorkOrderGateDto>, String) {
        let gates = self
            .gates
            .iter()
            .map(|gate| codegg_protocol::work_order::WorkOrderGateDto {
                kind: gate.kind.as_str().to_owned(),
                delay_secs: gate.delay_secs,
                not_before_ms: gate.not_before_ms,
                lane_id: gate.lane_id.as_ref().map(|id| id.as_str().to_owned()),
                trigger_ref: gate.trigger_ref.clone(),
            })
            .collect();
        (gates, self.join.as_str().to_owned())
    }
}

// ── Policy snapshots ───────────────────────────────────────────────────

/// Requested approval mode snapshot. Re-resolved/narrowed at execution;
/// never silently widened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequest {
    Interactive,
    Automatic,
    Yolo,
}

impl ApprovalRequest {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Automatic => "automatic",
            Self::Yolo => "yolo",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "interactive" => Some(Self::Interactive),
            "automatic" | "auto" => Some(Self::Automatic),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }
}

/// Requested sandbox profile snapshot. Orthogonal to approval mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRequest {
    ReadOnly,
    WorkspaceWrite,
    FullHost,
}

impl SandboxRequest {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WorkspaceWrite => "workspace_write",
            Self::FullHost => "full_host",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "read_only" | "readonly" | "read-only" => Some(Self::ReadOnly),
            "workspace_write" | "workspace-write" => Some(Self::WorkspaceWrite),
            "full_host" | "full-host" | "fullhost" => Some(Self::FullHost),
            _ => None,
        }
    }
}

/// Requested workspace isolation policy. `AutoIsolated` (the default)
/// means managed-worktree isolation for mutation-capable Git work at
/// materialization time; non-Git mutation serializes unless an explicit
/// unsafe option exists (M002 enforcement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePolicy {
    AutoIsolated,
    Shared,
    Serialized,
}

impl WorkspacePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoIsolated => "auto_isolated",
            Self::Shared => "shared",
            Self::Serialized => "serialized",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "auto_isolated" | "auto-isolated" | "autoisolated" => Some(Self::AutoIsolated),
            "shared" => Some(Self::Shared),
            "serialized" => Some(Self::Serialized),
            _ => None,
        }
    }
}

/// Closed lane failure policy. `HoldLane` (the default) holds
/// downstream work until an authorized actor retries, skips, or
/// advances; a failed task never silently unleashes the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneFailurePolicy {
    HoldLane,
    ContinueLane,
}

impl LaneFailurePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HoldLane => "hold_lane",
            Self::ContinueLane => "continue_lane",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "hold_lane" | "hold-lane" | "hold" => Some(Self::HoldLane),
            "continue_lane" | "continue-lane" | "continue" => Some(Self::ContinueLane),
            _ => None,
        }
    }
}

/// Closed attention reason codes. Bounded diagnostics only; attention
/// never carries secrets or hidden reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionCode {
    ModelUnavailable,
    PolicyNarrowed,
    WorktreeConflict,
    TriggerExpired,
    PredecessorFailed,
    PredecessorAttention,
    MaterializationFailed,
}

impl AttentionCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelUnavailable => "model_unavailable",
            Self::PolicyNarrowed => "policy_narrowed",
            Self::WorktreeConflict => "worktree_conflict",
            Self::TriggerExpired => "trigger_expired",
            Self::PredecessorFailed => "predecessor_failed",
            Self::PredecessorAttention => "predecessor_attention",
            Self::MaterializationFailed => "materialization_failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "model_unavailable" => Some(Self::ModelUnavailable),
            "policy_narrowed" => Some(Self::PolicyNarrowed),
            "worktree_conflict" => Some(Self::WorktreeConflict),
            "trigger_expired" => Some(Self::TriggerExpired),
            "predecessor_failed" => Some(Self::PredecessorFailed),
            "predecessor_attention" => Some(Self::PredecessorAttention),
            "materialization_failed" => Some(Self::MaterializationFailed),
            _ => None,
        }
    }
}

// ── Records ────────────────────────────────────────────────────────────

/// One durable project work order: human/team/script/agent authored
/// intent plus release description. Waiting rows exist without any
/// session row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrder {
    pub id: WorkOrderId,
    pub revision: u64,
    pub project_id: ProjectId,
    pub creator_principal: PrincipalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_work_order_id: Option<WorkOrderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_approval: Option<ApprovalRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_sandbox: Option<SandboxRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<WorkspacePolicy>,
    pub gates: ReleaseGateSet,
    /// Total occurrences to materialize (`1` = run once). Always
    /// finite; indefinite repetition has no representation.
    pub repeat_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_lane_id: Option<SequenceLaneId>,
    pub state: WorkOrderState,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_at_ms: Option<i64>,
}

impl WorkOrder {
    pub fn to_dto(&self) -> codegg_protocol::work_order::WorkOrderDto {
        let (gates, gate_join) = self.gates.to_dto();
        codegg_protocol::work_order::WorkOrderDto {
            work_order_id: self.id.as_str().to_owned(),
            revision: self.revision,
            project_id: self.project_id.as_str().to_owned(),
            creator_principal: self.creator_principal.as_str().to_owned(),
            parent_session_id: self.parent_session_id.clone(),
            parent_turn_id: self.parent_turn_id.clone(),
            parent_work_order_id: self
                .parent_work_order_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            title: self.title.clone(),
            prompt: self.prompt.clone(),
            requested_model: self.requested_model.clone(),
            requested_approval: self.requested_approval.map(|mode| mode.as_str().to_owned()),
            requested_sandbox: self
                .requested_sandbox
                .map(|profile| profile.as_str().to_owned()),
            workspace_policy: self
                .workspace_policy
                .map(|policy| policy.as_str().to_owned()),
            gates,
            gate_join: Some(gate_join),
            repeat_count: self.repeat_count,
            sequence_lane_id: self
                .sequence_lane_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            state: self.state.as_str().to_owned(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            cancelled_at_ms: self.cancelled_at_ms,
        }
    }
}

/// One execution instance of a work order. M001 creates `Waiting`
/// records on explicit request; the M002 coordinator will own claim
/// transitions. Materialization references stay absent until claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrderOccurrence {
    pub id: WorkOrderOccurrenceId,
    pub work_order_id: WorkOrderId,
    pub project_id: ProjectId,
    /// Explicit 0-based execution index (`0..repeat_count`).
    pub occurrence_index: u64,
    pub state: OccurrenceState,
    #[serde(default)]
    pub gate_latches: Vec<GateKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_code: Option<AttentionCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Claim/materialization timestamps, set by the M002 coordinator.
    /// Absent on M001-created waiting records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_at_ms: Option<i64>,
}

impl WorkOrderOccurrence {
    pub fn to_dto(&self) -> codegg_protocol::work_order::WorkOrderOccurrenceDto {
        codegg_protocol::work_order::WorkOrderOccurrenceDto {
            occurrence_id: self.id.as_str().to_owned(),
            work_order_id: self.work_order_id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            occurrence_index: self.occurrence_index,
            state: self.state.as_str().to_owned(),
            gate_latches: self
                .gate_latches
                .iter()
                .map(|kind| kind.as_str().to_owned())
                .collect(),
            not_before_ms: self.not_before_ms,
            next_check_at_ms: self.next_check_at_ms,
            session_id: self.session_id.clone(),
            job_id: self.job_id.clone(),
            workspace_id: self.workspace_id.clone(),
            worktree_id: self.worktree_id.clone(),
            attention_code: self.attention_code.map(|code| code.as_str().to_owned()),
            diagnostic: self.diagnostic.clone(),
            claimed_at_ms: self.claimed_at_ms,
            started_at_ms: self.started_at_ms,
            terminal_at_ms: self.terminal_at_ms,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// One revisioned project sequence lane plus its deterministic member
/// order. Ordering authority lives here (CAS revision), not in
/// scheduler job dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceLane {
    pub id: SequenceLaneId,
    pub project_id: ProjectId,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub failure_policy: LaneFailurePolicy,
    #[serde(default)]
    pub ordered_work_order_ids: Vec<WorkOrderId>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl SequenceLane {
    pub fn to_dto(&self) -> codegg_protocol::work_order::SequenceLaneDto {
        codegg_protocol::work_order::SequenceLaneDto {
            lane_id: self.id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            revision: self.revision,
            label: self.label.clone(),
            failure_policy: self.failure_policy.as_str().to_owned(),
            ordered_work_order_ids: self
                .ordered_work_order_ids
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Bounded per-project summary counts for later projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrderSummary {
    pub project_id: ProjectId,
    pub active: u64,
    pub paused: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub archived: u64,
    pub waiting_occurrences: u64,
    pub attention_occurrences: u64,
    pub lane_count: u64,
}

impl WorkOrderSummary {
    pub fn to_dto(&self) -> codegg_protocol::work_order::WorkOrderSummaryDto {
        codegg_protocol::work_order::WorkOrderSummaryDto {
            project_id: self.project_id.as_str().to_owned(),
            active: self.active,
            paused: self.paused,
            completed: self.completed,
            cancelled: self.cancelled,
            archived: self.archived,
            waiting_occurrences: self.waiting_occurrences,
            attention_occurrences: self.attention_occurrences,
            lane_count: self.lane_count,
        }
    }
}

// ── Inputs ─────────────────────────────────────────────────────────────

/// Validated input for creating one work order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkOrder {
    pub title: Option<String>,
    pub prompt: String,
    pub requested_model: Option<String>,
    pub requested_approval: Option<ApprovalRequest>,
    pub requested_sandbox: Option<SandboxRequest>,
    pub workspace_policy: Option<WorkspacePolicy>,
    pub gates: ReleaseGateSet,
    pub repeat_count: u32,
    pub sequence_lane_id: Option<SequenceLaneId>,
    pub parent_session_id: Option<String>,
    pub parent_turn_id: Option<String>,
    pub parent_work_order_id: Option<WorkOrderId>,
    pub idempotency_key: Option<String>,
}

/// Validated patch for updating one waiting work order. `None` leaves
/// a field unchanged. Lane placement changes through the lane reorder
/// operation, never through this patch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkOrderPatch {
    pub title: Option<Option<String>>,
    pub prompt: Option<String>,
    pub requested_model: Option<Option<String>>,
    pub requested_approval: Option<Option<ApprovalRequest>>,
    pub requested_sandbox: Option<Option<SandboxRequest>>,
    pub workspace_policy: Option<Option<WorkspacePolicy>>,
    pub gates: Option<ReleaseGateSet>,
    pub repeat_count: Option<u32>,
}

impl WorkOrderPatch {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.prompt.is_none()
            && self.requested_model.is_none()
            && self.requested_approval.is_none()
            && self.requested_sandbox.is_none()
            && self.workspace_policy.is_none()
            && self.gates.is_none()
            && self.repeat_count.is_none()
    }
}

/// Validated input for creating one sequence lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSequenceLane {
    pub label: Option<String>,
    pub failure_policy: LaneFailurePolicy,
    pub idempotency_key: Option<String>,
}

// ── Errors ─────────────────────────────────────────────────────────────

/// Typed failure for the work-order domain.
#[derive(Debug, Error)]
pub enum WorkOrderError {
    #[error("invalid work order {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("work order prompt too large: {bytes} bytes (max {max})")]
    PromptTooLarge { bytes: usize, max: usize },
    #[error("work order not found: {0}")]
    NotFound(String),
    #[error("stale work order revision for {id}: expected {expected}, current {current}")]
    RevisionConflict {
        id: String,
        expected: u64,
        current: u64,
    },
    #[error("illegal work order transition: {0}")]
    StateConflict(String),
    #[error("work order capacity is exhausted: {0}")]
    Capacity(String),
    #[error("work order store unavailable: {0}")]
    Unavailable(String),
    #[error("work order project mismatch: {0}")]
    ProjectMismatch(String),
    #[error("work order idempotency conflict for key {0}")]
    IdempotencyConflict(String),
    #[error("work order storage error: {0}")]
    Storage(#[from] StorageError),
}

impl WorkOrderError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "work_order_invalid_input",
            Self::PromptTooLarge { .. } => "work_order_prompt_too_large",
            Self::NotFound(_) => "work_order_not_found",
            Self::RevisionConflict { .. } => "work_order_revision_conflict",
            Self::StateConflict(_) => "work_order_state_conflict",
            Self::Capacity(_) => "work_order_capacity",
            Self::Unavailable(_) => "work_order_unavailable",
            Self::ProjectMismatch(_) => "work_order_project_mismatch",
            Self::IdempotencyConflict(_) => "work_order_idempotency_conflict",
            Self::Storage(_) => "work_order_storage_error",
        }
    }

    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for WorkOrderError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(StorageError::Database(error.to_string()))
    }
}

// ── Validation ─────────────────────────────────────────────────────────

fn check_chars(
    value: &str,
    field: &'static str,
    max_chars: usize,
) -> Result<String, WorkOrderError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(WorkOrderError::invalid(field, "value must not be empty"));
    }
    if trimmed.chars().count() > max_chars {
        return Err(WorkOrderError::invalid(
            field,
            format!("value exceeds {max_chars} characters"),
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(WorkOrderError::invalid(
            field,
            "value contains an unsupported character",
        ));
    }
    Ok(trimmed.to_owned())
}

fn check_optional_chars(
    value: Option<&str>,
    field: &'static str,
    max_chars: usize,
) -> Result<Option<String>, WorkOrderError> {
    value
        .map(|raw| check_chars(raw, field, max_chars))
        .transpose()
}

pub fn validate_title(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    check_optional_chars(value, "title", MAX_WORK_ORDER_TITLE_CHARS)
}

pub fn validate_prompt(value: &str) -> Result<String, WorkOrderError> {
    if value.trim().is_empty() {
        return Err(WorkOrderError::invalid(
            "prompt",
            "prompt must not be empty",
        ));
    }
    if value.len() > MAX_WORK_ORDER_PROMPT_BYTES {
        return Err(WorkOrderError::PromptTooLarge {
            bytes: value.len(),
            max: MAX_WORK_ORDER_PROMPT_BYTES,
        });
    }
    if value.bytes().any(|b| b == 0) {
        return Err(WorkOrderError::invalid(
            "prompt",
            "prompt contains a NUL byte",
        ));
    }
    Ok(value.to_owned())
}

pub fn validate_model(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    check_optional_chars(value, "requested_model", MAX_MODEL_ID_CHARS)
}

pub fn validate_idempotency_key(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_IDEMPOTENCY_KEY_LEN {
        return Err(WorkOrderError::invalid(
            "idempotency_key",
            format!("idempotency key must be 1..={MAX_IDEMPOTENCY_KEY_LEN} bytes"),
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(WorkOrderError::invalid(
            "idempotency_key",
            "idempotency key contains an unsupported character",
        ));
    }
    Ok(Some(trimmed.to_owned()))
}

pub fn validate_parent_ref(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    check_optional_chars(value, "parent", MAX_PARENT_REF_CHARS)
}

pub fn validate_repeat_count(value: Option<u32>) -> Result<u32, WorkOrderError> {
    let count = value.unwrap_or(1);
    if count == 0 || count > MAX_REPEAT_COUNT {
        return Err(WorkOrderError::invalid(
            "repeat_count",
            format!(
                "repeat count must be 1..={MAX_REPEAT_COUNT} (finite; no indefinite repetition)"
            ),
        ));
    }
    Ok(count)
}

pub fn validate_diagnostic(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    check_optional_chars(value, "diagnostic", MAX_DIAGNOSTIC_CHARS)
}

/// Validate one gate set shape (kinds, payloads, join). Cross-project
/// references (lane/parent scope) are enforced by the store, which
/// holds the project context.
pub fn validate_gate_set(
    gates: &[GateSpec],
    join: GateJoin,
) -> Result<ReleaseGateSet, WorkOrderError> {
    if gates.is_empty() {
        return Ok(ReleaseGateSet::immediate());
    }
    if gates.len() > 8 {
        return Err(WorkOrderError::invalid(
            "gates",
            "at most 8 release gates per work order",
        ));
    }
    let mut seen_kinds = std::collections::HashSet::new();
    for gate in gates {
        if !seen_kinds.insert(gate.kind) {
            return Err(WorkOrderError::invalid(
                "gates",
                format!("duplicate {} gate", gate.kind.as_str()),
            ));
        }
        match gate.kind {
            GateKind::Immediate => {
                if gates.len() > 1 {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "immediate eligibility cannot combine with other gates",
                    ));
                }
                if gate.delay_secs.is_some()
                    || gate.not_before_ms.is_some()
                    || gate.lane_id.is_some()
                    || gate.trigger_ref.is_some()
                {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "immediate gate carries no payload",
                    ));
                }
            }
            GateKind::Delay => {
                let Some(delay) = gate.delay_secs else {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "delay gate requires delay_secs",
                    ));
                };
                if !(0..=MAX_DELAY_SECS).contains(&delay) {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        format!("delay must be 0..={MAX_DELAY_SECS} seconds"),
                    ));
                }
                if gate.not_before_ms.is_some()
                    || gate.lane_id.is_some()
                    || gate.trigger_ref.is_some()
                {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "delay gate carries only delay_secs",
                    ));
                }
            }
            GateKind::NotBefore => {
                let Some(at) = gate.not_before_ms else {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "not_before gate requires not_before_ms",
                    ));
                };
                if !(0..=MAX_NOT_BEFORE_MS).contains(&at) {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "not_before timestamp is outside supported bounds",
                    ));
                }
                if gate.delay_secs.is_some() || gate.lane_id.is_some() || gate.trigger_ref.is_some()
                {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "not_before gate carries only not_before_ms",
                    ));
                }
            }
            GateKind::SequenceReady => {
                if gate.lane_id.is_none() {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "sequence_ready gate requires lane_id",
                    ));
                }
                if gate.delay_secs.is_some()
                    || gate.not_before_ms.is_some()
                    || gate.trigger_ref.is_some()
                {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "sequence_ready gate carries only lane_id",
                    ));
                }
            }
            GateKind::ExternalTrigger => {
                let Some(ref trigger_ref) = gate.trigger_ref else {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "external_trigger gate requires trigger_ref",
                    ));
                };
                let trimmed = trigger_ref.trim();
                if trimmed.is_empty() || trimmed.chars().count() > MAX_TRIGGER_REF_CHARS {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        format!("trigger reference must be 1..={MAX_TRIGGER_REF_CHARS} characters"),
                    ));
                }
                // The reference is an opaque same-project locator: it must
                // satisfy the shared identity lexical contract so it can
                // never name another project (no path separators). M005
                // binds it to a stored trigger verifier.
                if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "trigger reference must be an opaque locator, not a path",
                    ));
                }
                if gate.delay_secs.is_some()
                    || gate.not_before_ms.is_some()
                    || gate.lane_id.is_some()
                {
                    return Err(WorkOrderError::invalid(
                        "gates",
                        "external_trigger gate carries only trigger_ref",
                    ));
                }
            }
        }
    }
    let _ = join;
    Ok(ReleaseGateSet {
        join,
        gates: gates.to_vec(),
    })
}

pub fn validate_lane_label(value: Option<&str>) -> Result<Option<String>, WorkOrderError> {
    check_optional_chars(value, "lane_label", MAX_LANE_LABEL_CHARS)
}

pub fn validate_list_limit(value: Option<u32>) -> u32 {
    value
        .unwrap_or(DEFAULT_WORK_ORDER_LIST_LIMIT)
        .clamp(1, MAX_WORK_ORDER_LIST_LIMIT)
}

/// `true` when execution-shaping fields may still be edited: the work
/// order is mutable and no occurrence that depends on those values has
/// been claimed yet.
pub fn can_edit_execution_fields(state: WorkOrderState, claimed_occurrences: bool) -> bool {
    matches!(state, WorkOrderState::Active | WorkOrderState::Paused) && !claimed_occurrences
}

/// Structural audit metadata for one work-order mutation. Locators and
/// revisions only: never prompt bodies, secrets, or reasoning.
pub fn audit_metadata_for_work_order(
    work_order: &WorkOrder,
    operation: &str,
    decision_id: &str,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "project.id".to_owned(),
        work_order.project_id.as_str().to_owned(),
    );
    metadata.insert(
        "work_order.id".to_owned(),
        work_order.id.as_str().to_owned(),
    );
    metadata.insert(
        "work_order.revision".to_owned(),
        work_order.revision.to_string(),
    );
    metadata.insert("work_order.op".to_owned(), operation.to_owned());
    metadata.insert(
        "work_order.state".to_owned(),
        work_order.state.as_str().to_owned(),
    );
    metadata.insert("decision.id".to_owned(), decision_id.to_owned());
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(kind: GateKind) -> GateSpec {
        GateSpec {
            kind,
            delay_secs: None,
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        }
    }

    #[test]
    fn work_order_state_matrix_is_closed_and_terminal_one_way() {
        assert!(can_transition_work_order(
            WorkOrderState::Active,
            WorkOrderState::Paused
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Active,
            WorkOrderState::Completed
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Active,
            WorkOrderState::Cancelled
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Paused,
            WorkOrderState::Active
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Paused,
            WorkOrderState::Cancelled
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Completed,
            WorkOrderState::Archived
        ));
        assert!(can_transition_work_order(
            WorkOrderState::Cancelled,
            WorkOrderState::Archived
        ));
        // Terminal transitions are one-way.
        assert!(!can_transition_work_order(
            WorkOrderState::Completed,
            WorkOrderState::Active
        ));
        assert!(!can_transition_work_order(
            WorkOrderState::Cancelled,
            WorkOrderState::Paused
        ));
        assert!(!can_transition_work_order(
            WorkOrderState::Archived,
            WorkOrderState::Active
        ));
        assert!(!can_transition_work_order(
            WorkOrderState::Active,
            WorkOrderState::Archived
        ));
        assert!(!can_transition_work_order(
            WorkOrderState::Paused,
            WorkOrderState::Completed
        ));
        for state in [
            WorkOrderState::Active,
            WorkOrderState::Paused,
            WorkOrderState::Completed,
            WorkOrderState::Cancelled,
            WorkOrderState::Archived,
        ] {
            assert_eq!(WorkOrderState::parse(state.as_str()), Some(state));
        }
        assert_eq!(WorkOrderState::parse("running"), None);
    }

    #[test]
    fn occurrence_matrix_reserves_runtime_states_with_explicit_resume() {
        assert!(can_transition_occurrence(
            OccurrenceState::Waiting,
            OccurrenceState::Ready
        ));
        assert!(can_transition_occurrence(
            OccurrenceState::Ready,
            OccurrenceState::Claiming
        ));
        assert!(can_transition_occurrence(
            OccurrenceState::Claiming,
            OccurrenceState::Running
        ));
        assert!(can_transition_occurrence(
            OccurrenceState::Running,
            OccurrenceState::NeedsAttention
        ));
        // NeedsAttention resumes explicitly to Waiting only.
        assert!(can_transition_occurrence(
            OccurrenceState::NeedsAttention,
            OccurrenceState::Waiting
        ));
        assert!(!can_transition_occurrence(
            OccurrenceState::NeedsAttention,
            OccurrenceState::Running
        ));
        // Terminal states never transition out.
        for terminal in [
            OccurrenceState::Completed,
            OccurrenceState::Failed,
            OccurrenceState::Cancelled,
        ] {
            for next in [
                OccurrenceState::Waiting,
                OccurrenceState::Ready,
                OccurrenceState::Claiming,
                OccurrenceState::Running,
                OccurrenceState::NeedsAttention,
                OccurrenceState::Completed,
                OccurrenceState::Failed,
                OccurrenceState::Cancelled,
            ] {
                assert!(
                    !can_transition_occurrence(terminal, next),
                    "{terminal:?} -> {next:?} must be rejected"
                );
            }
        }
        assert!(OccurrenceState::Running.is_claimed());
        assert!(!OccurrenceState::Waiting.is_claimed());
        assert!(!OccurrenceState::Ready.is_claimed());
    }

    #[test]
    fn gate_validation_rejects_empty_and_incompatible_combinations() {
        // Empty normalizes to immediate.
        let set = validate_gate_set(&[], GateJoin::All).expect("empty defaults");
        assert_eq!(set.gates.len(), 1);
        assert_eq!(set.gates[0].kind, GateKind::Immediate);

        // Immediate cannot combine.
        let gates = vec![
            gate(GateKind::Immediate),
            GateSpec {
                kind: GateKind::Delay,
                delay_secs: Some(60),
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
        ];
        assert!(validate_gate_set(&gates, GateJoin::All).is_err());

        // Duplicate kinds are rejected.
        let gates = vec![
            GateSpec {
                kind: GateKind::Delay,
                delay_secs: Some(60),
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
            GateSpec {
                kind: GateKind::Delay,
                delay_secs: Some(120),
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            },
        ];
        assert!(validate_gate_set(&gates, GateJoin::All).is_err());

        // Negative durations and out-of-bounds timestamps fail.
        let gates = vec![GateSpec {
            kind: GateKind::Delay,
            delay_secs: Some(-1),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        }];
        assert!(validate_gate_set(&gates, GateJoin::All).is_err());
        let gates = vec![GateSpec {
            kind: GateKind::NotBefore,
            delay_secs: None,
            not_before_ms: Some(MAX_NOT_BEFORE_MS + 1),
            lane_id: None,
            trigger_ref: None,
        }];
        assert!(validate_gate_set(&gates, GateJoin::All).is_err());

        // Path-like trigger references fail closed.
        let gates = vec![GateSpec {
            kind: GateKind::ExternalTrigger,
            delay_secs: None,
            not_before_ms: None,
            lane_id: None,
            trigger_ref: Some("../other-project".to_owned()),
        }];
        assert!(validate_gate_set(&gates, GateJoin::All).is_err());

        // Missing payloads fail.
        assert!(validate_gate_set(&[gate(GateKind::Delay)], GateJoin::All).is_err());
        assert!(validate_gate_set(&[gate(GateKind::NotBefore)], GateJoin::All).is_err());
        assert!(validate_gate_set(&[gate(GateKind::SequenceReady)], GateJoin::All).is_err());
        assert!(validate_gate_set(&[gate(GateKind::ExternalTrigger)], GateJoin::All).is_err());

        // Unknown kind names fail closed.
        assert_eq!(GateKind::parse("shell_predicate"), None);
    }

    #[test]
    fn repeat_policy_is_finite_and_explicit() {
        assert_eq!(validate_repeat_count(None).unwrap(), 1);
        assert_eq!(validate_repeat_count(Some(1)).unwrap(), 1);
        assert_eq!(
            validate_repeat_count(Some(MAX_REPEAT_COUNT)).unwrap(),
            MAX_REPEAT_COUNT
        );
        assert!(validate_repeat_count(Some(0)).is_err());
        assert!(validate_repeat_count(Some(MAX_REPEAT_COUNT + 1)).is_err());
    }

    #[test]
    fn occurrence_indexing_is_explicit_zero_based() {
        // Occurrence 0 is the first execution; repeat_count N covers 0..N.
        let first_index: u64 = 0;
        let repeat_count: u32 = 3;
        let indices: Vec<u64> = (0..u64::from(repeat_count)).collect();
        assert_eq!(indices, vec![0, 1, 2]);
        assert_eq!(first_index, indices[0]);
    }

    #[test]
    fn field_bounds_reject_oversized_and_empty_input() {
        assert!(validate_title(Some("")).is_err());
        assert!(validate_title(Some(&"x".repeat(MAX_WORK_ORDER_TITLE_CHARS + 1))).is_err());
        assert!(validate_prompt("").is_err());
        assert!(validate_prompt(&"x".repeat(MAX_WORK_ORDER_PROMPT_BYTES + 1)).is_err());
        assert!(validate_model(Some(&"m".repeat(MAX_MODEL_ID_CHARS + 1))).is_err());
        assert!(validate_idempotency_key(Some("")).is_err());
        assert!(validate_idempotency_key(Some(&"k".repeat(MAX_IDEMPOTENCY_KEY_LEN + 1))).is_err());
        assert_eq!(validate_list_limit(None), DEFAULT_WORK_ORDER_LIST_LIMIT);
        assert_eq!(validate_list_limit(Some(0)), 1);
        assert_eq!(validate_list_limit(Some(10_000)), MAX_WORK_ORDER_LIST_LIMIT);
    }

    #[test]
    fn execution_field_edits_require_mutable_state_without_claims() {
        assert!(can_edit_execution_fields(WorkOrderState::Active, false));
        assert!(can_edit_execution_fields(WorkOrderState::Paused, false));
        assert!(!can_edit_execution_fields(WorkOrderState::Active, true));
        assert!(!can_edit_execution_fields(WorkOrderState::Completed, false));
        assert!(!can_edit_execution_fields(WorkOrderState::Cancelled, false));
        assert!(!can_edit_execution_fields(WorkOrderState::Archived, false));
    }

    #[test]
    fn policy_snapshots_parse_closed_sets() {
        assert_eq!(
            ApprovalRequest::parse("automatic"),
            Some(ApprovalRequest::Automatic)
        );
        assert_eq!(ApprovalRequest::parse("escalate"), None);
        assert_eq!(
            SandboxRequest::parse("full_host"),
            Some(SandboxRequest::FullHost)
        );
        assert_eq!(SandboxRequest::parse("nsjail"), None);
        assert_eq!(
            WorkspacePolicy::parse("auto_isolated"),
            Some(WorkspacePolicy::AutoIsolated)
        );
        assert_eq!(WorkspacePolicy::parse("overlay"), None);
        assert_eq!(
            LaneFailurePolicy::parse("hold_lane"),
            Some(LaneFailurePolicy::HoldLane)
        );
        assert_eq!(LaneFailurePolicy::parse("merge"), None);
        assert_eq!(
            AttentionCode::parse("model_unavailable"),
            Some(AttentionCode::ModelUnavailable)
        );
        assert_eq!(AttentionCode::parse("vibes"), None);
    }

    #[test]
    fn error_codes_are_stable_wire_names() {
        assert_eq!(
            WorkOrderError::NotFound("wo-1".to_owned()).code(),
            "work_order_not_found"
        );
        assert_eq!(
            WorkOrderError::RevisionConflict {
                id: "wo-1".to_owned(),
                expected: 1,
                current: 2,
            }
            .code(),
            "work_order_revision_conflict"
        );
    }
}
