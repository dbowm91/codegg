# Server Architecture Review

**Review Date**: 2026-05-26
**Reviewed File**: `architecture/server.md` (271 lines)
**Source Directory**: `src/server/`

---

## Summary

The documentation is largely accurate with correct line numbers and field definitions. However, there are several discrepancies: incomplete module exports, missing mDNS documentation, and some behavioral nuances around `RenderFrame` and WebSocket authentication.

---

## Module Organization

### Files in `src/server/`

| File | Status | Notes |
|------|--------|-------|
| `mod.rs` | ⚠️ Partial | Exports `http`, `mdns`, `middleware`, `routes`, `rpc`, `state`, `ws`. Doc only mentions `http`, `mdns`, `state` |
| `http.rs` | ✅ | 307 lines - server initialization and router setup |
| `state.rs` | ✅ | 69 lines - ServerState and WsRateLimiter |
| `ws.rs` | ✅ | 667 lines - WebSocket handlers |
| `rpc.rs` | ✅ | 115 lines - JSON-RPC types (RpcRequest, RpcResponse, RpcError) |
| `mdns.rs` | ❌ Missing | 384 lines - mDNS service discovery, not documented at all |
| `middleware/mod.rs` | ✅ | 1 line - just exports auth |
| `middleware/auth.rs` | ✅ | 45 lines - authentication middleware |
| `routes/mod.rs` | ✅ | 25 lines - route module re-exports |
| `routes/session.rs` | ✅ | 188 lines |
| `routes/config.rs` | ✅ | 121 lines |
| `routes/mcp.rs` | ✅ | 46 lines |
| `routes/event.rs` | ✅ | 32 lines |
| `routes/permission.rs` | ✅ | 74 lines |
| `routes/question.rs` | ✅ | 74 lines |
| `routes/provider.rs` | ✅ | 32 lines |
| `routes/tool.rs` | ✅ | 30 lines |
| `routes/file.rs` | ✅ | 174 lines |
| `routes/project.rs` | ✅ | 126 lines |
| `routes/workspace.rs` | ✅ | 106 lines |
| `routes/health.rs` | ✅ | 5 lines |

### Module Exports (mod.rs)

**Documented** (lines 29-33):
```rust
pub use http::run_server;
pub use mdns::{discover_services, MdnsService};
pub use state::ServerState;
```

**Actual** (`src/server/mod.rs:1-11`):
```rust
mod http;
mod mdns;
mod middleware;
pub mod routes;
pub mod rpc;          // ← Not documented
mod state;
mod ws;

pub use http::run_server;
pub use mdns::{discover_services, MdnsService};
pub use state::ServerState;
```

**Issues**:
1. `rpc` module is public (line 5) but not documented
2. `routes` is public (line 4) but not documented
3. `ws` is private (line 7) - correct, not exported

---

## Component Verification

### Entry Point ✅
`src/server/http.rs:156`:
```rust
pub async fn run_server(host: &str, port: u16) -> Result<(), ServerRuntimeError>
```
Matches documentation line 21.

### http.rs Server Setup ✅
Lines 37-50 describe initialization steps correctly:
1. SQLite pool - `src/server/http.rs:162-164`
2. Config loading - `src/server/http.rs:166`
3. MCP connections - `src/server/http.rs:169-201`
4. ServerState creation with WsRateLimiter - `src/server/http.rs:203-209`
5. Router with middleware - `src/server/http.rs:220-289`

### Middleware Stack ✅
All documented middleware is present and in correct order:
- CORS (lines 112-154, applied at 286)
- Auth (lines 266-269)
- Rate limit (lines 213, 270-273)
- Compression (lines 215-218, 287)
- Security headers (lines 274-286)
- Trace (line 288)

### ServerState ✅
`src/server/state.rs:13-19`:
```rust
pub struct ServerState {
    pub project_dir: String,
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,
}
```
Fields match exactly. Document says "Shared across WebSockets" for WsRateLimiter - confirmed.

### WsRateLimiter ✅
`src/server/state.rs:22-26`:
```rust
pub struct WsRateLimiter {
    cache: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}
```
All fields documented. Rate limiting of 100 req/60s confirmed at `http.rs:208, 213`.

### WebSocket Endpoints ✅
- `/ws` - JSON-RPC at `src/server/ws.rs:111`
- `/tui` - TuiMessage protocol at `src/server/ws.rs:362`

### /ws JSON-RPC Methods ✅
`src/server/ws.rs:192-358` - All 5 methods implemented:
- `sessions.list`
- `sessions.get`
- `sessions.create`
- `providers.list`
- `tools.list`

### /tui TuiMessage Handler ✅
Lines 91-93 documentation matches `src/server/ws.rs:523-609`.

`convert_app_event()` at `ws.rs:612-663` correctly converts AppEvent to TuiMessage.

### Auth Validation ⚠️

**Two separate auth mechanisms**:

1. **REST API auth** (`src/server/middleware/auth.rs:7-41`):
   - Checks `CODEGG_SERVER_AUTH_DISABLED` first
   - Then checks `CODEGG_SERVER_TOKEN` env var
   - Then checks `server.token` config field
   - **If no token configured, allows request through**

2. **WebSocket auth** (`src/server/ws.rs:78-109`):
   - Checks `CODEGG_SERVER_AUTH_DISABLED` to skip auth
   - Validates against `CODEGG_SERVER_TOKEN` env var only
   - **If no token in env, returns INTERNAL_SERVER_ERROR (500)**

The documentation at lines 175-186 describes the REST API middleware correctly. The note at line 186 "When no token is configured, requests are **allowed** through" is accurate for REST API.

---

## REST API Routes ✅

All routes verified against `src/server/http.rs:220-289`:

| Route | Method | Handler | Location |
|-------|--------|---------|----------|
| `/api/sessions` | GET, POST | `list_sessions`, `create_session` | `:221-224` |
| `/api/sessions/:id` | GET | `get_session` | `:225` |
| `/api/sessions/:id/archive` | DELETE | `archive_session` | `:226` |
| `/api/sessions/:id/fork` | POST | `fork_session` | `:227` |
| `/api/sessions/:id/share` | POST | `share_session` | `:228` |
| `/api/sessions/:id/unshare` | POST | `unshare_session` | `:229` |
| `/api/sessions/:id/revert` | POST | `revert_session` | `:230` |
| `/api/sessions/:id/unrevert` | POST | `unrevert_session` | `:231` |
| `/api/sessions/:id/messages` | GET | `list_messages` | `:232` |
| `/api/config` | GET | `get_config` | `:233` |
| `/api/mcp` | GET | `list_mcp_servers` | `:234` |
| `/api/event` | GET | `sse_handler` | `:235` |
| `/api/question/:session_id` | GET, POST | `get_pending_questions`, `submit_question` | `:236-239` |
| `/api/permission/:session_id` | GET | `get_pending_permissions` | `:241-243` |
| `/api/permission/:session_id/submit` | POST | `submit_permission` | `:244-247` |
| `/api/providers` | GET | `list_providers` | `:248` |
| `/api/tools` | GET | `list_tools` | `:249` |
| `/api/file/read` | GET | `read_file` | `:250` |
| `/api/file/list` | GET | `list_files` | `:251` |
| `/api/file/write` | POST | `write_file` | `:252` |
| `/api/file/delete` | DELETE | `delete_file` | `:253` |
| `/api/project` | GET, POST | `get_project`, `create_project` | `:254-257` |
| `/api/project/list` | GET | `list_projects` | `:258` |
| `/api/workspace` | GET, POST | `get_workspace`, `create_workspace` | `:259-262` |
| `/api/workspace/list` | GET | `list_workspaces` | `:263` |
| `/ws` | GET | `handle_ws` | `:264` |
| `/tui` | GET | `handle_tui` | `:265` |
| `/health` | GET | `health_check` | `:292` |

All routes match documentation (lines 99-171).

---

## file.rs Path Sanitization ✅
`src/server/routes/file.rs:13-63`

Four-step process matches documentation (lines 190-194):
1. Joins root and requested path → `file.rs:15`
2. Checks for symlinks via `check_path_for_symlinks()` → `file.rs:27`
3. Canonicalizes and verifies result starts with root → `file.rs:34, 56`
4. For non-existent paths, manually resolves `..` components → `file.rs:36-52`

---

## SSE Event Handler ✅
`src/server/routes/event.rs:12-31`

Subscribes to `GlobalEventBus::subscribe()` (line 13). Confirmed.

---

## TUI Replay Buffer ✅
`src/server/ws.rs:23-50`

- Buffer capacity: 1024 (`ws.rs:26`)
- Sequence number: `AtomicU64` starting at 1 (`ws.rs:23`)
- `replay_tui_events()` filters by `seq > from_event_seq` (`ws.rs:45`)
- `Resume` handling at `ws.rs:553-572` sends `ResyncRequired` after replay

---

## TuiMessage Protocol ⚠️

### Client → Server ✅
`src/protocol/tui.rs:8-33`

All 8 variants documented at lines 215-222 match:
- `Input` (line 8)
- `KeyDown` (line 11)
- `MouseClick` (line 15)
- `Resize` (line 19)
- `Resume` (line 23)
- `PermissionResponse` (line 26)
- `QuestionResponse` (line 30)
- `SessionInfo` (line 49)

### Server → Client ⚠️
`src/protocol/tui.rs:4-75`

All 11 variants documented at lines 224-237 are defined:
- `EventEnvelope` (line 4)
- `TextDelta` (line 37)
- `RenderFrame` (line 34)
- `ToolCallStarted` (line 56)
- `ToolResult` (line 61)
- `PermissionPending` (line 40)
- `QuestionPending` (line 45)
- `SessionInfo` (line 49)
- `SessionEnded` (line 53)
- `Error` (line 66)
- `ResyncRequired` (line 69)

**Issue**: `RenderFrame` is documented as a Server→Client message, but in the actual codebase:
- It appears only in the Client→Server direction in `TuiMessage` enum
- `convert_app_event()` at `ws.rs:612-663` does NOT produce `RenderFrame` from any `AppEvent`
- It appears to be unused in the server→client direction

The `RenderFrame` variant is only sent **from client to server** (terminal render frames from a remote TUI client), not from server to client.

---

## Configuration ✅
`src/server/http.rs:112-154` for CORS origins, lines 156-307 for full initialization.

Environment variables documented at lines 251-253:
- `CODEGG_SERVER_TOKEN`
- `CODEGG_SERVER_AUTH_DISABLED`

Both are checked in `middleware/auth.rs:12,17`.

---

## Error Handling ✅
`src/error.rs:458-473`

All 5 variants match documentation (lines 257-265):
- `Bind(String)` - line 460
- `Shutdown(String)` - line 463
- `WebSocket(String)` - line 466
- `Rpc(String)` - line 469
- `Auth(String)` - line 472

---

## Missing from Documentation

1. **mDNS module** (`src/server/mdns.rs`) - 384 lines, completely undocumented
2. **`rpc` module** (`src/server/rpc.rs`) - 115 lines, public but not mentioned
3. **`routes` module** - public but not mentioned
4. **`RenderFrame` direction clarification** - appears in Client→Server direction in protocol, but docs label it under Server→Client table

---

## Line Number Verification

| Claim | Documented Line | Actual Line | Status |
|-------|----------------|-------------|--------|
| `run_server` function | 21 | `http.rs:156` | ✅ (doc line 21 is code example) |
| ServerState fields | 55-61 | `state.rs:13-19` | ✅ |
| WsRateLimiter fields | 63-67 | `state.rs:22-26` | ✅ |
| mod.rs exports | 29-33 | `mod.rs:9-11` | ✅ |
| Middleware stack | 44-50 | `http.rs:266-289` | ✅ |
| Route definitions | 99-171 | `http.rs:220-289` | ✅ |
| auth_middleware | 177 | `middleware/auth.rs:7` | ✅ |
| sanitize_path | 190 | `file.rs:13` | ✅ |
| SSE handler | 198 | `event.rs:12` | ✅ |
| TuiMessage enum | 210 | `tui.rs:1` | ✅ |
| ServerRuntimeError | 258 | `error.rs:458` | ✅ |

---

## Recommendations

1. **Add mDNS documentation** - The `_opencode._tcp.local.` service discovery is entirely missing
2. **Clarify `RenderFrame` direction** - Move it to Client→Server table or note it's bi-directional
3. **Document `rpc` and `routes` module exports** in mod.rs section
4. **Clarify WebSocket auth behavior** - Note that `validate_ws_auth()` in ws.rs differs from REST auth middleware

---

## Verification Commands

```bash
# Build verification
cargo build --features server 2>&1 | head -50

# List server source files
ls -la src/server/
ls -la src/server/routes/
ls -la src/server/middleware/
```