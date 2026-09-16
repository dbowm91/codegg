//! M001 storage/integration tests for durable continuation checkpoints.

use chrono::Utc;
use codegg_core::session::continuation::{
    continuation_event_id, ContinuationCheckpointPayload, ContinuationCheckpointStatus,
    ContinuationCheckpointStore, CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES,
};
use codegg_core::session::events::{ContextCompactedEvent, EventMeta, SessionEvent};
use codegg_core::session::schema;
use codegg_core::session::store::EventStore;
use codegg_core::storage::STORAGE_LAYOUT_VERSION;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn isolated_pool() -> SqlitePool {
    let name = format!("continuation_test_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    schema::migrate(&pool).await.expect("migrate");
    pool
}

async fn ensure_session(pool: &SqlitePool, session_id: &str) {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES ('continuation-project', '/tmp/continuation', '[]', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, 'continuation-project', 'continuation', \
         '/tmp/continuation', 'Continuation', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

fn payload_with(objective: &str) -> ContinuationCheckpointPayload {
    ContinuationCheckpointPayload::new(serde_json::json!({
        "objective": objective,
        "current_task": "port validation",
        "next_action": "run tests",
    }))
    .unwrap()
}

fn compacted_event(session_id: &str, checkpoint_seq_hint: &str) -> ContextCompactedEvent {
    let _ = checkpoint_seq_hint;
    ContextCompactedEvent {
        meta: EventMeta::new(session_id),
        messages_removed: 10,
        messages_remaining: 5,
        token_estimate_before: Some(10000),
        token_estimate_after: Some(3000),
        pinned_items: vec!["goal".to_string()],
        summarized_items: vec![],
        dropped_items: vec![],
        checkpoint_id: None,
        checkpoint_digest: None,
        epoch_sequence: None,
        previous_checkpoint_id: None,
        continuity_degraded_reason: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn migration_is_additive_and_layout_tracks_v58() {
    let pool = isolated_pool().await;
    assert_eq!(STORAGE_LAYOUT_VERSION, 59);
    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(version, 59);

    for table in [
        "continuation_checkpoint",
        "checkpoints",
        "session_events",
        "session",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
    for index in [
        "idx_continuation_checkpoint_latest",
        "idx_continuation_checkpoint_session_id",
        "idx_continuation_checkpoint_lineage",
    ] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
        )
        .bind(index)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing index {index}");
    }
    // Remigration is a restart-safe no-op.
    schema::migrate(&pool).await.expect("remigrate");
    let rerun: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rerun, 59);
}

#[tokio::test(flavor = "current_thread")]
async fn prepare_get_and_latest_installed() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-prepare").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    assert!(store
        .latest_installed("session-prepare")
        .await
        .unwrap()
        .is_none());

    let prepared = store
        .prepare("session-prepare", None, payload_with("objective one"))
        .await
        .unwrap();
    assert_eq!(prepared.status, ContinuationCheckpointStatus::Prepared);
    assert_eq!(prepared.sequence, 1);
    assert_eq!(prepared.previous_installed_id, None);

    // Prepared rows are ignored for resume.
    assert!(store
        .latest_installed("session-prepare")
        .await
        .unwrap()
        .is_none());

    let loaded = store
        .get("session-prepare", &prepared.id)
        .await
        .unwrap()
        .expect("prepared row readable");
    assert_eq!(loaded, prepared);

    let installed = store
        .install_with_compaction_event(
            "session-prepare",
            &prepared.id,
            compacted_event("session-prepare", "first"),
        )
        .await
        .unwrap();
    assert_eq!(installed.status, ContinuationCheckpointStatus::Installed);
    assert!(installed.installed_at.is_some());

    let latest = store
        .latest_installed("session-prepare")
        .await
        .unwrap()
        .expect("installed row is resume authority");
    assert_eq!(latest.id, prepared.id);
    assert_eq!(latest.sequence, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn lineage_sequences_and_second_epoch() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-lineage").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let first = store
        .prepare("session-lineage", None, payload_with("epoch one"))
        .await
        .unwrap();
    let first = store
        .install_with_compaction_event(
            "session-lineage",
            &first.id,
            compacted_event("session-lineage", "first"),
        )
        .await
        .unwrap();

    let second = store
        .prepare(
            "session-lineage",
            Some(&first.id),
            payload_with("epoch two"),
        )
        .await
        .unwrap();
    assert_eq!(second.sequence, 2);
    assert_eq!(
        second.previous_installed_id.as_deref(),
        Some(first.id.as_str())
    );

    let second = store
        .install_with_compaction_event(
            "session-lineage",
            &second.id,
            compacted_event("session-lineage", "second"),
        )
        .await
        .unwrap();
    let latest = store
        .latest_installed("session-lineage")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.id, second.id);

    let listed = store.list_for_session("session-lineage", 10).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].sequence, 1);
    assert_eq!(listed[1].sequence, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn install_and_event_commit_atomically() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-atomic").await;
    let store = ContinuationCheckpointStore::new(pool.clone());
    let events = EventStore::new(pool.clone());

    let prepared = store
        .prepare("session-atomic", None, payload_with("atomic epoch"))
        .await
        .unwrap();
    let installed = store
        .install_with_compaction_event(
            "session-atomic",
            &prepared.id,
            compacted_event("session-atomic", "atomic"),
        )
        .await
        .unwrap();

    let stored = events.list_for_session("session-atomic").await.unwrap();
    assert_eq!(stored.len(), 1);
    match &stored[0] {
        SessionEvent::ContextCompacted(event) => {
            assert_eq!(event.meta.id, continuation_event_id(&prepared.id));
            assert_eq!(event.checkpoint_id.as_deref(), Some(prepared.id.as_str()));
            assert_eq!(
                event.checkpoint_digest.as_deref(),
                Some(installed.payload_digest.as_str())
            );
            assert_eq!(event.epoch_sequence, Some(installed.sequence));
            assert_eq!(event.previous_checkpoint_id, None);
        }
        other => panic!("unexpected event {}", other.event_type_tag()),
    }

    // A failed install (stale parent) leaves no event behind.
    let stale = store
        .prepare("session-atomic", None, payload_with("stale candidate"))
        .await
        .expect_err("stale prepare must fail");
    assert!(stale.to_string().contains("stale parent"));
    let stored = events.list_for_session("session-atomic").await.unwrap();
    assert_eq!(stored.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_install_retry_converges_idempotently() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-idempotent").await;
    let store = ContinuationCheckpointStore::new(pool.clone());
    let events = EventStore::new(pool.clone());

    let prepared = store
        .prepare("session-idempotent", None, payload_with("retry epoch"))
        .await
        .unwrap();
    let first = store
        .install_with_compaction_event(
            "session-idempotent",
            &prepared.id,
            compacted_event("session-idempotent", "first"),
        )
        .await
        .unwrap();
    // Retry with a freshly stamped event carrying identical content.
    let second = store
        .install_with_compaction_event(
            "session-idempotent",
            &prepared.id,
            compacted_event("session-idempotent", "retry"),
        )
        .await
        .unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(second.status, ContinuationCheckpointStatus::Installed);
    let stored = events.list_for_session("session-idempotent").await.unwrap();
    assert_eq!(stored.len(), 1);

    // A conflicting event for the same checkpoint identity fails closed.
    let mut conflicting = compacted_event("session-idempotent", "conflict");
    conflicting.messages_remaining = 999;
    conflicting.pinned_items = vec!["different".to_string()];
    let err = store
        .install_with_compaction_event("session-idempotent", &prepared.id, conflicting)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("collision"));
}

#[tokio::test(flavor = "current_thread")]
async fn same_parent_contenders_cannot_both_install() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-contention").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let first = store
        .prepare("session-contention", None, payload_with("base"))
        .await
        .unwrap();
    let first = store
        .install_with_compaction_event(
            "session-contention",
            &first.id,
            compacted_event("session-contention", "base"),
        )
        .await
        .unwrap();

    let contender_a = store
        .prepare(
            "session-contention",
            Some(&first.id),
            payload_with("contender a"),
        )
        .await
        .unwrap();
    let contender_b = store
        .prepare(
            "session-contention",
            Some(&first.id),
            payload_with("contender b"),
        )
        .await
        .unwrap();
    assert_ne!(contender_a.sequence, contender_b.sequence);

    store
        .install_with_compaction_event(
            "session-contention",
            &contender_a.id,
            compacted_event("session-contention", "a"),
        )
        .await
        .unwrap();
    let err = store
        .install_with_compaction_event(
            "session-contention",
            &contender_b.id,
            compacted_event("session-contention", "b"),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("stale parent"));

    let latest = store
        .latest_installed("session-contention")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.id, contender_a.id);
}

#[tokio::test(flavor = "current_thread")]
async fn abort_semantics_and_candidate_cleanup() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-abort").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let candidate = store
        .prepare("session-abort", None, payload_with("abortable"))
        .await
        .unwrap();
    let aborted = store
        .mark_aborted(
            "session-abort",
            &candidate.id,
            "superseded by fresher state",
        )
        .await
        .unwrap();
    assert_eq!(aborted.status, ContinuationCheckpointStatus::Aborted);
    assert!(aborted.aborted_at.is_some());

    // Re-abort is idempotent.
    let again = store
        .mark_aborted("session-abort", &candidate.id, "another reason")
        .await
        .unwrap();
    assert_eq!(again.status, ContinuationCheckpointStatus::Aborted);

    // Aborted rows are never resume authority and cannot install.
    assert!(store
        .latest_installed("session-abort")
        .await
        .unwrap()
        .is_none());
    let err = store
        .install_with_compaction_event(
            "session-abort",
            &candidate.id,
            compacted_event("session-abort", "aborted"),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("aborted"));

    // An installed checkpoint cannot be turned into Aborted.
    let durable = store
        .prepare("session-abort", None, payload_with("durable"))
        .await
        .unwrap();
    store
        .install_with_compaction_event(
            "session-abort",
            &durable.id,
            compacted_event("session-abort", "durable"),
        )
        .await
        .unwrap();
    let err = store
        .mark_aborted("session-abort", &durable.id, "too late")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("installed"));

    // Bounded cleanup removes candidates but never installed epochs.
    store
        .delete_candidate("session-abort", &candidate.id)
        .await
        .unwrap();
    assert!(store
        .get("session-abort", &candidate.id)
        .await
        .unwrap()
        .is_none());
    let err = store
        .delete_candidate("session-abort", &durable.id)
        .await
        .unwrap_err();
    assert!(matches!(err, codegg_core::error::StorageError::NotFound(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn restart_recovers_only_installed_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("restart.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    ensure_session(&pool, "session-restart").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let first = store
        .prepare("session-restart", None, payload_with("restart base"))
        .await
        .unwrap();
    store
        .install_with_compaction_event(
            "session-restart",
            &first.id,
            compacted_event("session-restart", "base"),
        )
        .await
        .unwrap();
    // An abandoned preparation must not become resume authority.
    let orphan = store
        .prepare("session-restart", Some(&first.id), payload_with("orphan"))
        .await
        .unwrap();
    let orphan_id = orphan.id.clone();
    let first_id = first.id.clone();
    pool.close().await;

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    let store = ContinuationCheckpointStore::new(pool.clone());
    let events = EventStore::new(pool.clone());

    let latest = store
        .latest_installed("session-restart")
        .await
        .unwrap()
        .expect("installed epoch survives restart");
    assert_eq!(latest.id, first_id);
    let orphan_row = store
        .get("session-restart", &orphan_id)
        .await
        .unwrap()
        .expect("prepared row remains for diagnostics");
    assert_eq!(orphan_row.status, ContinuationCheckpointStatus::Prepared);
    let stored = events.list_for_session("session-restart").await.unwrap();
    assert_eq!(stored.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn tampered_payload_fails_closed() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-tamper").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let prepared = store
        .prepare("session-tamper", None, payload_with("tamper target"))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE continuation_checkpoint SET payload_json = ? WHERE session_id = ? AND id = ?",
    )
    .bind(r#"{"schema_version":1,"body":{"objective":"forged"}}"#)
    .bind("session-tamper")
    .bind(&prepared.id)
    .execute(&pool)
    .await
    .unwrap();
    let err = store.get("session-tamper", &prepared.id).await.unwrap_err();
    assert!(err.to_string().contains("digest mismatch"));
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_and_invalid_inputs_rejected_before_insert() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-bounds").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let big = "y".repeat(CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES);
    let oversized = ContinuationCheckpointPayload {
        schema_version: 1,
        body: serde_json::json!({"objective": big}),
    };
    let err = store
        .prepare("session-bounds", None, oversized)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("exceeds"));
    let listed = store.list_for_session("session-bounds", 10).await.unwrap();
    assert!(listed.is_empty());

    assert!(store.prepare("", None, payload_with("x")).await.is_err());
    assert!(store
        .prepare("has/slash", None, payload_with("x"))
        .await
        .is_err());
    assert!(store.get("session-bounds", "").await.is_err());

    let prepared = store
        .prepare("session-bounds", None, payload_with("event bounds"))
        .await
        .unwrap();
    let mut oversized_event = compacted_event("session-bounds", "bounds");
    oversized_event.pinned_items = vec!["item".to_string(); 65];
    let err = store
        .install_with_compaction_event("session-bounds", &prepared.id, oversized_event)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("exceeds"));
    assert!(store
        .latest_installed("session-bounds")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_carry_no_payload_body() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "session-diagnostics").await;
    let store = ContinuationCheckpointStore::new(pool.clone());

    let distinctive = "diagnostic-body-marker-should-not-appear";
    let payload = ContinuationCheckpointPayload::new(serde_json::json!({
        "objective": "ordinary work",
    }))
    .unwrap();
    let prepared = store
        .prepare("session-diagnostics", None, payload)
        .await
        .unwrap();
    let summary = prepared.diagnostic_summary();
    assert!(!summary.contains(distinctive));
    assert!(!summary.contains("ordinary work"));
    assert!(summary.contains(&prepared.id));
    assert!(summary.contains(&prepared.payload_digest));
}
