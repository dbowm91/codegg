//! Durable `WorkOrder`/`WorkOrderOccurrence`/`SequenceLane` store (Project Work Orders M001).
//!
//! One store/service owner with CAS semantics. Simultaneous updates and
//! reorders return an explicit [`WorkOrderError::RevisionConflict`]
//! rather than last-writer-wins. Batch creation is all-or-nothing in one
//! transaction. Idempotent submissions converge on stored rows; a reused
//! key with a different payload is an explicit mismatch conflict.
//!
//! The store never executes work: it creates no session rows, submits no
//! jobs, allocates no worktrees, and invokes no models. It performs no
//! authorization itself; every method takes the already-resolved
//! [`ProjectId`] scope and fails with [`WorkOrderError::NotFound`] on
//! scope mismatch so callers cannot distinguish absent rows from foreign
//! rows.

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::model::{
    can_transition_work_order, validate_diagnostic, validate_gate_set, validate_idempotency_key,
    validate_lane_label, validate_model, validate_parent_ref, validate_prompt,
    validate_repeat_count, validate_title, ApprovalRequest, AttentionCode, GateJoin, GateKind,
    GateSpec, LaneFailurePolicy, NewSequenceLane, NewWorkOrder, OccurrenceState, ReleaseGateSet,
    SandboxRequest, SequenceLane, WorkOrder, WorkOrderError, WorkOrderOccurrence, WorkOrderPatch,
    WorkOrderState, WorkOrderSummary, WorkspacePolicy, MAX_IDEMPOTENCY_KEY_LEN,
    MAX_LANES_PER_PROJECT, MAX_LANE_MEMBERS, MAX_WORK_ORDER_BATCH_ITEMS, MAX_WORK_ORDER_LIST_LIMIT,
};
use crate::error::StorageError;
use crate::identity::{PrincipalId, ProjectId, SequenceLaneId, WorkOrderId, WorkOrderOccurrenceId};

// ── Schema ───────────────────────────────────────────────────────────────

/// Additive M001 schema. Safe on existing databases via `IF NOT EXISTS`.
/// Existing databases gain empty work-order tables; no `schedule` row is
/// ever backfilled. Length/state CHECKs mirror domain validation as a
/// defense-in-depth backstop.
pub const WORK_ORDER_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS work_order (
        id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL CHECK (revision >= 1),
        project_id TEXT NOT NULL CHECK (length(project_id) > 0 AND length(project_id) <= 128),
        creator_principal TEXT NOT NULL CHECK (length(creator_principal) > 0 AND length(creator_principal) <= 128),
        parent_session_id TEXT CHECK (parent_session_id IS NULL OR (length(parent_session_id) > 0 AND length(parent_session_id) <= 128)),
        parent_turn_id TEXT CHECK (parent_turn_id IS NULL OR (length(parent_turn_id) > 0 AND length(parent_turn_id) <= 128)),
        parent_work_order_id TEXT CHECK (parent_work_order_id IS NULL OR (length(parent_work_order_id) > 0 AND length(parent_work_order_id) <= 128)),
        title TEXT CHECK (title IS NULL OR (length(title) > 0 AND length(title) <= 256)),
        prompt TEXT NOT NULL CHECK (length(prompt) > 0 AND length(prompt) <= 32768),
        requested_model TEXT CHECK (requested_model IS NULL OR (length(requested_model) > 0 AND length(requested_model) <= 256)),
        requested_approval TEXT CHECK (requested_approval IS NULL OR requested_approval IN ('interactive','automatic','yolo')),
        requested_sandbox TEXT CHECK (requested_sandbox IS NULL OR requested_sandbox IN ('read_only','workspace_write','full_host')),
        workspace_policy TEXT CHECK (workspace_policy IS NULL OR workspace_policy IN ('auto_isolated','shared','serialized')),
        gates_json TEXT NOT NULL CHECK (length(gates_json) > 0 AND length(gates_json) <= 4096),
        gate_join TEXT NOT NULL CHECK (gate_join IN ('all','any')),
        repeat_count INTEGER NOT NULL CHECK (repeat_count >= 1 AND repeat_count <= 256),
        state TEXT NOT NULL CHECK (state IN ('active','paused','completed','cancelled','archived')),
        submission_key TEXT CHECK (submission_key IS NULL OR (length(submission_key) > 0 AND length(submission_key) <= 128)),
        spec_digest TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        cancelled_at INTEGER,
        UNIQUE(project_id, submission_key)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS work_order_occurrence (
        id TEXT PRIMARY KEY,
        work_order_id TEXT NOT NULL REFERENCES work_order(id) ON DELETE CASCADE,
        project_id TEXT NOT NULL CHECK (length(project_id) > 0 AND length(project_id) <= 128),
        occurrence_index INTEGER NOT NULL CHECK (occurrence_index >= 0),
        state TEXT NOT NULL CHECK (state IN ('waiting','ready','claiming','running','needs_attention','completed','failed','cancelled')),
        gate_latches_json TEXT NOT NULL DEFAULT '[]',
        not_before INTEGER,
        next_check_at INTEGER,
        session_id TEXT,
        job_id TEXT,
        workspace_id TEXT,
        worktree_id TEXT,
        claimed_at INTEGER,
        started_at INTEGER,
        terminal_at INTEGER,
        attention_code TEXT CHECK (attention_code IS NULL OR attention_code IN ('model_unavailable','policy_narrowed','worktree_conflict','trigger_expired','predecessor_failed','predecessor_attention','materialization_failed')),
        diagnostic TEXT CHECK (diagnostic IS NULL OR length(diagnostic) <= 1024),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(work_order_id, occurrence_index)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS sequence_lane (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL CHECK (length(project_id) > 0 AND length(project_id) <= 128),
        revision INTEGER NOT NULL CHECK (revision >= 1),
        label TEXT CHECK (label IS NULL OR (length(label) > 0 AND length(label) <= 128)),
        failure_policy TEXT NOT NULL CHECK (failure_policy IN ('hold_lane','continue_lane')),
        submission_key TEXT CHECK (submission_key IS NULL OR (length(submission_key) > 0 AND length(submission_key) <= 128)),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(project_id, submission_key)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS sequence_lane_member (
        lane_id TEXT NOT NULL REFERENCES sequence_lane(id) ON DELETE CASCADE,
        work_order_id TEXT NOT NULL REFERENCES work_order(id) ON DELETE CASCADE,
        position INTEGER NOT NULL CHECK (position >= 0),
        PRIMARY KEY(lane_id, work_order_id),
        UNIQUE(lane_id, position)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS work_order_batch (
        project_id TEXT NOT NULL,
        batch_key TEXT NOT NULL CHECK (length(batch_key) > 0 AND length(batch_key) <= 128),
        work_order_ids_json TEXT NOT NULL,
        spec_digest TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY(project_id, batch_key)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_work_order_project_state ON work_order(project_id, state, updated_at DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS idx_work_order_project_updated ON work_order(project_id, updated_at DESC, id DESC)",
    "CREATE INDEX IF NOT EXISTS idx_work_order_submission ON work_order(project_id, submission_key) WHERE submission_key IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_occurrence_work_order ON work_order_occurrence(work_order_id, occurrence_index)",
    "CREATE INDEX IF NOT EXISTS idx_occurrence_project_state ON work_order_occurrence(project_id, state)",
    "CREATE INDEX IF NOT EXISTS idx_lane_project ON sequence_lane(project_id, created_at)",
    "CREATE INDEX IF NOT EXISTS idx_lane_member_lane ON sequence_lane_member(lane_id, position)",
    "CREATE INDEX IF NOT EXISTS idx_lane_member_work_order ON sequence_lane_member(work_order_id)",
];

/// Ensure the work-order tables exist. Idempotent.
pub async fn ensure_work_order_tables(pool: &SqlitePool) -> Result<(), WorkOrderError> {
    for statement in WORK_ORDER_SCHEMA_STATEMENTS {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| WorkOrderError::Storage(StorageError::Migration(e.to_string())))?;
    }
    Ok(())
}

/// Owning project of one work-order, occurrence, or lane id, if the row
/// exists.
///
/// Used by the daemon authorization resolver so ID-only requests map to
/// a project scope. Unknown or malformed ids return `None` so team
/// principals fail closed.
pub async fn work_order_project(pool: &SqlitePool, raw_id: &str) -> Option<ProjectId> {
    for table in ["work_order", "work_order_occurrence", "sequence_lane"] {
        let query = format!("SELECT project_id FROM {table} WHERE id = ?");
        let row: Option<(String,)> = sqlx::query_as(&query)
            .bind(raw_id)
            .fetch_optional(pool)
            .await
            .ok()?;
        if let Some((project_raw,)) = row {
            return ProjectId::parse(&project_raw).ok();
        }
    }
    None
}

// ── Row mapping ──────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct WorkOrderRow {
    id: String,
    revision: i64,
    project_id: String,
    creator_principal: String,
    parent_session_id: Option<String>,
    parent_turn_id: Option<String>,
    parent_work_order_id: Option<String>,
    title: Option<String>,
    prompt: String,
    requested_model: Option<String>,
    requested_approval: Option<String>,
    requested_sandbox: Option<String>,
    workspace_policy: Option<String>,
    gates_json: String,
    gate_join: String,
    repeat_count: i64,
    state: String,
    spec_digest: String,
    created_at: i64,
    updated_at: i64,
    cancelled_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct OccurrenceRow {
    id: String,
    work_order_id: String,
    project_id: String,
    occurrence_index: i64,
    state: String,
    gate_latches_json: String,
    not_before: Option<i64>,
    next_check_at: Option<i64>,
    session_id: Option<String>,
    job_id: Option<String>,
    workspace_id: Option<String>,
    worktree_id: Option<String>,
    claimed_at: Option<i64>,
    started_at: Option<i64>,
    terminal_at: Option<i64>,
    attention_code: Option<String>,
    diagnostic: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(sqlx::FromRow)]
struct LaneRow {
    id: String,
    project_id: String,
    revision: i64,
    label: Option<String>,
    failure_policy: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct StoredGate {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delay_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    not_before_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trigger_ref: Option<String>,
}

fn encode_gate_set(gates: &ReleaseGateSet) -> Result<String, WorkOrderError> {
    let stored: Vec<StoredGate> = gates
        .gates
        .iter()
        .map(|gate| StoredGate {
            kind: gate.kind.as_str().to_owned(),
            delay_secs: gate.delay_secs,
            not_before_ms: gate.not_before_ms,
            lane_id: gate.lane_id.as_ref().map(|id| id.as_str().to_owned()),
            trigger_ref: gate.trigger_ref.clone(),
        })
        .collect();
    serde_json::to_string(&stored)
        .map_err(|e| WorkOrderError::invalid("gates", format!("gate set is not serializable: {e}")))
}

fn decode_gate_set(gates_json: &str, gate_join: &str) -> Result<ReleaseGateSet, WorkOrderError> {
    let bad = || WorkOrderError::invalid("gates", "stored gate set failed to parse");
    let stored: Vec<StoredGate> = serde_json::from_str(gates_json).map_err(|_| bad())?;
    let mut gates = Vec::with_capacity(stored.len());
    for raw in stored {
        let kind = GateKind::parse(&raw.kind).ok_or_else(bad)?;
        gates.push(GateSpec {
            kind,
            delay_secs: raw.delay_secs,
            not_before_ms: raw.not_before_ms,
            lane_id: raw
                .lane_id
                .map(|id| SequenceLaneId::parse(&id))
                .transpose()
                .map_err(|_| bad())?,
            trigger_ref: raw.trigger_ref,
        });
    }
    let join = GateJoin::parse(gate_join).ok_or_else(bad)?;
    validate_gate_set(&gates, join)
}

fn parse_opt<T>(
    raw: Option<String>,
    field: &'static str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Option<T>, WorkOrderError> {
    raw.map(|value| {
        parse(&value).ok_or_else(|| WorkOrderError::invalid(field, "stored row failed to parse"))
    })
    .transpose()
}

fn row_to_work_order(row: WorkOrderRow) -> Result<WorkOrder, WorkOrderError> {
    let bad = |field: &'static str| WorkOrderError::invalid(field, "stored row failed to parse");
    Ok(WorkOrder {
        id: WorkOrderId::parse(&row.id).map_err(|_| bad("work_order_id"))?,
        revision: u64::try_from(row.revision).map_err(|_| bad("revision"))?,
        project_id: ProjectId::parse(&row.project_id).map_err(|_| bad("project_id"))?,
        creator_principal: PrincipalId::parse(&row.creator_principal)
            .map_err(|_| bad("creator_principal"))?,
        parent_session_id: row.parent_session_id,
        parent_turn_id: row.parent_turn_id,
        parent_work_order_id: row
            .parent_work_order_id
            .map(|id| WorkOrderId::parse(&id))
            .transpose()
            .map_err(|_| bad("parent_work_order_id"))?,
        title: row.title,
        prompt: row.prompt,
        requested_model: row.requested_model,
        requested_approval: parse_opt(
            row.requested_approval,
            "requested_approval",
            ApprovalRequest::parse,
        )?,
        requested_sandbox: parse_opt(
            row.requested_sandbox,
            "requested_sandbox",
            SandboxRequest::parse,
        )?,
        workspace_policy: parse_opt(
            row.workspace_policy,
            "workspace_policy",
            WorkspacePolicy::parse,
        )?,
        gates: decode_gate_set(&row.gates_json, &row.gate_join)?,
        repeat_count: u32::try_from(row.repeat_count).map_err(|_| bad("repeat_count"))?,
        sequence_lane_id: None, // Resolved from lane membership by the service, not stored on the row.
        state: WorkOrderState::parse(&row.state).ok_or_else(|| bad("state"))?,
        created_at_ms: row.created_at,
        updated_at_ms: row.updated_at,
        cancelled_at_ms: row.cancelled_at,
    })
}

fn row_to_occurrence(row: OccurrenceRow) -> Result<WorkOrderOccurrence, WorkOrderError> {
    let bad = |field: &'static str| WorkOrderError::invalid(field, "stored row failed to parse");
    let latches_raw: Vec<String> =
        serde_json::from_str(&row.gate_latches_json).map_err(|_| bad("gate_latches"))?;
    let mut gate_latches = Vec::with_capacity(latches_raw.len());
    for raw in latches_raw {
        gate_latches.push(GateKind::parse(&raw).ok_or_else(|| bad("gate_latches"))?);
    }
    Ok(WorkOrderOccurrence {
        id: WorkOrderOccurrenceId::parse(&row.id).map_err(|_| bad("occurrence_id"))?,
        work_order_id: WorkOrderId::parse(&row.work_order_id).map_err(|_| bad("work_order_id"))?,
        project_id: ProjectId::parse(&row.project_id).map_err(|_| bad("project_id"))?,
        occurrence_index: u64::try_from(row.occurrence_index)
            .map_err(|_| bad("occurrence_index"))?,
        state: OccurrenceState::parse(&row.state).ok_or_else(|| bad("state"))?,
        gate_latches,
        not_before_ms: row.not_before,
        next_check_at_ms: row.next_check_at,
        session_id: row.session_id,
        job_id: row.job_id,
        workspace_id: row.workspace_id,
        worktree_id: row.worktree_id,
        attention_code: parse_opt(row.attention_code, "attention_code", AttentionCode::parse)?,
        diagnostic: row.diagnostic,
        claimed_at_ms: row.claimed_at,
        started_at_ms: row.started_at,
        terminal_at_ms: row.terminal_at,
        created_at_ms: row.created_at,
        updated_at_ms: row.updated_at,
    })
}

// ── Config and service ───────────────────────────────────────────────────

/// Bounds for the durable work-order tables.
#[derive(Debug, Clone)]
pub struct WorkOrderConfig {
    /// Maximum page size served by list operations.
    pub max_list_limit: u32,
    /// Default page size when the caller passes no limit.
    pub default_list_limit: u32,
    /// Maximum items accepted in one atomic batch.
    pub max_batch_items: usize,
    /// Maximum lanes retained per project.
    pub max_lanes_per_project: usize,
    /// Maximum members in one lane.
    pub max_lane_members: usize,
}

impl Default for WorkOrderConfig {
    fn default() -> Self {
        Self {
            max_list_limit: MAX_WORK_ORDER_LIST_LIMIT,
            default_list_limit: super::model::DEFAULT_WORK_ORDER_LIST_LIMIT,
            max_batch_items: MAX_WORK_ORDER_BATCH_ITEMS,
            max_lanes_per_project: MAX_LANES_PER_PROJECT,
            max_lane_members: MAX_LANE_MEMBERS,
        }
    }
}

impl WorkOrderConfig {
    /// Wire capability advertisement for this configuration.
    pub fn capabilities_dto(&self) -> codegg_protocol::work_order::WorkOrderCapabilitiesDto {
        codegg_protocol::work_order::WorkOrderCapabilitiesDto {
            supported: true,
            protocol_version: codegg_protocol::work_order::WORK_ORDER_PROTOCOL_VERSION,
            max_title_chars: super::model::MAX_WORK_ORDER_TITLE_CHARS,
            max_prompt_bytes: super::model::MAX_WORK_ORDER_PROMPT_BYTES,
            max_batch_items: self.max_batch_items,
            max_list_limit: self.max_list_limit,
            max_repeat_count: super::model::MAX_REPEAT_COUNT,
            max_lanes_per_project: self.max_lanes_per_project,
            max_lane_members: self.max_lane_members,
        }
    }

    fn clamp_list_limit(&self, requested: Option<u32>) -> u32 {
        let limit = requested.unwrap_or(self.default_list_limit).max(1);
        limit.min(self.max_list_limit)
    }
}

/// Outcome of a creation: the durable work order plus whether a retry
/// converged on an already-stored idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOutcome {
    pub work_order: WorkOrder,
    pub duplicate: bool,
}

/// Outcome of an atomic batch creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOutcome {
    pub batch_key: Option<String>,
    pub work_orders: Vec<WorkOrder>,
    pub duplicate: bool,
}

/// Bounded work-order listing page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkOrderListPage {
    pub work_orders: Vec<WorkOrder>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
}

/// Bounded occurrence listing page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceListPage {
    pub occurrences: Vec<WorkOrderOccurrence>,
    pub truncated: bool,
}

/// Daemon-owned project work-order service.
///
/// Holds an optional durable pool. When no pool is present, durable
/// operations fail with [`WorkOrderError::Unavailable`]. The daemon
/// constructs one shared instance; restart reopens the same catalog so
/// identities and revisions survive.
#[derive(Debug, Clone)]
pub struct WorkOrderService {
    pool: Option<SqlitePool>,
    config: WorkOrderConfig,
}

impl WorkOrderService {
    pub fn new(pool: Option<SqlitePool>, config: WorkOrderConfig) -> Self {
        Self { pool, config }
    }

    pub fn with_defaults(pool: Option<SqlitePool>) -> Self {
        Self::new(pool, WorkOrderConfig::default())
    }

    pub fn config(&self) -> &WorkOrderConfig {
        &self.config
    }

    pub fn capabilities_dto(&self) -> codegg_protocol::work_order::WorkOrderCapabilitiesDto {
        self.config.capabilities_dto()
    }

    fn durable_pool(&self) -> Result<SqlitePool, WorkOrderError> {
        self.pool.clone().ok_or_else(|| {
            WorkOrderError::Unavailable(
                "project work orders require a durable database pool".to_owned(),
            )
        })
    }

    // ── Create ───────────────────────────────────────────────────────

    /// Create one durable waiting work order. Duplicate submission keys
    /// return the stored row; a reused key with a different payload is
    /// an explicit mismatch conflict.
    pub async fn create_work_order(
        &self,
        project: &ProjectId,
        creator: &PrincipalId,
        input: NewWorkOrder,
        now_ms: i64,
    ) -> Result<CreateOutcome, WorkOrderError> {
        let pool = self.durable_pool()?;
        let clean = self.validate_new_input(&pool, project, &input).await?;
        let key = validate_idempotency_key(input.idempotency_key.as_deref())?;

        if let Some(ref key) = key {
            if let Some(existing) = self.by_submission_key(&pool, project, key).await? {
                if existing.spec_digest_mismatch(&clean.digest) {
                    return Err(WorkOrderError::IdempotencyConflict(key.clone()));
                }
                let work_order = self.hydrate(&pool, existing.row).await?;
                return Ok(CreateOutcome {
                    work_order,
                    duplicate: true,
                });
            }
        }

        let id = WorkOrderId::new();
        sqlx::query(
            "INSERT INTO work_order (id, revision, project_id, creator_principal, parent_session_id, \
             parent_turn_id, parent_work_order_id, title, prompt, requested_model, requested_approval, \
             requested_sandbox, workspace_policy, gates_json, gate_join, repeat_count, state, \
             submission_key, spec_digest, created_at, updated_at, cancelled_at) \
             VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, ?, ?, NULL)",
        )
        .bind(id.as_str())
        .bind(project.as_str())
        .bind(creator.as_str())
        .bind(clean.parent_session_id.clone())
        .bind(clean.parent_turn_id.clone())
        .bind(clean.parent_work_order_id.clone())
        .bind(clean.title.clone())
        .bind(clean.prompt.clone())
        .bind(clean.requested_model.clone())
        .bind(clean.requested_approval.map(ApprovalRequest::as_str))
        .bind(clean.requested_sandbox.map(SandboxRequest::as_str))
        .bind(clean.workspace_policy.map(WorkspacePolicy::as_str))
        .bind(clean.gates_json.clone())
        .bind(clean.gate_join.clone())
        .bind(i64::from(clean.repeat_count))
        .bind(key.clone())
        .bind(clean.digest.clone())
        .bind(now_ms)
        .bind(now_ms)
        .execute(&pool)
        .await
        .map_err(map_unique_violation)?;

        if let Some(ref lane_id) = clean.sequence_lane_id {
            self.append_to_lane(&pool, project, lane_id, &id, now_ms)
                .await?;
        }

        let work_order = self
            .get_work_order(project, &id)
            .await?
            .ok_or_else(|| WorkOrderError::NotFound(id.as_str().to_owned()))?;
        Ok(CreateOutcome {
            work_order,
            duplicate: false,
        })
    }

    /// Atomically create a bounded ordered batch of work orders.
    /// Either every item (and the optional lane ordering) commits in one
    /// transaction or none does.
    #[allow(clippy::too_many_arguments)]
    pub async fn batch_create_work_orders(
        &self,
        project: &ProjectId,
        creator: &PrincipalId,
        items: Vec<NewWorkOrder>,
        sequence_lane_id: Option<SequenceLaneId>,
        batch_key: Option<String>,
        now_ms: i64,
    ) -> Result<BatchOutcome, WorkOrderError> {
        let pool = self.durable_pool()?;
        if items.is_empty() || items.len() > self.config.max_batch_items {
            return Err(WorkOrderError::invalid(
                "items",
                format!("batch must carry 1..={} items", self.config.max_batch_items),
            ));
        }
        let batch_key = validate_idempotency_key(batch_key.as_deref())?;
        if let Some(ref lane_id) = sequence_lane_id {
            self.require_lane_in_project(&pool, project, lane_id)
                .await?;
        }

        let mut cleaned = Vec::with_capacity(items.len());
        for item in &items {
            let mut item = item.clone();
            if sequence_lane_id.is_some() {
                if item.sequence_lane_id.is_some() {
                    return Err(WorkOrderError::invalid(
                        "sequence_lane_id",
                        "batch lane placement is exclusive: set it once on the batch or per item, not both",
                    ));
                }
                item.sequence_lane_id.clone_from(&sequence_lane_id);
            }
            cleaned.push(self.validate_new_input(&pool, project, &item).await?);
        }
        let batch_digest = batch_spec_digest(&cleaned);

        if let Some(ref key) = batch_key {
            if let Some(existing) = self.batch_by_key(&pool, project, key).await? {
                if existing != batch_digest {
                    return Err(WorkOrderError::IdempotencyConflict(key.clone()));
                }
                let ids = self.batch_member_ids(&pool, project, key).await?;
                let mut work_orders = Vec::with_capacity(ids.len());
                for id in ids {
                    let Some(work_order) = self.get_work_order(project, &id).await? else {
                        return Err(WorkOrderError::Storage(StorageError::Database(
                            "batch member row lost".to_owned(),
                        )));
                    };
                    work_orders.push(work_order);
                }
                return Ok(BatchOutcome {
                    batch_key,
                    work_orders,
                    duplicate: true,
                });
            }
        }

        // All-or-nothing: insert every work order, then the lane
        // positions, then the batch ledger inside one transaction.
        let mut tx = pool.begin().await?;
        let mut ids = Vec::with_capacity(cleaned.len());
        for clean in &cleaned {
            let id = WorkOrderId::new();
            sqlx::query(
                "INSERT INTO work_order (id, revision, project_id, creator_principal, parent_session_id, \
                 parent_turn_id, parent_work_order_id, title, prompt, requested_model, requested_approval, \
                 requested_sandbox, workspace_policy, gates_json, gate_join, repeat_count, state, \
                 submission_key, spec_digest, created_at, updated_at, cancelled_at) \
                 VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, ?, ?, NULL)",
            )
            .bind(id.as_str())
            .bind(project.as_str())
            .bind(creator.as_str())
            .bind(clean.parent_session_id.clone())
            .bind(clean.parent_turn_id.clone())
            .bind(clean.parent_work_order_id.clone())
            .bind(clean.title.clone())
            .bind(clean.prompt.clone())
            .bind(clean.requested_model.clone())
            .bind(clean.requested_approval.map(ApprovalRequest::as_str))
            .bind(clean.requested_sandbox.map(SandboxRequest::as_str))
            .bind(clean.workspace_policy.map(WorkspacePolicy::as_str))
            .bind(clean.gates_json.clone())
            .bind(clean.gate_join.clone())
            .bind(i64::from(clean.repeat_count))
            .bind(clean.submission_key.clone())
            .bind(clean.digest.clone())
            .bind(now_ms)
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(map_unique_violation)?;
            ids.push(id);
        }
        if let Some(ref lane_id) = sequence_lane_id {
            let base: Option<i64> = sqlx::query_scalar(
                "SELECT COALESCE(MAX(position), -1) FROM sequence_lane_member WHERE lane_id = ?",
            )
            .bind(lane_id.as_str())
            .fetch_one(&mut *tx)
            .await?;
            let base: i64 = base.unwrap_or(-1) + 1;
            for (offset, id) in ids.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO sequence_lane_member (lane_id, work_order_id, position) VALUES (?, ?, ?)",
                )
                .bind(lane_id.as_str())
                .bind(id.as_str())
                .bind(base + offset as i64)
                .execute(&mut *tx)
                .await
                .map_err(map_unique_violation)?;
            }
            self.bump_lane_revision_tx(&mut tx, lane_id, now_ms).await?;
        } else {
            // Per-item lane placement commits in the same transaction.
            for (clean, id) in cleaned.iter().zip(ids.iter()) {
                if let Some(ref lane_id) = clean.sequence_lane_id {
                    let base: Option<i64> = sqlx::query_scalar(
                        "SELECT COALESCE(MAX(position), -1) FROM sequence_lane_member WHERE lane_id = ?",
                    )
                    .bind(lane_id.as_str())
                    .fetch_one(&mut *tx)
                    .await?;
                    sqlx::query(
                        "INSERT INTO sequence_lane_member (lane_id, work_order_id, position) VALUES (?, ?, ?)",
                    )
                    .bind(lane_id.as_str())
                    .bind(id.as_str())
                    .bind(base.unwrap_or(-1) + 1)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_unique_violation)?;
                    self.bump_lane_revision_tx(&mut tx, lane_id, now_ms).await?;
                }
            }
        }
        if let Some(ref key) = batch_key {
            let member_ids: Vec<&str> = ids.iter().map(WorkOrderId::as_str).collect();
            let member_json =
                serde_json::to_string(&member_ids).unwrap_or_else(|_| "[]".to_owned());
            sqlx::query(
                "INSERT INTO work_order_batch (project_id, batch_key, work_order_ids_json, spec_digest, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(project.as_str())
            .bind(key)
            .bind(member_json)
            .bind(&batch_digest)
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(map_unique_violation)?;
        }
        tx.commit().await?;

        let mut work_orders = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(work_order) = self.get_work_order(project, &id).await? else {
                return Err(WorkOrderError::Storage(StorageError::Database(
                    "batch member row lost".to_owned(),
                )));
            };
            work_orders.push(work_order);
        }
        Ok(BatchOutcome {
            batch_key,
            work_orders,
            duplicate: false,
        })
    }

    // ── Read ─────────────────────────────────────────────────────────

    /// Fetch one work order in project scope. Foreign or absent rows
    /// report `None` (the daemon maps this to the privacy-preserving
    /// not-found shape).
    pub async fn get_work_order(
        &self,
        project: &ProjectId,
        id: &WorkOrderId,
    ) -> Result<Option<WorkOrder>, WorkOrderError> {
        let pool = self.durable_pool()?;
        let row: Option<WorkOrderRow> =
            sqlx::query_as("SELECT * FROM work_order WHERE id = ? AND project_id = ?")
                .bind(id.as_str())
                .bind(project.as_str())
                .fetch_optional(&pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut work_order = row_to_work_order(row)?;
        work_order.sequence_lane_id = self.lane_for_work_order(&pool, &work_order.id).await?;
        Ok(Some(work_order))
    }

    /// Bounded work-order listing for one project, newest first.
    /// `cursor` is the opaque `next_cursor` of a prior page.
    pub async fn list_work_orders(
        &self,
        project: &ProjectId,
        state_filter: Option<WorkOrderState>,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<WorkOrderListPage, WorkOrderError> {
        let pool = self.durable_pool()?;
        let bound = self.config.clamp_list_limit(limit);
        let (cursor_at, cursor_id) = parse_cursor(cursor)?;
        let rows: Vec<WorkOrderRow> = if let Some(filter) = state_filter {
            sqlx::query_as(
                "SELECT * FROM work_order WHERE project_id = ? AND state = ? \
                 AND ((updated_at < ?) OR (updated_at = ? AND id < ?)) \
                 ORDER BY updated_at DESC, id DESC LIMIT ?",
            )
            .bind(project.as_str())
            .bind(filter.as_str())
            .bind(cursor_at)
            .bind(cursor_at)
            .bind(cursor_id)
            .bind(i64::from(bound) + 1)
            .fetch_all(&pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT * FROM work_order WHERE project_id = ? \
                 AND ((updated_at < ?) OR (updated_at = ? AND id < ?)) \
                 ORDER BY updated_at DESC, id DESC LIMIT ?",
            )
            .bind(project.as_str())
            .bind(cursor_at)
            .bind(cursor_at)
            .bind(cursor_id)
            .bind(i64::from(bound) + 1)
            .fetch_all(&pool)
            .await?
        };
        let truncated = rows.len() > bound as usize;
        let mut work_orders = Vec::with_capacity(rows.len().min(bound as usize));
        for row in rows.into_iter().take(bound as usize) {
            let mut work_order = row_to_work_order(row)?;
            work_order.sequence_lane_id = self.lane_for_work_order(&pool, &work_order.id).await?;
            work_orders.push(work_order);
        }
        let next_cursor = if truncated {
            work_orders
                .last()
                .map(|last| format!("{}:{}", last.updated_at_ms, last.id.as_str()))
        } else {
            None
        };
        Ok(WorkOrderListPage {
            work_orders,
            next_cursor,
            truncated,
        })
    }

    // ── Mutate ───────────────────────────────────────────────────────

    /// Update one waiting work order under CAS revision protection.
    /// Execution-shaping fields are rejected once any occurrence that
    /// depends on those values has been claimed.
    pub async fn update_work_order(
        &self,
        project: &ProjectId,
        id: &WorkOrderId,
        patch: WorkOrderPatch,
        expected_revision: u64,
        now_ms: i64,
    ) -> Result<WorkOrder, WorkOrderError> {
        let pool = self.durable_pool()?;
        if patch.is_empty() {
            return Err(WorkOrderError::invalid(
                "patch",
                "update carries no changes",
            ));
        }
        let Some(current) = self.get_work_order(project, id).await? else {
            return Err(WorkOrderError::NotFound(id.as_str().to_owned()));
        };
        if !matches!(
            current.state,
            WorkOrderState::Active | WorkOrderState::Paused
        ) {
            return Err(WorkOrderError::StateConflict(format!(
                "work order {} is {} and cannot be edited",
                id.as_str(),
                current.state.as_str()
            )));
        }

        let touches_execution_fields = patch.prompt.is_some()
            || patch.requested_model.is_some()
            || patch.gates.is_some()
            || patch.repeat_count.is_some();
        if touches_execution_fields && self.has_claimed_occurrences(&pool, id).await? {
            return Err(WorkOrderError::StateConflict(format!(
                "work order {} has claimed occurrences; prompt/model/gates/repeat are immutable",
                id.as_str()
            )));
        }

        let title = match patch.title {
            None => current.title.clone(),
            Some(None) => None,
            Some(Some(raw)) if raw.trim().is_empty() => None,
            Some(Some(raw)) => validate_title(Some(raw.as_str()))?,
        };
        let prompt = match patch.prompt {
            None => current.prompt.clone(),
            Some(raw) => validate_prompt(&raw)?,
        };
        let requested_model = match patch.requested_model {
            None => current.requested_model.clone(),
            Some(None) => None,
            Some(Some(raw)) => validate_model(Some(raw.as_str()))?,
        };
        let requested_approval = match patch.requested_approval {
            None => current.requested_approval,
            Some(value) => value,
        };
        let requested_sandbox = match patch.requested_sandbox {
            None => current.requested_sandbox,
            Some(value) => value,
        };
        let workspace_policy = match patch.workspace_policy {
            None => current.workspace_policy,
            Some(value) => value,
        };
        let gates = match patch.gates {
            None => current.gates.clone(),
            Some(gates) => gates,
        };
        let repeat_count = match patch.repeat_count {
            None => current.repeat_count,
            Some(count) => validate_repeat_count(Some(count))?,
        };
        let gates_json = encode_gate_set(&gates)?;
        let digest = work_order_spec_digest(
            &title,
            &prompt,
            &requested_model,
            &requested_approval,
            &requested_sandbox,
            &workspace_policy,
            &gates_json,
            &gates.join,
            repeat_count,
            current.sequence_lane_id.as_ref(),
            current.parent_session_id.as_deref(),
            current.parent_turn_id.as_deref(),
            current.parent_work_order_id.as_ref(),
        );

        let affected = sqlx::query(
            "UPDATE work_order SET revision = revision + 1, title = ?, prompt = ?, requested_model = ?, \
             requested_approval = ?, requested_sandbox = ?, workspace_policy = ?, gates_json = ?, \
             gate_join = ?, repeat_count = ?, spec_digest = ?, updated_at = ? \
             WHERE id = ? AND project_id = ? AND revision = ?",
        )
        .bind(title)
        .bind(prompt)
        .bind(requested_model)
        .bind(requested_approval.map(|mode| mode.as_str().to_owned()))
        .bind(requested_sandbox.map(|profile| profile.as_str().to_owned()))
        .bind(workspace_policy.map(|policy| policy.as_str().to_owned()))
        .bind(gates_json)
        .bind(gates.join.as_str())
        .bind(i64::from(repeat_count))
        .bind(digest)
        .bind(now_ms)
        .bind(id.as_str())
        .bind(project.as_str())
        .bind(i64::try_from(expected_revision).unwrap_or(i64::MAX))
        .execute(&pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(self
                .revision_or_not_found(&pool, project, id, expected_revision)
                .await?);
        }
        let Some(updated) = self.get_work_order(project, id).await? else {
            return Err(WorkOrderError::NotFound(id.as_str().to_owned()));
        };
        Ok(updated)
    }

    /// Move one work order along its template lifecycle (`cancel`,
    /// `pause`, `resume`). Terminal transitions are one-way per
    /// [`can_transition_work_order`]; repeating the current state is an
    /// idempotent success.
    pub async fn transition_work_order(
        &self,
        project: &ProjectId,
        id: &WorkOrderId,
        target: WorkOrderState,
        now_ms: i64,
    ) -> Result<WorkOrder, WorkOrderError> {
        let pool = self.durable_pool()?;
        let Some(current) = self.get_work_order(project, id).await? else {
            return Err(WorkOrderError::NotFound(id.as_str().to_owned()));
        };
        if current.state == target {
            return Ok(current);
        }
        if !can_transition_work_order(current.state, target) {
            return Err(WorkOrderError::StateConflict(format!(
                "work order {} cannot move from {} to {}",
                id.as_str(),
                current.state.as_str(),
                target.as_str()
            )));
        }
        let cancelled_at: Option<i64> = match target {
            WorkOrderState::Cancelled => Some(now_ms),
            _ => current.cancelled_at_ms,
        };
        sqlx::query(
            "UPDATE work_order SET revision = revision + 1, state = ?, updated_at = ?, cancelled_at = ? \
             WHERE id = ? AND project_id = ?",
        )
        .bind(target.as_str())
        .bind(now_ms)
        .bind(cancelled_at)
        .bind(id.as_str())
        .bind(project.as_str())
        .execute(&pool)
        .await?;
        let Some(updated) = self.get_work_order(project, id).await? else {
            return Err(WorkOrderError::NotFound(id.as_str().to_owned()));
        };
        Ok(updated)
    }

    // ── Occurrences ──────────────────────────────────────────────────

    /// Record one `Waiting` occurrence for a work order (explicit M001
    /// primitive; the M002 coordinator will own claim transitions).
    /// The index assigns deterministically as `MAX + 1` per work order
    /// with a `UNIQUE` backstop and one contention retry.
    pub async fn create_occurrence(
        &self,
        project: &ProjectId,
        work_order_id: &WorkOrderId,
        diagnostic: Option<&str>,
        now_ms: i64,
    ) -> Result<WorkOrderOccurrence, WorkOrderError> {
        let pool = self.durable_pool()?;
        let Some(_) = self.get_work_order(project, work_order_id).await? else {
            return Err(WorkOrderError::NotFound(work_order_id.as_str().to_owned()));
        };
        let diagnostic = validate_diagnostic(diagnostic)?;
        let id = WorkOrderOccurrenceId::new();
        let mut attempts = 0;
        loop {
            attempts += 1;
            let next_index: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(occurrence_index), -1) + 1 FROM work_order_occurrence WHERE work_order_id = ?",
            )
            .bind(work_order_id.as_str())
            .fetch_one(&pool)
            .await?;
            let insert = sqlx::query(
                "INSERT INTO work_order_occurrence (id, work_order_id, project_id, occurrence_index, state, \
                 gate_latches_json, diagnostic, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, 'waiting', '[]', ?, ?, ?)",
            )
            .bind(id.as_str())
            .bind(work_order_id.as_str())
            .bind(project.as_str())
            .bind(next_index)
            .bind(diagnostic.clone())
            .bind(now_ms)
            .bind(now_ms)
            .execute(&pool)
            .await;
            match insert {
                Ok(_) => break,
                Err(error)
                    if attempts < 10
                        && error
                            .as_database_error()
                            .is_some_and(|db| db.is_unique_violation()) =>
                {
                    continue;
                }
                Err(error) => return Err(map_unique_violation(error)),
            }
        }
        let Some(occurrence) = self.get_occurrence(project, &id).await? else {
            return Err(WorkOrderError::Storage(StorageError::Database(
                "occurrence row lost".to_owned(),
            )));
        };
        Ok(occurrence)
    }

    /// Fetch one occurrence record in project scope.
    pub async fn get_occurrence(
        &self,
        project: &ProjectId,
        id: &WorkOrderOccurrenceId,
    ) -> Result<Option<WorkOrderOccurrence>, WorkOrderError> {
        let pool = self.durable_pool()?;
        let row: Option<OccurrenceRow> =
            sqlx::query_as("SELECT * FROM work_order_occurrence WHERE id = ? AND project_id = ?")
                .bind(id.as_str())
                .bind(project.as_str())
                .fetch_optional(&pool)
                .await?;
        row.map(row_to_occurrence).transpose()
    }

    /// Bounded occurrence listing for one work order, ascending by
    /// explicit occurrence index.
    pub async fn list_occurrences(
        &self,
        project: &ProjectId,
        work_order_id: &WorkOrderId,
        limit: Option<u32>,
    ) -> Result<OccurrenceListPage, WorkOrderError> {
        let pool = self.durable_pool()?;
        let Some(_) = self.get_work_order(project, work_order_id).await? else {
            return Err(WorkOrderError::NotFound(work_order_id.as_str().to_owned()));
        };
        let bound = self.config.clamp_list_limit(limit);
        let rows: Vec<OccurrenceRow> = sqlx::query_as(
            "SELECT * FROM work_order_occurrence WHERE work_order_id = ? AND project_id = ? \
             ORDER BY occurrence_index ASC LIMIT ?",
        )
        .bind(work_order_id.as_str())
        .bind(project.as_str())
        .bind(i64::from(bound) + 1)
        .fetch_all(&pool)
        .await?;
        let truncated = rows.len() > bound as usize;
        let mut occurrences = Vec::with_capacity(rows.len().min(bound as usize));
        for row in rows.into_iter().take(bound as usize) {
            occurrences.push(row_to_occurrence(row)?);
        }
        Ok(OccurrenceListPage {
            occurrences,
            truncated,
        })
    }

    // ── Lanes ────────────────────────────────────────────────────────

    /// Create one revisioned sequence lane for a project.
    pub async fn create_lane(
        &self,
        project: &ProjectId,
        input: NewSequenceLane,
        now_ms: i64,
    ) -> Result<SequenceLane, WorkOrderError> {
        let pool = self.durable_pool()?;
        let label = validate_lane_label(input.label.as_deref())?;
        let key = validate_idempotency_key(input.idempotency_key.as_deref())?;
        if self.lane_count(&pool, project).await? >= self.config.max_lanes_per_project {
            return Err(WorkOrderError::Capacity(
                "project sequence-lane budget is exhausted".to_owned(),
            ));
        }
        if let Some(ref key) = key {
            if let Some(existing) = self.lane_by_submission_key(&pool, project, key).await? {
                return Ok(existing);
            }
        }
        let id = SequenceLaneId::new();
        sqlx::query(
            "INSERT INTO sequence_lane (id, project_id, revision, label, failure_policy, submission_key, created_at, updated_at) \
             VALUES (?, ?, 1, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(project.as_str())
        .bind(label)
        .bind(input.failure_policy.as_str())
        .bind(key)
        .bind(now_ms)
        .bind(now_ms)
        .execute(&pool)
        .await
        .map_err(map_unique_violation)?;
        let Some(lane) = self.get_lane(project, &id).await? else {
            return Err(WorkOrderError::Storage(StorageError::Database(
                "lane row lost".to_owned(),
            )));
        };
        Ok(lane)
    }

    /// Fetch one lane with its deterministic member order.
    pub async fn get_lane(
        &self,
        project: &ProjectId,
        id: &SequenceLaneId,
    ) -> Result<Option<SequenceLane>, WorkOrderError> {
        let pool = self.durable_pool()?;
        let row: Option<LaneRow> =
            sqlx::query_as("SELECT * FROM sequence_lane WHERE id = ? AND project_id = ?")
                .bind(id.as_str())
                .bind(project.as_str())
                .fetch_optional(&pool)
                .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        self.hydrate_lane(&pool, row).await.map(Some)
    }

    /// Bounded lane listing for one project, oldest first.
    pub async fn list_lanes(
        &self,
        project: &ProjectId,
        limit: Option<usize>,
    ) -> Result<(Vec<SequenceLane>, bool), WorkOrderError> {
        let pool = self.durable_pool()?;
        let bound = limit
            .unwrap_or(self.config.max_lanes_per_project)
            .max(1)
            .min(self.config.max_lanes_per_project.max(1));
        let rows: Vec<LaneRow> = sqlx::query_as(
            "SELECT * FROM sequence_lane WHERE project_id = ? ORDER BY created_at ASC, id ASC LIMIT ?",
        )
        .bind(project.as_str())
        .bind((bound as i64) + 1)
        .fetch_all(&pool)
        .await?;
        let truncated = rows.len() > bound;
        let mut lanes = Vec::with_capacity(rows.len().min(bound));
        for row in rows.into_iter().take(bound) {
            lanes.push(self.hydrate_lane(&pool, row).await?);
        }
        Ok((lanes, truncated))
    }

    /// Replace one lane's member order under CAS revision protection.
    /// `ordered_ids` must name every current member exactly once:
    /// concurrent reorder writers serialize on the lane revision instead
    /// of silently losing one client's change.
    pub async fn reorder_lane(
        &self,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
        expected_revision: u64,
        ordered_ids: Vec<WorkOrderId>,
        now_ms: i64,
    ) -> Result<SequenceLane, WorkOrderError> {
        let pool = self.durable_pool()?;
        if ordered_ids.len() > self.config.max_lane_members {
            return Err(WorkOrderError::invalid(
                "ordered_work_order_ids",
                format!(
                    "lane holds at most {} members",
                    self.config.max_lane_members
                ),
            ));
        }
        let Some(current) = self.get_lane(project, lane_id).await? else {
            return Err(WorkOrderError::NotFound(lane_id.as_str().to_owned()));
        };
        if current.revision != expected_revision {
            return Err(WorkOrderError::RevisionConflict {
                id: lane_id.as_str().to_owned(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        let mut current_sorted: Vec<&str> = current
            .ordered_work_order_ids
            .iter()
            .map(WorkOrderId::as_str)
            .collect();
        current_sorted.sort_unstable();
        let mut next_sorted: Vec<&str> = ordered_ids.iter().map(WorkOrderId::as_str).collect();
        next_sorted.sort_unstable();
        if current_sorted != next_sorted {
            return Err(WorkOrderError::invalid(
                "ordered_work_order_ids",
                "reorder must name every current lane member exactly once",
            ));
        }
        self.write_lane_order(
            &pool,
            project,
            lane_id,
            expected_revision,
            ordered_ids,
            now_ms,
        )
        .await
    }

    /// Shared lane-order writer: project/pinned validation plus the
    /// atomic delete + re-insert transaction. Callers own membership
    /// preconditions (exact-set for reorder, append/insert for attach).
    async fn write_lane_order(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
        expected_revision: u64,
        ordered_ids: Vec<WorkOrderId>,
        now_ms: i64,
    ) -> Result<SequenceLane, WorkOrderError> {
        for id in &ordered_ids {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT project_id FROM work_order WHERE id = ?")
                    .bind(id.as_str())
                    .fetch_optional(pool)
                    .await?;
            let Some((stored_project,)) = row else {
                return Err(WorkOrderError::NotFound(id.as_str().to_owned()));
            };
            if stored_project != project.as_str() {
                return Err(WorkOrderError::ProjectMismatch(id.as_str().to_owned()));
            }
            if self.has_claimed_occurrences(pool, id).await? {
                return Err(WorkOrderError::StateConflict(format!(
                    "work order {} is claimed and pinned in lane order",
                    id.as_str()
                )));
            }
        }

        // Delete + re-insert inside one transaction: no transient
        // UNIQUE violation, no partial position update on failure.
        let mut tx = pool.begin().await?;
        let affected = sqlx::query(
            "UPDATE sequence_lane SET revision = revision + 1, updated_at = ? \
             WHERE id = ? AND project_id = ? AND revision = ?",
        )
        .bind(now_ms)
        .bind(lane_id.as_str())
        .bind(project.as_str())
        .bind(i64::try_from(expected_revision).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected == 0 {
            // Roll back before re-reading: the re-read needs a pool
            // connection of its own, and holding this transaction open
            // across that acquire self-deadlocks small pools.
            tx.rollback().await?;
            return Err(WorkOrderError::RevisionConflict {
                id: lane_id.as_str().to_owned(),
                expected: expected_revision,
                current: self
                    .get_lane(project, lane_id)
                    .await?
                    .map(|lane| lane.revision)
                    .unwrap_or(expected_revision),
            });
        }
        sqlx::query("DELETE FROM sequence_lane_member WHERE lane_id = ?")
            .bind(lane_id.as_str())
            .execute(&mut *tx)
            .await?;
        for (position, id) in ordered_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO sequence_lane_member (lane_id, work_order_id, position) VALUES (?, ?, ?)",
            )
            .bind(lane_id.as_str())
            .bind(id.as_str())
            .bind(position as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        let Some(lane) = self.get_lane(project, lane_id).await? else {
            return Err(WorkOrderError::NotFound(lane_id.as_str().to_owned()));
        };
        Ok(lane)
    }

    /// Atomically move one waiting work order before or after another
    /// member of the same lane.
    pub async fn move_work_order(
        &self,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
        work_order_id: &WorkOrderId,
        before: Option<&WorkOrderId>,
        after: Option<&WorkOrderId>,
        now_ms: i64,
    ) -> Result<SequenceLane, WorkOrderError> {
        if before.is_some() == after.is_some() {
            return Err(WorkOrderError::invalid(
                "move",
                "move names exactly one anchor: before or after",
            ));
        }
        let Some(current) = self.get_lane(project, lane_id).await? else {
            return Err(WorkOrderError::NotFound(lane_id.as_str().to_owned()));
        };
        if !current.ordered_work_order_ids.contains(work_order_id) {
            return Err(WorkOrderError::NotFound(work_order_id.as_str().to_owned()));
        }
        let anchor = before.or(after).expect("exactly one anchor");
        if anchor == work_order_id {
            return Err(WorkOrderError::invalid(
                "move",
                "move anchor must differ from the moved work order",
            ));
        }
        if !current.ordered_work_order_ids.contains(anchor) {
            return Err(WorkOrderError::NotFound(anchor.as_str().to_owned()));
        }
        let mut order: Vec<WorkOrderId> = current
            .ordered_work_order_ids
            .iter()
            .filter(|id| *id != work_order_id)
            .cloned()
            .collect();
        let anchor_pos = order
            .iter()
            .position(|id| id == anchor)
            .expect("anchor is a member");
        let insert_at = if before.is_some() {
            anchor_pos
        } else {
            anchor_pos + 1
        };
        order.insert(insert_at, work_order_id.clone());
        self.reorder_lane(project, lane_id, current.revision, order, now_ms)
            .await
    }

    /// Atomically attach one existing waiting work order to a lane,
    /// appended or inserted at `position`. The lane revision guards the
    /// read-modify-write so concurrent attach/reorder writers serialize.
    pub async fn attach_to_lane(
        &self,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
        expected_revision: u64,
        work_order_id: &WorkOrderId,
        position: Option<u64>,
        now_ms: i64,
    ) -> Result<SequenceLane, WorkOrderError> {
        let pool = self.durable_pool()?;
        let Some(current) = self.get_lane(project, lane_id).await? else {
            return Err(WorkOrderError::NotFound(lane_id.as_str().to_owned()));
        };
        if current.revision != expected_revision {
            return Err(WorkOrderError::RevisionConflict {
                id: lane_id.as_str().to_owned(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        if current.ordered_work_order_ids.contains(work_order_id) {
            return Err(WorkOrderError::invalid(
                "work_order_id",
                "work order is already a lane member",
            ));
        }
        let Some(work_order) = self.get_work_order(project, work_order_id).await? else {
            return Err(WorkOrderError::NotFound(work_order_id.as_str().to_owned()));
        };
        if !matches!(
            work_order.state,
            WorkOrderState::Active | WorkOrderState::Paused
        ) {
            return Err(WorkOrderError::StateConflict(format!(
                "work order {} is {} and cannot join a lane",
                work_order_id.as_str(),
                work_order.state.as_str()
            )));
        }
        if self.has_claimed_occurrences(&pool, work_order_id).await? {
            return Err(WorkOrderError::StateConflict(format!(
                "work order {} is claimed and cannot join a lane",
                work_order_id.as_str()
            )));
        }
        if current.ordered_work_order_ids.len() >= self.config.max_lane_members {
            return Err(WorkOrderError::Capacity(
                "sequence lane member budget is exhausted".to_owned(),
            ));
        }
        let mut order = current.ordered_work_order_ids.clone();
        match position {
            None => order.push(work_order_id.clone()),
            Some(pos) => {
                let at = usize::try_from(pos).unwrap_or(usize::MAX).min(order.len());
                order.insert(at, work_order_id.clone());
            }
        }
        self.write_lane_order(&pool, project, lane_id, current.revision, order, now_ms)
            .await
    }

    /// Bounded per-project summary counts for later projections.
    pub async fn summary_counts(
        &self,
        project: &ProjectId,
    ) -> Result<WorkOrderSummary, WorkOrderError> {
        let pool = self.durable_pool()?;
        let counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT state, COUNT(*) FROM work_order WHERE project_id = ? GROUP BY state",
        )
        .bind(project.as_str())
        .fetch_all(&pool)
        .await?;
        let mut summary = WorkOrderSummary {
            project_id: project.clone(),
            active: 0,
            paused: 0,
            completed: 0,
            cancelled: 0,
            archived: 0,
            waiting_occurrences: 0,
            attention_occurrences: 0,
            lane_count: 0,
        };
        for (state, count) in counts {
            let count = u64::try_from(count).unwrap_or(0);
            match state.as_str() {
                "active" => summary.active = count,
                "paused" => summary.paused = count,
                "completed" => summary.completed = count,
                "cancelled" => summary.cancelled = count,
                "archived" => summary.archived = count,
                _ => {}
            }
        }
        let waiting: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM work_order_occurrence WHERE project_id = ? AND state IN ('waiting','ready')",
        )
        .bind(project.as_str())
        .fetch_one(&pool)
        .await?;
        let attention: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM work_order_occurrence WHERE project_id = ? AND state = 'needs_attention'",
        )
        .bind(project.as_str())
        .fetch_one(&pool)
        .await?;
        let lanes: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sequence_lane WHERE project_id = ?")
                .bind(project.as_str())
                .fetch_one(&pool)
                .await?;
        summary.waiting_occurrences = u64::try_from(waiting.0).unwrap_or(0);
        summary.attention_occurrences = u64::try_from(attention.0).unwrap_or(0);
        summary.lane_count = u64::try_from(lanes.0).unwrap_or(0);
        Ok(summary)
    }

    // ── Internals ────────────────────────────────────────────────────

    async fn validate_new_input(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        input: &NewWorkOrder,
    ) -> Result<CleanedWorkOrder, WorkOrderError> {
        let title = validate_title(input.title.as_deref())?;
        let prompt = validate_prompt(&input.prompt)?;
        let requested_model = validate_model(input.requested_model.as_deref())?;
        let gates = validate_gate_set(&input.gates.gates, input.gates.join)?;
        let repeat_count = validate_repeat_count(Some(input.repeat_count))?;
        let parent_session_id = validate_parent_ref(input.parent_session_id.as_deref())?;
        let parent_turn_id = validate_parent_ref(input.parent_turn_id.as_deref())?;
        let submission_key = validate_idempotency_key(input.idempotency_key.as_deref())?;

        // Cross-project references fail closed: lanes, parents, and
        // sequence gates must resolve inside the same project.
        if let Some(ref lane_id) = input.sequence_lane_id {
            self.require_lane_in_project(pool, project, lane_id).await?;
        }
        for gate in &gates.gates {
            if gate.kind == GateKind::SequenceReady {
                let lane_id = gate.lane_id.as_ref().ok_or_else(|| {
                    WorkOrderError::invalid("gates", "sequence_ready gate requires lane_id")
                })?;
                self.require_lane_in_project(pool, project, lane_id).await?;
            }
        }
        let parent_work_order_id = if let Some(ref parent_id) = input.parent_work_order_id {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT project_id FROM work_order WHERE id = ?")
                    .bind(parent_id.as_str())
                    .fetch_optional(pool)
                    .await?;
            match row {
                Some((stored_project,)) if stored_project == project.as_str() => {
                    Some(parent_id.clone())
                }
                _ => {
                    return Err(WorkOrderError::ProjectMismatch(format!(
                        "parent work order {} is not in this project",
                        parent_id.as_str()
                    )));
                }
            }
        } else {
            None
        };

        let gates_json = encode_gate_set(&gates)?;
        if gates_json.len() > super::model::MAX_GATE_SPEC_BYTES {
            return Err(WorkOrderError::invalid(
                "gates",
                "release-gate description exceeds the durable bound",
            ));
        }
        let requested_approval = input.requested_approval;
        let requested_sandbox = input.requested_sandbox;
        let workspace_policy = input.workspace_policy;
        let digest = work_order_spec_digest(
            &title,
            &prompt,
            &requested_model,
            &requested_approval,
            &requested_sandbox,
            &workspace_policy,
            &gates_json,
            &gates.join,
            repeat_count,
            input.sequence_lane_id.as_ref(),
            parent_session_id.as_deref(),
            parent_turn_id.as_deref(),
            parent_work_order_id.as_ref(),
        );
        Ok(CleanedWorkOrder {
            title,
            prompt,
            requested_model,
            requested_approval,
            requested_sandbox,
            workspace_policy,
            gates_json,
            gate_join: gates.join.as_str().to_owned(),
            repeat_count,
            sequence_lane_id: input.sequence_lane_id.clone(),
            parent_session_id,
            parent_turn_id,
            parent_work_order_id: parent_work_order_id.map(|id| id.as_str().to_owned()),
            submission_key,
            digest,
        })
    }

    async fn require_lane_in_project(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
    ) -> Result<(), WorkOrderError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT project_id FROM sequence_lane WHERE id = ?")
                .bind(lane_id.as_str())
                .fetch_optional(pool)
                .await?;
        match row {
            Some((stored_project,)) if stored_project == project.as_str() => Ok(()),
            _ => Err(WorkOrderError::ProjectMismatch(format!(
                "sequence lane {} is not in this project",
                lane_id.as_str()
            ))),
        }
    }

    async fn by_submission_key(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        key: &str,
    ) -> Result<Option<StoredHead>, WorkOrderError> {
        let row: Option<WorkOrderRow> =
            sqlx::query_as("SELECT * FROM work_order WHERE project_id = ? AND submission_key = ?")
                .bind(project.as_str())
                .bind(key)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|row| StoredHead { row }))
    }

    async fn hydrate(
        &self,
        pool: &SqlitePool,
        row: WorkOrderRow,
    ) -> Result<WorkOrder, WorkOrderError> {
        let id = WorkOrderId::parse(&row.id)
            .map_err(|_| WorkOrderError::invalid("work_order_id", "stored row failed to parse"))?;
        let mut work_order = row_to_work_order(row)?;
        work_order.sequence_lane_id = self.lane_for_work_order(pool, &id).await?;
        Ok(work_order)
    }

    async fn lane_for_work_order(
        &self,
        pool: &SqlitePool,
        id: &WorkOrderId,
    ) -> Result<Option<SequenceLaneId>, WorkOrderError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT lane_id FROM sequence_lane_member WHERE work_order_id = ?")
                .bind(id.as_str())
                .fetch_optional(pool)
                .await?;
        row.map(|(raw,)| {
            SequenceLaneId::parse(&raw).map_err(|_| {
                WorkOrderError::invalid("sequence_lane_id", "stored row failed to parse")
            })
        })
        .transpose()
    }

    async fn append_to_lane(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        lane_id: &SequenceLaneId,
        work_order_id: &WorkOrderId,
        now_ms: i64,
    ) -> Result<(), WorkOrderError> {
        self.require_lane_in_project(pool, project, lane_id).await?;
        if self.lane_member_count(pool, lane_id).await? >= self.config.max_lane_members {
            return Err(WorkOrderError::Capacity(
                "sequence lane member budget is exhausted".to_owned(),
            ));
        }
        let base: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), -1) FROM sequence_lane_member WHERE lane_id = ?",
        )
        .bind(lane_id.as_str())
        .fetch_one(pool)
        .await?;
        sqlx::query(
            "INSERT INTO sequence_lane_member (lane_id, work_order_id, position) VALUES (?, ?, ?)",
        )
        .bind(lane_id.as_str())
        .bind(work_order_id.as_str())
        .bind(base.unwrap_or(-1) + 1)
        .execute(pool)
        .await
        .map_err(map_unique_violation)?;
        let mut tx = pool.begin().await?;
        self.bump_lane_revision_tx(&mut tx, lane_id, now_ms).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn bump_lane_revision_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        lane_id: &SequenceLaneId,
        now_ms: i64,
    ) -> Result<(), WorkOrderError> {
        sqlx::query(
            "UPDATE sequence_lane SET revision = revision + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now_ms)
        .bind(lane_id.as_str())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn hydrate_lane(
        &self,
        pool: &SqlitePool,
        row: LaneRow,
    ) -> Result<SequenceLane, WorkOrderError> {
        let bad =
            |field: &'static str| WorkOrderError::invalid(field, "stored row failed to parse");
        let id = SequenceLaneId::parse(&row.id).map_err(|_| bad("sequence_lane_id"))?;
        let member_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT work_order_id FROM sequence_lane_member WHERE lane_id = ? ORDER BY position ASC",
        )
        .bind(id.as_str())
        .fetch_all(pool)
        .await?;
        let mut ordered_work_order_ids = Vec::with_capacity(member_rows.len());
        for (raw,) in member_rows {
            ordered_work_order_ids
                .push(WorkOrderId::parse(&raw).map_err(|_| bad("work_order_id"))?);
        }
        Ok(SequenceLane {
            id,
            project_id: ProjectId::parse(&row.project_id).map_err(|_| bad("project_id"))?,
            revision: u64::try_from(row.revision).map_err(|_| bad("revision"))?,
            label: row.label,
            failure_policy: LaneFailurePolicy::parse(&row.failure_policy)
                .ok_or_else(|| bad("failure_policy"))?,
            ordered_work_order_ids,
            created_at_ms: row.created_at,
            updated_at_ms: row.updated_at,
        })
    }

    async fn lane_count(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
    ) -> Result<usize, WorkOrderError> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sequence_lane WHERE project_id = ?")
                .bind(project.as_str())
                .fetch_one(pool)
                .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }

    async fn lane_member_count(
        &self,
        pool: &SqlitePool,
        lane_id: &SequenceLaneId,
    ) -> Result<usize, WorkOrderError> {
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sequence_lane_member WHERE lane_id = ?")
                .bind(lane_id.as_str())
                .fetch_one(pool)
                .await?;
        Ok(usize::try_from(count.0).unwrap_or(0))
    }

    async fn lane_by_submission_key(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        key: &str,
    ) -> Result<Option<SequenceLane>, WorkOrderError> {
        let row: Option<LaneRow> = sqlx::query_as(
            "SELECT * FROM sequence_lane WHERE project_id = ? AND submission_key = ?",
        )
        .bind(project.as_str())
        .bind(key)
        .fetch_optional(pool)
        .await?;
        match row {
            None => Ok(None),
            Some(row) => self.hydrate_lane(pool, row).await.map(Some),
        }
    }

    async fn has_claimed_occurrences(
        &self,
        pool: &SqlitePool,
        id: &WorkOrderId,
    ) -> Result<bool, WorkOrderError> {
        let states = [
            "claiming",
            "running",
            "needs_attention",
            "completed",
            "failed",
        ];
        for state in states {
            let count: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM work_order_occurrence WHERE work_order_id = ? AND state = ?",
            )
            .bind(id.as_str())
            .bind(state)
            .fetch_one(pool)
            .await?;
            if count.0 > 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn revision_or_not_found(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        id: &WorkOrderId,
        expected_revision: u64,
    ) -> Result<WorkOrderError, WorkOrderError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT revision FROM work_order WHERE id = ? AND project_id = ?")
                .bind(id.as_str())
                .bind(project.as_str())
                .fetch_optional(pool)
                .await?;
        match row {
            None => Ok(WorkOrderError::NotFound(id.as_str().to_owned())),
            Some((current,)) => Ok(WorkOrderError::RevisionConflict {
                id: id.as_str().to_owned(),
                expected: expected_revision,
                current: u64::try_from(current).unwrap_or(0),
            }),
        }
    }

    async fn batch_by_key(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        key: &str,
    ) -> Result<Option<String>, WorkOrderError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT spec_digest FROM work_order_batch WHERE project_id = ? AND batch_key = ?",
        )
        .bind(project.as_str())
        .bind(key)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(digest,)| digest))
    }

    async fn batch_member_ids(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
        key: &str,
    ) -> Result<Vec<WorkOrderId>, WorkOrderError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT work_order_ids_json FROM work_order_batch WHERE project_id = ? AND batch_key = ?",
        )
        .bind(project.as_str())
        .bind(key)
        .fetch_optional(pool)
        .await?;
        let Some((json,)) = row else {
            return Ok(Vec::new());
        };
        let raw: Vec<String> = serde_json::from_str(&json).map_err(|_| {
            WorkOrderError::invalid("batch_key", "stored batch row failed to parse")
        })?;
        raw.into_iter()
            .map(|id| {
                WorkOrderId::parse(&id).map_err(|_| {
                    WorkOrderError::invalid("batch_key", "stored batch row failed to parse")
                })
            })
            .collect()
    }
}

// ── Spec digests ─────────────────────────────────────────────────────────

/// Validated and normalized work-order creation input plus its durable
/// spec digest (used for idempotency mismatch detection).
struct CleanedWorkOrder {
    title: Option<String>,
    prompt: String,
    requested_model: Option<String>,
    requested_approval: Option<ApprovalRequest>,
    requested_sandbox: Option<SandboxRequest>,
    workspace_policy: Option<WorkspacePolicy>,
    gates_json: String,
    gate_join: String,
    repeat_count: u32,
    sequence_lane_id: Option<SequenceLaneId>,
    parent_session_id: Option<String>,
    parent_turn_id: Option<String>,
    parent_work_order_id: Option<String>,
    submission_key: Option<String>,
    digest: String,
}

struct StoredHead {
    row: WorkOrderRow,
}

impl StoredHead {
    fn spec_digest_mismatch(&self, digest: &str) -> bool {
        self.row.spec_digest != digest
    }
}

#[allow(clippy::too_many_arguments)]
fn work_order_spec_digest(
    title: &Option<String>,
    prompt: &str,
    requested_model: &Option<String>,
    requested_approval: &Option<ApprovalRequest>,
    requested_sandbox: &Option<SandboxRequest>,
    workspace_policy: &Option<WorkspacePolicy>,
    gates_json: &str,
    gate_join: &GateJoin,
    repeat_count: u32,
    lane_id: Option<&SequenceLaneId>,
    parent_session_id: Option<&str>,
    parent_turn_id: Option<&str>,
    parent_work_order_id: Option<&WorkOrderId>,
) -> String {
    let fields = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        title.as_deref().unwrap_or(""),
        prompt,
        requested_model.as_deref().unwrap_or(""),
        requested_approval
            .map(ApprovalRequest::as_str)
            .unwrap_or(""),
        requested_sandbox.map(SandboxRequest::as_str).unwrap_or(""),
        workspace_policy.map(WorkspacePolicy::as_str).unwrap_or(""),
        gates_json,
        gate_join.as_str(),
        repeat_count,
        lane_id.map(SequenceLaneId::as_str).unwrap_or(""),
        parent_session_id.unwrap_or(""),
        parent_turn_id.unwrap_or(""),
        parent_work_order_id.map(WorkOrderId::as_str).unwrap_or(""),
    );
    format!("{:x}", Sha256::digest(fields.as_bytes()))
}

fn batch_spec_digest(cleaned: &[CleanedWorkOrder]) -> String {
    let fields = cleaned
        .iter()
        .map(|item| item.digest.clone())
        .collect::<Vec<_>>()
        .join("|");
    format!("{:x}", Sha256::digest(fields.as_bytes()))
}

fn parse_cursor(cursor: Option<&str>) -> Result<(i64, String), WorkOrderError> {
    let Some(cursor) = cursor else {
        return Ok((i64::MAX, "\u{10ffff}".to_owned()));
    };
    let (at_raw, id) = cursor
        .split_once(':')
        .ok_or_else(|| WorkOrderError::invalid("cursor", "work order cursor is malformed"))?;
    let at: i64 = at_raw
        .parse()
        .map_err(|_| WorkOrderError::invalid("cursor", "work order cursor is malformed"))?;
    if id.is_empty() || id.len() > MAX_IDEMPOTENCY_KEY_LEN {
        return Err(WorkOrderError::invalid(
            "cursor",
            "work order cursor is malformed",
        ));
    }
    Ok((at, id.to_owned()))
}

fn map_unique_violation(error: sqlx::Error) -> WorkOrderError {
    if error
        .as_database_error()
        .is_some_and(|db| db.is_unique_violation())
    {
        return WorkOrderError::IdempotencyConflict("concurrent submission".to_owned());
    }
    WorkOrderError::from(error)
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        GateJoin, GateKind, GateSpec, LaneFailurePolicy, NewSequenceLane, NewWorkOrder,
        OccurrenceState, ReleaseGateSet, WorkOrderPatch, WorkOrderState,
    };
    use super::*;

    async fn temp_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    fn project() -> ProjectId {
        ProjectId::parse("project-1").unwrap()
    }

    fn creator() -> PrincipalId {
        PrincipalId::parse("local-owner").unwrap()
    }

    fn input(prompt: &str) -> NewWorkOrder {
        NewWorkOrder {
            title: Some("Test work".to_owned()),
            prompt: prompt.to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: ReleaseGateSet::immediate(),
            repeat_count: 1,
            sequence_lane_id: None,
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            idempotency_key: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_get_list_round_trip_without_session_rows() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool.clone()));
        let outcome = service
            .create_work_order(&project(), &creator(), input("Do the thing"), 1_000)
            .await
            .expect("create");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.work_order.revision, 1);
        assert_eq!(outcome.work_order.state, WorkOrderState::Active);

        // Waiting work orders exist without session rows.
        let sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM session")
            .fetch_one(&pool)
            .await
            .expect("count sessions");
        assert_eq!(sessions.0, 0);

        let fetched = service
            .get_work_order(&project(), &outcome.work_order.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(fetched, outcome.work_order);

        let page = service
            .list_work_orders(&project(), None, None, None)
            .await
            .expect("list");
        assert_eq!(page.work_orders.len(), 1);
        assert!(!page.truncated);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_submission_key_converges_or_conflicts() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let mut first = input("Same work");
        first.idempotency_key = Some("key-1".to_owned());
        let one = service
            .create_work_order(&project(), &creator(), first, 1_000)
            .await
            .expect("create");
        assert!(!one.duplicate);

        // Same payload retries converge.
        let mut retry = input("Same work");
        retry.idempotency_key = Some("key-1".to_owned());
        let two = service
            .create_work_order(&project(), &creator(), retry, 2_000)
            .await
            .expect("retry");
        assert!(two.duplicate);
        assert_eq!(two.work_order.id, one.work_order.id);

        // Same key with a different payload is an explicit conflict.
        let mut clash = input("Different work");
        clash.idempotency_key = Some("key-1".to_owned());
        let error = service
            .create_work_order(&project(), &creator(), clash, 3_000)
            .await
            .expect_err("mismatch must conflict");
        assert!(matches!(error, WorkOrderError::IdempotencyConflict(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_requires_expected_revision_and_mutable_state() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let outcome = service
            .create_work_order(&project(), &creator(), input("v1"), 1_000)
            .await
            .expect("create");
        let id = outcome.work_order.id.clone();

        let patch = WorkOrderPatch {
            prompt: Some("v2".to_owned()),
            ..WorkOrderPatch::default()
        };
        let stale = service
            .update_work_order(&project(), &id, patch.clone(), 99, 2_000)
            .await
            .expect_err("stale revision");
        assert!(matches!(stale, WorkOrderError::RevisionConflict { .. }));

        let updated = service
            .update_work_order(&project(), &id, patch, 1, 2_000)
            .await
            .expect("update");
        assert_eq!(updated.revision, 2);
        assert_eq!(updated.prompt, "v2");

        // Terminal transitions are one-way.
        let cancelled = service
            .transition_work_order(&project(), &id, WorkOrderState::Cancelled, 3_000)
            .await
            .expect("cancel");
        assert_eq!(cancelled.state, WorkOrderState::Cancelled);
        let resume = service
            .transition_work_order(&project(), &id, WorkOrderState::Active, 4_000)
            .await
            .expect_err("cancelled cannot resume");
        assert!(matches!(resume, WorkOrderError::StateConflict(_)));
        let edit = service
            .update_work_order(
                &project(),
                &id,
                WorkOrderPatch {
                    prompt: Some("v3".to_owned()),
                    ..WorkOrderPatch::default()
                },
                cancelled.revision,
                4_000,
            )
            .await
            .expect_err("cancelled cannot edit");
        assert!(matches!(edit, WorkOrderError::StateConflict(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn batch_creation_is_atomic_and_ordered() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool.clone()));
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: Some("queue".to_owned()),
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                500,
            )
            .await
            .expect("lane");

        let outcome = service
            .batch_create_work_orders(
                &project(),
                &creator(),
                vec![input("one"), input("two"), input("three")],
                Some(lane.id.clone()),
                Some("batch-1".to_owned()),
                1_000,
            )
            .await
            .expect("batch");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.work_orders.len(), 3);

        let lane = service
            .get_lane(&project(), &lane.id)
            .await
            .expect("lane")
            .expect("row");
        assert_eq!(lane.revision, 2);
        let ordered: Vec<String> = lane
            .ordered_work_order_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        let created: Vec<String> = outcome
            .work_orders
            .iter()
            .map(|work| work.id.as_str().to_owned())
            .collect();
        assert_eq!(ordered, created);

        // Retry with the same batch key converges without a second commit.
        let retry = service
            .batch_create_work_orders(
                &project(),
                &creator(),
                vec![input("one"), input("two"), input("three")],
                Some(lane.id.clone()),
                Some("batch-1".to_owned()),
                2_000,
            )
            .await
            .expect("batch retry");
        assert!(retry.duplicate);
        assert_eq!(retry.work_orders.len(), 3);
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM work_order")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reorder_is_cas_guarded_and_set_exact() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                500,
            )
            .await
            .expect("lane");
        let a = service
            .create_work_order(&project(), &creator(), input("a"), 1_000)
            .await
            .expect("a");
        let b = service
            .create_work_order(&project(), &creator(), input("b"), 1_001)
            .await
            .expect("b");
        // Place both members via two atomic moves through reorder.
        let lane = service
            .reorder_lane(&project(), &lane.id, lane.revision, vec![], 1_002)
            .await
            .expect("empty reorder");
        assert_eq!(lane.revision, 2);
        // Attach members by recreating order explicitly is covered by
        // batch placement; here reorder an empty lane stays empty.
        assert!(lane.ordered_work_order_ids.is_empty());

        // Stale revision makes zero mutation.
        let stale = service
            .reorder_lane(
                &project(),
                &lane.id,
                1,
                vec![a.work_order.id.clone(), b.work_order.id.clone()],
                1_003,
            )
            .await
            .expect_err("stale lane revision");
        assert!(matches!(stale, WorkOrderError::RevisionConflict { .. }));
        let lane = service
            .get_lane(&project(), &lane.id)
            .await
            .expect("lane")
            .expect("row");
        assert_eq!(lane.revision, 2);
        assert!(lane.ordered_work_order_ids.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn occurrence_index_assigns_deterministically() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let outcome = service
            .create_work_order(&project(), &creator(), input("repeatable"), 1_000)
            .await
            .expect("create");
        let first = service
            .create_occurrence(&project(), &outcome.work_order.id, None, 1_001)
            .await
            .expect("first occurrence");
        assert_eq!(first.occurrence_index, 0);
        assert_eq!(first.state, OccurrenceState::Waiting);
        let second = service
            .create_occurrence(&project(), &outcome.work_order.id, None, 1_002)
            .await
            .expect("second occurrence");
        assert_eq!(second.occurrence_index, 1);
        let page = service
            .list_occurrences(&project(), &outcome.work_order.id, None)
            .await
            .expect("list");
        assert_eq!(page.occurrences.len(), 2);
        assert_eq!(page.occurrences[0].occurrence_index, 0);
        assert_eq!(page.occurrences[1].occurrence_index, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cross_project_references_fail_closed() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let other = ProjectId::parse("project-2").unwrap();
        let lane = service
            .create_lane(
                &other,
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                500,
            )
            .await
            .expect("other-project lane");
        let mut foreign = input("foreign lane");
        foreign.sequence_lane_id = Some(lane.id.clone());
        let error = service
            .create_work_order(&project(), &creator(), foreign, 1_000)
            .await
            .expect_err("cross-project lane");
        assert!(matches!(error, WorkOrderError::ProjectMismatch(_)));

        // Sequence gates naming a foreign lane fail the same way.
        let mut gated = input("foreign gate");
        gated.gates = ReleaseGateSet {
            join: GateJoin::All,
            gates: vec![GateSpec {
                kind: GateKind::SequenceReady,
                delay_secs: None,
                not_before_ms: None,
                lane_id: Some(lane.id.clone()),
                trigger_ref: None,
            }],
        };
        let error = service
            .create_work_order(&project(), &creator(), gated, 1_001)
            .await
            .expect_err("cross-project gate");
        assert!(matches!(error, WorkOrderError::ProjectMismatch(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn project_scope_hides_foreign_rows() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let outcome = service
            .create_work_order(&project(), &creator(), input("private"), 1_000)
            .await
            .expect("create");
        let other = ProjectId::parse("project-2").unwrap();
        assert!(service
            .get_work_order(&other, &outcome.work_order.id)
            .await
            .expect("get")
            .is_none());
        let page = service
            .list_work_orders(&other, None, None, None)
            .await
            .expect("list");
        assert!(page.work_orders.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn work_order_project_resolves_every_opaque_id() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool.clone()));
        let outcome = service
            .create_work_order(&project(), &creator(), input("resolvable"), 1_000)
            .await
            .expect("create");
        let occurrence = service
            .create_occurrence(&project(), &outcome.work_order.id, None, 1_001)
            .await
            .expect("occurrence");
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                1_002,
            )
            .await
            .expect("lane");
        for raw in [
            outcome.work_order.id.as_str(),
            occurrence.id.as_str(),
            lane.id.as_str(),
        ] {
            assert_eq!(super::work_order_project(&pool, raw).await, Some(project()));
        }
        assert_eq!(super::work_order_project(&pool, "missing").await, None);
        assert_eq!(super::work_order_project(&pool, "../escape").await, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_reorder_serializes_with_exactly_one_winner() {
        let pool = temp_pool().await;
        let service = WorkOrderService::with_defaults(Some(pool));
        let lane = service
            .create_lane(
                &project(),
                NewSequenceLane {
                    label: None,
                    failure_policy: LaneFailurePolicy::HoldLane,
                    idempotency_key: None,
                },
                1,
            )
            .await
            .expect("lane");
        let a = service
            .create_work_order(&project(), &creator(), input("a"), 2)
            .await
            .expect("a");
        let b = service
            .create_work_order(&project(), &creator(), input("b"), 3)
            .await
            .expect("b");
        let lane = service
            .attach_to_lane(&project(), &lane.id, 1, &a.work_order.id, None, 4)
            .await
            .expect("attach a");
        let lane = service
            .attach_to_lane(
                &project(),
                &lane.id,
                lane.revision,
                &b.work_order.id,
                None,
                5,
            )
            .await
            .expect("attach b");
        let revision = lane.revision;
        let lane_id = lane.id.clone();
        let project = project();

        // Both writers race the same expected revision. Exactly one
        // wins; the loser receives an explicit conflict (never a pool
        // stall: the conflict path rolls back before re-reading).
        let (first, second) = tokio::join!(
            service.reorder_lane(
                &project,
                &lane_id,
                revision,
                vec![a.work_order.id.clone(), b.work_order.id.clone()],
                6,
            ),
            service.reorder_lane(
                &project,
                &lane_id,
                revision,
                vec![b.work_order.id.clone(), a.work_order.id.clone()],
                7,
            )
        );
        let outcomes = [first.is_ok(), second.is_ok()];
        assert_eq!(
            outcomes.iter().filter(|ok| **ok).count(),
            1,
            "exactly one reorder must win"
        );
        for result in [first, second] {
            if let Err(error) = result {
                assert!(
                    matches!(error, WorkOrderError::RevisionConflict { .. }),
                    "loser must see a revision conflict, got {error:?}"
                );
            }
        }
        let lane = service
            .get_lane(&project, &lane_id)
            .await
            .expect("lane")
            .expect("row");
        assert_eq!(lane.revision, revision + 1);
        assert_eq!(lane.ordered_work_order_ids.len(), 2);
    }
}
