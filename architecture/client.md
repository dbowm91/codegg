# Client Module

## Native local frontend client

`crates/codegg-client` is the reusable frontend-side native client. It owns
`FrontendDescriptor`, bounded `ClientError` classifications, local daemon
path discovery, typed local endpoint parsing, the multiplexed CoreFrame client,
and connect/reuse/start orchestration. Its client stream uses Tokio's shared
byte-stream I/O boundary; Unix uses domain sockets and Windows uses named
pipes. Windows serving remains unqualified until live security and lifecycle
evidence is recorded. It depends on `codegg-protocol` and contains no daemon
authority.
`FrontendDescriptor` fields are private and selected by trusted composition
code. The root `SocketCoreClient` and `connect_or_start_daemon` APIs remain
compatibility adapters for the existing `CoreClient` and daemon lifecycle
types; both delegate to the shared implementation.

The optional Tauri shell is another trusted GUI composition and consumes only
this crate. Its JavaScript renderer receives a narrow bridge and never opens
the local socket itself; see [desktop architecture](desktop.md).

`LocalSocketClient` owns one reader task, bounded request/event channels,
request correlation, handshake/client identity, and reconnect. The TUI starts
it with an explicit TUI descriptor. A headless GUI-kind consumer can use the
same API without importing TUI or daemon modules. Disconnect only releases
frontend-owned transport tasks; daemon-owned turns and jobs continue.

`DocumentController` is the shared `document.v1` replica. It uses the
`codegg-document` rope core, applies local transactions before transport,
tracks daemon revision independently, assigns stable change IDs, serializes
flushes, retains uncertain requests for idempotent retry, and bounds pending
transactions and bytes. Save flushes accepted local work first. Conflict keeps
the replica intact; reconnect divergence requires explicit resync/recovery.
One owned debounce task flushes changes; adjacent unsent insertions coalesce
only when byte-coordinate rebasing is exact. A replacement transport
reattaches the same path and reacquires the writer lease; daemon restart/gone
state keeps the client draft without writing it over disk.
It owns no cursor, selection, viewport, keymap, or rendering state. The TUI
adapter wraps this controller and stores only presentation placeholders.

## Loss-aware events and the session projection driver (M004)

`ClientEvent` (`crates/codegg-client/src/event.rs`) is the only
loss-aware consumption surface beside the untouched `subscribe()`:
`Event` (an in-window live event), `Lagged` (receiver lag forced a
typed resync, never log-only loss), and `Closed` (transport shutdown
publishes disconnected state). `is_closed()` polls shutdown with a 25ms
liveness budget.

`SessionProjectionDriver` (same crate) owns one session projection
around `HeadlessProjectionConsumer`: pre-request receiver install,
exact subscription filtering, ack cadence, resume/replay/resync with a
fresh-subscribe fallback, stop with unsubscribe plus cursor retention,
and connection-owned, session-authorized subscription. `StoppedDriver`
resumes with a fresh subscription id (ids are never reused).
`artifact_excerpt` reads through a driver task command channel against
the consumer-validated registry (opaque handle, project binding,
revision, 64 KiB window); unknown handles and over-range reads are
typed bounded errors, never paths.

`compose_turn_submit` / `PromptIntent`
(`crates/codegg-client/src/compose.rs`) is the shared prompt composer:
the desktop submits opaque text and the daemon resolves durable model,
agents, and messages. The desktop never composes `TurnSubmit` itself.

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
   - `build_tui_ws_url()` (`attach.rs:154`): Converts HTTP/HTTPS/WS/WSS
     to the `/tui` WebSocket endpoint.
   - `build_http_url()` (`attach.rs:167`): Converts WS/WSS to HTTP/HTTPS
     for the health check.

2. **Health Check** (`sdk.rs:27`, request timeout at `:33`)
   - `RemoteClient::health()` → `GET /health` with 10s timeout.
    - The client uses `eggfetch-core 0.2.0` with the packaged WebPKI trust set
     and explicit redirect bounds.
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

5. **Channel Setup** (`attach.rs:101-104`)
   - `event_tx/rx`: server WS → TUI (256 capacity)
   - `out_tx/rx`: TUI → server WS (256 capacity)

6. **Background Tasks** (`attach.rs:109-144`)
   - `event_task`: receives WS messages, parses JSON, forwards to TUI
   - `send_task`: receives `TuiMessage` from TUI, serializes, sends WS

7. **TUI Initialization** (`attach.rs:99`)
   - `tui::App::new_remote()` with event channels.

8. **Cleanup** (`attach.rs:148-149`)
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
// src/client/sdk.rs:5
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
| `ProjectionSubscribe` | `request` | Subscribe to projection |
| `ProjectionResume` | `cursor, include_snapshot_if_resync` | Resume with cursor |
| `ProjectionAck` | `ack` | Acknowledge events |
| `ProjectionUnsubscribe` | `subscription_id` | Release a subscription |

### Server → Client Messages

| Variant | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq, payload` | Sequence-tagged for replay |
| `TextDelta` | `delta` | Streaming text output |
| `StateSnapshot` | `sequence, snapshot` | Full state for remote rendering |
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
| Health check (HTTP) | 10s | `sdk.rs:33` |
| Health check (connect) | 10s | `sdk.rs:12` |
| WebSocket connect | 30s | `attach.rs:46` |
| Max retry attempts | 3 | `attach.rs:38` |
| Backoff (1st retry) | 1s | `attach.rs:41` |
| Backoff (2nd retry) | 2s | `attach.rs:41` |
| Backoff (3rd retry) | 4s | `attach.rs:41` |

## Invariants & Gotchas

- **`REMOTE_TUI_PROTOCOL_VERSION = 5`** (`tui.rs:14`). This is the
  wire version the client negotiates. The server validates compatibility.
- **`RenderFrame` is unsupported**: Both client and server reject it.
  Remote rendering uses `StateSnapshot` instead. Note `StateSnapshot` is
  answered by the TUI's remote-mode handler (`src/tui/app/mod.rs`), not by
  `src/server/ws.rs`.
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

## Source verification

Verified 2026-10-06 against `src/client/attach.rs`, `src/client/sdk.rs`,
and `crates/codegg-protocol/src/tui.rs`. Corrected nine stale
`file.rs:line` refs in the connection-flow steps and timing table:
`build_tui_ws_url` `attach.rs:148` → `:154`, `build_http_url` `:161` →
`:167`, the health-check step `sdk.rs:35` → `:27`, channel setup
`attach.rs:95-101` → `:101-104`, background tasks `:103-138` →
`:109-144`, TUI initialization `:93` → `:99`, cleanup `:142-143` →
`:148-149`, the `RemoteClient` decl `sdk.rs:7` → `:5`, and the two
health-check timeout refs (`sdk.rs:40` → `:33`, `sdk.rs:26` → `:12`).
Retargeted the retry/backoff refs to their real declarations —
`max_attempts` at `attach.rs:38` and the `2^(attempt-1)` backoff at
`:41`, replacing the repeated `:42`. Verified accurate: `run_attach` at
`attach.rs:17`, the 30s connect timeout at `:46`, both 256-entry channel
capacities (`REMOTE_EVENT_CHANNEL_CAPACITY` / `REMOTE_OUTBOUND_CHANNEL_CAPACITY`),
3 max retry attempts with 1s/2s/4s backoff,
`REMOTE_TUI_PROTOCOL_VERSION` = 5 at `tui.rs:14`, `TuiMessage` at
`tui.rs:19`, and the five `ClientError` variants.

Re-verified 2026-10-06 against `crates/codegg-protocol/src/tui.rs` and
`src/server/ws.rs`. Corrected three field cells in the Client → Server /
Server → Client tables that named fields the variants do not have:
`ProjectionSubscribe` carries `request`
(`ProjectionSubscriptionRequest`), not `stream_id`; `ProjectionAck` carries
`ack` (`ProjectionAck`), not `subscription_id`/`seq`; and `StateSnapshot`
carries `sequence, snapshot`, not `snapshot` alone. Added the missing
`ProjectionUnsubscribe { subscription_id }` row. Corrected the
"Remote rendering uses `StateSnapshot`" invariant to record that the server
never emits that variant — `src/server/ws.rs` has zero
`TuiMessage::StateSnapshot` constructions; it is produced only by the TUI's
remote-mode handler (`src/tui/app/mod.rs:1855`).
