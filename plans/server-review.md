# Server Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Entry point `run_server(host, port)` exists | VERIFIED | `src/server/http.rs:159` - signature matches |
| Module exports `run_server`, `discover_services`, `MdnsService`, `ServerState` | VERIFIED | `src/server/mod.rs:8-10` |
| ServerState has `project_dir`, `pool`, `mcp_service`, `config`, `ws_rate_limiter` | INCORRECT | `ServerState` at `state.rs:14-21` has 6 fields including `event_bus` not documented |
| WsRateLimiter is shared across WebSockets | VERIFIED | `Arc<WsRateLimiter>` in state, cloned on line 108/369 |
| Rate limiting is 100 req/60s per IP | VERIFIED | `RateLimiter::new(100, 60)` at http.rs:217, WsRateLimiter at state.rs:32 |
| CORS middleware is configurable | VERIFIED | `build_cors()` at http.rs:115-157 handles origins from config |
| Auth middleware checks CODEGG_SERVER_AUTH_DISABLED, CODEGG_SERVER_TOKEN, server.token | VERIFIED | `auth_middleware` at middleware/auth.rs:7-43 |
| Compression uses gzip/br | VERIFIED | `CompressionLayer::new().gzip(true).br(true)` at http.rs:219-222 |
| Security headers (X-Content-Type-Options, X-Frame-Options, HSTS) | VERIFIED | http.rs:278-289 |
| `/ws` endpoint supports sessions.list, sessions.get, sessions.create, providers.list, tools.list | VERIFIED | ws.rs:156-322 |
| `/tui` endpoint uses TuiMessage protocol | VERIFIED | ws.rs:326-455 |
| Auth validation via `validate_ws_auth()` shared between handlers | VERIFIED | ws.rs:42-73, called at lines 81 and 332 |
| SSE handler uses GlobalEventBus directly | INCORRECT | SSE at routes/event.rs:13 uses `GlobalEventBus::subscribe()` directly, but state.rs:11 imports `crate::server::routes::GlobalEventBus` which doesn't exist - COMPILE ERROR |
| TUI WebSocket converts AppEvent to TuiMessage | VERIFIED | `convert_app_event()` at ws.rs:547-598 |
| Session routes: list, get, create, archive, fork, share, unshare, revert, unrevert, messages | VERIFIED | routes/session.rs - all endpoints present |
| Config route returns redacted API keys | VERIFIED | routes/config.rs:23-70 redaction logic |
| MCP route lists servers | VERIFIED | routes/mcp.rs:17-46 |
| Event route SSE streams | VERIFIED | routes/event.rs:12-31 |
| Permission routes for pending/submit | VERIFIED | routes/permission.rs |
| Question routes for pending/submit | VERIFIED | routes/question.rs |
| Provider/Tool routes list endpoints | VERIFIED | routes/provider.rs, routes/tool.rs |
| File routes read/list/write/delete | VERIFIED | routes/file.rs - all present |
| Project routes get/list/create | VERIFIED | routes/project.rs - all present |
| Workspace routes get/list/create | VERIFIED | routes/workspace.rs - all present |
| ServerRuntimeError enum has Bind, Shutdown variants | VERIFIED | crate::error::ServerRuntimeError |
| Event bus field was removed from ServerState | INCORRECT | ServerState still has `event_bus: GlobalEventBus` field at state.rs:18 - NOT REMOVED |

## Bugs Found

### Critical

1. **GlobalEventBus Import Error** (`state.rs:11`, `http.rs:210`)
   - Code imports `crate::server::routes::GlobalEventBus` but it doesn't exist in routes module
   - SSE handler correctly uses `crate::bus::global::GlobalEventBus::subscribe()` directly
   - The local `event_bus` in ServerState is never used - both SSE and TUI subscribe to global directly
   - **Compiles: NO** - causes compilation error

2. **RpcRequest/RpcResponse Not Imported** (`ws.rs:116-130`)
   - `ws.rs` uses `RpcRequest`, `RpcResponse`, `RpcError` but doesn't import them from `rpc.rs`
   - Only `JsonRpcMessage` is defined in `rpc.rs` - the types used in ws.rs are different
   - This appears to be dead code or the types were renamed
   - **Compiles: NO** - causes compilation errors

### High

3. **Unused Local EventBus** (`state.rs:18`, `http.rs:210`)
   - ServerState has `event_bus: GlobalEventBus` field that's never used
   - SSE handler (`event.rs:13`) and TUI handler (`ws.rs:401`) both subscribe directly to `crate::bus::global::GlobalEventBus`
   - The local event_bus created at http.rs:210 is completely orphaned

4. **Health Check Duplicated** (`http.rs:111-113`, `routes/health.rs:3`)
   - `http.rs` has inline `async fn health_check()` returning `"ok"`
   - `routes/health.rs` also has `pub async fn health_check()` returning `"ok"`
   - The one in `http.rs:296` is actually used in the router, making `routes/health.rs` dead code

### Medium

5. **TuiSessionState.model Hardcoded** (`ws.rs:470`)
   - Default model is hardcoded: `"anthropic/claude-sonnet-4-20250514"`
   - Should be configurable or derive from actual session config

6. **rate_limit_key Can Be Zero-Length** (`ws.rs:542`)
   - If `SessionInfo` id is empty string, `rate_limit_key` becomes `"session:"`
   - Could cause issues with rate limiter tracking

7. **PermissionResponse Not Actually Responding** (`routes/permission.rs:23-45`)
   - `submit_permission` validates request but never calls `PermissionRegistry::respond()`
   - Simply returns a success response without actually recording the decision
   - Permission system won't actually record the user's choice

8. **Event Stream Doesn't Send heartbeats** (`routes/event.rs:26-31`)
   - Heartbeat stream is created but merged after the main stream
   - If main stream has no events, heartbeat won't fire until first event
   - Should use `merge()` with heartbeat first or ensure heartbeat fires independently

## Improvement Suggestions

### Performance

1. **ProviderRegistry/ToolRegistry Created Per-Request**
   - `ws.rs:285-286`, `provider.rs:19-20`, `tool.rs:19` create new registry instances on each request
   - Should be cached/shared or at least use a shared instance

2. **SessionStore Created Per-Request**
   - Every session route creates `SessionStore::new(state.pool)`
   - Could be cached or derived from state more efficiently

3. **MCP Service Lock Contention**
   - `routes/mcp.rs:20` does `mcp_service.read().await` which holds lock during serialization
   - Could release lock earlier and then serialize

### Correctness

1. **Auth Middleware Returns 401 Even When Auth Disabled**
   - When `CODEGG_SERVER_AUTH_DISABLED` is set, auth_middleware returns 401 if no token configured (line 41)
   - This contradicts the intent: if auth is disabled, requests should be allowed

2. **Config Reload Not Supported**
   - Config is loaded once at startup in `http.rs:169`
   - No mechanism to reload config or notify server of config changes

3. **WebSocket Auth Disabled Logic Inconsistent**
   - `validate_ws_auth()` at ws.rs:43 checks `CODEGG_SERVER_AUTH_DISABLED` via `is_err()`
   - But `auth_middleware` checks via `is_ok()` - one checks if env var IS set, other if it IS NOT set
   - This creates an inconsistency between HTTP and WebSocket auth

### Maintainability

1. **Missing Error Types for Server Operations**
   - No dedicated error type for server operations beyond generic `ServerRuntimeError`
   - Operations like invalid routes, deserialization failures should have typed errors

2. **Event Type Serialization Not Consistent**
   - SSE at event.rs:17 uses `event.event_type()` but AppEvent may not have this method
   - Should verify `AppEvent::event_type()` exists on all event variants

3. **No Request ID/Correlation Across Logs**
   - WebSocket messages don't have correlation IDs for tracing
   - Difficult to trace a request from client through to completion

4. **Hardcoded Magic Numbers**
   - Rate limit of 100, window of 60s, broadcast channel size of 2048
   - Should be configurable via config schema

## Priority Actions (top 5 items to fix)

1. **Fix GlobalEventBus import error** - Change `use crate::server::routes::GlobalEventBus` to `use crate::bus::global::GlobalEventBus` in state.rs and http.rs, OR remove the unused event_bus field entirely since both handlers use the global directly

2. **Fix RpcRequest/RpcResponse types in ws.rs** - Either import from rpc.rs if types exist there, or define the missing types (RpcRequest, RpcResponse, RpcError are used but not defined)

3. **Implement actual permission response** - `submit_permission` must call `PermissionRegistry::respond()` to actually record the user's decision

4. **Fix auth middleware logic** - When `CODEGG_SERVER_AUTH_DISABLED` is set, requests should be allowed even without a token

5. **Remove dead code** - Either remove the local `event_bus` field from ServerState or actually use it consistently