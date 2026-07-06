//! LLM provider interface and implementations.
//!
//! This module provides the Provider trait for interacting with various LLM backends
//! including Anthropic, OpenAI, Google Vertex, AWS Bedrock, and more. Providers handle
//! authentication, request formatting, streaming responses, and error handling.

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::auth_types::{AuthResolver, ResolvedAuth, ResolverContext};
use crate::auth_types::{Credential, CredentialKind, CredentialStore};
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
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    pub supports_defer_loading: bool,
    pub supports_tool_references: bool,
    pub max_tools_per_request: Option<usize>,
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
        registry.register(crate::anthropic::AnthropicProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        let cfg = crate::openai::OpenAiConfig::default_with_key(key);
        registry.register(crate::openai::OpenAiProvider::new(cfg));
    }
    if let Ok(key) = std::env::var("GOOGLE_API_KEY") {
        registry.register(crate::google::GoogleProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        registry.register(crate::openrouter::OpenRouterProvider::new(key));
    }
    if let Ok(key) = std::env::var("OPENCODE_ZEN_API_KEY") {
        registry.register(crate::opencode_zen::OpencodeZenProvider::new(key));
    }
    if let Ok(key) = std::env::var("MISTRAL_API_KEY") {
        registry.register(crate::additional::create_mistral(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        registry.register(crate::additional::create_groq(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("DEEPINFRA_API_KEY") {
        registry.register(crate::additional::create_deepinfra(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("CEREBRAS_API_KEY") {
        registry.register(crate::additional::create_cerebras(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("COHERE_API_KEY") {
        registry.register(crate::additional::create_cohere(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("TOGETHERAI_API_KEY") {
        registry.register(crate::additional::create_together(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("PERPLEXITY_API_KEY") {
        registry.register(crate::additional::create_perplexity(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("XAI_API_KEY") {
        registry.register(crate::additional::create_xai(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("VENICE_API_KEY") {
        registry.register(crate::additional::create_venice(
            crate::auth_types::Credential::api_key(key),
        ));
    }
    if let Ok(key) = std::env::var("MINIMAX_API_KEY") {
        registry.register(crate::additional::create_minimax(key));
    }
}

/// Centralized credential resolution for provider registration.
///
/// Builds a [`ResolverContext`] from the legacy [`ProviderConfig`] fields
/// and a shared [`CredentialStore`], then calls
/// [`AuthResolver::resolve`]. The full [`ResolvedAuth`] is returned so the
/// caller can inspect the [`CredentialKind`] / `expires_at` metadata, not
/// just the secret.
pub(crate) fn resolve_provider_credential(
    provider_id: &str,
    cfg: Option<&codegg_config::schema::ProviderConfig>,
    env_var: Option<&str>,
    store: Option<&std::sync::Arc<CredentialStore>>,
) -> Result<Option<ResolvedAuth>, crate::auth_types::AuthError> {
    let resolver = AuthResolver::new();
    let ctx = ResolverContext {
        provider_id: provider_id.to_string(),
        account_id: cfg.and_then(|c| c.account_id.clone()),
        legacy_api_key: cfg.and_then(|c| c.api_key.clone()),
        legacy_decrypted: None,
        env_override: env_var.map(|s| s.to_string()),
        store: store.cloned(),
    };
    let auth_ref = cfg.and_then(|c| c.auth.as_ref()).map(|a| match a {
        codegg_config::schema::AuthConfig::ApiKey {
            env,
            value,
            encrypted_value,
        } => crate::auth_types::AuthConfig::ApiKey {
            env: env.clone(),
            value: value.clone(),
            encrypted_value: encrypted_value.clone(),
        },
        codegg_config::schema::AuthConfig::Stored { account_id } => {
            crate::auth_types::AuthConfig::Stored {
                account_id: account_id.clone(),
            }
        }
        codegg_config::schema::AuthConfig::ExternalCommand {
            command,
            args,
            timeout_ms,
        } => crate::auth_types::AuthConfig::ExternalCommand {
            command: command.clone(),
            args: args.clone(),
            timeout_ms: *timeout_ms,
        },
        codegg_config::schema::AuthConfig::OAuthDevice {
            client_id,
            scopes,
            auth_url,
            token_url,
        } => crate::auth_types::AuthConfig::OAuthDevice {
            client_id: client_id.clone(),
            scopes: scopes.clone(),
            auth_url: auth_url.clone(),
            token_url: token_url.clone(),
        },
        codegg_config::schema::AuthConfig::None => crate::auth_types::AuthConfig::None,
    });
    resolver.resolve(auth_ref.as_ref(), &ctx)
}

/// Convert a [`ResolvedAuth`] into a [`Credential::api_key`] credential,
/// warning if a bearer-style credential was supplied to a path that only
/// supports static API keys.
fn ensure_api_key_credential(name: &str, resolved: &ResolvedAuth) -> Option<Credential> {
    if resolved.credential.secret.is_empty() {
        return None;
    }
    match resolved.credential.kind {
        CredentialKind::ApiKey => Some(resolved.credential.clone()),
        CredentialKind::BearerToken => {
            tracing::warn!(
                "register_api_key_provider: provider '{}' resolved a bearer token but this provider path only supports API-key credentials; skipping",
                name
            );
            None
        }
    }
}

/// Register a provider that accepts a full [`Credential`] envelope.
///
/// This is the preferred path for OpenAI-compatible providers. It
/// preserves `CredentialKind` (api key vs. bearer token) and any
/// `expires_at` metadata at construction time.
fn register_credential_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&std::collections::HashMap<String, codegg_config::schema::ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    store: Option<&std::sync::Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(Credential) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, Some(env_var), store) {
            Ok(Some(resolved)) => {
                if !resolved.credential.secret.is_empty() {
                    tracing::debug!(
                        "register_credential_provider: registering provider '{}' via {}",
                        name,
                        resolved.source.as_str()
                    );
                    registry.register(factory(resolved.credential));
                } else {
                    tracing::warn!(
                        "register_credential_provider: NO KEY for provider '{}', env_var='{}' (empty resolved key)",
                        name,
                        env_var
                    );
                }
            }
            Ok(None) => {
                tracing::warn!(
                    "register_credential_provider: NO KEY for provider '{}', env_var='{}' (no credential configured)",
                    name,
                    env_var
                );
            }
            Err(e) => {
                // Recognize-but-unimplemented auth modes (e.g. OAuthDevice,
                // ExternalCommand) should not prevent other providers from
                // loading. Log and skip.
                tracing::warn!(
                    "register_credential_provider: provider '{}' could not be resolved: {}",
                    name,
                    e
                );
            }
        }
    }
}

/// Register a provider that only accepts a static API-key secret.
///
/// Resolves a credential through [`resolve_provider_credential`], then
/// collapses it to a `String` for the factory. Bearer-style credentials
/// produce a warning and the provider is skipped, since callers of this
/// helper genuinely need a static API key (e.g. they use a non-Bearer
/// auth header like `x-api-key`).
fn register_api_key_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&std::collections::HashMap<String, codegg_config::schema::ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    store: Option<&std::sync::Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(String) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, Some(env_var), store) {
            Ok(Some(resolved)) => {
                if let Some(cred) = ensure_api_key_credential(name, &resolved) {
                    tracing::debug!(
                        "register_api_key_provider: registering provider '{}' via {}",
                        name,
                        resolved.source.as_str()
                    );
                    registry.register(factory(cred.secret));
                }
            }
            Ok(None) => {
                tracing::warn!(
                    "register_api_key_provider: NO KEY for provider '{}', env_var='{}' (no credential configured)",
                    name,
                    env_var
                );
            }
            Err(e) => {
                tracing::warn!(
                    "register_api_key_provider: provider '{}' could not be resolved: {}",
                    name,
                    e
                );
            }
        }
    }
}

/// Backwards-compatible registration helper for config-driven providers
/// that have a `base_url` override. Resolves a credential and threads the
/// (possibly absent) `base_url` through to the factory. Uses the
/// [`register_api_key_provider`] path because all current callers
/// (Anthropic, OpenAI native, Google, OpenRouter) use static API keys.
///
/// All credential lookups flow through
/// [`resolve_provider_credential`]; the resolver itself is responsible
/// for honoring `ctx.legacy_api_key`. This helper does **not** read
/// `cfg.api_key` directly so there is a single resolution path.
fn register_config_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&std::collections::HashMap<String, codegg_config::schema::ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    store: Option<&std::sync::Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(String, Option<String>) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, None, store) {
            Ok(Some(resolved)) => {
                if let Some(cred) = ensure_api_key_credential(name, &resolved) {
                    tracing::debug!(
                        "register_config_provider: provider '{}' resolved via {}",
                        name,
                        resolved.source.as_str()
                    );
                    let base_url = cfg_opt.and_then(|c| c.base_url.clone());
                    registry.register(factory(cred.secret, base_url));
                }
            }
            Ok(None) => {
                tracing::debug!(
                    "register_config_provider: provider '{}' had no credential configured",
                    name
                );
            }
            Err(e) => {
                tracing::warn!(
                    "register_config_provider: provider '{}' could not be resolved: {}",
                    name,
                    e
                );
            }
        }
    }
}

pub fn register_builtin_with_config(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
) {
    let providers = config.provider.as_ref();
    let disabled = config.disabled_providers.as_ref();

    // Build a single, shared credential store. A failure to construct the
    // store does not abort registration: env-var and inline API-key paths
    // still work without it; only `AuthConfig::Stored` becomes unavailable.
    let store: Option<std::sync::Arc<CredentialStore>> = CredentialStore::at_default_location()
        .map(std::sync::Arc::new)
        .map_err(|e| {
            tracing::warn!(
                "register_builtin_with_config: could not open user credential store: {e}"
            );
            e
        })
        .ok();

    register_config_provider(
        registry,
        providers,
        disabled,
        "anthropic",
        store.as_ref(),
        |key, base_url| {
            let mut p = crate::anthropic::AnthropicProvider::new(key);
            if let Some(url) = base_url {
                p = p.with_base_url(url);
            }
            Box::new(p)
        },
    );

    register_config_provider(
        registry,
        providers,
        disabled,
        "openai",
        store.as_ref(),
        |key, base_url| {
            let mut cfg = crate::openai::OpenAiConfig::default_with_key(key);
            if let Some(url) = base_url {
                cfg.base_url = url;
            }
            Box::new(crate::openai::OpenAiProvider::new(cfg))
        },
    );

    register_config_provider(
        registry,
        providers,
        disabled,
        "google",
        store.as_ref(),
        |key, _base_url| Box::new(crate::google::GoogleProvider::new(key)),
    );

    register_config_provider(
        registry,
        providers,
        disabled,
        "openrouter",
        store.as_ref(),
        |key, _base_url| Box::new(crate::openrouter::OpenRouterProvider::new(key)),
    );

    register_api_key_provider(
        registry,
        providers,
        disabled,
        "opencode_zen",
        "OPENCODE_ZEN_API_KEY",
        store.as_ref(),
        |key| Box::new(crate::opencode_zen::OpencodeZenProvider::new(key)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "mistral",
        "MISTRAL_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_mistral(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "groq",
        "GROQ_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_groq(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "deepinfra",
        "DEEPINFRA_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_deepinfra(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "cerebras",
        "CEREBRAS_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_cerebras(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "cohere",
        "COHERE_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_cohere(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "together",
        "TOGETHERAI_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_together(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "perplexity",
        "PERPLEXITY_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_perplexity(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "xai",
        "XAI_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_xai(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "venice",
        "VENICE_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_venice(cred)),
    );

    register_api_key_provider(
        registry,
        providers,
        disabled,
        "minimax",
        "MINIMAX_API_KEY",
        store.as_ref(),
        |key| Box::new(crate::additional::create_minimax(key)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "opencode_go",
        "OPENCODE_GO_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_opencode_go(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "generalcompute",
        "GENERALCOMPUTE_API_KEY",
        store.as_ref(),
        |cred| Box::new(crate::additional::create_generalcompute(cred)),
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

    // ---- Credential resolution tests ----

    use crate::auth_types::CredentialStore;
    use crate::auth_types::ResolvedAuthSource;
    use codegg_config::schema::{AuthConfig, Config, ProviderConfig};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_provider_config() -> ProviderConfig {
        ProviderConfig {
            auth: Some(AuthConfig::ApiKey {
                env: None,
                value: Some("inline-key".to_string()),
                encrypted_value: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_provider_credential_reads_inline_value() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev = std::env::var("RESOLVE_TEST_PROVIDER_API_KEY").ok();
        std::env::remove_var("RESOLVE_TEST_PROVIDER_API_KEY");
        let cfg = make_provider_config();
        let resolved = resolve_provider_credential("resolve_test_provider", Some(&cfg), None, None)
            .expect("ok")
            .expect("some");
        assert_eq!(resolved.credential.secret, "inline-key");
        assert_eq!(resolved.credential.kind, CredentialKind::ApiKey);
        if let Some(v) = prev {
            std::env::set_var("RESOLVE_TEST_PROVIDER_API_KEY", v);
        }
    }

    #[test]
    fn resolve_provider_credential_reads_from_store_when_stored() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
        let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "resolve-store-test-master");
        std::env::remove_var("RESOLVE_TEST_PROVIDER_API_KEY");

        let tmp = tempfile::tempdir().expect("tmpdir");
        let store =
            Arc::new(CredentialStore::at_path(tmp.path().join("credentials.json")).expect("store"));
        store
            .put(
                "resolve_test_provider",
                Some("acct-1"),
                CredentialKind::ApiKey,
                "stored-key",
                None,
                vec![],
            )
            .expect("put");

        let cfg = ProviderConfig {
            auth: Some(AuthConfig::Stored {
                account_id: Some("acct-1".to_string()),
            }),
            ..Default::default()
        };
        let resolved =
            resolve_provider_credential("resolve_test_provider", Some(&cfg), None, Some(&store))
                .expect("ok")
                .expect("some");
        assert_eq!(resolved.credential.secret, "stored-key");
        assert_eq!(resolved.credential.kind, CredentialKind::ApiKey);
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);

        // Restore env
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        if let Some(v) = prev_enc {
            std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
        } else {
            std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        }
        if let Some(v) = prev_opencode {
            std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
        } else {
            std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
        }
    }

    #[test]
    fn register_builtin_with_config_registers_via_env_var() {
        let _guard = crate::auth_types::test_support::lock_env();
        // Use a unique env var so the test does not depend on the host's
        // existing XAI_API_KEY / OPENAI_API_KEY.
        let prev_xai = std::env::var("XAI_API_KEY").ok();
        std::env::set_var("XAI_API_KEY", "xai-env-key");

        let config = Config::default();
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        assert!(
            registry.get("xai").is_some(),
            "xai should be registered from XAI_API_KEY"
        );

        if let Some(v) = prev_xai {
            std::env::set_var("XAI_API_KEY", v);
        } else {
            std::env::remove_var("XAI_API_KEY");
        }
    }

    #[test]
    fn register_builtin_with_config_uses_typed_auth_inline() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_xai = std::env::var("XAI_API_KEY").ok();
        std::env::remove_var("XAI_API_KEY");

        let mut providers = HashMap::new();
        providers.insert(
            "xai".to_string(),
            ProviderConfig {
                auth: Some(AuthConfig::ApiKey {
                    env: None,
                    value: Some("inline-xai-key".to_string()),
                    encrypted_value: None,
                }),
                ..Default::default()
            },
        );
        let config = Config {
            provider: Some(providers),
            ..Default::default()
        };
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        assert!(
            registry.get("xai").is_some(),
            "xai should be registered from typed auth descriptor"
        );

        if let Some(v) = prev_xai {
            std::env::set_var("XAI_API_KEY", v);
        }
    }

    #[test]
    fn register_builtin_with_config_stored_credential_works() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_xai = std::env::var("XAI_API_KEY").ok();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::remove_var("XAI_API_KEY");
        std::env::set_var("CODEGG_MASTER_KEY", "stored-xai-test-master");

        // Use HOME to point at a temp config dir so the production
        // credential-store path is hermetic.
        let tmp = tempfile::tempdir().expect("tmpdir");
        let prev_home = std::env::var("HOME").ok();
        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));

        // Seed the store directly via the in-process API.
        let store_path = dirs::config_dir()
            .map(|d| d.join("codegg").join("credentials.json"))
            .expect("config dir");
        let store = CredentialStore::at_path(store_path.clone()).expect("store");
        store
            .put(
                "xai",
                Some("default"),
                CredentialKind::ApiKey,
                "stored-xai-key",
                None,
                vec![],
            )
            .expect("put");

        let mut providers = HashMap::new();
        providers.insert(
            "xai".to_string(),
            ProviderConfig {
                auth: Some(AuthConfig::Stored {
                    account_id: Some("default".to_string()),
                }),
                ..Default::default()
            },
        );
        let config = Config {
            provider: Some(providers),
            ..Default::default()
        };
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        assert!(
            registry.get("xai").is_some(),
            "xai should be registered from a Stored credential"
        );

        // Cleanup
        let _ = std::fs::remove_file(&store_path);
        if let Some(v) = prev_xai {
            std::env::set_var("XAI_API_KEY", v);
        }
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        if let Some(v) = prev_home {
            std::env::set_var("HOME", v);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(v) = prev_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn register_api_key_provider_rejects_bearer_credentials() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_minimax = std::env::var("MINIMAX_API_KEY").ok();
        std::env::remove_var("MINIMAX_API_KEY");
        // No env, no config: resolution should not find a key at all,
        // and the provider should not be registered.
        let config = Config::default();
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        assert!(registry.get("minimax").is_none());
        if let Some(v) = prev_minimax {
            std::env::set_var("MINIMAX_API_KEY", v);
        }
    }

    #[test]
    fn ensure_api_key_credential_rejects_bearer_token() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev = std::env::var("ENSURE_BEARER_TEST_API_KEY").ok();
        std::env::remove_var("ENSURE_BEARER_TEST_API_KEY");
        let prev_openai = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");

        let resolved = ResolvedAuth {
            credential: Credential::bearer("a-bearer", None),
            source: ResolvedAuthSource::UserStore,
        };
        // The helper must NOT collapse a bearer credential to a string
        // for API-key-only providers.
        assert!(ensure_api_key_credential("bearer_only_provider", &resolved).is_none());

        let resolved_api = ResolvedAuth {
            credential: Credential::api_key("an-api-key"),
            source: ResolvedAuthSource::EnvConventional,
        };
        let got = ensure_api_key_credential("api_key_provider", &resolved_api)
            .expect("api-key credential must pass through");
        assert_eq!(got.secret, "an-api-key");
        assert_eq!(got.kind, CredentialKind::ApiKey);

        if let Some(v) = prev {
            std::env::set_var("ENSURE_BEARER_TEST_API_KEY", v);
        }
        if let Some(v) = prev_openai {
            std::env::set_var("OPENAI_API_KEY", v);
        }
    }

    #[test]
    fn register_builtin_with_config_uses_legacy_api_key_through_resolver() {
        // Phase 1: register_config_provider no longer reads
        // `cfg.api_key` directly. The legacy field must still register
        // providers via the resolver path (which inspects
        // `ctx.legacy_api_key`). Use the `anthropic` slot, which is
        // one of the providers routed through register_config_provider.
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_anthropic = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_xai = std::env::var("XAI_API_KEY").ok();
        let prev_openai = std::env::var("OPENAI_API_KEY").ok();
        let prev_google = std::env::var("GOOGLE_API_KEY").ok();
        let prev_openrouter = std::env::var("OPENROUTER_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("XAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("OPENROUTER_API_KEY");

        let mut providers = HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                api_key: Some("legacy-anthropic-key".to_string()),
                ..Default::default()
            },
        );
        let config = Config {
            provider: Some(providers),
            ..Default::default()
        };
        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        assert!(
            registry.get("anthropic").is_some(),
            "legacy api_key must still register through the resolver"
        );

        if let Some(v) = prev_anthropic {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
        if let Some(v) = prev_xai {
            std::env::set_var("XAI_API_KEY", v);
        }
        if let Some(v) = prev_openai {
            std::env::set_var("OPENAI_API_KEY", v);
        }
        if let Some(v) = prev_google {
            std::env::set_var("GOOGLE_API_KEY", v);
        }
        if let Some(v) = prev_openrouter {
            std::env::set_var("OPENROUTER_API_KEY", v);
        }
    }

    #[test]
    fn openai_compatible_factory_preserves_bearer_kind() {
        use crate::openai_compatible::OpenAiCompatibleProvider;

        let cred = Credential::bearer("short-lived-token", None);
        let provider = OpenAiCompatibleProvider::simple_with_credential(
            "bearer_kind_test",
            "Bearer Kind Test",
            cred,
            "https://example.invalid/v1",
        );
        assert_eq!(provider.config.credential.kind, CredentialKind::BearerToken);
        assert_eq!(provider.config.credential.secret, "short-lived-token");

        let cred2 = Credential::api_key("sk-static-key");
        let provider2 = OpenAiCompatibleProvider::simple_with_credential(
            "api_key_kind_test",
            "ApiKey Kind Test",
            cred2,
            "https://example.invalid/v1",
        );
        assert_eq!(provider2.config.credential.kind, CredentialKind::ApiKey);
        assert_eq!(provider2.config.credential.secret, "sk-static-key");
    }
}
