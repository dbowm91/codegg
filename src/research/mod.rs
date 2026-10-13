pub mod claims;
pub mod coordinator;
pub mod error;
pub mod extract;
pub mod llm;
pub mod runtime;
pub mod service;
pub mod sources;
pub mod store;
pub mod synthesis;
pub mod templates;
pub mod triggers;
pub mod types;
pub mod verify;

/// Shared test provider for research plumbing tests.
///
/// Only the plumbing (does the provider reach the coordinator, does the
/// model-backed branch activate) is under test here — no test asserts on
/// model output, so a single inert provider is enough.
#[cfg(test)]
pub(crate) mod tests {
    use crate::provider::{
        ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError,
    };
    use async_trait::async_trait;

    pub(crate) struct StubProvider;

    #[async_trait]
    impl Provider for StubProvider {
        fn id(&self) -> &str {
            "research-stub"
        }

        fn name(&self) -> &str {
            "Research Stub"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self)
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            Ok(Box::pin(futures_util::stream::iter(Vec::<
                Result<ChatEvent, ProviderError>,
            >::new())))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(Vec::new())
        }
    }
}
