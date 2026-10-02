use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Clone)]
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: eggfetch_core::Client,
    id_override: Option<String>,
    name_override: Option<String>,
    models_override: Option<Vec<ModelInfo>>,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: crate::setup_catalog::ANTHROPIC_BASE_URL.to_string(),
            client: create_http_client(),
            id_override: None,
            name_override: None,
            models_override: None,
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_id(mut self, id: String) -> Self {
        self.id_override = Some(id);
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name_override = Some(name);
        self
    }

    pub fn with_models(mut self, models: Vec<ModelInfo>) -> Self {
        self.models_override = Some(models);
        self
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        self.try_build_body(req)
            .unwrap_or_else(|_| serde_json::Value::Null)
    }

    fn try_build_body(&self, req: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        let mut canonical = crate::wire::canonical_request(req, None);
        if let Some(system) = req.system.as_deref().filter(|_| {
            !canonical
                .messages
                .iter()
                .any(|message| message.role == eggpool_wire::ir::CanonicalRole::System)
        }) {
            canonical
                .messages
                .insert(0, crate::wire::system_message(system));
        }
        let mut body = crate::wire::encode(
            &canonical,
            eggpool_wire::profile::WireSurface::AnthropicMessages,
            false,
        )?;
        // CodeGG's established Messages contract always represents message
        // content as typed blocks, even when the shared codec can compact text.
        if let Some(messages) = body
            .get_mut("messages")
            .and_then(serde_json::Value::as_array_mut)
        {
            for message in messages {
                if let Some(text) = message.get("content").and_then(serde_json::Value::as_str) {
                    message["content"] = serde_json::json!([{"type":"text", "text":text}]);
                }
            }
        }
        Ok(body)
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        self.id_override.as_deref().unwrap_or("anthropic")
    }

    fn name(&self) -> &str {
        self.name_override.as_deref().unwrap_or("Anthropic")
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.try_build_body(req)?;
        let url = format!("{}/v1/messages", self.base_url);
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let mut resp = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .map_err(|e| ProviderError::api("serialization", e.to_string()))?
            .send()
            .await
            .map_err(ProviderError::from)?;

        if resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(resp.headers()));
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

        let stream = resp.bytes_stream().map_err(ProviderError::from)?;
        Ok(crate::wire::shared_stream(
            stream,
            eggpool_wire::codec::StreamAdapterKind::AnthropicMessagesSse,
            None,
            req.context.wire_policy.clone(),
        ))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        if let Some(models) = &self.models_override {
            return Ok(models.clone());
        }
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
