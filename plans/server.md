# Server Architecture Review

## Architecture Document
- Path: architecture/server.md

## Source Code Location
- src/server/

## Verification Summary
Partial

The architecture document is mostly accurate but has several inconsistencies and missing documentation items that should be addressed.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Entry point `run_server(host, port)` | Pass | Matches http.rs:156 |
| Module exports in mod.rs | Pass | Accurate at lines 8-10 |
| HTTP router middleware layers | Pass | CORS, Auth, RateLimit, Compression, Security headers all verified |
| Rate limit 100 req/60s per IP | Pass | Both RateLimiter (http.rs) and WsRateLimiter (state.rs) use 100, 60 |
| ServerState fields | Pass | All 6 fields verified in state.rs |
| WsRateLimiter shared across WebSockets | Pass | Verified in http.rs:209 - created once, shared via state |
| `/ws` JSON-RPC endpoint | Pass | Supported methods verified in ws.rs:156-323 |
| `/tui` TuiMessage protocol | Pass | All variants handled in ws.rs |
| Auth validation via `validate_ws_auth()` | Pass | Shared function verified ws.rs:42-73 |
| Session routes (`/api/sessions/*`) | Pass | All 9 endpoints verified in http.rs:222-233 and session.rs |
| Config route (`/api/config`) | Pass | Verified in http.rs:234 |
| MCP routes (`/api/mcp`) | Pass | Verified in http.rs:235 |
| Event SSE route (`/api/event`) | Pass | Verified in http.rs:236, directly uses GlobalEventBus (not state) |
| Permission routes | Pass | Both GET and POST verified in http.rs:241-248 |
| Question routes | Pass | Both GET and POST verified in http.rs:237-240 |
| Provider/Tool routes | Pass | Verified in http.rs:249-250 |
| File routes | Pass | All 5 endpoints verified in http.rs:251-254 |
| Project routes | Pass | All 3 endpoints verified in http.rs:255-259 |
| Workspace routes | Pass | All 3 endpoints verified in http.rs:260-264 |
| Auth middleware priority | Partial | Doc order: CODEGG_SERVER_AUTH_DISABLED -> CODEGG_SERVER_TOKEN -> config. Actual: CODEGG_SERVER_AUTH_DISABLED -> (CODEGG_SERVER_TOKEN or config.token). Doc claims server.token is checked after env var, but code uses OR logic. |
| Token validation uses constant-time comparison | Pass | Verified via `subtle::ConstantTimeEq` in auth.rs:44 and ws.rs:60 |
| SSE handler subscribes to GlobalEventBus | Pass | event.rs:13 uses `GlobalEventBus::subscribe()` directly |
| SSE 15-second heartbeat | Pass | Verified event.rs:26-28 |
| SSE sends ResyncRequired on lag | Partial | Only partially implemented - TUI WS sends resync on lag (ws.rs:428), but SSE handler does not |
| File sanitize_path function | Pass | All 4 steps verified in file.rs:13-63 |
| ServerRuntimeError enum | Pass | Bind and Shutdown variants exist |
| Health route `/health` | Pass | Simple "ok" string at routes/health.rs |

## Issues Found

### Bugs
- **SSE ResyncRequired missing**: SSE handler (`routes/event.rs`) does not send `ResyncRequired` when the client lags, but the architecture doc states it should. Only the TUI WebSocket handler implements this (ws.rs:428).

### Inconsistencies
- **GlobalEventBus duplication**: Architecture doc (line 70) states "event_bus field was removed - SSE handler and TUI WebSocket directly use GlobalEventBus::subscribe()". However, `ServerState` still contains `event_bus: GlobalEventBus` field (state.rs:18) which is used for `GlobalEventBus::subscribe()` in upgrade_tui (ws.rs:401). The SSE handler correctly uses global directly. The TUI handler still gets it via state.clone(). The doc is misleading about the field being "removed".
- **SSE endpoint uses GlobalEventBus directly but routes/event.rs imports from crate::bus::global**: While SSE correctly uses the global directly (event.rs:13), the http.rs:207 creates a `routes::GlobalEventBus::new()` which is never used by event.rs. This local event_bus is only used by upgrade_tui for subscription (ws.rs:401), not by SSE. This is confusing but functional.
- **Auth middleware behavior differs from doc**: Doc says auth check order is: 1) CODEGG_SERVER_AUTH_DISABLED 2) CODEGG_SERVER_TOKEN 3) server.token 4) Reject. But code uses OR logic between CODEGG_SERVER_TOKEN and config.server.token, meaning if either is set and valid, auth passes. If neither is set but CODEGG_SERVER_AUTH_DISABLED is not set, it returns INTERNAL_SERVER_ERROR (auth.rs:68) not UNAUTHORIZED.

### Missing Documentation
- **list_messages endpoint**: Session routes table in architecture doc shows `/api/sessions/:id/messages` (GET) but it's actually implemented in `config.rs:100-120` not session.rs, and it uses `MessageListResponse` with `messages` and `total` fields, not a simple message list.
- **McpService in ServerState**: The `Arc<RwLock<McpService>>` type is used but McpService is not imported/defined in server module. It comes from crate::mcp.
- **GlobalEventBus in routes**: There's a `routes::GlobalEventBus` (defined in routes/mod.rs via re-export from a local module that doesn't appear to exist in the routes directory structure). This creates confusion about which GlobalEventBus is being used.
- **Rate limiting behavior**: When a WebSocket client exceeds rate limit, the server sends an error response but does not close the connection immediately - it breaks after sending the error (ws.rs:116-128). This is correct behavior but worth documenting.
- **WsRateLimiter vs RateLimiter**: There are two rate limiters - `RateLimiter` in http.rs (used for HTTP middleware) and `WsRateLimiter` in state.rs (used for WebSocket connections). Both have same default values but different implementations and are not shared between HTTP and WS rate limiting contexts.
- **RPC Response error code 429 for rate limiting**: In ws.rs, rate limit errors use code 429 in RpcResponse (ws.rs:121), but this is a non-standard JSON-RPC error code (standard is -32000 to -32099 for server errors, or use 429 HTTP status for transport-level limiting).

### Improvement Opportunities
- **Clean up unused event_bus field**: ServerState.event_bus is only used by upgrade_tui (ws.rs:401), not by SSE. Consider whether this should remain in ServerState or be passed differently.
- **Consolidate rate limiters**: Could unify RateLimiter (http) and WsRateLimiter (state) into a single implementation since they have the same defaults.
- **Document list_messages location**: The `list_messages` function appears in config.rs (not session.rs as the route table implies) - should clarify in documentation.
- **SSE handler could send ResyncRequired**: When SSE client lags, the handler just drops events silently. Could benefit from the same ResyncRequired logic used in TUI WebSocket.
- **Health check endpoint**: Could expand `/health` to check database connectivity and MCP server status, not just return static "ok" string.

## Recommendations
1. Update architecture doc line 70 to clarify that ServerState.event_bus IS used by TUI WebSocket handler but NOT by SSE handler.
2. Add SSE ResyncRequired on lag behavior to routes/event.rs to match TUI WebSocket behavior.
3. Clarify auth middleware documentation to reflect the OR logic between env var and config token.
4. Document that list_messages is in config.rs not session.rs.
5. Consider adding database/MCP health checks to /health endpoint.
6. Add ResyncRequired support to SSE handler for client lag scenarios.
