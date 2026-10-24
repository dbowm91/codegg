//! C003 — the delegated tab renders the delegated run's recorded transcript.
//!
//! The delegated tab used to be a run inspector: it named the agent, the
//! task, the status and the ids, and deliberately showed no conversation.
//! It is now both — the inspector metadata is **retained**, and the child's
//! recorded transcript is rendered beneath it.
//!
//! The three transcript states are three different facts and each one is
//! rendered distinctly:
//!
//! * **complete** — the payload holds everything that was recorded;
//! * **truncated** — it holds a prefix, and the tab says where it stops;
//! * **absent** — nothing was recorded, and the tab says that rather than
//!   painting an empty pane that reads as "the child did nothing".

use codegg::protocol::projection::dto::{
    AgentRunSummary, AgentRunTranscriptEntryProjection, AgentRunTranscriptKind,
    AgentRunTranscriptProjection, AgentRunTranscriptRole, AgentRunTranscriptTruncation,
};
use codegg::protocol::projection::fixtures::active_turn_snapshot;
use codegg::protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use codegg::tui::app::App;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

const RUN_ID: &str = "run-delegated-1";

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn render_app_to_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text_in_buffer(buffer: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            text.push(cell.symbol().chars().next().unwrap_or(' '));
        }
        text.push('\n');
    }
    text
}

fn buffer_contains(buffer: &Buffer, needle: &str) -> bool {
    text_in_buffer(buffer)
        .to_lowercase()
        .contains(&needle.to_lowercase())
}

/// A case-sensitive containment check, for a heading whose capitalisation is
/// itself the signal.
fn buffer_contains_exact(buffer: &Buffer, needle: &str) -> bool {
    text_in_buffer(buffer).contains(needle)
}

fn entry(
    ordinal: i64,
    kind: AgentRunTranscriptKind,
    body: &str,
) -> AgentRunTranscriptEntryProjection {
    AgentRunTranscriptEntryProjection {
        ordinal,
        role: AgentRunTranscriptRole::Orchestrator,
        kind,
        body: body.to_string(),
        byte_len: body.len() as i64,
        created_at: 1_700_000_000_000 + ordinal,
    }
}

/// Seed a session projection with one delegated run carrying `transcript`,
/// then open that run's tab.
fn app_with_delegated_tab(transcript: Option<AgentRunTranscriptProjection>) -> App {
    use codegg::protocol::projection::dto::AgentTreeNodeProjection;

    let mut app = test_app();
    let mut snapshot = active_turn_snapshot();
    let mut turn = snapshot
        .active_turn
        .expect("the fixture has an active turn");
    turn.agent_tree.push_back(AgentTreeNodeProjection {
        child_instance_id: String::new(),
        originating_turn_id: None,
        task_id: 42,
        agent: "explore".into(),
        description: "map the subsystem".into(),
        status: codegg::protocol::projection::dto::AgentTreeStatus::Completed,
        parent_task_id: None,
        created_at: 1,
        completed_at: Some(2),
        result_summary: None,
        display_name: Some("Sally".into()),
        profile: Some("Reconnaissance generalist.".into()),
        usefulness: None,
        run_id: Some(RUN_ID.into()),
    });
    snapshot.active_turn = Some(turn);
    snapshot.agent_runs.push_back(AgentRunSummary {
        run_id: RUN_ID.into(),
        task_id: "task-42".into(),
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        agent: "explore".into(),
        status: "completed".into(),
        depth: 1,
        control_status: "none".into(),
        updated_at: 3,
        transcript,
        ..Default::default()
    });

    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: snapshot.project_id.clone(),
        workspace_id: Some(snapshot.workspace_id.clone()),
        session_id: Some(snapshot.primary_session_id.clone()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    app.projection_client
        .controller_mut()
        .unsubscribe(&ProjectionSubscriptionId::new("sub-1"));
    app.projection_client.controller_mut().install_subscription(
        ProjectionSubscriptionId::new("sub-1"),
        descriptor,
        snapshot,
    );

    // One render pass: the rows are derived from the snapshot during
    // rendering, exactly as they are in the real TUI.
    render_app_to_buffer(&mut app, 120, 40);
    assert!(
        app.open_subagent_tab("42"),
        "the delegated run's tab must open from its projection row"
    );
    app
}

#[test]
fn the_tab_renders_the_conversation() {
    let transcript = AgentRunTranscriptProjection::checked(
        vec![
            entry(
                0,
                AgentRunTranscriptKind::ModelMessage,
                "reading the auth flow",
            ),
            entry(1, AgentRunTranscriptKind::ToolCall, "grep -rn authorize"),
        ],
        None,
    )
    .expect("an in-bound payload");
    let mut app = app_with_delegated_tab(Some(transcript));
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "reading the auth flow"),
        "the tab must render the conversation it was given"
    );
    assert!(
        buffer_contains(&buffer, "grep -rn authorize"),
        "the tab must render every row of the transcript"
    );
}

#[test]
fn the_tab_retains_the_inspector_metadata_above_the_transcript() {
    let transcript = AgentRunTranscriptProjection::checked(
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "reading the auth flow",
        )],
        None,
    )
    .expect("an in-bound payload");
    let mut app = app_with_delegated_tab(Some(transcript));
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    let text = text_in_buffer(&buffer);

    // The run's identity and status are still on screen.
    assert!(
        buffer_contains(&buffer, "Sally"),
        "the display name is retained"
    );
    assert!(
        buffer_contains(&buffer, "explore"),
        "the agent type is retained"
    );
    assert!(
        buffer_contains(&buffer, "completed"),
        "the status is retained"
    );
    assert!(
        buffer_contains(&buffer, RUN_ID),
        "the durable run id is retained"
    );
    assert!(
        buffer_contains(&buffer, "map the subsystem"),
        "the task description is retained"
    );

    // The metadata comes first, the conversation after it.
    let metadata = text.find("Identity").expect("the identity section");
    let conversation = text
        .find("reading the auth flow")
        .expect("the conversation");
    assert!(
        metadata < conversation,
        "the run's identity and status are rendered above the conversation"
    );
}

#[test]
fn the_absent_case_renders_a_statement_rather_than_an_empty_pane() {
    let mut app = app_with_delegated_tab(Some(AgentRunTranscriptProjection::Absent));
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "No transcript was recorded for this run"),
        "an absent transcript must be stated, not rendered as an empty pane"
    );
    assert!(
        !buffer_contains(&buffer, "complete ·"),
        "an absent transcript must not read as a complete one"
    );
}

#[test]
fn the_truncated_case_renders_the_truncation_point() {
    let transcript = AgentRunTranscriptProjection::checked(
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "the first row",
        )],
        Some(AgentRunTranscriptTruncation {
            next_ordinal: 1,
            omitted_entries: 9,
            recording_stopped: false,
        }),
    )
    .expect("an in-bound payload");
    let mut app = app_with_delegated_tab(Some(transcript));
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "truncated"),
        "a truncated transcript must say so"
    );
    assert!(
        buffer_contains(&buffer, "not the whole conversation"),
        "a truncated transcript must say that it is not the whole conversation"
    );
    assert!(
        buffer_contains(&buffer, "ordinal 1"),
        "a truncated transcript must show where it stops"
    );
}

#[test]
fn a_transcript_longer_than_the_viewport_is_navigable_and_says_more_exists() {
    let mut rows = Vec::new();
    for index in 0..40 {
        rows.push(entry(
            index,
            AgentRunTranscriptKind::ModelMessage,
            &format!("conversation row {index}"),
        ));
    }
    let transcript =
        AgentRunTranscriptProjection::checked(rows, None).expect("an in-bound payload");
    let mut app = app_with_delegated_tab(Some(transcript));

    let buffer = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&buffer, "more lines below"),
        "a transcript taller than the viewport must say that more exists"
    );

    // The tab is navigable: scrolling moves the view, and the row that was
    // below the fold becomes visible.
    app.ui_state.subagents.tabs.scroll_active(12);
    let scrolled = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&scrolled, "conversation row 1"),
        "the scrolled view shows the rows the fold was hiding"
    );
}

#[test]
fn an_older_daemon_leaves_the_tab_as_the_run_inspector() {
    // No transcript supplied at all — a daemon older than the transcript
    // milestone, or a summary built for a lifecycle event. The tab stays the
    // inspector it was, and it does not claim the run recorded nothing: that
    // is a different fact.
    let mut app = app_with_delegated_tab(None);
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "Sally"),
        "the inspector metadata is still rendered"
    );
    assert!(
        buffer_contains(&buffer, RUN_ID),
        "the durable run id is still rendered"
    );
    assert!(
        !buffer_contains(&buffer, "No transcript was recorded"),
        "a summary with no transcript is not the same as a run that recorded none"
    );
    assert!(
        !buffer_contains_exact(&buffer, " Transcript"),
        "there is no transcript section to render"
    );
}

#[test]
fn a_tool_call_and_a_tool_result_render_as_such_and_a_huge_output_renders_bounded() {
    // Inside the wire bound (8 KiB) but over the display bound (4 KiB): the
    // payload crosses whole and the screen shows a bounded piece of it.
    let huge = "x".repeat(6 * 1024);
    let transcript = AgentRunTranscriptProjection::checked(
        vec![
            entry(0, AgentRunTranscriptKind::ToolCall, "bash -lc ls"),
            entry(1, AgentRunTranscriptKind::ToolResult, &huge),
        ],
        None,
    )
    .expect("an in-bound payload");
    let mut app = app_with_delegated_tab(Some(transcript));
    // A tall viewport: the cut body wraps over many rows, and the marker
    // that says it was cut sits below them.
    let buffer = render_app_to_buffer(&mut app, 120, 120);
    let text = text_in_buffer(&buffer);
    assert!(
        buffer_contains(&buffer, "tool call"),
        "a tool call must render as a tool call"
    );
    assert!(
        buffer_contains(&buffer, "tool result"),
        "a tool result must render as a tool result"
    );
    assert!(
        buffer_contains(&buffer, "more bytes of this tool result are not shown"),
        "an unbounded tool output must render bounded, and say so"
    );
    let painted_x = text.chars().filter(|character| *character == 'x').count();
    assert!(
        painted_x < huge.len(),
        "the whole blob must not reach the screen: {painted_x} of {} bytes painted",
        huge.len()
    );
}
