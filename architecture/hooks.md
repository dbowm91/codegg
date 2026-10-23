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
| Plugin hooks | `src/plugin/hooks.rs` (types), `src/plugin/service.rs` (dispatch) |
| Plugin hook lifecycle/policy | `src/plugin/lifecycle.rs`, `src/plugin/permission.rs` |

## How It Works

### Shell Command Hooks

1. Config entries in the top-level `hooks` array (`Vec<HookConfigEntry>`) are
   parsed by `HookRegistry::from_config()`.
2. Each `ShellCommand` entry becomes a `ShellCommandHook` that runs
   `sh -c <command>` through the **managed-process** surface —
   `ManagedProcessService::run` with a `ManagedProcessRequest` — not a bare
   `std::process::Command`. The request carries the hook timeout and an
   `OutputPolicy::with_limits(64 * 1024, 64 * 1024)` cap.
3. The child's environment is `EnvironmentPolicy::sanitized()` plus the
   `CODEGG_*` context variables (see the table below).
4. Hooks run via `HookRegistry::run_hooks()`, which collects errors into a
   returned `Vec<AppError>` without early-return.
5. A shell hook can never *deny* anything — its error is only logged. It is
   still **awaited inline** before the call site continues, so a slow hook
   delays the turn by up to its own timeout.

Shell hooks fire from two places:

| Event | Call site |
|---|---|
| `SessionStart` | `src/agent/loop.rs:931` (context) / `:943` (`run_hooks`) — once, at the top of `AgentLoop::run` (which opens at `:909`) |
| `AgentStart` | `src/agent/loop.rs:1165` / `:1177` — once per **turn iteration** |
| `AgentEnd` | `src/agent/loop.rs:2050` / `:2062` — once per **turn iteration** |
| `SessionEnd` | `src/agent/loop.rs:2222` / `:2235` — once, after the turn loop exits |
| `PreToolExecute` | `src/agent/tool_batch.rs:1751` / `:1763` — before each tool runs |
| `PostToolExecute` | `src/agent/tool_batch.rs:2215` / `:2227` — after each tool returns |

The turn loop itself opens at `src/agent/loop.rs:1111`.

### Plugin Hooks

1. Plugins register for `HookType` variants in their manifest. The canonical
   form is a `[[capabilities]]` entry with `type = "hook"` and
   `hook_type = "tool.execute.before"`; the older `[[hooks]]` entry
   (`type = "tool.execute.before"`, optional `priority`) is still accepted and
   is auto-converted to `PluginCapability::Hook` entries when the manifest
   declares no capabilities at all (`manifest.rs:512-524`).
2. Plugin hooks **can block** execution, but only the hook types the policy
   classifies as `Blocking`: `ToolExecuteBefore` and `Auth`
   (`policy.rs:110`, `classify_hook`). `SessionCompacting` is classified
   **Observation**, not Blocking. Blocking hooks are **disabled by default**
   (`PluginLifecyclePolicy::enable_blocking_hooks: false`), so out of the box
   no plugin hook can block.
3. Returns `HookResult` with `blocked`, `output`, `error`, and `effects`
   fields.
4. `PluginService::dispatch_hook` runs the registered hooks for one
   `HookType` **in a chain**: each hook's `output` becomes the next hook's
   `input`, and `effects` accumulate across the chain. A `blocked` result
   short-circuits and is returned immediately; so does an `error` result.
5. Before each hook runs, `PluginService` applies the plugin policy through
   `plugin::permission::check_lifecycle_hook_allowed`; a denied hook is
   skipped with a warning rather than invoked.

The policy also gates the *runtime* and the *trust class* of a lifecycle
hook: `PluginRuntimeSpec::Passive` is never allowed, a `Process` runtime
requires `allow_process_lifecycle_hooks` (default **off**), and
`Auth`/`Provider` hooks require the `Builtin` trust class
(`permission.rs:197-231`).

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

`HookRegistry::from_config()` (:189) builds from a `HookConfigEntry` list.
`HookRegistry::run_hooks()` (:215) executes all hooks for an event,
collecting errors into a returned `Vec<AppError>`. `HookRegistry::has_hooks()`
(:227) reports whether any hook is registered for an event.

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

// :62
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}

// :93
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
    pub effects: Vec<crate::protocol::ui::UiEffect>,
}
```

`HookType::as_str()` (:23) returns the **dotted** wire name
(`"tool.execute.before"`, `"session.compacting"`, …). That is the form the
manifest, the permission policy, and the runtime invocation all use, and
`HookType::parse()` (:41) is the parser in use everywhere. The `strum`
`EnumString` derive on the same enum yields **snake_case**
(`tool_execute_before`) instead, so the two spellings coexist: use
`HookType::parse` for the dotted form.

`HookContext::into_plugin_invocation()` (:69) converts a plugin hook context
into a `crate::protocol::plugin::PluginInvocation` carrying
`PluginCapabilityInvocation::Hook { hook_type }`.

## Configuration Surface

`Config::hooks` (`crates/codegg-config/src/schema.rs:287`) is
`Option<Vec<HookConfigEntry>>` — a **flat array of tables**, not a table of
per-event sub-tables. `HookConfigEntry` (:1340) carries `event: String` plus
`#[serde(flatten)] hook: HookConfig`, and `HookConfig` (:1358) is
`#[serde(tag = "type", rename_all = "snake_case")]`.

```toml
[[hooks]]
event = "pre_tool_execute"
type = "shell_command"
command = "echo"
timeout_secs = 10
```

There is no `hooks.enabled` key and no `[[hooks.pre_tool_execute]]`
sub-table. Neither shape can become a registered hook: `HookConfigEntry`
requires an `event` field and the flattened `HookConfig` requires the `type`
tag, so a `hooks.enabled` boolean or a per-event sub-table is not a
`HookConfigEntry` at all. (`Config` is `#[serde(default)]`, but that default
only covers an **absent** `hooks` key — it does not rescue a malformed
entry.)

`InlineScript` hook type is deprecated (`codegg-config/src/schema.rs:1366`
carries the `#[deprecated]` attribute) and silently skipped at runtime with a
warning (`src/hooks/mod.rs:205-208`).

### Environment Variables Passed to Shell Hooks

| Variable | Description |
|----------|-------------|
| `CODEGG_HOOK_EVENT` | Event name (`pre_tool_execute`, etc.) |
| `CODEGG_SESSION_ID` | Current session ID |
| `CODEGG_TOOL_NAME` | Tool name (Pre/PostToolExecute only) |
| `CODEGG_TOOL_ARGUMENTS` | Tool args JSON (Pre/PostToolExecute only) |
| `CODEGG_TOOL_RESULT` | Tool result (PostToolExecute only) |
| `CODEGG_TIMESTAMP` | Unix timestamp |

The hook child does **not** inherit the parent environment wholesale.
`ShellCommandHook::execute` builds `EnvironmentPolicy::sanitized()` and
adds the `CODEGG_*` variables with `with_var`. The resulting child
environment is:

- the `codegg_git::ALLOWED_ENV_VARS` allowlist (21 names on Unix, including
  `PATH`, `HOME`, `TMPDIR`, `SSH_AUTH_SOCK`, the CA-bundle variables) that is
  present in the parent, minus `codegg_git::ALWAYS_STRIPPED_ENV_VARS` (28
  command-bearing Git/SSH/pager variables such as `GIT_SSH_COMMAND`,
  `GIT_DIR`, `GIT_ASKPASS`). On Windows the allowlist additionally carries
  `SystemRoot`, `WINDIR`, `TEMP`, `TMP` (`src/managed_process.rs:84-93`);
- the six `CODEGG_*` context variables above;
- six **service-owned** invariants the managed-process layer sets *after* the
  caller's variables: `CI=1`, `NO_COLOR=1`, `TERM=dumb`,
  `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat`, `PAGER=cat`. `GIT_PAGER` and
  `PAGER` are on the denied list yet are set to `cat` by the service — the
  service owns those two.

`EnvironmentPolicy::with_var` refuses to set a denied variable, so a
`CODEGG_*` name could not reintroduce a stripped one in practice.


## Invariants & Gotchas

- A shell hook's environment is the **sanitized managed-process** environment
  (allowlist + `CODEGG_*` + service-owned invariants), not the parent
  environment and not a bare `env_clear()`-plus-`PATH`.
- Shell hook errors are collected, not propagated. A failing hook does
  not abort the agent loop — the error is only logged by the call site.
- Shell hooks are awaited **inline**, so a hung hook delays the turn by up
  to its timeout (30s by default). "Never blocks" means it can never *deny*
  the action, not that it costs no time.
- `AgentStart` and `AgentEnd` fire once per **turn iteration** of
  `AgentLoop::run`'s loop (opened at `src/agent/loop.rs:1111`), not once per
  session. `SessionStart` fires once before the loop; `SessionEnd` fires once
  after it.
- A provider **stream error** makes `AgentLoop::run` `return Err(e)`
  (`src/agent/loop.rs:1478`) before the loop body reaches either the
  `AgentEnd` or the `SessionEnd` hook, so **both are skipped** on a stream
  error. The same is true when the loop breaks early on the turn-limit
  check, a cancel signal, or a stalled-recovery decision.
- **A shell hook's working directory is the process CWD, not the session
  workspace.** `ShellCommandHook::execute` resolves it with
  `std::env::current_dir()` (`src/hooks/mod.rs:118`). In the user-scoped
  daemon that is wherever the daemon was launched, not the session's
  `workspace_root` — so a shell hook runs in an unrelated tree. This
  contradicts the daemon-workspace rule the rest of the codebase follows,
  and `scripts/check_daemon_cwd_usage.py` does not currently scan
  `src/hooks/`.
- A shell hook runs on the **managed-process** surface, so it is subject to
  the managed-process admission and output limits; it is not an exempt
  direct spawn.
- Plugin hooks have a 5-second timeout per hook (`hook_timeout:
  Duration::from_secs(5)`, `src/plugin/service.rs:41`). Shell hooks default to
  30s (configurable via `timeout_secs`, `src/hooks/mod.rs:107`).
- Plugin hook errors include the plugin_id prefix:
  `{plugin_id}: hook timeout: ...` (`src/plugin/service.rs:641`).
- A plugin hook's `blocked` result short-circuits the chain: later hooks for
  that event do not run, and the accumulated `effects` up to that point are
  preserved on the returned `HookResult`.
- A plugin hook denied by `plugin::permission::check_lifecycle_hook_allowed`
  is **skipped**, not failed: the chain continues with the previous input.

## Testing

```bash
# Plugin hook types, dispatch chain, and policy
cargo test -p codegg -- plugin::hooks
cargo test -p codegg -- plugin::service
cargo test -p codegg -- plugin::permission
```

**Coverage gap**: `src/hooks/mod.rs` (the shell-hook module) has **no**
`#[cfg(test)]` module and no integration test in the tree exercises
`HookRegistry::from_config` / `run_hooks`. `cargo test -p codegg -- hooks`
therefore matches only the *plugin* hook tests, not the shell-hook path.
Any change to the shell-hook registry, the managed-process invocation, or the
`CODEGG_*` environment contract needs a new test.

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

Verified 2026-10-23 against `src/hooks/mod.rs`, `src/plugin/hooks.rs`,
`src/plugin/service.rs`, `src/plugin/permission.rs`,
`crates/codegg-config/src/schema.rs`, `src/agent/loop.rs`,
`src/agent/tool_batch.rs`, `src/managed_process.rs`, and
`crates/egggit/src/process.rs`. Corrections made:

| Claim in the previous revision | Finding |
|---|---|
| "Config entries in `[[hooks.*]]` TOML arrays" | Wrong shape. `Config::hooks` is `Option<Vec<HookConfigEntry>>` (`schema.rs:287`), so the TOML is a flat `[[hooks]]` array of tables with `event` plus a `#[serde(flatten)]` `HookConfig` tagged `type`. The documented `[hooks] enabled = true` / `[[hooks.pre_tool_execute]]` shape does not populate the registry — replaced with the real shape. |
| "The environment is cleared (`env_clear()`), then `PATH` and `CODEGG_*` context variables are set"; "`PATH` — user's PATH (sole inherited env var)" | Stale. `ShellCommandHook::execute` builds `EnvironmentPolicy::sanitized()` (the `codegg_git::ALLOWED_ENV_VARS` allowlist minus `ALWAYS_STRIPPED_ENV_VARS`), adds the `CODEGG_*` variables, and the managed-process layer then forces `CI`/`NO_COLOR`/`TERM`/`GIT_TERMINAL_PROMPT`/`GIT_PAGER`/`PAGER`. `PATH` is inherited but is **not** the only inherited variable. |
| "spawns `sh -c <command>`" (implied bare spawn) | Incomplete. The hook runs through `ManagedProcessService::run` with a `ManagedProcessRequest`, the hook timeout, and `OutputPolicy::with_limits(64 * 1024, 64 * 1024)`. |
| "Shell hooks **never block** execution — they are fire-and-forget" | Misleading. The result can never deny, but the hook is awaited inline and can delay the turn by up to its timeout. |
| "`AgentEnd` hooks do NOT run on stream errors (the loop breaks before reaching them)" | Right conclusion, wrong mechanism and incomplete. `AgentLoop::run` `return Err(e)` at `src/agent/loop.rs:1477-1480` on a provider stream error, which skips **both** the `AgentEnd` and the `SessionEnd` hook. |
| "`SessionEnd` hooks run after the loop exits" | Correct but placed in the wrong frame: `AgentStart`/`AgentEnd` fire once per **turn iteration** of the loop opened at `src/agent/loop.rs:1111`; `SessionEnd` fires once after it closes — the hook context is built at `src/agent/loop.rs:2222` and dispatched at `:2235`. (An earlier revision of this table said `:2095`; that line is `self.drain_follow_up(...)`, not the hook.) |
| plugin hook error prefix `src/plugin/service.rs:624` | Now `src/plugin/service.rs:641` (the file grew). |
| `InlineScript` deprecated ref `schema.rs:1010` | Now `schema.rs:1366`; the runtime skip arm is `src/hooks/mod.rs:205-208`. |
| — | Added: the plugin hook chain semantics (`dispatch_hook` feeds each hook's `output` into the next hook's `input`, accumulates `effects`, short-circuits on `blocked`/`error`), the `check_lifecycle_hook_allowed` policy skip, `HookType::as_str`'s dotted wire names vs the `strum` snake_case derive, `HookContext { hook_type, input }`, `HookRegistry::has_hooks`, and the call-site table for each shell hook event. |

Confirmed still accurate: `HookEvent` (`:16`), `HookContext` (`:55`), the
`Hook` trait (`:93`), `ShellCommandHook` (`:97`), `HookRegistry` (`:174`),
`HookRegistry::from_config` (`:189`), `HookRegistry::run_hooks` (`:215`),
the 30s shell-hook default (`hooks/mod.rs:107`), the 5s plugin hook timeout
(`service.rs:41`), the 13 `HookType` variants and their order, and the 4
`HookResult` fields.

Verified 2026-10-23 (second pass) against `src/hooks/mod.rs`,
`src/plugin/hooks.rs`, `src/plugin/policy.rs`, `src/plugin/permission.rs`,
`src/plugin/manifest.rs`, `src/managed_process.rs`,
`crates/egggit/src/process.rs`, `crates/codegg-config/src/schema.rs`,
`src/agent/loop.rs`, `src/agent/tool_batch.rs`, and
`scripts/check_daemon_cwd_usage.py`. Corrections made:

| Claim in the previous revision | Finding |
|---|---|
| "Plugin hooks **can block** execution (`ToolExecuteBefore`, `SessionCompacting`)" | **Wrong.** `classify_hook` (`src/plugin/policy.rs:110`) puts `SessionCompacting` in `HookCategory::Observation`, not `Blocking`. Only `ToolExecuteBefore` and `Auth` are `Blocking` — and blocking hooks are **off by default** (`PluginLifecyclePolicy::enable_blocking_hooks: false`, `policy.rs:34`), so out of the box no plugin hook can block. Corrected, with the runtime/trust gates added. |
| "WASM plugins register for `HookType` variants via their manifest (`[[hooks]]` entries, `type = "tool.execute.before"`)" | Incomplete. `[[hooks]]` is the **legacy** form; the canonical form is a `[[capabilities]]` entry with `type = "hook"` and `hook_type = "..."`. Legacy `[[hooks]]` entries are auto-converted to `PluginCapability::Hook` only when the manifest declares no capabilities at all (`manifest.rs:512-524`). |
| "`SessionEnd` fires once after the loop closes at `:2095`" | Wrong line. The `SessionEnd` hook context is built at `src/agent/loop.rs:2222` and dispatched at `:2235`; `:2095` is `self.drain_follow_up(...)`. Fixed. |
| "the loop opened at `src/agent/loop.rs:1110`" | Off by one: `loop {` is at `:1111`. Fixed. |
| "the runtime skip arm is `src/hooks/mod.rs:204-208`" | The `InlineScript` arm starts at `:205` (`:204` is the `#[allow(deprecated)]` attribute). Fixed. |
| "a config that uses either shape does not populate the registry" | Imprecise. Neither shape is a `HookConfigEntry` at all (it requires `event` plus the flattened `HookConfig`'s `type` tag), and `Config`'s `#[serde(default)]` covers only an **absent** `hooks` key, not a malformed entry. Reworded. |
| "the `codegg_git::ALLOWED_ENV_VARS` allowlist (21 names …)" | The 21 is the **Unix** count; on Windows the allowlist additionally carries `SystemRoot`, `WINDIR`, `TEMP`, `TMP` (`src/managed_process.rs:84-93`). Noted. |
| — | Added: the shell-hook **working directory** fact — `ShellCommandHook::execute` takes it from `std::env::current_dir()` (`src/hooks/mod.rs:118`), which in the user-scoped daemon is the launch directory, not the session `workspace_root`; recorded as an invariant and reported as a code issue (`scripts/check_daemon_cwd_usage.py` does not scan `src/hooks/`). |
| — | Added: a **coverage gap** in Testing — `src/hooks/mod.rs` has no `#[cfg(test)]` module and no integration test exercises the shell-hook registry, so `cargo test -p codegg -- hooks` matches only the plugin-hook tests. |

Re-confirmed as accurate: every `src/hooks/mod.rs` line ref (`HookEvent` `:16`,
`HookContext` `:55`, `Hook` trait `:93`, `ShellCommandHook` `:97`,
`HookRegistry` `:174`, `from_config` `:189`, `run_hooks` `:215`, `has_hooks`
`:227`, the 30s default `:107`, the 6 `CODEGG_*` variables `:67-83`), the
`ManagedProcessService::run` + `OutputPolicy::with_limits(64 KiB, 64 KiB)`
invocation, `EnvironmentPolicy::sanitized()` / `with_var`, the 21-name
allowlist and 28-name deny list in `crates/egggit/src/process.rs:33`/`:60`,
the six service-owned env invariants in `src/managed_process.rs:167-173`,
`Config::hooks` `schema.rs:287`, `HookConfigEntry` `:1340`, `HookConfig`
`:1358`, `InlineScript` `#[deprecated]` `:1366`, all six shell-hook call
sites, `HookType::as_str`/`parse` (`plugin/hooks.rs:23`/`:41`),
`into_plugin_invocation` (`:69`), the `dispatch_hook` chain semantics
(`service.rs:567-651`), the `{plugin_id}: hook timeout: …` prefix
(`service.rs:641`), and the 5s plugin hook timeout (`service.rs:41`).

