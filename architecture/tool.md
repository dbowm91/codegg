# Tool Module

The `tool` module provides the built-in tools that the agent can use to interact with the filesystem, shell, and external services.

## Overview

**Location**: `src/tool/`

**Key Responsibilities**:
- Tool registry management
- Built-in tool implementations (33+ tools)
- Tool execution with permission checking
- Parameter validation

## Tool Trait

All tools implement the `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
}
```

**Note**: Unlike the earlier architecture draft, tools do NOT receive a `ToolContext` struct. They receive only `serde_json::Value` as input.

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

## Built-in Tools

### File Operations

| Tool | File | Description |
|------|------|-------------|
| **read** | `read.rs` | Read file contents |
| **write** | `write.rs` | Write content to file |
| **edit** | `edit.rs` | Make targeted edits to files |
| **glob** | `glob.rs` | Find files by pattern |
| **grep** | `grep.rs` | Search file contents |
| **list** | `list.rs` | List directory contents |
| **diff** | `diff.rs` | Show file differences |
| **replace** | `replace.rs` | Find and replace |
| **multiedit** | `multiedit.rs` | Multiple edits in one operation |
| **apply_patch** | `apply_patch.rs` | Apply patches |

### Shell Execution

| Tool | File | Description |
|------|------|-------------|
| **bash** | `bash.rs` | Execute shell commands |
| **terminal** | `terminal.rs` | Terminal operations |
| **git** | `git.rs` | Git operations |
| **commit** | `commit.rs` | Generate commit messages |

### Code Operations

| Tool | File | Description |
|------|------|-------------|
| **codesearch** | `codesearch.rs` | Advanced code search |
| **review** | `review.rs` | Code review |
| **lsp** | `lsp.rs` | LSP tool wrapper |

### Web Operations

| Tool | File | Description |
|------|------|-------------|
| **webfetch** | `webfetch.rs` | Fetch web page content |
| **websearch** | `websearch.rs` | Search the web |

### Task Management

| Tool | File | Description |
|------|------|-------------|
| **task** | `task.rs` | Execute subagent task |
| **todo** | `todo.rs` | Todo list management |
| **plan_enter** | `plan.rs` | Enter plan mode |
| **plan_exit** | `plan.rs` | Exit plan mode |

### External Integrations

| Tool | File | Description |
|------|------|-------------|
| **question** | `question.rs` | Ask user questions |
| **skill** | `skill.rs` | Activate skills |
| **batch** | `batch.rs` | Batch operations |
| **tool_search** | `tool_search.rs` | On-demand tool discovery |

## ToolRegistry

Manages registration and lookup of tools:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: ToolCatalog,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn with_defaults() -> Self;
    pub fn register(&mut self, tool: impl Tool + 'static);
    pub fn get(&self, name: &str) -> Option<&dyn Tool>;
    pub fn list(&self) -> Vec<&dyn Tool>;
    pub fn definitions(&self) -> Vec<ToolDefinition>;
    pub fn filter_out(&mut self, denied_tools: &[String]);
    pub fn catalog(&self) -> &ToolCatalog;
}
```

### ToolCatalog

The catalog maintains metadata and supports deferred loading:

```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}

impl ToolCatalog {
    pub fn register(&mut self, tool: &dyn Tool);
    pub fn search(&self, query: &str) -> Vec<&ToolMetadata>;
    pub fn get(&self, name: &str) -> Option<&ToolMetadata>;
    pub fn list(&self) -> Vec<&ToolMetadata>;
}
```

## Tool Execution Flow

```
AgentLoop
├── Provider sends ToolCall event
├── ToolRegistry::get(tool_name)
│   └── Tool::execute(input)
│       ├── Path validation (for file tools)
│       └── Execute tool logic
└── Return Result<String, ToolError>
```

## ToolExecutor

Provides retry logic with exponential backoff for transient errors:

```rust
pub struct ToolExecutor {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}

impl ToolExecutor {
    pub async fn execute_with_retry<F, Fut>(&self, f: F) -> Result<Value, ToolError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Value, ToolError>>,
    {
        // Exponential backoff with jitter for Io, Network, Timeout errors
    }
}
```

## ToolError

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

## Path Validation

For file operations, tools use `validate_path` from `util.rs`:

```rust
pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let canonical = canonicalize_path_internal(path)?;
    let root_canonical = allowed_root.canonicalize()?;
    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(...));
    }
    Ok(canonical)
}
```

- Checks against allowed root directory
- Validates symlinks for security (rejects paths containing symlinks)
- Ensures paths are within allowed directories

## Security Considerations

1. **Tool path validation** - All file paths validated before access
2. **Symlink protection** - `check_path_for_symlinks()` rejects paths containing symlinks
3. **Permission enforcement** - Tools check permissions before execution
4. **Snapshot before modify** - File state captured before destructive operations
5. **SSRF protection** - WebFetch validates URLs against internal IP ranges
6. **BashTool blocked patterns** - Regex-based detection of dangerous commands

## Configuration

Related config:

```toml
[tools]
allowed = ["bash", "read", "edit", "glob", "grep"]
denied = ["delete"]

[tools.path_rules]
allowed_paths = ["/home/**", "/workspace/**"]
denied_paths = ["/etc/**", "/root/**"]
```

## Known Implementation Notes

1. **Tool definition caching**: Cache key includes version for proper invalidation
2. **Plan tools split**: `plan_enter` and `plan_exit` are separate tools, not one `plan` tool
3. **ToolCatalog for metadata**: The catalog tracks tool metadata separately from registry

## See Also

- [agent.md](agent.md) - Uses ToolRegistry for tool execution
- [permission.md](permission.md) - Permission checking before execution
- [snapshot.md](snapshot.md) - File state capture before modifications
- [security.md](security.md) - SSRF and path validation