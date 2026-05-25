# Server Module Architecture Review

**Status**: STALE (re-review after May 25 changes)

**Review Date**: 2026-05-25
**Source Files Reviewed**: `src/server/` (mod.rs, http.rs, state.rs, ws.rs, rpc.rs, mdns.rs, routes/, middleware/)
**Documentation Reviewed**: `architecture/server.md`

---

## Summary

The server module provides an Axum-based HTTP server with WebSocket support for remote TUI connections. The architecture document is **mostly accurate** but has one significant inaccuracy regarding `ServerRuntimeError` variants and one section that is misplaced (SSE client methods belong to MCP module, not server).

---

## Verified Items

### Module Structure (`mod.rs`)
| Documented | Actual | Status |
|------------|--------|--------|
| `pub use http::run_server;` | `pub use http::run_server;` | ✅ Exact match |
| `pub use mdns::{discover_services, MdnsService};` | `pub use mdns::{discover_services, MdnsService};` | ✅ Exact match |
| `pub use state::ServerState;` | `pub use state::ServerState;` | ✅ Exact match |

### ServerState (`state.rs`)
| Documented | Actual | Status |
|------------|--------|--------|
| `project_dir: String` | `project_dir: String` | ✅ |
| `pool: SqlitePool` | `pool: SqlitePool` | ✅ |
| `mcp_service: Arc<RwLock<McpService>>` | `mcp_service: Arc<RwLock<McpService>>` | ✅ |
| `config: Config` | `config: Config` | ✅ |
| `ws_rate_limiter: Arc<WsRateLimiter>` | `ws_rate_limiter: Arc<WsRateLimiter>` | ✅ |
| WsRateLimiter with `cache`, `max_requests`, `window` | WsRateLimiter with same fields | ✅ |

### HTTP Server Setup (`http.rs`)
| Documented Step | Status |
|-----------------|--------|
| 1. Initializes SQLite database pool | ✅ Line 162 |
| 2. Loads configuration | ✅ Line 166 |
| 3. Connects to configured MCP servers | ✅ Lines 169-201 |
| 4. Creates ServerState with shared WsRateLimiter | ✅ Lines 203-209 |
| 5. Builds Axum router with routes and middleware | ✅ Lines 220-289 |

**Middleware verified:**
- CORS (configurable origins) ✅ Lines 112-154
- Auth middleware (token validation) ✅ Line 268
- Rate limit middleware (100 req/60s per IP) ✅ Lines 213, 270-273
- Compression (gzip/br) ✅ Lines 215-218
- Security headers (X-Content-Type-Options, X-Frame-Options, HSTS) ✅ Lines 274-285
- Trace logging ✅ Line 288

### WebSocket Endpoints (`ws.rs`)
| Endpoint | Handler | Status |
|----------|---------|--------|
| `/ws` | `handle_ws` | ✅ |
| `/tui` | `handle_tui` | ✅ |
| `validate_ws_auth()` shared | Lines 78-109, called by both | ✅ |

**JSON-RPC methods for `/ws`:**
| Method | Status |
|--------|--------|
| sessions.list | ✅ |
| sessions.get | ✅ |
| sessions.create | ✅ |
| providers.list | ✅ |
| tools.list | ✅ |

### REST API Routes (`routes/`)
All routes in the architecture doc match `http.rs` lines 220-264:

| Route | Status |
|-------|--------|
| GET/POST `/api/sessions` | ✅ |
| GET `/api/sessions/:id` | ✅ |
| DELETE `/api/sessions/:id/archive` | ✅ |
| POST `/api/sessions/:id/fork` | ✅ |
| POST `/api/sessions/:id/share` | ✅ |
| POST `/api/sessions/:id/unshare` | ✅ |
| POST `/api/sessions/:id/revert` | ✅ |
| POST `/api/sessions/:id/unrevert` | ✅ |
| GET `/api/sessions/:id/messages` | ✅ |
| GET `/api/config` | ✅ |
| GET `/api/mcp` | ✅ |
| GET `/api/event` (SSE) | ✅ |
| GET/POST `/api/question/:session_id` | ✅ |
| GET `/api/permission/:session_id` | ✅ |
| POST `/api/permission/:session_id/submit` | ✅ |
| GET `/api/providers` | ✅ |
| GET `/api/tools` | ✅ |
| GET `/api/file/read` | ✅ |
| GET `/api/file/list` | ✅ |
| POST `/api/file/write` | ✅ |
| DELETE `/api/file/delete` | ✅ |
| GET/POST `/api/project` | ✅ |
| GET `/api/project/list` | ✅ |
| GET/POST `/api/workspace` | ✅ |
| GET `/api/workspace/list` | ✅ |

### Authentication (`middleware/auth.rs`)
**Documented order:**
1. CODEGG_SERVER_AUTH_DISABLED env var (skip auth) ✅
2. CODEGG_SERVER_TOKEN env var ✅
3. server.token config field ✅
4. Reject if none set ✅

### Path Sanitization (`routes/file.rs`)
The `sanitize_path()` function documented 4-step process:
1. Joins root and requested path ✅ Line 15
2. Checks for symlinks via `check_path_for_symlinks()` ✅ Line 27
3. Canonicalizes and verifies result starts with root ✅ Lines 34, 56
4. For non-existent paths, manually resolves `..` components ✅ Lines 36-53

### SSE Handler (`routes/event.rs`)
**Documented:** SSE handler at `/api/event` subscribes directly to `GlobalEventBus::subscribe()`
**Actual:** ✅ Line 13: `let mut rx = crate::bus::global::GlobalEventBus::subscribe();`

### TUI Replay Buffer (`ws.rs`)
| Documented | Actual |
|------------|--------|
| Bounded event buffer | ✅ Line 26: `const TUI_EVENT_BUFFER_MAX: usize = 1024;` |
| Monotonically increasing sequence number | ✅ Line 23: `static TUI_EVENT_SEQ: AtomicU64 = AtomicU64::new(1);` |
| Resume replays events with higher sequence number | ✅ Lines 41-51, 553-563 |
| Sends ResyncRequired after replay | ✅ Lines 564-571 |

### TuiMessage Protocol (`src/protocol/tui.rs`)

**Client → Server variants:**
| Variant | Fields | Status |
|---------|--------|--------|
| Input | text: String | ✅ |
| KeyDown | key: String, modifiers: Vec<String> | ✅ |
| MouseClick | x: u16, y: u16 | ✅ |
| Resize | w: u16, h: u16 | ✅ |
| Resume | from_event_seq: u64 | ✅ |
| PermissionResponse | id: String, choice: String | ✅ |
| QuestionResponse | id: String, answers: serde_json::Value | ✅ |
| SessionInfo | id: String, model: String | ✅ |

**Server → Client variants:**
| Variant | Fields | Status |
|---------|--------|--------|
| EventEnvelope | event_seq: u64, payload: Box<TuiMessage> | ✅ |
| TextDelta | delta: String | ✅ |
| ToolCallStarted | tool_name, tool_id, arguments | ✅ |
| ToolResult | tool_id, output, success | ✅ |
| PermissionPending | id, tool, path | ✅ |
| QuestionPending | id, questions: Vec<QuestionSpec> | ✅ |
| SessionInfo | id, model | ✅ |
| SessionEnded | stop_reason | ✅ |
| Error | message | ✅ |
| ResyncRequired | reason, pending_permissions, pending_questions | ✅ |

---

## Discrepancies Found

### 1. ServerRuntimeError Incomplete (SIGNIFICANT)

**Location**: `architecture/server.md` lines 276-280

**Documented:**
```rust
pub enum ServerRuntimeError {
    Bind(String),     // Failed to bind address
    Shutdown(String), // Server shutdown error
}
```

**Actual** (`src/error.rs` lines 458-473):
```rust
pub enum ServerRuntimeError {
    #[error("server bind failed: {0}")]
    Bind(String),
    #[error("server shutdown error: {0}")]
    Shutdown(String),
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error("authentication failed: {0}")]
    Auth(String),
}
```

**Impact**: The architecture doc shows only 2 of 5 variants. Missing variants `WebSocket`, `Rpc`, and `Auth` are used in the actual implementation (ws.rs line 62 uses it as a rejection type).

**Recommendation**: Update architecture doc to include all 5 variants.

---

### 2. SSE Client Methods Misplaced (SIGNIFICANT - WRONG LOCATION)

**Location**: `architecture/server.md` lines 197-225

**Documented**: These methods are presented as part of the server module:
- `connect_sse()`
- `connect_sse_stream()`
- `take_sse_events()`

**Actual Location**: These methods are in `src/mcp/remote.rs` on the `RemoteClient` struct, not part of `src/server/`.

**Impact**: The architecture doc incorrectly associates these MCP client methods with the server module.

**Recommendation**: Move this section to `architecture/mcp.md` where it belongs, or remove it if the MCP module has its own architecture doc.

---

### 3. Health Route Handling (MINOR)

**Location**: `architecture/server.md` line 121 (SKILL.md line 40)

**Documented**: Health route at `GET /health` - standalone inline handler in http.rs

**Actual**: `routes/health.rs` contains a standalone `health_check()` function, but the main router uses an inline handler at the top level of `http.rs` (line 292).

**Impact**: Low - The SKILL.md accurately notes "standalone, not wired to main router"

**Recommendation**: The documentation is slightly misleading. Consider either:
1. Removing the health route from the inline router if it's meant to be standalone
2. Or using the `routes/health::health_check` function in the router

---

## Code Quality Observations

### 1. TUI Event Buffer Lock Contention

**Location**: `ws.rs` lines 24-39

The `TUI_EVENT_BUFFER` uses a `StdMutex` (from `std::sync::Mutex`) while other parts of the codebase use tokio's async mutex. This is a deliberate choice for the event recording path, but worth noting that blocking the thread could impact latency under load.

**Recommendation**: Document why `StdMutex` is used here (performance vs blocking semantics trade-off).

### 2. Rate Limiter Type Mismatch

**Location**: `http.rs` uses `tokio::sync::Mutex` for HTTP rate limiting
**Location**: `state.rs` uses `std::sync::Mutex` for WebSocket rate limiting

Both achieve similar functionality with different async/blocking characteristics. This is intentional but could be confusing.

**Recommendation**: Add comments explaining the different synchronization strategies.

### 3. Permission/Question Path in Event Handler

**Location**: `ws.rs` lines 452-463

The SSE broadcast send failure handling checks `matches!(event, AppEvent::PermissionPending { .. })` but `AppEvent` is imported at line 665, after all the code that uses it. This works due to Rust's import rules but is non-idiomatic.

**Recommendation**: Move the import to the top of the file.

---

## Recommendations

### Documentation Fixes

1. **ServerRuntimeError**: Add missing `WebSocket`, `Rpc`, and `Auth` variants to the architecture doc
2. **SSE Client Methods**: Move to MCP architecture doc or remove from server docs
3. **Health Route**: Clarify that routes/health.rs is unused by the main router

### Code Fixes

1. **Move imports**: Move `use crate::bus::events::AppEvent;` and related imports in `ws.rs` to the top of the file (line 665 → top)

### Minor Improvements

1. Add doc comments to exported items in `routes/mod.rs` explaining what each re-export does
2. Consider adding a constant `TUI_EVENT_BUFFER_DEFAULT_SIZE` or similar for clarity

---

## Conclusion

The server module architecture is **mostly well-documented** with accurate route definitions, middleware descriptions, and protocol documentation. The main issues are:

1. **ServerRuntimeError incomplete** - Shows 2 variants instead of 5
2. **SSE Client Methods misplaced** - Belong to MCP module, not server

These are documentation accuracy issues that should be corrected to prevent confusion for future developers.
