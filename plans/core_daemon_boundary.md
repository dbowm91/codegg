# Core Daemon Boundary Handoff Plan

## Purpose

The first modularization arc has reached a clean stopping point: `codegg-core` owns low-coupling runtime/session/state modules, root re-exports those modules for compatibility, and `scripts/check-core-boundary.sh` enforces that `codegg-core` does not import high-coupling root domains or UI/server/plugin crates.

The next meaningful pass is not another broad crate move. It is a boundary pass around root `src/core/daemon.rs` and related transport/runtime code. The goal is to reduce the remaining concrete coupling from daemon/core transport code into `agent`, `tool`, and `permission` so a future pass can either:

1. move more of `src/core/**` into `codegg-core`, or
2. keep daemon orchestration root-side but make it cleaner and easier to test.

This plan should introduce narrow seams and DTO-style inputs/outputs. It should not move `src/core/daemon.rs` into `codegg-core` yet.

## Current State Summary

`codegg-core` owns:

```text
bus
error
goal
memory
model_profile
protocol_conversions
resilience
session
snapshot
storage
task_state
worktree
```

Root still owns:

```text
src/core/**          # daemon/transport facade
src/agent/**         # AgentLoop, SubAgentPool, scheduler, prompts
src/tool/**          # ToolRegistry, Tool trait, built-in tools
src/permission/**    # permission checking/modes
src/tui/**
src/server/**
src/mcp/**
src/plugin/**
```

Recent seam work added:

```text
src/tool/factory.rs          # build_session_tool_registry()
src/agent/runtime_factory.rs # build_agent_loop(), agent/permission construction seam
```

The important remaining coupling is that `src/core/daemon.rs` and/or `src/core/mod.rs` still know too much about concrete agent runtime types, task/subagent pool types, and tool-registry construction. This pass should reduce that knowledge without changing runtime behavior.

## Non-Goals

Do not move `src/core/daemon.rs` into `codegg-core`.

Do not move `src/agent`, `src/tool`, or `src/permission`.

Do not move TUI, server, MCP, plugin, search, research, auth, crypto, theme, TTS, or upgrade.

Do not rewrite the agent loop.

Do not redesign the tool system.

Do not change model-facing tool names or schemas.

Do not change permission semantics.

Do not change core protocol request/response JSON shape unless required for a bug fix.

Do not weaken the `codegg-core` boundary check.

## Phase 0: Baseline and Coupling Inventory

Run baseline validation:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

If any command is already failing, record the failure in the implementation notes before changing code.

Create a before/after inventory of daemon coupling:

```bash
rg -n "crate::agent|crate::tool|crate::permission|SubAgentPool|BackgroundScheduler|AgentLoop|ToolRegistry|ToolBackendConfig|PermissionChecker|PermissionChoice" src/core src/agent/runtime_factory.rs src/tool/factory.rs
```

Save the before state in `plans/codegg_core_extraction.md` or a short implementation note at the bottom of this plan after the pass.

## Phase 1: Define Core Runtime Input Types

The daemon should accept a compact runtime dependency object instead of several concrete optional fields spread through constructors.

Add a root-side module, preferably:

```text
src/core/runtime_deps.rs
```

or, if `src/core/` already has a better naming convention:

```text
src/core/deps.rs
```

Suggested shape:

```rust
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<codegg_core::memory::MemoryStore>>,
    pub agent_runtime: Option<Arc<dyn AgentRuntimeProvider>>,
    pub task_runtime: Option<Arc<dyn TaskRuntimeProvider>>,
}
```

However, do not force this exact shape if current constructors make a smaller transitional form easier.

The key objective is to avoid exposing `SubAgentPool` and `BackgroundScheduler` directly as fields of `CoreDaemon` long-term.

### Minimum acceptable version

If trait object migration is too much for this pass, introduce a transitional struct that still contains concrete agent types but localizes them:

```rust
pub struct RootAgentRuntimeDeps {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
}
```

Then `CoreDaemon` stores only one `agent_runtime_deps` field instead of multiple concrete fields. This is not the final architecture, but it lowers spread and prepares trait replacement.

Prefer the trait approach if it is straightforward.

## Phase 2: Introduce an Agent Runtime Provider Boundary

The daemon currently needs to start turns and coordinate agent execution. It should not need to construct or know the full concrete `AgentLoop` graph.

Add a narrow root-side trait, probably under:

```text
src/agent/runtime_provider.rs
```

or extend the existing:

```text
src/agent/runtime_factory.rs
```

Suggested concept:

```rust
#[async_trait::async_trait]
pub trait AgentRuntimeProvider: Send + Sync {
    async fn submit_turn(&self, input: AgentTurnInput) -> Result<AgentTurnHandle, crate::error::AppError>;
}
```

But keep it scoped to actual daemon needs. If daemon currently constructs `AgentLoop` and spawns it itself, start with a lower-level boundary:

```rust
pub struct AgentLoopBuildInput { ... }

pub trait AgentLoopFactory: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> Result<crate::agent::AgentLoop, crate::error::AppError>;
}
```

### Preferred transitional approach

Keep `AgentLoop` concrete internally to `src/agent/runtime_factory.rs`, but make `src/core/daemon.rs` call a narrow function/trait that returns a ready-to-run runtime object.

Target import direction:

```text
src/core/daemon.rs -> src/agent/runtime_factory.rs via one small function/trait
src/agent/runtime_factory.rs -> agent, permission, prompt, tool factory
```

Unwanted import direction:

```text
src/core/daemon.rs -> crate::agent::prompt
src/core/daemon.rs -> crate::permission::PermissionChecker
src/core/daemon.rs -> crate::tool::{ToolRegistry, ToolBackendConfig, TaskTool}
```

### Acceptance criteria for Phase 2

After this phase:

```bash
rg -n "crate::permission|PermissionChecker|PermissionChoice" src/core/daemon.rs src/core/mod.rs
```

should return no matches.

This check should already be close to true; preserve it.

Also reduce direct `crate::agent` references in daemon/core. Ideally only type-erased provider/factory references remain.

```bash
rg -n "crate::agent|SubAgentPool|BackgroundScheduler|AgentLoop|prompt::" src/core/daemon.rs src/core/mod.rs
```

Document any remaining matches and why they remain.

## Phase 3: Tighten Tool Registry Factory Boundary

`src/tool/factory.rs` already has `build_session_tool_registry()`, but it may still expose agent-specific types through parameters such as `SubAgentPool`.

The goal is to make tool construction consume a small root-side task/spawn capability rather than a concrete subagent pool, where feasible.

Suggested abstraction:

```rust
pub trait TaskSpawnerProvider: Send + Sync {
    fn task_store(&self) -> Arc<...>;
    fn spawner(&self) -> Arc<...>;
}
```

If the associated types are too awkward, use a simpler transitional DTO:

```rust
pub struct TaskToolRuntime {
    pub task_store: ...,
    pub spawner: ...,
}
```

Then `tool/factory.rs` takes `Option<TaskToolRuntime>` instead of `Option<&Arc<SubAgentPool>>`.

### Important constraint

Do not make `codegg-core` depend on this trait/DTO yet. Keep it root-side until the concrete associated types are stable.

### Acceptance criteria for Phase 3

```bash
rg -n "SubAgentPool|crate::agent" src/tool/factory.rs
```

should ideally return no matches.

If it still returns matches, document why the current task tool construction requires direct subagent pool access and what the next boundary should be.

## Phase 4: Consolidate Turn Submission Logic

Look for duplicated or spread-out code in `src/core/daemon.rs` related to:

```text
TurnSubmit
AgentLoop construction
permission checker construction
tool registry construction
active turn registration
event publishing
```

Move orchestration details that are not protocol-specific into one root-side module, such as:

```text
src/core/turn_runner.rs
```

or:

```text
src/agent/turn_runtime.rs
```

A good boundary is:

```rust
pub async fn run_turn(input: TurnRunInput) -> Result<TurnRunOutput, AppError>
```

`CoreDaemon` should remain responsible for:

- decoding `CoreRequest::TurnSubmit`
- assigning/reading session/turn IDs
- updating `SessionRuntimeRegistry`
- publishing protocol/core events
- returning `CoreResponse`

The turn runtime should own:

- building agent runtime
- building permission/tool runtime through factories
- executing the turn
- returning completion/failure data

Do not overdo this. A single helper function with a clear input struct is better than a large trait hierarchy.

## Phase 5: Update `src/core/mod.rs` and Constructors

If `CoreDaemon::new(...)` currently takes several concrete optional runtime dependencies, introduce a more explicit constructor:

```rust
impl CoreDaemon {
    pub fn new(deps: CoreRuntimeDeps) -> Self { ... }

    pub fn new_legacy(
        pool: Option<sqlx::SqlitePool>,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<codegg_core::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    ) -> Self { ... }
}
```

Alternatively, keep the old constructor and add:

```rust
pub fn with_deps(deps: CoreRuntimeDeps) -> Self
```

Then migrate call sites gradually.

Do not break existing CLI/TUI construction paths in this pass.

## Phase 6: Dependency and Boundary Checks

After refactoring, run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

Then run coupling checks:

```bash
rg -n "crate::permission|PermissionChecker|PermissionChoice" src/core/daemon.rs src/core/mod.rs
rg -n "crate::tool|ToolRegistry|ToolBackendConfig|TaskTool" src/core/daemon.rs src/core/mod.rs
rg -n "crate::agent|SubAgentPool|BackgroundScheduler|AgentLoop|prompt::" src/core/daemon.rs src/core/mod.rs
rg -n "SubAgentPool|crate::agent" src/tool/factory.rs
```

Expected outcomes:

- permission matches in daemon/core: zero
- tool matches in daemon/core: zero or substantially reduced
- agent matches in daemon/core: substantially reduced; preferably behind one provider/factory type
- tool factory direct `SubAgentPool` usage: preferably zero, but documented if not

## Phase 7: Update Architecture and Plans

Update:

```text
architecture/core.md
plans/codegg_core_extraction.md
```

Record:

- new runtime/factory modules added
- before/after coupling counts
- what `CoreDaemon` still owns
- what agent/tool/permission factories now own
- whether `src/core/daemon.rs` is closer to being movable
- remaining blockers before moving `src/core/**` or daemon logic

If `src/core/daemon.rs` is still not movable, say so explicitly. That is acceptable.

## Acceptance Criteria

This pass is complete when:

1. Existing workspace checks pass:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

2. `codegg-core` remains free of forbidden imports and forbidden UI/server/plugin deps.
3. `src/core/daemon.rs` no longer imports or directly constructs permission checker objects.
4. `src/core/daemon.rs` no longer directly constructs tool registry internals; it goes through `src/tool/factory.rs` or equivalent.
5. Direct agent coupling in `src/core/daemon.rs` is reduced and localized behind a factory/provider/runtime seam.
6. Constructor inputs for `CoreDaemon` are clearer and less spread across concrete agent/tool internals.
7. Runtime behavior is unchanged for ordinary turn submission.
8. Architecture docs record remaining blockers honestly.

## Suggested Commit Structure

1. `Add core runtime dependency container`
2. `Introduce agent runtime provider seam`
3. `Tighten task tool runtime factory boundary`
4. `Extract turn submission orchestration helper`
5. `Migrate CoreDaemon constructors to runtime deps`
6. `Update core architecture docs and extraction notes`

## Notes for Implementer

This pass should be incremental. Avoid abstracting every type in the agent runtime. The objective is to reduce concrete dependency spread from daemon/core code, not to invent a full plugin architecture.

Prefer one or two narrow data structs and helper functions over a large trait hierarchy. Trait boundaries are useful only where they make the next crate move easier.

If a clean trait boundary becomes awkward because associated types are complex, stop at a root-side DTO/factory function and document the remaining coupling. A smaller successful boundary pass is better than a brittle abstraction.
