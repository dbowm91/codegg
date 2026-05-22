use std::sync::Arc;

use axum::{
    extract::{ws::WebSocket, ConnectInfo, FromRequestParts, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tracing::info;

use crate::error::ServerRuntimeError;
use crate::protocol::tui::{QuestionSpec, TuiMessage};

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
        None => {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
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
    let send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        while let Some(msg) = out_rx.recv().await {
            let _ = ws_tx.send(msg).await;
        }
        drop(state_clone);
    });

    let rate_limiter = state.ws_rate_limiter.clone();

    let recv_task = tokio::spawn(async move {
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
        _ = send_task => {
            recv_task.abort();
        }
        _ = recv_task => {
            send_task.abort();
        }
    }

    info!("WebSocket connection closed");
}

async fn handle_rpc_request(
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
            let id = req
                .params
                .as_ref()
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
            let dir = req
                .params
                .as_ref()
                .and_then(|p| p.get("directory"))
                .and_then(|v| v.as_str())
                .unwrap_or(&state.project_dir);
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

#[allow(dead_code)]
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

let send_task = tokio::spawn(async move {
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

    let recv_task = tokio::spawn(async move {
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
        let mut event_bus_rx = GlobalEventBus::subscribe();
        loop {
            match event_bus_rx.recv().await {
                Ok(event) => {
                    if let Some(tui_msg) = convert_app_event(event.clone()) {
                        if let Ok(json) = serde_json::to_string(&tui_msg) {
                            let ws_msg = axum::extract::ws::Message::Text(json.into());
                            if bus_tx_clone3.send(ws_msg).is_err() {
                                tracing::warn!("WebSocket send failed, client may have lagged");
                                if matches!(event, AppEvent::PermissionPending { .. })
                                    || matches!(event, AppEvent::QuestionPending { .. })
                                {
                                    let resync_msg = TuiMessage::ResyncRequired {
                                        reason: None,
                                        pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
                                        pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
                                    };
                                    if let Ok(json) = serde_json::to_string(&resync_msg) {
                                        let _ = bus_tx_clone3.send(axum::extract::ws::Message::Text(json.into()));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    tracing::warn!("Event bus receiver lagged, sending resync");
                    let resync_msg = TuiMessage::ResyncRequired {
                        reason: Some("lagged".to_string()),
                        pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
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
    #[allow(dead_code)]
    model: String,
    rate_limit_key: String,
}

impl TuiSessionState {
    fn new(rate_limit_key: String) -> Self {
        Self {
            session_id: None,
            model: "anthropic/claude-sonnet-4-20250514".to_string(),
            rate_limit_key,
        }
    }
}

async fn handle_tui_message(
    msg: TuiMessage,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    _bus_tx: &mpsc::UnboundedSender<axum::extract::ws::Message>,
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
        TuiMessage::PermissionResponse { id, choice } => {
            let perm_choice = match choice.as_str() {
                "allow" => PermissionChoice::AllowOnce,
                "deny" => PermissionChoice::DenyOnce,
                "always_allow" => PermissionChoice::AlwaysAllow,
                "always_deny" => PermissionChoice::AlwaysDeny,
                _ => PermissionChoice::DenyOnce,
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
            state_guard.model = model;
            state_guard.rate_limit_key = format!("session:{}", id);
        }
        _ => {}
    }
}

fn convert_app_event(event: AppEvent) -> Option<TuiMessage> {
    match event {
        AppEvent::TextDelta { delta, .. } => Some(TuiMessage::TextDelta {
            delta: delta.to_string(),
        }),
        AppEvent::ReasoningDelta { delta: _, .. } => None,
        AppEvent::ToolCallStarted {
            tool_name,
            tool_id,
            arguments,
            ..
        } => Some(TuiMessage::ToolCallStarted {
            tool_name,
            tool_id,
            arguments,
        }),
        AppEvent::ToolResult {
            tool_id,
            output,
            success,
            ..
        } => Some(TuiMessage::ToolResult {
            tool_id,
            output,
            success,
        }),
        AppEvent::PermissionPending {
            perm_id,
            tool,
            path,
            ..
        } => Some(TuiMessage::PermissionPending {
            id: perm_id,
            tool,
            path,
        }),
        AppEvent::QuestionPending {
            session_id,
            questions,
        } => {
            let questions_vec: Vec<QuestionSpec> = serde_json::from_str(&questions).ok()?;
            Some(TuiMessage::QuestionPending {
                id: session_id,
                questions: questions_vec,
            })
        }
        AppEvent::AgentFinished { stop_reason, .. } => {
            Some(TuiMessage::SessionEnded { stop_reason })
        }
        _ => None,
    }
}

use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::permission::PermissionChoice;
