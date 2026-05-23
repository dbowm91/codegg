use crate::error::ProviderError;
use crate::provider::sse_parser::parse_anthropic_buffer;
use crate::provider::{
    create_http_client, ChatRequest, ContentPart, EventStream, Message, ModelInfo,
    Provider, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use serde_json::json;

#[derive(Clone)]
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.anthropic.com".to_string(),
            client: create_http_client(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        for msg in &req.messages {
            match msg {
                Message::System { content: _ } => {}
                Message::User { content } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => {
                                json!({"type": "text", "text": text})
                            }
                            ContentPart::Image { image_url } => {
                                let (media_type, data) = parse_image_url(&image_url.url);
                                json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": data,
                                    }
                                })
                            }
                        })
                        .collect();
                    messages.push(json!({"role": "user", "content": parts}));
                }
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => {
                                json!({"type": "text", "text": text})
                            }
                            ContentPart::Image { image_url } => {
                                let (media_type, data) = parse_image_url(&image_url.url);
                                json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": data,
                                    }
                                })
                            }
                        })
                        .collect();

                    for tc in tool_calls {
                        parts.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments,
                        }));
                    }

                    messages.push(json!({"role": "assistant", "content": parts}));
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content,
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
        });

        if let Some(ref system) = req.system {
            body["system"] = json!([{"type": "text", "text": system}]);
        }

        if let Some(ref tools) = req.tools {
            let tool_defs: Vec<serde_json::Value> =
                tools.iter().map(|t| t.to_anthropic()).collect();
            body["tools"] = json!(tool_defs);
        }

        if let Some(temp) = req.temperature {
            body["temperature"] = json!(temp);
        }

        if let Some(top_p) = req.top_p {
            body["top_p"] = json!(top_p);
        }

        if let Some(max) = req.max_tokens {
            body["max_tokens"] = json!(max);
        }

        body
    }
}

fn parse_image_url(url: &str) -> (String, String) {
    if let Some(data) = url.strip_prefix("data:") {
        if let Some((media, rest)) = data.split_once(";base64,") {
            return (media.to_string(), rest.to_string());
        }
    }
    ("image/png".to_string(), url.to_string())
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn name(&self) -> &str {
        "Anthropic"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let url = format!("{}/v1/messages", self.base_url);
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let resp = client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(ProviderError::from)?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimit);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: {}", status, err_text),
            ));
        }

        let stream = resp.bytes_stream();
        let buffer = String::new();

        Ok(Box::pin(unfold(
            (stream, buffer),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(event) = parse_anthropic_buffer(&mut buffer) {
                        return Some((event, (stream, buffer)));
                    }

                    let chunk = stream.next().await;
                    match chunk {
                        Some(Ok(bytes)) => {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            buffer.push_str(&text);
                            if buffer.len() > MAX_BUFFER_SIZE {
                                return Some((
                                    Err(ProviderError::Stream(
                                        "response buffer exceeded limit".to_string(),
                                    )),
                                    (stream, buffer),
                                ));
                            }
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(ProviderError::Stream(e.to_string())),
                                (stream, buffer),
                            ));
                        }
                        None => {
                            if buffer.is_empty() {
                                return None;
                            }
                            if let Some(event) = parse_anthropic_buffer(&mut buffer) {
                                return Some((event, (stream, buffer)));
                            }
                            return None;
                        }
                    }
                }
            },
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "claude-sonnet-4-20250514".to_string(),
                name: "Claude Sonnet 4".to_string(),
                provider: "anthropic".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "claude-opus-4-20250514".to_string(),
                name: "Claude Opus 4".to_string(),
                provider: "anthropic".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(32_000),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "claude-3-5-sonnet-20241022".to_string(),
                name: "Claude 3.5 Sonnet".to_string(),
                provider: "anthropic".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(8_192),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "claude-3-5-haiku-20241022".to_string(),
                name: "Claude 3.5 Haiku".to_string(),
                provider: "anthropic".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(8_192),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
        ])
    }
}
