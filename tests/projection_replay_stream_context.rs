mod common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionStreamKind};

#[tokio::test]
async fn session_stream_uses_canonical_non_empty_ids() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-abc".into()),
        turn_id: None,
        payload: CoreEvent::TurnStarted {
            session_id: "sess-abc".into(),
            turn_id: "turn-1".into(),
        },
    };

    let ctx = ProjectionPublicationContext {
        session_id: Some("sess-abc".into()),
        project_id: Some("proj-xyz".into()),
        workspace_id: Some("ws-main".into()),
        binding_revision: 1,
    };

    let _ = seam.publish(&envelope, ctx).await.unwrap();

    let desc = store
        .lookup_session_stream("sess-abc", "proj-xyz")
        .await
        .unwrap()
        .unwrap();
    assert!(!desc.project_id.is_empty());
    assert_eq!(desc.project_id, "proj-xyz");
    assert_eq!(desc.session_id.as_deref(), Some("sess-abc"));
    assert_eq!(desc.workspace_id.as_deref(), Some("ws-main"));
}

#[tokio::test]
async fn project_a_never_receives_project_b_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));

    // Publish event for project A
    let env_a = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-a".into()),
        turn_id: None,
        payload: CoreEvent::TurnStarted {
            session_id: "sess-a".into(),
            turn_id: "turn-a".into(),
        },
    };
    let ctx_a = ProjectionPublicationContext {
        session_id: Some("sess-a".into()),
        project_id: Some("proj-A".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = service
        .publish_from_core_with_context(&env_a, &ctx_a)
        .await
        .unwrap();

    // Publish event for project B
    let env_b = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 2,
        timestamp_ms: 2000,
        session_id: Some("sess-b".into()),
        turn_id: None,
        payload: CoreEvent::TurnStarted {
            session_id: "sess-b".into(),
            turn_id: "turn-b".into(),
        },
    };
    let ctx_b = ProjectionPublicationContext {
        session_id: Some("sess-b".into()),
        project_id: Some("proj-B".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = service
        .publish_from_core_with_context(&env_b, &ctx_b)
        .await
        .unwrap();

    // Verify streams are separate
    let desc_a = store
        .lookup_session_stream("sess-a", "proj-A")
        .await
        .unwrap()
        .unwrap();
    let desc_b = store
        .lookup_session_stream("sess-b", "proj-B")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(desc_a.stream_id, desc_b.stream_id);
    assert_eq!(desc_a.project_id, "proj-A");
    assert_eq!(desc_b.project_id, "proj-B");
}

#[tokio::test]
async fn session_a_never_receives_session_b_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));

    let env = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-a".into()),
        turn_id: None,
        payload: CoreEvent::TurnStarted {
            session_id: "sess-a".into(),
            turn_id: "turn-1".into(),
        },
    };

    let ctx_a = ProjectionPublicationContext {
        session_id: Some("sess-a".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    let _ = service
        .publish_from_core_with_context(&env, &ctx_a)
        .await
        .unwrap();

    let desc_a = store
        .lookup_session_stream("sess-a", "proj-1")
        .await
        .unwrap()
        .unwrap();

    // sess-b should have no stream
    let desc_b = store
        .lookup_session_stream("sess-b", "proj-1")
        .await
        .unwrap();
    assert!(desc_b.is_none());
    // And desc_a should only be for sess-a
    assert_eq!(desc_a.session_id.as_deref(), Some("sess-a"));
}

#[tokio::test]
async fn rebind_invalidates_old_stream_creates_new() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));

    // Create initial stream with revision 1
    let (desc1, created1) = store
        .get_or_create_session_stream_with_revision("sess-1", "proj-A", Some("ws-1"), 1)
        .await
        .unwrap();
    assert!(created1);
    assert_eq!(desc1.project_id, "proj-A");

    // Rebind to proj-B with revision 2
    let (desc2, created2) = store
        .get_or_create_session_stream_with_revision("sess-1", "proj-B", Some("ws-2"), 2)
        .await
        .unwrap();
    assert!(created2);
    assert_eq!(desc2.project_id, "proj-B");
    assert_ne!(desc1.stream_id, desc2.stream_id);

    // The old stream should be rebound, not active
    let old_active = store
        .lookup_session_stream("sess-1", "proj-A")
        .await
        .unwrap();
    assert!(old_active.is_none(), "old stream should be rebound");

    // The new stream should be active
    let new_active = store
        .lookup_session_stream("sess-1", "proj-B")
        .await
        .unwrap();
    assert!(new_active.is_some());
    assert_eq!(new_active.unwrap().stream_id, desc2.stream_id);
}

#[tokio::test]
async fn same_binding_revision_is_idempotent() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));

    let (desc1, created1) = store
        .get_or_create_session_stream_with_revision("sess-1", "proj-A", None, 1)
        .await
        .unwrap();
    assert!(created1);

    let (desc2, created2) = store
        .get_or_create_session_stream_with_revision("sess-1", "proj-A", None, 1)
        .await
        .unwrap();
    assert!(!created2);
    assert_eq!(desc1.stream_id, desc2.stream_id);
}

#[tokio::test]
async fn concurrent_rebind_and_publish_resolves_consistently() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));

    // Create initial stream
    let _ = store
        .get_or_create_session_stream_with_revision("sess-1", "proj-A", None, 1)
        .await
        .unwrap();

    // Concurrently publish and rebind
    let svc1 = service.clone();
    let svc2 = service.clone();
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

    let ctx_b = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-B".into()),
        workspace_id: None,
        binding_revision: 2,
    };

    let (r1, r2) = tokio::join!(
        svc1.publish_from_core_with_context(&env, &ctx_a),
        svc2.publish_from_core_with_context(&env, &ctx_b),
    );

    // Both should succeed (one publishes to old stream, one to new)
    assert!(r1.is_ok());
    assert!(r2.is_ok());

    // After rebind, only proj-B should have an active stream
    let active_a = store
        .lookup_session_stream("sess-1", "proj-A")
        .await
        .unwrap();
    let active_b = store
        .lookup_session_stream("sess-1", "proj-B")
        .await
        .unwrap();
    assert!(active_a.is_none(), "proj-A stream should be rebound");
    assert!(active_b.is_some(), "proj-B stream should be active");
}
