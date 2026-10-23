//! Main-buffer row accounting: one producer for the drawn rows and the row
//! count (invariant I1).
//!
//! The transcript's scrollbar has to agree with what is on screen: the rows
//! are drawn from one `Vec<Line>` and the row count is measured from that
//! same vector. Two things can break the agreement, and both were possible
//! while the row count was re-derived by a second implementation of the same
//! rules:
//!
//! - the count and the drawn rows drift apart, so the scroll range stops
//!   describing the content and the bottom of the buffer becomes unreachable;
//! - a message's row count stops being the sum of its parts' rows, because a
//!   part's rows are counted one way and the message another.
//!
//! **What is observable here.** The row count is `MessagesWidget::total_lines`
//! and the scrollbar's range is `MessagesWidget::max_scroll`. ratatui-widgets
//! 0.3.2's `ScrollbarState` keeps its fields private and exposes no
//! `content_length` accessor, so `max_scroll` is the range proxy in these
//! tests; the scrollbar's own content length is not read and is not claimed.

use codegg::session::message::ToolStatus;
use codegg::tui::app::App;
use codegg::tui::components::messages::{MessageRole, MsgPart, UIMessage};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// The terminal the transcript is rendered into. Wide enough that the prose
/// wraps over several rows, tall enough that the whole transcript fits.
const RENDER_WIDTH: u16 = 60;
const RENDER_HEIGHT: u16 = 60;

/// The assistant's prose. It ends in `ZULU`, a token that appears nowhere
/// else, so the row that carries it is the last row of the transcript and can
/// be located in the rendered buffer.
const PROSE: &str = "The comparative harness observed both transports under a sustained load and the trailing delta was dropped by the stream transport rather than by the harness itself, which the run records under the ZULU";

/// The reasoning body. It is indented by two columns when it is drawn.
const REASONING: &str = "The capture semantics differ between the two transports, so the comparative harness has to observe both under the same sustained load before the trailing delta can be attributed.";

fn tool_part() -> MsgPart {
    MsgPart::ToolCall {
        id: "t1".into(),
        name: "bash".into(),
        input: "cargo test -p codegg".into(),
        output: "42 passed; 0 failed".into(),
        status: ToolStatus::Completed,
        duration_ms: Some(1200),
        exit_code: Some(0),
        output_lines: Some(1),
        expanded: false,
    }
}

fn reasoning_part() -> MsgPart {
    MsgPart::Reasoning {
        content: REASONING.to_string(),
        collapsed: false,
    }
}

fn prose_part() -> MsgPart {
    MsgPart::Text {
        content: PROSE.to_string(),
    }
}

fn assistant_message(parts: Vec<MsgPart>) -> UIMessage {
    UIMessage {
        role: MessageRole::Assistant,
        parts,
        timestamp: None,
        is_plan_mode: None,
    }
}

/// An app on the Session route whose transcript holds one assistant message
/// with the given parts.
///
/// The route matters: the Home route renders an empty-state placeholder in
/// the viewport and never draws the transcript, so the row count measured
/// against it would be about a surface that is not there.
fn app_with_parts(parts: Vec<MsgPart>) -> App {
    let mut app = App::new_for_testing("/tmp".into());
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Session("s1".into()));
    app.messages_state
        .messages
        .messages
        .push(assistant_message(parts));
    app
}

fn render(app: &mut App) -> Buffer {
    let backend = TestBackend::new(RENDER_WIDTH, RENDER_HEIGHT);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.render(frame);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The row of `area` that carries `needle`, as an absolute buffer row.
fn row_containing(buf: &Buffer, area: Rect, needle: &str) -> Option<u16> {
    (area.y..area.y + area.height).find(|&y| {
        (area.x..area.x + area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .contains(needle)
    })
}

#[test]
fn a_message_with_a_tool_call_a_reasoning_block_and_prose_counts_every_part_s_rows() {
    // One message carrying all three parts.
    let mut combined = app_with_parts(vec![tool_part(), reasoning_part(), prose_part()]);
    render(&mut combined);
    let combined_rows = combined.messages_state.messages.total_lines();

    // The same three parts, each in its own message. The rows are produced
    // by the same builder in the same order, so the two transcripts must
    // occupy the same number of rows.
    let mut split = app_with_parts(vec![tool_part()]);
    split
        .messages_state
        .messages
        .messages
        .push(assistant_message(vec![reasoning_part()]));
    split
        .messages_state
        .messages
        .messages
        .push(assistant_message(vec![prose_part()]));
    render(&mut split);
    let split_rows = split.messages_state.messages.total_lines();

    assert!(
        combined_rows > 0 && split_rows > 0,
        "the fixture must render rows: combined={combined_rows} split={split_rows}"
    );
    assert_eq!(
        combined_rows, split_rows,
        "a message's row count must be the sum of its parts' rows"
    );
}

#[test]
fn the_rows_the_transcript_draws_are_the_rows_the_row_count_reports() {
    let mut app = app_with_parts(vec![tool_part(), reasoning_part(), prose_part()]);
    let buffer = render(&mut app);
    let total = app.messages_state.messages.total_lines();
    let viewport = app.viewport_area.expect("viewport area recorded");

    // The prose is the last part, so the row carrying its final token is the
    // last row of the transcript.
    let last_row = row_containing(&buffer, viewport, "ZULU")
        .expect("the last prose row must be on screen when the content fits");

    assert!(
        total > 0,
        "the transcript must occupy rows: the row count is {total}"
    );
    assert_eq!(
        last_row as usize - viewport.y as usize + 1,
        total,
        "the transcript must draw exactly the rows the row count reports"
    );
}

#[test]
fn the_scroll_range_is_the_row_count_minus_the_viewport() {
    let mut app = app_with_parts(vec![tool_part(), reasoning_part(), prose_part()]);
    render(&mut app);
    let total = app.messages_state.messages.total_lines();
    assert!(total > 0, "the transcript must occupy rows");

    // One row short of the content: the scroll range is exactly one row.
    app.messages_state.messages.set_visible_height(total - 1);
    assert_eq!(
        app.messages_state.messages.max_scroll(),
        1,
        "a viewport one row short leaves one row of scroll range"
    );
    assert!(
        app.messages_state.messages.needs_scrollbar(),
        "content that overflows the viewport needs a scrollbar"
    );

    // A viewport that fits the content has no range at all.
    app.messages_state.messages.set_visible_height(total);
    assert_eq!(
        app.messages_state.messages.max_scroll(),
        0,
        "a viewport that fits the content has no scroll range"
    );
    assert!(
        !app.messages_state.messages.needs_scrollbar(),
        "content that fits the viewport needs no scrollbar"
    );
}
