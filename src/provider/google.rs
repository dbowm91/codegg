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
pub struct GoogleProvider {
    api_key: String,
    client: reqwest::Client,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: create_http_client(),
        }
    }

    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut contents: Vec<serde_json::Value> = Vec::new();

        for msg in &req.messages {
            match msg {
                Message::System { content } => {
                    if contents.is_empty() {
                        contents.push(json!({
                            "role": "user",
                            "parts": [{"text": content}]
                        }));
                        contents.push(json!({
                            "role": "model",
                            "parts": [{"text": "OK"}]
                        }));
                    }
                }
                Message::User { content } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => {
                                json!({"text": text})
                            }
                            ContentPart::Image { image_url } => {
                                let (mime, data) = parse_image_data(&image_url.url);
                                json!({
                                    "inline_data": {
                                        "mime_type": mime,
                                        "data": data,
                                    }
                                })
                            }
                        })
                        .collect();
                    contents.push(json!({"role": "user", "parts": parts}));
                }
                Message::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut parts: Vec<serde_json::Value> = content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => {
                                if !text.is_empty() {
                                    Some(json!({"text": text}))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        })
                        .collect();

                    for tc in tool_calls {
                        parts.push(json!({
                            "functionCall": {
                                "id": tc.id,
                                "name": tc.name,
                                "args": tc.arguments,
                            }
                        }));
                    }

                    if !parts.is_empty() {
                        contents.push(json!({"role": "model", "parts": parts}));
                    }
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    let parsed: serde_json::Value =
                        serde_json::from_str(content).unwrap_or(json!({"result": content}));
                    let existing = contents
                        .iter_mut()
                        .rev()
                        .find(|c| c.get("role").and_then(|r| r.as_str()) == Some("function"));
                    if let Some(existing) = existing {
                        if let Some(parts) =
                            existing.get_mut("parts").and_then(|p| p.as_array_mut())
                        {
                            parts.push(json!({
                                "functionResponse": {
                                    "name": tool_call_id,
                                    "response": {"name": tool_call_id, "content": parsed},
                                }
                            }));
                        }
                    } else {
                        contents.push(json!({
                            "role": "function",
                            "parts": [{
                                "functionResponse": {
                                    "name": tool_call_id,
                                    "response": {"name": tool_call_id, "content": parsed},
                                }
                            }]
                        }));
                    }
                }
            }
        }

        let mut body = json!({
            "contents": contents,
        });

        if let Some(ref tools) = req.tools {
            let tool_defs: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    let mut params = t.parameters.clone();
                    if params.get("type").is_none() {
                        params = json!({"type": "OBJECT", "properties": params});
                    }
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": params,
                    })
                })
                .collect();
            body["tools"] = json!([{"function_declarations": tool_defs}]);
        }

        let mut generation_config = serde_json::Map::new();

        if let Some(temp) = req.temperature {
            generation_config.insert("temperature".to_string(), json!(temp));
        }

        if let Some(top_p) = req.top_p {
            generation_config.insert("topP".to_string(), json!(top_p));
        }

        if let Some(max) = req.max_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max));
        }

        if !generation_config.is_empty() {
            body["generationConfig"] = json!(generation_config);
        }

        body
    }
}

fn parse_image_data(url: &str) -> (String, String) {
    if let Some(data) = url.strip_prefix("data:") {
        if let Some((mime, rest)) = data.split_once(";base64,") {
            return (mime.to_string(), rest.to_string());
        }
    }
    ("image/png".to_string(), url.to_string())
}

#[async_trait]
impl Provider for GoogleProvider {
    fn id(&self) -> &str {
        "google"
    }

    fn name(&self) -> &str {
        "Google"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let model = req.model.clone();
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            model
        );

        let resp = client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-goog-api-key", &api_key)
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
                    if let Some(event) = parse_google_buffer(&mut buffer) {
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
                id: "gemini-2.5-pro".to_string(),
                name: "Gemini 2.5 Pro".to_string(),
                provider: "google".to_string(),
                context_window: 1_000_000,
                max_output_tokens: Some(65_536),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gemini-2.5-flash".to_string(),
                name: "Gemini 2.5 Flash".to_string(),
                provider: "google".to_string(),
                context_window: 1_000_000,
                max_output_tokens: Some(65_536),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gemini-2.0-flash".to_string(),
                name: "Gemini 2.0 Flash".to_string(),
                provider: "google".to_string(),
                context_window: 1_000_000,
                max_output_tokens: Some(8_192),
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

fn parse_google_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    while let Some(idx) = buffer.find('\n') {
        let line = buffer[..idx].trim().to_string();
        *buffer = buffer[idx + 1..].to_string();

        if line.is_empty() {
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(event) = parse_google_chunk(&val) {
                    return Some(event);
                }
            }
        }
    }
    None
}

fn parse_google_chunk(val: &serde_json::Value) -> Option<Result<ChatEvent, ProviderError>> {
    if let Some(error) = val.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string();
        return Some(Err(msg.into()));
    }

    if let Some(candidates) = val.get("candidates").and_then(|c| c.as_array()) {
        for candidate in candidates {
            if let Some(content) = candidate.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                return Some(Ok(ChatEvent::TextDelta(text.to_string().into())));
                            }
                        }

                        if let Some(exec) = part.get("functionCall") {
                            let name = exec
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args = exec.get("args").cloned().unwrap_or(json!({}));
                            let id = exec
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                            return Some(Ok(ChatEvent::ToolCall(ToolCall {
                                id: id.into(),
                                name: name.into(),
                                arguments: args,
                            })));
                        }

                        if let Some(thought) = part.get("thought").and_then(|t| t.as_bool()) {
                            if thought {
                                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                    return Some(Ok(ChatEvent::ReasoningDelta(
                                        text.to_string().into(),
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            if let Some(finish_reason) = candidate.get("finishReason").and_then(|v| v.as_str()) {
                let mut usage = TokenUsage::default();
                if let Some(u) = val.get("usageMetadata") {
                    if let Some(prompt) = u.get("promptTokenCount").and_then(|v| v.as_u64()) {
                        usage.input_tokens = prompt as usize;
                    }
                    if let Some(completion) = u.get("candidatesTokenCount").and_then(|v| v.as_u64())
                    {
                        usage.output_tokens = completion as usize;
                    }
                    if let Some(total) = u.get("totalTokenCount").and_then(|v| v.as_u64()) {
                        usage.total_tokens = total as usize;
                    } else {
                        usage.total_tokens = usage.input_tokens + usage.output_tokens;
                    }
                }
                return Some(Ok(ChatEvent::Finish {
                    stop_reason: finish_reason.to_string().into(),
                    usage,
                }));
            }
        }
    }

    None
}
