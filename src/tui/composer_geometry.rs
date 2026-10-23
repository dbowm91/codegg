//! The visual-row geometry of the composer's wrapped text.
//!
//! The composer used to count **logical** lines — the newline-separated
//! segments of the buffer — everywhere: its height, its scroll, and the cell
//! its caret was drawn on. A logical line is not a row on screen. Once the
//! composer wraps, one logical line is several rows and one row is a fragment
//! of a line, so every "where is the caret" question has to be asked in the
//! **visual** row's terms.
//!
//! This module is the one place that answers them. It takes the wrapped rows
//! the shared primitive produced and gives back:
//!
//! - which row and column a byte is on — [`VisualLayout::cell_of_byte`];
//! - which byte is at a row and column — [`VisualLayout::byte_at`];
//! - where a caret at a byte is drawn — [`VisualLayout::caret_position`];
//! - how many rows the content occupies — [`VisualLayout::row_count`].
//!
//! ## The contract
//!
//! The bytes the wrap deliberately does **not** render — a line break, and
//! the whitespace a row break swallowed — are on no cell at all. The wrap's
//! own contract says as much: the range of a collapsed whitespace run "is what
//! a consumer needs to map a byte in the gap back to the column the space
//! occupies", so the run a row break dropped is attributed to no column while
//! the run that became a space is attributed to that space's column.
//!
//! A caret, though, must always have somewhere to be drawn, so
//! [`VisualLayout::caret_position`] answers for those bytes as well: a caret
//! after a line break sits at the start of the next row, and a caret after
//! the last rendered character sits one column past it.
//!
//! ## The trailing row

use crate::tui::wrap::{visible_width, WrappedRow};
use unicode_width::UnicodeWidthChar;

/// One cell of the wrapped layout: a rendered column and the source bytes
/// drawn in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualCell {
    /// The visual row the cell is on.
    pub row: usize,
    /// The display column of the cell within its row.
    pub column: usize,
    /// The first source byte drawn in the cell.
    pub byte_start: usize,
    /// The source bytes drawn in the cell, exclusive.
    pub byte_end: usize,
    /// The cell's display width in terminal columns.
    pub width: usize,
}

/// A position in the wrapped layout.
///
/// The column may be one past the last cell of the row: that is the
/// insertion point at the end of the row's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualPosition {
    /// The visual row the position is on.
    pub row: usize,
    /// The display column within the row.
    pub column: usize,
}

/// The wrapped layout of a text buffer.
///
/// Built from the source text and the rows the shared wrap primitive
/// produced for it. The two must describe the same text: the rows' byte
/// offsets are offsets into `source`.
#[derive(Debug, Clone, Default)]
pub struct VisualLayout {
    /// The cells of each visual row, in column order.
    cells: Vec<Vec<VisualCell>>,
    /// The source range each row covers, `(start, end)`.
    ranges: Vec<(usize, usize)>,
    /// The byte offsets of the source's line breaks.
    newlines: Vec<usize>,
}

impl VisualLayout {
    /// Build the layout of `source` from the wrapped rows that describe it.
    ///
    /// A buffer that ends with a line break gets one extra row: the empty
    /// line the caret sits on after the break.
    pub fn new(source: &str, rows: &[WrappedRow]) -> Self {
        let mut cells = Vec::with_capacity(rows.len() + 1);
        let mut ranges = Vec::with_capacity(rows.len() + 1);
        for (row_index, row) in rows.iter().enumerate() {
            cells.push(row_cells(row, row_index));
            ranges.push((row.source_start, row.source_end));
        }
        if source.ends_with('\n') {
            cells.push(Vec::new());
            ranges.push((source.len(), source.len()));
        }
        let newlines = source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(offset, _)| offset)
            .collect();
        Self {
            cells,
            ranges,
            newlines,
        }
    }

    /// The number of visual rows the content occupies.
    ///
    /// Never zero: an empty buffer still has the row its caret sits on.
    pub fn row_count(&self) -> usize {
        self.cells.len()
    }

    /// The cells of a row, in column order.
    pub fn row_cells(&self, row: usize) -> &[VisualCell] {
        self.cells.get(row).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The display width of a row, in terminal columns.
    pub fn row_width(&self, row: usize) -> usize {
        self.row_cells(row)
            .last()
            .map(|cell| cell.column + cell.width)
            .unwrap_or(0)
    }

    /// The first source byte of a row.
    pub fn row_source_start(&self, row: usize) -> Option<usize> {
        self.ranges.get(row).map(|(start, _)| *start)
    }

    /// The cell that holds the source byte `byte`.
    ///
    /// A byte inside a multi-byte character is on that character's cell, and
    /// a byte inside a collapsed whitespace run is on that run's cell. A byte
    /// the wrap does not render — a line break, or whitespace dropped at a row
    /// break — is on no cell.
    pub fn cell_of_byte(&self, byte: usize) -> Option<VisualCell> {
        for (row_index, row) in self.cells.iter().enumerate() {
            let (start, end) = self.ranges[row_index];
            if start > byte {
                break;
            }
            if end <= byte {
                continue;
            }
            for cell in row {
                if cell.byte_start <= byte && byte < cell.byte_end {
                    return Some(*cell);
                }
            }
        }
        None
    }

    /// The source byte drawn at a row and a column.
    ///
    /// The byte is the cell's first byte: a multi-byte character's cell
    /// answers with that character's start.
    pub fn byte_at(&self, row: usize, column: usize) -> Option<usize> {
        self.row_cells(row)
            .iter()
            .find(|cell| cell.column == column)
            .map(|cell| cell.byte_start)
    }

    /// Where a caret at `byte` is drawn.
    ///
    /// The caret is *before* the byte it names, so:
    ///
    /// - a byte a rendered character starts on is that character's cell;
    /// - a byte just after a line break is the start of the next row;
    /// - any other byte is one column past the last character before it.
    pub fn caret_position(&self, byte: usize) -> Option<VisualPosition> {
        if let Some(cell) = self.cell_starting_at(byte) {
            return Some(VisualPosition {
                row: cell.row,
                column: cell.column,
            });
        }
        if byte > 0 && self.newlines.binary_search(&(byte - 1)).is_ok() {
            return Some(VisualPosition {
                row: self.row_after(byte - 1),
                column: 0,
            });
        }
        if let Some(cell) = self.last_cell_before(byte) {
            return Some(VisualPosition {
                row: cell.row,
                column: cell.column + cell.width,
            });
        }
        Some(VisualPosition { row: 0, column: 0 })
    }

    /// The cell whose first byte is exactly `byte`.
    fn cell_starting_at(&self, byte: usize) -> Option<VisualCell> {
        for row in &self.cells {
            for cell in row {
                if cell.byte_start == byte {
                    return Some(*cell);
                }
                if cell.byte_start > byte {
                    return None;
                }
            }
        }
        None
    }

    /// The last cell that starts before `byte`.
    fn last_cell_before(&self, byte: usize) -> Option<VisualCell> {
        let mut found = None;
        for row in &self.cells {
            for cell in row {
                if cell.byte_start >= byte {
                    return found;
                }
                found = Some(*cell);
            }
        }
        found
    }

    /// The row that follows the line break at `newline`.
    fn row_after(&self, newline: usize) -> usize {
        self.ranges
            .iter()
            .position(|(start, _)| *start > newline)
            .unwrap_or_else(|| self.ranges.len().saturating_sub(1))
    }
}

/// A slice whose byte length matches its source range is the source text
/// itself, so its characters are the row's cells one for one. A slice whose
/// byte length does not match is a collapsed whitespace run: the wrap
/// replaced the run with one space, and the run's whole byte range belongs to
/// that one column.
fn row_cells(row: &WrappedRow, row_index: usize) -> Vec<VisualCell> {
    let mut cells = Vec::new();
    let mut column = 0usize;
    for slice in &row.slices {
        let verbatim = slice.text.len() == slice.source_end - slice.source_start;
        if !verbatim {
            let width = visible_width(&slice.text).max(1);
            cells.push(VisualCell {
                row: row_index,
                column,
                byte_start: slice.source_start,
                byte_end: slice.source_end,
                width,
            });
            column += width;
            continue;
        }
        let text = slice.text.as_str();
        // A slice that carries a terminal control sequence measures zero
        // columns for the sequence itself, so the widths are taken from the
        // sequence-aware measurement rather than from the characters.
        let measured = text.contains('\u{1b}');
        let mut previous = 0usize;
        for (offset, ch) in text.char_indices() {
            let next = offset + ch.len_utf8();
            let width = if measured {
                visible_width(&text[..next]) - visible_width(&text[..previous])
            } else {
                UnicodeWidthChar::width(ch).unwrap_or(0)
            };
            previous = next;
            if width == 0 {
                continue;
            }
            cells.push(VisualCell {
                row: row_index,
                column,
                byte_start: slice.source_start + offset,
                byte_end: slice.source_start + next,
                width,
            });
            column += width;
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::wrap::wrap_styled_spans_by_word_rows;
    use ratatui::text::Span;

    /// The layout of `text` wrapped at `width`.
    fn layout(text: &str, width: u16) -> VisualLayout {
        let rows = wrap_styled_spans_by_word_rows(&[Span::raw(text.to_string())], width);
        VisualLayout::new(text, &rows)
    }

    /// The texts the mapping is proved over.
    const TEXTS: &[&str] = &[
        "",
        "hello",
        "hello world",
        "hello\nworld",
        "hello\n",
        "hello\n\n",
        "\n",
        "a long line of prompt text that is wider than the row",
        "supercalifragilisticexpialidocious",
        "héllo wörld",
        "日本語のテキスト",
        "mixed  spaces   here",
        "trailing spaces   ",
    ];

    /// The widths the mapping is proved over.
    const WIDTHS: &[u16] = &[1, 2, 3, 5, 8, 40];

    /// Every cell holds exactly one byte, and every rendered byte is on
    /// exactly one cell.
    ///
    /// This is the bijection the invariant rests on, and it is a property
    /// over the texts and the widths rather than a set of examples: the
    /// cells and the bytes they name are the same set, seen from two sides.
    #[test]
    fn the_mapping_is_a_bijection_over_widths_and_texts() {
        for text in TEXTS {
            for width in WIDTHS {
                let layout = layout(text, *width);
                let mut seen_bytes: Vec<usize> = Vec::new();
                for row in 0..layout.row_count() {
                    for cell in layout.row_cells(row) {
                        assert_eq!(
                            layout.byte_at(cell.row, cell.column),
                            Some(cell.byte_start),
                            "text {text:?} width {width}: the cell's byte is the byte at it"
                        );
                        assert_eq!(
                            layout.cell_of_byte(cell.byte_start),
                            Some(*cell),
                            "text {text:?} width {width}: the byte is on its own cell"
                        );
                        for byte in cell.byte_start..cell.byte_end {
                            assert_eq!(
                                layout.cell_of_byte(byte),
                                Some(*cell),
                                "text {text:?} width {width}: byte {byte} is on the cell"
                            );
                        }
                        assert!(
                            !seen_bytes.contains(&cell.byte_start),
                            "text {text:?} width {width}: two cells claim byte {}",
                            cell.byte_start
                        );
                        seen_bytes.push(cell.byte_start);
                    }
                }
            }
        }
    }

    /// The inverse direction holds too: the byte at a cell is the byte the
    /// cell names, so the two lookups cannot disagree.
    #[test]
    fn the_byte_and_the_cell_are_inverses() {
        for text in TEXTS {
            for width in WIDTHS {
                let layout = layout(text, *width);
                for row in 0..layout.row_count() {
                    for cell in layout.row_cells(row) {
                        let byte = layout
                            .byte_at(cell.row, cell.column)
                            .expect("the cell's byte");
                        assert_eq!(
                            layout.cell_of_byte(byte),
                            Some(*cell),
                            "text {text:?} width {width}: the round trip holds"
                        );
                    }
                }
            }
        }
    }

    /// An empty buffer still has a row: the row its caret sits on.
    #[test]
    fn an_empty_buffer_has_one_row_and_one_caret_position() {
        let layout = layout("", 10);
        assert_eq!(layout.row_count(), 1);
        assert!(layout.row_cells(0).is_empty());
        assert_eq!(
            layout.caret_position(0),
            Some(VisualPosition { row: 0, column: 0 })
        );
    }

    /// A buffer that ends with a line break has one more row than the wrap
    /// produced: the empty line the caret sits on after the break.
    #[test]
    fn a_buffer_ending_in_a_newline_has_a_trailing_row() {
        let layout = layout("hello\n", 10);
        assert_eq!(layout.row_count(), 2);
        assert_eq!(
            layout.caret_position(6),
            Some(VisualPosition { row: 1, column: 0 })
        );
        assert_eq!(
            layout.caret_position(5),
            Some(VisualPosition { row: 0, column: 5 })
        );
    }

    /// Content wider than the row is wrapped, and every byte of it is on a
    /// cell of the layout.
    #[test]
    fn content_wider_than_the_row_wraps_onto_several_rows() {
        let layout = layout("hello world", 5);
        assert_eq!(layout.row_count(), 2);
        assert_eq!(layout.byte_at(0, 0), Some(0));
        assert_eq!(layout.byte_at(1, 0), Some(6));
        assert_eq!(layout.cell_of_byte(4), Some(layout.row_cells(0)[4]));
    }

    /// A token wider than the row is hard-broken, so it lands on several rows
    /// and no byte of it is lost.
    #[test]
    fn a_token_wider_than_the_row_is_hard_broken() {
        let layout = layout("abcdefgh", 3);
        assert_eq!(layout.row_count(), 3);
        for byte in 0..8 {
            assert!(
                layout.cell_of_byte(byte).is_some(),
                "byte {byte} is on a cell"
            );
        }
    }

    #[test]
    fn a_multi_byte_character_is_one_cell() {
        let layout = layout("é", 10);
        assert_eq!(layout.row_count(), 1);
        assert_eq!(layout.row_cells(0).len(), 1);
        let cell = layout.row_cells(0)[0];
        assert_eq!(cell.byte_start, 0);
        assert_eq!(cell.byte_end, 2);
        assert_eq!(cell.width, 1);
        assert_eq!(layout.cell_of_byte(1), Some(cell));
    }

    /// replaced it, which is what the wrap's contract asks a consumer for.
    #[test]
    fn a_collapsed_whitespace_run_is_on_the_space_s_column() {
        let layout = layout("a  b", 10);
        let space = layout
            .row_cells(0)
            .iter()
            .find(|cell| cell.byte_start == 1)
            .expect("the collapsed run is one cell");
        assert_eq!(space.byte_end, 3);
        assert_eq!(space.width, 1);
        assert_eq!(layout.cell_of_byte(2), Some(*space));
    }

    /// A caret after a line break is at the start of the next row.
    #[test]
    fn a_caret_after_a_line_break_is_at_the_start_of_the_next_row() {
        let layout = layout("one\ntwo", 10);
        assert_eq!(
            layout.caret_position(4),
            Some(VisualPosition { row: 1, column: 0 })
        );
        assert_eq!(
            layout.caret_position(3),
            Some(VisualPosition { row: 0, column: 3 })
        );
    }
}
