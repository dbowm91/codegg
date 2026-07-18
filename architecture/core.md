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

`codegg-core` currently owns these modules (exported from `crates/codegg-core/src/lib.rs`):

| Module | Key Types |
|--------|-----------|
| `bus` | GlobalEventBus, PermissionRegistry, QuestionRegistry |
| `error` | AppError, ProviderError, ToolError, is_retryable |
| `goal` | Goal, GoalStatus, GoalBudget, GoalStore, runtime |
| `identity` | Path-independent typed IDs and project/repository/workspace/session relations |
| `memory` | persistent session-to-session learning |
| `model_profile` | model profile types |
| `protocol_conversions` | core-safe domain↔DTO conversions (session, message, provider, config) |
| `resilience` | CircuitBreaker, FallbackProvider |
| `session` | session storage, message history, checkpointing |
| `snapshot` | file state capture and restore |
| `storage` | SQLite initialization, connection pooling |
| `task_state` | task state tracking |
| `worktree` | git worktree management |

### Re-exports into Root

Root `src/lib.rs` re-exports these modules so downstream code can use `crate::bus`, `crate::session`, etc.:

```rust
pub use codegg_core::bus;
pub use codegg_core::goal;
pub use codegg_core::identity;
pub use codegg_core::memory;
pub use codegg_core::model_profile;
pub use codegg_core::resilience;
pub use codegg_core::session;
pub use codegg_core::snapshot;
pub use codegg_core::storage;
pub use codegg_core::task_state;
pub use codegg_core::worktree;
```

### Root-Side Modules (intentionally not moved)

These modules remain in root `src/` due to high coupling with UI/server/agent:

| Module | Reason |
|--------|--------|
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
| `crypto` | AES-256-GCM encryption |
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

### Dependencies (Cargo.toml)

`codegg-core` depends on sibling workspace crates and external libraries, but **never** on UI/server/plugin crates:

```
codegg-config, codegg-protocol, codegg-providers
egggit, egglsp, eggsentry
anyhow, base64, reqwest, async-trait, chrono, dashmap, dirs,
once_cell, parking_lot, rand, regex, serde, serde_json, sha2,
similar, sqlx, thiserror, tokio, tracing, uuid
```

### Forbidden Dependencies

`codegg-core` must NOT depend on:

- **UI**: `ratatui`, `crossterm`, `ratatui_textarea`
- **Server**: `axum`, `tower_http`, `tokio_tungstenite`
- **Plugin**: `wasmtime`, `wasmtime_wasi`

Run `./scripts/check-core-boundary.sh` to verify no forbidden imports or dependencies have crept in.

### Why Root `src/error.rs` Still Exists

Root `src/error.rs` re-exports `codegg_core::error::*` and adds Axum-specific response wrappers (`AxumAppError`, `AxumServerRuntimeError`) behind `#[cfg(feature = "server")]`. This avoids pulling `axum` into `codegg-core`.

### Why Protocol Conversions Are Split

- `crates/codegg-core/src/protocol_conversions.rs`: Core-safe conversions (session, message, provider, config) that don't depend on agent/server runtime.
- `src/protocol_conversions.rs`: Agent-specific conversions + re-export of core conversions via `pub use codegg_core::protocol_conversions::*;`.

`codegg-protocol` must not depend on domain/runtime crates; conversions intentionally live outside it.

### Next Likely Extraction Target

The daemon/agent/tool/permission boundary, not TUI. `src/core/daemon.rs` is the next candidate but requires resolving agent coupling first.

---

## `src/core/` Module (Transport Facade)

**Location**: `src/core/`

The `core` module is the request/response facade that separates TUI transport from the underlying agent and session logic.

### Key Responsibilities

- Provide a typed request/response boundary for UI and transport adapters
- Centralize session, memory, task, worktree, permission, and question operations
- Support in-process, stdio, and socket-backed execution modes
- Bridge core events into the global event bus when running in-process

### Runtime Boundary Modules

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `core::runtime_deps` | `CoreRuntimeDeps`, `LegacyAgentRuntimeDeps` | Bundles optional runtime dependencies (pool, memory_store, legacy_agent, turn_runtime) so `CoreDaemon` doesn't import concrete agent/tool types directly. Always has a default TurnRuntime; override via `with_turn_runtime()`. `LegacyAgentRuntimeDeps` is a transitional container grouping `subagent_pool` and `bg_scheduler` — these will eventually be absorbed into the turn runtime abstraction. Construct via `from_parts()` (preferred) or legacy `new()`. |
| `agent::agent_loop_factory` | `AgentLoopFactory` (transitional), `AgentLoopBuildInput`, `DefaultAgentLoopFactory` | Build-only factory trait used internally by `DefaultTurnRuntime`. Not for daemon injection; use TurnRuntime instead. |
| `agent::turn_runtime` | `TurnRuntime`, `TurnRunInput`, `TurnRunOutput`, `DefaultTurnRuntime` | Execution-oriented trait that owns tool registry, permission checker, agent loop construction, system prompt assembly, and turn execution. Daemon delegates to this instead of building tools/permissions inline |
| `agent::task_tool_runtime` | `TaskToolRuntime` | Narrow DTO extracting task/subagent tool construction from `SubAgentPool` |

### `CoreClient`

```rust
#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}
```

`subscribe()` is event-capable for the in-process client. The stdio and socket clients currently expose request/response transport and return an empty receiver.

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process. Constructed via `with_deps(CoreRuntimeDeps)` (preferred) or legacy `new(pool, subagent_pool, memory_store, bg_scheduler)`. Contains a `deps: CoreRuntimeDeps` field bundling `pool`, `memory_store`, `legacy_agent: LegacyAgentRuntimeDeps` (which holds `subagent_pool` and `bg_scheduler`), and `turn_runtime` (always present, defaults to `DefaultTurnRuntime`). `subscribe()` reads from GlobalEventBus and forwards events to the channel. Turn execution (spawned async) publishes `AgentFinished`/`Error` events to the bus. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to a Unix socket endpoint and exchanges JSONL requests/responses |

### Protocol

Defined in `crates/codegg-protocol/src/core.rs`.

#### Envelopes

| Type | Purpose |
|------|---------|
| `RequestEnvelope<T>` | Wraps requests with `protocol_version` and `request_id` |
| `EventEnvelope<T>` | Wraps events with sequence, timestamp, and optional session/turn metadata |
| `CoreRequest` | Typed requests for sessions, turns, memory, tasks, worktrees, permissions, questions, and model refresh |
| `CoreResponse` | Typed responses for acknowledgements, JSON payloads, sessions, and errors |
| `CoreEvent` | Core-side event stream for in-process subscribers |

#### Request Families

- Session lifecycle: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, create-from-template, initialize, subscribe, resume
- Turn lifecycle: submit, cancel, steer, agent select, model select
- Session data: message loading and message counts
- Operational helpers: model refresh, permission/question response, memory CRUD, task CRUD/scheduling, worktree listing

#### Request Handler Behavior

**Handled variants** (produce meaningful response):
- `TurnSubmit` — Spawns agent loop, returns `Ack` immediately
- `SessionMessagesLoad` / `SessionMessageCounts` — Returns session data
- `SessionCreate` / `SessionLoad` / `SessionAttach` — Session operations
- All other session variants (List, Fork, Delete, Archive, Restore, Share, Unshare, Rename, Export, Import, CreateFromTemplate)
- `PermissionRespond` / `QuestionRespond` — Registry responses
- `ModelsRefresh` — Returns refreshed model list
- `TaskList` / `TaskSchedule` / `TaskDelete` — Task operations
- `MemoryList` / `MemorySearch` / `MemoryRemember` / `MemoryForget` — Memory operations
- `WorktreeList` — Returns worktree list

**Fallthrough variants** (return `Ack` without processing):
- `Initialize`, `Subscribe`, `Resume`, `TurnCancel`, `TurnSteer`, `AgentSelect`, `ModelSelect`

### Transport Modes

| Mode | Description |
|------|-------------|
| DaemonClient (default) | Connects to (or auto-starts) the user-scoped singleton daemon via `connect_or_start_daemon` (`src/core/instance.rs`). Uses `SocketCoreClient`. |
| StandaloneInproc | Runs the core in the current process via `InprocCoreClient`. Visible non-production mode; requires `--standalone`. |
| StandaloneStdio | Spawns `codegg core-stdio` via `StdioCoreClient`. Compatibility/testing; requires `--stdio`. |

Selection: `CoreRuntimeMode` enum (default `DaemonClient`). `--standalone` maps to `StandaloneInproc`; `--stdio` maps to `StandaloneStdio`. Legacy `--core-transport inproc|stdio` still parses but emits a deprecation warning.

### Singleton Lifecycle

Phase 1 establishes the production invariant that exactly one user-scoped Codegg daemon owns execution at a time. All implementation lives in `src/core/instance.rs`.

**`DaemonPaths`** resolves all per-user daemon artifacts:

| Path | Purpose |
|------|---------|
| `daemon.lock` | Advisory exclusive lock (`flock(LOCK_EX \| LOCK_NB)`) — authoritative identity |
| `daemon.json` | Atomic metadata record (diagnostic only) |
| `core.sock` | Unix domain socket the daemon binds |
| `daemon.log` | Debug log (best-effort) |

Production locations: macOS `$HOME/Library/Application Support/codegg`, Linux `${XDG_RUNTIME_DIR:-/tmp}/codegg`. Override via `CODEGG_DAEMON_HOME`.

**`DaemonInstanceGuard`** is an RAII guard that holds the flock for the daemon's lifetime. On drop it removes the metadata file (if owned by this guard) and releases the lock. The OS also releases the flock automatically on process exit.

**`DaemonInstanceMetadata`** (`daemon.json`) carries: `daemon_id`, `generation` (UUID), `pid`, `socket_path`, `protocol_version`, `started_at`, `binary_version`. Written atomically (temp file + rename) after socket bind. The lock is authoritative; metadata is diagnostic.

**`connect_or_start_daemon`** is the canonical frontend entry point (`src/core/instance.rs`). It tries connecting to the user-scoped endpoint; if absent and autostart is enabled, spawns a child daemon process (`codegg daemon start`) and polls for readiness with a bounded timeout. Returns a live `SocketCoreClient` on success.

**`CoreRuntimeMode`** enum:
- `DaemonClient` (default) — connect-or-start against the singleton daemon
- `StandaloneInproc` — in-process core, no daemon interaction (`--standalone`)
- `StandaloneStdio` — `core-stdio` subprocess (`--stdio`)

`InprocCoreClient` is now only used by tests, embedding, and `--standalone` mode. The default TUI uses `SocketCoreClient` through `connect_or_start_daemon`.

The `PROTOCOL_VERSION = 2` constant is unchanged in this phase. The `generation` UUID lives in the on-disk metadata file, not in the wire protocol.

### Workspace Registry and Execution Context (Phase 2)

Phase 2 introduces workspace identity as a first-class daemon concept. A daemon may now serve multiple distinct workspaces (project roots) and must track which workspace each execution context targets.

**`WorkspaceRegistry`** (`crates/codegg-core/src/workspace.rs`) is daemon-owned and deduplicates canonical roots via `get_or_register`. Rejects nonexistent paths and symlink aliases. `CoreDaemon` holds `workspaces: Arc<WorkspaceRegistry>`.

**`ExecutionContext`** (`crates/codegg-core/src/workspace.rs`) is immutable and passed by `Arc` through `TurnRunInput` to every daemon execution path. Replaces `std::env::current_dir()` reasoning. Carries `workspace_root`, `workspace_id`, `session_id`, and path policy. `TurnRunInput` has `execution: Arc<ExecutionContext>`.

**`WorkspaceId`** is a typed `String` newtype identifying a registered workspace.

**Session binding**: `CoreDaemon::bind_runtime_for_session` resolves a `session_id` to a `SessionRuntime` via `SessionStore` + `WorkspaceRegistry`. `TurnSubmit`, `AgentSelect`, and `ModelSelect` reject unbound sessions.

**Storage migration v22**: adds a `workspace` table and `workspace_id` index on `session`. Existing sessions are lazily resolved on next access; their `directory` is canonicalized into a workspace record.

**Protocol**: `WorkspaceSnapshot` DTO, `CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`, `SessionSnapshot::workspace_id` + `directory`, `ServerCapabilities::workspace_registration` + `workspace_snapshots`.

**Static guard**: `scripts/check_daemon_cwd_usage.py` scans protected modules for `std::env::current_dir()` usage. Existing legacy uses in tool `default()` constructors are allowlisted; new production-path uses fail CI.

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

- The core protocol version is currently `2` (`PROTOCOL_VERSION` in `crates/codegg-protocol/src/core.rs`).
- Local TUI flows should prefer `CoreClient` over direct store access when a request already exists in `CoreRequest`.
- The in-process client subscribes to the GlobalEventBus and forwards events to the channel receiver. Actual event publishing happens inside `tokio::spawn` within turn execution handlers.
- `CoreDaemon` uses `CoreRuntimeDeps` to bundle runtime dependencies. The legacy `new(pool, subagent_pool, memory_store, bg_scheduler)` constructor is retained for backward compatibility. Prefer `from_parts(pool, memory_store, legacy_agent, turn_runtime)` for new code, or `with_turn_runtime()` to override the default turn runtime.
- Turn execution goes through the injected `TurnRuntime` trait (`agent::turn_runtime`). `CoreRuntimeDeps` always holds an `Arc<dyn TurnRuntime>` (defaults to `DefaultTurnRuntime`); the daemon calls `deps.turn_runtime.run_turn(input)` instead of constructing a runtime directly. The runtime owns tool registry construction, permission checker construction, agent loop construction, system prompt assembly, and background spawning.
- `AgentLoopFactory` (build-only trait, formerly `AgentRuntimeProvider`) is transitional and used only internally by `DefaultTurnRuntime`. New code should prefer `TurnRuntime`.
- `src/core/daemon.rs` has zero direct references to `AgentLoop`, `ToolRegistry`, `PermissionChecker`, `TaskToolRuntime`, or `build_session_tool_registry`.
- Daemon provider validation is intentionally duplicated (daemon validates provider existence before delegating to turn runtime) to preserve backward-compatible provider_not_found response shape.
- Daemon still owns: request validation, session_id/turn_id management, active-turn bookkeeping, TurnStarted event publishing, and CoreResponse return.

### Test Coverage

- `turn_submit_uses_injected_runtime` (`src/core/daemon.rs:3100`) — Verifies that `TurnSubmit` delegates to the injected `TurnRuntime` rather than constructing one inline.
## Project context resolver

Daemon request handlers use the core-owned `ProjectContextResolver` for session
creation, loading, turns, and project-scoped listing. It performs bounded input
parsing and durable membership/lifecycle checks before execution. The resolver
does not authorize principals and does not scan the filesystem or use process
cwd as identity authority.
