//! Project Work Orders M004: global Workspace dashboard state.
//!
//! Frontend-only projection/controller state for the global Workspace
//! dashboard. Durable truth stays daemon-side behind the single bounded
//! `CoreRequest::WorkspaceDashboard` aggregate; this module owns no
//! sessions, jobs, schedules, or preferences and performs no I/O. Every
//! async completion that consumes this state carries the dashboard
//! `generation` plus the routing `reconnect_epoch` so stale completions
//! (tab close, reconnect, superseding refresh, revocation) are dropped
//! at apply time.
//!
//! The user-facing "Workspace" dashboard is not a new durable Workspace
//! object and does not change the canonical meaning of `WorkspaceId`:
//! rows are per-project activity summaries used for navigation only.
//! Entering/leaving the dashboard never cancels or mutates running
//! work; closing a project tab elsewhere invalidates matching stale
//! route tokens (checked by the caller via `UiRouteToken`).

use crate::protocol::work_order::{ProjectActivitySummaryDto, WorkOrderDto};
use crate::tui::app::state::async_request::AsyncUiRequestState;
use crate::tui::app::state::project_tabs::ProjectTabId;

/// Maximum dashboard rows cached frontend-side (matches the daemon
/// page bound so one page never exceeds the cache).
pub const MAX_DASHBOARD_ROWS: usize = 128;

/// Maximum visible rows rendered in the dashboard viewport.
pub const MAX_DASHBOARD_VISIBLE_ROWS: usize = 16;

/// Maximum filter-query characters.
pub const MAX_DASHBOARD_FILTER_LEN: usize = 128;

/// Maximum expanded running-task titles shown inline for one project.
pub const MAX_DASHBOARD_EXPANDED_TASKS: usize = 8;

/// One cached dashboard row plus frontend staleness.
#[derive(Debug, Clone)]
pub struct WorkspaceDashboardRow {
    pub summary: ProjectActivitySummaryDto,
    /// Set by event hints (`WorkOrderChanged`, session/permission
    /// activity) until the next bounded refresh replaces the row.
    pub stale: bool,
}

impl WorkspaceDashboardRow {
    pub fn attention_count(&self) -> u64 {
        self.summary.needs_attention_count
            + self.summary.pending_permission_count
            + self.summary.pending_question_count
    }

    pub fn running_count(&self) -> u64 {
        self.summary.running_session_count + self.summary.running_work_order_count
    }
}

/// Global dashboard view state: bounded cached rows, filter/selection,
/// refresh generation, return-to-prior-view identity, and one lazily
/// fetched inline expansion for the selected project.
#[derive(Debug)]
pub struct WorkspaceDashboardState {
    /// Cached rows in daemon order (attention/running/recency/name).
    pub rows: Vec<WorkspaceDashboardRow>,
    /// Current filter query text (bounded).
    pub query: String,
    /// Selected position within [`Self::filtered_indices`].
    pub selected_row: usize,
    /// Whether the daemon reported truncation (more pages exist).
    pub truncated: bool,
    /// Cursor for the next page (last `project_id`), if any.
    pub next_cursor: Option<String>,
    /// Refresh generation: bumped on every successful load; late
    /// completions with an older generation are dropped.
    pub generation: u64,
    /// Reconnect epoch captured when the dashboard was opened; late
    /// completions from a prior epoch are dropped.
    pub reconnect_epoch: u64,
    /// The project tab that was active when the dashboard opened.
    /// `Esc` returns focus there without reloading unrelated state.
    pub return_tab: Option<ProjectTabId>,
    /// Event-hint dirty flag: set without stealing focus; cleared by a
    /// bounded refresh.
    pub dirty: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Inline expansion: selected project with lazily fetched running
    /// tasks (one bounded `WorkOrderList`, never N+1 per row).
    pub expanded_project_id: Option<String>,
    pub expanded_tasks: Vec<WorkOrderDto>,
    pub expanded_loading: bool,
    pub expanded_error: Option<String>,
    /// Async request state for dashboard load/expand operations.
    pub request: AsyncUiRequestState,
}

impl WorkspaceDashboardState {
    pub fn new(return_tab: Option<ProjectTabId>, reconnect_epoch: u64) -> Self {
        Self {
            rows: Vec::new(),
            query: String::new(),
            selected_row: 0,
            truncated: false,
            next_cursor: None,
            generation: 0,
            reconnect_epoch,
            return_tab,
            dirty: false,
            loading: true,
            error: None,
            notice: None,
            expanded_project_id: None,
            expanded_tasks: Vec::new(),
            expanded_loading: false,
            expanded_error: None,
            request: AsyncUiRequestState::new(),
        }
    }

    /// Compute filtered positions over the cached rows. Uses the same
    /// fuzzy scoring convention as the project picker (display name,
    /// project id), capped at [`MAX_DASHBOARD_ROWS`]. Daemon order is
    /// preserved within equal scores by stable sort.
    pub fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.trim();
        let mut scored: Vec<(usize, usize)> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let name_score = crate::util::fuzzy::fuzzy_score(query, &row.summary.display_name);
                let id_score = crate::util::fuzzy::fuzzy_score(query, &row.summary.project_id);
                (i, name_score.max(id_score))
            })
            .filter(|(_, score)| *score > 0 || query.is_empty())
            .collect();
        scored.sort_by_key(|item| std::cmp::Reverse(item.1));
        scored
            .into_iter()
            .take(MAX_DASHBOARD_ROWS)
            .map(|(i, _)| i)
            .collect()
    }

    /// Push filter text, bounded.
    pub fn push_filter(&mut self, ch: char) {
        if self.query.chars().count() < MAX_DASHBOARD_FILTER_LEN {
            self.query.push(ch);
            self.selected_row = 0;
        }
    }

    pub fn pop_filter(&mut self) {
        self.query.pop();
        self.selected_row = 0;
    }

    pub fn clear_filter(&mut self) {
        if !self.query.is_empty() {
            self.query.clear();
            self.selected_row = 0;
        }
    }

    /// Move selection up/down over the filtered list, clamped.
    pub fn select_up(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        self.selected_row = self.selected_row.saturating_sub(1).min(count - 1);
    }

    pub fn select_down(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        self.selected_row = (self.selected_row + 1).min(count - 1);
    }

    /// Move selection by `delta` rows (PgUp/PgDn friendly), clamped.
    pub fn move_selection(&mut self, delta: isize) {
        let count = self.filtered_indices().len();
        if count == 0 {
            self.selected_row = 0;
            return;
        }
        let next = (self.selected_row as isize + delta).clamp(0, count as isize - 1) as usize;
        self.selected_row = next;
    }

    pub fn go_top(&mut self) {
        self.selected_row = 0;
    }

    pub fn go_bottom(&mut self) {
        let count = self.filtered_indices().len();
        self.selected_row = count.saturating_sub(1);
    }

    /// Currently selected row, if any.
    pub fn selected(&self) -> Option<&WorkspaceDashboardRow> {
        let indices = self.filtered_indices();
        indices
            .get(self.selected_row)
            .and_then(|&i| self.rows.get(i))
    }

    /// Begin a new load generation; returns the generation the
    /// completion must carry to be accepted.
    pub fn begin_refresh(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.generation
    }

    /// Apply one bounded dashboard page. Returns `false` (dropped) when
    /// `generation` is stale. Replacement is whole-page: rows never mix
    /// across generations or authorization epochs.
    pub fn apply_loaded(
        &mut self,
        generation: u64,
        rows: Vec<ProjectActivitySummaryDto>,
        truncated: bool,
        next_cursor: Option<String>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let capped = rows.into_iter().take(MAX_DASHBOARD_ROWS);
        self.rows = capped
            .map(|summary| WorkspaceDashboardRow {
                summary,
                stale: false,
            })
            .collect();
        self.truncated = truncated;
        self.next_cursor = next_cursor;
        self.loading = false;
        self.dirty = false;
        self.error = None;
        let count = self.filtered_indices().len();
        if self.selected_row >= count {
            self.selected_row = count.saturating_sub(1);
        }
        // Expansion references a prior generation: drop it rather than
        // show tasks for a possibly revoked project.
        self.expanded_project_id = None;
        self.expanded_tasks.clear();
        self.expanded_loading = false;
        self.expanded_error = None;
        true
    }

    /// Mark rows dirty from event hints without stealing focus. Unknown
    /// project hints mark the whole view dirty (bounded refresh will
    /// reconcile); known rows are flagged individually.
    pub fn mark_hint_dirty(&mut self, project_id: Option<&str>) {
        match project_id {
            Some(id) => {
                for row in &mut self.rows {
                    if row.summary.project_id == id {
                        row.stale = true;
                    }
                }
                self.dirty = true;
            }
            None => {
                self.dirty = true;
            }
        }
    }

    /// Fail-closed revocation: drop the row and any expansion/detail for
    /// `project_id` immediately on denial or archive. Hidden data must
    /// not linger after authorization is revoked.
    pub fn clear_revoked(&mut self, project_id: &str) {
        self.rows.retain(|row| row.summary.project_id != project_id);
        if self.expanded_project_id.as_deref() == Some(project_id) {
            self.expanded_project_id = None;
            self.expanded_tasks.clear();
            self.expanded_loading = false;
            self.expanded_error = None;
        }
        let count = self.filtered_indices().len();
        if self.selected_row >= count {
            self.selected_row = count.saturating_sub(1);
        }
    }

    /// Begin an inline expansion fetch for `project_id`. Only one
    /// expansion is live at a time; switching projects replaces it.
    pub fn begin_expand(&mut self, project_id: &str) {
        self.expanded_project_id = Some(project_id.to_string());
        self.expanded_tasks.clear();
        self.expanded_loading = true;
        self.expanded_error = None;
    }

    /// Apply lazily fetched tasks for the expanded project. Dropped
    /// when the expansion moved on or the generation is stale.
    pub fn apply_expanded(
        &mut self,
        generation: u64,
        project_id: &str,
        tasks: Vec<WorkOrderDto>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        if self.expanded_project_id.as_deref() != Some(project_id) {
            return false;
        }
        self.expanded_tasks = tasks
            .into_iter()
            .take(MAX_DASHBOARD_EXPANDED_TASKS)
            .collect();
        self.expanded_loading = false;
        self.expanded_error = None;
        true
    }

    /// Total attention badges across visible rows (for tests/badges).
    pub fn total_attention(&self) -> u64 {
        self.rows.iter().map(|row| row.attention_count()).sum()
    }
}

/// Coarse one-line badge for a dashboard row. Counts only; never the
/// underlying permission/question content.
pub fn dashboard_row_badge(row: &WorkspaceDashboardRow) -> String {
    let summary = &row.summary;
    let mut bits = Vec::new();
    if summary.pending_permission_count > 0 {
        bits.push(format!("permission×{}", summary.pending_permission_count));
    }
    if summary.pending_question_count > 0 {
        bits.push(format!("question×{}", summary.pending_question_count));
    }
    if summary.needs_attention_count > 0 {
        bits.push(format!("attention×{}", summary.needs_attention_count));
    }
    if summary.running_session_count + summary.running_work_order_count > 0 {
        bits.push(format!(
            "running×{}",
            summary.running_session_count + summary.running_work_order_count
        ));
    }
    if summary.waiting_work_order_count + summary.future_work_order_count > 0 {
        bits.push(format!(
            "future×{}",
            summary.waiting_work_order_count + summary.future_work_order_count
        ));
    }
    if bits.is_empty() {
        return summary.coarse_status_code.clone();
    }
    bits.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, name: &str, attention: u64, running: u64) -> WorkspaceDashboardRow {
        WorkspaceDashboardRow {
            summary: ProjectActivitySummaryDto {
                project_id: id.to_string(),
                display_name: name.to_string(),
                lifecycle: "active".to_string(),
                running_session_count: running,
                running_work_order_count: 0,
                waiting_work_order_count: 1,
                future_work_order_count: 1,
                needs_attention_count: attention,
                pending_permission_count: 0,
                pending_question_count: 0,
                last_activity_at: Some(100),
                coarse_status_code: if attention > 0 {
                    "attention".to_string()
                } else {
                    "waiting".to_string()
                },
                counts_visible: true,
            },
            stale: false,
        }
    }

    fn state_with(rows: Vec<WorkspaceDashboardRow>) -> WorkspaceDashboardState {
        let mut state = WorkspaceDashboardState::new(None, 0);
        state.loading = false;
        state.rows = rows;
        state
    }

    #[test]
    fn empty_dashboard_has_no_selection() {
        let state = state_with(vec![]);
        assert!(state.selected().is_none());
        assert_eq!(state.filtered_indices().len(), 0);
    }

    #[test]
    fn navigation_clamps_over_filtered_rows() {
        let mut state = state_with(vec![
            row("a", "Alpha", 0, 0),
            row("b", "Beta", 1, 0),
            row("c", "Gamma", 0, 2),
        ]);
        assert_eq!(state.selected().unwrap().summary.project_id, "a");
        state.select_down();
        state.select_down();
        assert_eq!(state.selected().unwrap().summary.project_id, "c");
        state.select_down();
        assert_eq!(state.selected().unwrap().summary.project_id, "c");
        state.select_up();
        assert_eq!(state.selected().unwrap().summary.project_id, "b");
        state.go_top();
        assert_eq!(state.selected().unwrap().summary.project_id, "a");
        state.go_bottom();
        assert_eq!(state.selected().unwrap().summary.project_id, "c");
        state.move_selection(-10_000);
        assert_eq!(state.selected().unwrap().summary.project_id, "a");
        state.move_selection(10_000);
        assert_eq!(state.selected().unwrap().summary.project_id, "c");
    }

    #[test]
    fn filter_matches_display_name_and_resets_selection() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0), row("b", "Beta", 0, 0)]);
        state.go_bottom();
        state.query = "alp".to_string();
        state.selected_row = 5;
        let indices = state.filtered_indices();
        assert_eq!(indices.len(), 1);
        state.selected_row = state.selected_row.min(indices.len().saturating_sub(1));
        assert_eq!(state.selected().unwrap().summary.project_id, "a");
    }

    #[test]
    fn filter_push_pop_bounded_and_resets_selection() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0)]);
        state.selected_row = 0;
        state.push_filter('x');
        assert_eq!(state.query, "x");
        state.pop_filter();
        assert!(state.query.is_empty());
        for _ in 0..(MAX_DASHBOARD_FILTER_LEN + 10) {
            state.push_filter('y');
        }
        assert!(state.query.chars().count() <= MAX_DASHBOARD_FILTER_LEN);
        state.clear_filter();
        assert!(state.query.is_empty());
    }

    #[test]
    fn stale_generation_apply_is_dropped() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0)]);
        let gen = state.begin_refresh();
        assert!(!state.apply_loaded(gen.wrapping_add(1), vec![], false, None));
        assert_eq!(state.rows.len(), 1);
        assert!(state.apply_loaded(gen, vec![], false, None));
        assert!(state.rows.is_empty());
    }

    #[test]
    fn apply_loaded_replaces_rows_and_drops_expansion() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0)]);
        let gen = state.begin_refresh();
        state.begin_expand("a");
        assert!(state.apply_loaded(gen, vec![row("b", "Beta", 0, 0).summary], false, None));
        assert_eq!(state.rows.len(), 1);
        assert!(state.expanded_project_id.is_none());
        assert!(!state.dirty);
        assert!(!state.loading);
    }

    #[test]
    fn hint_marks_row_stale_without_focus_change() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0), row("b", "Beta", 0, 0)]);
        let selected_before = state.selected_row;
        state.mark_hint_dirty(Some("b"));
        assert!(state.rows[1].stale);
        assert!(!state.rows[0].stale);
        assert!(state.dirty);
        assert_eq!(state.selected_row, selected_before);
    }

    #[test]
    fn unknown_hint_marks_view_dirty() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0)]);
        state.mark_hint_dirty(Some("missing"));
        assert!(state.dirty);
        state.dirty = false;
        state.mark_hint_dirty(None);
        assert!(state.dirty);
    }

    #[test]
    fn revocation_clears_row_and_expansion() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0), row("b", "Beta", 0, 0)]);
        state.begin_expand("b");
        state.clear_revoked("b");
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].summary.project_id, "a");
        assert!(state.expanded_project_id.is_none());
        assert!(state.expanded_tasks.is_empty());
    }

    #[test]
    fn expanded_apply_rejects_stale_generation_and_project() {
        let mut state = state_with(vec![row("a", "Alpha", 0, 0)]);
        let gen = state.begin_refresh();
        state.begin_expand("a");
        assert!(!state.apply_expanded(gen.wrapping_add(1), "a", vec![]));
        assert!(state.expanded_loading);
        assert!(!state.apply_expanded(gen, "other", vec![]));
        assert!(state.apply_expanded(gen, "a", vec![]));
        assert!(!state.expanded_loading);
    }

    #[test]
    fn badge_is_coarse_counts_only() {
        let summary = row("a", "Alpha", 2, 1).summary;
        let badge = dashboard_row_badge(&WorkspaceDashboardRow {
            summary,
            stale: false,
        });
        assert!(badge.contains("attention×2"));
        assert!(badge.contains("running×1"));
        assert!(!badge.contains("secret"));
    }

    #[test]
    fn dashboard_row_ordering_prefers_attention_then_running() {
        let idle = row("idle", "Idle", 0, 0);
        let running = row("run", "Running", 0, 3);
        let attention = row("att", "Attention", 2, 0);
        assert!(attention.attention_count() > running.attention_count());
        assert!(running.running_count() > idle.running_count());
    }
}
