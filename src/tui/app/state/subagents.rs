//! Active-subagent state for the main buffer's working strip.
//!
//! The main buffer previously advertised delegated work only through a
//! `[...3]` badge in the header. That states *how many* subagents exist and
//! nothing about *who* they are or what they are doing, so a turn that fans
//! out to several agents reads as one opaque spinner. This module owns the
//! rows that make that work legible, plus the tab set opened by selecting one.
//!
//! Two rules shape the design:
//!
//! - The strip is **projection-derived, never synthesized**. Rows come from
//!   the same `agent_tree` / `agent_runs` snapshot the sidebar already
//!   consumes, so the main buffer cannot disagree with the sidebar about
//!   which runs are active.
//! - The strip is **view state**. It may be absent, stale, or empty without
//!   that being an error: there is no synthetic "0 subagents" row, and a
//!   missing snapshot clears the rows rather than inventing any.
//!
//! Open subagents live in [`SubagentTabs`], which owns identity, display order,
//! and the attached/detached distinction. This module owns only the *rows* and
//! the mouse hit geometry, and reconciles the two on every arriving snapshot.

use ratatui::layout::Rect;

use crate::tui::app::state::subagent_tabs::{SubagentTabId, SubagentTabState, SubagentTabs};
use crate::tui::wrap::ellipsize_to_width;

/// Hard-truncate to `width` chars, with no ellipsis.
///
/// Used where the budget can legitimately be 0: `ellipsize_to_width`
/// returns its input unchanged in that case, which would defeat the bound.
fn take_chars(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}

/// One delegated run advertised in the working strip.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentRow {
    /// Canonical task identity; the join key against durable runs.
    pub task_id: String,
    /// Durable run id when one exists, so the view can fetch run detail.
    ///
    /// Carried on the event rather than joined locally: the projection's
    /// `task_id` is a daemon-local `u64` while `AgentRunSummary::task_id` is a
    /// UUID string, so a local join on those two can never match.
    pub run_id: Option<String>,
    /// Human-readable name drawn from the curated pool at spawn, e.g. `Sally`.
    ///
    /// `None` for a node recorded before display names existed; see
    /// [`SubagentRow::label`].
    pub display_name: Option<String>,
    /// The subagent's name, e.g. `explore`.
    pub agent: String,
    /// What the subagent was asked to do.
    pub description: String,
    /// Raw status string from the projection.
    pub status: String,
}

impl SubagentRow {
    /// What to call this subagent in the UI.
    ///
    /// Prefers the curated display name (`Sally`) and falls back to the agent
    /// type (`explore`) when the node predates display names. The fallback is
    /// a real value rather than a placeholder so a replayed older snapshot
    /// still reads correctly instead of showing an empty column.
    pub fn label(&self) -> &str {
        match self.display_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => self.agent.as_str(),
        }
    }

    /// Whether this row is still running.
    ///
    /// The projection spells statuses inconsistently across producers, so
    /// this matches on the lowercase token rather than an exact string; a
    /// status the producer invents later defaults to *not* running, which
    /// keeps a stale row from claiming to be live.
    pub fn is_running(&self) -> bool {
        let status = self.status.trim().to_ascii_lowercase();
        status.is_empty()
            || status.contains("running")
            || status.contains("in_progress")
            || status.contains("in-progress")
            || status.contains("active")
            || status.contains("pending")
            || status.contains("queued")
            || status.contains("started")
    }

    /// The single buffer row for this subagent, ellipsized to `width`.
    ///
    /// The activity column carries the *description* rather than a free-form
    /// progress string: the projection adapter hardcodes `progress: None`, so
    /// there is no live progress text to show. The description is what the
    /// user can actually act on, and it is what identifies the work.
    ///
    /// The lead shows the curated display name when there is one. The agent
    /// type still matters, so a name is shown *with* its type rather than
    /// instead of it: `Sally (explore)` tells the user both which human label
    /// to look for later and what the subagent actually is.
    pub fn summary_line(&self, width: usize) -> String {
        let glyph = if self.is_running() { "▸" } else { "·" };
        let status = self.status.trim().to_ascii_lowercase();
        let status = if status.is_empty() {
            "running".to_string()
        } else {
            status
        };
        let lead = match self.display_name.as_deref() {
            Some(name) if !name.trim().is_empty() && name != self.agent => {
                format!("  {glyph} {name} ({})", self.agent)
            }
            _ => format!("  {glyph} {}", self.agent),
        };
        let tail = format!(" · {status}");
        let sep = " — ";

        // `ellipsize_to_width` returns its input unchanged at width 0, so a
        // naive `width - lead - tail` budget silently overflows instead of
        // truncating. Every branch below therefore either has a non-zero
        // budget or bypasses the helper.
        let fixed = lead.chars().count() + sep.chars().count() + tail.chars().count();
        if width <= fixed {
            // Too narrow for the full row: keep the status (it is what
            // marks a live row from a finished one) and trim the lead.
            if width <= tail.chars().count() {
                return take_chars(&tail, width);
            }
            let head = take_chars(&lead, width - tail.chars().count());
            return format!("{head}{tail}");
        }

        // Reserve the tail before the description so an ellipsized
        // description still leaves the status readable.
        let budget = width - fixed;
        let description = ellipsize_to_width(&self.description, budget);
        format!("{lead}{sep}{description}{tail}")
    }
}

/// Subagent state for the main buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentState {
    /// Rows currently advertised, in projection order.
    pub rows: Vec<SubagentRow>,
    /// Subagents opened as tabs in the main buffer. `None` as the active tab
    /// *is* the main conversation; see [`SubagentTabs`].
    pub tabs: SubagentTabs,
    /// Hit rects for the rendered rows, in absolute frame coordinates,
    /// paired with their index into `rows`.
    ///
    /// Rebuilt on every render. Mouse hit testing reads the *last rendered*
    /// geometry rather than recomputing a layout, so the click target is
    /// exactly the row the user saw — the same contract the scrollbar drag
    /// path uses.
    pub hits: Vec<(usize, Rect)>,
}

impl SubagentState {
    /// Replace the advertised rows from a projection snapshot.
    ///
    /// A snapshot with no subagents clears the list. Open tabs are *not*
    /// closed: each is reconciled instead, so a tab whose subject just left
    /// the tree detaches rather than vanishing from under the user, and a tab
    /// whose subject came back is refreshed and re-attached.
    ///
    /// Only call this when a snapshot actually arrived. A missing snapshot is
    /// not evidence that a subject is gone, so routing that case here would
    /// detach every live tab during a projection hiccup.
    pub fn set_rows(&mut self, rows: Vec<SubagentRow>) {
        self.tabs.sync_with_rows(&rows);
        self.rows = rows;
    }

    /// Number of subagents still running.
    pub fn running_count(&self) -> usize {
        self.rows.iter().filter(|row| row.is_running()).count()
    }

    /// Which row index the given frame cell falls on, if any.
    pub fn hit_at(&self, column: u16, row: u16) -> Option<usize> {
        self.hits
            .iter()
            .find(|(_, rect)| {
                column >= rect.x
                    && column < rect.x + rect.width
                    && row >= rect.y
                    && row < rect.y + rect.height
            })
            .map(|(index, _)| *index)
    }

    /// The subagent tab currently on screen, if any.
    pub fn active_tab(&self) -> Option<&SubagentTabState> {
        self.tabs.active()
    }

    /// Mutable accessor for the active tab.
    pub fn active_tab_mut(&mut self) -> Option<&mut SubagentTabState> {
        self.tabs.active_mut()
    }

    /// Open the subagent at `index` as a tab, returning whether a row was hit.
    ///
    /// Delegated to [`SubagentTabs::open_or_focus`], so a subject that is
    /// already open gains focus rather than duplicating its tab.
    pub fn open_row(&mut self, index: usize) -> bool {
        let Some(row) = self.rows.get(index).cloned() else {
            return false;
        };
        self.open(row)
    }

    /// Open the tab for the given projection `task_id`, if a row carries it.
    ///
    /// The keyboard path into a tab, used by the tasks overlay and — from M004
    /// — the sessions panel. Both address a subject by its canonical id rather
    /// than by a row index, so they cannot open a tab for a row that has since
    /// shifted under them.
    pub fn open_task(&mut self, task_id: &str) -> bool {
        let Some(row) = self.rows.iter().find(|row| row.task_id == task_id).cloned() else {
            return false;
        };
        self.open(row)
    }

    /// Whether a tab for `task_id` is already open.
    pub fn has_tab(&self, task_id: &str) -> bool {
        self.tabs.contains(task_id)
    }

    /// Open `row` as a tab, returning whether the tab set took it.
    ///
    /// `false` covers both refusals the tab set makes — a blank `task_id` that
    /// cannot key a tab, and a set already at capacity. The caller decides
    /// whether either is worth telling the user about.
    pub fn open(&mut self, row: SubagentRow) -> bool {
        self.tabs.open_or_focus(row).is_some()
    }

    /// Move `offset` places through the tab cycle.
    pub fn cycle_tabs(&mut self, offset: isize) -> bool {
        self.tabs.cycle(offset).is_some()
    }

    /// Close the active tab, returning to the main buffer.
    ///
    /// With no tabs open this is a no-op rather than an error: the main buffer
    /// is already what is showing.
    pub fn close(&mut self) {
        self.tabs.close_active();
        self.hits.clear();
    }

    /// The tab id for `task_id`, if one is open.
    pub fn tab_id_for(&self, task_id: &str) -> Option<&SubagentTabId> {
        self.tabs
            .active_tab_id()
            .filter(|id| id.as_str() == task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str) -> SubagentRow {
        SubagentRow {
            task_id: "t1".into(),
            run_id: Some("r1".into()),
            display_name: Some("Sally".into()),
            agent: "explore".into(),
            description: "Find the auth flow".into(),
            status: status.into(),
        }
    }

    fn row_id(task_id: &str, status: &str) -> SubagentRow {
        SubagentRow {
            task_id: task_id.into(),
            ..row(status)
        }
    }

    #[test]
    fn label_prefers_the_display_name() {
        assert_eq!(row("running").label(), "Sally");
    }

    #[test]
    fn label_falls_back_to_the_agent_type() {
        let mut r = row("running");
        r.display_name = None;
        assert_eq!(r.label(), "explore");
        // A blank name is treated as absent rather than rendered as a gap.
        r.display_name = Some("   ".into());
        assert_eq!(r.label(), "explore");
    }

    #[test]
    fn summary_line_shows_name_and_agent_together() {
        let line = row("running").summary_line(60);
        assert!(line.contains("Sally"), "{line}");
        assert!(line.contains("explore"), "{line}");
    }

    #[test]
    fn summary_line_does_not_duplicate_the_agent_as_a_parenthetical() {
        let mut r = row("running");
        // When the display name equals the agent type, print it once.
        r.display_name = Some("explore".into());
        let line = r.summary_line(60);
        assert_eq!(line.matches("explore").count(), 1, "{line}");
    }

    #[test]
    fn live_statuses_are_recognized() {
        for status in ["Running", "running", "in_progress", "active", "pending", ""] {
            assert!(row(status).is_running(), "{status} should count as running");
        }
        for status in ["Completed", "failed", "cancelled"] {
            assert!(
                !row(status).is_running(),
                "{status} should not count as running"
            );
        }
    }

    #[test]
    fn summary_line_ellipsizes_instead_of_overflowing() {
        let mut long = row("running");
        long.description = "x".repeat(400);
        let line = long.summary_line(40);
        assert!(
            line.chars().count() <= 40,
            "got {} cols",
            line.chars().count()
        );
        // The status survives truncation — it is what marks a live row.
        assert!(line.ends_with("running"), "{line}");
    }

    #[test]
    fn summary_line_fits_a_very_narrow_strip() {
        for width in 0..=30usize {
            let line = row("running").summary_line(width);
            assert!(
                line.chars().count() <= width,
                "width {width} produced {} cols: {line:?}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn summary_line_keeps_the_status_when_it_has_to_truncate() {
        // Whatever room is left after the status must not be spent on the
        // description: the status is what marks a live row.
        let line = row("running").summary_line(16);
        assert!(line.chars().count() <= 16, "{line:?}");
        assert!(line.contains("running"), "{line:?}");
    }

    #[test]
    fn running_count_ignores_finished_rows() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running"), row("completed"), row("running")]);
        assert_eq!(state.running_count(), 2);
    }

    #[test]
    fn hit_testing_resolves_the_rendered_row() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running"), row("running")]);
        state.hits = vec![(0, Rect::new(2, 10, 30, 1)), (1, Rect::new(2, 11, 30, 1))];
        assert_eq!(state.hit_at(5, 11), Some(1));
        assert_eq!(state.hit_at(5, 10), Some(0));
        assert_eq!(state.hit_at(5, 12), None);
        assert_eq!(state.hit_at(40, 10), None);
    }

    #[test]
    fn opening_a_row_captures_it_and_closing_returns_to_main() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running")]);
        assert!(state.open_row(0));
        assert_eq!(
            state.active_tab().map(|tab| tab.row.agent.clone()),
            Some("explore".into())
        );
        // An out-of-range index must not clobber an open tab.
        assert!(!state.open_row(7));
        assert!(state.active_tab().is_some());
        state.close();
        assert!(state.active_tab().is_none());
    }

    #[test]
    fn opening_the_same_row_twice_focuses_one_tab() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running"), row_id("t2", "running")]);
        assert!(state.open_row(0));
        assert!(state.open_row(1));
        assert!(state.open_row(0));
        assert_eq!(state.tabs.len(), 2, "no duplicate tab for the same subject");
        assert_eq!(
            state.active_tab().map(|tab| tab.tab_id.as_str()),
            Some("t1")
        );
    }

    #[test]
    fn a_task_id_addresses_the_same_tab_a_row_index_does() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running"), row_id("t2", "running")]);
        assert!(state.open_task("t2"));
        assert_eq!(
            state.active_tab().map(|tab| tab.tab_id.as_str()),
            Some("t2")
        );
        assert!(state.has_tab("t2"));
        assert!(!state.has_tab("nope"));
        // A task id with no row opens nothing rather than inventing one.
        assert!(!state.open_task("nope"));
        assert_eq!(state.tabs.len(), 1);
    }

    #[test]
    fn a_snapshot_dropping_a_run_detaches_its_tab_rather_than_closing_it() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running")]);
        state.open_row(0);
        state.set_rows(Vec::new());

        assert!(state.rows.is_empty());
        let tab = state.active_tab().expect("the tab survives its subject");
        assert_eq!(tab.row.task_id, "t1", "it keeps the row it was opened with");
        assert!(
            !tab.is_running(),
            "a detached tab must not keep claiming live work"
        );
    }

    #[test]
    fn closing_with_no_tabs_open_is_a_no_op() {
        let mut state = SubagentState::default();
        state.close();
        assert!(state.active_tab().is_none());
        assert!(state.tabs.is_empty());
    }
}
