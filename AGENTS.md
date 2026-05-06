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
├── shell/
│   └── AGENTS.override.md        # Shell session management
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