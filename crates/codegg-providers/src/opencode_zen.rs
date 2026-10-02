use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Clone)]
pub struct OpencodeZenProvider {
    api_key: String,
    client: eggfetch_core::Client,
    base_url: String,
}

impl OpencodeZenProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: create_http_client(),
            base_url: crate::setup_catalog::OPENCODE_ZEN_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    pub(crate) fn build_body(&self, req: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        let mut body = crate::wire::encode_openai_chat(req, None, false)?;
        if req.tools.is_none() {
            body.as_object_mut().map(|object| object.remove("tools"));
        }
        crate::wire::omit_openai_fields(&mut body, &["response_format", "reasoning_effort"]);
        body.as_object_mut()
            .map(|object| object.remove("tool_choice"));
        Ok(body)
    }
}

#[async_trait]
impl Provider for OpencodeZenProvider {
    fn id(&self) -> &str {
        "opencode_zen"
    }

    fn name(&self) -> &str {
        "Codegg Zen"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req)?;
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.api_key.clone();
        let client = self.client.clone();

        tracing::debug!(
            "CodeggZen: sending request to {} with model {}",
            url,
            req.model
        );

        let req_builder = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("authorization", &format!("Bearer {}", api_key))
            .header("content-type", "application/json");

        let mut resp = req_builder
            .json(&body)
            .map_err(|e| ProviderError::api("serialization", e.to_string()))?
            .send()
            .await
            .map_err(|e| {
                tracing::error!("CodeggZen: request failed: {}", e);
                ProviderError::from(e)
            })?;

        tracing::debug!("CodeggZen: received response with status {}", resp.status());

        if resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(resp.headers()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            tracing::error!("CodeggZen: API error ({}): {}", status, err_text);
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: {}", status, err_text),
            ));
        }

        let stream = resp.bytes_stream().map_err(ProviderError::from)?;
        Ok(crate::wire::openai_chat_stream(
            stream,
            req.context.wire_policy.clone(),
            Some(std::time::Duration::from_secs(30)),
        ))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "big-pickle".to_string(),
                name: "Big Pickle (Free)".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "minimax-m2.5-free".to_string(),
                name: "MiniMax M2.5 Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 200_000,
                max_output_tokens: Some(64_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "nemotron-3-super-free".to_string(),
                name: "Nemotron 3 Super Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(32_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
            ModelInfo {
                id: "qwen3.6-plus-free".to_string(),
                name: "Qwen3.6 Plus Free".to_string(),
                provider: "opencode_zen".to_string(),
                context_window: 128_000,
                max_output_tokens: Some(32_000),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            },
        ])
    }

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/models", self.base_url);
        let client = self.client.clone();

        let mut resp = client
            .get(&url)
            .map_err(ProviderError::from)?
            .timeout(crate::provider_core::non_streaming_timeout())
            .send()
            .await
            .map_err(ProviderError::from)?;

        if resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::rate_limit_from_headers(resp.headers()));
        }

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(ProviderError::api(
                status.as_u16().to_string(),
                format!("HTTP {}: failed to fetch models", status),
            ));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            ProviderError::api(
                "parse_error",
                format!("failed to parse models response: {}", e),
            )
        })?;

        let mut models = Vec::new();

        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for entry in data {
                if let (Some(id), Some(name)) = (
                    entry.get("id").and_then(|v| v.as_str()),
                    entry.get("name").and_then(|v| v.as_str()),
                ) {
                    let context_window = entry
                        .get("context_window")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(128_000) as usize;
                    let max_output = entry
                        .get("max_output_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize);
                    let supports_tools = entry
                        .get("supports_tools")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let supports_vision = entry
                        .get("supports_vision")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    models.push(ModelInfo {
                        id: id.to_string(),
                        name: name.to_string(),
                        provider: "opencode_zen".to_string(),
                        context_window,
                        max_output_tokens: max_output,
                        supports_tools,
                        supports_vision,
                        variants: vec![],
                    });
                }
            }
        }

        if models.is_empty() {
            return self.models().await;
        }

        Ok(models)
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id() {
        let provider = OpencodeZenProvider::new("test-key".to_string());
        assert_eq!(provider.id(), "opencode_zen");
    }

    #[test]
    fn test_provider_name() {
        let provider = OpencodeZenProvider::new("test-key".to_string());
        assert_eq!(provider.name(), "Codegg Zen");
    }

    #[test]
    fn test_with_base_url() {
        let provider = OpencodeZenProvider::new("test-key".to_string())
            .with_base_url("https://custom.api.com/v1".to_string());
        assert_eq!(provider.base_url, "https://custom.api.com/v1");
    }
}
