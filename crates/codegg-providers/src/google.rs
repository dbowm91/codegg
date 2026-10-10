use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

/// Native Gemini API origin. The discovery **path** is owned by the shared
/// provider profile; this is only the origin the provider already talks to.
const GEMINI_NATIVE_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

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
            "{GEMINI_NATIVE_BASE}/models/{}:streamGenerateContent?alt=sse",
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

    /// Discover models from the native Gemini `/models` endpoint.
    ///
    /// The path comes from the shared provider profile rather than a CodeGG
    /// local constant. There is no compiled-in catalog: an unreachable or empty
    /// response yields an empty catalog rather than fiction.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let endpoint =
            crate::provider_profile::resolved_models_endpoint(self.id()).map_err(|error| {
                ProviderError::api(
                    "provider_profile_contract",
                    format!("shared provider profile could not resolve discovery: {error}"),
                )
            })?;
        let options = crate::eggpool::EggpoolProbeOptions::default();

        let url = format!(
            "{GEMINI_NATIVE_BASE}{}{}",
            endpoint.path,
            crate::openai_compatible::encode_query(&endpoint.query)
        );

        let mut response = match self
            .client
            .get(&url)
            .map_err(ProviderError::from)?
            .timeout(crate::provider_core::non_streaming_timeout())
            .max_decoded_body_size(options.response_byte_limit)
            .header("x-goog-api-key", &self.api_key)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("google discovery failed: {}", error);
                return Ok(Vec::new());
            }
        };

        if !response.status().is_success() {
            tracing::warn!("google discovery returned HTTP {}", response.status());
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
        let Some(entries) = value.get("models").and_then(|m| m.as_array()) else {
            return Ok(Vec::new());
        };
        if entries.len() > options.model_count_limit {
            return Ok(Vec::new());
        }

        let mut models = Vec::with_capacity(entries.len());
        for entry in entries {
            // Gemini reports ids as `models/<id>`; the wire path and the model
            // selector both use the bare id.
            let Some(id) = entry
                .get("name")
                .and_then(|v| v.as_str())
                .and_then(|name| name.strip_prefix("models/"))
            else {
                continue;
            };
            if id.is_empty() || id.chars().count() > options.model_string_limit {
                continue;
            }
            let name = entry
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(id);
            // Gemini genuinely advertises its context window, so it is read
            // rather than invented. Capability booleans are not advertised at
            // all and stay false, meaning "unknown", never "in capable".
            let context_window = entry
                .get("inputTokenLimit")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let max_output_tokens = entry
                .get("outputTokenLimit")
                .and_then(|v| v.as_u64())
                .map(|value| value as usize);
            models.push(ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
                provider: self.id().to_string(),
                context_window,
                max_output_tokens,
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
