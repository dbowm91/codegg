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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CoreClient, InprocCoreClient};
    use crate::protocol::dto::ProjectRegisterRequestDto;
    use std::sync::Arc;
    use std::time::Duration;

    async fn test_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:tui_project_init_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn tui_approval_publishes_through_core_client_and_daemon() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("README.md"), "# TUI integration fixture\n").unwrap();
        let pool = test_pool().await;
        let client = Arc::new(InprocCoreClient::new(
            None,
            None,
            Some(pool),
            crate::config::schema::Config::default(),
            None,
        ));
        let workspace = match client
            .request(crate::core::new_request(
                "tui-init-register-workspace".into(),
                CoreRequest::WorkspaceRegister {
                    root: root.path().display().to_string(),
                },
            ))
            .await
            .unwrap()
        {
            CoreResponse::WorkspaceSnapshot { workspace } => workspace,
            other => panic!("expected registered workspace, got {other:?}"),
        };
        let project = match client
            .request(crate::core::new_request(
                "tui-init-register-project".into(),
                CoreRequest::ProjectRegister {
                    request: ProjectRegisterRequestDto {
                        workspace_id: workspace.workspace_id.clone(),
                        display_name: "TUI init fixture".into(),
                        description: None,
                        tags: Vec::new(),
                        repository_id: None,
                        source: "test".into(),
                    },
                },
            ))
            .await
            .unwrap()
        {
            CoreResponse::ProjectRegistered { project } => project,
            other => panic!("expected registered project, got {other:?}"),
        };

        let mut app = App::new_for_testing(root.path().display().to_string());
        let tab = app
            .project_tabs
            .active_mut()
            .expect("test app has an active tab");
        tab.project_id = Some(project.project_id.clone());
        tab.workspace_id = Some(workspace.workspace_id.clone());
        tab.workspace_root = Some(root.path().to_path_buf());
        app.set_core_client(client);
        app.ensure_tui_cmd_channel();
        start_project_init_draft(&mut app);

        let preview = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, preview).await;
        assert!(
            !root.path().join("AGENTS.md").exists(),
            "preview has no write side effect"
        );
        app.project_tabs.active_mut().unwrap().project_id = Some("different-project".into());
        app.process_msg(crate::tui::app::TuiMsg::ProjectInitApprove);
        assert!(
            !root.path().join("AGENTS.md").exists(),
            "a switched project cannot receive the draft"
        );
        assert!(
            app.tui_cmd_rx.as_mut().unwrap().try_recv().is_err(),
            "stale approval queues no publish request"
        );
        app.project_tabs.active_mut().unwrap().project_id = Some(project.project_id.clone());
        start_project_init_draft(&mut app);
        let preview = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, preview).await;
        app.process_msg(crate::tui::app::TuiMsg::CloseDialog);
        assert!(
            !root.path().join("AGENTS.md").exists(),
            "cancel leaves the workspace unchanged"
        );
        assert!(
            app.tui_cmd_rx.as_mut().unwrap().try_recv().is_err(),
            "cancel queues no publish request"
        );
        start_project_init_draft(&mut app);
        let preview = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, preview).await;
        app.process_msg(crate::tui::app::TuiMsg::ProjectInitApprove);

        let publish = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(publish, TuiCommand::ProjectInitPublish { .. }));
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, publish).await;
        let completion = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            &completion,
            TuiCommand::ProjectInitPublishFinished { error: None, .. }
        ));
        assert!(root.path().join("AGENTS.md").is_file());
        crate::tui::runtime::command_dispatch::dispatch_tui_command(&mut app, completion).await;
        let refresh = tokio::time::timeout(
            Duration::from_secs(5),
            app.tui_cmd_rx.as_mut().unwrap().recv(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            refresh,
            TuiCommand::AssetRefreshFinished {
                report: Some(_),
                error: None
            }
        ));
    }
}
