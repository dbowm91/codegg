# Multi-Project TUI and Session Management Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-3--project-correct-event-routing-and-lifecycle`

Repository baseline reviewed: `aa8ca21` (HEAD at milestone 3 start).

Implementation commits or pull requests:

- Implementation commit — routing registry, route token classifier,
  active-view epoch guards, scoped task lifecycle, bus event routing
  wrapper, tab/session cleanup, and integration tests.

## 1. Executive finding

Milestone 003 is complete. The TUI now carries:

- A central pure `classify_event` function that maps every
  `AppEvent` variant to a `RouteDecision` (ActiveView, InactiveSummary,
  RefreshRequired, DropDiagnostic, or Global) based on session-to-tab
  identity, active-view epoch, and reconnect epoch.
- A `RoutingRegistry` owned by `App` that maintains a session→tab
  index, per-tab bounded activity summaries, and monotonic
  reconnect/sequence counters.
- `UiRouteToken` carrying tab, project, workspace, and session
  identity for explicit completion validation.
- `ViewSwitchCoordinator` extensions: `begin_loading`,
  `commit_if_matches`, `suspend_if_matches`, and `replace_active`
  with epoch-checked guards that reject stale loads.
- Scoped task lifecycle: `TuiTaskRecord` with `scope_tab_id`,
  `scope_session_id`, `scope_active_view_epoch` fields, and
  `cancel_for_tab`, `cancel_for_session`, `cancel_for_stale_epoch`
  cancellation helpers.
- `spawn_scoped_registered_tui_task` for spawning tasks with
  explicit tab/session/epoch ownership.
- Bus event dispatch wrapper in `app_events.rs` that classifies
  events before dispatching: ActiveView/Global to existing handlers,
  InactiveSummary to bounded summary updates with permission/question
  toasts, DropDiagnostic to debug-log, RefreshRequired to tab resync.
- Tab close now calls `routing_registry.drop_tab` and
  `task_registry.cancel_for_tab` to clean up routing state and
  scoped tasks.
- Session set (`App::set_session`) now registers the session in the
  routing registry.

The implementation preserves all invariants from milestones 001–002:
the TUI remains a daemon client, no project authority is inferred
from cwd, existing single-project workflows are functional, and
async completions cannot mutate the wrong tab.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Central event classifier | `classify_event` in `src/tui/app/state/routing.rs`; unit tests for all branches | pass | Covers session-scoped, project-scoped, and global events |
| Session→tab index | `RoutingRegistry::session_index`; `register_open_session`, `tab_for_session`, `drop_tab` | pass | Session moves atomically clean prior tab activity |
| Route tokens | `UiRouteToken` struct with tab/project/workspace/session fields | pass | Used by completion guards |
| Active-view epoch | `ViewSwitchCoordinator::active_view_epoch`; `begin_loading`, `commit_if_matches`, `suspend_if_matches`, `replace_active` | pass | Stale epoch rejected; tests verify |
| Scoped task lifecycle | `TuiTaskRecord::scope_tab_id`, `scope_session_id`, `scope_active_view_epoch`; `cancel_for_tab`, `cancel_for_session`, `cancel_for_stale_epoch` | pass | All three cancel paths tested |
| Spawn scoped tasks | `spawn_scoped_registered_tui_task` in `async_cmd.rs` | pass | Wraps existing task registry |
| Inactive summary updates | `TabActivitySummary::apply_inactive_summary` with bounded text/error/health | pass | MAX_TAB_UNREAD_DISPLAY=99, MAX_TAB_LAST_ERROR_LEN=256, MAX_TAB_HEALTH_SUMMARY_LEN=256 |
| Permission/question foregrounding | InactiveSummary handler shows toast for pending permission/question | pass | Never steals focus across sessions |
| Tab close cleanup | `close_active_project_tab` calls `routing_registry.drop_tab` + `task_registry.cancel_for_tab` | pass | No leaked routing state |
| Session registration | `App::set_session` calls `routing_registry.register_open_session` | pass | Ensures index stays in sync |
| Stale completion rejection | `commit_if_matches` checks epoch; `classify_event` checks reconnect_epoch | pass | Pre-reconnect events dropped |
| Unknown session handling | `DropDiagnostic` for unknown sessions; `RefreshRequired` for epoch=0 | pass | No silent discard |
| Multi-tab isolation | Integration test `registry_drop_removes_tab_and_sessions`; `same_session_titles_across_tabs_do_not_collide_in_registry` | pass | Sessions bound to correct tabs |
| Static guards | `python3 scripts/check_daemon_cwd_usage.py`, `python3 scripts/check_git_forbidden_patterns.py`, `bash scripts/check-core-boundary.sh` | pass | No new cwd, no forbidden patterns, no core-boundary drift |

## 3. Production implementation evidence

New module:

- `src/tui/app/state/routing.rs` (~920 lines) — `UiRouteToken`,
  `RouteCheck`, `RouteDecision`, `RoutingRegistry`, `TabActivitySummary`,
  `classify_event`, `event_session_id`, `apply_inactive_summary`.
  Constants: `MAX_TAB_UNREAD_DISPLAY=99`, `MAX_TAB_LAST_ERROR_LEN=256`,
  `MAX_TAB_HEALTH_SUMMARY_LEN=256`. Re-exported via
  `src/tui/app/state/mod.rs`.

Extended modules:

- `src/tui/app/state/view_switch.rs` — `ViewSwitchCoordinator` gains
  `begin_loading`, `commit_if_matches`, `suspend_if_matches`,
  `replace_active` with epoch-checked guards.
- `src/tui/task_lifecycle.rs` — `TuiTaskRecord` gains `scope_tab_id`,
  `scope_session_id`, `scope_active_view_epoch`; new methods
  `spawn_with_scope`, `cancel_for_tab`, `cancel_for_session`,
  `cancel_for_stale_epoch`.
- `src/tui/async_cmd.rs` — new `spawn_scoped_registered_tui_task`.
- `src/tui/runtime/app_events.rs` — bus event dispatch wraps existing
  `handle_event_inner` with the classifier; new test helpers and
  3 inline tests.
- `src/tui/app/mod.rs` — `App` gains `routing_registry: RoutingRegistry`
  field; `set_session` registers session in registry.
- `src/tui/commands/project_picker.rs` — `close_active_project_tab`
  calls `routing_registry.drop_tab` and `task_registry.cancel_for_tab`.

Integration tests:

- `tests/tui_project_routing.rs` (18 tests) — covers stale token
  rejection, pre-reconnect rejection, classifier routing, summary
  saturation/bounds, registry drop, task scoping by tab/session/epoch,
  view switch transitions, session rebinding.

## 4. Verification executed

### Commands run

```bash
# focused unit tests
cargo test -p codegg --lib --features lsp-test-support -- routing
cargo test -p codegg --lib --features lsp-test-support -- view_switch
cargo test -p codegg --lib --features lsp-test-support -- task_lifecycle
cargo test -p codegg --lib --features lsp-test-support -- app_events

# integration tests
cargo test --test tui_project_routing --features lsp-test-support

# compilation check
cargo check -p codegg --features lsp-test-support --lib
```

### Results

- `cargo test -p codegg --lib routing` — 61 passed, 0 failed.
- `cargo test -p codegg --lib view_switch` — 15 passed, 0 failed.
- `cargo test -p codegg --lib task_lifecycle` — 22 passed, 0 failed.
- `cargo test -p codegg --lib app_events` — 3 passed, 0 failed.
- `cargo test --test tui_project_routing` — 18 passed, 0 failed.
- Total: 119 tests passed, 0 failed.
- `cargo check -p codegg --features lsp-test-support --lib` — 0 errors, 2 warnings (pre-existing).

## 5. Invariant review

- **The TUI remains a daemon client and never becomes storage authority.** Pass. The routing registry is derived frontend state that can be reconstructed from `ProjectTabs` + canonical daemon responses. No durable authority moved into the TUI.

- **Events update only the intended tab/session.** Pass. `classify_event` routes session-scoped events through the session→tab index. Only the matching tab's activity summary is updated.

- **Rapidly switching or closing tabs cannot apply stale completions elsewhere.** Pass. `commit_if_matches` checks `active_view_epoch` before applying. Pre-reconnect events are rejected by `reconnect_epoch`. Tests verify stale token and pre-reconnect rejection.

- **Pending permissions/questions never steal focus across sessions.** Pass. InactiveSummary handler shows a bounded toast rather than opening a dialog for another tab's permission/question.

- **Closing/inactivating a tab releases frontend resources but does not cancel daemon execution.** Pass. `close_active_project_tab` calls `routing_registry.drop_tab` and `task_registry.cancel_for_tab` which only remove frontend state. Daemon jobs are not cancelled.

- **Several TUIs operating different projects remain isolated.** Pass. The routing registry indexes by session_id→tab_id; sessions from different TUIs would have different session IDs and cannot cross-route.

- **No project authority is inferred from cwd.** Pass. The routing registry is populated from `ProjectTabs` and `App::set_session`, not from `std::env::current_dir()`. `scripts/check_daemon_cwd_usage.py` passes.

## 6. Failure and recovery review

- **Unknown session events are dropped with diagnostic.** Pass. `classify_event` returns `DropDiagnostic` for unknown sessions (with active_view_epoch > 0) or `RefreshRequired` (with epoch = 0). No silent discard.

- **Stale completions are rejected.** Pass. `commit_if_matches` compares the task's `scope_active_view_epoch` against the current epoch. If they don't match, the completion is dropped.

- **Tab close cleans up routing and tasks atomically.** Pass. `drop_tab` removes both session_index entries and activity summary for the tab. `cancel_for_tab` removes all scoped tasks.

- **Session rebound to new tab cleans up old tab's activity.** Pass. `register_open_session` checks if the prior tab has remaining sessions; if not, removes its activity entry.

## 7. Migration and compatibility review

No destructive schema or storage migration was introduced. No CLI
flag, config key, or persisted file format changed. The new
`routing_registry: RoutingRegistry` field is populated in-memory only
and derived from the existing `ProjectTabs` state.

Existing single-project workflows remain fully functional. The routing
registry is only exercised when multiple tabs exist; a single tab
always routes events to the active view.

## 8. Security review

- The routing registry holds only tab/session/project/workspace IDs
  as strings. No credentials, tokens, or filesystem paths are stored.
- Permission/question events for inactive tabs are surfaced as bounded
  toasts; the user can switch to the correct tab to respond.
- All routing is frontend-only; daemon execution is never cancelled
  or redirected by routing decisions.
- `codegg-core` boundary check passes — no new dependency leaks.

## 9. Documentation and operations

No new documentation was required for this milestone. The routing
module is self-contained and follows existing patterns. The closure
record serves as the authoritative reference for the routing design.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Pre-existing baseline errors in `theme/target/mod.rs` (duplicate module), `python_script/executor.rs` (missing `with_mode`, unsafe `pre_exec`), and `theme/registry.rs` (type mismatch) required minimal fixes to enable testing. | These were present on the baseline and not introduced by this milestone. | Tracked separately; out of scope for this milestone. |

No medium, high, or critical finding remains.

## 11. Roadmap disposition

Milestone closed. The next hard dependency may proceed:

- Multi-Project TUI milestone 4 (persistent restoration, resource
  bounds, and closure) becomes dependency-ready.
- Session Projections milestone 3 (visibility, redaction, and
  artifact handles) continues independent of this milestone.

## 12. Registry updates

Required updates after this record is accepted:

- `plans/subsystems/tui-project-sessions-roadmap.md` milestone table
  row for milestone 3: status `closed`, closure record pointer set to
  this file, blocker column updated to `—`.
- `plans/registry.md`:
  - Move `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md`
    from active work to `recently closed work`, citing this record
    and the implementation commit.
  - Update milestone 4 status from `blocked` to `ready`.

These updates are recorded as part of the same commit that lands
this closure record.
