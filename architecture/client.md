# Client Module

## Native local frontend client

`crates/codegg-client` is the reusable frontend-side native client. It owns
`FrontendDescriptor`, bounded `ClientError` classifications, local daemon
path discovery, typed local endpoint parsing, the multiplexed CoreFrame client,
and connect/reuse/start orchestration. Its client stream uses a `LocalIo`
trait (`AsyncRead + AsyncWrite + Unpin + Send`) split with `tokio::io::split`;
Unix uses domain sockets and Windows uses named pipes. Windows serving
remains unqualified until live security and lifecycle evidence is recorded.
It depends on `codegg-protocol` and contains no daemon authority.
`FrontendDescriptor` fields are private and selected by trusted composition
code. The root `SocketCoreClient` (`src/core/transport/socket.rs`) and
`connect_or_start_daemon` APIs remain compatibility adapters for the existing
`CoreClient` and daemon lifecycle types; both delegate to the shared
implementation.

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

`ClientEvent` (`crates/codegg-client/src/local.rs:39`) is the only
loss-aware consumption surface beside the untouched `subscribe()`:
`Event` (an in-window live event), `Lagged { dropped }` (receiver lag forced a
typed resync, never log-only loss), and `Closed` (transport shutdown
publishes disconnected state). `LocalSocketClient::is_closed()`
(`local.rs:157`) is a plain atomic read of the reader task's terminal state.

`SessionProjectionDriver` (`crates/codegg-client/src/driver.rs:483`) owns one
session projection around `HeadlessProjectionConsumer`: pre-request receiver
install, exact subscription filtering, ack cadence, resume/replay/resync with
a fresh-subscribe fallback, stop with unsubscribe plus cursor retention,
and connection-owned, session-authorized subscription. `StoppedDriver`
(`driver.rs:705`) resumes with a fresh subscription id (ids are never
reused). `artifact_excerpt` (`driver.rs:625`) reads through a driver task
command channel against the consumer-validated registry (opaque handle,
project binding, revision, 64 KiB window); unknown handles and over-range
reads are typed bounded errors, never paths. The driver also exposes
`node_telemetry` (`driver.rs`), a read-only operator view of the configured
remote Eggwork nodes that never creates, mutates, or releases a job.

`compose_turn_submit` / `PromptIntent`
(`crates/codegg-client/src/compose.rs`) is the shared prompt composer:
the desktop submits opaque text and the daemon resolves durable model,
agents, and messages. The desktop never composes `TurnSubmit` itself.
`PromptIntent` (`compose.rs:123`) is the state machine that tracks that
submission across `PromptIntentState` (`compose.rs:110`): `Captured`,
`CreatingSession`, `Submitting`, `Accepted`, `Failed`.

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
| `src/core/transport/socket.rs` | Root `SocketCoreClient` compatibility adapter |
| `src/core/instance.rs` | `connect_or_start_daemon` singleton entry point |
| `crates/codegg-client/src/lib.rs` | `FrontendDescriptor`, `codegg_client::ClientError` |
| `crates/codegg-client/src/local.rs` | `LocalSocketClient`, `ClientEvent` |
| `crates/codegg-client/src/connect.rs` | `connect_or_start_local_daemon` |
| `crates/codegg-client/src/paths.rs` | `LocalDaemonPaths`, `LocalEndpoint` |
| `crates/codegg-client/src/driver.rs` | `SessionProjectionDriver`, `StoppedDriver` |
| `crates/codegg-client/src/document.rs` | `DocumentController` |
| `crates/codegg-client/src/compose.rs` | `compose_turn_submit`, `PromptIntent` |
| `crates/codegg-client/src/lsp.rs` | typed LSP accessors, `DiagnosticsReconciler`, and the shared scoped request coordinator |
| `crates/codegg-protocol/src/tui.rs` | `TuiMessage`, `REMOTE_TUI_PROTOCOL_VERSION` |

## How It Works

### LSP request coordination

`codegg-client::LspRequestCoordinator` is the shared admission/freshness
contract used by the native TUI and desktop host. A request key binds the
frontend attachment, route generation, project/workspace/session, optional
document/path/server, document generation, operation, and position/argument.
Equivalent in-flight work is collapsed; a newer request of the same operation
supersedes older positions or scopes; the outstanding set is bounded. A
completed response is delivered only when its ticket remains current and the
active scope and document generation still match. The coordinator owns no
transport, editor text, or rendering state, and no lock is held across an
await. Each frontend still maps typed outcomes into its own view state.

### Project LSP diagnostics stream

`LspDiagnosticsStream` shares project-stream cursor and per-file sequence
reconciliation through `codegg-client::LspDiagnosticsStream`. Both production
frontends create their loss-aware event receiver before the dedicated
`LspDiagnosticsSubscribe` request. The subscription snapshot has no readiness
status, so it seeds stream ordering only; the frontend follows it with the
authorized `LspDiagnosticsGet` and renders only that pull's `Ready` or
`NotReady` result. `Ready(empty)` clears the view and `NotReady` retains the
last complete set. A stream gap, local receiver lag, transport gap, or close
marks the view stale and starts a fresh subscription plus authoritative pull.

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

There are **two** `ClientError` types; do not confuse them.

```rust
// crates/codegg-core/src/error.rs:428
// (re-exported by the root crate as `codegg::error::ClientError`)
pub enum ClientError {
    Connection(String),
    Unreachable(String),
    Rpc(String),
    WebSocket(String),
    Auth(String),
}
```

The remote TUI client (`src/client/sdk.rs`, `src/client/attach.rs`) uses
this one — it is the error type of the `codegg` root crate's
`src/error.rs`, which re-exports `codegg_core::error::*`.

The native client crate owns a **separate, bounded** classification
(`crates/codegg-client/src/lib.rs:89`) with 13 variants:
`InvalidEndpoint`, `Connect`, `Handshake`, `Protocol`, `PeerClosed`,
`Transport`, `WaiterCancelled`, `RequestConflict`,
`RequestCapacityClosed`, `StartupTimeout`, `InconsistentSingleton`,
`ChildExited` — plus the `Connect`/`Transport` `io::Error` sources.

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
| `KeyDown` | `key: String, modifiers: Vec<String>` | Keyboard events |
| `MouseClick` | `x: u16, y: u16` | Mouse clicks |
| `Resize` | `w: u16, h: u16` | Terminal resize |
| `Resume` | `from_event_seq: u64` | Resume handshake |
| `RequestSnapshot` | `reason: Option<String>` | Request full state snapshot |
| `PermissionResponse` | `id: String, choice: String` | Permission answer |
| `QuestionResponse` | `id: String, answers: serde_json::Value` | Question answer |
| `ProjectionCapabilities` | `capabilities` | Negotiate projection mode |
| `ProjectionSubscribe` | `request` | Subscribe to projection |
| `ProjectionResume` | `cursor, include_snapshot_if_resync` | Resume with cursor |
| `ProjectionAck` | `ack` | Acknowledge events |
| `ProjectionUnsubscribe` | `subscription_id` | Release a subscription |

`SessionInfo` is server-to-client only and is listed in the next table.

### Server → Client Messages

| Variant | Fields | Purpose |
|---------|--------|---------|
| `EventEnvelope` | `event_seq: u64, payload: Box<TuiMessage>` | Sequence-tagged for replay |
| `TextDelta` | `delta` | Streaming text output |
| `StateSnapshot` | `sequence, snapshot` | Full state for remote rendering |
| `ToolCallStarted` | `tool_name, tool_id, arguments` | Tool started |
| `ToolResult` | `tool_id, output, success` | Tool completed |
| `PermissionPending` | `id, tool, path` | Permission request |
| `QuestionPending` | `id, questions` | Question request |
| `SessionInfo` | `id, model` | Session metadata |
| `SessionEnded` | `stop_reason` | Session termination |
| `PluginUiEffect` | `envelope: UiEffectEnvelope` | Plugin UI effect |
| `Error` | `message` | Error message |
| `ResyncRequired` | `reason, pending_permissions, pending_questions` | Re-sync needed |

`App::handle_remote_event()` (`src/tui/app/mod.rs:3432`) unwraps
`EventEnvelope` first, then dispatches the inner payload. Replayed and
live events share the same handler path.

## Configuration Surface

Client-side only. No config file entries. Connection parameters are
passed via CLI arguments.

**Timeouts (hardcoded):**

| Timeout | Value | Location |
|---------|-------|----------|
| Health check (HTTP) | 10s | `sdk.rs:33` |
| Health check (client build) | 10s | `sdk.rs:12` |
| WebSocket connect | 30s | `attach.rs:46` |
| Max retry attempts | 3 | `attach.rs:39` |
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
- **No panic net in the client tasks**: `src/client/attach.rs` contains no
  `catch_unwind`. The panic net lives in the TUI event loop
  (`src/tui/runtime/event_loop.rs:217`) and in the render path
  (`src/tui/app/render.rs`). A panic inside the client's spawned
  `event_task`/`send_task` therefore aborts that task, not the process.
- **Channel capacity 256**: Both event and outbound channels are bounded
  at 256 (`attach.rs:14-15`). Full channels drop messages silently.
- **Projection-first handshake**: Client sends `ProjectionCapabilities`
  before the legacy `Resume` marker. The server may select
  projection-primary mode; the legacy marker is a raw-compat fallback.
- **No reconnection logic**: If the WebSocket drops, `run_attach`
  returns. The caller must re-invoke.
- **URL normalization is lenient**: Trailing slashes are stripped and the
  `/tui` path is appended regardless of input format. A missing scheme is
  passed through unchanged (it is *not* upgraded to HTTP), so
  `example:8080` yields a schemeless `example:8080/tui`.

## Testing

```bash
# Root package (the remote TUI client lives here)
cargo test -p codegg

# Native client crate
cargo test -p codegg-client

# TUI render family (one executable per subject area)
cargo test --test tui_render -- tui

# Session projection driver (native client)
cargo test -p codegg-client --test session_projection_driver

# Projection replay family (daemon ↔ client wire contract)
cargo test --test projection_replay
```

The root package's integration tests are **families** — one executable per
subject area under `tests/<family>/`. `cargo test --test <family>` no longer
selects a single member; use `cargo test --test <family> -- <member>`.

## Related Docs

- [server.md](server.md) — server that accepts connections
- `crates/codegg-protocol/src/tui.rs` — TuiMessage protocol
- [tui.md](tui.md) — TUI and remote-client integration
- `src/core/instance.rs` — `connect_or_start_daemon` singleton

## Source verification

Verified 2026-10-23 against `src/client/attach.rs`, `src/client/sdk.rs`,
`src/client/mod.rs`, `src/core/instance.rs`, `src/core/transport/socket.rs`,
`src/tui/app/mod.rs`, `src/tui/runtime/event_loop.rs`,
`crates/codegg-client/src/` (`lib.rs`, `local.rs`, `driver.rs`,
`document.rs`, `compose.rs`, `connect.rs`, `paths.rs`, `lsp.rs`),
`crates/codegg-core/src/error.rs`, and
`crates/codegg-protocol/src/tui.rs`.
- Re-verified 2026-10-27 against `crates/codegg-client/src/lsp.rs`,
  `src/tui/commands/editor.rs`, and
  `apps/desktop/src-tauri/src/document.rs`: the common diagnostics stream
  validates subscription metadata and per-file sequences, while both
  frontends obtain readiness through a separate authorized pull and recover
  after stream loss.
- **Removed a dead reference**: `ClientEvent` was documented at
  `crates/codegg-client/src/event.rs`. No such file exists; the type lives
  in `crates/codegg-client/src/local.rs:39`. Retargeted the reference and
  the doc's `Lagged` description to the real `Lagged { dropped: u64 }`
  shape.
- **Corrected a wrong invariant**: the doc claimed a `catch_unwind` net on
  the client's spawned event task (`attach.rs:103`). `src/client/attach.rs`
  contains no `catch_unwind` at all. The panic net is in the TUI event loop
  (`src/tui/runtime/event_loop.rs:217`) and the render path
  (`src/tui/app/render.rs`); a panic in a client task aborts that task.
- **Corrected the `is_closed()` liveness claim**: the doc said `is_closed()`
  "polls shutdown with a 25ms liveness budget". `is_closed()`
  (`crates/codegg-client/src/local.rs:157`) is a plain `AtomicBool` load.
  The 25 ms interval is the `SessionProjectionDriver`'s peer-death liveness
  tick (`crates/codegg-client/src/driver.rs:417`), a different component.
- **Disambiguated `ClientError`**: the doc pointed at `src/error.rs`, which
  only re-exports `codegg_core::error::*`. The five-variant enum the remote
  TUI client uses is `codegg_core::error::ClientError`
  (`crates/codegg-core/src/error.rs:428`); the native client crate owns a
  *different*, 13-variant `codegg_client::ClientError`
  (`crates/codegg-client/src/lib.rs:89`), which the doc had not mentioned
  despite naming it in the opening section.
- **Corrected the URL-normalization invariant**: a missing scheme is *not*
  upgraded to HTTP — `build_tui_ws_url` (`attach.rs:154`) passes the
  schemeless base through and only appends `/tui`.
- Corrected the message-table field cells against `tui.rs:19-220`:
  `RequestSnapshot` carries `reason: Option<String>` (not "—"),
  `KeyDown`/`MouseClick`/`Resize`/`PermissionResponse`/`QuestionResponse`
  now show their real field types, `PluginUiEffect { envelope }` was added
  to the server→client table, and `SessionInfo` was removed from the
  client→server table (it is server→client only).
- Corrected the `StateSnapshot` production-site reference: the doc cited
  `src/tui/app/mod.rs:1855`; the `RemoteTuiMessage::StateSnapshot`
  constructions are at `src/tui/app/mod.rs:3591` and `:3601`, and
  `handle_remote_event` is at `:3432`.
- Corrected `max_attempts` `attach.rs:38` -> `:39`.
- Rebuilt the "Where It Lives" table: it listed only the three `src/client/`
  files and `tui.rs`, omitting the `codegg-client` crate's own modules
  (`local.rs`, `connect.rs`, `paths.rs`, `driver.rs`, `document.rs`,
  `compose.rs`, `lsp.rs`) and the root `SocketCoreClient` /
  `connect_or_start_daemon` adapters.
- Replaced the dead test command `cargo test --test tui` (no such target;
  `tests/tui` does not exist) with the family form
  `cargo test --test tui_render -- tui`, and added the native client's own
  test targets.
- Verified accurate, no change: `run_attach` at `attach.rs:17`;
  `build_tui_ws_url` at `:154` and `build_http_url` at `:167`; the 30s
  connect timeout at `:46`; the `2^(attempt-1)` backoff at `:41`; the
  projection-first handshake (`ProjectionCapabilities` at `:82`,
  `TuiMessage::Resume` at `:92`); `tui::App::new_remote` at `:99`; channel
  setup at `:101-104`; the two background tasks at `:109`/`:136` and their
  abort at `:148-149`; both 256-entry channel capacities
  (`REMOTE_EVENT_CHANNEL_CAPACITY` / `REMOTE_OUTBOUND_CHANNEL_CAPACITY` at
  `attach.rs:14-15`); `RemoteClient` at `sdk.rs:5` with `health()` at `:27`
  and both 10s timeouts at `:12`/`:33`; `REMOTE_TUI_PROTOCOL_VERSION = 5`
  at `tui.rs:14`; `TuiMessage` at `tui.rs:19`; the five
  `codegg_core::error::ClientError` variants; `SnapshotDaemon`'s
  `daemon_id` / `uptime_secs` / `active_sessions` / `connected_clients`
  fields; and the `daemon status` CLI printing `generation` and `started_at`
  (`src/main.rs:1993-1996`).

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
