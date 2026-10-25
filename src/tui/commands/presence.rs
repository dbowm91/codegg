//! Collaborator presence commands (Presence M002).
//!
//! The TUI owns no presence truth. Every refresh performs an explicit
//! `PresenceCapabilities` negotiation followed by a bounded presence read
//! through the daemon-owned `CoreClient`. Results land back on the event
//! loop as `TuiCommand::PresenceSnapshotLoaded` completions guarded by
//! per-project request id + reconnect epoch.
//!
//! M007-017: a refresh a **user** asked for travels through the catalog's
//! `presence` form, so the daemon records it as a canonical `Run`. The
//! daemon's liveness hint does not. See [`PresenceRefresh`].

use crate::protocol::core::{CoreRequest, CoreResponse, PresenceSnapshotDto};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Why the presence snapshot is being re-read: the distinction is the
/// **`Run`**. A read a user asked for is a command effect the daemon audits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresenceRefresh {
    /// A user action: opening the panel, an explicit refresh, a reconnect,
    /// a tab or project switch. It travels through the catalog's form.
    User,
    /// The daemon's `PresenceUpdated` liveness hint.
    ///
    /// The hint answers a collaborator's presence heartbeat
    /// (`src/core/daemon_projection.rs:26-81`), which a client sends on a
    /// `lease_ttl / 3` cadence — every 30 seconds by default
    /// (`crates/codegg-core/src/presence.rs:148`). A read here is a read per
    /// heartbeat, so it stays on the direct request.
    Hint,
}

/// Read one project's presence snapshot. A user-triggered read travels
/// through the catalog's `presence` form; a hint-triggered one does not.
async fn read_presence_snapshot(
    core_client: &std::sync::Arc<dyn crate::core::CoreClient>,
    scope: crate::protocol::core::CommandCatalogScopeDto,
    project_id: &str,
    refresh: PresenceRefresh,
) -> Result<CoreResponse, crate::error::AppError> {
    match refresh {
        PresenceRefresh::User => {
            crate::tui::command_route::send_command(
                core_client,
                scope,
                "collaborators",
                "presence",
                project_id,
            )
            .await
        }
        PresenceRefresh::Hint => {
            let request = crate::core::new_request(
                format!("presence-snapshot-{}", uuid::Uuid::new_v4()),
                CoreRequest::PresenceSnapshotGet {
                    project_id: project_id.to_string(),
                },
            );
            core_client.request(request).await
        }
    }
}

/// Turn the daemon's answer into the completion the reducer applies. Both
/// transports answer with the same response, so the completion is built in
/// one place.
fn presence_snapshot_completion(
    request_id: u64,
    project_id: String,
    reconnect_epoch: u64,
    answer: Result<CoreResponse, crate::error::AppError>,
) -> TuiCommand {
    match answer {
        Ok(CoreResponse::PresenceSnapshot { snapshot }) => TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot: Some(snapshot),
            error: None,
            unauthorized: false,
            unsupported: false,
            reconnect_epoch,
        },
        Ok(CoreResponse::Error { code, message }) => TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot: None,
            error: Some(format!("{code}: {message}")),
            unauthorized: code == "project_not_found",
            unsupported: false,
            reconnect_epoch,
        },
        // A refusal that never reached the effect arrives as the command
        // execution's own result.
        Ok(CoreResponse::CommandExecute { result }) => {
            let (code, message) = crate::tui::command_route::refusal_parts(&result);
            TuiCommand::PresenceSnapshotLoaded {
                request_id,
                project_id,
                snapshot: None,
                error: Some(format!("{code}: {message}")),
                unauthorized: code == "project_not_found",
                unsupported: false,
                reconnect_epoch,
            }
        }
        Ok(other) => TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot: None,
            error: Some(format!("Unexpected core response: {other:?}")),
            unauthorized: false,
            unsupported: false,
            reconnect_epoch,
        },
        Err(e) => TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot: None,
            error: Some(format!("Presence snapshot request failed: {e}")),
            unauthorized: false,
            unsupported: false,
            reconnect_epoch,
        },
    }
}

/// Kick off a presence refresh for `project_id`. Capability negotiation
/// happens first: an older daemon returns `supported: false` and the panel
/// renders the generic unavailable state.
pub(crate) fn start_refresh_presence(app: &mut App, project_id: String, refresh: PresenceRefresh) {
    if project_id.is_empty() {
        return;
    }
    if app.core_client.is_none() {
        // No daemon: the same unavailable shape as an old daemon.
        let request_id = app.presence.begin_refresh(&project_id);
        let epoch = app.presence.reconnect_epoch;
        apply_presence_snapshot_loaded(
            app,
            request_id,
            project_id,
            None,
            Some("Core unavailable — check daemon status with /doctor".to_string()),
            false,
            true,
            epoch,
        );
        return;
    }
    // Coalesce: never start a duplicate fetch while one is in flight.
    if !app.presence.needs_refresh(&project_id) {
        return;
    }
    let request_id = app.presence.begin_refresh(&project_id);
    let reconnect_epoch = app.presence.reconnect_epoch;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    // The presence project travels in the argument: the panel reads the
    // **active tab's** project, not the attached session's.
    let scope = crate::tui::command_route::scope_for(app);

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "presence_refresh",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::PresenceSnapshotLoaded {
                    request_id,
                    project_id,
                    snapshot: None,
                    error: Some("Core unavailable".to_string()),
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            };

            // 1. Capability negotiation: an error or `supported: false`
            //    degrades to the generic unavailable panel.
            let cap_request = crate::core::new_request(
                format!("presence-capabilities-{}", uuid::Uuid::new_v4()),
                CoreRequest::PresenceCapabilities,
            );
            let supported = match core_client.request(cap_request).await {
                Ok(CoreResponse::PresenceCapabilities { capabilities }) => capabilities.supported,
                Ok(CoreResponse::Error { .. }) => false,
                Ok(_) => false,
                Err(_) => false,
            };
            if !supported {
                return Some(TuiCommand::PresenceSnapshotLoaded {
                    request_id,
                    project_id,
                    snapshot: None,
                    error: None,
                    unauthorized: false,
                    unsupported: true,
                    reconnect_epoch,
                });
            }

            // 2. Bounded snapshot; denials arrive as `project_not_found`.
            let answer = read_presence_snapshot(&core_client, scope, &project_id, refresh).await;
            Some(presence_snapshot_completion(
                request_id,
                project_id,
                reconnect_epoch,
                answer,
            ))
        },
    );
}

/// Apply a `PresenceSnapshotLoaded` completion, dropping stale ones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_presence_snapshot_loaded(
    app: &mut App,
    request_id: u64,
    project_id: String,
    snapshot: Option<PresenceSnapshotDto>,
    error: Option<String>,
    unauthorized: bool,
    unsupported: bool,
    reconnect_epoch: u64,
) {
    if unsupported {
        app.presence.set_capability(false);
        // Close this project's request binding even if the entry was
        // created after the capability flip.
        if let Some(entry) = app.presence.get(&project_id) {
            let _ = (entry, request_id);
        }
        // Close the in-flight binding so the loading flag clears.
        let _ = app.presence.apply_error(
            request_id,
            &project_id,
            String::new(),
            false,
            true,
            reconnect_epoch,
        );
        return;
    }
    // A transient error leaves the cached capability alone.
    if snapshot.is_some() && app.presence.capability_supported.is_none() {
        app.presence.set_capability(true);
    }
    if let Some(snapshot) = snapshot {
        let _ = app
            .presence
            .apply_snapshot(request_id, &project_id, &snapshot, reconnect_epoch);
    } else if let Some(err) = error {
        let _ = app.presence.apply_error(
            request_id,
            &project_id,
            err,
            unauthorized,
            false,
            reconnect_epoch,
        );
    }
}

/// Render the collaborator panel for the active project, fetching on demand
/// when the entry is missing or stale, then opening the scrollable dialog.
/// No session state is mutated.
pub(crate) fn show_collaborators(app: &mut App) {
    let Some(project_id) = app.active_project_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active project — open a project tab first");
        return;
    };
    if app.presence.needs_refresh(&project_id) {
        start_refresh_presence(app, project_id.clone(), PresenceRefresh::User);
    }
    let lines = app.presence.panel_lines(&project_id);
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Collaborators,
        lines,
    );
}

/// Explicit `/collaborators refresh` path.
pub(crate) fn refresh_collaborators(app: &mut App) {
    let Some(project_id) = app.active_project_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active project — open a project tab first");
        return;
    };
    // `start_refresh_presence` coalesces while loading; for an explicit user
    // refresh we still respect that. When the entry is Ready and fresh,
    // re-fetch anyway by beginning a new request directly.
    if !app.presence.needs_refresh(&project_id) {
        let request_id = app.presence.begin_refresh(&project_id);
        let reconnect_epoch = app.presence.reconnect_epoch;
        let core_client = app.core_client.clone();
        let tx = app.tui_cmd_tx.clone();
        let pid = project_id.clone();
        let scope = crate::tui::command_route::scope_for(app);
        spawn_registered_tui_task(
            tx,
            &mut app.task_registry,
            TuiTaskKind::Command,
            "presence_refresh",
            async move {
                let Some(core_client) = core_client else {
                    return Some(TuiCommand::PresenceSnapshotLoaded {
                        request_id,
                        project_id: pid,
                        snapshot: None,
                        error: Some("Core unavailable".to_string()),
                        unauthorized: false,
                        unsupported: true,
                        reconnect_epoch,
                    });
                };
                let answer =
                    read_presence_snapshot(&core_client, scope, &pid, PresenceRefresh::User).await;
                Some(presence_snapshot_completion(
                    request_id,
                    pid,
                    reconnect_epoch,
                    answer,
                ))
            },
        );
        let lines = app.presence.panel_lines(&project_id);
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::Collaborators,
            lines,
        );
        return;
    }
    start_refresh_presence(app, project_id.clone(), PresenceRefresh::User);
    let lines = app.presence.panel_lines(&project_id);
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Collaborators,
        lines,
    );
}
