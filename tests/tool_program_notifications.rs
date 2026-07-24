//! Integration tests for background tool program notifications.
//!
//! Tests the `ToolProgramNotificationService` end-to-end: recording,
//! claiming, acknowledging, suppression, expiry, session bounds, and
//! idempotency.

use codegg::scheduler::tool_program_notifications::{
    NotificationState, ProgramHandle, ToolProgramNotification, ToolProgramNotificationService,
};

fn make_notification(
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
        agent_id: Some("agent-1".into()),
        turn_id: Some("turn-1".into()),
        status: status.to_string(),
        summary: format!("program {} finished with status {}", program_id, status),
        failure_class: if success {
            None
        } else {
            Some("timeout".into())
        },
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
async fn record_and_retrieve_notification() {
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;

    let got = svc.get("tp-1").await;
    assert!(got.is_some());
    let got = got.unwrap();
    assert_eq!(got.program_id, "tp-1");
    assert_eq!(got.session_id, "s1");
    assert_eq!(got.status, "completed");
    assert!(got.success);
}

#[tokio::test]
async fn idempotent_record_does_not_overwrite() {
    let svc = ToolProgramNotificationService::new();
    let mut n1 = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n1.clone()).await;

    // Second record with different summary
    n1.summary = "changed".to_string();
    let result = svc.record_notification(n1).await;
    assert_eq!(
        result.summary,
        "program tp-1 finished with status completed"
    );
}

#[tokio::test]
async fn claim_succeeds_only_from_pending() {
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;

    // First claim succeeds
    assert!(svc.claim("tp-1").await);
    // Second claim fails (already claimed)
    assert!(!svc.claim("tp-1").await);
}

#[tokio::test]
async fn acknowledge_succeeds_only_from_claimed() {
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;

    // Cannot acknowledge pending
    assert!(!svc.acknowledge("tp-1").await);
    // Claim first
    assert!(svc.claim("tp-1").await);
    // Now acknowledge succeeds
    assert!(svc.acknowledge("tp-1").await);
    // Double acknowledge fails
    assert!(!svc.acknowledge("tp-1").await);
}

#[tokio::test]
async fn pending_for_session_filters_correctly() {
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-2", "s1", "failed", false))
        .await;
    svc.record_notification(make_notification("tp-3", "s2", "completed", true))
        .await;

    let pending_s1 = svc.pending_for_session("s1").await;
    assert_eq!(pending_s1.len(), 2);

    let pending_s2 = svc.pending_for_session("s2").await;
    assert_eq!(pending_s2.len(), 1);

    let pending_empty = svc.pending_for_session("nonexistent").await;
    assert!(pending_empty.is_empty());
}

#[tokio::test]
async fn claimed_not_in_pending() {
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.claim("tp-1").await;

    let pending = svc.pending_for_session("s1").await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn suppress_removes_from_pending() {
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;

    assert!(svc.suppress("tp-1").await);
    let pending = svc.pending_for_session("s1").await;
    assert!(pending.is_empty());

    let n = svc.get("tp-1").await.unwrap();
    assert_eq!(n.state, NotificationState::Suppressed);
}

#[tokio::test]
async fn expire_stale_claims() {
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.claim("tp-1").await;

    // Manually set updated_at to the past
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
        svc.record_notification(make_notification(
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
async fn notification_state_transitions() {
    let svc = ToolProgramNotificationService::new();
    let n = make_notification("tp-1", "s1", "completed", true);
    svc.record_notification(n).await;

    // Initial state
    let n = svc.get("tp-1").await.unwrap();
    assert_eq!(n.state, NotificationState::Pending);

    // Claim
    svc.claim("tp-1").await;
    let n = svc.get("tp-1").await.unwrap();
    assert_eq!(n.state, NotificationState::Claimed);

    // Acknowledge
    svc.acknowledge("tp-1").await;
    let n = svc.get("tp-1").await.unwrap();
    assert_eq!(n.state, NotificationState::Delivered);
}

#[tokio::test]
async fn multiple_sessions_isolated() {
    let svc = ToolProgramNotificationService::new();
    svc.record_notification(make_notification("tp-1", "s1", "completed", true))
        .await;
    svc.record_notification(make_notification("tp-2", "s2", "completed", true))
        .await;

    // Claiming in s1 doesn't affect s2
    svc.claim("tp-1").await;
    let pending_s1 = svc.pending_for_session("s1").await;
    assert!(pending_s1.is_empty());
    let pending_s2 = svc.pending_for_session("s2").await;
    assert_eq!(pending_s2.len(), 1);
}

#[tokio::test]
async fn program_handle_serialization() {
    let handle = ProgramHandle {
        program_id: "tp-1".into(),
        job_id: "j-tp-1".into(),
        status: "submitted".into(),
        submitted_at: 1234567890,
        timeout_ms: 120_000,
        inspect_ref: "tp-1".into(),
        cancel_ref: "j-tp-1".into(),
    };

    let json = serde_json::to_value(&handle).unwrap();
    assert_eq!(json["program_id"], "tp-1");
    assert_eq!(json["job_id"], "j-tp-1");
    assert_eq!(json["timeout_ms"], 120_000);

    let back: ProgramHandle = serde_json::from_value(json).unwrap();
    assert_eq!(back.program_id, "tp-1");
}

#[tokio::test]
async fn notification_serialization_roundtrip() {
    let n = make_notification("tp-1", "s1", "completed", true);
    let json = serde_json::to_value(&n).unwrap();
    let back: ToolProgramNotification = serde_json::from_value(json).unwrap();
    assert_eq!(back.program_id, "tp-1");
    assert_eq!(back.session_id, "s1");
    assert!(back.success);
}
