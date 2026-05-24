# Error Module Architecture Review

**Date**: 2026-05-24  
**Reviewer**: File Search Specialist  
**Files Reviewed**:
- `architecture/error.md`
- `src/error.rs`
- `.opencode/skills/error/SKILL.md`
- `src/exec.rs` (classify_error function)

---

## Summary

The error module is well-implemented and mostly accurately documented. The centralized error handling approach using `thiserror` is consistent throughout the codebase. Most claims in the architecture document were verified against the actual implementation.

**Total Issues Found**: 2 (1 documentation discrepancy, 1 skill discrepancy)

---

## Discrepancies Between Docs and Code

### 1. ProviderError::is_retryable() - Documentation Missing Auth Variant

**Issue**: The architecture document at `architecture/error.md:106-115` shows `ProviderError::is_retryable()` but does NOT include `Auth(_)` in the retryable matches. The actual implementation at `src/error.rs:162-171` includes `ProviderError::Auth(_)` as retryable.

**Reference**:
- Doc: `architecture/error.md:106-115`
- Code: `src/error.rs:162-171`

**Code**:
```rust
pub fn is_retryable(&self) -> bool {
    matches!(
        self,
        ProviderError::RateLimit
            | ProviderError::Timeout(_)
            | ProviderError::Stream(_)
            | ProviderError::CircuitOpen(_)
            | ProviderError::Auth(_)  // <-- Present in code but not docs
    )
}
```

**Recommendation**: Update `architecture/error.md:106-115` to include `| ProviderError::Auth(_)` in the matches pattern.

---

### 2. Skill Document Lacks Auth in is_retryable

**Issue**: The skill document at `.opencode/skills/error/SKILL.md:40-50` also shows `ProviderError::is_retryable()` without `Auth(_)` variant.

**Reference**:
- Skill: `.opencode/skills/error/SKILL.md:40-50`

**Recommendation**: Update skill document to include `Auth(_)` in the retryable matches pattern.

---

## Verified Correct Items

The following items were verified as correctly documented and implemented:

### AppError Enum (src/error.rs:11-63)
- All 15 variants present and match documentation
- `#[from]` attributes properly configured for automatic conversions
- Error messages correctly formatted

### ProviderError Enum (src/error.rs:110-139)
- All 8 variants present: NotFound, Api, Stream, RateLimit, Auth, ModelNotFound, Timeout, CircuitOpen
- `api()` and `api_with_url()` helper constructors present
- `From<String>`, `From<&str>`, `From<reqwest::Error>`, `From<CircuitError>` conversions implemented
- `is_retryable()` includes RateLimit, Timeout, Stream, CircuitOpen, Auth

### ToolError Enum (src/error.rs:326-350)
- All 8 variants present: NotFound, Execution, Timeout, Permission, Format, Disabled, Io, Network
- `is_retryable()` correctly returns true for Io, Network, Timeout

### StorageError Enum (src/error.rs:84-102)
- All 6 variants present: Database, Migration, NotFound, LlmOperation, Import, Export
- `From<sqlx::Error>` conversion implemented

### PermissionError Enum (src/error.rs:362-368)
- Both variants present: Denied, Check

### AgentError Enum (src/error.rs:316-323)
- Both variants present: NotFound, Invalid

### McpError Enum (src/error.rs:371-389)
- All 6 variants present: Connection, Server, ToolCall, OAuth, Encryption, Timeout
- `is_retryable()` includes Connection, Server, ToolCall, OAuth, Timeout

### LspError Enum (src/error.rs:400-428)
- All 9 variants present with proper error message formats
- `is_retryable()` includes DownloadFailed, LaunchFailed, RequestFailed, RequestTimeout, Io

### PluginError Enum (src/error.rs:439-455)
- All 5 variants present: NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest
- LoadFailed and InstallFailed use `#[from]` for underlying error types

### ServerRuntimeError Enum (src/error.rs:457-473)
- All 5 variants present: Bind, Shutdown, WebSocket, Rpc, Auth
- `IntoResponse` implementation present for server feature

### ClientError Enum (src/error.rs:503-519)
- All 5 variants present: Connection, Unreachable, Rpc, WebSocket, Auth

### HTTP Status Mapping (src/error.rs:216-314)
- All status codes correctly mapped as documented
- Security measure verified: canonical reason strings used, no detail leakage

### Exec Mode Classification (src/exec.rs:189-259)
- All error variants properly classified with distinct error codes
- `classify_error()` function handles all AppError variants

---

## Key Conversions Verified

| Conversion | Location | Status |
|------------|----------|--------|
| `sqlx::Error` -> `StorageError::Database` | src/error.rs:104-108 | Verified |
| `reqwest::Error` -> `ProviderError::Api` | src/error.rs:194-203 | Verified |
| `CircuitError::Open` -> `ProviderError::CircuitOpen` | src/error.rs:205-213 | Verified |
| `String`/`&str` -> `ProviderError::Api` | src/error.rs:174-192 | Verified |

---

## Recommendations

### For Documentation
1. **Update `architecture/error.md`**: Add `ProviderError::Auth(_)` to the `is_retryable()` method documentation (line 113)

### For Skill Document
1. **Update `.opencode/skills/error/SKILL.md`**: Add `ProviderError::Auth(_)` to the `is_retryable()` method documentation (line 47)

### No Code Changes Required
No bugs or inconsistencies were found in the actual implementation. The code is correct.

---

## Conclusion

The error module is well-maintained with proper error handling patterns. The only discrepancies are minor documentation omissions where `Auth` was missing from the `is_retryable()` examples in both the architecture document and skill document. All actual code implementations are correct and consistent with the design intent.
