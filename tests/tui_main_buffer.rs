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

/// Text of the working-strip rows only.
///
/// The live toast column is a 60x10 overlay pinned to the top right, so
/// `buffer_text` cannot distinguish "the status row says this" from "a toast
/// that happens to be on screen right now says this". Reading the strip's own
/// rect is the only way to assert on the record rather than the notification.
fn strip_text(app: &App, buf: &Buffer) -> String {
    let Some(strip) = app.ui_state.working_area else {
        return String::new();
    };
    (strip.y..strip.y + strip.height)
        .map(|y| {
            (strip.x..strip.x + strip.width)
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
fn the_status_row_is_reserved_in_every_state_so_the_transcript_never_reflows() {
    // The strip's height is subtracted from the transcript viewport, so a strip
    // that appears and disappears moves everything the user is reading. It used
    // to be gated on "there is something to say", which meant the transcript
    // jumped on every submit and again on every completion.
    //
    // Row 0 is reserved unconditionally now, so the invariant to defend is not
    // "idle shows no strip" — it is that the viewport is the same size in every
    // state. That is the stronger claim and the one actually worth a test.
    let mut strip_heights = Vec::new();
    let mut prompt_tops = Vec::new();
    for status in [
        SessionStatus::Idle,
        SessionStatus::Working,
        SessionStatus::Error,
        SessionStatus::Idle,
    ] {
        let mut app = test_app();
        app.session_state.session_status = status;
        render_to_buffer(&mut app, 100, 30);
        let strip = app
            .ui_state
            .working_area
            .expect("the status row is reserved in every state");
        strip_heights.push(strip.height);
        prompt_tops.push(app.prompt_area.expect("prompt area recorded").y);
    }

    assert!(
        strip_heights.iter().all(|h| *h == 1),
        "the status row must stay exactly one row tall, got {strip_heights:?}"
    );
    assert!(
        prompt_tops.windows(2).all(|w| w[0] == w[1]),
        "the composer must not move between session states, got {prompt_tops:?}"
    );
}

#[test]
fn an_idle_buffer_names_idle_rather_than_claiming_to_work() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    let buffer = render_to_buffer(&mut app, 100, 30);
    let text = buffer_text(&buffer);

    assert!(
        text.contains("idle"),
        "the reserved status row must still say something when nothing is running:\n{text}"
    );
    assert!(
        !text.contains("working"),
        "an idle session must never claim to be working:\n{text}"
    );
    assert!(
        !text.contains("stop"),
        "an idle session must not advertise a stop key it cannot honour:\n{text}"
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
    // long enough to be read after the fact. Read from the strip's own rect so
    // a toast that happens to be on screen cannot satisfy this.
    let buffer = render_to_buffer(&mut app, 100, 30);
    let strip = strip_text(&app, &buffer);
    assert!(
        strip.contains("529 overloaded"),
        "the failure reason must stay on screen:\n{strip}"
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
    let resumed = strip_text(&app, &buffer);
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
    let strip = strip_text(&app, &buffer);
    assert!(
        strip.contains("429 rate_limit"),
        "reason must show:\n{strip}"
    );
    // A spinner beside a failure would claim work is still progressing.
    assert!(
        !strip.contains('⠋') && !strip.contains('⠙') && !strip.contains('⠹'),
        "a latched failure must not animate as though work continues:\n{strip}"
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
        text.contains("to stop"),
        "the working strip must advertise the stop key; a long pause with no \
         way out is what reads as a disconnect. Rendered:\n{text}"
    );
}

#[test]
fn the_stop_confirmation_is_visible_between_the_two_presses() {
    let mut app = test_app();
    // A fresh app composes (Insert), and bare-char bindings are Normal-mode
    // only so `x` can never shadow a letter being typed. Reaching for the stop
    // key therefore means leaving Insert first.
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    app.session_state.session_status = SessionStatus::Working;

    // First press: arm, and say what completes it. Without this the second
    // press looks like a no-op and the key reads as broken.
    let _ = render_to_buffer(&mut app, 100, 30);
    app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
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

    assert!(
        text.contains("stopped"),
        "a stopped agent must say so. Rendered:\n{text}"
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
