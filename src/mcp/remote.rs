use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::join;
use tokio::sync::{Mutex, Notify};
use tokio::time::sleep;

use crate::error::McpError;
use crate::mcp::{McpPrompt, McpResource, McpResourceContent, PromptArgument};
use crate::provider::ToolDefinition;
use crate::security::ssrf::{revalidate_dns, validate_host_ip, validate_url_host};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ConnectionState {
    Connected,
    #[default]
    Disconnected,
    Reconnecting {
        attempt: u32,
    },
}

pub struct McpConnectionManager {
    client: RemoteClient,
    state: Arc<Mutex<ConnectionState>>,
    retry_count: Arc<AtomicU64>,
    max_retries: u64,
    base_delay: Duration,
    max_delay: Duration,
    heartbeat_interval: Duration,
    heartbeat_task: Arc<AtomicBool>,
    shutdown: Arc<Notify>,
}

impl McpConnectionManager {
    pub fn new(
        url: &str,
        headers: HashMap<String, String>,
        timeout: u64,
    ) -> Result<Self, McpError> {
        let client = RemoteClient::new(url, headers, timeout)?;
        Ok(Self {
            client,
            state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            retry_count: Arc::new(AtomicU64::new(0)),
            max_retries: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_task: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(Notify::default()),
        })
    }

    pub async fn connect(&mut self) -> Result<(), McpError> {
        *self.state.lock().await = ConnectionState::Connected;
        self.retry_count.store(0, Ordering::SeqCst);
        self.client.initialize().await?;
        self.start_heartbeat().await;
        Ok(())
    }

    async fn start_heartbeat(&self) {
        let client = Arc::new(Mutex::new(self.client.clone()));
        let interval = self.heartbeat_interval;
        let running = Arc::clone(&self.heartbeat_task);
        let shutdown = Arc::clone(&self.shutdown);
        let state = Arc::clone(&self.state);

        running.store(true, Ordering::SeqCst);
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown.notified() => break,
                    _ = interval_timer.tick() => {
                        if !running.load(Ordering::SeqCst) {
                            break;
                        }
                        let current_state = state.lock().await.clone();
                        if !matches!(current_state, ConnectionState::Connected) {
                            break;
                        }
                        let mut c = client.lock().await;
                        if let Err(e) = c.send_notification("ping", json!({})).await {
                            tracing::warn!("heartbeat failed: {}, will reconnect", e);
                            break;
                        }
                    }
                }
            }
            running.store(false, Ordering::SeqCst);
        });
    }

    pub async fn reconnect(&mut self) -> Result<(), McpError> {
        loop {
            let retry_count = self.retry_count.fetch_add(1, Ordering::SeqCst);
            let multiplier = 2u64.saturating_pow(retry_count as u32);
            let delay_secs = self
                .base_delay
                .as_secs()
                .saturating_mul(multiplier)
                .min(self.max_delay.as_secs());
            let delay = Duration::from_secs(delay_secs);

            tracing::info!(
                "attempting reconnect in {:?} (attempt {})",
                delay,
                retry_count + 1
            );
            *self.state.lock().await = ConnectionState::Reconnecting {
                attempt: retry_count as u32 + 1,
            };

            if retry_count >= self.max_retries {
                *self.state.lock().await = ConnectionState::Disconnected;
                return Err(McpError::Connection(
                    "max reconnection attempts exceeded".into(),
                ));
            }

            sleep(delay).await;

            match self.client.reconnect().await {
                Ok(_) => {
                    *self.state.lock().await = ConnectionState::Connected;
                    self.retry_count.store(0, Ordering::SeqCst);
                    self.start_heartbeat().await;
                    return Ok(());
                }
                Err(e) => {
                    if retry_count + 1 >= self.max_retries {
                        *self.state.lock().await = ConnectionState::Disconnected;
                        return Err(e);
                    }
                    tracing::warn!("reconnect attempt {} failed: {}", retry_count + 1, e);
                    continue;
                }
            }
        }
    }

    pub async fn ensure_connected(&mut self) -> Result<(), McpError> {
        let state = self.state.lock().await.clone();
        match state {
            ConnectionState::Connected => Ok(()),
            ConnectionState::Disconnected | ConnectionState::Reconnecting { .. } => {
                self.connect().await
            }
        }
    }

    pub async fn state(&self) -> ConnectionState {
        self.state.lock().await.clone()
    }

    pub fn client_mut(&mut self) -> &mut RemoteClient {
        &mut self.client
    }

    pub fn client(&self) -> &RemoteClient {
        &self.client
    }

    pub async fn shutdown(&self) {
        self.heartbeat_task.store(false, Ordering::SeqCst);
        self.shutdown.notify_one();
        self.client.shutdown().await;
        *self.state.lock().await = ConnectionState::Disconnected;
    }

    pub async fn set_oauth_token(&self, token: String) {
        self.client.set_oauth_token(token).await;
    }

    pub async fn discover_tools(&mut self) -> Result<Vec<ToolDefinition>, McpError> {
        self.ensure_connected().await?;
        self.client.discover_tools().await
    }

    pub async fn call_tool(
        &mut self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        self.ensure_connected().await?;
        self.client.call_tool(tool, arguments).await
    }

    pub async fn list_prompts(&mut self) -> Result<Vec<McpPrompt>, McpError> {
        self.ensure_connected().await?;
        self.client.list_prompts().await
    }

    pub async fn get_prompt(
        &mut self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<String, McpError> {
        self.ensure_connected().await?;
        self.client.get_prompt(name, arguments).await
    }

    pub async fn list_resources(&mut self) -> Result<Vec<McpResource>, McpError> {
        self.ensure_connected().await?;
        self.client.list_resources().await
    }

    pub async fn read_resource(&mut self, uri: &str) -> Result<McpResourceContent, McpError> {
        self.ensure_connected().await?;
        self.client.read_resource(uri).await
    }

    pub async fn disconnect(&mut self) -> Result<(), McpError> {
        self.client.disconnect().await
    }

    pub fn max_retries(&self) -> u64 {
        self.max_retries
    }

    pub fn set_max_retries(&mut self, max_retries: u64) {
        self.max_retries = max_retries;
    }

    pub fn base_delay(&self) -> Duration {
        self.base_delay
    }

    pub fn set_base_delay(&mut self, base_delay: Duration) {
        self.base_delay = base_delay;
    }

    pub fn max_delay(&self) -> Duration {
        self.max_delay
    }

    pub fn set_max_delay(&mut self, max_delay: Duration) {
        self.max_delay = max_delay;
    }

    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    pub fn set_heartbeat_interval(&mut self, interval: Duration) {
        self.heartbeat_interval = interval;
    }
}

pub struct RemoteClient {
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
    session_id: Mutex<Option<String>>,
    sse_url: Mutex<Option<String>>,
    oauth_token: Mutex<Option<String>>,
    sse_events: Arc<Mutex<Vec<serde_json::Value>>>,
    request_id: AtomicU64,
    shutdown: Arc<Mutex<bool>>,
    sse_shutdown: Arc<Notify>,
    validated_ips: Mutex<Option<Vec<IpAddr>>>,
}

impl Clone for RemoteClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            headers: self.headers.clone(),
            client: self.client.clone(),
            session_id: Mutex::new(None),
            sse_url: Mutex::new(None),
            oauth_token: Mutex::new(None),
            sse_events: Arc::clone(&self.sse_events),
            request_id: AtomicU64::new(1),
            shutdown: Arc::clone(&self.shutdown),
            sse_shutdown: Arc::clone(&self.sse_shutdown),
            validated_ips: Mutex::new(None),
        }
    }
}

#[derive(Debug, Serialize)]
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
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
}

impl RemoteClient {
    pub fn new(
        url: &str,
        headers: HashMap<String, String>,
        timeout: u64,
    ) -> Result<Self, McpError> {
        let host = validate_url_host(url).map_err(McpError::Connection)?;

        let parsed = reqwest::Url::parse(url)
            .map_err(|e| McpError::Connection(format!("invalid URL: {}", e)))?;
        let port = parsed
            .port()
            .unwrap_or_else(|| if parsed.scheme() == "https" { 443 } else { 80 });
        let validated_ips = validate_host_ip(&host, port).map_err(McpError::Connection)?;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(timeout))
            .build()
            .map_err(|e| McpError::Connection(format!("failed to create HTTP client: {}", e)))?;

        Ok(Self {
            url: url.to_string(),
            headers,
            client,
            session_id: Mutex::new(None),
            sse_url: Mutex::new(None),
            oauth_token: Mutex::new(None),
            sse_events: Arc::new(Mutex::new(Vec::new())),
            request_id: AtomicU64::new(1),
            shutdown: Arc::new(Mutex::new(false)),
            sse_shutdown: Arc::new(Notify::default()),
            validated_ips: Mutex::new(Some(validated_ips)),
        })
    }

    pub async fn set_oauth_token(&self, token: String) {
        *self.oauth_token.lock().await = Some(token);
    }

    pub async fn clear_oauth_token(&self) {
        *self.oauth_token.lock().await = None;
    }

    pub async fn initialize(&mut self) -> Result<(), McpError> {
        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "codegg",
                "version": "0.1.0"
            }
        });

        let result = self.send_request("initialize", init_params).await?;

        if let Some(session_id) = result.get("sessionId").and_then(|s| s.as_str()) {
            *self.session_id.lock().await = Some(session_id.to_string());
        }

        if let Some(endpoints) = result.get("endpoints") {
            if let Some(sse) = endpoints.get("sse").and_then(|s| s.as_str()) {
                *self.sse_url.lock().await = Some(sse.to_string());
            }
        }

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

    pub async fn disconnect(&mut self) -> Result<(), McpError> {
        let _ = self
            .send_notification("notifications/cancelled", json!({}))
            .await;
        *self.session_id.lock().await = None;
        *self.sse_url.lock().await = None;
        Ok(())
    }

    pub async fn reconnect(&mut self) -> Result<(), McpError> {
        *self.session_id.lock().await = None;
        *self.sse_url.lock().await = None;
        self.initialize().await
    }

    pub async fn connect_sse(&self) -> Result<(), McpError> {
        let sse_url = self
            .sse_url
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("no SSE endpoint available".into()))?;

        let mut request = self.client.get(&sse_url);

        for (k, v) in &self.headers {
            if v.contains('\r') || v.contains('\n') {
                return Err(McpError::Server(
                    "header value contains invalid characters".into(),
                ));
            }
            request = request.header(k, v);
        }

        if let Some(ref token) = *self.oauth_token.lock().await {
            request = request.header("Authorization", format!("Bearer {token}"));
        }

        if let Some(ref session_id) = *self.session_id.lock().await {
            request = request.header("Mcp-Session-Id", session_id);
        }

        request = request.header("Accept", "text/event-stream");

        let resp = request
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(McpError::Connection(format!(
                "SSE connection failed: HTTP {}",
                resp.status()
            )));
        }

        self.connect_sse_stream(resp).await
    }

    pub async fn shutdown(&self) {
        *self.shutdown.lock().await = true;
        self.sse_shutdown.notify_one();
    }

    async fn connect_sse_stream(&self, resp: reqwest::Response) -> Result<(), McpError> {
        let events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::clone(&self.sse_events);
        let sse_shutdown = Arc::clone(&self.sse_shutdown);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buf = Vec::new();
            let mut data_lines = Vec::new();
            const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB limit

            use futures::StreamExt;
            loop {
                tokio::select! {
                    biased;
                    _ = sse_shutdown.notified() => {
                        break;
                    }
                    chunk = stream.next() => {
                        match chunk {
                            Some(Ok(bytes)) => {
                                if buf.len() + bytes.len() > MAX_BUFFER_SIZE {
                                    tracing::warn!("SSE buffer exceeded {} bytes, truncating", MAX_BUFFER_SIZE);
                                    break;
                                }
                                buf.extend_from_slice(&bytes);
                                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                                    let line = String::from_utf8_lossy(&buf[..pos]).to_string();
                                    buf.drain(..=pos);
                                    let trimmed = line.trim_end();

                                    if trimmed.starts_with("data: ") {
                                        data_lines
                                            .push(trimmed.strip_prefix("data: ").unwrap_or("").to_string());
                                    } else if trimmed.starts_with("data:") {
                                        data_lines
                                            .push(trimmed.strip_prefix("data:").unwrap_or("").to_string());
                                    } else if trimmed.is_empty() && !data_lines.is_empty() {
                                        let data = data_lines.join("\n");
                                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                                            events.lock().await.push(json);
                                        }
                                        data_lines.clear();
                                    }
                                }
                            }
                            Some(Err(_)) => break,
                            None => break,
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn take_sse_events(&self) -> Vec<serde_json::Value> {
        let mut events = self.sse_events.lock().await;
        std::mem::take(&mut *events)
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

        let response = self.post_json(&request).await?;

        if let Some(err) = response.error {
            return Err(McpError::Server(err.message));
        }

        response
            .result
            .ok_or_else(|| McpError::Server("empty response".into()))
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
        self.post_json(&notification).await?;
        Ok(())
    }

    async fn post_json<T: Serialize>(&self, msg: &T) -> Result<JsonRpcResponse, McpError> {
        let (oauth_token, session_id) = {
            let parsed = reqwest::Url::parse(&self.url)
                .map_err(|e| McpError::Connection(format!("invalid URL: {}", e)))?;
            let host = parsed
                .host_str()
                .ok_or_else(|| McpError::Connection("URL must have a host".to_string()))?
                .to_string();
            let port =
                parsed
                    .port()
                    .unwrap_or_else(|| if parsed.scheme() == "https" { 443 } else { 80 });

            let (oauth, sid, valid) = join!(
                async { self.oauth_token.lock().await.clone() },
                async { self.session_id.lock().await.clone() },
                async { self.validated_ips.lock().await.clone() }
            );

            if let Some(ref ips) = valid {
                revalidate_dns(&host, port, ips).map_err(McpError::Connection)?;
            }

            (oauth, sid)
        };

        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        for (k, v) in &self.headers {
            if v.contains('\r') || v.contains('\n') {
                return Err(McpError::Server(
                    "header value contains invalid characters".into(),
                ));
            }
            request = request.header(k, v);
        }

        if let Some(ref token) = oauth_token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }

        if let Some(ref sid) = session_id {
            request = request.header("Mcp-Session-Id", sid);
        }

        let body = serde_json::to_string(msg).map_err(|e| McpError::Server(e.to_string()))?;

        let resp = request
            .body(body)
            .send()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        if resp.status().is_success() {
            if let Some(sid) = resp.headers().get("Mcp-Session-Id") {
                if let Ok(sid_str) = sid.to_str() {
                    *self.session_id.lock().await = Some(sid_str.to_string());
                }
            }
        }

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| McpError::Connection(e.to_string()))?;

        if !status.is_success() {
            return Err(McpError::Server(format!("HTTP {status}: {text}")));
        }

        if text.is_empty() {
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: None,
                result: Some(serde_json::Value::Null),
                error: None,
            });
        }

        if text.starts_with("event:") {
            return self.parse_sse_response(&text);
        }

        serde_json::from_str::<JsonRpcResponse>(&text)
            .map_err(|e| McpError::Server(format!("invalid json response: {e}")))
    }

    fn parse_sse_response(&self, text: &str) -> Result<JsonRpcResponse, McpError> {
        let mut data_lines = Vec::new();
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                data_lines.push(data);
            } else if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data);
            }
        }

        let data = data_lines.join("\n");
        if data.is_empty() {
            return Err(McpError::Server("empty SSE data".into()));
        }

        serde_json::from_str::<JsonRpcResponse>(&data)
            .map_err(|e| McpError::Server(format!("invalid SSE data: {e}")))
    }
}
