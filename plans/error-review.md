# Error Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| **Location**: `src/error.rs` | VERIFIED | Actual file is `src/error.rs` (note: not `src/error/mod.rs`) |
| **AppError enum variants** (lines 19-70) | VERIFIED | All 15 variants present: Config, Storage, Provider, Agent, Tool, Permission, Mcp, Plugin, Lsp, Io, Json, Http, Other, Worktree, Upgrade, Clipboard, Tui |
| **ProviderError::is_retryable()** (lines 106-115) | VERIFIED | Method exists with correct retryable variants: RateLimit, Timeout, Stream, CircuitOpen |
| **ToolError::is_retryable()** (lines 148-156) | VERIFIED | Method exists with correct variants: Io, Network, Timeout |
| **PermissionError** (lines 160-169) | VERIFIED | Denied { tool, path } and Check variants present |
| **ConfigError variants** (line 173) | VERIFIED | NotFound, Invalid, Parse, Merge, Watch all present (lines 66-81) |
| **StorageError variants** (line 174) | VERIFIED | Database, Migration, NotFound, LlmOperation, Import, Export all present (lines 83-102) |
| **AgentError variants** (line 175) | VERIFIED | NotFound, Invalid present (lines 312-319) |
| **McpError variants** (line 176) | VERIFIED | Connection, Server, ToolCall, OAuth, Encryption, Timeout present (lines 366-385) |
| **LspError variants** (line 177) | INCORRECT | Documented 10 variants, actual has 10 but RequestTimeout missing from doc. Actual: ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json. RequestTimeout IS in actual code at line 404-405. |
| **PluginError variants** (line 178) | VERIFIED | NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest present (lines 417-433) |
| **ClientError** (line 179) | VERIFIED | Connection, Unreachable, Rpc, WebSocket, Auth present (lines 481-497) |
| **ServerRuntimeError** (line 180) | VERIFIED | Bind, Shutdown, WebSocket, Rpc, Auth present (lines 435-451) |
| **sqlx::Error -> StorageError::Database** (line 186) | VERIFIED | Lines 104-108 implement From<sqlx::Error> |
| **reqwest::Error -> ProviderError::Api** (line 187) | VERIFIED | Lines 193-202 implement From<reqwest::Error> |
| **CircuitError::Open -> ProviderError::CircuitOpen** (line 188) | VERIFIED | Lines 204-212 implement From<CircuitError> |
| **String/&str -> ProviderError::Api** (line 189) | VERIFIED | Lines 173-191 implement From<String> and From<&str> |
| **HTTP Status Mapping table** (lines 195-215) | VERIFIED | All mappings match actual `IntoResponse` impl (lines 215-309) |
| **ConfigError::NotFound -> 404** | VERIFIED | Line 218 |
| **ConfigError::Invalid/Parse/Merge -> 400** | VERIFIED | Lines 219-221 |
| **StorageError::NotFound -> 404** | VERIFIED | Line 224 |
| **StorageError::Database/Migration/LlmOperation -> 500** | VERIFIED | Lines 225-229 |
| **ProviderError::Auth -> 401** | VERIFIED | Line 231 |
| **ProviderError::RateLimit -> 429** | VERIFIED | Line 232 |
| **ProviderError::Timeout -> 504** | VERIFIED | Line 233 |
| **ProviderError::NotFound/ModelNotFound -> 404** | VERIFIED | Lines 234-235 |
| **ProviderError::Api/Stream/CircuitOpen -> 502** | VERIFIED | Lines 236-238 |
| **ToolError::NotFound -> 404** | VERIFIED | Line 243 |
| **ToolError::Permission -> 403** | VERIFIED | Lines 244-246 |
| **ToolError::Timeout -> 504** | VERIFIED | Line 247 |
| **McpError::OAuth -> 401** | VERIFIED | Line 256 |
| **McpError::Timeout -> 504** | VERIFIED | Line 257 |
| **McpError::Connection/Server/ToolCall/Encryption -> 502** | VERIFIED | Lines 258-261 |
| **PluginError::NotFound -> 404** | VERIFIED | Line 263 |
| **PluginError::InvalidManifest -> 400** | VERIFIED | Line 264 |
| **PluginError::LoadFailed/HookFailed/InstallFailed -> 500** | VERIFIED | Lines 265-267 |

## Bugs Found

### Critical

1. **None identified** - Core error handling is sound.

### High

1. **ProviderError::Auth is not retryable but has valid use case for retry** (error.rs:162-170)
   - `is_retryable()` returns false for Auth errors, but transient auth failures (expired token, network glitch) should be retryable.
   - Currently only `RateLimit`, `Timeout`, `Stream`, and `CircuitOpen` are retryable.
   - **Fix**: Consider adding `ProviderError::Auth` to the retryable list, or add a separate method `is_retryable_with_backoff()`.

### Medium

1. **StorageError::Import and Export missing HTTP status mapping** (error.rs:224-229)
   - `Import` and `Export` variants are not explicitly mapped in `IntoResponse`, falling through to the catch-all 500.
   - This is technically correct but inconsistent with how other variants are explicitly listed.
   - Not a bug, just undocumented behavior.

2. **ToolError::Disabled returns 403** (error.rs:248)
   - `ToolError::Disabled(_)` maps to `StatusCode::FORBIDDEN` which semantically means "you cannot access this" rather than "this is turned off."
   - A more appropriate status might be 503 Service Unavailable or 404 Not Found depending on context.

3. **LspError::RequestTimeout missing from HTTP status table** (architecture/error.md:177)
   - The architecture doc lists LspError variants but omits `RequestTimeout`.
   - Actual code maps it to 502 BAD_GATEWAY (line 274) same as `RequestFailed`.

## Improvement Suggestions

### Performance

1. **Consider caching HTTP status codes** for frequently used errors.
   - Currently, `IntoResponse` pattern matches on every call, which is fine for error paths but could be optimized if errors are constructed frequently.
   - However, this is likely premature optimization.

### Correctness

1. **Add `is_retryable()` for `McpError`** - MCP connection errors could be transient and benefit from retry logic.

2. **Add `is_retryable()` for `LspError`** - Download failures, request timeouts, and launch failures are all potentially retryable scenarios.

3. **Consider `is_retryable()` for `StorageError`** - Database transient failures could be retryable.

4. **Add explicit HTTP mapping for `ToolError::Disabled`** - Consider 503 instead of 403 to better reflect that the tool exists but is disabled.

### Maintainability

1. **Document the `is_retryable()` convention** - Not all error types have `is_retryable()` methods. Consider establishing a pattern or trait for retryable errors.

2. **Add test coverage for `ServerRuntimeError::IntoResponse`** - Already covered in tests (lines 595-616), but expand to cover all variants.

3. **Add tests for error conversion implementations** - `From<CircuitError> for ProviderError`, `From<String> for ProviderError`, etc. are not explicitly tested.

4. **Consider extracting HTTP mapping to a helper function** - The `IntoResponse` impl is 95 lines and could be broken down for readability.

5. **Add inline documentation for `api_with_url()`** - Line 150-160 creates Api variant with URL, but this helper is undocumented in the architecture.

## Priority Actions (top 5 items to fix)

1. **Update architecture/error.md** - Add missing `RequestTimeout` to LspError variants list.

2. **Consider adding `Auth` to retryable errors** - This is a design decision that requires careful consideration of retry behavior for auth failures.

3. **Add `is_retryable()` to `McpError`** - Aligns with the pattern established by `ProviderError` and `ToolError`.

4. **Add `is_retryable()` to `LspError`** - Would provide consistent retry behavior across error types.

5. **Clarify `ToolError::Disabled` HTTP mapping** - Consider using 503 Service Unavailable instead of 403 to better reflect semantics.

## Notes

- The error module is well-structured with consistent use of `thiserror` for derive.
- The `From` trait implementations provide ergonomic error conversions.
- The `IntoResponse` implementation correctly avoids leaking sensitive details in error responses (verified by test at line 585-592).
- Test coverage is present for HTTP status mapping, but could be expanded for error conversions.