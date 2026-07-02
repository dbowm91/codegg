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
| `egglsp-test-server` | Fake LSP server binary for integration tests |

Root `src/` is the application: agent, TUI, tools, server, auth, etc.

`examples/plugins/` (Phase 13) contains six reference plugins plus two helper SDKs — process / wasm / builtin / python / rust patterns. Each example is a standalone crate or directory; the root workspace is unmodified.

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

# Real-server smoke (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke

# Plugin example SDKs (Phase 13)
cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml          # 11 tests
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v       # 24 tests
cargo build --target wasm32-unknown-unknown \
  --manifest-path examples/plugins/wasm-command-table/Cargo.toml --release  # WASM build
```

## Critical Gotchas

- **Plugin UI protocol (Phase 3/4)**: `codegg_protocol::ui` defines portable UI types (`UiNode`, `UiEffect`). Phase 2 adds TUI-side consumption: `PluginUiState` in `src/tui/app/state/plugin_ui.rs` stores dialogs/panels/status items; `PluginUiRenderer` in `src/tui/components/plugin_renderer.rs` lowers `UiNode` to ratatui. `App::apply_plugin_ui_effect()` routes effects. `Dialog::Plugin` and `DialogType::Plugin` are the generic plugin dialog variants. Panels and status items are stored but not visually rendered in Phase 2. Phase 3 adds generic `TuiCommand` plugin variants (`PluginCommandRun`, `PluginCommandFinished`, `PluginUiEffect`) and `src/tui/commands/plugins.rs` with `start_plugin_command`, `apply_plugin_command_finished` (response application), and `apply_plugin_ui_effect` (direct effect dispatch). Phase 4 replaces the stub with real process execution: `start_plugin_command` now accepts a `ProcessCommandSpec` from `src/command/mod.rs` and spawns a child process with timeout (default 5s), stdout cap (1 MiB), stderr cap (256 KiB), and stdin piping. `CommandConfig` in `crates/codegg-config/src/schema.rs` gains `runtime`, `command`, `args`, `stdin`, `stdout`, `timeout_ms`, `cwd`, `env`, `output` fields. Frontmatter with `runtime: process` yields a `ProcessCommandSpec` on the `Command.process` field. The TUI `execute_command` method checks `cmd.process` before `cmd.template`. Stdout modes: `text` (plain), `json` (parse as `PluginResponse`), `auto` (try JSON, fall back to text).
- **Plugin manifest/registry redesign (Phase 5)**: `PluginManifest` is now the canonical internal type mapping to `codegg_protocol::plugin::PluginManifestDto`. It has `api_version`, `runtime: PluginRuntimeSpec`, `capabilities: Vec<PluginCapability>`, `permissions: PluginPermissionSet`, plus legacy `hooks`/`config` fields. `PluginRuntimeSpec` is `Builtin { handler }` | `Process { command, args, timeout_ms }` | `Wasm { module, ... }`. `PluginCapability` is `Command` | `Hook` | `Panel` | `StatusWidget` | `EventSubscription`. `PluginTrustClass` (Builtin, LocalProcess, SandboxedWasm, TrustedLocal) is inferred from runtime kind. `PluginRegistry` indexes by capability: `command()`, `commands()`, `panels()`, `status_widgets()`, `event_subscribers()`. Duplicate command names are rejected. Disabled plugins are excluded from capability queries. Legacy `[[hooks]]` manifests auto-convert to capabilities. `PluginService::invoke_command()` is the Phase 5 entry point; actual runtime dispatch is Phase 6. `src/plugin/tui.rs` is deprecated/legacy.
- **Plugin runtime abstraction (Phase 6+7)**: `src/plugin/runtime/` defines `PluginRuntime` trait (`async fn invoke(PluginInvocation) -> Result<PluginResponse, RuntimeError>`) with `ProcessRuntime` and `WasmRuntime` implementations. `RuntimeError` covers Unsupported, Spawn, Timeout, NonZeroExit, InvalidJson, Io. `RuntimeLimits` controls timeout_ms, max_stdout_bytes, max_stderr_bytes. `ProcessRuntimeSpec` converts from both `ProcessCommandSpec` and `PluginRuntimeSpec::Process`. `ProcessRuntime` handles child process spawning, stdin piping, timeout, output capping, and stdout mode parsing (Text/Json/Auto). `WasmRuntime` (Phase 7) executes WASM plugins via Wasmtime behind the `plugins` feature flag; supports modern `codegg_plugin_invoke` ABI (returns packed i64 with ptr/len) and legacy per-hook exports (`on_auth`, etc.) for backward compatibility. `WasmModuleCache` provides mtime-based module caching and per-plugin fuel budgets. `loader.rs` is now a compatibility shim — `execute_wasm_hook` delegates to `WasmRuntime`. `PluginResponse` is unified: `codegg_protocol::plugin::PluginResponse` (with `effects: Vec<UiEffect>`) is the single canonical type re-exported from `plugin/mod.rs`. `PluginDiagnostic` is also unified via re-export from protocol. Hook context conversions: `HookContext::into_plugin_invocation()` and `HookResult::from_plugin_response()` bridge the old hook types with the new runtime. TUI `start_plugin_command` delegates to `ProcessRuntime` instead of inline process spawning. Stable invocation IDs: project-local commands use `command:local:<command-name>`, installed plugins use `plugin:<plugin-name>:<command-name>`. `PluginContext` carries session_id, model, project_dir, and metadata (slash_command_name, executable, raw_args).
- **BuiltinRuntime (Phase 8)**: `BuiltinRuntime` (`src/plugin/runtime/builtin.rs`) is a first-class runtime alongside `ProcessRuntime` and `WasmRuntime`. `BuiltinHandlerRegistry` maps handler IDs to native Rust hook handler functions. `BuiltinRuntime` implements `PluginRuntime` and dispatches `PluginInvocation` through registered handlers. Adapter functions `invocation_to_hook_context()` and `hook_result_to_plugin_response()` bridge the hook handler model with the runtime model. `builtin_plugin_manifests()` and `builtin_runtime_registry()` in `src/plugin/builtin/mod.rs` provide canonical sources for builtin metadata. `PluginService` accepts an optional `Arc<BuiltinRuntime>` via `with_builtin_runtime()`. Individual builtin modules (copilot, codex, gitlab, poe) expose `PLUGIN_ID`, `HANDLER_ID`, and `manifest()` functions.
- **Plugin lifecycle hooks (Phase 9)**: `LifecycleHooks` in `src/plugin/lifecycle.rs` is a policy-aware dispatcher wrapping `PluginService`. Provides typed methods: `emit_event()`, `before_tool_execute()`, `after_tool_execute()`, `transform_messages()`, `shell_env()`. Each checks `PluginLifecyclePolicy` (`src/plugin/policy.rs`) before dispatch. `PluginLifecyclePolicy` gates hook execution by type (observation/mutating/blocking) and runtime (builtin/WASM/process). Defaults: observation enabled, mutating/blocking disabled, process hooks disabled. `PluginHookOutcome<T>` enum: `Ok(T)`, `Skipped`, `Blocked{reason}`, `Failed{error}`. `PluginService` is created in `CoreDaemon` via `create_default_plugin_service()` and wired into `AgentLoop` through `TurnRunInput`. Shell env hooks dispatch before process spawn in `ShellRuntime::spawn()`. Message transform hooks run before provider calls in `AgentLoop::run()`. Pre/post tool hooks and compaction hooks were already wired (dead code) and are now active.
- **Plugin UI frontend-neutral events (Phase 10)**: `UiEffectEnvelope`, `UiEffectSource`, and `PluginUiCapabilities` in `codegg_protocol::ui` provide typed transport for plugin UI effects across core events and remote TUI protocol. `CoreEvent::PluginUiEffect` and `TuiMessage::PluginUiEffect` carry session-scoped plugin UI effects through the event bus. `HookResult` and `PluginHookOutcome::Ok` now carry `effects: Vec<UiEffect>` preserving plugin effects from lifecycle hooks. The agent loop emits `AppEvent::PluginUiEffect` for each effect, which the event bridge maps to `CoreEvent::PluginUiEffect` for remote clients. `ClientCapabilities` has `plugin_ui_*` boolean flags for capability negotiation. `degrade_node_to_text()` in `codegg_protocol::ui` provides deterministic degradation for unsupported node types. `PluginUiCapabilities::supports_effect()` checks whether a client supports a given effect's surface type.
- **Plugin UI multi-frontend readiness (Phase 15)**: Plugin UI events are wrapped in `UiEffectEnvelope` (typed source + session + invocation) on both `CoreEvent::PluginUiEffect` and `TuiMessage::PluginUiEffect`. The wire-shape version is `PROTOCOL_VERSION = 2` for core events and `REMOTE_TUI_PROTOCOL_VERSION = 3` for the remote TUI channel. `UiLimits::balanced()` (and `text_only()`) provide validated caps (`max_effects_per_response`, `max_effect_bytes`, `max_node_depth`, `max_table_rows`, `max_table_columns`, `max_string_len`, `max_panels_per_plugin`, `max_status_items_per_plugin`, `max_open_dialogs_global`, `max_snapshot_body_bytes`). `UiLimits::validate_effect(effect)` and `validate_effects(effects)` are the canonical validation gates. `App::apply_plugin_ui_envelope(envelope)` is the canonical entry point for all plugin UI effects regardless of transport (local TUI command or remote WebSocket); it derives `source_plugin_id` from the envelope, runs the session guard, validates against `UiLimits::balanced()`, enforces surface-ownership rules, and then delegates to `App::apply_plugin_ui_effect(effect, plugin_id_opt)`. `App::validate_plugin_ui_effects(effects)` is the batch validator used by lifecycle hooks and the event bridge. `RemotePanelView` / `RemoteStatusItemView` gained optional `source_plugin_id` and `body: Option<UiNode>` fields; the snapshot builder includes the body only when serialized size ≤ `SNAPSHOT_BODY_LIMIT` (default 16 KiB, mirrors `UiLimits::max_snapshot_body_bytes`). Helper `plugin_id_from_surface_id(id)` extracts the owning plugin from the surface id (`<plugin>:<command>` form; `command:local:...` has no plugin owner). All three legacy snapshots (no `source_plugin_id` / no `body`) deserialize cleanly via `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- **Plugin UI corrective hardening (Phase 11)**: closes correctness gaps in the plugin UI/runtime integration. Four corrections: (1) WASM fuel accounting in `src/plugin/runtime/wasm.rs` now returns the *unused* portion of a fuel reservation via `return_unused_fuel()` — never the consumed amount — so per-plugin budgets decrease by actual fuel usage; on error the full reserved amount is returned so failed invocations do not burn fuel. (2) `BuiltinRuntime` (`src/plugin/runtime/builtin.rs`) is now strictly hook-only: `invocation_to_hook_context()` rejects `PluginCapabilityInvocation::Command` and unknown hook type strings with `RuntimeError::Unsupported` (no more silent fallback to `HookType::Auth`). `make_builtin_info()` skips unknown hook types instead of falling back. `PluginService::invoke_command()` for a builtin plugin returns `PluginError::Runtime` if no command handler is registered. (3) `EmitChat` effects are rendered visibly by `App::apply_plugin_ui_effect()` via the toast / info-dialog surface (short blocks toast, long blocks open the scrollable InfoDialog). Both `ChatFormat::Plain` and `ChatFormat::Markdown` are lowered to line-based text — no markdown link execution or escape sequence interpretation. Output is **not** added to the model-visible chat transcript. (4) `PluginRegistry` capability queries (`hooks_for`, `command`, `commands`, `panels`, `status_widgets`, `event_subscribers`) and the duplicate checks in `set_enabled()` no longer use `try_read()`; they snapshot enabled plugin ids under a single read guard via the new public `enabled_plugin_ids()` helper. Visibility now depends only on actual enabled state, not lock contention. A structural regression test (`registry_does_not_use_try_read_as_code`) ensures `try_read()` is not reintroduced. New `PluginUiApplyResult::ChatApplied` indicates the App-level emit-chat path succeeded; `ChatRequested` remains for the lower-level `PluginUiState::apply_effect()` path.
- **Plugin management UX (Phase 12)**: `src/plugin/management.rs` provides `PluginManager` with `list()`, `info()`, `enable()`, `disable()`, `doctor()`, `remove()` methods. `PluginManager::enable/disable` actually call `PluginRegistry::set_enabled`. `PluginManagementView` is the flattened view struct with `from_info` (registry) and `from_marketplace` (filesystem) constructors. Selector resolution lives on `PluginRegistry::resolve_plugin_selector` (exact id → exact name → unique prefix). `src/plugin/management_ui.rs` provides `plugins_table()`, `plugin_info_node()`, `doctor_report_node()` returning portable `UiNode`, plus `node_to_lines()` for textual fallback rendering. `src/tui/commands/plugin_management.rs` has 7 handlers (`show_plugins`, `show_plugin_info`, `enable_plugin`, `disable_plugin`, `doctor_plugin`, `remove_plugin`, `install_plugin`) plus `verify_remove_target_is_safe()` for path safety. Commands registered: `/plugins` (aliases `/plugin-list`, `/plugin-ls`), `/plugin-info`, `/plugin-enable`, `/plugin-disable`, `/plugin-doctor`, `/plugin-remove`, `/plugin-install`. TUI handlers render output via the `UiNode` → `node_to_lines` path so all visible output flows through the portable schema. Enable/disable persistence uses a `disabled_plugins.toml` file in the plugins dir. `PluginRegistry::set_enabled` takes `&self` (not `&mut self`) since mutations use interior RwLocks. Doctor checks: registered, manifest_valid, api_version, has_capabilities, runtime_configured, runtime_available (process exec or WASM feature), enable_state, no_duplicate_capabilities, permissions_declared, output_surfaces, install_path, no_error_diagnostics, registry_consistency.
- **Plugin security policy hardening (Phase 12)**: `PluginPolicy` in `src/plugin/policy.rs` is a composite policy combining `PluginLifecyclePolicy`, `PluginUiPolicy`, `PluginPermissionPolicy`, `PluginInstallPolicy`, and `PluginRuntimePolicy`. All sub-policies default to conservative: observation hooks allowed, mutating/blocking/process hooks denied, secrets denied, env passthrough denied, undeclared capabilities denied, panel/status UI effects denied. `PluginService` accepts `Option<Arc<PluginPolicy>>` via `with_policy()`. When set, `invoke_command()` checks command declarations and `dispatch_hook()` checks hook type + trust class + auth-hook high-trust requirement before execution. `src/plugin/permission.rs` provides `PolicyDecision` (Allow/Deny/Degrade) with four check functions: `check_invocation_allowed`, `check_ui_effect_allowed`, `check_lifecycle_hook_allowed`, `check_secret_access_allowed`. Policy is opt-in: when absent, all checks pass (backward compatible). Process plugins are local executable code. They are not sandboxed. They are suitable for explicit user-invoked local commands, not silent lifecycle interception by default.
- **Plugin SDKs and examples (Phase 13)**: `examples/plugins/` ships six runnable reference plugins plus two helper SDKs. Process examples: `process-quota-text` (zero-SDK stdout) and `process-quota-json` (JSON stdin/stdout). WASM examples: `wasm-command-table` (modern `codegg_plugin_invoke` ABI, dialog with table), `wasm-hook-message-transform` (observation-only event subscription), `wasm-status-widget` (panel + status widget). Builtin-reference is documentation-only. SDKs: `sdk-python/` (stdlib-only vendorable Python package, 24 unit tests), `sdk-rust/` (Rust helper crate with `codegg_plugin!` macro, 1 MiB bump allocator, 11 tests with one wasm-only `#[ignore]`). Each example is independent (own `Cargo.toml` with `[workspace]` isolation or its own directory); the root workspace is not modified. Build commands: `cargo build --target wasm32-unknown-unknown --release` per example; validation: `cargo test --manifest-path examples/plugins/sdk-rust/Cargo.toml` and `PYTHONPATH=examples/plugins/sdk-python python3 -m unittest discover examples/plugins/sdk-python/tests -v`. The wire format lives in `crates/codegg-protocol/src/plugin.rs` (`PLUGIN_PROTOCOL_VERSION = 1`) and `crates/codegg-protocol/src/ui.rs`; SDKs re-export those types rather than redefining them so the format cannot drift.
- **Plugin UI component modularization (Phase 14)**: `UiNodeRenderer` (`src/tui/components/ui_node_renderer.rs`) is the canonical lowering adapter for `UiNode` trees — both plugin and first-party informational surfaces render through it. The legacy `PluginUiRenderer` is a compat alias re-exported from `src/tui/components/plugin_renderer.rs`. `UiNodeDialog` (`src/tui/components/dialogs/ui_node.rs`) is a generic scrollable dialog that takes a `UiNode` directly (j/k scroll, PageUp/PageDown page, Home/End jump, Esc/Enter close) and reuses `DialogType::Plugin` in the focus manager. `src/tui/ui_builders/` contains pure first-party builders — `stats.rs` (`stats_node` for `/tui-stats` with `TaskSummaryView` DTO), `plugins.rs` (re-exports `management_ui` builders), `shell.rs` (`shell_detail_node` for `/shell-show`). Renderer hardening: empty-table fallback message, key-value alignment to longest key, column-width cap of 60 chars with `…` truncation, ANSI/CSI and control-character sanitization on line output, safe `total=0` percentage. Interactive components (permission/question/command-palette/file-diff/source-preview/tree/security-review) remain native ratatui components and MUST NOT be migrated to `UiNode`.

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
- **Dialog::Plugin is generic**: A single `Dialog::Plugin` variant handles all plugin dialogs. Plugin dialog content is stored in `PluginUiState.dialogs` and rendered via `PluginDialog` component (`src/tui/components/dialogs/plugin.rs`).
- **UiState has 27 fields** (lines 40-92 in `src/tui/app/state/ui.rs`). `timeline_visible` and `timeline_selected` are in `UiState`, NOT `App`.
- **Async command pattern**: High-latency TuiCommand handlers use a spawn-and-complete pattern via `spawn_tui_task`. The `start_*` function spawns work; a typed completion variant arrives back on the command channel. Stale protection uses request IDs for import and research operations. See `src/tui/async_cmd.rs` and `plans/tui_phase_1_event_loop_responsiveness.md`. **Converted handlers** (non-exhaustive): `ReloadSessions`, `LoadSessionMessages`, `OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `ResearchListRuns`, `ResearchLoadRun`, `ResearchLoadSection`, `MemorySummary`, `MemorySearch`, `MemoryRemember`, `MemoryForget`, `RunDoctor`, all session mutations (delete, archive, fork, bulk delete/archive/export, rename, undo delete, share, unshare, export), goal operations (show, checkpoint, budget, refresh session state), task operations (list, delete, schedule), worktree list, template create, and notification send.
- **AsyncUiRequestState (Phase 10)**: `AsyncUiRequestState` in `src/tui/app/state/async_request.rs` replaces ad-hoc generation counters and boolean in-flight flags. It tracks request ID, loading, cancelled, and last_error. `DialogState` fields use `AsyncUiRequestState` instances: `import_request`, `research_request`, `session_reload_request`, `task_list_request`, `task_delete_request`, `worktree_list_request`, `template_create_request`, `session_mutation_request`, `session_messages_request`. `close_dialog()` is `pub(crate)` and cancels async request states for Import (`import_request.cancel()`), ResearchBrowser (`research_request.cancel()`), and Session (`session_reload_request.cancel()`, `session_messages_request.cancel()`) dialogs. **Completion semantics (canonical pattern):** apply handlers call `if !state.finish(request_id) { return; }` or `if !state.fail(request_id, err) { return; }` and never mix `is_current()` with manual mutation. The `finish`/`fail` guard returns `false` for stale or cancelled completions, so the apply function returns immediately. `close_dialog()` for the Session dialog also cancels `task_list_request`, `task_delete_request`, `worktree_list_request`, `template_create_request`, `session_mutation_request`, and `task_registry.cancel_kind(TuiTaskKind::Command)`. **All 12 apply handlers follow the canonical pattern:** `apply_import_preview_loaded`, `apply_import_confirmed`, `apply_research_runs_loaded`, `apply_research_run_loaded`, `apply_research_section_loaded`, `apply_sessions_reloaded`, `apply_session_mutation_finished`, `apply_session_messages_loaded`, `apply_tasks_listed`, `apply_task_operation_finished`, `apply_worktree_listed`, `apply_template_session_created`. Each has at least one stale-completion test in `src/tui/mod.rs::async_cmd_tests`. New apply handlers that take a `request_id` MUST use the guard pattern and add a stale-completion test.
- **Background task lifecycle (Phase 7)**: `TuiTaskRegistry` on `App` tracks spawned tasks for counting, cancellation, and shutdown. Use `spawn_registered_tui_task(tx, registry, kind, name, fut)` for lifecycle-tracked tasks; the original `spawn_tui_task` remains for fire-and-forget work. `App::prepare_shutdown()` cancels all registered tasks and kills shell handles. `/tui-stats` reports active task counts by kind, oldest task, completed count, and cancelled count. The registry lives at `src/tui/task_lifecycle.rs`. **Kinds**: `Command`, `FileDiff`, `Shell`, `Research`, `Memory`, `Notification`, `SecurityReview`, `Indexer`, `GitStatus`, `Other`. **Outcome accounting**: `completed_count` increments on `reap_finished`; `cancelled_count` increments in `cancel_kind` / `cancel`. `panicked_count` is reserved for a future `JoinHandle` upgrade (the current abort-handle-only design cannot observe panics). `reap_finished()` runs periodically in the event-loop wake arm. `TuiTaskRecord::is_finished` is true for both natural completion and abort (AbortHandle::is_finished semantics).
- **Async file diff sidebar pipeline**: `AppEvent::FileChanged` no longer performs synchronous file reads or text-diff on the event loop. The handler does only cheap state mutation (marks diff as `DiffStatsState::Pending`, updates sidebar immediately), then spawns a background task via `spawn_sidebar_diff_stats()` in `src/tui/file_diff.rs`. The task sends `TuiCommand::FileDiffStatsReady { path, generation, result }` back through the command channel. Concurrency is bounded by a semaphore (max 2 background diff tasks). The worker enforces a 1 MiB size cap, binary detection (NUL byte probe), and invalid UTF-8 skip. Stale results are discarded by comparing the generation counter. `ChangedFile` uses `DiffStatsState` enum (`Pending | Ready | Skipped | Error`), not raw additions/deletions fields. `SidebarFileChange` in `src/tui/components/sidebar.rs` renders all four states (pending spinner, ready counts, skipped reason, error message). Do NOT spam toasts for skipped or failed diffs -- this is sidebar metadata, not an operational failure.
- **Git sidebar is cached, not rendered live**: `GitSidebarState` (`src/tui/app/state/session.rs`) caches `root`, `branch`, `dirty`, `last_refreshed`, `loading`, `error`, and a `generation: u64`. The render path (`render_sidebar`) is a pure read of this struct -- it never shells out to git. Refresh is triggered by `TuiMsg::SelectSession`, `App::set_session`, and after session reload via `start_refresh_git_sidebar` (`src/tui/commands/git_sidebar.rs`). The probe runs `egggit::status::repo_status` inside `tokio::time::timeout` (`GIT_REFRESH_TIMEOUT = 3s`) so a wedged git invocation cannot block. Results arrive as `TuiCommand::GitSidebarRefreshFinished { generation, ... }`; stale generations are dropped silently.
- **Remote TUI snapshot sequencing**: `App::remote_sequence: u64` (init at 0) is the monotonic counter for remote client snapshots. `remote_snapshot()` is non-mutating; `next_remote_snapshot()` increments. The `Resume { from_event_seq }` handler in `handle_remote_event` enforces: `from_event_seq == 0` → `ResyncRequired { reason: "invalid_resume_sequence" }`; `from_event_seq > remote_sequence` → `ResyncRequired { reason: "requested_sequence_ahead_of_current" }`; otherwise → fresh `StateSnapshot`. `RemoteTuiStateSnapshot.git: Option<RemoteGitInfo>` carries cached git info to remote clients.
- **Long output goes to info dialog**: `App::show_short_or_info(info_type, lines)` toasts when ≤3 lines, otherwise opens the scrollable `InfoDialog` (which reuses if already open). Use this for memory summary/search, /search, /state, and any handler that may emit more than a few lines. Reserve raw `toasts.info(joined)` for genuinely single-line responses (`/status`, `/cost`).
- **Sync dispatch is the rule**: `src/tui/runtime/command_dispatch.rs` arms are all `fn` (non-async). Handlers that need real async work either fire-and-forget via `tokio::spawn` and post a `TuiCommand` completion variant back (e.g. `handle_goal_*`, `handle_compact_session`), or use `spawn_registered_tui_task` for lifecycle-tracked work (e.g. `handle_spawn_subagent` → `SubagentSpawnFinished`, `start_refresh_git_sidebar` → `GitSidebarRefreshFinished`). New dispatch arms should NOT add `.await`.
- **AgentLoop has 49 fields** at `src/agent/loop.rs:1380`. Many docs claim 15.
- **TerminalGuard owns lifecycle**: `src/tui/terminal.rs` provides `TerminalGuard` which tracks each terminal feature (alt screen, raw mode, bracketed paste, mouse capture) and restores in reverse order on drop. `run_event_loop` creates a `TerminalGuard` instead of calling `enter_raw()`/`exit_raw()` directly.
- **Component-level render fallbacks**: `App::render()` wraps risky surfaces (viewport/messages, sidebar, dialog, completions, timeline) in `catch_unwind`. A component panic renders a compact fallback in that region instead of resetting the whole frame. Root render panic recovery remains as final fallback. `TuiDiagnostics` now has `component_render_panic_count` and `recent_component_render_panics` fields tracking component-level panics.
- **Progressive panic recovery**: First root failure: log + render error. Repeated failures (≥1): hide optional overlays/dialogs. Final fallback (≥3 = `MAX_RENDER_PANICS`): reset minimal volatile UI state. `clear_render_error()` only resets panic tracking, not dialog/command state.
- **`App::reset_state()` is conservative**: Only clears dialog, command_mode, timeline_visible, show_completions, completion_filter. Does NOT clear prompt text or search state.
- **`App::extract_panic_message()`**: Associated function that extracts a human-readable string from `Box<dyn Any + Send>` panic payloads. Used by both component fallbacks and root render recovery.
- **TUI Phase 4 logging/diagnostics**: `src/tui/mod.rs` no longer has an unconditional `debug_log!` macro that writes `codegg_debug.log` to the working directory. All TUI debug logging goes through `tracing` with structured fields under targets like `codegg::tui::events`, `codegg::tui::session`, `codegg::tui::input`, `codegg::tui::render`, `codegg::tui::loop`. The `debug_log!` macro in `src/tui/app/mod.rs` and `src/tui/input.rs` remains feature-gated behind `debug-logging`. `TuiDiagnostics` struct in `src/tui/app/state/diagnostics.rs` tracks slow loops, slow renders, slow commands, dropped bus events, and render panics. The `/tui-stats` slash command displays a diagnostics summary.
- **TUI Phase 5 help text**: Help dialog content is generated from `build_help_lines(vim_mode, active_mode)` in `src/tui/input.rs`, not hardcoded. `HelpMode` (Insert/Normal/Command/Dialog) and `HelpEntry` types centralize help metadata. Insert mode help shows modifier shortcuts only; bare `?`, `/`, `j`, `k` are documented as text input. `UiState` stores `vim_mode: bool` for help generation.
- **Remote TUI protocol is event/state-driven (Phase 8)**: The `/tui` WebSocket uses `TuiCommand` enum. `RenderFrame` is unsupported — receiving it returns an `Error` with code `unsupported_render_frame`. Remote clients should use `StateSnapshot` and `RequestSnapshot`. Protocol version: `REMOTE_TUI_PROTOCOL_VERSION = 1`. `App::remote_snapshot()` is pure/nonblocking.
- **TUI Phase 11 runtime module decomposition**: `src/tui/mod.rs` was decomposed from ~7950 lines to ~1040 lines. Command handlers moved to `src/tui/commands/` (9 submodules: sessions, tasks, goals, memory, research, import, shell, security, diagnostics). Runtime logic moved to `src/tui/runtime/` (event_loop, command_dispatch, app_events, render_recovery). The event loop lives in `runtime/event_loop.rs`. The command dispatch match is in `runtime/command_dispatch.rs`. Bus event handling is in `runtime/app_events.rs`. Render panic recovery is in `runtime/render_recovery.rs`. `create_terminal()` and `AppTerminal` type alias moved to `terminal.rs`.
- **TUI Phase 12 UX consistency and discoverability polish**: `InfoDialog` is the standard scrollable surface for long structured output (tui-stats, task list, worktree list, memory search, doctor, shell list, shell show). Status bar uses `TuiStatusSummary` and `build_status_summary()` with priority order: render error > permission > question > security > working > shell > tasks > idle. Activity chips indicate reloading, importing, research, mem, tasks, shell, diff, security, agent, subagents. Dialog footers use standardized hints with `|` separator. Error messages use "Core unavailable — check daemon status with /doctor" pattern. Shell detail dialog (`/shell-show`) shows promoted state, head/tail truncation metadata, and scrollable InfoDialog output.

### Tool Registry

- **ToolCatalog::register() takes `&dyn Tool`**, not `Box<dyn Tool>`.
- **multiedit tool exists but NOT in default registry**: `src/tool/multiedit.rs` exists, `pub mod multiedit` is registered, but it's NOT in `ToolRegistry::with_defaults()`.
- **30 tools** in `ToolRegistry::with_defaults()` (`src/tool/mod.rs:231-406`).
- **Tool session constructor**: `with_session_config_defaults(&Config, ...)` is the production constructor. `with_session_defaults(...)` is the legacy all-native fallback.
- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` is used by both `apply_patch` tool and LSP preview operations.

### Agent Runtime

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` via `deps.turn_runtime`. No direct `DefaultTurnRuntime` construction in daemon code (0 direct agent refs).
- **AgentLoopFactory** (`src/agent/agent_loop_factory.rs`) is a build-only seam.
- **CoreRuntimeDeps** (`src/core/runtime_deps.rs`): Bundles pool, memory_store, legacy_agent (LegacyAgentRuntimeDeps), turn_runtime (Arc<dyn TurnRuntime>). Use `with_deps()` for new code.

### LSP

- **egglsp is authoritative**: `src/lsp/` is a thin shim. All real LSP logic lives in `crates/egglsp/`.
- **40 LSP servers** configured in `crates/egglsp/src/server.rs`.
- **Preview-only boundary**: `renamePreview`, `formatPreview`, `sourceActionPreview` never write to disk. `workspace/executeCommand` is never invoked.
- **Capability-gated operations**: `semanticContext` and `securityContext` check `LspCapabilitySnapshot` before expensive LSP calls. Unsupported ops append notes, don't fail.
- **LSP tests need `lsp-test-support` feature**: The fake server binary is `codegg-lsp-test-server`. Tests use polling loops (bounded waits), not fixed sleeps.
- **Workflow recipes (Phase 7)**: `crates/egglsp/src/workflow_recipes.rs` provides named workflow recipes (repair_local, repair_hunk, review_file, review_diff, security_review_enriched, hunk_source_navigation, preview_suggestion, impact_analysis, test_failure_repair, interface_boundary, cross_file_repair, call_neighborhood) that compose existing LSP primitives into bounded workflows. Recipes use `RecipeSettings` for tier-aware defaults and `RecipeOutcome` for rendered results.
- **Preview artifact lifecycle (Phase 8)**: `PreviewArtifactRegistry` tracks preview artifacts with lifecycle (created→inspectable→applicable, stale→recompute/discard, applied, cleared). Cap: 32 entries (oldest evicted). Registry methods: `register`, `get`, `remove`, `clear`, `mark_applied`, `mark_stale`, `refresh_staleness`. TUI helpers: `render_preview_list`, `render_preview_detail`, `export_preview_apply_candidate`. Agent context renderer includes "not applied" and "user approval required" safety wording. `LspTool` remains read-only.
- **Phase 9 lifecycle commands**: `/lsp-servers`, `/lsp-capabilities`, `/lsp-errors`, `/lsp-root`, `/lsp-restart`, `/lsp-stop` are new. Use `/lsp-servers` to discover server keys before using per-key commands.
- **Preview apply (Phase 9)**: `/lsp-preview-apply` applies patches directly to disk with SHA-256 hash revalidation. Stale previews are blocked. `LspTool` remains read-only (no LSP `workspace/applyEdit`); file writes use standard `std::fs` operations. Per-key stop uses `shutdown_all` fallback. `/lsp-start` and `/lsp-replay-docs` deferred (no clean scoped API). All gating lives in `egglsp::tui_summary::validate_preview_apply` as a testable boundary — it returns a typed `PreviewApplyPlan` without mutating disk; the TUI handler does the actual `std::fs::write` calls and only calls `mark_preview_applied` after every write succeeds. Failed writes leave the preview pending. **Write-side hardening**: `write_preview_apply_plan_atomically_enough()` performs per-file SHA-256 recheck before each write; `PreviewApplyWriteReport` tracks per-file successes/failures; `mark_preview_applied` only called on full success; partial failures reported without marking applied. 10 new tests prove the invariant.
- **Phase 10 bounded semantic operations**: Five new recipe functions (`execute_impact_analysis`, `execute_test_failure_repair`, `execute_interface_boundary`, `execute_cross_file_repair`, `execute_call_neighborhood`) lower into `LspContextPacket` via `collect_context`. New types `SymbolTarget` and `HierarchyDirection` in `crates/egglsp/src/context.rs`. Test failure repair uses heuristic symbol extraction from failure messages. Each operation enforces budget/truncation limits per `RecipeSettings` tier. Key gotchas: `SymbolTarget` is file+position (not name-based), `HierarchyDirection` is `Incoming|Outgoing|Both` (not `Callers|Callees`), and capped references vary by model tier (e.g., impact analysis: 5/20/50 refs for Small/Workhorse/Frontier).
- **Phase 11 context policy**: `LspContextPolicy` in `crates/egglsp/src/context_policy.rs` centralizes tier/workflow/risk/budget/stale decisions. `resolve_model_tier()` uses precedence: explicit override > config override > model family heuristic > Workhorse default. `TierSource` tracks which step produced the result. Workflow/tier defaults (12 recipes × 3 tiers) for feature flags (`include_cross_file`, `include_hierarchy`, `include_previews`) and budgets are centralized in `LspContextPolicy::workflow_tier_defaults()`. Convert to `RecipeSettings` or `LspContextRenderConfig` via `to_recipe_settings()` / `to_render_config()`. **Fixed in Phase 15:** `LspContextRenderConfig` now exposes `include_cross_file` / `include_hierarchy` fields; `to_render_config()` propagates those policy flags correctly.
- **Phase 12 semantic memory cache**: `LspSemanticCache` in `crates/egglsp/src/cache.rs` provides an optional bounded memory cache for LSP-derived evidence packets. Cache keys encode workspace root, server ID, operation, request fingerprint, file content hashes, capability fingerprint, and budget fingerprint. Production cache keys now include request-scoped file hashes via `collect_cache_file_hashes_for_request()` in `src/tool/lsp.rs` (cap of 16 files with debug logging). When the primary file is unreadable, cache is bypassed for that request. Cache uses **conservative eviction** (always removes on generation mismatch, file hash change, TTL expiry, or capability fingerprint change — never silently retained). Config via `[lsp_semantic_cache]` with `mode` ("disabled" default / "memory"), `max_entries` (64), `max_bytes` (4MB), `ttl_seconds` (300). Config is wired from `codegg-config` through `ToolRegistryOptions` to `LspTool::with_cache_config()`. **Production wiring**: `LspTool::lsp_context_for_agent_with_input` routes through the cache when enabled, via the sync `LspSemanticCache::get` / `insert` API (not `collect_context_cached`) because the cache guard is `!Send` and cannot cross `.await`. Pattern: lock, lookup, drop lock, await `collect_context` on miss, lock again, insert. TUI commands: `/lsp-cache-status`, `/lsp-cache-clear [--all|<root>]`. Cache is opt-in and disabled by default.
- **Phase 13 real-world validation and doctor**: `crates/egglsp/src/doctor.rs` provides `LspDoctorReport` and `build_doctor_report()` for the `/lsp-doctor [path]` TUI command. Doctor is read-only, never starts servers. `LspObservabilitySnapshot` in `health.rs` combines operational, cache, and preview metrics. Validation tiers: unit, fake-server, real-server-smoke (feature-gated), manual-doctor. Real-server smoke tests skip cleanly when binaries are missing.
- **Phase 14 workflow composition UX**: Ten new `/lsp-*` workflow commands (`/lsp-repair-local`, `/lsp-repair-hunk`, `/lsp-review-file`, `/lsp-review-diff`, `/lsp-security-review`, `/lsp-impact`, `/lsp-test-repair`, `/lsp-interface`, `/lsp-cross-repair`, `/lsp-call-neighbors`) invoke named recipes via `LspTool::run_lsp_workflow()`. Composed workflows (`execute_composed_security_review`, `execute_composed_repair_failing_test`) combine multiple recipes with explicit caps and sub-recipe provenance tracking. `LspWorkflowInvocation` maps command args to recipe parameters. `LspWorkflowDisplay` provides consistent output with evidence count, freshness, truncation, preview IDs, and suggested next actions. All commands are read-only and never auto-apply previews.
- **Phase 15 renderer-policy unification and context diagnostics**: Fixed impact-analysis cap-note bug (inverted comparison). `LspContextRenderConfig` and `RecipeSettings` now carry `include_cross_file` and `include_hierarchy` fields propagated from `LspContextPolicy`. `LspContextDiagnostics` struct provides structured context-shaping diagnostics. `/lsp-context-diagnostics <file-path>` TUI command available on demand. Behavior tests added for all `StaleEvidencePolicy` and `LspUnavailablePolicy` variants.
- **Phase 16 disk cache evaluation (deferred)**: Benchmarked disk-backed LSP semantic cache. Disk I/O viable (~460µs overhead) but deferred due to privacy risks (plaintext source snippets, secrets leakage). Memory-only cache remains the only mode. Decision record at `plans/lsp_phase_16_disk_cache_decision.md`. Threat model at `architecture/lsp_disk_cache_threat_model.md`. Benchmark harness at `crates/egglsp/tests/lsp_cache_benchmark.rs`.
- **Phase 17 manual lifecycle controls (deferred)**: Evaluated `/lsp-start` and `/lsp-replay-docs`. Auto-start via `get_or_create_client()` handles server startup on demand; document replay is handled internally by the restart coordinator. Per-key stop uses `shutdown_all()` fallback (no service-level `terminate_runtime` API yet). No evidence of lifecycle control failures. Decision note at `plans/lsp_phase_17_decision_note.md`.
- **Phase 13-17 corrective verification pass (2026-06-27)**: docs/roadmap reconciliation plus test hardening. Plan: `plans/lsp_phase_13_17_corrective_verification_plan.md`. Added 52 tests total: 8 in `crates/egglsp/src/doctor.rs` (Phase 13), 11 in `crates/egglsp/src/workflow_recipes.rs` (Phase 14), 8 in `crates/egglsp/src/context_policy.rs` + 4 in `crates/egglsp/src/context_renderer.rs` (Phase 15), 6 dispatch tests for `/lsp-doctor` and `/lsp-context-diagnostics` in `src/tui/app/mod.rs`, and 15 tool-level tests in `tests/lsp.rs`. Fixed two bugs: `crates/egglsp/src/workflow_recipes.rs` lines 923 and 1167 used bare `+ 20` / `+ 10` instead of `saturating_add` (would overflow with extreme line numbers). Static safety sweep confirmed no `workspace/applyEdit`/`workspace/executeCommand` regressions on the model-facing path; `mark_preview_applied` only called after `all_succeeded`. Final closure criteria met for all eight workstreams.

### Auth

- **ExternalCommand is disabled**: Both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for any non-empty command. Async timeout plumbing is a follow-up.
- **Credential store**: `~/.config/codegg/credentials.json`. Requires `CODEGG_MASTER_KEY` to store new credentials (not to read env/config-backed keys).
- **Provider registration**: Adding ANY provider via config disables all env-var auto-registration (intentional).
- **Auth logging**: Never log secret prefix/suffix/length. Follow `ResolvedAuthSource::as_str()` pattern.

### Security

- **Security review workflow** (`src/security/workflow/`): Read-only, never mutates files. Risk markers become review prompts, never findings.
- **Security finding synthesis**: Evidence-based, requires 2+ evidence dimensions. Same-file scoping only. Different-file evidence never supports a finding.
- **Auth middleware**: When no token is configured, requests are allowed through (dev convenience, review for production).

### Human Shell

- **Central invariant**: A human `!` command is not model context unless the user explicitly promotes it.
- **Syntax**: `!command` runs a shell command with output hidden from the model (ephemeral). `!!command` runs and auto-promotes output into the conversation.
- **Module location**: `src/shell/` — `types.rs` (ShellOrigin, ShellCapturePolicy, ShellCommandId, ShellRequest, ShellEvent, PromptSubmissionKind), `runtime.rs` (ShellRuntime, ShellHandle), `store.rs` (ShellOutputStore, BoundedOutput, ShellOutputEntry with `exit_code: Option<i32>`), `policy.rs` (HumanShellPolicyDecision, evaluate_command), `digest.rs` (ShellDigest, ShellFailure, TruncationReport, `build_from_entry()` convenience API).
- **TUI commands**: `/shell-list`, `/shell-include <id> [stdout|stderr|all]`, `/shell-rerun <id>`, `/shell-kill <id>`. `TuiCommand` variants: `RunHumanShell`, `ShellEvent`, `ShellInclude`, `ShellRerun`, `ShellKill`, `ShellList`. `handle_shell_list` shows compact status with exit codes (format: `[id] done exit=N X.Xs $ command`). `handle_shell_kill` now uses `mark_killed(id, elapsed)` to set status to `Killed` (not `Exited`) with proper elapsed calculation. Late `Exited` events no longer overwrite `Killed` status.
- **MsgPart::ShellCell**: TUI renders shell output as a collapsible cell with id, command, cwd, stdout/stderr preview, status, elapsed, exit code, truncation flag, promoted flag, and expanded state.
- **Config section**: `[human_shell]` — `enabled`, `default_timeout_secs`, `max_history_entries`, `max_bytes_per_command`, `max_total_bytes`, `ansi`, `confirm_dangerous`, `auto_promote_bangbang`.
- **ShellCapturePolicy**: `DisplayOnly` (no storage), `StoreEphemeral` (stored but not in context), `StoreAndPromote` (stored and promoted into context).
- **ShellOrigin**: `HumanEphemeral` (user `!`), `HumanPromoted` (user `!!` or `/shell-include`), `AgentTool` (tool execution).
- **Policy evaluation**: `evaluate_command()` blocks destructive commands (rm -rf /, mkfs, dd to device, fork bombs, shutdown/reboot/halt) and warns on risky ones (rm -rf ., git clean -f, sudo, curl|sh, chmod 777, recursive chown).
- **Bounded storage**: `ShellOutputStore` uses `BoundedOutput` (head 256KB + tail 256KB per command), evicts oldest entries by count and total bytes.
- **Digest extraction**: `ShellDigest::build(status, ...)` extracts Rust compiler errors, warnings, test failures, and panics from stdout/stderr for structured failure reporting. Generates failures for `Killed`, `TimedOut`, and `FailedToStart` statuses. `ShellDigest::build_from_entry()` is a convenience constructor that takes a `&ShellOutputEntry` directly.

### Context Policy

- Context policy is **disabled by default** (`observe` mode). Config via `[context_policy]`.
- Volatile-tail compaction is **disabled by default** (`observe` mode).
- Active mutation of context packer is **disabled**.

## Architecture Docs

| Document | Covers | Key Gotchas |
|----------|--------|-------------|
| `architecture/overview.md` | System-wide module map, verified counts, event flow | Counts drift — verify against source |
| `architecture/agent.md` | AgentLoop, compaction, routing, team coordination | AgentLoop has 49 fields |
| `architecture/auth.md` | Auth types, credential store, CLI | ExternalCommand disabled |
| `architecture/bus.md` | Event bus, PermissionRegistry, QuestionRegistry | Sync registries, registration-before-publish |
| `architecture/cache-aware-context.md` | Cache-aware packing, context policy | Disabled by default (observe mode) |
| `architecture/client.md` | Remote TUI WebSocket client | |
| `architecture/codegg_core.md` | Core crate boundary enforcement | Forbidden imports list |
| `architecture/command.md` | Slash command registry from markdown files | Two command systems: `src/command/` + `src/tui/command.rs` |
| `architecture/compaction.md` | Context window overflow management | |
| `architecture/config.md` | Config loading, validation, file watching | In `crates/codegg-config` |
| `architecture/context-ledger.md` | Context ledger | |
| `architecture/core.md` | Core facade, transport adapters | |
| `architecture/crypto.md` | AES-256-GCM encryption, Argon2id | |
| `architecture/error.md` | Centralized AppError enum | Server errors behind `#[cfg(feature = "server")]` |
| `architecture/exec.md` | Non-interactive exec mode | |
| `architecture/git.md` | Git facts (read-only, in `crates/egggit`) | |
| `architecture/goal.md` | Goal system | |
| `architecture/human_shell.md` | Human shell execution, promotion model, safety policy | Central invariant: ! commands not in model context unless promoted |
| `architecture/hooks.md` | Lifecycle hooks for agent events | |
| `architecture/ide.md` | VS Code/JetBrains detection, diff viewing | |
| `architecture/lsp.md` | LSP client, diagnostics, code operations, Phase 13 doctor/validation | egglsp is authoritative; 40 servers |
| `architecture/mcp.md` | MCP client (local/remote) | |
| `architecture/memory.md` | Persistent memory across sessions | In `crates/codegg-core` |
| `architecture/native_crates.md` | Workspace crates, backend contract | |
| `architecture/permission.md` | Access control, DoomLoop detection, mode system | |
| `architecture/plugin.md` | WASM plugin system with hooks and fuel tracking | No `wasm.rs`; `marketplace.rs` exists |
| `architecture/protocol.md` | Shared request/response envelopes | In `crates/codegg-protocol` |
| `architecture/provider.md` | LLM provider implementations | In `crates/codegg-providers` |
| `architecture/resilience.md` | Circuit breaker, retry mechanisms | In `crates/codegg-core` |
| `architecture/search_backend.md` | Search backend dispatch | |
| `architecture/security.md` | SSRF, sandboxing, security review workflow | Read-only; eggsentry does scanning |
| `architecture/server.md` | HTTP/WebSocket server (feature-gated) | |
| `architecture/session.md` | SQLite session storage | In `crates/codegg-core` |
| `architecture/shell_session.md` | Shell session metadata (no PTY) | |
| `architecture/skills.md` | Runtime skill loader and activation | |
| `architecture/snapshot.md` | File state capture and restore | In `crates/codegg-core` |
| `architecture/storage.md` | SQLite initialization and pooling | In `crates/codegg-core` |
| `architecture/tool.md` | Tool system, registry, backends, execution | 30 tools in default registry |
| `architecture/tts.md` | Text-to-speech (macOS `say`) | |
| `architecture/tui.md` | Terminal user interface (Ratatui) | |
| `architecture/upgrade.md` | Self-upgrade via GitHub releases | |
| `architecture/util.md` | Clipboard, fuzzy search, pricing, metrics | |
| `architecture/worktree.md` | Git worktree management | In `crates/codegg-core` |
| `.opencode/skills/*/SKILL.md` | Module-specific skill guides | Loaded on-demand via `/skill:` |

## Key Lessons

1. **Verify claims against code** — Many "bugs" in docs turned out to be correct after inspection.
2. **Documentation goes stale** — Struct fields get added/removed; always compare docs to source.
3. **Line numbers are fragile** — References like `watcher.rs:157` can be off by several lines. Use code search.
4. **Count from source, not docs** — Tool/server/command counts drift. Count actual entries in `with_defaults()`, `server_definitions()`, `CommandRegistry`.
5. **Don't assume tool registration** — Not every tool in `/tool` is in the default registry.
