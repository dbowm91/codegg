# AGENTS.md

## Quick Start

Rust 1.81+ required. Edition 2021. Tokio async runtime.

```bash
cargo build --all-features           # build
cargo clippy --all-features -- -D warnings  # lint (errors in CI)
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14  # full suite, capped
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
| `codegg-core` | Domain types: bus, error, goal, memory, migration, run_store, session, storage, snapshot, worktree, workspace, workspace_services, task_state, model_profile, resilience, protocol_conversions |
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

See `architecture/testing.md` for the full test resource taxonomy, Tokio runtime flavor rules, pool strategy guidance, test selection pattern, and capped full-suite command.

## Static Guards

Run these after changing execution surfaces, agent definitions, or codegg-core:

```bash
python3 scripts/check-core-boundary.sh              # codegg-core boundary enforcement
python3 scripts/check_daemon_cwd_usage.py           # workspace-bound daemon path guard
python3 scripts/check_project_agent_pwd_inference.py # project-agent PWD-inference guard (Runtime Assets M2)
python3 scripts/check_discovery_invariants.py       # bounded project-discovery safety guard
python3 scripts/check_project_catalog_invariants.py # project-catalog and discovery invariants
python3 scripts/check_scheduler_bypass.py           # scheduler-bypass guard
python3 scripts/check_execution_ownership.py        # process-spawn site ownership manifest
python3 scripts/check_git_forbidden_patterns.py     # git secret boundary + policy drift
scripts/check_provider_connections_m4_coverage.sh   # provider lifecycle/protocol coverage
scripts/check_provider_connections_tombstone_compat.sh # additive tombstone/reference guard
python3 scripts/check_builtin_agents.py             # verify TOML matches generated.rs
python3 scripts/check-tokio-test-flavors.py         # regression guard for bare #[tokio::test]
python3 scripts/generate_builtin_agents.py --check  # agent asset staleness + schema validation
bash scripts/check_projection_disclosure.sh          # projection disclosure encapsulation guard (M3)
python3 scripts/check_projection_transport_isolation.py # raw projection transport isolation guard (M5)
python3 scripts/check_websocket_bounds.py             # reject unbounded server WebSocket channels (M6)
```

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
cargo test --test git_execution_origin_matrix
python3 scripts/check_git_forbidden_patterns.py
cargo test -p egglsp

# TUI render regression tests (headless, no terminal needed)
cargo test --test tui_render

# TUI unit/integration tests
cargo test --test tui

# Shell projection tests
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
cargo test -p codegg --lib shell::redactor
cargo test -p codegg --lib shell::rtk

# Test runner module (resolver, parser, report formatter, previous-failures index)
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib test_runner::projection
cargo test -p codegg --lib test_runner::custom
cargo test -p codegg --lib test_runner::index
cargo test -p codegg --lib tool::test
cargo test -p codegg --lib test_runner::runner::tests
cargo test -p codegg --lib tui::commands::test
cargo test -p codegg --lib async_request

# LSP integration (fake server, no network, needs lsp-test-support)
cargo test -p egglsp --features lsp-test-support --test scenario_engine
cargo test --features lsp-test-support --test lsp_composite_stdio

# Real-server smoke tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer --nocapture

# Plugin example SDKs
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml
PYTHONPATH=examples/plugins/sdk-python python3 -m unittest discover examples/plugins/sdk-python/tests -v
cargo build --target wasm32-unknown-unknown --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release

# Eggsact adapter integration tests
cargo test --test eggsact_adapter
cargo test --test eggsact_deterministic_tools

# Eggsearch adapter/unit tests
cargo test -p codegg --lib search_backend::eggsearch
cargo test -p codegg --lib search_backend::bootstrap
cargo test --test fake_eggsearch_mcp
cargo test --test search_backend_eggsearch
cargo test --test search_backend_arg_mapping
cargo test --test preflight_integration
cargo test -p codegg --lib search_backend::framing

# Command intent/planner/routing
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib command_intent::shell_shape
cargo test -p codegg --lib command_planner
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::bash

# Adversarial tests
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
cargo test --test command_routing_execution_ownership

# Projection disclosure and artifact handle tests (M3)
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles

# Phase 4–5 durable jobs + scheduler tests
cargo test --test durable_jobs_phase4
cargo test --test scheduler_submission_idempotency
cargo test --test scheduler_permit_lifecycle
cargo test --test scheduler_cancellation
cargo test --test scheduler_restart_recovery
cargo test --test scheduler_contention
cargo test --test scheduler_authority_matrix
cargo test --test managed_process_descendants
cargo test --test scheduler_resource_profiles
cargo test --test scheduler_protocol_consistency

# Phase 09 projection contract tests
cargo test -p codegg --lib shell::projector -- projection_id
cargo test -p codegg --lib shell::projector -- span_role
cargo test -p codegg --lib shell::projector -- promotion
cargo test -p codegg --lib shell::projector -- preferred_projector
cargo test -p codegg --lib python_script::projection
cargo test -p codegg --lib test_runner::projection

# Tokio flavor audit
python3 scripts/audit_tokio_tests.py
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

CI runs on push/PR to dev/main. Independent jobs: `agent-assets`, `fmt`, `check`, `clippy`, `test`, `audit`. Then `plugin-focused` (depends on fmt/check/clippy/test) runs plugin install/management/registry/TUI tests, the core boundary check, and the `check_scheduler_bypass.py` static guard. `examples` (depends on plugin-focused) tests SDKs and WASM builds. `build-cross` (depends on plugin-focused) builds release binaries for linux-x86_64, linux-aarch64, darwin-x86_64, darwin-aarch64. The `agent-assets` job validates built-in agent TOML schemas and checks for stale generated output. The `test` job runs the full workspace test suite plus explicit shell projection validation steps (harness, context budget, redactor, RTK unit tests) and the `check_execution_ownership.py` static guard. Local equivalent: `scripts/validate_plugin_ui.sh`.

## Critical Gotchas

### Sync vs Async

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT `await` them.
- **Registration-before-publish**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event.

### Daemon lifecycle

- **Singleton invariant**: Exactly one user-scoped Codegg daemon is active per OS user. The lock is at `daemon.lock` in the user-scoped runtime directory (macOS: `$HOME/Library/Application Support/codegg`, Linux: `${XDG_RUNTIME_DIR:-/tmp}/codegg`). Override with `CODEGG_DAEMON_HOME`.
- **Connect-or-start default**: Plain `codegg` uses `connect_or_start_daemon` (`src/core/instance.rs`) to connect to the running daemon or auto-start one. `--standalone` runs an in-process core; `--stdio` uses the hidden `core-stdio` path. `--core-transport inproc|stdio` is deprecated and emits a warning.
- **Server requires `--standalone-core`**: The HTTP server does not silently construct its own daemon. Without `--standalone-core`, it exits with an actionable error.
- **Lock is authoritative**: `DaemonInstanceGuard` holds `flock(LOCK_EX | LOCK_NB)` for the daemon's lifetime. `daemon.json` metadata is diagnostic only.
- **PID file is legacy**: The old `<socket>.pid` file is still written for backward compat, but the authoritative identity is the metadata record + lock.
- **Stop verifies liveness**: `daemon stop` probes the socket before sending SIGTERM. It refuses to unlink paths it does not own.
- See `src/core/instance.rs` for the full contract.

### Workspace Registry (Phase 2)

- **WorkspaceId**: typed `String` newtype in `crates/codegg-core/src/workspace.rs`.
- **WorkspaceRegistry**: daemon-owned, deduplicates canonical roots. Rejects nonexistent paths and symlink aliases.
- **ExecutionContext**: immutable, passed by `Arc` through `TurnRunInput`. Replaces `std::env::current_dir()` reasoning. Carries `workspace_root`, `workspace_id`, `session_id`, and path policy.
- **Static guard**: `scripts/check_daemon_cwd_usage.py` scans protected modules for `std::env::current_dir()` usage. New production-path uses fail CI.
- See `crates/codegg-core/src/workspace.rs` for the full contract.

### Workspace Services and Storage (Phase 3)

- **WorkspaceServices**: per-workspace bundle owning `Arc<dyn RunStore>`, `Arc<WorkspacePathPolicy>`, `Arc<WorkspaceLockTable>`, `Arc<WorkspaceConfigSnapshot>`. Constructed by `ProductionWorkspaceServicesFactory` at `<workspace>/.codegg/runs/`.
- **Storage split** (`crates/codegg-core/src/storage/mod.rs`): `init_daemon_catalog(&DaemonPaths)` owns the user-scoped catalog. `init_legacy_project_store(root)` retains backward compat. `init` is deprecated.
- **STORAGE_LAYOUT_VERSION = 32**. **DaemonPaths** (`crates/codegg-core/src/storage/paths.rs`) is the single source of truth for catalog and asset paths.
- **Migration tooling** (`crates/codegg-core/src/migration.rs`): `migrate_legacy_project_database` is idempotent.
- See `crates/codegg-core/src/workspace_services.rs` for the full contract.

### Durable Jobs and Schedules (Phase 4)

- **Typed IDs**: `JobId`, `AttemptId`, `ScheduleId`, `DependencyId`, `DaemonGeneration` — opaque UUID strings, never parsed as integers.
- **JobState machine**: Terminal states never regress. Transitions enforced via `validate_state_transition` (`crates/codegg-core/src/jobs/store.rs:65`).
- **AttemptState machine**: Terminal states never regress. Transitions enforced via `validate_attempt_transition` (`crates/codegg-core/src/jobs/store.rs:114`).
- **Daemon generation recovery**: `recover_generation` marks all attempts whose `daemon_generation` ≠ current as `Interrupted`. Requeues iff `RecoveryPolicy` permits based on `IdempotencyClass`.
- **Idempotency**: `IdempotencyClass::is_retry_eligible()` returns `true` for `ReadOnly` and `SafeRepeat`. Persisted at creation time.
- **JobStore/ScheduleStore traits**: Live in `crates/codegg-core/src/jobs/`. UI/server/plugin/auth-free (boundary enforced by `scripts/check-core-boundary.sh`).
- **RunStore linkage**: `JobAttempt.run_id: Option<RunId>` links attempt to RunStore. RunStore is NOT the queue authority.
- See `crates/codegg-core/src/jobs/mod.rs` for the full contract.

### Global Admission Control Scheduler (Phase 5)

- **Submission boundary**: `JobSubmissionService` (`src/scheduler/submission.rs`) is the daemon-owned facade. Callers must not create a job and separately dispatch it.
- **Single authority**: `JobScheduler` (`src/scheduler/scheduler.rs`) is the only daemon admission authority for submitted work.
- **Static guards**: `scripts/check_scheduler_bypass.py` rejects direct TestRunner calls, legacy subagent sends, and background scheduler loop starts outside explicit sites. `scripts/check_execution_ownership.py` enforces the machine-readable `docs/execution-ownership.toml` manifest. `scripts/check_daemon_cwd_usage.py` remains required for workspace-bound daemon paths. Run all after changing any execution surface.
- See `architecture/scheduler.md`, `src/scheduler/`, and `tests/scheduler_phase5.rs`.

### Execution Ownership Inventory

- **Manifest**: `docs/execution-ownership.toml` is the machine-readable inventory of all production process-spawn sites.
- **Static guard**: `scripts/check_execution_ownership.py` greps for canonical spawn patterns and fails CI on unclassified sites.
- **Owner classes**: `scheduler`, `interactive`, `standalone_compat`, `definition_or_adapter`, `deferred_domain_executor`, `test_only`, `forbidden_bypass` (must be fixed; guard fails).
- See `scripts/check_execution_ownership.py` and `docs/execution-ownership.toml`.

### Module Splits

- **Error enums** live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports + adds `AxumAppError`/`AxumServerRuntimeError` behind `#[cfg(feature = "server")]`.
- **protocol_conversions**: Core conversions in `crates/codegg-core/src/protocol_conversions.rs`. Root re-exports core via `pub use codegg_core::protocol_conversions::*;`.
- **Protocol is a re-export**: `src/protocol/` deleted. `src/lib.rs` has `pub use codegg_protocol as protocol;`. Use `codegg_protocol::dto` types.
- **Provider is a re-export**: `src/provider/` re-exports from `crates/codegg-providers` as `codegg::provider`.

### TUI

- **TUI render.rs doesn't exist**: `src/tui/app/` contains `mod.rs` (~13K lines) and `types.rs`. Command handlers are in `src/tui/commands/` (20 submodules). Runtime is in `src/tui/runtime/`.
- **Custom test command validation is strict argv-prefix**: `src/test_runner/custom.rs::validate_custom_command` is the single source of truth. Rejects shell metacharacters. Argv-token-bounded match, so `pytestevil` and `cargo testify` do NOT match. Both generated and custom commands execute via `Command::new(argv[0]).args(&argv[1..])` — never via a shell.
- **Previous-failures index**: `.codegg/test-runs/index.json` stores up to 100 recent test run entries. Written atomically after every test run.
- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum.
- **DialogType is in component.rs**, not `types.rs`. FocusManager is in `component/focus.rs`.
- **Dialog::Plugin is generic**: A single `Dialog::Plugin` variant handles all plugin dialogs.
- **Async command pattern**: High-latency handlers use spawn-and-complete via `spawn_tui_task`. New apply handlers MUST use `state.finish(request_id)` / `state.fail(request_id, err)` guard pattern and add a stale-completion test. See `src/tui/async_cmd.rs`.
- **Sync dispatch is the rule**: `src/tui/runtime/command_dispatch.rs` arms are all `fn` (non-async). New dispatch arms should NOT add `.await`.
- **Long output goes to info dialog**: `App::show_short_or_info(info_type, lines)` toasts when ≤3 lines, otherwise opens scrollable `InfoDialog`.
- **Background task lifecycle**: `TuiTaskRegistry` on `App` tracks spawned tasks. Use `spawn_registered_tui_task(tx, registry, kind, name, fut)`.
- **Git sidebar is cached, not rendered live**: `GitSidebarState` caches git info. Stale generations dropped silently.
- **Remote TUI protocol is event/state-driven**: The `/tui` WebSocket uses `TuiCommand` enum. `RenderFrame` is unsupported.

### Tool Registry

- **ToolCatalog::register() takes `&dyn Tool`**, not `Box<dyn Tool>`.
- **multiedit tool exists but NOT in default registry**: `src/tool/multiedit.rs` exists, `pub mod multiedit` is registered, but it's NOT in `ToolRegistry::with_defaults()`.
- **~37 tools** in `ToolRegistry::with_options()` (`src/tool/mod.rs`). Count varies by config. Includes 8 always-visible eggsact deterministic tools.
- **Tool session constructor**: `with_session_config_defaults(&Config, ...)` is the production constructor. `with_session_defaults(...)` is the legacy all-native fallback.
- **Integrated tool config (Phase 6)**: `src/tool/integrated_config.rs` resolves evidence/deterministic/preflight runtime configs once from `Config`. Subagents use `with_config(&config)` (`src/agent/worker.rs:698`) to inherit backend config.
- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` is used by both `apply_patch` tool and LSP preview operations.
- **eggsact is in-process, not MCP**: The `eggsact` dependency is consumed as a direct Rust dependency (`src/eggsact/adapter.rs`). `EggsactRuntime` wraps `eggsact::agent::ToolRegistry` in-process. Provenance must tag `backend = "native"`, `implementation = "eggsact/<tool_name>"`, `trust = LocalTrusted`.
- **Deterministic tools**: `EggsactTool` generic wrapper in `src/tool/deterministic.rs` exposes 8 always-visible tools (`text_equal`, `text_diff_explain`, `text_replace_check`, `validate_json`, `validate_toml`, `command_preflight`, `path_normalize`, `text_security_inspect`) plus 5 deferred tools. Registered best-effort; if `EggsactRuntime::new()` fails, tools are silently skipped.
- **Preflight**: `src/preflight/` provides harness-side automatic validation before mutating operations using eggsact. **Harness-internal only** — not model-facing. Findings are severity-classified (`Block`/`Warn`/`Annotate`).
- **CommandIntentMode**: `Observe | Active | deprecated Route` with default `Observe`. `Active` enables dispatch to structured backends. `route_safe_commands = true` alone does NOT enable active routing.

### Agent Runtime

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` via `deps.turn_runtime`.
- **AgentLoop has ~49 fields** at `src/agent/loop.rs:1380`. Many docs claim 15.
- **AgentLoopFactory** (`src/agent/agent_loop_factory.rs`) is a build-only seam.
- **CoreRuntimeDeps** (`src/core/runtime_deps.rs`): Bundles pool, memory_store, legacy_agent, turn_runtime.
- **AgentRegistry** (`src/agent/registry.rs`): Central registry separating declarative sources from resolved runtime agents. Prefer over `resolve_agents()`.
- **Emergency fallback model**: `EMERGENCY_DEFAULT_MODEL` in `src/agent/mod.rs`. Users should always configure models explicitly.

### LSP

- **egglsp is authoritative**: `src/lsp/` is a thin shim. All real LSP logic lives in `crates/egglsp/`.
- **39 LSP servers** configured in `crates/egglsp/src/server.rs`.
- **Preview-only boundary**: `renamePreview`, `formatPreview`, `sourceActionPreview` never write to disk.
- **LSP tests need `lsp-test-support` feature**: The fake server binary is `codegg-lsp-test-server`. Tests use polling loops, not fixed sleeps.
- **Preview apply (Phase 9)**: `/lsp-preview-apply` applies patches with SHA-256 hash revalidation. `LspTool` remains read-only.
- **LSP semantic cache** is opt-in and disabled by default. Config via `[lsp_semantic_cache]`.

### Plugin System

- **Plugin UI types** live in `codegg_protocol::ui` (`UiNode`, `UiEffect`).
- **PluginRuntime trait** (`src/plugin/runtime/`): `ProcessRuntime`, `WasmRuntime`, `BuiltinRuntime`. WASM requires `plugins` feature flag.
- **PluginRegistry** indexes by capability. Duplicate command names rejected.
- **PluginManager** (`src/plugin/management.rs`) is the canonical API: `list()`, `info()`, `enable()`, `disable()`, `doctor()`, `remove()`, `install_from_path()`, `uninstall()`.
- **Plugin install validation**: `src/plugin/install.rs` does lexical path traversal checks BEFORE canonicalizing. Rejects symlinks, hardlinks, and absolute paths.
- **Plugin security policy**: `PluginPolicy` in `src/plugin/policy.rs`. All default to conservative. Policy is opt-in.
- **Plugin SDKs**: `examples/plugins/sdk-rust/` (11 tests) and `examples/plugins/sdk-python/` (24 tests).

### Auth

- **ExternalCommand is disabled**: Both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported`.
- **Credential store**: `~/.config/codegg/credentials.json`. Requires `CODEGG_MASTER_KEY` to store new credentials.
- **Provider registration**: Adding ANY provider via config disables all env-var auto-registration.
- **Auth logging**: Never log secret prefix/suffix/length.

### Security

- **Security review workflow** (`src/security/workflow/`): Read-only, never mutates files.
- **Security finding synthesis**: Evidence-based, requires 2+ evidence dimensions. Same-file scoping only.

### Git

- **GitExecutionService is the canonical read executor**: `src/git_service.rs`. Delegates to `egggit` for structured parsing; falls back to subprocess for mutations. Downstream consumers should consume `GitPayload` variants.
- **egggit is read-only, mutations stay in Codegg**: `egggit` modules never mutate. All mutations handled by `git_mutations` executor (`src/git_mutations.rs`).
- **status_v2 replaces raw status parsing**: `status_v2::rich_status()` returns `RichRepoStatus`. Legacy `status::RepoStatus` remains for backward compat.
- **GitTool structured-first execution**: `src/tool/git.rs` attempts structured execution before raw subprocess.
- **Phase D: typed mutations**: `src/tool/git.rs` accepts `mutation` action. All mutations route through `GitMutationExecutor` with snapshot/delta/RunStore persistence.
- **Phase E: network/config/destructive**: `src/git_network_policy.rs` and `src/git_network_ops.rs`. `PushForce::Force` and broad clean are rejected by tool-side policy. Config keys gated by allowlist.
- **Phase F: conflicts/recovery**: `crates/egggit/src/operation_state.rs` exposes `RepositoryOperationState` + `RecoveryAction`. `src/git_recovery.rs` exposes `continue_in_progress`, `abort_in_progress_typed`, `skip_in_progress`.
- **Track U: unified Bash→Git routing**: Git families split into `GitRead`, `GitLocalMutation`, `GitNetwork`, `GitDestructive`. Conservative default (`route_git_local_mutation = Off`). See `tests/git_execution_origin_matrix.rs`.
- See `architecture/git.md` for the full contract.

### Human Shell

- **Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.
- **Syntax**: `!command` runs ephemeral (hidden from model). `!!command` runs and auto-promotes output.
- **Module location**: `src/shell/` — `types.rs`, `runtime.rs`, `store.rs`, `policy.rs`, `digest.rs`, `projection.rs`, `projector.rs`, `projection_bridge.rs`, `rtk.rs`, `redactor.rs`.
- **Policy**: `evaluate_command()` blocks destructive commands, warns on risky ones.
- **Projection pipeline (Phases 1–10)**: `CommandOutputStore` retains raw output. `ProjectionSelector` handles projection (safe/truncated/error-retention). RTK integration is env-gated (`CODEGG_RTK_INTEGRATION=1`). Redaction applied inside `ProjectionSelector::project()`. Context budget compaction via `ProjectionContextMetadata`. See `architecture/human_shell.md`.

### Command Intent and Planning

- **Command intent classifier**: `classify_command_with_context(command, ctx)` in `src/command_intent/mod.rs` classifies into intent families (Test, GitReadOnly, GitMutating, SearchReadOnly, PythonAnalyze/Transform/Verify, Build, Lint, Format, RawShell, Rejected).
- **Active routing mode**: `CommandIntentMode::Active` dispatches to structured backends. Default is `Observe`. Kill switch: `CODEGG_ROUTING_DISABLE=1`.
- **Validation gate**: `validate_for_active_routing()` checks 7 conditions. Failed gate remains observe; once active scheduler routing is selected, errors are returned without fallback.
- **Git classification**: Delegates to `codegg_git::parse_git_argv()` for risk assessment. `git push`, `git reset --hard`, `git clean -f` classified as High risk.
- **ShellShape parsing**: `ShellShape` enum in `src/command_intent/shell_shape.rs`. Handles quotes, escapes, operators.
- **Command planner**: `plan_execution()` in `src/command_intent/plan.rs` maps intents to `ExecutionBackend`.
- **Python scripting**: `src/python_script/` is the sole canonical module. Analyze/Transform/Verify modes. Python is NOT hidden inside bash.
- **Package manager safety**: Package managers (`npm install`, `pip install`, `cargo install`) are classified as `RawShell`, NOT Build.
- See `architecture/command_intent.md`, `architecture/command_planner.md`, `architecture/command_routing.md`.

### Context Policy

- Context policy is **disabled by default** (`observe` mode). Config via `[context_policy]`.
- Volatile-tail compaction is **disabled by default** (`observe` mode).

## Architecture Docs

`architecture/` has 44 docs covering every module. See `architecture/overview.md` for the full module map and navigation index. Key ones:

| Document | Key Gotchas |
|----------|-------------|
| `architecture/overview.md` | Module map, verified counts (107 commands, 44 events, 39 LSP servers, ~37 tools, 9 agents) |
| `architecture/agent.md` | AgentLoop has ~49 fields at `src/agent/loop.rs:1380` |
| `architecture/bus.md` | 44 AppEvent variants; PermissionRegistry/QuestionRegistry are synchronous |
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
| `architecture/jobs.md` | Phase 4 durable jobs, attempts, schedules, recovery, idempotency |
| `architecture/scheduler.md` | Phase 5 admission control, fair queue, executor dispatch |
| `architecture/command.md` | 107 built-in slash commands |
| `architecture/config.md` | Config schema in `crates/codegg-config/src/schema.rs` |
| `architecture/provider.md` | 16 auto-registered providers via env vars; CircuitBreaker pattern |
| `architecture/preflight.md` | Harness-side eggsact preflight: types, policy config, tool integration, anti-recursion |

`.agents/skills/*/SKILL.md` contain 45 module-specific skill guides loaded on-demand via `/skill:`.

## Key Lessons

1. **Verify claims against code** — Many "bugs" in docs turned out to be correct after inspection.
2. **Documentation goes stale** — Struct fields get added/removed; always compare docs to source.

## Where New Components Belong

### New Web Search Providers
Add to the **eggsearch** project, not to Codegg's built-in search provider registry (`src/search/`). The built-in registry is legacy fallback only.

### New Deterministic Validators
Add to the **eggsact** crate. The eggsact project owns the validation logic. Codegg's `EggsactTool` wrapper in `src/tool/deterministic.rs` exposes eggsact tools to the model. New tools need:
1. Implementation in eggsact with the `codegg_core` profile
2. Registration in `build_eggsact_tools()` in `src/tool/deterministic.rs`
3. Category assignment (always-visible vs deferred)

### New LSP Servers
Add to `crates/egglsp/src/server.rs`. Each server needs a `LspRule` entry with command, extensions, and initialization options. See `architecture/lsp.md`.

### New Native Tool Crates
Follow the library-first, MCP-second pattern in `architecture/native_crates.md`. Durable tool domains live in workspace crates under `crates/` and are consumed directly in-process.

### New Git Operations
`codegg-git` (`crates/codegg-git`) is the typed Git operation model, argv parser, and risk classification crate. `classify_git()` delegates to `codegg_git::parse_git_argv()`. See `architecture/command_intent.md`.
