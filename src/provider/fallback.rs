#[allow(unused_imports)]
use super::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, TokenUsage};
use crate::error::ProviderError;
use crate::resilience::CircuitBreaker;
#[allow(unused_imports)]
use async_trait::async_trait;

pub struct FallbackProvider {
    providers: Vec<Box<dyn Provider>>,
    status_codes: Vec<u16>,
    circuit_breakers: Vec<CircuitBreaker>,
}

impl FallbackProvider {
    pub fn new(providers: Vec<Box<dyn Provider>>, status_codes: Vec<u16>) -> Self {
        let status_codes = if status_codes.is_empty() {
            vec![429, 500, 502, 503, 504]
        } else {
            status_codes
        };
        let circuit_breakers = providers
            .iter()
            .map(|p| CircuitBreaker::new(p.name(), 3, 60, 2))
            .collect();
        Self {
            providers,
            status_codes,
            circuit_breakers,
        }
    }
}

#[async_trait]
impl Provider for FallbackProvider {
    fn id(&self) -> &str {
        "fallback"
    }

    fn name(&self) -> &str {
        "FallbackProvider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self {
            providers: self.providers.iter().map(|p| p.clone_box()).collect(),
            status_codes: self.status_codes.clone(),
            circuit_breakers: self.circuit_breakers.clone(),
        })
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut last_error = None;

        for (i, provider) in self.providers.iter().enumerate() {
            // Check if circuit breaker allows calling this provider
            if let Some(cb) = self.circuit_breakers.get(i) {
                if !cb.is_available().await {
                    tracing::warn!(
                        "fallback: skipping provider {} ({}) - circuit breaker is open",
                        provider.name(),
                        provider.id()
                    );
                    let msg = format!("circuit_breaker open for {}", provider.name());
                    last_error = Some(ProviderError::api(
                        "circuit_breaker",
                        &msg,
                    ));
                    continue;
                }
            }

            match provider.stream(request).await {
                Ok(stream) => {
                    // Record success in circuit breaker
                    if let Some(cb) = self.circuit_breakers.get(i) {
                        cb.record_success().await;
                    }
                    if i > 0 {
                        tracing::info!(
                            "fallback: recovered on provider {} ({}) after {} failed attempts",
                            provider.name(),
                            provider.id(),
                            i
                        );
                    }
                    return Ok(stream);
                }
                Err(e) => {
                    // Record failure in circuit breaker
                    if let Some(cb) = self.circuit_breakers.get(i) {
                        cb.record_failure().await;
                    }

                    let code = status_code(&e);
                    let is_retryable = code
                        .map(|c| self.status_codes.contains(&c))
                        .unwrap_or(false);

                    tracing::warn!(
                        "fallback: provider {} ({}) failed with error {} (status: {:?}, retryable: {})",
                        provider.name(),
                        provider.id(),
                        e,
                        code,
                        is_retryable
                    );

                    if is_retryable {
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| ProviderError::api("fallback", "all providers failed")))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut all_models = Vec::new();
        for provider in &self.providers {
            if let Ok(models) = provider.models().await {
                all_models.extend(models);
            }
        }
        Ok(all_models)
    }
}

fn status_code(e: &ProviderError) -> Option<u16> {
    match e {
        ProviderError::Api { code, .. } => code.parse().ok(),
        ProviderError::RateLimit => Some(429),
        _ => None,
    }
}

pub fn default_status_codes() -> Vec<u16> {
    vec![429, 500, 502, 503, 504]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone)]
    struct MockProvider {
        id: String,
        name: String,
        should_fail: bool,
        fail_count: Arc<AtomicUsize>,
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail || self.fail_count.load(Ordering::SeqCst) > 0 {
                self.fail_count.fetch_sub(1, Ordering::SeqCst);
                return Err(ProviderError::api("429", "rate limited"));
            }
            Ok(Box::pin(futures::stream::once(async {
                Ok(ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                })
            })))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_fallback_first_provider_succeeds() {
        let provider1 = MockProvider {
            id: "p1".to_string(),
            name: "Provider1".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let fallback = FallbackProvider::new(
            vec![Box::new(provider1.clone()), Box::new(provider2.clone())],
            vec![429, 500, 502, 503, 504],
        );

        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fallback_falls_to_second_on_retryable() {
        let fail_count = Arc::new(AtomicUsize::new(1));
        let call_count = Arc::new(AtomicUsize::new(0));

        let provider1 = MockProvider {
            id: "p1".to_string(),
            name: "Provider1".to_string(),
            should_fail: true,
            fail_count: fail_count.clone(),
            call_count: call_count.clone(),
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let fallback = FallbackProvider::new(
            vec![Box::new(provider1.clone()), Box::new(provider2.clone())],
            vec![429],
        );

        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fallback_returns_last_error_on_all_fail() {
        let provider1 = MockProvider {
            id: "p1".to_string(),
            name: "Provider1".to_string(),
            should_fail: true,
            fail_count: Arc::new(AtomicUsize::new(1)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: true,
            fail_count: Arc::new(AtomicUsize::new(1)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let fallback = FallbackProvider::new(
            vec![Box::new(provider1.clone()), Box::new(provider2.clone())],
            vec![429],
        );

        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fallback_non_retryable_error_stops_immediately() {
        let call_count = Arc::new(AtomicUsize::new(0));

        let provider1 = MockProvider {
            id: "p1".to_string(),
            name: "Provider1".to_string(),
            should_fail: true,
            fail_count: Arc::new(AtomicUsize::new(1)),
            call_count: call_count.clone(),
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
        };

        let fallback = FallbackProvider::new(
            vec![Box::new(provider1.clone()), Box::new(provider2.clone())],
            vec![500],
        );

        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_default_status_codes() {
        let fallback = FallbackProvider::new(vec![], vec![]);
        assert_eq!(fallback.status_codes, vec![429, 500, 502, 503, 504]);
    }
}
