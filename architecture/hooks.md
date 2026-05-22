# Hooks Module

The `hooks` module provides a lifecycle event system for running custom scripts at key points in the agent loop execution.

## Overview

**Location**: `src/hooks/`

**Key Responsibilities**:
- Register and manage shell command hooks
- Execute hooks at lifecycle events
- Two distinct hook systems: shell command hooks and WASM plugin hooks

## Two Hook Systems

This codebase has **two separate hook systems**:

1. **`src/hooks/mod.rs`** - Shell Command Hooks (user-defined external commands)
2. **`src/plugin/hooks.rs`** - WASM Plugin Hooks (plugin-based extensibility)

---

# Shell Command Hooks

## HookEvent Enum

```rust
pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    SessionStart,
    SessionEnd,
    AgentStart,
    AgentEnd,
}
```

**Note**: `PreAgentRun` and `PostAgentRun` are documented but **not implemented**.

## HookContext

```rust
pub struct HookContext {
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub timestamp: i64,
}
```

## HookRegistry

```rust
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}

pub trait Hook: Send + Sync {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>;
}
```

### Hook Execution

```rust
impl HookRegistry {
    pub async fn run_hooks(&self, event: HookEvent, ctx: &HookContext) -> Vec<AppError> {
        // Executes all hooks for an event
        // Errors are collected and returned, not early-returned
    }
}
```

### ShellCommandHook

```rust
pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,
    pub event: HookEvent,
}

impl ShellCommandHook {
    pub fn new(command: String, timeout_secs: Option<u64>, event: HookEvent) -> Self;
}

impl Hook for ShellCommandHook {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError> {
        // Spawns `sh -c <command>` with CODEGG_* env vars
        // Default timeout: 30 seconds
        // Uses user's actual PATH from environment
        // Error messages include event name for debugging
    }
}
```

## Hook Configuration

```toml
[hooks]
enabled = true

[[hooks.pre_tool_execute]]
event = "pre_tool_execute"
type = "shell_command"
command = "echo"
timeout_secs = 10

[[hooks.post_agent_run]]
event = "agent_end"
type = "shell_command"
command = "curl"
args = ["-X", "POST", "https://example.com/hook"]
```

## Environment Variables

Hooks receive context via environment variables:

| Variable | Description |
|----------|-------------|
| `CODEGG_HOOK_EVENT` | Current hook event name |
| `CODEGG_SESSION_ID` | Current session ID |
| `CODEGG_TOOL_NAME` | Tool being executed (PreToolExecute/PostToolExecute only) |
| `CODEGG_TOOL_ARGUMENTS` | Tool arguments JSON (PreToolExecute/PostToolExecute only) |
| `CODEGG_TOOL_RESULT` | Tool result (PostToolExecute only) |
| `CODEGG_TIMESTAMP` | Unix timestamp |
| `PATH` | User's actual PATH from environment |

---

# Plugin Hooks (WASM)

**Location**: `src/plugin/hooks.rs`

## HookType Enum

```rust
pub enum HookType {
    Auth,
    Provider,
    ToolDefinition,
    ToolExecuteBefore,    // CAN BLOCK execution
    ToolExecuteAfter,
    ChatParams,
    ChatHeaders,
    Event,
    Config,
    ShellEnv,
    TextComplete,
    SessionCompacting,    // CAN BLOCK compaction
    MessagesTransform,
}
```

## HookResult

```rust
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,        // Can prevent execution
    pub error: Option<String>,
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self;
    pub fn blocked() -> Self;
    pub fn error(msg: impl Into<String>) -> Self;
}
```

---

# Integration Points

## Shell Command Hooks in AgentLoop

| Location | Event | Can Block? |
|----------|-------|-----------|
| `loop.rs:1255` | `SessionStart` | No |
| `loop.rs:1351` | `AgentStart` | No |
| `loop.rs:1751` | `PreToolExecute` | No |
| `loop.rs:1825` | `PostToolExecute` | No |
| `loop.rs:1524` | `AgentEnd` | No |
| `loop.rs:1545` | `SessionEnd` | No |

**Important**: Stream errors now break the loop instead of returning early, ensuring `AgentEnd` and `SessionEnd` hooks run.

## Plugin Hooks in AgentLoop

| Location | Event | Can Block? |
|----------|-------|-----------|
| `loop.rs:1764` | `ToolExecuteBefore` | **Yes** |
| `loop.rs:1806` | `ToolExecuteAfter` | No |
| `loop.rs:1157` | `SessionCompacting` | **Yes** |

---

# Key Differences from Plugin Hooks

| Feature | Shell Command Hooks | Plugin Hooks |
|---------|--------------------|--------------|
| Source | User config | WASM plugins |
| Implementation | `src/hooks/mod.rs` | `src/plugin/hooks.rs` |
| Hook trait | `Hook` trait | `HookType` enum |
| Blocking | Never | `ToolExecuteBefore`, `SessionCompacting` |
| Return type | `Vec<AppError>` | `HookResult` |
| Configuration | TOML config | Plugin manifest |

---

# See Also

- [agent.md](agent.md) - AgentLoop integration
- [plugin.md](plugin.md) - WASM plugin hooks