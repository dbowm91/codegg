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

## System Architecture

The architecture follows a **layered separation** between the TUI frontend, the Core runtime, and the Agent processing engine:

```
┌────────────────────────────────────────────────────────────────────────────────┐
│                                TUI Layer                                        │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │  App (State Machine) │ Components (14) │ Dialogs (20) │ Input Handling  │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
│                                      │                                          │
│                              TuiMessage │ CoreResponse                          │
│                                      │                                          │
├──────────────────────────────────────┼──────────────────────────────────────────┤
│                                Core Layer                                       │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │  CoreClient (facade) │ Transport Adapters │ Protocol Envelopes          │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
│                                      │                                          │
│                                      ▼                                          │
│  ┌──────────────────────────────────────────────────────────────────────────┐  │
│  │                        AgentLoop                                          │  │
│  │  ┌───────────┐  ┌───────────┐  ┌────────────┐  ┌─────────────────────┐   │  │
│  │  │ Provider  │  │    Tool   │  │ Permission │  │   GlobalEventBus    │   │  │
│  │  │ (LLM)     │  │  Registry │  │  Checker   │  │   (pub/sub + regs)  │   │  │
│  │  └───────────┘  └───────────┘  └────────────┘  └─────────────────────┘   │  │
│  │       │              │               │                    ▲              │  │
│  │       │              │               │                    │              │  │
│  │       ▼              ▼               ▼                    │              │  │
│  │  ┌───────────┐  ┌───────────┐  ┌────────────┐  ┌───────────┐ │              │
│  │  │   Hook    │  │  Snapshot │  │    MCP     │  │   LSP     │ │              │
│  │  │  Registry │  │  Manager  │  │   Service  │  │   Client  │ │              │
│  │  └───────────┘  └───────────┘  └────────────┘  └───────────┘ │              │
│  └──────────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                  │
                    ▼                  ▼                  ▼
             ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
             │ LLM Provider│  │ MCP Servers │  │ LSP Servers │
             │ (20+ models)│  │(local/remote)│ │ (44+ langs) │
             └─────────────┘  └─────────────┘  └─────────────┘
```

---

## Module Index

Each module has a dedicated `.md` file in this directory. Click any link to deep dive into that component.

### [Agent Core](agent.md) — `src/agent/`

The heart of the system — handles LLM interactions, tool execution, message processing, context management, and multi-agent coordination.

| Submodule | File | Purpose |
|-----------|------|---------|
| **AgentLoop** | `loop.rs` | Main event loop, streaming, tool execution |
| **Compaction** | `compaction.rs` | Context window overflow management |
| **Router** | `router.rs` | Model auto-routing by task complexity |
| **Processor** | `processor.rs` | Turn processing and response handling |
| **Task** | `task.rs` | Background task scheduling |
| **Worker** | `worker.rs` | SubAgentPool and SubAgentSpawner |
| **Team** | `team.rs` | Single-team coordination |
| **Teams** | `teams.rs` | Multi-team coordination |
| **Prompt** | `prompt.rs` | System prompt construction |
| **Mention** | `mention.rs` | Agent mention/selection handling |
| **Prompts** | `prompts/` | Prompt templates directory |

**Key Interfaces**:
- `AgentLoop::run()` — Main execution loop
- `AgentLoop::run_with_prompt()` — Convenience for simple prompts

**See**: [agent.md](agent.md) for full AgentLoop internals, compaction strategies, router, and team coordination.

---

### [Provider](provider.md) — `src/provider/`

Unified interface for 20+ LLM backends with chat streaming, model discovery, and caching.

| Provider | File | Notes |
|---------|------|-------|
| **Anthropic** | `anthropic.rs` | Claude models via API |
| **OpenAI** | `openai.rs` | GPT-4 and compatible |
| **Google** | `google.rs` | Gemini via API |
| **Vertex** | `vertex.rs` | Google Cloud Vertex |
| **Azure** | `azure.rs` | Azure OpenAI |
| **Bedrock** | `bedrock.rs` | AWS Bedrock |
| **OpenRouter** | `openrouter.rs` | Aggregator |
| **GitHub Copilot** | `copilot.rs` | Via api.ai.com |
| **GitLab** | `gitlab.rs` | GitLab Duo |
| **Cloudflare** | `cloudflare.rs` | Workers AI |
| **CodeggZen** | `codegg_zen.rs` | Internal service |

**Key Types**:
- `Provider` trait — `stream()`, `models()`, `ping()`
- `ChatRequest` / `ChatEvent` — streaming interface
- `ModelInfo` — model metadata

**See**: [provider.md](provider.md) for provider implementations, streaming, model catalog, and caching.

---

### [Tool](tool.md) — `src/tool/`

Tool registry and 33+ built-in tools for file operations, git, search, web, LSP, and more.

| Tool Category | Tools |
|--------------|-------|
| **File I/O** | `read`, `write`, `edit`, `glob`, `replace` |
| **Shell** | `bash`, `terminal` |
| **Git** | `git`, `commit`, `review` |
| **Search** | `grep`, `codesearch`, `websearch`, `webfetch` |
| **Code** | `lsp`, `diff`, `apply_patch`, `multiedit` |
| **Agent** | `task`, `skill`, `plan`, `batch` |
| **Meta** | `question`, `todo`, `tool_search`, `invalid` |

**Key Types**:
- `Tool` trait — `name()`, `description()`, `parameters()`, `execute()`
- `ToolRegistry` — tool registration and lookup
- `ToolResult` — execution result

**See**: [tool.md](tool.md) for tool trait, built-in tool implementations, executor, and catalog.

---

### [Event Bus](bus.md) — `src/bus/`

Inter-component communication through a publish-subscribe event bus, plus lifecycle hooks for extensibility.

| Component | File | Purpose |
|-----------|------|---------|
| **GlobalEventBus** | `global.rs` | Broadcast channel (2048 capacity) for `AppEvent` distribution |
| **AppEvent** | `events.rs` | 36 event variants (session, message, tool, streaming, etc.) |
| **PermissionRegistry** | `mod.rs` (bus/) | Request/response for permission decisions (300s TTL) |
| **QuestionRegistry** | `mod.rs` (bus/) | Request/response for question answers (300s TTL) |

**See**: [bus.md](bus.md) for event types, subscription patterns, and the permission/question registry.

---

### [Core](core.md) — `src/core/`

Request/response facade for separating TUI transport from core session and agent logic.

| Component | Purpose |
|----------|---------|
| **CoreClient** | Trait for `request()` and `subscribe()` |
| **InprocCoreClient** | Local in-process implementation |
| **Transport Adapters** | `inproc`, `stdio`, `socket` for different deployment modes |
| **Protocol** | `CoreRequest`/`CoreResponse` envelopes, `TuiMessage` protocol |

**See**: [core.md](core.md) for the transport split, request families, and protocol envelopes.

---

### [Permission](permission.md) — `src/permission/`

Access control, path restrictions, DoomLoop detection, and mode-based permissions.

| Component | Purpose |
|-----------|---------|
| **PermissionChecker** | Central access control validation |
| **PermissionRuleset** | Tool rules, path rules, default level |
| **DoomLoopDetector** | Repetitive action detection (window-based) |
| **BuiltinModes** | `review`, `debug`, `docs`, `agent` modes |

**See**: [permission.md](permission.md) for permission rulesets, modes, and DoomLoop detection.

---

### [Security](security.md) — `src/security/`

SSRF protection, internal IP validation, symlink detection, and Landlock sandboxing.

| Function | Purpose |
|----------|---------|
| `validate_url_host()` | SSRF protection with IPv4/IPv6 validation |
| `is_internal_ip()` | Block internal IP access |
| `validate_path_safety()` | Symlink traversal protection |
| `request_landlock_sandbox()` | OS-level filesystem sandboxing |

**See**: [security.md](security.md) for SSRF protection and sandboxing.

---

### [Crypto](crypto.md) — `src/crypto/`

AES-256-GCM encryption with Argon2id key derivation for API keys and secrets.

| Feature | Details |
|---------|---------|
| **v2 Format** | `v2:` prefix with Argon2id KDF |
| **Legacy** | HMAC-SHA256 support for old keys |
| **Config** | Automatic encrypt/decrypt of provider API keys |

**See**: [crypto.md](crypto.md) for encryption implementation and key derivation.

---

### [Session](session.md) — `src/session/`

SQLite-backed session storage, message history, checkpointing, import/export, and session sharing.

| Store | Purpose |
|-------|---------|
| **SessionStore** | Session CRUD, soft delete, archive, fork |
| **MessageStore** | Message history with parts |
| **CheckpointStore** | Session checkpointing |
| **SessionShare** | Expiring share tokens |

**See**: [session.md](session.md) for database schema, stores, and checkpointing.

---

### [Storage](storage.md) — `src/storage/`

SQLite database initialization, connection pooling, WAL mode, and pragma configuration.

| Feature | Details |
|---------|---------|
| **Pool** | `SqlitePoolOptions` with 30s acquire timeout |
| **WAL Mode** | Enabled for concurrent reads |
| **Pragmas** | Batched for efficiency |
| **Health** | `health_check()` and `close()` methods |

**See**: [storage.md](storage.md) for SQLite initialization and connection pooling.

---

### [Memory](memory.md) — `src/memory/`

Persistent memory system for session-to-session learning with namespace-based organization.

| Feature | Details |
|---------|---------|
| **Namespaces** | Hierarchical (`user/preferences`, `project/{hash}/conventions`) |
| **Consolidation** | Rule-based pattern detection with importance scoring |
| **Commands** | `/memory`, `/memory-search`, `/memory-remember`, `/memory-forget` |
| **Auto-run** | `experimental.memory_auto_consolidate` option |

**See**: [memory.md](memory.md) for memory store, consolidation, and namespaces.

---

### [Snapshot](snapshot.md) — `src/snapshot/`

File state capture (full/incremental) and restore for rollback safety.

| Feature | Details |
|---------|---------|
| **Full Capture** | Complete project state |
| **Incremental** | Delta-based capture |
| **Restore** | Path traversal protection |
| **Integration** | Pre-file-modification snapshots |

**See**: [snapshot.md](snapshot.md) for file state capture and restore.

---

### [TUI](tui.md) — `src/tui/`

Ratatui-based terminal UI with CoreClient-backed session flows, 21 dialog types, FocusManager, and keyboard shortcuts.

| Component | Purpose |
|-----------|---------|
| **App** | Main state machine |
| **Components** | 13 reusable widgets (messages, prompt, sidebar, etc.) |
| **Dialogs** | 20 modal dialogs (permission, confirm, model, etc.) |
| **Input** | Keyboard handling and keybindings |
| **Route** | State routing and navigation |

**See**: [tui.md](tui.md) for App state machine, components, dialogs, and input handling.

---

### [Client](client.md) — `src/client/`

WebSocket client for remote TUI connections to server with health checking, resume/replay, and protocol handling.

**See**: [client.md](client.md) for remote TUI client implementation.

---

### [Server](server.md) — `src/server/`

Axum-based HTTP server with WebSocket support, REST API, SSE events, rate limiting, and TUI replay buffering.

| Route | Purpose |
|-------|---------|
| `/api/sessions/*` | Session CRUD, fork, archive, share, revert |
| `/api/config` | Configuration management |
| `/api/mcp/*` | MCP server management |
| `/api/event` | SSE event stream |
| `/api/permission/*` | Permission handling |
| `/api/question/*` | Question handling |
| `/api/providers` | Provider listing |
| `/api/tools` | Tool listing |
| `/api/file/*` | File read/write/delete |
| `/api/project/*` | Project management |
| `/api/workspace/*` | Workspace management |
| `/ws/tui` | WebSocket TUI connection |
| `/health` | Health check |

**Note**: 13 route modules in `src/server/routes/` (config, event, file, health, mcp, permission, project, provider, question, session, tool, workspace).

**See**: [server.md](server.md) for HTTP/WebSocket server architecture.

---

### [MCP](mcp.md) — `src/mcp/`

Model Context Protocol client (local via stdio, remote via HTTP) with OAuth support and auto-reconnection.

| Client Type | Transport |
|-------------|-----------|
| **LocalClient** | stdio child process |
| **RemoteClient** | HTTP with OAuth |
| **McpConnectionManager** | Auto-reconnect with exponential backoff |

**See**: [mcp.md](mcp.md) for MCP client implementations, OAuth flow, and auto-reconnection.

---

### [LSP](lsp.md) — `src/lsp/`

Language Server Protocol client for diagnostics, code operations (goto definition, find references, hover, completion).

| Feature | Details |
|---------|---------|
| **Servers** | 44+ pre-configured language servers |
| **Operations** | goto definition, find references, hover, completion |
| **Diagnostics** | Real-time error/warning display |
| **Download** | Automatic server download and update |

**See**: [lsp.md](lsp.md) for LSP client and server management.

---

### [IDE](ide.md) — `src/ide/`

IDE detection (VS Code, JetBrains) and diff viewer integration.

**See**: [ide.md](ide.md) for IDE integration and diff viewing.

---

### [Plugin](plugin.md) — `src/plugin/`

WASM plugin system via Wasmtime with 10 hook types, builtin handlers, and TUI extensions.

| Feature | Details |
|---------|---------|
| **Hooks** | `tool.execute.before/after`, `agent.start/end`, etc. |
| **Builtin** | `copilot`, `gitlab`, `codex`, `poe` handlers |
| **Fuel** | Per-plugin fuel budgets |
| **TUI Extensions** | Custom dialogs and widgets |

**See**: [plugin.md](plugin.md) for WASM hooks, plugin service, and TUI extensions.

---

### [Skills](skills.md) — `src/skills/`

Skill system for specialized capabilities loaded from markdown files with YAML frontmatter.

**See**: [skills.md](skills.md) for skill loading and activation.

---

### [Command](command.md) — `src/command/`

Slash command registry from markdown files with template variable substitution.

**See**: [command.md](command.md) for slash command execution.

---

### [Config](config.md) — `src/config/`

Configuration loading from JSONC files, schema validation, hot-reload via file watching, and env var interpolation.

**See**: [config.md](config.md) for configuration schema and validation.

---

### [Hooks](hooks.md) — `src/hooks/`

Lifecycle hooks system (`PreToolExecute`, `PostToolExecute`, `SessionStart`, `SessionEnd`, etc.) for shell command hooks.

**See**: [hooks.md](hooks.md) for lifecycle hooks system.

---

### [Exec](exec.md) — `src/exec.rs`

Non-interactive exec mode for CI/CD with JSON input/output and structured error classification.

**See**: [exec.md](exec.md) for exec mode.

---

### [Error](error.md) — `src/error.rs`

Centralized `AppError` enum using thiserror with `is_retryable()` methods, `ProviderError`, `ToolError`, and HTTP status mapping.

**See**: [error.md](error.md) for error classification and handling.

---

### [Resilience](resilience.md) — `src/resilience/`

Circuit breaker pattern with state machine (Closed/Open/HalfOpen) and `FallbackProvider` for provider redundancy.

**See**: [resilience.md](resilience.md) for circuit breaker and fallback provider.

---

### [PTY Session](pty_session.md) — `src/pty_session/`

Shell session metadata management (in-memory only, no actual PTY).

**See**: [pty_session.md](pty_session.md) for shell session metadata.

---

### [Worktree](worktree.md) — `src/worktree/`

Git worktree management (list, create, remove) and git root detection.

**See**: [worktree.md](worktree.md) for git worktree management.

---

### [TTS](tts.md) — `src/tts/`

Text-to-speech (macOS only via `say` command).

**See**: [tts.md](tts.md) for text-to-speech.

---

### [Util](util.md) — `src/util/`

Utility functions: clipboard operations, fuzzy string matching, text truncation, and metrics collection.

**See**: [util.md](util.md) for utility functions.

---

### [Upgrade](upgrade.md) — `src/upgrade/`

Self-upgrade functionality via GitHub releases.

**See**: [upgrade.md](upgrade.md) for self-upgrade.

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

## Configuration Precedence

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
├── agent/              # AgentLoop, compaction, router, task, team, worker
├── bus/                # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── core/               # CoreClient facade and transport adapters
├── client/             # Remote TUI WebSocket client
├── command/            # Slash command registry
├── config/             # Configuration loading, validation, watching, schema
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
├── protocol/           # CoreRequest/CoreResponse and TuiMessage protocols
├── provider/           # LLM provider implementations
├── pty_session/        # Shell session metadata
├── resilience/         # Circuit breaker, FallbackProvider
├── security/           # SSRF protection, Landlock
├── server/             # HTTP server, WebSocket handlers
├── session/            # Session storage, schema, checkpointing
├── skills/             # Skill system
├── snapshot/           # File state capture and restore
├── storage/            # SQLite initialization
├── tool/               # Tool registry, built-in tools
├── tts/                # Text-to-speech
├── tui/                # Terminal UI (app, components, dialogs)
├── upgrade/            # Self-upgrade via GitHub
├── util/               # Clipboard, fuzzy, truncate, metrics
└── worktree/           # Git worktree management
```

---

## Architecture Files Index

| File | Module | Description |
|------|--------|-------------|
| [agent.md](agent.md) | agent | AgentLoop, compaction, router, task, team, worker |
| [tool.md](tool.md) | tool | Tool registry, built-in tools, executor |
| [provider.md](provider.md) | provider | LLM backends, streaming, model catalog |
| [bus.md](bus.md) | bus | GlobalEventBus, AppEvent, registries |
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
| [pty_session.md](pty_session.md) | pty_session | Shell session metadata |
| [upgrade.md](upgrade.md) | upgrade | Self-upgrade via GitHub |
| [util.md](util.md) | util | Clipboard, fuzzy, truncate |
| [worktree.md](worktree.md) | worktree | Git worktree management |
| [compaction.md](compaction.md) | agent/compaction | Context overflow strategies |
| [tts.md](tts.md) | tts | Text-to-speech |