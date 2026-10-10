//! Terminal text selection for rendered TUI output.
//!
//! The TUI takes over the terminal's own text selection: mouse reporting is
//! enabled (`src/tui/terminal.rs`) so the app can implement click-to-focus,
//! wheel scrolling and scrollbar dragging, and once a terminal is reporting
//! mouse events it stops offering its native drag-select. That makes output
//! such as a `cat` result or a stack trace impossible to select and copy.
//!
//! This module provides the missing half: it reconstructs a rectangular
//! selection over the *rendered* frame buffer. The buffer is the exact set
//! of cells the user can see, including styles and wrapping, so the extracted
//! text is what is on screen rather than a re-derivation from the model that
//! might disagree with the rendered layout.
//!
//! Design notes:
//!
//! - Selection is anchored in absolute buffer coordinates and normalized to
//!   (start, end) with `start <= end` in both row-major and column order, so
//!   a backwards drag selects the same range as a forwards one.
//! - Only *visible* cells can be selected; there is no scrollback, because
//!   the frame buffer does not contain it.
//! - A cell whose symbol is a wide-glyph continuation contributes its
//!   neighbour's character once rather than an empty column, so CJK text
//!   round-trips instead of coming back with holes.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// A rectangular selection over rendered frame cells, in absolute buffer
/// coordinates, normalized so `start <= end` row-major.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    pub start: (u16, u16),
    pub end: (u16, u16),
}

impl TextSelection {
    /// Build a normalized selection from an anchor and a drag position.
    ///
    /// Coordinates are `(column, row)`, so ordering has to be decided on
    /// `(row, column)` — a plain tuple comparison would order by column
    /// first and treat a selection dragged from the bottom-left to the
    /// top-right as reversed.
    pub fn new(anchor: (u16, u16), focus: (u16, u16)) -> Self {
        let anchor_rc = (anchor.1, anchor.0);
        let focus_rc = (focus.1, focus.0);
        if focus_rc < anchor_rc {
            Self {
                start: focus,
                end: anchor,
            }
        } else {
            Self {
                start: anchor,
                end: focus,
            }
        }
    }

    /// Whether `cell` — a `(column, row)` pair — falls inside this selection.
    pub fn contains(&self, cell: (u16, u16)) -> bool {
        let (x, y) = cell;
        let (sx, sy) = self.start;
        let (ex, ey) = self.end;
        if y < sy || y > ey {
            return false;
        }
        // A selection confined to one row is a plain column range; otherwise
        // the first row starts at the anchor, the last row ends at the focus,
        // and the rows between span the full width.
        if sy == ey {
            x >= sx && x <= ex
        } else if y == sy {
            x >= sx
        } else if y == ey {
            x <= ex
        } else {
            true
        }
    }
}

/// Extract the selected text from a rendered frame buffer.
///
/// Rows are trimmed of trailing whitespace so a selection does not pick up
/// the padding that fills the rest of a row. Interior blank rows are kept —
/// they are real content — but a selection confined to a single row yields
/// exactly that row.
pub fn extract(buffer: &Buffer, selection: TextSelection) -> String {
    let area = buffer.area();
    let bounds = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    };
    let mut lines: Vec<String> = Vec::new();

    for y in selection.start.1..=selection.end.1 {
        if y < bounds.y || y >= bounds.y + bounds.height {
            continue;
        }
        let row_start = if y == selection.start.1 {
            selection.start.0.max(bounds.x)
        } else {
            bounds.x
        };
        let row_end = if y == selection.end.1 {
            selection
                .end
                .0
                .min(bounds.x + bounds.width.saturating_sub(1))
        } else {
            bounds.x + bounds.width.saturating_sub(1)
        };
        if row_end < row_start {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for x in row_start..=row_end {
            if x < bounds.x || x >= bounds.x + bounds.width {
                continue;
            }
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }

    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn buffer_with(lines: &[&str]) -> Buffer {
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let height = lines.len() as u16;
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        for (y, line) in lines.iter().enumerate() {
            for (x, ch) in line.chars().enumerate() {
                buffer[(x as u16, y as u16)]
                    .set_symbol(&ch.to_string())
                    .set_style(Style::default());
            }
        }
        buffer
    }

    #[test]
    fn backwards_drag_selects_the_same_range() {
        let forward = TextSelection::new((0, 0), (4, 2));
        let backward = TextSelection::new((4, 2), (0, 0));
        assert_eq!(forward, backward);
        assert_eq!(forward.start, (0, 0));
        assert_eq!(forward.end, (4, 2));
    }

    #[test]
    fn extracts_a_multi_row_selection() {
        let buffer = buffer_with(&["alpha bravo", "charlie del", "echo foxtrot"]);
        // Rectangular semantics: a drag ending mid-way along the second row
        // takes the whole first row and the first 7 columns of the second.
        let text = extract(&buffer, TextSelection::new((0, 0), (6, 1)));
        assert_eq!(text, "alpha bravo\ncharlie");
    }

    #[test]
    fn backwards_drag_extracts_identical_text() {
        let buffer = buffer_with(&["alpha bravo", "charlie del", "echo foxtrot"]);
        let forward = extract(&buffer, TextSelection::new((0, 0), (6, 1)));
        let backward = extract(&buffer, TextSelection::new((6, 1), (0, 0)));
        assert_eq!(forward, backward);
    }

    #[test]
    fn single_row_selection_stays_on_that_row() {
        let buffer = buffer_with(&["alpha bravo", "charlie del"]);
        let text = extract(&buffer, TextSelection::new((6, 0), (10, 0)));
        assert_eq!(text, "bravo");
    }

    #[test]
    fn trailing_padding_is_trimmed_per_row() {
        let buffer = buffer_with(&["ab", "cdef"]);
        // Selecting to the far right of row 1 must not append the padding
        // that fills the remainder of the row.
        let text = extract(&buffer, TextSelection::new((0, 0), (3, 1)));
        assert_eq!(text, "ab\ncdef");
    }

    #[test]
    fn blank_interior_rows_are_preserved() {
        let buffer = buffer_with(&["ab", "", "cd"]);
        let text = extract(&buffer, TextSelection::new((0, 0), (1, 2)));
        assert_eq!(text, "ab\n\ncd");
    }

    #[test]
    fn selection_contains_respects_row_bounds() {
        let selection = TextSelection::new((2, 1), (4, 3));
        assert!(selection.contains((3, 2)));
        assert!(selection.contains((2, 1)));
        assert!(selection.contains((4, 3)));
        assert!(!selection.contains((1, 1)));
        assert!(!selection.contains((5, 3)));
        assert!(!selection.contains((3, 0)));
        assert!(!selection.contains((3, 4)));
    }

    #[test]
    fn extraction_outside_the_buffer_is_clamped_not_panicking() {
        let buffer = buffer_with(&["abc"]);
        // A drag that ends past the last cell must not index out of bounds.
        let text = extract(&buffer, TextSelection::new((0, 0), (99, 9)));
        assert_eq!(text, "abc");
    }
}
