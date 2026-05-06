//! IDE MCP Server - provides IDE integration tools via MCP protocol.
//!
//! This server exposes IDE diff viewing capabilities as MCP tools, supporting
//! both stdio transport (for IDE extensions) and unix socket mode.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{stdin, stdout, Write};
use std::sync::Arc;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify};

use crate::error::McpError;

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<u64>,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
}

type PendingRequests =
    Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<serde_json::Value, McpError>>>>>;

pub struct IdeServer {
    tools: HashMap<String, ToolHandler>,
    pending: PendingRequests,
    shutdown: Arc<Mutex<bool>>,
    shutdown_notify: Arc<Notify>,
}

type ToolHandler =
    Arc<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;

impl IdeServer {
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        tools.insert(
            "openDiff".to_string(),
            Arc::new(open_diff_handler) as ToolHandler,
        );

        Self {
            tools,
            pending: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(Mutex::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    #[allow(unused_mut)]
    pub async fn run_stdio(&self) -> Result<(), McpError> {
        let mut stdin = stdin();
        let mut stdout = stdout();
        let mut input = String::new();

        let mut initialized = false;

        loop {
            input.clear();
            let read_result = stdin.read_line(&mut input);
            match read_result {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }

            let trimmed = input.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
                let response = self.handle_request(request, &mut initialized).await;
                let response_json = serde_json::to_string(&response)
                    .map_err(|e| McpError::Server(e.to_string()))?;
                writeln!(stdout, "{}", response_json)
                    .map_err(|e| McpError::Connection(e.to_string()))?;
                stdout
                    .flush()
                    .map_err(|e| McpError::Connection(e.to_string()))?;
            }
        }

        Ok(())
    }

    pub async fn run_socket(&self, socket_path: &str) -> Result<(), McpError> {
        let listener = UnixListener::bind(socket_path)
            .map_err(|e| McpError::Connection(format!("failed to bind socket: {}", e)))?;

        loop {
            tokio::select! {
                biased;
                _ = self.shutdown_notify.notified() => break,
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let server = Arc::new(self.clone_for_connection());
                            tokio::spawn(async move {
                                let _ = server.handle_connection(stream).await;
                            });
                        }
                        Err(_) => continue,
                    }
                }
            }
        }

        Ok(())
    }

    fn clone_for_connection(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            pending: Arc::clone(&self.pending),
            shutdown: Arc::clone(&self.shutdown),
            shutdown_notify: Arc::clone(&self.shutdown_notify),
        }
    }

    async fn handle_connection(&self, mut stream: UnixStream) -> Result<(), McpError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        use BufReader as SyncBufReader;

        let (reader, mut writer) = stream.split();
        let mut reader = SyncBufReader::new(reader);
        let mut line = String::new();
        let mut initialized = false;

        loop {
            line.clear();
            let read_result = reader.read_line(&mut line).await;
            match read_result {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(trimmed) {
                let response = self.handle_request(request, &mut initialized).await;
                let response_json = serde_json::to_string(&response)
                    .map_err(|e| McpError::Server(e.to_string()))?;
                writer
                    .write_all(format!("{}\n", response_json).as_bytes())
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
                writer
                    .flush()
                    .await
                    .map_err(|e| McpError::Connection(e.to_string()))?;
            }
        }

        Ok(())
    }

    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        initialized: &mut bool,
    ) -> JsonRpcResponse {
        let id = request.id;
        let method = request.method;
        let params = request.params;

        if method == "initialize" {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "codegg-ide",
                    "version": "0.1.0"
                }
            });
            *initialized = true;
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            };
        }

        if !*initialized && method != "initialize" {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32002,
                    message: "server not initialized".to_string(),
                    data: None,
                }),
            };
        }

        if method == "notifications/initialized" {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: None,
                result: Some(serde_json::Value::Null),
                error: None,
            };
        }

        let result = match method.as_str() {
            "tools/list" => {
                let tools: Vec<serde_json::Value> = self
                    .tools
                    .keys()
                    .map(|name| {
                        json!({
                            "name": name,
                            "description": get_tool_description(name),
                            "inputSchema": get_tool_schema(name)
                        })
                    })
                    .collect();
                json!({ "tools": tools })
            }
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                if let Some(handler) = self.tools.get(name) {
                    match handler(arguments) {
                        Ok(result) => json!({
                            "content": [{
                                "type": "text",
                                "text": result.to_string()
                            }]
                        }),
                        Err(e) => {
                            return JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32603,
                                    message: e,
                                    data: None,
                                }),
                            };
                        }
                    }
                } else {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32601,
                            message: format!("tool not found: {}", name),
                            data: None,
                        }),
                    };
                }
            }
            _ => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("method not found: {}", method),
                        data: None,
                    }),
                };
            }
        };

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn shutdown(&self) {
        let shutdown = self.shutdown.clone();
        let notify = self.shutdown_notify.clone();
        tokio::spawn(async move {
            *shutdown.lock().await = true;
            notify.notify_one();
        });
    }
}

impl Default for IdeServer {
    fn default() -> Self {
        Self::new()
    }
}

fn get_tool_description(name: &str) -> &'static str {
    match name {
        "openDiff" => {
            "Opens the native IDE diff viewer with specified files and optional line ranges"
        }
        _ => "",
    }
}

fn get_tool_schema(name: &str) -> serde_json::Value {
    match name {
        "openDiff" => json!({
            "type": "object",
            "properties": {
                "original": {
                    "type": "string",
                    "description": "Original file path (or @file#L1-L99 syntax)"
                },
                "modified": {
                    "type": "string",
                    "description": "Modified file path (or @file#L1-L99 syntax)"
                }
            },
            "required": ["original", "modified"]
        }),
        _ => json!({ "type": "object", "properties": {} }),
    }
}

fn open_diff_handler(arguments: serde_json::Value) -> Result<serde_json::Value, String> {
    let original = arguments
        .get("original")
        .and_then(|v| v.as_str())
        .ok_or("original file required")?;
    let modified = arguments
        .get("modified")
        .and_then(|v| v.as_str())
        .ok_or("modified file required")?;

    let (original_path, original_lines) = parse_file_reference(original)?;
    let (modified_path, modified_lines) = parse_file_reference(modified)?;

    crate::ide::open_diff(
        &original_path,
        &modified_path,
        original_lines,
        modified_lines,
    )
    .map_err(|e| e.to_string())?;

    Ok(json!({
        "success": true,
        "message": format!("Opened diff: {} vs {}", original_path, modified_path)
    }))
}

fn parse_file_reference(input: &str) -> Result<(String, Option<(usize, usize)>), String> {
    let input = input.trim();

    if let Some(at_pos) = input.find('@') {
        let path = input[..at_pos].trim().to_string();
        let rest = &input[at_pos..];

        let line_spec = rest.strip_prefix('@').ok_or("invalid @ syntax")?;

        if let Some(hash_pos) = line_spec.find('#') {
            let file_part = &line_spec[..hash_pos];
            let lines_part = &line_spec[hash_pos + 1..];

            let lines = parse_line_range(lines_part)?;
            let path = if file_part.is_empty() {
                path
            } else {
                if !path.is_empty() {
                    format!("{}@{}", path, file_part)
                } else {
                    file_part.to_string()
                }
            };
            return Ok((path, Some(lines)));
        }

        return Ok((path, None));
    }

    Ok((input.to_string(), None))
}

fn parse_line_range(s: &str) -> Result<(usize, usize), String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok((1, usize::MAX));
    }

    if let Some(dash_pos) = s.find('-') {
        let start: usize = s[..dash_pos]
            .trim()
            .parse()
            .map_err(|_| "invalid start line")?;
        let end: usize = s[dash_pos + 1..]
            .trim()
            .parse()
            .map_err(|_| "invalid end line")?;
        Ok((start, end))
    } else {
        let line: usize = s.parse().map_err(|_| "invalid line number")?;
        Ok((line, line))
    }
}
