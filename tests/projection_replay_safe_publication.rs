mod common;

use std::sync::Arc;

use codegg_core::projection_replay::safe_publication::{
    classify, has_safe_origin, is_persistent, SafePublicationClass,
};
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::CoreEvent;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};

#[test]
fn turn_started_is_safe() {
    let event = CoreEvent::TurnStarted {
        session_id: "s".into(),
        turn_id: "t".into(),
    };
    assert_eq!(classify(&event), SafePublicationClass::Safe);
}

#[test]
fn turn_reasoning_delta_is_internal() {
    let event = CoreEvent::TurnReasoningDelta {
        session_id: "s".into(),
        turn_id: "t".into(),
        delta: "d".into(),
    };
    assert_eq!(classify(&event), SafePublicationClass::Internal);
}

#[test]
fn connection_rotated_is_sensitive() {
    let event = CoreEvent::ConnectionRotated {
        connection_id: "c".into(),
        new_revision: 2,
        catalog_revision: None,
        actor_seam: "test".into(),
    };
    assert_eq!(classify(&event), SafePublicationClass::Sensitive);
}

#[test]
fn safe_events_are_persistent() {
    let events = vec![
        CoreEvent::TurnStarted {
            session_id: "s".into(),
            turn_id: "t".into(),
        },
        CoreEvent::ToolStarted {
            session_id: "s".into(),
            turn_id: None,
            tool_name: "bash".into(),
            tool_id: "t1".into(),
            arguments: "{}".into(),
        },
        CoreEvent::TurnCompleted {
            session_id: "s".into(),
            turn_id: "t".into(),
            stop_reason: "end_turn".into(),
        },
    ];
    for event in &events {
        assert!(is_persistent(classify(event)));
    }
}

#[test]
fn internal_events_are_not_persistent() {
    let event = CoreEvent::TurnReasoningDelta {
        session_id: "s".into(),
        turn_id: "t".into(),
        delta: "d".into(),
    };
    assert!(!is_persistent(classify(&event)));
}

#[test]
fn sensitive_events_are_not_persistent() {
    let event = CoreEvent::ConnectionRotated {
        connection_id: "c".into(),
        new_revision: 2,
        catalog_revision: None,
        actor_seam: "test".into(),
    };
    assert!(!is_persistent(classify(&event)));
}

#[test]
fn safe_origin_detected_for_session_events() {
    let event = CoreEvent::TurnStarted {
        session_id: "s".into(),
        turn_id: "t".into(),
    };
    assert!(has_safe_origin(&event));
}

#[test]
fn safe_origin_not_detected_without_session() {
    let event = CoreEvent::FileChanged {
        path: "f".into(),
        action: "modified".into(),
    };
    assert!(has_safe_origin(&event));
}

#[tokio::test]
async fn no_internal_or_sensitive_in_durable_rows() {
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

    let seq = store.next_event_seq(desc.stream_id.as_str()).await.unwrap();
    store
        .insert_event(desc.stream_id.as_str(), seq, &env)
        .await
        .unwrap();

    let rows = store
        .events_after(desc.stream_id.as_str(), 0, 100, u64::MAX)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].visibility_class, "public");
}

#[test]
fn visibility_class_for_internal_matches() {
    use codegg_core::projection_replay::safe_publication::visibility_for_class;
    use codegg_protocol::projection::dto::VisibilityClass;

    assert_eq!(
        visibility_for_class(SafePublicationClass::Safe),
        VisibilityClass::Public
    );
    assert_eq!(
        visibility_for_class(SafePublicationClass::Internal),
        VisibilityClass::Internal
    );
    assert_eq!(
        visibility_for_class(SafePublicationClass::Sensitive),
        VisibilityClass::Sensitive
    );
}
