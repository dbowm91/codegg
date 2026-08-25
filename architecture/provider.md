# Provider Module Architecture

## Purpose

The provider crate (`crates/codegg-providers/`) is the unified LLM backend
abstraction layer. It defines the `Provider` trait, implements 15+ provider
backends, manages credential resolution through a single resolver path,
and provides resilience primitives (circuit breaker, fallback, response
cache).

**Re-export**: `codegg::provider` via `pub use codegg_providers as provider`
in `src/lib.rs`.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-providers/src/provider_core.rs` | Trait, registry, types, registration |
| `crates/codegg-providers/src/anthropic.rs` | Anthropic Messages API |
| `crates/codegg-providers/src/openai.rs` | OpenAI Chat Completions |
| `crates/codegg-providers/src/google.rs` | Google Generative Language API |
| `crates/codegg-providers/src/azure.rs` | Azure OpenAI Service |
| `crates/codegg-providers/src/vertex.rs` | Google Vertex AI (wraps OpenAI-compat) |
| `crates/codegg-providers/src/bedrock.rs` | AWS Bedrock with SigV4 signing |
| `crates/codegg-providers/src/openrouter.rs` | OpenRouter aggregator |
| `crates/codegg-providers/src/openai_compatible.rs` | Generic OpenAI-compatible |
| `crates/codegg-providers/src/copilot.rs` | GitHub Copilot |
| `crates/codegg-providers/src/cloudflare.rs` | Cloudflare Workers AI |
| `crates/codegg-providers/src/gitlab.rs` | GitLab AI gateway |
| `crates/codegg-providers/src/opencode_zen.rs` | Codegg Zen service |
| `crates/codegg-providers/src/additional.rs` | Factory functions for OpenAI-compat providers |
| `crates/codegg-providers/src/fallback.rs` | FallbackProvider with circuit breaker |
| `crates/codegg-providers/src/circuit.rs` | CircuitBreaker implementation |
| `crates/codegg-providers/src/catalog.rs` | ModelCatalog with live fetch |
| `crates/codegg-providers/src/discovery.rs` | ModelDiscoveryService with SQLite cache |
| `crates/codegg-providers/src/models.rs` | Embedded free-tier model definitions |
| `crates/codegg-providers/src/sse_parser.rs` | SSE parsing for streaming |
| `crates/codegg-providers/src/text_tool_parser.rs` | Bounded textual tool-call repair |
| `crates/codegg-providers/src/cache.rs` | Provider response cache |
| `crates/codegg-providers/src/responses_api.rs` | OpenAI Responses API adapter |
| `crates/codegg-providers/src/auth_types.rs` | Credential, CredentialStore, AuthResolver |

## How It Works

### Registration Flow

Two entry points exist:

**`register_builtin(registry)`** (`provider_core.rs:432`) — registers 15
providers, each gated on its environment variable. No config dependency.

**`register_builtin_with_config(registry, config)`** (`provider_core.rs:770`)
— the primary production path. Registers 17 providers by checking config
first, then falling back to env vars per-provider. Uses three helper
functions with a single credential resolution path:

1. `register_config_provider` — for providers with base_url override
   (anthropic, openai, google, openrouter)
2. `register_credential_provider` — for OpenAI-compatible providers that
   accept a `Credential` envelope (mistral, groq, deepinfra, cerebras,
   cohere, together, perplexity, xai, venice, opencode_go, generalcompute)
3. `register_api_key_provider` — for providers needing a static API key
   string only (opencode_zen, minimax)

If the registry is still empty after config-based registration, falls back
to `register_builtin()` for env-var-only registration. This means:
**adding any provider via config does NOT disable others**. Each provider
independently checks config then env var. Only when config-based
registration produces zero results does the pure env-var path run.

The `disabled_providers` config list is checked per-provider in each helper;
matching providers are skipped.

### Credential Resolution

`resolve_provider_credential(provider_id, cfg, env_var, store)`
(`provider_core.rs:506`) is the single resolution path. It builds a
`ResolverContext` and calls `AuthResolver::resolve`. Resolution order:

1. Explicit `auth.env` env var
2. Conventional `{PROVIDER}_API_KEY`
3. Inline `auth.value`
4. Decrypted `auth.encrypted_value` (requires `CODEGG_MASTER_KEY`)
5. User-level `CredentialStore` lookup (by `account_id`)
6. Legacy `api_key` / `encrypted_api_key` fields

`ExternalCommand` and `OAuthDevice` auth modes are parsed but return
`AuthError::Unsupported`. The previous `std::process::Command` shell-out
path has been removed.

### Provider Lifecycle (Connection M4)

Durable connections use states: `active`, `disabled`,
`credential_missing`, `provisioning_rotating`, `tombstoned`, `error`,
`stale`. Only `active` is selectable. Tombstones preserve identity until
purge succeeds with no references.

Rotation stages a new credential binding, validates the endpoint, runs
the bounded Eggpool model probe, and commits metadata in one SQLite
transaction. Connection refresh is explicit, single-flight, and bounded
by provider probe limits.

Operator flow: connect -> select -> rotate -> refresh -> disable -> delete
(tombstone) -> restore -> purge.

## Key Types & APIs

### Provider Trait (`provider_core.rs:50`)

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn clone_box(&self) -> Box<dyn Provider>;
    async fn stream(&self, request: &ChatRequest)
        -> Result<EventStream, ProviderError>;
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    async fn ping(&self) -> Result<bool, ProviderError>;
}
```

### ProviderCapabilities (`provider_core.rs:94`)

Per-provider capability flags for tool deferral, request limits, and hosted
programmatic tool calling. `for_provider(id)` returns capabilities for
anthropic (defer loading, tool references) and openai (full Responses API,
hosted programs, nested calls). Others default to no special capabilities.

### ChatRequest (`provider_core.rs:172`)

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

### Message (`provider_core.rs:186`)

Tagged enum (`#[serde(tag = "role")]`): `System`, `User` (Vec<ContentPart>),
`Assistant` (Vec<ContentPart> + tool_calls), `Tool` (tool_call_id + content).

### ContentPart (`provider_core.rs:251`)

Untagged enum: `Text { text }`, `Image { image_url }`, `Reasoning { text, visibility }`.
`Reasoning` is `#[serde(skip)]` — never serialized on the wire. Max 256KB.

### ChatEvent (`provider_core.rs:299`)

Streaming response events: `TextDelta`, `ReasoningDelta`, `ToolCall`,
`ToolResult`, `Finish { stop_reason, usage }`, `Error`.

### ToolDefinition (`provider_core.rs:341`)

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_loading: Option<bool>,
}
```

Methods: `to_openai()`, `to_anthropic()` — convert to provider-specific
wire format. Both handle `defer_loading`.

### ModelInfo (`provider_core.rs:389`)

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

### ProviderRegistry (`provider_core.rs:401`)

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn Provider>>,
}
// new(), register(), get(), list()
```

### ProviderCapabilities::for_provider (`provider_core.rs:130`)

Returns provider-specific capabilities. Anthropic: defer loading +
tool references. OpenAI: full Responses API + hosted programs +
nested calls + 128 tools/request + python hosted language.

### EventStream (`provider_core.rs:34`)

```rust
pub type EventStream = Pin<Box<dyn Stream<Item =
    Result<ChatEvent, ProviderError>> + Send>>;
```

### FallbackProvider (`fallback.rs:8`)

```rust
pub struct FallbackProvider {
    providers: Vec<Box<dyn Provider>>,
    status_codes: Vec<u16>,
    circuit_breakers: Vec<CircuitBreaker>,
}
```

Default retryable codes: `[429, 500, 502, 503, 504]`. Iterates providers
in order, checks circuit breaker before calling, exponential backoff on
retryable failures (1s, 2s, 4s... max 30s).

### CircuitBreaker (`circuit.rs:43`)

```rust
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}
pub enum CircuitState { Closed, Open, HalfOpen }
```

Defaults: `failure_threshold=3`, `timeout_secs=60`, `success_threshold=2`,
`max_half_open_duration=30s`. Transitions: Closed->Open after threshold,
Open->HalfOpen after timeout, HalfOpen->Closed after success threshold.

## Registration Summary

### 15 Env-Var Providers (`register_builtin()`)

Registered if the corresponding env var is set:

| Env Var | Provider |
|---------|----------|
| `ANTHROPIC_API_KEY` | Anthropic |
| `OPENAI_API_KEY` | OpenAI |
| `GOOGLE_API_KEY` | Google |
| `OPENROUTER_API_KEY` | OpenRouter |
| `OPENCODE_ZEN_API_KEY` | OpenCode Zen |
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

### 17 Config+Env Providers (`register_builtin_with_config()`)

Same 15 plus `opencode_go` (OPENCODE_GO_API_KEY) and
`generalcompute` (GENERALCOMPUTE_API_KEY). Each checks config first,
then env var. **Per-provider independence**: adding one via config does
NOT suppress others. `register_builtin()` is only called as fallback
if registry is empty after all config-based attempts.

### Config-Only Providers (NOT auto-registered)

SAP AI Core, Zenmux, Kilo, Vercel AI Gateway — require explicit
`provider.<id>` config entries.

## Provider Implementations

### Anthropic (`anthropic.rs`)

- Base URL: `https://api.anthropic.com`
- API version: `2023-06-01`
- SSE streaming with `stream: true`
- Thinking budget via `thinking.budget_tokens`
- Hardcoded models: claude-sonnet-4-20250514, claude-opus-4-20250514,
  claude-3-5-sonnet-20241022, claude-3-5-haiku-20241022

### OpenAI (`openai.rs`)

- OpenAiConfig: api_key, base_url, provider_id, provider_name,
  requires_org_header, organization, omit_stream_options, tool_choice
- Factory methods: `default_with_key`, `openai`, `groq`, `xai`,
  `mistral`, `cerebras`

### Google (`google.rs`)

- `streamGenerateContent` with SSE
- Custom `contents` array format
- Tool defs as `function_declarations` in `tools` array
- Models: gemini-2.5-pro, gemini-2.5-flash, gemini-2.0-flash

### Azure (`azure.rs`)

- Endpoint: `{endpoint}/openai/deployments/{model}/chat/completions?api-version=2024-10-21`
- `api-key` header auth
- Models: gpt-4.1, gpt-4o

### Bedrock (`bedrock.rs`)

- AWS SigV4 signing
- Endpoint: `https://bedrock-runtime.{region}.amazonaws.com/model/{model}/converse-stream`
- Session token support for temp credentials
- Custom SSE for Bedrock event stream format
- Models: anthropic.claude-sonnet-4, anthropic.claude-3-5-sonnet, meta.llama3-1-405b

### OpenAI Compatible (`openai_compatible.rs`)

Generic provider. `OpenAiCompatibleConfig` holds `Credential` (not raw API
key). Factory methods: `simple()` (wraps key in `Credential::api_key`),
`simple_with_credential()` (preserves `CredentialKind` and `expires_at`).
30-second chunk timeout. Dynamic model discovery via `/models`.

### Additional Factories (`additional.rs`)

| Function | ID | Base URL | Auth |
|----------|-----|----------|------|
| `create_xai` | xai | https://api.x.ai/v1 | Credential |
| `create_mistral` | mistral | https://api.mistral.ai/v1 | Credential |
| `create_groq` | groq | https://api.groq.com/openai/v1 | Credential |
| `create_deepinfra` | deepinfra | https://api.deepinfra.com/v1/openai | Credential |
| `create_cerebras` | cerebras | https://api.cerebras.ai/v1 | Credential |
| `create_cohere` | cohere | https://api.cohere.ai/compatibility/v1 | Credential |
| `create_together` | together | https://api.together.xyz/v1 | Credential |
| `create_perplexity` | perplexity | https://api.perplexity.ai | Credential |
| `create_venice` | venice | https://api.venice.ai/api/v1 | Credential |
| `create_generalcompute` | generalcompute | https://api.generalcompute.com/v1 | Credential |
| `create_minimax` | minimax | https://api.minimax.io/anthropic | String |
| `create_sap_ai_core` | sap_ai_core | (config-only) | String |
| `create_zenmux` | zenmux | (config-only) | String |
| `create_kilo` | kilo | (config-only) | String |
| `create_vercel_ai_gateway` | vercel_ai_gateway | (config-only) | String |
| `create_opencode_go` | opencode_go | https://opencode.ai/go/v1 | Credential |

`create_minimax` takes `String` because the MiniMax endpoint is
Anthropic-compatible and uses a different auth header.

### Codegg Zen (`opencode_zen.rs`)

- Base URL: `https://opencode.ai/zen/v1`
- Implements `discover_models()` to fetch from `/models`
- Embedded models: big-pickle, minimax-m2.5-free,
  nemotron-3-super-free, qwen3.6-plus-free

## SSE Parsing (`sse_parser.rs`)

SseParser handles OpenAI-compatible and Anthropic SSE. Tool call streaming
accumulates arguments across chunks. State preserved via markers:
`\n__TC__:{json}` (queued tool calls),
`\n__OAI_STATE__:{json}` (OpenAI tool state).

### Text Tool Repair (`text_tool_parser.rs`)

Bounded repair, not generic prose scanning. Only activates when the model
adapter explicitly sets `text_tool_repair` to `hermes_xml`, `invoke_json`,
or `raw_json_envelope`. Validates against the current tool surface and
argument schema. Unconfigured adapters never scan assistant prose.

## HTTP Client (`provider_core.rs:22`)

```rust
pub fn create_http_client() -> reqwest::Client {
    // 60s timeout, 10s connect, 32 idle per host, 30s keepalive
}
```

## Invariants & Gotchas

- **Single resolution path**: `resolve_provider_credential` is the only
  credential resolver. No helper reads `cfg.api_key` directly.
- **Per-provider config independence**: Adding `anthropic` via config does
  NOT suppress `openai` env-var registration. Only if the registry is empty
  after all config-based registrations does `register_builtin()` run.
- **Secret logging forbidden**: Auth log lines use `ResolvedAuthSource::as_str()`
  (e.g. `env(explicit)`, `config(encrypted)`). Never log secret prefix/suffix.
- **ExternalCommand disabled**: Both `AuthResolver::resolve` and
  `ExternalCommandProvider::fetch` return `AuthError::Unsupported`.
- **ContentPart::Reasoning is skip-serialized**: Never appears on the wire.
- **ToolDefinition.defer_loading**: Annotated on each tool definition;
  providers that support it (anthropic, openai) honor it; others ignore it.
- **FallbackProvider creates one CircuitBreaker per provider** at
  construction time, not lazily.

## Testing

```bash
cargo test -p codegg-providers              # all provider tests
cargo test -p codegg-providers -- sse       # SSE parser tests
cargo test -p codegg-providers -- circuit    # circuit breaker tests
cargo test -p codegg-providers -- fallback   # fallback provider tests
```

## Related Docs

- `architecture/core.md` — core facade and transport adapters
- `architecture/config.md` — provider config schema and merging
- `architecture/auth.md` — auth security policy
- `architecture/resilience.md` — circuit breaker pattern details
