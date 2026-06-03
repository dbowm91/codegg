use crate::error::ProviderError;
use crate::provider::sse_parser::parse_openai_buffer;
use crate::provider::{
    assistant_text_content_value, create_http_client, ChatRequest, ContentPart, EventStream,
    Message, ModelInfo, Provider, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use serde_json::json;

use std::time::Duration;

#[derive(Clone)]
pub struct OpencodeZenProvider {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
}

impl OpencodeZenProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: create_http_client(),
            base_url: "https://opencode.ai/zen/v1".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        for msg in &req.messages {
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
                    let mut assistant_msg = json!({
                        "role": "assistant",
                        "content": assistant_text_content_value(content)
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
                                        "arguments": tc.arguments.to_string(),
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

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": true,
        });

        if let Some(ref tools) = req.tools {
            let tool_defs: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
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

#[async_trait]
impl Provider for OpencodeZenProvider {
    fn id(&self) -> &str {
        "opencode_zen"
    }

    fn name(&self) -> &str {
        "Codegg Zen"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        tracing::debug!("CodeggZen: sending request to {} with model {}", url, req.model);

        let req_builder = client
            .post(&url)
            .header("authorization", format!("Bearer {}", api_key))
            .header("content-type", "application/json");

        let resp = req_builder
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("CodeggZen: request failed: {}", e);
                ProviderError::from(e)
            })?;

        tracing::debug!("CodeggZen: received response with status {}", resp.status());

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimit);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
                tracing::error!("CodeggZen: API error ({}): {}", status, err_text);
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: {}", status, err_text),
            ));
        }

        let stream = resp.bytes_stream();
        let buffer = String::new();

        tracing::debug!("CodeggZen: starting stream processing");

        Ok(Box::pin(unfold(
            (stream, buffer),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(event) = parse_openai_buffer(&mut buffer) {
                        return Some((event, (stream, buffer)));
                    }

                    if buffer.len() > MAX_BUFFER_SIZE {
                            tracing::error!("CodeggZen: buffer overflow");
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
                        Ok(None) => return None,
                        Err(_) => {
                            tracing::error!("CodeggZen: stream chunk timeout");
                            return Some((
                                Err(ProviderError::Stream("stream chunk timeout".to_string())),
                                (stream, buffer),
                            ));
                        }
                    };

                    match chunk {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            tracing::trace!("CodeggZen: received chunk: {}", text);
                            buffer.push_str(&text);
                        }
                        Err(e) => {
                            tracing::error!("CodeggZen: stream error: {}", e);
                            return Some((
                                Err(ProviderError::Stream(e.to_string())),
                                (stream, buffer),
                            ));
                        }
                    }
                }
            },
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "big-pickle".to_string(),
                name: "Big Pickle (Free)".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "minimax-m2.5-free".to_string(),
                name: "MiniMax M2.5 Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "nemotron-3-super-free".to_string(),
                name: "Nemotron 3 Super Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(32_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "qwen3.6-plus-free".to_string(),
                name: "Qwen3.6 Plus Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(32_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
        ])
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let client = self.client.clone();

        let resp = client.get(&url).send().await.map_err(ProviderError::from)?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimit);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: failed to fetch models", status),
            ));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            ProviderError::api(
                "parse_error",
                format!("failed to parse models response: {}", e),
            )
        })?;

        let mut models = Vec::new();

        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for entry in data {
                if let (Some(id), Some(name)) = (
                    entry.get("id").and_then(|v| v.as_str()),
                    entry.get("name").and_then(|v| v.as_str()),
                ) {
                    let context_window = entry
                        .get("context_window")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(128_000) as usize;
                    let max_output = entry
                        .get("max_output_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize);
                    let supports_tools = entry
                        .get("supports_tools")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let supports_vision = entry
                        .get("supports_vision")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    models.push(ModelInfo {
                        id: id.to_string(),
                        name: name.to_string(),
                        provider: "opencode_zen".to_string(),
                        context_window,
                        max_output_tokens: max_output,
                        supports_tools,
                        supports_vision,
                        variants: vec![],
                    });
                }
            }
        }

        if models.is_empty() {
            return self.models().await;
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

    #[test]
    fn test_provider_id() {
        let provider = CodeggZenProvider::new("test-key".to_string());
        assert_eq!(provider.id(), "opencode_zen");
    }

    #[test]
    fn test_provider_name() {
        let provider = CodeggZenProvider::new("test-key".to_string());
        assert_eq!(provider.name(), "Codegg Zen");
    }

    #[test]
    fn test_with_base_url() {
        let provider = CodeggZenProvider::new("test-key".to_string())
            .with_base_url("https://custom.api.com/v1".to_string());
        assert_eq!(provider.base_url, "https://custom.api.com/v1");
    }
}
