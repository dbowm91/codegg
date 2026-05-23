# Provider Module Architecture Review

## Verified Claims

### Core Traits and Types

| Claim | Status | Evidence |
|-------|--------|----------|
| `Provider` trait with `id()`, `name()`, `clone_box()`, `stream()`, `models()`, `discover_models()`, `ping()` | VERIFIED | `src/provider/mod.rs:60-73` |
| `ChatRequest` struct with messages, model, tools, system, temperature, top_p, max_tokens, response_format | VERIFIED | `src/provider/mod.rs:97-107` |
| `Message` enum with System/User/Assistant/Tool variants and `Arc<String>` content fields | VERIFIED | `src/provider/mod.rs:109-126` |
| `ContentPart` enum (Text, Image) | VERIFIED | `src/provider/mod.rs:128-133` |
| `ImageUrl` struct | VERIFIED | `src/provider/mod.rs:135-138` |
| `ChatEvent` enum (TextDelta, ReasoningDelta, ToolCall, ToolResult, Finish, Error) | VERIFIED | `src/provider/mod.rs:140-154` |
| `ToolCall` struct with `Arc<String>` id/name and arguments | VERIFIED | `src/provider/mod.rs:166-171` |
| `ToolDefinition` with `name`, `description`, `parameters` and `to_openai()`/`to_anthropic()` | VERIFIED | `src/provider/mod.rs:181-207` |
| `TokenUsage` with input/output/total/reasoning_tokens | VERIFIED | `src/provider/mod.rs:173-179` |
| `ModelInfo` struct with all fields including `variants: Vec<ModelVariant>` | VERIFIED | `src/provider/mod.rs:219-229` |
| `ModelVariant` struct with suffix, context_window_override, max_output_override, extra_params, prompt | VERIFIED | `src/provider/mod.rs:209-217` |
| `ResponseFormat` enum (JsonObject, JsonSchema) | VERIFIED | `src/provider/mod.rs:157-164` |
| `EventStream` type alias | VERIFIED | `src/provider/mod.rs:58` |
| `ProviderRegistry` with register/get/list | VERIFIED | `src/provider/mod.rs:231-260` |

### Key Components

| Claim | Status | Evidence |
|-------|--------|----------|
| `ModelCatalog` with models HashMap, last_fetch Option<Instant>, cache_ttl Duration | VERIFIED | `src/provider/catalog.rs:5-10` |
| `ProviderCache` with DashMap store | VERIFIED | `src/provider/cache.rs:15-17` (note: field is `cache` not `store`) |
| `FallbackProvider` with providers, status_codes Vec<u16>, circuit_breakers | VERIFIED | `src/provider/fallback.rs:8-12` |
| `SseParser` with buffer, delimiter, is_openai, current_tool, args_buffer | VERIFIED | `src/provider/sse_parser.rs:16-24` |
| `MAX_BUFFER_SIZE` = 1MB | VERIFIED | `src/provider/mod.rs:44` |
| `create_http_client()` with timeout/connection/pool settings | VERIFIED | `src/provider/mod.rs:46-56` |

### Registration Patterns

| Claim | Status | Evidence |
|-------|--------|----------|
| `register_builtin()` registers 15 providers via env vars | VERIFIED | `src/provider/mod.rs:262-309` |
| `register_builtin_with_config()` exists | VERIFIED | `src/provider/mod.rs:373-520` |
| `register_config_provider()` internal helper | VERIFIED | `src/provider/mod.rs:311-330` |
| `register_env_fallback_provider()` internal helper | VERIFIED | `src/provider/mod.rs:332-371` |

### ProviderError

| Claim | Status | Evidence |
|-------|--------|----------|
| `ProviderError::NotFound`, `Api`, `Stream`, `RateLimit`, `Auth`, `ModelNotFound`, `Timeout`, `CircuitOpen` | VERIFIED | `src/error.rs:111-139` |
| `ProviderError::api()` constructor | VERIFIED | `src/error.rs:142-148` |
| `ProviderError::api_with_url()` constructor | VERIFIED | `src/error.rs:150-160` |
| `ProviderError::is_retryable()` returns true for RateLimit, Timeout, Stream, CircuitOpen, Auth | VERIFIED | `src/error.rs:162-171` (note: Auth included, doc showed only RateLimit/Timeout/Stream/CircuitOpen) |
| `From<CircuitError>` conversion | VERIFIED | `src/error.rs:205-213` |

### Providers

| Claim | Status | Evidence |
|-------|--------|----------|
| All provider files exist (anthropic, openai, google, azure, vertex, bedrock, openrouter, codegg_zen) | VERIFIED | Glob results |
| Additional providers (create_mistral, create_groq, etc.) | VERIFIED | `src/provider/additional.rs:4-179` |
| CodeggZen models (big-pickle, minimax-m2.5-free, nemotron-3-super-free, qwen3.6-plus-free) | VERIFIED | `src/provider/models.rs:3-46` |

### FallbackProvider Behavior

| Claim | Status | Evidence |
|-------|--------|----------|
| Default status codes [429, 500, 502, 503, 504] | VERIFIED | `src/provider/fallback.rs:16-17` |
| Exponential backoff: 2^i seconds capped at 30s | VERIFIED | `src/provider/fallback.rs:107` |
| Circuit breaker uses failure_threshold=3, timeout=60s, success_threshold=2 | VERIFIED | `src/provider/fallback.rs:23` |

---

## Bugs/Discrepancies Found

### High Priority

1. **SSE Parser Drain Off-By-One Error**
   - **Location**: `src/provider/sse_parser.rs:48-50`
   - **Issue**: After draining the chunk up to the delimiter, the second `drain(..delimiter.len())` drains from position 0 of the *remaining* buffer, not the original position
   - **Example**: Buffer "line1\n\nline2\n\n" with delimiter "\n\n":
     - Line 49 drains "line1\n", buffer becomes "line2\n\n"
     - Line 50 drains first 2 chars of buffer = "li", buffer becomes "ne2\n\n"
   - **Impact**: Parser may misalign for certain message sequences
   - **Fix**: Change line 50 to `self.buffer.drain(idx + self.delimiter.len()..)` but `idx` is not in scope. Should track original length or restructure
   - **Note**: Previous review incorrectly marked this as correct; the drain pattern is buggy

2. **Google Provider ToolCall ID Ignored**
   - **Location**: `src/provider/google.rs:353`
   - **Issue**: `let id = uuid::Uuid::new_v4().to_string();` - generates new UUID instead of extracting from response
   - **Impact**: Tool result correlation with original tool call may fail
   - **Fix**: Extract and use the `id` field from the `functionCall` response

3. **Anthropic Beta Header Not Configurable**
   - **Location**: `src/provider/anthropic.rs:178`
   - **Issue**: Hardcoded `anthropic-beta: prompt-caching-2024-07-31` header may fail for non-beta accounts
   - **Impact**: API requests may fail 400 for accounts not enrolled in prompt caching beta
   - **Fix**: Make beta feature conditional or at minimum make header name configurable via config

### Medium Priority

4. **ProviderCache Clock Skew Handling**
   - **Location**: `src/provider/cache.rs:47-52`
   - **Issue**: If `entry.timestamp_secs > now` (future timestamp due to clock skew), subtraction underflows
   - **Impact**: Entries could persist indefinitely or be incorrectly expired
   - **Fix**: Use `saturating_sub` instead of normal subtraction

5. **OpenAI Tool Arguments Double Serialization**
   - **Location**: `src/provider/openai.rs:178` and `src/provider/openai_compatible.rs:132`
   - **Issue**: `tc.arguments.to_string()` may double-serialize if arguments is already a JSON string
   - **Impact**: Tool calls may receive `"{\"key\":\"value\"}"` instead of `{"key":"value"}`
   - **Fix**: Check if arguments is string and pass directly, or serialize properly

6. **register_env_fallback_provider Silent Failure**
   - **Location**: `src/provider/mod.rs:364-368`
   - **Issue**: Empty key logs at `debug!` level - may be invisible in production
   - **Impact**: Hard to debug why provider wasn't registered
   - **Fix**: Consider `warn!` level for "no key found" scenario

7. **status_code() Non-Numeric Error Codes**
   - **Location**: `src/provider/fallback.rs:131-137`
   - **Issue**: `code.parse::<u16>()` fails for non-numeric codes like "rate_limit_exceeded"
   - **Impact**: Non-numeric error codes aren't treated as retryable even when they should be
   - **Fix**: Match specific error codes/patterns instead of parsing as u16

### Low Priority

8. **ProviderCache DefaultHasher**
   - **Location**: `src/provider/cache.rs:33-36`
   - **Issue**: Uses `DefaultHasher` which may have hash collision issues
   - **Fix**: Consider `AHasher` for better distribution

9. **Discovery Service Not Integrated**
   - **Location**: `src/provider/discovery.rs` vs actual provider usage
   - **Issue**: `ModelDiscoveryService` exists but most providers return hardcoded model lists via `models()`
   - **Impact**: Model discovery isn't fully utilized
   - **Fix**: Wire `ModelDiscoveryService` into provider initialization

---

## Improvement Suggestions

### High Priority

1. **Fix SSE Parser Buffer Drain**
   - **File**: `src/provider/sse_parser.rs:48-50`
   - **Suggestion**: Restructure to avoid the dual drain pattern that causes misalignment
   - **Priority**: High - affects parsing correctness

2. **Fix Google Tool Call ID Extraction**
   - **File**: `src/provider/google.rs:353`
   - **Suggestion**: Extract `id` from `functionCall` response instead of generating UUID
   - **Priority**: High - breaks tool result correlation

3. **Add Retryable Error Pattern Matching**
   - **File**: `src/provider/fallback.rs:131-137`
   - **Suggestion**: Match on error message patterns instead of parsing u16
   - **Priority**: Medium-High - some rate limit errors silently fail

### Medium Priority

4. **Conditional Anthropic Beta Header**
   - **File**: `src/provider/anthropic.rs:178`
   - **Suggestion**: Only send beta header when beta features are needed or make configurable
   - **Priority**: Medium

5. **Fix Tool Arguments Serialization**
   - **Files**: `src/provider/openai.rs:178`, `src/provider/openai_compatible.rs:132`
   - **Suggestion**: Pass arguments as Value, not serialized string
   - **Priority**: Medium

6. **ProviderCache Clock Skew Safety**
   - **File**: `src/provider/cache.rs:47-52`
   - **Suggestion**: Use `saturating_sub` for timestamp comparison
   - **Priority**: Medium

### Low Priority

7. **Use AHasher for ProviderCache**
   - **File**: `src/provider/cache.rs:33-36`
   - **Suggestion**: Replace `DefaultHasher` with `AHasher`
   - **Priority**: Low

8. **Add Discovery Logging**
   - **File**: `src/provider/openai_compatible.rs:369-374`
   - **Suggestion**: Add warn-level log when model discovery fails
   - **Priority**: Low

9. **Document SSE Parser State Passing**
   - **File**: `src/provider/sse_parser.rs`
   - **Suggestion**: The `__TC__:`, `__OAI_STATE__:` string markers are fragile - add validation or consider alternative approach
   - **Priority**: Low

10. **Integration Test Coverage**
    - **Suggestion**: Add tests for SSE parsing edge cases, circuit breaker transitions, cache eviction
    - **Priority**: Low

---

## Documentation Accuracy Summary

The architecture document `architecture/provider.md` is **mostly accurate** with these corrections needed:

### Corrections Required

1. **Line 169**: `is_retryable()` actually includes `Auth(_)` - doc shows only RateLimit/Timeout/Stream/CircuitOpen
2. **Line 228**: `ProviderCache` field is `cache` not `store`
3. **Line 272**: `register_builtin()` shows SAP AI Core, Zenmux, Kilo, Vercel AI Gateway - but these require config and are registered via `register_builtin_with_config()` not `register_builtin()`
4. **Line 272**: Missing Codegg Go in the list (actually in `additional.rs` and registered via `register_builtin_with_config()`)
5. **Line 319**: `register_config_provider` returns `Box<dyn Provider>` not just the result

### Missing from Documentation

1. `ResponseFormat` enum with JsonObject/JsonSchema variants
2. `ModelVariant` struct and its fields
3. `ModelDiscoveryService` in `discovery.rs` - fully documented but not mentioned in arch doc
4. `ProviderCache` in `cache.rs` - mentioned but field name incorrect
5. `OpenAiCompatibleConfig` and `OpenAiCompatibleProvider` in `openai_compatible.rs`
6. `text_tool_parser.rs` module
7. `ping()` method on `Provider` trait (added per AGENTS.md notes)