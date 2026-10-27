# CodeGG Architecture Overview

CodeGG is a high-performance AI coding agent built in Rust, designed for terminal-based interaction with deep IDE and LSP integration.

This document is the **bird's eye view**: how the system is decomposed, how a turn actually flows end to end, and an **index into one deep-dive document per discrete module/component** in this directory.

> **How to read this directory**
>
> - Start here for shape and vocabulary.
> - Follow a module link for the contract, invariants, and file-level detail of that one component.
> - Every deep-dive doc ends with a *Source verification* note naming what was checked against code and when.
> - The `## Verified counts` table below is the **single source of truth for counts**. Verify each row against its listed `Source`; do not copy a number from one document to another.

## Contents

- [Deployment topology](#deployment-topology)
- [System architecture](#system-architecture)
- [How everything works together](#how-everything-works-together)
- [Command execution pipeline](#command-execution-pipeline)
- [Module map](#module-map) — the index, grouped by layer
- [Tools and capabilities at a glance](#tools-and-capabilities-at-a-glance)
- [Verified counts](#verified-counts)
- [Feature gates](#feature-gates)
- [Database schema](#database-schema)
- [Data flow](#data-flow)
- [Key architectural patterns](#key-architectural-patterns)
- [Directory layout](#directory-layout)
- [Deep-dive index](#deep-dive-index) — every document in this directory
- [Static guards](#static-guards)

## Deployment topology

The supported distribution remains one `codegg` executable per target. The daemon is a user-scoped singleton discovered and started by that executable; the TUI and daemon share the same business-logic libraries and invocation contracts. Runtime-safety milestone M007 measured the dependency and release profiles and retained this no-split topology because a daemon/TUI split did not produce a material deployment improvement without adding packaging or ownership complexity. Future role-specific binaries require a new measured deployment constraint and compatibility plan; they are not an advertised installation mode.

An optional Tauri desktop shell lives under `apps/desktop` and is built with its own Rust and JavaScript toolchains. It has its own renderer-owned, remappable input model with mouse parity — see [the desktop's input model](desktop.md#the-desktops-input-model) — and its shortcuts are **in-window only**: there is no OS-global shortcut. Its Rust host uses `codegg-client` to connect to or start the existing `codegg` daemon; it is not a second daemon binary and does not change the root Cargo workspace's Rust 1.89 contract. See [desktop architecture](desktop.md).

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

Modules are grouped by architectural layer. Each row links to the deep-dive document for that component. The **Source** column names the owning code, so a reader can go from a row to its implementation without a second lookup.

### Agent Layer — Orchestration and Execution

The agent layer owns the execution cycle: receiving user input, routing through the LLM, executing tools, and managing multi-agent coordination.

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Agent | Main agent loop, compaction, routing, durable delegated-run orchestration | `src/agent/` | [agent.md](agent.md) |
| Agent Tool Surface | Model-facing tool definitions, categories, and gating rules | `src/tool/mod.rs` | [agent-tool-surface.md](agent-tool-surface.md) |
| Asset Context / Snapshot | Explicit context, immutable snapshot builder, generation coordinator, bounded operator refresh, turn/agent-run pinning | `src/context/` | [agent.md](agent.md) |
| ACP | Agent Client Protocol v1 stdio adapter over the singleton daemon | `src/acp.rs` | [acp.md](acp.md) |
| Command Intent | Command classification, risk assessment, execution capability model — pipeline stage 1 | `src/command_intent/` | [command_intent.md](command_intent.md) |
| Command Planner | Maps classified intents to execution backends, permissions, projection policy — stage 2 | `src/command_planner.rs` | [command_planner.md](command_planner.md) |
| Command Routing | Compatibility facade over the pre-M006 routing API — stage 3 | `src/command_routing.rs` | [command_routing.md](command_routing.md) |
| Test Runner | Test resolution, stdout/stderr parsing, failure taxonomy, previous-failures index | `src/test_runner/` | [test_runner.md](test_runner.md) |
| Python Scripting | Analyze/Transform/Verify modes, AST risk scanning, Landlock sandboxing | `src/python_script/` | [python_scripting.md](python_scripting.md) |
| Research | Structured research pipeline: sources → evidence → claims → verification | `src/research/` | [research.md](research.md) |
| Work Orders | Durable work-order lifecycle and plan item tracking | `crates/codegg-core/src/work_order/` | [work_orders.md](work_orders.md) |
| Work Plan | Plan item tracking, evidence binding, repository binding, todo sync | `crates/codegg-core/src/work_plan/` | [work_plan.md](work_plan.md) |
| Model Profile / Task State | Model behavioral profiles, todo/task state machine, injection and projection | `crates/codegg-core/src/model_profile/`, `task_state/` | [model_profile_task_state.md](model_profile_task_state.md) |
| Blackboard Contract Matrix | The frozen comparison baseline for the multi-agent workstream: the six capabilities, their authorities, their production consumers, and the S0–S4 configuration table | `architecture/blackboard_contract_matrix.md` | [blackboard_contract_matrix.md](blackboard_contract_matrix.md) |

### Tool Layer — Capabilities and Execution

The tool layer defines the built-in tools the agent can invoke, the backend abstraction for dispatch, and the deterministic validation pipeline.

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Tool | `Tool` trait, `ToolRegistry`, `ToolCatalog` + `tool_search` on-demand discovery, backend abstraction, all built-in tools | `src/tool/` | [tool.md](tool.md) |
| Tool Broker | Single canonical execution boundary for production tool calls — ordered policy pipeline, typed results | `src/tool/broker.rs` | [tool_broker.md](tool_broker.md) |
| Tool Programs | Durable program domain, storage, call ledger, lineage, replay | `crates/codegg-core/src/tool_program/` | [tool_programs.md](tool_programs.md) |
| Tool Program Language | Restricted-Python language specification — parse-only, fail-closed, deterministic IR | `crates/codegg-core/src/tool_program/` | [tool_program_language.md](tool_program_language.md) |
| Deterministic Tools | Eggsact in-process deterministic validators | `src/eggsact/`, `src/tool/deterministic.rs` | [deterministic_tools.md](deterministic_tools.md) |
| Preflight | Harness-side eggsact validation before mutating operations — never model-facing | `src/preflight/` | [preflight.md](preflight.md) |
| Tool Advisor | Bounded local semantic/causal tool-selection advisor (opt-in) | `src/tool_advisor/` | [tool-advisor.md](tool-advisor.md), [spike](tool-advisor-framework-spike.md) |
| Decision Engine | Backend-neutral decision contract with an optional System One-compatible backend; explicit opt-in, bounded fallback; owns advisor decision paths | `src/decision.rs` | [tool-advisor.md](tool-advisor.md) |
| SDM Decision Backend | Adapter from the backend-neutral decision interface to the pinned SDM local artifact runtime | `src/decision_sdm.rs` | [tool-advisor.md](tool-advisor.md) |
| Interactive Process | PTY lifecycle + attach protocol — distinct from finite managed process runs | `src/interactive_process.rs` | [process-tool-execution-ownership.md](process-tool-execution-ownership.md) |
| Managed Process | Finite managed process lifecycle, including the Windows ACL lease | `src/managed_process.rs`, `src/managed_process/` | [process-tool-execution-ownership.md](process-tool-execution-ownership.md) |
| Git Service / Mutations / Network / Recovery | Canonical read executor plus typed guarded mutations, network policy, recovery | `src/git_*.rs` | [git.md](git.md) |
| Git Publication | Staging, publishing, and reconciliation of private index publications | `src/git_publication/` | [git.md](git.md) |
| Search Backend | Dispatch between eggsearch MCP and explicit disabled mode | `src/search_backend/` | [search_backend.md](search_backend.md) |

### Tools and capabilities at a glance

Built-in tools live in `src/tool/` (plus `src/tool/bash/`) and `src/context/`. The registry is assembled in `src/tool/mod.rs::with_options()`; visibility is gated by todo policy, evidence-backend availability, LSP/security backend availability, eggsact availability, and context-read config. A `DisabledTool` stub substitutes a typed error when a backend-gated domain is unavailable, so it carries a runtime-supplied name rather than a fixed one. See [tool.md](tool.md) and [agent-tool-surface.md](agent-tool-surface.md) for the authoritative surface.

| Capability | Tools |
|------------|-------|
| Filesystem | `read`, `write`, `edit`, `apply_patch`, `replace`, `diff`, `glob`, `grep`, `list` |
| Shell / execution | `bash`, `verify`, `terminal`, `test`, `batch`, `python_script` |
| Source intelligence | `lsp`, `lsp_read`, `lsp_preview_apply`, `codesearch`, `repo_map`, `ci_watch` |
| Version control | `git`, `git_query`, `git_read`, `commit` |
| Research / evidence | `research`, `research_search`, `repo_search`, `repo_fetch`, `security_search`, `batch_fetch`, `evidence_bundle`, `websearch`, `webfetch` |
| Planning / tasks | `task`, `job`, `todoread`, `todowrite`, `plan_enter`, `plan_exit`, `goal_get`, `goal_request_completion`, `goal_update_progress`, `work_order`, `work_plan_get`, `work_plan_update_item` |
| Review / quality | `review`, `security`, plus 13 eggsact deterministic validators: `text_equal`, `text_diff_explain`, `text_replace_check`, `text_inspect`, `text_fingerprint`, `text_security_inspect`, `validate_json`, `validate_toml`, `path_normalize`, `identifier_inspect`, `structured_data_compare`, `command_preflight`, `config_preflight` |
| Extension / resource | `mcp_resource_read`, `mcp_resource_search`, `extension_search`, `extension_install_request`, `memory_get`, `memory_search` |
| Programmatic / meta | `tool_program`, `tool_search`, `question`, `skill`, `skill_proposal`, `image`, `context_read`, `invalid` |

### TUI Layer — User Interface

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| TUI | Ratatui terminal UI, async command pattern (spawn-and-complete), state management | `src/tui/` | [tui.md](tui.md) |
| Command | Slash command registry assembled from markdown manifests | `src/command/`, `src/tui/command.rs` | [command.md](command.md) |
| Theme | Frontend-neutral theme system (SemanticTheme → ratatui, Halloy) | `src/theme/` | [theme.md](theme.md) |
| Human Shell | `!`/`!!` commands, projection pipeline, safety policy, redaction | `src/shell/` | [human_shell.md](human_shell.md) |
| IDE | VS Code / JetBrains detection and diff viewing | `src/ide/` | [ide.md](ide.md) |
| Documents | Shared editor document foundation — frontend-neutral text snapshots and transactions | `src/document_service.rs`, `crates/codegg-document/` | [document.md](document.md) |
| Desktop | Optional Tauri desktop shell over `codegg-client` | `apps/desktop/` | [desktop.md](desktop.md) |

### Core Layer — Daemon and Transport

The core layer owns the singleton daemon lifecycle, transport adapters, request routing, and the workspace registry.

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Core | `CoreClient` facade, transport adapters, daemon lifecycle, request handling | `src/core/` | [core.md](core.md) |
| Command Catalog | **The daemon's single definition of every command** — identity, name, aliases, description, kind, argument forms, and the catalog's bounds | `crates/codegg-core/src/command_catalog.rs` | [command.md](command.md) |
| Frontend Actions | The **declared, finite** set of actions a frontend may execute for a frontend-effect form — closed and non-mutating | `crates/codegg-core/src/frontend_action.rs` | [command.md](command.md) |
| Command Executor | The daemon's table-driven dispatch from a catalog form to a daemon request, its serve-ability predicate, and its audit as a canonical `Run` | `src/core/daemon_commands.rs` | [command.md](command.md) |
| Workspace | Workspace registry, canonical root tracking, immutable `Arc`-wrapped execution context | `crates/codegg-core/src/workspace.rs` | [workspace.md](workspace.md) |
| Workspace Services | Per-workspace service bundles, single-flight activation, user-scoped catalog | `crates/codegg-core/src/workspace_services.rs` | [workspace_services.md](workspace_services.md) |
| Jobs | Durable jobs, attempts, schedules, recovery, idempotency | `crates/codegg-core/src/jobs/` | [jobs.md](jobs.md) |
| Scheduler | Global admission control, fair queue, executor dispatch, permit lifecycle | `src/scheduler/` | [scheduler.md](scheduler.md) |
| Session | SQLite session storage, message history, migrations, checkpointing | `crates/codegg-core/src/session/` | [session.md](session.md) |
| Storage | SQLite initialization and pooling — user-scoped catalog + project store | `crates/codegg-core/src/storage/` | [storage.md](storage.md) |
| Bus | Event bus publish/subscribe, `PermissionRegistry`, `QuestionRegistry` | `crates/codegg-core/src/bus/` | [bus.md](bus.md) |
| Projection | Frontend-neutral derived view, deterministic reducer, scoped subscriptions, durable replay | `crates/codegg-core/src/projection_replay/` | [projection.md](projection.md) |
| Protocol | Shared request/response/event envelopes and capability structs | `crates/codegg-protocol/` | [protocol.md](protocol.md) |
| Identity | Typed domain identity foundation — opaque string newtypes with UUIDv4 | `crates/codegg-core/src/identity.rs` | [identity.md](identity.md) |
| Project Identity Storage | Durable logical-project and repository authority, binding, reconciliation | `crates/codegg-core/src/project_storage.rs` | [project_identity_storage.md](project_identity_storage.md) |
| Project Catalog | Daemon-owned project catalog — list, get, register, archive, restore | `crates/codegg-core/src/project_catalog.rs` | [project_catalog.md](project_catalog.md) |
| Authorization | Daemon operation-boundary gate — principals, capability map, attribution | `crates/codegg-core/src/authorization/` | [authorization.md](authorization.md) |
| Audit | Append-only structural audit store and instrumentation coverage | `crates/codegg-core/src/audit.rs` | [audit.md](audit.md) |
| Collaboration | Project-scoped channels, threads, mentions, edits, retention | `crates/codegg-core/src/collaboration/` | [collaboration.md](collaboration.md) |
| Presence | Ephemeral project-scoped presence — leases, heartbeat, expiry | `crates/codegg-core/src/presence.rs` | [presence.md](presence.md) |
| Workstream Claims | Durable group-scoped task/path/interface claims and prerequisite edges; intent only | `crates/codegg-core/src/workstream_claim.rs` | [workstream_claims.md](workstream_claims.md) |
| Checkout Writer Permit | Durable, generation-fenced per-checkout writer token; state machine, CAS, quarantine, bounded waiter | `crates/codegg-core/src/writer_permit.rs` | [writer_permit.md](writer_permit.md) |
| Build Admission | Counted, workspace-scoped build quota; configuration-owned limit; coarse authorized group status | `crates/codegg-core/src/jobs/quota.rs` | [build_admission.md](build_admission.md) |
| Session Launch | Durable, idempotent session launch into an isolated managed checkout; stage machine, saga, binding | `crates/codegg-core/src/session_launch.rs` | [session_launch.md](session_launch.md) |
| Server | Axum HTTP/WS server (feature `server`) — remote TUI, REST, SSE, token auth | `src/server/` | [server.md](server.md) |
| Client | Remote TUI WebSocket client with resume/replay | `src/client/` | [client.md](client.md) |
| Exec | Non-interactive exec mode for CI/CD with JSON I/O | `src/exec.rs` | [exec.md](exec.md) |
| Error | Centralized `AppError` enum with classification | `src/error.rs` | [error.md](error.md) |

### Provider Layer — LLM Backends

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Provider | LLM provider implementations, streaming, circuit breaker, connection lifecycle | `crates/codegg-providers/` | [provider.md](provider.md) |
| Model Adapters | Model adapter patterns and conversions | `crates/codegg-providers/` | [model-adapters.md](model-adapters.md) |
| Config | Configuration schema, paths, loading, validation, file watching | `crates/codegg-config/` | [config.md](config.md) |

### Integration Layer — External Systems

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| LSP | Language Server Protocol — `egglsp` is authoritative; `src/lsp/` is a thin shim | `crates/egglsp/`, `src/lsp/` | [lsp.md](lsp.md) |
| MCP | Model Context Protocol client — local/remote servers, OAuth, reconnection | `src/mcp/` | [mcp.md](mcp.md) |
| Plugin | WASM plugin system (Wasmtime), manifest parsing, hooks, registry, lifecycle | `src/plugin/` | [plugin.md](plugin.md) |
| Search Backend | Wrapper between search tools and eggsearch MCP, with explicit disabled mode | `src/search_backend/` | [search_backend.md](search_backend.md) |
| Install / Sidecars | Installation-owned runfile resolution for the sandbox helper and eggsearch sidecar | `src/install.rs` | [upgrade.md](upgrade.md) |

### Security Layer

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Permission | Tool/path access control, loop detection, mode-based permissions | `src/permission/` | [permission.md](permission.md) |
| Policy Surface | Frontend-neutral approval/sandbox user-surface logic | `src/policy_surface.rs` | [permission.md](permission.md) |
| Security | SSRF protection, internal IP validation, Landlock filesystem sandboxing | `src/security/` | [security.md](security.md) |
| Eggsentry | Deterministic security scanning — secrets, commands, dependencies, unsafe code | `crates/eggsentry/` | [security.md](security.md) |
| Scratch | Daemon-owned scratch resources for model execution — session-derived roots, never caller-supplied paths | `src/scratch.rs` | [security.md](security.md) |
| Crypto | AES-256-GCM encryption, Argon2id key derivation | `crates/codegg-providers/src/crypto.rs` | [crypto.md](crypto.md) |
| Auth | Authentication and credential management | `src/auth/` | [auth.md](auth.md) |
| Authorization | Operation-boundary capability enforcement | `crates/codegg-core/src/authorization/` | [authorization.md](authorization.md) |
| Audit | Append-only security record | `crates/codegg-core/src/audit.rs` | [audit.md](audit.md) |
| Approval Reviewer | LLM-assisted approval review surface | `src/agent/` | [approval_reviewer.md](approval_reviewer.md) |

### Native Crates (Workspace)

Codegg follows a **library-first, MCP-second** tool architecture: durable tool domains live in workspace crates and are consumed directly in-process. The same crates can later expose optional MCP adapter binaries without changing model-facing tool names. See [native_crates.md](native_crates.md).

| Crate | Purpose |
|-------|---------|
| `codegg-core` | Domain types: agent convergence, authorization, audit, bus, decision semantics, error, goal, identity, jobs, memory, model profile, process admission, project catalog/discovery/storage, projection replay, provider connections, repository lineage, resilience, run store, session, snapshot, storage, task state, tool programs, workspace, workspace services, worktree, writer permit |
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

| Module | Purpose | Source | Deep dive |
|--------|---------|--------|-----------|
| Hooks | Lifecycle hooks for agent events | `src/hooks/` | [hooks.md](hooks.md) |
| Memory | Persistent memory across sessions | `crates/codegg-core/src/memory/` | [memory.md](memory.md) |
| Goal | Goal tracking and post-turn verification | `crates/codegg-core/src/goal/` | [goal.md](goal.md) |
| Snapshot | File state capture and restore | `crates/codegg-core/src/snapshot/` | [snapshot.md](snapshot.md) |
| Worktree | Git worktree management and leases | `crates/codegg-core/src/worktree.rs`, `worktree_service.rs` | [worktree.md](worktree.md) |
| Run Store | Persistent run index and artifact storage | `crates/codegg-core/src/run_store.rs` | [run_store.md](run_store.md) |
| Resilience / Retry | Circuit breaker and retry policy | `crates/codegg-core/src/resilience.rs` | [resilience.md](resilience.md), [retry.md](retry.md) |
| Skills | Runtime skill loader and activation | `src/skills/` | [skills.md](skills.md) |
| Context/Compaction Ownership | Single-owner map for compaction budgets/triggers/strategy | `architecture/context-compaction-ownership.md` | [context-compaction-ownership.md](context-compaction-ownership.md) |
| Process/Tool Execution Ownership | Canonical process-ownership map, inventoried in `docs/execution-ownership.toml` | `architecture/process-tool-execution-ownership.md` | [process-tool-execution-ownership.md](process-tool-execution-ownership.md) |
| Directory Listing | The single daemon-neutral workspace directory walker | `src/directory_listing.rs` | [workspace.md](workspace.md) |
| TTS | Text-to-speech | `src/tts/` | [tts.md](tts.md) |
| Upgrade | Verified managed-runfile self-upgrade | `src/upgrade/` | [upgrade.md](upgrade.md) |
| Util | Clipboard, fuzzy matching, pricing, metrics | `src/util/` | [util.md](util.md) |
| Testing | Test taxonomy, isolation, runtime rules, CI profile | `architecture/testing.md` | [testing.md](testing.md) |
| Windows MXC Feasibility | The macOS-equivalent containment assessment for Windows | `architecture/windows-mxc-feasibility.md` | [windows-mxc-feasibility.md](windows-mxc-feasibility.md) |

## Verified counts

Every count below was re-measured against the current tree on **2026-10-25**, each against its own listed `Source`, in the merged tree that carries `origin/main` `69ceec814` together with the delegated-run corrective line's branch tip `a5a87fd31`. No number was carried over from another tree.

Re-measure them at any time with:

```bash
python3 scripts/measure_overview_counts.py
```

The method behind each row — including the two that are non-obvious (enum-variant counting and production-tool-name detection) — is recorded in [count_audit_history.md](count_audit_history.md).

Two rows were **remeasured on 2026-10-21** for the provider-reasoning-control
milestone, each against its own listed `Source` rather than copied from
another row. `CI guard scripts` (63 → 65) was measured against
`scripts/check_*`: the count carries both the open-reasoning-vocabulary guard
and the model-profile capability mirror guard that landed with this
milestone. The `Storage layout version` row was **stale** (83 → 84), measured
against `crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION`: the
reasoning-selection migration that landed before this milestone moved the
version, and the row had not been re-measured since. The remaining rows were
left alone — this milestone adds no architecture document, no `codegg-core`
top-level module, no protocol variant, no database table, no integration test
file, and no migration of its own.

Every row was then **remeasured again on 2026-10-21**, when the whole
provider-reasoning-control milestone chain closed. Four rows had drifted and
were corrected, each against its own listed `Source`: `CoreRequest` variants
(**261 → 264**, 238 struct + 26 unit), `CoreResponse` variants (**171 → 173**),
authorized operations (**255 → 258**, distinct `OperationDescriptor::new`
names), and `CI guard scripts` (**65 → 66**, the TUI reasoning-surface guard).
The first three were missed by milestone 002's closure record, which claimed
the table had been remeasured when it had not; the fourth is this milestone's
own guard. The remaining rows were **confirmed unchanged** by measurement, not
by assumption: architecture docs **87**, tool registrations **65**
(`registry.register` + `registry.register_boxed` inside `with_options()`),
distinct production tool names **61**, tool source files **80**, LSP server
definitions **39**, `codegg-core` modules **47**, `AppEvent` variants **60**,
`CoreEvent` variants **92**, `ProjectionEvent` variants **47**, identity
newtypes **19**, built-in slash commands **167**, built-in agents **10**,
bundled themes **50**, TUI state modules **33** (34 `.rs` files incl.
`mod.rs`), database tables **88** (89 distinct `CREATE TABLE` names less the
transient `origin_attribution_new`), storage layout version **84**, Git
operation variants **54**, Git risk classes **11**, workspace member crates
**11** (+ root), and integration test files **280**.

Six rows moved again on **2026-10-22**, when the provider-reasoning-control
milestone chain merged onto `main`; each was re-measured against its own
listed `Source` rather than merged. `CoreRequest` variants **263 → 266**
(240 struct + 26 unit), `CoreResponse` variants **172 → 174** (173 struct +
1 unit), authorized operations **257 → 260** (the distinct operation names in
`operation_descriptor`, with the representative list's 250 recorded beside
it), `Storage layout version` **87 → 88** (the branch's reasoning-selection
migration was renumbered from v84 to **v88**, because `main` had already
published v84 through v87), integration test files **296 → 298** (the new
`tests/reasoning_overlay_narrowing.rs`, and one more), and CI guard scripts
**65 → 70** (`ls scripts/check_*` — `main`'s tree carries 66, and this chain
contributes four: `check_reasoning_intent_fidelity.py`,
`check_reasoning_vocabulary_open.py`, `check_model_profile_capability_mirror.py`
and `check_tui_reasoning_surface.py`). The remaining rows were **confirmed
unchanged** by measurement: architecture docs **90**, tool registrations **67**,
distinct production tool names **61**, tool source files **83**, LSP server
definitions **39**, `codegg-core` modules **54**, `AppEvent` variants **61**,
`CoreEvent` variants **92**, `ProjectionEvent` variants **47**, identity
newtypes **19**, built-in slash commands **168**, built-in agents **10**,
bundled themes **50**, TUI state modules **33** (34 `.rs` files incl.
`mod.rs`), database tables **95 statements / 91 tables**, Git operation variants
**54**, Git risk classes **11**, and workspace member crates **11** (+ root).

Three rows moved again on **2026-10-23**, when the provider-reasoning-control
milestone chain was reconciled with `main` for the third time — `main` had
advanced by 79 commits, carrying the CI/test-throughput recovery workstream
and its M004 root-test-target family consolidation. The **integration test
files** row changed its **Source**, not just its number: `main` consolidated
the root integration test *modules* into family directories, so `ls tests/*.rs`
alone now reports 4 and no longer measures the suite. The row's Source is
therefore `tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper
tree excluded, and the count is **321** (4 `tests/*.rs` plus 317
`tests/*/*.inc`). The **CI guard scripts** row is **80** (`ls scripts/check_*`
— `main`'s tree carries 76, and this chain contributes four:
`check_reasoning_intent_fidelity.py`, `check_reasoning_vocabulary_open.py`,
`check_model_profile_capability_mirror.py` and `check_tui_reasoning_surface.py`).
The **tool source files** row was **stale on `main` and is corrected rather
than incremented**: it read 83, but `main` added
`src/tool/bash/tests/git_fixture.rs` without re-measuring the row, so
re-measured against its own listed `Source` (`src/tool/**.rs`, incl. `bash/`)
it is **84**. This chain contributes **no** root integration test file of its
own — `crates/codegg-core/tests/reasoning_overlay_narrowing.rs` is outside the
row's Source, and the two root test files it edited
(`tests/model_preference_convergence.rs` and `tests/connect_tui_restoration.rs`)
were *moved* by `main`'s consolidation into
`tests/core_daemon/model_preference_convergence.inc` and
`tests/tui_render/connect_tui_restoration.inc`, so the merge kept both edits
without adding a file. Every other row was **confirmed unchanged** by
measurement: architecture docs **91**, tool registrations **67** (54 `register`
+ 13 `register_boxed`), distinct production tool names **61**, LSP server
definitions **39**, `codegg-core` modules **54**, `AppEvent` variants **61**,
`CoreRequest` variants **266** (240 struct + 26 unit), `CoreResponse` variants
**174** (173 struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent`
variants **47**, authorized operations **260**, identity newtypes **19**, built-in
slash commands **168**, built-in agents **10**, bundled themes **50**, TUI state
modules **33** (34 `.rs` files incl. `mod.rs`), database tables **95 statements
/ 91 tables**, storage layout version **88**, Git operation variants **54**, Git
risk classes **11**, and workspace member crates **11** (+ root).

Three rows moved once more on **2026-10-23**, when the repository audit
corrective workstream's C001, C002, C003, C004, C005, C006 and C007
milestones landed. Each milestone added one thing of each kind, and every value was
re-measured against its own listed `Source` with
`python3 scripts/measure_overview_counts.py` rather than incremented by hand:
**integration test files 327 → 334** (C001's
`tests/tui_render/nested_runtime_command_dispatch.inc`, C002's
`tests/security_permission/destructive_delete_classification.inc`, C004's
`tests/core_daemon/job_state_terminality_c004.inc` and
`tests/scheduler_jobs/job_status_projection_c004.inc`, C005's
`tests/security_permission/reviewer_broker_boundary.inc` and
`tests/tui_render/dialog_dispatch_coverage.inc`, and C006's
`tests/tui_render/tui_render_cost.inc` — all seven are members of
**existing** families, so the root package's **test-target** count did not
move), **guard scripts (all) 80 → 86** (the new
`scripts/check_no_nested_runtime_block_on.py`,
`scripts/check_destructive_risk_signal_coverage.py`,
`scripts/check_command_argument_contract.py`,
`scripts/check_job_state_terminality_agreement.py` and
`scripts/check_tui_render_cost.py` and C007's
`scripts/check_comment_budget.py` — C005 **repaired** the existing
`scripts/check_tool_broker_boundary.py` in place rather than adding a file, so
that row did not move for it), and **CI guard scripts 40 → 47** (those same
six new guards plus the repaired broker guard, each wired into
`scripts/verify.sh quick` and the hosted CI guard step; a guard's two
`ci.yml` steps — the guard and its `--self-test` — are one distinct script).
No row's **measure** changed. The remaining 21 rows were **confirmed
unchanged** by measurement, not by assumption.

One row moved once more on **2026-10-23**, when the file picker surface
corrective's M001 landed: **integration test files 334 → 335**
(`tests/tui_render/file_picker_surface_corrective.inc`). The new file is a
member of the **existing** `tui_render` family, so the root package's
**test-target** count did not move, and no row's **measure** changed. The
count was re-measured against its own listed `Source` with
`python3 scripts/measure_overview_counts.py`.

One row moved again on **2026-10-24**, when the desktop-GUI parity phase
added its root-side half: **integration test files 335 → 336**
(`tests/frontend_parity/tui_reconstruction.inc`). The new file is a member of
the **new** `frontend_parity` family, so the root package's **test-target**
count moved by one with it; no row's **measure** changed. The count was
re-measured against its own listed `Source` with
`python3 scripts/measure_overview_counts.py`.

That measurement was taken before this line was reconciled with `main`, and
`main` had moved the same row on its own side, so the merged tree's count is
neither parent's: `main` measured **338** and this line adds
`tests/frontend_parity/tui_reconstruction.inc`, for a merged **339**. The
merged value was **measured** in the merged tree with
`python3 scripts/measure_overview_counts.py` — not incremented from either
parent — and `scripts/check_test_target_references.py` checks the recorded row
against the tree, so the two cannot drift.

Three further rows moved on this line's side of the merge, each by the same
two-request delta: the desktop-GUI phase adds `CoreRequest::GitRead` and
`CoreRequest::SkillCatalogueList`, so `CoreRequest` variants are **268**
(242 struct + 26 unit), `CoreResponse` variants are **176** (175 struct + 1
unit), and authorized operations are **262** — with the
`representative_requests()` list holding **252**. Every other row was
**confirmed unchanged** by measurement, not by assumption: architecture docs
**92**, distinct production tool names **75**, tool source files **83**, LSP
server definitions **39**, `codegg-core` modules **54**, `AppEvent` variants
**61**, `CoreEvent` variants **92**, `ProjectionEvent` variants **47**,
identity newtypes **19**, built-in slash commands **169**, built-in agents
**10**, bundled themes **50**, TUI state modules **33** (34 `.rs` files incl.
`mod.rs`), database tables **95 statements / 91 tables**, storage layout
version **88**, Git operation variants **54**, Git risk classes **11**, and
workspace member crates **11** (+ root). The *Guard scripts (all)* row was
**92** and the *CI guard scripts* row **55** on this line's own side of that
merge, both carried unchanged from the `main` it was reconciled with, which had
added the writer-fence, contained-worktree-grant, checkout-mediation,
daemon-cwd, Git-forbidden-pattern and Git-consistency-consumer guards.

One more reconciliation, on **2026-10-24**, merged this line's tip with the
`main` that carried the M008 corrective and the M007 planning line. Neither
side's numbers were correct for the merged tree, so every moved row was
**re-measured in the merged tree** with
`python3 scripts/measure_overview_counts.py` and **not** incremented from
either parent. *Integration test files* is **340** — `main`'s **339** plus this
line's `tests/frontend_parity/tui_reconstruction.inc`. *Guard scripts (all)*
is **93** (`ls scripts/check_*` — `main`'s `scripts/check_scratch_reclaim_invariants.py`
is in the merged tree and this line adds no guard), *CI guard scripts* is
**56** (the distinct `scripts/check_*` names in `.github/workflows/ci.yml`),
and the quick-only difference is unchanged at **14**. The `CoreRequest` (268),
`CoreResponse` (176) and authorized-operations (262) rows keep this line's
values, which the merged tree confirms. Every other row was **confirmed
unchanged** by measurement. `scripts/check_test_target_references.py` checks the
recorded integration-test-file row against the tree, so that row cannot drift.

The four milestones were developed on sibling branches from a common base and
each re-measured these rows in isolation, so each recorded only its own
increment. Merging them lost one increment per row in `architecture/overview.md`
— the two edits landed in different hunks, so the file merged without a
conflict — and the row values above are the canonical measurement of the
**merged** tree. The `codegg-core` modules row is still **54**: C004's new
`codegg-core/src/jobs/evidence.rs` is a **sub**module of `jobs`, not a
top-level `pub mod` in the crate's `lib.rs`. The full reconciliation is in
`architecture/count_audit_history.md`.

Four rows moved again on **2026-10-23**, when this line was reconciled with
`main` for the **fourth** time, merging `origin/main` `6e097dd3a` with the
five post-closure corrective milestones' branch tip `d5b7efc02`. Each moved
row was re-measured against its own listed `Source` in the merged tree, with
`python3 scripts/measure_overview_counts.py`, and **not** incremented from
either parent. *Integration test files* is **337** (`tests/*.rs` +
`tests/*/*.inc`, with the `tests/common/` helper tree excluded). *Guard
scripts (all)* is **90** (`ls scripts/check_*`). *CI guard scripts* is **53**
(the distinct `scripts/check_*` names in `.github/workflows/ci.yml` — it
remains a strict subset of `quick`, which runs **67** guards and runs **14**
guards CI does not). The *Tool registrations* row's **total** did not move — it
is still 67 — but its **split** did: **54 `register` + 13 `register_boxed` →
53 `register` + 14 `register_boxed`**, because C009 moved the `commit` tool off
the plain `register` path onto the fail-closed `UnrootedTool` wrapper, which
registers boxed rather than in place. Every other row was **confirmed
unchanged** by measurement, not by assumption: architecture docs **92**,
distinct production tool names **75**, tool source files **83**, LSP server
definitions **39**, `codegg-core` modules **54**, `AppEvent` variants **61**,
`CoreRequest` variants **266** (240 struct + 26 unit), `CoreResponse` variants
**174** (173 struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent`
variants **47**, authorized operations **260**, identity newtypes **19**,
built-in slash commands **169**, built-in agents **10**, bundled themes **50**,
TUI state modules **33** (34 `.rs` files incl. `mod.rs`), database tables **95
statements / 91 tables**, storage layout version **88**, Git operation variants
**54**, Git risk classes **11**, and workspace member crates **11** (+ root).
The full reconciliation is in `architecture/count_audit_history.md`.

Three rows moved again on **2026-10-24**, when this line was reconciled with
`main` for the **sixth** time, merging `origin/main` `a4bbba81e` (PR #183, the
composer's second post-closure corrective) with the writer-containment line's
branch tip `28ebe7163`. Each moved row was re-measured against its own listed
`Source` in the merged tree, with `python3 scripts/measure_overview_counts.py`,
and **not** incremented from either parent. *Guard scripts (all)* is **92**
(`ls scripts/check_*` — the union of the two guard sets: `main` added
`scripts/check_tui_composer_keymap_drift.py`). *CI guard scripts* is **55**
(the distinct `scripts/check_*` names in `.github/workflows/ci.yml` — it
remains a strict subset of `quick`, which runs **69** guards and runs **14**
guards CI does not). *Integration test files* is **338**, and it did **not**
move: `main`'s composer corrective extended the existing members
`tests/tui_render/tui_composer_claims.inc` and
`tests/tui_render/tui_composer_motions.inc` and rewrote the already-existing
`tests/tui_render/tui_composer_selection.inc`, so it added no new integration
test file — a new-file line in a diff stat is not a new file. The two sides'
test-file sets were differenced to confirm the union rather than inferred from
the diff stat. `AGENTS.md`'s published counts were corrected to the merged
tree's measured **69 quick / 55 CI**; the *quick runs 14 guards CI does not*
difference is unchanged at 14, re-derived by differencing the guards
`run_guards()` names against the guards `.github/workflows/ci.yml` names.
Every other row was **confirmed unchanged** by measurement, not by assumption:
architecture docs **92**, distinct production tool names **75**, tool source
files **83**, LSP server definitions **39**, `codegg-core` modules **54**,
`AppEvent` variants **61**, `CoreRequest` variants **266** (240 struct + 26
unit), `CoreResponse` variants **174** (173 struct + 1 unit), `CoreEvent`
variants **92**, `ProjectionEvent` variants **47**, authorized operations
**260**, identity newtypes **19**, built-in slash commands **169**, built-in
agents **10**, bundled themes **50**, TUI state modules **33** (34 `.rs` files
incl. `mod.rs`), database tables **95 statements / 91 tables**, storage layout
version **88**, Git operation variants **54**, Git risk classes **11**, and
workspace member crates **11** (+ root). The **Tool registrations** row is
unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** —
and it is named here rather than left implicit, because it is the row the
fourth reconciliation had to qualify with a split note. The comment budget was
re-banked on the merged tree: **124,310** comment lines, **1,095,646** words, a
**0.1330** ratio — over the branch's prior ceiling of 123,805 and still
**below** the 124,865 pre-reduction baseline, the same relation the fourth and
fifth reconciliations recorded. The full reconciliation is in
`architecture/count_audit_history.md`.


| Item | Count | Source |
|------|-------|--------|
| Architecture docs | 92 | `architecture/*.md` |
| Tool registrations in `with_options()` | 67 (53 `register` + 14 `register_boxed`) | `ToolRegistry::with_options()` (`src/tool/mod.rs`) — the registrations are the `registry.register(..)` / `registry.register_boxed(..)` calls in that constructor's body, not the constructor's own call sites |
| Distinct production tool names | 78 | `impl Tool for …` `name()` across `src/` (the whole tree — a production `Tool` impl may live with the subsystem that owns the tool's behaviour, e.g. `src/python_script/tool.rs`), **plus** the 16 eggsact validator names at their `EggsactTool::new()` call sites in `src/tool/deterministic.rs` — that family's `impl Tool for EggsactTool` returns `self.codegg_name`, so a literal-name scan cannot see it. 67 production `impl Tool for` blocks, of which 5 resolve their name at runtime; excludes the 9 test-only mock impls under `#[cfg(test)]`: 3 in `program_manifest.rs`, plus `catalog.rs`, `tool_search.rs`, `disabled.rs`, `broker.rs` and `src/scheduler/tool_program_executor.rs`. The runtime-named families that only *substitute* for an already-counted tool add no new name: `DisabledTool` carries the name of the tool it replaces (`lsp`, `lsp_read`, `security`) and `UnrootedTool` forwards its inner tool's name |
| Tool source files | 83 | `src/tool/**.rs` (incl. `bash/`; excl. the `bash/tests/` fixture, which is test support, not tool source) |
| LSP server definitions | 39 | `crates/egglsp/src/server.rs::server_definitions()` |
| `codegg-core` modules | 58 | `crates/codegg-core/src/lib.rs` (`pub mod`) |
| `AppEvent` variants | 61 | `crates/codegg-core/src/bus/events.rs` |
| `CoreRequest` variants | 271 (245 struct + 26 unit) | `crates/codegg-protocol/src/core.rs` |
| `CoreResponse` variants | 179 (178 struct + 1 unit) | `crates/codegg-protocol/src/core.rs` |
| `CoreEvent` variants | 97 | `crates/codegg-protocol/src/core.rs` |
| `ProjectionEvent` variants | 50 | `crates/codegg-protocol/src/projection/event.rs` |
| Authorized operations | 265 | `operation_descriptor` (`crates/codegg-core/src/authorization/policy.rs`) — 271 arms carrying 265 distinct operation names, covering all 271 `CoreRequest` variants; the `representative_requests()` list holds 229 of them |
| Identity newtypes | 19 | `typed_identity!` (`crates/codegg-core/src/identity.rs`) |
| Built-in slash commands | 169 | asserted by `built_in_command_count_matches_release_docs` |
| Built-in agents | 10 | `assets/agents/*.toml` |
| Bundled themes | 50 | `assets/themes/**/*.toml` |
| TUI state modules | 34 (35 `.rs` files incl. `mod.rs`) | `src/tui/app/state/` |
| Database tables (`CREATE TABLE`) | 99 statements / 95 tables | `crates/codegg-core/src/session/schema.rs` (96 distinct `CREATE TABLE` names, less the transient `origin_attribution_new` rebuild name; `migration_version`, `origin_attribution` and that rebuild each appear twice) |
| Storage layout version | 90 | `crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION` |
| Git operation variants | 54 | `crates/codegg-git/src/operation.rs` |
| Git risk classes | 11 | `crates/codegg-git/src/risk.rs` |
| Workspace member crates | 11 (+ root) | root `Cargo.toml` |
| Integration test files | 355 | `tests/*.rs` + `tests/*/*.inc` — the root integration test *modules*, with the `tests/common/` helper tree excluded |
| Guard scripts (all) | 102 | `scripts/check_*` — every guard the repository ships |
| CI guard scripts | 68 | `.github/workflows/ci.yml` — a strict subset of the above; quick runs 14 guards CI does not |

### Count reconciliation — delegated live transcript, M001–M003 (2026-10-26)

The delegated live transcript line moved **six** code-count rows. Every figure
below was **re-measured** against its own listed `Source` with
`scripts/measure_overview_counts.py`, not incremented.

- *`CoreRequest` variants* is **271** (245 struct + 26 unit) — the new
  `AgentRunTranscriptRead` arm, added **beside** the existing arms, never
  replacing one.
- *`CoreResponse` variants* is **179** (178 struct + 1 unit) — the new
  `AgentRunTranscriptRead` arm.
- *`CoreEvent` variants* is **97** — the new `AgentRunTranscriptAdvanced` arm.
  The row read **92** before this line and the tree already held **96** at
  `HEAD`: **four** of the movement was pre-existing drift, not this line's.
  This line moved it **96 → 97**.
- *`ProjectionEvent` variants* is **50** — the new
  `AgentRunTranscriptAdvanced` wire arm. The row read **47** and the tree held
  **49** at `HEAD`: the same **two** of the movement was pre-existing drift.
  This line moved it **49 → 50**.
- *Authorized operations* is **265** — the new `agent_run_transcript_read`
  descriptor. The row's note read "264 arms carrying 264 distinct operation
  names, covering all 270 `CoreRequest` variants; the
  `representative_requests()` list holds 254 of them". The arms are now
  **271**, the distinct names **265**, and the `CoreRequest` variants they
  cover **271**. The `representative_requests()` figure was **already** stale
  at `HEAD`: the list holds **229** arms there and holds **229** here — this
  line did not move it.
- *TUI state modules* is **34** (35 `.rs` files incl. `mod.rs`) — the new
  `src/tui/app/state/transcript_cache.rs`, the delegated tab's bounded
  per-tab paging cache.
- *Integration test files* is **355** (`tests/*.rs` + `tests/*/*.inc`, with the
  `tests/common/` helper tree excluded) — by **three**: the new
  `tests/core_daemon/delegated_transcript_m002.inc`,
  `tests/projection_replay/delegated_transcript_advance.inc` and
  `tests/tui_render/transcript_follow.inc`. All three sit inside the
  existing `core_daemon`, `projection_replay` and `tui_render` families, so
  the root package's **test-target** count did **not** move. The absolute
  figure is re-measured on the merged tree (`80171f452`), which carries one
  further test file from `main` beyond this line's own three.
- *Distinct production tool names* is **78** — **not** moved by this line: it
  adds no `impl Tool for` block and touches no file under `src/tool/`. The
  row read **75** and the tree already held **78** at `HEAD`, so the **three**
  of the movement was pre-existing drift. The row's own detail was stale with
  it: it counted **13** eggsact validator names where the tree holds **16**,
  and **8** test-only mock impls where the tree holds **9**.

*Guard scripts (all)* is **102** and *CI guard scripts* is **68**, both
unchanged: this line strengthened **existing** guards and added none. The
comment budget was **re-banked** on the merged tree
(`python3 scripts/check_comment_budget.py --bank`), which raised the recorded
ceilings for the files this line's comments grew and added the new files to
`docs/comment-budget.toml`.

### Count reconciliation — M008 (2026-10-24)

Three code-count rows moved when the managed-agent-scratch M008 line landed its
corrective pass. *Integration test files* moved to **339** (`tests/*.rs` +
`tests/*/*.inc`, with the `tests/common/` helper tree excluded — by one, the
new `tests/scheduler_jobs/scratch_record_less_tree.inc`, which drives a full
sweep pass over a tree whose owner record is missing). *Guard scripts (all)*
moved to **93** (`ls scripts/check_*` — the new
`scripts/check_scratch_reclaim_invariants.py`, which enforces the reclaim
path's error taxonomy, its single removal routine, the emptiness proof that
authorises a removal, and the one shared rule the census and the validator
both use). *CI guard scripts* moved to **56** (the `ci.yml` list, which gained
the same guard and its self-test). The quick-only difference was unchanged at
**14** guards CI does not run: the new guard runs in **both** `verify.sh quick`
and CI, so it adds no quick-only entry. Every other code-count row was
re-checked against its own listed `Source` and is unchanged.

### Count reconciliation — desktop parity merge (2026-10-24)

One code-count row moved when the desktop parity work merged into `main`.
*Integration test files* is **340** (`tests/*.rs` + `tests/*/*.inc`, with the
`tests/common/` helper tree excluded — the count moved by one, the new
`tests/frontend_parity/tui_reconstruction.inc`, which reconstructs a session
from the TUI's own projection and compares it against the desktop's). *Guard
scripts (all)* is **93** (`ls scripts/check_*` — unchanged: the desktop parity
work adds no guard), *CI guard scripts* is **56** (the `ci.yml` list, unchanged),
and the quick-only difference is unchanged at **14**. The *quick* guard count
is **70**. Every other code-count row was re-checked against its own listed
`Source` and is unchanged.

### Count reconciliation — the C005 diff-consumer runtime target (2026-10-24)

One row moved because of a change on this line — the standalone integration
target `tests/git_consistency_consumers_c005.rs`, which closes the Linux
sandbox C005 closure record's last open finding. *Integration test files* is
**341**, re-measured on the tree with `python3 scripts/measure_overview_counts.py`
rather than incremented. Every other row was confirmed unchanged by
measurement.

### Count reconciliation — the M007-D merge with `main` (2026-10-25)

`main` moved to `76b046fd3` (PR #196, the C009 hooks-path disposition) while
the M007-D branch was open, and three rows moved when the branch absorbed it.
*Integration test files* is **343** (`tests/*.rs` + `tests/*/*.inc`, with the
`tests/common/` helper tree excluded — `main`'s **342** plus this line's
`tests/core_daemon/command_catalog_m007d.inc`). *Guard scripts (all)* is
**99** (`ls scripts/check_*` — `main`'s **97** plus the command-catalog
ownership guard and the frontend-action-set guard this line added), and *CI
guard scripts* is **62** (`main`'s **60** plus the same two, which run in both
lanes). The *quick* guard count moved **74 → 76**, and the *quick runs 14
guards CI does not* difference is unchanged at **14** (both new guards run in
both lanes). `AGENTS.md`'s published counts moved to **76 quick / 62 CI**.
Every row was re-measured on the merged tree with
`python3 scripts/measure_overview_counts.py` rather than incremented.

The comment corpus moved **125,249 → 126,485** comment lines and **1,104,302 →
1,115,299** words, at a **0.1340** ratio. The move is the cost of this line's
own documentation — the closure record and the command catalog's eight
architecture documents — not of `main`'s change.

### Count reconciliation — the C009 hooks-path disposition (2026-10-25)

Three rows moved because of a change on this line — the contained worktree
child's argv now pins `core.hooksPath`, with a static guard and a standalone
integration target proving it. *Guard scripts (all)* is **97**, *CI guard
scripts* is **60** (the workflow gained the same guard and its self-test), and
*Integration test files* is **342** — the standalone target
`tests/contained_worktree_hooks_path.rs`. Every row was re-measured on the tree
with `python3 scripts/measure_overview_counts.py` rather than incremented. The
*quick* guard count moved **73 → 74**, the *quick runs 14 guards CI does not*
difference is unchanged at **14** (the new guard runs in both lanes), and
`AGENTS.md`'s published counts moved to **74 quick / 60 CI**. The quick lane's
`==>` lines moved from 121 to **123** (`run_guards` **119**, `run_quick` **4**).
Every other row was confirmed unchanged by measurement.

The comment corpus moved **125,212 → 125,249** comment lines and **1,103,943 →
1,104,302** words, at a **0.1337** ratio. The move is the cost of the new
target's documentation; it is partly offset by two genuine per-file reductions
in the same diff — `src/git_contained_worktree.rs` **284 → 280** and
`tests/contained_worktree_c008.rs` **171 → 170** — where three comment blocks
in the source duplicated what the named functions already say, and the
containment test's obtained-root block repeated the same paragraph the source
documents. The corpus remains **above** the 124,865 pre-reduction figure, which
is a historical reference and not a guard.

The table itself was also **repaired**: it carried **two** copies of the three
count rows — one set at **341** and one at **340** — a merge artifact from an
earlier reconciliation, in which both sides moved the same rows without the
same values. The duplicate was collapsed to one row set carrying the tree's
measurement, which is the shape the table is supposed to have.

### Count reconciliation — the ninth reconciliation with `main` (2026-10-25)

`origin/main` advanced to `cb4ceed9d`, carrying PRs #192 and #194 — the desktop keyboard-first input model (M007-C) and the desktop editor surface (M007-E). One file conflicted: the three count rows. *Guard scripts (all)* is **96** (`main` added `scripts/check_desktop_editor_surface_authority.py`) and *CI guard scripts* is **59** (the workflow gained the same guard and its self-test) — both unions of the two sides' sets. *Integration test files* is **341**, unchanged from the eighth reconciliation, because `main`'s two desktop milestones added no integration test file. The *quick* guard count moved **72 → 73**, the *quick runs 14 guards CI does not* difference is unchanged at **14**, `AGENTS.md`'s published counts moved to **73 quick / 59 CI**, and the quick lane's `==>` lines moved from 119 to **121** (`run_guards` **117**, `run_quick` **4**). Every other row was confirmed unchanged by measurement: architecture docs **92**, distinct production tool names **75**, tool source files **83**, LSP server definitions **39**, `codegg-core` modules **54**, `AppEvent` variants **61**, `CoreRequest` variants **268** (242 struct + 26 unit), `CoreResponse` variants **176** (175 struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent` variants **47**, authorized operations **262**, identity newtypes **19**, built-in slash commands **169**, built-in agents **10**, bundled themes **50**, TUI state modules **33** (34 `.rs` files incl. `mod.rs`), database tables **95 statements / 91 tables**, storage layout version **88**, Git operation variants **54**, Git risk classes **11**, and workspace member crates **11** (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`**.

The comment corpus did **not** move: the merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio, and the re-bank moved no per-file ceiling, because `main`'s two desktop milestones added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the seventh reconciliation already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.

### Count reconciliation — the eighth reconciliation with `main` (2026-10-24)

`origin/main` advanced to `6e8a6bb60`, carrying PRs #185 and #191 — the desktop chat region and side panel (M007-B) and the panel layer's authority guard. Nothing conflicted textually, but the auto-merge produced **two** copies of the three count rows, which is the shape a count table takes when both sides moved the same rows without the same values; the duplicate was collapsed to one row set carrying the merged tree's measurement. *Guard scripts (all)* is **95** (`main` added `scripts/check_desktop_panel_authority.py`) and *CI guard scripts* is **58** (the workflow gained the same guard and its self-test). *Integration test files* is **341**, unchanged — `main`'s M007-B work added no integration test file. The *quick* guard count moved **71 → 72**, the *quick runs 14 guards CI does not* difference is unchanged at **14**, `AGENTS.md`'s published counts moved to **72 quick / 58 CI**, and the quick lane's `==>` lines moved from 117 to **119** (`run_guards` **115**, `run_quick` **4**). Every other row was confirmed unchanged by measurement: architecture docs **92**, distinct production tool names **75**, tool source files **83**, LSP server definitions **39**, `codegg-core` modules **54**, `AppEvent` variants **61**, `CoreRequest` variants **268** (242 struct + 26 unit), `CoreResponse` variants **176** (175 struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent` variants **47**, authorized operations **262**, identity newtypes **19**, built-in slash commands **169**, built-in agents **10**, bundled themes **50**, TUI state modules **33** (34 `.rs` files incl. `mod.rs`), database tables **95 statements / 91 tables**, storage layout version **88**, Git operation variants **54**, Git risk classes **11**, and workspace member crates **11** (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`**.

The comment corpus did **not** move: the merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio, and the re-bank moved no per-file ceiling, because `main`'s new commits added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the seventh reconciliation already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.

### Count reconciliation — the seventh reconciliation with `main` (2026-10-24)

`origin/main` advanced to `4aa07fa72`, carrying PRs #184, #187, #188, #189 and
#190, and two files conflicted. Three rows moved, each re-measured against its
own listed `Source` in the merged tree rather than carried over from either
parent. *Integration test files* is **341** — the union of `main`'s two new
files and this line's one. *Guard scripts (all)* is **94** — the union of
`main`'s two new guards and this line's none. *CI guard scripts* is **57** —
the union of the two workflows' distinct `scripts/check_*` names. Both of
`main`'s new guards run in **both** `quick` and CI, so the *quick runs 14
guards CI does not* difference is unchanged at **14**, and the *quick* guard
count is **71**; `AGENTS.md`'s published counts moved with them, to **71
quick / 57 CI**. The quick lane's `==>` lines moved from 113 to **117**
(`run_guards` **113**, `run_quick` **4**). Every other row was confirmed
unchanged by measurement: architecture docs **92**, distinct production tool
names **75**, tool source files **83**, LSP server definitions **39**,
`codegg-core` modules **54**, `AppEvent` variants **61**, `CoreRequest`
variants **268** (242 struct + 26 unit), `CoreResponse` variants **176** (175
struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent` variants
**47**, authorized operations **262**, identity newtypes **19**, built-in slash
commands **169**, built-in agents **10**, bundled themes **50**, TUI state
modules **33** (34 `.rs` files incl. `mod.rs`), database tables **95 statements
/ 91 tables**, storage layout version **88**, Git operation variants **54**,
Git risk classes **11**, and workspace member crates **11** (+ root). The
**Tool registrations** row is unchanged too — still **67**, still **53
`register` + 14 `register_boxed`** — and it is named here rather than left
implicit, because it is the row the fourth reconciliation had to qualify with a
split note.

**The comment corpus is no longer below the 124,865 pre-reduction baseline,
and this is recorded rather than papered over.** The corpus is a ratchet — no
recorded ceiling may be exceeded — and the merged tree measures **125,212**
comment lines, **1,103,943** words and a **0.1337** ratio, which the re-bank
records exactly as measured. The **124,865** figure is the pre-reduction
corpus from the C007 comment-reduction workstream, and it is a **historical
reference, not a guard**: no script, no entry in `docs/comment-budget.toml`,
and no CI step enforces it. The corpus crossed it *before* this merge —
`main`'s own tree banked **125,161**, already **296 lines above** the
reference — so this merge neither crossed it nor caused it. The fourth, fifth,
sixth and ninth reconciliations each recorded the corpus *below* the baseline
because it *was* below on those trees. That relation no longer holds, and
saying otherwise would be false.

### Count reconciliation — M007-A desktop gate guard (2026-10-24)

Two code-count rows moved when the desktop build and hosted gate line added the
guard that keeps the desktop tree inside an automated gate. *Guard scripts
(all)* is **94** (`ls scripts/check_*` — the new
`scripts/check_desktop_gate_wiring.py`, which fails closed when the hosted
desktop qualification stops covering the desktop tree: a dispatch-only
trigger, a path filter that no longer names `apps/desktop/**`, a missing
`clippy` step for the desktop host, or a missing local entry point).
*CI guard scripts* is **57** (the distinct `scripts/check_*` names in
`.github/workflows/ci.yml` — it gained the same guard and its self-test). The
quick-only difference is unchanged at **14** guards CI does not run: the new
guard runs in **both** `verify.sh quick` and CI, so it adds no quick-only
entry. The *quick* guard count is **71**. Every other code-count row was
re-checked against its own listed `Source` and is unchanged.

### Count reconciliation — M007-E desktop editor surface (2026-10-25)

Two code-count rows moved when the desktop editor milestone added the guard
that keeps the editor's text authority where it belongs. *Guard scripts (all)*
is **96** (`ls scripts/check_*` — the new
`scripts/check_desktop_editor_authority.py`, which holds the desktop editor's
**one** bounded viewport slice as a recorded pair and refuses a host that
serves a second copy of the text, a renderer that keeps a whole-document
string or line array of its own, a viewport concatenated into one document
string, a renderer filesystem or network read, or a DTO contract that drifts
from the host's structs). *CI guard scripts* is **59** (the distinct
`scripts/check_*` names in `.github/workflows/ci.yml` — it gained the same
guard and its self-test). The quick-only difference is unchanged at **14**
guards CI does not run: the new guard runs in **both** `verify.sh quick` and
CI, so it adds no quick-only entry. The *quick* guard count is **73**.
*Integration test files* is unchanged at **340** — the guard adds a script, and
the milestone's host test lives under the desktop crate's own `tests/`
directory, which the root `tests/*.rs` Source does not count. The **desktop
panel authority guard's recorded bridge baseline** moved from **23** to **38**
targets in the same change, because the editor's fifteen bounded commands are
new bridge targets and that guard's own contract says a new target is a
deliberate edit to it, in the same change that adds it. Every other code-count
row was re-checked against its own listed `Source` and is unchanged.

### Count reconciliation — M007-B desktop panel authority guard (2026-10-24)

Two code-count rows moved when the desktop chat-region milestone added the
guard that keeps the renderer's panel layer free of authority. *Guard scripts
(all)* is **95** (`ls scripts/check_*` — the new
`scripts/check_desktop_panel_authority.py`, which holds the desktop's 23
bridge targets as a **recorded** set and refuses a panel or region module
that calls `invoke`, imports the bridge, holds a Tauri capability, reaches a
filesystem or a process, or keeps a durable frontend-owned store).
*CI guard scripts* is **58** (the distinct `scripts/check_*` names in
`.github/workflows/ci.yml` — it gained the same guard and its self-test). The
quick-only difference is unchanged at **14** guards CI does not run: the new
guard runs in **both** `verify.sh quick` and CI, so it adds no quick-only
entry. The *quick* guard count is **72**. *Integration test files* is
unchanged at **340** — the guard adds a script, not a test file. Every other
code-count row was re-checked against its own listed `Source` and is
unchanged.

### Count reconciliation — delegated-run corrective, merge with `main` (2026-10-24)

Merging `origin/main` (`c4fa30aeb`, the managed-agent-scratch M008 line) into
the delegated-run corrective branch moved the same three rows again, because
both lines added a test module and a guard. The measured values on the merged
tree are *Integration test files* **347**, *Guard scripts (all)* **96** and
*CI guard scripts* **62** — each the sum of the two lines' own deltas. The
quick-only difference is **unchanged at 14**: the delegated-run corrective's
six new guards all run in **both** `verify.sh quick` and CI, and M008's one new
guard does too, so neither line adds a quick-only entry. `AGENTS.md`'s
published counts were corrected at the source to **76** for `quick` and **62**
for CI. The full record is in `architecture/count_audit_history.md`.

### Count reconciliation — delegated-run corrective, third merge with `main` (2026-10-25)

`origin/main` advanced to `cb4ceed9d` (PR #194, the M007 desktop editor
surface), which carried the desktop parity line's three new guards and its own
new integration test module. Merging it into the delegated-run corrective
branch moved the same three rows again. The measured values on the merged tree
are *Integration test files* **348**, *Guard scripts (all)* **99** and *CI
guard scripts* **65** — each the sum of the two lines' own deltas, re-measured
against its own listed `Source` rather than incremented from either parent. The
quick-only difference is **unchanged at 14**: the delegated-run corrective's
three new guards and all three of the desktop line's new guards run in **both**
`verify.sh quick` and CI, so neither line adds a quick-only entry. `AGENTS.md`'s
published counts were corrected at the source to **79** for `quick` and **65**
for CI. The full record is in `architecture/count_audit_history.md`.

### Count reconciliation — delegated-run corrective, fourth merge with `main` (2026-10-25)

`origin/main` advanced to `682ae762a` (PR #193, the contained-worktree C008
line). It added **one** integration test file and **no** guard, so only the
*Integration test files* row moved: the measured value on the merged tree is
**349**. *Guard scripts (all)* stays **99** and *CI guard scripts* stays **65**,
and `AGENTS.md`'s published counts (**79** for `quick`, **65** for CI) are
therefore unchanged and needed no correction. The quick-only difference is
**unchanged at 14** and was **re-derived**, not carried over.

The merge also **collapsed a duplicated table block** in `main`'s own copy of
this file: `origin/main` carried the three count rows **twice** — once as
341 / 96 / 59 and again as 340 / 96 / 59 — a merge artifact from PR #193. The
merged tree carries **one** block, holding the merged tree's measured values.
That is `main`'s defect, corrected here by the merge; this branch made no
change to `main`'s content.

### Count reconciliation — delegated-run corrective, fifth merge with `main` (2026-10-25)

`origin/main` advanced to `76b046fd3` (PR #196, the Linux trusted Git C009
hooks-path disposition). It added **one** integration test file and **one**
guard, so four rows moved. The measured values on the merged tree are
*Integration test files* **350**, *Guard scripts (all)* **100** and *CI guard
scripts* **66**, and `AGENTS.md`'s published counts were corrected at the
source to **80** for `quick` and **66** for CI. The quick-only difference is
**unchanged at 14** and was **re-derived**, not carried over: all four of the
two lines' new guards run in **both** surfaces. The quick lane's `==>` lines
moved from **133** to **135** (`run_guards` **131**, `run_quick` **4**).

`main`'s own repair came with it: `main`'s thirteenth amendment records that
this file's Verified-counts table carried **two** copies of the three count
rows — one set at 341 and one at 340 — a merge artifact from an earlier
reconciliation, and that it collapsed the duplicate. The merged tree carries
**one** block, holding the merged tree's measured values.

The full record is in `architecture/count_audit_history.md`.


### Count reconciliation — delegated-run corrective, sixth merge with `main` (2026-10-25)

`origin/main` advanced to `69ceec814`, carrying PRs #195, #197 and #198 — the
desktop command-parity line (M007-D, ADR-0052), its closure records, and the
owner-directed corrective registrations. **Three** files conflicted, and **all
three were documentation**: `AGENTS.md`, this file, and
`docs/comment-budget.toml`. Every code file merged cleanly — the daemon's
command catalog, the frontend action set and the TUI's migrated served forms
touch no file this line owns, and the two lines' code sets do not overlap at
all.

Four rows moved. Each was **re-measured** against its own listed `Source` in the
merged tree and **not** incremented from either parent:

- *`codegg-core` modules* is **58** (`crates/codegg-core/src/lib.rs`) — `main`'s
  **56** (which added `command_catalog` and `frontend_action`) plus this line's
  **two** delegated-run modules. Both parents measure **56**; the merged tree
  measures **58**.
- *Integration test files* is **352** (`tests/*.rs` + `tests/*/*.inc`, with the
  `tests/common/` helper tree excluded) — the base **342**, plus this line's
  **eight** and `main`'s **one**, with no overlap.
- *Guard scripts (all)* is **102** (`ls scripts/check_*`) — the base **97**,
  plus this line's **three** and `main`'s **two**, with no overlap.
- *CI guard scripts* is **68** (the distinct `scripts/check_*` names in
  `.github/workflows/ci.yml`) — the base **60**, plus the same **six**.

The *quick* guard count moved **80 → 82**, and `AGENTS.md`'s published counts
moved to **82** for `quick` and **68** for CI. The quick-only difference is
**unchanged at 14** and was **re-derived**, not carried over: every one of the
two lines' new guards runs in **both** surfaces. The quick lane's `==>` lines
moved from **135** to **139** (`run_guards` **135**, `run_quick` **4**).

`main`'s own M007-D section records *`codegg-core` modules* as **56**. That is
`main`'s measurement of `main`'s tree and stands as the record of that
milestone; the row in the table holds the merged tree's measured **58**.

The comment budget was re-banked on the merged tree, after `cargo fmt --all` —
which produced **no** edit, `main`'s merged code being already formatted. The
banked manifest holds **127,700** comment lines, **1,127,120** words and a
**0.1341** ratio, and the bank diff is confined to the files the merge touched,
plus the global block.

Every other code-count row was re-checked against its own listed `Source` and is
unchanged: architecture docs **92**, distinct production tool names **75**, tool
source files **83**, LSP server definitions **39**, `AppEvent` variants **61**,
`CoreRequest` variants **270** (244 struct + 26 unit), `CoreResponse` variants
**178** (177 struct + 1 unit), `CoreEvent` variants **92**, `ProjectionEvent`
variants **47**, authorized operations **264**, identity newtypes **19**, built-in
slash commands **169**, built-in agents **10**, bundled themes **50**, TUI state
modules **33** (34 `.rs` files incl. `mod.rs`), database tables **99 statements /
95 tables**, storage layout version **90**, Git operation variants **54**, Git
risk classes **11**, and workspace member crates **11** (+ root). The **Tool
registrations** row is unchanged too — still **67**, still **53 `register` + 14
`register_boxed`**.

The full record is in `architecture/count_audit_history.md`.


### Count reconciliation — M007-D daemon-owned command catalog (2026-10-25)

Six code-count rows moved when the daemon-owned command catalog milestone
landed (ADR-0052, accepted 2026-10-24). *`codegg-core` modules* is **56**
(`crates/codegg-core/src/lib.rs` — the new `command_catalog` and
`frontend_action` modules). *`CoreRequest` variants* is **270** (244 struct +
26 unit — the new `CommandCatalog` and `CommandExecute` arms, added **beside**
the existing `GitRead` and `SkillCatalogueList` arms, never replacing them).
*`CoreResponse` variants* is **178** (177 struct + 1 unit — the matching
`CommandCatalog` and `CommandExecute` arms). *Authorized operations* is **264**
(`operation_descriptor` — 264 arms carrying 264 distinct operation names,
covering all 270 `CoreRequest` variants; the `representative_requests()` list
holds 254 of them): the catalog read takes `project.read` and the command
execution takes the dedicated `command.execute` capability, which was already
in the enum because the human-shell run family uses it. *Integration test
files* is **341** (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/`
helper tree excluded — the new `tests/core_daemon/command_catalog_m007d.inc`).
*Guard scripts (all)* is **98** (`ls scripts/check_*` — the new
`scripts/check_command_catalog_ownership.py`, which proves the daemon's catalog
and the TUI's command surface are the same set, that only the catalog can
execute a command, and that no frontend holds a second catalog, and the new
`scripts/check_frontend_action_set.py`, which proves the frontend's
dispatchable set is declared, finite, closed and non-mutating). *CI guard
scripts* is **61** (the distinct `scripts/check_*` names in
`.github/workflows/ci.yml` — it gained both guards and both self-tests). The
quick-only difference is unchanged at **14** guards CI does not run: the new
guards run in **both** `verify.sh quick` and CI, so they add no quick-only
entry. The *quick* guard count is **75**. Every other code-count row was
re-checked against its own listed `Source` and is unchanged.


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
- `src/directory_listing.rs` is the only workspace directory walker; the TUI navigator and the daemon handler both delegate to it.
- All process-spawn sites are inventoried in `docs/execution-ownership.toml` and enforced by `check_execution_ownership.py`.

### Typed command pipeline

`command_intent::pipeline::prepare_command` is the canonical entry point for raw shell-originated input. It classifies once, plans with an explicit execution context, and calls `CommandPlan::dispatch_target()` once. The plan owns backend, permission, projector, timeout, and routing-family selection. The daemon/tool boundary remains the authorization authority; active-routing validation is only a preflight gate.

### Shell projection pipeline

Shell output flows through a multi-phase projection pipeline: raw capture → projector selection → compression → redaction → expansion handles → context budget → promotion decisions. Each phase is independently testable. See [human_shell.md](human_shell.md) for the current phase list.

### Workspace-scoped services

Each registered workspace gets its own `WorkspaceServices` bundle (RunStore, path policy, lock table, config). Bundles are lazily activated, lease-tracked, and idle-evicted.

### Scheduler-owned execution

The `JobScheduler` is the single daemon admission authority for submitted work. All heavy operations (tests, managed processes, subagent dispatch) flow through `JobSubmissionService` → `JobScheduler` → executor dispatch.

### Documentation surfaces

The repository carries four documentation surfaces. Each has **one** authority, and the others point at it rather than restating it — a contract copied into a second place is a contract that drifts and then lies.

| Surface | Audience | Authority for | Index |
|---|---|---|---|
| `architecture/*.md` (92 docs) | Implementers | Contracts, invariants, and **all** counts | this document |
| `docs/*.md` | Users | How to use and configure CodeGG | `README.md` |
| `.opencode/skills/*/SKILL.md` (27) | Agents | On-demand module guides | `AGENTS.md` → *Skills index* |
| `plans/**` | Maintainers | Milestone and roadmap status | `plans/registry.md` |

The rules that keep them from diverging:

- A **count** lives in the `## Verified counts` table above and nowhere else. Every other document names the symbol or path it counted instead of repeating the number, so a count cannot drift in a place nobody re-measures.
- A **contract change** updates the owning `architecture/` document and the matching skill guide together. The *Skills index* in `AGENTS.md` gives the pairing.
- A **user-facing claim** (a command, a flag, a config key) lands in `docs/` first, with the `architecture/` document carrying the contract behind it.
- A skill guide **links** to its `architecture/` document rather than copying the contract, and ends with a dated *Source verification* note naming what was checked.
- `docs/validation/` closure records are historical evidence: their line numbers are pinned to old SHAs by design and are never refreshed.

`scripts/check_first_party_skills.py` enforces the mechanical part of this — frontmatter shape, package-name uniqueness, relative-link resolution, and the presence of each skill's row in the `AGENTS.md` *Skills index*. `scripts/check_test_target_references.py` enforces the count table's integration-test row against the tree. Neither is a substitute for reading.

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
│   ├── eggsact/                # Eggsact adapter (in-process)
│   ├── git_publication/        # Private index publication (staging, publish, reconcile)
│   ├── hooks/                  # Lifecycle hooks
│   ├── ide/                    # VS Code/JetBrains detection
│   ├── lsp/                    # LSP thin re-export shim
│   ├── managed_process/        # Managed process internals (Windows ACL lease)
│   ├── mcp/                    # MCP client
│   ├── permission/             # Access control
│   ├── plugin/                 # WASM plugin system
│   ├── preflight/              # Eggsact preflight validation
│   ├── python_script/          # Python scripting
│   ├── research/               # Research pipeline
│   ├── scheduler/              # Admission control, fair queue
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
│   ├── acp.rs                  # Agent Client Protocol adapter
│   ├── background_task_migration.rs
│   ├── command_outcome.rs      # Command outcome classification
│   ├── decision.rs             # Opt-in System One decision backend
│   ├── decision_sdm.rs         # Opt-in pinned SDM local Rank backend
│   ├── directory_listing.rs    # The single daemon-neutral directory walker
│   ├── document_service.rs     # Daemon authority for open editor documents
│   ├── error.rs                # Centralized error taxonomy
│   ├── execution_subject_capture.rs
│   ├── executor_audit_hooks.rs # Executor-owned live audit hook pins
│   ├── git_*.rs                # Git service/mutations, network, recovery, publication
│   ├── goal_continuation.rs    # Host-observed goal continuation evidence
│   ├── goal_verification.rs    # Goal verification pass
│   ├── http_client.rs          # Ordinary HTTP client construction policy
│   ├── install.rs              # Installation-owned sidecar resolution
│   ├── interactive_process*.rs # PTY lifecycle + attach protocol
│   ├── job_*.rs                # Job dispatch, recovery
│   ├── lib.rs                  # Library root (re-exports)
│   ├── live_execution_audit.rs # Live execution audit hooks
│   ├── main.rs                 # Binary entry point
│   ├── managed_process.rs      # Managed process lifecycle
│   ├── policy_surface.rs       # Frontend-neutral approval/sandbox surface
│   ├── protocol_conversions.rs # Protocol conversion helpers
│   ├── run_rerun.rs            # Run rerun linkage
│   ├── scratch.rs              # Daemon-owned scratch resources
│   ├── test_failpoint.rs       # Debug-build failpoints for recovery tests
│   └── work_plan_*.rs          # Plan arbiter, eggplan, evidence, binding, todo sync
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
├── tests/                      # Integration tests (339 files)
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

## Deep-dive index

Every architecture document in this directory, grouped by theme. The module map
above is the *narrative* index — one row per component, grouped by layer. This
section is the *complete* index — every document, with a one-line description
of what it covers. A document that appears in both is listed here once.

### Agent and Execution
- [Agent Loop](agent.md) — execution cycle, compaction, routing, delegated runs
- [Agent Tool Surface](agent-tool-surface.md) — model-facing tool definitions and gating
- [ACP](acp.md) — Agent Client Protocol v1 stdio adapter
- [Command Intent](command_intent.md) — classification, risk, execution capabilities
- [Command Planner](command_planner.md) — backend mapping, permission generation, projector policy
- [Command Routing](command_routing.md) — compatibility facade over the pre-M006 API
- [Exec](exec.md) — non-interactive execution mode
- [Model Profile & Task State](model_profile_task_state.md) — behavioral profiles, todo/task state
- [Blackboard Contract Matrix](blackboard_contract_matrix.md) — the frozen comparison baseline for the multi-agent workstream
- [Research](research.md) — sources → evidence → claims → verification
- [Work Orders](work_orders.md) — durable work-order lifecycle
- [Work Plan](work_plan.md) — plan item tracking

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
- [Search Backend](search_backend.md) — eggsearch dispatch and disabled behavior

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
- [Workspace](workspace.md) — registry, execution context, directory listing
- [Workspace Services](workspace_services.md) — per-workspace bundles, migration
- [Checkout Writer Permit](writer_permit.md) — durable generation-fenced writer token, quarantine, bounded waiter
- [Build Admission](build_admission.md) — counted per-workspace build quota, configuration-owned limit, coarse group status
- [Session Launch](session_launch.md) — durable idempotent session launch into an isolated checkout
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
- [Workstream Claims](workstream_claims.md) — durable group-scoped claims and prerequisite edges
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
- [Windows MXC Feasibility](windows-mxc-feasibility.md) — macOS-equivalent containment assessment for Windows

### Measurement
- [Count Audit History](count_audit_history.md) — the method and reconciliation history behind the Verified counts table

### Historical Handoffs
- [Git Phase F Handoff](git_phase_f_handoff.md) — snapshot-time closure record
- [Git Polish/Verification Handoff](git_polish_verification_handoff.md) — snapshot-time post-closure record

## Comment policy

A comment earns its place by carrying something the code cannot: a rule the
code cannot enforce, a boundary it will not discover on its own, a failure
mode that looks impossible until you know the history, or a pointer to the
document that owns the reasoning.

What a comment does **not** do is restate the code beside it. The
`architecture/` documents are the canonical home of the rationale; a code
comment that repeats them is a third copy, and the third copy drifts. The
policy is therefore:

- **A rule stays.** "The waiter unregisters on every exit path, so age-based
  eviction is only a leak guard" is a rule the code cannot enforce by
  itself. It stays at the code.
- **A restatement goes.** "This module provides the pair table and the
  depth-counting scanner" says what the next twenty lines say. It goes.
- **A duplicate becomes a pointer.** If the `architecture/` document already
  holds the reasoning, the comment names it instead of repeating it.
- **A history becomes its current-state rule.** "This used to keep only the
  file name" is replaced by "this resolves the full relative path"; the
  current rule is what a reader needs.
- **A known limitation is disclosed.** A comment that names an imprecision,
  an approximation or a residual stays, because a reader who cannot see the
  limitation cannot reason about the code.

`scripts/check_comment_budget.py` enforces the parts of this that can be
checked mechanically, against the budget in `docs/comment-budget.toml`:

| Rule | Fails on |
|---|---|
| **R0** unreadable file | the guard could not read a file — a partial scan is never reported as a pass |
| **R1** stale path reference | a comment names a repository path that does not exist |
| **R2** unresolvable doc pointer | a comment names an `architecture/` document, or a section inside one, that does not exist |
| **R3** oversized comment block | a comment block is longer than `max_block_lines` |
| **R4** comment budget | a file's comment mass exceeds its recorded ceiling, or the corpus exceeds the global ceiling |
| **R5** orphaned contract | a long block restates a contract without naming the document that owns it |
| **R6** lost guard landmark | a comment marker that a guard greps for is gone from the tree |

**The budget is a ratchet, not a snapshot.** Every recorded value is a
ceiling, derived from a measurement of the tree
(`python3 scripts/check_comment_budget.py --bank` recomputes them). The
corpus may not grow past the recorded total, and a reduction lowers the
ceiling. The only way to spend the lines a reduction frees is to edit
`docs/comment-budget.toml`, which shows up in the same diff as the comment
that took them — so the corpus cannot re-inflate silently.

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

Full `check_*` inventory lives in `scripts/` (99 guards) and additionally covers audit coverage/invariants, authorization matrix, command catalog ownership, discovery invariants, frontend action set, identity path usage, project-catalog invariants, provider-connections coverage/tombstones, sandbox contract, tool-broker boundary, TUI project authority, TUI editor text authority, TUI editor key authority, TUI composer key ownership, and websocket bounds. See `AGENTS.md` for the complete change-triggered guard list.

## Source verification

Every row of the `## Verified counts` table was re-measured on **2026-10-23** against the tree at `12999178a`, each against its own listed `Source`, using `scripts/measure_overview_counts.py`. All rows agree with the published values.

Three coverage properties were also verified on the same tree, and each is now enforced by the structure of this document rather than by prose:

- **Every architecture document is linked from this index.** All 92 documents under `architecture/` are reachable from the module map or the deep-dive index below.
- **Every `src/` module directory has an owning deep dive.** Each of the 34 module directories under `src/` maps to a document named in the module map above.
- **The tool catalog table is exact.** The names in *Tools and capabilities at a glance* were cross-checked against the measured production tool names — the two sets are identical, with no name in one and absent from the other.

### What the deep-dive pass changed

Every document in this directory was reviewed against source on **2026-10-23** and carries a dated *Source verification* note recording what was checked. That pass corrected several defects that the count table alone could not surface:

- **The tool-name count was scoped too narrowly.** The published "61 distinct production tool names" measured only `src/tool/` + `src/context/`, which excluded `python_script` — a production tool registered unconditionally by `with_options()` that lives in `src/python_script/`. The row now measures the whole `src/` tree, and a later pass added the 13 runtime-named `eggsact` validators it was still missing: the row publishes **75** (62 literal + 13 eggsact). The method and both corrections are recorded in [count_audit_history.md](count_audit_history.md).
- **`authorization.md` published stale operation counts** ("228 native operations", "226 operations") against a measured **263 `CoreRequest` variants** and **257 distinct operation names**.
- **`workstream_claims.md` published `STORAGE_LAYOUT_VERSION` as 79** — the value of the *migration* that adds the claims schema, not the current layout (**87**).
- **`process-tool-execution-ownership.md` published a stale manifest count** (58 entries) against the **60** that `docs/execution-ownership.toml` actually declares.
- **Several documents pointed at integration tests that no longer exist as targets.** The root package's tests are families, so `cargo test --test <member>` selects zero tests and still exits `0`. The affected documents now use the `--test <family> -- <member>` form.

Two **code** issues were found and are recorded in the documents that own them, rather than silently fixed here:

- `crates/codegg-client/Cargo.toml` declared `codegg-document` as a bare path dependency, bypassing the workspace's `=0.1.0` pin — the only member that did so. **Fixed here**: it now inherits from the workspace.
- `ExecutorKind::BashDispatch` in `src/scheduler/executor.rs` is **unreachable**: `executor_variant` has no `BashDispatch` arm and `JobPayload` has no such variant, so the `(_, PayloadVariant::BashDispatch)` arm can never fire. The variant survives only because `PayloadVariant` carries `#[allow(dead_code)]`, which is why clippy does not report it. Recorded in [scheduler.md](scheduler.md) §"ExecutorRegistry".

The measurement method behind each count, and the reconciliation history that pinned it, is recorded in [count_audit_history.md](count_audit_history.md).

Two file-count rows moved again on **2026-10-22**, when the provider-reasoning-control line was reconciled with `main` a second time: `main` had advanced past `8cee65934` to `47f5572b1` (the agent-workstream blackboard's M008) while the reconciliation was still in flight. The two rows are the **union of both sides' contributions**, each measured in the merged tree rather than carried over from either side: integration test files are **300** (`ls tests/*.rs` — `main` contributes the two new `tests/blackboard_trajectory_m008.rs` and `tests/blackboard_economics_m008.rs`, and **this line contributes none**: its new suite, `crates/codegg-core/tests/reasoning_overlay_narrowing.rs`, lives under that crate's own `tests/` directory, which the root `tests/*.rs` Source does not count — the same distinction the M008 pass drew for `tests/common/blackboard.rs`); CI guard scripts are **71** (`ls scripts/check_*` — `main`'s 67 plus this line's four). The architecture-doc row is **91** and is `main`'s alone: this line adds no architecture document. Every code-count row is unchanged and was re-checked against its own listed `Source` rather than assumed.

Every other row was re-checked against its listed `Source` and was unchanged **at that date**;
the later count passes are recorded below and in
[count_audit_history.md](count_audit_history.md).

Corrected 2026-10-23 (second count pass): the *Distinct production tool names*
row understated the production surface by 13. The measure scanned
`impl Tool for … name()` for a string literal, which cannot see the `eggsact`
deterministic family — `impl Tool for EggsactTool` returns `self.codegg_name`,
so its names live at the `EggsactTool::new()` call sites in
`src/tool/deterministic.rs`. Those 13 are real callable production tools
(`codegg doctor deterministic-tools` reports them as 8 always-visible and 5
deferred). The row now reads **75** (62 literal + 13 eggsact runtime-named),
the measure derives the eggsact names from their constructor call sites, and
both this catalog table and `docs/tools.md`'s family table now name all 75 —
the two sets are identical. Two runtime-named families were checked and add
nothing: `DisabledTool` carries the name of the tool it replaces and
`UnrootedTool` forwards its inner tool's name. The method and the reasoning
are in [count_audit_history.md](count_audit_history.md).

Added 2026-10-23 (documentation-surfaces pass): the **Documentation surfaces** subsection under *Key architectural patterns* records the four-surface contract — `architecture/` (92 docs), `docs/`, `.opencode/skills/` (27 guides), and `plans/` — with one authority each. Re-verified on the same tree: all **92** documents under `architecture/` remain linked from the module map or the deep-dive index (the deep-dive index alone links 91, with this document as the 92nd), the `src/` module-directory count is still **34**, and `scripts/check_*` still holds **76** guards. `scripts/measure_overview_counts.py` was re-run and every row of the `## Verified counts` table still agrees — no code changed in this pass, so no count moved.

**Amended 2026-10-22 (same row, no count moved).** The contained operation was **measured** to be two steps rather than one: `git worktree add <path> <existing-branch>` locks and rewrites `refs/heads/<branch>` even when the value is unchanged, so it cannot run inside a grant that excludes `refs`. ADR-0048 was **amended** accordingly — the operation is `git worktree add --detach <path> <commit>` followed by `git symbolic-ref HEAD refs/heads/<branch>`, both inside the same containment. The writable set did not widen, no code-count row moved, and no new module, variant, tool or guard was added.

One row moved on **2026-10-22** when the CI/test-throughput recovery line reconciled with `main` and absorbed `main`'s integration tests into the recovery families: CI guard scripts are **75** (`ls scripts/check_*` — the count moved by one, the new `scripts/check_test_self_reexec_paths.py`). The **integration test files** row is unchanged at **318** (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded): the absorption moved files between families without adding or removing one. The new guard exists because the absorption changed the path cargo knows a moved test by — a family member's path is `<member>::<test_fn>`, a standalone target's is `<test_fn>` — and a self-re-exec test that keeps the old bare path selects **zero** tests while still exiting **0**, so the child does no work and the parent reads that empty success as evidence. The hosted Windows and Linux M004 lanes showed both consequences: a fixture child that published no PID, and an interrupted-publication recovery scenario that was never exercised. Every code-count row was re-checked against its own listed `Source` and is unchanged.

One row moved again on **2026-10-22** when the same line reconciled with `main` a **sixth** time, at `7d3c6ed09` (PR #166, the Linux trusted-Git C008 contained worktree): integration test files are **319** (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded — the count moved by one, the new `tests/composed_two_agent_race_m011.rs`). That file **stays top-level**: it drives two live agents, and the agent-loop harness suites drain the process-global event bus and assert that nothing was left pending on it, so sharing a binary with them turns that assertion into a cross-test race. The root package's test-target count is therefore **25** (21 families + 4) and the workspace's **53**.

The same merge also produced a tree that **did not compile**, and no merge tool reported it: `main`'s edit to the agent-loop harness included a shared helper with `#[path = "common/scripted_provider.rs"]`, and a `#[path]` attribute is relative to **the file that carries it** — the harness had moved into `tests/agent_loop/`, so the path resolved to a directory that does not exist. The two sides touched different paths, git paired them by rename, and the break was silent; `cargo check --workspace --all-targets` on the merged tree is what found it. The merged family reaches the shared tree through the binding its root already declares, so the private include was both broken and redundant.

CI guard scripts are now **76** (`ls scripts/check_*`): `main`'s advance added
no guard, and this line's two guards are the ones it did — `check_test_self_reexec_paths.py` and `check_test_common_binding.py`. Every code-count row was re-checked against its own listed `Source` and is unchanged.

One row moved on **2026-10-23** when the Linux sandbox admission follow-up corrective opened C003 (typed Git live-read consistency and hostile-metadata review): integration test files are **321** (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded — the count moved by two, C003's new `tests/command_git/git_read_consistency.inc` and `tests/command_git/git_hostile_metadata.inc`). Both new members live inside the consolidated `command_git` family, so the root package's **test-target** count did not move: it is still **25** (21 families + 4) and the workspace's **53**. Only the integration-test *file* count moved, and only because two files were added.

No row moved again on **2026-10-22** when the same line reconciled with `main` a **seventh** time, at `726f3c645` (PR #167, the Linux trusted-Git C008 contained worktree). That advance is **documentation only** — four records (`architecture/blackboard_contract_matrix.md`, `plans/closure/agent-workstream-blackboard/011-status.md`, `plans/registry.md`, `plans/subsystems/agent-workstream-blackboard-roadmap.md`) and no code — so the counts stand unchanged: integration test files **319**, CI guard scripts **76**, root package test targets **25** (21 families + 4), workspace test targets **53**. Both sides edited `plans/registry.md`, so the merge was checked rather than assumed: the row the branch owns is byte-identical to its pre-merge copy, the row `main` owns is byte-identical to `origin/main`'s, and the file both sides edited differs from **both** parents.

Two rows moved again on **2026-10-23**: the root package's test-target count is now **23** (19 families + 4) and the workspace's **51**. The integration test file count did **not** move — it is **321** — because the change was a **move**, not a deletion: `interactive_process_sessions` and `interactive_terminal_tui` were absorbed into the `interactive_process_attach_resume` family, and a move preserves the module count by construction. The two absorbed families sat in the `ci` nextest profile's whole-binary exclusion list, so on the hosted 4-vCPU runner they ran one at a time, alone; three exclusive executables cost three serial slots for 40 test functions, and one costs one. It also removes two link units, because a family's cost is dominated by the code it links rather than by the number of test functions in it. The `ci` profile's exclusion list now names four binaries.

This file supersedes the earlier overview's stale counts (77 docs / 53 tool registrations / 53 `AppEvent` variants / 142 slash commands / 71 tables / layout v56 / 21 guards / 189 tests) and adds previously undocumented surfaces: `src/tool_advisor/`, `crates/codegg-document/`, `src/command/`, `src/tui/document_session.rs`, `src/tui/interactive_terminal.rs`, `apps/desktop/`, and the excluded `eggwork-test-node` fixture workspace.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime ownership migration and M006 closure"). The `src/` tree above gained two root modules that were missing: `src/decision.rs` (the opt-in System One-compatible `DecisionEngine` backend) and `src/decision_sdm.rs` (the opt-in pinned SDM local Rank backend, gated behind `--features decision-runtime-sdm`). The same commit deleted thirteen `src/tool_advisor/` modules (`contextual.rs`, `late_interaction.rs`, `operating_point.rs`, `requalify.rs`, `retrieval_architecture.rs`, `retrieval_projection.rs`, `retrieval_signal.rs`, `retrieval_signal_v2.rs`, `sequence_encoder.rs`, `sequence_qualification.rs`, `sequence_ranking.rs`, `sequence_retrieval.rs`, `training.rs`), leaving `causal_active.rs`, `causal_frontier.rs`, `causal_observe.rs`, `context_v2.rs`, `decision_adapter.rs`, `mod.rs`, `order_invariance.rs`, `retrieval_relevance.rs`, and `training_data.rs`. See [tool-advisor.md](tool-advisor.md) for the decision-subsystem contract and the retired-work history.

**Six rows moved again on 2026-10-23**, when the provider-reasoning-control line was
reconciled with `main` a **sixth** time. `main` had landed a documentation-surfaces
pass (PR #175) that changed **two rows' `Source`**: *Distinct production tool names*
now scans the whole `src/` tree rather than `src/tool/` + `src/context/`, and *CI guard
scripts* is now counted from `.github/workflows/ci.yml` rather than from
`scripts/verify.sh quick`; a new row, *Guard scripts (all)*, was added. Every row was
**re-measured in the merged tree against its own listed `Source`**, using `main`'s own
measurement method as recorded in
[count_audit_history.md](count_audit_history.md): architecture docs **92**, distinct
production tool names **75** (67 production `impl Tool for` blocks, less the 8
test-only mocks, plus the 13 eggsact validator names — the method reproduces `main`'s
published 75 exactly), tool source files **84**, guard scripts (all) **80**
(`ls scripts/check_*`), CI guard scripts **40** (the `ci.yml` list, of which 14 are not
in `quick`), integration test files **321**, storage layout version **88**, and
`CoreRequest` variants **266**. **One row was stale on `main` and is corrected rather
than incremented**: *tool source files* read 83, but the tree carries 84 `.rs` files
under `src/tool/` — `main`'s count excludes `src/tool/bash/tests/git_fixture.rs`, and its
own listed `Source` (`src/tool/**.rs`, incl. `bash/`) does not exclude it.
One row moved on **2026-10-23** when the TUI text-rendering line added its first two milestones: the integration test files row is **327** (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded — the count moved by six, the new `tests/tui_render/tui_main_buffer_row_accounting.inc`, `tests/tui_render/tui_main_buffer_word_wrap.inc`, `tests/tui_render/tui_composer_wrap.inc`, `tests/tui_render/tui_formatted_json_detail_view.inc` and `tests/tui_render/tui_main_buffer_collapsed_layout.inc` and `tests/tui_render/tui_main_buffer_row_precise_disclosure.inc`). All six new members live inside the existing `tui_render` family, so the root package's **test-target** count did **not** move: it is still **23** (19 families + 4) and the workspace's **51**. Only the integration-test *file* count moved, and only because six files were added. Every other code-count row was re-checked against its own listed `Source` and is unchanged.

One row moved again on **2026-10-23**, when this line was reconciled with `main` a
**second** time, at `e3215a06f` (PR #172, the provider-reasoning-control line). That
merge landed **four** new guards — `check_model_profile_capability_mirror.py`,
`check_reasoning_intent_fidelity.py`, `check_reasoning_vocabulary_open.py` and
`check_tui_reasoning_surface.py` — and four new `ci.yml` guard steps, so the *Guard
scripts (all)* row is **80** and the *CI guard scripts* row is **40**. **Neither row
moved because of this line**; both are recorded here because this line's own paragraph
above was written against the earlier `main` at `ff82c68da`, where they read 76 and 36,
and that paragraph's closing sentence — "every other code-count row ... is unchanged" —
was true of the tree it described and is **not** true of the merged tree. The
*Integration test files* row is **327**: PR #172's reconciliation read **321** in its
merged tree, and this line's six new members still sit on top of that. Every other
code-count row was re-checked against its own listed `Source` in the merged tree and is
unchanged.

*Guard scripts (all)* is **81** (the new `scripts/check_destructive_risk_signal_coverage.py`),
and *CI guard scripts* is **41** (that one guard's two `ci.yml` steps — the guard and its

Re-verified 2026-10-25 against the M007-D branch with
`scripts/measure_overview_counts.py`: the `## Verified counts` rows read
**270** `CoreRequest` variants, **264** native operations, **341** integration
test files, **98** guard scripts, **61** CI guard scripts and **92**
architecture documents. The count-reconciliation prose under this section
was corrected to the same figures — it had understated each guard count by
one.
