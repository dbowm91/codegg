# Server Module Architecture Review

## Verified Claims

### Entry Point & Module Exports
- Entry point `run_server(host, port)` at `http.rs:159` - signature matches documentation
- Module exports `run_server`, `discover_services`, `MdnsService`, `ServerState` - verified at `mod.rs:8-10`

### ServerState Structure
- `project_dir: String` - verified at `state.rs:15`
- `pool: SqlitePool` - verified at `state.rs:16`
- `mcp_service: Arc<RwLock<McpService>>` - verified at `state.rs:17`
- `config: Config` - verified at `state.rs:19`
- `ws_rate_limiter: Arc<WsRateLimiter>` - verified at `state.rs:20`
- `WsRateLimiter` has `cache`, `max_requests`, `window` fields - verified at `state.rs:24-28`

### Middleware
- Rate limiting is 100 req/60s per IP - verified at `http.rs:217` and `state.rs:32`
- CORS middleware is configurable via `build_cors()` at `http.rs:115-157`
- Compression uses gzip/br at `http.rs:219-222`
- Security headers (X-Content-Type-Options, X-Frame-Options, HSTS) at `http.rs:278-289`
- Auth middleware checks env vars and config token at `middleware/auth.rs:7-43`

### WebSocket Endpoints
- `/ws` endpoint defined at `http.rs:268`
- `/tui` endpoint defined at `http.rs:269`
- Auth validation via `validate_ws_auth()` shared between handlers at `ws.rs:42-73`
- TUI WebSocket converts AppEvent to TuiMessage via `convert_app_event()` at `ws.rs:547-598`

### REST API Routes
- Session routes: list, get, create, archive, fork, share, unshare, revert, unrevert - all verified in `routes/session.rs`
- Config route at `routes/config.rs` returns redacted API keys
- MCP route at `routes/mcp.rs` lists servers
- SSE event route at `routes/event.rs`
- Permission routes at `routes/permission.rs`
- Question routes at `routes/question.rs`
- Provider/Tool routes at `routes/provider.rs` and `routes/tool.rs`
- File routes at `routes/file.rs` - read/list/write/delete
- Project routes at `routes/project.rs` - get/list/create
- Workspace routes at `routes/workspace.rs` - get/list/create

### TuiMessage Protocol
- Client→Server variants: Input, KeyDown, MouseClick, Resize, PermissionResponse, QuestionResponse, SessionInfo - verified in `protocol/tui.rs`
- Server→Client variants: TextDelta, ToolCallStarted, ToolResult, PermissionPending, QuestionPending, SessionInfo, SessionEnded, Error, ResyncRequired - verified in `protocol/tui.rs`
- `RenderFrame` variant exists but marked as legacy (doc correctly notes it)
- `QuestionSpec` struct with id, prompt, default fields at `protocol/tui.rs:71-75`

## Bugs/Discrepancies Found

### Critical (Compilation Errors)

1. **GlobalEventBus Import Error** (`state.rs:11`, `http.rs:210`)
   - Code imports `crate::server::routes::GlobalEventBus` but this type doesn't exist in the routes module
   - SSE handler correctly uses `crate::bus::global::GlobalEventBus::subscribe()` directly at `event.rs:13`
   - TUI WebSocket also uses `crate::bus::global::GlobalEventBus::subscribe()` at `ws.rs:401`
   - The local `event_bus` field in ServerState is NEVER used - both handlers subscribe to the global directly
   - **This is a compilation error**

2. **RpcRequest/RpcResponse/RpcError Not Defined** (`ws.rs`)
   - `ws.rs` uses `RpcRequest`, `RpcResponse`, `RpcError` but these types are not imported or defined
   - `rpc.rs` defines `JsonRpcMessage`, `JsonRpcError` - different names than what's used in `ws.rs`
   - **This is a compilation error**

### High Priority

3. **Unused Local EventBus** (`state.rs:18`, `http.rs:210`)
   - ServerState has `event_bus: GlobalEventBus` field that's never used by any code path
   - Both SSE handler and TUI WebSocket subscribe directly to `crate::bus::global::GlobalEventBus`
   - Should either use this field consistently or remove it

4. **submit_permission Doesn't Actually Submit** (`routes/permission.rs:23-45`)
   - `submit_permission` validates the request and returns a success response
   - Never calls `PermissionRegistry::respond()` to actually record the decision
   - Permission choices are not persisted to the permission registry

5. **Health Check Duplicated** (`http.rs:111-113`, `routes/health.rs:3-4`)
   - `http.rs` has inline `async fn health_check()` returning `"ok"`
   - `routes/health.rs` also has `pub async fn health_check()` returning `"ok"`
   - The one in `http.rs` is used at `http.rs:296` - `routes/health.rs` is dead code

### Medium Priority

6. **Auth Middleware Returns 401 When Auth Disabled Without Token** (`middleware/auth.rs:37-41`)
   - When `CODEGG_SERVER_AUTH_DISABLED` is set AND no token is configured, returns 401
   - This is a safety measure but contradicts the "auth disabled" intent - if disabled, should allow requests

7. **TuiSessionState.model Hardcoded** (`ws.rs:470`)
   - Default model hardcoded as `"anthropic/claude-sonnet-4-20250514"`
   - Should be configurable or derived from actual session

8. **rate_limit_key Format Issue** (`ws.rs:541`)
   - If `SessionInfo` id is empty, `rate_limit_key` becomes `"session:"` with trailing colon
   - Could cause inconsistent rate limit tracking

9. **WebSocket Auth Logic Inconsistent** (`ws.rs:43` vs `middleware/auth.rs:12`)
   - `validate_ws_auth()` checks `CODEGG_SERVER_AUTH_DISABLED` via `is_err()` (auth disabled if env var NOT set)
   - `auth_middleware` checks via `is_ok()` (auth disabled if env var IS set)
   - This is inconsistent between HTTP and WebSocket auth paths

### Low Priority

10. **ProviderRegistry/ToolRegistry Created Per-Request**
    - `ws.rs:285-286`, `provider.rs:19-20`, `tool.rs:19` create new registry instances on each request
    - Could be cached/shared for better performance

11. **Event Type Serialization Uses Undocumented Method**
    - SSE at `event.rs:17` uses `event.event_type()` but this method isn't documented
    - Should verify this method exists on all AppEvent variants

## Improvement Suggestions

### High Priority

1. **Fix GlobalEventBus import error** - Either:
   - Remove the `event_bus` field from ServerState since it's never used, OR
   - Change the import to `use crate::bus::global::GlobalEventBus` and actually use it in SSE/TUI handlers

2. **Define missing RPC types** - Either:
   - Add `RpcRequest`, `RpcResponse`, `RpcError` types to `rpc.rs` and import them in `ws.rs`, OR
   - Rename usages in `ws.rs` to use existing `JsonRpcMessage` and `JsonRpcError` types

3. **Implement actual permission response** - `submit_permission` must call `PermissionRegistry::respond()`

4. **Fix auth middleware logic** - When `CODEGG_SERVER_AUTH_DISABLED` is set, requests should be allowed regardless of token configuration

### Medium Priority

5. **Remove dead code** - Remove `routes/health.rs` (duplicate health_check) or the inline one in `http.rs`

6. **Consolidate WebSocket auth logic** - Make `validate_ws_auth()` check `CODEGG_SERVER_AUTH_DISABLED` the same way as `auth_middleware`

7. **Make model configurable** - Remove hardcoded model from `TuiSessionState::new()`

### Low Priority

8. **Cache ProviderRegistry/ToolRegistry** - Create once and reuse instead of per-request instantiation

9. **Add correlation IDs** - WebSocket messages don't have request IDs for tracing

10. **Make rate limits configurable** - Hardcoded 100 req/60s should be in config

11. **Document AppEvent::event_type()** - Verify this method exists and document its behavior

## Configuration Notes

The documentation shows `[server]` config section with `host`, `port`, `token`, and `[server.cors]` with `origins`. This matches the actual `ServerConfig` schema. Environment variables `CODEGG_SERVER_TOKEN` and `CODEGG_SERVER_AUTH_DISABLED` are correctly documented.