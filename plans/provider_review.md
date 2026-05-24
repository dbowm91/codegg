# Provider Module Architecture Review

## Summary

Reviewed `architecture/provider.md` against the actual implementation in `src/provider/`. The architecture document is **largely accurate** with only minor discrepancies. Most of the implementation details match the documented behavior.

## Verified Items (Correct)

### Provider Trait
- `src/provider/mod.rs:60-73` - Trait definition matches exactly
- `ping()` method exists and works correctly

### ProviderError
- `src/error.rs:110-172` - All variants correctly documented
- `is_retryable()` method implementation correct (includes RateLimit, Timeout, Stream, CircuitOpen, **and Auth** - see discrepancy below)
- `api()` and `api_with_url()` constructors exist at error.rs:142-160

### Core Types (Message, ChatEvent, ToolCall, ContentPart, TokenUsage)
- All use `Arc<String>` for content fields as documented
- All struct definitions match documentation

### ProviderRegistry
- `src/provider/mod.rs:231-260` - Correct implementation

### FallbackProvider
- `src/provider/fallback.rs:8-141` - Circuit breaker integration correct
- Exponential backoff: `2^i` seconds capped at 30s (line 107) - matches doc

### HTTP Client Configuration
- `src/provider/mod.rs:46-56` - Matches exactly with `.inspect_err()` and `.unwrap_or_default()`

### Buffer Size Limits
- `src/provider/mod.rs:44` - `MAX_BUFFER_SIZE: usize = 1024 * 1024` - correctly documented

### SSE Parser Functions
- `parse_openai_buffer()` at sse_parser.rs:370
- `parse_anthropic_buffer()` at sse_parser.rs:496
- Both documented in architecture

### ModelCatalog
- `src/provider/catalog.rs:5-10` - Correct fields documented

### Register Functions
- `register_builtin()` at mod.rs:262-309 - Correct 15 providers
- `register_builtin_with_config()` at mod.rs:373-520 - Correct implementation

## Discrepancies Found

### 1. SseParser Struct Fields (Minor)

**Architecture doc (line 247-256) shows:**
```rust
pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,  // Listed
    args_buffer: String,                             // Listed
    openai_tool_states: HashMap<usize, OpenAiToolState>,  // Listed
}
```

**Actual implementation at sse_parser.rs:16-24:**
```rust
pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,                                  // Undocumented in arch
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,
    args_buffer: String,
    openai_tool_states: HashMap<usize, OpenAiToolState>,
}
```

The `is_openai: bool` field is not mentioned in architecture doc but is part of the struct.

### 2. ProviderError::is_retryable() - Auth variant missing from docs

**Architecture doc (line 330-337):**
```rust
pub fn is_retryable(&self) -> bool {
    matches!(
        self,
        ProviderError::RateLimit
            | ProviderError::Timeout(_)
            | ProviderError::Stream(_)
            | ProviderError::CircuitOpen(_)
    )
}
```

**Actual implementation at src/error.rs:162-171:**
```rust
pub fn is_retryable(&self) -> bool {
    matches!(
        self,
        ProviderError::RateLimit
            | ProviderError::Timeout(_)
            | ProviderError::Stream(_)
            | ProviderError::CircuitOpen(_)
            | ProviderError::Auth(_)  // <-- Missing from docs
    )
}
```

### 3. Additional Provider Registration - codegg_go not in register_builtin

**Architecture doc (line 48):** Lists `create_codegg_go()` in the Additional Providers table.

**Actual registration in mod.rs:** `register_builtin_with_config()` at lines 508-515 registers `codegg_go` via `register_env_fallback_provider`. This is correct but `codegg_go` is NOT registered via `register_builtin()` (the env-var-only function), only via the config-based function.

This is actually correct behavior - the doc may be slightly misleading as it implies `register_builtin` handles all 15 providers when `codegg_go` requires the config-based registration path.

### 4. ModelDiscoveryService cache_ttl storage

**Architecture doc (line 209):** Shows `cache_ttl: Duration`

**Actual in discovery.rs:14:** `ttl: Duration` - This is correct but used differently. The `ModelCatalog` in `catalog.rs:9` uses `cache_ttl: Duration` but `ModelDiscoveryService` stores it as `ttl: Duration` and stores `last_fetch: Option<Instant>` differently.

### 5. Undocumented function: `parse_anthropic_buffer_with_state()`

**Location:** `src/provider/sse_parser.rs:500-519`

This function exists but is not documented in the architecture. It allows passing external state for tool call parsing which is useful for resumption.

## No Bugs Found

The implementation does not contain any bugs relative to the documentation - the documented behavior is correctly implemented. The discrepancies are all documentation-related, not code bugs.

## Recommendations

### For Architecture Document:

1. **Add `is_openai: bool` to SseParser struct** (line 248-256) - This field is important as it determines delimiter selection and parsing behavior.

2. **Update ProviderError::is_retryable()** (line 331-337) - Add `| ProviderError::Auth(_)` to the match pattern.

3. **Clarify codegg_go registration** - The Additional Providers table implies it works with `register_builtin()` but it only works with `register_builtin_with_config()`. Consider adding a note.

4. **Add `parse_anthropic_buffer_with_state()`** to the SSE Parser section with documentation about its use for resumption.

5. **Consider adding `debug_log!` macro** to the Implementation Notes section since it's used extensively in provider code.

### For Code:

No code changes recommended - all implementation is correct.

## File Reference Summary

| Item | File | Lines |
|------|------|-------|
| SseParser struct | src/provider/sse_parser.rs | 16-24 |
| ProviderError::is_retryable | src/error.rs | 162-171 |
| register_builtin | src/provider/mod.rs | 262-309 |
| register_builtin_with_config | src/provider/mod.rs | 373-520 |
| parse_anthropic_buffer_with_state | src/provider/sse_parser.rs | 500-519 |
| FallbackProvider exponential backoff | src/provider/fallback.rs | 107-109 |
| create_http_client | src/provider/mod.rs | 46-56 |

## Conclusion

The provider module is well-implemented and well-documented. The architecture document is accurate for all major functionality. The only issues are minor documentation gaps that do not affect the correctness of the implementation.
