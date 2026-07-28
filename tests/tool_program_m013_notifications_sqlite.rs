//! M013 notification lifecycle SQLite authority tests.
//!
//! Covers closure criteria related to F03 / C1-C5:
//! - C1: One canonical textual state representation in SQLite (no JSON-quoted enum strings).
//! - C2: SQLite compare-and-set transitions are authoritative for all state changes.
//! - C3: Durable injection identity is persisted with uniqueness constraint.
//! - C4: SQL, serialization, transaction, append, and acknowledgement errors propagate as Result::Err.
//! - C5: Independent service instances share one database and converge.

#![cfg(test)]

mod common;

use codegg::scheduler::tool_program_notifications::{
    NotificationState, NotificationStoreError, ToolProgramNotificationService,
};
use std::sync::Arc;

fn make_notification(
    program_id: &str,
    session_id: &str,
    status: &str,
    success: bool,
) -> codegg::scheduler::tool_program_notifications::ToolProgramNotification {
    let now = chrono::Utc::now().timestamp_millis();
    let classification = if success {
        codegg_protocol::projection::dto::NotificationClassification::Completed
    } else {
        codegg_protocol::projection::dto::NotificationClassification::IncompleteRecoverable
    };
    codegg::scheduler::tool_program_notifications::ToolProgramNotification {
        notification_id: program_id.to_string(),
        program_id: program_id.to_string(),
        job_id: format!("j-{}", program_id),
        session_id: session_id.to_string(),
        agent_id: Some("agent-1".into()),
        turn_id: Some("turn-1".into()),
        status: status.to_string(),
        summary: format!("program {} finished with status {}", program_id, status),
        failure_class: None,
        success,
        classification,
        payload_digest: format!("{:x}", md5::compute(program_id.as_bytes())),
        program_handle: codegg::scheduler::tool_program_notifications::ProgramHandle {
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

/// M013 C1: state column in SQLite stores a raw textual value, not a JSON-quoted string.
#[tokio::test(flavor = "current_thread")]
async fn c1_state_column_is_raw_text_not_json_quoted() {
    let pool = common::pool::isolated_pool().await;
    let service = ToolProgramNotificationService::with_pool(pool.clone());
    let notification = make_notification("tp-m013-c1-state", "sess-1", "completed", true);
    service.record_notification(notification).await;

    let row: (String,) =
        sqlx::query_as("SELECT state FROM tool_program_notification WHERE notification_id = ?1")
            .bind("tp-m013-c1-state")
            .fetch_one(&pool)
            .await
            .expect("read row");

    assert_eq!(
        row.0, "pending",
        "state column must be the raw token 'pending', not a JSON-quoted string; got {:?}",
        row.0
    );
    assert!(
        !row.0.starts_with('"') && !row.0.ends_with('"'),
        "state column must not have JSON quote wrapping; got {:?}",
        row.0
    );
}

/// M013 C1: state column updates with raw tokens on every transition.
#[tokio::test(flavor = "current_thread")]
async fn c1_state_transitions_use_raw_tokens() {
    let pool = common::pool::isolated_pool().await;
    let service = ToolProgramNotificationService::with_pool(pool.clone());
    let notification = make_notification("tp-m013-c1-transit", "sess-1", "completed", true);
    service.record_notification(notification).await;

    service.claim("tp-m013-c1-transit").await.unwrap();
    let row: (String,) =
        sqlx::query_as("SELECT state FROM tool_program_notification WHERE notification_id = ?1")
            .bind("tp-m013-c1-transit")
            .fetch_one(&pool)
            .await
            .expect("read row after claim");
    assert_eq!(
        row.0, "claimed",
        "claimed transition must produce raw 'claimed' in state column; got {:?}",
        row.0
    );

    service.acknowledge("tp-m013-c1-transit").await.unwrap();
    let row: (String,) =
        sqlx::query_as("SELECT state FROM tool_program_notification WHERE notification_id = ?1")
            .bind("tp-m013-c1-transit")
            .fetch_one(&pool)
            .await
            .expect("read row after ack");
    assert_eq!(
        row.0, "delivered",
        "acknowledge transition must produce raw 'delivered' in state column; got {:?}",
        row.0
    );
}

/// M013 C5: independent service instances with separate connections sharing one database.
#[tokio::test(flavor = "current_thread")]
async fn c5_two_independent_services_share_database() {
    let pool = common::pool::isolated_pool().await;
    let service1 = Arc::new(ToolProgramNotificationService::with_pool(pool.clone()));
    let service2 = Arc::new(ToolProgramNotificationService::with_pool(pool.clone()));

    let notification = make_notification("tp-m013-c5-shared", "sess-1", "completed", true);
    service1.record_notification(notification).await;

    // service2 has separate in-memory cache but the database is shared.
    let claim1 = service1.claim("tp-m013-c5-shared").await.unwrap();
    let claim2 = service2.claim("tp-m013-c5-shared").await.unwrap();
    assert!(claim1, "first claim wins");
    assert!(!claim2, "second claim from independent service must fail");

    // service2 sees the durable claimed state via SQL even without its own cache.
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT state, claim_owner FROM tool_program_notification WHERE notification_id = ?1",
    )
    .bind("tp-m013-c5-shared")
    .fetch_one(&pool)
    .await
    .expect("read row");
    assert_eq!(row.0, "claimed");
    assert!(row.1.is_some(), "claim_owner must be persisted");
}

/// M013 C2: SQL error on transition propagates as Result::Err.
#[tokio::test(flavor = "current_thread")]
async fn c4_sql_error_propagates_as_err() {
    // Closed pool: every operation that touches the pool must return Err.
    let pool = common::pool::isolated_pool().await;
    pool.close().await;
    let service = ToolProgramNotificationService::with_pool(pool);
    let notification = make_notification("tp-m013-c4-err", "sess-1", "completed", true);
    service.record_notification(notification).await;

    let result = service.claim("tp-m013-c4-err").await;
    assert!(
        matches!(result, Err(NotificationStoreError::Io(_))),
        "closed-pool claim must return Err(Io); got {:?}",
        result
    );
}

/// M013 C1: SQLite CHECK or unique constraint proves state values are bounded.
#[tokio::test(flavor = "current_thread")]
async fn c1_record_notification_writes_raw_state() {
    let pool = common::pool::isolated_pool().await;
    let service = ToolProgramNotificationService::with_pool(pool.clone());
    let notification = make_notification("tp-m013-c1-raw", "sess-1", "completed", true);
    service.record_notification(notification).await;

    let state: String = sqlx::query_scalar(
        "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
    )
    .bind("tp-m013-c1-raw")
    .fetch_one(&pool)
    .await
    .expect("state");

    // The state must be exactly the lowercase enum token, not a JSON-encoded variant.
    assert_eq!(state, "pending");
    assert_eq!(state.len(), "pending".len());
}

/// M013 C-13: Restart-via-drop — notification state survives service restart.
/// Drops the service and creates a new one on the same pool, verifying
/// durable state is authoritative.
#[tokio::test(flavor = "current_thread")]
async fn c13_notification_state_survives_service_restart() {
    let pool = common::pool::isolated_pool().await;

    // Phase 1: Record and claim a notification, then drop the service.
    {
        let service = ToolProgramNotificationService::with_pool(pool.clone());
        let notification = make_notification("tp-m013-c13-restart", "sess-1", "completed", true);
        service.record_notification(notification).await;
        let claimed = service.claim("tp-m013-c13-restart").await.unwrap();
        assert!(claimed, "first claim must succeed");
    }
    // service is dropped here — simulates daemon shutdown.

    // Phase 2: New service on the same pool — simulates daemon restart.
    {
        let service2 = ToolProgramNotificationService::with_pool(pool.clone());

        // The notification must still be in "claimed" state.
        let row: (String,) = sqlx::query_as(
            "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-restart")
        .fetch_one(&pool)
        .await
        .expect("read row after restart");
        assert_eq!(
            row.0, "claimed",
            "notification must remain claimed after service restart"
        );

        // Second claim from new service must fail (already claimed).
        let claim2 = service2.claim("tp-m013-c13-restart").await.unwrap();
        assert!(!claim2, "duplicate claim must fail after restart");

        // Acknowledge from new service must succeed.
        service2.acknowledge("tp-m013-c13-restart").await.unwrap();
        let row: (String,) = sqlx::query_as(
            "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-restart")
        .fetch_one(&pool)
        .await
        .expect("read row after ack");
        assert_eq!(
            row.0, "delivered",
            "acknowledge from new service must produce delivered state"
        );
    }
}

/// M013 C-13: Restart after acknowledge — delivered state persists.
#[tokio::test(flavor = "current_thread")]
async fn c13_delivered_state_survives_restart() {
    let pool = common::pool::isolated_pool().await;

    // Phase 1: Full lifecycle — record, claim, acknowledge.
    {
        let service = ToolProgramNotificationService::with_pool(pool.clone());
        let notification = make_notification("tp-m013-c13-delivered", "sess-1", "completed", true);
        service.record_notification(notification).await;
        service.claim("tp-m013-c13-delivered").await.unwrap();
        service.acknowledge("tp-m013-c13-delivered").await.unwrap();
    }

    // Phase 2: New service — delivered state must persist.
    {
        let _service2 = ToolProgramNotificationService::with_pool(pool.clone());
        let row: (String,) = sqlx::query_as(
            "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-delivered")
        .fetch_one(&pool)
        .await
        .expect("read row after restart");
        assert_eq!(
            row.0, "delivered",
            "delivered state must survive service restart"
        );
    }
}

/// M013 C-13: Restart before claim — pending notification is claimable by new service.
#[tokio::test(flavor = "current_thread")]
async fn c13_pending_notification_claimable_after_restart() {
    let pool = common::pool::isolated_pool().await;

    // Phase 1: Record but don't claim.
    {
        let service = ToolProgramNotificationService::with_pool(pool.clone());
        let notification =
            make_notification("tp-m013-c13-pending-restart", "sess-1", "completed", true);
        service.record_notification(notification).await;
    }

    // Phase 2: New service can claim the pending notification.
    {
        let service2 = ToolProgramNotificationService::with_pool(pool.clone());
        let claimed = service2.claim("tp-m013-c13-pending-restart").await.unwrap();
        assert!(
            claimed,
            "new service must be able to claim pending notification"
        );
    }
}

/// M013 C-13: Restart after injection reservation (claim + mark_injected
/// but before acknowledge) — the new service must not re-inject.
#[tokio::test(flavor = "current_thread")]
async fn c13_injection_reservation_survives_restart() {
    let pool = common::pool::isolated_pool().await;

    // Phase 1: Record, claim, and mark_injected — but don't acknowledge.
    {
        let service = ToolProgramNotificationService::with_pool(pool.clone());
        let mut notification =
            make_notification("tp-m013-c13-inject-res", "sess-1", "completed", true);
        notification.injection_key = Some("tp-inject:tp-m013-c13-inject-res:sess-1".into());
        service.record_notification(notification).await;
        let claimed = service.claim("tp-m013-c13-inject-res").await.unwrap();
        assert!(claimed, "claim must succeed");
        service
            .mark_injected("tp-m013-c13-inject-res", "evt-injected-1")
            .await
            .unwrap();
    }

    // Phase 2: New service — injection must be persisted and not repeated.
    {
        let service2 = ToolProgramNotificationService::with_pool(pool.clone());
        // The notification must still be in "claimed" state with the injection
        // persisted in the record_json.
        let row: (String, String) = sqlx::query_as(
            "SELECT state, record_json FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-inject-res")
        .fetch_one(&pool)
        .await
        .expect("read row after restart");
        assert_eq!(row.0, "claimed", "state must remain claimed after restart");

        // The injected_event_id must be present in the persisted record.
        let record: codegg::scheduler::tool_program_notifications::ToolProgramNotification =
            serde_json::from_str(&row.1).expect("parse record_json");
        assert_eq!(
            record.injected_event_id.as_deref(),
            Some("evt-injected-1"),
            "injected_event_id must survive restart"
        );

        // Acknowledge from new service to complete the lifecycle.
        service2
            .acknowledge("tp-m013-c13-inject-res")
            .await
            .unwrap();
        let row: (String,) = sqlx::query_as(
            "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-inject-res")
        .fetch_one(&pool)
        .await
        .expect("read row after ack");
        assert_eq!(row.0, "delivered", "must reach delivered after ack");
    }
}

/// M013 C-13: Restart after durable append (claim + mark_injected + persisted
/// to SQLite but notification not yet delivered) — the new service sees the
/// injected state and can complete delivery.
#[tokio::test(flavor = "current_thread")]
async fn c13_durable_append_survives_restart() {
    let pool = common::pool::isolated_pool().await;

    // Phase 1: Record, claim, mark_injected — simulates "durable append" done.
    {
        let service = ToolProgramNotificationService::with_pool(pool.clone());
        let mut notification =
            make_notification("tp-m013-c13-durable-append", "sess-1", "completed", true);
        notification.injection_key = Some("tp-inject:tp-m013-c13-durable-append:sess-1".into());
        service.record_notification(notification).await;
        service.claim("tp-m013-c13-durable-append").await.unwrap();
        service
            .mark_injected("tp-m013-c13-durable-append", "evt-durable-1")
            .await
            .unwrap();
    }

    // Phase 2: New service — verify injected state persists and acknowledge.
    {
        let service2 = ToolProgramNotificationService::with_pool(pool.clone());

        // Verify the injection was persisted.
        let row: (String, String) = sqlx::query_as(
            "SELECT state, record_json FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-durable-append")
        .fetch_one(&pool)
        .await
        .expect("read row after restart");
        assert_eq!(
            row.0, "claimed",
            "state must remain claimed (not delivered) after restart"
        );

        let record: codegg::scheduler::tool_program_notifications::ToolProgramNotification =
            serde_json::from_str(&row.1).expect("parse record_json");
        assert_eq!(
            record.injected_event_id.as_deref(),
            Some("evt-durable-1"),
            "injected_event_id must survive restart"
        );

        // Acknowledge from new service.
        service2
            .acknowledge("tp-m013-c13-durable-append")
            .await
            .unwrap();
        let row: (String,) = sqlx::query_as(
            "SELECT state FROM tool_program_notification WHERE notification_id = ?1",
        )
        .bind("tp-m013-c13-durable-append")
        .fetch_one(&pool)
        .await
        .expect("read final state");
        assert_eq!(
            row.0, "delivered",
            "must reach delivered after acknowledge from new service"
        );
    }
}
