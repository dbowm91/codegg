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
//! - M004: the persisted `selected_model_id` is a display-only hint.
//!   The daemon-owned durable selection is authoritative: once a
//!   `SnapshotSession`/selection snapshot arrives,
//!   [`reconcile_tab_model_with_daemon`] overwrites the hint and the
//!   next manifest write persists the daemon value.

use std::collections::HashMap;
use std::path::PathBuf;

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
    /// Daemon-authoritative workspace root for the resolved workspace.
    /// This is captured during restore so the active tab never falls
    /// back to the process launch directory while its session loads.
    pub resolved_workspace_root: Option<PathBuf>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectDetailSnapshot {
    pub project_id: String,
    pub archived: bool,
    /// Workspace ids known to the daemon for this project. Used to
    /// validate persisted `workspace_id`.
    pub workspaces: Vec<String>,
    /// Canonical roots keyed by workspace id. Roots are routing data,
    /// not project identity, and are copied only into the live tab.
    pub workspace_roots: HashMap<String, PathBuf>,
    /// Session ids known to the daemon for this project, each
    /// tagged with the canonical project binding. Used to detect
    /// `Rebound` and `Missing`.
    pub sessions: Vec<SessionBinding>,
    /// Whether `sessions` is an authoritative enumeration of the
    /// project's sessions, as opposed to "the daemon did not tell
    /// us".
    ///
    /// `ProjectDetailsDto` carries only a `session_count`, never the
    /// session list, so the TUI adapter always leaves this `false`.
    /// An empty `sessions` vec therefore means *unknown*, not *none*:
    /// treating it as an enumeration made every restored tab emit a
    /// spurious `session_missing` diagnostic and silently drop a
    /// session that still existed. Session binding validation runs
    /// only when this is `true`.
    pub sessions_known: bool,
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
                resolved_workspace_root: None,
                resolved_session_id: None,
                diagnostic: Some("missing project_id".into()),
            };
        };

        // First check the catalog fast path.
        let catalog_match = self.catalog.iter().find(|e| e.project_id == pid);

        // Fall back to a per-project detail lookup.
        //
        // This lookup MUST NOT be gated on `catalog_match`: the TUI
        // deliberately issues `ProjectGet` for exactly those projects
        // the catalog has not reported yet, so the detail snapshot is
        // the only evidence available for them. Gating it on the
        // catalog match made every such fetch useless — the freshly
        // loaded detail was fetched, handed to the coordinator, and
        // then ignored, so the entry was classified `project_missing`
        // and the tab was dropped even though the daemon had just
        // confirmed the project exists.
        let detail = self.project_details.get(pid);

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
                resolved_workspace_root: None,
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
                    resolved_workspace_root: None,
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
                    resolved_workspace_root: None,
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
            //
            // Only runs when the detail snapshot actually enumerates
            // the project's sessions. When the daemon only reported a
            // session count, the persisted binding is preserved
            // untouched — an unenumerated list is not evidence that
            // the session is gone.
            if let Some(sid) = persisted
                .session_id
                .as_deref()
                .filter(|_| detail.sessions_known)
            {
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
        //
        // A project that resolved keeps its tab regardless of whether
        // the session binding survived validation; only the binding
        // itself is affected, and that is already reflected in
        // `resolved_session_id`. Every arm below therefore collapsed
        // to `Valid`, so the status is stated directly. A tab is
        // opened whenever the project identity resolved, and the
        // session binding is dropped only by the explicit validation
        // arms above.
        let status = RestoreEntryStatus::Valid;

        let _ = diagnostic;

        let resolved_workspace_root = resolved_workspace_id
            .as_deref()
            .and_then(|workspace_id| detail.and_then(|d| d.workspace_roots.get(workspace_id)))
            .cloned();

        RestoreEntry {
            tab_id,
            persisted: persisted.clone(),
            status,
            resolved_project_id,
            resolved_workspace_id,
            resolved_workspace_root,
            resolved_session_id,
            diagnostic: None,
        }
    }
}

/// M004: reconcile a restored tab's display-hint model with the
/// daemon-owned durable selection.
///
/// The manifest hint is applied first by [`apply_restore_plan`] so the
/// UI has something to render before the daemon responds. Once the
/// daemon `SnapshotSession`/selection snapshot arrives, the daemon
/// value wins: when `daemon_model` is `Some`, the tab is overwritten
/// (even if the hint was empty or stale) and the caller should persist
/// on the next manifest write. Returns `true` when the tab changed.
pub fn reconcile_tab_model_with_daemon(
    tab: &mut ProjectTabState,
    daemon_model: Option<&str>,
) -> bool {
    match daemon_model {
        Some(daemon) if tab.model != daemon => {
            tab.model = daemon.to_string();
            true
        }
        _ => false,
    }
}

/// The binding this process established at launch, captured before the
/// restore plan clears the tab container.
///
/// `root` is `None` in compat mode — the launch directory could not be
/// canonicalized — and there is nothing to preserve then.
#[derive(Debug, Clone, Default)]
pub struct LaunchBinding {
    pub root: Option<std::path::PathBuf>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub label: String,
    pub model: String,
    pub agent: String,
}

impl LaunchBinding {
    /// Capture the launch binding from the currently active tab.
    pub fn capture(tabs: &crate::tui::app::state::project_tabs::ProjectTabs) -> Option<Self> {
        tabs.active().map(|tab| Self {
            root: tab.workspace_root.clone(),
            project_id: tab.project_id.clone(),
            workspace_id: tab.workspace_id.clone(),
            session_id: tab.session_id.clone(),
            label: tab.label.clone(),
            model: tab.model.clone(),
            agent: tab.agent.clone(),
        })
    }
}

/// What [`preserve_launch_binding`] had to do, so the caller can report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBindingOutcome {
    /// Nothing to preserve (compat mode), or the restored active tab already
    /// points at the launch directory. The ordinary case, and the only one
    /// that leaves the plan's result untouched.
    Unchanged,
    /// The plan restored a tab for the launch directory but activated a
    /// different one; that tab was focused.
    FocusedExistingTab,
    /// The launch directory was absent from the plan and its tab was
    /// re-established and activated.
    Reestablished,
}

/// Keep the session on the directory the user launched in.
///
/// The manifest is *user*-scoped, not workspace-scoped, so it can
/// legitimately name a project from an earlier session — and
/// [`apply_restore_plan`] clears every tab before installing the restored
/// ones. Launching `codegg` inside a git worktree could therefore activate a
/// tab rooted in whatever checkout the previous session used, with nothing on
/// screen saying so.
///
/// That matters because `tab.workspace_root` anchors every relative path in
/// the session (tools, `@` completions, the file tree). Retargeting it
/// silently moves the whole session into a tree the user never opened — the
/// same failure class as a tool reading the daemon's launch directory instead
/// of the workspace root, reached from the other end.
///
/// Returns the heavy-load target unchanged when the launch binding survived,
/// and `None` when the active tab changed: the plan's heavy load targets a
/// session the user is no longer looking at.
///
/// Reporting the disagreement is the caller's job — this function only
/// decides. Silently correcting would be the opposite mistake (the user may
/// have meant to resume the older project); what is never acceptable is the
/// two disagreeing invisibly.
pub fn preserve_launch_binding(
    tabs: &mut crate::tui::app::state::project_tabs::ProjectTabs,
    launch: Option<LaunchBinding>,
    heavy_target: Option<crate::tui::app::state::project_tabs::ProjectTabId>,
) -> (
    LaunchBindingOutcome,
    Option<crate::tui::app::state::project_tabs::ProjectTabId>,
) {
    let Some(launch) = launch else {
        return (LaunchBindingOutcome::Unchanged, heavy_target);
    };
    let Some(root) = launch.root.clone() else {
        return (LaunchBindingOutcome::Unchanged, heavy_target);
    };
    // The restored active tab already points at the launch directory: the
    // manifest and the launch agree.
    if tabs
        .active()
        .and_then(|tab| tab.workspace_root.clone())
        .as_ref()
        == Some(&root)
    {
        return (LaunchBindingOutcome::Unchanged, heavy_target);
    }

    // Restored, but not active. Focus the matching tab rather than
    // re-creating a binding that already exists.
    let restored_match = tabs
        .ordered()
        .into_iter()
        .find(|tab| tab.workspace_root.as_ref() == Some(&root))
        .map(|tab| tab.tab_id.clone());
    if let Some(tab_id) = restored_match {
        if tabs.set_active(&tab_id) {
            return (LaunchBindingOutcome::FocusedExistingTab, None);
        }
        return (LaunchBindingOutcome::Unchanged, heavy_target);
    }

    // Absent from the plan. Re-establish the launch tab, carrying the
    // identities `bootstrap_launch_project_context` registered for it.
    let mut tab = crate::tui::app::state::project_tabs::ProjectTabState::empty(
        crate::tui::app::state::project_tabs::ProjectTabId::new(),
        launch.label.clone(),
    );
    tab.project_id = launch.project_id.clone();
    tab.workspace_id = launch.workspace_id.clone();
    tab.session_id = launch.session_id.clone();
    tab.workspace_root = Some(root);
    tab.model = launch.model.clone();
    tab.agent = launch.agent.clone();
    tabs.add_and_activate(tab);
    (LaunchBindingOutcome::Reestablished, None)
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
        tab.workspace_root = entry.resolved_workspace_root.clone();
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
    use crate::tui::app::state::project_tabs::ProjectTabs;

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

    /// A project the catalog has NOT reported yet, but for which a
    /// `ProjectGet` detail landed, must restore.
    ///
    /// The TUI issues `ProjectGet` precisely for projects missing from
    /// the catalog, so gating the detail lookup on a catalog match made
    /// the freshly fetched evidence unusable: the entry was classified
    /// `project_missing` and the tab was dropped even though the daemon
    /// had just confirmed the project exists.
    #[test]
    fn project_detail_resolves_a_project_absent_from_the_catalog() {
        let mut snap = snapshot();
        // Catalog intentionally empty — this is the startup race.
        assert!(snap.catalog.is_empty());
        snap.project_details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: false,
            },
        );
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));
        m.active_project_id = Some("p1".into());

        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries[0].status, RestoreEntryStatus::Valid);
        assert_eq!(plan.entries[0].resolved_project_id.as_deref(), Some("p1"));
        assert!(
            plan.entries[0].opens_tab(),
            "detail alone must open the tab"
        );
        assert!(
            plan.diagnostics.is_empty(),
            "a project proven by ProjectGet must not emit project_missing: {:?}",
            plan.diagnostics
        );
        assert!(plan.active_tab_id.is_some());
    }

    /// `ProjectDetailsDto` carries only a session *count*, so the TUI
    /// adapter always reports the session list as unenumerated. An
    /// unenumerated list must not be read as "the session is gone".
    #[test]
    fn an_unenumerated_session_list_preserves_the_persisted_binding() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        snap.project_details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: false,
            },
        );
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_session("p1", "s1"));
        m.active_project_id = Some("p1".into());

        let plan = snap.build_restore_plan(&m);
        assert_eq!(
            plan.entries[0].resolved_session_id.as_deref(),
            Some("s1"),
            "a live session must survive a restore that could not enumerate sessions"
        );
        assert!(plan.entries[0].has_heavy_session());
        assert!(
            !plan.diagnostics.iter().any(|d| d.code == "session_missing"),
            "unknown session list must not emit session_missing: {:?}",
            plan.diagnostics
        );
    }

    /// An *enumerated* list that lacks the session is authoritative and
    /// must still drop the binding.
    #[test]
    fn an_enumerated_session_list_without_the_session_still_drops_it() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "p1".into(),
            archived: false,
        });
        snap.project_details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: false,
                workspaces: vec![],
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: true,
            },
        );
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_session("p1", "ghost"));

        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries[0].opens_tab());
        assert_eq!(plan.entries[0].resolved_session_id, None);
        assert!(!plan.entries[0].has_heavy_session());
        assert!(plan.diagnostics.iter().any(|d| d.code == "session_missing"));
    }

    /// An archived project is still skipped even when only a detail
    /// proves it — the catalog fast path is not the only evidence.
    #[test]
    fn archived_project_found_only_via_detail_is_still_skipped() {
        let mut snap = snapshot();
        snap.project_details.insert(
            "p1".to_string(),
            ProjectDetailSnapshot {
                project_id: "p1".into(),
                archived: true,
                workspaces: vec![],
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: false,
            },
        );
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(tab_with_project("p1"));

        let plan = snap.build_restore_plan(&m);
        assert_eq!(plan.entries[0].status, RestoreEntryStatus::Archived);
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
                workspace_roots: HashMap::new(),
                sessions: vec![SessionBinding {
                    session_id: "s1".into(),
                    canonical_project_id: "p1".into(),
                }],
                sessions_known: true,
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
                workspace_roots: HashMap::new(),
                sessions: vec![SessionBinding {
                    session_id: "s1".into(),
                    canonical_project_id: "p-other".into(),
                }],
                sessions_known: true,
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
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: true,
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
                workspace_roots: HashMap::new(),
                sessions: vec![],
                sessions_known: true,
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
    fn restore_carries_canonical_workspace_root_into_tab() {
        let mut snap = snapshot();
        snap.catalog.push(CatalogEntry {
            project_id: "project-b".into(),
            archived: false,
        });
        let mut workspace_roots = HashMap::new();
        workspace_roots.insert("workspace-b".into(), PathBuf::from("/projects/b"));
        snap.project_details.insert(
            "project-b".into(),
            ProjectDetailSnapshot {
                project_id: "project-b".into(),
                archived: false,
                workspaces: vec!["workspace-b".into()],
                workspace_roots,
                sessions: vec![],
                sessions_known: true,
            },
        );
        let mut manifest = TuiWorkspaceManifest {
            active_project_id: Some("project-b".into()),
            ..TuiWorkspaceManifest::default()
        };
        manifest.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("project-b".into()),
            workspace_id: Some("workspace-b".into()),
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: None,
        });

        let plan = snap.build_restore_plan(&manifest);
        let mut tabs = crate::tui::app::state::project_tabs::ProjectTabs::new();
        apply_restore_plan(&mut tabs, &plan);
        assert_eq!(
            tabs.active().and_then(|tab| tab.workspace_root.clone()),
            Some(PathBuf::from("/projects/b"))
        );
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
        let m = TuiWorkspaceManifest {
            preferences: ManifestPreferences {
                sidebar_visible: Some(false),
            },
            ..Default::default()
        };
        let plan = snap.build_restore_plan(&m);
        assert!(plan.entries.is_empty());
    }

    #[test]
    fn daemon_model_wins_over_manifest_hint() {
        use crate::tui::app::state::project_tabs::{ProjectTabId, ProjectTabState};
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), "p1".into());
        tab.model = "openai/stale-model".to_string();
        assert!(reconcile_tab_model_with_daemon(
            &mut tab,
            Some("openai/gpt-4o")
        ));
        assert_eq!(tab.model, "openai/gpt-4o");
        // Idempotent when already matching.
        assert!(!reconcile_tab_model_with_daemon(
            &mut tab,
            Some("openai/gpt-4o")
        ));
        // No daemon snapshot yet: hint is kept.
        assert!(!reconcile_tab_model_with_daemon(&mut tab, None));
        assert_eq!(tab.model, "openai/gpt-4o");
    }

    // ---- launch-binding guard -------------------------------------------------
    //
    // The failure these cover is invisible by construction: the tab container is
    // cleared and re-rooted, so nothing on screen reports the swap. Assert on
    // the active tab's `workspace_root`, which is what every relative path in
    // the session actually resolves against.

    fn launch_binding(root: &str) -> LaunchBinding {
        LaunchBinding {
            root: Some(std::path::PathBuf::from(root)),
            project_id: Some("p-launch".into()),
            workspace_id: Some("w-launch".into()),
            session_id: Some("s-launch".into()),
            label: "launch".into(),
            model: "openai/gpt-4o".into(),
            agent: "build".into(),
        }
    }

    fn rooted_tab(label: &str, root: &str) -> ProjectTabState {
        let mut tab = ProjectTabState::empty(ProjectTabId::new(), label.into());
        tab.project_id = Some(format!("p-{label}"));
        tab.workspace_id = Some(format!("w-{label}"));
        tab.session_id = Some(format!("s-{label}"));
        tab.workspace_root = Some(std::path::PathBuf::from(root));
        tab.model = "openai/gpt-4o".into();
        tab.agent = "build".into();
        tab
    }

    fn active_root(tabs: &ProjectTabs) -> Option<std::path::PathBuf> {
        tabs.active().and_then(|tab| tab.workspace_root.clone())
    }

    #[test]
    fn a_restore_agreeing_with_the_launch_directory_changes_nothing() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(rooted_tab("launch", "/repo"));
        let heavy = tabs.active().map(|t| t.tab_id.clone());

        let (outcome, target) =
            preserve_launch_binding(&mut tabs, Some(launch_binding("/repo")), heavy.clone());

        assert_eq!(outcome, LaunchBindingOutcome::Unchanged);
        assert_eq!(
            target, heavy,
            "an agreeing restore must keep the heavy load"
        );
        assert_eq!(
            active_root(&tabs).as_deref(),
            Some(std::path::Path::new("/repo"))
        );
        assert_eq!(tabs.len(), 1, "no extra tab invented for the agreeing case");
    }

    #[test]
    fn a_restore_rooted_elsewhere_does_not_retarget_the_launch_directory() {
        // The reported failure: `apply_restore_plan` cleared the launch tab and
        // installed a tab rooted in the previous session's checkout, leaving the
        // session anchored to a tree the user never opened.
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(rooted_tab("launch", "/repo"));
        let launch = LaunchBinding::capture(&tabs);
        assert!(launch.is_some());

        // ... the plan runs and roots the active tab somewhere else ...
        tabs.clear_for_restore();
        tabs.add_and_activate(rooted_tab("old", "/other/checkout"));

        let (outcome, target) = preserve_launch_binding(&mut tabs, launch, None);

        assert_eq!(outcome, LaunchBindingOutcome::Reestablished);
        assert_eq!(
            target, None,
            "the plan's heavy load targets a tab we are leaving"
        );
        assert_eq!(
            active_root(&tabs).as_deref(),
            Some(std::path::Path::new("/repo")),
            "the launch directory must win over the restored root"
        );
        let active = tabs.active().expect("a tab is active");
        assert_eq!(active.project_id.as_deref(), Some("p-launch"));
        assert_eq!(active.workspace_id.as_deref(), Some("w-launch"));
        assert_eq!(active.session_id.as_deref(), Some("s-launch"));
        assert_eq!(
            active.model, "openai/gpt-4o",
            "model choice is carried, not reset"
        );
        assert_eq!(active.agent, "build");
    }

    #[test]
    fn a_restored_tab_for_the_launch_directory_is_focused_not_duplicated() {
        let mut tabs = ProjectTabs::new();
        tabs.add_and_activate(rooted_tab("launch", "/repo"));
        let launch = LaunchBinding::capture(&tabs);

        tabs.clear_for_restore();
        let old_id = tabs.add_tab(rooted_tab("old", "/other/checkout"));
        let keep_id = tabs.add_tab(rooted_tab("launch", "/repo"));
        tabs.set_active(&old_id);

        let (outcome, target) = preserve_launch_binding(&mut tabs, launch, None);

        assert_eq!(outcome, LaunchBindingOutcome::FocusedExistingTab);
        assert_eq!(target, None);
        assert_eq!(
            tabs.active().map(|t| t.tab_id.clone()),
            Some(keep_id),
            "the existing launch tab is focused rather than re-created"
        );
        assert_eq!(tabs.len(), 2, "focusing must not duplicate the tab");
    }

    #[test]
    fn compat_mode_preserves_nothing_and_keeps_the_heavy_load() {
        let mut tabs = ProjectTabs::new();
        let heavy = tabs.add_and_activate(rooted_tab("launch", "/repo"));
        // Launch directory could not be canonicalized: nothing to preserve.
        let compat = LaunchBinding {
            root: None,
            ..LaunchBinding::default()
        };

        let (outcome, target) =
            preserve_launch_binding(&mut tabs, Some(compat), Some(heavy.clone()));

        assert_eq!(outcome, LaunchBindingOutcome::Unchanged);
        assert_eq!(target, Some(heavy));
    }

    #[test]
    fn an_absent_capture_preserves_nothing_and_keeps_the_heavy_load() {
        let mut tabs = ProjectTabs::new();
        let heavy = tabs.add_and_activate(rooted_tab("launch", "/repo"));

        let (outcome, target) = preserve_launch_binding(&mut tabs, None, Some(heavy.clone()));

        assert_eq!(outcome, LaunchBindingOutcome::Unchanged);
        assert_eq!(target, Some(heavy));
    }
}
