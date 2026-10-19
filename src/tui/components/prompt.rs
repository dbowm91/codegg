use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;

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
    pub scroll: usize,
    /// Horizontal offset used to keep the cursor visible on long prompt lines.
    pub horizontal_scroll: usize,
    pub command_mode: bool,
    pub waiting: bool,
    pub char_count: Option<usize>,
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
            command_mode: false,
            waiting: false,
            char_count: None,
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

    pub fn set_text(&mut self, text: String) {
        self.buffer.set_text(text);
        self.horizontal_scroll = 0;
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.buffer.set_caret(pos);
    }

    /// Place the cursor at a display column on a logical prompt line.
    ///
    /// Mouse coordinates are cell-based while the caret is a UTF-8 byte
    /// offset. Converting here keeps wide and multibyte characters from
    /// leaving the cursor inside a code point and also makes multiline mouse
    /// placement behave like keyboard navigation.
    pub fn set_cursor_at_column(&mut self, line_index: usize, column: usize) {
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

    pub fn paste(&mut self, text: String) {
        self.buffer.insert(&text);
    }

    pub fn num_lines(&self) -> usize {
        self.buffer.text().split('\n').count()
    }

    pub fn cursor_line(&self) -> usize {
        self.buffer.caret_line()
    }

    pub fn needed_height(&self, max_height: u16) -> u16 {
        let min_height: u16 = 3;
        let total_lines = self.num_lines() as u16;
        let char_count_lines = if self.char_count.unwrap_or(self.buffer.text().len()) > 500 {
            1
        } else {
            0
        };
        let multiline_indicator = if self.buffer.text().contains('\n') {
            1
        } else {
            0
        };
        let total_needed = 1 + total_lines + char_count_lines + multiline_indicator;
        total_needed.clamp(min_height, max_height)
    }

    pub fn clamp_scroll(&mut self, visible_lines: usize) {
        let total_lines = self.num_lines();
        if total_lines <= visible_lines {
            self.scroll = 0;
        } else {
            let max_scroll = total_lines.saturating_sub(visible_lines);
            let cursor_line = self.cursor_line();
            if cursor_line < self.scroll {
                self.scroll = cursor_line;
            } else if cursor_line >= self.scroll + visible_lines {
                self.scroll = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
            }
            self.scroll = self.scroll.min(max_scroll);
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

    pub fn ensure_cursor_visible(&mut self, visible_lines: usize) {
        self.ensure_cursor_visible_with_width(visible_lines, usize::MAX);
    }

    /// Keep the cursor visible vertically and horizontally.
    ///
    /// The one-argument variant remains useful to callers that only have a
    /// vertical viewport. TUI rendering should use this width-aware variant
    /// so long single-line prompts do not leave the cursor off-screen.
    pub fn ensure_cursor_visible_with_width(&mut self, visible_lines: usize, visible_width: usize) {
        let cursor_line = self.cursor_line();
        let total_lines = self.num_lines();
        if total_lines <= visible_lines {
            self.scroll = 0;
        } else {
            let max_scroll = total_lines.saturating_sub(visible_lines);
            if cursor_line < self.scroll {
                self.scroll = cursor_line;
            } else if cursor_line >= self.scroll + visible_lines {
                self.scroll = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
            }
            self.scroll = self.scroll.min(max_scroll);
        }

        if visible_width == usize::MAX || visible_width == 0 {
            if visible_width == 0 {
                self.horizontal_scroll = 0;
            }
            return;
        }

        let before = &self.buffer.text()[..self.buffer.caret().min(self.buffer.text().len())];
        let column = before
            .rsplit('\n')
            .next()
            .map(UnicodeWidthStr::width)
            .unwrap_or(0);

        if column < self.horizontal_scroll {
            self.horizontal_scroll = column;
        } else if column >= self.horizontal_scroll.saturating_add(visible_width) {
            self.horizontal_scroll = column.saturating_sub(visible_width.saturating_sub(1));
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
    /// row below it. The paragraph scrolls as a whole, so the text line the
    /// caret is on sits at `area.y + 1 + (line - scroll)`, and a caret whose
    /// line has scrolled off the top has no cell at all.
    pub fn caret_cell(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let before = &self.buffer.text()[..self.buffer.caret().min(self.buffer.text().len())];
        let line_idx = before.chars().filter(|&c| c == '\n').count();
        if line_idx < self.scroll {
            // The caret's line is above the visible window.
            return None;
        }
        let visible_line_idx = u16::try_from(line_idx - self.scroll).ok()?;
        let cursor_y = area.y.checked_add(1)?.checked_add(visible_line_idx)?;
        if cursor_y >= area.bottom() {
            return None;
        }
        let col = before
            .rsplit('\n')
            .next()
            .map(UnicodeWidthStr::width)
            .unwrap_or(0);
        let col = col.saturating_sub(self.horizontal_scroll);
        let cursor_x = area
            .x
            .checked_add(col.min(area.width.saturating_sub(1) as usize) as u16)?;
        if cursor_x >= area.right() {
            return None;
        }
        Some((cursor_x, cursor_y))
    }

    /// Whether the composer paints a solid block at the caret.
    pub fn paints_block_cursor(&self) -> bool {
        self.cursor_style == ComposerCursor::Block
    }
}

impl Default for PromptWidget {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Widget for &PromptWidget {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut all_lines: Vec<Line> = Vec::new();

        let prefix_text = if self.command_mode {
            "[cmd] ".to_string()
        } else if self.waiting {
            "[...] ".to_string()
        } else {
            String::new()
        };

        all_lines.push(Line::from(
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
        ));

        let text_lines: Vec<Line> = {
            let text = if self.buffer.text().is_empty() {
                &self.placeholder
            } else {
                self.buffer.text()
            };
            text.split('\n')
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(self.theme.foreground),
                    ))
                })
                .collect()
        };
        all_lines.extend(text_lines);

        let char_count_to_show = self.char_count.unwrap_or(self.buffer.text().len());
        if char_count_to_show > 500 {
            all_lines.push(Line::from(Span::styled(
                format!("  [{char_count_to_show} chars]"),
                Style::default().fg(self.theme.muted),
            )));
        }

        if self.buffer.text().contains('\n') {
            all_lines.push(Line::from(Span::styled(
                "  (multiline)",
                Style::default().fg(self.theme.muted),
            )));
        }

        let paragraph = Paragraph::new(all_lines).scroll((
            self.scroll.min(u16::MAX as usize) as u16,
            self.horizontal_scroll.min(u16::MAX as usize) as u16,
        ));
        paragraph.render(area, buf);

        // The solid block is the Normal-mode caret. In Insert mode the app
        // positions the terminal's own cursor at the same cell instead, so
        // the two are mutually exclusive by construction — painting both
        // would show two carets, and painting neither in Insert mode would
        // leave the user with no caret at all.
        if self.paints_block_cursor() {
            if let Some((cursor_x, cursor_y)) = self.caret_cell(area) {
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
