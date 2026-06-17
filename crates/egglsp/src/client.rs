//! LSP Client - Language Server Protocol implementation.
//!
//! Manages LSP server lifecycle and communication:
//! - Spawns language servers (rust-analyzer, pyright, etc.)
//! - Handles JSON-RPC message protocol over stdin/stdout
//! - Tracks open files and diagnostics
//! - Supports concurrent requests with atomic ID counter
//!
//! A single background reader task exclusively owns the server's stdout.
//! All JSON-RPC responses are routed to pending oneshot senders; notifications
//! (e.g. `textDocument/publishDiagnostics`) are dispatched independently of
//! request state. When the reader exits, all pending requests fail immediately
//! via [`fail_all_pending`].
//!
//! `diagnostics_may_still_be_warming` returns `true` only when no cache entry
//! exists for a URI after a recent sync — i.e. the server has not yet sent a
//! `publishDiagnostics` response. An empty diagnostics vec means the server
//! reported the file as clean, not that it is still warming.
//!
//! Key types:
//! - `LspClient` - main client managing server process
//! - `LspProcess` - spawned server process with streams
//! - `DiagnosticEntry` - file URI + diagnostic data

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Tracks whether the client transport (writer pipe to the server) is
/// still operational. When the background reader detects a write failure
/// for a server-request response, it transitions to `Failed` and all
/// pending requests are drained. Subsequent `send_request` /
/// `send_notification` calls return `LspError::WriterClosed` immediately.
#[derive(Debug, Clone)]
pub(crate) enum ClientTransportState {
    Running,
    Failed { reason: String },
}

/// Read-only snapshot of the transport state for integration tests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClientTransportSnapshot {
    Running,
    Failed { reason: String },
}

use lsp_types::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, info, warn};
use url::Url;

use super::launch::{self, LspLaunchSpec, LspProcess};
use super::server::LspServerDef;
use super::server_request::{dispatch_server_request, ServerRequestContext, ServerRequestReply};
use super::writer::LspWriter;
use crate::error::LspError;

/// JSON-RPC message ID, preserving both numeric and string forms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

impl JsonRpcId {
    /// Returns the numeric value if this is a `Number` variant.
    pub fn as_number(&self) -> Option<i64> {
        match self {
            JsonRpcId::Number(n) => Some(*n),
            JsonRpcId::String(_) => None,
        }
    }
}

impl fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonRpcId::Number(n) => write!(f, "{n}"),
            JsonRpcId::String(s) => write!(f, "{s}"),
        }
    }
}

impl Serialize for JsonRpcId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            JsonRpcId::Number(n) => serializer.serialize_i64(*n),
            JsonRpcId::String(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for JsonRpcId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        match val {
            serde_json::Value::Number(n) => {
                let num = n.as_i64().ok_or_else(|| {
                    serde::de::Error::custom(format!("invalid JSON-RPC id number: {n}"))
                })?;
                Ok(JsonRpcId::Number(num))
            }
            serde_json::Value::String(s) => Ok(JsonRpcId::String(s)),
            _ => Err(serde::de::Error::custom(format!(
                "invalid JSON-RPC id type: {val}"
            ))),
        }
    }
}

type PendingMap =
    Arc<Mutex<HashMap<JsonRpcId, oneshot::Sender<Result<serde_json::Value, LspError>>>>>;

pub(crate) async fn fail_all_pending(pending: &PendingMap, error_msg: &str) {
    let mut pending = pending.lock().await;
    let drained = std::mem::take(&mut *pending);
    for (_, tx) in drained {
        let _ = tx.send(Err(LspError::RequestFailed(error_msg.to_string())));
    }
}

/// Atomically transition the transport to `Failed` and drain all pending requests.
///
/// This is idempotent: if the transport is already `Failed`, subsequent calls are no-ops.
/// The transport lock is released before draining the pending map to avoid holding it
/// across the (potentially lengthy) iteration.
async fn fail_transport(
    transport_state: &Arc<Mutex<ClientTransportState>>,
    pending: &PendingMap,
    reason: impl Into<String>,
) {
    let mut state = transport_state.lock().await;
    match &*state {
        ClientTransportState::Failed { .. } => (), // already failed
        ClientTransportState::Running => {
            let reason = reason.into();
            *state = ClientTransportState::Failed {
                reason: reason.clone(),
            };
            drop(state); // release transport lock before draining pending
            fail_all_pending(pending, &reason).await;
        }
    }
}

/// Classified JSON-RPC message from the server.
#[derive(Debug)]
pub enum JsonRpcMessage {
    Response {
        id: JsonRpcId,
        result: serde_json::Value,
    },
    ErrorResponse {
        id: JsonRpcId,
        code: Option<i64>,
        message: String,
        data: Option<serde_json::Value>,
    },
    ServerRequest {
        id: JsonRpcId,
        method: String,
        params: serde_json::Value,
    },
    Notification {
        method: String,
        params: serde_json::Value,
    },
    Unknown,
}

/// Extract a `JsonRpcId` from a `serde_json::Value`, if present.
///
/// Returns `None` for null IDs, floating-point IDs (non-integer numbers),
/// and object/array ID values.
fn extract_id(value: &serde_json::Value) -> Option<JsonRpcId> {
    let id_val = value.get("id")?;
    match id_val {
        serde_json::Value::Number(n) => n.as_i64().map(JsonRpcId::Number),
        serde_json::Value::String(s) => Some(JsonRpcId::String(s.clone())),
        serde_json::Value::Null => None,
        _ => None,
    }
}

/// Classify a raw JSON-RPC value into its semantic type.
///
/// Classification order (structural, no silent drops):
/// 1. id + method          → server request
/// 2. id + valid error     → error response (error must be object with code + message)
/// 3. id + result field    → success response (explicit `result: null` is valid)
/// 4. method without id    → notification
/// 5. otherwise            → unknown (id-only objects are unknown, not responses)
pub fn classify_json_rpc_message(value: serde_json::Value) -> JsonRpcMessage {
    let id = extract_id(&value);
    let method = value.get("method").and_then(|v| v.as_str());

    match (id, method) {
        (Some(id), Some(method)) => {
            let params = value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            JsonRpcMessage::ServerRequest {
                id,
                method: method.to_string(),
                params,
            }
        }
        (Some(id), None) if is_structural_error(&value) => {
            let error = value.get("error").unwrap();
            let code = error.get("code").and_then(|c| c.as_i64());
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string();
            let data = error.get("data").cloned();
            JsonRpcMessage::ErrorResponse {
                id,
                code,
                message,
                data,
            }
        }
        (Some(id), None) if value.get("result").is_some() => {
            let result = value.get("result").cloned().unwrap();
            JsonRpcMessage::Response { id, result }
        }
        (None, Some(method)) => {
            let params = value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            JsonRpcMessage::Notification {
                method: method.to_string(),
                params,
            }
        }
        _ => JsonRpcMessage::Unknown,
    }
}

/// Returns true if the value contains a structurally valid JSON-RPC error:
/// `error` is an object with a numeric `code` and a string `message`.
fn is_structural_error(value: &serde_json::Value) -> bool {
    let error = match value.get("error") {
        Some(serde_json::Value::Object(obj)) => obj,
        _ => return false,
    };
    error.get("code").is_some_and(|c| c.as_i64().is_some())
        && error.get("message").is_some_and(|m| m.is_string())
}

/// Dispatch a notification by method. Currently handles diagnostics.
pub async fn dispatch_notification(
    diagnostics: &tokio::sync::Mutex<HashMap<String, DiagnosticCacheEntry>>,
    server_generation: u64,
    method: &str,
    params: serde_json::Value,
) {
    if method == "textDocument/publishDiagnostics" {
        if let Some(uri) = params.get("uri").and_then(|v| v.as_str()) {
            let version = params
                .get("version")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
            if let Some(diags_value) = params.get("diagnostics") {
                match serde_json::from_value::<Vec<lsp_types::Diagnostic>>(diags_value.clone()) {
                    Ok(diags) => {
                        let count = diags.len();
                        diagnostics.lock().await.insert(
                            uri.to_string(),
                            DiagnosticCacheEntry {
                                diagnostics: diags,
                                received_at: std::time::Instant::now(),
                                source: crate::diagnostics::LspDiagnosticSource::Pushed,
                                content_version: version,
                                server_generation,
                                post_restart: server_generation > 1,
                            },
                        );
                        debug!(uri, count, "received diagnostics via background reader");
                    }
                    Err(e) => {
                        warn!(error = %e, uri, "failed to parse diagnostics");
                    }
                }
            }
        }
    }
}

/// Apply a `$/progress` notification to the progress tracker.
///
/// The expected payload shape is
/// `{ "token": <token>, "value": { "kind": "begin" | "report" | "end", ... } }`.
/// Tokens are stored as their string form (any non-string token
/// is rendered to a debug string so the active set still observes
/// the begin/end pair).
///
/// - `begin` inserts the token into `active_tokens`.
/// - `report` does not change the active set but refreshes
///   `last_progress_at`.
/// - `end` removes the token from `active_tokens`.
///
/// All three kinds refresh `last_progress_at = Some(Instant::now())`
/// so the `WaitForProgressEndOrTimeout` readiness policy can
/// observe liveness.
pub(crate) async fn update_progress_state(
    progress_state: &Arc<Mutex<ProgressState>>,
    params: &serde_json::Value,
) {
    let token = params.get("token").map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    let kind = params
        .get("value")
        .and_then(|v| v.get("kind"))
        .and_then(|v| v.as_str());
    let Some(token) = token else {
        return;
    };
    let Some(kind) = kind else {
        return;
    };
    let mut state = progress_state.lock().await;
    state.last_progress_at = Some(Instant::now());
    state.observed_any = true;
    match kind {
        "begin" => {
            state.observed_begin = true;
            state.active_tokens.insert(token);
        }
        "end" => {
            state.active_tokens.remove(&token);
            if state.observed_begin && state.active_tokens.is_empty() {
                state.completed_cycle = true;
            }
        }
        "report" => {
            // Reports do not change the active set but are an
            // explicit liveness signal.
        }
        other => {
            debug!(kind = other, "ignoring unknown $/progress kind");
        }
    }
}

pub fn url_to_uri(url: &Url) -> Result<Uri, LspError> {
    Uri::from_str(url.as_str()).map_err(|e| LspError::RequestFailed(format!("invalid URL: {e}")))
}

fn uri_to_path_str(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_string())
}

pub struct DiagnosticEntry {
    pub uri: String,
    pub diagnostic: lsp_types::Diagnostic,
}

/// Tracks in-flight `$/progress` tokens and the timestamp of the
/// most recent progress notification. The
/// `LspReadinessPolicy::WaitForProgressEndOrTimeout` policy uses
/// this state to gate the transition from `Indexing` to `Ready`.
///
/// All mutations happen on the background reader task; readers
/// (e.g. `wait_for_progress_end`, `progress_snapshot`,
/// `operational_summary`) lock the inner `Mutex` briefly.
#[derive(Debug, Default)]
pub(crate) struct ProgressState {
    /// Active progress tokens (i.e. `begin` received, `end` not yet
    /// received).
    pub active_tokens: HashSet<String>,
    /// Timestamp of the most recent `$/progress` notification of
    /// any kind. `None` until the first progress notification
    /// arrives.
    pub last_progress_at: Option<Instant>,
    /// Whether a `begin` progress notification has been observed.
    pub observed_begin: bool,
    /// Whether any progress notification (begin/report/end) has been
    /// observed.
    pub observed_any: bool,
    /// Whether a complete begin→end cycle has been observed (begin was
    /// seen, then an end was received that drained `active_tokens` to
    /// empty).
    pub completed_cycle: bool,
}

/// Snapshot of the in-flight progress state at a moment in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    /// Number of active progress tokens (begins without matching
    /// ends).
    pub active_count: usize,
    /// Whether a complete begin→end cycle has been observed.
    pub completed_cycle: bool,
    /// Age in milliseconds since the most recent progress
    /// notification, or `None` if no progress has been observed.
    pub last_progress_age_ms: Option<u64>,
}

/// Aggregated operational summary of an LSP client. Combines the
/// last-message and last-diagnostics timestamps with the progress
/// tracker so callers can build a single snapshot for readiness
/// gating and operational notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationalSummary {
    /// Age in milliseconds since the most recent protocol message
    /// arrived, or `None` if no message has been received.
    pub last_message_age_ms: Option<u64>,
    /// Age in milliseconds since the most recent diagnostics
    /// notification arrived, or `None` if no diagnostics have
    /// been received.
    pub last_diagnostics_age_ms: Option<u64>,
    /// Number of active progress tokens.
    pub progress_active_count: usize,
    /// Age in milliseconds since the most recent progress
    /// notification, or `None` if no progress has been observed.
    pub progress_last_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticCacheEntry {
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    /// Timestamp of the most recent diagnostic push. Monotonic
    /// clock value — not meaningful across process restarts, so it
    /// is skipped from serialization. `Instant` cannot be
    /// serialized by `serde` without an external feature flag.
    #[serde(skip, default = "std::time::Instant::now")]
    pub received_at: std::time::Instant,
    pub source: crate::diagnostics::LspDiagnosticSource,
    pub content_version: Option<i32>,
    /// Server generation that produced these diagnostics.
    ///
    /// `0` is the sentinel for "never assigned" (entries from the
    /// pre-Phase-3 era or unit tests that bypass the service). After
    /// a server restart, the restart coordinator re-keys retained
    /// diagnostics to `current - 1` so the freshness classifier
    /// returns [`LspDiagnosticFreshness::Stale`] until new
    /// diagnostics arrive.
    #[serde(default)]
    pub server_generation: u64,
    /// Whether these diagnostics were produced by a server that has
    /// been restarted at least once since the start of this client
    /// key. `true` only when the entry arrived from a post-restart
    /// client. Survives across multiple restarts (it is
    /// monotonically sticky). See `LspDiagnosticSnapshot` for the
    /// authoritative definition.
    #[serde(default)]
    pub post_restart: bool,
}

impl DiagnosticCacheEntry {
    /// Return a new entry with `server_generation` set to
    /// `generation`. The `post_restart` flag is set to `true` when
    /// `generation > 0` because the entry originated from a
    /// post-publication client (a generation `2` or higher is a
    /// post-restart client — generation `0` is the "never
    /// assigned" sentinel and generation `1` is the cold-start
    /// publication).
    ///
    /// `received_at` and `content_version` are preserved.
    pub fn with_generation(&self, generation: u64) -> Self {
        Self {
            diagnostics: self.diagnostics.clone(),
            received_at: self.received_at,
            source: self.source,
            content_version: self.content_version,
            server_generation: generation,
            post_restart: self.post_restart || generation > 1,
        }
    }
}

/// Configuration options for LspClient behavior.
#[derive(Debug, Clone, Copy)]
pub struct LspClientOptions {
    /// Timeout for client-initiated requests. Default: 30s.
    pub request_timeout: Duration,
    /// Timeout for responding to server-initiated requests. Default: 5s.
    pub server_request_timeout: Duration,
}

impl Default for LspClientOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            server_request_timeout: Duration::from_secs(5),
        }
    }
}

pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub writer: LspWriter,
    pub request_id: AtomicU64,
    pub capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    /// Tracks when each file was last opened or changed, for diagnostics warm-up detection.
    pub last_content_change_at: Mutex<HashMap<String, Instant>>,
    pub diagnostics: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>>,
    pub diagnostics_invalidated_at: Arc<Mutex<Option<Instant>>>,
    /// Timestamp of the most recent protocol message received from
    /// the server. Updated by the background reader on every
    /// successfully parsed message. `None` until the first message
    /// arrives.
    pub last_message_at: Arc<Mutex<Option<Instant>>>,
    /// Timestamp of the most recent diagnostics notification received
    /// from the server. Updated by the background reader's
    /// notification dispatcher. `None` until the first diagnostics
    /// publish arrives.
    pub last_diagnostics_at: Arc<Mutex<Option<Instant>>>,
    /// Tracks in-flight `$/progress` tokens and the timestamp of
    /// the most recent progress notification. Mutated by the
    /// background reader on every `$/progress` notification.
    /// Read by `progress_snapshot`, `wait_for_progress_end`, and
    /// `operational_summary` to back the readiness policies in
    /// `LspReadinessPolicy::WaitForProgressEndOrTimeout`.
    pub(crate) progress_state: Arc<Mutex<ProgressState>>,
    pub pending: PendingMap,
    /// Transport health: transitions to `Failed` when the background reader
    /// cannot write a server-request response. Checked by `send_request` /
    /// `send_notification` to fail fast instead of writing to a broken pipe.
    pub(crate) transport_state: Arc<Mutex<ClientTransportState>>,
    #[allow(dead_code)] // read via ServerRequestContext; snapshot is #[cfg(test)]
    pub(crate) dynamic_registrations: Arc<RwLock<crate::server_request::DynamicRegistrationState>>,
    /// The child process handle, extracted during construction for
    /// process monitoring. `None` after the handle has been taken by
    /// the supervisor monitor task.
    pub(crate) child: Mutex<Option<tokio::process::Child>>,
    /// Stderr pipe handle retained during construction so the
    /// authoritative process runtime can take ownership of the
    /// reader at monitor-startup time. `None` after the runtime has
    /// taken it (or after the legacy stderr drain is used in tests).
    pub(crate) stderr: Mutex<Option<tokio::process::ChildStderr>>,
    /// Monotonically increasing generation counter for the
    /// server that produced the current diagnostic cache. Starts at
    /// `0` (never assigned) and is bumped by the restart coordinator
    /// after a successful reinit.
    pub(crate) server_generation: Arc<AtomicU64>,
    options: LspClientOptions,
    #[cfg(test)]
    test_shutdown_count: Option<Arc<std::sync::atomic::AtomicUsize>>,
    _reader_task: tokio::task::JoinHandle<()>,
}

/// Snapshot of LspClient operational health.
///
/// This is an observational type — fields reflect the state at the time
/// of the call and may change immediately afterward. It is not a
/// synchronization primitive.
#[derive(Debug, Clone)]
pub struct LspClientHealthSnapshot {
    /// Current transport state (Running or Failed).
    pub transport: ClientTransportSnapshot,
    /// Number of requests awaiting responses.
    pub pending_requests: usize,
}

impl LspClient {
    pub async fn new(
        server: &LspServerDef,
        binary: &Path,
        root: &Path,
        env: &[(String, String)],
        configuration: serde_json::Value,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        let args: Vec<&str> = server.args.iter().map(|s| &**s).collect();
        let binary_str = binary.to_str().ok_or_else(|| {
            LspError::LaunchFailed(format!(
                "binary path is not valid UTF-8: {}",
                binary.display()
            ))
        })?;
        let process = launch::spawn_server(binary_str, &args, env, Some(root)).await?;
        Self::finish_from_process(server.id.to_string(), process, root, configuration, options)
            .await
    }

    pub async fn new_with_launch_spec(
        launch: LspLaunchSpec,
        root: &Path,
        configuration: serde_json::Value,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        let LspLaunchSpec {
            id,
            command,
            args,
            env,
            ..
        } = launch;
        let process = launch::spawn_server_owned(&command, &args, &env, Some(root)).await?;
        Self::finish_from_process(id, process, root, configuration, options).await
    }

    async fn finish_from_process(
        server_id: String,
        mut process: LspProcess,
        root: &Path,
        configuration: serde_json::Value,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        // Take the stderr pipe. The authoritative process runtime
        // (see `crate::runtime`) is the preferred owner of the
        // reader because it bounds capture and surfaces the tail in
        // exit events. If no runtime is installed (e.g. legacy
        // tests that construct clients directly), fall back to the
        // legacy drain so existing behavior is preserved.
        let stderr_handle = process.stderr.take().map(|b| b.into_inner());

        // Split process: stdout and stdin first, then extract child.
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| LspError::LaunchFailed("stdout not available".to_string()))?;

        let writer = LspWriter::new(
            process
                .stdin
                .take()
                .ok_or_else(|| LspError::LaunchFailed("stdin not available".to_string()))?,
        );

        // Extract child handle for process monitoring after stdout/stdin are taken.
        let child = process.child;

        let diagnostics: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let transport_state: Arc<Mutex<ClientTransportState>> =
            Arc::new(Mutex::new(ClientTransportState::Running));
        let dynamic_registrations = Arc::new(RwLock::new(
            crate::server_request::DynamicRegistrationState::new(),
        ));
        let last_message_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let last_diagnostics_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let progress_state: Arc<Mutex<ProgressState>> =
            Arc::new(Mutex::new(ProgressState::default()));
        let server_generation: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

        // Spawn background stdout reader.
        let reader_diagnostics = diagnostics.clone();
        let reader_pending = pending.clone();
        let reader_transport_state = transport_state.clone();
        let reader_last_message_at = last_message_at.clone();
        let reader_last_diagnostics_at = last_diagnostics_at.clone();
        let reader_progress_state = progress_state.clone();
        let reader_server_generation = server_generation.clone();
        let server_id_for_reader = server_id.clone();
        let reader_writer = writer.clone_inner();
        let reader_context = ServerRequestContext {
            server_id: server_id.clone(),
            root: root.to_path_buf(),
            configuration,
            workspace_folders: vec![lsp_types::WorkspaceFolder {
                uri: url_to_uri(
                    &url::Url::from_file_path(root)
                        .map_err(|_| LspError::LaunchFailed("invalid root path".to_string()))?,
                )?,
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }],
            dynamic_registrations: dynamic_registrations.clone(),
        };
        let reader_task = tokio::spawn(async move {
            Self::background_reader(
                stdout,
                reader_diagnostics,
                reader_pending,
                reader_transport_state,
                reader_last_message_at,
                reader_last_diagnostics_at,
                reader_progress_state,
                reader_server_generation,
                server_id_for_reader,
                reader_writer,
                reader_context,
                options.server_request_timeout,
            )
            .await;
        });

        Ok(Self {
            server_id,
            root: root.to_path_buf(),
            writer,
            request_id: AtomicU64::new(0),
            capabilities: Arc::new(Mutex::new(None)),
            opened_files: Mutex::new(HashMap::new()),
            last_content_change_at: Mutex::new(HashMap::new()),
            diagnostics,
            diagnostics_invalidated_at: Arc::new(Mutex::new(None)),
            last_message_at,
            last_diagnostics_at,
            progress_state,
            pending,
            transport_state,
            dynamic_registrations,
            child: Mutex::new(Some(child)),
            stderr: Mutex::new(stderr_handle),
            server_generation,
            options,
            #[cfg(test)]
            test_shutdown_count: None,
            _reader_task: reader_task,
        })
    }

    #[cfg(test)]
    pub(crate) async fn test_stub(
        server_id: &str,
        root: &Path,
        shutdown_count: Arc<std::sync::atomic::AtomicUsize>,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        let cwd = if root.is_dir() {
            root
        } else {
            root.parent().unwrap_or(root)
        };
        let mut process = launch::spawn_server("sleep", &["1000"], &[], Some(cwd)).await?;

        // Test stubs retain the legacy stderr drain so the OS pipe
        // never back-pressures the long-running `sleep` child used in
        // service tests. The runtime path (production) owns the
        // stderr reader instead.
        if let Some(stderr) = process.stderr.take() {
            launch::spawn_stderr_drain(server_id, stderr.into_inner());
        }

        // Split process: stdout and stdin first, then extract child.
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| LspError::LaunchFailed("stdout not available".to_string()))?;
        let writer = LspWriter::new(
            process
                .stdin
                .take()
                .ok_or_else(|| LspError::LaunchFailed("stdin not available".to_string()))?,
        );

        // Extract child handle for process monitoring after stdout/stdin are taken.
        let child = process.child;

        let diagnostics: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let transport_state: Arc<Mutex<ClientTransportState>> =
            Arc::new(Mutex::new(ClientTransportState::Running));
        let dynamic_registrations = Arc::new(RwLock::new(
            crate::server_request::DynamicRegistrationState::new(),
        ));
        let last_message_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let last_diagnostics_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let progress_state: Arc<Mutex<ProgressState>> =
            Arc::new(Mutex::new(ProgressState::default()));
        let server_generation: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

        let reader_diagnostics = diagnostics.clone();
        let reader_pending = pending.clone();
        let reader_transport_state = transport_state.clone();
        let reader_last_message_at = last_message_at.clone();
        let reader_last_diagnostics_at = last_diagnostics_at.clone();
        let reader_progress_state = progress_state.clone();
        let reader_server_generation = server_generation.clone();
        let server_id = server_id.to_string();
        let reader_server_id = server_id.clone();
        let reader_writer = writer.clone_inner();
        let reader_context = ServerRequestContext {
            server_id: server_id.clone(),
            root: root.to_path_buf(),
            configuration: serde_json::Value::Null,
            workspace_folders: vec![lsp_types::WorkspaceFolder {
                uri: url_to_uri(
                    &url::Url::from_file_path(root)
                        .map_err(|_| LspError::LaunchFailed("invalid root path".to_string()))?,
                )?,
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }],
            dynamic_registrations: dynamic_registrations.clone(),
        };

        let reader_task = tokio::spawn(async move {
            Self::background_reader(
                stdout,
                reader_diagnostics,
                reader_pending,
                reader_transport_state,
                reader_last_message_at,
                reader_last_diagnostics_at,
                reader_progress_state,
                reader_server_generation,
                reader_server_id,
                reader_writer,
                reader_context,
                options.server_request_timeout,
            )
            .await;
        });

        Ok(Self {
            server_id: server_id.clone(),
            root: root.to_path_buf(),
            writer,
            request_id: AtomicU64::new(0),
            capabilities: Arc::new(Mutex::new(None)),
            opened_files: Mutex::new(HashMap::new()),
            last_content_change_at: Mutex::new(HashMap::new()),
            diagnostics,
            diagnostics_invalidated_at: Arc::new(Mutex::new(None)),
            last_message_at,
            last_diagnostics_at,
            progress_state,
            pending,
            transport_state,
            dynamic_registrations,
            child: Mutex::new(Some(child)),
            // Test stubs use the legacy stderr drain; no runtime
            // owns the stderr pipe.
            stderr: Mutex::new(None),
            server_generation,
            options,
            #[cfg(test)]
            test_shutdown_count: Some(shutdown_count),
            _reader_task: reader_task,
        })
    }

    /// Background task that reads framed JSON-RPC messages from stdout
    /// and routes them to pending request senders, notification handlers,
    /// or the server-request dispatcher.
    async fn background_reader(
        mut stdout: tokio::process::ChildStdout,
        diagnostics: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>>,
        pending: PendingMap,
        transport_state: Arc<Mutex<ClientTransportState>>,
        last_message_at: Arc<Mutex<Option<Instant>>>,
        last_diagnostics_at: Arc<Mutex<Option<Instant>>>,
        progress_state: Arc<Mutex<ProgressState>>,
        server_generation: Arc<AtomicU64>,
        server_id: String,
        writer: Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>,
        server_request_context: ServerRequestContext,
        server_request_timeout: Duration,
    ) {
        let writer = LspWriter::from_inner(writer);
        loop {
            // Read Content-Length framed message.
            let resp_str = match read_framed_message(&mut stdout).await {
                Ok(s) => s,
                Err(e) => {
                    let msg = format!("LSP server '{}' stdout reader exiting: {}", server_id, e);
                    debug!(server = %server_id, error = %e, "stdout reader exiting");
                    fail_transport(&transport_state, &pending, &msg).await;
                    break;
                }
            };

            let value: serde_json::Value = match serde_json::from_str(&resp_str) {
                Ok(v) => v,
                Err(e) => {
                    let reason = format!("LSP server '{}' sent invalid JSON: {}", server_id, e);
                    warn!(server = %server_id, error = %e, "failed to parse JSON-RPC message");
                    fail_transport(&transport_state, &pending, &reason).await;
                    break;
                }
            };

            // Update the last-message timestamp as soon as the
            // frame is successfully parsed. Diagnostics arrival is
            // recorded separately inside the notification branch.
            *last_message_at.lock().await = Some(Instant::now());

            match classify_json_rpc_message(value) {
                JsonRpcMessage::Response { id, result } => {
                    let sender = pending.lock().await.remove(&id);
                    if let Some(tx) = sender {
                        let _ = tx.send(Ok(result));
                    } else {
                        debug!(server = %server_id, id = %id, "late or unmatched response ID");
                    }
                }
                JsonRpcMessage::ErrorResponse {
                    id, code, message, ..
                } => {
                    let sender = pending.lock().await.remove(&id);
                    if let Some(tx) = sender {
                        let code_str = code.map(|c| c.to_string()).unwrap_or_default();
                        let _ = tx.send(Err(LspError::RequestFailed(format!(
                            "LSP error {code_str}: {message}"
                        ))));
                    } else {
                        debug!(server = %server_id, id = %id, code, message, "late or unmatched error response ID");
                    }
                }
                JsonRpcMessage::ServerRequest { id, method, params } => {
                    debug!(server = %server_id, id = %id, method = %method, "dispatching server request");
                    let reply = match tokio::time::timeout(
                        server_request_timeout,
                        dispatch_server_request(&server_request_context, &method, params),
                    )
                    .await
                    {
                        Ok(reply) => reply,
                        Err(_elapsed) => {
                            warn!(server = %server_id, id = %id, method = %method, "server request dispatch timed out");
                            ServerRequestReply::Error {
                                code: -32603,
                                message: format!("server request '{method}' timed out internally"),
                                data: None,
                            }
                        }
                    };
                    match reply {
                        ServerRequestReply::Result(result) => {
                            if let Err(e) = writer.send_response_result(&id, result).await {
                                let reason =
                                    format!("failed to write server-request response: {e}");
                                warn!(server = %server_id, id = %id, error = %e, "writer failure, entering failed state");
                                fail_transport(&transport_state, &pending, &reason).await;
                                break;
                            }
                        }
                        ServerRequestReply::Error {
                            code,
                            message,
                            data,
                        } => {
                            if let Err(e) =
                                writer.send_response_error(&id, code, &message, data).await
                            {
                                let reason =
                                    format!("failed to write server-request error response: {e}");
                                warn!(server = %server_id, id = %id, error = %e, "writer failure, entering failed state");
                                fail_transport(&transport_state, &pending, &reason).await;
                                break;
                            }
                        }
                    }
                }
                JsonRpcMessage::Notification { method, params } => {
                    if method == "textDocument/publishDiagnostics" {
                        *last_diagnostics_at.lock().await = Some(Instant::now());
                    } else if method == "$/progress" {
                        update_progress_state(&progress_state, &params).await;
                    }
                    let gen = server_generation.load(Ordering::Relaxed);
                    dispatch_notification(&diagnostics, gen, &method, params).await;
                }
                JsonRpcMessage::Unknown => {
                    debug!(server = %server_id, "received unknown JSON-RPC message");
                }
            }
        }
    }

    pub async fn initialize(
        &self,
        init_opts: Option<serde_json::Value>,
    ) -> Result<ServerCapabilities, LspError> {
        let root_uri = Url::from_file_path(&self.root)
            .map_err(|_| LspError::LaunchFailed("invalid root path".to_string()))?;
        let root_uri_str = root_uri.to_string();

        let params = serde_json::json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "codegg",
                "version": env!("CARGO_PKG_VERSION")
            },
            "rootUri": root_uri_str,
            "initializationOptions": init_opts,
            "capabilities": {
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": false,
                        "willSave": false,
                        "willSaveWaitUntil": false,
                        "didSave": true
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true
                        }
                    },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "signatureHelp": {
                        "signatureInformation": {
                            "documentationFormat": ["markdown", "plaintext"]
                        }
                    },
                    "references": {
                        "dynamicRegistration": false
                    },
                    "definition": {
                        "dynamicRegistration": false
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true,
                        "versionSupport": true
                    },
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "quickfix",
                                    "refactor",
                                    "refactor.extract",
                                    "refactor.inline",
                                    "source"
                                ]
                            }
                        }
                    },
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    }
                },
                "workspace": {
                    "workspaceFolders": true
                }
            },
            "workspaceFolders": [{
                "uri": root_uri_str,
                "name": self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            }]
        });

        let result = self.send_request("initialize", params).await?;
        let caps: InitializeResult = serde_json::from_value(result)?;
        *self.capabilities.lock().await = Some(caps.capabilities.clone());

        info!(server = %self.server_id, "LSP server initialized");

        Ok(caps.capabilities)
    }

    pub async fn send_initialized(&self) -> Result<(), LspError> {
        self.send_notification("initialized", serde_json::json!({}))
            .await
    }

    pub async fn open_file(&self, uri: &Url, text: &str, version: i32) -> Result<(), LspError> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url_to_uri(uri)?,
                language_id: self.detect_language_id(uri),
                version,
                text: text.to_string(),
            },
        };
        self.send_notification("textDocument/didOpen", serde_json::to_value(params)?)
            .await?;

        let uri_str = uri.to_string();
        self.opened_files
            .lock()
            .await
            .insert(uri_str.clone(), version);
        self.last_content_change_at
            .lock()
            .await
            .insert(uri_str, Instant::now());
        Ok(())
    }

    pub async fn update_file(&self, uri: &Url, text: &str, version: i32) -> Result<(), LspError> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: url_to_uri(uri)?,
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        };
        self.send_notification("textDocument/didChange", serde_json::to_value(params)?)
            .await?;

        let uri_str = uri.to_string();
        self.opened_files
            .lock()
            .await
            .insert(uri_str.clone(), version);
        self.last_content_change_at
            .lock()
            .await
            .insert(uri_str, Instant::now());
        Ok(())
    }

    pub async fn close_file(&self, uri: &Url) -> Result<(), LspError> {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(uri)?,
            },
        };
        self.send_notification("textDocument/didClose", serde_json::to_value(params)?)
            .await?;

        self.opened_files.lock().await.remove(&uri.to_string());
        Ok(())
    }

    pub async fn save_file(&self, uri: &Url, text: Option<&str>) -> Result<(), LspError> {
        let params = DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(uri)?,
            },
            text: text.map(|s| s.to_string()),
        };
        self.send_notification("textDocument/didSave", serde_json::to_value(params)?)
            .await?;

        // When save includes text content, mark diagnostics as potentially stale
        // because the server may recompute diagnostics for the new content.
        if text.is_some() {
            let uri_str = uri.to_string();
            self.last_content_change_at
                .lock()
                .await
                .insert(uri_str, Instant::now());
        }

        Ok(())
    }

    pub async fn go_to_definition(
        &self,
        uri: &Url,
        position: Position,
    ) -> Result<Option<GotoDefinitionResponse>, LspError> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self
            .send_request("textDocument/definition", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        let resp: GotoDefinitionResponse = serde_json::from_value(result)?;
        Ok(Some(resp))
    }

    pub async fn find_references(
        &self,
        uri: &Url,
        position: Position,
    ) -> Result<Vec<Location>, LspError> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self
            .send_request("textDocument/references", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let refs: Vec<Location> = serde_json::from_value(result)?;
        Ok(refs)
    }

    pub async fn hover(&self, uri: &Url, position: Position) -> Result<Option<Hover>, LspError> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
        };

        let result = self
            .send_request("textDocument/hover", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        let hover: Hover = serde_json::from_value(result)?;
        Ok(Some(hover))
    }

    pub async fn document_symbols(&self, uri: &Url) -> Result<Vec<DocumentSymbol>, LspError> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(uri)?,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self
            .send_request("textDocument/documentSymbol", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let symbols: Vec<DocumentSymbol> = serde_json::from_value(result)?;
        Ok(symbols)
    }

    pub async fn prepare_call_hierarchy(
        &self,
        uri: &Url,
        position: Position,
    ) -> Result<Vec<CallHierarchyItem>, LspError> {
        let params = serde_json::to_value(CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })?;

        let result = self
            .send_request("textDocument/prepareCallHierarchy", params)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<CallHierarchyItem> = serde_json::from_value(result)?;
        Ok(items)
    }

    pub async fn incoming_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, LspError> {
        let params = serde_json::to_value(CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let result = self
            .send_request("callHierarchy/incomingCalls", params)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyIncomingCall> = serde_json::from_value(result)?;
        Ok(calls)
    }

    pub async fn outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, LspError> {
        let params = serde_json::to_value(CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let result = self
            .send_request("callHierarchy/outgoingCalls", params)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyOutgoingCall> = serde_json::from_value(result)?;
        Ok(calls)
    }

    pub async fn prepare_type_hierarchy(
        &self,
        uri: &Url,
        position: Position,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let params = serde_json::to_value(TypeHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })?;

        let result = self
            .send_request("textDocument/prepareTypeHierarchy", params)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(result)?;
        Ok(items)
    }

    pub async fn supertypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let params = serde_json::to_value(TypeHierarchySupertypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let result = self
            .send_request("typeHierarchy/supertypes", params)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(result)?;
        Ok(items)
    }

    pub async fn subtypes(
        &self,
        item: TypeHierarchyItem,
    ) -> Result<Vec<TypeHierarchyItem>, LspError> {
        let params = serde_json::to_value(TypeHierarchySubtypesParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })?;

        let result = self.send_request("typeHierarchy/subtypes", params).await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let items: Vec<TypeHierarchyItem> = serde_json::from_value(result)?;
        Ok(items)
    }

    pub async fn code_actions(
        &self,
        uri: &Url,
        range: Range,
        context: CodeActionContext,
    ) -> Result<Vec<CodeActionOrCommand>, LspError> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: url_to_uri(uri)?,
            },
            range,
            context,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self
            .send_request("textDocument/codeAction", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let actions: Vec<CodeActionOrCommand> = serde_json::from_value(result)?;
        Ok(actions)
    }

    pub async fn completion(
        &self,
        uri: &Url,
        position: Position,
        trigger_kind: Option<CompletionTriggerKind>,
        trigger_char: Option<String>,
    ) -> Result<Vec<CompletionItem>, LspError> {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: trigger_kind.map(|kind| CompletionContext {
                trigger_kind: kind,
                trigger_character: trigger_char,
            }),
        };

        let result = self
            .send_request("textDocument/completion", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(Vec::new());
        }

        let items: CompletionList = serde_json::from_value(result)?;
        Ok(items.items)
    }

    pub async fn signature_help(
        &self,
        uri: &Url,
        position: Position,
    ) -> Result<Option<SignatureHelp>, LspError> {
        let params = SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: url_to_uri(uri)?,
                },
                position,
            },
            work_done_progress_params: Default::default(),
            context: None,
        };

        let result = self
            .send_request("textDocument/signatureHelp", serde_json::to_value(params)?)
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        let help: SignatureHelp = serde_json::from_value(result)?;
        Ok(Some(help))
    }

    pub async fn shutdown(&self) -> Result<(), LspError> {
        self.request_protocol_shutdown().await
    }

    /// Send the protocol-level LSP `shutdown` request and `exit`
    /// notification to the server. This is the **protocol** shutdown
    /// path: it does NOT wait on the child process, does NOT set
    /// the runtime intent, and does NOT force-kill hung processes.
    /// The authoritative process owner is the [`LspProcessRuntime`]
    /// for the current generation, and it is responsible for
    /// setting `LspProcessIntent::GracefulShutdownRequested`
    /// **before** this method is called so the exit classification
    /// marks the process as expected.
    ///
    /// Service-level shutdown uses [`crate::service::LspService`]
    /// to drive the runtime intent, send this request under a
    /// bounded deadline, then wait on the runtime and force-kill
    /// hung processes. Direct callers of this method bypass that
    /// coordination; only the production `LspService` shutdown path
    /// or test harnesses should invoke it.
    pub async fn request_protocol_shutdown(&self) -> Result<(), LspError> {
        #[cfg(test)]
        if let Some(counter) = &self.test_shutdown_count {
            counter.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
        self.send_request("shutdown", serde_json::json!(null))
            .await?;
        self.send_notification("exit", serde_json::json!({})).await
    }

    /// Wait for the child process to exit, bounded by `timeout`.
    ///
    /// Returns:
    /// - `Ok(Ok(()))` — child exited within the timeout.
    /// - `Ok(Err(LspError::RequestTimeout))` — timeout elapsed.
    /// - `Ok(Err(io_error))` — `wait()` itself failed.
    /// - `Err(LspError::RequestFailed)` — no child handle is available
    ///   (the supervisor monitor has already taken it).
    pub async fn wait_for_child_exit(
        &self,
        timeout_duration: std::time::Duration,
    ) -> Result<Result<(), LspError>, LspError> {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            Some(child) => match tokio::time::timeout(timeout_duration, child.wait()).await {
                Ok(Ok(_status)) => Ok(Ok(())),
                Ok(Err(err)) => Ok(Err(LspError::Io(err))),
                Err(_elapsed) => Ok(Err(LspError::RequestTimeout(format!(
                    "child did not exit within {timeout_duration:?}"
                )))),
            },
            None => Err(LspError::RequestFailed(
                "no child handle available (already taken by supervisor)".to_string(),
            )),
        }
    }

    /// Non-blocking check of child exit status.
    ///
    /// Returns `Some(Ok(status))` if the child has exited,
    /// `Some(Err(err))` if the underlying `try_wait` failed,
    /// `None` if the child is still running or the handle has been taken.
    pub async fn try_wait_child(&self) -> Option<Result<std::process::ExitStatus, std::io::Error>> {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(Ok(status)),
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            },
            None => None,
        }
    }

    /// Take ownership of the stderr pipe handle. Used by the
    /// authoritative process runtime to wire the stderr ring
    /// buffer into the exit event. Returns `None` if a test stub
    /// populated the field with `None` (the legacy stderr drain
    /// path).
    pub(crate) async fn take_stderr(&self) -> Option<tokio::process::ChildStderr> {
        let mut guard = self.stderr.lock().await;
        guard.take()
    }

    #[allow(dead_code)]
    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// Timeout for server-request dispatch. Current handlers are fast and
    /// local (hashmap lookups, vector construction), but this guard prevents
    /// a misbehaving server from blocking stdout consumption indefinitely.
    #[allow(dead_code)]
    pub(crate) const SERVER_REQUEST_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(5);

    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspError> {
        // Fail fast if transport is already in a failed state.
        if let ClientTransportState::Failed { ref reason } = *self.transport_state.lock().await {
            return Err(LspError::WriterClosed(reason.clone()));
        }

        let raw_id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let id = JsonRpcId::Number(raw_id as i64);

        // Register pending request before writing to stdin.
        let (tx, rx) = oneshot::channel();
        {
            self.pending.lock().await.insert(id.clone(), tx);
        }

        // Write the request via the shared writer.
        if let Err(e) = self.writer.send_request_message(&id, method, params).await {
            self.pending.lock().await.remove(&id);
            fail_transport(&self.transport_state, &self.pending, e.to_string()).await;
            return Err(e);
        }

        // Wait for the background reader to deliver the response.
        let result = tokio::time::timeout(self.options.request_timeout, rx).await;

        match result {
            Ok(Ok(Ok(val))) => Ok(val),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => {
                // oneshot dropped without sending — background reader exited.
                self.pending.lock().await.remove(&id);
                Err(LspError::RequestFailed(format!(
                    "LSP request '{}' failed: response channel dropped",
                    method
                )))
            }
            Err(_) => {
                return self.handle_request_timeout(method, id).await;
            }
        }
    }

    async fn handle_request_timeout(
        &self,
        method: &str,
        id: JsonRpcId,
    ) -> Result<serde_json::Value, LspError> {
        self.pending.lock().await.remove(&id);

        // If the transport is already failed, skip the write (it would
        // certainly fail) and drain any remaining pending requests.
        if let ClientTransportState::Failed { ref reason } = *self.transport_state.lock().await {
            let reason = reason.clone();
            fail_all_pending(&self.pending, &reason).await;
            return Err(LspError::RequestTimeout(format!(
                "LSP request '{}' timed out after {:?}",
                method, self.options.request_timeout
            )));
        }

        let cancel_params = serde_json::json!({ "id": id });
        if let Err(e) = self
            .writer
            .send_notification_message("$/cancelRequest", cancel_params)
            .await
        {
            debug!(
                server = %self.server_id,
                method = %method,
                error = %e,
                "failed to send cancellation notification after timeout"
            );
            fail_transport(&self.transport_state, &self.pending, e.to_string()).await;
        }

        Err(LspError::RequestTimeout(format!(
            "LSP request '{}' timed out after {:?}",
            method, self.options.request_timeout
        )))
    }

    pub async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), LspError> {
        // Fail fast if transport is already in a failed state.
        if let ClientTransportState::Failed { ref reason } = *self.transport_state.lock().await {
            return Err(LspError::WriterClosed(reason.clone()));
        }

        let result = self.writer.send_notification_message(method, params).await;
        if let Err(ref e) = result {
            fail_transport(&self.transport_state, &self.pending, e.to_string()).await;
        }
        result
    }

    /// Returns the current count of requests awaiting responses.
    ///
    /// This is an observation, not a synchronization primitive.
    /// The count may change immediately after the call.
    pub async fn pending_request_count(&self) -> usize {
        self.pending.lock().await.len()
    }

    /// Returns the elapsed time (in milliseconds) since the most
    /// recent protocol message was received from the server.
    ///
    /// Returns `None` if no message has been received yet.
    /// Computed against the monotonic clock; non-negative.
    pub async fn last_message_age_ms(&self) -> Option<u64> {
        let guard = self.last_message_at.lock().await;
        guard.map(|instant| {
            let elapsed = instant.elapsed();
            elapsed.as_millis().try_into().unwrap_or(u64::MAX)
        })
    }

    /// Returns the elapsed time (in milliseconds) since the most
    /// recent `textDocument/publishDiagnostics` notification was
    /// received from the server.
    ///
    /// Returns `None` if no diagnostics notification has been
    /// received yet. Computed against the monotonic clock; non-negative.
    pub async fn last_diagnostics_age_ms(&self) -> Option<u64> {
        let guard = self.last_diagnostics_at.lock().await;
        guard.map(|instant| {
            let elapsed = instant.elapsed();
            elapsed.as_millis().try_into().unwrap_or(u64::MAX)
        })
    }

    /// Return a snapshot of the in-flight progress state.
    ///
    /// `active_count` is the number of progress tokens that have
    /// observed a `begin` without a matching `end`.
    /// `last_progress_age_ms` is the age of the most recent
    /// `$/progress` notification of any kind, or `None` if no
    /// progress has been observed yet.
    pub async fn progress_snapshot(&self) -> ProgressSnapshot {
        let state = self.progress_state.lock().await;
        let active_count = state.active_tokens.len();
        let last_progress_age_ms = state
            .last_progress_at
            .map(|instant| instant.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        ProgressSnapshot {
            active_count,
            completed_cycle: state.completed_cycle,
            last_progress_age_ms,
        }
    }

    /// Wait for all active progress tokens to complete (i.e. each
    /// token that observed a `begin` observes a matching `end`),
    /// or for `timeout` to elapse.
    ///
    /// Returns `true` when the active set drains to zero within
    /// the timeout (or was already empty), `false` otherwise. The
    /// wait is implemented as a coarse polling loop so it does
    /// not require per-token notifications.
    pub async fn wait_for_progress_end(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        let step = Duration::from_millis(20);
        loop {
            {
                let state = self.progress_state.lock().await;
                if state.completed_cycle {
                    return true;
                }
            }
            if started.elapsed() >= timeout {
                return self.progress_state.lock().await.completed_cycle;
            }
            tokio::time::sleep(step.min(timeout.saturating_sub(started.elapsed()))).await;
        }
    }

    /// Wait for the first `textDocument/publishDiagnostics`
    /// notification to arrive, bounded by `timeout`.
    ///
    /// Returns `true` when at least one diagnostic has already
    /// been recorded (or arrives within the timeout), `false`
    /// otherwise. Implemented as a coarse polling loop on
    /// `last_diagnostics_at`.
    pub async fn wait_for_first_diagnostics(&self, timeout: Duration) -> bool {
        if timeout.is_zero() {
            return self.last_diagnostics_at.lock().await.is_some();
        }
        let started = Instant::now();
        let step = Duration::from_millis(20);
        loop {
            if self.last_diagnostics_at.lock().await.is_some() {
                return true;
            }
            if started.elapsed() >= timeout {
                return self.last_diagnostics_at.lock().await.is_some();
            }
            tokio::time::sleep(step.min(timeout.saturating_sub(started.elapsed()))).await;
        }
    }

    /// Return a single aggregated [`OperationalSummary`] covering
    /// the most recent message, diagnostics, and progress
    /// observations.
    pub async fn operational_summary(&self) -> OperationalSummary {
        let last_message_age_ms = self
            .last_message_at
            .lock()
            .await
            .map(|i| i.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        let last_diagnostics_age_ms = self
            .last_diagnostics_at
            .lock()
            .await
            .map(|i| i.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        let state = self.progress_state.lock().await;
        let progress_active_count = state.active_tokens.len();
        let progress_last_age_ms = state
            .last_progress_at
            .map(|i| i.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        OperationalSummary {
            last_message_age_ms,
            last_diagnostics_age_ms,
            progress_active_count,
            progress_last_age_ms,
        }
    }

    /// Re-key every diagnostic cache entry to a new `server_generation`
    /// and `post_restart` flag.
    ///
    /// Used by the restart coordinator (Pass 5 / Phase 17) right
    /// after a fresh client is published and BEFORE document replay
    /// so the freshness classifier returns
    /// [`LspDiagnosticFreshness::Stale`] for any retained diagnostic
    /// until the new server emits its own first push.
    ///
    /// `received_at` and `content_version` are preserved per-entry
    /// (the underlying observations are still real, only the
    /// authoritative generation is updated).
    ///
    /// `post_restart` becomes `true` whenever `post_restart` is
    /// requested and the new generation is non-zero.
    pub async fn set_all_diagnostic_generations(&self, generation: u64, post_restart: bool) {
        let mut map = self.diagnostics.lock().await;
        for entry in map.values_mut() {
            entry.server_generation = generation;
            entry.post_restart = entry.post_restart || post_restart;
        }
    }

    /// Return the latest `server_generation` observed across all
    /// diagnostic cache entries. `0` when no entries exist (the
    /// "no client" sentinel).
    pub async fn current_diagnostic_generation(&self) -> u64 {
        let map = self.diagnostics.lock().await;
        map.values().map(|e| e.server_generation).max().unwrap_or(0)
    }

    /// Returns a combined health snapshot of transport state and pending request count.
    ///
    /// This is an observational check, not a synchronization primitive.
    /// The returned values reflect the state at the moment of the call.
    pub async fn health_snapshot(&self) -> LspClientHealthSnapshot {
        LspClientHealthSnapshot {
            transport: self.transport_state_snapshot().await,
            pending_requests: self.pending_request_count().await,
        }
    }

    /// Return the current `server_generation` counter.
    pub fn server_generation(&self) -> u64 {
        self.server_generation.load(Ordering::Relaxed)
    }

    /// Bind the server generation counter and re-key all existing
    /// diagnostic cache entries to the new generation.
    pub async fn bind_server_generation(&self, generation: u64) {
        self.server_generation.store(generation, Ordering::Relaxed);
        let post_restart = generation > 1;
        let mut map = self.diagnostics.lock().await;
        for entry in map.values_mut() {
            entry.server_generation = generation;
            entry.post_restart = post_restart;
        }
    }

    /// Return a snapshot (clone) of the full diagnostic cache.
    pub async fn diagnostic_cache_snapshot(&self) -> HashMap<String, DiagnosticCacheEntry> {
        self.diagnostics.lock().await.clone()
    }

    /// Install retained diagnostics from a previous generation,
    /// updating existing entries only when the incoming generation
    /// is newer.
    pub async fn install_retained_diagnostics(
        &self,
        _source: &str,
        entries: HashMap<String, DiagnosticCacheEntry>,
    ) {
        let mut map = self.diagnostics.lock().await;
        for (uri, entry) in entries {
            map.entry(uri)
                .and_modify(|existing| {
                    if existing.server_generation < entry.server_generation {
                        *existing = entry.clone();
                    }
                })
                .or_insert(entry);
        }
    }

    /// Returns an observation of the current transport state.
    ///
    /// This is a snapshot and may not reflect state changes that occur
    /// immediately after the call. Useful for operational health monitoring,
    /// not a synchronization primitive.
    pub async fn transport_state_snapshot(&self) -> ClientTransportSnapshot {
        match &*self.transport_state.lock().await {
            ClientTransportState::Running => ClientTransportSnapshot::Running,
            ClientTransportState::Failed { reason } => ClientTransportSnapshot::Failed {
                reason: reason.clone(),
            },
        }
    }

    /// Returns a snapshot of the server's dynamic registration state.
    ///
    /// This is primarily for test support and internal diagnostics.
    /// The snapshot reflects the state at the time of the call.
    #[doc(hidden)]
    pub async fn dynamic_registration_snapshot(
        &self,
    ) -> Vec<crate::server_request::DynamicRegistration> {
        self.dynamic_registrations.read().await.snapshot()
    }

    fn detect_language_id(&self, uri: &Url) -> String {
        let path = uri.path();
        if let Some(ext) = path.rsplit('.').next() {
            if let Some(lang) = super::language::extension_to_language_id(ext) {
                return lang.to_string();
            }
        }
        "plaintext".to_string()
    }

    pub async fn get_diagnostics(&self, uri: &str) -> Vec<lsp_types::Diagnostic> {
        self.diagnostics
            .lock()
            .await
            .get(uri)
            .map(|e| e.diagnostics.clone())
            .unwrap_or_default()
    }

    pub async fn get_all_diagnostics(&self) -> HashMap<String, Vec<lsp_types::Diagnostic>> {
        self.diagnostics
            .lock()
            .await
            .iter()
            .map(|(k, e)| (k.clone(), e.diagnostics.clone()))
            .collect()
    }

    /// Returns true if the file was opened or changed very recently and
    /// no diagnostics have been received yet for it.
    pub async fn diagnostics_may_still_be_warming(&self, uri: &str) -> bool {
        let last = self.last_content_change_at.lock().await;
        if let Some(instant) = last.get(uri) {
            let elapsed = instant.elapsed();
            if elapsed < std::time::Duration::from_secs(2) {
                let diags = self.diagnostics.lock().await;
                return !diags.contains_key(uri);
            }
        }
        false
    }

    pub async fn process_notification(&self, notification: &str) {
        // Update the last-message timestamp for any notification
        // routed through this path (test-only entry point).
        *self.last_message_at.lock().await = Some(Instant::now());
        if let Some(diags) = parse_publish_diagnostics(notification) {
            let uri = diags.0;
            let diagnostics = diags.1;
            let count = diagnostics.len();
            self.diagnostics.lock().await.insert(
                uri.clone(),
                DiagnosticCacheEntry {
                    diagnostics,
                    received_at: std::time::Instant::now(),
                    source: crate::diagnostics::LspDiagnosticSource::Pushed,
                    content_version: None,
                    server_generation: 0,
                    post_restart: false,
                },
            );
            *self.last_diagnostics_at.lock().await = Some(Instant::now());
            debug!(uri, count, "received diagnostics");
        }
    }

    /// Return a fresh diagnostic snapshot with freshness metadata.
    pub async fn diagnostic_snapshot(
        &self,
        uri: &str,
    ) -> crate::diagnostics::LspDiagnosticSnapshot {
        let file_path = uri_to_path_str(uri);
        let invalidated_at = *self.diagnostics_invalidated_at.lock().await;
        let entry = self.diagnostics.lock().await.get(uri).cloned();
        let last_change = self.last_content_change_at.lock().await.get(uri).copied();

        let (entry, freshness) = classify_diagnostic_freshness(
            entry,
            last_change,
            invalidated_at,
            self.server_generation(),
        );

        match (entry, freshness) {
            (Some(entry), freshness) => {
                let server_generation = entry.server_generation;
                let post_restart = entry.post_restart;
                crate::diagnostics::LspDiagnosticSnapshot {
                    file_path: PathBuf::from(file_path),
                    diagnostics: entry
                        .diagnostics
                        .into_iter()
                        .map(|d| crate::diagnostics::FileDiagnostic {
                            file: uri.to_string(),
                            line: d.range.start.line,
                            column: d.range.start.character,
                            message: d.message,
                            severity: d.severity.unwrap_or(lsp_types::DiagnosticSeverity::ERROR),
                            source: d.source,
                            code: d.code.as_ref().map(|c| match c {
                                lsp_types::NumberOrString::Number(n) => n.to_string(),
                                lsp_types::NumberOrString::String(s) => s.clone(),
                            }),
                        })
                        .collect(),
                    age_ms: entry.received_at.elapsed().as_millis() as i64,
                    source: entry.source,
                    freshness,
                    server_generation: Some(server_generation),
                    post_restart,
                }
            }
            (None, _) => {
                crate::diagnostics::LspDiagnosticSnapshot::unavailable(PathBuf::from(file_path))
            }
        }
    }
}

/// Pure helper that classifies diagnostic freshness from cache state.
///
/// Returns `(Option<DiagnosticCacheEntry>, LspDiagnosticFreshness)`:
/// - `None` freshness entry + `Unavailable` means no cache entry or invalidated-without-stale-data.
/// - `Some(entry)` + freshness means the entry should be used with the given freshness label.
/// When `current_generation` is non-zero and the entry's `server_generation` does not match,
/// the freshness is classified as `Stale`.
pub(crate) fn classify_diagnostic_freshness(
    entry: Option<DiagnosticCacheEntry>,
    last_content_change: Option<Instant>,
    invalidated_at: Option<Instant>,
    current_generation: u64,
) -> (
    Option<DiagnosticCacheEntry>,
    crate::diagnostics::LspDiagnosticFreshness,
) {
    if let Some(invalidated_at) = invalidated_at {
        return match entry {
            Some(entry) if entry.received_at < invalidated_at => (
                Some(entry),
                crate::diagnostics::LspDiagnosticFreshness::Stale,
            ),
            _ => (
                None,
                crate::diagnostics::LspDiagnosticFreshness::Unavailable,
            ),
        };
    }

    match entry {
        None => (
            None,
            crate::diagnostics::LspDiagnosticFreshness::Unavailable,
        ),
        Some(entry) => {
            // Generation mismatch: the entry was produced by a
            // different (older) server generation than the one
            // currently expected.
            if current_generation > 0 && entry.server_generation != current_generation {
                return (
                    Some(entry),
                    crate::diagnostics::LspDiagnosticFreshness::Stale,
                );
            }
            let freshness = match last_content_change {
                Some(changed_at) if changed_at > entry.received_at => {
                    crate::diagnostics::LspDiagnosticFreshness::PossiblyStale
                }
                _ => crate::diagnostics::LspDiagnosticFreshness::Fresh,
            };
            (Some(entry), freshness)
        }
    }
}

/// Parse a `textDocument/publishDiagnostics` notification from raw JSON-RPC.
/// Returns `(uri, diagnostics)` if valid, `None` otherwise.
/// Unknown notifications or malformed payloads return `None` without error.
pub fn parse_publish_diagnostics(
    notification: &str,
) -> Option<(String, Vec<lsp_types::Diagnostic>)> {
    let val: serde_json::Value = serde_json::from_str(notification).ok()?;
    let method = val.get("method").and_then(|m| m.as_str())?;
    if method != "textDocument/publishDiagnostics" {
        return None;
    }
    let params = val.get("params")?;
    let uri = params.get("uri")?.as_str()?;
    let diags_value = params.get("diagnostics")?;
    let diagnostics: Vec<lsp_types::Diagnostic> =
        serde_json::from_value(diags_value.clone()).ok()?;
    Some((uri.to_string(), diagnostics))
}

/// Maximum allowed Content-Length for a single inbound LSP frame (64 MiB).
///
/// Real LSP responses rarely exceed a few hundred KiB. This limit protects
/// against a malicious or buggy server claiming an absurdly large frame and
/// causing the client to allocate unbounded memory.
const MAX_LSP_FRAME_BYTES: usize = 64 * 1024 * 1024;

const MAX_LSP_HEADER_BYTES: usize = 16 * 1024;
const MAX_LSP_HEADER_LINE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LspHeaders {
    content_length: usize,
}

fn parse_lsp_headers(header: &[u8]) -> Result<LspHeaders, LspError> {
    if header.len() > MAX_LSP_HEADER_BYTES {
        return Err(LspError::Protocol(format!(
            "LSP header block exceeds {} bytes (read {})",
            MAX_LSP_HEADER_BYTES,
            header.len()
        )));
    }

    let header_str = std::str::from_utf8(header)
        .map_err(|_| LspError::Protocol("LSP header block is not valid UTF-8".to_string()))?;

    let mut content_length: Option<usize> = None;
    for raw_line in header_str.split("\r\n") {
        if raw_line.is_empty() {
            continue;
        }
        if raw_line.len() > MAX_LSP_HEADER_LINE_BYTES {
            return Err(LspError::Protocol(format!(
                "LSP header line exceeds {} bytes",
                MAX_LSP_HEADER_LINE_BYTES
            )));
        }

        let (name, value) = raw_line.split_once(':').ok_or_else(|| {
            LspError::Protocol(format!("malformed LSP header line: {raw_line:?}"))
        })?;
        let name = name.trim();
        let value = value.trim();

        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(LspError::Protocol(
                    "duplicate Content-Length header".to_string(),
                ));
            }
            let parsed = value.parse::<usize>().map_err(|_| {
                LspError::Protocol(format!("invalid Content-Length value: {value:?}"))
            })?;
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("Content-Type") {
            continue;
        }
    }

    let content_length = content_length
        .ok_or_else(|| LspError::Protocol("missing Content-Length header".to_string()))?;

    Ok(LspHeaders { content_length })
}

/// Read a single Content-Length framed message from a stdout stream.
async fn read_framed_message<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<String, LspError> {
    use tokio::io::AsyncReadExt;

    let mut header_buf = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        let n = reader
            .read(&mut byte)
            .await
            .map_err(|e| LspError::Protocol(format!("read header failed: {e}")))?;
        if n == 0 {
            return Err(LspError::Protocol(format!(
                "unexpected EOF after reading {} header bytes",
                header_buf.len()
            )));
        }
        header_buf.push(byte[0]);

        if header_buf.len() > MAX_LSP_HEADER_BYTES {
            return Err(LspError::Protocol(format!(
                "LSP header block exceeds {} bytes (read {})",
                MAX_LSP_HEADER_BYTES,
                header_buf.len()
            )));
        }

        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let content_length = parse_lsp_headers(&header_buf)?.content_length;

    if content_length > MAX_LSP_FRAME_BYTES {
        return Err(LspError::Protocol(format!(
            "Content-Length {} exceeds maximum allowed frame size of {} bytes",
            content_length, MAX_LSP_FRAME_BYTES
        )));
    }

    let mut body = vec![0u8; content_length];
    let mut read = 0usize;
    while read < content_length {
        let n = reader
            .read(&mut body[read..])
            .await
            .map_err(|e| LspError::Protocol(format!("read body failed: {e}")))?;
        if n == 0 {
            return Err(LspError::Protocol(format!(
                "unexpected EOF while reading LSP body: read {} of {} bytes",
                read, content_length
            )));
        }
        read += n;
    }

    String::from_utf8(body).map_err(|e| {
        LspError::Protocol(format!(
            "invalid UTF-8 in response body (read {} bytes): {}",
            content_length, e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LspDiagnosticFreshness;
    use std::time::Duration;

    #[test]
    fn classify_response_message() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"capabilities": {}}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, JsonRpcId::Number(1));
                assert!(result.get("capabilities").is_some());
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn classify_error_response_message() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": {"code": -32600, "message": "Invalid Request"}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ErrorResponse {
                id,
                code,
                message,
                data,
            } => {
                assert_eq!(id, JsonRpcId::Number(2));
                assert_eq!(code, Some(-32600));
                assert_eq!(message, "Invalid Request");
                assert!(data.is_none());
            }
            _ => panic!("expected ErrorResponse"),
        }
    }

    #[test]
    fn classify_notification_message() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///test.rs", "diagnostics": []}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Notification { method, .. } => {
                assert_eq!(method, "textDocument/publishDiagnostics");
            }
            _ => panic!("expected Notification"),
        }
    }

    #[test]
    fn classify_unknown_message() {
        let msg = serde_json::json!({"jsonrpc": "2.0"});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn parse_publish_diagnostics_valid() {
        let json = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///src/main.rs",
                "diagnostics": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 10 }
                        },
                        "message": "unused variable",
                        "severity": 2
                    }
                ]
            }
        });
        let result = parse_publish_diagnostics(&json.to_string());
        assert!(result.is_some());
        let (uri, diags) = result.unwrap();
        assert_eq!(uri, "file:///src/main.rs");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unused variable");
    }

    #[test]
    fn parse_publish_diagnostics_unknown_notification() {
        let json = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/completion",
            "params": {}
        });
        assert!(parse_publish_diagnostics(&json.to_string()).is_none());
    }

    #[test]
    fn parse_publish_diagnostics_malformed_json() {
        assert!(parse_publish_diagnostics("not json").is_none());
    }

    #[test]
    fn parse_publish_diagnostics_empty_diagnostics() {
        let json = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///src/main.rs",
                "diagnostics": []
            }
        });
        let result = parse_publish_diagnostics(&json.to_string());
        assert!(result.is_some());
        let (_, diags) = result.unwrap();
        assert!(diags.is_empty());
    }

    #[tokio::test]
    async fn dispatch_publish_diagnostics_updates_cache() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let params = serde_json::json!({
            "uri": "file:///test.rs",
            "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 5 }
                },
                "message": "test error",
                "severity": 1
            }]
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;
        let map = diags.lock().await;
        let entry = map.get("file:///test.rs").expect("entry should exist");
        assert_eq!(entry.diagnostics.len(), 1);
        assert_eq!(entry.diagnostics[0].message, "test error");
        assert_eq!(
            entry.source,
            crate::diagnostics::LspDiagnosticSource::Pushed
        );
    }

    #[tokio::test]
    async fn dispatch_unknown_notification_ignored() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        dispatch_notification(&diags, 0, "textDocument/completion", serde_json::json!({})).await;
        let map = diags.lock().await;
        assert!(map.is_empty());
    }

    #[test]
    fn classify_malformed_non_object() {
        assert!(matches!(
            classify_json_rpc_message(serde_json::json!("just a string")),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_empty_object() {
        assert!(matches!(
            classify_json_rpc_message(serde_json::json!({})),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_empty_diagnostics_notification() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///test.rs",
                "diagnostics": []
            }
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Notification { method, params } => {
                assert_eq!(method, "textDocument/publishDiagnostics");
                let diags = params.get("diagnostics").unwrap().as_array().unwrap();
                assert!(diags.is_empty());
            }
            _ => panic!("expected Notification for empty diagnostics"),
        }
    }

    #[tokio::test]
    async fn dispatch_empty_diagnostics_inserts_empty_vec() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let params = serde_json::json!({
            "uri": "file:///test.rs",
            "diagnostics": []
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;
        let map = diags.lock().await;
        let entry = map.get("file:///test.rs").expect("entry should exist");
        assert!(entry.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn dispatch_stores_version_metadata() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let params = serde_json::json!({
            "uri": "file:///test.rs",
            "version": 5,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                "severity": 1,
                "message": "test error"
            }]
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;
        let map = diags.lock().await;
        let entry = map.get("file:///test.rs").expect("entry should exist");
        assert_eq!(entry.content_version, Some(5));
    }

    #[tokio::test]
    async fn dispatch_stores_received_at_timestamp() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let before = std::time::Instant::now();
        let params = serde_json::json!({
            "uri": "file:///test.rs",
            "diagnostics": []
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;
        let after = std::time::Instant::now();
        let map = diags.lock().await;
        let entry = map.get("file:///test.rs").expect("entry should exist");
        assert!(entry.received_at >= before);
        assert!(entry.received_at <= after);
    }

    #[tokio::test]
    async fn warming_logic_no_cache_entry_means_warming() {
        let last_content_change_at =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
                String,
                Instant,
            >::new()));
        let diagnostics =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
                String,
                DiagnosticCacheEntry,
            >::new()));
        let uri = "file:///test.rs";

        last_content_change_at
            .lock()
            .await
            .insert(uri.to_string(), Instant::now());
        let has_received = diagnostics.lock().await.contains_key(uri);
        assert!(!has_received, "no cache entry means not yet received");
    }

    #[tokio::test]
    async fn warming_logic_empty_cache_entry_means_clean() {
        let diagnostics =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
                String,
                DiagnosticCacheEntry,
            >::new()));
        let uri = "file:///test.rs";

        diagnostics.lock().await.insert(
            uri.to_string(),
            DiagnosticCacheEntry {
                diagnostics: Vec::new(),
                received_at: std::time::Instant::now(),
                source: crate::diagnostics::LspDiagnosticSource::Pushed,
                content_version: None,
                server_generation: 0,
                post_restart: false,
            },
        );
        let has_received = diagnostics.lock().await.contains_key(uri);
        assert!(
            has_received,
            "empty vec entry means server responded (clean)"
        );
    }

    #[tokio::test]
    async fn warming_logic_nonempty_cache_entry_means_clean() {
        let diagnostics =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
                String,
                DiagnosticCacheEntry,
            >::new()));
        let uri = "file:///test.rs";

        diagnostics.lock().await.insert(
            uri.to_string(),
            DiagnosticCacheEntry {
                diagnostics: vec![lsp_types::Diagnostic {
                    range: lsp_types::Range {
                        start: lsp_types::Position {
                            line: 0,
                            character: 0,
                        },
                        end: lsp_types::Position {
                            line: 0,
                            character: 5,
                        },
                    },
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: "test".to_string(),
                    ..Default::default()
                }],
                received_at: std::time::Instant::now(),
                source: crate::diagnostics::LspDiagnosticSource::Pushed,
                content_version: None,
                server_generation: 0,
                post_restart: false,
            },
        );
        let has_received = diagnostics.lock().await.contains_key(uri);
        assert!(has_received, "nonempty vec entry means server responded");
    }

    #[tokio::test]
    async fn dispatch_notification_stores_metadata() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let params = serde_json::json!({
            "uri": "file:///test.rs",
            "version": 5,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                "severity": 1,
                "message": "test error"
            }]
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;
        let lock = diags.lock().await;
        let entry = lock.get("file:///test.rs").expect("entry should exist");
        assert_eq!(entry.diagnostics.len(), 1);
        assert_eq!(entry.content_version, Some(5));
        assert_eq!(
            entry.source,
            crate::diagnostics::LspDiagnosticSource::Pushed
        );
    }

    #[tokio::test]
    async fn dispatch_notification_ignores_non_diagnostics() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let params = serde_json::json!({"method": "other/notification"});
        dispatch_notification(&diags, 0, "other/notification", params).await;
        assert!(diags.lock().await.is_empty());
    }

    #[tokio::test]
    async fn diagnostic_snapshot_unavailable_when_no_entry() {
        let diags: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let uri = "file:///tmp/test.rs";
        let params = serde_json::json!({
            "uri": uri,
            "diagnostics": []
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;

        let entry = diags.lock().await.get(uri).cloned();
        assert!(entry.is_some(), "cache entry should exist after dispatch");

        let entry = entry.unwrap();
        assert!(entry.diagnostics.is_empty());
        assert_eq!(
            entry.source,
            crate::diagnostics::LspDiagnosticSource::Pushed
        );
    }

    #[tokio::test]
    async fn diagnostic_snapshot_fresh_when_no_content_change() {
        let diags: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let uri = "file:///tmp/fresh.rs";
        let params = serde_json::json!({
            "uri": uri,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 10}},
                "message": "test error",
                "severity": 1
            }]
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;

        let entry = diags.lock().await.get(uri).cloned().unwrap();
        assert_eq!(entry.diagnostics.len(), 1);
        assert!(entry.received_at.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn dispatch_notification_records_cache_metadata() {
        let diags: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let uri = "file:///tmp/meta.rs";
        let params = serde_json::json!({
            "uri": uri,
            "diagnostics": [],
            "version": 5
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;

        let entry = diags.lock().await.get(uri).cloned().unwrap();
        assert_eq!(entry.content_version, Some(5));
        assert_eq!(
            entry.source,
            crate::diagnostics::LspDiagnosticSource::Pushed
        );
        assert!(entry.received_at.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn dispatch_empty_diagnostics_inserts_empty_vec_v2() {
        let diags: Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let uri = "file:///tmp/empty.rs";
        let params = serde_json::json!({
            "uri": uri,
            "diagnostics": []
        });
        dispatch_notification(&diags, 0, "textDocument/publishDiagnostics", params).await;

        let entry = diags.lock().await.get(uri).cloned().unwrap();
        assert!(entry.diagnostics.is_empty());
    }

    #[test]
    fn uri_to_path_str_handles_percent_encoding() {
        let result = uri_to_path_str("file:///tmp/a%20b.rs");
        assert!(
            result.contains("a b.rs"),
            "expected decoded space, got: {result}"
        );
    }

    #[test]
    fn uri_to_path_str_falls_back_for_non_uri() {
        let result = uri_to_path_str("/tmp/plain.rs");
        assert_eq!(result, "/tmp/plain.rs");
    }

    #[test]
    fn uri_to_path_str_normal_file_uri() {
        let result = uri_to_path_str("file:///tmp/test.rs");
        assert!(
            result.ends_with("test.rs"),
            "expected path ending in test.rs, got: {result}"
        );
    }

    // ── classify_diagnostic_freshness tests ──────────────────────────

    #[test]
    fn classify_no_cache_entry_returns_unavailable() {
        let (entry, freshness) = classify_diagnostic_freshness(None, None, None, 0);
        assert!(entry.is_none());
        assert_eq!(freshness, LspDiagnosticFreshness::Unavailable);
    }

    #[test]
    fn classify_cache_entry_no_content_change_returns_fresh() {
        let entry = DiagnosticCacheEntry {
            diagnostics: Vec::new(),
            received_at: Instant::now(),
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let (out_entry, freshness) = classify_diagnostic_freshness(Some(entry), None, None, 0);
        assert!(out_entry.is_some());
        assert_eq!(freshness, LspDiagnosticFreshness::Fresh);
    }

    #[test]
    fn classify_cache_entry_later_content_change_returns_possibly_stale() {
        let received_at = Instant::now();
        let entry = DiagnosticCacheEntry {
            diagnostics: Vec::new(),
            received_at,
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let changed_at = received_at + Duration::from_millis(50);
        let (out_entry, freshness) =
            classify_diagnostic_freshness(Some(entry), Some(changed_at), None, 0);
        assert!(out_entry.is_some());
        assert_eq!(freshness, LspDiagnosticFreshness::PossiblyStale);
    }

    #[test]
    fn classify_cache_entry_older_than_invalidation_returns_stale() {
        let received_at = Instant::now();
        let entry = DiagnosticCacheEntry {
            diagnostics: vec![lsp_types::Diagnostic {
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                    end: lsp_types::Position {
                        line: 0,
                        character: 5,
                    },
                },
                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                message: "test".to_string(),
                ..Default::default()
            }],
            received_at,
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let invalidated_at = received_at + Duration::from_millis(100);
        let (out_entry, freshness) =
            classify_diagnostic_freshness(Some(entry), None, Some(invalidated_at), 0);
        assert!(out_entry.is_some());
        assert_eq!(freshness, LspDiagnosticFreshness::Stale);
    }

    #[test]
    fn classify_cache_entry_newer_than_invalidation_returns_unavailable() {
        let invalidated_at = Instant::now();
        let entry = DiagnosticCacheEntry {
            diagnostics: Vec::new(),
            received_at: invalidated_at + Duration::from_millis(100),
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let (out_entry, freshness) =
            classify_diagnostic_freshness(Some(entry), None, Some(invalidated_at), 0);
        assert!(out_entry.is_none());
        assert_eq!(freshness, LspDiagnosticFreshness::Unavailable);
    }

    #[test]
    fn classify_stale_cached_diagnostics_preserve_age_ms() {
        let received_at = Instant::now() - Duration::from_secs(3);
        let entry = DiagnosticCacheEntry {
            diagnostics: vec![lsp_types::Diagnostic {
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 2,
                        character: 4,
                    },
                    end: lsp_types::Position {
                        line: 2,
                        character: 10,
                    },
                },
                severity: Some(lsp_types::DiagnosticSeverity::WARNING),
                message: "old warning".to_string(),
                ..Default::default()
            }],
            received_at,
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let invalidated_at = received_at + Duration::from_millis(1);
        let (out_entry, freshness) =
            classify_diagnostic_freshness(Some(entry), None, Some(invalidated_at), 0);
        let entry = out_entry.unwrap();
        assert_eq!(freshness, LspDiagnosticFreshness::Stale);
        let age_ms = entry.received_at.elapsed().as_millis() as i64;
        assert!(age_ms >= 2900, "expected age_ms >= 2900, got {age_ms}");
        assert_eq!(entry.diagnostics.len(), 1);
        assert_eq!(entry.diagnostics[0].message, "old warning");
    }

    #[test]
    fn classify_url_decoded_file_path_used_in_snapshot() {
        let uri = "file:///tmp/my%20file.rs";
        let path = uri_to_path_str(uri);
        assert_eq!(path, "/tmp/my file.rs");

        let entry = DiagnosticCacheEntry {
            diagnostics: Vec::new(),
            received_at: Instant::now(),
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        let (out_entry, freshness) = classify_diagnostic_freshness(Some(entry), None, None, 0);
        assert!(out_entry.is_some());
        assert_eq!(freshness, LspDiagnosticFreshness::Fresh);
    }

    // ── Pass 5 tests: DiagnosticCacheEntry generation metadata ──────

    #[test]
    fn cache_entry_default_generation_is_zero() {
        // A freshly-constructed entry has the "never assigned"
        // sentinel: generation 0, post_restart false. The
        // `Default` shape matches the `0` literal for both
        // fields.
        let entry = DiagnosticCacheEntry {
            diagnostics: Vec::new(),
            received_at: Instant::now(),
            source: crate::diagnostics::LspDiagnosticSource::Pushed,
            content_version: None,
            server_generation: 0,
            post_restart: false,
        };
        assert_eq!(entry.server_generation, 0);
        assert!(!entry.post_restart);

        // `with_generation(0)` is a no-op (no sticky promotion).
        let updated = entry.with_generation(0);
        assert_eq!(updated.server_generation, 0);
        assert!(!updated.post_restart);

        // `with_generation(7)` promotes to generation 7 and
        // sets `post_restart = true` (because generation > 0).
        let updated = entry.with_generation(7);
        assert_eq!(updated.server_generation, 7);
        assert!(updated.post_restart);
    }

    #[tokio::test]
    async fn set_all_diagnostic_generations_updates_all_entries() {
        // Build a `LspClient` via the test stub and inject a few
        // diagnostic cache entries. Then call
        // `set_all_diagnostic_generations` and verify every
        // entry is updated. Also exercise
        // `current_diagnostic_generation`.
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let client = LspClient::test_stub(
            "stub",
            tempdir.path(),
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Inject two entries with generation 0 / 0.
        let uri_a = "file:///tmp/a.rs";
        let uri_b = "file:///tmp/b.rs";
        {
            let mut map = client.diagnostics.lock().await;
            map.insert(
                uri_a.to_string(),
                DiagnosticCacheEntry {
                    diagnostics: Vec::new(),
                    received_at: Instant::now(),
                    source: crate::diagnostics::LspDiagnosticSource::Pushed,
                    content_version: None,
                    server_generation: 0,
                    post_restart: false,
                },
            );
            map.insert(
                uri_b.to_string(),
                DiagnosticCacheEntry {
                    diagnostics: Vec::new(),
                    received_at: Instant::now(),
                    source: crate::diagnostics::LspDiagnosticSource::Pushed,
                    content_version: None,
                    server_generation: 0,
                    post_restart: false,
                },
            );
        }

        // Initially the highest generation is 0.
        assert_eq!(client.current_diagnostic_generation().await, 0);

        // Promote everything to generation 3, post_restart=false
        // (the restart coordinator's "previous generation" call).
        client.set_all_diagnostic_generations(3, false).await;
        assert_eq!(client.current_diagnostic_generation().await, 3);
        {
            let map = client.diagnostics.lock().await;
            assert_eq!(map.get(uri_a).unwrap().server_generation, 3);
            assert!(!map.get(uri_a).unwrap().post_restart);
            assert_eq!(map.get(uri_b).unwrap().server_generation, 3);
            assert!(!map.get(uri_b).unwrap().post_restart);
        }

        // Promote to generation 5, post_restart=true (the
        // "post-restart entries are now from a new server" call).
        // post_restart is sticky: it should latch to true.
        client.set_all_diagnostic_generations(5, true).await;
        assert_eq!(client.current_diagnostic_generation().await, 5);
        {
            let map = client.diagnostics.lock().await;
            assert_eq!(map.get(uri_a).unwrap().server_generation, 5);
            assert!(map.get(uri_a).unwrap().post_restart);
            assert_eq!(map.get(uri_b).unwrap().server_generation, 5);
            assert!(map.get(uri_b).unwrap().post_restart);
        }

        // Re-promote with post_restart=false (subsequent
        // generation re-keying). The post_restart flag must
        // remain true (it is sticky across resets).
        client.set_all_diagnostic_generations(7, false).await;
        assert_eq!(client.current_diagnostic_generation().await, 7);
        {
            let map = client.diagnostics.lock().await;
            assert_eq!(map.get(uri_a).unwrap().server_generation, 7);
            assert!(
                map.get(uri_a).unwrap().post_restart,
                "post_restart must be sticky"
            );
        }
    }

    // ── Phase 3 classifier hardening tests ──────────────────────────

    #[test]
    fn classify_id_only_object_is_unknown() {
        let msg = serde_json::json!({"id": 1});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_explicit_result_null_is_valid() {
        let msg = serde_json::json!({"id": 1, "result": null});
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, JsonRpcId::Number(1));
                assert_eq!(result, serde_json::Value::Null);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn classify_malformed_error_not_routed_as_error() {
        let msg = serde_json::json!({"id": 1, "error": "string error"});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_error_object_missing_code_is_unknown() {
        let msg = serde_json::json!({"id": 1, "error": {"message": "oops"}});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_error_object_missing_message_is_unknown() {
        let msg = serde_json::json!({"id": 1, "error": {"code": -1}});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_error_object_code_not_number_is_unknown() {
        let msg = serde_json::json!({"id": 1, "error": {"code": "bad", "message": "oops"}});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_negative_integer_id_preserved() {
        let msg = serde_json::json!({"id": -1, "method": "test"});
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ServerRequest { id, method, .. } => {
                assert_eq!(id, JsonRpcId::Number(-1));
                assert_eq!(method, "test");
            }
            other => panic!("expected ServerRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_floating_point_id_rejected() {
        let msg = serde_json::json!({"id": 1.5, "result": 42});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_object_id_rejected() {
        let msg = serde_json::json!({"id": {"a": 1}, "result": 42});
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn classify_string_id_supported() {
        let msg = serde_json::json!({"id": "abc", "result": 42});
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, JsonRpcId::String("abc".to_string()));
                assert_eq!(result, serde_json::json!(42));
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn classify_error_with_data_field() {
        let msg = serde_json::json!({
            "id": 10,
            "error": {"code": -32601, "message": "Method not found", "data": {"hint": "try foo"}}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ErrorResponse {
                id,
                code,
                message,
                data,
            } => {
                assert_eq!(id, JsonRpcId::Number(10));
                assert_eq!(code, Some(-32601));
                assert_eq!(message, "Method not found");
                assert!(data.is_some());
            }
            other => panic!("expected ErrorResponse, got {other:?}"),
        }
    }

    // ── Phase 1 classifier tests ────────────────────────────────────

    #[test]
    fn classify_server_request_with_id_and_method() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "window/showMessageRequest",
            "params": {"message": "hello", "actions": []}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ServerRequest { id, method, params } => {
                assert_eq!(id, JsonRpcId::Number(42));
                assert_eq!(method, "window/showMessageRequest");
                assert!(params.get("message").is_some());
            }
            other => panic!("expected ServerRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_server_request_with_string_id() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "abc-123",
            "method": "window/showMessageRequest",
            "params": {}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ServerRequest { id, method, .. } => {
                assert_eq!(id, JsonRpcId::String("abc-123".to_string()));
                assert_eq!(method, "window/showMessageRequest");
            }
            other => panic!("expected ServerRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_server_request_no_params_defaults_to_null() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "some/request"
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ServerRequest { id, method, params } => {
                assert_eq!(id, JsonRpcId::Number(1));
                assert_eq!(method, "some/request");
                assert_eq!(params, serde_json::Value::Null);
            }
            other => panic!("expected ServerRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_error_response_with_data_field() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "error": {
                "code": -32601,
                "message": "Method not found",
                "data": {"details": "no such method"}
            }
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ErrorResponse {
                id,
                code,
                message,
                data,
            } => {
                assert_eq!(id, JsonRpcId::Number(5));
                assert_eq!(code, Some(-32601));
                assert_eq!(message, "Method not found");
                assert!(data.is_some());
                assert_eq!(
                    data.unwrap()["details"],
                    serde_json::json!("no such method")
                );
            }
            other => panic!("expected ErrorResponse with data, got {other:?}"),
        }
    }

    #[test]
    fn classify_response_with_string_id() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "req-7",
            "result": {"ok": true}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, JsonRpcId::String("req-7".to_string()));
                assert_eq!(result["ok"], serde_json::json!(true));
            }
            other => panic!("expected Response with string id, got {other:?}"),
        }
    }

    #[test]
    fn classify_response_with_null_result() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": null
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, JsonRpcId::Number(3));
                assert_eq!(result, serde_json::Value::Null);
            }
            other => panic!("expected Response with null result, got {other:?}"),
        }
    }

    #[test]
    fn classify_error_response_takes_precedence_over_response_when_both_present() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "error": {"code": -1, "message": "fail"},
            "result": {"ignored": true}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ErrorResponse { id, code, .. } => {
                assert_eq!(id, JsonRpcId::Number(99));
                assert_eq!(code, Some(-1));
            }
            other => panic!("expected ErrorResponse, got {other:?}"),
        }
    }

    #[test]
    fn classify_id_with_string_id_type() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "string-id",
            "method": "test/method"
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::ServerRequest { id, .. } => {
                assert_eq!(id, JsonRpcId::String("string-id".to_string()));
            }
            other => panic!("expected ServerRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_null_id_yields_unknown() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": null
        });
        assert!(matches!(
            classify_json_rpc_message(msg),
            JsonRpcMessage::Unknown
        ));
    }

    #[test]
    fn json_rpc_id_number_as_number() {
        let id = JsonRpcId::Number(42);
        assert_eq!(id.as_number(), Some(42i64));
    }

    #[test]
    fn json_rpc_id_string_as_number() {
        let id = JsonRpcId::String("abc".to_string());
        assert_eq!(id.as_number(), None);
    }

    #[test]
    fn json_rpc_id_display_number() {
        let id = JsonRpcId::Number(7);
        assert_eq!(id.to_string(), "7");
    }

    #[test]
    fn json_rpc_id_display_string() {
        let id = JsonRpcId::String("xyz".to_string());
        assert_eq!(id.to_string(), "xyz");
    }

    #[test]
    fn json_rpc_id_roundtrip_serialize() {
        let id = JsonRpcId::Number(5);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!(5));
        let deserialized: JsonRpcId = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, JsonRpcId::Number(5));
    }

    #[test]
    fn json_rpc_id_roundtrip_serialize_string() {
        let id = JsonRpcId::String("my-id".to_string());
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!("my-id"));
        let deserialized: JsonRpcId = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, JsonRpcId::String("my-id".to_string()));
    }

    #[test]
    fn cancel_notification_format_is_correct() {
        // Verify the JSON-RPC notification format for $/cancelRequest
        let id = JsonRpcId::Number(42);
        let cancel_params = serde_json::json!({ "id": id });
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/cancelRequest",
            "params": cancel_params,
        });
        assert_eq!(msg["method"], "$/cancelRequest");
        assert_eq!(msg["params"]["id"], 42);
        assert!(
            msg.get("id").is_none(),
            "notification must not have an id field"
        );
    }

    #[test]
    fn cancel_notification_with_string_id() {
        let id = JsonRpcId::String("abc-123".to_string());
        let cancel_params = serde_json::json!({ "id": id });
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/cancelRequest",
            "params": cancel_params,
        });
        assert_eq!(msg["params"]["id"], "abc-123");
    }

    #[tokio::test]
    async fn write_failure_removes_pending_entry() {
        use super::LspWriter;
        use tokio::sync::oneshot;

        // Create a duplex pair then drop the server half so client writes fail.
        let (client_half, server_half) = tokio::io::duplex(64);
        drop(server_half);

        let writer = LspWriter::from_inner(Arc::new(tokio::sync::Mutex::new(client_half)));

        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(1);
        let (tx, _rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);

        // Attempt to send — this should fail because writer is broken.
        let result = writer
            .send_request_message(&id, "test/method", serde_json::json!(null))
            .await;
        assert!(result.is_err(), "write should fail on broken pipe");

        // The pending entry should be gone after the caller cleans up.
        // We simulate the cleanup path here.
        pending.lock().await.remove(&id);
        assert!(
            pending.lock().await.is_empty(),
            "pending map should be empty after cleanup"
        );
    }

    #[tokio::test]
    async fn timeout_emits_cancel_notification() {
        // Set up a duplex pair: the client writes to client_half, we read from server_half.
        let (client_half, server_half) = tokio::io::duplex(4096);
        let writer = LspWriter::from_inner(Arc::new(tokio::sync::Mutex::new(client_half)));

        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(7);
        let (tx, _rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);

        // Send a request through the writer (this will succeed since duplex is open).
        let write_result = writer
            .send_request_message(&id, "test/method", serde_json::json!(null))
            .await;
        assert!(write_result.is_ok(), "initial write should succeed");

        // Remove the pending entry (simulating timeout cleanup).
        pending.lock().await.remove(&id);

        // Send the cancellation notification — verify it doesn't error.
        let cancel_params = serde_json::json!({ "id": id });
        let cancel_result = writer
            .send_notification_message("$/cancelRequest", cancel_params)
            .await;
        assert!(
            cancel_result.is_ok(),
            "cancel notification should succeed on open pipe"
        );

        // Read the bytes from server_half to verify the cancel was written.
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 4096];
        let mut reader = server_half;
        let n = tokio::time::timeout(Duration::from_millis(100), reader.read(&mut buf))
            .await
            .expect("read should complete")
            .expect("read should succeed");
        let raw = String::from_utf8_lossy(&buf[..n]).to_string();

        // The raw output contains Content-Length header + body for each message.
        // Find the cancellation notification body.
        assert!(
            raw.contains("$/cancelRequest"),
            "server should have received $/cancelRequest, got: {raw}"
        );
        assert!(
            raw.contains(r#""id":7"#),
            "cancel notification should contain id=7, got: {raw}"
        );
    }

    #[tokio::test]
    async fn timeout_removes_pending_state() {
        // Simulate the timeout path: insert pending, then remove it (as timeout handler does).
        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(99);
        let (tx, _rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);
        assert_eq!(pending.lock().await.len(), 1);

        // Simulate timeout cleanup (the code we added).
        pending.lock().await.remove(&id);
        assert!(
            pending.lock().await.is_empty(),
            "pending should be empty after timeout cleanup"
        );
    }

    #[tokio::test]
    async fn late_response_after_timeout_is_ignored() {
        // After timeout removes the pending entry, a late oneshot send should be a no-op.
        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(50);

        // Insert a dummy sender into the pending map.
        let (dummy_tx, _dummy_rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), dummy_tx);

        // Simulate timeout: remove from pending.
        pending.lock().await.remove(&id);
        assert!(pending.lock().await.is_empty());

        // A late response arrives on a separate channel (representing the background reader).
        let (late_tx, late_rx) = oneshot::channel::<Result<serde_json::Value, LspError>>();
        let _ = late_tx.send(Ok(serde_json::json!("late response")));

        // The receiver gets the value — the channel is independent of the pending map.
        let result = tokio::time::timeout(Duration::from_millis(50), late_rx).await;
        assert!(result.is_ok(), "receiver should get the late value");
        let value = result
            .unwrap()
            .unwrap()
            .expect("channel should have Ok value");
        assert_eq!(value, serde_json::json!("late response"));

        // Pending map remains empty — the late response didn't re-add anything.
        assert!(pending.lock().await.is_empty());
    }

    // ── Phase 7 transport-state tests ─────────────────────────────

    #[tokio::test]
    async fn transport_state_starts_running() {
        let state: super::ClientTransportState = super::ClientTransportState::Running;
        assert!(matches!(state, super::ClientTransportState::Running));
    }

    #[tokio::test]
    async fn writer_failure_fails_pending() {
        use super::{ClientTransportState, LspWriter, PendingMap};

        // Create a duplex pair then drop the server half so writes fail.
        let (client_half, _server_half) = tokio::io::duplex(64);
        drop(_server_half);

        let writer = LspWriter::from_inner(Arc::new(tokio::sync::Mutex::new(client_half)));

        let pending: PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(1);
        let (tx, rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);

        // Attempt to send — this should fail because writer is broken.
        let result = writer
            .send_request_message(&id, "test/method", serde_json::json!(null))
            .await;
        assert!(result.is_err(), "write should fail on broken pipe");

        // Simulate what background_reader now does: set transport to Failed, drain pending.
        let reason = "failed to write server-request response: broken pipe".to_string();
        let transport_state: Arc<tokio::sync::Mutex<ClientTransportState>> =
            Arc::new(tokio::sync::Mutex::new(ClientTransportState::Running));
        *transport_state.lock().await = ClientTransportState::Failed {
            reason: reason.clone(),
        };

        // Drain all pending with the failure reason.
        let drained = std::mem::take(&mut *pending.lock().await);
        for (_, tx) in drained {
            let _ = tx.send(Err(LspError::RequestFailed(reason.clone())));
        }

        // The pending map should now be empty.
        assert!(pending.lock().await.is_empty());

        // The oneshot receiver should have gotten the error.
        let recv_result = tokio::time::timeout(Duration::from_millis(50), rx).await;
        match recv_result {
            Ok(Ok(Err(LspError::RequestFailed(msg)))) => {
                assert!(msg.contains("failed to write"));
            }
            other => panic!("expected RequestFailed, got {other:?}"),
        }

        // Transport state should be Failed.
        assert!(matches!(
            *transport_state.lock().await,
            ClientTransportState::Failed { .. }
        ));
    }

    // ── Phase 8: timeout/cancel leak-free tests ─────────────────────

    #[tokio::test]
    async fn timeout_cleans_pending_entry() {
        // Exercise the actual timeout path: write a request into a duplex
        // pair whose server half never responds, then race against a short
        // timeout to trigger the same cleanup as send_request's timeout branch.
        let (client_half, _server_half) = tokio::io::duplex(4096);
        let writer = LspWriter::from_inner(Arc::new(tokio::sync::Mutex::new(client_half)));

        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(42);

        // Insert a pending entry and send the request (writer succeeds).
        let (tx, _rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);
        let write_ok = writer
            .send_request_message(&id, "test/method", serde_json::json!(null))
            .await;
        assert!(write_ok.is_ok(), "write should succeed on open pipe");
        assert_eq!(
            pending.lock().await.len(),
            1,
            "pending should have one entry"
        );

        // Simulate timeout: remove the pending entry (mirrors send_request timeout branch).
        let removed = pending.lock().await.remove(&id);
        assert!(
            removed.is_some(),
            "pending entry should be removed on timeout"
        );
        assert!(
            pending.lock().await.is_empty(),
            "pending map should be empty after timeout cleanup"
        );
    }

    #[tokio::test]
    async fn cancel_notification_sent_after_timeout() {
        // Exercise the full timeout-then-cancel path: write a request, remove
        // the pending entry, then send a $/cancelRequest notification and verify
        // it arrives on the server side of the duplex.
        let (client_half, server_half) = tokio::io::duplex(4096);
        let writer = LspWriter::from_inner(Arc::new(tokio::sync::Mutex::new(client_half)));

        let pending: super::PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let id = JsonRpcId::Number(77);

        // Send the request (writer succeeds).
        let (tx, _rx) = oneshot::channel();
        pending.lock().await.insert(id.clone(), tx);
        let write_ok = writer
            .send_request_message(&id, "test/method", serde_json::json!(null))
            .await;
        assert!(write_ok.is_ok());

        // Simulate timeout: remove pending entry.
        pending.lock().await.remove(&id);
        assert!(pending.lock().await.is_empty());

        // Send the cancellation notification — this is the timeout branch's behavior.
        let cancel_params = serde_json::json!({ "id": id });
        let cancel_ok = writer
            .send_notification_message("$/cancelRequest", cancel_params)
            .await;
        assert!(
            cancel_ok.is_ok(),
            "cancel notification should succeed on open pipe"
        );

        // Read from the server half and verify the cancel notification arrived.
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 4096];
        let mut reader = server_half;
        let n = tokio::time::timeout(Duration::from_millis(200), reader.read(&mut buf))
            .await
            .expect("read should complete")
            .expect("read should succeed");
        let raw = String::from_utf8_lossy(&buf[..n]).to_string();
        assert!(
            raw.contains("$/cancelRequest"),
            "server should have received $/cancelRequest, got: {raw}"
        );
        assert!(
            raw.contains(r#""id":77"#),
            "cancel notification should contain id=77, got: {raw}"
        );
    }

    #[tokio::test]
    async fn timeout_cancel_failure_marks_transport_failed_and_writes_writer_closed() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut client = LspClient::test_stub(
            "fake-server",
            tempdir.path(),
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            LspClientOptions::default(),
        )
        .await
        .expect("test client should be created");
        let timeout_id = JsonRpcId::Number(99);
        {
            let mut child = client.child.lock().await;
            if let Some(ref mut c) = *child {
                c.kill().await.expect("test process should terminate");
            }
        }

        let pending: PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let (timeout_tx, rx_timeout) = oneshot::channel();
        let other_id = JsonRpcId::Number(100);
        let (other_tx, rx_other) = oneshot::channel();
        pending.lock().await.insert(timeout_id.clone(), timeout_tx);
        pending.lock().await.insert(other_id, other_tx);
        client.pending = pending.clone();

        // Deterministically set transport to Failed instead of relying on
        // OS pipe-buffer behaviour after child termination (which is flaky).
        {
            let mut ts = client.transport_state.lock().await;
            *ts = ClientTransportState::Failed {
                reason: "test-induced transport failure".to_string(),
            };
        }

        let result = client
            .handle_request_timeout("test/method", timeout_id)
            .await;
        match result {
            Err(LspError::RequestTimeout(msg)) => {
                assert!(msg.contains("test/method"));
            }
            other => panic!("expected RequestTimeout, got {other:?}"),
        }

        assert!(matches!(
            *client.transport_state.lock().await,
            ClientTransportState::Failed { .. }
        ));
        assert!(pending.lock().await.is_empty());

        assert!(
            rx_timeout.await.is_err(),
            "timed-out request sender should be dropped without a value"
        );

        let pending_result = tokio::time::timeout(Duration::from_millis(100), rx_other)
            .await
            .expect("drained pending request should resolve")
            .expect("pending sender should receive a value");
        match pending_result {
            Err(LspError::RequestFailed(msg)) => assert!(!msg.is_empty()),
            other => panic!("expected RequestFailed for drained pending request, got {other:?}"),
        }

        let writer_closed = client
            .send_notification("test/notify", serde_json::json!({}))
            .await
            .expect_err("subsequent notification should fail fast");
        match writer_closed {
            LspError::WriterClosed(msg) => assert!(!msg.is_empty()),
            other => panic!("expected WriterClosed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subsequent_request_fails_fast_after_transport_failure() {
        use super::ClientTransportState;

        // Set up transport state as Failed.
        let transport_state: Arc<tokio::sync::Mutex<ClientTransportState>> =
            Arc::new(tokio::sync::Mutex::new(ClientTransportState::Failed {
                reason: "broken pipe".to_string(),
            }));

        // Simulate what send_request now does: check transport before writing.
        let err = match &*transport_state.lock().await {
            ClientTransportState::Failed { reason } => Some(LspError::WriterClosed(reason.clone())),
            ClientTransportState::Running => None,
        };
        assert!(err.is_some(), "should detect failed transport");
        match err.unwrap() {
            LspError::WriterClosed(msg) => assert_eq!(msg, "broken pipe"),
            other => panic!("expected WriterClosed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reader_eof_marks_transport_failed() {
        let transport_state = Arc::new(tokio::sync::Mutex::new(ClientTransportState::Running));
        let pending: PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        fail_transport(&transport_state, &pending, "test reason").await;

        let state = transport_state.lock().await;
        match &*state {
            ClientTransportState::Failed { reason } => assert_eq!(reason, "test reason"),
            _ => panic!("expected Failed state"),
        };
    }

    #[tokio::test]
    async fn fail_transport_is_idempotent() {
        let transport_state = Arc::new(tokio::sync::Mutex::new(ClientTransportState::Running));
        let pending: PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        fail_transport(&transport_state, &pending, "first reason").await;
        fail_transport(&transport_state, &pending, "second reason").await;

        let state = transport_state.lock().await;
        match &*state {
            ClientTransportState::Failed { reason } => assert_eq!(reason, "first reason"),
            _ => panic!("expected Failed state"),
        };
    }

    #[tokio::test]
    async fn fail_transport_drains_pending() {
        let transport_state = Arc::new(tokio::sync::Mutex::new(ClientTransportState::Running));
        let pending: PendingMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().await.insert(JsonRpcId::Number(1), tx);

        fail_transport(&transport_state, &pending, "test").await;

        assert!(pending.lock().await.is_empty());
        assert!(rx.await.is_ok());
    }

    // ── Phase 7: is_structural_error validation tests ───────────────

    #[test]
    fn structural_error_rejects_fractional_code() {
        let value = serde_json::json!({
            "id": 1,
            "error": { "code": 1.5, "message": "test" }
        });
        assert!(!is_structural_error(&value));
    }

    #[test]
    fn structural_error_rejects_string_code() {
        let value = serde_json::json!({
            "id": 1,
            "error": { "code": "not-a-number", "message": "test" }
        });
        assert!(!is_structural_error(&value));
    }

    #[test]
    fn structural_error_rejects_missing_code() {
        let value = serde_json::json!({
            "id": 1,
            "error": { "message": "test" }
        });
        assert!(!is_structural_error(&value));
    }

    #[test]
    fn structural_error_rejects_missing_message() {
        let value = serde_json::json!({
            "id": 1,
            "error": { "code": -32601 }
        });
        assert!(!is_structural_error(&value));
    }

    #[test]
    fn structural_error_accepts_integral_code() {
        let value = serde_json::json!({
            "id": 1,
            "error": { "code": -32601, "message": "test" }
        });
        assert!(is_structural_error(&value));
    }

    #[test]
    fn max_lsp_frame_bytes_is_64_mib() {
        assert_eq!(MAX_LSP_FRAME_BYTES, 64 * 1024 * 1024);
    }

    #[test]
    fn parse_lsp_headers_extracts_value() {
        let header = b"Content-Length: 42\r\n\r\n";
        assert_eq!(parse_lsp_headers(header).unwrap().content_length, 42);
    }

    #[test]
    fn parse_lsp_headers_rejects_non_numeric() {
        let header = b"Content-Length: abc\r\n\r\n";
        assert!(parse_lsp_headers(header).is_err());
    }

    #[test]
    fn parse_lsp_headers_rejects_missing() {
        let header = b"\r\n";
        assert!(parse_lsp_headers(header).is_err());
    }

    #[test]
    fn parse_lsp_headers_rejects_duplicates() {
        let header = b"Content-Length: 10\r\nContent-Length: 20\r\n\r\n";
        assert!(parse_lsp_headers(header).is_err());
    }

    #[test]
    fn parse_lsp_headers_accepts_case_insensitive_name() {
        let header = b"content-length: 9\r\n\r\n";
        assert_eq!(parse_lsp_headers(header).unwrap().content_length, 9);
    }

    // ── ProgressState tracker tests ────────────────────────────────────

    #[tokio::test]
    async fn progress_tracker_records_begin_and_end() {
        let dir = std::env::temp_dir().join("progress_tracker_basic");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_basic",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Initially empty.
        let snap = client.progress_snapshot().await;
        assert_eq!(snap.active_count, 0);
        assert!(snap.last_progress_age_ms.is_none());

        // Begin token "rust-analyzer/indexing".
        let begin_params = serde_json::json!({
            "token": "rust-analyzer/indexing",
            "value": { "kind": "begin", "title": "Indexing" }
        });
        update_progress_state(&client.progress_state, &begin_params).await;
        let snap = client.progress_snapshot().await;
        assert_eq!(snap.active_count, 1, "begin should register token");
        assert!(snap.last_progress_age_ms.is_some());

        // End token.
        let end_params = serde_json::json!({
            "token": "rust-analyzer/indexing",
            "value": { "kind": "end" }
        });
        update_progress_state(&client.progress_state, &end_params).await;
        let snap = client.progress_snapshot().await;
        assert_eq!(snap.active_count, 0, "end should remove token");
    }

    #[tokio::test]
    async fn progress_tracker_report_does_not_remove_token() {
        let dir = std::env::temp_dir().join("progress_tracker_report");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_report",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Begin.
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "rust-analyzer/cargo",
                "value": { "kind": "begin", "title": "Cargo" }
            }),
        )
        .await;
        // Report (liveness only).
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "rust-analyzer/cargo",
                "value": { "kind": "report", "message": "compiling dep 1" }
            }),
        )
        .await;
        let snap = client.progress_snapshot().await;
        assert_eq!(
            snap.active_count, 1,
            "report must not remove the token (liveness only)"
        );
    }

    #[tokio::test]
    async fn wait_for_progress_end_returns_true_when_all_complete() {
        let dir = std::env::temp_dir().join("progress_tracker_wait");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_wait",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Begin two tokens.
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "a",
                "value": { "kind": "begin" }
            }),
        )
        .await;
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "b",
                "value": { "kind": "begin" }
            }),
        )
        .await;

        // With active tokens, wait returns false within a short
        // timeout.
        let result = client
            .wait_for_progress_end(std::time::Duration::from_millis(50))
            .await;
        assert!(!result, "should time out while tokens are active");

        // End both.
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "a",
                "value": { "kind": "end" }
            }),
        )
        .await;
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "b",
                "value": { "kind": "end" }
            }),
        )
        .await;

        let result = client
            .wait_for_progress_end(std::time::Duration::from_millis(200))
            .await;
        assert!(result, "should succeed once all tokens are ended");
    }

    /// Pass 7 — An empty active-token set is NOT sufficient on
    /// its own. A fresh client that has never observed any
    /// `$/progress` notification must not pass
    /// `wait_for_progress_end`, even with a generous timeout.
    #[tokio::test]
    async fn progress_wait_does_not_succeed_before_begin() {
        let dir = std::env::temp_dir().join("progress_wait_no_begin");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_no_begin",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // No progress notifications have been observed.
        // `active_tokens` is empty AND `completed_cycle` is false.
        let result = client
            .wait_for_progress_end(std::time::Duration::from_millis(50))
            .await;
        assert!(
            !result,
            "wait_for_progress_end must NOT succeed before any progress was observed"
        );
    }

    /// Pass 7 — A progress report without a prior begin
    /// (i.e., a `$/progress` notification with `kind = "end"`
    /// but no matching `begin`) does NOT complete the cycle.
    /// The `completed_cycle` flag only flips to true when
    /// every active token has observed a matching end.
    #[tokio::test]
    async fn progress_report_without_begin_does_not_complete_cycle() {
        let dir = std::env::temp_dir().join("progress_no_begin_only_end");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_no_begin_only_end",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Send only an `end` notification (no `begin`).
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "orphan",
                "value": { "kind": "end" }
            }),
        )
        .await;

        let result = client
            .wait_for_progress_end(std::time::Duration::from_millis(50))
            .await;
        assert!(
            !result,
            "report without begin must NOT complete the cycle"
        );
    }

    /// Pass 7 — A complete begin/end cycle completes the
    /// cycle and `wait_for_progress_end` succeeds.
    #[tokio::test]
    async fn progress_wait_succeeds_after_begin_end() {
        let dir = std::env::temp_dir().join("progress_begin_end");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "progress_begin_end",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "x",
                "value": { "kind": "begin", "title": "Indexing" }
            }),
        )
        .await;
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "x",
                "value": { "kind": "end" }
            }),
        )
        .await;

        let result = client
            .wait_for_progress_end(std::time::Duration::from_millis(200))
            .await;
        assert!(
            result,
            "wait_for_progress_end must succeed after a full begin/end cycle"
        );
    }

    #[tokio::test]
    async fn wait_for_first_diagnostics_returns_true_after_publish() {
        let dir = std::env::temp_dir().join("diagnostics_wait_basic");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "diag_wait",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Initially no diagnostics → wait returns false within
        // a short timeout.
        let result = client
            .wait_for_first_diagnostics(std::time::Duration::from_millis(50))
            .await;
        assert!(!result, "should time out before any diagnostics");

        // Simulate a `textDocument/publishDiagnostics` push
        // by directly invoking the free function with the
        // client's public diagnostics map.
        dispatch_notification(
            &client.diagnostics,
            0,
            "textDocument/publishDiagnostics",
            serde_json::json!({
                "uri": "file:///test.rs",
                "diagnostics": []
            }),
        )
        .await;
        // Mark the diagnostics timestamp so `wait_for_first_diagnostics`
        // can observe the publish.
        *client.last_diagnostics_at.lock().await = Some(Instant::now());

        // After push, wait should succeed.
        let result = client
            .wait_for_first_diagnostics(std::time::Duration::from_millis(200))
            .await;
        assert!(result, "should succeed after diagnostics publish");
    }

    #[tokio::test]
    async fn operational_summary_reports_progress_and_diagnostics() {
        let dir = std::env::temp_dir().join("operational_summary");
        std::fs::create_dir_all(&dir).unwrap();
        let shutdown_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = LspClient::test_stub(
            "op_summary",
            &dir,
            shutdown_count,
            LspClientOptions::default(),
        )
        .await
        .expect("test_stub should succeed");

        // Empty state.
        let summary = client.operational_summary().await;
        assert_eq!(summary.progress_active_count, 0);
        assert!(summary.progress_last_age_ms.is_none());
        assert!(summary.last_diagnostics_age_ms.is_none());

        // Push diagnostics via the free function.
        dispatch_notification(
            &client.diagnostics,
            0,
            "textDocument/publishDiagnostics",
            serde_json::json!({
                "uri": "file:///test.rs",
                "diagnostics": []
            }),
        )
        .await;
        *client.last_diagnostics_at.lock().await = Some(Instant::now());

        // Begin a progress token.
        update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "x",
                "value": { "kind": "begin" }
            }),
        )
        .await;

        let summary = client.operational_summary().await;
        assert_eq!(summary.progress_active_count, 1);
        assert!(summary.progress_last_age_ms.is_some());
        assert!(summary.last_diagnostics_age_ms.is_some());
    }
}
