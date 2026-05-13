use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::error::ProviderError;
use crate::provider::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::provider::{ChatEvent, ChatRequest, ModelInfo, Provider};

#[derive(Clone)]
pub struct GitLabProvider {
    inner: OpenAiCompatibleProvider,
}

impl GitLabProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new(
                "gitlab",
                "GitLab",
                OpenAiCompatibleConfig {
                    api_key: api_key.clone(),
                    base_url: "https://gitlab.com/api/v4/ai/chat".to_string(),
                    auth_header: "Authorization".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice_auto: false,
                    models: vec![
                        ModelInfo {
                            id: "gitlab/claude-sonnet-4".to_string(),
                            name: "Claude Sonnet 4 (GitLab)".to_string(),
                            provider: "anthropic".to_string(),
                            context_window: 200_000,
                            max_output_tokens: Some(64_000),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "gitlab/gpt-4o".to_string(),
                            name: "GPT-4o (GitLab)".to_string(),
                            provider: "openai".to_string(),
                            context_window: 128_000,
                            max_output_tokens: Some(16_384),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                    ],
                },
            ),
        }
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new(
                "gitlab",
                "GitLab",
                OpenAiCompatibleConfig {
                    api_key,
                    base_url,
                    auth_header: "Authorization".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice_auto: false,
                    models: vec![
                        ModelInfo {
                            id: "gitlab/claude-sonnet-4".to_string(),
                            name: "Claude Sonnet 4 (GitLab)".to_string(),
                            provider: "anthropic".to_string(),
                            context_window: 200_000,
                            max_output_tokens: Some(64_000),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "gitlab/gpt-4o".to_string(),
                            name: "GPT-4o (GitLab)".to_string(),
                            provider: "openai".to_string(),
                            context_window: 128_000,
                            max_output_tokens: Some(16_384),
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
impl Provider for GitLabProvider {
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
