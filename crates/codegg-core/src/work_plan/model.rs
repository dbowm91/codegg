//! Durable WorkPlan foundation (long-horizon M002).
//!
//! `WorkPlan`/`WorkItem` is detailed work state, not execution authority.
//! It records what remains, what blocks it, and what host-owned evidence
//! could satisfy it. It never grants tool capability, never replays side
//! effects, and never stores hidden model reasoning.
//!
//! Long-term references: `plans/000-long-term-specification.md`
//! (`#4.2`, `#16`, `#24`, `#26`, `#29`) and ADR-0003
//! (`plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`).
//!
//! Ownership: `codegg-core::work_plan` owns these types and their
//! validation. Storage lives in `super::store` against tables created by
//! migration v59. Goal, TodoState, AgentTask/AgentRun, scheduler Job, and
//! continuation checkpoints are consumed by reference only.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use thiserror::Error;

// ── Bounds (enforced in domain validation, not just UI) ─────────────────────

/// Maximum items in one plan. Deliberately larger than TodoState's
/// projection cap (10-12) so a large bounded plan is valid and queryable
/// while TodoState stays small and model-facing.
pub const MAX_ITEMS_PER_PLAN: usize = 64;
/// Maximum objective characters per plan.
pub const MAX_OBJECTIVE_CHARS: usize = 4000;
/// Maximum origin-provenance characters per plan.
pub const MAX_ORIGIN_PROVENANCE_CHARS: usize = 1024;
/// Maximum current-phase characters per plan.
pub const MAX_PHASE_CHARS: usize = 256;
/// Maximum description characters per item.
pub const MAX_ITEM_DESCRIPTION_CHARS: usize = 1024;
/// Maximum dependencies per item.
pub const MAX_DEPENDENCIES_PER_ITEM: usize = 16;
/// Maximum acceptance criteria per item.
pub const MAX_ACCEPTANCE_PER_ITEM: usize = 16;
/// Maximum acceptance description characters.
pub const MAX_ACCEPTANCE_CHARS: usize = 512;
/// Maximum acceptance note characters.
pub const MAX_ACCEPTANCE_NOTE_CHARS: usize = 512;
/// Maximum evidence refs per item.
pub const MAX_EVIDENCE_PER_ITEM: usize = 16;
/// Maximum evidence ref-id characters.
pub const MAX_EVIDENCE_REF_CHARS: usize = 512;
/// Maximum evidence detail characters.
pub const MAX_EVIDENCE_DETAIL_CHARS: usize = 512;
/// Maximum blocker characters per item.
pub const MAX_BLOCKER_CHARS: usize = 1024;
/// Maximum next-action characters per item.
pub const MAX_NEXT_ACTION_CHARS: usize = 1024;
/// Maximum owner run/job ref characters (provenance only).
pub const MAX_OWNER_REF_CHARS: usize = 256;
/// Maximum session/project/turn/goal scope-id characters.
pub const MAX_SCOPE_ID_CHARS: usize = 256;

// ── IDs ─────────────────────────────────────────────────────────────────────

/// Durable WorkPlan identity. Distinct from `AgentTaskId`, `AgentRunId`,
/// scheduler `JobId`, Goal ID, and Todo ID by type and by `wp_` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkPlanId(pub String);

/// Durable WorkItem identity. Distinct from plan and execution identities
/// by type and by `wi_` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkItemId(pub String);

impl WorkPlanId {
    pub fn generate() -> Self {
        Self(format!("wp_{}", uuid::Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl WorkItemId {
    pub fn generate() -> Self {
        Self(format!("wi_{}", uuid::Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkPlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for WorkItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Statuses ────────────────────────────────────────────────────────────────

/// Plan lifecycle. `Active`/`Blocked` are the only non-terminal states;
/// `Completed`/`Cancelled` are terminal and never transition out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlanStatus {
    Active,
    Blocked,
    Completed,
    Cancelled,
}

impl WorkPlanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "blocked" => Some(Self::Blocked),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// Item execution state. `Pending` means created but not yet known
/// actionable; `Actionable` means ready to start. Both are actionable
/// candidates once dependencies are satisfied — see
/// [`actionable_items`]. `Completed`/`Cancelled` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Pending,
    Actionable,
    InProgress,
    Blocked,
    Completed,
    Cancelled,
}

impl WorkItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Actionable => "actionable",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "actionable" => Some(Self::Actionable),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

// ── Acceptance and evidence ─────────────────────────────────────────────────

/// Host-evidence disposition for one acceptance criterion.
///
/// Model text can explain progress but never creates `Satisfied` evidence:
/// only a canonical host-owned source (or an explicit user judgment) does.
/// A ref whose target cannot be found is `Unavailable` — never satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAcceptanceDisposition {
    Unmet,
    Satisfied,
    RequiresUserJudgment,
}

impl WorkAcceptanceDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unmet => "unmet",
            Self::Satisfied => "satisfied",
            Self::RequiresUserJudgment => "requires_user_judgment",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unmet" => Some(Self::Unmet),
            "satisfied" => Some(Self::Satisfied),
            "requires_user_judgment" => Some(Self::RequiresUserJudgment),
            _ => None,
        }
    }
}

/// One bounded acceptance criterion on a WorkItem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAcceptance {
    pub description: String,
    pub disposition: WorkAcceptanceDisposition,
    #[serde(default)]
    pub note: Option<String>,
}

/// Closed set of canonical evidence-ref targets. Refs point at
/// host-owned identities; they never duplicate payloads and never carry
/// hidden reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkEvidenceKind {
    TestJob,
    DelegatedRun,
    SchedulerJob,
    AgentRun,
    Artifact,
    Commit,
}

impl WorkEvidenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TestJob => "test_job",
            Self::DelegatedRun => "delegated_run",
            Self::SchedulerJob => "scheduler_job",
            Self::AgentRun => "agent_run",
            Self::Artifact => "artifact",
            Self::Commit => "commit",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "test_job" => Some(Self::TestJob),
            "delegated_run" => Some(Self::DelegatedRun),
            "scheduler_job" => Some(Self::SchedulerJob),
            "agent_run" => Some(Self::AgentRun),
            "artifact" => Some(Self::Artifact),
            "commit" => Some(Self::Commit),
            _ => None,
        }
    }
}

/// One bounded pointer to canonical host-owned evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkEvidenceRef {
    pub kind: WorkEvidenceKind,
    pub ref_id: String,
    #[serde(default)]
    pub detail: Option<String>,
}

// ── Plan and item ───────────────────────────────────────────────────────────

/// Durable detailed plan. Objective and provenance are immutable after
/// creation; status/phase/current-item/goal-binding evolve through CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlan {
    pub id: WorkPlanId,
    pub revision: i64,
    pub session_id: String,
    pub project_id: String,
    #[serde(default)]
    pub origin_turn_id: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    pub objective: String,
    pub objective_digest: String,
    pub origin_provenance: String,
    pub status: WorkPlanStatus,
    #[serde(default)]
    pub current_phase: Option<String>,
    #[serde(default)]
    pub current_item_id: Option<WorkItemId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Durable detailed work item. Owner run/job refs are provenance only and
/// grant no write permission to any plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub plan_id: WorkPlanId,
    pub revision: i64,
    pub position: i64,
    #[serde(default)]
    pub parent_item_id: Option<WorkItemId>,
    #[serde(default)]
    pub dependencies: Vec<WorkItemId>,
    pub status: WorkItemStatus,
    pub description: String,
    #[serde(default)]
    pub acceptance: Vec<WorkAcceptance>,
    #[serde(default)]
    pub evidence: Vec<WorkEvidenceRef>,
    #[serde(default)]
    pub owner_run_id: Option<String>,
    #[serde(default)]
    pub owner_job_id: Option<String>,
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub blocker: Option<String>,
    #[serde(default)]
    pub next_action: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Inputs ──────────────────────────────────────────────────────────────────

/// Creation input for a plan. Objective/provenance are validated and the
/// digest is derived; callers cannot supply a forged digest.
#[derive(Debug, Clone)]
pub struct NewWorkPlan {
    pub session_id: String,
    pub project_id: String,
    pub origin_turn_id: Option<String>,
    pub goal_id: Option<String>,
    pub objective: String,
    pub origin_provenance: String,
    pub current_phase: Option<String>,
}

/// Creation input for an item.
#[derive(Debug, Clone)]
pub struct NewWorkItem {
    pub parent_item_id: Option<WorkItemId>,
    pub dependencies: Vec<WorkItemId>,
    pub status: WorkItemStatus,
    pub description: String,
    pub acceptance: Vec<WorkAcceptance>,
    pub evidence: Vec<WorkEvidenceRef>,
    pub owner_run_id: Option<String>,
    pub owner_job_id: Option<String>,
    pub blocker: Option<String>,
    pub next_action: Option<String>,
}

/// Mutable item fields for a CAS update (status changes go through
/// [`can_transition_item`] via the store transition path).
#[derive(Debug, Clone, Default)]
pub struct WorkItemPatch {
    pub parent_item_id: Option<Option<WorkItemId>>,
    pub dependencies: Option<Vec<WorkItemId>>,
    pub description: Option<String>,
    pub acceptance: Option<Vec<WorkAcceptance>>,
    pub evidence: Option<Vec<WorkEvidenceRef>>,
    pub owner_run_id: Option<Option<String>>,
    pub owner_job_id: Option<Option<String>>,
    pub blocker: Option<Option<String>>,
    pub next_action: Option<Option<String>>,
}

// ── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum WorkPlanError {
    #[error("work plan validation: {0}")]
    Validation(String),
    #[error("work plan revision conflict: expected {expected}, found {found}")]
    Conflict { expected: i64, found: i64 },
    #[error("work plan not found: {0}")]
    NotFound(String),
    #[error("work plan scope mismatch: {0}")]
    ScopeMismatch(String),
    #[error("work plan terminal state: {0}")]
    Terminal(String),
    #[error("work plan storage: {0}")]
    Storage(String),
}

impl From<crate::error::StorageError> for WorkPlanError {
    fn from(e: crate::error::StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}

impl From<sqlx::Error> for WorkPlanError {
    fn from(e: sqlx::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

// ── Pure validation ─────────────────────────────────────────────────────────

fn has_nul(value: &str) -> bool {
    value.contains('\0')
}

fn validate_scope_id(kind: &str, value: &str) -> Result<(), WorkPlanError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(WorkPlanError::Validation(format!(
            "{kind} must not be empty"
        )));
    }
    if value.len() > MAX_SCOPE_ID_CHARS {
        return Err(WorkPlanError::Validation(format!(
            "{kind} exceeds {MAX_SCOPE_ID_CHARS} chars"
        )));
    }
    if has_nul(value) {
        return Err(WorkPlanError::Validation(format!(
            "{kind} must not contain NUL"
        )));
    }
    Ok(())
}

fn validate_optional_scope_id(kind: &str, value: Option<&str>) -> Result<(), WorkPlanError> {
    if let Some(v) = value {
        validate_scope_id(kind, v)?;
    }
    Ok(())
}

fn validate_bounded(kind: &str, value: &str, max_chars: usize) -> Result<(), WorkPlanError> {
    if value.chars().count() > max_chars {
        return Err(WorkPlanError::Validation(format!(
            "{kind} exceeds {max_chars} chars"
        )));
    }
    if has_nul(value) {
        return Err(WorkPlanError::Validation(format!(
            "{kind} must not contain NUL"
        )));
    }
    Ok(())
}

fn validate_optional_bounded(
    kind: &str,
    value: Option<&str>,
    max_chars: usize,
) -> Result<(), WorkPlanError> {
    if let Some(v) = value {
        validate_bounded(kind, v, max_chars)?;
    }
    Ok(())
}

/// Deterministic origin digest over the objective text only. Provenance
/// and free-form progress text never enter the digest.
pub fn work_plan_origin_digest(objective: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(objective.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub fn validate_work_plan_id(id: &WorkPlanId) -> Result<(), WorkPlanError> {
    if !id.0.starts_with("wp_") {
        return Err(WorkPlanError::Validation(
            "work plan id must carry wp_ prefix".to_string(),
        ));
    }
    if id.0.len() > crate::identity::MAX_ID_LENGTH + 3 {
        return Err(WorkPlanError::Validation(
            "work plan id exceeds maximum length".to_string(),
        ));
    }
    if has_nul(&id.0) {
        return Err(WorkPlanError::Validation(
            "work plan id must not contain NUL".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_work_item_id(id: &WorkItemId) -> Result<(), WorkPlanError> {
    if !id.0.starts_with("wi_") {
        return Err(WorkPlanError::Validation(
            "work item id must carry wi_ prefix".to_string(),
        ));
    }
    if id.0.len() > crate::identity::MAX_ID_LENGTH + 3 {
        return Err(WorkPlanError::Validation(
            "work item id exceeds maximum length".to_string(),
        ));
    }
    if has_nul(&id.0) {
        return Err(WorkPlanError::Validation(
            "work item id must not contain NUL".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_new_work_plan(input: &NewWorkPlan) -> Result<(), WorkPlanError> {
    validate_scope_id("session_id", &input.session_id)?;
    validate_scope_id("project_id", &input.project_id)?;
    validate_optional_scope_id("origin_turn_id", input.origin_turn_id.as_deref())?;
    validate_optional_scope_id("goal_id", input.goal_id.as_deref())?;
    let objective = input.objective.trim();
    if objective.is_empty() {
        return Err(WorkPlanError::Validation(
            "objective must not be empty".to_string(),
        ));
    }
    validate_bounded("objective", &input.objective, MAX_OBJECTIVE_CHARS)?;
    if input.origin_provenance.trim().is_empty() {
        return Err(WorkPlanError::Validation(
            "origin_provenance must not be empty".to_string(),
        ));
    }
    validate_bounded(
        "origin_provenance",
        &input.origin_provenance,
        MAX_ORIGIN_PROVENANCE_CHARS,
    )?;
    validate_optional_bounded(
        "current_phase",
        input.current_phase.as_deref(),
        MAX_PHASE_CHARS,
    )?;
    Ok(())
}

pub fn validate_acceptance(criterion: &WorkAcceptance) -> Result<(), WorkPlanError> {
    if criterion.description.trim().is_empty() {
        return Err(WorkPlanError::Validation(
            "acceptance description must not be empty".to_string(),
        ));
    }
    validate_bounded(
        "acceptance description",
        &criterion.description,
        MAX_ACCEPTANCE_CHARS,
    )?;
    validate_optional_bounded(
        "acceptance note",
        criterion.note.as_deref(),
        MAX_ACCEPTANCE_NOTE_CHARS,
    )?;
    Ok(())
}

pub fn validate_evidence_ref(evidence: &WorkEvidenceRef) -> Result<(), WorkPlanError> {
    if evidence.ref_id.trim().is_empty() {
        return Err(WorkPlanError::Validation(
            "evidence ref_id must not be empty".to_string(),
        ));
    }
    validate_bounded("evidence ref_id", &evidence.ref_id, MAX_EVIDENCE_REF_CHARS)?;
    validate_optional_bounded(
        "evidence detail",
        evidence.detail.as_deref(),
        MAX_EVIDENCE_DETAIL_CHARS,
    )?;
    Ok(())
}

pub fn validate_new_work_item(
    plan_id: &WorkPlanId,
    input: &NewWorkItem,
    existing_count: usize,
) -> Result<(), WorkPlanError> {
    validate_work_plan_id(plan_id)?;
    if existing_count >= MAX_ITEMS_PER_PLAN {
        return Err(WorkPlanError::Validation(format!(
            "plan already holds {existing_count} items (max {MAX_ITEMS_PER_PLAN})"
        )));
    }
    if input.description.trim().is_empty() {
        return Err(WorkPlanError::Validation(
            "item description must not be empty".to_string(),
        ));
    }
    validate_bounded(
        "item description",
        &input.description,
        MAX_ITEM_DESCRIPTION_CHARS,
    )?;
    if input.dependencies.len() > MAX_DEPENDENCIES_PER_ITEM {
        return Err(WorkPlanError::Validation(format!(
            "dependencies exceed {MAX_DEPENDENCIES_PER_ITEM}"
        )));
    }
    if input.acceptance.len() > MAX_ACCEPTANCE_PER_ITEM {
        return Err(WorkPlanError::Validation(format!(
            "acceptance exceeds {MAX_ACCEPTANCE_PER_ITEM}"
        )));
    }
    if input.evidence.len() > MAX_EVIDENCE_PER_ITEM {
        return Err(WorkPlanError::Validation(format!(
            "evidence exceeds {MAX_EVIDENCE_PER_ITEM}"
        )));
    }
    for criterion in &input.acceptance {
        validate_acceptance(criterion)?;
    }
    for evidence in &input.evidence {
        validate_evidence_ref(evidence)?;
    }
    validate_optional_bounded(
        "owner_run_id",
        input.owner_run_id.as_deref(),
        MAX_OWNER_REF_CHARS,
    )?;
    validate_optional_bounded(
        "owner_job_id",
        input.owner_job_id.as_deref(),
        MAX_OWNER_REF_CHARS,
    )?;
    validate_optional_bounded("blocker", input.blocker.as_deref(), MAX_BLOCKER_CHARS)?;
    validate_optional_bounded(
        "next_action",
        input.next_action.as_deref(),
        MAX_NEXT_ACTION_CHARS,
    )?;
    validate_item_blocker_rule(input.status, input.blocker.as_deref())?;
    // Duplicate dependency check is structural; existence/cycle checks need
    // the full item set and live in validate_item_graph.
    let mut seen = HashSet::new();
    for dep in &input.dependencies {
        validate_work_item_id(dep)?;
        if !seen.insert(dep.as_str()) {
            return Err(WorkPlanError::Validation(
                "duplicate dependency".to_string(),
            ));
        }
    }
    if let Some(parent) = &input.parent_item_id {
        validate_work_item_id(parent)?;
    }
    Ok(())
}

/// Blocker text is required exactly when the item reports `Blocked`.
/// It carries a bounded host-visible reason; hidden reasoning never belongs
/// here.
pub fn validate_item_blocker_rule(
    status: WorkItemStatus,
    blocker: Option<&str>,
) -> Result<(), WorkPlanError> {
    match status {
        WorkItemStatus::Blocked => {
            let reason = blocker.unwrap_or("").trim();
            if reason.is_empty() {
                return Err(WorkPlanError::Validation(
                    "blocked items require a blocker reason".to_string(),
                ));
            }
            validate_bounded("blocker", blocker.unwrap_or(""), MAX_BLOCKER_CHARS)?;
            Ok(())
        }
        _ => {
            if let Some(text) = blocker {
                if !text.trim().is_empty() {
                    return Err(WorkPlanError::Validation(
                        "blocker reason is only valid for blocked items".to_string(),
                    ));
                }
            }
            Ok(())
        }
    }
}

/// Allowed explicit item transitions. Terminal states have no outgoing
/// edges. `Blocked` cannot complete directly: it must return to an
/// actionable path first so unblocking stays explicit.
pub fn can_transition_item(from: WorkItemStatus, to: WorkItemStatus) -> bool {
    if from == to {
        return true;
    }
    match from {
        WorkItemStatus::Pending => matches!(
            to,
            WorkItemStatus::Actionable
                | WorkItemStatus::InProgress
                | WorkItemStatus::Blocked
                | WorkItemStatus::Cancelled
        ),
        WorkItemStatus::Actionable => matches!(
            to,
            WorkItemStatus::Pending
                | WorkItemStatus::InProgress
                | WorkItemStatus::Blocked
                | WorkItemStatus::Completed
                | WorkItemStatus::Cancelled
        ),
        WorkItemStatus::InProgress => matches!(
            to,
            WorkItemStatus::Actionable
                | WorkItemStatus::Blocked
                | WorkItemStatus::Completed
                | WorkItemStatus::Cancelled
        ),
        WorkItemStatus::Blocked => matches!(
            to,
            WorkItemStatus::Pending
                | WorkItemStatus::Actionable
                | WorkItemStatus::InProgress
                | WorkItemStatus::Cancelled
        ),
        WorkItemStatus::Completed | WorkItemStatus::Cancelled => false,
    }
}

/// Allowed explicit plan transitions. Completion/cancellation are guarded
/// explicit decisions in M002; completion arbitration belongs to M003.
pub fn can_transition_plan(from: WorkPlanStatus, to: WorkPlanStatus) -> bool {
    if from == to {
        return true;
    }
    match from {
        WorkPlanStatus::Active => matches!(
            to,
            WorkPlanStatus::Blocked | WorkPlanStatus::Completed | WorkPlanStatus::Cancelled
        ),
        WorkPlanStatus::Blocked => matches!(
            to,
            WorkPlanStatus::Active | WorkPlanStatus::Completed | WorkPlanStatus::Cancelled
        ),
        WorkPlanStatus::Completed | WorkPlanStatus::Cancelled => false,
    }
}

/// Dependency-graph validation for one item against the plan's item set:
/// deps/parent must exist in the same plan, must not be self, and must not
/// introduce a cycle (parent chain or dependency closure).
pub fn validate_item_graph(
    item_id: &WorkItemId,
    parent: Option<&WorkItemId>,
    dependencies: &[WorkItemId],
    all_items: &[WorkItem],
) -> Result<(), WorkPlanError> {
    let known: HashSet<&str> = all_items.iter().map(|item| item.id.as_str()).collect();
    if let Some(parent_id) = parent {
        if parent_id == item_id {
            return Err(WorkPlanError::Validation(
                "item cannot be its own parent".to_string(),
            ));
        }
        if !known.contains(parent_id.as_str()) {
            return Err(WorkPlanError::Validation(format!(
                "parent item {} is not in this plan",
                parent_id.as_str()
            )));
        }
    }
    for dep in dependencies {
        if dep == item_id {
            return Err(WorkPlanError::Validation(
                "item cannot depend on itself".to_string(),
            ));
        }
        if !known.contains(dep.as_str()) {
            return Err(WorkPlanError::Validation(format!(
                "dependency {} is not in this plan",
                dep.as_str()
            )));
        }
        if let Some(parent_id) = parent {
            if dep == parent_id {
                // A parent edge plus a dependency edge to the same item is
                // redundant but harmless; keep it explicit rather than
                // rejecting, so ordering intent stays visible.
            }
        }
    }
    // Cycle check over dependency edges plus the candidate's own edges.
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for item in all_items {
        if item.id == *item_id {
            continue;
        }
        edges.insert(
            item.id.as_str(),
            item.dependencies.iter().map(|d| d.as_str()).collect(),
        );
    }
    edges.insert(
        item_id.as_str(),
        dependencies.iter().map(|d| d.as_str()).collect(),
    );
    if has_dependency_cycle(&edges) {
        return Err(WorkPlanError::Validation(
            "dependency cycle rejected".to_string(),
        ));
    }
    // Parent-chain cycle check.
    let mut parents: HashMap<&str, Option<&str>> = HashMap::new();
    for item in all_items {
        if item.id == *item_id {
            continue;
        }
        parents.insert(
            item.id.as_str(),
            item.parent_item_id.as_ref().map(|p| p.as_str()),
        );
    }
    parents.insert(item_id.as_str(), parent.map(|p| p.as_str()));
    let mut cursor = parent.map(|p| p.as_str());
    let mut visited = HashSet::new();
    while let Some(current) = cursor {
        if current == item_id.as_str() {
            return Err(WorkPlanError::Validation(
                "parent cycle rejected".to_string(),
            ));
        }
        if !visited.insert(current) {
            return Err(WorkPlanError::Validation(
                "parent cycle rejected".to_string(),
            ));
        }
        cursor = parents.get(current).copied().flatten();
    }
    Ok(())
}

fn has_dependency_cycle(edges: &HashMap<&str, Vec<&str>>) -> bool {
    // Iterative DFS with explicit color marks. Bounded by MAX_ITEMS_PER_PLAN
    // nodes so traversal cannot grow without bound.
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color: HashMap<&str, u8> = edges.keys().map(|k| (*k, WHITE)).collect();
    for root in edges.keys() {
        if color[root] != WHITE {
            continue;
        }
        let mut stack: Vec<(&str, bool)> = vec![(root, false)];
        while let Some((node, processed)) = stack.pop() {
            if processed {
                color.insert(node, BLACK);
                continue;
            }
            if color[node] == BLACK {
                continue;
            }
            if color[node] == GRAY {
                return true;
            }
            color.insert(node, GRAY);
            stack.push((node, true));
            if let Some(next) = edges.get(node) {
                for dep in next {
                    // Unknown targets are rejected earlier; ignore them here
                    // so validation order stays deterministic.
                    if !edges.contains_key(dep) {
                        continue;
                    }
                    match color.get(dep) {
                        Some(&GRAY) => return true,
                        Some(&BLACK) => continue,
                        _ => stack.push((dep, false)),
                    }
                }
            }
        }
    }
    false
}

/// Deterministic actionability: an item is actionable when it is
/// `Pending`/`Actionable` and every dependency is `Completed`.
/// Ordering is stable by `(position, id)` so hosts and tests observe one
/// order. Terminal, in-progress, and blocked items are never actionable.
pub fn actionable_items(items: &[WorkItem]) -> Vec<&WorkItem> {
    let completed: BTreeSet<&str> = items
        .iter()
        .filter(|item| item.status == WorkItemStatus::Completed)
        .map(|item| item.id.as_str())
        .collect();
    let mut out: Vec<&WorkItem> = items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                WorkItemStatus::Pending | WorkItemStatus::Actionable
            ) && item
                .dependencies
                .iter()
                .all(|dep| completed.contains(dep.as_str()))
        })
        .collect();
    out.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acceptance(description: &str) -> WorkAcceptance {
        WorkAcceptance {
            description: description.to_string(),
            disposition: WorkAcceptanceDisposition::Unmet,
            note: None,
        }
    }

    fn evidence(ref_id: &str) -> WorkEvidenceRef {
        WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: ref_id.to_string(),
            detail: None,
        }
    }

    fn item_fixture(id: &str, status: WorkItemStatus, deps: Vec<&str>) -> WorkItem {
        let now = Utc::now();
        WorkItem {
            id: WorkItemId(format!("wi_{id}")),
            plan_id: WorkPlanId("wp_plan".to_string()),
            revision: 0,
            position: 0,
            parent_item_id: None,
            dependencies: deps
                .into_iter()
                .map(|d| WorkItemId(format!("wi_{d}")))
                .collect(),
            status,
            description: "do work".to_string(),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn ids_carry_distinct_prefixes() {
        let plan = WorkPlanId::generate();
        let item = WorkItemId::generate();
        assert!(plan.as_str().starts_with("wp_"));
        assert!(item.as_str().starts_with("wi_"));
        assert!(validate_work_plan_id(&plan).is_ok());
        assert!(validate_work_item_id(&item).is_ok());
        assert!(validate_work_plan_id(&WorkPlanId("goal-1".to_string())).is_err());
    }

    #[test]
    fn bounds_reject_oversized_text() {
        let long = "x".repeat(MAX_OBJECTIVE_CHARS + 1);
        let input = NewWorkPlan {
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: long,
            origin_provenance: "turn:t".to_string(),
            current_phase: None,
        };
        assert!(validate_new_work_plan(&input).is_err());
    }

    #[test]
    fn item_bounds_reject_oversized_lists() {
        let plan = WorkPlanId("wp_p".to_string());
        let input = NewWorkItem {
            parent_item_id: None,
            dependencies: (0..MAX_DEPENDENCIES_PER_ITEM + 1)
                .map(|i| WorkItemId(format!("wi_dep{i}")))
                .collect(),
            status: WorkItemStatus::Pending,
            description: "d".to_string(),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            blocker: None,
            next_action: None,
        };
        assert!(validate_new_work_item(&plan, &input, 0).is_err());

        let many_acceptance = NewWorkItem {
            dependencies: vec![],
            acceptance: (0..MAX_ACCEPTANCE_PER_ITEM + 1)
                .map(|_| acceptance("a"))
                .collect(),
            ..input.clone()
        };
        assert!(validate_new_work_item(&plan, &many_acceptance, 0).is_err());

        let many_evidence = NewWorkItem {
            dependencies: vec![],
            acceptance: vec![],
            evidence: (0..MAX_EVIDENCE_PER_ITEM + 1)
                .map(|i| evidence(&format!("e{i}")))
                .collect(),
            ..input.clone()
        };
        assert!(validate_new_work_item(&plan, &many_evidence, 0).is_err());
    }

    #[test]
    fn max_items_bound_enforced() {
        let plan = WorkPlanId("wp_p".to_string());
        let input = NewWorkItem {
            parent_item_id: None,
            dependencies: vec![],
            status: WorkItemStatus::Pending,
            description: "d".to_string(),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            blocker: None,
            next_action: None,
        };
        assert!(validate_new_work_item(&plan, &input, MAX_ITEMS_PER_PLAN).is_err());
        assert!(validate_new_work_item(&plan, &input, MAX_ITEMS_PER_PLAN - 1).is_ok());
    }

    #[test]
    fn item_transition_matrix() {
        assert!(can_transition_item(
            WorkItemStatus::Pending,
            WorkItemStatus::Actionable
        ));
        assert!(!can_transition_item(
            WorkItemStatus::Pending,
            WorkItemStatus::Completed
        ));
        assert!(!can_transition_item(
            WorkItemStatus::Blocked,
            WorkItemStatus::Completed
        ));
        assert!(!can_transition_item(
            WorkItemStatus::Completed,
            WorkItemStatus::Actionable
        ));
        assert!(!can_transition_item(
            WorkItemStatus::Cancelled,
            WorkItemStatus::Pending
        ));
        assert!(can_transition_plan(
            WorkPlanStatus::Active,
            WorkPlanStatus::Completed
        ));
        assert!(!can_transition_plan(
            WorkPlanStatus::Completed,
            WorkPlanStatus::Active
        ));
    }

    #[test]
    fn blocker_rule_requires_reason_only_when_blocked() {
        assert!(validate_item_blocker_rule(WorkItemStatus::Blocked, Some("waiting")).is_ok());
        assert!(validate_item_blocker_rule(WorkItemStatus::Blocked, None).is_err());
        assert!(validate_item_blocker_rule(WorkItemStatus::Blocked, Some("  ")).is_err());
        assert!(validate_item_blocker_rule(WorkItemStatus::Actionable, Some("x")).is_err());
        assert!(validate_item_blocker_rule(WorkItemStatus::Actionable, None).is_ok());
    }

    #[test]
    fn actionability_gates_on_completed_dependencies() {
        let mut a = item_fixture("a", WorkItemStatus::Completed, vec![]);
        a.position = 0;
        let mut b = item_fixture("b", WorkItemStatus::Pending, vec!["a"]);
        b.position = 1;
        let mut c = item_fixture("c", WorkItemStatus::Pending, vec!["b"]);
        c.position = 2;
        let mut d = item_fixture("d", WorkItemStatus::InProgress, vec![]);
        d.position = 3;
        let items = vec![c.clone(), b.clone(), d.clone(), a.clone()];
        let actionable = actionable_items(&items);
        assert_eq!(actionable.len(), 1);
        assert_eq!(actionable[0].id.as_str(), "wi_b");
    }

    #[test]
    fn cycle_rejected() {
        let a = item_fixture("a", WorkItemStatus::Pending, vec![]);
        let mut b = item_fixture("b", WorkItemStatus::Pending, vec!["a"]);
        b.position = 1;
        let items = vec![a, b];
        // New item c depending on b is fine.
        assert!(validate_item_graph(
            &WorkItemId("wi_c".to_string()),
            None,
            &[WorkItemId("wi_b".to_string())],
            &items
        )
        .is_ok());
        // Self dependency rejected.
        assert!(validate_item_graph(
            &WorkItemId("wi_a".to_string()),
            None,
            &[WorkItemId("wi_a".to_string())],
            &items
        )
        .is_err());
        // Unknown dependency rejected (cross-plan/cross-scope safety).
        assert!(validate_item_graph(
            &WorkItemId("wi_c".to_string()),
            None,
            &[WorkItemId("wi_missing".to_string())],
            &items
        )
        .is_err());
        // Two-node cycle: make a depend on b while b depends on a.
        let mut cyclic_a = item_fixture("a", WorkItemStatus::Pending, vec!["b"]);
        cyclic_a.position = 0;
        let cyclic_b = item_fixture("b", WorkItemStatus::Pending, vec!["a"]);
        let cyclic_items = vec![cyclic_a, cyclic_b];
        assert!(validate_item_graph(
            &WorkItemId("wi_a".to_string()),
            None,
            &[WorkItemId("wi_b".to_string())],
            &cyclic_items
        )
        .is_err());
    }

    #[test]
    fn origin_digest_stable_and_objective_only() {
        let first = work_plan_origin_digest("implement work plans");
        let second = work_plan_origin_digest("implement work plans");
        assert_eq!(first, second);
        assert!(first.starts_with("sha256:"));
        assert_ne!(first, work_plan_origin_digest("implement work plans!"));
    }

    #[test]
    fn evidence_shape_validated() {
        assert!(validate_evidence_ref(&evidence("job-1")).is_ok());
        let empty = WorkEvidenceRef {
            kind: WorkEvidenceKind::Artifact,
            ref_id: "  ".to_string(),
            detail: None,
        };
        assert!(validate_evidence_ref(&empty).is_err());
    }

    #[test]
    fn no_reasoning_payload_exists() {
        // Structural guard: serializing plan/item must never expose a
        // reasoning/thinking field. If such a field is added, this test
        // names it loudly instead of silently persisting it.
        let now = Utc::now();
        let plan = WorkPlan {
            id: WorkPlanId("wp_p".to_string()),
            revision: 0,
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "o".to_string(),
            objective_digest: work_plan_origin_digest("o"),
            origin_provenance: "turn:t".to_string(),
            status: WorkPlanStatus::Active,
            current_phase: None,
            current_item_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        let json = serde_json::to_string(&plan).expect("serialize");
        for forbidden in ["reasoning", "thinking", "chain_of_thought", "scratchpad"] {
            assert!(!json.contains(forbidden), "forbidden field {forbidden}");
        }
    }
}
