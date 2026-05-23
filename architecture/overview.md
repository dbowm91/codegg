# Codegg Architecture Overview

Codegg is a Rust rewrite of an AI coding agent built for performance and efficiency. It uses modern Rust async primitives and is designed for high-performance tool-augmented LLM interactions.

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

The codebase contains **34 discrete modules**. Each module has detailed documentation in its respective `.md` file below.

### Core Agent Processing

| Module | Docs | Description |
|--------|------|-------------|
| **agent** | [agent.md](agent.md) | Main agent loop (`AgentLoop`), message processing, subagent pool, context compaction, model auto-routing, team coordination |
| **provider** | [provider.md](provider.md) | Unified interface for 20+ LLM backends (Anthropic, OpenAI, Google, Azure, Bedrock, etc.) with chat streaming, model discovery, and caching |
| **tool** | [tool.md](tool.md) | Tool registry and 33+ built-in tools (bash, read, write, edit, glob, grep, git, webfetch, task, etc.) |

### Communication & Events

| Module | Docs | Description |
|--------|------|-------------|
| **bus** | [event-bus.md](event-bus.md) | `GlobalEventBus` (broadcast channel), `PermissionRegistry`, `QuestionRegistry` for inter-component communication |
| **hooks** | [hooks.md](hooks.md) | Lifecycle hooks system (`PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, etc.) for shell command hooks |
| **protocol** | (inline) | `TuiMessage` enum for client-server TUI protocol over WebSocket |

### Security & Permissions

| Module | Docs | Description |
|--------|------|-------------|
| **permission** | [permission.md](permission.md) | Access control (`PermissionChecker`), path restrictions, DoomLoop detection, mode-based permissions (Review/Debug/Docs) |
| **security** | [security.md](security.md) | SSRF protection, internal IP validation (IPv4/IPv6), symlink detection, Landlock sandboxing |
| **crypto** | [crypto.md](crypto.md) | AES-256-GCM encryption with Argon2id key derivation for API keys and secrets |

### Data & Persistence

| Module | Docs | Description |
|--------|------|-------------|
| **session** | [session.md](session.md) | SQLite-backed session storage, message history, checkpointing, import/export, session sharing with expiring tokens |
| **storage** | [storage.md](storage.md) | SQLite database initialization, connection pooling, WAL mode, pragmas configuration |
| **memory** | [memory.md](memory.md) | Persistent memory system for session-to-session learning with namespace-based organization and rule-based consolidation |
| **snapshot** | [snapshot.md](snapshot.md) | File state capture (full/incremental) and restore for rollback safety |

### User Interface

| Module | Docs | Description |
|--------|------|-------------|
| **tui** | [tui.md](tui.md) | Ratatui-based terminal UI with 21 dialog types, FocusManager, keyboard shortcuts, theme system, notifications |
| **client** | [client.md](client.md) | WebSocket client for remote TUI connections to server with health checking and protocol handling |

### External Integrations

| Module | Docs | Description |
|--------|------|-------------|
| **mcp** | [mcp.md](mcp.md) | Model Context Protocol client (local via stdio, remote via HTTP) with OAuth support and auto-reconnection |
| **lsp** | [lsp.md](lsp.md) | Language Server Protocol client for diagnostics, code operations (goto definition, find references, hover, completion) |
| **ide** | [ide.md](ide.md) | IDE detection (VS Code, JetBrains) and diff viewer integration |

### Extensibility

| Module | Docs | Description |
|--------|------|-------------|
| **plugin** | [plugin.md](plugin.md) | WASM plugin system via Wasmtime with 10 hook types, builtin handlers (copilot, gitlab, codex, poe), and TUI extensions |
| **skills** | [skills.md](skills.md) | Skill system for specialized capabilities loaded from markdown files with YAML frontmatter |
| **command** | [command.md](command.md) | Slash command registry from markdown files with template variable substitution |

### Infrastructure

| Module | Docs | Description |
|--------|------|-------------|
| **config** | [config.md](config.md) | Configuration loading from JSONC files, schema validation, hot-reload via file watching, env var interpolation |
| **server** | [server.md](server.md) | Axum-based HTTP server with WebSocket support, REST API (sessions, config, MCP, files, projects), SSE events, rate limiting |
| **exec** | [exec.md](exec.md) | Non-interactive exec mode for CI/CD with JSON input/output and structured error classification |

### Utilities

| Module | Docs | Description |
|--------|------|-------------|
| **error** | [error.md](error.md) | Centralized `AppError` enum using thiserror with `is_retryable()` methods and HTTP status mapping |
| **resilience** | [resilience.md](resilience.md) | Circuit breaker pattern with state machine (Closed/Open/HalfOpen) and `FallbackProvider` for provider redundancy |
| **util** | [util.md](util.md) | Clipboard operations, fuzzy string matching, text truncation, metrics collection |
| **worktree** | [worktree.md](worktree.md) | Git worktree management (list, create, remove) and git root detection |
| **pty** | [pty.md](pty.md) | Shell session metadata management (in-memory only, no actual PTY) |
| **upgrade** | [upgrade.md](upgrade.md) | Self-upgrade functionality via GitHub releases |
| **tts** | [tts.md](tts.md) | Text-to-speech (macOS only via `say` command) |

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                    TUI                                           │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │  App (state machine)  │  Components (21 dialogs)  │  Input (keybindings) │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
│                              │                                                 │
│                              │ TuiMessage (via GlobalEventBus)                  │
└──────────────────────────────┼─────────────────────────────────────────────────┘
                               │
         ┌─────────────────────┴─────────────────────┐
         │                                           │
         ▼                                           ▼
┌─────────────────────────────┐         ┌─────────────────────────────┐
│        Local TUI             │         │     Remote TUI (client)      │
│   (runs in terminal)         │         │   (WebSocket to server)     │
└─────────────────────────────┘         └─────────────────────────────┘
         │                                           │
         └─────────────────────┬─────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              AgentLoop                                           │
│                                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │   Provider   │  │     Tool      │  │  Permission  │  │   GlobalEvent    │    │
│  │ (LLM Calls)  │  │   Registry    │  │   Checker    │  │       Bus        │    │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────────┘    │
│         │                 │                 │                   ▲               │
│         │                 │                 │                   │               │
│         ▼                 ▼                 ▼                   │               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐         │               │
│  │    Hook     │  │   Snapshot   │  │    MCP      │         │               │
│  │  Registry   │  │   Manager    │  │   Service   │         │               │
│  └──────────────┘  └──────────────┘  └──────────────┘         │               │
│         │                 │                 │                   │               │
│         │                 │                 │                   │               │
│         ▼                 ▼                 ▼                   │               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐         │               │
│  │   Memory    │  │   Session    │  │    LSP      │         │               │
│  │   Store     │  │   Storage    │  │   Client    │         │               │
│  └──────────────┘  └──────────────┘  └──────────────┘         │               │
└─────────────────────────────────────────────────────────────────────────────────┘
                               │
         ┌─────────────────────┼─────────────────────┐
         │                     │                     │
         ▼                     ▼                     ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   LLM Provider  │  │   MCP Servers   │  │   LSP Servers   │
│  (20+ backends) │  │  (local/remote) │  │ (30+ languages) │
└─────────────────┘  └─────────────────┘  └─────────────────┘
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
    │<══════ TuiMessage protocol ═════════════════════════════│
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
├── agent/              # Main agent loop, compaction, router, team
├── bus/                # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── client/             # Remote TUI WebSocket client
├── command/            # Slash command registry
├── config/            # Configuration loading, validation, watching
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
├── pty/               # Shell session metadata
├── resilience/        # Circuit breaker, FallbackProvider
├── security/          # SSRF protection, Landlock
├── server/            # HTTP server, WebSocket handlers
├── session/           # Session storage, schema, checkpointing
├── skills/            # Skill system
├── snapshot/          # File state capture and restore
├── storage/           # SQLite initialization
├── tool/              # Tool registry, built-in tools
├── tts/               # Text-to-speech
├── tui/               # Terminal UI
├── upgrade/           # Self-upgrade via GitHub
├── util/              # Clipboard, fuzzy, truncate, metrics
└── worktree/          # Git worktree management
```