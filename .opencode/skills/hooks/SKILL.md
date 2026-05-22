---
name: hooks
description: Hooks system for agent loop lifecycle events in opencode-rs
version: 1.2.0
tags:
  - hooks
  - lifecycle
  - agent
  - events
  - plugin
---

# Hooks System Guide

This skill covers the hooks system in opencode-rs, which allows running custom commands or scripts at specific points in the agent loop execution.

## Two Hook Systems

opencode-rs has **two distinct hook systems** that are often confused:

1. **Shell Command Hooks** (`src/hooks/mod.rs`) - User-configured via config.yaml
2. **Plugin Hooks** (`src/plugin/hooks.rs`) - WASM plugins using hook types

## Shell Command Hooks

### HookEvent Enum

Defined in `src/hooks/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    SessionStart,
    SessionEnd,
    AgentStart,
    AgentEnd,
}
```

### Shell HookContext

```rust
pub struct HookContext {
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub timestamp: i64,
}

impl HookContext {
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        // Returns environment variables:
        // - CODEGG_HOOK_EVENT
        // - CODEGG_SESSION_ID
        // - CODEGG_TOOL_NAME
        // - CODEGG_TOOL_ARGUMENTS
        // - CODEGG_TOOL_RESULT
        // - CODEGG_TIMESTAMP
    }
}
```

### Shell HookRegistry

```rust
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}

impl HookRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, event: HookEvent, hook: Box<dyn Hook>);
    pub fn from_config(hook_defs: &[HookConfigEntry]) -> Self;
    pub async fn run_hooks(&self, event: HookEvent, ctx: &HookContext) -> Vec<AppError>;
    pub fn has_hooks(&self, event: HookEvent) -> bool;
}
```

**Important**: `run_hooks()` returns `Vec<AppError>` - errors are collected and returned, not short-circuit.

### ShellCommandHook Implementation

```rust
pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,
}

#[async_trait]
impl Hook for ShellCommandHook {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError> {
        let env_vars = ctx.to_env_vars();
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&self.command)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        let output = tokio::time::timeout(self.timeout, cmd.output()).await;
        // ... error handling
    }
}
```

### Configuration

Hooks are configured in config.yaml under the `hooks` key:

```yaml
hooks:
  pre_tool_execute:
    - event: pre_tool_execute
      hook:
        shell_command:
          command: echo "Tool {tool_name} executing"
          timeout_secs: 30
  post_tool_execute:
    - event: post_tool_execute
      hook:
        shell_command:
          command: echo "Tool completed"
          timeout_secs: 30
```

### Shell Hook Variables

| Variable | Description |
|----------|-------------|
| `CODEGG_HOOK_EVENT` | Event type (e.g., `pre_tool_execute`) |
| `CODEGG_SESSION_ID` | Current session ID |
| `CODEGG_TOOL_NAME` | Name of the tool being executed |
| `CODEGG_TOOL_ARGUMENTS` | Tool input arguments (JSON) |
| `CODEGG_TOOL_RESULT` | Tool output (for post-tool events) |
| `CODEGG_TIMESTAMP` | Unix timestamp |

## Plugin Hooks

Plugin hooks use a different system defined in `src/plugin/hooks.rs`.

### HookType Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
pub enum HookType {
    Auth,
    Provider,
    ToolDefinition,
    ToolExecuteBefore,  // Called from execute_tool_calls()
    ToolExecuteAfter,    // Called from execute_tool_calls()
    ChatParams,
    ChatHeaders,
    Event,
    Config,
    ShellEnv,
    TextComplete,
    SessionCompacting,
    MessagesTransform,
}
```

### Plugin HookContext and HookResult

```rust
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}

pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self;
    pub fn blocked() -> Self;
    pub fn error(msg: impl Into<String>) -> Self;
}
```

### Plugin Service Dispatch

```rust
pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult {
    let hooks = self.registry.hooks_for(ctx.hook_type).await;
    for hook in hooks {
        if !self.registry.is_enabled(&hook.plugin_id).await {
            continue;
        }
        let result = self.execute_hook_with_timeout(&hook.plugin_id, hook_ctx).await;
        match result {
            Ok(res) => {
                if res.blocked { return res; }
                if let Some(err) = &res.error { return res; }
                current_input = res.output;
            }
            Err(err) => {
                return HookResult::error(format!("{}: hook timeout: {}", hook.plugin_id, err));
            }
        }
    }
    HookResult::ok(current_input)
}
```

### Available Dispatch Methods

```rust
pub async fn dispatch_tool_execute_before(&self, input: serde_json::Value) -> HookResult;
pub async fn dispatch_tool_execute_after(&self, input: serde_json::Value) -> HookResult;
pub async fn dispatch_tool_definition(&self, input: serde_json::Value) -> HookResult;
pub async fn dispatch_session_compacting(&self, input: serde_json::Value) -> HookResult;
pub async fn dispatch_messages_transform(&self, input: serde_json::Value) -> HookResult;
// ... and more
```

## ToolExecuteBefore/After Integration (2026-05-22)

As of 2026-05-22, `ToolExecuteBefore` and `ToolExecuteAfter` plugin hooks are **now called** from `execute_tool_calls()` in `agent/loop.rs`:

```rust
// Before tool execution - can block
if let Some(ref ps) = plugin_service {
    let input = serde_json::json!({
        "tool_name": tool_name,
        "arguments": tc_arc.arguments,
        "session_id": session_id,
    });
    let hook_result = ps.dispatch_tool_execute_before(input).await;
    if hook_result.blocked {
        return Err(ToolError::Execution("blocked by plugin hook".to_string()));
    }
}

// ... tool execution ...

// After tool execution
if let Some(ref ps) = plugin_service {
    let input = serde_json::json!({
        "tool_name": tool_name,
        "arguments": tc_arc.arguments,
        "session_id": session_id,
        "result": result.as_ref().ok(),
    });
    let hook_result = ps.dispatch_tool_execute_after(input).await;
}
```

## Configuration Validation (2026-05-22)

`HookRegistry::from_config()` now logs warnings for:
- Invalid hook event names (e.g., `"pre_tool_execut"` instead of `"pre_tool_execute"`)
- Unimplemented `InlineScript` hook type (still returns early)

## Security Considerations

### Shell Hooks
1. **Command Injection** - Hook commands are executed with shell interpolation. Validate any user-provided data.
2. **Timeouts** - Hooks have a default timeout of 30 seconds to prevent hanging.
3. **Error Handling** - Hook failures should not crash the agent loop; errors are logged and ignored.
4. **Environment** - Hooks run with `env_clear()` and minimal `PATH` (`/usr/local/bin:/usr/bin:/bin`).

### Plugin Hooks
1. **Fuel Limits** - Per-plugin fuel budgets prevent runaway plugins.
2. **Timeout** - 5 second timeout per hook.
3. **Blocked Execution** - If `blocked: true`, the agent loop aborts the operation.

## Adding a New Hook Event

### Shell Command Hook
1. Add the variant to `HookEvent` enum in `src/hooks/mod.rs`
2. Implement `as_str()` and `FromStr` traits
3. Call `HookRegistry::run_hooks()` at the appropriate point in the agent loop
4. Update this SKILL.md

### Plugin Hook Type
1. Add the variant to `HookType` enum in `src/plugin/hooks.rs`
2. Add `dispatch_*` method to `PluginService` in `src/plugin/service.rs`
3. Call the dispatch method at the appropriate point in the agent loop
4. Update this SKILL.md

## Known Differences

| Aspect | Shell Hooks | Plugin Hooks |
|--------|-------------|--------------|
| Context | Typed struct with specific fields | Generic JSON `input` |
| Env prefix | `CODEGG_` | N/A (JSON) |
| Timeout | 30s default | 5s hardcoded |
| Error handling | Collect all, continue | Stop on first error |
| Blocking support | No | Yes (`blocked: true`) |

## Related Skills

- See `.opencode/skills/plugin/SKILL.md` for WASM plugin system
- See `AGENTS.md` for project-wide patterns