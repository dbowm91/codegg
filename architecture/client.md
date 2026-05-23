# Client Module

The `client` module provides the WebSocket client for remote TUI connections.

## Overview

**Location**: `src/client/`

**Key Responsibilities**:
- WebSocket connection to server
- Remote TUI protocol
- Session attach/detach
- Server health checking

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

3. **WebSocket Connection** - 30-second timeout using `tokio_tungstenite::connect_async()`

4. **Channel Setup**:
   - `event_tx/rx` - Receives JSON events from server WebSocket → TUI
   - `out_tx/rx` - Sends TuiMessage from TUI → server WebSocket

5. **Two Background Tasks**:
   - `event_task` - Receives WebSocket messages, parses JSON, forwards to TUI
   - `send_task` - Receives `TuiMessage` from TUI, serializes to JSON, sends over WebSocket

6. **TUI Initialization** - Creates `tui::App::new_remote()` with event channels

7. **Cleanup** - Both tasks aborted when `run_event_loop()` returns

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

The client uses `TuiMessage` enum (from `src/protocol/tui.rs`) with `#[serde(tag = "type")]` for JSON serialization.

### Client → Server Messages (Input/Control)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16`, `y: u16` | Mouse clicks |
| `Resize` | `w: u16`, `h: u16` | Terminal resize |
| `RenderFrame` | `content: String` | Frame content (unused) |
| `PermissionResponse` | `id: String`, `choice: String` | Permission answer |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | Question answer |

### Server → Client Messages (Events)

| Variant | Fields | Purpose |
|---------|--------|---------|
| `TextDelta` | `delta: String` | Incremental text update |
| `PermissionPending` | `id: String`, `tool: String`, `path: Option<String>` | Permission request |
| `QuestionPending` | `id: String`, `questions: Vec<QuestionSpec>` | Question request |
| `SessionInfo` | `id: String`, `model: String` | Session metadata |
| `SessionEnded` | `stop_reason: String` | Session termination |
| `ToolCallStarted` | `tool_name: String`, `tool_id: String`, `arguments: String` | Tool execution started |
| `ToolResult` | `tool_id: String`, `output: String`, `success: bool` | Tool execution result |
| `Error` | `message: String` | Error message |
| `ResyncRequired` | `reason: Option<String>`, `pending_permissions: Vec<String>`, `pending_questions: Vec<String>` | Client re-sync needed |

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
- `src/protocol/tui.rs` - TuiMessage protocol definitions
- `.opencode/skills/client/SKILL.md` - Detailed implementation guide