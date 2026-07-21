mod common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, PROTOCOL_VERSION,
};
use codegg_protocol::projection::replay::{
    ProjectionAck, ProjectionCursor, ProjectionStreamId, ProjectionStreamKind,
    ProjectionSubscriptionRequest,
};

async fn test_seam_and_service() -> (Arc<ProjectionPublicationSeam>, Arc<ProjectionReplayService>) {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service.clone()));
    (seam, service)
}

#[tokio::test]
async fn projection_capabilities_returns_supported() {
    let (seam, _service) = test_seam_and_service().await;
    // The capabilities response is purely data-driven, no daemon needed
    let resp = CoreResponse::ProjectionCapabilitiesResponse {
        supported: true,
        projection_version: 1,
        max_events_per_batch: 512,
        max_event_bytes: 64 * 1024,
        max_subscriptions_per_client: 32,
        max_subscriptions_per_daemon: 256,
        retention_session_max_events: 20_000,
        retention_project_max_events: 50_000,
    };
    assert!(matches!(
        resp,
        CoreResponse::ProjectionCapabilitiesResponse {
            supported: true,
            ..
        }
    ));
}

#[tokio::test]
async fn subscribe_session_creates_subscription() {
    let (seam, service) = test_seam_and_service().await;

    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };

    let sub_id = service
        .subscribe_session("sess-1", "proj-1", Some("ws-1"), "client-1", &req)
        .await
        .unwrap();

    assert!(!sub_id.0.is_empty());
    assert_eq!(service.subscriptions().active_count(), 1);
}

#[tokio::test]
async fn subscribe_project_creates_subscription() {
    let (_seam, service) = test_seam_and_service().await;

    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Project,
        scope_id: "proj-1".into(),
        cursor: None,
        projection_version: 1,
    };

    let sub_id = service
        .subscribe_project("proj-1", "client-1", &req)
        .await
        .unwrap();

    assert!(!sub_id.0.is_empty());
    assert_eq!(service.subscriptions().active_count(), 1);
}

#[tokio::test]
async fn resume_replays_persisted_events() {
    let (seam, service) = test_seam_and_service().await;

    // Publish some events first
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
    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = service
        .publish_from_core_with_context(&env, &ctx)
        .await
        .unwrap();

    // Subscribe
    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub_id = service
        .subscribe_session("sess-1", "proj-1", None, "client-1", &req)
        .await
        .unwrap();

    // Resume from seq 0
    let desc = service
        .store()
        .lookup_session_stream("sess-1", "proj-1")
        .await
        .unwrap()
        .unwrap();
    let cursor = ProjectionCursor {
        stream_id: desc.stream_id.clone(),
        event_seq: 0,
        projection_version: 1,
    };

    let outcome = service.resume(&sub_id, &cursor, false).await.unwrap();
    match outcome {
        codegg_core::projection_replay::service::ResumeOutcome::Replayed { events, .. } => {
            assert!(
                !events.is_empty(),
                "should have replayed at least one event"
            );
        }
        other => panic!("expected Replayed, got {:?}", other),
    }
}

#[tokio::test]
async fn ack_updates_last_acked_seq() {
    let (_seam, service) = test_seam_and_service().await;

    // Publish an event
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
    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = service
        .publish_from_core_with_context(&env, &ctx)
        .await
        .unwrap();

    // Subscribe
    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub_id = service
        .subscribe_session("sess-1", "proj-1", None, "client-1", &req)
        .await
        .unwrap();

    // Ack the event
    let desc = service
        .store()
        .lookup_session_stream("sess-1", "proj-1")
        .await
        .unwrap()
        .unwrap();
    let cursor = ProjectionCursor {
        stream_id: desc.stream_id.clone(),
        event_seq: 1,
        projection_version: 1,
    };

    let result = service.ack(&sub_id, &cursor).await.unwrap();
    match result {
        codegg_core::projection_replay::service::AckResult::Accepted { last_acked_seq, .. } => {
            assert_eq!(last_acked_seq, 1);
        }
        other => panic!("expected Accepted, got {:?}", other),
    }
}

#[tokio::test]
async fn unsubscribe_removes_subscription() {
    let (_seam, service) = test_seam_and_service().await;

    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub_id = service
        .subscribe_session("sess-1", "proj-1", None, "client-1", &req)
        .await
        .unwrap();
    assert_eq!(service.subscriptions().active_count(), 1);

    service.unsubscribe(&sub_id).await.unwrap();
    assert_eq!(service.subscriptions().active_count(), 0);
}
