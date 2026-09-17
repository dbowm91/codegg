# Project Work Orders and Task View M004 — Global Workspace Dashboard and Team-Aware Task Projection

Status: blocked

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADR:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`

Primary class: capability

Hard dependency: M003 closure.

## 1. Objective

Evolve the existing project-picker/multi-project TUI infrastructure into a global “Workspace” dashboard that shows every project the principal may see, its coarse current/future WorkOrder and session activity, and whether anything needs attention or permission—without redefining canonical `Workspace`, eagerly activating projects, or issuing an N+1 query tree.

The dashboard must be reachable through a discoverable hotkey and `/workspace`, preserve the active session/tab when entered, use Vim-like navigation, and let the user descend into the relevant project/task/session.

## 2. Current implementation evidence

- `ProjectCatalog` list/get operations are daemon-owned, bounded, path-independent, and intentionally probe-free.
- the current project picker already supports catalog filtering, project opening/focusing, workspace selection/registration, project tabs, and view-switch epoch guards.
- project tabs already hold bounded session summaries and inactive activity summaries.
- routing already distinguishes active view, inactive summary, refresh-required, drop-diagnostic, and global events.
- presence, observation, project chat, and task/session events already use project-scoped bounded frontend projections and reconnect generation guards.
- unauthorized project enumeration is filtered at the daemon boundary.
- closing/switching a project tab releases frontend state but does not cancel daemon execution.

## 3. Invariants that must not regress

- The user-facing “Workspace” dashboard is not a new durable Workspace object and does not change the canonical meaning of `WorkspaceId`.
- Dashboard data is daemon-owned projection; TUI never scans files, queries cwd, or reconstructs global truth by joining local caches.
- Project catalog listing remains cheap/probe-free.
- Dashboard refresh does not initialize LSP, Git, provider, build, or workspace services for every project.
- Unauthorized projects/tasks/sessions are absent or privacy-equivalent to absent according to current authorization policy.
- Pending permission/question signals never disclose prompt/tool secret content at global scope.
- Entering/leaving dashboard does not cancel or mutate running work.
- Stale/reconnect completions cannot apply to the wrong project/tab/dashboard generation.
- The dashboard stays bounded for deployments with many projects.

## 4. Scope

### In scope

- bounded daemon-side global/project activity projection;
- aggregate counts/status for WorkOrders and sessions per authorized project;
- attention, pending permission/question, and coarse health indicators;
- reuse/evolution of current project-picker state/component or a shared catalog projection owner;
- `/workspace` command and one configurable hotkey/action;
- Vim-like `j/k`, arrows, `g/G`, search/filter, Enter, Esc/back semantics consistent with existing TUI conventions;
- project row expansion/detail fetch only on explicit selection/focus;
- navigation from dashboard → project Task view → materialized session;
- team privacy/authorization and principal-specific filtering;
- event/hint-driven stale marking with bounded refresh behavior;
- remote TUI/frontend-neutral projection compatibility as appropriate;
- help/docs/tests.

### Explicitly out of scope

- scanning/discovering new projects beyond existing ProjectCatalog/discovery operations;
- project administration/member management UI;
- a second project tab model;
- web/mobile frontend implementation;
- raw prompt/tool-output display in global rows;
- remote-node scheduling changes;
- external task-trigger endpoint;
- agent WorkOrder tool.

## 5. Dashboard projection contract

Do not make the TUI implement:

```text
ProjectList
  -> for each project SessionList
  -> for each project WorkOrderList
  -> for each session permission/job query
```

Add one bounded daemon-side projection/request capable of returning rows such as:

```text
ProjectActivitySummary
  project_id
  display_name
  lifecycle/health coarse state
  running_session_count
  running_work_order_count
  waiting_work_order_count
  future_work_order_count
  needs_attention_count
  pending_permission_count
  pending_question_count
  last_activity_at?
  coarse_status_code
```

No full prompt, command arguments, filesystem paths, trigger secrets, provider secrets, diff bodies, or hidden reasoning belongs in the global row.

The projection may be derived from canonical stores on request plus event-maintained indexes/caches if needed for performance, but those caches are rebuildable and never become independent authority.

## 6. Authorization/privacy

Prefer an enumeration-style request that returns only projects visible to the bound principal. Per-row task/session counts must be computed only after project visibility/capability checks.

Decide whether Viewer-level principals may see task/session *counts* versus only project presence according to existing `project.read`, `project.observe`, and `session.read/observe` semantics. The decision must be explicit in the implementation and tests; do not leak existence by mixing unauthorized counts into a visible project row.

Attention labels must be coarse, e.g. `permission`, `question`, `failed`, `workspace`, `model`, not the underlying sensitive request content.

## 7. TUI route/view semantics

The dashboard should be a global route/overlay/view state, not destructive navigation.

Required behavior:

- entering stores enough frontend route state to return to the exact prior project/session view;
- Esc/back returns without reloading unrelated project state when still valid;
- Enter on a project focuses/opens its project tab and defaults to the project's Task/session summary view;
- Enter on an expanded running task opens the materialized session;
- future WorkOrder selection opens project Task detail rather than fabricating a session;
- current project tab/session remains alive while dashboard is shown;
- closing a project tab from elsewhere continues to invalidate matching stale route tokens.

Reuse current `ProjectPickerState`, `ProjectTabs`, `ViewSwitchCoordinator`, `RoutingRegistry`, and shared catalog filtering where practical. If the picker remains separately useful for registration, factor common catalog list/filter/navigation state rather than copy it.

## 8. Navigation/keybindings

Add a configurable `OpenWorkspaceDashboard` action and `/workspace` command. The default hotkey must pass the existing keybinding collision audit and be documented in Insert/Normal/Vim help surfaces.

Dashboard semantics should follow existing list views:

- `j/k` or Up/Down — select;
- `g/G` — top/bottom where normal-mode conventions already use them;
- `/` — filter/search if current command mode permits without conflict;
- Enter — descend/open;
- Esc — return;
- optional project-tab navigation bindings remain unchanged.

Do not hard-code a key inside the component outside the keybinding system unless it is a standard modal navigation key already treated that way elsewhere.

## 9. Lazy details and bounds

Global rows must have hard caps/cursors. Suggested behavior:

- initial page bounded to 50–128 projects consistent with current catalog bounds;
- stable ordering by attention/running activity then recency/name, or current existing catalog ordering plus explicit attention badges; choose and document deterministic order;
- search/filter applies to bounded/cursor-aware catalog data;
- task/session details fetched only for selected project;
- no worktree path/Git status probe until project is entered/activated;
- no model/provider refresh for inactive projects.

If exact counts are expensive, use bounded stored aggregate/index data with an explicit “stale/unknown” state instead of synchronous deep queries.

## 10. Attention integration

Global view should distinguish:

- pending human permission;
- pending structured question;
- WorkOrder needs-attention;
- failed/held sequence;
- project/workspace health unavailable;
- disconnected/remote execution where existing projection supports it.

A count/badge is enough. Selecting the project should route the user to the project Task/session view where detailed authorized context can be loaded.

No global modal should automatically steal focus because an inactive project produced a permission/question event.

## 11. Event/reconnect model

Prefer hint/event → mark row/project dirty → bounded refresh rather than shipping every detailed session/task event into the dashboard.

Maintain:

- dashboard request/generation ID;
- reconnect epoch;
- per-project stale/last-refresh markers;
- bounded LRU/cache if project activity rows are retained frontend-side.

Reconnect must rebuild from daemon projection. A stale cached count may be displayed only if clearly marked/within existing projection conventions; authorization revocation must clear hidden data immediately on denied refresh.

## 12. Ordered work packages

### A — Core/global activity projection

Define bounded summary DTO/service/query and project-authorized filtering. Add aggregate/index path only if measured/query review shows it is needed.

### B — Shared catalog/dashboard state

Refactor current picker/catalog list state where useful; add dashboard route state, stale guards, and return-to-prior-view semantics.

### C — Render/navigation

Implement project rows, badges, bounded scrolling/filtering, hotkey `/workspace`, Enter/Esc transitions, and narrow-terminal behavior.

### D — Project/task/session descent

Wire selected project into existing tab/task/session loading paths without duplicate ownership or eager activation.

### E — Event/reconnect/privacy qualification

Add hint refresh, reconnect resync, revocation clearing, inactive permission behavior, and team-role tests.

## 13. Required tests

Core/projection:

- only authorized projects returned;
- counts do not include unauthorized hidden resources;
- row fields bounded/redacted;
- large project set pages deterministically;
- query does not call project activation/heavy service constructors (static/mock evidence).

TUI:

- hotkey and `/workspace` open same dashboard;
- Esc returns to exact prior session/project;
- Enter focuses correct project/task/session;
- j/k/arrows/filter/top/bottom navigation;
- narrow terminal rendering;
- dashboard open does not cancel running TUI/daemon tasks;
- stale completion/reconnect epoch rejection;
- project revocation clears row/detail;
- inactive permission/question increments badge without focus theft;
- project picker registration behavior remains available and unchanged if dashboard reuses its internals.

Performance/behavior:

- opening dashboard with many registered projects does not trigger LSP/Git/provider/build activation;
- no N+1 SessionList/WorkOrderList behavior in the TUI.

## 14. Required verification

```bash
cargo test -p codegg --lib -- project_catalog
cargo test -p codegg --lib -- tui::
cargo test --test tui_project_picker
cargo test --test tui_project_tabs
cargo test --test tui_project_routing
cargo test --test tui --test tui_render
cargo test --test authorization
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 15. Acceptance criteria

- `/workspace` and a documented hotkey open one global bounded dashboard.
- All visible projects are authorization-correct for the current principal.
- Rows show coarse running/future/attention/permission/question state without sensitive content.
- Dashboard uses one bounded aggregate projection, not TUI-side N+1 fan-out.
- Opening it does not eagerly activate projects/services.
- User can descend to project Task view and materialized session, then return predictably.
- Vim-like navigation and existing project-tab semantics remain coherent.
- Revocation/reconnect/stale completion behavior is fail-closed/project-correct.

## 16. Stop conditions

Stop if the design requires redefining canonical Workspace, adding TUI-owned durable state, eager project activation, scanning project paths from the frontend, or authorizing rows based on cached frontend membership.

## 17. Closure evidence required

- implementation commits;
- projection DTO/bounds/privacy matrix;
- query/activation evidence showing no N+1 heavy activation;
- navigation/keybinding table;
- reconnect/revocation/stale-route test outcomes;
- large-project/narrow-terminal render evidence;
- exact verification commands and residual findings.
