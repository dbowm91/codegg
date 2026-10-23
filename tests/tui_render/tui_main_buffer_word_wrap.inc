//! Word-aware wrapping in the main buffer.
//!
//! Two properties are asserted here, and they are asserted through the
//! **rendered** surface rather than by reading the code:
//!
//! 1. **One producer (I1).** The main buffer's rows and the count the
//!    scrollbar is built from come from one producer — the layout cache fed
//!    by `build_message_rows`. A wrap change that escaped that producer
//!    would desynchronize the scrollbar from the content, so the drawn rows
//!    are counted back out of a rendered buffer and compared with the
//!    count the widget reports.
//!
//! 2. **The prose breaks at word boundaries.** The model's own output used
//!    to be wrapped one character at a time, so a row could end in the
//!    middle of a word. The transcript now uses the shared word-aware
//!    engine, and a row ends mid-word only when the word alone is wider
//!    than the row.
//!
//! `ScrollbarState` in ratatui 0.30 keeps its `content_length` and
//! `position` private and exposes no getter, so the scrollbar's range is
//! read through the widget's public `max_scroll()` — the value the
//! scrollbar's own range is built from.

use codegg::session::message::ToolStatus;
use codegg::tui::components::messages::{MessageRole, MessagesWidget, MsgPart, UIMessage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

const WIDTH: u16 = 60;
const VIEWPORT: usize = 12;

/// The assistant prose: one paragraph, so the transcript's row count is the
/// wrap's row count and not a blank-line separator's.
const PROSE: &str = "The trailing delta is dropped by the stream transport, and the comparative harness confirms it across both transports under sustained load.";

/// The reasoning body the model emitted before the answer.
const REASONING: &str = "The capture semantics differ between the two transports, so the harness has to observe both under the same sustained load.";

/// The tool call's output.
const TOOL_OUTPUT: &str = "running 42 tests\ntest result: ok";

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
    widget
}

/// The three parts the acceptance criteria name: a tool call, a reasoning
/// block and assistant prose.
fn all_parts() -> Vec<MsgPart> {
    vec![
        MsgPart::Text {
            content: PROSE.to_string(),
        },
        MsgPart::Reasoning {
            content: REASONING.to_string(),
            collapsed: false,
        },
        MsgPart::ToolCall {
            id: "t1".to_string(),
            name: "bash".to_string(),
            input: "cargo test".to_string(),
            output: TOOL_OUTPUT.to_string(),
            status: ToolStatus::Completed,
            duration_ms: Some(12),
            exit_code: Some(0),
            output_lines: Some(2),
            expanded: false,
        },
    ]
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

/// The rendered rows of one part on its own, so a message's parts can be
/// counted apart from one another.
fn rows_of_part(part: MsgPart) -> usize {
    widget_with(vec![part]).total_lines()
}

/// Assert that no rendered row ends in the middle of a word.
///
/// Each whitespace-separated piece of a row must be a whole word of the
/// source. A character-aware wrap produces rows whose last piece is a
/// fragment — "the comparative har" followed by "ness" — and that is what
/// this rejects.
fn assert_breaks_at_word_boundaries(rows: &[String], source: &str) {
    let words: Vec<&str> = source.split_whitespace().collect();
    for row in rows {
        for piece in row.split_whitespace() {
            assert!(
                words.contains(&piece),
                "row {row:?} breaks mid-word: {piece:?} is not a whole word of the source"
            );
        }
    }
}

/// The transcript's rows are the rows the scrollbar counts, and the prose
/// they carry is wrapped by words.
#[test]
fn the_transcript_draws_the_rows_the_scrollbar_counts() {
    let widget = widget_with(all_parts());
    let total = widget.total_lines();
    assert!(total > 0, "a transcript with three parts must occupy rows");

    // The drawn rows, counted back out of the rendered buffer.
    let drawn = drawn_rows(&widget, total as u16);
    let non_blank = drawn.iter().filter(|row| !row.trim().is_empty()).count();
    assert_eq!(
        non_blank, total,
        "the widget must draw every row it counts: {drawn:?}"
    );

    // The scrollbar's range is built from the same count. `ScrollbarState`
    // exposes no accessor for its length, so `max_scroll` is the public
    // proxy for it.
    assert_eq!(
        widget.max_scroll(),
        total.saturating_sub(VIEWPORT),
        "the scroll range must come from the same count as the drawn rows"
    );

    // The prose's rows are the shared word-aware engine's rows, so the
    // transcript and every other surface that uses that engine lay the same
    // text out the same way — and a row ends at a word boundary rather than
    // at an arbitrary character.
    let prose_widget = widget_with(vec![MsgPart::Text {
        content: PROSE.to_string(),
    }]);
    let prose_drawn = drawn_rows(&prose_widget, prose_widget.total_lines() as u16);
    let prose_rows: Vec<String> = prose_drawn
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect();
    assert_eq!(
        prose_rows,
        codegg::tui::wrap::wrap_to_strings(PROSE, WIDTH),
        "the prose must occupy the rows the word-aware engine produces"
    );
}

/// A message carrying a tool call, a reasoning block and assistant prose
/// occupies the sum of its parts' rows.
#[test]
fn a_message_with_a_tool_call_a_reasoning_block_and_prose_renders_the_sum_of_its_parts_rows() {
    let prose_rows = rows_of_part(MsgPart::Text {
        content: PROSE.to_string(),
    });
    let reasoning_rows = rows_of_part(MsgPart::Reasoning {
        content: REASONING.to_string(),
        collapsed: false,
    });
    let tool_rows = rows_of_part(MsgPart::ToolCall {
        id: "t1".to_string(),
        name: "bash".to_string(),
        input: "cargo test".to_string(),
        output: TOOL_OUTPUT.to_string(),
        status: ToolStatus::Completed,
        duration_ms: Some(12),
        exit_code: Some(0),
        output_lines: Some(2),
        expanded: false,
    });

    let combined = widget_with(all_parts()).total_lines();
    assert_eq!(
        combined,
        prose_rows + reasoning_rows + tool_rows,
        "a message's rows must be the sum of its parts' rows"
    );

    // And the prose those rows carry breaks at word boundaries. The check
    // reads the prose's own rows: the other parts' rows carry their own
    // text (the reasoning header, the tool header), which is not prose.
    let prose_widget = widget_with(vec![MsgPart::Text {
        content: PROSE.to_string(),
    }]);
    let prose_drawn = drawn_rows(&prose_widget, prose_widget.total_lines() as u16);
    assert_breaks_at_word_boundaries(&prose_drawn, PROSE);
}
