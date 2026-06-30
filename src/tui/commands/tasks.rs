//! Task, worktree, template, notification, and miscellaneous command handlers.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::Dialog;
use crate::tui::app::SessionStatus;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;
use std::sync::Arc;

pub(crate) fn start_list_tasks(app: &mut App) {
    let request_id = app.dialog_state.task_list_request.begin();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "list_tasks",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TasksListed {
                    request_id,
                    tasks: Vec::new(),
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("task-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::TaskList,
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data }) => {
                    let tasks = data
                        .get("tasks")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    Some(TuiCommand::TasksListed {
                        request_id,
                        tasks,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::TasksListed {
                    request_id,
                    tasks: Vec::new(),
                    error: Some(format!("Failed to list tasks: {}", message)),
                }),
                Ok(_other) => Some(TuiCommand::TasksListed {
                    request_id,
                    tasks: Vec::new(),
                    error: Some("Unexpected task response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TasksListed {
                    request_id,
                    tasks: Vec::new(),
                    error: Some(format!("Failed to list tasks: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_tasks_listed(
    app: &mut App,
    request_id: u64,
    tasks: Vec<serde_json::Value>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .task_list_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state.toasts.warning(&err);
        return;
    }
    if !app.dialog_state.task_list_request.finish(request_id) {
        return;
    }
    if tasks.is_empty() {
        app.messages_state.toasts.info("No background tasks");
    } else {
        let list: Vec<String> = tasks
            .iter()
            .map(|t| {
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let message = t
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let interval_secs = t.get("interval_secs").and_then(|v| v.as_u64()).unwrap_or(0);
                format!(
                    "{}: {} ({}s)",
                    id.chars().take(8).collect::<String>(),
                    message.chars().take(30).collect::<String>(),
                    interval_secs
                )
            })
            .collect();
        if list.len() > 5 {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::TaskList,
                list,
            );
        } else {
            app.messages_state.toasts.info(&list.join(" | "));
        }
    }
}

pub(crate) fn start_delete_task(app: &mut App, id: String) {
    let request_id = app.dialog_state.task_delete_request.begin();
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "delete_task",
        async move {
            let Ok(parsed_id) = id.parse::<u64>() else {
                return Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: None,
                    error: Some("Task id must be numeric".to_string()),
                });
            };
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("task-delete-{}", uuid::Uuid::new_v4()),
                CoreRequest::TaskDelete { id: parsed_id },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Ack) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(parsed_id.to_string()),
                    error: None,
                }),
                Ok(CoreResponse::Error { code, .. }) if code == "task_not_found" => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "delete".to_string(),
                        task_id: Some(parsed_id.to_string()),
                        error: Some("Task not found".to_string()),
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "delete".to_string(),
                        task_id: Some(parsed_id.to_string()),
                        error: Some(format!("Failed to delete task: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(parsed_id.to_string()),
                    error: Some("Unexpected task response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(parsed_id.to_string()),
                    error: Some(format!("Failed to delete task: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_task_operation_finished(
    app: &mut App,
    request_id: u64,
    op: String,
    task_id: Option<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .task_delete_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state.toasts.warning(&err);
        return;
    }
    if !app.dialog_state.task_delete_request.finish(request_id) {
        return;
    }
    match op.as_str() {
        "delete" => {
            app.messages_state.toasts.info("Task deleted");
        }
        "schedule" => {
            let display_id = task_id
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(8)
                .collect::<String>();
            app.messages_state
                .toasts
                .info(&format!("Task {} scheduled", display_id));
        }
        _ => {
            app.messages_state.toasts.info(&format!("{} completed", op));
        }
    }
}

pub(crate) fn start_task_schedule(app: &mut App, interval_secs: u64, message: String) {
    let request_id = app.dialog_state.task_delete_request.begin();
    let core_client = app.core_client.clone();
    let session_id = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "task_schedule",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "schedule".to_string(),
                    task_id: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("task-schedule-{}", uuid::Uuid::new_v4()),
                CoreRequest::TaskSchedule {
                    session_id,
                    interval_secs,
                    message,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data }) => {
                    let task_id = data
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "schedule".to_string(),
                        task_id,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "schedule".to_string(),
                        task_id: None,
                        error: Some(format!("Failed to schedule task: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "schedule".to_string(),
                    task_id: None,
                    error: Some("Unexpected task response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "schedule".to_string(),
                    task_id: None,
                    error: Some(format!("Failed to schedule task: {}", e)),
                }),
            }
        },
    );
}

#[allow(dead_code)]
pub(crate) async fn handle_task_schedule(app: &mut App, interval_secs: u64, message: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let session_id = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let request = crate::core::new_request(
        format!("task-schedule-{}", uuid::Uuid::new_v4()),
        CoreRequest::TaskSchedule {
            session_id,
            interval_secs,
            message,
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let task_id = data
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            app.messages_state.toasts.info(&format!(
                "Task {} scheduled (every {}s)",
                task_id.chars().take(8).collect::<String>(),
                interval_secs
            ));
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to schedule task: {}", message)),
        Ok(_other) => app
            .messages_state
            .toasts
            .warning("Unexpected task response"),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to schedule task: {}", e)),
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_list_tasks(app: &mut App) {
    if let Some(core_client) = app.core_client.clone() {
        let request = crate::core::new_request(
            format!("task-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::TaskList,
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let tasks = data
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if tasks.is_empty() {
                    app.messages_state.toasts.info("No background tasks");
                } else {
                    let list: Vec<String> = tasks
                        .iter()
                        .map(|t| {
                            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                            let message = t
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let interval_secs =
                                t.get("interval_secs").and_then(|v| v.as_u64()).unwrap_or(0);
                            format!(
                                "{}: {} ({}s)",
                                id.chars().take(8).collect::<String>(),
                                message.chars().take(30).collect::<String>(),
                                interval_secs
                            )
                        })
                        .collect();
                    app.messages_state.toasts.info(&list.join(" | "));
                }
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to list tasks: {}", message));
            }
            Ok(_other) => {
                app.messages_state
                    .toasts
                    .warning("Unexpected task response");
            }
            Err(e) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to list tasks: {}", e));
            }
        }
    } else {
        app.messages_state.toasts.warning("Core client unavailable");
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_delete_task(app: &mut App, id: String) {
    if let Some(core_client) = app.core_client.clone() {
        let parsed_id = id.parse::<u64>().ok();
        let Some(parsed_id) = parsed_id else {
            app.messages_state.toasts.warning("Task id must be numeric");
            return;
        };
        let request = crate::core::new_request(
            format!("task-delete-{}", uuid::Uuid::new_v4()),
            CoreRequest::TaskDelete { id: parsed_id },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Ack) => app.messages_state.toasts.info("Task deleted"),
            Ok(CoreResponse::Error { code, .. }) if code == "task_not_found" => {
                app.messages_state.toasts.warning("Task not found");
            }
            Ok(CoreResponse::Error { message, .. }) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Failed to delete task: {}", message));
            }
            Ok(_other) => app
                .messages_state
                .toasts
                .warning("Unexpected task response"),
            Err(e) => app
                .messages_state
                .toasts
                .warning(&format!("Failed to delete task: {}", e)),
        }
    } else {
        app.messages_state.toasts.warning("Core client unavailable");
    }
}

pub(crate) fn start_worktree_list(app: &mut App) {
    let request_id = app.dialog_state.worktree_list_request.begin();
    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "worktree_list",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::WorktreeListed {
                    request_id,
                    worktrees: Vec::new(),
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let request = crate::core::new_request(
                format!("worktree-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::WorktreeList { project_dir },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data }) => {
                    let trees = data
                        .get("worktrees")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let names: Vec<String> = trees
                        .iter()
                        .map(|t| {
                            let path = t.get("path").and_then(|v| v.as_str()).unwrap_or_default();
                            let branch =
                                t.get("branch").and_then(|v| v.as_str()).unwrap_or_default();
                            format!("{} ({})", path, branch)
                        })
                        .collect();
                    Some(TuiCommand::WorktreeListed {
                        request_id,
                        worktrees: names,
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::WorktreeListed {
                    request_id,
                    worktrees: Vec::new(),
                    error: Some(format!("Failed to list worktrees: {}", message)),
                }),
                Ok(_other) => Some(TuiCommand::WorktreeListed {
                    request_id,
                    worktrees: Vec::new(),
                    error: Some("Unexpected worktree response".to_string()),
                }),
                Err(e) => Some(TuiCommand::WorktreeListed {
                    request_id,
                    worktrees: Vec::new(),
                    error: Some(format!("Failed to list worktrees: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_worktree_listed(
    app: &mut App,
    request_id: u64,
    worktrees: Vec<String>,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app
            .dialog_state
            .worktree_list_request
            .fail(request_id, err.clone())
        {
            return;
        }
        app.messages_state.toasts.warning(&err);
        return;
    }
    if !app.dialog_state.worktree_list_request.finish(request_id) {
        return;
    }
    if worktrees.is_empty() {
        app.messages_state.toasts.info("No worktrees found");
    } else if worktrees.len() > 5 {
        let lines: Vec<String> = worktrees.into_iter().map(|w| format!("  {}", w)).collect();
        app.open_info_dialog(
            crate::tui::components::dialogs::info::InfoType::WorktreeList,
            lines,
        );
    } else {
        app.messages_state.toasts.info(&worktrees.join(", "));
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_worktree_list(app: &mut App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("worktree-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::WorktreeList {
            project_dir: app.session_state.project_dir.clone(),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let trees = data
                .get("worktrees")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if trees.is_empty() {
                app.messages_state.toasts.info("No worktrees found");
            } else {
                let names: Vec<String> = trees
                    .iter()
                    .map(|t| {
                        let path = t.get("path").and_then(|v| v.as_str()).unwrap_or_default();
                        let branch = t.get("branch").and_then(|v| v.as_str()).unwrap_or_default();
                        format!("{} ({})", path, branch)
                    })
                    .collect();
                app.messages_state.toasts.info(&names.join(", "));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to list worktrees: {}", message)),
        Ok(_other) => app
            .messages_state
            .toasts
            .warning("Unexpected worktree response"),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Failed to list worktrees: {}", e)),
    }
}

pub(crate) fn start_send_notification(
    app: &mut App,
    notification_type: crate::tui::components::notification::NotificationType,
    body: String,
) {
    let notification_mgr = app.notification_manager.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Notification,
        "send_notification",
        async move {
            let Some(mgr) = notification_mgr else {
                return Some(TuiCommand::NotificationSent {
                    error: Some("Notification manager not available".to_string()),
                });
            };
            match mgr.send(notification_type, &body).await {
                Ok(()) => Some(TuiCommand::NotificationSent { error: None }),
                Err(e) => Some(TuiCommand::NotificationSent {
                    error: Some(format!("Failed to send notification: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_notification_sent(_app: &mut App, error: Option<String>) {
    if let Some(err) = error {
        tracing::warn!("{}", err);
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_send_notification(
    app: &mut App,
    notification_type: crate::tui::components::notification::NotificationType,
    body: String,
) {
    if let Some(ref notification_mgr) = app.notification_manager {
        if let Err(e) = notification_mgr.send(notification_type, &body).await {
            tracing::warn!("Failed to send notification: {}", e);
        }
    }
}

pub(crate) fn handle_compact_session(app: &mut App) {
    if app.session_state.session_status == SessionStatus::Working {
        app.messages_state
            .toasts
            .info("Compaction will occur at end of current turn");
    } else {
        app.messages_state
            .toasts
            .info("Compaction happens automatically during processing");
    }
}

pub(crate) fn handle_open_diff_dialog(
    app: &mut App,
    old_content: Box<str>,
    new_content: Box<str>,
    title: Box<str>,
) {
    let mut dialog =
        crate::tui::components::dialogs::diff::DiffDialog::new(old_content, new_content, title);
    dialog.set_theme(&app.ui_state.theme);
    app.dialog_state.diff_dialog = Some(dialog);
    app.open_dialog(Dialog::Diff);
}

pub(crate) fn handle_spawn_subagent(app: &mut App, agent_name: String, prompt: String) {
    use crate::agent::worker::SubAgentRequest;
    use crate::tui::async_cmd::spawn_registered_tui_task;
    use crate::tui::task_lifecycle::TuiTaskKind;

    if prompt.trim().is_empty() {
        app.messages_state
            .toasts
            .error("Subagent prompt cannot be empty");
        return;
    }

    let Some(pool) = app.subagent_pool.as_ref().map(Arc::clone) else {
        app.messages_state
            .toasts
            .error("Subagent pool not initialized");
        return;
    };

    let session_id = app
        .session_state
        .session
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();

    let task_id = rand::random::<u64>();

    let request = SubAgentRequest {
        task_id,
        prompt: prompt.clone(),
        agent: agent_name.clone(),
        parent_id: Some(session_id.clone()),
        denied_tools: Vec::new(),
        allowed_paths: Vec::new(),
        description: format!(
            "Task for agent '{}': {}",
            agent_name,
            prompt.chars().take(100).collect::<String>()
        ),
        depth: 1,
        max_tool_calls: None,
    };

    app.messages_state
        .messages
        .add_user_message(format!("@{} {}", agent_name, prompt), None);

    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "spawn_subagent",
        async move {
            let spawner = pool.spawner();
            if let Err(e) = spawner.send(request).await {
                return Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                    agent_name,
                    task_id,
                    prompt,
                    error: Some(format!("Failed to spawn subagent: {}", e)),
                });
            }
            Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                agent_name,
                task_id,
                prompt,
                error: None,
            })
        },
    );
}

pub(crate) fn apply_subagent_spawn_finished(
    app: &mut App,
    agent_name: String,
    task_id: u64,
    _prompt: String,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.error(&err);
        return;
    }
    app.messages_state.toasts.info(&format!(
        "Spawned subagent '{}' with task #{}",
        agent_name, task_id
    ));
}

pub(crate) fn handle_file_diff_stats_ready(
    app: &mut App,
    path: std::path::PathBuf,
    generation: u64,
    result: crate::tui::file_diff::FileDiffStatsResult,
) {
    use crate::tui::app::state::session::DiffStatsState;

    // Find the changed-file entry by path.
    if let Some(entry) = app
        .session_state
        .changed_files
        .iter_mut()
        .find(|f| f.path == path)
    {
        // Ignore stale completions.
        if entry.diff_state.generation() != generation {
            return;
        }
        entry.diff_state = DiffStatsState::from_result(generation, result);
    } else {
        return;
    }

    // Refresh sidebar.
    let changes = app
        .session_state
        .changed_files
        .iter()
        .map(|file| crate::tui::components::sidebar::SidebarFileChange {
            path: file.path.to_string_lossy().into_owned(),
            action: file.action.clone(),
            diff_preview: file.diff_preview.clone(),
            diff_state: file.diff_state.clone(),
        })
        .collect();
    app.sidebar.set_file_changes(changes);
}
