use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::tui::app::TuiMsg;
use crate::tui::command::{Command, COMMAND_REGISTRY};
use crossterm::event::KeyEvent;

#[derive(Clone)]
pub struct CommandPalette {
    pub query: String,
    pub filtered: Vec<&'static Command>,
    pub cursor: usize,
    pub scroll: usize,
    visible_height: usize,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            filtered: Vec::new(),
            cursor: 0,
            scroll: 0,
            visible_height: 7,
        }
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        let results = COMMAND_REGISTRY.filter(&self.query);
        self.filtered = results.into_iter().map(|(cmd, _)| cmd).collect();
        self.cursor = 0;
        self.scroll = 0;
    }

    pub fn cursor_down(&mut self) {
        if self.cursor < self.filtered.len().saturating_sub(1) {
            self.cursor += 1;
        }
        self.clamp_scroll();
    }

    pub fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max_visible = self.visible_height;
        if self.cursor >= self.scroll + max_visible {
            self.scroll = self.cursor.saturating_sub(max_visible.saturating_sub(1));
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
    }

    pub fn selected(&self) -> Option<&'static Command> {
        self.filtered.get(self.cursor).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub fn visible_count(&self) -> usize {
        self.filtered.len()
    }

    pub fn render(&mut self, frame: &mut Frame, prompt_area: Rect, theme: &Arc<Theme>) {
        if self.filtered.is_empty() {
            let max_h = 3u16;
            let compl_h = max_h + 2;
            let compl_w = 50.min(prompt_area.width.saturating_sub(2));
            let compl_area = Rect {
                x: prompt_area.x + 1,
                y: prompt_area.y.saturating_sub(compl_h),
                width: compl_w,
                height: compl_h,
            };
            frame.render_widget(ratatui::widgets::Clear, compl_area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background));
            let hint = Span::styled(" No results ", Style::default().fg(theme.muted));
            frame.render_widget(
                Paragraph::new(hint)
                    .block(block)
                    .alignment(Alignment::Center),
                compl_area,
            );
            return;
        }

        let max_visible = self.visible_height;
        let max_h = (max_visible as u16).min(self.filtered.len() as u16);
        let hints_h = 1u16;
        let compl_h = max_h + hints_h + 1;
        let compl_w = 50.min(prompt_area.width.saturating_sub(2));
        let compl_area = Rect {
            x: prompt_area.x + 1,
            y: prompt_area.y.saturating_sub(compl_h),
            width: compl_w,
            height: compl_h,
        };

        frame.render_widget(ratatui::widgets::Clear, compl_area);

        let list_height = max_h + 1;
        let list_area = Rect {
            x: compl_area.x,
            y: compl_area.y,
            width: compl_area.width,
            height: list_height,
        };
        let hints_area = Rect {
            x: compl_area.x,
            y: compl_area.y + list_height,
            width: compl_area.width,
            height: hints_h,
        };

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .skip(self.scroll)
            .take(max_visible)
            .enumerate()
            .map(|(i, cmd)| {
                let is_selected = (i + self.scroll) == self.cursor;
                let base_style = if is_selected {
                    Style::default().fg(theme.background).bg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };

                let name_style = if is_selected {
                    base_style.add_modifier(Modifier::BOLD)
                } else {
                    base_style
                };

                let cat_style = if is_selected {
                    Style::default().fg(theme.background).bg(theme.muted)
                } else {
                    Style::default().fg(theme.muted)
                };

                let mut spans = vec![
                    Span::styled(format!("{:?} ", cmd.category), cat_style),
                    Span::styled(&cmd.name, name_style),
                ];

                if !cmd.aliases.is_empty() {
                    spans.push(Span::raw(" ("));
                    spans.push(Span::styled(
                        cmd.aliases.join(", "),
                        if is_selected {
                            Style::default().fg(theme.background)
                        } else {
                            Style::default().fg(theme.muted)
                        },
                    ));
                    spans.push(Span::raw(")"));
                }

                if !cmd.description.is_empty() {
                    spans.push(Span::raw(" - "));
                    spans.push(Span::styled(
                        &cmd.description,
                        if is_selected {
                            Style::default().fg(theme.background)
                        } else {
                            Style::default().fg(theme.secondary)
                        },
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));
        let list = List::new(items).block(block);
        frame.render_widget(list, list_area);

        let hint_style = Style::default().fg(theme.muted);
        let hint_text = Line::from(vec![
            Span::styled(" \u{2191}\u{2193} ", hint_style),
            Span::styled("navigate", hint_style),
            Span::styled(" \u{00B7} ", hint_style),
            Span::styled("Enter ", hint_style),
            Span::styled("select", hint_style),
            Span::styled(" \u{00B7} ", hint_style),
            Span::styled("Esc ", hint_style),
            Span::styled("cancel", hint_style),
        ]);
        let hints_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));
        frame.render_widget(
            Paragraph::new(hint_text)
                .block(hints_block)
                .alignment(Alignment::Center),
            hints_area,
        );
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CommandPalette {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor_down();
            }
            KeyCode::Esc => {
                return Some(TuiMsg::CloseDialog);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render(frame, area, theme);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_down_empty() {
        let mut palette = CommandPalette::new();
        palette.filtered = vec![];
        palette.visible_height = 5;

        palette.cursor_down();
        assert_eq!(palette.cursor, 0);
    }

    #[test]
    fn test_cursor_up_empty() {
        let mut palette = CommandPalette::new();
        palette.filtered = vec![];
        palette.visible_height = 5;
        palette.cursor = 1;

        palette.cursor_up();
        assert_eq!(palette.cursor, 0);
    }

    #[test]
    fn test_cursor_clamp_scroll_empty() {
        let mut palette = CommandPalette::new();
        palette.filtered = vec![];
        palette.visible_height = 3;
        palette.scroll = 0;
        palette.cursor = 0;

        palette.cursor_down();
        assert_eq!(palette.cursor, 0);
    }

    #[test]
    fn test_is_empty_when_no_filtered() {
        let palette = CommandPalette::new();
        assert!(palette.is_empty());
    }

    #[test]
    fn test_visible_count_empty() {
        let palette = CommandPalette::new();
        assert_eq!(palette.visible_count(), 0);
    }

    #[test]
    fn test_selected_none_when_empty() {
        let palette = CommandPalette::new();
        assert!(palette.selected().is_none());
    }

    #[test]
    fn test_cursor_down_no_wrap() {
        let mut palette = CommandPalette::new();
        palette.cursor = 0;
        palette.visible_height = 5;
        palette.filtered = vec![];

        palette.cursor_down();
        assert_eq!(palette.cursor, 0);
    }

    #[test]
    fn test_cursor_up_no_wrap() {
        let mut palette = CommandPalette::new();
        palette.cursor = 0;
        palette.visible_height = 5;
        palette.filtered = vec![];

        palette.cursor_up();
        assert_eq!(palette.cursor, 0);
    }
}
