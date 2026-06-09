# First `codegg-core` Extraction Handoff Plan

## Status: Complete (hardening done 2026-06-09)

This plan was the initial handoff for `codegg-core` extraction. The extraction and subsequent hardening are complete. See `plans/codegg_core_extraction.md` for the final state.

## Purpose

The repository has completed the preparatory modularization work needed for a cautious first `codegg-core` extraction:

- `codegg-config`, `codegg-protocol`, and `codegg-providers` are extracted workspace crates.
- Root `codegg::protocol` now re-exports `codegg_protocol`.
- `bus -> permission` has been broken by making `PermissionDecision` bus-owned.
- `goal -> tool` has been broken by moving goal tool adapters into `src/tool/goal.rs`.
- `core/daemon.rs` has reduced direct permission/tool coupling through factory seams.

This pass should create `crates/codegg-core` and move only the low-risk, mostly self-contained Group A modules. Do not move `src/core/daemon.rs`, `src/agent`, `src/tool`, `src/permission`, `src/mcp`, `src/tui`, `src/server`, or other high-coupling modules in this pass.

The goal is a compiling workspace where root `codegg` depends on `codegg-core`, and root re-exports moved modules for compatibility. This should make the next extraction pass mostly about daemon/agent/tool boundary work rather than basic runtime/session state.

## Non-Goals

Do not move the agent loop.

Do not move the TUI.

Do not move server/client code.

Do not move tool registry or tools, except for import updates required by moved core modules.

Do not move permission checking.

Do not move MCP, LSP wrappers, plugin, auth, crypto, theme, TTS, research, search, or upgrade modules.

Do not redesign `AppError` beyond what is necessary to compile the extracted crate.

Do not remove root compatibility re-exports yet.

Do not attempt to optimize dependencies perfectly during the first extraction. Compile correctness and clean dependency direction matter more.

## Target Workspace Shape

Add a new workspace member:

```text
crates/codegg-core/
  Cargo.toml
  src/lib.rs
  src/error.rs
  src/resilience/**
  src/snapshot/**
  src/worktree/**
  src/session/**
  src/storage/**
  src/bus/**
  src/memory/**
  src/model_profile/**
  src/task_state/**
  src/goal/**
```

Root `Cargo.toml` should include:

```toml
[workspace]
members = [
    ".",
    "crates/codegg-core",
    "crates/codegg-config",
    "crates/codegg-protocol",
    "crates/codegg-providers",
    "crates/eggsentry",
    "crates/eggcontext",
    "crates/egggit",
    "crates/egglsp",
]
```

Root `codegg` should depend on:

```toml
codegg-core = { path = "crates/codegg-core" }
```

Root `src/lib.rs` should temporarily re-export moved modules so existing code keeps compiling:

```rust
pub use codegg_core::bus;
pub use codegg_core::error;
pub use codegg_core::goal;
pub use codegg_core::memory;
pub use codegg_core::model_profile;
pub use codegg_core::resilience;
pub use codegg_core::session;
pub use codegg_core::snapshot;
pub use codegg_core::storage;
pub use codegg_core::task_state;
pub use codegg_core::worktree;
```

Use explicit re-exports, not `pub use codegg_core::*`, so ownership stays visible.

## Phase 0: Baseline

Before moving files, run:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

Also run:

```bash
rg "crate::tool|crate::agent|crate::permission|crate::mcp|crate::plugin|crate::tui|crate::server" \
  src/resilience src/snapshot src/worktree src/session src/storage src/bus src/memory src/model_profile src/task_state src/goal src/error.rs
```

Record any matches in the implementation notes. Known/acceptable issues:

- `error.rs` contains `McpError`, `LspError`, `PluginError` enums as central error taxonomy.
- `task_state` may depend on `model_profile`.
- Some moved modules may still reference `crate::config` or `crate::provider`; these should become `codegg_config` and `codegg_providers` imports.

## Phase 1: Create `crates/codegg-core`

Create:

```text
crates/codegg-core/Cargo.toml
crates/codegg-core/src/lib.rs
```

Initial `Cargo.toml` should be conservative. Start with dependencies known to be used by Group A modules:

```toml
[package]
name = "codegg-core"
version = "0.1.0"
edition = "2021"
description = "Core runtime, session, storage, state, and domain types for codegg"
license = "MIT"

[dependencies]
codegg-config = { path = "../codegg-config" }
codegg-protocol = { path = "../codegg-protocol" }
codegg-providers = { path = "../codegg-providers" }
eggcontext = { path = "../eggcontext" }
egggit = { path = "../egggit" }
eggsentry = { path = "../eggsentry" }

anyhow = "1"
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }
dashmap = "5"
globset = "0.4"
ignore = "0.4"
once_cell = "1"
parking_lot = "0.12"
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate", "chrono", "json"] }
thiserror = "2"
tiktoken = "3.1"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "sync", "time", "fs", "io-util", "process"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
walkdir = "2"
```

Do not obsess over this dependency list on the first commit. Use compiler feedback to remove/add.

Initial `src/lib.rs`:

```rust
#![deny(unsafe_code)]

pub mod bus;
pub mod error;
pub mod goal;
pub mod memory;
pub mod model_profile;
pub mod resilience;
pub mod session;
pub mod snapshot;
pub mod storage;
pub mod task_state;
pub mod worktree;
```

Optionally include `pub mod protocol_conversions;` later only after deciding how to handle agent conversions.

## Phase 2: Move Clean Leaf Modules First

Move the lowest-risk modules first, one module at a time:

```text
src/resilience/** -> crates/codegg-core/src/resilience/**
src/snapshot/**   -> crates/codegg-core/src/snapshot/**
src/worktree/**   -> crates/codegg-core/src/worktree/**
```

Update imports inside the moved modules:

```rust
crate::error -> crate::error
crate::config -> codegg_config
crate::provider -> codegg_providers
```

In root `src/lib.rs`, replace `pub mod resilience;`, `pub mod snapshot;`, and `pub mod worktree;` with:

```rust
pub use codegg_core::resilience;
pub use codegg_core::snapshot;
pub use codegg_core::worktree;
```

Run:

```bash
cargo check -p codegg-core
cargo check -p codegg
```

Fix only compile errors directly related to the moved modules.

## Phase 3: Move `error.rs` Carefully

Move:

```text
src/error.rs -> crates/codegg-core/src/error.rs
```

Root `src/lib.rs` should re-export:

```rust
pub use codegg_core::error;
```

### Important error guidance

`error.rs` currently centralizes many error enums, including `AppError`, `ToolError`, `PermissionError`, `McpError`, `LspError`, `PluginError`, `ServerRuntimeError`, and `ClientError`.

This is acceptable for this first extraction only if these enums remain lightweight and do not import root-only modules. The current state is probably acceptable because the plugin/MCP/LSP types appear to be enums defined in `error.rs`, not concrete imports from `src/plugin`, `src/mcp`, or `src/lsp`.

However, if moving `error.rs` pulls in root-only server/TUI dependencies, split server/client response glue out before moving:

- Keep `IntoResponse for AppError` in root or server module if it forces `axum` into `codegg-core`.
- Keep `IntoResponse for ServerRuntimeError` in root/server if it forces server dependencies into `codegg-core`.
- `codegg-core` should not depend on `axum` in this pass.

Preferred fix if `axum` is the only issue:

1. Move pure error enums to `codegg-core/src/error.rs`.
2. Create root/server-side HTTP mapping helper, for example `src/server/error_response.rs`, that implements `IntoResponse` for root-visible errors if Rust orphan rules allow it. If orphan rules block direct impls, provide a wrapper type:

```rust
pub struct HttpAppError(pub codegg_core::error::AppError);
```

3. Keep behavior equivalent.

Run:

```bash
cargo check -p codegg-core
cargo check -p codegg --features server
cargo test --workspace
```

## Phase 4: Move Session and Storage

Move:

```text
src/session/** -> crates/codegg-core/src/session/**
src/storage/** -> crates/codegg-core/src/storage/**
```

Update imports:

```rust
crate::error -> crate::error
crate::config -> codegg_config
crate::provider -> codegg_providers
crate::protocol -> codegg_protocol
```

Root `src/lib.rs`:

```rust
pub use codegg_core::session;
pub use codegg_core::storage;
```

Expected root code changes:

- Existing `use codegg::session::...` should continue to work through re-export.
- Internal root modules may use `crate::session`; that should still work if the root re-export is named `session`.
- If ambiguity occurs, import from `codegg_core::session` directly in touched files.

Run:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo test --workspace
```

## Phase 5: Move Bus

Move:

```text
src/bus/** -> crates/codegg-core/src/bus/**
```

Root `src/lib.rs`:

```rust
pub use codegg_core::bus;
```

Important checks:

```bash
rg "crate::permission" crates/codegg-core/src/bus
```

must return no matches.

Root modules that use `crate::bus::PermissionRegistry` or `crate::bus::PermissionDecision` should keep compiling through the re-export. If not, update imports to `codegg_core::bus::...`.

Run:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo test --workspace
```

## Phase 6: Move Model Profile, Task State, Memory, Goal

Move in this order:

```text
src/model_profile/** -> crates/codegg-core/src/model_profile/**
src/task_state/**    -> crates/codegg-core/src/task_state/**
src/memory/**        -> crates/codegg-core/src/memory/**
src/goal/**          -> crates/codegg-core/src/goal/**
```

Root `src/lib.rs`:

```rust
pub use codegg_core::model_profile;
pub use codegg_core::task_state;
pub use codegg_core::memory;
pub use codegg_core::goal;
```

Important boundary checks:

```bash
rg "crate::tool" crates/codegg-core/src/goal
rg "crate::agent|crate::tool|crate::permission|crate::mcp|crate::plugin|crate::tui|crate::server" \
  crates/codegg-core/src/model_profile crates/codegg-core/src/task_state crates/codegg-core/src/memory crates/codegg-core/src/goal
```

Acceptable dependencies:

```text
codegg_config
codegg_providers
codegg_protocol
crate::session
crate::storage
crate::bus
crate::error
```

Unacceptable dependencies:

```text
root codegg crate
crate::agent
crate::tool
crate::permission
crate::tui
crate::server
```

Run:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo test --workspace
```

## Phase 7: Decide on `protocol_conversions.rs`

Do not blindly move `src/protocol_conversions.rs`.

Split it into two groups:

### Safe to move with core

Conversions involving extracted core-owned types:

```text
Session <-> codegg_protocol::dto::Session
Message <-> codegg_protocol::dto::Message
ProviderMessage <-> codegg_protocol::dto::ProviderMessage only if provider message type is in codegg_providers
SessionTemplate <-> codegg_protocol::dto::SessionTemplate if config type is in codegg_config
```

### Keep root-side for now

Conversions involving root-only or high-coupling types:

```text
Agent <-> codegg_protocol::dto::Agent
anything requiring src/agent/**
anything requiring src/tool/**
anything requiring src/tui/**
```

Preferred result:

```text
crates/codegg-core/src/protocol_conversions.rs  # core-safe conversions
src/protocol_conversions.rs                     # remaining root/high-coupling conversions
```

Root `src/lib.rs` should not re-export two conflicting modules. If split, use clear names:

```rust
pub use codegg_core::protocol_conversions as core_protocol_conversions;
pub mod protocol_conversions; // root-only high-coupling conversions
```

Alternatively, keep all conversions in root for this pass. That is acceptable if moving it becomes messy.

## Phase 8: Root Manifest Pruning

After moved modules compile, prune dependencies from root only if clearly unused there and now owned by `codegg-core`.

Likely candidates after extraction:

```text
chrono
sqlx
uuid
walkdir
ignore
tiktoken
dashmap
parking_lot
```

Do not remove any dependency still used by root modules such as TUI, tools, plugins, server, search, or auth.

Use:

```bash
cargo machete
```

if available. Otherwise use `rg`:

```bash
rg "chrono|sqlx|uuid|walkdir|ignore|tiktoken|dashmap|parking_lot" src tests
```

Run after each manifest edit:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
```

## Phase 9: Update Documentation

Update `plans/codegg_core_extraction.md` after the extraction.

Record:

- Which modules moved into `crates/codegg-core`.
- Which modules stayed root-side and why.
- Remaining `core/daemon.rs` coupling.
- Whether `error.rs` moved fully or was split.
- Whether `protocol_conversions.rs` moved, split, or stayed root-side.
- Dependencies removed from root `Cargo.toml`.
- Validation commands run.

Add a short architecture note if useful:

```text
architecture/core.md
```

Do not write a long theoretical document. Keep it factual and tied to current module ownership.

## Acceptance Criteria

The pass is complete when:

1. `crates/codegg-core` exists and is a workspace member.
2. At minimum, these modules are moved and compile from `codegg-core`:

```text
resilience
snapshot
worktree
session
storage
bus
model_profile
memory
```

3. Preferably, these also move if compile-safe:

```text
task_state
goal
error
```

4. Root `src/lib.rs` re-exports moved modules explicitly.
5. `codegg-core` does not depend on root `codegg`.
6. `codegg-core` does not depend on TUI/server crates such as `ratatui`, `crossterm`, `axum`, or `tower-http`.
7. `codegg-core` does not import root high-coupling modules:

```bash
rg "crate::agent|crate::tool|crate::permission|crate::mcp|crate::plugin|crate::tui|crate::server" crates/codegg-core/src
```

Known exceptions must be documented. Prefer zero exceptions for this first pass.

8. Existing public root imports keep working through re-exports.
9. These commands pass:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If practical:

```bash
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

## Suggested Commit Structure

Use small commits:

1. `Create codegg-core crate`
2. `Move leaf runtime modules into codegg-core`
3. `Move session storage and bus into codegg-core`
4. `Move model profile memory task state and goal into codegg-core`
5. `Move or split core error types`
6. `Update root re-exports and imports`
7. `Prune root dependencies after core extraction`
8. `Update codegg-core extraction documentation`

## Notes for Implementer

Keep this extraction boring. The desired output is a compiling crate boundary, not a redesigned runtime.

If a module pulls in `agent`, `tool`, `permission`, `mcp`, `plugin`, `tui`, or `server`, stop and either leave that module in root for this pass or split the offending adapter into root.

Do not move `src/core/daemon.rs` in this pass. Despite the factory seams, it still stores concrete agent runtime types and should be handled after a trait/factory boundary is cleaner.

The safest successful result is a smaller `codegg-core` containing state, storage, session, bus, goal runtime, memory, model-profile, and common errors, while root still orchestrates agent/tool/daemon integration.
