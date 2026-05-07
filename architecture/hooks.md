# Hooks Module

The `hooks` module provides a lifecycle event system for running custom scripts at key points.

## Overview

**Location**: `src/hooks/`

**Key Responsibilities**:
- Register and manage hooks
- Execute hooks at lifecycle events
- Shell command hook execution

## HookEvent Enum

```rust
pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    PreAgentRun,
    PostAgentRun,
    SessionStart,
    SessionEnd,
    AgentStart,
    AgentEnd,
}
```

## HookContext

Data passed to hooks:

```rust
pub struct HookContext {
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub message: Option<String>,
    pub metadata: Value,
}
```

## HookRegistry

```rust
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Hook>>,
}

pub struct Hook {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}
```

### Hook Execution

```rust
impl HookRegistry {
    pub async fn run_hooks(&self, event: HookEvent, ctx: HookContext) -> HookResult {
        let hooks = self.hooks.get(&event);
        for hook in hooks {
            hook.execute(&ctx).await?;
        }
        Ok(HookResult::Continue)
    }
}
```

### ShellCommandHook

```rust
pub struct ShellCommandHook {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
}

impl ShellCommandHook {
    pub async fn execute(&self, ctx: &HookContext) -> Result<HookOutput> {
        // Run command with ctx as JSON
    }
}
```

## Hook Configuration

```toml
[hooks]
enabled = true

[[hooks.pre_tool_execute]]
name = "log-tool-call"
command = "echo"
args = ["{{tool_name}}", "{{params}}"]

[[hooks.post_agent_run]]
name = "notify-complete"
command = "curl"
args = ["-X", "POST", "https://example.com/hook", "-d", "{{result}}"]
```

## Variables in Hooks

Hooks can use template variables:

| Variable | Description |
|----------|-------------|
| `{{tool_name}}` | Tool being executed |
| `{{params}}` | Tool parameters (JSON) |
| `{{session_id}}` | Current session ID |
| `{{result}}` | Tool/LLM result |
| `{{message}}` | User message |

## Integration Points

Hooks are called at these points in `AgentLoop`:

```rust
impl AgentLoop {
    async fn run(&self) -> Result<()> {
        // AgentStart
        self.hooks.run_hooks(HookEvent::AgentStart, ctx).await?;

        loop {
            // PreToolExecute
            self.hooks.run_hooks(HookEvent::PreToolExecute, ctx).await?;

            // Tool execution...

            // PostToolExecute
            self.hooks.run_hooks(HookEvent::PostToolExecute, ctx).await?;
        }

        // AgentEnd
        self.hooks.run_hooks(HookEvent::AgentEnd, ctx).await?;
    }
}
```

## See Also

- [agent.md](agent.md) - AgentLoop integration
- [plugin.md](plugin.md) - WASM plugin hooks
