---
name: core
description: Core facade and transport adapters for TUI/session separation
version: 1.0.0
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

Core requests and responses are defined in `src/protocol/core.rs` and wrapped in `RequestEnvelope` / `CoreResponse`.

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

Migration v22 adds a `workspace` table and `workspace_id` index on `session`. Existing sessions are lazily resolved on next access; their `directory` is canonicalized into a workspace record.

### Protocol

- `WorkspaceSnapshot` DTO for workspace state serialization
- `CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`
- `SessionSnapshot::workspace_id` + `directory`
- `ServerCapabilities::workspace_registration` + `workspace_snapshots`

### Static Guard

`scripts/check_daemon_cwd_usage.py` scans protected modules for `std::env::current_dir()` usage. Legacy uses in tool `default()` constructors are allowlisted; new production-path uses fail CI.

See `architecture/core.md` "Workspace Registry and Execution Context (Phase 2)" and `plans/single-daemon-phase-02-workspace-registry-and-execution-context.md` for the full contract.

## Maintenance Rules

- Prefer `CoreClient` over direct `SessionStore` or `MessageStore` access when a request already exists in `CoreRequest`
- If a new UI action needs backend state, add the request to `CoreRequest` before wiring the TUI directly to storage
- Keep `core` protocol changes aligned with `architecture/core.md`, `architecture/tui.md`, `architecture/client.md`, and `architecture/server.md`
