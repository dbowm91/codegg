//! The `/revert` command handler (M007-H, work package C).
//!
//! The revert is a **daemon** effect: the daemon resolves the checkout from
//! its own durable binding, contains the path inside it, takes the
//! checkout's writer permit, and issues the typed `GitOperation`. This
//! module never spawns `git` and never restores a file itself; it sends the
//! command and renders the daemon's answer.
//!
//! The scope comes from [`crate::tui::command_route::scope_for`], so the
//! daemon learns the project and the workspace from the attached session's
//! canonical binding and never from the frontend's guess.

use crate::protocol::core::CoreResponse;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Run `/revert <path>` against the daemon.
pub(crate) fn start_revert(app: &mut App, path: String) {
    let core_client = app.core_client.clone();
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "revert",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::RevertFinished {
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response =
                crate::tui::command_route::send_command(&core_client, scope, "revert", "", &path)
                    .await;
            match response {
                Ok(response) => Some(TuiCommand::RevertFinished {
                    response: Some(response),
                    error: None,
                }),
                Err(error) => Some(TuiCommand::RevertFinished {
                    response: None,
                    error: Some(format!("Failed to revert: {}", error)),
                }),
            }
        },
    );
}

/// Render the daemon's answer.
///
/// The daemon's typed refusal is rendered as it arrives. The TUI never
/// substitutes a local approximation of the effect it could not perform.
pub(crate) fn apply_revert_finished(
    app: &mut App,
    response: Option<CoreResponse>,
    error: Option<String>,
) {
    if let Some(error) = error {
        app.messages_state.toasts.error(&error);
        return;
    }
    let Some(response) = response else {
        return;
    };
    match response {
        CoreResponse::Error { code, message } => {
            app.messages_state
                .toasts
                .error(&format!("{code}: {message}"));
        }
        CoreResponse::GitRevert { result } => {
            if result.paths.is_empty() {
                app.messages_state
                    .toasts
                    .warning("The revert restored nothing");
                return;
            }
            app.messages_state
                .toasts
                .success(&format!("Reverted: {}", result.paths.join(", ")));
        }
        _ => {
            app.messages_state
                .toasts
                .warning("The revert returned an unexpected response");
        }
    }
}
