use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use std::sync::Arc;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::section::Section;
use crate::session::Session;
use crate::tui::app::TuiMsg;
use crate::tui::components::hint_line::HintLine;

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

/// The busiest state's line count, so the box does not resize as the state
/// moves from input to preview to result while it is open.
const IMPORT_BODY_ROWS: u16 = 10;

impl Widget for &ImportDialog {
    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lines: Vec<Line> = Vec::new();

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
                        HintLine::from_items(&[("Esc", "close"), ("Enter", "preview/import")])
                            .render(0),
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
                        HintLine::from_items(&[("Esc", "close"), ("Enter", "import")]).render(0),
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
                    HintLine::from_items(&[("Esc", "close"), ("Enter", "continue")]).render(0),
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

        let inner = Section::new("Import Session", &self.theme)
            .focused(true)
            .draw(area, buf);
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(inner, buf);
    }
}

impl Component for ImportDialog {
    fn preferred_area(&self, available: Rect) -> Rect {
        crate::tui::components::dialog_size::preferred_rows(
            available,
            Section::owned_height(IMPORT_BODY_ROWS),
        )
    }

    /// `Char`/`Backspace` edit the input only in [`ImportState::Input`], the one
    /// state that shows it. Neither bounds the input, because
    /// [`Component::handle_paste`] bounds nothing either — a limit on one path
    /// alone would refuse text the other accepts.
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Esc => Some(TuiMsg::CloseDialog),
            crossterm::event::KeyCode::Char(c) if self.state == ImportState::Input => {
                self.set_input(c);
                None
            }
            crossterm::event::KeyCode::Backspace if self.state == ImportState::Input => {
                self.backspace();
                None
            }
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
                        HintLine::from_items(&[("Esc", "close"), ("Enter", "preview/import")])
                            .render(0),
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
                        HintLine::from_items(&[("Esc", "close"), ("Enter", "import")]).render(0),
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
                    HintLine::from_items(&[("Esc", "close"), ("Enter", "continue")]).render(0),
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

        let inner = Section::new("Import Session", theme)
            .focused(true)
            .draw(area, frame.buffer_mut());
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .render(inner, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Import
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// One character at a time, which is the path a reader actually uses.
    fn typed(text: &str) -> ImportDialog {
        let mut dialog = ImportDialog::default();
        for c in text.chars() {
            assert_eq!(
                dialog.handle_key(key(KeyCode::Char(c))),
                None,
                "a typed character changes the box without reporting a message"
            );
        }
        dialog
    }

    #[test]
    fn typing_reaches_the_input_and_backspace_pops_it() {
        let mut dialog = typed("codegg://share/abc");
        assert_eq!(
            dialog.input, "codegg://share/abc",
            "the typed URL never reached the box's input"
        );

        assert_eq!(
            dialog.handle_key(key(KeyCode::Backspace)),
            None,
            "backspace edits the box without reporting a message"
        );
        assert_eq!(
            dialog.input, "codegg://share/ab",
            "backspace must pop the last typed character"
        );
    }

    #[test]
    fn typing_is_scoped_to_the_state_that_shows_the_input() {
        let mut dialog = typed("codegg://share/abc");
        dialog.set_preview(Session::default(), 3);
        let submitted = dialog.input.clone();

        dialog.handle_key(key(KeyCode::Char('z')));
        dialog.handle_key(key(KeyCode::Backspace));

        assert_eq!(
            dialog.input, submitted,
            "the preview state advertises Enter and Esc alone, so it must not edit \
             a source that has already been submitted"
        );
    }

    #[test]
    fn typing_and_pasting_reach_the_input_at_the_same_bound() {
        // Paste bounds nothing, so typing must bound nothing either: the parity
        // is the property, not a shared literal.
        let text = "x".repeat(4096);
        let typed_dialog = typed(&text);
        let mut pasted = ImportDialog::default();
        assert_eq!(
            pasted.handle_paste(text.clone()),
            None,
            "paste edits the box without reporting a message"
        );

        assert_eq!(
            typed_dialog.input.len(),
            text.len(),
            "typing truncated input that paste would have accepted whole"
        );
        assert_eq!(
            typed_dialog.input, pasted.input,
            "the two input paths disagree for the same text"
        );
    }

    #[test]
    fn enter_submits_a_preview_then_confirms_it() {
        let mut dialog = typed("abc123456789");
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::SubmitImportPreview),
            "Enter in the input state must ask for a preview"
        );

        dialog.set_preview(Session::default(), 3);
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::ConfirmImport),
            "Enter in the preview state must ask to import"
        );
    }
}
