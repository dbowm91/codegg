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

## Tool Trait

All tools implement the `Tool` trait defined at `src/tool/mod.rs:54-60`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
}
```

**Important Notes**:
- Tools receive only `serde_json::Value` as input (no `ToolContext` struct)
- `ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`) - a common oversight
- Every `Tool` reports a `ToolCategory` via `fn category(&self) -> ToolCategory { ReadOnly }` (with a default of `ReadOnly`)

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
| **lsp** | `lsp.rs` | Query LSP server for code intelligence (defined in `src/tool/lsp.rs`, registered in `with_defaults()` at lines 161-163) |

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

Manages registration and lookup of tools at `src/tool/mod.rs:70-73`:

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
| `with_defaults()` | Create registry with all 27 built-in tools |
| `register(&mut self, tool: impl Tool + 'static)` | Register a tool (takes `&dyn Tool` not `Box<dyn Tool>`) |
| `get(&self, name: &str) -> Option<&dyn Tool>` | Get tool by name |
| `list(&self) -> Vec<&dyn Tool>` | List all tools |
| `filter_out(&mut self, denied_tools: &[String])` | Remove denied tools from registry |
| `definitions(&self) -> Vec<ToolDefinition>` | Get tool definitions for LLM |
| `catalog(&self) -> &ToolCatalog` | Access the tool catalog |

### Registration in with_defaults() (lines 89-119)

```rust
pub fn with_defaults() -> Self {
    let mut registry = Self::new();
    registry.register(crate::tool::bash::BashTool::default());
    registry.register(crate::tool::read::ReadTool::default());
    registry.register(crate::tool::edit::EditTool::default());
    registry.register(crate::tool::write::WriteTool::default());
    registry.register(crate::tool::glob::GlobTool::default());
    registry.register(crate::tool::grep::GrepTool::default());
    registry.register(crate::tool::list::ListTool::default());
    registry.register(crate::tool::task::TaskTool::default());
    registry.register(crate::tool::webfetch::WebFetchTool::default());
    registry.register(crate::tool::websearch::WebSearchTool::default());
    registry.register(crate::tool::image::ImageTool::default());
    registry.register(crate::tool::codesearch::CodeSearchTool);
    registry.register(crate::tool::question::QuestionTool);
    registry.register(crate::tool::todo::TodoTool::default());
    registry.register(crate::tool::skill::SkillTool);
    registry.register(crate::tool::apply_patch::ApplyPatchTool::new());
    registry.register(crate::tool::diff::DiffTool::default());
    registry.register(crate::tool::replace::ReplaceTool::default());
    registry.register(crate::tool::review::ReviewTool::default());
    registry.register(crate::tool::batch::BatchTool::default());
    registry.register(crate::tool::terminal::TerminalTool::default());
    registry.register(crate::tool::git::GitTool::default());
    registry.register(crate::tool::lsp::LspTool::new(Arc::new(
        crate::lsp::service::LspService::new(crate::config::schema::LspConfig::default()),
    )));
    registry.register(crate::tool::commit::CommitTool::new());
    registry.register(crate::tool::plan::PlanEnterTool);
    registry.register(crate::tool::plan::PlanExitTool);
    registry.register(crate::tool::invalid::InvalidTool);
    // Register tool_search with catalog for on-demand tool discovery
    let search_tool = crate::tool::tool_search::ToolSearchTool::new(Arc::new(registry.catalog().clone()));
    registry.register(search_tool);
    registry
}
```

Note: ImageTool IS registered in `with_defaults()` at line 102.

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

## File Structure Summary

```
src/tool/
├── mod.rs          # Tool trait, ToolRegistry, with_defaults() (27 tools)
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
├── task.rs         # Subagent task spawning
├── todo.rs         # Todo list management
├── webfetch.rs     # URL content fetching
├── websearch.rs    # Web search via Exa
├── codesearch.rs   # Code search via Exa
├── question.rs     # User question asking
├── skill.rs        # Skill loading
├── review.rs       # LLM-based code review
├── batch.rs        # Parallel tool execution
├── terminal.rs     # Terminal command execution
├── git.rs          # Git command execution
├── commit.rs       # LLM-generated commit messages
├── plan.rs         # plan_enter and plan_exit tools
├── invalid.rs      # Malformed call handler
├── multiedit.rs    # Multi-edit tool (NOT registered)
├── image.rs        # DALL-E image generation
├── tool_search.rs  # On-demand tool discovery
├── lsp.rs          # LSP client tools
├── teams.rs        # Team operation tools
├── formatter.rs     # Auto-formatting support
└── ...
```

## See Also

- [agent.md](agent.md) - Uses ToolRegistry for tool execution
- [permission.md](permission.md) - Permission checking before execution
- [snapshot.md](snapshot.md) - File state capture before modifications
- [security.md](security.md) - SSRF and path validation
- [subagent.md](../.opencode/skills/subagent/SKILL.md) - SubAgentPool and TaskStore for task tool
