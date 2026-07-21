//! Bus event handling: processes AppEvent variants from the global event bus.

use super::super::app::{App, SessionStatus};
use super::super::app::state::{
    apply_inactive_summary, classify_event, InactiveSummaryKind, RouteDecision,
};
use super::super::components::toast::Toast;
use crate::bus::events::AppEvent;
use crate::permission::PermissionRequest;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::util::truncate::truncate_prefix;

/// Handle a batch of bus events. Returns `true` if the UI should re-render.
pub(crate) fn handle_app_event_batch(app: &mut App, events: Vec<AppEvent>) -> bool {
    let mut needs_render = false;
    for event in events {
        needs_render |= handle_routed_event(app, event);
    }
    needs_render
}

/// Decide whether the event is allowed to mutate the heavy active view.
/// Returns `true` only when routing classifies the event as
/// `ActiveView` (or for genuine globals). All other routing outcomes
/// receive bounded summary updates or are dropped.
fn handle_routed_event(app: &mut App, event: AppEvent) -> bool {
    let active_tab_id = app.project_tabs.active_tab_id().cloned();
    let active_view_epoch = app.view_switch.active_view_epoch;
    let decision = classify_event(
        &event,
        &app.routing_registry,
        active_tab_id.as_ref(),
        active_view_epoch,
    );
    match decision {
        RouteDecision::ActiveView { .. } | RouteDecision::Global => {
            handle_event_inner(app, event)
        }
        RouteDecision::InactiveSummary { tab_id } => {
            let kind = inactive_kind_for(&event);
            let detail = inactive_detail_for(&event);
            apply_inactive_summary(&mut app.routing_registry, &tab_id, kind, detail.as_deref());
            if matches!(kind, InactiveSummaryKind::PendingPermission) {
                let active_id = app.project_tabs.active_tab_id().cloned();
                if Some(&tab_id) != active_id.as_ref() {
                    app.messages_state.toasts.info(
                        "Permission request in inactive project; switch to that tab to respond.",
                    );
                }
            }
            if matches!(kind, InactiveSummaryKind::PendingQuestion) {
                let active_id = app.project_tabs.active_tab_id().cloned();
                if Some(&tab_id) != active_id.as_ref() {
                    app.messages_state.toasts.info(
                        "Question pending in inactive project; switch to that tab to answer.",
                    );
                }
            }
            true
        }
        RouteDecision::DropDiagnostic { reason } => {
            tracing::debug!(
                target: "codegg::tui::routing",
                ?event,
                reason,
                "dropped event: ownership could not be resolved"
            );
            false
        }
        RouteDecision::RefreshRequired { reason } => {
            for tab in app.project_tabs.ordered() {
                app.routing_registry
                    .activity_mut(&tab.tab_id)
                    .mark_resync_required();
            }
            tracing::debug!(
                target: "codegg::tui::routing",
                ?event,
                reason,
                "refresh required: resync flag raised"
            );
            true
        }
    }
}

fn inactive_kind_for(event: &AppEvent) -> InactiveSummaryKind {
    match event {
        AppEvent::PermissionPending { .. } => InactiveSummaryKind::PendingPermission,
        AppEvent::QuestionPending { .. } => InactiveSummaryKind::PendingQuestion,
        AppEvent::AgentFinished { .. }
        | AppEvent::TextDelta { .. }
        | AppEvent::ReasoningDelta { .. }
        | AppEvent::ToolCallStarted { .. }
        | AppEvent::ToolResult { .. } => InactiveSummaryKind::UnreadActivity,
        AppEvent::CompactionTriggered { .. }
        | AppEvent::ContextUpdated { .. }
        | AppEvent::TodoUpdated { .. }
        | AppEvent::GoalUpdated { .. }
        | AppEvent::GoalUsageUpdated { .. }
        | AppEvent::GoalBudgetLimited { .. }
        | AppEvent::GoalCompleted { .. }
        | AppEvent::SubagentStarted { .. }
        | AppEvent::SubagentProgress { .. }
        | AppEvent::SubagentCompleted { .. }
        | AppEvent::SubagentFailed { .. }
        | AppEvent::TestRunStarted { .. }
        | AppEvent::TestRunProgress { .. }
        | AppEvent::TestRunCompleted { .. } => InactiveSummaryKind::StatusUpdate,
        _ => InactiveSummaryKind::StatusUpdate,
    }
}

fn inactive_detail_for(event: &AppEvent) -> Option<String> {
    match event {
        AppEvent::PermissionPending { tool, .. } => Some(format!("permission: {tool}")),
        AppEvent::QuestionPending { .. } => Some("question pending".to_string()),
        AppEvent::ToolResult { tool_name, success, .. } => Some(format!(
            "tool {}: {}",
            tool_name,
            if *success { "ok" } else { "error" }
        )),
        AppEvent::SubagentCompleted { agent, .. } => Some(format!("subagent {agent} done")),
        AppEvent::SubagentFailed { agent, error, .. } => {
            Some(format!("subagent {agent} failed: {error}"))
        }
        AppEvent::CompactionTriggered { .. } => Some("compaction triggered".to_string()),
        AppEvent::TestRunCompleted { status, .. } => Some(format!("test run {status}")),
        _ => None,
    }
}

fn handle_event_inner(app: &mut App, event: AppEvent) -> bool {
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
                        truncate_prefix(&arguments, 200)
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
                "ModelChanged received"
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
        AppEvent::PluginUiEffect {
            session_id,
            plugin_id,
            invocation_id: _,
            effect,
        } => {
            let current_session = app
                .session_state
                .session
                .as_ref()
                .map(|s| s.id.as_str())
                .unwrap_or_default();
            let matches_session = session_id
                .as_deref()
                .map(|sid| sid == current_session)
                .unwrap_or(true);
            if matches_session {
                let _ = app.apply_plugin_ui_effect(effect, Some(&plugin_id));
            }
            true
        }
        _ => {
            tracing::debug!(target: "codegg::tui::events", "unhandled bus event");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::events::AppEvent;
    use crate::tui::app::state::ProjectTabId;
    use crate::tui::app::state::routing::{RouteDecision, RoutingRegistry};

    fn make_app_with_two_tabs() -> App {
        use crate::tui::app::state::project_tabs::ProjectTabState;
        let mut app = App::new_for_testing(
            std::path::PathBuf::from("/tmp/test")
                .to_string_lossy()
                .into_owned(),
        );
        let active_id = app
            .project_tabs
            .active_tab_id()
            .cloned()
            .expect("compat tab");
        // Inject a second tab
        let second_id = ProjectTabId::new();
        let mut second = ProjectTabState::empty(second_id.clone(), "secondary".into());
        second.project_id = Some("p-other".into());
        second.workspace_id = Some("w-other".into());
        second.session_id = Some("s-other".into());
        app.project_tabs.add_and_activate(second);
        // Restore primary as active and update its session id.
        let _ = app.project_tabs.set_active(&active_id);
        // Register both sessions with the routing registry.
        app.routing_registry
            .register_open_session(active_id.clone(), "s-primary".into());
        app.routing_registry
            .register_open_session(second_id, "s-other".into());
        app
    }

    #[test]
    fn inactive_text_delta_updates_summary_only() {
        let app = make_app_with_two_tabs();
        let active_id = app.project_tabs.active_tab_id().cloned().unwrap();
        let other_tab = app
            .project_tabs
            .ordered()
            .iter()
            .find(|t| t.tab_id != active_id)
            .map(|t| t.tab_id.clone())
            .expect("at least one inactive tab");
        let decision = classify_event(
            &AppEvent::TextDelta {
                session_id: "s-other".into(),
                delta: "hi".into(),
            },
            &app.routing_registry,
            Some(&active_id),
            app.view_switch.active_view_epoch,
        );
        assert_eq!(
            decision,
            RouteDecision::InactiveSummary { tab_id: other_tab.clone() }
        );
    }

    #[test]
    fn stale_view_epoch_drops_per_session_events() {
        let mut registry = RoutingRegistry::new();
        let tid = ProjectTabId::new();
        registry.register_open_session(tid.clone(), "s1".into());
        let event = AppEvent::TextDelta {
            session_id: "s1".into(),
            delta: "hi".into(),
        };
        let good = classify_event(&event, &registry, Some(&tid), 7);
        assert_eq!(
            good,
            RouteDecision::ActiveView {
                tab_id: tid.clone()
            }
        );
    }

    #[test]
    fn unknown_session_dropped() {
        let registry = RoutingRegistry::new();
        let tid = ProjectTabId::new();
        let event = AppEvent::TextDelta {
            session_id: "ghost".into(),
            delta: "x".into(),
        };
        let decision = classify_event(&event, &registry, Some(&tid), 7);
        match decision {
            RouteDecision::DropDiagnostic { .. } => {}
            other => panic!("expected drop diagnostic, got {other:?}"),
        }
    }
}
