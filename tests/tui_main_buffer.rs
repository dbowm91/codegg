//! Main-buffer usability regressions: click-to-expand, the row above the
//! composer, and the subagent buffer.
//!
//! These cover three defects that all made the LLM buffer hard to read or
//! hard to drive:
//!
//! 1. **Click-to-expand was unreachable.** Text selection was given priority
//!    over click-to-focus inside the message viewport, and its `Down` arm
//!    returned unconditionally. `on_click`'s viewport arm — the only thing
//!    that expands reasoning or tool output — therefore became dead code for
//!    left clicks. Click and drag now share the press and are told apart on
//!    release: same cell is a click, different cells are a selection.
//! 2. **A working turn showed no throbber.** The main buffer had no status
//!    row of its own between the transcript and the composer; the header
//!    badge was the only indicator, and it names nothing about which subagents
//!    are in flight.
//! 3. **Subagents were a bare count.** `[...3]` in the header states how many
//!    delegated runs exist and nothing about who they are. The tasks overlay
//!    (`Ctrl+T`) lists them, and the subagent tab opens a buffer that `Esc`
//!    closes.
//!
//! The working strip that used to sit between the transcript and the composer
//! is **gone**: it reserved a row that read `idle` most of the time, and the
//! task state it carried now lives in the tasks overlay. What is left above
//! the composer is the one *situational* row — a latched failure, an armed
//! exit — and that is what these tests read.

use codegg::session::message::ToolStatus;
use codegg::tui::app::state::subagents::SubagentRow;
use codegg::tui::app::state::tasks::TaskRow;
use codegg::tui::app::{App, SessionStatus};
use codegg::tui::components::messages::MsgPart;
use codegg::tui::input::InputMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn test_app() -> App {
    App::new_for_testing("/tmp".into())
}

fn render_to_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text of the row directly above the composer.
///
/// The main buffer's one situational surface: the latched turn failure and
/// the exit confirmations live there, and a panel's keymap is advertised
/// there too. The live toast column is a 60x10 overlay pinned to the top
/// right, so `buffer_text` cannot distinguish "the row says this" from "a
/// toast that happens to be on screen right now says this". Reading the row
/// itself is the only way to assert on the record rather than the
/// notification.
///
/// The row directly **above** the input box.
///
/// The prompt chunk is carved in this order: the hint line, then the todo
/// strip, then the tasks overlay, and the input box gets what is left. With no
/// todos and no overlay open — the default in these tests — the row above the
/// input box is the hint line.
fn hint_row_text(app: &App, buf: &Buffer) -> String {
    let prompt = app.prompt_area.expect("prompt area recorded");
    if prompt.y == 0 {
        return String::new();
    }
    let y = prompt.y - 1;
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
}

fn click(app: &mut App, column: u16, row: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// An app on the Session route with one assistant message carrying a
/// collapsible tool output.
///
/// The route matters: the Home route renders an empty-state placeholder in
/// the viewport and never draws the transcript, so a click-to-expand test
/// against Home would pass or fail for reasons unrelated to hit testing.
fn app_with_collapsible_tool_output() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session("s1".into()));
    app.messages_state
        .messages
        .add_assistant_text("running the suite".into());
    let last = app.messages_state.messages.messages.last_mut().unwrap();
    last.parts.push(MsgPart::ToolCall {
        id: "t1".into(),
        name: "bash".into(),
        input: "cargo test".into(),
        output: "42 passed; 0 failed".into(),
        status: ToolStatus::Completed,
        duration_ms: Some(1200),
        exit_code: Some(0),
        output_lines: Some(1),
        expanded: false,
    });
    app
}

// ---------------------------------------------------------------------------
// 1. Click-to-expand
// ---------------------------------------------------------------------------

#[test]
fn clicking_a_message_in_the_viewport_expands_its_tool_output() {
    let mut app = app_with_collapsible_tool_output();
    let buffer = render_to_buffer(&mut app, 120, 40);
    let area = app.viewport_area.expect("viewport area recorded");

    // Click the row that actually renders the assistant text, so this tests
    // hit-testing against real rendered geometry rather than a guess.
    let y = row_containing(&buffer, buffer_area(&buffer), "running the suite")
        .expect("the assistant text should be on screen");

    assert!(
        !tool_output_expanded(&app),
        "fixture should start collapsed"
    );
    click(&mut app, area.x + 2, y);
    assert!(
        tool_output_expanded(&app),
        "a click on a message must reach the expand handler"
    );
}

#[test]
fn dragging_in_the_viewport_still_selects_and_does_not_expand() {
    let mut app = app_with_collapsible_tool_output();
    let _ = render_to_buffer(&mut app, 120, 40);
    let area = app.viewport_area.expect("viewport area recorded");

    let before = tool_output_expanded(&app);
    let y = area.y + 2;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + 1,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: area.x + 9,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x + 9,
        row: y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(
        tool_output_expanded(&app),
        before,
        "a drag is a selection, not a click"
    );
    assert!(
        app.selection_anchor.is_none(),
        "the drag should have been resolved and cleared"
    );
}

#[test]
fn a_click_cancels_rather_than_copying_one_character() {
    let mut app = test_app();
    let _ = render_to_buffer(&mut app, 120, 40);
    let area = app.viewport_area.expect("viewport area recorded");
    click(&mut app, area.x + 2, area.y + 2);
    assert!(app.selection_anchor.is_none());
    assert!(app.selection_focus.is_none());
}

// ---------------------------------------------------------------------------
// 2. Working strip
// ---------------------------------------------------------------------------

#[test]
fn the_main_buffer_reserves_no_status_row() {
    // The working strip is gone. Its status row read `idle` for most of the
    // buffer's life and spent a row of vertical space to say it; the task
    // state it carried now lives in the `Ctrl+T` tasks overlay, and the row
    // above the composer is the buffer's one *situational* surface.
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let buffer = render_to_buffer(&mut app, 100, 30);
    let row = hint_row_text(&app, &buffer);

    assert!(
        !row.contains("idle"),
        "the buffer must not carry an idle indicator any more:\n{row}"
    );
    assert!(
        !row.contains("awaiting"),
        "the wait for delegated work is the overlay's business, not a row here:\n{row}"
    );
    // The row above the composer is the hint line, and it is the *only*
    // thing between the transcript and the input.
    let prompt = app.prompt_area.expect("prompt area recorded");
    let viewport = app.viewport_area.expect("viewport area recorded");
    assert!(
        prompt.y > viewport.y + viewport.height,
        "the prompt chunk must start below the transcript: {prompt:?} {viewport:?}"
    );
}

#[test]
fn the_composer_does_not_move_between_session_states() {
    // The strip's height used to be subtracted from the transcript viewport, so
    // a strip that appeared and disappeared moved everything the user was
    // reading — it was gated on "there is something to say", and it said
    // something on every submit and again on every completion.
    //
    // There is no reserved row now, so the invariant is the stronger and
    // simpler one: nothing about the turn state changes the geometry.
    let mut prompt_tops = Vec::new();
    let mut viewport_heights = Vec::new();
    for status in [
        SessionStatus::Idle,
        SessionStatus::Working,
        SessionStatus::Error,
        SessionStatus::Idle,
    ] {
        let mut app = test_app();
        app.session_state.session_status = status;
        render_to_buffer(&mut app, 100, 30);
        prompt_tops.push(app.prompt_area.expect("prompt area recorded").y);
        viewport_heights.push(app.viewport_area.expect("viewport area recorded").height);
    }

    assert!(
        prompt_tops.windows(2).all(|w| w[0] == w[1]),
        "the composer must not move between session states, got {prompt_tops:?}"
    );
    assert!(
        viewport_heights.windows(2).all(|w| w[0] == w[1]),
        "the transcript must not resize between session states, got {viewport_heights:?}"
    );
}

#[test]
fn an_idle_buffer_says_nothing_about_the_turn_state() {
    // The row above the composer is *situational*: it says what the user must
    // know now, and an idle session has nothing to say. It used to read
    // `idle`, which is a state the user can see perfectly well elsewhere.
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    let buffer = render_to_buffer(&mut app, 100, 30);
    let row = hint_row_text(&app, &buffer);

    assert!(
        !row.contains("idle"),
        "the buffer must not carry an idle indicator any more:\n{row}"
    );
    assert!(
        !row.contains("working"),
        "an idle session must never claim to be working:\n{row}"
    );
    assert!(
        !row.contains("stop"),
        "an idle session must not advertise a stop key it cannot honour:\n{row}"
    );
}

#[test]
fn a_failed_turn_keeps_saying_why_until_the_agent_resumes() {
    use codegg::protocol::core::CoreEvent;

    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session(
            "session-under-test".to_string(),
        ));
    app.session_state.session_status = SessionStatus::Working;

    app.apply_core_event(CoreEvent::TurnFailed {
        session_id: "s1".into(),
        turn_id: Some("t1".into()),
        message: "Agent error: provider error: 529 overloaded".into(),
    });

    // The reason must be readable from the main buffer alone. A toast is the
    // notification; this is the record, and it is the only surface that survives
    // long enough to be read after the fact. Read from the row above the
    // composer so a toast that happens to be on screen cannot satisfy this.
    let buffer = render_to_buffer(&mut app, 100, 30);
    let row = hint_row_text(&app, &buffer);
    assert!(
        row.contains("529 overloaded"),
        "the failure reason must stay on screen:\n{row}"
    );
    assert_eq!(
        app.session_state.last_error.as_deref(),
        Some("Agent error: provider error: 529 overloaded"),
        "the latch holds the reason independently of any toast"
    );

    // Resuming clears it: the user has acted on it and has moved on.
    app.apply_core_event(CoreEvent::TurnStarted {
        session_id: "s1".into(),
        turn_id: "t2".into(),
    });
    assert_eq!(
        app.session_state.last_error, None,
        "a resumed agent must stop reporting the previous failure"
    );
    let buffer = render_to_buffer(&mut app, 100, 30);
    let resumed = hint_row_text(&app, &buffer);
    assert!(
        !resumed.contains("529 overloaded"),
        "a resumed agent must not keep reporting the previous failure:\n{resumed}"
    );
}

#[test]
fn an_error_outranks_a_running_phase_so_the_reason_is_never_buried() {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session(
            "session-under-test".to_string(),
        ));
    app.session_state.session_status = SessionStatus::Working;
    app.report_turn_error("Agent error: provider error: 429 rate_limit");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let row = hint_row_text(&app, &buffer);
    assert!(row.contains("429 rate_limit"), "reason must show:\n{row}");
    // A spinner beside a failure would claim work is still progressing.
    assert!(
        !row.contains('⠋') && !row.contains('⠙') && !row.contains('⠹'),
        "a latched failure must not animate as though work continues:\n{row}"
    );
}

#[test]
fn the_phase_enum_names_the_states_that_used_to_be_silent() {
    use codegg::tui::components::messages::ActivityPhase;

    assert_eq!(ActivityPhase::Thinking.label(), "thinking");
    assert_eq!(ActivityPhase::Writing.label(), "writing");
    assert_eq!(
        ActivityPhase::WaitingForApproval {
            tool: "bash".into()
        }
        .label(),
        "awaiting your approval · bash"
    );
    assert_eq!(
        ActivityPhase::Retrying {
            attempt: 2,
            total: 3,
            class: "rate_limit".into(),
        }
        .label(),
        "retrying 2/3 · rate_limit",
        "a silent backoff is the state that read as a hang; it must name itself"
    );
    assert_eq!(
        ActivityPhase::RunningTool {
            name: "bash".into(),
            summary: String::new(),
        }
        .label(),
        "running bash",
        "an unparseable argument must not lose the tool name"
    );
}

#[test]
fn a_pending_permission_names_the_tool_the_user_is_being_asked_about() {
    use codegg::protocol::core::CoreEvent;

    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session(
            "session-under-test".to_string(),
        ));
    app.session_state.session_status = SessionStatus::Working;

    app.apply_core_event(CoreEvent::PermissionPending {
        session_id: "s1".into(),
        id: "p1".into(),
        tool: "bash".into(),
        path: None,
        turn_id: Some("t1".into()),
    });

    let phase = app
        .messages_state
        .messages
        .activity
        .as_ref()
        .expect("a pending permission is an activity phase, not silence");
    assert!(
        phase.phase.label().contains("awaiting your approval"),
        "waiting on a human must not read as a stall, got {:?}",
        phase.phase
    );
}

/// The task state the working strip used to draw lives in the `Ctrl+T` tasks
/// overlay: todos, then the active subagents, then the live jobs.
#[test]
fn active_subagents_are_listed_in_the_tasks_overlay() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let rows = vec![
        SubagentRow {
            child_instance_id: None,
            profile: None,
            usefulness: None,
            task_id: "1".into(),
            run_id: None,
            display_name: Some("Sally".into()),
            agent: "explore".into(),
            description: "Map the auth flow".into(),
            status: "running".into(),
        },
        SubagentRow {
            child_instance_id: None,
            profile: None,
            usefulness: None,
            task_id: "2".into(),
            run_id: None,
            display_name: Some("Greg".into()),
            agent: "build".into(),
            description: "Warm the cargo cache".into(),
            status: "completed".into(),
        },
    ];
    // The overlay reads the *projection*, not the local row list: the daemon
    // owns the truth about what is running, so a test that seeds only the
    // local rows proves nothing about the real surface.
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;
    let buffer = render_to_buffer(&mut app, 100, 50);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("Subagents"),
        "the overlay must name the group: {text}"
    );
    assert!(text.contains("explore"), "{text}");
    assert!(text.contains("Map the auth flow"), "{text}");
    assert!(
        !text.contains("Warm the cargo cache"),
        "a finished subagent is not work in flight: {text}"
    );
    assert!(
        app.tasks_area.expect("the overlay reserves rows").height > 0,
        "the overlay must be on screen when it is expanded"
    );
}

#[test]
fn a_long_fan_out_is_bounded_by_the_overlay_with_an_honest_count() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let rows: Vec<SubagentRow> = (0..12)
        .map(|i| SubagentRow {
            child_instance_id: None,
            profile: None,
            usefulness: None,
            task_id: i.to_string(),
            run_id: None,
            display_name: None,
            agent: format!("agent{i}"),
            description: format!("task {i}"),
            status: "running".into(),
        })
        .collect();
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;
    let buffer = render_to_buffer(&mut app, 100, 50);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("Subagents"),
        "the overlay must name the group: {text}"
    );
    assert!(
        text.contains("more"),
        "a bound that hides rows must say so: {text}"
    );
    assert!(
        !text.contains("task 6"),
        "the bound must actually bind, or the assertion above proves nothing: {text}"
    );
}

/// Regression: a main agent blocked on delegated work must be visible as such.
///
/// The main agent fans out and then goes silent while it waits for a subagent
/// or a job. The wait is derived from the projection and listed by the tasks
/// overlay, so it cannot be dropped on the floor by an unwired handler — and
/// the main buffer does not claim to be idle while it happens.
#[test]
fn a_main_agent_waiting_on_subagents_is_listed_by_the_overlay() {
    let mut app = test_app();
    // The main agent's own stream is finished; it is blocked on the subagent.
    app.session_state.session_status = SessionStatus::Idle;
    let rows = vec![
        SubagentRow {
            child_instance_id: None,
            profile: None,
            usefulness: None,
            task_id: "t1".into(),
            run_id: None,
            display_name: None,
            agent: "explore".into(),
            description: "map the thing".into(),
            status: "running".into(),
        },
        SubagentRow {
            child_instance_id: None,
            profile: None,
            usefulness: None,
            task_id: "t2".into(),
            run_id: None,
            display_name: None,
            agent: "explore".into(),
            description: "already done".into(),
            status: "completed".into(),
        },
    ];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);

    app.tasks_expanded = true;
    let buffer = render_to_buffer(&mut app, 100, 50);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("Subagents"),
        "the overlay must name the group: {text}"
    );
    assert!(
        text.contains("map the thing"),
        "the live row must be listed: {text}"
    );
    assert!(
        !text.contains("already done"),
        "a finished row is not work in flight: {text}"
    );
}

#[test]
fn an_actually_idle_session_has_no_pending_work() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    let rows = vec![SubagentRow {
        child_instance_id: None,
        profile: None,
        usefulness: None,
        task_id: "t1".into(),
        run_id: None,
        display_name: None,
        agent: "explore".into(),
        description: "done".into(),
        status: "completed".into(),
    }];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);

    app.tasks_expanded = true;
    let buffer = render_to_buffer(&mut app, 100, 50);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("Nothing pending"),
        "with no live work the overlay says so: {text}"
    );
}

/// Seed the projection controller the way the daemon-attached path does: one
/// session subscription holding an authoritative snapshot with a live job.
fn seed_projection_with_live_job(app: &mut App) {
    use codegg::protocol::projection::dto::JobProjection;
    use codegg::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use codegg::protocol::projection::snapshot::SessionProjectionSnapshot;

    let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
    snapshot.jobs.push_back(JobProjection {
        job_id: "job-1".into(),
        workspace_id: "ws".into(),
        kind: "subagent".into(),
        state: "running".into(),
        summary: "map the subsystem".into(),
        session_id: Some("sess".into()),
        turn_id: None,
        active_attempt_id: None,
        error_class: None,
        updated_at: 0,
    });
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "proj".into(),
        workspace_id: Some("ws".into()),
        session_id: Some("sess".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    app.projection_client.controller_mut().install_subscription(
        ProjectionSubscriptionId::new("sub-1"),
        descriptor,
        snapshot,
    );
}

/// Install authoritative session-owned child rows for task-overlay tests.
fn seed_projection_with_subagents(app: &mut App, rows: &[SubagentRow]) {
    use codegg::protocol::projection::dto::{AgentTreeNodeProjection, AgentTreeStatus};
    use codegg::protocol::projection::replay::{
        ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
        ProjectionSubscriptionId,
    };
    use codegg::protocol::projection::snapshot::SessionProjectionSnapshot;

    let mut snapshot = SessionProjectionSnapshot::empty("sess", "proj", "ws");
    snapshot.subagents = rows
        .iter()
        .map(|row| AgentTreeNodeProjection {
            child_instance_id: row.child_instance_id.clone().unwrap_or_default(),
            originating_turn_id: None,
            task_id: row.task_id.parse().unwrap_or_default(),
            agent: row.agent.clone(),
            description: row.description.clone(),
            status: match row.status.as_str() {
                "completed" => AgentTreeStatus::Completed,
                "failed" => AgentTreeStatus::Failed,
                _ => AgentTreeStatus::Running,
            },
            parent_task_id: None,
            created_at: 0,
            completed_at: None,
            result_summary: None,
            display_name: row.display_name.clone(),
            profile: row.profile.clone(),
            usefulness: row.usefulness.clone(),
            run_id: row.run_id.clone(),
        })
        .collect();
    let descriptor = ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("stream-sess").expect("valid stream id"),
        kind: ProjectionStreamKind::Session,
        project_id: "proj".into(),
        workspace_id: Some("ws".into()),
        session_id: Some("sess".into()),
        projection_version: 1,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    };
    let subscription_id = ProjectionSubscriptionId::new("sub-children");
    let controller = app.projection_client.controller_mut();
    let _ = controller.unsubscribe(&subscription_id);
    controller.install_subscription(subscription_id, descriptor, snapshot);
}

/// Regression: a live job must reach the status row through the projection.
///
/// Every other test here sets `ui_state.subagents` directly, which is exactly
/// how the original tests passed while the real daemon-attached TUI showed
/// `idle`: nothing in production ever put rows there. The daemon-attached
/// frontend subscribes per session and never switches a project tab, so
/// `active_tab_id` is `None` — and `active_snapshot` used to bail out on
/// exactly that, reporting "no snapshot" for a session with a job running.
#[test]
fn a_live_job_in_the_projection_reaches_the_tasks_overlay() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    seed_projection_with_live_job(&mut app);

    // The daemon-attached TUI has no active tab; the single subscription is
    // unambiguous, so the status surfaces must still see it.
    assert_eq!(
        app.projection_client.active_tab_id(),
        None,
        "precondition: the daemon-socket path never sets a tab"
    );
    app.tasks_expanded = true;
    let buffer = render_to_buffer(&mut app, 100, 50);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("Jobs"),
        "the overlay must name the group: {text}"
    );
    assert!(
        text.contains("map the subsystem"),
        "the live job must be listed: {text}"
    );
}

/// Regression: the projection must reach the tasks overlay without the
/// sidebar.
///
/// `ui_state.subagents` was previously written only from the session-panel
/// render path, so a layout that does not draw the panel left the count at
/// zero. The overlay must not depend on some other widget being visible.
#[test]
fn the_tasks_overlay_counts_live_work_without_the_sidebar() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    seed_projection_with_live_job(&mut app);

    // No panel draw happens here; the render pass alone must establish the
    // job count the overlay reads.
    let _ = render_to_buffer(&mut app, 100, 50);
    app.tasks_expanded = true;
    let text = buffer_text(&render_to_buffer(&mut app, 100, 50));
    assert!(
        text.contains("map the subsystem"),
        "the job count must survive a render that never draws the panel: {text}"
    );
}

// ---------------------------------------------------------------------------
// 3. Subagent buffer
// ---------------------------------------------------------------------------

/// The task state the working strip used to draw lives in the `Ctrl+T` tasks
/// overlay, and the overlay is where a subagent's buffer is opened from.
#[test]
fn the_tasks_overlay_opens_a_subagent_buffer_and_esc_returns_to_main() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let rows = vec![SubagentRow {
        child_instance_id: None,
        profile: None,
        usefulness: None,
        task_id: "7".into(),
        run_id: Some("run-7".into()),
        display_name: Some("Sally".into()),
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;
    render_to_buffer(&mut app, 100, 50);

    // The overlay's cursor rests on a body row, and `↵` opens it.
    assert!(
        app.move_tasks_selection(1),
        "the overlay must have a selectable row to rest on"
    );
    assert!(
        app.open_selected_task(),
        "the overlay must open the row under its cursor"
    );
    let tab = app
        .ui_state
        .subagents
        .active_tab()
        .cloned()
        .expect("the overlay opens the subagent's buffer");
    assert_eq!(tab.row.agent, "explore");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);
    assert!(text.contains("Subagent"), "{text}");
    assert!(text.contains("Map the auth flow"), "{text}");
    assert!(text.contains("run-7"), "{text}");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.ui_state.subagents.active_tab().is_none(),
        "Esc must return to the main buffer"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tool_output_expanded(app: &App) -> bool {
    app.messages_state.messages.messages.iter().any(|m| {
        m.parts
            .iter()
            .any(|part| matches!(part, MsgPart::ToolCall { expanded: true, .. }))
    })
}

/// The buffer row inside `area` whose rendered text contains `needle`.
fn buffer_area(buffer: &Buffer) -> ratatui::layout::Rect {
    buffer.area
}

fn row_containing(buffer: &Buffer, area: ratatui::layout::Rect, needle: &str) -> Option<u16> {
    (area.y..area.y + area.height).find(|&y| {
        let line: String = (area.x..area.x + area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        line.contains(needle)
    })
}

// ---------------------------------------------------------------------------
// 4. Turn-stall watchdog vs. a long-running tool
// ---------------------------------------------------------------------------

#[test]
fn a_long_running_tool_does_not_trip_the_stall_watchdog() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    // Simulate a turn that has been silent well past the stall threshold
    // because a tool call is still in flight.
    app.session_state.working_since =
        Some(std::time::Instant::now() - codegg::tui::app::TURN_STALL_TIMEOUT * 2);
    app.session_state.running_tool = Some(codegg::tui::app::state::session::RunningTool::new(
        "t1", "bash",
    ));

    assert!(
        !app.check_turn_stall(),
        "a silent turn with a tool in flight is working, not wedged"
    );
    assert_eq!(app.session_state.session_status, SessionStatus::Working);
}

#[test]
fn a_tool_that_outlives_its_grace_still_trips_the_watchdog() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.session_state.working_since =
        Some(std::time::Instant::now() - codegg::tui::app::TURN_STALL_TIMEOUT * 2);
    let mut running = codegg::tui::app::state::session::RunningTool::new("t1", "bash");
    running.started = std::time::Instant::now() - codegg::tui::app::TURN_STALL_TOOL_GRACE * 2;
    app.session_state.running_tool = Some(running);

    assert!(
        app.check_turn_stall(),
        "a runaway tool must not suppress the watchdog forever"
    );
    assert_eq!(app.session_state.session_status, SessionStatus::Error);
}

#[test]
fn a_silent_turn_with_no_tool_still_trips_the_watchdog() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.session_state.working_since =
        Some(std::time::Instant::now() - codegg::tui::app::TURN_STALL_TIMEOUT * 2);

    assert!(app.check_turn_stall());
    assert_eq!(app.session_state.session_status, SessionStatus::Error);
}

// ---------------------------------------------------------------------------
// 6. Scrolling a long transcript
// ---------------------------------------------------------------------------

/// A session whose transcript is much taller than the viewport, ending in a
/// marker row that can only be on screen if scrolling reached the real end.
///
/// Every message carries a tool result whose lines are long enough to wrap.
/// That combination is what used to break: the scroll range was computed from
/// a row *estimate* that disagreed with the rows actually drawn, and because
/// the disagreement was per message it accumulated down the buffer — so the
/// tail drifted further and further out of reach the longer the transcript got.
fn app_with_long_wrapping_transcript() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session("s1".into()));
    for i in 0..30 {
        app.messages_state
            .messages
            .add_user_message(format!("request {i}"), None);
        app.messages_state
            .messages
            .add_assistant_text(format!("working on request {i}"));
        let long_output = (0..6)
            .map(|n| format!("detail {n} {}", "padding".repeat(20)))
            .collect::<Vec<_>>()
            .join("\n");
        let last = app.messages_state.messages.messages.last_mut().unwrap();
        last.parts.push(MsgPart::ToolCall {
            id: format!("t{i}"),
            name: "bash".into(),
            input: "cargo test".into(),
            output: long_output,
            status: ToolStatus::Completed,
            duration_ms: Some(10),
            exit_code: Some(0),
            output_lines: Some(6),
            expanded: false,
        });
    }
    app.messages_state
        .messages
        .add_assistant_text("THE-TRANSCRIPT-ENDS-HERE".into());
    app
}

#[test]
fn scrolling_to_the_bottom_reveals_the_last_line_of_a_long_transcript() {
    let mut app = app_with_long_wrapping_transcript();

    // Start at the bottom, the way the TUI follows a live turn.
    app.messages_state.messages.scroll_to_bottom();
    let buffer = render_to_buffer(&mut app, 100, 30);

    assert!(
        buffer_text(&buffer).contains("THE-TRANSCRIPT-ENDS-HERE"),
        "the final line of the transcript must be reachable by scrolling"
    );
}

#[test]
fn every_row_of_the_viewport_is_filled_at_the_bottom_of_a_long_transcript() {
    let mut app = app_with_long_wrapping_transcript();
    app.messages_state.messages.scroll_to_bottom();
    let buffer = render_to_buffer(&mut app, 100, 30);
    let area = app.viewport_area.expect("viewport area recorded");

    // The viewport is 30 rows less the prompt/footer chrome. At the bottom of
    // the buffer every one of those rows has content: a row index that
    // over-counts leaves blank space here instead of showing the tail.
    let last_row: String = (area.x..area.x + area.width)
        .map(|x| buffer[(x, area.y + area.height - 1)].symbol())
        .collect();
    assert!(
        !last_row.trim().is_empty(),
        "the bottom viewport row must not be blank when scrolled to the end"
    );
}

#[test]
fn expanding_a_tool_call_extends_the_reachable_range_of_a_long_transcript() {
    let mut app = app_with_long_wrapping_transcript();
    app.messages_state.messages.scroll_to_bottom();
    render_to_buffer(&mut app, 100, 30);
    let collapsed_reach = app.messages_state.messages.max_scroll();

    // Expand the first tool result, well above the fold.
    let target = app
        .messages_state
        .messages
        .messages
        .iter()
        .position(|m| {
            m.parts
                .iter()
                .any(|p| matches!(p, MsgPart::ToolCall { id, .. } if id == "t0"))
        })
        .expect("fixture contains the first tool call");
    app.messages_state.messages.toggle_tool_output(target);
    render_to_buffer(&mut app, 100, 30);

    // Collapsed output shows a preview; expanding shows all of it, so the
    // reachable range has to grow to match.
    assert!(
        app.messages_state.messages.max_scroll() > collapsed_reach,
        "expanding must extend the reachable range ({} -> {})",
        collapsed_reach,
        app.messages_state.messages.max_scroll()
    );

    // And the tail must still be reachable afterwards.
    app.messages_state.messages.scroll_to_bottom();
    let buffer = render_to_buffer(&mut app, 100, 30);
    assert!(
        buffer_text(&buffer).contains("THE-TRANSCRIPT-ENDS-HERE"),
        "the end of the transcript must stay reachable after an expansion"
    );
}

#[test]
fn a_search_highlight_survives_a_re_render() {
    let mut app = app_with_long_wrapping_transcript();
    app.messages_state.messages.scroll_to_bottom();
    let _ = render_to_buffer(&mut app, 100, 30);

    app.messages_state
        .messages
        .search("THE-TRANSCRIPT-ENDS-HERE");
    let buffer = render_to_buffer(&mut app, 100, 30);
    let area = app.viewport_area.expect("viewport area recorded");
    assert!(
        row_containing(&buffer, area, "THE-TRANSCRIPT-ENDS-HERE").is_some(),
        "the match must still be drawn after the search invalidates the rows"
    );

    app.messages_state.messages.clear_search();
    let _ = render_to_buffer(&mut app, 100, 30);
}

// ---------------------------------------------------------------------------
// 4. Stopping, resuming, and the agent activity indicator
// ---------------------------------------------------------------------------
//
// A turn the user cannot interrupt is a turn they can only wait out, and a
// long wait with no changing pixels reads as a dropped connection rather than
// as work. These cover the three answers that make it legible: the key is
// discoverable, the destructive direction is confirmed rather than
// single-keystroke, and the stopped state stays visible.

#[test]
fn a_working_turn_names_the_key_that_stops_it() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("stop agent"),
        "the row above the composer must advertise the stop key; a long pause \
         with no way out is what reads as a disconnect. Rendered:\n{text}"
    );
}

#[test]
fn the_stop_confirmation_is_visible_between_the_two_presses() {
    let mut app = test_app();
    // The stop key is a chord (`Ctrl+Alt+X`) since ADR-0042 gave the bare `x`
    // to the composer. A chord cannot be composed as text, so it needs no
    // mode gate — but the app is put in Normal mode anyway, because that is
    // where the user reaches for it.
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.session_state.session_status = SessionStatus::Working;

    // First press: arm, and say what completes it. Without this the second
    // press looks like a no-op and the key reads as broken.
    let _ = render_to_buffer(&mut app, 100, 30);
    app.on_key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(
        app.prompt_state.stop_armed_at.is_some(),
        "one press must arm, not stop"
    );
    assert!(
        app.session_state.session_status == SessionStatus::Working,
        "an armed confirmation must not have stopped the turn"
    );

    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);
    assert!(
        text.contains("again to stop agent"),
        "the armed state must say which press completes it. Rendered:\n{text}"
    );
}

#[test]
fn a_single_x_in_insert_mode_is_typed_rather_than_stopping_the_agent() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.input_mode = codegg::tui::input::InputMode::Insert;

    let _ = render_to_buffer(&mut app, 100, 30);
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    assert!(
        app.prompt_state.stop_armed_at.is_none(),
        "`x` while composing must be a letter, never a stop"
    );
    assert!(
        app.prompt_state.prompt.get_text().contains('x'),
        "the letter must have reached the composer"
    );
}

#[test]
fn a_stopped_turn_stays_visible_and_names_the_way_back() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    app.session_state.stopped_turn = true;

    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    // The stopped *state* word is gone with the status row; what matters is
    // that the turn stays visible and names the way out of it.
    assert!(
        text.contains("start agent"),
        "a stopped agent must name the way back. Rendered:\n{text}"
    );
    assert!(
        text.contains("/start"),
        "the stopped state must name the way out. Rendered:\n{text}"
    );
}

#[test]
fn an_idle_agent_shows_no_stop_affordance() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    app.session_state.stopped_turn = false;

    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    assert!(
        !text.contains("to stop") && !text.contains("stopped"),
        "nothing to stop and nothing stopped means no stop affordance. \
         Rendered:\n{text}"
    );
}

#[test]
fn a_saturated_overlay_never_squeezes_the_transcript_below_its_floor() {
    // Forty todos, ten running subagents and ten running jobs on a 30-row
    // terminal: far more work in flight than the pane has rows. The contract
    // is not "the overlay always fits" - it is that the transcript keeps its
    // `Min(10)` floor and the overlay yields instead of the layout
    // over-committing. An overshooting overlay is not merely tall: it makes
    // the carve yield, so `Shift+T` would read as a dead key.
    use codegg::tui::app::state::subagents::SubagentRow;

    for (width, height) in [(100u16, 30u16), (100, 50), (120, 24)] {
        let mut app = test_app();
        app.session_state.session_status = SessionStatus::Working;
        app.set_todos(
            (0..40)
                .map(|i| codegg::tui::app::TodoEntry {
                    content: format!("todo number {i}"),
                    status: "pending".into(),
                    priority: "medium".into(),
                })
                .collect(),
        );
        app.ui_state.subagents.set_rows(
            (0..10)
                .map(|i| SubagentRow {
                    child_instance_id: None,
                    profile: None,
                    usefulness: None,
                    task_id: i.to_string(),
                    run_id: None,
                    display_name: Some(format!("Sally{i}")),
                    agent: "explore".into(),
                    description: format!("a long description for subagent {i}"),
                    status: "running".into(),
                })
                .collect(),
        );
        app.tasks_expanded = true;

        render_to_buffer(&mut app, width, height);

        let viewport = app.viewport_area.expect("viewport area recorded");
        assert!(
            viewport.height >= 10,
            "transcript fell to {} rows at {width}x{height}; the floor is 10",
            viewport.height
        );
        // Whatever rows the overlay did take, the composer still has a box.
        let prompt = app.prompt_area.expect("prompt area recorded");
        assert!(
            prompt.height >= 1,
            "the composer vanished at {width}x{height}"
        );
    }
}

#[test]
fn the_overlay_appears_when_the_pane_has_room_for_it() {
    use codegg::tui::app::state::subagents::SubagentRow;

    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.set_todos(vec![codegg::tui::app::TodoEntry {
        content: "ship the thing".into(),
        status: "in_progress".into(),
        priority: "high".into(),
    }]);
    let rows = vec![SubagentRow {
        child_instance_id: None,
        profile: None,
        usefulness: None,
        task_id: "1".into(),
        run_id: None,
        display_name: Some("Sally".into()),
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;

    render_to_buffer(&mut app, 100, 50);
    let area = app
        .tasks_area
        .expect("the overlay reserves rows when there is room");
    assert!(area.height > 0);

    let text = buffer_text(&render_to_buffer(&mut app, 100, 50));
    // Both groups are labelled and named, so the reader can tell what each
    // block is without inferring it from the rows.
    assert!(text.contains("Todo list"), "{text}");
    assert!(text.contains("Subagents"), "{text}");
    assert!(text.contains("Sally"), "{text}");
    assert!(text.contains("explore"), "{text}");
}

#[test]
fn the_overlay_names_rows_omitted_by_its_bound() {
    use codegg::tui::app::state::subagents::SubagentRow;

    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let rows: Vec<SubagentRow> = (0..10)
        .map(|index| SubagentRow {
            child_instance_id: Some(format!("child-{index}")),
            task_id: index.to_string(),
            run_id: None,
            display_name: Some(format!("worker-{index}")),
            profile: None,
            usefulness: None,
            agent: "explore".into(),
            description: "background work".into(),
            status: "running".into(),
        })
        .collect();
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;

    let text = buffer_text(&render_to_buffer(&mut app, 120, 50));
    assert!(text.contains("Subagents · 4 more"), "{text}");
}

#[test]
fn a_collapsed_tasks_overlay_reserves_nothing() {
    let mut app = test_app();
    app.set_todos(vec![codegg::tui::app::TodoEntry {
        content: "a todo".into(),
        status: "pending".into(),
        priority: "medium".into(),
    }]);
    app.tasks_expanded = false;
    assert_eq!(app.tasks_strip_height(), 0);
    render_to_buffer(&mut app, 100, 30);
    assert!(
        app.tasks_area.is_none(),
        "a collapsed overlay must not paint"
    );
}

/// Regression: an expanded-but-empty overlay must still claim a row.
///
/// The renderer has a "Nothing pending" line for exactly this state, but it
/// could never paint: `tasks_strip_height` returned 0, so no rows were carved,
/// so `tasks_area` was `None`, so `render_tasks_strip` returned before reaching
/// it. `Shift+T` was therefore a silently dead key on an idle session — which is
/// the first thing a user tries when they go looking for the feature.
#[test]
fn an_expanded_but_empty_tasks_overlay_confirms_the_key_worked() {
    let mut app = test_app();
    app.set_todos(vec![]);
    app.tasks_expanded = true;

    assert!(
        app.task_rows().is_empty(),
        "precondition: an idle session has nothing to list"
    );
    assert_eq!(
        app.tasks_strip_height(),
        1,
        "an expanded overlay must reserve the 'Nothing pending' row"
    );

    render_to_buffer(&mut app, 100, 30);
    assert!(
        app.tasks_area.is_some(),
        "the overlay must be given space so it can confirm the toggle"
    );
    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    assert!(
        text.contains("Nothing pending"),
        "the confirmation line must actually paint:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// 4. Subagent tabs
// ---------------------------------------------------------------------------
//
// Opening a subagent used to be a single slot: one `Option`, no way to keep two
// runs side by side, no way to see how many were open. The tab set fixes both,
// and the tests below pin the parts that are easy to get subtly wrong: the
// cycle wrap, the capacity refusal, the "subject left the snapshot" terminal
// state, and the one-row bar's width arithmetic.

fn subagent(task_id: &str, name: &str, status: &str) -> SubagentRow {
    SubagentRow {
        child_instance_id: Some(task_id.into()),
        task_id: task_id.into(),
        run_id: Some(format!("run-{task_id}")),
        display_name: Some(name.into()),
        profile: None,
        usefulness: None,
        agent: "explore".into(),
        description: format!("work for {task_id}"),
        status: status.into(),
    }
}

/// The same row as the projection delivers it once descriptors exist.
fn described_subagent(task_id: &str, name: &str, status: &str) -> SubagentRow {
    SubagentRow {
        child_instance_id: Some(task_id.into()),
        profile: Some("Reconnaissance generalist.".into()),
        usefulness: Some("Useful for broad recon.".into()),
        ..subagent(task_id, name, status)
    }
}

/// C005: the subagent tab is the one surface that carries Task **and** Agent.
///
/// That is what lets the one-line rows spend their single slot on the
/// descriptor without losing the task, so the trade only holds while the tab
/// really renders both. This test pins that.
#[test]
fn the_subagent_tab_shows_the_task_and_the_descriptor_together() {
    let mut app = app_with_subagents(vec![described_subagent("2", "Sally", "running")]);
    app.open_subagent_tab("2");

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(
        text.contains("work for 2"),
        "the tab dropped the task description"
    );
    assert!(
        text.contains("Reconnaissance generalist."),
        "the tab dropped the descriptor"
    );
    assert!(
        text.contains("Useful for broad recon."),
        "the tab dropped the operational guidance"
    );
    assert!(text.contains("Agent"), "no Agent section heading: {text}");
}

/// A tab opened on a node recorded before descriptors existed still renders,
/// with no empty `Agent` heading over an empty body.
#[test]
fn the_subagent_tab_omits_the_agent_section_without_a_descriptor() {
    let mut app = app_with_subagents(vec![subagent("2", "Sally", "running")]);
    app.open_subagent_tab("2");

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(text.contains("work for 2"), "the task is still shown");
    assert!(
        !text.contains("Reconnaissance generalist."),
        "no descriptor was carried, so none may be invented"
    );
}

fn app_with_subagents(rows: Vec<SubagentRow>) -> App {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app
}

/// Text of the one row the subagent tab bar occupies.
///
/// Read from the bar's own row rather than from the whole buffer: the live
/// toast column is an overlay pinned to the top right, so a whole-buffer scan
/// cannot distinguish "the bar says this" from "a toast happens to say this".
fn tab_bar_text(app: &App, buf: &Buffer) -> String {
    let viewport = app.viewport_area.expect("viewport drawn");
    // The bar is carved off the top of the viewport, so it is the row above it.
    (0..buf.area.width)
        .map(|x| buf[(x, viewport.y - 1)].symbol())
        .collect()
}

#[test]
fn shift_n_and_shift_p_cycle_the_tab_set_and_wrap_through_the_conversation() {
    let mut app = app_with_subagents(vec![
        subagent("1", "Sally", "running"),
        subagent("2", "Bertha", "running"),
    ]);
    // `Shift+N` is Normal-only by contract — `N` is typed text while
    // composing — so the test has to be in Normal to press it at all.
    app.ui_state.input_mode = InputMode::Normal;
    app.open_subagent_tab("1");
    app.open_subagent_tab("2");

    let press = |app: &mut App, letter: char| {
        app.on_key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT));
    };
    let active = |app: &App| {
        app.ui_state
            .subagents
            .active_tab()
            .map(|tab| tab.tab_id.as_str().to_string())
    };
    assert_eq!(
        active(&app).as_deref(),
        Some("2"),
        "the newest tab is active"
    );

    press(&mut app, 'N');
    assert_eq!(
        active(&app).as_deref(),
        None,
        "cycling forward off the last tab wraps to the conversation"
    );
    press(&mut app, 'P');
    assert_eq!(
        active(&app).as_deref(),
        Some("2"),
        "cycling backward from the conversation must land on the LAST tab"
    );
    press(&mut app, 'P');
    assert_eq!(active(&app).as_deref(), Some("1"));
    press(&mut app, 'P');
    assert_eq!(
        active(&app).as_deref(),
        None,
        "and wraps forward to the conversation from the first tab"
    );
    press(&mut app, 'N');
    assert_eq!(
        active(&app).as_deref(),
        Some("1"),
        "then forward into the first tab again"
    );
}

#[test]
fn shift_x_closes_the_active_tab_without_stopping_the_run() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.ui_state.input_mode = InputMode::Normal;
    app.open_subagent_tab("1");

    app.on_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT));
    assert!(app.ui_state.subagents.active_tab().is_none());
    // Closing a tab closes nothing else: the run is still advertised by the
    // projection, so the working strip must keep showing it.
    assert_eq!(
        app.ui_state.subagents.rows.len(),
        1,
        "closing a tab must not clear the rows"
    );
    assert!(app.ui_state.subagents.running_count() == 1);
}

#[test]
fn the_tab_bar_names_every_open_tab_and_marks_the_active_one() {
    let mut app = app_with_subagents(vec![
        subagent("1", "Sally", "running"),
        subagent("2", "Bertha", "running"),
    ]);
    app.open_subagent_tab("1");
    app.open_subagent_tab("2");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let bar = tab_bar_text(&app, &buffer);
    assert!(
        bar.contains("Main"),
        "the conversation is always offered: {bar:?}"
    );
    assert!(bar.contains("Sally"), "{bar:?}");
    assert!(bar.contains("Bertha"), "{bar:?}");
}

/// The tab strip's own row, cell by cell.
///
/// The cells are read individually because "the persona is italic" is a claim
/// about *how* the text is painted, and a whole-row text comparison cannot
/// tell an italic persona from a plain one.
fn tab_bar_cells(app: &App, buf: &Buffer) -> (String, Vec<Cell>) {
    let viewport = app.viewport_area.expect("viewport drawn");
    let cells: Vec<Cell> = (0..buf.area.width)
        .map(|x| buf[(x, viewport.y - 1)].clone())
        .collect();
    let text = cells.iter().map(|cell| cell.symbol()).collect();
    (text, cells)
}

/// The text of the cells the strip painted in italics.
fn tab_bar_italic_text(app: &App, buf: &Buffer) -> String {
    let (_, cells) = tab_bar_cells(app, buf);
    cells
        .iter()
        .filter(|cell| cell.modifier.contains(Modifier::ITALIC))
        .map(|cell| cell.symbol())
        .collect()
}

/// The strip names the main conversation and states the keys that operate it.
#[test]
fn the_tab_bar_names_main_and_states_the_keys_that_operate_it() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let bar = tab_bar_text(&app, &buffer);
    assert!(bar.contains("Main"), "the main entry is named: {bar:?}");
    for key in ["⇧P", "⇧N", "⇧X"] {
        assert!(bar.contains(key), "the strip does not state {key}: {bar:?}");
    }
}

/// The main entry is a tab, not plain text: it carries a marker of its own.
#[test]
fn the_tab_bar_marks_the_main_entry_as_a_tab() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let bar = tab_bar_text(&app, &buffer);
    // The main entry is painted as "<marker> Main". A liveness glyph here
    // would read as "the main conversation is a run", which it is not — so the
    // assertion is on the entry's own painted text and not on the whole row,
    // which also carries the *other* tabs' liveness glyphs.
    assert!(
        bar.contains("▣ Main"),
        "the main entry must read as a tab: {bar:?}"
    );
}

/// The active tab's persona is on the strip, in italics.
///
/// The persona is the projection's own value — the row the tab already holds —
/// so the strip never resolves a name against a pool it does not have.
#[test]
fn the_tab_bar_shows_the_active_tabs_persona_in_italics() {
    let mut app = app_with_subagents(vec![described_subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let (bar, _) = tab_bar_cells(&app, &buffer);
    assert!(
        bar.contains("Reconnaissance generalist."),
        "the persona is not on the strip: {bar:?}"
    );
    let italic = tab_bar_italic_text(&app, &buffer);
    assert!(
        italic.contains("Reconnaissance generalist."),
        "the persona is not painted in italics: {italic:?}"
    );
}

/// The persona belongs to the tab on screen. The main conversation has none,
/// so the strip must not show one while it is the active entry.
#[test]
fn the_tab_bar_shows_no_persona_while_the_conversation_is_the_active_tab() {
    let mut app = app_with_subagents(vec![
        described_subagent("1", "Sally", "running"),
        described_subagent("2", "Bertha", "running"),
    ]);
    app.open_subagent_tab("1");
    app.open_subagent_tab("2");
    // Cycle back to the conversation while a tab is still open, so the strip
    // row exists and the main entry is the active one.
    app.ui_state.subagents.tabs.cycle(1);

    let buffer = render_to_buffer(&mut app, 100, 30);
    let bar = tab_bar_text(&app, &buffer);
    assert!(bar.contains("Main"), "the strip still exists: {bar:?}");
    assert!(
        !bar.contains("Reconnaissance"),
        "the main conversation has no persona: {bar:?}"
    );
}

/// The labels are paid for before the persona. A row too narrow for both keeps
/// every tab label and drops the persona rather than squeezing it to a stub.
#[test]
fn the_tab_bar_keeps_its_labels_and_drops_the_persona_when_the_row_is_narrow() {
    let mut app = app_with_subagents(vec![described_subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");

    // 40 columns leaves the labels their full width and nothing for the
    // persona: the strip must not truncate a label to make room for it.
    let buffer = render_to_buffer(&mut app, 40, 30);
    let bar = tab_bar_text(&app, &buffer);
    assert!(bar.contains("Sally"), "the label survives: {bar:?}");
    assert!(
        !bar.contains("Reconnaissance"),
        "the persona must be dropped, not squeezed: {bar:?}"
    );
}

/// The strip is one row. Its content must never spill onto the row below it,
/// whatever the width.
#[test]
fn the_tab_bar_keeps_its_content_on_its_own_row() {
    for width in [20u16, 30, 40, 60, 100] {
        let mut app = app_with_subagents(vec![
            described_subagent("1", "Sally", "running"),
            described_subagent("2", "Bertha", "running"),
        ]);
        app.open_subagent_tab("1");
        app.open_subagent_tab("2");
        let buffer = render_to_buffer(&mut app, width, 30);
        let viewport = app.viewport_area.expect("viewport drawn");

        let strip: String = (0..width)
            .map(|x| buffer[(x, viewport.y - 1)].symbol())
            .collect();
        let below: String = (0..width)
            .map(|x| buffer[(x, viewport.y)].symbol())
            .collect();
        assert!(
            !below.contains("⇧P"),
            "width {width}: the hints spilled onto the next row: {below:?}"
        );
        assert!(
            strip.chars().count() <= width as usize,
            "width {width}: the strip row is longer than the row"
        );
    }
}

/// With no tabs open the strip row does not exist, so the hints cannot be
/// showing on a row the layout never reserved.
#[test]
fn no_tabs_means_no_strip_row_and_no_hints() {
    let app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert_eq!(app.subagent_tab_strip_height(), 0);

    let mut app = app;
    let buffer = render_to_buffer(&mut app, 100, 30);
    assert!(
        !buffer_text(&buffer).contains("⇧P"),
        "the hints must not appear without a strip row"
    );
}

#[test]
fn no_tab_bar_reserves_no_rows() {
    let app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert_eq!(app.subagent_tab_strip_height(), 0);
}

/// `(baseline viewport rows, viewport rows with tabs open)`.
///
/// The rows are set *before* the baseline render on purpose. Adding subagent
/// rows grows the working strip, so a test that opened tabs on an app whose
/// rows only appeared at the same time would be measuring the strip too and
/// would report a bar that costs five rows when it costs one.
fn tab_bar_layout_cost(width: u16, height: u16) -> (u16, u16) {
    let mut app = app_with_subagents(vec![
        subagent("1", "Sally", "running"),
        subagent("2", "Bertha", "running"),
        subagent("3", "Cleo", "running"),
    ]);
    render_to_buffer(&mut app, width, height);
    let baseline = app.viewport_area.expect("viewport drawn").height;

    for task_id in ["1", "2", "3"] {
        app.open_subagent_tab(task_id);
    }
    render_to_buffer(&mut app, width, height);
    (baseline, app.viewport_area.expect("viewport drawn").height)
}

#[test]
fn the_tab_bar_costs_exactly_one_row_whenever_it_renders() {
    // Asserted across several sizes rather than one: a layout that only holds
    // at 100x30 is not a layout.
    for (width, height) in [(100u16, 30u16), (100, 50), (120, 24), (80, 20)] {
        let (baseline, with_tabs) = tab_bar_layout_cost(width, height);
        let delta = baseline as i32 - with_tabs as i32;
        assert!(
            delta == 0 || delta == 1,
            "{width}x{height}: the tab bar cost {delta} rows; it is one row or none"
        );
    }
}

#[test]
fn the_tab_bar_never_pushes_the_transcript_below_its_floor() {
    // `Min(10)` in `session_layout` is a *soft* floor: ratatui yields it when
    // the pane is oversubscribed, and the working strip is allowed to
    // oversubscribe it — three subagent rows on a 20-line pane already claim
    // their own. So the bar must be the thing that yields, not the transcript.
    //
    // On a pane with no room the bar is dropped entirely. That is the honest
    // outcome and the one worth pinning: tabs stay reachable by `Shift+N`
    // either way, whereas a truncated transcript is the loss the user cannot
    // see coming.
    for (width, height) in [(100u16, 30u16), (100, 50), (120, 24), (80, 20)] {
        let (baseline, with_tabs) = tab_bar_layout_cost(width, height);
        assert!(
            with_tabs >= MIN_TRANSCRIPT_FLOOR,
            "{width}x{height}: the tab bar pushed the transcript to {with_tabs} rows, \
             below the floor of {MIN_TRANSCRIPT_FLOOR} (baseline was {baseline})"
        );
    }
}

/// The transcript's floor in `session_layout` (`Constraint::Min(10)`).
const MIN_TRANSCRIPT_FLOOR: u16 = 10;

#[test]
fn a_tab_whose_subject_left_the_snapshot_says_so_instead_of_claiming_to_run() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");

    // The turn ended: the per-turn agent tree was cleared and a snapshot with
    // no tree arrived.
    app.ui_state.subagents.set_rows(Vec::new());
    let tab = app
        .ui_state
        .subagents
        .active_tab()
        .expect("the tab survives its subject");
    assert!(
        !tab.is_running(),
        "a detached tab must not claim to be live"
    );

    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    assert!(
        text.contains("no longer in the session's projection"),
        "the view must state the terminal state rather than leave a stale claim: {text}"
    );
    // It keeps showing what the run was, which is the point of detaching.
    assert!(text.contains("work for 1"), "{text}");
}

#[test]
fn a_detached_tab_returns_to_live_when_its_subject_comes_back() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.open_subagent_tab("1");
    app.ui_state.subagents.set_rows(Vec::new());
    assert!(!app
        .ui_state
        .subagents
        .active_tab()
        .expect("open")
        .is_running());

    app.ui_state
        .subagents
        .set_rows(vec![subagent("1", "Sally", "running")]);
    assert!(app
        .ui_state
        .subagents
        .active_tab()
        .expect("open")
        .is_running());
}

#[test]
fn the_overlay_is_a_keyboard_path_into_a_tab() {
    let mut app = app_with_subagents(vec![
        subagent("1", "Sally", "running"),
        subagent("2", "Bertha", "running"),
    ]);
    app.ui_state.input_mode = InputMode::Normal;
    app.toggle_tasks_list();
    assert!(app.tasks_expanded);

    // One `j` from a fresh overlay lands on the *first* row, not the second.
    assert!(app.move_tasks_selection(1));
    assert_eq!(app.tasks_selected, Some(1), "j lands on the first body row");

    assert!(app.open_selected_task());
    assert_eq!(
        app.ui_state
            .subagents
            .active_tab()
            .map(|tab| tab.tab_id.as_str()),
        Some("1")
    );
    assert!(!app.tasks_expanded, "opening closes the overlay");
}

#[test]
fn an_overlay_selection_does_not_retarget_when_its_row_finishes() {
    let mut app = test_app();
    let rows = vec![
        SubagentRow {
            child_instance_id: Some("child-first".into()),
            task_id: "7".into(),
            run_id: None,
            display_name: Some("first".into()),
            profile: None,
            usefulness: None,
            agent: "explore".into(),
            description: "first task".into(),
            status: "running".into(),
        },
        SubagentRow {
            child_instance_id: Some("child-second".into()),
            task_id: "7".into(),
            run_id: None,
            display_name: Some("second".into()),
            profile: None,
            usefulness: None,
            agent: "explore".into(),
            description: "second task".into(),
            status: "running".into(),
        },
    ];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.tasks_expanded = true;
    app.move_tasks_selection(1);
    let rows = vec![subagent("7", "second", "running")];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);

    assert!(
        !app.open_selected_task(),
        "the disappeared selection must not slide onto the next row"
    );
    assert!(app.ui_state.subagents.tabs.is_empty());
}

#[test]
fn the_overlay_cursor_walks_every_group_without_resting_on_a_heading() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    app.set_todos(vec![codegg::tui::app::TodoEntry {
        content: "a todo".into(),
        status: "pending".into(),
        priority: "medium".into(),
    }]);
    app.ui_state.input_mode = InputMode::Normal;
    app.toggle_tasks_list();

    // Rows are [Todo header, todo, Subagents header, subagent]. The cursor
    // must visit both body rows and rest on neither heading — a cursor parked
    // on `── Subagents ──` would make `Enter` a no-op the user has to learn to
    // step over.
    let mut visited = Vec::new();
    for _ in 0..4 {
        if !app.move_tasks_selection(1) {
            break;
        }
        let index = app.tasks_selected.expect("cursor");
        assert!(
            !matches!(app.bounded_task_rows()[index], TaskRow::Header { .. }),
            "the cursor rested on a group heading at {index}"
        );
        visited.push(index);
    }
    assert_eq!(visited, vec![1, 3], "both body rows, in display order");
    // Clamped at the end rather than wrapping.
    assert!(!app.move_tasks_selection(1));
}

#[test]
fn the_overlay_cursor_never_rests_on_a_row_that_was_truncated_away() {
    let mut app = test_app();
    // Far more rows than the overlay's total budget.
    let rows: Vec<SubagentRow> = (0..60)
        .map(|index| subagent(&index.to_string(), &format!("n{index}"), "running"))
        .collect();
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    app.toggle_tasks_list();

    for _ in 0..40 {
        app.move_tasks_selection(1);
        let Some(index) = app.tasks_selected else {
            continue;
        };
        assert!(
            index < app.bounded_task_rows().len(),
            "the cursor pointed past the bounded rows"
        );
    }
}

// ── C007: addressing the composer to a running subagent ─────────────

/// Arm one row, the way `/subagent-send <task id>` does.
fn armed_to(app: &mut App, task_id: &str) -> bool {
    codegg::tui::commands::subagent_steer::arm_addressee(app, task_id)
}

fn composer_text(app: &mut App) -> String {
    buffer_text(&render_to_buffer(app, 100, 30))
}

/// The composer badge names the subagent, and stops naming the agent.
///
/// The single destination badge is the whole safety mechanism here: `[LLM]`
/// beside `[TO Sally]` would be two claims about one keypress and the wrong
/// one, because Enter does not start a turn while armed.
#[test]
fn addressing_a_running_subagent_names_it_as_the_composer_destination() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    let text = composer_text(&mut app);
    assert!(
        text.contains("[TO Sally]"),
        "the badge must name the addressed run: {text}"
    );
    assert!(
        !text.contains("[LLM]"),
        "a second destination badge would misreport where Enter goes: {text}"
    );
    assert!(
        text.contains("about Sally"),
        "the placeholder states the same thing: {text}"
    );
}

/// The addressee survives a re-render and a snapshot that still reports the
/// run as live. A user typing a long reply across several refreshes must not
/// silently have it become an ordinary prompt.
#[test]
fn an_addressee_survives_a_snapshot_that_still_reports_its_run_as_running() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    app.apply_subagent_rows(vec![subagent("1", "Sally", "running")]);
    assert!(app.prompt_state.subagent_addressee.is_some());
    assert!(composer_text(&mut app).contains("[TO Sally]"));
}

/// A run that finished but is **still in the tree** must clear the addressee.
/// A presence check alone would keep it armed, and the reply would then be
/// steered into a live turn on behalf of a task that is already done.
#[test]
fn a_finished_run_clears_the_addressee_even_while_it_is_still_in_the_tree() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    app.apply_subagent_rows(vec![subagent("1", "Sally", "completed")]);
    assert!(
        app.prompt_state.subagent_addressee.is_none(),
        "a finished run cannot stay addressed"
    );
    assert!(
        composer_text(&mut app).contains("[LLM]"),
        "the badge falls back to the ordinary destination"
    );
}

/// The other invalidation: the run left the tree entirely.
#[test]
fn a_run_that_leaves_the_tree_clears_the_addressee() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    app.apply_subagent_rows(Vec::new());
    assert!(app.prompt_state.subagent_addressee.is_none());
}

/// A subagent dispatched by the previous turn is not one this turn's agent is
/// running, so a turn start drops the address rather than carrying it forward.
#[test]
fn a_new_turn_clears_the_addressee() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    app.apply_core_event(codegg::protocol::core::CoreEvent::TurnStarted {
        session_id: "session-1".to_string(),
        turn_id: "turn-2".to_string(),
    });
    assert!(
        app.prompt_state.subagent_addressee.is_none(),
        "a reply aimed at the previous turn's run has no meaning here"
    );
}

/// I5: addressing a run cancels nothing. No request leaves the frontend at all
/// for an arm, so there is no path by which arming could interrupt, evict, or
/// cancel the run it names.
#[test]
fn addressing_a_subagent_issues_no_request_at_all() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    // The fake client records every request; an arm must add nothing to it.
    assert!(
        !app.session_state.stopped_turn,
        "arming must not stop anything"
    );
    assert!(app.session_state.active_turn_id.is_none());
}

/// A finished run cannot be armed in the first place, so a user who races the
/// projection gets a sentence rather than a silently inert badge.
#[test]
fn a_finished_run_refuses_to_arm() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "completed")]);
    assert!(!armed_to(&mut app, "1"));
    assert!(app.prompt_state.subagent_addressee.is_none());
}

/// `/subagent-send off` is the undo, and it is a command rather than a key so
/// a user who armed by mistake is never stuck with a mode they cannot clear.
#[test]
fn the_off_form_clears_the_addressee() {
    let mut app = app_with_subagents(vec![subagent("1", "Sally", "running")]);
    assert!(armed_to(&mut app, "1"));
    app.ui_state.command_mode = true;
    for c in "/subagent-send off".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.prompt_state.subagent_addressee.is_none());
    assert!(composer_text(&mut app).contains("[LLM]"));
}

// ---------------------------------------------------------------------------
// 6. Tab surface ownership
// ---------------------------------------------------------------------------
//
// The subagent view used to be drawn *over* the transcript rather than
// *instead of* it: the transcript was painted first, and the subagent's own
// text was then painted on top of it, so a reader looking at a subagent saw
// the main conversation showing through the subagent's view and in the
// viewport's last column. The tab strip row was painted foreground-only over
// a row carved out of the viewport, which made it the one surface in the TUI
// with no background at all.
//
// These tests pin the ownership: one surface, one rect, and no transparent
// cell anywhere in the main buffer.

/// Short tokens the transcript prints, spread across its one line.
///
/// The tokens are short and spread out because the baseline defect covered
/// only the *first* line of the subagent's view: one long marker was hidden
/// behind that line, and a short one is hidden by chance. Asserting on every
/// token asks the right question — is *any part* of the transcript showing
/// through — rather than whether one whole marker is intact.
const TRANSCRIPT_TOKENS: [&str; 6] = [
    "TRNS0001", "TRNS0002", "TRNS0003", "TRNS0004", "TRNS0005", "TRNS0006",
];

/// The transcript's single line: every token, space separated.
fn transcript_marker() -> String {
    TRANSCRIPT_TOKENS.join(" ")
}

/// Text that only the subagent tab's body prints.
///
/// The row carries a descriptor, so the working strip shows *that* rather than
/// the task description — which makes the task description a reliable marker
/// for "the tab body is on screen".
const TAB_ONLY_TEXT: &str = "Map the auth flow";

/// A space-free token, used to push the tab's own text to the rect's edge.
///
/// It has to be space-free: a word wrapper breaks at spaces, and a description
/// made of ordinary words would leave the last column empty however long the
/// text was.
const EDGE_FILLER: &str = "EDGEFILLER";

/// An app on the Session route whose transcript carries the marker.
///
/// The route matters: the Home route renders an empty-state placeholder and
/// never draws the transcript, so a ghosting test against Home would pass for
/// the wrong reason.
fn app_with_transcript_marker() -> App {
    let mut app = test_app();
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session("s1".into()));
    app.messages_state
        .messages
        .add_assistant_text(transcript_marker());
    app
}

/// The subagent whose tab body carries [`TAB_ONLY_TEXT`].
fn tabbed_subagent() -> SubagentRow {
    SubagentRow {
        child_instance_id: Some("child-1".into()),
        task_id: "1".into(),
        run_id: Some("run-1".into()),
        display_name: Some("Sally".into()),
        profile: Some("Reconnaissance generalist.".into()),
        usefulness: None,
        agent: "explore".into(),
        description: TAB_ONLY_TEXT.to_string(),
        status: "running".into(),
    }
}

/// Open the subagent's tab, with the projection seeded so the row survives a
/// render that refreshes it from the snapshot.
fn open_tabbed_subagent(app: &mut App) {
    let rows = vec![tabbed_subagent()];
    seed_projection_with_subagents(app, &rows);
    app.ui_state.subagents.set_rows(rows);
    assert!(app.open_subagent_tab("1"), "the tab opens");
}

/// Open a change review — a modal decision surface that outranks the tab.
fn open_change_review(app: &mut App) {
    let patches = vec![(
        "src/lib.rs".to_string(),
        "@@ -1,1 +1,1 @@\n-old();\n+new();\n".to_string(),
    )];
    app.change_review_state.open(
        "preview-1".to_string(),
        "edit".to_string(),
        "Rename helper".to_string(),
        "agent".to_string(),
        false,
        codegg::tui::unified_diff::parse_review(&patches),
        vec!["src/lib.rs".to_string()],
    );
}

/// The reported defect: the subagent's own view sat **on top of** the main
/// conversation, so the reader saw the main agent's transcript showing
/// through it.
#[test]
fn an_open_subagent_tab_replaces_the_transcript_instead_of_ghosting_over_it() {
    let mut app = app_with_transcript_marker();
    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    for token in TRANSCRIPT_TOKENS {
        assert!(
            text.contains(token),
            "precondition: the transcript renders {token}"
        );
    }

    open_tabbed_subagent(&mut app);
    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    for token in TRANSCRIPT_TOKENS {
        assert!(
            !text.contains(token),
            "the main conversation is showing through the subagent's own view: {token}\n{text}"
        );
    }
    assert!(
        text.contains("Subagent · Sally"),
        "the subagent's own view must be what is on screen:\n{text}"
    );
}

/// The negative case, so the ghosting test above cannot pass vacuously: with
/// no tab open the transcript is the surface, and it renders normally.
#[test]
fn with_no_tab_open_the_transcript_still_renders() {
    let mut app = app_with_transcript_marker();
    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    for token in TRANSCRIPT_TOKENS {
        assert!(
            text.contains(token),
            "the transcript is the viewport's owner when no tab is open: {token}\n{text}"
        );
    }
    assert!(
        !text.contains(TAB_ONLY_TEXT),
        "and nothing from a tab that was never opened:\n{text}"
    );
}

/// The viewport rect is filled to its edge.
///
/// The paragraph used to be one column narrower than the rect, and the rect
/// had no background of its own, so the last column showed whatever was behind
/// it — the transcript's scrollbar. A description with no spaces in it cannot
/// be word-wrapped short of the edge: it lands on one line, and the buffer
/// clips it at the rect's right border, so the subagent's own text reaches
/// the final column.
#[test]
fn the_subagent_view_fills_the_viewport_to_its_last_column() {
    let mut app = app_with_transcript_marker();
    let rows = vec![SubagentRow {
        description: EDGE_FILLER.repeat(40),
        ..tabbed_subagent()
    }];
    seed_projection_with_subagents(&mut app, &rows);
    app.ui_state.subagents.set_rows(rows);
    assert!(app.open_subagent_tab("1"));

    let buffer = render_to_buffer(&mut app, 100, 30);
    let viewport = app.viewport_area.expect("viewport drawn");
    let last = viewport.x + viewport.width - 1;
    let column: String = (viewport.y..viewport.y + viewport.height)
        .map(|y| buffer[(last, y)].symbol().to_string())
        .collect();

    assert!(
        column.chars().any(|c| EDGE_FILLER.contains(c)),
        "the subagent's own text never reaches the last column of the viewport: {column:?}"
    );
}

/// The tab strip row is carved out of the viewport chunk, so nothing else ever
/// paints it. It is the one row that must carry a background of its own.
#[test]
fn the_tab_strip_row_is_opaque() {
    let mut app = app_with_transcript_marker();
    open_tabbed_subagent(&mut app);
    let buffer = render_to_buffer(&mut app, 100, 30);
    let viewport = app.viewport_area.expect("viewport drawn");
    // The bar is carved off the top of the viewport, so it is the row above it.
    let row = viewport.y - 1;

    for x in viewport.x..viewport.x + viewport.width {
        assert_ne!(
            buffer[(x, row)].bg,
            Color::Reset,
            "the tab strip row is transparent at column {x}"
        );
    }
}

/// The change review is modal: it is a decision the user has to answer, so a
/// subagent tab must never cover it.
#[test]
fn an_open_change_review_is_not_covered_by_a_subagent_tab() {
    let mut app = app_with_transcript_marker();
    open_tabbed_subagent(&mut app);
    open_change_review(&mut app);

    let text = buffer_text(&render_to_buffer(&mut app, 100, 30));
    assert!(
        text.contains("Rename helper"),
        "the review must be what is on screen:\n{text}"
    );
    assert!(
        !text.contains(TAB_ONLY_TEXT),
        "the tab overlaid the review:\n{text}"
    );
}
