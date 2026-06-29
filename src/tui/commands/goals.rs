//! Goal-related TUI command handlers.
//!
//! Extracted from `src/tui/mod.rs` to keep the main module manageable.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

pub(crate) async fn handle_goal_set(
    app: &mut App,
    session_id: String,
    project_id: String,
    objective: String,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    tokio::spawn(async move {
        let request = crate::core::new_request(
            format!("goal-set-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalSet {
                session_id,
                project_id,
                objective,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("Goal");
                let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                tracing::info!("Goal created: {} ({})", title, id);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal set failed: {}", message);
            }
            Err(e) => {
                tracing::warn!("Goal set error: {}", e);
            }
            _ => {}
        }
    });
}

pub(crate) async fn handle_goal_from_file(
    app: &mut App,
    session_id: String,
    project_id: String,
    path: String,
) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    tokio::spawn(async move {
        let request = crate::core::new_request(
            format!("goal-from-file-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalFromFile {
                session_id,
                project_id,
                path,
            },
        );
        match core_client.request(request).await {
            Ok(CoreResponse::Json { data }) => {
                let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("Goal");
                let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                tracing::info!("Goal from file: {} ({})", title, id);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal from-file failed: {}", message);
            }
            Err(e) => {
                tracing::warn!("Goal from-file error: {}", e);
            }
            _ => {}
        }
    });
}

pub(crate) fn start_goal_show(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "goal_show",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "show".to_string(),
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response = core_client
                .request(crate::core::new_request(
                    format!("goal-show-{}", uuid::Uuid::new_v4()),
                    CoreRequest::GoalShow {
                        session_id: session_id.clone(),
                    },
                ))
                .await;
            match response {
                Ok(resp) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "show".to_string(),
                    response: Some(resp),
                    error: None,
                }),
                Err(e) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "show".to_string(),
                    response: None,
                    error: Some(format!("Goal show error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_goal_operation_finished(
    app: &mut App,
    session_id: String,
    op: String,
    response: Option<CoreResponse>,
    error: Option<String>,
) {
    if let Some(err) = error {
        app.messages_state.toasts.warning(&err);
        return;
    }
    let Some(response) = response else { return };
    match op.as_str() {
        "show" => {
            if let CoreResponse::Json { data } = response {
                if data.get("active").and_then(|v| v.as_bool()) == Some(false) {
                    app.messages_state.toasts.info("No active goal");
                } else if let Some(rendered) = data.get("rendered").and_then(|v| v.as_str()) {
                    app.messages_state.toasts.info(rendered);
                }
            }
        }
        "checkpoint" => {
            if let CoreResponse::Json { data } = response {
                if let Some(path) = data.get("checkpoint_path").and_then(|v| v.as_str()) {
                    tracing::info!("Goal checkpoint: {}", path);
                }
            }
        }
        "budget-raise" => {
            if let CoreResponse::Json { data } = response {
                let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("ok");
                let label = data.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                let value = data.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
                app.messages_state.toasts.info(&format!(
                    "Goal budget: {} = {} (status: {})",
                    label, value, status
                ));
            }
        }
        _ => {}
    }
    let _ = session_id;
}

pub(crate) fn start_goal_checkpoint(app: &mut App, session_id: String, project_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "goal_checkpoint",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "checkpoint".to_string(),
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response = core_client
                .request(crate::core::new_request(
                    format!("goal-checkpoint-{}", uuid::Uuid::new_v4()),
                    CoreRequest::GoalCheckpoint {
                        session_id: session_id.clone(),
                        project_id,
                    },
                ))
                .await;
            match response {
                Ok(resp) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "checkpoint".to_string(),
                    response: Some(resp),
                    error: None,
                }),
                Err(e) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "checkpoint".to_string(),
                    response: None,
                    error: Some(format!("Goal checkpoint error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_goal_budget_raise(
    app: &mut App,
    session_id: String,
    label: String,
    value: i64,
    max_turns: Option<i64>,
    max_model_tokens: Option<i64>,
    max_tool_calls: Option<i64>,
    max_wallclock_secs: Option<i64>,
) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "goal_budget_raise",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "budget-raise".to_string(),
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let response = core_client
                .request(crate::core::new_request(
                    format!("goal-budget-raise-{}", uuid::Uuid::new_v4()),
                    CoreRequest::GoalSetBudget {
                        session_id: session_id.clone(),
                        max_turns,
                        max_model_tokens,
                        max_tool_calls,
                        max_wallclock_secs,
                    },
                ))
                .await;
            match response {
                Ok(CoreResponse::Json { data }) => {
                    // Enrich the data with label and value for the apply function.
                    let mut enriched = data;
                    enriched["label"] = serde_json::Value::String(label);
                    enriched["value"] = serde_json::json!(value);
                    Some(TuiCommand::GoalOperationFinished {
                        session_id,
                        op: "budget-raise".to_string(),
                        response: Some(CoreResponse::Json { data: enriched }),
                        error: None,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => {
                    Some(TuiCommand::GoalOperationFinished {
                        session_id,
                        op: "budget-raise".to_string(),
                        response: None,
                        error: Some(format!("Budget update failed: {}", message)),
                    })
                }
                Ok(_other) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "budget-raise".to_string(),
                    response: None,
                    error: Some("Unexpected budget response".to_string()),
                }),
                Err(e) => Some(TuiCommand::GoalOperationFinished {
                    session_id,
                    op: "budget-raise".to_string(),
                    response: None,
                    error: Some(format!("Budget update error: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn start_refresh_session_state(app: &mut App, session_id: String) {
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Command,
        "refresh_session_state",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SessionStateRefreshed {
                    todos: Vec::new(),
                    active_goal: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            // Hydrate the todo list.
            let todos = match core_client
                .request(crate::core::new_request(
                    format!("todo-list-{}", uuid::Uuid::new_v4()),
                    CoreRequest::TodoList {
                        session_id: session_id.clone(),
                    },
                ))
                .await
            {
                Ok(CoreResponse::Json { data }) => {
                    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
                        items
                            .iter()
                            .filter_map(|v| {
                                Some(crate::tui::app::TodoEntry {
                                    content: v.get("content")?.as_str()?.to_string(),
                                    status: v.get("status")?.as_str()?.to_string(),
                                    priority: v.get("priority")?.as_str()?.to_string(),
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            };
            // Hydrate the active goal.
            let active_goal = match core_client
                .request(crate::core::new_request(
                    format!("active-goal-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ActiveGoalLoad {
                        session_id: session_id.clone(),
                    },
                ))
                .await
            {
                Ok(CoreResponse::Json { data }) => {
                    if data.get("active").and_then(|v| v.as_bool()) == Some(true) {
                        if let Some(goal_val) = data.get("goal") {
                            serde_json::from_value::<crate::bus::events::GoalSnapshot>(
                                goal_val.clone(),
                            )
                            .ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            Some(TuiCommand::SessionStateRefreshed {
                todos,
                active_goal,
                error: None,
            })
        },
    );
}

pub(crate) fn apply_session_state_refreshed(
    app: &mut App,
    todos: Vec<crate::tui::app::TodoEntry>,
    active_goal: Option<crate::bus::events::GoalSnapshot>,
    error: Option<String>,
) {
    if let Some(err) = error {
        tracing::warn!("Session state refresh failed: {}", err);
        return;
    }
    app.set_todos(todos);
    app.set_active_goal(active_goal);
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_goal_show(app: &mut App, session_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let response = core_client
        .request(crate::core::new_request(
            format!("goal-show-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalShow { session_id },
        ))
        .await;
    match response {
        Ok(CoreResponse::Json { data }) => {
            if data.get("active").and_then(|v| v.as_bool()) == Some(false) {
                app.messages_state.toasts.info("No active goal");
            } else if let Some(rendered) = data.get("rendered").and_then(|v| v.as_str()) {
                app.messages_state.toasts.info(rendered);
            }
        }
        Ok(CoreResponse::Error { message, .. }) => {
            app.messages_state.toasts.warning(&message);
        }
        Err(e) => {
            app.messages_state
                .toasts
                .warning(&format!("Goal show error: {}", e));
        }
        _ => {}
    }
}

pub(crate) async fn handle_goal_simple(app: &mut App, request: CoreRequest, label: &str) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let label = label.to_string();
    tokio::spawn(async move {
        let response = core_client
            .request(crate::core::new_request(
                format!("goal-{}-{}", label, uuid::Uuid::new_v4()),
                request,
            ))
            .await;
        match response {
            Ok(CoreResponse::Json { data }) => {
                tracing::info!("Goal {} response: {}", label, data);
            }
            Ok(CoreResponse::Error { message, .. }) => {
                tracing::warn!("Goal {} failed: {}", label, message);
            }
            Err(e) => {
                tracing::warn!("Goal {} error: {}", label, e);
            }
            _ => {}
        }
    });
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_refresh_session_state(app: &mut App, session_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        return;
    };
    // Hydrate the todo list.
    if let Ok(CoreResponse::Json { data }) = core_client
        .request(crate::core::new_request(
            format!("todo-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::TodoList {
                session_id: session_id.clone(),
            },
        ))
        .await
    {
        if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
            let entries: Vec<crate::tui::app::TodoEntry> = items
                .iter()
                .filter_map(|v| {
                    Some(crate::tui::app::TodoEntry {
                        content: v.get("content")?.as_str()?.to_string(),
                        status: v.get("status")?.as_str()?.to_string(),
                        priority: v.get("priority")?.as_str()?.to_string(),
                    })
                })
                .collect();
            app.set_todos(entries);
        }
    }
    // Hydrate the active goal.
    if let Ok(CoreResponse::Json { data }) = core_client
        .request(crate::core::new_request(
            format!("active-goal-{}", uuid::Uuid::new_v4()),
            CoreRequest::ActiveGoalLoad {
                session_id: session_id.clone(),
            },
        ))
        .await
    {
        if data.get("active").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(goal_val) = data.get("goal") {
                if let Ok(snap) =
                    serde_json::from_value::<crate::bus::events::GoalSnapshot>(goal_val.clone())
                {
                    app.set_active_goal(Some(snap));
                }
            }
        } else {
            app.set_active_goal(None);
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn handle_goal_checkpoint(app: &mut App, session_id: String, project_id: String) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let response = core_client
        .request(crate::core::new_request(
            format!("goal-checkpoint-{}", uuid::Uuid::new_v4()),
            CoreRequest::GoalCheckpoint {
                session_id,
                project_id,
            },
        ))
        .await;
    match response {
        Ok(CoreResponse::Json { data }) => {
            if let Some(path) = data.get("checkpoint_path").and_then(|v| v.as_str()) {
                tracing::info!("Goal checkpoint: {}", path);
            }
        }
        Ok(CoreResponse::Error { message, .. }) => {
            tracing::warn!("Goal checkpoint failed: {}", message);
        }
        Err(e) => {
            tracing::warn!("Goal checkpoint error: {}", e);
        }
        _ => {}
    }
}

/// `/goal budget …` handler. Two flavors:
///   * `show` — fetch the active goal and render its budget/usage as
///     a toast (sidebar already shows live status).
///   * `raise <axis> <n>` — bump a single axis of the active goal's
///     budget. Valid axes: `tokens`, `turns`, `tool-calls`, `wallclock`.
pub(crate) async fn handle_goal_budget(app: &mut App, session_id: String, subcommand: String) {
    let trimmed = subcommand.trim();
    let mut parts = trimmed.splitn(3, ' ');
    let head = parts.next().unwrap_or("").to_string();
    let rest = parts.collect::<Vec<_>>().join(" ").trim().to_string();

    if head == "show" {
        // Render a compact budget/usage summary by reading the live
        // GoalSnapshot already on the app, if any.
        if let Some(ref g) = app.active_goal {
            let line = crate::tui::app::format_goal_status_line(g);
            app.messages_state.toasts.info(&line);
        } else {
            app.messages_state
                .toasts
                .warning("No active goal — set one with /goal set <objective>");
        }
        return;
    }

    if head == "raise" {
        let mut parts = rest.splitn(2, ' ');
        let axis = parts.next().unwrap_or("").to_string();
        let value_str = parts.next().unwrap_or("").trim();
        if axis.is_empty() || value_str.is_empty() {
            app.messages_state
                .toasts
                .warning("Usage: /goal budget raise <tokens|turns|tool-calls|wallclock> <n>");
            return;
        }
        let value: i64 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => {
                app.messages_state
                    .toasts
                    .warning(&format!("Invalid number: {}", value_str));
                return;
            }
        };
        if value < 1 {
            app.messages_state
                .toasts
                .warning("Budget must be a positive integer");
            return;
        }
        let mut max_turns: Option<i64> = None;
        let mut max_model_tokens: Option<i64> = None;
        let mut max_tool_calls: Option<i64> = None;
        let mut max_wallclock_secs: Option<i64> = None;
        let mut label = axis.clone();
        match axis.as_str() {
            "tokens" => max_model_tokens = Some(value),
            "turns" => max_turns = Some(value),
            "tool-calls" | "tool_calls" | "calls" => {
                max_tool_calls = Some(value);
                label = "tool-calls".into();
            }
            "wallclock" | "wall" => max_wallclock_secs = Some(value),
            _ => {
                app.messages_state.toasts.warning(&format!(
                    "Unknown axis '{}'. Use tokens, turns, tool-calls, or wallclock.",
                    axis
                ));
                return;
            }
        }
        start_goal_budget_raise(
            app,
            session_id,
            label,
            value,
            max_turns,
            max_model_tokens,
            max_tool_calls,
            max_wallclock_secs,
        );
        return;
    }

    app.messages_state
        .toasts
        .warning("Usage: /goal budget [show | raise <axis> <n>]");
}
