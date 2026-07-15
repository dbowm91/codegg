# AGENTS.md

## Quick Start

Rust 1.81+ required. Edition 2021. Tokio async runtime.

```bash
cargo build --all-features           # build
cargo clippy --all-features -- -D warnings  # lint (errors in CI)
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14  # full suite, capped
cargo test --test single_daemon_lifecycle  # singleton daemon lifecycle
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
cargo ckgit        # check -p codegg-git
cargo cksplit      # check protocol + config + providers + root
```

## Workspace Crates

10 crates under `crates/`:

| Crate | Purpose |
|-------|---------|
| `codegg-core` | Domain types: bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions, run_store |
| `codegg-config` | Config schema, paths, loading, validation, file watching |
| `codegg-protocol` | CoreRequest, CoreResponse, CoreEvent, TuiMessage, UiNode, UiEffect, PluginManifestDto, PluginInvocation, PluginResponse (re-exported as `codegg::protocol`) |
| `codegg-providers` | LLM provider implementations, auth types, CircuitBreaker (re-exported as `codegg::provider`) |
| `codegg-git` | Typed Git operation model, argv parser, and risk classification |
| `egglsp` | LSP client/service/operations (authoritative implementation) |
| `egggit` | Read-only git facts: status (v2 rich structured), diff, changed files, log, blame, refs (branches/tags/remotes), worktree |
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

## Test Resource Budget

The workspace test matrix is large (~1,219 async tests across 94 files). Prefer the narrowest crate, test file, or test name that covers a change before reaching for a workspace-wide run.

When you do need the full suite locally, cap Cargo's build parallelism and limit test threads:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
```

`--test-threads=14` limits concurrent test execution per binary. `CARGO_BUILD_JOBS=1` prevents the compile/link fan-out that drives the RAM and iowait spikes.

Run `--all-features` and `lsp-test-support` paths as separate capped invocations when possible; those are the heaviest test paths in this repo.

**RunStore-specific guidance** (`crates/codegg-core/src/run_store.rs`): the
authoritative artifact checksum lives on the *artifact store* record
(`MemArtifactEntry.sha256` for `MemRunStore`, `ArtifactRecord.sha256`
from the persisted manifest for `FsRunStore`), not on the manifest
copy that `RunManifest.artifacts` carries. Tests verifying integrity
MUST mutate either the bytes on disk or the in-store record.
`FsRunStore.lock: tokio::sync::Mutex<()>` is **not reentrant**; callers
acquire it once and call `rewrite_index_locked` directly. See
`architecture/run_store.md` Invariants.

See `architecture/testing.md` for the full test resource taxonomy, Tokio runtime flavor rules, pool strategy guidance, test selection pattern, and capped full-suite command.

## Testing

```bash
# Core workspace crates
cargo test -p codegg-core
cargo test -p codegg-core run_store
cargo test -p codegg-config
cargo test -p codegg-protocol
cargo test -p codegg-providers

# Native tool crates
cargo test -p eggsentry
cargo test -p eggcontext
cargo test -p egggit
cargo test -p egggit status_v2
cargo test -p egggit log
cargo test -p egggit blame
cargo test -p egggit refs
cargo test -p egggit operation_state
cargo test -p egggit conflict
cargo test -p codegg git_service
cargo test -p codegg-git
cargo test --test git_recovery_integration
cargo test --test git_execution_origin_matrix    # Workstream D — 19 tests across 10 execution origins
python3 scripts/check_git_forbidden_patterns.py  # Workstream E — static checks (drift + secret boundaries)
cargo test -p egglsp

# TUI render regression tests (headless, no terminal needed)
cargo test --test tui_render

# TUI unit/integration tests
cargo test --test tui

# Shell output projection evaluation harness (fixture corpus, 11 invariant tests)
cargo test --test shell_projection_harness

# Shell projection context budget and compaction tests (33 tests)
cargo test --test shell_projection_phase10

# Shell projection redactor unit tests
cargo test -p codegg --lib shell::redactor

# Shell projection RTK unit tests (no RTK binary required)
cargo test -p codegg --lib shell::rtk

# Test runner module (resolver, parser with failure extraction, report formatter, previous-failures index)
cargo test -p codegg --lib test_runner

# Test report to projection adapter (Phase 03)
cargo test -p codegg --lib test_runner::projection

# Strict custom-command validator (argv-prefix allowlist + shell-metachar rejection)
cargo test -p codegg --lib test_runner::custom

# Previous-failures index (read/write, truncation, resolution, safety validation)
cargo test -p codegg --lib test_runner::index

# Test tool (model-facing wrapper for supervised test runner)
cargo test -p codegg --lib tool::test

# Run supervised tests via TUI slash command
# /test, /test workspace, /test changed, /test package <name>, /test file <path>, /test previous|prev|last, /test custom <argv>
# Custom commands must be plain whitespace-separated argv matching an allowlist prefix (cargo test, pytest, etc.).
# Shell metacharacters (`;`, `|`, `>`, `$`, backticks, newlines, etc.) are rejected at validation time.
# Previous failures scope reruns the most recent failing test from a bounded local index.
# See architecture/test_runner.md "Custom Command Allowlist" and "Previous-Failures Index" for the full contract.

# Test runner event sink tests
cargo test -p codegg --lib test_runner::runner::tests

# Test command parser tests
cargo test -p codegg --lib tui::commands::test

# Stale /test completion protection (shared with all async dialog paths)
cargo test -p codegg --lib async_request

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

# Eggsact adapter integration tests
cargo test --test eggsact_adapter
cargo test --test eggsact_deterministic_tools

# Eggsearch adapter unit tests (inline in source)
cargo test -p codegg --lib search_backend::eggsearch

# Eggsearch bootstrap tests (inline in source)
cargo test -p codegg --lib search_backend::bootstrap

# Eggsearch mock integration tests (dispatch, arg mapping, raw MCP)
cargo test --test fake_eggsearch_mcp
cargo test --test search_backend_eggsearch
cargo test --test search_backend_arg_mapping

# Preflight integration tests (policy, check methods, golden output)
cargo test --test preflight_integration

# Framing golden tests (inline in source)
cargo test -p codegg --lib search_backend::framing

# Live eggsearch smoke tests (opt-in, requires eggsearch binary)
cargo test --features live-eggsearch-tests --test live_eggsearch_smoke -- --ignored

# Test timing with nextest (install: cargo install cargo-nextest)
cargo nextest run --workspace --profile ci-heavy --all-features
cargo nextest run -p codegg-core --profile ci-heavy

# Audit tokio test flavors (finds current_thread tests with concurrency patterns)
python3 scripts/audit_tokio_tests.py

# Check for bare #[tokio::test] annotations (regression guard)
python3 scripts/check-tokio-test-flavors.py

# Command intent classifier (intent classification, risk assessment, ShellShape parsing, fixtures)
cargo test -p codegg --lib command_intent

# Shell shape parser (quotes, escapes, operators, complex shell detection)
cargo test -p codegg --lib command_intent::shell_shape

# Command planner (backend routing, permission generation, fixtures)
cargo test -p codegg --lib command_planner

# Command routing (backend routing MVP, test/git/search/python routing)
cargo test -p codegg --lib command_routing

# Python scripting (risk analysis, script execution, timeout, fixtures)
cargo test -p codegg --lib python_script

# Bash tool routing metadata (classify + plan + route integration, config tests)
cargo test -p codegg --lib tool::bash

# Adversarial command routing tests (139 tests — command smuggling, workspace escape, kill switches, Observe/Active modes, per-family RouteLevel, validation failures, safe/dangerous git mutations, full pipeline integration)
cargo test --test command_routing_adversarial

# Adversarial Python sandbox tests (57 tests — workspace escape, AST-evasive patterns)
cargo test --test python_sandbox_adversarial

# Adversarial context projection tests (90 tests — projection poisoning, binary/special chars, Unicode edge cases, content injection)
cargo test --test context_projection_adversarial

# Command routing execution ownership tests (13 tests — planned vs actual backend, RunOwnership, DelegatedBackend ownership, raw artifact safety, fallback records, provenance serde, backward compat)
cargo test --test command_routing_execution_ownership

# Phase 09 projection contract tests
cargo test -p codegg --lib shell::projector -- projection_id
cargo test -p codegg --lib shell::projector -- span_role
cargo test -p codegg --lib shell::projector -- promotion
cargo test -p codegg --lib shell::projector -- preferred_projector
cargo test -p codegg --lib python_script::projection
cargo test -p codegg --lib test_runner::projection
```

## Built-in Agent Assets

Built-in agent definitions live in `assets/agents/*.toml` with prompt text in `assets/prompts/agents/*.md`.

```bash
python3 scripts/generate_builtin_agents.py              # regenerate src/agent/builtins/generated.rs
python3 scripts/generate_builtin_agents.py --check      # staleness + schema validation (CI mode)
python3 scripts/check_builtin_agents.py                 # verify TOML matches generated.rs
```

Generated Rust is checked in at `src/agent/builtins/`. **Do not edit generated files directly.**
The `builtin_agents()` function in `src/agent/mod.rs` delegates to the generated code.

Schema validation (`--check`) enforces: valid `mode` (Primary/Subagent/All), required `name`/`description`, prompt file exists when `prompt_file` set, valid permission actions (allow/ask/deny), no unknown keys, no duplicate names, and deterministic output.

### Local Validation

After making changes to agents, run the full validation suite:

```bash
python3 scripts/generate_builtin_agents.py --check      # staleness + schema validation
python3 scripts/check_builtin_agents.py                 # verify TOML matches generated.rs
cargo fmt --check                                        # formatting check
cargo check --workspace                                  # compilation check
CARGO_BUILD_JOBS=1 cargo test --workspace -- --test-threads=14  # all tests, capped
```

## User/Project Agent Customization

Users and projects can add custom agents via TOML and Markdown files:

- **Global agents**: `~/.config/codegg/agents/*.toml` or `*.md`
- **Project agents**: `.codegg/agents/*.toml` or `*.md` (relative to `$PWD`)

### TOML Format

```toml
name = "my-agent"
mode = "subagent"          # case-insensitive: Primary, SUBAGENT, All, etc.
description = "A custom agent"
prompt = "You are a helpful assistant."

[permission]
read = "allow"
bash = "ask"
write = "deny"
```

Or wrapped format:

```toml
[agent]
name = "my-agent"
mode = "subagent"
description = "A custom agent"

[agent.permissions]
read = "allow"
```

### Overlay Flags

File-based TOML agents support overlay flags that control how they interact with base agents:

```toml
name = "my-agent"
mode = "subagent"
description = "Agent with overlay flags"
replace = false   # merge into base agent (default) vs full replacement
disable = false   # remove agent from resolution
merge = true      # explicitly enable merge mode
```

- **`replace = true`**: Full replacement — the overlay completely replaces the base agent (legacy behavior)
- **`replace = false`** (default): Merge mode — overlay fields are applied on top of the base agent. Scalar fields replace only when set. Permissions merge per-tool (overlay overwrites matching keys).
- **`merge = true`**: Explicitly enable merge mode (same as default, for clarity)
- **`disable = true`**: Removes the agent from resolution entirely (logged as Info diagnostic)

> **Note:** Overlay flags are TOML-only. Markdown files always use merge mode and do not support `replace`, `disable`, or `merge` flags.

### Rich Permissions

#### Simple Permissions

```toml
[permission]
read = "allow"
bash = "ask"
write = "deny"
```

#### Bash Permission Patterns

Fine-grained control over bash commands:

```toml
[bash_permission]
action = "ask"                                          # default action for unmatched commands
allow_patterns = ["git diff*", "cargo test*", "ls *"]   # auto-allowed command patterns
deny_patterns = ["curl*", "rm *", "sudo *"]             # auto-denied command patterns
```

- Patterns use glob syntax (`*` matches any characters)
- Deny patterns are evaluated before allow patterns
- The `action` field sets the default for commands that match no patterns

#### Path Permission Patterns

Fine-grained control over file access:

```toml
[path_permission]
allow = ["src/**", "crates/**", "tests/**"]   # allowed file path patterns
deny = [".git/**", "target/**", "**/*.env"]    # denied file path patterns
```

- Patterns use glob syntax (`**` matches directories, `*` matches within a directory)
- Denied paths are checked before allowed paths

### Markdown Format

```markdown
---
name: my-agent
mode: subagent
description: A custom agent
---

You are a focused code reviewer.
Check for safety issues.
```

The markdown body becomes the agent's prompt unless `prompt` or `prompt_file` is explicitly set.

> **Note:** Markdown is a **prompt-first, merge-only** format. It supports flat `permission` maps in frontmatter and `disable`, but does not support overlay flags (`replace`, `merge`) or structured permission sections (`[bash_permission]`, `[path_permission]`). Use TOML for those features.

### Prompt File Resolution

`prompt_file` is resolved relative to the directory containing the agent file:

```toml
prompt_file = "prompts/my-agent.md"  # resolved from agent file's directory
```

### Resolution Order

1. Compiled built-ins
2. Global files (`~/.config/codegg/agents/`)
3. Project files (`.codegg/agents/`)
4. Config `agent` map
5. Config `mode` map

**Overlay merge behavior**:
- Layers 2-3 (file-based agents): **Merge by default** — overlay fields are applied on top of the base agent. Use `replace = true` for full replacement.
- Layer 4 (config `agent` map): **Field-level merge** — each field uses `cfg.field.or_else(|| agent.field)` pattern. Permissions merge additively (config overwrites matching keys).
- Layer 5 (config `mode` map): **Permission merge** — mode tools are applied on top of existing agent permissions.

**Safety envelope**: Agent permissions are bounded by the most restrictive level across agent, session, config, and hard-deny layers. A deny at any layer overrides allows at lower layers.

Project files override global files. Config overrides file-based agents.

## CI Pipeline

CI runs on push/PR to dev/main: `agent-assets` → `fmt` → `check` → `clippy` → `test` → `plugin-focused` → `examples`. The `agent-assets` job validates built-in agent TOML schemas and checks for stale generated output. The `test` job runs the full workspace test suite plus explicit shell projection validation steps (harness, context budget, redactor, RTK unit tests). The `plugin-focused` job runs plugin install/management/registry/TUI tests and the core boundary check. `examples` tests SDKs and WASM builds. Local equivalent: `scripts/validate_plugin_ui.sh`.

## Critical Gotchas

### Sync vs Async

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT `await` them.
- **Registration-before-publish**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Daemon lifecycle

- **Singleton invariant**: Exactly one user-scoped Codegg daemon is active per OS user. The lock is at `daemon.lock` in the user-scoped runtime directory (macOS: `$HOME/Library/Application Support/codegg`, Linux: `${XDG_RUNTIME_DIR:-/tmp}/codegg`). Override with `CODEGG_DAEMON_HOME`.
- **Connect-or-start default**: Plain `codegg` uses `connect_or_start_daemon` (`src/core/instance.rs`) to connect to the running daemon or auto-start one. `--standalone` runs an in-process core; `--stdio` uses the hidden `core-stdio` path. `--core-transport inproc|stdio` is deprecated and emits a warning.
- **Server requires `--standalone-core`**: The HTTP server does not silently construct its own daemon. Without `--standalone-core`, it exits with an actionable error. Daemon-proxying server mode lands in a later phase.
- **Lock is authoritative**: `DaemonInstanceGuard` holds `flock(LOCK_EX | LOCK_NB)` for the daemon's lifetime. `daemon.json` metadata (daemon_id, generation, pid, socket_path, protocol_version, started_at, binary_version) is diagnostic only.
- **PID file is legacy**: The old `<socket>.pid` file is still written for backward compat with external scripts, but the authoritative identity is the metadata record + lock.
- **Stop verifies liveness**: `daemon stop` probes the socket before sending SIGTERM. It refuses to unlink paths it does not own.
- **Production invariant**: No two `codegg` daemons can be active for one user scope. This is enforced by the advisory lock, not by PID files.
- See `plans/single-daemon-phase-01-singleton-lifecycle-and-default-transport.md` and `src/core/instance.rs` for the full contract.

### Module Splits

- **Error enums** live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports + adds `AxumAppError`/`AxumServerRuntimeError` behind `#[cfg(feature = "server")]`.
- **protocol_conversions**: Core conversions in `crates/codegg-core/src/protocol_conversions.rs`. Agent-specific conversions in root `src/protocol_conversions.rs`. Root re-exports core via `pub use codegg_core::protocol_conversions::*;`.
- **Protocol is a re-export**: `src/protocol/` deleted. `src/lib.rs` has `pub use codegg_protocol as protocol;`. Use `codegg_protocol::dto` types.
- **Provider is a re-export**: `src/provider/` re-exports from `crates/codegg-providers` as `codegg::provider`.

### TUI

- **TUI render.rs doesn't exist**: `src/tui/app/` contains `mod.rs` (~13K lines) and `types.rs`. Command handlers are in `src/tui/commands/` (13 submodules). Runtime is in `src/tui/runtime/` (event_loop, command_dispatch, app_events, render_recovery).
- **Custom test command validation is strict argv-prefix**: `src/test_runner/custom.rs::validate_custom_command` is the single source of truth — used by both the model-facing `test` tool (`src/tool/test.rs`) and the `/test` slash command (`src/tui/commands/test.rs`). Rejects shell metacharacters (`;`, `|`, `>`, `<`, `&`, `$(`, `` ` ``, `$`, `\`, quotes, parentheses, braces, brackets, `*`, `?`, `~`, `#`, `!`), newlines, NUL/control characters, and bidi Unicode controls. Argv-token-bounded match, so `pytestevil` and `cargo testify` do NOT match. Resolver re-runs the validator as defense-in-depth before producing `ResolvedTestCommand.argv`. Both generated and custom commands execute via direct `Command::new(argv[0]).args(&argv[1..])` — never via a shell.
- **Previous-failures index**: `.codegg/test-runs/index.json` stores up to 100 recent test run entries (newest-first). `TestScope::PreviousFailures` loads the index, scans for the newest actionable failure (`Failed` or `TimedOut`), validates cwd and argv safety, and returns a `ResolvedTestCommand` for rerun. The index is written atomically after every test run via `append_to_index()` in `runner.rs`. See `architecture/test_runner.md` "Previous-Failures Index (Phase 06)".
- **TestRunner RunStore integration**: `TestTool` has `with_run_store()` constructor matching the bash/python pattern. TestRunner persists to `RunStore` (`RunKind::Test`) in addition to its existing `.codegg/test-runs/` storage. The `rerun` field is set so `can_rerun` works from TUI. Both the model-facing `test` tool and TUI `/test` slash command persist to RunStore.
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
- **~37 tools** in `ToolRegistry::with_options()` (`src/tool/mod.rs`). Count varies by config (conditional LSP, security, todo, context_read tools). Includes 8 always-visible eggsact deterministic tools.
- **Tool session constructor**: `with_session_config_defaults(&Config, ...)` is the production constructor. `with_session_defaults(...)` is the legacy all-native fallback.
- **Integrated tool config (Phase 6)**: `src/tool/integrated_config.rs` resolves evidence/deterministic/preflight runtime configs once from `Config` via `resolve_integrated_config()`. Passed through `ToolRegistryOptions` to `with_options()`. Subagents now use `with_config(&config)` (`src/agent/worker.rs:698`) to inherit backend config — previously used `with_defaults()` which dropped it.
- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` is used by both `apply_patch` tool and LSP preview operations.
- **eggsact is in-process, not MCP**: The `eggsact` dependency is consumed as a direct Rust dependency (`src/eggsact/adapter.rs`), not via MCP server. `EggsactRuntime` wraps `eggsact::agent::ToolRegistry` in-process. Provenance must tag `backend = "native"`, `implementation = "eggsact/<tool_name>"`, `trust = LocalTrusted`. `EggsactCallResult` carries structured fields (`result`, `findings`, `warnings`, `error_type`, `error`) in addition to the legacy string output.
- **Deterministic tools (Phase 4)**: `EggsactTool` generic wrapper in `src/tool/deterministic.rs` exposes a conservative subset of eggsact tools to the model. 8 always-visible tools (`text_equal`, `text_diff_explain`, `text_replace_check`, `validate_json`, `validate_toml`, `command_preflight`, `path_normalize`, `text_security_inspect`) plus 5 deferred tools discoverable via `tool_search`. All use `ToolCategory::ReadOnly` and are registered best-effort; if `EggsactRuntime::new()` fails, the tools are silently skipped. `truncate_utf8_safe()` in `src/eggsact/adapter.rs` is the shared helper for UTF-8 safe truncation of tool output. `DeterministicToolsConfig::validate()` provides runtime config validation for the deterministic tool layer. `BootstrapReport` has `tool_coverage_status()`, `missing_required_tools()`, and `missing_recommended_tools()` for classifying which eggsact tools are available vs missing.
- **Preflight (Phase 5)**: `src/preflight/` provides harness-side automatic validation before mutating operations (edits, config writes, shell commands) using eggsact. Config: `[preflight]` section in opencode.json. Default mode: `warn`. Key types: `PreflightService`, `PreflightPolicy`, `PreflightDecision`, `PreflightFinding`. Integration points: edit, replace, apply_patch, multiedit, bash tools. **Harness-internal only** — preflight calls do not appear as model-facing tool calls. Findings are severity-classified (`Block`/`Warn`/`Annotate`) and can block, warn, or annotate depending on policy mode (`off`, `observe`, `warn`, `block_on_definite`). **Structured parsing**: Decisions use structured eggsact fields first (`result`, `findings`, `warnings`), falling back to string parsing for legacy output. `PreflightConfig::validate()` provides runtime config validation.
- **CommandIntentMode**: `CommandIntentMode` enum (`Observe | Active | deprecated Route`) with default `Observe`. `CommandIntentConfig` has `mode` field. `Active` enables validated dispatch to structured backends; `Route` remains a backward-compatible alias. `route_safe_commands = true` alone does NOT enable active routing. Failed validation still falls back to the raw-shell path.

### Agent Runtime

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` via `deps.turn_runtime`. No direct `DefaultTurnRuntime` construction in daemon code.
- **AgentLoop has ~49 fields** at `src/agent/loop.rs:1380`. Many docs claim 15.
- **AgentLoopFactory** (`src/agent/agent_loop_factory.rs`) is a build-only seam.
- **CoreRuntimeDeps** (`src/core/runtime_deps.rs`): Bundles pool, memory_store, legacy_agent, turn_runtime. Use `with_deps()` for new code.
- **AgentRegistry** (`src/agent/registry.rs`): Central registry separating declarative sources from resolved runtime agents. Tracks source provenance (Builtin, GlobalFile, ProjectFile, ConfigAgent, ConfigMode, Session) and emits diagnostics. `AgentRegistry::load(config)` replicates the 5-layer resolution order from `resolve_agents()`. `into_agents()` provides backward compatibility. New code should prefer `AgentRegistry` over `resolve_agents()`.
- **Emergency fallback model**: `EMERGENCY_DEFAULT_MODEL` constant is centralized in `src/agent/mod.rs`. When no model is configured or resolved, this constant is used and a warning is emitted. Users should always configure models explicitly to avoid silent fallback behavior.

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

### Git

- **GitExecutionService is the canonical read executor**: `src/git_service.rs` provides `GitExecutionService` which delegates to `egggit` for structured parsing and falls back to subprocess execution for mutations. All structured git reads (status, diff, log, blame, refs, branches, tags, remotes) flow through this service. Downstream consumers should consume `GitPayload` variants, not raw stdout.
- **egggit is read-only, mutations stay in Codegg**: `egggit` modules (`status_v2`, `log`, `blame`, `refs`, `diff`, `worktree`) never mutate repository state. Commit, checkout, branch create/delete, stash push/pop, and all other mutations are handled by the `git_mutations` executor (`src/git_mutations.rs`) under the permission flow.
- **status_v2 replaces raw status parsing**: `status_v2::rich_status()` returns `RichRepoStatus` with branch state, ahead/behind, staged/unstaged/untracked/conflict entries. TUI sidebar and prompt context consume this directly. The legacy `status::RepoStatus` remains available for backward compatibility.
- **GitTool structured-first execution**: `src/tool/git.rs` attempts structured execution via `try_structured_read()` for all read-only subcommands before falling back to raw subprocess output.
- **Phase D: typed mutations with state deltas**: `src/tool/git.rs` accepts a `mutation` action (`stage_paths`, `stage_all`, `commit`, `branch_create`, `merge`, `rebase`, `cherry_pick`, `revert`, `abort`, etc.). All mutations route through `GitMutationExecutor` in `src/git_mutations.rs`, which captures `RepoSnapshot` before/after, computes `StateDelta`, pins env (`GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`), and persists to `RunStore` with `RunKind::GitMutation`, `PlannedBackend::Git`, `RunOwnership::DelegatedBackend`. See `architecture/git.md` Phase D section. The raw `subcommand` path is still available for passthrough.
- **Phase E: network, configuration, destructive operations**: `src/git_network_policy.rs` provides `NetworkEnvPolicy` for env hardening, `classify_network_failure()` for stderr classification, and `redact_url_credentials()` for sanitizing remote URLs before they reach the persistence layer. `src/git_network_ops.rs` adds typed helpers for `fetch`/`pull`/`push`/`remote_*`/`config_*`/`reset_*`/`clean_preview`/`clean`. All Phase E mutations are exposed as new `mutation` action entries in `src/tool/git.rs` and persist to RunStore with `RunKind::GitMutation`. `PushForce::Force` and `CleanRequest::is_broad()` are tagged destructive and rejected by tool-side policy (`ToolError::Execution`). Config keys are gated by `CONFIG_KEY_ALLOWLIST` and `CONFIG_DENIED_KEY_PATTERNS` (denies `credential.*`, `http.*`, `url.*`, `core.gitProxy`); global-only keys (`user.*`, `gpg.format`) are rejected when `scope=local`. See `architecture/git.md` Phase E section.
- **Phase F: conflicts, recovery, ergonomics, closure**: `crates/egggit/src/operation_state.rs` exposes `RepositoryOperationState` (eight families: merge, rebase, cherry-pick, revert, bisect, apply-mailbox, sequencer, unknown) plus `RecoveryAction::{Continue, Abort, Skip}` with operation-aware availability checks. `crates/egggit/src/conflict.rs` defines typed `ConflictEntry`, `ConflictKind`, `ConflictShape`, `ConflictReport` (no auto-resolve — agents edit markers, stage with `mutation: "stage_paths"`, then recover). `src/git_recovery.rs` exposes `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress` that detect state, dispatch the correct typed `GitOperation`, refuse cross-operation misuse, and persist via `git_run_store::persist_recovery` (RunKind::GitMutation, backend.detail = `"recover:<action>"`). The `git` tool gains `operation_state` (typed state probe) and `recover` (continue|abort|skip) parameters, both persisted to RunStore. TUI sidebar (`src/tui/commands/git_sidebar.rs` + `src/tui/app/state/session.rs`) caches `operation_state_label`, `available_actions`, and `conflicted_paths` from the typed probe. `project_recovery()` in `src/git_mutation_projector.rs` formats the result. Agent prompts (`assets/prompts/agents/general.md`) now carry Phase F git workflow guidance. Schema snapshot tests in `tool::git::schema_tests` pin mutation enum, recover enum, and description. See `architecture/git.md` Phase F section.
- **Phase F corrective security closure (post-merge)**: Two Phase F findings were closed by the corrective closure pass. (1) `remote_set_url` credential leakage: `GitOperation::RemoteAdd.url` and `GitOperation::RemoteSetUrl.url` are now typed as `codegg_git::RedactedUrl`, a newtype whose `Debug`/`Display`/`Serialize` paths see only the redacted form; raw is reachable exclusively via `RedactedUrl::expose_secret()`, consumed at the final `render_argv` boundary. `git_run_store.rs` flows the audit argv through `sanitize_argv_for_run_store` before persistence. `src/git_mutations.rs::sanitize_truncate_for_result` and `src/git_network_policy.rs::redact_url_credentials_in_text` are the two-line defense-in-depth sanitizers that keep credential leaks out of `MutationResult.stdout/stderr` and RunStore artifacts. (2) Raw fallback missing hardened env policy: every Codegg-owned `git` subprocess now flows through `GitEnvPolicy::apply()` (tokio async) or `GitEnvPolicy::apply_sync()` (synchronous TUI probes). The default policy includes `strip_command_bearers = true`, which removes 27 vars including `GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, all `GIT_CONFIG_*`, `GIT_DIR`, `GIT_WORK_TREE`, `GIT_PAGER`, etc. Affected callers: `src/tool/git.rs::run_raw_subcommand`, `src/git_service.rs::run_git_raw`, `src/tool/commit.rs::fetch_head_message`, `src/core/daemon.rs::SnapshotWorkspace`, the TUI diff/checkout/show dialogs, and `crates/codegg-core/src/worktree.rs::create_worktree`/`remove_worktree`. See `docs/validation/git-security-review.md` "Resolutions" section and `architecture/git.md` "Phase F corrective security closure" section.
- **Polish / maintainability / verification pass**: Three further invariants tighten the closure. (1) **Canonical subprocess policy**: `ALLOWED_ENV_VARS` and `ALWAYS_STRIPPED_ENV_VARS` now live in `codegg_git::process_policy` and are re-exported by both `src/git_mutations.rs` and `crates/codegg-core/src/worktree.rs`. The root crate and the core worktree helper can no longer silently drift — drift is caught by `cargo test -p codegg-git` (in-module tests), `cargo test -p codegg-core` (`worktree_uses_canonical_policy` + `canonical_includes_locally_drifted_entries`), and `src/git_mutations.rs::policy_drift_tests`. (2) **Audit-safe rerun argv**: `RerunDescriptor.argv` is now `Option<AuditSafeArgv>` (a newtype in `codegg_git::sensitive`). The only construction path (`AuditSafeArgv::from_argv`) runs the URL sanitizer on every token, so durable RunStore records are credential-free; the deserializer re-runs the sanitizer to normalize historical records. See `docs/validation/git-rerun-secret-lifecycle.md` for the full lifecycle. (3) **Forbidden-pattern static checks**: `scripts/check_git_forbidden_patterns.py` enforces (a) `expose_secret()` only at the `render_argv` boundary, (b) no hand-maintained env-policy tables, (c) `RerunDescriptor.argv` is always `AuditSafeArgv`, (d) git argv flowing into `RunInvocation` is sanitized. The script is part of the standard local validation. See `architecture/git_polish_verification_handoff.md` for the post-closure verified state.
- **Track U: unified Bash→Git routing (functional closure of the bash/git gap)**: The BashTool previously fell back to raw shell for `git add` and other GitMutating commands because `intent_kind_to_family(GitMutating) → None` and the planner's `ExecutionBackend::Git` carried no family. Track U splits the Git families into four: `GitRead` (read-only), `GitLocalMutation` (`add`, `commit`, `branch`, `checkout`, `restore`, `stash`, `merge`/`rebase`/`cherry-pick` without `--force`/network), `GitNetwork` (`fetch`/`pull`/`push`/`clone`/`remote add`/`ls-remote`), and `GitDestructive` (`reset --hard`, `clean -fdx`, `push --force`). Each family has its own `RouteLevel` config gate (`route_git_local_mutation`, `route_git_network`, `route_git_destructive` — all default `None`/conservative). The new `plan_family(plan)` resolver in `src/tool/bash.rs` calls `git_operation_family(&request.operation, &request.risk_set)` (in `src/command_intent/plan.rs`) which selects the family via risk precedence `Destructive > Network > Read > LocalMutation`. The BashTool's `RouteToGit` arm now dispatches through `dispatch_to_git`, which routes typed operations through `GitMutationExecutor` (with snapshot/delta/RunStore parity to the native tool, `backend.family = "git_bash_translation"`, `backend.detail = "bash_translation"`, `RunOwnership::DelegatedBackend`, no-double-execution) and falls back to managed-argv (with `GitEnvPolicy`) for `ManagedGitArgv` plumbing. Reads are NOT persisted (matching native tool behavior). The conservative default (`route_git_local_mutation = Off`) keeps existing user-visible behavior: bash git mutations still run via raw shell until the user opts in. New matrix rows pin the contract: `row_5` (default = raw shell), `row_5b` (`-C` fallback path), `row_5c` (typed active path → DelegatedBackend). See `tests/git_execution_origin_matrix.rs` and `architecture/git.md` Track U section.

### Human Shell

- **Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.
- **Syntax**: `!command` runs a shell command with output hidden from the model (ephemeral). `!!command` runs and auto-promotes output into the conversation.
- **Module location**: `src/shell/` — `types.rs`, `runtime.rs`, `store.rs`, `policy.rs`, `digest.rs`, `projection.rs`, `projector.rs`, `projection_bridge.rs`, `rtk.rs`, `redactor.rs`.
- **Policy evaluation**: `evaluate_command()` blocks destructive commands (rm -rf /, mkfs, dd to device, fork bombs, shutdown/reboot/halt) and warns on risky ones (rm -rf ., git clean -f, sudo, curl|sh, chmod 777, recursive chown).
- **Command-event projection (Phase 1)**: `CommandOutputStore` retains raw stdout/stderr out-of-band for the projection pipeline. `ShellCommandRunBridge` mirrors `ShellEvent`s into the store. Stable handles `cmd://<id>/<stream>` resolve raw output without rerunning commands. Caps: 32 MiB per stream, 64 MiB total, 100 history entries. Streams exceeding the cap are marked `OutputCompleteness::Partial` rather than silently truncated. The two stores (`ShellOutputStore` for TUI transcripts, `CommandOutputStore` for projection) run side by side — Phase 1 is additive, not a replacement.
- **Projection trait and built-in projectors (Phase 2)**: `src/shell/projector.rs` defines the `CommandOutputProjector` trait, `ProjectionRequest`/`ProjectionResult`, `ProjectionTarget`/`ProjectionBudget`/`ProjectionPolicy`, `ProjectionKind`/`ProjectionExactness`, `OmittedRange`, `ExpansionHandle`, and the `RawProjector` / `TruncatedProjector` / `ErrorRetentionProjector` implementations. `ProjectionSelector::with_defaults()` is the centralised selector; `default_command_projection` is now a thin wrapper around it. `apply_redaction_hook` is the Phase 8 redaction entry point; its call site lives in `ProjectionSelector::project` so the redaction contract cannot be bypassed by RTK or native projectors.
- **Projection config and TUI metadata (Phase 4, partial)**: `ShellOutputConfig` in `crates/codegg-config/src/schema.rs` defines `[shell.output]` with `projection` (off|safe|rtk|aggressive), `retain_raw`, `redact_model_visible_output`, `max_model_output_tokens`, `show_projection_metadata`, `prefer_native_projectors`, and `[shell.output.rtk]` sub-table. `ProjectionSelector::with_config()` builds a selector from config. Per-command rules are parsed but not yet consumed by the projection pipeline. Escape hatches and full rule-based projector selection are deferred.
- **RTK discovery and projection skeleton (Phase 5)**: `src/shell/rtk.rs` adds `RtkDiscovery` (lazy detection, version probe, availability state), `RtkAvailability` with `RtkState` enum (Disabled, Available, NotFound, Broken, TimedOut, UnsupportedVersion), `RtkCapabilities` with `CapabilityState` enum (Yes, No, Unknown), `CompressionEligibility` enum and `classify_command()`, and `RtkProjector` implementing `CommandOutputProjector` (skeleton). `RtkProjector::project()` returns `ProjectionError::BackendUnavailable` — the skeleton does NOT produce fake placeholder output. `ProjectionSelector::project()` falls back to safe projection on error and records a warning. `ProjectionSelector::with_rtk()` conditionally includes the RTK projector; `with_config()` reads `ShellOutputConfig` to build the selector. `ProjectionError::BackendUnavailable` handles unprobed discovery. Phase 5 adds RTK discovery, capability probing, eligibility classification, and an RtkProjector skeleton behind the projection abstraction.
- **RTK invocation (Phase 6)**: `src/shell/rtk.rs` replaces the skeleton with real invocation. `RtkInvocationMode` enum (PostProcess, Wrapper, Disabled) controls how RTK processes output. `RtkCapabilities::invocation_mode()` prefers PostProcess, falls back to Wrapper, defaults to Disabled. `probe_capabilities()` now probes stdin-piped post-process and wrapped-command modes, with structured `RtkCapabilityDiagnostics` (per-probe `ProbeOutcome`: Confirmed/Denied/Failed/Skipped) and help-text detection heuristic for PostProcess mode. `RtkProjector::project_post_process()` pipes captured stdout/stderr to RTK via stdin with 1 MiB input cap and configurable timeout. `RtkProjector::project_wrapper()` runs `rtk <command>` for eligible read-only commands; uses `argv` when available instead of whitespace splitting, and propagates `cwd` from the original command. Both return `ProjectionKind::ExternalCompressed` / `ProjectionExactness::Lossy` on success, or `ProjectionError::BackendUnavailable` on failure (selector falls back to safe projection). RTK remains disabled by default; native projectors still win by default. **Strict wrapper grammar (WS3)**: When `argv` is unavailable, `parse_simple_wrapper_command()` rejects shell metacharacters, quotes, pipes, redirects, env assignments, and command substitution. Complex commands without `argv` return `BackendUnavailable`. **Structured raw semantics (WS4)**: `ProjectionRawSemantics` on `ProjectionResult` distinguishes `OriginalCommandRaw`, `WrappedCommandRaw`, `OriginalRawUnavailable`, and `Unknown`. RTK wrapper mode sets `WrappedCommandRaw` (non-partial) or `OriginalRawUnavailable` (partial). **User-facing RTK status**: `RtkStatusSummary` provides multi-line status display via `RtkDiscovery::status_summary()`. **Stderr warning cap**: `MAX_STDERR_WARNING_BYTES = 512` prevents excessive context bloat. **RTK integration tests**: Env-gated via `CODEGG_RTK_INTEGRATION=1`; not part of standard CI.
- **Expansion handles and TUI UX (Phase 7)**: `CommandOutputStore::expand()` / `expand_stream()` expand retained raw output by handle. `CommandOutputExpansion` carries text, `ExpansionExactness` (Exact/LossyUtf8/Unavailable), byte counts, and warnings. `/shell-expand <id|last> stdout|stderr [start..end]` is the TUI command. Shell detail dialog shows projection metadata (projector, exactness, omitted ranges, expansion handles as `cmd://` URLs). `e` keybinding in detail dialog triggers expand. Expansion is local-only unless explicitly promoted. Expansion handle round-trip tests verify handles resolve to correct raw bytes.
- **Redaction pipeline (Phase 8)**: `src/shell/redactor.rs` implements the `Redactor` with six `RedactRule` implementations: `AuthorizationRule`, `EnvSecretRule`, `PemBlockRule`, `CloudCredentialRule`, `EmbeddedCredentialUrlRule`, `SessionMaterialRule`. `apply_redaction_hook` in `src/shell/projector.rs` calls `Redactor::new().redact()` and sets `RedactionState::Applied { replacements }` or `AppliedNoMatches`. The call site lives in `ProjectionSelector::project` so redaction cannot be bypassed by RTK or native projectors. `RedactionState` has six variants: `NotApplied`, `HookAppliedNoRules` (legacy), `Applied { replacements }`, `AppliedNoMatches`, `SkippedByPolicy`, `Unavailable`. Redaction is now applied only inside `ProjectionSelector::project()` — one authoritative coordinator. `config_command_projection()` does NOT apply redaction separately (the previous duplicate call was removed to prevent overwriting `RedactionState::Applied { replacements: N }` with `AppliedNoMatches`). The redaction test suite now includes false-positive, long-line, multiple-credentials-per-line, and edge-case tests.
- **Phase 09 projection contract**: `ProjectionResult` now includes `projection_id: ProjectionId`, `source_spans: Vec<ArtifactSpanRef>`, `redaction_records: Vec<RedactionRecord>`, and `rtk_metadata: RtkResultMetadata`. `ProjectionRecord` in run_store carries full projection metadata including source spans, redaction records, RTK metadata, and promotion decisions. `evaluate_promotion()` determines `PromotionDecision` based on budget, redaction state, and critical spans. `preferred_projector_for_run_kind()` maps `RunKind` to the preferred projector. Python projection now has `PythonProjector` implementing `CommandOutputProjector` and `project_python_result()` for converting `PythonRunResult` to `ProjectionResult`.
- **Context budget and compaction integration (Phase 10)**: `ProjectionContextMetadata` and `ProjectionFact` types carry critical facts (failed tests, error codes, diagnostic spans, changed files, redaction state) for compaction preservation. `ModelTier` (Mini/Workhorse/Frontier) and `ContextAwareBudget` provide model-tier-aware token budgets. `ProjectionResult::to_context_metadata()` extracts metadata for the compaction system. `is_already_projected` flag prevents double compression. `extract_critical_facts()` scans projected text for patterns. Tests in `tests/shell_projection_phase10.rs`.

### Command Intent and Planning

- **Command intent classification and routing metadata**: `classify_command_with_context(command, ctx)` in `src/command_intent/mod.rs` is the primary classifier — classifies commands into intent families (Test, GitReadOnly, GitMutating, SearchReadOnly, PythonAnalyze/Transform/Verify, Build, Lint, Format, RawShell, Rejected) using workspace-aware path checks. `classify_command()` is a backward-compatible wrapper that delegates to `classify_command_with_context` with a default context (process cwd fallback). In Phase 10, `BashTool::execute()` classifies commands, plans execution, validates for active routing, and dispatches to structured backends when enabled. Active test routing invokes the canonical TestRunner (no raw shell); active Python routing invokes the canonical PythonScript executor (no direct `python3 -c`). `RunOwnership::DelegatedBackend` is set only when the delegated subsystem actually executed and persisted a run (carrying a `RunId` proof); without a run ID, BashTool falls back to caller-owned persistence. `TestScope::BashDispatch(Vec<String>)` (`src/test_runner/types.rs:13`) is used by BashTool's dispatch to bypass allowlist re-validation — the argv has already passed planner classification. `run_kind_for_outcome()` (`src/command_outcome.rs:226-260`) maps `ActualExecutor::RawShell` unconditionally to `RunKind::RawShell` for all intents; semantic intent is preserved via `planned_backend`, routing metadata, and intent kind.
- **Active routing mode**: `CommandIntentMode::Active` enables dispatch to structured backends (TestRunner, NativeTool, PythonScript, ManagedProcess) instead of raw shell. Default mode is `Observe` (classify + annotate only). Per-family overrides via `route_build`, `route_lint`, `route_format` fields.
- **Kill switches**: `CODEGG_ROUTING_DISABLE=1` env var globally disables active routing. Per-family `RouteLevel::Off` disables routing for specific families.
- **Validation gate**: `validate_for_active_routing()` checks 7 conditions (SimpleArgv, High confidence, non-RawShell/Reject, non-Critical risk, no DestructiveFileMutation, no OutsideWorkspace, no pending permissions) before dispatching. Failed validation falls back to raw shell.
- **Git unified routing (Phase B)**: All git commands (reads and mutations) route through `ExecutionBackend::Git { request: GitExecutionRequest }` → `RoutingDecision::RouteToGit { request, timeout_secs }`. `GitExecutionRequest` carries typed `GitOperation` from codegg-git, argv, origin, risk set, and repository root. `classify_git()` delegates to `codegg_git::parse_git_argv()` for accurate risk assessment via `GitRiskClass`, with fallback to lightweight heuristics on parser failure. Dangerous operations (push, reset --hard, clean -f) route through the Git backend with high-risk policy rather than falling back to RawShell.
- **CommandIntentContext**: `CommandIntentContext { workspace_root: Option<PathBuf>, cwd: Option<PathBuf> }` carries workspace boundary information for path containment checks. `classify_search_with_context()` and `classify_file_read_with_context()` reject absolute outside-workspace paths from safe classification. Helper functions: `canonical_workspace_root()`, `path_is_inside_workspace()`, `absolute_path_outside_workspace()`. Active routing must use contextual classification, not bare process-cwd classification.
- **ShellShape parsing**: `ShellShape` enum (`Empty | SimpleArgv(Vec<String>) | ComplexShell`) in `src/command_intent/shell_shape.rs`. `parse_shell_words()` handles quotes, escapes, operators, redirection, command substitution, variable expansion. `CommandIntent` has `parsed_argv: Option<Vec<String>>` field. Classifier uses parsed argv for all `looks_like_*` functions. Planner uses parsed argv for ManagedArgv and TestRunner backends.
- **Git classification tightened**: `classify_git()` uses parsed argv, not string prefixes. Delegates to `codegg_git::parse_git_argv()` for accurate risk assessment via `GitRiskClass`, with fallback to lightweight heuristics on parser failure. `git branch`, `git tag`, `git remote` are only read-only for specific forms (--list, -l, -v, --show-current). Mutating forms (branch <name>, tag <name>, remote add/remove) correctly classified as GitMutating. `git push`, `git reset --hard`, `git clean -f` classified as High risk.
- **Search/read classification tightened**: `find -exec`, `-delete`, `-ok`, `-execdir` rejected from safe search. Absolute outside-workspace paths rejected from safe search/file-read. `which`/`whereis` no longer classified as file reads. `classify_search()` and `classify_file_read()` return Option for fallthrough.
- **Shell operator detection**: `has_shell_operators()` in `src/command_intent/mod.rs` uses quote-aware scanning to detect `|`, `;`, `$`, `` ` ``, `&`, `&&`, `||` outside quotes. Commands with operators are classified as `RawShell` (prevents `cargo test && rm -rf .` routing to TestRunner).
- **RiskAssessment constructors**: `RiskAssessment` has specific constructors: `read_only()` (no Subprocess), `raw_shell()` (with Subprocess), `managed_process()` (no Subprocess), `git_mutation()` (with GitMutation), `destructive()` (with DestructiveFileMutation). Generic `low()`/`medium()`/`high()` are retained for backward compat. `Subprocess` means the command may spawn child processes beyond the primary planned execution, not simply that codegg will execute a process.
- **Command planner maps intent to backend**: `plan_execution()` in `src/command_intent/plan.rs` maps classified intents to `ExecutionBackend` (RawShell, ManagedArgv, NativeTool, TestRunner, PythonScript, Git, Reject) with rich struct variants carrying metadata. Re-exports from `src/command_planner.rs`.
- **Plan includes projector and RTK metadata**: `CommandPlan` now carries `ProjectorRoute` (Raw, Truncated, ErrorRetention, GitStatus/Diff/Log, TestReport, FileSearch, PythonRun, RtkEligible) and `PlanRtkPolicy` (Disabled, Eligible, RequiredForPromotion).
- **Permission planning**: `CommandPermissionRequest` carries `PermissionDefault` (Allow, Ask, Deny) per capability. `generate_permission_requests()` maps `ExecutionCapability` → permission request with context-aware defaults. `DestructiveFileMutation` → `Deny`; `OutsideWorkspace` → `Deny`; `DependencyInstall` → `Deny`. `GitMutation` → `Allow` for `git add` only; `Ask` for all others (commit, checkout, switch, restore, stash push, merge, rebase, etc.). `WriteWorkspace` → `Ask` for writing formatters (`cargo fmt`, `prettier`, `black`, `isort`), `Allow` for read-only formatters (`--check`, `--diff`), `Ask` for other writes. `ReadWorkspace`, `Subprocess`, `EnvAccess`, `ContextPromotion` → `Allow`. `Network` → `Ask`.
- **Command routing resolves to subsystem**: `resolve_routing()` in `src/command_routing.rs` maps planned execution to concrete `RoutingDecision` variants (RouteToTestRunner, RouteToShell, RouteToPythonScripting, RouteToNativeTool, RouteToManagedProcess, RouteToGit, Rejected). Git backend routes to RouteToGit.
- **Python scripting is first-class**: `src/python_script/` is the sole canonical Python scripting module. `PythonScript` supports Analyze/Transform/Verify modes with `PythonCapabilityEnvelope` (9-field sandbox), `PythonCapabilityProfile` (per-mode filesystem roots, subprocess rules, sandbox requirements), `WorkspaceSnapshot` for transform diffing, `PythonScriptTool` (model-facing), and `project_python_run()` projection. Python is NOT hidden inside bash.
- **Capability enforcement**: `execute_python_script()` calls `resolve_policy()` for full policy resolution (AST risk → profile → enforcement backend) and `check_compatibility()` for legacy evidence — denied capabilities block execution. Landlock filesystem sandboxing on Linux (`build_landlock_allowed_paths()` + `build_landlock_deny_paths()` + `cmd.pre_exec()`). Portable fallback uses env_clear + cwd containment + snapshot detection. Snapshots are taken for ALL modes (Analyze, Transform, Verify), not just Transform. Analyze and Verify modes fail if ANY workspace files change. Capability checks distinguish file reads from file writes: `has_file_read` with `read_workspace`, `has_file_write` with `write_workspace`, destructive ops with `destructive_fs`. `PythonRunResult` carries enforcement evidence fields (`policy_decision`, `denied_capabilities`, `os_filesystem_isolation`, `effective_read_roots`, `effective_write_roots`, `allowed_subprocesses`, `enforcement_warnings`).
- **Environment hardening**: CWD validated (must exist, must be directory, defaults to current dir). `workspace_root: Option<PathBuf>` on `PythonScriptRequest` provides the authoritative workspace boundary — when set, CWD must be inside this root. Environment cleared via `.env_clear()` with only PATH, HOME, LANG, LC_ALL, VIRTUAL_ENV, PYTHONPATH, DYLD_LIBRARY_PATH restored.
- **AST-aware risk scanning**: `analyze_python_risk()` tries AST scanning first via `python3 -I` with stdin, falls back to string scanning if Python unavailable or parse fails. `PythonRiskAssessment` has `scanner: PythonRiskScanner` field (Ast | Fallback). Builds alias maps to resolve `import subprocess as sp; sp.run(...)` and `from subprocess import run; run(...)` forms through their aliases. Detects pathlib write_text/read_text/unlink, false positive reduction.
- **Python run labels (not artifact handles)**: `PythonRunResult` has `stdout_label`/`stderr_label`/`diff_label` — these are pseudo-local run identifiers, NOT registered in any artifact store and NOT expandable via `context_read` or other tools.
- **Conservative classifier**: The classifier recognizes simple argv-shaped commands and falls back to `RawShell` for complex cases (pipes, redirection, command substitution). It does NOT attempt full POSIX shell parsing.
- **Package manager safety boundary**: Package managers (`npm install`, `pip install`, `cargo install`) are classified as `RawShell`, NOT Build. They mutate global state and must not be auto-routed.
- **RtkProjectionPolicy**: Added to `src/shell/projector.rs` for controlling RTK projection behavior (Disabled/PostProcessOnly/WrapperOnly/Both).

### Context Policy

- Context policy is **disabled by default** (`observe` mode). Config via `[context_policy]`.
- Volatile-tail compaction is **disabled by default** (`observe` mode).

## Architecture Docs

`architecture/` has 44 docs covering every module. See `architecture/overview.md` for the full module map and navigation index. Key ones:

| Document | Key Gotchas |
|----------|-------------|
| `architecture/overview.md` | Module map, verified counts (105 commands, 42 events, 39 LSP servers, ~37 tools, 9 agents) |
| `architecture/agent.md` | AgentLoop has ~49 fields at `src/agent/loop.rs:1380` |
| `architecture/bus.md` | 42 AppEvent variants; PermissionRegistry/QuestionRegistry are synchronous |
| `architecture/lsp.md` | egglsp is authoritative; 39 servers; `src/lsp/` is thin re-export shim |
| `architecture/plugin.md` | No `wasm.rs`; `marketplace.rs` exists; PluginRuntime trait with Process/Wasm/Builtin |
| `architecture/tool.md` | ~38 tools in default registry; `ToolCatalog::register()` takes `&dyn Tool` |
| `architecture/deterministic_tools.md` | Eggsact in-process deterministic tools (8 always-visible + 5 deferred); trust model, registration, preflight integration |
| `architecture/tui.md` | `src/tui/app/mod.rs` ~13K lines; async command pattern; TuiTaskRegistry lifecycle |
| `architecture/human_shell.md` | ! commands not in model context unless promoted; Phases 1-10 projection pipeline |
| `architecture/command_intent.md` | Command intent classification, risk assessment, execution capability model |
| `architecture/command_planner.md` | Backend routing, permission generation, projector/RTK policy selection |
| `architecture/command_routing.md` | Routing resolution mapping planned execution to concrete subsystems |
| `architecture/python_scripting.md` | First-class Python scripting with Analyze/Transform/Verify modes, AST-aware risk analysis, capability enforcement, env hardening — sole canonical module at `src/python_script/` |
| `architecture/python_script.md` | Module-based Python scripting: types, sandbox, executor, projection, tool registration |
| `architecture/command.md` | 105 built-in slash commands |
| `architecture/config.md` | Config schema in `crates/codegg-config/src/schema.rs` |
| `architecture/provider.md` | 16 auto-registered providers via env vars; CircuitBreaker pattern |
| `architecture/preflight.md` | Harness-side eggsact preflight: types, policy config, tool integration, anti-recursion |

`.agents/skills/*/SKILL.md` contain 45 module-specific skill guides loaded on-demand via `/skill:`.

## Key Lessons

1. **Verify claims against code** — Many "bugs" in docs turned out to be correct after inspection.
2. **Documentation goes stale** — Struct fields get added/removed; always compare docs to source.

## Where New Components Belong

### New Web Search Providers
Add to the **eggsearch** project, not to Codegg's built-in search provider registry (`src/search/`). The built-in registry is legacy fallback only. Codegg owns the wrapper UX, permissioning, output caps, trust framing, and backend selection; the actual search/fetch logic lives in eggsearch.

### New Deterministic Validators
Add to the **eggsact** crate. The eggsact project owns the validation logic. Codegg's `EggsactTool` wrapper in `src/tool/deterministic.rs` exposes eggsact tools to the model. New tools need:
1. Implementation in eggsact with the `codegg_core` profile
2. Registration in `build_eggsact_tools()` in `src/tool/deterministic.rs`
3. Category assignment (always-visible vs deferred)

### New LSP Servers
Add to `crates/egglsp/src/server.rs`. Each server needs a `LspRule` entry with command, extensions, and initialization options. See `architecture/lsp.md` for the full contract.

### New Native Tool Crates
Follow the library-first, MCP-second pattern in `architecture/native_crates.md`. Durable tool domains live in workspace crates under `crates/` and are consumed directly in-process by Codegg's tool wrappers.
### New Git Operations

`codegg-git` (`crates/codegg-git`) is the typed Git operation model, argv parser, and risk classification crate. Phase B (command intent/routing) consumes these types directly — `classify_git()` delegates to `codegg_git::parse_git_argv()` and `ExecutionBackend::Git` carries the typed `GitOperation`. See `architecture/command_intent.md` for the classification contract.

3. **Line numbers are fragile** — References like `watcher.rs:157` can be off by several lines. Use code search.
4. **Count from source, not docs** — Tool/server/command counts drift. Count actual entries in `with_options()`, `server_definitions()`, `CommandRegistry`.
5. **Don't assume tool registration** — Not every tool in `/tool` is in the default registry.
