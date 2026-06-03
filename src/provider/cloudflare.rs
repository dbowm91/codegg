use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::error::ProviderError;
use crate::provider::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::provider::{ChatEvent, ChatRequest, ModelInfo, Provider};

#[derive(Clone)]
pub struct CloudflareProvider {
    inner: OpenAiCompatibleProvider,
}

impl CloudflareProvider {
    pub fn new(account_id: String, api_token: String) -> Self {
        let base_url = format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1");
        Self {
            inner: OpenAiCompatibleProvider::new(
                "cloudflare",
                "Cloudflare Workers AI",
                OpenAiCompatibleConfig {
                    api_key: api_token.clone(),
                    base_url,
                    auth_header: "Authorization".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice: crate::provider::openai_compatible::ToolChoice::None,
                    models: vec![
                        ModelInfo {
                            id: "cloudflare/@cf/meta/llama-3.3-70b-instruct-fp8-fast".to_string(),
                            name: "Llama 3.3 70B".to_string(),
                            provider: "meta".to_string(),
                            context_window: 128_000,
                            max_output_tokens: Some(8_192),
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "cloudflare/@cf/meta/llama-3.1-8b-instruct".to_string(),
                            name: "Llama 3.1 8B".to_string(),
                            provider: "meta".to_string(),
                            context_window: 128_000,
                            max_output_tokens: Some(8_192),
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "cloudflare/@cf/qwen/qwen1.5-14b-chat-awq".to_string(),
                            name: "Qwen 1.5 14B".to_string(),
                            provider: "qwen".to_string(),
                            context_window: 32_000,
                            max_output_tokens: Some(4_096),
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        },
                    ],
                },
            ),
        }
    }
}

#[async_trait]
impl Provider for CloudflareProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn stream(
        &self,
        request: &ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatEvent, ProviderError>> + Send>>, ProviderError>
    {
        self.inner.stream(request).await
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.models().await
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
}
