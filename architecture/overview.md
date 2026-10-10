# CodeGG Architecture Overview

CodeGG is a high-performance AI coding agent built in Rust, designed for terminal-based interaction with deep IDE and LSP integration. This document is the **bird's eye view**: how the system is decomposed, how a turn actually flows end to end, and an **index into one deep-dive document per discrete module/component** in this directory.

> **How to read this directory**
> - Start here for shape and vocabulary.
> - Follow a module link for the contract, invariants, and file-level detail of that one component.
> - Every deep-dive doc ends with a *Source verification* note naming what was checked against code and when.

## Deployment topology

The supported distribution remains one `codegg` executable per target. The daemon is a user-scoped singleton discovered and started by that executable; the TUI and daemon share the same business-logic libraries and invocation contracts. Runtime-safety milestone M007 measured the dependency and release profiles and retained this no-split topology because a daemon/TUI split did not produce a material deployment improvement without adding packaging or ownership complexity. Future role-specific binaries require a new measured deployment constraint and compatibility plan; they are not an advertised installation mode.

An optional Tauri desktop shell lives under `apps/desktop` and is built with its own Rust and JavaScript toolchains. Its Rust host uses `codegg-client` to connect to or start the existing `codegg` daemon; it is not a second daemon binary and does not change the root Cargo workspace's Rust 1.89 contract. See [desktop architecture](desktop.md).

## System architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          Frontend (Ratatui TUI)                     │
│                     Input ───► TUI ───► Render                      │
└────────────────────────┬────────────────────────────────────────────┘
                         │  CoreClient facade (request/response)
              ┌──────────▼──────────┐
              │   CoreClient       │  Inproc / Stdio / Socket
              └──────────┬──────────┘
                         │
┌────────────────────────▼────────────────────────────────────────────┐
│                        Daemon / AgentLoop                           │
│  ┌─────────┐   ┌──────────┐   ┌─────────┐   ┌──────────────────┐    │
│  │ Provider│──▶│ Messages │◀──│  Tools  │◀──│ Authorization    │    │
│  └─────────┘   └──────────┘   └────┬────┘   │  + Permission    │    │
│        │                           │        └──────────────────┘    │
│        │      ┌────────────────────▼──────────────┐                │
│        └─────▶│ ToolBroker (single tool boundary)  │                │
│               └────────────────────┬───────────────┘                │
│                       ┌────────────▼─────────────┐                  │
│                       │  JobSubmissionService     │                  │
│                       │  → JobScheduler           │                  │
│                       └───────────────────────────┘                  │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │  Event bus (AppEvent) ──► deterministic reducer ──► projection │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                       │
│  Domain layer: codegg-core (session, storage, jobs, workspace, …)     │
│  Crate layer:   egglsp · egggit · codegg-git · eggsentry · eggcontext │
└───────────────────────────────────────────────────────────────────────┘
```

## How everything works together

A single user prompt flows through the system along this path:

1. **Connect** — Running plain `codegg` calls `connect_or_start_daemon` (`src/core/instance.rs`), which connects to the running user-scoped daemon singleton (guarded by an authoritative nonblocking `File::try_lock`) or starts one. `--standalone` runs an in-process core instead.

2. **Transport** — The TUI talks to the core through the `CoreClient` facade over Inproc, Stdio, or Socket transports ([core.md](core.md)). Remote TUI sessions use the Axum WebSocket server ([server.md](server.md)); external agents use the ACP stdio adapter ([acp.md](acp.md)).

3. **Turn submission** — User input becomes a `CoreRequest`. The daemon hands work to `DefaultTurnRuntime.run_turn(TurnRunInput)` carrying an immutable, `Arc`-wrapped `ExecutionContext` (workspace root, workspace id, session id, path policy) — never `std::env::current_dir()` ([workspace.md](workspace.md)). Every request first passes the authorization gate (`operation_descriptor` capability check over a transport-bound principal; denials are side-effect-free), and security-relevant actions append to the structural audit store ([authorization.md](authorization.md), [audit.md](audit.md)).

4. **Agent loop cycle** — `AgentLoop` builds the turn context (asset snapshot, instructions, skills, memory, task state), streams an LLM completion through an optional bounded semantic selector for `virtual:<name>` models, then a provider ([provider.md](provider.md)), parses tool calls, executes them, appends results, and repeats until the model stops calling tools ([agent.md](agent.md)). Compaction manages context-window overflow ([compaction.md](compaction.md), [cache-aware-context.md](cache-aware-context.md)).

5. **Tool dispatch** — Every production tool call crosses the `ToolBroker` boundary: ordered policy pipeline → permission check → backend execution ([tool_broker.md](tool_broker.md)). Tools are native wrappers over workspace crates (`egglsp`, `egggit`, `eggsentry`, …), eggsact deterministic validators ([deterministic_tools.md](deterministic_tools.md)), MCP servers ([mcp.md](mcp.md)), or WASM plugins ([plugin.md](plugin.md)).

6. **Admission control** — Heavy operations (tests, managed processes, Python scripts, subagent dispatch, tool programs) do not run inline: callers submit through `JobSubmissionService`, the only door into the global `JobScheduler`, which enforces fair queuing and permits before dispatching executors ([scheduler.md](scheduler.md)). Work is durable — jobs, attempts, schedules, recovery, idempotency live in codegg-core ([jobs.md](jobs.md)).

7. **Events and projection** — Everything that happens publishes an `AppEvent` on the bus ([bus.md](bus.md)). A deterministic canonical reducer folds events into a frontend-neutral session projection; clients subscribe to scoped views and replay durable history ([projection.md](projection.md)). Project chat ([collaboration.md](collaboration.md)) and presence heartbeats ([presence.md](presence.md)) ride the same daemon-owned, authorized, privacy-preserving event path — chat bodies stay inert text, presence stays liveness-only and never authorizes work.

8. **Persistence** — Sessions, messages, todos, checkpoints, usage, goals, research runs, jobs, and schedules persist in SQLite (migration chain in `session/schema.rs`; current layout version in `storage::STORAGE_LAYOUT_VERSION`) ([session.md](session.md), [storage.md](storage.md)). Command, script, and test outputs land in the RunStore artifact store ([run_store.md](run_store.md)).

9. **Render** — The TUI applies projected state and renders ([tui.md](tui.md)). Human `!`/`!!` shell commands bypass the model unless explicitly promoted, flowing through a multi-phase safety/redaction projection pipeline first ([human_shell.md](human_shell.md)).

### Command execution pipeline

Shell-originated commands flow through one typed pipeline:

```
Raw Shell Command
    │
    ▼
prepare_command()                        ← command_intent::pipeline
    │  Parse/normalize + semantic intent
    ▼
plan_execution_with_context()            ← command_intent::plan
    │  Backend, permission/risk, projector policy
    ▼
CommandPlan::dispatch_target()
    │  One typed executor target
    ▼
┌──────────────────────────────────────────────────────────┐
│ RouteToTestRunner │ RouteToShell │ RouteToPython          │
│ RouteToGit        │ RouteToNativeTool │ RouteToManagedProcess│
└──────────────────────────────────────────────────────────┘
```

Active routing (`CommandIntentMode::Active`) dispatches to structured backends; the default `Observe` mode classifies and annotates only. `command_routing` is retained as a **source-compatible facade with no independent routing logic** — see [command_routing.md](command_routing.md) and [command_planner.md](command_planner.md).

## Module map

Modules are grouped by architectural layer. Each row links to the deep-dive document for that component.

### Agent Layer — Orchestration and Execution

The agent layer owns the execution cycle: receiving user input, routing through the LLM, executing tools, and managing multi-agent coordination.

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Agent | Main agent loop, compaction, routing, durable delegated-run orchestration | [agent.md](agent.md) |
| Agent Tool Surface | Model-facing tool definitions, categories, and gating rules | [agent-tool-surface.md](agent-tool-surface.md) |
| Asset Context / Snapshot | Explicit context, immutable snapshot builder, generation coordinator, bounded operator refresh, turn/agent-run pinning | [agent.md](agent.md) |
| ACP | Agent Client Protocol v1 stdio adapter over the singleton daemon | [acp.md](acp.md) |
| Command Intent | Command classification, risk assessment, execution capability model — pipeline stage 1 | [command_intent.md](command_intent.md) |
| Command Planner | Maps classified intents to execution backends, permissions, projection policy — stage 2 | [command_planner.md](command_planner.md) |
| Command Routing | Compatibility facade over the pre-M006 routing API — stage 3 | [command_routing.md](command_routing.md) |
| Test Runner | Test resolution, stdout/stderr parsing, failure taxonomy, previous-failures index | [test_runner.md](test_runner.md) |
| Python Scripting | Analyze/Transform/Verify modes, AST risk scanning, Landlock sandboxing | [python_scripting.md](python_scripting.md) |
| Research | Structured research pipeline: sources → evidence → claims → verification | [research.md](research.md) |
| Work Orders / Work Plan | Durable work-order lifecycle and plan item tracking | [work_orders.md](work_orders.md), [work_plan.md](work_plan.md) |
| Model Profile / Task State | Model behavioral profiles, todo/task state machine, injection and projection | [model_profile_task_state.md](model_profile_task_state.md) |

### Tool Layer — Capabilities and Execution

The tool layer defines the built-in tools the agent can invoke, the backend abstraction for dispatch, and the deterministic validation pipeline.

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Tool | `Tool` trait, `ToolRegistry`, `ToolCatalog` + `tool_search` on-demand discovery, backend abstraction, all built-in tools | [tool.md](tool.md) |
| Tool Broker | Single canonical execution boundary for production tool calls — ordered policy pipeline, typed results | [tool_broker.md](tool_broker.md) |
| Tool Programs | Durable program domain, storage, call ledger, lineage, replay | [tool_programs.md](tool_programs.md) |
| Tool Program Language | Restricted-Python language specification — parse-only, fail-closed, deterministic IR | [tool_program_language.md](tool_program_language.md) |
| Deterministic Tools | Eggsact in-process deterministic validators | [deterministic_tools.md](deterministic_tools.md) |
| Preflight | Harness-side eggsact validation before mutating operations — never model-facing | [preflight.md](preflight.md) |
| Tool Advisor | Bounded local semantic/causal tool-selection advisor (opt-in) | [tool-advisor.md](tool-advisor.md), [spike](tool-advisor-framework-spike.md) |
| Decision Engine | Backend-neutral decision contract with an optional System One-compatible backend; explicit opt-in, bounded fallback; owns advisor decision paths | [tool-advisor.md](tool-advisor.md) |
| Interactive Process | PTY lifecycle + attach protocol — distinct from finite managed process runs | [process-tool-execution-ownership.md](process-tool-execution-ownership.md) |
| Git Service / Mutations / Network / Recovery | Canonical read executor plus typed guarded mutations, network policy, recovery | [git.md](git.md) |
| Search Backend | Dispatch between in-tree search tools and the eggsearch MCP backend | [search_backend.md](search_backend.md) |

### Tools and capabilities at a glance

Built-in tools live in `src/tool/` (plus `src/tool/bash/`) and `src/context/`. The registry is assembled in `src/tool/mod.rs::with_options()`; visibility is gated by todo policy, evidence-backend availability, LSP/security backend availability, eggsact availability, and context-read config. A `DisabledTool` stub substitutes a typed error when a backend-gated domain is unavailable, so it carries a runtime-supplied name rather than a fixed one. See [tool.md](tool.md) and [agent-tool-surface.md](agent-tool-surface.md) for the authoritative surface.

| Capability | Tools |
|------------|-------|
| Filesystem | `read`, `write`, `edit`, `apply_patch`, `replace`, `diff`, `glob`, `grep`, `list` |
| Shell / execution | `bash`, `verify`, `terminal`, `test`, `batch` |
| Source intelligence | `lsp`, `lsp_read`, `lsp_preview_apply`, `codesearch`, `repo_map` |
| Version control | `git`, `git_query`, `git_read`, `commit` |
| Research / evidence | `research`, `research_search`, `repo_search`, `repo_fetch`, `security_search`, `batch_fetch`, `evidence_bundle`, `websearch`, `webfetch` |
| Planning / tasks | `task`, `todoread`, `todowrite`, `plan_enter`, `plan_exit`, `goal_get`, `goal_request_completion`, `goal_update_progress`, `work_order`, `work_plan_get`, `work_plan_update_item` |
| Review / quality | `review`, `security`, plus eggsact deterministic tools |
| Extension / resource | `mcp_resource_read`, `mcp_resource_search`, `extension_search`, `extension_install_request`, `memory_get`, `memory_search` |
| Programmatic / meta | `tool_program`, `tool_search`, `question`, `skill`, `skill_proposal`, `image`, `context_read`, `invalid` |

### TUI Layer — User Interface

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| TUI | Ratatui terminal UI, async command pattern (spawn-and-complete), state management | [tui.md](tui.md) |
| Command | Slash command registry assembled from markdown manifests | [command.md](command.md) |
| Theme | Frontend-neutral theme system (SemanticTheme → ratatui, Halloy) | [theme.md](theme.md) |
| Human Shell | `!`/`!!` commands, projection pipeline, safety policy, redaction | [human_shell.md](human_shell.md) |
| IDE | VS Code / JetBrains detection and diff viewing | [ide.md](ide.md) |
| Documents | Shared editor document foundation — frontend-neutral text snapshots and transactions | [document.md](document.md) |
| Desktop | Optional Tauri desktop shell over `codegg-client` | [desktop.md](desktop.md) |

### Core Layer — Daemon and Transport

The core layer owns the singleton daemon lifecycle, transport adapters, request routing, and the workspace registry.

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Core | `CoreClient` facade, transport adapters, daemon lifecycle, request handling | [core.md](core.md) |
| Workspace | Workspace registry, canonical root tracking, immutable `Arc`-wrapped execution context | [workspace.md](workspace.md) |
| Workspace Services | Per-workspace service bundles, single-flight activation, user-scoped catalog | [workspace_services.md](workspace_services.md) |
| Jobs | Durable jobs, attempts, schedules, recovery, idempotency | [jobs.md](jobs.md) |
| Scheduler | Global admission control, fair queue, executor dispatch, permit lifecycle | [scheduler.md](scheduler.md) |
| Session | SQLite session storage, message history, migrations, checkpointing | [session.md](session.md) |
| Storage | SQLite initialization and pooling — user-scoped catalog + project store | [storage.md](storage.md) |
| Bus | Event bus publish/subscribe, `PermissionRegistry`, `QuestionRegistry` | [bus.md](bus.md) |
| Projection | Frontend-neutral derived view, deterministic reducer, scoped subscriptions, durable replay | [projection.md](projection.md) |
| Protocol | Shared request/response/event envelopes and capability structs | [protocol.md](protocol.md) |
| Identity | Typed domain identity foundation — opaque string newtypes with UUIDv4 | [identity.md](identity.md) |
| Project Identity Storage | Durable logical-project and repository authority, binding, reconciliation | [project_identity_storage.md](project_identity_storage.md) |
| Project Catalog | Daemon-owned project catalog — list, get, register, archive, restore | [project_catalog.md](project_catalog.md) |
| Authorization | Daemon operation-boundary gate — principals, capability map, attribution | [authorization.md](authorization.md) |
| Audit | Append-only structural audit store and instrumentation coverage | [audit.md](audit.md) |
| Collaboration | Project-scoped channels, threads, mentions, edits, retention | [collaboration.md](collaboration.md) |
| Presence | Ephemeral project-scoped presence — leases, heartbeat, expiry | [presence.md](presence.md) |
| Server | Axum HTTP/WS server (feature `server`) — remote TUI, REST, SSE, token auth | [server.md](server.md) |
| Client | Remote TUI WebSocket client with resume/replay | [client.md](client.md) |
| Exec | Non-interactive exec mode for CI/CD with JSON I/O | [exec.md](exec.md) |
| Error | Centralized `AppError` enum with classification | [error.md](error.md) |

### Provider Layer — LLM Backends

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Provider | LLM provider implementations, streaming, circuit breaker, connection lifecycle | [provider.md](provider.md) |
| Model Adapters | Model adapter patterns and conversions | [model-adapters.md](model-adapters.md) |
| Config | Configuration schema, paths, loading, validation, file watching | [config.md](config.md) |

### Integration Layer — External Systems

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| LSP | Language Server Protocol — `egglsp` is authoritative; `src/lsp/` is a thin shim | [lsp.md](lsp.md) |
| MCP | Model Context Protocol client — local/remote servers, OAuth, reconnection | [mcp.md](mcp.md) |
| Plugin | WASM plugin system (Wasmtime), manifest parsing, hooks, registry, lifecycle | [plugin.md](plugin.md) |
| Search Backend | Wrapper between search tools and eggsearch MCP, with legacy in-tree fallback | [search_backend.md](search_backend.md) |

### Security Layer

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Permission | Tool/path access control, loop detection, mode-based permissions | [permission.md](permission.md) |
| Security | SSRF protection, internal IP validation, Landlock filesystem sandboxing | [security.md](security.md) |
| Eggsentry | Deterministic security scanning — secrets, commands, dependencies, unsafe code | [security.md](security.md) |
| Crypto | AES-256-GCM encryption, Argon2id key derivation | [crypto.md](crypto.md) |
| Auth | Authentication and credential management | [auth.md](auth.md) |
| Authorization | Operation-boundary capability enforcement | [authorization.md](authorization.md) |
| Audit | Append-only security record | [audit.md](audit.md) |
| Approval Reviewer | LLM-assisted approval review surface | [approval_reviewer.md](approval_reviewer.md) |

### Native Crates (Workspace)

Codegg follows a **library-first, MCP-second** tool architecture: durable tool domains live in workspace crates and are consumed directly in-process. The same crates can later expose optional MCP adapter binaries without changing model-facing tool names. See [native_crates.md](native_crates.md).

| Crate | Purpose |
|-------|---------|
| `codegg-core` | Domain types: agent convergence, authorization, audit, bus, decision semantics, error, goal, identity, jobs, memory, model profile, project catalog/discovery/storage, projection replay, provider connections, repository lineage, resilience, run store, session, snapshot, storage, task state, tool programs, workspace, workspace services, worktree |
| `codegg-config` | Configuration schema, paths, loading, validation, watching |
| `codegg-protocol` | `CoreRequest`/`CoreResponse`/`CoreEvent`, TUI frames, plugin DTOs, runtime asset DTOs |
| `codegg-client` | Frontend-side native client and identity |
| `codegg-document` | Frontend-neutral text snapshots and transactions for editor documents |
| `codegg-providers` | LLM provider implementations, auth types, circuit breaker, crypto |
| `codegg-git` | Typed git operation model, argv parser, risk classification |
| `egglsp` | LSP client/service/operations and server registry (authoritative) |
| `egggit` | Read-only git facts — status, diff, log, blame, refs, worktree |
| `eggsentry` | Deterministic security scanning |
| `eggcontext` | Token counting and context primitives |

Two directories under `crates/` are intentionally **not** workspace members: `egglsp-test-server` (fake LSP harness behind the `lsp-test-support` feature) and `eggwork-test-node` (standalone fixture workspace with its own lockfile). `apps/desktop/src-tauri` is also excluded.

### Utility and Support

| Module | Purpose | Deep dive |
|--------|---------|-----------|
| Hooks | Lifecycle hooks for agent events | [hooks.md](hooks.md) |
| Memory | Persistent memory across sessions | [memory.md](memory.md) |
| Goal | Goal tracking and post-turn verification | [goal.md](goal.md) |
| Snapshot | File state capture and restore | [snapshot.md](snapshot.md) |
| Worktree | Git worktree management and leases | [worktree.md](worktree.md) |
| Run Store | Persistent run index and artifact storage | [run_store.md](run_store.md) |
| Resilience / Retry | Circuit breaker and retry policy | [resilience.md](resilience.md), [retry.md](retry.md) |
| Skills | Runtime skill loader and activation | [skills.md](skills.md) |
| Context/Compaction Ownership | Single-owner map for compaction budgets/triggers/strategy | [context-compaction-ownership.md](context-compaction-ownership.md) |
| Process/Tool Execution Ownership | Canonical process-ownership map, inventoried in `docs/execution-ownership.toml` | [process-tool-execution-ownership.md](process-tool-execution-ownership.md) |
| TTS | Text-to-speech | [tts.md](tts.md) |
| Upgrade | Verified managed-runfile self-upgrade | [upgrade.md](upgrade.md) |
| Util | Clipboard, fuzzy matching, pricing, metrics | [util.md](util.md) |
| Testing | Test taxonomy, isolation, runtime rules, CI profile | [testing.md](testing.md) |

## Verified counts

Counts below were re-measured against the current tree on **2026-10-06**.

| Item | Count | Source |
|------|-------|--------|
| Architecture docs | 85 | `architecture/*.md` |
| Tool registrations in `with_options()` | 63 call sites | `src/tool/mod.rs` |
| Distinct production tool names | 59 | `impl Tool for …` `name()` across `src/tool/`, `src/context/` (excludes 2 test-only mocks in `program_manifest.rs`) |
| Tool source files | 77 | `src/tool/**.rs` (incl. `bash/`) |
| LSP server definitions | 39 | `crates/egglsp/src/server.rs::server_definitions()` |
| `codegg-core` modules | 46 | `crates/codegg-core/src/lib.rs` (`pub mod`) |
| `AppEvent` variants | 58 | `crates/codegg-core/src/bus/events.rs` |
| `CoreRequest` variants | 232 (209 struct + 23 unit) | `crates/codegg-protocol/src/core.rs` |
| `CoreResponse` variants | 147 | `crates/codegg-protocol/src/core.rs` |
| `CoreEvent` variants | 87 | `crates/codegg-protocol/src/core.rs` |
| `ProjectionEvent` variants | 47 | `crates/codegg-protocol/src/projection/event.rs` |
| Authorized operations | 226 | `operation_descriptor` (`crates/codegg-core/src/authorization/policy.rs`) |
| Identity newtypes | 17 | `typed_identity!` (`crates/codegg-core/src/identity.rs`) |
| Built-in slash commands | 154 | asserted by `built_in_command_count_matches_release_docs` |
| Built-in agents | 10 | `assets/agents/*.toml` |
| Bundled themes | 50 | `assets/themes/**/*.toml` |
| TUI state modules | 27 (28 `.rs` files incl. `mod.rs`) | `src/tui/app/state/` |
| Database tables (`CREATE TABLE`) | 73 | `crates/codegg-core/src/session/schema.rs` |
| Storage layout version | 68 | `crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION` |
| Git operation variants | 54 | `crates/codegg-git/src/operation.rs` |
| Git risk classes | 11 | `crates/codegg-git/src/risk.rs` |
| Workspace member crates | 11 (+ root) | root `Cargo.toml` |
| Integration test files | 215 | `tests/*.rs` |
| CI guard scripts | 41 | `scripts/check_*` |

## Feature gates

| Feature | Description |
|---------|-------------|
| `server` | Axum HTTP server, WebSocket TUI |
| `plugins` | WASM plugin system with wasmtime |
| `image` | Image support via ratatui-image |
| `arboard` | Clipboard support (default feature) |
| `debug-logging` | Debug logging output |
| `lsp-test-support` | Fake LSP server + integration test harness |
| `lsp-real-server-tests` | Real LSP server smoke tests (requires installed servers) |

Never sweep the workspace with `--all-features`: it drags in real-server tests. `verify.sh full` uses `--features server,plugins,lsp-test-support`.

## Database schema

```
┌────────────────────────────────────────────────────────────┐
│ Tables — see session/schema.rs; layout version in storage  │
├────────────────────────────────────────────────────────────┤
│ migration_version │ project │ session │ message │ part     │
│ todo │ permission │ session_share │ cached_models │ task   │
│ checkpoints │ snapshot │ usage │ goal │ session_events    │
│ research_run │ user_preferences │ core_event_log           │
│ notification_history │ workspace │ job │ job_attempt       │
│ job_dependency │ schedule │ schedule_occurrence │ …        │
└────────────────────────────────────────────────────────────┘
```

See [session.md](session.md) for the full table inventory and [storage.md](storage.md) for the migration chain.

## Data flow

```
User Input → TUI Event Loop → App::on_key() → State Mutation → Render
                                    │
                         CoreClient.request()
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
              AgentLoop     AuthorizationGate    HookRegistry
                    │               │               │
                    ▼               ▼               ▼
              Provider ◀──── ToolBroker ────▶ Tool backends
                    │                               │
                    │                    ┌──────────┴──────────┐
                    │                    ▼                     ▼
                    │            JobSubmissionService      Native crates
                    │                    │               (egglsp/egggit/…)
                    │                    ▼
                    │              JobScheduler
                    │
                    ▼
            AppEvent published on bus
                    │
                    ▼
        Deterministic reducer ──► session projection
                    │
                    ▼
            CoreClient.subscribe() → TUI updates
```

## Key architectural patterns

### Singleton daemon
Exactly one user-scoped daemon per OS user. `connect_or_start_daemon` (`src/core/instance.rs`) is the canonical entry point. `DaemonInstanceGuard` holds a nonblocking `File::try_lock` for the daemon's lifetime. Metadata in `daemon.json` is diagnostic only; the lock is authoritative.

### Library-first, MCP-second
Durable tool domains live in workspace crates under `crates/` and are consumed directly in-process. The same crates can later expose optional MCP adapter binaries without changing model-facing tool names.

### Single canonical boundaries
- `ToolBroker` is the only production tool-call boundary.
- `JobSubmissionService` is the only door into the `JobScheduler`; the scheduler is the only admission authority for submitted work.
- `GitExecutionService` is the canonical read executor; `egggit` never mutates.
- All process-spawn sites are inventoried in `docs/execution-ownership.toml` and enforced by `check_execution_ownership.py`.

### Typed command pipeline
`command_intent::pipeline::prepare_command` is the canonical entry point for raw shell-originated input. It classifies once, plans with an explicit execution context, and calls `CommandPlan::dispatch_target()` once. The plan owns backend, permission, projector, timeout, and routing-family selection. The daemon/tool boundary remains the authorization authority; active-routing validation is only a preflight gate.

### Shell projection pipeline
Shell output flows through a multi-phase projection pipeline: raw capture → projector selection → compression → redaction → expansion handles → context budget → promotion decisions. Each phase is independently testable. See [human_shell.md](human_shell.md) for the current phase list.

### Workspace-scoped services
Each registered workspace gets its own `WorkspaceServices` bundle (RunStore, path policy, lock table, config). Bundles are lazily activated, lease-tracked, and idle-evicted.

### Scheduler-owned execution
The `JobScheduler` is the single daemon admission authority for submitted work. All heavy operations (tests, managed processes, subagent dispatch) flow through `JobSubmissionService` → `JobScheduler` → executor dispatch.

## Deep-dive index

Every architecture document in this directory, grouped by theme.

### Agent and Execution
- [Agent Loop](agent.md) — execution cycle, compaction, routing, delegated runs
- [Agent Tool Surface](agent-tool-surface.md) — model-facing tool definitions and gating
- [ACP](acp.md) — Agent Client Protocol v1 stdio adapter
- [Command Intent](command_intent.md) — classification, risk, execution capabilities
- [Command Planner](command_planner.md) — backend mapping, permission generation, projector policy
- [Command Routing](command_routing.md) — compatibility facade over the pre-M006 API
- [Test Runner](test_runner.md) — resolution, parsing, failure extraction, failures index
- [Python Scripting](python_scripting.md) — Analyze/Transform/Verify, AST risk, sandbox
- [Research](research.md) — sources → evidence → claims → verification
- [Work Orders](work_orders.md) — durable work-order lifecycle
- [Work Plan](work_plan.md) — plan item tracking
- [Exec](exec.md) — non-interactive execution mode
- [Model Profile & Task State](model_profile_task_state.md) — behavioral profiles, todo/task state

### Context and Compaction
- [Compaction](compaction.md) — context-window overflow management
- [Cache-Aware Context](cache-aware-context.md) — cache-aware context packing
- [Context Ledger](context-ledger.md) — token counting and context utilities
- [Context/Compaction Ownership](context-compaction-ownership.md) — single compaction owner map

### Tools and Capabilities
- [Tool](tool.md) — trait, registry, built-in tools, backend abstraction
- [Tool Broker](tool_broker.md) — single canonical execution boundary
- [Tool Programs](tool_programs.md) — durable programs, ledger, lineage, replay
- [Tool Program Language](tool_program_language.md) — restricted-Python specification
- [Deterministic Tools](deterministic_tools.md) — eggsact in-process validators
- [Preflight](preflight.md) — harness-side pre-mutation validation
- [Tool Advisor](tool-advisor.md) — bounded tool-selection advisor
- [Tool Advisor Framework Spike](tool-advisor-framework-spike.md) — spike findings
- [Git](git.md) — service, mutations, network, recovery, credential lifecycle
- [Search Backend](search_backend.md) — eggsearch dispatch with legacy fallback

### User Interface
- [TUI](tui.md) — Ratatui UI, async commands, state management
- [Command](command.md) — built-in slash command registry
- [Theme](theme.md) — frontend-neutral theme system
- [Human Shell](human_shell.md) — `!`/`!!` commands and projection pipeline
- [IDE](ide.md) — VS Code / JetBrains integration
- [Documents](document.md) — shared editor document foundation
- [Desktop](desktop.md) — optional Tauri shell
- [TTS](tts.md) — text-to-speech

### Core and Infrastructure
- [Core](core.md) — daemon lifecycle, transport adapters, request routing
- [Protocol](protocol.md) — shared request/response/event envelopes
- [Workspace](workspace.md) — registry, execution context
- [Workspace Services](workspace_services.md) — per-workspace bundles, migration
- [Jobs](jobs.md) — durable jobs, attempts, schedules, recovery
- [Scheduler](scheduler.md) — admission control, fair queue, dispatch
- [Session](session.md) — SQLite storage, message history
- [Storage](storage.md) — initialization, pooling, migration chain
- [Bus](bus.md) — event bus, permission/question registries
- [Projection](projection.md) — projection contract, deterministic reducer
- [Project Catalog](project_catalog.md) — daemon-owned project catalog
- [Identity](identity.md) — typed identity newtypes
- [Project Identity Storage](project_identity_storage.md) — project/repo identity authority
- [Authorization](authorization.md) — operation-boundary gate, principals, attribution
- [Audit](audit.md) — append-only audit store
- [Collaboration](collaboration.md) — project channels, messages, structured actions
- [Presence](presence.md) — presence leases, collaborators, observation
- [Server](server.md) — HTTP/WebSocket server
- [Client](client.md) — remote TUI WebSocket client
- [Error](error.md) — centralized error handling
- [Native Crates](native_crates.md) — workspace crate architecture
- [CodeGG Core](codegg_core.md) — codegg-core crate internals

### Providers and Config
- [Provider](provider.md) — LLM provider implementations
- [Model Adapters](model-adapters.md) — adapter patterns and conversions
- [Config](config.md) — configuration loading and validation

### Security
- [Permission](permission.md) — access control, loop detection, modes
- [Security](security.md) — SSRF protection, Landlock sandboxing
- [Crypto](crypto.md) — AES-256-GCM encryption
- [Auth](auth.md) — authentication and credentials
- [Approval Reviewer](approval_reviewer.md) — approval review surface
- [LSP Disk Cache Threat Model](lsp_disk_cache_threat_model.md) — LSP cache security

### Integrations
- [LSP](lsp.md) — Language Server Protocol (egglsp authoritative)
- [MCP](mcp.md) — Model Context Protocol client
- [Plugin](plugin.md) — WASM plugin system

### Support
- [Hooks](hooks.md) — lifecycle hooks
- [Memory](memory.md) — persistent memory
- [Goal](goal.md) — goal tracking
- [Snapshot](snapshot.md) — file state capture/restore
- [Worktree](worktree.md) — git worktree management
- [Run Store](run_store.md) — run index and artifact storage
- [Resilience](resilience.md) — circuit breaker
- [Retry](retry.md) — retry policy
- [Skills](skills.md) — runtime skill loader
- [Process/Tool Execution Ownership](process-tool-execution-ownership.md) — process-ownership map
- [Upgrade](upgrade.md) — self-upgrade
- [Util](util.md) — clipboard, fuzzy search, pricing, metrics
- [Testing](testing.md) — test taxonomy, isolation, runtime rules

### Historical Handoffs
- [Git Phase F Handoff](git_phase_f_handoff.md) — snapshot-time closure record
- [Git Polish/Verification Handoff](git_polish_verification_handoff.md) — snapshot-time post-closure record

## Directory layout

```
codegg/
├── src/                        # Root crate (application)
│   ├── agent/                  # Agent loop, compaction, routing, run control
│   ├── auth/                   # Authentication
│   ├── bin/                    # Auxiliary binaries (sandbox helper)
│   ├── client/                 # Remote TUI WebSocket client
│   ├── command/                # Slash command registry
│   ├── command_intent/         # Command classification + planning
│   ├── context/                # Token counting, context_read tool
│   ├── core/                   # Daemon, transport, request handling
│   ├── decision.rs             # Opt-in System One decision backend
│   ├── decision_sdm.rs         # Opt-in pinned SDM local Rank backend
│   ├── eggsact/                # Eggsact adapter (in-process)
│   ├── hooks/                  # Lifecycle hooks
│   ├── ide/                    # VS Code/JetBrains detection
│   ├── lsp/                    # LSP thin re-export shim
│   ├── mcp/                    # MCP client
│   ├── permission/             # Access control
│   ├── plugin/                 # WASM plugin system
│   ├── preflight/              # Eggsact preflight validation
│   ├── python_script/          # Python scripting
│   ├── research/               # Research pipeline
│   ├── scheduler/              # Admission control, fair queue
│   ├── search/                 # Legacy in-tree search tools
│   ├── search_backend/         # Search backend dispatch
│   ├── security/               # SSRF, sandboxing
│   ├── server/                 # HTTP/WebSocket server (feature-gated)
│   ├── shell/                  # Human shell, projection pipeline
│   ├── skills/                 # Skill loader
│   ├── test_runner/            # Test execution, parsing, reporting
│   ├── theme/                  # Theme system
│   ├── tool/                   # Built-in tools + tool_search discovery
│   ├── tool_advisor/           # Bounded tool-selection advisor
│   ├── tts/                    # Text-to-speech
│   ├── tui/                    # Terminal UI (Ratatui)
│   ├── upgrade/                # Self-upgrade
│   ├── util/                   # Utilities
│   ├── interactive_process*.rs # PTY lifecycle + attach protocol
│   ├── acp.rs                  # Agent Client Protocol adapter
│   ├── exec.rs                 # Non-interactive exec mode
│   ├── goal_verification.rs    # Goal verification pass
│   ├── managed_process.rs      # Managed process lifecycle
│   ├── run_rerun.rs            # Run rerun linkage
│   ├── background_task_migration.rs
│   ├── protocol_conversions.rs # Protocol conversion helpers
│   ├── git_*.rs                # Git service/mutations, network, recovery
│   ├── command_*.rs            # Command pipeline (planner, routing, outcome)
│   ├── job_*.rs                # Job dispatch, recovery
│   ├── lib.rs                  # Library root (re-exports)
│   └── main.rs                 # Binary entry point
├── crates/                     # Workspace crates (library-first)
│   ├── codegg-core/            # Domain types, bus, jobs, session, storage
│   ├── codegg-config/          # Config schema, paths, loading
│   ├── codegg-protocol/        # Protocol envelopes (CoreRequest/Response/Event)
│   ├── codegg-client/          # Frontend-side native client
│   ├── codegg-document/        # Editor document snapshots + transactions
│   ├── codegg-providers/       # LLM providers, crypto, circuit breaker
│   ├── codegg-git/             # Typed git operation model + risk
│   ├── egglsp/                 # LSP client (authoritative) + server registry
│   ├── egggit/                 # Read-only git facts
│   ├── eggsentry/              # Security scanning
│   ├── eggcontext/             # Token counting
│   ├── egglsp-test-server/     # Fake LSP server (NOT a workspace member)
│   └── eggwork-test-node/      # Standalone fixture workspace (NOT a member)
├── apps/desktop/               # Optional Tauri shell
├── tests/                      # Integration tests (215 files)
├── assets/                     # Agent definitions, prompts, themes
│   ├── agents/                 # Built-in agent TOML definitions
│   ├── prompts/                # Agent prompt templates
│   └── themes/halloy/          # Bundled themes
├── scripts/                    # CI guards, generators, validators
├── architecture/               # Architecture documentation (this index)
├── plans/                      # Design proposals and phase plans
├── docs/                       # Validation docs, manifests
└── examples/plugins/           # Plugin SDKs and reference plugins
```

## Static guards

Run these after changing execution surfaces or adding workspace crate dependencies:

```bash
scripts/verify.sh quick                                          # canonical sanity sweep
bash scripts/check-core-boundary.sh                              # codegg-core boundary
python3 scripts/check_daemon_cwd_usage.py                        # workspace-bound daemon path guard
python3 scripts/check_scheduler_bypass.py                        # scheduler-bypass guard
python3 scripts/check_execution_ownership.py                     # process-spawn ownership manifest
python3 scripts/check_git_forbidden_patterns.py                  # git secret boundary + policy drift
python3 scripts/generate_builtin_agents.py --check              # agent asset staleness
python3 scripts/check_projection_transport_isolation.py          # projection transport guard
```

Full `check_*` inventory lives in `scripts/` (41 guards) and additionally covers audit coverage/invariants, authorization matrix, discovery invariants, identity path usage, project-catalog invariants, provider-connections coverage/tombstones, sandbox contract, tool-broker boundary, TUI project authority, and websocket bounds. See `AGENTS.md` for the complete change-triggered guard list.

## Source verification

Verified against the working tree on **2026-10-06**. Counts in this document were re-measured directly (tool registrations, distinct tool names, tool source files, LSP server definitions, `codegg-core` module count, `AppEvent` variants, slash-command registry length, built-in agents, bundled themes, TUI state modules, `CREATE TABLE` count, `STORAGE_LAYOUT_VERSION`, git operation/risk counts, workspace members, integration test files, guard scripts).

This file supersedes the earlier overview's stale counts (77 docs / 53 tool registrations / 53 `AppEvent` variants / 142 slash commands / 71 tables / layout v56 / 21 guards / 189 tests) and adds previously undocumented surfaces: `src/tool_advisor/`, `crates/codegg-document/`, `src/command/`, `src/tui/document_session.rs`, `src/tui/interactive_terminal.rs`, `apps/desktop/`, and the excluded `eggwork-test-node` fixture workspace.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime ownership migration and M006 closure"). The `src/` tree above gained two root modules that were missing: `src/decision.rs` (the opt-in System One-compatible `DecisionEngine` backend) and `src/decision_sdm.rs` (the opt-in pinned SDM local Rank backend, gated behind `--features decision-runtime-sdm`). The same commit deleted thirteen `src/tool_advisor/` modules (`contextual.rs`, `late_interaction.rs`, `operating_point.rs`, `requalify.rs`, `retrieval_architecture.rs`, `retrieval_projection.rs`, `retrieval_signal.rs`, `retrieval_signal_v2.rs`, `sequence_encoder.rs`, `sequence_qualification.rs`, `sequence_ranking.rs`, `sequence_retrieval.rs`, `training.rs`), leaving `causal_active.rs`, `causal_frontier.rs`, `causal_observe.rs`, `context_v2.rs`, `decision_adapter.rs`, `mod.rs`, `order_invariance.rs`, `retrieval_relevance.rs`, and `training_data.rs`. See [tool-advisor.md](tool-advisor.md) for the decision-subsystem contract and the retired-work history.
