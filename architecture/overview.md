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

Every count below was re-measured against the current tree on **2026-10-23**, each against its own listed `Source`, in the tree at `12999178a`. No number was carried over from another tree.

Re-measure them at any time with:

```bash
python3 scripts/measure_overview_counts.py
```

The method behind each row — including the two that are non-obvious (enum-variant counting and production-tool-name detection) — is recorded in [count_audit_history.md](count_audit_history.md).

| Item | Count | Source |
|------|-------|--------|
| Architecture docs | 92 | `architecture/*.md` |
| Tool registrations in `with_options()` | 67 (54 `register` + 13 `register_boxed`) | `ToolRegistry::with_options()` (`src/tool/mod.rs`) — the registrations are the `registry.register(..)` / `registry.register_boxed(..)` calls in that constructor's body, not the constructor's own call sites |
| Distinct production tool names | 75 | `impl Tool for …` `name()` across `src/` (the whole tree — a production `Tool` impl may live with the subsystem that owns the tool's behaviour, e.g. `src/python_script/tool.rs`), **plus** the 13 eggsact validator names at their `EggsactTool::new()` call sites in `src/tool/deterministic.rs` — that family's `impl Tool for EggsactTool` returns `self.codegg_name`, so a literal-name scan cannot see it. 67 production `impl Tool for` blocks, of which 5 resolve their name at runtime; excludes the 8 test-only mock impls under `#[cfg(test)]`: 3 in `program_manifest.rs`, plus `catalog.rs`, `tool_search.rs`, `disabled.rs`, `broker.rs` and `src/scheduler/tool_program_executor.rs`. The runtime-named families that only *substitute* for an already-counted tool add no new name: `DisabledTool` carries the name of the tool it replaces (`lsp`, `lsp_read`, `security`) and `UnrootedTool` forwards its inner tool's name |
| Tool source files | 83 | `src/tool/**.rs` (incl. `bash/`) |
| LSP server definitions | 39 | `crates/egglsp/src/server.rs::server_definitions()` |
| `codegg-core` modules | 54 | `crates/codegg-core/src/lib.rs` (`pub mod`) |
| `AppEvent` variants | 61 | `crates/codegg-core/src/bus/events.rs` |
| `CoreRequest` variants | 263 (239 struct + 24 unit) | `crates/codegg-protocol/src/core.rs` |
| `CoreResponse` variants | 172 (171 struct + 1 unit) | `crates/codegg-protocol/src/core.rs` |
| `CoreEvent` variants | 92 | `crates/codegg-protocol/src/core.rs` |
| `ProjectionEvent` variants | 47 | `crates/codegg-protocol/src/projection/event.rs` |
| Authorized operations | 257 | `operation_descriptor` (`crates/codegg-core/src/authorization/policy.rs`) — 257 arms carrying 257 distinct operation names, covering all 263 `CoreRequest` variants; the `representative_requests()` list holds 247 of them |
| Identity newtypes | 19 | `typed_identity!` (`crates/codegg-core/src/identity.rs`) |
| Built-in slash commands | 168 | asserted by `built_in_command_count_matches_release_docs` |
| Built-in agents | 10 | `assets/agents/*.toml` |
| Bundled themes | 50 | `assets/themes/**/*.toml` |
| TUI state modules | 33 (34 `.rs` files incl. `mod.rs`) | `src/tui/app/state/` |
| Database tables (`CREATE TABLE`) | 95 statements / 91 tables | `crates/codegg-core/src/session/schema.rs` (92 distinct `CREATE TABLE` names, less the transient `origin_attribution_new` rebuild name; `migration_version`, `origin_attribution` and that rebuild each appear twice) |
| Storage layout version | 87 | `crates/codegg-core/src/storage/mod.rs::STORAGE_LAYOUT_VERSION` |
| Git operation variants | 54 | `crates/codegg-git/src/operation.rs` |
| Git risk classes | 11 | `crates/codegg-git/src/risk.rs` |
| Workspace member crates | 11 (+ root) | root `Cargo.toml` |
| Integration test files | 321 | `tests/*.rs` + `tests/*/*.inc` — the root integration test *modules*, with the `tests/common/` helper tree excluded |
| Guard scripts (all) | 76 | `scripts/check_*` — every guard the repository ships |
| CI guard scripts | 36 | `.github/workflows/ci.yml` — a strict subset of the above; quick runs 14 guards CI does not |

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
├── tests/                      # Integration tests (321 files)
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

Full `check_*` inventory lives in `scripts/` (76 guards) and additionally covers audit coverage/invariants, authorization matrix, discovery invariants, identity path usage, project-catalog invariants, provider-connections coverage/tombstones, sandbox contract, tool-broker boundary, TUI project authority, TUI editor text authority, TUI editor key authority, TUI composer key ownership, and websocket bounds. See `AGENTS.md` for the complete change-triggered guard list.

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
