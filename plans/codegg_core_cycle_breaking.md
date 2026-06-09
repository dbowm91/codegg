# `codegg-core` Cycle-Breaking Handoff Plan

## Purpose

The previous modularization passes successfully extracted `codegg-config`, `codegg-protocol`, and `codegg-providers`, then consolidated the protocol surface so root `codegg::protocol` now re-exports `codegg_protocol`. The repository is ready for the next preparatory step before a real `codegg-core` crate extraction.

This pass should break the concrete dependency cycles identified in `plans/codegg_core_extraction.md` without yet moving all Group A modules into a new crate. The goal is to make the future `codegg-core` extraction mostly mechanical.

Do not start by creating `crates/codegg-core`. First make the root module graph extractable.

## Current Problem

The readiness note found that many Group A modules are clean, but several root-core modules still depend on higher-coupling domains:

```text
src/core/daemon.rs -> agent, permission, tool
src/core/mod.rs    -> agent
src/error.rs       -> plugin, permission, mcp, lsp
src/bus/mod.rs     -> permission::PermissionChoice
src/goal/tool.rs   -> tool::Tool
src/protocol_conversions.rs -> agent::Agent
src/task_state/mod.rs -> model_profile types
```

Some of these are acceptable temporarily, but the first three are blockers for clean extraction.

This plan focuses on four specific cycle-breaking changes:

1. Break `bus -> permission`.
2. Break `goal -> tool`.
3. Reduce `error.rs` coupling to plugin/MCP/LSP modules.
4. Introduce seams around `core/daemon.rs` so it can later move without concrete `AgentLoop`, `PermissionChecker`, and `ToolRegistry` dependencies.

## Non-Goals

Do not create `crates/codegg-core` in this pass unless all cycle-breaking work is done and the extraction is trivial.

Do not move the entire agent loop.

Do not redesign the permission system.

Do not redesign the tool registry.

Do not change model-facing tools, permission behavior, daemon protocol shape, session persistence, or CLI behavior.

Do not remove compatibility re-exports for `config`, `provider`, or `protocol`.

Do not convert every error type in the codebase. Only reduce the specific dependency edges blocking extraction.

## Phase 0: Baseline Validation

Before changing code, run:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If there are pre-existing failures, record them in the implementation notes and avoid hiding them behind refactor changes.

Also run these searches and save the results for comparison:

```bash
rg "crate::permission" src/bus src/core src/error.rs
rg "crate::tool" src/goal src/core
rg "crate::agent" src/core src/protocol_conversions.rs
rg "crate::plugin|crate::mcp|crate::lsp" src/error.rs
```

## Phase 1: Break `bus -> permission`

### Current issue

`src/bus/mod.rs` imports `crate::permission::PermissionChoice`. That makes the event bus depend on the permission module. For a future `codegg-core` extraction, `bus` should be a low-level runtime/event primitive, and permission should either depend on bus or communicate through plain DTOs. The dependency direction should not be bus → permission.

### Target design

Move the permission-choice wire/event enum into the bus layer or a neutral shared type. For this pass, prefer moving a small DTO into `src/bus/types.rs` or directly into `src/bus/mod.rs`.

Suggested type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}
```

Use whatever variants match the existing `PermissionChoice` exactly. Do not invent new semantics.

### Implementation steps

1. Inspect `src/permission/**` to confirm the exact `PermissionChoice` shape and serde representation.
2. Add a bus-owned event/DTO type with the same wire representation.
3. Replace `bus` imports of `crate::permission::PermissionChoice` with the bus-owned type.
4. Add conversion impls near the boundary, likely in `permission` or in the call site:

```rust
impl From<crate::permission::PermissionChoice> for crate::bus::PermissionDecision { ... }
impl From<crate::bus::PermissionDecision> for crate::permission::PermissionChoice { ... }
```

5. If conversion impls cause orphan-rule issues, add small helper functions instead.
6. Do not change event payload JSON shape.

### Acceptance criteria

```bash
rg "crate::permission" src/bus
```

must return no matches.

Run:

```bash
cargo check -p codegg
cargo test --workspace
```

## Phase 2: Break `goal -> tool`

### Current issue

`src/goal/tool.rs` implements goal-related tools and imports `crate::tool::Tool`. This makes the low-level goal runtime depend on the tool abstraction, which is backwards for extraction. The goal domain should own goal state/runtime; the tool module should adapt that state into model-facing tools.

### Target design

Move tool-adapter code out of `src/goal/` and into the tool layer.

Preferred target:

```text
src/tool/goal.rs
```

or, if the existing tool module has naming conventions:

```text
src/tool/goal_tools.rs
```

The `goal` module should keep only domain/runtime/storage/budget logic. The `tool` module should own `Tool` trait implementations for goal operations.

### Implementation steps

1. Move `src/goal/tool.rs` to `src/tool/goal.rs` or `src/tool/goal_tools.rs`.
2. Update `src/goal/mod.rs` to stop exporting `tool` if it currently does.
3. Update `src/tool/mod.rs` to declare the new module.
4. Update `ToolRegistry` registration paths for goal tools.
5. Update imports in tests and call sites.
6. Ensure goal runtime functions remain in `src/goal/**` and are called by the new tool adapter.

### Acceptance criteria

```bash
rg "crate::tool" src/goal
```

should return no matches, or only comments/documentation explicitly explaining the boundary.

Run:

```bash
cargo check -p codegg
cargo test --workspace
```

## Phase 3: Reduce `error.rs` Coupling to Plugin/MCP/LSP

### Current issue

`src/error.rs` currently references error types from higher-coupling modules such as plugin, MCP, permission, and LSP. This makes the central error module depend on modules that should move later or stay outside core.

The most problematic edges are:

```text
error.rs -> plugin
error.rs -> mcp
error.rs -> lsp
error.rs -> permission
```

### Target design

`AppError` should avoid importing concrete error types from high-coupling modules unless those modules are guaranteed to move into `codegg-core` with it.

For this pass, prefer one of two approaches:

#### Option A: String-backed variants for high-coupling module errors

Example:

```rust
#[error("plugin error: {0}")]
Plugin(String),

#[error("mcp error: {0}")]
Mcp(String),

#[error("lsp error: {0}")]
Lsp(String),
```

Then implement conversions outside `error.rs`, in the owning module:

```rust
impl From<crate::plugin::PluginError> for crate::error::AppError {
    fn from(err: crate::plugin::PluginError) -> Self {
        crate::error::AppError::Plugin(err.to_string())
    }
}
```

This keeps `error.rs` free of plugin imports.

#### Option B: Boxed dynamic source

Example:

```rust
#[error("plugin error: {0}")]
Plugin(#[source] Box<dyn std::error::Error + Send + Sync>),
```

This preserves more source-chain information but is more annoying to derive/clone/serialize if those traits are needed.

### Recommendation

Use Option A unless the existing error hierarchy relies heavily on source-chain inspection. Codegg is a CLI/TUI tool; stable display text and error categorization are more important than preserving exact concrete source types in `AppError` during this refactor.

### Implementation steps

1. Inspect `src/error.rs` for variants using concrete `crate::plugin`, `crate::mcp`, `crate::lsp`, and `crate::permission` types.
2. Convert plugin/MCP/LSP variants to string-backed variants or local lightweight enums.
3. Move `From<...>` impls for high-coupling error types out of `src/error.rs` and into the owning modules, if Rust coherence permits.
4. If a `From` impl must remain in `error.rs`, document why and mark it as a future extraction blocker.
5. For `permission`, decide whether `PermissionError` should move with core later. If yes, it can remain temporarily. If no, string-back it too.
6. Preserve user-facing error messages as closely as possible.

### Acceptance criteria

```bash
rg "crate::plugin|crate::mcp|crate::lsp" src/error.rs
```

should return no matches.

For permission:

```bash
rg "crate::permission" src/error.rs
```

should either return no matches or have a note explaining that permission is planned to move into `codegg-core` with error handling.

Run:

```bash
cargo check -p codegg
cargo test --workspace
```

## Phase 4: Introduce Core Daemon Seams

### Current issue

`src/core/daemon.rs` depends directly on concrete high-coupling types:

```text
AgentLoop
SubAgentPool
BackgroundScheduler
PermissionChecker
PermissionChoice
ToolBackendConfig
ToolRegistry
TaskTool
```

This is the largest blocker for extracting `src/core/**` into `codegg-core`.

Do not rewrite the daemon. Introduce seams that allow later extraction.

### Target design

The core daemon should depend on narrow traits or factory structs rather than constructing every high-coupling component inline.

There are three dependency groups:

1. Agent execution
2. Permission handling
3. Tool registry construction

This pass should introduce boundaries while preserving current behavior.

### 4.1 Agent execution seam

Add a small trait near the core boundary, for example:

```rust
#[async_trait::async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn run_turn(&self, input: AgentTurnInput) -> Result<AgentTurnOutput, AppError>;
}
```

Do not overdesign the trait. If a fully generic trait is too much for this pass, create a smaller `AgentLoopFactory` or `AgentRuntimeFactory` that hides construction details:

```rust
pub trait AgentRuntimeFactory: Send + Sync {
    fn build_runtime(&self, input: AgentRuntimeInput) -> Result<Box<dyn AgentRuntime>, AppError>;
}
```

The goal is not perfect abstraction. The goal is to move concrete `AgentLoop` construction out of `core/daemon.rs` or behind one narrow function.

Preferred minimum change:

- Extract the existing `AgentLoop` construction block from `core/daemon.rs` into a helper module outside `src/core`, such as `src/agent/runtime_factory.rs`.
- `core/daemon.rs` calls the helper through one narrow function.
- Document that this function will become a trait boundary during `codegg-core` extraction.

This reduces the number of concrete `agent` imports in `core/daemon.rs` even if it does not eliminate all of them.

### 4.2 Permission seam

If `core/daemon.rs` only needs permission decisions as request/response data, use the bus-owned `PermissionDecision` from Phase 1.

If it needs to invoke the permission checker, introduce a trait:

```rust
pub trait PermissionService: Send + Sync {
    fn check(... ) -> PermissionResult;
}
```

Do not move permission logic yet. Just reduce direct construction/import pressure in `core/daemon.rs`.

### 4.3 Tool registry seam

Move default tool registry construction out of `core/daemon.rs` into the `tool` module.

Preferred shape:

```rust
// src/tool/factory.rs
pub struct ToolRegistryFactory;

impl ToolRegistryFactory {
    pub fn for_session(config: &Config, ... ) -> ToolRegistry { ... }
}
```

Then `core/daemon.rs` calls one factory function rather than importing many tool types.

If `ToolBackendConfig` is needed by core only for reporting, use a plain report DTO or keep it behind the factory.

### Acceptance criteria

The following searches should show reduced coupling, even if not zero:

```bash
rg "crate::agent" src/core/daemon.rs src/core/mod.rs
rg "crate::permission" src/core/daemon.rs src/core/mod.rs
rg "crate::tool" src/core/daemon.rs src/core/mod.rs
```

Hard requirement for this pass:

- No new dependencies from Group A modules to TUI/server/plugin modules.
- `core/daemon.rs` should have fewer direct imports from `agent`, `permission`, and `tool` than before.
- Any remaining imports should be documented in `plans/codegg_core_extraction.md`.

Run:

```bash
cargo check -p codegg
cargo test --workspace
```

## Phase 5: Update `plans/codegg_core_extraction.md`

After cycle-breaking work, update the readiness note.

Required updates:

1. Mark `bus -> permission` resolved or explain remaining edge.
2. Mark `goal -> tool` resolved or explain remaining edge.
3. Mark `error.rs -> plugin/mcp/lsp` resolved or explain remaining edge.
4. Update `core/daemon.rs` coupling counts after the seam work.
5. Revise recommended first extraction slice.

The revised first extraction slice should likely be:

```text
src/resilience/**
src/snapshot/**
src/worktree/**
src/session/**
src/storage/**
src/bus/**
src/memory/**
src/model_profile/**
src/task_state/**
src/goal/** excluding any tool adapter
src/error.rs if high-coupling variants are resolved
src/protocol_conversions.rs only for extracted domain types, not agent conversions
```

Keep `src/core/daemon.rs` out of the first extraction slice if it still depends heavily on `agent`, `permission`, and `tool`.

## Phase 6: Optional Cleanup of `protocol_conversions.rs`

This is optional, but useful if touched.

The current conversion helpers use serde round-trips and `expect()`. That is fine as a temporary bridge, but long-term it hides DTO drift until runtime.

If time allows, improve the comment at the top of `src/protocol_conversions.rs`:

```text
These conversions are transitional. They intentionally live outside `codegg-protocol` so the protocol crate stays free of root domain dependencies. Replace serde round-trips with explicit mappings before moving this module into `codegg-core`.
```

Do not rewrite every conversion in this pass unless simple.

## Validation Checklist

Run after each phase:

```bash
cargo check -p codegg
```

Run at the end:

```bash
cargo check -p codegg-protocol
cargo check -p codegg-config
cargo check -p codegg-providers
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If feasible:

```bash
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

Run the cycle checks:

```bash
rg "crate::permission" src/bus
rg "crate::tool" src/goal
rg "crate::plugin|crate::mcp|crate::lsp" src/error.rs
rg "crate::agent" src/core/daemon.rs src/core/mod.rs
rg "crate::permission" src/core/daemon.rs src/core/mod.rs
rg "crate::tool" src/core/daemon.rs src/core/mod.rs
```

Expected:

- `src/bus` has no permission dependency.
- `src/goal` has no tool dependency.
- `src/error.rs` has no plugin/MCP/LSP dependency.
- `src/core` has reduced, documented coupling to agent/permission/tool.

## Suggested Commit Structure

Use small commits:

1. `Decouple bus events from permission module`
2. `Move goal tool adapters into tool layer`
3. `Decouple AppError from plugin mcp lsp concrete errors`
4. `Add daemon factory seams for agent permission and tools`
5. `Update codegg-core extraction readiness note`

## Notes for Implementer

This is a preparatory refactor. Do not chase perfection. The desired output is a root module graph that can be moved into `crates/codegg-core` with fewer cycles.

The most important hard wins are `bus -> permission`, `goal -> tool`, and `error.rs -> plugin/mcp/lsp`. If `core/daemon.rs` cannot be fully decoupled in this pass, reduce the coupling and document exactly what remains.

If an abstraction starts growing large, stop and choose a narrower factory function. Trait extraction should serve the crate split, not become a design project of its own.
