use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::error::AppError;
use crate::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, PROTOCOL_VERSION,
};

pub mod client_registry;
pub mod daemon;
pub mod event_log;
pub mod notification;
pub mod runtime_deps;
pub mod session_runtime;
pub mod transport;

#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;
    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}

/// In-process core client. Now delegates to CoreDaemon.
/// Kept for backward compatibility with embedded/inproc mode.
/// New code should use CoreDaemon directly or socket transport.
#[derive(Clone, Default)]
pub struct InprocCoreClient {
    daemon: Option<Arc<daemon::CoreDaemon>>,
    pub pool: Option<sqlx::SqlitePool>,
}

impl InprocCoreClient {
    /// Construct from a bundled [`runtime_deps::CoreRuntimeDeps`].
    pub fn with_deps(
        deps: runtime_deps::CoreRuntimeDeps,
        _config: crate::config::schema::Config,
    ) -> Self {
        let pool = deps.pool.clone();
        let daemon = Arc::new(daemon::CoreDaemon::with_deps(deps));
        Self {
            daemon: Some(daemon),
            pool,
        }
    }

    /// Legacy constructor for backward compatibility. Prefer `with_deps`.
    pub fn new(
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
        pool: Option<sqlx::SqlitePool>,
        config: crate::config::schema::Config,
    ) -> Self {
        Self::with_deps(
            runtime_deps::CoreRuntimeDeps::new(
                pool.clone(),
                subagent_pool,
                memory_store,
                bg_scheduler,
            ),
            config,
        )
    }
}

/// Publish a `GoalUpdated` bus event so the TUI (and any remote
/// subscribers) can reflect the latest goal state. Always pair with a
/// successful goal store write.
fn publish_goal_updated(session_id: &str, goal: Option<crate::goal::model::Goal>) {
    let snap = goal.map(|g| g.to_snapshot());
    crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::GoalUpdated {
        session_id: session_id.to_string(),
        goal: Box::new(snap),
    });
}

#[async_trait]
impl CoreClient for InprocCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match &self.daemon {
            Some(daemon) => daemon.handle_request(request).await,
            None => Ok(CoreResponse::Error {
                code: "not_initialized".to_string(),
                message: "CoreDaemon not initialized".to_string(),
            }),
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();

        if let Some(ref daemon) = self.daemon {
            let mut event_rx = daemon.event_log.subscribe();
            tokio::spawn(async move {
                while let Ok(event) = event_rx.recv().await {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            });
        } else {
            tokio::spawn(async move {
                let mut bus_rx = crate::bus::global::GlobalEventBus::subscribe();
                let mut seq: u64 = 1;
                loop {
                    match bus_rx.recv().await {
                        Ok(event) => {
                            if let Some(core_event) = map_app_event_to_core_event(event) {
                                let (env_session_id, env_turn_id) = match &core_event {
                                    CoreEvent::PermissionPending {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), turn_id.clone()),
                                    CoreEvent::QuestionPending {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), turn_id.clone()),
                                    CoreEvent::TurnStarted {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), Some(turn_id.clone())),
                                    CoreEvent::TurnTextDelta {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), Some(turn_id.clone())),
                                    CoreEvent::TurnReasoningDelta {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), Some(turn_id.clone())),
                                    CoreEvent::ToolStarted {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), turn_id.clone()),
                                    CoreEvent::ToolCompleted {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), turn_id.clone()),
                                    CoreEvent::TurnCompleted {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), Some(turn_id.clone())),
                                    CoreEvent::TurnFailed {
                                        session_id,
                                        turn_id,
                                        ..
                                    } => (Some(session_id.clone()), turn_id.clone()),
                                    _ => (None, None),
                                };
                                let envelope = EventEnvelope {
                                    protocol_version: PROTOCOL_VERSION,
                                    event_seq: seq,
                                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                                    session_id: env_session_id,
                                    turn_id: env_turn_id,
                                    payload: core_event,
                                };
                                seq = seq.saturating_add(1);
                                if tx.send(envelope).is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Core event bus lagged, {} events dropped", n);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            });
        }

        rx
    }
}

pub(crate) fn core_event_metadata(
    event: &crate::protocol::core::CoreEvent,
) -> (Option<String>, Option<String>) {
    use crate::protocol::core::CoreEvent;
    match event {
        CoreEvent::TurnStarted {
            session_id,
            turn_id,
        } => (Some(session_id.clone()), Some(turn_id.clone())),
        CoreEvent::TurnTextDelta {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), Some(turn_id.clone())),
        CoreEvent::TurnReasoningDelta {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), Some(turn_id.clone())),
        CoreEvent::ToolStarted {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), turn_id.clone()),
        CoreEvent::ToolCompleted {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), turn_id.clone()),
        CoreEvent::PermissionPending {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), turn_id.clone()),
        CoreEvent::QuestionPending {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), turn_id.clone()),
        CoreEvent::TurnCompleted {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), Some(turn_id.clone())),
        CoreEvent::TurnFailed {
            session_id,
            turn_id,
            ..
        } => (Some(session_id.clone()), turn_id.clone()),
        CoreEvent::SessionUpdated { session_id } => (Some(session_id.clone()), None),
        CoreEvent::SubagentStarted { session_id, .. }
        | CoreEvent::SubagentProgress { session_id, .. }
        | CoreEvent::SubagentCompleted { session_id, .. }
        | CoreEvent::SubagentFailed { session_id, .. } => (Some(session_id.clone()), None),
        _ => (None, None),
    }
}

/// Re-construct a `CoreEvent` with a new `turn_id`. Variants that carry
/// `turn_id` as a `String` (e.g. `TurnTextDelta`) are immutable after
/// `map_app_event_to_core_event` returns, so the bridge has to rebuild
/// them here when it discovers the active `turn_id` from the runtime.
/// Variants that carry `turn_id: Option<String>` are reconstructed by
/// taking the new value (overwriting the prior `None` or empty). All
/// other variants are returned unchanged.
pub(crate) fn set_turn_id_on_event(
    event: crate::protocol::core::CoreEvent,
    turn_id: String,
) -> crate::protocol::core::CoreEvent {
    use crate::protocol::core::CoreEvent;
    match event {
        CoreEvent::TurnStarted { session_id, .. } => CoreEvent::TurnStarted {
            session_id,
            turn_id,
        },
        CoreEvent::TurnTextDelta {
            session_id, delta, ..
        } => CoreEvent::TurnTextDelta {
            session_id,
            turn_id,
            delta,
        },
        CoreEvent::TurnReasoningDelta {
            session_id, delta, ..
        } => CoreEvent::TurnReasoningDelta {
            session_id,
            turn_id,
            delta,
        },
        CoreEvent::TurnCompleted {
            session_id,
            stop_reason,
            ..
        } => CoreEvent::TurnCompleted {
            session_id,
            turn_id,
            stop_reason,
        },
        CoreEvent::TurnFailed {
            session_id,
            message,
            ..
        } => CoreEvent::TurnFailed {
            session_id,
            turn_id: Some(turn_id),
            message,
        },
        CoreEvent::ToolStarted {
            session_id,
            tool_name,
            tool_id,
            arguments,
            ..
        } => CoreEvent::ToolStarted {
            session_id,
            turn_id: Some(turn_id),
            tool_name,
            tool_id,
            arguments,
        },
        CoreEvent::ToolCompleted {
            session_id,
            tool_id,
            output,
            success,
            ..
        } => CoreEvent::ToolCompleted {
            session_id,
            turn_id: Some(turn_id),
            tool_id,
            output,
            success,
        },
        other => other,
    }
}

pub fn core_event_type(event: &crate::protocol::core::CoreEvent) -> &'static str {
    use crate::protocol::core::CoreEvent;
    match event {
        CoreEvent::TurnStarted { .. } => "turn_started",
        CoreEvent::TurnCompleted { .. } => "turn_completed",
        CoreEvent::TurnFailed { .. } => "turn_failed",
        CoreEvent::ToolStarted { .. } => "tool_started",
        CoreEvent::ToolCompleted { .. } => "tool_completed",
        CoreEvent::PermissionPending { .. } => "permission_pending",
        CoreEvent::QuestionPending { .. } => "question_pending",
        CoreEvent::SessionUpdated { .. } => "session_updated",
        CoreEvent::SubagentStarted { .. } => "subagent_started",
        CoreEvent::SubagentCompleted { .. } => "subagent_completed",
        CoreEvent::SubagentFailed { .. } => "subagent_failed",
        CoreEvent::Error { .. } => "error",
        _ => "other",
    }
}

pub(crate) fn map_app_event_to_core_event(
    event: crate::bus::events::AppEvent,
) -> Option<CoreEvent> {
    match event {
        crate::bus::events::AppEvent::TextDelta { delta, session_id } => {
            Some(CoreEvent::TurnTextDelta {
                session_id: session_id.to_string(),
                turn_id: String::new(),
                delta: delta.to_string(),
            })
        }
        crate::bus::events::AppEvent::ReasoningDelta { delta, session_id } => {
            Some(CoreEvent::TurnReasoningDelta {
                session_id: session_id.to_string(),
                turn_id: String::new(),
                delta,
            })
        }
        crate::bus::events::AppEvent::ToolCallStarted {
            session_id,
            tool_name,
            tool_id,
            arguments,
        } => Some(CoreEvent::ToolStarted {
            session_id,
            turn_id: None,
            tool_name,
            tool_id,
            arguments,
        }),
        crate::bus::events::AppEvent::ToolResult {
            session_id,
            tool_id,
            output,
            success,
            ..
        } => Some(CoreEvent::ToolCompleted {
            session_id,
            turn_id: None,
            tool_id,
            output,
            success,
        }),
        crate::bus::events::AppEvent::PermissionPending {
            perm_id,
            session_id,
            turn_id,
            tool,
            path,
            ..
        } => Some(CoreEvent::PermissionPending {
            id: format!(
                "perm:{}:{}:{}",
                session_id,
                turn_id.as_deref().unwrap_or(""),
                perm_id
            ),
            session_id,
            turn_id,
            tool,
            path,
        }),
        crate::bus::events::AppEvent::QuestionPending {
            session_id,
            question_id,
            turn_id,
            questions,
        } => Some(CoreEvent::QuestionPending {
            id: format!(
                "question:{}:{}:{}",
                session_id,
                turn_id.as_deref().unwrap_or(""),
                question_id
            ),
            session_id,
            turn_id,
            questions: serde_json::from_str(&questions).unwrap_or(serde_json::Value::Null),
        }),
        // AgentFinished is intentionally NOT mapped to a CoreEvent here.
        // The TurnSubmit spawned task publishes `CoreEvent::TurnCompleted`
        // or `CoreEvent::TurnFailed` directly with the captured turn_id,
        // so the bridge must not produce a second lifecycle event. The
        // bus event is still used (via `start_event_bridge`) to update
        // runtime token counts and to emit a notification, neither of
        // which goes through `map_app_event_to_core_event`.
        crate::bus::events::AppEvent::AgentFinished { .. } => None,
        crate::bus::events::AppEvent::Error { message } => Some(CoreEvent::Error {
            code: "agent_error".to_string(),
            message,
        }),
        crate::bus::events::AppEvent::SubagentStarted {
            session_id,
            task_id,
            agent,
            description,
        } => Some(CoreEvent::SubagentStarted {
            session_id,
            task_id,
            agent,
            description,
        }),
        crate::bus::events::AppEvent::SubagentProgress {
            session_id,
            task_id,
            agent,
            message,
        } => Some(CoreEvent::SubagentProgress {
            session_id,
            task_id,
            agent,
            message,
        }),
        crate::bus::events::AppEvent::SubagentCompleted {
            session_id,
            task_id,
            agent,
            result_summary,
        } => Some(CoreEvent::SubagentCompleted {
            session_id,
            task_id,
            agent,
            result_summary,
        }),
        crate::bus::events::AppEvent::SubagentFailed {
            session_id,
            task_id,
            agent,
            error,
        } => Some(CoreEvent::SubagentFailed {
            session_id,
            task_id,
            agent,
            error,
        }),
        crate::bus::events::AppEvent::SessionCreated { id, .. } => {
            Some(CoreEvent::SessionUpdated { session_id: id })
        }
        crate::bus::events::AppEvent::SessionUpdated { id } => {
            Some(CoreEvent::SessionUpdated { session_id: id })
        }
        crate::bus::events::AppEvent::FileChanged { path, action, .. } => {
            Some(CoreEvent::FileChanged { path, action })
        }
        _ => None,
    }
}

pub fn new_request<T>(request_id: String, payload: T) -> RequestEnvelope<T> {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema::migrate;

    fn test_config() -> crate::config::schema::Config {
        crate::config::schema::Config::load().unwrap_or_default()
    }

    async fn test_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::SqlitePoolOptions;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        Box::leak(Box::new(dir));
        pool
    }

    #[tokio::test]
    async fn session_create_returns_session() {
        let pool = test_pool().await;
        let client = InprocCoreClient::new(None, None, None, Some(pool), test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/test".into(),
                title: Some("Test Session".into()),
            },
        );
        let resp = client.request(req).await.unwrap();
        assert!(
            matches!(resp, CoreResponse::Session { .. }),
            "expected Session, got {:?}",
            resp
        );
    }

    #[tokio::test]
    async fn session_load_existing() {
        let pool = test_pool().await;
        let client = InprocCoreClient::new(None, None, None, Some(pool.clone()), test_config());

        // Create a session first
        let create_req = new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/test".into(),
                title: Some("Load Me".into()),
            },
        );
        let session_id = match client.request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        // Load it back
        let load_req = new_request(
            "req-2".into(),
            CoreRequest::SessionLoad {
                session_id: session_id.clone(),
            },
        );
        let resp = client.request(load_req).await.unwrap();
        match resp {
            CoreResponse::Session { session } => assert_eq!(session.id, session_id),
            other => panic!("expected Session, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn session_load_not_found() {
        let pool = test_pool().await;
        let client = InprocCoreClient::new(None, None, None, Some(pool), test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::SessionLoad {
                session_id: "nonexistent".into(),
            },
        );
        let resp = client.request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => assert_eq!(code, "session_not_found"),
            other => panic!("expected Error(session_not_found), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn session_messages_load_empty() {
        let pool = test_pool().await;
        let client = InprocCoreClient::new(None, None, None, Some(pool.clone()), test_config());

        // Create a session
        let create_req = new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp/test".into(),
                title: Some("Msg Test".into()),
            },
        );
        let session_id = match client.request(create_req).await.unwrap() {
            CoreResponse::Session { session } => session.id,
            other => panic!("expected Session, got {:?}", other),
        };

        // Load messages
        let msg_req = new_request(
            "req-2".into(),
            CoreRequest::SessionMessagesLoad {
                session_id: session_id.clone(),
            },
        );
        let resp = client.request(msg_req).await.unwrap();
        match resp {
            CoreResponse::SessionMessages {
                session_id: sid,
                messages,
            } => {
                assert_eq!(sid, session_id);
                assert!(messages.is_empty());
            }
            other => panic!("expected SessionMessages, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_no_pending() {
        let client = InprocCoreClient::new(None, None, None, None, test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::PermissionRespond {
                id: "perm:nonexistent-session:turn-1:perm-1".into(),
                choice: "allow".into(),
            },
        );
        let resp = client.request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => assert_eq!(code, "permission_response_failed"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_respond_invalid_choice() {
        let client = InprocCoreClient::new(None, None, None, None, test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::PermissionRespond {
                id: "perm-1".into(),
                choice: "bogus".into(),
            },
        );
        let resp = client.request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => assert_eq!(code, "invalid_permission_choice"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn question_respond_no_pending() {
        let client = InprocCoreClient::new(None, None, None, None, test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::QuestionRespond {
                id: "question:nonexistent-session:turn-1:q-1".into(),
                answers: serde_json::json!("yes"),
            },
        );
        let resp = client.request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => assert_eq!(code, "question_response_failed"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn missing_pool_returns_error() {
        let client = InprocCoreClient::new(None, None, None, None, test_config());
        let req = new_request(
            "req-1".into(),
            CoreRequest::SessionCreate {
                directory: "/tmp".into(),
                title: None,
            },
        );
        let resp = client.request(req).await.unwrap();
        match resp {
            CoreResponse::Error { code, .. } => assert_eq!(code, "missing_pool"),
            other => panic!("expected Error(missing_pool), got {:?}", other),
        }
    }

    #[test]
    fn set_turn_id_on_event_replaces_empty_string_variants() {
        use crate::protocol::core::CoreEvent;
        let new_turn = "turn-fresh".to_string();

        // TurnTextDelta with an empty turn_id should be rewritten.
        let ev = CoreEvent::TurnTextDelta {
            session_id: "s1".into(),
            turn_id: String::new(),
            delta: "hi".into(),
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::TurnTextDelta { turn_id, .. } => assert_eq!(turn_id, new_turn),
            other => panic!("expected TurnTextDelta, got {:?}", other),
        }

        // TurnReasoningDelta
        let ev = CoreEvent::TurnReasoningDelta {
            session_id: "s1".into(),
            turn_id: String::new(),
            delta: "hmm".into(),
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::TurnReasoningDelta { turn_id, .. } => assert_eq!(turn_id, new_turn),
            other => panic!("expected TurnReasoningDelta, got {:?}", other),
        }

        // TurnCompleted
        let ev = CoreEvent::TurnCompleted {
            session_id: "s1".into(),
            turn_id: String::new(),
            stop_reason: "ok".into(),
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::TurnCompleted { turn_id, .. } => assert_eq!(turn_id, new_turn),
            other => panic!("expected TurnCompleted, got {:?}", other),
        }
    }

    #[test]
    fn set_turn_id_on_event_replaces_option_variants() {
        use crate::protocol::core::CoreEvent;
        let new_turn = "turn-fresh".to_string();

        // TurnFailed with None
        let ev = CoreEvent::TurnFailed {
            session_id: "s1".into(),
            turn_id: None,
            message: "boom".into(),
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::TurnFailed { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(new_turn.as_str()));
            }
            other => panic!("expected TurnFailed, got {:?}", other),
        }

        // ToolStarted
        let ev = CoreEvent::ToolStarted {
            session_id: "s1".into(),
            turn_id: None,
            tool_name: "bash".into(),
            tool_id: "t1".into(),
            arguments: "[]".into(),
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::ToolStarted { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(new_turn.as_str()));
            }
            other => panic!("expected ToolStarted, got {:?}", other),
        }

        // ToolCompleted
        let ev = CoreEvent::ToolCompleted {
            session_id: "s1".into(),
            turn_id: None,
            tool_id: "t1".into(),
            output: "ok".into(),
            success: true,
        };
        let out = set_turn_id_on_event(ev, new_turn.clone());
        match out {
            CoreEvent::ToolCompleted { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some(new_turn.as_str()));
            }
            other => panic!("expected ToolCompleted, got {:?}", other),
        }
    }

    #[test]
    fn set_turn_id_on_event_passes_through_unrelated_variants() {
        use crate::protocol::core::CoreEvent;
        // SessionUpdated has no turn_id and should be returned unchanged.
        let ev = CoreEvent::SessionUpdated {
            session_id: "s1".into(),
        };
        let out = set_turn_id_on_event(ev.clone(), "turn-x".into());
        match out {
            CoreEvent::SessionUpdated { session_id } => assert_eq!(session_id, "s1"),
            other => panic!("expected SessionUpdated, got {:?}", other),
        }

        // Error variant has no turn_id.
        let ev = CoreEvent::Error {
            code: "x".into(),
            message: "y".into(),
        };
        let out = set_turn_id_on_event(ev.clone(), "turn-x".into());
        match out {
            CoreEvent::Error { code, message } => {
                assert_eq!(code, "x");
                assert_eq!(message, "y");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }
}
