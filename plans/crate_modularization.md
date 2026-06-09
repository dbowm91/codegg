# Crate Modularization Handoff Plan

## Purpose

Codegg has already started moving in the right direction: the repository is a Cargo workspace and durable tool domains have begun moving into crates under `crates/` (`eggsentry`, `eggcontext`, `egggit`, `egglsp`). The next step is not a broad rewrite. The next step is to reduce root-crate rebuild scope by extracting the largest stable architectural seams into a small number of workspace crates while preserving behavior.

The immediate goal is faster local iteration and cleaner ownership boundaries. The longer-term goal is to make the daemon/frontend architecture easier to evolve: a thin CLI/TUI binary should sit on top of a core crate, protocol crate, provider crate, tool crate, config crate, and optional frontend/server crates.

## Current State Summary

The root `Cargo.toml` is both a workspace manifest and the main `codegg` package. It currently declares the root package plus these workspace members:

```toml
[workspace]
members = [".", "crates/eggsentry", "crates/eggcontext", "crates/egggit", "crates/egglsp"]
resolver = "2"
```

The root package still owns a very wide dependency surface: tokio, clap, ratatui, crossterm, reqwest, sqlx, grep, syntect, tiktoken, json/toml/yaml config parsing, auth crypto, notification support, server dependencies behind `server`, plugin dependencies behind `plugins`, image dependencies behind `image`, and the extracted tool crates.

The root `src/lib.rs` exports most system domains directly:

```text
agent, auth, crypto, exec, hooks, ide, memory, model_profile, security, tts,
bus, client, command, config, core, error, goal, lsp, mcp, permission,
plugin, protocol, provider, research, resilience, search, search_backend,
server, session, shell_session, skills, snapshot, storage, task_state,
theme, tool, tui, upgrade, util, worktree
```

The CLI binary imports nearly every major root module directly, including `agent`, `auth`, `config`, `core`, `exec`, `mcp`, `memory`, `protocol`, `provider`, `session`, `skills`, `storage`, `tui`, and `upgrade`.

The agent architecture is the highest-coupling area. `AgentLoop` currently owns orchestration, provider calls, permission checks, tool execution, context tracking, plugin hooks, MCP dispatch, model routing, snapshots, usage/pricing, security, todo state, event store, execution policy, subagent pool, and goal accounting. This makes it a natural resident of a future `codegg-core` crate, but it should not be split further until core-facing APIs are stabilized.

The tool architecture already has a useful crate-first precedent. Codegg-owned durable tool domains can live in workspace crates and be called directly in-process, with optional MCP adapters later. The plan below extends that pattern to the root architecture.

## Non-Goals

Do not split every module into its own crate.

Do not move every tool into an independent crate.

Do not change model-facing tool names.

Do not route hot-path internal operations through MCP subprocesses for the sake of modularity.

Do not attempt to rewrite the agent loop during this pass.

Do not move all CLI command handling at once. Keep the first pass behavior-preserving.

Do not extract `agent` into many small crates yet. The agent loop is too central and high-coupling; move it as part of `codegg-core`, then simplify internally in later passes.

## Desired End State

Target a conservative workspace shape:

```text
Cargo.toml
crates/
  codegg-core/          agent/session/orchestration/domain logic
  codegg-protocol/      daemon/frontend DTOs, envelopes, stream events
  codegg-config/        config schema, paths, loading, validation, defaults
  codegg-providers/     provider registry, provider traits, model catalog, streaming normalization
  codegg-tools/         Tool trait, registry, wrappers, permission-facing metadata
  codegg-tui/           ratatui/crossterm frontend
  codegg-server/        optional HTTP/WebSocket server mode, gated from default builds if practical
  eggsentry/
  eggcontext/
  egggit/
  egglsp/
src/
  main.rs               thin CLI binary
  lib.rs                temporary compatibility re-exports only, then shrink/remove later
```

This pass should probably implement only the first 3-5 crates, not the entire end state. The highest-value first-pass crates are:

```text
codegg-protocol
codegg-config
codegg-providers
codegg-core
codegg-tui
```

`codegg-tools` may be extracted in the same pass only if it does not cause excessive churn. Otherwise leave `tool` inside `codegg-core` temporarily and extract tools in a follow-up pass.

## Ordering Principle

Extract low-dependency crates before high-dependency crates.

The safest order is:

1. `codegg-protocol`
2. `codegg-config`
3. `codegg-providers`
4. `codegg-core`
5. `codegg-tui`
6. optional: `codegg-server`
7. optional follow-up: `codegg-tools`

This order avoids circular dependencies and gives the implementation model stable targets to move imports toward.

## Phase 0: Baseline and Dependency Measurement

Before moving code, capture current build behavior.

Run:

```bash
cargo check --workspace --all-features
cargo test --workspace --all-features
cargo check -p codegg
cargo tree -p codegg --depth 1 > target/codegg-root-deps-before.txt
cargo tree -p codegg --duplicates > target/codegg-duplicates-before.txt || true
```

If nightly is available, also run:

```bash
cargo +nightly build -Z timings --workspace
```

Record the baseline in a temporary note or in the PR description. Do not optimize from intuition alone; use this baseline to check whether the split actually reduces rebuild scope.

## Phase 1: Extract `codegg-protocol`

Create:

```text
crates/codegg-protocol/Cargo.toml
crates/codegg-protocol/src/lib.rs
```

Move or copy first, then delete after imports are corrected:

```text
src/protocol/** -> crates/codegg-protocol/src/**
```

The protocol crate should contain only serialization-safe types used between core, TUI, CLI, server, and future frontends. Keep it narrow:

```text
CoreRequest
CoreResponse
RequestEnvelope
session/event DTOs used by daemon/frontend boundaries
stream/event envelopes
serializable command/result structs that are not provider-specific
```

Expected dependencies:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"        # only if existing protocol errors need it
chrono = { version = "0.4", features = ["serde"] } # only if existing DTOs use chrono
uuid = { version = "1", features = ["serde"] }     # only if existing DTOs use uuid
```

Rules:

- `codegg-protocol` must not depend on `codegg-core`, `codegg-config`, `codegg-providers`, `codegg-tui`, `ratatui`, `reqwest`, `sqlx`, or `tokio` unless absolutely unavoidable.
- Prefer plain DTOs over types that pull in runtime dependencies.
- If a protocol type currently depends on a root domain type, duplicate or translate the DTO rather than depending backward into the root crate.

Update imports:

```rust
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
```

Temporary compatibility re-export in root `src/lib.rs` is acceptable:

```rust
pub use codegg_protocol as protocol;
```

But only keep this during transition. New code should import `codegg_protocol` directly.

Validation:

```bash
cargo check -p codegg-protocol
cargo check -p codegg
cargo test --workspace
```

## Phase 2: Extract `codegg-config`

Create:

```text
crates/codegg-config/Cargo.toml
crates/codegg-config/src/lib.rs
```

Move:

```text
src/config/** -> crates/codegg-config/src/**
```

The config crate should own:

```text
config schema
paths
loading/parsing/defaults
validation
watching, only if the watcher does not create dependency cycles
migration/defaulting helpers
```

Expected dependencies:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
toml = "0.8"
json5 = "0.4"
thiserror = "2"
notify = "7"           # if watcher stays here
url = "2"              # only if config schema currently uses it
semver = "1"           # only if config schema currently uses it
dirs = "6"             # if path discovery stays here
```

Rules:

- `codegg-config` may depend on `codegg-protocol` only if there is a real DTO need. Prefer no dependency.
- It must not depend on `codegg-core`, `codegg-tui`, `codegg-providers`, `codegg-tools`, or root `codegg`.
- Config schema types should remain data-only. Do not move provider runtime clients here.
- Keep conversion boundaries explicit: config schema can describe provider/tool/search settings, but runtime registries should live elsewhere.

Root compatibility re-export:

```rust
pub use codegg_config as config;
```

Update imports gradually from `crate::config::...` to `codegg_config::...` where practical.

Validation:

```bash
cargo check -p codegg-config
cargo check -p codegg
cargo test --workspace
```

## Phase 3: Extract `codegg-providers`

Create:

```text
crates/codegg-providers/Cargo.toml
crates/codegg-providers/src/lib.rs
```

Move:

```text
src/provider/** -> crates/codegg-providers/src/**
src/model_profile.rs -> crates/codegg-providers/src/model_profile.rs   # if tightly provider-related
```

The provider crate should own:

```text
Provider trait
ProviderRegistry
provider-specific clients
model catalog and model metadata
streaming response normalization
usage/pricing metadata if currently provider-bound
provider-specific error types
```

Expected dependencies:

```toml
async-trait = "0.1"
bytes = "1"
futures = "0.3"
reqwest = { version = "0.12", default-features = false, features = ["stream", "json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["rt", "macros", "sync", "time"] }
tokio-stream = { version = "0.1", features = ["sync"] }
tracing = "0.1"
url = "2"
```

Possible dependencies:

```toml
codegg-config = { path = "../codegg-config" }
codegg-protocol = { path = "../codegg-protocol" }
```

Rules:

- `codegg-providers` must not depend on `codegg-core` or `codegg-tui`.
- Provider streaming events should be provider-domain events, not TUI events.
- If provider code currently imports `Message` from `session`, decide whether message types belong in `codegg-core`, `codegg-protocol`, or a small `codegg-types` crate. Prefer `codegg-core` for now unless the cycle forces a small shared crate.
- Do not mix provider registry extraction with model-routing policy rewrites. Move first; improve later.

Root compatibility re-export:

```rust
pub use codegg_providers as provider;
```

Validation:

```bash
cargo check -p codegg-providers
cargo check -p codegg
cargo test --workspace
```

## Phase 4: Extract `codegg-core`

Create:

```text
crates/codegg-core/Cargo.toml
crates/codegg-core/src/lib.rs
```

Move the central non-UI/non-binary modules into `codegg-core`. Start with modules that are directly part of agent/session/runtime behavior:

```text
src/agent/**
src/bus/**
src/command/**
src/core/**
src/error.rs
src/exec/**
src/goal/**
src/hooks/**
src/ide/**
src/lsp/**              # thin wrapper only; full impl remains egglsp
src/mcp/**
src/memory/**
src/permission/**
src/resilience/**
src/session/**
src/shell_session/**
src/skills/**
src/snapshot/**
src/storage/**
src/task_state/**
src/util/**
src/worktree/**
```

Leave these in the root temporarily unless they are trivial to move:

```text
src/tui/**
src/server/**
src/client/**
src/upgrade/**
src/theme/**
src/tts/**
src/plugin/**
src/research/**
src/search/**
src/search_backend/**
src/tool/**
src/auth/**
src/crypto/**
src/security/**
```

Then move more as needed to satisfy imports, but keep a bias toward minimizing churn.

Likely `codegg-core` dependencies:

```toml
codegg-config = { path = "../codegg-config" }
codegg-protocol = { path = "../codegg-protocol" }
codegg-providers = { path = "../codegg-providers" }
eggcontext = { path = "../eggcontext" }
egggit = { path = "../egggit" }
egglsp = { path = "../egglsp" }
eggsentry = { path = "../eggsentry" }
```

Plus whichever runtime dependencies are still needed by moved modules.

Rules:

- `codegg-core` must not depend on `codegg-tui`.
- `codegg-core` must not depend on root `codegg`.
- If a moved module imports terminal rendering, move that rendering part back to TUI or introduce a plain data representation.
- If a moved module imports CLI-only clap types, keep that part in root binary.
- Preserve current public APIs as much as possible. The first pass should not redesign the agent loop.
- Continue to centralize permissions in core. Extracted crates may classify operations but cannot weaken permission policy.

Root compatibility re-export:

```rust
pub use codegg_core::*;
```

Or, preferably, explicit aliases:

```rust
pub use codegg_core::{agent, core, error, exec, mcp, session, storage};
```

Validation:

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo test --workspace
```

## Phase 5: Extract `codegg-tui`

Create:

```text
crates/codegg-tui/Cargo.toml
crates/codegg-tui/src/lib.rs
```

Move:

```text
src/tui/** -> crates/codegg-tui/src/**
```

The TUI crate should own only terminal UI concerns:

```text
ratatui widgets/layout/rendering
crossterm input handling
terminal event loop
textarea/editor widgets
image rendering when feature-enabled
status/toast rendering
frontend-side command palette behavior
```

Expected dependencies:

```toml
codegg-core = { path = "../codegg-core" }
codegg-config = { path = "../codegg-config" }
codegg-protocol = { path = "../codegg-protocol" }
ratatui = "0.29"
crossterm = { version = "0.28", features = ["event-stream"] }
ratatui-textarea = "0.4"
unicode-width = "0.2"
syntect = "5"          # only if syntax highlighting remains frontend-side
comrak = "0.35"        # only if markdown rendering remains frontend-side
```

Optional image feature:

```toml
[features]
default = []
image = ["dep:ratatui-image", "dep:image"]
```

Rules:

- `codegg-tui` may depend on `codegg-core`; `codegg-core` must not depend on `codegg-tui`.
- If TUI currently reaches into low-level provider/tool/session internals, prefer routing through `CoreClient`, protocol messages, or explicit core APIs.
- Keep terminal-only dependencies out of `codegg-core` after this extraction.
- Do not change UI behavior in this pass.

Update root binary:

```rust
use codegg_tui as tui;
```

Validation:

```bash
cargo check -p codegg-tui
cargo check -p codegg
cargo test --workspace
```

## Phase 6: Optional Server Split

Only do this after the core/TUI split compiles cleanly.

Create:

```text
crates/codegg-server/Cargo.toml
crates/codegg-server/src/lib.rs
```

Move:

```text
src/server/** -> crates/codegg-server/src/**
src/client/** -> crates/codegg-server/src/client/**   # if the attach client is tightly coupled to server protocol
```

Expected dependencies:

```toml
codegg-core = { path = "../codegg-core" }
codegg-protocol = { path = "../codegg-protocol" }
axum = { version = "0.8", features = ["ws"] }
http = "1"
tower-http = { version = "0.6", features = ["cors", "trace", "compression-br", "compression-gzip", "set-header"] }
tokio-tungstenite = "0.26"
```

Root feature wiring:

```toml
[features]
default = ["arboard"]
server = ["dep:codegg-server"]
```

If optional dependency wiring becomes annoying, keep `codegg-server` as a normal dependency for now and optimize later. Correctness and clean boundaries are more important than perfect feature gating in this pass.

Validation:

```bash
cargo check -p codegg-server
cargo check -p codegg --features server
cargo test --workspace --all-features
```

## Phase 7: Optional Tool Split Follow-Up

Defer this unless Phase 4 becomes too large.

Create:

```text
crates/codegg-tools/Cargo.toml
crates/codegg-tools/src/lib.rs
```

Candidate modules:

```text
src/tool/**
src/search_backend/**
src/search/**          # maybe, if builtin web search remains in-tree
src/research/**        # maybe later; high-coupling with providers/tools
src/security/**        # wrapper only; deterministic scanning already in eggsentry
```

Expected dependency direction:

```text
codegg-core -> codegg-tools    # possible, but watch cycles
codegg-tools -> codegg-config
codegg-tools -> eggcontext / egggit / egglsp / eggsentry
```

If `codegg-tools` needs core session/permission types and core needs `ToolRegistry`, that is a cycle. In that case, first split out a tiny `codegg-tool-api` crate:

```text
codegg-tool-api
  Tool trait
  ToolDefinition
  ToolCategory
  ToolError
  StructuredToolResult
  ToolExecutionContext
  ToolProvenance
```

Then:

```text
codegg-core -> codegg-tool-api
codegg-tools -> codegg-tool-api
codegg-core -> codegg-tools     # only for default registry construction, or inject registry from binary
```

Prefer deferring this until the core/TUI/provider/config/protocol split is stable.

## Cargo Manifest Updates

After each extraction, update the root `Cargo.toml` workspace members:

```toml
[workspace]
members = [
    ".",
    "crates/codegg-protocol",
    "crates/codegg-config",
    "crates/codegg-providers",
    "crates/codegg-core",
    "crates/codegg-tui",
    "crates/eggsentry",
    "crates/eggcontext",
    "crates/egggit",
    "crates/egglsp",
]
resolver = "2"
```

Move shared dependency versions into `[workspace.dependencies]` only after the first extraction compiles. Do not combine mechanical code moves with a large manifest normalization unless necessary.

A later cleanup can centralize versions like this:

```toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tokio = { version = "1" }
tracing = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["stream", "json", "rustls-tls"] }
```

## Import Migration Strategy

Use a mechanical, low-risk migration pattern.

During the transition, root `src/lib.rs` may re-export extracted crates so old imports keep compiling:

```rust
pub use codegg_config as config;
pub use codegg_protocol as protocol;
pub use codegg_providers as provider;
```

For core, use explicit module re-exports rather than `pub use codegg_core::*` if possible, because broad glob re-exports can hide ownership mistakes.

Once the workspace compiles, gradually convert imports from:

```rust
use crate::config::schema::Config;
use crate::protocol::core::CoreRequest;
use crate::provider::ProviderRegistry;
```

to:

```rust
use codegg_config::schema::Config;
use codegg_protocol::core::CoreRequest;
use codegg_providers::ProviderRegistry;
```

Do not chase every import in the first commit if compatibility re-exports make the build pass. Prefer smaller, reviewable steps.

## Cycle-Breaking Rules

If a cycle appears, do not paper over it by adding broad dependencies.

Use these rules:

1. DTOs go down into `codegg-protocol`.
2. Config schema goes down into `codegg-config`.
3. Provider runtime goes into `codegg-providers`.
4. Agent/session/runtime orchestration goes into `codegg-core`.
5. Terminal rendering stays in `codegg-tui`.
6. Tool API may need a small `codegg-tool-api` crate if `codegg-tools` extraction causes a cycle.
7. Root binary should depend on everyone; nobody should depend on root binary.

## Compile-Time Hygiene to Add During the Refactor

Add these aliases to `.cargo/config.toml` if the file exists, or create it:

```toml
[alias]
ck = "check --workspace --all-targets"
cktui = "check -p codegg-tui"
ckcore = "check -p codegg-core"
ckfast = "check -p codegg-core -p codegg-tui -p codegg-providers"
```

Do not enable release LTO for dev builds. The current release profile has `lto = true`, `strip = true`, and `codegen-units = 1`; that is fine for release artifacts but should not influence local development. If there are custom dev profile overrides elsewhere, keep them fast.

Consider adding a CI job that runs:

```bash
cargo check --workspace --all-targets --no-default-features
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

If all-features is too slow after the split, keep all-features in nightly CI and use targeted feature checks on normal PRs.

## Acceptance Criteria

The pass is complete when:

1. The workspace includes at least `codegg-protocol`, `codegg-config`, `codegg-providers`, `codegg-core`, and `codegg-tui`, or a documented subset if cycles make the full set too large for one pass.
2. The root `codegg` package is materially thinner than before and primarily acts as CLI entrypoint plus temporary compatibility exports.
3. `codegg-core` does not depend on `codegg-tui`.
4. `codegg-providers` does not depend on `codegg-core` or `codegg-tui`.
5. `codegg-protocol` has no heavy runtime/UI/provider dependencies.
6. TUI dependencies (`ratatui`, `crossterm`, `ratatui-textarea`, optional image rendering) are no longer required by the core crate.
7. Provider HTTP dependencies are isolated in `codegg-providers` where practical.
8. Existing commands still compile and run:

```bash
cargo run -- --help
cargo run -- providers
cargo run -- models
cargo run -- validate
cargo run -- completions bash
```

9. Existing tests pass:

```bash
cargo test --workspace --all-features
```

10. The PR or implementation note includes before/after compile observations, at minimum:

```bash
cargo tree -p codegg --depth 1
cargo check -p codegg-core
cargo check -p codegg-tui
cargo check -p codegg
```

## Suggested First Implementation Slice

If this is handed to a smaller model, constrain it to this slice first:

1. Create `crates/codegg-protocol` and move `src/protocol`.
2. Add root compatibility re-export.
3. Update only the imports required for compilation.
4. Run `cargo check --workspace`.
5. Commit.
6. Create `crates/codegg-config` and move `src/config`.
7. Add root compatibility re-export.
8. Update only imports required for compilation.
9. Run `cargo check --workspace` and `cargo test --workspace`.
10. Stop and report dependency cycles before attempting provider/core/TUI extraction.

This slice is deliberately conservative. It establishes the dependency direction and reveals cycles without touching the agent loop, provider clients, or terminal UI.

## Suggested Second Implementation Slice

After protocol/config compile cleanly:

1. Create `crates/codegg-providers`.
2. Move `src/provider` and provider-related model metadata.
3. Keep provider-facing message types where they are unless a cycle forces a small DTO move.
4. Add root compatibility re-export.
5. Run provider-specific tests and `cargo check --workspace`.
6. Stop if provider code needs session/tool/core types; document the cycle and decide whether those types belong in protocol, core, or a small shared crate.

## Suggested Third Implementation Slice

After provider extraction compiles cleanly:

1. Create `crates/codegg-core`.
2. Move only the modules needed for `CoreClient`, session execution, agent loop, and storage.
3. Leave TUI, server, upgrade, image, and plugin-heavy code in root until the core boundary compiles.
4. Add root compatibility re-exports.
5. Run `cargo check -p codegg-core`, then `cargo check -p codegg`.
6. Only after this passes, extract `codegg-tui`.

## Expected Payoff

This refactor should reduce incremental rebuild pain in three common workflows:

1. TUI iteration should stop invalidating provider clients, protocol DTOs, config parsing, and most core logic.
2. Provider/model-routing iteration should stop rebuilding ratatui/crossterm-heavy frontend code.
3. Config/protocol edits should become small, fast checks instead of root-crate-wide rebuilds.

The architectural payoff is also important: Codegg is already moving toward a core daemon with multiple frontends. This split makes that direction explicit instead of leaving the TUI, provider layer, protocol layer, and agent orchestration tangled in one broad root crate.
