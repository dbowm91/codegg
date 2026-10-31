//! The `/diff` command handler (M007-I, work package A).
//!
//! The diff view is a **daemon** effect: the daemon resolves the checkout
//! from its own durable binding and reads the bytes through the read-only
//! git fact layer. This module never spawns `git` and never reads the
//! working tree itself; it sends the command and renders the daemon's
//! answer.
//!
//! The scope comes from [`crate::tui::command_route::scope_for`], so the
//! daemon learns the project and the workspace from the attached session's
//! canonical binding and never from the frontend's guess.

use crate::protocol::core::CoreResponse;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Run `/diff <path>` — or the bare `/diff` — against the daemon.
///
/// The empty path is the **bare** branch: the whole working tree against
/// `HEAD`. The two branches are mutually exclusive, exactly as they were
/// when the frontend owned the read.
pub(crate) fn start_diff(app: &mut App, path: String) {
    let core_client = app.core_client.clone();
    let scope = crate::tui::command_route::scope_for(app);
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "diff",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::DiffFinished {
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response =
                crate::tui::command_route::send_command(&core_client, scope, "diff", "", &path)
                    .await;
            match response {
                Ok(response) => Some(TuiCommand::DiffFinished {
                    response: Some(response),
                    error: None,
                }),
                Err(error) => Some(TuiCommand::DiffFinished {
                    response: None,
                    error: Some(format!("Failed to read the diff: {}", error)),
                }),
            }
        },
    );
}

/// Render the daemon's answer.
///
/// The daemon's typed refusal is rendered as it arrives. The TUI never
/// substitutes a local approximation of the read it could not perform, and
/// it keeps no fallback that would report a stale diff.
pub(crate) fn apply_diff_finished(
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
        CoreResponse::GitDiff { result } => {
            if result.old_content == result.new_content {
                app.messages_state.toasts.info("No changes detected");
                return;
            }
            app.process_msg(crate::tui::app::TuiMsg::OpenDiffDialog {
                old_content: result.old_content.into_boxed_str(),
                new_content: result.new_content.into_boxed_str(),
                title: result.title.into_boxed_str(),
            });
        }
        _ => {
            app.messages_state
                .toasts
                .warning("The diff returned an unexpected response");
        }
    }
}
