# Codegg Architecture Overview

**Codegg** is a Rust-based AI coding agent built for performance and efficiency. It features a full terminal UI, tool-augmented LLM interactions, MCP/LSP integration, and extensibility through WASM plugins.

## Technology Stack

| Technology | Purpose |
|------------|---------|
| **Tokio** | Async runtime for concurrent operations |
| **SQLx** | SQLite with compile-time query verification |
| **Ratatui** | Terminal UI framework |
| **Axum** | HTTP server (feature-gated via `server` flag) |
| **Wasmtime** | WASM plugin runtime (feature-gated via `plugins` flag) |

---

## System Architecture

The system follows a **layered architecture** separating the TUI frontend, Core runtime, and Agent processing engine:

```
┌─────────────────────────────────────────────────────────────────┐
│                          TUI Layer                              │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  App (State Machine) │ Components │ Dialogs (21) │ Input  │  │
│  └────────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                    TuiMessage │ CoreResponse                     │
│                              │                                   │
├──────────────────────────────┼──────────────────────────────────┤
│                         Core Layer                               │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  CoreClient (facade) │ Transport Adapters │ Protocol       │  │
│  └────────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              ▼                                   │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │                       AgentLoop                            │  │
│  │  ┌─────────┐  ┌────────┐  ┌───────────┐  ┌──────────────┐  │  │
│  │  │Provider │  │  Tool  │  │Permission │  │GlobalEventBus│  │  │
│  │  │ (LLM)   │  │Registry│  │  Checker  │  │  (pub/sub)   │  │  │
│  │  └─────────┘  └────────┘  └───────────┘  └──────────────┘  │  │
│  │       │            │             │                ▲       │  │
│  │       ▼            ▼             ▼                │       │  │
│  │  ┌─────────┐  ┌────────┐  ┌───────────┐  ┌────────┐  │       │
│  │  │  Hook   │  │Snapshot│  │    MCP    │  │  LSP   │  │       │
│  │  │Registry │  │Manager │  │  Service  │  │ Client │  │       │
│  │  └─────────┘  └────────┘  └───────────┘  └────────┘  │       │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
                       │                    │                    │
           ┌───────────┴─────┐  ┌───────────┴──────┐  ┌─────────┴───────┐
│  LLM Provider   │  │   MCP Servers    │  │   LSP Servers   │
            │  (20+ models)   │  │ (local/remote)  │  │   (39 servers)  │
           └─────────────────┘  └──────────────────┘  └────────────────┘
```

---

## Module Index

Each module has a dedicated `.md` file in `architecture/`. Click any link for a deep dive.

### Core Runtime

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Agent](agent.md)** | AgentLoop, message processing, subagent pool, compaction, routing, team coordination | [agent.md](agent.md) |
| **[Provider](provider.md)** | Unified interface for 20+ LLM backends with streaming, model discovery, caching | [provider.md](provider.md) |
| **[Tool](tool.md)** | Tool registry and 26 built-in tools for file ops, git, search, web, and more | [tool.md](tool.md) |
| **[Event Bus](bus.md)** | GlobalEventBus (pub/sub), PermissionRegistry, QuestionRegistry | [bus.md](bus.md) |
| **[Core](core.md)** | CoreClient facade, transport adapters (inproc/stdio/socket), protocol envelopes | [core.md](core.md) |
| **[Compaction](compaction.md)** | Context window overflow management through intelligent compaction strategies | [compaction.md](compaction.md) |

### Security & Access Control

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Permission](permission.md)** | Access control, path restrictions, DoomLoop detection, mode system | [permission.md](permission.md) |
| **[Security](security.md)** | SSRF protection, internal IP validation, symlink detection, Landlock sandboxing | [security.md](security.md) |
| **[Crypto](crypto.md)** | AES-256-GCM encryption with Argon2id key derivation for API keys | [crypto.md](crypto.md) |

### Data & State Management

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Session](session.md)** | SQLite-backed session storage, message history, checkpointing | [session.md](session.md) |
| **[Storage](storage.md)** | SQLite initialization, connection pooling, WAL mode | [storage.md](storage.md) |
| **[Memory](memory.md)** | Persistent memory system with namespace-based organization | [memory.md](memory.md) |
| **[Snapshot](snapshot.md)** | File state capture (full/incremental) and restore | [snapshot.md](snapshot.md) |

### UI & Rendering

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[TUI](tui.md)** | Ratatui-based terminal UI with 21 dialog types, FocusManager, keyboard shortcuts | [tui.md](tui.md) |
| **[Client](client.md)** | WebSocket client for remote TUI connections with resume/replay | [client.md](client.md) |
| **[IDE](ide.md)** | IDE detection (VS Code, JetBrains) and diff viewer integration | [ide.md](ide.md) |

### Integration Services

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Server](server.md)** | Axum HTTP server with WebSocket, REST API, SSE events, rate limiting | [server.md](server.md) |
| **[MCP](mcp.md)** | Model Context Protocol client (local via stdio, remote via HTTP) with OAuth | [mcp.md](mcp.md) |
| **[LSP](lsp.md)** | Language Server Protocol client for diagnostics, code operations | [lsp.md](lsp.md) |
| **[Protocol](protocol.md)** | CoreRequest/CoreResponse, TuiMessage, EventEnvelope protocol definitions | [protocol.md](protocol.md) |

### Extensibility

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Plugin](plugin.md)** | WASM plugin system via Wasmtime with 13 hook types, builtin handlers | [plugin.md](plugin.md) |
| **[Skills](skills.md)** | Skill system loaded from markdown files with YAML frontmatter | [skills.md](skills.md) |
| **[Command](command.md)** | Slash command registry from markdown files with template substitution | [command.md](command.md) |
| **[Hooks](hooks.md)** | Lifecycle hooks system for agent loop events | [hooks.md](hooks.md) |

### Configuration & Utilities

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Config](config.md)** | Configuration loading, schema validation, hot-reload via file watching | [config.md](config.md) |
| **[Error](error.md)** | Centralized AppError enum with ProviderError, ToolError, is_retryable | [error.md](error.md) |
| **[Resilience](resilience.md)** | Circuit breaker pattern and FallbackProvider for redundancy | [resilience.md](resilience.md) |
| **[Util](util.md)** | Clipboard, fuzzy matching, text truncation, metrics | [util.md](util.md) |

### Additional Modules

| Module | Description | Deep Dive |
|--------|-------------|-----------|
| **[Exec](exec.md)** | Non-interactive exec mode for CI/CD with JSON I/O | [exec.md](exec.md) |
| **[PTY Session](pty_session.md)** | Shell session metadata management (in-memory, no actual PTY) | [pty_session.md](pty_session.md) |
| **[Worktree](worktree.md)** | Git worktree management, git root detection | [worktree.md](worktree.md) |
| **[TTS](tts.md)** | Text-to-speech (macOS only via `say` command) | [tts.md](tts.md) |
| **[Upgrade](upgrade.md)** | Self-upgrade functionality via GitHub releases | [upgrade.md](upgrade.md) |

---

## Key Data Flows

### Message Processing Flow

```
User Input → TUI → App::on_key() → TuiCommand
    → run_event_loop() → CoreClient::request()
        → AgentLoop::run()
            → Provider::stream() [LLM call]
            → Tool execution via ToolRegistry
                → PermissionChecker::check()
                → Snapshot capture (before file-modifying tools)
                → HookRegistry::run_hooks()
            → GlobalEventBus::publish() for events
        → CoreResponse returned to TUI
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
     │──── health check ─────────>│                            │
     │<─── ok ────────────────────│                            │
     │                             │                            │
     │──── WebSocket connect ────>│                            │
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
     │   ToolResult                │   GlobalEventBus::publish()│
     │                             │        │                   │
     │<═════════════════════════════│<──────┘                   │
```

---

## Directory Structure

```
src/
├── agent/              # AgentLoop, compaction, router, task, team, worker
├── bus/                # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── core/               # CoreClient facade and transport adapters
├── client/             # Remote TUI WebSocket client
├── command/            # Slash command registry
├── config/             # Configuration loading, validation, watching
├── crypto/             # AES-256-GCM encryption
├── exec.rs             # Non-interactive exec mode
├── hooks/              # Lifecycle hooks system
├── ide/                # VS Code, JetBrains integration
├── lib.rs              # Module exports
├── main.rs             # Entry point, CLI
├── memory/             # Persistent memory system
├── mcp/                # Model Context Protocol client
├── permission/         # Access control, DoomLoop detection
├── plugin/             # WASM plugin system
├── protocol/           # CoreRequest/CoreResponse, TuiMessage
├── provider/           # LLM provider implementations
├── pty_session/        # Shell session metadata
├── resilience/         # Circuit breaker, FallbackProvider
├── security/           # SSRF protection, Landlock
├── server/             # HTTP server, WebSocket handlers
├── session/            # Session storage, schema, checkpointing
├── skills/             # Skill system
├── snapshot/           # File state capture and restore
├── storage/             # SQLite initialization
├── tool/               # Tool registry, built-in tools (26 tools)
├── tts/                # Text-to-speech
├── tui/                # Terminal UI (app, components, dialogs)
├── upgrade/            # Self-upgrade via GitHub
├── util/               # Clipboard, fuzzy, truncate, metrics
└── worktree/          # Git worktree management
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

---

## Configuration Precedence

Configuration is loaded from (in order of precedence):

1. Environment variables (`CODAGG_*`)
2. Project config (`.codegg/codegg.jsonc`)
3. Global config (`~/.config/codegg/codegg.jsonc`)
4. System config (`/etc/codegg/codegg.json` on Unix, `~/Library/Application Support/codegg/codegg.json` on macOS)

---

## Built-in Agents

| Agent | Mode | Description |
|-------|------|-------------|
| `build` | Primary | Default agent with full permissions |
| `plan` | Primary | Read-only agent for planning |
| `general` | Subagent | Subagent without todo management |
| `explore` | All | Read-only exploration agent |
| `title` | Subagent | Generates session titles (hidden) |
| `summary` | Subagent | Generates session summaries (hidden) |
| `compaction` | Subagent | Context compaction agent (hidden) |

---

## Built-in Tools (26)

| Category | Tools |
|----------|-------|
| **File Operations** | `read`, `write`, `edit`, `glob`, `list` |
| **Search** | `grep`, `codesearch` |
| **Shell** | `bash`, `terminal` |
| **Git** | `git`, `commit`, `diff`, `review` |
| **Code** | `apply_patch`, `replace` |
| **Web** | `webfetch`, `websearch` |
| **Tasks** | `task`, `todo`, `batch`, `plan_enter`, `plan_exit` |
| **Special** | `question`, `skill`, `tool_search`, `invalid` |

---

## LLM Providers (20+)

| Provider | Implementation |
|----------|---------------|
| **Anthropic** | `anthropic.rs` |
| **OpenAI** | `openai.rs` |
| **Google** | `google.rs`, `vertex.rs` |
| **AWS** | `bedrock.rs` |
| **Azure** | `azure.rs` |
| **OpenRouter** | `openrouter.rs` |
| **Cloudflare** | `cloudflare.rs` |
| **GitLab** | `gitlab.rs` |
| **Copilot** | `copilot.rs` |
| **CodeggZen** | `codegg_zen.rs` |
| **Additional** | `additional.rs` (Mistral, Groq, Deepinfra, Cerebras, Cohere, TogetherAI, Perplexity, xAI, Venice, MiniMax, CodeggGo) |

---

## Architecture Files Index

| File | Module | Description |
|------|--------|-------------|
| [agent.md](agent.md) | agent | AgentLoop, compaction, router, task, team, worker |
| [tool.md](tool.md) | tool | Tool registry, built-in tools (26 tools), executor |
| [provider.md](provider.md) | provider | LLM backends (20+), streaming, model catalog |
| [bus.md](bus.md) | bus | GlobalEventBus, AppEvent, registries |
| [permission.md](permission.md) | permission | PermissionChecker, modes, DoomLoop |
| [security.md](security.md) | security | SSRF, IP validation, Landlock |
| [crypto.md](crypto.md) | crypto | AES-256-GCM encryption, key derivation |
| [session.md](session.md) | session | Session storage, stores, checkpointing |
| [storage.md](storage.md) | storage | SQLite initialization, pooling |
| [memory.md](memory.md) | memory | Memory store, consolidation, namespaces |
| [snapshot.md](snapshot.md) | snapshot | File state capture, restore |
| [tui.md](tui.md) | tui | App, components (21 dialogs), input |
| [client.md](client.md) | client | Remote TUI WebSocket client |
| [core.md](core.md) | core | CoreClient facade, transport adapters |
| [server.md](server.md) | server | HTTP server, WebSocket, REST API |
| [mcp.md](mcp.md) | mcp | MCP client, local/remote, OAuth |
| [lsp.md](lsp.md) | lsp | LSP client, diagnostics, operations |
| [ide.md](ide.md) | ide | IDE detection, diff viewing |
| [plugin.md](plugin.md) | plugin | WASM plugins, hooks, TUI extensions |
| [skills.md](skills.md) | skills | Skill system, YAML frontmatter |
| [command.md](command.md) | command | Slash commands, templates |
| [config.md](config.md) | config | Configuration schema, validation |
| [error.md](error.md) | error | AppError enum, error classification |
| [resilience.md](resilience.md) | resilience | Circuit breaker, FallbackProvider |
| [exec.md](exec.md) | exec | Non-interactive exec mode |
| [hooks.md](hooks.md) | hooks | Lifecycle hooks system |
| [pty_session.md](pty_session.md) | pty_session | Shell session metadata |
| [upgrade.md](upgrade.md) | upgrade | Self-upgrade via GitHub |
| [util.md](util.md) | util | Clipboard, fuzzy, truncate |
| [worktree.md](worktree.md) | worktree | Git worktree management |
| [tts.md](tts.md) | tts | Text-to-speech |
| [compaction.md](compaction.md) | compaction | Context window overflow management |
| [protocol.md](protocol.md) | protocol | CoreRequest/CoreResponse, TuiMessage definitions |