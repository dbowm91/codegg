# Server Module - Agent Override

## Overview

The `server/` module provides the HTTP server using Axum with WebSocket support for remote TUI connections.

## WebSocket Handler (`src/server/ws.rs`)

### TUI WebSocket Upgrade

The `upgrade_tui` function handles WebSocket connections at `/tui`:

```rust
async fn upgrade_tui(
    ws: WebSocketUpgrade,
    State(state): State<Arc<Mutex<Option<TuiSessionState>>>>,
    // ...
) {
    // 1. Subscribe to GlobalEventBus
    // 2. Spawn recv_task (receives from client)
    // 3. Spawn event_task (sends to client via GlobalEventBus)
    // 4. Spawn send_task (relays responses to client)
}
```

### Key Tasks

| Task | Purpose |
|------|---------|
| `recv_task` | Receives `TuiMessage` from client, handles PermissionResponse/QuestionResponse |
| `event_task` | Subscribes to GlobalEventBus, converts `AppEvent` → `TuiMessage`, sends to client |
| `send_task` | Relays PermissionPending/QuestionPending to client via `bus_tx` |

### ResyncRequired Handling

When the event bus lags or WebSocket send fails for Permission/Question events, the server sends `ResyncRequired`:

```rust
// Lagged event bus
Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
    let resync_msg = TuiMessage::ResyncRequired {
        reason: Some("lagged".to_string()),
        pending_permissions: PermissionRegistry::pending_permission_ids(),
        pending_questions: QuestionRegistry::pending_question_ids(),
    };
    if let Ok(json) = serde_json::to_string(&resync_msg) {
        let _ = bus_tx_clone3.send(axum::extract::ws::Message::Text(json.into()));
    }
}
```

**Important**: Use the `TuiMessage::ResyncRequired` variant directly, not raw `serde_json::json!`. This ensures consistent serialization.

### Converting AppEvent to TuiMessage

The `convert_app_event` function maps internal events to protocol messages:

```rust
fn convert_app_event(event: AppEvent) -> Option<TuiMessage> {
    match event {
        AppEvent::TextDelta { delta, .. } => Some(TuiMessage::TextDelta { ... }),
        AppEvent::ToolCallStarted { ... } => Some(TuiMessage::ToolCallStarted { ... }),
        AppEvent::ReasoningDelta { .. } => None,  // Filtered out - not sent to client
        AppEvent::PermissionPending { ... } => Some(TuiMessage::PermissionPending { ... }),
        // ...
    }
}
```

## TuiSessionState

```rust
struct TuiSessionState {
    session_id: Option<String>,
    model: String,  // Default: "anthropic/claude-sonnet-4-20250514"
    rate_limit_key: String,
}
```

## Message Types

### TuiMessage (Protocol)

Defined in `src/protocol/tui.rs` with `#[serde(tag = "type")]` - variants serialized with `"type"` field.

**Client → Server**: Input, KeyDown, MouseClick, Resize, PermissionResponse, QuestionResponse, RenderFrame

**Server → Client**: TextDelta, ToolCallStarted, ToolResult, PermissionPending, QuestionPending, SessionInfo, SessionEnded, Error, ResyncRequired

## Error Handling

Use `if let Ok(json) = serde_json::to_string(&msg)` pattern instead of `.unwrap_or_default()`. Log errors appropriately before silently ignoring serialization failures.

## Client Timeouts

Server doesn't set timeouts on WebSocket connections - client is responsible for:
- Health check timeout (10s)
- WebSocket connection timeout (30s)
- Proper reconnection logic on disconnect

## Related Skills

- See `client/SKILL.md` for client-side handling
- See `event-bus/SKILL.md` for AppEvent types