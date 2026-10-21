//! ACP v1 stdio adapter.
//!
//! This module intentionally owns only ACP wire framing and translation.  The
//! daemon, session store, turn runtime, permissions, and projection reducer
//! remain the authorities for execution and state.

use crate::agent;
use crate::core::instance::{connect_or_start_daemon, ConnectOrStartOptions};
use crate::core::transport::SocketCoreClient;
use crate::core::{new_request, CoreClient};
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope};
use crate::protocol::dto::{ContentPart, ProjectRegisterRequestDto, ProviderMessage};
use crate::protocol::projection::dto::ToolArgumentProjection;
use crate::protocol::projection::event::ProjectionEvent;
use crate::protocol::projection::replay::{ProjectionStreamKind, ProjectionSubscriptionRequest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const ACP_PROTOCOL_VERSION: u64 = 1;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// The JSON-RPC error code for a native turn failure.
///
/// Distinct from the `-32011` permission-pending code: `-32010` means the
/// native turn itself failed, `-32011` means the headless turn reached a
/// permission request nobody answered.
const ACP_TURN_FAILED_CODE: i64 = -32010;

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
    submission_event_floor: u64,
    projection_event_floor: u64,
    projection_subscription_id:
        Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
    turn_id: Option<String>,
    cancel_requested: bool,
    close_requested: bool,
    cancel_sent: bool,
}

impl ActivePrompt {
    fn cancel(&mut self, close: bool) {
        self.cancel_requested = true;
        self.close_requested |= close;
    }

    fn can_accept(&self, event: &EventEnvelope<CoreEvent>) -> bool {
        event.event_seq > self.submission_event_floor
            && event.session_id.as_deref() == Some(self.session_id.as_str())
    }

    fn bind_turn(&mut self, turn_id: &str) -> bool {
        if turn_id.is_empty() {
            return false;
        }
        match self.turn_id.as_deref() {
            Some(existing) => existing == turn_id,
            None => {
                self.turn_id = Some(turn_id.to_owned());
                true
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SessionBinding {
    subscription_id: Option<crate::protocol::projection::replay::ProjectionSubscriptionId>,
    cursor: Option<crate::protocol::projection::replay::ProjectionCursor>,
    root: Option<PathBuf>,
}

pub async fn run(
    policy: Option<crate::policy_surface::CliPolicyOverride>,
) -> Result<(), crate::error::AppError> {
    let mut client: Option<Arc<SocketCoreClient>> = None;
    let mut policy_applied = false;
    let mut policy_lease: Option<AcpPolicyLease> = None;
    let mut policy_snapshot: Option<crate::protocol::core::ExecutionPolicySnapshotDto> = None;
    let (_event_sink, mut events) = mpsc::channel::<EventEnvelope<CoreEvent>>(256);
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
                        let client = match ensure_client(&mut client, &mut events, policy, &mut policy_applied, &mut policy_lease, &mut policy_snapshot).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        let cwd = match absolute_cwd(&request.params) {
                            Ok(cwd) => cwd,
                            Err(message) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32602, &message).await?; continue; }
                        };
                        let title = request.params.get("title").and_then(Value::as_str).map(str::to_owned);
                        let workspace = client.request(new_request(
                            uuid::Uuid::new_v4().to_string(),
                            CoreRequest::WorkspaceRegister {
                                root: cwd.to_string_lossy().into_owned(),
                            },
                        )).await?;
                        let workspace_id = match workspace {
                            CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
                            other => {
                                write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?;
                                continue;
                            }
                        };
                        let display_name = cwd.file_name().map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| cwd.to_string_lossy().into_owned());
                        let project = client.request(new_request(
                            uuid::Uuid::new_v4().to_string(),
                            CoreRequest::ProjectRegister {
                                request: ProjectRegisterRequestDto {
                                    workspace_id: workspace_id.clone(),
                                    display_name,
                                    description: None,
                                    tags: Vec::new(),
                                    repository_id: None,
                                    source: "acp".to_string(),
                                },
                            },
                        )).await?;
                        let project_id = match project {
                            CoreResponse::ProjectRegistered { project } => project.project_id,
                            other => {
                                write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?;
                                continue;
                            }
                        };
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::SessionCreate {
                            directory: cwd.to_string_lossy().into_owned(), title, project_id: Some(project_id), workspace_id: Some(workspace_id),
                        })).await?;
                        match response {
                            CoreResponse::Session { session } => {
                                let id = session.id.clone();
                                client.subscribe_session_events(id.clone(), None).await?;
                        let subscription = subscribe(&client, &id, None).await?;
                                sessions.insert(id.clone(), SessionBinding { subscription_id: subscription.1, cursor: subscription.0, root: Some(cwd) });
                                let mut result = json!({"sessionId": id, "modes": {"currentModeId": "default", "availableModes": [{"id":"default", "name":"CodeGG", "description":"CodeGG standard agent"}]}});
                                if let (Some(requested), Some(snapshot)) = (policy, policy_snapshot.as_ref()) {
                                    result["codeggPolicy"] = json!({
                                        "requested": {
                                            "approvalMode": requested.approval_mode.map(|value| value.as_str()),
                                            "sandbox": requested.sandbox_profile.map(|value| value.as_str()),
                                        },
                                        "effective": {
                                            "approvalMode": snapshot.approval_mode.as_str(),
                                            "sandbox": snapshot.sandbox_profile.as_str(),
                                            "revision": snapshot.policy_revision,
                                        },
                                    });
                                }
                                write_result(&stdout, request.id.unwrap_or(Value::Null), result).await?;
                            }
                            other => write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?,
                        }
                    }
                    "session/load" | "session/resume" if initialized => {
                        let client = match ensure_client(&mut client, &mut events, policy, &mut policy_applied, &mut policy_lease, &mut policy_snapshot).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        let sid = session_id(&request.params)?;
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::SessionLoad { session_id: sid.clone() })).await?;
                        if matches!(response, CoreResponse::Session { .. }) {
                            client.subscribe_session_events(sid.clone(), None).await?;
                            let subscription = subscribe(&client, &sid, None).await?;
                            sessions.insert(sid.clone(), SessionBinding { subscription_id: subscription.1, cursor: subscription.0, root: None });
                            if request.method == "session/load" {
                                replay_snapshot(&stdout, request.id.clone().unwrap_or(Value::Null), &sid, subscription.2).await?;
                            } else {
                                write_result(&stdout, request.id.unwrap_or(Value::Null), json!({"sessionId": sid})).await?;
                            }
                        } else { write_core_error(&stdout, request.id.unwrap_or(Value::Null), response).await?; }
                    }
                    "session/prompt" if initialized => {
                        let client = match ensure_client(&mut client, &mut events, policy, &mut policy_applied, &mut policy_lease, &mut policy_snapshot).await {
                            Ok(client) => client,
                            Err(error) => { write_error(&stdout, request.id.unwrap_or(Value::Null), -32001, &error.to_string()).await?; continue; }
                        };
                        if active.is_some() { write_error(&stdout, request.id.unwrap_or(Value::Null), -32000, "only one active ACP prompt is supported per connection").await?; continue; }
                        let sid = session_id(&request.params)?;
                        let text = prompt_text(&request.params)?;
                        let agents = native_agents(&sid, sessions.get(&sid).and_then(|b| b.root.as_deref()))?;
                        let requested_model = request.params.get("model").and_then(Value::as_str);
                        let requested_agent = request.params.get("codeggAgent").and_then(Value::as_str);
                        if policy.is_some() && (requested_model.is_none() || requested_agent.is_none()) {
                            write_error(&stdout, request.id.unwrap_or(Value::Null), -32602, "unattended ACP trials require explicit model and codeggAgent fields").await?;
                            continue;
                        }
                        let model = requested_model.unwrap_or(agent::EMERGENCY_DEFAULT_MODEL).to_owned();
                        let projection_event_floor = sessions.get(&sid)
                            .and_then(|binding| binding.cursor.as_ref())
                            .map(|cursor| cursor.event_seq)
                            .unwrap_or(0);
                        let projection_subscription_id = sessions.get(&sid)
                            .and_then(|binding| binding.subscription_id.clone());
                        let current_agent_idx = match requested_agent {
                            Some(name) => match agents.iter().position(|agent| agent.name == name) {
                                Some(index) => index,
                                None => {
                                    write_error(&stdout, request.id.unwrap_or(Value::Null), -32602, "requested CodeGG agent is unavailable").await?;
                                    continue;
                                }
                            },
                            None => 0,
                        };
                        tracing::info!(model = %model, agent = %agents.get(current_agent_idx).map(|agent| agent.name.as_str()).unwrap_or("unknown"), "selected ACP turn identity");
                        let submission_event_floor = drain_event_floor(&mut events);
                        let response = client.request(new_request(uuid::Uuid::new_v4().to_string(), CoreRequest::TurnSubmit {
                            session_id: sid.clone(), text: text.clone(), plan_mode: false, model, agents, current_agent_idx,
                            messages: vec![ProviderMessage::User { content: vec![ContentPart::Text { text }] }],
                            reasoning: None,
                            human_shell_promotion_ids: Vec::new(),
                        })).await?;
                        match response {
                            CoreResponse::Ack => active = Some(ActivePrompt {
                                request_id: request.id.unwrap_or(Value::Null),
                                session_id: sid,
                                submission_event_floor,
                                projection_event_floor,
                                projection_subscription_id,
                                turn_id: None,
                                cancel_requested: false,
                                close_requested: false,
                                cancel_sent: false,
                            }),
                            CoreResponse::TurnAdmissionReceipt { receipt }
                                if receipt.status == "accepted" =>
                            {
                                active = Some(ActivePrompt {
                                    request_id: request.id.unwrap_or(Value::Null),
                                    session_id: sid,
                                    submission_event_floor,
                                    projection_event_floor,
                                    projection_subscription_id,
                                    turn_id: Some(receipt.turn_id),
                                    cancel_requested: false,
                                    close_requested: false,
                                    cancel_sent: false,
                                })
                            }
                            CoreResponse::TurnAdmissionReceipt { receipt } => {
                                write_error(
                                    &stdout,
                                    request.id.unwrap_or(Value::Null),
                                    -32002,
                                    &format!("turn admission was not accepted ({})", receipt.status),
                                )
                                .await?;
                            }
                            other => write_core_error(&stdout, request.id.unwrap_or(Value::Null), other).await?,
                        }
                    }
                    "session/cancel" => {
                        let sid = session_id(&request.params)?;
                        if let Some(prompt) = active.as_mut().filter(|p| p.session_id == sid) {
                            prompt.cancel(false);
                            cancel_if_ready(client.as_ref(), prompt).await;
                        }
                        write_result(&stdout, request.id.unwrap_or(Value::Null), json!({})).await?;
                    }
                    "session/close" => {
                        let sid = session_id(&request.params)?;
                        if let Some(prompt) = active.as_mut().filter(|p| p.session_id == sid) {
                            prompt.cancel(true);
                            cancel_if_ready(client.as_ref(), prompt).await;
                        }
                        if let Some(binding) = sessions.remove(&sid) {
                            if let (Some(client), Some(sub)) =
                                (client.as_ref(), binding.subscription_id)
                            {
                                let request = new_request(
                                    uuid::Uuid::new_v4().to_string(),
                                    CoreRequest::ProjectionUnsubscribe {
                                        subscription_id: sub,
                                    },
                                );
                                if let Err(error) = client.request(request).await {
                                    tracing::debug!(%error, "projection unsubscribe failed during session close");
                                }
                            }
                        }
                        write_result(&stdout, request.id.unwrap_or(Value::Null), json!({})).await?;
                    }
                    "shutdown" => { write_result(&stdout, request.id.unwrap_or(Value::Null), Value::Null).await?; break; }
                    "exit" => break,
                    "$/cancel_request" => {
                        let cancelled_id = request.params.get("id").cloned();
                        if let Some(prompt) = active.as_mut().filter(|p| cancelled_id.as_ref() == Some(&p.request_id)) {
                            prompt.cancel(false);
                            cancel_if_ready(client.as_ref(), prompt).await;
                        }
                    }
                    _ => write_error(&stdout, request.id.unwrap_or(Value::Null), -32601, "method not supported by CodeGG ACP v1").await?,
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                if let Some(prompt) = active.as_mut() {
                    let accepted = handle_event(&stdout, prompt, &event).await?;
                    if accepted {
                        cancel_if_ready(client.as_ref(), prompt).await;
                    }
                }
                if active.as_ref().is_some_and(|prompt| event_is_permission_pending(&event, prompt)) {
                    let mut prompt = active.take().expect("permission event matched active prompt");
                    prompt.cancel(false);
                    cancel_if_ready(client.as_ref(), &mut prompt).await;
                    write_error_with_data(
                        &stdout,
                        prompt.request_id,
                        -32011,
                        "headless ACP turn reached an unanswered permission request",
                        json!({"kind":"permission_pending", "sessionId":prompt.session_id, "turnId":prompt.turn_id}),
                    ).await?;
                    continue;
                }
                if let Some(prompt) = active.as_ref() {
                    if event_is_terminal(&event, prompt) {
                        let prompt = active.take().unwrap();
                        match terminal_stop_reason(&event) {
                            Some(completed_reason) => {
                                write_result(&stdout, prompt.request_id, json!({"stopReason": completed_reason,
                                    "codeggUsage": terminal_usage(&event)})).await?;
                            }
                            None => {
                                let failure_data = terminal_failure_data(&prompt, &event);
                                write_error_with_data(
                                    &stdout,
                                    prompt.request_id,
                                    ACP_TURN_FAILED_CODE,
                                    "native turn failed",
                                    failure_data,
                                ).await?;
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(prompt) = active.as_mut() {
        prompt.cancel(true);
        cancel_if_ready(client.as_ref(), prompt).await;
    }
    if let Some(client) = client.as_ref() {
        for binding in sessions.into_values() {
            if let Some(subscription_id) = binding.subscription_id {
                let _ = client
                    .request(new_request(
                        uuid::Uuid::new_v4().to_string(),
                        CoreRequest::ProjectionUnsubscribe { subscription_id },
                    ))
                    .await;
            }
        }
    }
    Ok(())
}

fn drain_event_floor(events: &mut mpsc::Receiver<EventEnvelope<CoreEvent>>) -> u64 {
    let mut floor = 0;
    while let Ok(event) = events.try_recv() {
        floor = floor.max(event.event_seq);
    }
    floor
}

async fn cancel_if_ready(client: Option<&Arc<SocketCoreClient>>, prompt: &mut ActivePrompt) {
    if !prompt.cancel_requested || prompt.cancel_sent {
        return;
    }
    let Some(turn_id) = prompt.turn_id.clone() else {
        return;
    };
    let Some(client) = client else { return };
    prompt.cancel_sent = true;
    let _ = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::TurnCancel {
                session_id: prompt.session_id.clone(),
                turn_id,
            },
        ))
        .await;
}

async fn ensure_client(
    client: &mut Option<Arc<SocketCoreClient>>,
    events: &mut mpsc::Receiver<EventEnvelope<CoreEvent>>,
    policy: Option<crate::policy_surface::CliPolicyOverride>,
    policy_applied: &mut bool,
    policy_lease: &mut Option<AcpPolicyLease>,
    policy_snapshot: &mut Option<crate::protocol::core::ExecutionPolicySnapshotDto>,
) -> Result<Arc<SocketCoreClient>, crate::error::AppError> {
    if let Some(client) = client {
        return Ok(Arc::clone(client));
    }
    if policy.is_some() && !*policy_applied {
        *policy_lease = Some(acquire_policy_lease()?);
    }
    let outcome = connect_or_start_daemon(ConnectOrStartOptions::for_default_paths())
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!(e.to_string())))?;
    let connected = Arc::new(outcome.client);
    *events = connected.subscribe();
    if let Some(policy) = policy {
        if !*policy_applied {
            *policy_snapshot = Some(apply_and_verify_policy(&connected, policy).await?);
            *policy_applied = true;
        }
    }
    *client = Some(Arc::clone(&connected));
    Ok(connected)
}

#[derive(Debug)]
struct AcpPolicyLease {
    _file: File,
}

fn acquire_policy_lease() -> Result<AcpPolicyLease, crate::error::AppError> {
    let root = std::env::var_os("CODEGG_DAEMON_HOME").ok_or_else(|| {
        crate::error::AppError::Other(anyhow::anyhow!(
            "explicit ACP policy overrides require a private CODEGG_DAEMON_HOME; refusing to mutate the default user daemon"
        ))
    })?;
    let root = PathBuf::from(root);
    std::fs::create_dir_all(&root)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("acp-policy-lease.lock"))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "another ACP policy trial is already active for this daemon root"
            )));
        }
        Err(std::fs::TryLockError::Error(error)) => {
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "could not lock ACP policy trial admission: {error}"
            )));
        }
    }
    Ok(AcpPolicyLease { _file: file })
}

async fn apply_and_verify_policy(
    client: &Arc<SocketCoreClient>,
    policy: crate::policy_surface::CliPolicyOverride,
) -> Result<crate::protocol::core::ExecutionPolicySnapshotDto, crate::error::AppError> {
    use crate::protocol::core::{ApprovalModeDto, SandboxProfileDto};
    let (Some(approval_mode), Some(sandbox_profile)) =
        (policy.approval_mode, policy.sandbox_profile)
    else {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "ACP unattended policy requires both --approval-mode yolo and --sandbox full-host|workspace-write"
        )));
    };
    if approval_mode != codegg_core::approval::ApprovalMode::Yolo
        || !matches!(
            sandbox_profile,
            codegg_core::approval::SandboxProfile::FullHost
                | codegg_core::approval::SandboxProfile::WorkspaceWrite
        )
    {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "ACP unattended policy requires yolo with full-host or workspace-write"
        )));
    }
    let current_preference = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::ApprovalPreferenceGet,
        ))
        .await?;
    let expected_revision = match current_preference {
        CoreResponse::ApprovalPreference { preference } => Some(preference.revision),
        CoreResponse::Error { code, .. } if code == "preference_unavailable" => None,
        _ => {
            return Err(crate::error::AppError::Other(anyhow::anyhow!(
                "daemon could not read the current ACP policy revision"
            )));
        }
    };
    let set = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::RuntimePolicySet {
                approval_mode: Some(approval_mode.as_str().to_string()),
                sandbox_profile: Some(sandbox_profile.as_str().to_string()),
                expected_revision,
            },
        ))
        .await?;
    if !matches!(set, CoreResponse::ApprovalPreference { .. }) {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "daemon rejected explicit ACP runtime policy"
        )));
    }
    let effective = client
        .request(new_request(
            uuid::Uuid::new_v4().to_string(),
            CoreRequest::ExecutionPolicyGet { session_id: None },
        ))
        .await?;
    let CoreResponse::ExecutionPolicy { snapshot } = effective else {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "daemon did not return an effective ACP policy snapshot"
        )));
    };
    let expected_mode = match approval_mode {
        codegg_core::approval::ApprovalMode::Interactive => ApprovalModeDto::Interactive,
        codegg_core::approval::ApprovalMode::Automatic => ApprovalModeDto::Automatic,
        codegg_core::approval::ApprovalMode::Yolo => ApprovalModeDto::Yolo,
    };
    let expected_profile = match sandbox_profile {
        codegg_core::approval::SandboxProfile::ReadOnly => SandboxProfileDto::ReadOnly,
        codegg_core::approval::SandboxProfile::WorkspaceWrite => SandboxProfileDto::WorkspaceWrite,
        codegg_core::approval::SandboxProfile::FullHost => SandboxProfileDto::FullHost,
    };
    if snapshot.approval_mode != expected_mode || snapshot.sandbox_profile != expected_profile {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "daemon narrowed explicit ACP policy (requested approval={} sandbox={}, effective approval={} sandbox={}); refusing the trial",
            expected_mode.as_str(), expected_profile.as_str(), snapshot.approval_mode.as_str(), snapshot.sandbox_profile.as_str()
        )));
    }
    tracing::info!(
        requested_approval = expected_mode.as_str(),
        effective_approval = snapshot.approval_mode.as_str(),
        requested_sandbox = expected_profile.as_str(),
        effective_sandbox = snapshot.sandbox_profile.as_str(),
        policy_revision = ?snapshot.policy_revision,
        "verified daemon ACP policy"
    );
    Ok(snapshot)
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
    let config = crate::core::load_config_or_default();
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
        Option<crate::protocol::projection::replay::ProjectionSnapshotBundle>,
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
            snapshot,
            ..
        } => Ok((Some(cursor), Some(subscription_id), Some(snapshot))),
        CoreResponse::Error { code, message } => Err(crate::error::AppError::Other(
            anyhow::anyhow!("{code}: {message}"),
        )),
        other => Err(crate::error::AppError::Other(anyhow::anyhow!(
            "unexpected projection subscription response: {other:?}"
        ))),
    }
}

async fn replay_snapshot(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    id: Value,
    sid: &str,
    snapshot: Option<crate::protocol::projection::replay::ProjectionSnapshotBundle>,
) -> Result<(), crate::error::AppError> {
    let Some(crate::protocol::projection::replay::ProjectionSnapshotBundle::One { snapshot }) =
        snapshot
    else {
        return write_result(stdout, id, json!({"sessionId": sid})).await;
    };
    let mut turns: Vec<_> = snapshot.recent_turns.iter().cloned().collect();
    if let Some(active) = snapshot.active_turn.clone() {
        turns.push(active);
    }
    turns.sort_by_key(|turn| turn.started_at);
    for turn in turns {
        for message in turn.messages {
            if message.visibility != crate::protocol::projection::dto::VisibilityClass::Public {
                continue;
            }
            let session_update = match message.role {
                crate::protocol::projection::dto::MessageRole::User => "user_message_chunk",
                crate::protocol::projection::dto::MessageRole::Assistant => "agent_message_chunk",
                crate::protocol::projection::dto::MessageRole::Tool
                | crate::protocol::projection::dto::MessageRole::System
                | crate::protocol::projection::dto::MessageRole::Reasoning => continue,
            };
            update(
                stdout,
                sid,
                json!({
                    "sessionUpdate": session_update,
                    "content": {"type": "text", "text": message.text}
                }),
            )
            .await?;
        }
    }
    write_result(stdout, id, json!({"sessionId": sid})).await
}

async fn handle_event(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    prompt: &mut ActivePrompt,
    event: &EventEnvelope<CoreEvent>,
) -> Result<bool, crate::error::AppError> {
    let is_projection = matches!(event.payload, CoreEvent::ProjectionStreamEvent { .. });
    if is_projection {
        if !projection_event_matches(event, prompt) {
            return Ok(false);
        }
    } else if !prompt.can_accept(event) {
        return Ok(false);
    }
    if let CoreEvent::ProjectionStreamEvent { envelope, .. } = &event.payload {
        let Some(turn_id) = envelope.turn_id.as_deref() else {
            return Ok(false);
        };
        if !prompt.bind_turn(turn_id) {
            return Ok(false);
        }
        if prompt.close_requested {
            return Ok(true);
        }
        match &envelope.payload {
            ProjectionEvent::MessageAppended { message } if message.visibility == crate::protocol::projection::dto::VisibilityClass::Public => update(stdout, &prompt.session_id, json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":message.text}})).await?,
            ProjectionEvent::ToolStarted { tool } => update(stdout, &prompt.session_id, json!({"sessionUpdate":"tool_call","toolCallId":tool.tool_id,"title":tool.tool_name,"status":"in_progress","codeggArgumentsSha256":tool_arguments_sha256(&tool.arguments)})).await?,
            ProjectionEvent::ToolCompleted { tool_id, output, success, .. } => update(stdout, &prompt.session_id, json!({"sessionUpdate":"tool_call_update","toolCallId":tool_id,"status":if *success {"completed"} else {"failed"},"rawOutput":format!("{output:?}")})).await?,
            _ => {}
        }
    } else if let CoreEvent::TurnStarted { turn_id, .. } = &event.payload {
        if !prompt.bind_turn(turn_id) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn event_is_terminal(event: &EventEnvelope<CoreEvent>, prompt: &ActivePrompt) -> bool {
    match &event.payload {
        CoreEvent::TurnCompleted { turn_id, .. } => {
            prompt.can_accept(event)
                && event.session_id.as_deref() == Some(prompt.session_id.as_str())
                && prompt.turn_id.as_deref() == Some(turn_id.as_str())
        }
        CoreEvent::TurnFailed { turn_id, .. } => {
            prompt.can_accept(event)
                && event.session_id.as_deref() == Some(prompt.session_id.as_str())
                && turn_id.as_deref() == prompt.turn_id.as_deref()
        }
        CoreEvent::ProjectionStreamEvent { envelope, .. } => match (
            projection_event_matches(event, prompt),
            envelope.turn_id.as_deref(),
            &envelope.payload,
        ) {
            (true, Some(event_turn), ProjectionEvent::TurnCompleted { turn_id, .. })
            | (true, Some(event_turn), ProjectionEvent::TurnFailed { turn_id, .. }) => {
                prompt.turn_id.as_deref() == Some(event_turn) && turn_id == event_turn
            }
            _ => false,
        },
        _ => false,
    }
}

fn projection_event_matches(event: &EventEnvelope<CoreEvent>, prompt: &ActivePrompt) -> bool {
    let CoreEvent::ProjectionStreamEvent {
        subscription_id,
        envelope,
        ..
    } = &event.payload
    else {
        return false;
    };
    let Some(expected_subscription) = prompt.projection_subscription_id.as_ref() else {
        return false;
    };
    subscription_id == expected_subscription
        && envelope.session_id.as_deref() == Some(prompt.session_id.as_str())
        && envelope.event_seq > prompt.projection_event_floor
        && envelope.turn_id.as_deref().is_some_and(|turn| {
            prompt
                .turn_id
                .as_deref()
                .is_none_or(|expected| expected == turn)
        })
}

fn event_is_permission_pending(event: &EventEnvelope<CoreEvent>, prompt: &ActivePrompt) -> bool {
    if !prompt.can_accept(event) || event.session_id.as_deref() != Some(prompt.session_id.as_str())
    {
        return false;
    }
    match &event.payload {
        CoreEvent::PermissionPending {
            session_id,
            turn_id: Some(turn_id),
            ..
        } => {
            session_id == &prompt.session_id && prompt.turn_id.as_deref() == Some(turn_id.as_str())
        }
        _ => false,
    }
}

fn terminal_stop_reason(event: &EventEnvelope<CoreEvent>) -> Option<&'static str> {
    match &event.payload {
        CoreEvent::TurnCompleted { stop_reason, .. } => Some(map_acp_stop_reason(stop_reason)),
        CoreEvent::TurnFailed { .. } => None,
        CoreEvent::ProjectionStreamEvent { envelope, .. } => match &envelope.payload {
            ProjectionEvent::TurnCompleted { stop_reason, .. } => {
                Some(map_acp_stop_reason(stop_reason))
            }
            ProjectionEvent::TurnFailed { .. } => None,
            _ => None,
        },
        _ => None,
    }
}

fn terminal_usage(event: &EventEnvelope<CoreEvent>) -> Value {
    let usage = match &event.payload {
        CoreEvent::TurnCompleted {
            turn_id,
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
            context_limit,
            ..
        } => Some((
            turn_id.as_str(),
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
            context_limit,
        )),
        CoreEvent::ProjectionStreamEvent { envelope, .. } => match &envelope.payload {
            ProjectionEvent::TurnCompleted {
                turn_id,
                input_tokens,
                output_tokens,
                cached_tokens,
                reasoning_tokens,
                context_limit,
                ..
            } => Some((
                turn_id.as_str(),
                input_tokens,
                output_tokens,
                cached_tokens,
                reasoning_tokens,
                context_limit,
            )),
            _ => None,
        },
        _ => None,
    };
    match usage {
        Some((turn_id, input, output, cached, reasoning, context)) => json!({
            "turnId": turn_id,
            "inputTokens": input,
            "outputTokens": output,
            "cachedTokens": cached,
            "reasoningTokens": reasoning,
            "contextLimit": context,
        }),
        None => Value::Null,
    }
}

/// Build the `-32010` JSON-RPC error `data` payload for a failed turn.
///
/// The payload carries the redacted failure class and the usage
/// provenance, and nothing else. It NEVER carries
/// `CoreEvent::TurnFailed.message`: that message embeds upstream provider
/// text and can carry credentials, so it stays in the local transcript.
///
/// The usage provenance distinguishes four facts that are easy to
/// conflate:
///
/// - `reported` — the provider reported token counts
/// - `not_observed` — the turn ended before any provider response
/// - `not_provided` — a provider response arrived but reported no tokens
/// - `lost_at_projection` — the native layer had usage, but this event
///   came through the projection stream, which does not carry it
/// - `unspecified` — availability could not be established
fn terminal_failure_data(prompt: &ActivePrompt, event: &EventEnvelope<CoreEvent>) -> Value {
    use crate::protocol::core::{TurnFailureCategory, UsageAvailability};

    let (category, usage, absent_reason) = match &event.payload {
        CoreEvent::TurnFailed {
            category, usage, ..
        } => (*category, *usage, UsageAvailability::Unspecified),
        CoreEvent::ProjectionStreamEvent { envelope, .. } => match &envelope.payload {
            ProjectionEvent::TurnFailed {
                category, usage, ..
            } => (*category, *usage, UsageAvailability::LostAtProjection),
            _ => (None, None, UsageAvailability::Unspecified),
        },
        _ => (None, None, UsageAvailability::Unspecified),
    };

    let category = category.unwrap_or(TurnFailureCategory::NativeFailureUnspecified);
    let (counts, reason) = match usage {
        Some(usage) => (Some(usage), usage.availability),
        None => (None, absent_reason),
    };

    // `codeggUsage` is null whenever no token count is available. A
    // partially reported usage still produces the object, with null for
    // the counts the route did not report.
    let usage_value = match counts {
        Some(usage)
            if usage.input_tokens.is_some()
                || usage.output_tokens.is_some()
                || usage.cached_tokens.is_some()
                || usage.reasoning_tokens.is_some() =>
        {
            json!({
                "inputTokens": usage.input_tokens,
                "outputTokens": usage.output_tokens,
                "cachedTokens": usage.cached_tokens,
                "reasoningTokens": usage.reasoning_tokens,
            })
        }
        _ => Value::Null,
    };

    json!({
        "kind": "turn_failed",
        "sessionId": prompt.session_id,
        "turnId": prompt.turn_id,
        "codeggFailureCategory": category.as_str(),
        "codeggUsage": usage_value,
        "codeggUsageReason": reason.as_str(),
    })
}

fn tool_arguments_sha256(arguments: &ToolArgumentProjection) -> Option<String> {
    let text = match arguments {
        ToolArgumentProjection::Inline { arguments } => arguments,
        ToolArgumentProjection::Summary { summary } => summary,
        ToolArgumentProjection::TruncatedArguments { preview, .. } => preview,
        ToolArgumentProjection::Handle { .. } => return None,
    };
    Some(format!("{:x}", Sha256::digest(text.as_bytes())))
}

fn map_acp_stop_reason(native: &str) -> &'static str {
    match native {
        "cancelled" | "canceled" => "cancelled",
        "max_tokens" => "max_tokens",
        "max_turn_requests" => "max_turn_requests",
        "refusal" => "refusal",
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
    write_error_with_data(stdout, id, code, message, Value::Null).await
}

async fn write_error_with_data(
    stdout: &Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    id: Value,
    code: i64,
    message: &str,
    data: Value,
) -> Result<(), crate::error::AppError> {
    write_frame(stdout, error_frame(id, code, message, data)).await
}

/// Build the JSON-RPC error frame.
///
/// Pure so the wire shape (code, message, data) can be asserted without a
/// stdout writer.
fn error_frame(id: Value, code: i64, message: &str, data: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message,"data":data}})
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

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> ActivePrompt {
        ActivePrompt {
            request_id: json!(1),
            session_id: "session-1".into(),
            submission_event_floor: 10,
            projection_event_floor: 10,
            projection_subscription_id: Some(
                crate::protocol::projection::replay::ProjectionSubscriptionId("sub-1".into()),
            ),
            turn_id: None,
            cancel_requested: false,
            close_requested: false,
            cancel_sent: false,
        }
    }

    fn event(seq: u64, session_id: &str, payload: CoreEvent) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: seq,
            timestamp_ms: 0,
            session_id: Some(session_id.into()),
            turn_id: None,
            payload,
        }
    }

    fn projection_event(
        seq: u64,
        subscription: &str,
        session: &str,
        turn: &str,
        payload: ProjectionEvent,
    ) -> EventEnvelope<CoreEvent> {
        EventEnvelope {
            protocol_version: 1,
            event_seq: 0,
            timestamp_ms: 0,
            session_id: None,
            turn_id: None,
            payload: CoreEvent::ProjectionStreamEvent {
                subscription_id: crate::protocol::projection::replay::ProjectionSubscriptionId(
                    subscription.into(),
                ),
                stream_id: crate::protocol::projection::replay::ProjectionStreamId(
                    "stream-1".into(),
                ),
                envelope: crate::protocol::projection::event::ProjectionEnvelope {
                    protocol_version: 1,
                    event_seq: seq,
                    timestamp_ms: 0,
                    session_id: Some(session.into()),
                    turn_id: Some(turn.into()),
                    scope: crate::protocol::projection::event::ProjectionStreamScope::Session,
                    payload,
                },
            },
        }
    }

    #[test]
    fn projection_stream_uses_nested_session_turn_subscription_and_cursor() {
        let mut active = prompt();
        active.turn_id = Some("turn-1".into());
        let terminal = projection_event(
            11,
            "sub-1",
            "session-1",
            "turn-1",
            ProjectionEvent::TurnCompleted {
                turn_id: "turn-1".into(),
                stop_reason: "stop".into(),
                completed_at: 0,
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
                reasoning_tokens: None,
                context_limit: None,
            },
        );
        assert!(projection_event_matches(&terminal, &active));
        assert!(event_is_terminal(&terminal, &active));
        assert!(!projection_event_matches(
            &projection_event(
                10,
                "sub-1",
                "session-1",
                "turn-1",
                ProjectionEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    stop_reason: "stop".into(),
                    completed_at: 0,
                    input_tokens: None,
                    output_tokens: None,
                    cached_tokens: None,
                    reasoning_tokens: None,
                    context_limit: None,
                }
            ),
            &active
        ));
        assert!(!projection_event_matches(
            &projection_event(
                11,
                "other-sub",
                "session-1",
                "turn-1",
                ProjectionEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    stop_reason: "stop".into(),
                    completed_at: 0,
                    input_tokens: None,
                    output_tokens: None,
                    cached_tokens: None,
                    reasoning_tokens: None,
                    context_limit: None,
                }
            ),
            &active
        ));
    }

    #[test]
    fn acp_harness_extensions_expose_turn_usage_and_argument_fingerprint() {
        let completed = event(
            13,
            "session-1",
            CoreEvent::TurnCompleted {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
                stop_reason: "end_turn".into(),
                input_tokens: Some(120),
                output_tokens: Some(35),
                cached_tokens: Some(20),
                reasoning_tokens: Some(8),
                context_limit: Some(8192),
            },
        );
        assert_eq!(terminal_usage(&completed)["inputTokens"], 120);
        assert_eq!(terminal_usage(&completed)["outputTokens"], 35);
        assert!(tool_arguments_sha256(&ToolArgumentProjection::Inline {
            arguments: "{\"path\":\"src/lib.rs\"}".into(),
        })
        .is_some());
        assert_eq!(
            tool_arguments_sha256(&ToolArgumentProjection::Handle {
                handle: "private".into(),
                byte_length: 10,
            }),
            None
        );

        // The SUCCESS path's `codeggUsage` shape is frozen: the failure
        // class and the usage reason belong to the failure payload only.
        let success_usage = terminal_usage(&completed);
        let keys: Vec<&str> = success_usage
            .as_object()
            .expect("success usage is an object")
            .keys()
            .map(String::as_str)
            .collect();
        for expected in [
            "turnId",
            "inputTokens",
            "outputTokens",
            "cachedTokens",
            "reasoningTokens",
            "contextLimit",
        ] {
            assert!(keys.contains(&expected), "missing {expected} in {keys:?}");
        }
        for forbidden in ["codeggFailureCategory", "codeggUsageReason"] {
            assert!(
                !keys.contains(&forbidden),
                "success usage must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn acp_turn_failed_error_carries_the_redacted_category_and_usage() {
        use crate::protocol::core::{TurnFailureCategory, TurnUsage, UsageAvailability};

        let mut active = prompt();
        active.turn_id = Some("turn-1".into());
        let failed = event(
            11,
            "session-1",
            CoreEvent::TurnFailed {
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                // A provider message with a credential-shaped string in it:
                // none of this may reach the wire payload.
                message: "Agent error: provider error: 529 overloaded sk-live-SECRET".into(),
                category: Some(TurnFailureCategory::ProviderApi),
                usage: Some(TurnUsage {
                    input_tokens: Some(120),
                    output_tokens: Some(35),
                    cached_tokens: Some(20),
                    reasoning_tokens: None,
                    availability: UsageAvailability::Reported,
                }),
            },
        );

        let data = terminal_failure_data(&active, &failed);
        let frame = error_frame(
            json!(1),
            ACP_TURN_FAILED_CODE,
            "native turn failed",
            data.clone(),
        );

        assert_eq!(frame["error"]["code"], -32010);
        assert_eq!(data["kind"], "turn_failed");
        assert_eq!(data["sessionId"], "session-1");
        assert_eq!(data["turnId"], "turn-1");
        assert_eq!(data["codeggFailureCategory"], "provider_api");
        assert_eq!(data["codeggUsage"]["inputTokens"], 120);
        assert_eq!(data["codeggUsage"]["outputTokens"], 35);
        assert_eq!(data["codeggUsage"]["cachedTokens"], 20);
        assert_eq!(data["codeggUsage"]["reasoningTokens"], Value::Null);
        assert_eq!(data["codeggUsageReason"], "reported");

        // The provider message is never projected into the payload.
        let rendered = data.to_string();
        assert!(
            !rendered.contains("SECRET"),
            "the payload must not carry provider text: {rendered}"
        );
        assert!(
            !rendered.contains("529"),
            "the payload must not carry the provider message: {rendered}"
        );
    }

    #[test]
    fn acp_native_turn_failure_without_usage_reports_unspecified() {
        let mut active = prompt();
        active.turn_id = Some("turn-1".into());
        let failed = event(
            11,
            "session-1",
            CoreEvent::TurnFailed {
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                message: "Agent error: provider turn cancelled".into(),
                category: Some(crate::protocol::core::TurnFailureCategory::Cancelled),
                usage: None,
            },
        );

        let data = terminal_failure_data(&active, &failed);
        assert_eq!(data["codeggFailureCategory"], "cancelled");
        assert_eq!(data["codeggUsage"], Value::Null);
        assert_eq!(data["codeggUsageReason"], "unspecified");
    }

    #[test]
    fn acp_projection_turn_failure_without_usage_reports_lost_at_projection() {
        let mut active = prompt();
        active.turn_id = Some("turn-1".into());
        let failed = projection_event(
            11,
            "sub-1",
            "session-1",
            "turn-1",
            ProjectionEvent::TurnFailed {
                turn_id: "turn-1".into(),
                message: "projection carried failure".into(),
                failed_at: 0,
                category: None,
                usage: None,
            },
        );

        let data = terminal_failure_data(&active, &failed);
        assert_eq!(data["kind"], "turn_failed");
        assert_eq!(data["sessionId"], "session-1");
        assert_eq!(data["turnId"], "turn-1");
        assert_eq!(data["codeggUsage"], Value::Null);
        assert_eq!(data["codeggUsageReason"], "lost_at_projection");
        assert_eq!(data["codeggFailureCategory"], "native_failure_unspecified");
    }

    #[test]
    fn acp_projection_turn_failure_keeps_a_reported_category_and_usage() {
        use crate::protocol::core::{TurnFailureCategory, TurnUsage, UsageAvailability};

        let mut active = prompt();
        active.turn_id = Some("turn-1".into());
        let failed = projection_event(
            11,
            "sub-1",
            "session-1",
            "turn-1",
            ProjectionEvent::TurnFailed {
                turn_id: "turn-1".into(),
                message: "projection carried failure".into(),
                failed_at: 0,
                category: Some(TurnFailureCategory::ProviderTransport),
                usage: Some(TurnUsage {
                    input_tokens: Some(11),
                    output_tokens: None,
                    cached_tokens: None,
                    reasoning_tokens: None,
                    availability: UsageAvailability::Reported,
                }),
            },
        );

        let data = terminal_failure_data(&active, &failed);
        assert_eq!(data["codeggFailureCategory"], "provider_transport");
        assert_eq!(data["codeggUsage"]["inputTokens"], 11);
        assert_eq!(data["codeggUsage"]["outputTokens"], Value::Null);
        assert_eq!(data["codeggUsageReason"], "reported");
    }

    #[test]
    fn lifecycle_rejects_pre_submission_and_neighbor_events() {
        let active = prompt();
        let before = event(
            10,
            "session-1",
            CoreEvent::TurnStarted {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
            },
        );
        assert!(!active.can_accept(&before));
        let neighbor = event(
            11,
            "session-2",
            CoreEvent::TurnStarted {
                session_id: "session-2".into(),
                turn_id: "turn-2".into(),
            },
        );
        assert!(!active.can_accept(&neighbor));
        assert!(active.turn_id.is_none());
    }

    #[test]
    fn lifecycle_binds_one_turn_and_rejects_stale_terminal_events() {
        let mut active = prompt();
        let started = event(
            11,
            "session-1",
            CoreEvent::TurnStarted {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
            },
        );
        assert!(active.can_accept(&started));
        assert!(active.bind_turn("turn-1"));
        assert!(!active.bind_turn("turn-2"));

        let stale = event(
            12,
            "session-1",
            CoreEvent::TurnCompleted {
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
                reasoning_tokens: None,
                context_limit: None,
                session_id: "session-1".into(),
                turn_id: "turn-2".into(),
                stop_reason: "end_turn".into(),
            },
        );
        assert!(!event_is_terminal(&stale, &active));
        let terminal = event(
            13,
            "session-1",
            CoreEvent::TurnCompleted {
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
                reasoning_tokens: None,
                context_limit: None,
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
                stop_reason: "end_turn".into(),
            },
        );
        assert!(event_is_terminal(&terminal, &active));
    }

    #[test]
    fn cancellation_and_close_are_pending_and_idempotent() {
        let mut active = prompt();
        active.cancel(false);
        active.cancel(true);
        assert!(active.cancel_requested);
        assert!(active.close_requested);
        assert!(!active.cancel_sent);
        assert!(active.bind_turn("turn-1"));
        active.cancel_sent = true;
        active.cancel(true);
        assert!(active.cancel_sent);
    }

    #[test]
    fn permission_pending_is_correlated_to_the_active_session_and_turn() {
        let mut active = prompt();
        active.bind_turn("turn-1");
        let pending = event(
            11,
            "session-1",
            CoreEvent::PermissionPending {
                id: "permission-1".into(),
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                tool: "bash".into(),
                path: None,
            },
        );
        assert!(event_is_permission_pending(&pending, &active));
        let stale = event(
            12,
            "session-1",
            CoreEvent::PermissionPending {
                id: "permission-2".into(),
                session_id: "session-1".into(),
                turn_id: Some("turn-2".into()),
                tool: "bash".into(),
                path: None,
            },
        );
        assert!(!event_is_permission_pending(&stale, &active));
    }

    #[test]
    fn terminal_outcome_does_not_mislabel_failure_as_cancellation() {
        let failed = event(
            11,
            "session-1",
            CoreEvent::TurnFailed {
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                message: "provider unavailable".into(),
                category: None,
                usage: None,
            },
        );
        assert_eq!(terminal_stop_reason(&failed), None);

        let completed = event(
            12,
            "session-1",
            CoreEvent::TurnCompleted {
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
                reasoning_tokens: None,
                context_limit: None,
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
                stop_reason: "end_turn".into(),
            },
        );
        assert_eq!(terminal_stop_reason(&completed), Some("end_turn"));
    }

    #[test]
    fn acp_maps_only_known_standard_stop_reasons() {
        assert_eq!(map_acp_stop_reason("max_tokens"), "max_tokens");
        assert_eq!(
            map_acp_stop_reason("max_turn_requests"),
            "max_turn_requests"
        );
        assert_eq!(map_acp_stop_reason("refusal"), "refusal");
        assert_eq!(map_acp_stop_reason("cancelled"), "cancelled");
        assert_eq!(map_acp_stop_reason("vendor_specific"), "end_turn");
    }
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
mod helper_tests {
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
