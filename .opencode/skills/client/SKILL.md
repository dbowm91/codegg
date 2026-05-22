---
name: client
description: Remote TUI client for WebSocket connections to codegg server
version: 1.0.0
tags: [client, remote, websocket, tui]
---

# Client Module Guide

This skill covers the `client/` module which provides remote TUI functionality via WebSocket connections to a codegg server.

## Overview

The client module (`src/client/`) enables users to connect their local terminal to a remote codegg server. It renders the TUI locally while maintaining a WebSocket connection for bidirectional communication.

## Files

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | 4 | Re-exports `run_attach` function |
| `attach.rs` | 118 | Main WebSocket connection logic with timeouts |
| `sdk.rs` | 44 | HTTP client for server health checks with timeouts |

## Entry Point

```rust
pub async fn run_attach(url: &str, token: Option<&str>) -> Result<(), ClientError>
```

Called via CLI: `codegg attach <url> --token <token>`

## Connection Flow (`attach.rs`)

### 1. URL Normalization
```rust
let ws_url = build_tui_ws_url(url);   // HTTP/HTTPS → WSS/WS + /tui
let http_url = build_http_url(url);   // WS/WSS → HTTP/HTTPS
```

### 2. Health Check (with 10s timeout)
```rust
let client = RemoteClient::new(&http_url, token)?;
client.health().await?;  // Uses /health endpoint
```

### 3. WebSocket Connection (with 30s timeout)
```rust
let ws_stream = match timeout(Duration::from_secs(30), connect_async(ws_request)).await {
    Ok(Ok((stream, _))) => stream,
    Ok(Err(e)) => return Err(ClientError::WebSocket(e.to_string())),
    Err(_) => return Err(ClientError::Connection("WebSocket connection timed out".to_string())),
};
```

**Important**: Always extract both values from the tuple: `(stream, _)`. The `_` is the HTTP response and must not be ignored in pattern matching.

### 4. Three Concurrent Tasks

```
┌─────────────────────────────────────────────────────────────┐
│                    WebSocket /tui                          │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  event_task (ws_rx)                                   │  │
│  │  - Receives TuiMessage JSON from server                │  │
│  │  - Parses and sends to event_tx channel                │  │
│  │  - Logs malformed messages with tracing::warn!         │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  send_task (out_rx)                                   │  │
│  │  - Receives TuiMessage from out_tx                    │  │
│  │  - Serializes to JSON and sends over WebSocket        │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      App (new_remote)                       │
│  src/tui/app/mod.rs:489                                    │
│                                                             │
│  • handle_remote_event() - processes server messages        │
│  • set_remote_send_tx() - sends user actions to server      │
│  • remote_mode = true                                       │
└─────────────────────────────────────────────────────────────┘
```

## RemoteClient HTTP Wrapper (`sdk.rs`)

```rust
pub struct RemoteClient {
    base_url: String,
    http: Client,  // reqwest client with timeout configured
}

impl RemoteClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError>

    pub async fn health(&self) -> Result<bool, ClientError> {
        // Uses GET /health with 10s timeout
        let url = format!("{}/health", self.base_url);
        let resp = self.http.get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(ClientError::Unreachable)?;
        if resp.status().is_success() {
            Ok(true)
        } else {
            Err(ClientError::Unreachable(format!(
                "health check failed: {}",
                resp.status()
            )))
        }
    }
}
```

## TuiMessage Protocol

Defined in `src/protocol/tui.rs`:

### Client → Server (Input)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key: String`, `modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16`, `y: u16` | Mouse clicks |
| `Resize` | `w: u16`, `h: u16` | Terminal resize |
| `PermissionResponse` | `id: String`, `choice: String` | Permission decision |
| `QuestionResponse` | `id: String`, `answers: serde_json::Value` | Question answers |

### Server → Client (Output)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `TextDelta` | `delta: String` | Streaming text |
| `ToolCallStarted` | `tool_name`, `tool_id`, `arguments` | Tool execution started |
| `ToolResult` | `tool_id`, `output`, `success` | Tool execution completed |
| `PermissionPending` | `id`, `tool`, `path` | Request permission |
| `QuestionPending` | `id`, `questions: Vec<QuestionSpec>` | Ask user question |
| `SessionInfo` | `id`, `model` | Session metadata |
| `SessionEnded` | `stop_reason: String` | Agent finished |
| `Error` | `message: String` | Error message |
| `ResyncRequired` | `reason`, `pending_permissions`, `pending_questions` | Resync needed |

## TUI Integration

### Remote App Initialization (`src/tui/app/mod.rs:489`)
```rust
pub fn new_remote(project_dir: String) -> Self {
    let mut app = Self::new(project_dir);
    app.ui_state.remote_mode = true;
    app.ui_state.remote_status = Some("Connected".to_string());
    app.remote_event_rx = None;
    app.remote_send_tx = None;
    app
}
```

### Remote Event Handling (`src/tui/app/mod.rs:682`)
```rust
pub fn handle_remote_event(&mut self, event: serde_json::Value) {
    match serde_json::from_value::<RemoteTuiMessage>(event) {
        Ok(RemoteTuiMessage::TextDelta { delta }) => { ... }
        Ok(RemoteTuiMessage::ToolCallStarted { ... }) => { ... }
        Ok(RemoteTuiMessage::ResyncRequired { reason, pending_permissions, pending_questions }) => {
            // Shows toast warning, logs full details
        }
        // ... other handlers
        _ => { /* unhandled */ }
    }
}
```

**Important**: When adding new `TuiMessage` variants, add handling in `handle_remote_event()` or they will be silently ignored.

## ClientError Enum (`src/error.rs`)

```rust
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("server not reachable: {0}")]
    Unreachable(String),

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("authentication failed: {0}")]
    Auth(String),
}
```

## Known Issues & Implementation Notes

### ResyncRequired Handling
- Server sends `ResyncRequired` when event bus lags or WebSocket send fails
- Client handles this by showing a toast warning with pending counts
- Always log the full details for debugging

### URL Building Edge Cases
The URL builders handle:
- `wss://` / `ws://` - pass through
- `https://` → `wss://`
- `http://` → `ws://`
- No scheme → just append `/tui`

But doesn't handle unix sockets or custom schemes.

### Timeouts Are Essential
Both health check (10s) and WebSocket connection (30s) have timeouts. Without them, unreachable servers will cause indefinite hangs.

## Related Skills

- See `.opencode/skills/event-bus/SKILL.md` for event types
- See `.opencode/skills/tui/SKILL.md` for TUI event handling
- See `.opencode/skills/provider/SKILL.md` for provider registration