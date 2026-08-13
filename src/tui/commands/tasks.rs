//! Task, worktree, template, notification, and miscellaneous command handlers.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::Dialog;
use crate::tui::app::SessionStatus;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

fn durable_schedule_spec(
    workspace_id: String,
    session_id: String,
    interval_secs: u64,
    message: String,
) -> crate::protocol::dto::ScheduleCreateDto {
    use codegg_core::jobs::schedule::{JobTemplate, MissedRunPolicy, OverlapPolicy, ScheduleKind};
    use codegg_core::jobs::JobKind;

    let kind = ScheduleKind::Interval {
        every: std::time::Duration::from_secs(interval_secs),
        anchor: chrono::Utc::now(),
    };
    let job_template = JobTemplate::for_subagent(
        JobKind::Subagent,
        message,
        "build".to_string(),
        Some(session_id.clone()),
    );

    crate::protocol::dto::ScheduleCreateDto {
        workspace_id,
        session_id: Some(session_id),
        kind: serde_json::to_value(kind).expect("durable schedule kind is serializable"),
        job_template: serde_json::to_value(job_template)
            .expect("durable schedule job template is serializable"),
        overlap_policy: serde_json::to_value(OverlapPolicy::SkipIfRunning)
            .expect("durable overlap policy is serializable")
            .as_str()
            .expect("overlap policy serializes as a string")
            .to_string(),
        missed_run_policy: serde_json::to_value(MissedRunPolicy::RunOnceNow)
            .expect("durable missed-run policy is serializable"),
        labels: std::collections::HashMap::new(),
    }
}

fn durable_schedule_task_value(
    schedule: &crate::protocol::dto::ScheduleSummaryDto,
) -> serde_json::Value {
    let kind = schedule
        .kind
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("schedule");
    let interval_secs = schedule
        .kind
        .get("every")
        .and_then(|every| every.get("secs"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();

    serde_json::json!({
        "id": schedule.schedule_id,
        "kind": kind,
        "state": schedule.state,
        "interval_secs": interval_secs,
    })
}

fn worktree_label(tree: &serde_json::Value) -> Option<String> {
    let path = tree.get("path").and_then(|v| v.as_str())?.trim();
    if path.is_empty() {
        return None;
    }

    let branch = tree
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if branch.is_empty() {
        Some(path.to_string())
    } else {
        Some(format!("{} ({})", path, branch))
    }
}

pub(crate) fn start_list_tasks(app: &mut App) {
    let Some(workspace_id) = app
        .session_state
        .session
        .as_ref()
        .and_then(|session| session.workspace_id.clone())
    else {
        app.messages_state
            .toasts
            .warning("Task schedules require an active workspace");
        return;
    };
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
                CoreRequest::ScheduleList {
                    workspace_id: Some(workspace_id),
                    include_archived: false,
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::ScheduleList { schedules }) => {
                    let tasks = schedules.iter().map(durable_schedule_task_value).collect();
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
                let kind = t.get("kind").and_then(|v| v.as_str()).unwrap_or("schedule");
                let label = if message.is_empty() { kind } else { message };
                let interval_secs = t.get("interval_secs").and_then(|v| v.as_u64()).unwrap_or(0);
                format!(
                    "{}: {} ({}s)",
                    id.chars().take(8).collect::<String>(),
                    label.chars().take(30).collect::<String>(),
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
                CoreRequest::ScheduleDelete {
                    schedule_id: id.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::ScheduleDeleted { schedule_id }) => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "delete".to_string(),
                        task_id: Some(schedule_id),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "delete".to_string(),
                        task_id: Some(id.clone()),
                        error: Some(format!("Failed to delete task: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(id.clone()),
                    error: Some("Unexpected task response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(id),
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
    let Some(session) = app.session_state.session.as_ref() else {
        app.messages_state
            .toasts
            .warning("Task schedules require an active session");
        return;
    };
    let Some(workspace_id) = session.workspace_id.clone() else {
        app.messages_state
            .toasts
            .warning("Task schedules require an active workspace");
        return;
    };
    let request_id = app.dialog_state.task_delete_request.begin();
    let core_client = app.core_client.clone();
    let session_id = session.id.clone();
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
                CoreRequest::ScheduleCreate {
                    spec: durable_schedule_spec(workspace_id, session_id, interval_secs, message),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::ScheduleCreated { schedule_id }) => {
                    Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "schedule".to_string(),
                        task_id: Some(schedule_id),
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
                    let names: Vec<String> = trees.iter().filter_map(worktree_label).collect();
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
    use crate::tui::async_cmd::spawn_registered_tui_task;
    use crate::tui::task_lifecycle::TuiTaskKind;

    if prompt.trim().is_empty() {
        app.messages_state
            .toasts
            .error("Subagent prompt cannot be empty");
        return;
    }

    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .error("Core client unavailable; subagents require the daemon scheduler");
        return;
    };

    let Some(session) = app.session_state.session.clone() else {
        app.messages_state
            .toasts
            .error("No active session for subagent");
        return;
    };
    let session_id = session.id.clone();
    let workspace_root = session.directory.clone();

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
            let workspace = match core_client
                .request(crate::core::new_request(
                    format!("subagent-workspace-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::WorkspaceRegister {
                        root: workspace_root.clone(),
                    },
                ))
                .await
            {
                Ok(crate::protocol::core::CoreResponse::WorkspaceSnapshot { workspace }) => {
                    workspace.workspace_id
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    return Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                        agent_name,
                        task_id: 0,
                        prompt,
                        error: Some(message),
                    });
                }
                Ok(other) => {
                    return Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                        agent_name,
                        task_id: 0,
                        prompt,
                        error: Some(format!("unexpected workspace response: {other:?}")),
                    });
                }
                Err(e) => {
                    return Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                        agent_name,
                        task_id: 0,
                        prompt,
                        error: Some(format!("workspace registration failed: {e}")),
                    });
                }
            };
            let spec = crate::protocol::dto::JobSubmitDto {
                submission_key: Some(format!("tui-subagent-{}", uuid::Uuid::new_v4())),
                workspace_id: workspace,
                session_id: Some(session_id.clone()),
                turn_id: None,
                kind: "subagent".into(),
                priority: "interactive".into(),
                source: serde_json::json!({"kind": "agent_delegated"}),
                payload: serde_json::json!({
                    "kind": "subagent",
                    "prompt": prompt,
                    "agent": agent_name,
                    "parent_id": session_id,
                    "denied_tools": [],
                    "allowed_paths": [workspace_root],
                    "max_tool_calls": null
                }),
                timeout_ms: None,
                retry_max_attempts: 1,
                retryable_failures: Vec::new(),
                idempotency: "non_idempotent".into(),
                not_before_ms: None,
                deadline_ms: None,
                schedule_id: None,
                depends_on: Vec::new(),
                labels: std::collections::HashMap::new(),
            };
            let response = core_client
                .request(crate::core::new_request(
                    format!("subagent-submit-{}", uuid::Uuid::new_v4()),
                    crate::protocol::core::CoreRequest::JobSubmit { spec },
                ))
                .await;
            match response {
                Ok(crate::protocol::core::CoreResponse::JobSubmitted { job_id }) => {
                    let task_id = job_id
                        .bytes()
                        .take(8)
                        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                    Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                        agent_name,
                        task_id,
                        prompt,
                        error: None,
                    })
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                        agent_name,
                        task_id: 0,
                        prompt,
                        error: Some(message),
                    })
                }
                Ok(other) => Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                    agent_name,
                    task_id: 0,
                    prompt,
                    error: Some(format!("unexpected job response: {other:?}")),
                }),
                Err(e) => Some(crate::tui::app::TuiCommand::SubagentSpawnFinished {
                    agent_name,
                    task_id: 0,
                    prompt,
                    error: Some(format!("subagent submission failed: {e}")),
                }),
            }
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

#[cfg(test)]
mod tests {
    use super::{durable_schedule_spec, durable_schedule_task_value, worktree_label};
    use serde_json::json;

    #[test]
    fn task_schedule_uses_durable_opaque_schedule_contract() {
        let spec = durable_schedule_spec(
            "workspace-1".to_string(),
            "session-1".to_string(),
            300,
            "check the build".to_string(),
        );

        assert_eq!(spec.workspace_id, "workspace-1");
        assert_eq!(spec.session_id.as_deref(), Some("session-1"));
        assert_eq!(spec.overlap_policy, "skip_if_running");
        assert_eq!(spec.kind["kind"], "interval");
        assert_eq!(spec.kind["every"]["secs"], 300);
        assert_eq!(spec.job_template["kind"], "subagent");
        assert_eq!(spec.job_template["payload"]["prompt"], "check the build");
        assert_eq!(spec.job_template["payload"]["parent_id"], "session-1");
    }

    #[test]
    fn task_list_projects_durable_schedule_summary_for_existing_view() {
        let schedule = crate::protocol::dto::ScheduleSummaryDto {
            schedule_id: "schedule-opaque-id".to_string(),
            workspace_id: "workspace-1".to_string(),
            session_id: Some("session-1".to_string()),
            kind: json!({
                "kind": "interval",
                "every": {"secs": 60, "nanos": 0},
            }),
            state: "active".to_string(),
            overlap_policy: "skip_if_running".to_string(),
            missed_run_policy: json!("run_once_now"),
            next_run_at_ms: None,
            last_occurrence_at_ms: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        };

        let task = durable_schedule_task_value(&schedule);
        assert_eq!(task["id"], "schedule-opaque-id");
        assert_eq!(task["kind"], "interval");
        assert_eq!(task["interval_secs"], 60);
        assert_eq!(task["state"], "active");
    }

    #[test]
    fn worktree_label_uses_path_and_branch() {
        let tree = json!({
            "path": "/repo/wt",
            "branch": "feature/release-polish"
        });

        assert_eq!(
            worktree_label(&tree).as_deref(),
            Some("/repo/wt (feature/release-polish)")
        );
    }

    #[test]
    fn worktree_label_omits_empty_branch() {
        let tree = json!({
            "path": "/repo/detached",
            "branch": ""
        });

        assert_eq!(worktree_label(&tree).as_deref(), Some("/repo/detached"));
    }

    #[test]
    fn worktree_label_skips_missing_path() {
        let tree = json!({
            "branch": "main"
        });

        assert_eq!(worktree_label(&tree), None);
    }
}
