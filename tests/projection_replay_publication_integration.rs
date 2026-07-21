mod common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use codegg_protocol::projection::replay::ProjectionStreamKind;

#[tokio::test]
async fn seam_publishes_turn_started_to_projection_storage() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));
    let seam = Arc::new(ProjectionPublicationSeam::new(service.clone()));

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-1".into()),
        payload: CoreEvent::TurnStarted {
            session_id: "sess-1".into(),
            turn_id: "turn-1".into(),
        },
    };

    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    };

    let outcome = seam.publish(&envelope, ctx).await.unwrap();
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Published { .. }
    ));

    // Verify the event was persisted
    let session_desc = store
        .lookup_session_stream("sess-1", "proj-1")
        .await
        .unwrap();
    assert!(session_desc.is_some());
    let desc = session_desc.unwrap();
    assert!(desc.high_water_seq > 0);
}

#[tokio::test]
async fn seam_skips_events_without_project_id() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let envelope = EventEnvelope {
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

    // Empty project_id -> Skipped
    let ctx = ProjectionPublicationContext::default();
    let outcome = seam.publish(&envelope, ctx).await.unwrap();
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Skipped { .. }
    ));
}

#[tokio::test]
async fn seam_skips_internal_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: None,
        payload: CoreEvent::TurnReasoningDelta {
            session_id: "sess-1".into(),
            turn_id: "turn-1".into(),
            delta: "reasoning".into(),
        },
    };

    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        ..Default::default()
    };

    let outcome = seam.publish(&envelope, ctx).await.unwrap();
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Skipped { .. }
    ));
}

#[tokio::test]
async fn seam_uses_real_stream_ids_not_synthetic() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let envelope = EventEnvelope {
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

    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    };

    let outcome = seam.publish(&envelope, ctx).await.unwrap();
    match outcome {
        codegg_core::projection_replay::service::PublishOutcome::Published {
            session_stream_seq,
            project_stream_seq,
        } => {
            // Verify the session stream exists and uses a UUID, not a synthetic ID
            let desc = store
                .lookup_session_stream("sess-1", "proj-1")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(desc.kind, ProjectionStreamKind::Session);
            assert_eq!(desc.project_id, "proj-1");
            assert_eq!(desc.session_id.as_deref(), Some("sess-1"));
            // The stream_id should be a UUID, not "session-stream"
            assert_ne!(desc.stream_id.as_str(), "session-stream");
            assert!(session_stream_seq > 0);
            assert!(project_stream_seq > 0);
        }
        other => panic!("expected Published, got {:?}", other),
    }
}
