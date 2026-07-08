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
│  │  │ Session│ │Memory │ │ │ MCP   │ │Plugins│ │Native  │       │  │
│  │  └────────┘ └───────┘ │ └───────┘ └──────┘ │ Crates │       │  │
│  │                                         ┌─── egglsp        │  │
│  │                                         │    egggit        │  │
│  │                                         │    eggsentry        │  │
│  │                                         │    eggcontext    │  │
│  │                                         └──────────────────┘  │
│  └──────────────────────┴───────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## Native Tool Crates (Workspace)

Codegg follows a **library-first, MCP-second** tool architecture (see
`plans/native_tool_crates.md`). Durable tool domains live in workspace
crates under `crates/` and are consumed directly in-process by Codegg's
tool wrappers. The same crates can later expose optional MCP adapter
binaries without changing the model-facing tool names.

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `crates/codegg-config` | Configuration schema, paths, loading, validation, watching | `schema.rs`, `paths.rs`, `watcher.rs` |
| `crates/codegg-protocol` | Core protocol types (CoreRequest, CoreResponse, CoreEvent, TuiMessage) | `core.rs`, `tui.rs` |
| `crates/codegg-providers` | LLM provider implementations, auth types, CircuitBreaker | `provider/mod.rs`, `auth/`, `circuit.rs` |
| `crates/egglsp` | Language Server Protocol client/server management | `service.rs`, `client.rs`, `operations.rs`, `server.rs` |
| `crates/egggit` | Read-only git facts (status, diff, changed files, worktrees) | `status.rs`, `diff.rs`, `worktree.rs` |
| `crates/eggsentry` | Deterministic security scanning (secrets, commands, deps, unsafe code) | `scanner.rs`, `command.rs`, `finding.rs`, `profile.rs` |
| `crates/eggcontext` | Token counting and context utilities (tiktoken-based) | `lib.rs` |

Codegg-side thin wrappers (`src/tool/lsp.rs`, `src/tool/git.rs`,
`src/tool/security.rs`, etc.) consume these crates. The model-facing
tool names (`lsp`, `git`, `security`, ...) and JSON schemas are
preserved exactly. Conversion between Codegg's `crate::config::schema`
types and the crates' local config types is one-way: crates never
import Codegg config types.

The native `websearch` and `webfetch` wrappers follow the same
pattern via `src/search_backend/`, with the external `eggsearch` MCP
server as the default backend (raw `mcp__eggsearch__*` tools are
hidden by default — see [MCP](mcp.md)).

## Core Protocol

- **Protocol Version**: 1 (defined in `crates/codegg-protocol/src/core.rs`)
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
| [config/](config.md) | Configuration loading, validation, file watching — in `crates/codegg-config` | `schema.rs`, `paths.rs`, `watcher.rs` |
| [context/](context-ledger.md) | Token counting and context utilities (tiktoken) — in `crates/eggcontext` | `lib.rs` |
| [core/](core.md) | Core facade, transport adapters, request handling | `mod.rs`, `transport/` |
| [crypto/](crypto.md) | AES-256-GCM encryption, Argon2id key derivation | `mod.rs` |
| [error/](error.md) | Centralized AppError enum with error classification | `mod.rs` |
| [exec/](exec.md) | Non-interactive exec mode for CI/CD with JSON I/O | `exec.rs` |
| [hooks/](hooks.md) | Lifecycle hooks for agent events | `mod.rs` |
| [ide/](ide.md) | VS Code / JetBrains detection and diff viewing | `mod.rs` |
| [lsp/](lsp.md) | LSP wrapper (implementation in `crates/egglsp`) | `mod.rs` (re-exports) |
| [mcp/](mcp.md) | Model Context Protocol client (local/remote) | `local.rs`, `remote.rs`, `auth.rs` |
| [memory/](memory.md) | Persistent memory across sessions | `mod.rs` |
| [model_profile/](model_profile_task_state.md) | Model behavioral profiles and task state policy — in `crates/codegg-core` | `types.rs`, `resolve.rs`, `policy.rs` |
| [permission/](permission.md) | Access control, DoomLoop detection, mode system | `mod.rs`, `modes.rs` |
| [plugin/](plugin.md) | WASM plugin system with hooks and fuel tracking | `loader.rs`, `service.rs`, `manifest.rs` |
| [provider/](provider.md) | LLM providers — in `crates/codegg-providers` | `mod.rs`, `anthropic.rs`, `fallback.rs` |
| [protocol/](protocol.md) | Shared request/response envelopes — in `crates/codegg-protocol` | `core.rs`, `tui.rs` |
| [research/](research.md) | Structured research pipeline: source collection → evidence → claims → verification | `coordinator.rs`, `types.rs`, `store.rs`, `claims.rs`, `verify.rs` |
| [resilience/](resilience.md) | Circuit breaker, retry mechanisms | `circuit.rs` |
| [search/](search_backend.md) | Web search and fetch tools + 7 eggsearch wrappers (builtin + MCP backend) | `mod.rs`, `websearch.rs`, `webfetch.rs` |
| [security/](security.md) | SSRF protection, Landlock sandboxing; scanning core in `crates/eggsentry` | `ssrf.rs`, `sandbox.rs` |
| [server/](server.md) | HTTP/WebSocket server for remote TUI | `http.rs`, `ws.rs`, `routes/` |
| [session/](session.md) | SQLite session storage, message history | `store.rs`, `schema.rs`, `message.rs` |
| [shell/](human_shell.md) | Human shell commands, projection pipeline (Phases 1–10), policy evaluation | `mod.rs`, `runtime.rs`, `projector.rs`, `redactor.rs`, `rtk.rs` |
| [shell_session/](shell_session.md) | Shell session metadata (no PTY) | `mod.rs` |
| [skills/](skills.md) | Runtime skill loader and activation | `mod.rs` |
| [snapshot/](snapshot.md) | File state capture and restore | `mod.rs` |
| [storage/](storage.md) | SQLite initialization and connection pooling | `mod.rs` |
| [task_state/](model_profile_task_state.md) | Todo/task state machine, injection, and projection | `mod.rs` |
| [theme/](theme.md) | Frontend-neutral theme system (SemanticTheme → ratatui) | `schema.rs`, `registry.rs`, `native.rs`, `halloy.rs`, `target/` |
| [tool/](tool.md) | Built-in tools (~38 tools in default registry) and backend abstractions | `mod.rs`, `backend.rs`, `bash.rs`, `read.rs`, etc. |
| [deterministic_tools/](deterministic_tools.md) | Eggsact in-process deterministic tools (8 always-visible + 5 deferred) | `deterministic.rs`, `eggsact/adapter.rs` |
| [tts/](tts.md) | Text-to-speech (macOS `say` command) | `mod.rs` |
| [tui/](tui.md) | Terminal user interface (Ratatui) | `app/mod.rs`, `components/` |
| [upgrade/](upgrade.md) | Self-upgrade via GitHub releases | `mod.rs` |
| [util/](util.md) | Clipboard, fuzzy search, pricing, metrics | `mod.rs` |
| [worktree/](worktree.md) | Git worktree management (read-only facts in `crates/egggit`) | `mod.rs` |

## Key Types

### Agent Loop
- `AgentLoop` - Main execution cycle in `agent/loop.rs`
- `Agent` - Agent definition with mode (Primary/Subagent/All)
- 9 built-in agents: build, plan, general, explore, title, summary, compaction, security-review, research

### Tools
- `Tool` trait - All tools implement `name()`, `description()`, `parameters()`, `execute()`
- Optional `execute_structured()` (default impl wraps `execute()`) — see `src/tool/backend.rs`
- ~38 built-in tools in default registry (bash, read, edit, write, glob, grep, task, webfetch, etc.)
- `ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`)
- `ToolRegistry::with_options(ToolRegistryOptions)` is the authoritative registration sequence; `with_defaults()` and the two session constructors `with_session_config_defaults(&Config, ...)` / `with_session_defaults(...)` are thin wrappers (production session code uses the config-aware one to preserve `[tool_backends]`)
- `Tool::expose_in_definitions()` (default `true`, overridden to `false` by `DisabledTool`) is the model-facing predicate; `ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()` both filter through it
- `ToolRegistry::execute_capture(name, input, ctx)` is the central execution path for native tool calls in the agent loop; `AgentLoop::build_tool_execution_context(tc, timeout_ms)` builds the `ToolExecutionContext` and `AgentLoop::resolve_native_backend(name)` resolves the `ToolBackendKind` (`Native` for most tools, `Mcp` for `websearch`/`webfetch` when `[search].backend = eggsearch`, `BuiltinLegacy` otherwise)

### Tool Backends
- `ToolBackendKind` — `Native | Mcp | Shell | BuiltinLegacy`
- `ToolExecutionContext` — request-scoped execution context (cwd, session_id, permission_mode, timeout)
- `ToolProvenance` — `backend`, `implementation`, `version`, `elapsed_ms`, `truncated`, `trust`
- `ToolTrust` — `LocalTrusted | LocalUntrusted | ExternalUntrusted | MutatingSideEffect`
- `StructuredToolResult` — wraps output with provenance; `legacy()` helper preserves the string contract for older tools
- Diagnostics: `/tool-backends` (aliases `/tools`, `/backends`) renders a table of tool → backend → implementation → status → raw MCP exposure
- Config: `[tool_backends.<domain>]` sections with `backend`, `expose_raw_mcp_tools`, `fallback_to_native`, `server_name`, `command`, `args`, `timeout_ms`, `env` (see `config::schema::ExternalToolBackendConfigSchema`)

### Events
- `AppEvent` enum - 42 variants for session, tool, MCP, permission, subagent, goal events
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
| Tools (default registry) | ~38 | `tool/mod.rs:with_options()` |
| LSP servers | 39 | `crates/egglsp/src/server.rs` |
| Native tool crates | 9 | `crates/` (codegg-core, codegg-config, codegg-protocol, codegg-providers, egglsp, egggit, eggsentry, eggcontext, egglsp-test-server) |
| UiState fields | 30 | `tui/app/state/ui.rs:40-98` |
| AppEvent variants | 42 | `crates/codegg-core/src/bus/events.rs:61-265` |
| Built-in commands | 105 | `tui/command.rs` (assertion at line 517) |
| Built-in agents | 9 | `agent/mod.rs:154-423` |

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
- [Context](context-ledger.md) - Token counting and context utilities
- [Core](core.md) - CoreClient facade and transports
- [Crypto](crypto.md) - API key encryption
- [Error](error.md) - Centralized error handling
- [Exec](exec.md) - Non-interactive execution mode
- [Git](git.md) - Git session management
- [Hooks](hooks.md) - Lifecycle hooks
- [Human Shell](human_shell.md) - Shell command execution and projection pipeline
- [IDE](ide.md) - VS Code/JetBrains integration
- [LSP](lsp.md) - Language Server Protocol
- [MCP](mcp.md) - Model Context Protocol
- [Memory](memory.md) - Persistent memory system
- [Model Profile & Task State](model_profile_task_state.md) - Model behavioral profiles, todo/task state machine
- [Native Crates](native_crates.md) — Workspace crates (egglsp, egggit, eggsentry, eggcontext, codegg-config, codegg-protocol, codegg-providers), backend contract, raw MCP exposure policy, diagnostics
- [Permission](permission.md) - Access control and modes
- [Plugin](plugin.md) - WASM plugin system
- [Protocol](protocol.md) - Shared request/response envelopes
- [Provider](provider.md) - LLM provider implementations
- [Research](research.md) - Structured research pipeline
- [Resilience](resilience.md) - Circuit breaker patterns
- [Security](security.md) - SSRF, sandboxing
- [Server](server.md) - HTTP/WebSocket for remote TUI
- [Session](session.md) - SQLite storage and message history
- [Shell Session](shell_session.md) - Shell session metadata
- [Skills](skills.md) - Runtime skill loader
- [Snapshot](snapshot.md) - File state capture and restore
- [Storage](storage.md) - SQLite initialization
- [Theme](theme.md) - Frontend-neutral theme system
- [Deterministic Tools](deterministic_tools.md) — Eggsact in-process deterministic utilities (text comparison, config validation, security inspection)
- [Tool](tool.md) - Tool system and registry
- [TTS](tts.md) - Text-to-speech
- [TUI](tui.md) - Terminal user interface
- [Upgrade](upgrade.md) - Self-upgrade functionality
- [Util](util.md) - Utility functions
- [Worktree](worktree.md) - Git worktree management
