use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum InfoType {
    Context,
    Cost,
    Usage,
    ShellShow,
    Stats,
    TaskList,
    WorktreeList,
    GoalShow,
    MemoryResults,
    DoctorReport,
    Agents,
}

#[derive(Clone)]
pub struct InfoDialog {
    info_type: InfoType,
    lines: Vec<String>,
    theme: Arc<Theme>,
    scroll: usize,
    custom_footer: Option<String>,
}

impl InfoDialog {
    pub fn new(theme: Arc<Theme>, info_type: InfoType, lines: Vec<String>) -> Self {
        Self {
            info_type,
            lines,
            theme,
            scroll: 0,
            custom_footer: None,
        }
    }

    pub fn set_content(&mut self, lines: Vec<String>) {
        self.lines = lines;
        self.scroll = 0;
    }

    pub fn set_info_type(&mut self, info_type: InfoType) {
        self.info_type = info_type;
        self.scroll = 0;
    }

    fn title(&self) -> &'static str {
        match self.info_type {
            InfoType::Context => " Context ",
            InfoType::Cost => " Cost ",
            InfoType::Usage => " Usage ",
            InfoType::ShellShow => " Shell Command ",
            InfoType::Stats => " TUI Stats ",
            InfoType::TaskList => " Tasks ",
            InfoType::WorktreeList => " Worktrees ",
            InfoType::GoalShow => " Goal ",
            InfoType::MemoryResults => " Memory ",
            InfoType::DoctorReport => " Doctor ",
            InfoType::Agents => " Agents ",
        }
    }

    fn dialog_type_for_info_type(&self) -> DialogType {
        match self.info_type {
            InfoType::Context => DialogType::Context,
            InfoType::Cost => DialogType::Cost,
            InfoType::Usage => DialogType::Usage,
            InfoType::ShellShow => DialogType::ShellShow,
            InfoType::Stats => DialogType::Stats,
            InfoType::TaskList => DialogType::TaskList,
            InfoType::WorktreeList => DialogType::WorktreeList,
            InfoType::GoalShow => DialogType::GoalShow,
            InfoType::MemoryResults => DialogType::MemoryResults,
            InfoType::DoctorReport => DialogType::DoctorReport,
            InfoType::Agents => DialogType::Agent,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_custom_footer(&mut self, footer: String) {
        self.custom_footer = Some(footer);
    }

    pub fn content_lines(&self) -> &[String] {
        &self.lines
    }
}

impl Component for InfoDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.scroll < self.lines.len().saturating_sub(1) {
                    self.scroll += 1;
                }
                None
            }
            crossterm::event::KeyCode::Enter => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let title_block = Block::default()
            .borders(Borders::ALL)
            .title(self.title())
            .border_style(Style::default().fg(theme.border));

        let visible_lines = (area.height as usize).saturating_sub(5);
        let total_lines = self.lines.len();
        let max_scroll = total_lines.saturating_sub(visible_lines);

        let start_idx = self.scroll.min(max_scroll);
        let end_idx = (start_idx + visible_lines).min(total_lines);

        let display_lines: Vec<Line> = self.lines[start_idx..end_idx]
            .iter()
            .map(|s| {
                Line::from(Span::styled(
                    s.as_str(),
                    Style::default().fg(theme.foreground),
                ))
            })
            .collect();

        let content_para = Paragraph::new(display_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

        let scroll_indicator = if total_lines > visible_lines {
            format!("Showing {}-{} of {}", start_idx + 1, end_idx, total_lines)
        } else {
            String::new()
        };

        let footer_text = if let Some(ref custom) = self.custom_footer {
            if scroll_indicator.is_empty() {
                custom.clone()
            } else {
                format!(" {}  |  {}", scroll_indicator, custom)
            }
        } else if scroll_indicator.is_empty() {
            " j/k scroll  |  Esc/Enter close ".to_string()
        } else {
            format!(" {}  |  j/k scroll  |  Esc/Enter close ", scroll_indicator)
        };

        let footer_block = Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(theme.secondary),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

        frame.render_widget(title_block, chunks[0]);
        frame.render_widget(content_para, chunks[1]);
        frame.render_widget(footer_block, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        self.dialog_type_for_info_type()
    }
}
