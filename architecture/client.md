# Client Module

The `client` module provides the WebSocket client for remote TUI connections.

## Overview

**Location**: `src/client/`

**Key Responsibilities**:
- WebSocket connection to server
- Remote TUI protocol
- Session attach/detach
- Server health checking

Both the remote TUI client (`src/client/`) and the local `SocketCoreClient` connect through the user-scoped singleton daemon. The canonical entry point is `connect_or_start_daemon` (`src/core/instance.rs`), which connects to the running daemon or auto-starts one. The `SnapshotDaemon` request/response surfaces `daemon_id`, `uptime_secs`, `active_sessions`, and `connected_clients`; the `daemon status` CLI prints `generation` and `started_at` from the on-disk metadata file.

## Components

### mod.rs

Module exports - re-exports `run_attach` as the public API.

```rust
pub use attach::run_attach;
```

### attach.rs

Main entry point for remote TUI attachment. Handles URL normalization, health checks, WebSocket connection, and TUI event loop.

```rust
pub async fn run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>
```

**Connection Flow**:

1. **URL Building** - Converts HTTP/HTTPS/WS/WSS URLs to appropriate endpoints:
   - `build_tui_ws_url()` - converts base URL to `{base}/tui` WebSocket endpoint
   - `build_http_url()` - converts WS/WSS to HTTP/HTTPS

2. **Health Check** - Creates `RemoteClient` and calls `health()` to verify server connectivity (10s timeout)

3. **WebSocket Connection** - 30-second timeout per attempt, up to 3 retries with exponential backoff (1s, 2s, 4s) using `tokio_tungstenite::connect_async()`

4. **Resume Handshake** - Immediately sends `TuiMessage::Resume { from_event_seq: 0 }` after connect so the server can replay buffered events when needed.

5. **Channel Setup**:
   - `event_tx/rx` - Receives JSON events from server WebSocket → TUI
   - `out_tx/rx` - Sends TuiMessage from TUI → server WebSocket
   - Event handling uses `catch_unwind` to prevent panics in spawned tasks from crashing the connection

6. **Two Background Tasks**:
   - `event_task` - Receives WebSocket messages, parses JSON, forwards to TUI
   - `send_task` - Receives `TuiMessage` from TUI, serializes to JSON, sends over WebSocket

7. **TUI Initialization** - Creates `tui::App::new_remote()` with event channels

8. **Cleanup** - Both tasks aborted when `run_event_loop()` returns

### sdk.rs

HTTP client SDK for server communication (health checks, API access).

```rust
pub struct RemoteClient {
    base_url: String,
    http: Client,
}

impl RemoteClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError>
    pub async fn health(&self) -> Result<bool, ClientError>
}
```

**Health Check**: Uses `GET /health` with 10-second timeout. Returns `Err(ClientError::Unreachable)` on non-success status or connection failure.

## Protocol

The client uses `TuiMessage` enum (from `crates/codegg-protocol/src/tui.rs`) with `#[serde(tag = "type")]` for JSON serialization.

### Client → Server Messages (Input/Control)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16`, `y: u16` | Mouse clicks |
| `Resize` | `w: u16`, `h: u16` | Terminal resize |
| `Resume` | `from_event_seq: u64` | Resume handshake for replayed server events |
| `RenderFrame` | `content: String` | ❌ unsupported — returns `Error` with code `unsupported_render_frame` |
| `StateSnapshot` | `snapshot: RemoteTuiStateSnapshot` | Full state snapshot (server→client, on reconnect or request) |
| `RequestSnapshot` | - | Request a full state snapshot from the daemon |
| `PermissionResponse` | `id: String`, `choice: String` | Permission answer |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | Question answer |

### Server → Client Messages (Events)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq: u64`, `payload: Box<TuiMessage>` | Sequence-tagged wrapper for replayable server events |
| `TextDelta` | `delta: String` | Incremental text update |
| `PermissionPending` | `id: String`, `tool: String`, `path: Option<String>` | Permission request |
| `QuestionPending` | `id: String`, `questions: Vec<QuestionSpec>` | Question request |
| `SessionInfo` | `id: String`, `model: String` | Session metadata |
| `SessionEnded` | `stop_reason: String` | Session termination |
| `ToolCallStarted` | `tool_name: String`, `tool_id: String`, `arguments: String` | Tool execution started |
| `ToolResult` | `tool_id: String`, `output: String`, `success: bool` | Tool execution result |
| `Error` | `message: String` | Error message |
| `ResyncRequired` | `reason: Option<String>`, `pending_permissions: Vec<String>`, `pending_questions: Vec<String>` | Client re-sync needed |

`App::handle_remote_event()` (in `src/tui/app/mod.rs:805`) unwraps `EventEnvelope` first and then dispatches the inner payload, so replayed events and live events share the same handler path.

## Error Handling

```rust
pub enum ClientError {
    Connection(String),    // General connection failures
    Unreachable(String),  // Server not reachable (used for health check failures)
    Rpc(String),          // RPC errors
    WebSocket(String),    // WebSocket-specific errors
    Auth(String),         // Authentication failures
}
```

## See Also

- [server.md](server.md) - Server that accepts connections
- `crates/codegg-protocol/src/tui.rs` - TuiMessage protocol definitions
- [tui.md](tui.md) - TUI and remote-client integration notes
