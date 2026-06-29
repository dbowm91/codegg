//! Bus event handling: processes AppEvent variants from the global event bus.

use super::super::app::{App, SessionStatus};
use super::super::components::toast::Toast;
use crate::bus::events::AppEvent;
use crate::permission::PermissionRequest;
use crate::protocol::core::{CoreRequest, CoreResponse};

/// Handle a batch of bus events. Returns `true` if the UI should re-render.
pub(crate) fn handle_app_event_batch(app: &mut App, events: Vec<AppEvent>) -> bool {
    let mut needs_render = false;
    for event in events {
        needs_render |= handle_single_event(app, event);
    }
    needs_render
}

fn handle_single_event(app: &mut App, event: AppEvent) -> bool {
    match event {
        AppEvent::TextDelta {
            delta, session_id, ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                session_id = %session_id,
                delta_len = delta.len(),
                "TextDelta received"
            );
            let delta_str = delta.to_string();
            if delta_str.contains('\n') {
                app.messages_state.messages.finalize_streaming();
            }
            app.add_live_output_delta(&delta_str);
            app.messages_state.messages.add_streaming_token(&delta_str);
            app.streaming_active = true;
            if matches!(app.session_state.session_status, SessionStatus::Working) {
                app.status_bar
                    .set_thinking(true, Some("Thinking...".to_string()));
            }
            true
        }
        AppEvent::ReasoningDelta { delta, .. } => {
            tracing::debug!(target: "codegg::tui::events", "ReasoningDelta received");
            app.messages_state.messages.add_reasoning(delta);
            app.streaming_active = true;
            true
        }
        AppEvent::ToolCallStarted {
            tool_name,
            tool_id,
            arguments,
            ..
        } => {
            tracing::debug!(target: "codegg::tui::events", tool = %tool_name, "ToolCallStarted");
            app.messages_state.messages.finalize_streaming();
            app.streaming_active = true;
            match serde_json::from_str::<serde_json::Value>(&arguments) {
                Ok(args_val) => {
                    app.messages_state
                        .messages
                        .add_tool_call(tool_id.clone(), tool_name, args_val);
                    app.messages_state.messages.mark_tool_call_running(&tool_id);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse tool call arguments for {}: {} (raw: {:?})",
                        tool_name,
                        e,
                        &arguments[..arguments.len().min(200)]
                    );
                    app.messages_state.messages.add_tool_call(
                        tool_id.clone(),
                        tool_name,
                        serde_json::Value::Null,
                    );
                    app.messages_state.messages.mark_tool_call_running(&tool_id);
                }
            }
            true
        }
        AppEvent::ToolResult {
            tool_id,
            tool_name: _,
            output,
            success,
            ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                tool_id = %tool_id,
                "ToolResult received"
            );
            let status = if success {
                crate::session::message::ToolStatus::Completed
            } else {
                crate::session::message::ToolStatus::Error
            };
            app.messages_state
                .messages
                .update_tool_call(&tool_id, output, status, None, None, None);
            true
        }
        AppEvent::AgentFinished {
            stop_reason,
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
            ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                stop_reason = %stop_reason,
                "AgentFinished received"
            );
            if stop_reason != "tool_calls" {
                app.session_state.session_status = SessionStatus::Idle;
                app.prompt_state.pending_send = false;
                app.status_bar.set_thinking(false, None);
                app.streaming_active = false;

                if let (Some(in_tok), Some(out_tok)) = (input_tokens, output_tokens) {
                    app.set_tokens(in_tok as u64, out_tok as u64);
                    if let Some(ct) = cached_tokens {
                        app.session_state.cached_tokens = ct as u64;
                    }
                }
                if let Some(rt) = reasoning_tokens {
                    app.session_state.reasoning_tokens += rt;
                }

                let should_consolidate = app.memory_store.is_some()
                    && crate::config::schema::Config::load()
                        .ok()
                        .and_then(|c| c.experimental)
                        .and_then(|e| e.memory_auto_consolidate)
                        .unwrap_or(false);
                if should_consolidate {
                    let session_id = app.session_state.session.as_ref().map(|s| s.id.clone());
                    let message_store = app.message_store.clone();
                    let core_client = app.core_client.clone();
                    let memory_store = app.memory_store.clone();
                    let project_dir = app.session_state.project_dir.clone();

                    app.task_registry.spawn(
                        crate::tui::task_lifecycle::TuiTaskKind::Memory,
                        "memory_consolidation",
                        async move {
                            let project_hash =
                                format!("{:x}", md5::compute(project_dir.as_bytes()));
                            let messages = if let (Some(client), Some(sid)) =
                                (core_client, session_id.clone())
                            {
                                let request = crate::core::new_request(
                                    format!("session-messages-{}", uuid::Uuid::new_v4()),
                                    CoreRequest::SessionMessagesLoad { session_id: sid },
                                );
                                match client.request(request).await {
                                    Ok(CoreResponse::SessionMessages { messages, .. }) => {
                                        crate::protocol_conversions::dtos_to_messages(messages)
                                    }
                                    _ => Vec::new(),
                                }
                            } else if let (Some(sid), Some(store)) = (session_id, message_store) {
                                store.list(&sid).await.unwrap_or_default()
                            } else {
                                Vec::new()
                            };
                            if !messages.is_empty() {
                                if let Some(ref mem) = memory_store {
                                    mem.consolidate_session(&messages, &project_hash);
                                    tracing::info!(
                                        "Auto-consolidated session {} memories",
                                        messages.len()
                                    );
                                }
                            }
                        },
                    );
                }

                if let Some(ref notif_mgr) = app.notification_manager {
                    let notif_type =
                        crate::tui::components::notification::NotificationType::Success;
                    let body = format!("Agent finished: {}", stop_reason);
                    let mgr = notif_mgr.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                            tracing::warn!("Failed to send notification: {}", e);
                        }
                    });
                }

                app.messages_state.messages.finalize_streaming();

                let tts = app.ui_state.tts.clone();
                if tts.is_speaking()
                    && !matches!(
                        app.ui_state.mode,
                        crate::tui::app::state::AppMode::RemoteCore { .. }
                    )
                {
                    tokio::spawn(async move {
                        if let Err(e) = tts.stop().await {
                            tracing::debug!("TTS stop error: {}", e);
                        }
                    });
                }
            } else if matches!(app.session_state.session_status, SessionStatus::Working) {
                app.status_bar
                    .set_thinking(true, Some("Thinking...".to_string()));
            }
            true
        }
        AppEvent::PermissionPending {
            perm_id,
            tool,
            path,
            args,
            ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                tool = %tool,
                ?path,
                "PermissionPending"
            );
            app.show_permission_dialog(perm_id, PermissionRequest { tool, path, args });
            true
        }
        AppEvent::QuestionPending {
            session_id,
            questions,
            ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                session_id = %session_id,
                "QuestionPending"
            );
            if let Ok(questions_vec) = serde_json::from_str::<
                Vec<crate::tui::components::dialogs::question::QuestionSpec>,
            >(&questions)
            {
                app.show_question_dialog(questions_vec, session_id);
            }
            true
        }
        AppEvent::FileChanged {
            path,
            action,
            old_content,
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                path = %path,
                action = %action,
                "FileChanged"
            );
            let path_buf = std::path::PathBuf::from(&path);

            let generation = if let Some(existing) = app
                .session_state
                .changed_files
                .iter_mut()
                .find(|file| file.path == path_buf)
            {
                let new_gen = existing.diff_state.generation().saturating_add(1);
                existing.action = action.clone();
                existing.diff_preview = Vec::new();
                existing.diff_state = crate::tui::app::state::session::DiffStatsState::Pending {
                    generation: new_gen,
                };
                new_gen
            } else {
                app.session_state.changed_files.push(
                    crate::tui::app::state::session::ChangedFile {
                        path: path_buf.clone(),
                        action: action.clone(),
                        diff_preview: Vec::new(),
                        diff_state: crate::tui::app::state::session::DiffStatsState::Pending {
                            generation: 0,
                        },
                    },
                );
                0
            };

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

            crate::tui::file_diff::spawn_sidebar_diff_stats(
                app.tui_cmd_tx.clone(),
                app.session_state.project_dir.clone(),
                path,
                old_content,
                generation,
                Some(&mut app.task_registry),
            );

            true
        }
        AppEvent::Error { message } => {
            tracing::debug!(
                target: "codegg::tui::events",
                message = %message,
                "Error received"
            );
            tracing::error!("Agent error: {}", message);
            app.session_state.session_status = SessionStatus::Error;
            app.status_bar.set_thinking(false, None);
            app.streaming_active = false;
            app.messages_state.toasts.add(Toast::error(&message));
            true
        }
        AppEvent::CompactionTriggered {
            tokens_before,
            tokens_after,
            ..
        } => {
            tracing::debug!(target: "codegg::tui::events", "CompactionTriggered");
            let compact_count = app.session_state.compaction_count + 1;
            app.session_state.compaction_count = compact_count;
            let before_str = if tokens_before > 0 {
                format!("~{}k", tokens_before / 1000)
            } else {
                "unknown".to_string()
            };
            let after_str = if tokens_after > 0 {
                format!("~{}k", tokens_after / 1000)
            } else {
                "unknown".to_string()
            };
            let toast = Toast::info(&format!("Compacted: {} → {} tokens", before_str, after_str));
            app.messages_state.toasts.add(toast);
            true
        }
        AppEvent::ModelChanged { model, complexity } => {
            tracing::debug!(
                target: "codegg::tui::events",
                model = %model,
                complexity = %complexity,
                "ModelChanged"
            );
            let short = model.split('/').next_back().unwrap_or(&model);
            app.messages_state
                .toasts
                .info(&format!("Routed: {} ({})", short, complexity));
            true
        }
        AppEvent::SubagentStarted {
            agent, description, ..
        } => {
            tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentStarted");
            app.messages_state.toasts.add(Toast::info(&format!(
                "Subagent '{}' started: {}",
                agent, description
            )));
            true
        }
        AppEvent::SubagentProgress { agent, message, .. } => {
            tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentProgress");
            app.messages_state
                .toasts
                .add(Toast::info(&format!("[{}] {}", agent, message)));
            true
        }
        AppEvent::SubagentCompleted {
            agent,
            result_summary: _,
            ..
        } => {
            tracing::debug!(target: "codegg::tui::events", agent = %agent, "SubagentCompleted");
            app.messages_state
                .toasts
                .add(Toast::success(&format!("Subagent '{}' completed", agent)));
            if let Some(ref notif_mgr) = app.notification_manager {
                let notif_type = crate::tui::components::notification::NotificationType::Success;
                let body = format!("Subagent '{}' completed", agent);
                let mgr = notif_mgr.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                        tracing::warn!("Failed to send notification: {}", e);
                    }
                });
            }
            true
        }
        AppEvent::SubagentFailed { agent, error, .. } => {
            tracing::debug!(
                target: "codegg::tui::events",
                agent = %agent,
                error = %error,
                "SubagentFailed"
            );
            app.messages_state.toasts.add(Toast::error(&format!(
                "Subagent '{}' failed: {}",
                agent, error
            )));
            if let Some(ref notif_mgr) = app.notification_manager {
                let notif_type = crate::tui::components::notification::NotificationType::Error;
                let body = format!("Subagent '{}' failed: {}", agent, error);
                let mgr = notif_mgr.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = mgr.blocking_send_with_config(notif_type, &body) {
                        tracing::warn!("Failed to send notification: {}", e);
                    }
                });
            }
            true
        }
        AppEvent::ContextUpdated {
            context_tokens,
            context_limit,
            ..
        } => {
            tracing::debug!(
                target: "codegg::tui::events",
                context_tokens,
                context_limit,
                "ContextUpdated"
            );
            app.session_state.context_tokens = context_tokens;
            app.session_state.context_limit = context_limit;
            true
        }
        AppEvent::TodoUpdated {
            session_id: event_session,
            items,
            ..
        } => {
            if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                if event_session == active_id {
                    let entries: Vec<crate::tui::app::TodoEntry> = items
                        .iter()
                        .map(|item| crate::tui::app::TodoEntry {
                            content: item.content.clone(),
                            status: item.status.clone(),
                            priority: item.priority.clone(),
                        })
                        .collect();
                    app.set_todos(entries);
                }
            }
            true
        }
        AppEvent::GoalUpdated {
            session_id: event_session,
            goal,
        } => {
            if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                if event_session == active_id {
                    if let Some(snap) = *goal {
                        app.set_active_goal(Some(snap));
                    } else {
                        app.set_active_goal(None);
                    }
                }
            }
            true
        }
        AppEvent::GoalUsageUpdated {
            session_id: event_session,
            goal_id,
            usage,
            budget,
        } => {
            if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                if event_session == active_id {
                    if let Some(ref mut active) = app.active_goal {
                        if active.id == goal_id {
                            active.usage = usage.clone();
                            active.budget = budget.clone();
                        }
                    }
                }
            }
            true
        }
        AppEvent::GoalBudgetLimited {
            session_id: event_session,
            goal_id,
            reason,
        } => {
            if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                if event_session == active_id {
                    if let Some(ref mut active) = app.active_goal {
                        if active.id == goal_id {
                            active.status = "budget_limited".to_string();
                        }
                    }
                    app.messages_state
                        .toasts
                        .warning(&format!("Goal budget limited: {}", reason));
                }
            }
            true
        }
        AppEvent::GoalCompleted {
            session_id: event_session,
            goal_id,
            evidence,
        } => {
            if let Some(active_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) {
                if event_session == active_id {
                    if let Some(ref mut active) = app.active_goal {
                        if active.id == goal_id {
                            active.status = "complete".to_string();
                        }
                    }
                    app.messages_state
                        .toasts
                        .info(&format!("Goal completed: {}", evidence));
                }
            }
            true
        }
        _ => {
            tracing::debug!(target: "codegg::tui::events", "unhandled bus event");
            false
        }
    }
}
