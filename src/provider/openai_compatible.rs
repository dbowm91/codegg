use crate::error::ProviderError;
use crate::provider::sse_parser::parse_openai_buffer;
use crate::provider::{
    assistant_text_content_value, create_http_client, openai_tool_arguments_value, ChatRequest,
    ContentPart, EventStream, Message, ModelInfo, Provider, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use serde_json::json;

use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Specific(String),
}

#[derive(Clone)]
pub struct OpenAiCompatibleConfig {
    pub api_key: String,
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
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(id: &str, name: &str, config: OpenAiCompatibleConfig) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            config,
            client: create_http_client(),
        }
    }

    pub fn simple(id: &str, name: &str, api_key: &str, base_url: &str) -> Self {
        Self::new(
            id,
            name,
            OpenAiCompatibleConfig {
                api_key: api_key.to_string(),
                base_url: base_url.to_string(),
                auth_header: "Authorization".to_string(),
                extra_headers: Vec::new(),
                models: Vec::new(),
                tool_choice: ToolChoice::Auto,
            },
        )
    }

    pub fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();
        for msg in &request.messages {
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
                        if text.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::String(text)
                        }
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
                                        "name": tc.name,
                                        "arguments": openai_tool_arguments_value(&tc.arguments),
                                    }
                                })
                            })
                            .collect();
                        assistant_msg["tool_calls"] = serde_json::json!(tool_calls_json);
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
            tools.iter().map(|t| t.to_openai()).collect::<Vec<_>>()
        });

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "tools": tools_json,
        });
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
                        "function": {"name": name}
                    });
                }
            }
        }

        body
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
            let key_len = self.config.api_key.len();
            let key_prefix = if key_len > 4 {
                &self.config.api_key[..4]
            } else {
                "short"
            };
            let key_suffix = if key_len > 4 {
                &self.config.api_key[key_len - 4..]
            } else {
                ""
            };
            tracing::debug!(
                "OpenAiCompatible({}): sending request to {}, auth_header={}, key_len={}, key_prefix={}...{}, model={}",
                self.name,
                url,
                self.config.auth_header,
                key_len,
                key_prefix,
                key_suffix,
                request.model
            );
            self.client
                .post(&url)
                .header(
                    &self.config.auth_header,
                    format!("Bearer {}", self.config.api_key),
                )
                .header("Content-Type", "application/json")
                .json(&body)
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

        tracing::debug!("{}: starting stream processing", provider_name);

        Ok(Box::pin(unfold(
            (stream, buffer),
            move |(mut stream, mut buffer)| {
                let provider_name = provider_name.clone();
                async move {
                    loop {
                        if let Some(event) = parse_openai_buffer(&mut buffer) {
                            return Some((event, (stream, buffer)));
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
                                    return Some((event, (stream, buffer)));
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
                format!("Bearer {}", self.config.api_key),
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
