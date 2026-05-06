use crate::error::ProviderError;
use crate::provider::{
    create_http_client, ChatEvent, ChatRequest, ContentPart, EventStream, Message, ModelInfo,
    Provider, TokenUsage, ToolCall, MAX_BUFFER_SIZE,
};
use async_trait::async_trait;
use futures::stream::unfold;
use futures::StreamExt;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct BedrockProvider {
    client: reqwest::Client,
    region: String,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
}

impl BedrockProvider {
    pub fn new(region: String, access_key: String, secret_key: String) -> Self {
        Self {
            client: create_http_client(),
            region,
            access_key,
            secret_key,
            session_token: None,
        }
    }

    pub fn with_session_token(mut self, token: String) -> Self {
        self.session_token = Some(token);
        self
    }

    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        for msg in &req.messages {
            match msg {
                Message::System { content: _ } => {}
                Message::User { content } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => {
                                json!({"text": text})
                            }
                            ContentPart::Image { image_url } => {
                                let (format, bytes) = parse_image_for_bedrock(&image_url.url);
                                json!({
                                    "image": {
                                        "format": format,
                                        "source": {"bytes": bytes}
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

                    if !tool_calls.is_empty() {
                        for tc in tool_calls {
                            parts.push(json!({
                                "toolUse": {
                                    "toolUseId": tc.id,
                                    "name": tc.name,
                                    "input": tc.arguments,
                                }
                            }));
                        }
                    }

                    if !parts.is_empty() {
                        messages.push(json!({"role": "assistant", "content": parts}));
                    }
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "toolResult": {
                                "toolUseId": tool_call_id,
                                "content": [{"text": content}],
                            }
                        }]
                    }));
                }
            }
        }

        let mut body = json!({
            "messages": messages,
        });

        if let Some(ref system) = req.system {
            body["system"] = json!([{"text": system}]);
        }

        if let Some(ref tools) = req.tools {
            let tool_defs: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "toolSpec": {
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": {"json": t.parameters},
                        }
                    })
                })
                .collect();
            body["toolConfig"] = json!({"tools": tool_defs});
        }

        let mut inference_config = serde_json::Map::new();

        if let Some(temp) = req.temperature {
            inference_config.insert("temperature".to_string(), json!(temp));
        }

        if let Some(top_p) = req.top_p {
            inference_config.insert("topP".to_string(), json!(top_p));
        }

        if let Some(max) = req.max_tokens {
            inference_config.insert("maxTokens".to_string(), json!(max));
        }

        if !inference_config.is_empty() {
            body["inferenceConfig"] = json!(inference_config);
        }

        body
    }

    fn endpoint(&self) -> String {
        format!("https://bedrock-runtime.{}.amazonaws.com", self.region)
    }
}

fn parse_image_for_bedrock(url: &str) -> (String, String) {
    if let Some(data) = url.strip_prefix("data:") {
        if let Some((mime, rest)) = data.split_once(";base64,") {
            let format = match mime {
                "image/jpeg" => "jpeg".to_string(),
                "image/png" => "png".to_string(),
                "image/gif" => "gif".to_string(),
                "image/webp" => "webp".to_string(),
                _ => "png".to_string(),
            };
            return (format, rest.to_string());
        }
    }
    ("png".to_string(), url.to_string())
}

#[async_trait]
impl Provider for BedrockProvider {
    fn id(&self) -> &str {
        "bedrock"
    }

    fn name(&self) -> &str {
        "Amazon Bedrock"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req);
        let body_bytes =
            serde_json::to_vec(&body).map_err(|e| ProviderError::from(e.to_string()))?;
        let model = req.model.clone();
        let endpoint = self.endpoint();
        let client = self.client.clone();
        let region = self.region.clone();
        let secret_key = self.secret_key.clone();
        let access_key = self.access_key.clone();
        let session_token = self.session_token.clone();
        let body_bytes_for_signing = body_bytes.clone();

        let url = format!("{}/model/{}/converse-stream", endpoint, model);

        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date_for_req = amz_date.clone();

        let authorization = tokio::task::spawn_blocking(move || {
            let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, region, "bedrock");

            let payload_hash = hex::encode(Sha256::digest(&body_bytes_for_signing));
            let canonical_uri = format!("/model/{}/converse-stream", model);
            let canonical_query = "";
            let canonical_headers = format!(
                "content-type:application/json\nhost:bedrock-runtime.{}.amazonaws.com\nx-amz-date:{}\n",
                region, amz_date
            );
            let signed_headers = "content-type;host;x-amz-date";
            let canonical_request = format!(
                "{}\n{}\n{}\n{}\n{}\n{}",
                "POST", canonical_uri, canonical_query, canonical_headers, signed_headers, payload_hash
            );

            let string_to_sign = format!(
                "AWS4-HMAC-SHA256\n{}\n{}\n{}",
                amz_date,
                credential_scope,
                hex::encode(Sha256::digest(canonical_request.as_bytes()))
            );

            let signing_key = derive_signing_key(&secret_key, &date_stamp, &region, "bedrock");
            let mut mac = Hmac::<Sha256>::new_from_slice(&signing_key)
                .map_err(|e| ProviderError::from(e.to_string()))?;
            mac.update(string_to_sign.as_bytes());
            let signature = hex::encode(mac.finalize().into_bytes());

Ok::<_, ProviderError>(format!(
                "AWS4-HMAC-SHA256 Credential={}/{}/{}/{}/{}, SignedHeaders={}, Signature={}",
                access_key, date_stamp, region, "bedrock", "aws4_request", signed_headers, signature
            ))
        })
        .await
        .map_err(|e| ProviderError::from(e.to_string()))??;

        let mut req_builder = client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-amz-date", &amz_date_for_req)
            .header("authorization", &authorization)
            .body(body_bytes);

        if let Some(ref token) = session_token {
            req_builder = req_builder.header("x-amz-security-token", token);
        }

        let resp = req_builder.send().await.map_err(ProviderError::from)?;

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
                    if let Some(event) = parse_bedrock_buffer(&mut buffer) {
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
                id: "anthropic.claude-sonnet-4-20250514-v1:0".to_string(),
                name: "Claude Sonnet 4 (Bedrock)".to_string(),
                provider: "bedrock".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
                name: "Claude 3.5 Sonnet (Bedrock)".to_string(),
                provider: "bedrock".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(8_192),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "meta.llama3-1-405b-instruct-v1:0".to_string(),
                name: "Llama 3.1 405B (Bedrock)".to_string(),
                provider: "bedrock".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(4_096),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
        ])
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

fn derive_signing_key(secret_key: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), date_stamp);
    let k_region = hmac_sha256(&k_date, region);
    let k_service = hmac_sha256(&k_region, service);
    hmac_sha256(&k_service, "aws4_request")
}

fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn parse_bedrock_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    while let Some(idx) = buffer.find("\n\n") {
        let chunk = buffer[..idx].to_string();
        *buffer = buffer[idx + 2..].to_string();

        if let Some(event) = parse_bedrock_sse(&chunk) {
            return Some(event);
        }
    }
    None
}

fn parse_bedrock_sse(chunk: &str) -> Option<Result<ChatEvent, ProviderError>> {
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
        "messageStart" => None,
        "contentBlockStart" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                if let Some(start) = val.get("start") {
                    if let Some(tool) = start.get("toolUse") {
                        let id = tool
                            .get("toolUseId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = tool
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let args = tool.get("input").cloned().unwrap_or(json!({}));
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
        "contentBlockDelta" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                if let Some(delta) = val.get("delta") {
                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                        return Some(Ok(ChatEvent::TextDelta(text.to_string().into())));
                    }
                    if let Some(reasoning) = delta.get("reasoningContent") {
                        if let Some(text) = reasoning.get("text").and_then(|t| t.as_str()) {
                            return Some(Ok(ChatEvent::ReasoningDelta(text.to_string().into())));
                        }
                    }
                    if let Some(tool_input) = delta.get("toolUse") {
                        if let Some(input) = tool_input.get("input") {
                            return Some(Ok(ChatEvent::ToolCall(ToolCall {
                                id: String::new().into(),
                                name: String::new().into(),
                                arguments: input.clone(),
                            })));
                        }
                    }
                }
            }
            None
        }
        "contentBlockStop" => None,
        "messageStop" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let stop_reason = val
                    .get("stopReason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("end_turn")
                    .to_string();
                Some(Ok(ChatEvent::Finish {
                    stop_reason: stop_reason.into(),
                    usage: TokenUsage::default(),
                }))
            } else {
                Some(Ok(ChatEvent::Finish {
                    stop_reason: "end_turn".to_string().into(),
                    usage: TokenUsage::default(),
                }))
            }
        }
        "metadata" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let mut usage = TokenUsage::default();
                if let Some(u) = val.get("usage") {
                    if let Some(input) = u.get("inputTokens").and_then(|v| v.as_u64()) {
                        usage.input_tokens = input as usize;
                    }
                    if let Some(output) = u.get("outputTokens").and_then(|v| v.as_u64()) {
                        usage.output_tokens = output as usize;
                    }
                    if let Some(total) = u.get("totalTokens").and_then(|v| v.as_u64()) {
                        usage.total_tokens = total as usize;
                    } else {
                        usage.total_tokens = usage.input_tokens + usage.output_tokens;
                    }
                }
                Some(Ok(ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage,
                }))
            } else {
                None
            }
        }
        "exception" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_str) {
                let msg = val
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("bedrock error")
                    .to_string();
                Some(Err(msg.into()))
            } else {
                Some(Err(ProviderError::api(
                    "bedrock_error",
                    "bedrock streaming error".to_string(),
                )))
            }
        }
        _ => None,
    }
}
