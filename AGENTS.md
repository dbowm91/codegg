# AGENTS.md

## Project Overview

This is a **Rust rewrite of an AI coding agent**, built for performance and efficiency. The codebase uses:

- **Tokio** for async runtime
- **SQLx** for SQLite database
- **Ratatui** for terminal UI
- **Axum** for HTTP server (feature-gated)
- **Wasmtime** for WASM plugins (feature-gated)

## Module Reference (36 Modules)

| Module | Purpose |
|--------|---------|
| `agent/` | AgentLoop, compaction, routing, team, turn_runtime |
| `auth/` | Typed `AuthConfig` (api_key / stored / external_command / oauth_device), `Credential`, `AuthResolver` (env → config → store priority), `CredentialStore` at `~/.config/codegg/credentials.json`, `ExternalCommandProvider` (typed but disabled — both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for any non-empty command until async timeout plumbing exists), OAuth device-flow scaffolding, `mask_secret`, `cli::AuthCli` for `codegg auth status | set-key | logout`. CLI validates provider/account ids (`[A-Za-z0-9_-]`, with `*` allowed for `logout` only) and never echoes key material. |
| `bus/` | Event bus system (GlobalEventBus, PermissionRegistry, QuestionRegistry) — now in `crates/codegg-core` (`codegg-core` crate) |
| `client/` | Remote TUI client for WebSocket connections with resume/replay support |
| `command/` | Slash command registry and routing from markdown files |
| `config/` | Configuration loading, validation, and file watcher — now in `crates/codegg-config` (`codegg-config` crate), re-exported as `codegg::config` |
| `crypto/` | AES-256-GCM encryption with Argon2id key derivation |
| `error/` | Centralized `AppError` enum with `ProviderError::is_retryable()`, `ToolError::is_retryable()`, `CircuitError` conversion — error enums now in `crates/codegg-core` (`codegg-core` crate), axum wrappers stay root-side |
| `exec/` | Non-interactive exec mode for CI/CD with JSON I/O |
| `git/` | **Removed** in 2026 native crate extraction. Read-only git facts now in `crates/egggit/` (`repo_status`, `diff_summary`, `changed_files`, `file_diff`, `validate_patch`, `list_worktrees`); mutating worktree operations in `src/worktree/`. The `git` tool wrapper still lives at `src/tool/git.rs` |
| `goal/` | Long-horizon goal runtime: budget enforcement, auto-continuation, GoalStore persistence, system prompt steering — now in `crates/codegg-core` (`codegg-core` crate) |
| `hooks/` | Hooks system for agent loop lifecycle events and plugin interaction |
| `ide/` | IDE integration (VS Code IPC, JetBrains remote mode) |
| `lsp/` | Language Server Protocol support (diagnostics, code operations) — egglsp crate is authoritative implementation, src/lsp/ is thin shim |
| `mcp/` | Model Context Protocol client (local, remote, auth) with auto-reconnect |
| `core/` | Core facade and transport adapters (inproc, stdio, socket) for request/response separation — `src/core/` is the transport layer; domain modules (bus, error, goal, memory, session, storage, snapshot, worktree, resilience, task_state, model_profile, protocol_conversions) live in `crates/codegg-core`. Also contains `runtime_deps` (`CoreRuntimeDeps`) for bundling runtime dependencies. |
| `memory/` | Persistent memory system for session learning and namespace management — now in `crates/codegg-core` (`codegg-core` crate) |
| `permission/` | Access control, path restrictions, DoomLoop detection, mode system |
| `plugin/` | WASM plugin system with hooks and TUI extensions |
| `provider/` | LLM provider implementations (Anthropic, OpenAI, Google, etc.) — now in `crates/codegg-providers` (`codegg-providers` crate), re-exported as `codegg::provider` |
| `protocol/` | Shared `CoreRequest`/`CoreResponse` and `TuiMessage` protocol envelopes — now in `crates/codegg-protocol` (`codegg-protocol` crate), re-exported as `codegg::protocol` |
| `shell_session/` | Shell session metadata management (in-memory, no actual PTY) |
| `resilience/` | Circuit breaker, retry mechanisms, and rate limiting — now in `crates/codegg-core` (`codegg-core` crate) |
| `search/` | Legacy in-tree web search providers (used as `builtin` fallback) |
| `search_backend/` | Pluggable backend layer for the native `websearch`/`webfetch` tools. Default backend is the external `eggsearch` MCP server; legacy in-tree providers are retained as an explicit fallback. |
| `security/` | SSRF protection, internal IP validation, Landlock sandboxing |
| `server/` | HTTP server (Axum) with WebSocket support for remote TUIs and replay buffering |
| `session/` | Session storage, message history, and checkpointing (SQLite) — now in `crates/codegg-core` (`codegg-core` crate) |
| `skills/` | Skill system for specialized capabilities (git, research, etc.) |
| `snapshot/` | Snapshot support for file state capture and restore — now in `crates/codegg-core` (`codegg-core` crate) |
| `storage/` | SQLite database storage layer and initialization — now in `crates/codegg-core` (`codegg-core` crate) |
| `theme/` | Frontend-neutral theme system, registry, Halloy compat, validation, projections |
| `tool/` | Built-in tools (bash, read, edit, task, webfetch, image, etc.) |
| `tts/` | Text-to-speech module with provider support |
| `tui/` | Terminal user interface (widgets, handlers, input processing, diff viewer, notifications, image support, CoreClient-backed flows) |
| `upgrade/` | Self-upgrade functionality via GitHub releases |
| `util/` | Utility functions (clipboard, fuzzy search, pricing, etc.) |
| `worktree/` | Git worktree support for project management — now in `crates/codegg-core` (`codegg-core` crate) |

## Architecture Index

- `architecture/core.md`: Core crate architecture, ownership boundaries, and extraction status
- `architecture/codegg_core.md`: codegg-core workspace crate (bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions)
- `architecture/tui.md`: TUI state, dialog/component maintenance, and CoreClient-backed flows
- `architecture/client.md`: Remote TUI client, resume handshake, and replay-aware event handling
- `architecture/server.md`: WebSocket TUI server, replay buffer, and REST/SSE routes
- `architecture/skills.md`: Runtime skill loader plus the repo-maintained `.skills/` copy
- `architecture/native_crates.md`: Workspace crates (egglsp, egggit, eggsentry, eggcontext, codegg-config, codegg-protocol, codegg-providers), backend contract, raw MCP exposure policy, diagnostics
- `architecture/git.md`: Git session management, git info injection, worktree per session (now in `crates/egggit` + `src/worktree`)
- `architecture/goal.md`: Goal runtime, budget enforcement, auto-continuation, TUI status bar
- `architecture/auth.md`: Typed AuthConfig, Credential, AuthResolver, user-level credential store, ExternalCommand safety, OAuth scaffolding, and CLI surface (`codegg auth ...`) — auth types now live in `codegg-providers`

## Critical Implementation Notes

These items are important for future agents to know when working with the codebase:

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: `PermissionRegistry` and `QuestionRegistry` do NOT store `session_id` in their keys. Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. This means `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id without code changes.

- **Error module split**: Pure error enums live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports from codegg-core plus `AxumAppError`/`AxumServerRuntimeError` wrappers behind `#[cfg(feature = "server")]`.

- **protocol_conversions split**: Core conversions (session, message, provider, config) live in `crates/codegg-core/src/protocol_conversions.rs`. Agent-specific conversions remain in root `src/protocol_conversions.rs`. Root re-exports core conversions via `pub use codegg_core::protocol_conversions::*;`.

- **PermissionDecision vs PermissionChoice**: `PermissionDecision` is the bus-owned DTO in `src/bus/mod.rs`. `PermissionChoice` is the domain type in `src/permission/mod.rs`. Bidirectional `From` impls allow conversion. The `PermissionRegistry` API uses `PermissionDecision`; use it when calling `respond()` or registering permissions through the bus.

- **Tool factory**: `src/tool/factory.rs` provides `build_session_tool_registry()` which consolidates tool construction (base registry + task tool + goal tools). Used by `core/daemon.rs` to reduce direct tool module coupling.

- **Agent runtime factory**: `src/agent/runtime_factory.rs` provides `build_agent_loop()` which consolidates agent loop construction (permission checker + AgentLoop::new + session/subagent configuration). Used by `core/daemon.rs` to reduce direct agent/permission module coupling.

- **CoreRuntimeDeps**: `CoreDaemon` stores a single `deps: CoreRuntimeDeps` field instead of separate `subagent_pool`, `memory_store`, `bg_scheduler` fields. `subagent_pool` and `bg_scheduler` are grouped under `legacy_agent: LegacyAgentRuntimeDeps`. Legacy `new()` constructor kept for backward compat; new code should use `with_deps()`.

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` instead of building tool registries, permission checkers, and agent loops inline. `TurnRuntime` owns the full turn lifecycle: provider resolution, tool registry construction, system prompt assembly, agent loop construction, and background spawning. `AgentLoopFactory` (build-only) is kept as a transitional internal detail.
- **Daemon TurnSubmit ownership**: Daemon still owns request validation, session_id/turn_id management, active-turn bookkeeping, and TurnStarted event publishing. Runtime owns everything else.

- **TaskToolRuntime**: `tool::factory::build_session_tool_registry` takes `Option<&TaskToolRuntime>` instead of `Option<&Arc<SubAgentPool>>`. This breaks the tool factory's direct dependency on `SubAgentPool`.

- **MCP reconnect wired up**: Heartbeat failures now trigger reconnect via `reconnect_needed` Notify mechanism

- **MCP DNS re-validation**: `RemoteClient::initialize()` re-validates DNS on each call (connect/reconnect), keeping `validated_ips` current

- **MCP ensure_connected()**: Clones all fields before `tokio::spawn` to avoid borrow-after-spawn issues

- **Protocol is now a re-export**: `src/protocol/` has been deleted. `src/lib.rs` uses `pub use codegg_protocol as protocol;`. The `codegg-protocol` crate is the single source of truth for `CoreRequest`, `CoreResponse`, `CoreEvent`, `TuiMessage`, and frame types. Root code uses DTO types from `codegg_protocol::dto` when constructing protocol messages; conversions between domain types and DTOs are in `src/protocol_conversions.rs`.

- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`

- **Component trait**: All dialogs implement `Component` trait with `handle_key`, `update`, `render` methods

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event

- **ResyncRequired serialization**: Server uses `TuiMessage::ResyncRequired` variant directly (not raw JSON)

- **Client timeouts**: Health check has 10s timeout, WebSocket connection has 30s timeout

- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`. TTS auto-stops when `AgentFinished` is received. Toggle with `/tts` or `/voice` command.

- **Subprocess PATH**: All tools use `std::env::var_os("PATH")` instead of hardcoded paths for proper Homebrew/cargo/pyenv tool discovery

- **Plugin fuel tracking**: `fuel_reserved` set at `loader.rs:270` is returned via `module_cache::CACHE.return_fuel()` on normal exits. Early error returns at `loader.rs:255-285` also correctly return fuel.

- **handle_remote_event location**: `src/tui/app/mod.rs:794` - not in client module

- **IdeServer async I/O**: `run_stdio()` uses `tokio::io::stdin()/stdout()` with async I/O, not blocking `std::io`

- **ToolCatalog::register() takes `&dyn Tool`**: Not `Box<dyn Tool>`. Document in architecture but often overlooked.

- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum at `types.rs:2-25`.

- **DialogType is in component.rs**: Not in `types.rs`. FocusManager is in `component/focus.rs`.

- **AgentLoop has 24 fields**: The struct at `src/agent/loop.rs:559-584` has 24 fields; many docs list only 15.

- **Exec mode question behavior**: `setup_question_channel_for_exec()` at `src/exec.rs:121` DOES set `question_rx`, meaning exec mode waits up to 300s before timing out. The "[question not supported]" string is in the `else` branch (non-exec path when `question_rx` is None).

- **Goal and todo are two separate surfaces**: Goals are long-horizon, multi-session, durable, autonomous. Todos are in-flight, per-turn, ephemeral. They form a hierarchy: a goal spans many sessions; each session may have todos as steps toward the goal. The system prompt steers models toward todos for in-flight planning and `goal_request_completion` for long-horizon work.

- **Goal budget accounting is atomic**: `GoalStore::increment_usage()` checks all four budget axes in a single transaction. If any axis is exceeded, the goal transitions to `BudgetLimited` atomically. The agent loop then queues a wrap-up prompt on the next turn.

- **Goal wall-clock is durable**: `wallclock_secs` is persisted in SQLite, so time spent on the goal across session restarts is accurately tracked. `GoalWallClock` in the agent loop tracks wall-clock deltas between accounting ticks.

### Known Issues (Lower Priority)

| Issue | Location | Status |
|-------|----------|--------|
| **TTS init ignores providers** | `src/tts/mod.rs:45-49` | Known issue - macOS say adequate |
| **Static CANONICAL_PATHS_CACHE** | `src/security/sandbox.rs:262` | Has 300s TTL + 100-entry cap now |
| **OAuth replay protection TOCTOU** | `src/mcp/auth.rs:318-332` | Known issue |

### Key Lessons from Review Sessions

1. **Always verify documentation claims against actual code** - Many "bugs" in review files turned out to be correctly implemented after direct inspection.

2. **Documentation can become stale** - Struct fields get added/removed; always compare architecture docs against actual source code.

3. **Counts should be verified** - Component/dialog counts (TUI), server counts (LSP), command counts can drift from reality. When fixing documentation, count from actual source files, not from other documentation. **UiState has 26 fields** (not 25 as some docs claim). `timeline_visible` and `timeline_selected` are in `UiState` struct (lines 62-63), NOT `App` struct.

4. **Line numbers in docs are fragile** - References like `watcher.rs:157-158` should be verified; they can be off by several lines. Use code search to find exact locations.

5. **Pre-verification before editing** - When a plan or review file claims "X is wrong in architecture doc", first check if it's been fixed since the review was written. Many "corrections" in old plans were already addressed.

6. **Use subagents for batch review work** - Process 4-5 plan files per subagent (2000 line context limit), consolidate results, then consolidate into final plan.

7. **multiedit tool exists but not in default registry** - `src/tool/multiedit.rs` exists and `multiedit` module is registered via `pub mod multiedit`, but it's NOT included in `ToolRegistry::with_defaults()`. Don't assume every tool in `/tool` is in the default registry.

8. **LSP server count is 39** (verified 2026-05-27) - count entries in `server_definitions()` array at `crates/egglsp/src/server.rs` (moved from `src/lsp/server.rs:27-383`). cmake-language-server is NOT in the list despite some review claims. clangd, rust-analyzer, gopls, etc. are included.

9. **Permission mode documentation corrected** - `architecture/permission.md:202` (docs mode) now correctly shows restricted tools as `bash, task, todowrite` (without `write`). Code at `modes.rs:174-178` correctly excludes `write`.

### Verified Codebase Facts

These items were verified during review sessions:

| Item | Value | Location |
|------|-------|----------|
| Tool count | 27 | `src/tool/mod.rs:90-122` (27 registrations in with_defaults()) |
| LSP server count | 39 | `crates/egglsp/src/server.rs` (moved from `src/lsp/server.rs:27-383`) |
| Native tool crates | 4 | `crates/egglsp`, `crates/egggit`, `crates/eggsentry`, `crates/eggcontext` — see `architecture/native_crates.md` |
| Extracted workspace crates | 4 (+1 new) | `crates/codegg-config`, `crates/codegg-protocol`, `crates/codegg-providers`, `crates/codegg-core` |
| Tool backend contract | `src/tool/backend.rs` | `ToolBackendKind`, `ToolProvenance`, `StructuredToolResult`, `build_report()` for `/tool-backends` |
| `/tool-backends` slash command | `src/tui/command.rs`, handler in `src/tui/app/mod.rs` | aliases: `/tools`, `/backends` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` except pool which is `Option<SqlitePool>` | `src/core/mod.rs:22-28` |
| CoreRuntimeDeps | Bundles pool, memory_store, legacy_agent (LegacyAgentRuntimeDeps grouping subagent_pool + bg_scheduler), turn_runtime (non-optional Arc<dyn TurnRuntime>) | `src/core/runtime_deps.rs` |
| AgentLoopFactory | Trait for agent loop construction seam | `src/agent/agent_loop_factory.rs` |
| TurnRuntime | Execution-oriented trait for turn lifecycle | `src/agent/turn_runtime.rs` |
| DefaultTurnRuntime | Default implementation building tools, permissions, prompt, agent loop | `src/agent/turn_runtime.rs` |
| Daemon direct agent refs | **0** (zero) | `src/core/daemon.rs` — acceptance target met |
| Daemon turn runtime injection | `deps.turn_runtime.run_turn()` | `src/core/daemon.rs:560` — no direct DefaultTurnRuntime construction |
| TaskToolRuntime | Narrow DTO for task tool construction | `src/agent/task_tool_runtime.rs` |
| Plugin fuel logic | Fixed - all early returns correctly return fuel | `src/plugin/loader.rs` |
| CoreEvent mapping | Complete - all events including Subagent* properly mapped | `src/core/mod.rs` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| UiState fields | 26 fields | `src/tui/app/state/ui.rs:27-76` |
| Subagent event types | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `crates/codegg-core/src/bus/events.rs:120-141` |
| CoreEvent has subagent variants | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `crates/codegg-protocol/src/core.rs:244-268` |
| map_app_event_to_core_event | All Subagent events mapped | `src/core/mod.rs` |
| SessionCompacting hook | IS dispatched in AgentLoop::compact_if_needed() | `src/agent/loop.rs:1216-1220` |
| hook_timeout vs WASM_HOOK_TIMEOUT | Outer 5s, inner 30s | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |
| Backoff formula | `2^i` (no jitter) | `crates/codegg-providers/src/provider/fallback.rs:107` |
| Client backoff formula | 1s, 2s, 4s (attempt 1,2,3) | `src/client/attach.rs:39` |
| Protocol version | 1 | `crates/codegg-protocol/src/core.rs:3` |
| AppEvent count | 36 | `crates/codegg-core/src/bus/events.rs:5-147` |
| Built-in command count | 46 (includes /tts, /pr, /issue, /checkpoint) | `src/tui/command.rs:79-182` |
| ToolDefCache | `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)` - model, plan_mode, lsp_enabled, mcp_count, perm_ver, definitions | `src/agent/loop.rs:60-67` |
| Timeline fields location | `timeline_visible` and `timeline_selected` are in `UiState` struct (lines 62-63), NOT `App` struct | `src/tui/app/state/ui.rs:62-63` |
| Snapshot hash | Uses SHA256 consistently | `crates/codegg-core/src/snapshot/mod.rs` |
| Git module | removed in 2026 native crate extraction; read-only facts now in `crates/egggit/` (`repo_status`, `diff_summary`, `changed_files`, `file_diff`, `validate_patch`, `list_worktrees`). Mutating worktree operations remain in `src/worktree/` (sync `create_worktree`/`remove_worktree`) | `crates/egggit/src/{lib,status,diff,worktree}.rs`, `src/worktree/mod.rs` |
| Pricing service | `src/util/pricing.rs` - ModelPricing, calculate_cost | `src/util/pricing.rs` |
| Auto-compact wrapper | Both `auto_compact()` and `auto_compact_sync()` exist | `src/agent/compaction.rs:550,594` |
| ImageTool | IS registered in ToolRegistry::with_defaults() | `src/tool/mod.rs:102` |
| Tool session constructor | `with_session_config_defaults(&Config, ...)` is the production session constructor; `with_session_defaults(...)` is the legacy all-native fallback (documented footgun for config-aware paths) | `src/tool/mod.rs:477,503` |
| `Tool::expose_in_definitions()` | Default `true`; `DisabledTool` overrides to `false`. Both `ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()` filter through it, so disabled/MCP-stub tools are hidden from the model but remain callable for diagnostics and `/tool-backends` | `src/tool/mod.rs:146`, `src/tool/disabled.rs:78` |
| Central native tool execution | `AgentLoop::execute_tool_calls` calls `ToolRegistry::execute_capture(name, input, ctx)` for native tool calls; provenance is recorded via `tracing::debug!` without changing the model-facing string output | `src/agent/loop.rs:3249` |
| Native tool execution context helper | `AgentLoop::build_tool_execution_context(tc, timeout_ms)` builds the `ToolExecutionContext`; `AgentLoop::resolve_native_backend(name)` resolves the `ToolBackendKind` (native by default, `Mcp` for `websearch`/`webfetch` when `[search].backend = eggsearch`, `BuiltinLegacy` otherwise) | `src/agent/loop.rs` |
| Structured execution smoke test | `tests/tool_structured_execution.rs` locks down `ToolProvenance` shape, disabled/MCP-fallback semantics, and definition visibility | `tests/tool_structured_execution.rs` |
| Live dispatcher wiring tests | `test_live_dispatcher_uses_execute_capture`, `test_live_dispatcher_passes_native_backend_in_context`, `test_live_dispatcher_model_output_shape_is_plain_string` prove the agent loop routes native calls through `execute_capture` and the model-facing tool content is a plain string (no provenance JSON) | `tests/agent_loop_harness.rs` |
| Dialog::Stats | EXISTS in Dialog enum | `src/tui/app/types.rs:21` |
| Provider auth resolver | `AuthResolver::resolve` is sync; `register_builtin_with_config` threads a shared `Arc<CredentialStore>` through `register_credential_provider` / `register_api_key_provider` / `register_config_provider`. All three helpers go through the centralized `resolve_provider_credential` helper. | `crates/codegg-providers/src/provider/mod.rs`, `crates/codegg-providers/src/auth/resolver.rs` |
| OpenAI-compatible factories | `create_xai`, `create_mistral`, `create_groq`, `create_deepinfra`, `create_cerebras`, `create_cohere`, `create_together`, `create_perplexity`, `create_venice`, `create_generalcompute`, `create_opencode_go` all take `Credential`; `create_minimax` takes `String` (Anthropic-compatible). Backed by `OpenAiCompatibleProvider::simple_with_credential`; the legacy `simple(id, name, api_key, base_url)` is a backwards-compatible shim. | `crates/codegg-providers/src/provider/additional.rs`, `crates/codegg-providers/src/provider/openai_compatible.rs` |
| ExternalCommand in resolver | Disabled in the synchronous resolver. Returns `AuthError::Unsupported("ExternalCommand")` because the existing `std::process::Command`-based `ExternalCommandProvider::fetch` does not enforce its timeout. Async timeout plumbing is a follow-up. | `crates/codegg-providers/src/auth/resolver.rs`, `crates/codegg-providers/src/auth/external.rs` |
| ExternalCommandProvider::fetch | Returns `AuthError::Unsupported("ExternalCommand requires async timeout plumbing")` for any non-empty command; an empty command yields `AuthError::Invalid`. The previous `std::process::Command` shell-out path has been removed; no safe code path can accidentally execute a configured command. | `crates/codegg-providers/src/auth/external.rs` |
| Provider credential resolution path | Single resolution path: every helper in `src/provider/mod.rs` (`register_credential_provider`, `register_api_key_provider`, `register_config_provider`) goes through `resolve_provider_credential()` → `AuthResolver::resolve()`. Legacy `cfg.api_key` is honored by the resolver via `ctx.legacy_api_key`; no helper reads `cfg.api_key` directly. | `crates/codegg-providers/src/provider/mod.rs`, `crates/codegg-providers/src/auth/resolver.rs` |
| `codegg providers` and `codegg models` | Both commands now use `register_builtin_with_config`, so they see the same set of providers (including those backed by the user credential store). | `src/main.rs` |
| Stored bearer-token policy | `AuthConfig::Stored` and the no-auth fallback's store lookup both filter to `CredentialKind::ApiKey`. A future OAuth/bearer-token refresh flow will need a separate `kind` selector or policy module. `codegg auth set-key` only writes `ApiKey` records. | `crates/codegg-providers/src/auth/resolver.rs` |
| `codegg auth` CLI validation | `AuthCli::set_key` and `AuthCli::logout` validate provider and account ids up-front to contain only `[A-Za-z0-9_-]`. The wildcard `*` is accepted **only** by `logout`. `set_key` never echoes the key, key length, or any prefix/suffix. `status` never prints ciphertext, plaintext, or secret-derived fingerprints. | `src/auth/cli.rs` |
| `codegg auth` CLI | `codegg auth status`, `codegg auth set-key <provider>` (stdin), `codegg auth logout <provider>` are wired in `src/main.rs` and backed by `auth::cli::AuthCli`. | `src/main.rs`, `src/auth/cli.rs` |
| Goal module | Goal, GoalStatus, GoalBudget, GoalUsage, GoalStore, runtime | `crates/codegg-core/src/goal/` |
| Goal budget axes | 4 axes: max_turns, max_model_tokens, max_tool_calls, max_wallclock_secs | `crates/codegg-core/src/goal/model.rs` |
| Goal wall-clock durability | wallclock_secs persisted in SQLite, survives session restarts | `crates/codegg-core/src/goal/model.rs`, `crates/codegg-core/src/goal/store.rs` |
| AgentLoop goal fields | goal_store, goal_wall_clock | `src/agent/loop.rs` |
| Per-turn token tracking | last_turn_input_tokens, last_turn_output_tokens | `src/agent/loop.rs` |
| TUI goal status bar | format_goal_status_line, set_goal on StatusBarWidget | `src/tui/app/mod.rs`, `src/tui/components/status_bar.rs` |
| Goal budget slash cmd | /goal budget [show\|raise <axis> <n>] | `src/tui/app/mod.rs` |
| Goal+todo prompt contract | goal_and_todos_contract() in assemble_system_prompt_with_profile | `src/agent/prompt.rs` |
| Theme module | SemanticTheme, ThemeRegistry, native/Halloy parsers, ratatui projection | `src/theme/` |
| Built-in theme count | **50** Halloy-format themes from `themes.halloy.chat`, bundled as TOML via `include_str!`. Default = `cyber-red` (`DEFAULT_THEME_ID`) | `src/theme/registry.rs:BUILTIN_THEME_FILES` |
| Halloy field coverage | `general.{background,border,horizontal_rule,highlight_indicator,unread_indicator}`; `text.{primary,secondary,tertiary,success,warning,error,info,debug,trace}`; `buffer.{action,topic,nickname,highlight,code,url,timestamp,selection,border,border_selected,background*,server_messages.default}`; 6/8-digit hex accepted (alpha stripped) | `src/theme/halloy.rs` |
| `Theme.code_theme` type | `String` (was `&'static str`) | `src/tui/theme.rs` |
| Theme persistence | **SQLite** `user_preferences.theme.active` (authoritative) + config mirror. Writes via `tokio::spawn` so the UI doesn't block. Read on startup via `App::apply_persisted_preferences`. | `src/storage/preferences.rs`, `src/tui/app/mod.rs` |
| Theme live preview | `ThemePickerDialog::PreviewState` machine: Up/Down → `TuiMsg::ThemePreviewChanged` (live apply, no persist); Enter → `TuiMsg::ThemeCommit` (persist + close); Esc / close → `TuiMsg::ThemeRevert` (restore original + close) | `src/tui/components/dialogs/theme.rs`, `src/tui/app/mod.rs` |
| Last-used model persistence | `KEY_MODEL_LAST_USED` in `user_preferences`. Updated on every `SelectModel` / `cycle_model_forward` / `cycle_model_backward`. Read on startup and applied to `agent_state.current_model` if present in the model list. | `src/tui/app/mod.rs` |
| `/theme` slash command | list, use, reload, diagnostics subcommands | `src/tui/app/mod.rs:handle_theme_command` |
| Boundary script | `scripts/check-core-boundary.sh` | Verifies no forbidden imports/dependencies in codegg-core |
| ckcore alias | `.cargo/config.toml` | `cargo ckcore` = `check -p codegg-core` |

### Security Notes

- **Auth middleware allows requests without token when none configured**: At `src/server/middleware/auth.rs:37-39`, when `expected_token` is `None`, requests are allowed through. This may be intentional for development but should be reviewed for production.
- **WebSocket auth is consistent with HTTP**: Both `src/server/ws.rs:103-106` and `middleware/auth.rs:37-39` return Ok when no token is configured.
- **ExternalCommand is disabled in the synchronous resolver**: `AuthConfig::ExternalCommand` is recognized but `AuthResolver::resolve` returns `AuthError::Unsupported("ExternalCommand")` because the underlying `ExternalCommandProvider::fetch` uses `std::process::Command` and does not enforce its timeout. Re-enable only when async timeout plumbing is in place. See `crates/codegg-providers/src/auth/resolver.rs:173-181`.
- **Provider registration logging policy**: `register_credential_provider` / `register_api_key_provider` / `register_config_provider` log only `ResolvedAuthSource::as_str()` and the env var name. They never log secret prefix / suffix / length. New log lines that touch auth must follow the same rule.

### CoreRequest Handler Attention Points

- `CoreRequest` enum in `crates/codegg-protocol/src/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- TurnSubmit now delegates to `self.deps.turn_runtime` (injected `Arc<dyn TurnRuntime>`) instead of constructing `DefaultTurnRuntime` directly.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - verify if TUI actually sends these before implementing meaningful responses.

## Helpful Patterns for Future Agents

### Provider Auto-Registration
- `register_builtin_with_config()` at `crates/codegg-providers/src/provider/mod.rs:501` registers providers via env vars, the typed `auth` block, and the user credential store. It builds a single `Arc<CredentialStore>` and threads it into the per-provider helpers.
- Per-provider helpers:
  - `register_credential_provider` — OpenAI-compatible providers that accept a full `Credential` envelope (mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, opencode_go, generalcompute).
  - `register_api_key_provider` — providers that genuinely need a static API-key string (opencode_zen, minimax). Rejects `CredentialKind::BearerToken` with a `tracing::warn!`.
  - `register_config_provider` — base-URL-aware variant (anthropic, openai native, google, openrouter).
- All three go through the centralized `resolve_provider_credential(provider_id, cfg, env_var, store)` helper.
- Adding ANY provider via config disables all env-var auto-registration (intentional design)
- SAP AI Core, Zenmux, Kilo, Vercel AI Gateway are config-only, NOT auto-registered
- Check `crates/codegg-providers/src/provider/mod.rs:register_builtin_with_config()` for details

## Documentation Structure

### Directory Structure

```
.opencode/skills/
├── agent-loop/          # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── auth/               # AuthConfig, Credential, AuthResolver, CredentialStore, mask_secret, codegg auth CLI
├── caching/            # Provider response caching (not yet wired in)
├── client/             # Remote TUI client, WebSocket
├── command/            # Slash commands, templates, execution
├── compaction/         # Context compaction strategies
├── config/             # Config loading, validation, encryption, watching
├── crypto/             # API key encryption
├── diff/               # Inline diff visualization
├── e2e/                # End-to-end testing guide
├── error/              # AppError, ProviderError, ToolError, is_retryable
├── event-bus/          # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── exec/               # Exec mode for CI/CD
├── git/                # Git session, git info in prompts, worktree management
├── goal/               # Long-horizon goal runtime, budget enforcement, auto-continuation
├── hooks/              # Hooks system for agent lifecycle
├── ide/                # IDE integration (VS Code, JetBrains)
├── lsp/                # LSP client, diagnostics, code operations
├── mcp/                # MCP connection manager
├── memory/             # Memory system, consolidation, patterns
├── mode/               # Mode system (Review/Debug/Docs)
├── model-dialog/       # Model selection/config dialog
├── notifications/       # Desktop notifications
├── permission/         # PermissionChecker, DoomLoop, PermissionRegistry
├── plugin/             # WASM plugin system
├── provider/           # LLM provider implementations
├── shell_session/      # Shell session metadata
├── question-response/  # Question/permission response shapes
├── resilience/          # Circuit breaker, FallbackProvider
├── router/             # Model auto-routing
├── sandbox/            # Landlock filesystem sandboxing
├── search/             # Legacy in-tree web search providers (builtin fallback)
├── search_backend/     # Pluggable backend for `websearch`/`webfetch` (eggsearch MCP)
├── security/           # SSRF, symlink protection, Landlock
├── server/             # HTTP/WebSocket server for remote TUI
├── session/            # Session storage, database schema
├── skills/             # Skill loading, activation, SkillIndex
├── snapshot/           # File state capture and restore
├── storage/            # SQLite initialization, pragmas
├── theme/              # Theme registry, semantic schema, Halloy compat, projections
├── subagent/           # SubAgentPool, SubAgentSpawner
├── team/               # Multi-agent team coordination
├── testing/            # Testing guide (unit, integration, E2E)
├── tool/               # Tool path validation, async command
├── tool-search/        # Tool discovery and catalog
├── tts/                # Text-to-speech module
├── tui/                # Terminal UI, keyboard shortcuts
├── tui_input/          # TUI input handling, paste, bindings
├── tui-dialog-maintenance/  # TUI dialog maintenance guide
├── tui-dialog-testing/      # TUI dialog testing guide
├── upgrade/            # Self-upgrade via GitHub releases
├── util/               # Clipboard, fuzzy matching, truncation
└── worktree/           # Git worktree management
```

### Adding New Module Guidance

When adding guidance for a new module:

1. Create `.opencode/skills/<module>/SKILL.md` with YAML frontmatter
2. Add the module to the skills directory structure above
3. Add the module to the Quick Reference table
4. Use frontmatter: `name`, `description`, `version`, `process`

### File Naming Convention

- `AGENTS.md` - Root index file (this file)
- `.opencode/skills/<name>/SKILL.md` - Module-specific skill guides
- `architecture/<module>.md` - Architecture documentation per module

## Quick Reference

| Topic | Location |
|-------|----------|
| Shell Session (shell session metadata) | `.opencode/skills/shell_session/SKILL.md` |
| Agent (AgentLoop, compaction, router, team, turn runtime) | `.opencode/skills/agent-loop/SKILL.md` |
| Event Bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | `.opencode/skills/event-bus/SKILL.md` |
| TUI (keyboard shortcuts, FocusManager, Component trait) | `.opencode/skills/tui/SKILL.md` |
| Core (CoreClient facade, transports, protocol envelopes) | `.opencode/skills/core/SKILL.md` |
| Search Backend (eggsearch MCP, `websearch`/`webfetch`, trust framing) | `.opencode/skills/search_backend/SKILL.md` |
| Security (SSRF, symlinks, Landlock) | `.opencode/skills/security/SKILL.md` |
| WASM plugins | `.opencode/skills/plugin/SKILL.md` |
| MCP (Model Context Protocol) | `.opencode/skills/mcp/SKILL.md` |
| Provider (LLM providers, Arc<String> types, FallbackProvider) | `.opencode/skills/provider/SKILL.md` |
| Codegg Config (Config, paths, validation, watching) | `crates/codegg-config/` |
| Codegg Protocol (CoreRequest, CoreResponse, CoreEvent, TuiMessage) | `crates/codegg-protocol/` |
| Codegg Providers (Provider trait, ProviderRegistry, auth types, CircuitBreaker) | `crates/codegg-providers/` |
| Codegg Core (bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions) | `crates/codegg-core/` |
| Auth (AuthConfig, Credential, AuthResolver, CredentialStore, mask_secret, `codegg auth ...` CLI) | `.opencode/skills/auth/SKILL.md`, `architecture/auth.md` |
| Crypto (API key encryption, Argon2id key derivation) | [architecture/crypto.md](architecture/crypto.md) |
| Error (AppError, ProviderError, ToolError, is_retryable, CircuitOpen) | `.opencode/skills/error/SKILL.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `.opencode/skills/resilience/SKILL.md` |
| Permission (mode system, PermissionChecker, DoomLoop, PermissionRegistry) | `.opencode/skills/permission/SKILL.md` |
| LSP (Language Server Protocol, diagnostics, code operations) | `.opencode/skills/lsp/SKILL.md` |
| Tool (path validation, async command, ToolExecutor, ToolCatalog) | `.opencode/skills/tool/SKILL.md` |
| Exec mode | `.opencode/skills/exec/SKILL.md` |
| Hooks system | `.opencode/skills/hooks/SKILL.md` |
| Client (remote TUI, WebSocket) | `.opencode/skills/client/SKILL.md` |
| Server (HTTP, WebSocket, REST API, SSE) | `.opencode/skills/server/SKILL.md` |
| Snapshot (file state capture and restore) | `.opencode/skills/snapshot/SKILL.md` |
| Skills (skill system overview) | `.opencode/skills/skills/SKILL.md` |
| Command (slash commands, templates, execution) | `.opencode/skills/command/SKILL.md`
| Git (git session, git info in prompts) | `.opencode/skills/git/SKILL.md` |
| IDE (VS Code, JetBrains detection, diff viewing) | `.opencode/skills/ide/SKILL.md` |
| Config (loading, validation, encryption, watching) | `.opencode/skills/config/SKILL.md` |
| Memory (session-to-session learning, consolidation) | `.opencode/skills/memory/SKILL.md` |
| Session (storage, SQLite, checkpoint, import/export) | `.opencode/skills/session/SKILL.md` |
| Storage (SQLite initialization, pragmas, pooling) | `.opencode/skills/storage/SKILL.md` |
| Theme (registry, semantic schema, Halloy compat, projections) | `.opencode/skills/theme/SKILL.md` |
| Upgrade (GitHub releases, self-upgrade) | `.opencode/skills/upgrade/SKILL.md` |
| Worktree (git worktrees, find_git_root) | `.opencode/skills/worktree/SKILL.md` |
| Subagent (SubAgentPool, SubAgentSpawner, worker) | `.opencode/skills/subagent/SKILL.md` |
| Compaction (context compaction strategies) | `.opencode/skills/compaction/SKILL.md` |
| Router (model auto-routing) | `.opencode/skills/router/SKILL.md` |
| Util (clipboard, fuzzy matching, truncation, metrics) | `.opencode/skills/util/SKILL.md` |

## Testing Commands

```bash
# Always run before/after changes
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features

# Specific feature testing
cargo test --all-features -- --test-threads=1  # For integration tests

# TUI tests
cargo test tui::input
cargo test tui
cargo test messages

# Run specific module tests
cargo test --package codegg -- <module>_test_pattern

# Test the native tool crates
cargo test -p eggsentry
cargo test -p eggcontext
cargo test -p egggit
cargo test -p egglsp

# Test the extracted workspace crates
cargo test -p codegg-config
cargo test -p codegg-protocol
cargo test -p codegg-providers
cargo test -p codegg-core

# Quick cargo aliases (defined in .cargo/config.toml)
cargo ck           # check --workspace --all-targets
cargo ckroot       # check -p codegg
cargo ckprotocol   # check -p codegg-protocol
cargo ckconfig     # check -p codegg-config
cargo ckproviders  # check -p codegg-providers
cargo ckcore        # check -p codegg-core
cargo cksplit      # check all split crates + root
```

## Security Reminders

- Security-sensitive changes require additional test coverage
- SSRF protection follows RFC 6892
- Command injection follows OWASP Cheat Sheets
- Path traversal follows OWASP File Upload guidance
- Feature gates: Changes to server/plugin modules need `--all-features` testing