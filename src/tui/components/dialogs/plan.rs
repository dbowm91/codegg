use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::scroll::CenteredScroll;
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct PlanDialog {
    pub theme: Arc<Theme>,
    pub plan_id: String,
    pub plan_description: String,
    pub selected: bool,
    pub scroll: CenteredScroll,
}

impl PlanDialog {
    pub fn new(plan_id: String, plan_description: String) -> Self {
        Self {
            theme: Arc::new(Theme::default()),
            plan_id,
            plan_description,
            selected: false,
            scroll: CenteredScroll::new(),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(vec![
            Span::styled("Plan ", Style::default().fg(self.theme.warning)),
            Span::styled("Confirmation", Style::default().fg(self.theme.foreground)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Plan Request ")
                .border_style(Style::default().fg(self.theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(self.theme.foreground),
                ))
            })
            .collect();

        let description = Paragraph::new(description_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border)),
        );
        frame.render_widget(description, chunks[1]);

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(self.theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(self.theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(self.theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(self.theme.foreground),
                ))
            },
        ];

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select, Enter confirm) ")
                .border_style(Style::default().fg(self.theme.border)),
        );
        frame.render_widget(options_para, chunks[2]);
    }
}

impl ratatui::widgets::Widget for &PlanDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        if area.height < 3 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .title(" Plan Request ");

        let inner = block.inner(area);
        block.render(area, buf);

        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(self.theme.foreground),
                ))
            })
            .collect();

        let description = Paragraph::new(description_lines);
        ratatui::widgets::Widget::render(description, inner, buf);

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(self.theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(self.theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(self.theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(self.theme.foreground),
                ))
            },
        ];

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select, Enter confirm) ")
                .border_style(Style::default().fg(self.theme.border)),
        );

        let opts_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y.saturating_sub(1),
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };
        ratatui::widgets::Widget::render(options_para, opts_area, buf);
    }
}

impl Component for PlanDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.selected = false;
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.selected = true;
                None
            }
            crossterm::event::KeyCode::Enter => Some(TuiMsg::CloseDialog),
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
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(6),
            Constraint::Length(5),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(vec![
            Span::styled("Plan ", Style::default().fg(theme.warning)),
            Span::styled("Confirmation", Style::default().fg(theme.foreground)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Plan Request ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let description_lines: Vec<Line> = self
            .plan_description
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.foreground))))
            .collect();

        let description = Paragraph::new(description_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(description, chunks[1]);

        let options: Vec<Line> = vec![
            if !self.selected {
                Line::from(Span::styled(
                    "> Cancel",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Cancel",
                    Style::default().fg(theme.foreground),
                ))
            },
            if self.selected {
                Line::from(Span::styled(
                    "> Confirm",
                    Style::default()
                        .fg(ratatui::style::Color::White)
                        .bg(theme.primary),
                ))
            } else {
                Line::from(Span::styled(
                    "  Confirm",
                    Style::default().fg(theme.foreground),
                ))
            },
        ];

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select, Enter confirm) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(options_para, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Plan
    }
}
