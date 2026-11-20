//! M006: the shared wrap engines, counted.
//!
//! `tui_render_cost.inc` measures the caches C006 introduced,
//! `tui_frame_cost.inc` the frame composition M001 removed, and the M002–M005
//! members the surfaces above them. This member measures the **wrap engines**
//! themselves — `wrap_styled_spans_by_word_rows` and `wrap_to_strings`, which
//! the composer, the main buffer and the chat body all share:
//!
//! * `wrap_span_rescans` — spans the styled wrap's lookup examined. It used to
//!   rescan the span list from index 0 for every word and every separator, so
//!   the work was `words x spans`; one cursor now walks the spans beside the
//!   segments, which `tokenize` emits in source order.
//! * `wrap_width_measures` — display-width passes the styled item builder
//!   made over a word's pieces. `chunk_word` used to measure every piece a
//!   second time to learn a width the builder already knew.
//!
//! Both counts are the work of **one** call, read as the difference of two
//! `RenderCost` snapshots, so the number a test asserts is the operation's own
//! work and not whatever else ran on the thread.
//!
//! The tests are written against the *after* state, and each failure message
//! prints the count it measured, so on the pre-M006 engine the failure output
//! is the before-count.

use std::sync::Arc;

use codegg::tui::components::prompt::PromptWidget;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::theme::Theme;
use codegg::tui::wrap::wrap_styled_spans_by_word_rows;
use ratatui::layout::Rect;
use ratatui::prelude::Buffer;
use ratatui::text::Span;
use ratatui::widgets::Widget;

/// Plain prose as one span: the shape the composer hands the engine.
const PROSE: &str = "the quick brown fox jumps over the lazy dog";

/// A word wider than any width below, so every wrap hard-breaks it.
const LONG_WORD: &str = "abcdefghij";

/// The cost of a piece of work, measured as the difference of two snapshots.
fn cost_of(work: impl FnOnce()) -> RenderCost {
    let before = render_cost::snapshot();
    work();
    render_cost::snapshot().since(&before)
}

/// `count` words, one span each, each span holding its word **and** the space
/// that follows it — the shape a styled transcript line has when every word
/// carries its own style run.
fn spans_of(count: usize) -> Vec<Span<'static>> {
    (0..count)
        .map(|index| {
            let word = format!("w{index:03}");
            if index + 1 == count {
                Span::raw(word)
            } else {
                Span::raw(format!("{word} "))
            }
        })
        .collect()
}

/// A paragraph whose middle word is an OSC-8 hyperlink: the sequence's bytes
/// are on no cell, so the word measures a URL and not a sequence.
fn osc8_paragraph() -> Vec<Span<'static>> {
    let url = "https://example.invalid/a/long/path";
    vec![
        Span::raw("read "),
        Span::raw(format!("\x1b]8;;{url}\x07{url}\x1b]8;;\x07")),
        Span::raw(" before the review"),
    ]
}

/// The most span examinations a **linear** lookup may perform for a source of
/// `spans` spans that asks `queries` questions of them.
///
/// A query is one word, one separator or one escape sequence: each asks the
/// cursor for the span it starts in, and the cursor answers by advancing over
/// the spans that end before it and reading the one that ends after it. Every
/// span is advanced over once for the whole wrap, so the count is at most
/// `spans + queries` — `O(spans + words)`, where the front rescan paid
/// `words x spans`.
fn rescan_bound(spans: usize, queries: usize) -> u64 {
    (spans + queries) as u64
}

/// One span is one examination per word and per separator, and that is what
/// it was before the change: a single span has nothing to rescan, so this
/// fixture is the **flat** case rather than the evidence of the win.
#[test]
fn one_span_costs_one_examination_per_word_and_separator() {
    let spans = vec![Span::raw(PROSE)];
    let words = PROSE.split_whitespace().count();
    let separators = words - 1;

    let cost = cost_of(|| {
        let _rows = wrap_styled_spans_by_word_rows(&spans, 20);
    });

    assert!(
        cost.wrap_span_rescans <= rescan_bound(1, words + separators),
        "one span costs one examination per word and per separator, and that is \
         all: {} examinations for 1 span, {words} words and {separators} \
         separators",
        cost.wrap_span_rescans,
    );
}

/// Twenty spans are examined once each, not once per word.
///
/// The pre-M006 lookup started at index 0 for every word and every separator,
/// so word `i` examined `i + 1` spans and the wrap examined 420 of them. The
/// cursor advances with the segments, so the same wrap examines 59.
#[test]
fn twenty_spans_are_examined_once_each() {
    let spans = spans_of(20);
    let words = 20;
    let queries = words + (words - 1);

    let cost = cost_of(|| {
        let _rows = wrap_styled_spans_by_word_rows(&spans, 30);
    });

    assert!(
        cost.wrap_span_rescans <= rescan_bound(spans.len(), queries),
        "twenty spans cost one examination each, not a rescan from the front \
         for every word: {} examinations for {} spans and {words} words",
        cost.wrap_span_rescans,
        spans.len(),
    );
}

/// A hundred spans are examined once each: the lookup is linear in the span
/// list, not quadratic in it.
///
/// The counted failure on the pre-M006 engine is `10100` examinations for
/// this fixture — `words x spans` with the separators counted too.
#[test]
fn a_hundred_spans_are_examined_once_each() {
    let spans = spans_of(100);
    let words = 100;
    let queries = words + (words - 1);

    let cost = cost_of(|| {
        let _rows = wrap_styled_spans_by_word_rows(&spans, 30);
    });

    assert!(
        cost.wrap_span_rescans <= rescan_bound(spans.len(), queries),
        "a hundred spans cost one examination each, not a rescan from the \
         front for every word: {} examinations for {} spans and {words} words",
        cost.wrap_span_rescans,
        spans.len(),
    );
}

/// A word that has to be hard-broken is measured once, not once per fragment.
///
/// The builder measures the piece; the chunker used to measure the piece again
/// and then measure every fragment it cut — four passes for this fixture, and
/// one after the change.
#[test]
fn a_hard_broken_word_is_measured_once() {
    let spans = vec![Span::raw(LONG_WORD)];

    let cost = cost_of(|| {
        let rows = wrap_styled_spans_by_word_rows(&spans, 4);
        assert_eq!(
            rows.iter().map(|row| row.text()).collect::<Vec<_>>(),
            vec!["abcd", "efgh", "ij"],
            "the fixture must hard-break the word: the wrap is what is counted"
        );
    });

    assert_eq!(
        cost.wrap_width_measures, 1,
        "a hard-broken word is measured once, where the chunker measured every \
         fragment again: {} measurements",
        cost.wrap_width_measures,
    );
}

/// A paragraph with an OSC-8 hyperlink is measured once per word, and the
/// sequence stays whole.
#[test]
fn an_osc8_paragraph_is_measured_once_per_word() {
    let spans = osc8_paragraph();
    let words: usize = 5;

    let cost = cost_of(|| {
        let rows = wrap_styled_spans_by_word_rows(&spans, 20);
        let rendered: String = rows.iter().map(|row| row.text()).collect();
        assert!(
            rendered.contains("\x1b]8;;https://example.invalid/a/long/path\x07"),
            "the hyperlink's opener must survive the wrap whole: {rendered:?}"
        );
        assert_eq!(
            rendered.matches("\x1b]8;;\x07").count(),
            1,
            "the hyperlink's closer must appear exactly once: {rendered:?}"
        );
    });

    assert_eq!(
        cost.wrap_width_measures, words as u64,
        "one measurement per word, not one per word plus one per piece again: \
         {} measurements for {words} words",
        cost.wrap_width_measures,
    );
    // Five words, the four runs between them, and the hyperlink's two escape
    // sequences: every one of them asks the cursor for a span.
    assert!(
        cost.wrap_span_rescans <= rescan_bound(spans.len(), words + 4 + 2),
        "the paragraph's {} spans are examined once each: {} examinations for \
         {words} words",
        spans.len(),
        cost.wrap_span_rescans,
    );
}

/// A cold composer frame measures each word once.
///
/// This is the count on the **real** path: the composer's layout cache is cold
/// on the first frame, so the frame performs one wrap, and that wrap is the
/// one this counts.
#[test]
fn a_cold_composer_frame_measures_each_word_once() {
    let words = PROSE.split_whitespace().count();

    let cost = cost_of(|| {
        let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
        widget.set_text(PROSE.to_string());
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);
    });

    assert_eq!(
        cost.composer_wraps, 1,
        "the cold frame must wrap the composer once for this count to be the \
         wrap's: {} wraps",
        cost.composer_wraps,
    );
    assert_eq!(
        cost.wrap_width_measures, words as u64,
        "the composer's frame measures each word once: {} measurements for \
         {words} words",
        cost.wrap_width_measures,
    );
    assert!(
        cost.wrap_span_rescans <= rescan_bound(1, words + (words - 1)),
        "the composer hands the engine one span: {} examinations for {words} \
         words",
        cost.wrap_span_rescans,
    );
}
