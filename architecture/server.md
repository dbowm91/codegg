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
// src/server/http.rs:182
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

1. **Auth** (`middleware/auth.rs`, M002) — Every accepted connection
   resolves to a canonical principal from trusted transport evidence.
   Personal tokens (`cggt_<token_id>.<secret>`) verify against the
   `personal_auth_token` digest store and bind distinct team principals as
   `AuthenticatedRemote`. The legacy global bearer (`CODEGG_SERVER_TOKEN`
   env → `server.token` config) remains only as bootstrap/compatibility:
   it binds `LocalOwner` via `bootstrap_global_bearer` and MUST NOT
   masquerade as distinct identities. Removal condition: once every
   operator holds a personal token, delete the shared secret and require
   personal tokens. When no credential resolves and auth is not explicitly
   disabled, **requests are rejected** (fail-closed: 503 when nothing is
   configured, 401 for wrong/unknown/revoked/expired). Set
   `CODEGG_SERVER_AUTH_DISABLED=1` to bypass (binds `LocalOwner` for
   diagnostics; still carries an explicit principal). Secrets never enter
   logs/events; only token kind is logged via `redact_presented_token`.

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
  ├── /config              — LocalOwner-only (daemon-global config)
  ├── /mcp                 — LocalOwner-only (daemon-global status)
  ├── /event               — LocalOwner-only SSE (unfiltered bus)
  ├── /question/:sid       — pending questions (session-scoped authz)
  ├── /permission/:sid     — pending permissions (session-scoped authz)
  ├── /providers           — LocalOwner-only (credential-adjacent)
  ├── /tools               — LocalOwner-only (daemon-global catalog)
  ├── /file/{read,list,write,delete}  — file ops (project-scoped authz)
  ├── /project, /projects  — project management (project-scoped authz)
  └── /workspace           — workspace management (project-scoped authz)
/api/v1/task-triggers/{id}/fire (POST) — narrow external trigger fire
  (M005; bearer-only capability, outside the principal auth layer —
  see below)
/ws                        — deprecated JSON-RPC, LocalOwner-only compat
/tui                       — TuiMessage protocol WebSocket (CoreAdapter)
/core                      — CoreFrame protocol WebSocket (CoreAdapter)
```

### HTTP authorization convergence (team-collaboration M001)

Every authenticated HTTP compatibility route converges on the canonical
authorization service via `src/server/authz.rs` — there is no second
authorization framework and no handler hand-rolls role expansion:

- `auth_middleware` resolves network credentials to canonical
  `AuthenticatedPrincipal` request extensions. Handlers consume that
  extension (`Extension<AuthenticatedPrincipal>`); body/query
  `principal`, `role`, or capability fields are never introduced or
  trusted.
- `route_disposition_table()` is the executable route-disposition matrix:
  `SharedAuthz` (same capability as the Core equivalent),
  `LocalOwnerOnly` (no safe project scope; team fails closed with a
  privacy-safe 404), `CoreAdapter` (`/tui`, `/core` — the daemon gate
  remains authoritative), and `TriggerCapability` (the narrow
  `cggtr_...` fire endpoint, deliberately outside principal auth).
  `scripts/check_http_route_disposition.py` fails CI when a mounted route
  lacks a disposition.
- Project/session/workspace reads and mutations use the same capabilities
  as their Core equivalents (`project.read` / `project.configure`,
  `session.read` / `session.create`, `project.configure` for share).
  Project enumeration filters by `visible_projects` before building rows.
- File routes require explicit canonical project/workspace context and
  `file.read` / `file.modify`; ambiguous or scope-free team requests fail
  closed with `project_not_found`. Authorization precedes any filesystem
  mutation, so denied writes have zero side effects.
- Permission/question lists require `session.read` on the owning session;
  responses require mutation authority (`session.create`) via the
  `authorize_control_response` hook, which M004 will narrow to the
  controller lease without another bypass. Pending IDs never leak across
  projects (denials are 404).
- `/api/event` is LocalOwner-only compatibility: the global event bus is
  unfiltered, so team principals receive 404 rather than a cross-project
  stream. A future milestone may adapt it to authorized
  projection/subscription machinery with explicit scope.
- Config/provider/tool/MCP surfaces are LocalOwner-only compatibility
  (no safe project scope; Core connection operations are opaque and fail
  closed for team principals).
- Legacy `/ws` JSON-RPC is LocalOwner-only compatibility with no
  projection authority; team clients must use `/core`. `/core` behavior
  and protocol remain authoritative; REST is an adapter surface.
- No HTTP handler infers project identity from cwd; scope always arrives
  explicitly (`project_id` + `workspace_id` or a unique directory locator
  resolved server-side) and is authorized before use.
- Task-trigger fire remains its separately authenticated `cggtr_...`
  capability and is never converted into a principal credential;
  principal bearers presented there are rejected without verification.

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
| `CODEGG_SERVER_TOKEN` | Legacy bootstrap bearer (overrides config). Compatibility only; maps to `LocalOwner`. |
| `CODEGG_SERVER_AUTH_DISABLED` | Disable auth entirely (binds `LocalOwner` for diagnostics). |

### Operator auth setup (M002, administered via `/team` in M003)

Personal-local needs no login: start the daemon normally and trusted local
transports resolve `LocalOwner` automatically. Team remote access uses
personal tokens, provisioned through the secret-safe local `/team`
surface (LocalOwner-only; see `architecture/identity.md`):

1. Create one principal per human via `/team principal-create <display-name>`
   (`TeamPrincipalCreate`; daemon-owned API, no request payload can self-grant).
2. Add the principal to each project via `/team add <principal-id> <role>`
   (project Owner scope, `member.manage`).
3. Issue one token per device via `/team token-create <principal-id> <label>`
   (`TeamTokenCreate` returns the one-time `cggt_...` plaintext in a
   copy-once modal; only the digest persists; local transport only).
4. Present it as `Authorization: Bearer cggt_...` on HTTP and WebSocket.
   Distinct tokens bind distinct canonical principals; revocation/expiry
   (`/team token-revoke <token-id>`, monotonic) fails new authentication
   immediately.
5. Keep the legacy `server.token`/`CODEGG_SERVER_TOKEN` only until every
   operator holds a personal token; it always maps to `LocalOwner` and
   never to distinct identities. Delete it to complete the migration.

### External task-trigger fire endpoint (M005)

Project Work Orders M005 exposes one narrow capability endpoint for
external automation (shell scripts, CI glue, cron wrappers) to
satisfy a declared `ExternalTrigger` gate — see
[work_orders.md](work_orders.md) for the trigger lifecycle. The
endpoint lives outside the principal `auth_middleware`: the bearer
`cggtr_<trigger-id>.<secret>` verifies against the stored trigger
verifier only and never binds an `AuthenticatedPrincipal`.

```text
POST /api/v1/task-triggers/<trigger-id>/fire
Authorization: Bearer cggtr_<trigger-id>.<secret>
Idempotency-Key: <optional caller-generated value>
Content-Length: 0
```

```bash
# Fire a waiting task from a script. The secret travels in the header,
# never in the URL (query-string secrets are ignored, never accepted).
curl -sS -X POST \
  "http://127.0.0.1:8080/api/v1/task-triggers/${TRIGGER_ID}/fire" \
  -H "Authorization: Bearer ${TRIGGER_TOKEN}" \
  -H "Idempotency-Key: ci-run-${GITHUB_RUN_ID:-local}-1" \
  -H "Content-Length: 0"
# {"status":"accepted","receipt_id":"..."} — or "already_fired" on replay.
```

Contract notes:

- POST-only: any other method answers `405` with zero side effect.
  `GET` can never fire (safe against link previewers and crawlers).
- The body must be empty (tiny 4 KiB cap enforced before parsing;
  even tiny bodies are rejected so no prompt/task mutation can ride
  a fire request).
- Success returns `{"status":"accepted"|"already_fired",
  "receipt_id":"..."}` — a stable opaque receipt, no project,
  work-order, occurrence, session, model, or gate detail.
- Unknown locators, wrong secrets, and revoked/expired/exhausted
  triggers share one generic `401` (`trigger_invalid`); the error
  carries no locator, project, secret, or verifier content.
- The `Authorization` value never enters logs, events, audit
  metadata, or error bodies (only the public locator and the narrow
  outcome word are logged). Responses carry hardening headers and
  the route has its own IP-keyed rate limiter plus a tiny body cap.
- Firing latches the bound gate and wakes the WorkOrder coordinator;
  it never starts an agent/session directly and never widens
  authority. Principal-shaped bearers presented here are rejected
  without principal verification, and trigger bearers presented to
  normal auth never bind a principal.

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
- **`ConnectInfo` must be served explicitly**: `run_server` serves
  `into_make_service_with_connect_info::<SocketAddr>()` because the
  IP-keyed HTTP rate limiter extracts `ConnectInfo` — a bare `Router`
  discards the accept-side address and every request would 500 (found
  by M005 HTTP qualification; the WebSocket test harness already used
  this pattern).

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
- [work_orders.md](work_orders.md) — external task-trigger lifecycle (M005)
- [authorization.md](authorization.md) — trigger management capability mapping
- `architecture/server.md` — implementation guide
