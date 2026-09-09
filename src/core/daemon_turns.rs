//! M002: `turns` request family for `CoreDaemon`.
//!
//! Turn submit/cancel/steer, agent/model selection writes, permission/question responses, and transport lifecycle.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use std::sync::Arc;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;
use super::event_log::EventFilter;

impl CoreDaemon {
    pub(crate) async fn handle_turns_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
            CoreRequest::TurnSubmit {
                session_id,
                model,
                agents,
                current_agent_idx,
                messages,
                plan_mode,
                ..
            } => {
                if current_agent_idx >= agents.len() {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Invalid agent index {} for {} agents",
                                current_agent_idx,
                                agents.len()
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "invalid_agent_index".to_string(),
                        message: "Invalid agent index".to_string(),
                    });
                }
                // A durable session selection is authoritative for new
                // turns. Never silently route around a selected connection
                // that has entered a non-active lifecycle state.
                if let Some(selection_service) = self.selection_service.as_ref() {
                    if let Ok(crate::protocol::provider::SessionSelectionDto::Selected {
                        connection,
                        ..
                    }) = selection_service.get(&session_id).await
                    {
                        if connection.state != "active" {
                            return Ok(CoreResponse::Error {
                                code: "connection_state".to_string(),
                                message: format!(
                                    "selected provider connection {} is {}",
                                    connection.id, connection.state
                                ),
                            });
                        }
                    }
                }
                // Validate the provider exists before delegating to the turn
                // runtime. This preserves the existing `provider_not_found`
                // response shape from the daemon layer. The turn runtime
                // also validates provider existence internally, so this is
                // intentionally duplicated for backward-compatible error handling.
                let mut registry = crate::provider::ProviderRegistry::new();
                let config = super::load_config_or_default();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let provider_name = model.split('/').next().unwrap_or("openai").to_string();
                let _model_name = model.split('/').next_back().unwrap_or(&model).to_string();
                let Some(_base_provider) = registry.get(&provider_name) else {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Provider '{}' not found. Please check your configuration.",
                                provider_name
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "provider_not_found".to_string(),
                        message: format!("Provider not found: {}", provider_name),
                    });
                };

                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };

                // Session-open and manual refreshes converge here as the
                // final correctness gate: the turn captures the currently
                // published immutable generation before runtime assembly.
                let asset_refresh = match self
                    .refresh_runtime_assets(
                        &runtime,
                        &session_id,
                        crate::agent::asset_refresh::RefreshReason::SessionLifecycle,
                    )
                    .await
                {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "turn_asset_refresh_failed".to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                if let Some(error) = Self::refresh_report_error(&asset_refresh) {
                    return Ok(CoreResponse::Error {
                        code: "turn_asset_refresh_failed".to_string(),
                        message: error.to_string(),
                    });
                }
                let asset_scope = crate::agent::asset_refresh::AssetScope::new(
                    &runtime.project_id,
                    runtime.workspace_id.as_str(),
                );
                let asset_snapshot =
                    self.asset_refresh
                        .snapshot(&asset_scope)
                        .await
                        .map(|published| {
                            (
                                published.snapshot.clone(),
                                std::sync::Arc::new(std::sync::Mutex::new(
                                    published.runtime_asset_pin(),
                                )),
                            )
                        });
                let (asset_snapshot, asset_pin) = asset_snapshot
                    .map(|(snapshot, pin)| (Some(snapshot), Some(pin)))
                    .unwrap_or((None, None));

                let turn_id = {
                    let mut active = runtime.active_turn.write().await;
                    if active.is_some() {
                        return Ok(CoreResponse::Error {
                            code: "turn_already_active".to_string(),
                            message: "A turn is already active for this session".to_string(),
                        });
                    }
                    let turn_id = format!("turn-{}", uuid::Uuid::new_v4());
                    *active = Some(crate::core::session_runtime::TurnHandle {
                        turn_id: turn_id.clone(),
                        cancel_tx: tokio::sync::watch::channel(false).0,
                        steer_tx: None,
                        started_at: chrono::Utc::now(),
                        asset_pin: asset_pin.clone(),
                    });
                    turn_id
                };

                {
                    let mut status = runtime.status.write().await;
                    *status = crate::core::session_runtime::RuntimeSessionStatus::Running;
                }

                // Emit TurnStarted immediately so subscribers (and the bridge
                // fallback) see a coherent turn identity from the first event.
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnStarted {
                            session_id: session_id.clone(),
                            turn_id: turn_id.clone(),
                        },
                    )
                    .await;

                // M003: capture originating-principal attribution for the turn.
                self.record_origin_with_decision(
                    &authority,
                    &authz_decision,
                    "turn",
                    turn_id.as_str(),
                )
                .await;

                // Presence M001: meaningful agent activity. Best-effort
                // only; turn correctness never depends on presence.
                self.note_presence_activity(
                    &authority,
                    trusted_client_id,
                    authz_decision.project_id.clone(),
                    Some(session_id.as_str()),
                    codegg_core::presence::PresenceActivity::AgentRunning,
                );

                // Build an immutable execution context from the bound
                // runtime's workspace identity. The context flows through
                // every daemon-owned execution path inside the turn.
                let workspace_record = codegg_core::workspace::WorkspaceRecord {
                    id: runtime.workspace_id.clone(),
                    canonical_root: runtime.workspace_root.clone(),
                    display_name: runtime
                        .workspace_root
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| runtime.workspace_root.to_string_lossy().into_owned()),
                    created_at: chrono::Utc::now(),
                    last_opened_at: chrono::Utc::now(),
                    archived_at: None,
                };
                let execution = codegg_core::workspace::ExecutionContext::new(
                    Arc::new(workspace_record),
                    Some(session_id.clone()),
                    Default::default(),
                );

                // Delegate to the injected turn runtime which handles tool
                // registry, agent loop construction, and background spawning.
                let plugin_service = match crate::plugin::create_default_plugin_service().await {
                    Some(service) => match service
                        .for_workspace(runtime.workspace_id.to_string())
                        .await
                    {
                        Ok(contextual) => Some(Arc::new(contextual)),
                        Err(error) => {
                            tracing::error!(%error, "failed to resolve plugin activation for workspace");
                            None
                        }
                    },
                    None => None,
                };
                // Retain the daemon-owned workspace service for the detached
                // turn. This keeps its canonical lock table alive until the
                // loop exits, so checkpoint capture and native mutation share
                // one inter-session repository authority.
                let workspace_service_lease = self
                    .workspace_services
                    .acquire(&runtime.workspace_id)
                    .await
                    .map_err(|error| AppError::Other(anyhow::anyhow!(error.to_string())))?;
                let turn_input = crate::agent::turn_runtime::TurnRunInput {
                    session_id: session_id.clone(),
                    agents_dto: agents,
                    current_agent_idx,
                    model,
                    messages_dto: messages,
                    plan_mode,
                    config,
                    pool: self.pool.clone(),
                    subagent_pool: self.deps.legacy_agent.subagent_pool.clone(),
                    memory_store: self.deps.memory_store.clone(),
                    event_log: Arc::clone(&self.event_log),
                    turn_id: turn_id.clone(),
                    lsp_service: self.deps.lsp_service.clone(),
                    lsp_context_input: None,
                    plugin_service,
                    execution,
                    submission: self.deps.submission.clone(),
                    workspace_service_lease: Some(workspace_service_lease),
                    agent_run_store: self.deps.agent_run_store.clone(),
                    convergence_store: self.deps.convergence_store.clone(),
                    run_control: self.deps.run_control.clone(),
                    run_group_service: self.deps.run_group_service.clone(),
                    project_id: codegg_core::identity::ProjectId::parse(&runtime.project_id).ok(),
                    repository_id: None,
                    asset_snapshot,
                    asset_pin,
                };
                let turn_output = self.deps.turn_runtime.run_turn(turn_input).await?;

                // Update the TurnHandle with the runtime's cancel/steer channels.
                {
                    let mut active = runtime.active_turn.write().await;
                    if let Some(handle) = active.as_mut() {
                        handle.cancel_tx = turn_output.cancel_tx;
                        handle.steer_tx = Some(turn_output.steer_tx);
                    }
                }

                Ok(CoreResponse::Ack)
            }
            CoreRequest::PermissionRespond { id, choice } => {
                let parsed = match choice.as_str() {
                    "allow" => crate::bus::PermissionDecision::AllowOnce,
                    "always_allow" => crate::bus::PermissionDecision::AlwaysAllow,
                    "deny" => crate::bus::PermissionDecision::DenyOnce,
                    "always_deny" => crate::bus::PermissionDecision::AlwaysDeny,
                    _ => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_choice".to_string(),
                            message: format!("Invalid permission choice: {}", choice),
                        });
                    }
                };
                // Extract session_id and simple perm_id from protocol ID: perm:{session_id}:{turn_id}:{perm_id}.
                // Reject malformed IDs explicitly rather than silently using empty defaults
                // (which could route a response to the wrong session).
                let (session_id, simple_perm_id) = match id.strip_prefix("perm:").and_then(|rest| {
                    let mut parts = rest.splitn(3, ':');
                    let sid = parts.next()?.to_string();
                    let _turn_id = parts.next()?;
                    let pid = parts.next()?.to_string();
                    Some((sid, pid))
                }) {
                    Some(parsed) => parsed,
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_id".to_string(),
                            message: format!(
                                "Permission ID '{}' is not in perm:<session_id>:<turn_id>:<perm_id> format",
                                id
                            ),
                        });
                    }
                };
                let sent = crate::bus::PermissionRegistry::respond_scoped(
                    &session_id,
                    &simple_perm_id,
                    parsed,
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_permissions.remove(&id);
                    }
                    // Emit PermissionResponded event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::PermissionResponded {
                            session_id,
                            tool: String::new(),
                            allowed: parsed.allowed(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "permission_response_failed".to_string(),
                        message: "No pending permission request found".to_string(),
                    })
                }
            }
            CoreRequest::QuestionRespond { id, answers } => {
                // Extract session_id and simple question_id from protocol ID: question:{session_id}:{turn_id}:{question_id}.
                // Reject malformed IDs explicitly.
                let (session_id, simple_question_id) = match id.strip_prefix("question:").and_then(
                    |rest| {
                        let mut parts = rest.splitn(3, ':');
                        let sid = parts.next()?.to_string();
                        let _turn_id = parts.next()?;
                        let qid = parts.next()?.to_string();
                        Some((sid, qid))
                    },
                ) {
                    Some(parsed) => parsed,
                    None => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_question_id".to_string(),
                            message: format!(
                                "Question ID '{}' is not in question:<session_id>:<turn_id>:<question_id> format",
                                id
                            ),
                        });
                    }
                };
                let sent = crate::bus::QuestionRegistry::answer_question_scoped(
                    &session_id,
                    &simple_question_id,
                    answers.to_string(),
                );
                if sent {
                    // Remove from session runtime's pending set
                    if let Some(runtime) = self.sessions.get(&session_id) {
                        runtime.pending_questions.remove(&id);
                    }
                    // Emit QuestionAnswered event
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::QuestionAnswered {
                            session_id,
                            answers: answers.to_string(),
                        },
                    );
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "question_response_failed".to_string(),
                        message: "No pending question found".to_string(),
                    })
                }
            }
            CoreRequest::ModelsRefresh => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let config = super::load_config_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                    std::path::PathBuf::new(),
                )
                .with_pool(pool);
                let models = discovery.refresh(&registry).await;
                let model_ids: Vec<String> = models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "models": model_ids }),
                })
            }
            CoreRequest::Subscribe { session_id } => {
                let current_seq = self.event_log.current_seq();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "current_seq": current_seq,
                        "session_id": session_id,
                    }),
                })
            }
            CoreRequest::Resume {
                session_id,
                from_event_seq,
            } => {
                let filter = EventFilter {
                    session_id: session_id.clone(),
                    client_id: None,
                    include_global: true,
                };

                let current_seq = self.event_log.current_seq();

                // `ResyncRequired` means "the requested sequence is too old
                // to replay from available event storage" -- not "there are
                // no new events". A client that is already caught up
                // (from_event_seq >= current_seq) gets an empty `Events`
                // response so the resume handshake always completes for
                // in-sync clients.
                if !self.event_log.covers_from(from_event_seq).await {
                    return Ok(CoreResponse::ResyncRequired {
                        from_event_seq,
                        current_seq,
                        session_id,
                    });
                }

                // Already caught up (or covered by the ring/DB): return
                // an empty events vector. A future `from_event_seq` (above
                // `current_seq`) also returns empty here rather than
                // erroring; clients treat that as a no-op resume.
                if from_event_seq >= current_seq {
                    return Ok(CoreResponse::Events {
                        events: Vec::new(),
                        current_seq,
                    });
                }

                let events = self.event_log.replay_from(from_event_seq, &filter).await;
                Ok(CoreResponse::Events {
                    events,
                    current_seq,
                })
            }
            CoreRequest::TurnCancel {
                session_id,
                turn_id,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        if handle.cancel_tx.send(true).is_err() {
                            tracing::debug!(turn_id = %turn_id, "turn cancellation receiver already closed");
                        }
                        Ok(CoreResponse::Ack)
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to cancel".to_string(),
                    }),
                }
            }
            CoreRequest::TurnSteer {
                session_id,
                turn_id,
                text,
            } => {
                let Some(runtime) = self.sessions.get(&session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("No runtime for session: {}", session_id),
                    });
                };
                let active = runtime.active_turn.read().await;
                match active.as_ref() {
                    Some(handle) if handle.turn_id == turn_id => {
                        if let Some(ref steer_tx) = handle.steer_tx {
                            match steer_tx.try_send(text) {
                                Ok(()) => Ok(CoreResponse::Ack),
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    tracing::debug!(turn_id = %turn_id, "turn steering receiver already closed");
                                    Ok(CoreResponse::Error {
                                        code: "steer_unavailable".to_string(),
                                        message: "Turn steering receiver is unavailable"
                                            .to_string(),
                                    })
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    Ok(CoreResponse::Error {
                                        code: "steer_queue_full".to_string(),
                                        message: "Turn steering queue is full".to_string(),
                                    })
                                }
                            }
                        } else {
                            Ok(CoreResponse::Error {
                                code: "steer_not_supported".to_string(),
                                message: "Turn does not support steering".to_string(),
                            })
                        }
                    }
                    Some(handle) => Ok(CoreResponse::Error {
                        code: "turn_id_mismatch".to_string(),
                        message: format!(
                            "Requested turn_id '{}' does not match active turn_id '{}'",
                            turn_id, handle.turn_id
                        ),
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "no_active_turn".to_string(),
                        message: "No active turn to steer".to_string(),
                    }),
                }
            }
            CoreRequest::AgentSelect {
                session_id,
                agent_name,
            } => {
                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                {
                    let mut selected = runtime.selected_agent.write().await;
                    *selected = Some(agent_name.clone());
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::SessionUpdated {
                        id: session_id.clone(),
                    },
                );
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ModelSelect { session_id, model } => {
                let runtime = match self.bind_runtime_for_session(&session_id).await {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_unbound".to_string(),
                            message: format!(
                                "session {} has no resolvable workspace: {}",
                                session_id, e
                            ),
                        });
                    }
                };
                {
                    let mut selected = runtime.selected_model.write().await;
                    *selected = Some(model.clone());
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::SessionUpdated {
                        id: session_id.clone(),
                    },
                );
                Ok(CoreResponse::Ack)
            }
            CoreRequest::SnapshotSession { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool.clone());
                let msg_store = crate::session::MessageStore::new(pool);

                let session = match store.get(&session_id).await {
                    Ok(Some(s)) => s,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "session_not_found".to_string(),
                            message: format!("Session not found: {}", session_id),
                        })
                    }
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "session_load_failed".to_string(),
                            message: e.to_string(),
                        })
                    }
                };

                let messages = match msg_store.list(&session_id).await {
                    Ok(messages) => messages,
                    Err(error) => {
                        tracing::warn!(error = %error, session_id = %session_id, "message replay query failed");
                        Vec::new()
                    }
                };

                let (
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                ) = if let Some(runtime) = self.sessions.get(&session_id) {
                    let status = format!("{:?}", *runtime.status.read().await);
                    let model = runtime.selected_model.read().await.clone();
                    let agent = runtime.selected_agent.read().await.clone();
                    let pending_permissions: Vec<String> = runtime
                        .pending_permissions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let pending_questions: Vec<String> = runtime
                        .pending_questions
                        .iter()
                        .map(|r| r.key().clone())
                        .collect();
                    let input_tokens = *runtime.last_input_tokens.read().await;
                    let output_tokens = *runtime.last_output_tokens.read().await;
                    let active_subagents = runtime
                        .active_subagent_count
                        .load(std::sync::atomic::Ordering::Relaxed);
                    (
                        status,
                        model,
                        agent,
                        pending_permissions,
                        pending_questions,
                        input_tokens,
                        output_tokens,
                        active_subagents,
                    )
                } else {
                    (
                        "idle".to_string(),
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                        0,
                    )
                };

                let event_seq = self.event_log.current_seq();

                Ok(CoreResponse::SnapshotSession {
                    event_seq,
                    session: crate::protocol_conversions::session_to_dto(session).unwrap_or_else(
                        |e| {
                            tracing::error!(error = %e, "session_to_dto conversion failed");
                            Default::default()
                        },
                    ),
                    messages: crate::protocol_conversions::messages_to_dtos(messages)
                        .unwrap_or_else(|e| {
                            tracing::error!(error = %e, "messages_to_dtos conversion failed");
                            Default::default()
                        }),
                    status,
                    selected_model,
                    selected_agent,
                    pending_permissions,
                    pending_questions,
                    input_tokens,
                    output_tokens,
                    active_subagents,
                })
            }
            CoreRequest::SnapshotModels => {
                let config = super::load_config_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let model_ids: Vec<String> = if let Some(pool) = self.pool.clone() {
                    let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                        std::path::PathBuf::new(),
                    )
                    .with_pool(pool);
                    let models = discovery.refresh(&registry).await;
                    models
                        .iter()
                        .map(|m| format!("{}/{}", m.provider, m.id))
                        .collect()
                } else {
                    let mut ids = Vec::new();
                    for provider in registry.list() {
                        if let Ok(models) = provider.models().await {
                            for m in models {
                                ids.push(format!("{}/{}", provider.id(), m.id));
                            }
                        }
                    }
                    ids
                };
                Ok(CoreResponse::ModelsSnapshot {
                    current_model: None,
                    models: model_ids,
                })
            }
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }
}
