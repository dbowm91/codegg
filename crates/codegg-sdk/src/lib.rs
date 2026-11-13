//! A typed client for CodeGG's embedding host protocol.
//!
//! It spawns `codegg host --stdio` (or attaches to any pair of pipes), speaks
//! newline-delimited JSON-RPC 2.0 with it, correlates requests, and delivers
//! notifications as a bounded stream. The wire types come from
//! [`codegg_host_protocol`], which is the single definition shared with the host
//! binary; this crate adds transport and correlation and nothing else.
//!
//! The contract of record is `docs/embedding.md`; the stability policy is
//! ADR-0060.
//!
//! ```no_run
//! use codegg_sdk::HostClient;
//!
//! # async fn example() -> Result<(), codegg_sdk::HostError> {
//! let client = HostClient::spawn("codegg", &["host", "--stdio"])?;
//! let hello = client.initialize("my-host", "1.0.0", &[]).await?;
//! assert_eq!(hello.api_version, 1);
//! # Ok(())
//! # }
//! ```

use codegg_host_protocol as wire;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

/// How many notifications are buffered before a slow consumer is reported.
pub const EVENT_BUFFER: usize = 256;
/// Default bound on one request/response round trip.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound on `turn/submit`. The turn's own work continues afterwards.
pub const SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);

/// A failure the client can distinguish without parsing prose.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The pipe could not be read or written.
    #[error("host transport: {0}")]
    Io(#[from] std::io::Error),
    /// The host process exited or closed stdio.
    #[error("host closed the connection")]
    Closed,
    /// No response arrived within the bound.
    #[error("no response within {0:?}")]
    Timeout(Duration),
    /// The host answered with a typed error.
    #[error("{message}", message = .0.message)]
    Rpc(wire::RpcError),
    /// A slow consumer fell behind and notifications were dropped.
    #[error("notification stream overflowed; events were dropped")]
    EventOverflow,
    /// The frame could not be built or read.
    #[error("wire error: {0}")]
    Wire(String),
}

impl HostError {
    /// The wire class, when the host named one.
    pub fn class(&self) -> Option<wire::ErrorClass> {
        match self {
            Self::Rpc(error) => error.data.as_ref().map(|data| data.class),
            _ => None,
        }
    }

    /// Whether the same request may reasonably be retried.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Rpc(error) => error
                .data
                .as_ref()
                .map(|data| data.retryable)
                .unwrap_or(false),
            Self::Timeout(_) | Self::EventOverflow => true,
            Self::Io(_) | Self::Closed | Self::Wire(_) => false,
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, wire::RpcError>>>>>;
type Writer = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

/// One host process, one session, one turn at a time.
pub struct HostClient {
    child: Option<Child>,
    writer: Writer,
    pending: Pending,
    events: mpsc::Receiver<wire::NotificationBody>,
    overflowed: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    next_id: AtomicU64,
    timeout: Duration,
}

impl std::fmt::Debug for HostClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostClient")
            .field("owned_process", &self.child.is_some())
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .finish()
    }
}

impl HostClient {
    /// Spawn `program` with `args` and speak the protocol on its stdio.
    ///
    /// The command is spawned with `stdin` and `stdout` piped; stderr is
    /// inherited so a host's diagnostics reach the caller's own stderr. There is
    /// no shell and no credential on argv.
    pub fn spawn(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Result<Self, HostError> {
        let mut command = Command::new(program);
        command.args(args);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or(HostError::Closed)?;
        let stdout = child.stdout.take().ok_or(HostError::Closed)?;
        Ok(Self::attach(stdout, stdin).with_child(child))
    }

    /// Attach to an existing pair of pipes. Used by tests and by hosts that own
    /// the process lifecycle themselves.
    pub fn attach<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let overflowed = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        {
            let pending = Arc::clone(&pending);
            let closed = Arc::clone(&closed);
            let overflowed = Arc::clone(&overflowed);
            tokio::spawn(async move {
                let mut lines = BufReader::new(reader).lines();
                loop {
                    let line = match lines.next_line().await {
                        Ok(Some(line)) => line,
                        Ok(None) => break,
                        Err(_) => break,
                    };
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(response) = serde_json::from_str::<wire::ResponseFrame>(&line) {
                        if let wire::RequestId::Number(id) = response.id {
                            let id = id.max(0) as u64;
                            let sender = pending.lock().await.remove(&id);
                            if let Some(sender) = sender {
                                let outcome = match (response.result, response.error) {
                                    (Some(result), _) => Ok(result),
                                    (None, Some(error)) => Err(error),
                                    (None, None) => Err(wire::RpcError::protocol(
                                        wire::code::INVALID,
                                        wire::ErrorClass::Internal,
                                        "response carried neither result nor error",
                                    )),
                                };
                                let _ = sender.send(outcome);
                            }
                            continue;
                        }
                        continue;
                    }
                    match serde_json::from_str::<wire::NotificationFrame>(&line) {
                        Ok(frame) => {
                            if events_tx.try_send(frame.body).is_err() {
                                overflowed.store(true, Ordering::SeqCst);
                            }
                        }
                        Err(_) => {
                            // A frame this client cannot read is not fatal for the
                            // session; it is reported through the event stream's
                            // overflow channel only when it hides a notification.
                            continue;
                        }
                    }
                }
                closed.store(true, Ordering::SeqCst);
                let mut pending = pending.lock().await;
                pending.clear();
            });
        }
        Self {
            child: None,
            writer: Arc::new(Mutex::new(Box::new(writer))),
            pending,
            events: events_rx,
            overflowed,
            closed,
            next_id: AtomicU64::new(1),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// The child this client owns, if it spawned one.
    pub fn with_child(mut self, child: Child) -> Self {
        self.child = Some(child);
        self
    }

    /// Override the bound on one request/response round trip.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// True once the host closed its side.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Send one request and wait for its response.
    pub async fn request(&self, body: wire::RequestBody) -> Result<serde_json::Value, HostError> {
        self.request_with_timeout(body, self.timeout).await
    }

    async fn request_with_timeout(
        &self,
        body: wire::RequestBody,
        timeout: Duration,
    ) -> Result<serde_json::Value, HostError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(HostError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        let frame = wire::RequestFrame {
            jsonrpc: "2.0".to_owned(),
            id: wire::RequestId::Number(id as i64),
            body,
        };
        let line = wire::encode(&frame).map_err(|error| HostError::Wire(error.to_string()))?;
        {
            let mut writer = self.writer.lock().await;
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(HostError::Rpc(error)),
            Ok(Err(_)) => Err(HostError::Closed),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(HostError::Timeout(timeout))
            }
        }
    }

    /// `initialize`. Must be the first request.
    pub async fn initialize(
        &self,
        client_name: &str,
        client_version: &str,
        capabilities: &[&str],
    ) -> Result<wire::InitializeResult, HostError> {
        let params = wire::InitializeParams {
            client_name: Some(client_name.to_owned()),
            client_version: Some(client_version.to_owned()),
            capabilities: Some(
                capabilities
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
            ),
        };
        let value = self.request(wire::RequestBody::Initialize(params)).await?;
        serde_json::from_value(value).map_err(|error| HostError::Wire(error.to_string()))
    }

    /// `session/open`.
    pub async fn open_session(
        &self,
        params: wire::SessionOpenParams,
    ) -> Result<wire::SessionOpenResult, HostError> {
        let value = self.request(wire::RequestBody::SessionOpen(params)).await?;
        serde_json::from_value(value).map_err(|error| HostError::Wire(error.to_string()))
    }

    /// `turn/submit`. The turn's events arrive on [`Self::recv`].
    pub async fn submit(
        &self,
        session_id: &str,
        prompt: &str,
        turn_id: Option<&str>,
    ) -> Result<wire::TurnSubmitResult, HostError> {
        let params = wire::TurnSubmitParams {
            session_id: session_id.to_owned(),
            prompt: prompt.to_owned(),
            turn_id: turn_id.map(str::to_owned),
        };
        let value = self
            .request_with_timeout(wire::RequestBody::TurnSubmit(params), SUBMIT_TIMEOUT)
            .await?;
        serde_json::from_value(value).map_err(|error| HostError::Wire(error.to_string()))
    }

    /// `turn/cancel`.
    pub async fn cancel(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<wire::TurnCancelResult, HostError> {
        let params = wire::TurnCancelParams {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
        };
        let value = self.request(wire::RequestBody::TurnCancel(params)).await?;
        serde_json::from_value(value).map_err(|error| HostError::Wire(error.to_string()))
    }

    /// `session/close`.
    pub async fn close(&self, session_id: &str) -> Result<wire::SessionCloseResult, HostError> {
        let params = wire::SessionCloseParams {
            session_id: session_id.to_owned(),
        };
        let value = self
            .request(wire::RequestBody::SessionClose(params))
            .await?;
        serde_json::from_value(value).map_err(|error| HostError::Wire(error.to_string()))
    }

    /// The next notification, or `None` once the host has closed.
    pub async fn recv(&mut self) -> Result<Option<wire::NotificationBody>, HostError> {
        if self.overflowed.swap(false, Ordering::SeqCst) {
            return Err(HostError::EventOverflow);
        }
        Ok(self.events.recv().await)
    }

    /// Stop the host: close stdin, then wait for it to exit and reap it.
    pub async fn shutdown(mut self) {
        self.stop().await;
    }

    async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

impl Drop for HostClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // `kill_on_drop` is already set; reaping here keeps the exit status
            // from lingering as a zombie for the caller's whole lifetime.
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_host_protocol::{ErrorClass, NotificationBody, StopReason};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// A scripted host. It answers the five methods and emits the notifications
    /// a real host emits for one turn.
    async fn scripted_host(server: tokio::io::DuplexStream, busy_on_second_submit: bool) {
        let (reader, mut writer) = tokio::io::split(server);
        let mut lines = BufReader::new(reader).lines();
        let mut submits = 0;
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: serde_json::Value = serde_json::from_str(&line).expect("request json");
            let method = frame["method"].as_str().expect("method");
            let id = frame["id"].clone();
            let result = match method {
                "initialize" => serde_json::json!({
                    "apiVersion": 1,
                    "codeggVersion": "0.1.0",
                    "capabilities": ["descriptor", "streaming", "cancellation", "usage"]
                }),
                "session/open" => serde_json::json!({
                    "sessionId": "s1",
                    "model": frame["params"]["model"].clone(),
                    "agent": "build",
                    "workspace": frame["params"]["workspace"].clone()
                }),
                "turn/submit" => {
                    submits += 1;
                    if busy_on_second_submit && submits > 1 {
                        let error = serde_json::json!({
                            "code": wire::code::BUSY,
                            "message": "a turn is already active",
                            "data": {"class": "busy", "retryable": true}
                        });
                        let response = serde_json::json!({
                            "jsonrpc": "2.0", "id": id, "error": error
                        });
                        writer
                            .write_all(format!("{response}\n").as_bytes())
                            .await
                            .expect("write");
                        continue;
                    }
                    serde_json::json!({"turnId": "t1"})
                }
                "turn/cancel" => serde_json::json!({"cancelled": true}),
                "session/close" => serde_json::json!({"closed": true}),
                other => panic!("unexpected method {other}"),
            };
            let response = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result});
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .expect("write");
            if method == "turn/submit" {
                for delta in ["Wri", "tin", "g."] {
                    let frame = wire::NotificationFrame {
                        jsonrpc: "2.0".to_owned(),
                        body: NotificationBody::Text(wire::TurnText {
                            session_id: "s1".to_owned(),
                            turn_id: "t1".to_owned(),
                            delta: delta.to_owned(),
                        }),
                    };
                    writer
                        .write_all(
                            format!("{}\n", wire::encode(&frame).expect("encode")).as_bytes(),
                        )
                        .await
                        .expect("write");
                }
                let tool = wire::NotificationFrame {
                    jsonrpc: "2.0".to_owned(),
                    body: NotificationBody::Tool(wire::TurnTool {
                        session_id: "s1".to_owned(),
                        turn_id: "t1".to_owned(),
                        phase: wire::ToolPhase::Started,
                        tool_id: "call_1".to_owned(),
                        tool_name: "write".to_owned(),
                        arguments: Some(r#"{"path":"hello.txt"}"#.to_owned()),
                        output: None,
                        success: None,
                        truncated: None,
                    }),
                };
                writer
                    .write_all(format!("{}\n", wire::encode(&tool).expect("encode")).as_bytes())
                    .await
                    .expect("write");
                for body in [
                    NotificationBody::Usage(wire::TurnUsage {
                        session_id: "s1".to_owned(),
                        turn_id: "t1".to_owned(),
                        usage: wire::Usage::reported(Some(812), Some(96), None),
                    }),
                    NotificationBody::Done(wire::TurnDone {
                        session_id: "s1".to_owned(),
                        turn_id: "t1".to_owned(),
                        stop_reason: StopReason::EndTurn,
                        usage: wire::Usage::reported(Some(812), Some(96), None),
                    }),
                ] {
                    let frame = wire::NotificationFrame {
                        jsonrpc: "2.0".to_owned(),
                        body,
                    };
                    writer
                        .write_all(
                            format!("{}\n", wire::encode(&frame).expect("encode")).as_bytes(),
                        )
                        .await
                        .expect("write");
                }
            }
        }
    }

    fn attached(busy_on_second_submit: bool) -> HostClient {
        let (client, server) = duplex(64 * 1024);
        tokio::spawn(scripted_host(server, busy_on_second_submit));
        let (reader, writer) = tokio::io::split(client);
        HostClient::attach(reader, writer)
    }

    async fn open(client: &HostClient) -> wire::SessionOpenResult {
        client
            .initialize("test", "1.0.0", &["descriptor", "streaming"])
            .await
            .expect("initialize");
        client
            .open_session(wire::SessionOpenParams {
                workspace: "/workspace".to_owned(),
                model: "openai/deepseek-v4.1-flash".to_owned(),
                agent: Some("build".to_owned()),
                approval: Some("yolo".to_owned()),
                sandbox: Some("full-host".to_owned()),
                session_id: None,
                provider: None,
            })
            .await
            .expect("open")
    }

    #[tokio::test]
    async fn a_session_streams_ordered_events_and_a_typed_done() {
        let mut client = attached(false);
        let session = open(&client).await;
        assert_eq!(session.session_id, "s1");
        let turn = client
            .submit("s1", "write it", Some("t1"))
            .await
            .expect("submit");
        assert_eq!(turn.turn_id, "t1");
        let mut text = String::new();
        let mut saw_tool = false;
        let mut saw_usage = false;
        let mut done = None;
        while let Some(event) = client.recv().await.expect("event") {
            match event {
                NotificationBody::Text(update) => text.push_str(&update.delta),
                NotificationBody::Tool(update) => {
                    assert_eq!(update.phase, wire::ToolPhase::Started);
                    saw_tool = true;
                }
                NotificationBody::Usage(update) => {
                    assert!(update.usage.reported);
                    assert_eq!(update.usage.input_tokens, Some(812));
                    saw_usage = true;
                }
                NotificationBody::Done(update) => {
                    assert_eq!(update.stop_reason, StopReason::EndTurn);
                    done = Some(update);
                    break;
                }
                NotificationBody::Error(_) => panic!("unexpected error notification"),
            }
        }
        assert_eq!(text, "Writing.");
        assert!(saw_tool, "no tool notification");
        assert!(saw_usage, "no usage notification");
        assert!(done.is_some(), "no terminal notification");
        let closed = client.close("s1").await.expect("close");
        assert!(closed.closed);
    }

    #[tokio::test]
    async fn a_busy_refusal_is_a_typed_class() {
        let mut client = attached(true);
        let _session = open(&client).await;
        client.submit("s1", "one", Some("t1")).await.expect("first");
        // Drain the first turn so the client is idle for the second submit.
        while let Some(event) = client.recv().await.expect("event") {
            if matches!(event, NotificationBody::Done(_)) {
                break;
            }
        }
        let error = client
            .submit("s1", "two", Some("t2"))
            .await
            .expect_err("busy");
        assert_eq!(error.class(), Some(ErrorClass::Busy));
        assert!(error.retryable());
        assert!(matches!(error, HostError::Rpc(_)));
    }

    #[tokio::test]
    async fn cancel_returns_an_acknowledgement() {
        let client = attached(false);
        let _session = open(&client).await;
        client
            .submit("s1", "long", Some("t1"))
            .await
            .expect("submit");
        let cancelled = client.cancel("s1", "t1").await.expect("cancel");
        assert!(cancelled.cancelled);
    }

    #[tokio::test]
    async fn a_closed_host_fails_pending_requests() {
        let (client, server) = duplex(8 * 1024);
        drop(server);
        let (reader, writer) = tokio::io::split(client);
        let host = HostClient::attach(reader, writer);
        let error = host.initialize("test", "1", &[]).await.expect_err("closed");
        assert!(matches!(error, HostError::Closed | HostError::Io(_)));
    }

    #[tokio::test]
    async fn a_timeout_is_reported_as_a_timeout() {
        let (client, _server) = duplex(8 * 1024);
        let (reader, writer) = tokio::io::split(client);
        let host = HostClient::attach(reader, writer).with_timeout(Duration::from_millis(50));
        let error = host
            .initialize("test", "1", &[])
            .await
            .expect_err("timeout");
        assert!(matches!(error, HostError::Timeout(_)));
        assert!(error.retryable());
    }

    #[tokio::test]
    async fn unknown_frames_do_not_break_correlation() {
        let (client, server) = duplex(16 * 1024);
        tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(_line)) = lines.next_line().await {
                writer
                    .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"turn/unknown\",\"params\":{}}\n")
                    .await
                    .expect("write");
                writer
                    .write_all(
                        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"apiVersion\":1,\"codeggVersion\":\"0.1.0\",\"capabilities\":[]}}\n",
                    )
                    .await
                    .expect("write");
            }
        });
        let (reader, writer) = tokio::io::split(client);
        let host = HostClient::attach(reader, writer);
        let hello = host.initialize("test", "1", &[]).await.expect("initialize");
        assert_eq!(hello.api_version, 1);
    }
}
