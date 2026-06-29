use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub confirmed: Option<bool>,
    pub selected_option: usize,
}

impl ConfirmDialog {
    pub fn new(title: String, message: String) -> Self {
        Self {
            title,
            message,
            confirmed: None,
            selected_option: 0,
        }
    }

    pub fn cursor_down(&mut self) {
        self.selected_option = (self.selected_option + 1).min(1);
    }

    pub fn cursor_up(&mut self) {
        if self.selected_option > 0 {
            self.selected_option -= 1;
        }
    }

    pub fn confirm(&mut self) {
        self.confirmed = Some(self.selected_option == 0);
    }
}

impl Component for ConfirmDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.cursor_up();
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.cursor_down();
            }
            crossterm::event::KeyCode::Enter => {
                self.confirm();
                return Some(TuiMsg::ConfirmResult(self.confirmed));
            }
            crossterm::event::KeyCode::Esc => {
                self.confirmed = Some(false);
                return Some(TuiMsg::ConfirmResult(self.confirmed));
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => {
                self.confirmed = Some(false);
                Some(TuiMsg::CloseDialog)
            }
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(5),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            &self.title,
            Style::default().fg(theme.warning),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let message = Paragraph::new(Line::from(Span::styled(
            &self.message,
            Style::default().fg(theme.foreground),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(message, chunks[1]);

        let options: Vec<Line> = ["Yes", "No"]
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let prefix = if i == self.selected_option {
                    "> "
                } else {
                    "  "
                };
                let style = if i == self.selected_option {
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                Line::from(Span::styled(format!("{prefix}{opt}"), style))
            })
            .collect();

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ↑↓ select  |  Enter confirm  |  Esc cancel ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(options_para, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Confirm
    }

    fn focusable_count(&self) -> usize {
        2
    }

    fn focused_index(&self) -> usize {
        self.selected_option
    }

    fn set_focused(&mut self, idx: usize) {
        self.selected_option = idx.min(1);
    }
}

impl Default for ConfirmDialog {
    fn default() -> Self {
        Self::new("Confirm".to_string(), "Are you sure?".to_string())
    }
}
