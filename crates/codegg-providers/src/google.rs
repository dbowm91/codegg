use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Clone)]
pub struct GoogleProvider {
    api_key: String,
    client: eggfetch_core::Client,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: create_http_client(),
        }
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
        crate::wire::encode(
            &canonical,
            eggpool_wire::profile::WireSurface::GeminiGenerateContent,
            false,
        )
    }
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
        let body = self.try_build_body(req)?;
        let model = req.model.clone();
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            model
        );

        let mut resp = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("content-type", "application/json")
            .header("x-goog-api-key", &api_key)
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
            eggpool_wire::codec::StreamAdapterKind::GeminiGenerateContentSse,
            None,
            req.context.wire_policy.clone(),
        ))
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
