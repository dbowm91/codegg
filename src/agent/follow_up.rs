//! Follow-up prompt draining and pending-notification injection.
//!
//! Physical decomposition of [`super::r#loop`] (M003): notification recovery
//! injection at safe turn boundaries and the non-blocking follow-up drain
//! that powers goal-continuation turns. Queue ownership and recovery
//! authority stay with the notification service and run control.

use super::context_runtime::ContextPackObservationPhase;
use super::loop_output::redact_local_paths;
use super::r#loop::AgentLoop;
use super::tool_inspect::{is_soft_stop_reason, tool_outcome_is_success};
use crate::agent::coordinator::TurnPhase;
use crate::agent::processor::EventProcessor;
use crate::agent::progress_recovery::AutonomyState;
use crate::bus::events::AppEvent;
use crate::provider::text_tool_parser::repair_text_as_tool_calls;
use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message};
use crate::tool::plan::detect_plan_mode_change;
use tokio::sync::mpsc;

impl AgentLoop {
    /// Check for pending background tool program notifications and
    /// inject them as system messages. Called at safe turn boundaries
    /// (start of each run).
    ///
    /// Classifies notifications into three categories as required by
    /// the plan: completed, incomplete-recoverable, and failed-terminal.
    /// Recovery for cross-process crashes is delegated to
    /// [`crate::agent::tool_program_recovery::inject_recoverable_notifications`].
    pub(super) async fn inject_pending_notifications(&self, messages: &mut Vec<Message>) {
        let Some(ref svc) = self.services.notification_service else {
            return;
        };
        let report = crate::agent::tool_program_recovery::inject_recoverable_notifications(
            self.services.event_store.as_deref(),
            svc,
            &self.session_id,
            |text| {
                messages.push(Message::System {
                    content: std::sync::Arc::new(text),
                });
            },
        )
        .await;
        for error in &report.errors {
            tracing::error!(%error, "Tool Program notification recovery error");
        }
    }

    /// Drains queued follow-up prompts, if any are already queued.
    ///
    /// Uses non-blocking `try_recv()` - does NOT wait if no follow-up is queued.
    /// This means late-arriving follow-ups (after `run()` returns) are NOT processed
    /// by the same `run()` call; they require a new `run()` invocation.
    pub(super) async fn drain_follow_up(
        &mut self,
        request: &mut ChatRequest,
        all_events: &mut Vec<ChatEvent>,
        processor: &mut EventProcessor,
    ) {
        let model_profile = crate::model_profile::ModelProfileResolver::new(&self.services.config)
            .resolve(&request.model);
        loop {
            // Check if a follow-up is already queued without blocking
            let prompt = match self.follow_up_rx.try_recv() {
                Ok(prompt) => {
                    tracing::info!("Processing follow-up: {}", prompt);
                    prompt
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No follow-up queued, return immediately without blocking
                    tracing::debug!("No follow-up queued, skipping drain");
                    return;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    tracing::info!("Follow-up channel disconnected");
                    return;
                }
            };

            request.messages.push(Message::User {
                content: vec![ContentPart::Text {
                    text: prompt.into(),
                }],
            });

            // Continue processing until done (handles tool calls and follow-up responses)
            let mut autonomy = AutonomyState::default();
            let mut just_executed_tools = false;
            loop {
                self.lifecycle.set_phase(TurnPhase::ContextPreparation);
                self.compact_if_needed(&mut request.messages, &model_profile)
                    .await;
                // Phase 5: observe in follow-up loop after compaction and before provider call.
                self.observe_context_pack(
                    request,
                    &model_profile,
                    ContextPackObservationPhase::AfterCompaction,
                );
                self.apply_tool_palette_policy_if_active(request, "BeforeProviderCall");
                self.observe_or_apply_volatile_tail_policy(request, "BeforeProviderCall");
                self.observe_context_pack(
                    request,
                    &model_profile,
                    ContextPackObservationPhase::BeforeProviderCall,
                );
                self.lifecycle.set_phase(TurnPhase::ProviderInvocation);
                let events =
                    match crate::agent::provider_turn::ProviderTurnAdapter::receive(self, request)
                        .await
                    {
                        Ok(events) => events,
                        Err(e) => {
                            tracing::error!("Follow-up stream error: {}", e);
                            return;
                        }
                    };

                for event in &events {
                    processor.process(event);
                }
                all_events.extend(events);

                let mut tool_calls = processor.tool_calls().to_vec();
                if tool_calls.is_empty() {
                    if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                        let preview: String = processor.text().chars().take(200).collect();
                        tracing::info!(
                            "tool-parse-fallback(followup): tool_calls=0, stop_reason={:?}, text_len={}, text_preview={:?}",
                            processor.stop_reason(),
                            processor.text().len(),
                            preview
                        );
                    }
                    let adapter =
                        crate::model_profile::ModelProfileResolver::new(&self.services.config)
                            .resolve_adapter(None, &request.model);
                    if let Some(profile) = adapter.text_tool_repair.as_deref() {
                        if !autonomy.adapter_repair_allowed() {
                            // Repair budget exhausted: M002 permits at most one
                            // bounded textual adapter repair.
                        } else {
                            match repair_text_as_tool_calls(
                                profile,
                                processor.text(),
                                processor.stop_reason(),
                                request.tools.as_deref().unwrap_or(&[]),
                            ) {
                                Ok(Some(parsed_calls)) => {
                                    for tc in &parsed_calls {
                                        crate::bus::global::GlobalEventBus::publish(
                                            AppEvent::ToolCallStarted {
                                                session_id: self.session_id.clone(),
                                                tool_name: tc.name.to_string(),
                                                tool_id: tc.id.to_string(),
                                                arguments: tc.arguments.to_string(),
                                            },
                                        );
                                    }
                                    tool_calls = parsed_calls;
                                }
                                Ok(None) => {}
                                Err(error) => tracing::warn!(
                                    adapter = %adapter.adapter_id,
                                    profile,
                                    ?error,
                                    "textual tool-call repair rejected provider response"
                                ),
                            }
                        }
                    }
                }

                if tool_calls.is_empty() {
                    if just_executed_tools
                        && is_soft_stop_reason(processor.stop_reason())
                        && autonomy.continuation_allowed()
                    {
                        if let Some(msg) = processor.to_assistant_message() {
                            request.messages.push(msg);
                        }
                        just_executed_tools = false;
                        processor.reset();
                        continue;
                    }
                    if matches!(processor.stop_reason(), Some("tool_calls")) {
                        let raw_text = processor.text().to_string();
                        let preview = if raw_text.len() > 600 {
                            format!("{}…", crate::util::truncate_prefix(&raw_text, 600))
                        } else {
                            raw_text
                        };
                        let preview = if preview.is_empty() {
                            "<empty stream>".to_string()
                        } else {
                            preview
                        };
                        tracing::warn!(
                            "Model returned stop_reason=tool_calls without parseable structured tool calls after retries; raw_text={}",
                            preview
                        );
                        crate::bus::global::GlobalEventBus::publish(AppEvent::Error {
                        message: format!(
                            "Model returned stop_reason=tool_calls without parseable structured tool calls after retries. Raw text: {}",
                            preview
                        ),
                    });
                    }
                    processor.reset();
                    break;
                }
                self.observe_tool_palette_starvation(&tool_calls);
                self.lifecycle.set_phase(TurnPhase::ToolExecution);
                let tool_results = match crate::agent::tool_batch::ToolBatchExecutor::new(self)
                    .execute(&tool_calls)
                    .await
                {
                    Ok(results) => results,
                    Err(e) => {
                        tracing::error!("Tool execution error: {}", e);
                        processor.reset();
                        return;
                    }
                };
                self.lifecycle.set_phase(TurnPhase::Recovery);
                just_executed_tools = !tool_results.is_empty();
                self.record_habit_tool_results(&tool_calls, &tool_results);

                // Push assistant message BEFORE tool results (fix Packet 2)
                if let Some(msg) = processor.to_assistant_message() {
                    request.messages.push(msg);
                }

                for (id, outcome) in &tool_results {
                    let tool_name = tool_calls
                        .iter()
                        .find(|tc| *tc.id == id.as_str())
                        .map(|tc| tc.name.to_string())
                        .unwrap_or_default();
                    let success = tool_outcome_is_success(outcome);
                    let redacted_output =
                        redact_local_paths(&outcome.model_text, &self.local_paths);
                    crate::bus::global::GlobalEventBus::publish(AppEvent::ToolResult {
                        tool_id: id.clone(),
                        tool_name,
                        session_id: self.session_id.clone(),
                        output: redacted_output,
                        success,
                    });
                }

                for (id, outcome) in &tool_results {
                    let content = &outcome.model_text;
                    if let Some(change) = detect_plan_mode_change(content) {
                        match change {
                            crate::tool::plan::PlanModeChange::Enter(topic) => {
                                self.enter_plan_mode(topic);
                                tracing::info!("Plan mode entered");
                            }
                            crate::tool::plan::PlanModeChange::Exit => {
                                self.exit_plan_mode();
                                tracing::info!("Plan mode exited");
                            }
                        }
                    }

                    let redacted_content = redact_local_paths(content, &self.local_paths);

                    let tool_name_str = tool_calls
                        .iter()
                        .find(|tc| tc.id.as_str() == id.as_str())
                        .map(|tc| tc.name.to_string())
                        .unwrap_or_default();

                    let turn = self.state.turn_count;
                    let handle_result =
                        crate::context::ContextHandle::build_tool(&self.session_id, turn, id);
                    let effective_handle = if self.services.projection_config.artifact_store_enabled
                    {
                        match handle_result {
                            Ok(ref handle) => {
                                let store_result = self
                                    .services
                                    .artifact_store
                                    .put(crate::context::ContextArtifact {
                                        handle: handle.clone(),
                                        session_id: self.session_id.clone(),
                                        turn_index: turn,
                                        tool_call_id: Some(id.clone()),
                                        tool_name: Some(tool_name_str.clone()),
                                        kind: crate::context::ArtifactKind::ToolResult,
                                        created_at_ms: chrono::Utc::now().timestamp_millis(),
                                        content_hash: crate::context::compute_content_hash(
                                            &redacted_content,
                                        ),
                                        redacted_content: redacted_content.clone(),
                                        raw_bytes_len: redacted_content.len(),
                                        estimated_tokens: crate::context::estimate_tokens(
                                            &redacted_content,
                                        ),
                                    })
                                    .await;
                                match store_result {
                                    Ok(()) => handle.as_str(),
                                    Err(err) => {
                                        tracing::warn!(
                                            tool_call_id = %id,
                                            tool_name = %tool_name_str,
                                            session_id = %self.session_id,
                                            error = %err,
                                            "failed to store context artifact; omitting recovery handle"
                                        );
                                        ""
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::warn!(
                                    tool_call_id = %id,
                                    tool_name = %tool_name_str,
                                    session_id = %self.session_id,
                                    error = %err,
                                    "failed to build context handle; omitting recovery handle"
                                );
                                ""
                            }
                        }
                    } else {
                        ""
                    };

                    let tool_args = tool_calls
                        .iter()
                        .find(|tc| tc.id.as_str() == id.as_str())
                        .map(|tc| tc.arguments.to_string());

                    let proj = crate::context::project_tool_output(
                        &tool_name_str,
                        tool_args.as_deref(),
                        &redacted_content,
                        tool_outcome_is_success(outcome),
                        effective_handle,
                        &self.services.projection_config,
                    );

                    self.context_ledger
                        .record_projection(&proj, effective_handle);

                    let msg = Message::Tool {
                        tool_call_id: id.clone().into(),
                        content: proj.model_text.into(),
                    };
                    request.messages.push(msg);
                }

                processor.reset();
            }
        }
    }
}
