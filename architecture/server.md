# Server Module

## Purpose

Axum-based HTTP/WebSocket server providing remote TUI connections, REST
API, SSE event streaming, and the CoreFrame protocol. Feature-gated
under `server`.

## Where It Lives

| Path | Role |
|------|------|
| `src/server/mod.rs` | Re-exports `run_server`, `MdnsService`, `ServerState` |
| `src/server/http.rs` | Router setup, middleware stack, `run_server()` |
| `src/server/ws.rs` | WebSocket handlers (`/ws`, `/tui`, `/core`) |
| `src/server/state.rs` | `ServerState`, `WsRateLimiter` |
| `src/server/rpc.rs` | JSON-RPC 2.0 request/response types |
| `src/server/scope.rs` | Scope resolution for project context |
| `src/server/mdns.rs` | mDNS service discovery |
| `src/server/middleware/auth.rs` | Token auth middleware |
| `src/server/routes/` | REST route handlers (13 modules) |

## How It Works

### Entry Point

```rust
// src/server/http.rs:170
pub async fn run_server(
    host: &str,
    port: u16,
    daemon: Option<Arc<CoreDaemon>>,
) -> Result<(), ServerRuntimeError>
```

Requires `--standalone-core` to construct its own daemon. Without it,
exits with an actionable error rather than silently creating a second
core that defeats the singleton invariant.

### Middleware Stack (outermost first)

1. **Auth** (`middleware/auth.rs`) — Bearer token from `Authorization`
   header. Resolution: `CODEGG_SERVER_TOKEN` env → `server.token` config.
   When no token resolves and auth is not explicitly disabled, **requests
   are rejected** (fail-closed). Set `CODEGG_SERVER_AUTH_DISABLED=1` to
   bypass.

2. **Rate Limit** — 100 requests / 60s window per IP. Returns 429 with
   `Retry-After` and `X-RateLimit-*` headers. Key map capped at 10,000
   entries with eviction.

3. **Security Headers** — `X-Content-Type-Options: nosniff`,
   `X-Frame-Options: DENY`, `Strict-Transport-Security: max-age=31536000`.

4. **CORS** — Configurable via `[server.cors]` origins. Defaults:
   `http://localhost:3000`, `http://127.0.0.1:3000`. Methods: GET, POST,
   DELETE.

5. **Compression** — gzip + brotli. Skips compression for 401, 403, 404,
   422, 500, 502, 503 responses.

6. **Trace** — Request logging.

### Router Structure

```
/health (GET)              — no auth, no rate limit
/api
  ├── /sessions            — CRUD, fork, share, unshare, revert
  ├── /config              — config (API keys redacted)
  ├── /mcp                 — MCP server listing
  ├── /event               — SSE stream from GlobalEventBus
  ├── /question/:sid       — pending questions
  ├── /permission/:sid     — pending permissions
  ├── /providers           — provider listing
  ├── /tools               — tool listing
  ├── /file/{read,list,write,delete}  — file ops
  ├── /project, /projects  — project management
  └── /workspace           — workspace management
/ws                        — deprecated JSON-RPC WebSocket
/tui                       — TuiMessage protocol WebSocket
/core                      — CoreFrame protocol WebSocket
```

### WebSocket Endpoints

#### `/tui` — TuiMessage Protocol

Primary WebSocket for remote TUI. Bidirectional `TuiMessage` traffic
with `#[serde(tag = "type")]` JSON serialization.

**Client → Server**: `Input`, `KeyDown`, `MouseClick`, `Resize`,
`Resume`, `RequestSnapshot`, `PermissionResponse`, `QuestionResponse`,
`SessionInfo`, `ProjectionCapabilities`, `ProjectionSubscribe`,
`ProjectionResume`, `ProjectionAck`, `ProjectionUnsubscribe`.

**Server → Client**: `EventEnvelope` (sequence-tagged for replay),
`TextDelta`, `StateSnapshot`, `ToolCallStarted`, `ToolResult`,
`PermissionPending`, `QuestionPending`, `SessionInfo`, `SessionEnded`,
`Error`, `ResyncRequired`.

`RenderFrame` is unsupported — returns `Error` with code
`unsupported_render_frame`.

**Inbound size limits**: 4 MiB message, 4 MiB frame (`ws.rs:36-37`).
**Outbound queue**: 256-entry bounded channel (`WS_OUTBOUND_QUEUE_CAPACITY`).

#### `/ws` — Deprecated JSON-RPC

Retained for bounded legacy compatibility. 256-message outbound capacity.
Supported methods: `sessions.list`, `sessions.get`, `sessions.create`,
`providers.list`, `tools.list`.

M008 caller disposition: no in-repository production or test client invokes
`/ws`; the public route is therefore retained as externally-supported
compatibility rather than removed on absence-of-evidence. It is not a
projection transport, has no subscription/resume authority, and remains
bounded and authenticated. Removal requires an explicit compatibility-window
decision plus evidence that supported external clients have migrated to
`/core` or `/tui`.

Legacy caller matrix:

| Surface | In-repository callers | Disposition |
|---|---|---|
| `/ws` JSON-RPC route/handler and `RpcRequest` types | Server route and handler only; no production/test client | Retain as externally-supported compatibility; bounded/authenticated, no projection authority |
| `/tui` raw event/state fallback | `src/client/attach.rs`, `src/server/ws.rs`, and TUI projection mode fallback | Retain temporarily; bounded/session-scoped/non-authoritative until a future protocol compatibility decision |
| `CoreRequest::ProjectionSnapshotGet` | Daemon decoding/explicit rejection only; no caller | Retain as wire compatibility; reject with `projection_snapshot_requires_subscription` |
| Projection-private raw event fallback | No caller; filtered by `convert_core_event_to_tui` and raw forwarders | Remove-now behavior already landed: private projection envelopes are discarded |

#### `/core` — CoreFrame Protocol

For non-TUI clients. Negotiates `ClientHello` → `ServerHello`, carries
typed request/response/event/projection/subscription-filter frames.

### Projection Transport

Connection-local across all WebSocket adapters. Each connection owns a
bounded registry of `ProjectionSubscriptionId` values with cursor,
retention floor, forwarder task, and cancellation token.

- **Queue**: 256 entries. Raw compatibility traffic is lower priority.
- **Lagged**: Sends `ResyncRequired` with pending permissions/questions.
- **Critical writer path**: One-shot receipt + 500ms timeout per
  projection snapshot/replay/resync/ack/unsubscribe.
- **Caps**: 32 projection subscriptions, 8 artifact reads, 32
  diagnostics per connection.

### ServerState

```rust
// src/server/state.rs:106
pub struct ServerState {
    pub pool: SqlitePool,
    pub mcp_service: Arc<RwLock<McpService>>,
    pub config: Config,
    pub ws_rate_limiter: Arc<WsRateLimiter>,
    pub daemon: Option<Arc<CoreDaemon>>,
    pub projection_lifecycle_seam: ProjectionLifecycleSeam,
    // Test-only seams (production: None):
    pub connection_task_probe: Option<Arc<ConnectionTaskProbe>>,
    pub probe_factory: Option<ConnectionProbeFactory>,
    pub transport_test_config: Option<ProjectionTransportTestConfig>,
}
```

`ServerState` does not own a current project. Project/workspace scope
arrives in requests and is validated by the daemon's
`ProjectContextResolver`.

### mDNS Discovery

```rust
// src/server/mdns.rs
pub struct MdnsService { ... }
pub async fn discover_services(timeout_ms: u64) -> Vec<String>
```

Service type: `_opencode._tcp.local.` on multicast `224.0.0.251:5353`.

## Key Types & APIs

### Error Handling

```rust
// src/error.rs (behind #[cfg(feature = "server")])
pub enum ServerRuntimeError {
    Bind(String),
    Shutdown(String),
    WebSocket(String),
    Rpc(String),
    Auth(String),
}
```

| Error | HTTP Status |
|-------|-------------|
| `Auth` | 401 |
| Others | 500 |

### Rate Limiters

Two independent rate limiters with bounded key maps:

- **HTTP**: `RateLimiter` in `http.rs` — 100 req/60s, keyed by IP
- **WebSocket**: `WsRateLimiter` in `state.rs` — 100 req/60s, keyed by
  session/connection. Capped at 10,000 keys with eviction.

## Configuration Surface

```toml
# config.toml
[server]
host = "0.0.0.0"
port = 8080
token = "optional-token"

[server.cors]
origins = ["http://localhost:3000"]
```

**Environment variables:**

| Variable | Purpose |
|----------|---------|
| `CODEGG_SERVER_TOKEN` | Auth token (overrides config) |
| `CODEGG_SERVER_AUTH_DISABLED` | Disable auth entirely |

## Invariants & Gotchas

- **Fail-closed auth**: When token auth is enabled (the default) but no
  token resolves from env or config, both HTTP and WebSocket reject all
  requests. The server logs a warning at startup.
- **Singleton daemon**: The server requires `--standalone-core`. Without
  it, the server exits with an actionable error.
- **No default project**: `ServerState` carries no project identity.
  Project/workspace IDs arrive in requests.
- **WebSocket inbound caps**: 4 MiB message/frame limits prevent memory
  pressure from oversized frames.
- **Projection ownership**: Each connection owns its subscriptions.
  No daemon-wide event broadcast carries `ProjectionStreamEvent`.
- **`/ws` is deprecated**: New clients should use `/tui` or `/core`.
  Its outbound queue is finite (256); overflow closes the connection.
- **Raw `/tui` compatibility is retained temporarily**: clients that cannot
  negotiate the canonical projection mode continue to receive only the
  bounded, session-scoped raw event surface. It is non-authoritative and
  cannot carry private projection envelopes. The removal condition is a
  future `/tui` protocol compatibility decision after legacy clients have
  migrated.
- **`RenderFrame` unsupported**: Both `/tui` and remote clients see
  `Error { code: "unsupported_render_frame" }`.

## Testing

```bash
# Server crate (feature-gated)
cargo test -p codegg --features server

# WebSocket integration
cargo test --test tui_render

# Static guards (after changes)
python3 scripts/check_websocket_bounds.py
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
```

## Related Docs

- [client.md](client.md) — remote TUI client
- [protocol.md](protocol.md) — CoreRequest/CoreResponse, TuiMessage
- [bus.md](bus.md) — GlobalEventBus, PermissionRegistry
- `architecture/server.md` — implementation guide
