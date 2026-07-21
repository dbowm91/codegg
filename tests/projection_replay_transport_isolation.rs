mod common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use codegg_protocol::projection::replay::ProjectionSubscriptionRequest;

async fn setup_service() -> (Arc<ProjectionPublicationSeam>, Arc<ProjectionReplayService>) {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service.clone()));
    (seam, service)
}

#[tokio::test]
async fn two_clients_different_streams_receive_only_own_events() {
    let (seam, service) = setup_service().await;

    // Client 1 subscribes to session "sess-1"
    let req1 = ProjectionSubscriptionRequest {
        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub1 = service
        .subscribe_session("sess-1", "proj-1", None, "client-1", &req1)
        .await
        .unwrap();
    let mut rx1 = service.take_subscription_receiver(&sub1).await.unwrap();

    // Client 2 subscribes to session "sess-2"
    let req2 = ProjectionSubscriptionRequest {
        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Session,
        scope_id: "sess-2".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub2 = service
        .subscribe_session("sess-2", "proj-2", None, "client-2", &req2)
        .await
        .unwrap();
    let mut rx2 = service.take_subscription_receiver(&sub2).await.unwrap();

    // Publish event to sess-1
    let env1 = EventEnvelope {
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
    let ctx1 = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = seam.publish(&env1, ctx1).await.unwrap();

    // Client 1 should receive the event
    let received1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx1.recv()).await;
    assert!(received1.is_ok(), "client 1 should receive an event");
    assert!(received1.unwrap().is_some(), "event should not be None");

    // Client 2 should NOT receive the event (timeout)
    let received2 = tokio::time::timeout(std::time::Duration::from_millis(200), rx2.recv()).await;
    assert!(
        received2.is_err() || received2.unwrap().is_none(),
        "client 2 should NOT receive sess-1 events"
    );
}

#[tokio::test]
async fn unsubscribe_cleans_up_forwarder() {
    let (_seam, service) = setup_service().await;

    let req = ProjectionSubscriptionRequest {
        scope: codegg_protocol::projection::replay::ProjectionStreamKind::Session,
        scope_id: "sess-1".into(),
        cursor: None,
        projection_version: 1,
    };
    let sub_id = service
        .subscribe_session("sess-1", "proj-1", None, "client-1", &req)
        .await
        .unwrap();

    // Take the receiver (simulating what transport does)
    let rx = service.take_subscription_receiver(&sub_id).await;
    assert!(rx.is_some(), "receiver should be available");

    // Unsubscribe
    service.unsubscribe(&sub_id).await.unwrap();
    assert_eq!(service.subscriptions().active_count(), 0);

    // Trying to take receiver again should return None
    let rx2 = service.take_subscription_receiver(&sub_id).await;
    assert!(
        rx2.is_none(),
        "receiver should not be available after unsubscribe"
    );
}
