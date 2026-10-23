//! Row-precise disclosure: a click reaches the part under the cursor.
//!
//! The click used to be answered by the *message*, and the message's
//! reasoning arm won: in a message carrying both a reasoning block and a
//! tool call, click one expanded the reasoning, click two collapsed it
//! again, and the tool call could never be reached by clicking at all.
//!
//! **What is observable here.**
//!
//! 1. **A row resolves to a part.** The transcript's rows are read back out
//!    of a rendered buffer, and each row is resolved to the part that owns
//!    it: the tool call's header to the tool call, the reasoning block's
//!    header to the reasoning block, and the prose's row to nothing.
//!
//! 2. **A click moves that part and only that part.** The click is the real
//!    one — a press and a release on the cell, through the app's own mouse
//!    path — so what is asserted here is what a user gets. Clicking the
//!    tool call's header opens the tool call's output; clicking the
//!    reasoning block's header opens the reasoning text; neither disturbs
//!    the other.
//!
//! 3. **A row with nothing expandable changes nothing.** The prose row is
//!    not a fallback to the message-level toggle.
//!
//! 4. **One producer (I1).** After an expand-all, the row count the
//!    scrollbar is built from is the row count the transcript draws.
//!    Expanding changes the row count, and the scrollbar has to follow.

use codegg::session::message::ToolStatus;
use codegg::tui::app::App;
use codegg::tui::components::messages::{MessageRole, MessagesWidget, MsgPart, RowPart, UIMessage};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

/// The transcript's width.
const WIDTH: u16 = 60;

/// The viewport height the widget reports its scroll range against.
const VIEWPORT: usize = 20;

/// The reasoning body. It is indented by two columns when it is drawn.
const REASONING: &str =
    "The capture semantics differ between the two transports, so the harness has to observe both.";

/// The tool call's output: several lines, so a collapsed call has something
/// to disclose.
const TOOL_OUTPUT: &str = "running 42 tests\ntest result: ok\nwarning: unused import";

/// The prose of the transcript.
const PROSE: &str = "The comparative harness confirms the delta across both transports.";

/// A collapsed tool call.
fn tool_part(id: &str) -> MsgPart {
    MsgPart::ToolCall {
        id: id.to_string(),
        name: "bash".to_string(),
        input: "{\"command\":\"cargo test -p codegg\"}".to_string(),
        output: TOOL_OUTPUT.to_string(),
        status: ToolStatus::Completed,
        duration_ms: Some(12),
        exit_code: Some(0),
        output_lines: Some(3),
        expanded: false,
    }
}

/// A collapsed reasoning block.
fn reasoning_part() -> MsgPart {
    MsgPart::Reasoning {
        content: REASONING.to_string(),
        collapsed: true,
    }
}

/// The prose.
fn prose_part() -> MsgPart {
    MsgPart::Text {
        content: PROSE.to_string(),
    }
}

/// The transcript under test: a collapsed tool call, a collapsed reasoning
/// block and the prose, in one message — the shape that made the tool call
/// unreachable.
fn transcript_parts() -> Vec<MsgPart> {
    vec![tool_part("t1"), reasoning_part(), prose_part()]
}

/// A widget holding one assistant message with the given parts.
fn widget_with(parts: Vec<MsgPart>) -> MessagesWidget {
    let mut widget = MessagesWidget::default();
    widget.set_width(WIDTH);
    widget.set_visible_height(VIEWPORT);
    widget.messages.push(UIMessage {
        role: MessageRole::Assistant,
        parts,
        timestamp: None,
        is_plan_mode: None,
    });
    widget.invalidate_rows();
    widget.scroll_to_top();
    widget
}

/// An app holding the transcript, with a viewport to click in.
fn app_with_transcript() -> App {
    let mut app = App::new_for_testing("/tmp/m004-row-precise-disclosure".to_string());
    app.messages_state.messages.set_width(WIDTH);
    app.messages_state.messages.set_visible_height(VIEWPORT);
    app.messages_state.messages.messages.push(UIMessage {
        role: MessageRole::Assistant,
        parts: transcript_parts(),
        timestamp: None,
        is_plan_mode: None,
    });
    app.messages_state.messages.invalidate_rows();
    app.messages_state.messages.scroll_to_top();
    app.viewport_area = Some(Rect::new(0, 0, WIDTH, VIEWPORT as u16));
    app
}

/// The rows the widget draws, read back out of a rendered buffer.
fn drawn_rows(widget: &MessagesWidget, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, WIDTH, height);
    let mut buffer = Buffer::empty(area);
    widget.render(area, &mut buffer);
    (0..height)
        .map(|y| {
            (0..WIDTH)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// The viewport row the transcript draws the given text on.
fn row_of(widget: &MessagesWidget, needle: &str) -> u16 {
    let rows = drawn_rows(widget, VIEWPORT as u16);
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("the transcript must draw a row containing {needle:?}: {rows:?}"))
        as u16
}

/// Click the viewport row `row`: a press and a release on the same cell,
/// through the app's own mouse path.
fn click(app: &mut App, row: u16) {
    let column = 2;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
}

/// The app treats a second press on the same surface inside its
/// double-click window as a double click, and a double click on the
/// viewport does nothing. The two clicks of this transcript have to be
/// separate gestures, so the window has to pass between them.
fn wait_out_the_double_click_window() {
    std::thread::sleep(std::time::Duration::from_millis(320));
}

/// A rendered transcript's rows resolve to the parts that own them: the tool
/// call's header to the tool call, the reasoning block's header to the
/// reasoning block, and the prose's row to nothing at all.
#[test]
fn a_rendered_row_resolves_to_the_part_that_owns_it() {
    let widget = widget_with(transcript_parts());
    let tool_row = row_of(&widget, "bash");
    let reasoning_row = row_of(&widget, "Thinking");
    let prose_row = row_of(&widget, "The comparative harness confirms");

    assert_eq!(
        widget.part_at_viewport_line(tool_row as usize),
        Some((0, RowPart::ToolCall { part_idx: 0 })),
        "the tool call's header row must resolve to the tool call"
    );
    assert_eq!(
        widget.part_at_viewport_line(reasoning_row as usize),
        Some((0, RowPart::Reasoning { part_idx: 1 })),
        "the reasoning block's header row must resolve to the reasoning block"
    );
    assert_eq!(
        widget.part_at_viewport_line(prose_row as usize),
        None,
        "a prose row holds nothing expandable, so it resolves to no part"
    );
}

/// Clicking each header in turn opens that part and leaves the other one
/// alone. The tool call's output and the reasoning text are separate
/// affordances, and a click on one is not a click on the other.
#[test]
fn clicking_each_header_expands_that_part_independently() {
    let mut app = app_with_transcript();

    let tool_row = row_of(&app.messages_state.messages, "bash");
    click(&mut app, tool_row);
    let after_tool = drawn_rows(&app.messages_state.messages, VIEWPORT as u16).join("\n");
    assert!(
        after_tool.contains("test result: ok"),
        "clicking the tool call's header must open its output: {after_tool}"
    );
    assert!(
        !after_tool.contains("capture semantics differ"),
        "clicking the tool call's header must not open the reasoning block: \
         {after_tool}"
    );

    wait_out_the_double_click_window();

    let reasoning_row = row_of(&app.messages_state.messages, "Thinking");
    click(&mut app, reasoning_row);
    let after_reasoning = drawn_rows(&app.messages_state.messages, VIEWPORT as u16).join("\n");
    assert!(
        after_reasoning.contains("capture semantics differ"),
        "clicking the reasoning header must open the reasoning text: \
         {after_reasoning}"
    );
    assert!(
        after_reasoning.contains("test result: ok"),
        "the tool call must still be open after the reasoning block was \
         clicked: {after_reasoning}"
    );
}

/// A row that holds nothing expandable changes nothing. It must not fall
/// back to the message-level toggle: that fallback is the defect this
/// milestone fixes.
#[test]
fn clicking_a_prose_row_changes_nothing() {
    let mut app = app_with_transcript();
    let prose_row = row_of(
        &app.messages_state.messages,
        "The comparative harness confirms",
    );

    click(&mut app, prose_row);

    let rows = drawn_rows(&app.messages_state.messages, VIEWPORT as u16).join("\n");
    assert!(
        !rows.contains("test result: ok"),
        "a prose row must not open the tool call: {rows}"
    );
    assert!(
        !rows.contains("capture semantics differ"),
        "a prose row must not open the reasoning block: {rows}"
    );
}

/// The scrollbar's row count is the row count the transcript draws (I1).
/// Expanding changes the row count — that is the point of expanding — and
/// the scrollbar has to follow it.
#[test]
fn the_scrollbar_follows_the_row_count_after_an_expand_all() {
    let mut widget = widget_with(transcript_parts());
    let collapsed_total = widget.total_lines();
    assert!(
        collapsed_total >= 3,
        "the collapsed transcript must draw a row per part: {collapsed_total}"
    );

    widget.set_all_disclosures_expanded(true);
    let expanded_total = widget.total_lines();

    assert!(
        expanded_total > collapsed_total,
        "expanding must add rows: {collapsed_total} -> {expanded_total}"
    );
    assert_eq!(
        expanded_total,
        drawn_rows(&widget, expanded_total as u16)
            .iter()
            .filter(|row| !row.trim().is_empty())
            .count(),
        "the widget must draw every row it counts after an expand-all"
    );
    assert_eq!(
        widget.max_scroll(),
        expanded_total.saturating_sub(VIEWPORT),
        "the scroll range must come from the same count as the drawn rows"
    );
}
