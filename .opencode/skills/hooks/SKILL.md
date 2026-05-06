---
name: hooks
description: Hooks system for agent loop lifecycle events in opencode-rs
version: 1.1.0
tags:
  - hooks
  - lifecycle
  - agent
  - events
---

# Hooks System Guide

This skill covers the hooks system in opencode-rs, which allows running custom commands or scripts at specific points in the agent loop execution.

## Hook Events

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

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreToolExecute => "pre_tool_execute",
            HookEvent::PostToolExecute => "post_tool_execute",
            HookEvent::SessionStart => "session_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::AgentStart => "agent_start",
            HookEvent::AgentEnd => "agent_end",
        }
    }
}
```

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

impl HookContext {
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        // Returns environment variables:
        // - OPENCODE_HOOK_EVENT
        // - OPENCODE_SESSION_ID
        // - OPENCODE_TOOL_NAME
        // - OPENCODE_TOOL_ARGUMENTS
        // - OPENCODE_TOOL_RESULT
        // - OPENCODE_TIMESTAMP
    }
}
```

## Hook Trait

```rust
#[async_trait]
pub trait Hook: Send + Sync {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>;
}
```

## ShellCommandHook

```rust
pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,
}

impl ShellCommandHook {
    pub fn new(command: String, timeout_secs: Option<u64>) -> Self {
        Self {
            command,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
        }
    }
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

## HookRegistry

```rust
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}

impl HookRegistry {
    pub fn new() -> Self;

    pub fn register(&mut self, event: HookEvent, hook: Box<dyn Hook>);

    pub fn from_config(hook_defs: &[HookConfigEntry]) -> Self {
        // Parses config and creates ShellCommandHook for each entry
    }

    pub async fn run_hooks(&self, event: HookEvent, ctx: &HookContext) -> Result<(), AppError> {
        if let Some(hooks) = self.hooks.get(&event) {
            for hook in hooks {
                hook.execute(ctx).await?;
            }
        }
        Ok(())
    }

    pub fn has_hooks(&self, event: HookEvent) -> bool;
}
```

## Configuration

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

## Hook Variables

Hooks can use environment variables in commands:

| Variable | Description |
|----------|-------------|
| `OPENCODE_HOOK_EVENT` | Event type (e.g., `pre_tool_execute`) |
| `OPENCODE_SESSION_ID` | Current session ID |
| `OPENCODE_TOOL_NAME` | Name of the tool being executed |
| `OPENCODE_TOOL_ARGUMENTS` | Tool input arguments (JSON) |
| `OPENCODE_TOOL_RESULT` | Tool output (for post-tool events) |
| `OPENCODE_TIMESTAMP` | Unix timestamp |

## Security Considerations

1. **Command Injection** - Hook commands are executed with shell interpolation. Validate any user-provided data.
2. **Timeouts** - Hooks have a default timeout of 30 seconds to prevent hanging.
3. **Error Handling** - Hook failures should not crash the agent loop; errors are logged and ignored.
4. **Environment** - Hooks run with `env_clear()` and minimal `PATH` (`/usr/local/bin:/usr/bin:/bin`).

## Adding a New Hook Event

1. Add the variant to `HookEvent` enum in `src/hooks/mod.rs`
2. Implement `as_str()` and `FromStr` traits
3. Call `HookRegistry::run_hooks()` at the appropriate point in the agent loop
4. Update this SKILL.md

(End of file - 107 lines)
