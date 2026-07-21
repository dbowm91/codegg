mod common;

use std::sync::Arc;

use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};

#[tokio::test]
async fn duplicate_publish_returns_existing_event() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = ProjectionReplayService::new(store.clone());

    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    let env = ProjectionEnvelope::session_event(
        1,
        0,
        "s1",
        None,
        ProjectionEvent::Diagnostic {
            code: "c".into(),
            message: "m".into(),
        },
    );

    let seq1 = store
        .next_event_seq(desc.stream_id.as_str())
        .await
        .unwrap();
    store
        .insert_event(desc.stream_id.as_str(), seq1, &env)
        .await
        .unwrap();

    let exists = store
        .event_exists(desc.stream_id.as_str(), seq1)
        .await
        .unwrap();
    assert!(exists);
}

#[tokio::test]
async fn insert_then_delete_leaves_no_trace() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    let env = ProjectionEnvelope::session_event(
        1,
        0,
        "s1",
        None,
        ProjectionEvent::Diagnostic {
            code: "c".into(),
            message: "m".into(),
        },
    );

    let seq = store
        .next_event_seq(desc.stream_id.as_str())
        .await
        .unwrap();
    store
        .insert_event(desc.stream_id.as_str(), seq, &env)
        .await
        .unwrap();

    assert!(store
        .event_exists(desc.stream_id.as_str(), seq)
        .await
        .unwrap());

    sqlx::query("DELETE FROM projection_event WHERE stream_id = ? AND event_seq = ?")
        .bind(desc.stream_id.as_str())
        .bind(seq as i64)
        .execute(store.pool())
        .await
        .unwrap();

    assert!(!store
        .event_exists(desc.stream_id.as_str(), seq)
        .await
        .unwrap());
}

#[tokio::test]
async fn checkpoint_persists_across_reopen() {
    let pool = common::projection_replay::test_pool().await;
    let store1 = Arc::new(ProjectionReplayStore::new(pool.clone()));
    let (desc, _) = store1
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    store1
        .write_checkpoint(desc.stream_id.as_str(), 50, 1, r#"{"v":1}"#)
        .await
        .unwrap();

    let store2 = ProjectionReplayStore::new(pool);
    let cp = store2
        .load_checkpoint_at_or_before(desc.stream_id.as_str(), 50)
        .await
        .unwrap();
    assert!(cp.is_some());
    assert_eq!(cp.unwrap().snapshot_json, r#"{"v":1}"#);
}

#[tokio::test]
async fn service_metrics_snapshot_works() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = ProjectionReplayService::new(store);

    let snap = service.metrics_snapshot();
    assert_eq!(snap.events_persisted_total, 0);
    assert_eq!(snap.publication_failures, 0);
}
