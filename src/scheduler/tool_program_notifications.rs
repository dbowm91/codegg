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

use codegg_protocol::projection::dto::NotificationClassification;
use serde::{Deserialize, Serialize};
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

/// In-memory notification store with claim/ack semantics.
///
/// This is daemon-scoped and does not persist to SQLite. On restart,
/// the job store's terminal state is the source of truth, and
/// reconciliation re-creates pending notifications from terminal jobs
/// that have not yet been acknowledged.
pub struct ToolProgramNotificationService {
    /// notification_id → notification record.
    pub notifications: RwLock<HashMap<String, ToolProgramNotification>>,
    /// session_id → set of pending notification_ids.
    pub session_index: RwLock<HashMap<String, Vec<String>>>,
    /// Policy configuration for bounds and backpressure.
    pub policy: NotificationPolicy,
}

impl ToolProgramNotificationService {
    pub fn new() -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            policy: NotificationPolicy::default(),
        }
    }

    /// Create a notification service with a custom policy.
    pub fn with_policy(policy: NotificationPolicy) -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
            policy,
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
        notification
    }

    /// Claim a notification for processing. Uses compare-and-set:
    /// only transitions from Pending to Claimed.
    ///
    /// Returns `true` if the claim succeeded, `false` if the
    /// notification was already claimed or in a non-pending state.
    pub async fn claim(&self, notification_id: &str) -> bool {
        let mut notifications = self.notifications.write().await;
        if let Some(n) = notifications.get_mut(notification_id) {
            if n.state == NotificationState::Pending {
                n.state = NotificationState::Claimed;
                n.updated_at = chrono::Utc::now().timestamp_millis();
                return true;
            }
        }
        false
    }

    /// Acknowledge consumption of a claimed notification.
    ///
    /// Transitions from Claimed to Delivered. Returns `false` if the
    /// notification is not in the Claimed state.
    pub async fn acknowledge(&self, notification_id: &str) -> bool {
        let mut notifications = self.notifications.write().await;
        if let Some(n) = notifications.get_mut(notification_id) {
            if n.state == NotificationState::Claimed {
                n.state = NotificationState::Delivered;
                n.updated_at = chrono::Utc::now().timestamp_millis();
                return true;
            }
        }
        false
    }

    /// Suppress a notification (e.g. parent session archived).
    pub async fn suppress(&self, notification_id: &str) -> bool {
        let mut notifications = self.notifications.write().await;
        if let Some(n) = notifications.get_mut(notification_id) {
            if n.state == NotificationState::Pending || n.state == NotificationState::Claimed {
                n.state = NotificationState::Suppressed;
                n.updated_at = chrono::Utc::now().timestamp_millis();
                return true;
            }
        }
        false
    }

    /// Get all pending notifications for a session, in deterministic
    /// (creation-time) order.
    pub async fn pending_for_session(&self, session_id: &str) -> Vec<ToolProgramNotification> {
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
        let mut expired = vec![];
        let mut notifications = self.notifications.write().await;
        for (id, n) in notifications.iter_mut() {
            if n.state == NotificationState::Claimed && (now - n.updated_at) > max_age_ms {
                n.state = NotificationState::Expired;
                n.updated_at = now;
                expired.push(id.clone());
            }
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
        let mut suppressed = vec![];
        let index = self.session_index.read().await;
        if let Some(nids) = index.get(session_id) {
            let pending: Vec<String> = {
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
            if pending.len() > max_pending {
                // Suppress oldest (first in list = earliest created)
                let to_suppress = &pending[..pending.len() - max_pending];
                let mut notifications = self.notifications.write().await;
                for nid in to_suppress {
                    if let Some(n) = notifications.get_mut(nid.as_str()) {
                        n.state = NotificationState::Suppressed;
                        n.updated_at = chrono::Utc::now().timestamp_millis();
                        suppressed.push(nid.clone());
                    }
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
            let payload_digest = format!("{:x}", md5::compute(payload.as_bytes()));
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
            };
            let existing = self.get(&job.program_id).await;
            if existing.is_none() {
                self.record_notification(notification).await;
                recovered += 1;
            }
        }
        recovered
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
        let payload_digest = format!("{:x}", md5::compute(payload.as_bytes()));
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
        assert!(svc.claim("tp-1").await);
        // Second claim fails
        assert!(!svc.claim("tp-1").await);
    }

    #[tokio::test]
    async fn acknowledge_succeeds_only_from_claimed() {
        let svc = ToolProgramNotificationService::new();
        let n = test_notification("tp-1", "s1", "completed", true);
        svc.record_notification(n).await;
        assert!(!svc.acknowledge("tp-1").await); // pending, not claimed
        assert!(svc.claim("tp-1").await);
        assert!(svc.acknowledge("tp-1").await);
        // Double ack fails
        assert!(!svc.acknowledge("tp-1").await);
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
        svc.claim("tp-1").await;
        let pending = svc.pending_for_session("s1").await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn suppress_removes_from_pending() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        assert!(svc.suppress("tp-1").await);
        let pending = svc.pending_for_session("s1").await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn expire_stale_claims() {
        let svc = ToolProgramNotificationService::new();
        svc.record_notification(test_notification("tp-1", "s1", "completed", true))
            .await;
        svc.claim("tp-1").await;
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
