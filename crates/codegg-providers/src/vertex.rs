use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

use crate::auth_types::Credential;
use crate::error::ProviderError;
use crate::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::{ChatEvent, ChatRequest, ModelInfo, Provider};

#[derive(Clone)]
pub struct VertexProvider {
    inner: OpenAiCompatibleProvider,
}

impl VertexProvider {
    pub fn new(project_id: String, access_token: String) -> Self {
        let base_url = format!(
            "https://{project_id}-aiplatform.googleapis.com/v1beta1/projects/{project_id}/locations/us-central1/endpoints/openapi"
        );
        Self {
            inner: OpenAiCompatibleProvider::new(
                "vertex",
                "Google Vertex",
                OpenAiCompatibleConfig {
                    credential: Credential::bearer(access_token, None),
                    base_url,
                    auth_header: "Bearer".to_string(),
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
impl Provider for VertexProvider {
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
