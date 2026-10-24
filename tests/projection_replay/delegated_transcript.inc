//! C003 — the delegated run's recorded transcript crosses the projection.
//!
//! The delegated tab reads the child's conversation from the durable
//! transcript store (C002) and the projection carries it (C003). These
//! tests pin the three facts the tab depends on:
//!
//! 1. the transcript arrives **in order**, with the run's own rows;
//! 2. a long run's payload is **bounded** and says where it stops;
//! 3. a client bound to **another session** does not receive it.
//!
//! The absent case matters as much as the present one: "no transcript was
//! recorded" and "here is the whole conversation" are different claims, and
//! a tab that cannot tell them apart renders an empty pane for a run that
//! simply has nothing recorded.

use super::common;

use std::sync::Arc;

use codegg_core::agent_run_transcript::{
    AgentRunTranscriptStore, InMemoryAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
};
use codegg_core::identity::AgentRunId;
use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use codegg_protocol::projection::dto::{
    AgentRunSummary, AgentRunTranscriptEntryProjection, AgentRunTranscriptKind,
    AgentRunTranscriptProjection, AgentRunTranscriptRole, AgentRunTranscriptTruncation,
};
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::replay::{ProjectionStreamKind, ProjectionSubscriptionRequest};

async fn setup_service() -> (Arc<ProjectionPublicationSeam>, Arc<ProjectionReplayService>) {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store));
    let seam = Arc::new(ProjectionPublicationSeam::new(service.clone()));
    (seam, service)
}

fn summary_with_transcript(
    run_id: &AgentRunId,
    transcript: Option<AgentRunTranscriptProjection>,
) -> AgentRunSummary {
    AgentRunSummary {
        run_id: run_id.to_string(),
        task_id: "task-1".into(),
        project_id: "proj-1".into(),
        workspace_id: "ws-1".into(),
        agent: "explore".into(),
        status: "completed".into(),
        depth: 1,
        control_status: "none".into(),
        updated_at: 10,
        transcript,
        ..Default::default()
    }
}

fn entry(ordinal: i64, body: &str) -> AgentRunTranscriptEntryProjection {
    AgentRunTranscriptEntryProjection {
        ordinal,
        role: AgentRunTranscriptRole::Orchestrator,
        kind: AgentRunTranscriptKind::ModelMessage,
        body: body.to_string(),
        byte_len: body.len() as i64,
        created_at: 1_700_000_000_000 + ordinal,
    }
}

/// A client bound to `session_id`, subscribed to that session's stream.
async fn subscribe(
    service: &ProjectionReplayService,
    session_id: &str,
    client_id: &str,
) -> tokio::sync::mpsc::Receiver<ProjectionEnvelope> {
    let request = ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Session,
        scope_id: session_id.to_string(),
        cursor: None,
        projection_version: 1,
    };
    let sub_id = service
        .subscribe_session(session_id, "proj-1", None, client_id, &request)
        .await
        .expect("subscribe to the session stream");
    service
        .take_subscription_receiver(&sub_id)
        .await
        .expect("take the subscription receiver")
}

#[tokio::test]
async fn the_projection_carries_the_transcript_in_order() {
    let (seam, service) = setup_service().await;
    let mut rx = subscribe(&service, "sess-1", "client-1").await;

    let run_id = AgentRunId::new();
    let transcript = AgentRunTranscriptProjection::checked(
        vec![entry(0, "first"), entry(1, "second"), entry(2, "third")],
        None,
    )
    .expect("an in-bound payload");
    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-1".into()),
        turn_id: None,
        payload: CoreEvent::AgentRunUpserted {
            session_id: "sess-1".into(),
            run: summary_with_transcript(&run_id, Some(transcript)),
        },
    };
    let context = ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    seam.publish(&envelope, context)
        .await
        .expect("publish the run upsert");

    let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("the owning client receives the run upsert")
        .expect("the envelope is delivered");
    match received.payload {
        ProjectionEvent::AgentRunUpserted { run } => {
            let transcript = run.transcript.expect("the transcript crossed");
            let bodies = transcript
                .entries()
                .iter()
                .map(|entry| entry.body.as_str())
                .collect::<Vec<_>>();
            assert_eq!(bodies, vec!["first", "second", "third"]);
            let ordinals = transcript
                .entries()
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>();
            assert_eq!(ordinals, vec![0, 1, 2], "the rows keep their order");
        }
        other => panic!("expected a run upsert, got {other:?}"),
    }
}

#[tokio::test]
async fn a_client_bound_to_another_session_does_not_receive_the_transcript() {
    let (seam, service) = setup_service().await;
    let mut owner_rx = subscribe(&service, "sess-owner", "client-owner").await;
    let mut other_rx = subscribe(&service, "sess-other", "client-other").await;

    let run_id = AgentRunId::new();
    let transcript = AgentRunTranscriptProjection::checked(
        vec![entry(0, "the owner's private conversation")],
        None,
    )
    .expect("an in-bound payload");
    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("sess-owner".into()),
        turn_id: None,
        payload: CoreEvent::AgentRunUpserted {
            session_id: "sess-owner".into(),
            run: summary_with_transcript(&run_id, Some(transcript)),
        },
    };
    let context = ProjectionPublicationContext {
        session_id: Some("sess-owner".into()),
        project_id: Some("proj-1".into()),
        workspace_id: None,
        binding_revision: 1,
    };
    seam.publish(&envelope, context)
        .await
        .expect("publish the run upsert");

    let received = tokio::time::timeout(std::time::Duration::from_secs(2), owner_rx.recv())
        .await
        .expect("the owning client receives the run upsert")
        .expect("the envelope is delivered");
    assert!(
        matches!(received.payload, ProjectionEvent::AgentRunUpserted { .. }),
        "the owning client receives the run upsert"
    );

    // The negative case: the client bound to another session receives
    // nothing at all — not the run, and therefore not its transcript.
    let leaked = tokio::time::timeout(std::time::Duration::from_millis(200), other_rx.recv()).await;
    assert!(
        leaked.is_err() || leaked.unwrap().is_none(),
        "a client bound to another session must not receive the transcript"
    );
}

#[tokio::test]
async fn a_run_with_no_transcript_yields_the_absent_variant() {
    let store = InMemoryAgentRunTranscriptStore::new();
    let run_id = AgentRunId::new();
    let read = store
        .read(&run_id, 0, 64)
        .await
        .expect("read a run with no transcript");
    match read {
        codegg_core::agent_run_transcript::AgentRunTranscriptRead::Absent => {}
        other => panic!("expected an absent transcript, got {other:?}"),
    }

    // The projection's absent variant is the wire form of that fact, and it
    // is distinct from an empty conversation.
    let absent = AgentRunTranscriptProjection::Absent;
    let empty = AgentRunTranscriptProjection::checked(Vec::new(), None).expect("an empty payload");
    assert_ne!(absent, empty);
    assert!(absent.entries().is_empty());
    assert!(empty.entries().is_empty());
}

#[tokio::test]
async fn a_long_runs_payload_is_bounded_and_states_its_truncation_point() {
    let store = InMemoryAgentRunTranscriptStore::new();
    let run_id = AgentRunId::new();
    let total = AgentRunTranscriptProjection::MAX_ROWS + 20;
    for index in 0..total {
        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                &format!("row {index}"),
            )
            .await
            .expect("append a row");
    }

    let summary = codegg_core::projection_replay::agent_run_summary_with_transcript(
        AgentRunSummary {
            transcript: None,
            run_id: run_id.to_string(),
            task_id: "task-1".into(),
            agent: "explore".into(),
            status: "completed".into(),
            ..Default::default()
        },
        &store,
        &run_id,
    )
    .await;
    let transcript = summary.transcript.expect("a transcript is attached");
    assert!(
        transcript.body_bytes() <= AgentRunTranscriptProjection::MAX_BYTES,
        "the payload must stay inside the declared bound"
    );
    match transcript {
        AgentRunTranscriptProjection::Truncated {
            entries,
            truncation,
        } => {
            assert!(entries.len() <= AgentRunTranscriptProjection::MAX_ROWS);
            assert_eq!(truncation.omitted_entries, 20);
            assert_eq!(
                truncation.next_ordinal,
                entries.last().expect("a carried row").ordinal + 1
            );
            assert!(!truncation.recording_stopped);
        }
        other => panic!("expected a truncated transcript, got {other:?}"),
    }
}

#[tokio::test]
async fn an_older_client_ignores_the_transcript_field() {
    // The client that does not know the field: the summary shape without it.
    #[derive(serde::Deserialize)]
    struct OlderClientSummary {
        run_id: String,
        agent: String,
    }

    let run_id = AgentRunId::new();
    let transcript = AgentRunTranscriptProjection::checked(vec![entry(0, "hello")], None)
        .expect("an in-bound payload");
    let encoded = serde_json::to_value(summary_with_transcript(&run_id, Some(transcript)))
        .expect("encode the summary");
    let decoded: OlderClientSummary =
        serde_json::from_value(encoded).expect("the older client ignores the unknown field");
    assert_eq!(decoded.run_id, run_id.to_string());
    assert_eq!(decoded.agent, "explore");
}

#[tokio::test]
async fn a_transcript_payload_over_the_declared_bound_is_refused() {
    let over = vec![entry(
        0,
        &"x".repeat(AgentRunTranscriptProjection::MAX_BYTES + 1),
    )];
    let refused = AgentRunTranscriptProjection::checked(over, None)
        .expect_err("an over-bound payload must be refused");
    assert_eq!(refused.max_bytes, AgentRunTranscriptProjection::MAX_BYTES);
    assert!(refused.bytes > refused.max_bytes);

    // A refused payload is never a silently shortened one: the two states
    // are different claims.
    let shortened = AgentRunTranscriptProjection::checked(
        vec![entry(
            0,
            &"x".repeat(AgentRunTranscriptProjection::MAX_BYTES),
        )],
        Some(AgentRunTranscriptTruncation {
            next_ordinal: 1,
            omitted_entries: 1,
            recording_stopped: false,
        }),
    )
    .expect("an in-bound payload");
    assert!(matches!(
        shortened,
        AgentRunTranscriptProjection::Truncated { .. }
    ));
}
