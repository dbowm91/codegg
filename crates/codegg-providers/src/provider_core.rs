//! LLM provider interface and implementations.
//!
//! This module provides the Provider trait for interacting with various LLM backends
//! including Anthropic, OpenAI, Google Vertex, AWS Bedrock, and more. Providers handle
//! authentication, request formatting, streaming responses, and error handling.

use async_trait::async_trait;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub use crate::auth_types::CredentialCapability as ProviderCredentialCapability;
use crate::auth_types::{AuthResolver, ResolvedAuth, ResolverContext};
use crate::auth_types::{Credential, CredentialCapability, CredentialKind, CredentialStore};
pub use crate::error::ProviderError;

pub const MAX_BUFFER_SIZE: usize = 1024 * 1024;

/// Finite total deadline for bounded non-streaming provider operations
/// (model discovery/listing and other ordinary provider metadata calls).
///
/// This is crate-internal transport-construction policy: sibling provider
/// modules apply it per request via `crate::provider_core::non_streaming_timeout()`.
/// It is not part of the supported external `codegg-providers` API.
///
/// The shared streaming-capable client deliberately carries no
/// client-level absolute `total`: Eggfetch 0.2.0 enforces `Timeout.total`
/// as one absolute deadline through response-body EOF (including
/// `bytes_stream()`), so a client-global 60s total would terminate healthy
/// long model streams. Long-stream liveness is owned by the agent layer
/// (`STREAM_SETUP_TIMEOUT = 120s`, `STREAM_IDLE_TIMEOUT = 90s`,
/// cancellation, retry-chain budget). Non-streaming call sites apply this
/// bound explicitly per request via `non_streaming_timeout()`.
pub(crate) const NON_STREAMING_PROVIDER_TOTAL: Duration = Duration::from_secs(60);

/// Request-level timeout for finite non-streaming provider operations.
///
/// Crate-internal policy helper. Returns a `Timeout` carrying only
/// `total = 60s`; all other phases are `None` so the shared client's
/// connect/pool policy is inherited via Eggfetch's per-field merge
/// (request `None` keeps the client value).
pub(crate) fn non_streaming_timeout() -> eggfetch_core::Timeout {
    eggfetch_core::Timeout {
        total: Some(NON_STREAMING_PROVIDER_TOTAL),
        ..eggfetch_core::Timeout::default()
    }
}

pub fn create_http_client() -> eggfetch_core::Client {
    eggfetch_core::Client::builder()
        .timeout(
            eggfetch_core::Timeout::builder()
                .connect(Duration::from_secs(10))
                .build(),
        )
        .max_idle_connections_per_host(32)
        .idle_timeout(Duration::from_secs(30))
        .follow_redirects(true)
        .max_redirects(10)
        .build()
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
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
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

/// Provider capabilities for tool deferral, request limits, and
/// hosted programmatic tool calling.
///
/// Determines which providers support deferred tool loading, tool references,
/// and the Responses API with hosted program support, allowing the agent loop
/// to partition tools into immediate vs deferred arrays and select the
/// appropriate execution backend.
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    pub supports_defer_loading: bool,
    pub supports_tool_references: bool,
    pub max_tools_per_request: Option<usize>,
    /// Whether the provider supports the Responses API format.
    pub supports_responses_api: bool,
    /// Whether the provider supports hosted programmatic tool calling
    /// (provider-executed programs with client-owned nested function calls).
    pub supports_hosted_programs: bool,
    /// Whether the provider supports client-owned nested function calls
    /// within hosted programs.
    pub supports_client_owned_nested_calls: bool,
    /// Whether the provider supports background/continuation for hosted programs.
    pub supports_hosted_continuation: bool,
    /// Supported hosted program languages (e.g. "python", "javascript").
    pub hosted_languages: Vec<String>,
    /// Maximum number of items allowed in a single Responses request.
    pub max_response_items: Option<usize>,
    /// Maximum number of nested function calls per response.
    pub max_nested_calls: Option<usize>,
    /// Whether the provider requires a fingerprint for continuation.
    pub requires_fingerprint: bool,
    /// Whether the provider supports output-schema / strict-function behavior.
    pub supports_output_schema: bool,
    /// Maximum result payload size in bytes for a single nested call result.
    pub max_result_size: Option<usize>,
    /// Maximum number of tool calls per hosted program execution.
    pub max_tool_calls_per_program: Option<usize>,
    /// Whether this provider's request adapter *removes* `reasoning_effort`
    /// from the outgoing body.
    ///
    /// True exactly for the adapters that call
    /// `wire::omit_openai_fields(.., ["response_format", "reasoning_effort"])`
    /// — Azure, OpenRouter, and OpenCode Zen. A user who selects an effort or
    /// a thinking budget on such a provider gets silence, not an error, so the
    /// frontend needs this to warn rather than let the setting look broken.
    ///
    /// This is deliberately NOT the inverse of "supports reasoning": most
    /// providers simply pass the field through without declaring anything, so
    /// `false` means "does not strip", never "cannot reason".
    pub strips_reasoning_effort: bool,
}

impl ProviderCapabilities {
    /// Get capabilities for a specific provider by ID.
    ///
    /// Conservative defaults: providers without explicit support
    /// default to not supporting deferral or hosted programs.
    pub fn for_provider(provider_id: &str) -> Self {
        match provider_id {
            "anthropic" => Self {
                supports_defer_loading: true,
                supports_tool_references: true,
                max_tools_per_request: None,
                ..Default::default()
            },
            "openai" => Self {
                supports_defer_loading: true,
                supports_tool_references: true,
                max_tools_per_request: Some(128),
                supports_responses_api: true,
                supports_hosted_programs: true,
                supports_client_owned_nested_calls: true,
                supports_hosted_continuation: true,
                hosted_languages: vec!["python".to_string()],
                max_response_items: Some(100),
                max_nested_calls: Some(50),
                requires_fingerprint: false,
                supports_output_schema: true,
                max_result_size: Some(1024 * 1024), // 1 MB
                max_tool_calls_per_program: Some(50),
                // Passes `reasoning_effort` through to the wire.
                strips_reasoning_effort: false,
            },
            // These three adapters strip `reasoning_effort` from the body
            // before it goes on the wire. Keep this list in lockstep with the
            // `omit_openai_fields` calls in azure.rs / openrouter.rs /
            // opencode_zen.rs.
            "azure" | "openrouter" | "opencode_zen" => Self {
                strips_reasoning_effort: true,
                ..Default::default()
            },
            _ => Self::default(),
        }
    }

    /// Whether this provider can execute hosted programs.
    pub fn can_host_programs(&self) -> bool {
        self.supports_responses_api && self.supports_hosted_programs
    }

    /// Whether this provider supports the full hosted program lifecycle
    /// (continuation, nested calls, background).
    pub fn full_hosted_support(&self) -> bool {
        self.can_host_programs()
            && self.supports_client_owned_nested_calls
            && self.supports_hosted_continuation
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRequestContext {
    /// Canonical conversation identity projected by the owning runtime.
    ///
    /// This is deliberately narrower than a header map: providers may use it
    /// only for an explicitly configured transport policy.
    pub session_id: Option<Arc<str>>,
    /// Immutable, conservative model-adapter projection for provider wire
    /// encoding. It carries no endpoint, credential, or arbitrary headers.
    pub wire_policy: Option<Arc<ProviderWirePolicy>>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderWirePolicy {
    /// Canonical tool name to provider wire name.
    pub tool_aliases: BTreeMap<String, String>,
    /// Wire tool name to canonical argument name to wire argument name.
    pub argument_aliases: BTreeMap<String, BTreeMap<String, String>>,
    /// Whether private provider reasoning may round-trip in assistant history.
    pub allow_private_reasoning_round_trip: bool,
    /// Closed Laguna-compatible reasoning parameter projection.
    pub include_reasoning_content: bool,
    pub enable_thinking: Option<bool>,
    pub tool_choice: Option<String>,
    pub max_parallel_tools: Option<usize>,
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
    pub context: ProviderRequestContext,
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

/// Project canonical history into the provider wire grammar without mutating
/// the messages retained by the agent/session. Some providers require every
/// assistant tool call to have a following result; an interrupted turn does
/// not make that result a durable fact, so the placeholder exists only in the
/// serialized request.
pub fn project_tool_call_history(messages: &[Message]) -> Vec<Message> {
    use std::collections::BTreeMap;

    let mut projected = Vec::with_capacity(messages.len() + 8);
    let mut pending = BTreeMap::<String, ()>::new();

    let flush = |projected: &mut Vec<Message>, pending: &mut BTreeMap<String, ()>| {
        for id in pending.keys() {
            projected.push(Message::Tool {
                tool_call_id: id.clone().into(),
                content: "[tool result missing due to interrupted history]"
                    .to_string()
                    .into(),
            });
        }
        pending.clear();
    };

    for message in messages {
        match message {
            Message::Assistant { tool_calls, .. } => {
                flush(&mut projected, &mut pending);
                for call in tool_calls {
                    pending.insert(call.id.to_string(), ());
                }
                projected.push(message.clone());
            }
            Message::Tool { tool_call_id, .. } => {
                pending.remove(tool_call_id.as_ref());
                projected.push(message.clone());
            }
            Message::User { .. } | Message::System { .. } => {
                flush(&mut projected, &mut pending);
                projected.push(message.clone());
            }
        }
    }
    flush(&mut projected, &mut pending);
    projected
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text {
        text: Arc<String>,
    },
    Image {
        image_url: ImageUrl,
    },
    /// Provider-private reasoning that may be sent back to a compatible
    /// model, but must never be treated as visible assistant text.
    Reasoning {
        #[serde(skip)]
        text: Arc<String>,
        visibility: ReasoningVisibility,
    },
}

impl fmt::Debug for ContentPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { text } => f.debug_struct("Text").field("text", text).finish(),
            Self::Image { image_url } => f
                .debug_struct("Image")
                .field("image_url", image_url)
                .finish(),
            Self::Reasoning { text, visibility } => f
                .debug_struct("Reasoning")
                .field("bytes", &text.len())
                .field("content", &"<private>")
                .field("visibility", visibility)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningVisibility {
    Private,
}

pub const MAX_REASONING_BYTES: usize = 256 * 1024;

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
    /// Keyed by provider id in **sorted** order.
    ///
    /// `ProviderRegistry::list()` feeds the model catalog, so its order decides
    /// which provider's models come first — and consumers fall back to
    /// `models[0]` when they have no explicit selection. A `HashMap` iterates
    /// in `RandomState` order, which is reseeded per process, so the "first"
    /// provider changed on every launch. That silently picked a different
    /// provider (and a different model) each run, which in turn routed turns to
    /// whichever backend hashed first. Sorted keys make the catalog stable.
    providers: BTreeMap<String, Box<dyn Provider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: BTreeMap::new(),
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

/// Executable provider × credential capability matrix.
///
/// The setup catalog in [`crate::setup_catalog`] is the single source of
/// truth: every built-in registration branch in `register_builtin_with_config`
/// must have a definition there, and this function reports that definition's
/// capability. Unknown or future providers default to the conservative
/// contract: explicit rejection rather than silent bearer reinterpretation.
///
/// - `ApiKeyOrBearer`: full-`Credential` OpenAI-compatible paths that
///   preserve `CredentialKind` through `OpenAiCompatibleProvider` and send
///   `Authorization: Bearer …` for either kind.
/// - `ApiKeyOnly`: `String`-contract factories (config-aware Anthropic /
///   OpenAI-native / Google / OpenRouter, plus OpenCode Zen and MiniMax).
///   Stored bearer records are rejected explicitly before transport.
///   OpenAI-native, OpenRouter, and Zen wire `Authorization: Bearer …` for
///   their long-lived API keys, but the factory contract remains a static
///   API-key string for this milestone; bearer lifecycle (expiry/short-lived
///   tokens) is not claimed for those paths.
pub fn credential_capability_for(provider_id: &str) -> CredentialCapability {
    crate::setup_catalog::setup_definition(provider_id)
        .map(|definition| definition.credential_capability)
        .unwrap_or(CredentialCapability::ApiKeyOnly)
}

/// All provider ids registered by `register_builtin_with_config`, in
/// registration order. Used by the capability-coverage test so a new branch
/// cannot land without a matrix entry.
pub fn builtin_registration_order() -> &'static [&'static str] {
    &[
        "anthropic",
        "openai",
        "google",
        "openrouter",
        "opencode_zen",
        "mistral",
        "groq",
        "deepinfra",
        "cerebras",
        "cohere",
        "together",
        "perplexity",
        "xai",
        "venice",
        "minimax",
        "opencode_go",
        "generalcompute",
    ]
}

/// Centralized credential resolution for provider registration.
///
/// Builds a [`ResolverContext`] from the legacy [`ProviderConfig`] fields
/// and a shared [`CredentialStore`], then calls
/// [`AuthResolver::resolve`]. The full [`ResolvedAuth`] is returned so the
/// caller can inspect the [`CredentialKind`] / `expires_at` metadata, not
/// just the secret.
///
/// The resolver context carries the target provider's
/// [`CredentialCapability`] from [`credential_capability_for`], so stored
/// bearer selection is centralized rather than branched per factory.
pub(crate) fn resolve_provider_credential(
    provider_id: &str,
    cfg: Option<&codegg_config::schema::ProviderConfig>,
    env_var: Option<&str>,
    store: Option<&std::sync::Arc<CredentialStore>>,
    pins: &[TurnCredentialPin],
) -> Result<Option<ResolvedAuth>, crate::auth_types::AuthError> {
    // A resolved provider connection is the authority for its own provider. This
    // check runs before config, env, and the store on purpose: rotation exists
    // precisely so a connection's key changes while the rest of the machine
    // (config, env, other stored records) still holds the old one, and any
    // later source could silently undo the rotation the user just performed.
    if let Some(pin) = pins.iter().find(|pin| pin.provider_id == provider_id) {
        if pin.credential.secret.is_empty() {
            return Ok(None);
        }
        tracing::debug!(
            "resolve_provider_credential: provider '{}' resolved from its bound provider connection account",
            provider_id
        );
        return Ok(Some(ResolvedAuth {
            credential: pin.credential.clone(),
            source: crate::auth_types::ResolvedAuthSource::ConnectionBinding,
        }));
    }
    let resolver = AuthResolver::new();
    // Decrypt any legacy `encrypted_api_key` so the resolver can prefer it
    // over a plaintext `api_key` when no env var or typed auth is configured.
    let legacy_decrypted = match cfg.and_then(|c| c.encrypted_api_key.as_deref()) {
        Some(enc) if !enc.is_empty() => match codegg_config::encryption::get_master_key() {
            Some(master) => match crate::crypto::decrypt_from_string(enc, &master) {
                Ok(plain) => Some(plain),
                Err(e) => {
                    tracing::warn!(
                            "resolve_provider_credential: failed to decrypt encrypted_api_key for provider '{}': {}",
                            provider_id,
                            e
                        );
                    None
                }
            },
            None => {
                tracing::warn!(
                        "resolve_provider_credential: encrypted_api_key present for provider '{}' but no usable master key (environment or managed master.key) is available; ignoring",
                        provider_id
                    );
                None
            }
        },
        _ => None,
    };
    let ctx = ResolverContext {
        provider_id: provider_id.to_string(),
        account_id: cfg.and_then(|c| c.account_id.clone()),
        legacy_api_key: cfg.and_then(|c| c.api_key.clone()),
        legacy_decrypted,
        env_override: env_var.map(|s| s.to_string()),
        store: store.cloned(),
        capability: credential_capability_for(provider_id),
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
    if let Some(resolved) = resolver.resolve(auth_ref.as_ref(), &ctx)? {
        return Ok(Some(resolved));
    }

    // Credential-store fallback.
    //
    // A credential added through the TUI `/connect` flow is written to the
    // credential store under the provider id and appears in neither config nor
    // the environment. Without this fallback the resolver returns `None`, the
    // provider is skipped during registration (`NO KEY for provider ...`), and
    // it therefore contributes no models — so connecting a provider through
    // the TUI appeared to have no effect at all: `/connect` reported success
    // while `/models` stayed empty.
    //
    // Config and env keep priority; this only supplies the missing case.
    // Expired records are treated as absent rather than as a bad key.
    //
    // Records are counted, not raced: `CredentialStore::list` is unsorted, so
    // taking the first match for a provider would authenticate with whatever
    // key was stored first — for a provider holding a superseded rotation
    // record plus a live one, the dead key wins every turn. One usable record
    // is unambiguous; two or more are refused with the candidate accounts
    // named so the operator can bind or remove one.
    let Some(store) = store else {
        return Ok(None);
    };
    let now = chrono::Utc::now();
    let usable: Vec<crate::auth_types::StoredCredentialRecord> = store
        .records_for_provider(provider_id)
        .into_iter()
        .filter(|record| record.expires_at.is_none_or(|expires_at| expires_at > now))
        .collect();
    let chosen = match usable.as_slice() {
        [] => return Ok(None),
        [only] => only,
        ambiguous => {
            let candidates = ambiguous
                .iter()
                .map(|record| match &record.account_id {
                    Some(account) => account.as_str(),
                    None => "no-account",
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(crate::auth_types::AuthError::AmbiguousCredential {
                provider_id: provider_id.to_string(),
                candidates,
            });
        }
    };
    let chosen = chosen.clone();
    let Ok(Some(secret)) = store.get_plaintext(provider_id, chosen.account_id.as_deref(), |_| true)
    else {
        return Ok(None);
    };
    if secret.is_empty() {
        return Ok(None);
    }
    tracing::debug!(
        "resolve_provider_credential: provider '{}' resolved from the credential store",
        provider_id
    );
    Ok(Some(ResolvedAuth {
        credential: crate::auth_types::Credential {
            kind: chosen.kind,
            secret,
            expires_at: chosen.expires_at,
        },
        source: crate::auth_types::ResolvedAuthSource::UserStore,
    }))
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
    scope: &RegistrationScope<'_>,
    factory: F,
) where
    F: FnOnce(Credential) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, Some(env_var), scope.store, scope.pins) {
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
                scope.failures.borrow_mut().push((name.to_string(), e));
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
    scope: &RegistrationScope<'_>,
    factory: F,
) where
    F: FnOnce(String) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, Some(env_var), scope.store, scope.pins) {
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
                scope.failures.borrow_mut().push((name.to_string(), e));
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
    scope: &RegistrationScope<'_>,
    factory: F,
) where
    F: FnOnce(String, Option<String>) -> Box<dyn Provider>,
{
    if disabled
        .map(|d| !d.contains(&name.to_string()))
        .unwrap_or(true)
    {
        let cfg_opt = providers.and_then(|p| p.get(name));
        match resolve_provider_credential(name, cfg_opt, None, scope.store, scope.pins) {
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
                scope.failures.borrow_mut().push((name.to_string(), e));
            }
        }
    }
}

/// Register every built-in provider from config, env, and the user store.
///
/// Surfaces are that cannot speak for a session (model listing, provider
/// pickers) use this entry point: a provider whose credential cannot be
/// resolved is logged and skipped, exactly as before. A turn must not use it
/// — [`register_builtin_with_config_for_turn`] is the turn-scoped authority
/// and reports an unresolvable credential instead of skipping it.
pub fn register_builtin_with_config(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
) {
    // Resolution errors are already logged by the per-provider helpers.
    let _ = register_builtins_with_store(registry, config, None, &[]);
}

/// Register the built-ins with connection authority for **every** provider.
///
/// [`register_builtin_with_config_for_turn`] binds exactly one provider because
/// a turn names exactly one. Process-wide paths — `codegg exec`, `/models`,
/// `codegg models`, the model pickers — have no turn and no session, so they used
/// to fall straight through to the credential-store fallback, which counts every
/// usable record for a provider and refuses once more than one exists.
///
/// That made a rotation permanently break those paths for the rotated provider:
/// the live record sits beside the orphan the rotation replaced, the count
/// reaches two, and the provider disappears from `codegg models` with
/// `Provider not found` even though a healthy `active` connection holds its
/// working key. Supplying every connection's locator here restores exactly the
/// rule a turn already obeys — a connection is the authority for its own
/// provider — so the same setup works from the CLI and the TUI.
///
/// Locators for providers the shared profile does not adapt are ignored, as is
/// any provider with no locator: those keep the pre-existing resolution order.
pub fn register_builtin_with_config_connections(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
    locators: &[(String, TurnCredentialLocator)],
    store: Option<std::sync::Arc<CredentialStore>>,
) -> Vec<(String, crate::auth_types::AuthError)> {
    if locators.is_empty() {
        return register_builtin_with_config_failures(registry, config, store);
    }
    let resolved = match store {
        Some(store) => Some(store),
        None => CredentialStore::at_default_location().map(Arc::new).ok(),
    };
    let Some(resolved) = resolved else {
        // Without a store there is nothing to pin from. Falling back to the
        // unbound pass is strictly better than registering nothing: it
        // reproduces the exact pre-existing behaviour, ambiguity refusal and all.
        return register_builtin_with_config_failures(registry, config, None);
    };
    let mut pins: Vec<TurnCredentialPin> = Vec::with_capacity(locators.len());
    for (provider_id, locator) in locators {
        let Ok(Some(credential)) =
            resolved.get_credential(&locator.provider_ref, Some(&locator.account_ref))
        else {
            continue;
        };
        // An expired bound record is dead material. It is left unpinned so the
        // ordinary resolution order reports it as missing rather than letting a
        // turn fail later as an opaque upstream 401.
        if credential
            .expires_at
            .is_some_and(|expires_at| expires_at <= chrono::Utc::now())
        {
            continue;
        }
        pins.push(TurnCredentialPin {
            provider_id: provider_id.clone(),
            credential,
        });
    }
    if pins.is_empty() {
        return register_builtin_with_config_failures(registry, config, None);
    }
    register_builtins_with_store(registry, config, Some(resolved), &pins)
}

fn register_builtin_with_config_failures(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
    store: Option<std::sync::Arc<CredentialStore>>,
) -> Vec<(String, crate::auth_types::AuthError)> {
    register_builtins_with_store(registry, config, store, &[])
}

/// One registration pass, returning every `(provider, error)` it hit so a
/// caller that owns user-facing error reporting can decide what to say.
///
/// `store` is the shared [`CredentialStore`]; `None` opens the default user
/// location, which is what every non-test caller wants. `pin` carries the
/// connection-bound credential for one provider (see [`TurnCredentialPin`]).
pub(crate) fn register_builtins_with_store(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
    store: Option<std::sync::Arc<CredentialStore>>,
    pins: &[TurnCredentialPin],
) -> Vec<(String, crate::auth_types::AuthError)> {
    let providers = config.provider.as_ref();
    let disabled = config.disabled_providers.as_ref();

    // Build a single, shared credential store. A failure to construct the
    // store does not abort registration: env-var and inline API-key paths
    // still work without it; only `AuthConfig::Stored` becomes unavailable.
    let store: Option<std::sync::Arc<CredentialStore>> = match store {
        Some(store) => Some(store),
        None => CredentialStore::at_default_location()
            .map(std::sync::Arc::new)
            .map_err(|e| {
                tracing::warn!(
                    "register_builtin_with_config: could not open user credential store: {e}"
                );
                e
            })
            .ok(),
    };
    let scope = RegistrationScope {
        store: store.as_ref(),
        pins,
        failures: std::cell::RefCell::new(Vec::new()),
    };

    register_config_provider(
        registry,
        providers,
        disabled,
        "anthropic",
        &scope,
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
        &scope,
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
        &scope,
        |key, _base_url| Box::new(crate::google::GoogleProvider::new(key)),
    );

    register_config_provider(
        registry,
        providers,
        disabled,
        "openrouter",
        &scope,
        |key, _base_url| Box::new(crate::openrouter::OpenRouterProvider::new(key)),
    );

    register_api_key_provider(
        registry,
        providers,
        disabled,
        "opencode_zen",
        "OPENCODE_ZEN_API_KEY",
        &scope,
        |key| Box::new(crate::opencode_zen::OpencodeZenProvider::new(key)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "mistral",
        "MISTRAL_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_mistral(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "groq",
        "GROQ_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_groq(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "deepinfra",
        "DEEPINFRA_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_deepinfra(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "cerebras",
        "CEREBRAS_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_cerebras(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "cohere",
        "COHERE_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_cohere(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "together",
        "TOGETHERAI_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_together(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "perplexity",
        "PERPLEXITY_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_perplexity(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "xai",
        "XAI_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_xai(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "venice",
        "VENICE_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_venice(cred)),
    );

    register_api_key_provider(
        registry,
        providers,
        disabled,
        "minimax",
        "MINIMAX_API_KEY",
        &scope,
        |key| Box::new(crate::additional::create_minimax(key)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "opencode_go",
        "OPENCODE_GO_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_opencode_go(cred)),
    );

    register_credential_provider(
        registry,
        providers,
        disabled,
        "generalcompute",
        "GENERALCOMPUTE_API_KEY",
        &scope,
        |cred| Box::new(crate::additional::create_generalcompute(cred)),
    );

    if registry.list().is_empty() {
        register_builtin(registry);
    }
    scope.failures.into_inner()
}

/// One provider's credential, already read from the store by the caller that
/// owns the durable connection.
///
/// Built by [`register_builtin_with_config_for_turn`] and threaded through
/// every registration helper so that a resolved connection wins for its own
/// provider regardless of what config, env, or record ordering say.
pub(crate) struct TurnCredentialPin {
    /// Registry provider id this credential is authoritative for.
    pub provider_id: String,
    pub credential: Credential,
}

/// Shared inputs for one [`register_builtins_with_store`] pass.
///
/// `failures` collects every provider whose credential could not be resolved
/// so a turn-scoped caller can report the one it cares about instead of
/// losing the distinction between "skipped" and "refused".
struct RegistrationScope<'a> {
    store: Option<&'a std::sync::Arc<CredentialStore>>,
    pins: &'a [TurnCredentialPin],
    failures: std::cell::RefCell<Vec<(String, crate::auth_types::AuthError)>>,
}

/// A durable provider connection's own credential-store account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCredentialLocator {
    /// `provider_connections.secret_provider_ref` — the store's provider key.
    pub provider_ref: String,
    /// `provider_connections.secret_account_ref` — the store's account key.
    pub account_ref: String,
}

/// Why one turn could not be given a provider credential.
///
/// Each variant is a distinct user-facing failure. Like
/// `crate::core::session_selection::ModelSelectResolveError`, they are never
/// collapsed: a bound-but-missing credential, an ambiguous store, and an
/// unreadable store are three different problems with three different fixes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TurnCredentialError {
    #[error(
        "provider connection credential '{}' for provider '{provider_id}' is missing from the credential store; re-run /connect or rotate this connection to restore it",
        account_id
    )]
    Missing {
        provider_id: String,
        account_id: String,
    },
    #[error(
        "no provider connection is bound to this session, and several credentials are stored for provider '{provider_id}' (accounts: {candidates}); bind one with /connect or remove the unused accounts"
    )]
    Ambiguous {
        provider_id: String,
        candidates: String,
    },
    #[error("credential for provider '{provider_id}' could not be resolved: {reason}")]
    Unresolved { provider_id: String, reason: String },
}

impl TurnCredentialError {
    /// Stable wire code for the failure, mirroring the split the daemon
    /// already uses for connection-state errors.
    pub fn code(&self) -> &'static str {
        match self {
            TurnCredentialError::Missing { .. } => "connection_credential_missing",
            TurnCredentialError::Ambiguous { .. } => "provider_credential_ambiguous",
            TurnCredentialError::Unresolved { .. } => "provider_credential_unresolved",
        }
    }
}

/// Register the built-ins for one turn, under that turn's credential authority.
///
/// `provider_id` is the registry id this turn will actually call. `locator`
/// is the account the session's resolved connection is bound to:
///
/// - `Some` — the bound record is the only admissible credential. It is read
///   by exact `(provider_ref, account_ref)`, pinned for the pass, and a
///   missing record is a hard error: no other record for this provider is a
///   legitimate substitute for the one the user selected.
/// - `None` — nothing binds this turn to an account, so a store holding more
///   than one usable record for `provider_id` is ambiguous rather than a
///   file-order coin flip. One usable record resolves exactly as before.
///
/// `store` is `None` for production callers, which use the default user
/// location; tests pass their own store.
pub fn register_builtin_with_config_for_turn(
    registry: &mut ProviderRegistry,
    config: &codegg_config::schema::Config,
    provider_id: &str,
    locator: Option<&TurnCredentialLocator>,
    store: Option<&std::sync::Arc<CredentialStore>>,
) -> Result<(), TurnCredentialError> {
    let store: Option<std::sync::Arc<CredentialStore>> = match store {
        Some(store) => Some(Arc::clone(store)),
        None => CredentialStore::at_default_location().map(Arc::new).ok(),
    };
    let pin = match locator {
        Some(locator) => {
            let store = store
                .as_ref()
                .ok_or_else(|| TurnCredentialError::Unresolved {
                    provider_id: provider_id.to_string(),
                    reason: "the user credential store could not be opened".to_string(),
                })?;
            let credential = store
                .get_credential(&locator.provider_ref, Some(&locator.account_ref))
                .map_err(|error| TurnCredentialError::Unresolved {
                    provider_id: provider_id.to_string(),
                    reason: error.to_string(),
                })?
                .ok_or_else(|| TurnCredentialError::Missing {
                    provider_id: provider_id.to_string(),
                    account_id: locator.account_ref.clone(),
                })?;
            // An expired bound record is dead material, not a usable one. It
            // is reported as missing rather than pinned, because sending it
            // would fail later as an opaque provider 401 instead of naming the
            // binding the operator has to rotate.
            if credential
                .expires_at
                .is_some_and(|expires_at| expires_at <= chrono::Utc::now())
            {
                return Err(TurnCredentialError::Missing {
                    provider_id: provider_id.to_string(),
                    account_id: locator.account_ref.clone(),
                });
            }
            Some(TurnCredentialPin {
                provider_id: provider_id.to_string(),
                credential,
            })
        }
        None => None,
    };
    let pins = pin.as_slice();
    let failures = register_builtins_with_store(registry, config, store, pins);
    if let Some((_, error)) = failures.into_iter().find(|(name, _)| name == provider_id) {
        return Err(match error {
            crate::auth_types::AuthError::AmbiguousCredential { candidates, .. } => {
                TurnCredentialError::Ambiguous {
                    provider_id: provider_id.to_string(),
                    candidates,
                }
            }
            other => TurnCredentialError::Unresolved {
                provider_id: provider_id.to_string(),
                reason: other.to_string(),
            },
        });
    }
    let explicitly_disabled = config
        .disabled_providers
        .as_ref()
        .is_some_and(|disabled| disabled.iter().any(|entry| entry == provider_id));
    if pin.is_some() && !explicitly_disabled && registry.get(provider_id).is_none() {
        // A bound credential that cannot build a provider (wrong credential
        // kind for this transport, empty secret) must not degrade into "no
        // provider configured", which would send the turn to another key.
        return Err(TurnCredentialError::Unresolved {
            provider_id: provider_id.to_string(),
            reason: "the bound credential could not build this provider".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn test_registry_new_is_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.list().is_empty());
    }

    fn read_http_request(stream: &mut TcpStream) {
        // C001: bounded WouldBlock-aware header reader. The redirect
        // listener is nonblocking, so the accepted stream can report
        // `WouldBlock` when the client has connected but not yet written
        // (the transient M001-closure flake). Retry transient readiness
        // within a local deadline; fail loudly on terminal I/O or expiry.
        // Production Eggfetch I/O and socket modes are untouched.
        let deadline = Duration::from_secs(2);
        let start = std::time::Instant::now();
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
            if start.elapsed() >= deadline {
                panic!(
                    "redirect fixture timed out waiting for HTTP headers after {:?}",
                    start.elapsed()
                );
            }
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("redirect fixture request read failed: {error:?}"),
            }
        }
    }

    fn redirect_server(redirects: usize) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        listener
            .set_nonblocking(true)
            .expect("set redirect server nonblocking");
        let address = listener.local_addr().expect("redirect server address");
        let handle = thread::spawn(move || {
            let mut requests = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while requests <= redirects && std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                // Accept inheritance of nonblocking mode is platform-specific;
                // force it so the WouldBlock-aware reader path is explicit.
                stream
                    .set_nonblocking(true)
                    .expect("set redirect stream nonblocking");
                read_http_request(&mut stream);
                let response = if requests < redirects {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: /hop/{}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        requests + 1
                    )
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        .to_string()
                };
                stream
                    .write_all(response.as_bytes())
                    .expect("write redirect response");
                requests += 1;
            }
            requests
        });
        (format!("http://{address}/start"), handle)
    }

    #[tokio::test]
    async fn shared_client_follows_redirects_and_enforces_ten_hop_bound() {
        let (url, server) = redirect_server(2);
        let mut response = create_http_client()
            .get(&url)
            .expect("build request")
            .send()
            .await
            .expect("redirect request succeeds");
        assert!(response.status().is_success());
        assert_eq!(response.text().await.expect("read response"), "ok");
        assert_eq!(server.join().expect("join redirect server"), 3);

        let (url, server) = redirect_server(11);
        let error = create_http_client()
            .get(&url)
            .expect("build request")
            .send()
            .await
            .expect_err("eleven redirects exceed the ten-hop bound");
        assert!(matches!(
            error,
            eggfetch_core::Error::TooManyRedirects { max: 10, .. }
        ));
        assert_eq!(server.join().expect("join bounded redirect server"), 11);
    }

    #[test]
    fn read_http_request_tolerates_transient_wouldblock() {
        // C001 regression: prove the hardened reader survives the transient
        // not-ready condition that panicked the old
        // `stream.read(...).expect("read HTTP request")` helper (M001
        // closure). Coordination (not chance): the client withholds bytes
        // until the server has proven `WouldBlock` on the accepted
        // nonblocking stream, so the old helper would panic here.
        use std::sync::mpsc::channel;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind WouldBlock fixture");
        let address = listener.local_addr().expect("fixture address");
        let (ready_tx, ready_rx) = channel::<()>();
        let (done_tx, done_rx) = channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept WouldBlock fixture");
            stream
                .set_nonblocking(true)
                .expect("fixture stream nonblocking");
            let mut probe = [0_u8; 1];
            match stream.read(&mut probe) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Ok(0) => panic!("WouldBlock fixture: unexpected EOF before client write"),
                Ok(_) => panic!("WouldBlock fixture: bytes arrived before induced wait"),
                Err(error) => panic!("WouldBlock fixture: unexpected probe error: {error:?}"),
            }
            ready_tx.send(()).expect("signal reader ready");
            read_http_request(&mut stream);
            done_tx.send(()).expect("signal headers consumed");
        });
        let mut client = TcpStream::connect(address).expect("connect WouldBlock fixture");
        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("server ready");
        // Withhold bytes briefly so the hardened reader also observes at
        // least one `WouldBlock` retry before headers arrive.
        thread::sleep(Duration::from_millis(50));
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("write fixture request");
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("headers consumed without panic");
        server.join().expect("join WouldBlock fixture");
    }

    #[tokio::test]
    async fn absolute_total_deadline_terminates_healthy_stream() {
        // M001 WP3 qualification: Eggfetch 0.2.0 `Timeout.total` is one
        // absolute deadline through response-body EOF, including
        // `bytes_stream()`. A server that keeps emitting chunks inside the
        // read/idle bound must still observe `TimeoutPhase::Total` once the
        // short test total elapses. This justifies removing the
        // client-global 60s total from the shared streaming-capable
        // provider client (WP4).
        use futures_util::StreamExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind streaming fixture");
        let addr = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept fixture");
            let mut buf = vec![0_u8; 4096];
            let mut acc = Vec::new();
            loop {
                let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
                    .await
                    .expect("read fixture request");
                if n == 0 {
                    break;
                }
                acc.extend_from_slice(&buf[..n]);
                if acc.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
            tokio::io::AsyncWriteExt::write_all(&mut stream, headers.as_bytes())
                .await
                .expect("write fixture headers");
            tokio::io::AsyncWriteExt::flush(&mut stream)
                .await
                .expect("flush fixture headers");
            for i in 0..30 {
                let payload = format!("data: chunk-{i}\n\n");
                let chunk = format!("{:X}\r\n{}\r\n", payload.len(), payload);
                if tokio::io::AsyncWriteExt::write_all(&mut stream, chunk.as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::io::AsyncWriteExt::flush(&mut stream).await.ok();
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, b"0\r\n\r\n").await;
        });

        let client = eggfetch_core::Client::builder()
            .timeout(
                eggfetch_core::Timeout::builder()
                    .connect(Duration::from_secs(5))
                    .total(Duration::from_millis(250))
                    .build(),
            )
            .follow_redirects(false)
            .build();
        let url = format!("http://{addr}/stream");
        let mut resp = client
            .get(&url)
            .expect("build fixture request")
            .send()
            .await
            .expect("fixture headers succeed");
        assert!(resp.status().is_success());
        let mut stream = resp.bytes_stream().expect("fixture bytes_stream");
        let mut chunks = 0_usize;
        let mut saw_total = false;
        loop {
            match stream.next().await {
                None => break,
                Some(Ok(_)) => chunks += 1,
                Some(Err(e)) => {
                    saw_total = matches!(
                        e,
                        eggfetch_core::Error::Timeout {
                            phase: eggfetch_core::TimeoutPhase::Total,
                            ..
                        }
                    );
                    break;
                }
            }
        }
        assert!(
            saw_total,
            "healthy chunked stream must terminate with TimeoutPhase::Total (chunks={chunks})"
        );
        assert!(
            chunks >= 1,
            "fixture must yield at least one chunk before the total deadline"
        );
        server.abort();
    }

    #[tokio::test]
    async fn shared_streaming_client_permits_long_active_stream() {
        // M001 WP4/WP5: the corrected shared client carries no absolute
        // total, so a healthy stream that keeps emitting inside idle bounds
        // must complete past the short WP3 total interval (250ms). The
        // fixture streams ~600ms of 30ms-spaced chunks; under the legacy
        // client-global 60s-equivalent policy this duration would already
        // exceed a short total, while the corrected client must succeed.
        use futures_util::StreamExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind long-stream fixture");
        let addr = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept fixture");
            let mut buf = vec![0_u8; 4096];
            let mut acc = Vec::new();
            loop {
                let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
                    .await
                    .expect("read fixture request");
                if n == 0 {
                    break;
                }
                acc.extend_from_slice(&buf[..n]);
                if acc.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
            tokio::io::AsyncWriteExt::write_all(&mut stream, headers.as_bytes())
                .await
                .expect("write fixture headers");
            tokio::io::AsyncWriteExt::flush(&mut stream)
                .await
                .expect("flush fixture headers");
            for i in 0..20 {
                let payload = format!("data: long-{i}\n\n");
                let chunk = format!("{:X}\r\n{}\r\n", payload.len(), payload);
                if tokio::io::AsyncWriteExt::write_all(&mut stream, chunk.as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                tokio::io::AsyncWriteExt::flush(&mut stream).await.ok();
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, b"0\r\n\r\n").await;
        });

        let url = format!("http://{addr}/stream");
        let mut resp = create_http_client()
            .get(&url)
            .expect("build fixture request")
            .send()
            .await
            .expect("long-stream headers succeed");
        assert!(resp.status().is_success());
        let mut stream = resp.bytes_stream().expect("long-stream bytes_stream");
        let mut chunks = 0_usize;
        while let Some(item) = stream.next().await {
            item.expect("long active stream must not fail before EOF");
            chunks += 1;
        }
        assert_eq!(chunks, 20, "all 20 chunks (~600ms) must arrive");
        server.abort();
    }

    #[test]
    fn non_streaming_timeout_carries_sixty_second_total() {
        // M001 WP4: bounded metadata operations retain an explicit finite
        // total. The helper carries only `total`; connect/pool inherit from
        // the shared client via per-field merge.
        let timeout = non_streaming_timeout();
        assert_eq!(timeout.total, Some(NON_STREAMING_PROVIDER_TOTAL));
        assert_eq!(
            NON_STREAMING_PROVIDER_TOTAL,
            Duration::from_secs(60),
            "historical 60s bound is preserved for metadata requests"
        );
        assert!(timeout.connect.is_none());
        assert!(timeout.read.is_none());
        assert!(timeout.pool.is_none());
        assert!(timeout.write.is_none());
    }

    #[test]
    fn provider_timeout_is_transient_before_visible_output() {
        // M001 WP5: a transport timeout before visible output stays
        // eligible only for bounded CodeGG retry (Transient), never
        // permanent. Post-visible no-replay is owned by
        // `src/agent/provider_turn.rs` (visible-output gate, covered by
        // `agent_loop_harness` midstream-failure tests) and must not be
        // weakened here.
        let timeout: ProviderError = eggfetch_core::Error::Timeout {
            phase: eggfetch_core::TimeoutPhase::Total,
            elapsed: Duration::from_millis(250),
        }
        .into();
        assert!(matches!(
            timeout.retry_disposition(),
            crate::error::RetryDisposition::Transient
        ));
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
    fn tool_history_projection_repairs_wire_only() {
        let messages = vec![Message::Assistant {
            content: vec![],
            tool_calls: vec![ToolCall {
                id: "call_1".to_string().into(),
                name: "read".to_string().into(),
                arguments: "{}".to_string().into(),
            }],
        }];

        let projected = project_tool_call_history(&messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(projected.len(), 2);
        assert!(matches!(projected[1], Message::Tool { .. }));
        if let Message::Tool { content, .. } = &projected[1] {
            assert!(content.contains("interrupted history"));
        }
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
            context: Default::default(),
        };
        assert_eq!(req.model, "test/model");
    }

    /// A request that actually carries a `reasoning_effort`, so an adapter
    /// that drops it is observably different from one that passes it through.
    fn effort_request() -> ChatRequest {
        ChatRequest {
            messages: vec![],
            model: "test/model".to_string(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: Some("high".to_string()),
            context: Default::default(),
        }
    }

    #[test]
    fn strips_reasoning_effort_matches_real_adapter_behavior() {
        // Locks the capability flag to what the adapters actually do. If a
        // fourth adapter starts stripping the field, this fails until the
        // `for_provider` list is updated too — the flag is what the TUI warns
        // on, so a stale `false` is a silently broken control.
        let azure = crate::azure::AzureProvider::new("k".into(), "https://e".into());
        let body = azure.build_body(&effort_request()).expect("azure body");
        assert!(
            body.get("reasoning_effort").is_none(),
            "azure is declared as stripping reasoning_effort but kept it"
        );

        let zen = crate::opencode_zen::OpencodeZenProvider::new("k".into());
        let body = zen.build_body(&effort_request()).expect("zen body");
        assert!(
            body.get("reasoning_effort").is_none(),
            "opencode_zen is declared as stripping reasoning_effort but kept it"
        );

        let router = crate::openrouter::OpenRouterProvider::new("k".into());
        let body = router
            .build_body(&effort_request())
            .expect("openrouter body");
        assert!(
            body.get("reasoning_effort").is_none(),
            "openrouter is declared as stripping reasoning_effort but kept it"
        );
    }

    #[test]
    fn strips_reasoning_effort_flag_matches_the_declared_provider_list() {
        for id in ["azure", "openrouter", "opencode_zen"] {
            assert!(
                ProviderCapabilities::for_provider(id).strips_reasoning_effort,
                "{id} strips reasoning_effort on the wire but the capability flag says otherwise"
            );
        }
        // A pass-through provider must NOT be flagged; over-reporting would
        // warn users who are in fact getting the behavior they asked for.
        assert!(!ProviderCapabilities::for_provider("openai").strips_reasoning_effort);
        assert!(!ProviderCapabilities::for_provider("anthropic").strips_reasoning_effort);
        // Unknown providers default to not stripping rather than warning
        // speculatively.
        assert!(
            !ProviderCapabilities::for_provider("some-future-provider").strips_reasoning_effort
        );
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
            Ok(Box::pin(futures_util::stream::empty()))
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    struct NamedProvider(&'static str);

    #[async_trait]
    impl Provider for NamedProvider {
        fn id(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            "Named"
        }
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(NamedProvider(self.0))
        }
        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            Ok(Box::pin(futures_util::stream::empty()))
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    /// Regression: `list()` orders the model catalog, and consumers that have
    /// no explicit selection fall back to `models[0]`. Backed by a `HashMap`,
    /// it iterated in `RandomState` order — reseeded per process — so the
    /// "first" provider, and therefore the model a session silently got,
    /// changed between launches.
    #[test]
    fn provider_list_order_is_sorted_and_stable_across_constructions() {
        const IDS: [&str; 6] = [
            "openai",
            "anthropic",
            "opencode_go",
            "generalcompute",
            "groq",
            "cerebras",
        ];
        let mut expected: Vec<String> = IDS.iter().map(|s| s.to_string()).collect();
        expected.sort();

        let mut first_seen: Option<Vec<String>> = None;
        for _ in 0..32 {
            let mut registry = ProviderRegistry::new();
            // Register in a deliberately non-sorted order.
            for id in IDS.iter().rev() {
                registry.register(NamedProvider(id));
            }
            let listed: Vec<String> = registry.list().iter().map(|p| p.id().to_string()).collect();
            assert_eq!(
                listed, expected,
                "list() must return providers sorted by id, regardless of registration order"
            );
            match &first_seen {
                None => first_seen = Some(listed),
                Some(previous) => {
                    assert_eq!(
                        &listed, previous,
                        "provider order must not vary between registries"
                    )
                }
            }
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
        let resolved =
            resolve_provider_credential("resolve_test_provider", Some(&cfg), None, None, &[])
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
        let resolved = resolve_provider_credential(
            "resolve_test_provider",
            Some(&cfg),
            None,
            Some(&store),
            &[],
        )
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
    fn resolve_provider_credential_falls_back_to_store_without_config_entry() {
        // Regression: a credential added through the TUI `/connect` flow is
        // written only to the credential store — there is no `[provider.*]`
        // config entry and no env var. Without the store fallback the resolver
        // returned `None`, the provider was skipped at registration
        // ("NO KEY for provider ..."), and it contributed no models, so
        // `/connect` reported success while `/models` stayed empty.
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "connect-fallback-test-master");
        std::env::remove_var("CONNECT_FALLBACK_TEST_API_KEY");

        let tmp = tempfile::tempdir().expect("tmpdir");
        let store =
            Arc::new(CredentialStore::at_path(tmp.path().join("credentials.json")).expect("store"));
        store
            .put(
                "connect_fallback_test",
                Some("conn-1"),
                CredentialKind::ApiKey,
                "connect-added-key",
                None,
                vec![],
            )
            .expect("put");

        // No config entry at all — exactly what `/connect` leaves behind.
        let resolved =
            resolve_provider_credential("connect_fallback_test", None, None, Some(&store), &[])
                .expect("ok")
                .expect("the stored credential must be found without config or env");
        assert_eq!(resolved.credential.secret, "connect-added-key");
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);

        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    #[test]
    fn env_still_wins_over_the_store_fallback() {
        // The fallback must not displace the existing precedence: an explicit
        // env var continues to beat a stored credential.
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_env = std::env::var("CONNECT_PRECEDENCE_TEST_API_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "connect-precedence-test-master");
        std::env::set_var("CONNECT_PRECEDENCE_TEST_API_KEY", "env-key");

        let tmp = tempfile::tempdir().expect("tmpdir");
        let store =
            Arc::new(CredentialStore::at_path(tmp.path().join("credentials.json")).expect("store"));
        store
            .put(
                "connect_precedence_test",
                Some("conn-1"),
                CredentialKind::ApiKey,
                "stored-key",
                None,
                vec![],
            )
            .expect("put");

        let resolved = resolve_provider_credential(
            "connect_precedence_test",
            None,
            Some("CONNECT_PRECEDENCE_TEST_API_KEY"),
            Some(&store),
            &[],
        )
        .expect("ok")
        .expect("env must resolve");
        assert_eq!(resolved.credential.secret, "env-key");

        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        match prev_env {
            Some(v) => std::env::set_var("CONNECT_PRECEDENCE_TEST_API_KEY", v),
            None => std::env::remove_var("CONNECT_PRECEDENCE_TEST_API_KEY"),
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

    #[test]
    fn resolve_provider_credential_decrypts_legacy_encrypted_api_key() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_env = std::env::var("LEGACY_ENC_TEST_API_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "legacy-encrypted-test-master");
        std::env::remove_var("LEGACY_ENC_TEST_API_KEY");

        let encrypted = crate::crypto::encrypt_to_string(
            "decrypted-legacy-key",
            "legacy-encrypted-test-master",
        )
        .expect("encrypt");

        let cfg = ProviderConfig {
            encrypted_api_key: Some(encrypted),
            ..Default::default()
        };
        let resolved = resolve_provider_credential("legacy_enc_test", Some(&cfg), None, None, &[])
            .expect("ok")
            .expect("some");
        assert_eq!(resolved.credential.secret, "decrypted-legacy-key");
        assert_eq!(resolved.source, ResolvedAuthSource::LegacyDecrypted);

        // Restore env
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        if let Some(v) = prev_env {
            std::env::set_var("LEGACY_ENC_TEST_API_KEY", v);
        }
    }

    #[test]
    fn resolve_provider_credential_skips_encrypted_api_key_without_master_key() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
        let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
        std::env::remove_var("CODEGG_MASTER_KEY");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        std::env::remove_var("OPENCODE_ENCRYPTION_KEY");

        let cfg = ProviderConfig {
            encrypted_api_key: Some("v2:deadbeef".to_string()),
            ..Default::default()
        };
        // No master key, no env override -> no credential resolved.
        let resolved =
            resolve_provider_credential("no_master_test", Some(&cfg), None, None, &[]).expect("ok");
        assert!(resolved.is_none());

        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
        if let Some(v) = prev_enc {
            std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
        }
        if let Some(v) = prev_opencode {
            std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
        }
    }

    // ---- M010: auth capability matrix and stored bearer closure ----

    #[test]
    fn capability_matrix_covers_every_registration_branch() {
        let order = builtin_registration_order();
        assert_eq!(order.len(), 17, "matrix must cover all 17 branches");
        let mut seen = std::collections::HashSet::new();
        for id in order {
            assert!(seen.insert(*id), "duplicate matrix entry for '{id}'");
        }
        // Compatible full-credential family.
        for id in [
            "mistral",
            "groq",
            "deepinfra",
            "cerebras",
            "cohere",
            "together",
            "perplexity",
            "xai",
            "venice",
            "opencode_go",
            "generalcompute",
        ] {
            assert_eq!(
                credential_capability_for(id),
                crate::auth_types::CredentialCapability::ApiKeyOrBearer,
                "provider '{id}' must be ApiKeyOrBearer"
            );
        }
        // Incompatible String-contract families.
        for id in [
            "anthropic",
            "openai",
            "google",
            "openrouter",
            "opencode_zen",
            "minimax",
        ] {
            assert_eq!(
                credential_capability_for(id),
                crate::auth_types::CredentialCapability::ApiKeyOnly,
                "provider '{id}' must be ApiKeyOnly"
            );
        }
        // Unknown providers must not silently inherit bearer support.
        assert_eq!(
            credential_capability_for("future_provider_xyz"),
            crate::auth_types::CredentialCapability::ApiKeyOnly
        );
    }

    fn m010_temp_store_with(
        provider: &str,
        account: Option<&str>,
        kind: CredentialKind,
        secret: &str,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> (tempfile::TempDir, Arc<CredentialStore>) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = Arc::new(CredentialStore::at_path(dir.path().join("c.json")).expect("store"));
        store
            .put(provider, account, kind, secret, expires_at, vec![])
            .expect("put");
        (dir, store)
    }

    fn m010_clear_env(keys: &[&str]) -> Vec<(String, Option<String>)> {
        keys.iter()
            .map(|k| {
                let prev = std::env::var(k).ok();
                std::env::remove_var(k);
                (k.to_string(), prev)
            })
            .collect()
    }

    fn m010_restore_env(saved: Vec<(String, Option<String>)>) {
        for (k, v) in saved {
            if let Some(v) = v {
                std::env::set_var(k, v);
            }
        }
    }

    #[test]
    fn stored_bearer_resolves_for_compatible_provider() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "m010-compat-resolve");
        let saved = m010_clear_env(&["XAI_API_KEY"]);
        let (_dir, store) = m010_temp_store_with(
            "xai",
            Some("default"),
            CredentialKind::BearerToken,
            "bearer-sentinel-xai",
            None,
        );
        let cfg = ProviderConfig {
            auth: Some(AuthConfig::Stored {
                account_id: Some("default".to_string()),
            }),
            ..Default::default()
        };
        let resolved =
            resolve_provider_credential("xai", Some(&cfg), Some("XAI_API_KEY"), Some(&store), &[])
                .expect("ok")
                .expect("some");
        assert_eq!(resolved.credential.kind, CredentialKind::BearerToken);
        assert_eq!(resolved.credential.secret, "bearer-sentinel-xai");
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);
        m010_restore_env(saved);
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    #[test]
    fn stored_bearer_rejected_for_api_key_only_providers() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "m010-incompat-resolve");
        // Cover both String-contract families: every config-aware provider
        // (anthropic/openai/google/openrouter) and every api-key-only
        // provider (opencode_zen/minimax). A bearer record must fail with a
        // typed incompatibility before any transport is constructed, so zero
        // outbound requests are possible.
        for (provider, env_var) in [
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("google", "GOOGLE_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
            ("opencode_zen", "OPENCODE_ZEN_API_KEY"),
            ("minimax", "MINIMAX_API_KEY"),
        ] {
            let saved = m010_clear_env(&[env_var]);
            let (_dir, store) = m010_temp_store_with(
                provider,
                Some("default"),
                CredentialKind::BearerToken,
                "bearer-sentinel-incompat",
                None,
            );
            let cfg = ProviderConfig {
                auth: Some(AuthConfig::Stored {
                    account_id: Some("default".to_string()),
                }),
                ..Default::default()
            };
            let err = match resolve_provider_credential(
                provider,
                Some(&cfg),
                Some(env_var),
                Some(&store),
                &[],
            ) {
                Ok(_) => panic!("bearer for '{provider}' must fail"),
                Err(err) => err,
            };
            assert!(
                matches!(err, crate::auth_types::AuthError::Unsupported(_)),
                "provider '{provider}' expected Unsupported, got {err:?}"
            );
            assert!(!format!("{err}").contains("bearer-sentinel-incompat"));
            m010_restore_env(saved);
        }
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    #[test]
    fn stored_expired_bearer_rejected_for_compatible_provider() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "m010-expired-resolve");
        let saved = m010_clear_env(&["GROQ_API_KEY"]);
        let expired = chrono::Utc::now() - chrono::Duration::minutes(10);
        let (_dir, store) = m010_temp_store_with(
            "groq",
            Some("default"),
            CredentialKind::BearerToken,
            "bearer-sentinel-expired",
            Some(expired),
        );
        let cfg = ProviderConfig {
            auth: Some(AuthConfig::Stored {
                account_id: Some("default".to_string()),
            }),
            ..Default::default()
        };
        let err = match resolve_provider_credential(
            "groq",
            Some(&cfg),
            Some("GROQ_API_KEY"),
            Some(&store),
            &[],
        ) {
            Ok(_) => panic!("expired bearer must fail"),
            Err(err) => err,
        };
        assert!(matches!(err, crate::auth_types::AuthError::Expired(_)));
        m010_restore_env(saved);
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    #[test]
    fn stored_api_key_still_resolves_for_api_key_only_provider() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "m010-apikey-still-ok");
        let saved = m010_clear_env(&["MINIMAX_API_KEY"]);
        let (_dir, store) = m010_temp_store_with(
            "minimax",
            Some("default"),
            CredentialKind::ApiKey,
            "stored-minimax-key",
            None,
        );
        let cfg = ProviderConfig {
            auth: Some(AuthConfig::Stored {
                account_id: Some("default".to_string()),
            }),
            ..Default::default()
        };
        let resolved = resolve_provider_credential(
            "minimax",
            Some(&cfg),
            Some("MINIMAX_API_KEY"),
            Some(&store),
            &[],
        )
        .expect("ok")
        .expect("some");
        assert_eq!(resolved.credential.kind, CredentialKind::ApiKey);
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);
        m010_restore_env(saved);
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    // ---- Turn credential authority ----

    /// Account ids shaped like a real rotated credential, so a regression that
    /// silently returns to file order has to pick a *different* key and fails
    /// loudly rather than passing by coincidence.
    const TURN_DEAD_ACCOUNT: &str = "d04e5c1e-8b22-4691-b9e9-7d44dda3b7be";
    const TURN_STALE_ACCOUNT: &str = "rot-10dc54e4-2a6f-4c1e-9f0b-1d2e3f4a5b6c";
    const TURN_BOUND_ACCOUNT: &str = "rot-b3c68955-77d1-4f0e-9a3c-2b5e6d7c8a90";

    fn turn_store(records: &[(&str, &str, &str)]) -> (tempfile::TempDir, Arc<CredentialStore>) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = Arc::new(CredentialStore::at_path(dir.path().join("c.json")).expect("store"));
        for (provider, account, secret) in records {
            store
                .put(
                    provider,
                    Some(account),
                    CredentialKind::ApiKey,
                    secret,
                    None,
                    vec![],
                )
                .expect("put");
        }
        (dir, store)
    }

    /// Build a turn registry with every process-global effect reverted before
    /// returning: a std mutex guard must never be held across an await.
    fn turn_registry(
        records: &[(&str, &str, &str)],
        provider_id: &str,
        locator: Option<&TurnCredentialLocator>,
        config: &codegg_config::schema::Config,
    ) -> (ProviderRegistry, Result<(), TurnCredentialError>) {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "turn-credential-authority-master");
        let saved = m010_clear_env(&[
            "OPENAI_API_KEY",
            "OPENCODE_GO_API_KEY",
            "OPENCODE_GO_API_KEY",
        ]);
        let (_dir, store) = turn_store(records);
        let mut registry = ProviderRegistry::new();
        let outcome = register_builtin_with_config_for_turn(
            &mut registry,
            config,
            provider_id,
            locator,
            Some(&store),
        );
        m010_restore_env(saved);
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        (registry, outcome)
    }

    fn openai_pointed_at(base_url: &str) -> codegg_config::schema::Config {
        let mut provider = ProviderConfig {
            base_url: Some(base_url.to_string()),
            ..Default::default()
        };
        provider.api_key = None;
        let mut providers = std::collections::HashMap::new();
        providers.insert("openai".to_string(), provider);
        codegg_config::schema::Config {
            provider: Some(providers),
            ..Default::default()
        }
    }

    /// One model-listing server that reports the Authorization header it
    /// received, so a test can prove *which key* the registered provider sends.
    fn capture_models_server() -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let authorization = request
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
                .map(|line| {
                    line[line.find(':').expect("colon") + 1..]
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            let _ = tx.send(authorization);
            let body = "{\"data\":[{\"id\":\"probe-model\"}]}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        (base, rx)
    }

    /// The regression guard: a turn bound to one connection must authenticate
    /// with that connection's account while two other records for the same
    /// provider sit in the store, one of them first on disk. Reading the key
    /// back off the wire is what makes this a real guard — the first record's
    /// dead key would be sent here if the binding were ignored.
    #[tokio::test]
    async fn turn_authenticates_with_the_credential_its_connection_is_bound_to() {
        let (base_url, authorization) = capture_models_server();
        let config = openai_pointed_at(&base_url);
        let locator = TurnCredentialLocator {
            provider_ref: "openai".to_string(),
            account_ref: TURN_BOUND_ACCOUNT.to_string(),
        };
        let (registry, outcome) = turn_registry(
            &[
                ("openai", TURN_DEAD_ACCOUNT, "dead-first-key"),
                ("openai", TURN_STALE_ACCOUNT, "stale-rotated-key"),
                ("openai", TURN_BOUND_ACCOUNT, "bound-live-key"),
            ],
            "openai",
            Some(&locator),
            &config,
        );
        outcome.expect("a bound connection must resolve its own credential");
        let provider = registry.get("openai").expect("openai must be registered");
        provider
            .models()
            .await
            .expect("model listing against the capture server");
        let sent =
            tokio::task::spawn_blocking(move || authorization.recv_timeout(Duration::from_secs(5)))
                .await
                .expect("join")
                .expect("captured authorization header");
        assert_eq!(
            sent, "Bearer bound-live-key",
            "the turn must authenticate with the bound account, not the first record on disk"
        );
    }

    /// A bound turn whose record is gone fails with a typed error naming the
    /// account. It must not quietly use one of the other records for the same
    /// provider — that is how a rotation appears to do nothing.
    #[test]
    fn turn_with_a_missing_bound_credential_fails_instead_of_using_another_key() {
        let locator = TurnCredentialLocator {
            provider_ref: "opencode_go".to_string(),
            account_ref: TURN_BOUND_ACCOUNT.to_string(),
        };
        let (registry, outcome) = turn_registry(
            &[
                ("opencode_go", TURN_DEAD_ACCOUNT, "dead-first-key"),
                ("opencode_go", TURN_STALE_ACCOUNT, "stale-rotated-key"),
            ],
            "opencode_go",
            Some(&locator),
            &codegg_config::schema::Config::default(),
        );
        let error = outcome.expect_err("a missing bound credential must fail");
        assert_eq!(error.code(), "connection_credential_missing");
        assert!(
            matches!(&error, TurnCredentialError::Missing { account_id, .. } if account_id == TURN_BOUND_ACCOUNT),
            "the error must name the missing account, got {error:?}"
        );
        assert!(
            registry.get("opencode_go").is_none(),
            "no provider may be built from an unrelated record"
        );
        let message = error.to_string();
        assert!(message.contains(TURN_BOUND_ACCOUNT), "message: {message}");
        assert!(!message.contains("dead-first-key"), "message: {message}");
    }

    /// No connection resolved, several records stored: refuse to guess and
    /// name every candidate so the operator can bind or remove one.
    #[test]
    fn unbound_turn_with_several_records_reports_the_candidates() {
        let (registry, outcome) = turn_registry(
            &[
                ("opencode_go", TURN_DEAD_ACCOUNT, "dead-first-key"),
                ("opencode_go", TURN_STALE_ACCOUNT, "stale-rotated-key"),
                ("opencode_go", TURN_BOUND_ACCOUNT, "live-key"),
            ],
            "opencode_go",
            None,
            &codegg_config::schema::Config::default(),
        );
        let error = outcome.expect_err("file order must not pick the credential");
        assert_eq!(error.code(), "provider_credential_ambiguous");
        let message = error.to_string();
        assert!(message.contains("opencode_go"), "message: {message}");
        for account in [TURN_DEAD_ACCOUNT, TURN_STALE_ACCOUNT, TURN_BOUND_ACCOUNT] {
            assert!(message.contains(account), "missing {account} in: {message}");
        }
        assert!(
            !message.contains("dead-first-key"),
            "a secret must never appear in the message: {message}"
        );
        assert!(
            registry.get("opencode_go").is_none(),
            "an ambiguous provider must not be registered with a guessed key"
        );
    }

    /// One record for the provider and no connection: the ordinary
    /// single-record store keeps working exactly as before.
    #[test]
    fn unbound_turn_with_one_record_still_resolves() {
        let (registry, outcome) = turn_registry(
            &[("opencode_go", TURN_BOUND_ACCOUNT, "only-key")],
            "opencode_go",
            None,
            &codegg_config::schema::Config::default(),
        );
        outcome.expect("a single stored record is unambiguous");
        assert!(registry.get("opencode_go").is_some());
    }

    /// Expired records are not candidates. A store holding one dead-by-date
    /// and one live record resolves the live one instead of reporting the
    /// pair as ambiguous.
    #[test]
    fn unbound_turn_ignores_expired_records() {
        let _guard = crate::auth_types::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "turn-expired-record-master");
        let saved = m010_clear_env(&["OPENCODE_GO_API_KEY"]);
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = Arc::new(CredentialStore::at_path(dir.path().join("c.json")).expect("store"));
        store
            .put(
                "opencode_go",
                Some(TURN_DEAD_ACCOUNT),
                CredentialKind::ApiKey,
                "expired-key",
                Some(chrono::Utc::now() - chrono::Duration::minutes(1)),
                vec![],
            )
            .expect("put expired");
        store
            .put(
                "opencode_go",
                Some(TURN_BOUND_ACCOUNT),
                CredentialKind::ApiKey,
                "live-key",
                None,
                vec![],
            )
            .expect("put live");
        let mut registry = ProviderRegistry::new();
        let outcome = register_builtin_with_config_for_turn(
            &mut registry,
            &codegg_config::schema::Config::default(),
            "opencode_go",
            None,
            Some(&store),
        );
        m010_restore_env(saved);
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        outcome.expect("an expired record must not make the pair ambiguous");
        assert!(registry.get("opencode_go").is_some());
    }
}
