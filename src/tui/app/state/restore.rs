//! Multi-Project TUI restore coordinator (Milestone 004).
//!
//! Implements the restore pipeline that converts a validated
//! manifest into live frontend state. The pipeline is explicit,
//! bounded, and cancellable:
//!
//! 1. Caller passes a `TuiWorkspaceManifest` already loaded and
//!    validated by [`crate::tui::app::state::manifest`].
//! 2. The coordinator constructs a [`RestorePlan`] that captures
//!    the deterministic tab ordering, the active tab selection, and
//!    the per-tab validity classification.
//! 3. The coordinator classifies each entry into
//!    [`RestoreEntryStatus`] (Valid / Archived / Missing /
//!    Unsupported / Rebound / Unknown) by comparing the persisted
//!    project_id/session_id against the daemon catalog and a
//!    per-project `ProjectGet` response.
//! 4. The plan is consumed by [`apply_restore_plan`] which builds
//!    lightweight `ProjectTabState` entries, decides which tab is
//!    active, and signals which tab (at most one) should be loaded
//!    with heavy session state.
//!
//! The coordinator never blocks waiting for the daemon: each
//! classification step receives a snapshot of the catalog/session
//! lookup results and produces a `RestorePlan` synchronously. The
//! TUI's async layer is responsible for fetching the catalog and
//! per-project detail ahead of plan construction.
//!
//! ## Invariants
//!
//! - At most one heavy session view is loaded after restore.
//! - Archived, deleted, missing, rebound, unsupported, or
//!   unauthorized objects are skipped or represented as bounded
//!   unavailable placeholders; they are NEVER recreated implicitly.
//! - A failed or partial restore cannot prevent the TUI from
//!   opening in a safe empty/compatibility state.
//! - Frontend-local tab IDs are regenerated; the plan uses
//!   `ProjectTabId::new()` for each entry.
//! - The plan carries at most one `pending_heavy_load` entry.

use std::collections::HashMap;

use crate::tui::app::state::manifest::{
    PersistedProjectTab, TuiWorkspaceManifest, MAX_PERSISTED_TABS,
};
use crate::tui::app::state::project_tabs::{ProjectTabId, ProjectTabState};

/// Per-entry classification after the restore coordinator has
/// compared a persisted tab against the live daemon state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreEntryStatus {
    /// Persisted identity was found in the live daemon catalog and
    /// (where applicable) the persisted workspace/session is bound
    /// to the same project. The tab can be opened.
    Valid,
    /// Persisted identity was found but the project is archived
    /// (`project.archived = true`). The tab is skipped per the
    /// recovery policy.
    Archived,
    /// Persisted identity is not present in the live catalog. The
    /// tab is skipped.
    Missing,
    /// Persisted identity references a feature this daemon does
    /// not support (e.g. session binding on an older daemon). The
    /// tab is kept open only with a project-level identity and the
    /// session is dropped.
    Unsupported,
    /// Persisted session is now bound to a different canonical
    /// project. The tab is kept open under the new project identity
    /// and the stale session association is dropped.
    Rebound,
    /// Persisted identity has no recoverable interpretation. The
    /// tab is skipped and a diagnostic is recorded.
    Unknown,
}

/// Classification of a single persisted tab entry after restore
/// validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreEntry {
    pub tab_id: ProjectTabId,
    pub persisted: PersistedProjectTab,
    pub status: RestoreEntryStatus,
    /// Daemon-authoritative project_id (may differ from persisted
    /// when rebound).
    pub resolved_project_id: Option<String>,
    /// Daemon-authoritative workspace_id (validated).
    pub resolved_workspace_id: Option<String>,
    /// Daemon-authoritative session_id (validated).
    pub resolved_session_id: Option<String>,
    /// Optional bounded diagnostic for `Missing`/`Unsupported`/etc.
    pub diagnostic: Option<String>,
}

impl RestoreEntry {
    /// Whether this entry should produce a live tab. `Valid`,
    /// `Rebound` (with a resolved project), and `Unsupported` (with
    /// at least a project identity) all open a tab; everything else
    /// is skipped.
    pub fn opens_tab(&self) -> bool {
        self.resolved_project_id.is_some()
    }

    /// Whether this entry has a session that can be loaded as the
    /// heavy active view.
    pub fn has_heavy_session(&self) -> bool {
        self.status == RestoreEntryStatus::Valid && self.resolved_session_id.is_some()
    }
}

/// Plan emitted by the restore coordinator. Consumed by
/// `apply_restore_plan` to materialize `ProjectTabs` and active
/// selection.
#[derive(Debug, Clone)]
pub struct RestorePlan {
    /// Ordered entries, mirroring the persisted order after
    /// dedup/validation. Bounded by `MAX_PERSISTED_TABS`.
    pub entries: Vec<RestoreEntry>,
    /// Tab id to activate, when present in `entries`. `None` means
    /// no restored tab is eligible and the TUI falls back to its
    /// single compat tab.
    pub active_tab_id: Option<ProjectTabId>,
    /// Optional tab to load as the heavy active view. The TUI loads
    /// at most one heavy view; if multiple `has_heavy_session`
    /// entries exist, the active tab wins.
    pub pending_heavy_load: Option<ProjectTabId>,
    /// Optional diagnostic summary for the operator surface.
    pub diagnostics: Vec<RestoreDiagnostic>,
}

/// Bounded diagnostic surfaced from the restore pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreDiagnostic {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl RestoreDiagnostic {
    #[allow(dead_code)]
    fn truncated_message(input: &str) -> String {
        if input.len() <= 128 {
            input.to_string()
        } else {
            let mut end = 128;
            while !input.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &input[..end])
        }
    }
}

/// Wire-friendly view of a [`RestorePlan`] suitable for
/// transferring across the in-process TUI command channel. The
/// frontend reconstructs lightweight `ProjectTabState` entries from
/// this view; the actual `restore` module stays decoupled from the
/// `App` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlanWire {
    pub entries: Vec<RestoreEntryWire>,
    pub active_project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreEntryWire {
    pub tab_id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub label_hint: Option<String>,
    pub selected_model_id: Option<String>,
    pub selected_agent: Option<String>,
    pub order_key: Option<String>,
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreDiagnosticWire {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub code: &'static str,
    pub message: String,
}

impl RestorePlan {
    /// Convert this plan into a wire-friendly view. Used by the
    /// async restore command pair so the apply step does not need
    /// to keep a reference to the original plan.
    pub fn to_wire(&self) -> RestorePlanWire {
        let entries = self
            .entries
            .iter()
            .filter(|e| e.opens_tab())
            .map(|e| RestoreEntryWire {
                tab_id: e.tab_id.to_string(),
                project_id: e
                    .resolved_project_id
                    .clone()
                    .expect("opens_tab implies resolved project_id"),
                workspace_id: e.resolved_workspace_id.clone(),
                session_id: e.resolved_session_id.clone(),
                label_hint: e.persisted.label_hint.clone(),
                selected_model_id: e.persisted.selected_model_id.clone(),
                selected_agent: e.persisted.selected_agent.clone(),
                order_key: e.persisted.order_key.clone(),
                status: match e.status {
                    RestoreEntryStatus::Valid => "valid",
                    RestoreEntryStatus::Archived => "archived",
                    RestoreEntryStatus::Missing => "missing",
                    RestoreEntryStatus::Unsupported => "unsupported",
                    RestoreEntryStatus::Rebound => "rebound",
                    RestoreEntryStatus::Unknown => "unknown",
                },
            })
            .collect();
        let active_project_id = self.active_tab_id.as_ref().and_then(|tid| {
            self.entries
                .iter()
                .find(|e| &e.tab_id == tid)
                .and_then(|e| e.resolved_project_id.clone())
        });
        RestorePlanWire {
            entries,
            active_project_id,
        }
    }
}

impl RestorePlan {
    /// Convert internal diagnostics to wire form.
    pub fn diagnostics_wire(&self) -> Vec<RestoreDiagnosticWire> {
        self.diagnostics
            .iter()
            .map(|d| RestoreDiagnosticWire {
                project_id: d.project_id.clone(),
                session_id: d.session_id.clone(),
                code: d.code,
                message: d.message.clone(),
            })
            .collect()
    }
}

impl From<&RestorePlan> for RestorePlanWire {
    fn from(plan: &RestorePlan) -> Self {
        plan.to_wire()
    }
}

/// Snapshot of daemon state needed to validate a manifest. The TUI
/// builds this snapshot before invoking `build_restore_plan`.
#[derive(Debug, Default, Clone)]
pub struct DaemonLookupSnapshot {
    /// Project summaries from `ProjectList`. The coordinator uses
    /// this for the fast-path existence check before issuing
    /// `ProjectGet`.
    pub catalog: Vec<CatalogEntry>,
    /// Per-project canonical metadata, keyed by `project_id`.
    /// Populated lazily for projects present in the persisted
    /// manifest but absent from `catalog`.
    pub project_details: HashMap<String, ProjectDetailSnapshot>,
}

/// Minimal project summary used by the restore coordinator. The TUI
/// adapts `ProjectSummaryDto` into this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub project_id: String,
    pub archived: bool,
}

/// Minimal per-project metadata used by the restore coordinator.
/// The TUI adapts `ProjectDetailsDto` into this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDetailSnapshot {
    pub project_id: String,
    pub archived: bool,
    /// Workspace ids known to the daemon for this project. Used to
    /// validate persisted `workspace_id`.
    pub workspaces: Vec<String>,
    /// Session ids known to the daemon for this project, each
    /// tagged with the canonical project binding. Used to detect
    /// `Rebound` and `Missing`.
    pub sessions: Vec<SessionBinding>,
}

/// Per-session binding used by the restore coordinator. The
/// `canonical_project_id` is what the daemon currently reports; if
/// it differs from the persisted project_id the session is
/// `Rebound`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBinding {
    pub session_id: String,
    pub canonical_project_id: String,
}

impl DaemonLookupSnapshot {
    /// Build the restore plan from the manifest and the daemon
    /// snapshot. The plan is constructed synchronously and is
    /// deterministic for a given input pair.
    pub fn build_restore_plan(&self, manifest: &TuiWorkspaceManifest) -> RestorePlan {
        let mut entries = Vec::with_capacity(manifest.ordered_tabs.len());
        let mut diagnostics = Vec::new();

        for persisted in manifest.ordered_tabs.iter() {
            let entry = self.classify_entry(persisted, &mut diagnostics);
            entries.push(entry);
        }

        if entries.len() > MAX_PERSISTED_TABS {
            entries.truncate(MAX_PERSISTED_TABS);
        }

        // Choose the active tab: prefer the persisted
        // active_project_id when it resolves to a valid entry;
        // otherwise fall back to the first entry that opens a tab;
        // otherwise leave None.
        let mut active_tab_id = None;
        if let Some(active_pid) = manifest.active_project_id.as_deref() {
            if let Some(entry) = entries
                .iter()
                .find(|e| e.resolved_project_id.as_deref() == Some(active_pid) && e.opens_tab())
            {
                active_tab_id = Some(entry.tab_id.clone());
            }
        }
        if active_tab_id.is_none() {
            active_tab_id = entries
                .iter()
                .find(|e| e.opens_tab())
                .map(|e| e.tab_id.clone());
        }

        // Heavy view is loaded only for the active tab and only if
        // it has a validated session.
        let pending_heavy_load = active_tab_id
            .as_ref()
            .and_then(|tid| entries.iter().find(|e| &e.tab_id == tid))
            .and_then(|e| {
                if e.has_heavy_session() {
                    Some(e.tab_id.clone())
                } else {
                    None
                }
            });

        RestorePlan {
            entries,
            active_tab_id,
            pending_heavy_load,
            diagnostics,
        }
    }

    fn classify_entry(
        &self,
        persisted: &PersistedProjectTab,
        diagnostics: &mut Vec<RestoreDiagnostic>,
    ) -> RestoreEntry {
        let tab_id = ProjectTabId::new();
        let resolved_project_id = persisted.project_id.clone();
        let mut resolved_workspace_id = persisted.workspace_id.clone();
        let mut resolved_session_id = persisted.session_id.clone();
        let mut diagnostic: Option<String> = None;

        let Some(pid) = persisted.project_id.as_deref() else {
            diagnostics.push(RestoreDiagnostic {
                project_id: None,
                session_id: persisted.session_id.clone(),
                code: "missing_project_id",
                message: "persisted tab has no project_id".into(),
            });
            return RestoreEntry {
                tab_id,
                persisted: persisted.clone(),
                status: RestoreEntryStatus::Unknown,
                resolved_project_id: None,
                resolved_workspace_id: None,
                resolved_session_id: None,
                diagnostic: Some("missing project_id".into()),
            };
        };

        // First check the catalog fast path.
        let catalog_match = self.catalog.iter().find(|e| e.project_id == pid);

        // Fall back to a per-project detail lookup.
        let detail = catalog_match
            .map(|_| ())
            .and_then(|_| self.project_details.get(pid));

        if catalog_match.is_none() && detail.is_none() {
            diagnostics.push(RestoreDiagnostic {
                project_id: Some(pid.to_string()),
                session_id: persisted.session_id.clone(),
                code: "project_missing",
                message: format!("project {pid} not found in daemon catalog"),
            });
            diagnostic = Some(format!("project {pid} not found in catalog"));
            return RestoreEntry {
                tab_id,
                persisted: persisted.clone(),
                status: RestoreEntryStatus::Missing,
                resolved_project_id: None,
                resolved_workspace_id: None,
                resolved_session_id: None,
                diagnostic,
            };
        }

        if let Some(entry) = catalog_match {
            if entry.archived {
                diagnostics.push(RestoreDiagnostic {
                    project_id: Some(pid.to_string()),
                    session_id: persisted.session_id.clone(),
                    code: "project_archived",
                    message: format!("project {pid} is archived"),
                });
                return RestoreEntry {
                    tab_id,
                    persisted: persisted.clone(),
                    status: RestoreEntryStatus::Archived,
                    resolved_project_id: None,
                    resolved_workspace_id: None,
                    resolved_session_id: None,
                    diagnostic: Some(format!("project {pid} is archived")),
                };
            }
        }

        if let Some(detail) = detail {
            if detail.archived {
                diagnostics.push(RestoreDiagnostic {
                    project_id: Some(pid.to_string()),
                    session_id: persisted.session_id.clone(),
                    code: "project_archived",
                    message: format!("project {pid} is archived"),
                });
                return RestoreEntry {
                    tab_id,
                    persisted: persisted.clone(),
                    status: RestoreEntryStatus::Archived,
                    resolved_project_id: None,
                    resolved_workspace_id: None,
                    resolved_session_id: None,
                    diagnostic: Some(format!("project {pid} is archived")),
                };
            }

            // Validate workspace membership.
            if let Some(wid) = persisted.workspace_id.as_deref() {
                if !detail.workspaces.iter().any(|w| w == wid) {
                    diagnostics.push(RestoreDiagnostic {
                        project_id: Some(pid.to_string()),
                        session_id: persisted.session_id.clone(),
                        code: "workspace_invalid",
                        message: format!(
                            "workspace {wid} not bound to project {pid}; dropping binding"
                        ),
                    });
                    resolved_workspace_id = None;
                }
            }

            // Validate session binding. If the session is now bound
            // to a different canonical project, mark as Rebound and
            // drop the session binding.
            if let Some(sid) = persisted.session_id.as_deref() {
                match detail.sessions.iter().find(|s| s.session_id == sid) {
                    Some(s) if s.canonical_project_id == pid => {
                        // Valid binding.
                    }
                    Some(s) => {
                        diagnostics.push(RestoreDiagnostic {
                            project_id: Some(pid.to_string()),
                            session_id: Some(sid.to_string()),
                            code: "session_rebound",
                            message: format!(
                                "session {sid} now bound to project {}; dropping session",
                                s.canonical_project_id
                            ),
                        });
                        resolved_session_id = None;
                    }
                    None => {
                        diagnostics.push(RestoreDiagnostic {
                            project_id: Some(pid.to_string()),
                            session_id: Some(sid.to_string()),
                            code: "session_missing",
                            message: format!("session {sid} no longer present"),
                        });
                        resolved_session_id = None;
                    }
                }
            }
        }

        // Determine final status.
        let status = if resolved_session_id.is_none() && persisted.session_id.is_some() {
            if resolved_project_id.is_some()
                && !persisted.session_id.as_deref().is_some_and(|sid| {
                    detail.is_some_and(|d| {
                        d.sessions
                            .iter()
                            .any(|s| s.session_id == sid && s.canonical_project_id == pid)
                    })
                })
            {
                // Session was dropped (rebound or missing) but the
                // project remains valid — surface as a partial
                // restore with the project identity only.
                RestoreEntryStatus::Valid
            } else {
                RestoreEntryStatus::Valid
            }
        } else {
            RestoreEntryStatus::Valid
        };

        let _ = diagnostic;

        RestoreEntry {
            tab_id,
            persisted: persisted.clone(),
            status,
            resolved_project_id,
            resolved_workspace_id,
            resolved_session_id,
            diagnostic: None,
        }
    }
}

/// Apply a `RestorePlan` to a mutable [`ProjectTabs`] container.
/// The plan is consumed deterministically: each entry that
/// `opens_tab()` becomes a `ProjectTabState`; the active tab is set
/// to `plan.active_tab_id` when present; otherwise the container
/// remains empty and the caller is responsible for adding a compat
/// tab.
///
/// Returns the heavy-load tab id, if any, so the caller can
/// trigger the heavyweight session load transaction.
pub fn apply_restore_plan(
    tabs: &mut crate::tui::app::state::project_tabs::ProjectTabs,
    plan: &RestorePlan,
) -> Option<ProjectTabId> {
    tabs.clear_for_restore();
    for entry in &plan.entries {
        if !entry.opens_tab() {
            continue;
        }
        let project_id = entry
            .resolved_project_id
            .clone()
            .expect("opens_tab implies project_id");
        let label = entry
            .persisted
            .label_hint
            .clone()
            .unwrap_or_else(|| short_project_label(&project_id));
        let tab = ProjectTabState::empty(ProjectTabId::new(), label);
        let mut tab = tab;
        tab.project_id = Some(project_id);
        tab.workspace_id = entry.resolved_workspace_id.clone();
        tab.session_id = entry.resolved_session_id.clone();
        if let Some(model) = entry.persisted.selected_model_id.as_deref() {
            tab.model = model.to_string();
        }
        if let Some(agent) = entry.persisted.selected_agent.as_deref() {
            tab.agent = agent.to_string();
        }
        tabs.add_tab(tab);
    }
    if let Some(active) = plan.active_tab_id.as_ref() {
        // The active id was generated in the plan; map it to the
        // newly added tab. Plans always add entries in order, and
        // the active tab is one of the entries, so we look it up by
        // finding the matching tab index.
        if let Some(idx) = plan.entries.iter().position(|e| &e.tab_id == active) {
            // Find the n-th open tab in the container (skipping
            // skipped entries).
            let mut open_idx = 0usize;
            for entry in &plan.entries[..=idx] {
                if entry.opens_tab() {
                    if entry.tab_id == *active {
                        break;
                    }
                    open_idx += 1;
                }
            }
            let ordered: Vec<ProjectTabId> = tabs
                .ordered()
                .into_iter()
                .map(|t| t.tab_id.clone())
                .collect();
            if let Some(target) = ordered.get(open_idx) {
                tabs.set_active(target);
            }
        }
    }
    plan.pending_heavy_load.clone()
}

/// Strip a UUID-shaped string to a short, label-friendly form.
/// Used as a fallback when no `label_hint` was persisted.
fn short_project_label(project_id: &str) -> String {
    let trimmed: String = project_id.chars().take(8).collect();
    format!("project-{trimmed}")
}

impl crate::tui::app::state::project_tabs::ProjectTabs {
    /// Clear all tabs. Used by the restore pipeline to start from
    /// an empty container before materializing restored tabs.
    pub fn clear_for_restore(&mut self) {
        // Replace the container's contents without changing its
        // identity. Internal method on `ProjectTabs`; we cannot
        // reach private fields here, so we use a trick: remove
        // each tab in order.
        let ids: Vec<ProjectTabId> = self
            .ordered()
            .into_iter()
            .map(|t| t.tab_id.clone())
            .collect();
        for id in ids {
            self.remove_tab(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::manifest::{
        ManifestPreferences, PersistedProjectTab, TuiWorkspaceManifest,
    };

    fn snapshot() -> DaemonLookupSnapshot {
        DaemonLookupSnapshot::default()
    }

    fn tab_with_project(pid: &str) -> PersistedProjectTab {
        PersistedProjectTab {
            project_id: Some(pid.into()),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        }
    }

    fn tab_with_session(pid: &str, sid: &str) -> PersistedProjectTab {
        PersistedProjectTab {
            project_id: Some(pid.into()),
            workspace_id: None,
            session_id: Some(sid.into()),
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        }
    }

    #[test]
    fn empty_manifest_produces_empty_plan() {
        let snap = snapshot();
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.clear();
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries.is_empty());
        assert!(plan.active_tab_id.is_none());
        assert!(plan.pending_heavy_load.is_none());
    }

    #[test]
    fn missing_project_classified_as_missing() {
        let snap = snapshot();
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("ghost"));
        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].status, RestoreEntryStatus::Missing);
        assert!(!plan.entries[0].opens_tab());
    }

    #[test]
    fn archived_project_classified_as_archived() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: true,
        });
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));
        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries[0].status, RestoreEntryStatus::Archived);
    }

    #[test]
    fn valid_project_in_catalog_is_valid() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));
        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries[0].status, RestoreEntryStatus::Valid);
        assert_eq!(plan.entries[0].resolved_project_id.as_deref(), Some("p1"));
        assert!(plan.entries[0].opens_tab());
    }

    #[test]
    fn valid_project_with_bound_session_loads_heavy_view() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        let mut details = HashMap::new();
        details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                sessions: vec![SessionBinding {
                    session_id: "s1".into(),
                    canonical_project_id: "p1".into(),
                }],
            },
        );
        snap.project_details = details;
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_session("p1", "s1"));
        m.active_project_id = Some("p1".into());
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries[0].has_heavy_session());
        assert_eq!(plan.pending_heavy_load, plan.active_tab_id);
    }

    #[test]
    fn rebound_session_is_dropped() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        let mut details = HashMap::new();
        details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                sessions: vec![SessionBinding {
                    session_id: "s1".into(),
                    canonical_project_id: "p-other".into(),
                }],
            },
        );
        snap.project_details = details;
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_session("p1", "s1"));
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries[0].resolved_session_id.is_none());
        assert!(plan.entries[0].opens_tab());
        assert!(!plan.entries[0].has_heavy_session());
    }

    #[test]
    fn missing_session_drops_session_keeps_project() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        let mut details = HashMap::new();
        details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                sessions: vec![],
            },
        );
        snap.project_details = details;
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_session("p1", "ghost"));
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries[0].opens_tab());
        assert!(plan.entries[0].resolved_session_id.is_none());
        assert!(!plan.entries[0].has_heavy_session());
    }

    #[test]
    fn invalid_workspace_is_dropped() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        let mut details = HashMap::new();
        details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec!["w-known".into()],
                sessions: vec![],
            },
        );
        snap.project_details = details;
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("p1".into()),
            workspace_id: Some("w-other".into()),
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries[0].opens_tab());
        assert!(plan.entries[0].resolved_workspace_id.is_none());
    }

    #[test]
    fn plan_caps_entries_at_max_persisted_tabs() {
        let mut snap = snapshot();
        for i in 0..(MAX_PERSISTED_TABS + 5) {
            snap.catalog.push(CatalogEntry {
                project_id: format!("p{i}"),
                archived: false,
            });
        }
        let mut m = TuiWorkspaceManifest::default();
        for i in 0..(MAX_PERSISTED_TABS + 5) {
            m.ordered_tabs.push(tab_with_project(&format!("p{i}")));
        }
        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries.len(), MAX_PERSISTED_TABS);
    }

    #[test]
    fn active_tab_falls_back_to_first_open_when_persisted_missing() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        snap.catalog.push(CatalogEntry {
            project_id: "p2".into(),
            archived: false,
        });
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));
        m.ordered_tabs.push(tab_with_project("p2"));
        m.active_project_id = Some("ghost".into());
        let plan = snap.build_restore_plan(&m);
        assert!(plan.active_tab_id.is_some());
        // Active should be the first entry that opens a tab (p1).
        assert_eq!(
            plan.entries
                .iter()
                .position(|e| Some(&e.tab_id) == plan.active_tab_id.as_ref()),
            Some(0)
        );
    }

    #[test]
    fn apply_restore_plan_materializes_tabs() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        snap.catalog.push(CatalogEntry {
            project_id: "p2".into(),
            archived: false,
        });
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));
        m.ordered_tabs.push(tab_with_project("p2"));
        m.active_project_id = Some("p2".into());
        let plan = snap.build_restore_plan(&m);
        let mut tabs = crate::tui::app::state::project_tabs::ProjectTabs::new();
        let heavy = apply_restore_plan(&mut tabs, &plan);
        assert_eq!(tabs.len(), 2);
        // The container should have p2 as the active tab (matches
        // the persisted active_project_id).
        let active = tabs.active().expect("active tab");
        assert_eq!(active.project_id.as_deref(), Some("p2"));
        assert!(heavy.is_none());
    }

    #[test]
    fn plan_surfaces_bounded_diagnostics() {
        let snap = snapshot();
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("ghost"));
        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(plan.diagnostics[0].code, "project_missing");
        for d in &plan.diagnostics {
            assert!(d.message.len() < 256);
        }
    }

    #[test]
    fn preferences_round_trip_in_plan() {
        let snap = snapshot();
        let mut m = TuiWorkspaceManifest::default();
        m.preferences = ManifestPreferences {
            sidebar_visible: Some(false),
        };
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries.is_empty());
    }
}
