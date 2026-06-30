# TUI Phase 7: Background Task Lifecycle Cleanup

## Implementation Status

Implemented in `src/tui/task_lifecycle.rs`. The production registry stores
`AbortHandle`s, not `JoinHandle`s or cancellation tokens, so cancellation is
abort-based. This keeps shutdown and dialog-close behavior simple and bounded,
but it also means the registry cannot currently distinguish panics from aborts;
`panicked_count` is reserved for a future JoinHandle-backed design.

## Objective

Centralize ownership and shutdown semantics for TUI-side background tasks. The TUI now uses spawned tasks for async command work, sidebar diffing, shell execution, notifications, research loading, and other nonblocking paths. That is the right responsiveness model, but the next step is making those tasks visible, cancellable, and bounded so future daemon/multi-session work does not accumulate orphaned work.

## Current Shape

The TUI has several sources of background work:

- async command completions spawned through `spawn_tui_task`
- sidebar diff tasks spawned through `file_diff::spawn_sidebar_diff_stats`
- shell command handles stored in `app.shell_handles`
- config watcher tasks
- file indexer / indexed-file refresh task
- notification work, sometimes through blocking tasks
- memory consolidation spawned after agent completion
- security review background dispatch
- research loading tasks

Some of this is intentionally fire-and-forget. Some should be cancelled when a dialog is closed, when a newer request supersedes it, or when the TUI exits. The current generation/request-id model protects UI state from stale completions, but it does not prevent wasted work or enforce shutdown.

## Design Goals

1. Keep UI mutation on the TUI event loop.
2. Track TUI-owned background tasks in one place.
3. Abort or invalidate tasks on app shutdown.
4. Keep cancellation semantics explicit: the current registry aborts tasks; any future cooperative cancellation must be layered intentionally.
5. Keep bounded concurrency for expensive classes of work.
6. Do not over-engineer a full task runtime; this phase should provide practical lifecycle hygiene.

## Proposed Data Model

Add a task registry under TUI app state, for example:

```rust
pub struct TuiTaskRegistry {
    next_id: u64,
    tasks: std::collections::HashMap<TuiTaskId, TuiTaskRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TuiTaskId(pub u64);

pub struct TuiTaskRecord {
    pub name: &'static str,
    pub kind: TuiTaskKind,
    pub started_at: std::time::Instant,
    pub abort_handle: tokio::task::AbortHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiTaskKind {
    Command,
    FileDiff,
    Shell,
    Research,
    Memory,
    Notification,
    SecurityReview,
    Indexer,
    Other,
}
```

The implemented registry uses `AbortHandle` only. A JoinHandle-backed design
would be needed before panic accounting can become meaningful.

## Implementation Steps

### 1. Introduce a task registry

Place it in `src/tui/task_lifecycle.rs` or under `src/tui/app/state/tasks.rs`. Add it to `App` or `UiState` depending on ownership preference. `App` is probably more appropriate because this is runtime state, not render state.

Expose operations:

```rust
impl TuiTaskRegistry {
    pub fn spawn<F>(&mut self, kind: TuiTaskKind, name: &'static str, fut: F) -> TuiTaskId
    where
        F: Future<Output = ()> + Send + 'static;

    pub fn cancel(&mut self, id: TuiTaskId);
    pub fn cancel_kind(&mut self, kind: TuiTaskKind);
    pub fn cancel_all(&mut self);
    pub fn reap_finished(&mut self);
    pub fn active_count(&self) -> usize;
}
```

The registry must not require mutable access after every task completion. Reaping can happen periodically from the event loop or when `/tui-stats` is requested.

### 2. Extend `spawn_tui_task`

Replace or overload `spawn_tui_task` so it can register tasks. A minimal migration path:

- Keep current `spawn_tui_task` for compatibility.
- Add `spawn_registered_tui_task(app, kind, name, fut)` that uses the registry.
- Gradually migrate start functions to the registered variant.

Completion send failures should still be logged at debug level and should not panic.

### 3. Add shutdown path

Add an explicit `App::shutdown_background_tasks()` or `App::prepare_shutdown()` method that:

- cancels registered tasks
- kills running shell commands or marks them killed according to policy
- stops config watcher if applicable
- stops file indexer task if applicable
- clears command sender if needed

Call this before leaving `run_event_loop`, and ensure the terminal guard still restores even if shutdown has errors.

### 4. Dialog cancellation/invalidation

For dialog-scoped tasks, cancellation should happen when the dialog closes or when a newer request supersedes an older request.

Targets:

- research list/load/section
- import preview/confirm
- tree dialog loading
- session message loading if user switches sessions
- memory search if repeated rapidly

Existing request IDs should remain even with cancellation, because cancellation is not guaranteed to stop immediately.

### 5. File diff lifecycle

Sidebar diff tasks already have concurrency bounding and stale generation checks. Add them to lifecycle only if cheap and practical. At minimum, expose active diff counts in diagnostics. If registered, they should be `TuiTaskKind::FileDiff` and should be aborted on shutdown.

### 6. Shell lifecycle alignment

Shell handles already live in `app.shell_handles`. Integrate them into shutdown semantics:

- On normal app exit, decide whether human shell commands are killed or detached. For a TUI-local shell feature, killing is safer and more predictable.
- Mark killed commands as `ShellStatus::Killed` with elapsed time.
- Avoid duplicate killed/exited updates from late events.

### 7. Diagnostics

Extend `/tui-stats` to include:

- active TUI tasks count
- active tasks by kind
- oldest active task name and age
- cancelled task count if tracked
- shell handles count

Do not render a huge task table in toasts. Keep the stats summary compact.

## Testing Plan

Unit tests:

1. Registry spawn increments active count.
2. Finished tasks are reaped.
3. `cancel_all` aborts or signals all active tasks.
4. `cancel_kind` only cancels matching task kinds.
5. Dialog cancellation invalidates older request IDs.
6. Shutdown path cancels registered tasks and marks shell commands killed where applicable.

Async tests:

1. Spawn a task waiting on cancellation and confirm cancellation is observed.
2. Drop the receiver for completion commands and confirm task completion is harmless.
3. Start two request generations and verify stale completion still cannot apply after cancellation.

Manual smoke tests:

1. Start research load, close the dialog, verify no stale UI mutation.
2. Start import preview, quickly switch source, verify only latest preview applies.
3. Start long shell command, quit TUI, verify process policy is applied.
4. Run `/tui-stats` during background work and confirm active counts are sane.

## Acceptance Criteria

- TUI-owned background tasks can be counted, cancelled, and reaped.
- App shutdown cancels or resolves background tasks intentionally.
- Dialog-scoped tasks are invalidated or cancelled on close/supersession.
- Shell commands have deterministic shutdown behavior.
- `/tui-stats` reports active task counts.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full daemon task scheduler.
- Persistent background job history.
- Remote-client cancellation protocol.
- Sophisticated task progress UI beyond compact diagnostics.
