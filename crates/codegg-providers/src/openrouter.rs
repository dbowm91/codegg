use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Clone)]
pub struct OpenRouterProvider {
    api_key: String,
    client: eggfetch_core::Client,
    app_name: Option<String>,
    app_url: Option<String>,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: create_http_client(),
            app_name: None,
            app_url: None,
        }
    }

    pub fn with_app_info(mut self, name: String, url: String) -> Self {
        self.app_name = Some(name);
        self.app_url = Some(url);
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
impl Provider for OpenRouterProvider {
    fn id(&self) -> &str {
        "openrouter"
    }

    fn name(&self) -> &str {
        "OpenRouter"
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.build_body(req)?;
        let url = format!(
            "{}/chat/completions",
            crate::setup_catalog::OPENROUTER_ENDPOINT
        );
        let api_key = self.api_key.clone();
        let client = self.client.clone();
        let app_name = self.app_name.clone();
        let app_url = self.app_url.clone();

        let req_builder = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("authorization", &format!("Bearer {}", api_key))
            .header("content-type", "application/json")
            .header(
                "HTTP-Referer",
                app_url.as_deref().unwrap_or("https://opencode.ai"),
            )
            .header("X-Title", app_name.as_deref().unwrap_or("Codegg"));

        let mut resp = req_builder
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

    /// Discover models upstream. OpenRouter speaks the OpenAI-compatible
    /// `/models` contract, so the shared compatible provider owns the bounded,
    /// profile-driven discovery call. No compiled-in catalog is shipped.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let discovery = crate::openai_compatible::OpenAiCompatibleProvider::simple_with_credential(
            "openrouter",
            "OpenRouter",
            crate::auth_types::Credential::api_key(self.api_key.clone()),
            crate::setup_catalog::OPENROUTER_ENDPOINT,
        );
        discovery.models().await
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}
