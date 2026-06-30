# Phase 6 Plan: Runtime Abstraction and Process Runtime Extraction

## Objective

Convert the current TUI-local process command execution into a real plugin runtime abstraction. This phase should introduce a runtime-neutral invocation layer that can execute process-backed plugin capabilities through `PluginService`, while preserving the already-working project-local `runtime: process` slash command path.

The end state is that process execution is no longer owned by `src/tui/commands/plugins.rs`. The TUI starts plugin commands, but execution is delegated to a runtime implementation that can later be used by TUI, core daemon, socket/stdio mode, tests, and installed plugin manifests.

## Current State

The first five phases implemented most of the planned foundation:

- `codegg_protocol::plugin` and `codegg_protocol::ui` exist.
- Dynamic slash commands support `runtime: process`.
- `src/tui/commands/plugins.rs` contains process spawning, timeout handling, stdout/stderr caps, JSON parsing, and response application.
- `PluginManifest` and `PluginRegistry` now model runtime plus capabilities.
- `PluginService::invoke_command()` exists, but it returns a local service-specific `PluginResponse` type rather than the protocol response with `UiEffect`s.

This phase should consolidate those seams before WASM work begins.

## Required Corrective Work Before Runtime Extraction

### 1. Unify plugin response types

`src/plugin/service.rs` currently defines its own local `PluginResponse` type with only `ok`, `data`, and `diagnostics`. Replace this with `crate::protocol::plugin::PluginResponse`, or add a short-lived adapter only if direct replacement causes excessive churn.

Preferred:

```rust
use crate::protocol::plugin::{PluginInvocation, PluginResponse};
use crate::protocol::ui::{ChatBlock, ChatFormat, UiEffect};
```

Remove the local `PluginResponse` struct from `src/plugin/service.rs` once all callers compile.

Acceptance:

- There is one canonical command response envelope: `codegg_protocol::plugin::PluginResponse`.
- Service-level command invocation can return UI effects.
- TUI response application can consume service/runtime responses without conversion.

### 2. Fix registry enable/duplicate semantics

`PluginRegistry` currently filters enabled plugins using a sync `try_read()` helper that defaults to enabled if it cannot acquire the lock. Replace this with async filtering that captures enabled plugin ids before filtering capability vectors, or store enabled state in an index that can be read safely.

Also fix duplicate command behavior across enable/disable transitions:

- registration should reject duplicates across all registered plugins, not only enabled plugins; or
- `set_enabled(plugin, true)` must revalidate that enabling the plugin will not create duplicate command/alias/panel/status ids.

Prefer strict global uniqueness for command names and aliases. It is simpler and safer.

Acceptance:

- Disabled plugins are never returned from capability queries.
- Re-enabling cannot silently introduce duplicate commands.
- Duplicate aliases are checked against names and aliases in both directions.
- Tests cover disabled-plugin duplicate edge cases.

### 3. Fix `unregister` return value

`PluginRegistry::unregister()` currently removes the plugin but returns `None`. Change it to return the removed `PluginInfo`.

Acceptance:

- `unregister()` returns `Some(info)` when the plugin existed.
- All capability indexes are cleaned up.
- Tests verify removed command/hook/panel/status/event entries disappear.

## Files to Add

### `src/plugin/runtime/mod.rs`

Introduce the runtime module.

Recommended exports:

```rust
pub mod process;

use async_trait::async_trait;
use crate::protocol::plugin::{PluginInvocation, PluginResponse};
use crate::plugin::manifest::PluginRuntimeSpec;

#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            max_stdout_bytes: 1024 * 1024,
            max_stderr_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("unsupported runtime: {0}")]
    Unsupported(String),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("runtime timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("process exited with code {code}: {stderr}")]
    NonZeroExit { code: i32, stdout: String, stderr: String },
    #[error("invalid response json: {0}")]
    InvalidJson(String),
    #[error("io error: {0}")]
    Io(String),
}

#[async_trait]
pub trait PluginRuntime: Send + Sync {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}
```

If the repo avoids `async_trait`, use boxed futures or an enum dispatcher. Do not add a new dependency if the workspace already has a preferred pattern.

### `src/plugin/runtime/process.rs`

Move process execution here from `src/tui/commands/plugins.rs`.

Recommended API:

```rust
pub struct ProcessRuntime {
    spec: ProcessRuntimeSpec,
    limits: RuntimeLimits,
}

pub struct ProcessRuntimeSpec {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: ProcessStdinMode,
    pub stdout: ProcessStdoutMode,
    pub timeout_ms: Option<u64>,
    pub cwd: Option<String>,
    pub env: Vec<String>,
}
```

Map from both:

- `crate::command::ProcessCommandSpec`;
- `crate::plugin::manifest::PluginRuntimeSpec::Process`.

Keep command execution direct. Do not invoke through a shell unless the spec explicitly asks for `sh`, `cmd`, `bash`, etc.

## Files to Modify

### `src/plugin/mod.rs`

Export `runtime`.

### `src/plugin/service.rs`

Update `PluginService::invoke_command()` to:

1. Look up the command registration.
2. Look up plugin info.
3. Build a canonical `PluginInvocation`.
4. Dispatch to the appropriate runtime.
5. Return `codegg_protocol::plugin::PluginResponse`.

For this phase, implement builtin and process behavior. WASM should return a clear unsupported/runtime-pending error or a structured response indicating Phase 7 is required.

### `src/tui/commands/plugins.rs`

Remove direct process spawning logic from this file. It should only:

- start a task;
- call the process runtime or `PluginService` path;
- post `PluginCommandFinished`;
- apply completed responses.

If project-local commands are not yet registered into `PluginRegistry`, use `ProcessRuntime` directly for those command specs. Installed plugin command manifests should go through `PluginService`.

### `src/command/mod.rs`

Keep `ProcessCommandSpec`, but add conversions into the process runtime spec. Do not duplicate runtime execution.

### `Cargo.toml`

Add `async-trait` only if necessary and not already present. Prefer existing project patterns if available.

## Invocation Context Improvements

The current process invocation identifies plugin id as `cmd:<executable>` and command name as the executable. Improve this.

Add enough metadata so process scripts can know what they are:

- slash command name, e.g. `quota`;
- command source path/config source;
- executable path;
- raw args;
- project dir;
- session id if available;
- current model and agent if available;
- protocol version.

For project-local commands, use a stable id such as:

```text
command:<source-path-or-config>:<command-name>
```

For installed plugin commands, use:

```text
plugin:<plugin-name>:<command-name>
```

Do not expose secrets by default. Environment variables should still only include explicit entries from config/manifest.

## Process Output Semantics

Move the current stdout parsing behavior into `ProcessRuntime`:

- `text`: return a `PluginResponse` with an `EmitChat` or fallback UI effect, or return text as data and let caller decide.
- `json`: require a valid `PluginResponse` JSON body.
- `auto`: parse JSON if possible, otherwise convert text into a standard `PluginResponse` with a plain/markdown chat or dialog effect.

Preferred: runtime returns a full `PluginResponse` for all successful paths. TUI should not need separate `stdout` success handling long-term.

For nonzero exit, return `RuntimeError::NonZeroExit { code, stdout, stderr }`; the caller can convert that into a failed `PluginResponse` or TUI error completion.

## Test Requirements

Add runtime-level tests that do not require a full TUI:

- text stdout becomes a successful `PluginResponse`;
- JSON stdout becomes the exact structured `PluginResponse`;
- auto mode falls back to text;
- JSON mode fails on invalid JSON;
- nonzero exit preserves stdout and stderr in `RuntimeError`;
- timeout fails deterministically;
- stdout/stderr caps are enforced;
- explicit env values are passed;
- cwd override is applied;
- no shell expansion occurs by default.

Avoid Unix-only commands where possible. Prefer a small test helper binary or current-exe fixture. If the existing test harness cannot support that quickly, isolate Unix-only tests behind cfg gates and add a follow-up note.

Add service-level tests:

- registering a process command plugin and invoking it routes to `ProcessRuntime`;
- disabled plugin command invocation fails;
- duplicate command registration is rejected globally;
- unregister returns removed info and cleans capability indexes.

Add TUI-level smoke tests:

- project-local process command still starts through `TuiCommand::PluginCommandRun`;
- completion response still applies effects.

## Acceptance Criteria

- There is a `src/plugin/runtime` module with a process runtime implementation.
- Process execution code is no longer duplicated in `src/tui/commands/plugins.rs`.
- `PluginService::invoke_command()` returns the protocol `PluginResponse` type.
- Project-local process commands still work.
- Registry enable/disable and duplicate semantics are hardened.
- `unregister()` returns the removed plugin info.
- Tests cover process runtime, registry edge cases, service invocation, and TUI completion application.

## Non-Goals

- Do not modernize WASM execution in this phase.
- Do not wire lifecycle hooks into core paths yet.
- Do not add plugin management slash commands.
- Do not add PyO3.
- Do not claim process plugins are sandboxed.

## Handoff Notes for Phase 7

After this phase, WASM modernization should implement the same `PluginRuntime` trait. The WASM runtime should consume `PluginInvocation` and emit protocol `PluginResponse`, matching the process runtime. Any remaining local response adapters should be removed before Phase 7 begins.
