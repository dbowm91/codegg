use crate::error::ProviderError;
use crate::{create_http_client, ChatRequest, EventStream, ModelInfo, Provider};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub base_url: String,
    pub provider_id: String,
    pub provider_name: String,
    pub requires_org_header: bool,
    pub organization: Option<String>,
    pub omit_stream_options: bool,
    pub tool_choice: crate::openai_compatible::ToolChoice,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: false,
            organization: None,
            omit_stream_options: false,
            tool_choice: crate::openai_compatible::ToolChoice::Auto,
        }
    }
}

impl OpenAiConfig {
    pub fn default_with_key(api_key: String) -> Self {
        Self {
            api_key,
            ..Default::default()
        }
    }

    pub fn openai(api_key: String) -> Self {
        Self {
            api_key,
            provider_id: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            requires_org_header: true,
            ..Default::default()
        }
    }

    pub fn groq(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            provider_id: "groq".to_string(),
            provider_name: "Groq".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }

    pub fn xai(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.x.ai/v1".to_string(),
            provider_id: "xai".to_string(),
            provider_name: "xAI".to_string(),
            ..Default::default()
        }
    }

    pub fn mistral(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.mistral.ai".to_string(),
            provider_id: "mistral".to_string(),
            provider_name: "Mistral".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }

    pub fn cerebras(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.cerebras.ai".to_string(),
            provider_id: "cerebras".to_string(),
            provider_name: "Cerebras".to_string(),
            omit_stream_options: true,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct OpenAiProvider {
    cfg: OpenAiConfig,
    client: eggfetch_core::Client,
}

impl OpenAiProvider {
    pub fn new(cfg: OpenAiConfig) -> Self {
        Self {
            cfg,
            client: create_http_client(),
        }
    }

    pub fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        self.try_build_body(req).unwrap_or_else(|error| {
            tracing::error!("shared OpenAI request encoding failed: {}", error);
            serde_json::Value::Null
        })
    }

    fn try_build_body(&self, req: &ChatRequest) -> Result<serde_json::Value, ProviderError> {
        crate::wire::encode_openai_chat(
            req,
            Some(&self.cfg.tool_choice),
            !self.cfg.omit_stream_options,
        )
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.cfg.provider_id
    }

    fn name(&self) -> &str {
        &self.cfg.provider_name
    }

    async fn stream(&self, req: &ChatRequest) -> Result<EventStream, ProviderError> {
        let body = self.try_build_body(req)?;
        let url = format!("{}/v1/chat/completions", self.cfg.base_url);
        let api_key = self.cfg.api_key.clone();
        let client = self.client.clone();
        let requires_org = self.cfg.requires_org_header;
        let org = self.cfg.organization.clone();

        let mut req_builder = client
            .post(&url)
            .map_err(ProviderError::from)?
            .header("authorization", &format!("Bearer {}", api_key))
            .header("content-type", "application/json");

        if requires_org {
            if let Some(ref org_id) = org {
                req_builder = req_builder.header("OpenAI-Organization", org_id);
            }
        }

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

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4.1".to_string(),
                name: "GPT-4.1".to_string(),
                provider: self.cfg.provider_id.clone(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4.1-mini".to_string(),
                name: "GPT-4.1 Mini".to_string(),
                provider: self.cfg.provider_id.clone(),
                context_window: 1_047_576,
                max_output_tokens: Some(32_768),
                supports_tools: true,
                supports_vision: true,
                variants: vec![],
            },
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: self.cfg.provider_id.clone(),
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
