# Provider Module

The `provider` module provides a unified interface for interacting with various LLM backends.

## Overview

**Location**: `src/provider/`

**Key Responsibilities**:
- Unified interface for LLM backends (Anthropic, OpenAI, Google, etc.)
- Chat request/response handling
- Model catalog and discovery
- Response caching
- Circuit breaker integration for provider fallback

## Provider Implementations

### Core Providers

| Provider | File | Models |
|----------|------|--------|
| **Anthropic** | `anthropic.rs` | Claude Sonnet 4, Opus 4, 3.5 Sonnet, 3.5 Haiku |
| **OpenAI** | `openai.rs` | GPT-4.1, GPT-4.1 Mini, GPT-4o |
| **Google** | `google.rs` | Gemini 2.5 Pro, Flash, 2.0 Flash |
| **Azure** | `azure.rs` | Azure OpenAI models |
| **Vertex** | `vertex.rs` | Google Vertex AI |
| **Bedrock** | `bedrock.rs` | AWS Bedrock (Claude, Llama, Mistral) |
| **OpenRouter** | `openrouter.rs` | Aggregated models |
| **CodeggZen** | `codegg_zen.rs` | big-pickle, minimax-m2.5-free, nemotron-3-super-free, qwen3.6-plus-free |

### Additional Providers (in `additional.rs`)

These are available via config or env var fallback (`register_builtin_with_config`):

| Provider | Registration | Factory Function |
|----------|--------------|------------------|
| Mistral | Env/GitHub Copilot | `create_mistral()` |
| Groq | Env/GitHub Copilot | `create_groq()` |
| DeepInfra | Env/GitHub Copilot | `create_deepinfra()` |
| Cerebras | Env/GitHub Copilot | `create_cerebras()` |
| Cohere | Env/GitHub Copilot | `create_cohere()` |
| TogetherAI | Env/GitHub Copilot | `create_together()` |
| Perplexity | Env/GitHub Copilot | `create_perplexity()` |
| xAI | Env/GitHub Copilot | `create_xai()` |
| Venice | Env/GitHub Copilot | `create_venice()` |
| MiniMax | Env/GitHub Copilot | `create_minimax()` |
| Codegg Go | Env/GitHub Copilot only | `create_codegg_go()` |

These providers exist in `additional.rs` but require explicit config to use:

| Provider | Factory Function |
|----------|-------------------|
| SAP AI Core | `create_sap_ai_core()` |
| Zenmux | `create_zenmux()` |
| Kilo | `create_kilo()` |
| Vercel AI Gateway | `create_vercel_ai_gateway()` |

### Additional OpenAI-Compatible Providers

| Provider | File |
|----------|------|
| Cloudflare Workers AI | `cloudflare.rs` |
| GitHub Copilot | `copilot.rs` |
| GitLab AI | `gitlab.rs` |
| OpenAI Compatible | `openai_compatible.rs` |

## Core Traits and Types

### Provider Trait

```rust
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
```

### ChatRequest

```rust
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub tools: Option<Vec<ToolDefinition>>,
    pub system: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<usize>,
    pub response_format: Option<ResponseFormat>,
}
```

### Message Enum

```rust
pub enum Message {
    System { content: Arc<String> },
    User { content: Vec<ContentPart> },
    Assistant { content: Vec<ContentPart>, tool_calls: Vec<ToolCall> },
    Tool { tool_call_id: Arc<String>, content: Arc<String> },
}
```

### ContentPart Enum

```rust
pub enum ContentPart {
    Text { text: Arc<String> },
    Image { image_url: ImageUrl },
}

pub struct ImageUrl {
    pub url: Arc<String>,
}
```

### ChatEvent Enum

```rust
pub enum ChatEvent {
    TextDelta(Arc<String>),
    ReasoningDelta(Arc<String>),
    ToolCall(ToolCall),
    ToolResult { tool_call_id: Arc<String>, content: Arc<String> },
    Finish { stop_reason: Arc<String>, usage: TokenUsage },
    Error(Arc<String>),
}
```

### ToolCall Struct

```rust
pub struct ToolCall {
    pub id: Arc<String>,
    pub name: Arc<String>,
    pub arguments: serde_json::Value,
}
```

### ToolDefinition

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn to_openai(&self) -> serde_json::Value { ... }
    pub fn to_anthropic(&self) -> serde_json::Value { ... }
}
```

### TokenUsage

```rust
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub reasoning_tokens: usize,
}
```

### ModelInfo

```rust
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
```

### ResponseFormat

```rust
pub enum ResponseFormat {
    JsonObject,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
        strict: bool,
    },
}
```

### ModelVariant

```rust
pub struct ModelVariant {
    pub suffix: String,
    pub context_window_override: Option<usize>,
    pub max_output_override: Option<usize>,
    pub extra_params: serde_json::Value,
    pub prompt: Option<String>,
}
```

## ProviderRegistry

Central registry for managing provider instances:

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn register(&mut self, provider: impl Provider + 'static);
    pub fn get(&self, id: &str) -> Option<&dyn Provider>;
    pub fn list(&self) -> Vec<&dyn Provider>;
}
```

## Key Components

### catalog.rs - Model Catalog

Maintains registry of available models with TTL-based caching:

```rust
pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
    last_fetch: Option<Instant>,
    cache_ttl: Duration,
}
```

Note: The catalog seeds from embedded models (`models.rs`) and can fetch live model data from `https://models.dev/api/models`.

### discovery.rs - Provider Discovery

Auto-discovers providers from environment variables and database cache.

### cache.rs - Response Caching

LRU-like cache with TTL for provider responses:

```rust
pub struct ProviderCache {
    cache: HashMap<CacheKey, CacheEntry>,
}
```

### fallback.rs - FallbackProvider

Multi-provider fallback chain with circuit breaker integration:

```rust
pub struct FallbackProvider {
    providers: Vec<Box<dyn Provider>>,
    status_codes: Vec<u16>,  // Default: [429, 500, 502, 503, 504]
    circuit_breakers: Vec<CircuitBreaker>,
}
```

### sse_parser.rs - SSE Parsing

Unified SSE parser for OpenAI and Anthropic streaming formats:

```rust
#[derive(Debug, Clone, Default)]
struct OpenAiToolState {
    id: String,
    name: String,
    args_buffer: String,
}

pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,
    args_buffer: String,
    openai_tool_states: HashMap<usize, OpenAiToolState>,
}

pub fn parse_openai_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>>;
pub fn parse_anthropic_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>>;
pub fn parse_anthropic_buffer_with_state(
    buffer: &mut String,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
) -> Option<Result<ChatEvent, ProviderError>>;
```

## Registration Patterns

### register_builtin_with_config

Primary entry point. Registers all providers from config with environment variable fallback:

```rust
pub fn register_builtin_with_config(registry: &mut ProviderRegistry, config: &Config);
```

Supports providers defined in config file with optional env var fallback for API keys.

### register_builtin

Registers providers from environment variables only (no config required):

```rust
pub fn register_builtin(registry: &mut ProviderRegistry);
```

Providers registered: ANTHROPIC_API_KEY, OPENAI_API_KEY, GOOGLE_API_KEY, OPENROUTER_API_KEY, CODEGG_ZEN_API_KEY, MISTRAL_API_KEY, GROQ_API_KEY, DEEPINFRA_API_KEY, CEREBRAS_API_KEY, COHERE_API_KEY, TOGETHERAI_API_KEY, PERPLEXITY_API_KEY, XAI_API_KEY, VENICE_API_KEY, MINIMAX_API_KEY

### register_config_provider

For providers that read API key and optional base URL from config:

```rust
fn register_config_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&HashMap<String, ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    factory: F,
) where
    F: FnOnce(String, Option<String>) -> Box<dyn Provider>,
```

### register_env_fallback_provider

For providers that fall back to environment variables when no config API key is provided:

```rust
fn register_env_fallback_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&HashMap<String, ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    factory: F,
) where
    F: FnOnce(String) -> Box<dyn Provider>,
```

## ProviderError

```rust
pub enum ProviderError {
    NotFound(String),
    Api { code: String, message: String, url: String },
    Stream(String),
    RateLimit,
    Auth(String),
    ModelNotFound(String),
    Timeout(String),
    CircuitOpen(String),
}

impl ProviderError {
    pub fn api(code: impl Into<String>, message: impl Into<String>) -> Self;
    pub fn api_with_url(code: impl Into<String>, message: impl Into<String>, url: impl Into<String>) -> Self;
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Auth(_)
                | ProviderError::Timeout(_)
                | ProviderError::Stream(_)
                | ProviderError::CircuitOpen(_)
        )
    }
}
```

## Interactions

```
AgentLoop
├── ProviderRegistry::get(provider_id)
│   └── Provider::stream(request)
│       └── HTTP request to LLM API
├── FallbackProvider
│   ├── CircuitBreaker::is_available()
│   └── CircuitBreaker::record_success/failure
└── Provider events → ChatEvent stream
```

## Configuration

Related config fields:

```toml
[provider]
default = "anthropic"

[providers.anthropic]
api_key = "sk-..."
base_url = "https://api.anthropic.com"  # optional override

[providers.openai]
api_key = "sk-..."

[providers.openrouter]
api_key = "sk-..."
base_url = "https://openrouter.ai/api/v1"  # required for OpenRouter
```

## Implementation Notes

### Arc<String> Usage

All content fields in `Message`, `ChatEvent`, `ToolCall` use `Arc<String>` for efficiency:

```rust
// When creating these types, use .into()
Message::System { content: "hello".into() }
ChatEvent::TextDelta("hello".into())
ToolCall { id: id.into(), name: name.into(), arguments }
```

### Buffer Size Limits

All streaming implementations must enforce buffer limits to prevent unbounded memory growth:

```rust
const MAX_BUFFER_SIZE: usize = 1024 * 1024;  // 1MB limit

if buffer.len() > MAX_BUFFER_SIZE {
    return Some((
        Err(ProviderError::Stream("response buffer exceeded limit".to_string())),
        (stream, buffer),
    ));
}
```

### HTTP Client Configuration

All providers use a shared HTTP client configuration:

```rust
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
```

Note: Uses `.inspect_err()` for warning logging and `.unwrap_or_default()` for graceful fallback on build failure.

## See Also

- [agent.md](agent.md) - Uses providers for LLM calls
- [resilience.md](resilience.md) - Circuit breaker pattern
- [error.md](error.md) - ProviderError and error handling