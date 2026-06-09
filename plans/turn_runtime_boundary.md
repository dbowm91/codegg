# Turn Runtime Boundary Handoff Plan

## Purpose

The previous daemon-boundary pass improved the shape of root `src/core/daemon.rs` by introducing:

- `core::runtime_deps::CoreRuntimeDeps`
- `agent::runtime_provider::{AgentRuntimeProvider, AgentLoopBuildInput}`
- `agent::task_tool_runtime::TaskToolRuntime`
- tighter `tool::factory::build_session_tool_registry(...)`

That pass localized concrete coupling, but it did not yet fully abstract turn execution. The current `AgentRuntimeProvider` is effectively a build-only factory: it returns a concrete `AgentLoop`, and the daemon still remains too close to the mechanics of turn execution.

This pass should turn the build-only seam into an execution-oriented boundary. The desired outcome is that daemon/core code submits a `TurnRunInput` to a root-side runtime service, and that service owns agent loop construction, tool registry construction, permission setup, task/subagent runtime setup, and actual turn execution.

Do not move `src/core/daemon.rs` into `codegg-core` yet. Do not move agent/tool/permission modules. This pass prepares the seam for a later move by making daemon less aware of agent internals.

## Non-Goals

Do not move `src/core/daemon.rs` into `codegg-core`.

Do not move `src/agent`, `src/tool`, or `src/permission`.

Do not move TUI, server, MCP, plugin, search, research, auth, crypto, theme, TTS, or upgrade.

Do not redesign agent prompting, routing, tool schema generation, model selection, or permission semantics.

Do not change `CoreRequest` / `CoreResponse` protocol shape.

Do not change model-facing tool names or schemas.

Do not introduce a large trait hierarchy. Prefer one execution trait and a small set of DTOs.

Do not weaken `scripts/check-core-boundary.sh`.

## Current State

`CoreRuntimeDeps` currently still contains concrete agent runtime types:

```rust
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
}
```

`AgentRuntimeProvider` currently builds an `AgentLoop`:

```rust
pub trait AgentRuntimeProvider: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop;
}
```

This is useful, but not enough. The daemon should not need to hold or care about `AgentLoop`; it should care about turn submission lifecycle and protocol/event state.

## Phase 0: Baseline Coupling Inventory

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

Record any pre-existing failures.

Capture coupling before changes:

```bash
rg -n "AgentLoop|AgentLoopBuildInput|build_agent_loop|SubAgentPool|BackgroundScheduler|ToolRegistry|ToolBackendConfig|PermissionChecker|PermissionChoice|TaskToolRuntime" src/core src/agent src/tool/factory.rs
```

Also inspect the full `TurnSubmit` path in `src/core/daemon.rs`:

```bash
rg -n "TurnSubmit|AgentLoop|build_agent_loop|build_session_tool_registry|active_turn|TurnStarted|TurnCompleted|TurnFailed" src/core/daemon.rs
```

The implementation notes should include a brief before/after count for the most important direct daemon references.

## Phase 1: Introduce Turn Runtime DTOs

Add a new module:

```text
src/agent/turn_runtime.rs
```

or, if this fits better under the core facade:

```text
src/core/turn_runtime.rs
```

Preferred location: `src/agent/turn_runtime.rs`, because it will own agent/tool/permission mechanics. `src/core` should become a caller of this runtime, not the owner of agent execution.

Define compact DTOs that reflect what the daemon needs to ask for and what it needs back.

Suggested shape:

```rust
pub struct TurnRunInput {
    pub session_id: String,
    pub turn_id: String,
    pub user_input: String,
    pub agents: Vec<crate::agent::Agent>,
    pub provider: Box<dyn crate::provider::Provider>,
    pub config: crate::config::schema::Config,
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<std::sync::Arc<crate::memory::MemoryStore>>,
    pub subagent_pool: Option<std::sync::Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<std::sync::Arc<crate::agent::task::BackgroundScheduler>>,
    pub task_state_policy: crate::model_profile::types::TaskStatePolicy,
    pub mcp_service: Option<std::sync::Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
}

pub struct TurnRunOutput {
    pub session_id: String,
    pub turn_id: String,
    pub assistant_message_id: Option<String>,
    pub summary: Option<String>,
}
```

Do not force these exact fields if current daemon code needs a different payload. The rule is: fields should be turn execution inputs/outputs, not protocol envelope details.

If the input becomes too large, split it into:

```rust
pub struct TurnRuntimeDeps { ... }
pub struct TurnRunRequest { ... }
```

That is acceptable.

## Phase 2: Replace Build-Only Provider with Execution Provider

Extend or replace `AgentRuntimeProvider` so it can execute a turn, not just build an `AgentLoop`.

Suggested trait:

```rust
#[async_trait::async_trait]
pub trait AgentRuntimeProvider: Send + Sync {
    async fn run_turn(&self, input: TurnRunInput) -> Result<TurnRunOutput, crate::error::AppError>;
}
```

The default implementation should live root-side and can internally call existing helpers:

```rust
pub struct DefaultAgentRuntimeProvider;

#[async_trait::async_trait]
impl AgentRuntimeProvider for DefaultAgentRuntimeProvider {
    async fn run_turn(&self, input: TurnRunInput) -> Result<TurnRunOutput, AppError> {
        // 1. Build TaskToolRuntime from subagent pool if present.
        // 2. Build ToolRegistry via tool::factory::build_session_tool_registry.
        // 3. Build AgentLoop through existing runtime_factory or inline logic.
        // 4. Execute the turn.
        // 5. Return TurnRunOutput.
    }
}
```

Keep the old `build_agent_loop` method only if many call sites still depend on it. If kept, mark it as transitional in comments.

### Important boundary rule

After this phase, `src/core/daemon.rs` should not call:

```rust
crate::agent::runtime_factory::build_agent_loop
crate::tool::factory::build_session_tool_registry
crate::permission::PermissionChecker
```

Those calls should be inside the default runtime provider.

## Phase 3: Move Tool/Permission Setup into Runtime Provider

The runtime provider should own the mechanics of building tool and permission machinery.

Move the following responsibilities out of daemon code if they are still there:

```text
TaskToolRuntime::from_subagent_pool(...)
build_session_tool_registry(...)
PermissionChecker construction
AgentLoop construction
agent prompt/runtime construction details
```

`src/core/daemon.rs` should pass `TurnRunInput` and receive `TurnRunOutput`; it should not assemble the tool registry or permission checker itself.

`src/tool/factory.rs` can still construct the tool registry. `src/agent/turn_runtime.rs` or `src/agent/runtime_provider.rs` should call it.

## Phase 4: Adjust `CoreRuntimeDeps`

Once the runtime provider owns execution, update `CoreRuntimeDeps` to prefer a provider field.

Target shape:

```rust
#[derive(Clone)]
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub agent_runtime: Arc<dyn crate::agent::runtime_provider::AgentRuntimeProvider>,
}
```

If still needed for default runtime construction, keep a root-side builder:

```rust
pub struct CoreRuntimeDepsBuilder {
    pool: Option<sqlx::SqlitePool>,
    memory_store: Option<Arc<MemoryStore>>,
    subagent_pool: Option<Arc<SubAgentPool>>,
    bg_scheduler: Option<Arc<BackgroundScheduler>>,
}
```

The builder may know concrete agent types. `CoreRuntimeDeps` should not, if feasible.

If this is too much for one pass, a good intermediate state is:

```rust
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub agent_runtime: Option<Arc<dyn AgentRuntimeProvider>>,
    pub legacy_agent_deps: Option<RootAgentRuntimeDeps>,
}
```

But avoid indefinite dual paths. Prefer a clean default `Arc<DefaultAgentRuntimeProvider>`.

## Phase 5: Update `CoreDaemon::TurnSubmit` Handling

Refactor `TurnSubmit` handling in `src/core/daemon.rs` so daemon owns protocol/runtime state, not agent execution mechanics.

Daemon should still own:

```text
request envelope validation
session_id / turn_id handling
SessionRuntimeRegistry active-turn state
CoreEvent TurnStarted / TurnCompleted / TurnFailed publishing
CoreResponse Ack/Error
cancellation/steering handles if already present
```

Runtime provider should own:

```text
agent/tool/permission setup
AgentLoop construction
AgentLoop execution
assistant/message output details
```

If event publishing currently occurs deep inside `AgentLoop`, do not rewrite that entire flow. Just avoid moving event semantics unless necessary. The first pass can have provider call existing loop behavior and daemon keep high-level start/fail/completion events.

### Acceptance target

`src/core/daemon.rs` should have zero direct `AgentLoop` references.

```bash
rg -n "AgentLoop|AgentLoopBuildInput|build_agent_loop|build_session_tool_registry|PermissionChecker|ToolRegistry|ToolBackendConfig|TaskToolRuntime" src/core/daemon.rs
```

Expected: zero matches, or only comments documenting old behavior.

## Phase 6: Keep `codegg-core` Clean

Run:

```bash
scripts/check-core-boundary.sh
```

Do not add imports from `codegg-core` back into agent/tool/permission in ways that invert desired ownership. The root runtime provider may depend on `codegg-core` types, but `codegg-core` must not depend on root runtime provider types.

## Phase 7: Tests and Validation

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

If available, run the main TUI/CLI smoke path manually:

```bash
cargo run -- --help
cargo run -- core-stdio --help
```

If these commands are not feasible in the environment, document that they were not run.

## Phase 8: Documentation Updates

Update:

```text
architecture/core.md
plans/codegg_core_extraction.md
```

Record:

- `AgentRuntimeProvider` is now execution-oriented, not build-only.
- where turn execution logic lives
- what daemon still owns
- what agent runtime provider owns
- whether `CoreRuntimeDeps` still contains concrete agent types
- whether `src/core/daemon.rs` still references `AgentLoop`, `ToolRegistry`, `PermissionChecker`, or `SubAgentPool`
- remaining blockers before moving any `src/core/**` modules

## Acceptance Criteria

This pass is complete when:

1. `AgentRuntimeProvider` or equivalent has an async `run_turn(...)`-style execution method.
2. Default runtime provider owns tool registry construction and agent loop construction.
3. `src/core/daemon.rs` no longer directly references `AgentLoop`, `AgentLoopBuildInput`, `build_agent_loop`, `ToolRegistry`, `ToolBackendConfig`, `PermissionChecker`, or `TaskToolRuntime`.
4. `CoreRuntimeDeps` is cleaner. Preferably it stores an `Arc<dyn AgentRuntimeProvider>` rather than concrete `SubAgentPool` / `BackgroundScheduler`. If not feasible, remaining concrete fields are documented as transitional.
5. Runtime behavior for ordinary turn submission is unchanged.
6. `codegg-core` boundary check still passes.
7. These pass or have documented pre-existing failures:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

## Suggested Commit Structure

1. `Add turn runtime DTOs`
2. `Make agent runtime provider execute turns`
3. `Move tool and permission setup into default runtime provider`
4. `Refactor daemon turn submit through runtime provider`
5. `Simplify CoreRuntimeDeps around provider seam`
6. `Update core architecture docs`

## Notes for Implementer

Keep the abstraction narrow. The point is not to design a permanent runtime framework. The point is to remove agent/tool/permission execution mechanics from daemon/core code so that future crate moves are possible.

If an async trait creates too much churn, use a concrete `DefaultTurnRuntime` struct with an async `run_turn` method first. A concrete service object is better than a leaky trait.

Do not move events unless necessary. Preserve current event stream behavior first, then clean up event ownership in a later pass.
