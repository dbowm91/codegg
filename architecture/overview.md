# Codegg Architecture Overview

Codegg is a Rust rewrite of an AI coding agent built for performance and efficiency. It uses modern Rust async primitives and is designed for high-performance tool-augmented LLM interactions.

## Technology Stack

| Technology | Purpose |
|------------|---------|
| **Tokio** | Async runtime for concurrent operations |
| **SQLx** | SQLite database with compile-time query checking |
| **Ratatui** | Terminal UI framework |
| **Axum** | HTTP server (feature-gated) |
| **Wasmtime** | WASM plugin runtime (feature-gated) |

## Module Index

The codebase is organized into 32 discrete modules. Each module has detailed documentation below.

### Core Agent Modules

| Module | File | Description |
|--------|------|-------------|
| **agent** | [agent.md](agent.md) | Main agent loop, message processing, subagent pool, compaction, routing, team coordination |
| **provider** | [provider.md](provider.md) | Unified interface for LLM backends (Anthropic, OpenAI, Google, etc.) |
| **tool** | [tool.md) | Built-in tools (bash, read, edit, task, webfetch, etc.) and tool registry |

### Communication & Events

| Module | File | Description |
|--------|------|-------------|
| **bus** | [event-bus.md](event-bus.md) | GlobalEventBus, PermissionRegistry, QuestionRegistry for async communication |
| **hooks** | [hooks.md](hooks.md) | Lifecycle hooks system for agent loop events |

### Security & Permissions

| Module | File | Description |
|--------|------|-------------|
| **permission** | [permission.md](permission.md) | Access control, path restrictions, DoomLoop detection, mode system |
| **security** | [security.md](security.md) | SSRF protection, internal IP validation, Landlock sandboxing |
| **crypto** | [crypto.md](crypto.md) | AES-256-GCM encryption for API keys and secrets |

### Data & Persistence

| Module | File | Description |
|--------|------|-------------|
| **session** | [session.md](session.md) | Session storage, message history, checkpointing (SQLite) |
| **storage** | [storage.md](storage.md) | SQLite database initialization and connection pooling |
| **memory** | [memory.md](memory.md) | Persistent memory system for session-to-session learning |
| **snapshot** | [snapshot.md](snapshot.md) | File state capture and restore |

### User Interface

| Module | File | Description |
|--------|------|-------------|
| **tui** | [tui.md](tui.md) | Ratatui-based terminal UI (widgets, handlers, layout, diff viewer) |
| **client** | [client.md](client.md) | Remote TUI client for WebSocket connections |

### External Integrations

| Module | File | Description |
|--------|------|-------------|
| **mcp** | [mcp.md](mcp.md) | Model Context Protocol client (local, remote, OAuth) |
| **lsp** | [lsp.md](lsp.md) | Language Server Protocol support for diagnostics and code operations |
| **ide** | [ide.md](ide.md) | IDE integration (VS Code IPC, JetBrains remote mode) |

### Extensibility

| Module | File | Description |
|--------|------|-------------|
| **plugin** | [plugin.md](plugin.md) | WASM plugin system with hooks and TUI extensions |
| **skills** | [skills.md](skills.md) | Skill system for specialized capabilities |
| **command** | [command.md](command.md) | Slash command registry loaded from markdown files |

### Infrastructure

| Module | File | Description |
|--------|------|-------------|
| **config** | [config.md](config.md) | Configuration loading, validation, file watching |
| **server** | [server.md](server.md) | HTTP server (Axum) with WebSocket support |
| **exec** | [exec.md](exec.md) | Non-interactive exec mode for CI/CD with JSON I/O |
| **upgrade** | [upgrade.md](upgrade.md) | Self-upgrade functionality via GitHub releases |

### Utilities

| Module | File | Description |
|--------|------|-------------|
| **error** | [error.md](error.md) | Centralized AppError enum using thiserror |
| **util** | [util.md](util.md) | Utility functions (clipboard, fuzzy search, etc.) |
| **worktree** | [worktree.md](worktree.md) | Git worktree support |
| **resilience** | [resilience.md](resilience.md) | Circuit breaker, retry mechanisms |
| **pty** | [pty.md](pty.md) | Shell session metadata management |
| **tts** | [tts.md](tts.md) | Text-to-speech (macOS only) |

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                              TUI                                     │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌──────────┐ │
│  │  App    │  │Components│  │  Input  │  │ Layout  │  │  Theme   │ │
│  └────┬────┘  └─────────┘  └─────────┘  └─────────┘  └──────────┘ │
└───────┼────────────────────────────────────────────────────────────────┘
        │
        │ TuiCommand / TuiMsg
        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         AgentLoop                                    │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌─────────────────┐ │
│  │ Provider  │  │   Tool    │  │ Permission│  │      Bus        │ │
│  │ (LLM Call) │  │ Registry  │  │  Checker  │  │ (GlobalEventBus)│ │
│  └───────────┘  └───────────┘  └───────────┘  └─────────────────┘ │
│         │              │              │                  ▲
│         │              │              │                  │
│         ▼              ▼              ▼                  │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐            │
│  │   Hook    │  │  Snapshot │  │  Memory   │            │
│  │ Registry  │  │           │  │           │            │
│  └───────────┘  └───────────┘  └───────────┘            │
└───────────────────────────────────────────────────────────────┬─────┘
                                                                │
        ┌───────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         External Services                            │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  │
│  │   MCP   │  │   LSP   │  │   IDE   │  │Plugins  │  │  LLM    │  │
│  │ Servers │  │ Servers │  │   Diff  │  │ (WASM)  │  │Provider │  │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘  └─────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Key Data Flows

### Message Processing Flow

```
User Input → TUI → App::on_key() → Route handling → TuiCommand
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

## Feature Flags

The following features are available via Cargo flags:

| Feature | Description |
|---------|-------------|
| `server` | Enable Axum HTTP server |
| `plugin` | Enable WASM plugin support |
| `tts` | Enable text-to-speech |
| `desktop` | Desktop-specific features |

## Database Schema

The SQLite database stores:

- **sessions** - Conversation sessions with metadata
- **messages** - Individual messages within sessions
- **parts** - Message content parts (text, tool calls, tool results)
- **todos** - Task tracking items
- **snapshots** - File state captures

See [session.md](session.md) for detailed schema documentation.

## Configuration

Configuration is loaded from (in order of precedence):

1. Environment variables (`CODAGG_*`)
2. Project config (`.codegg/config.toml`)
3. Global config (`~/.config/codegg/config.toml`)

See [config.md](config.md) for detailed configuration options.
