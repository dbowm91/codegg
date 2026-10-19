use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use super::super::super::theme::Theme;
use super::super::component::{Component, DialogType};
use super::super::scroll::CenteredScroll;
use crate::session::Session;
use crate::tui::app::state::tasks::TaskGroup;
use crate::tui::app::TuiMsg;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortBy {
    #[default]
    Date,
    Title,
    Activity,
}

type SortedCache = Option<(String, bool, SortBy, Vec<usize>)>;

impl SortBy {
    fn next(&self) -> Self {
        match self {
            Self::Date => Self::Title,
            Self::Title => Self::Activity,
            Self::Activity => Self::Date,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Title => "title",
            Self::Activity => "activity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DatePreset {
    #[default]
    All,
    Today,
    Last7Days,
    Last30Days,
    ThisMonth,
}

impl DatePreset {
    fn next(&self) -> Self {
        match self {
            Self::All => Self::Today,
            Self::Today => Self::Last7Days,
            Self::Last7Days => Self::Last30Days,
            Self::Last30Days => Self::ThisMonth,
            Self::ThisMonth => Self::All,
        }
    }

    #[allow(dead_code)]
    fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Today => "today",
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::ThisMonth => "month",
        }
    }
}

/// One live subagent, as the dialog renders it.
///
/// An owned copy rather than a borrowed
/// [`crate::tui::app::state::subagents::SubagentRow`] so the row survives a
/// snapshot refresh that replaces the shared vector, and so `Clone` (which the
/// focus-manager push depends on) does not have to reconstruct it. Every field
/// is projection-derived; nothing here is minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentListRow {
    /// Canonical task identity. Keys the cursor; never the label.
    pub task_id: String,
    /// Display name, or the agent type when the node carried none.
    pub label: String,
    /// The agent type, shown beside the name.
    pub agent: String,
    /// What the subagent was asked to do.
    pub description: String,
    /// The pool's descriptor for this subagent's agent, or `None` for a node
    /// recorded before descriptors existed.
    ///
    /// Rendered in place of `description` when present, so this dialog and the
    /// `Ctrl+T` overlay — which this row is derived from — stay in step.
    pub profile: Option<String>,
    /// Raw status from the projection.
    pub status: String,
}

/// One live job, as the dialog renders it. Read-only: there is no job detail
/// view to open, so the row carries its identity and its text and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobListRow {
    /// Canonical job identity. Keys the cursor; never the list position.
    pub job_id: String,
    pub kind: String,
    pub summary: String,
    pub state: String,
    /// Present when the job failed; drives the error styling.
    pub error_class: Option<String>,
}

/// One line of the dialog's flat row space.
///
/// Flat on purpose: the cursor is a single `usize` walking this vector, and a
/// nested shape would buy nothing here because there is no per-section
/// collapsing. Session rows **lead** the vector, so a session's row index is
/// also its index into `sorted_sessions()` — which is what keeps
/// `selected_session()` and every bulk-mode gesture working unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionDialogRow {
    /// A session; `index` addresses [`SessionDialog::sorted_sessions`].
    Session(usize),
    /// A live subagent.
    Subagent(SubagentListRow),
    /// A live job.
    Job(JobListRow),
    /// A section heading. **Never selectable** — `j`/`k` skip these, so the
    /// cursor can never come to rest on a line `Enter` has nothing to do with.
    Header(TaskGroup),
}

impl SessionDialogRow {
    /// Whether the cursor may rest on this row.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, SessionDialogRow::Header(_))
    }
}

/// A row's identity, independent of where it sits in the vector.
///
/// Used to carry focus across a refresh. An index is not a focus: the moment
/// one subagent finishes, every row below it shifts up by one, and a cursor
/// held as an index silently lands on a *different* entity — the user presses
/// Enter believing they are reopening the subagent they were looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowIdentity {
    Session(String),
    Subagent(String),
    Job(String),
}

/// What the TUI knows about the checkout a session runs in.
///
/// M006: a session's *directory* is where it runs, but a directory is
/// not an answer. The answer is "a CodeGG-managed checkout on branch
/// `feature/x`", or "the project's own root". This is the small fact
/// the session dialog needs to answer that, and the fact a launch
/// action needs to name a checkout it may reuse.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckoutInfo {
    /// The managed checkout's id. `None` for a session that runs in the
    /// project's own root.
    pub worktree_id: Option<String>,
    /// The checkout's branch.
    pub branch: Option<String>,
    /// The checkout's durable state (`ready`, `orphaned`, …).
    pub state: Option<String>,
    /// The checkout's health (`healthy`, `git_error`, …).
    pub health: Option<String>,
}

impl CheckoutInfo {
    /// Whether the checkout needs an operator's attention.
    ///
    /// A checkout that is not `ready` is one an operator has to look at: a
    /// failed `git worktree add` leaves an *orphaned* record with a
    /// `git_error` health, and the directory it names may or may not
    /// exist. That is the cleanup state the plan asks the surface to
    /// show.
    ///
    /// A **dirty** checkout is a normal working state — the agent has
    /// uncommitted changes — and is not marked. A *conflicted*,
    /// *missing* or *failed* one is.
    pub fn needs_attention(&self) -> bool {
        let state = self.state.as_deref().unwrap_or_default();
        let health = self.health.as_deref().unwrap_or_default();
        let state_ok = state.is_empty() || state == "ready" || state == "in_use";
        let health_ok =
            health.is_empty() || health == "clean" || health == "dirty" || health == "unknown";
        !state_ok || !health_ok
    }
}

pub struct SessionDialog {
    pub theme: Arc<Theme>,
    pub project_context: Option<String>,
    pub filter: String,
    /// Index into [`SessionDialog::rows`], not into the session list.
    pub selected: usize,
    pub sessions: Vec<Session>,
    pub sort_by: SortBy,
    pub date_preset: DatePreset,
    pub show_archived: bool,
    pub message_counts: HashMap<String, usize>,
    pub search_messages: bool,
    pub message_preview: HashMap<String, String>,
    pub loading: bool,
    pub scroll: CenteredScroll,
    pub bulk_mode: bool,
    pub selected_ids: HashMap<String, bool>,
    /// Live subagents shown under the session list.
    subagents: Vec<SubagentListRow>,
    /// Live jobs shown under the subagents.
    jobs: Vec<JobListRow>,
    /// M006: the managed checkouts, keyed by the checkout's path.
    ///
    /// A session's `directory` is the join key: the launch binds the
    /// session's directory to the checkout's path, so the checkout a
    /// session runs in is the one whose path matches.
    checkouts: HashMap<String, CheckoutInfo>,
    visible_height: usize,
    sorted_cache: RefCell<SortedCache>,
}

impl Clone for SessionDialog {
    fn clone(&self) -> Self {
        Self {
            theme: Arc::clone(&self.theme),
            project_context: self.project_context.clone(),
            filter: self.filter.clone(),
            selected: self.selected,
            sessions: self.sessions.clone(),
            sort_by: self.sort_by,
            date_preset: self.date_preset,
            show_archived: self.show_archived,
            message_counts: self.message_counts.clone(),
            search_messages: self.search_messages,
            message_preview: self.message_preview.clone(),
            loading: self.loading,
            scroll: self.scroll.clone(),
            bulk_mode: self.bulk_mode,
            selected_ids: self.selected_ids.clone(),
            subagents: self.subagents.clone(),
            jobs: self.jobs.clone(),
            checkouts: self.checkouts.clone(),
            visible_height: self.visible_height,
            sorted_cache: RefCell::new(self.sorted_cache.borrow().clone()),
        }
    }
}

impl SessionDialog {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            project_context: None,
            filter: String::new(),
            selected: 0,
            sessions: Vec::new(),
            sort_by: SortBy::Date,
            date_preset: DatePreset::All,
            show_archived: false,
            message_counts: HashMap::new(),
            search_messages: false,
            message_preview: HashMap::new(),
            loading: false,
            scroll: CenteredScroll::new(),
            bulk_mode: false,
            selected_ids: HashMap::new(),
            subagents: Vec::new(),
            jobs: Vec::new(),
            checkouts: HashMap::new(),
            visible_height: 10,
            sorted_cache: RefCell::new(None),
        }
    }

    pub fn set_theme(&mut self, theme: &Arc<Theme>) {
        self.theme = Arc::clone(theme);
    }

    pub fn set_project_context(&mut self, context: Option<String>) {
        self.project_context = context;
    }

    pub fn cycle_date_preset(&mut self) {
        self.date_preset = self.date_preset.next();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub fn load_sessions(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;
        self.selected = 0;
        self.filter.clear();
        self.message_counts.clear();
        self.loading = false;
        self.scroll.reset();
        self.sorted_cache = RefCell::new(None);
    }

    pub fn set_message_count(&mut self, session_id: &str, count: usize) {
        self.message_counts.insert(session_id.to_string(), count);
        if self.sort_by == SortBy::Activity {
            self.sorted_cache = RefCell::new(None);
        }
    }

    /// M006: set the managed checkouts the sessions may run in.
    ///
    /// Keyed by the checkout's path, because that is what a session
    /// records: a session's `directory` is the checkout's path when the
    /// session runs in a managed checkout, and the project's own root
    /// otherwise. A session with no entry runs in the root.
    pub fn set_checkouts(&mut self, checkouts: HashMap<String, CheckoutInfo>) {
        self.checkouts = checkouts;
    }

    /// M006: the checkout the selected session runs in.
    pub fn selected_checkout(&self) -> Option<&CheckoutInfo> {
        let session = self.selected_session()?;
        self.checkouts.get(session.directory.as_str())
    }

    pub fn search_label(&self) -> &'static str {
        if self.search_messages {
            "messages"
        } else {
            "title"
        }
    }

    pub fn toggle_search_mode(&mut self) {
        self.search_messages = !self.search_messages;
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn set_message_preview(&mut self, session_id: &str, preview: String) {
        self.message_preview.insert(session_id.to_string(), preview);
    }

    pub fn clear_message_preview(&mut self) {
        self.message_preview.clear();
    }

    /// Replace the live subagent and job sections.
    ///
    /// Focus is carried by identity, not by index: the previously selected
    /// row's identity is remembered, and after the swap the cursor is placed
    /// on the row that *same* entity now occupies. A row that vanished (a
    /// subagent that finished mid-scroll) therefore leaves the cursor on the
    /// nearest remaining selectable row rather than on whatever entity slid up
    /// into the old index.
    ///
    /// An empty pair is not the same as "nothing is live", so there is no
    /// clearing path here — the caller simply stops passing rows it no longer
    /// has, which is what the projection-sync rule requires.
    pub fn set_pending_rows(&mut self, subagents: Vec<SubagentListRow>, jobs: Vec<JobListRow>) {
        let previous = self.selected_identity();
        self.subagents = subagents;
        self.jobs = jobs;
        self.restore_identity(previous);
    }

    /// Live subagents currently shown, for tests and for callers that need to
    /// assert on the section without rendering.
    pub fn subagent_rows(&self) -> &[SubagentListRow] {
        &self.subagents
    }

    /// Live jobs currently shown.
    pub fn job_rows(&self) -> &[JobListRow] {
        &self.jobs
    }

    /// The flat row space: sessions, then each non-empty section behind its
    /// own heading.
    ///
    /// A section contributes **nothing at all** when empty — no heading, no
    /// placeholder. A heading with no rows under it is a row that teaches the
    /// user to ignore headings, which is the same rule the overlay applies and
    /// the reason the two surfaces read alike.
    pub fn rows(&self) -> Vec<SessionDialogRow> {
        let session_count = self.sorted_sessions().len();
        let mut rows: Vec<SessionDialogRow> =
            (0..session_count).map(SessionDialogRow::Session).collect();

        if !self.subagents.is_empty() {
            rows.push(SessionDialogRow::Header(TaskGroup::Subagents));
            rows.extend(
                self.subagents
                    .iter()
                    .cloned()
                    .map(SessionDialogRow::Subagent),
            );
        }
        if !self.jobs.is_empty() {
            rows.push(SessionDialogRow::Header(TaskGroup::Jobs));
            rows.extend(self.jobs.iter().cloned().map(SessionDialogRow::Job));
        }
        rows
    }

    /// The row under the cursor, owned.
    ///
    /// Owned rather than borrowed because [`SessionDialog::rows`] builds a
    /// fresh vector on every call; handing out a reference into it would
    /// outlive the temporary.
    pub fn selected_row(&self) -> Option<SessionDialogRow> {
        self.rows().into_iter().nth(self.selected)
    }

    /// The identity of the selected row, if the cursor rests on one.
    fn selected_identity(&self) -> Option<RowIdentity> {
        let row = self.selected_row()?;
        row_identity(&row, &self.sorted_sessions())
    }

    /// Put the cursor back on `identity`, or on the nearest selectable row if
    /// that entity is gone.
    fn restore_identity(&mut self, identity: Option<RowIdentity>) {
        let rows = self.rows();
        let Some(identity) = identity else {
            // Nothing was focused (or the cursor sat on a heading). Fall back
            // to the first selectable row rather than leaving it on a header.
            self.select_first_selectable();
            return;
        };
        if let Some(index) = rows
            .iter()
            .position(|row| row_identity(row, &self.sorted_sessions()) == Some(identity.clone()))
        {
            self.selected = index;
            return;
        }
        self.clamp_selection(&rows);
    }

    fn select_first_selectable(&mut self) {
        let rows = self.rows();
        if let Some(index) = rows.iter().position(SessionDialogRow::is_selectable) {
            self.selected = index;
        }
        self.sync_scroll(rows.len());
    }

    /// Bring `selected` back inside the selectable range after the row space
    /// changed shape underneath it.
    fn clamp_selection(&mut self, rows: &[SessionDialogRow]) {
        if rows.is_empty() {
            self.selected = 0;
            return;
        }
        if self.selected < rows.len() && rows[self.selected].is_selectable() {
            return;
        }
        // Past the end: land on the last selectable row.
        for index in (0..rows.len()).rev() {
            if rows[index].is_selectable() {
                self.selected = index;
                return;
            }
        }
        // Only headings remain (impossible today, but the walk must terminate).
        self.selected = 0;
    }

    fn sync_scroll(&mut self, len: usize) {
        let scroll = self.scroll.get();
        if self.selected < scroll || self.selected >= scroll + self.visible_height {
            self.scroll.clamp(self.selected, len, self.visible_height);
        }
    }

    /// Move the cursor to `index`, skipping forward past any heading it lands
    /// on. Used by the click paths, which compute a rendered line position.
    pub fn select_index(&mut self, index: usize) {
        let rows = self.rows();
        let mut index = index.min(rows.len().saturating_sub(1));
        while index < rows.len() && !rows[index].is_selectable() {
            index += 1;
        }
        if index < rows.len() {
            self.selected = index;
        }
        self.sync_scroll(rows.len());
    }

    pub fn select_up(&mut self) {
        let rows = self.rows();
        // Normalise first: `self.selected` is a public field, so a caller (or
        // a stale index left by a shrink that did not go through
        // `set_pending_rows`) can leave it past the end. Walking from a
        // clamped start is what keeps `rows[index]` in bounds — and the clamp
        // is *assigned*, not just used as the walk's origin, so a cursor that
        // cannot move still ends up on a real row instead of staying invisible
        // at index 99.
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let mut index = self.selected;
        while index > 0 {
            index -= 1;
            if rows[index].is_selectable() {
                self.selected = index;
                break;
            }
        }
        self.sync_scroll(rows.len());
    }

    pub fn select_down(&mut self) {
        let rows = self.rows();
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let mut index = self.selected;
        while index + 1 < rows.len() {
            index += 1;
            if rows[index].is_selectable() {
                self.selected = index;
                break;
            }
        }
        self.sync_scroll(rows.len());
    }

    pub fn selected_session(&self) -> Option<&Session> {
        match self.selected_row()? {
            SessionDialogRow::Session(index) => self.sorted_sessions().get(index).copied(),
            // A subagent, a job, or a heading is not a session, and must never
            // be resolved as one. Sessions lead the vector, so a bare
            // `sorted_sessions().get(self.selected)` would be right by
            // coincidence rather than by construction — this matches on the
            // row type instead.
            SessionDialogRow::Subagent(_)
            | SessionDialogRow::Job(_)
            | SessionDialogRow::Header(_) => None,
        }
    }

    /// What `Enter` does, per row type.
    ///
    /// Three row types, three honest answers:
    ///
    /// - **Session** → `SelectSession`, as before.
    /// - **Subagent** → `OpenSubagentTab`, the *same* message the sidebar and
    ///   the overlay's working strip send for the same `task_id`. Dispatching
    ///   the shared message rather than opening a tab from here is what makes
    ///   "the same tab every surface opens" true by construction instead of by
    ///   three call sites agreeing. It opens a view; it cancels nothing.
    /// - **Job** → consumed. There is no job detail view to open, so inventing
    ///   a destination here would be inventing a feature; the row is
    ///   read-only and `Enter` on it does nothing at all.
    pub fn activate_selected(&self) -> Option<TuiMsg> {
        match self.selected_row()? {
            SessionDialogRow::Session(index) => self
                .sorted_sessions()
                .get(index)
                .copied()
                .cloned()
                .map(|session| TuiMsg::SelectSession(Box::new(session))),
            SessionDialogRow::Subagent(row) => Some(TuiMsg::OpenSubagentTab {
                task_id: row.task_id,
            }),
            SessionDialogRow::Job(_) => None,
            SessionDialogRow::Header(_) => None,
        }
    }

    /// Number of sessions currently loaded into this dialog.
    ///
    /// Exposed so the reload path can assert that the *painted* instance
    /// (the focus-manager clone) received the rows, not just the App-owned
    /// field — filling only the latter is exactly the defect that left
    /// `/sessions` showing an empty list.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn update_cache(&self) {
        let mut sessions: Vec<&Session> = if self.filter.is_empty() {
            self.sessions.iter().collect()
        } else {
            let lower = self.filter.to_lowercase();
            self.sessions
                .iter()
                .filter(|s| s.title.to_lowercase().contains(&lower))
                .collect()
        };

        if !self.show_archived {
            sessions.retain(|s| s.time_archived.is_none());
        }

        match self.sort_by {
            SortBy::Date => sessions.sort_by_key(|b| std::cmp::Reverse(b.time_updated)),
            SortBy::Title => sessions.sort_by_key(|a| a.title.to_lowercase()),
            SortBy::Activity => {
                sessions.sort_by(|a, b| {
                    let a_count = self.message_counts.get(&a.id).unwrap_or(&0);
                    let b_count = self.message_counts.get(&b.id).unwrap_or(&0);
                    b_count
                        .cmp(a_count)
                        .then_with(|| b.time_updated.cmp(&a.time_updated))
                });
            }
        }

        let index_map: std::collections::HashMap<&str, usize> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect();

        let indices: Vec<usize> = sessions
            .iter()
            .filter_map(|s| index_map.get(s.id.as_str()).copied())
            .collect();

        *self.sorted_cache.borrow_mut() = Some((
            self.filter.clone(),
            self.show_archived,
            self.sort_by,
            indices,
        ));
    }

    fn sorted_sessions(&self) -> Vec<&Session> {
        let cache_valid = {
            self.sorted_cache
                .borrow()
                .as_ref()
                .map(|(f, a, s, _)| {
                    f == &self.filter && *a == self.show_archived && *s == self.sort_by
                })
                .unwrap_or(false)
        };

        if !cache_valid {
            self.update_cache();
        }

        if let Some((_, _, _, ref indices)) = *self.sorted_cache.borrow() {
            indices
                .iter()
                .filter_map(|&i| self.sessions.get(i))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn set_filter(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn cycle_sort(&mut self) {
        self.sort_by = self.sort_by.next();
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn toggle_show_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.selected = 0;
        self.sorted_cache = RefCell::new(None);
    }

    pub fn sort_label(&self) -> &'static str {
        self.sort_by.label()
    }

    pub fn toggle_bulk_mode(&mut self) {
        self.bulk_mode = !self.bulk_mode;
        if !self.bulk_mode {
            self.selected_ids.clear();
        }
        self.sorted_cache = RefCell::new(None);
    }

    pub fn toggle_selection(&mut self) {
        // Bulk mode selects *sessions*. A subagent or job row carries no
        // session id, and the old positional lookup would have silently
        // resolved one — ticking a box against a subagent that bulk-archive
        // then cannot act on. Route through `selected_session()` so the
        // cursor's row type decides.
        if let Some(session) = self.selected_session() {
            let id = session.id.clone();
            let entry = self.selected_ids.entry(id).or_insert(false);
            *entry = !*entry;
        }
    }

    pub fn select_all(&mut self) {
        let ids: Vec<String> = self
            .sorted_sessions()
            .iter()
            .map(|s| s.id.clone())
            .collect();
        for id in ids {
            self.selected_ids.insert(id, true);
        }
    }

    pub fn deselect_all(&mut self) {
        self.selected_ids.clear();
    }

    pub fn selected_count(&self) -> usize {
        self.selected_ids.values().filter(|&&v| v).count()
    }

    pub fn get_selected_ids(&self) -> Vec<String> {
        self.selected_ids
            .iter()
            .filter(|(_, &v)| v)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// A row's identity, for matching a cursor across a row-space change.
///
/// Sessions are matched by id and not by their `usize`, because re-sorting or
/// re-filtering moves a session to a different index without changing what it
/// is.
fn row_identity(row: &SessionDialogRow, sorted: &[&Session]) -> Option<RowIdentity> {
    match row {
        SessionDialogRow::Session(index) => sorted
            .get(*index)
            .map(|s| RowIdentity::Session(s.id.clone())),
        SessionDialogRow::Subagent(row) => Some(RowIdentity::Subagent(row.task_id.clone())),
        SessionDialogRow::Job(row) => Some(RowIdentity::Job(row.job_id.clone())),
        SessionDialogRow::Header(_) => None,
    }
}

fn highlight_match<'a>(text: &'a str, query: &str, theme: &Theme) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::raw(text)];
    }

    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut result = Vec::new();
    let mut last_end = 0;
    let query_byte_len = lower_query.len();

    for match_start in lower_text.match_indices(&lower_query) {
        let start = match_start.0;
        if start > last_end {
            result.push(Span::raw(&text[last_end..start]));
        }
        result.push(Span::styled(
            &text[start..start + query_byte_len],
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
        last_end = start + query_byte_len;
    }

    if last_end < text.len() {
        result.push(Span::raw(&text[last_end..]));
    }

    result
}

impl Default for SessionDialog {
    fn default() -> Self {
        Self::new(Arc::new(Theme::dark()))
    }
}

impl Component for SessionDialog {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg> {
        // The panel docking gesture, under the same "filter is empty"
        // guard that keeps `j`/`k` as navigation below. While a query is
        // being typed, `Shift+L` is the letter `L` and must reach the buffer.
        if self.filter.is_empty() {
            if let Some(dir) = crate::tui::panels::direction_of(key) {
                return Some(TuiMsg::MoveDirection(dir));
            }
        }
        match key.code {
            crossterm::event::KeyCode::Esc => {
                if self.bulk_mode {
                    self.toggle_bulk_mode();
                    None
                } else {
                    Some(TuiMsg::CloseDialog)
                }
            }
            crossterm::event::KeyCode::Up => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Down => {
                self.select_down();
                None
            }
            // `j`/`k` move the session list only while the filter is empty;
            // once the user is typing a query they are ordinary characters,
            // exactly as `d`/`a` below already yield to the filter.
            crossterm::event::KeyCode::Char('k') if self.filter.is_empty() => {
                self.select_up();
                None
            }
            crossterm::event::KeyCode::Char('j') if self.filter.is_empty() => {
                self.select_down();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if self.bulk_mode {
                    self.toggle_bulk_mode();
                    None
                } else {
                    self.activate_selected()
                }
            }
            crossterm::event::KeyCode::Char('d') if !self.bulk_mode && self.filter.is_empty() => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ConfirmDeleteSession {
                        session_id: session.id.clone(),
                    });
                }
                None
            }
            crossterm::event::KeyCode::Char('a') if !self.bulk_mode && self.filter.is_empty() => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ConfirmArchiveSession {
                        session_id: session.id.clone(),
                        unarchive: session.time_archived.is_some(),
                    });
                }
                None
            }
            crossterm::event::KeyCode::Char('f') => {
                if let Some(session) = self.selected_session() {
                    return Some(TuiMsg::ForkSession {
                        session_id: session.id.clone(),
                    });
                }
                None
            }
            // M006: the launch actions.
            //
            // `n` places a *new* session in a *new* managed checkout.
            // `w` places a new session in the selected session's
            // *existing* checkout — starting a session in a linked
            // worktree must not create another one. `F` forks the
            // selected session into a new checkout; the lowercase `f`
            // above stays the plain fork into the *same* workspace, so
            // neither meaning silently takes the other's key.
            crossterm::event::KeyCode::Char('n') if !self.bulk_mode && self.filter.is_empty() => {
                Some(TuiMsg::LaunchSession {
                    parent_session_id: None,
                    worktree_id: None,
                })
            }
            crossterm::event::KeyCode::Char('w') if !self.bulk_mode && self.filter.is_empty() => {
                match self
                    .selected_checkout()
                    .and_then(|checkout| checkout.worktree_id.clone())
                {
                    Some(worktree_id) => Some(TuiMsg::LaunchSession {
                        parent_session_id: None,
                        worktree_id: Some(worktree_id),
                    }),
                    // The selected session runs in the project's own root:
                    // there is no checkout to reuse, and reusing "nothing"
                    // would silently allocate a checkout instead.
                    None => Some(TuiMsg::LaunchNeedsCheckout),
                }
            }
            crossterm::event::KeyCode::Char('F') if !self.bulk_mode && self.filter.is_empty() => {
                let parent_session_id = self.selected_session().map(|session| session.id.clone());
                Some(TuiMsg::LaunchSession {
                    parent_session_id,
                    worktree_id: None,
                })
            }
            crossterm::event::KeyCode::Char('s') => {
                self.cycle_sort();
                None
            }
            crossterm::event::KeyCode::Char('h') => {
                self.toggle_show_archived();
                None
            }
            crossterm::event::KeyCode::Char('b') => {
                self.toggle_bulk_mode();
                None
            }
            crossterm::event::KeyCode::Char('m') => {
                self.toggle_search_mode();
                None
            }
            crossterm::event::KeyCode::Char(' ') if self.bulk_mode => {
                self.toggle_selection();
                None
            }
            crossterm::event::KeyCode::Char('A') if self.bulk_mode => {
                self.select_all();
                None
            }
            crossterm::event::KeyCode::Char('D') if self.bulk_mode => {
                self.deselect_all();
                None
            }
            crossterm::event::KeyCode::Char('a') if self.bulk_mode => {
                let session_ids = self.get_selected_ids();
                let count = session_ids.len();
                if count > 0 {
                    Some(TuiMsg::ConfirmBulkArchive {
                        count,
                        unarchive: false,
                        session_ids,
                    })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char('d') if self.bulk_mode => {
                let session_ids = self.get_selected_ids();
                let count = session_ids.len();
                if count > 0 {
                    Some(TuiMsg::ConfirmBulkDelete { count, session_ids })
                } else {
                    None
                }
            }
            crossterm::event::KeyCode::Char(c) if !self.bulk_mode => {
                self.set_filter(c);
                None
            }
            crossterm::event::KeyCode::Backspace if !self.bulk_mode => {
                self.backspace_filter();
                None
            }
            _ => None,
        }
    }

    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> {
        self.filter.push_str(&text);
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    #[allow(clippy::incompatible_msrv)]
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        self.set_theme(theme);
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Sessions ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if let Some(context) = &self.project_context {
            lines.push(Line::from(vec![
                Span::styled("project: ", Style::default().fg(theme.muted)),
                Span::styled(context, Style::default().fg(theme.secondary)),
            ]));
            lines.push(Line::from(""));
        }

        if !self.filter.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("filter: ", Style::default().fg(theme.muted)),
                Span::raw(&self.filter),
            ]));
        }

        let sort_indicator = if self.show_archived {
            " [archived]"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled("sort: ", Style::default().fg(theme.muted)),
            Span::raw(self.sort_label()),
            Span::raw(sort_indicator),
        ]));
        lines.push(Line::from(vec![
            Span::styled("search: ", Style::default().fg(theme.muted)),
            Span::raw(self.search_label()),
        ]));
        lines.push(Line::from(""));

        let sorted = self.sorted_sessions();
        let rows = self.rows();
        let scroll = self.scroll.get();

        if rows.is_empty() {
            let label = if self.loading {
                "  Reloading..."
            } else {
                "  (no sessions loaded)"
            };
            lines.push(Line::from(Span::styled(
                label,
                Style::default().fg(theme.muted),
            )));
        } else {
            let mut row_idx = 0usize;
            for (i, row) in rows.iter().enumerate() {
                if i < scroll {
                    continue;
                }
                // Headings are never selectable, so they take no stripe and no
                // selection style — a highlighted heading would read as a row
                // the cursor is on.
                if let SessionDialogRow::Header(group) = row {
                    lines.push(Line::from(Span::styled(
                        format!(" ── {} ──", group.title()),
                        Style::default().fg(theme.muted),
                    )));
                    continue;
                }

                let selected = i == self.selected;
                let bg = if row_idx.is_multiple_of(2) {
                    theme.background
                } else {
                    theme.alternate_bg
                };
                row_idx += 1;
                let lead = if selected { "▸" } else { " " };

                let text = match row {
                    SessionDialogRow::Session(session_index) => {
                        let Some(s) = sorted.get(*session_index) else {
                            continue;
                        };
                        let is_archived = s.time_archived.is_some();
                        let is_forked = s.parent_id.is_some();

                        let ts = chrono::DateTime::from_timestamp_millis(s.time_updated)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        let msg_count = self.message_counts.get(&s.id).unwrap_or(&0);

                        // M006: fork ancestry. The parent's title is
                        // what the user recognises; the parent's id is
                        // what they cannot.
                        let parent_title = s
                            .parent_id
                            .as_ref()
                            .and_then(|parent_id| {
                                self.sessions
                                    .iter()
                                    .find(|candidate| &candidate.id == parent_id)
                            })
                            .map(|parent| parent.title.clone());

                        let title_suffix = if is_forked {
                            if is_archived {
                                " [archived]".to_string()
                            } else if let Some(parent_title) = parent_title.as_deref() {
                                format!(" (fork of {parent_title})")
                            } else {
                                " (fork)".to_string()
                            }
                        } else if is_archived {
                            " [archived]".to_string()
                        } else {
                            String::new()
                        };

                        // M006: where the session actually runs. A
                        // managed checkout shows its branch; the
                        // project's own root shows `root`; a checkout
                        // that needs an operator is marked.
                        let checkout_note = match self.checkouts.get(s.directory.as_str()) {
                            Some(checkout) => {
                                let branch = checkout.branch.as_deref().unwrap_or("checkout");
                                if checkout.needs_attention() {
                                    format!(" {branch} ⚠")
                                } else {
                                    format!(" {branch}")
                                }
                            }
                            None => " root".to_string(),
                        };

                        let display_title = if title_suffix.is_empty() {
                            s.title.clone()
                        } else {
                            format!("{}{}", s.title, title_suffix)
                        };

                        let checkbox = if self.bulk_mode {
                            if *self.selected_ids.get(&s.id).unwrap_or(&false) {
                                "[✓] "
                            } else {
                                "[ ] "
                            }
                        } else {
                            ""
                        };

                        let base = if selected {
                            theme.selection_style()
                        } else if is_archived {
                            Style::default().fg(theme.muted).bg(bg)
                        } else {
                            Style::default().fg(theme.foreground).bg(bg)
                        };
                        // The bulk checkbox replaces the cursor marker: a
                        // checkbox column and a `▸` in the same cell is a
                        // column that means two things.
                        let prefix = if self.bulk_mode {
                            checkbox.to_string()
                        } else {
                            lead.to_string()
                        };
                        lines.push(Line::from(Span::styled(
                            format!(
                                "{}{}  {}msgs  {}{}",
                                prefix, display_title, msg_count, ts, checkout_note
                            ),
                            base,
                        )));
                        continue;
                    }
                    SessionDialogRow::Subagent(subagent) => {
                        // Name and agent type together, exactly as the overlay
                        // renders them: the name is what the user is looking
                        // for, the type is what it actually is.
                        let name = if subagent.label == subagent.agent {
                            subagent.label.clone()
                        } else {
                            format!("{} ({})", subagent.label, subagent.agent)
                        };
                        // Descriptor when the projection carried one, the task description
                        // otherwise: same rule the `Ctrl+T` overlay applies to the
                        // row this was derived from.
                        let topic = subagent
                            .profile
                            .as_deref()
                            .unwrap_or(subagent.description.as_str());
                        if topic.trim().is_empty() {
                            format!("{lead}{name} · {}", subagent.status)
                        } else {
                            format!("{lead}{name} · {} — {}", subagent.status, topic)
                        }
                    }
                    SessionDialogRow::Job(job) => {
                        let colour = if job.error_class.is_some() {
                            theme.error
                        } else {
                            theme.foreground
                        };
                        let text = if job.summary.trim().is_empty() {
                            format!("{lead}{} · {}", job.kind, job.state)
                        } else {
                            format!("{lead}{} · {} — {}", job.kind, job.state, job.summary)
                        };
                        lines.push(Line::from(Span::styled(
                            text,
                            if selected {
                                theme.selection_style()
                            } else {
                                Style::default().fg(colour).bg(bg)
                            },
                        )));
                        continue;
                    }
                    SessionDialogRow::Header(_) => unreachable!("handled above"),
                };

                lines.push(Line::from(Span::styled(
                    text,
                    if selected {
                        theme.selection_style()
                    } else {
                        Style::default().fg(theme.foreground).bg(bg)
                    },
                )));
            }
        }

        lines.push(Line::from(""));

        if self.bulk_mode {
            let count = self.selected_count();
            lines.push(Line::from(Span::styled(
                format!(
                    "↑/↓ navigate  Space select  Esc exit bulk mode  ({} selected)",
                    count
                ),
                Style::default().fg(theme.muted),
            )));
            lines.push(Line::from(Span::styled(
                "a archive  d delete  A select all  D deselect all",
                Style::default().fg(theme.muted),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "↑/↓ navigate  |  Enter open  |  Esc close  |  b bulk mode",
                Style::default().fg(theme.muted),
            )));
            // Only claim the sections when they exist. A hint that advertises
            // `Enter opens a subagent` over a list with no subagents teaches
            // the user to look for something that is not there.
            if !self.subagents.is_empty() || !self.jobs.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Enter opens the subagent transcript · jobs are read-only",
                    Style::default().fg(theme.muted),
                )));
            }
            lines.push(Line::from(Span::styled(
                "d delete  a archive  f fork  s sort  h toggle archived  m search messages",
                Style::default().fg(theme.muted),
            )));
            // M006: the launch actions are advertised here because a key
            // nobody can find is a feature nobody has. `f` and `F` are
            // different acts, so the hint says which is which.
            lines.push(Line::from(Span::styled(
                "n new session in a checkout  w new session in this checkout  F fork into a checkout",
                Style::default().fg(theme.muted),
            )));
        }

        if self.search_messages && !self.filter.is_empty() {
            // The preview belongs to the selected *session*. With the cursor
            // parked on a subagent or job row there is no session to preview,
            // and indexing the session list by a row index that now addresses
            // a different entity would show another session's messages.
            if let Some(selected_s) = self.selected_session() {
                if let Some(preview) = self.message_preview.get(&selected_s.id) {
                    let lines_preview: Vec<&str> = preview.lines().take(3).collect();
                    for (i, line) in lines_preview.iter().enumerate() {
                        let highlighted = highlight_match(line, &self.filter, theme);
                        if i == 0 {
                            let mut line_parts =
                                vec![Span::styled("Contains: ", Style::default().fg(theme.muted))];
                            line_parts.extend(highlighted);
                            lines.push(Line::from(line_parts));
                        } else {
                            let mut line_parts = vec![Span::styled(
                                "           ",
                                Style::default().fg(theme.muted),
                            )];
                            line_parts.extend(highlighted);
                            lines.push(Line::from(line_parts));
                        }
                    }
                }
            }
        }

        let block = Block::default()
            .title(" Sessions ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        paragraph.render(area, frame.buffer_mut());
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Session
    }
}

#[cfg(test)]
mod vim_key_tests {
    use super::*;
    use crate::tui::components::component::Component;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn dialog() -> SessionDialog {
        SessionDialog::new(Arc::new(Theme::default()))
    }

    #[test]
    fn vim_keys_extend_an_active_filter() {
        // j/k yield to the filter exactly as the guarded `d`/`a` actions
        // in the same handler already do.
        let mut d = dialog();
        d.set_filter('a');
        d.handle_key(key('j'));
        d.handle_key(key('k'));
        assert_eq!(
            d.filter, "ajk",
            "j/k must be ordinary characters once a filter is being typed"
        );
    }

    #[test]
    fn vim_keys_do_not_type_while_the_filter_is_empty() {
        let mut d = dialog();
        d.handle_key(key('j'));
        d.handle_key(key('k'));
        assert_eq!(d.filter, "", "j/k must not open the filter");
    }

    #[test]
    fn sessions_dialog_shows_active_project_context() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut d = dialog();
        d.set_project_context(Some("project-a · /workspace/a".to_string()));
        let theme = Arc::new(Theme::default());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| d.render(frame, frame.area(), &theme))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("project-a"));
        assert!(text.contains("/workspace/a"));
    }
}

/// C002 — the sections under the session list.
#[cfg(test)]
mod pending_section_tests {
    use super::*;
    use crate::tui::app::TuiMsg;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn dialog() -> SessionDialog {
        SessionDialog::new(Arc::new(Theme::default()))
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
            ..Default::default()
        }
    }

    fn subagent(task_id: &str, label: &str) -> SubagentListRow {
        SubagentListRow {
            task_id: task_id.to_string(),
            label: label.to_string(),
            agent: "explore".to_string(),
            description: "look around".to_string(),
            profile: None,
            status: "running".to_string(),
        }
    }

    fn job(job_id: &str, kind: &str) -> JobListRow {
        JobListRow {
            job_id: job_id.to_string(),
            kind: kind.to_string(),
            summary: "build the thing".to_string(),
            state: "running".to_string(),
            error_class: None,
        }
    }

    fn loaded(count: usize) -> SessionDialog {
        let mut d = dialog();
        d.load_sessions((0..count).map(|i| session(&format!("s{i}"))).collect());
        d
    }

    fn headers(d: &SessionDialog) -> Vec<TaskGroup> {
        d.rows()
            .into_iter()
            .filter_map(|row| match row {
                SessionDialogRow::Header(group) => Some(group),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sections_appear_when_rows_exist_and_vanish_when_they_do_not() {
        let mut d = loaded(2);
        assert!(
            headers(&d).is_empty(),
            "a sessions-only dialog must not render an empty section heading"
        );

        d.set_pending_rows(vec![subagent("t1", "Sally")], vec![job("j1", "test")]);
        assert_eq!(
            headers(&d),
            vec![TaskGroup::Subagents, TaskGroup::Jobs],
            "both headings appear once they have a row under them"
        );
        assert_eq!(
            d.rows().len(),
            6,
            "2 sessions + 2 headings + 1 subagent row + 1 job row"
        );

        // A refresh that reports nothing live withdraws the whole section,
        // heading included — not a heading stranded over an empty list.
        d.set_pending_rows(Vec::new(), Vec::new());
        assert!(headers(&d).is_empty());
        assert_eq!(d.rows().len(), 2, "only the sessions remain");
    }

    #[test]
    fn a_section_with_no_rows_contributes_no_heading() {
        let mut d = loaded(1);
        d.set_pending_rows(Vec::new(), vec![job("j1", "test")]);
        assert_eq!(
            headers(&d),
            vec![TaskGroup::Jobs],
            "an empty subagent section must not spend a row on a heading"
        );
    }

    #[test]
    fn navigation_walks_sessions_then_subagents_then_jobs_and_skips_headings() {
        let mut d = loaded(2);
        d.set_pending_rows(
            vec![subagent("t1", "Sally"), subagent("t2", "Frank")],
            vec![job("j1", "test")],
        );

        // Driven through `handle_key`, not through `select_down`, because the
        // requirement is about the keys: `j` is only navigation while the
        // filter is empty, and a call to the method would pass even if the
        // binding were gone.
        // rows: S0 S1 [Subagents] t1 t2 [Jobs] j1  == 7 rows.
        let seen: Vec<String> = (0..7)
            .map(|_| {
                let id = match d.selected_row().expect("a row is always selectable here") {
                    SessionDialogRow::Session(i) => format!("s{i}"),
                    SessionDialogRow::Subagent(row) => row.task_id,
                    SessionDialogRow::Job(row) => row.job_id,
                    SessionDialogRow::Header(_) => "HEADER".to_string(),
                };
                d.handle_key(key('j'));
                id
            })
            .collect();

        assert_eq!(
            seen,
            vec!["s0", "s1", "t1", "t2", "j1", "j1", "j1"],
            "j must skip both headings and stop at the last row"
        );
        assert!(
            !seen.contains(&"HEADER".to_string()),
            "the cursor came to rest on a section heading"
        );

        // And back up, which is where an off-by-one on the headings shows.
        for _ in 0..7 {
            d.handle_key(key('k'));
        }
        assert!(
            matches!(d.selected_row(), Some(SessionDialogRow::Session(0))),
            "k must walk back to the first session, got {:?}",
            d.selected_row()
        );
    }

    #[test]
    fn enter_on_a_subagent_row_opens_the_shared_tab_message() {
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t7", "Sally")], Vec::new());
        d.select_down(); // s0 -> the subagent (heading skipped)

        match d.handle_key(enter()) {
            // The same message `TuiMsg::OpenSubagentTab` the sidebar and the
            // working strip send, so all three routes open one tab.
            Some(TuiMsg::OpenSubagentTab { task_id }) => assert_eq!(task_id, "t7"),
            other => panic!("expected the shared OpenSubagentTab message, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_a_job_row_changes_nothing_and_closes_nothing() {
        let mut d = loaded(1);
        d.set_pending_rows(Vec::new(), vec![job("j9", "test")]);
        d.select_down();

        assert_eq!(
            d.handle_key(enter()),
            None,
            "a job row is read-only: Enter must not invent a destination"
        );
        assert!(
            d.filter.is_empty(),
            "Enter must not leak into the filter buffer either"
        );
    }

    #[test]
    fn enter_on_a_session_row_still_selects_the_session() {
        let mut d = loaded(2);
        match d.handle_key(enter()) {
            Some(TuiMsg::SelectSession(session)) => assert_eq!(session.id, "s0"),
            other => panic!("expected SelectSession, got {other:?}"),
        }
    }

    #[test]
    fn a_subagent_row_is_never_resolved_as_a_session() {
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t1", "Sally")], vec![job("j1", "test")]);
        d.select_down();
        assert!(
            d.selected_session().is_none(),
            "a subagent row must not resolve to the session sitting above it"
        );
        // The same for a job row, and for the heading in between.
        d.select_down();
        assert!(d.selected_session().is_none(), "a job row is not a session");
        d.select_down();
        assert!(d.selected_session().is_none());
    }

    #[test]
    fn a_vanished_row_does_not_move_the_cursor_onto_another_entity() {
        let mut d = loaded(1);
        d.set_pending_rows(
            vec![
                subagent("t1", "Sally"),
                subagent("t2", "Frank"),
                subagent("t3", "Ida"),
            ],
            Vec::new(),
        );
        // Park the cursor on the middle subagent: s0, [Sub], t1, t2.
        d.select_down();
        d.select_down();
        assert!(matches!(
            d.selected_row(),
            Some(SessionDialogRow::Subagent(ref r)) if r.task_id == "t2"
        ));

        // t2 finishes: its row goes, everything below it shifts up one.
        d.set_pending_rows(
            vec![subagent("t1", "Sally"), subagent("t3", "Ida")],
            Vec::new(),
        );

        // t3 is still live, so focus follows it rather than holding an index
        // that now points at a different subagent.
        assert!(
            matches!(
                d.selected_row(),
                Some(SessionDialogRow::Subagent(ref r)) if r.task_id == "t3"
            ),
            "focus must follow the entity, not the index; got {:?}",
            d.selected_row()
        );
    }

    #[test]
    fn focus_falls_back_to_a_selectable_row_when_the_focused_row_is_gone() {
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t1", "Sally")], Vec::new());
        d.select_down();
        assert!(matches!(
            d.selected_row(),
            Some(SessionDialogRow::Subagent(_))
        ));

        // The subagent finishes and its whole section disappears.
        d.set_pending_rows(Vec::new(), Vec::new());
        let row = d.selected_row().expect("the session list still has a row");
        assert!(
            row.is_selectable(),
            "focus must never be left on a heading or past the end: {row:?}"
        );
    }

    #[test]
    fn bulk_mode_selection_ignores_a_subagent_row() {
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t1", "Sally")], Vec::new());
        d.toggle_bulk_mode();
        d.select_down();
        d.toggle_selection();
        assert_eq!(
            d.selected_count(),
            0,
            "a subagent carries no session id; ticking it would arm a bulk \
             archive against a session the user never selected"
        );
    }

    #[test]
    fn clicking_a_heading_snaps_to_the_next_selectable_row() {
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t1", "Sally")], Vec::new());
        d.select_index(1); // row 1 is the Subagents heading
        assert!(
            matches!(
                d.selected_row(),
                Some(SessionDialogRow::Subagent(ref r)) if r.task_id == "t1"
            ),
            "a click on a heading must land on a row, got {:?}",
            d.selected_row()
        );
    }

    #[test]
    fn the_rendered_dialog_shows_both_sections() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t7", "Sally")], vec![job("j3", "cargo")]);

        let theme = Arc::new(Theme::default());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| d.render(frame, frame.area(), &theme))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(text.contains("Subagents"), "no subagent heading was drawn");
        assert!(text.contains("Sally"), "no subagent name was drawn");
        assert!(text.contains("Jobs"), "no jobs heading was drawn");
        assert!(text.contains("cargo"), "no job row was drawn");
    }

    #[test]
    fn a_stale_selection_index_past_the_end_does_not_panic_or_strand_the_cursor() {
        // `selected` is a public field the click handlers and the legacy
        // router assign directly, so an index past the end of the row space is
        // reachable. `select_up` walks backwards from it, which is exactly the
        // shape that indexes out of bounds.
        let mut d = loaded(1);
        d.set_pending_rows(vec![subagent("t1", "Sally")], vec![job("j1", "test")]);
        d.selected = 99;

        d.select_up();
        assert!(
            d.selected_row().map(|r| r.is_selectable()).unwrap_or(false),
            "k must land on a real row, got {:?}",
            d.selected_row()
        );

        d.selected = 99;
        d.select_down();
        assert!(
            d.selected_row().map(|r| r.is_selectable()).unwrap_or(false),
            "j must land on a real row, got {:?}",
            d.selected_row()
        );
    }

    #[test]
    fn a_sessions_only_dialog_renders_exactly_as_before() {
        // With no pending rows the row space is the session list, so the
        // pre-existing rendering is untouched — that is the regression guard
        // for every other `/sessions` test in the suite.
        let d = loaded(2);
        assert_eq!(d.rows().len(), 2);
        assert_eq!(d.selected_row(), Some(SessionDialogRow::Session(0)));
        assert_eq!(
            d.selected_session().map(|s| s.id.clone()),
            Some("s0".into())
        );
    }
}

/// M006: the checkout surface.
///
/// A session's row has to say *where the session runs* — a CodeGG-managed
/// checkout on a branch, or the project's own root — and *when that
/// checkout needs an operator*. A row that only shows a title and a
/// timestamp makes a checkout invisible, and an invisible checkout is
/// where a session's "which directory is this?" question goes to die.
#[cfg(test)]
mod checkout_surface_tests {
    use super::*;
    use crate::tui::app::TuiMsg;
    use crate::tui::components::component::Component;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn session(id: &str, directory: &str) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
            directory: directory.to_string(),
            ..Default::default()
        }
    }

    fn checkout(path: &str, branch: &str, state: &str, health: &str) -> CheckoutInfo {
        CheckoutInfo {
            worktree_id: Some(format!("wt-{path}")),
            branch: Some(branch.to_string()),
            state: Some(state.to_string()),
            health: Some(health.to_string()),
        }
    }

    fn rendered(d: &mut SessionDialog) -> String {
        let theme = Arc::new(Theme::default());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| d.render(frame, frame.area(), &theme))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn shift_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    #[test]
    fn a_session_in_a_managed_checkout_shows_its_branch() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/wt/feature-x")]);
        d.set_checkouts(HashMap::from([(
            "/wt/feature-x".to_string(),
            checkout("/wt/feature-x", "feature-x", "ready", "clean"),
        )]));

        let text = rendered(&mut d);
        assert!(
            text.contains("feature-x"),
            "the row must name the checkout's branch: {text}"
        );
        assert!(
            !text.contains("root"),
            "a session in a managed checkout is not in the root: {text}"
        );
    }

    #[test]
    fn a_session_in_the_project_root_is_marked_as_the_root() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/repo")]);

        let text = rendered(&mut d);
        assert!(
            text.contains("root"),
            "a session with no managed checkout runs in the root: {text}"
        );
    }

    #[test]
    fn a_dirty_checkout_is_not_marked() {
        // A dirty checkout is a normal working state — the agent has
        // uncommitted changes — and marking it would make the marker
        // meaningless.
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/wt/dirty")]);
        d.set_checkouts(HashMap::from([(
            "/wt/dirty".to_string(),
            checkout("/wt/dirty", "dirty", "ready", "dirty"),
        )]));

        let text = rendered(&mut d);
        assert!(
            !text.contains('⚠'),
            "a dirty checkout is not an operator problem: {text}"
        );
    }

    #[test]
    fn a_checkout_that_needs_attention_is_marked() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/wt/broken")]);
        d.set_checkouts(HashMap::from([(
            "/wt/broken".to_string(),
            checkout("/wt/broken", "broken", "orphaned", "git_error"),
        )]));

        let text = rendered(&mut d);
        assert!(
            text.contains('⚠'),
            "a checkout that needs an operator must be marked: {text}"
        );
    }

    #[test]
    fn a_fork_names_the_session_it_came_from() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        let mut parent = session("p1", "/repo");
        parent.title = "Parent".to_string();
        let mut child = session("c1", "/wt/fork");
        child.title = "Child".to_string();
        child.parent_id = Some("p1".to_string());
        d.load_sessions(vec![parent, child]);

        let text = rendered(&mut d);
        assert!(
            text.contains("fork of Parent"),
            "the row must name the fork's ancestry: {text}"
        );
    }

    #[test]
    fn the_create_action_asks_for_a_new_checkout() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/repo")]);

        let msg = d.handle_key(key('n'));
        assert_eq!(
            msg,
            Some(TuiMsg::LaunchSession {
                parent_session_id: None,
                worktree_id: None,
            })
        );
    }

    #[test]
    fn the_reuse_action_names_the_selected_checkout() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/wt/feature-x")]);
        d.set_checkouts(HashMap::from([(
            "/wt/feature-x".to_string(),
            checkout("/wt/feature-x", "feature-x", "ready", "clean"),
        )]));

        let msg = d.handle_key(key('w'));
        assert_eq!(
            msg,
            Some(TuiMsg::LaunchSession {
                parent_session_id: None,
                worktree_id: Some("wt-/wt/feature-x".to_string()),
            })
        );
    }

    #[test]
    fn the_reuse_action_refuses_when_the_session_has_no_checkout() {
        // Reusing "nothing" would quietly allocate a checkout instead,
        // which is the opposite of what the key asked for.
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/repo")]);

        let msg = d.handle_key(key('w'));
        assert_eq!(msg, Some(TuiMsg::LaunchNeedsCheckout));
    }

    #[test]
    fn the_fork_action_names_the_parent_session() {
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/repo")]);

        let msg = d.handle_key(shift_key('F'));
        assert_eq!(
            msg,
            Some(TuiMsg::LaunchSession {
                parent_session_id: Some("s1".to_string()),
                worktree_id: None,
            })
        );
    }

    #[test]
    fn the_plain_fork_key_still_forks_into_the_same_workspace() {
        // The lowercase `f` is the pre-existing plain fork. M006's fork
        // into a checkout is `F`; neither may take the other's key.
        let mut d = SessionDialog::new(Arc::new(Theme::default()));
        d.load_sessions(vec![session("s1", "/repo")]);

        let msg = d.handle_key(key('f'));
        assert_eq!(
            msg,
            Some(TuiMsg::ForkSession {
                session_id: "s1".to_string(),
            })
        );
    }
}
