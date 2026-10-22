//! Durable, generation-fenced checkout writer permits (M005 corrective C002).
//!
//! # What a permit is
//!
//! A **checkout writer permit** is the single CodeGG-held token for *writing* to
//! one managed worktree checkout. It is deliberately **not**:
//!
//! - a model-visible tool, and not a tool-adjacent capability at all;
//! - an admission scheduler, or a job-queue slot;
//! - a project-level global mutex — two different checkouts write in parallel;
//! - a claim that any particular writer has been mediated. C002 builds the
//!   *authority*; C003 and C004 are what route writers through it.
//!
//! # What it is not, precisely
//!
//! The permit is **not** the worktree lease. The lease says *who owns the
//! checkout*; the permit says *who may write to it right now*. A checkout can be
//! owned by one run and written by that run's writer; the two identities are
//! tracked separately, because ownership outlives a single write.
//!
//! # The state machine
//!
//! ```text
//!                 acquire
//!      Available ─────────► Held
//!          ▲                  │ │
//!          │                  │ │ begin_release
//!          │  complete_release│ ▼
//!          │   (settled)   Releasing
//!          │                  │
//!          │                  │ complete_release (unproven)
//!          │                  ▼
//!          └────────────── Quarantined
//!                              ▲
//!         quarantine (any state) ─┘
//! ```
//!
//! `Releasing` exists so that a crash *between* the holder declaring it is done
//! and the permit actually being freed is distinguishable from a holder that is
//! still working. Both are unsafe, but only one of them is *known* to be
//! mid-release, and reconciliation reports them differently.
//!
//! `Quarantined` is terminal until an operator releases it explicitly. A
//! quarantine is never cleared by a timer: **a lease deadline is never enough to
//! free a live writer**, and neither is a restart.
//!
//! # The fence
//!
//! Every mutating operation presents three things that must all match the
//! persisted row:
//!
//! 1. the **owner token** — a non-duplicable, identity-bound secret minted at
//!    acquisition, so "the holder" is never ambiguous;
//! 2. the **revision** — a monotonic counter bumped on every state change, so a
//!    stale actor cannot act on a row it has not re-read;
//! 3. the **lease generation** — the worktree lease generation the permit was
//!    granted under, so a holder from a superseded lease cannot mutate the row
//!    after the checkout was re-leased.
//!
//! Together these defeat ABA: a row that was released and re-acquired has a
//! new revision, a new owner token, and (if the lease moved) a new generation,
//! so a stale actor's request matches none of them.
//!
//! # Idempotency
//!
//! An acquisition carries an idempotency key. While that acquisition is live,
//! retrying it with the *same* payload returns the existing permit, and
//! retrying it with a *different* payload is refused as a conflict. The binding
//! is scoped to the live acquisition: a key is released when the permit is
//! released or quarantined, and a later acquisition may reuse it.
//!
//! # Quiescence
//!
//! A release is only permitted with **positive** evidence that the caller's
//! effects settled. Evidence that is merely absent is not evidence of
//! quiescence, and unproven evidence routes the permit to `Quarantined` rather
//! than freeing it.

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;

use crate::identity::{
    AgentRunGroupId, AgentRunId, PrincipalId, ProjectId, RepositoryId, WorkspaceId, WorktreeId,
};
use crate::storage::immediate::ImmediateWrite;

/// Bound on the stored operation label. The label is a closed enum, so this only
/// guards against a future free-text field being smuggled in under the same
/// column.
pub const MAX_WRITER_PERMIT_OPERATION_BYTES: usize = 64;

/// Bound on the stored quarantine detail. Details are diagnostic, never
/// load-bearing, and are truncated on write.
pub const MAX_WRITER_PERMIT_DETAIL_BYTES: usize = 512;

/// Bound on the stored idempotency key.
pub const MAX_WRITER_PERMIT_IDEMPOTENCY_BYTES: usize = 128;

/// Bound on the stored execution-attempt label.
pub const MAX_WRITER_PERMIT_ATTEMPT_BYTES: usize = 128;

/// Bound on the stored scope fingerprint.
pub const MAX_WRITER_PERMIT_FINGERPRINT_BYTES: usize = 1024;

/// Default permit lease. The lease is a **liveness hint for the holder**, never a
/// release authority: an expired lease quarantines the permit, it does not free
/// it.
pub const DEFAULT_WRITER_PERMIT_LEASE_MS: i64 = 15 * 60 * 1000;

/// Upper bound on permits a single project may hold at once. A permit row exists
/// per managed worktree, so this bounds the number of managed worktrees a
/// project may have outstanding, not the number of writers.
pub const MAX_WRITER_PERMITS_PER_PROJECT: i64 = 4096;

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// The typed identity tuple a permit is bound to.
///
/// Every field is resolved by the daemon from authoritative stores — never from
/// a model-supplied value. In particular `lease_generation` is the worktree's
/// *current* lease generation, read at acquisition time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterPermitScope {
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    pub worktree_id: WorktreeId,
    /// The worktree lease generation this permit is granted under.
    pub lease_generation: u64,
    /// The agent run group whose reservation covers this checkout, when the
    /// write happens under one.
    pub group_id: Option<AgentRunGroupId>,
    /// The authenticated principal that owns the write.
    pub principal_id: PrincipalId,
    /// The agent run executing the write, when there is one.
    pub agent_run_id: Option<AgentRunId>,
    /// The execution attempt within the run, when there is one.
    pub attempt_id: Option<String>,
}

impl WriterPermitScope {
    /// A stable fingerprint of the scope, used to detect an idempotency key
    /// being reused with an altered payload.
    ///
    /// The fingerprint is over *identity*, not over the operation: a retry that
    /// changes the operation is a different request, while a retry that merely
    /// re-states the same identity is the same request.
    pub fn fingerprint(&self) -> String {
        format!(
            "project={}|repo={}|workspace={}|worktree={}|generation={}|group={}|principal={}|run={}|attempt={}",
            self.project_id.as_str(),
            self.repository_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or("-"),
            self.workspace_id.as_str(),
            self.worktree_id.as_str(),
            self.lease_generation,
            self.group_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or("-"),
            self.principal_id.as_str(),
            self.agent_run_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or("-"),
            self.attempt_id.as_deref().unwrap_or("-"),
        )
    }
}

// ---------------------------------------------------------------------------
// Operation
// ---------------------------------------------------------------------------

/// The class of effect a permit is being held for.
///
/// This is a closed enum on purpose: the permit must not become a channel for
/// free text, and the operation label is what later milestones route on. It is
/// the *class* of effect, not the identity of the writer — the writer's
/// identity is the owner tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterPermitOperation {
    /// A native file write inside the checkout.
    NativeFileWrite,
    /// A native Git mutation inside the checkout.
    NativeGitMutation,
    /// A process spawned against the checkout.
    ProcessSpawn,
    /// A managed service that writes into the checkout.
    ManagedServiceWrite,
    /// A write to project-local state inside the checkout.
    ProjectLocalStateWrite,
}

impl WriterPermitOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeFileWrite => "native_file_write",
            Self::NativeGitMutation => "native_git_mutation",
            Self::ProcessSpawn => "process_spawn",
            Self::ManagedServiceWrite => "managed_service_write",
            Self::ProjectLocalStateWrite => "project_local_state_write",
        }
    }

    pub fn parse(value: &str) -> Result<Self, WriterPermitError> {
        match value {
            "native_file_write" => Ok(Self::NativeFileWrite),
            "native_git_mutation" => Ok(Self::NativeGitMutation),
            "process_spawn" => Ok(Self::ProcessSpawn),
            "managed_service_write" => Ok(Self::ManagedServiceWrite),
            "project_local_state_write" => Ok(Self::ProjectLocalStateWrite),
            _ => Err(WriterPermitError::CorruptState),
        }
    }
}

// ---------------------------------------------------------------------------
// Owner
// ---------------------------------------------------------------------------

/// The identity-bound holder of a permit.
///
/// The `token` is the non-duplicable part: it is minted by the store at
/// acquisition and is never reused, so two holders can never both present a
/// valid token for the same row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterPermitOwner {
    pub principal_id: PrincipalId,
    pub agent_run_id: Option<AgentRunId>,
    pub group_id: Option<AgentRunGroupId>,
    pub attempt_id: Option<String>,
    /// Non-duplicable, identity-bound possession token.
    pub token: String,
}

impl WriterPermitOwner {
    /// Whether the presented owner is the same identity as `other`, ignoring the
    /// token. Used to distinguish "the wrong actor" from "a stale copy of the
    /// right actor".
    pub fn same_identity(&self, other: &WriterPermitOwner) -> bool {
        self.principal_id == other.principal_id
            && self.agent_run_id == other.agent_run_id
            && self.group_id == other.group_id
            && self.attempt_id == other.attempt_id
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The permit's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterPermitState {
    /// Nobody holds the permit; it may be acquired.
    Available,
    /// A holder has the permit and has not declared it done.
    Held,
    /// The holder has declared it done; the permit is being freed.
    Releasing,
    /// The permit is parked. Only an explicit operator release frees it.
    Quarantined,
}

impl WriterPermitState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Held => "held",
            Self::Releasing => "releasing",
            Self::Quarantined => "quarantined",
        }
    }

    pub fn parse(value: &str) -> Result<Self, WriterPermitError> {
        match value {
            "available" => Ok(Self::Available),
            "held" => Ok(Self::Held),
            "releasing" => Ok(Self::Releasing),
            "quarantined" => Ok(Self::Quarantined),
            _ => Err(WriterPermitError::CorruptState),
        }
    }

    /// Whether the state carries a holder.
    pub fn holds_owner(self) -> bool {
        matches!(self, Self::Held | Self::Releasing)
    }
}

/// Why a permit was quarantined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterPermitQuarantineReason {
    /// The daemon restarted while the permit was live. The holder is gone.
    DaemonRestart,
    /// The holder reported the operation as aborted.
    AbortedOperation,
    /// The holder could not show that its effects completed.
    IncompleteEffects,
    /// The holder was a process-backed caller and its process tree could not be
    /// proven quiet.
    ProcessTreeUncertain,
    /// The worktree lease the permit was granted under is no longer current.
    LeaseExpired,
    /// The checkout was re-leased to a different owner.
    OwnershipChanged,
    /// A second writer claimed the same checkout.
    ConflictingEffect,
    /// An operator parked the permit deliberately.
    OperatorHold,
}

impl WriterPermitQuarantineReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DaemonRestart => "daemon_restart",
            Self::AbortedOperation => "aborted_operation",
            Self::IncompleteEffects => "incomplete_effects",
            Self::ProcessTreeUncertain => "process_tree_uncertain",
            Self::LeaseExpired => "lease_expired",
            Self::OwnershipChanged => "ownership_changed",
            Self::ConflictingEffect => "conflicting_effect",
            Self::OperatorHold => "operator_hold",
        }
    }

    pub fn parse(value: &str) -> Result<Self, WriterPermitError> {
        match value {
            "daemon_restart" => Ok(Self::DaemonRestart),
            "aborted_operation" => Ok(Self::AbortedOperation),
            "incomplete_effects" => Ok(Self::IncompleteEffects),
            "process_tree_uncertain" => Ok(Self::ProcessTreeUncertain),
            "lease_expired" => Ok(Self::LeaseExpired),
            "ownership_changed" => Ok(Self::OwnershipChanged),
            "conflicting_effect" => Ok(Self::ConflictingEffect),
            "operator_hold" => Ok(Self::OperatorHold),
            _ => Err(WriterPermitError::CorruptState),
        }
    }
}

/// The quarantine record attached to a parked permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterPermitQuarantine {
    pub reason: WriterPermitQuarantineReason,
    /// Bounded, redacted diagnostic detail. Never load-bearing.
    pub detail: String,
    pub quarantined_at_ms: i64,
}

/// The persisted permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterPermitRecord {
    pub worktree_id: WorktreeId,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    /// The lease generation this permit is bound to.
    pub lease_generation: u64,
    pub group_id: Option<AgentRunGroupId>,
    pub state: WriterPermitState,
    pub owner: Option<WriterPermitOwner>,
    pub operation: Option<WriterPermitOperation>,
    /// Monotonic, bumped on every state change.
    pub revision: u64,
    /// The idempotency key of the live acquisition, if any.
    pub idempotency_key: Option<String>,
    /// The scope fingerprint bound to `idempotency_key`.
    pub request_fingerprint: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub quarantine: Option<WriterPermitQuarantine>,
    pub acquired_at_ms: Option<i64>,
    pub updated_at_ms: i64,
}

impl WriterPermitRecord {
    /// Whether this permit is currently held by somebody.
    pub fn is_held(&self) -> bool {
        self.state.holds_owner()
    }

    /// The redacted operator/frontend view.
    ///
    /// The view carries identity and state, never a path, a command, or any
    /// other free text that could carry user content.
    pub fn to_view(&self) -> WriterPermitView {
        WriterPermitView {
            worktree_id: self.worktree_id.to_string(),
            project_id: self.project_id.to_string(),
            workspace_id: self.workspace_id.to_string(),
            repository_id: self.repository_id.as_ref().map(|id| id.to_string()),
            state: self.state.as_str().to_owned(),
            operation: self.operation.map(|op| op.as_str().to_owned()),
            revision: self.revision,
            lease_generation: self.lease_generation,
            group_id: self.group_id.as_ref().map(|id| id.to_string()),
            owner_principal_id: self
                .owner
                .as_ref()
                .map(|owner| owner.principal_id.to_string()),
            owner_agent_run_id: self
                .owner
                .as_ref()
                .and_then(|owner| owner.agent_run_id.as_ref())
                .map(|id| id.to_string()),
            quarantine_reason: self
                .quarantine
                .as_ref()
                .map(|quarantine| quarantine.reason.as_str().to_owned()),
            lease_expires_at_ms: self.lease_expires_at_ms,
            acquired_at_ms: self.acquired_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// The redacted, bounded projection of a permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterPermitView {
    pub worktree_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub repository_id: Option<String>,
    pub state: String,
    pub operation: Option<String>,
    pub revision: u64,
    pub lease_generation: u64,
    pub group_id: Option<String>,
    pub owner_principal_id: Option<String>,
    pub owner_agent_run_id: Option<String>,
    pub quarantine_reason: Option<String>,
    pub lease_expires_at_ms: Option<i64>,
    pub acquired_at_ms: Option<i64>,
    pub updated_at_ms: i64,
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// Positive evidence that a holder's effects have settled.
///
/// Absence of evidence is not evidence: [`QuiescenceEvidence::Unproven`] routes
/// the permit to quarantine rather than freeing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuiescenceEvidence {
    /// The caller's transaction committed or rolled back, and its effects were
    /// observed to have settled.
    Settled,
    /// The operation produced no effect at all, so there is nothing to settle.
    NoEffect,
    /// The caller cannot prove the effects settled.
    Unproven(WriterPermitQuarantineReason),
}

impl QuiescenceEvidence {
    /// Whether this evidence permits the permit to be freed.
    pub fn is_proven(&self) -> bool {
        matches!(self, Self::Settled | Self::NoEffect)
    }

    /// The quarantine reason to use when the evidence is unproven.
    pub fn unproven_reason(&self) -> Option<WriterPermitQuarantineReason> {
        match self {
            Self::Unproven(reason) => Some(*reason),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum WriterPermitError {
    #[error("writer permit storage error: {0}")]
    Storage(String),
    #[error("writer permit not found")]
    NotFound,
    #[error("writer permit is held by another owner")]
    Held,
    #[error("writer permit is quarantined")]
    Quarantined,
    #[error("writer permit owner token does not match the holder")]
    NotOwner,
    #[error("writer permit revision or lease generation is stale")]
    StaleRevision,
    #[error("writer permit idempotency key was reused with a different payload")]
    IdempotencyConflict,
    #[error("writer permit scope does not match the persisted scope")]
    ScopeMismatch,
    #[error("writer permit lease generation is not the current worktree generation")]
    StaleLeaseGeneration,
    #[error("writer permit scope is invalid")]
    InvalidScope,
    #[error("writer permit capacity is exhausted")]
    Capacity,
    #[error("writer permit wait timed out")]
    WaitTimeout,
    #[error("writer permit wait was cancelled")]
    Cancelled,
    #[error("writer permit state transition is not allowed")]
    InvalidTransition,
    #[error("writer permit release evidence was unproven: {0:?}")]
    UnprovenQuiescence(WriterPermitQuarantineReason),
    #[error("writer permit state is corrupt")]
    CorruptState,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Acquire the permit for a checkout.
#[derive(Debug, Clone)]
pub struct AcquireWriterPermit {
    pub scope: WriterPermitScope,
    pub operation: WriterPermitOperation,
    pub idempotency_key: String,
    pub lease_expires_at_ms: Option<i64>,
}

/// Declare the holder done; move the permit to `Releasing`.
#[derive(Debug, Clone)]
pub struct BeginReleaseWriterPermit {
    pub worktree_id: WorktreeId,
    pub owner_token: String,
    pub expected_revision: u64,
    pub expected_lease_generation: u64,
}

/// Free the permit (or hand it to an explicit successor).
#[derive(Debug, Clone)]
pub struct CompleteReleaseWriterPermit {
    pub worktree_id: WorktreeId,
    pub owner_token: String,
    pub expected_revision: u64,
    pub expected_lease_generation: u64,
    pub evidence: QuiescenceEvidence,
    /// An explicit, identity-bound successor. The successor receives a **new**
    /// non-duplicable token; the old one is invalidated by this transition.
    pub transfer: Option<WriterPermitTransfer>,
}

/// The successor of a released permit.
#[derive(Debug, Clone)]
pub struct WriterPermitTransfer {
    pub principal_id: PrincipalId,
    pub agent_run_id: Option<AgentRunId>,
    pub group_id: Option<AgentRunGroupId>,
    pub attempt_id: Option<String>,
    pub operation: WriterPermitOperation,
    pub idempotency_key: String,
    pub lease_expires_at_ms: Option<i64>,
}

/// Park the permit.
#[derive(Debug, Clone)]
pub struct QuarantineWriterPermit {
    pub worktree_id: WorktreeId,
    /// The holder's token, when the holder is quarantining itself. An operator
    /// quarantine passes `None`.
    pub owner_token: Option<String>,
    pub expected_revision: Option<u64>,
    pub reason: WriterPermitQuarantineReason,
    pub detail: String,
}

/// Free a quarantined permit. Operator-only.
#[derive(Debug, Clone)]
pub struct OperatorReleaseWriterPermit {
    pub worktree_id: WorktreeId,
    pub expected_revision: u64,
    pub reason: String,
    pub evidence: QuiescenceEvidence,
}

/// Filter for the operator listing.
#[derive(Debug, Clone, Default)]
pub struct WriterPermitFilter {
    pub project_id: Option<ProjectId>,
    pub state: Option<WriterPermitState>,
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// The durable permit store.
///
/// The store is the **authority**: it owns the state machine, the compare-and-
/// swap, and the quarantine transitions. The daemon adapter above it resolves
/// authorization and does the waiting; it never decides whether a transition is
/// legal.
#[derive(Debug, Clone)]
pub struct WriterPermitStore {
    pool: SqlitePool,
}

impl WriterPermitStore {
    pub fn new(pool: SqlitePool) -> Arc<Self> {
        Arc::new(Self { pool })
    }

    /// The pool this store writes to. Exposed so the daemon can share one
    /// connection pool rather than opening a second database.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Load the permit for a checkout, if one exists.
    pub async fn load(
        &self,
        worktree_id: &WorktreeId,
    ) -> Result<Option<WriterPermitRecord>, WriterPermitError> {
        let sql = select_one_sql();
        let row = sqlx::query(&sql)
            .bind(worktree_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(storage_error)?;
        row.as_ref().map(row_to_record).transpose()
    }

    /// List permits for the operator surface.
    pub async fn list(
        &self,
        filter: &WriterPermitFilter,
    ) -> Result<Vec<WriterPermitRecord>, WriterPermitError> {
        let limit = filter.limit.unwrap_or(256).clamp(1, 4096);
        let rows = match (&filter.project_id, filter.state) {
            (Some(project_id), Some(state)) => sqlx::query(&select_project_state_sql())
                .bind(project_id.as_str())
                .bind(state.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(storage_error)?,
            (Some(project_id), None) => sqlx::query(&select_project_sql())
                .bind(project_id.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(storage_error)?,
            (None, Some(state)) => sqlx::query(&select_state_sql())
                .bind(state.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(storage_error)?,
            (None, None) => sqlx::query(&select_all_sql())
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(storage_error)?,
        };
        rows.iter().map(row_to_record).collect()
    }

    /// Acquire the permit for a checkout.
    ///
    /// The whole decision is taken inside one `BEGIN IMMEDIATE` transaction, so
    /// two racing acquirers cannot both read `available` and both write. The
    /// loser sees the winner's row and gets [`WriterPermitError::Held`].
    pub async fn acquire(
        &self,
        request: AcquireWriterPermit,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        validate_idempotency_key(&request.idempotency_key)?;
        if request.scope.lease_generation == 0 {
            return Err(WriterPermitError::InvalidScope);
        }
        if request.operation.as_str().len() > MAX_WRITER_PERMIT_OPERATION_BYTES {
            return Err(WriterPermitError::InvalidScope);
        }
        if request
            .scope
            .attempt_id
            .as_ref()
            .is_some_and(|attempt| attempt.len() > MAX_WRITER_PERMIT_ATTEMPT_BYTES)
        {
            return Err(WriterPermitError::InvalidScope);
        }
        let fingerprint = request.scope.fingerprint();
        if fingerprint.len() > MAX_WRITER_PERMIT_FINGERPRINT_BYTES {
            return Err(WriterPermitError::InvalidScope);
        }
        let now = now_ms();

        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let existing = load_locked(&mut tx, &request.scope.worktree_id).await?;
        let mut expected_revision: Option<u64> = None;

        if let Some(row) = existing {
            match row.state {
                WriterPermitState::Quarantined => {
                    return Err(WriterPermitError::Quarantined);
                }
                WriterPermitState::Held | WriterPermitState::Releasing => {
                    // An idempotent retry of the *same* live acquisition returns
                    // the existing permit; a retry with an altered payload is a
                    // conflict; anything else is simply "held by someone else".
                    if row.idempotency_key.as_deref() == Some(request.idempotency_key.as_str()) {
                        if row.request_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                            return Ok(row);
                        }
                        return Err(WriterPermitError::IdempotencyConflict);
                    }
                    if row.lease_generation != request.scope.lease_generation {
                        return Err(WriterPermitError::StaleLeaseGeneration);
                    }
                    return Err(WriterPermitError::Held);
                }
                WriterPermitState::Available => {
                    // The revision the row carried when it was read. It goes
                    // into the compare-and-swap's `WHERE` clause, so the
                    // update is a single guarded statement rather than a
                    // decision taken from a read that something else could
                    // have invalidated. `BEGIN IMMEDIATE` already closes the
                    // window; the predicate makes the guard explicit.
                    expected_revision = Some(row.revision);
                }
            }
        }

        // Capacity is checked against the project's live permits.
        let live: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM writer_permit WHERE project_id = ? AND state IN ('held','releasing')",
        )
        .bind(request.scope.project_id.as_str())
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
        if live >= MAX_WRITER_PERMITS_PER_PROJECT {
            return Err(WriterPermitError::Capacity);
        }

        let owner = WriterPermitOwner {
            principal_id: request.scope.principal_id.clone(),
            agent_run_id: request.scope.agent_run_id.clone(),
            group_id: request.scope.group_id.clone(),
            attempt_id: request.scope.attempt_id.clone(),
            token: uuid::Uuid::new_v4().to_string(),
        };
        let lease_expires_at_ms = request
            .lease_expires_at_ms
            .or(Some(now + DEFAULT_WRITER_PERMIT_LEASE_MS));

        let updated = sqlx::query(
            "UPDATE writer_permit SET project_id = ?, repository_id = ?, workspace_id = ?, \
             lease_generation = ?, group_id = ?, state = 'held', owner_principal_id = ?, \
             owner_agent_run_id = ?, owner_group_id = ?, owner_attempt_id = ?, owner_token = ?, \
             operation = ?, revision = revision + 1, idempotency_key = ?, request_fingerprint = ?, \
             lease_expires_at_ms = ?, quarantine_reason = NULL, quarantine_detail = NULL, \
             quarantined_at_ms = NULL, acquired_at_ms = ?, updated_at_ms = ? \
             WHERE worktree_id = ? AND state = 'available' AND revision = ?",
        )
        .bind(request.scope.project_id.as_str())
        .bind(request.scope.repository_id.as_ref().map(|id| id.as_str()))
        .bind(request.scope.workspace_id.as_str())
        .bind(request.scope.lease_generation as i64)
        .bind(request.scope.group_id.as_ref().map(|id| id.as_str()))
        .bind(owner.principal_id.as_str())
        .bind(owner.agent_run_id.as_ref().map(|id| id.as_str()))
        .bind(owner.group_id.as_ref().map(|id| id.as_str()))
        .bind(owner.attempt_id.as_deref())
        .bind(owner.token.as_str())
        .bind(request.operation.as_str())
        .bind(request.idempotency_key.as_str())
        .bind(fingerprint.as_str())
        .bind(lease_expires_at_ms)
        .bind(now)
        .bind(now)
        .bind(request.scope.worktree_id.as_str())
        .bind(expected_revision.unwrap_or(0) as i64)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        let affected = updated.rows_affected();

        if affected == 0 && expected_revision.is_some() {
            // The row was available when it was read and is not available now.
            // Under `BEGIN IMMEDIATE` that cannot happen; the predicate is
            // here so a future change to the transaction shape fails loudly
            // instead of silently overwriting a holder.
            return Err(WriterPermitError::StaleRevision);
        }

        if affected == 0 {
            sqlx::query(
                "INSERT INTO writer_permit (worktree_id, project_id, repository_id, workspace_id, \
                 lease_generation, group_id, state, owner_principal_id, owner_agent_run_id, \
                 owner_group_id, owner_attempt_id, owner_token, operation, revision, \
                 idempotency_key, request_fingerprint, lease_expires_at_ms, acquired_at_ms, \
                 updated_at_ms) \
                 VALUES (?, ?, ?, ?, ?, ?, 'held', ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?)",
            )
            .bind(request.scope.worktree_id.as_str())
            .bind(request.scope.project_id.as_str())
            .bind(request.scope.repository_id.as_ref().map(|id| id.as_str()))
            .bind(request.scope.workspace_id.as_str())
            .bind(request.scope.lease_generation as i64)
            .bind(request.scope.group_id.as_ref().map(|id| id.as_str()))
            .bind(owner.principal_id.as_str())
            .bind(owner.agent_run_id.as_ref().map(|id| id.as_str()))
            .bind(owner.group_id.as_ref().map(|id| id.as_str()))
            .bind(owner.attempt_id.as_deref())
            .bind(owner.token.as_str())
            .bind(request.operation.as_str())
            .bind(request.idempotency_key.as_str())
            .bind(fingerprint.as_str())
            .bind(lease_expires_at_ms)
            .bind(now)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }

        let record = load_locked(&mut tx, &request.scope.worktree_id)
            .await?
            .ok_or(WriterPermitError::NotFound)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }

    /// Move a held permit to `Releasing`.
    pub async fn begin_release(
        &self,
        request: BeginReleaseWriterPermit,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let now = now_ms();
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let row = load_locked(&mut tx, &request.worktree_id).await?;
        let mut record = row.ok_or(WriterPermitError::NotFound)?;
        assert_owner(&record, &request.owner_token)?;
        if record.revision != request.expected_revision
            || record.lease_generation != request.expected_lease_generation
        {
            return Err(WriterPermitError::StaleRevision);
        }
        if record.state != WriterPermitState::Held {
            return Err(WriterPermitError::InvalidTransition);
        }
        let updated = sqlx::query(
            "UPDATE writer_permit SET state = 'releasing', revision = revision + 1, updated_at_ms = ? \
             WHERE worktree_id = ? AND revision = ? AND lease_generation = ? AND state = 'held' \
             AND owner_token = ?",
        )
        .bind(now)
        .bind(request.worktree_id.as_str())
        .bind(request.expected_revision as i64)
        .bind(request.expected_lease_generation as i64)
        .bind(request.owner_token.as_str())
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            return Err(WriterPermitError::StaleRevision);
        }
        record.state = WriterPermitState::Releasing;
        record.revision += 1;
        record.updated_at_ms = now;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }

    /// Free the permit, hand it to a successor, or quarantine it.
    ///
    /// The evidence decides. Proven evidence frees the permit; unproven evidence
    /// parks it. There is no third outcome, and no path that frees a permit
    /// without evidence.
    pub async fn complete_release(
        &self,
        request: CompleteReleaseWriterPermit,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let now = now_ms();
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let row = load_locked(&mut tx, &request.worktree_id).await?;
        let mut record = row.ok_or(WriterPermitError::NotFound)?;
        assert_owner(&record, &request.owner_token)?;
        if record.revision != request.expected_revision
            || record.lease_generation != request.expected_lease_generation
        {
            return Err(WriterPermitError::StaleRevision);
        }
        if record.state != WriterPermitState::Releasing {
            return Err(WriterPermitError::InvalidTransition);
        }

        if let Some(reason) = request.evidence.unproven_reason() {
            let detail = truncate_detail("release evidence was unproven");
            let updated = sqlx::query(
                "UPDATE writer_permit SET state = 'quarantined', revision = revision + 1, \
                 quarantine_reason = ?, quarantine_detail = ?, quarantined_at_ms = ?, \
                 updated_at_ms = ? \
                 WHERE worktree_id = ? AND revision = ? AND lease_generation = ? \
                 AND state = 'releasing' AND owner_token = ?",
            )
            .bind(reason.as_str())
            .bind(detail)
            .bind(now)
            .bind(now)
            .bind(request.worktree_id.as_str())
            .bind(request.expected_revision as i64)
            .bind(request.expected_lease_generation as i64)
            .bind(request.owner_token.as_str())
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
            if updated.rows_affected() != 1 {
                return Err(WriterPermitError::StaleRevision);
            }
            record.state = WriterPermitState::Quarantined;
            record.revision += 1;
            record.quarantine = Some(WriterPermitQuarantine {
                reason,
                detail: truncate_detail("release evidence was unproven"),
                quarantined_at_ms: now,
            });
            record.updated_at_ms = now;
            tx.commit().await.map_err(storage_error)?;
            return Ok(record);
        }

        match request.transfer {
            Some(transfer) => {
                validate_idempotency_key(&transfer.idempotency_key)?;
                let next_owner = WriterPermitOwner {
                    principal_id: transfer.principal_id.clone(),
                    agent_run_id: transfer.agent_run_id.clone(),
                    group_id: transfer.group_id.clone(),
                    attempt_id: transfer.attempt_id.clone(),
                    token: uuid::Uuid::new_v4().to_string(),
                };
                let lease_expires_at_ms = transfer
                    .lease_expires_at_ms
                    .or(Some(now + DEFAULT_WRITER_PERMIT_LEASE_MS));
                let fingerprint = format!(
                    "transfer|principal={}|run={}|group={}|attempt={}",
                    next_owner.principal_id.as_str(),
                    next_owner
                        .agent_run_id
                        .as_ref()
                        .map(|id| id.as_str())
                        .unwrap_or("-"),
                    next_owner
                        .group_id
                        .as_ref()
                        .map(|id| id.as_str())
                        .unwrap_or("-"),
                    next_owner.attempt_id.as_deref().unwrap_or("-"),
                );
                // The successor's token replaces the old one in the same
                // statement, so the old token is invalid the moment this
                // commits. There is no window in which two tokens are valid.
                let updated = sqlx::query(
                    "UPDATE writer_permit SET state = 'held', owner_principal_id = ?, \
                     owner_agent_run_id = ?, owner_group_id = ?, owner_attempt_id = ?, \
                     owner_token = ?, operation = ?, revision = revision + 1, \
                     idempotency_key = ?, request_fingerprint = ?, lease_expires_at_ms = ?, \
                     acquired_at_ms = ?, updated_at_ms = ? \
                     WHERE worktree_id = ? AND revision = ? AND lease_generation = ? \
                     AND state = 'releasing' AND owner_token = ?",
                )
                .bind(next_owner.principal_id.as_str())
                .bind(next_owner.agent_run_id.as_ref().map(|id| id.as_str()))
                .bind(next_owner.group_id.as_ref().map(|id| id.as_str()))
                .bind(next_owner.attempt_id.as_deref())
                .bind(next_owner.token.as_str())
                .bind(transfer.operation.as_str())
                .bind(transfer.idempotency_key.as_str())
                .bind(fingerprint.as_str())
                .bind(lease_expires_at_ms)
                .bind(now)
                .bind(now)
                .bind(request.worktree_id.as_str())
                .bind(request.expected_revision as i64)
                .bind(request.expected_lease_generation as i64)
                .bind(request.owner_token.as_str())
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
                if updated.rows_affected() != 1 {
                    return Err(WriterPermitError::StaleRevision);
                }
                record.state = WriterPermitState::Held;
                record.owner = Some(next_owner);
                record.operation = Some(transfer.operation);
                record.revision += 1;
                record.lease_expires_at_ms = lease_expires_at_ms;
                record.acquired_at_ms = Some(now);
                record.updated_at_ms = now;
            }
            None => {
                let updated = sqlx::query(
                    "UPDATE writer_permit SET state = 'available', owner_principal_id = NULL, \
                     owner_agent_run_id = NULL, owner_group_id = NULL, owner_attempt_id = NULL, \
                     owner_token = NULL, operation = NULL, revision = revision + 1, \
                     idempotency_key = NULL, request_fingerprint = NULL, lease_expires_at_ms = NULL, \
                     quarantine_reason = NULL, quarantine_detail = NULL, quarantined_at_ms = NULL, \
                     acquired_at_ms = NULL, updated_at_ms = ? \
                     WHERE worktree_id = ? AND revision = ? AND lease_generation = ? \
                     AND state = 'releasing' AND owner_token = ?",
                )
                .bind(now)
                .bind(request.worktree_id.as_str())
                .bind(request.expected_revision as i64)
                .bind(request.expected_lease_generation as i64)
                .bind(request.owner_token.as_str())
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
                if updated.rows_affected() != 1 {
                    return Err(WriterPermitError::StaleRevision);
                }
                record.state = WriterPermitState::Available;
                record.owner = None;
                record.operation = None;
                record.revision += 1;
                record.lease_expires_at_ms = None;
                record.idempotency_key = None;
                record.request_fingerprint = None;
                record.acquired_at_ms = None;
                record.updated_at_ms = now;
            }
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }

    /// Park the permit.
    pub async fn quarantine(
        &self,
        request: QuarantineWriterPermit,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let now = now_ms();
        let detail = truncate_detail(&request.detail);
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let row = load_locked(&mut tx, &request.worktree_id).await?;
        let mut record = row.ok_or(WriterPermitError::NotFound)?;
        if let Some(token) = request.owner_token.as_deref() {
            assert_owner(&record, token)?;
        }
        if let Some(expected) = request.expected_revision {
            if record.revision != expected {
                return Err(WriterPermitError::StaleRevision);
            }
        }
        if record.state == WriterPermitState::Quarantined {
            // Already parked. Idempotent: the reason is not rewritten, so a
            // second, weaker report cannot overwrite a stronger one.
            tx.commit().await.map_err(storage_error)?;
            return Ok(record);
        }
        let updated = sqlx::query(
            "UPDATE writer_permit SET state = 'quarantined', revision = revision + 1, \
             quarantine_reason = ?, quarantine_detail = ?, quarantined_at_ms = ?, updated_at_ms = ? \
             WHERE worktree_id = ? AND revision = ?",
        )
        .bind(request.reason.as_str())
        .bind(detail.as_str())
        .bind(now)
        .bind(now)
        .bind(request.worktree_id.as_str())
        .bind(record.revision as i64)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            return Err(WriterPermitError::StaleRevision);
        }
        record.state = WriterPermitState::Quarantined;
        record.revision += 1;
        record.quarantine = Some(WriterPermitQuarantine {
            reason: request.reason,
            detail,
            quarantined_at_ms: now,
        });
        record.updated_at_ms = now;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }

    /// Free a quarantined permit. Operator-only, and evidence-gated.
    ///
    /// This never touches the checkout. A quarantined permit says "the state of
    /// this checkout is unknown"; releasing it says "an operator looked and
    /// decided the checkout is safe to write again". The files are untouched by
    /// either.
    pub async fn operator_release(
        &self,
        request: OperatorReleaseWriterPermit,
    ) -> Result<WriterPermitRecord, WriterPermitError> {
        let now = now_ms();
        if let Some(reason) = request.evidence.unproven_reason() {
            return Err(WriterPermitError::UnprovenQuiescence(reason));
        }
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let row = load_locked(&mut tx, &request.worktree_id).await?;
        let mut record = row.ok_or(WriterPermitError::NotFound)?;
        if record.revision != request.expected_revision {
            return Err(WriterPermitError::StaleRevision);
        }
        if record.state != WriterPermitState::Quarantined {
            return Err(WriterPermitError::InvalidTransition);
        }
        let detail = truncate_detail(&request.reason);
        let updated = sqlx::query(
            "UPDATE writer_permit SET state = 'available', owner_principal_id = NULL, \
             owner_agent_run_id = NULL, owner_group_id = NULL, owner_attempt_id = NULL, \
             owner_token = NULL, operation = NULL, revision = revision + 1, \
             idempotency_key = NULL, request_fingerprint = NULL, lease_expires_at_ms = NULL, \
             quarantine_reason = NULL, quarantine_detail = NULL, quarantined_at_ms = NULL, \
             acquired_at_ms = NULL, updated_at_ms = ? \
             WHERE worktree_id = ? AND revision = ? AND state = 'quarantined'",
        )
        .bind(now)
        .bind(request.worktree_id.as_str())
        .bind(request.expected_revision as i64)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            return Err(WriterPermitError::StaleRevision);
        }
        let _ = detail;
        record.state = WriterPermitState::Available;
        record.owner = None;
        record.operation = None;
        record.revision += 1;
        record.quarantine = None;
        record.lease_expires_at_ms = None;
        record.idempotency_key = None;
        record.request_fingerprint = None;
        record.acquired_at_ms = None;
        record.updated_at_ms = now;
        tx.commit().await.map_err(storage_error)?;
        Ok(record)
    }

    /// Reconcile permits after a daemon restart.
    ///
    /// A daemon restart means every previously-held permit is orphaned: the
    /// process that held it is gone. Every `held` or `releasing` permit is
    /// therefore quarantined.
    ///
    /// This **never** replays the holder's operation. Repairing metadata by
    /// re-running a mutation would be a second effect, not a repair; the only
    /// safe repair is to park the permit and let an operator look.
    pub async fn reconcile_after_restart(
        &self,
    ) -> Result<Vec<WriterPermitRecord>, WriterPermitError> {
        let now = now_ms();
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(storage_error)?;
        let reconciliation_sql = format!(
            "SELECT {SELECT_COLUMNS} FROM writer_permit WHERE state IN ('held','releasing')"
        );
        let rows = sqlx::query(&reconciliation_sql)
            .fetch_all(&mut **tx)
            .await
            .map_err(storage_error)?;
        let mut records = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut record = row_to_record(row)?;
            let updated = sqlx::query(
                "UPDATE writer_permit SET state = 'quarantined', revision = revision + 1, \
                 quarantine_reason = ?, quarantine_detail = ?, quarantined_at_ms = ?, \
                 updated_at_ms = ? WHERE worktree_id = ? AND revision = ? AND state IN ('held','releasing')",
            )
            .bind(WriterPermitQuarantineReason::DaemonRestart.as_str())
            .bind("permit was live when the daemon restarted")
            .bind(now)
            .bind(now)
            .bind(record.worktree_id.as_str())
            .bind(record.revision as i64)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
            if updated.rows_affected() == 1 {
                record.state = WriterPermitState::Quarantined;
                record.revision += 1;
                record.quarantine = Some(WriterPermitQuarantine {
                    reason: WriterPermitQuarantineReason::DaemonRestart,
                    detail: "permit was live when the daemon restarted".to_owned(),
                    quarantined_at_ms: now,
                });
                record.updated_at_ms = now;
            }
            records.push(record);
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The permit row's columns, in the order [`row_to_record`] reads them.
const SELECT_COLUMNS: &str = "worktree_id, project_id, repository_id, workspace_id, \
     lease_generation, group_id, state, owner_principal_id, owner_agent_run_id, owner_group_id, \
     owner_attempt_id, owner_token, operation, revision, idempotency_key, request_fingerprint, \
     lease_expires_at_ms, quarantine_reason, quarantine_detail, quarantined_at_ms, acquired_at_ms, \
     updated_at_ms";

/// `SELECT <columns> FROM writer_permit` for one checkout.
fn select_one_sql() -> String {
    format!("SELECT {SELECT_COLUMNS} FROM writer_permit WHERE worktree_id = ?")
}

/// `SELECT <columns> FROM writer_permit` for every row, newest first.
fn select_all_sql() -> String {
    format!("SELECT {SELECT_COLUMNS} FROM writer_permit ORDER BY updated_at_ms DESC LIMIT ?")
}

/// `SELECT <columns> FROM writer_permit` for one project.
fn select_project_sql() -> String {
    format!(
        "SELECT {SELECT_COLUMNS} FROM writer_permit WHERE project_id = ? \
         ORDER BY updated_at_ms DESC LIMIT ?"
    )
}

/// `SELECT <columns> FROM writer_permit` for one project in one state.
fn select_project_state_sql() -> String {
    format!(
        "SELECT {SELECT_COLUMNS} FROM writer_permit WHERE project_id = ? AND state = ? \
         ORDER BY updated_at_ms DESC LIMIT ?"
    )
}

/// `SELECT <columns> FROM writer_permit` for one state.
fn select_state_sql() -> String {
    format!(
        "SELECT {SELECT_COLUMNS} FROM writer_permit WHERE state = ? \
         ORDER BY updated_at_ms DESC LIMIT ?"
    )
}

async fn load_locked(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    worktree_id: &WorktreeId,
) -> Result<Option<WriterPermitRecord>, WriterPermitError> {
    let sql = select_one_sql();
    let row = sqlx::query(&sql)
        .bind(worktree_id.as_str())
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?;
    row.as_ref().map(row_to_record).transpose()
}

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> Result<WriterPermitRecord, WriterPermitError> {
    let worktree_id = WorktreeId::parse(row.get::<String, _>("worktree_id").as_str())
        .map_err(|_| WriterPermitError::CorruptState)?;
    let project_id = ProjectId::parse(row.get::<String, _>("project_id").as_str())
        .map_err(|_| WriterPermitError::CorruptState)?;
    let workspace_id = WorkspaceId::parse(row.get::<String, _>("workspace_id").as_str())
        .map_err(|_| WriterPermitError::CorruptState)?;
    let repository_id = match row.get::<Option<String>, _>("repository_id") {
        Some(value) => {
            Some(RepositoryId::parse(value.as_str()).map_err(|_| WriterPermitError::CorruptState)?)
        }
        None => None,
    };
    let group_id = match row.get::<Option<String>, _>("group_id") {
        Some(value) => Some(
            AgentRunGroupId::parse(value.as_str()).map_err(|_| WriterPermitError::CorruptState)?,
        ),
        None => None,
    };
    let state = WriterPermitState::parse(row.get::<String, _>("state").as_str())?;

    let owner_principal = row.get::<Option<String>, _>("owner_principal_id");
    let owner_token = row.get::<Option<String>, _>("owner_token");
    let owner = match (owner_principal, owner_token) {
        (Some(principal), Some(token)) => Some(WriterPermitOwner {
            principal_id: PrincipalId::parse(principal.as_str())
                .map_err(|_| WriterPermitError::CorruptState)?,
            agent_run_id: match row.get::<Option<String>, _>("owner_agent_run_id") {
                Some(value) => Some(
                    AgentRunId::parse(value.as_str())
                        .map_err(|_| WriterPermitError::CorruptState)?,
                ),
                None => None,
            },
            group_id: match row.get::<Option<String>, _>("owner_group_id") {
                Some(value) => Some(
                    AgentRunGroupId::parse(value.as_str())
                        .map_err(|_| WriterPermitError::CorruptState)?,
                ),
                None => None,
            },
            attempt_id: row.get::<Option<String>, _>("owner_attempt_id"),
            token,
        }),
        (None, None) => None,
        // A principal without a token, or a token without a principal, is a
        // half-written owner. It is not "the holder" — there is no way to prove
        // possession — so the row is corrupt rather than merely held.
        _ => return Err(WriterPermitError::CorruptState),
    };

    let operation = match row.get::<Option<String>, _>("operation") {
        Some(value) => Some(WriterPermitOperation::parse(value.as_str())?),
        None => None,
    };

    let quarantine = match row.get::<Option<String>, _>("quarantine_reason") {
        Some(reason) => Some(WriterPermitQuarantine {
            reason: WriterPermitQuarantineReason::parse(reason.as_str())?,
            detail: row
                .get::<Option<String>, _>("quarantine_detail")
                .unwrap_or_default(),
            quarantined_at_ms: row
                .get::<Option<i64>, _>("quarantined_at_ms")
                .unwrap_or_default(),
        }),
        None => None,
    };

    Ok(WriterPermitRecord {
        worktree_id,
        project_id,
        repository_id,
        workspace_id,
        lease_generation: row.get::<i64, _>("lease_generation") as u64,
        group_id,
        state,
        owner,
        operation,
        revision: row.get::<i64, _>("revision") as u64,
        idempotency_key: row.get::<Option<String>, _>("idempotency_key"),
        request_fingerprint: row.get::<Option<String>, _>("request_fingerprint"),
        lease_expires_at_ms: row.get::<Option<i64>, _>("lease_expires_at_ms"),
        quarantine,
        acquired_at_ms: row.get::<Option<i64>, _>("acquired_at_ms"),
        updated_at_ms: row.get::<i64, _>("updated_at_ms"),
    })
}

fn assert_owner(record: &WriterPermitRecord, token: &str) -> Result<(), WriterPermitError> {
    match record.owner.as_ref() {
        Some(owner) if owner.token == token => Ok(()),
        Some(_) => Err(WriterPermitError::NotOwner),
        None => Err(WriterPermitError::NotOwner),
    }
}

fn validate_idempotency_key(key: &str) -> Result<(), WriterPermitError> {
    if key.is_empty() || key.len() > MAX_WRITER_PERMIT_IDEMPOTENCY_BYTES {
        return Err(WriterPermitError::InvalidScope);
    }
    Ok(())
}

fn truncate_detail(detail: &str) -> String {
    if detail.len() <= MAX_WRITER_PERMIT_DETAIL_BYTES {
        return detail.to_owned();
    }
    let mut end = MAX_WRITER_PERMIT_DETAIL_BYTES;
    while end > 0 && !detail.is_char_boundary(end) {
        end -= 1;
    }
    detail[..end].to_owned()
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn storage_error(error: impl std::fmt::Display) -> WriterPermitError {
    WriterPermitError::Storage(error.to_string())
}
