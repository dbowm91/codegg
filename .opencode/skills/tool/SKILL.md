---
name: tool
description: Tool trait, registration, execution flow, adding new tools
version: 1.2.0
tags:
  - tool
  - trait
  - registry
  - execution
  - tool definition
---

# Tool System Guide

This skill covers the tool system in opencode-rs, including the Tool trait, registration, and execution flow.

## Architecture Overview

```
ToolRegistry → Tool implementations
    │
    ├── BashTool
    ├── ReadTool
    ├── WriteTool
    ├── EditTool
    ├── GlobTool
    ├── GrepTool
    ├── ListTool
    ├── TaskTool
    ├── WebFetchTool
    ├── WebSearchTool
    └── ... (26 total)
```

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

### Required Methods

1. **name()** - Unique identifier for the tool
2. **description()** - LLM-facing description of what the tool does
3. **parameters()** - JSON schema for tool input parameters
4. **execute()** - Async execution logic

### Example Implementation

```rust
pub struct ReadTool {
    unrestricted: bool,
}

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Reads content from a file at the given path"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path = input["path"].as_str().ok_or_else(|| ToolError::InvalidInput("path required".into()))?;
        // ... read file and return content
    }
}
```

## ToolRegistry

Manages tool registration and lookup:

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

The `ToolCatalog` maintains metadata about tools and supports deferred loading:

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
    pub fn deferred_tools(&self) -> Vec<&ToolMetadata>;
}
```

## Built-in Tools

### File Tools

| Tool | Description |
|------|-------------|
| `read` | Read file contents |
| `write` | Write content to file |
| `edit` | Edit file by finding/replacing content |
| `replace` | Replace content with exact matching |
| `glob` | Find files by glob pattern |
| `grep` | Search file contents |
| `list` | List directory contents |
| `diff` | Show file differences |
| `apply_patch` | Apply patches |

### Shell Tools

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands |
| `terminal` | Execute terminal commands (interactive) |
| `git` | Git operations |
| `commit` | Commit changes to git (generates commit message via LLM) |

### Network Tools

| Tool | Description |
|------|-------------|
| `webfetch` | Fetch web page content |
| `websearch` | Search the web |
| `codesearch` | Search code examples |

### Planning Tools

| Tool | Description |
|------|-------------|
| `plan_enter` | Enter plan mode (read-only, allows plan file editing) |
| `plan_exit` | Exit plan mode and switch to build mode |

Note: These are two separate tools (`PlanEnterTool` and `PlanExitTool`) registered individually.

### Other Tools

| Tool | Description |
|------|-------------|
| `task` | Manage subagent tasks |
| `question` | Ask user questions |
| `todo` | Manage TODO list |
| `skill` | Load and use skills |
| `batch` | Execute multiple operations |

## Extended Tool Modules

These tools require separate registration (not included in `with_defaults()`).

### Team Tools (`src/tool/teams.rs`)

Multi-agent coordination via team-based communication:

```rust
pub struct TeamTools {
    pub team_create: TeamCreateTool,
    pub send_message: SendMessageTool,
    pub list_messages: ListMessagesTool,
    pub team_status: TeamStatusTool,
    pub list_teams: ListTeamsTool,
}
```

| Tool | Description |
|------|-------------|
| `team_create` | Create a new team |
| `send_message` | Send message to a team |
| `list_messages` | List messages in a team |
| `team_status` | Get team status |
| `list_teams` | List all teams |

Register via `TeamTools::register_all()`:
```rust
let team_tools = TeamTools::new(manager, base_dir);
team_tools.register_all(&mut registry);
```

### Multiedit Tool (`src/tool/multiedit.rs`)

Multiple edits in one operation - NOT included in `with_defaults()`:

| Tool | Description |
|------|-------------|
| `multiedit` | Apply multiple file edits atomically |

Register via `MultieditTool::register()`:
```rust
let multiedit = MultieditTool::new();
registry.register(multiedit);
```

### LSP Tool (`src/tool/lsp.rs`)

Language Server Protocol integration for code intelligence:

```rust
pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}
```

| Operation | Description |
|-----------|-------------|
| `goToDefinition` | Jump to symbol definition |
| `findReferences` | Find all references to a symbol |
| `hover` | Get hover information |
| `documentSymbol` | List symbols in a document |
| `workspaceSymbol` | Search symbols across workspace |
| `goToImplementation` | Jump to implementation |
| `prepareCallHierarchy` | Prepare call hierarchy |
| `incomingCalls` | List incoming calls |
| `outgoingCalls` | List outgoing calls |
| `codeAction` | Get code actions |
| `codeLens` | Get code lenses |

Parameters: `operation` (required), `file_path`, `line`, `column`, `end_line`, `end_column`, `symbol`

### Formatter (`src/tool/formatter.rs`)

Code formatting via external formatters (not a Tool, used internally):

```rust
pub struct Formatter {
    rules: HashMap<String, FormatterRule>,
}
```

| Method | Description |
|--------|-------------|
| `format_file(path)` | Run formatter on file |
| `has_rule(ext)` | Check if formatter exists for extension |

Configured via `FormatterConfig` with rules for each file extension. Uses user's actual PATH when spawning formatter process.

## Adding a New Tool

### 1. Create Tool File

Create `src/tool/mytool.rs`:

```rust
use async_trait::async_trait;
use serde_json::json;

use crate::error::ToolError;

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &str {
        "mytool"
    }

    fn description(&self) -> &str {
        "Description of what mytool does"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "param1": {
                    "type": "string",
                    "description": "First parameter"
                }
            },
            "required": ["param1"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let param1 = input["param1"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("param1 required".into()))?;
        
        // Tool implementation
        Ok(format!("result: {}", param1))
    }
}
```

### 2. Register Tool Module

In `src/tool/mod.rs`, add:

```rust
pub mod mytool;
```

### 3. Register in ToolRegistry

In `src/tool/mod.rs`, `with_defaults()`:

```rust
pub fn with_defaults() -> Self {
    let mut registry = Self::new();
    // ... existing tools
    registry.register(crate::tool::mytool::MyTool);
    registry
}
```

## Tool Execution Flow

```
AgentLoop
  ↓ (tool call from LLM)
execute_tools()
  ↓
ToolRegistry.get(tool_name)
  ↓
tool.execute(input)
  ↓
ToolResult
```

### ToolExecutor with Retry Logic (DEPRECATED)

**Note**: `ToolExecutor` exists at `src/tool/executor.rs:8` but is NOT integrated into the tool execution flow. It has been deprecated and should not be used.

```rust
#[deprecated(since = "2026-05-27", note = "Not integrated - architectural mismatch with ToolRegistry")]
pub struct ToolExecutor {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}

impl ToolExecutor {
    pub fn new(max_attempts: usize) -> Self;
    pub fn with_delays(mut self, base_delay: Duration, max_delay: Duration) -> Self;

    pub async fn execute_with_retry<F, Fut>(&self, f: F) -> Result<Value, ToolError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Value, ToolError>>,
    {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) if e.is_retryable() && attempt < self.max_attempts => {
                    let delay = self.calculate_delay(attempt);
                    sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

### ToolError Retry Logic

```rust
#[derive(Error, Debug)]
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
    /// Check if this error is retryable (transient errors)
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_))
    }
}
```

## Tool Input

Tools receive `serde_json::Value` as input directly in their `execute()` method. There is no `ToolContext` struct - context information must be accessed through other means if needed.

## Tool Result

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}
```

## Path Validation

File tools should use `validate_path` from `src/tool/util.rs`:

```rust
pub fn validate_path(path: &str, base_dir: &Path) -> Result<PathBuf, ToolError> {
    // Resolve and validate path stays within base_dir
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
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
        matches!(
            self,
            ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
        )
    }
}
```

## Security Considerations

1. **Path Validation** - Always validate paths with `validate_path()` to prevent directory traversal
2. **Symlink Handling** - Walk tools (list, grep, glob) skip symlinks during traversal
3. **BashTool Blocked Patterns** - Tools should check against blocked patterns
4. **Unrestricted Mode** - Available in permission system; skips path validation for trusted environments (use with caution)

### Subprocess Security

When spawning external processes, always use `env_clear()` with the user's actual PATH:

```rust
use std::process::Command;

let mut cmd = Command::new("git");
cmd.env_clear();
if let Some(path) = std::env::var_os("PATH") {
    cmd.env("PATH", path);
} else {
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
}
cmd.args(&["log", "--oneline"]);
```

**Critical**: Use the user's actual PATH via `std::env::var_os("PATH")` after `env_clear()`. Never hardcode PATH as this breaks tools installed in non-standard locations (e.g., Homebrew, cargo, pyenv). This pattern is implemented in: bash.rs, commit.rs, formatter.rs, git.rs, terminal.rs, review.rs.

## Tool Definition Conversion

Tools are converted to provider-specific formats:

```rust
pub fn to_openai(&self) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters,
        }
    })
}

pub fn to_anthropic(&self) -> serde_json::Value {
    json!({
        "name": self.name,
        "description": self.description,
        "input_schema": self.parameters,
    })
}
```