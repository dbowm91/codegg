use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::sse_parser::parse_openai_buffer;
use crate::{
    assistant_text_content_value, create_http_client, openai_tool_arguments_value,
    project_tool_call_history, ChatRequest, ContentPart, EventStream, Message, ModelInfo, Provider,
    ReasoningVisibility, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures_util::stream::unfold;
use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::json;

use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Specific(String),
}

#[derive(Debug, Clone, Default)]
struct RequestPolicy {
    reasoning_field: Option<&'static str>,
    thinking_field: Option<&'static str>,
    tool_aliases: &'static [(&'static str, &'static str)],
    argument_aliases: &'static [(&'static str, &'static str, &'static str)],
}

// This is the bounded wire projection of the declarative adapter contract.
// It intentionally contains no provider credentials, transport settings, or
// executable behavior.  Matching is explicit and exclusion-aware; it is not
// a model-name substring heuristic.
const LAGUNA_TOOL_ALIASES: &[(&str, &str)] = &[("bash", "shell")];
const LAGUNA_ARGUMENT_ALIASES: &[(&str, &str, &str)] = &[("shell", "command", "cmd")];

fn request_policy(provider: &str, model: &str) -> RequestPolicy {
    let provider = provider.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    let supported_provider = matches!(
        provider.as_str(),
        "local" | "vllm" | "sglang" | "openai" | "openai-compatible" | "poolside"
    );
    let laguna_model = regex::Regex::new(r"laguna-(m|xs|s)")
        .expect("built-in adapter regex")
        .is_match(&model)
        && !regex::Regex::new(r"(base|embed)")
            .expect("built-in adapter exclusion regex")
            .is_match(&model);
    if supported_provider && laguna_model {
        RequestPolicy {
            reasoning_field: Some("reasoning_content"),
            thinking_field: Some("enable_thinking"),
            tool_aliases: LAGUNA_TOOL_ALIASES,
            argument_aliases: LAGUNA_ARGUMENT_ALIASES,
        }
    } else {
        RequestPolicy::default()
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleConfig {
    pub credential: Credential,
    pub base_url: String,
    pub auth_header: String,
    pub extra_headers: Vec<(String, String)>,
    pub models: Vec<ModelInfo>,
    pub tool_choice: ToolChoice,
}

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    pub id: String,
    pub name: String,
    pub config: OpenAiCompatibleConfig,
    session_affinity_header: Option<HeaderName>,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(id: &str, name: &str, config: OpenAiCompatibleConfig) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            config,
            session_affinity_header: None,
            client: create_http_client(),
        }
    }

    pub fn simple(id: &str, name: &str, api_key: &str, base_url: &str) -> Self {
        Self::simple_with_credential(id, name, Credential::api_key(api_key), base_url)
    }

    /// Construct a provider that accepts a full [`Credential`] envelope.
    ///
    /// This preserves the [`crate::auth::CredentialKind`] (and any future
    /// metadata such as `expires_at`) so the registered provider can be
    /// used as-is by code that wants to inspect the credential type, not
    /// just the secret.
    pub fn simple_with_credential(
        id: &str,
        name: &str,
        credential: Credential,
        base_url: &str,
    ) -> Self {
        Self::new(
            id,
            name,
            OpenAiCompatibleConfig {
                credential,
                base_url: base_url.to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: Vec::new(),
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        )
    }

    /// Require one request-context session identity under a provider-owned
    /// transport header. The name is configuration, never caller input.
    pub fn with_session_affinity_header(mut self, name: &str) -> Result<Self, ProviderError> {
        self.session_affinity_header =
            Some(HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::api("invalid_header", "session-affinity header name is invalid")
            })?);
        Ok(self)
    }

    fn request_builder(
        &self,
        request: &ChatRequest,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::RequestBuilder, ProviderError> {
        let auth_name =
            HeaderName::from_bytes(self.config.auth_header.as_bytes()).map_err(|_| {
                ProviderError::api(
                    "invalid_header",
                    "configured authentication header is invalid",
                )
            })?;
        let auth_value = HeaderValue::from_str(
            &self.config.credential.authorization_header_value(),
        )
        .map_err(|_| {
            ProviderError::api(
                "invalid_header",
                "configured authentication value is invalid",
            )
        })?;

        let mut reserved = vec![auth_name.clone(), HeaderName::from_static("content-type")];
        if let Some(name) = &self.session_affinity_header {
            reserved.push(name.clone());
        }

        let mut builder = self.client.post(url).header(auth_name, auth_value).header(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );

        let mut extra_names = Vec::with_capacity(self.config.extra_headers.len());
        for (name, value) in &self.config.extra_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ProviderError::api("invalid_header", "configured extra header name is invalid")
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                ProviderError::api("invalid_header", "configured extra header value is invalid")
            })?;
            if reserved.contains(&header_name) || extra_names.contains(&header_name) {
                return Err(ProviderError::api(
                    "reserved_header_collision",
                    "configured extra header collides with a transport-owned header",
                ));
            }
            extra_names.push(header_name.clone());
            builder = builder.header(header_name, header_value);
        }

        if let Some(session_header) = &self.session_affinity_header {
            let session_id = request.context.session_id.as_deref().ok_or_else(|| {
                ProviderError::api(
                    "missing_session_context",
                    "provider requires a canonical session context",
                )
            })?;
            let session_value = HeaderValue::from_str(session_id).map_err(|_| {
                ProviderError::api(
                    "invalid_session_context",
                    "session context is not a valid HTTP header value",
                )
            })?;
            builder = builder.header(session_header.clone(), session_value);
        }

        Ok(builder.json(body))
    }

    pub fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        let adapter = request_policy(&self.id, &request.model);
        let mut messages: Vec<serde_json::Value> = Vec::new();
        for msg in project_tool_call_history(&request.messages).iter() {
            match msg {
                Message::System { content } => {
                    messages.push(json!({"role": "system", "content": content}));
                }
                Message::User { content } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => {
                                json!({"type": "text", "text": text})
                            }
                            ContentPart::Image { image_url } => {
                                json!({
                                    "type": "image_url",
                                    "image_url": {"url": image_url.url}
                                })
                            }
                            ContentPart::Reasoning { .. } => json!(""),
                        })
                        .collect();
                    let content_val = if parts.len() == 1
                        && parts[0].get("type").and_then(|v| v.as_str()) == Some("text")
                    {
                        parts[0].get("text").cloned().unwrap_or(json!(""))
                    } else {
                        json!(parts)
                    };
                    messages.push(json!({"role": "user", "content": content_val}));
                }
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    let content_value = if tool_calls.is_empty() {
                        assistant_text_content_value(content)
                    } else {
                        let text = content
                            .iter()
                            .filter_map(|p| match p {
                                ContentPart::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        serde_json::Value::String(text)
                    };
                    let mut assistant_msg = json!({
                        "role": "assistant",
                        "content": content_value,
                    });

                    if !tool_calls.is_empty() {
                        let tool_calls_json: Vec<serde_json::Value> = tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": wire_tool_name(&adapter, tc.name.as_str()),
                                        "arguments": openai_tool_arguments_value(&tc.arguments),
                                    }
                                })
                            })
                            .collect();
                        assistant_msg["tool_calls"] = serde_json::json!(tool_calls_json);
                    }

                    // Laguna's OpenAI-compatible contract requires the
                    // provider-private reasoning_content field on the next
                    // assistant turn. Other models must not receive it.
                    if let Some(reasoning_field) = reasoning_field(&adapter) {
                        if let Some(reasoning) = content.iter().find_map(|part| match part {
                            ContentPart::Reasoning { text, visibility }
                                if *visibility == ReasoningVisibility::Private =>
                            {
                                Some(text.as_str())
                            }
                            _ => None,
                        }) {
                            assistant_msg[reasoning_field] = json!(reasoning);
                        }
                    }

                    messages.push(assistant_msg);
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": content,
                    }));
                }
            }
        }

        let tools_json = request.tools.as_ref().map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    let mut value = tool.to_openai();
                    if let Some(function) = value.get_mut("function") {
                        if let Some(name_value) = function.get_mut("name") {
                            if let Some(name) = name_value.as_str() {
                                *name_value = json!(wire_tool_name(&adapter, name));
                            }
                        }
                        if let Some(parameters) = function.get_mut("parameters") {
                            alias_parameter_properties(&adapter, tool.name.as_str(), parameters);
                        }
                    }
                    value
                })
                .collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "tools": tools_json,
        });
        if let Some((field, configured_value)) = thinking_transform(&adapter) {
            let value = if configured_value.as_deref() == Some("true") {
                json!(request.thinking_budget != Some(0))
            } else if configured_value.as_deref() == Some("false") {
                json!(false)
            } else {
                json!(configured_value.unwrap_or_else(|| "true".to_string()))
            };
            body["chat_template_kwargs"] = json!({field: value});
        }
        let has_tools = request
            .tools
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        if has_tools {
            match &self.config.tool_choice {
                ToolChoice::Auto => {
                    body["tool_choice"] = json!("auto");
                }
                ToolChoice::Required => {
                    body["tool_choice"] = json!("required");
                }
                ToolChoice::None => {
                    body["tool_choice"] = json!("none");
                }
                ToolChoice::Specific(name) => {
                    body["tool_choice"] = json!({
                        "type": "function",
                        "function": {"name": wire_tool_name(&adapter, name)}
                    });
                }
            }
        }

        body
    }
}

fn reasoning_field(adapter: &RequestPolicy) -> Option<&str> {
    adapter.reasoning_field
}

fn thinking_transform(adapter: &RequestPolicy) -> Option<(&str, Option<String>)> {
    adapter
        .thinking_field
        .map(|field| (field, Some("true".to_string())))
}

fn wire_tool_name(adapter: &RequestPolicy, name: &str) -> String {
    adapter
        .tool_aliases
        .iter()
        .find_map(|(canonical, wire)| (*canonical == name).then_some(*wire))
        .unwrap_or(name)
        .to_string()
}

fn alias_parameter_properties(
    adapter: &RequestPolicy,
    tool_name: &str,
    parameters: &mut serde_json::Value,
) {
    let wire_name = wire_tool_name(adapter, tool_name);
    let Some(properties) = parameters
        .get_mut("properties")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    for (alias_tool, canonical, wire) in adapter.argument_aliases {
        if *alias_tool != wire_name {
            continue;
        }
        if let Some(schema) = properties.remove(*canonical) {
            properties.insert((*wire).to_string(), schema);
        }
    }
}

fn normalize_openai_event(
    event: Result<crate::ChatEvent, ProviderError>,
    adapter: &RequestPolicy,
) -> Option<Result<crate::ChatEvent, ProviderError>> {
    match event {
        Ok(crate::ChatEvent::ReasoningDelta(_)) if reasoning_field(adapter).is_none() => None,
        Ok(crate::ChatEvent::ToolCall(mut call)) => {
            let wire_name = call.name.to_string();
            let canonical_name = adapter
                .tool_aliases
                .iter()
                .find_map(|(canonical, wire)| (*wire == wire_name).then_some(*canonical))
                .unwrap_or(wire_name.as_str());
            if let Some(args) = call.arguments.as_object_mut() {
                for (tool, canonical, wire) in adapter.argument_aliases {
                    if *tool == wire_name {
                        if let Some(value) = args.remove(*wire) {
                            args.insert((*canonical).to_string(), value);
                        }
                    }
                }
            }
            call.name = canonical_name.to_string().into();
            Some(Ok(crate::ChatEvent::ToolCall(call)))
        }
        other => Some(other),
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = self.build_body(request);

        if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
            let body_str = serde_json::to_string_pretty(&body).unwrap_or_default();
            let preview: String = body_str.chars().take(4000).collect();
            tracing::info!(
                "openai_compatible request body: url={}, model={}, body_len={}, body_preview={}",
                url,
                request.model,
                body_str.len(),
                preview
            );
        }

        let tool_count = request.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let tool_preview = request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .take(4)
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "none".to_string());
        let first_tool_arg_shape = body
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|msgs| {
                msgs.iter().find_map(|msg| {
                    msg.get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|tc| tc.get("function"))
                        .and_then(|f| f.get("arguments"))
                        .map(|arg| {
                            if arg.is_string() {
                                "string"
                            } else if arg.is_object() {
                                "object"
                            } else if arg.is_array() {
                                "array"
                            } else if arg.is_null() {
                                "null"
                            } else if arg.is_number() {
                                "number"
                            } else if arg.is_boolean() {
                                "boolean"
                            } else {
                                "unknown"
                            }
                        })
                })
            })
            .unwrap_or("none");
        debug_log!(
            "openai_compatible request debug: model='{}', tool_count={}, tool_preview='{}', first_tool_arg_shape={}",
            request.model,
            tool_count,
            tool_preview,
            first_tool_arg_shape
        );

        let resp = {
            tracing::debug!(
                "OpenAiCompatible({}): sending request to {}, auth_header={}, model={}",
                self.name,
                url,
                self.config.auth_header,
                request.model
            );
            self.request_builder(request, &url, &body)?
                .send()
                .await
                .map_err(ProviderError::from)?
        };

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimit);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            tracing::error!(
                "OpenAiCompatible({}): API error ({}): {}",
                self.name,
                status,
                err
            );
            if std::env::var_os("CODEGG_DIAG_TOOL_PARSE").is_some() {
                let preview: String = err.chars().take(2000).collect();
                tracing::info!("openai_compatible error body: {}", preview);
            }
            return Err(ProviderError::api(
                "http_error",
                format!("API error: {err}"),
            ));
        }

        let stream = resp.bytes_stream();
        let buffer = String::new();
        let provider_name = self.name.clone();
        let adapter = request_policy(&self.id, &request.model);

        tracing::debug!("{}: starting stream processing", provider_name);

        Ok(Box::pin(unfold(
            (stream, buffer),
            move |(mut stream, mut buffer)| {
                let provider_name = provider_name.clone();
                let adapter = adapter.clone();
                async move {
                    loop {
                        if let Some(event) = parse_openai_buffer(&mut buffer) {
                            if let Some(event) = normalize_openai_event(event, &adapter) {
                                return Some((event, (stream, buffer)));
                            }
                            continue;
                        }

                        if buffer.len() > MAX_BUFFER_SIZE {
                            tracing::error!("{}: response buffer exceeded limit", provider_name);
                            return Some((
                                Err(ProviderError::Stream(
                                    "response buffer exceeded limit".to_string(),
                                )),
                                (stream, buffer),
                            ));
                        }

                        // Add a timeout for each chunk to prevent hanging
                        let chunk_result =
                            tokio::time::timeout(Duration::from_secs(30), stream.next()).await;

                        let chunk = match chunk_result {
                            Ok(Some(c)) => c,
                            Ok(None) => {
                                if buffer.is_empty() {
                                    return None;
                                }
                                if let Some(event) = parse_openai_buffer(&mut buffer) {
                                    if let Some(event) = normalize_openai_event(event, &adapter) {
                                        return Some((event, (stream, buffer)));
                                    }
                                }
                                return None;
                            }
                            Err(_) => {
                                tracing::error!("{}: stream chunk timeout", provider_name);
                                return Some((
                                    Err(ProviderError::Stream("stream chunk timeout".to_string())),
                                    (stream, buffer),
                                ));
                            }
                        };

                        match chunk {
                            Ok(bytes) => {
                                let text = String::from_utf8_lossy(&bytes).to_string();
                                tracing::trace!("{}: received chunk: {}", provider_name, text);
                                buffer.push_str(&text);
                            }
                            Err(e) => {
                                tracing::error!("{} stream error: {}", provider_name, e);
                                return Some((
                                    Err(ProviderError::Stream(e.to_string())),
                                    (stream, buffer),
                                ));
                            }
                        }
                    }
                }
            },
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut models = self.config.models.clone();

        let url = format!("{}/models", self.config.base_url);

        let resp = match self
            .client
            .get(&url)
            .header(
                &self.config.auth_header,
                self.config.credential.authorization_header_value(),
            )
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("discovery failed for {}: {}", self.name, e);
                return Ok(models);
            }
        };

        if !resp.status().is_success() {
            return Ok(models);
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(_) => return Ok(models),
        };

        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for entry in data {
                if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                    // Avoid duplicates
                    if !models.iter().any(|m| m.id == id) {
                        models.push(ModelInfo {
                            id: id.to_string(),
                            name: id.to_string(),
                            provider: self.id.clone(),
                            context_window: 128_000,
                            max_output_tokens: None,
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        });
                    }
                }
            }
        }

        Ok(models)
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_types::CredentialKind;
    use crate::{ContentPart, Message, ProviderRequestContext};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Arc};
    use std::thread;

    struct CapturedRequest {
        headers: Vec<(String, String)>,
        body: String,
    }

    fn spawn_capture_server(
        expected_requests: usize,
    ) -> (
        String,
        mpsc::Receiver<CapturedRequest>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let address = listener.local_addr().expect("capture server address");
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("accept provider request");
                let request = read_request(&mut stream);
                tx.send(request).expect("send captured request");
                let response_body =
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                )
                .expect("write capture response headers");
                stream
                    .write_all(response_body)
                    .expect("write capture response body");
            }
        });
        (format!("http://{address}/v1"), rx, handle)
    }

    fn read_request(stream: &mut TcpStream) -> CapturedRequest {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set capture timeout");
        let mut raw = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 4096];
            let count = stream.read(&mut chunk).expect("read provider request");
            assert!(count > 0, "provider closed before sending request");
            raw.extend_from_slice(&chunk[..count]);
            if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header_text = String::from_utf8_lossy(&raw[..header_end]).into_owned();
        let content_length = header_text
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .or_else(|| line.strip_prefix("content-length:"))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while raw.len() - header_end < content_length {
            let mut chunk = [0u8; 4096];
            let count = stream.read(&mut chunk).expect("read provider request body");
            assert!(count > 0, "provider closed before sending request body");
            raw.extend_from_slice(&chunk[..count]);
        }
        let mut headers = Vec::new();
        for line in header_text.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        CapturedRequest {
            headers,
            body: String::from_utf8_lossy(&raw[header_end..header_end + content_length]).into(),
        }
    }

    fn request(session_id: Option<&str>) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            }],
            model: "test-model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: ProviderRequestContext {
                session_id: session_id.map(Arc::from),
            },
        }
    }

    fn header_values<'a>(request: &'a CapturedRequest, name: &str) -> Vec<&'a str> {
        request
            .headers
            .iter()
            .filter(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    #[test]
    fn simple_with_credential_preserves_bearer_kind() {
        let cred = Credential::bearer("short-lived-token", None);
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "openai",
            "OpenAI",
            cred.clone(),
            "https://api.example.test/v1",
        );
        assert_eq!(provider.config.credential.kind, CredentialKind::BearerToken);
        assert_eq!(
            provider.config.credential.authorization_header_value(),
            "Bearer short-lived-token"
        );
    }

    #[test]
    fn simple_with_credential_preserves_api_key_kind() {
        let cred = Credential::api_key("sk-test-1234");
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "openai",
            "OpenAI",
            cred,
            "https://api.example.test/v1",
        );
        assert_eq!(provider.config.credential.kind, CredentialKind::ApiKey);
        assert_eq!(
            provider.config.credential.authorization_header_value(),
            "Bearer sk-test-1234"
        );
    }

    #[test]
    fn simple_wraps_api_key() {
        // Backwards-compat: `simple` should build a Credential::api_key under
        // the hood so existing callers see the same behavior.
        let provider =
            OpenAiCompatibleProvider::simple("xai", "xAI", "sk-x", "https://api.x.ai/v1");
        assert_eq!(provider.config.credential.kind, CredentialKind::ApiKey);
        assert_eq!(provider.config.credential.secret, "sk-x");
    }

    #[tokio::test]
    async fn session_affinity_is_stable_isolated_and_not_in_body() {
        let (base_url, rx, server) = spawn_capture_server(3);
        let provider =
            OpenAiCompatibleProvider::simple("opencode_go", "OpenCode Go", "test-key", &base_url)
                .with_session_affinity_header("x-opencode-session")
                .expect("test session header name is valid");

        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request 1");
        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request 2");
        let _stream = provider
            .stream(&request(Some("S2")))
            .await
            .expect("request 3");

        let captured: Vec<_> = (0..3)
            .map(|_| rx.recv().expect("captured request"))
            .collect();
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured[0], "x-opencode-session"),
            vec!["S1"]
        );
        assert_eq!(
            header_values(&captured[1], "x-opencode-session"),
            vec!["S1"]
        );
        assert_eq!(
            header_values(&captured[2], "x-opencode-session"),
            vec!["S2"]
        );
        assert!(!captured
            .iter()
            .any(|request| request.body.contains("S1") || request.body.contains("S2")));
    }

    #[tokio::test]
    async fn required_session_context_fails_before_network_io() {
        let (base_url, rx, server) = spawn_capture_server(0);
        let provider =
            OpenAiCompatibleProvider::simple("opencode_go", "OpenCode Go", "test-key", &base_url)
                .with_session_affinity_header("x-opencode-session")
                .expect("test session header name is valid");

        let error = match provider.stream(&request(None)).await {
            Ok(_) => panic!("missing context must fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::Api { ref code, .. } if code == "missing_session_context")
        );
        assert!(
            rx.try_recv().is_err(),
            "missing context sent a network request"
        );
        server.join().expect("capture server");
    }

    #[tokio::test]
    async fn session_context_is_not_global_header_leak() {
        let (base_url, rx, server) = spawn_capture_server(1);
        let provider = OpenAiCompatibleProvider::simple("openai", "OpenAI", "test-key", &base_url);
        let _stream = provider
            .stream(&request(Some("S1")))
            .await
            .expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert!(header_values(&captured, "x-opencode-session").is_empty());
    }

    #[tokio::test]
    async fn extra_headers_are_sent_and_reserved_collisions_fail_locally() {
        let (base_url, rx, server) = spawn_capture_server(1);
        let provider = OpenAiCompatibleProvider::new(
            "copilot",
            "Copilot",
            OpenAiCompatibleConfig {
                credential: Credential::api_key("test-key"),
                base_url,
                auth_header: "Authorization".to_string(),
                extra_headers: vec![("Editor-Version".to_string(), "codegg/test".to_string())],
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        );
        let _stream = provider.stream(&request(None)).await.expect("request");
        let captured = rx.recv().expect("captured request");
        server.join().expect("capture server");
        assert_eq!(
            header_values(&captured, "editor-version"),
            vec!["codegg/test"]
        );

        for extra_headers in [
            vec![("authorization".to_string(), "override".to_string())],
            vec![("CONTENT-TYPE".to_string(), "text/plain".to_string())],
            vec![("bad\r\nname".to_string(), "value".to_string())],
            vec![("X-Test".to_string(), "bad\r\nvalue".to_string())],
        ] {
            let provider = OpenAiCompatibleProvider::new(
                "openai",
                "OpenAI",
                OpenAiCompatibleConfig {
                    credential: Credential::api_key("test-key"),
                    base_url: "http://127.0.0.1:1/v1".to_string(),
                    auth_header: "Authorization".to_string(),
                    extra_headers,
                    models: Vec::new(),
                    tool_choice: ToolChoice::Auto,
                },
            );
            let error = match provider.stream(&request(None)).await {
                Ok(_) => panic!("invalid extra header must fail"),
                Err(error) => error,
            };
            assert!(
                matches!(error, ProviderError::Api { ref code, .. } if code == "reserved_header_collision" || code == "invalid_header")
            );
        }

        let provider = OpenAiCompatibleProvider::new(
            "opencode_go",
            "OpenCode Go",
            OpenAiCompatibleConfig {
                credential: Credential::api_key("test-key"),
                base_url: "http://127.0.0.1:1/v1".to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: vec![("X-OPENCODE-SESSION".to_string(), "other".to_string())],
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        )
        .with_session_affinity_header("x-opencode-session")
        .expect("test session header name is valid");
        let error = match provider.stream(&request(Some("S1"))).await {
            Ok(_) => panic!("session header collision must fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::Api { ref code, .. } if code == "reserved_header_collision")
        );
    }
}
