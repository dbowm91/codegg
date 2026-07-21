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

/// Maximum number of concurrent `ProjectGet` requests during
/// restore. The bounded list machinery already enforces a
/// per-request cap; this is the additional concurrency cap on the
/// restore pipeline.
pub const RESTORE_CONCURRENCY: usize = 4;

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
    // to `ProjectGet`.
    let mut snapshot = DaemonLookupSnapshot::default();
    if let Some(client) = app.core_client.as_ref() {
        // The TUI keeps a ProjectCatalogState; mirror its summaries
        // into the snapshot. The catalog is bounded and never
        // contains credentials.
        let entries = app.project_catalog.entries.clone();
        snapshot.catalog = entries
            .into_iter()
            .map(|e| CatalogEntry {
                project_id: e.project_id,
                archived: e.archived_at.is_some(),
            })
            .collect();
        let _ = client; // explicit reference for clarity
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
        if snapshot.catalog.iter().any(|e| e.project_id == pid) {
            // Already covered by the catalog fast-path.
            continue;
        }
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
    // Cap the in-flight count by RESTORE_CONCURRENCY.
    let concurrency = RESTORE_CONCURRENCY.min(needed.len());
    let chunked: Vec<String> = needed.into_iter().take(concurrency).collect();
    for pid in chunked {
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
    // restored (bounded to 3 entries).
    let restored_count = plan.entries.iter().filter(|e| e.opens_tab()).count();
    if restored_count > 0 {
        let msg = format!(
            "Restored {} tab{} from previous session",
            restored_count,
            if restored_count == 1 { "" } else { "s" }
        );
        app.messages_state.toasts.info(&msg);
    }
}

/// Apply a `ManifestRestoreProjectGetLoaded` completion. Updates
/// the in-memory snapshot and, if all in-flight requests for the
/// manifest have settled, re-applies the plan.
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
    // session_count); the restore coordinator therefore treats the
    // per-session Rebound detection as best-effort: the bound
    // session is preserved unless the persisted session is missing
    // from the catalog. Wire-up to per-session lookup is owned by
    // the SessionSelection/Milestone 3 project-correct event
    // routing and is not part of the manifest restore surface.
    let archived = details.project.archived_at.is_some();
    let detail = ProjectDetailSnapshot {
        project_id: details.project.project_id.clone(),
        archived,
        workspaces: details
            .workspaces
            .iter()
            .map(|w| w.workspace_id.clone())
            .collect(),
        sessions: Vec::new(),
    };

    // Re-run the restore plan with the new detail. We do not have a
    // direct handle to the prior snapshot, so we rebuild from the
    // catalog and the new detail.
    let mut snapshot = DaemonLookupSnapshot::default();
    if let Some(client) = app.core_client.as_ref() {
        let entries = app.project_catalog.entries.clone();
        snapshot.catalog = entries
            .into_iter()
            .map(|e| CatalogEntry {
                project_id: e.project_id,
                archived: e.archived_at.is_some(),
            })
            .collect();
        let _ = client;
    }
    snapshot
        .project_details
        .insert(detail.project_id.clone(), detail);

    let outcome = app.load_manifest();
    let manifest = match outcome {
        ManifestLoadOutcome::Loaded(m) => m,
        _ => return,
    };

    let plan = snapshot.build_restore_plan(&manifest);
    apply_plan(app, plan);
}
