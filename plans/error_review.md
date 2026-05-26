# Error Architecture Review

Date: 2026-05-26
Reviewer: Claude Code
Source: `src/error.rs` (639 lines)

---

## 1. Location

| Claim | Status | Actual |
|-------|--------|--------|
| Location: `src/error.rs` | ✓ | Confirmed |

---

## 2. AppError Enum

**Lines 11-63 in source.** Document claims lines 17-71.

| Variant | Doc | Source | Match |
|---------|-----|--------|-------|
| Config | ✓ | ✓ | Yes |
| Storage | ✓ | ✓ | Yes |
| Provider | ✓ | ✓ | Yes |
| Agent | ✓ | ✓ | Yes |
| Tool | ✓ | ✓ | Yes |
| Permission | ✓ | ✓ | Yes |
| Mcp | ✓ | ✓ | Yes |
| Plugin | ✓ | ✓ | Yes |
| Lsp | ✓ | ✓ | Yes |
| Io | ✓ | ✓ | Yes |
| Json | ✓ | ✓ | Yes |
| Http | ✓ | ✓ | Yes |
| Other | ✓ | ✓ | Yes |
| Worktree | ✓ | ✓ | Yes |
| Upgrade | ✓ | ✓ | Yes |
| Clipboard | ✓ | ✓ | Yes |
| Tui | ✓ | ✓ | Yes |

**Total: 17 variants.** Document does not state count, but structure matches exactly.

---

## 3. ProviderError

**Lines 110-139 in source.** Document claims lines 77-103.

| Variant | Doc | Source | Match |
|---------|-----|--------|-------|
| NotFound | ✓ | ✓ | Yes |
| Api | ✓ | ✓ | Yes |
| Stream | ✓ | ✓ | Yes |
| RateLimit | ✓ | ✓ | Yes |
| Auth | ✓ | ✓ | Yes |
| ModelNotFound | ✓ | ✓ | Yes |
| Timeout | ✓ | ✓ | Yes |
| CircuitOpen | ✓ | ✓ | Yes |

**Total: 8 variants.** Structure matches exactly.

### is_retryable()

**Document (lines 106-115):**
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

**Source (lines 162-171):** Matches exactly.

---

## 4. ToolError

**Lines 325-350 in source.** Document claims lines 121-157.

| Variant | Doc | Source | Match |
|---------|-----|--------|-------|
| NotFound | ✓ | ✓ | Yes |
| Execution | ✓ | ✓ | Yes |
| Timeout | ✓ | ✓ | Yes |
| Permission | ✓ | ✓ | Yes |
| Format | ✓ | ✓ | Yes |
| Disabled | ✓ | ✓ | Yes |
| Io | ✓ | ✓ | Yes |
| Network | ✓ | ✓ | Yes |

**Total: 8 variants.** Structure matches exactly.

### is_retryable()

**Document (lines 149-156):**
```rust
pub fn is_retryable(&self) -> bool {
    matches!(
        self,
        ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
    )
}
```

**Source (lines 352-358):** Matches exactly.

---

## 5. PermissionError

**Lines 361-368 in source.** Document claims lines 161-169.

| Variant | Doc | Source | Match |
|---------|-----|--------|-------|
| Denied { tool, path } | ✓ | ✓ | Yes |
| Check | ✓ | ✓ | Yes |

**Total: 2 variants.** Matches exactly.

---

## 6. Other Error Types

All document "bullet lists" at lines 174-199:

| Type | Doc Variants | Source Variants | Match |
|------|--------------|-----------------|-------|
| ConfigError | NotFound, Invalid, Parse, Merge, Watch (5) | Lines 65-81: Same 5 | ✓ |
| StorageError | Database, Migration, NotFound, LlmOperation, Import, Export (6) | Lines 83-102: Same 6 | ✓ |
| AgentError | NotFound, Invalid (2) | Lines 316-323: Same 2 | ✓ |
| McpError | Connection, Server, ToolCall, OAuth, Encryption, Timeout (6) | Lines 370-389: Same 6 | ✓ |
| LspError | ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json (9) | Lines 400-428: Same 9 | ✓ |
| PluginError | NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest (5) | Lines 439-455: Same 5 | ✓ |
| ClientError | Connection, Unreachable, Rpc, WebSocket, Auth (5) | Lines 503-519: Same 5 | ✓ |
| ServerRuntimeError | Bind, Shutdown, WebSocket, Rpc, Auth (5) | Lines 457-473: Same 5 | ✓ |

---

## 7. McpError::is_retryable()

**Document (lines 179-185):**
```rust
impl McpError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            McpError::Connection(_) | McpError::Server(_) | McpError::ToolCall(_) | McpError::OAuth(_) | McpError::Timeout(_)
        )
    }
}
```

**Source (lines 391-397):** Matches exactly.

---

## 8. LspError::is_retryable()

**Document (lines 189-196):**
```rust
impl LspError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LspError::DownloadFailed(_) | LspError::LaunchFailed(_) | LspError::RequestFailed(_) | LspError::RequestTimeout(_) | LspError::Io(_)
        )
    }
}
```

**Source (lines 430-436):** Matches exactly.

---

## 9. Key Conversions Table

| From | To | Notes | Status |
|------|-----|-------|--------|
| `sqlx::Error` | `StorageError::Database` | Database errors | ✓ (source lines 104-108) |
| `reqwest::Error` | `ProviderError::Api` | HTTP failures | ✓ (source lines 194-203) |
| `CircuitError::Open` | `ProviderError::CircuitOpen` | Circuit breaker | ✓ (source lines 205-213) |
| `String` / `&str` | `ProviderError::Api` | `api()` / `api_with_url()` | ✓ (source lines 142-192) |

---

## 10. HTTP Status Mapping

### Document claims at lines 210-237:

| Error | Doc Status | Source Status | Match |
|-------|-----------|--------------|-------|
| ConfigError::NotFound | 404 | 404 (line 219) | ✓ |
| ConfigError::Watch | 500 | 500 (line 223) | ✓ |
| StorageError::Import | 500 | 500 (line 229) | ✓ |
| StorageError::Export | 500 | same | ✓ |
| StorageError::NotFound | 404 | 404 (line 225) | ✓ |
| StorageError::Database/Migration/LlmOperation | 500 | 500 (lines 226-232) | ✓ |
| ProviderError::Auth | 401 | 401 (line 234) | ✓ |
| ProviderError::RateLimit | 429 | 429 (line 235) | ✓ |
| ProviderError::Timeout | 504 | 504 (line 236) | ✓ |
| ProviderError::NotFound/ModelNotFound | 404 | 404 (lines 237-238) | ✓ |
| ProviderError::Api/Stream/CircuitOpen | 502 | 502 (lines 239-241) | ✓ |
| ToolError::NotFound | 404 | 404 (line 246) | ✓ |
| ToolError::Permission | 403 | 403 (line 247) | ✓ |
| ToolError::Timeout | 504 | 504 (line 250) | ✓ |
| ToolError::Disabled | 403 | 403 (line 251) | ✓ |
| ToolError::Execution/Format/Io/Network | 502 | 502 (lines 252-255) | ✓ |
| McpError::OAuth | 401 | 401 (line 259) | ✓ |
| McpError::Timeout | 504 | 504 (line 260) | ✓ |
| McpError::Connection/Server/ToolCall/Encryption | 502 | 502 (lines 261-264) | ✓ |
| PluginError::NotFound | 404 | 404 (line 266) | ✓ |
| PluginError::InvalidManifest | 400 | 400 (line 267) | ✓ |
| PluginError::LoadFailed/HookFailed/InstallFailed | 500 | 500 (lines 268-270) | ✓ |

**All 22 HTTP status mappings verified correct.**

---

## 11. Additional Findings

### Additional From Implementations Not Documented

The source contains `From<reqwest::Error>` for `ProviderError` (lines 194-203), which the document mentions in the table but does not show as a full From impl block.

### ProviderError constructors

Source includes `api()` (lines 142-148) and `api_with_url()` (lines 150-160) constructors not explicitly documented in the Key Conversions table (though `String`/`&str` → `ProviderError::Api` is listed).

### Server/Client Errors Have IntoResponse

- `ServerRuntimeError` (lines 457-473) has `IntoResponse` impl (lines 475-501) - **NOT documented**
- `ClientError` (lines 503-519) exists but lacks `IntoResponse` - **NOT documented**

### Tests exist but not in documentation

Source includes comprehensive tests (lines 521-639) for HTTP status mapping.

---

## Summary

| Category | Claims | Verified | Discrepancies |
|----------|--------|----------|---------------|
| Location | 1 | 1 | None |
| AppError variants | 17 | 17 | None |
| ProviderError variants + is_retryable | 8 + 1 | 8 + 1 | None |
| ToolError variants + is_retryable | 8 + 1 | 8 + 1 | None |
| PermissionError variants | 2 | 2 | None |
| Sub-error variant lists | 8 types | 8 types | None |
| McpError::is辜tryable | 1 | 1 | None |
| LspError::is_retryable | 1 | 1 | None |
| Key Conversions | 4 | 4 | None |
| HTTP Status Mappings | 22 | 22 | None |

**Line numbers in doc are INCORRECT** (doc claims ~line 17-71 for AppError, but actual is 11-63).

**Not documented:**
1. `ServerRuntimeError` and its `IntoResponse` impl
2. `ClientError` exists (but no `IntoResponse`) — documented only in "Other Error Types" list as variants, but inner structure not shown
3. `ProviderError::api()` and `api_with_url()` constructors
4. `From<reqwest::Error> for ProviderError` full impl block in Key Conversions (mentioned but not shown)
5. Comprehensive test suite (lines 521-639)

**Overall: Documentation is accurate for what it covers.** No factual errors found.
