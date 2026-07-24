//! Integration tests for tool program projection events.
//!
//! Tests that the new `ToolProgramSubmitted` and `ToolProgramTerminal`
//! projection events are correctly created, serialized, and reduce
//! into snapshot state.

use codegg_protocol::core::{CoreEvent, EventEnvelope};
use codegg_protocol::projection::adapters::projection_events_from_core;
use codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION;
use codegg_protocol::projection::event::ProjectionEvent;
use codegg_protocol::projection::reducer::{ProjectionReducer, ReducerEventInput};
use codegg_protocol::projection::snapshot::SessionProjectionSnapshot;

fn make_envelope(seq: u64, payload: CoreEvent) -> EventEnvelope<CoreEvent> {
    EventEnvelope {
        protocol_version: 2,
        event_seq: seq,
        timestamp_ms: 1000,
        session_id: Some("s1".into()),
        turn_id: None,
        payload,
    }
}

#[test]
fn tool_program_completed_maps_to_terminal_projection() {
    let env = make_envelope(
        1,
        CoreEvent::ToolProgramCompleted {
            session_id: Some("s1".into()),
            program_id: "tp-1".into(),
            job_id: "j-1".into(),
            status: "completed".into(),
            summary: "ok".into(),
            calls_completed: 3,
        },
    );

    let events = projection_events_from_core(&env);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ProjectionEvent::ToolProgramTerminal {
            program_id,
            job_id,
            status,
            summary,
            ..
        } => {
            assert_eq!(program_id, "tp-1");
            assert_eq!(job_id, "j-1");
            assert_eq!(status, "completed");
            assert_eq!(summary, "ok");
        }
        other => panic!("expected ToolProgramTerminal, got {:?}", other),
    }
}

#[test]
fn tool_program_failed_maps_to_terminal_projection() {
    let env = make_envelope(
        1,
        CoreEvent::ToolProgramFailed {
            session_id: Some("s1".into()),
            program_id: "tp-2".into(),
            job_id: "j-2".into(),
            status: "failed".into(),
            error: "timeout exceeded".into(),
            failure_class: Some("timeout".into()),
        },
    );

    let events = projection_events_from_core(&env);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ProjectionEvent::ToolProgramTerminal {
            program_id,
            job_id,
            status,
            summary,
            ..
        } => {
            assert_eq!(program_id, "tp-2");
            assert_eq!(job_id, "j-2");
            assert_eq!(status, "failed");
            assert_eq!(summary, "timeout exceeded");
        }
        other => panic!("expected ToolProgramTerminal, got {:?}", other),
    }
}

#[test]
fn projection_event_is_not_turn_scoped() {
    let event = ProjectionEvent::ToolProgramSubmitted {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        submitted_at: 1000,
    };
    assert!(!event.is_turn_scoped("t1"));

    let event = ProjectionEvent::ToolProgramTerminal {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        status: "completed".into(),
        summary: "ok".into(),
        completed_at: 2000,
    };
    assert!(!event.is_turn_scoped("t1"));
}

#[test]
fn projection_event_serialization_roundtrip() {
    let event = ProjectionEvent::ToolProgramTerminal {
        program_id: "tp-1".into(),
        job_id: "j-1".into(),
        status: "completed".into(),
        summary: "ok".into(),
        completed_at: 2000,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["kind"], "tool_program_terminal");
    assert_eq!(json["program_id"], "tp-1");

    let back: ProjectionEvent = serde_json::from_value(json).unwrap();
    match back {
        ProjectionEvent::ToolProgramTerminal { program_id, .. } => {
            assert_eq!(program_id, "tp-1");
        }
        other => panic!("expected ToolProgramTerminal, got {:?}", other),
    }
}

#[test]
fn reducer_applies_tool_program_terminal() {
    let mut snapshot = SessionProjectionSnapshot::empty("s1", "p1", "w1");
    let reducer = ProjectionReducer::default();

    let input = ReducerEventInput {
        protocol_version: PROJECTION_PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("s1".into()),
        turn_id: None,
        payload: ProjectionEvent::ToolProgramTerminal {
            program_id: "tp-1".into(),
            job_id: "j-1".into(),
            status: "completed".into(),
            summary: "ok".into(),
            completed_at: 1000,
        },
    };

    let outcome = reducer.apply(&mut snapshot, input);
    assert!(matches!(
        outcome,
        codegg_protocol::projection::reducer::ApplyOutcome::Applied
    ));
    // The reducer should have recorded a diagnostic for the terminal event
    assert!(!snapshot.diagnostics.is_empty());
    let diag = &snapshot.diagnostics[0];
    assert_eq!(diag.code, "tool_program_terminal");
}

#[test]
fn reducer_applies_tool_program_submitted() {
    let mut snapshot = SessionProjectionSnapshot::empty("s1", "p1", "w1");
    let reducer = ProjectionReducer::default();

    let input = ReducerEventInput {
        protocol_version: PROJECTION_PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1000,
        session_id: Some("s1".into()),
        turn_id: None,
        payload: ProjectionEvent::ToolProgramSubmitted {
            program_id: "tp-1".into(),
            job_id: "j-1".into(),
            submitted_at: 1000,
        },
    };

    let outcome = reducer.apply(&mut snapshot, input);
    assert!(matches!(
        outcome,
        codegg_protocol::projection::reducer::ApplyOutcome::Applied
    ));
    assert!(!snapshot.diagnostics.is_empty());
    let diag = &snapshot.diagnostics[0];
    assert_eq!(diag.code, "tool_program_submitted");
}
