# TUI Cleanup and Hardening Plan

## Context

The recent TUI implementation pass materially improved the repo. The TUI is now decomposed into runtime and command-domain modules, has a task lifecycle registry, uses shared async request state for several dialog flows, has broader headless render regression tests, and has initial remote TUI snapshot DTOs.

The remaining work is cleanup and hardening rather than foundational repair. This plan focuses on closing small correctness holes and making the new architecture more reliable under slow core calls, stale completions, remote clients, large repos, and task shutdown.

## Goals

- Make shared async request state usage consistent across all migrated flows.
- Remove or quarantine remaining event-loop direct awaits that can block the TUI.
- Wire the remote TUI state/snapshot protocol enough that unsupported paths fail explicitly and resync behavior is testable.
- Harden the task lifecycle registry so completed, aborted, and panicked tasks are tracked accurately.
- Move render-time git probing out of `App::render_sidebar`.
- Strengthen targeted tests around these cleanup areas.

## Non-Goals

- Do not redesign the TUI again.
- Do not replace ratatui or rewrite render components.
- Do not build a full GUI/mobile frontend.
- Do not implement a complete remote collaboration system.
- Do not add a heavyweight background-job dashboard.

---

## Workstream 1: Normalize `AsyncUiRequestState` Completion Semantics

### Problem

`AsyncUiRequestState` now exists and is used by session reload, import, research, tasks, worktree, templates, session mutation, and session message loading. Some application paths check `is_current(request_id)` but do not call `finish(request_id)` or `fail(request_id, error)`. That can leave `loading` true after a result has been applied.

Known target from inspection:

- `apply_import_preview_loaded`
- `apply_import_confirmed`

Audit all other `apply_*` functions that use request IDs.

### Implementation Steps

1. Search for all request-state uses:

   ```bash
   rg "is_current\(|\.finish\(|\.fail\(|\.begin\(|AsyncUiRequestState" src/tui
   ```

2. For every `apply_*` function with a `request_id`, enforce this pattern:

   ```rust
   if let Some(err) = error {
       if !request_state.fail(request_id, err.clone()) {
           return;
       }
       // apply visible error
       return;
   }

   if !request_state.finish(request_id) {
       return;
   }
   // apply success
   ```

3. For success-without-data cases, still call `finish(request_id)`.

4. For stale results, return silently. Do not show stale error toasts.

5. For dialog-close/cancel paths, call `cancel()` rather than only setting dialog to `None`.

6. Ensure `cancel()` is called for:

   - import dialog close
   - research browser close
   - session dialog close if reload is in flight
   - tree dialog close if load is in flight
   - template dialog close during create
   - task/worktree dialogs if they become proper dialogs

### Specific Import Fix

Change import apply functions to this shape:

```rust
pub(crate) fn apply_import_preview_loaded(
    app: &mut App,
    request_id: u64,
    session: Option<crate::session::Session>,
    msg_count: usize,
    error: Option<String>,
) {
    if let Some(err) = error {
        if !app.dialog_state.import_request.fail(request_id, err.clone()) {
            return;
        }
        if let Some(ref mut import) = app.dialog_state.import_dialog {
            import.set_error(err);
        }
        return;
    }

    if !app.dialog_state.import_request.finish(request_id) {
        return;
    }
    if let (Some(ref mut import), Some(session)) = (&mut app.dialog_state.import_dialog, session) {
        import.set_preview(session, msg_count);
    }
}
```

Do the equivalent for confirm.

### Tests

Add unit tests or integration-style app-state tests:

- import preview success clears loading
- import preview error clears loading and stores last error
- import confirm success clears loading
- stale import preview does not mutate dialog and leaves current loading state intact
- cancel then stale result does not mutate dialog

### Acceptance Criteria

- All request-ID-based apply handlers call `finish`/`fail` exactly once for current completions.
- Stale completions never clear the current loading state.
- Dialog close invalidates outstanding requests.
- Tests cover import and at least one other request-state flow.

---

## Workstream 2: Remove Remaining Blocking Awaits From Command Dispatch

### Problem

The new `runtime/command_dispatch.rs` makes direct awaits easy to audit. Several remain:

- `handle_spawn_subagent(...).await`
- `handle_compact_session(app).await`
- `handle_open_diff_dialog(...).await`
- `handle_goal_set(...).await`
- `handle_goal_from_file(...).await`
- `handle_goal_simple(...).await` for pause/resume/clear/done
- `handle_goal_budget(...).await`
- `handle_security_review_run(...).await` legacy path

Some may be cheap. Others can perform core calls, filesystem reads, or security-review work and should use start/completion paths.

### Implementation Steps

1. Search for awaits in command dispatch:

   ```bash
   rg "\.await" src/tui/runtime/command_dispatch.rs
   ```

2. Classify each direct await:

   - pure UI/no I/O: may remain only if genuinely cheap
   - core-backed: convert
   - filesystem-backed: convert or use `spawn_blocking`/async fs in task
   - legacy/backcompat path: quarantine or convert to background dispatch

3. Goal operations should be converted first.

   Add start/apply helpers in `src/tui/commands/goals.rs`:

   ```rust
   pub(crate) fn start_goal_set(...)
   pub(crate) fn start_goal_from_file(...)
   pub(crate) fn start_goal_simple(...)
   pub(crate) fn start_goal_budget(...)
   ```

   Reuse `TuiCommand::GoalOperationFinished` where adequate. Add variants only if needed.

4. For `GoalFromFile`, read the file inside the spawned task with `tokio::fs::read_to_string` and return an error completion if it fails.

5. For `CompactSession`, add:

   ```rust
   TuiCommand::CompactSessionFinished { summary: String, error: Option<String> }
   ```

   Or reuse a generic toast completion if one exists.

6. For `OpenDiffDialog`, if the handler only constructs the dialog from already-supplied boxed content, make it synchronous and remove `async`. If it reads files or computes diffs, split heavy work into a background task.

7. For `SpawnSubagent`, inspect whether it calls the pool/core and may block. If yes, convert to:

   ```rust
   TuiCommand::SubagentSpawnFinished { agent_name: String, error: Option<String> }
   ```

8. For `SecurityReviewRun`, either:

   - convert legacy command path to the same spawn-and-finished implementation used by slash dispatch, or
   - mark it test/backcompat-only and ensure production slash dispatch never sends it.

### Tests

- Add a test or static guard that `command_dispatch.rs` has no `.await` except explicitly allowlisted cheap handlers.
- Goal set/from-file/simple/budget completion applies success and error without blocking.
- `GoalFromFile` file-read failure returns a visible error.
- Security review legacy path is explicit: either converted or documented/covered.

### Acceptance Criteria

- `command_dispatch.rs` has no avoidable direct awaits.
- Remaining direct awaits are documented with comments explaining why they are safe.
- Core-backed goal and compact operations use background completions.
- Tests cover at least the goal conversion and one stale/error path.

---

## Workstream 3: Remote TUI Protocol Hardening

### Problem

The protocol crate now has `REMOTE_TUI_PROTOCOL_VERSION`, `StateSnapshot`, `RequestSnapshot`, `ResyncRequired`, and `RemoteTuiStateSnapshot`. `App::remote_snapshot()` exists. However, remote protocol handling is not fully wired:

- `remote_snapshot()` uses `sequence: 0` unconditionally.
- `RenderFrame` appears to remain in the enum but does not have explicit unsupported handling in runtime/server paths.
- The WebSocket server still primarily handles legacy JSON-RPC and does not visibly process snapshot/resync messages.

### Implementation Steps

1. Add a remote sequence counter to app state or remote state:

   ```rust
   pub remote_sequence: u64
   ```

   Or store it in a dedicated remote runtime state struct.

2. Change `remote_snapshot()` so the caller supplies sequence or the app increments it through a mutating method:

   ```rust
   pub fn build_remote_snapshot(&self, sequence: u64) -> RemoteTuiStateSnapshot
   pub fn next_remote_snapshot(&mut self) -> RemoteTuiStateSnapshot
   ```

3. Add explicit unsupported `RenderFrame` handling wherever incoming `TuiMessage` is matched:

   ```rust
   TuiMessage::RenderFrame { .. } => {
       send Error { message: "unsupported_render_frame: state-driven remote TUI is required" }
   }
   ```

   Prefer a structured error if the protocol supports it.

4. Add `RequestSnapshot` handling:

   - client sends `RequestSnapshot`
   - server/app returns `StateSnapshot { sequence, snapshot }`
   - snapshot sequence is monotonic

5. Add `Resume { from_event_seq }` or `ResyncRequired` behavior:

   - if requested sequence is too old/unknown, return `ResyncRequired`
   - otherwise return current `StateSnapshot` until proper delta history exists

6. Document that current remote mode is snapshot-first, delta-later.

7. Audit docs so `architecture/protocol.md`, `architecture/tui.md`, and `.opencode/skills/tui/SKILL.md` all say the same thing.

### Tests

- `remote_snapshot` includes protocol version and nonzero/monotonic sequence when using the mutating API.
- `RequestSnapshot` returns `StateSnapshot`.
- `RenderFrame` returns explicit unsupported error.
- `Resume` either resyncs or returns a snapshot according to the chosen behavior.
- Snapshot builder does not panic with empty app, active session, dialog, messages, and toasts.

### Acceptance Criteria

- Remote snapshots have meaningful sequence numbers.
- Unsupported frame-driven rendering fails explicitly.
- Snapshot request/resync behavior is test-covered.
- Docs accurately describe the supported remote model.

---

## Workstream 4: Task Registry Hardening

### Problem

`TuiTaskRegistry` tracks spawned tasks and can cancel/reap them. The current design stores `AbortHandle` but not `JoinHandle`. That is simple, but it limits observability: panicked tasks are not joined/logged, and reaping relies on `AbortHandle::is_finished()`.

### Implementation Options

#### Option A: Keep `AbortHandle`, add better tests and docs

This is minimal. Confirm that `AbortHandle::is_finished()` reliably returns true for completed tasks and aborted tasks in the supported Tokio version. Add comments and tests.

#### Option B: Store `JoinHandle<()>`

Preferred if borrow/ownership remains simple:

```rust
pub struct TuiTaskRecord {
    pub name: &'static str,
    pub kind: TuiTaskKind,
    pub started_at: Instant,
    handle: tokio::task::JoinHandle<()>,
}
```

Then:

- `cancel()` calls `handle.abort()`
- `reap_finished()` drains finished tasks
- for finished tasks, optionally `try_join` is not directly available, but if `handle.is_finished()` is true, remove and spawn/log join result or await in a nonblocking maintenance path

If joining inside `reap_finished` is awkward because it is sync, add:

```rust
pub async fn reap_finished_async(&mut self)
```

Use the async reaper from the event loop wake path or `/tui-stats`.

### Implementation Steps

1. Decide whether to keep abort handles or move to join handles.
2. Add task outcome accounting:

   ```rust
   completed_count: u64,
   cancelled_count: u64,
   panicked_count: u64,
   ```

3. Update summary output with completed/cancelled/panicked counts.
4. Reap tasks periodically in the event loop, not only in tests or stats.
5. Ensure shutdown calls cancel and then optionally drains/reaps.
6. Document that cancellation is abort-based, not cooperative cancellation, unless cancellation tokens are later added.

### Tests

- completed tasks increment completed count after reap
- cancelled tasks increment cancelled count
- panicked task increments panicked count if join-handle design is used
- `summary()` includes active, completed, cancelled, and panicked counts
- event loop or app shutdown calls registry cancel path

### Acceptance Criteria

- Task registry has clear semantics for active, completed, cancelled, and panicked tasks.
- Finished tasks do not remain active after periodic reaping.
- Shutdown behavior is deterministic and tested.
- `/tui-stats` reports registry state accurately.

---

## Workstream 5: Move Git Probing Out of Render Path

### Problem

`App::render_sidebar` still performs git/worktree probing during render, including git-root lookup, branch lookup, and dirty-state checks. Render methods should be pure and fast. On large repos, slow filesystems, network mounts, or stuck git commands, this can hurt responsiveness.

### Design

Cache git status in app/session state and refresh it asynchronously on a cadence or relevant events.

Suggested state:

```rust
pub struct GitSidebarState {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub last_refreshed: Option<Instant>,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
}
```

Store under `SessionState` or a sidebar-specific state struct.

### Implementation Steps

1. Add cached git/sidebar state.
2. Replace render-time probing with:

   ```rust
   self.sidebar.set_git_info(
       self.session_state.git_sidebar.branch.clone(),
       self.session_state.git_sidebar.dirty,
       self.session_state.git_sidebar.root.clone(),
   );
   ```

3. Add a background refresh function:

   ```rust
   start_refresh_git_sidebar_state(app)
   ```

4. Trigger refresh on:

   - session change
   - config/project dir change
   - file change events, debounced
   - periodic interval if needed, e.g. every 5–15 seconds while TUI is active

5. Use `spawn_registered_tui_task` with `TuiTaskKind::Other` or add `GitStatus` kind.

6. Add stale generation handling so old git results do not overwrite newer session/project state.

7. Add size/cost guardrails:

   - timeout git dirty checks
   - skip dirty check for very large repos if needed
   - degrade to branch/root only on error

### Tests

- render_sidebar does not call git probing helpers directly; if possible add a static/source test or refactor to make this obvious.
- cached state renders expected branch/dirty/root.
- stale git refresh result is ignored after session/project switch.
- error result stores error but does not panic or block render.

### Acceptance Criteria

- No filesystem/process git probing occurs inside render methods.
- Sidebar git status comes from cached state.
- Git status refresh is async, bounded, and stale-protected.
- Render regression tests still pass.

---

## Workstream 6: Long Output and Info Dialog Follow-Through

### Problem

Phase 12 started moving structured output toward info dialogs, but not all long outputs have been audited. Some command outputs may still use multi-line toasts.

### Audit Targets

Search for toast calls that can include joined lists, multi-line strings, or detailed reports:

```bash
rg "toasts\.(info|warning|error)|Toast::" src/tui
```

Review targets:

- `/tui-stats`
- doctor report
- task list
- worktree list
- shell list
- memory search results
- goal show/budget
- bulk partial failures
- remote/protocol errors
- render fallback reports

### Implementation Steps

1. Define a helper:

   ```rust
   fn show_short_or_info_dialog(app: &mut App, info_type: InfoType, title_or_lines: Vec<String>)
   ```

   Or use existing `open_info_dialog` directly.

2. Use toast only for short one-line success/failure messages.
3. Use info dialog for structured/multi-line output.
4. For command flows where opening a dialog would be disruptive, show short toast plus allow `/last-info` or equivalent if already available. Do not invent too much UX here.

### Tests

- long task list opens info dialog or otherwise does not create a huge toast
- long shell list opens shell/info dialog
- `/tui-stats` details are accessible in a scrollable surface

### Acceptance Criteria

- No obvious multi-line structured command output is forced into a toast.
- Info dialogs are scrollable and have consistent footer hints.
- User-facing strings remain concise and consistent.

---

## Workstream 7: Verification and Regression Tests

### Required Commands

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If all-features is not viable, document the exact failing feature combination and run the strongest available matrix.

### Targeted Tests to Add

1. Async request completion:
   - import preview success/error clears loading
   - stale import result ignored
   - close/cancel invalidates result

2. Command dispatch:
   - no direct awaits except allowlisted functions, or goal direct awaits removed
   - goal operations return completion commands

3. Remote protocol:
   - snapshot sequence monotonic
   - request snapshot returns snapshot
   - render frame unsupported error

4. Task registry:
   - completed/cancelled/panicked accounting
   - periodic reap removes completed tasks
   - shutdown cancels active tasks

5. Git sidebar cache:
   - render uses cached data
   - stale background refresh ignored

6. Long output routing:
   - long command output opens info dialog instead of large toast

### Manual Smoke Tests

1. Start TUI in a large git repo and verify no render hitch from sidebar git status.
2. Run import preview then quickly close/switch source; verify no stale mutation.
3. Run goal commands during interaction; verify input stays responsive.
4. Request remote snapshot if a test client exists; verify sequence increments.
5. Start a long shell command and quit; verify shutdown kills/marks it according to policy.
6. Run `/tui-stats`, task list, shell list, and doctor; verify output is readable.

## Final Acceptance Checklist

- [ ] All current request-state apply handlers use `finish`/`fail` consistently.
- [ ] Dialog close invalidates in-flight request results.
- [ ] `command_dispatch.rs` has no avoidable direct awaits.
- [ ] Remaining direct awaits, if any, are documented and tested.
- [ ] Remote snapshot sequence is meaningful and monotonic.
- [ ] Unsupported `RenderFrame` path returns explicit error.
- [ ] Task registry reaping/outcome semantics are clear and tested.
- [ ] Git status is no longer probed during render.
- [ ] Long structured outputs use info dialogs or another scrollable surface.
- [ ] Headless render tests still pass across the size matrix.
- [ ] Workspace fmt/clippy/test checks pass.
