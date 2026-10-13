//! Shared filterable-panel-list mechanics.
//!
//! Two boxes now ask the same question in different words — "which panel
//! should the side column hold?" (`/panels`) and "which panel should this
//! content be sent to?" (`/agents`) — and both need the same seven things: a
//! bounded query, a cursor that survives filtering, a scroll that keeps the
//! highlight on screen, a NOR/INS mode, and the row/badge rendering that
//! explains all three.
//!
//! Those live here rather than in either dialog. The alternative was a third
//! copy of `move_cursor`/`scroll`/`matches`/`mode_badge`, which is how a box
//! and its panel end up disagreeing about what `j` does.
//!
//! ## Why the cursor is not a filtered index
//!
//! The cursor is an index into [`SidePanelId::ALL`], never into the filtered
//! view. Typing narrows what is *visible* without renumbering what `Enter`
//! acts on: a user who filtered to `chat` and pressed `Enter` wants chat, not
//! "the third of whatever matched".
//!
//! Filtered-out rows stay visible but dimmed rather than disappearing, so the
//! user can see *why* an entry they expected is not highlighted.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use std::sync::Arc;

use crate::tui::input::InputMode;
use crate::tui::panels::SidePanelId;
use crate::tui::theme::Theme;

/// Upper bound on the filter text, so a held key cannot grow it without limit.
pub(crate) const MAX_QUERY_LEN: usize = 48;

/// The parts of a filterable panel list that are identical across boxes.
#[derive(Debug, Clone)]
pub(crate) struct PanelListState {
    query: String,
    /// Index into [`SidePanelId::ALL`], not into the filtered view.
    cursor: usize,
    mode: InputMode,
}

impl PanelListState {
    /// Open on `active`, so `Enter` with no movement is a no-op rather than a
    /// silent jump to whatever happens to be first.
    pub(crate) fn new(active: SidePanelId) -> Self {
        Self {
            query: String::new(),
            cursor: active.index(),
            mode: InputMode::Normal,
        }
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn mode(&self) -> InputMode {
        self.mode
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn filtering(&self) -> bool {
        !self.query().trim().is_empty()
    }

    pub(crate) fn push_query(&mut self, ch: char) {
        if self.query.chars().count() < MAX_QUERY_LEN {
            self.query.push(ch);
        }
    }

    pub(crate) fn pop_query(&mut self) {
        self.query.pop();
    }

    pub(crate) fn clear_query(&mut self) {
        self.query.clear();
    }

    /// The highlighted panel, if the registry is non-empty.
    pub(crate) fn selected(&self) -> Option<SidePanelId> {
        SidePanelId::ALL.get(self.cursor).copied()
    }

    /// True when `panel` passes the current filter.
    ///
    /// Whitespace-separated terms, all of which must match, against the three
    /// strings a user could plausibly remember: its name, its title, and what
    /// it is for.
    pub(crate) fn matches(&self, panel: SidePanelId) -> bool {
        let query = self.query().trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        query.split_whitespace().all(|term| {
            panel.label().to_lowercase().contains(term)
                || panel.title().to_lowercase().contains(term)
                || panel.hint().to_lowercase().contains(term)
        })
    }

    /// Move the highlight by `delta`, clamped to the registry.
    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let len = SidePanelId::ALL.len() as isize;
        if len == 0 {
            return;
        }
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }

    /// Scroll offset that keeps the highlighted row on screen.
    ///
    /// There are more panels than rows — 33 entries into roughly ten lines —
    /// so without this `j` walks the highlight straight off the bottom of the
    /// box and `Enter` then commits a panel nobody could see was selected.
    /// The highlight is the only thing saying what `Enter` will do, so a
    /// highlight that has scrolled away is a silent mis-selection.
    pub(crate) fn scroll(&self, list_height: u16) -> u16 {
        // Two rows go to the list's own border.
        let page = list_height.saturating_sub(2);
        if page == 0 {
            return 0;
        }
        let total = SidePanelId::ALL.len() as u16;
        // Never past the end: with fewer rows than panels, sitting at the
        // bottom is what puts the *last* entries on screen.
        let max_offset = total.saturating_sub(page);
        let cursor = self.cursor as u16;
        if cursor < page {
            return 0;
        }
        cursor
            .saturating_add(1)
            .saturating_sub(page)
            .min(max_offset)
    }

    /// The filter row: the query, the invitation to type one, and the mode.
    ///
    /// `subject` names what is being listed ("panels"), so a second box reads
    /// as itself rather than as a copy of the first.
    pub(crate) fn filter_row<'s>(
        &self,
        theme: &Arc<Theme>,
        subject: &'static str,
    ) -> Vec<Span<'s>> {
        let mut spans = if self.filtering() {
            vec![
                Span::styled("search ", Style::default().fg(theme.muted)),
                Span::styled(
                    format!("{}_", self.query()),
                    Style::default().fg(theme.primary),
                ),
            ]
        } else if self.mode == InputMode::Normal {
            vec![
                Span::styled("i to filter", Style::default().fg(theme.muted)),
                Span::styled(format!(" {subject}"), Style::default().fg(theme.border)),
            ]
        } else {
            vec![Span::styled(
                format!("type to filter {subject}…"),
                Style::default().fg(theme.muted),
            )]
        };
        spans.extend(self.mode_badge(theme));
        spans
    }

    /// Inverse-video `INS`/`NOR` badge, matching every other dockable box.
    ///
    /// It rides the filter row because that is the surface it governs: the box
    /// opens in Normal, and a user who presses `j` expecting to scroll and
    /// gets nothing typed has no other way to find out why.
    pub(crate) fn mode_badge<'s>(&self, theme: &Arc<Theme>) -> Vec<Span<'s>> {
        let (label, colour) = match self.mode {
            InputMode::Normal => ("NOR", theme.muted),
            InputMode::Insert => ("INS", theme.primary),
        };
        vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!(" {label} "),
                Style::default()
                    .fg(theme.background)
                    .bg(colour)
                    .add_modifier(Modifier::BOLD),
            ),
        ]
    }

    /// Style for one registry row, given whether it is highlighted.
    pub(crate) fn row_style(&self, theme: &Arc<Theme>, index: usize, visible: bool) -> Style {
        if !visible {
            Style::default().fg(theme.border)
        } else if index == self.cursor {
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground)
        }
    }

    /// Handle the keys every one of these boxes shares: `Esc`, the arrows,
    /// `j`/`k`, `i`/`a`, and Insert-mode text.
    ///
    /// Returns a message only for `Esc`-from-Normal, which is the one shared
    /// gesture that means something to the app rather than to the box. Every
    /// other key is answered locally, which is what keeps a second copy of this
    /// dialog from inventing its own `Enter`.
    pub(crate) fn handle_shared_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<crate::tui::app::TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc => {
                if self.mode == InputMode::Insert {
                    // Leave Insert the way every other box does, with the
                    // filter intact.
                    self.mode = InputMode::Normal;
                } else if self.query().is_empty() {
                    return Some(crate::tui::app::TuiMsg::CloseDialog);
                } else {
                    self.clear_query();
                }
            }
            // Paging and arrows are not text, so they keep their meaning while
            // typing. In Normal they walk the highlight.
            KeyCode::Up => self.move_cursor(-1),
            KeyCode::Down => self.move_cursor(1),
            KeyCode::PageUp => self.move_cursor(-10),
            KeyCode::PageDown => self.move_cursor(10),
            KeyCode::Backspace if self.mode == InputMode::Insert => {
                self.pop_query();
            }
            KeyCode::Char('j') if self.mode == InputMode::Normal => self.move_cursor(1),
            KeyCode::Char('k') if self.mode == InputMode::Normal => self.move_cursor(-1),
            // Reaching the filter. `Enter` is *not* one of them — it already
            // means "commit" — so `i`/`a` and Esc are the two ways across.
            KeyCode::Char('i') | KeyCode::Char('a') if self.mode == InputMode::Normal => {
                self.mode = InputMode::Insert;
            }
            KeyCode::Char(ch)
                if self.mode == InputMode::Insert
                    && (key.modifiers == KeyModifiers::NONE
                        || key.modifiers == KeyModifiers::SHIFT) =>
            {
                self.push_query(ch);
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_stays_on_a_real_panel_while_filtering() {
        let mut state = PanelListState::new(SidePanelId::Agents);
        let agents = state.selected().expect("registry is non-empty");
        assert_eq!(agents, SidePanelId::Agents);

        // Filtering must not renumber what Enter acts on.
        state.push_query('c');
        state.push_query('h');
        state.push_query('a');
        state.push_query('t');
        assert!(state.filtering());
        assert_eq!(state.selected(), Some(SidePanelId::Agents));
    }

    #[test]
    fn move_cursor_clamps_to_the_registry() {
        let mut state = PanelListState::new(SidePanelId::Session);
        state.move_cursor(-100);
        assert_eq!(state.cursor(), 0);
        state.move_cursor(10_000);
        assert_eq!(state.cursor(), SidePanelId::ALL.len() - 1);
    }

    #[test]
    fn the_query_is_bounded() {
        let mut state = PanelListState::new(SidePanelId::Session);
        for _ in 0..(MAX_QUERY_LEN * 3) {
            state.push_query('a');
        }
        assert_eq!(state.query().chars().count(), MAX_QUERY_LEN);
    }

    #[test]
    fn every_registry_panel_matches_an_empty_filter() {
        let state = PanelListState::new(SidePanelId::Session);
        for panel in SidePanelId::ALL.iter().copied() {
            assert!(
                state.matches(panel),
                "{panel:?} should match an empty filter"
            );
        }
    }
}
