//! TUI collaborator presence projection (Presence M002).
//!
//! Daemon-owned ephemeral presence rendered as a bounded per-project
//! projection. The TUI owns no presence truth: every entry here is
//! derived from an authorized `PresenceSnapshotDto` fetched through
//! `CoreClient`, keyed by canonical `project_id`.
//!
//! Invariants:
//!
//! * Presence is keyed by `project_id` (daemon-typed). Frontend-local
//!   `ProjectTabId`s never appear in presence keys.
//! * Stale completions are dropped via per-project `request_id` plus a
//!   global `reconnect_epoch`. Pre-reconnect completions fail closed.
//! * Unauthorized and feature-absent projects render identically
//!   (`Unavailable` — hidden count, generic panel). Cached principals
//!   are cleared on authorization loss so hidden data is never rendered
//!   from a local cache.
//! * Rapid project switching cannot leak one project's collaborators
//!   into another: snapshots only update the entry matching
//!   `snapshot.project_id`.
//! * Memory is bounded: at most [`MAX_PRESENCE_PROJECTS`] projects,
//!   daemon-bounded principals per project, display-truncated panels.
//! * Activity labels are coarse (`active` / `idle` / `observing` /
//!   `agent running`). No reasoning, prompts, file content, or command
//!   text is carried.

use std::collections::{HashMap, VecDeque};

use crate::protocol::core::{PresenceActivityDto, PresenceSnapshotDto};

/// Maximum number of per-project presence entries retained.
/// Matches the projection-summary bound (16) so inactive tabs stay
/// bounded.
pub const MAX_PRESENCE_PROJECTS: usize = 16;
/// Maximum collaborators rendered in the panel before truncation.
pub const MAX_PRESENCE_DISPLAY_PRINCIPALS: usize = 32;
/// Maximum sessions shown per collaborator row before truncation.
pub const MAX_PRESENCE_DISPLAY_SESSIONS: usize = 8;
/// Maximum display bytes for one principal identity.
pub const MAX_PRINCIPAL_DISPLAY_LEN: usize = 48;
/// Maximum stored error bytes per project.
pub const MAX_PRESENCE_ERROR_LEN: usize = 256;

/// Coarse activity label permitted by policy. Never carries content.
pub fn activity_label(activity: PresenceActivityDto) -> &'static str {
    match activity {
        PresenceActivityDto::Active => "active",
        PresenceActivityDto::Idle => "idle",
        PresenceActivityDto::Observing => "observing",
        PresenceActivityDto::AgentRunning => "agent running",
    }
}

/// Rank for stable ordering ties: higher activity first, then id.
fn activity_rank(activity: PresenceActivityDto) -> u8 {
    match activity {
        PresenceActivityDto::AgentRunning => 3,
        PresenceActivityDto::Active => 2,
        PresenceActivityDto::Observing => 1,
        PresenceActivityDto::Idle => 0,
    }
}

/// Truncate a principal identity for display, preserving char boundaries.
pub fn display_principal(principal_id: &str) -> String {
    if principal_id.len() <= MAX_PRINCIPAL_DISPLAY_LEN {
        return principal_id.to_string();
    }
    let mut cut = MAX_PRINCIPAL_DISPLAY_LEN;
    while cut > 0 && !principal_id.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 1);
    out.push_str(&principal_id[..cut]);
    out.push('…');
    out
}

fn truncate_error(msg: &str) -> String {
    if msg.len() <= MAX_PRESENCE_ERROR_LEN {
        return msg.to_string();
    }
    let mut cut = MAX_PRESENCE_ERROR_LEN;
    while cut > 0 && !msg.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 1);
    out.push_str(&msg[..cut]);
    out.push('…');
    out
}

/// One collaborator row derived from a snapshot principal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollaboratorEntry {
    /// Canonical principal identity (full, not display-truncated).
    pub principal_id: String,
    pub activity: PresenceActivityDto,
    pub client_count: usize,
    /// Sorted session locators (bounded by daemon snapshot).
    pub session_ids: Vec<String>,
    pub last_active_ms: i64,
}

impl CollaboratorEntry {
    /// Bounded one-line rendering: identity + session/client counts +
    /// coarse status. No secrets or content.
    pub fn display_line(&self) -> String {
        let sessions = self.session_ids.len();
        let session_part = if sessions == 0 {
            "no sessions".to_string()
        } else if sessions == 1 {
            "1 session".to_string()
        } else {
            format!("{sessions} sessions")
        };
        let client_part = if self.client_count == 1 {
            "1 client".to_string()
        } else {
            format!("{} clients", self.client_count)
        };
        format!(
            "{} — {} · {} · {}",
            display_principal(&self.principal_id),
            activity_label(self.activity),
            session_part,
            client_part
        )
    }
}

/// Presentation status for one project's presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    /// Never fetched.
    Unknown,
    /// Fetch in flight.
    Loading,
    /// Authorized snapshot applied (may be empty — see entry count).
    Ready,
    /// Capability unsupported, unauthorized, or absent. Renders
    /// identically in all three cases; count is hidden.
    Unavailable,
    /// Transient failure with optional stale data retained.
    Error,
}

/// Per-project presence projection.
#[derive(Debug, Clone)]
pub struct ProjectPresence {
    pub project_id: String,
    pub principals: Vec<CollaboratorEntry>,
    pub truncated: bool,
    pub as_of_ms: i64,
    /// Local monotonic sequence bumped on each applied snapshot.
    pub sequence: u64,
    pub status: PresenceStatus,
    pub last_error: Option<String>,
    /// Current in-flight request id (0 when idle).
    pub current_request_id: u64,
    /// Whether a resync is required (reconnect, error, or hint).
    pub needs_resync: bool,
}

impl ProjectPresence {
    fn fresh(project_id: String) -> Self {
        Self {
            project_id,
            principals: Vec::new(),
            truncated: false,
            as_of_ms: 0,
            sequence: 0,
            status: PresenceStatus::Unknown,
            last_error: None,
            current_request_id: 0,
            needs_resync: false,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.status == PresenceStatus::Loading
    }

    pub fn collaborator_count(&self) -> usize {
        self.principals.len()
    }
}

/// Bounded per-project presence map. Derived frontend state only.
#[derive(Debug, Default)]
pub struct PresenceState {
    entries: HashMap<String, ProjectPresence>,
    /// Insertion order for LRU eviction of inactive projects.
    order: VecDeque<String>,
    /// `None` before the first capabilities round-trip.
    pub capability_supported: Option<bool>,
    /// Monotonic reconnect epoch. Bumped on transport reconnect so
    /// pre-reconnect completions are dropped.
    pub reconnect_epoch: u64,
    request_counter: u64,
    sequence_counter: u64,
}

impl PresenceState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a capabilities round-trip. Toggling to `false` marks
    /// every cached project unavailable without leaking which projects
    /// exist.
    pub fn set_capability(&mut self, supported: bool) {
        self.capability_supported = Some(supported);
        if !supported {
            for entry in self.entries.values_mut() {
                entry.principals.clear();
                entry.truncated = false;
                entry.status = PresenceStatus::Unavailable;
                entry.last_error = None;
                entry.needs_resync = false;
                entry.current_request_id = 0;
            }
        }
    }

    pub fn is_supported(&self) -> bool {
        self.capability_supported == Some(true)
    }

    /// Begin a refresh for `project_id`. Returns the request id the
    /// completion must echo. Marks the entry loading and records LRU
    /// order. Does not perform I/O.
    pub fn begin_refresh(&mut self, project_id: &str) -> u64 {
        self.request_counter = self.request_counter.saturating_add(1);
        let request_id = self.request_counter;
        let entry = self
            .entries
            .entry(project_id.to_string())
            .or_insert_with(|| ProjectPresence::fresh(project_id.to_string()));
        entry.current_request_id = request_id;
        entry.status = PresenceStatus::Loading;
        entry.needs_resync = false;
        self.touch_order(project_id);
        self.evict_if_needed(Some(project_id));
        request_id
    }

    /// Whether `project_id` needs a (re)fetch: unknown, resync-flagged,
    /// or currently in an error/unavailable state that the user asked
    /// to retry. Loading entries never need a duplicate fetch (no
    /// polling storm).
    pub fn needs_refresh(&self, project_id: &str) -> bool {
        match self.entries.get(project_id) {
            None => true,
            Some(entry) => match entry.status {
                PresenceStatus::Unknown => true,
                PresenceStatus::Loading => false,
                PresenceStatus::Ready => entry.needs_resync,
                PresenceStatus::Unavailable => entry.needs_resync,
                PresenceStatus::Error => true,
            },
        }
    }

    /// Apply an authorized snapshot. Drops stale completions (wrong
    /// request id, wrong reconnect epoch, or project mismatch) and
    /// returns `false` without mutating state.
    pub fn apply_snapshot(
        &mut self,
        request_id: u64,
        project_id: &str,
        snapshot: &PresenceSnapshotDto,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        // Routing guard: a snapshot for another project must never
        // overwrite this entry (rapid-switch leak prevention).
        if snapshot.project_id != project_id {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        let mut principals: Vec<CollaboratorEntry> = snapshot
            .principals
            .iter()
            .map(|p| CollaboratorEntry {
                principal_id: p.principal_id.clone(),
                activity: p.activity,
                client_count: p.client_count,
                session_ids: {
                    let mut sessions = p.session_ids.clone();
                    sessions.sort();
                    sessions
                },
                last_active_ms: p.last_active_ms,
            })
            .collect();
        // Stable ordering: activity rank first, then principal id.
        principals.sort_by(|a, b| {
            activity_rank(b.activity)
                .cmp(&activity_rank(a.activity))
                .then_with(|| a.principal_id.cmp(&b.principal_id))
        });
        self.sequence_counter = self.sequence_counter.saturating_add(1);
        entry.principals = principals;
        entry.truncated = snapshot.truncated;
        entry.as_of_ms = snapshot.as_of_ms;
        entry.sequence = self.sequence_counter;
        entry.status = PresenceStatus::Ready;
        entry.last_error = None;
        entry.current_request_id = 0;
        entry.needs_resync = false;
        self.touch_order(project_id);
        true
    }

    /// Apply a snapshot failure.
    ///
    /// * `unauthorized` covers both `project_not_found` denials (which
    ///   are indistinguishable from absent) and genuinely absent
    ///   projects: cached principals are cleared and the entry renders
    ///   identically to feature-absent.
    /// * `unsupported` covers older daemons without the presence
    ///   capability: same rendering as unauthorized.
    /// * Transient errors retain stale principals (if any) and flag
    ///   `needs_resync` so the next refresh replaces them.
    pub fn apply_error(
        &mut self,
        request_id: u64,
        project_id: &str,
        error: String,
        unauthorized: bool,
        unsupported: bool,
        reconnect_epoch: u64,
    ) -> bool {
        if reconnect_epoch != self.reconnect_epoch {
            return false;
        }
        let Some(entry) = self.entries.get_mut(project_id) else {
            return false;
        };
        if entry.current_request_id != request_id {
            return false;
        }
        entry.current_request_id = 0;
        if unauthorized || unsupported {
            entry.principals.clear();
            entry.truncated = false;
            entry.status = PresenceStatus::Unavailable;
            entry.last_error = None;
            entry.needs_resync = false;
            if unsupported {
                self.capability_supported = Some(false);
            }
        } else {
            entry.status = PresenceStatus::Error;
            entry.last_error = Some(truncate_error(&error));
            entry.needs_resync = true;
        }
        true
    }

    /// Mark a liveness hint (`PresenceUpdated { project_id }`). Carries
    /// no collaborator detail; flags the project for a bounded re-fetch
    /// when it is not already loading. Returns `true` when the caller
    /// should issue a refresh (coalesced — no task per update).
    pub fn note_hint(&mut self, project_id: &str) -> bool {
        // Unknown capability or unsupported: hints are ignored so old
        // daemons never cause fetch storms.
        if self.capability_supported == Some(false) {
            return false;
        }
        match self.entries.get_mut(project_id) {
            None => {
                // No entry yet: the active project will fetch on open.
                // Record the project as needing data without starting a
                // fetch here (the open path owns the request).
                let mut entry = ProjectPresence::fresh(project_id.to_string());
                entry.needs_resync = true;
                self.entries.insert(project_id.to_string(), entry);
                self.touch_order(project_id);
                self.evict_if_needed(None);
                true
            }
            Some(entry) => {
                if entry.status == PresenceStatus::Loading {
                    false
                } else {
                    entry.needs_resync = true;
                    true
                }
            }
        }
    }

    /// Transport reconnect: bump the epoch, drop in-flight request
    /// bindings, and flag every project for resync. Stale presentation
    /// is replaced by the next authoritative snapshot; nothing is
    /// fabricated here.
    pub fn on_reconnect(&mut self) -> u64 {
        self.reconnect_epoch = self.reconnect_epoch.saturating_add(1);
        for entry in self.entries.values_mut() {
            entry.current_request_id = 0;
            entry.needs_resync = true;
            if entry.status == PresenceStatus::Loading {
                entry.status = PresenceStatus::Unknown;
            }
        }
        self.reconnect_epoch
    }

    /// Authorization loss or tab close: drop the cached entry so hidden
    /// data is never rendered from a local cache.
    pub fn clear_project(&mut self, project_id: &str) {
        self.entries.remove(project_id);
        self.order.retain(|p| p != project_id);
    }

    /// Drop all entries (daemon restart / logout equivalent).
    pub fn clear_all(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub fn get(&self, project_id: &str) -> Option<&ProjectPresence> {
        self.entries.get(project_id)
    }

    pub fn project_count(&self) -> usize {
        self.entries.len()
    }

    /// Header summary for the tab strip: `Some("👥 N[ status]")` when
    /// authorized data exists, `None` when unavailable/loading/unknown
    /// (hidden — identical for unauthorized and absent).
    pub fn header_summary(&self, project_id: &str) -> Option<String> {
        let entry = self.entries.get(project_id)?;
        match entry.status {
            PresenceStatus::Ready => {
                let count = entry.principals.len();
                let suffix = if entry.needs_resync {
                    " (stale)"
                } else if entry
                    .principals
                    .iter()
                    .any(|p| matches!(p.activity, PresenceActivityDto::AgentRunning))
                {
                    " · agent running"
                } else {
                    ""
                };
                Some(format!("👥 {count}{suffix}"))
            }
            PresenceStatus::Error if !entry.principals.is_empty() => {
                Some(format!("👥 {} (stale)", entry.principals.len()))
            }
            _ => None,
        }
    }

    /// Bounded panel lines for `/collaborators`. Coarse labels only.
    /// When the daemon lacks presence support, every project — including
    /// never-fetched ones — renders the identical unavailable panel so
    /// old daemons never break tabs.
    pub fn panel_lines(&self, project_id: &str) -> Vec<String> {
        let Some(entry) = self.entries.get(project_id) else {
            if self.capability_supported == Some(false) {
                return vec![
                    "Collaborators unavailable".to_string(),
                    String::new(),
                    "Presence is unavailable for this project.".to_string(),
                    "Older daemons hide this panel; unauthorized projects look the same."
                        .to_string(),
                ];
            }
            return vec![
                "Collaborators — loading…".to_string(),
                String::new(),
                "Fetching presence for this project…".to_string(),
            ];
        };
        match entry.status {
            PresenceStatus::Unknown | PresenceStatus::Loading => vec![
                "Collaborators — loading…".to_string(),
                String::new(),
                "Fetching presence for this project…".to_string(),
            ],
            PresenceStatus::Unavailable => vec![
                "Collaborators unavailable".to_string(),
                String::new(),
                "Presence is unavailable for this project.".to_string(),
                "Older daemons hide this panel; unauthorized projects look the same.".to_string(),
            ],
            PresenceStatus::Ready if entry.principals.is_empty() => vec![
                "Collaborators (0)".to_string(),
                String::new(),
                "No other collaborators right now.".to_string(),
                "You are the only active presence in this project.".to_string(),
            ],
            PresenceStatus::Ready => {
                let mut lines = vec![
                    format!("Collaborators ({})", entry.principals.len()),
                    String::new(),
                ];
                let visible = entry
                    .principals
                    .iter()
                    .take(MAX_PRESENCE_DISPLAY_PRINCIPALS);
                for collaborator in visible {
                    lines.push(collaborator.display_line());
                    if collaborator.session_ids.len() > MAX_PRESENCE_DISPLAY_SESSIONS {
                        lines.push(format!(
                            "    +{} more sessions (bounded)",
                            collaborator.session_ids.len() - MAX_PRESENCE_DISPLAY_SESSIONS
                        ));
                    }
                }
                let hidden = entry
                    .principals
                    .len()
                    .saturating_sub(MAX_PRESENCE_DISPLAY_PRINCIPALS);
                if hidden > 0 || entry.truncated {
                    let extra = if entry.truncated && hidden == 0 {
                        "more on daemon".to_string()
                    } else {
                        format!("+{hidden} more (bounded)")
                    };
                    lines.push(String::new());
                    lines.push(extra);
                }
                if entry.needs_resync {
                    lines.push(String::new());
                    lines.push("Stale — refresh to resync.".to_string());
                }
                lines
            }
            PresenceStatus::Error => {
                if entry.principals.is_empty() {
                    let mut lines =
                        vec!["Collaborators — refresh failed".to_string(), String::new()];
                    lines.push(format!(
                        "Error: {}",
                        entry.last_error.as_deref().unwrap_or("unknown error")
                    ));
                    lines.push("Retry with /collaborators refresh.".to_string());
                    lines
                } else {
                    let mut lines = vec![
                        format!("Collaborators ({}) — stale", entry.principals.len()),
                        String::new(),
                    ];
                    for collaborator in entry
                        .principals
                        .iter()
                        .take(MAX_PRESENCE_DISPLAY_PRINCIPALS)
                    {
                        lines.push(collaborator.display_line());
                    }
                    lines.push(String::new());
                    lines.push(format!(
                        "Refresh failed: {} — showing last known.",
                        entry.last_error.as_deref().unwrap_or("unknown error")
                    ));
                    lines
                }
            }
        }
    }

    fn touch_order(&mut self, project_id: &str) {
        self.order.retain(|p| p != project_id);
        self.order.push_back(project_id.to_string());
    }

    fn evict_if_needed(&mut self, protect: Option<&str>) {
        while self.entries.len() > MAX_PRESENCE_PROJECTS {
            let victim = self
                .order
                .iter()
                .find(|p| Some(p.as_str()) != protect)
                .cloned();
            match victim {
                Some(id) => {
                    self.entries.remove(&id);
                    self.order.retain(|p| p != &id);
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::core::{PresencePrincipalDto, PresenceSnapshotDto};

    fn principal(
        id: &str,
        activity: PresenceActivityDto,
        sessions: &[&str],
    ) -> PresencePrincipalDto {
        PresencePrincipalDto {
            principal_id: id.to_string(),
            activity,
            client_count: 1,
            session_ids: sessions.iter().map(|s| s.to_string()).collect(),
            last_active_ms: 1,
        }
    }

    fn snapshot(
        project: &str,
        principals: Vec<PresencePrincipalDto>,
        truncated: bool,
    ) -> PresenceSnapshotDto {
        PresenceSnapshotDto {
            project_id: project.to_string(),
            as_of_ms: 42,
            principals,
            truncated,
        }
    }

    #[test]
    fn multi_project_routing_does_not_cross_contaminate() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let req_a = state.begin_refresh("proj-a");
        let req_b = state.begin_refresh("proj-b");
        let epoch = state.reconnect_epoch;
        assert!(state.apply_snapshot(
            req_a,
            "proj-a",
            &snapshot(
                "proj-a",
                vec![principal("alice", PresenceActivityDto::Active, &["s1"])],
                false
            ),
            epoch
        ));
        assert!(state.apply_snapshot(
            req_b,
            "proj-b",
            &snapshot(
                "proj-b",
                vec![principal("bob", PresenceActivityDto::Idle, &[])],
                false
            ),
            epoch
        ));
        assert_eq!(
            state.get("proj-a").unwrap().principals[0].principal_id,
            "alice"
        );
        assert_eq!(
            state.get("proj-b").unwrap().principals[0].principal_id,
            "bob"
        );
        // Cross-project snapshot is rejected.
        let req_c = state.begin_refresh("proj-a");
        assert!(!state.apply_snapshot(
            req_c,
            "proj-a",
            &snapshot(
                "proj-b",
                vec![principal("mallory", PresenceActivityDto::Active, &[])],
                false
            ),
            epoch
        ));
        assert_eq!(
            state.get("proj-a").unwrap().principals[0].principal_id,
            "alice"
        );
    }

    #[test]
    fn ordering_is_stable_and_bounded() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let req = state.begin_refresh("p");
        let epoch = state.reconnect_epoch;
        let snap = snapshot(
            "p",
            vec![
                principal("zeta", PresenceActivityDto::Idle, &[]),
                principal("alpha", PresenceActivityDto::Idle, &[]),
                principal("mid", PresenceActivityDto::AgentRunning, &["s1", "s2"]),
            ],
            true,
        );
        assert!(state.apply_snapshot(req, "p", &snap, epoch));
        let ids: Vec<&str> = state
            .get("p")
            .unwrap()
            .principals
            .iter()
            .map(|e| e.principal_id.as_str())
            .collect();
        // AgentRunning ranks first, then id order.
        assert_eq!(ids, vec!["mid", "alpha", "zeta"]);
        assert!(state.get("p").unwrap().truncated);
        let lines = state.panel_lines("p");
        assert!(lines.iter().any(|l| l.contains("Collaborators (3)")));
    }

    #[test]
    fn stale_request_id_is_dropped() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let stale = state.begin_refresh("p");
        let fresh = state.begin_refresh("p");
        let epoch = state.reconnect_epoch;
        // Stale completion cannot overwrite the newer request.
        assert!(!state.apply_snapshot(
            stale,
            "p",
            &snapshot(
                "p",
                vec![principal("old", PresenceActivityDto::Active, &[])],
                false
            ),
            epoch
        ));
        assert!(state.apply_snapshot(
            fresh,
            "p",
            &snapshot(
                "p",
                vec![principal("new", PresenceActivityDto::Active, &[])],
                false
            ),
            epoch
        ));
        assert_eq!(state.get("p").unwrap().principals[0].principal_id, "new");
    }

    #[test]
    fn reconnect_drops_pre_reconnect_completions_and_flags_resync() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let req = state.begin_refresh("p");
        let old_epoch = state.reconnect_epoch;
        let new_epoch = state.on_reconnect();
        assert_ne!(old_epoch, new_epoch);
        // Pre-reconnect completion is dropped.
        assert!(!state.apply_snapshot(
            req,
            "p",
            &snapshot(
                "p",
                vec![principal("ghost", PresenceActivityDto::Active, &[])],
                false
            ),
            old_epoch
        ));
        // Entry is flagged for resync.
        assert!(state.get("p").unwrap().needs_resync);
        // Fresh refresh after reconnect succeeds.
        let req2 = state.begin_refresh("p");
        assert!(state.apply_snapshot(
            req2,
            "p",
            &snapshot(
                "p",
                vec![principal("fresh", PresenceActivityDto::Active, &[])],
                false
            ),
            new_epoch
        ));
        assert!(!state.get("p").unwrap().needs_resync);
    }

    #[test]
    fn unauthorized_and_feature_absent_render_identically() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let req_a = state.begin_refresh("secret");
        let epoch = state.reconnect_epoch;
        assert!(state.apply_error(
            req_a,
            "secret",
            "project_not_found: denied".into(),
            true,
            false,
            epoch
        ));
        let req_b = state.begin_refresh("old-daemon-proj");
        assert!(state.apply_error(
            req_b,
            "old-daemon-proj",
            "unsupported".into(),
            false,
            true,
            epoch
        ));
        assert_eq!(
            state.get("secret").unwrap().status,
            PresenceStatus::Unavailable
        );
        assert_eq!(
            state.get("old-daemon-proj").unwrap().status,
            PresenceStatus::Unavailable
        );
        assert!(state.get("secret").unwrap().principals.is_empty());
        assert_eq!(
            state.panel_lines("secret"),
            state.panel_lines("old-daemon-proj")
        );
        assert_eq!(state.header_summary("secret"), None);
        assert_eq!(state.header_summary("old-daemon-proj"), None);
    }

    #[test]
    fn identical_principal_names_across_projects_do_not_collide() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let epoch = state.reconnect_epoch;
        let ra = state.begin_refresh("proj-a");
        let rb = state.begin_refresh("proj-b");
        assert!(state.apply_snapshot(
            ra,
            "proj-a",
            &snapshot(
                "proj-a",
                vec![principal("sam", PresenceActivityDto::Active, &["s-a"])],
                false
            ),
            epoch
        ));
        assert!(state.apply_snapshot(
            rb,
            "proj-b",
            &snapshot(
                "proj-b",
                vec![principal("sam", PresenceActivityDto::Idle, &["s-b"])],
                false
            ),
            epoch
        ));
        assert_eq!(
            state.get("proj-a").unwrap().principals[0].activity,
            PresenceActivityDto::Active
        );
        assert_eq!(
            state.get("proj-b").unwrap().principals[0].activity,
            PresenceActivityDto::Idle
        );
    }

    #[test]
    fn inactive_tab_resource_bound_evicts_oldest() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let epoch = state.reconnect_epoch;
        for i in 0..(MAX_PRESENCE_PROJECTS + 4) {
            let project = format!("proj-{i:02}");
            let req = state.begin_refresh(&project);
            assert!(state.apply_snapshot(
                req,
                &project,
                &snapshot(
                    &project,
                    vec![principal("u", PresenceActivityDto::Active, &[])],
                    false
                ),
                epoch
            ));
        }
        assert!(state.project_count() <= MAX_PRESENCE_PROJECTS);
        // Newest entries survive.
        assert!(state
            .get(&format!("proj-{:02}", MAX_PRESENCE_PROJECTS + 3))
            .is_some());
    }

    #[test]
    fn authorization_loss_clears_cache() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let req = state.begin_refresh("p");
        let epoch = state.reconnect_epoch;
        assert!(state.apply_snapshot(
            req,
            "p",
            &snapshot(
                "p",
                vec![principal("alice", PresenceActivityDto::Active, &[])],
                false
            ),
            epoch
        ));
        assert!(state.header_summary("p").is_some());
        state.clear_project("p");
        assert!(state.get("p").is_none());
        assert_eq!(state.header_summary("p"), None);
    }

    #[test]
    fn activity_labels_are_coarse_and_bounded() {
        assert_eq!(activity_label(PresenceActivityDto::Active), "active");
        assert_eq!(
            activity_label(PresenceActivityDto::AgentRunning),
            "agent running"
        );
        let entry = CollaboratorEntry {
            principal_id: "alice".to_string(),
            activity: PresenceActivityDto::AgentRunning,
            client_count: 2,
            session_ids: vec!["s1".to_string()],
            last_active_ms: 0,
        };
        let line = entry.display_line();
        assert!(line.contains("alice"));
        assert!(line.contains("agent running"));
        assert!(!line.contains("token"));
        assert!(!line.contains("prompt"));
    }

    #[test]
    fn hint_coalesces_while_loading() {
        let mut state = PresenceState::new();
        state.set_capability(true);
        let _req = state.begin_refresh("p");
        // Loading: hint does not request a duplicate fetch.
        assert!(!state.note_hint("p"));
        let epoch = state.reconnect_epoch;
        // Finish the load, then a hint flags resync exactly once.
        let current = state.get("p").unwrap().current_request_id;
        assert!(state.apply_snapshot(current, "p", &snapshot("p", vec![], false), epoch));
        assert!(state.note_hint("p"));
        assert!(state.get("p").unwrap().needs_resync);
    }
}
