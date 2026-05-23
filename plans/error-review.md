# Error Module Architecture Review

**Date**: 2026-05-23
**Reviewed Files**: `architecture/error.md`, `src/error.rs`

---

## Verified Claims

### AppError Enum
- All 16 variants match implementation exactly (Config, Storage, Provider, Agent, Tool, Permission, Mcp, Plugin, Lsp, Io, Json, Http, Other, Worktree, Upgrade, Clipboard, Tui)
- `#[from]` derive attributes correctly enable error propagation
- Error message formats match exactly

### ProviderError
- All 8 variants match: NotFound, Api, Stream, RateLimit, Auth, ModelNotFound, Timeout, CircuitOpen
- `is_retryable()` correctly returns true for: RateLimit, Timeout, Stream, CircuitOpen, **and Auth** (documentation only listed 4 - see bugs)
- `api()` and `api_with_url()` helper constructors documented and present
- `From<reqwest::Error>` conversion creates Api variant with code "request_error"
- `From<CircuitError>` conversion maps `CircuitError::Open(name)` to `ProviderError::CircuitOpen(name)`
- `From<String>` and `From<&str>` create Api variant with code "unknown"

### ToolError
- All 8 variants match: NotFound, Execution, Timeout, Permission, Format, Disabled, Io, Network
- `is_retryable()` correctly returns true for: Io, Network, Timeout

### PermissionError
- Both variants match exactly: Denied { tool, path }, Check(String)

### Other Error Types (line 171-180)
- ConfigError: NotFound, Invalid, Parse, Merge, Watch ✓
- StorageError: Database, Migration, NotFound, LlmOperation, Import, Export ✓
- AgentError: NotFound, Invalid ✓
- McpError: Connection, Server, ToolCall, OAuth, Encryption, Timeout ✓
- LspError: ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json ✓
- PluginError: NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest ✓
- ClientError: Connection, Unreachable, Rpc, WebSocket, Auth ✓
- ServerRuntimeError: Bind, Shutdown, WebSocket, Rpc, Auth ✓

### Key Conversions Table (line 184-189)
- `sqlx::Error` -> `StorageError::Database` ✓
- `reqwest::Error` -> `ProviderError::Api` ✓
- `CircuitError::Open` -> `ProviderError::CircuitOpen` ✓
- `String` / `&str` -> `ProviderError::Api` ✓

### HTTP Status Mapping (lines 193-214)
- All mappings are correct except `ConfigError::Watch` (see bugs)
- Error body does not leak details (uses canonical_reason only) ✓

### McpError::is_retryable()
- Present at line 388-394, returns true for Connection, Server, ToolCall, OAuth, Timeout
- **Not documented in architecture doc** - see improvement suggestions

### LspError::is_retryable()
- Present at line 427-434, returns true for DownloadFailed, LaunchFailed, RequestFailed, RequestTimeout, Io
- **Not documented in architecture doc** - see improvement suggestions

---

## Bugs/Discrepancies Found

### BUG 1 (High Priority): ProviderError::is_retryable() missing Auth variant
**Documentation** (line 107-114):
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

**Implementation** (line 162-171):
```rust
pub fn is_retryable(&self) -> bool {
    matches!(
        self,
        ProviderError::RateLimit
            | ProviderError::Timeout(_)
            | ProviderError::Stream(_)
            | ProviderError::CircuitOpen(_)
            | ProviderError::Auth(_)
    )
}
```

The implementation includes `Auth(_)` but the documentation does not. This is an intentional behavior difference - authentication errors may be transient (token expiry, rate limiting on auth endpoints) and the implementation considers them retryable.

### BUG 2 (Medium Priority): ConfigError::Watch HTTP status incorrect
**Documentation** (line 198):
| ConfigError::Watch | 400 |

**Implementation** (line 223):
```rust
AppError::Config(ConfigError::Watch(_)) => StatusCode::INTERNAL_SERVER_ERROR,
```

The documentation says ConfigError::Watch should map to 400, but it actually maps to 500 (INTERNAL_SERVER_ERROR).

### BUG 3 (Medium Priority): Missing StorageError::Import/Export HTTP mapping
**Documentation** (line 200):
```
StorageError::Database/Migration/LlmOperation | 500
```

**Implementation** (line 226-230):
```rust
AppError::Storage(StorageError::Database(_))
| AppError::Storage(StorageError::Migration(_))
| AppError::Storage(StorageError::LlmOperation { .. }) => {
    StatusCode::INTERNAL_SERVER_ERROR
}
```

StorageError::Import and StorageError::Export are not mapped. They fall through to the catch-all at line 290 and get 500, which is probably correct behavior, but they should be explicitly documented. Currently they have no explicit HTTP status mapping documented.

### BUG 4 (Low Priority): ToolError::Permission HTTP mapping inconsistency
**Documentation** (line 207):
```
ToolError::Permission | 403
```

**Implementation** (line 245):
```rust
AppError::Tool(ToolError::Permission(_)) | AppError::Permission(PermissionError::Denied { .. }) => {
    StatusCode::FORBIDDEN
}
```

This is actually correct, but the documentation only shows `ToolError::Permission` and doesn't mention that `PermissionError::Denied` also maps to 403. The implementation groups them together which is fine but the documentation is incomplete.

---

## Improvement Suggestions

### HIGH Priority

1. **Add McpError::is_retryable() to documentation** (line ~115)
   - McpError has its own `is_retryable()` method (line 388-394)
   - Returns true for: Connection, Server, ToolCall, OAuth, Timeout
   - Should be documented alongside ProviderError::is_retryable() and ToolError::is_retryable()

2. **Add LspError::is_retryable() to documentation** (line ~115)
   - LspError has its own `is_retryable()` method (line 427-434)
   - Returns true for: DownloadFailed, LaunchFailed, RequestFailed, RequestTimeout, Io
   - Should be documented alongside other is_retryable() methods

3. **Document Auth as retryable in ProviderError::is_retryable()** (line 107-114)
   - The implementation treats `Auth(_)` as retryable
   - Update documentation to include Auth variant or add a note explaining why

### MEDIUM Priority

4. **Fix ConfigError::Watch HTTP status in documentation** (line 198)
   - Change from 400 to 500 to match implementation

5. **Add explicit StorageError::Import/Export to HTTP mapping table** (line 200)
   - Add row: `StorageError::Import/Export | 500`
   - Or note that they fall through to default 500

6. **Document ToolError::Permission grouping with PermissionError::Denied** (line 207)
   - Update table to show: `ToolError::Permission + PermissionError::Denied | 403`
   - Or clarify that they share the same 403 status

### LOW Priority

7. **Add ClientError IntoResponse documentation** (after line 180)
   - ClientError (line 500-516) does not have an IntoResponse impl
   - Document that it has no HTTP mapping (only used client-side)

8. **Add error module location note** (line 7)
   - Documentation says "Location: `src/error.rs`" which is correct
   - Could add a note that error types are all in a single file (not a module directory)

9. **Add cross-reference to resilience module** (line 218)
   - The "See Also" section mentions resilience but doesn't explain how
   - Could clarify that `CircuitError` conversion to `ProviderError::CircuitOpen` enables circuit breaker integration

10. **Add cross-reference to exec module** (line 219)
    - The "See Also" mentions exec but doesn't explain how
    - Could clarify that exec mode uses error classification for JSON output

---

## Summary

The architecture documentation is **largely accurate** with minor discrepancies. The main issues are:

1. **ProviderError::is_retryable()** - implementation includes `Auth(_)` but docs don't
2. **ConfigError::Watch** HTTP status - docs say 400, implementation returns 500
3. **Missing is_retryable() methods** - McpError and LspError have these but docs don't mention them
4. **Incomplete HTTP mapping** - StorageError::Import/Export not explicitly mapped

The error module itself is well-structured with comprehensive test coverage verifying HTTP status mapping behavior. All error conversions are correctly documented.