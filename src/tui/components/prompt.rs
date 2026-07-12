use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;

pub struct PromptWidget {
    pub text: String,
    pub cursor: usize,
    pub focused: bool,
    pub theme: Arc<Theme>,
    pub prefix: Span<'static>,
    pub mode_indicator: Span<'static>,
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
            text: String::new(),
            cursor: 0,
            focused: true,
            theme,
            prefix: Span::raw("❯ "),
            mode_indicator: Span::raw(""),
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

    pub fn set_mode_indicator(&mut self, indicator: Span<'static>) {
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

    pub fn focus(&mut self) {
        self.focused = true;
    }

    pub fn blur(&mut self) {
        self.focused = false;
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn get_text(&self) -> String {
        self.text.clone()
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.len();
        self.horizontal_scroll = 0;
    }

    pub fn set_cursor(&mut self, pos: usize) {
        let pos = pos.min(self.text.len());
        self.cursor = if self.text.is_char_boundary(pos) {
            pos
        } else {
            self.text
                .char_indices()
                .map(|(boundary, _)| boundary)
                .take_while(|&boundary| boundary < pos)
                .last()
                .unwrap_or(0)
        };
    }

    /// Place the cursor at a display column on a logical prompt line.
    ///
    /// Mouse coordinates are cell-based while `cursor` is a UTF-8 byte
    /// offset. Converting here keeps wide and multibyte characters from
    /// leaving the cursor inside a code point and also makes multiline mouse
    /// placement behave like keyboard navigation.
    pub fn set_cursor_at_column(&mut self, line_index: usize, column: usize) {
        let mut line_start = 0;
        for (current_line, line) in self.text.split('\n').enumerate() {
            if current_line == line_index {
                let mut display_column: usize = 0;
                for (byte_offset, ch) in line.char_indices() {
                    let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if column < display_column.saturating_add(width) {
                        self.cursor = line_start + byte_offset;
                        return;
                    }
                    display_column = display_column.saturating_add(width);
                }
                self.cursor = line_start + line.len();
                return;
            }
            line_start += line.len() + 1;
        }
        self.cursor = self.text.len();
    }

    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.horizontal_scroll = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.text.insert(self.cursor, '\n');
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.text[..self.cursor];
        let ch_len = before
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        let new_cursor = self.cursor - ch_len;
        self.text.drain(new_cursor..self.cursor);
        self.cursor = new_cursor;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let ch_len = self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.text.drain(self.cursor..self.cursor + ch_len);
    }

    pub fn cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.text[..self.cursor];
        let ch_len = before
            .chars()
            .next_back()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.cursor -= ch_len;
    }

    pub fn cursor_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let ch_len = self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        self.cursor += ch_len;
    }

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn paste(&mut self, text: String) {
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }

    pub fn num_lines(&self) -> usize {
        if self.text.is_empty() {
            1
        } else {
            let base_lines = self.text.lines().count();
            if self.text.ends_with('\n') {
                base_lines + 1
            } else {
                base_lines
            }
        }
    }

    pub fn cursor_line(&self) -> usize {
        let before = &self.text[..self.cursor.min(self.text.len())];
        before.chars().filter(|&c| c == '\n').count()
    }

    pub fn needed_height(&self, max_height: u16) -> u16 {
        let min_height: u16 = 3;
        let total_lines = self.num_lines() as u16;
        let char_count_lines = if self.char_count.unwrap_or(self.text.len()) > 500 {
            1
        } else {
            0
        };
        let multiline_indicator = if self.text.contains('\n') { 1 } else { 0 };
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

        let before = &self.text[..self.cursor.min(self.text.len())];
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

        all_lines.push(Line::from(Span::styled(
            format!(
                "{}{}{}",
                prefix_text, self.mode_indicator.content, self.prefix.content
            ),
            Style::default().fg(self.theme.primary),
        )));

        let text_lines = if self.text.is_empty() && !self.focused {
            vec![Line::from(Span::styled(
                &self.placeholder,
                Style::default().fg(self.theme.muted),
            ))]
        } else {
            let text = if self.text.is_empty() {
                &self.placeholder
            } else {
                &self.text
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

        let char_count_to_show = self.char_count.unwrap_or(self.text.len());
        if char_count_to_show > 500 {
            all_lines.push(Line::from(Span::styled(
                format!("  [{char_count_to_show} chars]"),
                Style::default().fg(self.theme.muted),
            )));
        }

        if self.text.contains('\n') {
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

        if self.focused && !self.text.is_empty() {
            let before = &self.text[..self.cursor.min(self.text.len())];
            let line_idx = before.chars().filter(|&c| c == '\n').count();
            let col = before
                .rsplit('\n')
                .next()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0);
            let cursor_x = area.x
                + col
                    .saturating_sub(self.horizontal_scroll)
                    .min(area.width.saturating_sub(1) as usize) as u16;
            let visible_line_idx = line_idx.saturating_sub(self.scroll);
            let cursor_y = area.y + 1 + visible_line_idx as u16;
            if cursor_y < area.bottom() && cursor_x < area.right() {
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
