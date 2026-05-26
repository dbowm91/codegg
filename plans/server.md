# server Architecture Review Findings

## Verified Claims

- **Module location**: `src/server/` - CORRECT (mod.rs exports)
- **ServerState struct fields**: `project_dir`, `pool`, `mcp_service`, `config`, `ws_rate_limiter` - CORRECT (state.rs:13-19)
- **WsRateLimiter struct**: `cache`, `max_requests`, `window` - CORRECT (state.rs:22-26)
- **Rate limiting config**: 100 req/60s per IP - CORRECT (http.rs:208, 213)
- **CORS origins**: default includes `http://localhost:3000` and `http://127.0.0.1:3000` - CORRECT (http.rs:117-120)
- **Security headers**: X-Content-Type-Options, X-Frame-Options, HSTS - CORRECT (http.rs:274-285)
- **WebSocket endpoints**: `/ws` (JSON-RPC) and `/tui` (TuiMessage) - CORRECT (http.rs:264-265)
- **Auth middleware**: Checks `CODEGG_SERVER_AUTH_DISABLED` env, then `CODEGG_SERVER_TOKEN`, then config.token - CORRECT (middleware/auth.rs:12-19)
- **Auth allows requests when no token configured**: Lines 37-39 in middleware/auth.rs - CORRECT (documented note accurate)
- **SSE event stream**: Subscribes to GlobalEventBus at `/api/event` - CORRECT (routes/event.rs:12-31)
- **TUI Replay Buffer**: Bounded VecDeque with capacity 1024, sequence numbers - CORRECT (ws.rs:23-26)
- **TuiMessage protocol** in protocol/tui.rs - ALL VARIANTS VERIFIED:
  - EventEnvelope, Input, KeyDown, MouseClick, Resize, Resume - CORRECT
  - PermissionResponse, QuestionResponse - CORRECT
  - RenderFrame, TextDelta - CORRECT
  - PermissionPending, QuestionPending - CORRECT
  - SessionInfo, SessionEnded, ToolCallStarted, ToolResult - CORRECT
  - Error, ResyncRequired - CORRECT
- **Session routes**: All 11 endpoints documented in table - ALL PRESENT (http.rs:221-232)
- **Config routes**: GET `/api/config` - CORRECT (http.rs:233)
- **MCP routes**: GET `/api/mcp` - CORRECT (http.rs:234)
- **Permission routes**: GET and POST at `/api/permission/:session_id` - CORRECT (http.rs:240-247)
- **Question routes**: GET and POST at `/api/question/:session_id` - CORRECT (http.rs:236-239)
- **Provider/Tool routes**: GET `/api/providers`, GET `/api/tools` - CORRECT (http.rs:248-249)
- **Health route**: GET `/health` (no auth) - CORRECT (http.rs:292)
- **File routes**: Read, list, write, delete - ALL PRESENT (http.rs:250-253)
- **Project routes**: Get, create, list - ALL PRESENT (http.rs:254-258)
- **Workspace routes**: Get, create, list - ALL PRESENT (http.rs:259-263)
- **ServerRuntimeError enum**: Bind, Shutdown, WebSocket, Rpc, Auth - CORRECT (error.rs)
- **SSE methods in remote.rs**: `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` at lines 698-747 - CORRECT

## Stale Information

- **No stale information found**: All claims verified against source

## Bugs Found

- **Permission route path mismatch**: Document says `/api/permission/:session_id/submit` for POST, but actual route is `/api/permission/:session_id` with POST at line 240-247 in http.rs. The route table in docs shows two separate paths but there's actually one route with GET and POST on same path. Minor documentation inconsistency.

## Improvements Suggested

- **Document update needed**: The Permission Routes table shows separate entries for GET and POST with different paths (`/api/permission/:session_id` and `/api/permission/:session_id/submit`). Actual implementation has single route with GET and POST on same path. Should clarify table to show single row with "GET, POST" methods.
- **Client SSE note location**: The doc mentions SSE methods are documented in `architecture/mcp.md` but they actually live in `src/mcp/remote.rs:698-747` - this is a cross-reference that works but could be more specific.

## Cross-Module Issues

- **TuiMessage dependency**: Server module depends on `src/protocol/tui.rs` for TuiMessage enum. Changes to that protocol affect server behavior.
- **GlobalEventBus dependency**: SSE handler and TUI WebSocket both depend on GlobalEventBus for event streaming. If event types change, both need updates.
- **PermissionRegistry/QuestionRegistry**: Server uses static methods on these registries (lines 457-458, 566-567 in ws.rs) - noted in AGENTS.md that these don't filter by session_id which could affect multi-session scenarios.