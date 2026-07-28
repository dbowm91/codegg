//! M015 fail-closed notification persistence and independent-instance recovery.

mod common;

use codegg::scheduler::tool_program_notifications::{
    NotificationState, ProgramHandle, ToolProgramNotification, ToolProgramNotificationService,
};

fn notification(id: &str) -> ToolProgramNotification {
    let now = chrono::Utc::now().timestamp_millis();
    ToolProgramNotification {
        notification_id: id.into(),
        program_id: id.into(),
        job_id: format!("job-{id}"),
        session_id: "session-m015".into(),
        agent_id: Some("agent-m015".into()),
        turn_id: Some("turn-m015".into()),
        status: "completed".into(),
        summary: "completed".into(),
        failure_class: None,
        success: true,
        classification: codegg_protocol::projection::dto::NotificationClassification::Completed,
        payload_digest: format!("sha256:{id}"),
        program_handle: ProgramHandle {
            program_id: id.into(),
            job_id: format!("job-{id}"),
            status: "terminal".into(),
            submitted_at: now,
            timeout_ms: 1_000,
            inspect_ref: id.into(),
            cancel_ref: format!("job-{id}"),
        },
        state: NotificationState::Pending,
        created_at: now,
        updated_at: now,
        claim_owner: None,
        claim_lease_until: None,
        delivered_at: None,
        retry_count: 0,
        injection_key: Some(format!("tp-inject:{id}:session-m015")),
        injected_event_id: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn sqlite_write_failure_returns_error_without_cache_success() {
    let pool = common::pool::isolated_pool().await;
    let service = ToolProgramNotificationService::with_pool(pool.clone());
    pool.close().await;
    assert!(service
        .record_notification(notification("tp-write-fail"))
        .await
        .is_err());
    assert!(service.get("tp-write-fail").await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn independent_services_have_one_durable_claim_winner() {
    let pool = common::pool::isolated_pool().await;
    let first = ToolProgramNotificationService::with_pool(pool.clone());
    let second = ToolProgramNotificationService::with_pool(pool);
    first
        .record_notification(notification("tp-claim"))
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        first.claim_as("tp-claim", "daemon-a"),
        second.claim_as("tp-claim", "daemon-b")
    );
    assert_ne!(a.unwrap(), b.unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn append_before_ack_identity_survives_independent_instance() {
    let pool = common::pool::isolated_pool().await;
    let first = ToolProgramNotificationService::with_pool(pool.clone());
    first
        .record_notification(notification("tp-append"))
        .await
        .unwrap();
    assert!(first.claim("tp-append").await.unwrap());
    first
        .mark_injected("tp-append", "session-event-1")
        .await
        .unwrap();

    let second = ToolProgramNotificationService::with_pool(pool);
    let recovered = second.get("tp-append").await.unwrap().unwrap();
    assert_eq!(
        recovered.injected_event_id.as_deref(),
        Some("session-event-1")
    );
    assert!(second.acknowledge("tp-append").await.unwrap());
    assert!(!second.claim("tp-append").await.unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn recovery_query_failure_is_not_reported_as_zero_records() {
    let pool = common::pool::isolated_pool().await;
    let service = ToolProgramNotificationService::with_pool(pool.clone());
    pool.close().await;
    assert!(service.recover_from_pool().await.is_err());
}
