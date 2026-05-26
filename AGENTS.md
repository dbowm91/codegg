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
- `plans/plan.md`: Consolidated implementation plan (31 module reviews) with wave-based parallelization strategy

## Critical Implementation Notes

These items are important for future agents to know when working with the codebase:

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: `PermissionRegistry` and `QuestionRegistry` do NOT store `session_id` in their keys. Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. This means `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id without code changes.

- **MCP reconnect wired up**: Heartbeat failures now trigger reconnect via `reconnect_needed` Notify mechanism

- **MCP DNS re-validation**: `RemoteClient::initialize()` re-validates DNS on each call (connect/reconnect), keeping `validated_ips` current

- **MCP ensure_connected()**: Clones all fields before `tokio::spawn` to avoid borrow-after-spawn issues

- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`

- **Component trait**: All dialogs implement `Component` trait with `handle_key`, `update`, `render` methods

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event

- **ResyncRequired serialization**: Server uses `TuiMessage::ResyncRequired` variant directly (not raw JSON)

- **Client timeouts**: Health check has 10s timeout, WebSocket connection has 30s timeout

- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`

- **Subprocess PATH**: All tools use `std::env::var_os("PATH")` instead of hardcoded paths for proper Homebrew/cargo/pyenv tool discovery

- **Plugin fuel tracking**: `fuel_reserved` set at `loader.rs:270` is returned via `module_cache::CACHE.return_fuel()` on ALL exits

- **handle_remote_event location**: `src/tui/app/mod.rs:794` - not in client module

- **IdeServer async I/O**: `run_stdio()` uses `tokio::io::stdin()/stdout()` with async I/O, not blocking `std::io`

### Known Issues (Lower Priority)

- **SSE support not fully integrated**: `connect_sse()` and `connect_sse_stream()` exist but are not automatically called during remote connection setup. SSE events are collected but not yet processed by the agent.

- **Tool definition cache staleness**: Using `mcp_tool_count` as proxy means if MCP tool identities change without count changing, cache may be stale. MCP service would need to expose a version/hash for more precise invalidation.

- **Plugin global fuel budget unused**: `PLUGIN_FUEL_BUDGET` and `check_and_reset_fuel_budget()` at `loader.rs:15,24-41` are never called - dead code that could be removed or integrated.

### Key Lessons from Review Sessions

1. **Always verify documentation claims against actual code** - Many "bugs" in review files turned out to be correctly implemented after direct inspection.

2. **Documentation can become stale** - Struct fields get added/removed; always compare architecture docs against actual source code.

3. **Counts should be verified** - Component/dialog counts (TUI), server counts (LSP), command counts can drift from reality. When fixing documentation, count from actual source files, not from other documentation.

4. **Line numbers in docs are fragile** - References like `watcher.rs:157-158` should be verified as they can be off by several lines. Use code search to find exact locations.

5. **Pre-verification before editing** - When a plan or review file claims "X is wrong in architecture doc", first check if it's been fixed since the review was written. Many "corrections" in old plans were already addressed.

6. **Use subagents for batch review work** - Process 4-5 plan files per subagent (2000 line context limit), consolidate results, then consolidate into final plan.

### Verified Codebase Facts

These items were verified during a 2026-05-26 review session:

| Item | Value | Location |
|------|-------|----------|
| Tool count | 26 | `src/tool/mod.rs:89-119` |
| LSP server count | 39 | `src/lsp/server.rs:27-385` |
| PermissionResponse | `{level: PermissionLevel, persist: bool}` | `src/permission/mod.rs:1142-1145` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` | `src/core/mod.rs:22-28` |
| ToolExecutor usage | bash, read, glob tools | `src/tool/executor.rs:72,92,112` |
| Plugin fuel logic | CORRECT - returns early when exhausted | `src/plugin/loader.rs:262-266` |
| InlineScript | Deprecated, non-functional | `src/hooks/mod.rs:180-184` |
| CommandRegistry location | Line 72 | `src/tui/command.rs:72` |
| register_panic_cleanup | Private function for temp file cleanup | `src/ide/mod.rs:65-78` |
| ProviderError::Auth | is_retryable = true | `src/error.rs:169` |
| Memory frequency_bonus | `(count - 1) * 2.0` | `src/memory/patterns.rs:232` |
| Session events published | SessionCreated, MessageAdded | `src/bus/events.rs:7,21` |

### Security Notes

- **Auth middleware allows requests without token when none configured**: At `src/server/middleware/auth.rs:37-39`, when `expected_token` is `None`, requests are allowed through. This may be intentional for development but should be reviewed for production.

- **Plugin dead code**: `check_and_reset_fuel_budget()`, `PLUGIN_FUEL_BUDGET`, and `PLUGIN_FUEL_LAST_RESET` at `loader.rs:15,24-41` are never called - candidates for removal.

### CoreRequest Handler Attention Points

- `CoreRequest` enum in `src/protocol/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - verify if TUI actually sends these before implementing meaningful responses.

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
4. Use frontmatter: `name`, `description`, `version`, `process`

### File Naming Convention

- `AGENTS.md` - Root index file (this file)
- `.opencode/skills/<name>/SKILL.md` - Module-specific skill guides
- `architecture/<module>.md` - Architecture documentation per module

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
| LSP (Language Server Protocol, diagnostics, code operations) | `.opencode/skills/lsp/SKILL.md` |
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
| Session (storage, SQLite, checkpoint, import/export) | `.opencode/skills/session/SKILL.md` |
| Storage (SQLite initialization, pragmas, pooling) | `.opencode/skills/storage/SKILL.md` |
| Upgrade (GitHub releases, self-upgrade) | `.opencode/skills/upgrade/SKILL.md` |
| Worktree (git worktrees, find_git_root) | `.opencode/skills/worktree/SKILL.md` |
| Subagent (SubAgentPool, SubAgentSpawner, worker) | `.opencode/skills/subagent/SKILL.md` |
| Compaction (context compaction strategies) | `.opencode/skills/compaction/SKILL.md` |
| Router (model auto-routing) | `.opencode/skills/router/SKILL.md` |
| Util (clipboard, fuzzy matching, truncation, metrics) | `.opencode/skills/util/SKILL.md` |
