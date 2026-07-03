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
| `codegg-protocol` | CoreRequest, CoreResponse, CoreEvent, TuiMessage, UiNode, UiEffect, PluginManifestDto, PluginInvocation, PluginResponse (re-exported as `codegg::protocol`) |
| `codegg-providers` | LLM provider implementations, auth types, CircuitBreaker (re-exported as `codegg::provider`) |
| `egglsp` | LSP client/service/operations (authoritative implementation) |
| `egggit` | Read-only git facts (status, diff, changed files) |
| `eggsentry` | Security scanning (secrets, commands, deps) |
| `eggcontext` | Token counting and context utilities |
| `egglsp-test-server` | Fake LSP server binary for integration tests (NOT a workspace member; binary in root Cargo.toml behind `lsp-test-support` feature) |

Root `src/` is the application: agent, TUI, tools, server, auth, etc.

`examples/plugins/` contains six reference plugins plus two SDKs — process / wasm / builtin / python / rust patterns. Each example is independent; root workspace unmodified.

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

# TUI render regression tests (headless, no terminal needed)
cargo test --test tui_render

# TUI unit/integration tests
cargo test --test tui

# LSP integration (fake server, no network, needs lsp-test-support)
cargo test -p egglsp --features lsp-test-support --test scenario_engine
cargo test --features lsp-test-support --test lsp_composite_stdio

# Real-server smoke tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer --nocapture

# Plugin example SDKs
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml          # 11 tests
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v       # 24 tests
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release  # WASM build
```

## Built-in Agent Assets

Built-in agent definitions live in `assets/agents/*.toml` with prompt text in `assets/prompts/agents/*.md`.

```bash
python3 scripts/generate_builtin_agents.py   # regenerate src/agent/builtins/generated.rs
python3 scripts/check_builtin_agents.py      # verify TOML matches generated.rs
```

Generated Rust is checked in at `src/agent/builtins/`. **Do not edit generated files directly.**
The `builtin_agents()` function in `src/agent/mod.rs` delegates to the generated code.

## CI Pipeline

CI runs on push/PR to dev/main: `fmt` → `check` → `clippy` → `test` → `plugin-focused` → `examples`. The `plugin-focused` job runs plugin install/management/registry/TUI tests and the core boundary check. `examples` tests SDKs and WASM builds. Local equivalent: `scripts/validate_plugin_ui.sh`.

## Critical Gotchas

### Sync vs Async

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT `await` them.
- **Registration-before-publish**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Module Splits

- **Error enums** live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports + adds `AxumAppError`/`AxumServerRuntimeError` behind `#[cfg(feature = "server")]`.
- **protocol_conversions**: Core conversions in `crates/codegg-core/src/protocol_conversions.rs`. Agent-specific conversions in root `src/protocol_conversions.rs`. Root re-exports core via `pub use codegg_core::protocol_conversions::*;`.
- **Protocol is a re-export**: `src/protocol/` deleted. `src/lib.rs` has `pub use codegg_protocol as protocol;`. Use `codegg_protocol::dto` types.
- **Provider is a re-export**: `src/provider/` re-exports from `crates/codegg-providers` as `codegg::provider`.

### TUI

- **TUI render.rs doesn't exist**: `src/tui/app/` contains `mod.rs` (~13K lines) and `types.rs`. Command handlers are in `src/tui/commands/` (12 submodules). Runtime is in `src/tui/runtime/` (event_loop, command_dispatch, app_events, render_recovery).
- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum (`src/tui/app/types.rs`).
- **DialogType is in component.rs**, not `types.rs`. FocusManager is in `component/focus.rs`.
- **Dialog::Plugin is generic**: A single `Dialog::Plugin` variant handles all plugin dialogs. Plugin dialog content is stored in `PluginUiState.dialogs` and rendered via `PluginDialog` component.
- **Async command pattern**: High-latency TuiCommand handlers use spawn-and-complete via `spawn_tui_task`. The `start_*` function spawns work; a typed completion variant arrives back. Stale protection uses request IDs. See `src/tui/async_cmd.rs`. New apply handlers MUST use the `state.finish(request_id)` / `state.fail(request_id, err)` guard pattern and add a stale-completion test.
- **Sync dispatch is the rule**: `src/tui/runtime/command_dispatch.rs` arms are all `fn` (non-async). Handlers that need async work use `tokio::spawn` + `TuiCommand` completion, or `spawn_registered_tui_task` for lifecycle-tracked work. New dispatch arms should NOT add `.await`.
- **Long output goes to info dialog**: `App::show_short_or_info(info_type, lines)` toasts when ≤3 lines, otherwise opens scrollable `InfoDialog`. Reserve raw `toasts.info(joined)` for single-line responses.
- **Background task lifecycle**: `TuiTaskRegistry` on `App` tracks spawned tasks. Use `spawn_registered_tui_task(tx, registry, kind, name, fut)` for lifecycle-tracked tasks. `App::prepare_shutdown()` cancels all registered tasks.
- **Git sidebar is cached, not rendered live**: `GitSidebarState` caches git info. Refresh triggers on `TuiMsg::SelectSession` and session reload. Results arrive as `TuiCommand::GitSidebarRefreshFinished`; stale generations dropped silently.
- **Remote TUI protocol is event/state-driven**: The `/tui` WebSocket uses `TuiCommand` enum. `RenderFrame` is unsupported. Remote clients use `StateSnapshot` and `RequestSnapshot`.

### Tool Registry

- **ToolCatalog::register() takes `&dyn Tool`**, not `Box<dyn Tool>`.
- **multiedit tool exists but NOT in default registry**: `src/tool/multiedit.rs` exists, `pub mod multiedit` is registered, but it's NOT in `ToolRegistry::with_defaults()`.
- **~28 tools** in `ToolRegistry::with_options()` (`src/tool/mod.rs`). Count varies by config (conditional LSP, security, todo, context_read tools).
- **Tool session constructor**: `with_session_config_defaults(&Config, ...)` is the production constructor. `with_session_defaults(...)` is the legacy all-native fallback.
- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` is used by both `apply_patch` tool and LSP preview operations.

### Agent Runtime

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` via `deps.turn_runtime`. No direct `DefaultTurnRuntime` construction in daemon code.
- **AgentLoop has ~49 fields** at `src/agent/loop.rs:1380`. Many docs claim 15.
- **AgentLoopFactory** (`src/agent/agent_loop_factory.rs`) is a build-only seam.
- **CoreRuntimeDeps** (`src/core/runtime_deps.rs`): Bundles pool, memory_store, legacy_agent, turn_runtime. Use `with_deps()` for new code.

### LSP

- **egglsp is authoritative**: `src/lsp/` is a thin shim. All real LSP logic lives in `crates/egglsp/`.
- **39 LSP servers** configured in `crates/egglsp/src/server.rs`.
- **Preview-only boundary**: `renamePreview`, `formatPreview`, `sourceActionPreview` never write to disk. `workspace/executeCommand` is never invoked.
- **LSP tests need `lsp-test-support` feature**: The fake server binary is `codegg-lsp-test-server`. Tests use polling loops (bounded waits), not fixed sleeps.
- **Preview apply (Phase 9)**: `/lsp-preview-apply` applies patches directly to disk with SHA-256 hash revalidation. Stale previews are blocked. `LspTool` remains read-only; file writes use `std::fs`.
- **LSP semantic cache** is opt-in and disabled by default. Config via `[lsp_semantic_cache]`. Cache is memory-only (disk cache deferred for privacy reasons).
- **Workflow recipes**: `crates/egglsp/src/workflow_recipes.rs` provides named workflows (repair_local, review_file, impact_analysis, etc.) composing LSP primitives. All commands are read-only and never auto-apply previews.

### Plugin System

- **Plugin UI types** live in `codegg_protocol::ui` (`UiNode`, `UiEffect`). TUI consumption via `PluginUiState` and `PluginUiRenderer` / `UiNodeRenderer`.
- **PluginRuntime trait** (`src/plugin/runtime/`): `ProcessRuntime`, `WasmRuntime`, and `BuiltinRuntime` implementations. WASM requires `plugins` feature flag.
- **PluginRegistry** indexes by capability: `command()`, `commands()`, `panels()`, `status_widgets()`, `event_subscribers()`. Duplicate command names rejected.
- **PluginManager** (`src/plugin/management.rs`) is the canonical API for TUI commands: `list()`, `info()`, `enable()`, `disable()`, `doctor()`, `remove()`, `install_from_path()`, `uninstall()`.
- **Plugin install validation**: `src/plugin/install.rs` does lexical path traversal checks BEFORE canonicalizing. Rejects symlinks, hardlinks, and absolute paths in archive members.
- **Plugin security policy**: `PluginPolicy` in `src/plugin/policy.rs` combines lifecycle, UI, permission, install, and runtime sub-policies. All default to conservative. Policy is opt-in.
- **Plugin SDKs**: `examples/plugins/sdk-rust/` (11 tests) and `examples/plugins/sdk-python/` (24 tests). Wire format in `crates/codegg-protocol/src/plugin.rs` (`PLUGIN_PROTOCOL_VERSION = 1`).

### Auth

- **ExternalCommand is disabled**: Both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for any non-empty command.
- **Credential store**: `~/.config/codegg/credentials.json`. Requires `CODEGG_MASTER_KEY` to store new credentials (not to read env/config-backed keys).
- **Provider registration**: Adding ANY provider via config disables all env-var auto-registration (intentional).
- **Auth logging**: Never log secret prefix/suffix/length. Follow `ResolvedAuthSource::as_str()` pattern.

### Security

- **Security review workflow** (`src/security/workflow/`): Read-only, never mutates files. Risk markers become review prompts, never findings.
- **Security finding synthesis**: Evidence-based, requires 2+ evidence dimensions. Same-file scoping only. Different-file evidence never supports a finding.

### Human Shell

- **Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.
- **Syntax**: `!command` runs a shell command with output hidden from the model (ephemeral). `!!command` runs and auto-promotes output into the conversation.
- **Module location**: `src/shell/` — `types.rs`, `runtime.rs`, `store.rs`, `policy.rs`, `digest.rs`.
- **Policy evaluation**: `evaluate_command()` blocks destructive commands (rm -rf /, mkfs, dd to device, fork bombs, shutdown/reboot/halt) and warns on risky ones (rm -rf ., git clean -f, sudo, curl|sh, chmod 777, recursive chown).

### Context Policy

- Context policy is **disabled by default** (`observe` mode). Config via `[context_policy]`.
- Volatile-tail compaction is **disabled by default** (`observe` mode).

## Architecture Docs

`architecture/` has 44 docs covering every module. See the directory for full index. Key ones:

| Document | Key Gotchas |
|----------|-------------|
| `architecture/overview.md` | Counts drift — verify against source |
| `architecture/agent.md` | AgentLoop has ~49 fields |
| `architecture/plugin.md` | No `wasm.rs`; `marketplace.rs` exists |
| `architecture/lsp.md` | egglsp is authoritative; 39 servers |
| `architecture/human_shell.md` | ! commands not in model context unless promoted |

`.codegg/skills/*/SKILL.md` contain 44 module-specific skill guides loaded on-demand via `/skill:`.

## Key Lessons

1. **Verify claims against code** — Many "bugs" in docs turned out to be correct after inspection.
2. **Documentation goes stale** — Struct fields get added/removed; always compare docs to source.
3. **Line numbers are fragile** — References like `watcher.rs:157` can be off by several lines. Use code search.
4. **Count from source, not docs** — Tool/server/command counts drift. Count actual entries in `with_options()`, `server_definitions()`, `CommandRegistry`.
5. **Don't assume tool registration** — Not every tool in `/tool` is in the default registry.
