use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::hint_line::HintLine;
use crate::tui::components::section::Section;
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

/// The content rows of each of the dialog's three boxes. `Section` adds the
/// border, so counting it here as well would shrink the box by two rows.
const TITLE_ROWS: u16 = 2;
const MESSAGE_ROWS: u16 = 2;
const OPTIONS_ROWS: u16 = 3;

impl Component for ConfirmDialog {
    fn preferred_area(&self, available: Rect) -> Rect {
        crate::tui::components::dialog_size::preferred_rows(
            available,
            Section::owned_height(TITLE_ROWS)
                + Section::owned_height(MESSAGE_ROWS)
                + Section::owned_height(OPTIONS_ROWS),
        )
    }

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
            Constraint::Length(Section::owned_height(TITLE_ROWS)),
            Constraint::Min(Section::owned_height(MESSAGE_ROWS)),
            Constraint::Length(Section::owned_height(OPTIONS_ROWS)),
        ])
        .split(area);

        let title_inner = Section::new("Confirm", theme)
            .focused(true)
            .draw(chunks[0], frame.buffer_mut());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                &self.title,
                Style::default().fg(theme.warning),
            ))),
            title_inner,
        );

        let message_inner = Section::plain(theme).draw(chunks[1], frame.buffer_mut());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                &self.message,
                Style::default().fg(theme.foreground),
            ))),
            message_inner,
        );

        let mut options: Vec<Line> = ["Yes", "No"]
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let prefix = if i == self.selected_option {
                    "> "
                } else {
                    "  "
                };
                let style = if i == self.selected_option {
                    // `accent_text`, not white: the option is drawn on the
                    // accent *fill*, and white-on-white is what this used to
                    // render under a light theme.
                    Style::default().fg(theme.accent_text).bg(theme.primary)
                } else {
                    Style::default().fg(theme.foreground)
                };
                Line::from(Span::styled(format!("{prefix}{opt}"), style))
            })
            .collect();

        // The keymap stays inside the box rather than on its bottom edge.
        // A border hint is painted in the section's title style — muted when
        // the section is unfocused — and this dialog paints no other muted
        // text, so moving the keymap onto the edge would add six
        // `muted`-on-background contrast pairs across the bundled themes for a
        // string that was previously foreground-coloured. The primitive's
        // hint arm is used where the surface already paints muted text.
        options.push(Line::from(Span::styled(
            HintLine::from_items(&[("Esc", "cancel"), ("↑↓", "select"), ("Enter", "confirm")])
                .render(0),
            Style::default().fg(theme.foreground),
        )));
        let options_inner = Section::plain(theme).draw(chunks[2], frame.buffer_mut());
        frame.render_widget(Paragraph::new(options), options_inner);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn both_options_and_the_keymap_fit_the_stated_preference_at_24_rows() {
        let theme = Arc::new(Theme::dark());
        let mut dialog = ConfirmDialog::new("Delete".to_string(), "Are you sure?".to_string());
        let screen = Rect::new(0, 0, 80, 24);
        let rect =
            crate::tui::components::dialog_size::clamp_area(dialog.preferred_area(screen), screen);
        let mut terminal =
            Terminal::new(TestBackend::new(screen.width, screen.height)).expect("test terminal");
        terminal
            .draw(|frame| dialog.render(frame, rect, &theme))
            .expect("draw");

        let mut drawn = String::new();
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                if let Some(cell) = terminal.backend().buffer().cell((x, y)) {
                    drawn.push_str(cell.symbol());
                }
            }
            drawn.push('\n');
        }
        for row in ["Yes", "No", "Enter confirm"] {
            assert!(
                drawn.contains(row),
                "a 24-row Confirm must show {row:?} without scrolling; it drew:\n{drawn}"
            );
        }
    }
}
