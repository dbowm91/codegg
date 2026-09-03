# CodeGG Architecture Overview

CodeGG is a high-performance AI coding agent built in Rust, designed for terminal-based interaction with deep IDE and LSP integration. This document provides a bird's eye view of how the system works as a whole and serves as the index into detailed architecture documents — one per discrete module/component — in this directory.

## Deployment topology

The supported distribution remains one `codegg` executable per target. The daemon is a user-scoped singleton discovered and started by that executable; the TUI and daemon share the same business-logic libraries and invocation contracts. Runtime-safety milestone M007 measured the dependency and release profiles and retained this no-split topology because a daemon/TUI split did not produce a material deployment improvement without adding packaging or ownership complexity. Future role-specific binaries require a new measured deployment constraint and compatibility plan; they are not an advertised installation mode.

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          Terminal (Ratatui)                         │
│                    Input ─────► TUI ─────► Output                   │
└────────────────────────┬────────────────────────────────────────────┘
                         │
              ┌──────────▼──────────┐
              │   CoreClient       │  Inproc / Stdio / Socket
              │  (Request/Response) │
              └──────────┬──────────┘
                         │
┌────────────────────────▼────────────────────────────────────────────┐
│                       AgentLoop                                      │
│  ┌─────────┐   ┌──────────┐   ┌─────────┐   ┌──────────────────┐    │
│  │ Provider│──▶│Messages  │◀──│  Tools  │◀──│ PermissionChecker│    │
│  └─────────┘   └──────────┘   └─────────┘   └──────────────────┘    │
│        │                                                 ▲          │
│        │              ┌─────────────┐                    │          │
│        └─────────────▶│  Bus/Events │────────────────────┘          │
│                       └─────────────┘                                │
│                          │                                           │
│  ┌──────────────────────┼───────────────────────────────────────┐  │
│  │            Modules    │                                       │  │
│  │  ┌────────┐ ┌───────┐ │ ┌───────┐ ┌──────┐ ┌────────┐       │  │
│  │  │ Session│ │Memory │ │ │ MCP   │ │Plugins│ │Native  │       │  │
│  │  └────────┘ └───────┘ │ └───────┘ └──────┘ │ Crates │       │  │
│  │                                         ┌─── egglsp        │  │
│  │                                         │    egggit        │  │
│  │                                         │    codegg-git    │  │
│  │                                         │    eggsentry     │  │
│  │                                         │    eggcontext    │  │
│  │                                         └──────────────────┘  │  │
│  └──────────────────────┴───────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## How Everything Works Together

A single user prompt flows through the system along this path:

1. **Connect** — Running plain `codegg` calls `connect_or_start_daemon`
   (`src/core/instance.rs`) which connects to the running user-scoped daemon
   singleton (guarded by an authoritative `flock`) or starts one.
   `--standalone` runs an in-process core instead.
2. **Transport** — The TUI talks to the core through the `CoreClient`
   facade over Inproc, Stdio, or Socket transports ([core.md](core.md)).
   Remote TUI sessions use the Axum WebSocket server ([server.md](server.md));
   external agents use the ACP stdio adapter ([acp.md](acp.md)).
3. **Turn submission** — User input becomes a `CoreRequest`. The daemon hands
   work to `DefaultTurnRuntime.run_turn(TurnRunInput)` carrying an immutable,
   `Arc`-wrapped `ExecutionContext` (workspace root, workspace id, session id,
   path policy) — never `std::env::current_dir()` ([workspace.md](workspace.md)).
4. **Agent loop cycle** — `AgentLoop` builds the turn context (asset snapshot,
   instructions, skills, memory, task state), streams an LLM completion through
   a provider ([provider.md](provider.md)), parses tool calls, executes them,
   appends results, and repeats until the model stops calling tools
   ([agent.md](agent.md)). Compaction manages context-window overflow
   ([compaction.md](compaction.md)).
5. **Tool dispatch** — Every production tool call crosses the `ToolBroker`
   boundary: ordered policy pipeline → permission check → backend execution
   ([tool_broker.md](tool_broker.md)). Tools are either native wrappers over
   workspace crates (`egglsp`, `egggit`, `eggsentry`, …), eggsact deterministic
   validators ([deterministic_tools.md](deterministic_tools.md)), MCP servers
   ([mcp.md](mcp.md)), or WASM plugins ([plugin.md](plugin.md)).
6. **Admission control** — Heavy operations (tests, managed processes, Python
   scripts, subagent dispatch, tool programs) do not run inline: callers submit
   through `JobSubmissionService`, the only door into the global
   `JobScheduler`, which enforces fair queuing and permits before dispatching
   executors ([scheduler.md](scheduler.md)). Work is durable — jobs, attempts,
   schedules, recovery, idempotency live in codegg-core
   ([jobs.md](jobs.md)).
7. **Events and projection** — Everything that happens publishes an `AppEvent`
   on the bus ([bus.md](bus.md)). A deterministic canonical reducer folds
   events into a frontend-neutral session projection; clients subscribe to
   scoped views and replay durable history ([projection.md](projection.md)).
8. **Persistence** — Sessions, messages, todos, checkpoints, usage, goals,
   research runs, jobs, and schedules persist in SQLite (63 tables across 48
   migrations) ([session.md](session.md), [storage.md](storage.md)). Command,
   script, and test outputs land in the RunStore artifact store
   ([run_store.md](run_store.md)).
9. **Render** — The TUI applies projected state and renders
   ([tui.md](tui.md)). Human `!`/`!!` shell commands bypass the model unless
   explicitly promoted, flowing through a 10-phase safety/redaction projection
   pipeline first ([human_shell.md](human_shell.md)).

### Command Execution Pipeline

Shell-originated commands additionally flow through a 3-stage pipeline:

```
Raw Shell Command
    │
    ▼
classify_command_with_context()          ← command_intent (stage 1)
    │  Risk assessment, execution capabilities
    ▼
plan_execution()                         ← command_planner (stage 2)
    │  Backend selection, permission generation, projector policy
    ▼
resolve_routing()                        ← command_routing (stage 3)
    │  Concrete subsystem dispatch
    ▼
┌──────────────────────────────────────────────────────────┐
│ RouteToTestRunner │ RouteToShell │ RouteToPython          │
│ RouteToGit        │ RouteToNativeTool │ RouteToManagedProcess │
└──────────────────────────────────────────────────────────┘
```

Active routing (`CommandIntentMode::Active`) dispatches to structured backends;
the default `Observe` mode classifies and annotates only.

## Module Map

Modules are grouped by architectural layer. Each row links to the deep-dive document for that component.

### Agent Layer — Orchestration and Execution

The agent layer owns the core execution cycle: receiving user input, routing through the LLM, executing tools, and managing multi-agent coordination.

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Agent | Main agent loop, compaction, routing, team coordination, multi-agent orchestration | `loop.rs`, `worker.rs`, `compaction.rs`, `router.rs`, `team.rs` | [agent.md](agent.md) |
| AssetContext / Snapshot / Refresh | Explicit context, immutable snapshot builder, generation coordinator, bounded operator refresh, turn/agent-run pinning, lazy resource handles, and inert remote manifest DTOs (Runtime Assets Milestones 2–4) | `asset_context.rs`, `instructions.rs`, `asset_snapshot.rs`, `asset_snapshot_builder.rs`, `asset_refresh.rs`, `skills/resource.rs`, `codegg-protocol/src/runtime_assets.rs` | [agent.md](agent.md) |
| ACP | Agent Client Protocol v1 stdio adapter — presentation layer over the singleton daemon, JSON-RPC framing, projection event translation | `acp.rs` | [acp.md](acp.md) |
| Command Intent | Command intent classification, risk assessment, execution capability model — pipeline stage 1 | `mod.rs`, `shell_shape.rs`, `plan.rs` | [command_intent.md](command_intent.md) |
| Command Planner | Maps classified intents to execution backends, generates permissions, selects projection policy — pipeline stage 2 | `plan.rs` (re-exported via `src/command_planner.rs`) | [command_planner.md](command_planner.md) |
| Command Routing | Resolves planned execution to concrete subsystems (TestRunner, Shell, Python, Git, ManagedProcess) — pipeline stage 3 | `src/command_routing.rs` | [command_routing.md](command_routing.md) |
| Test Runner | Test command resolution, stdout/stderr parsing, failure-class taxonomy, streaming execution, previous-failures index, projection adapter | `types.rs`, `resolve.rs`, `parse.rs`, `report.rs`, `runner.rs`, `index.rs`, `projection.rs` | [test_runner.md](test_runner.md) |
| Python Script | First-class Python scripting with Analyze/Transform/Verify modes, AST risk scanning, Landlock sandboxing, workspace snapshots | `types.rs`, `analyze.rs`, `sandbox.rs`, `snapshot.rs`, `executor.rs`, `projection.rs`, `tool.rs` | [python_scripting.md](python_scripting.md) |
| Research | Structured research pipeline: source collection → evidence → claims → verification | `coordinator.rs`, `types.rs`, `store.rs`, `claims.rs`, `verify.rs` | [research.md](research.md) |

### Tool Layer — Capabilities and Execution

The tool layer defines the built-in tools the agent can invoke, the backend abstractions for dispatching, and the deterministic validation pipeline.

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Tool | Built-in tools (~31 core always registered; ~50 registration sites total once conditional todo/evidence/deterministic/context-read tools are included), `Tool` trait, `ToolCatalog`, backend abstraction (Native/MCP/Shell/Builtin) | `mod.rs`, `backend.rs`, `bash.rs`, `read.rs`, `edit.rs`, `write.rs`, `glob.rs`, `grep.rs` | [tool.md](tool.md) |
| Deterministic Tools | Eggsact in-process deterministic tools (8 always-visible + 5 deferred) — text comparison, config validation, security inspection | `deterministic.rs`, `eggsact/adapter.rs` | [deterministic_tools.md](deterministic_tools.md) |
| Preflight | Harness-side eggsact validation before mutating operations — severity-classified findings (Block/Warn/Annotate), never model-facing | `preflight/` | [preflight.md](preflight.md) |
| Git Service | Canonical read executor — delegates to egggit for structured parsing, subprocess fallback for mutations | `git_service.rs` | [git.md](git.md) |
| Git Mutations | Typed Git mutations with state deltas, snapshot/delta capture, RunStore persistence | `git_mutations.rs`, `git_mutations_ops.rs` | [git.md](git.md) |
| Git Network | Network operations (fetch/pull/push/remote), env hardening, URL credential redaction | `git_network_ops.rs`, `git_network_policy.rs` | [git.md](git.md) |
| Git Recovery | In-progress operation detection, continue/abort/skip with cross-operation misuse protection | `git_recovery.rs`, `git_run_store.rs` | [git.md](git.md) |
| Git Mutation Projector | Formats git mutation results for TUI and model consumption | `git_mutation_projector.rs` | [git.md](git.md) |
| Tool Programs | Durable program domain, storage, call ledger — restricted-Python programs with broker adapter, child-job composition, lineage tracking, replay, corrective closure (M006-M017) | `tool_program.rs`, `tool_program_context.rs`, `tool_program_ledger.rs`, `tool_program_result.rs`, `tool_program_source.rs`, `program_cache.rs`, `program_manifest.rs` | [tool_programs.md](tool_programs.md) |
| Tool Broker | Single canonical execution boundary for all production tool calls — ordered policy pipeline, typed results, direct + programmatic paths (M011/M012) | `broker.rs`, `contract.rs` | [tool_broker.md](tool_broker.md) |
| Tool Program Language | Restricted-Python language specification — parse-only pipeline, fail-closed, bounded execution, deterministic IR | `crates/codegg-core/src/tool_program/` | [tool_program_language.md](tool_program_language.md) |

### TUI Layer — User Interface

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| TUI | Ratatui terminal UI, async command pattern (spawn-and-complete), state management across 6 domains | `app/mod.rs`, `components/`, `commands/`, `runtime/` | [tui.md](tui.md) |
| Command | Slash command registry (108 built-in commands) from markdown files | `tui/command.rs` | [command.md](command.md) |
| Theme | Frontend-neutral theme system (SemanticTheme → ratatui, Halloy) | `theme/` | [theme.md](theme.md) |
| Shell | Human shell `!`/`!!` commands, projection pipeline (10 phases), safety policy, RTK integration, redaction | `shell/` | [human_shell.md](human_shell.md) |
| Shell Session | Shell session metadata (no PTY) | `shell_session/` | [shell_session.md](shell_session.md) |

### Core Layer — Daemon and Transport

The core layer owns the singleton daemon lifecycle, transport adapters, request routing, and workspace registry.

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Core | CoreClient facade, transport adapters (Socket/Inproc/Stdio), daemon lifecycle, request handling | `core/daemon.rs`, `core/instance.rs`, `core/transport/` | [core.md](core.md) |
| Workspace | Workspace registry, canonical root tracking, execution context (immutable `Arc`-wrapped) | `workspace.rs` (codegg-core) | [workspace.md](workspace.md) |
| Workspace Services | Per-workspace service bundles (RunStore, path policy, lock table, config), single-flight activation, user-scoped catalog, migration tooling | `workspace_services.rs` (codegg-core), `migration.rs` | [workspace_services.md](workspace_services.md) |
| Jobs | Durable jobs, attempts, schedules, recovery, idempotency (Phase 4) | `jobs/mod.rs`, `jobs/store.rs`, `jobs/schedule.rs` (codegg-core) | [jobs.md](jobs.md) |
| Scheduler | Global admission control scheduler, fair queue, executor dispatch, permit lifecycle (Phase 5) | `scheduler/mod.rs`, `scheduler/scheduler.rs`, `scheduler/admission.rs`, `scheduler/fair_queue.rs` | [scheduler.md](scheduler.md) |
| Job Dispatcher | Bridges durable jobs to existing executors (SubAgent, ManagedArgv, Test) | `job_dispatcher.rs`, `job_recovery.rs` | [jobs.md](jobs.md) |
| Managed Process | Managed process lifecycle with process-group cleanup, timeout, cancellation, descendant tracking | `managed_process.rs` | [scheduler.md](scheduler.md) |
| Session | SQLite session storage, message history, migrations, analytics, checkpointing | `session/` (codegg-core) | [session.md](session.md) |
| Storage | SQLite initialization and connection pooling — user-scoped catalog + legacy project store | `storage/` (codegg-core) | [storage.md](storage.md) |
| Bus | Event bus publish/subscribe (51 AppEvent variants), PermissionRegistry, QuestionRegistry | `bus/` (codegg-core) | [bus.md](bus.md) |
| Error | Centralized AppError enum with error classification | `error.rs` | [error.md](error.md) |
| Projection | Session projection contract — frontend-neutral derived view, deterministic canonical reducer, scoped subscriptions, durable replay (M1-M2) | `projection/` (codegg-protocol), `projection_replay/` (codegg-core) | [projection.md](projection.md) |
| Project Catalog | Daemon-owned project catalog — list, get, register, archive, restore; path-independent identity, lifecycle management | `project_catalog.rs`, `project_storage.rs` (codegg-core) | [project_catalog.md](project_catalog.md) |
| Identity | Typed domain identity foundation — opaque string newtypes with UUIDv4, validated parsing, lexical contract | `identity.rs` (codegg-core) | [identity.md](identity.md) |
| Project Identity Storage | Durable logical-project and repository authority — workspace/session binding, reconciliation, migration | `project_storage.rs` (codegg-core) | [project_identity_storage.md](project_identity_storage.md) |
| Exec | Non-interactive exec mode for CI/CD with JSON I/O | `exec.rs` | [exec.md](exec.md) |

### Provider Layer — LLM Backends

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Provider | LLM provider implementations, streaming, CircuitBreaker — 15 env-var auto-registered providers plus config-defined ones (Azure, Bedrock, Vertex AI, Cloudflare, GitHub Copilot, GitLab Duo, OpenAI-compatible, eggpool) | `provider_core.rs`, `anthropic.rs`, `openai.rs`, `google.rs`, `connection.rs` (codegg-providers) | [provider.md](provider.md) |
| Protocol | CoreRequest, CoreResponse, CoreEvent, TuiMessage, UiNode, UiEffect, PluginManifestDto | `core.rs`, `tui.rs` (codegg-protocol) | [protocol.md](protocol.md) |
| Config | Configuration schema, paths, loading, validation, file watching | `schema.rs`, `paths.rs`, `watcher.rs` (codegg-config) | [config.md](config.md) |
| Model Profile | Model behavioral profiles and task state policy | `model_profile/` (codegg-core) | [model_profile_task_state.md](model_profile_task_state.md) |
| Task State | Todo/task state machine, injection, and projection | `task_state/` (codegg-core) | [model_profile_task_state.md](model_profile_task_state.md) |

### Integration Layer — External Systems

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| LSP | Language Server Protocol client — 39 servers, diagnostics, code navigation, preview-only edits, semantic tokens | `lsp/` (thin shim), `egglsp/` (authoritative) | [lsp.md](lsp.md) |
| MCP | Model Context Protocol client — local/remote server connections, OAuth auth, auto-reconnection | `mcp/` | [mcp.md](mcp.md) |
| Search Backend | Wrapper between `websearch`/`webfetch` tools and eggsearch MCP server, with legacy in-tree fallback | `search_backend/` | [search_backend.md](search_backend.md) |
| Plugin | WASM plugin system (Wasmtime), manifest parsing, hook system, built-in plugins, install/registry, lifecycle/policy | `plugin/` | [plugin.md](plugin.md) |
| Server | Axum HTTP server (feature-gated) — WebSocket for remote TUI, REST API, SSE events, token auth, rate limiting | `server/` | [server.md](server.md) |
| Client | Remote TUI WebSocket client with resume/replay | `client/` | [client.md](client.md) |
| IDE | VS Code / JetBrains detection and diff viewing | `ide/` | [ide.md](ide.md) |

### Security Layer

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Permission | Tool/path access control, DoomLoop detection, mode-based permissions (Review/Debug/Docs) | `permission/` | [permission.md](permission.md) |
| Security | SSRF protection, internal IP validation, Landlock filesystem sandboxing | `security/` | [security.md](security.md) |
| Eggsentry | Deterministic security scanning — secrets, commands, dependencies, unsafe code | `eggsentry/` (crate) | [security.md](security.md) |
| Crypto | AES-256-GCM encryption, Argon2id key derivation for API key encryption | `auth/` | [crypto.md](crypto.md) |
| Auth | Authentication and credential management | `auth/` | [auth.md](auth.md) |

### Native Tool Crates (Workspace)

Codegg follows a **library-first, MCP-second** tool architecture. Durable tool domains live in workspace crates under `crates/` and are consumed directly in-process by Codegg's tool wrappers. The same crates can later expose optional MCP adapter binaries without changing the model-facing tool names. See [native_crates.md](native_crates.md).

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `codegg-core` | Domain types: bus, error, goal, identity, jobs, memory, migration, model_profile, project_catalog/discovery/storage, projection_replay, provider_connections, repository_lineage, resilience, run_store, session, snapshot, storage, task_state, tool_program, workspace, workspace_services, worktree | `lib.rs`, `bus/`, `jobs/`, `session/`, `storage/` |
| `codegg-config` | Configuration schema, paths, loading, validation, file watching | `schema.rs`, `paths.rs`, `watcher.rs` |
| `codegg-protocol` | CoreRequest, CoreResponse, CoreEvent, TuiMessage, UiNode, UiEffect, PluginManifestDto, runtime assets DTOs | `core.rs`, `tui.rs` |
| `codegg-providers` | LLM provider implementations, auth types, CircuitBreaker | `provider_core.rs`, `auth_types.rs`, `circuit.rs` |
| `codegg-git` | Typed Git operation model (54 operation variants), argv parser, risk classification (11 risk classes) | `operation.rs`, `risk.rs` |
| `egglsp` | LSP client/service/operations (authoritative implementation) | `service.rs`, `client.rs`, `operations.rs`, `server.rs` |
| `egggit` | Read-only git facts: status (v2 rich structured), diff, changed files, log, blame, refs, worktree | `status_v2/`, `diff/`, `log/`, `blame/`, `refs/`, `worktree/` |
| `eggsentry` | Security scanning — secrets, commands, dependencies, unsafe code | `scanner.rs`, `command.rs`, `finding.rs`, `profile.rs` |
| `eggcontext` | Token counting and context utilities (tiktoken-based) | `lib.rs` |

Codegg-side thin wrappers (`src/tool/lsp.rs`, `src/tool/git.rs`, `src/tool/security.rs`, etc.) consume these crates. The model-facing tool names (`lsp`, `git`, `security`, ...) and JSON schemas are preserved exactly.

### Utility and Support

| Module | Purpose | Key Files | Docs |
|--------|---------|-----------|------|
| Hooks | Lifecycle hooks for agent events | `hooks/` | [hooks.md](hooks.md) |
| Memory | Persistent memory across sessions | `memory/` (codegg-core) | [memory.md](memory.md) |
| Goal | Goal tracking and management | `goal/` (codegg-core) | [goal.md](goal.md) |
| Snapshot | File state capture and restore | `snapshot/` (codegg-core) | [snapshot.md](snapshot.md) |
| Worktree | Git worktree management | `worktree.rs` (codegg-core), `worktree/` (egggit) | [worktree.md](worktree.md) |
| Run Store | Persistent run index and artifact storage for commands, scripts, tests | `run_store.rs` (codegg-core) | [run_store.md](run_store.md) |
| Resilience | Circuit breaker, retry mechanisms | `resilience.rs` (codegg-core) | [resilience.md](resilience.md) |
| Skills | Runtime skill loader and activation | `skills/` | [skills.md](skills.md) |
| TTS | Text-to-speech (macOS `say` command) | `tts/` | [tts.md](tts.md) |
| Upgrade | Self-upgrade via GitHub releases | `upgrade/` | [upgrade.md](upgrade.md) |
| Util | Clipboard, fuzzy search, pricing, metrics | `util/` | [util.md](util.md) |

## Verified Counts

Counts below were re-verified against the current tree (see source column).

| Item | Count | Source |
|------|-------|--------|
| Tools (registration sites) | 50 | `src/tool/mod.rs::with_options()` |
| Tools (always registered core) | ~31 | remainder gated by todo policy / evidence backend / eggsact / context-read config |
| Eggsact deterministic tools | 8 visible + 5 deferred | `src/tool/deterministic.rs::build_eggsact_tools()` |
| LSP servers | 39 | `crates/egglsp/src/server.rs` |
| Native tool crates | 10 | `crates/` (9 workspace members + test-server binary) |
| AppEvent variants | 51 | `crates/codegg-core/src/bus/events.rs` |
| Built-in slash commands | 108 | `src/tui/command.rs` |
| Built-in agents | 9 | `assets/agents/*.toml` |
| Database tables | 63 | `crates/codegg-core/src/session/schema.rs` |
| Storage layout version | 48 | `crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION` |
| Integration test files | 167 | `tests/*.rs` |
| Architecture docs | 72 | `architecture/` |
| Shell projection phases | 10 | `src/shell/` |
| Python script modes | 3 | `src/python_script/types.rs` (Analyze/Transform/Verify) |
| Git operation variants | 54 | `crates/codegg-git/src/operation.rs` |
| Git risk classes | 11 | `crates/codegg-git/src/risk.rs` |
| Providers (env-var auto-registered) | 15 | `crates/codegg-providers/src/provider_core.rs::register_builtin()` |
| CI guard scripts | 19 | `scripts/` |

## Feature Gates

| Feature | Description |
|---------|-------------|
| `server` | Axum HTTP server, WebSocket TUI |
| `plugins` | WASM plugin system with wasmtime |
| `image` | Image support via ratatui-image |
| `arboard` | Clipboard support (default feature) |
| `debug-logging` | Debug logging output |
| `lsp-test-support` | Fake LSP server + integration test harness |
| `lsp-real-server-tests` | Real LSP server smoke tests (requires installed servers) |

## Database Schema

```
┌───────────────────────────────────────────────────────────────────┐
│ Tables (63, STORAGE_LAYOUT_VERSION = 48)                          │
├───────────────────────────────────────────────────────────────────┤
│ migration_version  │ project        │ session        │ message    │
│ part               │ todo           │ permission     │ session_share │
│ cached_models      │ task           │ checkpoints    │ snapshot   │
│ usage              │ goal           │ session_events │ research_run │
│ user_preferences   │ core_event_log │ notification_history │ workspace │
│ job                │ job_attempt    │ job_dependency │ schedule   │
│ schedule_occurrence                                                          │
└───────────────────────────────────────────────────────────────────┘
```

## Data Flow

```
User Input → TUI Event Loop → App::on_key() → State Mutation → Render
                                    │
                         CoreClient.request()
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
              AgentLoop      PermissionChecker    HookRegistry
                    │               │               │
                    ▼               ▼               ▼
              Provider ◀──── ToolRegistry ────▶ Tools
                    │
                    ▼
            GlobalEventBus::publish()
                    │
                    ▼
            CoreClient.subscribe() → TUI updates
```

## Key Architectural Patterns

### Singleton Daemon
Exactly one user-scoped daemon per OS user. `connect_or_start_daemon` (`src/core/instance.rs`) is the canonical entry point. `DaemonInstanceGuard` holds `flock(LOCK_EX | LOCK_NB)` for the daemon's lifetime. Metadata in `daemon.json` is diagnostic only; the lock is authoritative.

### Library-First, MCP-Second
Durable tool domains live in workspace crates under `crates/` and are consumed directly in-process. The same crates can later expose optional MCP adapter binaries without changing model-facing tool names.

### Single Canonical Boundaries
- `ToolBroker` is the only production tool-call boundary.
- `JobSubmissionService` is the only door into the `JobScheduler`; the scheduler is the only admission authority for submitted work.
- `GitExecutionService` is the canonical read executor; `egggit` never mutates.
- All process-spawn sites are inventoried in `docs/execution-ownership.toml`.

### 3-Stage Command Pipeline
Commands flow through classification → planning → routing. Each stage is a separate module with clear responsibilities. Active routing mode (`CommandIntentMode::Active`) enables dispatch to structured backends; default mode is `Observe` (classify + annotate only).

### Projection Pipeline (10 Phases)
Shell output flows through a 10-phase projection pipeline: raw capture → projector selection → RTK compression → redaction → expansion handles → context budget → promotion decisions. Each phase is independently testable.

### Workspace-Scoped Services
Each registered workspace gets its own `WorkspaceServices` bundle (RunStore, path policy, lock table, config). Bundles are lazily activated, lease-tracked, and idle-evicted.

### Scheduler-Owned Execution
The `JobScheduler` is the single daemon admission authority for submitted work. All heavy operations (tests, managed processes, subagent dispatch) flow through `JobSubmissionService` → `JobScheduler` → executor dispatch.

## Navigation

Deep-dive index. Every architecture document in this directory is listed here.

### Agent and Execution
- [Agent Loop](agent.md) — Main execution cycle, compaction, routing, multi-agent teams
- [ACP](acp.md) — Agent Client Protocol v1 stdio adapter
- [Command Intent](command_intent.md) — Command classification, risk assessment, execution capabilities
- [Command Planner](command_planner.md) — Backend mapping, permission generation, projection policy
- [Command Routing](command_routing.md) — Concrete subsystem dispatch
- [Test Runner](test_runner.md) — Test resolution, parsing, failure extraction, previous-failures index
- [Python Scripting](python_scripting.md) — Analyze/Transform/Verify modes, AST risk, Landlock sandbox
- [Research](research.md) — Structured research pipeline
- [Exec](exec.md) — Non-interactive execution mode

### Tools and Capabilities
- [Tool](tool.md) — Tool trait, registry, built-in tools, backend abstraction
- [Deterministic Tools](deterministic_tools.md) — Eggsact in-process validators
- [Preflight](preflight.md) — Harness-side validation before mutations
- [Git](git.md) — Git service, mutations, network, recovery, credential lifecycle
- [LSP](lsp.md) — Language Server Protocol (39 servers, egglsp authoritative)
- [MCP](mcp.md) — Model Context Protocol client
- [Search Backend](search_backend.md) — Web search/fetch with eggsearch backend
- [Plugin](plugin.md) — WASM plugin system (Wasmtime)
- [Tool Programs](tool_programs.md) — Durable program domain, storage, call ledger (M006-M017)
- [Tool Broker](tool_broker.md) — Single canonical execution boundary for tool calls
- [Tool Program Language](tool_program_language.md) — Restricted-Python language specification

### User Interface
- [TUI](tui.md) — Ratatui terminal UI, async commands, state management
- [Command](command.md) — 108 built-in slash commands
- [Theme](theme.md) — Frontend-neutral theme system
- [Human Shell](human_shell.md) — `!`/`!!` commands, 10-phase projection pipeline
- [Shell Session](shell_session.md) — Shell session metadata

### Core and Infrastructure
- [Core](core.md) — Daemon lifecycle, transport adapters, request routing
- [Workspace](workspace.md) — Workspace registry, execution context
- [Workspace Services](workspace_services.md) — Per-workspace service bundles, migration
- [Jobs](jobs.md) — Durable jobs, attempts, schedules, recovery
- [Scheduler](scheduler.md) — Admission control, fair queue, executor dispatch
- [Session](session.md) — SQLite storage, message history
- [Storage](storage.md) — SQLite initialization, connection pooling
- [Bus](bus.md) — Event bus, permission/question registries
- [Error](error.md) — Centralized error handling
- [Projection](projection.md) — Session projection contract, deterministic reducer
- [Project Catalog](project_catalog.md) — Daemon-owned project catalog service
- [Identity](identity.md) — Typed domain identity foundation
- [Project Identity Storage](project_identity_storage.md) — Project/repository identity, binding, reconciliation

### Providers and Config
- [Provider](provider.md) — LLM provider implementations
- [Protocol](protocol.md) — Shared request/response envelopes
- [Config](config.md) — Configuration loading and validation
- [Model Profile & Task State](model_profile_task_state.md) — Model behavioral profiles, todo/task state
- [Native Crates](native_crates.md) — Workspace crate architecture

### Security
- [Permission](permission.md) — Access control, DoomLoop detection, modes
- [Security](security.md) — SSRF protection, Landlock sandboxing
- [Crypto](crypto.md) — AES-256-GCM encryption
- [Auth](auth.md) — Authentication and credentials

### Support
- [Hooks](hooks.md) — Lifecycle hooks
- [Memory](memory.md) — Persistent memory
- [Goal](goal.md) — Goal tracking
- [Snapshot](snapshot.md) — File state capture/restore
- [Worktree](worktree.md) — Git worktree management
- [Run Store](run_store.md) — Run index and artifact storage
- [Resilience](resilience.md) — Circuit breaker, retry
- [Skills](skills.md) — Runtime skill loader
- [TTS](tts.md) — Text-to-speech
- [Upgrade](upgrade.md) — Self-upgrade
- [Util](util.md) — Clipboard, fuzzy search, pricing, metrics

### Git Handoffs
- [Git Phase F Handoff](git_phase_f_handoff.md) — Phase F closure handoff
- [Git Polish/Verification Handoff](git_polish_verification_handoff.md) — Post-closure verified state
- [LSP Disk Cache Threat Model](lsp_disk_cache_threat_model.md) — LSP cache security

### Additional References
- [Agent Tool Surface](agent-tool-surface.md) — Agent-facing tool definitions and categories
- [Cache-Aware Context](cache-aware-context.md) — Cache-aware context packing
- [Compaction](compaction.md) — Context window overflow management
- [Context Ledger](context-ledger.md) — Token counting and context utilities
- [CodeGG Core](codegg_core.md) — codegg-core crate internals
- [Model Adapters](model-adapters.md) — Model adapter patterns and conversions
- [Testing](testing.md) — Test resource taxonomy, Tokio runtime rules

## Directory Layout

```
codegg/
├── src/                        # Root crate (application)
│   ├── agent/                  # Agent loop, compaction, routing, teams
│   ├── auth/                   # Authentication, crypto
│   ├── bin/                    # Auxiliary binaries (sandbox helper)
│   ├── client/                 # Remote TUI WebSocket client
│   ├── command_intent/         # Command classification (stage 1)
│   ├── command/                # Slash command registry
│   ├── context/                # Token counting utilities
│   ├── core/                   # Daemon, transport, request handling
│   ├── eggsact/                # Eggsact adapter (in-process)
│   ├── hooks/                  # Lifecycle hooks
│   ├── ide/                    # VS Code/JetBrains detection
│   ├── lsp/                    # LSP thin re-export shim
│   ├── mcp/                    # MCP client
│   ├── permission/             # Access control
│   ├── plugin/                 # WASM plugin system
│   ├── preflight/              # Eggsact preflight validation
│   ├── python_script/          # Python scripting (Analyze/Transform/Verify)
│   ├── research/               # Research pipeline
│   ├── scheduler/              # Admission control, fair queue
│   ├── search/                 # Web search tools (legacy)
│   ├── search_backend/         # Search backend dispatch
│   ├── security/               # SSRF, sandboxing
│   ├── server/                 # HTTP/WebSocket server (feature-gated)
│   ├── shell/                  # Human shell, projection pipeline
│   ├── shell_session/          # Shell session metadata
│   ├── skills/                 # Skill loader
│   ├── test_runner/            # Test execution, parsing, reporting
│   ├── theme/                  # Theme system
│   ├── tool/                   # Built-in tools
│   ├── tts/                    # Text-to-speech
│   ├── tui/                    # Terminal UI (Ratatui)
│   ├── upgrade/                # Self-upgrade
│   ├── util/                   # Utilities
│   ├── git_*.rs                # Git mutations, network, recovery, store
│   ├── command_*.rs            # Command pipeline (planner, routing, outcome)
│   ├── job_*.rs                # Job dispatch, recovery
│   ├── managed_process.rs      # Managed process lifecycle
│   ├── lib.rs                  # Library root (re-exports)
│   └── main.rs                 # Binary entry point
├── crates/                     # Workspace crates (library-first)
│   ├── codegg-core/            # Domain types, bus, jobs, session, storage
│   ├── codegg-config/          # Config schema, paths, loading
│   ├── codegg-protocol/        # Protocol types (CoreRequest/Response/Event)
│   ├── codegg-providers/       # LLM provider implementations
│   ├── codegg-git/             # Git operation model, argv parser, risk
│   ├── egglsp/                 # LSP client (authoritative)
│   ├── egggit/                 # Read-only git facts (status, diff, log, etc.)
│   ├── eggsentry/              # Security scanning
│   ├── eggcontext/             # Token counting
│   └── egglsp-test-server/     # Fake LSP server for tests (not a member)
├── tests/                      # Integration tests (167 files)
├── assets/                     # Agent definitions, prompts, themes
│   ├── agents/                 # 9 built-in agent TOML definitions
│   └── prompts/                # Agent prompt templates
├── scripts/                    # CI guards, generators, validators
├── architecture/               # Architecture documentation (72 docs)
├── plans/                      # Design proposals and phase plans
├── docs/                       # Validation docs, manifests
└── examples/plugins/           # Plugin SDKs and reference plugins
```

## Static Guards

Run these after changing execution surfaces or adding workspace crate dependencies:

```bash
python3 scripts/check-core-boundary.sh           # codegg-core boundary enforcement
python3 scripts/check_daemon_cwd_usage.py        # workspace-bound daemon path guard
python3 scripts/check_scheduler_bypass.py        # scheduler-bypass guard
python3 scripts/check_execution_ownership.py     # process-spawn site ownership manifest
python3 scripts/check_git_forbidden_patterns.py  # git secret boundary + policy drift
python3 scripts/generate_builtin_agents.py --check  # agent asset staleness + schema validation
python3 scripts/check_projection_transport_isolation.py # projection raw-broadcast/identity guard
```

See AGENTS.md for the complete guard list.
