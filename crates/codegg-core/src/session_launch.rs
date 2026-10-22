//! M006 — the durable, recoverable session-launch saga.
//!
//! Launching a session into an isolated checkout spans two resources that
//! cannot share one transaction: the **checkout** is a Git worktree on
//! the filesystem, and the **session** is a row in SQLite. There is no
//! atomic multi-resource commit, so this module records the *request*
//! durably and advances it through explicit stages.
//!
//! # Why the request itself is durable
//!
//! Without a durable record of the request, a retry cannot find its own
//! prior progress: `WorktreeService::reserve` mints a fresh
//! [`WorktreeId`] per call, and `SessionStore::fork` mints a fresh
//! session id per call. A retried launch would therefore allocate a
//! *second* checkout and a *second* session. The saga row is what makes
//! the retry converge instead.
//!
//! # Idempotency
//!
//! A launch is keyed by `(project_id, idempotency_key)`. A retry with
//! the same key and the same request fingerprint converges on the
//! existing row. A retry with the same key and a *different* fingerprint
//! is refused: it is a different request wearing an old key, and
//! silently running it would attach a new checkout to an old key's
//! record.
//!
//! # Recoverability
//!
//! A stage is only advanced when the work it names has actually
//! happened, and the row is written before the next step begins. A
//! launch that dies half-way leaves a row at a known stage, and
//! [`SqliteSessionLaunchStore::reconcile`] can tell an operator exactly
//! which stage it reached and what it still owes.
//!
//! The module is UI-, server-, plugin- and auth-free: it is the lowest
//! level at which the daemon reasons about a launch. Authorization lives
//! in the root crate's adapter.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::Digest;
use thiserror::Error;

use crate::error::StorageError;
use crate::storage::immediate::ImmediateWrite;

/// Maximum bytes in a launch idempotency key.
pub const MAX_LAUNCH_IDEMPOTENCY_KEY_BYTES: usize = 128;

/// Maximum bytes in a stored request fingerprint.
pub const MAX_LAUNCH_FINGERPRINT_BYTES: usize = 1024;

/// Maximum bytes in a stored attention code.
pub const MAX_LAUNCH_ATTENTION_BYTES: usize = 128;

/// What the launch is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLaunchIntent {
    /// A fresh session with no parent.
    New,
    /// A new session carrying the parent's conversation history.
    Fork,
}

impl SessionLaunchIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionLaunchIntent::New => "new",
            SessionLaunchIntent::Fork => "fork",
        }
    }
}

/// Where the launched session runs.
///
/// Only **managed** checkouts are addressable. An external checkout has
/// no durable identity in this daemon, so there is nothing to bind a
/// session to and nothing to authorize later; admitting one would
/// produce a session whose checkout the daemon cannot name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SessionLaunchPlacement {
    /// Allocate a fresh managed checkout for this launch.
    NewManagedCheckout {
        /// Optional base commit or ref the checkout is created from.
        base_ref: Option<String>,
        /// Optional target branch name.
        target_branch: Option<String>,
    },
    /// Use an already-authorized managed checkout.
    ExistingManagedWorktree { worktree_id: String },
}

impl SessionLaunchPlacement {
    /// The base ref, when the placement names one.
    pub fn base_ref(&self) -> Option<&str> {
        match self {
            SessionLaunchPlacement::NewManagedCheckout { base_ref, .. } => base_ref.as_deref(),
            SessionLaunchPlacement::ExistingManagedWorktree { .. } => None,
        }
    }

    /// The target branch, when the placement names one.
    pub fn target_branch(&self) -> Option<&str> {
        match self {
            SessionLaunchPlacement::NewManagedCheckout { target_branch, .. } => {
                target_branch.as_deref()
            }
            SessionLaunchPlacement::ExistingManagedWorktree { .. } => None,
        }
    }
}

/// The durable stage of a launch.
///
/// The order is the order of the work: a request is accepted, a checkout
/// is reserved, the checkout becomes ready, the session is created, the
/// session is bound, and the launch completes. `NeedsAttention` is the
/// visible orphan — a launch that cannot proceed without a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLaunchStage {
    /// Recorded. No checkout or session exists yet.
    Accepted,
    /// A checkout identity and locator are reserved.
    CheckoutReserved,
    /// The checkout exists and is registered.
    CheckoutReady,
    /// The session row exists.
    SessionCreated,
    /// The session is bound to the workspace and the checkout.
    Bound,
    /// The launch is complete.
    Completed,
    /// The launch cannot proceed and needs an operator.
    NeedsAttention,
}

impl SessionLaunchStage {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionLaunchStage::Accepted => "accepted",
            SessionLaunchStage::CheckoutReserved => "checkout_reserved",
            SessionLaunchStage::CheckoutReady => "checkout_ready",
            SessionLaunchStage::SessionCreated => "session_created",
            SessionLaunchStage::Bound => "bound",
            SessionLaunchStage::Completed => "completed",
            SessionLaunchStage::NeedsAttention => "needs_attention",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "accepted" => SessionLaunchStage::Accepted,
            "checkout_reserved" => SessionLaunchStage::CheckoutReserved,
            "checkout_ready" => SessionLaunchStage::CheckoutReady,
            "session_created" => SessionLaunchStage::SessionCreated,
            "bound" => SessionLaunchStage::Bound,
            "completed" => SessionLaunchStage::Completed,
            "needs_attention" => SessionLaunchStage::NeedsAttention,
            _ => return None,
        })
    }

    /// The stage's position in the forward-only sequence.
    ///
    /// `NeedsAttention` is terminal and has no rank: it is not "further
    /// along" than anything, it is a *different* outcome. A rank lets a
    /// resumed launch compare the stage it recorded with the stage it is
    /// about to reach, and skip the ones it already passed — the store
    /// refuses a repeated or backwards transition, and a resume must not
    /// be mistaken for a contract violation.
    pub fn rank(self) -> Option<u8> {
        Some(match self {
            SessionLaunchStage::Accepted => 0,
            SessionLaunchStage::CheckoutReserved => 1,
            SessionLaunchStage::CheckoutReady => 2,
            SessionLaunchStage::SessionCreated => 3,
            SessionLaunchStage::Bound => 4,
            SessionLaunchStage::Completed => 5,
            SessionLaunchStage::NeedsAttention => return None,
        })
    }

    /// Whether the stage is terminal.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionLaunchStage::Completed | SessionLaunchStage::NeedsAttention
        )
    }
}

/// The durable launch record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLaunchRecord {
    pub launch_id: String,
    pub project_id: String,
    pub intent: SessionLaunchIntent,
    pub placement: SessionLaunchPlacement,
    /// The parent session, for a fork.
    pub parent_session_id: Option<String>,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub stage: SessionLaunchStage,
    /// The reserved or adopted checkout.
    pub worktree_id: Option<String>,
    /// The created session.
    pub session_id: Option<String>,
    /// The workspace the session runs in.
    pub workspace_id: Option<String>,
    /// Why the launch needs attention, when it does.
    pub attention_code: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// The request that starts a launch.
#[derive(Debug, Clone)]
pub struct SessionLaunchRequest {
    pub project_id: String,
    pub intent: SessionLaunchIntent,
    pub placement: SessionLaunchPlacement,
    pub parent_session_id: Option<String>,
    pub idempotency_key: String,
}

impl SessionLaunchRequest {
    /// The request's identity, over everything that changes what the
    /// launch *does*.
    ///
    /// The idempotency key is deliberately excluded: it identifies the
    /// request, it is not part of what the request asks for. Two
    /// requests with the same key and different fingerprints are a
    /// conflict, not a match.
    pub fn fingerprint(&self) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.project_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(self.intent.as_str().as_bytes());
        hasher.update([0u8]);
        match &self.placement {
            SessionLaunchPlacement::NewManagedCheckout {
                base_ref,
                target_branch,
            } => {
                hasher.update(b"new_managed_checkout");
                hasher.update([0u8]);
                hasher.update(base_ref.as_deref().unwrap_or("").as_bytes());
                hasher.update([0u8]);
                hasher.update(target_branch.as_deref().unwrap_or("").as_bytes());
            }
            SessionLaunchPlacement::ExistingManagedWorktree { worktree_id } => {
                hasher.update(b"existing_managed_worktree");
                hasher.update([0u8]);
                hasher.update(worktree_id.as_bytes());
            }
        }
        hasher.update([0u8]);
        hasher.update(self.parent_session_id.as_deref().unwrap_or("").as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Errors from the launch store.
#[derive(Debug, Error)]
pub enum SessionLaunchError {
    #[error("session launch not found: {0}")]
    NotFound(String),
    #[error("session launch idempotency conflict for key {key}")]
    IdempotencyConflict { key: String },
    #[error("invalid session launch request: {0}")]
    InvalidRequest(String),
    #[error("invalid session launch transition: {from} -> {to}")]
    InvalidTransition {
        from: &'static str,
        to: &'static str,
    },
    /// The checkout could not be reserved, created or adopted.
    #[error("session launch checkout failed: {0}")]
    Checkout(String),
    /// The workspace could not be registered or bound.
    #[error("session launch workspace failed: {0}")]
    Workspace(String),
    /// The session could not be created or forked.
    #[error("session launch session failed: {0}")]
    Session(String),
    #[error("session launch store error: {0}")]
    Storage(#[from] StorageError),
}

impl SessionLaunchError {
    /// The bounded, non-secret code recorded when a launch needs an
    /// operator.
    ///
    /// The code is a *category*, never the detail: the detail is the
    /// error's own message, which the operator surface shows in full.
    /// A code that carried the detail would end up in a durable record
    /// and in an operator's list, where it would be both unbounded and
    /// misleading.
    pub fn attention_code(&self) -> &'static str {
        match self {
            SessionLaunchError::NotFound(_) => "not_found",
            SessionLaunchError::IdempotencyConflict { .. } => "idempotency_conflict",
            SessionLaunchError::InvalidRequest(_) => "invalid_request",
            SessionLaunchError::InvalidTransition { .. } => "invalid_transition",
            SessionLaunchError::Checkout(_) => "checkout_failed",
            SessionLaunchError::Workspace(_) => "workspace_failed",
            SessionLaunchError::Session(_) => "session_failed",
            SessionLaunchError::Storage(_) => "storage_failed",
        }
    }
}

const LAUNCH_COLUMNS: &str = "launch_id, project_id, intent, placement, parent_session_id, \
     idempotency_key, request_fingerprint, stage, worktree_id, session_id, workspace_id, \
     attention_code, created_at_ms, updated_at_ms";

/// The durable launch store.
#[derive(Debug, Clone)]
pub struct SqliteSessionLaunchStore {
    pool: sqlx::SqlitePool,
}

impl SqliteSessionLaunchStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    /// Accept a launch request, or converge on the existing one.
    ///
    /// Idempotent on `(project_id, idempotency_key)`. The read and the
    /// write are one `BEGIN IMMEDIATE` transaction: a deferred
    /// transaction would let two concurrent identical requests both read
    /// "no row" and both insert.
    pub async fn accept(
        &self,
        request: &SessionLaunchRequest,
        now_ms: i64,
    ) -> Result<SessionLaunchRecord, SessionLaunchError> {
        if request.idempotency_key.is_empty()
            || request.idempotency_key.len() > MAX_LAUNCH_IDEMPOTENCY_KEY_BYTES
        {
            return Err(SessionLaunchError::InvalidRequest(
                "idempotency key must be 1..=128 bytes".to_string(),
            ));
        }
        if request.project_id.is_empty() {
            return Err(SessionLaunchError::InvalidRequest(
                "project id is required".to_string(),
            ));
        }
        if request.intent == SessionLaunchIntent::Fork
            && request
                .parent_session_id
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Err(SessionLaunchError::InvalidRequest(
                "a fork launch requires a parent session".to_string(),
            ));
        }
        let fingerprint = request.fingerprint();
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;

        let existing: Option<(String, String, String)> = sqlx::query_as(
            "SELECT launch_id, request_fingerprint, stage FROM session_launch \
             WHERE project_id = ? AND idempotency_key = ?",
        )
        .bind(&request.project_id)
        .bind(&request.idempotency_key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| StorageError::Database(error.to_string()))?;

        if let Some((launch_id, existing_fingerprint, stage)) = existing {
            if existing_fingerprint != fingerprint {
                return Err(SessionLaunchError::IdempotencyConflict {
                    key: request.idempotency_key.clone(),
                });
            }
            let stage = SessionLaunchStage::parse(&stage).ok_or_else(|| {
                StorageError::Database(format!("unknown session launch stage {stage}"))
            })?;
            let record = self
                .load_in(&mut tx, &launch_id)
                .await?
                .ok_or_else(|| StorageError::Database("session launch row vanished".to_string()))?;
            debug_assert_eq!(record.stage, stage);
            tx.commit()
                .await
                .map_err(|error| StorageError::Database(error.to_string()))?;
            return Ok(record);
        }

        let launch_id = uuid::Uuid::new_v4().to_string();
        let placement = serde_json::to_string(&request.placement).map_err(|error| {
            StorageError::Database(format!("cannot encode launch placement: {error}"))
        })?;
        sqlx::query(
            "INSERT INTO session_launch (launch_id, project_id, intent, placement, \
             parent_session_id, idempotency_key, request_fingerprint, stage, worktree_id, \
             session_id, workspace_id, attention_code, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, ?, ?)",
        )
        .bind(&launch_id)
        .bind(&request.project_id)
        .bind(request.intent.as_str())
        .bind(&placement)
        .bind(request.parent_session_id.as_deref())
        .bind(&request.idempotency_key)
        .bind(&fingerprint)
        .bind(SessionLaunchStage::Accepted.as_str())
        .bind(now_ms)
        .bind(now_ms)
        .execute(&mut **tx)
        .await
        .map_err(|error| StorageError::Database(error.to_string()))?;

        tx.commit()
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;

        Ok(SessionLaunchRecord {
            launch_id,
            project_id: request.project_id.clone(),
            intent: request.intent,
            placement: request.placement.clone(),
            parent_session_id: request.parent_session_id.clone(),
            idempotency_key: request.idempotency_key.clone(),
            request_fingerprint: fingerprint,
            stage: SessionLaunchStage::Accepted,
            worktree_id: None,
            session_id: None,
            workspace_id: None,
            attention_code: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        })
    }

    /// Advance a launch to the next stage, recording what it produced.
    ///
    /// The stage transition is checked: a launch only moves forward
    /// through the declared order, and a terminal stage never moves.
    pub async fn advance(
        &self,
        launch_id: &str,
        to: SessionLaunchStage,
        progress: SessionLaunchProgress,
        now_ms: i64,
    ) -> Result<SessionLaunchRecord, SessionLaunchError> {
        let mut tx = ImmediateWrite::begin(&self.pool)
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        let current = self
            .load_in(&mut tx, launch_id)
            .await?
            .ok_or_else(|| SessionLaunchError::NotFound(launch_id.to_string()))?;
        if current.stage.is_terminal() {
            return Err(SessionLaunchError::InvalidTransition {
                from: current.stage.as_str(),
                to: to.as_str(),
            });
        }
        if !is_forward_transition(current.stage, to) {
            return Err(SessionLaunchError::InvalidTransition {
                from: current.stage.as_str(),
                to: to.as_str(),
            });
        }
        let worktree_id = progress.worktree_id.or(current.worktree_id);
        let session_id = progress.session_id.or(current.session_id);
        let workspace_id = progress.workspace_id.or(current.workspace_id);
        let attention_code = progress.attention_code.or(current.attention_code);
        sqlx::query(
            "UPDATE session_launch SET stage = ?, worktree_id = ?, session_id = ?, \
             workspace_id = ?, attention_code = ?, updated_at_ms = ? WHERE launch_id = ?",
        )
        .bind(to.as_str())
        .bind(worktree_id.as_deref())
        .bind(session_id.as_deref())
        .bind(workspace_id.as_deref())
        .bind(attention_code.as_deref())
        .bind(now_ms)
        .bind(launch_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| StorageError::Database(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        self.load(launch_id)
            .await?
            .ok_or_else(|| SessionLaunchError::NotFound(launch_id.to_string()))
    }

    /// Load one launch.
    pub async fn load(
        &self,
        launch_id: &str,
    ) -> Result<Option<SessionLaunchRecord>, SessionLaunchError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        self.load_in(&mut conn, launch_id).await
    }

    /// List the launches of a project, newest first.
    pub async fn list(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionLaunchRecord>, SessionLaunchError> {
        let sql = format!(
            "SELECT {LAUNCH_COLUMNS} FROM session_launch WHERE project_id = ? \
             ORDER BY created_at_ms DESC, launch_id DESC LIMIT ?"
        );
        let rows = sqlx::query_as::<_, LaunchRow>(&sql)
            .bind(project_id)
            .bind(limit.min(256) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        rows.into_iter().map(LaunchRow::into_record).collect()
    }

    /// The launches that are not finished, for startup reconciliation.
    pub async fn reconcile(&self) -> Result<Vec<SessionLaunchRecord>, SessionLaunchError> {
        let sql = format!(
            "SELECT {LAUNCH_COLUMNS} FROM session_launch WHERE stage NOT IN \
             ('completed', 'needs_attention') ORDER BY created_at_ms ASC, launch_id ASC"
        );
        let rows = sqlx::query_as::<_, LaunchRow>(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        rows.into_iter().map(LaunchRow::into_record).collect()
    }

    async fn load_in(
        &self,
        conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
        launch_id: &str,
    ) -> Result<Option<SessionLaunchRecord>, SessionLaunchError> {
        let sql = format!("SELECT {LAUNCH_COLUMNS} FROM session_launch WHERE launch_id = ?");
        let row = sqlx::query_as::<_, LaunchRow>(&sql)
            .bind(launch_id)
            .fetch_optional(&mut **conn)
            .await
            .map_err(|error| StorageError::Database(error.to_string()))?;
        row.map(LaunchRow::into_record).transpose()
    }
}

/// What a stage transition produced.
#[derive(Debug, Clone, Default)]
pub struct SessionLaunchProgress {
    pub worktree_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub attention_code: Option<String>,
}

fn is_forward_transition(from: SessionLaunchStage, to: SessionLaunchStage) -> bool {
    use SessionLaunchStage::*;
    match from {
        Accepted => matches!(to, CheckoutReserved | NeedsAttention),
        CheckoutReserved => matches!(to, CheckoutReady | NeedsAttention),
        CheckoutReady => matches!(to, SessionCreated | NeedsAttention),
        SessionCreated => matches!(to, Bound | NeedsAttention),
        Bound => matches!(to, Completed | NeedsAttention),
        Completed | NeedsAttention => false,
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LaunchRow {
    launch_id: String,
    project_id: String,
    intent: String,
    placement: String,
    parent_session_id: Option<String>,
    idempotency_key: String,
    request_fingerprint: String,
    stage: String,
    worktree_id: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    attention_code: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl LaunchRow {
    fn into_record(self) -> Result<SessionLaunchRecord, SessionLaunchError> {
        let intent = match self.intent.as_str() {
            "new" => SessionLaunchIntent::New,
            "fork" => SessionLaunchIntent::Fork,
            other => {
                return Err(StorageError::Database(format!(
                    "unknown session launch intent {other}"
                ))
                .into())
            }
        };
        let placement: SessionLaunchPlacement =
            serde_json::from_str(&self.placement).map_err(|error| {
                SessionLaunchError::Storage(StorageError::Database(format!(
                    "cannot decode launch placement: {error}"
                )))
            })?;
        let stage = SessionLaunchStage::parse(&self.stage).ok_or_else(|| {
            SessionLaunchError::Storage(StorageError::Database(format!(
                "unknown session launch stage {}",
                self.stage
            )))
        })?;
        Ok(SessionLaunchRecord {
            launch_id: self.launch_id,
            project_id: self.project_id,
            intent,
            placement,
            parent_session_id: self.parent_session_id,
            idempotency_key: self.idempotency_key,
            request_fingerprint: self.request_fingerprint,
            stage,
            worktree_id: self.worktree_id,
            session_id: self.session_id,
            workspace_id: self.workspace_id,
            attention_code: self.attention_code,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

/// The count of launches by stage, for diagnostics.
pub fn stage_counts(records: &[SessionLaunchRecord]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for record in records {
        *counts.entry(record.stage.as_str().to_string()).or_insert(0) += 1;
    }
    counts
}
