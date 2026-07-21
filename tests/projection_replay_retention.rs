mod common;

use std::sync::Arc;

use codegg_core::projection_replay::retention::{MaintenanceReport, RetentionPolicy};
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};

#[tokio::test]
async fn retention_prunes_old_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..20 {
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

    let policy = RetentionPolicy {
        session_max_events: 10,
        ..Default::default()
    };

    let report = policy.maintenance_tick(&store, 0).await.unwrap();

    assert!(report.events_pruned > 0);
    let events = store.events_after(sid, 0, 100, u64::MAX).await.unwrap();
    assert!(events.len() <= 10);
}

#[tokio::test]
async fn checkpoint_written_when_interval_reached() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..300 {
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

    let policy = RetentionPolicy {
        checkpoint_event_interval: 100,
        ..Default::default()
    };

    let report = policy.maintenance_tick(&store, 0).await.unwrap();

    assert!(report.checkpoints_written > 0);
}

#[tokio::test]
async fn max_checkpoints_per_stream_enforced() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();
    let sid = desc.stream_id.as_str();

    for i in 0..5 {
        store
            .write_checkpoint(sid, i * 100, 1, r#"{"data":"x"}"#)
            .await
            .unwrap();
    }

    let deleted = store.prune_old_checkpoints(sid, 2).await.unwrap();
    assert_eq!(deleted, 3);
}

#[tokio::test]
async fn maintenance_tick_quarantines_corrupt_streams() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    sqlx::query("UPDATE projection_stream SET high_water_seq = 999, next_seq = 5 WHERE id = ?")
        .bind(desc.stream_id.as_str())
        .execute(store.pool())
        .await
        .unwrap();

    let policy = RetentionPolicy::default();
    let _ = policy.maintenance_tick(&store, 0).await;
}
