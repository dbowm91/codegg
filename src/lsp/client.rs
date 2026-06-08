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

use lsp_types::*;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn};
use url::Url;

use super::launch::{self, LspProcess};
use super::server::LspServerDef;
use crate::error::LspError;

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
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub notif_tx: mpsc::UnboundedSender<String>,
    pub notif_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
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

        let stderr_output = launch::drain_stderr(&mut process).await;
        if !stderr_output.is_empty() {
            info!(server = server.id, stderr = %stderr_output, "LSP server stderr");
        }

        let (tx, rx) = mpsc::unbounded_channel();

        let client = Self {
            server_id: server.id.to_string(),
            root: root.to_path_buf(),
            process: tokio::sync::Mutex::new(process),
            request_id: AtomicU64::new(0),
            capabilities: Mutex::new(None),
            opened_files: Mutex::new(HashMap::new()),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            notif_tx: tx,
            notif_rx: Mutex::new(Some(rx)),
        };

        Ok(client)
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

        let notif_rx = self.notif_rx.lock().await.take();
        if let Some(mut rx) = notif_rx {
            let server_id = self.server_id.clone();
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    tracing::debug!(server = %server_id, "LSP notification: {}", msg);
                }
            });
        }

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

        self.opened_files
            .lock()
            .await
            .insert(uri.to_string(), version);
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

        self.opened_files
            .lock()
            .await
            .insert(uri.to_string(), version);
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
        {
            let mut proc = self.process.lock().await;
            launch::send_request(&mut proc, &msg_str).await?;
        }

        let result = tokio::time::timeout(Self::REQUEST_TIMEOUT, async {
            loop {
                let resp_str = {
                    let mut proc = self.process.lock().await;
                    launch::read_response(&mut proc).await?
                };
                let resp: serde_json::Value = serde_json::from_str(&resp_str)?;

                if let Some(resp_id) = resp.get("id") {
                    if resp_id.as_i64() == Some(id as i64) {
                        if let Some(err) = resp.get("error") {
                            return Err(LspError::RequestFailed(format!(
                                "LSP error {}: {}",
                                err.get("code").map(|c| c.to_string()).unwrap_or_default(),
                                err.get("message")
                                    .map(|m| m.to_string())
                                    .unwrap_or_default()
                            )));
                        }
                        return Ok(resp
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null));
                    }
                }
                if let Err(e) = self.notif_tx.send(resp_str) {
                    warn!(error = %e, "failed to send notification to channel");
                }
            }
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => Err(LspError::RequestTimeout(format!(
                "LSP request '{}' timed out after {:?}",
                method,
                Self::REQUEST_TIMEOUT
            ))),
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

    pub async fn process_notification(&self, notification: &str) {
        let val: serde_json::Value = match serde_json::from_str(notification) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "failed to parse LSP notification");
                return;
            }
        };

        let method = val.get("method").and_then(|m| m.as_str()).unwrap_or("");

        if method == "textDocument/publishDiagnostics" {
            if let Some(params) = val.get("params") {
                if let Some(uri) = params.get("uri").and_then(|u| u.as_str()) {
                    if let Some(diags) = params.get("diagnostics") {
                        let parse_result: Result<Vec<lsp_types::Diagnostic>, _> =
                            serde_json::from_value(diags.clone());
                        match parse_result {
                            Ok(diags) => {
                                let count = diags.len();
                                self.diagnostics.lock().await.insert(uri.to_string(), diags);
                                debug!(uri, count, "received diagnostics");
                            }
                            Err(e) => {
                                warn!("Failed to parse diagnostics for {}: {}", uri, e);
                            }
                        }
                    }
                }
            }
        }
    }
}
