# `codegg-core` Post-Extraction Hardening Plan

## Purpose

The first `codegg-core` extraction is complete enough to be useful: the workspace now contains `crates/codegg-core`, and root `codegg` re-exports moved modules for compatibility. This pass should harden that extraction before any additional broad module moves.

Focus on dependency hygiene, boundary enforcement, documentation, and transitional conversion cleanup. Do not move `agent`, `tool`, `permission`, `mcp`, `tui`, `server`, or `core/daemon.rs` in this pass.

The expected result is a smaller, cleaner, and enforceable `codegg-core` boundary that future work cannot accidentally pollute with UI/server/agent/tool dependencies.

## Current State Summary

`crates/codegg-core` currently owns:

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

Root `src/lib.rs` re-exports the moved modules while keeping high-coupling modules in root.

Root `src/error.rs` now re-exports `codegg_core::error::*` and keeps Axum response glue root-side via `AxumAppError`, avoiding an `axum` dependency in `codegg-core`.

Core protocol conversions were split correctly:

```text
crates/codegg-core/src/protocol_conversions.rs  # core-safe conversions
src/protocol_conversions.rs                     # agent-only conversions + re-export
```

The main remaining concern is dependency bloat and lack of automated boundary checks.

## Non-Goals

Do not move more major modules.

Do not move `src/core/daemon.rs`.

Do not extract TUI, server, tools, permissions, MCP, plugin, search, research, auth, crypto, theme, TTS, or upgrade.

Do not rewrite provider/auth/tool semantics.

Do not remove root compatibility re-exports.

Do not replace all serde round-trip conversions in this pass unless trivial.

Do not add a new linting framework that requires uncommon tooling. Prefer simple scripts/tests that work with the current Rust toolchain and shell.

## Phase 0: Baseline Validation

Run before editing:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If all-features is reasonably fast:

```bash
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

Record any pre-existing failures in the implementation notes.

## Phase 1: Audit `codegg-core` Dependencies

`crates/codegg-core/Cargo.toml` should be pruned to only dependencies used by moved modules.

Current dependencies worth verifying carefully:

```text
base64
reqwest
egglsp
eggsentry
egggit
eggcontext
rand
sha2
similar
walkdir
globset
ignore
parking_lot
dashmap
async-trait
```

Some may be legitimate. Do not guess. Use evidence.

### 1.1 Use `cargo machete` if available

```bash
cargo machete
```

If installed, inspect its output for both root `codegg` and `codegg-core`. Do not blindly apply suggestions; some dependencies may be feature-gated or used in tests.

### 1.2 Manual dependency checks

For each suspicious dependency, search only within `crates/codegg-core`:

```bash
rg "base64|reqwest|egglsp|eggsentry|egggit|eggcontext|rand|sha2|similar|walkdir|globset|ignore|parking_lot|dashmap|async_trait" crates/codegg-core/src crates/codegg-core/tests
```

Also use exact crate import spellings:

```bash
rg "use base64|base64::|reqwest::|egglsp::|eggsentry::|egggit::|eggcontext::|rand::|sha2::|similar::|walkdir::|globset::|ignore::|parking_lot::|dashmap::|async_trait" crates/codegg-core
```

### 1.3 Prune in small batches

Remove only dependencies confirmed unused by `codegg-core`.

After each small batch:

```bash
cargo check -p codegg-core
cargo check -p codegg
```

Likely outcomes:

- `egglsp` probably should not be in `codegg-core` unless a moved module explicitly uses it.
- `eggsentry` probably should not be in `codegg-core` unless core error/tool conversions use it.
- `reqwest` should not be in `codegg-core` unless `AppError::Http(reqwest::Error)` remains in core error taxonomy.
- `base64`, `rand`, and `sha2` may be needed by session/import/security-ish code, but verify.
- `sqlx`, `chrono`, `serde`, `serde_json`, `tokio`, `thiserror`, `uuid`, `tracing`, and `codegg-config` are likely legitimate.

## Phase 2: Audit Root Dependencies After Extraction

Root `Cargo.toml` still carries many dependencies that may now be used only by `codegg-core`.

Candidates to verify:

```text
sqlx
chrono
walkdir
ignore
globset
dashmap
parking_lot
tiktoken
uuid
regex
similar
```

Some are still likely used by root modules, especially `tool`, `search`, `plugin`, `tui`, `mcp`, and `agent`. Verify before removing.

Use:

```bash
rg "sqlx|chrono|walkdir|ignore|globset|dashmap|parking_lot|tiktoken|uuid|regex|similar" src tests
```

If a dependency is only used by moved modules now under `crates/codegg-core`, remove it from root.

After each edit:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
```

Do not remove dependencies still needed for root feature-gated modules unless the feature still passes.

## Phase 3: Add Boundary Checks

Add a simple repo-local boundary check that can run in CI or locally without special dependencies.

Preferred file:

```text
scripts/check-core-boundary.sh
```

Make it executable if possible.

Suggested content:

```bash
#!/usr/bin/env bash
set -euo pipefail

bad_imports=$(rg -n "crate::(agent|tool|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src || true)
if [[ -n "$bad_imports" ]]; then
  echo "codegg-core has forbidden root-domain imports:"
  echo "$bad_imports"
  exit 1
fi

bad_deps=$(rg -n "ratatui|crossterm|ratatui_textarea|axum|tower_http|tokio_tungstenite|wasmtime|wasmtime_wasi" crates/codegg-core Cargo.toml || true)
if [[ -n "$bad_deps" ]]; then
  echo "codegg-core appears to reference forbidden UI/server/plugin dependencies:"
  echo "$bad_deps"
  exit 1
fi

echo "codegg-core boundary check passed"
```

Tune the pattern if the script produces false positives in docs or plans. Prefer checking `crates/codegg-core/src` plus `crates/codegg-core/Cargo.toml`, not the whole repo.

### 3.1 Add cargo alias

Update `.cargo/config.toml`:

```toml
ckcore = "check -p codegg-core"
```

If aliases are already present, add only this alias.

### 3.2 Optional CI hook

If the repo already has GitHub Actions, add the boundary script to the existing Rust check workflow. If there is no workflow, do not create a large CI setup in this pass; document the script usage instead.

## Phase 4: Clean Root Re-exports and Imports

Root `src/lib.rs` currently mixes `pub mod` and `pub use codegg_core::...`. This is acceptable, but make it visually clearer.

Group extracted re-exports together:

```rust
// Extracted core modules re-exported for root compatibility.
pub use codegg_core::{
    bus, goal, memory, model_profile, resilience, session, snapshot, storage, task_state, worktree,
};
```

Keep `pub mod error;` if root `src/error.rs` must provide Axum wrappers and re-export core errors. Do not replace it with `pub use codegg_core::error` unless server wrappers move elsewhere.

Avoid a noisy repo-wide import rewrite. Only adjust touched files or obvious broken imports.

## Phase 5: Review `codegg-core::error` Shape

The first extraction moved common error taxonomy into core. Validate that it does not pull server/TUI/plugin runtime dependencies into `codegg-core`.

Check:

```bash
rg "axum|ratatui|crossterm|wasmtime|tower_http|tokio_tungstenite" crates/codegg-core/src/error.rs crates/codegg-core/Cargo.toml
```

Expected: no matches except possibly comments.

Verify `AppError::Http(reqwest::Error)` is still needed. If only root/server/search uses HTTP errors, consider changing core to a string-backed variant:

```rust
Http(String)
```

and moving `From<reqwest::Error>` to root.

Do this only if it removes `reqwest` from `codegg-core` without causing widespread churn. Otherwise document `reqwest` as intentionally retained for this pass.

## Phase 6: Add Architecture Note

Create or update:

```text
architecture/core.md
```

Keep it concise. Include:

- What `codegg-core` owns now.
- What root still owns.
- What must not be imported by `codegg-core`.
- Why root `src/error.rs` still exists.
- Why protocol conversions are split.
- Next likely extraction target: daemon/agent/tool/permission boundary, not TUI.

Do not duplicate all plan files. This should be a current-state reference for future contributors/agents.

## Phase 7: Improve Protocol Conversion Comments

The conversion helpers still use serde round-trips and `expect()`. For this pass, do not rewrite all conversions unless easy. But make the transitional nature explicit.

Update comments in:

```text
crates/codegg-core/src/protocol_conversions.rs
src/protocol_conversions.rs
```

Document:

- These conversions intentionally live outside `codegg-protocol`.
- `codegg-protocol` must not depend on domain/runtime crates.
- Serde round-trips are a transitional compatibility bridge.
- Prefer explicit `From`/`TryFrom` mappings in a future cleanup pass.

Optional: replace only the simplest conversions with explicit mappings if the structs are small and stable. Do not introduce behavior changes.

## Phase 8: Update Extraction Plan Docs

Update:

```text
plans/codegg_core_extraction.md
```

Record:

- `codegg-core` has been created.
- Modules moved.
- Modules intentionally left root-side.
- Dependency pruning results.
- Boundary script path.
- Remaining blockers for moving `src/core/daemon.rs`.

If `plans/codegg_core_first_extraction.md` has an implementation status section, update it too. Otherwise leave it as a historical handoff plan.

## Acceptance Criteria

This pass is complete when:

1. `cargo check -p codegg-core` passes.
2. `cargo check -p codegg` passes.
3. `cargo check --workspace --all-targets` passes.
4. `cargo test --workspace` passes, or failures are documented as pre-existing.
5. `crates/codegg-core/Cargo.toml` has been audited and unused dependencies removed or documented.
6. Root `Cargo.toml` has been audited and obvious moved-only dependencies removed or documented.
7. A boundary check script exists and fails on forbidden imports/dependencies.
8. `.cargo/config.toml` includes a `ckcore` alias.
9. `architecture/core.md` exists or is updated with current ownership boundaries.
10. `plans/codegg_core_extraction.md` reflects the post-extraction reality.

Boundary check should pass:

```bash
scripts/check-core-boundary.sh
```

Manual forbidden import check should also pass:

```bash
rg "crate::(agent|tool|permission|mcp|plugin|tui|server|client|auth|crypto|search|search_backend|research|theme|tts|upgrade)" crates/codegg-core/src
```

Expected: no matches.

## Suggested Commit Structure

1. `Prune codegg-core dependencies`
2. `Prune root dependencies after core extraction`
3. `Add codegg-core boundary check`
4. `Tidy root core re-exports`
5. `Document codegg-core ownership boundary`
6. `Update core extraction status notes`

## Notes for Implementer

This is not an exciting pass, but it is important. Without dependency pruning and boundary checks, the crate split can slowly collapse as new imports creep into `codegg-core`.

Do not chase perfect minimization if it causes churn. Remove obvious unused dependencies, document intentional ones, and enforce the most important invariant: `codegg-core` must not depend on root high-coupling domains or UI/server crates.
