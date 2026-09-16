//! Durable `WorkPlan`/`WorkItem` store (long-horizon M002).
//!
//! One store/service owner with CAS semantics. Simultaneous updates return
//! an explicit [`WorkPlanError::Conflict`] rather than last-write-wins.
//! Owner run/job refs and evidence refs are provenance only: a referenced
//! run/job never grants write permission to a plan.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::model::{
    actionable_items, can_transition_item, can_transition_plan, validate_acceptance,
    validate_evidence_ref, validate_item_blocker_rule, validate_item_graph, validate_new_work_item,
    validate_new_work_plan, work_plan_origin_digest, NewWorkItem, NewWorkPlan, WorkAcceptance,
    WorkAcceptanceDisposition, WorkEvidenceKind, WorkEvidenceRef, WorkItem, WorkItemId,
    WorkItemPatch, WorkItemStatus, WorkPlan, WorkPlanError, WorkPlanId, WorkPlanStatus,
};

/// Additive M002 schema. Safe on existing databases via `IF NOT EXISTS`.
/// No unbounded plan blob: per-item dependency/acceptance/evidence JSON is
/// length-checked both in SQL and in domain validation.
pub const WORK_PLAN_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS work_plan (
        id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL CHECK (revision >= 0),
        session_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        origin_turn_id TEXT,
        goal_id TEXT,
        objective TEXT NOT NULL CHECK (length(objective) > 0 AND length(objective) <= 4000),
        objective_digest TEXT NOT NULL,
        origin_provenance TEXT NOT NULL CHECK (length(origin_provenance) > 0 AND length(origin_provenance) <= 1024),
        status TEXT NOT NULL CHECK (status IN ('active','blocked','completed','cancelled')),
        current_phase TEXT CHECK (current_phase IS NULL OR length(current_phase) <= 256),
        current_item_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        completed_at INTEGER,
        CHECK (origin_turn_id IS NULL OR length(origin_turn_id) <= 256),
        CHECK (goal_id IS NULL OR length(goal_id) <= 256),
        CHECK (length(session_id) > 0 AND length(session_id) <= 256),
        CHECK (length(project_id) > 0 AND length(project_id) <= 256)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS work_item (
        id TEXT PRIMARY KEY,
        plan_id TEXT NOT NULL REFERENCES work_plan(id) ON DELETE CASCADE,
        revision INTEGER NOT NULL CHECK (revision >= 0),
        position INTEGER NOT NULL CHECK (position >= 0),
        parent_item_id TEXT,
        dependencies_json TEXT NOT NULL DEFAULT '[]' CHECK (length(dependencies_json) <= 4096),
        status TEXT NOT NULL CHECK (status IN ('pending','actionable','in_progress','blocked','completed','cancelled')),
        description TEXT NOT NULL CHECK (length(description) > 0 AND length(description) <= 1024),
        acceptance_json TEXT NOT NULL DEFAULT '[]' CHECK (length(acceptance_json) <= 16384),
        evidence_json TEXT NOT NULL DEFAULT '[]' CHECK (length(evidence_json) <= 16384),
        owner_run_id TEXT CHECK (owner_run_id IS NULL OR length(owner_run_id) <= 256),
        owner_job_id TEXT CHECK (owner_job_id IS NULL OR length(owner_job_id) <= 256),
        attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
        blocker TEXT CHECK (blocker IS NULL OR length(blocker) <= 1024),
        next_action TEXT CHECK (next_action IS NULL OR length(next_action) <= 1024),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_work_plan_session_status ON work_plan(session_id, status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_work_plan_goal ON work_plan(goal_id) WHERE goal_id IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_work_item_plan ON work_item(plan_id, position)",
];

#[derive(sqlx::FromRow)]
struct WorkPlanRow {
    id: String,
    revision: i64,
    session_id: String,
    project_id: String,
    origin_turn_id: Option<String>,
    goal_id: Option<String>,
    objective: String,
    objective_digest: String,
    origin_provenance: String,
    status: String,
    current_phase: Option<String>,
    current_item_id: Option<String>,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct WorkItemRow {
    id: String,
    plan_id: String,
    revision: i64,
    position: i64,
    parent_item_id: Option<String>,
    dependencies_json: String,
    status: String,
    description: String,
    acceptance_json: String,
    evidence_json: String,
    owner_run_id: Option<String>,
    owner_job_id: Option<String>,
    attempts: i64,
    blocker: Option<String>,
    next_action: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn millis_to_datetime(ms: i64) -> Result<DateTime<Utc>, WorkPlanError> {
    DateTime::from_timestamp_millis(ms)
        .ok_or_else(|| WorkPlanError::Storage(format!("invalid work plan timestamp: {ms}")))
}

fn datetime_to_millis(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

fn plan_status_from_string(value: &str) -> Result<WorkPlanStatus, WorkPlanError> {
    WorkPlanStatus::parse(value)
        .ok_or_else(|| WorkPlanError::Storage(format!("invalid work plan status: {value}")))
}

fn item_status_from_string(value: &str) -> Result<WorkItemStatus, WorkPlanError> {
    WorkItemStatus::parse(value)
        .ok_or_else(|| WorkPlanError::Storage(format!("invalid work item status: {value}")))
}

fn disposition_from_string(value: &str) -> Result<WorkAcceptanceDisposition, WorkPlanError> {
    WorkAcceptanceDisposition::parse(value)
        .ok_or_else(|| WorkPlanError::Storage(format!("invalid acceptance disposition: {value}")))
}

fn evidence_kind_from_string(value: &str) -> Result<WorkEvidenceKind, WorkPlanError> {
    WorkEvidenceKind::parse(value)
        .ok_or_else(|| WorkPlanError::Storage(format!("invalid evidence kind: {value}")))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredAcceptance {
    description: String,
    disposition: String,
    note: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredEvidence {
    kind: String,
    ref_id: String,
    detail: Option<String>,
}

fn encode_acceptance(acceptance: &[WorkAcceptance]) -> Result<String, WorkPlanError> {
    let stored: Vec<StoredAcceptance> = acceptance
        .iter()
        .map(|item| StoredAcceptance {
            description: item.description.clone(),
            disposition: item.disposition.as_str().to_string(),
            note: item.note.clone(),
        })
        .collect();
    serde_json::to_string(&stored).map_err(|e| WorkPlanError::Storage(e.to_string()))
}

fn decode_acceptance(raw: &str) -> Result<Vec<WorkAcceptance>, WorkPlanError> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    let stored: Vec<StoredAcceptance> = serde_json::from_str(raw)
        .map_err(|e| WorkPlanError::Storage(format!("invalid acceptance: {e}")))?;
    let mut out = Vec::with_capacity(stored.len());
    for item in stored {
        out.push(WorkAcceptance {
            description: item.description,
            disposition: disposition_from_string(&item.disposition)?,
            note: item.note,
        });
    }
    Ok(out)
}

fn encode_evidence(evidence: &[WorkEvidenceRef]) -> Result<String, WorkPlanError> {
    let stored: Vec<StoredEvidence> = evidence
        .iter()
        .map(|item| StoredEvidence {
            kind: item.kind.as_str().to_string(),
            ref_id: item.ref_id.clone(),
            detail: item.detail.clone(),
        })
        .collect();
    serde_json::to_string(&stored).map_err(|e| WorkPlanError::Storage(e.to_string()))
}

fn decode_evidence(raw: &str) -> Result<Vec<WorkEvidenceRef>, WorkPlanError> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    let stored: Vec<StoredEvidence> = serde_json::from_str(raw)
        .map_err(|e| WorkPlanError::Storage(format!("invalid evidence: {e}")))?;
    let mut out = Vec::with_capacity(stored.len());
    for item in stored {
        out.push(WorkEvidenceRef {
            kind: evidence_kind_from_string(&item.kind)?,
            ref_id: item.ref_id,
            detail: item.detail,
        });
    }
    Ok(out)
}

fn encode_dependencies(deps: &[WorkItemId]) -> Result<String, WorkPlanError> {
    let ids: Vec<&str> = deps.iter().map(|d| d.as_str()).collect();
    serde_json::to_string(&ids).map_err(|e| WorkPlanError::Storage(e.to_string()))
}

fn decode_dependencies(raw: &str) -> Result<Vec<WorkItemId>, WorkPlanError> {
    if raw.trim().is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<String> = serde_json::from_str(raw)
        .map_err(|e| WorkPlanError::Storage(format!("invalid dependencies: {e}")))?;
    Ok(ids.into_iter().map(WorkItemId).collect())
}

fn row_to_plan(row: WorkPlanRow) -> Result<WorkPlan, WorkPlanError> {
    Ok(WorkPlan {
        id: WorkPlanId(row.id),
        revision: row.revision,
        session_id: row.session_id,
        project_id: row.project_id,
        origin_turn_id: row.origin_turn_id,
        goal_id: row.goal_id,
        objective: row.objective,
        objective_digest: row.objective_digest,
        origin_provenance: row.origin_provenance,
        status: plan_status_from_string(&row.status)?,
        current_phase: row.current_phase,
        current_item_id: row.current_item_id.map(WorkItemId),
        created_at: millis_to_datetime(row.created_at)?,
        updated_at: millis_to_datetime(row.updated_at)?,
        completed_at: row.completed_at.map(millis_to_datetime).transpose()?,
    })
}

fn row_to_item(row: WorkItemRow) -> Result<WorkItem, WorkPlanError> {
    Ok(WorkItem {
        id: WorkItemId(row.id),
        plan_id: WorkPlanId(row.plan_id),
        revision: row.revision,
        position: row.position,
        parent_item_id: row.parent_item_id.map(WorkItemId),
        dependencies: decode_dependencies(&row.dependencies_json)?,
        status: item_status_from_string(&row.status)?,
        description: row.description,
        acceptance: decode_acceptance(&row.acceptance_json)?,
        evidence: decode_evidence(&row.evidence_json)?,
        owner_run_id: row.owner_run_id,
        owner_job_id: row.owner_job_id,
        attempts: row.attempts,
        blocker: row.blocker,
        next_action: row.next_action,
        created_at: millis_to_datetime(row.created_at)?,
        updated_at: millis_to_datetime(row.updated_at)?,
    })
}

/// The single store/service owner for durable WorkPlans.
#[derive(Clone)]
pub struct WorkPlanStore {
    pool: SqlitePool,
}

impl WorkPlanStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn fetch_plan(&self, id: &str) -> Result<Option<WorkPlan>, WorkPlanError> {
        let row = sqlx::query_as::<_, WorkPlanRow>("SELECT * FROM work_plan WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_plan).transpose()
    }

    async fn fetch_items(&self, plan_id: &str) -> Result<Vec<WorkItem>, WorkPlanError> {
        let rows = sqlx::query_as::<_, WorkItemRow>(
            "SELECT * FROM work_item WHERE plan_id = ?1 ORDER BY position ASC, id ASC",
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_item).collect()
    }

    async fn require_plan(&self, id: &str) -> Result<WorkPlan, WorkPlanError> {
        self.fetch_plan(id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(id.to_string()))
    }

    fn validate_goal_binding(
        &self,
        plan_session: &str,
        plan_project: &str,
        goal_id: &str,
    ) -> Result<(), WorkPlanError> {
        if goal_id.trim().is_empty() {
            return Err(WorkPlanError::Validation(
                "goal_id must not be empty".to_string(),
            ));
        }
        if goal_id.len() > super::model::MAX_SCOPE_ID_CHARS {
            return Err(WorkPlanError::Validation(
                "goal_id exceeds bound".to_string(),
            ));
        }
        if goal_id.contains('\0') {
            return Err(WorkPlanError::Validation(
                "goal_id must not contain NUL".to_string(),
            ));
        }
        let _ = (plan_session, plan_project);
        Ok(())
    }

    async fn check_goal_scope(
        &self,
        plan_session: &str,
        plan_project: &str,
        goal_id: &str,
    ) -> Result<(), WorkPlanError> {
        self.validate_goal_binding(plan_session, plan_project, goal_id)?;
        // Validated ownership: the bound Goal must exist and live in the
        // same session/project. Goal runtime behavior is untouched; this is
        // a reference with scope checking, not authority.
        #[derive(sqlx::FromRow)]
        struct GoalScope {
            session_id: String,
            project_id: String,
        }
        let scope =
            sqlx::query_as::<_, GoalScope>("SELECT session_id, project_id FROM goal WHERE id = ?1")
                .bind(goal_id)
                .fetch_optional(&self.pool)
                .await?;
        match scope {
            None => Err(WorkPlanError::Validation(format!(
                "bound goal {goal_id} does not exist"
            ))),
            Some(scope) => {
                if scope.session_id != plan_session || scope.project_id != plan_project {
                    return Err(WorkPlanError::ScopeMismatch(format!(
                        "goal {goal_id} belongs to session {}/project {}",
                        scope.session_id, scope.project_id
                    )));
                }
                Ok(())
            }
        }
    }

    /// Create an active plan, cancelling any existing active/blocked plan
    /// for the same session in the same transaction. Replacement never
    /// deletes historical evidence; the predecessor is terminal-cancelled.
    pub async fn create_active(&self, input: NewWorkPlan) -> Result<WorkPlan, WorkPlanError> {
        validate_new_work_plan(&input)?;
        if let Some(goal_id) = input.goal_id.as_deref() {
            self.check_goal_scope(&input.session_id, &input.project_id, goal_id)
                .await?;
            // At most one active plan per goal as well as per session.
            let existing = sqlx::query_as::<_, WorkPlanRow>(
                "SELECT * FROM work_plan WHERE goal_id = ?1 AND status IN ('active','blocked') LIMIT 1",
            )
            .bind(goal_id)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(row) = existing {
                // Same-session replacement of the goal-bound plan is allowed
                // through the session-cancel path below; a different session
                // holding the goal is a scope conflict.
                let plan = row_to_plan(row)?;
                if plan.session_id != input.session_id || plan.project_id != input.project_id {
                    return Err(WorkPlanError::ScopeMismatch(format!(
                        "goal {goal_id} already has an active plan {}",
                        plan.id.as_str()
                    )));
                }
            }
        }

        let id = WorkPlanId::generate();
        let now = datetime_to_millis(&Utc::now());
        let digest = work_plan_origin_digest(&input.objective);
        let status = WorkPlanStatus::Active.as_str().to_string();

        let mut tx = self.pool.begin().await?;
        let cancel_result = sqlx::query(
            "UPDATE work_plan SET status = 'cancelled', updated_at = ?1, revision = revision + 1 \
             WHERE session_id = ?2 AND status IN ('active','blocked')",
        )
        .bind(now)
        .bind(&input.session_id)
        .execute(&mut *tx)
        .await;
        if let Err(e) = cancel_result {
            // Fresh databases pre-migration surface here; surface as storage
            // so callers see one error kind.
            return Err(WorkPlanError::Storage(e.to_string()));
        }
        sqlx::query(
            r#"INSERT INTO work_plan
               (id, revision, session_id, project_id, origin_turn_id, goal_id,
                objective, objective_digest, origin_provenance, status,
                current_phase, current_item_id, created_at, updated_at, completed_at)
               VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?11, NULL)"#,
        )
        .bind(id.as_str())
        .bind(&input.session_id)
        .bind(&input.project_id)
        .bind(input.origin_turn_id.as_deref())
        .bind(input.goal_id.as_deref())
        .bind(&input.objective)
        .bind(&digest)
        .bind(&input.origin_provenance)
        .bind(&status)
        .bind(input.current_phase.as_deref())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        self.require_plan(id.as_str()).await
    }

    pub async fn get(&self, id: &WorkPlanId) -> Result<Option<WorkPlan>, WorkPlanError> {
        self.fetch_plan(id.as_str()).await
    }

    /// Latest active/blocked plan for a session, if any. Legacy sessions
    /// simply have none.
    pub async fn active_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<WorkPlan>, WorkPlanError> {
        let row = sqlx::query_as::<_, WorkPlanRow>(
            "SELECT * FROM work_plan WHERE session_id = ?1 AND status IN ('active','blocked') \
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_plan).transpose()
    }

    pub async fn active_for_goal(&self, goal_id: &str) -> Result<Option<WorkPlan>, WorkPlanError> {
        let row = sqlx::query_as::<_, WorkPlanRow>(
            "SELECT * FROM work_plan WHERE goal_id = ?1 AND status IN ('active','blocked') \
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(goal_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_plan).transpose()
    }

    pub async fn get_item(&self, id: &WorkItemId) -> Result<Option<WorkItem>, WorkPlanError> {
        let row = sqlx::query_as::<_, WorkItemRow>("SELECT * FROM work_item WHERE id = ?1")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_item).transpose()
    }

    /// Bounded item listing in stable `(position, id)` order.
    pub async fn list_items(&self, plan_id: &WorkPlanId) -> Result<Vec<WorkItem>, WorkPlanError> {
        self.fetch_items(plan_id.as_str()).await
    }

    /// Deterministic actionability for a plan.
    pub async fn actionable_items(
        &self,
        plan_id: &WorkPlanId,
    ) -> Result<Vec<WorkItem>, WorkPlanError> {
        let items = self.fetch_items(plan_id.as_str()).await?;
        Ok(actionable_items(&items).into_iter().cloned().collect())
    }

    async fn bump_plan_revision(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        plan_id: &str,
        now: i64,
    ) -> Result<(), WorkPlanError> {
        sqlx::query("UPDATE work_plan SET revision = revision + 1, updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(plan_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Add one item. Plan revision bumps so a concurrent plan
    /// cancel/complete against a stale revision fails instead of silently
    /// racing the add.
    pub async fn add_item(
        &self,
        plan_id: &WorkPlanId,
        input: NewWorkItem,
    ) -> Result<(WorkPlan, WorkItem), WorkPlanError> {
        let plan = self.require_plan(plan_id.as_str()).await?;
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }
        let existing = self.fetch_items(plan_id.as_str()).await?;
        validate_new_work_item(plan_id, &input, existing.len())?;
        validate_item_graph(
            &WorkItemId("wi_candidate".to_string()),
            input.parent_item_id.as_ref(),
            &input.dependencies,
            &existing,
        )
        .map_err(|e| match e {
            // Rewrite the synthetic candidate id out of user-facing errors.
            WorkPlanError::Validation(msg) => {
                WorkPlanError::Validation(msg.replace("wi_candidate", "<new item>"))
            }
            other => other,
        })?;
        // Parent/dependency scope is enforced by validate_item_graph
        // (same-plan membership); cross-plan refs are rejected there.

        let id = WorkItemId::generate();
        // Full graph check with the real id (self-edge safety).
        let mut with_new = existing.clone();
        let now_dt = Utc::now();
        let candidate = WorkItem {
            id: id.clone(),
            plan_id: plan_id.clone(),
            revision: 0,
            position: next_position(&existing),
            parent_item_id: input.parent_item_id.clone(),
            dependencies: input.dependencies.clone(),
            status: input.status,
            description: input.description.clone(),
            acceptance: input.acceptance.clone(),
            evidence: input.evidence.clone(),
            owner_run_id: input.owner_run_id.clone(),
            owner_job_id: input.owner_job_id.clone(),
            attempts: 0,
            blocker: input.blocker.clone(),
            next_action: input.next_action.clone(),
            created_at: now_dt,
            updated_at: now_dt,
        };
        with_new.push(candidate.clone());
        validate_item_graph(
            &id,
            candidate.parent_item_id.as_ref(),
            &candidate.dependencies,
            &with_new,
        )?;

        let now = datetime_to_millis(&Utc::now());
        let deps_json = encode_dependencies(&input.dependencies)?;
        let acceptance_json = encode_acceptance(&input.acceptance)?;
        let evidence_json = encode_evidence(&input.evidence)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"INSERT INTO work_item
               (id, plan_id, revision, position, parent_item_id, dependencies_json,
                status, description, acceptance_json, evidence_json,
                owner_run_id, owner_job_id, attempts, blocker, next_action,
                created_at, updated_at)
               VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13, ?14, ?14)"#,
        )
        .bind(id.as_str())
        .bind(plan_id.as_str())
        .bind(candidate.position)
        .bind(input.parent_item_id.as_ref().map(|p| p.as_str()))
        .bind(&deps_json)
        .bind(input.status.as_str())
        .bind(&input.description)
        .bind(&acceptance_json)
        .bind(&evidence_json)
        .bind(input.owner_run_id.as_deref())
        .bind(input.owner_job_id.as_deref())
        .bind(input.blocker.as_deref())
        .bind(input.next_action.as_deref())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        Self::bump_plan_revision(&mut tx, plan_id.as_str(), now).await?;
        tx.commit().await?;

        let plan = self.require_plan(plan_id.as_str()).await?;
        let item = self
            .get_item(&id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(id.as_str().to_string()))?;
        Ok((plan, item))
    }

    /// CAS update of mutable item fields. Status changes must use
    /// [`Self::transition_item`].
    #[allow(clippy::too_many_arguments)]
    pub async fn update_item(
        &self,
        item_id: &WorkItemId,
        expected_revision: i64,
        patch: WorkItemPatch,
    ) -> Result<(WorkPlan, WorkItem), WorkPlanError> {
        let current = self
            .get_item(item_id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(item_id.as_str().to_string()))?;
        if current.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: current.revision,
            });
        }
        let plan = self.require_plan(current.plan_id.as_str()).await?;
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }
        if current.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "item {} is {}",
                current.id.as_str(),
                current.status.as_str()
            )));
        }

        let parent = patch
            .parent_item_id
            .unwrap_or(current.parent_item_id.clone());
        let dependencies = patch.dependencies.unwrap_or(current.dependencies.clone());
        let description = patch.description.unwrap_or(current.description.clone());
        let acceptance = patch.acceptance.unwrap_or(current.acceptance.clone());
        let evidence = patch.evidence.unwrap_or(current.evidence.clone());
        let owner_run_id = patch.owner_run_id.unwrap_or(current.owner_run_id.clone());
        let owner_job_id = patch.owner_job_id.unwrap_or(current.owner_job_id.clone());
        let blocker = patch.blocker.unwrap_or(current.blocker.clone());
        let next_action = patch.next_action.unwrap_or(current.next_action.clone());

        if dependencies.len() > super::model::MAX_DEPENDENCIES_PER_ITEM {
            return Err(WorkPlanError::Validation(
                "dependencies exceed bound".to_string(),
            ));
        }
        if acceptance.len() > super::model::MAX_ACCEPTANCE_PER_ITEM {
            return Err(WorkPlanError::Validation(
                "acceptance exceeds bound".to_string(),
            ));
        }
        if evidence.len() > super::model::MAX_EVIDENCE_PER_ITEM {
            return Err(WorkPlanError::Validation(
                "evidence exceeds bound".to_string(),
            ));
        }
        if description.trim().is_empty() {
            return Err(WorkPlanError::Validation(
                "item description must not be empty".to_string(),
            ));
        }
        if description.chars().count() > super::model::MAX_ITEM_DESCRIPTION_CHARS {
            return Err(WorkPlanError::Validation(
                "item description exceeds bound".to_string(),
            ));
        }
        for criterion in &acceptance {
            validate_acceptance(criterion)?;
        }
        for ref_item in &evidence {
            validate_evidence_ref(ref_item)?;
        }
        validate_item_blocker_rule(current.status, blocker.as_deref())?;
        // Owner refs are provenance only; validate shape, never authority.
        if let Some(value) = owner_run_id.as_deref() {
            if value.len() > super::model::MAX_OWNER_REF_CHARS || value.contains('\0') {
                return Err(WorkPlanError::Validation(
                    "owner_run_id exceeds bound".to_string(),
                ));
            }
        }
        if let Some(value) = owner_job_id.as_deref() {
            if value.len() > super::model::MAX_OWNER_REF_CHARS || value.contains('\0') {
                return Err(WorkPlanError::Validation(
                    "owner_job_id exceeds bound".to_string(),
                ));
            }
        }
        if let Some(value) = next_action.as_deref() {
            if value.chars().count() > super::model::MAX_NEXT_ACTION_CHARS || value.contains('\0') {
                return Err(WorkPlanError::Validation(
                    "next_action exceeds bound".to_string(),
                ));
            }
        }
        if let Some(parent_id) = parent.as_ref() {
            super::model::validate_work_item_id(parent_id)?;
        }
        {
            use std::collections::HashSet;
            let mut seen = HashSet::new();
            for dep in &dependencies {
                super::model::validate_work_item_id(dep)?;
                if !seen.insert(dep.as_str()) {
                    return Err(WorkPlanError::Validation(
                        "duplicate dependency".to_string(),
                    ));
                }
            }
        }

        let siblings = self.fetch_items(current.plan_id.as_str()).await?;
        validate_item_graph(item_id, parent.as_ref(), &dependencies, &siblings)?;

        let now = datetime_to_millis(&Utc::now());
        let deps_json = encode_dependencies(&dependencies)?;
        let acceptance_json = encode_acceptance(&acceptance)?;
        let evidence_json = encode_evidence(&evidence)?;
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"UPDATE work_item SET revision = revision + 1, parent_item_id = ?1,
                dependencies_json = ?2, description = ?3, acceptance_json = ?4,
                evidence_json = ?5, owner_run_id = ?6, owner_job_id = ?7,
                blocker = ?8, next_action = ?9, updated_at = ?10
               WHERE id = ?11 AND revision = ?12"#,
        )
        .bind(parent.as_ref().map(|p| p.as_str()))
        .bind(&deps_json)
        .bind(&description)
        .bind(&acceptance_json)
        .bind(&evidence_json)
        .bind(owner_run_id.as_deref())
        .bind(owner_job_id.as_deref())
        .bind(blocker.as_deref())
        .bind(next_action.as_deref())
        .bind(now)
        .bind(item_id.as_str())
        .bind(expected_revision)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .get_item(item_id)
                    .await?
                    .map(|item| item.revision)
                    .unwrap_or(-1),
            });
        }
        Self::bump_plan_revision(&mut tx, current.plan_id.as_str(), now).await?;
        tx.commit().await?;

        let item = self
            .get_item(item_id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(item_id.as_str().to_string()))?;
        let plan = self.require_plan(item.plan_id.as_str()).await?;
        Ok((plan, item))
    }

    /// Guarded item status transition with CAS. Entering `InProgress`
    /// bumps `attempts`; entering `Blocked` requires a blocker reason and
    /// leaving it clears the reason unless the caller supplies the next
    /// state's valid blocker (always `None` outside `Blocked`).
    pub async fn transition_item(
        &self,
        item_id: &WorkItemId,
        expected_revision: i64,
        new_status: WorkItemStatus,
        blocker: Option<String>,
        next_action: Option<String>,
    ) -> Result<(WorkPlan, WorkItem), WorkPlanError> {
        let current = self
            .get_item(item_id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(item_id.as_str().to_string()))?;
        if current.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: current.revision,
            });
        }
        if !can_transition_item(current.status, new_status) {
            return Err(WorkPlanError::Validation(format!(
                "item transition {} -> {} rejected",
                current.status.as_str(),
                new_status.as_str()
            )));
        }
        // Blocker discipline: Blocked requires a reason; every other state
        // must not retain one.
        let effective_blocker = match new_status {
            WorkItemStatus::Blocked => {
                let reason = blocker.or(current.blocker.clone());
                validate_item_blocker_rule(new_status, reason.as_deref())?;
                reason
            }
            _ => {
                if let Some(text) = blocker.as_deref() {
                    if !text.trim().is_empty() {
                        return Err(WorkPlanError::Validation(
                            "blocker reason is only valid for blocked items".to_string(),
                        ));
                    }
                }
                None
            }
        };
        if let Some(action) = next_action.as_deref() {
            if action.chars().count() > super::model::MAX_NEXT_ACTION_CHARS || action.contains('\0')
            {
                return Err(WorkPlanError::Validation(
                    "next_action exceeds bound".to_string(),
                ));
            }
        }
        // Completion requires no silent evidence fabrication: an item whose
        // acceptance is entirely `Unmet` with no evidence refs and no owner
        // run/job provenance cannot be marked completed by store callers
        // without at least one host-correlatable signal. M003 owns the full
        // arbiter; this is the M002 floor that keeps bare model text from
        // manufacturing `Satisfied`.
        if new_status == WorkItemStatus::Completed {
            let has_signal = current.owner_run_id.is_some()
                || current.owner_job_id.is_some()
                || !current.evidence.is_empty()
                || current
                    .acceptance
                    .iter()
                    .any(|criterion| criterion.disposition != WorkAcceptanceDisposition::Unmet);
            if !has_signal {
                return Err(WorkPlanError::Validation(
                    "completed items require acceptance disposition, evidence ref, or owner run/job provenance".to_string(),
                ));
            }
        }

        let plan = self.require_plan(current.plan_id.as_str()).await?;
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }

        let now = datetime_to_millis(&Utc::now());
        let attempts_delta = i64::from(new_status == WorkItemStatus::InProgress);
        let effective_next_action = next_action.or(current.next_action.clone());
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "UPDATE work_item SET status = ?1, blocker = ?2, next_action = ?3, \
             attempts = attempts + ?4, revision = revision + 1, updated_at = ?5 \
             WHERE id = ?6 AND revision = ?7",
        )
        .bind(new_status.as_str())
        .bind(effective_blocker.as_deref())
        .bind(effective_next_action.as_deref())
        .bind(attempts_delta)
        .bind(now)
        .bind(item_id.as_str())
        .bind(expected_revision)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .get_item(item_id)
                    .await?
                    .map(|item| item.revision)
                    .unwrap_or(-1),
            });
        }
        Self::bump_plan_revision(&mut tx, current.plan_id.as_str(), now).await?;
        tx.commit().await?;

        let item = self
            .get_item(item_id)
            .await?
            .ok_or_else(|| WorkPlanError::NotFound(item_id.as_str().to_string()))?;
        let plan = self.require_plan(item.plan_id.as_str()).await?;
        Ok((plan, item))
    }

    /// CAS update of plan phase/current-item pointers. Objective and
    /// provenance are immutable.
    pub async fn update_plan_meta(
        &self,
        plan_id: &WorkPlanId,
        expected_revision: i64,
        current_phase: Option<Option<String>>,
        current_item_id: Option<Option<WorkItemId>>,
    ) -> Result<WorkPlan, WorkPlanError> {
        let plan = self.require_plan(plan_id.as_str()).await?;
        if plan.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: plan.revision,
            });
        }
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }
        let phase = current_phase.unwrap_or(plan.current_phase.clone());
        if let Some(value) = phase.as_deref() {
            if value.chars().count() > super::model::MAX_PHASE_CHARS || value.contains('\0') {
                return Err(WorkPlanError::Validation(
                    "current_phase exceeds bound".to_string(),
                ));
            }
        }
        let current_item = current_item_id.unwrap_or(plan.current_item_id.clone());
        if let Some(item_id) = current_item.as_ref() {
            let item = self.get_item(item_id).await?.ok_or_else(|| {
                WorkPlanError::Validation("current item is not in any plan".to_string())
            })?;
            if item.plan_id != *plan_id {
                return Err(WorkPlanError::ScopeMismatch(
                    "current item belongs to a different plan".to_string(),
                ));
            }
        }
        let now = datetime_to_millis(&Utc::now());
        let result = sqlx::query(
            "UPDATE work_plan SET current_phase = ?1, current_item_id = ?2, \
             revision = revision + 1, updated_at = ?3 WHERE id = ?4 AND revision = ?5",
        )
        .bind(phase.as_deref())
        .bind(current_item.as_ref().map(|id| id.as_str()))
        .bind(now)
        .bind(plan_id.as_str())
        .bind(expected_revision)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .fetch_plan(plan_id.as_str())
                    .await?
                    .map(|p| p.revision)
                    .unwrap_or(-1),
            });
        }
        self.require_plan(plan_id.as_str()).await
    }

    /// Guarded plan status transition with CAS.
    pub async fn transition_plan(
        &self,
        plan_id: &WorkPlanId,
        expected_revision: i64,
        new_status: WorkPlanStatus,
    ) -> Result<WorkPlan, WorkPlanError> {
        let plan = self.require_plan(plan_id.as_str()).await?;
        if plan.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: plan.revision,
            });
        }
        if !can_transition_plan(plan.status, new_status) {
            return Err(WorkPlanError::Validation(format!(
                "plan transition {} -> {} rejected",
                plan.status.as_str(),
                new_status.as_str()
            )));
        }
        let now = datetime_to_millis(&Utc::now());
        let completed_at = if new_status.is_terminal() {
            Some(now)
        } else {
            None
        };
        let result = sqlx::query(
            "UPDATE work_plan SET status = ?1, revision = revision + 1, updated_at = ?2, \
             completed_at = ?3 WHERE id = ?4 AND revision = ?5",
        )
        .bind(new_status.as_str())
        .bind(now)
        .bind(completed_at)
        .bind(plan_id.as_str())
        .bind(expected_revision)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .fetch_plan(plan_id.as_str())
                    .await?
                    .map(|p| p.revision)
                    .unwrap_or(-1),
            });
        }
        self.require_plan(plan_id.as_str()).await
    }

    /// Bind an exact Goal with validated same-session/project ownership.
    /// Goal status/budget remain authoritative; this only records the
    /// reference. At most one active plan per goal.
    pub async fn bind_goal(
        &self,
        plan_id: &WorkPlanId,
        expected_revision: i64,
        goal_id: &str,
    ) -> Result<WorkPlan, WorkPlanError> {
        let plan = self.require_plan(plan_id.as_str()).await?;
        if plan.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: plan.revision,
            });
        }
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }
        self.check_goal_scope(&plan.session_id, &plan.project_id, goal_id)
            .await?;
        let holder = sqlx::query_as::<_, WorkPlanRow>(
            "SELECT * FROM work_plan WHERE goal_id = ?1 AND status IN ('active','blocked') LIMIT 1",
        )
        .bind(goal_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = holder {
            let other = row_to_plan(row)?;
            if other.id != *plan_id {
                return Err(WorkPlanError::ScopeMismatch(format!(
                    "goal {goal_id} already bound to plan {}",
                    other.id.as_str()
                )));
            }
        }
        let now = datetime_to_millis(&Utc::now());
        let result = sqlx::query(
            "UPDATE work_plan SET goal_id = ?1, revision = revision + 1, updated_at = ?2 \
             WHERE id = ?3 AND revision = ?4",
        )
        .bind(goal_id)
        .bind(now)
        .bind(plan_id.as_str())
        .bind(expected_revision)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .fetch_plan(plan_id.as_str())
                    .await?
                    .map(|p| p.revision)
                    .unwrap_or(-1),
            });
        }
        self.require_plan(plan_id.as_str()).await
    }

    /// Clear the Goal binding through CAS. Historical evidence is kept;
    /// only the live reference is removed.
    pub async fn unbind_goal(
        &self,
        plan_id: &WorkPlanId,
        expected_revision: i64,
    ) -> Result<WorkPlan, WorkPlanError> {
        let plan = self.require_plan(plan_id.as_str()).await?;
        if plan.revision != expected_revision {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: plan.revision,
            });
        }
        if plan.status.is_terminal() {
            return Err(WorkPlanError::Terminal(format!(
                "plan {} is {}",
                plan.id.as_str(),
                plan.status.as_str()
            )));
        }
        let now = datetime_to_millis(&Utc::now());
        let result = sqlx::query(
            "UPDATE work_plan SET goal_id = NULL, revision = revision + 1, updated_at = ?1 \
             WHERE id = ?2 AND revision = ?3",
        )
        .bind(now)
        .bind(plan_id.as_str())
        .bind(expected_revision)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::Conflict {
                expected: expected_revision,
                found: self
                    .fetch_plan(plan_id.as_str())
                    .await?
                    .map(|p| p.revision)
                    .unwrap_or(-1),
            });
        }
        self.require_plan(plan_id.as_str()).await
    }
}

fn next_position(existing: &[WorkItem]) -> i64 {
    existing
        .iter()
        .map(|item| item.position)
        .max()
        .unwrap_or(-1)
        + 1
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
        WorkEvidenceRef, WorkItemId, WorkItemPatch, WorkItemStatus, WorkPlanError, WorkPlanStatus,
        MAX_ITEMS_PER_PLAN,
    };
    use super::WorkPlanStore;

    async fn temp_pool() -> sqlx::SqlitePool {
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

    fn plan_input(session: &str) -> NewWorkPlan {
        NewWorkPlan {
            session_id: session.to_string(),
            project_id: "proj-1".to_string(),
            origin_turn_id: Some("turn-1".to_string()),
            goal_id: None,
            objective: "implement durable work plans".to_string(),
            origin_provenance: "turn:turn-1".to_string(),
            current_phase: Some("foundation".to_string()),
        }
    }

    fn item_input(description: &str) -> NewWorkItem {
        NewWorkItem {
            parent_item_id: None,
            dependencies: vec![],
            status: WorkItemStatus::Actionable,
            description: description.to_string(),
            acceptance: vec![WorkAcceptance {
                description: "criterion".to_string(),
                disposition: WorkAcceptanceDisposition::Unmet,
                note: None,
            }],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            blocker: None,
            next_action: Some("start".to_string()),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_and_load_round_trip() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        assert_eq!(plan.revision, 0);
        assert_eq!(plan.status, WorkPlanStatus::Active);
        assert!(plan.objective_digest.starts_with("sha256:"));
        let loaded = store.get(&plan.id).await.unwrap().unwrap();
        assert_eq!(loaded.id, plan.id);
        let active = store.active_for_session("sess-1").await.unwrap().unwrap();
        assert_eq!(active.id, plan.id);
        assert!(store.active_for_session("other").await.unwrap().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replacement_cancels_predecessor_without_deleting() {
        let store = WorkPlanStore::new(temp_pool().await);
        let first = store.create_active(plan_input("sess-1")).await.unwrap();
        let (_, historic) = store
            .add_item(&first.id, item_input("old work"))
            .await
            .unwrap();
        let second = store.create_active(plan_input("sess-1")).await.unwrap();
        assert_ne!(first.id, second.id);
        let predecessor = store.get(&first.id).await.unwrap().unwrap();
        assert_eq!(predecessor.status, WorkPlanStatus::Cancelled);
        // Items of the cancelled plan remain as historical evidence.
        let retained = store.get_item(&historic.id).await.unwrap().unwrap();
        assert_eq!(retained.plan_id, first.id);
        // Further mutation of a cancelled plan is rejected, not silently
        // applied.
        let err = store.add_item(&first.id, item_input("late work")).await;
        assert!(matches!(err, Err(WorkPlanError::Terminal(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn item_lifecycle_with_cas() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        let (plan_after_add, item) = store.add_item(&plan.id, item_input("first")).await.unwrap();
        assert_eq!(item.revision, 0);
        assert!(plan_after_add.revision > plan.revision);

        let (_, progressed) = store
            .transition_item(
                &item.id,
                item.revision,
                WorkItemStatus::InProgress,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(progressed.attempts, 1);

        // Stale writer cannot overwrite newer state.
        let stale = store
            .transition_item(
                &item.id,
                item.revision,
                WorkItemStatus::Completed,
                None,
                None,
            )
            .await;
        assert!(matches!(stale, Err(WorkPlanError::Conflict { .. })));

        // Blocked requires a reason.
        let blocked_err = store
            .transition_item(
                &progressed.id,
                progressed.revision,
                WorkItemStatus::Blocked,
                None,
                None,
            )
            .await;
        assert!(matches!(blocked_err, Err(WorkPlanError::Validation(_))));

        let (_, blocked) = store
            .transition_item(
                &progressed.id,
                progressed.revision,
                WorkItemStatus::Blocked,
                Some("waiting on test".to_string()),
                None,
            )
            .await
            .unwrap();
        assert_eq!(blocked.blocker.as_deref(), Some("waiting on test"));

        // Blocked cannot complete directly.
        let direct = store
            .transition_item(
                &blocked.id,
                blocked.revision,
                WorkItemStatus::Completed,
                None,
                None,
            )
            .await;
        assert!(matches!(direct, Err(WorkPlanError::Validation(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completion_requires_host_signal() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        let bare = NewWorkItem {
            acceptance: vec![],
            ..item_input("bare")
        };
        let (_, item) = store.add_item(&plan.id, bare).await.unwrap();
        let (_, in_progress) = store
            .transition_item(
                &item.id,
                item.revision,
                WorkItemStatus::InProgress,
                None,
                None,
            )
            .await
            .unwrap();
        // No acceptance signal, no evidence, no owner ref: completion refused.
        let err = store
            .transition_item(
                &in_progress.id,
                in_progress.revision,
                WorkItemStatus::Completed,
                None,
                None,
            )
            .await;
        assert!(matches!(err, Err(WorkPlanError::Validation(_))));

        // With an evidence ref the same transition is accepted.
        let with_evidence = NewWorkItem {
            evidence: vec![WorkEvidenceRef {
                kind: WorkEvidenceKind::TestJob,
                ref_id: "job-1".to_string(),
                detail: None,
            }],
            ..item_input("evidenced")
        };
        let (_, second) = store.add_item(&plan.id, with_evidence).await.unwrap();
        let (_, started) = store
            .transition_item(
                &second.id,
                second.revision,
                WorkItemStatus::InProgress,
                None,
                None,
            )
            .await
            .unwrap();
        let (_, done) = store
            .transition_item(
                &started.id,
                started.revision,
                WorkItemStatus::Completed,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(done.status, WorkItemStatus::Completed);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plan_cas_serializes_against_item_mutation() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        let observed_revision = plan.revision;
        let (_, _) = store.add_item(&plan.id, item_input("a")).await.unwrap();
        // The item add bumped the plan revision, so a cancel against the
        // stale revision fails instead of silently winning.
        let stale_cancel = store
            .transition_plan(&plan.id, observed_revision, WorkPlanStatus::Cancelled)
            .await;
        assert!(matches!(stale_cancel, Err(WorkPlanError::Conflict { .. })));
        let current = store.get(&plan.id).await.unwrap().unwrap();
        let cancelled = store
            .transition_plan(&plan.id, current.revision, WorkPlanStatus::Cancelled)
            .await
            .unwrap();
        assert_eq!(cancelled.status, WorkPlanStatus::Cancelled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_item_patch_validates_bounds_and_scope() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        let (_, item) = store.add_item(&plan.id, item_input("a")).await.unwrap();
        let oversized = "x".repeat(2048);
        let err = store
            .update_item(
                &item.id,
                item.revision,
                WorkItemPatch {
                    description: Some(oversized),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(err, Err(WorkPlanError::Validation(_))));

        // Cross-plan dependency rejected.
        let other = store.create_active(plan_input("sess-2")).await.unwrap();
        let (_, foreign) = store
            .add_item(&other.id, item_input("foreign"))
            .await
            .unwrap();
        let cross = store
            .update_item(
                &item.id,
                item.revision,
                WorkItemPatch {
                    dependencies: Some(vec![foreign.id.clone()]),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(
            cross,
            Err(WorkPlanError::Validation(_) | WorkPlanError::ScopeMismatch(_))
        ));
        let _ = WorkItemId::generate();
    }

    async fn ensure_test_session(pool: &sqlx::SqlitePool, session_id: &str, project_id: &str) {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, '[]', ?, ?)",
        )
        .bind(project_id)
        .bind("/tmp/test")
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 'test', '/tmp/test', 'Test', '1', ?, ?)",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn second_active_plan_per_goal_rejected_across_sessions() {
        let pool = temp_pool().await;
        ensure_test_session(&pool, "sess-1", "proj-1").await;
        ensure_test_session(&pool, "sess-2", "proj-2").await;
        // Seed a goal in sess-1/proj-1.
        let goal_store = crate::goal::GoalStore::new(pool.clone());
        let goal = goal_store
            .create_active("sess-1", "proj-1", "title", "objective", None, None, vec![])
            .await
            .unwrap();
        let store = WorkPlanStore::new(pool);
        let mut first_input = plan_input("sess-1");
        first_input.project_id = "proj-1".to_string();
        first_input.goal_id = Some(goal.id.clone());
        let first = store.create_active(first_input).await.unwrap();
        assert_eq!(first.goal_id.as_deref(), Some(goal.id.as_str()));
        let active = store.active_for_goal(&goal.id).await.unwrap().unwrap();
        assert_eq!(active.id, first.id);

        // Same goal from another session/project is a scope mismatch.
        let mut second_input = plan_input("sess-2");
        second_input.project_id = "proj-2".to_string();
        second_input.goal_id = Some(goal.id.clone());
        let err = store.create_active(second_input).await;
        assert!(matches!(
            err,
            Err(WorkPlanError::Validation(_) | WorkPlanError::ScopeMismatch(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_bounded_plan_round_trips() {
        let store = WorkPlanStore::new(temp_pool().await);
        let plan = store.create_active(plan_input("sess-1")).await.unwrap();
        // More than TodoState's projection cap (12) but within the plan cap.
        for index in 0..20 {
            let (_, _) = store
                .add_item(&plan.id, item_input(&format!("item {index}")))
                .await
                .unwrap();
        }
        let items = store.list_items(&plan.id).await.unwrap();
        assert_eq!(items.len(), 20);
        assert!(items.len() > 12);
        assert!(items.len() <= MAX_ITEMS_PER_PLAN);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_goal_binding_rejected() {
        let store = WorkPlanStore::new(temp_pool().await);
        let mut input = plan_input("sess-1");
        input.goal_id = Some("goal-does-not-exist".to_string());
        let err = store.create_active(input).await;
        assert!(matches!(err, Err(WorkPlanError::Validation(_))));
    }
}
