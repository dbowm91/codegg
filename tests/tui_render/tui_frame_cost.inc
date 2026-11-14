//! M001: the frame's own per-frame work, counted.
//!
//! `tui_render_cost.inc` measures the *content* caches C006 introduced. This
//! member measures the **frame composition** around them: the retained frame
//! buffer, the sidebar projection and its line model, the subagent tab, the
//! composer hint, the status summary, the tasks overlay's rows, and the
//! transcript watermark scan.
//!
//! Every test here reads a `RenderCost` snapshot before and after one
//! `App::render` call, so the number it asserts is the number of operations
//! **one drawn frame** performed. The counters count operations, not time;
//! no wall-clock claim is made anywhere and none could be reproduced on a
//! shared machine.
//!
//! The tests are written against the *after* state. On the baseline they
//! fail, and the failure output is the before-count: that is WP1's evidence.

use codegg::protocol::projection::dto::{
    AgentRunTranscriptEntryProjection, AgentRunTranscriptKind, AgentRunTranscriptRole,
    AgentTreeNodeProjection, AgentTreeStatus, JobProjection, TurnStatus,
};
use codegg::protocol::projection::fixtures::active_turn_snapshot;
use codegg::protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use codegg::tui::app::state::subagents::SubagentRow;
use codegg::tui::app::App;
use codegg::tui::input::InputMode;
use codegg::tui::panels::PanelPlacement;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::selection::TextSelection;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::Terminal;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn render(app: &mut App) -> Buffer {
    render_at(app, 120, 40)
}

fn render_at(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .expect("the frame draws");
    terminal.backend().buffer().clone()
}

/// One drawn frame's operation count.
fn frame_cost(app: &mut App) -> RenderCost {
    let before = render_cost::snapshot();
    let _buffer = render(app);
    render_cost::snapshot().since(&before)
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn subagent_row(task_id: &str, name: &str) -> SubagentRow {
    SubagentRow {
        child_instance_id: None,
        task_id: task_id.to_string(),
        run_id: None,
        display_name: Some(name.to_string()),
        profile: None,
        usefulness: None,
        agent: "explore".into(),
        description: "map the subsystem".into(),
        status: "running".into(),
        transcript: None,
    }
}

/// Install a one-subagent projection snapshot at `event_seq`.
///
/// The subscription is torn down first because `install_subscription`
/// rejects a duplicate stream id; without the unsubscribe a re-seed is
/// silently ignored and the test would assert against the previous
/// snapshot.
fn install_snapshot(app: &mut App, event_seq: u64, subagent_name: &str) {
    let mut snapshot = active_turn_snapshot();
    let mut turn = snapshot
        .active_turn
        .expect("the fixture carries an active turn");
    turn.status = TurnStatus::Active;
    turn.agent_tree.push_back(AgentTreeNodeProjection {
        child_instance_id: String::new(),
        originating_turn_id: None,
        task_id: 1,
        agent: "explore".into(),
        description: "map the subsystem".into(),
        status: AgentTreeStatus::Running,
        parent_task_id: None,
        created_at: 1,
        completed_at: None,
        result_summary: None,
        display_name: Some(subagent_name.to_string()),
        profile: None,
        usefulness: None,
        run_id: None,
    });
    snapshot.active_turn = Some(turn);
    snapshot.event_seq = event_seq;
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: snapshot.project_id.clone(),
        workspace_id: Some(snapshot.workspace_id.clone()),
        session_id: Some(snapshot.primary_session_id.clone()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: event_seq,
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
}

// ── WP2: the retained frame buffer ──────────────────────────────────────

/// A frame with no selection retains nothing.
#[test]
fn a_frame_with_no_selection_retains_no_frame_buffer() {
    let mut app = test_app();
    let cost = frame_cost(&mut app);

    assert!(
        app.last_frame_buffer.is_none(),
        "a frame nobody can drag over must not keep a copy of itself"
    );
    assert_eq!(
        cost.frame_buffer_clones, 0,
        "the frame copied its terminal buffer with no selection to resolve"
    );
}

/// A selection still retains exactly the frame that was drawn.
#[test]
fn a_frame_with_a_selection_retains_the_buffer_it_drew() {
    let mut app = test_app();
    app.messages_state
        .messages
        .add_assistant_text("the text a drag will later copy".into());
    app.selection_anchor = Some((2, 4));
    app.selection_focus = Some((20, 4));
    let before = render_cost::snapshot();
    let drawn = render(&mut app);
    let cost = render_cost::snapshot().since(&before);

    assert_eq!(
        cost.frame_buffer_clones, 1,
        "a live selection retains exactly one frame"
    );
    let retained = app
        .last_frame_buffer
        .as_ref()
        .expect("a live selection retains the frame it was drawn on");
    assert_eq!(
        &drawn, retained,
        "the retained buffer must be the frame the user saw"
    );
    let selection = TextSelection::new((2, 4), (20, 4));
    assert_eq!(
        codegg::tui::selection::extract(&drawn, selection),
        codegg::tui::selection::extract(retained, selection),
        "a drag resolves to the same text from either copy"
    );
}

/// Clearing a selection drops the frame it was retained for.
#[test]
fn clearing_a_selection_drops_the_retained_buffer() {
    let mut app = test_app();
    app.selection_anchor = Some((2, 4));
    app.selection_focus = Some((20, 4));
    render(&mut app);
    assert!(
        app.last_frame_buffer.is_some(),
        "the fixture must retain a frame for this test to mean anything"
    );

    app.clear_selection();

    assert!(
        app.last_frame_buffer.is_none(),
        "the buffer existed for the selection, so it goes with the selection"
    );
}

// ── WP3: the sidebar projection and line model ──────────────────────────

/// Two occupied panels do not populate the projection twice.
#[test]
fn two_open_panels_populate_the_sidebar_once_per_frame() {
    let mut app = test_app();
    app.panel_host.set_visible_at(PanelPlacement::Left, true);
    assert!(
        app.panel_host.is_visible_at(PanelPlacement::Right),
        "the fixture must have two panels open"
    );

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.sidebar_populations, 1,
        "the population step ran once per occupied panel, not once per frame"
    );
}

/// A frame whose inputs did not move does no population work at all.
#[test]
fn an_unchanged_frame_does_not_repopulate_the_sidebar() {
    let mut app = test_app();
    render(&mut app);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.sidebar_populations, 0,
        "nothing the population reads changed, so it must not run"
    );
}

/// The sidebar's line model is built once per render.
#[test]
fn the_sidebar_builds_its_line_model_once_per_frame() {
    let mut app = test_app();
    render(&mut app);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.sidebar_line_builds, 1,
        "the scroll clamp and the scrollbar each rebuilt the whole model"
    );
}

/// A projection that arrives on a frame is drawn on that frame.
#[test]
fn a_new_projection_is_drawn_on_the_frame_it_arrives() {
    let mut app = test_app();
    install_snapshot(&mut app, 0, "Sally");
    let first = buffer_text(&render(&mut app));
    assert!(
        first.contains("Sally"),
        "the fixture's first projection must reach the panel"
    );

    install_snapshot(&mut app, 1, "Bruno");
    let cost = frame_cost(&mut app);
    let second = buffer_text(&render(&mut app));

    assert_eq!(
        cost.sidebar_populations, 1,
        "a changed projection revision forces exactly one population"
    );
    assert!(
        second.contains("Bruno"),
        "the arriving projection must be drawn on the frame it arrives"
    );
    assert!(
        !second.contains("Sally"),
        "the replaced projection must not be served from the old rows"
    );
}

// ── WP4: the subagent tab ──────────────────────────────────────────────

/// A frame with a subagent tab open borrows the tab instead of copying it.
#[test]
fn an_open_subagent_tab_is_not_cloned_per_frame() {
    let mut app = test_app();
    app.ui_state
        .subagents
        .set_rows(vec![subagent_row("1", "Sally")]);
    assert!(
        app.ui_state.subagents.open_task("1"),
        "the fixture must open a tab"
    );
    // A populated transcript cache is the cost the clone used to copy; the
    // borrow must render it without one.
    if let Some(tab) = app.ui_state.subagents.active_tab_mut() {
        let mut cache = codegg::tui::app::state::transcript_cache::TranscriptCache::new(
            "run-1",
            "session-1",
            "project-1",
            None,
        );
        cache.entries.push_back(AgentRunTranscriptEntryProjection {
            ordinal: 0,
            role: AgentRunTranscriptRole::Orchestrator,
            kind: AgentRunTranscriptKind::ModelMessage,
            body: "the child said this".into(),
            byte_len: 20,
            created_at: 1,
        });
        tab.transcript = Some(cache);
    }
    render(&mut app);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.subagent_tab_clones, 0,
        "the view copied the tab (and its transcript cache) to read it"
    );
}

/// The borrowed subagent view draws what the cloned one drew.
#[test]
fn the_subagent_view_draws_the_same_buffer_on_every_frame() {
    let mut app = test_app();
    app.ui_state
        .subagents
        .set_rows(vec![subagent_row("1", "Sally")]);
    assert!(app.ui_state.subagents.open_task("1"));

    let cold = render(&mut app);
    let warm = render(&mut app);

    assert_eq!(
        cold, warm,
        "a warm frame must draw exactly what the cold frame drew"
    );
}

// ── WP5: the composer hint ─────────────────────────────────────────────

/// The hint line is built once per frame, not once for its height and once
/// for its text — on every branch of the builder's dispatch.
///
/// The two branches here are the composer's own row (Insert) and the LLM
/// buffer's turn keys (Working). The build is made once where the row's
/// height is decided and the resulting string is carried to the draw site,
/// so no branch can reintroduce a second build.
#[test]
fn the_composer_hint_is_built_once_per_frame() {
    let mut cases: Vec<(&str, App)> = Vec::new();

    let mut insert = test_app();
    insert.ui_state.input_mode = InputMode::Insert;
    cases.push(("insert", insert));

    let mut working = test_app();
    working.session_state.session_status = codegg::tui::app::SessionStatus::Working;
    cases.push(("working", working));

    // Resize mode outranks every surface hint, so it is the arm whose text is
    // built by a different branch than the panel and key hints below it. The
    // other branches (an armed chat seed, the editor route, an active panel)
    // are not separately constructed: the build happens at **one** call site
    // before the dispatch, so the branch cannot change how many builds a frame
    // performs — only which string is built, and that is the render comparison's
    // business rather than the counter's.
    let mut resizing = test_app();
    resizing.panel_host.set_resizing(true);
    cases.push(("resize mode", resizing));

    for (branch, mut app) in cases {
        // Warm the frame so the measurement is of a steady state.
        render(&mut app);

        let cost = frame_cost(&mut app);

        assert_eq!(
            cost.composer_hint_builds, 1,
            "{branch}: the height check and the draw each built the hint line"
        );
    }
}

// ── WP6: the status summary (deferred) ─────────────────────────────────

/// The footer still builds one summary per frame.
///
/// WP6's cache is **deferred** (plan §14: the summary's key cannot be
/// expressed without reading a field that changes every frame or widening
/// the ownership boundary). This test pins the current, recorded behaviour
/// so the cost is visible rather than forgotten.
#[test]
fn the_status_summary_is_built_once_per_frame() {
    let mut app = test_app();
    render(&mut app);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.status_summary_builds, 1,
        "the deferred footer builds exactly one summary per frame"
    );
}

// ── WP7: the tasks overlay ─────────────────────────────────────────────

/// The tasks overlay builds its row list once per frame, not twice.
#[test]
fn the_tasks_overlay_builds_its_rows_once_per_frame() {
    let mut app = test_app();
    install_snapshot(&mut app, 0, "Sally");
    app.tasks_expanded = true;
    render(&mut app);

    let cost = frame_cost(&mut app);

    assert_eq!(
        cost.task_list_builds, 1,
        "the height and the painter each derived the whole task list"
    );
    assert_eq!(
        cost.task_row_builds, 1,
        "the bounded rows were copied once for the one frame that draws them"
    );
}

// ── WP8: the watermark scan ────────────────────────────────────────────

/// An unchanged snapshot and tab set do not rescan the watermarks.
#[test]
fn an_unchanged_frame_does_not_rescan_the_watermarks() {
    let mut app = test_app();
    install_snapshot(&mut app, 0, "Sally");
    render(&mut app);

    let cost = frame_cost(&mut app);
    assert_eq!(
        cost.watermark_scans, 0,
        "the snapshot and the tab set did not move, so there was nothing to scan"
    );

    install_snapshot(&mut app, 1, "Bruno");
    let moved = frame_cost(&mut app);
    assert_eq!(
        moved.watermark_scans, 1,
        "a moved snapshot is exactly one scan"
    );
}

/// Adding a job to the snapshot changes the tasks overlay's "N more" math.
#[test]
fn the_tasks_overlay_stays_consistent_across_frames() {
    let mut app = test_app();
    install_snapshot(&mut app, 0, "Sally");
    app.tasks_expanded = true;
    let cold = render(&mut app);
    let warm = render(&mut app);

    assert_eq!(
        cold, warm,
        "a warm overlay must draw the same rows and the same totals"
    );
}

// ── WP9: render equivalence over the fixture set ───────────────────────

/// Every fixture state draws the same buffer on a warm frame as on a cold
/// one.
///
/// The warm frame is the one that reads the caches M001 introduced; a cache
/// that changed what is drawn would show up here as a difference between the
/// two buffers.
#[test]
fn every_fixture_state_draws_the_same_buffer_warm_as_cold() {
    // Idle, one panel.
    let mut idle = test_app();
    assert_eq!(render(&mut idle), render(&mut idle), "idle frame");

    // Two panels.
    let mut two = test_app();
    two.panel_host.set_visible_at(PanelPlacement::Left, true);
    assert_eq!(render(&mut two), render(&mut two), "two panels");

    // A live projection with the sidebar populated.
    let mut projected = test_app();
    install_snapshot(&mut projected, 0, "Sally");
    assert_eq!(
        render(&mut projected),
        render(&mut projected),
        "projection frame"
    );

    // The tasks overlay open over live work.
    let mut overlay = test_app();
    install_snapshot(&mut overlay, 0, "Sally");
    overlay.tasks_expanded = true;
    assert_eq!(render(&mut overlay), render(&mut overlay), "tasks overlay");

    // A subagent tab with a transcript cache.
    let mut tabbed = test_app();
    tabbed
        .ui_state
        .subagents
        .set_rows(vec![subagent_row("1", "Sally")]);
    assert!(tabbed.ui_state.subagents.open_task("1"));
    assert_eq!(render(&mut tabbed), render(&mut tabbed), "subagent tab");

    // A live selection: the one state that keeps the frame buffer.
    let mut selected = test_app();
    selected.selection_anchor = Some((2, 4));
    selected.selection_focus = Some((20, 4));
    assert_eq!(
        render(&mut selected),
        render(&mut selected),
        "live selection"
    );
}

/// The overlay's header still reports the true total after a bounded build.
#[test]
fn the_tasks_overlay_reports_the_true_group_total() {
    let mut app = test_app();
    let mut snapshot = active_turn_snapshot();
    snapshot.event_seq = 0;
    let mut turn = snapshot.active_turn.expect("fixture turn");
    turn.agent_tree.push_back(AgentTreeNodeProjection {
        child_instance_id: String::new(),
        originating_turn_id: None,
        task_id: 1,
        agent: "explore".into(),
        description: "map the subsystem".into(),
        status: AgentTreeStatus::Running,
        parent_task_id: None,
        created_at: 1,
        completed_at: None,
        result_summary: None,
        display_name: Some("Sally".into()),
        profile: None,
        usefulness: None,
        run_id: None,
    });
    snapshot.active_turn = Some(turn);
    for index in 0..3 {
        snapshot.jobs.push_back(JobProjection {
            job_id: format!("job-{index}"),
            workspace_id: snapshot.workspace_id.clone(),
            kind: "build".into(),
            state: "running".into(),
            summary: "build the thing".into(),
            session_id: Some(snapshot.primary_session_id.clone()),
            turn_id: None,
            active_attempt_id: None,
            error_class: None,
            updated_at: 0,
        });
    }
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
    app.tasks_expanded = true;
    let buffer = buffer_text(&render(&mut app));

    assert!(
        buffer.contains("Jobs (3)"),
        "the header must carry the projection's true job total: {buffer}"
    );
}
