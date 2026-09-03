//! Memory command handlers for summary, search, remember, and forget operations.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

#[allow(dead_code)]
#[cfg(test)]
pub(crate) async fn handle_memory_summary(app: &mut App) {
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Core unavailable — check daemon status with /doctor");
        return;
    };
    let project_namespace = crate::memory::project_namespace(&app.session_state.project_dir);
    if let Some(store) = app.memory_store.as_ref() {
        let _ = store.migrate_project_namespace(&app.session_state.project_dir);
    }
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
    let mut proj = match core_client.request(req_proj).await {
        Ok(CoreResponse::Json { data }) => data
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if proj.is_empty() {
        let legacy_request = crate::core::new_request(
            format!("memory-list-{}", uuid::Uuid::new_v4()),
            CoreRequest::MemoryList {
                namespace: crate::memory::legacy_project_namespace(&app.session_state.project_dir),
            },
        );
        proj = match core_client.request(legacy_request).await {
            Ok(CoreResponse::Json { data }) => data
                .get("memories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    }
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
    // Multi-line summaries belong in the scrollable info surface; a
    // joined toast of 5+ rows eats the toast column and pushes other
    // toasts out before they can be read.
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::MemoryResults,
        lines,
    );
}

#[allow(dead_code)]
#[cfg(test)]
pub(crate) async fn handle_memory_search(app: &mut App, query: String) {
    if query.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-search <query>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Core unavailable — check daemon status with /doctor");
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
                let mut result_lines = vec![format!("Found {} memories:", results.len())];
                result_lines.extend(lines);
                app.show_short_or_info(
                    crate::tui::components::dialogs::info::InfoType::MemoryResults,
                    result_lines,
                );
            }
        }
        Ok(CoreResponse::Error { message, .. }) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", message)),
        Ok(_other) => app
            .messages_state
            .toasts
            .warning("Unexpected memory response"),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory search failed: {}", e)),
    }
}

#[allow(dead_code)]
#[cfg(test)]
pub(crate) async fn handle_memory_remember(app: &mut App, text: String) {
    if text.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-remember <text to remember>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Core unavailable — check daemon status with /doctor");
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
        Ok(_other) => app
            .messages_state
            .toasts
            .warning("Unexpected memory response"),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory remember failed: {}", e)),
    }
}

#[allow(dead_code)]
#[cfg(test)]
pub(crate) async fn handle_memory_forget(app: &mut App, id: String) {
    if id.is_empty() {
        app.messages_state
            .toasts
            .warning("Usage: /memory-forget <id>");
        return;
    }
    let Some(core_client) = app.core_client.clone() else {
        app.messages_state
            .toasts
            .warning("Core unavailable — check daemon status with /doctor");
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
        Ok(_other) => app
            .messages_state
            .toasts
            .warning("Unexpected memory response"),
        Err(e) => app
            .messages_state
            .toasts
            .warning(&format!("Memory forget failed: {}", e)),
    }
}

pub(crate) fn start_memory_summary(app: &mut App) {
    let core_client = app.core_client.clone();
    let project_dir = app.session_state.project_dir.clone();
    if let Some(store) = app.memory_store.as_ref() {
        let _ = store.migrate_project_namespace(&project_dir);
    }
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "memory_summary",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::MemoryResult {
                    toast_message: "Core unavailable — check daemon status with /doctor"
                        .to_string(),
                    is_error: true,
                });
            };

            let project_namespace = crate::memory::project_namespace(&project_dir);
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
            let mut proj = match core_client.request(req_proj).await {
                Ok(CoreResponse::Json { data }) => data
                    .get("memories")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            if proj.is_empty() {
                let legacy_request = crate::core::new_request(
                    format!("memory-list-{}", uuid::Uuid::new_v4()),
                    CoreRequest::MemoryList {
                        namespace: crate::memory::legacy_project_namespace(&project_dir),
                    },
                );
                proj = match core_client.request(legacy_request).await {
                    Ok(CoreResponse::Json { data }) => data
                        .get("memories")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
            }
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
                    toast_message: "Core unavailable — check daemon status with /doctor"
                        .to_string(),
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
                Ok(_other) => Some(TuiCommand::MemoryResult {
                    toast_message: "Unexpected memory response".to_string(),
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
                    toast_message: "Core unavailable — check daemon status with /doctor"
                        .to_string(),
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
                Ok(_other) => Some(TuiCommand::MemoryResult {
                    toast_message: "Unexpected memory response".to_string(),
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
                    toast_message: "Core unavailable — check daemon status with /doctor"
                        .to_string(),
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
                Ok(_other) => Some(TuiCommand::MemoryResult {
                    toast_message: "Unexpected memory response".to_string(),
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
        let lines: Vec<String> = toast_message.lines().map(|s| s.to_string()).collect();
        if lines.len() > 3 {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::MemoryResults,
                lines,
            );
        } else {
            app.messages_state.toasts.info(&toast_message);
        }
    }
}

pub(crate) fn start_habit_list(app: &mut App, ready_only: bool) {
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "habit_list",
        async move {
            let result = (|| {
                let store = crate::memory::habit::HabitStore::new()
                    .map_err(|error| format!("Habit store unavailable: {error}"))?;
                let status =
                    ready_only.then_some(crate::memory::habit::HabitCandidateStatus::Ready);
                let candidates = store
                    .list(&project_dir, status, 32)
                    .map_err(|error| format!("Failed to load habit candidates: {error}"))?;
                if candidates.is_empty() {
                    return Ok(if ready_only {
                        "No ready habit candidates.".to_string()
                    } else {
                        "No habit candidates yet.".to_string()
                    });
                }
                let mut lines = vec![format!("Workflow habit candidates ({}):", candidates.len())];
                for candidate in candidates {
                    let id = candidate.id.as_str().chars().take(8).collect::<String>();
                    let status = format!("{:?}", candidate.status).to_lowercase();
                    lines.push(format!(
                        "- [{}] {} — {} ({} successes, {} sessions)",
                        id,
                        status,
                        candidate.summary(),
                        candidate.successful_occurrences,
                        candidate.distinct_sessions
                    ));
                }
                lines.push("Dismiss with /habit-dismiss <id>. Ready candidates are eligible for a later skill proposal.".to_string());
                Ok(lines.join("\n"))
            })();
            Some(match result {
                Ok(message) => TuiCommand::HabitResult {
                    toast_message: message,
                    is_error: false,
                },
                Err(message) => TuiCommand::HabitResult {
                    toast_message: message,
                    is_error: true,
                },
            })
        },
    );
}

pub(crate) fn start_habit_dismiss(app: &mut App, id: String) {
    let Some(id) = crate::memory::habit::HabitId::parse(&id) else {
        app.messages_state
            .toasts
            .warning("Invalid habit candidate ID");
        return;
    };
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "habit_dismiss",
        async move {
            let result = (|| {
                let store = crate::memory::habit::HabitStore::new()
                    .map_err(|error| format!("Habit store unavailable: {error}"))?;
                let dismissed = store
                    .dismiss(&project_dir, &id)
                    .map_err(|error| format!("Failed to dismiss habit candidate: {error}"))?;
                Ok(if dismissed {
                    "Habit candidate dismissed.".to_string()
                } else {
                    "Habit candidate not found or already finalized.".to_string()
                })
            })();
            Some(match result {
                Ok(message) => TuiCommand::HabitResult {
                    toast_message: message,
                    is_error: false,
                },
                Err(message) => TuiCommand::HabitResult {
                    toast_message: message,
                    is_error: true,
                },
            })
        },
    );
}

pub(crate) fn apply_habit_result(app: &mut App, message: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&message);
    } else {
        let lines: Vec<String> = message.lines().map(str::to_string).collect();
        if lines.len() > 3 {
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::MemoryResults,
                lines,
            );
        } else {
            app.messages_state.toasts.info(&message);
        }
    }
}

/// Execute publication only for the native/TUI user command.  There is no
/// corresponding model-facing tool or prompt path.
pub(crate) fn start_skill_publish(
    app: &mut App,
    id: String,
    target_scope: crate::skills::promotion::SkillTargetScope,
) {
    let Some(proposal_id) = crate::skills::promotion::SkillProposalId::parse(&id) else {
        app.messages_state
            .toasts
            .warning("Invalid skill proposal ID");
        return;
    };
    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();
    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "skill_publish",
        async move {
            let result = (|| {
                let store = crate::skills::promotion::SkillPromotionStore::new()
                    .map_err(|error| format!("Skill promotion store unavailable: {error}"))?;
                let proposal = store
                    .get_proposal(&project_dir, &proposal_id)
                    .map_err(|error| format!("Could not load skill proposal: {error}"))?
                    .ok_or_else(|| "Skill proposal not found".to_string())?;
                let request = crate::skills::publish::SkillPublicationRequest {
                    proposal_id: proposal.id.clone(),
                    expected_revision: proposal.revision,
                    expected_content_digest: proposal.content_digest.clone(),
                    target_scope,
                };
                let global_config_dir = dirs::config_dir()
                    .ok_or_else(|| "No platform config directory is available".to_string())?;
                let service = crate::skills::publish::SkillPublicationService::new(store);
                let result = service
                    .publish(
                        &project_dir,
                        std::path::Path::new(&project_dir),
                        &global_config_dir,
                        request,
                        chrono::Utc::now().timestamp_millis(),
                    )
                    .map_err(|error| error.to_string())?;
                let mut message = if result.idempotent {
                    format!(
                        "Skill publication already exists: {} (digest {}).",
                        result.relative_path,
                        result.content_digest.chars().take(12).collect::<String>()
                    )
                } else if result.reconciled {
                    format!(
                        "Reconciled published skill: {} (digest {}).",
                        result.relative_path,
                        result.content_digest.chars().take(12).collect::<String>()
                    )
                } else {
                    format!(
                        "Published skill: {} (digest {}). Refreshing runtime assets for subsequent turns.",
                        result.relative_path,
                        result.content_digest.chars().take(12).collect::<String>()
                    )
                };
                if let Some(shadowed_by) = result.shadowed_by {
                    message.push_str(&format!(
                        " Effective resolution is shadowed by {shadowed_by}."
                    ));
                }
                Ok(message)
            })();
            Some(match result {
                Ok(message) => TuiCommand::SkillPublishFinished {
                    message,
                    is_error: false,
                },
                Err(message) => TuiCommand::SkillPublishFinished {
                    message,
                    is_error: true,
                },
            })
        },
    );
}

pub(crate) fn apply_skill_publish_finished(app: &mut App, message: String, is_error: bool) {
    if is_error {
        app.messages_state.toasts.error(&message);
        return;
    }
    app.messages_state.toasts.success(&message);
    // The existing daemon-owned refresh path is the only authority allowed
    // to publish a new runtime asset generation.
    crate::tui::commands::agents::start_refresh_assets(app);
}
