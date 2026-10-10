use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::{ChatEvent, ChatRequest, ModelInfo, Provider};

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
                    credential: Credential::api_key(api_key),
                    base_url: "https://gitlab.com/api/v4/ai/chat".to_string(),
                    auth_header: "Authorization".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice: crate::openai_compatible::ToolChoice::None,
                    // No compiled-in seed list: this provider's models come from
                    // discovery, or from an operator-declared additive config block.
                    models: Vec::new(),
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
                    credential: Credential::api_key(api_key),
                    base_url,
                    auth_header: "Authorization".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice: crate::openai_compatible::ToolChoice::None,
                    // No compiled-in seed list: this provider's models come from
                    // discovery, or from an operator-declared additive config block.
                    models: Vec::new(),
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
