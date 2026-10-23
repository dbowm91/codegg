use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::cell::{Ref, RefCell};
use std::sync::Arc;

use super::super::composer_claims::ComposerFit;
use super::super::composer_geometry::VisualLayout;
use super::super::render_cost;
use super::super::theme::Theme;
use super::super::wrap::{wrap_styled_spans_by_word_rows, WrappedRow};

/// How the composer draws its caret.
///
/// `Block` is the solid block painted at the caret in Normal mode. `None` means
/// the widget paints no caret at all — either because the app positions the
/// terminal's own cursor at the caret (Insert mode, where the terminal blinks
/// it), or because the composer does not hold the keyboard and must show
/// nothing.
///
/// The two are not the same thing, but they look the same to the widget, and
/// the widget does not need to tell them apart: the app decides which one is
/// true, and it decides it from the same derived ownership fact that the
/// `[INS]`/`[NOR]` badge reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComposerCursor {
    /// The widget paints no caret.
    #[default]
    None,
    /// The widget paints a solid block at the caret.
    Block,
}

pub struct PromptWidget {
    /// The composer's text model: the buffer, the caret, the selection, and
    /// the bounded undo history.
    ///
    /// The model owns the buffer so there is exactly one answer to "what is
    /// in the composer" and "where is the caret". The widget owns the
    /// *presentation* around it.
    buffer: super::super::composer_text::ComposerText,
    /// How the composer draws its caret. Set by the app's render layer from
    /// the derived keyboard-ownership fact, never stored as an independent
    /// "is focused" flag — a stored copy can disagree with the badge, which
    /// is the defect class the mode resolution was written to kill.
    pub cursor_style: ComposerCursor,
    pub theme: Arc<Theme>,
    pub prefix: Span<'static>,
    pub mode_indicator: Line<'static>,
    pub placeholder: String,
    /// The composer's vertical scroll, in **visual rows**.
    ///
    /// A visual row is one row of the wrapped text, not one newline-separated
    /// segment of the buffer: a long line is several rows, and the scroll is
    /// what reaches the rows past the box's ten-row cap.
    pub scroll: usize,
    /// Horizontal offset used to keep the cursor visible on a row wider than
    /// the box.
    ///
    /// The wrap hard-breaks a token wider than the row, so every row fits the
    /// box and this is zero. It stays because the one case the wrap cannot
    /// cover is a single *character* wider than the whole row, and because the
    /// app's mouse placement adds it to the clicked column.
    pub horizontal_scroll: usize,
    /// The width the composer wraps at.
    ///
    /// Set by the app's render layer from the pane's width, which is the
    /// width the height was computed against. Zero means "no wrap": the
    /// mouse placement falls back to the logical line model.
    pub wrap_width: u16,
    /// The composer's fit, as the last layout pass measured it.
    ///
    /// The **cached** answer to "does the content fit its box", and the only
    /// answer the claim gate is allowed to read. The gate runs at key
    /// dispatch, before the render pass, so a measurement taken there would be
    /// a second computation of a fact the layout pass already owns — and two
    /// computations of one fact are two answers to it.
    ///
    /// It is [`ComposerFit::Unknown`] until the first layout pass measures
    /// it, and an unknown fit holds no conditional claim.
    fit: ComposerFit,
    pub command_mode: bool,
    pub waiting: bool,
    pub char_count: Option<usize>,
    /// The composer's wrapped layout, cached against the buffer's revision
    /// and the width it was wrapped at.
    ///
    /// The layout is the **one** answer to "what rows does this text make",
    /// and it is expensive: the wrap measures every character of the buffer.
    /// Four consumers want it in a frame — the height budget, the caret's
    /// visibility, the draw, and the caret's cell — and they used to each
    /// rebuild it, so a frame paid for the wrap four times over.
    ///
    /// The cache is keyed on the buffer's **revision** and the **width**,
    /// which are the two things the layout depends on and the two things that
    /// change it: a caret that moves does not move the revision, so a frame
    /// that only moves the caret reuses the layout, and a frame that changed
    /// the text rebuilds it exactly once.
    layout_cache: RefCell<Option<ComposerLayoutCache>>,
}

/// The composer's wrapped layout, and the key it was built for.
struct ComposerLayoutCache {
    /// The buffer revision the rows were wrapped from.
    revision: u64,
    /// The width the rows were wrapped at.
    width: u16,
    /// The wrapped rows.
    rows: Vec<WrappedRow>,
    /// The visual layout the rows describe.
    layout: VisualLayout,
}

impl PromptWidget {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            buffer: super::super::composer_text::ComposerText::new(),
            cursor_style: ComposerCursor::None,
            theme,
            prefix: Span::raw("❯ "),
            mode_indicator: Line::default(),
            placeholder: "Ask anything…".to_string(),
            scroll: 0,
            horizontal_scroll: 0,
            wrap_width: 0,
            fit: ComposerFit::Unknown,
            command_mode: false,
            waiting: false,
            char_count: None,
            layout_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_prefix(&mut self, prefix: Span<'static>) {
        self.prefix = prefix;
    }

    /// Takes a whole `Line` rather than a `Span` because the mode row
    /// carries two send targets (`[BUILD]`/`[CHAT]`) that must be styled
    /// differently from each other to show which one is armed.
    pub fn set_mode_indicator(&mut self, indicator: Line<'static>) {
        self.mode_indicator = indicator;
    }

    pub fn set_placeholder(&mut self, placeholder: String) {
        self.placeholder = placeholder;
    }

    /// Set the width the composer wraps at.
    ///
    /// The app's render layer sets it from the pane's width, which is the
    /// width the box's height was computed against. Zero means "no wrap",
    /// which keeps the mouse placement on the logical line model.
    ///
    /// The layout pass records the same width when it measures the fit, so
    /// the two never disagree about the width the composer is laid out at.
    pub fn set_wrap_width(&mut self, width: u16) {
        self.wrap_width = width;
    }

    pub fn set_command_mode(&mut self, command_mode: bool) {
        self.command_mode = command_mode;
    }

    pub fn set_waiting(&mut self, waiting: bool) {
        self.waiting = waiting;
    }

    pub fn set_char_count(&mut self, count: Option<usize>) {
        self.char_count = count;
    }

    /// Set how the composer draws its caret.
    pub fn set_cursor_style(&mut self, style: ComposerCursor) {
        self.cursor_style = style;
    }

    pub fn get_text(&self) -> String {
        self.buffer.text().to_string()
    }

    /// The composer's text, borrowed.
    ///
    /// The borrowed answer to "what is in the composer".
    /// [`Self::get_text`] clones it, which is right for a caller that has to
    /// keep the text and wrong for a caller that only reads it — a caller on
    /// the render path clones the whole buffer once per frame for a question
    /// that a prefix test answers without one.
    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: String) {
        self.buffer.set_text(text);
        self.horizontal_scroll = 0;
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.buffer.set_caret(pos);
    }

    /// Place the cursor at a display column on a **visual row** of the
    /// composer.
    ///
    /// Mouse coordinates are cell-based while the caret is a UTF-8 byte
    /// offset, and a click names a row of the *drawn* text — which, once the
    /// composer wraps, is a visual row and not a line of the buffer.
    /// Converting here keeps wide and multibyte characters from leaving the
    /// cursor inside a code point, and keeps a click on a wrapped row from
    /// landing on a line of the buffer that was never drawn.
    ///
    /// A composer that does not wrap yet — `wrap_width` is zero — keeps the
    /// logical line model, because then a row *is* a line.
    pub fn set_cursor_at_column(&mut self, row: usize, column: usize) {
        if self.wrap_width == 0 {
            self.set_cursor_at_logical_column(row, column);
            return;
        }
        let byte = {
            let layout = self.layout_at(self.wrap_width);
            let cells = layout.row_cells(row);
            match cells.iter().find(|cell| cell.column == column) {
                Some(cell) => cell.byte_start,
                // Past the row's last cell: the insertion point at the row's end.
                None => match cells.last() {
                    Some(cell) => cell.byte_end,
                    // An empty row — a blank line — starts at its own offset.
                    None => layout
                        .row_source_start(row)
                        .unwrap_or_else(|| self.buffer.text().len()),
                },
            }
        };
        self.buffer.set_caret(byte);
    }

    /// Place the cursor at a display column on a logical line of the buffer.
    fn set_cursor_at_logical_column(&mut self, line_index: usize, column: usize) {
        let text = self.buffer.text();
        let mut line_start = 0;
        for (current_line, line) in text.split('\n').enumerate() {
            if current_line == line_index {
                let mut display_column: usize = 0;
                for (byte_offset, ch) in line.char_indices() {
                    let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if column < display_column.saturating_add(width) {
                        self.buffer.set_caret(line_start + byte_offset);
                        return;
                    }
                    display_column = display_column.saturating_add(width);
                }
                self.buffer.set_caret(line_start + line.len());
                return;
            }
            line_start += line.len() + 1;
        }
        self.buffer.set_caret(text.len());
    }

    pub fn cursor_pos(&self) -> usize {
        self.buffer.caret()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.scroll = 0;
        self.horizontal_scroll = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        let mut encoded = [0u8; 4];
        let text = c.encode_utf8(&mut encoded);
        self.buffer.insert(text);
    }

    pub fn insert_newline(&mut self) {
        self.buffer.insert_newline();
    }

    /// Move the caret after the character it sits on, for insert-after.
    pub fn insert_after(&mut self) {
        self.buffer.insert_after();
    }

    /// Open an empty line below the caret's line.
    pub fn open_line_below(&mut self) {
        self.buffer.open_line_below();
    }

    /// The composer's line count — an empty buffer is one line.
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Undo the newest edit. Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        self.buffer.undo()
    }

    /// Redo the newest undone edit. Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        self.buffer.redo()
    }

    pub fn backspace(&mut self) {
        self.buffer.backspace();
    }

    pub fn delete(&mut self) {
        self.buffer.delete_forward();
    }

    pub fn cursor_left(&mut self) {
        self.buffer.move_left();
    }

    pub fn cursor_right(&mut self) {
        self.buffer.move_right();
    }

    pub fn cursor_home(&mut self) {
        self.buffer.move_home();
    }

    pub fn cursor_end(&mut self) {
        self.buffer.move_end();
    }

    /// Move the caret to the start of the next word.
    pub fn cursor_word_forward(&mut self) {
        self.buffer.move_word_forward();
    }

    /// Move the caret to the start of the previous word.
    pub fn cursor_word_backward(&mut self) {
        self.buffer.move_word_backward();
    }

    /// Move the caret to the last character of the word at the caret.
    pub fn cursor_word_end(&mut self) {
        self.buffer.move_word_end();
    }

    /// Move the caret one line down.
    pub fn cursor_line_down(&mut self) {
        self.buffer.move_line_down();
    }

    /// Move the caret one line up.
    pub fn cursor_line_up(&mut self) {
        self.buffer.move_line_up();
    }

    /// Move the caret one **visual row** down.
    ///
    /// A visual row is one row of the wrapped text, not one newline-separated
    /// line of the buffer: after the wrap a long line is several rows, and a
    /// row is what the user sees.
    pub fn cursor_visual_row_down(&mut self) {
        self.move_visual_row(1);
    }

    /// Move the caret one **visual row** up.
    pub fn cursor_visual_row_up(&mut self) {
        self.move_visual_row(-1);
    }

    /// Page the composer down by one page of visual rows.
    ///
    /// The window and the caret move together: the caret is always visible, so
    /// a window that moved without the caret would be undone by the next
    /// layout pass.
    pub fn page_down(&mut self) {
        self.page(1);
    }

    /// Page the composer up by one page of visual rows.
    pub fn page_up(&mut self) {
        self.page(-1);
    }

    /// Arm a character selection at the caret, or discard the held one.
    pub fn toggle_select_chars(&mut self) {
        self.buffer.toggle_select_chars();
    }

    /// Select the caret's line, or extend the held line selection by one line.
    pub fn select_line(&mut self) {
        self.buffer.select_line();
    }

    /// Delete the held selection, if there is one.
    ///
    /// Returns whether anything was deleted.
    pub fn delete_selection(&mut self) -> bool {
        self.buffer.delete_selection()
    }

    /// The byte range the held selection covers, if there is one.
    ///
    /// The same range the delete verb removes and the renderer paints, so
    /// "the selection" is one thing rather than three.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.buffer.selection_range()
    }

    /// The text the held selection covers, if there is one.
    pub fn selected_text(&self) -> Option<String> {
        self.buffer.selected_text()
    }

    pub fn paste(&mut self, text: String) {
        self.buffer.insert(&text);
    }

    /// The number of **logical** lines in the buffer — an empty buffer is one.
    ///
    /// Unchanged by the wrap: the claim condition and the vertical motions
    /// are written against the logical model, and this milestone leaves them
    /// there.
    pub fn num_lines(&self) -> usize {
        self.buffer.text().split('\n').count()
    }

    /// The caret's **logical** line.
    pub fn cursor_line(&self) -> usize {
        self.buffer.caret_line()
    }

    /// The number of marker rows the composer draws below its text.
    fn marker_rows(&self) -> usize {
        let mut rows = 0;
        if self.char_count.unwrap_or(self.buffer.text().len()) > 500 {
            rows += 1;
        }
        if self.buffer.text().contains('\n') {
            rows += 1;
        }
        rows
    }

    /// The box's internal split: the text window's height and the marker
    /// rows.
    ///
    /// The mode row is always the box's first row, so the text window is what
    /// is left under it. A box too short for the markers keeps the text and
    /// drops the markers: a composer that shows no text is worse than one
    /// that shows no character count.
    fn box_split(&self, height: u16) -> (u16, u16) {
        let markers = self.marker_rows() as u16;
        let text = height.saturating_sub(1 + markers);
        if text == 0 && height > 1 {
            return (height - 1, 0);
        }
        (text, markers)
    }

    /// The number of text rows visible in a box `height` rows tall.
    pub fn visible_text_rows(&self, height: u16) -> usize {
        self.box_split(height).0 as usize
    }

    /// The composer's fit, as the last layout pass measured it.
    ///
    /// The **cached** measurement, not a fresh one: the claim gate reads this
    /// so that the gate and the box cannot disagree about whether the content
    /// fits.
    pub fn content_fit(&self) -> ComposerFit {
        self.fit
    }

    /// Whether the composer's content does not fit its box.
    ///
    /// The wrapped row count against the visible row count, as the last layout
    /// pass measured them. A composer that has never been laid out has no
    /// measurement, and an unmeasured composer does not overflow: the claim
    /// that depends on this must not fire in a state the user cannot see.
    pub fn content_overflows(&self) -> bool {
        self.fit.overflows()
    }

    /// The height the composer needs for its text at `width`.
    ///
    /// The height is the mode row, the wrapped text rows and the marker rows,
    /// capped at **ten rows in total** and then bounded by the pane's
    /// budget. The budget wins: on a pane whose budget is below the cap, the
    /// budget is the height.
    ///
    /// The cap is on the box's total, not on the text: text past the cap
    /// stays in the buffer and is reachable by scrolling.
    pub fn needed_height(&self, max_height: u16, width: u16) -> u16 {
        const MIN_HEIGHT: u16 = 3;
        const MAX_HEIGHT: u16 = 10;
        let marker_rows = self.marker_rows() as u16;
        let text_rows = self.layout_at(width).row_count() as u16;
        let text_budget = MAX_HEIGHT.saturating_sub(1 + marker_rows);
        let total = 1 + text_rows.min(text_budget) + marker_rows;
        total.clamp(MIN_HEIGHT, max_height.max(MIN_HEIGHT))
    }

    /// Clamp the scroll to the content the window can show.
    pub fn clamp_scroll(&mut self, visible_rows: usize, width: u16) {
        let total = self.layout_at(width).row_count();
        if total <= visible_rows {
            self.scroll = 0;
        } else {
            self.scroll = self.scroll.min(total - visible_rows);
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll += 1;
    }

    /// Keep the caret visible in the wrapped layout.
    ///
    /// The scroll is a **visual row** offset: the caret's row is the row its
    /// byte is drawn on, and the composer scrolls so that row is inside the
    /// window. A caret past the ten-row cap therefore scrolls the composer
    /// down to itself, which is how the content past the cap is reached.
    ///
    /// This is also where the composer **measures its fit**: the layout pass
    /// is the only place that knows both the wrapped row count and the box's
    /// visible row count, so the measurement is taken here and cached. The
    /// claim gate reads the cached measurement instead of taking a second one.
    ///
    /// The width it is handed is the width the composer is laid out at, so
    /// the pass records it as the wrap width too: one width, the one the
    /// layout pass measured, so the motions cannot act on a different layout
    /// than the one the fit was measured against.
    pub fn ensure_cursor_visible_with_width(&mut self, visible_rows: usize, width: u16) {
        // The layout is read in one scope and the composer is adjusted in
        // another. The layout is borrowed from the widget, and a widget that
        // is being adjusted cannot also be borrowed.
        let (total, caret, caret_row, caret_row_width) = {
            let layout = self.layout_at(width);
            let total = layout.row_count();
            let caret = layout.caret_position(self.buffer.caret());
            let caret_row = caret.map(|position| position.row).unwrap_or(0);
            let caret_row_width = layout.row_width(caret_row);
            (total, caret, caret_row, caret_row_width)
        };
        self.wrap_width = width;
        // The fit, measured once: the wrapped row count against the number of
        // text rows the box shows. Three rows in a three-row box fits; four
        // rows in a three-row box does not.
        self.fit = ComposerFit::Measured {
            row_count: total,
            visible_rows,
        };
        if total <= visible_rows {
            self.scroll = 0;
        } else {
            let max_scroll = total - visible_rows;
            if caret_row < self.scroll {
                self.scroll = caret_row;
            } else if caret_row >= self.scroll + visible_rows {
                self.scroll = caret_row.saturating_sub(visible_rows.saturating_sub(1));
            }
            self.scroll = self.scroll.min(max_scroll);
        }

        // The horizontal scroll's one remaining case: a row wider than the
        // box. The wrap hard-breaks a token wider than the row, so this is
        // the case of a single character wider than the whole row.
        let visible_width = width as usize;
        let column = caret.map(|position| position.column).unwrap_or(0);
        if visible_width > 0 && caret_row_width > visible_width {
            if column < self.horizontal_scroll {
                self.horizontal_scroll = column;
            } else if column >= self.horizontal_scroll.saturating_add(visible_width) {
                self.horizontal_scroll = column.saturating_sub(visible_width.saturating_sub(1));
            }
        } else {
            self.horizontal_scroll = 0;
        }
    }

    /// The caret's cell inside `area`, or `None` when it is not on screen.
    ///
    /// One computation for two consumers: the painted block and the terminal
    /// cursor the app positions. They cannot disagree about where the caret is
    /// if they read the same answer, and the block used to compute its own
    /// position here.
    ///
    /// The composer's first row is the mode/prefix row, so the text starts one
    /// row below it, and the text window scrolls by **visual rows**. The
    /// caret's cell is the cell of the caret's *byte* in the wrapped layout:
    /// its column is the offset within the wrapped row, not within the
    /// logical line. A caret whose row has scrolled off the top has no cell at
    /// all.
    pub fn caret_cell(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let layout = self.layout_at(area.width);
        self.caret_cell_in(area, &layout)
    }

    /// The caret's cell, from a layout the caller already has.
    ///
    /// The render path has the layout in hand — it drew the rows from it —
    /// so it asks here rather than wrapping the text a second time.
    fn caret_cell_in(&self, area: Rect, layout: &VisualLayout) -> Option<(u16, u16)> {
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let (text_height, _) = self.box_split(area.height);
        let scroll = self.effective_scroll(layout, text_height as usize);
        let position = layout.caret_position(self.buffer.caret())?;
        if position.row < scroll {
            // The caret's row is above the visible window.
            return None;
        }
        let visible_row = position.row - scroll;
        if visible_row >= text_height as usize {
            return None;
        }
        let cursor_y = area
            .y
            .checked_add(1)?
            .checked_add(u16::try_from(visible_row).ok()?)?;
        if cursor_y >= area.bottom() {
            return None;
        }
        let col = position.column.saturating_sub(self.horizontal_scroll);
        let col = col.min(area.width.saturating_sub(1) as usize);
        let cursor_x = area.x.checked_add(u16::try_from(col).ok()?)?;
        if cursor_x >= area.right() {
            return None;
        }
        Some((cursor_x, cursor_y))
    }

    /// Whether the composer paints a solid block at the caret.
    pub fn paints_block_cursor(&self) -> bool {
        self.cursor_style == ComposerCursor::Block
    }

    /// The wrapped rows of the composer's text at `width`.
    ///
    /// The **one** producer of the composer's rows. Every consumer reads the
    /// cached layout instead of calling this, so a frame wraps the buffer
    /// once rather than once per consumer.
    fn wrapped_rows(&self, width: u16) -> Vec<WrappedRow> {
        render_cost::count_composer_wrap();
        wrap_styled_spans_by_word_rows(&[Span::raw(self.buffer.text().to_string())], width)
    }

    /// The visual layout of the composer's text at `width`.
    ///
    /// Cached against the buffer's revision and the width, so a frame that
    /// changed neither reuses the layout and a frame that changed one
    /// rebuilds it once.
    fn layout_at(&self, width: u16) -> Ref<'_, VisualLayout> {
        let revision = self.buffer.revision();
        let stale = match self.layout_cache.borrow().as_ref() {
            Some(cache) => cache.revision != revision || cache.width != width,
            None => true,
        };
        if stale {
            let text = self.buffer.text().to_string();
            let rows = self.wrapped_rows(width);
            let layout = VisualLayout::new(&text, &rows);
            *self.layout_cache.borrow_mut() = Some(ComposerLayoutCache {
                revision,
                width,
                rows,
                layout,
            });
        }
        Ref::map(self.layout_cache.borrow(), |cache| {
            &cache
                .as_ref()
                .expect("the layout cache is filled before it is read")
                .layout
        })
    }

    /// The wrapped rows of the composer's text at `width`.
    ///
    /// The rows and the layout are one cache entry, so a frame that draws
    /// the rows and asks for the caret's cell reads the same wrap.
    fn rows_at(&self, width: u16) -> Ref<'_, Vec<WrappedRow>> {
        // The layout is filled first, so the rows it names are the rows in
        // the cache entry this borrows.
        let _ = self.layout_at(width);
        Ref::map(self.layout_cache.borrow(), |cache| {
            &cache
                .as_ref()
                .expect("the layout cache is filled before it is read")
                .rows
        })
    }

    /// Move the caret by `delta` visual rows.
    ///
    /// The **desired column** is carried across the move where the target row
    /// allows: the caret keeps its column, and a target row shorter than that
    /// column takes the caret to the row's end. This is the rule the shared
    /// geometry's `move_vertical` implements for logical lines, in the
    /// visual-row model.
    ///
    /// The caret lands on a **byte** — the byte of the cell at the target
    /// row's column — so the caret's drawn cell is that byte's cell.
    ///
    /// A composer that does not wrap yet — `wrap_width` is zero — keeps the
    /// logical line model, because then a row *is* a line.
    fn move_visual_row(&mut self, delta: isize) {
        if self.wrap_width == 0 {
            if delta.is_negative() {
                self.buffer.move_line_up();
            } else {
                self.buffer.move_line_down();
            }
            return;
        }
        let byte = {
            let layout = self.layout_at(self.wrap_width);
            let Some(position) = layout.caret_position(self.buffer.caret()) else {
                return;
            };
            let last_row = layout.row_count().saturating_sub(1);
            let target = if delta.is_negative() {
                position.row.saturating_sub(delta.unsigned_abs())
            } else {
                position.row.saturating_add(delta as usize).min(last_row)
            };
            if target == position.row {
                // Clamped at the first or the last visual row: there is nowhere
                // to go, and the caret stays on the byte it names.
                return;
            }
            byte_at_visual(&layout, target, position.column)
        };
        self.buffer.set_caret(byte);
    }

    /// Page the composer by one page of visual rows.
    ///
    /// The window and the caret move together, and they move by the same
    /// amount: the caret is always visible, so a window that moved without
    /// the caret would be undone by the next layout pass, and a caret that
    /// moved without the window would leave the box showing something else.
    ///
    /// The page size is the transcript's own rule, so the two surfaces page
    /// comparably.
    fn page(&mut self, direction: isize) {
        let Some(page) = self.page_size() else {
            // No measurement, no page: the page size is the visible height,
            // and an unlaid-out composer has no visible height.
            return;
        };
        if direction.is_negative() {
            self.scroll = self.scroll.saturating_sub(page);
            self.move_visual_row(-(page as isize));
        } else {
            self.scroll = self.scroll.saturating_add(page);
            self.move_visual_row(page as isize);
        }
        if let ComposerFit::Measured { visible_rows, .. } = self.fit {
            self.clamp_scroll(visible_rows, self.wrap_width);
        }
    }

    /// The page size the composer's page motions use.
    ///
    /// The transcript's rule, read from the transcript's own page motion: the
    /// visible height minus two, with a floor of one.
    fn page_size(&self) -> Option<usize> {
        match self.fit {
            ComposerFit::Measured { visible_rows, .. } => {
                Some(visible_rows.saturating_sub(2).max(1))
            }
            ComposerFit::Unknown => None,
        }
    }

    /// The scroll offset the composer draws with.
    ///
    /// The requested offset, clamped so the window shows content: a scroll
    /// past the end would leave the box empty, which is the one outcome the
    /// composer must never produce.
    fn effective_scroll(&self, layout: &VisualLayout, text_height: usize) -> usize {
        let max_scroll = layout.row_count().saturating_sub(text_height);
        self.scroll.min(max_scroll)
    }

    /// The spans of one wrapped row, with the selected bytes painted.
    ///
    /// The selection is a byte range in the buffer and it has to survive the
    /// wrap, so it is painted per **wrapped row**: every slice of the row
    /// carries the byte range of the source behind it, and the selected part
    /// of a slice is split out of it.
    ///
    /// The unselected bytes render exactly as they always did — one span in
    /// the theme's foreground — so a composer with no selection is
    /// byte-identical to the composer before the selection existed.
    ///
    /// The selected part is painted as a **background**: `theme.selection` is
    /// a fill, and the theme's `selection_text` is the token derived to read
    /// on that fill. Painting the selection colour as a foreground instead is
    /// the dark-on-dark defect the other selected surfaces in the TUI record.
    fn row_line(&self, row: &WrappedRow, selection: Option<(usize, usize)>) -> Line<'static> {
        let base = Style::default().fg(self.theme.foreground);
        let selected = Style::default()
            .fg(self.theme.selection_text)
            .bg(self.theme.selection);
        let mut spans: Vec<Span<'static>> = Vec::new();
        for slice in &row.slices {
            let Some((sel_start, sel_end)) = selection else {
                spans.push(Span::styled(slice.text.clone(), base));
                continue;
            };
            let lo = sel_start.max(slice.source_start);
            let hi = sel_end.min(slice.source_end);
            if lo >= hi {
                spans.push(Span::styled(slice.text.clone(), base));
                continue;
            }
            // A collapsed whitespace run is shorter than the source it
            // replaced, so it is painted whole: the run is one column and the
            // selection covers that column.
            if slice.text.len() != slice.source_end - slice.source_start {
                spans.push(Span::styled(slice.text.clone(), selected));
                continue;
            }
            let lo = lo - slice.source_start;
            let hi = hi - slice.source_start;
            if lo > 0 {
                spans.push(Span::styled(slice.text[..lo].to_string(), base));
            }
            spans.push(Span::styled(slice.text[lo..hi].to_string(), selected));
            if hi < slice.text.len() {
                spans.push(Span::styled(slice.text[hi..].to_string(), base));
            }
        }
        Line::from(spans)
    }
}

/// The byte at a visual row and a column.
///
/// The cell's first byte, so a multi-byte character's cell answers with that
/// character's start. A column past the row's last cell is the insertion
/// point at the row's end, and an empty row — a blank line — starts at its own
/// offset.
fn byte_at_visual(layout: &VisualLayout, row: usize, column: usize) -> usize {
    let cells = layout.row_cells(row);
    if let Some(cell) = cells.iter().find(|cell| cell.column == column) {
        return cell.byte_start;
    }
    // A column inside a wide character is that character's cell.
    if let Some(cell) = cells
        .iter()
        .find(|cell| cell.column <= column && column < cell.column + cell.width)
    {
        return cell.byte_start;
    }
    match cells.last() {
        Some(cell) => cell.byte_end,
        None => layout.row_source_start(row).unwrap_or(0),
    }
}

impl Default for PromptWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &PromptWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (text_height, marker_height) = self.box_split(area.height);

        // The mode row is the box's first row and does not scroll. It is the
        // badge and the prompt marker, and a box that scrolls them away
        // leaves the user without the mode they are typing in.
        let prefix_text = if self.command_mode {
            "[cmd] ".to_string()
        } else if self.waiting {
            "[...] ".to_string()
        } else {
            String::new()
        };
        let mode_row = Line::from(
            std::iter::once(Span::styled(
                prefix_text,
                Style::default().fg(self.theme.primary),
            ))
            .chain(self.mode_indicator.spans.iter().cloned())
            .chain(std::iter::once(Span::styled(
                self.prefix.content.to_string(),
                Style::default().fg(self.theme.primary),
            )))
            .collect::<Vec<_>>(),
        );
        Paragraph::new(mode_row).render(
            Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            },
            buf,
        );

        // The text window: the wrapped rows, scrolled by visual row.
        //
        // The rows are the wrap's, so a line wider than the box is broken at
        // a word boundary instead of being clipped at the right edge.
        //
        // The layout is built once and used for the rows and for the caret's
        // cell: the wrap is the expensive part of the redraw path, and the
        // cell has to agree with the rows that were drawn.
        //
        // The rows come from the **cache**, so a frame that changed neither
        // the text nor the width draws the rows it already wrapped.
        let layout = self.layout_at(area.width);
        let rows = self.rows_at(area.width);
        if text_height > 0 {
            let scroll = self.effective_scroll(&layout, text_height as usize);
            // The selection is a range in the **buffer**, so it means nothing
            // against the placeholder. An empty composer has no selection
            // either, and the placeholder path renders exactly as it always
            // did.
            let selection = if self.buffer.text().is_empty() {
                None
            } else {
                self.buffer.selection_range()
            };
            let text_lines: Vec<Line> = if self.buffer.text().is_empty() {
                vec![Line::from(Span::styled(
                    self.placeholder.clone(),
                    Style::default().fg(self.theme.foreground),
                ))]
            } else {
                rows.iter()
                    .skip(scroll)
                    .take(text_height as usize)
                    .map(|row| self.row_line(row, selection))
                    .collect()
            };
            Paragraph::new(text_lines)
                .scroll((0, self.horizontal_scroll.min(u16::MAX as usize) as u16))
                .render(
                    Rect {
                        x: area.x,
                        y: area.y.saturating_add(1),
                        width: area.width,
                        height: text_height,
                    },
                    buf,
                );
        }

        // The marker rows sit at the bottom of the box: the character count
        // and the multiline marker describe the text as a whole, so they
        // stay visible while the text scrolls under them.
        if marker_height > 0 {
            let mut marker_lines: Vec<Line> = Vec::new();
            let char_count_to_show = self.char_count.unwrap_or(self.buffer.text().len());
            if char_count_to_show > 500 {
                marker_lines.push(Line::from(Span::styled(
                    format!("  [{char_count_to_show} chars]"),
                    Style::default().fg(self.theme.muted),
                )));
            }
            if self.buffer.text().contains('\n') {
                marker_lines.push(Line::from(Span::styled(
                    "  (multiline)",
                    Style::default().fg(self.theme.muted),
                )));
            }
            Paragraph::new(marker_lines).render(
                Rect {
                    x: area.x,
                    y: area
                        .y
                        .saturating_add(area.height.saturating_sub(marker_height)),
                    width: area.width,
                    height: marker_height,
                },
                buf,
            );
        }

        // The solid block is the Normal-mode caret. In Insert mode the app
        // positions the terminal's own cursor at the same cell instead, so
        // the two are mutually exclusive by construction — painting both
        // would show two carets, and painting neither in Insert mode would
        // leave the user with no caret at all.
        if self.paints_block_cursor() {
            if let Some((cursor_x, cursor_y)) = self.caret_cell_in(area, &layout) {
                if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
                    cell.set_style(
                        Style::default()
                            .fg(self.theme.background)
                            .bg(self.theme.primary)
                            .add_modifier(Modifier::REVERSED),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    /// Render the composer into a buffer.
    ///
    /// The composer's first row is the mode/prefix row, so the text starts
    /// on row 1 — the tests below read the text from there.
    fn render(widget: &PromptWidget, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        widget.render(area, &mut buffer);
        buffer
    }

    fn composer(text: &str) -> PromptWidget {
        let mut widget = PromptWidget::new(Arc::new(Theme::dark()));
        widget.set_text(text.to_string());
        widget
    }

    /// The drawn text of one row, without its trailing blanks.
    fn row_text(buffer: &Buffer, width: u16, row: u16) -> String {
        (0..width)
            .map(|column| buffer[(column, row)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// An unselected composer renders exactly as it always did: the text in
    /// the theme's foreground, with no background of its own.
    #[test]
    fn an_unselected_composer_renders_as_before() {
        let widget = composer("hello");
        let buffer = render(&widget, 20, 4);

        let cell = &buffer[(0, 1)];
        assert_eq!(cell.symbol(), "h");
        assert_eq!(cell.fg, widget.theme.foreground);
        assert_eq!(cell.bg, Color::Reset, "no selection, no background");
    }

    /// A held selection is painted with the theme's selection colours.
    #[test]
    fn a_selected_range_is_painted_with_the_selection_colours() {
        let mut widget = composer("hello world");
        widget.set_cursor(0);
        widget.toggle_select_chars();
        widget.cursor_right();

        let buffer = render(&widget, 20, 4);

        // The selection is inclusive of the head character, so "he" is
        // painted.
        for column in 0..2 {
            let cell = &buffer[(column, 1)];
            assert_eq!(cell.bg, widget.theme.selection, "column {column}");
            assert_eq!(cell.fg, widget.theme.selection_text, "column {column}");
        }
        let cell = &buffer[(2, 1)];
        assert_eq!(cell.symbol(), "l");
        assert_eq!(cell.bg, Color::Reset, "the text after the selection");
        assert_eq!(cell.fg, widget.theme.foreground);
    }

    /// A selection that spans a newline is painted on both lines.
    #[test]
    fn a_selection_spanning_a_newline_is_painted_on_both_lines() {
        let mut widget = composer("one\ntwo");
        widget.set_cursor(2);
        widget.toggle_select_chars();
        // Move onto the second line: the head follows, and the selection
        // covers the newline between them.
        widget.cursor_right();
        widget.cursor_right();

        let buffer = render(&widget, 20, 4);

        assert_eq!(
            buffer[(2, 1)].bg,
            widget.theme.selection,
            "the first line's tail is selected"
        );
        assert_eq!(
            buffer[(0, 2)].bg,
            widget.theme.selection,
            "the second line's head is selected"
        );
    }

    /// The placeholder path is unaffected: an empty composer has no
    /// selection, and its placeholder renders in the muted colour.
    #[test]
    fn the_placeholder_path_is_unaffected() {
        let widget = PromptWidget::new(Arc::new(Theme::dark()));
        let buffer = render(&widget, 30, 4);

        assert_eq!(buffer[(0, 1)].symbol(), "A");
        assert_eq!(buffer[(0, 1)].bg, Color::Reset);
    }

    /// The selection's range is the same one the delete verb removes.
    #[test]
    fn the_widget_reports_the_selection_the_delete_verb_removes() {
        let mut widget = composer("hello world");
        widget.set_cursor(0);
        widget.toggle_select_chars();
        widget.cursor_right();

        assert_eq!(widget.selection_range(), Some((0, 2)));
        assert_eq!(widget.selected_text().as_deref(), Some("he"));
        assert!(widget.delete_selection());
        assert_eq!(widget.get_text(), "llo world");
    }

    /// A paste inserts the given text at the caret.
    ///
    /// The **text** is a parameter here, and that is the point: the clipboard
    /// itself is the system clipboard, which is global state that parallel
    /// tests would race on. The verb's input is therefore tested here, and
    /// the app's wiring — read the clipboard, hand the text over — is the
    /// same system boundary every other clipboard call in the app shares.
    #[test]
    fn a_paste_inserts_the_text_at_the_caret() {
        let mut widget = composer("hello");
        widget.set_cursor(5);
        widget.paste(" world".to_string());
        assert_eq!(widget.get_text(), "hello world");
        assert_eq!(widget.cursor_pos(), 11, "the caret follows the paste");
    }

    /// One row of text is one row of the box: the box's height is the mode
    /// row plus the **wrapped** text rows plus the marker rows.
    #[test]
    fn one_row_of_text_is_one_row_of_the_box() {
        let widget = composer("hello");
        assert_eq!(widget.layout_at(80).row_count(), 1);
        // The box's floor is three rows, so a one-row prompt leaves a row
        // spare.
        assert_eq!(widget.needed_height(20, 80), 3);

        // A second row of text takes the box to four rows.
        let widget = composer("hello\nworld");
        assert_eq!(widget.layout_at(80).row_count(), 2);
        assert_eq!(widget.needed_height(20, 80), 4);
    }

    /// The marker rows are inside the ten-row cap: the cap is on the box's
    /// **total** rows, the mode row and the text rows and the markers
    /// together.
    #[test]
    fn the_marker_rows_are_inside_the_ten_row_cap() {
        let mut widget = composer("a\nb\nc\nd\ne\nf\ng");
        widget.set_char_count(Some(1000));
        // The character count and the multiline marker are two marker rows.
        assert_eq!(widget.marker_rows(), 2);
        // Seven text rows, two marker rows and the mode row are the cap: the
        // markers are inside it.
        assert_eq!(widget.needed_height(20, 80), 10);
        // Without the character count the text gets one more row of the cap.
        widget.set_char_count(None);
        assert_eq!(widget.marker_rows(), 1);
        assert_eq!(widget.needed_height(20, 80), 9);
    }

    /// The pane's budget wins over the cap: a box that may not grow past the
    /// budget is exactly the budget.
    #[test]
    fn the_pane_budget_wins_over_the_cap() {
        let widget = composer(&"x".repeat(200));
        // The cap would allow ten rows.
        assert_eq!(widget.needed_height(10, 10), 10);
        // The budget allows six, and the budget wins.
        assert_eq!(widget.needed_height(6, 10), 6);
    }

    /// The composer wraps at word boundaries: a word that does not fit moves
    /// to the next row whole.
    #[test]
    fn a_word_that_does_not_fit_moves_to_the_next_row_whole() {
        let widget = composer("alpha beta gamma");
        let buffer = render(&widget, 11, 4);
        assert_eq!(row_text(&buffer, 11, 1), "alpha beta");
        assert_eq!(row_text(&buffer, 11, 2), "gamma");
    }

    /// A token wider than the row is hard-broken at a character boundary, so
    /// no character of it is lost.
    #[test]
    fn a_token_wider_than_the_row_is_hard_broken() {
        let widget = composer("abcdefghij");
        let buffer = render(&widget, 4, 4);
        assert_eq!(row_text(&buffer, 4, 1), "abcd");
        assert_eq!(row_text(&buffer, 4, 2), "efgh");
        assert_eq!(row_text(&buffer, 4, 3), "ij");
    }

    /// The caret's drawn cell is the cell of its byte in the wrapped layout:
    /// the column is the offset within the **wrapped row**, not within the
    /// logical line.
    #[test]
    fn the_caret_cell_is_the_cell_of_its_byte() {
        for text in [
            "hello world",
            "one\ntwo three",
            "a longer prompt than the row",
        ] {
            for width in [6u16, 10, 20] {
                let mut widget = composer(text);
                let area = Rect::new(0, 0, width, 12);
                for byte in 0..=text.len() {
                    widget.set_cursor(byte);
                    let caret = widget.cursor_pos();
                    let layout = widget.layout_at(width);
                    let position = layout.caret_position(caret).expect("the caret has a cell");
                    let expected = Some((
                        position.column.min(width as usize - 1) as u16,
                        1 + position.row as u16,
                    ));
                    assert_eq!(
                        widget.caret_cell(area),
                        expected,
                        "text {text:?} at width {width}, byte {byte}"
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // The fit fact
    // -----------------------------------------------------------------------

    /// A text long enough to wrap onto more rows than a small box shows.
    fn long_text(words: usize) -> String {
        (0..words).map(|index| format!("w{index:02} ")).collect()
    }

    /// The fit is unknown until the layout pass measures it.
    ///
    /// The gate runs before the render pass, so a composer that has never
    /// been drawn has no measurement — and an unmeasured composer does not
    /// overflow. The key falls through rather than being swallowed in a state
    /// the user cannot see.
    #[test]
    fn the_fit_is_unknown_until_the_layout_measures_it() {
        let widget = composer("alpha beta gamma");

        assert_eq!(
            widget.content_fit(),
            ComposerFit::Unknown,
            "an unlaid-out composer has no measurement"
        );
        assert!(
            !widget.content_overflows(),
            "an unmeasured composer does not overflow"
        );
    }

    /// A composer whose content wraps to **exactly** the box's height fits.
    ///
    /// The exact-fit case is the one that is easy to get backwards: three rows
    /// in a three-row box is not an overflow, and the key must fall through.
    #[test]
    fn a_composer_whose_content_wraps_to_exactly_the_box_height_fits() {
        let mut widget = composer(&long_text(24));
        widget.set_wrap_width(10);
        let rows = widget.layout_at(10).row_count();
        assert!(rows > 1, "the text must wrap onto more than one row");

        // The box shows exactly as many rows as the content occupies.
        widget.ensure_cursor_visible_with_width(rows, 10);

        assert!(
            !widget.content_overflows(),
            "{rows} rows in a {rows}-row box fits: the content is not cut off"
        );
    }

    /// One row more than the box shows does not fit.
    #[test]
    fn a_composer_whose_content_overflows_its_box_overflows() {
        let mut widget = composer(&long_text(24));
        widget.set_wrap_width(10);
        let rows = widget.layout_at(10).row_count();

        widget.ensure_cursor_visible_with_width(rows - 1, 10);

        assert!(
            widget.content_overflows(),
            "{rows} rows in a {}-row box does not fit",
            rows - 1
        );
    }

    // -----------------------------------------------------------------------
    // The visual-row motions
    // -----------------------------------------------------------------------

    /// `j` moves the caret one **visual row** down.
    ///
    /// The row is a row of the wrapped text, not a line of the buffer: the
    /// text here has no newline in it at all, and it still occupies several
    /// rows.
    #[test]
    fn the_visual_row_motion_moves_the_caret_down_one_visual_row() {
        let mut widget = composer("alpha beta gamma delta epsilon zeta");
        widget.set_wrap_width(10);
        let layout = widget.layout_at(10);
        assert!(
            layout.row_count() > 1,
            "the text must wrap onto more than one row"
        );
        widget.set_cursor(0);

        widget.cursor_visual_row_down();

        let position = layout
            .caret_position(widget.cursor_pos())
            .expect("the caret is on a row");
        assert_eq!(position.row, 1, "`j` moved the caret one visual row down");
        assert_eq!(
            widget.cursor_pos(),
            layout.byte_at(1, 0).expect("the second row has a cell"),
            "the caret lands on the byte the geometry names"
        );
    }

    /// The caret's drawn cell is the cell of the byte it names.
    #[test]
    fn the_visual_row_motion_leaves_the_caret_on_the_cell_it_names() {
        let mut widget = composer("alpha beta gamma delta epsilon zeta");
        widget.set_wrap_width(10);
        widget.set_cursor(0);
        let area = Rect::new(0, 0, 10, 6);

        widget.cursor_visual_row_down();

        let layout = widget.layout_at(10);
        let position = layout
            .caret_position(widget.cursor_pos())
            .expect("the caret is on a row");
        assert_eq!(
            widget.caret_cell(area),
            Some((
                area.x + position.column as u16,
                area.y + 1 + position.row as u16,
            )),
            "the caret is drawn on the cell of the byte it names"
        );
    }

    /// The motion clamps at the last visual row.
    #[test]
    fn the_visual_row_motion_clamps_at_the_last_visual_row() {
        let mut widget = composer("alpha beta gamma delta");
        widget.set_wrap_width(10);
        let layout = widget.layout_at(10);
        let last_row = layout.row_count() - 1;
        widget.set_cursor(
            layout
                .byte_at(last_row, 0)
                .expect("the last row has a cell"),
        );
        let before = widget.cursor_pos();

        widget.cursor_visual_row_down();

        assert_eq!(
            widget.cursor_pos(),
            before,
            "the caret stays on the last visual row: there is nowhere to go"
        );
    }

    /// The motion clamps at the first visual row.
    #[test]
    fn the_visual_row_motion_clamps_at_the_first_visual_row() {
        let mut widget = composer("alpha beta gamma delta");
        widget.set_wrap_width(10);
        widget.set_cursor(0);
        let before = widget.cursor_pos();

        widget.cursor_visual_row_up();

        assert_eq!(
            widget.cursor_pos(),
            before,
            "the caret stays on the first visual row: there is nowhere to go"
        );
    }

    /// The motion lands on the geometry's answer at **every** byte.
    ///
    /// The sweep is over every byte of the text, because the interesting bytes
    /// are the awkward ones: a line break, the byte a row ends on, the last
    /// byte of the buffer.
    #[test]
    fn the_visual_row_motion_lands_on_the_geometrys_answer_at_every_byte() {
        let text = "alpha beta\ngamma delta epsilon";
        for width in [8u16, 11, 20] {
            let mut widget = composer(text);
            widget.set_wrap_width(width);
            let layout = widget.layout_at(width);
            assert!(
                layout.row_count() > 1,
                "the text must wrap onto more than one row at width {width}"
            );
            for byte in 0..=text.len() {
                widget.set_cursor(byte);
                widget.cursor_visual_row_down();

                let position = layout.caret_position(byte).expect("the caret is on a row");
                let target = (position.row + 1).min(layout.row_count() - 1);
                let expected = if target == position.row {
                    // Clamped at the last row: the caret stays put.
                    byte
                } else {
                    layout
                        .byte_at(target, position.column)
                        .or_else(|| layout.row_cells(target).last().map(|cell| cell.byte_end))
                        .unwrap_or_else(|| layout.row_source_start(target).unwrap_or(0))
                };
                assert_eq!(
                    widget.cursor_pos(),
                    expected,
                    "text {text:?} at width {width}, byte {byte}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The page motions
    // -----------------------------------------------------------------------

    /// The page size is the transcript's rule: the visible height minus two,
    /// with a floor of one.
    #[test]
    fn the_page_size_is_the_visible_height_minus_two() {
        let mut widget = composer("alpha beta");
        widget.set_wrap_width(10);

        widget.ensure_cursor_visible_with_width(6, 10);
        assert_eq!(
            widget.page_size(),
            Some(4),
            "the page is the visible height minus two"
        );

        widget.ensure_cursor_visible_with_width(2, 10);
        assert_eq!(widget.page_size(), Some(1), "the page has a floor of one");
    }

    /// The page moves the window and the caret together.
    ///
    /// The caret is always visible, so a window that paged without the caret
    /// would be undone by the next layout pass — the two move as one.
    #[test]
    fn the_page_motion_pages_the_window_and_the_caret_together() {
        let mut widget = composer(&long_text(24));
        widget.set_wrap_width(10);
        let rows = widget.layout_at(10).row_count();
        let visible_rows = 6;
        widget.ensure_cursor_visible_with_width(visible_rows, 10);
        assert!(
            widget.content_overflows(),
            "{rows} rows of content do not fit in a {visible_rows}-row box"
        );
        widget.set_cursor(widget.layout_at(10).byte_at(0, 0).expect("a first cell"));
        // The layout pass scrolls the window to the caret, so the window
        // starts at the first row.
        widget.ensure_cursor_visible_with_width(visible_rows, 10);
        assert_eq!(widget.scroll, 0, "the window starts at the first row");

        widget.page_down();

        let page = visible_rows.saturating_sub(2).max(1);
        let position = widget
            .layout_at(10)
            .caret_position(widget.cursor_pos())
            .expect("the caret is on a row");
        assert_eq!(position.row, page, "the caret paged with the window");
        assert_eq!(widget.scroll, page, "the window paged by the same amount");
    }

    /// A composer that fits has nothing to page.
    #[test]
    fn the_page_motion_does_nothing_when_the_content_fits() {
        let mut widget = composer("alpha beta");
        widget.set_wrap_width(10);
        let rows = widget.layout_at(10).row_count();
        widget.ensure_cursor_visible_with_width(rows, 10);
        widget.set_cursor(0);

        widget.page_down();

        assert_eq!(widget.scroll, 0, "a composer that fits does not page");
        assert_eq!(
            widget.cursor_pos(),
            0,
            "a composer that fits does not move its caret"
        );
    }
}
