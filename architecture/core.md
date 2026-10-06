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

`codegg-core` currently owns 45 modules (exported from
`crates/codegg-core/src/lib.rs`):

| Module | Key Types |
|--------|-----------|
| `agent_convergence` | durable convergence records |
| `agent_run` | durable agent-run records and store |
| `agent_run_control` | durable run control (cancel/steer) service |
| `agent_run_group` | durable run-group state |
| `approval` | approval policy types |
| `audit` | append-only audit events |
| `audit_instrumentation` | audit emission helpers |
| `authorization` | capability authorization service and operation matrix |
| `bus` | GlobalEventBus, PermissionRegistry, QuestionRegistry |
| `collaboration` | project-chat collaboration domain |
| `context` | ProjectContextResolver, ProjectContext, SessionId |
| `error` | AppError, ProviderError, ToolError, is_retryable |
| `goal` | Goal, GoalStatus, GoalBudget, GoalStore, runtime |
| `identity` | typed IDs and project/repository/workspace/session relations |
| `jobs` | JobStore, ScheduleStore, JobState, DaemonGeneration, RecoveryPolicy |
| `memory` | persistent session-to-session learning |
| `migration` | database migration and legacy store conversion |
| `model_profile` | model profile types |
| `model_routing` | Codegg adapter for shared semantic model-policy compilation |
| `presence` | ephemeral presence leases |
| `project_catalog` | durable logical project registry |
| `project_discovery` | project root detection heuristics |
| `project_discovery_service` | service facade for discovery |
| `project_storage` | project-scoped asset persistence |
| `projection_replay` | projection stream store, service, seam |
| `protocol_conversions` | core-safe domain↔DTO conversions |
| `provider_connections` | connection store, metadata, lifecycle |
| `repository_lineage` | repository identity and lineage |
| `resilience` | CircuitBreaker, FallbackProvider |
| `run_result` | durable run result records |
| `run_store` | run record persistence and artifact storage |
| `session` | session storage, message history, checkpointing |
| `session_control` | session control state |
| `snapshot` | file state capture and restore |
| `storage` | SQLite initialization, connection pooling |
| `task_state` | task state tracking |
| `team` | principal, membership, role, capability domain |
| `tool_program` | tool program IR, language, interpreter |
| `transport_auth` | transport authentication and principal binding (M002) |
| `work_order` | durable project work orders, occurrences, lanes, triggers |
| `work_plan` | work-plan types |
| `workspace` | WorkspaceRegistry, WorkspaceId, ExecutionContext |
| `workspace_services` | per-workspace service bundles and lifecycle |
| `worktree` | git worktree management |
| `worktree_service` | managed worktree lifecycle service |

### Re-exports into Root

Root `src/lib.rs` re-exports these modules so downstream code can use
`crate::bus`, `crate::session`, etc.:

```rust
pub use codegg_core::{
    agent_convergence, agent_run, agent_run_control, agent_run_group, bus,
    goal, identity, memory, migration, model_profile, project_storage,
    repository_lineage, resilience, run_store, session, snapshot, storage,
    task_state, work_plan, workspace, workspace_services, worktree,
};
```

### Root-Side Modules (intentionally not moved)

These modules remain in root `src/` due to high coupling with UI/server/agent:

| Module | Reason |
|--------|--------|
| `acp` | agent communication protocol |
| `agent` | AgentLoop, compaction, routing, durable run control |
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
| `interactive_process_attach` | bounded attach/resume protocol over the M001 PTY engine |
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
anyhow, base64, async-trait, chrono, dashmap, dirs,
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

The daemon/agent/tool/permission boundary, not TUI. Residual M002 split
the `CoreDaemon` request dispatch into `core::daemon_family` plus 16
`core::daemon_*` family modules; Residual M003 has separated construction,
bootstrap/recovery, refresh, and shutdown into `core::daemon_construct`,
`core::daemon_bootstrap`, `core::daemon_refresh`, and
`core::daemon_shutdown` while preserving the exact initialization order and
joined shutdown sequence.

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
| `core::daemon` | `CoreDaemon` | Single composition/lifecycle authority; owns workspace registry, event log, scheduler, workspace services, session runtime, notification router, asset refresh coordinator, and projection seam. `handle_request_with_client` runs authorization/audit, the boxed chat pre-router, the spawned interactive-process pre-router, then a thin `DaemonRequestFamily` router that delegates each envelope to exactly one family handler. ~6,000 lines (was ~12,000 before M002; lifecycle now in `daemon_construct`/`daemon_bootstrap`/`daemon_refresh`/`daemon_shutdown`). |
| `core::daemon_family` | `DaemonRequestFamily` | Sole request-to-owner routing table: `of(&CoreRequest)` maps all 232 variants to one family plus `owner_module()`. Chat/interactive classify here but are served pre-router to preserve stack/cancellation semantics. |
| `core::daemon_assets` | `handle_assets_request` | Asset refresh/status/capabilities over the daemon-owned `AssetRefreshCoordinator`. |
| `core::daemon_control` | `handle_control_request` | Active-turn control requests (cancel/steer/permission-question response routing) owned outside the turn handler. |
| `core::daemon_documents` | `handle_document_request` | Open/close/reload document operations over daemon-owned document state. |
| `core::daemon_lsp` | `handle_lsp_request` | LSP status/operations plus diagnostics-store reads. |
| `core::daemon_team` | `handle_team_request` | Team membership, principal, and device-token administration (team-collaboration M003). |
| `core::daemon_work_orders` | `handle_work_order_request` | Project work orders, occurrences, sequence lanes, and trigger management. |
| `core::daemon_workspace_dashboard` | `handle_workspace_dashboard_request` | Workspace dashboard aggregates over daemon-owned workspace state. |
| `core::daemon_providers` | `handle_providers_request` | Eggpool provisioning and provider-connection lifecycle over the daemon-owned provisioner. |
| `core::daemon_sessions` | `handle_sessions_request` | Session CRUD, selection reads, message reads, import/export/template over daemon-owned session stores. |
| `core::daemon_turns` | `handle_turns_request` | Turn submit/cancel/steer, agent/model selection writes, permission/question responses, transport lifecycle. |
| `core::daemon_jobs` | `handle_jobs_request` | Durable jobs, schedules, run records, tool-program inspection, legacy task shims over scheduler/job stores. |
| `core::daemon_projects` | `handle_projects_request` | Project catalog, workspace registry/services, managed worktrees, daemon/workspace snapshots. |
| `core::daemon_goals` | `handle_goals_request` | Session goals, todos, edit checkpoints, LSP preview apply over daemon-owned domain stores. |
| `core::daemon_projection` | `handle_projection_request` | Projection replay subscribe/resume/ack/snapshot/artifacts plus ephemeral presence leases. |
| `core::daemon_ops` | `handle_ops_request` | Audit query/export, memory, notification routing over daemon-owned stores. |
| `core::daemon_construct` | `with_deps`, `with_deps_and_identity`, `new`, `SeamProjectionSink` | Deterministic dependency construction in documented order; single `Self { .. }` assembly with no partial publish. |
| `core::daemon_bootstrap` | `hydrate_workspace_registry`, `recover_state`, `recover_jobs`, `start_event_bridge`, `initialize_recovery_sequence` | Startup hydration, event bridge, turn/job recovery, and replay. `initialize_recovery_sequence` is the canonical in-process hydrate -> bridge -> recover order. |
| `core::daemon_refresh` | `refresh_project_context`, `refresh_project_activation`, `activate_project_workspace`, `project_health`, `refresh_runtime_assets` | Runtime refresh coordinators plus shared workspace/binding resolvers. All refresh flows through the daemon-owned `AssetRefreshCoordinator`. |
| `core::daemon_shutdown` | `Drop`, `abort_background_handles` | Joined shutdown: cancel precedes joins; aborts projection-maintenance and worktree-reconcile tasks in order. Scheduler loop stays detached by design. |
| `core::instance` | `DaemonPaths`, `DaemonInstanceGuard`, `DaemonInstanceMetadata`, `CoreRuntimeMode`, `connect_or_start_daemon` | Daemon-owned lock/metadata lifecycle and compatibility-facing connect-or-start API. |
| `core::runtime_deps` | `CoreRuntimeDeps`, `LegacyAgentRuntimeDeps` | Bundles pool, memory_store, legacy_agent (subagent_pool), turn_runtime, lsp_service, workspace_services, workspace_service_policy, job_store, schedule_store, recovery_policy, daemon_generation, scheduler, submission, scheduler_config, connection_manager. Always has a default TurnRuntime; override via `with_turn_runtime()`. |
| `core::transport` | `SocketCoreClient`, `StdioCoreClient` | JSONL over platform-local byte streams and stdio. Also contains `daemon_socket` for the daemon-side accept loop. |

Frontend identity, local endpoint/path resolution, socket transport, and
connect/reuse/start orchestration live in the leaf `codegg-client` crate.
Root `SocketCoreClient` and `connect_or_start_daemon` preserve existing root
APIs while adapting the extracted client. Daemon lock ownership, metadata,
listener binding, and core construction remain root-owned. See
[`client.md`](client.md).
| `core::transport::projection` | projection stream management | Connection-local projection subscription, cursor, and forwarding state. |
| `core::event_log` | `EventLog` | In-memory event ring buffer with optional SQLite-backed projection sink. |
| `core::client_registry` | `ClientRegistry`, `AuthenticatedPrincipal` | Maps transport connection IDs to metadata plus the immutable transport-bound canonical principal (M002) for projection ownership and request authority. |
| `core::notification` | `NotificationRouter`, `AudioArbiter` | TTS and notification policy routing. |
| `core::lsp_diagnostics_store` | diagnostics store | Daemon-owned LSP diagnostics persistence. |
| `core::work_order_coordinator` | work-order coordinator | Wakes and advances work-order gates and schedules. |
| `core::session_runtime` | `SessionRuntimeRegistry` | Active session runtime state tracking. |
| `core::session_selection` | `SelectionService` | Session-level connection/model selection via typed stores. M004: `ModelSelect` adapter, `apply_last_used_preference`, durable-projected runtime cache. |
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

`subscribe()` is event-capable for in-process and local-socket clients.
Stdio currently exposes request/response transport and returns an empty
receiver. `SocketCoreClient` adapts `codegg-client::LocalSocketClient` and
forwards its bounded event stream.

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process. Constructed via `with_deps(CoreRuntimeDeps, Config)` (preferred) or the legacy convenience constructor. `subscribe()` reads from `daemon.event_log` when a daemon is present; falls back to `GlobalEventBus` in legacy no-daemon mode. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to the platform-local daemon endpoint and exchanges JSONL requests/responses |

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
- **Presence** (M001, ephemeral): capabilities, heartbeat, snapshot-get.
  Daemon-owned leases; principals come from transport authority; never
  authoritative. See `architecture/presence.md`.
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

End-user vs internal surface: ordinary `codegg --help` shows only user
operations. `--standalone` stays visible (documented non-daemon user
mode); the deprecated `--core-transport`, the compatibility `--stdio`,
and the internal `--core-endpoint` still parse but are hidden from
normal help, as is the `core-stdio` JSONL entry point. Local daemon
attachment is `codegg daemon attach` (top-level `attach-daemon` remains
as a hidden compatibility alias); it is distinct from the
feature-gated remote HTTP `attach`, which only exists in `server`
builds. `CODEGG_CORE_TRANSPORT` remains honored as a deprecated
environment override with the same mapping and warnings.

### Singleton Lifecycle

Phase 1 establishes the production invariant that exactly one user-scoped
Codegg daemon owns execution at a time. All implementation lives in
`src/core/instance.rs`.

**`DaemonPaths`** resolves all per-user daemon artifacts:

| Path | Purpose |
|------|---------|
| `daemon.lock` | Advisory nonblocking `File::try_lock` — authoritative identity |
| `daemon.json` | Atomic metadata record (diagnostic only) |
| `core.sock` / named pipe | Platform-local CoreFrame endpoint; Windows implementation is present but live transport/lifecycle qualification is pending |
| `daemon.log` | Debug log (best-effort, rotated at 10 MB) |

Production locations:
- macOS: `$HOME/Library/Application Support/codegg`
- Linux: `${XDG_RUNTIME_DIR:-/tmp}/codegg` (falls back to
  `$HOME/.local/share/codegg` when neither is writable)
- Other Unix: `/tmp/codegg`
- Windows: per-user local application data directory

Override via `CODEGG_DAEMON_HOME`.

**`DaemonInstanceGuard`** is an RAII guard that holds the platform lock for the
daemon's lifetime. The lock is acquired with nonblocking `File::try_lock`; on drop it
removes the metadata file (if owned by this guard) and releases the lock. The
OS also releases the lock automatically on process exit. The standard library
maps this API to the platform locking primitive, including `LockFileEx` on
Windows. Windows live runtime qualification remains required before claiming
Windows support.

**`DaemonInstanceMetadata`** (`daemon.json`) carries: `daemon_id`,
`generation` (UUID), `pid`, `socket_path` (legacy/native form), optional
platform-neutral `endpoint_uri`, `protocol_version`, `started_at`, and
`binary_version`. Written atomically (temp file + rename) after listener bind.
Legacy records without `endpoint_uri` remain readable. The lock is
authoritative; metadata is diagnostic.

**`connect_or_start_daemon`** is the canonical frontend entry point
(`src/core/instance.rs`). It tries a verified connection to the
user-scoped endpoint; readiness requires a `SnapshotDaemon` identity probe
response. If unavailable and autostart is enabled, it spawns
`codegg daemon start --endpoint <socket> --force-take-lock`, directs
stdout/stderr to `daemon.log`, polls for
readiness with bounded timeout, and reaps the child after readiness without
making the frontend its lifetime owner. Unix launches use a detached session;
Windows launches use an independent child process. Graceful `daemon stop` is
currently Unix-only; Windows stop/replacement behavior remains a closure
condition. Concurrent starters converge on
whichever process owns the singleton lock; if the child exits early, the
frontend continues probing through the original deadline.

`ConnectOrStartOptions` controls: `paths`, `autostart`, `startup_timeout`
(default 10 s), `poll_interval` (default 100 ms), and optional
`executable` override (also accepts `CODEGG_DAEMON_EXECUTABLE` env var).

`DaemonConnectError` variants: `StartupTimeout`, `InconsistentState`,
`ChildExited`, `Io`.

The ordinary TUI remains a daemon client by default. On Unix, `SIGINT` and
`SIGTERM` use the same cancellation path; graceful shutdown stops accepting
clients, drains within the configured bound, removes the owned
endpoint/metadata artifacts, and releases the lock. `daemon stop` verifies
the live wire daemon identity against metadata before signaling and waits
boundedly for observable cleanup without force-killing an unverified PID.

Endpoint selection is centralized in `DaemonPaths::resolve_for_endpoint`:
an explicit CLI endpoint wins over `CODEGG_CORE_ENDPOINT`, otherwise the
platform default is used. URI schemes are `unix://` on Unix and `npipe://` on
Windows; unsupported schemes fail explicitly. Custom endpoints reuse the
documented user-scoped lock, metadata, and log root. The production daemon opens and migrates the
user-scoped catalog (`codegg.db`) before normal runtime initialization;
project-local `.codegg/sessions.db` remains legacy/import storage only.

**`CoreRuntimeMode`** enum (`src/core/instance.rs:41`):
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
past that; the current layout version is defined by
`storage::STORAGE_LAYOUT_VERSION`
(`crates/codegg-core/src/storage/mod.rs`). Existing sessions are lazily
resolved on next access; their `directory` is canonicalized into a workspace
record.

**Protocol**: `WorkspaceSnapshot` DTO,
`CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`,
`SessionSnapshot::workspace_id` + `directory`,
`ServerCapabilities::workspace_registration` + `workspace_snapshots`.
Raw `WorkspaceRegister`, global `WorkspaceList`, and unscoped
`ProjectRegister` are LocalOwner/proven-local only (`opaque` scope;
team principals fail closed); see `architecture/authorization.md` and
`architecture/workspace.md`.

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
  `crates/codegg-protocol/src/core.rs:28`).
- `CoreDaemon` (~6,000 lines in `daemon.rs` plus 16 `daemon_*`
  family modules totaling ~15,400 lines plus four `daemon_*` lifecycle
  modules totaling ~2,400 lines) holds daemon identity, runtime deps, event
  log, session/client registries, notification router, workspace registry,
  workspace services, eggpool provisioner, selection service, asset refresh
  coordinator, project activation, and projection seam. Family handlers and
  lifecycle helpers are boring `impl CoreDaemon` methods operating on the
  same daemon-owned state; they introduce no new store, scheduler, state
  machine, or authority. Shared helpers used across families
  (`record_origin_with_decision`, connection DTOs, tool-program DTOs, audit
  emitters) stay in `daemon.rs` as `pub(crate)` so every family calls the
  same canonical implementation; refresh/binding helpers live canonically in
  `daemon_refresh` and bootstrap helpers in `daemon_bootstrap`. The dispatch
  futures stay boxed per family, preserving the pre-M002 stack discipline
  that keeps the top-level dispatcher small.
- Lifecycle ownership: `daemon_construct` owns `with_deps` /
  `with_deps_and_identity` / `new` in the documented 11-phase order with a
  single assembly point (no partial publish). `daemon_bootstrap` owns
  hydrate, event bridge, `recover_state`, `recover_jobs`, and replay, with
  `initialize_recovery_sequence` as the canonical in-process hydrate ->
  bridge -> recover order. `daemon_refresh` owns all asset/activation/health
  coordinators plus the shared binding resolvers. `daemon_shutdown` owns
  `Drop` + `abort_background_handles` (cancel precedes joins; scheduler loop
  stays detached by design). Two private join handles were widened
  `private` -> `pub(crate)` so construction/shutdown can own them without a
  public API change (same precedent as M002 helper widening).
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

- `turn_submit_uses_injected_runtime` (`src/core/daemon.rs:5944`) —
  Verifies that `TurnSubmit` delegates to the injected `TurnRuntime` rather
  than constructing one inline.
- `request_family_routes_each_coherent_family`
  (`src/core/daemon_family.rs`) — Pins one representative variant per
  family (plus `Initialize` → turns, both session-attach spellings →
  sessions, chat/interactive pre-router classification) to
  `DaemonRequestFamily::of`, so a new variant cannot land without an
  explicit owner.
- `thin_dispatcher_delegates_to_family_handlers`
  (`src/core/daemon_family.rs`) — Drives a pool-less daemon through every
  capability probe plus the legacy `TaskList` rejection and asserts no
  response falls through to the historical `unimplemented` contract,
  proving the thin router reaches each owning family handler.
- Lifecycle ordering pins (`daemon_construct`, `daemon_bootstrap`,
  `daemon_refresh`, `daemon_shutdown`) — `construction_phases_match_documented_order`,
  `bootstrap_phases_match_documented_order`,
  `refresh_coordinators_match_documented_set`, and
  `shutdown_phases_match_documented_order` fail on silent reorder; the
  19 new lifecycle tests (5 construct + 5 bootstrap + 5 refresh + 4
  shutdown) cover wiring, pool-less shape, identity, service reuse,
  hydrate/bridge/recover order, missing-pool/table tolerance, health/activation
  fail-closed behavior, and abort-on-drop without new frameworks.

### Project context resolver

Daemon request handlers use the core-owned `ProjectContextResolver` for
session creation, loading, turns, and project-scoped listing. It performs
bounded input parsing and durable membership/lifecycle checks before
execution. The resolver does not authorize principals and does not scan the
filesystem or use process cwd as identity authority.

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/lib.rs`, `src/lib.rs`,
`src/core/*.rs`, `src/core/instance.rs`,
`crates/codegg-protocol/src/core.rs`, and
`crates/codegg-core/src/storage/mod.rs`. Corrected the `codegg-core`
module inventory, which listed 28 of the crate's 45 `pub mod`
declarations: added `agent_convergence`, `agent_run`, `agent_run_control`,
`agent_run_group`, `approval`, `audit`, `audit_instrumentation`,
`authorization`, `collaboration`, `presence`, `run_result`,
`session_control`, `team`, `transport_auth`, `work_order`, `work_plan`,
and `worktree_service`, and stated the 45 total. Corrected the root
`src/lib.rs` re-export block, which omitted `agent_convergence`,
`agent_run`, `agent_run_control`, `agent_run_group`, and `work_plan`.
Corrected `daemon.rs` size `~4,600` → `~6,000` lines (actual 6,005) and
the family-module count nine → 16 (actual ~15,400 lines across the 16
family modules, plus ~2,400 across the four lifecycle modules); corrected
the `DaemonRequestFamily::of` variant count 166 → 232 to match
`CoreRequest`. Added the six undocumented `src/core/daemon_*` family
modules (`daemon_control`, `daemon_documents`, `daemon_lsp`,
`daemon_team`, `daemon_work_orders`, `daemon_workspace_dashboard`) and
the two undocumented modules `core::lsp_diagnostics_store` and
`core::work_order_coordinator`. Corrected `file.rs:line` refs:
`PROTOCOL_VERSION` `core.rs:26` → `:28`, `CoreRuntimeMode`
`instance.rs:42` → `:41`, and `turn_submit_uses_injected_runtime`
`daemon.rs:4489` → `:5944`. Verified accurate: the three `CoreClient`
implementations, the three `CoreRuntimeMode` variants, the
`DaemonPaths` / `DaemonInstanceGuard` / `DaemonInstanceMetadata` /
`DaemonConnectError` / `ConnectOrStartOptions` types and their
declarations in `instance.rs`, the `ConnectOrStartOptions` 10 s / 100 ms
defaults, `STORAGE_LAYOUT_VERSION` = 68, `PROTOCOL_VERSION` = 2, and the
`ExecutionContext` / `WorkspaceRegistry` contract.
