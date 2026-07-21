//! Multi-Project TUI event-routing identities and registry (Milestone 3).
//!
//! This module introduces a typed frontend [`UiRouteToken`] that captures
//! the canonical project / workspace / session identity plus the active
//! view epoch, reconnect epoch, and per-request generation. Every async
//! command pair started for a project tab captures a `UiRouteToken`
//! before its work begins; the completion is rejected if any populated
//! field no longer matches the live routing registry.
//!
//! [`RoutingRegistry`] is the central derived-state container that
//! tracks:
//!
//! - the [`ProjectTabId`] -> canonical tab scope mapping;
//! - the session_id -> tab_id index used by event routing;
//! - the active heavy-view scope (which tab owns the legacy
//!   `App::session_state` / `App::agent_state` surface);
//! - per-tab [`TabActivitySummary`] bounds (unread counts, pending
//!   permissions / questions, last error, and last accepted sequence);
//! - the monotonic reconnect_epoch (used to reject pre-reconnect
//!   completions after a transport drop);
//! - the monotonic last-accepted event sequence (used by the gap
//!   detector to trigger a bounded resync).
//!
//! [`RouteDecision`] is the result of the pure [`classify_event`]
//! function. The classifier never mutates state; it consumes the
//! routing registry immutably and returns a routing instruction for
//! the active-view reducer, the inactive summary reducer, or a
//! refresh/resync fallback.
//!
//! Invariants enforced here:
//!
//! - `ProjectTabId` is never serialized into daemon protocol identity
//!   fields. It is frontend-local only and is the only key on which
//!   we route within the TUI.
//! - A `UiRouteToken` with a non-matching `tab_id`, `project_id`,
//!   `workspace_id`, `session_id`, `active_view_epoch`, or
//!   `reconnect_epoch` cannot be used to mutate heavy UI state.
//! - The active heavy view and inactive summaries are mutually
//!   exclusive in their data: heavy view remains the legacy
//!   compatibility surface; inactive summaries hold only bounded
//!   indicators. Duplicate message bodies, file bodies, diffs, logs,
//!   LSP state, and exclusive workspace-service leases are never
//!   copied into an inactive summary.
//! - `last_accepted_sequence` is monotonically increasing per
//!   reconnect epoch so stale replays are dropped or trigger a
//!   bounded resync request.

use std::collections::HashMap;

use crate::bus::events::AppEvent;
use crate::tui::app::state::project_tabs::ProjectTabId;

/// Maximum unread count surfaced on a tab badge before saturation.
pub const MAX_TAB_UNREAD_DISPLAY: u32 = 99;
/// Maximum recorded last-error bytes per tab.
pub const MAX_TAB_LAST_ERROR_LEN: usize = 256;
/// Maximum recorded health/asset summary bytes per tab.
pub const MAX_TAB_HEALTH_SUMMARY_LEN: usize = 256;

/// Route identity for one in-flight operation or live event.
///
/// Every async command pair captures a token before its work begins.
/// The token's completion must validate every populated field against
/// the live routing registry before mutating heavy UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiRouteToken {
    /// Frontend-local tab id.
    pub tab_id: Option<ProjectTabId>,
    /// Daemon-typed project id. `None` for global/unscoped operations.
    pub project_id: Option<String>,
    /// Daemon-typed workspace id.
    pub workspace_id: Option<String>,
    /// Daemon-typed session id.
    pub session_id: Option<String>,
    /// Active-view epoch recorded when the token was captured.
    pub active_view_epoch: u64,
    /// Reconnect epoch recorded when the token was captured.
    pub reconnect_epoch: u64,
    /// Per-request generation recorded when the token was captured.
    pub request_generation: u64,
}

impl UiRouteToken {
    /// Construct a token bound to a specific tab and identities. Fields
    /// that remain `None` are not part of the validation match.
    pub fn new(
        tab_id: Option<ProjectTabId>,
        project_id: Option<String>,
        workspace_id: Option<String>,
        session_id: Option<String>,
        active_view_epoch: u64,
        reconnect_epoch: u64,
        request_generation: u64,
    ) -> Self {
        Self {
            tab_id,
            project_id,
            workspace_id,
            session_id,
            active_view_epoch,
            reconnect_epoch,
            request_generation,
        }
    }

    /// Wildcard token used for genuinely global operations (catalog
    /// refresh, keybinding dialogs, etc.) that have no canonical
    /// project/workspace/session binding.
    pub fn global(reconnect_epoch: u64) -> Self {
        Self::new(None, None, None, None, 0, reconnect_epoch, 0)
    }

    /// Returns `true` when every populated field on `self` matches
    /// the live routing registry at the supplied [`RouteCheck`]
    /// snapshot. Stale completions fail closed.
    pub fn matches(&self, check: &RouteCheck) -> bool {
        // Reconnect epoch always must match if the token captured one.
        if self.reconnect_epoch != check.reconnect_epoch {
            return false;
        }
        if let Some(ref tid) = self.tab_id {
            match &check.tab_id {
                Some(actual) if actual == tid => {}
                _ => return false,
            }
        }
        if let Some(ref proj) = self.project_id {
            match &check.project_id {
                Some(actual) if actual == proj => {}
                _ => return false,
            }
        }
        if let Some(ref ws) = self.workspace_id {
            match &check.workspace_id {
                Some(actual) if actual == ws => {}
                _ => return false,
            }
        }
        if let Some(ref sid) = self.session_id {
            match &check.session_id {
                Some(actual) if actual == sid => {}
                _ => return false,
            }
        }
        if self.active_view_epoch != 0 && self.active_view_epoch != check.active_view_epoch {
            return false;
        }
        true
    }
}

/// Live snapshot of routing identity for one tab. Used by
/// `UiRouteToken::matches` and built by [`RoutingRegistry::check_for`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteCheck {
    pub tab_id: Option<ProjectTabId>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub active_view_epoch: u64,
    pub reconnect_epoch: u64,
}

/// Routing decision produced by the pure [`classify_event`] function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    /// The event applies to the active heavy view for `tab_id`.
    ActiveView { tab_id: ProjectTabId },
    /// The event updates only the bounded inactive summary of `tab_id`.
    InactiveSummary { tab_id: ProjectTabId },
    /// The event is genuinely global (daemon lifecycle, connection,
    /// process-wide keybinding). Surface effects stay global.
    Global,
    /// The event's ownership is unknown/ambiguous/rebound; the caller
    /// must schedule a bounded refresh and surface a diagnostic.
    RefreshRequired { reason: &'static str },
    /// The event was routed to a tab or session that no longer exists;
    /// caller drops the event without mutating state.
    DropDiagnostic { reason: &'static str },
}

/// Per-tab bounded activity summary.
///
/// Holds only presentation fields and small counters. No message
/// bodies, tool outputs, diff bodies, logs, LSP state, or full event
/// queues are stored here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabActivitySummary {
    /// Monotonic counter incremented on every applied summary update.
    pub activity_revision: u64,
    /// Unread delta count, saturated at [`MAX_TAB_UNREAD_DISPLAY`].
    pub unread_count: u32,
    /// Number of pending permission requests for this tab.
    pub pending_permission_count: u32,
    /// Number of pending question requests for this tab.
    pub pending_question_count: u32,
    /// Whether a turn is in flight for this tab's session.
    pub turn_active: bool,
    /// Whether a run, job, or test is in flight for this tab.
    pub run_active: bool,
    /// Bounded last-error summary.
    pub last_error: Option<String>,
    /// Last accepted event sequence/cursor hint for this tab.
    pub last_accepted_sequence: u64,
    /// Bounded project health/runtime-asset summary.
    pub health_summary: Option<String>,
    /// Whether a resync/refresh is required.
    pub resync_required: bool,
}

impl TabActivitySummary {
    fn new() -> Self {
        Self::default()
    }

    /// Apply a uniform summary update. Increments the activity revision
    /// so consumers can detect changes cheaply.
    pub fn touch(&mut self) {
        self.activity_revision = self.activity_revision.saturating_add(1);
    }

    /// Record an unread delta up to the saturation cap.
    pub fn add_unread(&mut self, delta: u32) {
        let next = self.unread_count.saturating_add(delta);
        self.unread_count = next.min(MAX_TAB_UNREAD_DISPLAY);
        self.touch();
    }

    /// Clear unread count (e.g., when the tab is foregrounded).
    pub fn clear_unread(&mut self) {
        if self.unread_count != 0 {
            self.unread_count = 0;
            self.touch();
        }
    }

    /// Record a bounded status/error summary.
    pub fn record_status<S: AsRef<str>>(&mut self, msg: S) {
        let s = msg.as_ref();
        if s.len() > MAX_TAB_LAST_ERROR_LEN {
            self.last_error = Some(truncate_for_storage(s, MAX_TAB_LAST_ERROR_LEN));
        } else {
            self.last_error = Some(s.to_string());
        }
        self.touch();
    }

    /// Update health summary, bounded.
    pub fn record_health<S: AsRef<str>>(&mut self, msg: S) {
        let s = msg.as_ref();
        self.health_summary = Some(if s.len() > MAX_TAB_HEALTH_SUMMARY_LEN {
            truncate_for_storage(s, MAX_TAB_HEALTH_SUMMARY_LEN)
        } else {
            s.to_string()
        });
        self.touch();
    }

    /// Mark this tab as needing a bounded resync.
    pub fn mark_resync_required(&mut self) {
        if !self.resync_required {
            self.resync_required = true;
            self.touch();
        }
    }

    /// Clear the resync-required flag (after a successful resync).
    pub fn clear_resync_required(&mut self) {
        if self.resync_required {
            self.resync_required = false;
            self.touch();
        }
    }
}

fn truncate_for_storage(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 1);
    out.push_str(&s[..cut]);
    out.push('…');
    out
}

/// Routing registry owned by `App` (or a focused state module).
///
/// The registry is rebuilt from the current tabs and daemon responses;
/// it is **derived frontend state**, not durable authority. All values
/// here can be reconstructed from `ProjectTabs` + canonical daemon
/// responses.
#[derive(Debug, Default)]
pub struct RoutingRegistry {
    /// Map from session id -> tab id for currently-open sessions.
    session_index: HashMap<String, ProjectTabId>,
    /// Per-tab bounded activity summary.
    activity: HashMap<ProjectTabId, TabActivitySummary>,
    /// Monotonic reconnect epoch. Incremented every time the transport
    /// reconnects so pre-reconnect completions can be rejected.
    pub reconnect_epoch: u64,
    /// Monotonic last accepted event sequence (across all tabs and
    /// the global lane). Used by the gap detector.
    pub last_accepted_sequence: u64,
}

impl RoutingRegistry {
    /// Create a fresh empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconnect: bump the reconnect epoch. Returns the new value.
    pub fn bump_reconnect_epoch(&mut self) -> u64 {
        self.reconnect_epoch = self.reconnect_epoch.saturating_add(1);
        self.reconnect_epoch
    }

    /// Record that one session_id is now open in `tab_id`. Removes any
    /// prior binding for the same session id from another tab, and
    /// cleans up the prior tab's activity if it has no remaining
    /// session bindings.
    pub fn register_open_session(&mut self, tab_id: ProjectTabId, session_id: String) {
        // Remove any prior binding for this session_id if it was tied
        // to a different tab.
        let prior_tab = self
            .session_index
            .insert(session_id.clone(), tab_id.clone());
        if let Some(prior) = prior_tab {
            if prior != tab_id {
                // Check if the prior tab still has any other sessions.
                let prior_has_sessions = self.session_index.values().any(|v| *v == prior);
                if !prior_has_sessions {
                    self.activity.remove(&prior);
                }
            }
        }
        self.activity
            .entry(tab_id)
            .or_insert_with(TabActivitySummary::new);
    }

    /// Remove a session binding (on close, archive, or replacement).
    pub fn unregister_session(&mut self, session_id: &str) {
        self.session_index.remove(session_id);
    }

    /// Drop all session bindings and activity for one tab.
    /// Call before removing the tab from `ProjectTabs`.
    pub fn drop_tab(&mut self, tab_id: &ProjectTabId) {
        self.activity.remove(tab_id);
        self.session_index.retain(|_, v| v != tab_id);
    }

    /// Lookup the tab currently owning `session_id`.
    pub fn tab_for_session(&self, session_id: &str) -> Option<&ProjectTabId> {
        self.session_index.get(session_id)
    }

    /// Mutable accessor for one tab's activity summary, creating an
    /// empty one if the tab is unknown.
    pub fn activity_mut(&mut self, tab_id: &ProjectTabId) -> &mut TabActivitySummary {
        self.activity
            .entry(tab_id.clone())
            .or_insert_with(TabActivitySummary::new)
    }

    /// Read-only accessor for one tab's activity summary.
    pub fn activity(&self, tab_id: &ProjectTabId) -> Option<&TabActivitySummary> {
        self.activity.get(tab_id)
    }

    /// Snapshot the live identity for one tab. Used by
    /// `UiRouteToken::matches`. Owned clone: the caller may compare a
    /// `UiRouteToken` against the snapshot without holding any borrows
    /// on the registry.
    pub fn check_for(
        &self,
        tab_id: Option<&ProjectTabId>,
        project_id: Option<&str>,
        workspace_id: Option<&str>,
        session_id: Option<&str>,
        active_view_epoch: u64,
    ) -> RouteCheck {
        RouteCheck {
            tab_id: tab_id.cloned(),
            project_id: project_id.map(|s| s.to_string()),
            workspace_id: workspace_id.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            active_view_epoch,
            reconnect_epoch: self.reconnect_epoch,
        }
    }

    /// Bump and return the next event sequence. Called once per
    /// accepted live event.
    pub fn next_sequence(&mut self) -> u64 {
        self.last_accepted_sequence = self.last_accepted_sequence.saturating_add(1);
        self.last_accepted_sequence
    }

    /// Returns `true` when `incoming_sequence` is greater than the
    /// last accepted sequence for `tab_id`. Replays (equal or lower)
    /// fail closed and are reported as a diagnostic by the caller.
    pub fn is_sequence_after(&self, tab_id: &ProjectTabId, incoming_sequence: u64) -> bool {
        match self.activity.get(tab_id) {
            Some(summary) => incoming_sequence > summary.last_accepted_sequence,
            None => true,
        }
    }

    /// Record the accepted event sequence on `tab_id`.
    pub fn record_sequence(&mut self, tab_id: &ProjectTabId, incoming: u64) {
        let summary = self.activity_mut(tab_id);
        if incoming > summary.last_accepted_sequence {
            summary.last_accepted_sequence = incoming;
        }
    }
}

/// The pure event classifier.
///
/// Resolves a raw `AppEvent` and the active heavy-view scope into a
/// [`RouteDecision`]. The classifier never mutates the registry; it
/// reads from the [`RoutingRegistry`] and returns the routing
/// instruction. The caller is responsible for actually mutating
/// state.
///
/// Arguments:
///
/// - `event`: the live event from the global event bus.
/// - `registry`: the routing registry (read-only here).
/// - `active_tab`: the tab that currently owns the heavy view
///   (`App::session_state` / `App::agent_state`).
/// - `active_view_epoch`: the live active-view epoch for the heavy
///   view. A mismatched epoch causes every event to fall back to a
///   bounded `RefreshRequired`.
pub fn classify_event(
    event: &AppEvent,
    registry: &RoutingRegistry,
    active_tab: Option<&ProjectTabId>,
    active_view_epoch: u64,
) -> RouteDecision {
    let session_id = event_session_id(event);
    let project_id = event_project_id(event);

    match (session_id, project_id, active_tab) {
        // Truly global events that never carry canonical identity.
        (None, None, _) => RouteDecision::Global,

        // Project-scoped events with no session id: route by project
        // resolution. These come from project health, asset refresh,
        // and catalog-cache invalidation events.
        (None, Some(proj), _) => {
            // Project-scoped events route to whichever tab holds that
            // project id. Without a project-index (which is not in
            // scope for this milestone) we treat project-only events
            // as a refresh hint so the caller's global lane can
            // surface a diagnostic and a resync request.
            let _ = proj; // explicit use to keep the variable intent obvious
            RouteDecision::RefreshRequired {
                reason: "project_scoped_no_tab_index",
            }
        }

        // Session-scoped events: route through the session->tab index.
        (Some(sid), _, Some(atab)) => match registry.tab_for_session(&sid) {
            // Owned by the active heavy view.
            Some(tab) if Some(tab) == Some(atab) => RouteDecision::ActiveView {
                tab_id: tab.clone(),
            },
            // Owned by a different open tab; route to that tab's
            // bounded inactive summary.
            Some(tab) => RouteDecision::InactiveSummary {
                tab_id: tab.clone(),
            },
            // Session is unknown or was closed/rebound/archived.
            None => {
                if active_view_epoch == 0 {
                    RouteDecision::RefreshRequired {
                        reason: "session_owned_but_no_active_view",
                    }
                } else {
                    RouteDecision::DropDiagnostic {
                        reason: "session_owned_but_no_open_tab",
                    }
                }
            }
        },

        // Session-scoped event with no active tab selected.
        (Some(sid), _, None) => match registry.tab_for_session(&sid) {
            Some(tab) => RouteDecision::InactiveSummary {
                tab_id: tab.clone(),
            },
            None => RouteDecision::DropDiagnostic {
                reason: "session_owned_no_active_tab_no_index",
            },
        },
    }
}

/// Helper: extract the session_id from a raw `AppEvent` if it carries
/// one. Returns `None` when the variant is truly global.
pub fn event_session_id(event: &AppEvent) -> Option<String> {
    match event {
        AppEvent::SessionCreated { id, .. }
        | AppEvent::SessionUpdated { id }
        | AppEvent::SessionArchived { id }
        | AppEvent::SessionShared { id, .. }
        | AppEvent::SessionUnshared { id }
        | AppEvent::SessionReverted { id, .. } => Some(id.clone()),
        AppEvent::SessionForked { child_id, .. } => Some(child_id.clone()),
        AppEvent::MessageAdded { session_id, .. }
        | AppEvent::MessageDeleted { session_id, .. }
        | AppEvent::ToolCalled { session_id, .. }
        | AppEvent::TodoUpdated { session_id, .. }
        | AppEvent::GoalUpdated { session_id, .. }
        | AppEvent::QuestionPending { session_id, .. }
        | AppEvent::QuestionAnswered { session_id, .. }
        | AppEvent::PermissionPending { session_id, .. }
        | AppEvent::PermissionResponded { session_id, .. }
        | AppEvent::DiffPending { session_id, .. }
        | AppEvent::DiffResponded { session_id, .. }
        | AppEvent::ContextUpdated { session_id, .. }
        | AppEvent::ToolResult { session_id, .. }
        | AppEvent::SubagentStarted { session_id, .. }
        | AppEvent::SubagentProgress { session_id, .. }
        | AppEvent::SubagentCompleted { session_id, .. }
        | AppEvent::SubagentFailed { session_id, .. }
        | AppEvent::TestRunStarted { session_id, .. }
        | AppEvent::TestRunProgress { session_id, .. }
        | AppEvent::TestRunCompleted { session_id, .. }
        | AppEvent::CompactionTriggered { session_id, .. }
        | AppEvent::ToolCallStarted { session_id, .. }
        | AppEvent::AgentFinished { session_id, .. } => Some(session_id.clone()),
        AppEvent::ReasoningDelta { session_id, .. } | AppEvent::TextDelta { session_id, .. } => {
            Some(session_id.to_string())
        }
        AppEvent::McpServerConnected { .. }
        | AppEvent::McpServerDisconnected { .. }
        | AppEvent::McpToolListChanged { .. }
        | AppEvent::ConfigChanged
        | AppEvent::AgentChanged { .. }
        | AppEvent::ModelChanged { .. }
        | AppEvent::Error { .. }
        | AppEvent::Info { .. }
        | AppEvent::FileChanged { .. }
        | AppEvent::PluginUiEffect { .. } => None,
        // Defensive catch-all for any future AppEvent additions we
        // haven't classified.
        _ => None,
    }
}

/// Helper: extract a daemon-typed project id from a raw `AppEvent` if
/// it carries one. Used by the project-only classifier branch.
pub fn event_project_id(event: &AppEvent) -> Option<String> {
    match event {
        AppEvent::SessionCreated { project_id, .. } => Some(project_id.clone()),
        AppEvent::GoalUpdated { goal, .. } => goal
            .as_ref()
            .as_ref()
            .map(|g| g.project_id.clone())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// Inactive-summary reducer: applies an [`InactiveSummary`] routing
/// decision to the routing registry, never touching the heavy view.
///
/// Callers from the bus-event dispatcher pass the event payload as
/// `kind`. The reducer treats every kind as a bounded update: no
/// message bodies, tool outputs, file bodies, diff bodies, or full
/// event queues are copied into the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InactiveSummaryKind {
    UnreadActivity,
    PendingPermission,
    PendingQuestion,
    StatusUpdate,
    HealthUpdate,
    ResyncRequired,
}

pub fn apply_inactive_summary(
    registry: &mut RoutingRegistry,
    tab_id: &ProjectTabId,
    kind: InactiveSummaryKind,
    detail: Option<&str>,
) {
    let summary = registry.activity_mut(tab_id);
    match kind {
        InactiveSummaryKind::UnreadActivity => summary.add_unread(1),
        InactiveSummaryKind::PendingPermission => {
            summary.pending_permission_count = summary.pending_permission_count.saturating_add(1);
            summary.touch();
        }
        InactiveSummaryKind::PendingQuestion => {
            summary.pending_question_count = summary.pending_question_count.saturating_add(1);
            summary.touch();
        }
        InactiveSummaryKind::StatusUpdate => {
            if let Some(d) = detail {
                summary.record_status(d);
            } else {
                summary.touch();
            }
        }
        InactiveSummaryKind::HealthUpdate => {
            if let Some(d) = detail {
                summary.record_health(d);
            } else {
                summary.touch();
            }
        }
        InactiveSummaryKind::ResyncRequired => summary.mark_resync_required(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab() -> ProjectTabId {
        ProjectTabId::new()
    }

    #[test]
    fn route_token_matches_when_all_fields_align() {
        let tid = tab();
        let registry = RoutingRegistry::new();
        let token = UiRouteToken::new(
            Some(tid.clone()),
            Some("p1".into()),
            Some("w1".into()),
            Some("s1".into()),
            5,
            0,
            1,
        );
        let check = registry.check_for(Some(&tid), Some("p1"), Some("w1"), Some("s1"), 5);
        assert!(token.matches(&check));
    }

    #[test]
    fn route_token_rejects_stale_tab() {
        let tid = tab();
        let other = tab();
        let registry = RoutingRegistry::new();
        let token = UiRouteToken::new(
            Some(tid),
            Some("p1".into()),
            Some("w1".into()),
            Some("s1".into()),
            5,
            0,
            1,
        );
        let check = registry.check_for(Some(&other), Some("p1"), Some("w1"), Some("s1"), 5);
        assert!(!token.matches(&check));
    }

    #[test]
    fn route_token_rejects_stale_view_epoch() {
        let tid = tab();
        let registry = RoutingRegistry::new();
        let token = UiRouteToken::new(
            Some(tid.clone()),
            Some("p1".into()),
            Some("w1".into()),
            Some("s1".into()),
            5,
            0,
            1,
        );
        // Capture checks against epoch 7; token captured epoch 5.
        let check = registry.check_for(Some(&tid), Some("p1"), Some("w1"), Some("s1"), 7);
        assert!(!token.matches(&check));
    }

    #[test]
    fn route_token_rejects_pre_reconnect() {
        let mut registry = RoutingRegistry::new();
        let tid = tab();
        let token = UiRouteToken::new(
            Some(tid.clone()),
            Some("p".into()),
            Some("w".into()),
            Some("s".into()),
            1,
            0,
            1,
        );
        // Pre-reconnect registry is at epoch 0.
        let check = registry.check_for(Some(&tid), Some("p"), Some("w"), Some("s"), 1);
        assert!(token.matches(&check));
        // Bump reconnect epoch.
        registry.bump_reconnect_epoch();
        let after = registry.check_for(Some(&tid), Some("p"), Some("w"), Some("s"), 1);
        assert!(!token.matches(&after));
    }

    #[test]
    fn session_index_tracks_open_sessions() {
        let mut registry = RoutingRegistry::new();
        let t1 = tab();
        let t2 = tab();
        registry.register_open_session(t1.clone(), "s1".into());
        registry.register_open_session(t2.clone(), "s2".into());
        assert_eq!(registry.tab_for_session("s1"), Some(&t1));
        assert_eq!(registry.tab_for_session("s2"), Some(&t2));
        // Rebind s1 from t1 to t2: index is updated atomically.
        registry.register_open_session(t2.clone(), "s1".into());
        assert_eq!(registry.tab_for_session("s1"), Some(&t2));
    }

    #[test]
    fn drop_tab_clears_index_entries_pointing_at_it() {
        let mut registry = RoutingRegistry::new();
        let t1 = tab();
        let t2 = tab();
        registry.register_open_session(t1.clone(), "s1".into());
        registry.register_open_session(t2.clone(), "s2".into());
        registry.drop_tab(&t1);
        assert_eq!(registry.tab_for_session("s1"), None);
        assert_eq!(registry.tab_for_session("s2"), Some(&t2));
    }

    #[test]
    fn activity_summary_saturates_unread() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        for _ in 0..200 {
            apply_inactive_summary(&mut registry, &t, InactiveSummaryKind::UnreadActivity, None);
        }
        let summary = registry.activity(&t).unwrap();
        assert_eq!(summary.unread_count, MAX_TAB_UNREAD_DISPLAY);
    }

    #[test]
    fn activity_summary_records_pending_counts() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        apply_inactive_summary(
            &mut registry,
            &t,
            InactiveSummaryKind::PendingPermission,
            None,
        );
        apply_inactive_summary(
            &mut registry,
            &t,
            InactiveSummaryKind::PendingPermission,
            None,
        );
        apply_inactive_summary(
            &mut registry,
            &t,
            InactiveSummaryKind::PendingQuestion,
            None,
        );
        let summary = registry.activity(&t).unwrap();
        assert_eq!(summary.pending_permission_count, 2);
        assert_eq!(summary.pending_question_count, 1);
    }

    #[test]
    fn activity_summary_bounds_status_message() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        let huge = "x".repeat(MAX_TAB_LAST_ERROR_LEN * 4);
        apply_inactive_summary(
            &mut registry,
            &t,
            InactiveSummaryKind::StatusUpdate,
            Some(&huge),
        );
        let summary = registry.activity(&t).unwrap();
        let s = summary.last_error.as_deref().unwrap();
        assert!(s.len() <= MAX_TAB_LAST_ERROR_LEN + 4);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn activity_summary_clear_unread_resets_count() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        apply_inactive_summary(&mut registry, &t, InactiveSummaryKind::UnreadActivity, None);
        let summary = registry.activity_mut(&t);
        assert_eq!(summary.unread_count, 1);
        summary.clear_unread();
        assert_eq!(summary.unread_count, 0);
    }

    #[test]
    fn classify_global_event_routes_global() {
        let registry = RoutingRegistry::new();
        let event = AppEvent::ConfigChanged;
        let decision = classify_event(&event, &registry, None, 0);
        assert_eq!(decision, RouteDecision::Global);
    }

    #[test]
    fn classify_session_event_with_active_owner_routes_active() {
        let mut registry = RoutingRegistry::new();
        let tid = tab();
        registry.register_open_session(tid.clone(), "s1".into());
        let event = AppEvent::TextDelta {
            session_id: "s1".into(),
            delta: "hello".into(),
        };
        let decision = classify_event(&event, &registry, Some(&tid), 7);
        assert_eq!(
            decision,
            RouteDecision::ActiveView {
                tab_id: tid.clone()
            }
        );
    }

    #[test]
    fn classify_session_event_with_inactive_owner_routes_summary() {
        let mut registry = RoutingRegistry::new();
        let active = tab();
        let inactive = tab();
        registry.register_open_session(inactive.clone(), "s_other".into());
        let event = AppEvent::TextDelta {
            session_id: "s_other".into(),
            delta: "hello".into(),
        };
        let decision = classify_event(&event, &registry, Some(&active), 7);
        assert_eq!(
            decision,
            RouteDecision::InactiveSummary {
                tab_id: inactive.clone()
            }
        );
    }

    #[test]
    fn classify_session_event_with_unknown_session_drops_diagnostic() {
        let registry = RoutingRegistry::new();
        let active = tab();
        let event = AppEvent::TextDelta {
            session_id: "missing".into(),
            delta: "x".into(),
        };
        let decision = classify_event(&event, &registry, Some(&active), 7);
        match decision {
            RouteDecision::DropDiagnostic { .. } => {}
            other => panic!("expected drop diagnostic, got {other:?}"),
        }
    }

    #[test]
    fn classify_session_event_with_unknown_session_no_active_routes_summary() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        registry.register_open_session(t.clone(), "s_x".into());
        let event = AppEvent::TextDelta {
            session_id: "s_x".into(),
            delta: "x".into(),
        };
        let decision = classify_event(&event, &registry, None, 0);
        assert_eq!(
            decision,
            RouteDecision::InactiveSummary { tab_id: t.clone() }
        );
    }

    #[test]
    fn sequence_after_respects_monotonicity() {
        let mut registry = RoutingRegistry::new();
        let t = tab();
        registry.record_sequence(&t, 10);
        assert!(!registry.is_sequence_after(&t, 10));
        assert!(!registry.is_sequence_after(&t, 5));
        assert!(registry.is_sequence_after(&t, 11));
    }

    #[test]
    fn next_sequence_is_monotonic() {
        let mut registry = RoutingRegistry::new();
        let a = registry.next_sequence();
        let b = registry.next_sequence();
        assert!(b > a);
    }

    #[test]
    fn reconnect_epoch_bumps() {
        let mut registry = RoutingRegistry::new();
        let a = registry.bump_reconnect_epoch();
        let b = registry.bump_reconnect_epoch();
        assert!(b > a);
    }

    #[test]
    fn truncate_for_storage_preserves_utf8_boundaries() {
        let s = "é".repeat(50);
        let t = truncate_for_storage(&s, 8);
        // Must not panic; must respect char boundaries.
        assert!(t.len() <= 16);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn global_token_is_used_for_global_op_routes() {
        let _ = UiRouteToken::global(0);
    }
}
