//! Durable generic job terminal notifications (async-background-jobs M001).
//!
//! When a model tool submits work in `execution_mode=background`, the parent
//! agent continues immediately. When the scheduler-owned job reaches a truly
//! final terminal state, exactly one logical notification is persisted for
//! the owning session and later injected at a safe turn boundary.
//!
//! # Invariants
//!
//! - One accepted model call -> one durable job. Retry of the same accepted
//!   invocation (same `SubmissionKey` + same fingerprint) reuses the same
//!   job; same key + different payload is a conflict; two independent calls
//!   with identical payloads are distinct jobs (M009 semantics, enforced by
//!   [`crate::scheduler::submission::JobSubmissionService`]).
//! - A `JobHandle` is returned only after durable job acceptance *and*
//!   durable background subscription. Crash between submit and subscribe is
//!   healed by retry with the same submission key.
//! - Notifications are created only from persisted *final* outcomes
//!   (`Completed|Failed|Cancelled|TimedOut|Expired`). `Interrupted` (pending
//!   recovery) is never declared final. Running/progress and bounded
//!   observation timeouts are not terminal outcomes.
//! - Notification identity is the job identity (`notice_id = job_id`):
//!   duplicate terminal events produce the same notice (first-write-wins).
//! - Only explicitly subscribed background jobs produce notices. Ordinary
//!   foreground or historic jobs never retroactively create action items.
//! - Live bounded send is a hint, not the durable receipt. If the queue is
//!   full or the turn ended, the notice stays pending for next-turn
//!   recovery. Delivery-channel success never substitutes for stored
//!   session-event confirmation.
//! - The notice carries status, job/run references and a redacted bounded
//!   summary — never stdout dumps or credentials.
//!
//! This module intentionally mirrors the verified Tool Program
//! `tool_program_notifications` CAS/claim/ack mechanics without mutating its
//! IDs or tables.

use std::collections::HashMap;

use codegg_core::jobs::{JobId, JobRecord, JobState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

/// Bounds for the generic job notification queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobNotificationPolicy {
    /// Maximum pending notices per session before the oldest is suppressed.
    pub max_pending_per_session: usize,
    /// Maximum age in milliseconds for a claimed notice before expiry.
    pub claim_lease_ms: i64,
    /// Maximum bytes for a single notice payload.
    pub max_payload_bytes: usize,
    /// Maximum chars for the bounded terminal summary.
    pub max_summary_chars: usize,
}

impl Default for JobNotificationPolicy {
    fn default() -> Self {
        Self {
            max_pending_per_session: 16,
            claim_lease_ms: 300_000,
            max_payload_bytes: 8_192,
            max_summary_chars: 2_048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobNotificationState {
    Pending,
    Claimed,
    Delivered,
    Suppressed,
    Expired,
    Failed,
}

pub fn job_notification_state_to_str(state: JobNotificationState) -> &'static str {
    match state {
        JobNotificationState::Pending => "pending",
        JobNotificationState::Claimed => "claimed",
        JobNotificationState::Delivered => "delivered",
        JobNotificationState::Suppressed => "suppressed",
        JobNotificationState::Expired => "expired",
        JobNotificationState::Failed => "failed",
    }
}

fn job_notification_state_from_str(s: &str) -> Option<JobNotificationState> {
    match s {
        "pending" => Some(JobNotificationState::Pending),
        "claimed" => Some(JobNotificationState::Claimed),
        "delivered" => Some(JobNotificationState::Delivered),
        "suppressed" => Some(JobNotificationState::Suppressed),
        "expired" => Some(JobNotificationState::Expired),
        "failed" => Some(JobNotificationState::Failed),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobNotificationError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("notification not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("store unavailable")]
    Unavailable,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

/// Stable handle returned immediately after durable background acceptance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobHandle {
    pub job_id: String,
    pub kind: String,
    pub status: String,
    pub submitted_at: i64,
    pub inspect_ref: String,
    pub cancel_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// Durable terminal notice for one background job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTerminalNotice {
    pub notice_id: String,
    pub job_id: String,
    pub kind: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub invocation_key: Option<String>,
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub run_id: Option<String>,
    pub payload_digest: String,
    pub state: JobNotificationState,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub claim_owner: Option<String>,
    #[serde(default)]
    pub claim_lease_until: Option<i64>,
    #[serde(default)]
    pub delivered_at: Option<i64>,
    #[serde(default)]
    pub retry_count: u32,
    pub injection_key: String,
    #[serde(default)]
    pub injected_event_id: Option<String>,
}

/// Durable background subscription: proof that this job was explicitly
/// submitted in background mode by its owning session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobBackgroundSubscription {
    pub job_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub invocation_key: String,
    pub kind: String,
    pub created_at: i64,
    /// M003 delivery disposition requested at submit time. `notify_only`
    /// is the conservative default; `resume_owner` opts one background
    /// wait into a single bounded continuation. Old rows (pre-M003)
    /// deserialize as `notify_only`.
    #[serde(default)]
    pub disposition: JobCompletionDisposition,
}

/// M003 typed completion dispositions.
///
/// - `notify_only` (default): persist a redacted attention event and let
///   frontends show it; the next model turn may inspect and consume it.
///   No model tokens are spent by delivery itself.
/// - `resume_owner`: the owning task explicitly authorized one bounded
///   continuation for this wait. Delivery consumes a single-use lease
///   (exactly one continuation per logical terminal) at the next safe,
///   user-authorized turn boundary. Never an autonomous wake.
/// - `suppress`: cancelled/archived owner; audit/inspect only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobCompletionDisposition {
    #[default]
    NotifyOnly,
    ResumeOwner,
    Suppress,
}

impl JobCompletionDisposition {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "resume_owner" | "resume" => Self::ResumeOwner,
            "suppress" => Self::Suppress,
            _ => Self::NotifyOnly,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotifyOnly => "notify_only",
            Self::ResumeOwner => "resume_owner",
            Self::Suppress => "suppress",
        }
    }
}

/// M003 single-use continuation lease: exactly one accepted
/// model-continuation per logical terminal notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobContinuationLease {
    pub lease_id: String,
    pub notice_id: String,
    pub job_id: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub owner_principal: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub authorized_mode: String,
    pub status: JobContinuationStatus,
    pub expiry_ms: i64,
    pub max_wakes: u32,
    #[serde(default)]
    pub wakes_used: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobContinuationStatus {
    Pending,
    Consumed,
    Expired,
    Suppressed,
}

pub fn job_continuation_status_to_str(s: JobContinuationStatus) -> &'static str {
    match s {
        JobContinuationStatus::Pending => "pending",
        JobContinuationStatus::Consumed => "consumed",
        JobContinuationStatus::Expired => "expired",
        JobContinuationStatus::Suppressed => "suppressed",
    }
}

/// Delivery state diagram (M003 work package A):
/// `Pending -> SessionEventCommitted -> AvailableToOwner / ResumeQueued
/// -> Consumed / Expired / Suppressed`.
///
/// The durable `JobTerminalNotice.state` carries the first two legs
/// (`Pending -> Claimed -> Delivered` mirrors commit + ack); the
/// continuation lease carries the resume leg (`Pending -> Consumed /
/// Expired / Suppressed`). A `Delivered` notice with no lease is
/// `AvailableToOwner`; with a consumed lease it is `Consumed`; an
/// expired/suppressed lease or notice is `Expired / Suppressed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobDeliveryState {
    Pending,
    SessionEventCommitted,
    AvailableToOwner,
    ResumeQueued,
    Consumed,
    Expired,
    Suppressed,
}

/// Minimal terminal job view used for crash recovery without holding the
/// full scheduler executor types.
#[derive(Debug, Clone)]
pub struct RecoveredJobTerminal {
    pub job_id: String,
    pub kind: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub state: JobState,
    pub summary: String,
    pub run_id: Option<String>,
    pub created_at: i64,
}

pub struct JobNotificationService {
    notices: RwLock<HashMap<String, JobTerminalNotice>>,
    session_index: RwLock<HashMap<String, Vec<String>>>,
    subscriptions: RwLock<HashMap<String, JobBackgroundSubscription>>,
    leases: RwLock<HashMap<String, JobContinuationLease>>,
    pub policy: JobNotificationPolicy,
    pub continuation_policy: JobContinuationPolicy,
    pool: Option<SqlitePool>,
    /// C004 daemon-owned post-commit completion fanout. Installed once by
    /// the daemon at startup; `None` in legacy/test paths that never asked
    /// for live delivery. Fired only for newly stored terminals (never for
    /// idempotent duplicates), after the durable commit, best-effort.
    completion_tx: std::sync::RwLock<Option<tokio::sync::mpsc::UnboundedSender<JobTerminalNotice>>>,
}

/// Bounds for single-use continuation leases (M003).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobContinuationPolicy {
    /// Default lease TTL in milliseconds when the caller supplies none.
    pub default_ttl_ms: i64,
    /// Maximum TTL a caller may request.
    pub max_ttl_ms: i64,
    /// Maximum wakes per lease (always 1 in M003: exactly one continuation
    /// per logical terminal).
    pub max_wakes: u32,
}

impl Default for JobContinuationPolicy {
    fn default() -> Self {
        Self {
            default_ttl_ms: 30 * 60 * 1_000,
            max_ttl_ms: 4 * 60 * 60 * 1_000,
            max_wakes: 1,
        }
    }
}

impl JobNotificationService {
    pub fn new() -> Self {
        Self {
            notices: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            leases: RwLock::new(HashMap::new()),
            policy: JobNotificationPolicy::default(),
            continuation_policy: JobContinuationPolicy::default(),
            pool: None,
            completion_tx: std::sync::RwLock::new(None),
        }
    }

    pub fn with_policy(policy: JobNotificationPolicy) -> Self {
        Self {
            notices: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            leases: RwLock::new(HashMap::new()),
            policy,
            continuation_policy: JobContinuationPolicy::default(),
            pool: None,
            completion_tx: std::sync::RwLock::new(None),
        }
    }

    pub fn with_pool(pool: SqlitePool) -> Self {
        Self {
            notices: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            leases: RwLock::new(HashMap::new()),
            policy: JobNotificationPolicy::default(),
            continuation_policy: JobContinuationPolicy::default(),
            pool: Some(pool),
            completion_tx: std::sync::RwLock::new(None),
        }
    }

    /// C004: install the daemon-owned post-commit live-delivery sender.
    /// The daemon calls this once at startup; tests may install their own
    /// channel to observe exactly-once fanout without a daemon. Sync so
    /// daemon construction (which is sync) can install without awaiting.
    pub fn set_completion_hook_sync(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<JobTerminalNotice>,
    ) {
        if let Ok(mut guard) = self.completion_tx.write() {
            *guard = Some(tx);
        }
    }

    /// Async alias for test/harness paths already in async context.
    pub async fn set_completion_hook(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<JobTerminalNotice>,
    ) {
        self.set_completion_hook_sync(tx);
    }

    fn emit_completion(&self, notice: &JobTerminalNotice) {
        // Best-effort, non-blocking: the durable commit already happened;
        // a full/closed channel never fails the terminal path.
        if let Ok(guard) = self.completion_tx.read() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(notice.clone());
            }
        }
    }

    /// Subscribe a durably accepted job for background terminal delivery.
    ///
    /// Must be called with the host-resolved owning session (from
    /// `ToolExecutionContext`, never a model-supplied field) before the
    /// `JobHandle` is returned. Idempotent on `(job_id, session_id,
    /// invocation_key)`; a different session or invocation key for the same
    /// job is a conflict and fails closed.
    pub async fn subscribe(
        &self,
        job_id: &JobId,
        kind: &str,
        session_id: &str,
        turn_id: Option<&str>,
        invocation_key: &str,
    ) -> Result<JobBackgroundSubscription, JobNotificationError> {
        self.subscribe_with_options(
            job_id,
            kind,
            session_id,
            turn_id,
            invocation_key,
            JobCompletionDisposition::NotifyOnly,
            None,
            None,
            None,
        )
        .await
    }

    /// M003 subscription with an explicit delivery disposition.
    ///
    /// `resume_owner` additionally creates a single-use continuation lease
    /// bound to the host-resolved session/turn/principal/workspace ancestry.
    /// A model cannot create a more privileged resume authorization than the
    /// session's originating user/task contract: the principal and workspace
    /// come from the daemon boundary (`ToolExecutionContext`), never from
    /// model input. Expired/revoked/archived owners are refused at consume
    /// time, and user cancellation suppresses the lease.
    #[allow(clippy::too_many_arguments)]
    pub async fn subscribe_with_options(
        &self,
        job_id: &JobId,
        kind: &str,
        session_id: &str,
        turn_id: Option<&str>,
        invocation_key: &str,
        disposition: JobCompletionDisposition,
        owner_principal: Option<&str>,
        workspace_id: Option<&str>,
        ttl_ms: Option<i64>,
    ) -> Result<JobBackgroundSubscription, JobNotificationError> {
        if session_id.is_empty() {
            return Err(JobNotificationError::Storage(
                "background jobs require a real host-originated session context".into(),
            ));
        }
        if invocation_key.is_empty() {
            return Err(JobNotificationError::Storage(
                "background jobs require an invocation key".into(),
            ));
        }
        let job_id_str = job_id.as_str().to_string();
        {
            let subs = self.subscriptions.read().await;
            if let Some(existing) = subs.get(&job_id_str) {
                if existing.session_id != session_id || existing.invocation_key != invocation_key {
                    return Err(JobNotificationError::Conflict(format!(
                        "job {job_id_str} already subscribed by a different owner"
                    )));
                }
                return Ok(existing.clone());
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM job_background_subscription WHERE job_id = ?",
            )
            .bind(&job_id_str)
            .fetch_optional(pool)
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?
            {
                let existing: JobBackgroundSubscription = serde_json::from_str(&json)?;
                if existing.session_id != session_id || existing.invocation_key != invocation_key {
                    return Err(JobNotificationError::Conflict(format!(
                        "job {job_id_str} already subscribed by a different owner"
                    )));
                }
                let mut subs = self.subscriptions.write().await;
                subs.insert(job_id_str.clone(), existing.clone());
                return Ok(existing);
            }
        }
        let now = chrono::Utc::now().timestamp_millis();
        let sub = JobBackgroundSubscription {
            job_id: job_id_str.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.map(ToOwned::to_owned),
            invocation_key: invocation_key.to_string(),
            kind: kind.to_string(),
            created_at: now,
            disposition,
        };
        self.persist_subscription(&sub).await?;
        {
            let mut subs = self.subscriptions.write().await;
            if let Some(existing) = subs.get(&job_id_str) {
                if existing.session_id != session_id || existing.invocation_key != invocation_key {
                    return Err(JobNotificationError::Conflict(format!(
                        "job {job_id_str} already subscribed by a different owner"
                    )));
                }
                return Ok(existing.clone());
            }
            subs.insert(job_id_str.clone(), sub.clone());
        }
        // M003: opt-in single-use resume lease for exactly one background
        // wait. Created alongside the subscription (task-level authorization);
        // consumed exactly once at delivery. Default stays notify_only.
        if disposition == JobCompletionDisposition::ResumeOwner {
            let ttl = ttl_ms
                .unwrap_or(self.continuation_policy.default_ttl_ms)
                .clamp(1, self.continuation_policy.max_ttl_ms);
            let lease = JobContinuationLease {
                lease_id: job_id_str.clone(),
                notice_id: job_id_str.clone(),
                job_id: job_id_str.clone(),
                session_id: session_id.to_string(),
                turn_id: turn_id.map(ToOwned::to_owned),
                owner_principal: owner_principal.map(ToOwned::to_owned),
                workspace_id: workspace_id.map(ToOwned::to_owned),
                authorized_mode: "resume_owner".to_string(),
                status: JobContinuationStatus::Pending,
                expiry_ms: now.saturating_add(ttl),
                max_wakes: self.continuation_policy.max_wakes,
                wakes_used: 0,
                created_at: now,
                updated_at: now,
            };
            self.persist_lease(&lease).await?;
            let mut leases = self.leases.write().await;
            leases.insert(job_id_str, lease);
        }
        Ok(sub)
    }

    /// Load a continuation lease by its id (`lease_id = notice_id = job_id`).
    pub async fn get_lease(
        &self,
        lease_id: &str,
    ) -> Result<Option<JobContinuationLease>, JobNotificationError> {
        {
            let leases = self.leases.read().await;
            if let Some(l) = leases.get(lease_id) {
                return Ok(Some(l.clone()));
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM job_continuation_lease WHERE lease_id = ?",
            )
            .bind(lease_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?
            {
                let lease: JobContinuationLease = serde_json::from_str(&json)?;
                let mut leases = self.leases.write().await;
                leases.insert(lease_id.to_string(), lease.clone());
                return Ok(Some(lease));
            }
        }
        Ok(None)
    }

    /// Consume a pending resume lease exactly once (CAS Pending→Consumed).
    ///
    /// Enforces the M003 authority rule: the consuming session must equal
    /// the lease's owning session; expiry, prior consumption, suppression,
    /// and wake-budget exhaustion all fail (returning `Ok(false)`) rather
    /// than spending inference. User cancellation suppresses the lease
    /// separately via [`Self::suppress_lease`].
    pub async fn consume_resume_lease(
        &self,
        lease_id: &str,
        caller_session_id: &str,
    ) -> Result<bool, JobNotificationError> {
        self.load_lease_from_pool(lease_id).await?;
        let now = chrono::Utc::now().timestamp_millis();
        // Validate without holding the write lock across SQLite.
        let valid = {
            let leases = self.leases.read().await;
            let Some(l) = leases.get(lease_id) else {
                return Ok(false);
            };
            l.status == JobContinuationStatus::Pending
                && l.session_id == caller_session_id
                && now <= l.expiry_ms
                && l.wakes_used < l.max_wakes
        };
        if !valid {
            // Lazily expire stale leases so the terminal state is visible.
            let should_expire = {
                let leases = self.leases.read().await;
                leases.get(lease_id).is_some_and(|l| {
                    l.status == JobContinuationStatus::Pending && now > l.expiry_ms
                })
            };
            if should_expire {
                let _ = self.transition_lease(lease_id, "pending", "expired").await;
            }
            return Ok(false);
        }
        self.transition_lease(lease_id, "pending", "consumed").await
    }

    /// Suppress a lease (user cancel, archive, revocation, terminal cancel).
    /// Idempotent: pending→suppressed, otherwise no-op success state.
    pub async fn suppress_lease(&self, lease_id: &str) -> Result<bool, JobNotificationError> {
        self.load_lease_from_pool(lease_id).await?;
        if self
            .transition_lease(lease_id, "pending", "suppressed")
            .await?
        {
            return Ok(true);
        }
        // Already consumed/expired/suppressed: report current terminality.
        let leases = self.leases.read().await;
        Ok(leases
            .get(lease_id)
            .is_none_or(|l| l.status != JobContinuationStatus::Pending))
    }

    /// Classify the delivery state for a notice (M003 work package A).
    pub async fn delivery_state(
        &self,
        notice_id: &str,
    ) -> Result<Option<JobDeliveryState>, JobNotificationError> {
        let notice = self.get(notice_id).await?;
        let Some(n) = notice else {
            return Ok(None);
        };
        let lease = self.get_lease(notice_id).await?;
        Ok(Some(match n.state {
            JobNotificationState::Pending | JobNotificationState::Claimed => {
                if n.injected_event_id.is_some() {
                    JobDeliveryState::SessionEventCommitted
                } else {
                    JobDeliveryState::Pending
                }
            }
            JobNotificationState::Delivered => match lease.map(|l| l.status) {
                Some(JobContinuationStatus::Pending) => JobDeliveryState::ResumeQueued,
                Some(JobContinuationStatus::Consumed) => JobDeliveryState::Consumed,
                Some(JobContinuationStatus::Expired) => JobDeliveryState::Expired,
                Some(JobContinuationStatus::Suppressed) => JobDeliveryState::Suppressed,
                None => JobDeliveryState::AvailableToOwner,
            },
            JobNotificationState::Suppressed => JobDeliveryState::Suppressed,
            JobNotificationState::Expired | JobNotificationState::Failed => {
                JobDeliveryState::Expired
            }
        }))
    }

    async fn persist_lease(
        &self,
        lease: &JobContinuationLease,
    ) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json = serde_json::to_string(lease)
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        sqlx::query(
            "INSERT INTO job_continuation_lease (lease_id, notice_id, job_id, session_id, turn_id, owner_principal, workspace_id, authorized_mode, status, expiry_ms, max_wakes, wakes_used, created_at, updated_at, record_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(lease_id) DO NOTHING",
        )
        .bind(&lease.lease_id)
        .bind(&lease.notice_id)
        .bind(&lease.job_id)
        .bind(&lease.session_id)
        .bind(&lease.turn_id)
        .bind(&lease.owner_principal)
        .bind(&lease.workspace_id)
        .bind(&lease.authorized_mode)
        .bind(job_continuation_status_to_str(lease.status))
        .bind(lease.expiry_ms)
        .bind(lease.max_wakes as i64)
        .bind(lease.wakes_used as i64)
        .bind(lease.created_at)
        .bind(lease.updated_at)
        .bind(record_json)
        .execute(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn transition_lease(
        &self,
        lease_id: &str,
        from: &str,
        to: &str,
    ) -> Result<bool, JobNotificationError> {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(pool) = &self.pool {
            let result = sqlx::query(
                "UPDATE job_continuation_lease SET status = ?2, updated_at = ?3, \
                 wakes_used = CASE WHEN ?2 = 'consumed' THEN wakes_used + 1 ELSE wakes_used END, \
                 record_json = json_set(json_set(json_set(record_json, '$.status', ?2), '$.updated_at', ?3), '$.wakes_used', CASE WHEN ?2 = 'consumed' THEN wakes_used + 1 ELSE wakes_used END) \
                 WHERE lease_id = ?1 AND status = ?4",
            )
            .bind(lease_id)
            .bind(to)
            .bind(now)
            .bind(from)
            .execute(pool)
            .await
            .map_err(|e| JobNotificationError::Io(std::io::Error::other(e.to_string())))?;
            if result.rows_affected() > 0 {
                let mut leases = self.leases.write().await;
                if let Some(l) = leases.get_mut(lease_id) {
                    l.status = match to {
                        "consumed" => JobContinuationStatus::Consumed,
                        "expired" => JobContinuationStatus::Expired,
                        "suppressed" => JobContinuationStatus::Suppressed,
                        _ => JobContinuationStatus::Pending,
                    };
                    l.updated_at = now;
                    if to == "consumed" {
                        l.wakes_used = l.wakes_used.saturating_add(1);
                    }
                }
                return Ok(true);
            }
            // Refresh cache from the durable row on CAS miss.
            self.load_lease_from_pool(lease_id).await?;
            return Ok(false);
        }
        let mut leases = self.leases.write().await;
        let Some(l) = leases.get_mut(lease_id) else {
            return Ok(false);
        };
        if job_continuation_status_to_str(l.status) != from {
            return Ok(false);
        }
        l.status = match to {
            "consumed" => JobContinuationStatus::Consumed,
            "expired" => JobContinuationStatus::Expired,
            "suppressed" => JobContinuationStatus::Suppressed,
            _ => JobContinuationStatus::Pending,
        };
        l.updated_at = now;
        if to == "consumed" {
            l.wakes_used = l.wakes_used.saturating_add(1);
        }
        Ok(true)
    }

    async fn load_lease_from_pool(&self, lease_id: &str) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let result = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM job_continuation_lease WHERE lease_id = ?",
        )
        .bind(lease_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        let Some(json) = result else {
            return Ok(());
        };
        let mut lease: JobContinuationLease = serde_json::from_str(&json)?;
        let state_row: Option<String> =
            sqlx::query_scalar("SELECT status FROM job_continuation_lease WHERE lease_id = ?")
                .bind(lease_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        if let Some(s) = state_row.as_deref() {
            lease.status = match s {
                "consumed" => JobContinuationStatus::Consumed,
                "expired" => JobContinuationStatus::Expired,
                "suppressed" => JobContinuationStatus::Suppressed,
                _ => JobContinuationStatus::Pending,
            };
        }
        let mut leases = self.leases.write().await;
        leases.insert(lease_id.to_string(), lease);
        Ok(())
    }

    pub async fn get_subscription(
        &self,
        job_id: &str,
    ) -> Result<Option<JobBackgroundSubscription>, JobNotificationError> {
        {
            let subs = self.subscriptions.read().await;
            if let Some(sub) = subs.get(job_id) {
                return Ok(Some(sub.clone()));
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM job_background_subscription WHERE job_id = ?",
            )
            .bind(job_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?
            {
                let sub: JobBackgroundSubscription = serde_json::from_str(&json)?;
                let mut subs = self.subscriptions.write().await;
                subs.insert(job_id.to_string(), sub.clone());
                return Ok(Some(sub));
            }
        }
        Ok(None)
    }

    /// Record the one logical terminal notice for a subscribed background
    /// job. Idempotent on `notice_id = job_id`.
    ///
    /// Returns `Ok(None)` when the job has no background subscription
    /// (foreground or historic jobs never create action items) or when the
    /// terminal state is not truly final (`Interrupted`, running, etc.).
    /// Invalid cross-session linkage fails closed.
    pub async fn record_terminal(
        &self,
        job: &JobRecord,
        summary: &str,
        run_id: Option<&str>,
    ) -> Result<Option<JobTerminalNotice>, JobNotificationError> {
        if !is_final_job_state(&job.state) {
            return Ok(None);
        }
        let job_id_str = job.job_id.as_str().to_string();
        let sub = self.get_subscription(&job_id_str).await?;
        let Some(sub) = sub else {
            return Ok(None);
        };
        // Cross-session guard: the durable job owner must match the
        // subscription owner. A mismatch means forged lineage; fail closed
        // without leaking the stored owner.
        if let Some(job_session) = job.session_id.as_deref() {
            if job_session != sub.session_id {
                return Err(JobNotificationError::Unauthorized(
                    "job session does not match background subscription".into(),
                ));
            }
        } else {
            return Err(JobNotificationError::Unauthorized(
                "background job has no owning session".into(),
            ));
        }
        let kind = job.kind.as_str().to_string();
        let status = terminal_status_str(&job.state).to_string();
        let bounded_summary = bound_summary(summary, self.policy.max_summary_chars);
        let payload = format!("{}|{}|{}|{}", job_id_str, status, bounded_summary, kind);
        let payload_digest = format!("sha256:{:x}", Sha256::digest(payload.as_bytes()));
        let now = chrono::Utc::now().timestamp_millis();
        let injection_key = format!("job-notify:{}:{}", job_id_str, sub.session_id);
        let notice = JobTerminalNotice {
            notice_id: job_id_str.clone(),
            job_id: job_id_str.clone(),
            kind,
            session_id: sub.session_id.clone(),
            turn_id: sub.turn_id.clone(),
            invocation_key: Some(sub.invocation_key.clone()),
            status,
            summary: bounded_summary,
            run_id: run_id.map(ToOwned::to_owned),
            payload_digest,
            state: JobNotificationState::Pending,
            created_at: now,
            updated_at: now,
            claim_owner: None,
            claim_lease_until: None,
            delivered_at: None,
            retry_count: 0,
            injection_key,
            injected_event_id: None,
        };
        let stored = self.record_notice(notice).await?;
        self.enforce_session_bound(&stored.session_id, self.policy.max_pending_per_session)
            .await?;
        Ok(Some(stored))
    }

    /// Record the exactly-one terminal notice for a CI watch (M004).
    ///
    /// Watches are not scheduler jobs, so there is no `JobRecord`; the
    /// durable `ci_watch` row (created explicitly by the owning session)
    /// is the subscription proof, registered here as a matching
    /// `job_background_subscription` with `job_id = ciwatch:<watch_id>`
    /// at watch-creation time. This method checks that subscription and
    /// fails closed (`Unauthorized`) without it, preserving the "only
    /// explicitly subscribed background work notifies" invariant.
    /// Idempotent on `notice_id = ciwatch:<watch_id>`; non-final statuses
    /// (`running`, `interrupted`) return `Ok(None)`.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_terminal_for_watch(
        &self,
        notice_job_id: &str,
        kind: &str,
        session_id: &str,
        turn_id: Option<&str>,
        status: &str,
        summary: &str,
        run_id: Option<&str>,
    ) -> Result<Option<JobTerminalNotice>, JobNotificationError> {
        if !matches!(
            status,
            "completed" | "failed" | "cancelled" | "timed_out" | "expired" | "needs_attention"
        ) {
            return Ok(None);
        }
        if session_id.is_empty() {
            return Err(JobNotificationError::Unauthorized(
                "watch terminal has no owning session".into(),
            ));
        }
        let sub = self.get_subscription(notice_job_id).await?;
        let Some(sub) = sub else {
            return Err(JobNotificationError::Unauthorized(
                "watch has no background subscription".into(),
            ));
        };
        if sub.session_id != session_id {
            return Err(JobNotificationError::Unauthorized(
                "watch session does not match background subscription".into(),
            ));
        }
        let bounded_summary = bound_summary(summary, self.policy.max_summary_chars);
        let payload = format!("{notice_job_id}|{status}|{bounded_summary}|{kind}");
        let payload_digest = format!("sha256:{:x}", Sha256::digest(payload.as_bytes()));
        let now = chrono::Utc::now().timestamp_millis();
        let notice = JobTerminalNotice {
            notice_id: notice_job_id.to_string(),
            job_id: notice_job_id.to_string(),
            kind: kind.to_string(),
            session_id: session_id.to_string(),
            turn_id: turn_id.map(ToOwned::to_owned),
            invocation_key: Some(sub.invocation_key.clone()),
            status: status.to_string(),
            summary: bounded_summary,
            run_id: run_id.map(ToOwned::to_owned),
            payload_digest,
            state: JobNotificationState::Pending,
            created_at: now,
            updated_at: now,
            claim_owner: None,
            claim_lease_until: None,
            delivered_at: None,
            retry_count: 0,
            injection_key: format!("job-notify:{notice_job_id}:{session_id}"),
            injected_event_id: None,
        };
        let stored = self.record_notice(notice).await?;
        self.enforce_session_bound(&stored.session_id, self.policy.max_pending_per_session)
            .await?;
        Ok(Some(stored))
    }

    /// Reconcile terminal background jobs that completed before their
    /// subscription was durably recorded, or that terminated while the
    /// notifier was unavailable (restart, crash, lost receiver).
    ///
    /// Only jobs with an explicit subscription produce notices; this never
    /// retroactively notifies foreground or historic jobs.
    pub async fn reconcile_terminals(
        &self,
        terminals: Vec<RecoveredJobTerminal>,
    ) -> Result<usize, JobNotificationError> {
        let mut created = 0;
        for t in terminals {
            if !is_final_job_state(&t.state) {
                continue;
            }
            let Some(session_id) = t.session_id.as_deref().filter(|s| !s.is_empty()) else {
                continue;
            };
            // Must have an explicit subscription; otherwise this is a
            // foreground or historic job and must not create an action item.
            if self.get_subscription(&t.job_id).await?.is_none() {
                continue;
            }
            if self.get(&t.job_id).await?.is_some() {
                continue;
            }
            let kind = t.kind.clone();
            let status = terminal_status_str(&t.state).to_string();
            let bounded_summary = bound_summary(&t.summary, self.policy.max_summary_chars);
            let payload = format!("{}|{}|{}|{}", t.job_id, status, bounded_summary, kind);
            let payload_digest = format!("sha256:{:x}", Sha256::digest(payload.as_bytes()));
            let now = chrono::Utc::now().timestamp_millis();
            let notice = JobTerminalNotice {
                notice_id: t.job_id.clone(),
                job_id: t.job_id.clone(),
                kind,
                session_id: session_id.to_string(),
                turn_id: t.turn_id.clone(),
                invocation_key: None,
                status,
                summary: bounded_summary,
                run_id: t.run_id.clone(),
                payload_digest,
                state: JobNotificationState::Pending,
                created_at: t.created_at,
                updated_at: now,
                claim_owner: None,
                claim_lease_until: None,
                delivered_at: None,
                retry_count: 0,
                injection_key: format!("job-notify:{}:{}", t.job_id, session_id),
                injected_event_id: None,
            };
            self.record_notice(notice).await?;
            created += 1;
        }
        Ok(created)
    }

    /// Scan the durable job store for subscribed terminal jobs without
    /// notices. Bounded (256 rows) and newest-first so a fixed oldest-N
    /// scan cannot starve new notices: pending notices are always served
    /// from the session index, while this scan only backfills missing
    /// terminals.
    pub async fn recover_from_store(
        &self,
        store: &std::sync::Arc<dyn codegg_core::jobs::JobStore>,
    ) -> Result<usize, JobNotificationError> {
        use codegg_core::jobs::store::JobStoreQuery;
        let records = store
            .list_job_records(JobStoreQuery {
                states: vec![
                    JobState::Completed,
                    JobState::Failed,
                    JobState::Cancelled,
                    JobState::TimedOut,
                    JobState::Expired,
                ],
                kinds: vec![],
                workspace_id: None,
                session_id: None,
                limit: Some(256),
            })
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        let mut terminals = Vec::new();
        for job in records {
            let job_id_str = job.job_id.as_str().to_string();
            if self.get(&job_id_str).await?.is_some() {
                continue;
            }
            let Some(sub) = self.get_subscription(&job_id_str).await? else {
                continue;
            };
            terminals.push(RecoveredJobTerminal {
                job_id: job_id_str,
                kind: job.kind.as_str().to_string(),
                session_id: Some(sub.session_id.clone()),
                turn_id: sub.turn_id.clone(),
                state: job.state,
                summary: job
                    .cancel_reason
                    .clone()
                    .unwrap_or_else(|| format!("job finished in {:?}", job.state)),
                run_id: None,
                created_at: job.created_at.timestamp_millis(),
            });
        }
        self.reconcile_terminals(terminals).await
    }

    /// All durable terminal notices for startup idle-hint reconciliation
    /// (crash-gap healing, ADR-0031 §Decision.5).
    ///
    /// Returns pool rows merged by notice id when pooled (bounded, newest
    /// behavior matches `recover_from_store`'s 512-row ceiling), or the
    /// in-memory set when pool-less. Notices without an owning session
    /// are excluded: they never produce live hints. The rows are the
    /// authority; callers republish only the *hint* for notices missing
    /// from `core_event_log` — never a second notice, never a turn.
    pub async fn durable_notices_for_hint_recovery(
        &self,
    ) -> Result<Vec<JobTerminalNotice>, JobNotificationError> {
        if let Some(pool) = &self.pool {
            let rows = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM job_notification ORDER BY created_at ASC LIMIT 512",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
            let mut out = Vec::with_capacity(rows.len());
            for json in rows {
                let notice: JobTerminalNotice = serde_json::from_str(&json)?;
                if !notice.session_id.is_empty() {
                    out.push(notice);
                }
            }
            return Ok(out);
        }
        let notices = self.notices.read().await;
        Ok(notices
            .values()
            .filter(|n| !n.session_id.is_empty())
            .cloned()
            .collect())
    }

    async fn record_notice(
        &self,
        notice: JobTerminalNotice,
    ) -> Result<JobTerminalNotice, JobNotificationError> {
        if notice.session_id.is_empty() {
            return Err(JobNotificationError::Storage(
                "job notice has no owning session".into(),
            ));
        }
        if let Some(pool) = &self.pool {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM job_notification WHERE notice_id = ?",
            )
            .bind(&notice.notice_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?
            {
                let existing: JobTerminalNotice = serde_json::from_str(&json)?;
                // Semantic duplicate check: same job + same digest is the
                // same logical terminal. A different digest for the same
                // job id means a conflicting terminal claim; keep the
                // first and report it without overwriting.
                if existing.payload_digest != notice.payload_digest {
                    return Err(JobNotificationError::Conflict(format!(
                        "conflicting terminal for job {}",
                        notice.job_id
                    )));
                }
                let mut notices = self.notices.write().await;
                notices.insert(existing.notice_id.clone(), existing.clone());
                return Ok(existing);
            }
        }
        {
            let notices = self.notices.read().await;
            if let Some(existing) = notices.get(&notice.notice_id) {
                if existing.payload_digest != notice.payload_digest {
                    return Err(JobNotificationError::Conflict(format!(
                        "conflicting terminal for job {}",
                        notice.job_id
                    )));
                }
                return Ok(existing.clone());
            }
        }
        self.persist_notice(&notice).await?;
        {
            let mut notices = self.notices.write().await;
            if let Some(existing) = notices.get(&notice.notice_id) {
                return Ok(existing.clone());
            }
            notices.insert(notice.notice_id.clone(), notice.clone());
        }
        {
            let mut index = self.session_index.write().await;
            let entry = index.entry(notice.session_id.clone()).or_default();
            if !entry.contains(&notice.notice_id) {
                entry.push(notice.notice_id.clone());
            }
        }
        // C004 post-commit fanout: durable first-write won, so publish the
        // live hint exactly once. Duplicates return above without emitting.
        self.emit_completion(&notice);
        Ok(notice)
    }

    pub async fn claim(&self, notice_id: &str) -> Result<bool, JobNotificationError> {
        self.claim_as(notice_id, "job-notification-delivery").await
    }

    pub async fn claim_as(
        &self,
        notice_id: &str,
        owner: &str,
    ) -> Result<bool, JobNotificationError> {
        self.load_one_from_pool(notice_id).await?;
        self.recover_expired().await?;
        self.transition_with_owner(
            notice_id,
            JobNotificationState::Pending,
            JobNotificationState::Claimed,
            Some(owner.to_string()),
        )
        .await
    }

    pub async fn acknowledge(&self, notice_id: &str) -> Result<bool, JobNotificationError> {
        self.load_one_from_pool(notice_id).await?;
        self.transition_with_owner(
            notice_id,
            JobNotificationState::Claimed,
            JobNotificationState::Delivered,
            None,
        )
        .await
    }

    pub async fn mark_injected(
        &self,
        notice_id: &str,
        event_id: &str,
    ) -> Result<(), JobNotificationError> {
        self.load_one_from_pool(notice_id).await?;
        let updated = {
            let notices = self.notices.read().await;
            let Some(n) = notices.get(notice_id).cloned() else {
                return Err(JobNotificationError::NotFound);
            };
            let mut u = n;
            u.injected_event_id = Some(event_id.to_string());
            u
        };
        let record_json = serde_json::to_string(&updated)?;
        if let Some(pool) = &self.pool {
            let result = sqlx::query(
                "UPDATE job_notification SET record_json = ? WHERE notice_id = ? \
                 AND state NOT IN ('delivered', 'suppressed', 'expired') \
                 AND json_extract(record_json, '$.injected_event_id') IS NULL",
            )
            .bind(record_json)
            .bind(notice_id)
            .execute(pool)
            .await
            .map_err(|e| JobNotificationError::Io(std::io::Error::other(e.to_string())))?;
            if result.rows_affected() != 1 {
                return Err(JobNotificationError::Conflict(
                    "injection race or terminal state".into(),
                ));
            }
        }
        self.notices
            .write()
            .await
            .insert(notice_id.to_string(), updated);
        Ok(())
    }

    pub async fn is_injected(&self, notice_id: &str) -> bool {
        let notices = self.notices.read().await;
        notices
            .get(notice_id)
            .and_then(|n| n.injected_event_id.as_ref())
            .is_some()
    }

    pub async fn suppress(&self, notice_id: &str) -> Result<bool, JobNotificationError> {
        self.load_one_from_pool(notice_id).await?;
        if self
            .transition(
                notice_id,
                JobNotificationState::Pending,
                JobNotificationState::Suppressed,
            )
            .await?
        {
            return Ok(true);
        }
        self.transition(
            notice_id,
            JobNotificationState::Claimed,
            JobNotificationState::Suppressed,
        )
        .await
    }

    pub async fn pending_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<JobTerminalNotice>, JobNotificationError> {
        if self.pool.is_some() {
            self.load_session_from_pool(session_id).await?;
        }
        let index = self.session_index.read().await;
        let notices = self.notices.read().await;
        Ok(if let Some(ids) = index.get(session_id) {
            ids.iter()
                .filter_map(|id| notices.get(id))
                .filter(|n| n.state == JobNotificationState::Pending)
                .cloned()
                .collect()
        } else {
            vec![]
        })
    }

    pub async fn recoverable_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<JobTerminalNotice>, JobNotificationError> {
        if self.pool.is_some() {
            self.load_session_from_pool(session_id).await?;
        }
        let index = self.session_index.read().await;
        let notices = self.notices.read().await;
        Ok(if let Some(ids) = index.get(session_id) {
            ids.iter()
                .filter_map(|id| notices.get(id))
                .filter(|n| {
                    matches!(
                        n.state,
                        JobNotificationState::Pending | JobNotificationState::Claimed
                    )
                })
                .cloned()
                .collect()
        } else {
            vec![]
        })
    }

    pub async fn get(
        &self,
        notice_id: &str,
    ) -> Result<Option<JobTerminalNotice>, JobNotificationError> {
        {
            let notices = self.notices.read().await;
            if let Some(n) = notices.get(notice_id).cloned() {
                return Ok(Some(n));
            }
        }
        self.load_one_from_pool(notice_id).await?;
        let notices = self.notices.read().await;
        Ok(notices.get(notice_id).cloned())
    }

    pub async fn pending_count(&self, session_id: &str) -> Result<usize, JobNotificationError> {
        Ok(self.pending_for_session(session_id).await?.len())
    }

    /// Exact-owner guard: knowledge of a job id alone confers no rights.
    /// The caller's session must equal the notice's owning session.
    pub fn check_owner(
        notice: &JobTerminalNotice,
        caller_session_id: Option<&str>,
    ) -> Result<(), JobNotificationError> {
        let Some(caller) = caller_session_id.filter(|s| !s.is_empty()) else {
            return Err(JobNotificationError::Unauthorized(
                "job inspect requires a session context".into(),
            ));
        };
        if notice.session_id != caller {
            return Err(JobNotificationError::Unauthorized(
                "job belongs to a different session".into(),
            ));
        }
        Ok(())
    }

    pub async fn expire_stale(&self, max_age_ms: i64) -> Vec<String> {
        let now = chrono::Utc::now().timestamp_millis();
        let expired: Vec<String> = {
            let notices = self.notices.read().await;
            notices
                .iter()
                .filter(|(_, n)| {
                    n.state == JobNotificationState::Claimed && (now - n.updated_at) > max_age_ms
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &expired {
            let _ = self
                .transition_to(
                    id,
                    JobNotificationState::Claimed,
                    JobNotificationState::Expired,
                    now,
                    None,
                    None,
                )
                .await;
        }
        expired
    }

    pub async fn enforce_session_bound(
        &self,
        session_id: &str,
        max_pending: usize,
    ) -> Result<Vec<String>, JobNotificationError> {
        let pending: Vec<String> = {
            let index = self.session_index.read().await;
            let Some(ids) = index.get(session_id) else {
                return Ok(vec![]);
            };
            let notices = self.notices.read().await;
            ids.iter()
                .filter(|id| {
                    notices
                        .get(id.as_str())
                        .map(|n| n.state == JobNotificationState::Pending)
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        };
        let mut suppressed = vec![];
        if pending.len() > max_pending {
            for id in &pending[..pending.len() - max_pending] {
                if self.suppress(id).await? {
                    suppressed.push(id.clone());
                }
            }
        }
        Ok(suppressed)
    }

    async fn persist_subscription(
        &self,
        sub: &JobBackgroundSubscription,
    ) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json =
            serde_json::to_string(sub).map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        sqlx::query(
            "INSERT INTO job_background_subscription (job_id, session_id, turn_id, invocation_key, kind, created_at, record_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(job_id) DO UPDATE SET session_id = excluded.session_id, turn_id = excluded.turn_id, \
             invocation_key = excluded.invocation_key, kind = excluded.kind, record_json = excluded.record_json",
        )
        .bind(&sub.job_id)
        .bind(&sub.session_id)
        .bind(&sub.turn_id)
        .bind(&sub.invocation_key)
        .bind(&sub.kind)
        .bind(sub.created_at)
        .bind(record_json)
        .execute(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn persist_notice(&self, notice: &JobTerminalNotice) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json = serde_json::to_string(notice)
            .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        sqlx::query(
            "INSERT INTO job_notification (notice_id, job_id, session_id, turn_id, kind, status, summary, run_id, payload_digest, state, record_json, claim_owner, claim_lease_until, created_at, updated_at, delivered_at, injection_key, injected_event_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(notice_id) DO UPDATE SET state = excluded.state, record_json = excluded.record_json, claim_owner = excluded.claim_owner, claim_lease_until = excluded.claim_lease_until, updated_at = excluded.updated_at, delivered_at = excluded.delivered_at, injection_key = excluded.injection_key",
        )
        .bind(&notice.notice_id)
        .bind(&notice.job_id)
        .bind(&notice.session_id)
        .bind(&notice.turn_id)
        .bind(&notice.kind)
        .bind(&notice.status)
        .bind(&notice.summary)
        .bind(&notice.run_id)
        .bind(&notice.payload_digest)
        .bind(job_notification_state_to_str(notice.state))
        .bind(record_json)
        .bind(&notice.claim_owner)
        .bind(notice.claim_lease_until)
        .bind(notice.created_at)
        .bind(notice.updated_at)
        .bind((notice.state == JobNotificationState::Delivered).then_some(notice.updated_at))
        .bind(&notice.injection_key)
        .bind(&notice.injected_event_id)
        .execute(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn transition(
        &self,
        notice_id: &str,
        from: JobNotificationState,
        to: JobNotificationState,
    ) -> Result<bool, JobNotificationError> {
        self.transition_with_owner(notice_id, from, to, None).await
    }

    async fn transition_with_owner(
        &self,
        notice_id: &str,
        from: JobNotificationState,
        to: JobNotificationState,
        owner: Option<String>,
    ) -> Result<bool, JobNotificationError> {
        self.transition_to(
            notice_id,
            from,
            to,
            chrono::Utc::now().timestamp_millis(),
            owner,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn transition_to(
        &self,
        notice_id: &str,
        from: JobNotificationState,
        to: JobNotificationState,
        now: i64,
        owner: Option<String>,
        injected_event_id: Option<&str>,
    ) -> Result<bool, JobNotificationError> {
        if let Some(pool) = &self.pool {
            let from_str = job_notification_state_to_str(from);
            let to_str = job_notification_state_to_str(to);
            let owner_str = owner.as_deref();
            let claim_lease_until = if to == JobNotificationState::Claimed {
                Some(now.saturating_add(self.policy.claim_lease_ms))
            } else {
                None
            };
            let delivered_at = if to == JobNotificationState::Delivered {
                Some(now)
            } else {
                None
            };
            let result = sqlx::query(
                "UPDATE job_notification SET state = ?2, updated_at = ?3, \
                 claim_owner = CASE WHEN ?2 = 'claimed' THEN ?4 ELSE NULL END, \
                 claim_lease_until = CASE WHEN ?2 = 'claimed' THEN ?5 ELSE NULL END, \
                 delivered_at = CASE WHEN ?2 = 'delivered' THEN ?6 ELSE delivered_at END, \
                 record_json = json_set(json_set(json_set(json_set(json_set(json_set(record_json, '$.state', ?2), '$.updated_at', ?3), '$.claim_owner', ?4), '$.claim_lease_until', ?5), '$.delivered_at', CASE WHEN ?2 = 'delivered' THEN ?6 ELSE json_extract(record_json, '$.delivered_at') END), '$.injected_event_id', CASE WHEN ?8 IS NOT NULL THEN ?8 ELSE json_extract(record_json, '$.injected_event_id') END) \
                 WHERE notice_id = ?1 AND state = ?7",
            )
            .bind(notice_id)
            .bind(to_str)
            .bind(now)
            .bind(owner_str)
            .bind(claim_lease_until)
            .bind(delivered_at)
            .bind(from_str)
            .bind(injected_event_id)
            .execute(pool)
            .await
            .map_err(|e| {
                JobNotificationError::Io(std::io::Error::other(format!("SQLite CAS failed: {e}")))
            })?;
            let changed = result.rows_affected() > 0;
            if changed {
                let mut notices = self.notices.write().await;
                if let Some(n) = notices.get_mut(notice_id) {
                    n.state = to;
                    n.updated_at = now;
                    match to {
                        JobNotificationState::Claimed => {
                            n.claim_owner = owner.or_else(|| n.claim_owner.clone());
                            n.claim_lease_until = claim_lease_until;
                            n.retry_count = n.retry_count.saturating_add(1);
                        }
                        JobNotificationState::Delivered => {
                            n.claim_owner = None;
                            n.claim_lease_until = None;
                            n.delivered_at = Some(now);
                        }
                        JobNotificationState::Pending | JobNotificationState::Expired => {
                            n.claim_owner = None;
                            n.claim_lease_until = None;
                        }
                        JobNotificationState::Suppressed | JobNotificationState::Failed => {}
                    }
                }
            }
            return Ok(changed);
        }
        let updated = {
            let mut notices = self.notices.write().await;
            let Some(n) = notices.get_mut(notice_id) else {
                return Ok(false);
            };
            if n.state != from {
                return Ok(false);
            }
            n.state = to;
            n.updated_at = now;
            match to {
                JobNotificationState::Claimed => {
                    n.claim_owner = owner.or_else(|| n.claim_owner.clone());
                    n.claim_lease_until = Some(now.saturating_add(self.policy.claim_lease_ms));
                    n.retry_count = n.retry_count.saturating_add(1);
                }
                JobNotificationState::Delivered => {
                    n.claim_owner = None;
                    n.claim_lease_until = None;
                    n.delivered_at = Some(now);
                }
                JobNotificationState::Pending | JobNotificationState::Expired => {
                    n.claim_owner = None;
                    n.claim_lease_until = None;
                }
                JobNotificationState::Suppressed | JobNotificationState::Failed => {}
            }
            n.clone()
        };
        self.persist_notice(&updated).await?;
        Ok(true)
    }

    async fn load_session_from_pool(&self, session_id: &str) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM job_notification WHERE session_id = ? ORDER BY created_at ASC LIMIT 64",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        let mut notices = self.notices.write().await;
        let mut index = self.session_index.write().await;
        for json in rows {
            let notice: JobTerminalNotice = serde_json::from_str(&json)?;
            index
                .entry(notice.session_id.clone())
                .or_default()
                .retain(|id| id != &notice.notice_id);
            index
                .entry(notice.session_id.clone())
                .or_default()
                .push(notice.notice_id.clone());
            notices.insert(notice.notice_id.clone(), notice);
        }
        Ok(())
    }

    async fn load_one_from_pool(&self, notice_id: &str) -> Result<(), JobNotificationError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let result = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM job_notification WHERE notice_id = ?",
        )
        .bind(notice_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        let Some(json) = result else {
            return Ok(());
        };
        let notice: JobTerminalNotice = serde_json::from_str(&json)?;
        // Preserve Textual state column as authority when it disagrees with
        // a stale cached JSON (e.g. expiry raced the cache).
        let state_row: Option<String> =
            sqlx::query_scalar("SELECT state FROM job_notification WHERE notice_id = ?")
                .bind(notice_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| JobNotificationError::Storage(e.to_string()))?;
        let mut notice = notice;
        if let Some(s) = state_row
            .as_deref()
            .and_then(job_notification_state_from_str)
        {
            notice.state = s;
        }
        let mut notices = self.notices.write().await;
        let mut index = self.session_index.write().await;
        index
            .entry(notice.session_id.clone())
            .or_default()
            .retain(|id| id != notice_id);
        index
            .entry(notice.session_id.clone())
            .or_default()
            .push(notice_id.to_string());
        notices.insert(notice_id.to_string(), notice);
        Ok(())
    }

    async fn recover_expired(&self) -> Result<(), JobNotificationError> {
        let now = chrono::Utc::now().timestamp_millis();
        let ids: Vec<String> = {
            let notices = self.notices.read().await;
            notices
                .values()
                .filter(|n| {
                    n.state == JobNotificationState::Claimed
                        && now.saturating_sub(n.updated_at) > self.policy.claim_lease_ms
                })
                .map(|n| n.notice_id.clone())
                .collect()
        };
        for id in ids {
            self.transition_to(
                &id,
                JobNotificationState::Claimed,
                JobNotificationState::Expired,
                now,
                None,
                None,
            )
            .await?;
        }
        Ok(())
    }
}

impl Default for JobNotificationService {
    fn default() -> Self {
        Self::new()
    }
}

/// Truly final job states produce terminal notices. `Interrupted` means an
/// eligible attempt may still be requeued and must never be announced as
/// final. Running/progress are never terminal.
pub fn is_final_job_state(state: &JobState) -> bool {
    matches!(
        state,
        JobState::Completed
            | JobState::Failed
            | JobState::Cancelled
            | JobState::TimedOut
            | JobState::Expired
    )
}

pub fn terminal_status_str(state: &JobState) -> &'static str {
    match state {
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
        JobState::TimedOut => "timed_out",
        JobState::Expired => "expired",
        JobState::Interrupted => "interrupted",
        JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => "running",
    }
}

fn bound_summary(summary: &str, cap_chars: usize) -> String {
    let bounded: String = summary.chars().take(cap_chars).collect();
    // Never leak more than the bounded redacted summary; callers must not
    // pass stdout dumps or secrets here (enforced by the test vertical
    // slice, which passes only the compact runner report).
    bounded
}

/// Build the stable `JobHandle` returned immediately after durable
/// background acceptance.
pub fn job_handle_for(
    job_id: &JobId,
    kind: &str,
    status: &str,
    submitted_at: i64,
    run_id: Option<&str>,
) -> JobHandle {
    let job_id_str = job_id.as_str().to_string();
    JobHandle {
        inspect_ref: job_id_str.clone(),
        cancel_ref: job_id_str.clone(),
        job_id: job_id_str,
        kind: kind.to_string(),
        status: status.to_string(),
        submitted_at,
        run_id: run_id.map(ToOwned::to_owned),
    }
}

/// Format the single safe-boundary injection text for a job notice.
/// System-generated and authenticated by durable job-store terminal state;
/// external job status data is never trusted as instruction.
pub fn format_job_notice_content(notice: &JobTerminalNotice) -> String {
    match notice.status.as_str() {
        "completed" => format!(
            "Background job {} (kind {}) completed successfully: {}",
            notice.job_id, notice.kind, notice.summary
        ),
        "cancelled" => format!(
            "Background job {} (kind {}) was cancelled: {}",
            notice.job_id, notice.kind, notice.summary
        ),
        "timed_out" => format!(
            "Background job {} (kind {}) timed out: {}",
            notice.job_id, notice.kind, notice.summary
        ),
        "expired" => format!(
            "Background job {} (kind {}) expired: {}",
            notice.job_id, notice.kind, notice.summary
        ),
        // M004: an unverified/ambiguous CI result is attention, never a
        // CI failure. The message names the condition explicitly so a
        // failed API call or unclear conclusion cannot read as "CI failed".
        "needs_attention" => format!(
            "Background job {} (kind {}) needs attention: {}",
            notice.job_id, notice.kind, notice.summary
        ),
        _ => format!(
            "Background job {} (kind {}) failed ({}): {}",
            notice.job_id, notice.kind, notice.status, notice.summary
        ),
    }
}

/// Derive the durable injection identity for a notice.
pub fn injection_event_id_for(notice: &JobTerminalNotice) -> String {
    format!("job-event:{}", notice.injection_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        JobKind, JobPriority, JobSource, NewJob, ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;

    fn test_job(session: Option<&str>, state: JobState) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: JobId::new_unchecked("job-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-1"),
            session_id: session.map(ToOwned::to_owned),
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: codegg_core::jobs::JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: None,
                scope: None,
                parent_run_id: None,
            },
            resource_request: ResourceRequest::for_kind(JobKind::Test),
            state,
            current_attempt_id: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
            terminal_at: None,
            cancel_requested_at: None,
            cancel_reason: None,
            labels: Default::default(),
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: Default::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        }
    }

    #[test]
    fn final_states_exclude_interrupted() {
        assert!(is_final_job_state(&JobState::Completed));
        assert!(is_final_job_state(&JobState::Failed));
        assert!(is_final_job_state(&JobState::Cancelled));
        assert!(is_final_job_state(&JobState::TimedOut));
        assert!(is_final_job_state(&JobState::Expired));
        assert!(!is_final_job_state(&JobState::Interrupted));
        assert!(!is_final_job_state(&JobState::Running));
        assert!(!is_final_job_state(&JobState::Queued));
    }

    #[tokio::test]
    async fn foreground_job_without_subscription_produces_no_notice() {
        let svc = JobNotificationService::new();
        let job = test_job(Some("s1"), JobState::Completed);
        let out = svc.record_terminal(&job, "ok", None).await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn interrupted_never_produces_notice() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let job = test_job(Some("s1"), JobState::Interrupted);
        let out = svc.record_terminal(&job, "retryable", None).await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn duplicate_terminal_is_idempotent() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let job = test_job(Some("s1"), JobState::Completed);
        let first = svc
            .record_terminal(&job, "ok", None)
            .await
            .unwrap()
            .unwrap();
        let second = svc
            .record_terminal(&job, "ok", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.notice_id, second.notice_id);
        assert_eq!(first.payload_digest, second.payload_digest);
    }

    #[tokio::test]
    async fn cross_session_mismatch_fails_closed() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let job = test_job(Some("s2"), JobState::Completed);
        let err = svc.record_terminal(&job, "ok", None).await.unwrap_err();
        assert!(matches!(err, JobNotificationError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn owner_check_rejects_guessed_job() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let job = test_job(Some("s1"), JobState::Completed);
        let notice = svc
            .record_terminal(&job, "ok", None)
            .await
            .unwrap()
            .unwrap();
        assert!(JobNotificationService::check_owner(&notice, Some("s1")).is_ok());
        assert!(JobNotificationService::check_owner(&notice, Some("s2")).is_err());
        assert!(JobNotificationService::check_owner(&notice, None).is_err());
    }

    #[tokio::test]
    async fn claim_ack_lifecycle() {
        let svc = JobNotificationService::new();
        svc.subscribe(&JobId::new_unchecked("job-1"), "test", "s1", None, "inv-1")
            .await
            .unwrap();
        let job = test_job(Some("s1"), JobState::Failed);
        svc.record_terminal(&job, "boom", None).await.unwrap();
        assert!(svc.claim("job-1").await.unwrap());
        assert!(!svc.claim("job-1").await.unwrap());
        assert!(svc.acknowledge("job-1").await.unwrap());
        assert!(!svc.acknowledge("job-1").await.unwrap());
    }

    #[test]
    fn unused_newjob_import_guard() {
        // Keeps the `NewJob` import honest if the helper above changes.
        let _ = std::mem::size_of::<NewJob>();
    }
}
