use crate::error::ProviderError;
use crate::provider::{
    create_http_client, ChatEvent, ChatRequest, ContentPart, EventStream, Message, ModelInfo,
    Provider, TokenUsage, ToolCall, MAX_BUFFER_SIZE,
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
            .header("anthropic-beta", "prompt-caching-2024-07-31")
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

fn parse_anthropic_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    parse_anthropic_buffer_with_state(buffer, &mut None, &mut String::new())
}

fn parse_anthropic_buffer_with_state(
    buffer: &mut String,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
) -> Option<Result<ChatEvent, ProviderError>> {
    let mut pending_tool_calls: std::collections::VecDeque<ToolCall> =
        std::collections::VecDeque::new();

    while let Some(idx) = buffer.find("\n\n") {
        let chunk = buffer[..idx].to_string();
        *buffer = buffer[idx + 2..].to_string();

        if let Some(event) = parse_anthropic_sse_with_state(
            &chunk,
            current_tool,
            args_buffer,
            &mut pending_tool_calls,
        ) {
            return Some(event);
        }
    }

    if let Some(tc) = pending_tool_calls.pop_front() {
        return Some(Ok(ChatEvent::ToolCall(tc)));
    }
    None
}

#[allow(dead_code)]
fn parse_anthropic_sse(chunk: &str) -> Option<Result<ChatEvent, ProviderError>> {
    let mut current_tool: Option<(String, String, String)> = None;
    let mut args_buffer = String::new();
    let mut pending_tool_calls: std::collections::VecDeque<ToolCall> =
        std::collections::VecDeque::new();
    parse_anthropic_sse_with_state(
        chunk,
        &mut current_tool,
        &mut args_buffer,
        &mut pending_tool_calls,
    )
}

fn parse_anthropic_sse_with_state(
    chunk: &str,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
    _pending_tool_calls: &mut std::collections::VecDeque<ToolCall>,
) -> Option<Result<ChatEvent, ProviderError>> {
    let mut event_type: Option<String> = None;
    let mut data: Option<String> = None;

    for line in chunk.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            event_type = Some(ev.to_string());
        } else if let Some(d) = line.strip_prefix("data: ") {
            data = Some(d.to_string());
        }
    }

    let event_type = event_type?;
    let data_str = data?;

    match event_type.as_str() {
        "message_start" => None,
        "content_block_start" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let block = val.get("content_block")?;
                let btype = block.get("type")?.as_str()?;
                match btype {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                return Some(Ok(ChatEvent::TextDelta(text.to_string().into())));
                            }
                        }
                        None
                    }
                    "tool_use" => {
                        let id = block.get("id")?.as_str()?.to_string();
                        let name = block.get("name")?.as_str()?.to_string();
                        *current_tool = Some((id.clone(), name.clone(), id.clone()));
                        args_buffer.clear();
                        let args = block.get("input").cloned().unwrap_or(json!({}));
                        if args == json!({}) {
                            None
                        } else {
                            Some(Ok(ChatEvent::ToolCall(ToolCall {
                                id: id.into(),
                                name: name.into(),
                                arguments: args,
                            })))
                        }
                    }
                    "thinking" => None,
                    _ => None,
                }
            } else {
                None
            }
        }
        "content_block_delta" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let delta = val.get("delta")?;
                let dtype = delta.get("type")?.as_str()?;
                match dtype {
                    "text_delta" => {
                        let text = delta.get("text")?.as_str()?.to_string();
                        Some(Ok(ChatEvent::TextDelta(text.into())))
                    }
                    "thinking_delta" => {
                        let thinking = delta.get("thinking")?.as_str()?.to_string();
                        Some(Ok(ChatEvent::ReasoningDelta(thinking.into())))
                    }
                    "input_json_delta" => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if partial.is_empty() {
                            None
                        } else {
                            args_buffer.push_str(&partial);
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        "content_block_stop" => {
            if let Some((id, name, _)) = current_tool.take() {
                let args_str = std::mem::take(args_buffer);
                if !args_str.is_empty() {
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&args_str) {
                        return Some(Ok(ChatEvent::ToolCall(ToolCall {
                            id: id.into(),
                            name: name.into(),
                            arguments: args,
                        })));
                    }
                }
            }
            None
        }
        "message_delta" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let mut stop_reason = "end_turn".to_string();
                let mut input_tokens = 0;
                let mut output_tokens = 0;

                if let Some(delta) = val.get("delta") {
                    if let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                        stop_reason = sr.to_string();
                    }
                }

                if let Some(usage) = val.get("usage") {
                    if let Some(it) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                        input_tokens = it as usize;
                    }
                    if let Some(ot) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                        output_tokens = ot as usize;
                    }
                }

                Some(Ok(ChatEvent::Finish {
                    stop_reason: stop_reason.into(),
                    usage: TokenUsage {
                        input_tokens,
                        output_tokens,
                        total_tokens: input_tokens + output_tokens,
                        reasoning_tokens: 0,
                    },
                }))
            } else {
                None
            }
        }
        "message_stop" => None,
        "ping" => None,
        "error" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let msg = val
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                Some(Err(msg.into()))
            } else {
                Some(Err(ProviderError::api("parse_error", "unknown error")))
            }
        }
        _ => None,
    }
}
