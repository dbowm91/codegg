use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::components::diff::{DiffMode, DiffViewer};

#[derive(Clone)]
pub struct DiffDialog {
    pub viewer: DiffViewer,
}

impl DiffDialog {
    pub fn new(old_content: Box<str>, new_content: Box<str>, title: Box<str>) -> Self {
        Self {
            viewer: DiffViewer::new(old_content, new_content, title),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.viewer.set_theme(theme);
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
                if key.modifiers.is_empty() =>
            {
                self.viewer.handle_scroll(-1);
                true
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
                if key.modifiers.is_empty() =>
            {
                self.viewer.handle_scroll(1);
                true
            }
            crossterm::event::KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.viewer.toggle_mode();
                true
            }
            crossterm::event::KeyCode::PageUp => {
                self.viewer.handle_scroll(-10);
                true
            }
            crossterm::event::KeyCode::PageDown => {
                self.viewer.handle_scroll(10);
                true
            }
            crossterm::event::KeyCode::Home => {
                self.viewer.scroll.reset();
                true
            }
            crossterm::event::KeyCode::End => {
                self.viewer.scroll_to_bottom();
                true
            }
            _ => false,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.height < 7 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.viewer.theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(content_height),
            Constraint::Length(1),
        ])
        .split(inner);

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(self.viewer.theme.primary)),
            Span::styled(
                &self.viewer.title,
                Style::default().fg(self.viewer.theme.foreground),
            ),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(self.viewer.theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        frame.render_widget(header, chunks[0]);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => self.viewer.theme.error,
                    similar::ChangeTag::Insert => self.viewer.theme.success,
                    similar::ChangeTag::Equal => self.viewer.theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        let mode_toggle = match self.viewer.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!(
            "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
            mode_toggle
        );

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(self.viewer.theme.muted),
        )));

        frame.render_widget(footer, chunks[2]);
    }
}

impl Widget for &DiffDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.viewer.theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, buf);

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(content_height),
            Constraint::Length(1),
        ])
        .split(inner);

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(self.viewer.theme.primary)),
            Span::styled(
                &self.viewer.title,
                Style::default().fg(self.viewer.theme.foreground),
            ),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(self.viewer.theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        header.render(chunks[0], buf);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => self.viewer.theme.error,
                    similar::ChangeTag::Insert => self.viewer.theme.success,
                    similar::ChangeTag::Equal => self.viewer.theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                styled_line.render(line_area, buf);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        let mode_toggle = match self.viewer.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!(
            "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
            mode_toggle
        );

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(self.viewer.theme.muted),
        )));

        footer.render(chunks[2], buf);
    }
}

impl Component for DiffDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                self.viewer.handle_scroll(-1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                self.viewer.handle_scroll(1);
                None
            }
            KeyCode::Char('s') if key.modifiers.is_empty() => {
                self.viewer.toggle_mode();
                None
            }
            KeyCode::PageUp => {
                self.viewer.handle_scroll(-10);
                None
            }
            KeyCode::PageDown => {
                self.viewer.handle_scroll(10);
                None
            }
            KeyCode::Home => {
                self.viewer.scroll.reset();
                None
            }
            KeyCode::End => {
                self.viewer.scroll_to_bottom();
                None
            }
            KeyCode::Enter | KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        if area.height < 7 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.viewer.theme.border))
            .title(" Diff Viewer ");

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let content_height = inner.height.saturating_sub(2);
        if content_height == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(content_height),
            Constraint::Length(1),
        ])
        .split(inner);

        let mode_str = match self.viewer.mode {
            DiffMode::Inline => "Inline",
            DiffMode::SideBySide => "Side-by-Side",
        };

        let title_line = Line::from(vec![
            Span::styled("Diff: ", Style::default().fg(self.viewer.theme.primary)),
            Span::styled(
                &self.viewer.title,
                Style::default().fg(self.viewer.theme.foreground),
            ),
            Span::raw(" | "),
            Span::styled(mode_str, Style::default().fg(self.viewer.theme.secondary)),
        ]);

        let header = ratatui::widgets::Paragraph::new(title_line);
        frame.render_widget(header, chunks[0]);

        let visible_lines = content_height as usize;
        let scroll_offset = self.viewer.scroll.get();
        let mut y = 0u16;

        for hunk in &self.viewer.hunks {
            for line in hunk.lines.iter().skip(scroll_offset) {
                if y as usize >= visible_lines {
                    break;
                }

                let fg_color = match line.tag {
                    similar::ChangeTag::Delete => self.viewer.theme.error,
                    similar::ChangeTag::Insert => self.viewer.theme.success,
                    similar::ChangeTag::Equal => self.viewer.theme.foreground,
                };

                let old_num = line
                    .line_number_old
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_num = line
                    .line_number_new
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let prefix = match line.tag {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };

                let content = format!("{}{} │ {}", old_num, new_num, line.content);
                let full_line = format!("{}{}", prefix, &content[1..]);

                let styled_line =
                    Line::from(vec![Span::styled(full_line, Style::default().fg(fg_color))]);

                let line_area = Rect::new(chunks[1].x, chunks[1].y + y, chunks[1].width, 1);
                frame.render_widget(styled_line, line_area);

                y += 1;
            }

            if y as usize >= visible_lines {
                break;
            }
        }

        let mode_toggle = match self.viewer.mode {
            DiffMode::Inline => "Press 's' for Side-by-Side",
            DiffMode::SideBySide => "Press 's' for Inline",
        };

        let info = format!(
            "j/k/↑/↓ scroll  |  PgUp/PgDn  |  {}  |  Esc close",
            mode_toggle
        );

        let footer = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            info,
            Style::default().fg(self.viewer.theme.muted),
        )));

        frame.render_widget(footer, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Diff
    }
}
