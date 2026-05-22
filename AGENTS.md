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
| `shell/` | Shell session management (in-memory session metadata, no actual PTY) |
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
- **GlobalEventBus::publish() improved**: Now returns subscriber count on success, uses `trace` level for normal events (was `warn` for all cases). Channel closed errors properly distinguished.
- **Event flow documentation accurate**: Registration-before-publish pattern correctly documented in both `architecture/event-bus.md` and `.opencode/skills/event-bus/SKILL.md`

### Crypto Module (2026-05-22)
- **Crypto module updated**: architecture/crypto.md now accurately describes the implementation (Argon2id key derivation, v2 format with `v2:` prefix, legacy HMAC-SHA256 support)
- **Skill synchronized**: `.opencode/skills/crypto/SKILL.md` updated to match implementation

### Error Module (2026-05-22)
- **Architecture updated**: architecture/error.md now accurately describes the implementation (all error variants, is_retryable methods, HTTP status mapping)
- **Skill created**: `.opencode/skills/error/SKILL.md` created with comprehensive error handling guidance
- **Exec mode error classification expanded**: All AppError variants now have distinct error codes (was using catch-all EXECUTION_ERROR for many cases)
- **ProviderError::NotFound handled**: Added classification for provider not found errors
- **ToolError variants handled**: NotFound, Timeout, Permission, Disabled now have distinct codes

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

## Documentation Structure

### Directory Structure

```
.opencode/skills/
├── agent-loop/SKILL.md           # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── client/SKILL.md               # Remote TUI client, WebSocket
├── command/SKILL.md             # Slash commands, templates, execution
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
├── permission/SKILL.md          # Mode system
├── plugin/SKILL.md             # WASM sandboxing, fuel tracking
├── provider/SKILL.md            # Provider patterns, token estimation
├── resilience/SKILL.md           # Circuit breaker, FallbackProvider
├── security/SKILL.md            # SSRF, symlink protection, Landlock
├── session/SKILL.md             # Session storage, database schema
├── snapshot/SKILL.md            # Snapshot capture and restore
├── tool/SKILL.md                 # Tool path validation, async command
└── tui/SKILL.md                  # Terminal UI, keyboard shortcuts
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
| Agent (TuiCommand, TuiMsg, compaction, router, team) | `agent/AGENTS.override.md` |
| Event Bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | `.opencode/skills/event-bus/SKILL.md` |
| TUI (keyboard shortcuts) | `tui/AGENTS.override.md` |
| Security (SSRF, symlinks, Landlock) | `security/AGENTS.override.md` |
| WASM plugins | `.opencode/skills/plugin/SKILL.md` |
| MCP connection manager | `mcp/AGENTS.override.md` |
| Provider (token estimation) | `provider/AGENTS.override.md` |
| Crypto (API key encryption, Argon2id key derivation) | [architecture/crypto.md](architecture/crypto.md) |
| Error (AppError, ProviderError, ToolError, is_retryable, CircuitOpen) | `.opencode/skills/error/SKILL.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `resilience/AGENTS.override.md` |
| Permission (mode system) | `permission/AGENTS.override.md` |
| LSP (Language Server Protocol, diagnostics, code operations) | `.opencode/skills/lsp/SKILL.md` |
| Tool (path validation, async command) | `tool/AGENTS.override.md` |
| Exec mode | `.opencode/skills/exec/SKILL.md` |
| Hooks system | `.opencode/skills/hooks/SKILL.md` |
| Client (remote TUI, WebSocket) | `client/SKILL.md` |
| Server (WebSocket, TuiMessage serialization) | `server/AGENTS.override.md` |
| Snapshot (file state capture and restore) | `snapshot/AGENTS.override.md` |
| Skills (skill system overview) | `skills/AGENTS.override.md` |
| Command (slash commands, templates, execution) | `.opencode/skills/command/SKILL.md` |
| IDE (VS Code, JetBrains detection, diff viewing) | `.opencode/skills/ide/SKILL.md` |
| Testing (E2E, unit, integration) | `meta/AGENTS.override.md` |
| Config (loading, validation, encryption, watching) | `.opencode/skills/config/SKILL.md` |
| Memory (session-to-session learning, consolidation) | `.opencode/skills/memory/SKILL.md` |
| Updates, roadmap, code quality | `meta/AGENTS.override.md` |