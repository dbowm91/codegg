//! The composer's wrapped layout: a prompt wider than the box, the content
//! past the box's cap, and the caret that stays visible.
//!
//! The composer used to paint one paragraph line per newline-separated
//! segment of the buffer and let ratatui clip it at the right edge, so a
//! long prompt was cut off rather than wrapped. These tests are the
//! rendered evidence that it wraps now, and that the caret follows the
//! bytes it names into the wrapped layout.

use std::sync::Arc;

use codegg::tui::components::prompt::PromptWidget;
use codegg::tui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

/// The composer with `text` in it.
fn composer(text: &str) -> PromptWidget {
    let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
    widget.set_text(text.to_string());
    widget
}

/// The drawn text of one row, without its trailing blanks.
fn row_text(buffer: &Buffer, area: Rect, row: u16) -> String {
    (area.x..area.right())
        .map(|x| buffer[(x, row)].symbol().to_string())
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The drawn text of the box, one string per row.
fn box_rows(buffer: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.bottom())
        .map(|row| row_text(buffer, area, row))
        .collect()
}

/// The characters the box's text rows show, without the whitespace.
///
/// The wrap drops the whitespace at a row break, so the whitespace is not
/// what the rows have in common; the characters are.
fn drawn_characters(rows: &[String]) -> String {
    rows.iter()
        .skip(1)
        .flat_map(|row| row.chars())
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// A prompt wider than the box wraps instead of being clipped, and every
/// character of it is on screen.
#[test]
fn a_prompt_wider_than_the_box_wraps_instead_of_clipping() {
    let text = "the quick brown fox jumps over the lazy dog";
    let widget = composer(text);
    let area = Rect::new(0, 0, 20, 6);
    let mut buffer = Buffer::empty(area);
    widget.render(area, &mut buffer);

    let rows = box_rows(&buffer, area);
    let text_rows = rows.iter().skip(1).filter(|row| !row.is_empty()).count();
    assert!(
        text_rows > 1,
        "the prompt wraps onto more than one row: {rows:?}"
    );
    assert_eq!(
        drawn_characters(&rows),
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>(),
        "every character of the prompt is on screen"
    );
}

/// A prompt taller than the box scrolls: the content past the cap is
/// reachable, and the caret is on screen when it gets there.
#[test]
fn a_prompt_taller_than_the_box_scrolls_to_the_content_past_the_cap() {
    let text = (0..60)
        .map(|index| format!("w{index:02} "))
        .collect::<String>();
    let mut widget = composer(&text);
    let area = Rect::new(0, 0, 20, 10);

    // The caret at the end of a prompt that is taller than the box.
    widget.set_cursor(text.len());
    widget.ensure_cursor_visible_with_width(9, 20);
    assert!(widget.scroll > 0, "the composer scrolled to the caret");

    let mut buffer = Buffer::empty(area);
    widget.render(area, &mut buffer);
    let rows = box_rows(&buffer, area);
    assert!(
        rows.iter().any(|row| row.contains("w59")),
        "the end of the prompt is on screen: {rows:?}"
    );
    assert!(
        widget.caret_cell(area).is_some(),
        "the caret is on screen after the scroll"
    );
}

/// The caret stays visible when the composer is scrolled away from the top
/// of the text.
#[test]
fn the_caret_stays_visible_when_the_composer_is_scrolled() {
    let text = (0..60)
        .map(|index| format!("w{index:02} "))
        .collect::<String>();
    let mut widget = composer(&text);
    let area = Rect::new(0, 0, 20, 10);

    // The user scrolls down and then moves the caret up into the rows above
    // the window: the composer scrolls back to the caret's row.
    widget.scroll_down();
    widget.scroll_down();
    widget.scroll_down();
    let caret = text.len() / 2;
    widget.set_cursor(caret);
    widget.ensure_cursor_visible_with_width(9, 20);

    assert!(widget.scroll > 0, "the composer is scrolled");
    let cell = widget.caret_cell(area).expect("the caret is on screen");
    assert!(
        cell.1 >= area.y + 1 && cell.1 < area.y + 10,
        "the caret's cell is inside the text window: {cell:?}"
    );
}

/// A selection survives the wrap: the selected bytes are painted on the
/// wrapped row they landed on.
#[test]
fn a_selection_is_painted_on_the_wrapped_row_it_landed_on() {
    let mut widget = composer("hello world again");
    widget.set_cursor(9);
    widget.toggle_select_chars();
    for _ in 0..4 {
        widget.cursor_right();
    }

    let area = Rect::new(0, 0, 12, 6);
    let mut buffer = Buffer::empty(area);
    widget.render(area, &mut buffer);

    // "hello world" is the first row and "again" the second, so the
    // selection crosses the wrap and each row paints the part of it that
    // landed on that row.
    assert_eq!(
        buffer[(9, 1)].bg,
        widget.theme.selection,
        "the first row's tail is selected"
    );
    assert_eq!(
        buffer[(0, 2)].bg,
        widget.theme.selection,
        "the second row's head is selected"
    );
    assert_eq!(
        buffer[(2, 2)].bg,
        Color::Reset,
        "the second row's tail is not selected"
    );
}
