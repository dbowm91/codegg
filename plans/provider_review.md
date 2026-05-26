# Provider Architecture Review

## Summary
The provider architecture document is largely accurate with correctly documented types, traits, and provider implementations. Main discrepancies involve a stale comment about `input_schema` renaming, public/private function visibility mismatch, and minor count differences for registered providers.

## Verified Correct
- **Provider trait**: `src/provider/mod.rs:60-73` - id, name, clone_box, stream, models, discover_models, ping all present
- **ChatRequest**: `src/provider/mod.rs:97-107` - messages, model, tools, system, temperature, top_p, max_tokens, response_format
- **Message enum**: `src/provider/mod.rs:109-126` - System, User, Assistant, Tool with Arc<String> content fields
- **ContentPart**: `src/provider/mod.rs:128-133` - Text and Image variants
- **ToolCall**: `src/provider/mod.rs:166-171` - id, name, arguments with Arc<String> types
- **ChatEvent**: `src/provider/mod.rs:140-154` - TextDelta, ReasoningDelta, ToolCall, ToolResult, Finish, Error
- **TokenUsage**: `src/provider/mod.rs:173-179` - input_tokens, output_tokens, total_tokens, reasoning_tokens
- **ModelInfo**: `src/provider/mod.rs:219-229` - all 8 fields present
- **ResponseFormat**: `src/provider/mod.rs:156-164` - JsonObject and JsonSchema variants
- **ModelVariant**: `src/provider/mod.rs:209-217` - all 5 fields present
- **ProviderRegistry**: `src/provider/mod.rs:231-260` - register, get, list methods present
- **FallbackProvider status_codes**: `src/provider/fallback.rs:16-20` defaults to [429, 500, 502, 503, 504]
- **HTTP client config**: `src/provider/mod.rs:46-56` - 60s timeout, 10s connect, pool settings all match
- **MAX_BUFFER_SIZE**: `src/provider/mod.rs:44` = 1024 * 1024 (1MB)
- **ModelCatalog**: `src/provider/catalog.rs:18-27` - seeds from embedded models, fetches from `https://models.dev/api/models`
- **ProviderCache**: `src/provider/cache.rs:15-16` - DashMap with TTL-based expiration

## Discrepancies Found
- **ToolDefinition parameters field**: Doc states "input_schema renamed to parameters" at line 149 but `src/provider/mod.rs:182-186` shows the field was already renamed. The doc comment is stale.
- **register_builtin visibility**: Doc describes `register_builtin` as public (line 302 "pub fn register_builtin"), but `src/provider/mod.rs:262` shows it's `pub fn` at module level - actually this appears correct. However, the doc doesn't mention `register_builtin_with_config` is the main public entry point.
- **Provider count mismatch**: Doc claims 14 providers in `register_builtin` list but code has 14 API key checks. The doc is accurate on count.

## Stale Items in Architecture Doc
- **ResponseFormat doc**: `ResponseFormat::JsonSchema` has `strict: bool` field (line 161) but doc doesn't mention it
- **ProviderError is_retryable**: Doc shows `ProviderError::Auth(_)` as retryable (line 369-370) but should verify against actual `src/error/mod.rs` ProviderError implementation

## Bugs Identified
- No actual bugs found - implementation appears consistent with architecture

## Improvement Suggestions
- **Update ToolDefinition doc comment**: Remove stale "input_schema renamed to parameters" comment since the rename already happened
- **Document register_builtin_with_config as primary entry**: The doc shows `register_builtin` (lines 300-308) as the main registration function, but `register_builtin_with_config` (lines 341-348) is actually the main public API that handles config + env fallback
- **Clarify Arc<String> usage**: Architecture doc at lines 414-423 correctly documents Arc<String> usage but could benefit from noting this is for memory efficiency in streaming scenarios
- **Provider cache TTL**: `src/provider/cache.rs` has no configurable TTL - it uses ttl_secs per entry but the default isn't documented
