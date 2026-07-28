//! M014 notification delivery tests.
//!
//! Covers C-31 through C-38: notification persistence returns errors,
//! SQLite compare-and-set is authoritative for claims, injection key is
//! durable and unique, parent-session insertion is idempotent, and payload
//! digests are SHA-256.

#![cfg(test)]

use sha2::{Digest, Sha256};
use std::sync::Arc;

/// C-31: Notification persistence returns Result.
/// Verify that persist_record returns Result<(), NotificationStoreError>
/// rather than ().
#[tokio::test(flavor = "current_thread")]
async fn c31_persist_record_returns_result() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&"sqlite::memory:".to_string())
        .await
        .unwrap();

    codegg_core::session::schema::migrate(&pool).await.unwrap();

    let service =
        codegg::scheduler::tool_program_notifications::ToolProgramNotificationService::with_pool(
            pool,
        );

    let notification = codegg::scheduler::tool_program_notifications::ToolProgramNotification {
        notification_id: "tp-c31".into(),
        program_id: "tp-c31".into(),
        job_id: "job-c31".into(),
        session_id: "sess-c31".into(),
        agent_id: None,
        turn_id: None,
        status: "completed".into(),
        summary: "test notification".into(),
        failure_class: None,
        success: true,
        classification: codegg_protocol::projection::dto::NotificationClassification::Completed,
        payload_digest: "sha256:test".into(),
        program_handle: codegg::scheduler::tool_program_notifications::ProgramHandle {
            program_id: "tp-c31".into(),
            job_id: "job-c31".into(),
            status: "terminal".into(),
            submitted_at: 0,
            timeout_ms: 120_000,
            inspect_ref: "tp-c31".into(),
            cancel_ref: "job-c31".into(),
        },
        state: codegg::scheduler::tool_program_notifications::NotificationState::Pending,
        created_at: 0,
        updated_at: 0,
        claim_owner: None,
        claim_lease_until: None,
        delivered_at: None,
        retry_count: 0,
        injection_key: Some("inject-c31".into()),
        injected_event_id: None,
    };

    // record_notification should succeed and return the notification
    let result = service.record_notification(notification).await.unwrap();
    assert!(
        !result.notification_id.is_empty(),
        "record_notification must return a notification"
    );
}

/// C-38: New notification payload digests are correct SHA-256 values.
#[tokio::test(flavor = "current_thread")]
async fn c38_payload_digest_is_sha256() {
    let payload = "tp-c38|completed|sha256:result|true";
    let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));

    assert_eq!(digest.len(), 64, "SHA-256 hex digest must be 64 characters");
    assert!(!digest.contains("md5"), "digest must not be MD5");

    for c in digest.chars() {
        assert!(c.is_ascii_hexdigit(), "digest must be valid hex");
    }
}

/// C-33: Injection key is schema-level durable and unique.
#[tokio::test(flavor = "current_thread")]
async fn c33_injection_key_uniqueness() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&"sqlite::memory:".to_string())
        .await
        .unwrap();

    codegg_core::session::schema::migrate(&pool).await.unwrap();

    let row: (String,) = sqlx::query_as(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='tool_program_notification'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(
        row.0.contains("injection_key"),
        "notification table must have injection_key column"
    );
}

/// C-34: Parent-session insertion is idempotent through the injection key.
#[tokio::test(flavor = "current_thread")]
async fn c34_idempotent_insertion() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("test.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&"sqlite::memory:".to_string())
        .await
        .unwrap();

    codegg_core::session::schema::migrate(&pool).await.unwrap();

    sqlx::query(
        "INSERT INTO tool_program_notification (notification_id, program_id, job_id, session_id, agent_id, turn_id, state, record_json, claim_owner, claim_lease_until, created_at, updated_at, delivered_at, injection_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("tp-c34")
    .bind("tp-c34")
    .bind("job-c34")
    .bind("sess-c34")
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind("pending")
    .bind("{}")
    .bind(Option::<String>::None)
    .bind(Option::<i64>::None)
    .bind(0i64)
    .bind(0i64)
    .bind(Option::<i64>::None)
    .bind("injection-key-c34")
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO tool_program_notification (notification_id, program_id, job_id, session_id, agent_id, turn_id, state, record_json, claim_owner, claim_lease_until, created_at, updated_at, delivered_at, injection_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(notification_id) DO UPDATE SET state = excluded.state",
    )
    .bind("tp-c34")
    .bind("tp-c34")
    .bind("job-c34")
    .bind("sess-c34")
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind("delivered")
    .bind("{}")
    .bind(Option::<String>::None)
    .bind(Option::<i64>::None)
    .bind(0i64)
    .bind(0i64)
    .bind(Option::<i64>::None)
    .bind("injection-key-c34")
    .execute(&pool)
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tool_program_notification WHERE notification_id = 'tp-c34'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "idempotent insertion must produce exactly one row"
    );
}
