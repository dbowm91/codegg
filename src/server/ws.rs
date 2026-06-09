use std::sync::Arc;
use tokio::sync::Mutex;

use axum::{
    extract::{ws::WebSocket, ConnectInfo, FromRequestParts, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tracing::info;

use crate::error::ServerRuntimeError;
use crate::protocol::core::CoreRequest;
use crate::protocol::frames::CoreFrame;
use crate::protocol::tui::TuiMessage;
use crate::server::rpc::{RpcError, RpcRequest, RpcResponse};

#[derive(Clone, Debug)]
pub struct WebSocketAuth {
    pub authorization: Option<String>,
}

impl<S> FromRequestParts<S> for WebSocketAuth
where
    S: Send + Sync,
{
    type Rejection = ServerRuntimeError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let authorization = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());

        Ok(WebSocketAuth { authorization })
    }
}

fn validate_ws_auth(auth: &WebSocketAuth) -> Result<(), StatusCode> {
    let auth_required = std::env::var("CODEGG_SERVER_AUTH_DISABLED").is_err();

    if !auth_required {
        return Ok(());
    }

    let client_token = auth
        .authorization
        .as_ref()
        .and_then(|v| v.strip_prefix("Bearer ").map(|t| t.to_string()));

    let expected = std::env::var("CODEGG_SERVER_TOKEN").ok();

    match expected {
        Some(expected_token) => {
            let valid = client_token
                .as_ref()
                .map(|t| t.as_bytes().ct_eq(expected_token.as_bytes()).unwrap_u8() == 1)
                .unwrap_or(false);

            if !valid {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        None => {}
    }

    Ok(())
}

pub async fn handle_ws(
    ws: WebSocketUpgrade,
    State(state): State<crate::server::state::ServerState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    auth: WebSocketAuth,
) -> impl axum::response::IntoResponse {
    if let Err(res) = validate_ws_auth(&auth) {
        return res.into_response();
    }

    ws.on_upgrade(move |socket| async move {
        upgrade_ws(socket, state, addr).await;
    })
}

async fn upgrade_ws(
    socket: WebSocket,
    state: crate::server::state::ServerState,
    addr: std::net::SocketAddr,
) {
    let (ws_tx, ws_rx) = socket.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();

    let state_clone = state.clone();
    let mut send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        while let Some(msg) = out_rx.recv().await {
            let _ = ws_tx.send(msg).await;
        }
        drop(state_clone);
    });

    let rate_limiter = state.ws_rate_limiter.clone();

    let mut recv_task = tokio::spawn(async move {
        let mut ws_rx = ws_rx;
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                let key = addr.to_string();
                if !rate_limiter.check_rate_limit(&key).await {
                    let resp = RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: serde_json::Value::Null,
                        result: None,
                        error: Some(RpcError {
                            code: 429,
                            message: "Too Many Requests".to_string(),
                        }),
                    };
                    if let Ok(msg) = serde_json::to_string(&resp) {
                        let _ = out_tx.send(axum::extract::ws::Message::Text(msg.into()));
                    }
                    break;
                }
                if let Ok(req) = serde_json::from_str::<RpcRequest>(&text) {
                    let resp = handle_rpc_request(&req, &state).await;
                    if let Ok(msg) = serde_json::to_string(&resp) {
                        let _ = out_tx.send(axum::extract::ws::Message::Text(msg.into()));
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    info!("WebSocket connection closed");
}

/// Legacy JSON-RPC handler for /ws endpoint.
/// Delegates to CoreDaemon when available, falls back to direct DB access.
async fn handle_rpc_request(
    req: &RpcRequest,
    state: &crate::server::state::ServerState,
) -> RpcResponse {
    tracing::warn!("Legacy /ws RPC endpoint used - consider migrating to /core CoreFrame protocol");

    // Delegate to CoreDaemon when available
    if let Some(ref daemon) = state.daemon {
        let core_request = match req.method.as_str() {
            "sessions.list" => {
                let params = if let serde_json::Value::Object(ref p) = req.params {
                    p
                } else {
                    return RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id.clone(),
                        result: None,
                        error: Some(RpcError {
                            code: -32602,
                            message: "Invalid params".to_string(),
                        }),
                    };
                };
                let project_id = params
                    .get("project_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let show_archived = params
                    .get("show_archived")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                CoreRequest::SessionList {
                    project_id,
                    show_archived,
                    limit,
                }
            }
            "sessions.get" => {
                let id = if let serde_json::Value::Object(ref p) = req.params {
                    p.get("id").and_then(|v| v.as_str()).unwrap_or("")
                } else {
                    ""
                };
                CoreRequest::SessionLoad {
                    session_id: id.to_string(),
                }
            }
            "sessions.create" => {
                let dir = if let serde_json::Value::Object(ref p) = req.params {
                    p.get("directory")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&state.project_dir)
                } else {
                    &state.project_dir
                };
                CoreRequest::SessionCreate {
                    directory: dir.to_string(),
                    title: None,
                }
            }
            "providers.list" | "tools.list" => {
                // These don't map to CoreRequest; handle directly via daemon's DB pool
                return handle_rpc_direct(req, state).await;
            }
            _ => {
                return RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: None,
                    error: Some(RpcError {
                        code: -32601,
                        message: format!("Method not found: {}", req.method),
                    }),
                };
            }
        };

        let envelope =
            crate::core::new_request(req.id.as_str().unwrap_or("").to_string(), core_request);

        match daemon.handle_request(envelope).await {
            Ok(response) => {
                // Convert CoreResponse back to RpcResponse JSON
                let result = match response {
                    crate::protocol::core::CoreResponse::SessionList { sessions } => {
                        let data: Vec<_> = sessions
                            .into_iter()
                            .map(|s| {
                                serde_json::json!({
                                    "id": s.id,
                                    "title": s.title,
                                    "created": s.time_created,
                                    "updated": s.time_updated,
                                })
                            })
                            .collect();
                        Some(serde_json::json!({"sessions": data}))
                    }
                    crate::protocol::core::CoreResponse::Session { session } => {
                        Some(serde_json::json!({
                            "id": session.id,
                            "title": session.title,
                            "project_id": session.project_id,
                            "directory": session.directory,
                            "created": session.time_created,
                            "updated": session.time_updated,
                        }))
                    }
                    crate::protocol::core::CoreResponse::Ack => Some(serde_json::json!({})),
                    crate::protocol::core::CoreResponse::Error { code, message } => {
                        return RpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id.clone(),
                            result: None,
                            error: Some(RpcError {
                                code: -32603,
                                message: format!("{}: {}", code, message),
                            }),
                        };
                    }
                    _ => Some(serde_json::to_value(&response).unwrap_or_default()),
                };
                RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result,
                    error: None,
                }
            }
            Err(e) => RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id.clone(),
                result: None,
                error: Some(RpcError {
                    code: -32000,
                    message: e.to_string(),
                }),
            },
        }
    } else {
        // Legacy direct DB access (deprecated)
        handle_rpc_direct(req, state).await
    }
}

/// Legacy direct DB access handler. Used as fallback when no CoreDaemon is available,
/// or for methods not yet routed through CoreDaemon (providers.list, tools.list).
async fn handle_rpc_direct(
    req: &RpcRequest,
    state: &crate::server::state::ServerState,
) -> RpcResponse {
    match req.method.as_str() {
        "sessions.list" => {
            let store = crate::session::SessionStore::new(state.pool.clone());
            match store.list(&state.project_dir, 50).await {
                Ok(sessions) => {
                    let data: Vec<_> = sessions
                        .into_iter()
                        .map(|s| {
                            serde_json::json!({
                                "id": s.id,
                                "title": s.title,
                                "created": s.time_created,
                                "updated": s.time_updated,
                            })
                        })
                        .collect();
                    RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id.clone(),
                        result: Some(serde_json::json!({"sessions": data})),
                        error: None,
                    }
                }
                Err(e) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: None,
                    error: Some(RpcError {
                        code: -32603,
                        message: e.to_string(),
                    }),
                },
            }
        }
        "sessions.get" => {
            let id = if let serde_json::Value::Object(ref p) = req.params {
                p.get("id").and_then(|v| v.as_str()).unwrap_or("")
            } else {
                ""
            };
            let store = crate::session::SessionStore::new(state.pool.clone());
            match store.get(id).await {
                Ok(Some(s)) => {
                    if s.project_id != state.project_dir {
                        return RpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id.clone(),
                            result: None,
                            error: Some(RpcError {
                                code: -32602,
                                message: "Session not found".into(),
                            }),
                        };
                    }
                    RpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id.clone(),
                        result: Some(serde_json::json!({
                            "id": s.id,
                            "title": s.title,
                            "project_id": s.project_id,
                            "directory": s.directory,
                            "created": s.time_created,
                            "updated": s.time_updated,
                        })),
                        error: None,
                    }
                }
                Ok(None) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: None,
                    error: Some(RpcError {
                        code: -32602,
                        message: "Session not found".into(),
                    }),
                },
                Err(e) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: None,
                    error: Some(RpcError {
                        code: -32603,
                        message: e.to_string(),
                    }),
                },
            }
        }
        "sessions.create" => {
            let dir = if let serde_json::Value::Object(ref p) = req.params {
                p.get("directory")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&state.project_dir)
            } else {
                &state.project_dir
            };
            let store = crate::session::SessionStore::new(state.pool.clone());
            let input = crate::session::CreateSession {
                project_id: state.project_dir.clone(),
                directory: dir.to_string(),
                title: None,
                parent_id: None,
                workspace_id: None,
                agent: None,
                model: None,
                tags: None,
            };
            match store.create(input).await {
                Ok(s) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: Some(serde_json::json!({
                        "id": s.id,
                        "title": s.title,
                    })),
                    error: None,
                },
                Err(e) => RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id.clone(),
                    result: None,
                    error: Some(RpcError {
                        code: -32603,
                        message: e.to_string(),
                    }),
                },
            }
        }
        "providers.list" => {
            let mut registry = crate::provider::ProviderRegistry::new();
            crate::provider::register_builtin(&mut registry);
            let providers: Vec<_> = registry
                .list()
                .into_iter()
                .map(|p| serde_json::json!({"id": p.id(), "name": p.name()}))
                .collect();
            RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id.clone(),
                result: Some(serde_json::json!({"providers": providers})),
                error: None,
            }
        }
        "tools.list" => {
            let registry = crate::tool::ToolRegistry::default();
            let tools: Vec<_> = registry
                .list()
                .into_iter()
                .map(|t| serde_json::json!({"name": t.name(), "description": t.description()}))
                .collect();
            RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id.clone(),
                result: Some(serde_json::json!({"tools": tools})),
                error: None,
            }
        }
        _ => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id.clone(),
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
            }),
        },
    }
}

pub async fn handle_tui(
    ws: WebSocketUpgrade,
    State(state): State<crate::server::state::ServerState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    auth: WebSocketAuth,
) -> impl axum::response::IntoResponse {
    if let Err(res) = validate_ws_auth(&auth) {
        return res.into_response();
    }

    ws.on_upgrade(move |socket| async move {
        upgrade_tui(socket, state, addr).await;
    })
}

async fn upgrade_tui(
    socket: WebSocket,
    state: crate::server::state::ServerState,
    addr: std::net::SocketAddr,
) {
    let (ws_tx, ws_rx) = socket.split();

    let (_out_tx, mut out_rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();
    let (bus_tx, mut bus_rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();

    let bus_tx_clone = bus_tx.clone();
    let bus_tx_clone2 = bus_tx.clone();
    let bus_tx_clone3 = bus_tx.clone();
    let daemon_clone = state.daemon.clone();

    let mut send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        loop {
            tokio::select! {
                Some(msg) = out_rx.recv() => {
                    let _ = ws_tx.send(msg).await;
                }
                Some(event) = bus_rx.recv() => {
                    let _ = ws_tx.send(event).await;
                }
            }
        }
    });

    let rate_limiter = state.ws_rate_limiter.clone();

    let session_state = Arc::new(Mutex::new(TuiSessionState::new(addr.to_string())));

    let mut recv_task = tokio::spawn(async move {
        let mut ws_rx = ws_rx;
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                let key = {
                    let session = session_state.lock().await;
                    session.rate_limit_key.clone()
                };
                if !rate_limiter.check_rate_limit(&key).await {
                    let err = TuiMessage::Error {
                        message: "Too Many Requests".to_string(),
                    };
                    if let Ok(msg) = serde_json::to_string(&err) {
                        let _ = bus_tx_clone.send(axum::extract::ws::Message::Text(msg.into()));
                    }
                    break;
                }

                if let Ok(tui_msg) = serde_json::from_str::<TuiMessage>(&text) {
                    let sess_state = Arc::clone(&session_state);
                    let bus = bus_tx_clone2.clone();
                    handle_tui_message(tui_msg, &sess_state, &bus, &state).await;
                }
            }
        }
    });

    let event_task = tokio::spawn(async move {
        let Some(daemon) = daemon_clone else {
            tracing::warn!("No CoreDaemon available for /tui event task; live events disabled");
            return;
        };
        let mut event_rx = daemon.subscribe();
        loop {
            match event_rx.recv().await {
                Ok(envelope) => {
                    if let Some(tui_msg) = convert_core_event_to_tui(envelope.payload) {
                        let wire = TuiMessage::EventEnvelope {
                            event_seq: envelope.event_seq,
                            payload: Box::new(tui_msg),
                        };
                        if let Ok(json) = serde_json::to_string(&wire) {
                            if bus_tx_clone3
                                .send(axum::extract::ws::Message::Text(json.into()))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Event log receiver lagged, sending resync");
                    let resync_msg = TuiMessage::ResyncRequired {
                        reason: Some("lagged".to_string()),
                        pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(
                        ),
                        pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
                    };
                    if let Ok(json) = serde_json::to_string(&resync_msg) {
                        let _ = bus_tx_clone3.send(axum::extract::ws::Message::Text(json.into()));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
            event_task.abort();
        }
        _ = (&mut recv_task) => {
            send_task.abort();
            event_task.abort();
        }
    }

    info!("TUI WebSocket connection closed");
}

#[derive(Clone)]
struct TuiSessionState {
    session_id: Option<String>,
    model: Option<String>,
    rate_limit_key: String,
}

impl TuiSessionState {
    fn new(rate_limit_key: String) -> Self {
        Self {
            session_id: None,
            model: None,
            rate_limit_key,
        }
    }
}

async fn handle_tui_message(
    msg: TuiMessage,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::UnboundedSender<axum::extract::ws::Message>,
    _server_state: &crate::server::state::ServerState,
) {
    match msg {
        TuiMessage::Input { text } => {
            let sid = state.lock().await.clone();
            if let Some(session_id) = sid.session_id {
                tracing::debug!("Input for session {}: {}", session_id, text);
            }
        }
        TuiMessage::KeyDown { key, modifiers } => {
            let sid = state.lock().await.clone();
            if let Some(session_id) = sid.session_id {
                tracing::debug!(
                    "KeyDown for session {}: {} {:?}",
                    session_id,
                    key,
                    modifiers
                );
            }
        }
        TuiMessage::MouseClick { x, y } => {
            let sid = state.lock().await.clone();
            if let Some(session_id) = sid.session_id {
                tracing::debug!("MouseClick for session {}: {},{}", session_id, x, y);
            }
        }
        TuiMessage::Resize { w, h } => {
            let sid = state.lock().await.clone();
            if let Some(session_id) = sid.session_id {
                tracing::debug!("Resize for session {}: {}x{}", session_id, w, h);
            }
        }
        TuiMessage::Resume { from_event_seq } => {
            tracing::debug!("TUI resume requested from event seq {}", from_event_seq);
            if let Some(ref daemon) = _server_state.daemon {
                let filter = crate::core::event_log::EventFilter {
                    session_id: None,
                    client_id: None,
                    include_global: true,
                };
                let events = daemon.replay_from(from_event_seq, &filter).await;
                for event in events {
                    if let Some(tui_msg) = convert_core_event_to_tui(event.payload) {
                        let envelope = TuiMessage::EventEnvelope {
                            event_seq: event.event_seq,
                            payload: Box::new(tui_msg),
                        };
                        if let Ok(json) = serde_json::to_string(&envelope) {
                            let _ = bus_tx.send(axum::extract::ws::Message::Text(json.into()));
                        }
                    }
                }
            } else {
                let resync_msg = TuiMessage::ResyncRequired {
                    reason: Some("no_daemon".to_string()),
                    pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
                    pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
                };
                if let Ok(json) = serde_json::to_string(&resync_msg) {
                    let _ = bus_tx.send(axum::extract::ws::Message::Text(json.into()));
                }
                return;
            }
            let resync_msg = TuiMessage::ResyncRequired {
                reason: Some("resume_requested".to_string()),
                pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
                pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
            };
            if let Ok(json) = serde_json::to_string(&resync_msg) {
                let _ = bus_tx.send(axum::extract::ws::Message::Text(json.into()));
            }
        }
        TuiMessage::PermissionResponse { id, choice } => {
            let perm_choice = match choice.as_str() {
                "allow" => crate::bus::PermissionDecision::AllowOnce,
                "deny" => crate::bus::PermissionDecision::DenyOnce,
                "always_allow" => crate::bus::PermissionDecision::AlwaysAllow,
                "always_deny" => crate::bus::PermissionDecision::AlwaysDeny,
                _ => crate::bus::PermissionDecision::DenyOnce,
            };
            let id = id.clone();
            tokio::spawn(async move {
                let _ = crate::bus::PermissionRegistry::respond(id, perm_choice);
            });
        }
        TuiMessage::QuestionResponse { id, answers } => {
            let id = id.clone();
            let answers_value = answers.clone();
            tokio::spawn(async move {
                // Normalize answers to consistent JSON string format
                let answers_json = match serde_json::to_string(&answers_value) {
                    Ok(json) => json,
                    Err(_) => return,
                };
                let _ = crate::bus::QuestionRegistry::answer_question(id, answers_json);
            });
        }
        TuiMessage::SessionInfo { id, model } => {
            let mut state_guard = state.lock().await;
            state_guard.session_id = Some(id.clone());
            state_guard.model = Some(model);
            state_guard.rate_limit_key = if id.is_empty() {
                "session:unknown".to_string()
            } else {
                format!("session:{}", id)
            };
        }
        _ => {}
    }
}

/// Convert a CoreEvent back to a TuiMessage for legacy /tui clients replaying from EventLog.
fn convert_core_event_to_tui(event: crate::protocol::core::CoreEvent) -> Option<TuiMessage> {
    use crate::protocol::core::CoreEvent;
    match event {
        CoreEvent::TurnTextDelta { delta, .. } => Some(TuiMessage::TextDelta { delta }),
        CoreEvent::ToolStarted {
            tool_name,
            tool_id,
            arguments,
            ..
        } => Some(TuiMessage::ToolCallStarted {
            tool_name,
            tool_id,
            arguments,
        }),
        CoreEvent::ToolCompleted {
            tool_id,
            output,
            success,
            ..
        } => Some(TuiMessage::ToolResult {
            tool_id,
            output,
            success,
        }),
        CoreEvent::PermissionPending { id, tool, path, .. } => {
            Some(TuiMessage::PermissionPending { id, tool, path })
        }
        CoreEvent::QuestionPending { id, questions, .. } => Some(TuiMessage::QuestionPending {
            id,
            questions: serde_json::from_value(questions).ok()?,
        }),
        CoreEvent::TurnCompleted { stop_reason, .. } => {
            Some(TuiMessage::SessionEnded { stop_reason })
        }
        CoreEvent::TurnFailed { message, .. } => Some(TuiMessage::Error { message }),
        _ => None,
    }
}

pub async fn handle_core_ws(
    ws: WebSocketUpgrade,
    State(state): State<crate::server::state::ServerState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    auth: WebSocketAuth,
) -> impl axum::response::IntoResponse {
    if let Err(res) = validate_ws_auth(&auth) {
        return res.into_response();
    }

    ws.on_upgrade(move |socket| async move {
        upgrade_core_ws(socket, state, addr).await;
    })
}

async fn upgrade_core_ws(
    mut socket: WebSocket,
    state: crate::server::state::ServerState,
    addr: std::net::SocketAddr,
) {
    let Some(daemon) = state.daemon else {
        tracing::warn!("[{}] No CoreDaemon available for /core WebSocket", addr);
        let _ = socket.close().await;
        return;
    };

    let (ws_tx, mut ws_rx) = socket.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<axum::extract::ws::Message>();

    let mut send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        while let Some(msg) = out_rx.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut event_rx = daemon.subscribe();
    let out_tx_events = out_tx.clone();
    let mut event_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let frame = CoreFrame::Event(event);
            if let Ok(json) = serde_json::to_string(&frame) {
                if out_tx_events
                    .send(axum::extract::ws::Message::Text(json.into()))
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("[{}] CoreFrame WebSocket error: {}", addr, e);
                    break;
                }
            };

            match msg {
                axum::extract::ws::Message::Text(text) => {
                    match serde_json::from_str::<CoreFrame>(&text) {
                        Ok(frame) => {
                            let frames = handle_core_frame(frame, &daemon).await;
                            for frame in frames {
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    if out_tx
                                        .send(axum::extract::ws::Message::Text(json.into()))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[{}] Failed to parse CoreFrame: {}", addr, e);
                        }
                    }
                }
                axum::extract::ws::Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
            event_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
            event_task.abort();
        }
        _ = &mut event_task => {
            send_task.abort();
            recv_task.abort();
        }
    }

    info!("[{}] CoreFrame WebSocket connection closed", addr);
}

async fn handle_core_frame(
    frame: CoreFrame,
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
) -> Vec<CoreFrame> {
    let mut responses = Vec::new();
    match frame {
        CoreFrame::Request(envelope) => {
            let request_id = envelope.request_id.clone();
            match daemon.handle_request(envelope).await {
                Ok(response) => {
                    responses.push(CoreFrame::Response {
                        request_id,
                        response,
                    });
                }
                Err(e) => {
                    responses.push(CoreFrame::Error {
                        request_id: Some(request_id),
                        code: "handler_error".to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }
        CoreFrame::Subscribe {
            session_id,
            from_event_seq,
            ..
        } => {
            let filter = crate::core::event_log::EventFilter {
                session_id,
                client_id: None,
                include_global: true,
            };
            let from = from_event_seq.unwrap_or(1);
            let events = daemon.replay_from(from, &filter).await;
            for event in events {
                responses.push(CoreFrame::Event(event));
            }
        }
        CoreFrame::Ping => {
            responses.push(CoreFrame::Pong);
        }
        _ => {}
    }
    responses
}
