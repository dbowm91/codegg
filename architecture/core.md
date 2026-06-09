# Core Architecture

This document covers two distinct "core" concerns:

1. **`codegg-core` workspace crate** — domain types, storage, bus, and state
2. **`src/core/` module** — the daemon/transport facade (CoreClient, InprocCoreClient, etc.)

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
| `core::runtime_deps` | `CoreRuntimeDeps` | Bundles optional runtime dependencies (pool, memory_store, subagent_pool, bg_scheduler, turn_runtime) so `CoreDaemon` doesn't import concrete agent/tool types directly. Always has a default TurnRuntime; override via with_turn_runtime(). |
| `agent::runtime_provider` | `AgentRuntimeProvider` (transitional), `AgentLoopBuildInput`, `DefaultAgentRuntimeProvider` | Build-only factory trait used internally by `DefaultTurnRuntime`. Not for daemon injection; use TurnRuntime instead. |
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
| `InprocCoreClient` | Runs the core in the current process. Constructed via `with_deps(CoreRuntimeDeps)` (preferred) or legacy `new(pool, subagent_pool, memory_store, bg_scheduler)`. Contains a `deps: CoreRuntimeDeps` field bundling `pool`, `memory_store`, `subagent_pool`, `bg_scheduler`, and `turn_runtime` (always present, defaults to `DefaultTurnRuntime`). `subscribe()` reads from GlobalEventBus and forwards events to the channel. Turn execution (spawned async) publishes `AgentFinished`/`Error` events to the bus. |
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
| In-Process | Default. Keeps the core in the same binary via `InprocCoreClient`. |
| Stdio | Started with `core-stdio` command. Reads/writes JSONL on stdin/stdout. |
| Socket | Connects to a Unix socket endpoint with reconnect-and-retry-once. |

Selection: `--core-transport` flag → `CODEGG_CORE_TRANSPORT` env → default `inproc`.

### Implementation Notes

- The core protocol version is currently `1`.
- Local TUI flows should prefer `CoreClient` over direct store access when a request already exists in `CoreRequest`.
- The in-process client subscribes to the GlobalEventBus and forwards events to the channel receiver. Actual event publishing happens inside `tokio::spawn` within turn execution handlers.
- `CoreDaemon` uses `CoreRuntimeDeps` to bundle runtime dependencies. The legacy `new(pool, subagent_pool, memory_store, bg_scheduler)` constructor is retained for backward compatibility.
- Turn execution goes through the injected `TurnRuntime` trait (`agent::turn_runtime`). `CoreRuntimeDeps` always holds an `Arc<dyn TurnRuntime>` (defaults to `DefaultTurnRuntime`); the daemon calls `deps.turn_runtime.run_turn(input)` instead of constructing a runtime directly. The runtime owns tool registry construction, permission checker construction, agent loop construction, system prompt assembly, and background spawning.
- `AgentRuntimeProvider` (build-only trait) is transitional and used only internally by `DefaultTurnRuntime`. New code should prefer `TurnRuntime`.
- `src/core/daemon.rs` has zero direct references to `AgentLoop`, `ToolRegistry`, `PermissionChecker`, `TaskToolRuntime`, or `build_session_tool_registry`.
- Daemon provider validation is intentionally duplicated (daemon validates provider existence before delegating to turn runtime) to preserve backward-compatible provider_not_found response shape.
- Daemon still owns: request validation, session_id/turn_id management, active-turn bookkeeping, TurnStarted event publishing, and CoreResponse return.
