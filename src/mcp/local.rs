use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};

use crate::error::McpError;
use crate::mcp::{
    protocol, McpPrompt, McpResource, McpResourceContent, McpTool, McpToolCallResult,
    PromptArgument,
};

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<serde_json::Value>,
}

fn format_rpc_error(error: &JsonRpcError) -> String {
    match &error.data {
        Some(data) => format!("{} (code {}; data: {})", error.message, error.code, data),
        None => format!("{} (code {})", error.message, error.code),
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
}

type PendingSenders =
    Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<serde_json::Value, McpError>>>>>;

pub struct LocalClient {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    timeout: u64,
    child: Option<Child>,
    stdin: Option<tokio::process::ChildStdin>,
    pending: PendingSenders,
    shutdown_notify: Arc<Notify>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    request_id: AtomicU64,
    server_version: Option<String>,
    protocol: Option<protocol::NegotiatedProtocol>,
    discovery_metadata: Option<serde_json::Value>,
}

impl LocalClient {
    pub fn new(
        command: &str,
        args: Vec<String>,
        env: HashMap<String, String>,
        timeout: u64,
    ) -> Self {
        Self {
            command: command.to_string(),
            args,
            env,
            timeout,
            child: None,
            stdin: None,
            pending: Arc::new(Mutex::new(HashMap::new())),
            shutdown_notify: Arc::new(Notify::new()),
            stderr_task: None,
            request_id: AtomicU64::new(1),
            server_version: None,
            protocol: None,
            discovery_metadata: None,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), McpError> {
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .env_clear();

        if let Some(user_path) = std::env::var_os("PATH") {
            cmd.env("PATH", user_path);
        } else {
            cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }

        for (k, v) in &self.env {
            cmd.env(k, v);
        }

        let spawn_timeout = Duration::from_millis(self.timeout.min(10000));
        let mut child = match tokio::time::timeout(
            spawn_timeout,
            tokio::task::spawn_blocking(move || cmd.spawn()),
        )
        .await
        {
            Ok(Ok(Ok(child))) => child,
            Ok(Ok(Err(e))) => {
                return Err(McpError::Connection(format!(
                    "failed to spawn {}: {e}",
                    self.command
                )));
            }
            Ok(Err(e)) => {
                return Err(McpError::Connection(format!(
                    "failed to spawn {}: task join error: {e}",
                    self.command
                )));
            }
            Err(_) => {
                return Err(McpError::Connection(format!(
                    "failed to spawn {}: timeout after {:?}",
                    self.command, spawn_timeout
                )));
            }
        };

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Connection("failed to take stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Connection("failed to take stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| McpError::Connection("failed to take stderr".into()))?;

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.stderr_task = Some(tokio::spawn(Self::drain_stderr(stderr)));

        let pending = Arc::clone(&self.pending);
        let shutdown = Arc::clone(&self.shutdown_notify);
        tokio::spawn(async move {
            Self::read_loop(stdout, pending, shutdown).await;
        });

        let probe = self
            .send_request_with_protocol(
                protocol::SERVER_DISCOVER_METHOD,
                protocol::modern_discovery_params(),
                &protocol::NegotiatedProtocol::modern(),
            )
            .await;
        match probe {
            Ok(result) => {
                let discovery = protocol::parse_discovery(&result);
                if let Some(negotiated) = discovery.protocol {
                    self.protocol = Some(negotiated.clone());
                    self.server_version = discovery.server_version;
                    self.discovery_metadata = discovery.metadata;
                    if negotiated.is_modern() {
                        return Ok(());
                    }
                } else {
                    return Err(McpError::Server(
                        "server/discover returned no compatible protocol version".into(),
                    ));
                }
            }
            Err(error) if protocol::is_legacy_probe_error(&error) => {}
            Err(error) => return Err(error),
        }

        self.initialize_legacy().await
    }

    async fn initialize_legacy(&mut self) -> Result<(), McpError> {
        let result = self
            .send_request_with_protocol(
                "initialize",
                protocol::legacy_initialize_params(),
                &protocol::NegotiatedProtocol::legacy(),
            )
            .await?;
        self.protocol = Some(protocol::legacy_protocol_from_initialize(&result));
        self.server_version = result
            .pointer("/serverInfo/version")
            .and_then(|version| version.as_str())
            .map(str::to_owned);

        self.send_notification("notifications/initialized", json!({}))
            .await?;

        Ok(())
    }

    pub fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    pub fn protocol_version(&self) -> Option<&str> {
        self.protocol
            .as_ref()
            .map(protocol::NegotiatedProtocol::version)
    }

    pub fn discovery_metadata(&self) -> Option<serde_json::Value> {
        self.discovery_metadata.clone()
    }

    pub async fn discover_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let result = self.send_request("tools/list", json!({})).await?;
        protocol::parse_tools(&result, "")
    }

    pub async fn call_tool(
        &mut self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        Ok(self.call_tool_structured(tool, arguments).await?.text)
    }

    pub async fn call_tool_structured(
        &mut self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        let params = json!({
            "name": tool,
            "arguments": arguments
        });

        let result = self.send_request("tools/call", params).await?;
        let content = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let text_parts: Vec<String> = content
            .iter()
            .filter_map(|c| {
                c.get("type")
                    .and_then(|t| t.as_str())
                    .filter(|t| *t == "text")
                    .and_then(|_| c.get("text").and_then(|t| t.as_str()))
                    .map(String::from)
            })
            .collect();
        let structured = result.get("structuredContent").cloned().or_else(|| {
            content.iter().find_map(|c| {
                (c.get("type").and_then(|t| t.as_str()) == Some("json"))
                    .then(|| c.get("json").cloned())
                    .flatten()
            })
        });
        if content.is_empty() && structured.is_none() {
            return Err(McpError::ToolCall("invalid tool result".into()));
        }
        let text = text_parts.join("\n");
        let text = if text.is_empty() {
            structured
                .as_ref()
                .map(serde_json::Value::to_string)
                .unwrap_or_default()
        } else {
            text
        };

        Ok(McpToolCallResult {
            text,
            structured,
            is_error: result
                .get("isError")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            result_type: result
                .get("resultType")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            metadata: protocol::bounded_optional_value(
                result.get("_meta"),
                protocol::MAX_MCP_METADATA_BYTES,
            ),
        })
    }

    pub async fn list_prompts(&mut self) -> Result<Vec<McpPrompt>, McpError> {
        let result = self.send_request("prompts/list", json!({})).await?;
        let prompts = result
            .get("prompts")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let name = p.get("name")?.as_str()?.to_string();
                        let description = p
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from);
                        let arguments = p.get("arguments").and_then(|a| a.as_array()).map(|arr| {
                            arr.iter()
                                .filter_map(|a| {
                                    let name = a.get("name")?.as_str()?.to_string();
                                    let description = a
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .map(String::from);
                                    let required = a.get("required").and_then(|r| r.as_bool());
                                    Some(PromptArgument {
                                        name,
                                        description,
                                        required,
                                    })
                                })
                                .collect()
                        });
                        Some(McpPrompt {
                            name,
                            description,
                            arguments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(prompts)
    }

    pub async fn get_prompt(
        &mut self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<String, McpError> {
        let params = json!({
            "name": name,
            "arguments": arguments.unwrap_or(json!({}))
        });

        let result = self.send_request("prompts/get", params).await?;
        let messages = result
            .get("messages")
            .and_then(|m| m.as_array())
            .ok_or_else(|| McpError::Server("invalid prompt response".into()))?;

        let text_parts: Vec<String> = messages
            .iter()
            .filter_map(|m| {
                m.get("content").and_then(|c| c.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            c.get("type")
                                .and_then(|t| t.as_str())
                                .filter(|t| *t == "text")
                                .and_then(|_| c.get("text").and_then(|t| t.as_str()))
                                .map(String::from)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .collect();

        Ok(text_parts.join("\n\n"))
    }

    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>, McpError> {
        let result = self.send_request("resources/list", json!({})).await?;
        let resources = result
            .get("resources")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        let uri = r.get("uri")?.as_str()?.to_string();
                        let name = r.get("name")?.as_str()?.to_string();
                        let description = r
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from);
                        let mime_type =
                            r.get("mimeType").and_then(|m| m.as_str()).map(String::from);
                        Some(McpResource {
                            uri,
                            name,
                            description,
                            mime_type,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(resources)
    }

    pub async fn read_resource(&mut self, uri: &str) -> Result<McpResourceContent, McpError> {
        let params = json!({ "uri": uri });
        let result = self.send_request("resources/read", params).await?;
        let contents = result
            .get("contents")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| McpError::Server("invalid resource response".into()))?;

        let uri = contents
            .get("uri")
            .and_then(|u| u.as_str())
            .unwrap_or(uri)
            .to_string();
        let mime_type = contents
            .get("mimeType")
            .and_then(|m| m.as_str())
            .map(String::from);
        let text = contents
            .get("text")
            .and_then(|t| t.as_str())
            .map(String::from);
        let blob = contents
            .get("blob")
            .and_then(|b| b.as_str())
            .map(String::from);

        Ok(McpResourceContent {
            uri,
            mime_type,
            text,
            blob,
        })
    }

    pub async fn shutdown(&mut self) -> Result<(), McpError> {
        if let Err(error) = self
            .send_notification("notifications/cancelled", json!({}))
            .await
        {
            tracing::warn!(error = %error, "failed to send MCP cancellation notification");
        }
        self.shutdown_notify.notify_waiters();
        if let Some(ref mut child) = self.child {
            if let Err(error) = child.kill().await {
                tracing::warn!(error = %error, "failed to kill MCP child during shutdown");
            }
            if let Err(error) = child.wait().await {
                tracing::warn!(error = %error, "failed to reap MCP child during shutdown");
            }
        }
        if let Some(task) = self.stderr_task.take() {
            let _ = task.await;
        }
        self.child = None;
        self.stdin = None;
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let protocol = self
            .protocol
            .clone()
            .unwrap_or_else(protocol::NegotiatedProtocol::legacy);
        self.send_request_with_protocol(method, params, &protocol)
            .await
    }

    async fn send_request_with_protocol(
        &mut self,
        method: &str,
        params: serde_json::Value,
        protocol: &protocol::NegotiatedProtocol,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params: if protocol.is_modern() {
                protocol::modern_params(params)
            } else {
                params
            },
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        if let Err(e) = self.write_json(&request).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }

        let timeout = Duration::from_millis(self.timeout);
        let result = tokio::time::timeout(timeout, rx).await;
        let value = match result {
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(McpError::ToolCall(format!("request {method} timed out")));
            }
            Ok(Err(_)) => return Err(McpError::Connection("receiver dropped".into())),
            Ok(Ok(val)) => val?,
        };

        Ok(value)
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpError> {
        let protocol = self
            .protocol
            .clone()
            .unwrap_or_else(protocol::NegotiatedProtocol::legacy);
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: if protocol.is_modern() {
                protocol::modern_params(params)
            } else {
                params
            },
        };
        self.write_json(&notification).await
    }

    async fn write_json<T: Serialize>(&mut self, msg: &T) -> Result<(), McpError> {
        let data = serde_json::to_string(msg).map_err(|e| McpError::Server(e.to_string()))?;

        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(McpError::Connection(format!(
                        "MCP server process exited with status: {}",
                        status
                    )));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(McpError::Connection(format!(
                        "failed to check MCP server process status: {}",
                        e
                    )));
                }
            }
        }

        let stdin = self.stdin.as_mut().ok_or_else(|| {
            McpError::Connection(
                "MCP server process has no stdin available (process may have exited)".into(),
            )
        })?;

        let line = format!("{data}\n");
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::Connection(format!("stdin write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Connection(format!("stdin flush failed: {}", e)))?;

        Ok(())
    }

    async fn read_loop(
        stdout: tokio::process::ChildStdout,
        pending: PendingSenders,
        shutdown: Arc<Notify>,
    ) {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            let read_result = tokio::select! {
                biased;
                _ = shutdown.notified() => break,
                result = reader.read_line(&mut line) => result,
            };

            let bytes = match read_result {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            if bytes == 0 {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if let Some(id) = response.id {
                    let mut pending_lock = pending.lock().await;
                    if let Some(tx) = pending_lock.remove(&id) {
                        let result = if let Some(err) = response.error {
                            Err(McpError::Server(format_rpc_error(&err)))
                        } else if let Some(result) = response.result {
                            Ok(result)
                        } else {
                            Ok(serde_json::Value::Null)
                        };
                        let _ = tx.send(result);
                    }
                }
            }
        }

        // Drain all pending senders so callers get an error instead of hanging
        let mut pending_lock = pending.lock().await;
        for (_, tx) in pending_lock.drain() {
            let _ = tx.send(Err(McpError::Connection(
                "MCP server connection closed".into(),
            )));
        }
    }

    async fn drain_stderr(stderr: tokio::process::ChildStderr) {
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0_u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(&mut reader, &mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }
}

impl Drop for LocalClient {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modern_fixture() -> &'static str {
        r#"
            while IFS= read -r line; do
                case "$line" in
                    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"supportedVersions":["2026-07-28"],"capabilities":{"tools":{}},"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"modern-fixture","version":"9.1"}},"instructions":"fixture"}}' ;;
                    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"search","description":"Search","inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"annotations":{"readOnlyHint":true},"_meta":{"source":"fixture"}}]}}' ;;
                    *tools/call*) printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"resultType":"complete","content":[{"type":"text","text":"done"}],"structuredContent":{"answer":42},"isError":true,"_meta":{"trace":"fixture"},"unknown":true}}' ;;
                esac
            done
        "#
    }

    #[tokio::test(flavor = "current_thread")]
    async fn modern_probe_preserves_protocol_tool_metadata_and_result_envelope() {
        let mut client = LocalClient::new(
            "sh",
            vec!["-c".to_string(), modern_fixture().to_string()],
            HashMap::new(),
            2_000,
        );
        client.initialize().await.expect("modern probe succeeds");
        assert_eq!(client.protocol_version(), Some("2026-07-28"));
        assert_eq!(client.server_version(), Some("9.1"));
        assert!(client.discovery_metadata().is_some());

        let tools = client.discover_tools().await.expect("tool list succeeds");
        assert_eq!(tools[0].output_schema, Some(json!({"type": "object"})));
        assert_eq!(tools[0].annotations, Some(json!({"readOnlyHint": true})));
        assert_eq!(tools[0].metadata, Some(json!({"source": "fixture"})));

        let result = client
            .call_tool_structured("search", json!({}))
            .await
            .expect("tool call succeeds");
        assert_eq!(result.text, "done");
        assert_eq!(result.structured, Some(json!({"answer": 42})));
        assert!(result.is_error);
        assert_eq!(result.result_type.as_deref(), Some("complete"));
        assert_eq!(result.metadata, Some(json!({"trace": "fixture"})));
        client.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_legacy_probe_error_falls_back_to_initialize() {
        let script = r#"
            while IFS= read -r line; do
                case "$line" in
                    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}' ;;
                    *initialize*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"protocolVersion":"2024-11-05","serverInfo":{"version":"legacy"}}}' ;;
                    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}' ;;
                esac
            done
        "#;
        let mut client = LocalClient::new(
            "sh",
            vec!["-c".to_string(), script.to_string()],
            HashMap::new(),
            2_000,
        );
        client.initialize().await.expect("legacy fallback succeeds");
        assert_eq!(client.protocol_version(), Some("2024-11-05"));
        assert_eq!(client.server_version(), Some("legacy"));
        client.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn noisy_stderr_does_not_block_initialize() {
        let script = r#"
            head -c 131072 /dev/zero >&2
            while IFS= read -r line; do
                case "$line" in
                    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}' ;;
                    *initialize*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{}}' ;;
                esac
            done
        "#;
        let mut client = LocalClient::new(
            "sh",
            vec!["-c".to_string(), script.to_string()],
            HashMap::new(),
            2_000,
        );
        client
            .initialize()
            .await
            .expect("noisy MCP stderr must not deadlock initialization");
        client.shutdown().await.expect("shutdown noisy MCP fixture");
    }
}
