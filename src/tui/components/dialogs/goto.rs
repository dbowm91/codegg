use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct GotoDialog {
    pub theme: Arc<Theme>,
    pub input: String,
    pub total_messages: usize,
    pub target_index: Option<usize>,
    pub error: Option<String>,
}

impl GotoDialog {
    pub fn new(total_messages: usize) -> Self {
        Self {
            theme: Arc::new(Theme::default()),
            input: String::new(),
            total_messages,
            target_index: None,
            error: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_total(&mut self, total: usize) {
        self.total_messages = total;
    }

    pub fn append_char(&mut self, c: char) {
        self.input.push(c);
        self.parse_input();
    }

    pub fn backspace(&mut self) {
        self.input.pop();
        self.parse_input();
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.target_index = None;
        self.error = None;
    }

    fn parse_input(&mut self) {
        self.error = None;
        self.target_index = None;

        if self.input.is_empty() {
            return;
        }

        let input = self.input.trim();

        if input.starts_with('+') || input.starts_with('-') {
            let offset: isize = match input.parse() {
                Ok(n) => n,
                Err(_) => {
                    self.error = Some("Invalid offset".to_string());
                    return;
                }
            };
            self.target_index = Some(offset.max(0) as usize);
            return;
        }

        match input.parse::<usize>() {
            Ok(idx) => {
                if idx == 0 {
                    self.error = Some("Index starts at 1".to_string());
                    return;
                }
                if idx > self.total_messages {
                    self.error = Some(format!("Max: {}", self.total_messages));
                    return;
                }
                self.target_index = Some(idx - 1);
            }
            Err(_) => {
                self.error = Some("Enter a number".to_string());
            }
        }
    }

    pub fn is_valid(&self) -> bool {
        self.target_index.is_some() && self.error.is_none()
    }

    pub fn get_index(&self) -> Option<usize> {
        self.target_index
    }
}

impl ratatui::widgets::Widget for &GotoDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        if area.height < 3 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Go to Message ");

        let inner = block.inner(area);
        block.render(area, buf);

        let input_display = if self.input.is_empty() {
            Span::raw("Enter message number...")
        } else {
            Span::raw(&self.input)
        };

        let status = if let Some(ref err) = self.error {
            Span::styled(err, Style::default().fg(self.theme.error))
        } else if let Some(idx) = self.target_index {
            Span::raw(format!(" → message {}", idx + 1))
        } else {
            Span::raw("")
        };

        let help = Span::raw(" (number, +n/-n relative, Esc cancel, Enter go)");

        let lines = vec![
            Line::from(input_display),
            Line::from(status),
            Line::from(help),
        ];

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(self.theme.foreground))
            .wrap(ratatui::widgets::Wrap { trim: true });

        paragraph.render(inner, buf);
    }
}

impl Component for GotoDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Enter => {
                self.get_index().map(|index| TuiMsg::GotoMessage { index })
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.input.push_str(&text);
        self.parse_input();
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        if area.height < 3 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Go to Message ");

        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let input_display = if self.input.is_empty() {
            Span::raw("Enter message number...")
        } else {
            Span::raw(&self.input)
        };

        let status = if let Some(ref err) = self.error {
            Span::styled(err, Style::default().fg(theme.error))
        } else if let Some(idx) = self.target_index {
            Span::raw(format!(" → message {}", idx + 1))
        } else {
            Span::raw("")
        };

        let help = Span::raw(" (number, +n/-n relative, Esc cancel, Enter go)");

        let lines = vec![
            Line::from(input_display),
            Line::from(status),
            Line::from(help),
        ];

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(theme.foreground))
            .wrap(ratatui::widgets::Wrap { trim: true });

        paragraph.render(inner, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Goto
    }
}
