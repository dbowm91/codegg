//! Daemon-backed `/init` preview, explicit publish, and scoped refresh.
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::{App, TuiCommand};
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) fn start_project_init_draft(app: &mut App) {
    let context = match app.project_execution_context() {
        Ok(context) => context,
        Err(error) => {
            app.messages_state.toasts.error(&error);
            return;
        }
    };
    let (Some(project_id), Some(workspace_id)) = (context.project_id, context.workspace_id) else {
        app.messages_state
            .toasts
            .error("/init requires a registered project and workspace");
        return;
    };
    let client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project-init-draft",
        async move {
            let result = async {
                let client = client.ok_or_else(|| "Core unavailable".to_string())?;
                let req = crate::core::new_request(
                    format!("project-init-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectInitDraftGet {
                        project_id,
                        workspace_id,
                    },
                );
                match client.request(req).await.map_err(|e| e.to_string())? {
                    CoreResponse::ProjectInitDraft { draft } => Ok(draft),
                    CoreResponse::Error { code, message } => {
                        Err(format!("/init failed ({code}): {message}"))
                    }
                    _ => Err("Unexpected daemon response for /init preview".into()),
                }
            }
            .await;
            Some(match result {
                Ok(draft) => TuiCommand::ProjectInitDraftFinished {
                    draft: Some(draft),
                    error: None,
                },
                Err(error) => TuiCommand::ProjectInitDraftFinished {
                    draft: None,
                    error: Some(error),
                },
            })
        },
    );
}

pub(crate) fn start_project_init_publish(
    app: &mut App,
    project_id: String,
    workspace_id: String,
    draft_token: String,
) {
    let client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project-init-publish",
        async move {
            let result = async {
                let client = client.ok_or_else(|| "Core unavailable".to_string())?;
                let req = crate::core::new_request(
                    format!("project-init-publish-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectInitPublish {
                        project_id: project_id.clone(),
                        workspace_id: workspace_id.clone(),
                        draft_token,
                    },
                );
                match client.request(req).await.map_err(|e| e.to_string())? {
                    CoreResponse::ProjectInitPublished { .. } => Ok(()),
                    CoreResponse::Error { code, message } => {
                        Err(format!("Publication failed ({code}): {message}"))
                    }
                    _ => Err("Unexpected daemon response for /init publication".into()),
                }
            }
            .await;
            Some(TuiCommand::ProjectInitPublishFinished {
                project_id,
                workspace_id,
                error: result.err(),
            })
        },
    );
}

pub(crate) fn start_project_init_refresh(app: &mut App, project_id: String, workspace_id: String) {
    let client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "project-init-refresh",
        async move {
            let result = async {
                let client = client.ok_or_else(|| "Core unavailable".to_string())?;
                let req = crate::core::new_request(
                    format!("project-init-refresh-{}", uuid::Uuid::new_v4()),
                    CoreRequest::AssetRefresh {
                        request: crate::protocol::core::AssetRefreshRequestDto {
                            scope: crate::protocol::core::AssetRefreshScopeDto {
                                project_id,
                                workspace_id,
                            },
                            reason: crate::protocol::core::AssetRefreshReasonDto::Reload,
                            session_id: None,
                        },
                    },
                );
                match client.request(req).await.map_err(|e| e.to_string())? {
                    CoreResponse::AssetRefresh { report } => Ok(report),
                    CoreResponse::Error { code, message } => {
                        Err(format!("Instruction refresh failed ({code}): {message}"))
                    }
                    _ => Err("Unexpected daemon response for instruction refresh".into()),
                }
            }
            .await;
            Some(match result {
                Ok(report) => TuiCommand::AssetRefreshFinished {
                    report: Some(report),
                    error: None,
                },
                Err(error) => TuiCommand::AssetRefreshFinished {
                    report: None,
                    error: Some(error),
                },
            })
        },
    );
}
