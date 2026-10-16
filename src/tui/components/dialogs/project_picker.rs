//! Project picker dialog rendering helpers (Multi-Project TUI milestone 2).
//!
//! The picker state (`ProjectPickerState`) lives on
//! `App::dialog_state.project_picker`. This module provides:
//!
//! * [`ProjectPickerDialog`] — a `Component` implementation suitable
//!   for the `FocusManager` stack while the picker is open. It owns
//!   no per-dialog mutable state; it only renders a header and routes
//!   key events as a generic `CloseDialog`.
//! * [`render_picker_body`] — a free function that the App calls from
//!   its main render path with the live picker state. This is where
//!   the bounded row list, query input, and footer are produced.
//! * [`picker_visible_rows`] / [`visible_window`] — bounded view
//!   helpers for the filtered list.
//! * [`picker_key_to_msg`] — key-to-`TuiMsg` mapping used by the App
//!   to drive the picker phase machine.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::sync::Arc;

use crate::tui::app::state::project_picker::{
    PickerPhase, ProjectPickerState, MAX_PICKER_VISIBLE_ROWS,
};
use crate::tui::app::state::project_tabs::ProjectTabId;
use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct ProjectPickerDialog {
    pub theme: Arc<Theme>,
}

impl ProjectPickerDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self { theme }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    /// Compute a centered rectangle for the picker dialog of bounded
    /// height. Falls back to the available area if it is too small.
    pub fn picker_area(area: Rect) -> Rect {
        let height = 18u16.min(area.height.saturating_sub(2));
        let width = 64u16.min(area.width.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        Rect::new(x, y, width, height)
    }
}

impl Component for ProjectPickerDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // The picker state is owned by `App`. We only need to forward
        // Esc to close; all other key handling for the picker is done
        // by the App via `picker_key_to_msg`.
        match key.code {
            KeyCode::Esc => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, theme: &Arc<Theme>) {
        // This component renders a placeholder when invoked directly;
        // the App calls `render_picker_body` with the live picker
        // state when `Dialog::ProjectPicker` is open.
        if area.height < 5 {
            return;
        }
        let dialog_area = Self::picker_area(area);
        frame.render_widget(Clear, dialog_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Project Picker ");
        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);
        let placeholder = Paragraph::new("Loading project catalog...")
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true });
        frame.render_widget(placeholder, inner);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::ProjectPicker
    }
}

/// Bounded row representation for the picker's filtered list.
#[derive(Debug, Clone)]
pub struct PickerRow {
    pub project_id: String,
    pub display_name: String,
    pub archived: bool,
    pub lifecycle: String,
}

/// Build the visible rows for the picker from the picker state and
/// the catalog. Returns at most `MAX_PICKER_VISIBLE_ROWS` rows with
/// the selected row's window centered.
pub fn picker_visible_rows(
    picker: &ProjectPickerState,
    filtered_indices: &[usize],
    entries: &[crate::protocol::dto::ProjectSummaryDto],
) -> Vec<PickerRow> {
    if filtered_indices.is_empty() || entries.is_empty() {
        return Vec::new();
    }
    let total = filtered_indices.len();
    let visible = MAX_PICKER_VISIBLE_ROWS.min(total);
    let center = picker.selected_row.min(total.saturating_sub(1));
    let half = visible / 2;
    let start = if center < half {
        0
    } else if center + half >= total {
        total.saturating_sub(visible)
    } else {
        center - half
    };
    filtered_indices[start..start + visible]
        .iter()
        .filter_map(|&i| entries.get(i))
        .map(|e| PickerRow {
            project_id: e.project_id.clone(),
            display_name: e.display_name.clone(),
            archived: e.archived_at.is_some(),
            lifecycle: e.lifecycle.clone(),
        })
        .collect()
}

/// Build the visible row window metadata: (start_index, end_index_exclusive).
pub fn visible_window(filtered_count: usize, selected_row: usize) -> (usize, usize) {
    if filtered_count == 0 {
        return (0, 0);
    }
    let visible = MAX_PICKER_VISIBLE_ROWS.min(filtered_count);
    let center = selected_row.min(filtered_count - 1);
    let half = visible / 2;
    let start = if center < half {
        0
    } else if center + half >= filtered_count {
        filtered_count.saturating_sub(visible)
    } else {
        center - half
    };
    (start, start + visible)
}

/// Render the body of the picker dialog inside `inner` for the
/// provided picker state. Exposed so `App::render` can call it
/// directly with the live picker state.
pub fn render_picker_body(
    frame: &mut ratatui::Frame,
    picker: &ProjectPickerState,
    filtered_indices: &[usize],
    entries: &[crate::protocol::dto::ProjectSummaryDto],
    capability_supported: bool,
    inner: Rect,
    theme: &Arc<Theme>,
) {
    let phase_label = match picker.phase {
        PickerPhase::Catalog => "Projects",
        PickerPhase::WorkspaceSelection => "Choose Workspace",
        PickerPhase::RegistrationInput => "Register Local Project",
        PickerPhase::RegistrationConfirm => "Confirm Registration",
        PickerPhase::DestroyLoading => "Prepare Project Destroy",
        PickerPhase::DestroyConfirm => "Confirm Project Destroy",
        PickerPhase::LifecycleChanging => "Update Project",
        PickerPhase::Destroying => "Destroy Project",
        PickerPhase::ProjectEditLoading => "Load Project for Editing",
        PickerPhase::ProjectEdit => "Edit Project",
        PickerPhase::ProjectUpdating => "Update Project",
        PickerPhase::Error => "Error",
    };
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(inner);
    let title = Paragraph::new(Line::from(Span::styled(
        phase_label,
        Style::default().fg(theme.primary),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(title, chunks[0]);

    let count = filtered_indices.len();
    let total = entries
        .iter()
        .filter(|entry| picker.show_archived || entry.archived_at.is_none())
        .count();
    let mut filter_spans = if picker.query.is_empty() {
        vec![Span::styled(
            "type to filter projects…",
            Style::default().fg(theme.muted),
        )]
    } else {
        vec![
            Span::styled("search ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("{}_", picker.query),
                Style::default().fg(theme.primary),
            ),
            Span::styled(
                format!("  {count}/{total}"),
                Style::default().fg(theme.muted),
            ),
        ]
    };
    let (mode_label, mode_color) = match picker.input_mode {
        crate::tui::input::InputMode::Normal => ("NOR", theme.muted),
        crate::tui::input::InputMode::Insert => ("INS", theme.primary),
    };
    filter_spans.push(Span::styled(
        format!("  {mode_label} "),
        Style::default()
            .fg(theme.background)
            .bg(mode_color)
            .add_modifier(ratatui::style::Modifier::BOLD),
    ));
    let filter = Paragraph::new(Line::from(filter_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if picker.query.is_empty() {
                theme.border
            } else {
                theme.primary
            })),
    );
    frame.render_widget(filter, chunks[1]);

    let mut lines: Vec<Line> = Vec::new();

    match picker.phase {
        PickerPhase::Catalog => {
            let rows = picker_visible_rows(picker, filtered_indices, entries);
            let (start, _) = visible_window(filtered_indices.len(), picker.selected_row);
            if rows.is_empty() {
                let msg = if !capability_supported {
                    "Project catalog unsupported by this daemon.\nUpgrade or use --standalone."
                } else if picker.query.is_empty() {
                    "No projects registered yet."
                } else {
                    "No matches."
                };
                lines.push(Line::from(Span::styled(
                    msg.to_string(),
                    Style::default().fg(theme.muted),
                )));
            } else {
                for (offset, row) in rows.iter().enumerate() {
                    let actual_idx = start + offset;
                    let prefix = if actual_idx == picker.selected_row {
                        "> "
                    } else {
                        "  "
                    };
                    let archived_marker = if row.archived { "[A] " } else { "" };
                    let line_text = format!(
                        "{}{}{}  ({})",
                        prefix, archived_marker, row.display_name, row.project_id
                    );
                    let style = if actual_idx == picker.selected_row {
                        Style::default().fg(theme.primary)
                    } else if row.archived {
                        Style::default().fg(theme.muted)
                    } else {
                        Style::default().fg(theme.foreground)
                    };
                    lines.push(Line::from(Span::styled(line_text, style)));
                }
            }
            if picker.show_archived {
                lines.push(Line::from(Span::styled(
                    "(showing archived)".to_string(),
                    Style::default().fg(theme.muted),
                )));
            }
        }
        PickerPhase::WorkspaceSelection => {
            if let Some(detail) = &picker.cached_detail {
                for (i, ws) in detail.workspaces.iter().enumerate() {
                    let prefix = if i == picker.selected_row { "> " } else { "  " };
                    let line_text = format!("{}{}", prefix, ws.display_name);
                    let style = if i == picker.selected_row {
                        Style::default().fg(theme.primary)
                    } else {
                        Style::default().fg(theme.foreground)
                    };
                    lines.push(Line::from(Span::styled(line_text, style)));
                }
            }
        }
        PickerPhase::RegistrationInput => {
            lines.push(Line::from(Span::raw("Path:".to_string())));
            lines.push(Line::from(Span::raw(format!(
                "  {}",
                picker.registration_input
            ))));
            lines.push(Line::from(Span::styled(
                "Local only — directory must already exist.".to_string(),
                Style::default().fg(theme.muted),
            )));
        }
        PickerPhase::RegistrationConfirm => {
            lines.push(Line::from(Span::raw(format!(
                "Display name: {}",
                picker.registration.display_name
            ))));
            if !picker.registration.description.is_empty() {
                lines.push(Line::from(Span::raw(format!(
                    "Description: {}",
                    picker.registration.description
                ))));
            }
            if !picker.registration.tags.is_empty() {
                lines.push(Line::from(Span::raw(format!(
                    "Tags: {}",
                    picker.registration.tags.join(", ")
                ))));
            }
        }
        PickerPhase::DestroyLoading => {
            lines.push(Line::from(Span::raw(
                "Loading project and current session count…",
            )));
        }
        PickerPhase::DestroyConfirm => {
            let confirmation = picker.destroy_confirmation.as_ref();
            let name = confirmation
                .map(|c| c.display_name.as_str())
                .unwrap_or("this project");
            let session_count = confirmation.map(|c| c.session_count).unwrap_or(0);
            lines.push(Line::from(Span::styled(
                format!("Permanently destroy {name}?"),
                Style::default().fg(theme.error),
            )));
            lines.push(Line::from(format!(
                "{} session{} will be permanently deleted.",
                session_count,
                if session_count == 1 { "" } else { "s" }
            )));
            lines.push(Line::from(Span::styled(
                "This cannot be undone.",
                Style::default().fg(theme.error),
            )));
        }
        PickerPhase::LifecycleChanging => {
            lines.push(Line::from(Span::raw("Updating project lifecycle…")));
        }
        PickerPhase::Destroying => {
            lines.push(Line::from(Span::raw("Deleting project and its sessions…")));
        }
        PickerPhase::ProjectEditLoading => {
            lines.push(Line::from(Span::raw("Loading project name and directory…")));
        }
        PickerPhase::ProjectEdit => {
            if let Some(edit) = &picker.project_edit {
                let name_mark = if edit.editing_root { "  " } else { "> " };
                let root_mark = if edit.editing_root { "> " } else { "  " };
                lines.push(Line::from(format!(
                    "{name_mark}Name: {}",
                    edit.display_name
                )));
                lines.push(Line::from(format!("{root_mark}Directory: {}", edit.root)));
                lines.push(Line::from(Span::styled(
                    "Rebind changes directory bindings only; it never moves files.",
                    Style::default().fg(theme.muted),
                )));
                if let Some(error) = picker.last_error.as_deref() {
                    lines.push(Line::from(Span::styled(
                        error,
                        Style::default().fg(theme.error),
                    )));
                }
            }
        }
        PickerPhase::ProjectUpdating => {
            lines.push(Line::from(Span::raw(
                "Renaming project and rebinding sessions…",
            )));
        }
        PickerPhase::Error => {
            let err = picker.last_error.as_deref().unwrap_or("Unknown error");
            lines.push(Line::from(Span::styled(
                err.to_string(),
                Style::default().fg(theme.error),
            )));
        }
    }

    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme.foreground))
        .wrap(Wrap { trim: true });
    frame.render_widget(
        paragraph.block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        ),
        chunks[2],
    );

    let footer_text = match picker.phase {
        PickerPhase::DestroyConfirm => "Enter/y permanently destroy  |  Esc/n cancel",
        PickerPhase::DestroyLoading => "Esc cancel",
        PickerPhase::LifecycleChanging | PickerPhase::Destroying => "Working…",
        PickerPhase::ProjectEditLoading | PickerPhase::ProjectUpdating => "Working…",
        PickerPhase::ProjectEdit => "Tab switch field  |  Enter save  |  Esc cancel",
        _ if picker.input_mode == crate::tui::input::InputMode::Normal => {
            "j/k move  |  type to filter  |  i Insert  |  Enter select  |  Tab archived  |  Esc"
        }
        _ => {
            "type to filter  |  Backspace erase  |  Esc Normal  |  Enter select  |  Ctrl+R register"
        }
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(theme.muted))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
    frame.render_widget(footer, chunks[3]);
}

/// Map a key event to a picker-driven `TuiMsg`. The actual commit
/// logic runs in `App::process_msg` because the picker state is held
/// by the App. This helper exists so the App has a single place to
/// translate keys for the picker.
pub fn picker_key_to_msg(key: KeyEvent) -> Option<TuiMsg> {
    match key.code {
        KeyCode::Esc => Some(TuiMsg::CloseDialog),
        _ => None,
    }
}

/// Compute the deterministic tab display labels for the tab strip.
/// Returns one entry per tab in display order. Thin wrapper around
/// `ProjectTabs::display_labels`.
pub fn tab_strip_labels(tabs: &crate::tui::app::state::ProjectTabs) -> Vec<(ProjectTabId, String)> {
    tabs.display_labels()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dto(id: &str, name: &str) -> crate::protocol::dto::ProjectSummaryDto {
        crate::protocol::dto::ProjectSummaryDto {
            project_id: id.to_string(),
            display_name: name.to_string(),
            lifecycle: "active".to_string(),
            description: None,
            tags: Vec::new(),
            time_last_opened_at: None,
            registration_source: "test".to_string(),
            archived_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn picker_area_is_centered() {
        let area = Rect::new(0, 0, 100, 30);
        let picker = ProjectPickerDialog::picker_area(area);
        assert!(picker.width <= 64);
        assert!(picker.height <= 18);
    }

    #[test]
    fn picker_area_clamps_to_available() {
        let area = Rect::new(0, 0, 20, 10);
        let picker = ProjectPickerDialog::picker_area(area);
        assert!(picker.width <= 20);
        assert!(picker.height <= 10);
    }

    #[test]
    fn visible_rows_caps_at_max() {
        let picker = ProjectPickerState::new(true, 0);
        let entries: Vec<_> = (0..50)
            .map(|i| make_dto(&format!("p{}", i), &format!("P{}", i)))
            .collect();
        let indices: Vec<usize> = (0..50).collect();
        let rows = picker_visible_rows(&picker, &indices, &entries);
        assert!(rows.len() <= MAX_PICKER_VISIBLE_ROWS);
    }

    #[test]
    fn visible_window_centers_on_selection() {
        let (start, end) = visible_window(20, 10);
        assert!(start <= 10);
        assert!(end <= 20);
        assert!(end - start <= MAX_PICKER_VISIBLE_ROWS);
    }

    #[test]
    fn visible_window_clamps_to_end() {
        let (start, end) = visible_window(5, 4);
        assert_eq!(start, 0);
        assert!(end <= 5);
    }

    #[test]
    fn visible_window_handles_empty() {
        let (start, end) = visible_window(0, 0);
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn picker_key_to_msg_esc_closes() {
        let key = KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE);
        assert_eq!(picker_key_to_msg(key), Some(TuiMsg::CloseDialog));
    }

    #[test]
    fn picker_render_shows_filter_count_mode_badge_and_footer() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut picker = ProjectPickerState::new(true, 0);
        picker.query = "beta".to_string();
        let entries = vec![make_dto("a", "Alpha"), make_dto("b", "Beta")];
        let filtered = picker.filtered_indices(&entries);
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                render_picker_body(
                    frame,
                    &picker,
                    &filtered,
                    &entries,
                    true,
                    frame.area(),
                    &theme,
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Projects"));
        assert!(text.contains("search beta_"));
        assert!(text.contains("1/2"));
        assert!(text.contains("NOR"));
        assert!(text.contains("j/k move"));
        assert!(text.contains("Beta"));

        picker.input_mode = crate::tui::input::InputMode::Insert;
        terminal
            .draw(|frame| {
                render_picker_body(
                    frame,
                    &picker,
                    &filtered,
                    &entries,
                    true,
                    frame.area(),
                    &theme,
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("INS"));
        assert!(text.contains("Esc Normal"));
    }

    #[test]
    fn destroy_confirmation_names_project_session_count_and_irreversibility() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut picker = ProjectPickerState::new(true, 0);
        picker.phase = PickerPhase::DestroyConfirm;
        picker.destroy_confirmation = Some(
            crate::tui::app::state::project_picker::ProjectDestroyConfirmation {
                display_name: "Archived Project".to_string(),
                session_count: 2,
            },
        );
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                render_picker_body(frame, &picker, &[], &[], true, frame.area(), &theme);
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Confirm Project Destroy"));
        assert!(text.contains("Permanently destroy Archived Project?"));
        assert!(text.contains("2 sessions will be permanently deleted."));
        assert!(text.contains("This cannot be undone."));
        assert!(text.contains("Enter/y permanently destroy"));
        assert!(text.contains("Esc/n cancel"));
    }

    #[test]
    fn project_edit_shows_name_root_and_binding_only_copy() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut picker = ProjectPickerState::new(true, 0);
        picker.phase = PickerPhase::ProjectEdit;
        picker.project_edit = Some(crate::tui::app::state::project_picker::ProjectEditDraft {
            display_name: "Example".to_string(),
            root: "/work/example".to_string(),
            expected_updated_at: 42,
            editing_root: false,
        });
        let theme = Arc::new(Theme::dark());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| render_picker_body(frame, &picker, &[], &[], true, frame.area(), &theme))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Edit Project"));
        assert!(text.contains("Name: Example"));
        assert!(text.contains("Directory: /work/example"));
        assert!(text.contains("never moves files"));
        assert!(text.contains("Enter save"));
    }
}
