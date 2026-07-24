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

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// The state of a background program notification.
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
}

impl ToolProgramNotificationService {
    pub fn new() -> Self {
        Self {
            notifications: RwLock::new(HashMap::new()),
            session_index: RwLock::new(HashMap::new()),
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
}

impl Default for ToolProgramNotificationService {
    fn default() -> Self {
        Self::new()
    }
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
}
