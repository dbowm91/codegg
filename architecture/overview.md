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

The codebase contains **32 discrete modules** organized into 9 functional categories. Each module has detailed documentation in its respective `.md` file in this directory.

### [Core Agent Processing](agent.md)

The heart of the system — handles LLM interactions, tool execution, message processing, context management, and multi-agent coordination.

| Module | Description |
|--------|-------------|
| **agent** | Main `AgentLoop`, message processing, subagent pool, context compaction, model auto-routing, team coordination |
| **provider** | Unified interface for 20+ LLM backends with chat streaming, model discovery, and caching |
| **tool** | Tool registry and 33+ built-in tools for file operations, git, search, web, LSP, and more |

**Deep Dive**: See [agent.md](agent.md) for AgentLoop internals, compaction strategies, router, and team coordination.

### [Communication & Events](event-bus.md)

Inter-component communication through a publish-subscribe event bus, plus lifecycle hooks for extensibility.

| Module | Description |
|--------|-------------|
| **bus** | `GlobalEventBus` (broadcast channel), `PermissionRegistry`, `QuestionRegistry` for inter-component communication |
| **hooks** | Lifecycle hooks system (`PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, etc.) for shell command hooks |

**Deep Dive**: See [event-bus.md](event-bus.md) for event types, subscription patterns, and the permission/question registry.

### [Core Runtime & Protocol](core.md)

Request/response facade for separating TUI transport from core session and agent logic.

| Module | Description |
|--------|-------------|
| **core** | Typed `CoreClient` facade with in-process, stdio, and socket transport adapters |
| **protocol** | Shared `CoreRequest`/`CoreResponse` envelopes plus the TUI `TuiMessage` protocol and resume/replay envelopes |

**Deep Dive**: See [core.md](core.md) for the transport split, request families, and protocol envelopes.

### [Security & Permissions](permission.md)

Access control, encryption, and sandboxing for safe tool execution.

| Module | Description |
|--------|-------------|
| **permission** | Access control (`PermissionChecker`), path restrictions, DoomLoop detection, mode-based permissions (Review/Debug/Docs) |
| **security** | SSRF protection, internal IP validation (IPv4/IPv6), symlink detection, Landlock sandboxing |
| **crypto** | AES-256-GCM encryption with Argon2id key derivation for API keys and secrets |

**Deep Dive**: See [permission.md](permission.md) for permission rulesets, modes, and DoomLoop detection. See [security.md](security.md) for SSRF protection and sandboxing.

### [Data & Persistence](session.md)

Session storage, memory management, snapshots, and database infrastructure.

| Module | Description |
|--------|-------------|
| **session** | SQLite-backed session storage, message history, checkpointing, import/export, session sharing with expiring tokens |
| **storage** | SQLite database initialization, connection pooling, WAL mode, pragmas configuration |
| **memory** | Persistent memory system for session-to-session learning with namespace-based organization and rule-based consolidation |
| **snapshot** | File state capture (full/incremental) and restore for rollback safety |

**Deep Dive**: See [session.md](session.md) for database schema, stores, and checkpointing. See [storage.md](storage.md) for SQLite initialization.

### [User Interface](tui.md)

Terminal UI built with Ratatui, plus remote TUI client for server-based deployments.

| Module | Description |
|--------|-------------|
| **tui** | Ratatui-based terminal UI with CoreClient-backed session flows, 21 dialog types, FocusManager, keyboard shortcuts, theme system, notifications |
| **client** | WebSocket client for remote TUI connections to server with health checking, resume/replay, and protocol handling |

**Deep Dive**: See [tui.md](tui.md) for App state machine, components, dialogs, and input handling.

### [External Integrations](mcp.md)

Protocol clients for IDE extensions and language servers.

| Module | Description |
|--------|-------------|
| **mcp** | Model Context Protocol client (local via stdio, remote via HTTP) with OAuth support and auto-reconnection |
| **lsp** | Language Server Protocol client for diagnostics, code operations (goto definition, find references, hover, completion) |
| **ide** | IDE detection (VS Code, JetBrains) and diff viewer integration |

**Deep Dive**: See [mcp.md](mcp.md) for MCP client implementations. See [lsp.md](lsp.md) for LSP client and server management.

### [Extensibility](plugin.md)

Plugin system, skill loading, and slash commands.

| Module | Description |
|--------|-------------|
| **plugin** | WASM plugin system via Wasmtime with 10 hook types, builtin handlers (copilot, gitlab, codex, poe), and TUI extensions |
| **skills** | Skill system for specialized capabilities loaded from markdown files with YAML frontmatter |
| **command** | Slash command registry from markdown files with template variable substitution |

**Deep Dive**: See [plugin.md](plugin.md) for WASM hooks, plugin service, and TUI extensions.

### [Infrastructure](config.md)

HTTP server, configuration, exec mode, and system utilities.

| Module | Description |
|--------|-------------|
| **config** | Configuration loading from JSONC files, schema validation, hot-reload via file watching, env var interpolation |
| **server** | Axum-based HTTP server with WebSocket support, REST API (sessions, config, MCP, files, projects), SSE events, rate limiting, TUI replay buffering |
| **exec** | Non-interactive exec mode for CI/CD with JSON input/output and structured error classification |

**Deep Dive**: See [config.md](config.md) for configuration schema and validation. See [server.md](server.md) for HTTP/WebSocket server architecture.

### [Utilities](error.md)

Error handling, resilience patterns, and helper functions.

| Module | Description |
|--------|-------------|
| **error** | Centralized `AppError` enum using thiserror with `is_retryable()` methods and HTTP status mapping |
| **resilience** | Circuit breaker pattern with state machine (Closed/Open/HalfOpen) and `FallbackProvider` for provider redundancy |
| **util** | Clipboard operations, fuzzy string matching, text truncation, metrics collection |
| **worktree** | Git worktree management (list, create, remove) and git root detection |
| **pty_session** | Shell session metadata management (in-memory only, no actual PTY) |
| **upgrade** | Self-upgrade functionality via GitHub releases |
| **tts** | Text-to-speech (macOS only via `say` command) |

**Deep Dive**: See [error.md](error.md) for error classification. See [resilience.md](resilience.md) for circuit breaker and fallback provider.

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
├── core/               # CoreClient facade and transport adapters
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
├── protocol/          # CoreRequest/CoreResponse and TuiMessage protocols
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
├── core/
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

core/ (depends on)
├── agent/
├── bus/
├── provider/
├── tool/
├── permission/
├── session/
├── memory/
├── storage/
└── protocol/

tui/ (depends on)
├── core/
├── bus/
├── session/
├── command/
├── config/
└── plugin/

server/ (depends on)
├── bus/
├── session/
├── provider/
├── protocol/
└── config/

client/ (depends on)
├── protocol/
└── tui/

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
| Core Runtime | core/mod.rs, core/transport/* | [core.md](core.md) |
| Providers | provider/mod.rs, provider/catalog.rs | [provider.md](provider.md) |
| Tools | tool/mod.rs, tool/executor.rs, tool/catalog.rs | [tool.md](tool.md) |
| Events | bus/global.rs, bus/events.rs | [event-bus.md](event-bus.md) |
| Permissions | permission/mod.rs | [permission.md](permission.md) |
| Session Storage | session/mod.rs, session/store.rs | [session.md](session.md) |
| TUI | tui/app/mod.rs, tui/components/* | [tui.md](tui.md) |
| Client | client/attach.rs, client/sdk.rs | [client.md](client.md) |
| Server | server/mod.rs, server/http.rs, server/ws.rs | [server.md](server.md) |
| MCP | mcp/mod.rs, mcp/local.rs, mcp/remote.rs | [mcp.md](mcp.md) |
| LSP | lsp/mod.rs, lsp/client.rs | [lsp.md](lsp.md) |
| Plugins | plugin/mod.rs, plugin/hooks.rs | [plugin.md](plugin.md) |
| Config | config/mod.rs, config/schema.rs | [config.md](config.md) |
| Security | security/mod.rs | [security.md](security.md) |
| Errors | error.rs (root) | [error.md](error.md) |
| Resilience | resilience/mod.rs | [resilience.md](resilience.md) |

---

## Architecture Files

Each module has dedicated documentation:

| File | Module | Description |
|------|--------|-------------|
| [agent.md](agent.md) | agent | AgentLoop, compaction, router, task, team, worker |
| [tool.md](tool.md) | tool | Tool registry, built-in tools, executor |
| [provider.md](provider.md) | provider | LLM backends, streaming, model catalog |
| [event-bus.md](event-bus.md) | bus | GlobalEventBus, AppEvent, registries |
| [permission.md](permission.md) | permission | PermissionChecker, modes, DoomLoop |
| [security.md](security.md) | security | SSRF, IP validation, Landlock |
| [crypto.md](crypto.md) | crypto | AES-256-GCM encryption, key derivation |
| [session.md](session.md) | session | Session storage, stores, checkpointing |
| [storage.md](storage.md) | storage | SQLite initialization, pooling |
| [memory.md](memory.md) | memory | Memory store, consolidation, namespaces |
| [snapshot.md](snapshot.md) | snapshot | File state capture, restore |
| [tui.md](tui.md) | tui | App, components, dialogs, input, core-backed flows |
| [client.md](client.md) | client | Remote TUI WebSocket client, resume/replay |
| [core.md](core.md) | core | CoreClient facade, transport adapters, protocol envelopes |
| [mcp.md](mcp.md) | mcp | MCP client, local/remote, OAuth |
| [lsp.md](lsp.md) | lsp | LSP client, diagnostics, operations |
| [ide.md](ide.md) | ide | IDE detection, diff viewing |
| [plugin.md](plugin.md) | plugin | WASM plugins, hooks, TUI extensions |
| [skills.md](skills.md) | skills | Skill system, YAML frontmatter |
| [command.md](command.md) | command | Slash commands, templates |
| [config.md](config.md) | config | Configuration schema, validation |
| [server.md](server.md) | server | HTTP server, WebSocket, REST API, replay buffer |
| [error.md](error.md) | error | AppError enum, error classification |
| [resilience.md](resilience.md) | resilience | Circuit breaker, FallbackProvider |
| [exec.md](exec.md) | exec | Non-interactive exec mode |
| [hooks.md](hooks.md) | hooks | Lifecycle hooks system |
| [pty.md](pty.md) | pty_session | Shell session metadata |
| [upgrade.md](upgrade.md) | upgrade | Self-upgrade via GitHub |
| [util.md](util.md) | util | Clipboard, fuzzy, truncate |
| [worktree.md](worktree.md) | worktree | Git worktree management |
| [compaction.md](compaction.md) | agent/compaction | Context overflow strategies |
| [tts.md](tts.md) | tts | Text-to-speech |
