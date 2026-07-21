mod common;

use std::sync::Arc;

use codegg_core::projection_replay::service::{ProjectionReplayService, ResumeOutcome};
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::{
    ProjectionCursor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionRequest,
};

async fn setup_service_with_events(count: u64) -> ProjectionReplayService {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = ProjectionReplayService::new(store.clone());

    let (desc, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    for i in 0..count {
        let env = ProjectionEnvelope::session_event(
            i,
            i as i64 * 1000,
            "s1",
            None,
            ProjectionEvent::Diagnostic {
                code: format!("c{i}"),
                message: format!("m{i}"),
            },
        );
        let seq = store.next_event_seq(desc.stream_id.as_str()).await.unwrap();
        store
            .insert_event(desc.stream_id.as_str(), seq, &env)
            .await
            .unwrap();
        store
            .update_high_water(desc.stream_id.as_str(), seq)
            .await
            .unwrap();
    }

    let req = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: "s1".into(),
        cursor: None,
        projection_version: 1,
    };

    let sub_id = service
        .subscribe_session("s1", "p1", None, "c1", &req)
        .await
        .unwrap();

    let _ = sub_id;

    service
}

#[tokio::test]
async fn resume_from_zero_returns_all_events() {
    let service = setup_service_with_events(5).await;

    let sub_id = {
        let subs = service.subscriptions();
        let entries: Vec<_> = subs.by_id().iter().map(|e| e.key().clone()).collect();
        entries.into_iter().next().unwrap()
    };

    let store = service.store();
    let stream_id = {
        let subs = service.subscriptions();
        let entries: Vec<_> = subs.by_id().iter().map(|e| e.value().stream_id.clone()).collect();
        entries.into_iter().next().unwrap()
    };
    let _ = store.lookup_stream_by_id(stream_id.as_str()).await.unwrap();

    let cursor = ProjectionCursor {
        stream_id,
        event_seq: 0,
        projection_version: 1,
    };

    let outcome = service
        .resume(&sub_id, &cursor, false)
        .await
        .unwrap();

    match outcome {
        ResumeOutcome::Replayed {
            events,
            current_high_water,
            ..
        } => {
            assert!(!events.is_empty());
            assert!(current_high_water > 0);
        }
        other => panic!("expected Replayed, got {:?}", other),
    }
}

#[tokio::test]
async fn resume_at_high_water_returns_empty() {
    let service = setup_service_with_events(5).await;

    let store = service.store();
    let desc = store
        .lookup_stream_by_id("stream-not-used")
        .await
        .unwrap();

    let sid_str = "session-stream";
    let (desc_real, _) = store
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    let sub_id = {
        let subs = service.subscriptions();
        let entries: Vec<_> = subs.by_id().iter().map(|e| e.key().clone()).collect();
        entries.into_iter().next().unwrap()
    };

    let cursor = ProjectionCursor {
        stream_id: desc_real.stream_id.clone(),
        event_seq: desc_real.high_water_seq,
        projection_version: 1,
    };

    let outcome = service
        .resume(&sub_id, &cursor, false)
        .await
        .unwrap();

    match outcome {
        ResumeOutcome::Empty {
            current_high_water, ..
        } => {
            assert_eq!(current_high_water, desc_real.high_water_seq);
        }
        other => panic!("expected Empty, got {:?}", other),
    }
}

#[tokio::test]
async fn cursor_ahead_returns_resync() {
    let service = setup_service_with_events(5).await;

    let (desc, _) = service
        .store()
        .get_or_create_session_stream("s1", "p1", None)
        .await
        .unwrap();

    let sub_id = {
        let subs = service.subscriptions();
        let entries: Vec<_> = subs.by_id().iter().map(|e| e.key().clone()).collect();
        entries.into_iter().next().unwrap()
    };

    let cursor = ProjectionCursor {
        stream_id: desc.stream_id.clone(),
        event_seq: 1000,
        projection_version: 1,
    };

    let outcome = service
        .resume(&sub_id, &cursor, false)
        .await
        .unwrap();

    matches!(outcome, ResumeOutcome::Resync { .. });
}

#[tokio::test]
async fn stream_mismatch_returns_resync() {
    let service = setup_service_with_events(5).await;

    let sub_id = {
        let subs = service.subscriptions();
        let entries: Vec<_> = subs.by_id().iter().map(|e| e.key().clone()).collect();
        entries.into_iter().next().unwrap()
    };

    let cursor = ProjectionCursor {
        stream_id: ProjectionStreamId("nonexistent-stream".into()),
        event_seq: 0,
        projection_version: 1,
    };

    let outcome = service
        .resume(&sub_id, &cursor, false)
        .await
        .unwrap();

    match outcome {
        ResumeOutcome::Resync { reason, .. } => {
            assert_eq!(reason, codegg_protocol::projection::replay::ProjectionResyncReason::StreamMismatch);
        }
        other => panic!("expected Resync(StreamMismatch), got {:?}", other),
    }
}
