# AGENTS.md

## Project Overview

This is a **Rust rewrite of an AI coding agent**, built for performance and efficiency. The codebase uses:

- **Tokio** for async runtime
- **SQLx** for SQLite database
- **Ratatui** for terminal UI
- **Axum** for HTTP server (feature-gated)
- **Wasmtime** for WASM plugins (feature-gated)

## Module Reference (32 Modules)

| Module | Purpose |
|--------|---------|
| `agent/` | Main agent loop, message processing, subagent pool, prompt templates, compaction, routing, team coordination |
| `bus/` | Event bus system (GlobalEventBus, PermissionRegistry, QuestionRegistry) |
| `client/` | Remote TUI client for WebSocket connections |
| `command/` | Slash command registry and routing from markdown files |
| `config/` | Configuration loading, validation, and file watcher |
| `crypto/` | AES-256-GCM encryption with Argon2id key derivation |
| `error/` | Centralized `AppError` enum with `ProviderError::is_retryable()`, `ToolError::is_retryable()`, `CircuitError` conversion |
| `exec/` | Non-interactive exec mode for CI/CD with JSON I/O |
| `hooks/` | Hooks system for agent loop lifecycle events and plugin interaction |
| `ide/` | IDE integration (VS Code IPC, JetBrains remote mode) |
| `lsp/` | Language Server Protocol support (diagnostics, code operations) |
| `mcp/` | Model Context Protocol client (local, remote, auth) with auto-reconnect |
| `memory/` | Persistent memory system for session learning and namespace management |
| `permission/` | Access control, path restrictions, DoomLoop detection, mode system |
| `plugin/` | WASM plugin system with hooks and TUI extensions |
| `provider/` | LLM provider implementations (Anthropic, OpenAI, Google, etc.) |
| `pty/` | Shell session metadata management (in-memory, no actual PTY) |
| `resilience/` | Circuit breaker, retry mechanisms, and rate limiting |
| `security/` | SSRF protection, internal IP validation, Landlock sandboxing |
| `server/` | HTTP server (Axum) with WebSocket support for remote TUIs |
| `session/` | Session storage, message history, and checkpointing (SQLite) |
| `skills/` | Skill system for specialized capabilities (git, research, etc.) |
| `snapshot/` | Snapshot support for file state capture and restore |
| `storage/` | SQLite database storage layer and initialization |
| `tool/` | Built-in tools (bash, read, edit, task, webfetch, etc.) |
| `tts/` | Text-to-speech module with provider support |
| `tui/` | Terminal user interface (widgets, handlers, input processing, diff viewer, notifications, image support) |
| `upgrade/` | Self-upgrade functionality via GitHub releases |
| `util/` | Utility functions (clipboard, fuzzy search, etc.) |
| `worktree/` | Git worktree support for project management |

## Critical Implementation Notes (from Review Sessions)

These items were identified during module reviews and are important for future agents to know:

### Event Bus Module (2026-05-22)
- **GlobalEventBus::publish() returns subscriber count on success**: Uses `trace` level for normal events (was `warn` for all cases). Channel closed errors properly distinguished.
- **Event flow documentation accurate**: Registration-before-publish pattern correctly documented in both `architecture/event-bus.md` and `.opencode/skills/event-bus/SKILL.md`
- **Dead events removed from skill**: `PermissionRequested`, `PermissionGranted`, `PermissionDenied` removed from skill (never existed in code)

### Crypto Module (2026-05-22)
- **Crypto module updated**: architecture/crypto.md now accurately describes the implementation (Argon2id key derivation, v2 format with `v2:` prefix, legacy HMAC-SHA256 support)
- **Skill synchronized**: `.opencode/skills/crypto/SKILL.md` updated to match implementation

### Error Module (2026-05-22)
- **Architecture updated**: architecture/error.md now accurately describes the implementation (all error variants, is_retryable methods, HTTP status mapping)
- **Skill created**: `.opencode/skills/error/SKILL.md` created with comprehensive error handling guidance
- **Exec mode error classification expanded**: All AppError variants now have distinct error codes (was using catch-all EXECUTION_ERROR for many cases)
- **ProviderError::NotFound handled**: Added classification for provider not found errors
- **ToolError variants handled**: NotFound, Timeout, Permission, Disabled now have distinct codes

### Tool Module (2026-05-22)
- **Skill updated**: `.opencode/skills/tool/SKILL.md` updated to version 1.1.0 with accurate `ToolCatalog` and error variants
- **Tool trait accurately documented**: `execute()` takes only `serde_json::Value` (not `ToolContext` as shown in architecture doc)
- **ToolResult matches implementation**: Actual struct has `tool_name`, `input`, `output`, `success` (not the `content`, `error`, `metadata` shown in architecture)
- **ToolCatalog added**: Registry now includes `ToolCatalog` for metadata and deferred loading
- **plan_enter/plan_exit split**: Two separate tools in `plan.rs`, not a single `plan` tool
- **ToolExecutor with retry**: Exponential backoff with jitter for transient errors
- **BashTool security**: Regex-based blocked patterns for dangerous commands, optional Landlock sandboxing
- **Bug fixed**: `normalized` variable shadowing in `check_command_security()` - removed duplicate declaration
- **Bug fixed**: Redundant `unrestricted` captures in `glob.rs` and `grep.rs` - now properly uses closure-captured value

### Agent Module (2026-05-22)
- **Architecture doc updated**: `architecture/agent.md` now accurately reflects the actual implementation
- **AgentLoop struct fixed**: Architecture doc showed wrong field types (`agent: Agent` vs actual `agents: HashMap<String, Agent>`, `client: Client` didn't exist, `provider: Arc<ProviderRegistry>` vs actual `Box<dyn Provider>`)
- **`start_workers()` removed**: Dead no-op method removed from `SubAgentPool`
- **SubAgentSpawner code deduplication**: `send()` and `send_async()` now share implementation via `enqueue_request()` and `handle_response()` helpers
- **BackgroundScheduler task_id fixed**: Uses `task.id.parse()` to use actual task ID instead of `rand::random()`
- **Skill synchronized**: `.opencode/skills/subagent/SKILL.md` updated to reflect current API

### Verified Correct Items (not bugs)
- **Subagent event publishing**: `SubagentStarted`/`SubagentProgress`/`SubagentCompleted`/`SubagentFailed` events properly published via `GlobalEventBus`
- **`SubAgentPool` bounded concurrency**: Properly uses semaphore with default of 5, RAII guard pattern for active_count
- **Tool definition caching**: Properly versioned cache key (uses mcp_tool_count as proxy - see known limitation)
- **DoomLoop detection**: Implementation correctly uses window-based counting (not consecutive), and docstring accurately describes this
- **`decrypt_provider_keys()` is called in `Config::load()`**: API keys encrypted via `save()` are now automatically decrypted on load (fixed 2026-05-21)
- **`decrypt_provider_keys()` is called in `ConfigWatcher::reload_config()`**: Hot-reload now properly decrypts API keys (fixed 2026-05-22)
- **ProviderConfig merge is field-by-field**: When merging configs with same provider, fields are merged individually (fixed 2026-05-21)
- **`medium_model` is validated**: Validates `provider/model` format like `model` and `small_model` (fixed 2026-05-21)
- **`ProviderError::is_retryable()` implemented**: Centralizes retry logic for provider errors (added 2026-05-22)
- **`ToolError::is_retryable()` implemented**: Centralizes retry logic for tool errors (added 2026-05-22)
- **CircuitError → ProviderError::CircuitOpen conversion**: `From<CircuitError>` impl enables circuit breaker error propagation (added 2026-05-22)
- **`FallbackProvider` uses `CircuitOpen`**: Circuit breaker errors now create `ProviderError::CircuitOpen` instead of generic `ProviderError::api()` (fixed 2026-05-22)
- **SSE GlobalEventBus fixed**: SSE handler at `/api/event` now subscribes directly to `crate::bus::global::GlobalEventBus::subscribe()` instead of using isolated State parameter (fixed 2026-05-22)
- **Exec mode session_id**: `session_id` parameter in `ExecMode::new()` is now properly used (was ignored before, now falls back to UUID if None) (fixed 2026-05-22)
- **Exec mode error classification**: `CircuitOpen`, `Api`, and `Stream` errors now properly classified with distinct error codes (fixed 2026-05-22)
- **Exec mode config errors**: Config loading errors now properly returned as `CONFIG_ERROR` instead of silently using defaults (fixed 2026-05-22)
- **Exec mode question channel**: `setup_question_channel()` is now called in exec mode for proper question tool handling (fixed 2026-05-22)

### Exec Module (2026-05-22)
- **architecture/exec.md updated**: Now accurately describes the implementation (was showing outdated API with `task`/`workspace`/`context` fields)
- **ExecInput uses `prompt` field**: Not `task` as shown in previous architecture doc
- **`_duration_ms` fixed**: Error path now uses duration_ms instead of silently ignoring it
- **Skill synchronized**: `.opencode/skills/exec/SKILL.md` updated to match implementation

### Hooks System (2026-05-22)
- **ToolExecuteBefore/After plugin hooks called**: Both hooks ARE invoked in `execute_tool_calls()` at loop.rs:1764 and 1806. `ToolExecuteBefore` can block execution by returning `blocked: true`.
- **Shell hook config validation added**: Invalid event names (e.g., typos) now log warnings instead of silently failing. InlineScript is now deprecated with warning.
- **Plugin hook timeout errors include plugin_id**: Error message format changed from `"hook timeout: hook execution timed out"` to `"{plugin_id}: hook timeout: hook execution timed out"`.
- **ShellCommandHook PATH fixed**: Now uses user's actual `PATH` via `std::env::var_os("PATH")` instead of hardcoded `/usr/local/bin:/usr/bin:/bin`.
- **Early return bug fixed**: Stream errors now break the agent loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks always run.

### MCP Module (2026-05-22)
- **DNS rebinding protection fixed**: `validated_ips` changed to `Arc<Mutex<Option<Vec<IpAddr>>>>` so clones share state. `initialize()` now re-validates DNS on each call, preventing bypass after reconnects.
- **Race condition in ensure_connected()**: Refactored to clone all fields before `tokio::spawn`, avoiding borrow after spawn.
- **Hardcoded PATH fixed**: `LocalClient::initialize()` now uses user's actual PATH via `std::env::var_os("PATH")`, falls back to safe default.
- **Spawn timeout added**: Process spawn wrapped in `tokio::time::timeout` + `spawn_blocking`, capped at 10s to prevent hangs.
- **Auto-reconnect via McpConnectionManager**: Exponential backoff (1s→2s→4s→...→max 60s), max 5 retries, heartbeat every 30s. `ensure_connected()` spawns reconnection in background task.

### Known Issues (Lower Priority)
- **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.
- **Tool definition cache staleness**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

### Memory Module (2026-05-22)
- **File-based storage**: `~/.config/codegg/memory/` with namespace-based directories
- **Memory persistence fixed**: All 8 fields now saved/loaded correctly (was: only 4 fields persisted)
- **Hierarchical namespaces**: Namespaces like `user/preferences` and `project/{hash}/conventions` now work correctly
- **Consolidation system**: Rule-based pattern detection with importance scoring
- **Memory commands implemented**: `/memory` dashboard, `/memory-search`, `/memory-list`, `/memory-remember`, `/memory-forget`, `/memory-consolidate`
- **During-session memory**: `/memory-remember <text>` allows saving memories mid-session
- **Negation scoring fixed**: Negations ("don't use", "never") now correctly reduce importance (was: documentation said +8, code actually subtracted)
- **Auto-run**: `experimental.memory_auto_consolidate` config option enables automatic consolidation on session end

### Client Module (2026-05-22)
- **Health endpoint fixed**: `RemoteClient::health()` now uses `GET /health` instead of `GET /api/providers`
- **Health check error propagation**: Non-success HTTP status now returns `Err(ClientError::Unreachable)` instead of `Ok(false)`

### Client Module Review (2026-05-22)
- **Architecture doc accurate**: `architecture/client.md` correctly describes the implementation
- **Client skill line numbers updated**: `new_remote()` at line 492, `handle_remote_event()` at line 686
- **RenderFrame marked as legacy**: `RenderFrame` variant exists in `TuiMessage` but server doesn't send it
- **Skills fixed**: event-bus, server, tool skills updated for accuracy

### Permission Module (2026-05-22)
- **Architecture doc updated**: `architecture/permission.md` now accurately reflects the implementation
- **Skill synchronized**: `.opencode/skills/permission/SKILL.md` updated to match implementation
- **Dead events removed**: `PermissionRequested`, `PermissionGranted`, `PermissionDenied` were never used - removed from `AppEvent` (they were different from `PermissionPending`/`PermissionResponded`)
- **Mode bug fixed**: `docs` mode incorrectly listed `write` as both allowed (line 171) and restricted (line 178) - removed from `restricted_tools` in `BuiltinModes::docs()`
- **PermissionRegistry location noted**: Located in `src/bus/mod.rs`, not `src/permission/`

### IDE Module (2026-05-22)
- **Temp file flushing fixed**: `open_diff_vscode()` and `open_diff_jetbrains()` now properly flush temp files via `as_file()` + `flush()` before passing paths to IDE. Previously wrote to `tempfile` without proper handle acquisition.
- **open_diff_generic() fixed**: Now uses `std::env::split_paths()` (portable PATH parsing) and creates temp files with content instead of passing original paths directly. Matches behavior of IDE-specific handlers.
- **Skill synchronized**: `.opencode/skills/ide/SKILL.md` updated to version 1.2.0

### LSP Module (2026-05-22)
- **PATH parsing fixed**: `download.rs` now uses `std::env::split_paths()` instead of splitting by `MAIN_SEPARATOR` (which was broken on Unix where PATH uses `:` not `/`)
- **PHP server mapping fixed**: `language.rs` now maps PHP to `php-language-server` instead of non-existent `intelephense`
- **New server definitions added**: `perl-language-server`, `powershell-editor-services`, `graphql-language-server`, `buf-language-server`, `r-languageserver`, `nimlsp`, `vls`
- **Request timeout added**: `send_request()` in `client.rs` now has 30-second timeout with `LspError::RequestTimeout`
- **Hardcoded PATH fixed**: `launch.rs` now preserves user's actual PATH instead of hardcoding `/usr/local/bin:/usr/bin:/bin`
- **Stderr logging**: Server stderr is now drained and logged during LSP client initialization
- **Notification loop redundancy fixed**: `send_request()` now has cleaner notification handling with proper error logging on send failure
- **close_file race condition fixed**: Uses single write lock and properly removes file from `opened_files` after closing
- **save_file race condition fixed**: Uses single write lock instead of drop-read-then-acquire-write pattern

### Plugin Module (2026-05-22)
- **Architecture doc updated**: `architecture/plugin.md` now accurately describes the implementation (was showing outdated JSON manifest, wrong HookEvent/HookResult types)
- **Skill synchronized**: `.opencode/skills/plugin/SKILL.md` updated to version 1.1.0
- **WASM path fixed**: `execute_wasm_hook()` now correctly builds `plugins/{plugin_id}/plugin.wasm` path instead of using incorrect path format
- **WASM path type fixed**: `wasm_path_str = wasm_path.to_string_lossy()` to pass `&str` to `get_or_compile()`
- **Builtin handler registry fixed**: `BUILTIN_HANDLERS` now uses `LazyLock` with proper fn pointer casting to avoid fn item type mismatches
- **API HookType serialization fixed**: `api::hooks::HookType::as_str()` now returns dot notation (e.g., `tool.execute.before`) matching actual `HookType` implementation
- **Feature flag named correctly**: Uses `plugins` feature, not `plugin`

### Provider Module (2026-05-22)
- **Architecture doc updated**: `architecture/provider.md` now accurately reflects the implementation (was showing outdated Provider trait signature, wrong Message/ChatEvent types, incorrect ProviderRegistry methods)
- **Skill synchronized**: `.opencode/skills/provider/SKILL.md` updated with accurate types (Arc<String> usage, async trait methods)
- **Stale plan_registry.rs reference removed**: Architecture doc incorrectly referenced `wait_for_response()` bug in non-existent `plan_registry.rs`
- **Message types use Arc<String>**: All content fields use `Arc<String>` for efficiency - use `.into()` when creating
- **ChatEvent uses variants**: `TextDelta(Arc<String>)`, `ToolCall(ToolCall)`, `Finish{...}` - not the old struct variants shown in previous doc
- **FallbackProvider exponential backoff**: Retry delay is `2^i` seconds (capped at 30s) - correctly implemented
- **ProviderError::CircuitOpen**: Circuit breaker integration properly propagates open state via `CircuitError → ProviderError::CircuitOpen`

### Resilience Module (2026-05-22)
- **CircuitBreaker is_available() TOCTOU fixed**: Now uses write lock from the start instead of read-then-write, eliminating the time-of-check-time-of-use race. Implementation uses single `write().await` acquisition.
- **FallbackProvider exponential backoff**: Retry delay is `2^i` seconds (capped at 30s) - correctly implemented
- **ProviderError::CircuitOpen**: Circuit breaker integration properly propagates open state via `CircuitError → ProviderError::CircuitOpen`

### Worktree Module (2026-05-22)
- **find_git_root() bug fixed**: Now correctly detects worktrees by checking if `.git` is a file containing `gitdir:` prefix, not just a directory. Previously would fail to find git root when called from inside a worktree.
- **Architecture doc updated**: `architecture/worktree.md` now accurately reflects the implementation
- **Skill created**: `.opencode/skills/worktree/SKILL.md` created with module guidance
- **`Worktree` struct differs from doc**: `is_locked` and `is_main` are not implemented; actual fields are `path`, `branch`, `is_current`, `is_detached`
- **`remove_worktree()` signature differs**: Does not have `force` parameter as shown in old doc
- **`create_worktree()` signature differs**: Has `create_branch: bool` parameter (not shown in old doc)

### PTY Module (2026-05-22)
- **Architecture doc updated**: `architecture/pty.md` now accurately describes the implementation (was showing outdated `SessionManager` API with wrong field types)
- **`update_cwd` method added**: `PtyManager::update_cwd()` now exists and returns `Result<PtySession, StorageError>` (was documented but not implemented)
- **Skill created**: `.opencode/skills/pty/SKILL.md` created with module guidance

### Security Module (2026-05-22)
- **revalidate_dns() IPv6 fix**: Fixed to handle IPv4-mapped IPv6 addresses. When DNS returns IPv6 address that maps to an already-validated IPv4, it now correctly continues instead of false positive DNS rebinding detection. The check uses `ipv6_segments_to_ipv4()` to detect mapped addresses.
- **Architecture doc inaccurate**: `architecture/security.md` showed `is_internal_ip(&str)` and `validate_url_host(&Url)` but actual signatures are `is_internal_ip(&IpAddr)` and `validate_url_host(&str)`. SSRFChecker and LandlockSandbox structs don't exist - actual structs are standalone functions and `SandboxConfig`.
- **Skill synchronized**: `.opencode/skills/security/SKILL.md` and `.opencode/skills/sandbox/SKILL.md` updated with accurate types and function signatures.

### Security Module (2026-05-22 - fixes)
- **validate_path_safety() symlink check**: Added symlink check before canonicalization to prevent symlink traversal attacks. Uses `path.symlink_metadata()` to detect if the path itself is a symlink.
- **validate_path_safety() tests added**: Added `test_validate_path_safety` and `test_validate_path_safety_with_symlink` unit tests to verify path validation and symlink rejection.

### Server Module (2026-05-22)
- **WsRateLimiter shared**: `WsRateLimiter` in `ServerState` is now shared across all WebSocket connections (was created per-connection, causing inefficient rate limiting)
- **SSE GlobalEventBus fixed**: SSE handler at `/api/event` now subscribes directly to `crate::bus::global::GlobalEventBus::subscribe()` instead of using isolated State parameter
- **Dead EventBus removed**: `routes/event.rs` had an unused local `EventBus` struct - removed, SSE now uses `GlobalEventBus` directly
- **Health route simplified**: `routes/health.rs` simplified to just `health_check()` function (unused `Router` builder removed)
- **Auth deduplication**: `validate_ws_auth()` function now shared between `handle_ws` and `handle_tui` (was duplicated inline code)
- **rpc.rs status corrected**: `rpc.rs` is NOT unused - it defines `JsonRpcMessage` struct used by `ws.rs` for JSON-RPC responses

### Session Module (2026-05-22)
- **StorageError variants expanded**: Added `Import` and `Export` error variants to `StorageError` enum (were using `Database` variant)
- **CheckpointStore now exported**: `CheckpointStore` is now re-exported from `session::mod.rs` for use by other modules
- **generate_slug now exported**: `generate_slug` helper function is now `pub` and re-exported from `session::mod.rs`
- **Skill updated**: `.opencode/skills/session/SKILL.md` updated to version 1.1.0 with complete API documentation

### Snapshot Module (2026-05-22)
- **Architecture doc outdated**: `architecture/snapshot.md` was significantly out of date - updated to reflect actual implementation
- **Skill synchronized**: `.opencode/skills/snapshot/SKILL.md` updated with correct API signatures
- **`Snapshot` struct field `data`**: Stores files as JSON string, not as direct `HashMap` field
- **`SnapshotView` has `files`**: The `files: HashMap<String, FileSnapshot>` field is on `SnapshotView`, not `Snapshot`
- **`SnapshotManager::new()` requires `SqlitePool`**: Constructor signature is `new(pool, project_root)`, not just `new(project_root)`
- **restore() takes SnapshotView**: `restore(&self, snapshot: &SnapshotView)` not `restore(&self, id: &str)`
- **capture_incremental path validation**: Added validation that paths are within `project_root` before accepting
- **Restore error messages improved**: Now include file path on failure (e.g., "failed to write /path/file: ...")

### Storage Module (2026-05-22)
- **Architecture doc updated**: `architecture/storage.md` now accurately reflects the implementation (was showing incorrect `init()` return type, wrong `cache_size`, missing pragmas)
- **Skill created**: `.opencode/skills/storage/SKILL.md` created with module guidance
- **Pragmas batched**: Individual PRAGMA queries consolidated into single query for efficiency
- **Database struct is wrapper**: `Database` is simple wrapper around `SqlitePool` - most code uses `init()` directly
- **Migrations in session module**: Actual migration logic lives in `src/session/schema.rs`, not storage module

### Skills Module (2026-05-22)
- **Blocking fs calls fixed**: `load_dir()` and `parse_skill_file()` now use `tokio::fs` instead of `std::fs` for proper async I/O
- **Architecture doc inaccurate**: `architecture/skills.md` showed `SkillIndex` with `RwLock<HashMap<String, Skill>>` but actual is `Vec<Skill>` with no RwLock
- **SkillIndex methods differ**: Doc showed `load_from_dir()` and `search()` but actual are `load(project_dir)` and `find_matching(query)`
- **Skill struct differs**: Actual has `source: PathBuf` and `version: Option<String>` not in doc
- **Skill created**: `.opencode/skills/skills/SKILL.md` created with module guidance

### Implementation Patterns
- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.
- **MCP reconnect wired up**: Heartbeat failures now trigger reconnect via `reconnect_needed` Notify mechanism
- **MCP DNS re-validation**: `RemoteClient::initialize()` re-validates DNS on each call (connect/reconnect), keeping `validated_ips` current
- **MCP ensure_connected()**: Clones all fields before `tokio::spawn` to avoid borrow-after-spawn issues
- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`
- **Component trait**: All dialogs implement `Component` trait with `handle_key`, `update`, `render` methods
- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event
- **ResyncRequired serialization**: Server uses `TuiMessage::ResyncRequired` variant directly (not raw JSON)
- **Client timeouts**: Health check has 10s timeout, WebSocket connection has 30s timeout
- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`

### TTS Module (2026-05-22)
- **Error handling improved**: `speak()` now returns `Err(AppError::Io(...))` when `say` command fails instead of silently ignoring failures. Callers handle errors appropriately.
- **Skill synchronized**: `.opencode/skills/tts/SKILL.md` updated with error handling documentation

### Util Module (2026-05-22)
- **Architecture doc updated**: `architecture/util.md` now accurately reflects the implementation (was showing incorrect function signatures and types)
- **Skill created**: `.opencode/skills/util/SKILL.md` created with module guidance
- **clipboard.rs naming differs**: Actual functions are `copy_to_clipboard()` / `read_from_clipboard()` (not `copy()` / `paste()` as shown in doc)
- **fuzzy.rs API differs**: `fuzzy_match()` takes `&[String]` candidates returning `Vec<(String, usize)>`, not `Option<(usize, usize)>`. `fuzzy_score()` returns `usize`, not a `FuzzyScore` struct
- **truncate.rs differs**: Actual functions are `truncate_lines()` / `truncate_bytes()` (not `truncate()` / `truncate_with_ellipsis()` as shown in doc)
- **stat_core.rs misnamed**: Contains metrics infrastructure (Counter, Gauge, Histogram), not file statistics as name suggests
- **Feature gate noted**: Clipboard requires `arboard` feature flag

### Upgrade Module (2026-05-22)
- **Architecture doc updated**: `architecture/upgrade.md` now accurately reflects the implementation (was showing outdated `ReleaseInfo` struct and `github_api::get_latest_release` function)
- **Skill created**: `.opencode/skills/upgrade/SKILL.md` created with module guidance
- **Hardcoded PATH fixed**: `upgrade()` now uses user's actual PATH via `std::env::var_os("PATH")` instead of hardcoded `/usr/local/bin:/usr/bin:/bin`
- **Architecture missing `VersionInfo`**: Actual struct is `VersionInfo` with `current`, `latest`, `needs_update` fields (not `ReleaseInfo` with `version`, `tag_name`, `download_url`, `release_notes`)
- **Upgrade configuration not implemented**: Architecture showed `[upgrade]` config section but no such configuration is loaded in the module

### TUI Module (2026-05-22)
- **Architecture doc updated**: `architecture/tui.md` now accurately reflects the implementation (was showing outdated routes like Chat/Sessions/Settings, wrong App struct, missing FocusManager/Component)
- **Routes accurate**: Actual routes are `Home` and `Session(String)` - not the `Chat`, `Sessions`, `Settings`, `Skills`, `Permissions` shown in old doc
- **Dialog variants fixed**: Doc now shows all 21 Dialog variants including Context, Cost, Usage, Goto, Plan, Diff, Confirm
- **Dead code removed**: `render_dialog()` no longer has unreachable `{}` block after FocusManager render
- **State inconsistency handling added**: `on_key()` now properly handles case when `dialog.is_open()` but `focus_manager.is_empty()` - logs error and resets state instead of panicking via debug_assert
- **Skill updated**: `.opencode/skills/tui/SKILL.md` updated with `push_dialog()` method for temporary dialogs and defensive state consistency check

### Command Module (2026-05-22)
- **Async file loading**: `find_command_files()` and `load_command_from_file()` now use `tokio::fs` for async I/O (were using blocking `std::fs`)
- **`subtask` field deprecated**: Added `#[deprecated]` attribute to `subtask` field since it's not yet implemented
- **Architecture doc updated**: `architecture/command.md` now shows all 36 built-in commands (was showing 20)
- **Skill updated**: `.opencode/skills/command/SKILL.md` updated to version 1.1.0 with accurate line numbers and async API documentation

### Config Module (2026-05-22)
- **Dead code removed**: `find_tui_config()` and `load_tui_config()` removed from `paths.rs` and `mod.rs` - these functions were never used anywhere in the codebase
- **Skill updated**: `.opencode/skills/config/SKILL.md` updated to version 1.3.0 with accurate Config struct fields, ProviderConfig.api_key() method, and ConfigWatcher field documentation
- **Config struct complete**: Actual struct has ~45 fields (was documented with incomplete list showing ~20) - skill updated to reflect full schema
- **Architecture doc accurate**: `architecture/config.md` accurately describes the implementation after multiple review fixes

## Documentation Structure

### Directory Structure

```
.opencode/skills/
├── agent-loop/SKILL.md           # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── client/SKILL.md               # Remote TUI client, WebSocket
├── command/SKILL.md             # Slash commands, templates, execution
├── compaction/SKILL.md          # Context compaction strategies
├── config/SKILL.md               # Config loading, validation, encryption, watching
├── crypto/SKILL.md               # API key encryption
├── event-bus/SKILL.md           # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── error/SKILL.md                # AppError, ProviderError, ToolError, is_retryable, conversions
├── exec/SKILL.md                # Exec mode
├── hooks/SKILL.md               # Hooks system
├── ide/SKILL.md                 # IDE integration (VS Code, JetBrains)
├── lsp/SKILL.md                 # LSP client, diagnostics, code operations
├── memory/SKILL.md              # Memory system, consolidation, patterns
├── mcp/SKILL.md                 # MCP connection manager
├── mode/SKILL.md                # Mode system (Review/Debug/Docs)
├── permission/SKILL.md          # PermissionChecker, DoomLoop, PermissionRegistry
├── plugin/SKILL.md             # WASM sandboxing, fuel tracking
├── provider/SKILL.md            # Provider patterns, token estimation
├── resilience/SKILL.md           # Circuit breaker, FallbackProvider
├── router/SKILL.md              # Model auto-routing
├── sandbox/SKILL.md             # Landlock filesystem sandboxing
├── security/SKILL.md            # SSRF, symlink protection, Landlock
├── server/SKILL.md             # HTTP server, WebSocket, REST API, SSE
├── session/SKILL.md             # Session storage, database schema
├── skills/SKILL.md              # Skill loading, activation, SkillIndex
├── snapshot/SKILL.md            # Snapshot capture and restore
├── storage/SKILL.md             # SQLite database initialization, connection pooling
├── subagent/SKILL.md           # SubAgentPool, SubAgentSpawner, worker infrastructure
├── team/SKILL.md               # Multi-agent team coordination
├── tool/SKILL.md                 # Tool path validation, async command
├── tts/SKILL.md                # Text-to-speech module
├── tui/SKILL.md                  # Terminal UI, keyboard shortcuts
├── upgrade/SKILL.md              # Self-upgrade functionality via GitHub releases
├── util/SKILL.md                 # Clipboard, fuzzy matching, truncation, metrics
└── worktree/SKILL.md            # Git worktree management, find_git_root
```

### Adding New Module Guidance

When adding guidance for a new module:

1. Create `.codegg/docs/<module>/AGENTS.override.md`
2. Add the module to the table above
3. Place content specific to that module in its override file
4. For cross-cutting concerns (updates, roadmap, code quality), use `meta/AGENTS.override.md`

### File Naming Convention

- `AGENTS.md` - Root index file only (no module-specific content)
- `AGENTS.override.md` - Module-specific guidance that overrides/supplements root

## Quick Reference

| Topic | Location |
|-------|----------|
| PTY (shell session metadata) | `.opencode/skills/pty/SKILL.md` |
| Agent (AgentLoop, compaction, router, team) | `agent/AGENTS.override.md` |
| Event Bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | `.opencode/skills/event-bus/SKILL.md` |
| TUI (keyboard shortcuts, FocusManager, Component trait) | `.opencode/skills/tui/SKILL.md` |
| Security (SSRF, symlinks, Landlock) | `.opencode/skills/security/SKILL.md` |
| WASM plugins | `.opencode/skills/plugin/SKILL.md` |
| MCP connection manager | `mcp/AGENTS.override.md` |
| Provider (LLM providers, Arc<String> types, FallbackProvider) | `.opencode/skills/provider/SKILL.md` |
| Crypto (API key encryption, Argon2id key derivation) | [architecture/crypto.md](architecture/crypto.md) |
| Error (AppError, ProviderError, ToolError, is_retryable, CircuitOpen) | `.opencode/skills/error/SKILL.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `resilience/AGENTS.override.md` |
| Permission (mode system, PermissionChecker, DoomLoop, PermissionRegistry) | `.opencode/skills/permission/SKILL.md` |
| LSP (Language Server Protocol, diagnostics, code operations) | `.opencode/skills/lsp/SKILL.md` |
| Tool (path validation, async command, ToolExecutor, ToolCatalog) | `.opencode/skills/tool/SKILL.md` |
| Exec mode | `.opencode/skills/exec/SKILL.md` |
| Hooks system | `.opencode/skills/hooks/SKILL.md` |
| Client (remote TUI, WebSocket) | `.opencode/skills/client/SKILL.md` |
| Server (HTTP, WebSocket, REST API, SSE) | `.opencode/skills/server/SKILL.md` |
| Snapshot (file state capture and restore) | `snapshot/AGENTS.override.md` |
| Skills (skill system overview) | `skills/AGENTS.override.md` |
| Command (slash commands, templates, execution) | `.opencode/skills/command/SKILL.md` |
| IDE (VS Code, JetBrains detection, diff viewing) | `.opencode/skills/ide/SKILL.md` |
| Testing (E2E, unit, integration) | `meta/AGENTS.override.md` |
| Config (loading, validation, encryption, watching) | `.opencode/skills/config/SKILL.md` |
| Memory (session-to-session learning, consolidation) | `.opencode/skills/memory/SKILL.md` |
| Updates, roadmap, code quality | `meta/AGENTS.override.md` |
| Session (storage, SQLite, checkpoint, import/export) | `.opencode/skills/session/SKILL.md` |
| Storage (SQLite initialization, pragmas, pooling) | `.opencode/skills/storage/SKILL.md` |
| Skills (skill loading, activation, SkillIndex) | `.opencode/skills/skills/SKILL.md` |
| Upgrade (GitHub releases, self-upgrade) | `.opencode/skills/upgrade/SKILL.md` |
| Worktree (git worktrees, find_git_root) | `.opencode/skills/worktree/SKILL.md` |
| Subagent (SubAgentPool, SubAgentSpawner, worker) | `.opencode/skills/subagent/SKILL.md` |
| Compaction (context compaction strategies) | `.opencode/skills/compaction/SKILL.md` |
| Router (model auto-routing) | `.opencode/skills/router/SKILL.md` |