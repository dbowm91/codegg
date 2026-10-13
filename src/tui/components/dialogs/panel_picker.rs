//! "Which panel should this go in?" — the picker `/agents` opens.
//!
//! `/agents` used to jump straight into the side column, which meant the
//! command had no box of its own and no way to ask where its content belonged.
//! This is that box: pick a panel, and the agents list is loaded into it.
//!
//! It deliberately shares [`PanelListState`] with the panel manager rather than
//! copying it. Two boxes that both list panels must agree on what `j` does,
//! where the filter goes, and what `Enter` commits — and the cheapest way to
//! guarantee that is to have one implementation.
//!
//! Unlike the manager, this box makes **no** placement or hotkey change. It
//! answers one question ("which panel?") and delegates the answer to
//! `TuiMsg::SelectPanel`, which is the same message the manager emits. There
//! is no second route into panel selection, so a box and its panel cannot drift.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::dialogs::panel_list::PanelListState;
use crate::tui::panels::SidePanelId;
use crate::tui::theme::Theme;

pub struct PanelPickerDialog {
    theme: Arc<Theme>,
    list: PanelListState,
    /// Names what is being routed, e.g. `"agents"`. The box is generic; this
    /// is what stops it reading as a second panel manager.
    subject: &'static str,
}

impl Clone for PanelPickerDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            list: self.list.clone(),
            subject: self.subject,
        }
    }
}

impl std::fmt::Debug for PanelPickerDialog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PanelPickerDialog(<filtered panel list>)")
    }
}

impl PanelPickerDialog {
    /// Open on `active`, so `Enter` with no movement commits what is already
    /// loaded rather than silently jumping to the top of the registry.
    pub fn new(theme: Arc<Theme>, active: SidePanelId, subject: &'static str) -> Self {
        Self {
            theme,
            list: PanelListState::new(active),
            subject,
        }
    }

    /// The panel `Enter` would commit.
    pub fn selected(&self) -> Option<SidePanelId> {
        self.list.selected()
    }

    /// Which mode the filter is in, for the parity tests.
    pub fn input_mode(&self) -> crate::tui::input::InputMode {
        self.list.mode()
    }
}

impl Component for PanelPickerDialog {
    fn dialog_type(&self) -> DialogType {
        DialogType::PanelPicker
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        use crossterm::event::KeyCode;

        // Routing is the primary act in both modes, exactly as loading is in
        // the manager: a user who filtered to `chat` and pressed Enter wants
        // chat, not to be handed back to a box they must leave themselves.
        if key.code == KeyCode::Enter {
            let panel = self.selected()?;
            return Some(TuiMsg::SelectPanel { panel });
        }
        self.list.handle_shared_key(key)
    }

    fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
        // Purely local state: every change is the user's own key.
        None
    }

    /// This box owns no text buffer, so the directional is free to mean "dock
    /// me" — the same contract every other viewer box honours, and why it is
    /// left at the trait default rather than re-decided here.
    fn docks_as_viewer(&self) -> bool {
        true
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let buf = frame.buffer_mut();
        // The same budgeted chunks as the manager, and for the same reason: a
        // fifth `Length(3)` chunk pushes the footer — the only place these
        // keys are written down — off the bottom of a short terminal.
        let chunks = Layout::vertical([
            Constraint::Length(3), // title
            Constraint::Length(3), // filter
            Constraint::Min(1),    // list
            Constraint::Length(3), // footer
        ])
        .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Send {} to panel ", self.subject))
            .title_style(
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background));
        Widget::render(block, chunks[0], buf);

        let filtering = self.list.filtering();
        Widget::render(
            Paragraph::new(Line::from(self.list.filter_row(theme, "panels"))).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if filtering {
                        theme.primary
                    } else {
                        theme.border
                    })),
            ),
            chunks[1],
            buf,
        );

        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, panel) in SidePanelId::ALL.iter().copied().enumerate() {
            let visible = self.list.matches(panel);
            let marker = if index == self.list.cursor() {
                "▸"
            } else {
                " "
            };
            let style = self.list.row_style(theme, index, visible);
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(format!("{:<10}", panel.label()), style),
                Span::styled(
                    format!("{}  ", panel.hint()),
                    Style::default().fg(theme.muted),
                ),
            ]));
        }
        Widget::render(
            Paragraph::new(lines)
                .scroll((self.list.scroll(chunks[2].height), 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
            chunks[2],
            buf,
        );

        let footer = match (self.list.mode(), filtering) {
            (crate::tui::input::InputMode::Insert, true) => "Enter send  Esc normal  Bksp edit",
            (crate::tui::input::InputMode::Insert, false) => {
                "type to filter  Enter send  Esc normal"
            }
            (crate::tui::input::InputMode::Normal, true) => "↑↓ move  Enter send  Esc clear",
            (crate::tui::input::InputMode::Normal, false) => "↑↓ move  Enter send  i filter",
        };
        Widget::render(
            Paragraph::new(Line::from(Span::styled(
                format!(" {footer}"),
                Style::default().fg(theme.muted),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            ),
            chunks[3],
            buf,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn picker() -> PanelPickerDialog {
        PanelPickerDialog::new(Arc::new(Theme::default()), SidePanelId::Agents, "agents")
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn enter_commits_the_highlighted_panel() {
        let mut dialog = picker();
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::SelectPanel {
                panel: SidePanelId::Agents
            })
        );
    }

    #[test]
    fn filtering_commits_a_real_panel_without_renumbering() {
        let mut dialog = picker();
        // Reach the filter the way a user does: `i`, then type.
        dialog.handle_key(key(KeyCode::Char('i')));
        for ch in "chat".chars() {
            dialog.handle_key(key(KeyCode::Char(ch)));
        }
        // Filtering narrows what is visible without moving the highlight —
        // the same contract the manager is pinned to by
        // `typing_narrows_what_is_visible_without_moving_the_highlight`.
        // Moving it here would make the two boxes disagree about `j`.
        assert_eq!(
            dialog.selected(),
            Some(SidePanelId::Agents),
            "the cursor addresses the registry, not the filtered view"
        );
        assert!(
            dialog.list.matches(SidePanelId::Chat),
            "precondition: the filter really does match chat"
        );
        assert_eq!(
            dialog.handle_key(key(KeyCode::Enter)),
            Some(TuiMsg::SelectPanel {
                panel: SidePanelId::Agents
            })
        );
    }

    #[test]
    fn filtering_narrows_visibility_without_moving_the_highlight() {
        let mut dialog = picker();
        dialog.handle_key(key(KeyCode::Char('i')));
        for ch in "chat".chars() {
            dialog.handle_key(key(KeyCode::Char(ch)));
        }
        // Deliberate, shared with the manager and pinned there by
        // `typing_narrows_what_is_visible_without_moving_the_highlight`: the
        // cursor addresses `SidePanelId::ALL`, not the filtered view, so
        // typing never re-aims it. Re-aiming here would make the two boxes
        // disagree about what `j` does.
        assert!(
            dialog.list.matches(SidePanelId::Chat),
            "precondition: the filter really does match chat"
        );
        assert_eq!(
            dialog.selected(),
            Some(SidePanelId::Agents),
            "filtering must not move the highlight off the panel it started on"
        );
    }

    #[test]
    fn it_opens_in_normal_so_j_moves_rather_than_types() {
        let dialog = picker();
        assert_eq!(
            dialog.input_mode(),
            crate::tui::input::InputMode::Normal,
            "opening in Insert makes j/k filter text, which reads as a dead box"
        );
    }

    #[test]
    fn esc_from_normal_with_no_filter_closes() {
        let mut dialog = picker();
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::CloseDialog)
        );
    }

    #[test]
    fn esc_clears_the_filter_before_it_closes() {
        let mut dialog = picker();
        dialog.handle_key(key(KeyCode::Char('i')));
        dialog.handle_key(key(KeyCode::Char('a')));
        // First Esc: back to Normal with the query intact.
        assert_eq!(dialog.handle_key(key(KeyCode::Esc)), None);
        assert_eq!(dialog.list.query(), "a");
        // Second Esc: clear.
        assert_eq!(dialog.handle_key(key(KeyCode::Esc)), None);
        assert_eq!(dialog.list.query(), "");
        // Third Esc: nothing left to clear, so it closes.
        assert_eq!(
            dialog.handle_key(key(KeyCode::Esc)),
            Some(TuiMsg::CloseDialog)
        );
    }
}
