#[allow(unused_imports)]
use super::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, TokenUsage};
use crate::circuit::CircuitBreaker;
use crate::error::ProviderError;
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
            // Stream-aware admission: `try_admit` runs the same atomic
            // admission/probe state machine as `call`, but outcome
            // accounting happens exactly once at the terminal stream
            // boundary (acquisition failure, terminal stream failure, or
            // clean completion). Recording acquisition success eagerly
            // would reset the Closed failure count before a terminal
            // stream failure is known, making mid-stream failures
            // invisible to health.
            let breaker = self.circuit_breakers.get(i).cloned();
            if let Some(cb) = breaker.as_ref() {
                if let Err(e) = cb.try_admit().await {
                    let e = ProviderError::from(e);
                    tracing::warn!(
                        "fallback: skipping provider {} ({}) - circuit breaker is open",
                        provider.name(),
                        provider.id()
                    );
                    last_error = Some(e);
                    continue;
                }
            }
            let result = provider.stream(request).await;

            match result {
                Ok(stream) => {
                    if i > 0 {
                        tracing::info!(
                            "fallback: recovered on provider {} ({}) after {} failed attempts",
                            provider.name(),
                            provider.id(),
                            i
                        );
                    }
                    // Terminal-outcome accounting: clean completion records
                    // success, terminal stream failure records failure.
                    // Acquisition alone records nothing.
                    let wrapped =
                        wrap_stream_with_health(stream, breaker, provider.name().to_string());
                    return Ok(wrapped);
                }
                Err(e) => {
                    if matches!(e, ProviderError::CircuitOpen(_)) {
                        tracing::warn!(
                            "fallback: skipping provider {} ({}) - circuit breaker is open",
                            provider.name(),
                            provider.id()
                        );
                        last_error = Some(e);
                        continue;
                    }
                    // Terminal acquisition outcome for this slot.
                    if let Some(cb) = breaker.as_ref() {
                        cb.record_failure().await;
                    }
                    let code = status_code(&e);
                    // Failover when either the configured status list or the
                    // canonical retry taxonomy marks the acquisition failure
                    // retryable. The taxonomy covers transport/timeout/rate
                    // limit cases that carry no numeric status; the status
                    // list preserves explicit operator configuration.
                    // Permanent auth/invalid-request/model errors never
                    // fail over to another provider implicitly.
                    let status_retryable = code
                        .map(|c| self.status_codes.contains(&c))
                        .unwrap_or(false);
                    let is_retryable = status_retryable || e.is_retryable();

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
                        // Implement exponential backoff: base 1s, max 30s
                        let delay_secs = (2u64.pow(i as u32)).min(30);
                        tracing::info!("fallback: retrying next provider in {}s", delay_secs);
                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
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
        ProviderError::RateLimit | ProviderError::RateLimited { .. } => Some(429),
        _ => None,
    }
}

/// Wrap an acquired provider stream so the terminal stream outcome is
/// charged exactly once to the same circuit breaker that admitted the
/// acquisition.
///
/// Clean completion records success; terminal stream failure records
/// failure. Acquisition alone records nothing, and dropped-but-incomplete
/// streams record nothing. This reuses the existing breaker rather than
/// introducing a second health owner.
fn wrap_stream_with_health(
    stream: EventStream,
    breaker: Option<CircuitBreaker>,
    provider_name: String,
) -> EventStream {
    use futures_util::StreamExt;
    let Some(cb) = breaker else {
        return stream;
    };
    Box::pin(futures_util::stream::unfold(
        (stream, cb, provider_name),
        |(mut inner, cb, provider_name)| async move {
            match inner.next().await {
                None => {
                    cb.record_success().await;
                    None
                }
                Some(Ok(event)) => Some((Ok(event), (inner, cb, provider_name))),
                Some(Err(e)) => {
                    let class = e.error_class().to_string();
                    cb.record_failure().await;
                    tracing::warn!(
                        "fallback: provider {} terminal stream failure charged to circuit ({})",
                        provider_name,
                        class
                    );
                    Some((Err(e), (inner, cb, provider_name)))
                }
            }
        },
    ))
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
        error_code: Option<String>,
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
                let code = self.error_code.clone().unwrap_or_else(|| "429".to_string());
                return Err(ProviderError::api(code, "mock failure"));
            }
            Ok(Box::pin(futures_util::stream::once(async {
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
            error_code: None,
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
            error_code: None,
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
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_ok());
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
            // Permanent invalid-request failure: taxonomy and status list
            // agree it must not fail over.
            error_code: Some("400".to_string()),
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
            error_code: None,
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
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };

        let result = fallback.stream(&request).await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transient_taxonomy_fails_over_despite_narrow_status_list() {
        // 429 is transient by taxonomy even when the operator status list
        // only contains 500: the turn must still reach the next provider.
        let provider1 = MockProvider {
            id: "p1".to_string(),
            name: "Provider1".to_string(),
            should_fail: true,
            fail_count: Arc::new(AtomicUsize::new(1)),
            call_count: Arc::new(AtomicUsize::new(0)),
            error_code: None,
        };
        let provider2 = MockProvider {
            id: "p2".to_string(),
            name: "Provider2".to_string(),
            should_fail: false,
            fail_count: Arc::new(AtomicUsize::new(0)),
            call_count: Arc::new(AtomicUsize::new(0)),
            error_code: None,
        };
        let fallback =
            FallbackProvider::new(vec![Box::new(provider1), Box::new(provider2)], vec![500]);
        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };
        let result = fallback.stream(&request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn terminal_stream_failure_is_charged_to_circuit() {
        use crate::circuit::CircuitState;
        use futures_util::StreamExt;
        // Provider acquires fine, then the stream itself fails terminally.
        // The wrapper must charge exactly one failure to the slot breaker.
        struct MidStreamFail;
        #[async_trait]
        impl Provider for MidStreamFail {
            fn id(&self) -> &str {
                "mid"
            }
            fn name(&self) -> &str {
                "MidStreamFail"
            }
            fn clone_box(&self) -> Box<dyn Provider> {
                Box::new(Self)
            }
            async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
                Ok(Box::pin(futures_util::stream::once(async {
                    Err(ProviderError::Stream("interrupted".to_string()))
                })))
            }
            async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
                Ok(vec![])
            }
        }
        let fallback =
            FallbackProvider::new(vec![Box::new(MidStreamFail) as Box<dyn Provider>], vec![]);
        let request = ChatRequest {
            messages: vec![],
            model: "test".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };
        // Trip the breaker with three consecutive terminal stream failures
        // (threshold is 3): acquisition succeeds each time, so without the
        // wrapper the breaker would stay Closed forever.
        for _ in 0..3 {
            let mut stream = fallback
                .stream(&request)
                .await
                .expect("acquisition succeeds");
            let item = stream.next().await.expect("one terminal item");
            assert!(item.is_err());
        }
        assert_eq!(
            fallback.circuit_breakers[0].state().await,
            CircuitState::Open
        );
    }

    #[test]
    fn test_default_status_codes() {
        let fallback = FallbackProvider::new(vec![], vec![]);
        assert_eq!(fallback.status_codes, vec![429, 500, 502, 503, 504]);
    }
}
