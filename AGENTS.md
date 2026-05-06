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
| `crypto/` | AES-256-GCM encryption for API keys and secrets |
| `error/` | Centralized `AppError` enum using `thiserror` |
| `exec/` | Non-interactive exec mode for CI/CD with JSON I/O |
| `hooks/` | Hooks system for agent loop lifecycle events and plugin interaction |
| `ide/` | IDE integration (VS Code IPC, JetBrains remote mode) |
| `lsp/` | Language Server Protocol support (diagnostics, code operations) |
| `mcp/` | Model Context Protocol client (local, remote, auth) with auto-reconnect |
| `memory/` | Persistent memory system for session learning and namespace management |
| `permission/` | Access control, path restrictions, DoomLoop detection, mode system |
| `plugin/` | WASM plugin system with hooks and TUI extensions |
| `provider/` | LLM provider implementations (Anthropic, OpenAI, Google, etc.) |
| `pty/` | PTY (pseudo-terminal) support for terminal tools |
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

### Data Persistence Issues
- **Memory module doesn't persist**: `add()`/`delete()` in `src/memory/mod.rs` don't actually save to disk
- **Snapshot has no persistence**: In-memory only, lost on restart. Needs SQLite persistence.
- **Session `share_session` race**: UPSERT + set_share_url not atomic in `store.rs:1290-1313`

### Security Issues
- **Auth middleware is broken**: Wrong signature and undefined variables in `src/server/middleware/auth.rs`
- **Symlink bypass in tools**: `canonicalize_path()` in `util.rs` doesn't check intermediate symlinks. Use `check_path_for_symlinks()` before canonicalization.
- **IDE temp file race**: Predictable filenames in `src/ide/ide.rs`, needs `mkstemp` or `tempfile` crate

### Concurrency Issues
- **LSP request ID race**: Wrap-around issue in `client.rs:451-457`
- **Config watcher race**: Closure captures `tx` before stored in `self.watcher`
- **Bus module memory leak**: Dead letter channels if sender dropped without response in `src/bus/mod.rs`
- **Storage race condition**: `std::fs::File::create` vs SQLite atomic creation in `src/storage/mod.rs`

### Code Quality Issues
- **Commands.rs has duplicate code**: `handle_slash_command` appears twice (lines ~62-288 and ~323-536)
- **PlanRegistry unused**: `wait_for_response()` has send-then-discard bug in `src/agent/plan_registry.rs`
- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`

### Verified Correct Items (not bugs)
- **WebSocket rate limiter**: If `REDIS_URL` is set → use Redis; otherwise → use in-memory. Proper fallback behavior.
- **`process_request()` is implemented**: It publishes `SubagentStarted`/`SubagentCompleted` events and returns `SubAgentResult::success()`.

### Implementation Patterns
- **DoomLoop doc mismatch**: Comment says "consecutive" but implementation uses window-based counting
- **MCP reconnect exists**: `remote.rs` has `reconnect()` at line 470 - needs to be wired up to auto-retry
- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`

## Documentation Structure

Agent guidance is **modularized** to reduce context pollution. Each module has its own `AGENTS.override.md` file in `.codegg/docs/<module>/`. The root `AGENTS.md` serves as an index only.

### Directory Structure

```
.codegg/docs/
├── AGENTS.md                     # Root index (this file)
├── agent/
│   └── AGENTS.override.md        # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── bus/
│   └── AGENTS.override.md        # Event bus guidance
├── crypto/
│   └── AGENTS.override.md        # API key encryption
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
├── security/
│   └── AGENTS.override.md        # SSRF, symlink protection, Landlock
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
| TUI (keyboard shortcuts) | `tui/AGENTS.override.md` |
| Security (SSRF, symlinks, Landlock) | `security/AGENTS.override.md` |
| WASM plugins | `plugin/AGENTS.override.md` |
| MCP connection manager | `mcp/AGENTS.override.md` |
| Provider (token estimation) | `provider/AGENTS.override.md` |
| Crypto (API key encryption) | `crypto/AGENTS.override.md` |
| Permission (mode system) | `permission/AGENTS.override.md` |
| Tool (path validation, async command) | `tool/AGENTS.override.md` |
| Exec mode | `exec/AGENTS.override.md` |
| Hooks system | `hooks/AGENTS.override.md` |
| Testing (E2E, unit, integration) | `meta/AGENTS.override.md` |
| Updates, roadmap, code quality | `meta/AGENTS.override.md` |