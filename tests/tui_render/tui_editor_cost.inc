//! M005: the editor's draw path, counted.
//!
//! `tui_render_cost.inc` measures the content caches C006 introduced and
//! `tui_frame_cost.inc` the frame composition M001 removed. This member
//! measures the **editor's** draw path: the per-row read and its character
//! walk, the viewport's horizontal clamp, the semantic-token scan, and the
//! highlight cache's prefix check.
//!
//! Every test reads a `RenderCost` snapshot before and after one drawn
//! frame (or one highlight call), so the number it asserts is the work
//! **that operation** did. The counters count operations, not time; no
//! wall-clock claim is made here and none could be reproduced on a shared
//! machine.
//!
//! The tests are written against the *after* state, and the assertions
//! print the count they measured, so on the baseline the failure output is
//! the before-count.

use std::sync::Arc;

use codegg::protocol::lsp::LspSemanticTokenDto;
use codegg::tui::components::editor::EditorWidget;
use codegg::tui::document_session::TuiDocumentPresentation;
use codegg::tui::editor::{gutter_width, EditorMode, EditorPrefix};
use codegg::tui::editor_completion::EditorCompletionState;
use codegg::tui::editor_highlight::EditorHighlight;
use codegg::tui::editor_lsp::{validate_semantic_tokens, EditorLspState};
use codegg::tui::helix_selection::SelectionMode;
use codegg::tui::render_cost::{self, RenderCost};
use codegg::tui::theme::Theme;
use codegg_document::{
    DocumentBuffer, DocumentLimits, DocumentSnapshot, TextEdit, TextTransaction,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// The drawn frame's width. The block's two border columns leave the text
/// area 124 columns, of which the gutter and the scrollbar take a few — so
/// the document is drawn in roughly 120 columns, which [`assert_viewport`]
/// reports per fixture rather than assuming (the gutter widens with the
/// line count).
const WIDTH: u16 = 126;

/// The drawn frame's height. The block's two border rows and its one chrome
/// row leave a **40-row** text area, which the fixtures assert.
const HEIGHT: u16 = 43;

/// The text area's height, asserted by every fixture.
const ROWS: u64 = 40;

/// One editor, as a frame needs it: the presentation the viewport writes
/// back, and the two per-document caches the widget borrows.
struct Fixture {
    presentation: TuiDocumentPresentation,
    lsp: EditorLspState,
    completion: EditorCompletionState,
    highlight: EditorHighlight,
}

impl Fixture {
    fn new() -> Self {
        Self::with_lsp(EditorLspState::default())
    }

    fn with_lsp(lsp: EditorLspState) -> Self {
        Self {
            presentation: TuiDocumentPresentation::default(),
            lsp,
            completion: EditorCompletionState::default(),
            highlight: EditorHighlight::default(),
        }
    }
}

/// Draw one editor frame and hand back the painted buffer.
fn draw(fixture: &mut Fixture, snapshot: &DocumentSnapshot, width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal
        .draw(|frame| {
            EditorWidget {
                path: "src/lib.rs",
                status: "synced",
                notice: None,
                find_note: "",
                count_label: "",
                mode: EditorMode::Normal,
                buffer_focused: true,
                pending_prefix: EditorPrefix::Idle,
                command_active: false,
                command_line: "",
                command_selection: 0,
                search_active: false,
                search_line: "",
                search_count: 0,
                search_partial: false,
                scope: codegg::protocol::document::DocumentScope::default(),
                cursor_byte: fixture.presentation.cursor_byte,
                writable: true,
                busy: false,
                snapshot,
                lsp: &fixture.lsp,
                completion: &fixture.completion,
                highlight: &mut fixture.highlight,
                presentation: &mut fixture.presentation,
                theme: Arc::new(Theme::default()),
            }
            .render(frame, Rect::new(0, 0, width, height));
        })
        .expect("the frame draws");
    terminal.backend().buffer().clone()
}

/// The cost of a piece of work, measured as the difference of two snapshots.
fn cost_of(work: impl FnOnce()) -> RenderCost {
    let before = render_cost::snapshot();
    work();
    render_cost::snapshot().since(&before)
}

/// Assert the fixture really is a 40-row viewport, and hand back the columns
/// the document is drawn in, so a layout change shows up as a broken fixture
/// rather than as a mysterious count.
fn assert_viewport(fixture: &Fixture, snapshot: &DocumentSnapshot) -> u16 {
    assert_eq!(
        fixture.presentation.visible_lines, ROWS as usize,
        "the fixture must draw a 40-row text area"
    );
    (WIDTH - 2)
        .saturating_sub(gutter_width(snapshot.len_lines()))
        .saturating_sub(1)
}

/// Sixty short lines: a viewport of ordinary source.
fn rows_of_short_lines() -> String {
    (0..60)
        .map(|index| format!("let value_{index} = {index};\n"))
        .collect()
}

/// Sixty 4,000-byte lines: long enough that reading one whole is the cost
/// this milestone removes, and short enough that the frame budget renders
/// all forty rows.
fn rows_of_long_lines() -> String {
    (0..60)
        .map(|_| format!("{}\n", "x".repeat(4_000)))
        .collect()
}

/// One 20,000-byte line with no newline: a minified file.
fn minified_line() -> String {
    "x".repeat(20_000)
}

/// A 1,000-line document with one semantic token on each line.
fn rows_with_a_thousand_tokens() -> (DocumentSnapshot, EditorLspState) {
    let text: String = (0..1_000)
        .map(|index| format!("let value_{index} = {index};\n"))
        .collect();
    let snapshot = DocumentBuffer::new(&text).snapshot();
    let tokens: Vec<LspSemanticTokenDto> = (0..1_000)
        .map(|line| LspSemanticTokenDto {
            line,
            character: 0,
            length: 3,
            token_type: "keyword".into(),
            modifiers: Vec::new(),
        })
        .collect();
    let validated = validate_semantic_tokens(&snapshot, tokens);
    assert_eq!(
        validated.accepted.len(),
        1_000,
        "the fixture must hand the widget 1,000 tokens"
    );
    let lsp = EditorLspState {
        semantic_tokens: validated.accepted,
        semantic_revision: Some(1),
        ..EditorLspState::default()
    };
    (snapshot, lsp)
}

/// A 5,000-line document, scanned into the highlighter's cache.
fn five_thousand_lines() -> (DocumentBuffer, EditorHighlight) {
    let text: String = (0..5_000)
        .map(|index| format!("let value_{index} = {index};\n"))
        .collect();
    let buffer = DocumentBuffer::new(&text);
    let mut highlight = EditorHighlight::default();
    // Warm the scan through line 4,899, so the cache has seen the whole file
    // and the prefix check has a full prefix to verify.
    let _ = highlight.spans_for_line(&buffer.snapshot(), "src/lib.rs", "base16-ocean.dark", 4_899);
    (buffer, highlight)
}

// ── WP2: the per-row read and its walk ─────────────────────────────────

/// A warm frame reads each visible row **once**.
///
/// Two passes read the visible window — the viewport's horizontal clamp
/// measures every visible line, and the draw reads every row — so an
/// unchanged frame used to pay the height twice. The clamp is memoized, so
/// the second pass is the only one left.
#[test]
fn a_warm_frame_reads_each_visible_row_once() {
    let snapshot = DocumentBuffer::new(rows_of_short_lines()).snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    let cost = cost_of(|| {
        let _warm = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    assert_eq!(
        cost.editor_row_reads, ROWS,
        "a warm frame reads one window per row, not two: {} reads for {ROWS} rows, \
         with no token scan ({} steps) and no prefix check ({} lines)",
        cost.editor_row_reads, cost.editor_token_scan_steps, cost.highlight_prefix_lines,
    );
    assert_eq!(
        cost.editor_token_scan_steps, 0,
        "a document with no semantic tokens scans none: {} steps",
        cost.editor_token_scan_steps
    );
    assert_eq!(
        cost.highlight_prefix_lines, 0,
        "no edit moved the revision, so there is no prefix to verify: {} lines",
        cost.highlight_prefix_lines
    );
}

/// A row of a long line is read as far as its **window**, not as far as the
/// line.
///
/// The read is `O(drawn columns)` bytes — with the maximum UTF-8 cost of a
/// column as the constant, and one character of headroom for a character
/// that straddles the window's end — rather than `O(line bytes)`.
#[test]
fn a_long_row_is_read_as_far_as_its_window() {
    let snapshot = DocumentBuffer::new(rows_of_long_lines()).snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let columns = assert_viewport(&fixture, &snapshot);

    let cost = cost_of(|| {
        let _warm = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    let bound = ROWS * (u64::from(columns) + 2) * 4;
    assert_eq!(
        cost.editor_row_reads, ROWS,
        "one read per row: {} reads",
        cost.editor_row_reads
    );
    assert!(
        cost.editor_row_bytes <= bound,
        "a 4,000-byte row must be read as far as the {columns} drawn columns, not whole: \
         {} bytes for {ROWS} rows, bound {bound}",
        cost.editor_row_bytes
    );
}

/// A minified line costs its window, not its length.
#[test]
fn a_minified_line_is_read_as_far_as_its_window() {
    let text = minified_line();
    let snapshot = DocumentBuffer::new(&text).snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let columns = assert_viewport(&fixture, &snapshot);

    let cost = cost_of(|| {
        let _warm = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    let bound = (u64::from(columns) + 2) * 4;
    // A file with no trailing newline is two rope lines: the content and the
    // empty line after it. Both are visible, so both are read.
    let drawn = snapshot.len_lines().min(ROWS as usize) as u64;
    assert_eq!(
        cost.editor_row_reads, drawn,
        "one read per drawn row: {} reads for {drawn} rows",
        cost.editor_row_reads
    );
    assert!(
        cost.editor_row_bytes <= bound,
        "a {}-byte line must be read as far as the drawn columns: {} bytes, bound {bound}",
        text.len(),
        cost.editor_row_bytes
    );
}

// ── WP3: the viewport's horizontal clamp ───────────────────────────────

/// A scroll changes the window the clamp covers, so it measures again.
#[test]
fn a_scroll_measures_the_visible_window_again() {
    let snapshot = DocumentBuffer::new(rows_of_short_lines()).snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    fixture.presentation.view_scroll = Some(2);
    let cost = cost_of(|| {
        let _scrolled = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    assert_eq!(
        fixture.presentation.viewport_line, 2,
        "the fixture must actually scroll away from the cursor"
    );
    assert_eq!(
        cost.editor_row_reads,
        ROWS * 2,
        "a scrolled window is measured again and then drawn: {} reads for {ROWS} rows",
        cost.editor_row_reads
    );
}

/// A resize changes the drawn width, so the clamp measures again.
#[test]
fn a_resize_measures_the_visible_window_again() {
    let snapshot = DocumentBuffer::new(rows_of_short_lines()).snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    let cost = cost_of(|| {
        let _resized = draw(&mut fixture, &snapshot, WIDTH + 6, HEIGHT);
    });

    assert_eq!(
        cost.editor_row_reads,
        ROWS * 2,
        "a resized pane is measured again and then drawn: {} reads for {ROWS} rows",
        cost.editor_row_reads
    );
}

/// A document change moves the revision, so the clamp measures again.
#[test]
fn a_revision_change_measures_the_visible_window_again() {
    let mut buffer = DocumentBuffer::new(rows_of_short_lines());
    let snapshot = buffer.snapshot();
    let mut fixture = Fixture::new();
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    let edited = buffer
        .apply(
            &TextTransaction::new(vec![TextEdit::new(0..0, "// a comment line\n")]),
            DocumentLimits::default(),
        )
        .expect("the fixture's edit applies")
        .snapshot;
    let cost = cost_of(|| {
        let _warm = draw(&mut fixture, &edited, WIDTH, HEIGHT);
    });

    assert_eq!(
        cost.editor_row_reads,
        ROWS * 2,
        "an edited document is measured again and then drawn: {} reads for {ROWS} rows",
        cost.editor_row_reads
    );
}

/// A horizontal scroll re-applies the clamp without re-measuring anything.
///
/// The clamp's *result* is re-resolved on every frame — the cursor's column
/// is clamped against the longest line every time — but the longest line's
/// width cannot change while the revision, the first line, the height and
/// the width are the same, so the measurement is not repeated.
#[test]
fn a_horizontal_scroll_does_not_measure_the_window_again() {
    let snapshot = DocumentBuffer::new(rows_of_long_lines()).snapshot();
    let mut fixture = Fixture::new();
    fixture.presentation.cursor_byte = 10;
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    fixture.presentation.cursor_byte = 1_000;
    let cost = cost_of(|| {
        let _scrolled = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    assert!(
        fixture.presentation.viewport_column > 0,
        "the fixture must actually scroll horizontally"
    );
    assert_eq!(
        cost.editor_row_reads, ROWS,
        "a horizontal re-resolution draws the rows and measures nothing: \
         {} reads for {ROWS} rows",
        cost.editor_row_reads
    );
}

// ── WP4: the semantic-token scan ───────────────────────────────────────

/// A row's token scan is bounded by the tokens **on the row**, plus the
/// binary search that finds them.
///
/// The scan was a linear filter over every token in the document for every
/// visible row: 1,000 tokens over forty rows cost 40,000 comparisons. It is
/// now `O(log tokens + tokens on the row)`, so the same frame costs the
/// logarithm and forty tokens.
#[test]
fn a_rows_token_scan_is_bounded_by_the_tokens_on_it() {
    let (snapshot, lsp) = rows_with_a_thousand_tokens();
    let mut fixture = Fixture::with_lsp(lsp);
    let _cold = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    let _columns = assert_viewport(&fixture, &snapshot);

    let cost = cost_of(|| {
        let _warm = draw(&mut fixture, &snapshot, WIDTH, HEIGHT);
    });

    // log2(1,000) is ten probes; the row itself carries one token.
    let bound = ROWS * 12;
    assert!(
        cost.editor_token_scan_steps <= bound,
        "a row's scan must visit the row's tokens, not the document's: {} steps \
         for {ROWS} rows over 1,000 tokens, bound {bound}",
        cost.editor_token_scan_steps
    );
}

// ── WP5: the highlight cache's prefix check ────────────────────────────

/// An edit near the end of a 5,000-line document does not re-read the whole
/// prefix.
///
/// The check's job is to find the **first** line whose content differs from
/// the scan the cache holds. Starting at line 0 answers it by re-hashing
/// every scanned line — 4,900 of them, per keystroke. An edit knows where it
/// landed, so the check starts there.
#[test]
fn an_edit_near_the_end_does_not_reread_the_prefix() {
    for (line, expected) in [(10u32, 1u64), (2_500, 1), (4_900, 0)] {
        let (mut buffer, mut highlight) = five_thousand_lines();
        // A zero-length insertion leaves the line count and every later line
        // index exactly where they were, so the seed's line is the line the
        // change is on.
        let start = offset_of_line(line as usize);
        let edited = buffer
            .apply(
                &TextTransaction::new(vec![TextEdit::new(start..start, "//")]),
                DocumentLimits::default(),
            )
            .expect("the fixture's edit applies")
            .snapshot;
        highlight.note_edit_from(line as usize);

        let cost = cost_of(|| {
            let _ =
                highlight.spans_for_line(&edited, "src/lib.rs", "base16-ocean.dark", line as usize);
        });

        assert!(
            cost.highlight_prefix_lines <= expected,
            "an edit on line {line} of 5,000 must verify from the edit, not from \
             line 0: {} lines checked, bound {expected}",
            cost.highlight_prefix_lines
        );
    }
}

/// An edit the presentation cannot place still scans from the top.
///
/// This is the direction that must never be optimised away: a change the
/// editor did not make — a daemon-side edit, a reload — arrives as a new
/// revision with no line attached, and a cache that assumed "no seed means
/// nothing changed above the scan" would serve the previous colours.
#[test]
fn an_unplaced_edit_still_scans_from_the_top() {
    let (mut buffer, mut highlight) = five_thousand_lines();
    let start = offset_of_line(10);
    let edited = buffer
        .apply(
            &TextTransaction::new(vec![TextEdit::new(start..start, "//")]),
            DocumentLimits::default(),
        )
        .expect("the fixture's edit applies")
        .snapshot;

    let cost = cost_of(|| {
        let _ = highlight.spans_for_line(&edited, "src/lib.rs", "base16-ocean.dark", 10);
    });

    assert!(
        cost.highlight_prefix_lines >= 11,
        "an edit with no seed must verify the prefix it cannot rule out: {} lines",
        cost.highlight_prefix_lines
    );
}

/// The byte offset where `line` starts.
///
/// Every fixture line is `let value_N = N;\n`, so the offsets are the sum of
/// the previous lines' lengths — recomputed from that shape rather than
/// assumed, so a fixture change cannot silently seed the wrong line.
fn offset_of_line(line: usize) -> usize {
    (0..line)
        .map(|index| format!("let value_{index} = {index};\n").len())
        .sum()
}

// ── WP8: the rendering is unchanged ────────────────────────────────────

/// Every fixture state draws the same buffer from a warm cache as from a
/// cold one.
///
/// The warm fixture has already drawn three other states — the top of the
/// document, a scroll and a resize — so its caches hold rows and a clamp the
/// target state did not produce. A cache that served them would show up here
/// as a difference between the two buffers.
#[test]
fn every_fixture_state_draws_the_same_buffer_warm_as_cold() {
    let short = DocumentBuffer::new(rows_of_short_lines()).snapshot();
    let long = DocumentBuffer::new(rows_of_long_lines()).snapshot();
    let minified = DocumentBuffer::new(minified_line()).snapshot();
    let (tokened, lsp) = rows_with_a_thousand_tokens();
    let clipped = DocumentBuffer::new(multi_byte_lines()).snapshot();
    let truncated = DocumentBuffer::new(format!("{}\nshort line\n", "y".repeat(40_000))).snapshot();

    for (name, snapshot, tokens) in [
        ("a short file", &short, EditorLspState::default()),
        ("a long line", &long, EditorLspState::default()),
        ("a minified line", &minified, EditorLspState::default()),
        ("semantic tokens", &tokened, lsp.clone()),
        ("a multi-byte clip", &clipped, EditorLspState::default()),
        (
            "the truncation suffix",
            &truncated,
            EditorLspState::default(),
        ),
    ] {
        let middle = snapshot.len_bytes().min(300);
        for (label, state) in [
            ("at the top", presentation_at(0)),
            ("in the middle", presentation_at(middle)),
            ("at the end", presentation_at(snapshot.len_bytes())),
            ("with a held selection", selected_from(snapshot, 0, middle)),
        ] {
            same_warm_as_cold(&format!("{name}, {label}"), snapshot, &tokens, &state);
        }
    }
}

/// A presentation with the cursor at `cursor` and nothing else set.
fn presentation_at(cursor: usize) -> TuiDocumentPresentation {
    TuiDocumentPresentation {
        cursor_byte: cursor,
        ..TuiDocumentPresentation::default()
    }
}

/// A presentation holding one character-mode selection from `anchor` to
/// `head`, with the cursor on the head — the state the selection and cursor
/// layers of the draw are in when a reader is about to act on a range.
fn selected_from(
    snapshot: &DocumentSnapshot,
    anchor: usize,
    head: usize,
) -> TuiDocumentPresentation {
    let point = |byte: usize| codegg::tui::editor::EditorPoint {
        byte,
        line: codegg::tui::editor::cursor_line(snapshot, byte),
    };
    let mut selection =
        codegg::tui::helix_selection::Selection::new(point(anchor), SelectionMode::Char);
    selection.head = point(head);
    TuiDocumentPresentation {
        cursor_byte: head,
        selections: vec![selection],
        ..TuiDocumentPresentation::default()
    }
}

/// Lines whose 120-column clip lands on a multi-byte character.
///
/// Each line is thirty repeats of `漢字テキスト`: every character is one
/// column and three bytes, so the window's end at column 120 falls inside
/// the 121st character — the clip has to keep the character that starts
/// before it and ends after it.
fn multi_byte_lines() -> String {
    (0..40)
        .map(|index| format!("{}{index}\n", "漢字テキスト".repeat(30)))
        .collect()
}

/// Draw `snapshot` in `state` twice — once from a cold cache, once from a
/// cache that has drawn other states — and assert the frames are identical.
fn same_warm_as_cold(
    name: &str,
    snapshot: &DocumentSnapshot,
    lsp: &EditorLspState,
    state: &TuiDocumentPresentation,
) {
    let mut cold = Fixture::with_lsp(lsp.clone());
    cold.presentation = state.clone();
    let cold_frame = draw(&mut cold, snapshot, WIDTH, HEIGHT);

    let mut warm = Fixture::with_lsp(lsp.clone());
    // Three unrelated frames first: the top of the document, a scroll, and a
    // resize, so both caches have moved before the state under test.
    let _ = draw(&mut warm, snapshot, WIDTH, HEIGHT);
    warm.presentation.view_scroll = Some(3);
    let _ = draw(&mut warm, snapshot, WIDTH, HEIGHT);
    let _ = draw(&mut warm, snapshot, WIDTH + 6, HEIGHT);

    warm.presentation = state.clone();
    let warm_frame = draw(&mut warm, snapshot, WIDTH, HEIGHT);

    assert_eq!(
        cold_frame, warm_frame,
        "{name}: the warm frame drew something the cold frame did not"
    );
}

/// Every fixture actually draws its document.
///
/// The warm-versus-cold comparison below proves the caches agree with each
/// other; it cannot prove that either of them draws anything. A row whose
/// read was refused — a byte budget that split a character, say — leaves the
/// row blank, and two blank rows compare equal. This is the independent half:
/// the glyphs on screen are the document's.
#[test]
fn every_fixture_draws_its_document() {
    let short = DocumentBuffer::new(rows_of_short_lines()).snapshot();
    let long = DocumentBuffer::new(rows_of_long_lines()).snapshot();
    let minified = DocumentBuffer::new(minified_line()).snapshot();
    let (tokened, lsp) = rows_with_a_thousand_tokens();
    let clipped = DocumentBuffer::new(multi_byte_lines()).snapshot();
    let truncated = DocumentBuffer::new(format!("{}\nshort line\n", "y".repeat(40_000))).snapshot();

    for (name, snapshot, tokens, needle) in [
        (
            "a short file",
            &short,
            EditorLspState::default(),
            "let value_0 = 0;",
        ),
        ("a long line", &long, EditorLspState::default(), "xxxx"),
        (
            "a minified line",
            &minified,
            EditorLspState::default(),
            "xxxxxxxx",
        ),
        ("semantic tokens", &tokened, lsp.clone(), "let value_0 = 0;"),
        // The terminal buffer pads a wide glyph with a trailing space, so a
        // multi-character needle cannot match across it.
        (
            "a multi-byte clip",
            &clipped,
            EditorLspState::default(),
            "漢",
        ),
        (
            "the truncation suffix",
            &truncated,
            EditorLspState::default(),
            "too long",
        ),
    ] {
        let mut fixture = Fixture::with_lsp(tokens);
        let drawn = draw(&mut fixture, snapshot, WIDTH, HEIGHT);
        let text = buffer_text(&drawn);
        assert!(
            text.contains(needle),
            "{name}: the frame must draw its document; expected {needle:?} in:\n{text}"
        );
    }
}

/// A frame's glyphs, row by row.
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

/// A warm frame after an edit draws what a cold frame draws.
///
/// This is the clamp cache's correctness case with the **revision** moving
/// under it: the edit makes the document longer, so the horizontal clamp has
/// to re-measure and let the view scroll further right. A cache that kept
/// the old width would clamp the scroll at the old document's edge — the
/// view can no longer reach the end of a line that grew.
#[test]
fn a_frame_after_an_edit_draws_what_a_cold_frame_draws() {
    let mut buffer = DocumentBuffer::new("x".repeat(200));
    let before = buffer.snapshot();
    let mut warm = Fixture::new();
    warm.presentation.cursor_byte = 190;
    let _ = draw(&mut warm, &before, 46, 13);

    let after = buffer
        .replace_text("x".repeat(600))
        .expect("the fixture's replacement applies");
    warm.presentation.cursor_byte = 590;
    let warm_frame = draw(&mut warm, &after, 46, 13);

    let mut cold = Fixture::new();
    cold.presentation.cursor_byte = 590;
    let cold_frame = draw(&mut cold, &after, 46, 13);

    assert!(
        cold.presentation.viewport_column > 0,
        "the fixture must scroll horizontally for the clamp to bind"
    );
    assert_eq!(
        cold_frame, warm_frame,
        "the clamp kept the pre-edit width and could not scroll to the new edge"
    );
}
