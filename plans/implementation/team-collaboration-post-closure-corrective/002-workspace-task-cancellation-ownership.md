# Team Collaboration Post-Closure Corrective Milestone 002 — Workspace Task Cancellation Ownership

Status: ready for handoff

Repository baseline: `626585a1fad449a637e4828777bfa21d222abea0`

Source roadmap: `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m002--workspace-owned-task-cancellation`

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs: none required.

Primary class: invariant

## 1. Objective

Make Workspace close/cancellation affect only Workspace-owned frontend work. Leaving `Route::Workspace` must never abort unrelated generic `TuiTaskKind::Command` tasks from chat, team administration, control, provider, project/session, diagnostics, or WorkOrder flows.

## 2. Why this milestone is ready

M005 established the non-modal Workspace route, generation/reconnect guards, and existing `ChatState` reuse. The TUI task registry already supports kind, tab, session, epoch, and individual task cancellation. No daemon/protocol/storage change is required.

This is independent of the registration-authority corrective.

## 3. Current implementation evidence

`src/tui/commands/workspace_dashboard.rs::leave_workspace_view` currently starts with:

```rust
app.task_registry.cancel_kind(TuiTaskKind::Command);
```

`TuiTaskRegistry::cancel_kind` walks the entire registry and aborts every task whose kind matches. `TuiTaskKind::Command` is deliberately shared by many unrelated command modules.

The same Workspace close path already:

- increments dashboard generation;
- marks loading false;
- cancels the dashboard `RequestState`;
- clears Workspace-only route/panel binding; and
- navigates through route history.

Those guards are sufficient to stale-drop old completions if the underlying task finishes. A precise cancellation owner can be added without touching daemon work.

M005 tests covered Workspace route semantics, stale completions, revocation, and "no daemon cancel", but did not co-schedule an unrelated `Command` task while leaving Workspace. The category-wide cancellation therefore escaped qualification.

## 4. Invariants that must not regress

- Closing a frontend view never cancels daemon-owned turns/jobs.
- Closing Workspace never aborts unrelated TUI feature tasks.
- Workspace refresh/expand completions after close cannot mutate current state.
- Per-project chat drafts/cache survive leaving Workspace; chat is not a dashboard-owned ephemeral task.
- Reconnect and generation fencing remain authoritative against stale completions.
- Shutdown and tab/session-specific cancellation continue to work.
- Task registry counts remain truthful.

## 5. Scope

In scope:

- Workspace dashboard/view task spawning and close lifecycle.
- `TuiTaskKind` or task-record ownership if a dedicated Workspace kind/scope is the cleanest solution.
- Workspace request/generation cancellation.
- Focused task-lifecycle and Workspace regression tests.
- TUI architecture/skill documentation.

Out of scope:

- Changing daemon cancellation.
- Cancelling chat merely because Workspace closes.
- Reworking the generic task registry.
- Converting every `Command` task to a dedicated kind.
- Changing Workspace routing, selected-project semantics, Task scheduling, or chat policy.

## 6. Required production changes

### A. Remove broad generic cancellation

`leave_workspace_view` must not call `cancel_kind(TuiTaskKind::Command)`.

Use one precise ownership mechanism:

Preferred: add a dedicated `TuiTaskKind::Workspace` (or equivalently named view-specific kind) for Workspace dashboard refresh/expand work, and cancel that kind on Workspace close.

Acceptable alternative: store exact Workspace task IDs and cancel only those IDs.

Do not use tab/session cancellation as a proxy when Workspace can show a project that is not the active tab.

### B. Preserve stale fencing

Keep generation/request/reconnect checks. Cancellation is best-effort resource cleanup; stale-completion rejection remains the correctness boundary.

If a task completes concurrently with close, its apply path must observe missing/changed Workspace state and do nothing.

### C. Keep chat independent

Workspace side-panel chat uses the shared `ChatState/chat.v1` reducer and project-keyed drafts. It may have its own generic chat refresh tasks. Leaving Workspace should clear the panel binding/focus but must not destroy or abort unrelated chat state simply to close the dashboard.

### D. Diagnostics

`/tui-stats` and task-kind display should account for any new kind without brittle hard-coded count assumptions. Update architecture docs/tests that enumerate task kinds.

## 7. Ordered work packages

### Work package A — Pin the regression

Add a test that:

1. registers one long-running unrelated `TuiTaskKind::Command` task;
2. registers one Workspace-owned dashboard task;
3. enters/leaves Workspace;
4. proves the Workspace task is cancelled or its completion stale-dropped;
5. proves the unrelated Command task remains active/finishes normally; and
6. proves cancellation accounting increments only for the Workspace-owned task.

Add a second test around chat/project draft state if needed to prove leaving Workspace does not clear unrelated chat state.

### Work package B — Introduce precise ownership

Implement the smallest precise cancellation seam. If using a dedicated kind, change only Workspace dashboard/view task spawns that are genuinely owned by the route. Do not mechanically relabel generic chat/team/WorkOrder operations.

### Work package C — Re-run M005 behavior

Verify:

- selected project state;
- side-panel chat drafts;
- Session/Task composer;
- stale completion;
- reconnect;
- revocation;
- narrow terminal behavior; and
- modal-above-Workspace return.

## 8. Failure, cancellation, restart, contention semantics

- Close racing with task completion: generation/request guard wins; no state resurrection.
- Reopen after close: new generation/request owns new state; old completion drops.
- Reconnect while open: existing reconnect epoch semantics remain.
- Shutdown: global registry shutdown/cancel-all remains unchanged.
- Unrelated generic tasks: no change in lifetime solely because Workspace route closes.
- Chat refresh running during close: may complete into shared ChatState if still authorized; it must not re-open Workspace or route input.

## 9. Compatibility and migration

No storage/protocol migration.

Visible behavior improves: unrelated dialogs/commands no longer mysteriously stop when the user exits Workspace.

If a new task kind is added, it is frontend-internal and has no wire compatibility impact.

## 10. Required tests

At minimum:

- new focused `workspace_postclosure_m002_task_cancellation` integration/unit target;
- `workspace_m005_selected_project_chat`;
- `tui::task_lifecycle` unit tests;
- Workspace dashboard command tests;
- project routing/tab tests if touched.

Explicit negatives:

- team admin refresh survives Workspace close;
- generic chat refresh survives when not Workspace-owned;
- provider/project/session command task survives;
- old Workspace completion cannot repopulate closed state.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test --test workspace_postclosure_m002_task_cancellation -- --test-threads=1
cargo test --test workspace_m005_selected_project_chat -- --test-threads=1
cargo test -p codegg --lib tui::task_lifecycle -- --test-threads=1
cargo test -p codegg --lib tui::commands::workspace_dashboard -- --test-threads=1
cargo test --test tui_project_routing --test tui_project_tabs -- --test-threads=1
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

## 12. Documentation updates

Update `architecture/tui.md` and `.opencode/skills/tui/SKILL.md` if a new task kind/ownership rule is introduced. Document that Workspace close cancels only view-owned frontend fetches and relies on generation/request fencing for races.

## 13. Acceptance criteria

- Exiting Workspace does not call `cancel_kind(Command)`.
- An unrelated generic Command task survives Workspace close.
- Workspace-owned refresh/expand work is either precisely cancelled or safely stale-dropped.
- Cancellation counters identify only actually cancelled Workspace-owned tasks.
- Chat drafts/cache persist correctly.
- Existing M005 Workspace behavior stays green.
- No daemon turn/job cancellation is introduced.

## 14. Stop conditions

Stop and split the work if:

- precise cancellation requires redesigning the entire task registry;
- a daemon/protocol cancellation contract must change;
- chat must be made route-ephemeral to implement the fix;
- project/tab ownership semantics must change.

## 15. Closure evidence required

Closure record: `plans/closure/team-collaboration-post-closure-corrective/002-status.md`.

Include the failing baseline scenario, task-lifetime assertions, task-registry accounting, M005 regression results, and dependency audit for M003.

## 16. Handoff notes

The main correctness boundary is stale-completion rejection; cancellation is resource hygiene. Do not trade the current strong generation/reconnect fencing for a larger cancellation framework. The goal is simply to stop Workspace close from owning tasks it did not create.
