use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use crate::session::Session;
use crate::tui::app::TuiMsg;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImportState {
    Input,
    Preview,
    Importing,
    Done,
    Error,
}

#[derive(Clone)]
pub struct ImportDialog {
    pub theme: Arc<Theme>,
    pub input: String,
    pub state: ImportState,
    pub preview_session: Option<Session>,
    pub preview_msg_count: usize,
    pub imported_session: Option<Session>,
    pub error: Option<String>,
    pub conflict_mode: bool,
    pub preview_loading: bool,
}

impl ImportDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            input: String::new(),
            state: ImportState::Input,
            preview_session: None,
            preview_msg_count: 0,
            imported_session: None,
            error: None,
            conflict_mode: false,
            preview_loading: false,
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.state = ImportState::Input;
        self.preview_session = None;
        self.preview_msg_count = 0;
        self.imported_session = None;
        self.error = None;
        self.conflict_mode = false;
    }

    pub fn set_input(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn parse_input(&self) -> Option<ImportSource> {
        let input = self.input.trim();
        if input.is_empty() {
            return None;
        }

        if input.starts_with("codegg://share/") {
            let id = input.trim_start_matches("codegg://share/").to_string();
            return Some(ImportSource::SessionId(id));
        }

        if (input.ends_with(".json") || input.contains('/') || input.contains('\\'))
            && std::path::Path::new(input).exists()
        {
            return Some(ImportSource::FilePath(input.to_string()));
        }

        if input.len() > 10 && !input.contains('/') {
            return Some(ImportSource::SessionId(input.to_string()));
        }

        None
    }

    pub fn set_preview_loading(&mut self, loading: bool) {
        self.preview_loading = loading;
    }

    pub fn set_preview(&mut self, session: Session, msg_count: usize) {
        self.preview_session = Some(session);
        self.preview_msg_count = msg_count;
        self.state = ImportState::Preview;
        self.error = None;
        self.preview_loading = false;
    }

    pub fn set_error(&mut self, err: String) {
        self.error = Some(err);
        self.state = ImportState::Error;
        self.preview_loading = false;
    }

    pub fn set_importing(&mut self) {
        self.state = ImportState::Importing;
    }

    pub fn set_done(&mut self, session: Session) {
        self.imported_session = Some(session);
        self.state = ImportState::Done;
    }

    pub fn imported_session(&self) -> Option<&Session> {
        self.imported_session.as_ref()
    }
}

impl Default for ImportDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    SessionId(String),
    FilePath(String),
}

impl Widget for &ImportDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Import Session ",
            Style::default()
                .fg(self.theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        match self.state {
            ImportState::Input => {
                lines.push(Line::from(vec![Span::styled(
                    "Paste a share URL, session ID, or file path:",
                    Style::default().fg(self.theme.muted),
                )]));
                lines.push(Line::from(""));

                let input_display = if self.input.is_empty() {
                    Span::styled(
                        "  (paste or type here)",
                        Style::default().fg(self.theme.muted),
                    )
                } else {
                    Span::raw(format!("  {}  ", self.input))
                };
                lines.push(Line::from(input_display));
                lines.push(Line::from(""));

                lines.push(Line::from(vec![Span::styled(
                    "Examples:",
                    Style::default().fg(self.theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  codegg://share/abc123",
                    Style::default().fg(self.theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  abc123 (session ID)",
                    Style::default().fg(self.theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  /path/to/export.json",
                    Style::default().fg(self.theme.muted),
                )]));
                lines.push(Line::from(""));

                if self.preview_loading {
                    lines.push(Line::from(Span::styled(
                        "  Previewing...",
                        Style::default()
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "Enter preview  Esc cancel",
                        Style::default().fg(self.theme.muted),
                    )));
                }
            }
            ImportState::Preview => {
                if let Some(ref session) = self.preview_session {
                    lines.push(Line::from(vec![Span::styled(
                        "Session to import:",
                        Style::default().fg(self.theme.muted),
                    )]));
                    lines.push(Line::from(""));

                    let ts = chrono::DateTime::from_timestamp_millis(session.time_updated)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    lines.push(Line::from(vec![
                        Span::styled("Title: ", Style::default().fg(self.theme.muted)),
                        Span::styled(&session.title, Style::default().fg(self.theme.foreground)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Messages: ", Style::default().fg(self.theme.muted)),
                        Span::styled(
                            format!("{}", self.preview_msg_count),
                            Style::default().fg(self.theme.foreground),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Updated: ", Style::default().fg(self.theme.muted)),
                        Span::styled(ts, Style::default().fg(self.theme.foreground)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Directory: ", Style::default().fg(self.theme.muted)),
                        Span::styled(
                            &session.directory,
                            Style::default().fg(self.theme.foreground),
                        ),
                    ]));
                    lines.push(Line::from(""));

                    lines.push(Line::from(Span::styled(
                        "Enter import  Esc cancel",
                        Style::default().fg(self.theme.muted),
                    )));
                }
            }
            ImportState::Importing => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Importing...  ",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
            }
            ImportState::Done => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Import successful!  ",
                    Style::default()
                        .fg(self.theme.success)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                if let Some(ref session) = self.imported_session {
                    lines.push(Line::from(vec![
                        Span::styled("Session: ", Style::default().fg(self.theme.muted)),
                        Span::styled(&session.title, Style::default().fg(self.theme.foreground)),
                    ]));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Enter continue  Esc close",
                    Style::default().fg(self.theme.muted),
                )));
            }
            ImportState::Error => {
                lines.push(Line::from(Span::styled(
                    " Error ",
                    Style::default()
                        .fg(self.theme.error)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                if let Some(ref err) = self.error {
                    lines.push(Line::from(Span::styled(
                        format!("  {}  ", err),
                        Style::default().fg(self.theme.error),
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Esc close",
                    Style::default().fg(self.theme.muted),
                )));
            }
        }

        let block = Block::default()
            .title(" Import Session ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border))
            .style(Style::default().bg(self.theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}

impl Component for ImportDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Enter => match self.state {
                ImportState::Input => Some(TuiMsg::SubmitImportPreview),
                ImportState::Preview => Some(TuiMsg::ConfirmImport),
                ImportState::Done => Some(TuiMsg::CloseDialog),
                ImportState::Error => Some(TuiMsg::CloseDialog),
                ImportState::Importing => None,
            },
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.input.push_str(&text);
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, theme: &Arc<Theme>) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Import Session ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        match self.state {
            ImportState::Input => {
                lines.push(Line::from(vec![Span::styled(
                    "Paste a share URL, session ID, or file path:",
                    Style::default().fg(theme.muted),
                )]));
                lines.push(Line::from(""));

                let input_display = if self.input.is_empty() {
                    Span::styled("  (paste or type here)", Style::default().fg(theme.muted))
                } else {
                    Span::raw(format!("  {}  ", self.input))
                };
                lines.push(Line::from(input_display));
                lines.push(Line::from(""));

                lines.push(Line::from(vec![Span::styled(
                    "Examples:",
                    Style::default().fg(theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  codegg://share/abc123",
                    Style::default().fg(theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  abc123 (session ID)",
                    Style::default().fg(theme.muted),
                )]));
                lines.push(Line::from(vec![Span::styled(
                    "  /path/to/export.json",
                    Style::default().fg(theme.muted),
                )]));
                lines.push(Line::from(""));

                if self.preview_loading {
                    lines.push(Line::from(Span::styled(
                        "  Previewing...",
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "Enter preview  Esc cancel",
                        Style::default().fg(theme.muted),
                    )));
                }
            }
            ImportState::Preview => {
                if let Some(ref session) = self.preview_session {
                    lines.push(Line::from(vec![Span::styled(
                        "Session to import:",
                        Style::default().fg(theme.muted),
                    )]));
                    lines.push(Line::from(""));

                    let ts = chrono::DateTime::from_timestamp_millis(session.time_updated)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    lines.push(Line::from(vec![
                        Span::styled("Title: ", Style::default().fg(theme.muted)),
                        Span::styled(&session.title, Style::default().fg(theme.foreground)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Messages: ", Style::default().fg(theme.muted)),
                        Span::styled(
                            format!("{}", self.preview_msg_count),
                            Style::default().fg(theme.foreground),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Updated: ", Style::default().fg(theme.muted)),
                        Span::styled(ts, Style::default().fg(theme.foreground)),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Directory: ", Style::default().fg(theme.muted)),
                        Span::styled(&session.directory, Style::default().fg(theme.foreground)),
                    ]));
                    lines.push(Line::from(""));

                    lines.push(Line::from(Span::styled(
                        "Enter import  Esc cancel",
                        Style::default().fg(theme.muted),
                    )));
                }
            }
            ImportState::Importing => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Importing...  ",
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
            }
            ImportState::Done => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Import successful!  ",
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                if let Some(ref session) = self.imported_session {
                    lines.push(Line::from(vec![
                        Span::styled("Session: ", Style::default().fg(theme.muted)),
                        Span::styled(&session.title, Style::default().fg(theme.foreground)),
                    ]));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Enter continue  Esc close",
                    Style::default().fg(theme.muted),
                )));
            }
            ImportState::Error => {
                lines.push(Line::from(Span::styled(
                    " Error ",
                    Style::default()
                        .fg(theme.error)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                if let Some(ref err) = self.error {
                    lines.push(Line::from(Span::styled(
                        format!("  {}  ", err),
                        Style::default().fg(theme.error),
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Esc close",
                    Style::default().fg(theme.muted),
                )));
            }
        }

        let block = Block::default()
            .title(" Import Session ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Import
    }
}
