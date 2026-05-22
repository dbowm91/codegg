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
| `error/` | Centralized `AppError` enum, `ProviderError::is_retryable()`, error conversions |
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

### Verified Correct Items (not bugs)
- **WebSocket rate limiter**: If `REDIS_URL` is set → use Redis; otherwise → use in-memory. Proper fallback behavior.
- **`process_request()` is implemented**: It publishes `SubagentStarted`/`SubagentCompleted` events and returns `SubAgentResult::success()`.
- **`SubAgentPool` bounded concurrency**: Properly uses semaphore with default of 5
- **Tool definition caching**: Properly versioned cache key (uses mcp_tool_count as proxy - see known limitation)
- **DoomLoop detection**: Implementation correctly uses window-based counting (not consecutive), and docstring accurately describes this
- **`decrypt_provider_keys()` is called in `Config::load()`**: API keys encrypted via `save()` are now automatically decrypted on load (fixed 2026-05-21)
- **ProviderConfig merge is field-by-field**: When merging configs with same provider, fields are merged individually (fixed 2026-05-21)
- **`medium_model` is validated**: Validates `provider/model` format like `model` and `small_model` (fixed 2026-05-21)
- **`ProviderError::is_retryable()` implemented**: Centralizes retry logic for provider errors (added 2026-05-22)
- **CircuitError → ProviderError::CircuitOpen conversion**: `From<CircuitError>` impl enables circuit breaker error propagation (added 2026-05-22)
- **`FallbackProvider` uses `CircuitOpen`**: Circuit breaker errors now create `ProviderError::CircuitOpen` instead of generic `ProviderError::api()` (fixed 2026-05-22)
- **SSE GlobalEventBus fixed**: SSE handler at `/api/event` now subscribes directly to `crate::bus::global::GlobalEventBus::subscribe()` instead of using isolated State parameter (fixed 2026-05-22)
- **Exec mode session_id**: `session_id` parameter in `ExecMode::new()` is now properly used (was ignored before, now falls back to UUID if None) (fixed 2026-05-22)
- **Exec mode error classification**: `CircuitOpen`, `Api`, and `Stream` errors now properly classified with distinct error codes (fixed 2026-05-22)
- **Exec mode config errors**: Config loading errors now properly returned as `CONFIG_ERROR` instead of silently using defaults (fixed 2026-05-22)
- **Exec mode question channel**: `setup_question_channel()` is now called in exec mode for proper question tool handling (fixed 2026-05-22)

### Implementation Patterns
- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.
- **MCP reconnect wired up**: Heartbeat failures now trigger reconnect via `reconnect_needed` Notify mechanism
- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`
- **Component trait**: All dialogs implement `Component` trait with `handle_key`, `update`, `render` methods
- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event
- **ResyncRequired serialization**: Server uses `TuiMessage::ResyncRequired` variant directly (not raw JSON)
- **Client timeouts**: Health check has 10s timeout, WebSocket connection has 30s timeout

### Code Quality Issues (Lower Priority)
- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`
- **Tool definition cache staleness**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

## Documentation Structure

Agent guidance is **modularized** to reduce context pollution. Each module has its own `AGENTS.override.md` file in `.opencode/docs/<module>/`. The root `AGENTS.md` serves as an index only.

### Directory Structure

```
.opencode/docs/
├── AGENTS.md                     # Root index (this file)
├── agent/
│   └── AGENTS.override.md        # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── bus/
│   └── AGENTS.override.md        # Event bus guidance
├── command/
│   └── AGENTS.override.md        # Slash commands, templates, execution
├── config/
│   └── AGENTS.override.md        # Config loading, validation, encryption, file watching
├── crypto/
│   └── AGENTS.override.md        # API key encryption
├── error/
│   └── AGENTS.override.md        # AppError, ProviderError, is_retryable, CircuitOpen
├── exec/
│   └── AGENTS.override.md        # Exec mode
├── hooks/
│   └── AGENTS.override.md        # Hooks system
├── mcp/
│   └── AGENTS.override.md        # MCP connection manager
├── permission/
│   └── AGENTS.override.md        # Mode system
├── plugin/
│   └── AGENTS.override.md        # WASM sandboxing, fuel tracking
├── provider/
│   └── AGENTS.override.md        # Provider patterns, token estimation
├── resilience/
│   └── AGENTS.override.md        # Circuit breaker, FallbackProvider
├── security/
│   └── AGENTS.override.md        # SSRF, symlink protection, Landlock
├── server/
│   └── AGENTS.override.md        # WebSocket, TuiMessage, ResyncRequired
├── shell/
│   └── AGENTS.override.md        # Shell session management
├── skills/
│   └── AGENTS.override.md        # Skills system overview
├── snapshot/
│   └── AGENTS.override.md        # Snapshot capture and restore
├── tool/
│   └── AGENTS.override.md        # Tool path validation, async command pattern
├── tui/
│   └── AGENTS.override.md        # Keyboard shortcuts
└── meta/
    └── AGENTS.override.md        # Updates, roadmap, code quality
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
| WASM plugins | `plugin/AGENTS.override.md` |
| MCP connection manager | `mcp/AGENTS.override.md` |
| Provider (token estimation) | `provider/AGENTS.override.md` |
| Crypto (API key encryption) | `crypto/AGENTS.override.md` |
| Error (AppError, ProviderError, is_retryable, CircuitOpen) | `error/AGENTS.override.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `resilience/AGENTS.override.md` |
| Permission (mode system) | `permission/AGENTS.override.md` |
| Tool (path validation, async command) | `tool/AGENTS.override.md` |
| Exec mode | `exec/AGENTS.override.md` |
| Hooks system | `hooks/AGENTS.override.md` |
| Client (remote TUI, WebSocket) | `client/SKILL.md` |
| Server (WebSocket, TuiMessage serialization) | `server/AGENTS.override.md` |
| Snapshot (file state capture and restore) | `snapshot/AGENTS.override.md` |
| Skills (skill system overview) | `skills/AGENTS.override.md` |
| Command (slash commands, templates, execution) | `command/AGENTS.override.md` |
| Testing (E2E, unit, integration) | `meta/AGENTS.override.md` |
| Config (loading, validation, encryption, watching) | `config/SKILL.md` |
| Updates, roadmap, code quality | `meta/AGENTS.override.md` |