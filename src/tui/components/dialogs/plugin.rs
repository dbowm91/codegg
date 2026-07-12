use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct PluginDialog {
    id: String,
    title: String,
    lines: Vec<String>,
    scroll: usize,
    theme: Arc<Theme>,
}

impl PluginDialog {
    pub fn new(id: String, title: String, lines: Vec<String>, theme: Arc<Theme>) -> Self {
        Self {
            id,
            title,
            lines,
            scroll: 0,
            theme,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn update_content(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.scroll = 0;
    }
}

impl Component for PluginDialog {
    fn dialog_type(&self) -> DialogType {
        DialogType::Plugin
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                let max_scroll = self.lines.len().saturating_sub(1);
                if self.scroll < max_scroll {
                    self.scroll += 1;
                }
                None
            }
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, _theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        let title_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .border_style(Style::default().fg(self.theme.border));

        // The content paragraph has its own two-row border. Derive the
        // viewport from the actual content chunk so the footer does not make
        // the last rows unreachable.
        let visible_lines = (chunks[1].height as usize).saturating_sub(2);
        let total_lines = self.lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let display_lines: Vec<Line> = self.lines[start_idx..end_idx]
            .iter()
            .map(|s| {
                Line::from(Span::styled(
                    s.as_str(),
                    Style::default().fg(self.theme.foreground),
                ))
            })
            .collect();

        let content_para = Paragraph::new(display_lines)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.border)),
            );

        let scroll_indicator = if total_lines > visible_lines {
            format!(" {}-{} of {} ", start_idx + 1, end_idx, total_lines)
        } else {
            String::new()
        };

        let footer_text = if scroll_indicator.is_empty() {
            " j/k scroll | Esc close ".to_string()
        } else {
            format!("{} | j/k scroll | Esc close ", scroll_indicator.trim())
        };

        let footer_block = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(self.theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );

        frame.render_widget(title_block, chunks[0]);
        frame.render_widget(content_para, chunks[1]);
        frame.render_widget(footer_block, chunks[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Theme;

    fn test_theme() -> Arc<Theme> {
        Arc::new(Theme::dark())
    }

    #[test]
    fn test_new_plugin_dialog() {
        let theme = test_theme();
        let dialog = PluginDialog::new(
            "test-id".into(),
            "Test Plugin".into(),
            vec![],
            Arc::clone(&theme),
        );
        assert_eq!(dialog.id(), "test-id");
        assert_eq!(dialog.title, "Test Plugin");
    }

    #[test]
    fn test_scroll_down() {
        let theme = test_theme();
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines, Arc::clone(&theme));

        assert_eq!(dialog.scroll, 0);
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 1);
    }

    #[test]
    fn test_scroll_up() {
        let theme = test_theme();
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines, Arc::clone(&theme));

        dialog.scroll = 5;
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 4);
    }

    #[test]
    fn test_scroll_up_at_zero_stays_at_zero() {
        let theme = test_theme();
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines, Arc::clone(&theme));

        assert_eq!(dialog.scroll, 0);
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_scroll_down_capped() {
        let theme = test_theme();
        let lines: Vec<String> = (0..3).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines, Arc::clone(&theme));

        // scroll past the end
        dialog.scroll = 2;
        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 2);
    }

    #[test]
    fn test_vim_keys() {
        let theme = test_theme();
        let lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), lines, Arc::clone(&theme));

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('j'),
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 1);

        dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('k'),
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_esc_closes_dialog() {
        let theme = test_theme();
        let mut dialog = PluginDialog::new("id".into(), "Title".into(), vec![], Arc::clone(&theme));

        let msg = dialog.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        assert_eq!(msg, Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn test_update_content_replaces_lines_and_resets_scroll() {
        let theme = test_theme();
        let initial_lines: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let mut dialog = PluginDialog::new(
            "id".into(),
            "Title".into(),
            initial_lines,
            Arc::clone(&theme),
        );

        dialog.scroll = 10;
        let new_lines: Vec<String> = (0..5).map(|i| format!("new {}", i)).collect();
        dialog.update_content(new_lines);

        assert_eq!(dialog.lines.len(), 5);
        assert_eq!(dialog.lines[0], "new 0");
        assert_eq!(dialog.scroll, 0);
    }

    #[test]
    fn test_dialog_type() {
        let theme = test_theme();
        let dialog = PluginDialog::new("id".into(), "Title".into(), vec![], Arc::clone(&theme));
        assert_eq!(dialog.dialog_type(), DialogType::Plugin);
    }
}
