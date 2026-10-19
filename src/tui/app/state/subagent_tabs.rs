//! The subagent tab set over the main LLM buffer.
//!
//! Opening a delegated run used to be a single-slot affair: one `Option` held
//! the open view, so there was no way to keep two subagents side by side and
//! no way to say which one of several was on screen. This module makes that a
//! real tab set, deliberately mirroring [`crate::tui::app::state::project_tabs::ProjectTabs`]
//! rather than inventing a second convention. It is a **sibling** of project
//! tabs, not a replacement: project tabs live in the header, subagent tabs live
//! in the main buffer, and the two axes never consult each other.
//!
//! # Identity is the projection's, and it is not durable
//!
//! A [`SubagentTabId`] *is* the projection's canonical `task_id`. That is a
//! conscious departure from `ProjectTabId`, which mints a fresh UUID: a
//! subagent already carries a stable, snapshot-derived identity, and inventing
//! a second one would mean carrying a mapping between the two for no benefit.
//!
//! It is also the reason this module cannot promise anything across a restart.
//! `task_id` is a daemon-local `u64` minted per dispatched job, and the per-turn
//! agent tree is cleared when the turn ends (roadmap defect D-b). A tab id is
//! valid for exactly as long as this process remembers it, and nothing may
//! persist one.
//!
//! # "The subject left the snapshot" is a state, not an error
//!
//! Because the tree is per-turn, a tab's subject can vanish while the tab is
//! still open — the turn ended, or the 16-node per-turn cap evicted the node.
//! [`SubagentTabLiveness::Detached`] is that state, and it is load-bearing
//! rather than cosmetic: [`SubagentTabState::is_running`] refuses to report
//! `true` for a detached tab, so the buffer can never keep claiming a run is
//! live after the projection stopped vouching for it.
//!
//! # The main conversation is the absence of an active tab
//!
//! `active_tab_id == None` *is* the main conversation. That is the direct
//! analogue of `ProjectTabs::close_fallback_tab`: projects must always have
//! some tab to render, so removing the last one mints a fresh empty compat tab.
//! The main buffer already has a surface, so the fallback is to stop selecting
//! a subagent. Closing the last subagent tab therefore needs no special case and
//! cannot leave the renderer with nothing to draw.

use std::collections::HashMap;

use crate::tui::app::state::subagents::SubagentRow;

/// Ceiling on simultaneously open subagent tabs.
///
/// The per-turn agent tree caps at 16 nodes, but tabs outlive their subject by
/// design — a detached tab is a first-class state, not something evicted — so
/// the tab count is not bounded by anything the projection enforces. Without a
/// cap, a long session that fans out repeatedly would grow this map without
/// limit. At capacity the set **refuses** a new tab rather than evicting one:
/// silently closing a tab the user is reading is a worse failure than declining
/// the open, and the caller has a defined outcome to report.
pub const MAX_OPEN_SUBAGENT_TABS: usize = 16;

/// Label for the always-present main-conversation entry in the tab bar.
pub const MAIN_TAB_LABEL: &str = "Chat";

/// Columns between two entries in the tab bar.
///
/// Measured rather than assumed by [`bar_width`], so changing the separator
/// cannot silently desynchronise the width arithmetic from what is drawn.
pub const BAR_SEPARATOR: &str = " │ ";

/// Placeholder standing in for tabs dropped to fit the bar's width.
pub const BAR_ELLIPSIS: &str = "…";

/// Identity of one open subagent tab.
///
/// See the module docs: this is the projection's `task_id`, and it is valid
/// only for this process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubagentTabId(String);

impl SubagentTabId {
    /// Wrap a projection `task_id`.
    ///
    /// No validation happens here — see [`SubagentTabs::open_or_focus`], which
    /// is the only constructor path and is where a blank id is refused.
    pub fn new(task_id: impl Into<String>) -> Self {
        Self(task_id.into())
    }

    /// Borrow the underlying `task_id`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SubagentTabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a tab's subject is still in the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentTabLiveness {
    /// The subject is present in the current snapshot.
    Attached,
    /// The subject is gone: its turn ended, the per-turn tree was cleared, or
    /// the per-turn node cap evicted it. The tab keeps rendering its last
    /// known row so the user can still read what the run was doing, but it
    /// makes no live claim.
    Detached,
}

impl SubagentTabLiveness {
    /// Whether the subject is still in the projection.
    pub fn is_attached(self) -> bool {
        matches!(self, SubagentTabLiveness::Attached)
    }
}

/// One open subagent tab: its identity, its last known row, and its scroll.
///
/// Scroll lives *inside* the tab record rather than beside it. That is what
/// makes "a closed tab cannot resurrect scroll" true by construction: closing
/// removes the whole record, so reopening the same subject starts at the top
/// instead of inheriting where the user had scrolled in a previous visit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTabState {
    /// Stable identity for this tab.
    pub tab_id: SubagentTabId,
    /// The row as last known. Retained after detachment so a tab outliving
    /// its subject still shows what the run was.
    pub row: SubagentRow,
    /// Whether the subject is still in the projection.
    pub liveness: SubagentTabLiveness,
    /// Scroll offset within the rendered view, in rows.
    pub scroll: u16,
}

impl SubagentTabState {
    /// Build an attached tab for `row`.
    pub fn attached(tab_id: SubagentTabId, row: SubagentRow) -> Self {
        Self {
            tab_id,
            row,
            liveness: SubagentTabLiveness::Attached,
            scroll: 0,
        }
    }

    /// What to call this subagent in the UI. Delegates to the row so the tab
    /// strip, the overlay, and the buffer all name a subagent identically.
    pub fn label(&self) -> &str {
        self.row.label()
    }

    /// Whether this run is still going.
    ///
    /// The liveness check is the whole point: a detached tab's captured row
    /// still says `running` because that is what the projection last reported,
    /// so reading `row.is_running()` alone would keep claiming live work that
    /// the projection has stopped vouching for.
    pub fn is_running(&self) -> bool {
        self.liveness.is_attached() && self.row.is_running()
    }

    /// Status glyph for the tab bar.
    ///
    /// `●` live, `○` finished but still in the tree, `✕` gone from the tree.
    pub fn glyph(&self) -> &'static str {
        match self.liveness {
            SubagentTabLiveness::Detached => "✕",
            SubagentTabLiveness::Attached if self.row.is_running() => "●",
            SubagentTabLiveness::Attached => "○",
        }
    }
}

/// Ordered collection of open subagent tabs with at most one designated
/// active tab.
///
/// Invariants held after every mutation:
///
/// - `active_tab_id` is `None` (the main conversation) or resolves to a tab
///   that is in `tabs`.
/// - `order` contains no id that is missing from `tabs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentTabs {
    tabs: HashMap<SubagentTabId, SubagentTabState>,
    order: Vec<SubagentTabId>,
    active_tab_id: Option<SubagentTabId>,
}

impl SubagentTabs {
    /// Create an empty tab set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open subagent tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether no subagent tabs are open.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Whether the set has reached [`MAX_OPEN_SUBAGENT_TABS`].
    pub fn is_at_capacity(&self) -> bool {
        self.tabs.len() >= MAX_OPEN_SUBAGENT_TABS
    }

    /// The active tab id, or `None` when the main conversation is showing.
    pub fn active_tab_id(&self) -> Option<&SubagentTabId> {
        self.active_tab_id.as_ref()
    }

    /// The active tab, or `None` when the main conversation is showing.
    pub fn active(&self) -> Option<&SubagentTabState> {
        let id = self.active_tab_id.as_ref()?;
        self.tabs.get(id)
    }

    /// Mutable accessor for the active tab.
    pub fn active_mut(&mut self) -> Option<&mut SubagentTabState> {
        let id = self.active_tab_id.clone()?;
        self.tabs.get_mut(&id)
    }

    /// Lookup a tab by id.
    pub fn get(&self, tab_id: &SubagentTabId) -> Option<&SubagentTabState> {
        self.tabs.get(tab_id)
    }

    /// Whether a tab for this subject is already open.
    pub fn contains(&self, task_id: &str) -> bool {
        self.tabs.contains_key(&SubagentTabId::new(task_id))
    }

    /// All open tabs in display order.
    pub fn ordered(&self) -> Vec<&SubagentTabState> {
        self.order
            .iter()
            .filter_map(|id| self.tabs.get(id))
            .collect()
    }

    /// Whether any tab's subject has left the projection.
    pub fn has_detached(&self) -> bool {
        self.tabs.values().any(|tab| !tab.liveness.is_attached())
    }

    /// Make `tab_id` active. Returns `false` when the id is not open, leaving
    /// the active tab unchanged rather than falling back to an arbitrary one.
    pub fn set_active(&mut self, tab_id: &SubagentTabId) -> bool {
        if self.tabs.contains_key(tab_id) {
            self.active_tab_id = Some(tab_id.clone());
            true
        } else {
            false
        }
    }

    /// Remove a tab by id.
    ///
    /// When the removed tab was active the selection falls back to the
    /// adjacent previous tab, or to the new first when the removed tab was
    /// first — the same rule `ProjectTabs::remove_tab` uses. When nothing
    /// remains, the active id becomes `None`, which *is* the main
    /// conversation: this is the close-fallback analogue of
    /// `ProjectTabs::close_fallback_tab`, reached by having a real surface to
    /// fall back to rather than by minting an empty placeholder tab.
    pub fn remove_tab(&mut self, tab_id: &SubagentTabId) -> Option<SubagentTabState> {
        let removed = self.tabs.remove(tab_id);
        if removed.is_some() {
            let prev_index = self.order.iter().position(|id| id == tab_id);
            self.order.retain(|id| id != tab_id);
            if self.active_tab_id.as_ref() == Some(tab_id) {
                self.active_tab_id = prev_index
                    .and_then(|index| {
                        if index > 0 {
                            self.order.get(index - 1)
                        } else {
                            self.order.first()
                        }
                    })
                    .cloned();
            }
        }
        removed
    }

    /// Close the active tab, returning what was removed.
    pub fn close_active(&mut self) -> Option<SubagentTabState> {
        let id = self.active_tab_id.clone()?;
        self.remove_tab(&id)
    }

    /// Open `row` as a tab, or focus the existing tab for the same subject.
    ///
    /// Idempotent by design: the same subagent opened from the overlay and
    /// from the panel is one tab that gains focus, not two tabs that fight
    /// over it. An existing tab is also refreshed from the incoming row, so
    /// reopening one that went detached and came back immediately repairs it.
    ///
    /// Returns `None` in exactly two cases: the row has a blank `task_id`,
    /// which cannot key a tab, or the set is at capacity. Neither is an error
    /// — the caller decides whether a refusal is worth a toast.
    pub fn open_or_focus(&mut self, row: SubagentRow) -> Option<SubagentTabId> {
        let task_id = row.task_id.trim();
        if task_id.is_empty() {
            return None;
        }
        let id = SubagentTabId::new(task_id.to_string());

        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.row = row;
            tab.liveness = SubagentTabLiveness::Attached;
            self.active_tab_id = Some(id.clone());
            return Some(id);
        }

        if self.is_at_capacity() {
            return None;
        }

        let tab = SubagentTabState::attached(id.clone(), row);
        self.tabs.insert(id.clone(), tab);
        self.order.push(id.clone());
        self.active_tab_id = Some(id.clone());
        Some(id)
    }

    /// Move `offset` places through the tab cycle and return the new active tab.
    ///
    /// Cycle order is stable — the display order in `order`, plus the main
    /// conversation as position 0 — and wraps in both directions.
    ///
    /// Modelling the main conversation as an explicit *position* rather than as
    /// "no active tab" is what makes the wrap correct. Treating it as position
    /// −1 and taking a remainder would send "previous" from the main buffer to
    /// the second tab instead of the last; position 0 with an explicit
    /// wraparound puts it on the last tab, which is what a user stepping
    /// backwards out of the transcript expects.
    ///
    /// Returns `None` when there are no subagent tabs: with nothing to cycle
    /// to, the main conversation stays selected.
    pub fn cycle(&mut self, offset: isize) -> Option<&SubagentTabState> {
        let len = self.order.len() as isize;
        if len == 0 {
            return None;
        }
        let current = self
            .active_tab_id
            .as_ref()
            .and_then(|id| self.order.iter().position(|candidate| candidate == id))
            .map(|index| index as isize + 1)
            .unwrap_or(0);
        let next = (current + offset).rem_euclid(len + 1);
        self.active_tab_id = match next {
            0 => None,
            position => Some(self.order[position as usize - 1].clone()),
        };
        self.active()
    }

    /// Reconcile every tab against a freshly arrived snapshot.
    ///
    /// A tab whose subject is present is refreshed and re-attached. A tab whose
    /// subject is absent is marked [`SubagentTabLiveness::Detached`] and keeps
    /// its captured row: the turn that produced it has ended, and that row is
    /// the only remaining record of what the run was doing.
    ///
    /// Called only when a snapshot *arrives*. A missing snapshot is not
    /// evidence that a subject is gone — it means the projection stream has
    /// not spoken — so the caller must not route that case here.
    pub fn sync_with_rows(&mut self, rows: &[SubagentRow]) {
        for tab in self.tabs.values_mut() {
            match rows.iter().find(|row| row.task_id == tab.tab_id.as_str()) {
                Some(row) => {
                    tab.row = row.clone();
                    tab.liveness = SubagentTabLiveness::Attached;
                }
                None => {
                    tab.liveness = SubagentTabLiveness::Detached;
                }
            }
        }
    }

    /// The tab bar's entries in display order, main conversation first.
    pub fn bar_tabs(&self) -> Vec<SubagentBarTab> {
        let mut out = vec![SubagentBarTab {
            label: MAIN_TAB_LABEL.to_string(),
            main: true,
            active: self.active_tab_id.is_none(),
            glyph: "",
            ellipsis: false,
        }];
        for tab in self.ordered() {
            out.push(SubagentBarTab {
                label: tab.label().to_string(),
                main: false,
                active: self.active_tab_id.as_ref() == Some(&tab.tab_id),
                glyph: tab.glyph(),
                ellipsis: false,
            });
        }
        out
    }
}

/// One entry in the one-row buffer tab bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentBarTab {
    /// Text to draw for this entry.
    pub label: String,
    /// The always-leftmost main-conversation entry.
    pub main: bool,
    /// Whether this is the entry currently displayed.
    pub active: bool,
    /// Status glyph, empty for the main conversation.
    pub glyph: &'static str,
    /// Set on the placeholder standing in for dropped tabs.
    pub ellipsis: bool,
}

impl SubagentBarTab {
    /// Columns this entry's own text occupies, excluding separators.
    ///
    /// **Display** width, not `chars().count()`. The pool is curated today, but
    /// a display name is bounded by *bytes* upstream
    /// (`MAX_PROJECTION_DISPLAY_NAME_BYTES`), not by character count, so a
    /// wide-glyph name is a live input rather than a hypothetical one. Counting
    /// characters would under-measure it, the budget would overshoot, and the
    /// bar would paint into the next column.
    pub fn label_width(&self) -> usize {
        unicode_width::UnicodeWidthStr::width(self.label.as_str())
    }
}

/// Columns the bar occupies if these entries were drawn in order.
pub fn bar_width(entries: &[SubagentBarTab]) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let labels: usize = entries.iter().map(SubagentBarTab::label_width).sum();
    labels + unicode_width::UnicodeWidthStr::width(BAR_SEPARATOR) * (entries.len() - 1)
}

/// Truncate `text` to at most `width` display columns, with no ellipsis.
///
/// The same budget arithmetic as `crate::tui::wrap::ellipsize_to_width` but
/// without the ellipsis: this is the degenerate clamp for a bar narrower than
/// its own entries, where a trailing `…` would cost the one column the entry
/// needed to be visible at all.
fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if unicode_width::UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

/// Assemble the surviving entries, inserting a placeholder wherever subagent
/// tabs were dropped. Without it a trimmed bar would silently read as "these
/// are all your subagents" rather than "these are the ones that fitted".
fn assemble(tabs: &[SubagentBarTab], chosen: &[usize]) -> Vec<SubagentBarTab> {
    let mut out: Vec<SubagentBarTab> = Vec::with_capacity(chosen.len() + 1);
    for (position, &index) in chosen.iter().enumerate() {
        if position > 0 {
            let dropped = (chosen[position - 1] + 1)..index;
            if dropped.clone().any(|skip| !tabs[skip].main) {
                out.push(SubagentBarTab {
                    label: BAR_ELLIPSIS.to_string(),
                    main: false,
                    active: false,
                    glyph: "",
                    ellipsis: true,
                });
            }
        }
        out.push(tabs[index].clone());
    }
    out
}

/// Fit the bar's entries into `width` columns.
///
/// Three guarantees, in priority order:
///
/// 1. **The active entry is always present.** A tab bar that hides the tab
///    currently on screen is worse than one that hides other tabs.
/// 2. **The main conversation is kept** whenever anything else survives, so
///    there is always a visible way back to the transcript.
/// 3. Anything dropped leaves an ellipsis behind.
///
/// The suffix is grown newest-first and stops at the first entry that does not
/// fit, so the kept set is always `main` plus a *contiguous* run of tabs and the
/// ellipsis is never misleading about what it stands for. If the active tab
/// falls outside that run it gets the whole budget instead.
pub fn fit_bar_tabs(tabs: Vec<SubagentBarTab>, width: usize) -> Vec<SubagentBarTab> {
    if width == 0 || tabs.is_empty() {
        return Vec::new();
    }
    if bar_width(&tabs) <= width {
        return tabs;
    }

    let main_index = tabs.iter().position(|tab| tab.main).unwrap_or(0);
    let active_index = tabs.iter().position(|tab| tab.active);

    let mut chosen: Vec<usize> = vec![main_index];
    for index in (0..tabs.len()).rev().filter(|index| *index != main_index) {
        let mut trial = chosen.clone();
        trial.push(index);
        trial.sort_unstable();
        if bar_width(&assemble(&tabs, &trial)) <= width {
            chosen = trial;
        } else {
            break;
        }
    }

    if let Some(active) = active_index {
        if active != main_index && !chosen.contains(&active) {
            chosen = vec![main_index, active];
            if bar_width(&assemble(&tabs, &chosen)) > width {
                chosen = vec![active];
            }
        }
    }

    let mut entries = assemble(&tabs, &chosen);

    // Degenerate clamp: the bar is narrower than a single entry. Shrink labels
    // in place rather than letting the strip overflow its rect. Ellipses are
    // skipped so the "something was dropped" signal survives; if only the
    // ellipsis is over budget it stays, because it is one column of a bar that
    // has nothing left to describe.
    let mut overflow = bar_width(&entries).saturating_sub(width);
    for entry in entries.iter_mut().rev() {
        if overflow == 0 {
            break;
        }
        if entry.ellipsis {
            continue;
        }
        let current = entry.label_width();
        if current == 0 {
            continue;
        }
        entry.label = truncate_to_width(&entry.label, current.saturating_sub(overflow));
        overflow = overflow.saturating_sub(current);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(task_id: &str, status: &str) -> SubagentRow {
        SubagentRow {
            task_id: task_id.into(),
            run_id: None,
            display_name: Some(format!("name-{task_id}")),
            agent: "explore".into(),
            description: "work".into(),
            profile: None,
            usefulness: None,
            status: status.into(),
        }
    }

    fn bar(label: &str, main: bool, active: bool) -> SubagentBarTab {
        SubagentBarTab {
            label: label.into(),
            main,
            active,
            glyph: "",
            ellipsis: false,
        }
    }

    #[test]
    fn opening_a_row_makes_it_the_active_tab() {
        let mut tabs = SubagentTabs::new();
        assert!(tabs.is_empty());
        let id = tabs.open_or_focus(row("1", "running")).expect("opens");
        assert_eq!(tabs.active_tab_id(), Some(&id));
        assert_eq!(tabs.active().map(SubagentTabState::label), Some("name-1"));
    }

    #[test]
    fn reopening_the_same_subject_focuses_the_one_existing_tab() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.open_or_focus(row("2", "running"));
        assert_eq!(tabs.len(), 2);

        // Opening 1 again must not duplicate it.
        tabs.open_or_focus(row("1", "running"));
        assert_eq!(tabs.len(), 2, "the subject already has a tab");
        assert_eq!(tabs.active().map(|tab| tab.tab_id.as_str()), Some("1"));
    }

    #[test]
    fn a_blank_task_id_cannot_key_a_tab() {
        let mut tabs = SubagentTabs::new();
        // A tab id is the projection's task_id; a blank one would collide
        // with every other blank row, so it is refused rather than keyed.
        assert!(tabs.open_or_focus(row("   ", "running")).is_none());
        assert!(tabs.is_empty());
    }

    #[test]
    fn the_set_refuses_a_new_tab_at_capacity_rather_than_evicting_one() {
        let mut tabs = SubagentTabs::new();
        for index in 0..MAX_OPEN_SUBAGENT_TABS {
            assert!(tabs
                .open_or_focus(row(&index.to_string(), "running"))
                .is_some());
        }
        assert!(tabs.is_at_capacity());
        assert_eq!(tabs.len(), MAX_OPEN_SUBAGENT_TABS);

        // Still full, and the open is declined rather than costing the user a
        // tab they were reading.
        assert!(tabs.open_or_focus(row("overflow", "running")).is_none());
        assert_eq!(tabs.len(), MAX_OPEN_SUBAGENT_TABS);
        // Focusing an existing tab still works at capacity.
        assert!(tabs
            .open_or_focus(row("0", "running"))
            .is_some_and(|id| tabs.active_tab_id() == Some(&id)));
    }

    #[test]
    fn cycling_forward_from_the_main_buffer_lands_on_the_first_tab() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.open_or_focus(row("2", "running"));

        // Wrap forward off the end to reach the main conversation. Deliberately
        // *not* `close_active`: that removes a tab, so it would leave a
        // one-tab set and quietly turn this into a different test.
        assert!(
            tabs.cycle(1).is_none(),
            "off the end wraps to the transcript"
        );
        assert!(tabs.active_tab_id().is_none());
        assert_eq!(tabs.cycle(1).map(|tab| tab.tab_id.as_str()), Some("1"));
        assert_eq!(tabs.cycle(1).map(|tab| tab.tab_id.as_str()), Some("2"));
        assert!(
            tabs.cycle(1).is_none(),
            "and wraps back to the main conversation"
        );
    }

    #[test]
    fn cycling_backward_from_the_main_buffer_lands_on_the_last_tab() {
        // The wrap that a naive "main is position -1" remainder gets wrong:
        // stepping back out of the transcript must reach the *last* subagent.
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.open_or_focus(row("2", "running"));
        tabs.open_or_focus(row("3", "running"));
        assert!(tabs.cycle(1).is_none(), "onto the main conversation");

        assert_eq!(tabs.cycle(-1).map(|tab| tab.tab_id.as_str()), Some("3"));
        assert_eq!(tabs.cycle(-1).map(|tab| tab.tab_id.as_str()), Some("2"));
        assert_eq!(tabs.cycle(-1).map(|tab| tab.tab_id.as_str()), Some("1"));
        assert!(tabs.cycle(-1).is_none(), "wraps forward to the main buffer");
    }

    #[test]
    fn cycling_with_no_tabs_leaves_the_main_buffer_selected() {
        let mut tabs = SubagentTabs::new();
        assert!(tabs.cycle(1).is_none());
        assert!(tabs.cycle(-1).is_none());
        assert!(tabs.active_tab_id().is_none());
    }

    #[test]
    fn cycling_with_a_single_tab_alternates_between_it_and_the_main_buffer() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        assert!(tabs.cycle(1).is_none(), "on to the main buffer");
        assert_eq!(
            tabs.cycle(1).map(|tab| tab.tab_id.as_str()),
            Some("1"),
            "and back again"
        );
        assert!(tabs.cycle(1).is_none());
    }

    #[test]
    fn closing_the_active_tab_falls_back_to_its_neighbour() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.open_or_focus(row("2", "running"));
        tabs.open_or_focus(row("3", "running"));

        tabs.set_active(&SubagentTabId::new("2"));
        tabs.close_active();
        // Adjacent previous, exactly as projects do.
        assert_eq!(tabs.active_tab_id().map(|id| id.as_str()), Some("1"));

        tabs.set_active(&SubagentTabId::new("1"));
        tabs.close_active();
        // The removed tab was first, so the new first wins.
        assert_eq!(tabs.active_tab_id().map(|id| id.as_str()), Some("3"));
    }

    #[test]
    fn closing_the_last_tab_falls_back_to_the_main_conversation() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        assert_eq!(
            tabs.close_active().map(|tab| tab.row.task_id),
            Some("1".into())
        );
        assert!(tabs.is_empty());
        // No placeholder tab is minted: the main buffer already exists, so
        // "no active subagent tab" *is* the fallback surface.
        assert!(tabs.active_tab_id().is_none());
        assert!(tabs.active().is_none());
    }

    #[test]
    fn closing_an_inactive_tab_leaves_the_selection_alone() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.open_or_focus(row("2", "running"));
        tabs.set_active(&SubagentTabId::new("2"));
        tabs.remove_tab(&SubagentTabId::new("1"));
        assert_eq!(tabs.active_tab_id().map(|id| id.as_str()), Some("2"));
    }

    #[test]
    fn a_closed_tab_cannot_resurrect_its_scroll() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.active_mut().expect("open").scroll = 42;

        tabs.close_active();
        assert!(tabs.is_empty());

        // Reopening the same subject starts at the top rather than
        // inheriting the previous visit's scroll.
        tabs.open_or_focus(row("1", "running"));
        assert_eq!(tabs.active().map(|tab| tab.scroll), Some(0));
    }

    #[test]
    fn a_subject_that_leaves_the_snapshot_detaches_instead_of_claiming_to_run() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        assert!(tabs.active().expect("open").is_running());

        // The turn ended and the per-turn tree was cleared.
        tabs.sync_with_rows(&[]);

        let tab = tabs.active().expect("the tab survives its subject");
        assert_eq!(tab.liveness, SubagentTabLiveness::Detached);
        assert!(
            !tab.is_running(),
            "a detached tab must not keep claiming live work"
        );
        // The row it was reading is still there, which is the whole reason
        // detaching instead of removing.
        assert_eq!(tab.row.task_id, "1");
        assert!(tabs.has_detached());
    }

    #[test]
    fn a_detached_tab_that_returns_is_refreshed_and_re_attached() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.sync_with_rows(&[]);
        assert!(!tabs.active().expect("open").is_running());

        let mut back = row("1", "running");
        back.description = "resumed".into();
        tabs.sync_with_rows(&[back]);

        let tab = tabs.active().expect("open");
        assert_eq!(tab.liveness, SubagentTabLiveness::Attached);
        assert!(tab.is_running());
        assert_eq!(tab.row.description, "resumed", "the row is refreshed");
        assert!(!tabs.has_detached());
    }

    #[test]
    fn a_still_running_row_in_a_detached_tab_reads_as_detached_on_the_bar() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        tabs.sync_with_rows(&[]);
        assert_eq!(tabs.active().expect("open").glyph(), "✕");
    }

    #[test]
    fn bar_glyphs_distinguish_live_finished_and_gone() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        assert_eq!(tabs.active().expect("open").glyph(), "●");

        tabs.open_or_focus(row("2", "completed"));
        assert_eq!(tabs.active().expect("open").glyph(), "○");

        tabs.sync_with_rows(&[]);
        assert_eq!(tabs.active().expect("open").glyph(), "✕");
    }

    #[test]
    fn the_bar_always_offers_the_main_conversation_first() {
        let mut tabs = SubagentTabs::new();
        tabs.open_or_focus(row("1", "running"));
        let entries = tabs.bar_tabs();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].main);
        assert_eq!(entries[0].label, MAIN_TAB_LABEL);
        assert!(!entries[0].active, "a subagent tab is showing");

        tabs.close_active();
        assert!(tabs.bar_tabs()[0].active, "back to the transcript");
    }

    #[test]
    fn a_bar_that_fits_is_returned_untouched() {
        let entries = vec![bar("Chat", true, false), bar("Sally", false, true)];
        assert_eq!(fit_bar_tabs(entries.clone(), 80), entries);
    }

    #[test]
    fn a_zero_width_bar_renders_nothing() {
        let entries = vec![bar("Chat", true, true)];
        assert!(fit_bar_tabs(entries, 0).is_empty());
    }

    #[test]
    fn trimming_keeps_the_active_tab_and_marks_what_it_dropped() {
        let entries = vec![
            bar("Chat", true, false),
            bar("Sally", false, false),
            bar("Bertha", false, false),
            bar("Cleo", false, false),
            bar("Dara", false, true),
        ];
        let fitted = fit_bar_tabs(entries, 20);
        let total = bar_width(&fitted);
        assert!(total <= 20, "bar grew to {total} cols");
        assert!(
            fitted.iter().any(|entry| entry.active),
            "the tab on screen must survive the trim"
        );
        assert!(fitted[0].main, "the way back to the transcript survives");
        assert!(
            fitted.iter().any(|entry| entry.ellipsis),
            "dropped tabs leave a placeholder"
        );
    }

    #[test]
    fn trimming_never_overflows_even_when_the_bar_is_tiny() {
        let entries = vec![
            bar("Chat", true, false),
            bar("Sally", false, false),
            bar("Bertha", false, true),
        ];
        for width in 0..=24usize {
            let fitted = fit_bar_tabs(entries.clone(), width);
            assert!(
                bar_width(&fitted) <= width,
                "width {width} produced {} cols",
                bar_width(&fitted)
            );
        }
    }

    #[test]
    fn wide_glyph_labels_are_measured_in_columns_not_characters() {
        // A display name is bounded upstream by *bytes*, so a CJK name is a
        // live input. Counting characters would measure these labels at half
        // their real width and the bar would paint into the next column.
        let entries = vec![
            bar("Chat", true, false),
            bar("敏晴敏晴", false, false),
            bar("敏晴敏晴", false, true),
        ];
        assert_eq!(
            bar_width(&entries),
            "Chat".len() + 3 + 8 + 3 + 8,
            "a CJK label is two columns per character"
        );
        for width in 0..=30usize {
            let fitted = fit_bar_tabs(entries.clone(), width);
            assert!(
                bar_width(&fitted) <= width,
                "width {width} produced {} cols: {fitted:?}",
                bar_width(&fitted)
            );
        }
    }

    #[test]
    fn a_bar_narrower_than_its_active_tab_truncates_rather_than_dropping_it() {
        // Five cols cannot hold "Chat │ Sally"; the active tab is what the
        // user is reading, so it is shortened to fit rather than removed.
        let entries = vec![bar("Chat", true, false), bar("Sally", false, true)];
        let fitted = fit_bar_tabs(entries, 5);
        assert!(bar_width(&fitted) <= 5);
        assert!(
            fitted.iter().any(|entry| entry.active),
            "the active tab stays visible in some form"
        );
    }
}
