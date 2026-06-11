# Tool Module

The `tool` module provides the built-in tools that the agent can use to interact with the filesystem, shell, and external services.

## Overview

**Location**: `src/tool/`

**Key Responsibilities**:
- Tool registry management
- Built-in tool implementations (27 tools in `with_defaults()`)
- Tool execution with permission checking
- Parameter validation
- On-demand tool discovery via ToolCatalog
- Backend abstraction (native, MCP, shell, builtin legacy) — see `src/tool/backend.rs` and `architecture/native_crates.md`

## Tool Trait

All tools implement the `Tool` trait defined at `src/tool/mod.rs:85-108`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;

    fn category(&self) -> ToolCategory { ToolCategory::Mutating }
    fn set_available_tools(&mut self, _tools: Vec<String>) {}
    fn defer_loading(&self) -> bool { false }
    /// Whether this tool should appear in the model-facing tool
    /// definitions (default `true`). Overridden by `DisabledTool`
    /// to `false` so hidden stubs do not pollute the model tool
    /// surface.
    fn expose_in_definitions(&self) -> bool { true }

    // Optional structured execution — default wraps `execute()`.
    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> { ... }
}
```

**Important Notes**:
- Tools receive only `serde_json::Value` as input (no `ToolContext` struct) by default
- `ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`) - a common oversight
- Every `Tool` reports a `ToolCategory` via `fn category(&self) -> ToolCategory` (default `Mutating`)
- `execute_structured` is opt-in — new wrappers may use it; existing tools keep the default impl
- `expose_in_definitions` is opt-out — hidden stubs (`DisabledTool` for `disabled` or `mcp + fallback_to_native = false`) override it to `false` and rely on the registry/agent loop filtering step to keep them out of the model-facing catalog while remaining callable by name for diagnostics

### ToolCategory

Defined in `src/tool/mod.rs`, the category drives permission gating and
which tools survive `filter_tools_for_model()` (plan mode):

```rust
pub enum ToolCategory {
    ReadOnly,       // never prompts (read, glob, grep, list, webfetch, lsp, diff, plan_*, ...)
    SafeMutating,   // never prompts (todowrite, todoread, question, invalid)
    Mutating,       // normal Ask/Allow path (edit, write, apply_patch, replace, image, terminal, git, commit, review, task, ...)
    ShellExec,      // routed to destructive-pattern fallback (bash, ...)
}

impl ToolCategory {
    pub fn is_permission_free(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::SafeMutating)
    }
}
```

The lookup helper `tool_category_for_name()` in `src/permission/mod.rs`
maps a tool name to a category for the permission checker, falling back
to `Mutating` for unknown tools. This means the permission flow
short-circuits to `Allow` for read-only / safe-mutating tools before
any store / rule / glob check (a persistent `Deny` still wins), and
shell-exec tools get the destructive-pattern fallback described in
[permission.md](permission.md#toolcategory--permission-free-tools).

### ToolResult

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}
```

## Built-in Tools (29 total in default registry)

### File Operations

| Tool | File | Description |
|------|------|-------------|
| **read** | `read.rs` | Read file contents with line numbers. Images/PDFs returned as base64. Supports offset/limit. |
| **write** | `write.rs` | Create or overwrite files. Runs auto-formatting after write if configured. |
| **edit** | `edit.rs` | Surgically search and replace text with 8 matching strategies (exact, line-trimmed, whitespace-normalized, block-anchored, indentation-flexible, escape-normalized, trimmed-boundary, context-aware). |
| **glob** | `glob.rs` | Find files matching glob patterns. Uses `ignore` crate for gitignore compliance. |
| **grep** | `grep.rs` | Search file contents using regular expressions with context lines. Concurrent search with semaphore limiting (max 100 concurrent). |
| **list** | `list.rs` | List directory tree with ignore patterns. Limited to 300 files by default. |
| **diff** | `diff.rs` | Show differences between two file versions. Supports unified diff format and line ranges. |
| **replace** | `replace.rs` | Find and replace using regex. Replaces all occurrences by default. Supports capture groups ($1, $2). |
| **apply_patch** | `apply_patch.rs` | Apply unified diff patches. Supports update, create, delete, and move modes. |

### Shell Execution

| Tool | File | Description |
|------|------|-------------|
| **bash** | `bash.rs` | Execute shell commands with extensive security (blocked commands, blocked patterns regex, allowlist support, Landlock sandboxing). 120s default timeout. |
| **terminal** | `terminal.rs` | Run commands in interactive terminal session. Similar security to bash but with env var filtering. 60s default timeout. |
| **git** | `git.rs` | Execute git commands with subcommand/args model. 30s default timeout. |
| **commit** | `commit.rs` | Generate commit messages from diff using LLM. Stages all changes, generates message, commits with optional Co-Authored-By. |

### Web Operations

| Tool | File | Description |
|------|------|-------------|
| **webfetch** | `webfetch.rs` | Native wrapper. Dispatches to the configured backend via `search_backend::dispatch_web_fetch`. Default backend is the external `eggsearch` MCP server's `web_fetch` tool; legacy reqwest/html2text implementation is retained as the `builtin` fallback. |
| **websearch** | `websearch.rs` | Native wrapper. Dispatches to the configured backend via `search_backend::dispatch_web_search`. Default backend is the external `eggsearch` MCP server's `web_search` tool; the in-tree `SearchProviderRegistry` is the `builtin` fallback. |
| **codesearch** | `codesearch.rs` | Search for code examples, library docs, SDK patterns using Exa Code API. Uses EXA_API_KEY or EXA_CODE_API_KEY. |
| **research** | `research.rs` | Deep research tool. May invoke `websearch` and `webfetch` internally. |
| **image** | `image.rs` | Generate images using OpenAI's DALL-E model. Supports dall-e-3, size, quality parameters. Requires OPENAI_API_KEY. |

`websearch` and `webfetch` always present the stable native tool
names to the model. The raw `mcp__eggsearch__*` tools are hidden
from the model by default (`expose_raw_mcp_tools = false`). Set
that flag to `true` to expose them. See
[`search_backend/`](../.opencode/skills/search_backend/SKILL.md) for
the dispatch logic, config schema, and trust framing.

### Task Management

| Tool | File | Description |
|------|------|-------------|
| **task** | `task.rs` | Spawn subagents to handle tasks independently. Supports spawn/get actions. Uses TaskStore for persistence. |
| **todowrite** | `todo.rs` | Create, update, and manage todo items with persistent state. Supports priority (low/medium/high) and status (pending/in_progress/completed). |

### Planning

| Tool | File | Description |
|------|------|-------------|
| **plan_enter** | `plan.rs` | Enter plan mode. Toolset is reduced to read-only + `todowrite` + `bash`; bash is auto-rejected unless it matches the destructive-pattern allowlist (only safe commands). |
| **plan_exit** | `plan.rs` | Exit plan mode and switch to build agent. Optionally specify plan file. |

### User Interaction

| Tool | File | Description |
|------|------|-------------|
| **question** | `question.rs` | Ask user clarifying questions. Returns answers to continue agent loop. Supports options and initial values. |
| **skill** | `skill.rs` | Load a skill (SKILL.md) by name into context. Returns skill content and list of resource files. |

### Code Operations

| Tool | File | Description |
|------|------|-------------|
| **review** | `review.rs` | Analyze git diff and provide structured code review feedback using LLM. Uses emojis for categorization (bug, performance, style, suggestion). |
| **lsp** | `lsp.rs` | Query LSP server for code intelligence and preview-only semantic edits. Operations: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, diagnostics, renamePreview, formatPreview. Previews return `WorkspaceEditPreview` (unified diff patches + hashes + `patch_omitted` flag); previews are read-only — actual mutation stays in the mutating `apply_patch` tool. `lsp` tool is always `ToolCategory::ReadOnly`. |

### Security Operations

| Tool | File | Description |
|------|------|-------------|
| **security** | `security.rs` | Analyze code for security vulnerabilities. Checks for SQL injection, XSS, command injection, path traversal, and other common security issues. |

### Meta Operations

| Tool | File | Description |
|------|------|-------------|
| **batch** | `batch.rs` | Execute up to 25 tool calls in parallel. Each call limited to 100KB input, total output limited to 500KB. |
| **tool_search** | `tool_search.rs` | On-demand tool discovery. Searches catalog by name/description. Registered with catalog (not as a regular tool). |
| **invalid** | `invalid.rs` | Catch-all for malformed tool calls. Returns tool name and error message. |

## NOT Registered (exists but excluded from default registry)

**multiedit** (`src/tool/multiedit.rs`):
- Module exists and is registered via `pub mod multiedit` in `mod.rs`
- NOT included in `ToolRegistry::with_defaults()`
- Applies multiple edit operations to a single file sequentially
- Uses same path validation as other file tools

To register multiedit, add to `with_defaults()`:
```rust
registry.register(crate::tool::multiedit::MultiEditTool::default());
```

## ToolRegistry

Manages registration and lookup of tools at `src/tool/mod.rs:118-121`:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
}
```

### Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty registry |
| `with_options(ToolRegistryOptions)` | Authoritative registration sequence; the other constructors are thin wrappers |
| `with_defaults()` | Create registry with all 27 built-in tools, all-native backend defaults |
| `with_session_config_defaults(&Config, todo_state, policy, pool, session_id)` | **Production session constructor.** Resolves `ToolBackendConfig::from_config(&Config)` and threads it through `with_options`, so resolved `[tool_backends]` config (LSP/security backends, MCP fallback) is preserved. |
| `with_session_defaults(todo_state, policy, pool, session_id)` | Session registry with **all-native backend defaults** — drops any loaded `[tool_backends]`. Kept for tests and non-config-aware callers; the doc comment warns against using it in production paths. |
| `register(&mut self, tool: impl Tool + 'static)` | Register a tool (takes `&dyn Tool` not `Box<dyn Tool>`) |
| `get(&self, name: &str) -> Option<&dyn Tool>` | Get tool by name (includes hidden stubs) |
| `list(&self) -> Vec<&dyn Tool>` | List all tools (includes hidden stubs) |
| `filter_out(&mut self, denied_tools: &[String])` | Remove denied tools from registry |
| `definitions(&self) -> Vec<ToolDefinition>` | Get tool definitions for LLM — filters via `Tool::expose_in_definitions()` so `DisabledTool` stubs are hidden |
| `catalog(&self) -> &ToolCatalog` | Access the tool catalog |
| `set_search_mode(&mut self, mode)` | Set tool catalog search mode |
| `register_deferred_names(&mut self, names)` | Register names of tools that load on-demand |
| `set_search_tool_available_tools(&mut self, available)` | Inject available tool names into `tool_search` |
| `execute_capture(name, input, ctx) -> StructuredToolResult` | Central execution path used by `AgentLoop::execute_tool_calls` for native tools. Returns structured provenance; the model-facing `structured.output` matches the legacy `execute()` string. |
| `tool_backends()` | Resolved `ToolBackendConfig` captured at construction |
| `backend_report(mcp_server_names)` | Runtime-aware status report for `/tool-backends` (Active / FallbackToNative / Disabled / ConfiguredButUnavailable) |

### ToolRegistryOptions (Phase 2)

Centralizes all knobs that influence registration:

```rust
pub struct ToolRegistryOptions {
    pub todo_state: Option<Arc<Mutex<TodoState>>>,
    pub todo_policy: Option<TaskStatePolicy>,
    pub pool: Option<SqlitePool>,
    pub session_id: Option<String>,
    pub lsp_service: Option<Arc<LspService>>,
    pub tool_backends: ToolBackendConfig,
}
```

Both `with_defaults()` and `with_session_*_defaults(...)` build a
`ToolRegistryOptions` and delegate to `with_options()`. LSP service
construction is now injectable instead of hardcoded in two places.

### Native tool execution path

Native tool wrappers (e.g. `lsp`, `security`, `git`, `review`) call
into the corresponding workspace crate (`egglsp`, `eggsentry`, `egggit`)
for actual work. Crate local config types are converted from Codegg's
`crate::config::schema::*` types at the bridge site. See
`architecture/native_crates.md` for the full boundary, public APIs,
and provenance model.

The central execution path for native tools in
`AgentLoop::execute_tool_calls` (`src/agent/loop.rs`) is
`ToolRegistry::execute_capture(name, input, ctx)`. It calls
`Tool::execute_structured()` internally, populates a fallback
`ToolProvenance::legacy(...)` for tools that do not override it, and
records provenance via `tracing::debug!` (backend, implementation,
elapsed_ms). The returned `StructuredToolResult` is collapsed to
`structured.output` for the model — the model-facing string is
identical to the legacy `execute()` path. MCP tools
(`mcp__server__tool`) continue to dispatch through
`McpService::call_tool` and are not funnelled through
`execute_capture`.

The `ToolExecutionContext` passed to `execute_capture` is built by the
small helper `AgentLoop::build_tool_execution_context(tc, timeout_ms)`
(`src/agent/loop.rs`). It fills in `session_id`, `cwd`, `timeout_ms`,
and the resolved `ToolBackendKind`. Backend resolution is delegated
to `AgentLoop::resolve_native_backend(name)`: most tools resolve to
`Native`, while `websearch` / `webfetch` resolve to `Mcp` when
`[search].backend = eggsearch` and to `BuiltinLegacy` for the
`builtin` or `disabled` configurations. After the call returns, the
dispatcher emits a `tracing::debug!` line summarising the
`ToolProvenance` (backend, implementation, elapsed_ms, trust) so the
structured metadata stays internal and never reaches the model.

Regression coverage:

- `tests/tool_structured_execution.rs` — locks down the
  `ToolRegistry::execute_capture` contract (provenance shape,
  disabled/MCP-fallback semantics, definition visibility).
- `tests/agent_loop_harness.rs::test_live_dispatcher_uses_execute_capture`
  — proves the live agent-loop dispatcher routes native calls through
  `execute_capture`. The mock tool overrides `execute_structured`
  to record the call; if the dispatcher ever bypassed the structured
  path the recording would not fire and the test would fail.
- `tests/agent_loop_harness.rs::test_live_dispatcher_model_output_shape_is_plain_string`
  — locks down the model-facing `Message::Tool` content: it must
  match the raw tool output string and contain no provenance
  envelope (`provenance`, `backend`, `implementation`, `trust`,
  `elapsed_ms`).

### `expose_in_definitions` filtering

`Tool::expose_in_definitions()` (default `true`) is the model-facing
predicate. `DisabledTool` overrides it to `false`, so
`ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()`
both filter the tool out of the model-visible catalog. The stub
remains registered and callable by name so:

- `/tool-backends` and `ToolRegistry::backend_report(...)` can
  introspect the disabled/MCP-stub state.
- Tests can call the stub to assert the error message.
- The disabled reason remains in the registry for diagnostics.

Because both `definitions()` and `build_tool_definitions()` apply the
same predicate, the model's view of the tool surface and
`tool_search`'s view stay in lockstep: disabled/MCP-stub tools are
never advertised.

## ToolCatalog

Provides metadata management and search at `src/tool/catalog.rs:32-40`:

```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}
```

### ToolMetadata

```rust
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,
}
```

### Catalog Methods

| Method | Description |
|--------|-------------|
| `register(&mut self, tool: &dyn Tool)` | Register tool metadata (takes reference, not owned) |
| `search(&self, query: &str) -> Vec<&ToolMetadata>` | Search by name or description (case-insensitive) |
| `get(&self, name: &str) -> Option<&ToolMetadata>` | Get metadata by name |
| `list(&self) -> Vec<&ToolMetadata>` | List all metadata |
| `deferred_tools(&self) -> Vec<&ToolMetadata>` | List tools marked for deferred loading |
| `is_deferred(&self, name: &str) -> bool` | Check if tool is deferred |

## Tool Execution Flow

```
AgentLoop
├── Provider sends ToolCall event
├── ToolRegistry::get(tool_name)
│   └── tool.execute(input)
│       ├── Parameter extraction
│       ├── Path validation (for file tools)
│       ├── Permission checking
│       └── Execute tool logic
└── Return Result<String, ToolError>
```

### Execution Details by Tool Type

**File Tools** (read, write, edit, glob, grep, list, diff, replace, apply_patch):
1. Extract path from input JSON
2. Call `validate_path()` or `canonicalize_path()` from `util.rs`
3. Check symlinks with `check_path_for_symlinks()`
4. Perform operation in `tokio::task::spawn_blocking()`
5. Publish `AppEvent::FileChanged` for mutations

**Shell Tools** (bash, terminal):
1. Extract command from input
2. Check against `BLOCKED_PATTERN` regex
3. Check against `blocked_commands` HashSet
4. Validate allowlist if configured
5. Execute via `tokio::process::Command`
6. Apply output truncation (2000 lines, 50KB default)

**Web Tools** (webfetch, websearch, codesearch, image):
1. `websearch`/`webfetch` dispatch to the configured backend via
   `search_backend`. With the default `eggsearch` backend, SSRF
   protection is delegated to the eggsearch subprocess; with the
   `builtin` backend, `tool::webfetch::execute_builtin` runs the
   steps below.
2. `image` (and the legacy `builtin` webfetch) parse the URL,
   call `validate_host_ip()` for SSRF protection, then
   `revalidate_dns()` to verify DNS, then make the HTTP request
   with appropriate headers and process the response (markdown
   for HTML, base64 for images).

**Subagent Tools** (task):
1. Create task in TaskStore
2. Send to SubAgentSpawner
3. Return task_id for later retrieval via `action=get`

## Path Validation

All file operations use utility functions from `src/tool/util.rs`:

### validate_path (for restricted tools)

```rust
pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let canonical = canonicalize_path_internal(path)?;
    let root_canonical = allowed_root.canonicalize()?;
    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }
    Ok(canonical)
}
```

### canonicalize_path (for unrestricted tools)

```rust
pub fn canonicalize_path(path: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    canonicalize_path_internal(path)
}
```

### check_path_for_symlinks

```rust
pub fn check_path_for_symlinks(path: &Path) -> Result<(), ToolError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if current.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false) {
            return Err(ToolError::Permission(format!(
                "symlink not allowed in path: {}",
                current.display()
            )));
        }
    }
    Ok(())
}
```

### Key Validation Points

- **Symlinks rejected**: Paths containing symlinks are rejected
- **Allowed root enforcement**: File tools restrict operations to within `allowed_root`
- **spawn_blocking for I/O**: All file I/O happens in `tokio::task::spawn_blocking()` to avoid blocking the async runtime
- **Absolute path handling**: Relative paths are joined with allowed_root before validation

## ToolError

Defined in `src/error.rs`, used by all tools:

```rust
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
    #[error("tool timeout: {0}")]
    Timeout(String),
    #[error("permission denied: {0}")]
    Permission(String),
    #[error("tool formatting failed: {0}")]
    Format(String),
    #[error("tool disabled: {0}")]
    Disabled(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("network error: {0}")]
    Network(String),
}

impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_))
    }
}
```

## Security Considerations

1. **Tool path validation**: All file paths validated before access
2. **Symlink protection**: `check_path_for_symlinks()` rejects paths containing symlinks
3. **Permission enforcement**: Tools check permissions before execution
4. **BashTool blocked patterns**: Regex-based detection of 40+ dangerous command patterns
5. **BashTool blocked commands**: HashSet of full commands that are blocked (rm -rf /, mkfs, etc.)
6. **SSRF protection**: WebFetch validates URLs against internal IP ranges
7. **Subprocess PATH**: External processes use `std::env::var_os("PATH")` (not hardcoded)
8. **Environment variable filtering**: TerminalTool filters dangerous env vars (LD_PRELOAD, DYLD_*)
9. **Allowlist support**: BashTool and TerminalTool support command allowlists

## Tool Backend Diagnostics (Phase 10)

`/tool-backends` (aliases `/tools`, `/backends`) surfaces the native vs
MCP wiring of every model-facing tool. The handler builds a
synchronous report from the resolved `ToolBackendConfig` plus any
pre-installed context (e.g. eggsearch availability from
`search_backend::state`) and renders it as a toast. The report shape:

```
Tool         Backend   Implementation    Status       Raw MCP exposed
websearch    MCP       eggsearch          ready        no
webfetch     MCP       eggsearch          ready        no
lsp          Native    egglsp             ready        n/a
security     Native    eggsentry             ready        n/a
git          Native    codegg/egggit      ready        n/a
```

Status values are: `ready`, `disabled`, `unavailable`, `error(<msg>)`.
Warnings are appended when a backend is configured-but-unavailable or
when raw MCP tools are hidden because a native wrapper is active.

See `architecture/native_crates.md` for the underlying contract
(`ToolBackendKind`, `ToolProvenance`, `McpExposurePolicy`).

## File Structure Summary

```
src/tool/
├── mod.rs          # Tool trait, ToolRegistry, with_options() / with_defaults() / with_session_defaults()
├── backend.rs      # ToolBackendKind, ToolProvenance, ToolExecutionContext, StructuredToolResult,
│                   # ToolBackendConfig, build_report() for /tool-backends
├── catalog.rs      # ToolCatalog for metadata and search
├── util.rs         # Path validation helpers
├── bash.rs         # Shell command execution
├── read.rs         # File reading with image/PDF base64 support
├── write.rs        # File writing with auto-formatting
├── edit.rs         # 8-strategy edit matching
├── glob.rs         # Glob pattern file finding
├── grep.rs         # Regex content search
├── list.rs         # Directory tree listing
├── diff.rs         # Unified diff generation
├── replace.rs      # Regex find/replace
├── apply_patch.rs  # Unified diff patch application
├── patch_util.rs   # Shared patch utility functions for apply_patch and LSP preview
├── task.rs         # Subagent task spawning
├── todo.rs         # Todo list management
├── webfetch.rs     # URL content fetching (dispatches to search_backend)
├── websearch.rs    # Web search (dispatches to search_backend)
├── codesearch.rs   # Code search via Exa
├── question.rs     # User question asking
├── skill.rs        # Skill loading
├── review.rs       # LLM-based code review (uses egggit::diff_summary)
├── batch.rs        # Parallel tool execution
├── terminal.rs     # Terminal command execution
├── git.rs          # Git command execution (low-level wrapper)
├── commit.rs       # LLM-generated commit messages
├── plan.rs         # plan_enter and plan_exit tools
├── invalid.rs      # Malformed call handler
├── multiedit.rs    # Multi-edit tool (NOT registered)
├── image.rs        # DALL-E image generation
├── tool_search.rs  # On-demand tool discovery
├── lsp.rs          # LSP client tools (wraps egglsp::LspService)
├── security.rs     # Security scanning (wraps eggsentry)
├── teams.rs        # Team operation tools
├── formatter.rs    # Auto-formatting support
└── ...
```

## See Also

- [agent.md](agent.md) - Uses ToolRegistry for tool execution
- [permission.md](permission.md) - Permission checking before execution
- [snapshot.md](snapshot.md) - File state capture before modifications
- [security.md](security.md) - SSRF and path validation
- [subagent.md](../.opencode/skills/subagent/SKILL.md) - SubAgentPool and TaskStore for task tool
