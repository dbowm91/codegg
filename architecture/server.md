# Server Module

The `server` module provides HTTP server functionality for remote TUI connections and is feature-gated under the `server` feature flag.

## Overview

**Location**: `src/server/`

**Key Responsibilities**:
- Axum-based HTTP server with WebSocket support for remote TUI connections
- REST API endpoints for sessions, config, MCP servers, files, projects, and workspaces
- Server-Sent Events (SSE) for real-time event streaming
- Token-based authentication with development-friendly fallback
- Rate limiting for both HTTP requests and WebSocket connections
- mDNS service discovery for local network server detection

**Requires**: `server` feature flag

## Architecture

```
server/
├── mod.rs           # Module exports, re-exports run_server, MdnsService, ServerState
├── http.rs         # Main server setup, Axum router configuration, middleware stack
├── ws.rs           # WebSocket handlers (/ws for JSON-RPC, /tui for TuiMessage protocol)
├── state.rs        # ServerState and WsRateLimiter shared state
├── rpc.rs          # JSON-RPC request/response types
├── mdns.rs         # mDNS service discovery implementation
├── middleware/
│   └── auth.rs     # Token authentication middleware
└── routes/
    ├── mod.rs      # Route module exports
    ├── health.rs   # /health endpoint (no auth required)
    ├── event.rs    # SSE event streaming at /api/event
    ├── session.rs  # Session CRUD operations
    ├── config.rs   # Config retrieval (API keys redacted)
    ├── mcp.rs      # MCP server status listing
    ├── permission.rs # Permission response handling
    ├── question.rs # Question response handling
    ├── provider.rs # Provider listing
    ├── tool.rs     # Tool listing
    ├── file.rs     # File operations with path sanitization
    ├── project.rs  # Project management
    └── workspace.rs # Workspace management
```

## Entry Point

The server is started via `run_server()` in `http.rs`:

```rust
pub async fn run_server(host: &str, port: u16) -> Result<(), ServerRuntimeError>
```

This function:
1. Initializes the SQLite database pool
2. Loads configuration from `Config::load()`
3. Connects to all configured MCP servers
4. Creates `ServerState` with shared resources
5. Builds the Axum router with middleware stack
6. Binds to the specified address and starts serving

## HTTP Server Setup (http.rs)

### Middleware Stack

The Axum router applies middleware in this order (outermost first):

1. **Authentication Middleware** (`auth_middleware`)
   - Validates Bearer token from `Authorization` header
   - Checks env vars `CODEGG_SERVER_AUTH_DISABLED` and `CODEGG_SERVER_TOKEN`
   - Falls back to `server.token` from config
   - **Allows requests when no token is configured** (development-friendly)

2. **Rate Limit Middleware** (`rate_limit_middleware`)
   - 100 requests per 60-second window per IP address
   - Returns 429 with `Retry-After`, `X-RateLimit-*` headers

3. **Security Headers** (via `SetResponseHeaderLayer`)
   - `X-Content-Type-Options: nosniff`
   - `X-Frame-Options: DENY`
   - `Strict-Transport-Security: max-age=31536000; includeSubDomains`

4. **CORS Layer**
   - Configurable origins from `[server.cors]`
   - Defaults to `http://localhost:3000` and `http://127.0.0.1:3000`
   - Allows GET, POST, DELETE methods

5. **Compression Layer**
   - gzip and brotli compression
   - Skips compression for 401, 403, 404, 422, 500, 502, 503 responses

6. **Trace Layer** (for request logging)

### Router Structure

```
Router::new()
├── /health (GET) -> health_check  # No auth, no rate limit
└── /api (nested)
    ├── GET  /api/sessions
    ├── POST /api/sessions
    ├── GET  /api/sessions/:id
    ├── DELETE /api/sessions/:id/archive
    ├── POST /api/sessions/:id/fork
    ├── POST /api/sessions/:id/share
    ├── POST /api/sessions/:id/unshare
    ├── POST /api/sessions/:id/revert
    ├── POST /api/sessions/:id/unrevert
    ├── GET  /api/sessions/:id/messages
    ├── GET  /api/config
    ├── GET  /api/mcp
    ├── GET  /api/event            # SSE stream
    ├── GET  /api/question/:session_id
    ├── POST /api/question/:session_id
    ├── GET  /api/permission/:session_id
    ├── POST /api/permission/:session_id/submit
    ├── GET  /api/providers
    ├── GET  /api/tools
    ├── GET  /api/file/read?path=
    ├── GET  /api/file/list?path=
    ├── POST /api/file/write
    ├── DELETE /api/file/delete
    ├── GET  /api/project
    ├── POST /api/project
    ├── GET  /api/project/list
    ├── GET  /api/workspace
    ├── POST /api/workspace
    ├── GET  /api/workspace/list
    ├── GET  /ws                   # WebSocket for JSON-RPC
    └── GET  /tui                 # WebSocket for TuiMessage protocol
```

## WebSocket Handling (ws.rs)

### Two WebSocket Endpoints

#### `/ws` - JSON-RPC Interface

Used for lightweight RPC operations. Handles JSON-RPC 2.0 requests:

**Request format:**
```json
{"jsonrpc": "2.0", "id": 1, "method": "sessions.list", "params": {}}
```

**Supported methods:**
| Method | Description |
|--------|-------------|
| `sessions.list` | List last 50 sessions |
| `sessions.get` | Get session by ID (requires `id` param) |
| `sessions.create` | Create session (requires `directory` param) |
| `providers.list` | List all registered providers |
| `tools.list` | List all registered tools |

**Response format:**
```json
{"jsonrpc": "2.0", "id": 1, "result": {"sessions": [...]}, "error": null}
```

#### `/tui` - TuiMessage Protocol

The primary WebSocket for remote TUI communication. Handles bidirectional TuiMessage traffic.

**Client → Server Messages:**
| Message | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16`, `y: u16` | Mouse click events |
| `Resize` | `w: u16`, `h: u16` | Terminal resize |
| `Resume` | `from_event_seq: u64` | Client resume request for replay |
| `PermissionResponse` | `id: String`, `choice: String` | Permission decision (allow/deny/always_allow/always_deny) |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | Question answers |
| `SessionInfo` | `id: String`, `model: String` | Session metadata announcement |

**Server → Client Messages:**
| Message | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq: u64`, `payload: Box<TuiMessage>` | Sequence-tagged wrapper for replay |
| `TextDelta` | `delta: String` | Streaming text output |
| `RenderFrame` | `content: String` | Complete terminal frame content |
| `ToolCallStarted` | `tool_name`, `tool_id`, `arguments` | Tool execution started |
| `ToolResult` | `tool_id`, `output`, `success` | Tool execution completed |
| `PermissionPending` | `id`, `tool`, `path` | Pending permission request |
| `QuestionPending` | `id`, `questions: Vec<QuestionSpec>` | Pending question request |
| `SessionInfo` | `id`, `model` | Session metadata |
| `SessionEnded` | `stop_reason: String` | Agent finished |
| `Error` | `message: String` | Error message |
| `ResyncRequired` | `reason`, `pending_permissions`, `pending_questions` | Client sync required |

**Important Implementation Detail**: The server uses `TuiMessage::ResyncRequired` variant directly when serializing (not raw JSON). See `ws.rs` WebSocket handler for the implementation.

### Auth Validation

Both WebSocket endpoints share `validate_ws_auth()`:
1. Check `CODEGG_SERVER_AUTH_DISABLED` env var - skip auth if set
2. Extract Bearer token from Authorization header
3. Compare against `CODEGG_SERVER_TOKEN` env var using constant-time comparison
4. Return 401 Unauthorized if validation fails

## Replay Buffer

The `/tui` WebSocket maintains a bounded in-memory event buffer for client reconnection:

```rust
static TUI_EVENT_BUFFER: Lazy<StdMutex<VecDeque<(u64, TuiMessage)>>> =
    Lazy::new(|| StdMutex::new(VecDeque::with_capacity(1024)));
const TUI_EVENT_BUFFER_MAX: usize = 1024;
```

**Event Sequence Numbers:**
- Monotonically increasing 64-bit counter via `TUI_EVENT_SEQ`
- Assigned when events are recorded: `record_tui_event(event_seq, msg)`

**Replay Flow (Resume):**
1. Client sends `Resume { from_event_seq }` after reconnection
2. Server calls `replay_tui_events(from_event_seq)` to find buffered events where `seq > from_event_seq`
3. Events are wrapped in `EventEnvelope` and sent to client
4. Server then sends `ResyncRequired` with current pending permissions/questions

**Lagged Event Handling:**
- If `GlobalEventBus` receiver lags, server sends `ResyncRequired` with `reason: Some("lagged".to_string())`
- Pending permissions and questions are retrieved from `PermissionRegistry::pending_permission_ids()` and `QuestionRegistry::pending_question_ids()`

## ServerState (state.rs)

```rust
#[derive(Clone)]
pub struct ServerState {
    pub project_dir: String,              // Current project directory
    pub pool: SqlitePool,                 // SQLite connection pool
    pub mcp_service: Arc<RwLock<McpService>>, // MCP server connections
    pub config: Config,                   // Full configuration
    pub ws_rate_limiter: Arc<WsRateLimiter>,  // Shared WebSocket rate limiter
}
```

**Shared Rate Limiter for WebSocket:**
```rust
pub struct WsRateLimiter {
    cache: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,  // 100
    window: Duration,     // 60 seconds
}
```

**FromRef implementations** allow `ServerState` to be used with Axum's `State` extractor for routes that need `SqlitePool`, `Arc<RwLock<McpService>>`, or `Config`.

## Auth Middleware (middleware/auth.rs)

The auth middleware has specific behavior based on configuration:

```rust
pub async fn auth_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check 1: CODEGG_SERVER_AUTH_DISABLED env var
    let auth_disabled = std::env::var("CODEGG_SERVER_AUTH_DISABLED").is_ok();
    if auth_disabled {
        return Ok(next.run(request).await);
    }

    // Check 2: CODEGG_SERVER_TOKEN env var
    // Check 3: server.token config field
    let expected_token = std::env::var("CODEGG_SERVER_TOKEN").ok().or_else(|| {
        state.config.server.as_ref().and_then(|s| s.token.clone())
    });

    match expected_token {
        Some(expected) => {
            // Validate Bearer token with constant-time comparison
            let token = auth_header.and_then(|h| h.strip_prefix("Bearer "));
            match token {
                Some(provided) if validate_token(provided, &expected) => Ok(next.run(request).await),
                _ => Err(StatusCode::UNAUTHORIZED),
            }
        }
        None => {
            // IMPORTANT: When no token is configured, requests are ALLOWED
            // This is intentional for development mode
            Ok(next.run(request).await)
        }
    }
}
```

**Security Note**: When `CODEGG_SERVER_AUTH_DISABLED` is set or no token is configured, all requests are permitted. This is development-friendly but should be reviewed for production deployments.

## mDNS Service Discovery (mdns.rs)

Provides automatic server discovery on local networks via mDNS:

```rust
pub struct MdnsService {
    running: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<Arc<UdpSocket>>>>,
    service_name: String,   // "_opencode._tcp.local."
    port: u16,
    domain: String,         // "local." default
}

impl MdnsService {
    pub fn new(port: u16, domain: Option<String>) -> Self;
    pub async fn start(&self) -> Result<(), String>;
    pub fn stop(&self);
    pub fn is_running(&self) -> bool;
}
```

**Service Advertisement:**
- Service type: `_opencode._tcp.local.`
- Multicast address: `224.0.0.251:5353`
- Hostname: `codegg.{domain}` (default `codegg.local.`)
- TXT record includes port number

**Service Discovery:**
```rust
pub async fn discover_services(timeout_ms: u64) -> Vec<String>
```
Returns vector of `host:port` strings for discovered servers.

**Implementation Details:**
- Uses raw socket with `SO_REUSEADDR`
- Joins multicast group `224.0.0.251`
- Handles mDNS query parsing with DNS label compression support
- Returns service instances found within timeout

## REST API Routes

### Session Routes (routes/session.rs)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sessions` | List last 50 sessions |
| POST | `/api/sessions` | Create session |
| GET | `/api/sessions/:id` | Get session by ID |
| DELETE | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/fork` | Fork session |
| POST | `/api/sessions/:id/share` | Make session shareable |
| POST | `/api/sessions/:id/unshare` | Remove sharing |
| POST | `/api/sessions/:id/revert` | Revert to message |
| POST | `/api/sessions/:id/unrevert` | Undo revert |
| GET | `/api/sessions/:id/messages` | List messages (redacted) |

### Event Routes (routes/event.rs)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/event` | SSE event stream from GlobalEventBus |

SSE handler includes:
- 15-second heartbeat to keep connection alive
- Event filtering by type
- JSON serialization of `AppEvent` types

### Permission/Question Routes

**Permission (routes/permission.rs):**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/permission/:session_id` | Get pending permissions |
| POST | `/api/permission/:session_id/submit` | Submit permission response |

**Question (routes/question.rs):**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/question/:session_id` | Get pending questions |
| POST | `/api/question/:session_id` | Submit question answer |

**Important**: Both `PermissionRegistry` and `QuestionRegistry` do NOT store `session_id` in their keys. Keys are in format `{tool_call_id}-{tool_name}`, so `get_pending_*` functions cannot properly filter by session. They return empty lists when session_id is provided.

### File Routes (routes/file.rs)

File operations include path sanitization to prevent directory traversal:

```rust
pub fn sanitize_path(root: &str, requested: &str) -> Result<PathBuf, AppError>
```

**Security measures:**
1. Joins root and requested path
2. Checks for symlinks via `check_path_for_symlinks()`
3. Canonicalizes and verifies result starts with root
4. For non-existent paths, manually resolves `..` components
5. Returns error if path escapes root directory

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/file/read?path=` | Read file content |
| GET | `/api/file/list?path=` | List directory entries |
| POST | `/api/file/write` | Write file (creates parents) |
| DELETE | `/api/file/delete` | Delete file |

### Project/Workspace Routes

**Project (routes/project.rs):**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/project` | Get current project info |
| POST | `/api/project` | Create project |
| GET | `/api/project/list` | List all projects |

**Workspace (routes/workspace.rs):**
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/workspace` | Get current workspace info |
| POST | `/api/workspace` | Create workspace |
| GET | `/api/workspace/list` | List workspaces (includes git worktrees) |

## Configuration

**Config schema (`src/config/schema.rs`):**
```toml
[server]
host = "0.0.0.0"
port = 8080
token = "optional-token"

[server.cors]
origins = ["http://localhost:3000"]
```

**Environment variables:**
| Variable | Purpose |
|----------|---------|
| `CODEGG_SERVER_TOKEN` | Auth token (overrides config) |
| `CODEGG_SERVER_AUTH_DISABLED` | Disable auth entirely |

## Error Handling

**ServerRuntimeError enum:**
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

**HTTP status mapping:**
| Error | Status Code |
|-------|-------------|
| `Auth` | 401 Unauthorized |
| `Bind` | 500 Internal Server Error |
| `Shutdown` | 500 Internal Server Error |
| `WebSocket` | 500 Internal Server Error |
| `Rpc` | 500 Internal Server Error |

## Client Timeouts

- **Health check timeout**: 10 seconds (HTTP client `connect_timeout` in `src/client/sdk.rs`)
- **WebSocket connection timeout**: 30 seconds (tungstenite `connect_async` timeout in `src/client/attach.rs`)

These timeouts are configured client-side.

## See Also

- [client.md](client.md) - Remote TUI client, WebSocket connection, resume handshake
- [protocol.md](protocol.md) - CoreRequest/CoreResponse and TuiMessage protocol envelopes
- [bus.md](bus.md) - GlobalEventBus, PermissionRegistry, QuestionRegistry
- `.opencode/skills/server/SKILL.md` - Implementation guide
