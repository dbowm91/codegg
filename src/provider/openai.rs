use crate::error::ProviderError;
use crate::provider::sse_parser::parse_openai_buffer;
use crate::provider::{
    create_http_client, ChatRequest, ContentPart, EventStream, Message, ModelInfo, Provider,
    ResponseFormat, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub base_url: String,
    pub provider_id: String,
    pub provider_name: String,
    pub requires_org_header: bool,
    pub organization: Option<String>,
    pub omit_stream_options: bool,
    pub tool_choice_auto: bool,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com".to_string(),
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: false,
            organization: None,
            omit_stream_options: false,
            tool_choice_auto: false,
        }
    }
}

impl OpenAiConfig {
    pub fn default_with_key(api_key: String) -> Self {
        Self {
            api_key,
            ..Default::default()
        }
    }

    pub fn openai(api_key: String) -> Self {
        Self {
            api_key,
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: true,
            ..Default::default()
        }
    }

    pub fn groq(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            provider_id: "groq".to_string(),
            provider_name: "Groq".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }

    pub fn xai(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
            provider_id: "xai".to_string(),
            provider_name: "xAI".to_string(),
            ..Default::default()
        }
    }

    pub fn mistral(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.mistral.ai".to_string(),
            provider_id: "mistral".to_string(),
            provider_name: "Mistral".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }

    pub fn cerebras(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.cerebras.ai".to_string(),
            provider_id: "cerebras".to_string(),
            provider_name: "Cerebras".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct OpenAiProvider {
    cfg: OpenAiConfig,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(cfg: OpenAiConfig) -> Self {
        Self {
            cfg,
            client: create_http_client(),
        }
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
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
                    let text_parts: Vec<String> = content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.to_string()),
                            _ => None,
                        })
                        .collect();
                    let text = text_parts.join("");

                    let mut assistant_msg = json!({
                        "role": "assistant",
                        "content": if text.is_empty() {
                            serde_json::Value::Null
                        } else {
                            json!(text)
                        }
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
            let tool_defs: Vec<serde_json::Value> = tools.iter().map(|t| t.to_openai()).collect();
            body["tools"] = serde_json::json!(tool_defs);
            if self.cfg.tool_choice_auto {
                body["tool_choice"] = serde_json::json!("auto");
            }
        }

        if let Some(temp) = req.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(top_p) = req.top_p {
            body["top_p"] = serde_json::json!(top_p);
        }

        if let Some(max) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }

        if !self.cfg.omit_stream_options {
            body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        if let Some(ref format) = req.response_format {
            match format {
                ResponseFormat::JsonObject => {
                    body["response_format"] = json!({"type": "json_object"});
                }
                ResponseFormat::JsonSchema {
                    name,
                    schema,
                    strict,
                } => {
                    body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": {
                            "name": name,
                            "schema": schema,
                            "strict": strict,
                        }
                    });
                }
            }
        }

        body
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.cfg.provider_id
    }

    fn name(&self) -> &str {
        &self.cfg.provider_name
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let url = format!("{}/v1/chat/completions", self.cfg.base_url);
        let api_key = self.cfg.api_key.clone();
        let client = self.client.clone();
        let requires_org = self.cfg.requires_org_header;
        let org = self.cfg.organization.clone();

        let mut req_builder = client
            .post(&url)
            .header("authorization", format!("Bearer {}", api_key))
            .header("content-type", "application/json");

        if requires_org {
            if let Some(ref org_id) = org {
                req_builder = req_builder.header("OpenAI-Organization", org_id);
            }
        }

        let resp = req_builder
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

                    let chunk = stream.next().await?;
                    match chunk {
                        Ok(bytes) => {
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
                        Err(e) => {
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
                provider: self.cfg.provider_id.clone(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4.1-mini".to_string(),
                name: "GPT-4.1 Mini".to_string(),
                provider: self.cfg.provider_id.clone(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: self.cfg.provider_id.clone(),
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
