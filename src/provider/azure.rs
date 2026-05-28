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

#[derive(Clone)]
pub struct AzureProvider {
    api_key: String,
    endpoint: String,
    client: reqwest::Client,
}

impl AzureProvider {
    pub fn new(api_key: String, endpoint: String) -> Self {
        Self {
            api_key,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: create_http_client(),
        }
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

        body["stream_options"] = json!({"include_usage": true});

        body
    }
}

#[async_trait]
impl Provider for AzureProvider {
    fn id(&self) -> &str {
        "azure"
    }

    fn name(&self) -> &str {
        "Azure OpenAI"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let model = req.model.clone();
        let api_key = self.api_key.clone();
        let endpoint = self.endpoint.clone();
        let client = self.client.clone();

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version=2024-10-21",
            endpoint, model
        );

        let resp = client
            .post(&url)
            .header("api-key", &api_key)
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
                    if let Some(event) = parse_openai_buffer(&mut buffer) {
                        return Some((event, (stream, buffer)));
                    }

                    match stream.next().await {
                        None => {
                            if buffer.is_empty() {
                                return None;
                            }
                            if let Some(event) = parse_openai_buffer(&mut buffer) {
                                return Some((event, (stream, buffer)));
                            }
                            return None;
                        }
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
                    }
                }
            },
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4.1".to_string(),
                name: "GPT-4.1".to_string(),
                provider: "azure".to_string(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "azure".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(16_384),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
        ])
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}
