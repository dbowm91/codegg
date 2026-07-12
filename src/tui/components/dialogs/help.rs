use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::input::{build_help_lines, HelpMode, InputMode};
use crate::tui::theme::Theme;

pub struct HelpDialog {
    help_lines: Vec<String>,
    theme: Arc<Theme>,
    scroll: usize,
}

impl Clone for HelpDialog {
    fn clone(&self) -> Self {
        Self {
            help_lines: self.help_lines.clone(),
            theme: Arc::clone(&self.theme),
            scroll: self.scroll,
        }
    }
}

impl HelpDialog {
    pub fn new(theme: Arc<Theme>, help_lines: Vec<String>) -> Self {
        Self {
            help_lines,
            theme,
            scroll: 0,
        }
    }

    /// Create a help dialog with mode-aware content.
    pub fn new_with_mode(theme: Arc<Theme>, vim_mode: bool, input_mode: InputMode) -> Self {
        let active_mode = match input_mode {
            InputMode::Insert => HelpMode::Insert,
            InputMode::Normal => HelpMode::Normal,
        };
        let help_lines = build_help_lines(vim_mode, active_mode);
        Self {
            help_lines,
            theme,
            scroll: 0,
        }
    }

    pub fn select_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    pub fn select_down(&mut self) {
        if self.scroll < self.help_lines.len().saturating_sub(1) {
            self.scroll += 1;
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }
}

impl Component for HelpDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
            }
            crossterm::event::KeyCode::Esc => {
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
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "Help",
            Style::default().fg(theme.primary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(title, chunks[0]);

        let content_height = chunks[1].height as usize;
        let visible_height = content_height.saturating_sub(2);
        let max_scroll = self.help_lines.len().saturating_sub(visible_height);
        // Key handling does not know the dialog's eventual size. Clamp here
        // so repeated Down presses cannot scroll into a blank content pane.
        self.scroll = self.scroll.min(max_scroll);
        let visible_lines: Vec<Line> = self
            .help_lines
            .iter()
            .skip(self.scroll)
            .take(visible_height)
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let content = Paragraph::new(visible_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(content, chunks[1]);

        let mut scrollbar_state = ScrollbarState::new(self.help_lines.len()).position(self.scroll);
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(theme.primary)),
            chunks[1],
            &mut scrollbar_state,
        );

        let footer = Paragraph::new(Line::from(Span::styled(
            "j/k/↑/↓ scroll  |  Esc close",
            Style::default().fg(theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(footer, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Help
    }
}
