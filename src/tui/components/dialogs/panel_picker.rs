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

use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Frame;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::dialogs::panel_list::PanelListState;
use crate::tui::input::InputMode;
use crate::tui::panels::SidePanelId;
use crate::tui::theme::Theme;

/// Who owns this render's chrome.
///
/// The two paths differ only in that. [`Chrome::Owned`] is the modal box, which
/// draws its own four bordered chunks; [`Chrome::Hosted`] is a column in a side
/// panel, where the host has already painted a border, a title and the `●` focus
/// marker into the same rect.
///
/// The footer below is the one deliberate exception: this box keeps its
/// keymap footer in **both**, because it is only ever rendered modally and that
/// footer is the only place these keys are written down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chrome {
    Owned,
    Hosted,
}

/// The Normal-mode verbs this box implements, for the panel host to reuse.
///
/// `handle_key` delegates to [`PanelListState::handle_shared_key`], which is the
/// implementation, and this is the advertisement. `↑/↓` and `Esc` are
/// deliberately absent even though that function answers them:
/// `SidePanelHost::handle_key` claims the arrows as panel directions and `Esc`/
/// `q` as universal exits before the widget is consulted.
const HINT: &str = "j/k move · ^d/^u page · i/a filter · enter send";

/// The Insert-mode verbs.
///
/// In Insert the query is the buffer, so the keys that mean anything are the
/// ones that edit it: text, and `⌫` to pop a character. Typing is not a key and
/// so cannot be named in a keymap line — the `INS` badge on the filter row and
/// the surviving footer are what say it — while `Esc` back to Normal is absent
/// for the host-answered reason above. `Enter` still commits in both modes,
/// because `handle_key` intercepts it before delegating.
const INSERT_HINT: &str = "⌫ pop · enter send";

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

    /// One body for both render paths.
    ///
    /// `chunks[2].height` is the list's viewport and it is what
    /// `self.list.scroll(..)` is clamped against, which is why the chunk split
    /// is computed here, once, from the rect this routine was handed — a second
    /// copy of this layout computing its own viewport is how a docked box ends
    /// up with an unreachable tail and no error anywhere.
    ///
    /// The subject line, the filter row, the rows and the footer are the same
    /// content in both modes; only the borders and the row budget differ. The
    /// hosted budget spends one row per band instead of three, because two of
    /// each of those three is a border this widget no longer draws.
    fn render_body(&self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>, chrome: Chrome) {
        let buf = frame.buffer_mut();
        // The same budgeted chunks as the manager, and for the same reason: a
        // fifth `Length(3)` chunk pushes the footer — the only place these
        // keys are written down — off the bottom of a short terminal.
        let chunks = match chrome {
            Chrome::Owned => Layout::vertical([
                Constraint::Length(3), // title
                Constraint::Length(3), // filter
                Constraint::Min(1),    // list
                Constraint::Length(3), // footer
            ])
            .split(area),
            Chrome::Hosted => Layout::vertical([
                Constraint::Length(1), // subject
                Constraint::Length(1), // filter
                Constraint::Min(1),    // list
                Constraint::Length(1), // footer
            ])
            .split(area),
        };

        let subject = format!(" Send {} to panel ", self.subject);
        match chrome {
            Chrome::Owned => Widget::render(
                Block::default()
                    .borders(Borders::ALL)
                    .title(subject)
                    .title_style(
                        Style::default()
                            .fg(theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )
                    .border_style(Style::default().fg(theme.primary))
                    .style(Style::default().bg(theme.background)),
                chunks[0],
                buf,
            ),
            // The host's own title names the panel, not what is being routed, so
            // the subject stays — as a plain line, with no border to overwrite
            // anything and no `●` to cover.
            Chrome::Hosted => Widget::render(
                Paragraph::new(Line::from(Span::styled(
                    subject,
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                ))),
                chunks[0],
                buf,
            ),
        }

        let filtering = self.list.filtering();
        let filter_row = Paragraph::new(Line::from(self.list.filter_row(theme, "panels")));
        match chrome {
            Chrome::Owned => Widget::render(
                filter_row.block(Block::default().borders(Borders::ALL).border_style(
                    Style::default().fg(if filtering {
                        theme.primary
                    } else {
                        theme.border
                    }),
                )),
                chunks[1],
                buf,
            ),
            // The filter's primary-vs-border recolour is the modal telling the
            // user which band has the keyboard; a column is told by the host's
            // focus marker instead, so the host's border is the only rule here.
            Chrome::Hosted => Widget::render(filter_row, chunks[1], buf),
        }

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
        let list = Paragraph::new(lines).scroll((self.list.scroll(chunks[2].height), 0));
        match chrome {
            Chrome::Owned => Widget::render(
                list.block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
                chunks[2],
                buf,
            ),
            Chrome::Hosted => Widget::render(list, chunks[2], buf),
        }

        let footer = match (self.list.mode(), filtering) {
            (InputMode::Insert, true) => "Enter send  Esc normal  Bksp edit",
            (InputMode::Insert, false) => "type to filter  Enter send  Esc normal",
            (InputMode::Normal, true) => "↑↓ move  Enter send  Esc clear",
            (InputMode::Normal, false) => "↑↓ move  Enter send  i filter",
        };
        // The footer survives `Chrome::Hosted` — the one box in this milestone
        // where retiring it would be wrong. Every other widget here has a
        // `keymap_hint` the panel host prints, so its in-box footer was a
        // duplicate; this one has no `dock_target` arm in `App::dock_target`, so
        // the host never reads the hint for it and the footer is the *only*
        // place these keys are written down. Dropping it would strip the modal
        // — which is where this box actually lives — of its entire keymap to
        // satisfy a panel it can never be adopted into.
        let footer = Paragraph::new(Line::from(Span::styled(
            format!(" {footer}"),
            Style::default().fg(theme.muted),
        )));
        match chrome {
            Chrome::Owned => Widget::render(
                footer.block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme.border)),
                ),
                chunks[3],
                buf,
            ),
            // The text stays; only the border goes, or it would draw a fourth
            // box inside the host's.
            Chrome::Hosted => Widget::render(footer, chunks[3], buf),
        }
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

    /// The verbs this box owns in the mode it is currently in.
    ///
    /// Nothing reads this today: `docks_as_viewer` is `true` but
    /// `App::dock_target` has no `DialogType::PanelPicker` arm, so the host
    /// resolves the docking gesture, gets `None`, and refuses it —
    /// `Shift+←` never reaches `handle_key`. No arm is added here, because
    /// there is no panel a picker should be adopted into. It is implemented
    /// anyway so that the seam is truthful the moment a mapping does exist, and
    /// so the footer below and this string cannot drift apart: both are written
    /// from the same `handle_shared_key` arms.
    fn keymap_hint(&self) -> Option<String> {
        Some(
            match self.list.mode() {
                InputMode::Normal => HINT,
                InputMode::Insert => INSERT_HINT,
            }
            .to_string(),
        )
    }

    /// The picker without its four borders, on a seam it does not currently
    /// reach.
    ///
    /// `docks_as_viewer` says the widget could be adopted, `App::dock_target`
    /// says there is nowhere to adopt it *to*, and this is the path that
    /// resolves the contradiction: if a mapping is ever added, this box is
    /// already chrome-free, so adding one cannot be the change that also has to
    /// fix a panel that overwrites the host's border, title and `●` focus
    /// marker. The footer stays regardless — see `render_body`.
    fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let inner = area.inner(Margin::new(1, 1));
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        self.render_body(frame, inner, theme, Chrome::Hosted);
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.render_body(frame, area, theme, Chrome::Owned);
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
