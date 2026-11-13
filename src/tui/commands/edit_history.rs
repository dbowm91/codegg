//! Edit-checkpoint history: undo, reapply and list the checked edit service's checkpoints.

use crate::tui::app::send_tui;
use crate::tui::app::App;
use crate::tui::app::TuiCommand;

pub(crate) fn dispatch_edit_undo(app: &mut App, argument: &str) {
    let arg = argument;
    app.ui_state.command_mode = false;
    let (session_id, workspace_id) = match (
        app.active_session_id().map(|s| s.to_string()),
        app.active_workspace_id().map(|s| s.to_string()),
    ) {
        (Some(sid), Some(wid)) => (sid, wid),
        _ => {
            app.messages_state
                .toasts
                .warning("No active session/workspace for undo");
            return;
        }
    };
    if let Some(ref tx) = app.tui_cmd_tx {
        if arg.is_empty() {
            let _ = send_tui(
                tx,
                TuiCommand::EditUndoLatest {
                    session_id,
                    workspace_id,
                },
            );
            app.messages_state
                .toasts
                .info("Undoing latest checkpoint...");
        } else {
            let _ = send_tui(
                tx,
                TuiCommand::EditUndo {
                    checkpoint_id: arg.to_string(),
                    session_id,
                    workspace_id,
                },
            );
            app.messages_state
                .toasts
                .info(&format!("Undoing checkpoint {arg}..."));
        }
    } else {
        app.messages_state
            .toasts
            .warning("TUI command channel unavailable for undo");
    }
}

pub(crate) fn dispatch_edit_reapply(app: &mut App, argument: &str) {
    let arg = argument;
    app.ui_state.command_mode = false;
    let (session_id, workspace_id) = match (
        app.active_session_id().map(|s| s.to_string()),
        app.active_workspace_id().map(|s| s.to_string()),
    ) {
        (Some(sid), Some(wid)) => (sid, wid),
        _ => {
            app.messages_state
                .toasts
                .warning("No active session/workspace for reapply");
            return;
        }
    };
    if let Some(ref tx) = app.tui_cmd_tx {
        if arg.is_empty() {
            let _ = send_tui(
                tx,
                TuiCommand::EditReapplyLatest {
                    session_id,
                    workspace_id,
                },
            );
            app.messages_state
                .toasts
                .info("Reapplying latest undone checkpoint...");
        } else {
            let _ = send_tui(
                tx,
                TuiCommand::EditReapply {
                    checkpoint_id: arg.to_string(),
                    session_id,
                    workspace_id,
                },
            );
            app.messages_state
                .toasts
                .info(&format!("Reapplying checkpoint {arg}..."));
        }
    } else {
        app.messages_state
            .toasts
            .warning("TUI command channel unavailable for reapply");
    }
}

pub(crate) fn dispatch_edit_checkpoints(app: &mut App, argument: &str) {
    let _arg = argument;
    app.ui_state.command_mode = false;
    let (session_id, workspace_id) = match (
        app.active_session_id().map(|s| s.to_string()),
        app.active_workspace_id().map(|s| s.to_string()),
    ) {
        (Some(sid), Some(wid)) => (sid, wid),
        _ => {
            app.messages_state
                .toasts
                .warning("No active session/workspace for checkpoints");
            return;
        }
    };
    if let Some(ref tx) = app.tui_cmd_tx {
        let _ = send_tui(
            tx,
            TuiCommand::EditCheckpointList {
                session_id,
                workspace_id,
            },
        );
        app.messages_state.toasts.info("Loading checkpoints...");
    } else {
        app.messages_state
            .toasts
            .warning("TUI command channel unavailable for checkpoints");
    }
}
