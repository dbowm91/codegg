---
name: server
description: HTTP/WebSocket server for remote TUI connections with Axum
version: 1.3.0
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
| `ws.rs` | WebSocket handling for `/ws`, `/tui`, and `/core` endpoints, RPC request processing, TUI message handling |
| `state.rs` | `ServerState` struct with shared state including `WsRateLimiter` (note: `event_bus` field was removed - SSE uses global `GlobalEventBus`) |
| `rpc.rs` | RPC message structures: `RpcRequest`, `RpcResponse`, `RpcError` (for the deprecated `/ws` JSON-RPC compatibility endpoint) |
| `perm_ids.rs` | Scoped pending-request ID parsing for permissions/questions |
| `authz.rs` | Server-side authorization convergence; `route_disposition_table()` is the executable route/disposition matrix |
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
| `routes/health.rs` | `health_check()` handler, imported by `http.rs` and mounted at `/health` |
| `routes/task_trigger.rs` | External trigger fire router (own bearer capability + limiter, outside `auth_middleware`) |
| `scope.rs` | Request scope and path extraction utilities |

## Entry Point

```rust
// src/server/http.rs:182
pub async fn run_server(
    host: &str,
    port: u16,
    daemon: Option<Arc<CoreDaemon>>,
) -> Result<(), ServerRuntimeError>
```

**Phase 1 (singleton daemon)**: The server requires `--standalone-core` to
construct its own daemon. Without it, `cmd_server` (`src/main.rs:3552`) prints
an actionable error and exits with code 2 rather than silently creating a
second core that defeats the singleton invariant. The optional `daemon` handle
carries the daemon-connected server mode.

## ServerState

```rust
pub struct ServerState {
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,
    pub daemon: Option<Arc<crate::core::daemon::CoreDaemon>>,
    pub projection_lifecycle_seam: ProjectionLifecycleSeam,
    // Test-only seams (production leaves these None):
    pub connection_task_probe: Option<Arc<ConnectionTaskProbe>>,
    pub probe_factory: Option<ConnectionProbeFactory>,
    pub transport_test_config: Option<ProjectionTransportTestConfig>,
}
```

Key points:
- `ws_rate_limiter` is shared across all WebSocket connections (not created per-connection) and bounded (`MAX_WS_RATE_LIMITER_KEYS = 10_000`) against key churn
- `daemon` carries the optional `CoreDaemon` handle for daemon-connected server mode
- `projection_lifecycle_seam` is the connection-adapter lifecycle seam (no-op by default; tests may install pause/fault policy)
- SSE handler (`/api/event`) and TUI WebSocket (`/tui`) use `GlobalEventBus::subscribe()` from `crate::bus::global`
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

### Project Routes (`/api/project`, `/api/projects`)
- `GET /api/project` - Get current project info
- `POST /api/project` - Create project (LocalOwner-only raw daemon-local
  bootstrap; team principals fail closed before any mkdir/workspace/
  catalog/membership side effect)
- `GET /api/project/list` - List projects
- `GET /api/projects` - List bounded catalog summaries
- `GET /api/projects/:id` - Get one explicit project
- `POST /api/projects/:id/archive` - Archive one project logically
- `POST /api/projects/:id/restore` - Restore one project

### Workspace Routes (`/api/workspace`)
- `GET /api/workspace` - Get workspace info
- `POST /api/workspace` - Create workspace (project-scoped mutation
  under `project.configure`; raw global register/list stays
  LocalOwner-only)
- `GET /api/workspace/list` - List workspaces (project-scoped; global
  enumeration is not a team surface)

### WebSocket Routes
- `GET /ws` - Deprecated bounded JSON-RPC WebSocket (legacy compatibility)
- `GET /core` - CoreFrame WebSocket protocol for non-TUI clients
- `GET /tui` - WebSocket TUI endpoint (TuiMessage protocol)

### Health Route
- `GET /health` - Health check

### External Task-Trigger Fire Route
- `POST /api/v1/task-triggers/{trigger_id}/fire` - Fire a waiting task from
  external automation. Deliberately outside `auth_middleware`: it carries its
  own IP rate limiter and body cap, and the `cggtr_...` bearer is verified
  inside the handler against the stored verifier only. Principal bearers are
  rejected without verification.

## WebSocket Protocol

### `/ws` - Deprecated JSON-RPC (legacy compatibility)
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

Uses `TuiMessage` enum from the `codegg-protocol` crate (`crates/codegg-protocol/src/tui.rs`, re-exported as `codegg::protocol::tui::TuiMessage`; the root-level `src/protocol/` directory no longer exists):

**Client → Server**:
- `Input { text }` - User text input
- `KeyDown { key, modifiers }` - Keyboard events
- `MouseClick { x, y }` - Mouse clicks
- `Resize { w, h }` - Terminal resize
- `Resume { from_event_seq }` - Resume handshake for buffered event replay
- `RequestSnapshot { reason }` - Full replay from sequence 0 + resync
- `RenderFrame` - Rejected; returns `unsupported_render_frame`
- `PermissionResponse { id, choice }` - Permission answer
- `QuestionResponse { id, answers }` - Question answer
- `SessionInfo { id, model }` - Session metadata
- Projection family: `ProjectionCapabilities`, `ProjectionSubscribe`,
  `ProjectionAck`, `ProjectionResume`, `ProjectionUnsubscribe`,
  `ProjectionSubscriptionStatus`, `ProjectionArtifactListRequest`,
  `ProjectionArtifactReadRequest`

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
- Projection family: `ProjectionCapabilitiesAck`, `ProjectionSnapshot`,
  `ProjectionReplay`, `ProjectionResync`, `ProjectionEvent`, `ProjectionAckResult`,
  `ProjectionUnsubscribeResult`, `ProjectionSubscriptionStatusResult`,
  `ProjectionArtifactListResult`, `ProjectionArtifactReadResult`,
  `ProjectionCompatibilityDiagnostic`

`StateSnapshot` is declared in the enum and handled by the local TUI client,
but `src/server/ws.rs` never emits it. `PluginUiEffect` carries plugin UI
over the same socket.

Inbound caps: 4 MiB message and 4 MiB frame (`WS_MAX_MESSAGE_SIZE` /
`WS_MAX_FRAME_SIZE`, `ws.rs:35-36`). Outbound queue 256
(`WS_OUTBOUND_QUEUE_CAPACITY`, `ws.rs:28`); separate inbound request queue 32
(`TUI_REQUEST_QUEUE_CAPACITY`, `ws.rs:29`).

## Authentication

Server uses token-based authentication via `Authorization: Bearer <token>` header.

`resolve_bearer_principal` (`middleware/auth.rs`) tries in order:
1. Personal token `cggt_...`, verified against the digest store, binds the
   token's canonical principal.
2. Legacy global bearer — `CODEGG_SERVER_TOKEN` env var, else `server.token`
   config. Binds `LocalOwner` via `bootstrap_global_bearer`; it must never
   masquerade as a distinct team identity.
3. Anything else is rejected fail-closed: 503 when no credential is configured
   at all, 401 for a wrong/unknown/revoked/expired bearer.

Set `CODEGG_SERVER_AUTH_DISABLED=1` (or `true`, case-insensitive — `0` and
`false` do *not* disable) to bypass. The bypass still inserts an explicit
LocalOwner principal, so downstream code never sees an anonymous request.

Auth middleware applies to all `/api/*` routes plus the `/ws`, `/tui`, and
`/core` upgrades. WebSocket endpoints additionally run `validate_ws_auth()`
(`ws.rs`), which all three handlers call. `/health` and the task-trigger fire
route sit outside the middleware.

## Rate Limiting

### HTTP Rate Limiter
- 100 requests per 60 seconds per IP (`RateLimiter::new(100, 60)`, `http.rs:248`)
- Returns 429 with a `Retry-After` header
- In-memory `HashMap` with per-key sliding window, capped at
  `MAX_RATE_LIMITER_KEYS = 10_000` (`http.rs:50`) with eviction

### WebSocket Rate Limiter
- Shared `WsRateLimiter` in `ServerState`, capped at
  `MAX_WS_RATE_LIMITER_KEYS = 10_000` (`state.rs:147`)
- 100 requests per 60 seconds per connection. `/ws` keys on the peer address;
  `/tui` keys on `TuiSessionState::rate_limit_key`, which starts as the peer
  address and becomes `session:<id>` once `SessionInfo` arrives.
- Exceeded budget returns an error message on the WebSocket itself, not a
  status code.

## Path Sanitization (`routes/file.rs`)

`sanitize_path(root, requested)` requires an absolute root, then delegates to
`sanitize_path_from_root`, which ensures file operations stay inside it:
1. Joins root and requested path
2. Canonicalizes root, then checks the join for symlinks via
   `check_path_for_symlinks()` (imported from `crate::tool::util`, `file.rs:13`)
3. Canonicalizes the join and verifies it starts with the canonicalized root
4. For non-existent paths, manually resolves `..` components one segment at a
   time (re-checking symlinks per segment) before the containment test

## SSE Event Stream

The `/api/event` endpoint provides Server-Sent Events:
- LocalOwner-only; subscribes to `GlobalEventBus` directly
- Streams each `AppEvent` as JSON with an `event:` prefix
- Includes 15-second heartbeat comments (and a 15s `KeepAlive`)
- On broadcast lag, emits a `resync_required` SSE event carrying the dropped
  count. There is no separate send-failure resync path in `event.rs`.

### TUI Replay Buffer

The `/tui` WebSocket keeps a bounded event buffer and assigns sequence numbers to outbound events. When a client sends `Resume { from_event_seq }`, the server replays buffered events above that sequence before sending `ResyncRequired`.

## Implementation Notes

### WsRateLimiter is Shared
The `WsRateLimiter` in `ServerState` is shared across all WebSocket connections. Previously, a new `RateLimiter` was created per connection, which was inefficient.

### SSE Uses GlobalEventBus Directly
`routes/event.rs` SSE handler uses `GlobalEventBus::subscribe()` directly from `crate::bus::global`. There is no local EventBus struct.

### Health Route
`/health` is mounted on the outer router (`http.rs:349`), outside
`api_router`, so it carries neither auth nor rate limiting. Its handler is
`routes::health::health_check`, imported at `http.rs:24` — there is no second
inline copy.

### `/core` CoreFrame WebSocket
`/core` (handled by `ws::handle_core_ws`) is the remote core protocol for non-TUI clients. It negotiates projection capabilities during handshake, advertises the project-catalog capability, and uses the same bounded critical writer path as `/tui`. See `architecture/server.md` for the full protocol.

### No TLS Implementation
There is no TLS anywhere in this module, and no TLS section in the config
schema either — `ServerConfig` (`crates/codegg-config/src/schema.rs`) has no
TLS field, so there is nothing to configure or to wire up. Terminate TLS in a
reverse proxy in front of the listener.

## ServerRuntimeError

Defined in `crates/codegg-core/src/error.rs` (`ServerRuntimeError`):

```rust
pub enum ServerRuntimeError {
    Bind(String),
    Shutdown(String),
    WebSocket(String),
    Rpc(String),
    Auth(String),
}
```

Root `src/error.rs` re-exports it via `pub use codegg_core::error::*` and adds
the `AxumServerRuntimeError` newtype behind `#[cfg(feature = "server")]`, which
maps `Auth` to 401 and the other variants to 500.

## Static Guards

Run these after touching `src/server/`. The first two fail `verify.sh quick`
and the CI `verify` job; the projection and ownership guards are
change-triggered and are not wired into either, so run them explicitly when
you touch those surfaces.

| Script | Enforces | In `quick`/CI |
|---|---|---|
| `python3 scripts/check_http_route_disposition.py` | Every `.route(...)` in `http.rs` and `task_trigger.rs` appears in `authz::route_disposition_table()` with a matching method (and vice versa); handlers under `routes/` reference the shared `authz` adapter rather than touching stores/bus directly (except the `TriggerCapability` trigger module); no handler introduces body/query `principal`, `role`, or `capability` fields. | yes |
| `python3 scripts/check_execution_ownership.py` | Process/execution surfaces stay inside the documented ownership model (see `docs/execution-ownership.toml`). | yes |
| `python3 scripts/check_websocket_bounds.py` | WebSocket queue-capacity and inbound size bounds stay in sync with the constants in `ws.rs`. | no — run on change |
| `python3 scripts/check_projection_transport_isolation.py` | Projection transport stays connection-local; no daemon-wide broadcast carries projection events. | no — run on change |
| `python3 scripts/check_projection_transport_lifecycle.py` | Projection connection lifecycle/fencing invariants hold. | no — run on change |

## Testing

```bash
cargo test -p codegg --features server     # server crate (feature-gated)
cargo test --test tui_render               # WebSocket/TUI integration
```

## See Also

- `architecture/server.md` - Architecture overview (includes the `/core` CoreFrame protocol)
- `architecture/client.md` - Native client / remote TUI client
- `architecture/authorization.md` - capability and route-disposition model
- `.opencode/skills/core/SKILL.md` - Core facade and daemon lifecycle
- `.opencode/skills/context/SKILL.md` - Context projection used by WebSocket transports
- `docs/execution-ownership.toml` - execution ownership manifest

## `/tui` Message Handling

The message *lists* are above; this is how the server acts on them. Only the
raw fallback mode is described here — see `architecture/server.md` for the
projection-primary contract.

| Message | Handling |
|---------|----------|
| `Resume { from_event_seq }` | `daemon.replay_from(from_event_seq, filter)` → `EventEnvelope` per event, then `ResyncRequired` (reason `resume_requested`). With no daemon, sends `ResyncRequired` (reason `no_daemon`) and replays nothing. |
| `RequestSnapshot { reason }` | Same, but `replay_from(0, ...)`; resync reason `snapshot_requested`. |
| `Resume` / `RequestSnapshot` in projection-primary mode | Ignored — returns early and answers with a `ProjectionCompatibilityDiagnostic` telling the client to resume with a `ProjectionCursor`. |
| `Input`, `KeyDown`, `MouseClick`, `Resize` | Logging stubs (`tracing::debug!`); not forwarded to an App. |
| `PermissionResponse` | Responds via `PermissionRegistry`; unknown `choice` strings fall back to `DenyOnce`. |
| `QuestionResponse` | Responds via `QuestionRegistry`. |
| `SessionInfo` | Stores session metadata and re-keys the rate limiter to `session:<id>`. |

Key implementation details:
- `convert_core_event_to_tui()` maps `CoreEvent` → `TuiMessage` for replay.
- Broadcast-channel lag triggers `ResyncRequired` automatically.
- The server does NOT maintain TUI state — clients reconstruct from replay.
- Replay is session-filtered (`EventFilter { session_id, client_id: None,
  include_global: false }`), so a client never receives another session's events.

## Source verification

Re-verified 2026-10-06 against `src/server/`, `src/main.rs`, and
`crates/codegg-protocol/src/tui.rs`. Confirmed accurate: every route in the
HTTP Routes list is mounted in `src/server/http.rs` (compared against the
`.route(...)` table); `run_server` at `http.rs:182`; `ServerState`'s 9 fields
at `state.rs:106`; `ServerRuntimeError`'s 5 variants at
`crates/codegg-core/src/error.rs:375` and the `Auth`→401 / rest→500 mapping
in `src/error.rs:179-187`; `--standalone-core` exiting 2 at `src/main.rs:3552`;
both rate limiters at 100/60s with 10,000-key caps (`MAX_RATE_LIMITER_KEYS`
at `http.rs:50`, `MAX_WS_RATE_LIMITER_KEYS` at `state.rs:147`); the SSE
heartbeat and `resync_required` lag path; the `sanitize_path` ordering; the
five `/ws` JSON-RPC methods; and the 13 `routes/` modules.

Corrected this pass:
- **Health route** claimed `routes/health.rs` was "standalone, not wired to
  main router" with an "inline `async fn health_check()` at the top level".
  There is no inline copy: `http.rs:24` imports `routes::health::health_check`
  and `http.rs:349` mounts it on the outer router (outside `api_router`, hence
  no auth and no rate limit).
- **TLS** claimed "config supports TLS section". `ServerConfig`
  (`crates/codegg-config/src/schema.rs:1020`) has no TLS field and `tls`
  appears nowhere in `crates/codegg-config/src/` or `src/server/`.
- **SSE** claimed `ResyncRequired` on "lag or send failures"; `event.rs` only
  has the broadcast-lag path.
- **Rate limiting** gained the real key sources (`/ws` peer address, `/tui`
  `TuiSessionState::rate_limit_key`) and the two cap constants.
- **Auth** gained the personal-token (`cggt_`) path, which the old precedence
  list omitted entirely, the 503-vs-401 fail-closed split, and the fact that
  `CODEGG_SERVER_AUTH_DISABLED` only accepts `1`/`true`.
- **Files/routes** gained `authz.rs` and `routes/task_trigger.rs`, the
  `POST /api/v1/task-triggers/{trigger_id}/fire` route, and `/core` on the
  `ws.rs` row.
- **`/tui` protocol** lists were missing ~19 real `TuiMessage` variants (the
  projection family, `RequestSnapshot`, `RenderFrame`); added, plus the
  inbound/outbound size and queue bounds. Noted that `StateSnapshot` is
  declared but never emitted by `src/server/ws.rs`.
- The trailing "Remote TUI Protocol (Phase 8)" section duplicated the `/tui`
  message lists; replaced with a handling table carrying the verified replay
  reasons (`resume_requested`, `snapshot_requested`, `no_daemon`), the
  projection-primary early-return, and the session-scoped `EventFilter`.
- **See Also** paths switched from `.skills/...` to the canonical
  `.opencode/skills/...` (AGENTS.md names that the canonical location; the
  others are symlinks).
