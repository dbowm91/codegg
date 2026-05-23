# Codegg Architecture Overview

Codegg is a **Rust rewrite of an AI coding agent** built for performance and efficiency. It uses modern Rust async primitives and is designed for high-performance tool-augmented LLM interactions with a full terminal UI, MCP integration, LSP support, and extensibility through WASM plugins.

## Technology Stack

| Technology | Purpose |
|------------|---------|
| **Tokio** | Async runtime for concurrent operations |
| **SQLx** | SQLite database with compile-time query checking |
| **Ratatui** | Terminal UI framework |
| **Axum** | HTTP server (feature-gated via `server` flag) |
| **Wasmtime** | WASM plugin runtime (feature-gated via `plugins` flag) |

---

## Module Index

The codebase contains **34 discrete modules** organized into 8 functional categories. Each module has detailed documentation in its respective `.md` file in this directory.

### [Core Agent Processing](agent.md)

The heart of the system — handles LLM interactions, tool execution, message processing, context management, and multi-agent coordination.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **agent** | Main `AgentLoop`, message processing, subagent pool, context compaction, model auto-routing, team coordination | `loop.rs`, `processor.rs`, `prompt.rs`, `compaction.rs`, `router.rs`, `task.rs`, `team.rs`, `teams.rs`, `worker.rs`, `mention.rs` |
| **provider** | Unified interface for 20+ LLM backends with chat streaming, model discovery, and caching | `anthropic.rs`, `openai.rs`, `google.rs`, `azure.rs`, `bedrock.rs`, `copilot.rs`, `gitlab.rs`, `cloudflare.rs`, `codegg_zen.rs`, `openrouter.rs`, `openai_compatible.rs`, `fallback.rs`, `cache.rs`, `catalog.rs`, `discovery.rs`, `models.rs`, `sse_parser.rs`, `text_tool_parser.rs`, `vertex.rs`, `additional.rs` |
| **tool** | Tool registry and 33+ built-in tools for file operations, git, search, web, LSP, and more | `bash.rs`, `read.rs`, `write.rs`, `edit.rs`, `glob.rs`, `grep.rs`, `git.rs`, `task.rs`, `webfetch.rs`, `websearch.rs`, `commit.rs`, `review.rs`, `formatter.rs`, `diff.rs`, `batch.rs`, `plan.rs`, `terminal.rs`, `todo.rs`, `skill.rs`, `teams.rs`, `lsp.rs`, `codesearch.rs`, `apply_patch.rs`, `multiedit.rs`, `replace.rs`, `invalid.rs`, `list.rs`, `question.rs`, `tool_search.rs`, `util.rs`, `executor.rs`, `catalog.rs`, `mod.rs` |

### [Communication & Events](event-bus.md)

Inter-component communication through a publish-subscribe event bus, plus lifecycle hooks for extensibility.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **bus** | `GlobalEventBus` (broadcast channel), `PermissionRegistry`, `QuestionRegistry` for inter-component communication | `global.rs`, `events.rs`, `mod.rs` |
| **hooks** | Lifecycle hooks system (`PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, etc.) for shell command hooks | `mod.rs` |
| **protocol** | `TuiMessage` enum for client-server TUI protocol over WebSocket | `tui.rs`, `mod.rs` |

### [Security & Permissions](permission.md)

Access control, encryption, and sandboxing for safe tool execution.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **permission** | Access control (`PermissionChecker`), path restrictions, DoomLoop detection, mode-based permissions (Review/Debug/Docs) | `mod.rs` |
| **security** | SSRF protection, internal IP validation (IPv4/IPv6), symlink detection, Landlock sandboxing | `mod.rs` |
| **crypto** | AES-256-GCM encryption with Argon2id key derivation for API keys and secrets | `mod.rs` |

### [Data & Persistence](session.md)

Session storage, memory management, snapshots, and database infrastructure.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **session** | SQLite-backed session storage, message history, checkpointing, import/export, session sharing with expiring tokens | `mod.rs`, `store.rs`, `message.rs`, `checkpoint.rs`, `import.rs`, `models.rs`, `row.rs`, `schema.rs`, `status.rs` |
| **storage** | SQLite database initialization, connection pooling, WAL mode, pragmas configuration | `mod.rs` |
| **memory** | Persistent memory system for session-to-session learning with namespace-based organization and rule-based consolidation | `mod.rs` |
| **snapshot** | File state capture (full/incremental) and restore for rollback safety | `mod.rs` |

### [User Interface](tui.md)

Terminal UI built with Ratatui, plus remote TUI client for server-based deployments.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **tui** | Ratatui-based terminal UI with 21 dialog types, FocusManager, keyboard shortcuts, theme system, notifications | `app/mod.rs`, `app/types.rs`, `components/*.rs` (17 components), `dialogs/*.rs` (21 dialogs) |
| **client** | WebSocket client for remote TUI connections to server with health checking and protocol handling | `mod.rs` |

### [External Integrations](mcp.md)

Protocol clients for IDE extensions and language servers.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **mcp** | Model Context Protocol client (local via stdio, remote via HTTP) with OAuth support and auto-reconnection | `mod.rs`, `local.rs`, `remote.rs`, `auth.rs`, `cli.rs`, `ide_server.rs` |
| **lsp** | Language Server Protocol client for diagnostics, code operations (goto definition, find references, hover, completion) | `client.rs`, `diagnostics.rs`, `download.rs`, `language.rs`, `launch.rs`, `operations.rs`, `root.rs`, `server.rs`, `service.rs`, `mod.rs` |
| **ide** | IDE detection (VS Code, JetBrains) and diff viewer integration | `mod.rs` |

### [Extensibility](plugin.md)

Plugin system, skill loading, and slash commands.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **plugin** | WASM plugin system via Wasmtime with 10 hook types, builtin handlers (copilot, gitlab, codex, poe), and TUI extensions | `mod.rs`, `loader.rs`, `hooks.rs`, `registry.rs`, `service.rs`, `manifest.rs`, `install.rs`, `marketplace.rs`, `api.rs`, `tui.rs`, `event_bus.rs` |
| **skills** | Skill system for specialized capabilities loaded from markdown files with YAML frontmatter | `mod.rs` |
| **command** | Slash command registry from markdown files with template variable substitution | `mod.rs` |

### [Infrastructure](config.md)

HTTP server, configuration, exec mode, and system utilities.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **config** | Configuration loading from JSONC files, schema validation, hot-reload via file watching, env var interpolation | `mod.rs`, `paths.rs`, `schema.rs`, `watcher.rs` |
| **server** | Axum-based HTTP server with WebSocket support, REST API (sessions, config, MCP, files, projects), SSE events, rate limiting | `mod.rs`, `http.rs`, `ws.rs`, `rpc.rs`, `state.rs`, `mdns.rs` |
| **exec** | Non-interactive exec mode for CI/CD with JSON input/output and structured error classification | `mod.rs` |

### [Utilities](error.md)

Error handling, resilience patterns, and helper functions.

| Module | Description | Key Files |
|--------|-------------|-----------|
| **error** | Centralized `AppError` enum using thiserror with `is_retryable()` methods and HTTP status mapping | `error.rs` (root) |
| **resilience** | Circuit breaker pattern with state machine (Closed/Open/HalfOpen) and `FallbackProvider` for provider redundancy | `mod.rs` |
| **util** | Clipboard operations, fuzzy string matching, text truncation, metrics collection | `clipboard.rs`, `fuzzy.rs`, `truncate.rs`, `stat_core.rs` |
| **worktree** | Git worktree management (list, create, remove) and git root detection | `mod.rs` |
| **pty_session** | Shell session metadata management (in-memory only, no actual PTY) | `mod.rs` |
| **upgrade** | Self-upgrade functionality via GitHub releases | `mod.rs` |
| **tts** | Text-to-speech (macOS only via `say` command) | `mod.rs` |

---

## System Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                       TUI                                               │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │  App (state machine)  │  Components (17)  │  Dialogs (21)  │  Input (keybindings) │    │
│  └─────────────────────────────────────────────────────────────────────────────────┘    │
│                                    │                                                     │
│                                    │ TuiMessage (via GlobalEventBus)                      │
└────────────────────────────────────┼─────────────────────────────────────────────────────┘
                                      │
           ┌──────────────────────────┴──────────────────────────┐
           │                                                     │
           ▼                                                     ▼
┌─────────────────────────────┐                   ┌─────────────────────────────┐
│      Local TUI               │                   │   Remote TUI (client)       │
│    (runs in terminal)      │                   │  (WebSocket to server)      │
└─────────────────────────────┘                   └─────────────────────────────┘
           │                                                     │
           └─────────────────────────────┬────────────────────────┘
                                         │
                                         ▼
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                   AgentLoop                                              │
│                                                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐   │
│  │   Provider   │  │     Tool      │  │  Permission  │  │   GlobalEventBus          │   │
│  │ (LLM Calls)  │  │   Registry    │  │   Checker    │  │   (pub/sub + registries) │   │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────────────────┘   │
│         │                 │                 │                     ▲                     │
│         │                 │                 │                     │                     │
│         ▼                 ▼                 ▼                     │                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │                     │
│  │    Hook     │  │   Snapshot   │  │     MCP      │           │                     │
│  │  Registry   │  │   Manager    │  │   Service    │           │                     │
│  └──────────────┘  └──────────────┘  └──────────────┘           │                     │
│         │                 │                 │                   │                     │
│         │                 │                 │                   │                     │
│         ▼                 ▼                 ▼                   │                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │                     │
│  │   Memory    │  │   Session    │  │     LSP      │           │                     │
│  │   Store     │  │   Storage    │  │   Client    │           │                     │
│  └──────────────┘  └──────────────┘  └──────────────┘           │                     │
└─────────────────────────────────────────────────────────────────────────────────────────┘
                                      │
           ┌──────────────────────────┼──────────────────────────┐
           │                          │                          │
           ▼                          ▼                          ▼
┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐
│   LLM Provider      │  │   MCP Servers       │  │   LSP Servers        │
│  (20+ backends)    │  │  (local/remote)     │  │ (42+ language servers│
└─────────────────────┘  └─────────────────────┘  └─────────────────────┘
```

---

## Key Data Flows

### Message Processing Flow

```
User Input → TUI → App::on_key() → TuiCommand
    → run_event_loop() → AgentLoop::run()
        → Provider::stream() [LLM call]
        → Tool execution via ToolRegistry
            → PermissionChecker::check()
            → Snapshot capture (before file-modifying tools)
            → HookRegistry::run_hooks()
        → GlobalEventBus::publish() for events
    → TUI receives events via bus_rx → UI update + re-render
```

### Permission Flow

```
Tool call request → PermissionChecker::check()
    → Check PermissionStore (cached decisions)
    → Check rules (agent > session > config)
    → Check path globs
    → If Ask: PermissionRegistry::register()
        → GlobalEventBus::publish(PermissionPending)
            → TUI shows permission dialog
                → User responds
                    → PermissionRegistry::respond()
                        → Decision cached
```

### Remote TUI Flow

```
Local Client                    Server                      AgentLoop
     │                             │                            │
     │──── health check ──────────>│                            │
     │<─── ok ─────────────────────│                            │
     │                             │                            │
     │──── WebSocket connect ────>│                            │
     │                             │                            │
     │<══════ TuiMessage protocol ═════════════════════════════════│
     │   Input/KeyDown/Resize      │                            │
     │         │                   │                            │
     │         │──────────────────>│   TuiCommand              │
     │         │                   │        │                   │
     │         │                   │        ▼                   │
     │         │                   │   AgentLoop::run()         │
     │         │                   │        │                   │
     │         │                   │        ▼                   │
     │<════════════════════════════│   AppEvent published       │
     │   SessionInfo/TextDelta     │        │                   │
     │   ToolCallStarted           │        ▼                   │
     │   ToolResult                │   GlobalEventBus::publish() │
     │                             │        │                   │
     │<═════════════════════════════│<──────┘                   │
```

---

## Feature Flags

| Feature | Description |
|---------|-------------|
| `server` | Enable Axum HTTP server + remote TUI support |
| `plugins` | Enable WASM plugin system via Wasmtime |
| `tts` | Enable text-to-speech (macOS `say` command) |
| `arboard` | Enable clipboard operations |
| `image` | Enable image support in TUI |
| `desktop` | Desktop-specific features |

---

## Database Schema

SQLite database (`sessions.db`) stores:

| Table | Description |
|-------|-------------|
| `sessions` | Conversation sessions with metadata |
| `messages` | Individual messages (as JSON) |
| `parts` | Message content parts (text, tool calls, tool results) |
| `todos` | Task tracking items |
| `snapshots` | File state captures |
| `session_share` | Session sharing tokens |
| `task` | Subagent task tracking |
| `cached_models` | Provider model cache |
| `migration_version` | Schema migration tracking |

See [session.md](session.md) for detailed schema documentation.

---

## Configuration

Configuration is loaded from (in order of precedence):

1. Environment variables (`CODAGG_*`)
2. Project config (`.codegg/codegg.jsonc`)
3. Global config (`~/.config/codegg/codegg.jsonc`)
4. System config (`/etc/codegg/codegg.json` on Unix, `~/Library/Application Support/codegg/codegg.json` on macOS)

See [config.md](config.md) for detailed configuration options.

---

## Directory Structure

```
src/
├── agent/              # AgentLoop, compaction, router, team, worker, task
├── bus/                # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── client/             # Remote TUI WebSocket client
├── command/            # Slash command registry
├── config/            # Configuration loading, validation, watching, schema
├── crypto/            # AES-256-GCM encryption
├── exec.rs            # Non-interactive exec mode
├── hooks/             # Lifecycle hooks system
├── ide/               # VS Code, JetBrains integration
├── lib.rs             # Module exports
├── main.rs            # Entry point, CLI
├── memory/            # Persistent memory system
├── mcp/               # Model Context Protocol client
├── permission/        # Access control, DoomLoop detection
├── plugin/            # WASM plugin system
├── protocol/          # TuiMessage enum
├── provider/          # LLM provider implementations
├── pty_session/       # Shell session metadata
├── resilience/        # Circuit breaker, FallbackProvider
├── security/          # SSRF protection, Landlock
├── server/            # HTTP server, WebSocket handlers
├── session/           # Session storage, schema, checkpointing
├── skills/            # Skill system
├── snapshot/          # File state capture and restore
├── storage/           # SQLite initialization
├── tool/              # Tool registry, built-in tools
├── tts/               # Text-to-speech
├── tui/               # Terminal UI (app, components, dialogs)
├── upgrade/           # Self-upgrade via GitHub
├── util/              # Clipboard, fuzzy, truncate, metrics
└── worktree/          # Git worktree management
```

---

## Module Dependency Graph

```
main.rs (CLI entry point)
├── config/
├── session/
├── memory/
├── mcp/
├── provider/
├── storage/
├── tui/
├── exec/
├── upgrade/
└── agent/

agent/ (depends on)
├── provider/
├── tool/
├── bus/
├── hooks/
├── session/
├── memory/
├── permission/
├── snapshot/
├── mcp/
├── lsp/
├── config/
└── resilience/

tui/ (depends on)
├── bus/
├── session/
├── command/
├── config/
└── plugin/

server/ (depends on)
├── bus/
├── session/
├── provider/
└── config/

client/ (depends on)
├── bus/
└── protocol/

mcp/ (depends on)
├── bus/
├── security/
└── crypto/

lsp/ (depends on)
├── bus/
└── security/
```

---

## Quick Reference

| Category | Key Files | See Also |
|----------|-----------|----------|
| Agent Loop | agent/loop.rs, agent/processor.rs | [agent.md](agent.md) |
| Providers | provider/mod.rs, provider/catalog.rs | [provider.md](provider.md) |
| Tools | tool/mod.rs, tool/executor.rs, tool/catalog.rs | [tool.md](tool.md) |
| Events | bus/global.rs, bus/events.rs | [event-bus.md](event-bus.md) |
| Permissions | permission/mod.rs | [permission.md](permission.md) |
| Session Storage | session/mod.rs, session/store.rs | [session.md](session.md) |
| TUI | tui/app/mod.rs, tui/components/* | [tui.md](tui.md) |
| Server | server/mod.rs, server/http.rs, server/ws.rs | [server.md](server.md) |
| MCP | mcp/mod.rs, mcp/local.rs, mcp/remote.rs | [mcp.md](mcp.md) |
| LSP | lsp/mod.rs, lsp/client.rs | [lsp.md](lsp.md) |
| Plugins | plugin/mod.rs, plugin/hooks.rs | [plugin.md](plugin.md) |
| Config | config/mod.rs, config/schema.rs | [config.md](config.md) |
| Security | security/mod.rs | [security.md](security.md) |
| Errors | error.rs (root) | [error.md](error.md) |
| Resilience | resilience/mod.rs | [resilience.md](resilience.md) |