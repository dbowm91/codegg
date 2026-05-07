# Tool Module

The `tool` module provides the built-in tools that the agent can use to interact with the filesystem, shell, and external services.

## Overview

**Location**: `src/tool/`

**Key Responsibilities**:
- Tool registry management
- Built-in tool implementations (35+ tools)
- Tool execution with permission checking
- Parameter validation

## Tool Trait

All tools implement the `Tool` trait:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> &Value;
    fn execute(&self, params: Value, context: ToolContext) -> impl Future<Output = ToolResult>;
}
```

### ToolContext

Passed to every tool execution:

```rust
pub struct ToolContext {
    pub session_id: String,
    pub workspace_dir: PathBuf,
    pub agent_id: String,
    pub permission_checker: Arc<PermissionChecker>,
    pub event_bus: GlobalEventBus,
}
```

### ToolResult

```rust
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    pub error: Option<String>,
    pub metadata: Option<Value>,
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

### Shell Execution

| Tool | File | Description |
|------|------|-------------|
| **bash** | `bash.rs` | Execute shell commands |
| **terminal** | `terminal.rs` | Terminal operations |

### Code Operations

| Tool | File | Description |
|------|------|-------------|
| **diff** | `diff.rs` | Show file differences |
| **replace** | `replace.rs` | Find and replace |
| **multiedit** | `multiedit.rs` | Multiple edits in one operation |
| **codesearch** | `codesearch.rs` | Advanced code search |
| **review** | `review.rs` | Code review |
| **commit** | `commit.rs` | Generate commit messages |

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
| **plan** | `plan.rs` | Plan mode detection |

### External Integrations

| Tool | File | Description |
|------|------|-------------|
| **git** | `git.rs` | Git operations |
| **lsp** | `lsp.rs` | LSP tool wrapper |
| **question** | `question.rs` | Ask user questions |
| **skill** | `skill.rs` | Activate skills |

### Utility Tools

| Tool | File | Description |
|------|------|-------------|
| **batch** | `batch.rs` | Batch operations |
| **tool_search** | `tool_search.rs` | On-demand tool discovery |

## ToolRegistry

Manages registration and lookup of tools:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    pub fn register(&mut self, tool: impl Tool);
    pub fn list(&self) -> Vec<ToolDefinition>;
}
```

**Note**: Tool definitions are cached with versioned keys for proper invalidation.

## Tool Execution Flow

```
AgentLoop
├── Provider sends ToolCall event
├── ToolRegistry::get(tool_name)
│   └── Tool::execute(params, context)
│       ├── PermissionChecker::check()
│       ├── Snapshot capture (for file-modifying tools)
│       └── Execute tool logic
└── Return ToolResult
```

## Permission Checking

Tools call `PermissionChecker::check()` before execution:

```rust
impl Tool for BashTool {
    async fn execute(&self, params: Value, context: ToolContext) -> ToolResult {
        // Check permission first
        context.permission_checker.check(
            ToolRequest {
                tool_name: "bash".to_string(),
                params: params.clone(),
                path_rules: self.path_rules.clone(),
            }
        ).await?;

        // Proceed with execution
    }
}
```

## Path Validation

For file operations, tools validate paths:

```rust
pub fn validate_path(path: &Path, rules: &[PathRule]) -> Result<(), ToolError>;
```

- Checks against permission rulesets
- Validates symlinks for security
- Ensures paths are within allowed directories

## Security Considerations

1. **Tool path validation** - All file paths validated before access
2. **Permission enforcement** - PermissionChecker gates all tool execution
3. **Snapshot before modify** - File state captured before destructive operations
4. **SSRF protection** - WebFetch validates URLs against internal IP ranges

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
2. **Path validation**: Uses `security::validate_path_safety()` for symlink protection

## See Also

- [agent.md](agent.md) - Uses ToolRegistry for tool execution
- [permission.md](permission.md) - Permission checking before execution
- [snapshot.md](snapshot.md) - File state capture before modifications
- [security.md](security.md) - SSRF and path validation
