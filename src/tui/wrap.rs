//! Shared display-width-aware word wrapping for TUI text surfaces.
//!
//! `ratatui::widgets::Paragraph::line_count` is gated behind the unstable
//! `rendered-line-info` feature, so row-accurate wrapped scrolling cannot be
//! delegated to the widget. Both the chat viewport and the read-only log
//! dialog therefore pre-wrap their content with these helpers: the same
//! algorithm decides both how many rows a string occupies and which rows to
//! draw, which is what keeps scroll offsets aligned with what is on screen.
//!
//! Two engines live here. [`wrap_to_strings`] wraps plain text by words and
//! returns plain strings. [`wrap_styled_spans_by_word`] wraps **styled**
//! spans by the same rule and additionally reports the **byte range** of the
//! source behind every row, so a consumer can map a byte offset back to the
//! row and column it came from. Both engines exclude terminal escape
//! sequences from word segmentation and from width measurement.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Return the number of visual lines produced by [`wrap_to_strings`]. Keeping
/// estimation on the rendering path prevents scroll/layout drift when display
/// width differs from the number of Unicode scalar values.
pub fn wrap_count(s: &str, width: u16) -> usize {
    wrap_to_strings(s, width).len()
}

/// Split a string into wrapped lines of at most `width` display columns each.
/// Returns one entry per visual line (preserves explicit newlines as line
/// breaks). Matches the line-counting semantics of [`wrap_count`].
///
/// Word-wrap is greedy: words that fit on the current line stay there; a
/// word that would overflow the current line starts a new line. Words
/// longer than `width` (URLs, long paths) are hard-broken at character
/// boundaries to avoid overflowing the render area. Trailing whitespace is
/// trimmed from each output line so the wrap doesn't double-space between
/// words or leave stray spaces at line ends.
pub fn wrap_to_strings(s: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    if width == 0 || s.is_empty() {
        return vec![s.to_string()];
    }
    fn hard_break_word(word: &str, width: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut chunk = String::new();
        let mut chunk_width = 0;
        for ch in word.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch_width > 0 && chunk_width + ch_width > width && !chunk.is_empty() {
                chunks.push(std::mem::take(&mut chunk));
                chunk_width = 0;
            }
            chunk.push(ch);
            chunk_width += ch_width;
        }
        if !chunk.is_empty() || chunks.is_empty() {
            chunks.push(chunk);
        }
        chunks
    }

    fn wrap_logical_line(line: &str, width: usize) -> Vec<String> {
        let mut wrapped = Vec::new();
        let mut current = String::new();
        let mut current_width = 0;
        let mut word = String::new();

        let flush_word = |word: &mut String,
                          current: &mut String,
                          current_width: &mut usize,
                          wrapped: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            let chunks = hard_break_word(word, width);
            let word_width = chunks
                .first()
                .map(|chunk| {
                    chunk
                        .chars()
                        .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                        .sum::<usize>()
                })
                .unwrap_or(0);
            let is_hard_break = chunks.len() > 1;

            if current.is_empty() {
                // Start a new line below. The final chunk remains current so
                // a following word can still share its line.
            } else if !is_hard_break && *current_width + 1 + word_width <= width {
                current.push(' ');
                *current_width += 1;
            } else {
                wrapped.push(std::mem::take(current));
                *current_width = 0;
            }

            for (idx, chunk) in chunks.iter().enumerate() {
                if idx + 1 == chunks.len() {
                    current.push_str(chunk);
                    *current_width += chunk
                        .chars()
                        .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
                        .sum::<usize>();
                } else {
                    wrapped.push(chunk.clone());
                }
            }
            word.clear();
        };

        for ch in line.chars() {
            if ch.is_whitespace() {
                flush_word(&mut word, &mut current, &mut current_width, &mut wrapped);
            } else {
                word.push(ch);
            }
        }
        flush_word(&mut word, &mut current, &mut current_width, &mut wrapped);
        wrapped.push(current);
        wrapped
    }

    let mut lines = Vec::new();
    let logical_lines = s.strip_suffix('\n').unwrap_or(s).split('\n');
    for logical_line in logical_lines {
        lines.extend(wrap_logical_line(logical_line, width));
    }
    lines
}

/// The ASCII escape that introduces a terminal control sequence.
const ESC: u8 = 0x1b;
/// The ASCII bell that terminates an OSC control sequence.
const BEL: u8 = 0x07;

/// One styled slice of a wrapped row.
///
/// A slice is the smallest unit the wrap emits: a contiguous run of the
/// source that shares one style, together with the byte range of the source
/// it was produced from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSlice {
    /// The rendered text of this slice.
    pub text: String,
    /// The style the source span carried.
    pub style: Style,
    /// Byte offset of this slice's first byte in the source text.
    pub source_start: usize,
    /// Byte offset one past this slice's last byte in the source text.
    pub source_end: usize,
}

impl StyledSlice {
    /// The slice's display width in terminal columns.
    ///
    /// Escape sequences are excluded, so a slice that carries an OSC-8
    /// hyperlink measures only the text the terminal actually shows.
    pub fn display_width(&self) -> usize {
        visible_width(&self.text)
    }
}

/// One visual row of a styled word wrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedRow {
    /// The row's styled slices, in display order.
    pub slices: Vec<StyledSlice>,
    /// Byte offset of the first source byte behind this row.
    pub source_start: usize,
    /// Byte offset one past the last source byte behind this row.
    pub source_end: usize,
    /// The row's display width in terminal columns.
    pub width: usize,
}

impl WrappedRow {
    /// The row's text, escape sequences included verbatim.
    pub fn text(&self) -> String {
        self.slices
            .iter()
            .map(|slice| slice.text.as_str())
            .collect()
    }
}

/// The display width of `s` in terminal columns, ignoring escape sequences.
///
/// A terminal control sequence occupies no columns: the printable parts of an
/// OSC-8 opener (`ESC ] 8 ; ; <url> BEL`) are *parameters* of the sequence,
/// not text. Measuring the raw bytes counted the hyperlink target as visible
/// text, so a URL measured roughly `2n + 8` columns instead of `n` and broke
/// at half the available width.
pub fn visible_width(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut width = 0usize;
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == ESC {
            idx = advance_past_escape(s, idx);
            continue;
        }
        let ch = s[idx..].chars().next().expect("idx is a char boundary");
        width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        idx += ch.len_utf8();
    }
    width
}

/// The length in bytes of the terminal control sequence that starts at
/// `bytes[0]`, which must be [`ESC`].
///
/// The parser covers the sequence families a text surface can carry: OSC
/// (`ESC ]`, terminated by BEL or ST), the string sequences (DCS, SOS, PM,
/// APC), CSI (`ESC [`), and the two- and three-byte escapes. An unterminated
/// sequence runs to the end of the input, so a truncated sequence is still
/// treated as one atomic token rather than leaking printable bytes into the
/// width measurement.
fn escape_sequence_len(bytes: &[u8]) -> usize {
    let Some(&second) = bytes.get(1) else {
        return 1;
    };
    match second {
        // OSC: `ESC ] ... (BEL | ST)`.
        b']' => scan_to_terminator(bytes, 2),
        // DCS / SOS / PM / APC: `ESC P|X|^|_ ... ST`.
        b'P' | b'X' | b'^' | b'_' => scan_to_terminator(bytes, 2),
        // CSI: `ESC [ params intermediates final`.
        b'[' => {
            let mut idx = 2;
            while idx < bytes.len() && (0x30..=0x3f).contains(&bytes[idx]) {
                idx += 1;
            }
            while idx < bytes.len() && (0x20..=0x2f).contains(&bytes[idx]) {
                idx += 1;
            }
            if idx < bytes.len() {
                idx += 1;
            }
            idx
        }
        // A two-byte escape with one intermediate: `ESC ( B`.
        0x20..=0x2f => bytes.len().min(3),
        // Any other two-byte escape.
        _ => bytes.len().min(2),
    }
}

/// The end of a string sequence: the first BEL, or an ST (`ESC \`).
fn scan_to_terminator(bytes: &[u8], from: usize) -> usize {
    let mut idx = from;
    while idx < bytes.len() {
        if bytes[idx] == BEL {
            return idx + 1;
        }
        if bytes[idx] == ESC && bytes.get(idx + 1) == Some(&b'\\') {
            return idx + 2;
        }
        idx += 1;
    }
    bytes.len()
}

/// The byte offset just past the escape sequence that starts at `idx`.
///
/// The result is always a char boundary: a sequence that ends mid-character
/// (a malformed one) is extended to the end of that character rather than
/// splitting it.
fn advance_past_escape(s: &str, idx: usize) -> usize {
    let bytes = s.as_bytes();
    let mut end = idx + escape_sequence_len(&bytes[idx..]);
    while end < bytes.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    end
}

/// The source text of a styled wrap, with the span boundaries kept.
struct StyledSource {
    text: String,
    /// `(start, end, style)` per input span, in source order.
    spans: Vec<(usize, usize, Style)>,
}

impl StyledSource {
    fn from_spans(spans: &[Span<'static>]) -> Self {
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(spans.len());
        for span in spans {
            let start = text.len();
            text.push_str(span.content.as_ref());
            ranges.push((start, text.len(), span.style));
        }
        Self {
            text,
            spans: ranges,
        }
    }

    /// The style of the source byte at `offset`.
    fn style_at(&self, offset: usize) -> Style {
        self.spans
            .iter()
            .find(|&&(start, end, _)| start <= offset && offset < end)
            .map(|&(_, _, style)| style)
            .unwrap_or_default()
    }

    /// The whole source as one row, for the zero-width case.
    fn whole_row(&self) -> WrappedRow {
        let slices = self
            .spans
            .iter()
            .map(|&(start, end, style)| StyledSlice {
                text: self.text[start..end].to_string(),
                style,
                source_start: start,
                source_end: end,
            })
            .collect::<Vec<_>>();
        WrappedRow {
            width: visible_width(&self.text),
            source_start: 0,
            source_end: self.text.len(),
            slices,
        }
    }
}

/// What a segment of the source is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentKind {
    /// A terminal control sequence: atomic, and zero display width.
    Escape,
    /// A run of printable, non-whitespace characters.
    Word,
    /// A run of whitespace: the separator between two words.
    Space,
    /// A hard line break.
    Newline,
}

/// A run of the source of one kind.
struct Segment {
    kind: SegmentKind,
    start: usize,
    end: usize,
}

/// Split the source into escape sequences, words, whitespace runs and line
/// breaks.
///
/// A control sequence is one token: it is never split, and it never ends a
/// word — the printable text around it stays a single word, which is what
/// keeps an OSC-8 hyperlink from being cut in half.
fn tokenize(text: &str) -> Vec<Segment> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let ch = text[idx..].chars().next().expect("idx is a char boundary");
        if ch == '\u{1b}' {
            let end = advance_past_escape(text, idx);
            segments.push(Segment {
                kind: SegmentKind::Escape,
                start: idx,
                end,
            });
            idx = end;
            continue;
        }
        if ch == '\n' {
            segments.push(Segment {
                kind: SegmentKind::Newline,
                start: idx,
                end: idx + 1,
            });
            idx += 1;
            continue;
        }
        if ch.is_whitespace() {
            let start = idx;
            while idx < bytes.len() {
                let next = text[idx..].chars().next().expect("idx is a char boundary");
                if next == '\n' || next == '\u{1b}' || !next.is_whitespace() {
                    break;
                }
                idx += next.len_utf8();
            }
            segments.push(Segment {
                kind: SegmentKind::Space,
                start,
                end: idx,
            });
            continue;
        }
        let start = idx;
        while idx < bytes.len() {
            let next = text[idx..].chars().next().expect("idx is a char boundary");
            if next == '\n' || next == '\u{1b}' || next.is_whitespace() {
                break;
            }
            idx += next.len_utf8();
        }
        segments.push(Segment {
            kind: SegmentKind::Word,
            start,
            end: idx,
        });
    }
    segments
}

/// One piece of a word: printable text, or an escape sequence.
#[derive(Clone)]
struct Piece<'a> {
    text: &'a str,
    style: Style,
    start: usize,
    end: usize,
    is_escape: bool,
    width: usize,
}

/// A word: the printable text between two whitespace runs, with any escape
/// sequences that sit inside it.
struct Word<'a> {
    pieces: Vec<Piece<'a>>,
    /// The whitespace run that separated this word from the previous one.
    space: Option<Space>,
}

/// A run of whitespace between two words.
///
/// The run is collapsed to one space, so the run's own text is not carried;
/// the range is what a consumer needs to map a byte in the gap back to the
/// column the space occupies.
struct Space {
    style: Style,
    start: usize,
    end: usize,
}

/// The items the wrap places on rows, in source order.
enum Item<'a> {
    Word(Word<'a>),
    /// A hard line break.
    Break {
        start: usize,
        end: usize,
    },
}

/// Group the source's segments into words.
fn build_items<'a>(source: &'a StyledSource) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = Vec::new();
    let mut pieces: Vec<Piece<'a>> = Vec::new();
    let mut space: Option<Space> = None;

    let flush_word =
        |pieces: &mut Vec<Piece<'a>>, space: &mut Option<Space>, items: &mut Vec<Item<'a>>| {
            if pieces.is_empty() {
                return;
            }
            items.push(Item::Word(Word {
                pieces: std::mem::take(pieces),
                space: space.take(),
            }));
        };

    for segment in tokenize(&source.text) {
        match segment.kind {
            SegmentKind::Newline => {
                flush_word(&mut pieces, &mut space, &mut items);
                space = None;
                items.push(Item::Break {
                    start: segment.start,
                    end: segment.end,
                });
            }
            SegmentKind::Space => {
                flush_word(&mut pieces, &mut space, &mut items);
                space = Some(Space {
                    style: source.style_at(segment.start),
                    start: segment.start,
                    end: segment.end,
                });
            }
            SegmentKind::Escape => {
                pieces.push(Piece {
                    text: &source.text[segment.start..segment.end],
                    style: source.style_at(segment.start),
                    start: segment.start,
                    end: segment.end,
                    is_escape: true,
                    width: 0,
                });
            }
            SegmentKind::Word => {
                for (start, end, style) in split_at_spans(&source.spans, segment.start, segment.end)
                {
                    let text = &source.text[start..end];
                    pieces.push(Piece {
                        text,
                        style,
                        start,
                        end,
                        is_escape: false,
                        width: visible_width(text),
                    });
                }
            }
        }
    }
    flush_word(&mut pieces, &mut space, &mut items);
    items
}

/// Split `start..end` at the source's span boundaries, so that every part
/// carries exactly one style.
fn split_at_spans(
    spans: &[(usize, usize, Style)],
    start: usize,
    end: usize,
) -> Vec<(usize, usize, Style)> {
    let mut parts = Vec::new();
    let mut cursor = start;
    for &(span_start, span_end, style) in spans {
        if span_end <= cursor {
            continue;
        }
        if span_start >= end {
            break;
        }
        let part_start = cursor.max(span_start);
        let part_end = end.min(span_end);
        if part_start < part_end {
            parts.push((part_start, part_end, style));
        }
        cursor = part_end;
        if cursor >= end {
            break;
        }
    }
    if parts.is_empty() && start < end {
        parts.push((start, end, Style::default()));
    }
    parts
}

/// Split a word into the chunks the wrap emits.
///
/// A word that fits the row is one chunk. A word that is wider than the row
/// is hard-broken at character boundaries, exactly as [`wrap_to_strings`]
/// does, so the two engines agree. An escape sequence is never split: it
/// travels whole into the chunk its text belongs to.
fn chunk_word<'a>(pieces: &[Piece<'a>], width: usize) -> Vec<Vec<Piece<'a>>> {
    let mut chunks: Vec<Vec<Piece<'a>>> = Vec::new();
    let mut chunk: Vec<Piece<'a>> = Vec::new();
    let mut chunk_width = 0usize;
    let mut chunk_text = 0usize;

    for piece in pieces {
        if piece.is_escape {
            chunk.push(piece.clone());
            continue;
        }
        let mut seg_start = 0usize;
        for (offset, ch) in piece.text.char_indices() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch_width > 0 && chunk_width + ch_width > width && chunk_text > 0 {
                if seg_start < offset {
                    chunk.push(Piece {
                        text: &piece.text[seg_start..offset],
                        style: piece.style,
                        start: piece.start + seg_start,
                        end: piece.start + offset,
                        is_escape: false,
                        width: visible_width(&piece.text[seg_start..offset]),
                    });
                }
                chunks.push(std::mem::take(&mut chunk));
                chunk_width = 0;
                chunk_text = 0;
                seg_start = offset;
            }
            chunk_width += ch_width;
            chunk_text += 1;
        }
        if seg_start < piece.text.len() {
            let segment_width = visible_width(&piece.text[seg_start..]);
            chunk.push(Piece {
                text: &piece.text[seg_start..],
                style: piece.style,
                start: piece.start + seg_start,
                end: piece.end,
                is_escape: false,
                width: segment_width,
            });
            chunk_width += segment_width;
        }
    }
    if !chunk.is_empty() || chunks.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// Wrap styled spans by word, reporting the byte range of the source behind
/// every row.
///
/// This is the primitive the main buffer and the composer share. Its
/// contract:
///
/// - **Breaks at whitespace.** Words are placed greedily, separated by a
///   single space, and trailing whitespace is dropped. A token that alone is
///   wider than `width` is hard-broken at character boundaries, exactly as
///   [`wrap_to_strings`] does, so the two engines agree.
/// - **Preserves each word's style.** A span is split at word boundaries
///   rather than flattened, so a style that spans a word boundary survives
///   on both sides of it.
/// - **Preserves byte offsets.** Every row reports the byte range of the
///   original source behind it: `source_start` is inclusive, `source_end` is
///   exclusive, and both are offsets into the concatenation of the input
///   spans' contents. Every slice reports the same for its own bytes.
/// - **Excludes escape sequences.** A terminal control sequence is one
///   atomic, zero-width token: it never ends a word, never consumes row
///   width, and is never split across two rows.
///
/// A `width` of zero returns the source as one unwrapped row, and an empty
/// source returns one empty row, so the row count is never zero for a
/// non-empty buffer.
pub fn wrap_styled_spans_by_word_rows(spans: &[Span<'static>], width: u16) -> Vec<WrappedRow> {
    let source = StyledSource::from_spans(spans);
    let width = width as usize;
    if width == 0 {
        return vec![source.whole_row()];
    }

    let mut rows: Vec<WrappedRow> = Vec::new();
    let mut row: Vec<StyledSlice> = Vec::new();
    let mut row_width = 0usize;

    for item in build_items(&source) {
        let word = match item {
            Item::Break { start, end } => {
                // A hard line break ends the row. An empty row still occupies
                // a row: a blank line between two paragraphs is content.
                push_row(&mut rows, &mut row, &mut row_width, Some((start, end)));
                continue;
            }
            Item::Word(word) => word,
        };

        let word_width: usize = word.pieces.iter().map(|piece| piece.width).sum();
        let chunks = chunk_word(&word.pieces, width);
        let is_hard_break = chunks.len() > 1;

        if !row.is_empty() {
            if !is_hard_break && row_width + 1 + word_width <= width {
                // The word shares the row: the whitespace run between them
                // becomes the single space that separates them.
                if let Some(space) = &word.space {
                    row.push(StyledSlice {
                        text: " ".to_string(),
                        style: space.style,
                        source_start: space.start,
                        source_end: space.end,
                    });
                    row_width += 1;
                }
            } else {
                push_row(&mut rows, &mut row, &mut row_width, None);
            }
        }

        let mut chunks = chunks.into_iter();
        if let Some(first) = chunks.next() {
            for piece in first {
                row_width += piece.width;
                row.push(StyledSlice {
                    text: piece.text.to_string(),
                    style: piece.style,
                    source_start: piece.start,
                    source_end: piece.end,
                });
            }
        }
        // Every chunk but the last is a row of its own; the last one stays
        // open so a following word can still share it.
        for chunk in chunks {
            push_row(&mut rows, &mut row, &mut row_width, None);
            for piece in chunk {
                row_width += piece.width;
                row.push(StyledSlice {
                    text: piece.text.to_string(),
                    style: piece.style,
                    source_start: piece.start,
                    source_end: piece.end,
                });
            }
        }
    }

    if !row.is_empty() || rows.is_empty() {
        push_row(&mut rows, &mut row, &mut row_width, None);
    }
    rows
}

/// Wrap styled spans by word and return the rows as `Line`s.
///
/// This is the same wrap as [`wrap_styled_spans_by_word_rows`], reduced to the
/// rendered rows: the byte ranges are dropped and the slices become the
/// line's spans. It exists so a consumer that only draws text does not have
/// to walk the row structure.
pub fn wrap_styled_spans_by_word(spans: &[Span<'static>], width: u16) -> Vec<Line<'static>> {
    wrap_styled_spans_by_word_rows(spans, width)
        .into_iter()
        .map(|row| {
            Line::from(
                row.slices
                    .into_iter()
                    .map(|slice| Span::styled(slice.text, slice.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// Close the row that is being built and start a new one.
fn push_row(
    rows: &mut Vec<WrappedRow>,
    row: &mut Vec<StyledSlice>,
    row_width: &mut usize,
    empty_range: Option<(usize, usize)>,
) {
    let (source_start, source_end) = match row.first() {
        Some(first) => (
            first.source_start,
            row.last()
                .map(|last| last.source_end)
                .unwrap_or(first.source_end),
        ),
        None => empty_range.unwrap_or((0, 0)),
    };
    rows.push(WrappedRow {
        slices: std::mem::take(row),
        source_start,
        source_end,
        width: *row_width,
    });
    *row_width = 0;
}

/// Shorten `s` to at most `width` display columns, appending a horizontal
/// ellipsis when anything was removed.
///
/// Display width (not char count) is what matters in a terminal, so a CJK or
/// emoji-heavy task title is measured by the columns it actually occupies.
/// Returns the input unchanged when it already fits or `width` is 0.
pub fn ellipsize_to_width(s: &str, width: usize) -> String {
    if width == 0 || s.is_empty() {
        return s.to_string();
    }
    let current = unicode_width::UnicodeWidthStr::width(s);
    if current <= width {
        return s.to_string();
    }
    const ELLIPSIS: char = '\u{2026}';
    // The ellipsis is one **column** wide. `…` is three bytes in UTF-8, so
    // reserving its byte length here made every ellipsized string two
    // columns narrower than the budget it was handed — a silent under-fill
    // that shortened every tool header's target.
    let ellipsis_width = unicode_width::UnicodeWidthChar::width(ELLIPSIS).unwrap_or(1);
    let budget = width.saturating_sub(ellipsis_width);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push(ELLIPSIS);
    out
}

/// Collapse embedded newlines into a visible separator so multi-line input (a
/// shell command, a tool argument) renders as one wrappable line without
/// silently losing content.
pub fn normalize_inline(s: &str) -> String {
    s.replace("\r\n", "\n").replace(['\n', '\r'], " ↵ ")
}

/// Truncate to `max_width` display columns and report how many characters were
/// dropped.
///
/// [`ellipsize_to_width`] alone is not enough on surfaces where the string *is*
/// the diagnostic — an approval dialog or an in-flight tool header. A silently
/// shortened command reads as the whole command, so the caller cannot tell what
/// they are authorizing or what is running.
pub fn elide_marked(s: &str, max_width: usize) -> String {
    if max_width == 0 || s.is_empty() {
        return s.to_string();
    }
    let out = ellipsize_to_width(s, max_width);
    if out == s {
        return s.to_string();
    }
    let kept = out.trim_end_matches('\u{2026}').chars().count();
    let dropped = s.chars().count().saturating_sub(kept);
    if dropped == 0 {
        out
    } else {
        format!("{out} (+{dropped} chars)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    /// The OSC-8 hyperlink the transcript emits for an auto-linked URL: the
    /// target, a BEL, the visible text, then the closer.
    fn osc8(url: &str, text: &str) -> String {
        format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07")
    }

    /// The complete escape sequences in `text`, and the printable text
    /// between them.
    fn scan_escapes(text: &str) -> (Vec<String>, String) {
        let mut sequences = Vec::new();
        let mut visible = String::new();
        let bytes = text.as_bytes();
        let mut idx = 0usize;
        while idx < bytes.len() {
            if bytes[idx] == ESC {
                let end = advance_past_escape(text, idx);
                sequences.push(text[idx..end].to_string());
                idx = end;
                continue;
            }
            let ch = text[idx..].chars().next().expect("idx is a char boundary");
            visible.push(ch);
            idx += ch.len_utf8();
        }
        (sequences, visible)
    }

    fn rows_text(rows: &[WrappedRow]) -> Vec<String> {
        rows.iter().map(|row| row.text()).collect()
    }

    #[test]
    fn zero_width_is_passed_through_unwrapped() {
        // Ratatui treats a zero-width area as nothing to draw; returning the
        // input unchanged keeps callers free of special cases.
        let long = "x".repeat(500);
        assert_eq!(wrap_to_strings(&long, 0), vec![long]);
    }

    #[test]
    fn count_matches_wrapping_output() {
        for w in [1u16, 5, 12, 20, 80] {
            let s = "the quick brown fox jumps over the lazy dog 0123456789";
            assert_eq!(wrap_to_strings(s, w).len(), wrap_count(s, w));
        }
    }

    #[test]
    fn no_wrapped_row_exceeds_the_width() {
        let s = "Investigate request-direction stream event capture semantics end to end";
        for w in [8u16, 17, 40] {
            for row in wrap_to_strings(s, w) {
                let width = unicode_width::UnicodeWidthStr::width(row.as_str());
                assert!(
                    width <= w as usize,
                    "row of display width {width} exceeded width {w}: {row:?}"
                );
            }
        }
    }

    #[test]
    fn ellipsize_respects_display_width() {
        assert_eq!(ellipsize_to_width("short", 10), "short");
        // Wide glyphs are measured by columns, so three of them already
        // exceed a four-column budget and get cut, not kept whole.
        for w in [4usize, 6, 10] {
            let out = ellipsize_to_width("\u{4f60}\u{597d}\u{4e16}\u{754c}", w);
            assert!(
                unicode_width::UnicodeWidthStr::width(out.as_str()) <= w,
                "ellipsized output of width {w} overflowed: {out:?}"
            );
        }
        assert!(ellipsize_to_width("abcdefghij", 5).ends_with('\u{2026}'));
    }

    #[test]
    fn ellipsize_fills_its_column_budget() {
        // The ellipsis is one column wide. Reserving its UTF-8 byte length
        // instead left the result two columns short of the budget it was
        // given, so a header's target was shorter than the row allowed.
        for width in 2usize..12 {
            let out = ellipsize_to_width("abcdefghijklmnopqrstuvwxyz", width);
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(out.as_str()),
                width,
                "ellipsized output did not fill a {width}-column budget: {out:?}"
            );
        }
    }

    // -------------------------------------------------------------------
    // The styled, word-aware primitive
    // -------------------------------------------------------------------

    #[test]
    fn wrap_styled_spans_by_word_breaks_at_word_boundaries() {
        let spans = vec![Span::raw("the quick brown fox".to_string())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 10);
        assert_eq!(rows_text(&rows), vec!["the quick", "brown fox"]);
    }

    #[test]
    fn wrap_styled_spans_by_word_hard_breaks_an_over_long_token() {
        // A token wider than the row is broken at character boundaries, the
        // same rule the plain word-aware engine uses.
        let spans = vec![Span::raw("abcdefghij".to_string())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 4);
        assert_eq!(rows_text(&rows), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap_to_strings("abcdefghij", 4), rows_text(&rows));
    }

    #[test]
    fn wrap_styled_spans_by_word_preserves_a_style_across_a_word_boundary() {
        // The styled span covers a space, so the style spans a word boundary.
        // Both words must keep it rather than the span being flattened.
        let style = Style::default().fg(Color::Red);
        let spans = vec![Span::styled("bold text".to_string(), style)];
        let rows = wrap_styled_spans_by_word_rows(&spans, 4);
        assert_eq!(rows_text(&rows), vec!["bold", "text"]);
        assert!(
            rows.iter()
                .flat_map(|row| row.slices.iter())
                .all(|slice| slice.style == style),
            "every slice must keep the style of the span it came from"
        );
    }

    #[test]
    fn wrap_styled_spans_by_word_reports_each_row_source_byte_range() {
        let spans = vec![Span::raw("alpha beta gamma".to_string())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 10);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].source_start, rows[0].source_end), (0, 10));
        assert_eq!((rows[1].source_start, rows[1].source_end), (11, 16));
        // The slices carry their own ranges, so a byte offset maps to a
        // column without re-deriving the row's text.
        let slices: Vec<(usize, usize)> = rows[0]
            .slices
            .iter()
            .map(|slice| (slice.source_start, slice.source_end))
            .collect();
        assert_eq!(slices, vec![(0, 5), (5, 6), (6, 10)]);
        for row in &rows {
            assert!(
                row.source_start < row.source_end,
                "an empty row has no source range to report"
            );
        }
    }

    #[test]
    fn wrap_styled_spans_by_word_on_an_empty_source_returns_one_empty_row() {
        let rows = wrap_styled_spans_by_word_rows(&[], 10);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].slices.is_empty());
        assert_eq!((rows[0].source_start, rows[0].source_end), (0, 0));
    }

    #[test]
    fn wrap_styled_spans_by_word_with_a_zero_width_returns_the_source_unwrapped() {
        // A zero-width row is not drawable; the source is returned as one row
        // so no content is dropped.
        let spans = vec![Span::raw("hello world".to_string())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text(), "hello world");
    }

    #[test]
    fn wrap_styled_spans_by_word_keeps_an_osc8_span_whole() {
        // The hyperlink's target appears twice in the span's content and the
        // blob has no whitespace, so a naive word-wrap treats it as one word
        // and breaks it at an arbitrary character — cutting the escape
        // sequence in half, which is a terminal injection.
        let url = "https://example.invalid/a/long/path";
        let blob = osc8(url, url);
        let spans = vec![Span::raw(blob.clone())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 10);

        let opener = format!("\x1b]8;;{url}\x07");
        let closer = "\x1b]8;;\x07";
        let mut seen: Vec<String> = Vec::new();
        for row in &rows {
            let (sequences, _) = scan_escapes(&row.text());
            for sequence in sequences {
                assert!(
                    sequence == opener || sequence == closer,
                    "a row carries a truncated escape sequence: {sequence:?}"
                );
                seen.push(sequence);
            }
        }
        assert_eq!(seen, vec![opener, closer.to_string()]);
        // The visible text survives the wrap intact.
        let visible: String = rows.iter().map(|row| scan_escapes(&row.text()).1).collect();
        assert_eq!(visible, url);
    }

    #[test]
    fn wrap_styled_spans_by_word_does_not_count_escape_printable_parts_toward_the_width() {
        // The target of the hyperlink is a parameter of the escape sequence,
        // not text: an n-column URL measures n columns, not `2n + 8`.
        let url = "https://example.invalid/a/long/path";
        let blob = osc8(url, url);
        let spans = vec![Span::raw(blob.clone())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 10);

        let measured: usize = rows.iter().map(|row| row.width).sum();
        assert_eq!(measured, visible_width(url));
        assert_ne!(measured, 2 * url.len() + 8);
        for row in &rows {
            assert!(row.width <= 10, "a row exceeded the width: {row:?}");
        }
    }

    #[test]
    fn wrap_styled_spans_by_word_does_not_split_a_span_that_is_only_an_escape_sequence() {
        let only = "\x1b]8;;https://example.invalid\x07\x1b]8;;\x07";
        let spans = vec![Span::raw(only.to_string())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 4);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text(), only);
        assert_eq!(rows[0].width, 0);
    }

    #[test]
    fn wrap_styled_spans_by_word_agrees_with_the_plain_word_aware_engine() {
        // The two engines must break the same way, or the transcript and the
        // surfaces that use the plain engine would lay out differently.
        let text = "Investigate the request-direction stream event capture semantics end to end";
        for width in [8u16, 17, 40] {
            let spans = vec![Span::raw(text.to_string())];
            assert_eq!(
                rows_text(&wrap_styled_spans_by_word_rows(&spans, width)),
                wrap_to_strings(text, width),
                "the two engines disagree at width {width}"
            );
        }
    }

    #[test]
    fn visible_width_ignores_escape_sequences() {
        let url = "https://example.invalid";
        let blob = osc8(url, url);
        assert_eq!(visible_width(&blob), url.len());
        assert_eq!(visible_width("plain text"), 10);
    }

    #[test]
    fn wrap_styled_spans_by_word_emits_word_sized_slices_not_one_per_character() {
        // The character-aware wrapper emitted one span per character, so the
        // per-layout allocation grew with the answer's length. The word-aware
        // wrap emits one slice per word, plus one separator for the words
        // that share a row.
        let text = "Investigate the request-direction stream event capture semantics end to end, then implement the comparative harness across both transports and report which one drops the trailing delta under sustained load, with the evidence attached to the run so a reader can reproduce it without guessing: name the transport, name the delta, and name the load profile that makes the difference visible rather than occasional, and state the threshold that separates the two behaviours instead of the impression of one.";
        let spans = vec![Span::styled(text.to_string(), Style::default())];
        let rows = wrap_styled_spans_by_word_rows(&spans, 60);

        let slices: usize = rows.iter().map(|row| row.slices.len()).sum();
        let words = text.split_whitespace().count();
        assert!(
            slices <= 2 * words,
            "the wrap emitted {slices} slices for {words} words"
        );
        assert!(
            slices < text.chars().count() / 3,
            "the wrap emitted {slices} slices for {} characters",
            text.chars().count()
        );
    }
}
