# Provider Module Architecture

## Overview

The provider module (`crates/codegg-providers/`) provides the interface and implementations for interacting with various LLM (Large Language Model) backends. It offers a unified `Provider` trait that abstracts over different API providers (Anthropic, OpenAI, Google, Azure, etc.), handling authentication, request formatting, streaming responses, and error handling.

**Re-export**: `codegg::provider` via `pub use codegg_providers as provider` in `src/lib.rs`

## Provider Trait and Core Types

### Provider Trait (`src/provider/mod.rs:74-87`)

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

Key methods:
- `id()` - Returns a unique identifier string (e.g., "anthropic", "openai")
- `name()` - Returns a human-readable name (e.g., "Anthropic", "OpenAI")
- `clone_box()` - Creates a boxed clone of the provider
- `stream()` - Main method to send a chat request and receive a streaming response
- `models()` - Returns a list of available models
- `discover_models()` - Override point for dynamic model discovery (default calls `models()`)
- `ping()` - Health check (default implementation calls `models()`)

### ChatRequest (`src/provider/mod.rs:111-123`)

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
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
}
```

### Message Types (`src/provider/mod.rs:125-142`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System { content: Arc<String> },
    User { content: Vec<ContentPart> },
    Assistant { content: Vec<ContentPart>, tool_calls: Vec<ToolCall> },
    Tool { tool_call_id: Arc<String>, content: Arc<String> },
}
```

### ContentPart (`src/provider/mod.rs:144-149`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text { text: Arc<String> },
    Image { image_url: ImageUrl },
}
```

### ChatEvent (`src/provider/mod.rs:156-170`)

The streaming response is a stream of `ChatEvent` values:

```rust
pub enum ChatEvent {
    TextDelta(Arc<String>),           // Text content delta
    ReasoningDelta(Arc<String>),        // Reasoning/thinking content
    ToolCall(ToolCall),                 // Tool invocation
    ToolResult {                        // Tool execution result
        tool_call_id: Arc<String>,
        content: Arc<String>,
    },
    Finish {                            // Response complete
        stop_reason: Arc<String>,
        usage: TokenUsage,
    },
    Error(Arc<String>),                 // Error occurred
}
```

### ModelInfo (`src/provider/mod.rs:236-246`)

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

### ModelVariant (`src/provider/mod.rs:226-234`)

```rust
pub struct ModelVariant {
    pub suffix: String,
    pub context_window_override: Option<usize>,
    pub max_output_override: Option<usize>,
    pub extra_params: serde_json::Value,
    pub prompt: Option<String>,
}
```

### TokenUsage (`src/provider/mod.rs:189-196`)

```rust
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub reasoning_tokens: usize,
    pub cached_tokens: Option<usize>,
}
```

### ToolDefinition (`src/provider/mod.rs:198-224`)

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

Methods for converting to provider-specific formats:
- `to_openai()` - Converts to OpenAI function format
- `to_anthropic()` - Converts to Anthropic tool format

### EventStream Type

```rust
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ChatEvent, ProviderError>> + Send>>;
```

## ProviderRegistry (`src/provider/mod.rs:248-277`)

The registry manages all available providers:

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, provider: impl Provider + 'static);
    pub fn get(&self, id: &str) -> Option<&dyn Provider>;
    pub fn list(&self) -> Vec<&dyn Provider>;
}
```

### Registration Functions

#### `register_builtin()` (`src/provider/mod.rs:279-326`)

Registers 15 providers based on environment variables. Each provider is only registered if the corresponding API key environment variable is set. Providers are independent - adding one via config does NOT disable others; each provider checks its own config key.

| Environment Variable | Provider |
|---------------------|----------|
| `ANTHROPIC_API_KEY` | Anthropic |
| `OPENAI_API_KEY` | OpenAI |
| `GOOGLE_API_KEY` | Google |
| `OPENROUTER_API_KEY` | OpenRouter |
| `OPENCODE_ZEN_API_KEY` | Codegg Zen |
| `MISTRAL_API_KEY` | Mistral |
| `GROQ_API_KEY` | Groq |
| `DEEPINFRA_API_KEY` | DeepInfra |
| `CEREBRAS_API_KEY` | Cerebras |
| `COHERE_API_KEY` | Cohere |
| `TOGETHERAI_API_KEY` | Together AI |
| `PERPLEXITY_API_KEY` | Perplexity |
| `XAI_API_KEY` | xAI |
| `VENICE_API_KEY` | Venice |
| `MINIMAX_API_KEY` | MiniMax |

#### `register_builtin_with_config()` (`src/provider/mod.rs:390-537`)

Registers 16 providers from config file, with fallback to environment variables. This function:

1. Registers 16 providers: the same 15 as `register_builtin()` plus `opencode_go`, each checking config first then env var
2. **Per-provider independence**: Each provider is checked independently against the config map. Adding one provider (e.g., `anthropic`) via config does NOT suppress or disable other providers that fall back to env vars. This is a per-provider fallback, not a global toggle.
3. Only calls `register_builtin()` if registry is still empty after config-based registration

Config-only providers (SAP AI Core, Zenmux, Kilo, Vercel AI Gateway) are NOT auto-registered - they require explicit config entries.

## Provider Implementations

### 1. Anthropic (`src/provider/anthropic.rs`)

Direct implementation using Anthropic's Messages API.

**Key features:**
- Base URL: `https://api.anthropic.com`
- API version header: `anthropic-version: 2023-06-01`
- Uses SSE streaming with `stream: true`
- Supports thinking budget via `thinking.budget_tokens`
- Image support via base64 inline data
- Custom SSE parsing via `parse_anthropic_buffer()`

**Models (hardcoded):**
- `claude-sonnet-4-20250514` (200K ctx, 64K output)
- `claude-opus-4-20250514` (200K ctx, 32K output)
- `claude-3-5-sonnet-20241022` (200K ctx, 8K output)
- `claude-3-5-haiku-20241022` (200K ctx, 8K output)

### 2. OpenAI (`src/provider/openai.rs`)

Full implementation with `OpenAiConfig` for customization.

**OpenAiConfig options:**
- `api_key`, `base_url`
- `provider_id`, `provider_name`
- `requires_org_header` (default: false for generic, true for OpenAI brand)
- `organization` (optional OpenAI org)
- `omit_stream_options` (for some providers like Groq)
- `tool_choice` (`ToolChoice` enum: `Auto`/`Required`/`None`/`Specific(name)`)

**Factory methods:**
- `OpenAiConfig::default_with_key(api_key)` - Generic OpenAI-compatible
- `OpenAiConfig::openai(api_key)` - Official OpenAI (requires org header)
- `OpenAiConfig::groq(api_key)` - Groq specific settings
- `OpenAiConfig::xai(api_key)` - xAI specific settings
- `OpenAiConfig::mistral(api_key)` - Mistral specific settings
- `OpenAiConfig::cerebras(api_key)` - Cerebras specific settings

### 3. Google (`src/provider/google.rs`)

Uses Google's Generative Language API.

**Key features:**
- Uses `streamGenerateContent` with SSE
- Custom message format with `contents` array
- Tool definitions wrapped as `function_declarations` in `tools` array
- Supports thinking/reasoning via `thought` flag in parts

**Models:**
- `gemini-2.5-pro` (1M ctx, 65K output)
- `gemini-2.5-flash` (1M ctx, 65K output)
- `gemini-2.0-flash` (1M ctx, 8K output)

### 4. Azure (`src/provider/azure.rs`)

Azure OpenAI Service implementation.

**Key features:**
- Endpoint format: `{endpoint}/openai/deployments/{model}/chat/completions?api-version=2024-10-21`
- Uses `api-key` header instead of Authorization
- Always includes `stream_options: { include_usage: true }`

**Models:**
- `gpt-4.1` (1M ctx, 32K output)
- `gpt-4o` (128K ctx, 16K output)

### 5. Vertex (`src/provider/vertex.rs`)

Google Cloud Vertex AI implementation. Wraps `OpenAiCompatibleProvider`.

**Key features:**
- Base URL: `https://{project_id}-aiplatform.googleapis.com/v1beta1/projects/{project_id}/locations/us-central1/endpoints/openapi`
- Uses Bearer token authentication

### 6. Bedrock (`src/provider/bedrock.rs`)

Amazon AWS Bedrock implementation.

**Key features:**
- Uses AWS Signature Version 4 signing
- Endpoint: `https://bedrock-runtime.{region}.amazonaws.com/model/{model}/converse-stream`
- Supports session tokens for temporary credentials
- Custom SSE parsing for Bedrock's event stream format
- Tool calls use `toolUse` format within message content

**Models:**
- `anthropic.claude-sonnet-4-20250514-v1:0` (200K ctx)
- `anthropic.claude-3-5-sonnet-20241022-v2:0` (200K ctx)
- `meta.llama3-1-405b-instruct-v1:0` (128K ctx)

### 7. OpenRouter (`src/provider/openrouter.rs`)

OpenRouter aggregator. Adds `HTTP-Referer` and `X-Title` headers.

**Models:**
- `anthropic/claude-sonnet-4` (200K ctx)
- `openai/gpt-4.1` (1M ctx)
- `google/gemini-2.5-pro` (1M ctx)

### 8. OpenAI Compatible (`src/provider/openai_compatible.rs`)

Generic OpenAI-compatible API provider. Used as base for many providers.

**OpenAiCompatibleConfig:**
```rust
pub struct OpenAiCompatibleConfig {
    pub credential: Credential,                          // from crate::auth
    pub base_url: String,
    pub auth_header: String,
    pub extra_headers: Vec<(String, String)>,
    pub models: Vec<ModelInfo>,
    pub tool_choice: ToolChoice,
}
```

Note: `OpenAiCompatibleConfig` no longer has an `api_key: String` field; it
holds a `auth::Credential` (kind / secret / expires_at). Two factory
methods exist:

- `OpenAiCompatibleProvider::simple(id, name, api_key, base_url)` — wraps
  the API key in `Credential::api_key(api_key)`. Backwards compatible.
- `OpenAiCompatibleProvider::simple_with_credential(id, name, credential, base_url)`
  — accepts a full `Credential` envelope so the registered provider
  preserves `CredentialKind` (api key vs. bearer) and any `expires_at`
  metadata.

**Key features:**
- Includes debug logging for request details (model, tool count, first tool arg shape)
- 30-second timeout per chunk to prevent hanging
- Dynamic model discovery via `/models` endpoint

**Factory methods:**
- `OpenAiCompatibleProvider::simple(id, name, api_key, base_url)` - Simple
  setup; wraps the API key in `Credential::api_key(...)`.
- `OpenAiCompatibleProvider::simple_with_credential(id, name, credential, base_url)`
  - Same, but accepts a full `Credential` so bearer tokens and other
  `CredentialKind` variants are preserved.

#### Auth path through the resolver

`register_builtin_with_config` (`src/provider/mod.rs:501`) wires the user
credential store into registration by calling
`CredentialStore::at_default_location()` once and threading
`Arc<CredentialStore>` into each registration helper. The helpers are:

- `resolve_provider_credential(provider_id, cfg, env_var, store)` — a single
  helper in `src/provider/mod.rs` that builds a `ResolverContext` and calls
  `AuthResolver::resolve`. Returns the full `ResolvedAuth` (not just the
  secret) so the caller can inspect the source.
- `register_credential_provider(...)` — factories that accept a `Credential`
  directly. Used for all OpenAI-compatible providers (mistral, groq,
  deepinfra, cerebras, cohere, together, perplexity, xai, venice,
  opencode_go, generalcompute). `CredentialKind::BearerToken` is preserved
  so a future OAuth flow can land here without re-flattening to a string.
- `register_api_key_provider(...)` — factories that take only the secret
  string. Used for `minimax` (Anthropic-compatible) and any provider that
  cannot accept a bearer credential. Rejects `CredentialKind::BearerToken`
  with a `tracing::warn!` and skips registration.
- `register_config_provider(...)` — base-URL-aware variant for Anthropic,
  OpenAI native, Google, and OpenRouter. Threads the resolved secret
  through to the factory closure along with `cfg.base_url`.

This is the **single resolution path** for provider registration. No
helper reads `cfg.api_key` directly; legacy `provider.<id>.api_key`
fields are honored by `AuthResolver` via `ctx.legacy_api_key`. New
auth modes should be added to `AuthResolver` and reflected in
`ResolvedAuthSource`; they will then be available to all three
registration helpers automatically.

The typed `cfg.auth` descriptor
(`AuthConfig::ApiKey { env, value, encrypted_value }`, `Stored { .. }`,
`ExternalCommand { .. }`, or `OAuthDevice { .. }`) is passed into
`AuthResolver::resolve`. The `OAuthDevice` and `ExternalCommand` variants
are recognized but return `AuthError::Unsupported` from the synchronous
resolver. `ExternalCommand` is intentionally disabled: both
`AuthResolver::resolve` and `ExternalCommandProvider::fetch` return
`Unsupported` for a non-empty command (an empty command yields
`Invalid`), and the previous `std::process::Command`-based shell-out
path has been removed. Async timeout plumbing is a follow-up.

These `tracing::debug!` / `tracing::warn!` lines log only
`ResolvedAuthSource::as_str()` (a stable label like `env(explicit)`,
`config(encrypted)`, `user_store`, ...) and the env var name; they
**never** log secret prefix or suffix of the resolved key. New log
lines that touch auth must follow the same rule — see
`architecture/auth.md` for the security policy.

### 9. Copilot (`src/provider/copilot.rs`)

GitHub Copilot implementation. Wraps `OpenAiCompatibleProvider`.

**Key features:**
- Base URL: `https://api.githubcopilot.com`
- Adds `Editor-Version: codegg/1.0` header

**Models:**
- `copilot/gpt-4o` (128K ctx)
- `copilot/o1` (200K ctx, 100K output)
- `copilot/o3-mini` (200K ctx, 100K output)
- `copilot/claude-sonnet-4` (200K ctx)

### 10. Cloudflare (`src/provider/cloudflare.rs`)

Cloudflare Workers AI. Wraps `OpenAiCompatibleProvider`.

**Key features:**
- Base URL: `https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1`

**Models:**
- `@cf/meta/llama-3.3-70b-instruct-fp8-fast` (128K ctx)
- `@cf/meta/llama-3.1-8b-instruct` (128K ctx)
- `@cf/qwen/qwen1.5-14b-chat-awq` (32K ctx)

### 11. GitLab (`src/provider/gitlab.rs`)

GitLab AI gateway. Wraps `OpenAiCompatibleProvider`.

**Key features:**
- Base URL: `https://gitlab.com/api/v4/ai/chat`
- Supports custom base URL via `with_base_url()`

**Models:**
- `gitlab/claude-sonnet-4` (200K ctx)
- `gitlab/gpt-4o` (128K ctx)

### 12. Codegg Zen (`src/provider/opencode_zen.rs`)

Codegg's own Zen service implementation. Not based on OpenAiCompatible.

**Key features:**
- Base URL: `https://opencode.ai/zen/v1`
- Implements `discover_models()` to fetch from `/models` endpoint

**Models (embedded):**
- `big-pickle` (200K ctx, 64K output) - Free
- `minimax-m2.5-free` (200K ctx, 64K output) - Free
- `nemotron-3-super-free` (128K ctx, 32K output) - Free
- `qwen3.6-plus-free` (128K ctx, 32K output) - Free

### 13. Additional Providers (`src/provider/additional.rs`)

Factory functions for additional OpenAI-compatible providers. All
OpenAI-compatible factories take a `Credential` (preserving
`CredentialKind` and `expires_at`); `create_minimax` takes a `String`
because the MiniMax endpoint is Anthropic-compatible and uses a different
auth header. The legacy `create_xai(api_key: String)` /
`create_opencode_go(api_key: String)` wrappers are kept as
backwards-compatible shims.

| Function | ID | Name | Base URL | Auth |
|----------|-----|------|----------|------|
| `create_xai(credential)` | xai | xAI | https://api.x.ai/v1 | `Credential` |
| `create_mistral(credential)` | mistral | Mistral | https://api.mistral.ai/v1 | `Credential` |
| `create_groq(credential)` | groq | Groq | https://api.groq.com/openai/v1 | `Credential` |
| `create_deepinfra(credential)` | deepinfra | DeepInfra | https://api.deepinfra.com/v1/openai | `Credential` |
| `create_cerebras(credential)` | cerebras | Cerebras | https://api.cerebras.ai/v1 | `Credential` |
| `create_cohere(credential)` | cohere | Cohere | https://api.cohere.ai/compatibility/v1 | `Credential` |
| `create_together(credential)` | together | Together AI | https://api.together.xyz/v1 | `Credential` |
| `create_perplexity(credential)` | perplexity | Perplexity | https://api.perplexity.ai | `Credential` |
| `create_venice(credential)` | venice | Venice | https://api.venice.ai/api/v1 | `Credential` |
| `create_generalcompute(credential)` | generalcompute | GeneralCompute | https://api.generalcompute.com/v1 | `Credential` |
| `create_minimax(api_key)` | minimax | MiniMax | https://api.minimax.io/anthropic | `String` (Anthropic-compatible) |
| `create_sap_ai_core(api_key, base_url)` | sap_ai_core | SAP AI Core | (config-only) | `String` |
| `create_zenmux(api_key, base_url)` | zenmux | Zenmux | (config-only) | `String` |
| `create_kilo(api_key, base_url)` | kilo | Kilo | (config-only) | `String` |
| `create_vercel_ai_gateway(api_key, base_url)` | vercel_ai_gateway | Vercel AI Gateway | (config-only) | `String` |
| `create_opencode_go(credential)` | opencode_go | OpenCode Go | https://opencode.ai/go/v1 | `Credential` |

Note: `create_minimax()` includes embedded model definitions for the
minimax-M2.7 / 2.5 / 2.1 series.

## FallbackProvider with Circuit Breaker

### FallbackProvider (`src/provider/fallback.rs:8-31`)

```rust
pub struct FallbackProvider {
    providers: Vec<Box<dyn Provider>>,
    status_codes: Vec<u16>,
    circuit_breakers: Vec<CircuitBreaker>,
}
```

**Default retryable status codes:** `[429, 500, 502, 503, 504]`

**Behavior:**
1. Iterates through providers in order
2. Checks circuit breaker before calling provider
3. On success: records success in circuit breaker, returns stream
4. On failure: records failure, checks if status code is retryable
5. If retryable: waits with exponential backoff (1s, 2s, 4s... max 30s), tries next provider
6. If not retryable: returns error immediately
7. If all fail: returns last error

### Circuit Breaker (`src/resilience/circuit.rs:44-186`)

```rust
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}

pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}
```

**Configuration options:**
- `failure_threshold`: failures before opening (default: 3)
- `timeout_secs`: seconds before trying half-open (default: 60)
- `success_threshold`: successes in half-open to close (default: 2)
- `max_half_open_duration`: max seconds in half-open (default: 30)

**State transitions:**
- Closed -> Open: after `failure_threshold` consecutive failures
- Open -> HalfOpen: after `timeout_secs` elapsed
- HalfOpen -> Closed: after `success_threshold` consecutive successes
- HalfOpen -> Open: on any failure in half-open state

## Model Discovery and Catalog

### ModelCatalog (`src/provider/catalog.rs:5-109`)

```rust
pub struct ModelCatalog {
    models: HashMap<String, ModelInfo>,
    last_fetch: Option<Instant>,
    cache_ttl: Duration,
}
```

**Features:**
- Seeds from embedded models on creation
- Can fetch live model list from `https://models.dev/api/models`
- 1-hour cache TTL
- `merge()` method to combine models from multiple sources

### Embedded Models (`src/provider/models.rs:3-46`)

```rust
pub fn embedded_models() -> Vec<ModelInfo>
```

Returns free tier models:
- Big Pickle (Free) - opencode_zen
- MiniMax M2.5 Free - opencode_zen
- Nemotron 3 Super Free - opencode_zen
- Qwen3.6 Plus Free - opencode_zen

### ModelDiscoveryService (`src/provider/discovery.rs:9-265`)

```rust
pub struct ModelDiscoveryService {
    models: Arc<RwLock<Vec<ModelInfoInternal>>>,
    last_refresh: Arc<RwLock<Option<Instant>>>,
    cache_path: PathBuf,
    ttl: Duration,
    pool: Option<SqlitePool>,
}
```

**Features:**
- Caches models in SQLite database (`cached_models` table)
- TTL-based refresh (default: 1 hour)
- Calls `provider.discover_models()` on each provider
- Handles database persistence

### ProviderCache (`src/provider/cache.rs:15-83`)

Simple in-memory cache for provider responses.

```rust
pub struct ProviderCache {
    cache: DashMap<CacheKey, CacheEntry>,
}
```

- Key: `(provider, model, input_hash)`
- TTL per entry
- `evict_expired()` method to remove expired entries

## SSE Parsing (`src/provider/sse_parser.rs`)

Handles parsing of Server-Sent Events from various providers.

### SseParser (`src/provider/sse_parser.rs:16-382`)

```rust
pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,
    args_buffer: String,
    openai_tool_states: HashMap<usize, OpenAiToolState>,
}
```

**Key functions:**
- `parse_openai_buffer()` - Parses OpenAI-compatible SSE
- `parse_anthropic_buffer()` - Parses Anthropic SSE
- Handles tool call streaming (accumulating arguments across chunks)
- Supports reasoning content via `reasoning_content` or `reasoning` fields
- Handles both `delta` and `message` tool call formats

### State Preservation

The parser preserves state across chunks via special markers in the buffer:
- `\n__TC__:{json}` - Queued tool calls
- `\n__OAI_STATE__:{json}` - OpenAI tool state for multi-part tool calls

## Text Tool Parser (`src/provider/text_tool_parser.rs`)

Parses plain text responses as potential tool calls via regex patterns.

```rust
pub fn parse_text_as_tool_calls(text: &str) -> Option<Vec<ToolCall>>
```

**Patterns:**
1. `invoke("tool_name", {...})` - Direct invocation format
2. ` ```tool_name\n{...}\n``` ` - Code block format

## Request/Response Flow

### Typical Provider Flow

1. **Build Request Body**
   - Convert `ChatRequest` to provider-specific JSON format
   - Handle system messages, user messages, assistant messages, tool messages
   - Convert `ToolDefinition` to provider-specific format
   - Add generation config (temperature, top_p, max_tokens)

2. **Send Request**
   - POST to provider endpoint
   - Include auth headers (API key, Bearer token, etc.)
   - Set content-type to application/json

3. **Handle Response**
   - Check status code (429 = RateLimit, other failures = ProviderError::api)
   - Stream response body as SSE
   - Parse SSE events into `ChatEvent` stream

4. **Stream Events**
   - Return `EventStream` (async stream of `Result<ChatEvent, ProviderError>`)
   - Events include: `TextDelta`, `ReasoningDelta`, `ToolCall`, `ToolResult`, `Finish`, `Error`

### Message Format Mapping

| Message Type | OpenAI | Anthropic | Google | Bedrock |
|-------------|--------|-----------|--------|---------|
| System | `{"role": "system", "content": ...}` | `{"type": "text", "text": ...}` in system array | Triggers initial user/model exchange | `{"text": ...}` in system array |
| User | `{"role": "user", "content": [...]}` | `{"role": "user", "content": [...]}` | `{"role": "user", "parts": [...]}` | `{"role": "user", "content": [...]}` |
| Assistant | `{"role": "assistant", "content": ..., "tool_calls": [...]}` | `{"role": "assistant", "content": [...]}` with `tool_use` parts | `{"role": "model", "parts": [...]}` with `functionCall` | `{"role": "assistant", "content": [...]}` with `toolUse` parts |
| Tool | `{"role": "tool", "tool_call_id": ..., "content": ...}` | `{"role": "user", "content": [{"type": "tool_result", ...}]}` | `{"role": "function", "parts": [{"functionResponse": ...}]}` | `{"role": "user", "content": [{"toolResult": ...}]}` |

### Tool Definition Mapping

| Provider | Format |
|----------|--------|
| OpenAI | `{"type": "function", "function": {"name": ..., "description": ..., "parameters": ...}}` |
| Anthropic | `{"name": ..., "description": ..., "input_schema": ...}` |
| Google | `{"name": ..., "description": ..., "parameters": ...}` wrapped in `function_declarations` |
| Bedrock | `{"toolSpec": {"name": ..., "description": ..., "inputSchema": {"json": ...}}}` |

## HTTP Client Configuration (`src/provider/mod.rs:46-56`)

```rust
pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
}
```

## Provider Auto-Registration Summary

### Auto-Registered via Environment Variables (`register_builtin()`)
These 15 providers register automatically if their env var is set (no config needed):
- anthropic, openai, google, openrouter, opencode_zen, mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, minimax

### Config + Env Var Fallback (`register_builtin_with_config()`)
16 providers check config first, then fall back to env vars. Includes all 15 above plus `opencode_go`. Providers are independent - adding one via config does NOT disable others.

### Config-Only Providers (NOT auto-registered)
These require explicit config entries and have no env var fallback:
- SAP AI Core, Zenmux, Kilo, Vercel AI Gateway

## Error Handling

### ProviderError (`src/error.rs` - ProviderError variant)

Key methods:
- `is_retryable()` - Determines if error should trigger fallback/retry
- Various variants: Api, RateLimit, Stream, etc.

### Retry Logic in FallbackProvider

1. Check circuit breaker status
2. On error, extract status code
3. If status code in retryable set (default: 429, 500-504), try next provider
4. Exponential backoff: `2^i` seconds (1s, 2s, 4s, 8s... max 30s)
5. Return last error if all providers exhausted

## Durable provider connections

The durable connection foundation is owned by the daemon and is additive to
the provider registry. `codegg_core::provider_connections` stores
`ProviderConnection` metadata under a typed `ProviderConnectionId`, with
explicit personal, project, or deployment scope. A connection records the
provider kind, normalized endpoint/TLS metadata, display name, lifecycle
state, an opaque `SecretRef`, and a monotonically increasing revision. It
never stores resolved credential material.

SQLite persistence is provided by `ProviderConnectionStore`. The connection
row contains only metadata plus an opaque secret reference and the
provider/account locator needed to ask the existing encrypted
`CredentialStore` for a credential. Listing, hydration, and metadata updates
do not resolve credentials or perform network probes.

The daemon's `ConnectionManager` is the lazy runtime seam. It resolves one
connection at a time, rejects disabled or missing-credential records with
typed errors, caches instances by connection ID and record revision, and
invalidates cached instances when the metadata revision changes. Legacy
`ProviderRegistry` construction remains available for environment/config
registration; the connection path does not replace or implicitly migrate
that behavior.

## Related Architecture Documents

- `architecture/core.md` - Core facade and transport adapters
- `architecture/skills.md` - Runtime skill loader
- `architecture/resilience.md` - Circuit breaker pattern details

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/codegg-providers/src/provider/mod.rs` | Provider trait, registry, core types, registration |
| `crates/codegg-providers/src/provider/anthropic.rs` | Anthropic API implementation |
| `crates/codegg-providers/src/provider/openai.rs` | OpenAI API implementation |
| `crates/codegg-providers/src/provider/google.rs` | Google Generative AI implementation |
| `crates/codegg-providers/src/provider/azure.rs` | Azure OpenAI implementation |
| `crates/codegg-providers/src/provider/vertex.rs` | Google Vertex AI (wraps OpenAiCompatible) |
| `crates/codegg-providers/src/provider/bedrock.rs` | AWS Bedrock with SigV4 signing |
| `crates/codegg-providers/src/provider/openrouter.rs` | OpenRouter aggregator |
| `crates/codegg-providers/src/provider/openai_compatible.rs` | Generic OpenAI-compatible provider |
| `crates/codegg-providers/src/provider/copilot.rs` | GitHub Copilot |
| `crates/codegg-providers/src/provider/cloudflare.rs` | Cloudflare Workers AI |
| `crates/codegg-providers/src/provider/gitlab.rs` | GitLab AI |
| `crates/codegg-providers/src/provider/opencode_zen.rs` | Codegg Zen service |
| `crates/codegg-providers/src/provider/additional.rs` | Additional provider factories |
| `crates/codegg-providers/src/provider/fallback.rs` | Fallback provider with circuit breaker |
| `crates/codegg-providers/src/provider/catalog.rs` | Model catalog with live fetch |
| `crates/codegg-providers/src/provider/discovery.rs` | Model discovery service with DB cache |
| `crates/codegg-providers/src/provider/models.rs` | Embedded model definitions |
| `crates/codegg-providers/src/provider/sse_parser.rs` | SSE parsing for streaming responses |
| `crates/codegg-providers/src/provider/text_tool_parser.rs` | Text-based tool call parsing |
| `crates/codegg-providers/src/provider/cache.rs` | Provider response cache |
| `crates/codegg-providers/src/circuit.rs` | Circuit breaker implementation |
