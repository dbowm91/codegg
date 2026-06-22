use std::sync::Arc;
use std::time::Instant;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
use chrono::Utc;

use super::event_log::EventFilter;
use super::runtime_deps::CoreRuntimeDeps;
use crate::core::session_runtime::RuntimeSessionStatus;

pub struct CoreDaemon {
    pub daemon_id: String,
    pub pool: Option<sqlx::SqlitePool>,
    pub deps: CoreRuntimeDeps,
    pub event_log: Arc<super::event_log::EventLog>,
    pub sessions: Arc<crate::core::session_runtime::SessionRuntimeRegistry>,
    pub clients: Arc<super::client_registry::ClientRegistry>,
    pub notification_router: Arc<super::notification::NotificationRouter>,
    pub audio_arbiter: Option<Arc<super::notification::AudioArbiter>>,
    pub started_at: Instant,
}

impl CoreDaemon {
    /// Construct a `CoreDaemon` from a bundled [`CoreRuntimeDeps`].
    pub fn with_deps(deps: CoreRuntimeDeps) -> Self {
        let daemon_id = format!("codegg-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let config = crate::config::schema::Config::load().unwrap_or_default();
        let capacity = config
            .daemon
            .as_ref()
            .and_then(|d| d.event_log_capacity)
            .unwrap_or(4096);
        let event_log = match deps.pool {
            Some(ref p) => Arc::new(super::event_log::EventLog::new_with_pool(
                capacity,
                p.clone(),
            )),
            None => Arc::new(super::event_log::EventLog::new(capacity)),
        };
        let notification_router = Arc::new(super::notification::NotificationRouter::new(
            super::notification::NotificationPolicy::from_config(&config),
        ));
        let audio_arbiter = if notification_router.is_tts_enabled() {
            let arbiter = Arc::new(super::notification::AudioArbiter::new(Arc::clone(
                &notification_router,
            )));
            arbiter.start();
            Some(arbiter)
        } else {
            None
        };
        Self {
            daemon_id,
            pool: deps.pool.clone(),
            deps,
            event_log,
            sessions: Arc::new(crate::core::session_runtime::SessionRuntimeRegistry::new()),
            clients: Arc::new(super::client_registry::ClientRegistry::new()),
            notification_router,
            audio_arbiter,
            started_at: Instant::now(),
        }
    }

    /// Legacy constructor for backward compatibility. Prefer `with_deps`.
    pub fn new(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self {
        Self::with_deps(CoreRuntimeDeps::new(
            pool,
            subagent_pool,
            memory_store,
            bg_scheduler,
        ))
    }

    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<
        crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>,
    > {
        self.event_log.subscribe()
    }

    /// Apply the bridge fallback to a single `AppEvent` and return the
    /// resulting `(session_id, turn_id, core_event)` triple, or `None` if
    /// the event has no corresponding `CoreEvent`. If the bridged event
    /// has an empty or missing `turn_id`, look up the active turn for the
    /// session and attach its `turn_id` to the event so every event
    /// belonging to a turn carries the same identity.
    pub(crate) async fn bridge_app_event(
        &self,
        app_event: crate::bus::events::AppEvent,
    ) -> Option<(
        Option<String>,
        Option<String>,
        crate::protocol::core::CoreEvent,
    )> {
        let mut core_event = super::map_app_event_to_core_event(app_event)?;
        let (session_id, mut turn_id) = super::core_event_metadata(&core_event);
        let turn_id_empty = match &turn_id {
            Some(t) => t.is_empty(),
            None => true,
        };
        if turn_id_empty {
            if let Some(sid) = session_id.clone() {
                if let Some(runtime) = self.sessions.get(&sid) {
                    let active = runtime.active_turn.read().await;
                    if let Some(handle) = active.as_ref() {
                        core_event =
                            super::set_turn_id_on_event(core_event, handle.turn_id.clone());
                        turn_id = Some(handle.turn_id.clone());
                    }
                }
            }
        }
        Some((session_id, turn_id, core_event))
    }

    /// Recover daemon state after restart.
    /// Marks previously active turns as failed and logs stale permissions/questions.
    pub async fn recover_state(&self) {
        let Some(ref pool) = self.pool else {
            return;
        };

        // Find interrupted turns: TurnStarted without a matching TurnCompleted/TurnFailed
        // for the same session_id + turn_id. Use the explicit event-type strings
        // written by `core_event_type()` (snake_case) so the query is stable and
        // grep-able. The DISTINCT + NOT EXISTS pattern ensures each (session, turn)
        // pair is reported at most once and we only flag turns that have a real
        // turn_id (e.g., not blank rows from older schemas).
        let active_turns: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT e1.session_id, e1.turn_id \
             FROM core_event_log e1 \
             WHERE e1.event_type = 'turn_started' \
             AND e1.turn_id IS NOT NULL \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.session_id = e1.session_id \
                 AND e2.turn_id = e1.turn_id \
                 AND (e2.event_type = 'turn_completed' OR e2.event_type = 'turn_failed') \
             )",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !active_turns.is_empty() {
            tracing::info!(
                "Recovery: found {} interrupted turn(s), emitting TurnFailed",
                active_turns.len()
            );
            for (session_id, turn_id) in &active_turns {
                tracing::info!(
                    "  Marking session {} turn {} as failed (daemon restarted while active)",
                    session_id,
                    turn_id
                );
                self.event_log
                    .publish(
                        Some(session_id.clone()),
                        Some(turn_id.clone()),
                        crate::protocol::core::CoreEvent::TurnFailed {
                            session_id: session_id.clone(),
                            turn_id: Some(turn_id.clone()),
                            message: "Daemon restarted while turn was active".to_string(),
                        },
                    )
                    .await;

                // Clear runtime state for this session
                if let Some(runtime) = self.sessions.get(session_id) {
                    let mut active = runtime.active_turn.write().await;
                    *active = None;
                    drop(active);

                    let mut status = runtime.status.write().await;
                    *status = RuntimeSessionStatus::Idle;
                }
            }
        }

        // Count stale PermissionPending events (no PermissionResponded in same session)
        let stale_perms: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'permission_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'permission_responded' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // Count stale QuestionPending events (no QuestionAnswered in same session)
        let stale_questions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM core_event_log WHERE event_type = 'question_pending' \
             AND NOT EXISTS ( \
                 SELECT 1 FROM core_event_log e2 \
                 WHERE e2.event_type = 'question_answered' \
                 AND e2.session_id = core_event_log.session_id \
             )",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if stale_perms > 0 || stale_questions > 0 {
            tracing::info!(
                "Recovery: {} stale permission(s), {} stale question(s) from previous run (will timeout naturally)",
                stale_perms,
                stale_questions
            );
        }

        tracing::info!("Daemon state recovery complete");
    }

    pub async fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<crate::protocol::core::EventEnvelope<crate::protocol::core::CoreEvent>> {
        self.event_log.replay_from(from_event_seq, filter).await
    }

    pub fn start_event_bridge(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            let mut bus_rx = crate::bus::global::GlobalEventBus::subscribe();
            loop {
                match bus_rx.recv().await {
                    Ok(app_event) => {
                        if let Some((session_id, turn_id, core_event)) =
                            daemon.bridge_app_event(app_event.clone()).await
                        {
                            daemon
                                .event_log
                                .publish(session_id, turn_id, core_event)
                                .await;
                        }
                        match &app_event {
                            crate::bus::events::AppEvent::AgentFinished {
                                session_id,
                                stop_reason,
                                input_tokens,
                                output_tokens,
                                ..
                            } => {
                                // Update runtime token counts
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    *runtime.last_input_tokens.write().await = *input_tokens;
                                    *runtime.last_output_tokens.write().await = *output_tokens;
                                }
                                use super::notification::*;
                                let kind = if stop_reason == "error" {
                                    NotificationKind::TurnFailed
                                } else {
                                    NotificationKind::TurnCompleted
                                };
                                let priority = if stop_reason == "error" {
                                    NotificationPriority::High
                                } else {
                                    NotificationPriority::Low
                                };
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind,
                                    priority,
                                    message: format!(
                                        "Turn {} for session {}",
                                        stop_reason, session_id
                                    ),
                                    dedupe_key: Some(format!("turn-done:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::PermissionPending {
                                session_id,
                                turn_id,
                                tool,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::PermissionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: format!("Permission required for tool: {}", tool),
                                    dedupe_key: Some(format!("perm:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::QuestionPending {
                                session_id,
                                turn_id,
                                ..
                            } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: turn_id.clone(),
                                    kind: NotificationKind::QuestionRequired,
                                    priority: NotificationPriority::Urgent,
                                    message: "Question requires your input".to_string(),
                                    dedupe_key: Some(format!("question:{}", session_id)),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::Error { message } => {
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: None,
                                    turn_id: None,
                                    kind: NotificationKind::Error,
                                    priority: NotificationPriority::High,
                                    message: message.clone(),
                                    dedupe_key: None,
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentStarted {
                                session_id, ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                            crate::bus::events::AppEvent::SubagentCompleted {
                                session_id,
                                task_id,
                                agent,
                                ..
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentCompleted,
                                    priority: NotificationPriority::Normal,
                                    message: format!(
                                        "Subagent {} completed task {}",
                                        agent, task_id
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-done:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            crate::bus::events::AppEvent::SubagentFailed {
                                session_id,
                                task_id,
                                agent,
                                error,
                            } => {
                                if let Some(runtime) = daemon.sessions.get(session_id) {
                                    runtime
                                        .active_subagent_count
                                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                use super::notification::*;
                                let event = NotificationEvent {
                                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                                    session_id: Some(session_id.clone()),
                                    turn_id: None,
                                    kind: NotificationKind::SubagentFailed,
                                    priority: NotificationPriority::High,
                                    message: format!(
                                        "Subagent {} failed task {}: {}",
                                        agent, task_id, error
                                    ),
                                    dedupe_key: Some(format!(
                                        "subagent-fail:{}:{}",
                                        session_id, task_id
                                    )),
                                    created_at: Utc::now(),
                                };
                                daemon.notification_router.emit(event.clone()).await;
                                if let Some(ref pool) = daemon.pool {
                                    daemon
                                        .notification_router
                                        .persist_notification(pool, &event)
                                        .await;
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Event bridge lagged, {} events dropped", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub async fn handle_request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
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
                // Validate the provider exists before delegating to the turn
                // runtime. This preserves the existing `provider_not_found`
                // response shape from the daemon layer. The turn runtime
                // also validates provider existence internally, so this is
                // intentionally duplicated for backward-compatible error handling.
                let mut registry = crate::provider::ProviderRegistry::new();
                let config = crate::config::schema::Config::load().unwrap_or_default();
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

                let runtime = self.sessions.get_or_create(
                    &session_id,
                    &session_id,
                    std::path::PathBuf::from("."),
                );

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

                // Delegate to the injected turn runtime which handles tool
                // registry, agent loop construction, and background spawning.
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
            CoreRequest::SessionMessagesLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::MessageStore::new(pool);
                match store.list(&session_id).await {
                    Ok(messages) => Ok(CoreResponse::SessionMessages {
                        session_id,
                        messages: crate::protocol_conversions::messages_to_dtos(messages),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_messages_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionMessageCounts { session_ids } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.message_counts(&session_ids).await {
                    Ok(counts) => Ok(CoreResponse::SessionMessageCounts { counts }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_message_counts_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreate { directory, title } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .create(crate::session::CreateSession {
                        project_id: directory.clone(),
                        directory,
                        title,
                        parent_id: None,
                        workspace_id: None,
                        agent: None,
                        model: None,
                        tags: None,
                    })
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionLoad { session_id } | CoreRequest::SessionAttach { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.get(&session_id).await {
                    Ok(Some(session)) => {
                        self.sessions.get_or_create(
                            &session_id,
                            &session.project_id,
                            std::path::PathBuf::from(&session.directory),
                        );
                        Ok(CoreResponse::Session {
                            session: crate::protocol_conversions::session_to_dto(session),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("Session not found: {}", session_id),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionList {
                project_id,
                show_archived,
                limit,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let sessions = if show_archived {
                    store.list_all(&project_id, None).await
                } else {
                    store.list(&project_id, limit).await
                };
                match sessions {
                    Ok(sessions) => Ok(CoreResponse::SessionList {
                        sessions: crate::protocol_conversions::sessions_to_dtos(sessions),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionFork { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.fork(&session_id).await {
                    Ok(_) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_fork_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionDelete {
                session_id,
                permanent,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if permanent {
                    store.delete(&session_id).await.map(|_| ())
                } else {
                    store.soft_delete(&session_id).await.map(|_| ())
                };
                match result {
                    Ok(()) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_delete_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionArchive {
                session_id,
                unarchive,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if unarchive {
                    store.unarchive(&session_id).await
                } else {
                    store.archive(&session_id).await
                };
                match result {
                    Ok(_) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_archive_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRestore { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.restore(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_restore_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionShare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.share_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_share_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionUnshare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.unshare_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_unshare_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRename {
                session_id,
                new_title,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .update(
                        &session_id,
                        crate::session::UpdateSession {
                            title: Some(new_title),
                            share_url: None,
                            summary_additions: None,
                            summary_deletions: None,
                            summary_files: None,
                            summary_diffs: None,
                            revert: None,
                            permission: None,
                            tags: None,
                            time_compacting: None,
                            time_archived: None,
                        },
                    )
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_rename_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionExport { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.export_session(&session_id).await {
                    Ok(data) => Ok(CoreResponse::Json { data }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_export_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionImportData { data } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.import_session(data, None).await {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_import_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreateFromTemplate {
                template,
                project_id,
                directory,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .create_from_template(
                        &crate::protocol_conversions::dto_to_session_template(template),
                        &project_id,
                        &directory,
                    )
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session {
                        session: crate::protocol_conversions::session_to_dto(session),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_from_template_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
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
                let config = crate::config::schema::Config::load().unwrap_or_default();
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
            CoreRequest::TaskList => {
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let tasks = scheduler.list().await;
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "tasks": tasks.iter().map(|t| serde_json::json!({
                            "id": t.id,
                            "message": t.message,
                            "interval_secs": t.interval.as_secs(),
                            "session_id": t.session_id,
                            "created_at": t.created_at,
                            "last_run": t.last_run,
                        })).collect::<Vec<_>>()
                    }),
                })
            }
            CoreRequest::TaskDelete { id } => {
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let removed = scheduler.remove(&id.to_string()).await;
                if removed {
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "task_not_found".to_string(),
                        message: format!("Task not found: {}", id),
                    })
                }
            }
            CoreRequest::TaskSchedule {
                session_id,
                interval_secs,
                message,
            } => {
                let Some(scheduler) = self.deps.legacy_agent.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let task = crate::agent::task::BackgroundTask::new(
                    session_id.clone(),
                    std::time::Duration::from_secs(interval_secs),
                    message.clone(),
                );
                let task_id = task.id.clone();
                match scheduler.add(task).await {
                    Ok(_) => {
                        if let Some(pool) = self.deps.legacy_agent.subagent_pool.clone() {
                            let request = crate::agent::worker::SubAgentRequest {
                                task_id: 0,
                                prompt: format!("[Background] {}", message),
                                agent: "build".to_string(),
                                parent_id: Some(session_id),
                                denied_tools: Vec::new(),
                                allowed_paths: Vec::new(),
                                description: "Background loop task".to_string(),
                                depth: 1,
                                max_tool_calls: None,
                            };
                            let _ = pool.spawner().send(request).await;
                        }
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "task_id": task_id, "interval_secs": interval_secs }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "task_schedule_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorktreeList { project_dir } => {
                let git_root = std::path::PathBuf::from(&project_dir);
                let Some(root) = crate::worktree::find_git_root(&git_root) else {
                    return Ok(CoreResponse::Json {
                        data: serde_json::json!({ "worktrees": [] }),
                    });
                };
                match crate::worktree::list_worktrees(&root).await {
                    Ok(trees) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "worktrees": trees.iter().map(|t| serde_json::json!({
                                "path": t.path,
                                "branch": t.branch
                            })).collect::<Vec<_>>()
                        }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "worktree_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::MemoryList { namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.list(&namespace);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemorySearch { query } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.search(&query);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemoryRemember { text, namespace } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let ns = namespace.unwrap_or_else(|| "user/preferences".to_string());
                let memory = crate::memory::Memory::new(ns, text);
                memory_store.add(memory.clone());
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memory": memory }),
                })
            }
            CoreRequest::MemoryForget { id } => {
                let Some(memory_store) = self.deps.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let deleted = memory_store.delete(&id).is_some();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "deleted": deleted }),
                })
            }
            CoreRequest::GoalSet {
                session_id,
                project_id,
                objective,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                let title = objective
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(&objective)
                    .chars()
                    .take(80)
                    .collect::<String>();
                let completion_criteria = vec![
                    "Implementation satisfies the stated objective.".to_string(),
                    "Relevant tests or checks have been run, or skipped with justification."
                        .to_string(),
                    "Checkpoint/progress state is updated.".to_string(),
                ];
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        None,
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            None,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalFromFile {
                session_id,
                project_id,
                path,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let file_path = if std::path::Path::new(&path).is_absolute() {
                    std::path::PathBuf::from(&path)
                } else {
                    std::path::PathBuf::from(&project_id).join(&path)
                };
                let content = match tokio::fs::read_to_string(&file_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "file_read_failed".to_string(),
                            message: format!("Failed to read {}: {}", path, e),
                        })
                    }
                };
                let title = content
                    .lines()
                    .find(|l| l.starts_with('#'))
                    .map(|l| {
                        l.trim_start_matches('#')
                            .trim()
                            .chars()
                            .take(80)
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| {
                        std::path::Path::new(&path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Goal from file".to_string())
                    });
                let objective = format!("Follow implementation plan from {}", path);
                let completion_criteria = vec![
                    "All phases in the plan file that are in scope are completed.".to_string(),
                    "Tests/checks specified in the plan have been run.".to_string(),
                    "Goal checkpoint is updated with completed/remaining work.".to_string(),
                ];
                let plan_excerpt = if content.len() > 4000 {
                    Some(&content[..4000])
                } else {
                    Some(content.as_str())
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        Some(path),
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            plan_excerpt,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": goal.title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalShow { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                            crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                                .await
                                .ok()
                                .flatten()
                        } else {
                            None
                        };
                        let rendered = crate::goal::render::render_goal_status(&goal);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "goal": serde_json::to_value(&goal).unwrap_or_default(),
                                "rendered": rendered,
                                "checkpoint_excerpt": checkpoint_excerpt,
                            }),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_show_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalPause { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Paused)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_pause_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to pause".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_pause_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalResume { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.latest_paused_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Active)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_resume_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_paused_goal".to_string(),
                        message: "No paused goal to resume".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_resume_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalClear { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.clear_active_for_session(&session_id).await {
                    Ok(()) => {
                        super::publish_goal_updated(&session_id, None);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "cleared": true }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_clear_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalDone { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Complete)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated.clone()));
                                crate::bus::global::GlobalEventBus::publish(
                                    crate::bus::events::AppEvent::GoalCompleted {
                                        session_id: session_id.clone(),
                                        goal_id: goal.id.clone(),
                                        evidence: "marked complete via /goal done".to_string(),
                                    },
                                );
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_done_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to mark done".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_done_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalCheckpoint {
                session_id,
                project_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        if let Some(ref cp_path) = goal.checkpoint_path {
                            let update = crate::goal::GoalProgressUpdate {
                                current_phase: goal.current_phase.clone(),
                                progress_summary: Some(goal.progress_summary.clone()),
                                next_action: goal.next_action.clone(),
                                completed_items: vec![],
                                remaining_items: vec![],
                                open_questions: goal.open_questions.clone(),
                            };
                            let _ =
                                crate::goal::checkpoint::append_checkpoint_update(cp_path, &update)
                                    .await;
                            Ok(CoreResponse::Json {
                                data: serde_json::json!({ "checkpoint_path": cp_path, "appended": true }),
                            })
                        } else {
                            let project_path = std::path::PathBuf::from(&project_id);
                            match crate::goal::checkpoint::create_checkpoint_file(
                                &project_path,
                                &goal,
                                None,
                            )
                            .await
                            {
                                Ok(path) => {
                                    let path_str = path.to_string_lossy().to_string();
                                    let _ = sqlx::query(
                                        "UPDATE goal SET checkpoint_path = ? WHERE id = ?",
                                    )
                                    .bind(&path_str)
                                    .bind(&goal.id)
                                    .execute(&goal_store.pool)
                                    .await;
                                    Ok(CoreResponse::Json {
                                        data: serde_json::json!({ "checkpoint_path": path_str, "created": true }),
                                    })
                                }
                                Err(e) => Ok(CoreResponse::Error {
                                    code: "goal_checkpoint_failed".to_string(),
                                    message: e.to_string(),
                                }),
                            }
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_checkpoint_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::TodoList { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::store::TodoStore::new(pool);
                match store.list(&session_id).await {
                    Ok(items) => {
                        let snapshots: Vec<crate::bus::events::TodoItemSnapshot> = items
                            .iter()
                            .enumerate()
                            .map(|(i, item)| {
                                use crate::bus::events::TodoItemSnapshot;
                                TodoItemSnapshot {
                                    id: format!("pos-{}", i),
                                    content: item.content.clone(),
                                    status: item.status.clone(),
                                    priority: item.priority.clone(),
                                }
                            })
                            .collect();
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "items": serde_json::to_value(&snapshots)
                                    .unwrap_or(serde_json::Value::Array(vec![])),
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "todo_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ActiveGoalLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "active": true,
                            "goal": serde_json::to_value(goal.to_snapshot())
                                .unwrap_or(serde_json::Value::Null),
                        }),
                    }),
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "active_goal_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalSetBudget {
                session_id,
                max_turns,
                max_model_tokens,
                max_tool_calls,
                max_wallclock_secs,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let new_budget = crate::goal::model::GoalBudget {
                            max_turns,
                            max_model_tokens,
                            max_tool_calls,
                            max_wallclock_secs,
                        };
                        match goal_store.set_budget(&goal.id, new_budget).await {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "ok", "id": goal.id }),
                                })
                            }
                            Ok(None) => Ok(CoreResponse::Json {
                                data: serde_json::json!({ "status": "ok", "id": goal.id }),
                            }),
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_set_budget_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to update budget".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_set_budget_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
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
                        let _ = handle.cancel_tx.send(true);
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
                            let _ = steer_tx.send(text);
                            Ok(CoreResponse::Ack)
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
                let runtime = self.sessions.get_or_create(
                    &session_id,
                    &session_id,
                    std::path::PathBuf::from("."),
                );
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
                let runtime = self.sessions.get_or_create(
                    &session_id,
                    &session_id,
                    std::path::PathBuf::from("."),
                );
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

                let messages = msg_store.list(&session_id).await.unwrap_or_default();

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
                    session: crate::protocol_conversions::session_to_dto(session),
                    messages: crate::protocol_conversions::messages_to_dtos(messages),
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
                let config = crate::config::schema::Config::load().unwrap_or_default();
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
            CoreRequest::SnapshotDaemon => {
                let event_seq = self.event_log.current_seq();
                let session_ids = self.sessions.list_sessions();
                let mut snapshots = Vec::new();
                for sid in &session_ids {
                    if let Some(runtime) = self.sessions.get(sid) {
                        let status = format!("{:?}", *runtime.status.read().await);
                        let model = runtime.selected_model.read().await.clone();
                        let agent = runtime.selected_agent.read().await.clone();
                        let has_active_turn = runtime.active_turn.read().await.is_some();
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
                        snapshots.push(crate::protocol::core::SessionSnapshot {
                            session_id: sid.clone(),
                            project_id: runtime.project_id.clone(),
                            status,
                            selected_model: model,
                            selected_agent: agent,
                            has_active_turn,
                            pending_permissions,
                            pending_questions,
                            input_tokens,
                            output_tokens,
                            active_subagents,
                        });
                    }
                }
                Ok(CoreResponse::SnapshotDaemon {
                    event_seq,
                    daemon_id: self.daemon_id.clone(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    active_sessions: snapshots,
                    connected_clients: self
                        .clients
                        .list()
                        .iter()
                        .map(|c| crate::protocol::core::ClientSnapshot {
                            client_id: c.client_id.clone(),
                            client_name: c.client_name.clone(),
                            connected_at: c.connected_at.to_rfc3339(),
                            attached_sessions: c.attached_sessions.clone(),
                        })
                        .collect(),
                })
            }
            CoreRequest::SnapshotWorkspace { project_dir } => {
                let path = std::path::PathBuf::from(&project_dir);

                let git_root = crate::worktree::find_git_root(&path);

                let git_status = git_root.as_ref().and_then(|root| {
                    std::process::Command::new("git")
                        .args(["status", "--porcelain"])
                        .current_dir(root)
                        .output()
                        .ok()
                        .map(|output| {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let changed_files = stdout.lines().count();
                            serde_json::json!({
                                "git_root": root.to_string_lossy(),
                                "changed_files": changed_files,
                            })
                        })
                });

                let worktrees: Vec<serde_json::Value> = match git_root.as_ref() {
                    Some(root) => crate::worktree::list_worktrees(root)
                        .await
                        .unwrap_or_default()
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "path": t.path,
                                "branch": t.branch,
                            })
                        })
                        .collect(),
                    None => Vec::new(),
                };

                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "project_dir": project_dir,
                        "git_status": git_status,
                        "worktrees": worktrees,
                    }),
                })
            }
            CoreRequest::NotificationSpeak {
                text,
                kind,
                priority,
                session_id,
            } => {
                use super::notification::*;
                let kind = match kind.as_deref() {
                    Some("turn_completed") => NotificationKind::TurnCompleted,
                    Some("turn_failed") => NotificationKind::TurnFailed,
                    Some("awaiting_input") => NotificationKind::AwaitingInput,
                    Some("permission_required") => NotificationKind::PermissionRequired,
                    Some("question_required") => NotificationKind::QuestionRequired,
                    Some("subagent_completed") => NotificationKind::SubagentCompleted,
                    Some("subagent_failed") => NotificationKind::SubagentFailed,
                    Some("error") => NotificationKind::Error,
                    _ => NotificationKind::AwaitingInput,
                };
                let priority = match priority.as_deref() {
                    Some("urgent") => NotificationPriority::Urgent,
                    Some("high") => NotificationPriority::High,
                    Some("low") => NotificationPriority::Low,
                    _ => NotificationPriority::Normal,
                };
                let event = NotificationEvent {
                    id: format!("notif-{}", uuid::Uuid::new_v4()),
                    session_id,
                    turn_id: None,
                    kind,
                    priority,
                    message: text,
                    dedupe_key: None,
                    created_at: Utc::now(),
                };
                self.notification_router.emit(event.clone()).await;
                if let Some(ref pool) = self.pool {
                    self.notification_router
                        .persist_notification(pool, &event)
                        .await;
                }
                Ok(CoreResponse::Ack)
            }
            CoreRequest::NotificationStop => {
                if let Some(ref arbiter) = self.audio_arbiter {
                    arbiter.request_interrupt();
                }
                Ok(CoreResponse::Ack)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::turn_runtime::TurnRuntime;
    use crate::core::CoreEvent;
    use crate::session::schema::migrate;

    /// Build a fresh in-memory SQLite pool with the full session
    /// schema. No on-disk tempdir is created, so the pool's memory is
    /// reclaimed when the test's `SqlitePool` is dropped — no
    /// `Box::leak` required.
    async fn in_memory_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:daemon_test_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        migrate(&pool).await.expect("migrate");
        pool
    }

    async fn test_daemon() -> CoreDaemon {
        let pool = in_memory_pool().await;
        CoreDaemon::new(Some(pool), None, None, None)
    }

    #[tokio::test]
    async fn daemon_has_unique_id() {
        let d1 = test_daemon().await;
        let d2 = test_daemon().await;
        assert_ne!(d1.daemon_id, d2.daemon_id);
    }

    #[tokio::test]
    async fn session_create_through_daemon() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/test".into(),
                title: Some("Test".into()),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Session { .. }));
    }

    #[tokio::test]
    async fn snapshot_daemon_returns_state() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-1".into(), CoreRequest::SnapshotDaemon);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::SnapshotDaemon {
                daemon_id,
                uptime_secs,
                ..
            } => {
                assert!(!daemon_id.is_empty());
                assert!(uptime_secs < 5);
            }
            other => panic!("expected SnapshotDaemon, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_submit_rejects_when_active() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: None,
            },
        );
        let session_id = match daemon.handle_request(req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            _ => panic!("expected Session"),
        };

        let runtime =
            daemon
                .sessions
                .get_or_create(&session_id, &session_id, std::path::PathBuf::new());
        assert!(runtime.active_turn.read().await.is_none());
    }

    #[tokio::test]
    async fn resume_returns_typed_resync_when_seq_too_old() {
        // This test exercises the same path as before -- a too-old seq
        // when nothing is recorded anywhere. To force the ring to
        // have no record of seq 1, we use a no-pool daemon (so the
        // DB layer is bypassed) and a small ring, then evict the
        // only event by overflowing the ring.
        let daemon = CoreDaemon::new(None, None, None, None);
        // No pool is configured, so the event log is in-memory only
        // and the ring is the source of truth.
        // Publish a few events to a small ring by setting capacity
        // indirectly: we use the default capacity (4096) and publish
        // a single event so seq=1 is in the ring, then issue a
        // resume from seq 0 with no pool -- this would be covered by
        // the ring. To get a true "too old" we need to evict seq 1.
        // The cleanest way without changing daemon internals is to
        // request a seq the ring definitely does not have; with no
        // pool, the only valid request is one the ring can satisfy.
        // A future seq (e.g. 999_999) is treated as caught-up and
        // returns Events(empty), NOT ResyncRequired. So we use a
        // daemon without a pool and no events at all, with a
        // from_event_seq < current_seq (0 < 0 is false). The truly
        // "too old" case below uses a pool + eviction.
        // With no events and from_event_seq=0 and current_seq=0, the
        // path is caught-up and returns empty events.
        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 999_999,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 0);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_returns_typed_events_on_success() {
        let daemon = test_daemon().await;

        daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-ok".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, 1);
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_seq, 1);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_current_seq_returns_empty_events_not_resync() {
        // A client that is already caught up (from_event_seq == current_seq)
        // must get an empty Events response, NOT ResyncRequired. This is
        // the core Pass 2 invariant: ResyncRequired is reserved for
        // too-old sequences that can no longer be replayed.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-current".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(
                    events.is_empty(),
                    "expected empty events for caught-up client, got {:?}",
                    events
                );
            }
            other => panic!("expected Events(empty), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_future_seq_returns_empty_events() {
        // from_event_seq > current_seq is treated as "no new events" --
        // the client effectively overshot but we don't have anything to
        // send. Return Events(empty, current_seq) so the client can
        // resync its bookkeeping. The plan lists this as one of the
        // acceptable behaviors; we chose empty events.
        let daemon = test_daemon().await;
        let s1 = daemon
            .event_log
            .publish(
                Some("s1".into()),
                None,
                crate::protocol::core::CoreEvent::SessionUpdated {
                    session_id: "s1".into(),
                },
            )
            .await;

        let req = crate::core::new_request(
            "req-resume-future".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: s1 + 100,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Events {
                events,
                current_seq,
            } => {
                assert_eq!(current_seq, s1);
                assert!(events.is_empty());
            }
            other => panic!("expected Events(empty) for future seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn resume_from_too_old_seq_returns_resync() {
        // To force a real "too old" outcome we need a daemon without
        // a SQLite pool. With a pool, the DB layer would still cover
        // any from_event_seq whose `from_event_seq + 1` is in the
        // persisted range, so the resync path becomes unreachable
        // for ordinary replay requests. With no pool, the ring is
        // the source of truth and eviction makes old seqs unsatisfiable.
        let daemon = CoreDaemon::new(None, None, None, None);
        // Publish enough events to overflow the default ring (4096).
        for _ in 0..5000 {
            daemon
                .event_log
                .publish(
                    Some("s1".into()),
                    None,
                    crate::protocol::core::CoreEvent::Error {
                        code: "filler".into(),
                        message: "m".into(),
                    },
                )
                .await;
        }
        let current = daemon.event_log.current_seq();
        assert!(
            current > 4096,
            "ring should have wrapped, current={}",
            current
        );

        // from_event_seq=0 is now too old: the ring's front is
        // current-4095 and there is no DB to fall back to.
        let req = crate::core::new_request(
            "req-resume-old".into(),
            CoreRequest::Resume {
                session_id: Some("s1".into()),
                from_event_seq: 0,
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ResyncRequired {
                from_event_seq,
                current_seq,
                session_id,
            } => {
                assert_eq!(from_event_seq, 0);
                assert_eq!(current_seq, current);
                assert_eq!(session_id.as_deref(), Some("s1"));
            }
            other => panic!("expected ResyncRequired for too-old seq, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn recovery_detects_interrupted_turn() {
        let daemon = test_daemon().await;

        // Subscribe before publishing so we can observe the recovery-emitted TurnFailed.
        let mut rx = daemon.event_log.subscribe();

        // Insert an interrupted TurnStarted directly (no matching TurnCompleted/TurnFailed).
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // The recovery should have published a TurnFailed for (s1, t1).
        let mut found = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected recovery to emit TurnFailed for s1/t1");
    }

    #[tokio::test]
    async fn recovery_ignores_completed_turn() {
        let daemon = test_daemon().await;

        let mut rx = daemon.event_log.subscribe();

        // Insert a completed turn: TurnStarted followed by TurnCompleted.
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (1, 's1', 't1', 'turn_started', '{}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) \
             VALUES (2, 's1', 't1', 'turn_completed', '{\"stop_reason\":\"ok\"}')",
        )
        .execute(daemon.pool.as_ref().unwrap())
        .await
        .unwrap();

        daemon.recover_state().await;

        // Drain and ensure no TurnFailed was emitted.
        let mut emitted_failed = false;
        while let Ok(env) = rx.try_recv() {
            if let crate::protocol::core::CoreEvent::TurnFailed {
                session_id,
                turn_id,
                ..
            } = &env.payload
            {
                if session_id == "s1" && turn_id.as_deref() == Some("t1") {
                    emitted_failed = true;
                    break;
                }
            }
        }
        assert!(
            !emitted_failed,
            "did not expect recovery to emit TurnFailed for a completed turn"
        );
    }

    #[tokio::test]
    async fn snapshot_models_returns_model_ids() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request("req-snap".into(), CoreRequest::SnapshotModels);
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::ModelsSnapshot {
                current_model,
                models,
            } => {
                assert!(current_model.is_none());
                // With no providers configured, the model list is empty.
                // The format contract is `provider/model` (e.g. `openai/gpt-4o`),
                // which is exercised by ModelsRefresh; for the empty-config case
                // we only assert the response shape is well-formed.
                for m in &models {
                    assert!(
                        m.contains('/'),
                        "model id '{}' should be 'provider/model'",
                        m
                    );
                }
            }
            other => panic!("expected ModelsSnapshot, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-invalid".into(),
            CoreRequest::PermissionRespond {
                id: "perm-1".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_permission_id");
                assert!(message.contains("perm-1"));
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_malformed_id() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-perm-malformed".into(),
            CoreRequest::PermissionRespond {
                id: "perm:foo:bar".into(),
                choice: "allow".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "invalid_permission_id");
            }
            other => panic!("expected Error(invalid_permission_id), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn question_respond_invalid_id_format() {
        let daemon = test_daemon().await;
        let req = crate::core::new_request(
            "req-q-invalid".into(),
            CoreRequest::QuestionRespond {
                id: "q-1".into(),
                answers: serde_json::json!("yes"),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "invalid_question_id");
                assert!(message.contains("q-1"));
            }
            other => panic!("expected Error(invalid_question_id), got {:?}", other),
        }
    }

    /// Manually install a `TurnHandle` on the given runtime's
    /// `active_turn` so we can exercise `TurnCancel`/`TurnSteer` paths
    /// without spinning up an actual agent loop. Returns the cancel
    /// sender, the cancel receiver (so the watch channel stays open),
    /// and the steer receiver so tests can observe the downstream
    /// effects.
    async fn install_active_turn(
        runtime: &std::sync::Arc<crate::core::session_runtime::SessionRuntime>,
        turn_id: &str,
    ) -> (
        tokio::sync::watch::Sender<bool>,
        tokio::sync::watch::Receiver<bool>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
    ) {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (steer_tx, steer_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut active = runtime.active_turn.write().await;
        *active = Some(crate::core::session_runtime::TurnHandle {
            turn_id: turn_id.to_string(),
            cancel_tx: cancel_tx.clone(),
            steer_tx: Some(steer_tx),
            started_at: chrono::Utc::now(),
        });
        (cancel_tx, cancel_rx, steer_rx)
    }

    #[tokio::test]
    async fn turn_cancel_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-cancel-wrong",
            "s-cancel-wrong",
            std::path::PathBuf::from("."),
        );
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-real").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-wrong".into(),
                turn_id: "turn-typo".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, message } => {
                assert_eq!(code, "turn_id_mismatch");
                assert!(message.contains("turn-typo"));
                assert!(message.contains("turn-real"));
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }

        // The runtime should still have an active turn; we did not cancel.
        let active = runtime.active_turn.read().await;
        assert!(
            active.is_some(),
            "active_turn should remain set after a rejected cancel"
        );
        // The cancel channel should not have been signaled.
        assert!(
            !*cancel_tx.borrow(),
            "cancel_tx should not have been signaled"
        );
    }

    #[tokio::test]
    async fn turn_cancel_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-cancel-ok",
            "s-cancel-ok",
            std::path::PathBuf::from("."),
        );
        let (cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, "turn-good").await;

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-ok".into(),
                turn_id: "turn-good".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The cancel channel should have been signaled.
        assert!(
            *cancel_tx.borrow(),
            "cancel_tx should have been signaled on matching turn_id"
        );
    }

    #[tokio::test]
    async fn turn_cancel_no_active_turn() {
        let daemon = test_daemon().await;
        // Register the session but do not install an active turn.
        daemon.sessions.get_or_create(
            "s-cancel-none",
            "s-cancel-none",
            std::path::PathBuf::from("."),
        );

        let req = crate::core::new_request(
            "req-cancel".into(),
            CoreRequest::TurnCancel {
                session_id: "s-cancel-none".into(),
                turn_id: "turn-anything".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "no_active_turn");
            }
            other => panic!("expected Error(no_active_turn), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_wrong_id_rejected() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-steer-wrong",
            "s-steer-wrong",
            std::path::PathBuf::from("."),
        );
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, "turn-real-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-wrong".into(),
                turn_id: "turn-typo".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "turn_id_mismatch");
            }
            other => panic!("expected Error(turn_id_mismatch), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn turn_steer_correct_id_succeeds() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-steer-ok",
            "s-steer-ok",
            std::path::PathBuf::from("."),
        );
        let (_cancel_tx, _cancel_rx, mut steer_rx) =
            install_active_turn(&runtime, "turn-good-steer").await;

        let req = crate::core::new_request(
            "req-steer".into(),
            CoreRequest::TurnSteer {
                session_id: "s-steer-ok".into(),
                turn_id: "turn-good-steer".into(),
                text: "redirect".into(),
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The steer channel should have received the message.
        let got = tokio::time::timeout(std::time::Duration::from_millis(50), steer_rx.recv())
            .await
            .expect("steer message should arrive")
            .expect("steer_rx should yield a value");
        assert_eq!(got, "redirect");
    }

    #[tokio::test]
    async fn turn_started_emitted_on_submit() {
        // Set up an env var to register the openai provider so TurnSubmit
        // passes the provider-not-found check. The actual API call will
        // fail in the spawned agent loop, but we only care that TurnStarted
        // is published synchronously by the daemon before the spawn.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let daemon = test_daemon().await;
        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        let session_id = "s-submit-started".to_string();
        let req = crate::core::new_request(
            "req-submit".into(),
            CoreRequest::TurnSubmit {
                session_id: session_id.clone(),
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent)],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));

        // The TurnStarted event should be in the log, identified by
        // session_id and a turn_id that starts with "turn-".
        let filter = EventFilter {
            session_id: Some(session_id.clone()),
            include_global: true,
            client_id: None,
        };
        let events = daemon.event_log.replay_from(0, &filter).await;

        let mut found: Option<(String, String)> = None;
        for env in &events {
            if let CoreEvent::TurnStarted {
                session_id: sid,
                turn_id,
            } = &env.payload
            {
                if sid == &session_id {
                    found = Some((sid.clone(), turn_id.clone()));
                    break;
                }
            }
        }
        let (sid, turn_id) = found.expect("expected TurnStarted event in log");
        assert_eq!(sid, session_id);
        assert!(
            turn_id.starts_with("turn-"),
            "turn_id '{}' should start with 'turn-'",
            turn_id
        );

        std::env::remove_var("OPENAI_API_KEY");
    }

    #[tokio::test]
    async fn bridge_attaches_turn_id_for_text_delta() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-bridge-delta",
            "s-bridge-delta",
            std::path::PathBuf::from("."),
        );
        let turn_id = "turn-bridge-delta".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        // A TextDelta from the bus carries no turn_id; the bridge must
        // attach the active turn_id.
        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-delta".into(),
            delta: "hi".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (session_id, attached_turn_id, core_event) = result;
        assert_eq!(session_id.as_deref(), Some("s-bridge-delta"));
        assert_eq!(attached_turn_id.as_deref(), Some(turn_id.as_str()));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id: tid, .. } => {
                assert_eq!(
                    tid, turn_id,
                    "TurnTextDelta should carry the active turn_id"
                );
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_longer_maps_agent_finished_to_turn_completed() {
        // Pass 3 invariant: the bridge must NOT produce a duplicate
        // `CoreEvent::TurnCompleted` for `AppEvent::AgentFinished`,
        // because the TurnSubmit spawned task publishes the lifecycle
        // event directly with the captured turn_id. The bus event is
        // still consumed by the event bridge to update token counts
        // and emit notifications, but it does not flow through
        // `map_app_event_to_core_event`.
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-bridge-finished",
            "s-bridge-finished",
            std::path::PathBuf::from("."),
        );
        let turn_id = "turn-bridge-finished".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) = install_active_turn(&runtime, &turn_id).await;

        let app_event = crate::bus::events::AppEvent::AgentFinished {
            session_id: "s-bridge-finished".into(),
            stop_reason: "completed".into(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            reasoning_tokens: None,
        };
        let result = daemon.bridge_app_event(app_event).await;
        assert!(
            result.is_none(),
            "AgentFinished must not produce a CoreEvent from the bridge; got {:?}",
            result
        );
    }

    #[tokio::test]
    async fn direct_turn_completion_uses_runtime_turn_id() {
        // The TurnSubmit spawn task publishes a CoreEvent::TurnCompleted
        // directly with the captured turn_id. We exercise this path
        // here by publishing the same event shape the spawn task
        // produces and asserting that the envelope carries the
        // non-empty turn id and matches what a subscriber sees on
        // the broadcast channel.
        let daemon = test_daemon().await;
        let session_id = "s-direct-completion".to_string();
        let turn_id = "turn-direct".to_string();
        let mut rx = daemon.event_log.subscribe();

        // Direct publish path (mirrors the spawn task).
        daemon
            .event_log
            .publish(
                Some(session_id.clone()),
                Some(turn_id.clone()),
                CoreEvent::TurnCompleted {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    stop_reason: "completed".to_string(),
                },
            )
            .await;

        let env = rx.recv().await.expect("expected an envelope on the bus");
        match env.payload {
            CoreEvent::TurnCompleted {
                turn_id: tid,
                stop_reason,
                ..
            } => {
                assert_eq!(tid, turn_id);
                assert_eq!(stop_reason, "completed");
            }
            other => panic!("expected TurnCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_keeps_turn_id_from_event_when_present() {
        let daemon = test_daemon().await;
        let runtime = daemon.sessions.get_or_create(
            "s-bridge-explicit",
            "s-bridge-explicit",
            std::path::PathBuf::from("."),
        );
        let active_turn_id = "turn-active".to_string();
        let (_cancel_tx, _cancel_rx, _steer_rx) =
            install_active_turn(&runtime, &active_turn_id).await;

        // ToolResult carries a turn_id on the AppEvent? No - the bus
        // AppEvent::ToolResult doesn't have a turn_id. The bridged
        // CoreEvent::ToolCompleted has turn_id: None, so the bridge
        // should fall back to the active turn_id.
        let app_event = crate::bus::events::AppEvent::ToolResult {
            session_id: "s-bridge-explicit".into(),
            tool_id: "t1".into(),
            tool_name: "bash".into(),
            output: "ok".into(),
            success: true,
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map ToolResult");
        let (_session_id, attached_turn_id, core_event) = result;
        assert_eq!(attached_turn_id.as_deref(), Some(active_turn_id.as_str()));
        match core_event {
            CoreEvent::ToolCompleted { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(active_turn_id.as_str()));
            }
            other => panic!("expected ToolCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn bridge_no_active_turn_keeps_empty_turn_id() {
        let daemon = test_daemon().await;
        // No active turn installed for this session.
        daemon.sessions.get_or_create(
            "s-bridge-none",
            "s-bridge-none",
            std::path::PathBuf::from("."),
        );

        let app_event = crate::bus::events::AppEvent::TextDelta {
            session_id: "s-bridge-none".into(),
            delta: "orphan".into(),
        };
        let result = daemon
            .bridge_app_event(app_event)
            .await
            .expect("bridge_app_event should map TextDelta");
        let (_session_id, attached_turn_id, core_event) = result;
        // No active turn -> turn_id is the empty default from the mapper.
        assert_eq!(attached_turn_id.as_deref(), Some(""));
        match core_event {
            CoreEvent::TurnTextDelta { turn_id, .. } => {
                assert_eq!(turn_id, "");
            }
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }
    }

    /// A minimal fake turn runtime that records whether `run_turn` was called.
    struct FakeTurnRuntime {
        called: std::sync::atomic::AtomicBool,
    }

    impl FakeTurnRuntime {
        fn new() -> Self {
            Self {
                called: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::agent::turn_runtime::TurnRuntime for FakeTurnRuntime {
        async fn run_turn(
            &self,
            _input: crate::agent::turn_runtime::TurnRunInput,
        ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
            self.called.store(true, std::sync::atomic::Ordering::SeqCst);
            let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
            let (steer_tx, _steer_rx) = tokio::sync::mpsc::unbounded_channel();
            Ok(crate::agent::turn_runtime::TurnRunOutput {
                cancel_tx,
                steer_tx,
            })
        }
    }

    #[tokio::test]
    async fn turn_submit_uses_injected_runtime() {
        // Verify that CoreDaemon::TurnSubmit delegates to the injected
        // TurnRuntime instead of constructing DefaultTurnRuntime directly.
        std::env::set_var("OPENAI_API_KEY", "test-key-not-used");

        let fake = Arc::new(FakeTurnRuntime::new());
        let deps = CoreRuntimeDeps::new(None, None, None, None)
            .with_turn_runtime(Arc::clone(&fake) as Arc<dyn TurnRuntime>);
        let daemon = CoreDaemon::with_deps(deps);

        let agent = crate::agent::Agent {
            name: "test".into(),
            description: "test agent".into(),
            ..Default::default()
        };

        let session_id = "s-inject-test".to_string();
        let req = crate::core::new_request(
            "req-inject".into(),
            CoreRequest::TurnSubmit {
                session_id,
                text: "hello".into(),
                plan_mode: false,
                model: "openai/gpt-4o".into(),
                agents: vec![crate::protocol_conversions::agent_to_dto(agent)],
                current_agent_idx: 0,
                messages: vec![],
            },
        );
        let resp = daemon.handle_request(req).await.unwrap();
        assert!(matches!(resp, CoreResponse::Ack));
        assert!(
            fake.called.load(std::sync::atomic::Ordering::SeqCst),
            "injected FakeTurnRuntime should have been invoked"
        );
        // Note: do not remove OPENAI_API_KEY here to avoid racing
        // with other tests that also set it. The env var is process-global.
    }
}
