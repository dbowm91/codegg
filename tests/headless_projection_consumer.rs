//! End-to-end protocol-surface exercise for the non-TUI reference consumer.
//!
//! The harness below stands in for an authenticated core transport: all
//! inputs are the same typed `CoreResponse` frames a daemon emits, while the
//! consumer itself has no dependency on the TUI crate or rendering state.

use codegg::protocol::core::CoreResponse;
use codegg::protocol::projection::consumer::{
    HeadlessConnectionState, HeadlessEventOutcome, HeadlessProjectionConsumer,
};
use codegg::protocol::projection::dto::SessionSummaryProjection;
use codegg::protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg::protocol::projection::fixtures::{
    active_turn_event_script, idle_snapshot, FIXTURE_PROJECT_ID, FIXTURE_SESSION_ID,
};
use codegg::protocol::projection::replay::{
    ArtifactHandleKind, ProjectionArtifactHandleDto, ProjectionArtifactReadOutcome,
    ProjectionArtifactReadRequest, ProjectionCursor, ProjectionReplayBatch,
    ProjectionSnapshotBundle, ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
    ProjectionSubscriptionId,
};
use codegg::protocol::projection::PROJECTION_PROTOCOL_VERSION;

fn descriptor() -> ProjectionStreamDescriptor {
    ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("session-stream-1").unwrap(),
        kind: ProjectionStreamKind::Session,
        project_id: FIXTURE_PROJECT_ID.into(),
        workspace_id: Some("workspace-fixture".into()),
        session_id: Some(FIXTURE_SESSION_ID.into()),
        projection_version: PROJECTION_PROTOCOL_VERSION,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    }
}

fn response_capabilities() -> CoreResponse {
    CoreResponse::ProjectionCapabilitiesResponse {
        supported: true,
        projection_version: PROJECTION_PROTOCOL_VERSION,
        max_events_per_batch: 512,
        max_event_bytes: 64 * 1024,
        max_subscriptions_per_client: 32,
        max_subscriptions_per_daemon: 256,
        retention_session_max_events: 20_000,
        retention_project_max_events: 50_000,
    }
}

#[test]
fn headless_consumer_bootstraps_replays_resumes_and_reads_bounded_artifact() {
    let mut consumer = HeadlessProjectionConsumer::new();
    consumer.accept_response(&response_capabilities()).unwrap();
    assert_eq!(
        consumer.connection_state(),
        HeadlessConnectionState::Connected
    );

    let descriptor = descriptor();
    consumer
        .accept_response(&CoreResponse::ProjectionSubscribed {
            subscription_id: ProjectionSubscriptionId::new("sub-1"),
            descriptor: descriptor.clone(),
            snapshot: ProjectionSnapshotBundle::One {
                snapshot: Box::new(idle_snapshot()),
            },
            cursor: ProjectionCursor {
                stream_id: descriptor.stream_id.clone(),
                event_seq: 0,
                projection_version: PROJECTION_PROTOCOL_VERSION,
            },
            retention_floor_seq: 0,
        })
        .unwrap();

    let events: Vec<_> = active_turn_event_script()
        .into_iter()
        .map(|input| ProjectionEnvelope {
            protocol_version: input.protocol_version,
            event_seq: input.event_seq,
            timestamp_ms: input.timestamp_ms,
            session_id: input.session_id,
            turn_id: input.turn_id,
            scope: codegg::protocol::projection::event::ProjectionStreamScope::Session,
            payload: input.payload,
        })
        .collect();
    let replay = CoreResponse::ProjectionReplay {
        subscription_id: Some(ProjectionSubscriptionId::new("sub-1")),
        batch: ProjectionReplayBatch {
            descriptor: descriptor.clone(),
            events: events.clone(),
            snapshot: None,
            replay_start_seq: 1,
            replay_end_seq: 3,
            current_high_water: 3,
            truncation_flag: false,
            next_cursor: None,
        },
    };
    let replay_outcome = consumer.accept_response(&replay).unwrap().unwrap();
    assert_eq!(replay_outcome.applied, 3);
    assert_eq!(consumer.cursor().unwrap().event_seq, 3);
    assert_eq!(
        consumer
            .snapshot()
            .unwrap()
            .active_turn
            .as_ref()
            .unwrap()
            .messages
            .len(),
        1
    );

    // A replayed delivery is harmless and does not duplicate state.
    assert!(matches!(
        consumer.apply_event(events[2].clone()),
        HeadlessEventOutcome::Duplicate { .. }
    ));
    assert_eq!(
        consumer
            .snapshot()
            .unwrap()
            .active_turn
            .as_ref()
            .unwrap()
            .tools
            .len(),
        1
    );

    // The cursor survives interruption; the next request is authoritative
    // cursor-based resume rather than a local history reconstruction.
    consumer.disconnect();
    consumer
        .connect(&codegg::protocol::projection::ProjectionCapabilities::current())
        .unwrap();
    let resume = consumer.resume_request().unwrap();
    assert!(matches!(
        resume,
        codegg::protocol::core::CoreRequest::ProjectionResume { .. }
    ));

    let terminal_seq = consumer.cursor().unwrap().event_seq + 1;
    let terminal_event = ProjectionEnvelope::session_event(
        terminal_seq,
        terminal_seq as i64,
        FIXTURE_SESSION_ID,
        Some("turn-1".into()),
        ProjectionEvent::TurnCompleted {
            turn_id: "turn-1".into(),
            stop_reason: "ok".into(),
            completed_at: terminal_seq as i64,
        },
    );
    let terminal_run_seq = terminal_seq + 1;
    let terminal_run = ProjectionEnvelope::session_event(
        terminal_run_seq,
        terminal_run_seq as i64,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::RunStarted {
            run: codegg::protocol::projection::dto::RunProjection {
                run_id: "run-1".into(),
                kind: "test".into(),
                command: "cargo test".into(),
                status: "running".into(),
                summary: String::new(),
                job_id: None,
                log_dir: None,
                started_at: terminal_run_seq as i64,
                completed_at: None,
                artifact_count: 0,
                pinned: false,
            },
        },
    );
    let completed_run_seq = terminal_run_seq + 1;
    let completed_run = ProjectionEnvelope::session_event(
        completed_run_seq,
        completed_run_seq as i64,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::RunCompleted {
            run_id: "run-1".into(),
            status: "completed".into(),
            summary: "ok".into(),
            completed_at: completed_run_seq as i64,
        },
    );
    let session_terminal_seq = completed_run_seq + 1;
    let mut session_summary: SessionSummaryProjection =
        consumer.snapshot().unwrap().primary_session.clone();
    session_summary.status = "completed".into();
    let session_terminal = ProjectionEnvelope::session_event(
        session_terminal_seq,
        session_terminal_seq as i64,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::SessionActivated {
            summary: session_summary,
        },
    );
    let replay = CoreResponse::ProjectionReplay {
        subscription_id: Some(ProjectionSubscriptionId::new("sub-2")),
        batch: ProjectionReplayBatch {
            descriptor,
            events: vec![
                terminal_event,
                terminal_run,
                completed_run,
                session_terminal,
            ],
            snapshot: None,
            replay_start_seq: terminal_seq,
            replay_end_seq: session_terminal_seq,
            current_high_water: session_terminal_seq,
            truncation_flag: false,
            next_cursor: None,
        },
    };
    consumer.accept_response(&replay).unwrap();
    assert!(consumer.session_is_terminal());
    assert_eq!(consumer.run_is_terminal("run-1"), Some(true));

    consumer
        .accept_response(&CoreResponse::ProjectionArtifactList {
            handles: vec![ProjectionArtifactHandleDto {
                handle_id: "artifact-1".into(),
                kind: ArtifactHandleKind::RunOutput,
                project_id: FIXTURE_PROJECT_ID.into(),
                source_record_id: "run-1".into(),
                content_type: "text/plain".into(),
                total_bytes: Some(5),
                created_at: 0,
                expires_at: None,
                revision: 1,
                public_summary: Some("hello".into()),
            }],
        })
        .unwrap();
    let request = consumer
        .artifact_read_request("artifact-1", 0, Some(5))
        .unwrap();
    assert!(matches!(
        request,
        codegg::protocol::core::CoreRequest::ProjectionArtifactRead { .. }
    ));
    consumer
        .accept_response(&CoreResponse::ProjectionArtifactRead {
            outcome: ProjectionArtifactReadOutcome::Ok(
                codegg::protocol::projection::replay::ProjectionArtifactReadResponse {
                    handle_id: "artifact-1".into(),
                    revision: 1,
                    start: 0,
                    end: 5,
                    content_type: "text/plain".into(),
                    content: "hello".into(),
                    redacted: false,
                    truncated: false,
                    note: None,
                },
            ),
        })
        .unwrap();
    assert_eq!(consumer.last_artifact().unwrap().content, "hello");
}

#[test]
fn headless_consumer_rejects_private_reasoning_and_malformed_artifact() {
    let mut consumer = HeadlessProjectionConsumer::new();
    consumer
        .connect(&codegg::protocol::projection::ProjectionCapabilities::current())
        .unwrap();
    let descriptor = descriptor();
    consumer
        .accept_subscribed(
            ProjectionSubscriptionId::new("sub-1"),
            descriptor,
            ProjectionSnapshotBundle::One {
                snapshot: Box::new(idle_snapshot()),
            },
            ProjectionCursor {
                stream_id: ProjectionStreamId::new("session-stream-1").unwrap(),
                event_seq: 0,
                projection_version: PROJECTION_PROTOCOL_VERSION,
            },
        )
        .unwrap();
    let private = ProjectionEnvelope::session_event(
        1,
        1,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::ReasoningAppended {
            message_id: "reasoning-1".into(),
            delta: "must not escape".into(),
        },
    );
    assert!(matches!(
        consumer.apply_event(private),
        HeadlessEventOutcome::IgnoredNonPublic { event_seq: 1 }
    ));

    consumer
        .accept_artifact_handles(vec![ProjectionArtifactHandleDto {
            handle_id: "artifact-1".into(),
            kind: ArtifactHandleKind::RunOutput,
            project_id: FIXTURE_PROJECT_ID.into(),
            source_record_id: "run-1".into(),
            content_type: "text/plain".into(),
            total_bytes: None,
            created_at: 0,
            expires_at: None,
            revision: 1,
            public_summary: None,
        }])
        .unwrap();
    let oversized = ProjectionArtifactReadOutcome::Ok(
        codegg::protocol::projection::replay::ProjectionArtifactReadResponse {
            handle_id: "artifact-1".into(),
            revision: 1,
            start: 0,
            end: 1,
            content_type: "text/plain".into(),
            content: "x".repeat(ProjectionArtifactReadRequest::MAX_READ_BYTES as usize + 1),
            redacted: false,
            truncated: false,
            note: None,
        },
    );
    assert!(consumer.accept_artifact_outcome(&oversized).is_err());
}
