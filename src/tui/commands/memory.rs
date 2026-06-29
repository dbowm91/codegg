//! Memory command handlers for summary, search, remember, and forget operations.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

#[allow(dead_code)]
pub(crate) async fn handle_memory_summary(app: &mut App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let project_hash = format!(
        "{:x}",
        md5::compute(app.session_state.project_dir.as_bytes())
    );
    let project_namespace = format!("project/{}", project_hash);
    let req_prefs = crate::core::new_request(
        format!("memory-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryList {
            namespace: "user/preferences".to_string(),
        },
    );
    let req_proj = crate::core::new_request(
        format!("memory-list-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryList {
            namespace: project_namespace.clone(),
        },
    );
    let prefs = match core_client.request(req_prefs).await {
        Ok(CoreResponse::Json { data }) => data
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let proj = match core_client.request(req_proj).await {
        Ok(CoreResponse::Json { data }) => data
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let total = prefs.len() + proj.len();
    if total == 0 {
        app.messages_state
            .toasts
            .info("No memories yet. Use /memory-remember <text> to save something.");
        return;
    }
    let mut lines = vec![format!("Memory Summary ({} total):", total)];
    if !prefs.is_empty() {
        lines.push(format!("  user/preferences ({}):", prefs.len()));
        for m in prefs.iter().take(5) {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect::<String>();
            let title = m
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            lines.push(format!("    - [{}] {}", id, title));
        }
    }
    if !proj.is_empty() {
        lines.push(format!("  {} ({}):", project_namespace, proj.len()));
        for m in proj.iter().take(5) {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect::<String>();
            let title = m
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            lines.push(format!("    - [{}] {}", id, title));
        }
    }
    app.messages_state.toasts.info(&lines.join("\n"));
}

#[allow(dead_code)]
pub(crate) async fn handle_memory_search(app: &mut App, query: String) {
    if query.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-search <query>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-search-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemorySearch {
            query: query.clone(),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let results = data
                .get("memories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if results.is_empty() {
                app.messages_state
                    .toasts
                    .info(&format!("No memories found matching '{}'", query));
            } else {
                let lines: Vec<String> = results
                    .iter()
                    .take(10)
                    .map(|m| {
                        let id = m
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .chars()
                            .take(8)
                            .collect::<String>();
                        let title = m
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(untitled)");
                        format!("- [{}] {}", id, title)
                    })
                    .collect();
                app.messages_state.toasts.info(&format!(
                    "Found {} memories:\n{}",
                    results.len(),
                    lines.join("\n")
                ));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory search response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", e)),
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_memory_remember(app: &mut App, text: String) {
    if text.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-remember <text to remember>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-remember-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryRemember {
            text,
            namespace: Some("user/preferences".to_string()),
        },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { .. }) | Ok(CoreResponse::Ack) => {
            app.messages_state.toasts.info("Remembered")
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory remember failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory remember response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory remember failed: {}", e)),
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_memory_forget(app: &mut App, id: String) {
    if id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-forget <id>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    };
    let request = crate::core::new_request(
        format!("memory-forget-{}", uuid::Uuid::new_v4()),
        CoreRequest::MemoryForget { id: id.clone() },
    );
    match core_client.request(request).await {
        Ok(CoreResponse::Json { data }) => {
            let deleted = data
                .get("deleted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if deleted {
                app.messages_state.toasts.info("Memory deleted");
            } else {
                app.messages_state
                    .toasts
                    .warning(&format!("Memory '{}' not found", id));
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory forget failed: {}", message)),
        Ok(other) => app
            .messages_state
            .toasts
            .warning(&format!("Unexpected memory forget response: {:?}", other)),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory forget failed: {}", e)),
    }
}

pub(crate) fn start_memory_summary(app: &mut App) {
    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "memory_summary",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::MemoryResult {
                    toast_message: "Core client unavailable".to_string(),
                    is_error: true,
                });
            };

            let project_hash = format!("{:x}", md5::compute(project_dir.as_bytes()));
            let project_namespace = format!("project/{}", project_hash);
            let req_prefs = crate::core::new_request(
                format!("memory-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::MemoryList {
                    namespace: "user/preferences".to_string(),
                },
            );
            let req_proj = crate::core::new_request(
                format!("memory-list-{}", uuid::Uuid::new_v4()),
                CoreRequest::MemoryList {
                    namespace: project_namespace.clone(),
                },
            );
            let prefs = match core_client.request(req_prefs).await {
                Ok(CoreResponse::Json { data }) => data
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let proj = match core_client.request(req_proj).await {
                Ok(CoreResponse::Json { data }) => data
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let total = prefs.len() + proj.len();
            if total == 0 {
                return Some(TuiCommand::MemoryResult {
                    toast_message:
                        "No memories yet. Use /memory-remember <text> to save something."
                            .to_string(),
                    is_error: false,
                });
            }
            let mut lines = vec![format!("Memory Summary ({} total):", total)];
            if !prefs.is_empty() {
                lines.push(format!("  user/preferences ({}):", prefs.len()));
                for m in prefs.iter().take(5) {
                    let id = m
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .chars()
                        .take(8)
                        .collect::<String>();
                    let title = m
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(untitled)");
                    lines.push(format!("    - [{}] {}", id, title));
                }
            }
            if !proj.is_empty() {
                lines.push(format!("  {} ({}):", project_namespace, proj.len()));
                for m in proj.iter().take(5) {
                    let id = m
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .chars()
                        .take(8)
                        .collect::<String>();
                    let title = m
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(untitled)");
                    lines.push(format!("    - [{}] {}", id, title));
                }
            }
            Some(TuiCommand::MemoryResult {
                toast_message: lines.join("\n"),
                is_error: false,
            })
        },
    );
}

pub(crate) fn start_memory_search(app: &mut App, query: String) {
    if query.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-search <query>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "memory_search",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::MemoryResult {
                    toast_message: "Core client unavailable".to_string(),
                    is_error: true,
                });
            };

            let request = crate::core::new_request(
                format!("memory-search-{}", uuid::Uuid::new_v4()),
                CoreRequest::MemorySearch {
                    query: query.clone(),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data }) => {
                    let results = data
                        .get("memories")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if results.is_empty() {
                        return Some(TuiCommand::MemoryResult {
                            toast_message: format!("No memories found matching '{}'", query),
                            is_error: false,
                        });
                    }
                    let lines: Vec<String> = results
                        .iter()
                        .take(10)
                        .map(|m| {
                            let id = m
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .chars()
                                .take(8)
                                .collect::<String>();
                            let title = m
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("(untitled)");
                            format!("- [{}] {}", id, title)
                        })
                        .collect();
                    Some(TuiCommand::MemoryResult {
                        toast_message: format!(
                            "Found {} memories:\n{}",
                            results.len(),
                            lines.join("\n")
                        ),
                        is_error: false,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory search failed: {}", message),
                    is_error: true,
                }),
                Ok(other) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Unexpected memory search response: {:?}", other),
                    is_error: true,
                }),
                Err(e) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory search failed: {}", e),
                    is_error: true,
                }),
            }
        },
    );
}

pub(crate) fn start_memory_remember(app: &mut App, text: String) {
    if text.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-remember <text to remember>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "memory_remember",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::MemoryResult {
                    toast_message: "Core client unavailable".to_string(),
                    is_error: true,
                });
            };

            let request = crate::core::new_request(
                format!("memory-remember-{}", uuid::Uuid::new_v4()),
                CoreRequest::MemoryRemember {
                    text,
                    namespace: Some("user/preferences".to_string()),
                },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { .. }) | Ok(CoreResponse::Ack) => {
                    Some(TuiCommand::MemoryResult {
                        toast_message: "Remembered".to_string(),
                        is_error: false,
                    })
                }
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory remember failed: {}", message),
                    is_error: true,
                }),
                Ok(other) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Unexpected memory remember response: {:?}", other),
                    is_error: true,
                }),
                Err(e) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory remember failed: {}", e),
                    is_error: true,
                }),
            }
        },
    );
}

pub(crate) fn start_memory_forget(app: &mut App, id: String) {
    if id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-forget <id>");
        return;
    }

    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "memory_forget",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::MemoryResult {
                    toast_message: "Core client unavailable".to_string(),
                    is_error: true,
                });
            };

            let request = crate::core::new_request(
                format!("memory-forget-{}", uuid::Uuid::new_v4()),
                CoreRequest::MemoryForget { id: id.clone() },
            );
            match core_client.request(request).await {
                Ok(CoreResponse::Json { data }) => {
                    let deleted = data
                        .get("deleted")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if deleted {
                        Some(TuiCommand::MemoryResult {
                            toast_message: "Memory deleted".to_string(),
                            is_error: false,
                        })
                    } else {
                        Some(TuiCommand::MemoryResult {
                            toast_message: format!("Memory '{}' not found", id),
                            is_error: false,
                        })
                    }
                }
                Ok(CoreResponse::Error { message, .. }) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory forget failed: {}", message),
                    is_error: true,
                }),
                Ok(other) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Unexpected memory forget response: {:?}", other),
                    is_error: true,
                }),
                Err(e) => Some(TuiCommand::MemoryResult {
                    toast_message: format!("Memory forget failed: {}", e),
                    is_error: true,
                }),
            }
        },
    );
}

pub(crate) fn apply_memory_result(app: &mut App, toast_message: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&toast_message);
    } else {
        app.messages_state.toasts.info(&toast_message);
    }
}
