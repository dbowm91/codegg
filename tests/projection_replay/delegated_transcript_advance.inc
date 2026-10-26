//! M002 — the body-free transcript-advanced advisory on the projection stream.
//!
//! The advisory is the *signal*, not the transcript. It says a delegated
//! run's committed recording advanced, and it says so without carrying any
//! of the recording: no text, no tool arguments, no reasoning, no
//! credentials. A reader that wants the conversation asks for the
//! authorized page.
//!
//! These tests pin the three properties that make the signal safe to
//! broadcast:
//!
//! - it is **body-free** on the wire;
//! - it reaches the projection stream **with no TUI attached**, so the
//!   producer is not tied to an open tab;
//! - the reducer turns it into a **bounded watermark** and nothing else —
//!   it never invents a transcript entry.

use super::common;

use std::sync::Arc;

use codegg_core::projection_replay::seam::{
    ProjectionPublicationContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::ProjectionReplayService;
use codegg_core::projection_replay::store::ProjectionReplayStore;
use codegg_protocol::core::{CoreEvent, EventEnvelope, PROTOCOL_VERSION};
use codegg_protocol::projection::event::ProjectionEvent;
use codegg_protocol::projection::reducer::{ApplyOutcome, ProjectionReducer, ReducerEventInput};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

/// The advisory's core event, as the daemon publishes it.
fn advisory(run_id: &str, ordinal: i64, recording_stopped: bool) -> CoreEvent {
    CoreEvent::AgentRunTranscriptAdvanced {
        session_id: "sess-1".into(),
        run_id: run_id.into(),
        ordinal,
        recording_stopped,
    }
}

fn envelope(payload: CoreEvent) -> EventEnvelope<CoreEvent> {
    EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 7,
        timestamp_ms: 1_000,
        session_id: Some("sess-1".into()),
        turn_id: None,
        payload,
    }
}

fn context() -> ProjectionPublicationContext {
    ProjectionPublicationContext {
        session_id: Some("sess-1".into()),
        project_id: Some("proj-1".into()),
        workspace_id: Some("ws-1".into()),
        binding_revision: 1,
    }
}

#[tokio::test]
async fn the_advisory_reaches_the_projection_stream_with_no_tui_attached() {
    let pool = common::projection_replay::test_pool().await;
    let store = Arc::new(ProjectionReplayStore::new(pool));
    let service = Arc::new(ProjectionReplayService::new(store.clone()));
    let seam = Arc::new(ProjectionPublicationSeam::new(service));

    // Nothing is attached: no TUI, no subscriber, no client. The producer
    // is the daemon's event log, not an open tab.
    let outcome = seam
        .publish(&envelope(advisory("run-1", 12, false)), context())
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            codegg_core::projection_replay::service::PublishOutcome::Published { .. }
        ),
        "the advisory must publish with no TUI attached, got {outcome:?}"
    );

    let descriptor = store
        .lookup_session_stream("sess-1", "proj-1")
        .await
        .unwrap()
        .expect("the advisory created the session's projection stream");
    let rows = store
        .events_after(descriptor.stream_id.as_str(), 0, 16, 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the advisory is one projection event, got {rows:?}"
    );
    assert!(
        rows[0]
            .payload_json
            .contains("agent_run_transcript_advanced"),
        "the stream carries the advisory, got {}",
        rows[0].payload_json
    );
}

#[tokio::test]
async fn the_advisory_carries_no_transcript_body() {
    let event = ProjectionEvent::AgentRunTranscriptAdvanced {
        run_id: "run-1".into(),
        ordinal: 12,
        recording_stopped: false,
        at: 1_000,
    };
    let encoded = serde_json::to_string(&event).expect("the advisory is encodable");
    for forbidden in ["body", "entries", "redacted_body", "text", "arguments"] {
        assert!(
            !encoded.contains(forbidden),
            "the advisory must not carry a {forbidden} field, got {encoded}"
        );
    }
    assert!(encoded.contains("run-1"));
    assert!(encoded.contains("12"));
}

#[tokio::test]
async fn the_reducer_records_a_watermark_and_invents_no_transcript() {
    let reducer = ProjectionReducer::default();
    let mut snapshot = SessionProjectionSnapshot::empty("sess-1", "proj-1", "ws-1");

    let outcome = reducer.apply(
        &mut snapshot,
        ReducerEventInput::session(
            1,
            1_000,
            "sess-1",
            None,
            ProjectionEvent::AgentRunTranscriptAdvanced {
                run_id: "run-1".into(),
                ordinal: 12,
                recording_stopped: false,
                at: 1_000,
            },
        ),
    );
    assert_eq!(outcome, ApplyOutcome::Applied);
    let watermark = snapshot
        .transcript_watermark("run-1")
        .expect("the advisory records a watermark");
    assert_eq!(watermark.ordinal, 12);
    assert!(!watermark.recording_stopped);
    assert!(
        snapshot.agent_runs.is_empty(),
        "the advisory must not invent a durable run summary"
    );
    assert!(
        snapshot.active_turn.is_none(),
        "the advisory must not invent a turn or a transcript entry"
    );
}

#[tokio::test]
async fn a_duplicate_advisory_is_idempotent() {
    let reducer = ProjectionReducer::default();
    let mut snapshot = SessionProjectionSnapshot::empty("sess-1", "proj-1", "ws-1");

    for seq in 1..=3u64 {
        let outcome = reducer.apply(
            &mut snapshot,
            ReducerEventInput::session(
                seq,
                1_000,
                "sess-1",
                None,
                ProjectionEvent::AgentRunTranscriptAdvanced {
                    run_id: "run-1".into(),
                    ordinal: 12,
                    recording_stopped: false,
                    at: 1_000,
                },
            ),
        );
        assert_eq!(outcome, ApplyOutcome::Applied);
    }
    let watermark = snapshot
        .transcript_watermark("run-1")
        .expect("the advisory records a watermark");
    assert_eq!(
        watermark.ordinal, 12,
        "a duplicate advisory does not move the watermark"
    );
    assert_eq!(
        snapshot.transcript_watermarks.len(),
        1,
        "a duplicate advisory does not add a second watermark"
    );
}
