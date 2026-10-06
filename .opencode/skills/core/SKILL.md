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

Semantic model routing is a separate optional agent-loop capability. It may
resolve a configured `virtual:<name>` model to a concrete model, but it must
never replace a durable session `ProviderConnectionId`, connection revision,
credential, lifecycle decision, or the already-selected per-turn provider
object. The route must be compatible with that selected connection; direct
cross-provider routes fail closed, while an EggPool connection may pass the
concrete model to EggPool's own account/provider router. `sticky` and
`affinity_ttl_s` are retained shared-policy fields, but Codegg does not
implement local affinity caching. See `architecture/agent.md` and
`architecture/config.md`.

## Request-Family Routing (Residual M002)

`CoreDaemon` is the single composition/lifecycle authority. Request
handling is physically decomposed by family; do not add a new generic
coordinator, service bus, or DI framework:

`DaemonRequestFamily` has 15 variants; 11 are dispatched by the family
router in `handle_request_with_client`, and 4 (chat, team, interactive,
work orders) return earlier through their own pre-router arms.

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
| documents | `handle_document_request` | `src/core/daemon_documents.rs` |
| lsp | `handle_lsp_request` | `src/core/daemon_lsp.rs` |
| chat | `handle_chat_request` | `src/core/daemon.rs` |
| team | `handle_team_request` | `src/core/daemon_team.rs` |
| work orders | `handle_work_order_request` | `src/core/daemon_work_orders.rs` |
| interactive | `run_interactive_request` | `src/core/daemon.rs` |

Two further `daemon_*` modules are sub-handlers rather than routed
families: `daemon_control.rs` (`handle_control_request`, called from the
turns family) and `daemon_workspace_dashboard.rs`
(`handle_workspace_dashboard_request`, called from the projects family).

- `DaemonRequestFamily::of` (`src/core/daemon_family.rs`) is the only
  request-to-owner routing table, and `DaemonRequestFamily::owner_module`
  is the module map for the same enum. New `CoreRequest` variants must be
  classified there first; the routing test fails otherwise.
- `handle_request_with_client` stays a thin router: authorization/audit
  preamble, boxed chat pre-router, boxed work-order pre-router, boxed team
  pre-router, spawned interactive-process pre-router, then one `Box::pin`
  delegate per routed family. Keep it that way.
- Family handlers are boring `impl CoreDaemon` methods on shared
  daemon-owned state. Shared request helpers live in `src/core/daemon.rs`
  as `pub(crate)`; lifecycle helpers live canonically in their owning
  lifecycle modules below, not in request handlers.

## Lifecycle Ownership (Residual M003)

`CoreDaemon` is still the single composition/lifecycle authority. Lifecycle
code is grouped by responsibility; do not add a new generic coordinator,
service bus, or DI framework:

| Responsibility | Entry points | Module |
|---|---|---|
| construction | `with_deps`, `with_deps_and_identity`, `new`, `SeamProjectionSink` | `src/core/daemon_construct.rs` |
| bootstrap/recovery | `hydrate_workspace_registry`, `recover_state`, `recover_jobs`, `start_event_bridge`, `initialize_recovery_sequence`, `replay_from` | `src/core/daemon_bootstrap.rs` |
| refresh | `refresh_project_context`, `refresh_project_activation`, `activate_project_workspace`, `project_health`, `refresh_runtime_assets` + binding resolvers | `src/core/daemon_refresh.rs` |
| shutdown/join | `Drop`, `abort_background_handles` | `src/core/daemon_shutdown.rs` |

- `initialize_recovery_sequence` is the canonical in-process hydrate ->
  bridge -> recover order (`InprocCoreClient::initialize_recovery`
  delegates to it). Socket/daemon paths keep their pre-existing bridge ->
  recover shape; do not reorder startup to "fix" it here.
- Construction has a single assembly point with no partial publish;
  shutdown aborts projection-maintenance then worktree-reconcile in order
  (scheduler loop stays detached by design).
- New lifecycle behavior needs explicit ordering tests at the extracted seam,
  not a line-count gate.

## Core Client Types

| Type | Use |
|------|-----|
| `SocketCoreClient` | Default local mode; connects to the user-scoped singleton daemon through the platform-local endpoint |
| `InprocCoreClient` | Test/embedding mode; runs the core in the current process. Requires `--standalone`. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests over stdin/stdout. Requires `--stdio`. |

## Singleton Lifecycle

Codegg runs exactly one user-scoped daemon per OS user. The lock and metadata live at:

| OS | Default root (`DaemonPaths::resolve`) |
|----|-----------------|
| macOS | `$HOME/Library/Application Support/codegg` |
| Linux | `$XDG_RUNTIME_DIR/codegg`, then `$XDG_DATA_HOME/codegg`, then `$HOME/.local/share/codegg` |
| other Unix | `/tmp/codegg` |

Inside that root: `daemon.lock`, `daemon.json` (metadata),
`core.sock` (or a Windows named pipe), and `daemon.log`. Override the root
with `CODEGG_DAEMON_HOME`; override only the socket with
`CODEGG_CORE_ENDPOINT` or `DaemonPaths::resolve_for_endpoint`. Key types in
`src/core/instance.rs`:

- **`DaemonPaths`** — resolves lock, metadata, socket, and log paths
- **`DaemonInstanceGuard`** — RAII guard holding a nonblocking `File::try_lock` for the daemon's lifetime
- **`DaemonInstanceMetadata`** — atomic `daemon.json` record (diagnostic only; lock is authoritative)
- **`CoreRuntimeMode`** — `DaemonClient` (default), `StandaloneInproc`, `StandaloneStdio`
- **`connect_or_start_daemon`** — canonical frontend entry point; connects to the running daemon or auto-starts one

## Protocol Basics

Core requests and responses are defined in the `codegg-protocol` workspace crate (`crates/codegg-protocol`), re-exported as `codegg::protocol` via `src/lib.rs`. The root-level `src/protocol/` directory no longer exists.

Important points:
- `protocol_version` is part of every request envelope
- `subscribe()` is event-capable for `InprocCoreClient` and
  `SocketCoreClient`; the socket client forwards the bounded event stream
  from `codegg-client`'s `LocalSocketClient`
- `StdioCoreClient` currently provides request/response transport only and
  returns an empty event receiver

## Transport Selection

Local TUI startup selects core transport via `CoreRuntimeMode` (default `DaemonClient`):

1. `--standalone` → `StandaloneInproc` (in-process core, no daemon interaction)
2. `--stdio` → `StandaloneStdio` (core-stdio subprocess)
3. Default → `DaemonClient` (connect-or-start against the user-scoped singleton daemon)

Legacy `--core-transport inproc|stdio` still parses but emits a deprecation warning.

Ordinary `codegg --help` shows only user operations: `--standalone` stays
visible while `--core-transport`, `--stdio`, and `--core-endpoint` still
parse but are hidden, as is the `core-stdio` entry point. Local daemon
attachment is `codegg daemon attach` (top-level `attach-daemon` remains
as a hidden compatibility alias).

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
- `TurnSubmit` and `AgentSelect` bind through `bind_runtime_for_session` and
  return `session_unbound` when binding fails. `ModelSelect` does not bind:
  it is a durable `SelectionService` adapter that validates session existence
  through `SessionStore` and only then projects the canonical model into the
  runtime cache best-effort.

### Storage

The workspace table was introduced by migration v22 (a `workspace` table plus
`workspace_id` index on `session`). The schema has advanced well past that:
the current layout version is defined by `storage::STORAGE_LAYOUT_VERSION`
(`crates/codegg-core/src/storage/mod.rs`). Existing sessions are lazily
resolved on next access; their `directory` is canonicalized into a workspace
record.

### Protocol

- `WorkspaceSnapshot` DTO for workspace state serialization
- `CoreRequest::WorkspaceRegister|WorkspaceList|WorkspaceArchive|WorkspaceSnapshotRequest`
  (raw `WorkspaceRegister`, global `WorkspaceList`, and unscoped
  `ProjectRegister` are LocalOwner/proven-local only: `opaque` scope, so
  team principals fail closed with `authorization_scope_required`; a
  `workspace_id` locator never proves ownership; project-scoped workspace
  mutation under `project.configure` remains available)
- `SessionSnapshot::workspace_id` + `directory`
- `ServerCapabilities::workspace_registration` + `workspace_snapshots`

`ServerCapabilities::workspace_registration` appears on both the client- and
server-side capability structs in `crates/codegg-protocol/src/frames.rs`;
`workspace_snapshots` only on the server-side one.

### Static Guard

`scripts/check_daemon_cwd_usage.py` scans protected modules for `std::env::current_dir()` usage. Legacy uses in tool `default()` constructors are allowlisted; new production-path uses fail CI.

See `architecture/core.md` "Workspace Registry and Execution Context (Phase 2)" and `crates/codegg-core/src/workspace.rs` for the full contract.

## CoreRuntimeDeps

`CoreRuntimeDeps` (`src/core/runtime_deps.rs`) is the daemon dependency bundle.
The legacy three-argument constructor is `CoreRuntimeDeps::new(pool,
subagent_pool, memory_store)`; the struct itself carries far more:
`pool`, `memory_store`, `legacy_agent`, `agent_run_store`, `run_control`,
`run_group_service`, `convergence_store`, `turn_runtime`, `lsp_service`,
`workspace_services`, `worktree_service`, `workspace_service_policy`,
`job_store`, `schedule_store`, `recovery_policy`, `daemon_generation`,
`scheduler`, `submission`, `scheduler_config`, and `connection_manager`.
`turn_runtime` is always present and defaults to
`DefaultTurnRuntime`; override via `with_turn_runtime()`.

## Maintenance Rules

- Prefer `CoreClient` over direct `SessionStore` or `MessageStore` access when a request already exists in `CoreRequest`
- If a new UI action needs backend state, add the request to `CoreRequest` before wiring the TUI directly to storage
- Keep `core` protocol changes aligned with `architecture/core.md`, `architecture/tui.md`, `architecture/client.md`, and `architecture/server.md`

## Source verification

Verified 2026-10-06 against `src/core/daemon_family.rs`,
`src/core/daemon.rs`, `src/core/daemon_construct.rs`,
`src/core/daemon_bootstrap.rs`, `src/core/daemon_refresh.rs`,
`src/core/daemon_shutdown.rs`, `src/core/daemon_turns.rs`,
`src/core/instance.rs`, `src/core/mod.rs`, `src/core/runtime_deps.rs`,
`src/core/transport/{socket,stdio}.rs`, `src/main.rs`,
`crates/codegg-client/src/paths.rs`,
`crates/codegg-core/src/workspace.rs`,
`crates/codegg-core/src/storage/mod.rs`, and
`crates/codegg-protocol/src/{core,frames,dto}.rs`. Corrected the
request-family table, which listed 9 of the 15 `DaemonRequestFamily`
variants and omitted the `daemon_control` / `daemon_workspace_dashboard`
sub-handler modules; the router bullet, which omitted the work-order and
team pre-routers; the `subscribe()` contract, which wrongly called the
local-socket client event-less; the `ModelSelect` unbound-session claim,
which does not go through `bind_runtime_for_session`; the `CoreRuntimeDeps`
field list, which omitted `agent_run_store`, `run_control`,
`run_group_service`, `convergence_store`, and `worktree_service` and
mis-stated the legacy constructor arity; and the daemon-home table, which
gave a Linux default of `${XDG_RUNTIME_DIR:-/tmp}` instead of the actual
`XDG_RUNTIME_DIR` → `XDG_DATA_HOME` → `$HOME/.local/share` →
`/tmp` cascade. Claims without a traceable source were removed rather than
guessed.
