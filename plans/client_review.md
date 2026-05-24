# Client Module Architecture Review

**Date**: 2026-05-24
**Reviewer**: Architecture Review Agent
**Module**: `client/`
**Files Reviewed**:
- `src/client/mod.rs` (4 lines)
- `src/client/attach.rs` (154 lines)
- `src/client/sdk.rs` (53 lines)

---

## Summary

The client module provides WebSocket connectivity for remote TUI connections. The implementation is largely correct and well-structured, but several discrepancies exist between the documentation and actual code.

**Overall Assessment**: Implementation is correct but documentation needs updates.

---

## Verified Items

### Correct Items

1. **Module exports** - `mod.rs` correctly re-exports `run_attach` as public API
2. **RemoteClient structure** - `sdk.rs` struct matches docs (`base_url`, `http` fields)
3. **Health check endpoint** - Uses `GET /health` with 10s timeout (sdk.rs:36-51)
4. **WebSocket connection** - 30-second timeout on connection (attach.rs:43)
5. **Authentication** - Bearer token in Authorization header (attach.rs:27-29)
6. **ClientError enum** - All 5 variants match (`Connection`, `Unreachable`, `Rpc`, `WebSocket`, `Auth`) defined in `src/error.rs:504-519`
7. **URL building** - Both `build_tui_ws_url()` and `build_http_url()` work as documented
8. **Two background tasks** - `event_task` and `send_task` properly implemented
9. **TuiMessage protocol** - All variants defined in `src/protocol/tui.rs` match documented table
10. **Connection retry logic** - 3 retries with exponential backoff (2s, 4s) implemented at attach.rs:35-66

---

## Discrepancies Found

### 1. RenderFrame Handling (Documentation Inaccurate)

**Location**: architecture/client.md line 85, SKILL.md line 144

**Issue**: Documentation states `RenderFrame` is "unused" / "not implemented" implying complete non-handling. However, the TUI actually handles it:

**Actual code** (`src/tui/app/mod.rs:755-759`):
```rust
Ok(RemoteTuiMessage::RenderFrame { content }) => {
    tracing::warn!(
        "RenderFrame received ({} bytes) but rendering not implemented",
        content.len()
    );
}
```

**Recommendation**: Update docs to clarify RenderFrame is "received and logged (not rendered)" rather than "unused."

### 2. new_remote Parameter Name Mismatch

**Location**: SKILL.md line 151, actual code at `src/tui/app/mod.rs:492`

**Issue**: Skill says `pub fn new_remote(project_dir: String)` but actual signature is `pub fn new_remote(project_dir: String)` - this is actually correct. The confusion is that `attach.rs:72` calls `tui::App::new_remote(url.to_string())` passing URL as project_dir.

**Analysis**: The `project_dir` parameter is stored but only used locally (never sent to server). The URL is passed because `new_remote()` doesn't actually use the parameter for remote mode - it just sets `remote_mode = true` and initializes channels. This is semantically confusing but not a bug.

**Recommendation**: Document that `project_dir` is accepted but ignored in remote mode, or consider renaming the parameter to clarify its purpose.

### 3. Missing Documentation: Retry Logic

**Location**: `src/client/attach.rs:35-66`

**Issue**: The architecture doc does not document the retry with exponential backoff (3 attempts, 2s/4s delay).

**Current behavior**:
- Up to 3 WebSocket connection attempts
- Exponential backoff: 2s, 4s between attempts
- 30s timeout per attempt
- Returns `ClientError::WebSocket` after all retries exhausted

**Recommendation**: Add retry logic to architecture documentation under "Connection Flow" or "Error Handling."

### 4. Missing Documentation: Panic Handling

**Location**: `src/client/attach.rs:80-112`

**Issue**: The event_task uses `catch_unwind` to handle panics, but this is not documented.

**Current behavior**:
```rust
let result = std::panic::catch_unwind(move || async { ... });
if let Err(panic_err) = result {
    tracing::error!("event_task panicked: {:?}", panic_err);
}
```

**Recommendation**: Add note about panic recovery in event handling.

---

## Code Issues

### Issue 1: Semantic Confusion in new_remote Parameter

**File**: `src/client/attach.rs:72`
**Line**: `let mut app = tui::App::new_remote(url.to_string());`

**Problem**: Passing `url` as `project_dir` is semantically confusing. The `new_remote()` function accepts a `project_dir` string but in remote mode this parameter is not meaningfully used.

**Impact**: Low - code works correctly, just confusing for maintenance.

**Recommendation**: Consider either:
1. Document that the parameter is ignored in remote mode
2. Rename parameter to `display_name` or similar to clarify purpose
3. Or change to `new_remote(url: String)` if that's the actual intent

### Issue 2: Health Check Error Message Inconsistency

**File**: `src/client/sdk.rs:43`
**Line**: `map_err(ClientError::Unreachable(e.to_string()))`

**Problem**: Other error paths in `health()` use `format!` with custom messages, but this one uses `e.to_string()`. Not a bug but inconsistent style.

**Recommendation**: Use consistent formatting: `ClientError::Unreachable(format!("connection failed: {}", e))`

---

## Documentation Improvements Needed

1. **Add retry/backoff documentation** to architecture/client.md
2. **Clarify RenderFrame handling** - "received and logged" not "unused"
3. **Add panic handling note** to architecture or skill doc
4. **Clarify new_remote parameter** - semantically confusing URL passing

---

## No Bugs Found (Verified Correct)

1. **ClientError enum correctly defined** in `src/error.rs:504-519`
2. **Health check uses /health endpoint** - confirmed correct (not /api/providers as noted in AGENTS.md review)
3. **WebSocket auth header** correctly set as `Bearer {token}`
4. **event_tx/out_tx channels** properly set up and passed to App
5. **Both tasks properly aborted** on event loop completion (lines 126-127)
6. **catch_unwind prevents event_task crashes** from propagating

---

## File References

| Issue | File | Lines |
|-------|------|-------|
| RenderFrame handling | `src/tui/app/mod.rs` | 755-759 |
| URL passed as project_dir | `src/client/attach.rs` | 72 |
| Retry logic | `src/client/attach.rs` | 35-66 |
| Panic catch | `src/client/attach.rs` | 81-112 |
| ClientError enum | `src/error.rs` | 504-519 |
| Health check | `src/client/sdk.rs` | 35-52 |

---

## Conclusion

The client module implementation is correct and functional. The main issues are:
1. Documentation inaccuracies (RenderFrame, retry logic)
2. Semantic confusion in parameter naming/usage
3. Missing documentation for panic handling

No critical bugs found. Recommended to update documentation to match actual behavior.
