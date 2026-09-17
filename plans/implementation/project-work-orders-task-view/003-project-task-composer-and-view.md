# Project Work Orders and Task View M003 — Project Task Composer, Scheduling Sheet, and Task View

Status: ready

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADRs:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: capability

Hard dependency: M002 closure.

## 1. Objective

Add the user-facing project Task mode and view to the reference TUI. A user should type a prompt exactly as in a normal new session, switch/cycle to Task composer mode, press Enter, configure optional scheduling/ordering controls, and create a WorkOrder. With the default zero-delay configuration, the WorkOrder should start immediately and otherwise become visible as future project work.

The project view must show running, future/waiting, attention, and recent work, permit safe lane reorder, and open any materialized task as a normal session.

## 2. Current implementation evidence

- `InputMode` currently means Insert/Normal only and should remain a text-editing/Vim concern.
- bare `Tab` currently maps to `SwitchAgent`; Shift+Tab maps to permission-mode cycling. Any Task-mode interaction must deliberately fit the existing mode/agent-selector UX without a keybinding collision.
- the TUI already has project tabs, project picker, session summaries, route tokens, view-switch epochs, scoped async task lifecycle, and project-correct inactive-session routing.
- `/tasks` currently lists/deletes durable low-level schedules and fabricates labels by fetching `ScheduleGet`; it is not a project WorkOrder view.
- long-output info dialogs, async request generations, and Vim-like `j/k` conventions already exist.
- runtime preferences already persist selected model and execution preferences daemon-side.

## 3. Invariants that must not regress

- TUI is projection/controller only; durable WorkOrder truth stays in daemon/core stores.
- `InputMode::Insert|Normal` remains separate from composer mode.
- existing Tab/Shift+Tab behavior cannot silently collide with Task-mode selection.
- creating a default Task never bypasses the WorkOrder service by directly creating a session/job from the TUI.
- entering a running WorkOrder session uses canonical project/session routing and stale-completion guards.
- closing/switching views does not cancel daemon-owned work.
- reorder applies only to eligible waiting WorkOrders and uses expected lane revision.
- last Task-mode model is a convenience preference, never execution authority.
- unavailable model/policy/workspace state is shown as attention, not silently repaired in the TUI.

## 4. Scope

### In scope

- explicit `ComposerMode::Session | Task` or equivalent frontend state;
- integration with current mode/agent selector and help/keybinding tables;
- Task prompt authoring using the existing prompt editor/history/stash behavior where appropriate;
- Enter → scheduling sheet/dialog rather than direct prompt submit when in Task mode;
- default schedule: immediate/zero delay, no repeat beyond one occurrence, no external trigger;
- scheduling controls for sequence lane/order, wait/delay, not-before time, finite repeat count, gate join All/Any, and external-trigger enable placeholder when supported;
- Task-mode model selection and persisted last-used Task model;
- project Task list/view with running/waiting/future/attention/recent grouping;
- `j/k` and arrows for selection/reorder using daemon CAS;
- open/focus materialized session;
- cancel/retry/skip/attention actions only where M001/M002 expose canonical operations;
- normal steering/permission/question/session control after focus;
- migration/deprecation of the old `/tasks` schedule presentation without deleting low-level Schedule protocol;
- `/tasks` and/or `/task` command discovery pointing to the new view;
- tests for narrow terminals, stale async completions, project/tab isolation, and keybinding collisions;
- architecture/help documentation.

### Explicitly out of scope

- global multi-project dashboard (M004);
- HTTP trigger creation/fire UI beyond a disabled/capability-aware scheduling option (M005);
- agent WorkOrder tool (M006);
- new scheduler/worktree/session logic;
- automatic merging/integration of completed worktrees;
- drag-and-drop/mouse-specific reorder as a requirement.

## 5. Composer mode design

Do not add Task as a third `InputMode`. Add a distinct composer/submission mode owned by project/session UI state.

The mode should be visible near the prompt in the same semantic area where the user currently sees agent/model/permission context. The implementation should inspect whether current `SwitchAgent` Tab behavior already represents the desired selector strip. If so, extend that selector coherently. If not, add a configurable `ToggleComposerMode`/`NextComposerMode` action and deliberately choose a collision-free default, preserving Tab behavior unless the user-facing redesign explicitly migrates it with tests/help updates.

The requested user experience is that Tab can reach Task mode. If implementing that requires changing existing `SwitchAgent`, perform one documented selector model rather than two competing Tab handlers.

Task prompt text is not submitted to the session transcript before WorkOrder confirmation succeeds. Failure restores/preserves the editable prompt exactly once, following the existing prompt/session-create continuation pattern.

## 6. Scheduling sheet/dialog

On Enter in Task mode, open a focused modal that shows at least:

- prompt/title preview;
- model;
- run condition summary;
- sequential lane toggle + current queue placement;
- delay/wait duration (default 0);
- optional not-before timestamp;
- finite repeat count (default 1);
- external-trigger gate toggle only if server capability exists;
- All/Any join policy when more than one nontrivial gate is enabled;
- effective requested approval/sandbox/workspace policy summary;
- confirm/cancel.

If “sequential” is enabled, display running and waiting tasks in the current project/lane. Selection/reorder should use `j/k` or Up/Down and clearly distinguish pinned running tasks from movable future tasks.

The modal should show a concise natural-language result, for example:

```text
Run after task abc123 AND wait 20m · repeat 3x · model foo/bar
```

Avoid exposing raw JSON gate structures.

## 7. Time input semantics

Reuse a deterministic parser with explicit timezone behavior. Store/submit absolute UTC timestamps but render local time with timezone/offset. Delay accepts bounded duration forms.

Ambiguous time input must fail validation in the modal, not guess. If the project/server capability does not support a selected gate, disable it or return an actionable error rather than silently dropping it.

## 8. Last Task model preference

Extend daemon-owned runtime preference semantics with a task-composer model key/scope rather than using frontend-only manifests or old generic `user_preferences` as authority.

Rules:

- human Task mode defaults to last valid Task model if available;
- explicit selection updates the preference through the canonical settings service;
- WorkOrder creation snapshots the selected stable identity;
- if the remembered model no longer exists, the composer falls back to the normal current/default selection *before creation* with a visible notice; an already-created WorkOrder never silently changes model;
- ordinary Session model preference behavior remains unchanged.

## 9. Project Task view

Provide one full project-scoped view/panel, not a collection of toasts.

Suggested sections:

```text
RUNNING
  status  task/session title        elapsed   attention

FUTURE / WAITING
  order   task title                release condition

NEEDS ATTENTION
  task    reason                    action hint

RECENT
  task/session title                result     finished
```

The view should be bounded, pageable/scrollable, and lazy about heavy session details. Use WorkOrder summary DTOs and session IDs; do not issue `ScheduleGet`-style N+1 calls for every row.

Selecting a materialized running/recent WorkOrder opens/focuses the canonical session through existing project-tab/session-loading machinery. Selecting a future WorkOrder opens task detail/edit actions, not a fake session.

## 10. Reorder semantics

- only waiting/unclaimed lane members are movable;
- current running/claimed predecessor is pinned;
- `j/k` without reorder modifier may navigate; choose a consistent existing convention for move (for example Shift+j/k or a focused “reorder” mode) if bare `j/k` navigation would otherwise conflict;
- every move includes expected lane revision;
- conflict refreshes the lane and displays “queue changed; retry” rather than applying local speculative order;
- moving a task never directly edits Job dependencies.

## 11. Attention and session control

Project Task view must surface at least:

- pending permission/question on a materialized session;
- model unavailable;
- policy denied/narrowed where user action is required;
- sequence predecessor failed/held;
- workspace/worktree conflict/retained attention;
- scheduler/materialization recovery attention.

For running sessions, stop/cancel/steer/permission/question response delegates to existing session/job control surfaces. Do not add WorkOrder-specific versions unless the action targets pre-session state.

## 12. Migration from old `/tasks`

The current schedule UI is a thin compatibility/user surface over low-level `Schedule*` operations. M003 should:

- make `/tasks` open the WorkOrder Task view when WorkOrder capability is available;
- optionally retain a clearly named `/schedules` diagnostic/low-level view if existing users need direct Schedule access;
- preserve low-level protocol and scheduler behavior;
- remove duplicated schedule-label fetching/N+1 behavior from the primary task UX;
- retain explicit compatibility diagnostics for older servers without WorkOrder capability.

Do not revive legacy `TaskList/TaskSchedule/TaskDelete` protocol.

## 13. Async/stale routing requirements

Use existing `AsyncUiRequestState`, `UiRouteToken`, view-switch epoch, reconnect epoch, project tab scope, and registered TUI task patterns.

Every create/list/reorder/edit completion must carry enough project/view/request identity to be rejected after:

- project tab switch/close;
- workspace rebind;
- reconnect;
- task-view generation change;
- another successful reorder/update.

A late create success may have committed daemon state even if the frontend route is stale; do not delete the WorkOrder to “undo” a stale UI completion. Refresh the project projection when next foregrounded.

## 14. Ordered work packages

### A — Composer-mode state and keybinding audit

Add composer mode, selector integration, configurable action(s), help entries, collision tests, and prompt preservation semantics.

### B — Scheduling sheet

Build bounded modal state, validation, queue/lane preview, model/policy summary, and WorkOrderCreate request flow.

### C — Project Task projection/view

Add bounded Task view state, async refresh/event hints, sections, navigation, detail/focus behavior, and attention badges.

### D — Reorder and lifecycle actions

Wire CAS reorder and waiting-task cancel/edit/retry/skip operations using canonical daemon services.

### E — Old task UI migration + docs

Route `/tasks` to WorkOrders, preserve optional low-level schedule diagnostics, remove primary N+1 schedule-label behavior, and update TUI/help/architecture docs.

## 15. Required tests

- composer Session↔Task mode transitions;
- Tab/keybinding collision audit and Vim bindings;
- Task prompt retained after create failure/cancel;
- default zero-delay create payload;
- delay/not-before/repeat/All/Any validation;
- task model preference survives restart and does not alter ordinary session preference;
- unsupported gate capability fails visibly;
- project Task sections/order/attention rendering;
- narrow terminal behavior;
- reorder CAS conflict refresh;
- running task opens correct session/project tab;
- stale create/list/reorder completions are dropped;
- tab close does not cancel daemon WorkOrder/session;
- permission/question attention on inactive task never steals focus;
- `/tasks` compatibility behavior and no legacy Task protocol use.

## 16. Required verification

```bash
cargo test -p codegg --lib -- tui::
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test --test tui_project_routing
cargo test --test tui --test tui_render
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy -p codegg --lib --features=lsp-test-support -- -D warnings
./scripts/verify.sh quick
```

## 17. Acceptance criteria

- User can author a Task prompt from the normal composer and confirm it through a scheduling sheet.
- Default configuration is immediate/zero delay and starts through WorkOrder/M002 behavior.
- User can see and safely reorder future sequential tasks.
- Running task session opens and behaves exactly like a normal session.
- Last Task model is durable and separately scoped as a convenience preference.
- Attention/permission states are visible without focus theft.
- `/tasks` presents WorkOrders; no second scheduler or legacy Task protocol returns.
- Async completions remain project/view correct.

## 18. Stop conditions

Stop if the TUI begins owning WorkOrder truth, creates sessions/jobs directly for tasks, overloads `InputMode`, introduces an unresolved Tab collision, or requires raw path/cwd project inference.

## 19. Closure evidence required

- implementation commits;
- before/after task UX/keybinding table;
- WorkOrder create/reorder request traces or focused test evidence;
- stale-route matrix;
- Task-model persistence evidence;
- old `/tasks` migration/compatibility evidence;
- TUI render/keybinding/narrow-terminal test results;
- exact verification commands and residual findings.
