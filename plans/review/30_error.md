# Error Architecture Review (2026-05-25)

## Verified Correct Items

1. **AppError enum structure** (lines 17-70): All 16 variants match `src/error.rs:12-63`
2. **ConfigError** (line 66-81): 5 variants match
3. **StorageError** (lines 83-102): 6 variants including `Import`/`Export` match
4. **ProviderError** (lines 111-139): All 8 variants match - NotFound, Api, Stream, RateLimit, Auth, ModelNotFound, Timeout, CircuitOpen
5. **ProviderError::is_retryable()** (lines 162-172): Correctly returns true for RateLimit, Timeout, Stream, CircuitOpen, Auth
6. **ProviderError::api() and api_with_url()** (lines 142-160): Helper constructors documented
7. **String/&str From implementations** (lines 174-192): Present in code, documented in Key Conversions
8. **CircuitError -> ProviderError::CircuitOpen conversion** (lines 205-213): Present in code
9. **ToolError enum** (lines 326-350): All 7 variants match
10. **ToolError::is_retryable()** (lines 352-359): Correctly returns true for Io, Network, Timeout
11. **PermissionError** (lines 361-368): 2 variants match
12. **McpError** (lines 370-389): 6 variants match
13. **McpError::is_retryable()** (lines 391-398): Correctly returns true for Connection, Server, ToolCall, OAuth, Timeout
14. **LspError** (lines 400-428): 8 variants match (includes Io/Json as #[from])
15. **LspError::is_retryable()** (lines 430-437): Correctly returns true for DownloadFailed, LaunchFailed, RequestFailed, RequestTimeout, Io
16. **PluginError** (lines 439-455): 5 variants match with LoadFailed/InstallFailed as #[from]
17. **ClientError** (lines 503-519): 5 variants match
18. **ServerRuntimeError** (lines 457-473): 5 variants match
19. **HTTP Status Mapping table** (lines 210-237): All entries match `IntoResponse` impl (lines 216-313)
20. **Key Conversions table** (lines 201-208): All conversions present

## Items That Need Updates

### 1. Location path (architecture/error.md:7)
**Current**: `src/error.rs`
**Correct**: `src/error.rs` (the arch doc uses backticks which is fine, but the location note is somewhat redundant since it just says "Location")

### 2. Missing api_with_url() documented (architecture/error.md:207-208)
**Issue**: Key Conversions table lists helper constructors but doesn't document `api_with_url()`
**Fix**: Add `ProviderError::api_with_url(code, message, url)` to the table at lines 207-208

### 3. Missing reqwest -> ProviderError conversion details
**Issue**: Table at line 206 says `reqwest::Error -> ProviderError::Api` but doesn't mention url extraction
**Fix**: Consider adding a note that URL is extracted from reqwest error

## No Bugs Found

The error module implementation is correct and matches the architecture documentation. All `is_retryable()` methods are properly implemented, error conversions are wired correctly, and HTTP status mapping is accurate.