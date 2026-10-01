use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

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
            "{}/openai/deployments/{}/chat/completions?api-version=2024-10-21",
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

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4.1".to_string(),
                name: "GPT-4.1".to_string(),
                provider: "azure".to_string(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "azure".to_string(),
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
