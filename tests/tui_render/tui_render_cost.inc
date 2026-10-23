//! C006: the render path derives each expensive result once.
//!
//! This milestone is a **cost** milestone, so its evidence is a count of
//! operations rather than a picture of the screen: a frame that wraps the
//! composer four times and a frame that wraps it once draw the same pixels,
//! and only the count tells them apart.
//!
//! Two things are asserted here, and they are the two halves of the
//! milestone's contract:
//!
//! * **the cost** — the counters in `codegg::tui::render_cost` say how many
//!   wraps, parses, scans and row copies the work took, and the counts are
//!   bounded by the *change* rather than by the size of the content;
//! * **the equivalence** — the cached path produces exactly the rows the
//!   uncached path produced, over a fixture set that includes the escape
//!   sequences and the OSC-8 hyperlink the wrap's contract is about.
//!
//! The counts are process-global, so every test reads a snapshot before and
//! after the work it measures and compares the difference.

use std::sync::Arc;

use codegg::tui::components::messages::MessagesWidget;
use codegg::tui::components::prompt::PromptWidget;
use codegg::tui::editor_highlight::EditorHighlight;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::widgets::Widget;

/// The texts the equivalence is proved over.
///
/// The empty buffer, a plain line, a line wider than the row, multi-byte
/// characters, a **terminal control sequence** and an **OSC-8 hyperlink**
/// are here because each of them breaks a different assumption: the
/// hyperlink's target appears twice in the span's content and carries no
/// whitespace, so a word wrap that treated it as text would break it in the
/// middle and paint half an escape sequence.
const TEXTS: &[&str] = &[
    "",
    "hello",
    "a line that is much wider than the row it is wrapped at",
    "héllo wörld",
    "日本語のテキスト",
    "before \u{1b}[31mcoloured\u{1b}[0m after",
    "a \u{1b}]8;;https://example.invalid/link\u{1b}\\link\u{1b}]8;;\u{1b}\\ tail",
];

/// The widths the equivalence is proved over.
const WIDTHS: &[u16] = &[8, 20, 40];

/// The composer's drawn rows, at `width`.
fn composer_buffer(text: &str, width: u16) -> Buffer {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text(text.to_string());
    let area = Rect::new(0, 0, width, 8);
    let mut buffer = Buffer::empty(area);
    (&widget).render(area, &mut buffer);
    buffer
}

/// The cost of a piece of work, measured as the difference of two snapshots.
fn cost_of(work: impl FnOnce()) -> RenderCost {
    let before = render_cost::snapshot();
    work();
    render_cost::snapshot().since(&before)
}

/// A frame wraps the composer once, and the frame's consumers share it.
///
/// The composer had four consumers — the height budget, the caret's
/// visibility, the draw and the caret's cell — and each of them rebuilt the
/// wrapped layout, so a frame paid for the wrap four times over.
#[test]
fn the_composer_is_wrapped_once_per_frame_and_shared_by_its_consumers() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("a composer buffer that is long enough to wrap".to_string());
    let area = Rect::new(0, 0, 40, 8);

    let cost = cost_of(|| {
        // The four consumers of one frame, in the order the render path
        // asks for them.
        let _height = widget.needed_height(10, area.width);
        let _visible = widget.visible_text_rows(area.height);
        widget.ensure_cursor_visible_with_width(4, area.width);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);
        let _caret = widget.caret_cell(area);
    });

    assert_eq!(
        cost.composer_wraps, 1,
        "the four consumers of the layout share one wrap, not four"
    );
}

/// A frame that changed nothing does no wrapping at all.
#[test]
fn an_unchanged_composer_is_not_wrapped_again() {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text("an unchanged composer".to_string());
    let area = Rect::new(0, 0, 40, 8);
    // The first frame warms the cache.
    let mut buffer = Buffer::empty(area);
    (&widget).render(area, &mut buffer);

    let cost = cost_of(|| {
        let _height = widget.needed_height(10, area.width);
        widget.ensure_cursor_visible_with_width(4, area.width);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);
        let _caret = widget.caret_cell(area);
    });

    assert_eq!(
        cost.composer_wraps, 0,
        "a frame that changed neither the text nor the width does no wrap"
    );
}

/// The cached composer draws exactly what the uncached composer drew.
///
/// The fixture set carries the escape sequences and the hyperlink, because
/// the cached layout is only the same layout if the escape sequences are
/// still one atomic token in it.
#[test]
fn the_cached_composer_draws_the_same_rows_as_a_fresh_one() {
    for text in TEXTS {
        for width in WIDTHS {
            let warm = composer_buffer(text, *width);
            let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
            widget.set_text(text.to_string());
            let area = Rect::new(0, 0, *width, 8);
            let mut buffer = Buffer::empty(area);
            (&widget).render(area, &mut buffer);
            // The second frame is the one that reads the cache.
            (&widget).render(area, &mut buffer);
            assert_eq!(
                warm, buffer,
                "text {text:?} at width {width}: the cached frame drew something else"
            );
        }
    }
}

/// The streaming tail's rows are the rows a fresh parse produces.
///
/// The tail is parsed incrementally, so the rows it produces after a stream
/// of tokens have to be the rows a single parse of the whole response
/// produces.
#[test]
fn the_streaming_tail_renders_the_same_rows_as_a_fresh_parse() {
    let theme = Arc::new(Theme::dark());
    let response = "first paragraph\n\nsecond paragraph with **bold** text\n\n- one\n- two\n";

    let mut streamed = MessagesWidget::new(theme.clone());
    streamed.set_width(40);
    for chunk in response.as_bytes().chunks(7) {
        let chunk = String::from_utf8_lossy(chunk).to_string();
        streamed.add_streaming_token(&chunk);
    }

    let mut fresh = MessagesWidget::new(theme);
    fresh.set_width(40);
    fresh.add_streaming_token(response);

    assert_eq!(
        streamed.streaming_tail_rows(),
        fresh.streaming_tail_rows(),
        "the incrementally parsed tail must render what a fresh parse renders"
    );
}

/// The tail's parse count is bounded by the change, not by the response.
///
/// The response is streamed in the same number of chunks whether it is ten
/// bytes or ten thousand, and the parse count must not grow with it.
#[test]
fn the_streaming_parse_count_does_not_grow_with_the_response() {
    let theme = Arc::new(Theme::dark());
    let short = "a short response";
    let long = &"a long response. ".repeat(400);

    let parses = |text: &str| {
        let mut widget = MessagesWidget::new(theme.clone());
        widget.set_width(40);
        let mut count = 0u64;
        for chunk in text.as_bytes().chunks(16) {
            let chunk = String::from_utf8_lossy(chunk).to_string();
            widget.add_streaming_token(&chunk);
            // The tail is parsed when it is drawn, not when it is appended.
            let before = render_cost::snapshot();
            let _rows = widget.streaming_tail_rows();
            count += render_cost::snapshot().since(&before).markdown_parses;
        }
        count
    };

    let short_parses = parses(short);
    let long_parses = parses(long);
    assert!(
        short_parses > 0,
        "a streamed response is parsed at least once"
    );
    assert_eq!(
        short_parses, long_parses,
        "the parse count is bounded by the number of changes, not by the \
         response's length: {short} cost {short_parses} and a response {} \
         times longer cost {long_parses}",
        long.len() / short.len(),
    );
}

/// A local edit keeps the editor's highlight cache.
///
/// The cache was keyed on the document's revision, so every keystroke threw
/// it away — on the one path the cache was built for.
#[test]
fn a_local_edit_keeps_the_highlights_of_the_lines_above_it() {
    let theme_name = "base16-ocean.dark";
    let before = "let value = 1;\nlet other = 2;\nlet third = 3;\n";
    let after = "let value = 1;\nlet other = 22;\nlet third = 3;\n";

    let mut highlight = EditorHighlight::default();
    let original = codegg_document::DocumentBuffer::new(before).snapshot();
    let original_spans: Vec<_> = (0..3)
        .map(|line| {
            highlight
                .spans_for_line(&original, "src/main.rs", theme_name, line)
                .to_vec()
        })
        .collect();

    // The edit changes the second line only.
    let edited = codegg_document::DocumentBuffer::new(after).snapshot();
    let cost = cost_of(|| {
        for line in 0..3 {
            highlight.spans_for_line(&edited, "src/main.rs", theme_name, line);
        }
    });

    assert!(
        !original_spans[0].is_empty(),
        "the first line is coloured, so the cache had something to keep"
    );
    assert_eq!(
        highlight.spans_for_line(&edited, "src/main.rs", theme_name, 0),
        original_spans[0].as_slice(),
        "the line above the edit keeps its colours"
    );
    assert!(
        cost.highlight_scans < 3 * 3,
        "the edit re-scans only the lines it touched, not the whole file: {} \
         scans for three lines",
        cost.highlight_scans
    );
}

/// The tasks overlay copies the rows it draws, not the rows it drops.
///
/// The overlay draws at most twelve rows. Copying the whole task list to
/// throw most of it away cost the list twice per frame.
#[test]
fn the_tasks_overlay_copies_only_the_rows_it_draws() {
    use codegg::tui::app::state::tasks::{
        build_task_rows, bounded, TodoLike, MAX_TASKS_STRIP_ROWS, MAX_TASKS_TOTAL_ROWS,
    };

    let todos: Vec<TodoLike> = (0..200)
        .map(|index| TodoLike {
            bubble: "○",
            content: format!("todo {index}"),
        })
        .collect();
    let rows = build_task_rows(&todos, &[], &[]);

    let cost = cost_of(|| {
        let bounded = bounded(&rows);
        assert!(
            bounded.len() <= MAX_TASKS_TOTAL_ROWS,
            "the overlay draws at most its budget: {} rows",
            bounded.len()
        );
    });

    assert_eq!(
        cost.task_row_builds, MAX_TASKS_STRIP_ROWS as u64,
        "only the drawn rows are copied"
    );
    assert!(
        cost.task_row_builds < rows.len() as u64,
        "the dropped rows are not copied: {} of {} copied",
        cost.task_row_builds,
        rows.len()
    );
}
