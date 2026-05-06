use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::error::ProviderError;
use crate::provider::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::provider::{ChatEvent, ChatRequest, ModelInfo, Provider};

#[derive(Clone)]
pub struct CopilotProvider {
    inner: OpenAiCompatibleProvider,
}

impl CopilotProvider {
    pub fn new(token: String) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new(
                "copilot",
                "GitHub Copilot",
                OpenAiCompatibleConfig {
                    api_key: token.clone(),
                    base_url: "https://api.githubcopilot.com".to_string(),
                    auth_header: "Authorization".to_string(),
                    extra_headers: vec![("Editor-Version".to_string(), "codegg/1.0".to_string())],
                    models: vec![
                        ModelInfo {
                            id: "copilot/gpt-4o".to_string(),
                            name: "GPT-4o (Copilot)".to_string(),
                            provider: "openai".to_string(),
                            context_window: 128_000,
                            max_output_tokens: Some(16_384),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "copilot/o1".to_string(),
                            name: "o1 (Copilot)".to_string(),
                            provider: "openai".to_string(),
                            context_window: 200_000,
                            max_output_tokens: Some(100_000),
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "copilot/o3-mini".to_string(),
                            name: "o3-mini (Copilot)".to_string(),
                            provider: "openai".to_string(),
                            context_window: 200_000,
                            max_output_tokens: Some(100_000),
                            supports_tools: true,
                            supports_vision: false,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "copilot/claude-sonnet-4".to_string(),
                            name: "Claude Sonnet 4 (Copilot)".to_string(),
                            provider: "anthropic".to_string(),
                            context_window: 200_000,
                            max_output_tokens: Some(64_000),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                    ],
                },
            ),
        }
    }
}

#[async_trait]
impl Provider for CopilotProvider {
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
