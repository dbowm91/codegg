# Client Module Architecture Review

**Review Date**: 2026-05-27
**Reviewed Files**: `architecture/client.md`, `src/client/` (mod.rs, attach.rs, sdk.rs), `src/protocol/tui.rs`

---

## Verified Claims

### Module Structure ✓
- `mod.rs` correctly re-exports `run_attach` as public API
- `attach.rs` contains main WebSocket connection logic
- `sdk.rs` contains HTTP client for health checks

### Core Functionality ✓
- `run_attach(url, token)` async function signature matches
- `build_tui_ws_url()` and `build_http_url()` functions exist and work as documented
- `RemoteClient` struct has `base_url: String` and `http: Client` fields
- `RemoteClient::new()` and `health()` methods exist with correct signatures
- Health check uses `GET /health` with 10s timeout
- WebSocket connection uses 30s timeout

### TuiMessage Protocol ✓
- All documented variants exist in `src/protocol/tui.rs`
- Fields match exactly (Input, KeyDown, MouseClick, Resize, PermissionResponse, QuestionResponse, TextDelta, PermissionPending, QuestionPending, SessionInfo, SessionEnded, ToolCallStarted, ToolResult, Error, ResyncRequired)
- `RenderFrame` exists but is not used by client (documented correctly)
- `QuestionSpec` struct exists with `id`, `prompt`, `default` fields

### Error Handling ✓
- `ClientError` enum has all 5 variants with correct variants:
  - `Connection(String)`
  - `Unreachable(String)`
  - `Rpc(String)`
  - `WebSocket(String)`
  - `Auth(String)`

### TUI Integration ✓
- `App::new_remote()` exists at correct location
- `set_remote_event_rx()` and `set_remote_send_tx()` methods exist
- `handle_remote_event()` processes all documented TuiMessage variants

---

## Bugs/Discrepancies Found

### 1. WebSocket Retry Logic Not Documented (HIGH)
**Location**: `src/client/attach.rs:35-66`
**Issue**: The documentation says "WebSocket Connection - 30-second timeout" implying a single attempt, but the actual implementation retries up to 3 times with exponential backoff (2s, 4s delays).
**Impact**: Users reading the docs would not know connection failures are retried automatically.
**Fix**: Document the retry behavior in the Connection Flow section.

### 2. Task Count Inconsistency (MEDIUM)
**Location**: `architecture/client.md:47` vs `.opencode/skills/client/SKILL.md:57`
**Issue**: Architecture doc says "Two Background Tasks" but skill says "Three Concurrent Tasks". Actual implementation has TWO tasks (event_task and send_task).
**Fix**: Skill should say "Two" not "Three".

### 3. Line Counts Outdated (LOW)
**Issue**: Documentation says `attach.rs: 118 lines` and `sdk.rs: 44 lines`, but actual are 154 and 53 respectively.
**Fix**: Update line counts in skill documentation.

### 4. Missing Bearer Token Documentation (MEDIUM)
**Location**: `src/client/attach.rs:27-29`
**Issue**: Authorization header is set as `Bearer {token}` but neither architecture doc nor skill documents this authentication mechanism.
**Fix**: Add note about Bearer token authentication in WebSocket connection section.

### 5. Error Propagation Not Documented (LOW)
**Location**: `src/client/attach.rs:129`
**Issue**: The function returns `ClientError::Connection` wrapping TUI errors, but this is not documented.
**Fix**: Document that TUI event loop errors are converted to `ClientError::Connection`.

### 6. Cleanup Behavior Not Documented (LOW)
**Location**: `src/client/attach.rs:126-127`
**Issue**: Both tasks are aborted when `run_event_loop()` returns, but this graceful cleanup is not documented.
**Fix**: Add cleanup step to Connection Flow documentation.

---

## Improvement Suggestions

### HIGH Priority

1. **Document WebSocket Retry Logic**
   - Add retry behavior to architecture/client.md Connection Flow section
   - Specify max attempts (3) and backoff strategy (2^i seconds, capped at 4s)
   - Update skill to reflect actual implementation

2. **Fix Task Count Inconsistency**
   - Update `.opencode/skills/client/SKILL.md` line 57 to say "Two" instead of "Three"

### MEDIUM Priority

3. **Document Bearer Token Authentication**
   - Add authentication mechanism to both architecture doc and skill
   - Note that token is passed via `Authorization: Bearer {token}` header

4. **Update Line Counts**
   - Fix `attach.rs` line count from 118 to 154 in skill
   - Fix `sdk.rs` line count from 44 to 53 in skill

### LOW Priority

5. **Document Error Propagation**
   - Add note in Error Handling section that `run_attach()` wraps TUI errors in `ClientError::Connection`

6. **Document Task Cleanup**
   - Add cleanup step showing both tasks are aborted after event loop returns

---

## Summary

The architecture document and skill are mostly accurate, but there is one significant omission: the WebSocket connection retry logic that retries 3 times with exponential backoff is not documented. This is important behavior that users relying on the docs would be unaware of. The task count inconsistency should also be fixed for clarity.