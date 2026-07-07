# Architecture

## Overview

codegg is a high-performance AI coding agent built in Rust. It uses Tokio for async runtime, SQLx for SQLite database, Ratatui for terminal UI, Axum for HTTP server (feature-gated), and Wasmtime for WASM plugins (feature-gated).

## Module Structure

### Workspace Crates (`crates/`)

Core logic is extracted into workspace crates. The root `src/` re-exports many of these.

```
crates/
├── codegg-config/       # Configuration schema, paths, loading, validation, file watching
├── codegg-core/         # Domain types: bus, error, goal, memory, session, storage, snapshot, worktree, resilience
│   ├── src/bus/         #   GlobalEventBus, PermissionRegistry, QuestionRegistry (42 AppEvent variants)
│   ├── src/error.rs     #   AppError, ToolError, ProviderError, is_retryable
│   ├── src/goal/        #   Goal tracking and management
│   ├── src/memory/      #   Persistent memory for session-to-session learning
│   ├── src/session/     #   Session storage, MessageStore, schema migrations (v1-v12)
│   ├── src/storage/     #   SQLite storage abstractions
│   ├── src/snapshot/    #   File state capture and restore
│   ├── src/worktree/    #   Git worktree management
│   └── src/resilience/  #   CircuitBreaker, retry logic
├── codegg-protocol/     # Core protocol types (CoreRequest, CoreResponse, CoreEvent, TuiMessage)
├── codegg-providers/    # LLM provider implementations, auth types, CircuitBreaker (16+ providers)
├── eggsentry/           # Security scanning (secrets, commands, dependency auditing)
├── eggcontext/          # Token counting and context utilities
├── egggit/              # Read-only git facts (status, diff, changed files)
├── egglsp/              # Language Server Protocol client/service/operations (39 servers)
└── egglsp-test-server/  # Fake LSP server binary for integration tests
```

### Application (`src/`)

Root `src/` is the application layer: agent loop, TUI, tools, server, auth, plugins.

```
src/
├── agent/               # Agent loop, compaction, routing, team coordination
│   ├── mod.rs           # Agent struct, builtin_agents(), resolve_agents(), AgentRegistry
│   ├── loop.rs          # AgentLoop (~49 fields) - main execution flow
│   ├── loop_factory.rs  # AgentLoopFactory - build-only seam
│   ├── worker.rs        # SubAgentPool, run_subagent_task, bounded concurrency (5)
│   ├── compaction.rs    # Context tracking, auto-compaction, adaptive strategies
│   ├── task.rs          # BackgroundScheduler, task persistence (load/save/update)
│   ├── plan_registry.rs # PlanRegistry for planning tool
│   ├── prompt.rs        # Prompt building, system prompts, agent config application
│   ├── processor.rs     # Message processing utilities
│   ├── mention.rs       # @ mention subagent parsing
│   ├── router.rs        # Model auto-routing by task complexity
│   ├── team.rs          # Multi-agent teams via file-based inbox
│   └── prompts/         # Prompt templates (default.txt, anthropic.txt, etc.)
├── auth/                # AuthConfig, Credential, AuthResolver (re-exports from codegg-providers)
├── command/             # Slash command registry and routing (105 built-in commands)
├── error.rs             # Central error types + AxumAppError (feature-gated)
├── hooks/               # Hooks system for agent loop lifecycle events
├── ide/                 # IDE integration (VS Code, JetBrains diff viewing)
├── lsp/                 # LSP shim (authoritative impl in crates/egglsp)
├── mcp/                 # MCP (Model Context Protocol) client/server
├── permission/          # Access control, DoomLoop detection, mode system
│   ├── mod.rs           # PermissionChecker, PathCache
│   └── rule.rs          # ToolRule pattern matching (glob patterns)
├── plugin/              # Plugin system with WASM/process/builtin runtimes
│   ├── registry.rs      # PluginRegistry, capability indexing
│   ├── management.rs    # PluginManager (install/enable/disable/remove)
│   ├── lifecycle.rs     # Plugin lifecycle management
│   ├── service.rs       # PluginService, hook dispatch
│   ├── policy.rs        # PluginPolicy (lifecycle, UI, permission, install, runtime)
│   ├── builtin/         # Built-in plugins (poe, gitlab, copilot, codex)
│   └── runtime/         # ProcessRuntime, WasmRuntime, BuiltinRuntime
├── provider/            # LLM providers — re-export from crates/codegg-providers
├── security/            # SSRF protection, Landlock sandboxing, security review workflow
├── server/              # HTTP server (Axum, feature-gated behind `server` feature)
│   ├── http.rs          # Route setup, CORS (localhost defaults), security headers
│   ├── ws.rs            # WebSocket handler
│   ├── routes/          # Route handlers (events, auth)
│   └── middleware/      # Auth middleware, rate limiting
├── session/             # Session storage — re-export from codegg-core
├── shell/               # Human shell (! commands), projection pipeline, RTK integration
├── skills/              # Skill system — re-export from codegg-config
├── snapshot/            # File state capture — re-export from codegg-core
├── storage/             # Storage abstractions — re-export from codegg-core
├── tool/                # ~30 built-in tools and backend abstractions
│   ├── mod.rs           # Tool trait, ToolRegistry, tool definition caching
│   ├── bash.rs          # BashTool, BLOCKED_PATTERNS, env_clear()
│   ├── read.rs          # ReadTool, symlink check
│   ├── write.rs         # WriteTool
│   ├── edit.rs          # EditTool
│   ├── apply_patch.rs   # ApplyPatchTool
│   ├── glob.rs          # GlobTool
│   ├── grep.rs          # GrepTool
│   ├── webfetch.rs      # WebFetchTool, SSRF protection
│   ├── lsp.rs           # LspTool, Language Server Protocol integration
│   ├── terminal.rs      # TerminalTool, PTY-based terminal
│   └── ...              # ~20 more tools (commit, diff, task, todo, plan, etc.)
├── tui/                 # Terminal user interface
│   ├── app/             # Main TUI application (~13K lines)
│   │   ├── mod.rs       # App struct, event handling, state management
│   │   ├── types.rs     # Dialog, CompletionType, HistoryEntry, SessionStatus
│   │   └── state/       # UiState, SessionState, UI state management
│   ├── commands/        # TuiCommand dispatch handlers (12+ submodules)
│   ├── components/      # UI widgets (messages, sidebar, prompt, dialogs/, etc.)
│   └── runtime/         # Async command dispatch, event loop, render recovery
├── upgrade/             # Self-upgrade functionality
└── util/                # Utility functions (clipboard, fuzzy matching, pricing, metrics)
```

## Key Architectural Patterns

### Error Handling

- Uses `thiserror` for error types with `#[derive(Error)]`
- Error types defined in `src/error.rs` with variants for each module
- Pattern: `AppError::Module(ErrorType::Variant(message))`
- `AppError` implements `axum::response::IntoResponse` (feature-gated with `server` feature)
- `SessionSummaryProvider` trait uses `AppError` (not `anyhow::Error`)

### Async/Await

- All async functions use `async_trait` for trait object safety
- Use `tokio::spawn` for detached tasks with shutdown signaling via `tokio::sync::broadcast`
- Prefer `tokio::sync::broadcast` for task shutdown signaling
- PermissionChecker uses `tokio::sync::RwLock` (not parking_lot) for async-compatible locking
- PermissionRegistry uses `tokio::sync::Mutex` for async-compatible locking

### State Management

- `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for shared state
- `parking_lot` mutexes for sync locks (faster than tokio's)
- `dashmap` for concurrent HashMap operations
- `Arc<Theme>` in TUI components to avoid cloning on every `set_theme()` call

### Database

- SQLx with compile-time checked queries
- SQLite for session persistence
- Connection pooling via `SqlitePool` (default max 10 connections)
- Use `SESSION_COLUMNS` and `SESSION_COLUMNS_QUALIFIED` constants for column lists
- Batch operations with `QueryBuilder::push_values()`
- Session table has `time_deleted` column for soft deletes with 30-second undo window
- Transactions must use `&mut *tx` (not `&self.pool`) to prevent connection pool exhaustion

## Agent Loop

The main loop in `src/agent/loop.rs` handles:

1. Get user input
2. Build chat request with context (auto-compact if needed)
3. Stream response from provider (SSE, 1MB buffer limit)
4. Process tool calls with permission checks (via PermissionChecker)
5. Handle question tool prompts via QuestionRegistry
6. Update session

AgentLoop integrates with TUI via `GlobalEventBus::publish()` emitting `AppEvent`s.

### Tool Definition Caching

AgentLoop caches tool definitions when model, plan_mode, lsp_enabled, mcp_tool_count, and permission_version haven't changed. ModelFlags are pre-computed once per tool build.

### Auto-Compaction

`src/agent/compaction.rs` implements adaptive compaction strategies:
- `auto_compact()` selects strategy based on message characteristics
- `ContextTracker` monitors token usage
- Defaults to `false` in config (opt-in)

## Event-Driven TUI

The TUI uses `tokio::select!` with `GlobalEventBus` subscription in `run_event_loop()`:

```rust
tokio::select! {
    biased;
    Some(result) = reader.next() => { /* keyboard/mouse */ }
    Ok(event) = bus_rx.recv() => {
        match event {
            AppEvent::TextDelta { delta, .. } => { /* display */ }
            AppEvent::ToolCallStarted { .. } => { /* display */ }
            AppEvent::PermissionPending { .. } => { /* show dialog */ }
            AppEvent::QuestionPending { .. } => { /* show dialog */ }
            AppEvent::SubagentStarted { .. } => { /* show progress */ }
            AppEvent::SubagentCompleted { .. } => { /* show result */ }
            AppEvent::CompactionTriggered { .. } => { /* show notification */ }
        }
    }
    Some(cmd) = cmd_rx.recv() => { /* handle TuiCommand */ }
}
```

### Remote TUI Protocol

Fully implemented. `handle_remote_event` in `src/tui/app/mod.rs` processes events during the event loop. Events flow from WebSocket to `event_rx` channel, processed via select branch alongside keyboard input.

## Permission System

Path-based restrictions with three outcomes (defined in `src/permission/mod.rs`):

- **Allow**: Tool executes immediately
- **Deny**: Tool returns error to LLM
- **Ask**: Dialog shown, execution pauses until user response (300s timeout)

**Flow:**
1. `AgentLoop` calls `permission_checker.check(tool, path)` (async, must await)
2. If `Ask`, publishes `PermissionPending` via GlobalEventBus
3. Registers with `PermissionRegistry` and waits
4. TUI shows permission dialog
5. User responds → `PermissionRegistry::respond(perm_id, choice)`
6. `AgentLoop` resumes based on user's choice

**Path Cache TTL:** 1 second (`PATH_CANONICALIZE_CACHE_TTL_SECS`), not 30 seconds.

**Session Isolation:** `PersistentDecision` has `session_id` field for session-specific permissions.

## TuiCommand Pattern

TUI uses a command channel (`tui_cmd_tx: Option<mpsc::Sender<TuiCommand>>`) for async operations from synchronous event handlers:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TuiCommand {
    // Representative subset — actual enum has 88+ variants (src/tui/app/mod.rs)
    DeleteSession { session_id: String },
    ArchiveSession { session_id: String, unarchive: bool },
    UndoDelete { session_id: String },
    ForkSession { session_id: String },
    ShareSession { session_id: String },
    BulkDelete { session_ids: Vec<String> },
    BulkArchive { session_ids: Vec<String>, unarchive: bool },
    BulkExport { session_ids: Vec<String> },
    ReloadSessions,
    OpenTreeDialog,
    PreviewImport { source: ImportSource },
    ConfirmImport { source: ImportSource },
    CreateFromTemplate { template_id: String, session_id: String },
    LoadSessionMessages { session_id: String },
    SpawnSubagent { agent_name: String, prompt: String },
    // ... 70+ more variants for git, LSP, plugins, tasks, etc.
}
```

**Pattern:** Send command to async handler instead of blocking event loop:

```rust
// ✅ Good - send command to async handler
fn handle_fork_session(&mut self) {
    if let Some(ref tx) = self.tui_cmd_tx {
        let _ = tx.try_send(TuiCommand::ForkSession { session_id });
    }
}
```

## Feature Flags

- `server`: HTTP server support (Axum), WebSocket events
- `plugins`: WASM plugin support (Wasmtime), ModuleCache with mtime invalidation
- `debug-logging`: File-based debug logging to `codegg_debug.log`

## Performance Considerations

- 1MB streaming buffer limit per provider (`MAX_BUFFER_SIZE`) to prevent unbounded memory growth
- 60s request timeout, 10s connect timeout for HTTP clients (`create_http_client()`)
- Path canonicalization caching with 1-second TTL (`PATH_CANONICALIZE_CACHE_TTL_SECS`)
- DoomLoopDetector uses HashMap for O(1) loop detection, normalizes tool names (lowercase, trimmed)
- Theme uses `Arc<Theme>` to avoid cloning on every `set_theme()` call
- SSE parser uses `drain()` and indices to reduce allocations
- Bash regex uses `once_cell::sync::Lazy<Vec<...>>` for pre-compiled patterns (52 BLOCKED_PATTERNS)
- Session read-after-write uses `RETURNING` clause instead of UPDATE+SELECT
- Batch inserts use `QueryBuilder::push_values()`
- HTTP pool tuning: `pool_max_idle_per_host(32)`, `pool_idle_timeout(30s)`, `tcp_keepalive(30s)`
- WASM per-plugin fuel tracking uses `DashMap<String, AtomicU64>`
- `estimate_tokens_sync` model param is unused (always called with `None`)
- Parallel tool calls: SSE parser queues tool calls using internal buffer

## Security

- SSRF protection with internal IP blocking (IPv4-mapped IPv6 handled via `is_internal_ip()`)
- Bash environment isolation via `env_clear()`, minimal safe PATH (`/usr/local/bin:/usr/bin:/bin`)
- Multi-word blocked command detection (`rm -rf`, `&&`, `||`, standalone `&`)
- Rate limiting on WebSocket endpoints keyed by actual peer address (not attacker-controlled headers)
- Path validation with `validate_path()` in `src/tool/util.rs`
- Symlink rejection: walk tools (list, grep, glob) skip symlinks; direct file tools (read, write, edit, etc.) do NOT check symlinks unless `unrestricted=false`
- MCP remote DNS rebinding protection (validate at connection + before each request)
- HTTP security headers (X-Content-Type-Options, X-Frame-Options, HSTS) in `src/server/http.rs`
- CORS uses localhost defaults when `cors_origins` is empty, never permissive fallback
- WebFetch uses DNS revalidation before each request to prevent rebinding attacks
- `time_deleted` column for soft deletes with 30-second undo window
- Write tools have `unrestricted` parameter that bypasses `validate_path` when `true`

## Keyboard Shortcuts

### Session Dialog
- **'b'** - Toggle bulk mode for multi-session operations (delete, archive, export)
- **Space** - Select/deselect sessions in bulk mode
- **Ctrl+A** - Select all sessions in bulk mode

### Other Shortcuts
- **Ctrl+L** - Open model selection dialog
- **Ctrl+Y** - Toggle TTS on/off
- **Ctrl+Shift+Y** - Stop TTS playback
- **@** - Mention subagent in prompt
- **/model** or **/models** - Change model via command

## Agent Configuration

Agents can be defined via:
- **JSON config**: `~/.config/codegg/config.json` or project `.codegg/config.json`
- **Markdown files**: `~/.config/codegg/agents/*.md` or `.codegg/agents/*.md`

Markdown agent format:
```markdown
---
description: Code review agent
mode: subagent
temperature: 0.1
color: "#ff6b6b"
steps: 50
---

You are a code review agent. Focus on code quality.
```

Agent struct fields: `top_p: Option<f64>`, `color: Option<String>`, `steps: Option<usize>`.

## Skill System

Skills provide specialized capabilities loaded from markdown files with YAML frontmatter.

**Location:**
- `~/.config/codegg/skills/` - User skills
- `.codegg/skills/` - Project skills

**Format:**
```markdown
---
name: code-review
description: Performs thorough code reviews
version: "1.0"
tags: [review, quality]
permission:
  bash: ask
  write: allow
---

You are a code review agent specialized in finding bugs.
```

**Activation:** Use `/skill:code-review` during a session.

**Permission:** Skill tool permissions checked via config schema `permission.skill` at AgentLoop level.

## SubAgent System

SubAgentPool (`src/agent/worker.rs`):
- Bounded concurrency limit: 5
- Uses `parent_id` reference for session sharing
- `run_subagent_task()` runs real AgentLoop instead of returning formatted string
- TUI handles SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed events
- Invocation via `@agent_name` mention in prompt (CompletionOverlay handles syntax)

## Provider System

Provider implementations (`src/provider/`):
- All use SSE streaming with 1MB buffer limits
- HTTP clients have 60s timeout + 10s connect timeout
- Detect HTTP 429 and return `ProviderError::RateLimit`
- Exponential backoff with MAX_RETRY_DELAY of 30s cap
- Retry on RateLimit, Timeout, and Stream errors
- Tool definition adapters: `ToolDefinition::to_openai()` and `ToolDefinition::to_anthropic()`
- Google provider uses `Uuid::new_v4()` for unique tool call IDs
- Anthropic `message_delta` emits Finish events
- Ollama uses text-to-tool fallback via `text_tool_parser.rs`

## Session Storage (Split Module)

Session module is split into focused files:
- `store.rs` (68KB) - SessionStore, MessageStore, all CRUD operations, transactions
- `models.rs` - Session, Message, SessionRow structs
- `row.rs` - Row conversion utilities
- `import.rs` - Session import/export (Codegg, Claude formats)
- `schema.rs` - Database migrations (v1-v12), task table creation
- `status.rs` - SessionStatus, Analytics
- `message.rs` - Message types, ToolStatus enum
- `checkpoint.rs` - Session checkpointing
- `mod.rs` - Re-exports

**Transaction Pattern:** All operations within a transaction use `&mut *tx` (not `&self.pool`). Example: `fork()` uses transaction correctly for all operations (lines 941-1148 in store.rs).

## Debugging

### Tracing
- Use `tracing` for structured logging
- Set `RUST_LOG=debug` or use `-v` / `-vv` / `-vvv` CLI flags
- `tracing_subscriber::fmt()` initializes logging in `main.rs`

### Direct File Logging for TUI Debugging
When debugging TUI issues (key events, state mutations, rendering):

```rust
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("debug.log")
        {
            let _ = writeln!(file, "[MODULE-DEBUG] {}", format!($($arg)*));
        }
    };
}
```

Enabled via `debug-logging` feature flag. Writes to `codegg_debug.log`.
