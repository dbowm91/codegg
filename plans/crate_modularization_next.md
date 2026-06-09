# Crate Modularization Next-Pass Handoff Plan

## Implementation Complete

This pass has been completed. Here is a summary of changes:

### Root protocol replaced by `codegg-protocol`
- `src/lib.rs` now uses `pub use codegg_protocol as protocol;` instead of `pub mod protocol;`
- `src/protocol/` directory has been deleted (mod.rs, core.rs, frames.rs, tui.rs)
- `codegg-protocol` is now the single source of truth for all protocol types

### Protocol type conversions
- Added `src/protocol_conversions.rs` with bidirectional serde-based conversion helpers
- Covers: `session::Session` ↔ `dto::Session`, `message::Message` ↔ `dto::Message`, `agent::Agent` ↔ `dto::Agent`, `provider::Message` ↔ `dto::ProviderMessage`, `config::schema::SessionTemplate` ↔ `dto::SessionTemplate`
- Conversion sites: `src/core/daemon.rs`, `src/tui/app/mod.rs`, `src/tui/mod.rs`

### Dependencies removed from root `Cargo.toml`
- `notify` (moved to `codegg-config`)
- `json5` (moved to `codegg-config`)
- `ulid` (unused)
- `pathdiff` (unused)
- `dunce` (unused)
- `unicode-segmentation` (unused)
- `bytes` (unused, transitive through reqwest/tokio)

### Dependencies intentionally kept
- `dirs` — still used by 18 call sites across src/
- `serde_yaml`, `toml` — still used for themes, plugins, skills frontmatter
- All TUI/server/plugin dependencies — remain until those crates are extracted

### Build aliases
- Created `.cargo/config.toml` with aliases: `ck`, `ckroot`, `ckprotocol`, `ckconfig`, `ckproviders`, `cksplit`

### Documentation updated
- `architecture/protocol.md`, `architecture/core.md`, `architecture/client.md` — stale `src/protocol/` paths updated
- `.opencode/skills/client/SKILL.md` — stale path updated

### codegg-core extraction readiness
- Created `plans/codegg_core_extraction.md` with module classification (Groups A–D), cycle-risk findings, and extraction strategy

### Commands run
```bash
cargo check -p codegg-protocol
cargo check -p codegg-config
cargo check -p codegg-providers
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace  # (pending)
```

## Purpose

Codegg is now in a useful intermediate state: `codegg-config`, `codegg-protocol`, and `codegg-providers` exist as workspace crates, alongside the previously extracted `eggcontext`, `egggit`, `egglsp`, and `eggsentry` crates. This pass should consolidate that work and remove the remaining duplication/coupling that prevents the split from producing reliable compile-time and architectural benefits.

The priority is not yet a full `codegg-core` extraction. The priority is to make the first-stage crate split internally coherent:

1. Make `codegg-protocol` the single source of truth for daemon/frontend protocol types.
2. Remove stale root `src/protocol` duplication.
3. Audit and prune root dependencies that now belong only to extracted crates.
4. Normalize imports away from compatibility re-exports where practical.
5. Prepare, but do not fully execute, the next `codegg-core` extraction.

This should be a cleanup/hardening pass after the initial extraction.

## Current State Summary

The root workspace now includes:

```toml
[workspace]
members = [
    ".",
    "crates/codegg-config",
    "crates/codegg-protocol",
    "crates/codegg-providers",
    "crates/eggsentry",
    "crates/eggcontext",
    "crates/egggit",
    "crates/egglsp",
]
resolver = "2"
```

This is directionally correct.

`codegg-config` appears to own config schema, paths, loading, errors, encryption helpers, and the watcher.

`codegg-providers` appears to own the provider modules, provider auth types, registry, model metadata, provider errors, circuit breaker, streaming/request/message types, and text-tool parsing.

`codegg-protocol` has a cleaned standalone protocol surface that uses crate-local DTOs, such as `crate::dto::Session` and `crate::dto::Message`, instead of root `crate::session` types.

The main remaining problem is that root `src/protocol/*` still exists and `src/lib.rs` still declares:

```rust
pub mod protocol;
```

That means the root crate is still exposing a separate protocol module whose types are coupled to root session internals. This is exactly the kind of duplicated boundary that can drift and make later daemon/frontend work brittle.

The root `Cargo.toml` also still carries many dependencies that may now be owned only by extracted crates. Some will still be used by root modules, but the manifest was not aggressively pruned after extraction. This should be audited before moving into the heavier `codegg-core` phase.

## Non-Goals

Do not extract `codegg-core` in this pass unless protocol cleanup and dependency pruning are already complete and the remaining move is trivial.

Do not extract `codegg-tui` in this pass.

Do not extract `codegg-tools` in this pass.

Do not change model-facing tool names, protocol JSON shape, provider behavior, config file semantics, or CLI behavior.

Do not rewrite provider internals, auth storage, MCP dispatch, or the agent loop.

Do not introduce a compatibility layer that leaves two protocol implementations active.

## Phase 1: Make `codegg-protocol` the Single Source of Truth

### 1.1 Replace the root protocol module with a re-export

In `src/lib.rs`, replace:

```rust
pub mod protocol;
```

with:

```rust
pub use codegg_protocol as protocol;
```

This matches the existing style already used for config and providers:

```rust
pub use codegg_config as config;
pub use codegg_providers as provider;
```

The immediate goal is to keep old `codegg::protocol::...` imports compiling while ensuring they resolve to the extracted crate.

### 1.2 Update direct root imports where feasible

Prefer direct external-crate imports in new or touched code:

```rust
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
```

instead of:

```rust
use codegg::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
```

For root-internal code, either form is acceptable during transition, but direct `codegg_protocol::...` imports make dependency ownership clearer.

### 1.3 Delete or quarantine `src/protocol/*`

After the re-export compiles, remove the old files:

```text
src/protocol/mod.rs
src/protocol/core.rs
src/protocol/frames.rs
src/protocol/tui.rs
```

If some old root-only helper has not been moved into `crates/codegg-protocol`, move it into the protocol crate first. Do not leave `src/protocol` as a second implementation.

### 1.4 Confirm DTO coverage

The extracted `codegg-protocol` crate must contain all protocol types previously exposed by `src/protocol`:

```text
core request/response envelopes
core events
frontend/TUI protocol types
frame types
session/message DTOs needed by protocol responses
```

Where old protocol types referred to `crate::session::Session` or `crate::session::message::Message`, the new crate should use protocol DTOs and conversion helpers should live on the root/core side.

Expected pattern:

```rust
// in codegg-protocol
pub struct Session { ... }
pub struct Message { ... }

// in root or future codegg-core
impl From<crate::session::Session> for codegg_protocol::dto::Session { ... }
impl From<crate::session::message::Message> for codegg_protocol::dto::Message { ... }
```

Do not make `codegg-protocol` depend on root `codegg` or future `codegg-core`.

### Validation

Run:

```bash
cargo check -p codegg-protocol
cargo check -p codegg
cargo test --workspace
```

Also search for stale root protocol references:

```bash
rg "crate::protocol|pub mod protocol|src/protocol|codegg::protocol" src crates tests
```

Acceptable results:

- `pub use codegg_protocol as protocol;`
- direct imports from `codegg_protocol`
- possibly public API references through `codegg::protocol` from external-facing tests

Unacceptable results:

- `src/protocol` still compiling as a root module
- extracted `codegg-protocol` depending on root session/core/provider types

## Phase 2: Dependency Pruning Audit

The root manifest still contains many dependencies that may have moved into `codegg-config` or `codegg-providers`. Audit them systematically. Do not remove dependencies blindly; use compiler feedback and search.

### 2.1 Generate dependency evidence

Run:

```bash
cargo tree -p codegg --depth 1 > target/codegg-root-deps-after-first-split.txt
cargo tree -p codegg-config --depth 1 > target/codegg-config-deps.txt
cargo tree -p codegg-protocol --depth 1 > target/codegg-protocol-deps.txt
cargo tree -p codegg-providers --depth 1 > target/codegg-providers-deps.txt
```

Use `cargo machete` if available:

```bash
cargo machete
```

If `cargo machete` is not installed, use `rg` and `cargo check` manually.

### 2.2 Audit likely config-only dependencies

Check whether the root crate still directly needs these:

```text
json5
serde_yaml
toml
notify
dirs
```

These may now belong only to `codegg-config`, except `dirs` may still be used by other root modules.

Search examples:

```bash
rg "json5|serde_yaml|toml::|notify::|dirs::" src tests
```

If there are no root uses, remove the dependency from root `Cargo.toml`.

### 2.3 Audit likely provider-only dependencies

Check whether the root crate still directly needs these after provider extraction:

```text
reqwest
bytes
tokio-stream
futures
async-trait
url
chrono
hex
rand
dashmap
aes-gcm
argon2
hmac
sha2
```

Some of these may still be used outside providers: auth/crypto, storage, MCP, IDs, search backends, or plugins may legitimately need them. Remove only what is unused by root.

Search examples:

```bash
rg "reqwest|bytes::|tokio_stream|futures::|async_trait|url::|chrono::|hex::|rand::|dashmap|aes_gcm|argon2|hmac|sha2" src tests
```

### 2.4 Audit protocol-only dependencies

`codegg-protocol` should stay small. It currently should need only:

```text
serde
serde_json
```

If protocol DTOs grow to require `chrono`, `uuid`, or `ulid`, make the dependency explicit there. Do not smuggle root types into protocol to avoid adding a small dependency.

### 2.5 Keep root dependencies if root still owns the module

Do not remove these simply because they are heavy:

```text
ratatui
crossterm
ratatui-textarea
syntect
comrak
sqlx
grep / grep-* crates
wasmtime / wasmtime-wasi when plugins feature is enabled
axum / tower-http / tokio-tungstenite when server feature is enabled
ratatui-image / image when image feature is enabled
```

They still likely belong to root until `codegg-tui`, `codegg-core`, `codegg-server`, and possibly `codegg-tools` are extracted.

### Validation

After every small manifest removal batch:

```bash
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If a dependency is used only by tests, move it to `[dev-dependencies]` instead of keeping it as a normal dependency.

## Phase 3: Normalize Extracted-Crate Imports

The root library now has compatibility re-exports for config/provider and should have one for protocol after Phase 1. That is acceptable for public compatibility, but internal ownership should become explicit over time.

### 3.1 Prefer direct imports for extracted crates

In touched root files, migrate from:

```rust
use crate::config::schema::Config;
use crate::provider::{ChatRequest, ProviderRegistry};
use crate::protocol::core::CoreRequest;
```

to:

```rust
use codegg_config::schema::Config;
use codegg_providers::{ChatRequest, ProviderRegistry};
use codegg_protocol::core::CoreRequest;
```

Do this only when a file is already being touched or when the migration is easy and mechanical. Avoid a huge noisy import-only diff if it will obscure behavioral regressions.

### 3.2 Keep compatibility re-exports for now

Keep these in root `src/lib.rs` for downstream compatibility and lower migration risk:

```rust
pub use codegg_config as config;
pub use codegg_providers as provider;
pub use codegg_protocol as protocol;
```

They can be revisited only after `codegg-core` and `codegg-tui` are extracted.

### Validation

Run:

```bash
cargo check --workspace --all-targets
cargo test --workspace
```

## Phase 4: Prepare `codegg-core` Extraction Without Moving It Yet

This phase should produce a small readiness note or documentation update, not necessarily move code.

Create or update:

```text
plans/codegg_core_extraction.md
```

or add a section to this plan's implementation notes.

The readiness note should classify root modules into one of four groups.

### Group A: Likely core-first modules

These are candidates for the first `codegg-core` extraction slice:

```text
src/core/**
src/session/**
src/storage/**
src/bus/**
src/error.rs
src/exec/**
src/memory/**
src/goal/**
src/task_state/**
src/snapshot/**
src/resilience/**
src/worktree/**
src/util/**
```

Reason: these are runtime/session/state modules that should eventually be usable by daemon, TUI, CLI, and tests without depending on terminal rendering.

### Group B: Core but high-coupling; move after Group A

```text
src/agent/**
src/permission/**
src/mcp/**
src/hooks/**
src/ide/**
src/lsp/**
src/shell_session/**
src/skills/**
```

Reason: these are core-domain modules but are coupled to providers, tools, plugins, permissions, MCP, LSP, or session state. They may move into `codegg-core`, but they should follow the lower-risk state/session/core modules.

### Group C: Keep root or later frontend/server/tool crates

```text
src/tui/**
src/server/**
src/client/**
src/tool/**
src/search/**
src/search_backend/**
src/research/**
src/security/**
src/theme/**
src/tts/**
src/plugin/**
src/upgrade/**
src/auth/**
src/crypto/**
```

Reason: these either have heavy UI/server/tool/plugin/provider/auth dependencies, need separate design, or require more careful cycle-breaking.

Some of these may eventually move, but not in the first `codegg-core` slice.

### Group D: Already extracted or wrapper-only

```text
crates/codegg-config
crates/codegg-protocol
crates/codegg-providers
crates/eggcontext
crates/egggit
crates/egglsp
crates/eggsentry
src/lsp/**             # wrapper around egglsp; classify carefully
src/security/**        # wrapper around eggsentry plus Codegg-side policy; classify carefully
```

### 4.1 Search for TUI dependencies in core candidates

Before moving Group A modules, check whether they import terminal/UI crates:

```bash
rg "ratatui|crossterm|ratatui_textarea|tui::" src/core src/session src/storage src/bus src/exec src/memory src/goal src/task_state src/snapshot src/resilience src/worktree src/util
```

If they do, split UI-specific code out before moving.

### 4.2 Search for provider/tool cycles in core candidates

```bash
rg "crate::provider|crate::tool|crate::tui|crate::server|crate::plugin|crate::mcp" src/core src/session src/storage src/bus src/exec src/memory src/goal src/task_state src/snapshot src/resilience src/worktree src/util
```

Document the results. The goal is to know whether Group A can move cleanly in the next pass.

## Phase 5: Add Lightweight Build Aliases

If `.cargo/config.toml` exists, update it. Otherwise create it.

Suggested aliases:

```toml
[alias]
ck = "check --workspace --all-targets"
ckroot = "check -p codegg"
ckprotocol = "check -p codegg-protocol"
ckconfig = "check -p codegg-config"
ckproviders = "check -p codegg-providers"
cksplit = "check -p codegg-protocol -p codegg-config -p codegg-providers -p codegg"
```

Keep aliases boring and local-dev oriented. Do not add aliases that require nightly or optional tools.

## Phase 6: Documentation Update

Update `plans/crate_modularization.md` or add an implementation note at the top of `plans/crate_modularization_next.md` after the pass is done.

Record:

```text
- whether root protocol was fully replaced by codegg-protocol
- whether src/protocol was deleted
- dependencies removed from root Cargo.toml
- dependencies intentionally kept and why
- any cycles or blockers found for codegg-core extraction
- exact cargo check/test commands run
```

This matters because the next implementation pass will otherwise have to rediscover the same dependency/cycle information.

## Acceptance Criteria

This pass is complete when all of the following are true:

1. `src/lib.rs` re-exports `codegg_protocol` as `protocol`.
2. Root `src/protocol/*` is deleted, or there is a documented reason for a temporary file to remain.
3. `codegg-protocol` is the only implementation of protocol DTOs and envelopes.
4. `codegg-protocol` does not depend on root `codegg`, `codegg-core`, `codegg-providers`, or `codegg-tui`.
5. Root `Cargo.toml` has been audited and unused dependencies introduced by the old config/provider/protocol ownership have been removed or moved to dev-dependencies.
6. Compatibility re-exports remain for `config`, `provider`, and `protocol`.
7. Touched imports prefer direct extracted-crate paths, but no broad noisy import-only rewrite is required.
8. A readiness note exists for the future `codegg-core` extraction, including module grouping and cycle-risk findings.
9. These commands pass:

```bash
cargo check -p codegg-protocol
cargo check -p codegg-config
cargo check -p codegg-providers
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
```

If all-features is feasible, also run:

```bash
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

## Suggested Commit Structure

Use small commits if possible:

1. `Use codegg-protocol as root protocol export`
2. `Remove stale root protocol module`
3. `Prune root dependencies after crate extraction`
4. `Document codegg-core extraction readiness`
5. `Add cargo check aliases for split crates`

## Notes for the Implementer

Be conservative. This pass is about making the first extraction correct and stable, not about maximizing the number of moved files.

The largest hidden risk is protocol drift: if `src/protocol` and `crates/codegg-protocol` both remain active, future server/TUI/daemon work can silently use different types. Remove that duplication first.

The second-largest risk is manifest bloat: if root `codegg` still declares all heavy dependencies, the workspace split will look good structurally but will not help much during local iteration. Prune only what is truly unused, but actually do the audit.

Do not touch the agent loop unless a compiler error requires a trivial import adjustment. The agent/core split should be a separate pass after the root protocol and dependency boundary is clean.
