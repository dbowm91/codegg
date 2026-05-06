use async_trait::async_trait;
use futures::StreamExt;
use codegg::provider::{
    ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, ProviderRegistry,
    TokenUsage,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockProvider {
    responses: Vec<String>,
    model_list: Vec<ModelInfo>,
}

struct RateLimitMockProvider {
    request_count: AtomicUsize,
    limit: usize,
}

struct RetryableErrorMockProvider {
    attempt_count: AtomicUsize,
    max_attempts: usize,
}

struct TimeoutMockProvider;

struct AuthErrorMockProvider;

impl MockProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses,
            model_list: vec![ModelInfo {
                id: "mock/model".to_string(),
                name: "Mock Model".to_string(),
                provider: "mock".to_string(),
                context_window: 4096,
                max_output_tokens: Some(2048),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            }],
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn name(&self) -> &str {
        "Mock Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self {
            responses: self.responses.clone(),
            model_list: self.model_list.clone(),
        })
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let responses = self.responses.clone();
        let stream = futures::stream::iter(
            responses
                .into_iter()
                .map(|text| Ok::<_, ProviderError>(ChatEvent::TextDelta(text.into()))),
        )
        .chain(futures::stream::once(async {
            Ok(ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    total_tokens: 30,
                    reasoning_tokens: 0,
                },
            })
        }));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self.model_list.clone())
    }
}

struct ErrorMockProvider;

#[async_trait]
impl Provider for ErrorMockProvider {
    fn id(&self) -> &str {
        "error-mock"
    }

    fn name(&self) -> &str {
        "Error Mock"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self)
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let stream = futures::stream::once(async {
            Err::<ChatEvent, ProviderError>(ProviderError::Api {
                code: "simulated".to_string(),
                message: "simulated error".to_string(),
                url: "mock://".to_string(),
            })
        });
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Api {
            code: "models".to_string(),
            message: "models error".to_string(),
            url: "mock://".to_string(),
        })
    }
}

impl RateLimitMockProvider {
    fn new(limit: usize) -> Self {
        Self {
            request_count: AtomicUsize::new(0),
            limit,
        }
    }
}

#[async_trait]
impl Provider for RateLimitMockProvider {
    fn id(&self) -> &str {
        "rate-limit-mock"
    }

    fn name(&self) -> &str {
        "Rate Limit Mock"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self::new(self.limit))
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let count = self.request_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.limit {
            return Err(ProviderError::RateLimit);
        }
        let stream = futures::stream::iter(vec![Ok::<_, ProviderError>(ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        })]);
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[async_trait]
impl Provider for TimeoutMockProvider {
    fn id(&self) -> &str {
        "timeout-mock"
    }

    fn name(&self) -> &str {
        "Timeout Mock"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self)
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        unreachable!("timeout mock should timeout")
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[async_trait]
impl Provider for AuthErrorMockProvider {
    fn id(&self) -> &str {
        "auth-error-mock"
    }

    fn name(&self) -> &str {
        "Auth Error Mock"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self)
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        Err(ProviderError::Auth("invalid token".to_string()))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Auth("invalid token".to_string()))
    }
}

impl RetryableErrorMockProvider {
    fn new(max_attempts: usize) -> Self {
        Self {
            attempt_count: AtomicUsize::new(0),
            max_attempts,
        }
    }
}

#[async_trait]
impl Provider for RetryableErrorMockProvider {
    fn id(&self) -> &str {
        "retryable-mock"
    }

    fn name(&self) -> &str {
        "Retryable Error Mock"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self::new(self.max_attempts))
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let count = self.attempt_count.fetch_add(1, Ordering::SeqCst);
        if count < self.max_attempts.saturating_sub(1) {
            return Err(ProviderError::Api {
                code: "retryable".to_string(),
                message: format!("retryable error attempt {}", count + 1),
                url: "mock://".to_string(),
            });
        }
        let stream = futures::stream::iter(vec![Ok::<_, ProviderError>(ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        })]);
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_mock_provider_streams_text() {
    let provider = MockProvider::new(vec!["Hello, ".to_string(), "world!".to_string()]);

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let mut stream = provider.stream(&request).await.unwrap();
    let mut collected = Vec::new();

    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        collected.push(event.unwrap());
    }

    assert_eq!(collected.len(), 3);
    assert!(matches!(&collected[0], ChatEvent::TextDelta(t) if t.as_ref() == "Hello, "));
    assert!(matches!(&collected[1], ChatEvent::TextDelta(t) if t.as_ref() == "world!"));
    assert!(
        matches!(&collected[2], ChatEvent::Finish { stop_reason, .. } if stop_reason.as_ref() == "stop")
    );
}

#[tokio::test]
async fn test_mock_provider_returns_models() {
    let provider = MockProvider::new(vec![]);
    let models = provider.models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "mock/model");
    assert!(models[0].supports_tools);
}

#[tokio::test]
async fn test_mock_provider_error_stream() {
    let provider = ErrorMockProvider;

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let mut stream = provider.stream(&request).await.unwrap();
    let event = futures::StreamExt::next(&mut stream).await.unwrap();
    assert!(event.is_err());
}

#[tokio::test]
async fn test_mock_provider_error_models() {
    let provider = ErrorMockProvider;
    let result = provider.models().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_registry_with_mock_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(MockProvider::new(vec![]));

    let provider = registry.get("mock").unwrap();
    assert_eq!(provider.id(), "mock");
    assert_eq!(provider.name(), "Mock Provider");

    let models = provider.models().await.unwrap();
    assert_eq!(models.len(), 1);
}

#[tokio::test]
async fn test_registry_list_includes_mock() {
    let mut registry = ProviderRegistry::new();
    registry.register(MockProvider::new(vec![]));
    registry.register(ErrorMockProvider);

    let list = registry.list();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn test_mock_provider_with_tool_call_events() {
    struct ToolCallMockProvider;

    #[async_trait]
    impl Provider for ToolCallMockProvider {
        fn id(&self) -> &str {
            "tool-mock"
        }

        fn name(&self) -> &str {
            "Tool Call Mock"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self)
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            let stream = futures::stream::iter(vec![
                Ok(ChatEvent::TextDelta("Let me use a tool".to_string().into())),
                Ok(ChatEvent::ToolCall(codegg::provider::ToolCall {
                    id: "call_1".to_string().into(),
                    name: "bash".to_string().into(),
                    arguments: serde_json::json!({"command": "echo hello"}),
                })),
                Ok(ChatEvent::Finish {
                    stop_reason: "tool_calls".to_string().into(),
                    usage: TokenUsage::default(),
                }),
            ]);
            Ok(Box::pin(stream))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    let provider = ToolCallMockProvider;
    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let mut stream = provider.stream(&request).await.unwrap();
    let mut events = Vec::new();

    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        events.push(event.unwrap());
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[1], ChatEvent::ToolCall(tc) if tc.name.as_ref() == "bash"));
}

#[tokio::test]
async fn test_mock_provider_reasoning_delta() {
    struct ReasoningMockProvider;

    #[async_trait]
    impl Provider for ReasoningMockProvider {
        fn id(&self) -> &str {
            "reasoning-mock"
        }

        fn name(&self) -> &str {
            "Reasoning Mock"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self)
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            let stream = futures::stream::iter(vec![
                Ok(ChatEvent::ReasoningDelta(
                    "Let me think about this...".to_string().into(),
                )),
                Ok(ChatEvent::TextDelta("Here's my answer".to_string().into())),
                Ok(ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage {
                        input_tokens: 100,
                        output_tokens: 50,
                        total_tokens: 150,
                        reasoning_tokens: 0,
                    },
                }),
            ]);
            Ok(Box::pin(stream))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    let provider = ReasoningMockProvider;
    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let mut stream = provider.stream(&request).await.unwrap();
    let mut events = Vec::new();

    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        events.push(event.unwrap());
    }

    assert!(matches!(
        &events[0],
        ChatEvent::ReasoningDelta(s) if s.as_ref() == "Let me think about this..."
    ));
    assert!(matches!(&events[1], ChatEvent::TextDelta(s) if s.as_ref() == "Here's my answer"));

    if let ChatEvent::Finish { usage, .. } = &events[2] {
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    } else {
        panic!("Expected Finish event");
    }
}

#[tokio::test]
async fn test_rate_limit_provider_blocks_after_limit() {
    let provider = RateLimitMockProvider::new(2);

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let result1 = provider.stream(&request).await;
    assert!(result1.is_ok());

    let result2 = provider.stream(&request).await;
    assert!(result2.is_ok());

    let result3 = provider.stream(&request).await;
    assert!(result3.is_err());
    if let Err(e) = result3 {
        assert!(matches!(e, ProviderError::RateLimit));
    } else {
        panic!("expected error");
    }
}

#[tokio::test]
async fn test_auth_error_provider_returns_auth_error() {
    let provider = AuthErrorMockProvider;

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let result = provider.stream(&request).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, ProviderError::Auth(_)));
    } else {
        panic!("expected error");
    }
}

#[tokio::test]
async fn test_auth_error_provider_models_returns_auth_error() {
    let provider = AuthErrorMockProvider;

    let result = provider.models().await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ProviderError::Auth(_)));
}

#[tokio::test]
async fn test_retryable_provider_retries_then_succeeds() {
    let provider = RetryableErrorMockProvider::new(3);

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let result1 = provider.stream(&request).await;
    assert!(result1.is_err());
    if let Err(ProviderError::Api { message, .. }) = result1 {
        assert!(message.contains("retryable error attempt 1"));
    } else {
        panic!("expected Api error with attempt 1");
    }

    let result2 = provider.stream(&request).await;
    assert!(result2.is_err());
    if let Err(ProviderError::Api { message, .. }) = result2 {
        assert!(message.contains("retryable error attempt 2"));
    } else {
        panic!("expected Api error with attempt 2");
    }

    let result3 = provider.stream(&request).await;
    assert!(result3.is_ok());
}

#[tokio::test]
async fn test_registry_get_returns_error_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(ErrorMockProvider);
    registry.register(RateLimitMockProvider::new(5));

    let error_provider = registry.get("error-mock");
    assert!(error_provider.is_some());
    assert_eq!(error_provider.unwrap().id(), "error-mock");
}

#[tokio::test]
async fn test_timeout_provider_returns_timeout_error() {
    let provider = TimeoutMockProvider;

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        provider.stream(&request),
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_provider_rate_limit_error() {
    let provider = RateLimitMockProvider::new(0);

    let request = ChatRequest {
        messages: vec![],
        model: "mock/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let result = provider.stream(&request).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, ProviderError::RateLimit));
    } else {
        panic!("expected error");
    }
}
