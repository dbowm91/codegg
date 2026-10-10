use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

/// Azure OpenAI data-plane API version, shared by chat and discovery so the
/// two can never drift onto different revisions.
const AZURE_API_VERSION: &str = "2024-10-21";

#[derive(Clone)]
pub struct AzureProvider {
    api_key: String,
    endpoint: String,
    client: eggfetch_core::Client,
}

impl AzureProvider {
    pub fn new(api_key: String, endpoint: String) -> Self {
        Self {
            api_key,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: create_http_client(),
        }
    }

    pub(crate) fn build_body(&self, req: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        let mut body = crate::wire::encode_openai_chat(req, None, true)?;
        body.as_object_mut()
            .map(|object| object.remove("tool_choice"));
        if req.tools.is_none() {
            body.as_object_mut().map(|object| object.remove("tools"));
        }
        crate::wire::omit_openai_fields(&mut body, &["response_format", "reasoning_effort"]);
        body.as_object_mut().map(|object| object.remove("model"));
        Ok(body)
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
        let body = self.build_body(req)?;
        let model = req.model.clone();
        let api_key = self.api_key.clone();
        let endpoint = self.endpoint.clone();
        let client = self.client.clone();

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={AZURE_API_VERSION}",
            endpoint, model
        );

        let mut resp = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("api-key", &api_key)
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
        Ok(crate::wire::openai_chat_stream(
            stream,
            req.context.wire_policy.clone(),
            None,
        ))
    }

    /// Best-effort discovery against the Azure OpenAI data-plane models
    /// endpoint.
    ///
    /// Azure scopes access to *deployments* the operator created rather than
    /// exposing a stable catalog, so this frequently yields nothing. That is
    /// the truthful result and is returned as an empty catalog; there is no
    /// compiled-in list to fall back to.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let options = crate::eggpool::EggpoolProbeOptions::default();
        let url = format!(
            "{}/openai/models?api-version={}",
            self.endpoint, AZURE_API_VERSION
        );

        let mut response = match self
            .client
            .get(&url)
            .map_err(ProviderError::from)?
            .timeout(crate::provider_core::non_streaming_timeout())
            .max_decoded_body_size(options.response_byte_limit)
            .header("api-key", &self.api_key)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("azure discovery failed: {}", error);
                return Ok(Vec::new());
            }
        };

        if !response.status().is_success() {
            tracing::warn!("azure discovery returned HTTP {}", response.status());
            return Ok(Vec::new());
        }
        if response
            .content_length()
            .is_some_and(|length| length > options.response_byte_limit as u64)
        {
            return Ok(Vec::new());
        }

        let body = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Vec::new()),
        };

        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
            return Ok(Vec::new());
        };
        let Some(entries) = value.get("data").and_then(|m| m.as_array()) else {
            return Ok(Vec::new());
        };
        if entries.len() > options.model_count_limit {
            return Ok(Vec::new());
        }

        let mut models = Vec::with_capacity(entries.len());
        for entry in entries {
            let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if id.is_empty() || id.chars().count() > options.model_string_limit {
                continue;
            }
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or(id);
            // Azure does not advertise context or capability facts here, so
            // they mean "unknown", never an invented limit.
            models.push(ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
                provider: self.id().to_string(),
                context_window: 0,
                max_output_tokens: None,
                supports_tools: false,
                supports_vision: false,
                variants: Vec::new(),
            });
        }

        Ok(models)
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}
