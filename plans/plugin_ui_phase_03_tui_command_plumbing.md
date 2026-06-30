# Phase 3 Plan: Generic TUI Command Plumbing for Plugin Effects

## Objective

Add the TUI command and async dispatch plumbing needed to run plugin-backed slash commands and apply plugin UI responses without blocking the render loop.

This phase should not yet implement real process/WASM runtime execution. It should introduce the generic TUI command variants, command handlers, response application path, and tests using mocked or synthetic `PluginResponse` values.

## Architectural Context

The TUI already uses a spawn-and-complete pattern for high-latency work. `TuiCommand` starts work, a task posts a typed completion variant, and `runtime/command_dispatch.rs` applies the result synchronously. Plugin execution must follow this pattern.

The current command dispatcher is already large. Avoid adding one `TuiCommand` variant per plugin command or per plugin UI surface. Add a small generic set of variants and centralize plugin command behavior in a dedicated handler module.

## Files to Modify

### `src/tui/app/mod.rs`

Add generic plugin command variants to `TuiCommand`.

Recommended shape:

```rust
PluginCommandRun {
    command: String,
    args: Vec<String>,
},
PluginCommandFinished {
    invocation_id: String,
    command: String,
    response: Option<Box<crate::protocol::plugin::PluginResponse>>,
    stdout: Option<String>,
    stderr: Option<String>,
    error: Option<String>,
},
PluginUiEffect {
    effect: crate::protocol::ui::UiEffect,
},
```

Use `Box<PluginResponse>` if enum-size warnings become an issue.

Do not include runtime-specific fields here. `TuiCommand` should not care whether a command is template, process, WASM, or builtin.

### `src/tui/commands/plugins.rs`

Add the command handler module.

Recommended functions:

```rust
pub fn start_plugin_command(app: &mut App, command: String, args: Vec<String>);

pub fn apply_plugin_command_finished(
    app: &mut App,
    invocation_id: String,
    command: String,
    response: Option<Box<PluginResponse>>,
    stdout: Option<String>,
    stderr: Option<String>,
    error: Option<String>,
);

pub fn apply_plugin_ui_effect(app: &mut App, effect: UiEffect);
```

In this phase, `start_plugin_command` can be a stub that posts a controlled unsupported/not-wired result or calls a mock/test-only path. The important part is that the dispatch/apply path is correct and non-blocking.

### `src/tui/commands/mod.rs`

Export the new `plugins` module.

### `src/tui/runtime/command_dispatch.rs`

Add dispatch arms:

```rust
TuiCommand::PluginCommandRun { command, args } => {
    start_plugin_command(app, command, args);
}
TuiCommand::PluginCommandFinished { ... } => {
    apply_plugin_command_finished(app, ...);
}
TuiCommand::PluginUiEffect { effect } => {
    apply_plugin_ui_effect(app, effect);
}
```

Keep dispatch non-blocking.

### `src/tui/app/state/plugin_ui.rs`

If Phase 2 added only basic effect state, extend it as needed to support command completion application.

Recommended additions:

- last invocation id;
- in-flight command map if needed;
- last command error;
- helper to apply a `PluginResponse` by iterating effects.

Avoid turning this into runtime execution state. Execution belongs in Phase 4/6.

## Response Application Rules

`apply_plugin_command_finished` should be deterministic:

1. If `error` is present, show an error toast and optionally an info dialog with stderr/stdout diagnostics.
2. If `response` is present and `response.ok == true`, apply each `UiEffect` in order.
3. If `response` is present and `response.ok == false`, apply any diagnostic effects but show an error/warning toast.
4. If no structured response exists but `stdout` exists, render stdout as chat/plain text or info dialog depending on length.
5. If only `stderr` exists, render as warning/error diagnostics.
6. If nothing exists, show a concise “plugin command produced no output” warning.

Reuse existing `show_short_or_info` behavior where available for long output.

## Synthetic Execution for This Phase

Because real command plugin runtime begins in Phase 4, `start_plugin_command` should not try to discover or execute scripts yet. Good options:

- Return a clear `not_implemented` completion for unknown commands.
- Add a test-only helper that directly sends `PluginCommandFinished`.
- Add an internal `debug`/`cfg(test)` path to validate applying sample responses.

Do not wire plugin command names into the slash command registry yet unless a minimal hidden test seam is needed.

## Tests

Add tests in the TUI command tests area or a new test module for `commands::plugins`.

Cover:

- structured `PluginResponse` with `ShowToast` adds a toast;
- structured `PluginResponse` with `OpenDialog` adds plugin dialog state;
- multiple effects apply in order;
- error completion shows error toast;
- stdout fallback creates chat/info output;
- stderr fallback does not panic;
- empty completion shows a warning;
- `PluginUiEffect` dispatch applies directly;
- dispatch remains usable from unit tests without spawning real processes.

Where possible, use `App::new_for_testing`.

## Acceptance Criteria

- `TuiCommand` has generic plugin command/effect variants.
- `src/tui/commands/plugins.rs` exists and owns plugin response application.
- `runtime/command_dispatch.rs` routes plugin variants.
- Synthetic `PluginResponse` values can produce toasts/dialog state/chat fallback.
- No process or WASM execution is introduced yet.
- Command dispatch remains non-blocking.
- Tests cover success, error, stdout fallback, and UI effect application.

## Non-Goals

- Do not extend dynamic command discovery yet.
- Do not execute process scripts.
- Do not modernize the old WASM loader yet.
- Do not add plugin installation commands.
- Do not make plugins intercept tools/messages.

## Risks and Mitigations

### Risk: Large `TuiCommand` enum growth

Mitigation: add only generic plugin variants. Use boxed response payloads if enum size becomes an issue.

### Risk: Plugin output bypasses state model

Mitigation: all structured UI output should go through `apply_plugin_ui_effect` and `PluginUiState`, not ad-hoc dialog fields.

### Risk: Long stdout becomes unreadable toast

Mitigation: use existing short-or-info behavior. Toasts are for short feedback only.

## Handoff Notes for Phase 4

Phase 4 should call `TuiCommand::PluginCommandRun` from the slash command path when a dynamic command has `runtime: process`. The actual process execution can then post `PluginCommandFinished` using the exact response application path introduced here.
