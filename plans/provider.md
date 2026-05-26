# Provider Module Architecture Review Findings

## Verified Claims

- **Provider trait** (lines 60-73): Correctly documented with `stream`, `models`, `discover_models`, `ping` methods
- **ChatRequest struct** (lines 97-107): Matches exactly with all fields
- **Message enum** (lines 109-126): `System`, `User`, `Assistant`, `Tool` variants with Arc<String> content
- **ContentPart enum** (lines 128-133): `Text` and `Image` variants
- **ImageUrl struct** (lines 135-138): Arc<String> url field
- **ChatEvent enum** (lines 140-154): All variants matching including `ReasoningDelta`
- **ToolCall struct** (lines 166-171): id, name, arguments fields
- **TokenUsage struct** (lines 173-179): All token fields including `reasoning_tokens`
- **ModelInfo struct** (lines 219-229): All fields matching
- **ResponseFormat enum** (lines 157-164): `JsonObject` and `JsonSchema` variants
- **ModelVariant struct** (lines 209-217): All fields matching
- **ToolDefinition** (lines 181-207): `to_openai()` and `to_anthropic()` methods present
- **ProviderRegistry** (lines 231-260): `register`, `get`, `list` methods present
- **ModelCatalog** (catalog.rs): Seeds from embedded models, fetches from models.dev API
- **ProviderError::is_retryable()** (error.rs): Correctly includes `Auth`, `RateLimit`, `Timeout`, `Stream`, `CircuitOpen`
- **MAX_BUFFER_SIZE = 1024 * 1024** (provider/mod.rs:44): Correct constant
- **create_http_client()** (provider/mod.rs:46-56): Exact builder pattern matches
- **register_builtin()** (provider/mod.rs:262-309): All 14 environment variables registered
- **register_env_fallback_provider()** (provider/mod.rs:332-371): Signature matches
- **register_config_provider()** (provider/mod.rs:311-330): Signature matches
- **register_builtin_with_config()** (provider/mod.rs:373-520): Registers providers via config with env fallback

## Stale Information

- **Lines 36-49 list SAP AI Core, Zenmux, Kilo, Vercel AI Gateway**: These appear in `additional.rs` as factory functions but are NOT called from `register_builtin_with_config()` - only `codegg_go` is registered at line 513-514. The others require `base_url` and are likely configured via config-based providers.
- **Lines 51-58 "Discovery Providers"**: The category label implies auto-discovery, but `cloudflare.rs`, `copilot.rs`, `gitlab.rs`, `openai_compatible.rs` are standalone providers not auto-discovered. The doc doesn't clarify this is just a categorization, not behavior.

## Bugs Found

None significant.

## Improvements Suggested

1. **Misleading Section Title**: Lines 51-58 called "Discovery Providers" but these don't auto-discover. Should be labeled "Additional OpenAI-Compatible Providers" or similar to reflect they use `OpenAiCompatibleProvider::simple()`.

2. **Incomplete Registration Info**: The table at lines 31-49 lists factory functions, but doesn't clarify that only some are auto-registered via `register_env_fallback_provider`. Others (SAP AI Core, Zenmux, Kilo, Vercel AI Gateway) require explicit config registration.

## Cross-Module Issues

- **resilience.md** (line 145) documents `FallbackProvider` default parameters correctly, but missing that `max_half_open_duration` is 30 seconds in `CircuitBreakerInner::new()` at circuit.rs:66.
