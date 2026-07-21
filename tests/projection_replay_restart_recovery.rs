mod common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};

#[tokio::test]
async fn restart_preserves_events_and_high_water() {
    let pool = common::projection_replay::test_pool().await;

    // Phase 1: Publish events through the first service instance
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store));
        let seam = Arc::new(ProjectionPublicationSeam::new(service));

        for i in 1..=5 {
            let env = EventEnvelope {
                protocol_version: PROTOCOL_VERSION,
                event_seq: i,
                timestamp_ms: i as i64 * 1000,
                session_id: Some("sess-1".into()),
                turn_id: None,
                payload: CoreEvent::TurnStarted {
                    session_id: "sess-1".into(),
                    turn_id: format!("turn-{}", i),
                },
            };
            let ctx = ProjectionPublicationContext {
                session_id: Some("sess-1".into()),
                project_id: Some("proj-1".into()),
                workspace_id: None,
                binding_revision: 1,
            };
            let _ = seam.publish(&env, ctx).await.unwrap();
        }
    }

    // Phase 2: Simulate restart - create new service from same pool
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));

        // Verify high water survived restart
        let desc = store
            .lookup_session_stream("sess-1", "proj-1")
            .await
            .unwrap();
        assert!(desc.is_some(), "stream should survive restart");
        let desc = desc.unwrap();
        assert_eq!(
            desc.high_water_seq, 5,
            "high water should be 5 after publishing 5 events"
        );

        // Verify events survive restart
        let events = store
            .events_after(desc.stream_id.as_str(), 0, 100, 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(events.len(), 5, "all 5 events should survive restart");
    }
}

#[tokio::test]
async fn restart_after_rebind_preserves_new_binding() {
    let pool = common::projection_replay::test_pool().await;

    // Phase 1: Create initial stream, rebind, publish
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let seam = Arc::new(ProjectionPublicationSeam::new(service));

        // Initial binding
        let env = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: 1,
            timestamp_ms: 1000,
            session_id: Some("sess-1".into()),
            turn_id: None,
            payload: CoreEvent::TurnStarted {
                session_id: "sess-1".into(),
                turn_id: "turn-1".into(),
            },
        };
        let ctx_a = ProjectionPublicationContext {
            session_id: Some("sess-1".into()),
            project_id: Some("proj-A".into()),
            workspace_id: None,
            binding_revision: 1,
        };
        let _ = seam.publish(&env, ctx_a).await.unwrap();

        // Rebind to proj-B
        let ctx_b = ProjectionPublicationContext {
            session_id: Some("sess-1".into()),
            project_id: Some("proj-B".into()),
            workspace_id: None,
            binding_revision: 2,
        };
        let _ = seam.publish(&env, ctx_b).await.unwrap();
    }

    // Phase 2: Restart and verify
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));

        // Old stream should be rebound
        let old = store
            .lookup_session_stream("sess-1", "proj-A")
            .await
            .unwrap();
        assert!(old.is_none(), "old proj-A stream should be rebound");

        // New stream should be active
        let new_desc = store
            .lookup_session_stream("sess-1", "proj-B")
            .await
            .unwrap();
        assert!(
            new_desc.is_some(),
            "new proj-B stream should survive restart"
        );
        let desc = new_desc.unwrap();
        assert_eq!(
            desc.high_water_seq, 1,
            "new stream's high water should be 1 (first event on new stream)"
        );
    }
}

#[tokio::test]
async fn no_sequence_reuse_across_restarts() {
    let pool = common::projection_replay::test_pool().await;

    // Publish 3 events
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store));
        let seam = Arc::new(ProjectionPublicationSeam::new(service));

        for i in 1..=3 {
            let env = EventEnvelope {
                protocol_version: PROTOCOL_VERSION,
                event_seq: i,
                timestamp_ms: i as i64 * 1000,
                session_id: Some("sess-1".into()),
                turn_id: None,
                payload: CoreEvent::TurnStarted {
                    session_id: "sess-1".into(),
                    turn_id: format!("turn-{}", i),
                },
            };
            let ctx = ProjectionPublicationContext {
                session_id: Some("sess-1".into()),
                project_id: Some("proj-1".into()),
                workspace_id: None,
                binding_revision: 1,
            };
            let _ = seam.publish(&env, ctx).await.unwrap();
        }
    }

    // Restart and publish 2 more events
    {
        let store = Arc::new(ProjectionReplayStore::new(pool.clone()));
        let service = Arc::new(ProjectionReplayService::new(store.clone()));
        let seam = Arc::new(ProjectionPublicationSeam::new(service));

        for i in 4..=5 {
            let env = EventEnvelope {
                protocol_version: PROTOCOL_VERSION,
                event_seq: i,
                timestamp_ms: i as i64 * 1000,
                session_id: Some("sess-1".into()),
                turn_id: None,
                payload: CoreEvent::TurnStarted {
                    session_id: "sess-1".into(),
                    turn_id: format!("turn-{}", i),
                },
            };
            let ctx = ProjectionPublicationContext {
                session_id: Some("sess-1".into()),
                project_id: Some("proj-1".into()),
                workspace_id: None,
                binding_revision: 1,
            };
            let _ = seam.publish(&env, ctx).await.unwrap();
        }

        // Verify high water is 5 and all events are present
        let desc = store
            .lookup_session_stream("sess-1", "proj-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(desc.high_water_seq, 5);

        let events = store
            .events_after(desc.stream_id.as_str(), 0, 100, 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(events.len(), 5);
        // Verify sequences are 1,2,3,4,5 (no reuse)
        let seqs: Vec<u64> = events.iter().map(|e| e.event_seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    }
}
