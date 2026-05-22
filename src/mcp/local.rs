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
use crate::mcp::{McpPrompt, McpResource, McpResourceContent, PromptArgument};
use crate::provider::ToolDefinition;

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
    message: String,
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
    request_id: AtomicU64,
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
            request_id: AtomicU64::new(1),
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

        self.child = Some(child);
        self.stdin = Some(stdin);

        let pending = Arc::clone(&self.pending);
        let shutdown = Arc::clone(&self.shutdown_notify);
        tokio::spawn(async move {
            Self::read_loop(stdout, pending, shutdown).await;
        });

        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "codegg",
                "version": "0.1.0"
            }
        });

        let _result = self.send_request("initialize", init_params).await?;

        self.send_notification("notifications/initialized", json!({}))
            .await?;

        Ok(())
    }

    pub async fn discover_tools(&mut self) -> Result<Vec<ToolDefinition>, McpError> {
        let result = self.send_request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| McpError::Server("invalid tools response".into()))?;

        Ok(tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?.to_string();
                let description = t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let parameters = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({ "type": "object", "properties": {} }));
                Some(ToolDefinition {
                    name,
                    description,
                    parameters,
                })
            })
            .collect())
    }

    pub async fn call_tool(
        &mut self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let params = json!({
            "name": tool,
            "arguments": arguments
        });

        let result = self.send_request("tools/call", params).await?;
        let content = result
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| McpError::ToolCall("invalid tool result".into()))?;

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

        Ok(text_parts.join("\n"))
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
        let _ = self
            .send_notification("notifications/cancelled", json!({}))
            .await;
        self.shutdown_notify.notify_waiters();
        if let Some(ref mut child) = self.child {
            let _ = child.kill().await;
            let _ = child.wait().await;
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
        let id = self.next_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        self.write_json(&request).await?;

        let timeout = Duration::from_millis(self.timeout);
        let result = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| McpError::ToolCall(format!("request {method} timed out")))?
            .map_err(|_| McpError::Connection("receiver dropped".into()))??;

        Ok(result)
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpError> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        self.write_json(&notification).await
    }

    async fn write_json<T: Serialize>(&mut self, msg: &T) -> Result<(), McpError> {
        let data = serde_json::to_string(msg).map_err(|e| McpError::Server(e.to_string()))?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| McpError::Connection("stdin not available".into()))?;

        let line = format!("{data}\n");
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

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
                            Err(McpError::Server(err.message))
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
    }
}

impl Drop for LocalClient {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}
