//! Scripted-host coverage over an in-process duplex, with a fake provider.

use super::*;
use crate::config::schema::{Config, SearchBackendConfig, SearchConfig};
use crate::provider::{
    ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, TokenUsage, ToolCall,
};
use futures_util::stream;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

const SECRET: &str = "sk-host-test-secret-9f2c";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeMode {
    Normal,
    Slow,
    Error,
}

struct FakeState {
    mode: FakeMode,
    calls: usize,
}

#[derive(Clone)]
struct FakeProvider {
    state: Arc<StdMutex<FakeState>>,
}

impl FakeProvider {
    fn new() -> (Self, Arc<StdMutex<FakeState>>) {
        let state = Arc::new(StdMutex::new(FakeState {
            mode: FakeMode::Normal,
            calls: 0,
        }));
        (
            Self {
                state: Arc::clone(&state),
            },
            state,
        )
    }
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn id(&self) -> &str {
        "fake"
    }

    fn name(&self) -> &str {
        "Fake"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let (mode, call) = {
            let mut state = self.state.lock().expect("fake state");
            state.calls += 1;
            (state.mode, state.calls)
        };
        if mode == FakeMode::Error {
            return Err(ProviderError::Auth("upstream rejected".to_owned()));
        }
        if mode == FakeMode::Slow {
            return Ok(Box::pin(stream::unfold(0usize, |index| async move {
                if index >= 2_000 {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                Some((
                    Ok(ChatEvent::TextDelta(Arc::new(format!("tick {index}")))),
                    index + 1,
                ))
            })));
        }
        let events: Vec<Result<ChatEvent, ProviderError>> = if call == 1 {
            vec![
                Ok(ChatEvent::TextDelta(Arc::new("one".to_owned()))),
                Ok(ChatEvent::TextDelta(Arc::new("two".to_owned()))),
                Ok(ChatEvent::TextDelta(Arc::new("three".to_owned()))),
                Ok(ChatEvent::ToolCall(ToolCall {
                    id: Arc::new("call_1".to_owned()),
                    name: Arc::new("read".to_owned()),
                    arguments: json!({"path": "hello.txt"}),
                })),
                Ok(ChatEvent::Finish {
                    stop_reason: Arc::new("tool_calls".to_owned()),
                    usage: TokenUsage {
                        input_tokens: 812,
                        output_tokens: 96,
                        cached_tokens: Some(640),
                        ..Default::default()
                    },
                }),
            ]
        } else {
            vec![Ok(ChatEvent::Finish {
                stop_reason: Arc::new("end_turn".to_owned()),
                usage: TokenUsage {
                    input_tokens: 812,
                    output_tokens: 96,
                    ..Default::default()
                },
            })]
        };
        Ok(Box::pin(stream::iter(events)))
    }
}

static STDERR: OnceLock<Arc<StdMutex<Vec<u8>>>> = OnceLock::new();

#[derive(Clone)]
struct CaptureWriter(Arc<StdMutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("stderr buffer")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn stderr_capture() -> Arc<StdMutex<Vec<u8>>> {
    Arc::clone(STDERR.get_or_init(|| {
        let buffer = Arc::new(StdMutex::new(Vec::new()));
        let writer = CaptureWriter(Arc::clone(&buffer));
        let _ = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .try_init();
        buffer
    }))
}

struct Harness {
    input: Option<DuplexStream>,
    output: BufReader<DuplexStream>,
    task: tokio::task::JoinHandle<Result<(), HostError>>,
    stderr: Arc<StdMutex<Vec<u8>>>,
    raw: Vec<String>,
}

impl Harness {
    fn new(provider: Option<Box<dyn Provider>>) -> Self {
        Self::with_capacity(provider, 64 * 1024)
    }

    fn with_capacity(provider: Option<Box<dyn Provider>>, capacity: usize) -> Self {
        let (input, host_input) = tokio::io::duplex(capacity);
        let (host_output, output) = tokio::io::duplex(capacity);
        let task = tokio::spawn(run_stdio_with_seam(
            host_input,
            host_output,
            provider,
            Some(test_config()),
        ));
        Self {
            input: Some(input),
            output: BufReader::new(output),
            task,
            stderr: stderr_capture(),
            raw: Vec::new(),
        }
    }

    async fn send_raw(&mut self, bytes: &[u8]) {
        self.input
            .as_mut()
            .expect("input open")
            .write_all(bytes)
            .await
            .expect("write frame");
    }

    async fn send(&mut self, frame: Value) {
        let line = serde_json::to_string(&frame).expect("encode");
        self.send_raw(line.as_bytes()).await;
        self.send_raw(b"\n").await;
    }

    async fn next_line(&mut self) -> Option<String> {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), self.output.read_line(&mut line))
            .await
            .expect("frame read timed out")
            .expect("read frame");
        if read == 0 {
            return None;
        }
        self.raw.push(line.clone());
        Some(line)
    }

    async fn next(&mut self) -> Value {
        let line = self.next_line().await.expect("host closed");
        serde_json::from_str(&line).expect("frame is JSON")
    }

    async fn result(&mut self, id: Value) -> Value {
        loop {
            let frame = self.next().await;
            if frame.get("id") == Some(&id) {
                assert!(frame.get("error").is_none(), "unexpected error: {frame}");
                return frame["result"].clone();
            }
        }
    }

    async fn error(&mut self, id: Value) -> Value {
        loop {
            let frame = self.next().await;
            if frame.get("id") == Some(&id) {
                assert!(frame.get("result").is_none(), "unexpected result: {frame}");
                return frame["error"].clone();
            }
        }
    }

    async fn notification(&mut self) -> Value {
        loop {
            let frame = self.next().await;
            if frame.get("method").is_some() {
                return frame;
            }
        }
    }

    async fn initialize(&mut self) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"capabilities": ["descriptor", "streaming", "cancellation", "usage"]},
        }))
        .await;
        self.result(json!(1)).await
    }

    async fn open(&mut self, workspace: &Path, session_id: &str, agent: &str) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/open",
            "params": {
                "workspace": workspace,
                "model": "fake/test-model",
                "agent": agent,
                "approval": "yolo",
                "sandbox": "workspace-write",
                "sessionId": session_id,
                "provider": descriptor(SECRET),
                "futureField": true,
            },
        }))
        .await;
        self.result(json!(2)).await
    }

    async fn submit(&mut self, id: i64, session_id: &str, turn_id: &str) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "turn/submit",
            "params": {"sessionId": session_id, "turnId": turn_id, "prompt": "go"},
        }))
        .await;
        self.result(json!(id)).await
    }

    async fn finish(&mut self) -> Result<(), HostError> {
        drop(self.input.take());
        tokio::time::timeout(Duration::from_secs(10), &mut self.task)
            .await
            .expect("host task timed out")
            .expect("host task join")
    }
}

fn test_config() -> Config {
    Config {
        search: Some(SearchConfig {
            backend: Some(SearchBackendConfig::Disabled),
            ..Default::default()
        }),
        ..Config::default()
    }
}

fn descriptor(secret: &str) -> Value {
    json!({
        "id": "fake",
        "displayName": "Fake",
        "baseUrl": "https://example.invalid/v1",
        "protocol": "chat_completions",
        "headers": [{"name": "x-tenant", "value": "demo"}],
        "credential": {"kind": "api_key", "value": secret},
        "models": [{
            "id": "test-model",
            "contextWindow": 64000,
            "maxOutputTokens": 1024,
            "supportsTools": true,
        }],
    })
}

fn workspace() -> PathBuf {
    let directory = std::env::temp_dir().join(format!("codegg-host-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).expect("workspace");
    std::fs::write(directory.join("hello.txt"), b"hello\n").expect("file");
    directory
}

fn turn_done(frame: &Value) -> bool {
    matches!(
        frame["method"].as_str(),
        Some("turn/done") | Some("turn/error")
    )
}

#[tokio::test]
async fn handshake_open_stream_tools_usage_and_close() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    let initialized = host.initialize().await;
    assert_eq!(initialized["apiVersion"], 1);
    assert!(!initialized["codeggVersion"]
        .as_str()
        .unwrap_or("")
        .is_empty());
    assert_eq!(
        initialized["capabilities"],
        json!(["descriptor", "streaming", "cancellation", "usage"])
    );

    let directory = workspace();
    let opened = host.open(&directory, "s-main", "build").await;
    assert_eq!(opened["sessionId"], "s-main");
    assert_eq!(opened["agent"], "build");
    assert_eq!(opened["model"], "fake/test-model");
    assert_eq!(
        opened["workspace"],
        directory
            .canonicalize()
            .expect("canonical")
            .to_string_lossy()
            .as_ref()
    );

    assert_eq!(host.submit(3, "s-main", "t1").await["turnId"], "t1");
    let mut text = String::new();
    for _ in 0..3 {
        let frame = host.notification().await;
        assert_eq!(frame["method"], "turn/text");
        assert_eq!(frame["params"]["sessionId"], "s-main");
        assert_eq!(frame["params"]["turnId"], "t1");
        text.push_str(frame["params"]["delta"].as_str().expect("delta"));
    }
    assert_eq!(text, "onetwothree");

    let started = host.notification().await;
    assert_eq!(started["method"], "turn/tool");
    assert_eq!(started["params"]["phase"], "started");
    assert_eq!(started["params"]["toolId"], "call_1");
    assert_eq!(started["params"]["toolName"], "read");
    assert_eq!(started["params"]["arguments"], r#"{"path":"hello.txt"}"#);

    let finished = host.notification().await;
    assert_eq!(finished["method"], "turn/tool");
    assert_eq!(finished["params"]["phase"], "finished");
    assert_eq!(
        finished["params"]["success"],
        json!(true),
        "tool frame: {finished}"
    );
    assert_eq!(finished["params"]["truncated"], json!(false));

    let usage = host.notification().await;
    assert_eq!(usage["method"], "turn/usage");
    assert_eq!(usage["params"]["reported"], json!(true));
    assert!(usage["params"]["inputTokens"].as_u64().expect("tokens") > 0);

    let done = host.notification().await;
    assert_eq!(done["method"], "turn/done");
    assert_eq!(done["params"]["stopReason"], "end_turn");
    assert_eq!(done["params"]["usage"], {
        json!({
            "reported": true,
            "inputTokens": usage["params"]["inputTokens"],
            "outputTokens": usage["params"]["outputTokens"],
            "cachedTokens": usage["params"]["cachedTokens"],
            "reasoningTokens": usage["params"]["reasoningTokens"],
        })
    });

    host.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/close",
        "params": {"sessionId": "s-main"},
    }))
    .await;
    assert_eq!(host.result(json!(4)).await["closed"], json!(true));
    assert!(host.raw.iter().all(|line| !line.contains(SECRET)));
    assert!(host.finish().await.is_ok());
    let stderr = String::from_utf8_lossy(&host.stderr.lock().expect("stderr")).to_string();
    assert!(!stderr.contains(SECRET));
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn open_before_initialize_is_refused() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/open",
        "params": {"workspace": workspace(), "model": "fake/test-model"},
    }))
    .await;
    let error = host.error(json!(2)).await;
    assert_eq!(error["code"], -32002);
    assert_eq!(error["data"]["class"], "not_initialized");
    assert_eq!(host.initialize().await["apiVersion"], 1);
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn a_second_open_is_refused_without_smearing_the_first() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.open(&directory, "s-one", "build").await;
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "session/open",
        "params": {"workspace": directory, "model": "fake/test-model"},
    }))
    .await;
    let error = host.error(json!(5)).await;
    assert_eq!(error["data"]["class"], "invalid_request");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn an_invalid_workspace_leaves_no_session_behind() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "session/open",
        "params": {
            "workspace": "/definitely/missing/codegg-workspace",
            "model": "fake/test-model",
            "provider": descriptor(SECRET),
        },
    }))
    .await;
    let error = host.error(json!(6)).await;
    assert_eq!(error["data"]["class"], "workspace");
    let directory = workspace();
    let opened = host.open(&directory, "s-after", "build").await;
    assert_eq!(opened["sessionId"], "s-after");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn an_unknown_agent_is_named_and_refused() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "session/open",
        "params": {
            "workspace": directory,
            "model": "fake/test-model",
            "agent": "no-such-agent",
            "provider": descriptor(SECRET),
        },
    }))
    .await;
    let error = host.error(json!(7)).await;
    assert_eq!(error["data"]["class"], "invalid_request");
    assert!(error["message"]
        .as_str()
        .expect("message")
        .contains("no-such-agent"));
    let opened = host.open(&directory, "s-agent", "build").await;
    assert_eq!(opened["agent"], "build");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn a_busy_turn_is_refused_cancelling_is_bounded_and_the_next_turn_runs() {
    let (provider, state) = FakeProvider::new();
    state.lock().expect("state").mode = FakeMode::Slow;
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.open(&directory, "s-busy", "build").await;
    assert_eq!(host.submit(8, "s-busy", "t-slow").await["turnId"], "t-slow");
    let first = host.notification().await;
    assert_eq!(first["method"], "turn/text");

    host.send(json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "turn/submit",
        "params": {"sessionId": "s-busy", "turnId": "t-second", "prompt": "again"},
    }))
    .await;
    let busy = host.error(json!(9)).await;
    assert_eq!(busy["code"], -32001);
    assert_eq!(busy["data"]["class"], "busy");
    assert_eq!(busy["data"]["retryable"], json!(true));

    let started = Instant::now();
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "turn/cancel",
        "params": {"sessionId": "s-busy", "turnId": "t-slow"},
    }))
    .await;
    assert_eq!(host.result(json!(10)).await["cancelled"], json!(true));
    let mut terminal = host.notification().await;
    while !turn_done(&terminal) {
        terminal = host.notification().await;
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancellation took {:?}",
        started.elapsed()
    );
    assert_eq!(terminal["method"], "turn/done");
    assert_eq!(terminal["params"]["stopReason"], "cancelled");

    tokio::time::sleep(Duration::from_millis(50)).await;
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "turn/cancel",
        "params": {"sessionId": "s-busy", "turnId": "t-slow"},
    }))
    .await;
    assert_eq!(host.result(json!(11)).await["cancelled"], json!(false));

    state.lock().expect("state").mode = FakeMode::Normal;
    assert_eq!(
        host.submit(12, "s-busy", "t-next").await["turnId"],
        "t-next"
    );
    let mut terminal = host.notification().await;
    while !turn_done(&terminal) {
        terminal = host.notification().await;
    }
    assert_eq!(terminal["method"], "turn/done");
    assert_eq!(terminal["params"]["stopReason"], "end_turn");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn a_provider_error_ends_the_turn_and_the_session_stays_usable() {
    let (provider, state) = FakeProvider::new();
    state.lock().expect("state").mode = FakeMode::Error;
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.open(&directory, "s-error", "build").await;
    host.submit(13, "s-error", "t-error").await;
    let mut frame = host.notification().await;
    while !turn_done(&frame) {
        frame = host.notification().await;
    }
    assert_eq!(frame["method"], "turn/error");
    assert_eq!(frame["params"]["class"], "auth");
    assert_eq!(frame["params"]["retryable"], json!(false));
    assert_eq!(frame["params"]["usage"]["reported"], json!(false));

    state.lock().expect("state").mode = FakeMode::Normal;
    host.submit(14, "s-error", "t-after-error").await;
    let mut terminal = host.notification().await;
    while !turn_done(&terminal) {
        terminal = host.notification().await;
    }
    assert_eq!(terminal["method"], "turn/done");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn eof_during_a_turn_exits_zero() {
    let (provider, state) = FakeProvider::new();
    state.lock().expect("state").mode = FakeMode::Slow;
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.open(&directory, "s-eof", "build").await;
    host.submit(15, "s-eof", "t-eof").await;
    assert_eq!(host.notification().await["method"], "turn/text");
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn session_close_exits_zero() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    host.open(&directory, "s-close", "build").await;
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 16,
        "method": "session/close",
        "params": {"sessionId": "s-close"},
    }))
    .await;
    assert_eq!(host.result(json!(16)).await["closed"], json!(true));
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn the_descriptor_secret_never_leaves_the_open_frame() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;
    let directory = workspace();
    let mut invalid = descriptor(SECRET);
    invalid["models"][0]["contextWindow"] = json!(0);
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 17,
        "method": "session/open",
        "params": {"workspace": directory, "model": "fake/test-model", "provider": invalid},
    }))
    .await;
    let error = host.error(json!(17)).await;
    assert_eq!(error["data"]["class"], "invalid_request");
    assert!(!error.to_string().contains(SECRET));

    host.open(&directory, "s-secret", "build").await;
    host.submit(18, "s-secret", "t-secret").await;
    let mut terminal = host.notification().await;
    while !turn_done(&terminal) {
        terminal = host.notification().await;
    }
    host.send(json!({
        "jsonrpc": "2.0",
        "id": 19,
        "method": "session/close",
        "params": {"sessionId": "s-secret"},
    }))
    .await;
    assert_eq!(host.result(json!(19)).await["closed"], json!(true));
    while host.next_line().await.is_some() {}
    assert!(host.finish().await.is_ok());
    let emitted = host.raw.join("\n");
    assert!(!emitted.contains(SECRET), "a frame carried the credential");
    let stderr = String::from_utf8_lossy(&host.stderr.lock().expect("stderr")).to_string();
    assert!(!stderr.contains(SECRET), "stderr carried the credential");
}

#[tokio::test]
async fn unknown_methods_notifications_responses_and_fields_are_handled() {
    let (provider, _state) = FakeProvider::new();
    let mut host = Harness::new(Some(Box::new(provider)));
    host.initialize().await;

    host.send(json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "turn/steer",
        "params": {},
    }))
    .await;
    let unknown = host.error(json!(20)).await;
    assert_eq!(unknown["code"], -32601);
    assert_eq!(unknown["data"]["class"], "invalid_request");

    host.send(json!({
        "jsonrpc": "2.0",
        "method": "turn/text",
        "params": {},
    }))
    .await;
    let notification = host.next().await;
    assert_eq!(notification["id"], Value::Null);
    assert_eq!(notification["error"]["data"]["class"], "invalid_request");

    host.send(json!({
        "jsonrpc": "2.0",
        "id": 21,
        "result": {},
    }))
    .await;
    let response = host.next().await;
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], -32600);

    host.send(json!({
        "jsonrpc": "2.0",
        "id": 22,
        "method": "initialize",
        "params": {"futureField": true},
    }))
    .await;
    assert_eq!(host.result(json!(22)).await["apiVersion"], 1);
    assert!(host.finish().await.is_ok());
}

#[tokio::test]
async fn invalid_utf8_is_refused_and_exits_non_zero() {
    let mut host = Harness::new(None);
    host.send_raw(&[0xff, 0xfe, b'\n']).await;
    let refusal = host.next().await;
    assert_eq!(refusal["id"], Value::Null);
    assert_eq!(refusal["error"]["data"]["class"], "invalid_request");
    let error = host.finish().await.expect_err("host exits non-zero");
    assert_eq!(error.exit_code(), EXIT_UNPARSEABLE_FRAME);
}

#[tokio::test]
async fn an_oversized_frame_is_refused_and_exits_non_zero() {
    let mut host = Harness::with_capacity(None, MAX_FRAME_BYTES + 4096);
    let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
    host.send_raw(&oversized).await;
    host.send_raw(b"\n").await;
    let refusal = host.next().await;
    assert_eq!(refusal["id"], Value::Null);
    assert_eq!(refusal["error"]["data"]["class"], "invalid_request");
    let error = host.finish().await.expect_err("host exits non-zero");
    assert_eq!(error.exit_code(), EXIT_UNPARSEABLE_FRAME);
}
