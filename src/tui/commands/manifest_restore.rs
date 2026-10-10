//! TUI commands for milestone 4 manifest restore.
//!
//! The pipeline is implemented as a coordinator that runs on the TUI
//! command channel. The single entry point `apply_manifest_restore`
//! is dispatched on startup (or on explicit operator request) and
//! drives the following sequence:
//!
//! 1. Load the manifest if not already loaded. Records the
//!    `daemon_instance_hint` for diagnostic purposes.
//! 2. If the manifest is empty/rejected, leave the TUI in its
//!    compat single-tab mode and exit.
//! 3. Issue bounded `ProjectGet` requests for each persisted
//!    project in parallel (capped). Completions update an internal
//!    daemon snapshot.
//! 4. When all in-flight requests complete (or are cancelled), build
//!    a [`crate::tui::app::state::restore::RestorePlan`] and apply
//!    it to `App::project_tabs`.
//! 5. The plan emits at most one `pending_heavy_load` tab; the
//!    heavy-load transaction reuses the existing milestone 3 view
//!    switch machinery.
//!
//! Cancellation: the coordinator is registered as a TUI task so the
//! existing `TuiTaskRegistry::cancel_for_tab` lifecycle applies.

use crate::tui::app::state::manifest::{ManifestLoadOutcome, TuiWorkspaceManifest};
use crate::tui::app::state::restore::{
    apply_restore_plan, CatalogEntry, DaemonLookupSnapshot, ProjectDetailSnapshot, RestorePlan,
};
use crate::tui::app::App;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;
use crate::tui::TuiCommand;

/// Re-drive the restore after new daemon evidence lands.
///
/// Safe and idempotent: it re-reads the manifest, rebuilds the
/// snapshot from the catalog plus accumulated project details,
/// spawns `ProjectGet` only for projects with no detail yet, and
/// re-applies the resulting plan. Called when the project catalog
/// completes, since the initial restore request is dispatched before
/// that round-trip can return.
pub(crate) fn replay_manifest_restore(app: &mut App) {
    if app.manifest_restore_announced {
        // A plan already opened at least one tab; the remaining
        // evidence only refines it, and re-running would reset live
        // tab state the operator may already be looking at.
        return;
    }
    apply_manifest_restore(app);
}

/// Apply the manifest restore. Idempotent: subsequent calls after a
/// successful restore are no-ops. Logs the outcome to the operator
/// toast surface and persists the normalized manifest.
pub(crate) fn apply_manifest_restore(app: &mut App) {
    // 1. Load the manifest if not already done this session.
    let outcome = app.load_manifest();
    let manifest = match outcome {
        ManifestLoadOutcome::Loaded(m) => m,
        ManifestLoadOutcome::Absent => {
            tracing::debug!(
                target: "codegg::tui::manifest",
                "no persisted manifest; staying in compat mode"
            );
            return;
        }
        ManifestLoadOutcome::Rejected(diag) => {
            tracing::info!(
                target: "codegg::tui::manifest",
                message = diag.short_message(),
                "manifest rejected; staying in compat mode"
            );
            app.ui_state
                .diagnostics
                .record_restore_diagnostic(diag.short_message());
            return;
        }
    };

    // Record the daemon instance hint for diagnostic purposes.
    app.manifest_daemon_hint = manifest.daemon_instance_hint.clone();

    if manifest.ordered_tabs.is_empty() {
        tracing::debug!(
            target: "codegg::tui::manifest",
            "manifest is empty; staying in compat mode"
        );
        return;
    }

    // 2. Build the daemon snapshot. The fast-path uses the cached
    // catalog when available; per-project detail lookups fall back
    // to `ProjectGet` results accumulated on `App`.
    let mut snapshot = build_daemon_snapshot(app);

    // The catalog is filled asynchronously by `refresh_project_catalog`.
    // On the very first pass it is still empty, and every persisted tab
    // would classify as `project_missing` — which drops the tab and
    // wipes `project_tabs` on a verdict the daemon had not actually
    // made yet. Wait for evidence instead: `ProjectCatalogRefreshed`
    // and the `ProjectGet` completions both re-drive the restore.
    if snapshot.catalog.is_empty() && snapshot.project_details.is_empty() {
        tracing::debug!(
            target: "codegg::tui::manifest",
            "daemon snapshot not populated yet; deferring restore until catalog or project detail arrives"
        );
        return;
    }

    // 3. Spawn per-project ProjectGet tasks for any project we
    // don't already have detail for. Bounded by
    // RESTORE_CONCURRENCY and the manifest's own cap.
    spawn_restore_project_gets(app, &manifest, &mut snapshot);

    // 4. Apply the plan synchronously using the snapshot built so
    // far. Subsequent completions update individual entries but
    // the TUI does not block waiting for them; the operator can
    // trigger a refresh.
    let plan = snapshot.build_restore_plan(&manifest);
    apply_plan(app, plan);
}

/// Assemble the daemon lookup snapshot the restore planner needs from
/// the catalog the TUI has cached plus every `ProjectGet` result that
/// has landed so far.
///
/// The detail map lives on `App` rather than being rebuilt per call:
/// each `ProjectGet` completion used to reconstruct the snapshot from
/// scratch and insert only its own project, so with several persisted
/// tabs every completion erased the evidence gathered by the previous
/// ones and the last one to land won.
fn build_daemon_snapshot(app: &App) -> DaemonLookupSnapshot {
    DaemonLookupSnapshot {
        catalog: app
            .project_catalog
            .entries
            .iter()
            .map(|e| CatalogEntry {
                project_id: e.project_id.clone(),
                archived: e.archived_at.is_some(),
            })
            .collect(),
        project_details: app.restore_project_details.clone(),
    }
}

/// Spawn bounded `ProjectGet` requests for any persisted project
/// not already covered by the snapshot's fast-path catalog entries.
/// Completions are sent through the TUI command channel and update
/// the snapshot incrementally; a follow-up `apply_manifest_restore`
/// call (or an explicit refresh) materializes the new state.
fn spawn_restore_project_gets(
    app: &mut App,
    manifest: &TuiWorkspaceManifest,
    snapshot: &mut DaemonLookupSnapshot,
) {
    let core_client = match app.core_client.clone() {
        Some(c) => c,
        None => return,
    };
    let mut needed: Vec<String> = Vec::new();
    for tab in &manifest.ordered_tabs {
        let Some(pid) = tab.project_id.as_deref() else {
            continue;
        };
        if snapshot.project_details.contains_key(pid) {
            continue;
        }
        if !needed.contains(&pid.to_string()) {
            needed.push(pid.to_string());
        }
    }
    if needed.is_empty() {
        return;
    }

    let tx = match app.tui_cmd_tx.clone() {
        Some(t) => t,
        None => return,
    };
    // Every needed project is requested. The previous code truncated
    // the list with `.take(RESTORE_CONCURRENCY)` and never fetched a
    // fifth project, so any manifest with more tabs than the cap
    // silently lost the tail to `project_missing`.
    for pid in needed {
        let client = core_client.clone();
        spawn_registered_tui_task(
            Some(tx.clone()),
            &mut app.task_registry,
            TuiTaskKind::Command,
            "manifest_project_get",
            async move {
                let req = crate::core::new_request(
                    format!("manifest-restore-get-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::ProjectGet {
                        project_id: pid.clone(),
                    },
                );
                let request_id: u64 = 0;
                let response = client.request(req).await;
                match response {
                    Ok(crate::protocol::core::CoreResponse::ProjectGet { project }) => {
                        Some(TuiCommand::ManifestRestoreProjectGetLoaded {
                            request_id,
                            project_id: pid,
                            result: Some(project),
                            error: None,
                        })
                    }
                    Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                        Some(TuiCommand::ManifestRestoreProjectGetLoaded {
                            request_id,
                            project_id: pid,
                            result: None,
                            error: Some(message),
                        })
                    }
                    Ok(_) => Some(TuiCommand::ManifestRestoreProjectGetLoaded {
                        request_id,
                        project_id: pid,
                        result: None,
                        error: Some("Unexpected response".to_string()),
                    }),
                    Err(e) => Some(TuiCommand::ManifestRestoreProjectGetLoaded {
                        request_id,
                        project_id: pid,
                        result: None,
                        error: Some(e.to_string()),
                    }),
                }
            },
        );
    }
}

/// Apply a [`RestorePlan`] to the TUI. The plan's pending heavy
/// load is routed through the existing view-switch coordinator so
/// the heavy session view is loaded exactly once.
fn apply_plan(app: &mut App, plan: RestorePlan) {
    if plan.entries.is_empty() {
        return;
    }

    // Track diagnostics for the operator surface.
    for diag in &plan.diagnostics {
        let message = format!("{}: {}", diag.code, diag.message);
        tracing::info!(
            target: "codegg::tui::manifest",
            code = diag.code,
            message = %diag.message,
            "restore diagnostic"
        );
        app.ui_state.diagnostics.record_restore_diagnostic(&message);
    }

    // Materialize lightweight tabs.
    let heavy_target = apply_restore_plan(&mut app.project_tabs, &plan);
    // The restored tab carries the user's persisted `provider/model` choice;
    // adopt it as the active model so the next catalog refresh cannot decide
    // the session's model on the frontend's behalf.
    app.adopt_persisted_tab_model();
    app.refresh_project_command_registry();

    // Persist the normalized manifest. The TUI's existing save
    // scheduling will debounce and write.
    app.schedule_manifest_save();

    // If the plan wants a heavy session view loaded, trigger it
    // through the existing view-switch coordinator.
    if let Some(tab_id) = heavy_target {
        let target_session = app
            .project_tabs
            .get(&tab_id)
            .and_then(|t| t.session_id.clone());
        let target_project = app
            .project_tabs
            .get(&tab_id)
            .and_then(|t| t.project_id.clone());
        if let (Some(session_id), Some(project_id)) = (target_session, target_project) {
            // Use the existing controlled switch transaction.
            super::project_picker::switch_active_tab(app, &tab_id);
            // The switch transaction will issue SnapshotSession for
            // the bound session; we just ensure the target is
            // active.
            tracing::debug!(
                target: "codegg::tui::manifest",
                tab_id = %tab_id,
                session_id = %session_id,
                project_id = %project_id,
                "queued heavy session load for restored tab"
            );
        }
    }

    // Surface the plan in a toast so the user can see what was
    // restored. The restore is re-driven as catalog and project
    // details arrive, so this fires only on the first apply that
    // actually opened a tab; later passes would otherwise repeat
    // the same message once per round-trip.
    let restored_count = plan.entries.iter().filter(|e| e.opens_tab()).count();
    if restored_count > 0 && !app.manifest_restore_announced {
        app.manifest_restore_announced = true;
        let msg = format!(
            "Restored {} tab{} from previous session",
            restored_count,
            if restored_count == 1 { "" } else { "s" }
        );
        app.messages_state.toasts.info(&msg);
    }
}

/// Apply a `ManifestRestoreProjectGetLoaded` completion. Updates
/// the accumulated per-project detail map and re-applies the plan.
pub(crate) fn apply_manifest_project_get_loaded(
    app: &mut App,
    _request_id: u64,
    project_id: String,
    result: Option<crate::protocol::dto::ProjectDetailsDto>,
    error: Option<String>,
) {
    if let Some(err) = error {
        tracing::debug!(
            target: "codegg::tui::manifest",
            project_id = %project_id,
            error = %err,
            "manifest ProjectGet failed"
        );
        return;
    }
    let Some(details) = result else {
        return;
    };

    // Build a per-project detail snapshot for the restore module.
    // `ProjectDetailsDto` does not embed a session list (only
    // session_count), so `sessions_known` stays `false`: the
    // coordinator must treat the session binding as unverifiable
    // rather than absent, otherwise a live session is dropped on
    // every restore. Per-session Rebound detection is owned by the
    // SessionSelection/Milestone 3 project-correct event routing.
    let archived = details.project.archived_at.is_some();
    let detail = ProjectDetailSnapshot {
        project_id: details.project.project_id.clone(),
        archived,
        workspaces: details
            .workspaces
            .iter()
            .map(|w| w.workspace_id.clone())
            .collect(),
        workspace_roots: details
            .workspaces
            .iter()
            .filter_map(|w| {
                w.canonical_root
                    .as_deref()
                    .map(|root| (w.workspace_id.clone(), std::path::PathBuf::from(root)))
            })
            .collect(),
        sessions: Vec::new(),
        sessions_known: false,
    };

    // Merge into the accumulated map instead of rebuilding the
    // snapshot from scratch. Rebuilding dropped every previously
    // fetched sibling, so only the last completion to land survived
    // and the other tabs regressed to `project_missing`.
    app.restore_project_details
        .insert(detail.project_id.clone(), detail);

    let outcome = app.load_manifest();
    let manifest = match outcome {
        ManifestLoadOutcome::Loaded(m) => m,
        _ => return,
    };

    let plan = build_daemon_snapshot(app).build_restore_plan(&manifest);
    apply_plan(app, plan);
}
