---
name: server
description: HTTP/WebSocket server for remote TUI connections with Axum
version: 1.1.0
tags: [server, http, websocket, rest-api, sse]
---

# Server Module Guide

This skill covers the `server/` module which provides HTTP server functionality for remote TUI connections.

## Overview

The server module (`src/server/`) provides an Axum-based HTTP server with WebSocket support for remote TUI connections.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Module exports - `run_server`, `ServerState`, `discover_services`, `MdnsService` |
| `http.rs` | Main server setup with Axum router, middleware, CORS, rate limiting, all API routes |
| `ws.rs` | WebSocket handling for `/ws` and `/tui` endpoints, RPC request processing, TUI message handling |
| `state.rs` | `ServerState` struct with shared state including `WsRateLimiter` (note: `event_bus` field was removed - SSE uses global `GlobalEventBus`) |
| `rpc.rs` | RPC message structures: `RpcRequest`, `RpcResponse`, `RpcError` (for `/ws` JSON-RPC), plus `JsonRpcMessage`/`JsonRpcError` legacy types |
| `mdns.rs` | mDNS service discovery implementation |
| `middleware/mod.rs` | Auth middleware module |
| `middleware/auth.rs` | Token validation middleware |
| `routes/mod.rs` | Re-exports all route handlers |
| `routes/session.rs` | Session CRUD, archive, fork, share, revert endpoints |
| `routes/config.rs` | Config retrieval (with API key redaction), message listing |
| `routes/provider.rs` | Provider registry listing |
| `routes/tool.rs` | Tool registry listing |
| `routes/event.rs` | SSE endpoint for event streaming |
| `routes/file.rs` | File CRUD with symlink protection and path sanitization |
| `routes/project.rs` | Project info and listing |
| `routes/workspace.rs` | Workspace management with git worktree detection |
| `routes/mcp.rs` | MCP server status listing |
| `routes/permission.rs` | Permission submission and pending queries |
| `routes/question.rs` | Question submission and pending queries |
| `routes/health.rs` | Health check route (standalone, not wired to main router) |

## Entry Point

```rust
pub async fn run_server(host: &str, port: u16) -> Result<(), ServerRuntimeError>
```

**Phase 1 (singleton daemon)**: The server requires `--standalone-core` to construct its own daemon. Without it, the server exits with an actionable error rather than silently creating a second core that defeats the singleton invariant. Daemon-proxying server mode (where the server forwards to the user-scoped singleton daemon) lands in a later phase.

## ServerState

```rust
pub struct ServerState {
    pub project_dir: String,
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,
}
```

Key points:
- `ws_rate_limiter` is shared across all WebSocket connections (not created per-connection)
- Note: `event_bus` field was removed - SSE handler (`/api/event`) and TUI WebSocket (`/tui`) now directly use `GlobalEventBus::subscribe()` from `crate::bus::global`
- MCP service is wrapped in `Arc<RwLock<>>` for concurrent access

## HTTP Routes

### Session Routes (`/api/sessions`)
- `GET /api/sessions` - List sessions
- `POST /api/sessions` - Create session
- `GET /api/sessions/:id` - Get session
- `DELETE /api/sessions/:id/archive` - Archive session
- `POST /api/sessions/:id/fork` - Fork session
- `POST /api/sessions/:id/share` - Share session
- `POST /api/sessions/:id/unshare` - Unshare session
- `POST /api/sessions/:id/revert` - Revert session
- `POST /api/sessions/:id/unrevert` - Unrevert session
- `GET /api/sessions/:id/messages` - List messages

### Config Routes (`/api/config`)
- `GET /api/config` - Get config (redacted API keys)

### MCP Routes (`/api/mcp`)
- `GET /api/mcp` - List MCP servers

### Event Routes (`/api/event`)
- `GET /api/event` - SSE event stream

### Permission Routes (`/api/permission/:session_id`)
- `GET /api/permission/:session_id` - Get pending permissions
- `POST /api/permission/:session_id/submit` - Submit permission

### Question Routes (`/api/question/:session_id`)
- `GET /api/question/:session_id` - Get pending questions
- `POST /api/question/:session_id` - Submit question answer

### Provider/Tool Routes
- `GET /api/providers` - List providers
- `GET /api/tools` - List tools

### File Routes (`/api/file`)
- `GET /api/file/read?path=` - Read file
- `GET /api/file/list?path=` - List directory
- `POST /api/file/write` - Write file
- `DELETE /api/file/delete` - Delete file

### Project Routes (`/api/project`)
- `GET /api/project` - Get project info
- `POST /api/project` - Create project
- `GET /api/project/list` - List projects

### Workspace Routes (`/api/workspace`)
- `GET /api/workspace` - Get workspace info
- `POST /api/workspace` - Create workspace
- `GET /api/workspace/list` - List workspaces

### WebSocket Routes
- `GET /ws` - WebSocket RPC endpoint (JSON-RPC)
- `GET /tui` - WebSocket TUI endpoint (TuiMessage protocol)

### Health Route
- `GET /health` - Health check (inline handler)

## WebSocket Protocol

### `/ws` - JSON-RPC
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

### `/tui` - TuiMessage Protocol

Uses `TuiMessage` enum from `src/protocol/tui.rs`:

**Client → Server**:
- `Input { text }` - User text input
- `KeyDown { key, modifiers }` - Keyboard events
- `MouseClick { x, y }` - Mouse clicks
- `Resize { w, h }` - Terminal resize
- `Resume { from_event_seq }` - Resume handshake for buffered event replay
- `PermissionResponse { id, choice }` - Permission answer
- `QuestionResponse { id, answers }` - Question answer
- `SessionInfo { id, model }` - Session metadata

**Server → Client**:
- `EventEnvelope { event_seq, payload }` - Sequence-tagged wrapper for replayable TUI events
- `TextDelta { delta }` - Streaming text
- `ToolCallStarted { tool_name, tool_id, arguments }` - Tool execution started
- `ToolResult { tool_id, output, success }` - Tool execution completed
- `PermissionPending { id, tool, path }` - Request permission
- `QuestionPending { id, questions }` - Ask user question
- `SessionInfo { id, model }` - Session metadata
- `SessionEnded { stop_reason }` - Agent finished
- `Error { message }` - Error message
- `ResyncRequired { reason, pending_permissions, pending_questions }` - Resync needed

## Authentication

Server uses token-based authentication via `Authorization: Bearer <token>` header.

Configuration (in order of precedence):
1. Environment variable: `CODEGG_SERVER_TOKEN`
2. Config file: `server.token`
3. If not configured and `CODEGG_SERVER_AUTH_DISABLED` not set, requests are rejected

Auth middleware applies to all `/api/*` routes. WebSocket endpoints (`/ws`, `/tui`) have their own auth validation in `validate_ws_auth()`.

## Rate Limiting

### HTTP Rate Limiter
- 100 requests per 60 seconds per IP
- Returns 429 with `Retry-After` header
- Uses in-memory HashMap with per-key sliding window

### WebSocket Rate Limiter
- Shared `WsRateLimiter` in `ServerState`
- 100 requests per 60 seconds per connection (keyed by IP or session ID)
- Returns error message on the WebSocket itself

## Path Sanitization (`routes/file.rs`)

The `sanitize_path()` function ensures file operations stay within allowed directories:
1. Joins root and requested path
2. Checks for symlinks in the path via `check_path_for_symlinks()`
3. Canonicalizes and verifies result starts with root
4. For non-existent paths, manually resolves `..` components to prevent traversal

## SSE Event Stream

The `/api/event` endpoint provides Server-Sent Events:
- Subscribes to `GlobalEventBus`
- Streams `AppEvent` as JSON with `event:` prefix
- Includes 15-second heartbeat comments
- Sends `ResyncRequired` on lag or send failures

### TUI Replay Buffer

The `/tui` WebSocket keeps a bounded event buffer and assigns sequence numbers to outbound events. When a client sends `Resume { from_event_seq }`, the server replays buffered events above that sequence before sending `ResyncRequired`.

## Implementation Notes

### WsRateLimiter is Shared
The `WsRateLimiter` in `ServerState` is shared across all WebSocket connections. Previously, a new `RateLimiter` was created per connection, which was inefficient.

### SSE Uses GlobalEventBus Directly
`routes/event.rs` SSE handler uses `GlobalEventBus::subscribe()` directly from `crate::bus::global`. There is no local EventBus struct.

### Health Route Standalone
`routes/health.rs` contains a standalone `health_check()` function. The main router uses an inline `async fn health_check()` at the top level.

### No TLS Implementation
Although config supports TLS section, no TLS handling exists in the code.

## ServerRuntimeError

```rust
pub enum ServerRuntimeError {
    Bind(String),
    Shutdown(String),
}
```

## See Also

- [architecture/server.md](../architecture/server.md) - Architecture overview
- `.skills/client/SKILL.md` - Client module for remote TUI
- `.skills/tui/SKILL.md` - TUI module with remote protocol details
- `.skills/event-bus/SKILL.md` - GlobalEventBus for SSE

## Remote TUI Protocol (Phase 8)

The `/tui` WebSocket endpoint implements an event/state-driven protocol for remote TUI connections.

### Client → Server Messages

| Message | Handling |
|---------|----------|
| `Resume { from_event_seq }` | Replays events from EventLog starting at the given sequence |
| `RequestSnapshot { reason }` | Replays all events from sequence 0 + sends `ResyncRequired` |
| `Input`, `KeyDown`, `MouseClick`, `Resize` | Logging stubs (not yet forwarded to App) |
| `PermissionResponse` | Responds via `PermissionRegistry` |
| `QuestionResponse` | Responds via `QuestionRegistry` |
| `SessionInfo` | Stores session metadata for rate limiting |

### Server → Client Messages

| Message | Source |
|---------|--------|
| `EventEnvelope { event_seq, payload }` | Wraps all events from CoreEvent broadcast |
| `ResyncRequired { reason, pending_permissions, pending_questions }` | Sent on broadcast lag, Resume, or RequestSnapshot |
| `Error { message }` | Rate limit exceeded, RenderFrame rejection |

### Key Implementation Details

- `convert_core_event_to_tui()` maps `CoreEvent` → `TuiMessage` for replay
- Broadcast channel lag triggers automatic `ResyncRequired`
- Rate limiting per session via `TuiSessionState`
- Server does NOT maintain TUI state — clients reconstruct from event replay
