//! LSP Client - Language Server Protocol implementation.
//!
//! Manages LSP server lifecycle and communication:
//! - Spawns language servers (rust-analyzer, pyright, etc.)
//! - Handles JSON-RPC message protocol over stdin/stdout
//! - Tracks open files and diagnostics
//! - Supports concurrent requests with atomic ID counter
//!
//! Key types:
//! - `LspClient` - main client managing server process
//! - `LspProcess` - spawned server process with streams
//! - `DiagnosticEntry` - file URI + diagnostic data

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use lsp_types::*;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};
use url::Url;

use super::launch::{self, LspProcess};
use super::server::LspServerDef;
use crate::error::LspError;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, LspError>>>>>;

/// Classified JSON-RPC message from the server.
pub enum JsonRpcMessage {
    Response {
        id: u64,
        result: serde_json::Value,
    },
    ErrorResponse {
        id: u64,
        code: Option<i64>,
        message: String,
    },
    Notification {
        method: String,
        params: serde_json::Value,
    },
    Unknown,
}

/// Classify a raw JSON-RPC value into its semantic type.
pub fn classify_json_rpc_message(value: serde_json::Value) -> JsonRpcMessage {
    let id = value.get("id").and_then(|v| v.as_u64());
    let method = value.get("method").and_then(|v| v.as_str());

    match (id, method) {
        (Some(id), _) if value.get("error").is_some() => {
            let code = value
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_i64());
            let message = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string();
            JsonRpcMessage::ErrorResponse { id, code, message }
        }
        (Some(id), _) => {
            let result = value
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
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

/// Dispatch a notification by method. Currently handles diagnostics.
pub async fn dispatch_notification(
    diagnostics: &tokio::sync::Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>,
    method: &str,
    params: serde_json::Value,
) {
    if method == "textDocument/publishDiagnostics" {
        if let Some(uri) = params.get("uri").and_then(|v| v.as_str()) {
            if let Some(diags_value) = params.get("diagnostics") {
                match serde_json::from_value::<Vec<lsp_types::Diagnostic>>(diags_value.clone()) {
                    Ok(diags) => {
                        let count = diags.len();
                        diagnostics.lock().await.insert(uri.to_string(), diags);
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

pub fn url_to_uri(url: &Url) -> Result<Uri, LspError> {
    Uri::from_str(url.as_str()).map_err(|e| LspError::RequestFailed(format!("invalid URL: {e}")))
}

pub struct DiagnosticEntry {
    pub uri: String,
    pub diagnostic: lsp_types::Diagnostic,
}

pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicU64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    /// Tracks when each file was last opened or changed, for diagnostics warm-up detection.
    pub last_opened_at: Mutex<HashMap<String, Instant>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub pending: PendingMap,
    _reader_task: tokio::task::JoinHandle<()>,
}

impl LspClient {
    pub async fn new(
        server: &LspServerDef,
        binary: &Path,
        root: &Path,
        env: &[(String, String)],
    ) -> Result<Self, LspError> {
        let args: Vec<&str> = server.args.iter().map(|s| &**s).collect();
        let binary_str = binary.to_str().ok_or_else(|| {
            LspError::LaunchFailed(format!(
                "binary path is not valid UTF-8: {}",
                binary.display()
            ))
        })?;
        let mut process = launch::spawn_server(binary_str, &args, env, Some(root)).await?;

        if let Some(stderr) = process.stderr.take() {
            launch::spawn_stderr_drain(server.id, stderr.into_inner());
        }

        // Split process: stdout goes to background reader, stdin stays in LspClient.
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| LspError::LaunchFailed("stdout not available".to_string()))?;

        let diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        // Spawn background stdout reader.
        let reader_diagnostics = diagnostics.clone();
        let reader_pending = pending.clone();
        let server_id = server.id.to_string();
        let reader_task = tokio::spawn(async move {
            Self::background_reader(stdout, reader_diagnostics, reader_pending, server_id).await;
        });

        let client = Self {
            server_id: server.id.to_string(),
            root: root.to_path_buf(),
            process: tokio::sync::Mutex::new(process),
            request_id: AtomicU64::new(0),
            capabilities: Mutex::new(None),
            opened_files: Mutex::new(HashMap::new()),
            last_opened_at: Mutex::new(HashMap::new()),
            diagnostics,
            pending,
            _reader_task: reader_task,
        };

        Ok(client)
    }

    /// Background task that reads framed JSON-RPC messages from stdout
    /// and routes them to pending request senders or notification handlers.
    async fn background_reader(
        mut stdout: tokio::process::ChildStdout,
        diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
        pending: PendingMap,
        server_id: String,
    ) {
        loop {
            // Read Content-Length framed message.
            let resp_str = match read_framed_message(&mut stdout).await {
                Ok(s) => s,
                Err(e) => {
                    debug!(server = %server_id, error = %e, "stdout reader exiting");
                    break;
                }
            };

            let value: serde_json::Value = match serde_json::from_str(&resp_str) {
                Ok(v) => v,
                Err(e) => {
                    warn!(server = %server_id, error = %e, "failed to parse JSON-RPC message");
                    continue;
                }
            };

            match classify_json_rpc_message(value) {
                JsonRpcMessage::Response { id, result } => {
                    let sender = pending.lock().await.remove(&id);
                    if let Some(tx) = sender {
                        let _ = tx.send(Ok(result));
                    }
                }
                JsonRpcMessage::ErrorResponse { id, code, message } => {
                    let sender = pending.lock().await.remove(&id);
                    if let Some(tx) = sender {
                        let code_str = code.map(|c| c.to_string()).unwrap_or_default();
                        let _ = tx.send(Err(LspError::RequestFailed(format!(
                            "LSP error {code_str}: {message}"
                        ))));
                    }
                }
                JsonRpcMessage::Notification { method, params } => {
                    dispatch_notification(&diagnostics, &method, params).await;
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
        self.last_opened_at
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
        self.last_opened_at
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
            .await
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
        self.send_request("shutdown", serde_json::json!(null))
            .await?;
        self.send_notification("exit", serde_json::json!({})).await
    }

    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspError> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let msg_str = serde_json::to_string(&msg)?;

        // Register pending request before writing to stdin.
        let (tx, rx) = oneshot::channel();
        {
            self.pending.lock().await.insert(id, tx);
        }

        // Write the request.
        {
            let mut proc = self.process.lock().await;
            launch::send_request(&mut proc, &msg_str).await?;
        }

        // Wait for the background reader to deliver the response.
        let result = tokio::time::timeout(Self::REQUEST_TIMEOUT, rx).await;

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
                // Timeout — clean up pending entry; background reader will ignore late response.
                self.pending.lock().await.remove(&id);
                Err(LspError::RequestTimeout(format!(
                    "LSP request '{}' timed out after {:?}",
                    method,
                    Self::REQUEST_TIMEOUT
                )))
            }
        }
    }

    pub async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), LspError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let msg_str = serde_json::to_string(&msg)?;
        let mut proc = self.process.lock().await;
        launch::send_request(&mut proc, &msg_str).await
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
            .cloned()
            .unwrap_or_default()
    }

    pub async fn get_all_diagnostics(&self) -> HashMap<String, Vec<lsp_types::Diagnostic>> {
        self.diagnostics.lock().await.clone()
    }

    /// Returns true if the file was opened or changed very recently and
    /// no diagnostics have been received yet for it.
    pub async fn diagnostics_may_still_be_warming(&self, uri: &str) -> bool {
        let last = self.last_opened_at.lock().await;
        if let Some(instant) = last.get(uri) {
            let elapsed = instant.elapsed();
            // If opened/changed within the last 2 seconds, diagnostics may still be warming.
            if elapsed < std::time::Duration::from_secs(2) {
                let diags = self.diagnostics.lock().await;
                return !diags.contains_key(uri) || diags.get(uri).map_or(true, |d| d.is_empty());
            }
        }
        false
    }

    pub async fn process_notification(&self, notification: &str) {
        if let Some(diags) = parse_publish_diagnostics(notification) {
            let uri = diags.0;
            let diagnostics = diags.1;
            let count = diagnostics.len();
            self.diagnostics
                .lock()
                .await
                .insert(uri.clone(), diagnostics);
            debug!(uri, count, "received diagnostics");
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

/// Read a single Content-Length framed message from a stdout stream.
async fn read_framed_message(stdout: &mut tokio::process::ChildStdout) -> Result<String, LspError> {
    use tokio::io::AsyncReadExt;
    let mut header_buf = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        stdout
            .read_exact(&mut byte)
            .await
            .map_err(|e| LspError::RequestFailed(format!("read header failed: {}", e)))?;
        header_buf.push(byte[0]);

        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let header_str = String::from_utf8_lossy(&header_buf);
    let content_length = parse_content_length(&header_str)
        .ok_or_else(|| LspError::RequestFailed("missing Content-Length header".to_string()))?;

    let mut body = vec![0u8; content_length];
    stdout
        .read_exact(&mut body)
        .await
        .map_err(|e| LspError::RequestFailed(format!("read body failed: {}", e)))?;

    String::from_utf8(body)
        .map_err(|e| LspError::RequestFailed(format!("invalid utf8 in response: {}", e)))
}

fn parse_content_length(header: &str) -> Option<usize> {
    for line in header.lines() {
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            return val.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_response_message() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"capabilities": {}}
        });
        match classify_json_rpc_message(msg) {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, 1);
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
            JsonRpcMessage::ErrorResponse { id, code, message } => {
                assert_eq!(id, 2);
                assert_eq!(code, Some(-32600));
                assert_eq!(message, "Invalid Request");
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
        dispatch_notification(&diags, "textDocument/publishDiagnostics", params).await;
        let map = diags.lock().await;
        assert!(map.contains_key("file:///test.rs"));
        assert_eq!(map["file:///test.rs"].len(), 1);
        assert_eq!(map["file:///test.rs"][0].message, "test error");
    }

    #[tokio::test]
    async fn dispatch_unknown_notification_ignored() {
        let diags = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        dispatch_notification(&diags, "textDocument/completion", serde_json::json!({})).await;
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
}
