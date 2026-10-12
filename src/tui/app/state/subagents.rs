//! Active-subagent state for the main buffer's working strip.
//!
//! The main buffer previously advertised delegated work only through a
//! `[...3]` badge in the header. That states *how many* subagents exist and
//! nothing about *who* they are or what they are doing, so a turn that fans
//! out to several agents reads as one opaque spinner. This module owns the
//! rows that make that work legible, plus the buffer view opened by clicking
//! one.
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

use ratatui::layout::Rect;

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
    pub run_id: Option<String>,
    /// The subagent's name, e.g. `explore`.
    pub agent: String,
    /// What the subagent was asked to do.
    pub description: String,
    /// Raw status string from the projection.
    pub status: String,
}

impl SubagentRow {
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
    pub fn summary_line(&self, width: usize) -> String {
        let glyph = if self.is_running() { "▸" } else { "·" };
        let status = self.status.trim().to_ascii_lowercase();
        let status = if status.is_empty() {
            "running".to_string()
        } else {
            status
        };
        let lead = format!("  {glyph} {}", self.agent);
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

/// The subagent buffer opened over the main buffer.
///
/// `Esc` clears it and returns to the transcript. It is deliberately a plain
/// view rather than a modal dialog: a modal would take the keyboard hostage
/// and make "Esc goes back" ambiguous with "Esc closes a dialog".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentView {
    /// The row that was clicked, captured so the view stays coherent even if
    /// the next projection snapshot drops the run.
    pub row: SubagentRow,
    /// Scroll offset within the rendered view, in rows.
    pub scroll: u16,
}

/// Subagent state for the main buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentState {
    /// Rows currently advertised, in projection order.
    pub rows: Vec<SubagentRow>,
    /// The open subagent buffer, if any.
    pub open: Option<SubagentView>,
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
    /// A snapshot with no subagents clears the list. The open view is left
    /// alone: it holds its own captured row, so a run disappearing from the
    /// tree does not yank the buffer the user is reading out from under them.
    pub fn set_rows(&mut self, rows: Vec<SubagentRow>) {
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

    /// Open the subagent buffer for `index`, returning whether a row was hit.
    pub fn open_row(&mut self, index: usize) -> bool {
        let Some(row) = self.rows.get(index).cloned() else {
            return false;
        };
        self.open = Some(SubagentView { row, scroll: 0 });
        true
    }

    /// Close the subagent buffer, returning to the main buffer.
    pub fn close(&mut self) {
        self.open = None;
        self.hits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str) -> SubagentRow {
        SubagentRow {
            task_id: "t1".into(),
            run_id: Some("r1".into()),
            agent: "explore".into(),
            description: "Find the auth flow".into(),
            status: status.into(),
        }
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
            state.open.as_ref().map(|v| v.row.agent.clone()),
            Some("explore".into())
        );
        // An out-of-range index must not clobber an open view.
        assert!(!state.open_row(7));
        assert!(state.open.is_some());
        state.close();
        assert!(state.open.is_none());
    }

    #[test]
    fn a_snapshot_dropping_a_run_leaves_the_open_view_intact() {
        let mut state = SubagentState::default();
        state.set_rows(vec![row("running")]);
        state.open_row(0);
        state.set_rows(Vec::new());
        assert!(state.rows.is_empty());
        assert!(state.open.is_some(), "the view holds its own captured row");
    }
}
