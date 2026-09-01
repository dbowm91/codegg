# Core Architecture

This document covers two distinct "core" concerns:

1. **`codegg-core` workspace crate** — domain types, storage, bus, and state
2. **`src/core/` module** — the daemon/transport facade (CoreClient, InprocCoreClient, etc.)

## Provider connection runtime ownership

`ConnectionManager` caches provider instances by `(connection_id, revision)`.
Rotation and refresh invalidate future resolutions after their transaction
commits while preserving already captured instances for in-flight requests.
Durable lifecycle transitions and purge eligibility live in the core
provider-connection store; the daemon protocol routes operator actions through
that authority.

---

## `codegg-core` Workspace Crate

**Location**: `crates/codegg-core/`

### Owned Modules

`codegg-core` currently owns these modules (exported from
`crates/codegg-core/src/lib.rs`):

| Module | Key Types |
|--------|-----------|
| `bus` | GlobalEventBus, PermissionRegistry, QuestionRegistry |
| `context` | ProjectContextResolver, ProjectContext, SessionId |
| `error` | AppError, ProviderError, ToolError, is_retryable |
| `goal` | Goal, GoalStatus, GoalBudget, GoalStore, runtime |
| `identity` | typed IDs and project/repository/workspace/session relations |
| `jobs` | JobStore, ScheduleStore, JobState, DaemonGeneration, RecoveryPolicy |
| `memory` | persistent session-to-session learning |
| `migration` | database migration and legacy store conversion |
| `model_profile` | model profile types |
| `project_catalog` | durable logical project registry |
| `project_discovery` | project root detection heuristics |
| `project_discovery_service` | service facade for discovery |
| `project_storage` | project-scoped asset persistence |
| `projection_replay` | projection stream store, service, seam |
| `protocol_conversions` | core-safe domain↔DTO conversions |
| `provider_connections` | connection store, metadata, lifecycle |
| `repository_lineage` | repository identity and lineage |
| `resilience` | CircuitBreaker, FallbackProvider |
| `run_store` | run record persistence and artifact storage |
| `session` | session storage, message history, checkpointing |
| `snapshot` | file state capture and restore |
| `storage` | SQLite initialization, connection pooling |
| `task_state` | task state tracking |
| `tool_program` | tool program IR, language, interpreter |
| `workspace` | WorkspaceRegistry, WorkspaceId, ExecutionContext |
| `workspace_services` | per-workspace service bundles and lifecycle |
| `worktree` | git worktree management |

### Re-exports into Root

Root `src/lib.rs` re-exports these modules so downstream code can use
`crate::bus`, `crate::session`, etc.:

```rust
pub use codegg_core::{
    bus, goal, identity, memory, migration, model_profile,
    project_storage, repository_lineage, resilience, run_store,
    session, snapshot, storage, task_state, workspace,
    workspace_services, worktree,
};
```

### Root-Side Modules (intentionally not moved)

These modules remain in root `src/` due to high coupling with UI/server/agent:

| Module | Reason |
|--------|--------|
| `acp` | agent communication protocol |
| `agent` | AgentLoop, compaction, routing, team |
| `tool` | all built-in tools |
| `permission` | access control, modes |
| `mcp` | Model Context Protocol client |
| `tui` | terminal user interface |
| `server` | HTTP/WebSocket server (feature-gated) |
| `client` | remote TUI client (feature-gated) |
| `core` | daemon runtime, transport adapters |
| `plugin` | WASM plugin system |
| `search`, `search_backend` | web search |
| `research` | deep research |
| `auth` | typed auth config, credential store |
| `context` | context projection and compaction |
| `scheduler` | global admission control (Phase 5) |
| `managed_process` | non-interactive process execution |
| `command_intent` | command classification and routing |
| `command_planner` | execution backend planning |
| `command_routing` | structured command dispatch |
| `command_outcome` | command outcome tracking |
| `shell` | human shell execution model |
| `python_script` | restricted-Python scripting |
| `preflight` | pre-mutation validation harness |
| `theme` | theme system |
| `tts` | text-to-speech |
| `upgrade` | self-upgrade |
| `hooks` | agent lifecycle hooks |
| `ide` | IDE integration |
| `lsp` | Language Server Protocol |
| `security` | SSRF, sandboxing |
| `shell_session` | shell session metadata |
| `skills` | skill loading and activation |
| `command` | slash command registry |
| `exec` | non-interactive exec mode |
| `util` | clipboard, fuzzy search, pricing |
| `protocol_conversions` | agent-specific domain↔DTO conversions |
| `eggsact` | deterministic tool runtime (in-process) |
| `background_task_migration` | legacy task migration |
| `git_mutation_projector` | git mutation projection |
| `git_mutations` | typed git mutation executor |
| `git_mutations_ops` | git mutation operation implementations |
| `git_network_ops` | git network operation implementations |
| `git_network_policy` | git network policy enforcement |
| `git_recovery` | git conflict recovery |
| `git_run_store` | git run store integration |
| `git_service` | canonical read git executor |
| `job_dispatcher` | job dispatch logic |
| `job_recovery` | job recovery logic |
| `test_runner` | test execution harness |

### Dependencies (Cargo.toml)

`codegg-core` depends on sibling workspace crates and external libraries,
but **never** on UI/server/plugin crates:

```
codegg-config, codegg-git, codegg-protocol, codegg-providers
egggit, egglsp, eggsentry
anyhow, base64, reqwest, async-trait, chrono, dashmap, dirs,
md5, once_cell, parking_lot, rand, regex, serde, serde_json,
sha2, similar, sqlx, thiserror, tokio, tokio-util, toml,
tracing, uuid, rustpython-parser
```

### Forbidden Dependencies

`codegg-core` must NOT depend on:

- **UI**: `ratatui`, `crossterm`, `ratatui_textarea`
- **Server**: `axum`, `tower_http`, `tokio_tungstenite`
- **Plugin**: `wasmtime`, `wasmtime_wasi`

Run `./scripts/check-core-boundary.sh` to verify no forbidden imports or
dependencies have crept in.

### Why Root `src/error.rs` Still Exists

Root `src/error.rs` re-exports `codegg_core::error::*` and adds
Axum-specific response wrappers (`AxumAppError`, `AxumServerRuntimeError`)
behind `#[cfg(feature = "server")]`. This avoids pulling `axum` into
`codegg-core`.

### Why Protocol Conversions Are Split

- `crates/codegg-core/src/protocol_conversions.rs`: Core-safe conversions
  (session, message, provider, config) that don't depend on agent/server
  runtime.
- `src/protocol_conversions.rs`: Agent-specific conversions + re-export of
  core conversions via `pub use codegg_core::protocol_conversions::*;`.

`codegg-protocol` must not depend on domain/runtime crates; conversions
intentionally live outside it.

### Next Likely Extraction Target

The daemon/agent/tool/permission boundary, not TUI. `src/core/daemon.rs`
is the next candidate but requires resolving agent coupling first.

---

## `src/core/` Module (Transport Facade)

**Location**: `src/core/`

The `core` module is the request/response facade that separates TUI
transport from the underlying agent and session logic.

### Key Responsibilities

- Provide a typed request/response boundary for UI and transport adapters
- Centralize session, memory, task, worktree, permission, and question
  operations
- Support in-process, stdio, and socket-backed execution modes
- Bridge core events into the global event bus when running in-process

### Module Inventory

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `core::daemon` | `CoreDaemon` | Central request dispatcher; owns workspace registry, event log, scheduler, workspace services, session runtime, notification router, asset refresh coordinator, and projection seam. 7686 lines. |
| `core::instance` | `DaemonPaths`, `DaemonInstanceGuard`, `DaemonInstanceMetadata`, `CoreRuntimeMode`, `connect_or_start_daemon` | Singleton daemon lifecycle, user-scoped path resolution, flock-based lock, connect-or-start helper. |
| `core::runtime_deps` | `CoreRuntimeDeps`, `LegacyAgentRuntimeDeps` | Bundles pool, memory_store, legacy_agent (subagent_pool), turn_runtime, lsp_service, workspace_services, workspace_service_policy, job_store, schedule_store, recovery_policy, daemon_generation, scheduler, submission, scheduler_config, connection_manager. Always has a default TurnRuntime; override via `with_turn_runtime()`. |
| `core::transport` | `SocketCoreClient`, `StdioCoreClient` | JSONL-over-socket and JSONL-over-stdio transports. Also contains `daemon_socket` for daemon-side socket accept loop. |
| `core::transport::projection` | projection stream management | Connection-local projection subscription, cursor, and forwarding state. |
| `core::event_log` | `EventLog` | In-memory event ring buffer with optional SQLite-backed projection sink. |
| `core::client_registry` | `ClientRegistry` | Maps transport connection IDs to metadata for projection ownership. |
| `core::notification` | `NotificationRouter`, `AudioArbiter` | TTS and notification policy routing. |
| `core::session_runtime` | `SessionRuntimeRegistry` | Active session runtime state tracking. |
| `core::session_selection` | `SelectionService` | Session-level connection/model selection via typed stores. |
| `core::provider_connections` | `ConnectionManager`, `ProviderConnectionStore` | Provider instance caching, lifecycle, and purge. |
| `core::eggpool` | `EggpoolProvisioner` | Connection provisioning and background refresh. |
| `core::project_activation` | `ProjectActivationRegistry` | Owner-scoped activation leases for projects. |

### `CoreClient`

```rust
#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>>;
}
```

`subscribe()` is event-capable for the in-process client. The stdio and
socket clients currently expose request/response transport and return an
empty receiver.

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process. Constructed via `with_deps(CoreRuntimeDeps, Config)` (preferred) or the legacy convenience constructor. `subscribe()` reads from `daemon.event_log` when a daemon is present; falls back to `GlobalEventBus` in legacy no-daemon mode. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to a Unix socket endpoint and exchanges JSONL requests/responses |

### Protocol

Defined in `crates/codegg-protocol/src/core.rs`.

#### Envelopes

| Type | Purpose |
|------|---------|
| `RequestEnvelope<T>` | Wraps requests with `protocol_version` and `request_id` |
| `EventEnvelope<T>` | Wraps events with sequence, timestamp, and optional session/turn metadata |
| `CoreRequest` | Typed requests (see families below) |
| `CoreResponse` | Typed responses for acknowledgements, JSON payloads, sessions, and errors |
| `CoreEvent` | Core-side event stream for in-process subscribers |

#### Request Families

- **Session lifecycle**: list, create, load, attach, fork, delete, archive,
  restore, share, unshare, rename, export, import, create-from-template,
  initialize, subscribe, resume
- **Turn lifecycle**: submit, cancel, steer, agent select, model select
- **Session data**: message loading and message counts
- **Provider connections**: create, cancel, status, list, models, rotate,
  refresh, enable, disable, delete, restore, purge, detail
- **Session selection**: get, list, update, models
- **Workspace**: register, list, archive, snapshot, services snapshot,
  config reload
- **Project catalog**: list, get, register, archive, restore, health,
  capabilities
- **Durable jobs**: submit, wait, get, list, cancel, retry, attempts,
  recovery report
- **Schedules**: create, list, get, pause, resume, delete
- **Goals**: set, from-file, show, pause, resume, clear, done, checkpoint,
  set-budget
- **Projections**: capabilities, subscribe, resume, ack, unsubscribe,
  snapshot-get, artifact-read, artifact-list
- **Tool programs**: list, inspect, call-page, notification-reinject,
  recovery-debug-inspect
- **Memory**: search, list, remember, forget
- **Tasks**: list, schedule, delete
- **Worktree**: list
- **Operational**: model refresh, permission/question response, snapshot
  (session/workspace/models/daemon), notification speak/stop, todo list,
  active goal load, run list/get/artifact-read, asset refresh/status,
  eggpool connection lifecycle

#### Request Handler Behavior

**Handled variants** (produce meaningful response):
- `TurnSubmit` — Spawns agent loop, returns `Ack` immediately
- `SessionMessagesLoad` / `SessionMessageCounts` — Returns session data
- `SessionCreate` / `SessionLoad` / `SessionAttach` — Session operations
- All other session variants (List, Fork, Delete, Archive, Restore, Share,
  Unshare, Rename, Export, Import, CreateFromTemplate)
- `PermissionRespond` / `QuestionRespond` — Registry responses
- `ModelsRefresh` — Returns refreshed model list
- `TaskList` / `TaskSchedule` / `TaskDelete` — Task operations
- `MemoryList` / `MemorySearch` / `MemoryRemember` / `MemoryForget`
- `WorktreeList` — Returns worktree list
- All workspace, project, job, schedule, goal, projection, provider
  connection, selection, asset refresh, and eggpool variants

**Fallthrough variants** (return `Ack` without processing):
- `Initialize`, `Subscribe`, `Resume`, `TurnCancel`, `TurnSteer`,
  `AgentSelect`, `ModelSelect`

### Transport Modes

| Mode | Description |
|------|-------------|
| DaemonClient (default) | Connects to (or auto-starts) the user-scoped singleton daemon via `connect_or_start_daemon` (`src/core/instance.rs`). Uses `SocketCoreClient`. |
| StandaloneInproc | Runs the core in the current process via `InprocCoreClient`. Visible non-production mode; requires `--standalone`. |
| StandaloneStdio | Spawns `codegg core-stdio` via `StdioCoreClient`. Compatibility/testing; requires `--stdio`. |

Selection: `CoreRuntimeMode` enum (default `DaemonClient`). `--standalone`
maps to `StandaloneInproc`; `--stdio` maps to `StandaloneStdio`. Legacy
`--core-transport inproc|stdio` still parses but emits a deprecation warning.

### Singleton Lifecycle

Phase 1 establishes the production invariant that exactly one user-scoped
Codegg daemon owns execution at a time. All implementation lives in
`src/core/instance.rs`.

**`DaemonPaths`** resolves all per-user daemon artifacts:

| Path | Purpose |
|------|---------|
| `daemon.lock` | Advisory exclusive lock on Unix (`flock(LOCK_EX \| LOCK_NB)`) — authoritative identity |
| `daemon.json` | Atomic metadata record (diagnostic only) |
| `core.sock` | Unix domain socket the daemon binds |
| `daemon.log` | Debug log (best-effort, rotated at 10 MB) |

Production locations:
- macOS: `$HOME/Library/Application Support/codegg`
- Linux: `${XDG_RUNTIME_DIR:-/tmp}/codegg` (falls back to
  `$HOME/.local/share/codegg` when neither is writable)
- Other Unix: `/tmp/codegg`

Override via `CODEGG_DAEMON_HOME`.

**`DaemonInstanceGuard`** is an RAII guard that holds the platform lock for the
daemon's lifetime. On Unix this is a non-blocking exclusive flock; on drop it
removes the metadata file (if owned by this guard) and releases the lock. The
OS also releases the lock automatically on process exit. Windows builds are
currently compatibility-only and do not enforce the singleton lock; production
Windows support requires a native `LockFileEx` implementation before the
singleton guarantee can apply there.

**`DaemonInstanceMetadata`** (`daemon.json`) carries: `daemon_id`,
`generation` (UUID), `pid`, `socket_path`, `protocol_version`,
`started_at`, `binary_version`. Written atomically (temp file + rename)
after socket bind. The lock is authoritative; metadata is diagnostic.

**`connect_or_start_daemon`** is the canonical frontend entry point
(`src/core/instance.rs`). It tries a verified connection to the
user-scoped endpoint; readiness requires a `SnapshotDaemon` identity probe
response. If unavailable and autostart is enabled, it spawns
`codegg daemon start --endpoint <socket> --force-take-lock` in a detached
Unix session (`setsid`), directs stdout/stderr to `daemon.log`, polls for
readiness with bounded timeout, and reaps the child after readiness without
making the frontend its lifetime owner. Concurrent starters converge on
whichever process owns the singleton lock; if the child exits early, the
frontend continues probing through the original deadline.

`ConnectOrStartOptions` controls: `paths`, `autostart`, `startup_timeout`
(default 10 s), `poll_interval` (default 100 ms), and optional
`executable` override (also accepts `CODEGG_DAEMON_EXECUTABLE` env var).

`DaemonConnectError` variants: `StartupTimeout`, `InconsistentState`,
`ChildExited`, `Io`.

The ordinary TUI remains a daemon client by default. `SIGINT` and
`SIGTERM` use the same cancellation path; graceful shutdown stops accepting
clients, drains within the configured bound, removes the owned
socket/metadata artifacts, and releases the lock. `daemon stop` verifies
the live wire daemon identity against metadata before signaling and waits
boundedly for observable cleanup without force-killing an unverified PID.

Endpoint selection is centralized in `DaemonPaths::resolve_for_endpoint`:
an explicit CLI endpoint wins over `CODEGG_CORE_ENDPOINT`, otherwise the
platform default is used. Custom sockets reuse the documented user-scoped
lock, metadata, and log root. The production daemon opens and migrates the
user-scoped catalog (`codegg.db`) before normal runtime initialization;
project-local `.codegg/sessions.db` remains legacy/import storage only.

**`CoreRuntimeMode`** enum (`src/core/instance.rs:42`):
- `DaemonClient` (default) — connect-or-start against the singleton daemon
- `StandaloneInproc` — in-process core, no daemon interaction (`--standalone`)
- `StandaloneStdio` — `core-stdio` subprocess (`--stdio`)

`InprocCoreClient` is now only used by tests, embedding, and
`--standalone` mode. The default TUI uses `SocketCoreClient` through
`connect_or_start_daemon`.

The `PROTOCOL_VERSION = 2` constant is unchanged. The `generation` UUID
lives in the on-disk metadata file, not in the wire protocol.

### Workspace Registry and Execution Context (Phase 2)

Phase 2 introduces workspace identity as a first-class daemon concept. A
daemon may now serve multiple distinct workspaces (project roots) and must
track which workspace each execution context targets.

**`WorkspaceRegistry`** (`crates/codegg-core/src/workspace.rs`) is
daemon-owned and deduplicates canonical roots via `get_or_register`. Rejects
nonexistent paths and symlink aliases. `CoreDaemon` holds
`workspaces: Arc<WorkspaceRegistry>`.

**`ExecutionContext`** (`crates/codegg-core/src/workspace.rs`) is immutable
and passed by `Arc` through `TurnRunInput` to every daemon execution path.
Replaces `std::env::current_dir()` reasoning. Carries `workspace_root`,
`workspace_id`, `session_id`, and path policy. `TurnRunInput` has
`execution: Arc<ExecutionContext>`.

**`WorkspaceId`** is a typed `String` newtype identifying a registered
workspace.

**Session binding**: `CoreDaemon::bind_runtime_for_session` resolves a
`session_id` to a `SessionRuntime` via `SessionStore` + `WorkspaceRegistry`.
`TurnSubmit`, `AgentSelect`, and `ModelSelect` reject unbound sessions.

**Storage**: workspace tables were introduced by migration v22 (a `workspace`
table plus `workspace_id` index on `session`). The schema has advanced well
past that; the current layout version is `STORAGE_LAYOUT_VERSION = 38`
(`crates/codegg-core/src/storage/mod.rs`). Existing sessions are lazily
resolved on next access; their `directory` is canonicalized into a workspace
record.

**Protocol**: `WorkspaceSnapshot` DTO,
`CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`,
`SessionSnapshot::workspace_id` + `directory`,
`ServerCapabilities::workspace_registration` + `workspace_snapshots`.

**Static guard**: `scripts/check_daemon_cwd_usage.py` scans protected
modules for `std::env::current_dir()` usage. Existing legacy uses in tool
`default()` constructors are allowlisted; new production-path uses fail CI.

See `crates/codegg-core/src/workspace.rs` for the full contract.

### Scheduler-owned execution (Phase 5 cutover)

Daemon-owned heavy work crosses `JobSubmissionService` before admission. The
facade validates the workspace-bound payload, applies the central resource
profile and exclusivity policy, creates the durable job, and enqueues it as
one logical operation. `JobScheduler` then owns queueing, permits, attempt
lifecycle, cancellation, and completion persistence.

`CoreRequest::JobSubmit`, `CoreRequest::JobWait`, and
`CoreRequest::SchedulerSnapshot` are the client-facing boundary. The daemon
snapshot carries only a bounded scheduler projection; clients fetch full job
and attempt records through dedicated operations. A disabled scheduler is an
explicit error state in daemon mode, never a route back to direct execution.

The canonical non-shell process policy is implemented by
`src/managed_process.rs`. It receives a durable job/attempt provenance pair,
uses sanitized noninteractive environment defaults, manages process groups,
enforces timeout and cancellation cleanup, and bounds captured output.

Explicit `--standalone` and `--stdio` compatibility modes may retain narrow
legacy adapters for tests and embedding, but they do not participate in the
singleton daemon's machine-wide admission guarantee. See
[`scheduler.md`](scheduler.md) for the execution-surface inventory and
compatibility boundary.

### Implementation Notes

- The core protocol version is currently `2` (`PROTOCOL_VERSION` in
  `crates/codegg-protocol/src/core.rs:26`).
- `CoreDaemon` (7686 lines) has ~20 fields covering daemon identity,
  runtime deps, event log, session/client registries, notification router,
  workspace registry, workspace services, eggpool provisioner, selection
  service, asset refresh coordinator, project activation, and projection
  seam.
- Projection transport ownership is connection-local in
  `src/core/transport/projection.rs`. The Unix socket and `/core` WebSocket
  retain daemon-issued subscription IDs, persisted stream descriptors,
  cursors, forwarder tasks, and cancellation state in the same bounded
  owner model.
- Local TUI flows should prefer `CoreClient` over direct store access when
  a request already exists in `CoreRequest`.
- The in-process client subscribes to `daemon.event_log` (via
  `EventLog::subscribe()`) when a daemon is present and forwards events to
  the channel receiver. In legacy no-daemon mode it falls back to
  `GlobalEventBus::subscribe()`. Actual event publishing happens inside
  `tokio::spawn` within turn execution handlers.
- `CoreDaemon` uses `CoreRuntimeDeps` to bundle runtime dependencies. The
  legacy convenience constructor remains for embedded callers; scheduled
  work is never supplied as a separate runtime dependency. Prefer
  `with_deps` for new code.
- Turn execution goes through the injected `TurnRuntime` trait
  (`agent::turn_runtime`). `CoreRuntimeDeps` always holds an
  `Arc<dyn TurnRuntime>` (defaults to `DefaultTurnRuntime`); the daemon
  calls `deps.turn_runtime.run_turn(input)` instead of constructing a
  runtime directly. The runtime owns tool registry construction, permission
  checker construction, agent loop construction, system prompt assembly,
  and background spawning.
- `build_agent_loop` is used only internally by `DefaultTurnRuntime`; its
  complete typed input prevents workspace identity from being patched into a
  partially initialized loop. New code should prefer `TurnRuntime`.
- `src/core/daemon.rs` has zero direct references to `AgentLoop`,
  `ToolRegistry`, `PermissionChecker`, `TaskToolRuntime`, or
  `build_session_tool_registry`.
- Daemon provider validation is intentionally duplicated (daemon validates
  provider existence before delegating to turn runtime) to preserve
  backward-compatible provider_not_found response shape.
- Daemon still owns: request validation, session_id/turn_id management,
  active-turn bookkeeping, TurnStarted event publishing, and CoreResponse
  return.

### Test Coverage

- `turn_submit_uses_injected_runtime` (`src/core/daemon.rs:7630`) —
  Verifies that `TurnSubmit` delegates to the injected `TurnRuntime` rather
  than constructing one inline.

### Project context resolver

Daemon request handlers use the core-owned `ProjectContextResolver` for
session creation, loading, turns, and project-scoped listing. It performs
bounded input parsing and durable membership/lifecycle checks before
execution. The resolver does not authorize principals and does not scan the
filesystem or use process cwd as identity authority.
