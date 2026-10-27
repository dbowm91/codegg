//! M003 — the delegated tab follows the delegated run's recorded transcript.
//!
//! The tab used to be a boundary read: whatever the session projection
//! carried when it opened, and nothing after. These tests pin the **live**
//! behaviour and, just as importantly, the honesty of the states the tab
//! can be in.
//!
//! Three claims are separated here, because they used to be one:
//!
//! * the **fetched** rows (read through the authorized page read) are what
//!   the tab shows, and they win over a stale snapshot preview;
//! * the **window** the tab keeps is bounded, and a bounded window says so
//!   rather than passing for the whole conversation;
//! * the follow's **state** is stated: still recording, finished, the
//!   source's own recording stopped, or the follow unavailable — and never
//!   "the subagent did nothing".

use codegg::protocol::projection::dto::{
    AgentRunSummary, AgentRunTranscriptEntryProjection, AgentRunTranscriptKind,
    AgentRunTranscriptProjection, AgentRunTranscriptRole,
};
use codegg::protocol::projection::fixtures::active_turn_snapshot;
use codegg::protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use codegg::tui::app::state::transcript_cache::{TranscriptCache, TranscriptFollowStatus};
use codegg::tui::app::App;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

const RUN_ID: &str = "run-live-1";

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

/// Seed a session projection with one delegated run, then open its tab.
///
/// `run_id` is `None` for the case where the run has no durable identity —
/// the shape a run dispatched before the transcript store existed has.
fn app_with_delegated_tab(run_id: Option<&str>) -> App {
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
        status: codegg::protocol::projection::dto::AgentTreeStatus::Running,
        parent_task_id: None,
        created_at: 1,
        completed_at: None,
        result_summary: None,
        display_name: Some("Sally".into()),
        profile: Some("Reconnaissance generalist.".into()),
        usefulness: None,
        run_id: run_id.map(str::to_string),
    });
    snapshot.active_turn = Some(turn);
    snapshot.agent_runs.push_back(AgentRunSummary {
        run_id: run_id.unwrap_or_default().to_string(),
        task_id: "task-42".into(),
        project_id: snapshot.project_id.clone(),
        workspace_id: snapshot.workspace_id.clone(),
        agent: "explore".into(),
        status: "running".into(),
        depth: 1,
        control_status: "none".into(),
        updated_at: 3,
        transcript: Some(
            AgentRunTranscriptProjection::checked(
                vec![entry(
                    0,
                    AgentRunTranscriptKind::ModelMessage,
                    "the stale preview row",
                )],
                None,
            )
            .expect("an in-bound payload"),
        ),
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

    render_app_to_buffer(&mut app, 120, 40);
    assert!(
        app.open_subagent_tab("42"),
        "the delegated run's tab must open from its projection row"
    );
    app
}

/// Give the open tab a live-follow cache holding `entries`.
fn seed_cache(
    app: &mut App,
    status: TranscriptFollowStatus,
    entries: Vec<AgentRunTranscriptEntryProjection>,
) {
    let tab_id = app
        .ui_state
        .subagents
        .tabs
        .active_tab_id()
        .cloned()
        .expect("a delegated tab is open");
    let tab = app
        .ui_state
        .subagents
        .tabs
        .get_mut(&tab_id)
        .expect("the delegated tab");
    let mut cache = TranscriptCache::new(RUN_ID, "sess-1", "project-1", None);
    cache.status = status;
    for row in entries {
        cache.entries.push_back(row);
    }
    tab.transcript = Some(cache);
}

/// Append one more row to the open tab's fetched entries.
fn append_fetched(app: &mut App, row: AgentRunTranscriptEntryProjection) {
    let tab_id = app
        .ui_state
        .subagents
        .tabs
        .active_tab_id()
        .cloned()
        .expect("a delegated tab is open");
    let tab = app
        .ui_state
        .subagents
        .tabs
        .get_mut(&tab_id)
        .expect("the delegated tab");
    tab.transcript
        .as_mut()
        .expect("a live-follow cache")
        .entries
        .push_back(row);
}

#[test]
fn the_tab_renders_the_rows_the_page_read_fetched() {
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(
        &mut app,
        TranscriptFollowStatus::Following,
        vec![
            entry(0, AgentRunTranscriptKind::ModelMessage, "fetched row zero"),
            entry(1, AgentRunTranscriptKind::ToolCall, "fetched row one"),
        ],
    );
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "fetched row zero"),
        "a row the page read fetched must be rendered"
    );
    assert!(
        buffer_contains(&buffer, "fetched row one"),
        "every fetched row must be rendered, not just the first"
    );
}

#[test]
fn the_fetched_rows_win_over_a_stale_snapshot_preview() {
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(
        &mut app,
        TranscriptFollowStatus::Following,
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "fetched row zero",
        )],
    );
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        !buffer_contains(&buffer, "the stale preview row"),
        "a snapshot preview must not displace the rows the page read fetched"
    );
}

#[test]
fn a_bounded_window_says_that_earlier_entries_were_not_loaded() {
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(
        &mut app,
        TranscriptFollowStatus::Following,
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "the kept tail row",
        )],
    );
    let tab_id = app
        .ui_state
        .subagents
        .tabs
        .active_tab_id()
        .cloned()
        .expect("a delegated tab is open");
    app.ui_state
        .subagents
        .tabs
        .get_mut(&tab_id)
        .expect("the delegated tab")
        .transcript
        .as_mut()
        .expect("a live-follow cache")
        .evicted_entries = 12;
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "earlier entries not loaded"),
        "a bounded window must say that the earlier entries were not loaded"
    );
}

#[test]
fn a_run_with_no_durable_identity_says_live_follow_is_unavailable() {
    let mut app = app_with_delegated_tab(None);
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "Live follow unavailable"),
        "a run with no durable identity must be labelled, not shown as an empty pane"
    );
    assert!(
        buffer_contains(&buffer, "no durable identity"),
        "the label must name why the follow is unavailable"
    );
}

#[test]
fn a_follow_that_is_still_recording_does_not_claim_the_transcript_is_complete() {
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(
        &mut app,
        TranscriptFollowStatus::Following,
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "fetched row zero",
        )],
    );
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "following"),
        "a run that may still record must be shown as following"
    );
    assert!(
        !buffer_contains(&buffer, "complete ·"),
        "a run that may still record must not be shown as complete"
    );
}

#[test]
fn a_source_whose_own_recording_stopped_is_not_shown_as_complete() {
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(
        &mut app,
        TranscriptFollowStatus::RecordingStopped,
        vec![entry(
            0,
            AgentRunTranscriptKind::ModelMessage,
            "fetched row zero",
        )],
    );
    let buffer = render_app_to_buffer(&mut app, 120, 40);
    assert!(
        buffer_contains(&buffer, "recording stopped at the store's per-run bound"),
        "the source's own truncation must be stated as the source's truncation"
    );
    assert!(
        !buffer_contains(&buffer, "complete ·"),
        "a truncated source must not be shown as a complete transcript"
    );
}

#[test]
fn a_scrolled_up_view_is_anchored_when_new_rows_arrive() {
    let mut rows = Vec::new();
    for index in 0..40 {
        rows.push(entry(
            index,
            AgentRunTranscriptKind::ModelMessage,
            &format!("conversation row {index}"),
        ));
    }
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(&mut app, TranscriptFollowStatus::Following, rows);

    // The reader scrolls up from the top of the conversation.
    app.ui_state.subagents.tabs.scroll_active(12);
    let before = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&before, "conversation row 1"),
        "the scrolled view shows the rows the fold was hiding"
    );

    // A new row lands while the reader is still up there.
    append_fetched(
        &mut app,
        entry(
            40,
            AgentRunTranscriptKind::ModelMessage,
            "conversation row 40",
        ),
    );
    let after = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&after, "conversation row 1"),
        "a new row must not pull a scrolled-up view away from what the reader was reading"
    );
    assert!(
        !buffer_contains(&after, "conversation row 40"),
        "the new row is below the fold for a reader who scrolled up"
    );
}

#[test]
fn a_view_pinned_to_the_bottom_follows_new_rows() {
    let mut rows = Vec::new();
    for index in 0..40 {
        rows.push(entry(
            index,
            AgentRunTranscriptKind::ModelMessage,
            &format!("conversation row {index}"),
        ));
    }
    let mut app = app_with_delegated_tab(Some(RUN_ID));
    seed_cache(&mut app, TranscriptFollowStatus::Following, rows);

    // The reader jumps to the live edge.
    app.ui_state.subagents.tabs.scroll_active_to_bottom();
    let pinned = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&pinned, "conversation row 39"),
        "a view pinned to the bottom shows the last recorded row"
    );

    append_fetched(
        &mut app,
        entry(
            40,
            AgentRunTranscriptKind::ModelMessage,
            "conversation row 40",
        ),
    );
    let followed = render_app_to_buffer(&mut app, 120, 20);
    assert!(
        buffer_contains(&followed, "conversation row 40"),
        "a view pinned to the bottom must follow the new row"
    );
}
