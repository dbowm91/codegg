# AGENTS.md

## Quick Start

Rust 1.81+ required. Edition 2021. Tokio async runtime.

```bash
cargo build --all-features           # build
cargo clippy --all-features -- -D warnings  # lint (errors in CI)
cargo test --all-features            # test everything
cargo fmt                            # format
```

## Cargo Aliases (`.cargo/config.toml`)

```bash
cargo ck           # check --workspace --all-targets
cargo ckroot       # check -p codegg
cargo ckcore       # check -p codegg-core
cargo ckprotocol   # check -p codegg-protocol
cargo ckconfig     # check -p codegg-config
cargo ckproviders  # check -p codegg-providers
cargo cksplit      # check protocol + config + providers + root
```

## Workspace Crates

9 crates under `crates/`:

| Crate | Purpose |
|-------|---------|
| `codegg-core` | Domain types: bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions |
| `codegg-config` | Config schema, paths, loading, validation, file watching |
| `codegg-protocol` | CoreRequest, CoreResponse, CoreEvent, TuiMessage (re-exported as `codegg::protocol`) |
| `codegg-providers` | LLM provider implementations, auth types, CircuitBreaker (re-exported as `codegg::provider`) |
| `egglsp` | LSP client/service/operations (authoritative implementation) |
| `egggit` | Read-only git facts (status, diff, changed files) |
| `eggsentry` | Security scanning (secrets, commands, deps) |
| `eggcontext` | Token counting and context utilities |
| `egglsp-test-server` | Fake LSP server binary for integration tests |

Root `src/` is the application: agent, TUI, tools, server, auth, etc.

## codegg-core Boundary

**codegg-core must NOT depend on UI, server, plugin, or auth crates.** This is enforced by:

```bash
scripts/check-core-boundary.sh
```

Forbidden imports in `codegg-core`: `agent`, `tool`, `permission`, `mcp`, `plugin`, `tui`, `server`, `client`, `auth`, `crypto`, `search`, `search_backend`, `research`, `theme`, `tts`, `upgrade`. Forbidden dependencies: `ratatui`, `crossterm`, `axum`, `wasmtime`, etc.

Run this after touching `codegg-core` or adding workspace crate dependencies.

## Feature Gates

| Feature | What it enables |
|---------|----------------|
| `server` | HTTP/WebSocket server (axum, tower-http) |
| `plugins` | WASM plugin system (wasmtime) |
| `image` | Image rendering in TUI (ratatui-image) |
| `lsp-test-support` | Fake LSP server + integration test harness |
| `lsp-real-server-tests` | Real LSP server smoke tests (requires installed servers) |
| `debug-logging` | Extra debug logging |
| `arboard` | Clipboard support (default) |

Changes to server/plugin modules need `--all-features` testing. LSP integration tests need `lsp-test-support`.

## Testing

```bash
# Core workspace crates
cargo test -p codegg-core
cargo test -p codegg-config
cargo test -p codegg-protocol
cargo test -p codegg-providers

# Native tool crates
cargo test -p eggsentry
cargo test -p eggcontext
cargo test -p egggit
cargo test -p egglsp

# LSP integration (fake server, no network, needs lsp-test-support)
cargo test -p egglsp --features lsp-test-support --test scenario_engine
cargo test --features lsp-test-support --test lsp_composite_stdio

# Real-server smoke tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer --nocapture
```

## Critical Gotchas

### Sync vs Async

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT `await` them.
- **PermissionDecision vs PermissionChoice**: `PermissionDecision` is the bus-owned DTO (`crates/codegg-core/src/bus/mod.rs`). `PermissionChoice` is the domain type (`src/permission/mod.rs`). Bidirectional `From` impls exist. The `PermissionRegistry` API uses `PermissionDecision`.
- **Registration-before-publish**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Module Splits

- **Error enums** live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports + adds `AxumAppError`/`AxumServerRuntimeError` behind `#[cfg(feature = "server")]`.
- **protocol_conversions**: Core conversions in `crates/codegg-core/src/protocol_conversions.rs`. Agent-specific conversions in root `src/protocol_conversions.rs`. Root re-exports core via `pub use codegg_core::protocol_conversions::*;`.
- **Protocol is a re-export**: `src/protocol/` deleted. `src/lib.rs` has `pub use codegg_protocol as protocol;`. Use `codegg_protocol::dto` types.
- **Provider is a re-export**: `src/provider/` re-exports from `crates/codegg-providers` as `codegg::provider`.

### TUI

- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` in `src/tui/app/`.
- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum (`src/tui/app/types.rs:2-25`).
- **DialogType is in component.rs**, not `types.rs`. FocusManager is in `component/focus.rs`.
- **UiState has 27 fields** (lines 40-92 in `src/tui/app/state/ui.rs`). `timeline_visible` and `timeline_selected` are in `UiState`, NOT `App`.
- **AgentLoop has 49 fields** at `src/agent/loop.rs:1380`. Many docs claim 15.

### Tool Registry

- **ToolCatalog::register() takes `&dyn Tool`**, not `Box<dyn Tool>`.
- **multiedit tool exists but NOT in default registry**: `src/tool/multiedit.rs` exists, `pub mod multiedit` is registered, but it's NOT in `ToolRegistry::with_defaults()`.
- **27 tools** in `ToolRegistry::with_defaults()` (`src/tool/mod.rs:90-122`).
- **Tool session constructor**: `with_session_config_defaults(&Config, ...)` is the production constructor. `with_session_defaults(...)` is the legacy all-native fallback.
- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` is used by both `apply_patch` tool and LSP preview operations.

### Agent Runtime

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` via `deps.turn_runtime`. No direct `DefaultTurnRuntime` construction in daemon code (0 direct agent refs).
- **AgentLoopFactory** (`src/agent/agent_loop_factory.rs`) is a build-only seam.
- **CoreRuntimeDeps** (`src/core/runtime_deps.rs`): Bundles pool, memory_store, legacy_agent (LegacyAgentRuntimeDeps), turn_runtime (Arc<dyn TurnRuntime>). Use `with_deps()` for new code.

### LSP

- **egglsp is authoritative**: `src/lsp/` is a thin shim. All real LSP logic lives in `crates/egglsp/`.
- **39 LSP servers** configured in `crates/egglsp/src/server.rs`.
- **Preview-only boundary**: `renamePreview`, `formatPreview`, `sourceActionPreview` never write to disk. `workspace/executeCommand` is never invoked.
- **Capability-gated operations**: `semanticContext` and `securityContext` check `LspCapabilitySnapshot` before expensive LSP calls. Unsupported ops append notes, don't fail.
- **LSP tests need `lsp-test-support` feature**: The fake server binary is `codegg-lsp-test-server`. Tests use polling loops (bounded waits), not fixed sleeps.

### Auth

- **ExternalCommand is disabled**: Both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for any non-empty command. Async timeout plumbing is a follow-up.
- **Credential store**: `~/.config/codegg/credentials.json`. Requires `CODEGG_MASTER_KEY` to store new credentials (not to read env/config-backed keys).
- **Provider registration**: Adding ANY provider via config disables all env-var auto-registration (intentional).
- **Auth logging**: Never log secret prefix/suffix/length. Follow `ResolvedAuthSource::as_str()` pattern.

### Security

- **Security review workflow** (`src/security/workflow/`): Read-only, never mutates files. Risk markers become review prompts, never findings.
- **Security finding synthesis**: Evidence-based, requires 2+ evidence dimensions. Same-file scoping only. Different-file evidence never supports a finding.
- **Auth middleware**: When no token is configured, requests are allowed through (dev convenience, review for production).

### Context Policy

- Context policy is **disabled by default** (`observe` mode). Config via `[context_policy]`.
- Volatile-tail compaction is **disabled by default** (`observe` mode).
- Active mutation of context packer is **disabled**.

## Architecture Docs

- `architecture/core.md` — Core crate ownership boundaries
- `architecture/lsp.md` — LSP client, diagnostics, code operations
- `architecture/auth.md` — Auth types, credential store, CLI
- `architecture/cache-aware-context.md` — Cache-aware packing, context policy
- `architecture/native_crates.md` — Workspace crates, backend contract
- `.opencode/skills/*/SKILL.md` — Module-specific skill guides

## Key Lessons

1. **Verify claims against code** — Many "bugs" in docs turned out to be correct after inspection.
2. **Documentation goes stale** — Struct fields get added/removed; always compare docs to source.
3. **Line numbers are fragile** — References like `watcher.rs:157` can be off by several lines. Use code search.
4. **Count from source, not docs** — Tool/server/command counts drift. Count actual entries in `with_defaults()`, `server_definitions()`, `CommandRegistry`.
5. **Don't assume tool registration** — Not every tool in `/tool` is in the default registry.
