//! Generic, Eggplan-free persistence seam for repository Plan binding
//! (Eggplan M003, additive storage v68).
//!
//! This module stores *identity and reconciliation bookkeeping only*:
//!
//! - which CodeGG `WorkPlan` mirrors which external repository Plan;
//! - the proven CodeGG-workspace <-> external-repository identity tuple;
//! - the external structural intent digest last observed;
//! - the last external plan revision mirrored into CodeGG;
//! - the stable per-item id map used for cross-store translation.
//!
//! It deliberately contains **no** external crate types, no filesystem or
//! network access, and no assessment authority. Repository I/O,
//! identity proof, cross-store ordering, and translation live in the CodeGG
//! application layer (`src/work_plan_repository_binding.rs`), which is the
//! only production owner of external repository state.
//!
//! The mirror stored next to these rows is ordinary `work_plan`/`work_item`
//! state. It is never an independent source of truth once a binding exists:
//! the authoritative plan lives in the repository, and every mutating or
//! assessment-completion boundary reconciles through this seam.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::model::{WorkItemId, WorkPlanError, WorkPlanId};

/// Additive v68 schema. Safe on existing databases via `IF NOT EXISTS` and
/// leaves every historical unbound plan and work order unchanged.
pub const WORK_PLAN_REPOSITORY_BINDING_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS work_plan_eggplan_binding (
        work_plan_id TEXT PRIMARY KEY REFERENCES work_plan(id) ON DELETE CASCADE,
        workspace_id TEXT NOT NULL,
        codegg_repository_id TEXT NOT NULL,
        eggplan_repository_id TEXT NOT NULL,
        eggplan_plan_id TEXT NOT NULL,
        last_seen_plan_revision INTEGER NOT NULL CHECK (last_seen_plan_revision >= 0),
        intent_digest TEXT NOT NULL,
        projection_digest TEXT NOT NULL,
        binding_state TEXT NOT NULL
            CHECK (binding_state IN ('synced','needs_reconcile','conflict','released')),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        released_at INTEGER,
        CHECK (length(workspace_id) > 0 AND length(workspace_id) <= 256),
        CHECK (length(codegg_repository_id) > 0 AND length(codegg_repository_id) <= 256),
        CHECK (length(eggplan_repository_id) > 0 AND length(eggplan_repository_id) <= 256),
        CHECK (length(eggplan_plan_id) > 0 AND length(eggplan_plan_id) <= 96),
        CHECK (length(intent_digest) > 0 AND length(intent_digest) <= 128),
        CHECK (length(projection_digest) > 0 AND length(projection_digest) <= 128)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS work_plan_eggplan_item_binding (
        work_plan_id TEXT NOT NULL REFERENCES work_plan(id) ON DELETE CASCADE,
        work_item_id TEXT NOT NULL REFERENCES work_item(id) ON DELETE CASCADE,
        eggplan_item_id TEXT NOT NULL,
        UNIQUE (work_plan_id, work_item_id),
        UNIQUE (work_plan_id, eggplan_item_id),
        CHECK (length(eggplan_item_id) > 0 AND length(eggplan_item_id) <= 96)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS work_order_eggplan_binding (
        work_order_id TEXT PRIMARY KEY,
        codegg_repository_id TEXT NOT NULL,
        eggplan_repository_id TEXT NOT NULL,
        eggplan_plan_id TEXT NOT NULL,
        intent_digest TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        CHECK (length(codegg_repository_id) > 0 AND length(codegg_repository_id) <= 256),
        CHECK (length(eggplan_repository_id) > 0 AND length(eggplan_repository_id) <= 256),
        CHECK (length(eggplan_plan_id) > 0 AND length(eggplan_plan_id) <= 96),
        CHECK (length(intent_digest) > 0 AND length(intent_digest) <= 128)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_work_plan_eggplan_live ON work_plan_eggplan_binding(eggplan_repository_id, eggplan_plan_id) WHERE binding_state != 'released'",
    "CREATE INDEX IF NOT EXISTS idx_work_plan_eggplan_item ON work_plan_eggplan_item_binding(work_item_id)",
    "CREATE INDEX IF NOT EXISTS idx_work_order_eggplan_plan ON work_order_eggplan_binding(eggplan_repository_id, eggplan_plan_id)",
];

/// Reconciliation state of one bound CodeGG mirror.
///
/// `synced` and `needs_reconcile` remain live; `conflict` is fail-closed;
/// `released` is terminal history and no longer blocks another live binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryBindingState {
    Synced,
    NeedsReconcile,
    Conflict,
    Released,
}

impl RepositoryBindingState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::NeedsReconcile => "needs_reconcile",
            Self::Conflict => "conflict",
            Self::Released => "released",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "synced" => Some(Self::Synced),
            "needs_reconcile" => Some(Self::NeedsReconcile),
            "conflict" => Some(Self::Conflict),
            "released" => Some(Self::Released),
            _ => None,
        }
    }

    /// `true` while the binding still owns the CodeGG mirror. Only a
    /// released binding stops being live authority.
    pub const fn is_live(self) -> bool {
        !matches!(self, Self::Released)
    }
}

/// One durable CodeGG-mirror <-> repository-Plan binding row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryPlanBinding {
    pub work_plan_id: WorkPlanId,
    pub workspace_id: String,
    /// CodeGG project repository identity, resolved through
    /// `ProjectStorage` at binding time. Never overwritten by the external
    /// repository id.
    pub codegg_repository_id: String,
    /// Proven external repository identity. Distinct namespace from
    /// `codegg_repository_id`; translation is only legal through the proven
    /// binding tuple.
    pub eggplan_repository_id: String,
    pub eggplan_plan_id: String,
    pub last_seen_plan_revision: i64,
    pub intent_digest: String,
    pub projection_digest: String,
    pub binding_state: RepositoryBindingState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
}

/// One durable item identity map entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryItemBinding {
    pub work_plan_id: WorkPlanId,
    pub work_item_id: WorkItemId,
    pub eggplan_item_id: String,
}

/// One host-authored work-order binding *request*.
///
/// This row is a validated request, never scheduler authority: occurrence
/// release, materialization, and scheduling stay CodeGG-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryWorkOrderBinding {
    pub work_order_id: String,
    pub codegg_repository_id: String,
    pub eggplan_repository_id: String,
    pub eggplan_plan_id: String,
    pub intent_digest: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct BindingRow {
    work_plan_id: String,
    workspace_id: String,
    codegg_repository_id: String,
    eggplan_repository_id: String,
    eggplan_plan_id: String,
    last_seen_plan_revision: i64,
    intent_digest: String,
    projection_digest: String,
    binding_state: String,
    created_at: i64,
    updated_at: i64,
    released_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct ItemBindingRow {
    work_plan_id: String,
    work_item_id: String,
    eggplan_item_id: String,
}

#[derive(sqlx::FromRow)]
struct WorkOrderBindingRow {
    work_order_id: String,
    codegg_repository_id: String,
    eggplan_repository_id: String,
    eggplan_plan_id: String,
    intent_digest: String,
    created_at: i64,
}

fn millis_to_datetime(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(value).unwrap_or_else(Utc::now)
}

fn datetime_to_millis(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

fn row_to_binding(row: BindingRow) -> Result<RepositoryPlanBinding, WorkPlanError> {
    let state = RepositoryBindingState::parse(&row.binding_state).ok_or_else(|| {
        WorkPlanError::Storage(format!(
            "unknown repository binding state {:?}",
            row.binding_state
        ))
    })?;
    Ok(RepositoryPlanBinding {
        work_plan_id: WorkPlanId(row.work_plan_id),
        workspace_id: row.workspace_id,
        codegg_repository_id: row.codegg_repository_id,
        eggplan_repository_id: row.eggplan_repository_id,
        eggplan_plan_id: row.eggplan_plan_id,
        last_seen_plan_revision: row.last_seen_plan_revision,
        intent_digest: row.intent_digest,
        projection_digest: row.projection_digest,
        binding_state: state,
        created_at: millis_to_datetime(row.created_at),
        updated_at: millis_to_datetime(row.updated_at),
        released_at: row.released_at.map(millis_to_datetime),
    })
}

fn row_to_item(row: ItemBindingRow) -> Result<RepositoryItemBinding, WorkPlanError> {
    Ok(RepositoryItemBinding {
        work_plan_id: WorkPlanId(row.work_plan_id),
        work_item_id: WorkItemId(row.work_item_id),
        eggplan_item_id: row.eggplan_item_id,
    })
}

fn row_to_work_order(
    row: WorkOrderBindingRow,
) -> Result<RepositoryWorkOrderBinding, WorkPlanError> {
    Ok(RepositoryWorkOrderBinding {
        work_order_id: row.work_order_id,
        codegg_repository_id: row.codegg_repository_id,
        eggplan_repository_id: row.eggplan_repository_id,
        eggplan_plan_id: row.eggplan_plan_id,
        intent_digest: row.intent_digest,
        created_at: millis_to_datetime(row.created_at),
    })
}

const MAX_ID_CHARS: usize = 96;
const MAX_DIGEST_CHARS: usize = 128;

/// The single persistence owner for repository binding bookkeeping.
///
/// Every method is a plain SQLite operation; no external store handle is
/// accepted, cached, or constructed here.
#[derive(Clone)]
pub struct RepositoryBindingStore {
    pool: SqlitePool,
}

impl RepositoryBindingStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// `true` when the v68 binding tables exist in this database.
    ///
    /// A catalog that has not been migrated yet has no bindings; read paths
    /// must treat that exactly like "unbound" so pre-M003 behavior is
    /// preserved instead of hard-failing.
    pub async fn schema_available(pool: &SqlitePool) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'work_plan_eggplan_binding'",
        )
        .fetch_one(pool)
        .await
        .map(|count| count > 0)
        .unwrap_or(false)
    }

    /// Persist a new mirror + binding + item map atomically.
    ///
    /// The caller has already written the `work_plan`/`work_item` mirror
    /// rows; this transaction only records the binding. The one-live-binding
    /// invariant is enforced here, so a duplicate attempt fails closed
    /// instead of producing two competing authorities.
    pub async fn create_binding(
        &self,
        binding: &RepositoryPlanBinding,
        items: &[RepositoryItemBinding],
    ) -> Result<(), WorkPlanError> {
        for item in items {
            if item.work_plan_id != binding.work_plan_id {
                return Err(WorkPlanError::Validation(
                    "item binding belongs to another plan".to_string(),
                ));
            }
        }
        if items
            .iter()
            .map(|item| item.eggplan_item_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != items.len()
        {
            return Err(WorkPlanError::Validation(
                "duplicate repository item id in one binding".to_string(),
            ));
        }
        let now = datetime_to_millis(binding.created_at);
        let mut tx = self.pool.begin().await?;
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT work_plan_id FROM work_plan_eggplan_binding \
             WHERE eggplan_repository_id = ?1 AND eggplan_plan_id = ?2 \
             AND workspace_id = ?3 AND binding_state != 'released' LIMIT 1",
        )
        .bind(&binding.eggplan_repository_id)
        .bind(&binding.eggplan_plan_id)
        .bind(&binding.workspace_id)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_some() {
            return Err(WorkPlanError::ScopeMismatch(format!(
                "repository plan {} is already bound by another live CodeGG work plan",
                binding.eggplan_plan_id
            )));
        }
        sqlx::query(
            "INSERT INTO work_plan_eggplan_binding \
             (work_plan_id, workspace_id, codegg_repository_id, eggplan_repository_id, \
              eggplan_plan_id, last_seen_plan_revision, intent_digest, projection_digest, \
              binding_state, created_at, updated_at, released_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, NULL)",
        )
        .bind(binding.work_plan_id.as_str())
        .bind(&binding.workspace_id)
        .bind(&binding.codegg_repository_id)
        .bind(&binding.eggplan_repository_id)
        .bind(&binding.eggplan_plan_id)
        .bind(binding.last_seen_plan_revision)
        .bind(&binding.intent_digest)
        .bind(&binding.projection_digest)
        .bind(binding.binding_state.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        for item in items {
            sqlx::query(
                "INSERT INTO work_plan_eggplan_item_binding \
                 (work_plan_id, work_item_id, eggplan_item_id) VALUES (?1, ?2, ?3)",
            )
            .bind(item.work_plan_id.as_str())
            .bind(item.work_item_id.as_str())
            .bind(&item.eggplan_item_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_binding(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Option<RepositoryPlanBinding>, WorkPlanError> {
        let row: Option<BindingRow> =
            sqlx::query_as("SELECT * FROM work_plan_eggplan_binding WHERE work_plan_id = ?1")
                .bind(work_plan_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_binding).transpose()
    }

    /// The live binding for one CodeGG plan, if any. Released rows are
    /// history and never returned.
    pub async fn live_binding(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Option<RepositoryPlanBinding>, WorkPlanError> {
        Ok(self
            .get_binding(work_plan_id)
            .await?
            .filter(|binding| binding.binding_state.is_live()))
    }

    /// Find any other live CodeGG plan bound to the same repository Plan.
    pub async fn other_live_binding_for_plan(
        &self,
        workspace_id: &str,
        eggplan_repository_id: &str,
        eggplan_plan_id: &str,
        exclude: &WorkPlanId,
    ) -> Result<Option<RepositoryPlanBinding>, WorkPlanError> {
        let row: Option<BindingRow> = sqlx::query_as(
            "SELECT * FROM work_plan_eggplan_binding \
             WHERE workspace_id = ?1 AND eggplan_repository_id = ?2 AND eggplan_plan_id = ?3 \
             AND binding_state != 'released' AND work_plan_id != ?4 LIMIT 1",
        )
        .bind(workspace_id)
        .bind(eggplan_repository_id)
        .bind(eggplan_plan_id)
        .bind(exclude.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_binding).transpose()
    }

    /// The single live binding of one repository Plan in one workspace, if
    /// any. Used to reject a duplicate live binding before the mirror rows
    /// exist.
    pub async fn live_binding_for_repository_plan(
        &self,
        workspace_id: &str,
        eggplan_repository_id: &str,
        eggplan_plan_id: &str,
    ) -> Result<Option<RepositoryPlanBinding>, WorkPlanError> {
        let row: Option<BindingRow> = sqlx::query_as(
            "SELECT * FROM work_plan_eggplan_binding \
             WHERE workspace_id = ?1 AND eggplan_repository_id = ?2 AND eggplan_plan_id = ?3 \
             AND binding_state != 'released' LIMIT 1",
        )
        .bind(workspace_id)
        .bind(eggplan_repository_id)
        .bind(eggplan_plan_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_binding).transpose()
    }

    pub async fn list_item_bindings(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<Vec<RepositoryItemBinding>, WorkPlanError> {
        let rows: Vec<ItemBindingRow> = sqlx::query_as(
            "SELECT * FROM work_plan_eggplan_item_binding WHERE work_plan_id = ?1 \
             ORDER BY work_item_id",
        )
        .bind(work_plan_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_item).collect()
    }

    pub async fn item_binding_for_work_item(
        &self,
        work_item_id: &WorkItemId,
    ) -> Result<Option<RepositoryItemBinding>, WorkPlanError> {
        let row: Option<ItemBindingRow> =
            sqlx::query_as("SELECT * FROM work_plan_eggplan_item_binding WHERE work_item_id = ?1")
                .bind(work_item_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_item).transpose()
    }

    /// Advance the observed repository revision/digests and set the state in
    /// one statement. Callers use this for every reconcile outcome; they
    /// never write partial binding state by hand.
    pub async fn record_reconcile(
        &self,
        work_plan_id: &WorkPlanId,
        last_seen_plan_revision: i64,
        intent_digest: &str,
        projection_digest: &str,
        state: RepositoryBindingState,
    ) -> Result<(), WorkPlanError> {
        if last_seen_plan_revision < 0 {
            return Err(WorkPlanError::Validation(
                "repository plan revision cannot be negative".to_string(),
            ));
        }
        if intent_digest.is_empty()
            || intent_digest.len() > MAX_DIGEST_CHARS
            || projection_digest.is_empty()
            || projection_digest.len() > MAX_DIGEST_CHARS
        {
            return Err(WorkPlanError::Validation(
                "binding digests are required and bounded".to_string(),
            ));
        }
        let now = datetime_to_millis(Utc::now());
        let released_at = match state {
            RepositoryBindingState::Released => Some(now),
            _ => None,
        };
        let result = sqlx::query(
            "UPDATE work_plan_eggplan_binding \
             SET last_seen_plan_revision = ?1, intent_digest = ?2, projection_digest = ?3, \
                 binding_state = ?4, updated_at = ?5, released_at = ?6 \
             WHERE work_plan_id = ?7",
        )
        .bind(last_seen_plan_revision)
        .bind(intent_digest)
        .bind(projection_digest)
        .bind(state.as_str())
        .bind(now)
        .bind(released_at)
        .bind(work_plan_id.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::NotFound(work_plan_id.as_str().to_string()));
        }
        Ok(())
    }

    /// Move a live binding to `conflict` while preserving the last proven
    /// digests. Conflict is fail-closed: it never rewrites the identity
    /// tuple and never releases the binding.
    pub async fn mark_conflict(&self, work_plan_id: &WorkPlanId) -> Result<(), WorkPlanError> {
        let result = sqlx::query(
            "UPDATE work_plan_eggplan_binding SET binding_state = 'conflict', updated_at = ?1 \
             WHERE work_plan_id = ?2 AND binding_state != 'released'",
        )
        .bind(datetime_to_millis(Utc::now()))
        .bind(work_plan_id.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::NotFound(work_plan_id.as_str().to_string()));
        }
        Ok(())
    }

    /// Mark a binding `needs_reconcile` after an interrupted cross-store
    /// write. Never reconstructs repository state; only flags it.
    pub async fn mark_needs_reconcile(
        &self,
        work_plan_id: &WorkPlanId,
    ) -> Result<(), WorkPlanError> {
        let result = sqlx::query(
            "UPDATE work_plan_eggplan_binding \
             SET binding_state = 'needs_reconcile', updated_at = ?1 \
             WHERE work_plan_id = ?2 AND binding_state = 'synced'",
        )
        .bind(datetime_to_millis(Utc::now()))
        .bind(work_plan_id.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::NotFound(work_plan_id.as_str().to_string()));
        }
        Ok(())
    }

    /// Terminalize the binding after a guarded repository closure or
    /// repository-side cancellation. Released bindings no longer block a
    /// future live binding of the same repository Plan.
    pub async fn release(&self, work_plan_id: &WorkPlanId) -> Result<(), WorkPlanError> {
        self.record_reconcile_state_only(work_plan_id, RepositoryBindingState::Released)
            .await
    }

    async fn record_reconcile_state_only(
        &self,
        work_plan_id: &WorkPlanId,
        state: RepositoryBindingState,
    ) -> Result<(), WorkPlanError> {
        let now = datetime_to_millis(Utc::now());
        let released_at = match state {
            RepositoryBindingState::Released => Some(now),
            _ => None,
        };
        let result = sqlx::query(
            "UPDATE work_plan_eggplan_binding \
             SET binding_state = ?1, updated_at = ?2, released_at = ?3 \
             WHERE work_plan_id = ?4",
        )
        .bind(state.as_str())
        .bind(now)
        .bind(released_at)
        .bind(work_plan_id.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(WorkPlanError::NotFound(work_plan_id.as_str().to_string()));
        }
        Ok(())
    }

    /// Record a host-authorized work-order binding request.
    pub async fn create_work_order_binding(
        &self,
        request: &RepositoryWorkOrderBinding,
    ) -> Result<(), WorkPlanError> {
        if request.eggplan_plan_id.is_empty() || request.eggplan_plan_id.len() > MAX_ID_CHARS {
            return Err(WorkPlanError::Validation(
                "repository plan id is required and bounded".to_string(),
            ));
        }
        sqlx::query(
            "INSERT INTO work_order_eggplan_binding \
             (work_order_id, codegg_repository_id, eggplan_repository_id, eggplan_plan_id, \
              intent_digest, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(work_order_id) DO UPDATE SET \
                codegg_repository_id = excluded.codegg_repository_id, \
                eggplan_repository_id = excluded.eggplan_repository_id, \
                eggplan_plan_id = excluded.eggplan_plan_id, \
                intent_digest = excluded.intent_digest",
        )
        .bind(&request.work_order_id)
        .bind(&request.codegg_repository_id)
        .bind(&request.eggplan_repository_id)
        .bind(&request.eggplan_plan_id)
        .bind(&request.intent_digest)
        .bind(datetime_to_millis(request.created_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_work_order_binding(
        &self,
        work_order_id: &str,
    ) -> Result<Option<RepositoryWorkOrderBinding>, WorkPlanError> {
        let row: Option<WorkOrderBindingRow> =
            sqlx::query_as("SELECT * FROM work_order_eggplan_binding WHERE work_order_id = ?1")
                .bind(work_order_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(row_to_work_order).transpose()
    }

    pub async fn clear_work_order_binding(&self, work_order_id: &str) -> Result<(), WorkPlanError> {
        sqlx::query("DELETE FROM work_order_eggplan_binding WHERE work_order_id = ?1")
            .bind(work_order_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn temp_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    async fn seed_plan(pool: &SqlitePool, plan_id: &str) {
        sqlx::query(
            "INSERT INTO work_plan (id, revision, session_id, project_id, objective, \
             objective_digest, origin_provenance, status, created_at, updated_at) \
             VALUES (?1, 0, 's1', 'p1', 'objective', 'sha256:0', 'test', 'active', 1, 1)",
        )
        .bind(plan_id)
        .execute(pool)
        .await
        .expect("plan");
    }

    async fn seed_item(pool: &SqlitePool, plan_id: &str, item_id: &str) {
        sqlx::query(
            "INSERT INTO work_item (id, plan_id, revision, position, status, description, \
             created_at, updated_at) VALUES (?1, ?2, 0, 0, 'pending', 'd', 1, 1)",
        )
        .bind(item_id)
        .bind(plan_id)
        .execute(pool)
        .await
        .expect("item");
    }

    fn binding(plan_id: &str) -> RepositoryPlanBinding {
        RepositoryPlanBinding {
            work_plan_id: WorkPlanId(plan_id.to_string()),
            workspace_id: "codegg-workspace:w1".to_string(),
            codegg_repository_id: "repo_1".to_string(),
            eggplan_repository_id: "epr_1".to_string(),
            eggplan_plan_id: "epl_1".to_string(),
            last_seen_plan_revision: 3,
            intent_digest: "sha256:intent".to_string(),
            projection_digest: "sha256:projection".to_string(),
            binding_state: RepositoryBindingState::Synced,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            released_at: None,
        }
    }

    fn item_binding(plan_id: &str, item_id: &str, eggplan_item_id: &str) -> RepositoryItemBinding {
        RepositoryItemBinding {
            work_plan_id: WorkPlanId(plan_id.to_string()),
            work_item_id: WorkItemId(item_id.to_string()),
            eggplan_item_id: eggplan_item_id.to_string(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn binding_and_item_map_survive_restart() {
        let pool = temp_pool().await;
        seed_plan(&pool, "wp_1").await;
        seed_item(&pool, "wp_1", "wi_1").await;
        let store = RepositoryBindingStore::new(pool.clone());
        store
            .create_binding(&binding("wp_1"), &[item_binding("wp_1", "wi_1", "epi_1")])
            .await
            .expect("create");

        // A fresh store handle models a daemon restart: the rows are the
        // authority, not the in-memory handle.
        let reopened = RepositoryBindingStore::new(pool);
        let loaded = reopened
            .get_binding(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("get")
            .expect("row");
        assert_eq!(loaded.eggplan_repository_id, "epr_1");
        assert_eq!(loaded.codegg_repository_id, "repo_1");
        assert_eq!(loaded.last_seen_plan_revision, 3);
        assert_eq!(loaded.binding_state, RepositoryBindingState::Synced);
        let items = reopened
            .list_item_bindings(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].eggplan_item_id, "epi_1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_live_binding_is_rejected() {
        let pool = temp_pool().await;
        seed_plan(&pool, "wp_1").await;
        seed_plan(&pool, "wp_2").await;
        let store = RepositoryBindingStore::new(pool);
        store
            .create_binding(&binding("wp_1"), &[])
            .await
            .expect("first");
        let mut second = binding("wp_2");
        second.work_plan_id = WorkPlanId("wp_2".to_string());
        let error = store
            .create_binding(&second, &[])
            .await
            .expect_err("duplicate");
        assert!(matches!(error, WorkPlanError::ScopeMismatch(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn released_binding_frees_the_repository_plan() {
        let pool = temp_pool().await;
        seed_plan(&pool, "wp_1").await;
        seed_plan(&pool, "wp_2").await;
        let store = RepositoryBindingStore::new(pool);
        store
            .create_binding(&binding("wp_1"), &[])
            .await
            .expect("a");
        store
            .release(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("release");
        let mut second = binding("wp_2");
        second.work_plan_id = WorkPlanId("wp_2".to_string());
        store.create_binding(&second, &[]).await.expect("rebinding");
        assert!(store
            .live_binding(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("live")
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn conflict_and_reconcile_flags_preserve_identity() {
        let pool = temp_pool().await;
        seed_plan(&pool, "wp_1").await;
        let store = RepositoryBindingStore::new(pool);
        store
            .create_binding(&binding("wp_1"), &[])
            .await
            .expect("a");
        store
            .mark_needs_reconcile(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("needs");
        store
            .mark_conflict(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("conflict");
        let loaded = store
            .get_binding(&WorkPlanId("wp_1".to_string()))
            .await
            .expect("get")
            .expect("row");
        assert_eq!(loaded.binding_state, RepositoryBindingState::Conflict);
        assert_eq!(loaded.intent_digest, "sha256:intent");
        assert_eq!(loaded.eggplan_repository_id, "epr_1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn work_order_binding_request_round_trips() {
        let pool = temp_pool().await;
        let store = RepositoryBindingStore::new(pool);
        let request = RepositoryWorkOrderBinding {
            work_order_id: "wo_1".to_string(),
            codegg_repository_id: "repo_1".to_string(),
            eggplan_repository_id: "epr_1".to_string(),
            eggplan_plan_id: "epl_1".to_string(),
            intent_digest: "sha256:intent".to_string(),
            created_at: Utc::now(),
        };
        store
            .create_work_order_binding(&request)
            .await
            .expect("create");
        let loaded = store
            .get_work_order_binding("wo_1")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(loaded.eggplan_plan_id, "epl_1");
        store.clear_work_order_binding("wo_1").await.expect("clear");
        assert!(store
            .get_work_order_binding("wo_1")
            .await
            .expect("get")
            .is_none());
    }
}
