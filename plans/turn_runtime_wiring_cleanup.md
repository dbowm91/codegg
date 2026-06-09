# Turn Runtime Wiring Cleanup Handoff Plan

## Purpose

The previous pass introduced an execution-oriented turn runtime boundary:

- `src/agent/turn_runtime.rs`
- `TurnRunInput`
- `TurnRunOutput`
- `TurnRuntime`
- `DefaultTurnRuntime`

`CoreDaemon::TurnSubmit` now delegates agent/tool/permission execution mechanics to `DefaultTurnRuntime`, which is the right architectural direction.

However, the current wiring is incomplete:

1. `CoreRuntimeDeps` has an `agent_runtime: Option<Arc<dyn TurnRuntime>>`, but `CoreDaemon::TurnSubmit` appears to instantiate `DefaultTurnRuntime` directly rather than using the injected runtime.
2. `CoreRuntimeDeps` still exposes concrete `SubAgentPool` and `BackgroundScheduler` fields.
3. The older build-only `AgentRuntimeProvider` / `AgentLoopBuildInput` seam still exists and should be marked internal/transitional or narrowed.
4. Some provider-resolution validation may now be duplicated between daemon and turn runtime.

This pass should finish the wiring and reduce transitional duplication without changing runtime behavior.

## Non-Goals

Do not move `src/core/daemon.rs` into `codegg-core`.

Do not move `src/agent`, `src/tool`, or `src/permission`.

Do not change `CoreRequest` / `CoreResponse` schema.

Do not change model-facing tool names or tool schemas.

Do not redesign the agent loop.

Do not change permission semantics.

Do not rewrite all turn event publishing.

Do not remove compatibility constructors unless all call sites are migrated cleanly.

Do not weaken `scripts/check-core-boundary.sh`.

## Current Problem

The desired boundary is:

```text
CoreDaemon::TurnSubmit
  -> builds protocol/session-level TurnRunInput
  -> calls injected Arc<dyn TurnRuntime>
  -> stores returned cancel/steer handles
```

Current code appears closer to:

```text
CoreDaemon::TurnSubmit
  -> constructs DefaultTurnRuntime directly
  -> calls run_turn(...)
```

That means tests or alternate runtimes cannot replace turn execution via `CoreRuntimeDeps.agent_runtime`, even though the field exists.

## Phase 0: Baseline

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

If any command fails before changes, record the failure in implementation notes.

Capture current coupling:

```bash
rg -n "DefaultTurnRuntime|agent_runtime|TurnRuntime|SubAgentPool|BackgroundScheduler|AgentLoopBuildInput|AgentRuntimeProvider|build_agent_loop" src/core src/agent
```

## Phase 1: Make `CoreRuntimeDeps` Own a Default Turn Runtime

Change `CoreRuntimeDeps` so the daemon can always ask it for a runtime.

Preferred shape:

```rust
#[derive(Clone)]
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    pub turn_runtime: Arc<dyn crate::agent::turn_runtime::TurnRuntime>,
}
```

Use `turn_runtime`, not `agent_runtime`, to match the actual abstraction.

`CoreRuntimeDeps::new(...)` should set:

```rust
turn_runtime: Arc::new(crate::agent::turn_runtime::DefaultTurnRuntime)
```

Add an override method:

```rust
pub fn with_turn_runtime(mut self, runtime: Arc<dyn TurnRuntime>) -> Self {
    self.turn_runtime = runtime;
    self
}
```

If deriving `Default` becomes awkward because trait objects do not implement `Default`, write a manual `impl Default for CoreRuntimeDeps`.

### Acceptance Criteria

`CoreRuntimeDeps` should no longer use `Option<Arc<dyn TurnRuntime>>`; it should always have a runtime.

```bash
rg -n "agent_runtime|Option<Arc<dyn TurnRuntime>>" src/core/runtime_deps.rs
```

Expected: no matches.

## Phase 2: Wire Daemon Through Injected Runtime

Update `CoreDaemon::TurnSubmit` so it uses the runtime from `self.deps`.

Replace direct construction like:

```rust
let turn_runtime = crate::agent::turn_runtime::DefaultTurnRuntime;
let turn_output = turn_runtime.run_turn(turn_input).await?;
```

with:

```rust
let turn_output = self.deps.turn_runtime.run_turn(turn_input).await?;
```

or, if naming differs:

```rust
let turn_output = self.deps.turn_runtime().run_turn(turn_input).await?;
```

Do not clone large turn input values unnecessarily. Clone only the `Arc` runtime if needed.

### Acceptance Criteria

```bash
rg -n "DefaultTurnRuntime" src/core/daemon.rs
```

Expected: no matches.

```bash
rg -n "turn_runtime\.run_turn|deps\.turn_runtime" src/core/daemon.rs
```

Expected: at least one match in `TurnSubmit`.

## Phase 3: Rename and Clarify Runtime Terms

The field is currently named `agent_runtime`, but the trait is `TurnRuntime`. Use consistent terms:

- `TurnRuntime` for execution-oriented runtime.
- `turn_runtime` for fields/variables.
- `AgentRuntimeProvider` only for the older build-only factory if kept.

Update docs/comments:

```text
src/core/runtime_deps.rs
src/agent/turn_runtime.rs
architecture/core.md
plans/codegg_core_extraction.md
```

Avoid saying `agent_runtime` when the type is actually `TurnRuntime`.

## Phase 4: Remove Duplicate Provider Validation in Daemon if Safe

`DefaultTurnRuntime::run_turn(...)` already resolves the provider and returns `AppError::Provider(...)` on missing provider.

If `CoreDaemon::TurnSubmit` still builds a provider registry just to validate provider existence before delegating, consider removing that duplicate validation.

### Preferred behavior

Daemon should validate only protocol/session conditions:

```text
current_agent_idx bounds
active turn already running
session runtime state
```

Turn runtime should validate execution conditions:

```text
provider exists
model profile resolution
tool registry construction
agent loop setup
```

If removing daemon provider validation changes the `CoreResponse::Error` shape too much, keep it for this pass and document it as duplication. Do not break user-visible behavior just to reduce duplication.

### Acceptance Criteria

Either:

- provider validation is removed from daemon and missing-provider errors are converted cleanly into `CoreResponse::Error`, or
- the duplicate validation remains with a comment explaining it preserves existing `provider_not_found` response shape.

## Phase 5: Narrow or Mark `AgentRuntimeProvider` as Transitional

The old build-only provider still returns a concrete `AgentLoop`:

```rust
pub trait AgentRuntimeProvider: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop;
}
```

Now that `TurnRuntime` exists, this trait should either become private/internal or be renamed to reflect what it does.

### Option A: Rename to `AgentLoopFactory`

Preferred if it is still useful internally:

```rust
pub trait AgentLoopFactory: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop;
}

pub struct DefaultAgentLoopFactory;
```

Update `DefaultTurnRuntime` to use `AgentLoopFactory` terminology.

### Option B: Make the existing trait crate-private

If renaming is too much churn, make the comments explicit:

```rust
/// Transitional build-only factory used internally by DefaultTurnRuntime.
/// Do not inject this into CoreDaemon; use TurnRuntime instead.
```

### Acceptance Criteria

`src/core/**` should not import or mention `AgentRuntimeProvider`, `AgentLoopBuildInput`, or `build_agent_loop`.

```bash
rg -n "AgentRuntimeProvider|AgentLoopBuildInput|build_agent_loop" src/core
```

Expected: no matches.

## Phase 6: Consider a Builder for Concrete Legacy Agent Deps

`CoreRuntimeDeps` still has concrete fields:

```rust
subagent_pool: Option<Arc<SubAgentPool>>
bg_scheduler: Option<Arc<BackgroundScheduler>>
```

Do not remove them if many call sites still need them. But reduce their conceptual weight.

Preferred improvement:

```rust
pub struct LegacyAgentRuntimeDeps {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
}
```

Then:

```rust
pub struct CoreRuntimeDeps {
    pub pool: Option<sqlx::SqlitePool>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub legacy_agent: LegacyAgentRuntimeDeps,
    pub turn_runtime: Arc<dyn TurnRuntime>,
}
```

This makes clear that concrete agent fields are transitional legacy wiring, not daemon-core ownership.

If this creates too much churn, leave the fields but update comments to say they exist only to populate `TurnRunInput` until runtime construction owns those dependencies fully.

## Phase 7: Testing Hook for Fake Runtime

Add a small test-only fake runtime to prove injection works.

Suggested location:

```text
src/core/daemon.rs #[cfg(test)] mod tests
```

or:

```text
tests/core_turn_runtime_injection.rs
```

The test should:

1. Implement a minimal `TurnRuntime` that records it was called and returns dummy cancel/steer channels.
2. Build `CoreRuntimeDeps::new(...).with_turn_runtime(Arc::new(fake))`.
3. Construct `CoreDaemon::with_deps(deps)`.
4. Submit a minimal valid `CoreRequest::TurnSubmit`.
5. Assert the fake runtime was invoked.

If building a full `TurnSubmit` is too heavy, add a smaller unit test for `CoreRuntimeDeps::with_turn_runtime` and a documented TODO for integration coverage. Prefer real daemon coverage if feasible.

## Phase 8: Validation

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

Then run coupling checks:

```bash
rg -n "DefaultTurnRuntime" src/core/daemon.rs
rg -n "AgentRuntimeProvider|AgentLoopBuildInput|build_agent_loop" src/core
rg -n "agent_runtime" src/core src/agent architecture plans
rg -n "crate::(agent|tool|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src
```

Expected:

- daemon does not instantiate `DefaultTurnRuntime` directly
- core does not mention build-only agent loop factory types
- terminology uses `turn_runtime` for the execution runtime
- `codegg-core` remains clean

## Phase 9: Documentation Updates

Update:

```text
architecture/core.md
plans/codegg_core_extraction.md
plans/turn_runtime_boundary.md
```

Record:

- daemon uses injected `TurnRuntime`
- fake runtime/injection test, if added
- old `AgentRuntimeProvider` status: renamed, internal, or transitional
- whether provider validation remains duplicated and why
- remaining blockers before daemon can move closer to `codegg-core`

## Acceptance Criteria

This pass is complete when:

1. `CoreRuntimeDeps` has a non-optional `turn_runtime: Arc<dyn TurnRuntime>` or equivalent accessor.
2. `CoreDaemon::TurnSubmit` uses the injected runtime instead of constructing `DefaultTurnRuntime` directly.
3. `src/core/**` has no references to `AgentRuntimeProvider`, `AgentLoopBuildInput`, or `build_agent_loop`.
4. Runtime terminology is consistent: `TurnRuntime` / `turn_runtime`, not `agent_runtime`, for execution-level runtime.
5. The old build-only provider is renamed to `AgentLoopFactory` or explicitly documented as transitional/internal.
6. A test or minimal proof exists that injected runtime wiring is used.
7. `scripts/check-core-boundary.sh` still passes.
8. Workspace checks pass or pre-existing failures are documented.

## Suggested Commit Structure

1. `Make CoreRuntimeDeps own a default turn runtime`
2. `Route CoreDaemon turn submit through injected runtime`
3. `Clarify build-only agent loop factory terminology`
4. `Add turn runtime injection coverage`
5. `Update core architecture docs`

## Notes for Implementer

This is a small but important wiring pass. Do not broaden scope into daemon extraction or agent loop redesign.

The key invariant: the daemon should be injectable and testable at the turn execution boundary. If `CoreDaemon` still constructs `DefaultTurnRuntime` directly, the boundary is not complete.
