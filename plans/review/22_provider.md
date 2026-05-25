# Provider Architecture Review (2026-05-25)

## Verified Correct Items

### Provider Trait (mod.rs:60-73)
- `Provider` trait correctly documented with `id()`, `name()`, `clone_box()`, `stream()`, `models()`, `discover_models()`, and `ping()` methods
- `ping()` implementation matches: `self.models().await.map(|m| !m.is_empty())`

### ChatEvent Enum (mod.rs:140-154)
- All 6 variants correctly documented: `TextDelta`, `ReasoningDelta`, `ToolCall`, `ToolResult`, `Finish`, `Error`
- Field names match: `tool_call_id` and `content` on `ToolResult`, `stop_reason` and `usage` on `Finish`

### Message Enum (mod.rs:109-126)
- All 4 variants correctly documented with `Arc<String>` for content fields
- `ContentPart::Text` and `ContentPart::Image` correctly documented

### ToolCall, ToolDefinition, TokenUsage, ModelInfo (mod.rs:166-229)
- All struct definitions accurate with correct field names

### Arc<String> Usage Pattern (mod.rs:113,124,168-170)
- All content fields correctly use `Arc<String>` as documented

### HTTP Client Configuration (mod.rs:46-56)
- `create_http_client()` matches exactly: timeout, connect_timeout, pool settings, `.inspect_err()`, `.unwrap_or_default()`

### Buffer Size Limits (mod.rs:44)
- `MAX_BUFFER_SIZE` is correctly documented as `1024 * 1024`

### ProviderError (error.rs:111-172)
- All 8 variants documented correctly: `NotFound`, `Api`, `Stream`, `RateLimit`, `Auth`, `ModelNotFound`, `Timeout`, `CircuitOpen`
- `api()` and `api_with_url()` methods documented correctly (mod.rs:142-160)
- `is_retryable()` implementation matches: includes `RateLimit`, `Timeout`, `Stream`, `CircuitOpen`, `Auth`

### ProviderRegistry (mod.rs:231-260)
- Methods `register()`, `get()`, `list()` documented correctly

### catalog.rs - ModelCatalog (catalog.rs:5-10)
- `models: HashMap<String, ModelInfo>`, `last_fetch: Option<Instant>`, `cache_ttl: Duration` match exactly

### cache.rs - ProviderCache
- Documented field name `store` vs actual `cache` - minor naming inconsistency

### fallback.rs - FallbackProvider
- Fields correctly documented: `providers`, `status_codes`, `circuit_breakers`
- Default status codes `[429, 500, 502, 503, 504]` matches line 17

### SSE Parser (sse_parser.rs:16-24)
- All 8 fields documented correctly:
  - `buffer`, `delimiter`, `is_openai`, `pending_tool_calls`, `current_tool`, `args_buffer`, `openai_tool_states`

### register_builtin (mod.rs:262-309)
- All 15 environment variables documented correctly

### Additional Providers Table (provider.md:33-48)
- `create_codegg_go()` correctly shown as registered via `register_env_fallback_provider` with `CODEGG_GO_API_KEY` env var (mod.rs:508-515)

### Discovery Providers Table (provider.md:51-58)
- All 4 discovery providers listed correctly

### ResponseFormat Enum (mod.rs:156-164)
- `JsonObject` and `JsonSchema` variants correctly documented

---

## Incorrect/Stale Items

### 1. ModelCatalog `cache` field naming (catalog.rs)
- **Doc says**: `store: DashMap<String, CachedResponse>` (line 226)
- **Actual**: `cache: DashMap<CacheKey, CacheEntry>` (line 15-17)
- **Fix**: Change `store` to `cache` and `CachedResponse` to `CacheEntry` in documentation

### 2. SseParser undocumented struct (sse_parser.rs)
- **Missing**: `OpenAiToolState` struct (lines 9-14) is not documented
- Contains `id`, `name`, `args_buffer` fields
- **Fix**: Add `OpenAiToolState` struct documentation

### 3. additional.rs - `create_codegg_go()` takes only api_key (additional.rs:172-179)
- **Doc says** (line 48): registered via `register_env_fallback_provider` with factory `FnOnce(String)`
- **Actual**: Factory signature `FnOnce(String)` is correct (takes only api_key)
- **Correct**: No change needed - factory signature is accurate

### 4. `ResponseFormat` not documented
- **Doc does not mention**: `ResponseFormat` enum at mod.rs:156-164
- **Fix**: Add `ResponseFormat` to Core Traits section with `JsonObject` and `JsonSchema { name, schema, strict }` variants

### 5. `ModelVariant` not documented
- **Doc does not mention**: `ModelVariant` struct at mod.rs:209-217
- **Fix**: Add `ModelVariant` struct documentation with all 5 fields

---

## Minor Improvements

### 1. Provider implementations table (provider.md:20-29)
- Core provider file/description table is accurate - no changes needed

### 2. `register_builtin_with_config` documentation complete (mod.rs:373-520)
- All registration calls correctly documented

---

## Summary

The architecture document is **highly accurate** with only 5 minor issues:
1. `ProviderCache.store` should be `cache` with `CacheEntry` type
2. `OpenAiToolState` struct missing from SseParser section
3. `ResponseFormat` enum missing from Core Traits
4. `ModelVariant` struct missing from Core Traits

No bugs found in related provider code. The implementation matches the documentation well.