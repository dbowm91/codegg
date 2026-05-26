# Server Architecture Review

## Summary
The server module documentation is mostly accurate with well-documented routes and protocol definitions. Several discrepancies exist around TuiMessage variants (missing `RenderFrame`), WebSocket methods, and SSE integration status that should be clarified.

## Verified Correct
- `run_server()` entry point - `src/server/http.rs:156` with correct signature
- `ServerState` struct - `src/server/state.rs:12-19` with correct fields
- `WsRateLimiter` struct - `src/server/state.rs:21-26` with `cache`, `max_requests`, `window`
- Module exports - `src/server/mod.rs:1-11` exports `run_server`, `discover_services`, `MdnsService`, `ServerState`
- REST API routes - `src/server/http.rs:220-289` matches documented routes exactly
- Session routes table - All 10 session endpoints match `src/server/routes/session.rs`
- Auth middleware - `src/server/middleware/auth.rs:7-41` validates tokens correctly
- `validate_ws_auth()` - `src/server/ws.rs:78-109` shared between WebSocket handlers
- `/ws` endpoint - `src/server/ws.rs:111-124` with JSON-RPC methods
- `/tui` endpoint - `src/server/ws.rs:362-375` with TuiMessage protocol
- SSE handler - `src/server/routes/event.rs:12-32` subscribes to GlobalEventBus
- TUI replay buffer - `src/server/ws.rs:23-26` with 1024 event capacity
- `sanitize_path()` function - `src/server/routes/file.rs:13-63` with symlink check via `check_path_for_symlinks()`
- Rate limiting - HTTP rate limiter at `src/server/http.rs:41-72`, WebSocket rate limiter in `src/server/state.rs:28-51`

## Discrepancies Found
- **TuiMessage `RenderFrame` missing from doc**: The protocol table at lines 209-235 only shows Server→Client variants but omits `RenderFrame` which exists at `src/protocol/tui.rs:34-36`. This is a significant omission in the protocol documentation.
- **Auth middleware doc order differs**: Doc at lines 172-178 says order is: CODEGG_SERVER_AUTH_DISABLED → CODEGG_SERVER_TOKEN → server.token → reject. Actual code at `src/server/middleware/auth.rs:12-40` is: CODEGG_SERVER_AUTH_DISABLED → check CODEGG_SERVER_TOKEN first, then server.token, then **OK (allow)** if none set (not reject). The doc incorrectly says it rejects if no token is set.
- **TuiMessage QuestionResponse field name**: Doc at line 220 shows `answers: serde_json::Value` but code at `src/protocol/tui.rs:31-32` has `id: String, answers: serde_json::Value`. The `id` field is missing from the doc.

## Bugs Identified
- **Auth middleware allows requests without token**: Per `src/server/middleware/auth.rs:37-39`, if no `CODEGG_SERVER_TOKEN` env var and no `server.token` in config, the middleware returns `Ok(next.run(request).await)` - it allows the request through instead of rejecting. This contradicts the documented behavior at line 177 which says "Reject if none set".

## Stale Items in Architecture Doc
- **Lines 201-206 (SSE Methods)**: The doc references `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` as if they belong to the server module, but they are actually MCP client methods in `src/mcp/remote.rs`. This section should be moved to or cross-referenced from `architecture/mcp.md`.
- **SSE Known Issue at line 205**: "SSE methods exist but are not automatically called during remote connection setup" - this is documented as a known issue but doesn't belong in the server architecture doc since it's an MCP client issue.

## Improvement Suggestions
- Add `RenderFrame` to the TuiMessage protocol table
- Fix auth middleware documentation to accurately reflect that missing tokens result in **allowing** the request (which may be a security concern)
- Add `id` field to `QuestionResponse` in protocol documentation
- Move SSE client method references to MCP architecture doc
- Consider adding the health check endpoint (`/health`) to the route documentation
- Document that the `Pool` type in `ServerState` is actually `sqlx::SqlitePool` (line 15 of state.rs shows `pool: SqlitePool` matching the doc)