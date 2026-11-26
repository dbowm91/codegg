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
//! The cursor indexes [`SidePanelId::ALL`]; a filter hides rows without
//! renumbering it, and the highlight follows the nearest match.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use std::sync::Arc;

use crate::input::InputMode;
use crate::panels::{PanelClass, SidePanelId};
use crate::theme::Theme;

/// Upper bound on the filter text, so a held key cannot grow it without limit.
pub(crate) const MAX_QUERY_LEN: usize = 48;

/// The parts of a filterable panel list that are identical across boxes.
#[derive(Debug, Clone)]
pub(crate) struct PanelListState {
    query: String,
    /// Index into [`SidePanelId::ALL`], not into the filtered view.
    cursor: usize,
    mode: InputMode,
    show_all: bool,
}

impl PanelListState {
    /// Open on `active`, so `Enter` with no movement is a no-op rather than a
    /// silent jump to whatever happens to be first.
    pub(crate) fn new(active: SidePanelId) -> Self {
        Self {
            query: String::new(),
            cursor: active.index(),
            mode: InputMode::Normal,
            show_all: false,
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
        SidePanelId::ALL.get(self.effective_cursor()).copied()
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

    pub(crate) fn appears(&self, index: usize) -> bool {
        let Some(panel) = SidePanelId::ALL.get(index).copied() else {
            return false;
        };
        if self.filtering() {
            return self.matches(panel);
        }
        self.show_all || panel.class() == PanelClass::Full
    }

    /// The row the highlight and `Enter` point at: the nearest listed one.
    pub(crate) fn effective_cursor(&self) -> usize {
        if self.appears(self.cursor) {
            return self.cursor;
        }
        let len = SidePanelId::ALL.len();
        (self.cursor..len)
            .chain(0..self.cursor)
            .find(|index| self.appears(*index))
            .unwrap_or(self.cursor)
    }

    pub(crate) fn shows_all(&self) -> bool {
        self.show_all
    }

    pub(crate) fn toggle_show_all(&mut self) {
        self.show_all = !self.show_all;
    }

    pub(crate) fn shown(&self) -> Vec<usize> {
        (0..SidePanelId::ALL.len())
            .filter(|index| self.appears(*index))
            .collect()
    }

    /// Move by `delta`, skipping rows that are not listed.
    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let len = SidePanelId::ALL.len() as isize;
        if len == 0 || delta == 0 {
            return;
        }
        let step = delta.signum();
        for _ in 0..delta.unsigned_abs() {
            let mut next = self.cursor as isize + step;
            let mut moved = false;
            while next >= 0 && next < len {
                if self.appears(next as usize) {
                    self.cursor = next as usize;
                    moved = true;
                    break;
                }
                next += step;
            }
            if !moved {
                break;
            }
        }
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
        let shown = self.shown();
        let Some(position) = shown
            .iter()
            .position(|index| *index == self.effective_cursor())
        else {
            return 0;
        };
        let total = shown.len() as u16;
        // Never past the end: with fewer rows than panels, sitting at the
        // bottom is what puts the *last* entries on screen.
        let max_offset = total.saturating_sub(page);
        let cursor = position as u16;
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
    ) -> Option<crate::app::TuiMsg> {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc => {
                if self.mode == InputMode::Insert {
                    // Leave Insert the way every other box does, with the
                    // filter intact.
                    self.mode = InputMode::Normal;
                } else if self.query().is_empty() {
                    return Some(crate::app::TuiMsg::CloseDialog);
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
    fn filtering_points_the_highlight_at_the_panel_the_user_typed() {
        let mut state = PanelListState::new(SidePanelId::Agents);
        assert_eq!(state.selected(), Some(SidePanelId::Agents));
        for ch in "chat".chars() {
            state.push_query(ch);
        }
        assert!(state.filtering());
        // The raw cursor is a registry index and never renumbers.
        assert_eq!(state.cursor(), SidePanelId::Agents.index());
        // The row Enter acts on follows the filter, so typing a name and
        // pressing Enter loads *that* panel.
        assert_eq!(state.selected(), Some(SidePanelId::Chat));
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
    fn the_last_row_is_reachable_through_a_one_row_viewport() {
        let mut state = PanelListState::new(SidePanelId::Session);
        state.move_cursor(10_000);
        let last = SidePanelId::ALL.len() - 1;
        assert_eq!(state.cursor(), last);
        let shown = state.shown();
        let last_position = shown.len() - 1;
        assert_eq!(shown.last().copied(), Some(last));
        assert_eq!(
            state.scroll(3) as usize,
            last_position,
            "a three-row box shows one content row after its border, and this \
             helper subtracts those two rows itself — so the offset must be the \
             last listed row rather than zero"
        );
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
