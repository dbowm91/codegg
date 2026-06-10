# Turn Runtime Final Hardening Handoff Plan

> **Status: Completed** (2026-06-09)
> All phases implemented: injected runtime test added, `AgentRuntimeProvider` renamed to `AgentLoopFactory`, `LegacyAgentRuntimeDeps` groups concrete agent deps, `bg_scheduler` usage audited, architecture docs updated. All acceptance criteria met.

## Purpose

The daemon/turn-runtime boundary is now real:

- `CoreRuntimeDeps` owns a non-optional `turn_runtime: Arc<dyn TurnRuntime>`.
- `CoreRuntimeDeps::new(...)` defaults to `DefaultTurnRuntime`.
- `CoreRuntimeDeps::with_turn_runtime(...)` supports injection.
- `CoreDaemon::TurnSubmit` calls `self.deps.turn_runtime.run_turn(...)` instead of constructing `DefaultTurnRuntime` directly.

This pass should finish the boundary by adding proof coverage, clarifying the remaining build-only factory terminology, and grouping concrete legacy agent dependencies so their transitional status is obvious.

This is a small hardening pass. Do not broaden it into daemon extraction or agent-loop redesign.

## Non-Goals

Do not move `src/core/daemon.rs` into `codegg-core`.

Do not move `src/agent`, `src/tool`, or `src/permission`.

Do not change `CoreRequest` / `CoreResponse` schema.

Do not change model-facing tool names or tool schemas.

Do not change permission semantics.

Do not rewrite provider/model routing.

Do not change ordinary turn execution behavior.

Do not remove legacy constructors unless all call sites are migrated cleanly.

Do not weaken `scripts/check-core-boundary.sh`.

## Current State

The desired path now exists:

```text
CoreDaemon::TurnSubmit
  -> validates protocol/session concerns
  -> creates active turn
  -> emits TurnStarted
  -> builds TurnRunInput
  -> calls self.deps.turn_runtime.run_turn(...)
  -> stores cancel/steer handles
```

The remaining rough edges:

1. There is no clear test proving that injected `TurnRuntime` is actually used.
2. `AgentRuntimeProvider` still exists as a build-only factory that returns concrete `AgentLoop`; it is documented as transitional but still named like a runtime provider.
3. `CoreRuntimeDeps` still exposes concrete `SubAgentPool` and `BackgroundScheduler` directly.
4. Architecture docs should record the final boundary state and remaining blockers.

## Phase 0: Baseline

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

If any command already fails, record the failure before changing code.

Capture current runtime-boundary state:

```bash
rg -n "turn_runtime|with_turn_runtime|DefaultTurnRuntime|TurnRuntime|SubAgentPool|BackgroundScheduler|AgentRuntimeProvider|AgentLoopBuildInput|build_agent_loop" src/core src/agent
```

## Phase 1: Add Injected Runtime Coverage

Add a test proving that `CoreDaemon` uses the injected runtime from `CoreRuntimeDeps`.

Preferred location:

```text
src/core/daemon.rs
```

inside a `#[cfg(test)] mod tests` block, unless there is already a better core test module.

### Test shape

Implement a small fake runtime:

```rust
struct FakeTurnRuntime {
    called: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl crate::agent::turn_runtime::TurnRuntime for FakeTurnRuntime {
    async fn run_turn(
        &self,
        _input: crate::agent::turn_runtime::TurnRunInput,
    ) -> Result<crate::agent::turn_runtime::TurnRunOutput, crate::error::AppError> {
        self.called.store(true, std::sync::atomic::Ordering::SeqCst);
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(false);
        let (steer_tx, _steer_rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(crate::agent::turn_runtime::TurnRunOutput { cancel_tx, steer_tx })
    }
}
```

Then construct:

```rust
let called = Arc::new(AtomicBool::new(false));
let deps = CoreRuntimeDeps::new(None, None, None, None)
    .with_turn_runtime(Arc::new(FakeTurnRuntime { called: called.clone() }));
let daemon = CoreDaemon::with_deps(deps);
```

Submit a minimal valid `CoreRequest::TurnSubmit`.

The request needs:

- one agent DTO
- `current_agent_idx = 0`
- a model string whose provider exists in default config, or enough config setup to satisfy the daemon's provider pre-validation
- empty or minimal messages
- `plan_mode = false`

Assert:

```rust
assert!(called.load(Ordering::SeqCst));
```

Also assert `CoreResponse::Ack` if practical.

### If a full `TurnSubmit` test is too heavy

If constructing a valid `TurnSubmit` is blocked by provider/config assumptions, add a narrower unit test around `CoreRuntimeDeps::with_turn_runtime(...)` and document why full daemon coverage is deferred. But prefer the daemon-level test because the seam exists specifically for daemon injection.

## Phase 2: Rename Build-Only Provider to `AgentLoopFactory`

The existing `src/agent/runtime_provider.rs` is now misnamed. It does not provide a runtime; it builds an `AgentLoop`.

Preferred cleanup:

1. Rename the trait:

```rust
pub trait AgentLoopFactory: Send + Sync {
    fn build_agent_loop(&self, input: AgentLoopBuildInput) -> crate::agent::r#loop::AgentLoop;
}
```

2. Rename implementation:

```rust
pub struct DefaultAgentLoopFactory;
```

3. Update `DefaultTurnRuntime` to use `DefaultAgentLoopFactory`.

4. Keep the module path `runtime_provider.rs` if moving files would cause too much churn, but update comments to say the module is legacy/transitional. Prefer adding a later TODO to rename the file to `agent_loop_factory.rs`.

Better option if churn is acceptable:

```text
src/agent/runtime_provider.rs -> src/agent/agent_loop_factory.rs
```

and update `src/agent/mod.rs` accordingly.

### Compatibility option

If renaming public names is too risky, keep type aliases temporarily:

```rust
pub type AgentRuntimeProvider = dyn AgentLoopFactory;
```

Only use aliases if needed. Prefer direct rename if the symbol is internal.

### Acceptance Criteria

Core should not mention the build-only factory:

```bash
rg -n "AgentRuntimeProvider|AgentLoopBuildInput|build_agent_loop|DefaultAgentRuntimeProvider" src/core
```

Expected: no matches.

The agent module should make the build-only nature clear:

```bash
rg -n "AgentLoopFactory|DefaultAgentLoopFactory|Transitional build-only" src/agent
```

Expected: clear matches.

## Phase 3: Group Legacy Concrete Agent Dependencies

`CoreRuntimeDeps` still has:

```rust
pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
```

Group these under a clearly transitional container:

```rust
#[derive(Clone, Default)]
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

Keep `CoreRuntimeDeps::new(pool, subagent_pool, memory_store, bg_scheduler)` for compatibility, but have it populate `legacy_agent`.

Add a clearer constructor for new code:

```rust
pub fn from_parts(
    pool: Option<sqlx::SqlitePool>,
    memory_store: Option<Arc<crate::memory::MemoryStore>>,
    legacy_agent: LegacyAgentRuntimeDeps,
    turn_runtime: Arc<dyn TurnRuntime>,
) -> Self
```

or a builder if the project already prefers builders.

### Update call sites

Change references like:

```rust
self.deps.subagent_pool.clone()
self.deps.bg_scheduler.clone()
```

to:

```rust
self.deps.legacy_agent.subagent_pool.clone()
self.deps.legacy_agent.bg_scheduler.clone()
```

Do not remove the legacy deps if still required for task/background scheduling. Just make their legacy status explicit.

## Phase 4: Audit `bg_scheduler` Usage

Search:

```bash
rg -n "bg_scheduler|BackgroundScheduler" src/core src/agent src/tool
```

If `bg_scheduler` is no longer used by daemon or turn runtime, remove it from `CoreRuntimeDeps` entirely or keep it only in `LegacyAgentRuntimeDeps` with a TODO.

If it is still used for task scheduling, leave it grouped under `legacy_agent` and document why.

## Phase 5: Documentation Updates

Update:

```text
architecture/core.md
plans/codegg_core_extraction.md
plans/turn_runtime_boundary.md
plans/turn_runtime_wiring_cleanup.md
```

Record:

- injected turn runtime is now covered by test
- `CoreDaemon` uses `deps.turn_runtime`
- build-only provider has been renamed to `AgentLoopFactory` or documented as transitional
- concrete agent dependencies are grouped under `LegacyAgentRuntimeDeps`
- remaining blockers before daemon/core extraction

## Phase 6: Validation

Run:

```bash
cargo ckcore
cargo ckroot
cargo ck
cargo test --workspace
scripts/check-core-boundary.sh
```

Then run targeted checks:

```bash
rg -n "DefaultTurnRuntime" src/core/daemon.rs
rg -n "AgentRuntimeProvider|DefaultAgentRuntimeProvider" src/core src/agent/turn_runtime.rs
rg -n "AgentLoopBuildInput|build_agent_loop" src/core
rg -n "subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>" src/core/runtime_deps.rs
rg -n "LegacyAgentRuntimeDeps" src/core/runtime_deps.rs architecture/core.md
rg -n "crate::(agent|tool|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src
```

Expected:

- no direct `DefaultTurnRuntime` construction in daemon
- no old `AgentRuntimeProvider` terminology in core
- no build-loop factory references in core
- concrete agent deps are grouped or explicitly documented
- `codegg-core` remains clean

## Acceptance Criteria

This pass is complete when:

1. There is a test or credible minimal proof that injected `TurnRuntime` is used by `CoreDaemon::TurnSubmit`.
2. The build-only agent loop provider is renamed to `AgentLoopFactory` / `DefaultAgentLoopFactory` or is explicitly marked transitional with no core usage.
3. `CoreRuntimeDeps` groups remaining concrete agent fields under `LegacyAgentRuntimeDeps`, or documents why that grouping was deferred.
4. `CoreDaemon` still uses `self.deps.turn_runtime.run_turn(...)` and does not construct `DefaultTurnRuntime` directly.
5. `src/core/**` has no references to `AgentRuntimeProvider`, `AgentLoopBuildInput`, or `build_agent_loop`.
6. `codegg-core` boundary check still passes.
7. Workspace checks pass or pre-existing failures are documented.

## Suggested Commit Structure

1. `Add turn runtime injection test`
2. `Rename build-only provider to agent loop factory`
3. `Group legacy agent runtime deps`
4. `Update core architecture documentation`

## Notes for Implementer

Keep this pass small. The purpose is to finish the boundary, not to extract daemon.

The fake-runtime test is the most important item. Without it, the trait seam is easy to accidentally bypass later.

If renaming files causes churn, rename types first and leave file movement for a later cleanup.
