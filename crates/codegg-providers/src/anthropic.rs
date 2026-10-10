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
        // Shared codec helper. This provider owns its base URL, the
        // `anthropic-version` header, and transport; the Messages request
        // grammar itself belongs to the shared kernel so providers that merely
        // resolve models to this surface reuse one encoding.
        crate::wire::encode_anthropic_messages(req)
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

    /// Discover models upstream, additively over any operator-declared models.
    ///
    /// For `anthropic` the endpoint comes from the shared profile. This
    /// provider is also reused for Anthropic-compatible gateways under their own
    /// ids (MiniMax, for example), and the profile's path for such a provider is
    /// expressed against a different base than the one this provider talks to.
    /// Rather than concatenate a mismatched path, a provider with no reviewed
    /// discovery contract falls back to its operator-declared seeds — which is
    /// exactly the escape hatch for a gateway that serves no catalog.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut models = self.models_override.clone().unwrap_or_default();
        let endpoint = match crate::provider_profile::resolved_models_endpoint(self.id()) {
            Ok(endpoint) => endpoint,
            Err(crate::provider_profile::ProfileError::ProviderNotAdapted { .. }) => {
                tracing::debug!(
                    "no reviewed discovery contract for {}; using declared models only",
                    self.id()
                );
                return Ok(models);
            }
            Err(error) => {
                return Err(ProviderError::api(
                    "provider_profile_contract",
                    format!("shared provider profile could not resolve discovery: {error}"),
                ));
            }
        };

        let options = crate::eggpool::EggpoolProbeOptions::default();
        let base = self.base_url.trim_end_matches('/');
        let url = format!(
            "{base}{}{}",
            endpoint.path,
            crate::openai_compatible::encode_query(&endpoint.query)
        );

        let mut response = match self
            .client
            .get(&url)
            .map_err(ProviderError::from)?
            .timeout(crate::provider_core::non_streaming_timeout())
            .max_decoded_body_size(options.response_byte_limit)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("anthropic discovery failed: {}", error);
                return crate::openai_compatible::discovery_failed(
                    self.id(),
                    endpoint.required,
                    models,
                );
            }
        };

        if !response.status().is_success() {
            tracing::warn!("anthropic discovery returned HTTP {}", response.status());
            return crate::openai_compatible::discovery_failed(
                self.id(),
                endpoint.required,
                models,
            );
        }

        if response
            .content_length()
            .is_some_and(|length| length > options.response_byte_limit as u64)
        {
            return crate::openai_compatible::discovery_failed(
                self.id(),
                endpoint.required,
                models,
            );
        }

        let body = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => {
                return crate::openai_compatible::discovery_failed(
                    self.id(),
                    endpoint.required,
                    models,
                )
            }
        };

        // Anthropic's `/models` payload is the same bounded `data` array the
        // shared compatible parser already accepts, so reuse it rather than
        // adding a second parser. Capability and limit facts are taken only
        // when the upstream entry states them; otherwise `false`/`0` mean "not
        // advertised" rather than an invented capability.
        let summaries = match crate::eggpool::parse_compatible_models_response(&body, &options) {
            Ok(summaries) => summaries,
            Err(_) => {
                return crate::openai_compatible::discovery_failed(
                    self.id(),
                    endpoint.required,
                    models,
                )
            }
        };

        for summary in summaries {
            if !models.iter().any(|existing| existing.id == summary.id) {
                models.push(ModelInfo {
                    id: summary.id,
                    name: summary.name,
                    provider: self.id().to_string(),
                    context_window: 0,
                    max_output_tokens: None,
                    supports_tools: false,
                    supports_vision: false,
                    variants: Vec::new(),
                });
            }
        }

        Ok(models)
    }
}
