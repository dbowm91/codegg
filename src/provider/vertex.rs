use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::error::ProviderError;
use crate::provider::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use crate::provider::{ChatEvent, ChatRequest, ModelInfo, Provider};

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
                    api_key: access_token,
                    base_url,
                    auth_header: "Bearer".to_string(),
                    extra_headers: Vec::new(),
                    tool_choice: crate::provider::openai_compatible::ToolChoice::None,
                    models: vec![
                        ModelInfo {
                            id: "vertex/gemini-2.5-pro".to_string(),
                            name: "Gemini 2.5 Pro".to_string(),
                            provider: "google".to_string(),
                            context_window: 1_048_576,
                            max_output_tokens: Some(65_536),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "vertex/gemini-2.5-flash".to_string(),
                            name: "Gemini 2.5 Flash".to_string(),
                            provider: "google".to_string(),
                            context_window: 1_048_576,
                            max_output_tokens: Some(65_536),
                            supports_tools: true,
                            supports_vision: true,
                            variants: Vec::new(),
                        },
                        ModelInfo {
                            id: "vertex/gemini-2.0-flash".to_string(),
                            name: "Gemini 2.0 Flash".to_string(),
                            provider: "google".to_string(),
                            context_window: 1_048_576,
                            max_output_tokens: Some(8_192),
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
