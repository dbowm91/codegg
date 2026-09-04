//! TUI command path for daemon-owned historical run reruns.

use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) fn start_run_rerun(app: &mut App, parent_run_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Daemon connection unavailable; cannot rerun this run");
        return;
    };
    let Some(session) = app.session_state.session.as_ref() else {
        app.messages_state
            .toasts
            .error("A bound session is required to rerun this run");
        return;
    };
    let workspace_id = session.workspace_id.clone();
    let Some(workspace_id) = workspace_id else {
        app.messages_state
            .toasts
            .error("The current session has no canonical workspace binding");
        return;
    };
    let session_id = Some(session.id.clone());
    let tx = app.tui_cmd_tx.clone();
    let parent_for_result = parent_run_id.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "run_rerun",
        async move {
            let request = crate::core::new_request(
                format!("run-rerun-{}", uuid::Uuid::new_v4()),
                crate::protocol::core::CoreRequest::RunRerun {
                    workspace_id,
                    parent_run_id: parent_run_id.clone(),
                    session_id,
                },
            );
            let result = match core_client.request(request).await {
                Ok(crate::protocol::core::CoreResponse::RunRerunAccepted {
                    child_job_id, ..
                }) => TuiCommand::RunRerunFinished {
                    parent_run_id: parent_for_result,
                    child_job_id: Some(child_job_id),
                    error: None,
                },
                Ok(crate::protocol::core::CoreResponse::Error { code, message }) => {
                    TuiCommand::RunRerunFinished {
                        parent_run_id: parent_for_result,
                        child_job_id: None,
                        error: Some(format!("{code}: {message}")),
                    }
                }
                Ok(other) => TuiCommand::RunRerunFinished {
                    parent_run_id: parent_for_result,
                    child_job_id: None,
                    error: Some(format!("unexpected rerun response: {other:?}")),
                },
                Err(error) => TuiCommand::RunRerunFinished {
                    parent_run_id: parent_for_result,
                    child_job_id: None,
                    error: Some(format!("rerun request failed: {error}")),
                },
            };
            Some(result)
        },
    );
}

pub(crate) fn apply_run_rerun_finished(
    app: &mut App,
    parent_run_id: String,
    child_job_id: Option<String>,
    error: Option<String>,
) {
    if let Some(error) = error {
        app.messages_state.toasts.error(&error);
    } else if let Some(child_job_id) = child_job_id {
        app.messages_state.toasts.info(&format!(
            "Rerun accepted for {parent_run_id}; child job {child_job_id} queued"
        ));
    }
}
