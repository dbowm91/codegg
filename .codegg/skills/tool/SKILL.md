---
name: tool
description: Tool trait, registration, execution flow, adding new tools
version: 1.0.0
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
    └── ... (25+ total)
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
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn with_defaults() -> Self;
    pub fn register(&mut self, tool: impl Tool + 'static);
    pub fn get(&self, name: &str) -> Option<&dyn Tool>;
    pub fn list(&self) -> Vec<&dyn Tool>;
    pub fn definitions(&self) -> Vec<ToolDefinition>;
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
| `multiedit` | Multiple edits in one operation |
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

### Other Tools

| Tool | Description |
|------|-------------|
| `task` | Manage subagent tasks |
| `question` | Ask user questions |
| `todo` | Manage TODO list |
| `skill` | Load and use skills |
| `batch` | Execute multiple operations |
| `lsp` | LSP (Language Server Protocol) integration |

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
ToolExecutor.execute_with_retry()
  ↓
tool.execute(input)
  ↓
ToolResult
```

### ToolExecutor with Retry Logic

```rust
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
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("timeout: {0}")]
    Timeout(String),
}

impl ToolError {
    /// Check if this error is retryable (transient errors)
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_))
    }
}
```

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
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("not found: {0}")]
    NotFound(String),
}
```

## Security Considerations

1. **Path Validation** - Always validate paths with `validate_path()` to prevent directory traversal
2. **Symlink Handling** - Walk tools (list, grep, glob) skip symlinks during traversal
3. **BashTool Blocked Patterns** - Tools should check against blocked patterns
4. **Unrestricted Mode** - For trusted environments only; skips validation

### Subprocess Security

When spawning external processes, always use `env_clear()` with a minimal safe PATH:

```rust
use std::process::Command;

let mut cmd = Command::new("git");
cmd.env_clear();
cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");  // Hardcoded, not from environment
cmd.args(&["log", "--oneline"]);
```

**Critical**: Use hardcoded PATH `/usr/local/bin:/usr/bin:/bin` after `env_clear()`. Do NOT use `std::env::var("PATH")` as this restores the original unsafe PATH. This pattern is implemented in: bash.rs, commit.rs, formatter.rs, git.rs, terminal.rs, mcp/local.rs, lsp/launch.rs, hooks/mod.rs.

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