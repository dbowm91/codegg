use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use std::sync::Arc;

use crossterm::event::KeyEvent;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::permission::PermissionRequest;
use crate::tui::app::TuiMsg;

#[derive(Clone)]
pub struct PermissionDialog {
    pub request: PermissionRequest,
    pub selected_option: usize,
    pub theme: Arc<Theme>,
    pub confirm_persistent: bool,
    pub pending_persistent: Option<usize>,
}

impl PermissionDialog {
    pub fn new(request: PermissionRequest, theme: Arc<Theme>) -> Self {
        Self {
            request,
            selected_option: 0,
            theme,
            confirm_persistent: false,
            pending_persistent: None,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn options(&self) -> Vec<&str> {
        if self.confirm_persistent {
            vec!["⚠ Confirm & Persist", "← Cancel"]
        } else {
            vec!["Allow Once", "Always Allow", "Deny Once", "Always Deny"]
        }
    }

    pub fn selected_option(&self) -> usize {
        self.selected_option
    }

    pub fn cursor_down(&mut self) {
        self.selected_option = if self.confirm_persistent {
            1
        } else {
            (self.selected_option + 1).min(3)
        };
    }

    pub fn cursor_up(&mut self) {
        if self.selected_option > 0 {
            self.selected_option -= 1;
        }
    }

    fn tool_icon(&self) -> &str {
        match self.request.tool.as_str() {
            "bash" => ">$",
            "read" => "[R]",
            "edit" => "[E]",
            "write" => "[W]",
            "glob" => "[G]",
            "grep" => "[/]",
            "list" => "[L]",
            "task" => "[T]",
            "webfetch" => "[W]",
            "websearch" => "[S]",
            _ => "[?]",
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(5),
        ])
        .split(area);

        let tool_icon = self.tool_icon();
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", tool_icon),
                Style::default().fg(theme.warning),
            ),
            Span::styled(
                format!("Tool '{}' requires permission", self.request.tool),
                Style::default().fg(theme.foreground),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Permission Request ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let mut details = Vec::new();
        if let Some(ref path) = self.request.path {
            details.push(Line::from(vec![
                Span::styled("Path: ", Style::default().fg(theme.muted)),
                Span::styled(path, Style::default().fg(theme.foreground)),
            ]));
        }
        if let Some(ref args) = self.request.args {
            let args_str = if args.is_string() {
                args.as_str().unwrap_or("").to_string()
            } else {
                args.to_string()
            };
            if !args_str.is_empty() && args_str != "null" {
                let truncated = if args_str.chars().count() > 50 {
                    let count = args_str.chars().count() - 47;
                    format!(
                        "{}... (+{count} chars)",
                        args_str.chars().take(47).collect::<String>()
                    )
                } else {
                    args_str
                };
                details.push(Line::from(vec![
                    Span::styled("Args: ", Style::default().fg(theme.muted)),
                    Span::styled(truncated, Style::default().fg(theme.foreground)),
                ]));
            }
        }
        if details.is_empty() {
            details.push(Line::from(Span::styled(
                "No additional details",
                Style::default().fg(theme.muted),
            )));
        }

        let detail = Paragraph::new(details).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(detail, chunks[1]);

        let options: Vec<Line> = self
            .options()
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let prefix = if i == self.selected_option {
                    "> "
                } else {
                    "  "
                };
                let style = if self.confirm_persistent {
                    if i == 0 {
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.foreground)
                    }
                } else {
                    if i == self.selected_option {
                        Style::default()
                            .fg(ratatui::style::Color::White)
                            .bg(theme.primary)
                    } else {
                        Style::default().fg(theme.foreground)
                    }
                };
                Line::from(Span::styled(format!("{prefix}{}", opt), style))
            })
            .collect();

        let options_para = Paragraph::new(options).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Options (↑↓ select, Enter confirm) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(options_para, chunks[2]);
    }
}

impl Component for PermissionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.confirm_persistent {
                    self.confirm_persistent = false;
                    self.selected_option = 0;
                    None
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.cursor_up();
                None
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.cursor_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if self.confirm_persistent {
                    if self.selected_option == 0 {
                        self.confirm_persistent = false;
                        let choice = self.pending_persistent.unwrap_or(1);
                        self.pending_persistent = None;
                        self.selected_option = 0;
                        Some(TuiMsg::SubmitPermission {
                            choice_index: choice,
                        })
                    } else {
                        self.confirm_persistent = false;
                        self.pending_persistent = None;
                        self.selected_option = 0;
                        None
                    }
                } else {
                    let choice = self.selected_option;
                    if choice == 1 || choice == 3 {
                        self.confirm_persistent = true;
                        self.pending_persistent = Some(choice);
                        self.selected_option = 0;
                        None
                    } else {
                        Some(TuiMsg::SubmitPermission {
                            choice_index: choice,
                        })
                    }
                }
            }
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
        self.render(frame, area, theme);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Permission
    }
}
