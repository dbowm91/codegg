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
| `client/` | Remote TUI client for WebSocket connections with resume/replay support |
| `command/` | Slash command registry and routing from markdown files |
| `config/` | Configuration loading, validation, and file watcher |
| `crypto/` | AES-256-GCM encryption with Argon2id key derivation |
| `error/` | Centralized `AppError` enum with `ProviderError::is_retryable()`, `ToolError::is_retryable()`, `CircuitError` conversion |
| `exec/` | Non-interactive exec mode for CI/CD with JSON I/O |
| `hooks/` | Hooks system for agent loop lifecycle events and plugin interaction |
| `ide/` | IDE integration (VS Code IPC, JetBrains remote mode) |
| `lsp/` | Language Server Protocol support (diagnostics, code operations) |
| `mcp/` | Model Context Protocol client (local, remote, auth) with auto-reconnect |
| `core/` | Core facade and transport adapters (inproc, stdio, socket) for request/response separation |
| `memory/` | Persistent memory system for session learning and namespace management |
| `permission/` | Access control, path restrictions, DoomLoop detection, mode system |
| `plugin/` | WASM plugin system with hooks and TUI extensions |
| `provider/` | LLM provider implementations (Anthropic, OpenAI, Google, etc.) |
| `protocol/` | Shared `CoreRequest`/`CoreResponse` and `TuiMessage` protocol envelopes |
| `pty_session/` | Shell session metadata management (in-memory, no actual PTY) |
| `resilience/` | Circuit breaker, retry mechanisms, and rate limiting |
| `security/` | SSRF protection, internal IP validation, Landlock sandboxing |
| `server/` | HTTP server (Axum) with WebSocket support for remote TUIs and replay buffering |
| `session/` | Session storage, message history, and checkpointing (SQLite) |
| `skills/` | Skill system for specialized capabilities (git, research, etc.) |
| `snapshot/` | Snapshot support for file state capture and restore |
| `storage/` | SQLite database storage layer and initialization |
| `tool/` | Built-in tools (bash, read, edit, task, webfetch, etc.) |
| `tts/` | Text-to-speech module with provider support |
| `tui/` | Terminal user interface (widgets, handlers, input processing, diff viewer, notifications, image support, CoreClient-backed flows) |
| `upgrade/` | Self-upgrade functionality via GitHub releases |
| `util/` | Utility functions (clipboard, fuzzy search, etc.) |
| `worktree/` | Git worktree support for project management |

## Architecture Index

- `architecture/core.md`: Core facade, transport adapters, request envelopes, and protocol boundaries
- `architecture/tui.md`: TUI state, dialog/component maintenance, and CoreClient-backed flows
- `architecture/client.md`: Remote TUI client, resume handshake, and replay-aware event handling
- `architecture/server.md`: WebSocket TUI server, replay buffer, and REST/SSE routes
- `architecture/skills.md`: Runtime skill loader plus the repo-maintained `.skills/` copy
- `plans/tui_separation.md`: Completed TUI/core separation plan and phase notes

## Critical Implementation Notes (from Review Sessions)

These items were identified during module reviews and are important for future agents to know:

### Event Bus Module (2026-05-22)
- **GlobalEventBus::publish() returns subscriber count on success**: Uses `trace` level for normal events (was `warn` for all cases). Channel closed errors properly distinguished.
- **Event flow documentation accurate**: Registration-before-publish pattern correctly documented in both `architecture/bus.md` and `.opencode/skills/event-bus/SKILL.md`
- **Dead events removed**: `PermissionRequested`, `PermissionGranted`, `PermissionDenied` removed from skill and architecture doc (never existed in code - only `PermissionPending`/`PermissionResponded` exist)
- **AppEvent count corrected**: 36 variants (was incorrectly documented as 38 or 40+)

### Crypto Module (2026-05-27)
- **Crypto module updated**: architecture/crypto.md now accurately describes the implementation (Argon2id key derivation, v2 format with `v2:` prefix, legacy HMAC-SHA256 support)
- **Skill synchronized**: `.opencode/skills/crypto/SKILL.md` updated to match implementation
- **FORMAT_V2_PREFIX documented**: The constant `pub const FORMAT_V2_PREFIX: &str = "v2:"` at `src/crypto/mod.rs:10` is now explicitly documented

### Error Module (2026-05-22)
- **Architecture updated**: architecture/error.md now accurately describes the implementation (all error variants, is_retryable methods, HTTP status mapping)
- **Skill created**: `.opencode/skills/error/SKILL.md` created with comprehensive error handling guidance
- **Exec mode error classification expanded**: All AppError variants now have distinct error codes (was using catch-all EXECUTION_ERROR for many cases)
- **ProviderError::NotFound handled**: Added classification for provider not found errors
- **ToolError variants handled**: NotFound, Timeout, Permission, Disabled now have distinct codes
- **StorageError documentation complete**: Import/Export variants now documented (architecture/error.md)
- **classify_error cleanup**: Uses direct `ToolError` imports instead of `crate::error::ToolError` prefix

### Tool Module (2026-05-22)
- **Skill updated**: `.opencode/skills/tool/SKILL.md` updated to version 1.2.0 with accurate `ToolCatalog` and error variants
- **Tool trait accurately documented**: `execute()` takes only `serde_json::Value` (not `ToolContext` as shown in architecture doc)
- **ToolResult matches implementation**: Actual struct has `tool_name`, `input`, `output`, `success` (not the `content`, `error`, `metadata` shown in architecture)
- **ToolCatalog added**: Registry now includes `ToolCatalog` for metadata and deferred loading
- **plan_enter/plan_exit split**: Two separate tools in `plan.rs`, not a single `plan` tool
- **ToolExecutor with retry**: Exponential backoff with jitter for transient errors
- **BashTool security**: Regex-based blocked patterns for dangerous commands, optional Landlock sandboxing
- **Bug fixed**: `normalized` variable shadowing in `check_command_security()` - removed duplicate declaration
- **Bug fixed**: Redundant `unrestricted` captures in `glob.rs` and `grep.rs` - now properly uses closure-captured value
- **Subprocess PATH fixed**: All tools now use user's actual PATH via `std::env::var_os("PATH")` instead of hardcoded paths (bash.rs, git.rs, terminal.rs, commit.rs, review.rs, formatter.rs) - fixes Homebrew/cargo/pyenv tool discovery

### Agent Module (2026-05-22)
- **Architecture doc updated**: `architecture/agent.md` now accurately reflects the actual implementation
- **AgentLoop struct fixed**: Architecture doc showed wrong field types (`agent: Agent` vs actual `agents: HashMap<String, Agent>`, `client: Client` didn't exist, `provider: Arc<ProviderRegistry>` vs actual `Box<dyn Provider>`)
- **`start_workers()` removed**: Dead no-op method removed from `SubAgentPool`
- **SubAgentSpawner code deduplication**: `send()` and `send_async()` now share implementation via `enqueue_request()` and `handle_response()` helpers
- **BackgroundScheduler task_id fixed**: Uses `task.id.parse()` to use actual task ID instead of `rand::random()`. Invalid IDs now cause the task to be skipped (logged with warning) rather than falling back to random.
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
- **Error messages include duration**: Error output now includes execution duration in milliseconds for debugging (e.g., `"Permission denied: Tool 'bash' denied by permissions (1234ms)"`)

### Hooks System (2026-05-25)
- **Shell hooks vs plugin hooks notation**: Shell hooks use underscore notation (`pre_tool_execute`) vs plugin hooks use dot notation (`tool.execute.before`)
- **AgentEnd hooks skipped on stream error**: Stream errors break the loop at `src/agent/loop.rs:1369`, causing `AgentEnd` hooks (lines 1518-1533) to be SKIPPED. `SessionEnd` hooks (lines 1539-1554) RUN because they are outside the loop.

### MCP Module (2026-05-22)
- **DNS rebinding protection fixed**: `validated_ips` changed to `Arc<Mutex<Option<Vec<IpAddr>>>>` so clones share state. `initialize()` now re-validates DNS on each call, preventing bypass after reconnects.
- **Race condition in ensure_connected()**: Refactored to clone all fields before `tokio::spawn`, avoiding borrow after spawn.
- **Hardcoded PATH fixed**: `LocalClient::initialize()` now uses user's actual PATH via `std::env::var_os("PATH")`, falls back to safe default.
- **Spawn timeout added**: Process spawn wrapped in `tokio::time::timeout` + `spawn_blocking`, capped at 10s to prevent hangs.
- **Auto-reconnect via McpConnectionManager**: Exponential backoff (1s→2s→4s→...→max 60s), max 5 retries, heartbeat every 30s. `ensure_connected()` spawns reconnection in background task.

### MCP Module (2026-05-23)
- **McpConnectionManager Clone impl fixed**: Removed derived `Clone` that was unsound due to `CancellationToken` being `!Clone`. Implemented `Clone` manually with proper `Arc::clone` for Arc fields.
- **OAuth replay protection race fixed**: Changed order so `mark_code_used()` is called BEFORE `exchange_code_for_tokens()`, eliminating race window. If exchange fails after marking, code remains used (acceptable - prevents replay of failed codes).
- **IdeServer async I/O**: `run_stdio()` now uses `tokio::io::stdin()/stdout()` with `AsyncBufReadExt` and `AsyncWriteExt` instead of blocking `std::io`. Added `io-std` feature to tokio in Cargo.toml.

### LSP Module (2026-05-23)
- **Request ID wrap-around fixed**: Changed `request_id` from `AtomicI64` to `AtomicU64` to avoid signed overflow issues. Removed faulty `id == 0` check that didn't catch general wrap-around.

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
- **Negation scoring fixed**: Negations ("don't use", "never") now correctly use `base_score + negation_modifier` (was: only negation_modifier used)
- **Auto-run**: `experimental.memory_auto_consolidate` config option enables automatic consolidation on session end
- **Bug fixed - access_count tracking**: `get()` now increments `access_count` when retrieving (was: never incremented)
- **Bug fixed - topic matching**: `consolidate_session()` now strips title prefixes before comparing topics for correct superseding

### Client Module (2026-05-22)
- **Health endpoint fixed**: `RemoteClient::health()` now uses `GET /health` instead of `GET /api/providers`
- **Health check error propagation**: Non-success HTTP status now returns `Err(ClientError::Unreachable)` instead of `Ok(false)`

### Client Module Review (2026-05-22)
- **Architecture doc accurate**: `architecture/client.md` correctly describes the implementation
- **Client skill line numbers updated**: `new_remote()` at line 510, `handle_remote_event()` at line 794
- **RenderFrame marked as legacy**: `RenderFrame` variant exists in `TuiMessage` but server doesn't send it
- **Skills fixed**: event-bus, server, tool skills updated for accuracy

### Permission Module (2026-05-22)
- **Architecture doc updated**: `architecture/permission.md` now accurately reflects the implementation
- **Skill synchronized**: `.opencode/skills/permission/SKILL.md` updated to match implementation
- **Dead events removed**: `PermissionRequested`, `PermissionGranted`, `PermissionDenied` were never used - removed from `AppEvent` (they were different from `PermissionPending`/`PermissionResponded`)
- **Mode bug fixed**: `docs` mode incorrectly listed `write` as both allowed (line 171) and restricted (line 178) - removed from `restricted_tools` in `BuiltinModes::docs()`
- **PermissionRegistry location noted**: Located in `src/bus/mod.rs`, not `src/permission/`

### Permission Module (2026-05-22 - Session Review)
- **PERMISSION_TYPES bug fixed**: Removed `external_directory` from `PERMISSION_TYPES` - it was incorrectly included and is not a real tool name (line 79 in mod.rs)
- **check_external_directory marked as #[allow(dead_code)]**: Function exists but is unused; marked with attribute to suppress warnings while keeping for potential future use

### IDE Module (2026-05-22)
- **Temp file flushing fixed**: `open_diff_vscode()` and `open_diff_jetbrains()` now properly flush temp files via `as_file()` + `flush()` before passing paths to IDE. Previously wrote to `tempfile` without proper handle acquisition.
- **open_diff_generic() fixed**: Now uses `std::env::split_paths()` (portable PATH parsing) and creates temp files with content instead of passing original paths directly. Matches behavior of IDE-specific handlers.
- **Skill synchronized**: `.opencode/skills/ide/SKILL.md` updated to version 1.3.0
- **Temp file handle bug fixed**: `open_diff_vscode()`, `open_diff_jetbrains()`, and `open_diff_generic()` now drop temp files before invoking IDE. Previously, file handles could still be open when passing paths to the IDE, causing failures on some platforms.
- **Error messages improved**: IDE diff failures now include exit status and stderr output (e.g., `"vscode diff failed (exit 1): error message"`)
- **Unused imports removed**: Consolidated `use` statements at module level (`std::io::Write`, `std::process::Command`, `tempfile::Builder`, `std::env::split_paths`)

### LSP Module (2026-05-22)
- **Architecture doc updated**: `architecture/lsp.md` now reflects 9-field `LspClient` struct (was showing 8), `DiagnosticEntry` type, `FileDiagnostic` struct, `should_debounce()` method
- **Skill updated**: `.opencode/skills/lsp/SKILL.md` updated to version 1.1.0 with accurate type documentation
- **PATH parsing fixed**: `download.rs` now uses `std::env::split_paths()` instead of splitting by `MAIN_SEPARATOR` (which was broken on Unix where PATH uses `:` not `/`)
- **PHP server mapping fixed**: `language.rs` now maps PHP to `php-language-server` instead of non-existent `intelephense`
- **New server definitions added**: `perl-language-server`, `powershell-editor-services`, `graphql-language-server`, `buf-language-server`, `r-languageserver`, `nimlsp`, `vls` (42 total servers, was documented as "30+")
- **Request timeout added**: `send_request()` in `client.rs` now has 30-second timeout with `LspError::RequestTimeout`
- **Hardcoded PATH fixed**: `launch.rs` now preserves user's actual PATH instead of hardcoding `/usr/local/bin:/usr/bin:/bin`
- **Stderr logging**: Server stderr is now drained and logged during LSP client initialization
- **Notification loop redundancy fixed**: `send_request()` now has cleaner notification handling with proper error logging on send failure
- **close_file race condition fixed**: Uses single write lock and properly removes file from `opened_files` after closing
- **save_file race condition fixed**: Uses single write lock instead of drop-read-then-acquire-write pattern
- **Undocumented types added to arch**: `DownloadSpec`, `ArchiveType`, `build_env_overrides()` now documented
- **Undocumented functions added**: `read_notification()`, `terminate()`, `parse_content_length()` now documented
- **`detect_language` signature fixed**: Takes `&str` not `&Path` as shown in old doc

### Plugin Module (2026-05-22)
- **Architecture doc updated**: `architecture/plugin.md` now accurately describes the implementation (was showing outdated JSON manifest, wrong HookEvent/HookResult types)
- **Skill synchronized**: `.opencode/skills/plugin/SKILL.md` updated to version 1.1.0
- **WASM path fixed**: `execute_wasm_hook()` now correctly builds `plugins/{plugin_id}/plugin.wasm` path instead of using incorrect path format
- **WASM path type fixed**: `wasm_path_str = wasm_path.to_string_lossy()` to pass `&str` to `get_or_compile()`
- **Builtin handler registry fixed**: `BUILTIN_HANDLERS` now uses `LazyLock` with proper fn pointer casting to avoid fn item type mismatches
- **API HookType serialization fixed**: `api::hooks::HookType::as_str()` now returns dot notation (e.g., `tool.execute.before`) matching actual `HookType` implementation
- **Feature flag named correctly**: Uses `plugins` feature, not `plugin`

### Plugin Module (2026-05-23)
- **dispatch_to_plugin removed**: Dead `dispatch_to_plugin` function at `src/plugin/event_bus.rs:63-69` was removed. Function only logged and never actually dispatched events to plugins. Plugin system uses `HookType::Event` via `PluginService::dispatch_event()` for event dispatch.

### Plugin Module (2026-05-22 - Session Review)
- **hooks_for() sorting removed**: `PluginRegistry::hooks_for()` no longer sorts hooks since `register()` already sorts them via `sort_hooks()`. Removed redundant sort (was sorting twice on every call).
- **Builtin plugin handlers documented**: All 4 builtins (copilot, gitlab, codex, poe) have working `auth` hook handlers that inject Bearer tokens. Previously undocumented.
- **Skill updated**: `.opencode/skills/plugin/SKILL.md` updated with builtin handler details

### Provider Module (2026-05-22)
- **Architecture doc updated**: `architecture/provider.md` now accurately reflects the implementation (was showing outdated Provider trait signature, wrong Message/ChatEvent types, incorrect ProviderRegistry methods)
- **Skill synchronized**: `.opencode/skills/provider/SKILL.md` updated with accurate types (Arc<String> usage, async trait methods)
- **Stale plan_registry.rs reference removed**: Architecture doc incorrectly referenced `wait_for_response()` bug in non-existent `plan_registry.rs`
- **Message types use Arc<String>**: All content fields use `Arc<String>` for efficiency - use `.into()` when creating
- **ChatEvent uses variants**: `TextDelta(Arc<String>)`, `ToolCall(ToolCall)`, `Finish{...}` - not the old struct variants shown in previous doc
- **FallbackProvider exponential backoff**: Retry delay is `2^i` seconds (capped at 30s) - correctly implemented
- **ProviderError::CircuitOpen**: Circuit breaker integration properly propagates open state via `CircuitError → ProviderError::CircuitOpen`

### Provider Module (2026-05-26)
- **ping() method added to Provider trait**: `async fn ping(&self) -> Result<bool, ProviderError>` at mod.rs:70-72, returns `models().await.map(|m| !m.is_empty())`
- **ModelCatalog struct corrected**: architecture doc showed wrong fields (`cache`, `ttl_secs`) - actual has `models: HashMap`, `last_fetch: Option<Instant>`, `cache_ttl: Duration`
- **SseParser struct corrected**: Added 3 undocumented fields: `is_openai: bool`, `current_tool: Option<(String, String, String)>`, `args_buffer: String`
- **create_http_client() corrected**: Actual has `.inspect_err()` and `.unwrap_or_default()` not shown in doc
- **register_builtin() documented**: Function at mod.rs:262-309 registers providers from env vars (15 total) - was undocumented
- **Additional providers complete**: Added missing `create_codegg_go()` to documentation
- **ProviderError::api_with_url() documented**: Constructor method exists at error.rs:150-160 but was undocumented

### Resilience Module (2026-05-22)
- **CircuitBreaker is_available() TOCTOU fixed**: Now uses write lock from the start instead of read-then-write, eliminating the time-of-check-time-of-use race. Implementation uses single `write().await` acquisition.
- **FallbackProvider exponential backoff**: Retry delay is `2^i` seconds (capped at 30s) - correctly implemented
- **ProviderError::CircuitOpen**: Circuit breaker integration properly propagates open state via `CircuitError → ProviderError::CircuitOpen`

### Resilience Module Review (2026-05-26)
- **Architecture doc accurate**: `architecture/resilience.md` correctly reflects implementation
- **Skill updated**: `.opencode/skills/resilience/SKILL.md` updated to v1.2.0 with FallbackProvider default parameters and exponential backoff documentation
- **No bugs found**: Core implementation correct, error conversion properly wired

### Worktree Module (2026-05-23)
- **is_git_worktree() added**: New public function to check if a directory is a Git worktree by detecting `.git` file with `gitdir:` prefix
- **is_git_file() made public**: Function changed from `pub(crate)` to `pub` for reuse by other modules
- **Duplicate is_git_worktree() removed**: `src/server/routes/workspace.rs` had a local async version - now uses `worktree::is_git_worktree()` directly
- **Duplicate find_git_root() removed**: `src/server/routes/project.rs` had a local async version - now uses `worktree::find_git_root()` directly
- **Tests added**: 3 new tests for `is_git_worktree()` and 2 for `is_git_file()` in `tests/worktree.rs`
- **Skill updated**: `.opencode/skills/worktree/SKILL.md` updated to v1.1.0 with new functions documented

### Worktree Module (2026-05-22)
- **find_git_root() bug fixed**: Now correctly detects worktrees by checking if `.git` is a file containing `gitdir:` prefix, not just a directory. Previously would fail to find git root when called from inside a worktree.
- **Architecture doc updated**: `architecture/worktree.md` now accurately reflects the implementation
- **Skill created**: `.opencode/skills/worktree/SKILL.md` created with module guidance
- **`Worktree` struct differs from doc**: `is_locked` and `is_main` are not implemented; actual fields are `path`, `branch`, `is_current`, `is_detached`
- **`remove_worktree()` signature differs**: Does not have `force` parameter as shown in old doc
- **`create_worktree()` signature differs**: Has `create_branch: bool` parameter (not shown in old doc)

### PTY Module (2026-05-22)
- **Architecture doc updated**: `architecture/pty_session.md` now accurately describes the implementation (was showing outdated `SessionManager` API with wrong field types)
- **`update_cwd` method added**: `PtyManager::update_cwd()` now exists and returns `Result<PtySession, StorageError>` (was documented but not implemented)
- **Skill created**: `.opencode/skills/pty/SKILL.md` created with module guidance

### Security Module (2026-05-22)
- **revalidate_dns() IPv6 fix**: Fixed to handle IPv4-mapped IPv6 addresses. When DNS returns IPv6 address that maps to an already-validated IPv4, it now correctly continues instead of false positive DNS rebinding detection. The check uses `ipv6_segments_to_ipv4()` to detect mapped addresses.
- **Architecture doc inaccurate**: `architecture/security.md` showed `is_internal_ip(&str)` and `validate_url_host(&Url)` but actual signatures are `is_internal_ip(&IpAddr)` and `validate_url_host(&str)`. SSRFChecker and LandlockSandbox structs don't exist - actual structs are standalone functions and `SandboxConfig`.
- **Skill synchronized**: `.opencode/skills/security/SKILL.md` and `.opencode/skills/sandbox/SKILL.md` updated with accurate types and function signatures.

### Security Module (2026-05-22 - fixes)
- **validate_path_safety() symlink check**: Added symlink check before canonicalization to prevent symlink traversal attacks. Uses `path.symlink_metadata()` to detect if the path itself is a symlink.
- **validate_path_safety() tests added**: Added `test_validate_path_safety` and `test_validate_path_safety_with_symlink` unit tests to verify path validation and symlink rejection.

### Security Module (2026-05-26)
- **validate_url_host() returns lowercase**: Fixed to return host normalized to lowercase for case-insensitive comparison consistency.

### Server Module (2026-05-22)
- **WsRateLimiter shared**: `WsRateLimiter` in `ServerState` is now shared across all WebSocket connections (was created per-connection, causing inefficient rate limiting)
- **SSE GlobalEventBus fixed**: SSE handler at `/api/event` now subscribes directly to `crate::bus::global::GlobalEventBus::subscribe()` instead of using isolated State parameter
- **Health route simplified**: `routes/health.rs` simplified to just `health_check()` function (unused `Router` builder removed)
- **Auth deduplication**: `validate_ws_auth()` function now shared between `handle_ws` and `handle_tui` (was duplicated inline code)
- **rpc.rs status corrected**: `rpc.rs` is NOT unused - it defines `JsonRpcMessage` struct used by `ws.rs` for JSON-RPC responses

### Server Module (2026-05-24)
- **RpcRequest/RpcResponse/RpcError added**: These types were missing from `rpc.rs` but referenced in `ws.rs`. Added them to fix compilation.
- **health module exported**: `routes/mod.rs` now exports `health` module so `http.rs` can import `health_check`
- **GlobalEventBus removed from ServerState**: Since `GlobalEventBus` is a static singleton (not Clone), it was removed from `ServerState`. SSE and WebSocket handlers use `GlobalEventBus::subscribe()` directly.

### Session Module (2026-05-22)
- **StorageError variants expanded**: Added `Import` and `Export` error variants to `StorageError` enum (were using `Database` variant)
- **CheckpointStore now exported**: `CheckpointStore` is now re-exported from `session::mod.rs` for use by other modules
- **generate_slug now exported**: `generate_slug` helper function is now `pub` and re-exported from `session::mod.rs`
- **Skill updated**: `.opencode/skills/session/SKILL.md` updated to version 1.1.0 with complete API documentation

### Session Module (2026-05-26)
- **Architecture doc updated**: `architecture/session.md` now accurately reflects implementation (includes `WorkingFile`, `ToolStatus`, `SessionStatus`, `SessionState`, undocumented methods)
- **Skill updated**: `.opencode/skills/session/SKILL.md` updated to version 1.2.0 with all stores, helpers, and re-exports documented
- **`has_unfinished` renamed**: `CheckpointStore::has_unfinished()` renamed to `has_checkpoint()` for clarity
- **Event publishing clarified**: Only `SessionCreated` and `MessageAdded` events exist (not the 5 events previously listed in arch doc)
- **Undocumented types added**: `WorkingFile`, `ToolStatus`, `SessionStatus`, `SessionState`, `compute_checksum`, `create_working_file`, `verify_file`
- **Undocumented tables**: `session_share`, `task`, `snapshot`, `cached_models`, `migration_version` now documented

### Snapshot Module (2026-05-23)
- **Path traversal fix**: `restore_to_path()` now validates restored paths don't escape target directory via `canonicalize()` check (e.g., `../../etc/passwd` blocked)
- **Skill updated**: SKILL.md v1.1.0 documents security improvement

### Snapshot Module (2026-05-22)
- **Architecture doc outdated**: `architecture/snapshot.md` was significantly out of date - updated to reflect actual implementation
- **Skill synchronized**: `.opencode/skills/snapshot/SKILL.md` updated with correct API signatures
- **`Snapshot` struct field `data`**: Stores files as JSON string, not as direct `HashMap` field
- **`SnapshotView` has `files`**: The `files: HashMap<String, FileSnapshot>` field is on `SnapshotView`, not `Snapshot`
- **`SnapshotManager::new()` requires `SqlitePool`**: Constructor signature is `new(pool, project_root)`, not just `new(project_root)`
- **restore() takes SnapshotView**: `restore(&self, snapshot: &SnapshotView)` not `restore(&self, id: &str)`
- **capture_incremental path validation**: Added validation that paths are within `project_root` before accepting
- **Restore error messages improved**: Now include file path on failure (e.g., "failed to write /path/file: ...")
- **Snapshot table in session schema**: Database table defined in `src/session/schema.rs` migration v13, not in snapshot module

### Storage Module (2026-05-22)
- **Architecture doc updated**: `architecture/storage.md` now accurately reflects the implementation (was showing incorrect `init()` return type, wrong `cache_size`, missing pragmas)
- **Skill created**: `.opencode/skills/storage/SKILL.md` created with module guidance
- **Pragmas batched**: Individual PRAGMA queries consolidated into single query for efficiency
- **Database struct is wrapper**: `Database` is simple wrapper around `SqlitePool` - most code uses `init()` directly
- **Migrations in session module**: Actual migration logic lives in `src/session/schema.rs`, not storage module

### Storage Module (2026-05-26)
- **health_check() added**: `Database::health_check()` method to verify database connectivity via `SELECT 1`
- **close() added**: `Database::close()` method for graceful connection pool shutdown using async pool close
- **acquire_timeout configured**: `SqlitePoolOptions::acquire_timeout(Duration::from_secs(30))` for connection acquisition timeout
- **Sync fs bug fixed**: `init()` now uses `tokio::fs::metadata()` instead of blocking `std::fs::metadata()` for read-only check
- **Skill updated**: `.opencode/skills/storage/SKILL.md` updated to version 1.1.0 with new methods documented

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

### Command Module - Pending Fix
- **Panic on error** (2026-05-25): `find_command_files()` at mod.rs:21-24 panics with "expected" on load failure instead of graceful handling. Fix: use `filter_map(|r| r.ok())` pattern. See `plans/plan.md` Wave 1.

### Key Lessons from Review Sessions
- **Always verify documentation claims against actual code**. Many "bugs" in review files turned out to be correctly implemented after direct inspection. The act of reviewing often reveals assumptions that were wrong.
- When encountering a claim like "Bug X exists in file Y", first read the actual code at that location to confirm before marking it as a bug.

### TTS Module (2026-05-26)
- **Error handling improved**: `speak()` now returns `Err(AppError::Io(...))` when `say` command fails instead of silently ignoring failures. Callers handle errors appropriately.
- **Skill synchronized**: `.opencode/skills/tts/SKILL.md` updated with accurate `toggle_tts` and `stop_tts` implementation
- **init() fixed**: `Tts::init()` now properly handles `TtsProvider` via match instead of ignoring the parameter

### Util Module (2026-05-22)
- **Architecture doc updated**: `architecture/util.md` now accurately reflects the implementation (was showing incorrect function signatures and types)
- **Skill created**: `.opencode/skills/util/SKILL.md` created with module guidance
- **clipboard.rs naming differs**: Actual functions are `copy_to_clipboard()` / `read_from_clipboard()` (not `copy()` / `paste()` as shown in doc)
- **fuzzy.rs API differs**: `fuzzy_match()` takes `&[String]` candidates returning `Vec<(String, usize)>`, not `Option<(usize, usize)>`. `fuzzy_score()` returns `usize`, not a `FuzzyScore` struct
- **truncate.rs differs**: Actual functions are `truncate_lines()` / `truncate_bytes()` (not `truncate()` / `truncate_with_ellipsis()` as shown in doc)
- **stat_core.rs misnamed**: Contains metrics infrastructure (Counter, Gauge, Histogram), not file statistics as name suggests
- **Feature gate noted**: Clipboard requires `arboard` feature flag

### Util Module (2026-05-26)
- **Skill updated**: `.opencode/skills/util/SKILL.md` updated to v1.1.0 with accurate integration points
- **Integration verified**: 5 usage locations confirmed (fuzzy_score: 3, clipboard: 2)
- **Tests passing**: 24 unit tests across fuzzy (11) and truncate (13) modules
- **No bugs found**: Implementation is correct; stat_core metrics not actively used but available for future observability needs

### Upgrade Module (2026-05-22)
- **Architecture doc updated**: `architecture/upgrade.md` now accurately reflects the implementation (was showing outdated `ReleaseInfo` struct and `github_api::get_latest_release` function)
- **Skill created**: `.opencode/skills/upgrade/SKILL.md` created with module guidance
- **Hardcoded PATH fixed**: `upgrade()` now uses user's actual PATH via `std::env::var_os("PATH")` instead of hardcoded `/usr/local/bin:/usr/bin:/bin`
- **Architecture missing `VersionInfo`**: Actual struct is `VersionInfo` with `current`, `latest`, `needs_update` fields (not `ReleaseInfo` with `version`, `tag_name`, `download_url`, `release_notes`)
- **Upgrade configuration not implemented**: Architecture showed `[upgrade]` config section but no such configuration is loaded in the module

### Upgrade Module (2026-05-23)
- **Error message inconsistency fixed**: `check_for_updates()` error message now consistently uses `format!("request failed: {e}")` pattern (was using `e.to_string()` which is equivalent but less explicit)
- **`upgrade()` not called by CLI**: The `cmd_upgrade()` in `main.rs` only checks and reports updates but does **not** call `upgrade::upgrade()` to perform the actual upgrade. The `upgrade()` function exists but is unused.

### TUI Module (2026-05-22)
- **Architecture doc updated**: `architecture/tui.md` now accurately reflects the implementation (was showing outdated routes like Chat/Sessions/Settings, wrong App struct, missing FocusManager/Component)
- **Routes accurate**: Actual routes are `Home` and `Session(String)` - not the `Chat`, `Sessions`, `Settings`, `Skills`, `Permissions` shown in old doc
- **Dialog variants fixed**: Doc now shows all 21 Dialog variants including Context, Cost, Usage, Goto, Plan, Diff, Confirm
- **Dead code removed**: `render_dialog()` no longer has unreachable `{}` block after FocusManager render
- **State inconsistency handling added**: `on_key()` now properly handles case when `dialog.is_open()` but `focus_manager.is_empty()` - logs error and resets state instead of panicking via debug_assert
- **Skill updated**: `.opencode/skills/tui/SKILL.md` updated with `push_dialog()` method for temporary dialogs and defensive state consistency check

### TUI Module Review (2026-05-23)
- **Component trait location fixed**: Architecture doc showed `component/mod.rs` but actual is `component.rs` (single file with submodules in `component/` subdirectory)
- **Additional components documented**: `help_overlay.rs`, `tool_output.rs` added to component list (were missing from docs)
- **Dialog files verified**: All 21 dialog implementations present in `dialogs/` directory
- **skill synchronized**: `.opencode/skills/tui/SKILL.md` version bumped to 3.0.0 with accurate Component trait documentation

### Command Module (2026-05-22)
- **Async file loading**: `find_command_files()` and `load_command_from_file()` now use `tokio::fs` for async I/O (were using blocking `std::fs`)
- **`subtask` field deprecated**: Added `#[deprecated]` attribute to `subtask` field since it's not yet implemented
- **Command count**: 41 built-in commands (updated from 36)
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
├── agent-loop/          # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
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
├── hooks/              # Hooks system for agent lifecycle
├── ide/                # IDE integration (VS Code, JetBrains)
├── lsp/                # LSP client, diagnostics, code operations
├── mcp/                # MCP connection manager
├── memory/             # Memory system, consolidation, patterns
├── mode/               # Mode system (Review/Debug/Docs)
├── model-dialog/       # Model selection/config dialog
├── notifications/      # Desktop notifications
├── permission/         # PermissionChecker, DoomLoop, PermissionRegistry
├── plugin/             # WASM plugin system
├── provider/           # LLM provider implementations
├── pty/                # Shell session metadata
├── question-response/  # Question/permission response shapes
├── resilience/          # Circuit breaker, FallbackProvider
├── router/             # Model auto-routing
├── sandbox/            # Landlock filesystem sandboxing
├── security/           # SSRF, symlink protection, Landlock
├── server/             # HTTP/WebSocket server for remote TUI
├── session/            # Session storage, database schema
├── skills/             # Skill loading, activation, SkillIndex
├── snapshot/           # File state capture and restore
├── storage/            # SQLite initialization, pragmas
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
4. Use frontmatter: `name`, `description`, `version`, `tags`

### File Naming Convention

- `AGENTS.md` - Root index file (this file)
- `.opencode/skills/<name>/SKILL.md` - Module-specific skill guides
- `architecture/<module>.md` - Architecture documentation per module

## Archived Documents

| Document | Status | Notes |
|----------|--------|-------|
| `plans/plan.md` (Phase 1) | Archived (2026-05-24) | All Phase 1 review items completed |
| `plans/plan.md` (Phase 2) | Active (2026-05-25) | Remaining documentation corrections |

## Quick Reference

| Topic | Location |
|-------|----------|
| PTY (shell session metadata) | `.opencode/skills/pty/SKILL.md` |
| Agent (AgentLoop, compaction, router, team) | `.opencode/skills/agent-loop/SKILL.md` |
| Event Bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | `.opencode/skills/event-bus/SKILL.md` |
| TUI (keyboard shortcuts, FocusManager, Component trait) | `.skills/tui/SKILL.md` |
| Core (CoreClient facade, transports, protocol envelopes) | `.skills/core/SKILL.md` |
| Security (SSRF, symlinks, Landlock) | `.opencode/skills/security/SKILL.md` |
| WASM plugins | `.opencode/skills/plugin/SKILL.md` |
| MCP (Model Context Protocol) | `.opencode/skills/mcp/SKILL.md` |
| Provider (LLM providers, Arc<String> types, FallbackProvider) | `.opencode/skills/provider/SKILL.md` |
| Crypto (API key encryption, Argon2id key derivation) | [architecture/crypto.md](architecture/crypto.md) |
| Error (AppError, ProviderError, ToolError, is_retryable, CircuitOpen) | `.opencode/skills/error/SKILL.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `.opencode/skills/resilience/SKILL.md` |
| Permission (mode system, PermissionChecker, DoomLoop, PermissionRegistry) | `.opencode/skills/permission/SKILL.md` |
| LSP (Language Server Protocol, diagnostics, code operations) | `.opencode/skills/lsp/SKILL.md` (v1.1.0, 40 servers) |
| Tool (path validation, async command, ToolExecutor, ToolCatalog) | `.opencode/skills/tool/SKILL.md` |
| Exec mode | `.opencode/skills/exec/SKILL.md` |
| Hooks system | `.opencode/skills/hooks/SKILL.md` |
| Client (remote TUI, WebSocket) | `.skills/client/SKILL.md` |
| Server (HTTP, WebSocket, REST API, SSE) | `.skills/server/SKILL.md` |
| Snapshot (file state capture and restore) | `.opencode/skills/snapshot/SKILL.md` |
| Skills (skill system overview) | `.skills/skills/SKILL.md` |
| Command (slash commands, templates, execution) | `.opencode/skills/command/SKILL.md` |
| IDE (VS Code, JetBrains detection, diff viewing) | `.opencode/skills/ide/SKILL.md` |
| Config (loading, validation, encryption, watching) | `.opencode/skills/config/SKILL.md` |
| Memory (session-to-session learning, consolidation) | `.opencode/skills/memory/SKILL.md` |
| Session (storage, SQLite, checkpoint, import/export) | `.opencode/skills/session/SKILL.md` (v1.2.0) |
| Storage (SQLite initialization, pragmas, pooling) | `.opencode/skills/storage/SKILL.md` (v1.1.0) |
| Upgrade (GitHub releases, self-upgrade) | `.opencode/skills/upgrade/SKILL.md` (v1.1.0) |
| Worktree (git worktrees, find_git_root) | `.opencode/skills/worktree/SKILL.md` |
| Subagent (SubAgentPool, SubAgentSpawner, worker) | `.opencode/skills/subagent/SKILL.md` |
| Compaction (context compaction strategies) | `.opencode/skills/compaction/SKILL.md` |
| Router (model auto-routing) | `.opencode/skills/router/SKILL.md` |
| Util (clipboard, fuzzy matching, truncation, metrics) | `.opencode/skills/util/SKILL.md` (v1.1.0) |
