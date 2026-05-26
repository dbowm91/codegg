# Server Module

The `server` module provides HTTP server functionality for remote TUI connections.

## Overview

**Location**: `src/server/`

**Key Responsibilities**:
- Axum-based HTTP server with WebSocket support
- REST API for sessions, config, MCP, files, projects, workspaces
- SSE event streaming for real-time updates
- Token-based authentication
- Rate limiting (HTTP and WebSocket)

**Requires**: `server` feature flag

## Entry Point

```rust
pub async fn run_server(host: &str, port: u16) -> Result<(), ServerRuntimeError>
```

## Components

### mod.rs

Module exports:
```rust
pub use http::run_server;
pub use mdns::{discover_services, MdnsService};
pub use state::ServerState;
```

### http.rs - Server Setup

Main server initialization:
1. Initializes SQLite database pool
2. Loads configuration
3. Connects to configured MCP servers
4. Creates `ServerState` with shared `WsRateLimiter`
5. Builds Axum router with all routes and middleware

Router includes:
- CORS middleware (configurable origins)
- Auth middleware (token validation)
- Rate limit middleware (100 req/60s per IP)
- Compression middleware (gzip/br)
- Security headers (X-Content-Type-Options, X-Frame-Options, HSTS)
- Trace logging

### state.rs - ServerState

```rust
pub struct ServerState {
    pub project_dir: String,
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,  // Shared across WebSockets
}

pub struct WsRateLimiter {
    cache: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}
```

### ws.rs - WebSocket Handlers

Two WebSocket endpoints:

**`/ws`** - JSON-RPC interface:
```rust
// Request
{"jsonrpc": "2.0", "id": 1, "method": "sessions.list", "params": {}}

// Response
{"jsonrpc": "2.0", "id": 1, "result": {"sessions": [...]}}
```

Supported methods:
- `sessions.list` - List sessions
- `sessions.get` - Get session by ID
- `sessions.create` - Create new session
- `providers.list` - List providers
- `tools.list` - List tools

**`/tui`** - TuiMessage protocol:
- Uses `TuiMessage` enum from `src/protocol/tui.rs`
- Handles `Input`, `KeyDown`, `MouseClick`, `Resize`, `Resume`, `PermissionResponse`, `QuestionResponse`, `SessionInfo`
- Converts `AppEvent` from `GlobalEventBus` to `TuiMessage` payloads, then wraps them in `EventEnvelope` for server→client streaming

Auth validation via `validate_ws_auth()` shared between both handlers.

### routes/ - REST API

#### Session Routes (`/api/sessions`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sessions` | List sessions |
| POST | `/api/sessions` | Create session |
| GET | `/api/sessions/:id` | Get session |
| DELETE | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/fork` | Fork session |
| POST | `/api/sessions/:id/share` | Share session |
| POST | `/api/sessions/:id/unshare` | Unshare session |
| POST | `/api/sessions/:id/revert` | Revert to message |
| POST | `/api/sessions/:id/unrevert` | Unrevert session |
| GET | `/api/sessions/:id/messages` | List messages |

#### Config Routes (`/api/config`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/config` | Get config (API keys redacted) |

#### MCP Routes (`/api/mcp`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/mcp` | List MCP servers |

#### Event Routes (`/api/event`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/event` | SSE event stream |

#### Permission Routes (`/api/permission/:session_id`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/permission/:session_id` | Get pending permissions |
| POST | `/api/permission/:session_id/submit` | Submit permission |

#### Question Routes (`/api/question/:session_id`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/question/:session_id` | Get pending questions |
| POST | `/api/question/:session_id` | Submit question answer |

#### Provider/Tool Routes
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/providers` | List providers |
| GET | `/api/tools` | List tools |

#### File Routes (`/api/file`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/file/read?path=` | Read file |
| GET | `/api/file/list?path=` | List directory |
| POST | `/api/file/write` | Write file |
| DELETE | `/api/file/delete` | Delete file |

#### Project Routes (`/api/project`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/project` | Get project info |
| POST | `/api/project` | Create project |
| GET | `/api/project/list` | List projects |

#### Workspace Routes (`/api/workspace`)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/workspace` | Get workspace info |
| POST | `/api/workspace` | Create workspace |
| GET | `/api/workspace/list` | List workspaces |

### middleware/auth.rs - Authentication

Token validation for all `/api/*` routes:
```rust
pub async fn auth_middleware(...) -> Result<Response, StatusCode> {
    // Checks in order:
    // 1. CODEGG_SERVER_AUTH_DISABLED env var (skip auth)
    // 2. CODEGG_SERVER_TOKEN env var
    // 3. server.token config field
    // 4. Allow if no token configured (no auth enforcement)
}
```

**Note**: When no token is configured, requests are **allowed** through without authentication. This is intentional for development mode. Set a token in production.

### routes/file.rs - Path Sanitization

The `sanitize_path()` function ensures file operations stay within allowed directories:
1. Joins root and requested path
2. Checks for symlinks in the path via `check_path_for_symlinks()`
3. Canonicalizes and verifies result starts with root
4. For non-existent paths, manually resolves `..` components

### routes/event.rs - SSE Handler

The SSE handler at `/api/event` subscribes directly to `GlobalEventBus::subscribe()` and streams events to connected clients.

### TUI Replay Buffer

The `/tui` WebSocket maintains a bounded event buffer and assigns a monotonically increasing sequence number to each outbound event. When a client sends `Resume { from_event_seq }`, the server replays buffered events with a higher sequence number before sending `ResyncRequired`.

### Client SSE Methods

Client SSE connection methods are documented in `architecture/mcp.md`. The `RemoteClient` in `src/mcp/remote.rs` provides `connect_sse()`, `connect_sse_stream()`, and `take_sse_events()` for MCP server connections.

## Protocol

### TuiMessage (from `src/protocol/tui.rs`)

**Client → Server**:
| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16`, `y: u16` | Mouse clicks |
| `Resize` | `w: u16`, `h: u16` | Terminal resize |
| `Resume` | `from_event_seq: u64` | Resume handshake for buffered event replay |
| `PermissionResponse` | `id: String`, `choice: String` | Permission answer |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | Question answer |
| `SessionInfo` | `id: String`, `model: String` | Session metadata |

**Server → Client**:
| Variant | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq: u64`, `payload: Box<TuiMessage>` | Sequence-tagged wrapper for replayable TUI events |
| `TextDelta` | `delta: String` | Streaming text |
| `RenderFrame` | `content: String` | Rendered UI frame content |
| `ToolCallStarted` | `tool_name`, `tool_id`, `arguments` | Tool execution |
| `ToolResult` | `tool_id`, `output`, `success` | Tool result |
| `PermissionPending` | `id`, `tool`, `path` | Permission request |
| `QuestionPending` | `id`, `questions: Vec<QuestionSpec>` | Question request |
| `SessionInfo` | `id`, `model` | Session metadata |
| `SessionEnded` | `stop_reason` | Agent finished |
| `Error` | `message` | Error message |
| `ResyncRequired` | `reason`, `pending_permissions`, `pending_questions` | Resync needed |

## Configuration

```toml
[server]
host = "0.0.0.0"
port = 8080
token = "..."  # or use CODEGG_SERVER_TOKEN env var

[server.cors]
origins = ["http://localhost:3000", "http://127.0.0.1:3000"]
```

Environment variables:
- `CODEGG_SERVER_TOKEN` - Auth token
- `CODEGG_SERVER_AUTH_DISABLED` - Disable auth (not recommended)

## Error Handling

```rust
pub enum ServerRuntimeError {
    Bind(String),        // Failed to bind address
    Shutdown(String),   // Server shutdown error
    WebSocket(String),  // WebSocket connection error
    Rpc(String),        // JSON-RPC error
    Auth(String),       // Authentication failed
}
```

## See Also

- [client.md](client.md) - Remote TUI client
- `.skills/server/SKILL.md` - Detailed implementation guide
- `src/protocol/tui.rs` - TuiMessage protocol definitions
