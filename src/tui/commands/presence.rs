//! Collaborator presence commands (Presence M002).
//!
//! The TUI owns no presence truth. Every refresh performs an explicit
//! `PresenceCapabilities` negotiation followed by a bounded
//! `PresenceSnapshotGet` through the daemon-owned `CoreClient`. Results
//! land back on the event loop as `TuiCommand::PresenceSnapshotLoaded`
//! completions guarded by per-project request id + reconnect epoch, so
//! stale completions (after a tab switch or reconnect) are dropped at
//! apply time.
//!
//! Concurrency: `start_*` bumps the reducer generation via
//! `PresenceState::begin_refresh` and `apply_*` drops stale completions.
//! Hints (`PresenceUpdated { project_id }`) only flag `needs_resync`;
//! they never spawn a task per update — the next explicit refresh (tab
//! open, user action, or reconnect) performs the single re-fetch.

use crate::protocol::core::{CoreRequest, CoreResponse, PresenceSnapshotDto};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Kick off a presence refresh for `project_id`.
///
/// Capability negotiation happens first: older daemons that do not
/// advertise presence return `supported: false` and the panel renders
/// the generic unavailable state without breaking project tabs.
pub(crate) fn start_refresh_presence(app: &mut App, project_id: String) {
    if project_id.is_empty() {
        return;
    }
    if app.core_client.is_none() {
        // No daemon: surface the same unavailable shape as an old
        // daemon so tabs keep working.
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

            // 1. Capability negotiation. Older daemons answer with an
            //    error or `supported: false`; both degrade to the
            //    generic unavailable panel.
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

            // 2. Bounded snapshot. Authorization is enforced daemon-side;
            //    denials arrive as `project_not_found`.
            let snap_request = crate::core::new_request(
                format!("presence-snapshot-{}", uuid::Uuid::new_v4()),
                CoreRequest::PresenceSnapshotGet {
                    project_id: project_id.clone(),
                },
            );
            match core_client.request(snap_request).await {
                Ok(CoreResponse::PresenceSnapshot { snapshot }) => {
                    Some(TuiCommand::PresenceSnapshotLoaded {
                        request_id,
                        project_id,
                        snapshot: Some(snapshot),
                        error: None,
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch,
                    })
                }
                Ok(CoreResponse::Error { code, message }) => {
                    let unauthorized = code == "project_not_found";
                    Some(TuiCommand::PresenceSnapshotLoaded {
                        request_id,
                        project_id,
                        snapshot: None,
                        error: Some(format!("{code}: {message}")),
                        unauthorized,
                        unsupported: false,
                        reconnect_epoch,
                    })
                }
                Ok(other) => Some(TuiCommand::PresenceSnapshotLoaded {
                    request_id,
                    project_id,
                    snapshot: None,
                    error: Some(format!("Unexpected core response: {other:?}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
                Err(e) => Some(TuiCommand::PresenceSnapshotLoaded {
                    request_id,
                    project_id,
                    snapshot: None,
                    error: Some(format!("Presence snapshot request failed: {e}")),
                    unauthorized: false,
                    unsupported: false,
                    reconnect_epoch,
                }),
            }
        },
    );
}

/// Apply a `PresenceSnapshotLoaded` completion. Drops stale completions
/// (wrong request id or reconnect epoch) and enforces the
/// project-routing guard in [`crate::tui::app::state::PresenceState`].
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
        // `set_capability(false)` already marks every entry
        // unavailable; ensure this project's request binding is closed
        // even if the entry was created after the capability flip.
        if let Some(entry) = app.presence.get(&project_id) {
            let _ = (entry, request_id);
        }
        // Close the in-flight binding via an explicit error apply so
        // the loading flag clears even on the first fetch.
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
    // First successful capability contact marks support. Failures that
    // are not "unsupported" leave the cached capability alone so a
    // transient error does not permanently hide the panel.
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

/// Render the collaborator panel for the active project. Fetches on
/// demand when the entry is missing or stale, then opens the scrollable
/// dialog with the current (possibly loading) lines. Focus/key behavior
/// follows the standard info-dialog convention (`j`/`k` scroll,
/// `Esc`/`Enter` close); no session state is mutated.
pub(crate) fn show_collaborators(app: &mut App) {
    let Some(project_id) = app.active_project_id().map(str::to_string) else {
        app.messages_state
            .toasts
            .info("No active project — open a project tab first");
        return;
    };
    if app.presence.needs_refresh(&project_id) {
        start_refresh_presence(app, project_id.clone());
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
    // Force a fetch even when the reducer thinks data is fresh: bump
    // through `begin_refresh` by clearing the resync flag path. The
    // cheapest correct force is to mark resync and start.
    if let Some(entry) = app.presence.get(&project_id) {
        let _ = entry;
    }
    // `start_refresh_presence` coalesces while loading; for an explicit
    // user refresh we still respect that (no storm on rapid repeats).
    // When the entry is Ready and fresh, re-fetch anyway by beginning a
    // new request directly.
    if !app.presence.needs_refresh(&project_id) {
        let request_id = app.presence.begin_refresh(&project_id);
        let reconnect_epoch = app.presence.reconnect_epoch;
        let core_client = app.core_client.clone();
        let tx = app.tui_cmd_tx.clone();
        let pid = project_id.clone();
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
                let snap_request = crate::core::new_request(
                    format!("presence-snapshot-{}", uuid::Uuid::new_v4()),
                    CoreRequest::PresenceSnapshotGet {
                        project_id: pid.clone(),
                    },
                );
                match core_client.request(snap_request).await {
                    Ok(CoreResponse::PresenceSnapshot { snapshot }) => {
                        Some(TuiCommand::PresenceSnapshotLoaded {
                            request_id,
                            project_id: pid,
                            snapshot: Some(snapshot),
                            error: None,
                            unauthorized: false,
                            unsupported: false,
                            reconnect_epoch,
                        })
                    }
                    Ok(CoreResponse::Error { code, message }) => {
                        let unauthorized = code == "project_not_found";
                        Some(TuiCommand::PresenceSnapshotLoaded {
                            request_id,
                            project_id: pid,
                            snapshot: None,
                            error: Some(format!("{code}: {message}")),
                            unauthorized,
                            unsupported: false,
                            reconnect_epoch,
                        })
                    }
                    Ok(other) => Some(TuiCommand::PresenceSnapshotLoaded {
                        request_id,
                        project_id: pid,
                        snapshot: None,
                        error: Some(format!("Unexpected core response: {other:?}")),
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch,
                    }),
                    Err(e) => Some(TuiCommand::PresenceSnapshotLoaded {
                        request_id,
                        project_id: pid,
                        snapshot: None,
                        error: Some(format!("Presence snapshot request failed: {e}")),
                        unauthorized: false,
                        unsupported: false,
                        reconnect_epoch,
                    }),
                }
            },
        );
        let lines = app.presence.panel_lines(&project_id);
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::Collaborators,
            lines,
        );
        return;
    }
    start_refresh_presence(app, project_id.clone());
    let lines = app.presence.panel_lines(&project_id);
    app.open_info_dialog(
        crate::tui::components::dialogs::info::InfoType::Collaborators,
        lines,
    );
}
