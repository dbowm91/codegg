# TUI Phase 1: Event Loop Responsiveness Foundation

## Objective

Move high-latency and potentially blocking command work out of the TUI event loop while preserving the existing single-owner UI state model. The TUI should remain responsive to keyboard input, mouse input, resize events, streaming redraws, spinner animation, and toast expiry even when core-backed operations are slow.

## Current Problem

`run_event_loop` directly awaits many `TuiCommand` handlers inside the command arm. Examples include session reload, session deletion/archive/fork/share/export/rename, bulk operations, tree loading, import preview/confirm, session message loading, memory operations, research browser loading, doctor, security review, and goal operations. While those futures are awaited, the loop cannot process terminal events or render ticks.

Some handlers are cheap enough most of the time, but the TUI should not depend on core latency being small. This becomes more important in daemon mode, remote-core mode, slow SQLite operations, network-backed providers, and future multi-session workflows.

## Design Direction

Introduce a standard async command request/result pattern:

1. The event loop receives an initiating `TuiCommand`.
2. It performs only immediate UI mutation, such as setting a dialog to loading or adding a short toast.
3. It clones the needed core client or immutable inputs.
4. It spawns the slow work in a Tokio task.
5. The task sends a typed completion command back through `tui_cmd_tx`.
6. The event loop receives the completion command and mutates UI state synchronously.

This keeps all UI state mutation on the event loop while removing I/O latency from the loop.

## New Command Pattern

Add completion variants where needed rather than returning raw JSON or mutating app state from spawned tasks. Prefer domain-specific typed results.

Suggested new variants:

```rust
TuiCommand::SessionsReloaded {
    sessions: Vec<crate::protocol::core::SessionDto>,
    message_counts: std::collections::HashMap<String, usize>,
    error: Option<String>,
}

TuiCommand::SessionMessagesLoaded {
    session_id: String,
    messages: Vec<crate::session::message::Message>,
    error: Option<String>,
}

TuiCommand::TreeDialogLoaded {
    current_session_id: Option<String>,
    nodes: Vec<crate::tui::components::dialogs::tree::TreeNode>,
    error: Option<String>,
}

TuiCommand::ImportPreviewLoaded {
    request_id: u64,
    result: ImportPreviewResult,
}

TuiCommand::ResearchRunsLoaded {
    request_id: u64,
    runs: Vec<crate::research::types::ResearchRunSummary>,
    error: Option<String>,
}

TuiCommand::ResearchRunLoaded {
    request_id: u64,
    run_id: String,
    bundle: Option<crate::research::types::ResearchBundle>,
    error: Option<String>,
}
```

Exact type names should be adjusted to match the current codebase. If a type is private or inconvenient to move through the command enum, create a small public DTO in the relevant dialog or handler module.

## Scope for This Phase

Prioritize the handlers most likely to block UI interaction:

- `ReloadSessions`
- `LoadSessionMessages`
- `OpenTreeDialog`
- `PreviewImport`
- `ConfirmImport`
- `ResearchListRuns`
- `ResearchLoadRun`
- `ResearchLoadSection`
- `MemorySummary`
- `MemorySearch`
- `MemoryRemember`
- `MemoryForget`
- `RunDoctor`

Do not try to convert every command in one pass if that becomes too large. Goal commands and simple session mutations can remain direct for a follow-up if needed, but all commands that can fetch lists, load files, or call diagnostics should be moved first.

## Implementation Steps

### 1. Create an async command helper module

Add a module such as `src/tui/async_cmd.rs` or `src/tui/commands/async_work.rs` with helpers for spawning tasks that send completions back to `mpsc::Sender<TuiCommand>`.

Suggested helper shape:

```rust
pub fn spawn_tui_task<F>(tx: Option<tokio::sync::mpsc::Sender<TuiCommand>>, name: &'static str, fut: F)
where
    F: std::future::Future<Output = Option<TuiCommand>> + Send + 'static,
{
    let Some(tx) = tx else {
        tracing::warn!(task = name, "cannot spawn TUI task without command sender");
        return;
    };
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = fut.await;
        tracing::debug!(task = name, elapsed_ms = started.elapsed().as_millis(), "TUI task finished");
        if let Some(cmd) = result {
            if let Err(e) = tx.send(cmd).await {
                tracing::debug!(task = name, "TUI task completion dropped: {}", e);
            }
        }
    });
}
```

Keep it simple. Cancellation and task ownership can be improved in Phase 7.

### 2. Add request IDs/generation counters where stale results are possible

Dialogs that can issue repeated async requests should track a `request_id` or generation. This is especially important for import preview, research loading, memory search, and model/session filtering if added later.

Store the active request ID in the dialog state or app state. Completion handlers should compare IDs and ignore stale results.

### 3. Convert `reload_sessions`

Split current `reload_sessions(app).await` into:

- `start_reload_sessions(app)` for immediate UI mutation and task spawn.
- `fetch_sessions_for_dialog(core_client, project_id, show_archived)` as a pure async function returning sessions/counts/error.
- `apply_sessions_reloaded(app, sessions, counts, error)` for UI mutation.

`start_reload_sessions` should call `app.dialog_state.session_dialog.set_loading(true)` immediately. The completion handler should clear loading in all success and failure paths.

### 4. Convert session message loading

`handle_load_session_messages` currently clears messages before the load result is known. Avoid clearing the visible transcript until the requested session messages actually arrive. Instead, show loading state or toast, fetch messages in the background, then replace messages on success. On failure, keep the old visible state and show an error toast.

This prevents slow or failed loads from blanking the UI.

### 5. Convert tree dialog loading

`handle_open_tree_dialog` should open the tree dialog immediately with loading state, then populate nodes on completion. The tree-building computation can happen in the task because it only uses cloned session data and message counts. Completion should ignore stale results if the active session changed.

### 6. Convert import preview/confirm

Import preview can involve reading a file and sending import data to core. Move both off-loop. Keep the dialog interactive while the preview is loading. Add `request_id` so previewing one source and quickly switching to another cannot display the older preview.

For confirm import, set loading/disabled state, spawn the core operation, then apply done/error in the completion command.

### 7. Convert research browser loading

Research list/run/section loads should not block the loop. Set `browser.loading = true`, spawn the file/service load, then apply results. Use request IDs per browser operation.

### 8. Convert memory commands and doctor

Memory summary/search/remember/forget and doctor operations should send concise completion messages back to the TUI. Short commands can still surface their result as toasts for this phase; later phases can move long output to dialogs.

### 9. Add loop-block diagnostics

Add a simple event-loop stall detector around each select iteration or render cycle. If the loop spends more than a threshold, for example 100 ms or 250 ms, log a warning with the last active operation if known. Keep this low overhead.

A minimal approach is to record `let loop_start = Instant::now();` near the top of the loop and log if elapsed exceeds a threshold before the next iteration.

## Testing Plan

Add fake-core tests that simulate slow responses. A fake core client should delay a response while the TUI command path returns promptly. Because the real event loop is hard to drive directly, test the split functions where possible and add at least one integration-style async test around the command spawn helper.

Required tests:

1. Reload sessions sets loading immediately and eventually applies sessions/counts.
2. Failed reload clears loading and displays an error toast.
3. Session message load failure does not clear existing messages.
4. Stale import preview completion is ignored when request IDs do not match.
5. Research browser stale run load is ignored.
6. Spawn helper drops completion safely if the receiver is gone.

Manual verification:

1. Run the TUI against a fake or intentionally delayed core operation.
2. Open session list and trigger reload while pressing keys or resizing terminal.
3. Confirm spinner/toasts continue to animate.
4. Confirm no command freezes the UI for the duration of a slow core request.

## Acceptance Criteria

- Slow core-backed TUI commands no longer block terminal input processing.
- Session and research dialogs show loading/error states consistently.
- Stale async results cannot overwrite newer dialog state for converted commands.
- Existing visible messages are not cleared before successful session-message load.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-features` pass.

## Out of Scope

- Full cancellation infrastructure for every async task. That belongs in Phase 7.
- Full dialog state-machine unification. That belongs in Phase 10.
- Large-scale file/module refactor of `src/tui/mod.rs`. That belongs in Phase 11.
