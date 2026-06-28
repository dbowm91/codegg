# TUI Remaining Items Closure Plan

## Context

The recent TUI pass substantially improved the shape of the repo. The TUI now has async command scaffolding, async sidebar diff computation, a terminal lifecycle guard, diagnostics state, mode-aware help metadata, and better shell digest/exit-code handling.

However, the implementation is not fully closed. The main remaining items are:

1. Several core-backed mutation handlers are still awaited directly on the TUI event loop.
2. Render panic recovery is less destructive than before, but it is still root-level rather than component-level.
3. Shell kill semantics no longer leave commands running forever, but killed commands are represented as generic `Exited` with `None` exit code.
4. Feature-gated input debug logging can still write `codegg_debug.log` into the current working directory.
5. Verification coverage should be extended so these fixes stay closed.

This plan is a corrective closure pass. It should be implemented incrementally and should avoid changing product behavior beyond responsiveness, correctness, diagnostics, and cleanup.

## Goals

- Remove remaining avoidable awaits from the event-loop command arm.
- Preserve single-owner UI mutation through typed `TuiCommand` completions.
- Add component-level render fallbacks for the riskiest render surfaces.
- Represent killed shell commands distinctly from normal process exit.
- Remove project-directory debug-log writes from the TUI path, even behind debug feature flags, unless explicitly configured.
- Add targeted tests for stale completions, nonblocking command initiation, render fallback, shell killed state, and debug-log path behavior.

## Non-Goals

- Do not rewrite the TUI or replace ratatui.
- Do not implement the later full background-task lifecycle manager from the broader roadmap.
- Do not redesign the remote TUI protocol.
- Do not move every long toast into a polished dialog in this pass.
- Do not alter core protocol semantics unless needed for typed command completions.

---

## Workstream 1: Finish Event-Loop Responsiveness for Mutation Handlers

### Problem

The previous pass moved many read/load operations to `start_*` plus completion commands. Remaining mutation handlers are still awaited directly from the event loop, including session delete, archive, fork, bulk delete, bulk archive, bulk export, share, unshare, export, rename, undo-delete, template creation, task operations, worktree list, refresh session state, compact session, goal operations, notifications, and possibly security-review legacy dispatch.

Not every one of these is equally urgent, but any handler that calls `core_client.request(...).await`, performs filesystem work, clipboard work, notification work, or loops over multiple core requests can still freeze the TUI.

### Design

Use the existing `spawn_tui_task` pattern. For each converted command:

1. The event-loop match arm calls `start_*` synchronously.
2. `start_*` captures immutable inputs and `tui_cmd_tx`.
3. `start_*` sets immediate loading/disabled/pending state where applicable.
4. The spawned task performs core or blocking work.
5. The spawned task sends a typed completion command.
6. `apply_*` handles UI state mutation, toasts, dialog updates, and optional reload chaining.

Do not mutate `App` from spawned tasks.

### Priority Order

#### 1. Session list mutation commands

Convert first because they are visible and frequently used:

- `DeleteSession`
- `ArchiveSession`
- `ForkSession`
- `BulkDelete`
- `BulkArchive`
- `BulkExport`
- `RenameSession`
- `UndoDelete`

Add completion variants such as:

```rust
TuiCommand::SessionMutationFinished {
    request_id: u64,
    op: SessionMutationOp,
    affected_ids: Vec<String>,
    message: String,
    reload_after: bool,
    error: Option<String>,
}
```

Where:

```rust
#[derive(Debug, Clone)]
pub enum SessionMutationOp {
    Delete,
    Archive,
    Unarchive,
    Fork,
    BulkDelete,
    BulkArchive,
    BulkUnarchive,
    BulkExport,
    Rename,
    UndoDelete,
}
```

If adding a shared enum is too invasive, use narrower completion variants. The shared variant is preferred to reduce enum growth.

Completion application should:

- Clear pending/loading state.
- Show success/error toast.
- Trigger `start_reload_sessions(app)` if `reload_after` is true.
- Avoid calling old `reload_sessions(app).await`.

Bulk operations should execute in the spawned task and aggregate errors. The UI should not toast success blindly if some operations failed. Suggested behavior:

- All success: `"N sessions archived"`.
- Partial failure: warning toast with `"Archived X/Y sessions; Z failed"`.
- Full failure: error toast.

#### 2. Share/unshare/export

Convert:

- `ShareSession`
- `UnshareSession`
- `ExportSession`

Use typed completions:

```rust
TuiCommand::ShareSessionFinished { session: Option<SessionDto>, error: Option<String> }
TuiCommand::UnshareSessionFinished { session: Option<SessionDto>, error: Option<String> }
TuiCommand::ExportSessionFinished { json: Option<String>, error: Option<String> }
```

Clipboard copy for export can be handled in one of two ways:

- Preferred: perform JSON serialization in task, return string, then copy on UI thread only if clipboard call is cheap and current code already does this.
- Safer: perform clipboard copy in `spawn_blocking` inside the task and return success/error.

Do not block the event loop on clipboard APIs if they can hang on the platform.

#### 3. Goal commands

Convert the goal handlers that call core:

- `GoalSet`
- `GoalFromFile`
- `GoalShow`
- `GoalPause`
- `GoalResume`
- `GoalClear`
- `GoalDone`
- `GoalCheckpoint`
- `GoalBudget`
- `RefreshSessionState`

Suggested completion:

```rust
TuiCommand::GoalOperationFinished {
    session_id: String,
    op: String,
    response: Option<CoreResponse>,
    error: Option<String>,
}
```

For `GoalFromFile`, file reading should happen in the spawned task or `spawn_blocking` if it is synchronous. Apply results only if the relevant session is still active where appropriate.

#### 4. Tasks/worktree/compact/template/notification

Convert remaining direct awaits where they may perform I/O or core requests:

- `CreateFromTemplate`
- `ListTasks`
- `DeleteTask`
- `TaskSchedule`
- `WorktreeList`
- `CompactSession`
- `SendNotification`
- `OpenDiffDialog` only if it performs expensive work; if it only opens a dialog from supplied boxed strings, it can remain direct.

Notification sending should be `spawn_blocking` or routed through the existing notification manager in a background task.

### Remove or quarantine old async handlers

After each conversion, do one of the following:

- Delete the old `async fn handle_*` if no longer used.
- Mark it `#[cfg(test)]` only if needed by tests.
- Keep only pure helper functions that do not take `&mut App` across awaits.

Avoid accumulating both old `handle_*().await` and new `start_*` paths for the same operation.

### Tests

Add tests around converted command paths where feasible:

- Starting a mutation sets a pending/loading flag and does not require awaiting core.
- Completion with success shows success toast and triggers reload via `ReloadSessions`/`start_reload_sessions` path, not direct await.
- Completion with error clears pending state and does not trigger reload unless explicitly desired.
- Bulk partial failures produce warning text.
- Stale completion is ignored if request IDs are used.

If a fake core client is too heavy for unit tests, split task bodies into pure async helper functions that accept a trait/mockable client or cover the application side with synthetic completion commands.

### Acceptance Criteria

- No `cmd_rx` match arm directly awaits core-backed session mutation handlers.
- No converted handler calls `reload_sessions(app).await`; all reloads go through `start_reload_sessions(app)` or `TuiCommand::ReloadSessions`.
- Bulk operations do not block the TUI loop and report partial failures accurately.
- Slow mutation operations can run while input, resize, toasts, and render ticks continue.

---

## Workstream 2: Component-Level Render Fallbacks

### Problem

Render recovery is still centered around `catch_unwind` for the whole `render_app` call. This is better than crashing, and the reset is now less destructive, but a single bad surface can still force the whole render-error path.

### Design

Add component-level fallback wrappers for dynamic/high-risk surfaces. Root-level catch remains as final protection.

Target surfaces:

- message timeline / messages widget
- sidebar
- active dialog
- completion overlay
- research browser dialog if rendered through dialog path
- security-review dialog if rendered through dialog path
- shell-show/info dialog if rendered through dialog path
- toasts, if toast rendering can panic on pathological content

### Implementation Options

Preferred approach:

1. Add a helper in a new module such as `src/tui/render_guard.rs`.
2. The helper catches panic around a closure that renders one surface.
3. On failure, it logs through `tracing`, records diagnostics, and draws a small fallback block in the same area.

Example shape:

```rust
pub fn render_guarded<F>(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    name: &'static str,
    diagnostics: &mut TuiDiagnostics,
    render: F,
) where
    F: FnOnce(&mut ratatui::Frame) + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(frame))) {
        Ok(()) => {}
        Err(err) => {
            diagnostics.record_component_render_panic(name);
            render_component_fallback(frame, area, name);
            tracing::error!(component = name, "TUI component render panic");
        }
    }
}
```

Borrowing around `Frame` and `&mut App` may require a less generic implementation. If so, use local `catch_unwind` blocks inside `App::render` around each surface.

### Diagnostics additions

Extend `TuiDiagnostics` with:

```rust
pub component_render_panic_count: u64,
pub recent_component_render_panics: VecDeque<ComponentRenderPanicRecord>,
```

Where each record includes component name and timestamp.

### Fallback rendering

Use compact fallback blocks:

- messages: `Messages render error`
- sidebar: `Sidebar unavailable`
- dialog: close/hide dialog or render `Dialog render error`
- completions: hide completions and continue
- toasts: skip toasts for that frame and log

Do not render large panic payloads. Use short UI text and structured logs.

### Tests

- Add a test-only panic injection path for at least one widget or surface.
- Verify a component panic increments component panic diagnostics.
- Verify the frame still renders fallback content and root panic count does not increment for guarded component panics.
- Keep root-level panic recovery tests for failures outside guarded surfaces.

### Acceptance Criteria

- At least messages, sidebar, active dialog, and completion overlay have guarded render boundaries.
- A guarded component panic does not trigger the root render panic path.
- Component render failures are visible in diagnostics and logs.
- Root render catch remains as final fallback.

---

## Workstream 3: Shell Killed-State Semantics

### Problem

`handle_shell_kill` currently removes the handle, calls `kill()`, and marks the entry as `Exited` with `exit_code = None` and zero elapsed. This prevents commands from remaining stuck as running, but it erases the difference between a process that exited normally with no code and a command killed by the user.

### Design

Add a distinct shell status:

```rust
pub enum ShellStatus {
    Running,
    Exited,
    TimedOut,
    FailedToStart,
    Killed,
}
```

Add store method:

```rust
pub fn mark_killed(&mut self, id: ShellCommandId, elapsed: Duration)
```

The elapsed duration should be calculated from `started_at` if possible. Do not use zero unless the start time is unavailable.

### Runtime/event handling

Review shell runtime events. If killing a handle can later produce an exited event, ensure late events do not overwrite `Killed` incorrectly. Options:

- If entry is already `Killed`, ignore later `Exited` events from that command.
- Or preserve `Killed` as user-facing status but store late raw exit details separately if desired.

### Digest behavior

Update `ShellDigest::build_from_entry` or render path so killed commands are not treated as success. Since `ShellDigest` currently carries only `exit_code`, add status context or a failure reason.

Preferred:

```rust
pub struct ShellDigest {
    pub status: ShellStatus,
    ...
}
```

Then add generic failure extraction for `Killed`, `TimedOut`, and `FailedToStart` even when no exit code exists.

Minimal acceptable version:

- Keep `ShellDigest` shape but add an extracted failure in `build_from_entry` for killed/timed out/failed-to-start entries.

### UI updates

Update:

- shell list: show `killed 12.3s $ cmd`
- shell show dialog: show `Status: killed`
- shell ask/include digest: include killed status and do not imply success

### Tests

- `mark_killed` sets status to `Killed`, finished time, elapsed, and keeps exit code `None`.
- Killed entries render as killed in shell list.
- Killed shell ask/include digest includes killed status/failure.
- Late exited event does not overwrite killed status if that race is possible.

### Acceptance Criteria

- Killed shell commands are distinct from normal exited commands.
- Killed commands do not remain running.
- Killed commands do not render as successful or ambiguous in digest/context.

---

## Workstream 4: Debug Logging Sink Cleanup

### Problem

The unconditional `debug_log!` in `src/tui/mod.rs` has been removed, but `src/tui/input.rs` still has a feature-gated debug macro that writes `codegg_debug.log` in the current working directory when `debug-logging` is enabled.

Feature-gated debug logging is better than unconditional logging, but writing into the user's project directory is still undesirable.

### Design

Replace file-writing debug macros with `tracing::debug!` by default. If file logging is required, route it through a configured sink or app data/cache path.

Preferred change:

```rust
#[cfg(feature = "debug-logging")]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        tracing::debug!(target: "codegg::tui::input", "{}", format!($($arg)*));
    };
}
```

Remove `OpenOptions` and `Write` imports from `src/tui/input.rs` unless another explicit debug-file system exists.

Optional configurable file sink:

- Use `CODEGG_TUI_DEBUG_LOG=/path/to/log` only if explicitly set.
- Otherwise log through `tracing`.
- If app cache paths already exist, default there only in debug builds.

### Tests/static checks

- Add a test or static assertion if feasible that no TUI module contains `open("codegg_debug.log")` or `OpenOptions` for debug file output.
- At minimum, add a repo-level comment or doc note in `.opencode/skills/tui/SKILL.md` that TUI debug logging should go through `tracing`, not project-directory files.

### Acceptance Criteria

- `src/tui/input.rs` no longer writes `codegg_debug.log` directly.
- TUI debug messages use `tracing` or an explicitly configured file path.
- Normal and debug-feature builds do not create debug logs in arbitrary project directories unless a user explicitly configured that path.

---

## Workstream 5: Verification and CI Confidence

### Problem

GitHub did not report combined CI statuses during the repo inspection. The code has many tests, but this closure pass should add verification that directly protects the newly fixed areas.

### Required checks

Run locally or in CI:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If all-features is not currently viable, document the failing feature combination and run the strongest existing workspace test matrix.

### Targeted tests to add

#### Event-loop responsiveness/application tests

- Converted mutation completion applies success toast and reload trigger.
- Converted mutation completion applies error toast and clears pending state.
- Bulk partial failure renders warning summary.
- Starting a converted mutation does not require awaiting the core request.

#### Render fallback tests

- Guarded messages/sidebar/dialog/completion panic records component diagnostics and renders fallback.
- Root render panic path still works for unguarded catastrophic failures.

#### Shell tests

- `ShellStatus::Killed` is stored and rendered.
- Killed digest is non-successful.
- Late exit does not overwrite killed if applicable.

#### Debug logging tests/static check

- No direct `codegg_debug.log` write remains in TUI modules.

### Manual smoke tests

Run in a real terminal and inside `zellij`:

1. Open TUI and trigger session reload while pressing keys/resizing.
2. Delete/archive/rename a session with a deliberately slow core path if possible; verify input/render remains responsive.
3. Trigger file-change events for a large file and confirm sidebar shows skipped/pending without freezing.
4. Run a long shell command, kill it, then check `/shell-list`, `/shell-show`, and `/shell-ask` output.
5. Enable debug logging and verify no `codegg_debug.log` appears in the project directory.
6. If panic injection exists, trigger a component panic and verify the rest of the UI survives.

### Acceptance Criteria

- All targeted tests pass.
- Standard formatting/lint/test commands pass or documented exceptions are added to the handoff notes.
- Manual smoke test does not reveal terminal restoration, responsiveness, or shell-status regressions.

---

## Suggested Implementation Order

1. Replace input debug file macro with `tracing`.
2. Add `ShellStatus::Killed`, `mark_killed`, shell-list/show/digest updates, and tests.
3. Convert remaining session mutation handlers to start/completion commands.
4. Convert share/unshare/export/rename and clipboard-sensitive paths.
5. Convert goal/task/worktree/compact/template/notification direct awaits where practical.
6. Add component-level render fallback wrappers for messages/sidebar/dialog/completion.
7. Add verification tests and run the workspace checks.

This order closes simple correctness issues first, then tackles the larger event-loop cleanup, then hardens render behavior.

## Final Handoff Checklist

- [ ] No direct `reload_sessions(app).await` remains in event-loop command handlers.
- [ ] No direct core-backed mutation handler is awaited from `cmd_rx` for session mutations.
- [ ] Converted operations return typed completion commands.
- [ ] Bulk mutation results distinguish success, partial failure, and full failure.
- [ ] Component render fallback protects at least messages, sidebar, dialog, and completion overlay.
- [ ] Shell killed state is distinct and tested.
- [ ] TUI debug logging does not write `codegg_debug.log` in the project directory.
- [ ] Diagnostics include component render failures.
- [ ] Targeted tests and workspace checks pass.
