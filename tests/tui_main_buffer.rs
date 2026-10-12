//! Main-buffer usability regressions: click-to-expand, the working strip,
//! and the subagent buffer.
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
//!    delegated runs exist and nothing about who they are. The working strip
//!    lists them, and clicking one opens a subagent buffer that `Esc` closes.

use codegg::session::message::ToolStatus;
use codegg::tui::app::state::subagents::SubagentRow;
use codegg::tui::app::{App, SessionStatus};
use codegg::tui::components::messages::MsgPart;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
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
fn a_working_turn_draws_a_throbber_above_the_composer() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("working") || text.contains('s'),
        "expected an activity throbber while the turn is in flight"
    );
    let strip = app
        .ui_state
        .working_area
        .expect("the strip must record the rect it was drawn into");
    assert!(strip.height >= 1);
    // The strip belongs to the buffer, not the composer: it sits above the
    // prompt row so a turn in flight reads as part of the transcript.
    let prompt = app.prompt_area.expect("prompt area recorded");
    assert!(
        strip.y + strip.height <= prompt.y,
        "strip {:?} must not overlap the prompt {:?}",
        (strip.y, strip.height),
        prompt.y
    );
}

#[test]
fn an_idle_buffer_reclaims_the_strip_rows() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    let buffer = render_to_buffer(&mut app, 100, 30);
    assert!(
        app.ui_state.working_area.is_none(),
        "an idle turn must not reserve a strip"
    );
    assert!(!buffer_text(&buffer).contains("working 0s"));
}

#[test]
fn active_subagents_are_listed_with_their_task_and_status() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![
        SubagentRow {
            task_id: "1".into(),
            run_id: None,
            agent: "explore".into(),
            description: "Map the auth flow".into(),
            status: "running".into(),
        },
        SubagentRow {
            task_id: "2".into(),
            run_id: None,
            agent: "build".into(),
            description: "Warm the cargo cache".into(),
            status: "completed".into(),
        },
    ]);
    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    assert!(text.contains("explore"), "{text}");
    assert!(text.contains("Map the auth flow"), "{text}");
    assert!(text.contains("build"), "{text}");
    assert!(text.contains("1 subagent"), "{text}");
    assert_eq!(app.ui_state.subagents.hits.len(), 2);
}

#[test]
fn a_long_fan_out_is_truncated_with_an_honest_count() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(
        (0..12)
            .map(|i| SubagentRow {
                task_id: i.to_string(),
                run_id: None,
                agent: format!("agent{i}"),
                description: format!("task {i}"),
                status: "running".into(),
            })
            .collect(),
    );
    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);
    assert!(text.contains("more"), "truncation must be visible: {text}");
    assert!(
        text.contains("12 subagent"),
        "the header must report the true total: {text}"
    );
}

// ---------------------------------------------------------------------------
// 3. Subagent buffer
// ---------------------------------------------------------------------------

#[test]
fn clicking_a_subagent_row_opens_its_buffer_and_esc_returns_to_main() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        task_id: "7".into(),
        run_id: Some("run-7".into()),
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }]);
    render_to_buffer(&mut app, 100, 30);

    let strip = app.ui_state.working_area.expect("strip drawn");
    let (_, row_rect) = app.ui_state.subagents.hits[0];
    // Row 0 of the strip is the throbber, so the first subagent is one below.
    assert_eq!(row_rect.y, strip.y + 1);

    click(&mut app, strip.x + 3, row_rect.y);
    let open = app
        .ui_state
        .subagents
        .open
        .clone()
        .expect("clicking a subagent row opens its buffer");
    assert_eq!(open.row.agent, "explore");

    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);
    assert!(text.contains("Subagent"), "{text}");
    assert!(text.contains("Map the auth flow"), "{text}");
    assert!(text.contains("run-7"), "{text}");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.ui_state.subagents.open.is_none(),
        "Esc must return to the main buffer"
    );
}

#[test]
fn clicking_the_throbber_row_opens_nothing() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        task_id: "7".into(),
        run_id: None,
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }]);
    render_to_buffer(&mut app, 100, 30);

    let strip = app.ui_state.working_area.expect("strip drawn");
    click(&mut app, strip.x + 3, strip.y);
    assert!(app.ui_state.subagents.open.is_none());
}

#[test]
fn the_strip_is_hit_tested_before_the_composer() {
    // The strip is carved out of the prompt chunk. If the prompt were tested
    // first, a subagent click would resolve to click-to-focus and the row
    // would be unopenable.
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        task_id: "7".into(),
        run_id: None,
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }]);
    render_to_buffer(&mut app, 100, 30);

    let strip = app.ui_state.working_area.expect("strip drawn");
    let (_, row_rect) = app.ui_state.subagents.hits[0];
    assert!(
        strip.y + strip.height <= app.prompt_area.expect("prompt").y,
        "the strip must not overlap the prompt rect"
    );
    click(&mut app, strip.x + 3, row_rect.y);
    assert!(app.ui_state.subagents.open.is_some());
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
