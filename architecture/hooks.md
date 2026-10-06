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
// :16
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

// :93
pub trait Hook: Send + Sync {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>;
}

// :97
pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,  // default 30s (hooks/mod.rs:107)
    pub event: HookEvent,
}

// :174
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}
```

`HookRegistry::from_config()` (:189) builds from `HookConfigEntry` list.
`HookRegistry::run_hooks()` (:215) executes all hooks for an event,
collecting errors into a returned `Vec<AppError>`.

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

// :93
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

`InlineScript` hook type is deprecated (`codegg-config/src/schema.rs:1010`
carries the `#[deprecated]` attribute) and silently skipped at runtime with a
warning (`src/hooks/mod.rs:205-206`).

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
- Plugin hooks have a 5-second timeout per hook (`hook_timeout:
  Duration::from_secs(5)`, `src/plugin/service.rs:41`). Shell hooks default to
  30s (configurable via `timeout_secs`, `src/hooks/mod.rs:107`).
- Plugin hook errors include the plugin_id prefix:
  `{plugin_id}: hook timeout: ...` (`src/plugin/service.rs:624`)

## Testing

```bash
cargo test -p codegg -- hooks
```

## Related Docs

- [agent.md](agent.md) — AgentLoop integration points
- [plugin.md](plugin.md) — WASM plugin hooks

## Source verification

Verified 2026-10-06 against `src/hooks/mod.rs`, `src/plugin/hooks.rs`,
`src/plugin/service.rs`, and `crates/codegg-config/src/schema.rs`. Corrected 6
stale refs, all shifted by +4 lines except one: the `Hook` trait `:89` → `:93`,
`ShellCommandHook` `:93` → `:97`, `HookRegistry` `:170` → `:174`,
`HookRegistry::from_config` `:185` → `:189`, `HookRegistry::run_hooks`
`:211` → `:215`, and `HookResult` `plugin/hooks.rs:92` → `:93`.
Added source refs for three previously uncited invariants: the `InlineScript`
`#[deprecated]` attribute (`schema.rs:975`) and its skip-with-warning arm
(`hooks/mod.rs:205-206`), the 5s plugin hook timeout
(`src/plugin/service.rs:41`), and the `{plugin_id}: hook timeout` error prefix
(`service.rs:624`); also recorded that `run_hooks()` returns
`Vec<AppError>` and the 30s shell-hook default at `hooks/mod.rs:107`.
Confirmed correct as written: `HookEvent` (`:16`, 6 variants) and
`HookContext` (`:55`, 6 fields) with exact field lists, the 6 `CODEGG_*`
environment variable names (`hooks/mod.rs:68-83`), `HookType`
(`plugin/hooks.rs:6`, 13 variants in the documented order), `HookResult`
(4 fields, `blocked`/`output`/`error`/`effects`), and that `src/hooks/` is a
single `mod.rs` with no sibling files.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"). That commit inserted
`DecisionEngineConfig` near the top of `crates/codegg-config/src/schema.rs`,
shifting every later declaration down by 30 lines. Corrected: the
`InlineScript` `#[deprecated]` ref `975`->`1010` in the working tree (+35,
which includes an in-flight 5-line `AutoupdateConfig` doc-comment insertion);
re-checked that it lands on the `#[deprecated]` attribute itself. No other hook
reference in this document was affected — `src/hooks/mod.rs` and
`src/plugin/hooks.rs` are untouched by that commit.
