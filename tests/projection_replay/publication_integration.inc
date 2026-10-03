use super::common;

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

/// Stored envelopes must carry their stream-local row sequence, not the
/// daemon-global log sequence. The source envelope arrives with the global
/// `event_seq` (41/42 here, simulating session-create and selection events
/// that never project to this stream); cursors, high-water marks, replay
/// ranges, and consumer gap detection all run in stream-local row space.
/// Persisting the global number made every post-snapshot event look like a
/// history gap, so canonical consumers resynced forever and never rendered
/// live turns (M004 desktop session slice).
#[tokio::test]
async fn seam_rewrites_envelope_seq_to_stream_row_seq() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));
    let seam = Arc::new(ProjectionPublicationSeam::new(service.clone()));

    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-seq".into()),
        project_id: Some("proj-seq".into()),
        workspace_id: Some("ws-seq".into()),
        binding_revision: 1,
    };

    for (turn, global_seq) in [("turn-a", 41u64), ("turn-b", 42u64)] {
        let envelope = EventEnvelope {
            protocol_version: PROTOCOL_VERSION,
            event_seq: global_seq,
            timestamp_ms: 1000,
            session_id: Some("sess-seq".into()),
            turn_id: Some(turn.into()),
            payload: CoreEvent::TurnStarted {
                session_id: "sess-seq".into(),
                turn_id: turn.into(),
            },
        };
        let outcome = seam.publish(&envelope, ctx.clone()).await.unwrap();
        assert!(
            matches!(
                outcome,
                codegg_core::projection_replay::service::PublishOutcome::Published { .. }
            ),
            "expected Published, got {outcome:?}"
        );
    }

    let desc = store
        .lookup_session_stream("sess-seq", "proj-seq")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(desc.high_water_seq, 2);

    let rows = store
        .events_after(desc.stream_id.as_str(), 0, 16, 1 << 20)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for (index, row) in rows.iter().enumerate() {
        let row_seq = (index + 1) as u64;
        assert_eq!(row.event_seq, row_seq);
        let envelope: codegg_protocol::projection::event::ProjectionEnvelope =
            serde_json::from_str(&row.payload_json).unwrap();
        assert_eq!(
            envelope.event_seq, row_seq,
            "stored envelope must carry the row sequence, not the global log sequence"
        );
    }
    // Consumer gap check passes from a fresh cursor: the first stored
    // envelope immediately follows sequence zero.
    let first: codegg_protocol::projection::event::ProjectionEnvelope =
        serde_json::from_str(&rows[0].payload_json).unwrap();
    assert_eq!(first.event_seq, 1);
}
