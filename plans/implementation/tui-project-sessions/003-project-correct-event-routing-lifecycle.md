# Multi-Project TUI Milestone 003 — Project-Correct Event Routing and Lifecycle

Status: ready for handoff

Repository baseline: `f569386e4cb68d9752505c3b8d4205161a40c3c4` (`main`; planning-only commits after this baseline do not alter production behavior)

Source roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-3--project-correct-event-routing-and-lifecycle`

Applicable closure evidence:

- `plans/closure/tui-project-sessions/001-status.md`
- `plans/closure/tui-project-sessions/002-status.md`
- `plans/closure/project-catalog/004-status.md`
- `plans/closure/runtime-assets/004-status.md`

Primary class: correctness / lifecycle

## 1. Objective

Make every asynchronous completion, live daemon event, background task, and active-view transition project/session correct while several project tabs are open.

Milestone 002 made projects and tabs visible and selectable, but intentionally retained one legacy heavy `App::session_state` / `App::agent_state` rendering surface. Milestone 003 must turn that compatibility surface into a controlled active-view cache whose mutations are accepted only when they match the current tab, canonical project/workspace/session binding, and view epoch.

The milestone succeeds when rapid switching, closing, reconnecting, and concurrent activity in several projects cannot:

- render Project A data in Project B;
- apply a stale completion to a replacement tab or session;
- leak permission/question prompts across sessions;
- retain workspace services or background tasks after their owning tab/session is gone;
- silently discard relevant inactive-tab activity;
- create a second frontend authority for durable session state.

## 2. Why this milestone is ready

Multi-Project TUI Milestone 002 is closed at `f569386` and provides:

- `ProjectTabId`, ordered `ProjectTabs`, stable active-tab identity, and per-tab canonical project/workspace/session IDs;
- bounded project/session summaries;
- the `ProjectPickerState` workflow and project-local session selection;
- `ViewSwitchCoordinator` with `active_view_epoch`;
- bounded tab navigation and deterministic close fallback;
- async request-generation guards for picker and view loading;
- one active heavy compatibility view.

The daemon already exposes canonical session bindings, project-scoped catalog operations, session snapshots, runtime-asset generations, provider-selection state, and live core events. No new durable frontend store is needed.

Session Projections Milestone 004 will later migrate the TUI to canonical projection replay. This milestone must therefore build transport-neutral routing/lifecycle seams that can consume raw `CoreEvent` today and `ProjectionEnvelope` later without another tab model.

## 3. Current production evidence and gaps

At baseline `f569386`:

- `ProjectTabState` owns lightweight project/workspace/session/model/agent fields and bounded session summaries.
- `ViewSwitchCoordinator` rejects stale snapshot loads through an epoch, but that epoch is not systematically attached to all command completions and live event reducers.
- existing rendering and most event handling still mutate global `App::session_state`, `App::agent_state`, dialog state, toasts, Git state, run/test state, permission/question state, and task state;
- inactive tabs do not have a uniform bounded activity summary or unread/error/pending badge model;
- live events are primarily keyed by session ID, while several global or partially scoped event families require canonical routing context;
- some commands carry session IDs, some carry workspace/project IDs, and some rely on whichever session is active when completion arrives;
- tab close invalidates view loading but does not yet prove cancellation/cleanup across every task kind;
- reconnect/resume may replay events after the user has switched tabs, so sequence order alone is insufficient without identity/epoch checks;
- the TUI still has compatibility accessors and global fields that are easy for new code to mutate directly.

## 4. Invariants

- The daemon remains the sole durable project, workspace, session, run, job, and execution authority.
- `ProjectTabId` remains frontend-local and never enters daemon protocol identity fields.
- A mutation of active heavy state requires a matching tab ID, project ID, workspace ID, session ID where applicable, and active-view epoch.
- A live event is routed from canonical daemon identity, never from label, directory, cwd, tab index, or current focus alone.
- Stale completions are dropped before any state mutation, toast, dialog, badge, or task-lifecycle transition.
- Inactive tabs retain bounded summaries and activity indicators only; they do not retain full message history, file bodies, unbounded logs, LSP state, or exclusive workspace-service leases.
- Exactly one heavy active-session view is resident in this milestone.
- Switching tabs does not cancel daemon-owned turns, runs, jobs, or tests unless the user explicitly requests cancellation.
- Closing a tab releases frontend tasks/subscriptions/leases but never deletes or archives daemon-owned work.
- Permissions and questions are shown only for their owning session/context; inactive ownership is indicated without stealing focus.
- Provider/model/agent state is reconciled from the loaded canonical session, not trusted from stale tab presentation state.
- Global daemon events remain global only when their protocol contract is genuinely global.
- Unknown or unrouteable events fail closed to bounded diagnostics and resync/refresh requests rather than guessing an owner.
- The routing seam must be reusable by Session Projections Milestone 004.

## 5. Scope

### In scope

- A typed frontend operation/routing token containing tab ID, canonical project/workspace/session IDs, view epoch, reconnect epoch, and request generation.
- A central event-routing classifier for raw `CoreEvent` and existing command completions.
- A canonical session-to-tab index with explicit duplicate/open-session policy.
- Bounded per-tab activity summary: unread/activity count, pending permissions/questions, active turn/run/job/test flags, last error, health/refresh state, and last observed sequence.
- Controlled active-view load, commit, suspend, replace, and clear transitions.
- Systematic conversion of async command pairs to identity-bearing completions.
- Project/session-correct handling for turn, message, tool, permission, question, subagent, run, job, test, file-change, Git, asset-refresh, health, provider-selection, and session-lifecycle families.
- Inactive-tab routing that updates summaries without materializing heavy state.
- Explicit foregrounding behavior for permission/question dialogs.
- Frontend task/subscription/lease ownership by tab and session.
- Cleanup on tab close, session replacement, reconnect, transport loss, and app shutdown.
- Resync/refresh behavior for gaps, unknown ownership, archived/rebound sessions, and stale bindings.
- Static guards against direct global-state mutation from async completion paths.
- Focused unit, fake-client, transport, render, race, and lifecycle tests.
- TUI architecture and troubleshooting documentation.

### Explicitly out of scope

- Persistent tab restoration across TUI restart; Milestone 004 owns it.
- Migrating primary TUI state to `SessionProjectionSnapshot`; Session Projections Milestone 004 owns it.
- Final projection visibility/redaction policy.
- Multiple simultaneous heavy session views.
- Team presence, observer mode, chat, roles, or authorization.
- Changing daemon project/session identity semantics.
- Cancelling daemon execution merely because a tab becomes inactive or closes.
- A new TUI-specific daemon protocol.

## 6. Target architecture

### 6.1 Route identity

Add a compact immutable token, for example:

```text
UiRouteToken
|-- tab_id: ProjectTabId
|-- project_id: ProjectId string
|-- workspace_id: WorkspaceId string
|-- session_id: optional SessionId string
|-- active_view_epoch: u64
|-- reconnect_epoch: u64
`-- request_generation: u64
```

Every asynchronous command started for a tab captures this token. Its completion must validate all populated fields before mutation. A matching session ID with a mismatched project/workspace/binding is not acceptable.

Do not serialize `ProjectTabId` into daemon requests except as opaque client-local correlation metadata if an existing envelope explicitly supports it.

### 6.2 Routing registry

Introduce one frontend routing registry owned by `App` or a focused state module:

- `tab_id -> canonical tab scope`;
- `session_id -> tab_id` for open sessions;
- active tab and active heavy-view scope;
- per-tab task/subscription ownership;
- per-tab bounded activity summary;
- reconnect epoch and last accepted event sequence.

The registry is derived frontend state. It must be rebuilt from the current tabs and daemon responses; it is not durable authority.

Opening the same project continues to focus the existing project tab. If a project tab changes selected session, update the session index atomically and invalidate the old view epoch.

### 6.3 Event classification

Create one classifier that returns a routing decision such as:

```text
RouteDecision
|-- ActiveView { tab_id, scope }
|-- InactiveSummary { tab_id, scope }
|-- Global
|-- RefreshRequired { reason, scope }
`-- DropDiagnostic { reason }
```

Rules:

1. Prefer explicit canonical IDs in the envelope/payload.
2. Resolve session-owned events through the session-to-tab index and verify the tab binding.
3. Route project/workspace events only to matching tabs.
4. Treat truly daemon-global lifecycle/connection events as global.
5. Never route a sessionless event to the active tab merely because it is focused.
6. Unknown, ambiguous, archived, or rebound ownership triggers a bounded refresh/resync decision.

The classifier must be pure and independently testable.

### 6.4 Active heavy-view transaction

Model switching as an explicit transaction:

1. increment epoch and mark the requested target scope;
2. stop accepting old-scope completions into heavy state;
3. retain only bounded lightweight summary for the previous tab;
4. request canonical session snapshot/selection/assets/health as required;
5. validate token and canonical binding on completion;
6. atomically replace heavy view and route;
7. subscribe/attach event ownership for the new scope;
8. mark the view ready and repaint.

Failure leaves the prior heavy view or a bounded explicit error state; it must not partially merge two sessions.

### 6.5 Inactive-tab summaries

Extend `ProjectTabState` with bounded presentation-only fields. Suggested fields:

- `activity_revision`;
- `unread_count` with saturation cap;
- pending permission/question counts;
- active turn/run/job/test booleans or small counters;
- last bounded status/error summary;
- last accepted event sequence/cursor hint;
- project health/runtime-asset summary;
- stale/resync-required flag.

Do not store message bodies, tool outputs, diffs, logs, or full event queues.

### 6.6 Dialog and foreground policy

- A permission/question for the active session may open the existing dialog.
- An inactive-session request updates that tab's badge and a bounded toast naming the project/session; it must not replace the active dialog.
- Selecting the indicated tab/session then loads canonical state and obtains the pending request from daemon state before rendering it.
- Responses carry daemon-issued request/session identity and are rejected if the dialog ownership token is stale.

### 6.7 Task and resource ownership

Extend `TuiTaskRegistry` or add a thin ownership index so tasks can be queried/cancelled by:

- tab ID;
- project/workspace/session scope;
- command kind;
- view epoch.

On close/replacement/reconnect:

- cancel frontend-only loads, probes, searches, and subscriptions;
- release frontend-held workspace/runtime leases;
- preserve daemon-owned execution;
- remove stale completion channels;
- ensure task completion cannot recreate removed tab state.

### 6.8 Compatibility and future projection adoption

Keep raw `CoreEvent` support. Place routing behind an adapter boundary that Session Projections Milestone 004 can feed with `ProjectionEnvelope` and replay/resync results.

Do not embed raw-event-specific assumptions in tab state. The active view reducer and inactive summary reducer should accept a small normalized frontend event shape where practical.

## 7. Work packages

### A — Routing identities and registry

- Add `UiRouteToken`, routing registry, session index, and activity-summary model.
- Add validation helpers and diagnostics.
- Prove index updates are atomic on session switch and tab close.

### B — Active-view lifecycle

- Convert view switching into explicit transactional phases.
- Validate canonical snapshot bindings before commit.
- Reconcile model/agent/provider/assets from daemon state.
- Add error and retry states.

### C — Async completion migration

- Inventory all TUI `start_*` / `apply_*`, spawned tasks, and completion variants.
- Attach routing tokens to project/session-scoped operations.
- Reject stale tokens before side effects.
- Add a guard/checklist preventing new unscoped completion variants.

### D — Live event routing

- Implement pure classifier and active/inactive/global reducers.
- Cover all event families and unknown variants.
- Add refresh/resync path for ownership gaps.

### E — Dialogs, tasks, and cleanup

- Bind dialogs to explicit ownership.
- Scope task/subscription/lease lifecycle.
- Cleanly handle tab close, transport loss, reconnect, shutdown, and session rebind.

### F — Verification and documentation

- Add deterministic race/fake-client suites.
- Stress rapid switching and concurrent project activity.
- Update architecture/TUI/keybinding/troubleshooting docs.
- Produce closure evidence.

## 8. Required tests

- stale snapshot completion after tab switch is dropped;
- stale completion after tab close cannot recreate state;
- identical session titles/IDs in different projects do not cross-route;
- Project A turn/tool/message events never mutate Project B heavy view;
- inactive Project A events update only bounded Project A summary;
- inactive permission/question does not steal Project B focus;
- foregrounding the owning tab retrieves and renders the pending request;
- project/workspace health and asset events route only to matching tabs;
- global connection event remains global;
- session rebind invalidates old route and requests resync;
- archived/missing project produces bounded unavailable state;
- replayed duplicate event is idempotent;
- reconnect epoch rejects pre-reconnect completions;
- queue/event gap requests bounded resync;
- switching during active turn does not cancel the turn;
- closing a tab does not cancel daemon run/job/test;
- frontend tasks/subscriptions/leases are released on close and shutdown;
- rapid randomized switching with concurrent events preserves identity invariants;
- inactive summaries remain within caps;
- older daemon/single-tab compatibility remains functional;
- current TUI, render, picker, tab, session-selection, lifecycle, protocol, and static-guard suites remain green.

## 9. Acceptance criteria

- Every project/session-scoped async completion carries and validates an explicit routing token.
- Every live event family has an explicit routing classification.
- No active heavy-state mutation relies only on current focus, route, directory, or tab index.
- Inactive tabs receive bounded summaries without heavy-state duplication.
- Permission/question ownership is project/session correct.
- Switching and closing are safe under in-flight loads and events.
- Frontend tasks, subscriptions, and leases have explicit owners and bounded cleanup.
- Daemon-owned execution survives tab inactivity/closure unless explicitly cancelled.
- Reconnect and replay cannot apply stale pre-reconnect completions.
- Unknown/ambiguous ownership fails closed with refresh/resync diagnostics.
- The routing boundary can consume canonical projection events later without replacing `ProjectTabs`.
- No new path-derived project authority or frontend storage authority is introduced.
- Resource caps and existing compatibility behavior are preserved.
- Architecture documentation and a strict closure record are complete.

## 10. Verification commands

At minimum:

```bash
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo test -p codegg --lib tui::
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test --test tui --test tui_render
cargo test --test session_selection
cargo test --test single_daemon_lifecycle
cargo test -p codegg-protocol
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

Add focused `tui_project_routing` and lifecycle/race integration targets rather than hiding the new contract only inside the broad TUI suite.

## 11. Handoff and downstream unlock

When this plan is strictly closed:

- Multi-Project TUI Milestone 004 becomes dependency-ready.
- Session Projections Milestone 004 may rely on the normalized routing and active-view lifecycle seams, but still remains blocked on Session Projections Milestone 003.

Do not start TUI Milestone 004 implementation merely because this file exists; require the Milestone 003 closure record.