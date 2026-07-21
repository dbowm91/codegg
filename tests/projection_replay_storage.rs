mod common;

use std::sync::Arc;

use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::{ProjectionStreamKind, MAX_REPLAY_EVENTS};

#[tokio::test]
async fn session_stream_created_idempotently() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (d1, created1) = store
        .get_or_create_session_stream("s1", "p1", Some("w1"))
        .await
        .unwrap();
    assert!(created1);
    let (d2, created2) = store
        .get_or_create_session_stream("s1", "p1", Some("w1"))
        .await
        .unwrap();
    assert!(!created2);
    assert_eq!(d1.stream_id, d2.stream_id);
}

#[tokio::test]
async fn project_stream_created_idempotently() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (d1, created1) = store.get_or_create_project_stream("p1").await.unwrap();
    assert!(created1);
    let (d2, created2) = store.get_or_create_project_stream("p1").await.unwrap();
    assert!(!created2);
    assert_eq!(d1.stream_id, d2.stream_id);
}

#[tokio::test]
async fn concurrent_inserts_produce_contiguous_sequences() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id;

    let mut handles = vec![];
    for _ in 0..10 {
        let store = store.clone();
        let sid = sid.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..10 {
                let env = ProjectionEnvelope::session_event(
                    i,
                    0,
                    "s1",
                    None,
                    ProjectionEvent::Diagnostic {
                        code: "c".into(),
                        message: "m".into(),
                    },
                );
                let seq = store.next_event_seq(sid.as_str()).await.unwrap();
                store
                    .insert_event(sid.as_str(), seq, &env)
                    .await
                    .unwrap();
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let events = store
        .events_after(sid.as_str(), 0, MAX_REPLAY_EVENTS, u64::MAX)
        .await
        .unwrap();
    assert_eq!(events.len(), 100);
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.event_seq, (i as u64) + 1);
    }
}

#[tokio::test]
async fn restart_allocates_above_persisted_high_water() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool.clone());
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..5 {
        let env = ProjectionEnvelope::session_event(
            i,
            0,
            "s1",
            None,
            ProjectionEvent::Diagnostic {
                code: "c".into(),
                message: "m".into(),
            },
        );
        let seq = store.next_event_seq(sid).await.unwrap();
        store.insert_event(sid, seq, &env).await.unwrap();
        store.update_high_water(sid, seq).await.unwrap();
    }

    let store2 = ProjectionReplayStore::new(pool);
    let desc2 = store2.lookup_stream_by_id(sid).await.unwrap().unwrap();
    assert_eq!(desc2.high_water_seq, 5);

    let env = ProjectionEnvelope::session_event(
        99,
        0,
        "s1",
        None,
        ProjectionEvent::Diagnostic {
            code: "c".into(),
            message: "m".into(),
        },
    );
    let next = store2.next_event_seq(sid).await.unwrap();
    assert!(next > desc2.high_water_seq);
    store2.insert_event(sid, next, &env).await.unwrap();
    store2.update_high_water(sid, next).await.unwrap();

    let desc3 = store2.lookup_stream_by_id(sid).await.unwrap().unwrap();
    assert_eq!(desc3.high_water_seq, next);
}

#[tokio::test]
async fn rollback_leaves_no_visible_event() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    let env = ProjectionEnvelope::session_event(
        0,
        0,
        "s1",
        None,
        ProjectionEvent::Diagnostic {
            code: "c".into(),
            message: "m".into(),
        },
    );

    let seq = store.next_event_seq(sid).await.unwrap();
    store.insert_event(sid, seq, &env).await.unwrap();

    let exists = store.event_exists(sid, seq).await.unwrap();
    assert!(exists);

    sqlx::query("DELETE FROM projection_event WHERE stream_id = ? AND event_seq = ?")
        .bind(sid)
        .bind(seq as i64)
        .execute(store.pool())
        .await
        .unwrap();

    let exists = store.event_exists(sid, seq).await.unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn events_after_returns_correct_window() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..10 {
        let env = ProjectionEnvelope::session_event(
            i,
            i as i64,
            "s1",
            None,
            ProjectionEvent::Diagnostic {
                code: format!("c{i}"),
                message: format!("m{i}"),
            },
        );
        store.insert_event(sid, i, &env).await.unwrap();
    }

    let rows = store.events_after(sid, 3, 3, u64::MAX).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].event_seq, 4);
    assert_eq!(rows[1].event_seq, 5);
    assert_eq!(rows[2].event_seq, 6);
}

#[tokio::test]
async fn checkpoint_round_trip() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    store
        .write_checkpoint(sid, 10, 1, r#"{"test":"data"}"#)
        .await
        .unwrap();

    let cp = store.load_checkpoint_at_or_before(sid, 10).await.unwrap();
    assert!(cp.is_some());
    let cp = cp.unwrap();
    assert_eq!(cp.checkpoint_seq, 10);
    assert_eq!(cp.snapshot_json, r#"{"test":"data"}"#);

    let cp2 = store.load_checkpoint_at_or_before(sid, 5).await.unwrap();
    assert!(cp2.is_none());
}

#[tokio::test]
async fn prune_events_and_preserve_floor() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..10 {
        let env = ProjectionEnvelope::session_event(
            i,
            0,
            "s1",
            None,
            ProjectionEvent::Diagnostic {
                code: "c".into(),
                message: "m".into(),
            },
        );
        store.insert_event(sid, i, &env).await.unwrap();
    }

    let pruned = store.prune_before(sid, 5).await.unwrap();
    assert_eq!(pruned, 5);

    let remaining = store.events_after(sid, 0, 100, u64::MAX).await.unwrap();
    assert_eq!(remaining.len(), 5);
    assert_eq!(remaining[0].event_seq, 5);

    let desc = store.lookup_stream_by_id(sid).await.unwrap().unwrap();
    assert_eq!(desc.retention_floor_seq, 5);
}
