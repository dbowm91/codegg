# Client Module

## Purpose

WebSocket client for remote TUI connections. Handles URL normalization,
health checks, retry logic, WebSocket establishment, projection
capability negotiation, and bidirectional TuiMessage event loop.

## Where It Lives

| Path | Role |
|------|------|
| `src/client/mod.rs` | Re-exports `run_attach` |
| `src/client/attach.rs` | Main entry: health check, WS connect, event loop |
| `src/client/sdk.rs` | `RemoteClient` — HTTP client for health/API |
| `crates/codegg-protocol/src/tui.rs` | `TuiMessage`, `REMOTE_TUI_PROTOCOL_VERSION` |

## How It Works

### Entry Point

```rust
// src/client/attach.rs:17
pub async fn run_attach(
    url: &str,
    token: Option<&str>,
) -> Result<(), ClientError>
```

### Connection Flow

1. **URL Building**
   - `build_tui_ws_url()` (`attach.rs:148`): Converts HTTP/HTTPS/WS/WSS
     to the `/tui` WebSocket endpoint.
   - `build_http_url()` (`attach.rs:161`): Converts WS/WSS to HTTP/HTTPS
     for the health check.

2. **Health Check** (`sdk.rs:35`)
   - `RemoteClient::health()` → `GET /health` with 10s timeout.
   - Returns `Err(ClientError::Unreachable)` on non-success.

3. **WebSocket Connection** (`attach.rs:37-73`)
   - 30-second timeout per attempt.
   - Up to 3 retries with exponential backoff (1s, 2s, 4s).
   - Uses `tokio_tungstenite::connect_async()`.

4. **Capability Handshake** (`attach.rs:82-91`)
   - Sends `TuiMessage::ProjectionCapabilities` with current capabilities
     first.
   - Sends `TuiMessage::Resume { from_event_seq: 0 }` as a bounded
     raw-compatibility fallback.
   - Once a projection subscription exists, reconnects use
     `ProjectionResume` with the persisted `ProjectionCursor`.

5. **Channel Setup** (`attach.rs:95-101`)
   - `event_tx/rx`: server WS → TUI (256 capacity)
   - `out_tx/rx`: TUI → server WS (256 capacity)

6. **Background Tasks** (`attach.rs:103-138`)
   - `event_task`: receives WS messages, parses JSON, forwards to TUI
   - `send_task`: receives `TuiMessage` from TUI, serializes, sends WS

7. **TUI Initialization** (`attach.rs:93`)
   - `tui::App::new_remote()` with event channels.

8. **Cleanup** (`attach.rs:142-143`)
   - Both tasks aborted when `run_event_loop()` returns.

### Daemon Integration

Both the remote TUI client (`src/client/`) and the local
`SocketCoreClient` connect through the user-scoped singleton daemon.
The canonical entry point is `connect_or_start_daemon`
(`src/core/instance.rs`), which performs a complete bounded
handshake/identity probe before reusing a daemon or auto-starting one.

- A peer EOF fails outstanding socket requests.
- Reconnects establish a fresh handshake.
- `SnapshotDaemon` surfaces `daemon_id`, `uptime_secs`,
  `active_sessions`, `connected_clients`.
- `daemon status` CLI prints `generation` and `started_at`.

## Key Types & APIs

### RemoteClient

```rust
// src/client/sdk.rs:7
pub struct RemoteClient {
    base_url: String,
    http: Client,
}

impl RemoteClient {
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self, ClientError>;
    pub async fn health(&self) -> Result<bool, ClientError>;
}
```

### ClientError

```rust
// src/error.rs
pub enum ClientError {
    Connection(String),
    Unreachable(String),
    Rpc(String),
    WebSocket(String),
    Auth(String),
}
```

### Protocol

```rust
// crates/codegg-protocol/src/tui.rs:14
pub const REMOTE_TUI_PROTOCOL_VERSION: u32 = 5;

// crates/codegg-protocol/src/tui.rs:19
pub enum TuiMessage { ... }
```

`TuiMessage` uses `#[serde(tag = "type")]` for JSON wire format.

### Client → Server Messages

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text: String` | User text input |
| `KeyDown` | `key, modifiers` | Keyboard events |
| `MouseClick` | `x, y` | Mouse clicks |
| `Resize` | `w, h` | Terminal resize |
| `Resume` | `from_event_seq: u64` | Resume handshake |
| `RequestSnapshot` | — | Request full state snapshot |
| `PermissionResponse` | `id, choice` | Permission answer |
| `QuestionResponse` | `id, answers` | Question answer |
| `SessionInfo` | `id, model` | Session metadata |
| `ProjectionCapabilities` | `capabilities` | Negotiate projection mode |
| `ProjectionSubscribe` | `stream_id, ...` | Subscribe to projection |
| `ProjectionResume` | `cursor, ...` | Resume with cursor |
| `ProjectionAck` | `subscription_id, seq` | Acknowledge events |

### Server → Client Messages

| Variant | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq, payload` | Sequence-tagged for replay |
| `TextDelta` | `delta` | Streaming text output |
| `StateSnapshot` | `snapshot` | Full state for remote rendering |
| `ToolCallStarted` | `tool_name, tool_id, arguments` | Tool started |
| `ToolResult` | `tool_id, output, success` | Tool completed |
| `PermissionPending` | `id, tool, path` | Permission request |
| `QuestionPending` | `id, questions` | Question request |
| `SessionInfo` | `id, model` | Session metadata |
| `SessionEnded` | `stop_reason` | Session termination |
| `Error` | `message` | Error message |
| `ResyncRequired` | `reason, pending_permissions, pending_questions` | Re-sync needed |

`App::handle_remote_event()` (`src/tui/app/mod.rs`) unwraps
`EventEnvelope` first, then dispatches the inner payload. Replayed and
live events share the same handler path.

## Configuration Surface

Client-side only. No config file entries. Connection parameters are
passed via CLI arguments.

**Timeouts (hardcoded):**

| Timeout | Value | Location |
|---------|-------|----------|
| Health check (HTTP) | 10s | `sdk.rs:40` |
| Health check (connect) | 10s | `sdk.rs:26` |
| WebSocket connect | 30s | `attach.rs:46` |
| Max retry attempts | 3 | `attach.rs:39` |
| Backoff (1st retry) | 1s | `attach.rs:42` |
| Backoff (2nd retry) | 2s | `attach.rs:42` |
| Backoff (3rd retry) | 4s | `attach.rs:42` |

## Invariants & Gotchas

- **`REMOTE_TUI_PROTOCOL_VERSION = 5`** (`tui.rs:14`). This is the
  wire version the client negotiates. The server validates compatibility.
- **`RenderFrame` is unsupported**: Both client and server reject it.
  Remote rendering uses `StateSnapshot` instead.
- **`catch_unwind`** on event task (`attach.rs:103`): Panics in the
  spawned event task do not crash the connection.
- **Channel capacity 256**: Both event and outbound channels are bounded
  at 256 (`attach.rs:14-15`). Full channels drop messages silently.
- **Projection-first handshake**: Client sends `ProjectionCapabilities`
  before the legacy `Resume` marker. The server may select
  projection-primary mode; the legacy marker is a raw-compat fallback.
- **No reconnection logic**: If the WebSocket drops, `run_attach`
  returns. The caller must re-invoke.
- **URL normalization is lenient**: Missing scheme defaults to HTTP.
  Trailing slashes are stripped. The `/tui` path is appended regardless
  of input format.

## Testing

```bash
# Client crate (no special features needed)
cargo test -p codegg

# TUI remote integration
cargo test --test tui_render

# Full remote TUI scenario
cargo test --test tui -- --test-threads=1
```

## Related Docs

- [server.md](server.md) — server that accepts connections
- `crates/codegg-protocol/src/tui.rs` — TuiMessage protocol
- [tui.md](tui.md) — TUI and remote-client integration
- `src/core/instance.rs` — `connect_or_start_daemon` singleton
