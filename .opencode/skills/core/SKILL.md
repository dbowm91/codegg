---
name: core
description: Core facade and transport adapters for TUI/session separation
version: 1.1.0
tags:
  - core
  - transport
  - tui
  - session
  - protocol
---

# Core Facade Guide

This skill covers the `src/core/` module, which is the request/response boundary between the TUI and the underlying agent/session logic.

## What Core Owns

- Session lifecycle operations: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, and template creation
- Session message access and message counts
- Turn submission and turn control
- Permission/question response routing
- Memory, task, and worktree helpers
- Model refresh and agent/model selection helpers

## Request-Family Routing (Residual M002)

`CoreDaemon` is the single composition/lifecycle authority. Request
handling is physically decomposed by family; do not add a new generic
coordinator, service bus, or DI framework:

| Family | Handler | Module |
|---|---|---|
| assets | `handle_assets_request` | `src/core/daemon_assets.rs` |
| providers | `handle_providers_request` | `src/core/daemon_providers.rs` |
| sessions | `handle_sessions_request` | `src/core/daemon_sessions.rs` |
| turns | `handle_turns_request` | `src/core/daemon_turns.rs` |
| jobs | `handle_jobs_request` | `src/core/daemon_jobs.rs` |
| projects | `handle_projects_request` | `src/core/daemon_projects.rs` |
| goals | `handle_goals_request` | `src/core/daemon_goals.rs` |
| projection | `handle_projection_request` | `src/core/daemon_projection.rs` |
| ops | `handle_ops_request` | `src/core/daemon_ops.rs` |

- `DaemonRequestFamily::of` (`src/core/daemon_family.rs`) is the only
  request-to-owner routing table. New `CoreRequest` variants must be
  classified there first; the routing test fails otherwise.
- `handle_request_with_client` stays a thin router: authorization/audit
  preamble, boxed chat pre-router, spawned interactive-process
  pre-router, then one `Box::pin` delegate per family. Keep it that way.
- Family handlers are boring `impl CoreDaemon` methods on shared
  daemon-owned state. Shared helpers live in `src/core/daemon.rs` as
  `pub(crate)`; construction/lifecycle extraction belongs to M003, not
  to request handlers.

## Core Client Types

| Type | Use |
|------|-----|
| `SocketCoreClient` | Default local mode; connects to the user-scoped singleton daemon via Unix socket |
| `InprocCoreClient` | Test/embedding mode; runs the core in the current process. Requires `--standalone`. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests over stdin/stdout. Requires `--stdio`. |

## Singleton Lifecycle

Codegg runs exactly one user-scoped daemon per OS user. The lock and metadata live at:

| OS | Default location |
|----|-----------------|
| macOS | `$HOME/Library/Application Support/codegg/daemon.lock` |
| Linux | `${XDG_RUNTIME_DIR:-/tmp}/codegg/daemon.lock` |

Override with `CODEGG_DAEMON_HOME`. Key types in `src/core/instance.rs`:

- **`DaemonPaths`** — resolves lock, metadata, socket, and log paths
- **`DaemonInstanceGuard`** — RAII guard holding `flock(LOCK_EX | LOCK_NB)` for the daemon's lifetime
- **`DaemonInstanceMetadata`** — atomic `daemon.json` record (diagnostic only; lock is authoritative)
- **`CoreRuntimeMode`** — `DaemonClient` (default), `StandaloneInproc`, `StandaloneStdio`
- **`connect_or_start_daemon`** — canonical frontend entry point; connects to the running daemon or auto-starts one

## Protocol Basics

Core requests and responses are defined in the `codegg-protocol` workspace crate (`crates/codegg-protocol`), re-exported as `codegg::protocol` via `src/lib.rs`. The root-level `src/protocol/` directory no longer exists.

Important points:
- `protocol_version` is part of every request envelope
- `subscribe()` only emits live events for the in-process client today
- stdio/socket clients currently provide request/response transport and return an empty event receiver

## Transport Selection

Local TUI startup selects core transport via `CoreRuntimeMode` (default `DaemonClient`):

1. `--standalone` → `StandaloneInproc` (in-process core, no daemon interaction)
2. `--stdio` → `StandaloneStdio` (core-stdio subprocess)
3. Default → `DaemonClient` (connect-or-start against the user-scoped singleton daemon)

Legacy `--core-transport inproc|stdio` still parses but emits a deprecation warning.

## Workspace Registry and Execution Context (Phase 2)

Phase 2 introduces workspace identity as a first-class daemon concept. The daemon serves multiple distinct workspaces and tracks which workspace each execution targets.

### Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `WorkspaceId` | `crates/codegg-core/src/workspace.rs` | Typed `String` newtype identifying a registered workspace |
| `WorkspaceRegistry` | `crates/codegg-core/src/workspace.rs` | Daemon-owned registry; deduplicates canonical roots via `get_or_register` |
| `ExecutionContext` | `crates/codegg-core/src/workspace.rs` | Immutable, `Arc`-wrapped context carrying `workspace_root`, `workspace_id`, `session_id`, and path policy |

### Binding Contract

- `CoreDaemon` holds `workspaces: Arc<WorkspaceRegistry>`.
- `TurnRunInput` carries `execution: Arc<ExecutionContext>`.
- `CoreDaemon::bind_runtime_for_session` resolves a `session_id` to a `SessionRuntime` via `SessionStore` + `WorkspaceRegistry`.
- `TurnSubmit`, `AgentSelect`, and `ModelSelect` reject unbound sessions.

### Storage

The workspace table was introduced by migration v22 (a `workspace` table plus
`workspace_id` index on `session`). The schema has advanced well past that:
the current layout version is `STORAGE_LAYOUT_VERSION = 36`
(`crates/codegg-core/src/storage/mod.rs`). Existing sessions are lazily
resolved on next access; their `directory` is canonicalized into a workspace
record.

### Protocol

- `WorkspaceSnapshot` DTO for workspace state serialization
- `CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`
- `SessionSnapshot::workspace_id` + `directory`
- `ServerCapabilities::workspace_registration` + `workspace_snapshots`

### Static Guard

`scripts/check_daemon_cwd_usage.py` scans protected modules for `std::env::current_dir()` usage. Legacy uses in tool `default()` constructors are allowlisted; new production-path uses fail CI.

See `architecture/core.md` "Workspace Registry and Execution Context (Phase 2)" and `crates/codegg-core/src/workspace.rs` for the full contract.

## CoreRuntimeDeps

`CoreRuntimeDeps` (`src/core/runtime_deps.rs`) is the daemon dependency bundle.
It carries far more than the legacy four: `pool`, `memory_store`,
`legacy_agent`, `turn_runtime`, plus `lsp_service`, `workspace_services`,
`workspace_service_policy`, `job_store`, `schedule_store`, `recovery_policy`,
`daemon_generation`, `scheduler`, `submission`, `scheduler_config`, and
`connection_manager`. It always holds a default `TurnRuntime`; override via
`with_turn_runtime()`.

## Maintenance Rules

- Prefer `CoreClient` over direct `SessionStore` or `MessageStore` access when a request already exists in `CoreRequest`
- If a new UI action needs backend state, add the request to `CoreRequest` before wiring the TUI directly to storage
- Keep `core` protocol changes aligned with `architecture/core.md`, `architecture/tui.md`, `architecture/client.md`, and `architecture/server.md`
