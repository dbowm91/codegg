//! ACP v1 stdio adapter.
//!
//! This module intentionally owns only ACP wire framing and translation.  The
//! daemon, session store, turn runtime, permissions, and projection reducer
//! remain the authorities for execution and state.

use crate::agent;
use crate::config::schema::Config;
use crate::core::instance::{connect_or_start_daemon, ConnectOrStartOptions};
use crate::core::transport::SocketCoreClient;
use crate::core::{new_request, CoreClient};
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope};
use crate::protocol::dto::{ContentPart, ProviderMessage};
use crate::protocol::projection::event::ProjectionEvent;
use crate::protocol::projection::replay::{ProjectionStreamKind, ProjectionSubscriptionRequest};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const ACP_PROTOCOL_VERSION: u64 = 1;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, serde::Deserialize)]
struct RpcRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Clone)]
struct ActivePrompt {
    request_id: Value,
    session_id: String,
    turn_id: Option<String>,
    cancelled: bool,
}

#[derive(Debug, Clone)]
struct SessionBinding {
    subscription_id: Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
    root: Option<PathBuf>,
}

pub async fn run() -> Result<(), crate::error::AppError> {
    let mut client: Option<Arc<SocketCoreClient>> = None;
    let (_event_sink, mut events) = mpsc::unbounded_channel::<EventEnvelope<CoreEvent>>();
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let mut sessions = HashMap::<String, SessionBinding>::new();
    let mut active: Option<ActivePrompt> = None;
    let mut initialized = false;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line.map_err(crate::error::AppError::Io)? else { break };
                if line.len() > MAX_FRAME_BYTES {
                    write_error(&stdout, Value::Null, -32600, "ACP frame exceeds the 1 MiB limit").await?;
                    continue;
                }
                let request = match serde_json::from_str::<RpcRequest>(&line) {
                    Ok(request) => request,
                    Err(error) => {
                        write_error(&stdout, Value::Null, -32700, &format!("invalid JSON: {error}")).await?;
                        continue;
                    }
                };
                if request.jsonrpc.as_deref() != Some("2.0") {
                    write_error(&stdout, request.id.unwrap_or(Value::Null), -32600, "jsonrpc must be 2.0").await?;
                    continue;
                }
                match request.method.as_str() {
                    "initialize" => {
                        let requested = request.params.get("protocolVersion").and_then(Value::as_u64).unwrap_or(0);
                        if requested != ACP_PROTOCOL_VERSION {
                            write_error(&stdout, request.id.unwrap_or(Value::Null), -32602, "only ACP protocol version 1 is supported").await?;
                        } else {
                            initialized = true;
                            write_result(&stdout, request.id.unwrap_or(Value::Null), json!({
                                "protocolVersion": ACP_PROTOCOL_VERSION,
                                "agentInfo": {"name": "codegg", "title": "CodeGG", "version": env!("CARGO_PKG_VERSION")},
                                "agentCapabilities": {
                                    "loadSession": true,
                                    "promptCapabilities": {"text": true, "image": false, "audio": false, "embeddedContext": false}
                                },
                                "authMethods": []
                            })).await?;
                        }
                    }
                    "session/new" if initialized => {
                        let client = match ensure_client(&mut client, &mut events).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        let cwd = match absolute_cwd(&request.params) {
                            Ok(cwd) => cwd,
                            Err(message) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32602, &message).await?; continue; }
                        };
                        let title = request.params.get("title").and_then(Value::as_str).map(str::to_owned);
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::SessionCreate {
                            directory: cwd.to_string_lossy().into_owned(), title, project_id: None, workspace_id: None,
                        })).await?;
                        match response {
                            CoreResponse::Session { session } => {
                                let id = session.id.clone();
                        let subscription = subscribe(&client, &id, None).await?;
                                sessions.insert(id.clone(), SessionBinding { subscription_id: subscription.1, root: Some(cwd) });
                                write_result(&stdout, request.id.unwrap_or(Value::Null), json!({"sessionId": id, "modes": {"currentModeId": "default", "availableModes": [{"id":"default", "name":"CodeGG", "description":"CodeGG standard agent"}]}})).await?;
                            }
                            other => write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?,
                        }
                    }
                    "session/load" | "session/resume" if initialized => {
                        let client = match ensure_client(&mut client, &mut events).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        let sid = session_id(&request.params)?;
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::SessionLoad { session_id: sid.clone() })).await?;
                        if matches!(response, CoreResponse::Session { .. }) {
                            let subscription = subscribe(&client, &sid, None).await?;
                            sessions.insert(sid.clone(), SessionBinding { subscription_id: subscription.1, root: None });
                            if request.method == "session/load" {
                                replay_snapshot(&stdout, &client, request.id.clone().unwrap_or(Value::Null), &sid).await?;
                            } else {
                                write_result(&stdout, request.id.unwrap_or(Value::Null), json!({"sessionId": sid})).await?;
                            }
                        } else { write_core_error(&stdout, request.id.unwrap_or(Value::Null), response).await?; }
                    }
                    "session/prompt" if initialized => {
                        let client = match ensure_client(&mut client, &mut events).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        if active.is_some() { write_error(&stdout, request.id.unwrap_or(Value::Null), -32000, "only one active ACP prompt is supported per connection").await?; continue; }
                        let sid = session_id(&request.params)?;
                        let text = prompt_text(&request.params)?;
                        let agents = native_agents(&sid, sessions.get(&sid).and_then(|b| b.root.as_deref()))?;
                        let model = request.params.get("model").and_then(Value::as_str).unwrap_or(agent::EMERGENCY_DEFAULT_MODEL).to_owned();
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnSubmit {
                            session_id: sid.clone(), text: text.clone(), plan_mode: false, model, agents, current_agent_idx: 0,
                            messages: vec![ProviderMessage::User { content: vec![ContentPart::Text { text }] }],
                        })).await?;
                        match response {
                            CoreResponse::Ack => active = Some(ActivePrompt { request_id: request.id.unwrap_or(Value::Null), session_id: sid, turn_id: None, cancelled: false }),
                            other => write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?,
                        }
                    }
                    "session/cancel" => {
                        let sid = session_id(&request.params)?;
                        if let (Some(client), Some(prompt)) = (client.as_ref(), active.as_ref().filter(|p| p.session_id == sid)) {
                            if let Some(turn_id) = &prompt.turn_id { let _ = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnCancel { session_id: sid, turn_id: turn_id.clone() })).await; }
                        }
                        write_result(&stdout, request.id.unwrap_or(Value::Null), json!({})).await?;
                    }
                    "session/close" => {
                        let sid = session_id(&request.params)?;
                        if let (Some(client), Some(prompt)) = (client.as_ref(), active.as_ref().filter(|p| p.session_id == sid)) {
                            if let Some(turn_id) = &prompt.turn_id { let _ = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnCancel { session_id: sid.clone(), turn_id: turn_id.clone() })).await; }
                        }
                        if let Some(binding) = sessions.remove(&sid) { if let (Some(client), Some(sub)) = (client.as_ref(), binding.subscription_id) { let _ = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::ProjectionUnsubscribe { subscription_id: sub })).await; } }
                        write_result(&stdout, request.id.unwrap_or(Value::Null), json!({})).await?;
                    }
                    "shutdown" => { write_result(&stdout, request.id.unwrap_or(Value::Null), Value::Null).await?; break; }
                    "exit" => break,
                    "$/cancel_request" => {
                        let cancelled_id = request.params.get("id").cloned();
                        if let Some(prompt) = active.as_mut().filter(|p| cancelled_id.as_ref() == Some(&p.request_id)) {
                            prompt.cancelled = true;
                            if let (Some(client), Some(turn_id)) = (client.as_ref(), prompt.turn_id.as_ref()) {
                                let _ = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnCancel { session_id: prompt.session_id.clone(), turn_id: turn_id.clone() })).await;
                            }
                        }
                    }
                    _ => write_error(&stdout, request.id.unwrap_or(Value::Null), -32601, "method not supported by CodeGG ACP v1").await?,
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                if let Some(prompt) = active.as_mut() {
                    let had_turn = prompt.turn_id.is_some();
                    handle_event(&stdout, prompt, &event).await?;
                    if prompt.cancelled && !had_turn {
                        if let Some(turn_id) = &prompt.turn_id {
                            if let Some(client) = client.as_ref() { let _ = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnCancel { session_id: prompt.session_id.clone(), turn_id: turn_id.clone() })).await; }
                        }
                    }
                    if prompt.turn_id.is_none() { continue; }
                }
                if let Some(prompt) = active.as_ref() { if event_is_terminal(&event, &prompt.session_id, prompt.turn_id.as_deref()) { let prompt = active.take().unwrap(); let reason = terminal_reason(&event); write_result(&stdout, prompt.request_id, json!({"stopReason": reason})).await?; } }
            }
        }
    }
    Ok(())
}

async fn ensure_client(
    client: &mut Option<Arc<SocketCoreClient>>,
    events: &mut mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>,
) -> Result<Arc<SocketCoreClient>, crate::error::AppError> {
    if let Some(client) = client {
        return Ok(Arc::clone(client));
    }
    let outcome = connect_or_start_daemon(ConnectOrStartOptions::for_default_paths())
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e.to_string())))?;
    let connected = Arc::new(outcome.client);
    *events = connected.subscribe();
    *client = Some(Arc::clone(&connected));
    Ok(connected)
}

fn absolute_cwd(params: &Value) -> Result<PathBuf, String> {
    let raw = params
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or("session/new requires cwd")?;
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("cwd must be absolute".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("cwd is not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err("cwd must be a directory".into());
    }
    Ok(canonical)
}

fn session_id(params: &Value) -> Result<String, crate::error::AppError> {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| crate::error::AppError::Other(anyhow::anyhow!("sessionId is required")))
}

fn prompt_text(params: &Value) -> Result<String, crate::error::AppError> {
    let prompt = params
        .get("prompt")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::error::AppError::Other(anyhow::anyhow!("prompt must be an array")))?;
    let mut out = String::new();
    for block in prompt {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "only text prompt blocks are supported"
            )));
        }
        let text = block.get("text").and_then(Value::as_str).ok_or_else(|| {
            crate::error::AppError::Other(anyhow::anyhow!("text block is missing text"))
        })?;
        if out.len() + text.len() > MAX_FRAME_BYTES {
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "prompt exceeds the 1 MiB limit"
            )));
        }
        out.push_str(text);
    }
    Ok(out)
}

fn native_agents(
    _session_id: &str,
    project_root: Option<&Path>,
) -> Result<Vec<crate::protocol::dto::Agent>, crate::error::AppError> {
    let config = Config::load().unwrap_or_default();
    let agents = agent::resolve_agents_with_context(&config, project_root)
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e.to_string())))?;
    agents
        .into_iter()
        .map(|a| {
            serde_json::from_value(serde_json::to_value(a).unwrap_or_default())
                .map_err(crate::error::AppError::Json)
        })
        .collect()
}

async fn subscribe(
    client: &Arc<SocketCoreClient>,
    session_id: &str,
    cursor: Option<crate::protocol::projection::replay::ProjectionCursor>,
) -> Result<
    (
        Option<crate::protocol::projection::replay::ProjectionCursor>,
        Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
    ),
    crate::error::AppError,
> {
    let response = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::ProjectionSubscribe {
                request: ProjectionSubscriptionRequest {
                    scope: ProjectionStreamKind::Session,
                    scope_id: session_id.to_owned(),
                    cursor,
                    projection_version: 1,
                },
            },
        ))
        .await?;
    match response {
        CoreResponse::ProjectionSubscribed {
            subscription_id,
            cursor,
            ..
        } => Ok((Some(cursor), Some(subscription_id))),
        CoreResponse::Error { code, message } => Err(crate::error::AppError::Other(
            anyhow::anyhow!("{code}: {message}"),
        )),
        _ => Ok((None, None)),
    }
}

async fn replay_snapshot(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    client: &Arc<SocketCoreClient>,
    id: Value,
    sid: &str,
) -> Result<(), crate::error::AppError> {
    let response = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::SessionMessagesLoad {
                session_id: sid.to_owned(),
            },
        ))
        .await?;
    if let CoreResponse::SessionMessages { messages, .. } = response {
        for message in messages {
            for part in message.data.parts {
                if let crate::protocol::dto::PartData::Text { text } = part.data {
                    update(stdout, sid, json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}})).await?;
                }
            }
        }
    }
    write_result(stdout, id, json!({"sessionId": sid})).await
}

async fn handle_event(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    prompt: &mut ActivePrompt,
    event: &EventEnvelope<CoreEvent>,
) -> Result<(), crate::error::AppError> {
    if event.session_id.as_deref() != Some(prompt.session_id.as_str()) {
        return Ok(());
    }
    if let CoreEvent::ProjectionStreamEvent { envelope, .. } = &event.payload {
        if prompt.turn_id.is_none() {
            prompt.turn_id = envelope.turn_id.clone();
        }
        match &envelope.payload {
            ProjectionEvent::MessageAppended { message } if message.visibility == crate::protocol::projection::dto::VisibilityClass::Public => update(stdout, &prompt.session_id, json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":message.text}})).await?,
            ProjectionEvent::ToolStarted { tool } => update(stdout, &prompt.session_id, json!({"sessionUpdate":"tool_call","toolCallId":tool.tool_id,"title":tool.tool_name,"status":"in_progress"})).await?,
            ProjectionEvent::ToolCompleted { tool_id, output, success, .. } => update(stdout, &prompt.session_id, json!({"sessionUpdate":"tool_call_update","toolCallId":tool_id,"status":if *success {"completed"} else {"failed"},"rawOutput":format!("{output:?}")})).await?,
            _ => {}
        }
    } else if let CoreEvent::TurnStarted { turn_id, .. } = &event.payload {
        prompt.turn_id = Some(turn_id.clone());
    }
    Ok(())
}

fn event_is_terminal(event: &EventEnvelope<CoreEvent>, session: &str, turn: Option<&str>) -> bool {
    if event.session_id.as_deref() != Some(session) {
        return false;
    }
    match &event.payload {
        CoreEvent::TurnCompleted { turn_id, .. } => turn.map(|t| t == turn_id).unwrap_or(true),
        CoreEvent::TurnFailed { turn_id, .. } => {
            turn.map(|t| Some(t) == turn_id.as_deref()).unwrap_or(true)
        }
        CoreEvent::ProjectionStreamEvent { envelope, .. } => matches!(
            envelope.payload,
            ProjectionEvent::TurnCompleted { .. } | ProjectionEvent::TurnFailed { .. }
        ),
        _ => false,
    }
}

fn terminal_reason(event: &EventEnvelope<CoreEvent>) -> &'static str {
    match &event.payload {
        CoreEvent::TurnCompleted { .. } => "end_turn",
        CoreEvent::TurnFailed { .. } => "cancelled",
        CoreEvent::ProjectionStreamEvent { envelope, .. } => match envelope.payload {
            ProjectionEvent::TurnCompleted { .. } => "end_turn",
            ProjectionEvent::TurnFailed { .. } => "cancelled",
            _ => "end_turn",
        },
        _ => "end_turn",
    }
}

async fn update(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    sid: &str,
    update: Value,
) -> Result<(), crate::error::AppError> {
    write_notification(
        stdout,
        "session/update",
        json!({"sessionId":sid,"update":update}),
    )
    .await
}
async fn write_result(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    id: Value,
    result: Value,
) -> Result<(), crate::error::AppError> {
    write_frame(stdout, json!({"jsonrpc":"2.0","id":id,"result":result})).await
}
async fn write_error(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    id: Value,
    code: i64,
    message: &str,
) -> Result<(), crate::error::AppError> {
    write_frame(
        stdout,
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
    )
    .await
}
async fn write_notification(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    method: &str,
    params: Value,
) -> Result<(), crate::error::AppError> {
    write_frame(
        stdout,
        json!({"jsonrpc":"2.0","method":method,"params":params}),
    )
    .await
}
async fn write_frame(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    frame: Value,
) -> Result<(), crate::error::AppError> {
    let text = serde_json::to_string(&frame).map_err(crate::error::AppError::Json)?;
    let mut out = stdout.lock().await;
    out.write_all(text.as_bytes())
        .await
        .map_err(crate::error::AppError::Io)?;
    out.write_all(b"\n")
        .await
        .map_err(crate::error::AppError::Io)?;
    out.flush().await.map_err(crate::error::AppError::Io)
}
async fn write_core_error(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    id: Value,
    response: CoreResponse,
) -> Result<(), crate::error::AppError> {
    match response {
        CoreResponse::Error { code, message } => {
            write_error(stdout, id, -32000, &format!("{code}: {message}")).await
        }
        _ => write_error(stdout, id, -32000, "native daemon rejected the request").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_missing_cwd() {
        assert!(absolute_cwd(&json!({"cwd":"relative"})).is_err());
        assert!(absolute_cwd(&json!({})).is_err());
    }

    #[test]
    fn accepts_text_prompt_and_rejects_non_text() {
        let text = prompt_text(&json!({"prompt":[{"type":"text","text":"hello"}]})).unwrap();
        assert_eq!(text, "hello");
        assert!(prompt_text(&json!({"prompt":[{"type":"image","data":"x"}]})).is_err());
    }

    #[test]
    fn rejects_oversized_prompt() {
        let text = "x".repeat(MAX_FRAME_BYTES + 1);
        assert!(prompt_text(&json!({"prompt":[{"type":"text","text":text}]})).is_err());
    }
}
