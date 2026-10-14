//! C002 explicitly authorized idle owner continuation.
//!
//! When an authorized user/task explicitly requests "run the build/watch CI
//! and continue once it finishes," the daemon may start one bounded **normal**
//! owner turn on the durable terminal event — even when `AgentLoop::run()` has
//! ended and no frontend reconnects. Default stays `notify_only` with zero
//! autonomous model tokens.
//!
//! # Authority contract
//!
//! - `on_complete=notify_only` (default) **never enqueues a model invocation**.
//!   A model setting `resume_owner` without a host-trusted grant gets
//!   notify-only (explicit denial for the idle path); the pre-existing
//!   next-turn lease marker is preserved for backward compatibility but never
//!   triggers idle dispatch.
//! - A durable `ContinuationGrant` binds authenticated principal, session,
//!   workspace, originating task/turn, accepted job/notice identity, allowed
//!   model/connection, budgets, expiry, and max continuations (default 1).
//!   Without a valid grant, `resume_owner` is notify-only for idle. Legacy
//!   rows without a grant never auto-resume after upgrade.
//! - A completion event is untrusted data until matched to final `JobStore`
//!   state or validated C001 watch state. Only host-authored compact
//!   status/result refs enter authorized prompt context; remote CI titles,
//!   logs, and action names stay inert.
//! - At most one admitted continuation turn per notice/grant (DB CAS,
//!   `UNIQUE(notice_id, grant_id)` + state machine). A crash after provider
//!   submission but before response is ambiguous and becomes
//!   `needs_attention` — never blind replay. Exactly-once logical admission
//!   is possible; exactly-once external execution is not claimed.
//! - Dispatch routes through the same turn-runtime admission as ordinary
//!   host-authorized turns via the injected [`ContinuationExecutor`]. The
//!   scheduler/CI callback never calls `AgentLoop.run()` directly and never
//!   substitutes principal authentication. Production wiring submits a
//!   standard turn request; tests inject a fake executor (no paid inference).
//! - User cancel, stale session, revocation, expiry, and quota exhaustion win
//!   before provider invocation. A second simultaneous owner loop is never
//!   started; if another turn is already running the intent parks for
//!   attention per policy (fail-closed, one actionable notice retained).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

/// Maximum continuations a single grant may authorize (default 1, cap 4).
pub const GRANT_MAX_CONTINUATIONS_CAP: u32 = 4;

/// Default grant TTL when the caller supplies none (30 minutes).
pub const GRANT_DEFAULT_TTL_MS: i64 = 30 * 60 * 1_000;

/// Maximum grant TTL (4 hours).
pub const GRANT_MAX_TTL_MS: i64 = 4 * 60 * 60 * 1_000;

#[derive(Debug, thiserror::Error)]
pub enum ContinuationError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("not found")]
    NotFound,
}

/// Durable host-trusted approval for one background wait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationGrant {
    pub grant_id: String,
    /// Background identity this grant authorizes: scheduler `job_id` or
    /// `ciwatch:<watch_id>`.
    pub notice_job_id: String,
    pub session_id: String,
    #[serde(default)]
    pub owner_principal: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub origin_turn_id: Option<String>,
    #[serde(default)]
    pub allowed_model: Option<String>,
    #[serde(default)]
    pub allowed_connection: Option<String>,
    pub max_continuations: u32,
    #[serde(default)]
    pub used_continuations: u32,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_duration_ms: Option<i64>,
    pub status: ContinuationGrantStatus,
    pub expiry_ms: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationGrantStatus {
    Pending,
    Approved,
    Revoked,
    Consumed,
    Expired,
}

pub fn grant_status_to_str(s: ContinuationGrantStatus) -> &'static str {
    match s {
        ContinuationGrantStatus::Pending => "pending",
        ContinuationGrantStatus::Approved => "approved",
        ContinuationGrantStatus::Revoked => "revoked",
        ContinuationGrantStatus::Consumed => "consumed",
        ContinuationGrantStatus::Expired => "expired",
    }
}

fn grant_status_from_str(s: &str) -> Option<ContinuationGrantStatus> {
    match s {
        "pending" => Some(ContinuationGrantStatus::Pending),
        "approved" => Some(ContinuationGrantStatus::Approved),
        "revoked" => Some(ContinuationGrantStatus::Revoked),
        "consumed" => Some(ContinuationGrantStatus::Consumed),
        "expired" => Some(ContinuationGrantStatus::Expired),
        _ => None,
    }
}

/// Exactly-once dispatch intent for one `(notice, grant)` pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleContinuationIntent {
    pub intent_id: String,
    pub notice_id: String,
    pub grant_id: String,
    pub session_id: String,
    pub state: IdleContinuationState,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub cause: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleContinuationState {
    Pending,
    Eligible,
    Claimed,
    TurnAccepted,
    Started,
    Completed,
    Suppressed,
    Expired,
    NeedsAttention,
}

pub fn intent_state_to_str(s: IdleContinuationState) -> &'static str {
    match s {
        IdleContinuationState::Pending => "pending",
        IdleContinuationState::Eligible => "eligible",
        IdleContinuationState::Claimed => "claimed",
        IdleContinuationState::TurnAccepted => "turn_accepted",
        IdleContinuationState::Started => "started",
        IdleContinuationState::Completed => "completed",
        IdleContinuationState::Suppressed => "suppressed",
        IdleContinuationState::Expired => "expired",
        IdleContinuationState::NeedsAttention => "needs_attention",
    }
}

fn intent_state_from_str(s: &str) -> Option<IdleContinuationState> {
    match s {
        "pending" => Some(IdleContinuationState::Pending),
        "eligible" => Some(IdleContinuationState::Eligible),
        "claimed" => Some(IdleContinuationState::Claimed),
        "turn_accepted" => Some(IdleContinuationState::TurnAccepted),
        "started" => Some(IdleContinuationState::Started),
        "completed" => Some(IdleContinuationState::Completed),
        "suppressed" => Some(IdleContinuationState::Suppressed),
        "expired" => Some(IdleContinuationState::Expired),
        "needs_attention" => Some(IdleContinuationState::NeedsAttention),
        _ => None,
    }
}

/// Authorized prompt context for the continuation turn. Only host-authored
/// compact refs — never remote CI text as instructions.
#[derive(Debug, Clone)]
pub struct ContinuationContext {
    pub notice_id: String,
    pub grant_id: String,
    pub session_id: String,
    pub summary: String,
    pub model: Option<String>,
}

/// Outcome of one idempotent dispatch attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// Exactly one turn was admitted by this call.
    Dispatched,
    /// A prior call already admitted the turn (replay safe).
    AlreadyDispatched,
    /// No dispatch: notify-only, missing/invalid grant, cancel/expiry/
    /// revocation/budget, unsafe status, or owner mismatch. The durable
    /// notice is retained for attention.
    Suppressed,
    /// Ambiguous provider-side start: recorded as needs_attention, never
    /// replayed.
    NeedsAttention,
}

/// Pluggable turn-submission boundary. Production routes through the
/// canonical turn runtime; tests inject a fake that records invocation
/// without spending inference.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait ContinuationExecutor: Send + Sync {
    async fn execute_turn(&self, ctx: ContinuationContext) -> Result<String, String>;
}

/// Test-only fake: records invocations with their authorized context.
#[derive(Default)]
pub struct FakeContinuationExecutor {
    calls: RwLock<Vec<ContinuationContext>>,
    fail_after_accept: bool,
}

impl FakeContinuationExecutor {
    pub fn new() -> Self {
        Self {
            calls: RwLock::new(Vec::new()),
            fail_after_accept: false,
        }
    }

    /// Simulate ambiguity: accept the turn (record the call) then report a
    /// provider-side failure that must become `needs_attention`, not replay.
    pub fn failing() -> Self {
        Self {
            calls: RwLock::new(Vec::new()),
            fail_after_accept: true,
        }
    }

    pub async fn call_count(&self) -> usize {
        self.calls.read().await.len()
    }

    pub async fn calls(&self) -> Vec<ContinuationContext> {
        self.calls.read().await.clone()
    }
}

#[async_trait::async_trait]
impl ContinuationExecutor for FakeContinuationExecutor {
    async fn execute_turn(&self, ctx: ContinuationContext) -> Result<String, String> {
        let turn_id = format!(
            "turn-{}-{}",
            ctx.notice_id.len(),
            self.calls.read().await.len()
        );
        self.calls.write().await.push(ctx);
        if self.fail_after_accept {
            return Err("ambiguous provider-side start".to_string());
        }
        Ok(turn_id)
    }
}

/// Input for creating a grant. All identity comes from the host boundary,
/// never model input.
pub struct NewGrant {
    pub notice_job_id: String,
    pub session_id: String,
    pub owner_principal: Option<String>,
    pub workspace_id: Option<String>,
    pub origin_turn_id: Option<String>,
    pub allowed_model: Option<String>,
    pub allowed_connection: Option<String>,
    pub max_continuations: Option<u32>,
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u32>,
    pub max_duration_ms: Option<i64>,
    pub ttl_ms: Option<i64>,
}

/// C005 (ADR-0031): server-resolved origin for one host-approved grant.
///
/// Every identity field here MUST be resolved server-side by the grant
/// handler (transport authority + gate decision + owning session row),
/// never copied from the request payload. The type boundary — not a
/// comment — is what keeps caller-supplied `NewGrant` strings from
/// reaching the autonomous path: only this input flows into
/// [`ContinuationService::authorize_host_grant`], and only grants with a
/// stored [`codegg_core::authorization::OriginAttribution`] row (written
/// by the same handler) are wake-eligible. Legacy `NewGrant` rows stay
/// non-autonomous.
pub struct HostGrantInput {
    pub notice_job_id: String,
    pub session_id: String,
    pub owner_principal: String,
    pub workspace_id: Option<String>,
    pub origin_turn_id: Option<String>,
    pub allowed_model: Option<String>,
    pub allowed_connection: Option<String>,
    pub max_continuations: Option<u32>,
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u32>,
    pub max_duration_ms: Option<i64>,
    pub ttl_ms: Option<i64>,
}

/// Delegated client marker recorded as audit/presence metadata for an
/// idle-continuation turn. Never an authority: the authority is the
/// reconstructed grant owner.
pub fn delegated_client_id(grant_id: &str) -> String {
    format!("idle:{grant_id}")
}

/// Deterministic delegation submission id: identical (notice, grant)
/// pairs coalesce on the durable receipt, so a retried wake never mints
/// a second turn identity.
pub fn delegation_submission_id(notice_id: &str, grant_id: &str) -> String {
    format!(
        "idle-{:x}",
        sha2::Sha256::digest(format!("idle-continuation|{notice_id}|{grant_id}").as_bytes())
    )
}

/// Canonical digest over the delegated turn content bound to one wake.
/// The receipt identity check compares this, so a reused submission id
/// with different content or authority is a conflict, never a replay.
pub fn delegation_request_digest(
    notice_id: &str,
    grant_id: &str,
    session_id: &str,
    owner_principal: &str,
    summary: &str,
) -> String {
    format!(
        "{:x}",
        sha2::Sha256::digest(
            format!(
                "idle-continuation|{notice_id}|{grant_id}|{session_id}|{owner_principal}|{summary}"
            )
            .as_bytes()
        )
    )
}

#[derive(Default)]
pub struct ContinuationService {
    grants: RwLock<HashMap<String, ContinuationGrant>>,
    intents: RwLock<HashMap<String, IdleContinuationIntent>>,
    pool: Option<SqlitePool>,
}

impl ContinuationService {
    pub fn new() -> Self {
        Self {
            grants: RwLock::new(HashMap::new()),
            intents: RwLock::new(HashMap::new()),
            pool: None,
        }
    }

    pub fn with_pool(pool: SqlitePool) -> Self {
        Self {
            grants: RwLock::new(HashMap::new()),
            intents: RwLock::new(HashMap::new()),
            pool: Some(pool),
        }
    }

    /// Create a host-authorized grant. Fails closed on missing session/job,
    /// empty ids, or over-cap continuations. Returns the durable grant.
    pub async fn authorize(&self, input: NewGrant) -> Result<ContinuationGrant, ContinuationError> {
        if input.notice_job_id.is_empty() || input.session_id.is_empty() {
            return Err(ContinuationError::Unauthorized(
                "continuation grants require a host-resolved job and session".into(),
            ));
        }
        let max_cont = input
            .max_continuations
            .unwrap_or(1)
            .clamp(1, GRANT_MAX_CONTINUATIONS_CAP);
        let now = chrono::Utc::now().timestamp_millis();
        let ttl = input
            .ttl_ms
            .unwrap_or(GRANT_DEFAULT_TTL_MS)
            .clamp(1, GRANT_MAX_TTL_MS);
        let grant_id = format!(
            "cg-{:x}",
            sha2::Sha256::digest(
                format!(
                    "{}|{}|{}|{now}",
                    input.notice_job_id,
                    input.session_id,
                    input.origin_turn_id.as_deref().unwrap_or("")
                )
                .as_bytes()
            )
        );
        // Idempotent on grant_id (same inputs coalesce).
        if let Some(existing) = self.get_grant(&grant_id).await? {
            return Ok(existing);
        }
        let grant = ContinuationGrant {
            grant_id: grant_id.clone(),
            notice_job_id: input.notice_job_id,
            session_id: input.session_id,
            owner_principal: input.owner_principal,
            workspace_id: input.workspace_id,
            origin_turn_id: input.origin_turn_id,
            allowed_model: input.allowed_model,
            allowed_connection: input.allowed_connection,
            max_continuations: max_cont,
            used_continuations: 0,
            max_tokens: input.max_tokens,
            max_tool_calls: input.max_tool_calls,
            max_duration_ms: input.max_duration_ms,
            status: ContinuationGrantStatus::Approved,
            expiry_ms: now.saturating_add(ttl),
            created_at: now,
            updated_at: now,
        };
        self.persist_grant(&grant).await?;
        {
            let mut grants = self.grants.write().await;
            grants.entry(grant_id.clone()).or_insert(grant.clone());
        }
        Ok(grant)
    }

    /// C005 (ADR-0031): create one host-approved grant from
    /// server-resolved origin.
    ///
    /// The caller MUST supply [`HostGrantInput`] built from the transport
    /// authority, the gate decision, and the owning session row — never
    /// from the request payload. Storage is shared with [`Self::authorize`]
    /// (same table, same clamps, same idempotency), but wake eligibility
    /// additionally requires the stored `continuation_grant` origin
    /// attribution row the grant handler writes in the same request: a
    /// grant without attestable origin — every legacy row — stays
    /// notify-only no matter what its strings claim.
    pub async fn authorize_host_grant(
        &self,
        input: HostGrantInput,
    ) -> Result<ContinuationGrant, ContinuationError> {
        if input.notice_job_id.is_empty()
            || input.session_id.is_empty()
            || input.owner_principal.is_empty()
        {
            return Err(ContinuationError::Unauthorized(
                "host grants require a server-resolved job, session, and owner".into(),
            ));
        }
        self.authorize(NewGrant {
            notice_job_id: input.notice_job_id,
            session_id: input.session_id,
            owner_principal: Some(input.owner_principal),
            workspace_id: input.workspace_id,
            origin_turn_id: input.origin_turn_id,
            allowed_model: input.allowed_model,
            allowed_connection: input.allowed_connection,
            max_continuations: input.max_continuations,
            max_tokens: input.max_tokens,
            max_tool_calls: input.max_tool_calls,
            max_duration_ms: input.max_duration_ms,
            ttl_ms: input.ttl_ms,
        })
        .await
    }

    pub async fn get_grant(
        &self,
        grant_id: &str,
    ) -> Result<Option<ContinuationGrant>, ContinuationError> {
        {
            let grants = self.grants.read().await;
            if let Some(g) = grants.get(grant_id) {
                return Ok(Some(g.clone()));
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM continuation_grant WHERE grant_id = ?",
            )
            .bind(grant_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?
            {
                let mut grant: ContinuationGrant = serde_json::from_str(&json)?;
                let status_row: Option<String> =
                    sqlx::query_scalar("SELECT status FROM continuation_grant WHERE grant_id = ?")
                        .bind(grant_id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| ContinuationError::Storage(e.to_string()))?;
                if let Some(s) = status_row.as_deref().and_then(grant_status_from_str) {
                    grant.status = s;
                }
                // Never overwrite: a concurrent writer in this process
                // may hold a newer entry (status/budget advanced after our
                // read). Return the cached entry, which writers keep current.
                let mut grants = self.grants.write().await;
                let entry = grants.entry(grant_id.to_owned()).or_insert(grant);
                return Ok(Some(entry.clone()));
            }
        }
        Ok(None)
    }

    /// DB-first grant load for authority decisions (wake admission,
    /// status reads). Process memory is only a hint: a revocation or
    /// budget consumption committed by another daemon (stale `flock`,
    /// divergent home) must be visible here even when this service cached
    /// an older row. Refreshes the cache on a hit; evicts on an
    /// authoritative miss. Also overlays `used_continuations`, which the
    /// budget bump keeps in its column without rewriting `record_json`.
    pub(crate) async fn get_grant_fresh(
        &self,
        grant_id: &str,
    ) -> Result<Option<ContinuationGrant>, ContinuationError> {
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM continuation_grant WHERE grant_id = ?",
            )
            .bind(grant_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?
            {
                let mut grant: ContinuationGrant = serde_json::from_str(&json)?;
                let status_row: Option<String> =
                    sqlx::query_scalar("SELECT status FROM continuation_grant WHERE grant_id = ?")
                        .bind(grant_id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| ContinuationError::Storage(e.to_string()))?;
                if let Some(s) = status_row.as_deref().and_then(grant_status_from_str) {
                    grant.status = s;
                }
                let used_row: Option<i64> = sqlx::query_scalar(
                    "SELECT used_continuations FROM continuation_grant WHERE grant_id = ?",
                )
                .bind(grant_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| ContinuationError::Storage(e.to_string()))?;
                if let Some(used) = used_row {
                    grant.used_continuations = used.max(0) as u32;
                }
                let mut grants = self.grants.write().await;
                grants.insert(grant_id.to_owned(), grant.clone());
                return Ok(Some(grant));
            }
            self.grants.write().await.remove(grant_id);
            return Ok(None);
        }
        self.get_grant(grant_id).await
    }

    /// Find the newest usable grant for one background job. Returns `None`
    /// when no approved, unexpired grant with remaining budget exists —
    /// the idle path then stays notify-only.
    pub async fn grant_for_job(
        &self,
        notice_job_id: &str,
    ) -> Result<Option<ContinuationGrant>, ContinuationError> {
        if let Some(pool) = &self.pool {
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT grant_id FROM continuation_grant WHERE notice_job_id = ? AND status = 'approved' ORDER BY created_at DESC LIMIT 8",
            )
            .bind(notice_job_id)
            .fetch_all(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
            for id in ids {
                if let Some(g) = self.get_grant_fresh(&id).await? {
                    if self.grant_usable(&g) {
                        return Ok(Some(g));
                    }
                }
            }
            return Ok(None);
        }
        let grants = self.grants.read().await;
        Ok(grants
            .values()
            .filter(|g| g.notice_job_id == notice_job_id && self.grant_usable(g))
            .max_by_key(|g| g.created_at)
            .cloned())
    }

    fn grant_usable(&self, grant: &ContinuationGrant) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        grant.status == ContinuationGrantStatus::Approved
            && now <= grant.expiry_ms
            && grant.used_continuations < grant.max_continuations
    }

    /// Revoke a grant (user cancel, principal change, workspace removal).
    /// Idempotent; returns `true` when the grant is now non-approved.
    pub async fn revoke(&self, grant_id: &str) -> Result<bool, ContinuationError> {
        self.transition_grant(grant_id, "approved", "revoked").await
    }

    async fn transition_grant(
        &self,
        grant_id: &str,
        from: &str,
        to: &str,
    ) -> Result<bool, ContinuationError> {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let result = sqlx::query(
                "UPDATE continuation_grant SET status = ?2, updated_at = ?3 WHERE grant_id = ?1 AND status = ?4",
            )
            .bind(grant_id)
            .bind(to)
            .bind(now)
            .bind(from)
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
            if result.rows_affected() > 0 {
                let mut grants = self.grants.write().await;
                if let Some(g) = grants.get_mut(grant_id) {
                    g.status =
                        grant_status_from_str(to).unwrap_or(ContinuationGrantStatus::Revoked);
                    g.updated_at = now;
                }
                return Ok(true);
            }
            let _ = self.get_grant(grant_id).await;
            return Ok(false);
        }
        let mut grants = self.grants.write().await;
        let Some(g) = grants.get_mut(grant_id) else {
            return Ok(false);
        };
        if grant_status_to_str(g.status) != from {
            return Ok(false);
        }
        g.status = grant_status_from_str(to).unwrap_or(ContinuationGrantStatus::Revoked);
        g.updated_at = now;
        Ok(true)
    }

    async fn persist_grant(&self, grant: &ContinuationGrant) -> Result<(), ContinuationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json = serde_json::to_string(grant)?;
        sqlx::query(
            "INSERT INTO continuation_grant (grant_id, notice_job_id, session_id, owner_principal, workspace_id, origin_turn_id, allowed_model, allowed_connection, max_continuations, used_continuations, max_tokens, max_tool_calls, max_duration_ms, status, expiry_ms, created_at, updated_at, record_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(grant_id) DO NOTHING",
        )
        .bind(&grant.grant_id)
        .bind(&grant.notice_job_id)
        .bind(&grant.session_id)
        .bind(&grant.owner_principal)
        .bind(&grant.workspace_id)
        .bind(&grant.origin_turn_id)
        .bind(&grant.allowed_model)
        .bind(&grant.allowed_connection)
        .bind(grant.max_continuations as i64)
        .bind(grant.used_continuations as i64)
        .bind(grant.max_tokens.map(|v| v as i64))
        .bind(grant.max_tool_calls.map(|v| v as i64))
        .bind(grant.max_duration_ms)
        .bind(grant_status_to_str(grant.status))
        .bind(grant.expiry_ms)
        .bind(grant.created_at)
        .bind(grant.updated_at)
        .bind(record_json)
        .execute(pool)
        .await
        .map_err(|e| ContinuationError::Storage(e.to_string()))?;
        Ok(())
    }

    pub(crate) async fn bump_grant_use(&self, grant_id: &str) -> Result<(), ContinuationError> {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let _ = sqlx::query(
                "UPDATE continuation_grant SET used_continuations = used_continuations + 1, updated_at = ?2 WHERE grant_id = ?1",
            )
            .bind(grant_id)
            .bind(now)
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
        }
        let mut grants = self.grants.write().await;
        if let Some(g) = grants.get_mut(grant_id) {
            g.used_continuations = g.used_continuations.saturating_add(1);
            g.updated_at = now;
            if g.used_continuations >= g.max_continuations {
                g.status = ContinuationGrantStatus::Consumed;
                if let Some(pool) = &self.pool {
                    let _ = sqlx::query(
                        "UPDATE continuation_grant SET status = 'consumed', updated_at = ?2 WHERE grant_id = ?1",
                    )
                    .bind(grant_id)
                    .bind(now)
                    .execute(pool)
                    .await;
                }
            }
        }
        Ok(())
    }

    // ── Intent CAS ──────────────────────────────────────────────────────

    fn intent_id(notice_id: &str, grant_id: &str) -> String {
        format!("{notice_id}+{grant_id}")
    }

    pub async fn get_intent(
        &self,
        notice_id: &str,
        grant_id: &str,
    ) -> Result<Option<IdleContinuationIntent>, ContinuationError> {
        let id = Self::intent_id(notice_id, grant_id);
        {
            let intents = self.intents.read().await;
            if let Some(i) = intents.get(&id) {
                return Ok(Some(i.clone()));
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM idle_continuation WHERE intent_id = ?",
            )
            .bind(&id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?
            {
                let mut intent: IdleContinuationIntent = serde_json::from_str(&json)?;
                let state_row: Option<String> =
                    sqlx::query_scalar("SELECT state FROM idle_continuation WHERE intent_id = ?")
                        .bind(&id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| ContinuationError::Storage(e.to_string()))?;
                if let Some(s) = state_row.as_deref().and_then(intent_state_from_str) {
                    intent.state = s;
                }
                // Never overwrite (see get_grant above): the race loser
                // who read before the winner's write must not clobber the
                // winner's entry (e.g. a recorded turn id) with stale data.
                let mut intents = self.intents.write().await;
                let entry = intents.entry(id.clone()).or_insert(intent);
                return Ok(Some(entry.clone()));
            }
        }
        Ok(None)
    }

    /// All intents for one notice (replay/audit path). DB-authoritative when
    /// pooled; in-memory otherwise. Used to report `AlreadyDispatched`
    /// without requiring a still-usable grant (the grant is consumed by the
    /// first dispatch).
    pub async fn intents_for_notice(
        &self,
        notice_id: &str,
    ) -> Result<Vec<IdleContinuationIntent>, ContinuationError> {
        if let Some(pool) = &self.pool {
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT intent_id FROM idle_continuation WHERE notice_id = ? LIMIT 16",
            )
            .bind(notice_id)
            .fetch_all(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(json) = sqlx::query_scalar::<_, String>(
                    "SELECT record_json FROM idle_continuation WHERE intent_id = ?",
                )
                .bind(&id)
                .fetch_optional(pool)
                .await
                .map_err(|e| ContinuationError::Storage(e.to_string()))?
                {
                    let mut intent: IdleContinuationIntent = serde_json::from_str(&json)?;
                    let state_row: Option<String> = sqlx::query_scalar(
                        "SELECT state FROM idle_continuation WHERE intent_id = ?",
                    )
                    .bind(&id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| ContinuationError::Storage(e.to_string()))?;
                    if let Some(s) = state_row.as_deref().and_then(intent_state_from_str) {
                        intent.state = s;
                    }
                    // Never overwrite (see get_grant above); report the
                    // cached entry so a concurrent admission's recorded
                    // turn id is never replaced by a stale read.
                    let mut intents = self.intents.write().await;
                    let entry = intents.entry(id.clone()).or_insert(intent);
                    out.push(entry.clone());
                }
            }
            return Ok(out);
        }
        let intents = self.intents.read().await;
        Ok(intents
            .values()
            .filter(|i| i.notice_id == notice_id)
            .cloned()
            .collect())
    }

    pub(crate) async fn insert_intent_pending(
        &self,
        notice_id: &str,
        grant_id: &str,
        session_id: &str,
    ) -> Result<bool, ContinuationError> {
        let id = Self::intent_id(notice_id, grant_id);
        let now = chrono::Utc::now().timestamp_millis();
        let intent = IdleContinuationIntent {
            intent_id: id.clone(),
            notice_id: notice_id.to_string(),
            grant_id: grant_id.to_string(),
            session_id: session_id.to_string(),
            state: IdleContinuationState::Pending,
            turn_id: None,
            cause: Some("completed_background_job".to_string()),
            created_at: now,
            updated_at: now,
        };
        if let Some(pool) = &self.pool {
            let record_json = serde_json::to_string(&intent)?;
            let result = sqlx::query(
                "INSERT INTO idle_continuation (intent_id, notice_id, grant_id, session_id, state, turn_id, cause, created_at, updated_at, record_json) \
                 VALUES (?, ?, ?, ?, 'pending', NULL, 'completed_background_job', ?, ?, ?) ON CONFLICT(intent_id) DO NOTHING",
            )
            .bind(&id)
            .bind(notice_id)
            .bind(grant_id)
            .bind(session_id)
            .bind(now)
            .bind(now)
            .bind(record_json)
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
            if result.rows_affected() > 0 {
                let mut intents = self.intents.write().await;
                intents.insert(id, intent);
                return Ok(true);
            }
            let _ = self.get_intent(notice_id, grant_id).await;
            return Ok(false);
        }
        let mut intents = self.intents.write().await;
        if intents.contains_key(&id) {
            return Ok(false);
        }
        intents.insert(id, intent);
        Ok(true)
    }

    pub(crate) async fn transition_intent(
        &self,
        notice_id: &str,
        grant_id: &str,
        from: IdleContinuationState,
        to: IdleContinuationState,
    ) -> Result<bool, ContinuationError> {
        let id = Self::intent_id(notice_id, grant_id);
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let result = sqlx::query(
                "UPDATE idle_continuation SET state = ?2, updated_at = ?3 WHERE intent_id = ?1 AND state = ?4",
            )
            .bind(&id)
            .bind(intent_state_to_str(to))
            .bind(now)
            .bind(intent_state_to_str(from))
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
            if result.rows_affected() > 0 {
                let mut intents = self.intents.write().await;
                if let Some(i) = intents.get_mut(&id) {
                    i.state = to;
                    i.updated_at = now;
                }
                return Ok(true);
            }
            let _ = self.get_intent(notice_id, grant_id).await;
            return Ok(false);
        }
        let mut intents = self.intents.write().await;
        let Some(i) = intents.get_mut(&id) else {
            return Ok(false);
        };
        if i.state != from {
            return Ok(false);
        }
        i.state = to;
        i.updated_at = now;
        Ok(true)
    }

    pub(crate) async fn set_intent_turn(
        &self,
        notice_id: &str,
        grant_id: &str,
        turn_id: &str,
    ) -> Result<(), ContinuationError> {
        let id = Self::intent_id(notice_id, grant_id);
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let _ = sqlx::query(
                "UPDATE idle_continuation SET turn_id = ?2, updated_at = ?3 WHERE intent_id = ?1",
            )
            .bind(&id)
            .bind(turn_id)
            .bind(now)
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
        }
        let mut intents = self.intents.write().await;
        if let Some(i) = intents.get_mut(&id) {
            i.turn_id = Some(turn_id.to_string());
            i.updated_at = now;
        }
        Ok(())
    }

    pub(crate) async fn mark_intent(
        &self,
        notice_id: &str,
        grant_id: &str,
        to: IdleContinuationState,
    ) -> Result<(), ContinuationError> {
        let id = Self::intent_id(notice_id, grant_id);
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let _ = sqlx::query(
                "UPDATE idle_continuation SET state = ?2, updated_at = ?3 WHERE intent_id = ?1",
            )
            .bind(&id)
            .bind(intent_state_to_str(to))
            .bind(now)
            .execute(pool)
            .await
            .map_err(|e| ContinuationError::Storage(e.to_string()))?;
        }
        let mut intents = self.intents.write().await;
        if let Some(i) = intents.get_mut(&id) {
            i.state = to;
            i.updated_at = now;
        }
        Ok(())
    }
}

/// Idle-dispatch coordinator. Owns the grant/intent stores plus the
/// injected turn-submission boundary; it never constructs provider requests
/// itself.
pub struct IdleContinuationCoordinator {
    service: Arc<ContinuationService>,
    executor: Arc<dyn ContinuationExecutor>,
}

impl IdleContinuationCoordinator {
    pub fn new(service: Arc<ContinuationService>, executor: Arc<dyn ContinuationExecutor>) -> Self {
        Self { service, executor }
    }

    pub fn service(&self) -> &Arc<ContinuationService> {
        &self.service
    }

    /// Idempotent idle dispatch for one terminal notice.
    ///
    /// Callers supply the durable notice identity plus the host-verified
    /// terminal summary and the expected owner lineage. Every mismatch fails
    /// closed to [`DispatchOutcome::Suppressed`] with the notice retained;
    /// ambiguity after provider submission becomes `NeedsAttention`.
    #[allow(clippy::too_many_arguments)]
    pub async fn maybe_dispatch(
        &self,
        notice_id: &str,
        notice_job_id: &str,
        session_id: &str,
        owner_principal: Option<&str>,
        workspace_id: Option<&str>,
        terminal_status: &str,
        summary: &str,
        disposition_is_resume: bool,
        owner_idle: bool,
        provider_available: bool,
    ) -> DispatchOutcome {
        // Default is notify-only: only an explicit resume opt-in may proceed.
        if !disposition_is_resume {
            return DispatchOutcome::Suppressed;
        }
        // Only resumable terminal statuses may continue. Cancel/expiry/
        // suppression/attention never resume; a failed build resumes only
        // when explicitly granted (the grant below), never from status text.
        if !matches!(terminal_status, "completed" | "failed" | "timed_out") {
            return DispatchOutcome::Suppressed;
        }
        if !owner_idle || !provider_available {
            return DispatchOutcome::Suppressed;
        }
        // Replay short-circuit: an already-admitted intent reports its
        // disposition without requiring a still-usable grant (the first
        // dispatch consumes the grant budget). This keeps exact-once replay
        // safe after `bump_grant_use` marks the grant consumed.
        if let Ok(intents) = self.service.intents_for_notice(notice_id).await {
            for intent in intents {
                match intent.state {
                    IdleContinuationState::Completed
                    | IdleContinuationState::TurnAccepted
                    | IdleContinuationState::Started
                    | IdleContinuationState::Claimed
                    | IdleContinuationState::Pending
                    | IdleContinuationState::Eligible => {
                        return DispatchOutcome::AlreadyDispatched;
                    }
                    IdleContinuationState::NeedsAttention => {
                        return DispatchOutcome::NeedsAttention;
                    }
                    _ => {}
                }
            }
        }
        // A valid, usable grant for this exact background identity is required.
        // A model-supplied `resume_owner` without a grant is notify-only.
        let grant = match self.service.grant_for_job(notice_job_id).await {
            Ok(Some(g)) => g,
            _ => return DispatchOutcome::Suppressed,
        };
        // Bind the grant to this notice's owner lineage. Any mismatch fails
        // closed (archived session, changed principal, wrong workspace, wrong
        // watch/repo all surface here because the grant was bound at authorize
        // time to the originating lineage).
        if grant.session_id != session_id {
            return DispatchOutcome::Suppressed;
        }
        if let Some(expected) = grant.owner_principal.as_deref() {
            if owner_principal != Some(expected) {
                return DispatchOutcome::Suppressed;
            }
        } else if owner_principal.is_some() {
            // Grant was issued without a principal binding; a newly present
            // principal is a lineage change — fail closed.
            return DispatchOutcome::Suppressed;
        }
        if let Some(expected) = grant.workspace_id.as_deref() {
            if workspace_id != Some(expected) {
                return DispatchOutcome::Suppressed;
            }
        }
        let now = chrono::Utc::now().timestamp_millis();
        if now > grant.expiry_ms {
            return DispatchOutcome::Suppressed;
        }
        // Exactly-once claim: first-write-wins on (notice, grant), then
        // Pending→Claimed CAS. A replay or a second frontend loses here.
        let first_write = match self
            .service
            .insert_intent_pending(notice_id, &grant.grant_id, session_id)
            .await
        {
            Ok(v) => v,
            Err(_) => return DispatchOutcome::Suppressed,
        };
        if !first_write {
            // An intent already exists: report its disposition without
            // dispatching again. Any non-terminal existing intent means a
            // concurrent winner already claimed first-write.
            match self.service.get_intent(notice_id, &grant.grant_id).await {
                Ok(Some(i)) => match i.state {
                    IdleContinuationState::NeedsAttention => {
                        return DispatchOutcome::NeedsAttention;
                    }
                    IdleContinuationState::Suppressed | IdleContinuationState::Expired => {
                        return DispatchOutcome::Suppressed;
                    }
                    _ => {
                        return DispatchOutcome::AlreadyDispatched;
                    }
                },
                _ => return DispatchOutcome::Suppressed,
            }
        }
        // We won the first write; CAS to Claimed (guards a raced insert).
        match self
            .service
            .transition_intent(
                notice_id,
                &grant.grant_id,
                IdleContinuationState::Pending,
                IdleContinuationState::Claimed,
            )
            .await
        {
            Ok(true) => {}
            _ => return DispatchOutcome::AlreadyDispatched,
        }
        // Persist the accepted turn identity + cause BEFORE provider dispatch,
        // so a crash after submission is distinguishable from a crash before.
        let ctx = ContinuationContext {
            notice_id: notice_id.to_string(),
            grant_id: grant.grant_id.clone(),
            session_id: session_id.to_string(),
            summary: summary.to_string(),
            model: grant.allowed_model.clone(),
        };
        // Pre-generate the turn id deterministically per intent so a replay
        // after TurnAccepted cannot mint a second turn.
        let turn_id = format!(
            "idlet-{:x}",
            sha2::Sha256::digest(format!("{notice_id}|{}", grant.grant_id).as_bytes())
        );
        let _ = self
            .service
            .set_intent_turn(notice_id, &grant.grant_id, &turn_id)
            .await;
        match self
            .service
            .transition_intent(
                notice_id,
                &grant.grant_id,
                IdleContinuationState::Claimed,
                IdleContinuationState::TurnAccepted,
            )
            .await
        {
            Ok(true) => {}
            _ => return DispatchOutcome::AlreadyDispatched,
        }
        // Dispatch through the canonical executor boundary.
        match self.executor.execute_turn(ctx).await {
            Ok(_executor_turn) => {
                let _ = self.service.bump_grant_use(&grant.grant_id).await;
                let _ = self
                    .service
                    .mark_intent(notice_id, &grant.grant_id, IdleContinuationState::Completed)
                    .await;
                DispatchOutcome::Dispatched
            }
            Err(_) => {
                // Ambiguous provider-side start: never replay.
                let _ = self
                    .service
                    .mark_intent(
                        notice_id,
                        &grant.grant_id,
                        IdleContinuationState::NeedsAttention,
                    )
                    .await;
                DispatchOutcome::NeedsAttention
            }
        }
    }
}
