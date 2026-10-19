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
use codegg::tui::app::state::tasks::TaskRow;
use codegg::tui::app::{App, SessionStatus};
use codegg::tui::components::messages::MsgPart;
use codegg::tui::input::InputMode;
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
            profile: None,
            usefulness: None,
            task_id: "2".into(),
            run_id: None,
            display_name: Some("Greg".into()),
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
                profile: None,
                usefulness: None,
                task_id: i.to_string(),
                run_id: None,
                display_name: None,
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

/// Regression: a main agent blocked on delegated work must not read as idle.
///
/// The main agent fans out and then goes silent while it waits for a subagent
/// or a job. `ActivityPhase::WaitingOnSubagent` existed for this the whole time
/// and was never constructed by anything, so row 0 fell through to the literal
/// word `idle` for the entire wait — which is indistinguishable from a hung
/// session. The wait is now derived from the projection.
#[test]
fn a_main_agent_waiting_on_subagents_does_not_read_as_idle() {
    let mut app = test_app();
    // The main agent's own stream is finished; it is blocked on the subagent.
    app.session_state.session_status = SessionStatus::Idle;
    app.ui_state.subagents.set_rows(vec![
        SubagentRow {
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
            profile: None,
            usefulness: None,
            task_id: "t2".into(),
            run_id: None,
            display_name: None,
            agent: "explore".into(),
            description: "already done".into(),
            status: "completed".into(),
        },
    ]);

    assert_eq!(app.awaiting_task_counts().0, 1, "only the live row counts");
    let buffer = render_to_buffer(&mut app, 100, 30);
    // Row 0 is the status surface under test. The footer separately renders the
    // session's own status word, so assert on the strip rather than the frame.
    let strip = strip_text(&app, &buffer);
    assert!(
        strip.contains("awaiting 1 task"),
        "a blocked main agent must name the wait: {strip}"
    );
    assert!(
        !strip.lines().next().unwrap_or_default().contains(" idle"),
        "`idle` is the wrong answer while work is in flight: {strip}"
    );
    assert!(
        strip.contains("1 subagent"),
        "the row must break the wait down by kind: {strip}"
    );
}

#[test]
fn an_actually_idle_session_still_reads_idle() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        profile: None,
        usefulness: None,
        task_id: "t1".into(),
        run_id: None,
        display_name: None,
        agent: "explore".into(),
        description: "done".into(),
        status: "completed".into(),
    }]);

    assert_eq!(app.awaiting_task_counts(), (0, 0));
    let buffer = render_to_buffer(&mut app, 100, 30);
    let strip = strip_text(&app, &buffer);
    assert!(
        strip.lines().next().unwrap_or_default().contains("idle"),
        "with no live work, idle is the honest answer: {strip}"
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

/// Regression: a live job must reach the status row through the projection.
///
/// Every other test here sets `ui_state.subagents` directly, which is exactly
/// how the original tests passed while the real daemon-attached TUI showed
/// `idle`: nothing in production ever put rows there. The daemon-attached
/// frontend subscribes per session and never switches a project tab, so
/// `active_tab_id` is `None` — and `active_snapshot` used to bail out on
/// exactly that, reporting "no snapshot" for a session with a job running.
#[test]
fn a_live_job_in_the_projection_reaches_the_status_row() {
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
    assert_eq!(
        app.awaiting_task_counts(),
        (0, 1),
        "one live job must be counted straight from the projection"
    );

    let buffer = render_to_buffer(&mut app, 100, 30);
    let strip = strip_text(&app, &buffer);
    assert!(
        strip.contains("awaiting 1 task"),
        "a session waiting on a job must name the wait: {strip}"
    );
    assert!(
        strip.contains("1 job"),
        "the row must break the wait down by kind: {strip}"
    );
}

/// Regression: the projection must reach the status row without the sidebar.
///
/// `ui_state.subagents` was previously written only from the session-panel
/// render path, so a layout that does not draw the panel left the count at
/// zero. Row 0 is the strip's own status surface and must not depend on some
/// other widget being visible.
#[test]
fn the_status_row_counts_live_work_without_the_sidebar() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Idle;
    seed_projection_with_live_job(&mut app);

    // No panel draw happens here; the render pass alone must establish the
    // job count that row 0 reads.
    let _ = render_to_buffer(&mut app, 100, 30);
    assert_eq!(
        app.awaiting_task_counts().1,
        1,
        "the job count must survive a render that never draws the panel"
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
        profile: None,
        usefulness: None,
        task_id: "7".into(),
        run_id: Some("run-7".into()),
        display_name: Some("Sally".into()),
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
    let tab = app
        .ui_state
        .subagents
        .active_tab()
        .cloned()
        .expect("clicking a subagent row opens its buffer");
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

#[test]
fn clicking_the_throbber_row_opens_nothing() {
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        profile: None,
        usefulness: None,
        task_id: "7".into(),
        run_id: None,
        display_name: None,
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }]);
    render_to_buffer(&mut app, 100, 30);

    let strip = app.ui_state.working_area.expect("strip drawn");
    click(&mut app, strip.x + 3, strip.y);
    assert!(app.ui_state.subagents.active_tab().is_none());
}

#[test]
fn the_strip_is_hit_tested_before_the_composer() {
    // The strip is carved out of the prompt chunk. If the prompt were tested
    // first, a subagent click would resolve to click-to-focus and the row
    // would be unopenable.
    let mut app = test_app();
    app.session_state.session_status = SessionStatus::Working;
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        profile: None,
        usefulness: None,
        task_id: "7".into(),
        run_id: None,
        display_name: None,
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
    assert!(app.ui_state.subagents.active_tab().is_some());
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
    app.ui_state.subagents.set_rows(vec![SubagentRow {
        profile: None,
        usefulness: None,
        task_id: "1".into(),
        run_id: None,
        display_name: Some("Sally".into()),
        agent: "explore".into(),
        description: "Map the auth flow".into(),
        status: "running".into(),
    }]);
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
        bar.contains("Chat"),
        "the conversation is always offered: {bar:?}"
    );
    assert!(bar.contains("Sally"), "{bar:?}");
    assert!(bar.contains("Bertha"), "{bar:?}");
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
