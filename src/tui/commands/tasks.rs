//! Task, worktree, template, notification, and miscellaneous command handlers.

use crate::core::CoreClient;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::Dialog;
use crate::tui::app::SessionStatus;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) const SCHEDULE_DISPLAY_ID_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScheduleIdResolutionError {
    Empty,
    TooShort { minimum: usize },
    NotFound(String),
    Ambiguous { input: String, matches: usize },
}

impl std::fmt::Display for ScheduleIdResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Schedule ID cannot be empty"),
            Self::TooShort { minimum } => write!(
                f,
                "Schedule ID prefix must be at least {minimum} characters (use the ID shown by /tasks)"
            ),
            Self::NotFound(input) => write!(f, "No schedule found for ID '{input}'"),
            Self::Ambiguous { input, matches } => write!(
                f,
                "Schedule ID prefix '{input}' is ambiguous ({matches} matches); use a longer or full schedule ID"
            ),
        }
    }
}

pub(crate) fn schedule_display_id(schedule_id: &str) -> String {
    schedule_id.chars().take(SCHEDULE_DISPLAY_ID_LEN).collect()
}

pub(crate) fn resolve_schedule_id(
    input: &str,
    schedules: &[crate::protocol::dto::ScheduleSummaryDto],
) -> Result<String, ScheduleIdResolutionError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ScheduleIdResolutionError::Empty);
    }

    if let Some(schedule) = schedules
        .iter()
        .find(|schedule| schedule.schedule_id == input)
    {
        return Ok(schedule.schedule_id.clone());
    }

    if input.chars().count() < SCHEDULE_DISPLAY_ID_LEN {
        return Err(ScheduleIdResolutionError::TooShort {
            minimum: SCHEDULE_DISPLAY_ID_LEN,
        });
    }

    let matches: Vec<&str> = schedules
        .iter()
        .filter(|schedule| schedule.schedule_id.starts_with(input))
        .map(|schedule| schedule.schedule_id.as_str())
        .collect();
    match matches.as_slice() {
        [schedule_id] => Ok((*schedule_id).to_string()),
        [] => Err(ScheduleIdResolutionError::NotFound(input.to_string())),
        matches => Err(ScheduleIdResolutionError::Ambiguous {
            input: input.to_string(),
            matches: matches.len(),
        }),
    }
}

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

fn schedule_kind(schedule: &crate::protocol::dto::ScheduleSummaryDto) -> &str {
    schedule
        .kind
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("schedule")
}

fn schedule_fallback_label(schedule: &crate::protocol::dto::ScheduleSummaryDto) -> String {
    format!("{}/{}", schedule_kind(schedule), schedule.state)
}

pub(crate) fn schedule_label(
    summary: &crate::protocol::dto::ScheduleSummaryDto,
    detail: Option<&crate::protocol::dto::ScheduleRecordDto>,
) -> String {
    detail
        .and_then(|record| record.job_template.get("payload"))
        .and_then(|payload| payload.get("prompt"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| schedule_fallback_label(summary))
}

fn durable_schedule_task_value_with_label(
    schedule: &crate::protocol::dto::ScheduleSummaryDto,
    label: &str,
) -> serde_json::Value {
    let interval_secs = schedule
        .kind
        .get("every")
        .and_then(|every| every.get("secs"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();

    serde_json::json!({
        "id": schedule.schedule_id,
        "kind": schedule_kind(schedule),
        "state": schedule.state,
        "interval_secs": interval_secs,
        "message": label,
    })
}

fn durable_schedule_task_value(
    schedule: &crate::protocol::dto::ScheduleSummaryDto,
) -> serde_json::Value {
    let label = schedule_label(schedule, None);
    durable_schedule_task_value_with_label(schedule, &label)
}

fn durable_schedule_task_line(task: &serde_json::Value) -> String {
    let id = task.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let message = task
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let kind = task
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("schedule");
    let state = task
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let label = if message.is_empty() { kind } else { message };
    let interval_secs = task
        .get("interval_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    format!(
        "{}: {} ({}s, {})",
        schedule_display_id(id),
        label.chars().take(30).collect::<String>(),
        interval_secs,
        state,
    )
}

async fn request_workspace_schedules(
    core_client: &dyn CoreClient,
    workspace_id: &str,
) -> Result<Vec<crate::protocol::dto::ScheduleSummaryDto>, String> {
    let request = crate::core::new_request(
        format!("task-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::ScheduleList {
            workspace_id: Some(workspace_id.to_string()),
            include_archived: false,
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::ScheduleList { schedules }) => Ok(schedules),
        Ok(CoreResponse::Error { message, .. }) => Err(format!("Failed to list tasks: {message}")),
        Ok(_other) => Err("Unexpected task response".to_string()),
        Err(error) => Err(format!("Failed to list tasks: {error}")),
    }
}

async fn request_schedule_detail(
    core_client: &dyn CoreClient,
    schedule_id: &str,
) -> Option<crate::protocol::dto::ScheduleRecordDto> {
    let request = crate::core::new_request(
        format!("task-get-{}", uuid::Uuid::new_v4()),
        CoreRequest::ScheduleGet {
            schedule_id: schedule_id.to_string(),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::ScheduleGet { schedule }) => Some(schedule),
        _ => None,
    }
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
            match request_workspace_schedules(core_client.as_ref(), &workspace_id).await {
                Ok(schedules) => {
                    let mut tasks = Vec::with_capacity(schedules.len());
                    for schedule in &schedules {
                        let detail =
                            request_schedule_detail(core_client.as_ref(), &schedule.schedule_id)
                                .await;
                        let detail = detail.filter(|record| record.workspace_id == workspace_id);
                        let task = match detail.as_ref() {
                            Some(detail) => {
                                let label = schedule_label(schedule, Some(detail));
                                durable_schedule_task_value_with_label(schedule, &label)
                            }
                            None => durable_schedule_task_value(schedule),
                        };
                        tasks.push(task);
                    }
                    Some(TuiCommand::TasksListed {
                        request_id,
                        tasks,
                        error: None,
                    })
                }
                Err(error) => Some(TuiCommand::TasksListed {
                    request_id,
                    tasks: Vec::new(),
                    error: Some(error),
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
        let list: Vec<String> = tasks.iter().map(durable_schedule_task_line).collect();
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
            let schedules =
                match request_workspace_schedules(core_client.as_ref(), &workspace_id).await {
                    Ok(schedules) => schedules,
                    Err(error) => {
                        return Some(TuiCommand::TaskOperationFinished {
                            request_id,
                            op: "delete".to_string(),
                            task_id: Some(id.clone()),
                            error: Some(error),
                        });
                    }
                };
            let resolved_id = match resolve_schedule_id(&id, &schedules) {
                Ok(schedule_id) => schedule_id,
                Err(error) => {
                    return Some(TuiCommand::TaskOperationFinished {
                        request_id,
                        op: "delete".to_string(),
                        task_id: Some(id.clone()),
                        error: Some(error.to_string()),
                    });
                }
            };
            let request = crate::core::new_request(
                format!("task-delete-{}", uuid::Uuid::new_v4()),
                CoreRequest::ScheduleDelete {
                    schedule_id: resolved_id.clone(),
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
                        task_id: Some(resolved_id.clone()),
                        error: Some(format!("Failed to delete task: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(resolved_id.clone()),
                    error: Some("Unexpected task response".to_string()),
                }),
                Err(e) => Some(TuiCommand::TaskOperationFinished {
                    request_id,
                    op: "delete".to_string(),
                    task_id: Some(resolved_id),
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
            let display_id = schedule_display_id(task_id.as_deref().unwrap_or(""));
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
    use super::{
        durable_schedule_spec, durable_schedule_task_line, durable_schedule_task_value,
        durable_schedule_task_value_with_label, resolve_schedule_id, schedule_display_id,
        schedule_label, worktree_label, ScheduleIdResolutionError, SCHEDULE_DISPLAY_ID_LEN,
    };
    use serde_json::json;

    fn summary(id: &str, workspace_id: &str) -> crate::protocol::dto::ScheduleSummaryDto {
        crate::protocol::dto::ScheduleSummaryDto {
            schedule_id: id.to_string(),
            workspace_id: workspace_id.to_string(),
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
        }
    }

    fn record(
        id: &str,
        workspace_id: &str,
        job_template: serde_json::Value,
    ) -> crate::protocol::dto::ScheduleRecordDto {
        crate::protocol::dto::ScheduleRecordDto {
            schedule_id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            session_id: Some("session-1".to_string()),
            kind: json!({
                "kind": "interval",
                "every": {"secs": 60, "nanos": 0},
            }),
            job_template,
            state: "active".to_string(),
            overlap_policy: "skip_if_running".to_string(),
            missed_run_policy: json!("run_once_now"),
            next_run_at_ms: None,
            last_occurrence_at_ms: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

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
        assert_eq!(task["message"], "interval/active");
    }

    #[test]
    fn schedule_display_token_is_centralized_and_bounded() {
        assert_eq!(schedule_display_id("1234567890"), "12345678");
        assert_eq!(schedule_display_id("short"), "short");
        assert_eq!(SCHEDULE_DISPLAY_ID_LEN, 8);
    }

    #[test]
    fn schedule_id_resolver_accepts_exact_and_unique_display_prefix() {
        let schedules = vec![summary("12345678-full-id", "workspace-1")];

        assert_eq!(
            resolve_schedule_id("  12345678-full-id  ", &schedules),
            Ok("12345678-full-id".to_string())
        );
        assert_eq!(
            resolve_schedule_id("12345678", &schedules),
            Ok("12345678-full-id".to_string())
        );
    }

    #[test]
    fn schedule_id_resolver_rejects_empty_too_short_and_unknown_input() {
        let schedules = vec![summary("12345678-full-id", "workspace-1")];

        assert_eq!(
            resolve_schedule_id(" ", &schedules),
            Err(ScheduleIdResolutionError::Empty)
        );
        assert_eq!(
            resolve_schedule_id("1234567", &schedules),
            Err(ScheduleIdResolutionError::TooShort {
                minimum: SCHEDULE_DISPLAY_ID_LEN
            })
        );
        assert_eq!(
            resolve_schedule_id("abcdefgh", &schedules),
            Err(ScheduleIdResolutionError::NotFound("abcdefgh".to_string()))
        );
    }

    #[test]
    fn schedule_id_resolver_rejects_ambiguous_prefix_without_selecting_one() {
        let schedules = vec![
            summary("12345678-first", "workspace-1"),
            summary("12345678-second", "workspace-1"),
        ];

        assert_eq!(
            resolve_schedule_id("12345678", &schedules),
            Err(ScheduleIdResolutionError::Ambiguous {
                input: "12345678".to_string(),
                matches: 2,
            })
        );
    }

    #[test]
    fn schedule_id_resolver_only_considers_workspace_scoped_list_results() {
        let active_workspace_schedules = vec![summary("active-12345678", "workspace-1")];

        assert_eq!(
            resolve_schedule_id("other-12345678", &active_workspace_schedules),
            Err(ScheduleIdResolutionError::NotFound(
                "other-12345678".to_string()
            ))
        );
    }

    #[test]
    fn schedule_label_extracts_prompt_from_durable_subagent_record() {
        let schedule = summary("schedule-1", "workspace-1");
        let detail = record(
            "schedule-1",
            "workspace-1",
            json!({
                "kind": "subagent",
                "payload": {"kind": "subagent", "prompt": "check the build"}
            }),
        );

        assert_eq!(schedule_label(&schedule, Some(&detail)), "check the build");
    }

    #[test]
    fn schedule_label_falls_back_for_missing_or_unsupported_detail() {
        let schedule = summary("schedule-1", "workspace-1");
        let unsupported = record("schedule-1", "workspace-1", json!({"kind": "unknown"}));

        assert_eq!(
            schedule_label(&schedule, Some(&unsupported)),
            "interval/active"
        );
        assert_eq!(schedule_label(&schedule, None), "interval/active");
        assert_eq!(
            durable_schedule_task_value(&schedule)["message"],
            "interval/active"
        );
    }

    #[test]
    fn schedule_label_and_row_keep_long_prompt_bounded_at_presentation_boundary() {
        let schedule = summary("schedule-1", "workspace-1");
        let detail = record(
            "schedule-1",
            "workspace-1",
            json!({
                "kind": "subagent",
                "payload": {"kind": "subagent", "prompt": "abcdefghijklmnopqrstuvwxyz1234567890"}
            }),
        );
        let task = durable_schedule_task_value_with_label(
            &schedule,
            &schedule_label(&schedule, Some(&detail)),
        );

        assert_eq!(
            durable_schedule_task_line(&task),
            "schedule: abcdefghijklmnopqrstuvwxyz1234 (60s, active)"
        );
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
