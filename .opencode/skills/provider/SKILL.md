---
name: provider
description: Provider system architecture and registration patterns in opencode-rs
version: 1.2.0
tags:
  - provider
  - llm
  - registration
  - anthropic
  - openai
  - auth
---

# Provider System Guide

This skill covers the LLM provider system in opencode-rs.

## Overview

Providers implement the `Provider` trait to communicate with various LLM backends. The system supports:
- **Direct providers**: Anthropic, OpenAI, Google Vertex, AWS Bedrock, etc.
- **Additional providers**: Mistral, Groq, DeepInfra, Cerebras, Cohere, Together AI, Perplexity, xAI, Venice
- **Discovery providers**: Cloudflare, Copilot, GitLab, OpenRouter, OpenAI Compatible

## Provider Trait

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

## Registration Helper Functions

The provider registration system uses helper functions to reduce code duplication:

### register_builtin

Registers providers from environment variables (no config required). Called automatically if `register_builtin_with_config` finds no providers:

```rust
pub fn register_builtin(registry: &mut ProviderRegistry);
```

Providers registered: ANTHROPIC_API_KEY, OPENAI_API_KEY, GOOGLE_API_KEY, OPENROUTER_API_KEY, CODEGG_ZEN_API_KEY, MISTRAL_API_KEY, GROQ_API_KEY, DEEPINFRA_API_KEY, CEREBRAS_API_KEY, COHERE_API_KEY, TOGETHERAI_API_KEY, PERPLEXITY_API_KEY, XAI_API_KEY, VENICE_API_KEY, MINIMAX_API_KEY, OPENCODE_GO_API_KEY, GENERALCOMPUTE_API_KEY. `register_builtin` wraps the env-var key in `Credential::api_key(...)` so the OpenAI-compatible factories (which now accept a `Credential`) get a uniform envelope.

### register_builtin_with_config

The main public entry point. Builds a single
`Arc<CredentialStore>` at the top (warns and continues if it cannot be
opened) and threads it into every per-provider helper. The
`CredentialStore` is what makes `AuthConfig::Stored` actually resolvable
during registration.

```rust
pub fn register_builtin_with_config(
    registry: &mut ProviderRegistry,
    config: &crate::config::schema::Config,
);
```

If no providers are registered (because nothing was configured), it
falls back to `register_builtin(registry)`.

### Centralized credential resolution

```rust
pub(crate) fn resolve_provider_credential(
    provider_id: &str,
    cfg: Option<&ProviderConfig>,
    env_var: Option<&str>,
    store: Option<&Arc<CredentialStore>>,
) -> Result<Option<ResolvedAuth>, AuthError>;
```

Builds a `ResolverContext` from the legacy `ProviderConfig` fields and
the shared credential store, then calls `AuthResolver::resolve`. The
full `ResolvedAuth` is returned (not just the secret) so the caller can
inspect `CredentialKind` and `source`.

### register_credential_provider

For OpenAI-compatible providers that accept a full `Credential` envelope
(preserves `CredentialKind` and `expires_at`):

```rust
fn register_credential_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&HashMap<String, ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    store: Option<&Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(Credential) -> Box<dyn Provider>,
```

Used for `mistral`, `groq`, `deepinfra`, `cerebras`, `cohere`, `together`,
`perplexity`, `xai`, `venice`, `opencode_go`, `generalcompute`.

### register_api_key_provider

For providers that genuinely need a static API-key string. Resolves a
credential through `resolve_provider_credential` and rejects
`CredentialKind::BearerToken` with a `tracing::warn!` (the provider is
skipped because callers of this path cannot accept a bearer token):

```rust
fn register_api_key_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&HashMap<String, ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    env_var: &str,
    store: Option<&Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(String) -> Box<dyn Provider>,
```

Used for `opencode_zen` and `minimax` (Anthropic-compatible, different
auth header).

### register_config_provider

For providers with a configurable `base_url`. Resolves a credential and
threads the (possibly absent) `base_url` through to the factory:

```rust
fn register_config_provider<F>(
    registry: &mut ProviderRegistry,
    providers: Option<&HashMap<String, ProviderConfig>>,
    disabled: Option<&Vec<String>>,
    name: &str,
    store: Option<&Arc<CredentialStore>>,
    factory: F,
) where
    F: FnOnce(String, Option<String>) -> Box<dyn Provider>,
```

Used for `anthropic`, `openai` (native), `google`, `openrouter`.

## Provider Module Structure

Provider implementations live in `crates/codegg-providers/src/` and are re-exported as `codegg::provider` via `pub use codegg_providers as provider;`.

## Available Providers

### Direct Providers
- **Anthropic**: Claude models via `ANTHROPIC_API_KEY`
- **OpenAI**: GPT models via `OPENAI_API_KEY`
- **Google**: Gemini models via `GOOGLE_API_KEY` or `VERTEX_PROJECT_ID`
- **Azure**: Azure OpenAI via `AZURE_OPENAI_*` config
- **AWS Bedrock**: Claude via Bedrock via `AWS_*` config

### Additional Providers (from `crates/codegg-providers/src/additional.rs`)
- **Mistral**: `MISTRAL_API_KEY` via `create_mistral()`
- **Groq**: `GROQ_API_KEY` via `create_groq()`
- **DeepInfra**: `DEEPINFRA_API_KEY` via `create_deepinfra()`
- **Cerebras**: `CEREBRAS_API_KEY` via `create_cerebras()`
- **Cohere**: `COHERE_API_KEY` via `create_cohere()`
- **Together AI**: `TOGETHERAI_API_KEY` via `create_together()`
- **Perplexity**: `PERPLEXITY_API_KEY` via `create_perplexity()`
- **xAI**: `XAI_API_KEY` via `create_xai()`
- **Venice**: `VENICE_API_KEY` via `create_venice()`
- **MiniMax**: `MINIMAX_API_KEY` via `create_minimax()`
- **Codegg Go**: `CODEGG_GO_API_KEY` via `create_codegg_go()`

### Config-Based Providers (require base_url)
- **SAP AI Core**: via `SAP_AI_CORE_*` config
- **Zenmux**: via `ZENMUX_*` config
- **Kilo**: via `KILO_*` config
- **Vercel AI Gateway**: via `VERCEL_AI_GATEWAY_*` config

### Discovery Providers
- **Cloudflare Workers AI**: `CLOUDFLARE_*` config
- **GitHub Copilot**: `GITHUB_TOKEN` or `COPILOT_*` config
- **GitLab**: `GITLAB_*` config
- **OpenRouter**: `OPENROUTER_*` config
- **OpenAI Compatible**: Generic OpenAI-compatible API

### Provider-Specific Base URLs
Some providers require custom base URLs configured in config:
```json
{
  "providers": {
    "anthropic": { "api_key": "...", "base_url": "https://api.anthropic.com" },
    "openrouter": { "api_key": "...", "base_url": "https://openrouter.ai/api/v1" },
    "openai_compatible": { "api_key": "...", "base_url": "https://your-endpoint.com/v1" }
  }
}
```

### OpenAI-compatible factory functions

`crates/codegg-providers/src/additional.rs` factory functions take a `Credential` (not
a raw `String`) so the registered provider can preserve the credential
kind / `expires_at` metadata:

```rust
pub fn create_mistral(credential: Credential) -> impl Provider
pub fn create_groq(credential: Credential) -> impl Provider
pub fn create_xai(credential: Credential) -> impl Provider
// ... etc.
```

`create_minimax` is the exception — it returns an Anthropic-compatible
provider and takes an `api_key: String` because the MiniMax endpoint
uses a different auth header.

`OpenAiCompatibleProvider::simple_with_credential(id, name, credential, base_url)`
is the underlying constructor. The legacy
`OpenAiCompatibleProvider::simple(id, name, api_key, base_url)` is kept
as a backwards-compatible shim that wraps the API key in
`Credential::api_key(...)`. New code should prefer
`simple_with_credential`.

## Adding a New Provider

1. Create provider module (e.g., in `crates/codegg-providers/src/`)
2. Implement `Provider` trait with `clone_box()`
3. Add module declaration to `crates/codegg-providers/src/lib.rs`
4. Add registration using `register_credential_provider` (for
   OpenAI-compatible providers), `register_api_key_provider` (for
   providers that need a static API key string), or
   `register_config_provider` (for providers with a configurable
   `base_url`).
5. If using config-based pattern, ensure `ProviderConfig` handling is complete

## Provider Implementation Best Practices

### HTTP Client Configuration

All providers must configure timeouts on the HTTP client to prevent hanging requests:

```rust
use std::time::Duration;

pub struct NewProvider {
    client: reqwest::Client,
}

impl NewProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }
}
```

### Streaming Buffer Limits

All streaming implementations must have buffer size limits to prevent unbounded memory growth:

```rust
const MAX_BUFFER_SIZE: usize = 1024 * 1024;  // 1MB limit

// In the streaming unfold closure:
Some(Ok(bytes)) => {
    let text = String::from_utf8_lossy(&bytes).to_string();
    buffer.push_str(&text);
    if buffer.len() > MAX_BUFFER_SIZE {
        return Some((
            Err(ProviderError::Stream("response buffer exceeded limit".to_string())),
            (stream, buffer),
        ));
    }
}
```

### Rate Limit Detection

All providers must detect 429 TOO_MANY_REQUESTS responses:

```rust
if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
    return Err(ProviderError::RateLimit);
}
```

### Blocking Operations

CPU-bound operations (crypto, heavy computation) must be wrapped in `spawn_blocking`:

```rust
let result = tokio::task::spawn_blocking(move || {
    // CPU-bound work here
    compute_signature(data)
})
.await
.map_err(|e| ProviderError::Api(format!("spawn_blocking failed: {}", e)))??;
```

## SSE Parser Unification

A unified SSE parser exists in `crates/codegg-providers/src/sse_parser.rs` used by most providers. However, `src/mcp/remote.rs` uses inline SSE parsing. Future work could unify this.

## Tool Definition Format Unification

The `ToolDefinition` struct provides adapter methods for different provider formats:

```rust
impl ToolDefinition {
    pub fn to_openai(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }

    pub fn to_anthropic(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters,
        })
    }
}
```

**Usage in providers:**
```rust
// OpenAI provider
let tool_defs: Vec<serde_json::Value> = tools.iter().map(|t| t.to_openai()).collect();

// Anthropic provider
let tool_defs: Vec<serde_json::Value> = tools.iter().map(|t| t.to_anthropic()).collect();
```

This eliminates code duplication while allowing provider-specific formatting.

## Base Directory
Relative paths in this skill are relative to the codebase root.

## Provider Transcript Golden Tests (Packet 7)

Located in `tests/provider_transcripts.rs`. These tests verify that provider serialization correctly handles tool calls, tool results, and message ordering.

### OpenAI Serializer Tests

```rust
#[test]
fn test_openai_serialize_assistant_with_tool_calls() {
    let provider = OpenAiProvider::new(OpenAiConfig::default());
    let messages = vec![
        Message::Assistant {
            content: vec![text_content("I'll use echo_args")],
            tool_calls: vec![tc("call_1", "echo_args", json!({"value": "hello"}))],
        },
        tool_msg("call_1", r#"{"value":"hello"}"#),
    ];
    let body = provider.build_body(&request);
    // Verify tool_calls array, function name, arguments
}
```

Key test patterns:
- `test_openai_serialize_user_message()` - Basic user message
- `test_openai_serialize_assistant_with_tool_calls()` - Tool calls with arguments
- `test_openai_serialize_text_plus_tool_call_same_turn()` - Mixed content + tool calls
- `test_openai_serialize_multiple_tool_calls()` - Multiple tool calls in one message
- `test_openai_serialize_multiple_tool_results()` - Multiple tool results
- `test_openai_serialize_denied_tool_result()` - Empty content for denied tools

### Anthropic Serializer Tests

```rust
#[test]
fn test_anthropic_serialize_assistant_tool_use() {
    let provider = AnthropicProvider::new("test-key".to_string());
    // Verify content array with text and tool_use parts
    // tool_use part has: type, id, name, input
}
```

Key test patterns:
- `test_anthropic_serialize_assistant_tool_use()` - Tool use in content array
- `test_anthropic_serialize_tool_result()` - Tool result in user message content
- `test_anthropic_serialize_multiple_tool_calls()` - Multiple tool_use parts
- `test_anthropic_serialize_denied_tool_result()` - Empty tool result content

### Tool Result ID Matching

Critical invariant: tool result IDs must match assistant tool call IDs:

```rust
#[test]
fn test_tool_result_id_matches_assistant_tool_call_id() {
    // After provider serialization, verify:
    // assistant tool_calls[].id == tool_result tool_call_id (OpenAI)
    // assistant content[].id == tool_result tool_use_id (Anthropic)
}
```

### Compaction Preserves IDs

```rust
#[test]
fn test_compaction_preserves_assistant_tool_call_and_tool_result_pair() {
    let result = compact_messages(messages, CompactionStrategy::DropMiddleMessages);
    // Verify assistant comes before tool result
    // Verify tool_call ID is preserved
}
```

## ScriptedProvider for Testing

The `ScriptedProvider` in `tests/agent_loop_harness.rs` enables deterministic provider testing:

```rust
#[derive(Clone)]
struct ScriptedProvider {
    responses: Vec<Vec<ChatEvent>>,  // Each inner vec = one turn
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
}

// Usage in tests:
let responses = vec![
    vec![ChatEvent::ToolCall(...), ChatEvent::Finish {...}],  // Turn 1
    vec![ChatEvent::TextDelta(...), ChatEvent::Finish {...}], // Turn 2
];
let provider = Box::new(ScriptedProvider::new(responses));
let requests = provider.get_requests().await;  // Inspect recorded requests
```

## Recent Updates (2026-05-22)

### ProviderError::is_retryable()

`ProviderError` has an `is_retryable()` method for determining if a provider error should trigger retry logic:

```rust
impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Timeout(_)
                | ProviderError::Stream(_)
                | ProviderError::CircuitOpen(_)
        )
    }
}
```

The agent loop uses this method for retry determination.

### CircuitOpen Integration (2026-05-22)

`FallbackProvider` uses `ProviderError::CircuitOpen` when a circuit breaker is open:

```rust
if !cb.is_available().await {
    last_error = Some(ProviderError::CircuitOpen(provider.name().to_string()));
    continue;
}
```

This propagates circuit-open errors properly, which map to HTTP 502 in the error module's `IntoResponse`.

## Message Types with Arc<String>

The `Message` enum, `ToolCall` struct, and `ChatEvent` enum use `Arc<String>` for content fields:

```rust
pub enum Message {
    System { content: Arc<String> },
    User { content: Vec<ContentPart> },
    Assistant { content: Vec<ContentPart> },
    Tool { tool_call_id: Arc<String>, content: Arc<String> },
}

pub struct ToolCall {
    pub id: Arc<String>,
    pub name: Arc<String>,
    pub arguments: serde_json::Value,
}

pub enum ContentPart {
    Text { text: Arc<String> },
    Image { image_url: ImageUrl },
}

pub struct ImageUrl {
    pub url: Arc<String>,
}

pub enum ChatEvent {
    TextDelta(Arc<String>),
    ReasoningDelta(Arc<String>),
    ToolCall(ToolCall),
    ToolResult { tool_call_id: Arc<String>, content: Arc<String> },
    Finish { stop_reason: Arc<String>, usage: TokenUsage },
    Error(Arc<String>),
}
```

**When creating these types:**
```rust
// Use .into() to convert String to Arc<String>
Message::System { content: "hello".into() }
ContentPart::Text { text: some_string.into() }
ToolCall { id: id.into(), name: name.into(), arguments }
```

**When comparing Arc<String> with &str:**
```rust
// Use &*arc_string == "literal" or arc_string.as_str() == "literal"
if &*tc.name == "question" { ... }
```
