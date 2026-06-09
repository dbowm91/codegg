//! LLM provider interface and implementations.
//!
//! This module provides the Provider trait for interacting with various LLM backends
//! including Anthropic, OpenAI, Google Vertex, AWS Bedrock, and more. Providers handle
//! authentication, request formatting, streaming responses, and error handling.

macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*);
    };
}

pub mod additional;
pub mod anthropic;
pub mod azure;
pub mod bedrock;
pub mod cache;
pub mod catalog;
pub mod cloudflare;
pub mod copilot;
pub mod discovery;
pub mod fallback;
pub mod gitlab;
pub mod google;
pub mod models;
pub mod openai;
pub mod openai_compatible;
pub mod opencode_zen;
pub mod openrouter;
pub mod sse_parser;
pub mod text_tool_parser;
pub mod vertex;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::auth::resolver::{AuthResolver, ResolverContext};
pub use crate::error::ProviderError;

pub const MAX_BUFFER_SIZE: usize = 1024 * 1024;

pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .inspect_err(|e| tracing::warn!("HTTP client builder failed, using default: {}", e))
        .unwrap_or_default()
}

pub type EventStream = Pin<Box<dyn Stream<Item = Result<ChatEvent, ProviderError>> + Send>>;

pub fn assistant_text_content_value(content: &[ContentPart]) -> serde_json::Value {
    let mut text = String::new();
    for part in content {
        if let ContentPart::Text { text: part_text } = part {
            text.push_str(part_text);
        }
    }
    serde_json::json!(text)
}

pub fn openai_tool_arguments_value(arguments: &serde_json::Value) -> serde_json::Value {
    serde_json::json!(arguments.to_string())
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn clone_box(&self) -> Box<dyn Provider>;
    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError>;
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.models().await
    }
    async fn ping(&self) -> Result<bool, ProviderError> {
        self.models().await.map(|m| !m.is_empty())
    }
}

#[async_trait]
impl Provider for Box<dyn Provider> {
    fn id(&self) -> &str {
        self.as_ref().id()
    }
    fn name(&self) -> &str {
        self.as_ref().name()
    }
    fn clone_box(&self) -> Box<dyn Provider> {
        self.as_ref().clone_box()
    }
    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.as_ref().stream(request).await
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.as_ref().models().await
    }
    async fn ping(&self) -> Result<bool, ProviderError> {
        self.as_ref().ping().await
    }
}

/// Provider capabilities for tool deferral and request limits.
///
/// Determines which providers support deferred tool loading and tool references,
/// allowing the agent loop to partition tools into immediate vs deferred arrays.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub supports_defer_loading: bool,
    pub supports_tool_references: bool,
    pub max_tools_per_request: Option<usize>,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            supports_defer_loading: false,
            supports_tool_references: false,
            max_tools_per_request: None,
        }
    }
}

impl ProviderCapabilities {
    /// Get capabilities for a specific provider by ID.
    ///
    /// Conservative defaults: providers without explicit support
    /// default to not supporting deferral.
    pub fn for_provider(provider_id: &str) -> Self {
        match provider_id {
            "anthropic" => Self {
                supports_defer_loading: true,
                supports_tool_references: true,
                max_tools_per_request: None,
            },
            "openai" => Self {
                supports_defer_loading: true,
                supports_tool_references: true,
                max_tools_per_request: Some(128),
            },
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub tools: Option<Vec<ToolDefinition>>,
    pub system: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<usize>,
    pub response_format: Option<ResponseFormat>,
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: Arc<String>,
    },
    User {
        content: Vec<ContentPart>,
    },
    Assistant {
        content: Vec<ContentPart>,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: Arc<String>,
        content: Arc<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text { text: Arc<String> },
    Image { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: Arc<String>,
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    TextDelta(Arc<String>),
    ReasoningDelta(Arc<String>),
    ToolCall(ToolCall),
    ToolResult {
        tool_call_id: Arc<String>,
        content: Arc<String>,
    },
    Finish {
        stop_reason: Arc<String>,
        usage: TokenUsage,
    },
    Error(Arc<String>),
}

#[derive(Debug, Clone)]
pub enum ResponseFormat {
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        strict: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Arc<String>,
    pub name: Arc<String>,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub reasoning_tokens: usize,
    pub cached_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub defer_loading: Option<bool>,
}

impl ToolDefinition {
    pub fn to_openai(&self) -> serde_json::Value {
        let mut func = serde_json::json!({
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters,
        });
        if let Some(defer) = self.defer_loading {
            func["defer_loading"] = serde_json::json!(defer);
        }
        serde_json::json!({
            "type": "function",
            "function": func,
        })
    }

    pub fn to_anthropic(&self) -> serde_json::Value {
        let mut tool = serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters,
        });
        if let Some(defer) = self.defer_loading {
            tool["defer_loading"] = serde_json::json!(defer);
        }
        tool
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ModelVariant {
    pub suffix: String,
    pub context_window_override: Option<usize>,
    pub max_output_override: Option<usize>,
    pub extra_params: serde_json::Value,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: usize,
    pub max_output_tokens: Option<usize>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub variants: Vec<ModelVariant>,
}

pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: impl Provider + 'static) {
        let id = provider.id().to_string();
        self.providers.insert(id, Box::new(provider));
    }

    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.providers.get(id).map(|p| p.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn Provider> {
        self.providers.values().map(|p| p.as_ref()).collect()
    }
}

pub fn register_builtin(registry: &mut ProviderRegistry) {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        registry.register(crate::provider::anthropic::AnthropicProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let cfg = crate::provider::openai::OpenAiConfig::default_with_key(key);
        registry.register(crate::provider::openai::OpenAiProvider::new(cfg));
    }
    if let Ok(key) = std::env::var("GOOGLE_API_KEY") {
        registry.register(crate::provider::google::GoogleProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        registry.register(crate::provider::openrouter::OpenRouterProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENCODE_ZEN_API_KEY") {
        registry.register(crate::provider::opencode_zen::OpencodeZenProvider::new(key));
    }
    if let Ok(key) = std::env::var("MISTRAL_API_KEY") {
        registry.register(crate::provider::additional::create_mistral(key));
    }
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        registry.register(crate::provider::additional::create_groq(key));
    }
    if let Ok(key) = std::env::var("DEEPINFRA_API_KEY") {
        registry.register(crate::provider::additional::create_deepinfra(key));
    }
    if let Ok(key) = std::env::var("CEREBRAS_API_KEY") {
        registry.register(crate::provider::additional::create_cerebras(key));
    }
    if let Ok(key) = std::env::var("COHERE_API_KEY") {
        registry.register(crate::provider::additional::create_cohere(key));
    }
    if let Ok(key) = std::env::var("TOGETHERAI_API_KEY") {
        registry.register(crate::provider::additional::create_together(key));
    }
    if let Ok(key) = std::env::var("PERPLEXITY_API_KEY") {
        registry.register(crate::provider::additional::create_perplexity(key));
    }
    if let Ok(key) = std::env::var("XAI_API_KEY") {
        registry.register(crate::provider::additional::create_xai(key));
    }
    if let Ok(key) = std::env::var("VENICE_API_KEY") {
        registry.register(crate::provider::additional::create_venice(key));
    }
    if let Ok(key) = std::env::var("MINIMAX_API_KEY") {
        registry.register(crate::provider::additional::create_minimax(key));
    }
}

fn register_config_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&std::collections::HashMap<String, crate::config::schema::ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    factory: F,
) where
    F: FnOnce(String, Option<String>) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        if let Some(cfg) = providers.and_then(|p| p.get(name)) {
            // Resolve through the typed `auth` descriptor first; fall back
            // to the legacy `api_key` field for backward compatibility.
            let resolver = AuthResolver::new();
            let ctx = ResolverContext {
                provider_id: name.to_string(),
                account_id: cfg.account_id.clone(),
                legacy_api_key: cfg.api_key.clone(),
                ..Default::default()
            };
            let resolved = resolver.resolve(cfg.auth.as_ref(), &ctx).ok().flatten();
            if let Some(resolved) = resolved {
                if !resolved.credential.secret.is_empty() {
                    tracing::debug!(
                        "register_config_provider: provider '{}' resolved via {}",
                        name,
                        resolved.source.as_str()
                    );
                    registry.register(factory(resolved.credential.secret, cfg.base_url.clone()));
                }
            } else if let Some(ref key) = cfg.api_key {
                registry.register(factory(key.clone(), cfg.base_url.clone()));
            }
        }
    }
}

fn register_env_fallback_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&std::collections::HashMap<String, crate::config::schema::ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    factory: F,
) where
    F: FnOnce(String) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: name.to_string(),
            account_id: cfg_opt.and_then(|c| c.account_id.clone()),
            legacy_api_key: cfg_opt.and_then(|c| c.api_key.clone()),
            env_override: Some(env_var.to_string()),
            ..Default::default()
        };
        match resolver.resolve(cfg_opt.and_then(|c| c.auth.as_ref()), &ctx) {
            Ok(Some(resolved)) => {
                if !resolved.credential.secret.is_empty() {
                    tracing::debug!(
                        "register_env_fallback_provider: registering provider '{}' via {}",
                        name,
                        resolved.source.as_str()
                    );
                    registry.register(factory(resolved.credential.secret));
                } else {
                    tracing::warn!(
                        "register_env_fallback_provider: NO KEY for provider '{}', env_var='{}' (empty resolved key)",
                        name,
                        env_var
                    );
                }
            }
            Ok(None) => {
                tracing::warn!(
                    "register_env_fallback_provider: NO KEY for provider '{}', env_var='{}' (empty key)",
                    name,
                    env_var
                );
            }
            Err(e) => {
                // Recognize-but-unimplemented auth modes (e.g. OAuthDevice)
                // should not prevent other providers from loading. Log and
                // skip.
                tracing::warn!(
                    "register_env_fallback_provider: provider '{}' could not be resolved: {}",
                    name,
                    e
                );
            }
        }
    }
}

pub fn register_builtin_with_config(
    registry: &mut ProviderRegistry,
    config: &crate::config::schema::Config,
) {
    let providers = config.provider.as_ref();
    let disabled = config.disabled_providers.as_ref();

    register_config_provider(
        registry,
        providers,
        disabled,
        "anthropic",
        |key, base_url| {
            let mut p = crate::provider::anthropic::AnthropicProvider::new(key);
            if let Some(url) = base_url {
                p = p.with_base_url(url);
            }
            Box::new(p)
        },
    );

    register_config_provider(registry, providers, disabled, "openai", |key, base_url| {
        let mut cfg = crate::provider::openai::OpenAiConfig::default_with_key(key);
        if let Some(url) = base_url {
            cfg.base_url = url;
        }
        Box::new(crate::provider::openai::OpenAiProvider::new(cfg))
    });

    register_config_provider(registry, providers, disabled, "google", |key, _base_url| {
        Box::new(crate::provider::google::GoogleProvider::new(key))
    });

    register_config_provider(
        registry,
        providers,
        disabled,
        "openrouter",
        |key, _base_url| Box::new(crate::provider::openrouter::OpenRouterProvider::new(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "opencode_zen",
        "OPENCODE_ZEN_API_KEY",
        |key| Box::new(crate::provider::opencode_zen::OpencodeZenProvider::new(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "mistral",
        "MISTRAL_API_KEY",
        |key| Box::new(crate::provider::additional::create_mistral(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "groq",
        "GROQ_API_KEY",
        |key| Box::new(crate::provider::additional::create_groq(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "deepinfra",
        "DEEPINFRA_API_KEY",
        |key| Box::new(crate::provider::additional::create_deepinfra(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "cerebras",
        "CEREBRAS_API_KEY",
        |key| Box::new(crate::provider::additional::create_cerebras(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "cohere",
        "COHERE_API_KEY",
        |key| Box::new(crate::provider::additional::create_cohere(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "together",
        "TOGETHERAI_API_KEY",
        |key| Box::new(crate::provider::additional::create_together(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "perplexity",
        "PERPLEXITY_API_KEY",
        |key| Box::new(crate::provider::additional::create_perplexity(key)),
    );

    register_env_fallback_provider(registry, providers, disabled, "xai", "XAI_API_KEY", |key| {
        Box::new(crate::provider::additional::create_xai(key))
    });

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "venice",
        "VENICE_API_KEY",
        |key| Box::new(crate::provider::additional::create_venice(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "minimax",
        "MINIMAX_API_KEY",
        |key| Box::new(crate::provider::additional::create_minimax(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "opencode_go",
        "OPENCODE_GO_API_KEY",
        |key| Box::new(crate::provider::additional::create_opencode_go(key)),
    );

    register_env_fallback_provider(
        registry,
        providers,
        disabled,
        "generalcompute",
        "GENERALCOMPUTE_API_KEY",
        |key| Box::new(crate::provider::additional::create_generalcompute(key)),
    );

    if registry.list().is_empty() {
        register_builtin(registry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new_is_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = ProviderRegistry::new();
        let provider = TestProvider;
        registry.register(provider);
        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list() {
        let mut registry = ProviderRegistry::new();
        registry.register(TestProvider);
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id(), "test");
    }

    #[test]
    fn test_registry_default_impl() {
        let registry = ProviderRegistry::default();
        assert!(registry.list().is_empty());
    }

    #[test]
    fn test_model_info() {
        let info = ModelInfo {
            id: "gpt-4".to_string(),
            name: "GPT-4".to_string(),
            provider: "openai".to_string(),
            context_window: 8192,
            max_output_tokens: Some(4096),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        };
        assert_eq!(info.id, "gpt-4");
        assert!(info.supports_tools);
    }

    #[test]
    fn test_tool_definition() {
        let def = ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            defer_loading: None,
        };
        assert_eq!(def.name, "test_tool");
    }

    #[test]
    fn test_message_system() {
        let msg = Message::System {
            content: "You are helpful".to_string().into(),
        };
        assert!(matches!(msg, Message::System { .. }));
    }

    #[test]
    fn test_message_user() {
        let msg = Message::User {
            content: vec![ContentPart::Text {
                text: "Hello".to_string().into(),
            }],
        };
        assert!(matches!(msg, Message::User { .. }));
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::Assistant {
            content: vec![ContentPart::Text {
                text: "Hi there".to_string().into(),
            }],
            tool_calls: vec![],
        };
        assert!(matches!(msg, Message::Assistant { .. }));
    }

    #[test]
    fn test_message_tool() {
        let msg = Message::Tool {
            tool_call_id: "call_1".to_string().into(),
            content: "result".to_string().into(),
        };
        assert!(matches!(msg, Message::Tool { .. }));
    }

    #[test]
    fn test_content_part_text() {
        let part = ContentPart::Text {
            text: "hello".to_string().into(),
        };
        assert!(matches!(part, ContentPart::Text { .. }));
    }

    #[test]
    fn test_chat_event_text_delta() {
        let event = ChatEvent::TextDelta("hello".to_string().into());
        assert!(matches!(event, ChatEvent::TextDelta(_)));
    }

    #[test]
    fn test_chat_event_finish() {
        let event = ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        };
        assert!(matches!(event, ChatEvent::Finish { .. }));
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_chat_request() {
        let req = ChatRequest {
            messages: vec![],
            model: "test/model".to_string(),
            tools: None,
            system: Some("system".to_string()),
            temperature: Some(0.7),
            top_p: None,
            max_tokens: Some(1024),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
        };
        assert_eq!(req.model, "test/model");
    }

    struct TestProvider;

    #[async_trait]
    impl Provider for TestProvider {
        fn id(&self) -> &str {
            "test"
        }
        fn name(&self) -> &str {
            "Test"
        }
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(TestProvider)
        }
        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            Ok(Box::pin(futures::stream::empty()))
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }
}
