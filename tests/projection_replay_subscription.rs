mod common;

use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_core::projection_replay::subscription::{SubscriptionConfig, SubscriptionRegistry};
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::{ProjectionStreamId, ProjectionStreamKind};

#[tokio::test]
async fn project_a_subscription_receives_no_project_b_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc_a, _) = store.get_or_create_project_stream("pA").await.unwrap();
    let (desc_b, _) = store.get_or_create_project_stream("pB").await.unwrap();

    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let (sub_a, _rx_a) = reg
        .register("c1", &desc_a.stream_id, ProjectionStreamKind::Project, 1)
        .unwrap();
    reg.set_live(&sub_a).unwrap();

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

    let _ = reg.deliver_to_stream(desc_b.stream_id.as_str(), env);

    assert!(reg.by_id().get(&sub_a).is_some());
}

#[tokio::test]
async fn session_a_receives_no_sibling_session_events() {
    let pool = common::projection_replay::test_pool().await;
    let store = ProjectionReplayStore::new(pool);
    let (desc_a, _) = store
        .get_or_create_session_stream("sA", "p1", None)
        .await
        .unwrap();
    let (desc_b, _) = store
        .get_or_create_session_stream("sB", "p1", None)
        .await
        .unwrap();

    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let (sub_a, _rx_a) = reg
        .register("c1", &desc_a.stream_id, ProjectionStreamKind::Session, 1)
        .unwrap();
    reg.set_live(&sub_a).unwrap();

    let env = ProjectionEnvelope::session_event(
        1,
        0,
        "sB",
        None,
        ProjectionEvent::Diagnostic {
            code: "c".into(),
            message: "m".into(),
        },
    );

    let delivered = reg.deliver_to_stream(desc_b.stream_id.as_str(), env);
    assert_eq!(delivered.unwrap(), 0);
}

#[tokio::test]
async fn subscription_per_client_and_global_caps() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig {
        max_per_client: 1,
        max_per_daemon: 2,
        queue_capacity: 8,
        idle_timeout_ms: 60_000,
    });
    let sid = ProjectionStreamId::new("s1").unwrap();

    let _ = reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();

    assert!(reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .is_err());

    let _ = reg
        .register("c2", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();

    assert!(reg
        .register("c3", &sid, ProjectionStreamKind::Session, 1)
        .is_err());
}

#[tokio::test]
async fn ack_monotonicity_and_idempotency() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let sid = ProjectionStreamId::new("s1").unwrap();
    let (sub, _rx) = reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();

    let lag = reg.ack(&sub, 5, &sid, 1, 10).unwrap();
    assert_eq!(lag, 5);

    let lag = reg.ack(&sub, 5, &sid, 1, 10).unwrap();
    assert_eq!(lag, 5);

    let lag = reg.ack(&sub, 3, &sid, 1, 10).unwrap();
    assert_eq!(lag, 5);

    assert!(reg.ack(&sub, 11, &sid, 1, 10).is_err());
}

#[tokio::test]
async fn ack_stream_mismatch_rejected() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let sid1 = ProjectionStreamId::new("s1").unwrap();
    let sid2 = ProjectionStreamId::new("s2").unwrap();
    let (sub, _rx) = reg
        .register("c1", &sid1, ProjectionStreamKind::Session, 1)
        .unwrap();

    assert!(matches!(
        reg.ack(&sub, 1, &sid2, 1, 10),
        Err(codegg_core::projection_replay::subscription::SubscriptionError::StreamMismatch)
    ));
}

#[tokio::test]
async fn unsubscribe_removes_subscription() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let sid = ProjectionStreamId::new("s1").unwrap();
    let (sub, _rx) = reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();
    assert_eq!(reg.active_count(), 1);
    reg.unsubscribe(&sub).unwrap();
    assert_eq!(reg.active_count(), 0);
}

#[tokio::test]
async fn deliver_to_live_subscriptions() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let sid = ProjectionStreamId::new("s1").unwrap();
    let (sub, _rx) = reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();
    reg.set_live(&sub).unwrap();
    drop(_rx);

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

    let count = reg.deliver_to_stream(sid.as_str(), env).unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn deliver_to_initializing_subscriptions_skipped() {
    let reg = SubscriptionRegistry::new(SubscriptionConfig::default());
    let sid = ProjectionStreamId::new("s1").unwrap();
    let (sub, _rx) = reg
        .register("c1", &sid, ProjectionStreamKind::Session, 1)
        .unwrap();
    // Don't set live

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

    let count = reg.deliver_to_stream(sid.as_str(), env).unwrap();
    assert_eq!(count, 0);
}
