# Architecture

## Overview

codegg is a high-performance AI coding agent built in Rust. It uses Tokio for async runtime, SQLx for SQLite database, Ratatui for terminal UI, Axum for HTTP server (feature-gated), and Wasmtime for WASM plugins (feature-gated).

## Module Structure

```
src/
├── agent/              # Main agent loop, message processing, worker pool, task scheduling
│   ├── mod.rs           # Agent struct, builtin_agents(), resolve_agents()
│   ├── loop.rs          # AgentLoop - main execution flow
│   ├── worker.rs        # SubAgentPool, run_subagent_task, bounded concurrency (5)
│   ├── compaction.rs    # Context tracking, auto-compaction, adaptive strategies
│   ├── task.rs          # BackgroundScheduler, task persistence (load/save/update)
│   ├── plan_registry.rs # PlanRegistry for planning tool
│   ├── prompt.rs        # Prompt building, system prompts, agent config application
│   ├── processor.rs     # Message processing utilities
│   ├── mention.rs       # @ mention subagent parsing
│   └── prompts/         # Prompt templates directory
├── bus/                # Event bus system (GlobalEventBus)
│   ├── mod.rs           # GlobalEventBus, PermissionRegistry, QuestionRegistry
│   ├── events.rs        # AppEvent definitions (TextDelta, ToolCallStarted, etc.)
│   └── global.rs        # Global event bus singleton
├── client/             # API client utilities
├── command/            # Slash command registry and routing
├── config/             # Configuration loading and validation
│   ├── mod.rs           # Config struct, loading logic
│   └── schema.rs        # JSON schema definitions (agents, skills, permissions)
├── error.rs            # Central error types (AppError, ToolError, ProviderError, etc.)
├── hooks/              # Hooks system for agent loop lifecycle events
│   └── mod.rs          # HookEvent enum (PreToolExecute, PostToolExecute, SessionStart, etc.)
├── ide/                # IDE integration (VS Code, JetBrains diff viewing)
│   └── mod.rs          # IDE MCP server for openDiff tool
├── lsp/                # Language Server Protocol support
├── mcp/                # MCP (Model Context Protocol) client/server
│   ├── mod.rs           # McpManager, ToolRegistry integration
│   ├── local.rs         # Local MCP server connections (stdio)
│   ├── remote.rs        # Remote MCP with OAuth, DNS rebinding protection
│   ├── ide_server.rs    # IDE MCP server for diff viewing
│   ├── auth.rs          # OAuth flow for MCP
│   └── transport.rs     # MCP transport implementations
├── memory/             # Persistent memory system for session-to-session learning
│   └── mod.rs          # MemoryStore, save() persistence, superseded_by field
├── model/              # Model definitions and flags (if separate from provider)
├── permission/         # Access control and path restrictions
│   ├── mod.rs           # PermissionChecker, PathCache, PATH_CANONICALIZE_CACHE_TTL_SECS=1
│   └── rule.rs         # ToolRule pattern matching (glob patterns like "git *")
├── plugin/             # WASM plugin system with fuel tracking
│   ├── mod.rs           # PluginManager, WASM execution
│   ├── loader.rs        # Module caching with DashMap, mtime-based invalidation
│   ├── marketplace.rs   # Plugin marketplace integration
│   └── service.rs       # PluginService, hook dispatch
├── provider/           # LLM providers — re-export from `crates/codegg-providers`
│   └── (see crates/codegg-providers/src/)  # anthropic, openai, google, azure, bedrock, etc.
├── pty/                # PTY (pseudo-terminal) support
├── resilience/         # Circuit breaker and resilience patterns
│   └── mod.rs          # CircuitBreaker, retry logic
├── server/             # HTTP server (Axum, feature-gated)
│   ├── http.rs          # Route setup, CORS (localhost defaults, never permissive), security headers
│   ├── ws.rs            # WebSocket handler, returns 500 for missing config
│   ├── routes/          # Route handlers (events, auth)
│   └── middleware/      # Auth middleware (applied to /api routes), rate limiting
├── session/            # Session storage and management (split module)
│   ├── mod.rs           # Re-exports, core types
│   ├── store.rs         # SessionStore (68KB), MessageStore, bulk operations
│   ├── models.rs        # Session, Message, SessionRow structs
│   ├── row.rs            # Row conversion utilities
│   ├── import.rs        # Session import/export (Codegg, Claude, etc.)
│   ├── schema.rs        # Database migrations (v1-v12), task table
│   ├── status.rs        # SessionStatus, Analytics
│   ├── message.rs       # Message types, ToolStatus
│   └── checkpoint.rs    # Session checkpointing
├── skills/             # Skill system for agent capabilities
│   └── mod.rs          # Skill loading from markdown files with YAML frontmatter
├── snapshot/           # Snapshot support for file state
├── storage/            # Storage abstractions
├── tts/                # Text-to-speech module
│   └── mod.rs          # Tts struct, TtsEngine trait, Ctrl+Y toggle, Ctrl+Shift+Y stop
├── tool/               # Built-in tools
│   ├── mod.rs           # Tool trait, ToolRegistry, tool definition caching
│   ├── bash.rs          # BashTool, BLOCKED_PATTERNS (46 regex), env_clear(), &&/|| blocking
│   ├── read.rs          # ReadTool, symlink check, unrestricted bypass
│   ├── write.rs         # WriteTool, unrestricted bypass
│   ├── edit.rs          # EditTool, unrestricted bypass
│   ├── replace.rs       # ReplaceTool, unrestricted bypass
│   ├── multiedit.rs     # MultiEditTool, unrestricted bypass
│   ├── apply_patch.rs   # ApplyPatchTool, unrestricted bypass
│   ├── glob.rs          # GlobTool, symlink skipping
│   ├── grep.rs          # GrepTool, symlink skipping
│   ├── list.rs          # ListTool, symlink skipping
│   ├── webfetch.rs      # WebFetchTool, SSRF protection, DNS revalidation
│   ├── websearch.rs     # WebSearchTool
│   ├── codesearch.rs    # CodeSearchTool
│   ├── commit.rs        # CommitTool, LLM-generated commit messages
│   ├── diff.rs          # DiffTool
│   ├── lsp.rs           # LspTool, Language Server Protocol integration
│   ├── task.rs          # TaskTool, TaskStore (in-memory HashMap, no persistence)
│   ├── terminal.rs      # TerminalTool, PTY-based terminal
│   ├── todo.rs          # TodoTool
│   ├── plan.rs          # PlanTool
│   ├── question.rs      # QuestionTool
│   ├── skill.rs         # SkillTool
│   ├── batch.rs         # BatchTool, parallel tool execution
│   ├── git.rs           # Git utilities
│   ├── formatter.rs     # Output formatting
│   ├── executor.rs      # Tool execution utilities
│   ├── invalid.rs       # Invalid tool placeholder
│   └── util.rs          # validate_path(), canonicalize_path(), symlink checking
├── tui/                # Terminal user interface
│   ├── app/              # Main TUI application
│   │   ├── mod.rs         # App struct, TuiCommand enum (17 variants), run_event_loop()
│   │   ├── handlers.rs    # Input handling (107KB, on_key, mouse, dialogs, @ mention completion)
│   │   ├── types.rs       # CompletionType, Dialog, HistoryEntry, SessionStatus, TodoEntry
│   │   └── state/         # UiState, SessionState, and other UI state management
│   ├── components/       # UI widgets
│   │   ├── mod.rs         # Component exports
│   │   ├── messages.rs    # Message display widget (47KB)
│   │   ├── sidebar.rs     # Sidebar widget with tooltips
│   │   ├── prompt.rs      # Prompt input widget
│   │   ├── status_bar.rs   # Bottom status bar (status, transient indicators, token usage)
│   │   ├── dialogs/       # Dialog implementations
│   │   │   ├── mod.rs     # Dialog exports
│   │   │   ├── session.rs # SessionDialog with bulk mode (b key), sort/filter
│   │   │   ├── model.rs   # Model selection dialog (Ctrl+L)
│   │   │   ├── agent.rs   # Agent selection dialog
│   │   │   ├── permission.rs # Permission dialog (Allow/Deny/Ask)
│   │   │   ├── question.rs  # Question dialog
│   │   │   ├── plan.rs      # Plan dialog
│   │   │   ├── goto.rs      # GotoDialog (jump-to-message)
│   │   │   ├── import.rs    # Import dialog
│   │   │   ├── connect.rs   # Connection dialog (MCP)
│   │   │   ├── confirm.rs   # Confirmation dialog
│   │   │   ├── keybind.rs   # Keybind dialog
│   │   │   ├── mcp.rs       # MCP browser dialog
│   │   │   ├── share.rs     # Share dialog
│   │   │   ├── template.rs # Template dialog
│   │   │   ├── theme.rs     # Theme picker dialog
│   │   │   ├── tree.rs      # Tree dialog
│   │   │   └── help_overlay.rs # Help overlay
│   │   ├── completion_overlay.rs # Completion popup (path detection, file/dir icons)
│   │   ├── toast.rs       # Toast notification manager
│   │   ├── spinner.rs     # Spinner widget
│   │   ├── scroll.rs      # Scroll utilities
│   │   └── tool_output.rs # Tool output display
│   ├── input.rs         # Input handling, keybindings (InputAction, KeybindConfig)
│   ├── layout.rs        # Layout configuration (LayoutConfig, TuiLayout)
│   ├── theme.rs         # Theme definitions (uses Arc<Theme> to avoid cloning)
│   └── route.rs         # Route management (Route, RouteManager)
├── upgrade/            # Self-upgrade functionality
├── util/               # Utility functions
│   ├── fuzzy.rs        # Fuzzy matching for search
│   └── time.rs         # Time utilities
```

**Note**: Several modules have been extracted to workspace crates:
- `bus/`, `memory/`, `session/`, `storage/`, `snapshot/`, `worktree/`, `resilience/` → `crates/codegg-core/`
- `config/` → `crates/codegg-config/`
- `provider/` → `crates/codegg-providers/` (re-exported at root as `codegg::provider`)
- `error/` → `crates/codegg-core/src/error.rs` (re-exported at root)
- `lsp/` → `crates/egglsp/` (src/lsp/ is now a thin shim)
- `git/` → `crates/egggit/` (src/git/ removed, src/tool/git.rs remains)

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
- Bash regex uses `LazyLock<Regex>` for pre-compiled patterns (46 BLOCKED_PATTERNS)
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
