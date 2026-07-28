//! Durable parent notification for background Tool Programs.
//!
//! When a tool program is submitted in background mode, the parent
//! agent continues immediately. When the program reaches a terminal
//! state, exactly one logical notification is created and delivered
//! to the parent session's notification inbox.
//!
//! # Invariants
//!
//! - Every background program produces at most one actionable terminal
//!   notification for its parent session.
//! - Notification identity is derived from the program_id and is
//!   idempotent: duplicate terminal events produce the same notification.
//! - Progress events never enqueue model follow-ups.
//! - Notification delivery is durable and survives daemon restart via
//!   the job store's terminal state.

use std::collections::HashMap;
use std::path::Path;

use crate::tool::tool_program_result::ProgramResultRecord;
use codegg_core::jobs::JobPayload;
use codegg_protocol::projection::dto::NotificationClassification;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

/// Configuration for notification queue bounds and backpressure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPolicy {
    /// Maximum number of pending notifications per session before
    /// the oldest is suppressed.
    pub max_pending_per_session: usize,
    /// Maximum age in milliseconds for a claimed notification before
    /// it is expired (lease timeout).
    pub claim_lease_ms: i64,
    /// Maximum total bytes for a single notification payload. Larger
    /// payloads are truncated.
    pub max_payload_bytes: usize,
}

impl Default for NotificationPolicy {
    fn default() -> Self {
        Self {
            max_pending_per_session: 16,
            claim_lease_ms: 300_000, // 5 minutes
            max_payload_bytes: 8_192,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationState {
    /// Terminal event received, notification pending delivery to parent.
    Pending,
    /// Parent agent has claimed the notification (lease acquired).
    Claimed,
    /// Parent agent has acknowledged consumption.
    Delivered,
    /// Notification suppressed (e.g. parent session archived).
    Suppressed,
    /// Notification expired without delivery (lease timeout).
    Expired,
    /// Terminal failure to deliver (inspectable, does not mutate
    /// program status).
    Failed,
}

/// M013-C1: canonical textual representation of [`NotificationState`] for
/// SQLite columns and CAS predicates. Avoids the JSON-quoted `"pending"`
/// form produced by `serde_json::to_string` for unit-variant enums.
pub fn notification_state_to_str(state: NotificationState) -> &'static str {
    match state {
        NotificationState::Pending => "pending",
        NotificationState::Claimed => "claimed",
        NotificationState::Delivered => "delivered",
        NotificationState::Suppressed => "suppressed",
        NotificationState::Expired => "expired",
        NotificationState::Failed => "failed",
    }
}

/// Error type for notification store operations.
///
/// M012-D: Database transition errors are returned to the caller and
/// never reported as success.
#[derive(Debug, thiserror::Error)]
pub enum NotificationStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("notification not found")]
    NotFound,
    #[error("conflict: notification already in target state")]
    Conflict,
    #[error("store unavailable")]
    Unavailable,
    #[error("storage error: {0}")]
    Storage(String),
}

/// A durable notification record for a background tool program
/// completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgramNotification {
    /// Logical notification identity (program_id).
    pub notification_id: String,
    /// The program that completed.
    pub program_id: String,
    /// The scheduler job ID.
    pub job_id: String,
    /// Parent session that submitted the program.
    pub session_id: String,
    /// Parent agent run that submitted the program.
    pub agent_id: Option<String>,
    /// Parent turn that submitted the program.
    pub turn_id: Option<String>,
    /// Compact terminal status.
    pub status: String,
    /// Summary of the result (bounded).
    pub summary: String,
    /// Failure class if failed.
    pub failure_class: Option<String>,
    /// Whether the program completed successfully.
    pub success: bool,
    /// Three-way classification: completed, incomplete-recoverable,
    /// or failed-terminal.
    pub classification: codegg_protocol::projection::dto::NotificationClassification,
    /// SHA-256 digest of the notification payload for idempotency
    /// verification.
    pub payload_digest: String,
    /// Program handle for inspection.
    pub program_handle: ProgramHandle,
    /// Current notification state.
    pub state: NotificationState,
    /// Creation timestamp (millis since epoch).
    pub created_at: i64,
    /// Last state transition timestamp.
    pub updated_at: i64,
    /// Delivery claimant and bounded lease, persisted across restart.
    #[serde(default)]
    pub claim_owner: Option<String>,
    #[serde(default)]
    pub claim_lease_until: Option<i64>,
    /// Durable acknowledgement timestamp.
    #[serde(default)]
    pub delivered_at: Option<i64>,
    #[serde(default)]
    pub retry_count: u32,
    /// M012-D: Injection key for durable event injection.
    pub injection_key: Option<String>,
    /// M012-D: Injected event ID after durable injection.
    pub injected_event_id: Option<String>,
}

/// Compact handle returned to the parent when a background program is
/// submitted. Contains everything the parent needs to inspect or cancel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramHandle {
    /// Logical program identity.
    pub program_id: String,
    /// Scheduler job ID.
    pub job_id: String,
    /// Display status at submission time.
    pub status: String,
    /// Submission timestamp (millis since epoch).
    pub submitted_at: i64,
    /// Effective timeout in milliseconds.
    pub timeout_ms: u64,
    /// Inspection reference (program_id).
    pub inspect_ref: String,
    /// Cancel reference (job_id).
    pub cancel_ref: String,
}

/// Compact representation of a terminal tool program job, used for
/// notification recovery after a daemon restart.
#[derive(Debug, Clone)]
pub struct RecoveredTerminalJob {
    pub program_id: String,
    pub job_id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub summary: String,
    pub failure_class: Option<String>,
    pub success: bool,
    pub created_at: i64,
}

/// Notification store with claim/ack semantics.
///
/// The daemon-scoped cache is backed by SQLite in production through
/// [`Self::with_pool`]. On restart, terminal job state and the durable
/// notification table are reconciled so acknowledged notifications are not
/// recreated.
pub struct ToolProgramNotificationService {
    /// notification_id → notification record.
    pub notifications: RwLock<HashMap<String, ToolProgramNotification>>,
    /// session_id → set of pending notification_ids.
    pub session_index: RwLock<HashMap<String, Vec<String>>>,
    /// Policy configuration for bounds and backpressure.
    pub policy: NotificationPolicy,
    pool: Option<SqlitePool>,
}

impl ToolProgramNotificationService {
    pub fn new() -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            policy: NotificationPolicy::default(),
            pool: None,
        }
    }

    /// Create a notification service with a custom policy.
    pub fn with_policy(policy: NotificationPolicy) -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            policy,
            pool: None,
        }
    }

    /// Production constructor. Notification state is cached for low-latency
    /// reads but every transition is also written to the daemon catalog.
    pub fn with_pool(pool: SqlitePool) -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            policy: NotificationPolicy::default(),
            pool: Some(pool),
        }
    }

    /// Record a terminal notification for a background program.
    ///
    /// Idempotent: if a notification with the same `notification_id`
    /// already exists, this returns the existing record without
    /// mutation.
    pub async fn record_notification(
        &self,
        notification: ToolProgramNotification,
    ) -> ToolProgramNotification {
        if notification.session_id.is_empty() {
            tracing::warn!(
                notification_id = %notification.notification_id,
                "refusing to persist Tool Program notification without parent session"
            );
            return notification;
        }
        if let Some(pool) = &self.pool {
            if let Ok(Some(json)) = sqlx::query_scalar::<_, String>(
                "SELECT record_json FROM tool_program_notification WHERE notification_id = ?",
            )
            .bind(&notification.notification_id)
            .fetch_optional(pool)
            .await
            {
                if let Ok(existing) = serde_json::from_str::<ToolProgramNotification>(&json) {
                    return existing;
                }
            }
        }
        let mut notifications = self.notifications.write().await;
        if let Some(existing) = notifications.get(&notification.notification_id) {
            return existing.clone();
        }
        let session_id = notification.session_id.clone();
        let nid = notification.notification_id.clone();
        notifications.insert(nid.clone(), notification.clone());
        drop(notifications);

        let mut index = self.session_index.write().await;
        index.entry(session_id).or_default().push(nid);
        drop(index);
        if let Err(error) = self.persist_record(&notification).await {
            tracing::warn!(%error, notification_id = %notification.notification_id, "failed to persist Tool Program notification");
        }
        // M013-C-11: If persist_record fails, the notification is still
        // in the in-memory cache and will be recovered by recover_from_pool
        // on restart. Log the failure for diagnostics.
        notification
    }

    /// Create the one terminal notification for a completed background
    /// invocation. Submission itself never creates an actionable record.
    pub async fn record_terminal_result(
        &self,
        program_id: &str,
        job_id: &str,
        session_id: Option<&str>,
        agent_id: Option<&str>,
        turn_id: Option<&str>,
        record: &ProgramResultRecord,
    ) {
        let Some(session_id) = session_id.filter(|value| !value.is_empty()) else {
            return;
        };
        let status = serde_json::to_value(record.result.status)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "failed".into());
        let failure_class = record.result.failure_class.as_ref().and_then(|class| {
            serde_json::to_value(class)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
        });
        let success = matches!(
            record.result.status,
            codegg_core::tool_program::ProgramStatus::Completed
        );
        let summary = record
            .result
            .error_message
            .clone()
            .unwrap_or_else(|| format!("Tool Program {} {}", program_id, status));
        let bounded_summary = summary.chars().take(1024).collect::<String>();
        let classification = classify_terminal(&status, failure_class.as_deref(), success);
        let payload = format!("{program_id}|{status}|{}|{success}", record.result_digest);
        let notification = ToolProgramNotification {
            notification_id: program_id.to_string(),
            program_id: program_id.to_string(),
            job_id: job_id.to_string(),
            session_id: session_id.to_string(),
            agent_id: agent_id.map(ToOwned::to_owned),
            turn_id: turn_id.map(ToOwned::to_owned),
            status,
            summary: bounded_summary,
            failure_class,
            success,
            classification,
            payload_digest: format!("{:x}", Sha256::digest(payload.as_bytes())),
            program_handle: ProgramHandle {
                program_id: program_id.to_string(),
                job_id: job_id.to_string(),
                status: "terminal".into(),
                submitted_at: record.recorded_at,
                timeout_ms: 120_000,
                inspect_ref: program_id.to_string(),
                cancel_ref: job_id.to_string(),
            },
            state: NotificationState::Pending,
            created_at: record.recorded_at,
            updated_at: record.recorded_at,
            claim_owner: None,
            claim_lease_until: None,
            delivered_at: None,
            retry_count: 0,
            injection_key: Some(format!("tp-inject:{}:{}", program_id, session_id)),
            injected_event_id: None,
        };
        let session_id = notification.session_id.clone();
        self.record_notification(notification).await;
        let _ = self
            .enforce_session_bound(&session_id, self.policy.max_pending_per_session)
            .await;
    }

    /// Reconcile terminal background programs from the durable job catalog.
    /// This is safe to call at every turn boundary: notification identity is
    /// the program identity and delivered records are never recreated.
    pub async fn recover_from_pool(&self) -> usize {
        let Some(pool) = &self.pool else { return 0 };
        let rows = sqlx::query(
            "SELECT j.id, j.session_id, j.turn_id, j.state, j.payload_json, j.time_created, w.canonical_root FROM job j LEFT JOIN workspace w ON w.id = j.workspace_id WHERE j.kind = 'tool_program' AND j.state IN ('completed', 'failed', 'cancelled', 'timed_out', 'interrupted') AND j.session_id IS NOT NULL ORDER BY j.time_created ASC LIMIT 256",
        )
        .fetch_all(pool)
        .await;
        let Ok(rows) = rows else { return 0 };
        let mut recovered = 0;
        for row in rows {
            let job_id: String = row.get("id");
            let session_id: Option<String> = row.get("session_id");
            let turn_id: Option<String> = row.get("turn_id");
            let state: String = row.get("state");
            let payload_json: String = row.get("payload_json");
            let created_at: i64 = row.get("time_created");
            let workspace_root: Option<String> = row.get("canonical_root");
            let Ok(JobPayload::ToolProgram {
                program_id,
                execution_mode,
                execution_context_json,
                ..
            }) = serde_json::from_str(&payload_json)
            else {
                continue;
            };
            if execution_mode != "background" {
                continue;
            }
            let context = execution_context_json.as_deref().and_then(|json| {
                serde_json::from_str::<codegg_core::jobs::ToolProgramExecutionContext>(json).ok()
            });
            let Some(root) = workspace_root else { continue };
            let loaded =
                crate::tool::tool_program_result::ToolProgramResultStore::new(Path::new(&root))
                    .load(&program_id)
                    .ok()
                    .flatten();
            if let Some(record) = loaded {
                self.record_terminal_result(
                    &program_id,
                    &job_id,
                    session_id.as_deref(),
                    context.as_ref().and_then(|value| value.agent_id.as_deref()),
                    turn_id
                        .as_deref()
                        .or_else(|| context.as_ref().and_then(|value| value.turn_id.as_deref())),
                    &record,
                )
                .await;
                recovered += 1;
            } else {
                recovered += self
                    .recover_from_terminal_jobs(vec![RecoveredTerminalJob {
                        program_id,
                        job_id,
                        session_id,
                        status: state.clone(),
                        summary: format!("Tool Program terminal state: {state}"),
                        failure_class: None,
                        success: state == "completed",
                        created_at,
                    }])
                    .await;
            }
        }
        recovered
    }

    /// Claim a notification for processing. Uses compare-and-set:
    /// only transitions from Pending to Claimed.
    ///
    /// Returns `true` if the claim succeeded, `false` if the
    /// notification was already claimed or in a non-pending state.
    pub async fn claim(&self, notification_id: &str) -> Result<bool, NotificationStoreError> {
        self.claim_as(notification_id, "tool-program-delivery")
            .await
    }

    /// Claim a notification with an explicit delivery owner.
    pub async fn claim_as(
        &self,
        notification_id: &str,
        owner: &str,
    ) -> Result<bool, NotificationStoreError> {
        self.load_one_from_pool(notification_id).await;
        self.recover_expired().await;
        self.transition_with_owner(
            notification_id,
            NotificationState::Pending,
            NotificationState::Claimed,
            Some(owner.to_string()),
        )
        .await
    }

    /// Acknowledge consumption of a claimed notification.
    ///
    /// Transitions from Claimed to Delivered. Returns `false` if the
    /// notification is not in the Claimed state.
    pub async fn acknowledge(&self, notification_id: &str) -> Result<bool, NotificationStoreError> {
        self.load_one_from_pool(notification_id).await;
        self.transition_with_owner(
            notification_id,
            NotificationState::Claimed,
            NotificationState::Delivered,
            None,
        )
        .await
    }

    /// M012-F03: Mark a notification as injected with the durable event
    /// identity. This is called after the notification message has been
    /// appended to the session so that recovery can detect the injection
    /// and acknowledge without re-injecting.
    pub async fn mark_injected(
        &self,
        notification_id: &str,
        event_id: &str,
    ) -> Result<(), NotificationStoreError> {
        // Update the in-memory record
        let record_json = {
            let mut notifications = self.notifications.write().await;
            if let Some(n) = notifications.get_mut(notification_id) {
                n.injected_event_id = Some(event_id.to_string());
                serde_json::to_string(n).ok()
            } else {
                None
            }
        };
        // Persist the update to SQLite with CAS: only update if the
        // notification has not been delivered, suppressed, or already
        // injected (prevents races where another instance may have
        // acknowledged the claim or already injected).
        // M013-C1: state column stores raw lowercase tokens (e.g.
        // 'claimed'), so terminal states are matched with that form.
        if let (Some(pool), Some(json)) = (&self.pool, record_json) {
            sqlx::query(
                "UPDATE tool_program_notification
                 SET record_json = ?
                 WHERE notification_id = ?
                   AND state NOT IN ('delivered', 'suppressed', 'expired')
                   AND json_extract(record_json, '$.injected_event_id') IS NULL",
            )
            .bind(json)
            .bind(notification_id)
            .execute(pool)
            .await
            .map_err(|e| {
                NotificationStoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        }
        Ok(())
    }

    /// M012-F03: Check if a notification has already been injected into
    /// the session. If `injected_event_id` is set, the notification was
    /// injected and recovery should acknowledge without re-injecting.
    pub async fn is_injected(&self, notification_id: &str) -> bool {
        let notifications = self.notifications.read().await;
        notifications
            .get(notification_id)
            .and_then(|n| n.injected_event_id.as_ref())
            .is_some()
    }

    /// Suppress a notification (e.g. parent session archived).
    pub async fn suppress(&self, notification_id: &str) -> Result<bool, NotificationStoreError> {
        self.load_one_from_pool(notification_id).await;
        if self
            .transition(
                notification_id,
                NotificationState::Pending,
                NotificationState::Suppressed,
            )
            .await?
        {
            return Ok(true);
        }
        self.transition(
            notification_id,
            NotificationState::Claimed,
            NotificationState::Suppressed,
        )
        .await
    }

    /// Get all pending notifications for a session, in deterministic
    /// (creation-time) order.
    pub async fn pending_for_session(&self, session_id: &str) -> Vec<ToolProgramNotification> {
        if self.pool.is_some() {
            self.load_session_from_pool(session_id).await;
        }
        let index = self.session_index.read().await;
        let notifications = self.notifications.read().await;
        if let Some(nids) = index.get(session_id) {
            nids.iter()
                .filter_map(|nid| notifications.get(nid))
                .filter(|n| n.state == NotificationState::Pending)
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }

    /// Get a notification by ID.
    pub async fn get(&self, notification_id: &str) -> Option<ToolProgramNotification> {
        let notifications = self.notifications.read().await;
        if let Some(notification) = notifications.get(notification_id).cloned() {
            return Some(notification);
        }
        drop(notifications);
        self.load_one_from_pool(notification_id).await;
        let notifications = self.notifications.read().await;
        notifications.get(notification_id).cloned()
    }

    /// Count pending notifications for a session.
    pub async fn pending_count(&self, session_id: &str) -> usize {
        self.pending_for_session(session_id).await.len()
    }

    /// Expire old claimed notifications (lease timeout).
    /// Returns IDs of expired notifications.
    pub async fn expire_stale(&self, max_age_ms: i64) -> Vec<String> {
        let now = chrono::Utc::now().timestamp_millis();
        let expired: Vec<String> = {
            let notifications = self.notifications.read().await;
            notifications
                .iter()
                .filter(|(_, n)| {
                    n.state == NotificationState::Claimed && (now - n.updated_at) > max_age_ms
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &expired {
            let _ = self
                .transition_to(
                    id,
                    NotificationState::Claimed,
                    NotificationState::Expired,
                    now,
                    None,
                )
                .await;
        }
        expired
    }

    /// Test helper: set the `updated_at` timestamp of a notification.
    /// Used to test expiry without waiting.
    #[cfg(test)]
    pub async fn set_updated_at(&self, notification_id: &str, timestamp: i64) {
        let mut notifications = self.notifications.write().await;
        if let Some(n) = notifications.get_mut(notification_id) {
            n.updated_at = timestamp;
        }
    }

    /// Bound the total number of notifications per session.
    /// Returns IDs of notifications that were suppressed to enforce
    /// the bound.
    pub async fn enforce_session_bound(&self, session_id: &str, max_pending: usize) -> Vec<String> {
        let pending: Vec<String> = {
            let index = self.session_index.read().await;
            let Some(nids) = index.get(session_id) else {
                return vec![];
            };
            let notifications = self.notifications.read().await;
            nids.iter()
                .filter(|nid| {
                    notifications
                        .get(nid.as_str())
                        .map(|n| n.state == NotificationState::Pending)
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        };
        let mut suppressed = vec![];
        if pending.len() > max_pending {
            // Suppress oldest (first in list = earliest created).
            for nid in &pending[..pending.len() - max_pending] {
                if self.suppress(nid).await.unwrap_or(false) {
                    suppressed.push(nid.clone());
                }
            }
        }
        suppressed
    }

    /// Recover pending notifications from terminal job records after a
    /// daemon restart. For each terminal tool program job that has not
    /// been acknowledged, creates a pending notification record so the
    /// AgentLoop can inject it at the next turn boundary.
    ///
    /// This is idempotent: duplicate program_ids are ignored.
    pub async fn recover_from_terminal_jobs(
        &self,
        terminal_jobs: Vec<RecoveredTerminalJob>,
    ) -> usize {
        let mut recovered = 0;
        for job in terminal_jobs {
            let classification =
                classify_terminal(&job.status, job.failure_class.as_deref(), job.success);
            let payload = format!(
                "{}|{}|{}|{}",
                job.program_id, job.status, job.summary, job.success
            );
            let payload_digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
            let notification = ToolProgramNotification {
                notification_id: job.program_id.clone(),
                program_id: job.program_id.clone(),
                job_id: job.job_id.clone(),
                session_id: job.session_id.clone().unwrap_or_default(),
                agent_id: None,
                turn_id: None,
                status: job.status.clone(),
                summary: job.summary.clone(),
                failure_class: job.failure_class.clone(),
                success: job.success,
                classification,
                payload_digest,
                program_handle: ProgramHandle {
                    program_id: job.program_id.clone(),
                    job_id: job.job_id.clone(),
                    status: job.status.clone(),
                    submitted_at: job.created_at,
                    timeout_ms: 120_000,
                    inspect_ref: job.program_id.clone(),
                    cancel_ref: job.job_id.clone(),
                },
                state: NotificationState::Pending,
                created_at: job.created_at,
                updated_at: job.created_at,
                claim_owner: None,
                claim_lease_until: None,
                delivered_at: None,
                retry_count: 0,
                injection_key: Some(format!(
                    "tp-inject:{}:{}",
                    job.program_id,
                    job.session_id.as_deref().unwrap_or("unknown")
                )),
                injected_event_id: None,
            };
            let existing = self.get(&job.program_id).await;
            if existing.is_none() {
                self.record_notification(notification).await;
                recovered += 1;
            }
        }
        recovered
    }

    async fn persist_record(
        &self,
        notification: &ToolProgramNotification,
    ) -> Result<(), NotificationStoreError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json = serde_json::to_string(notification)
            .map_err(|e| NotificationStoreError::Storage(e.to_string()))?;
        sqlx::query(
            "INSERT INTO tool_program_notification (notification_id, program_id, job_id, session_id, agent_id, turn_id, state, record_json, claim_owner, claim_lease_until, created_at, updated_at, delivered_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(notification_id) DO UPDATE SET state = excluded.state, record_json = excluded.record_json, claim_owner = excluded.claim_owner, claim_lease_until = excluded.claim_lease_until, updated_at = excluded.updated_at, delivered_at = excluded.delivered_at",
        )
        .bind(&notification.notification_id)
        .bind(&notification.program_id)
        .bind(&notification.job_id)
        .bind(&notification.session_id)
        .bind(&notification.agent_id)
        .bind(&notification.turn_id)
        .bind(notification_state_to_str(notification.state))
        .bind(record_json)
        .bind(&notification.claim_owner)
        .bind(notification.claim_lease_until)
        .bind(notification.created_at)
        .bind(notification.updated_at)
        .bind((notification.state == NotificationState::Delivered).then_some(notification.updated_at))
        .execute(pool)
        .await
        .map_err(|e| NotificationStoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn transition(
        &self,
        notification_id: &str,
        from: NotificationState,
        to: NotificationState,
    ) -> Result<bool, NotificationStoreError> {
        self.transition_with_owner(notification_id, from, to, None)
            .await
    }

    async fn transition_with_owner(
        &self,
        notification_id: &str,
        from: NotificationState,
        to: NotificationState,
        owner: Option<String>,
    ) -> Result<bool, NotificationStoreError> {
        self.transition_to(
            notification_id,
            from,
            to,
            chrono::Utc::now().timestamp_millis(),
            owner,
        )
        .await
    }

    async fn transition_to(
        &self,
        notification_id: &str,
        from: NotificationState,
        to: NotificationState,
        now: i64,
        owner: Option<String>,
    ) -> Result<bool, NotificationStoreError> {
        // M012-D: When a SQLite pool is available, use CAS as the authority.
        if let Some(pool) = &self.pool {
            let from_str = notification_state_to_str(from);
            let to_str = notification_state_to_str(to);
            let owner_str = owner.as_deref();
            let claim_lease_until = if to == NotificationState::Claimed {
                Some(now.saturating_add(self.policy.claim_lease_ms))
            } else {
                None
            };
            let delivered_at = if to == NotificationState::Delivered {
                Some(now)
            } else {
                None
            };
            let result = sqlx::query(
                "UPDATE tool_program_notification
                   SET state = ?2, updated_at = ?3,
                       claim_owner = CASE WHEN ?2 = 'claimed' THEN ?4 ELSE NULL END,
                       claim_lease_until = CASE WHEN ?2 = 'claimed' THEN ?5 ELSE NULL END,
                       delivered_at = CASE WHEN ?2 = 'delivered' THEN ?6 ELSE delivered_at END,
                       record_json = json_set(
                           json_set(
                               json_set(
                                   json_set(record_json, '$.state', ?2),
                                   '$.updated_at', ?3),
                               '$.claim_owner', ?4),
                           '$.claim_lease_until', ?5)
                 WHERE notification_id = ?1 AND state = ?7",
            )
            .bind(notification_id)
            .bind(to_str)
            .bind(now)
            .bind(owner_str)
            .bind(claim_lease_until)
            .bind(delivered_at)
            .bind(from_str)
            .execute(pool)
            .await
            .map_err(|e| {
                NotificationStoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("SQLite CAS failed: {}", e),
                ))
            })?;
            let changed = result.rows_affected() > 0;
            if changed {
                // Update in-memory cache to match durable state
                let mut notifications = self.notifications.write().await;
                if let Some(notification) = notifications.get_mut(notification_id) {
                    notification.state = to;
                    notification.updated_at = now;
                    match to {
                        NotificationState::Claimed => {
                            notification.claim_owner =
                                owner.or_else(|| notification.claim_owner.clone());
                            notification.claim_lease_until = claim_lease_until;
                            notification.retry_count = notification.retry_count.saturating_add(1);
                        }
                        NotificationState::Delivered => {
                            notification.claim_owner = None;
                            notification.claim_lease_until = None;
                            notification.delivered_at = Some(now);
                        }
                        NotificationState::Pending | NotificationState::Expired => {
                            notification.claim_owner = None;
                            notification.claim_lease_until = None;
                        }
                        NotificationState::Suppressed | NotificationState::Failed => {}
                    }
                }
            }
            return Ok(changed);
        }

        // Fallback: in-memory CAS when no pool is available.
        let updated = {
            let mut notifications = self.notifications.write().await;
            let Some(notification) = notifications.get_mut(notification_id) else {
                return Ok(false);
            };
            if notification.state != from {
                return Ok(false);
            }
            notification.state = to;
            notification.updated_at = now;
            match to {
                NotificationState::Claimed => {
                    notification.claim_owner = owner.or_else(|| notification.claim_owner.clone());
                    notification.claim_lease_until =
                        Some(now.saturating_add(self.policy.claim_lease_ms));
                    notification.retry_count = notification.retry_count.saturating_add(1);
                }
                NotificationState::Delivered => {
                    notification.claim_owner = None;
                    notification.claim_lease_until = None;
                    notification.delivered_at = Some(now);
                }
                NotificationState::Pending | NotificationState::Expired => {
                    notification.claim_owner = None;
                    notification.claim_lease_until = None;
                }
                NotificationState::Suppressed | NotificationState::Failed => {}
            }
            notification.clone()
        };
        if let Err(error) = self.persist_record(&updated).await {
            tracing::warn!(%error, notification_id = %updated.notification_id, "failed to persist Tool Program notification transition");
        }
        Ok(true)
    }

    async fn load_session_from_pool(&self, session_id: &str) {
        let Some(pool) = &self.pool else { return };
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM tool_program_notification WHERE session_id = ? ORDER BY created_at ASC LIMIT 64",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await;
        let Ok(rows) = rows else { return };
        let mut notifications = self.notifications.write().await;
        let mut index = self.session_index.write().await;
        for json in rows {
            if let Ok(notification) = serde_json::from_str::<ToolProgramNotification>(&json) {
                index
                    .entry(notification.session_id.clone())
                    .or_default()
                    .retain(|id| id != &notification.notification_id);
                index
                    .entry(notification.session_id.clone())
                    .or_default()
                    .push(notification.notification_id.clone());
                notifications.insert(notification.notification_id.clone(), notification);
            }
        }
    }

    async fn load_one_from_pool(&self, notification_id: &str) {
        let Some(pool) = &self.pool else { return };
        let result = sqlx::query_scalar::<_, String>(
            "SELECT record_json FROM tool_program_notification WHERE notification_id = ?",
        )
        .bind(notification_id)
        .fetch_optional(pool)
        .await;
        let Ok(Some(json)) = result else {
            return;
        };
        let Ok(notification) = serde_json::from_str::<ToolProgramNotification>(&json) else {
            return;
        };
        let mut notifications = self.notifications.write().await;
        let mut index = self.session_index.write().await;
        index
            .entry(notification.session_id.clone())
            .or_default()
            .retain(|id| id != notification_id);
        index
            .entry(notification.session_id.clone())
            .or_default()
            .push(notification_id.to_string());
        notifications.insert(notification_id.to_string(), notification);
    }

    async fn recover_expired(&self) {
        let now = chrono::Utc::now().timestamp_millis();
        let ids: Vec<String> = {
            let notifications = self.notifications.read().await;
            notifications
                .values()
                .filter(|notification| {
                    notification.state == NotificationState::Claimed
                        && now.saturating_sub(notification.updated_at) > self.policy.claim_lease_ms
                })
                .map(|notification| notification.notification_id.clone())
                .collect()
        };
        for id in ids {
            let _ = self
                .transition_to(
                    &id,
                    NotificationState::Claimed,
                    NotificationState::Pending,
                    now,
                    None,
                )
                .await;
        }
    }
}

impl Default for ToolProgramNotificationService {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify a terminal notification into one of three categories:
/// completed, incomplete-recoverable, or failed-terminal.
fn classify_terminal(
    _status: &str,
    failure_class: Option<&str>,
    success: bool,
) -> NotificationClassification {
    if success {
        return NotificationClassification::Completed;
    }
    match failure_class {
        Some("timeout") | Some("stall") | Some("interrupted") => {
            NotificationClassification::IncompleteRecoverable
        }
        _ => {
            // compile_error, policy_denied, resource_exhausted, etc.
            NotificationClassification::FailedTerminal
        }
    }
}

/// Public wrapper for `classify_terminal` — exposed for integration
/// tests and consumers that need to classify a terminal notification.
pub fn classify_terminal_for_test(
    status: &str,
    failure_class: Option<&str>,
    success: bool,
) -> codegg_protocol::projection::dto::NotificationClassification {
    classify_terminal(status, failure_class, success)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_notification(
        program_id: &str,
        session_id: &str,
        status: &str,
        success: bool,
    ) -> ToolProgramNotification {
        let now = chrono::Utc::now().timestamp_millis();
        let classification = classify_terminal(status, None, success);
        let payload = format!(
            "{}|{}|program {} finished|{}",
            program_id, status, program_id, success
        );
        let payload_digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
        ToolProgramNotification {
            notification_id: program_id.to_string(),
            program_id: program_id.to_string(),
            job_id: format!("j-{}", program_id),
            session_id: session_id.to_string(),
            agent_id: None,
            turn_id: None,
            status: status.to_string(),
            summary: format!("program {} finished", program_id),
            failure_class: None,
            success,
            classification,
            payload_digest,
            program_handle: ProgramHandle {
                program_id: program_id.to_string(),
                job_id: format!("j-{}", program_id),
                status: "submitted".to_string(),
                submitted_at: now,
                timeout_ms: 120_000,
                inspect_ref: program_id.to_string(),
                cancel_ref: format!("j-{}", program_id),
            },
            state: NotificationState::Pending,
            created_at: now,
            updated_at: now,
            claim_owner: None,
            claim_lease_until: None,
            delivered_at: None,
            retry_count: 0,
            injection_key: None,
            injected_event_id: None,
        }
    }

    #[tokio::test]
    async fn record_and_get_notification() {
        let svc = ToolProgramNotificationService::new();
        let n = test_notification("tp-1", "s1", "completed", true);
        svc.record_notification(n.clone()).await;
        let got = svc.get("tp-1").await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().status, "completed");
    }

    #[tokio::test]
    async fn idempotent_record() {
        let svc = ToolProgramNotificationService::new();
        let mut n1 = test_notification("tp-1", "s1", "completed", true);
        svc.record_notification(n1.clone()).await;
        // Record again with different summary — should not overwrite
        n1.summary = "changed".to_string();
        let result = svc.record_notification(n1).await;
        assert_eq!(result.summary, "program tp-1 finished");
    }

    #[tokio::test]
    async fn claim_succeeds_only_from_pending() {
        let svc = ToolProgramNotificationService::new();
        let n = test_notification("tp-1", "s1", "completed", true);
        svc.record_notification(n).await;
        assert!(svc.claim("tp-1").await.unwrap());
        // Second claim fails
        assert!(!svc.claim("tp-1").await.unwrap());
    }

    #[tokio::test]
    async fn acknowledge_succeeds_only_from_claimed() {
        let svc = ToolProgramNotificationService::new();
        let n = test_notification("tp-1", "s1", "completed", true);
        svc.record_notification(n).await;
        assert!(!svc.acknowledge("tp-1").await.unwrap()); // pending, not claimed
        assert!(svc.claim("tp-1").await.unwrap());
        assert!(svc.acknowledge("tp-1").await.unwrap());
        // Double ack fails
        assert!(!svc.acknowledge("tp-1").await.unwrap());
    }

    #[tokio::test]
    async fn pending_for_session() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        svc.record_notification(test_notification("tp-2", "s1", "failed", false))
            .await;
        svc.record_notification(test_notification("tp-3", "s2", "completed", true))
            .await;

        let pending = svc.pending_for_session("s1").await;
        assert_eq!(pending.len(), 2);
        let pending_s2 = svc.pending_for_session("s2").await;
        assert_eq!(pending_s2.len(), 1);
    }

    #[tokio::test]
    async fn claimed_not_in_pending() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        svc.claim("tp-1").await.unwrap();
        let pending = svc.pending_for_session("s1").await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn suppress_removes_from_pending() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        assert!(svc.suppress("tp-1").await.unwrap());
        let pending = svc.pending_for_session("s1").await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn expire_stale_claims() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        svc.claim("tp-1").await.unwrap();
        // Set updated_at to the past so it's stale
        {
            let mut notifications = svc.notifications.write().await;
            if let Some(n) = notifications.get_mut("tp-1") {
                n.updated_at = chrono::Utc::now().timestamp_millis() - 1000;
            }
        }
        // Expire with 100ms max age — should expire since updated_at is 1s old
        let expired = svc.expire_stale(100).await;
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "tp-1");
        let n = svc.get("tp-1").await.unwrap();
        assert_eq!(n.state, NotificationState::Expired);
    }

    #[tokio::test]
    async fn session_bound_enforcement() {
        let svc = ToolProgramNotificationService::new();
        for i in 0..5 {
            svc.record_notification(test_notification(
                &format!("tp-{}", i),
                "s1",
                "completed",
                true,
            ))
            .await;
        }
        let suppressed = svc.enforce_session_bound("s1", 2).await;
        assert_eq!(suppressed.len(), 3);
        let pending = svc.pending_for_session("s1").await;
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn empty_session_pending() {
        let svc = ToolProgramNotificationService::new();
        let pending = svc.pending_for_session("nonexistent").await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn recover_from_terminal_jobs_creates_pending_notifications() {
        let svc = ToolProgramNotificationService::new();
        let jobs = vec![
            RecoveredTerminalJob {
                program_id: "tp-1".into(),
                job_id: "j-1".into(),
                session_id: Some("s1".into()),
                status: "completed".into(),
                summary: "ok".into(),
                failure_class: None,
                success: true,
                created_at: 1000,
            },
            RecoveredTerminalJob {
                program_id: "tp-2".into(),
                job_id: "j-2".into(),
                session_id: Some("s1".into()),
                status: "failed".into(),
                summary: "timeout".into(),
                failure_class: Some("timeout".into()),
                success: false,
                created_at: 2000,
            },
        ];
        let recovered = svc.recover_from_terminal_jobs(jobs).await;
        assert_eq!(recovered, 2);

        let pending = svc.pending_for_session("s1").await;
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn recover_from_terminal_jobs_is_idempotent() {
        let svc = ToolProgramNotificationService::new();
        let jobs = vec![RecoveredTerminalJob {
            program_id: "tp-1".into(),
            job_id: "j-1".into(),
            session_id: Some("s1".into()),
            status: "completed".into(),
            summary: "ok".into(),
            failure_class: None,
            success: true,
            created_at: 1000,
        }];
        let recovered1 = svc.recover_from_terminal_jobs(jobs.clone()).await;
        assert_eq!(recovered1, 1);

        // Recover again — should not create duplicate
        let recovered2 = svc.recover_from_terminal_jobs(jobs).await;
        assert_eq!(recovered2, 0);

        let pending = svc.pending_for_session("s1").await;
        assert_eq!(pending.len(), 1);
    }
}
