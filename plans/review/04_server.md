# Server Module Architecture Review (2026-05-25)

## Verified Correct Items

- **mod.rs exports** (lines 28-33): `run_server`, `discover_services`, `MdnsService`, `ServerState` all correctly exported
- **http.rs middleware stack** (lines 44-50): CORS, auth, rate limit (100 req/60s), compression (gzip/br), security headers, trace logging - all accurate
- **ServerState struct** (lines 54-61): Fields match exactly - `project_dir`, `pool`, `mcp_service`, `config`, `ws_rate_limiter`
- **WsRateLimiter struct** (lines 63-67): Fields match - `cache`, `max_requests`, `window`
- **WebSocket endpoints** (lines 72-95): `/ws` (JSON-RPC) and `/tui` (TuiMessage protocol) - both correct with accurate method list
- **validate_ws_auth()** (line 95): Correctly shared between `/ws` and `/tui` handlers
- **All REST routes** (lines 97-166): Sessions, config, MCP, event, permission, question, provider/tool, file, project, workspace - all routes match actual implementation
- **middleware/auth.rs token validation** (lines 168-178): Env vars `CODEGG_SERVER_AUTH_DISABLED`, `CODEGG_SERVER_TOKEN`, config fallback - accurate
- **TUI Replay Buffer** (lines 193-195): 1024 event buffer, monotonic sequence, Resume sends ResyncRequired after replay - correct
- **TuiMessage protocol tables** (lines 209-235): All variants and fields match `src/protocol/tui.rs`
- **ServerRuntimeError enum** (lines 256-263): All 5 variants (Bind, Shutdown, WebSocket, Rpc, Auth) - correct
- **Event SSE subscription** (line 191): `/api/event` subscribes to `GlobalEventBus::subscribe()` directly - correct

## Incorrect/Stale Items

1. **Lines 52-68 - FromRef implementations missing from doc**
   - `state.rs` has `FromRef` implementations for `SqlitePool`, `Arc<RwLock<McpService>>`, and `Config` (lines 53-69)
   - These enable router state extraction and should be documented

2. **Lines 63-67 - WsRateLimiter check_rate_limit signature differs**
   - Doc implies returns `(bool, usize)` (allowed, remaining)
   - Actual returns just `bool` - `state.rs:37-50`

3. **Lines 127 - Rate limit response headers not documented**
   - Middleware adds `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` response headers on success (`http.rs:96-107`)
   - Not mentioned in architecture doc

4. **Lines 131 - Permission route description misleading**
   - Says "GET pending permissions for a session"
   - But `get_pending_permissions_for_session()` ignores session_id param and returns ALL pending permissions

## Bugs Found in Related Code

1. **permission.rs:27 - session_id mismatch check is wrong**
   ```rust
   if req.session_id != perm_id.splitn(2, '-').next().unwrap_or(&req.session_id)
   ```
   - `perm_id` format is `{tool_call_id}-{tool_name}`, not `{session_id}-...`
   - This comparison will almost never match as intended

2. **permission.rs:65-90 - get_pending_permissions_for_session ignores session_id**
   - Comment admits: "To filter by session, we would need to extend the registry to store session_id"
   - Returns all pending permissions regardless of which session was requested

3. **question.rs:63-73 - get_pending_questions_for_session filter is faulty**
   ```rust
   .filter(|id| *id == session_id)
   ```
   - `pending_question_ids()` returns IDs that don't match session_id format
   - Self-comparison means returns empty results for most sessions

## Specific Line Number Corrections

| Location | Issue | Fix |
|----------|-------|-----|
| Lines 52-68 | Missing FromRef impls | Add: `impl FromRef<ServerState> for SqlitePool/Arc<RwLock<McpService>>/Config` |
| Lines 63-67 | check_rate_limit returns `bool` not `(bool, usize)` | Document actual signature returns only `bool` |
| Line 127 | Missing rate limit response headers | Add X-RateLimit-* headers to middleware description |
| Line 131 | "Get pending permissions" description misleads | Clarify: returns all pending permissions (session_id param unused) |
