# `codegg-core` Extraction Readiness Note

> Prepared as Phase 4 of `plans/crate_modularization_next.md`.
> This document classifies root modules, documents cycle risks, and recommends
> next steps for a future `codegg-core` extraction.

## Module Classification

### Group A: Likely Core-First Modules

Candidates for the first `codegg-core` slice. These are runtime/session/state
modules that should eventually be usable by daemon, TUI, CLI, and tests without
depending on terminal rendering.

| Module | Purpose | File Count |
|--------|---------|-----------|
| `src/core/**` | Core facade, transport adapters, daemon | 6 files |
| `src/session/**` | Session storage, schema, models, import | 6 files |
| `src/storage/**` | SQLite database init, connection pooling | 2 files |
| `src/bus/**` | Event bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | 2 files |
| `src/error.rs` | Centralized AppError, ToolError, etc. | 1 file |
| `src/exec/**` | Non-interactive exec mode for CI/CD | 0 files (empty) |
| `src/memory/**` | Persistent memory system, patterns | 2 files |
| `src/goal/**` | Long-horizon goal runtime, budget enforcement | 7 files |
| `src/task_state/**` | Task state management (TodoItem state machine) | 1 file |
| `src/snapshot/**` | File state capture and restore | 2 files |
| `src/resilience/**` | Circuit breaker, retry mechanisms | 1 file |
| `src/worktree/**` | Git worktree support | 1 file |
| `src/util/**` | Utility functions (clipboard, fuzzy search, pricing) | varies |
| `src/protocol_conversions.rs` | Protocol DTO bridge functions | 1 file |

**Not listed in plan but should be Group A:**
- `src/model_profile/**` — Model profile resolution, types, policy. Depends only on
  extracted crates (`codegg_config`, `codegg_providers`) and internal Group A types.

### Group B: Core but High-Coupling

Move after Group A. These are core-domain modules coupled to providers, tools,
plugins, permissions, MCP, or LSP.

| Module | Purpose |
|--------|---------|
| `src/agent/**` | Main agent loop, prompt templates, subagent pool, team coordination |
| `src/permission/**` | Access control, DoomLoop detection, mode system |
| `src/mcp/**` | Model Context Protocol client (local, remote, auth) |
| `src/hooks/**` | Hooks system for agent loop lifecycle |
| `src/ide/**` | IDE integration (VS Code IPC, JetBrains) |
| `src/lsp/**` | LSP wrapper around egglsp |
| `src/shell_session/**` | Shell session metadata management |
| `src/skills/**` | Skill system for specialized capabilities |

### Group C: Keep Root or Later Crates

These have heavy UI/server/tool/plugin/auth dependencies and require separate
design or more careful cycle-breaking.

| Module | Purpose |
|--------|---------|
| `src/tui/**` | Terminal UI (ratatui) |
| `src/server/**` | HTTP/WebSocket server (axum) |
| `src/client/**` | Remote TUI client |
| `src/tool/**` | Built-in tools |
| `src/search/**` | Web search providers |
| `src/search_backend/**` | Search backend layer |
| `src/research/**` | Research tool |
| `src/security/**` | SSRF protection, Landlock sandboxing |
| `src/theme/**` | Theme system |
| `src/tts/**` | Text-to-speech |
| `src/plugin/**` | WASM plugin system |
| `src/upgrade/**` | Self-upgrade functionality |
| `src/auth/**` | Auth CLI (`codegg auth`) |
| `src/crypto/**` | AES-256-GCM encryption |

### Group D: Already Extracted or Wrapper-Only

| Crate | Purpose |
|-------|---------|
| `crates/codegg-config` | Config schema, paths, loading, watcher |
| `crates/codegg-protocol` | Protocol DTOs, CoreRequest, CoreResponse, TuiMessage |
| `crates/codegg-providers` | Provider trait, registry, auth, streaming, circuit breaker |
| `crates/eggcontext` | Context extraction |
| `crates/egggit` | Read-only git facts |
| `crates/egglsp` | LSP server definitions |
| `crates/eggsentry` | Security sandboxing |

## TUI Dependency Check

**Result: CLEAN**

No Group A module imports `ratatui`, `crossterm`, `ratatui_textarea`, or any
`tui::` namespace. The search across all 13 Group A module directories returned
zero matches.

This means Group A modules can move to `codegg-core` without pulling in any
terminal UI dependencies.

## Cycle-Risk Findings

These are the dependencies from Group A modules into Group B/C modules that
must be resolved before or during extraction.

### High Risk (must break before extraction)

| Source | Target | Severity | Details |
|--------|--------|----------|---------|
| `src/core/daemon.rs` | `crate::agent::*` | **High** | `SubAgentPool`, `BackgroundScheduler`, `AgentLoop`, `prompt::load_agent_prompt` — 12 references |
| `src/core/daemon.rs` | `crate::permission::*` | **High** | `PermissionChecker`, `PermissionChoice` — 5 references |
| `src/core/daemon.rs` | `crate::tool::*` | **High** | `ToolBackendConfig`, `ToolRegistry`, `TaskTool` — 5 references |
| `src/core/mod.rs` | `crate::agent::*` | **High** | `SubAgentPool`, `BackgroundScheduler` — 2 references |
| `src/error.rs` | `crate::plugin::*` | **High** | `LoadError`, `InstallError` — 2 `From` impls |
| `src/error.rs` | `crate::permission` | **Medium** | `PermissionError` — enum variant only |
| `src/error.rs` | `crate::mcp` | **Medium** | `McpError` — enum variant only |
| `src/error.rs` | `crate::lsp` | **Medium** | `LspError` — enum variant only |
| `src/bus/mod.rs` | `crate::permission` | **Medium** | `PermissionChoice` import — 1 reference |

### Low Risk (may resolve with minor refactoring)

| Source | Target | Severity | Details |
|--------|--------|----------|---------|
| `src/goal/tool.rs` | `crate::tool::Tool` | **Low** | `Tool` trait impl for goal tools — move `goal/tool.rs` to Group B or extract trait |
| `src/protocol_conversions.rs` | `crate::agent::Agent` | **Low** | 2 conversion functions (`agent_to_dto`, `dto_to_agent`) |
| `src/task_state/mod.rs` | `crate::model_profile::types` | **Low** | `CompletedTodoExposure`, `TaskStatePolicy`, `TodoMode` |

### No Cycles (clean movers)

These Group A modules have zero imports from Group B/C modules:

| Module | Dependencies |
|--------|-------------|
| `src/session/**` | `crate::error`, `crate::config` (extracted) |
| `src/storage/**` | `crate::error`, `crate::session` |
| `src/memory/**` | `crate::session`, `crate::memory` (internal) |
| `src/goal/**` (excl. `tool.rs`) | `crate::error`, `crate::bus`, `crate::session` |
| `src/task_state/**` | `crate::model_profile`, `crate::session`, `crate::bus` |
| `src/snapshot/**` | `crate::error` only |
| `src/resilience/**` | No `crate::` imports |
| `src/worktree/**` | `crate::error` only |
| `src/exec/**` | No `crate::` imports (empty module) |
| `src/model_profile/**` | `crate::config` (extracted), `crate::provider` (extracted) |

## Extraction Strategy

### Phase A1: Zero-Cycle Modules (lowest risk)

Move these first — they have no or minimal Group B/C dependencies:

1. `src/error.rs` — **with care**: extract `AppError` but keep
   `PluginError`/`McpError`/`LspError` variants as feature-gated or boxed
2. `src/resilience/**` — standalone
3. `src/snapshot/**` — depends only on `error`
4. `src/worktree/**` — depends only on `error`
5. `src/session/**` — depends on `error` and `config` (extracted)
6. `src/storage/**` — depends on `error` and `session`
7. `src/bus/**` — break `permission::PermissionChoice` import (use enum copy
   or extract `PermissionChoice` into core)
8. `src/memory/**` — depends on `session`
9. `src/goal/**` — break `tool::Tool` import by moving `goal/tool.rs` to
   agent or tool module
10. `src/task_state/**` — resolve `model_profile` dependency
11. `src/model_profile/**` — already clean (only extracted-crate deps)
12. `src/exec/**` — empty, move trivially

### Phase A2: Protocol Conversions

`src/protocol_conversions.rs` bridges Group A types to `codegg_protocol` DTOs.
Move it into `codegg-core` after the Group A modules are extracted. The
`agent_to_dto`/`dto_to_agent` functions require `crate::agent::Agent` to be
accessible — either move the `Agent` type to core or keep these conversions
in root.

### Phase A3: Core Facade (highest risk in Group A)

`src/core/**` is the hardest Group A module due to its 17+ references into
Group B (`agent`, `permission`, `tool`). Options:

1. **Extract `core` last in Group A** — after Group B modules are decoupled
2. **Keep `core/daemon.rs` in root** — only extract `core/mod.rs`, `core/transport`,
   `core/client_registry`, `core/notification`, `core/session_runtime`
3. **Define trait boundaries** — `CoreDaemon` depends on `AgentLoop` via trait,
   not concrete type

## Recommended Next Steps

1. **Break `bus → permission` cycle**: Copy `PermissionChoice` enum into `bus`
   module or extract into a shared `codegg-core` types module.

2. **Break `goal → tool` cycle**: Move `goal/tool.rs` into `src/tool/goal.rs`
   (or keep goal tools registered by the tool module, not the goal module).

3. **Break `core → agent` cycle**: Define `AgentExecutor` trait in core that
   `AgentLoop` implements. `CoreDaemon` depends on trait, not concrete type.

4. **Break `core → permission` cycle**: `PermissionChecker` and `PermissionChoice`
   are needed by `CoreDaemon` for request routing. Extract permission types into
   core or a shared types module.

5. **Break `core → tool` cycle**: `ToolBackendConfig` and `ToolRegistry` are
   constructed in `daemon.rs`. Either move construction to root or define
   tool-registry-as-trait.

6. **Handle `error.rs` plugin/mcp/lsp variants**: These error types reference
   Group C modules. Options:
   - Feature-gate the variants (`#[cfg(feature = "plugin")]`)
   - Box the error types to break compile-time dependency
   - Move error types for plugin/mcp/lsp into their respective modules

7. **Verify `task_state → model_profile` path**: Ensure `model_profile` is
   classified as Group A (it should be, given its dependencies are only
   extracted crates).

8. **Run validation** after each cycle break:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

## Summary

| Category | Count | Status |
|----------|-------|--------|
| Group A modules | 14 (+1 unlisted) | 9 have zero Group B/C cycles |
| Group B modules | 8 | Move after Group A |
| Group C modules | 14 | Keep root or later crates |
| Group D crates | 7 | Already extracted |
| High-risk cycles | 6 | Must break before extraction |
| Low-risk cycles | 3 | Minor refactoring needed |
| TUI dependencies | 0 | Clean — no terminal UI coupling |
