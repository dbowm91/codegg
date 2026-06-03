# CodeGG Architecture Overview

CodeGG is a high-performance AI coding agent built in Rust, designed for terminal-based interaction with deep IDE and LSP integration. This document provides a bird's eye view of the entire system and serves as an index to detailed architecture documents.

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          Terminal (Ratatui)                         │
│                    Input ─────► TUI ─────► Output                   │
└────────────────────────┬────────────────────────────────────────────┘
                         │
              ┌──────────▼──────────┐
              │   CoreClient       │  Inproc / Stdio / Socket
              │  (Request/Response) │
              └──────────┬──────────┘
                         │
┌────────────────────────▼────────────────────────────────────────────┐
│                       AgentLoop                                      │
│  ┌─────────┐   ┌──────────┐   ┌─────────┐   ┌──────────────────┐    │
│  │ Provider│──▶│Messages  │◀──│  Tools  │◀──│ PermissionChecker│    │
│  └─────────┘   └──────────┘   └─────────┘   └──────────────────┘    │
│        │                                                 ▲          │
│        │              ┌─────────────┐                    │          │
│        └─────────────▶│  Bus/Events │────────────────────┘          │
│                       └─────────────┘                                │
│                          │                                           │
│  ┌──────────────────────┼───────────────────────────────────────┐  │
│  │            Modules    │                                       │  │
│  │  ┌────────┐ ┌───────┐ │ ┌───────┐ ┌──────┐ ┌────────┐       │  │
│  │  │ Session│ │Memory │ │ │ LSP   │ │MCP   │ │Plugins │       │  │
│  │  └────────┘ └───────┘ │ └───────┘ └──────┘ └────────┘       │  │
│  └──────────────────────┴───────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Protocol

- **Protocol Version**: 1 (defined in `protocol/core.rs`)
- **Request/Response Separation**: `CoreRequest` / `CoreResponse` via `RequestEnvelope<T>` and `EventEnvelope<T>`
- **Transport Adapters** (in `core/transport/`):
  - `InprocCoreClient` - In-process communication (used by TUI)
  - `StdioCoreClient` - Subprocess communication
  - `SocketCoreClient` - Network communication

## Module Map

| Module | Purpose | Key Files |
|--------|---------|-----------|
| [agent/](agent.md) | Main agent loop, compaction, routing, team coordination | `loop.rs`, `worker.rs`, `compaction.rs`, `router.rs` |
| [bus/](bus.md) | Event bus publish/subscribe, permission/question registries | `global.rs`, `events.rs`, `mod.rs` |
| [client/](client.md) | Remote TUI WebSocket client with resume/replay | `attach.rs` |
| [command/](command.md) | Slash command registry from markdown files | `mod.rs` |
| [config/](config.md) | Configuration loading, validation, file watching | `schema.rs`, `paths.rs`, `watcher.rs` |
| [core/](core.md) | Core facade, transport adapters, request handling | `mod.rs`, `transport/` |
| [crypto/](crypto.md) | AES-256-GCM encryption, Argon2id key derivation | `mod.rs` |
| [error/](error.md) | Centralized AppError enum with error classification | `mod.rs` |
| [exec/](exec.md) | Non-interactive exec mode for CI/CD with JSON I/O | `exec.rs` |
| [hooks/](hooks.md) | Lifecycle hooks for agent events | `mod.rs` |
| [ide/](ide.md) | VS Code / JetBrains detection and diff viewing | `mod.rs` |
| [lsp/](lsp.md) | Language Server Protocol support (39 servers) | `server.rs`, `service.rs`, `operations.rs` |
| [mcp/](mcp.md) | Model Context Protocol client (local/remote) | `local.rs`, `remote.rs`, `auth.rs` |
| [memory/](memory.md) | Persistent memory across sessions | `mod.rs` |
| [permission/](permission.md) | Access control, DoomLoop detection, mode system | `mod.rs`, `modes.rs` |
| [plugin/](plugin.md) | WASM plugin system with hooks and fuel tracking | `loader.rs`, `service.rs`, `manifest.rs` |
| [provider/](provider.md) | LLM providers (Anthropic, OpenAI, Google, etc.) | `mod.rs`, `anthropic.rs`, `fallback.rs` |
| [protocol/](protocol.md) | Shared request/response envelopes and message types | `core.rs`, `tui.rs` |
| [resilience/](resilience.md) | Circuit breaker, retry mechanisms | `circuit.rs` |
| [security/](security.md) | SSRF protection, Landlock sandboxing | `ssrf.rs`, `sandbox.rs` |
| [server/](server.md) | HTTP/WebSocket server for remote TUI | `http.rs`, `ws.rs`, `routes/` |
| [session/](session.md) | SQLite session storage, message history | `store.rs`, `schema.rs`, `message.rs` |
| [shell_session/](shell_session.md) | Shell session metadata (no PTY) | `mod.rs` |
| [skills/](skills.md) | Runtime skill loader and activation | `mod.rs` |
| [snapshot/](snapshot.md) | File state capture and restore | `mod.rs` |
| [storage/](storage.md) | SQLite initialization and connection pooling | `mod.rs` |
| [tool/](tool.md) | Built-in tools (27 tools in default registry) | `mod.rs`, `bash.rs`, `read.rs`, etc. |
| [tts/](tts.md) | Text-to-speech (macOS `say` command) | `mod.rs` |
| [tui/](tui.md) | Terminal user interface (Ratatui) | `app/mod.rs`, `components/` |
| [upgrade/](upgrade.md) | Self-upgrade via GitHub releases | `mod.rs` |
| [util/](util.md) | Clipboard, fuzzy search, pricing, metrics | `mod.rs` |
| [worktree/](worktree.md) | Git worktree management | `mod.rs` |

**Orphaned module**: `git/` exists at `src/git/mod.rs` but is not declared in `lib.rs` and has no references.

## Key Types

### Agent Loop
- `AgentLoop` - Main execution cycle in `agent/loop.rs`
- `Agent` - Agent definition with mode (Primary/Subagent/All)
- 7 built-in agents: build, plan, general, explore, title, summary, compaction

### Tools
- `Tool` trait - All tools implement `name()`, `description()`, `parameters()`, `execute()`
- 27 built-in tools in default registry (bash, read, edit, write, glob, grep, task, webfetch, etc.)
- `ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`)

### Events
- `AppEvent` enum - 36 variants for session, tool, MCP, permission, subagent events
- `GlobalEventBus` - tokio broadcast channel (2048 buffer)
- PermissionRegistry and QuestionRegistry are **synchronous** (`fn`, not `async fn`)

### Session
- SQLite storage with WAL mode, 15 migrations
- Tables (13): `migration_version`, `project`, `session`, `message`, `part`, `todo`, `permission`, `session_share`, `cached_models`, `task`, `checkpoints`, `snapshot`, `usage`

### Provider
- `Provider` trait with `chat()` streaming method
- `FallbackProvider` with circuit breaker (backoff: `2^i`)
- Auto-registered via env vars (16 providers): anthropic, openai, google, openrouter, opencode_zen, mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, minimax, opencode_go
- Config-only (not auto-registered): SAP AI Core, Zenmux, Kilo, Vercel AI Gateway

## Verified Counts

| Item | Count | Location |
|------|-------|----------|
| Tools (default registry) | 27 | `tool/mod.rs:89-119` |
| LSP servers | 39 | `lsp/server.rs:27-383` |
| UiState fields | 26 | `tui/app/state/ui.rs:27-76` |
| AppEvent variants | 36 | `bus/events.rs:5-147` |
| Built-in commands | 46 | `tui/command.rs:79-182` |
| Built-in agents | 7 | `agent/mod.rs:147-262` |

## Feature Gates

| Feature | Description |
|---------|-------------|
| `server` | Axum HTTP server, WebSocket TUI |
| `plugins` | WASM plugin system with wasmtime |
| `image` | Image support via ratatui-image |
| `arboard` | Clipboard support (default feature) |
| `debug-logging` | Debug logging output |

## Database Schema

```
┌───────────────────────────────────────────────────────────────────┐
│ Tables (13 total, 15 migrations)                                  │
├───────────────────────────────────────────────────────────────────┤
│ migration_version  │ project        │ session        │ message    │
│ part               │ todo           │ permission     │ session_share │
│ cached_models      │ task           │ checkpoints    │ snapshot   │
│ usage              │                                                     │
└───────────────────────────────────────────────────────────────────┘
```

## Event Flow

```
User Input → TUI Event Loop → App::on_key() → State Mutation → Render
                                    │
                         CoreClient.request()
                                    │
                    ┌───────────────┼───────────────┐
                    ▼               ▼               ▼
              AgentLoop      PermissionChecker    HookRegistry
                    │               │               │
                    ▼               ▼               ▼
              Provider ◀──── ToolRegistry ────▶ Tools
                    │
                    ▼
            GlobalEventBus::publish()
                    │
                    ▼
            CoreClient.subscribe() → TUI updates
```

## Error Handling

- `AppError` enum - centralized error type
- `ProviderError::is_retryable()` - RateLimit, Timeout, Stream, CircuitOpen, Auth
- `ToolError::is_retryable()` - Io, Network, Timeout
- `McpError::is_retryable()` - Connection, Server, ToolCall, OAuth, Timeout
- `LspError::is_retryable()` - DownloadFailed, LaunchFailed, RequestFailed, RequestTimeout, Io

## Security

- AES-256-GCM with Argon2id key derivation for API key encryption
- SSRF protection with internal IP validation
- HMAC-based permission decision persistence
- Landlock filesystem sandboxing for bash tool

## Navigation

- [Agent Loop](agent.md) - Main execution cycle, compaction, routing
- [Bus/Events](bus.md) - Event bus and registries
- [Client](client.md) - Remote TUI WebSocket client
- [Command](command.md) - Slash command registry
- [Compaction](compaction.md) - Context window overflow management
- [Config](config.md) - Configuration loading and validation
- [Core](core.md) - CoreClient facade and transports
- [Crypto](crypto.md) - API key encryption
- [Error](error.md) - Centralized error handling
- [Exec](exec.md) - Non-interactive execution mode
- [Git](git.md) - Git session management
- [Hooks](hooks.md) - Lifecycle hooks
- [IDE](ide.md) - VS Code/JetBrains integration
- [LSP](lsp.md) - Language Server Protocol
- [MCP](mcp.md) - Model Context Protocol
- [Memory](memory.md) - Persistent memory system
- [Permission](permission.md) - Access control and modes
- [Plugin](plugin.md) - WASM plugin system
- [Protocol](protocol.md) - Shared request/response envelopes
- [Provider](provider.md) - LLM provider implementations
- [Resilience](resilience.md) - Circuit breaker patterns
- [Security](security.md) - SSRF, sandboxing
- [Server](server.md) - HTTP/WebSocket for remote TUI
- [Session](session.md) - SQLite storage and message history
- [Shell Session](shell_session.md) - Shell session metadata
- [Skills](skills.md) - Runtime skill loader
- [Snapshot](snapshot.md) - File state capture and restore
- [Storage](storage.md) - SQLite initialization
- [Tool](tool.md) - Tool system and registry
- [TTS](tts.md) - Text-to-speech
- [TUI](tui.md) - Terminal user interface
- [Upgrade](upgrade.md) - Self-upgrade functionality
- [Util](util.md) - Utility functions
- [Worktree](worktree.md) - Git worktree management
