# Hooks Module

The `hooks` module provides two separate lifecycle event systems:
user-defined shell command hooks and WASM plugin hooks.

## Purpose

Allow users and plugins to run code at key points in the agent loop:
before/after tool execution, session start/end, and agent start/end.
Shell hooks are config-driven external commands; plugin hooks are
WASM-invoked and can block execution.

## Where It Lives

| System | Location |
|--------|----------|
| Shell command hooks | `src/hooks/mod.rs` (single file) |
| Plugin hooks | `src/plugin/hooks.rs` |

## How It Works

### Shell Command Hooks

1. Config entries in `[[hooks.*]]` TOML arrays are parsed by
   `HookRegistry::from_config()`.
2. Each entry becomes a `ShellCommandHook` that spawns `sh -c <command>`.
3. The environment is cleared (`env_clear()`), then `PATH` and
   `CODEGG_*` context variables are set.
4. Hooks run via `HookRegistry::run_hooks()` which collects errors
   without early-return.
5. Shell hooks **never block** execution — they are fire-and-forget.

### Plugin Hooks

1. WASM plugins register for `HookType` variants via their manifest.
2. Plugin hooks **can block** execution (`ToolExecuteBefore`,
   `SessionCompacting`).
3. Returns `HookResult` with `blocked`, `output`, `error`, and `effects`
   fields.

## Key Types & APIs

### Shell Command Hooks (`src/hooks/mod.rs`)

```rust
// :15
pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    SessionStart,
    SessionEnd,
    AgentStart,
    AgentEnd,
}

// :55
pub struct HookContext {
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub timestamp: i64,
}

// :89
pub trait Hook: Send + Sync {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>;
}

// :94
pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,  // default 30s
    pub event: HookEvent,
}

// :151
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}
```

`HookRegistry::from_config()` (:167) builds from `HookConfigEntry` list.
`HookRegistry::run_hooks()` (:193) executes all hooks for an event,
collecting errors.

### Plugin Hooks (`src/plugin/hooks.rs`)

```rust
// :6
pub enum HookType {
    Auth, Provider, ToolDefinition,
    ToolExecuteBefore,    // CAN BLOCK
    ToolExecuteAfter,
    ChatParams, ChatHeaders, Event, Config,
    ShellEnv, TextComplete,
    SessionCompacting,    // CAN BLOCK
    MessagesTransform,
}

// :92
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
    pub effects: Vec<crate::protocol::ui::UiEffect>,
}
```

## Configuration Surface

```toml
[hooks]
enabled = true

[[hooks.pre_tool_execute]]
event = "pre_tool_execute"
type = "shell_command"
command = "echo"
timeout_secs = 10
```

`InlineScript` hook type is deprecated and silently skipped at runtime.

### Environment Variables Passed to Shell Hooks

| Variable | Description |
|----------|-------------|
| `CODEGG_HOOK_EVENT` | Event name (`pre_tool_execute`, etc.) |
| `CODEGG_SESSION_ID` | Current session ID |
| `CODEGG_TOOL_NAME` | Tool name (Pre/PostToolExecute only) |
| `CODEGG_TOOL_ARGUMENTS` | Tool args JSON (Pre/PostToolExecute only) |
| `CODEGG_TOOL_RESULT` | Tool result (PostToolExecute only) |
| `CODEGG_TIMESTAMP` | Unix timestamp |
| `PATH` | User's PATH (sole inherited env var) |

## Invariants & Gotchas

- `env_clear()` means hooks inherit **nothing** from the parent process
  except the explicitly set vars and `PATH`.
- Shell hook errors are collected, not propagated. A failing hook does
  not abort the agent loop.
- `AgentEnd` hooks do NOT run on stream errors (the loop breaks before
  reaching them).
- `SessionEnd` hooks run after the loop exits.
- Plugin hooks have a 5-second timeout per hook. Shell hooks default to
  30s (configurable via `timeout_secs`).
- Plugin hook errors include the plugin_id prefix:
  `{plugin_id}: hook timeout: ...`

## Testing

```bash
cargo test -p codegg -- hooks
```

## Related Docs

- [agent.md](agent.md) — AgentLoop integration points
- [plugin.md](plugin.md) — WASM plugin hooks
