# TUI Phase 11: Runtime Module Decomposition

## Objective

Split the large TUI runtime implementation into focused modules without changing behavior. Recent work improved correctness and responsiveness, but it also added many start/apply handlers and completion variants. `src/tui/mod.rs` is now carrying too many responsibilities: terminal entrypoint, event loop, command dispatch, app-event application, shell handlers, async command start/apply functions, render recovery, and domain-specific glue.

This phase is a controlled decomposition pass. It should make future changes safer and smaller while preserving the current event-loop model.

## Current Problem

A large central TUI runtime file makes it hard to reason about:

- which handlers are allowed to await
- which handlers only start background work
- where UI state mutation occurs
- where stale completions are applied
- which code is event-bus handling versus command handling
- which code is domain behavior versus runtime plumbing

The file is also a merge-conflict magnet. As more TUI features are added, the command enum and event loop will keep growing unless the dispatch logic is modularized.

## Design Constraints

1. Do not change behavior in the first decomposition patch.
2. Keep `run_event_loop(app)` as the public entrypoint unless there is a strong reason to rename it.
3. Preserve the rule: background tasks perform work; typed completions mutate UI state on the TUI thread.
4. Avoid moving code across crate visibility boundaries in ways that force broad `pub` leakage.
5. Keep the module tree easy for agents to navigate.

## Proposed Module Layout

Suggested layout:

```text
src/tui/
  mod.rs                       # public exports + run_event_loop entrypoint shim
  runtime/
    mod.rs                     # run_event_loop and runtime constants
    event_loop.rs              # select loop and render cadence
    render_recovery.rs         # root render panic handling helpers
    command_dispatch.rs        # TuiCommand match dispatcher
    app_events.rs              # AppEvent bus handling/coalescing
    terminal_runtime.rs        # create_terminal integration if not in terminal.rs
  commands/
    mod.rs
    sessions.rs                # reload/session mutations/share/export/tree/session messages
    import.rs                  # preview/confirm import
    research.rs                # research browser async start/apply
    memory.rs                  # memory start/apply
    goals.rs                   # goal operations
    tasks.rs                   # task/worktree/template/notification
    shell.rs                   # shell run/include/ask/list/show/kill/rerun
    security.rs                # security-review dispatch/apply
    diagnostics.rs             # doctor/tui-stats command glue
```

This is a target structure, not a requirement to land all movement in one commit. If Rust visibility gets noisy, use fewer modules initially:

```text
runtime.rs
command_handlers.rs
app_event_handlers.rs
shell_handlers.rs
```

Then split further later.

## Migration Strategy

### 1. Add module shells first

Create new files with empty modules and re-export nothing. This makes the intended structure visible.

### 2. Move pure helpers first

Move helpers that do not require many private fields:

- `format_system_time`
- `format_shell_status`
- `latest_user_message_text`
- render panic message extraction if it is not app-private
- small apply helpers with narrow signatures

### 3. Move shell handlers

Shell handlers are relatively domain-contained:

- `handle_run_human_shell`
- `handle_shell_event`
- `handle_shell_include`
- `handle_shell_ask`
- `handle_shell_rerun`
- `handle_shell_kill`
- `handle_shell_list`
- `handle_shell_show`

Place in `src/tui/commands/shell.rs`.

Keep function signatures as:

```rust
pub(super) fn handle_shell_list(app: &mut App) { ... }
```

Use `pub(super)` or `pub(crate)` only as needed.

### 4. Move session command handlers

Move session reload, mutation, share, export, tree, and message-load handlers into `commands/sessions.rs`.

This includes:

- `start_reload_sessions`
- `apply_sessions_reloaded`
- `start_delete_session`
- `start_archive_session`
- `start_fork_session`
- `start_bulk_delete`
- `start_bulk_archive`
- `start_bulk_export`
- `start_rename_session`
- `start_undo_delete`
- `apply_session_mutation_finished`
- `start_share_session`
- `apply_share_session_finished`
- `start_unshare_session`
- `apply_unshare_session_finished`
- `start_export_session`
- `apply_export_session_finished`
- `start_load_session_messages`
- `apply_session_messages_loaded`
- `start_open_tree_dialog`
- `apply_tree_dialog_loaded`

### 5. Move research/import/memory/goal/task handlers

Move by domain in separate commits if possible. Each domain should expose only the start/apply functions needed by dispatcher.

### 6. Extract command dispatcher

Replace the giant `match cmd` inside the event loop with a function:

```rust
async fn dispatch_tui_command(app: &mut App, cmd: TuiCommand) {
    match cmd { ... }
}
```

Initially this function can still live in `runtime/command_dispatch.rs` and call domain handlers. This will shrink the event loop and make residual direct awaits easier to audit.

### 7. Extract app-event handling

Move the `AppEvent` coalescing and match body to:

```rust
async fn handle_app_event_batch(app: &mut App, events: Vec<AppEvent>) -> bool
```

Return `needs_render` as a boolean or a small enum:

```rust
pub enum EventHandlingResult {
    NoRender,
    NeedsRender,
    Exit,
}
```

This separates bus semantics from command semantics.

### 8. Extract render cadence/recovery helpers

Keep root render call in the event loop, but move helper functions/constants into `runtime/render_recovery.rs` if it improves readability:

- max render panic threshold
- render duration diagnostics
- render error handling
- panic recovery state reduction

Component-level guard logic inside `App::render` can remain in app module unless a reusable render guard is introduced later.

## Visibility Guidelines

Use the narrowest visibility that compiles:

- `pub(super)` for handlers used by sibling runtime modules
- `pub(crate)` only when cross-tree access is genuinely needed
- avoid `pub` unless the function is part of the external crate API

If moving a handler forces many app fields to become public, stop and reassess. It may be better to keep that handler in place or add a narrow method on `App`.

## Naming Guidelines

Use consistent prefixes:

- `start_*` for initiating background work
- `apply_*` for completion/UI-state mutation
- `handle_*` for synchronous local event handling
- `build_*` for pure DTO construction
- `format_*` for presentation helpers

Do not mix `handle_*` for both async start and apply paths.

## Testing Plan

Because this is intended as behavior-preserving refactor, tests should focus on regression prevention:

- Existing workspace tests must pass unchanged.
- Add one small compile-time module test per new module if useful.
- If moving shell/session handlers, keep or move existing shell dispatch tests.
- After extracting dispatcher, add tests for representative commands mapping to expected start/apply behavior if feasible.

Manual smoke tests:

1. Run TUI and submit a prompt.
2. Trigger session reload, delete/archive, and load messages.
3. Run shell command, include output, kill long command.
4. Open import/research/session dialogs.
5. Trigger file-change sidebar updates.
6. Quit and verify terminal restore.

## Acceptance Criteria

- `src/tui/mod.rs` is reduced to public module exports plus the high-level runtime entrypoint or a thin shim.
- Command dispatch is separated from event-bus handling.
- Major command domains are moved into focused modules.
- No behavior changes are introduced intentionally.
- No broad public visibility leakage is introduced.
- Existing and new tests pass.
- Workspace checks pass.

## Out of Scope

- Rewriting the event loop architecture.
- Changing TUI command semantics.
- Introducing a plugin/frontend protocol.
- Replacing component render code.
- Full background task lifecycle changes beyond preserving existing calls.
