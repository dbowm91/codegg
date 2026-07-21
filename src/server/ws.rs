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

use crate::core::transport::projection::{
    OwnedProjectionSubscription, ProjectionConnectionMode, ProjectionConnectionState,
};
use crate::error::AxumServerRuntimeError;
use crate::protocol::core::{CoreEvent, CoreRequest, EventEnvelope};
use crate::protocol::frames::{CoreFrame, ServerCapabilities, ServerHello};
use crate::protocol::tui::TuiMessage;
use crate::server::rpc::{RpcError, RpcRequest, RpcResponse};
use crate::server::scope::{resolve_context, ScopeQuery};

const WS_OUTBOUND_QUEUE_CAPACITY: usize = 256;

fn rpc_error(req: &RpcRequest, code: i64, message: impl Into<String>) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id.clone(),
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
        }),
    }
}

async fn rpc_context(
    state: &crate::server::state::ServerState,
    params: &serde_json::Value,
    session_id: Option<&str>,
) -> Result<codegg_core::context::ProjectContext, String> {
    let scope = ScopeQuery::from_json(params);
    resolve_context(&state.pool, &scope, session_id)
        .await
        .map_err(|error| format!("{:?}", error.0))
}

#[derive(Clone, Debug)]
pub struct WebSocketAuth {
    pub authorization: Option<String>,
}

impl<S> FromRequestParts<S> for WebSocketAuth
where
    S: Send + Sync,
{
    type Rejection = AxumServerRuntimeError;

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

    if let Some(expected_token) = expected {
        let valid = client_token
            .as_ref()
            .map(|t| t.as_bytes().ct_eq(expected_token.as_bytes()).unwrap_u8() == 1)
            .unwrap_or(false);

        if !valid {
            return Err(StatusCode::UNAUTHORIZED);
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
                let context = match rpc_context(state, &req.params, None).await {
                    Ok(context) => context,
                    Err(error) => return rpc_error(req, -32602, error),
                };
                let params = req.params.as_object();
                let show_archived = params
                    .and_then(|p| p.get("show_archived"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let limit = params
                    .and_then(|p| p.get("limit"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(50) as usize;
                CoreRequest::SessionList {
                    project_id: context.project_id.to_string(),
                    show_archived,
                    limit,
                }
            }
            "sessions.get" => {
                let id = req
                    .params
                    .as_object()
                    .and_then(|p| p.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if let Err(error) = rpc_context(state, &req.params, Some(id)).await {
                    return rpc_error(req, -32602, error);
                }
                CoreRequest::SessionLoad {
                    session_id: id.to_string(),
                }
            }
            "sessions.create" => {
                let context = match rpc_context(state, &req.params, None).await {
                    Ok(context) => context,
                    Err(error) => return rpc_error(req, -32602, error),
                };
                CoreRequest::SessionCreate {
                    directory: context.workspace_root.to_string_lossy().into_owned(),
                    title: None,
                    project_id: Some(context.project_id.to_string()),
                    workspace_id: Some(context.workspace_id.to_string()),
                }
            }
            "providers.list" | "tools.list" => {
                // These don't map to CoreRequest; handle directly via daemon's DB pool
                return handle_rpc_direct(req, state).await;
            }
            _ => {
                return rpc_error(req, -32601, format!("Method not found: {}", req.method));
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
            let context = match rpc_context(state, &req.params, None).await {
                Ok(context) => context,
                Err(error) => return rpc_error(req, -32602, error),
            };
            let store = crate::session::SessionStore::new(state.pool.clone());
            match store
                .list_by_canonical_project(context.project_id.as_str(), Some(50))
                .await
            {
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
            if let Err(error) = rpc_context(state, &req.params, Some(id)).await {
                return rpc_error(req, -32602, error);
            }
            let store = crate::session::SessionStore::new(state.pool.clone());
            match store.get(id).await {
                Ok(Some(s)) => RpcResponse {
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
                },
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
            let context = match rpc_context(state, &req.params, None).await {
                Ok(context) => context,
                Err(error) => return rpc_error(req, -32602, error),
            };
            let store = crate::session::SessionStore::new(state.pool.clone());
            let input = crate::session::CreateSession {
                project_id: context.project_id.as_str().to_string(),
                directory: context.workspace_root.to_string_lossy().into_owned(),
                title: None,
                parent_id: None,
                workspace_id: Some(context.workspace_id.to_string()),
                agent: None,
                model: None,
                tags: None,
                provider_connection_id: None,
                provider_connection_revision: None,
                model_catalog_revision: None,
                selected_model_id: None,
            };
            match store
                .create_with_binding(
                    input,
                    &context.project_id,
                    &context.workspace_id,
                    "server_ws_session_create",
                )
                .await
            {
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

    let (out_tx, mut out_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let (projection_tx, mut projection_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let (raw_tx, mut raw_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let connection_id = format!("tui-{}", uuid::Uuid::new_v4());
    let projection = Arc::new(tokio::sync::Mutex::new(ProjectionConnectionState::new(
        connection_id.clone(),
    )));
    let session_state = Arc::new(Mutex::new(TuiSessionState::new(
        addr.to_string(),
        projection,
    )));
    let daemon_clone = state.daemon.clone();

    let mut send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        loop {
            tokio::select! {
                biased;
                msg = out_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
                msg = projection_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
                msg = raw_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
            }
        }
    });

    let rate_limiter = state.ws_rate_limiter.clone();
    let out_tx_for_recv = out_tx.clone();
    let projection_tx_for_recv = projection_tx.clone();
    let session_state_for_recv = Arc::clone(&session_state);
    let state_for_recv = state.clone();

    let session_state_for_recv_key = Arc::clone(&session_state);
    let mut recv_task = tokio::spawn(async move {
        let mut ws_rx = ws_rx;
        while let Some(Ok(msg)) = ws_rx.next().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                let key = {
                    let session = session_state_for_recv_key.lock().await;
                    session.rate_limit_key.clone()
                };
                if !rate_limiter.check_rate_limit(&key).await {
                    let err = TuiMessage::Error {
                        message: "Too Many Requests".to_string(),
                    };
                    if let Ok(msg) = serde_json::to_string(&err) {
                        let _ =
                            out_tx_for_recv.try_send(axum::extract::ws::Message::Text(msg.into()));
                    }
                    break;
                }

                if let Ok(tui_msg) = serde_json::from_str::<TuiMessage>(&text) {
                    handle_tui_message(
                        tui_msg,
                        &session_state_for_recv,
                        &out_tx_for_recv,
                        &projection_tx_for_recv,
                        &state_for_recv,
                    )
                    .await;
                }
            }
        }
    });

    let raw_tx_events = raw_tx.clone();
    let mut event_task = tokio::spawn(async move {
        let Some(daemon) = daemon_clone else {
            tracing::warn!("No CoreDaemon available for /tui event task; live events disabled");
            return;
        };
        let mut event_rx = daemon.subscribe();
        loop {
            match event_rx.recv().await {
                Ok(envelope) => {
                    if matches!(
                        envelope.payload,
                        crate::protocol::core::CoreEvent::ProjectionStreamEvent { .. }
                    ) {
                        continue;
                    }
                    if let Some(tui_msg) = convert_core_event_to_tui(envelope.payload) {
                        let wire = TuiMessage::EventEnvelope {
                            event_seq: envelope.event_seq,
                            payload: Box::new(tui_msg),
                        };
                        if let Ok(json) = serde_json::to_string(&wire) {
                            if raw_tx_events
                                .try_send(axum::extract::ws::Message::Text(json.into()))
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
                        let _ =
                            raw_tx_events.try_send(axum::extract::ws::Message::Text(json.into()));
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
        _ = (&mut event_task) => {
            send_task.abort();
            recv_task.abort();
        }
    }

    let subscription_ids: Vec<_> = session_state
        .lock()
        .await
        .projection
        .lock()
        .await
        .subscriptions()
        .map(|subscription| subscription.subscription_id.clone())
        .collect();
    let projection_state = session_state.lock().await.projection.clone();
    projection_state.lock().await.cleanup().await;
    if let Some(daemon) = state.daemon {
        for subscription_id in subscription_ids {
            let _ = daemon
                .handle_request_for_client(
                    crate::core::new_request(
                        format!("tui-projection-disconnect-{}", uuid::Uuid::new_v4()),
                        CoreRequest::ProjectionUnsubscribe { subscription_id },
                    ),
                    &connection_id,
                )
                .await;
        }
    }

    info!("TUI WebSocket connection closed");
}

#[derive(Clone)]
struct TuiSessionState {
    session_id: Option<String>,
    model: Option<String>,
    rate_limit_key: String,
    projection: Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
}

impl TuiSessionState {
    fn new(
        rate_limit_key: String,
        projection: Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    ) -> Self {
        Self {
            session_id: None,
            model: None,
            rate_limit_key,
            projection,
        }
    }
}

async fn handle_tui_message(
    msg: TuiMessage,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
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
            if state.lock().await.projection.lock().await.mode()
                == ProjectionConnectionMode::ProjectionPrimary
            {
                let diagnostic = TuiMessage::ProjectionCompatibilityDiagnostic {
                    code: "raw_resume_ignored_in_projection_primary".into(),
                    message: "projection-primary connections resume with ProjectionCursor".into(),
                };
                if let Ok(json) = serde_json::to_string(&diagnostic) {
                    let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
                }
                return;
            }
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
                            let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
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
                    let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
                }
                return;
            }
            let resync_msg = TuiMessage::ResyncRequired {
                reason: Some("resume_requested".to_string()),
                pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
                pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
            };
            if let Ok(json) = serde_json::to_string(&resync_msg) {
                let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
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
        TuiMessage::RequestSnapshot { reason } => {
            tracing::info!("RequestSnapshot from client: reason={:?}", reason);
            if state.lock().await.projection.lock().await.mode()
                == ProjectionConnectionMode::ProjectionPrimary
            {
                return;
            }
            if let Some(ref daemon) = _server_state.daemon {
                let filter = crate::core::event_log::EventFilter {
                    session_id: None,
                    client_id: None,
                    include_global: true,
                };
                let events = daemon.replay_from(0, &filter).await;
                for event in events {
                    if let Some(tui_msg) = convert_core_event_to_tui(event.payload) {
                        let envelope = TuiMessage::EventEnvelope {
                            event_seq: event.event_seq,
                            payload: Box::new(tui_msg),
                        };
                        if let Ok(json) = serde_json::to_string(&envelope) {
                            let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
                        }
                    }
                }
            }
            let resync_msg = TuiMessage::ResyncRequired {
                reason: Some("snapshot_requested".to_string()),
                pending_permissions: crate::bus::PermissionRegistry::pending_permission_ids(),
                pending_questions: crate::bus::QuestionRegistry::pending_question_ids(),
            };
            if let Ok(json) = serde_json::to_string(&resync_msg) {
                let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
            }
        }
        TuiMessage::ProjectionCapabilities { capabilities } => {
            handle_projection_capabilities(capabilities, state, bus_tx, _server_state).await;
        }
        TuiMessage::ProjectionSubscribe { request } => {
            handle_projection_subscribe(request, state, bus_tx, projection_tx, _server_state).await;
        }
        TuiMessage::ProjectionAck { ack } => {
            handle_projection_ack(ack, state, bus_tx, _server_state).await;
        }
        TuiMessage::ProjectionResume {
            cursor,
            include_snapshot_if_resync,
        } => {
            handle_projection_resume(
                cursor,
                include_snapshot_if_resync,
                state,
                bus_tx,
                projection_tx,
                _server_state,
            )
            .await;
        }
        TuiMessage::ProjectionUnsubscribe { subscription_id } => {
            handle_projection_unsubscribe(subscription_id, state, bus_tx, _server_state).await;
        }
        TuiMessage::ProjectionSubscriptionStatus { subscription_id } => {
            handle_projection_status(subscription_id, state, bus_tx).await;
        }
        TuiMessage::ProjectionArtifactListRequest {
            request_id,
            project_id,
        } => {
            handle_projection_artifact_list(request_id, project_id, state, bus_tx, _server_state)
                .await;
        }
        TuiMessage::ProjectionArtifactReadRequest {
            request_id,
            request,
            project_id,
        } => {
            handle_projection_artifact_read(
                request_id,
                request,
                project_id,
                state,
                bus_tx,
                _server_state,
            )
            .await;
        }
        _ => {}
    }
}

/// Handle `ProjectionCapabilities` from a remote TUI client. The
/// server negotiates against its own capabilities and replies with a
/// `ProjectionCapabilitiesAck` carrying the negotiated version (or a
/// rejection reason).
async fn handle_projection_capabilities(
    client_caps: crate::protocol::projection::caps::ProjectionCapabilities,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    use crate::protocol::projection::caps::{
        ProjectionCapabilities, PROJECTION_PROTOCOL_VERSION, PROJECTION_PROTOCOL_VERSION_MIN,
    };
    let daemon_caps = if server_state.daemon.is_some() {
        ProjectionCapabilities {
            min_version: PROJECTION_PROTOCOL_VERSION_MIN,
            max_version: PROJECTION_PROTOCOL_VERSION,
            supports_incremental_events: true,
            supports_unknown_fields: true,
        }
    } else {
        ProjectionCapabilities {
            min_version: PROJECTION_PROTOCOL_VERSION,
            max_version: PROJECTION_PROTOCOL_VERSION,
            supports_incremental_events: false,
            supports_unknown_fields: false,
        }
    };
    let negotiated = ProjectionCapabilities::negotiate(&client_caps, &daemon_caps);
    let accepted = negotiated.is_some();
    let reason = if !accepted {
        Some("no_overlapping_projection_version".to_string())
    } else {
        None
    };
    let ack = TuiMessage::ProjectionCapabilitiesAck {
        accepted,
        negotiated_version: negotiated,
        reason,
    };
    if let Ok(json) = serde_json::to_string(&ack) {
        let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
    }
    let projection = state.lock().await.projection.clone();
    let mode = if accepted {
        ProjectionConnectionMode::ProjectionPrimary
    } else {
        ProjectionConnectionMode::RawCompatibility
    };
    {
        let mut projection_state = projection.lock().await;
        projection_state.set_mode(mode, negotiated);
    }
    if !accepted {
        let (client_id, subscription_ids) = {
            let projection_state = projection.lock().await;
            (
                projection_state.connection_id().to_string(),
                projection_state
                    .subscriptions()
                    .map(|subscription| subscription.subscription_id.clone())
                    .collect::<Vec<_>>(),
            )
        };
        projection.lock().await.cleanup().await;
        if let Some(daemon) = &server_state.daemon {
            for subscription_id in subscription_ids {
                let _ = daemon
                    .handle_request_for_client(
                        crate::core::new_request(
                            format!("tui-projection-downgrade-{}", uuid::Uuid::new_v4()),
                            CoreRequest::ProjectionUnsubscribe { subscription_id },
                        ),
                        &client_id,
                    )
                    .await;
            }
        }
    }
    if accepted {
        let diagnostic = TuiMessage::ProjectionCompatibilityDiagnostic {
            code: "raw_compatibility_deprecated".into(),
            message: "legacy raw session channels remain bounded for v4 compatibility and are not projection authority".into(),
        };
        if let Ok(json) = serde_json::to_string(&diagnostic) {
            let _ = bus_tx.try_send(axum::extract::ws::Message::Text(json.into()));
        }
    }
}

/// Handle `ProjectionSubscribe` from a remote TUI client. The server
/// forwards the request to the daemon and pipes the initial snapshot
/// plus any live projection envelopes back over the WebSocket.
async fn handle_projection_subscribe(
    request: crate::protocol::projection::replay::ProjectionSubscriptionRequest,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    use crate::protocol::projection::replay::ProjectionSnapshotBundle;
    use crate::protocol::projection::snapshot::SessionProjectionSnapshot;

    if !require_projection_primary(state, bus_tx).await {
        return;
    }

    let Some(daemon) = &server_state.daemon else {
        queue_tui_error(bus_tx, "projection_unavailable_no_daemon");
        return;
    };
    let projection = state.lock().await.projection.clone();
    let client_id = projection.lock().await.connection_id().to_string();
    let requested_cursor = request.cursor.clone();

    let response = daemon
        .handle_request_for_client(
            crate::core::new_request(
                format!("ws-projection-subscribe-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionSubscribe { request },
            ),
            &client_id,
        )
        .await;
    match response {
        Ok(crate::protocol::core::CoreResponse::ProjectionSubscribed {
            subscription_id,
            descriptor,
            snapshot,
            cursor,
            retention_floor_seq,
        }) => {
            if !install_tui_projection_receiver(
                daemon,
                &projection,
                &subscription_id,
                &descriptor,
                &cursor,
                retention_floor_seq,
                projection_tx,
                bus_tx,
            )
            .await
            {
                let _ = daemon
                    .handle_request_for_client(
                        crate::core::new_request(
                            format!("ws-projection-cleanup-{}", uuid::Uuid::new_v4()),
                            CoreRequest::ProjectionUnsubscribe {
                                subscription_id: subscription_id.clone(),
                            },
                        ),
                        &client_id,
                    )
                    .await;
                queue_tui_error(bus_tx, "projection_receiver_install_failed");
                return;
            }
            let snapshot = match snapshot {
                ProjectionSnapshotBundle::One { snapshot } => *snapshot,
                ProjectionSnapshotBundle::BoundedSessionList { sessions, .. } => {
                    sessions.into_iter().next().unwrap_or_else(|| {
                        SessionProjectionSnapshot::empty(
                            descriptor.session_id.as_deref().unwrap_or(""),
                            &descriptor.project_id,
                            descriptor.workspace_id.as_deref().unwrap_or(""),
                        )
                    })
                }
            };
            let msg = TuiMessage::ProjectionSnapshot {
                subscription_id: subscription_id.clone(),
                descriptor,
                snapshot: Box::new(snapshot),
                cursor: Some(cursor),
                retention_floor_seq: Some(retention_floor_seq),
            };
            queue_tui(bus_tx, &msg);
            if let Some(cursor) = requested_cursor {
                let resume = daemon
                    .handle_request_for_client(
                        crate::core::new_request(
                            format!("ws-projection-resume-{}", uuid::Uuid::new_v4()),
                            CoreRequest::ProjectionResume {
                                cursor,
                                include_snapshot_if_resync: true,
                            },
                        ),
                        &client_id,
                    )
                    .await;
                emit_tui_projection_response(
                    daemon,
                    &projection,
                    resume,
                    bus_tx,
                    projection_tx,
                    &client_id,
                )
                .await;
            }
            if let Some(subscription) = projection.lock().await.subscription_mut(&subscription_id) {
                subscription.mark_live();
            }
        }
        Ok(crate::protocol::core::CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        }) => {
            emit_tui_projection_response(
                daemon,
                &projection,
                Ok(crate::protocol::core::CoreResponse::ProjectionReplay {
                    subscription_id: Some(subscription_id),
                    batch,
                }),
                bus_tx,
                projection_tx,
                &client_id,
            )
            .await;
        }
        Ok(crate::protocol::core::CoreResponse::ProjectionResyncRequired {
            subscription_id,
            reason,
            descriptor,
            requested_cursor,
            snapshot,
        }) => {
            let msg = TuiMessage::ProjectionResync {
                subscription_id,
                reason,
                descriptor,
                requested_cursor,
                snapshot,
            };
            queue_tui(bus_tx, &msg);
        }
        Ok(other) => {
            queue_tui_error(bus_tx, &format!("projection_subscribe_failed:{other:?}"));
        }
        Err(err) => {
            queue_tui_error(bus_tx, &format!("projection_subscribe_error:{err}"));
        }
    }
}

fn queue_tui(tx: &mpsc::Sender<axum::extract::ws::Message>, message: &TuiMessage) -> bool {
    serde_json::to_string(message)
        .ok()
        .map(|json| {
            tx.try_send(axum::extract::ws::Message::Text(json.into()))
                .is_ok()
        })
        .unwrap_or(false)
}

fn queue_tui_error(tx: &mpsc::Sender<axum::extract::ws::Message>, message: &str) {
    let _ = queue_tui(
        tx,
        &TuiMessage::Error {
            message: message.to_string(),
        },
    );
}

async fn require_projection_primary(
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
) -> bool {
    let projection = state.lock().await.projection.clone();
    if projection.lock().await.mode() == ProjectionConnectionMode::ProjectionPrimary {
        true
    } else {
        queue_tui_error(bus_tx, "projection_capabilities_required");
        false
    }
}

async fn install_tui_projection_receiver(
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
    projection: &Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    subscription_id: &crate::protocol::projection::replay::ProjectionSubscriptionId,
    descriptor: &crate::protocol::projection::replay::ProjectionStreamDescriptor,
    cursor: &crate::protocol::projection::replay::ProjectionCursor,
    retention_floor_seq: u64,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
) -> bool {
    if projection.lock().await.owns(subscription_id) {
        return true;
    }
    let Some(seam) = daemon.projection_seam.as_ref() else {
        let client_id = projection.lock().await.connection_id().to_string();
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("tui-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                &client_id,
            )
            .await;
        return false;
    };
    let Some(mut rx) = seam
        .service()
        .take_subscription_receiver(subscription_id)
        .await
    else {
        let client_id = projection.lock().await.connection_id().to_string();
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("tui-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                &client_id,
            )
            .await;
        return false;
    };
    let mut state = projection.lock().await;
    let owned = OwnedProjectionSubscription::new(
        subscription_id.clone(),
        descriptor.clone(),
        cursor.clone(),
        retention_floor_seq,
        state.reconnect_generation(),
    );
    let ready = owned.ready.clone();
    let cancellation = owned.cancellation.clone();
    if state.insert_subscription(owned).is_err() {
        drop(state);
        let client_id = projection.lock().await.connection_id().to_string();
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("tui-projection-duplicate-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                &client_id,
            )
            .await;
        return false;
    }
    let output = projection_tx.clone();
    let control_output = bus_tx.clone();
    let projection_for_task = Arc::clone(projection);
    let sub_id = subscription_id.clone();
    let stream_id = descriptor.stream_id.clone();
    let descriptor_for_lag = descriptor.clone();
    let handle = tokio::spawn(async move {
        tokio::select! {
            _ = cancellation.cancelled() => return,
            _ = ready.notified() => {}
        }
        loop {
            let envelope = tokio::select! {
                _ = cancellation.cancelled() => break,
                envelope = rx.recv() => envelope,
            };
            let Some(envelope) = envelope else { break };
            let message = TuiMessage::ProjectionEvent {
                subscription_id: sub_id.clone(),
                stream_id: Some(stream_id.clone()),
                envelope: envelope.clone(),
            };
            if !queue_tui(&output, &message) {
                if let Some(subscription) =
                    projection_for_task.lock().await.subscription_mut(&sub_id)
                {
                    subscription.mark_resync_required();
                }
                let _ = queue_tui(
                    &control_output,
                    &TuiMessage::ProjectionResync {
                        subscription_id: Some(sub_id.clone()),
                        reason: crate::protocol::projection::replay::ProjectionResyncReason::SubscriberLagged,
                        descriptor: Some(descriptor_for_lag.clone()),
                        requested_cursor: None,
                        snapshot: None,
                    },
                );
                break;
            }
            if let Some(subscription) = projection_for_task.lock().await.subscription_mut(&sub_id) {
                subscription.latest_cursor =
                    crate::protocol::projection::replay::ProjectionCursor {
                        stream_id: stream_id.clone(),
                        event_seq: envelope.event_seq,
                        projection_version: envelope.protocol_version,
                    };
            }
        }
    });
    if let Some(subscription) = state.subscription_mut(subscription_id) {
        subscription.forwarder = Some(handle);
    }
    true
}

async fn emit_tui_projection_response(
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
    projection: &Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    response: Result<crate::protocol::core::CoreResponse, crate::error::AppError>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
    client_id: &str,
) {
    match response {
        Ok(crate::protocol::core::CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        }) => {
            let cursor = batch.next_cursor.clone().unwrap_or(
                crate::protocol::projection::replay::ProjectionCursor {
                    stream_id: batch.descriptor.stream_id.clone(),
                    event_seq: batch.current_high_water,
                    projection_version: batch.descriptor.projection_version,
                },
            );
            if !install_tui_projection_receiver(
                daemon,
                projection,
                &subscription_id,
                &batch.descriptor,
                &cursor,
                batch.descriptor.retention_floor_seq,
                projection_tx,
                bus_tx,
            )
            .await
            {
                queue_tui_error(bus_tx, "projection_receiver_install_failed");
                return;
            }
            queue_tui(
                bus_tx,
                &TuiMessage::ProjectionReplay {
                    subscription_id,
                    batch,
                },
            );
        }
        Ok(crate::protocol::core::CoreResponse::ProjectionResyncRequired {
            subscription_id,
            reason,
            descriptor,
            requested_cursor,
            snapshot,
        }) => {
            if let Some(subscription_id) = subscription_id.as_ref() {
                if let Some(mut subscription) =
                    projection.lock().await.remove_subscription(subscription_id)
                {
                    subscription.cancel();
                    if let Some(forwarder) = subscription.forwarder.take() {
                        forwarder.abort();
                        let _ = forwarder.await;
                    }
                    let _ = daemon
                        .handle_request_for_client(
                            crate::core::new_request(
                                format!("tui-projection-resync-{}", uuid::Uuid::new_v4()),
                                CoreRequest::ProjectionUnsubscribe {
                                    subscription_id: subscription_id.clone(),
                                },
                            ),
                            client_id,
                        )
                        .await;
                }
            }
            queue_tui(
                bus_tx,
                &TuiMessage::ProjectionResync {
                    subscription_id,
                    reason,
                    descriptor,
                    requested_cursor,
                    snapshot,
                },
            );
        }
        Ok(other) => queue_tui_error(bus_tx, &format!("projection_operation_failed:{other:?}")),
        Err(error) => queue_tui_error(bus_tx, &format!("projection_operation_error:{error}")),
    }
}

/// Handle `ProjectionAck` from a remote TUI client. The server
/// forwards the acknowledgement to the daemon so the durable replay
/// store can advance the subscription's last-acked cursor.
async fn handle_projection_ack(
    ack: crate::protocol::projection::replay::ProjectionAck,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let Some(daemon) = &server_state.daemon else {
        queue_tui_error(bus_tx, "projection_unavailable_no_daemon");
        return;
    };
    let projection = state.lock().await.projection.clone();
    let client_id = projection.lock().await.connection_id().to_string();
    if !projection.lock().await.owns(&ack.subscription_id) {
        queue_tui(
            bus_tx,
            &TuiMessage::ProjectionAckResult {
                ack,
                accepted: false,
                last_acked_seq: None,
                lag_count: None,
                error: Some("projection_subscription_not_owned".into()),
            },
        );
        return;
    }
    let ack_for_response = ack.clone();
    let response = daemon
        .handle_request_for_client(
            crate::core::new_request(
                format!("ws-projection-ack-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionAck { ack },
            ),
            &client_id,
        )
        .await;
    if let Ok(crate::protocol::core::CoreResponse::ProjectionAckAccepted {
        last_acked_seq, ..
    }) = &response
    {
        if let Some(subscription) = projection
            .lock()
            .await
            .subscription_mut(&ack_for_response.subscription_id)
        {
            subscription.last_acked_seq = *last_acked_seq;
        }
    }
    let message = match response {
        Ok(crate::protocol::core::CoreResponse::ProjectionAckAccepted {
            last_acked_seq,
            lag_count,
            ..
        }) => TuiMessage::ProjectionAckResult {
            ack: ack_for_response,
            accepted: true,
            last_acked_seq: Some(last_acked_seq),
            lag_count: Some(lag_count),
            error: None,
        },
        Ok(other) => TuiMessage::ProjectionAckResult {
            ack: ack_for_response,
            accepted: false,
            last_acked_seq: None,
            lag_count: None,
            error: Some(format!("projection_ack_failed:{other:?}")),
        },
        Err(error) => TuiMessage::ProjectionAckResult {
            ack: ack_for_response,
            accepted: false,
            last_acked_seq: None,
            lag_count: None,
            error: Some(error.to_string()),
        },
    };
    queue_tui(bus_tx, &message);
}

async fn handle_projection_resume(
    cursor: crate::protocol::projection::replay::ProjectionCursor,
    include_snapshot_if_resync: bool,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let Some(daemon) = &server_state.daemon else {
        queue_tui_error(bus_tx, "projection_unavailable_no_daemon");
        return;
    };
    let projection = state.lock().await.projection.clone();
    let client_id = projection.lock().await.connection_id().to_string();
    let response = daemon
        .handle_request_for_client(
            crate::core::new_request(
                format!("ws-projection-resume-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionResume {
                    cursor,
                    include_snapshot_if_resync,
                },
            ),
            &client_id,
        )
        .await;
    let live_id = match &response {
        Ok(crate::protocol::core::CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            ..
        }) => Some(subscription_id.clone()),
        _ => None,
    };
    emit_tui_projection_response(
        daemon,
        &projection,
        response,
        bus_tx,
        projection_tx,
        &client_id,
    )
    .await;
    if let Some(subscription_id) = live_id {
        if let Some(subscription) = projection.lock().await.subscription_mut(&subscription_id) {
            subscription.mark_live();
        }
    }
}

async fn handle_projection_unsubscribe(
    subscription_id: crate::protocol::projection::replay::ProjectionSubscriptionId,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let projection = state.lock().await.projection.clone();
    let client_id = projection.lock().await.connection_id().to_string();
    if !projection.lock().await.owns(&subscription_id) {
        queue_tui(
            bus_tx,
            &TuiMessage::ProjectionUnsubscribeResult {
                subscription_id,
                accepted: false,
                reason: Some("projection_subscription_not_owned".into()),
            },
        );
        return;
    }
    let accepted = if let Some(daemon) = &server_state.daemon {
        daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("ws-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                &client_id,
            )
            .await
            .map(|response| {
                matches!(
                    response,
                    crate::protocol::core::CoreResponse::ProjectionUnsubscribed { .. }
                )
            })
            .unwrap_or(false)
    } else {
        false
    };
    if let Some(mut owned_subscription) = projection
        .lock()
        .await
        .remove_subscription(&subscription_id)
    {
        owned_subscription.cancel();
        if let Some(forwarder) = owned_subscription.forwarder.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
    }
    queue_tui(
        bus_tx,
        &TuiMessage::ProjectionUnsubscribeResult {
            subscription_id,
            accepted,
            reason: if accepted {
                None
            } else {
                Some("projection_unsubscribe_failed".into())
            },
        },
    );
}

async fn handle_projection_status(
    subscription_id: crate::protocol::projection::replay::ProjectionSubscriptionId,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let projection = state.lock().await.projection.clone();
    let state = projection.lock().await;
    let Some(subscription) = state.subscription(&subscription_id) else {
        queue_tui_error(bus_tx, "projection_subscription_not_owned");
        return;
    };
    queue_tui(
        bus_tx,
        &TuiMessage::ProjectionSubscriptionStatusResult {
            status: crate::protocol::projection::replay::ProjectionSubscriptionStatus {
                id: subscription.subscription_id.clone(),
                scope: subscription.descriptor.kind,
                last_delivered_seq: subscription.latest_cursor.event_seq,
                last_acked_seq: subscription.last_acked_seq,
                state: subscription.lifecycle.into(),
                lag_count: subscription
                    .descriptor
                    .high_water_seq
                    .saturating_sub(subscription.latest_cursor.event_seq),
            },
        },
    );
}

async fn handle_projection_artifact_list(
    request_id: String,
    project_id: String,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let projection = state.lock().await.projection.clone();
    if !projection.lock().await.owns_project(&project_id) {
        queue_tui(
            bus_tx,
            &TuiMessage::ProjectionArtifactListResult {
                request_id,
                handles: vec![],
                error: Some("projection_scope_not_owned".into()),
            },
        );
        return;
    }
    if !projection.lock().await.try_begin_artifact_read() {
        queue_tui(
            bus_tx,
            &TuiMessage::ProjectionArtifactListResult {
                request_id,
                handles: vec![],
                error: Some("projection_artifact_read_limit".into()),
            },
        );
        return;
    }
    let client_id = projection.lock().await.connection_id().to_string();
    let response = if let Some(daemon) = &server_state.daemon {
        daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("ws-projection-artifact-list-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionArtifactList { project_id },
                ),
                &client_id,
            )
            .await
    } else {
        Err(crate::error::AppError::Other(anyhow::anyhow!(
            "projection daemon unavailable"
        )))
    };
    projection.lock().await.end_artifact_read();
    match response {
        Ok(crate::protocol::core::CoreResponse::ProjectionArtifactList { handles }) => {
            queue_tui(
                bus_tx,
                &TuiMessage::ProjectionArtifactListResult {
                    request_id,
                    handles,
                    error: None,
                },
            );
        }
        Ok(_) | Err(_) => {
            queue_tui(
                bus_tx,
                &TuiMessage::ProjectionArtifactListResult {
                    request_id,
                    handles: vec![],
                    error: Some("projection_artifact_list_failed".into()),
                },
            );
        }
    }
}

async fn handle_projection_artifact_read(
    request_id: String,
    request: crate::protocol::projection::replay::ProjectionArtifactReadRequest,
    project_id: String,
    state: &Arc<tokio::sync::Mutex<TuiSessionState>>,
    bus_tx: &mpsc::Sender<axum::extract::ws::Message>,
    server_state: &crate::server::state::ServerState,
) {
    if !require_projection_primary(state, bus_tx).await {
        return;
    }
    let projection = state.lock().await.projection.clone();
    if !projection.lock().await.owns_project(&project_id)
        || !projection.lock().await.try_begin_artifact_read()
    {
        queue_tui(
            bus_tx,
            &TuiMessage::ProjectionArtifactReadResult {
                request_id,
                outcome:
                    crate::protocol::projection::replay::ProjectionArtifactReadOutcome::Denied {
                        reason: "projection_scope_not_owned".into(),
                    },
            },
        );
        return;
    }
    let client_id = projection.lock().await.connection_id().to_string();
    let response = if let Some(daemon) = &server_state.daemon {
        daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("ws-projection-artifact-read-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionArtifactRead {
                        request,
                        project_id,
                        context_correlation_id: Some(client_id.clone()),
                    },
                ),
                &client_id,
            )
            .await
    } else {
        Err(crate::error::AppError::Other(anyhow::anyhow!(
            "projection daemon unavailable"
        )))
    };
    projection.lock().await.end_artifact_read();
    let outcome = match response {
        Ok(crate::protocol::core::CoreResponse::ProjectionArtifactRead { outcome }) => outcome,
        Ok(_) | Err(_) => {
            crate::protocol::projection::replay::ProjectionArtifactReadOutcome::InvalidRequest {
                reason: "projection_artifact_read_failed".into(),
            }
        }
    };
    queue_tui(
        bus_tx,
        &TuiMessage::ProjectionArtifactReadResult {
            request_id,
            outcome,
        },
    );
}

/// Convert a CoreEvent back to a TuiMessage for legacy /tui clients replaying from EventLog.
fn convert_core_event_to_tui(event: crate::protocol::core::CoreEvent) -> Option<TuiMessage> {
    use crate::protocol::core::CoreEvent;
    match event {
        // Projection-private events are delivered only by the owned
        // per-subscription receiver forwarder. They never enter the raw
        // daemon-wide compatibility broadcast.
        CoreEvent::ProjectionStreamEvent { .. } => None,
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

    let (out_tx, mut out_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let (projection_tx, mut projection_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let (raw_tx, mut raw_rx) =
        mpsc::channel::<axum::extract::ws::Message>(WS_OUTBOUND_QUEUE_CAPACITY);
    let connection_id = format!("core-ws-{}", uuid::Uuid::new_v4());
    let projection = Arc::new(tokio::sync::Mutex::new(ProjectionConnectionState::new(
        connection_id.clone(),
    )));

    let mut send_task = tokio::spawn(async move {
        let mut ws_tx = ws_tx;
        loop {
            tokio::select! {
                biased;
                msg = out_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
                msg = projection_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
                msg = raw_rx.recv() => {
                    let Some(msg) = msg else { break };
                    if ws_tx.send(msg).await.is_err() { break; }
                }
            }
        }
    });

    let mut event_rx = daemon.subscribe();
    let raw_tx_events = raw_tx.clone();
    let mut event_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            if matches!(
                event.payload,
                crate::protocol::core::CoreEvent::ProjectionStreamEvent { .. }
            ) {
                continue;
            }
            let frame = CoreFrame::Event(event);
            if let Ok(json) = serde_json::to_string(&frame) {
                if raw_tx_events
                    .try_send(axum::extract::ws::Message::Text(json.into()))
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    let projection_for_recv = Arc::clone(&projection);
    let out_tx_for_recv = out_tx.clone();
    let projection_tx_for_recv = projection_tx.clone();
    let daemon_for_recv = Arc::clone(&daemon);
    let connection_id_for_recv = connection_id.clone();
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
                            let frames = handle_core_frame(
                                frame,
                                &daemon_for_recv,
                                &projection_for_recv,
                                &connection_id_for_recv,
                                &out_tx_for_recv,
                                &projection_tx_for_recv,
                            )
                            .await;
                            for frame in &frames {
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    if out_tx_for_recv
                                        .try_send(axum::extract::ws::Message::Text(json.into()))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            for frame in &frames {
                                if let CoreFrame::Response { response, .. } = frame {
                                    if let Some(subscription_id) = projection_response_id(response)
                                    {
                                        if let Some(subscription) = projection_for_recv
                                            .lock()
                                            .await
                                            .subscription_mut(&subscription_id)
                                        {
                                            subscription.mark_live();
                                        }
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

    let subscription_ids: Vec<_> = projection
        .lock()
        .await
        .subscriptions()
        .map(|subscription| subscription.subscription_id.clone())
        .collect();
    projection.lock().await.cleanup().await;
    for subscription_id in subscription_ids {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("core-ws-projection-disconnect-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe { subscription_id },
                ),
                &connection_id,
            )
            .await;
    }

    info!("[{}] CoreFrame WebSocket connection closed", addr);
}

async fn handle_core_frame(
    frame: CoreFrame,
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
    projection: &Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    client_id: &str,
    out_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
) -> Vec<CoreFrame> {
    let mut responses = Vec::new();
    match frame {
        CoreFrame::Request(envelope) => {
            let request_id = envelope.request_id.clone();
            if matches!(
                &envelope.payload,
                crate::protocol::core::CoreRequest::EggpoolConnectionCreate { .. }
                    | crate::protocol::core::CoreRequest::ConnectionRotateSecretStage { .. }
                    | crate::protocol::core::CoreRequest::ConnectionRotateBegin { .. }
            ) {
                responses.push(CoreFrame::Response {
                    request_id,
                    response: Box::new(crate::protocol::core::CoreResponse::Error {
                        code: "secret_operation_remote_denied".to_string(),
                        message: "Secret-bearing provider connection creation is available only through local authenticated IPC".to_string(),
                    }),
                });
                return responses;
            }
            let projection_request = matches!(
                &envelope.payload,
                crate::protocol::core::CoreRequest::ProjectionSubscribe { .. }
                    | crate::protocol::core::CoreRequest::ProjectionResume { .. }
                    | crate::protocol::core::CoreRequest::ProjectionAck { .. }
                    | crate::protocol::core::CoreRequest::ProjectionUnsubscribe { .. }
                    | crate::protocol::core::CoreRequest::ProjectionSnapshotGet { .. }
                    | crate::protocol::core::CoreRequest::ProjectionArtifactRead { .. }
                    | crate::protocol::core::CoreRequest::ProjectionArtifactList { .. }
            );
            let projection_scope_owned = match &envelope.payload {
                crate::protocol::core::CoreRequest::ProjectionArtifactRead {
                    project_id, ..
                }
                | crate::protocol::core::CoreRequest::ProjectionArtifactList { project_id } => {
                    Some(projection.lock().await.owns_project(project_id))
                }
                _ => None,
            };
            let artifact_read_started = if projection_scope_owned == Some(true) {
                projection.lock().await.try_begin_artifact_read()
            } else {
                false
            };
            if projection_request
                && projection.lock().await.mode() != ProjectionConnectionMode::ProjectionPrimary
            {
                if artifact_read_started {
                    projection.lock().await.end_artifact_read();
                }
                responses.push(CoreFrame::Response {
                    request_id,
                    response: Box::new(crate::protocol::core::CoreResponse::Error {
                        code: "projection_capabilities_required".into(),
                        message:
                            "send a projection-capable ClientHello before projection operations"
                                .into(),
                    }),
                });
                return responses;
            }
            if projection_scope_owned == Some(true) && !artifact_read_started {
                responses.push(CoreFrame::Response {
                    request_id,
                    response: Box::new(crate::protocol::core::CoreResponse::Error {
                        code: "projection_artifact_read_limit".into(),
                        message: "projection artifact read limit exceeded".into(),
                    }),
                });
                return responses;
            }
            if projection_scope_owned == Some(false) {
                responses.push(CoreFrame::Response {
                    request_id,
                    response: Box::new(crate::protocol::core::CoreResponse::Error {
                        code: "projection_scope_not_owned".into(),
                        message: "projection artifact scope is not owned by this connection".into(),
                    }),
                });
                return responses;
            }
            match daemon.handle_request_for_client(envelope, client_id).await {
                Ok(response) => {
                    if let crate::protocol::core::CoreResponse::ProjectionResyncRequired {
                        subscription_id: Some(subscription_id),
                        ..
                    } = &response
                    {
                        cleanup_core_projection_subscription(
                            daemon,
                            projection,
                            subscription_id,
                            client_id,
                        )
                        .await;
                    }
                    if matches!(
                        response,
                        crate::protocol::core::CoreResponse::ProjectionSubscribed { .. }
                            | crate::protocol::core::CoreResponse::ProjectionReplay {
                                subscription_id: Some(_),
                                ..
                            }
                    ) && !install_core_projection_response(
                        daemon,
                        projection,
                        &response,
                        out_tx,
                        projection_tx,
                        client_id,
                    )
                    .await
                    {
                        responses.push(CoreFrame::Response {
                            request_id,
                            response: Box::new(crate::protocol::core::CoreResponse::Error {
                                code: "projection_receiver_install_failed".into(),
                                message: "projection live receiver could not be installed".into(),
                            }),
                        });
                        return responses;
                    }
                    responses.push(CoreFrame::Response {
                        request_id,
                        response: Box::new(response),
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
            if artifact_read_started {
                projection.lock().await.end_artifact_read();
            }
        }
        CoreFrame::ClientHello(hello) => {
            let projection_supported = hello.capabilities.session_projection;
            projection.lock().await.set_mode(
                if projection_supported {
                    ProjectionConnectionMode::ProjectionPrimary
                } else {
                    ProjectionConnectionMode::RawCompatibility
                },
                projection_supported.then_some(1),
            );
            if !projection_supported {
                let subscription_ids: Vec<_> = projection
                    .lock()
                    .await
                    .subscriptions()
                    .map(|subscription| subscription.subscription_id.clone())
                    .collect();
                projection.lock().await.cleanup().await;
                for subscription_id in subscription_ids {
                    let _ = daemon
                        .handle_request_for_client(
                            crate::core::new_request(
                                format!("core-ws-projection-downgrade-{}", uuid::Uuid::new_v4()),
                                CoreRequest::ProjectionUnsubscribe { subscription_id },
                            ),
                            client_id,
                        )
                        .await;
                }
            }
            responses.push(CoreFrame::ServerHello(ServerHello {
                daemon_id: daemon.daemon_id.clone(),
                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                server_capabilities: ServerCapabilities {
                    event_replay: true,
                    session_management: true,
                    permission_routing: true,
                    workspace_registration: true,
                    workspace_snapshots: true,
                    durable_jobs: true,
                    durable_schedules: true,
                    identity_aware_context: true,
                    project_catalog: true,
                    session_projection: true,
                },
                client_id: client_id.to_string(),
            }));
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
                if !matches!(
                    event.payload,
                    crate::protocol::core::CoreEvent::ProjectionStreamEvent { .. }
                ) {
                    responses.push(CoreFrame::Event(event));
                }
            }
        }
        CoreFrame::Ping => {
            responses.push(CoreFrame::Pong);
        }
        _ => {}
    }
    responses
}

fn projection_response_id(
    response: &crate::protocol::core::CoreResponse,
) -> Option<crate::protocol::projection::replay::ProjectionSubscriptionId> {
    match response {
        crate::protocol::core::CoreResponse::ProjectionSubscribed {
            subscription_id, ..
        }
        | crate::protocol::core::CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            ..
        } => Some(subscription_id.clone()),
        _ => None,
    }
}

async fn cleanup_core_projection_subscription(
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
    projection: &Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    subscription_id: &crate::protocol::projection::replay::ProjectionSubscriptionId,
    client_id: &str,
) {
    if let Some(mut subscription) = projection.lock().await.remove_subscription(subscription_id) {
        subscription.cancel();
        if let Some(forwarder) = subscription.forwarder.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
    }
    let _ = daemon
        .handle_request_for_client(
            crate::core::new_request(
                format!("core-ws-projection-resync-{}", uuid::Uuid::new_v4()),
                CoreRequest::ProjectionUnsubscribe {
                    subscription_id: subscription_id.clone(),
                },
            ),
            client_id,
        )
        .await;
}

async fn install_core_projection_response(
    daemon: &Arc<crate::core::daemon::CoreDaemon>,
    projection: &Arc<tokio::sync::Mutex<ProjectionConnectionState>>,
    response: &crate::protocol::core::CoreResponse,
    control_tx: &mpsc::Sender<axum::extract::ws::Message>,
    projection_tx: &mpsc::Sender<axum::extract::ws::Message>,
    client_id: &str,
) -> bool {
    let (subscription_id, descriptor, cursor, retention_floor_seq) = match response {
        crate::protocol::core::CoreResponse::ProjectionSubscribed {
            subscription_id,
            descriptor,
            cursor,
            retention_floor_seq,
            ..
        } => (
            subscription_id.clone(),
            descriptor.clone(),
            cursor.clone(),
            *retention_floor_seq,
        ),
        crate::protocol::core::CoreResponse::ProjectionReplay {
            subscription_id: Some(subscription_id),
            batch,
        } => (
            subscription_id.clone(),
            batch.descriptor.clone(),
            batch.next_cursor.clone().unwrap_or(
                crate::protocol::projection::replay::ProjectionCursor {
                    stream_id: batch.descriptor.stream_id.clone(),
                    event_seq: batch.current_high_water,
                    projection_version: batch.descriptor.projection_version,
                },
            ),
            batch.descriptor.retention_floor_seq,
        ),
        _ => return false,
    };
    if projection.lock().await.owns(&subscription_id) {
        return true;
    }
    let Some(seam) = daemon.projection_seam.as_ref() else {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("core-ws-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    };
    let Some(mut rx) = seam
        .service()
        .take_subscription_receiver(&subscription_id)
        .await
    else {
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("core-ws-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    };
    let mut state = projection.lock().await;
    let owned = OwnedProjectionSubscription::new(
        subscription_id.clone(),
        descriptor.clone(),
        cursor,
        retention_floor_seq,
        state.reconnect_generation(),
    );
    let ready = owned.ready.clone();
    let cancellation = owned.cancellation.clone();
    if state.insert_subscription(owned).is_err() {
        drop(state);
        let _ = daemon
            .handle_request_for_client(
                crate::core::new_request(
                    format!("core-ws-projection-unsubscribe-{}", uuid::Uuid::new_v4()),
                    CoreRequest::ProjectionUnsubscribe {
                        subscription_id: subscription_id.clone(),
                    },
                ),
                client_id,
            )
            .await;
        return false;
    }
    let output = projection_tx.clone();
    let control_output = control_tx.clone();
    let projection_for_task = Arc::clone(projection);
    let sub_id = subscription_id.clone();
    let stream_id = descriptor.stream_id.clone();
    let lag_descriptor = descriptor.clone();
    let handle = tokio::spawn(async move {
        tokio::select! {
            _ = cancellation.cancelled() => return,
            _ = ready.notified() => {}
        }
        loop {
            let envelope = tokio::select! {
                _ = cancellation.cancelled() => break,
                envelope = rx.recv() => envelope,
            };
            let Some(envelope) = envelope else { break };
            let event_seq = envelope.event_seq;
            let projection_version = envelope.protocol_version;
            let event = CoreEvent::ProjectionStreamEvent {
                subscription_id: sub_id.clone(),
                stream_id: stream_id.clone(),
                envelope,
            };
            let frame = CoreFrame::Event(EventEnvelope {
                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                event_seq: 0,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                session_id: None,
                turn_id: None,
                payload: event,
            });
            let Ok(json) = serde_json::to_string(&frame) else {
                break;
            };
            if output
                .try_send(axum::extract::ws::Message::Text(json.into()))
                .is_err()
            {
                if let Some(subscription) =
                    projection_for_task.lock().await.subscription_mut(&sub_id)
                {
                    subscription.mark_resync_required();
                }
                let resync = CoreFrame::Response {
                    request_id: format!("projection-lag-{}", uuid::Uuid::new_v4()),
                    response: Box::new(crate::protocol::core::CoreResponse::ProjectionResyncRequired {
                        subscription_id: Some(sub_id.clone()),
                        reason: crate::protocol::projection::replay::ProjectionResyncReason::SubscriberLagged,
                        descriptor: Some(lag_descriptor.clone()),
                        requested_cursor: None,
                        snapshot: None,
                    }),
                };
                if let Ok(json) = serde_json::to_string(&resync) {
                    let _ = control_output.try_send(axum::extract::ws::Message::Text(json.into()));
                }
                break;
            }
            if let Some(subscription) = projection_for_task.lock().await.subscription_mut(&sub_id) {
                subscription.latest_cursor =
                    crate::protocol::projection::replay::ProjectionCursor {
                        stream_id: stream_id.clone(),
                        event_seq,
                        projection_version,
                    };
            }
        }
    });
    if let Some(subscription) = state.subscription_mut(&subscription_id) {
        subscription.forwarder = Some(handle);
    }
    true
}
