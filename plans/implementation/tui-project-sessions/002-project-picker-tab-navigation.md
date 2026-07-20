# Multi-Project TUI Milestone 002 — Project Picker and Tab Navigation

Status: ready for handoff

Repository baseline: `1c37787afc6b2afd437f1d3f21a6fe26226a73d7` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `d1e5b70` — bounded `project_catalog.v1` protocol operations and request-scoped server project/workspace authority.
- `972c286` and `2293a11` — project/workspace-scoped runtime-asset refresh, immutable generations, and session lifecycle refresh behavior.
- `62e26b1` — typed `ProjectTabId`, ordered `ProjectTabs`, bounded `ProjectCatalogState`, asynchronous catalog loading, active-tab accessors, and strict one-tab compatibility startup.
- `f6c8669` — frontend-neutral projection contracts and canonical reducer. This milestone must remain compatible with that contract but does not adopt it as the TUI's primary state authority; frontend projection adoption remains Session Projections Milestone 004.

Source roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-2--project-picker-and-tab-navigation`

Applicable closure evidence:

- `plans/closure/project-catalog/004-status.md`
- `plans/closure/runtime-assets/003-status.md`
- `plans/closure/runtime-assets/004-status.md`
- `plans/closure/tui-project-sessions/001-status.md`
- `plans/closure/session-projections/001-status.md`

Applicable ADRs:

- None. The canonical documents already establish daemon-owned project/session authority and frontend-local project tabs. Stop for an ADR if implementation requires making tab state durable authority, changing project identity/cardinality, or replacing the native protocol with a TUI-specific project API.

Primary class: capability

## 1. Objective

Make the Milestone 001 project-tab state usable through a complete bounded navigation surface:

- `Space f` opens a searchable project picker backed by `ProjectCatalogState`;
- selecting a project opens or focuses one project tab;
- projects with several workspaces require an explicit workspace choice;
- each tab can list and select sessions belonging to its canonical project/workspace scope;
- users can move to the next/previous tab, select a tab directly, and close a tab without mutating daemon-owned sessions;
- the active tab's lightweight model/agent/session selection is restored without keeping full session histories resident for every inactive project;
- an explicit, local-safe one-off registration path can register an existing workspace and then register it as a project without creating directories or treating a path as identity.

The milestone succeeds when one TUI process can visibly open, switch among, and close several project tabs while preserving the existing single-project workflow, using only daemon-issued project/workspace/session IDs.

## 2. Why this milestone is ready

Multi-Project TUI Milestone 001 is closed and provides the required frontend state seam:

- opaque frontend-local `ProjectTabId`;
- ordered `ProjectTabs` with stable active-tab identity;
- per-tab project/workspace/session/model/agent summaries;
- bounded `ProjectCatalogState` and asynchronous list refresh;
- request-generation scaffolding and stale-result rejection;
- active-tab accessors that preserve compatibility with the current single-project render path.

Project Catalog Milestone 004 is closed and provides:

- `ProjectList` and `ProjectGet` with bounded project/workspace summaries;
- `ProjectRegister` for an already registered `WorkspaceId`;
- `WorkspaceRegister` for an explicit existing local root;
- stable project lifecycle and health responses;
- `SessionList` and existing session load/attach/create operations that resolve canonical session bindings;
- no process-global `ServerState.project_dir` authority.

No additional daemon storage or discovery capability is required to implement navigation.

## 3. Current implementation evidence

At the repository baseline:

- `src/tui/app/state/project_tabs.rs` already supports add, activate, lookup, ordered iteration, project lookup, active identity updates, deterministic removal fallback, and one compatibility tab.
- `ProjectTabState` contains only lightweight identity and selected model/agent state. Full messages, changed files, Git sidebar data, prompt history, and other heavy active-session data remain in the existing `App::session_state` compatibility surface.
- `src/tui/commands/project_catalog.rs` performs `ProjectCatalogCapabilities` followed by bounded `ProjectList` using the established `start_*` / `apply_*` async command pattern.
- `ProjectGet` is protocol-tested but has no TUI detail command or cache.
- There is no project picker dialog, workspace picker, project-local session picker, tab bar, visible tab-selection action, next/previous/close action, or one-off project registration workflow.
- Existing render and input paths still assume one active session and route; a tab switch therefore needs a controlled active-view transition rather than direct mutation of global fields.
- Milestone 001 intentionally leaves exactly one compatibility tab at startup and mirrors `set_session`, model, and agent changes into that tab.

## 4. Invariants that must not regress

- The daemon and native protocol remain the sole project, workspace, and session authority.
- A `ProjectTabId` is frontend-local and is never serialized as a `ProjectId`, `WorkspaceId`, or `SessionId`.
- Paths are display locators or explicit local registration input only. They are never project IDs and are never used to merge projects.
- Opening an already-open project focuses the existing tab by default; it does not silently create duplicate tabs for the same project. A future explicit duplicate-view feature is out of scope.
- Closing a tab never deletes, archives, restores, cancels, or otherwise mutates daemon-owned sessions, jobs, runs, projects, or workspaces.
- Switching tabs never changes process cwd.
- Inactive tabs retain only bounded lightweight summaries and selection intent. They do not retain full message histories, file bodies, diff bodies, LSP state, or exclusive workspace-service leases.
- There is exactly one active heavy view/session state in this milestone. Switching must replace that view only after an identity-matched asynchronous load succeeds.
- Asynchronous project detail, workspace, session-list, session-load, and registration completions carry tab/project/workspace identity plus a request generation and are rejected when stale.
- Model/agent/provider selection follows session/daemon authority when a loaded session disagrees with stale tab presentation state.
- Existing modal priority, input routing, task tracking, terminal restoration, prompt submission, and provider-selection behavior remain correct.
- Project picker and tab rendering remain bounded for large catalogs and narrow terminals.
- Unsupported older daemons remain in explicit one-tab compatibility mode.

## 5. Scope

### In scope

- Project picker dialog opened by `Space f`.
- Bounded filtering/fuzzy matching over already loaded `ProjectSummaryDto` values.
- Loading/error/unsupported/truncated/empty/archived picker states.
- On-demand `ProjectGet` for the selected project.
- Explicit workspace selection when a project has more than one usable workspace.
- Opening a new tab or focusing an existing tab for a selected project.
- A bounded visible tab strip or equivalent tab indicator integrated into the existing TUI layout.
- Direct tab selection plus next/previous/close actions through the existing configurable keybinding/action system.
- Deterministic close fallback and a protected final compatibility/empty tab state.
- Project-local bounded session list loading through `SessionList` using the stable project ID.
- Session selection/load/attach for the active tab with canonical binding validation.
- Project-aware new-session creation through the active project/workspace context.
- Lightweight per-tab selected session/model/agent/provider presentation restoration.
- A controlled active-view switching transaction around the existing single heavy `SessionState` and related compatibility fields.
- Explicit one-off local project registration using `WorkspaceRegister` followed by `ProjectRegister`.
- Local/remote transport restrictions for path-bearing registration.
- Focused unit, fake-client, render, keybinding, and integration tests.
- TUI/keybinding/architecture documentation.

### Explicitly out of scope

- Persistent tab restoration across process restart; Milestone 004 owns it.
- Full project-correct routing of every streaming event, dialog, Git completion, and background task under rapid switching; Milestone 003 owns the systematic reducer/lifecycle pass.
- Keeping an independent full `SessionState` for every inactive tab.
- Canonical adoption of `SessionProjectionSnapshot`; Session Projections Milestone 004 owns frontend migration.
- Project discovery-root configuration or scan controls.
- Project archive/restore management UI beyond displaying lifecycle/unavailable state.
- Arbitrary remote filesystem browsing or remote path registration.
- Creating directories from picker input.
- Multiple first-class tabs for the same project.
- Team presence, observer mode, chat, ACP, or authorization policy.
- Persistent frontend preferences.

## 6. Required production changes

### 6.1 Picker and dialog state

Add a dedicated project picker state, following existing dialog ownership patterns rather than embedding mutable picker fields across `App`:

- query text;
- filtered catalog indices or stable project IDs;
- selected row;
- current phase: catalog, workspace selection, registration input/confirmation, or error;
- request IDs for project detail, workspace registration, project registration, and session list;
- bounded diagnostic/error text;
- whether archived projects are shown;
- a snapshot of the catalog generation used to build the result set.

Add `Dialog::ProjectPicker` or an equivalently explicit dialog variant. Do not overload the session picker or `Goto` dialog with project authority.

Picker filtering must:

- operate only on bounded daemon-returned summaries;
- match display name, bounded tags, and stable project ID where useful;
- preserve deterministic ordering for equal scores;
- cap rendered rows to the viewport and an implementation constant;
- remain responsive without spawning one task per keystroke;
- rebuild safely when a catalog refresh changes entries.

### 6.2 Catalog detail and workspace selection

Add a typed TUI command pair for `ProjectGet`:

- start with `(request_id, target_project_id, initiating_tab_id or picker_generation)`;
- return bounded `ProjectDetailsDto` or a typed error;
- reject stale completion if the picker closed, catalog generation changed, or target project changed.

Workspace rules:

1. Zero workspaces: permit an informational/unavailable tab only when useful, but do not create or attach a session; show an actionable diagnostic.
2. One workspace: select it automatically after validating the returned IDs.
3. Several workspaces: require an explicit workspace choice; never choose the first row implicitly.
4. Archived/unavailable project or workspace: preserve visibility and actionable state; do not activate or create sessions.
5. A workspace locator is display-only and may be omitted for remote clients.

### 6.3 Opening and focusing tabs

Add one high-level `open_or_focus_project` transition that owns the complete operation:

- look up an existing tab by canonical project ID;
- if present, initiate the controlled switch to that tab;
- otherwise enforce `MAX_OPEN_PROJECT_TABS` (initial target: 16; expose the constant and test it);
- create a `ProjectTabState` from `ProjectDetailsDto` and the selected workspace;
- assign a fresh `ProjectTabId`;
- initialize tab-local request state and empty selected session;
- add and activate only after the project/workspace selection is valid;
- start a bounded project-local session list request;
- close the picker after the transition is committed.

Do not create a tab from a raw catalog row before workspace resolution completes.

### 6.4 Active-view switch transaction

Milestone 001 intentionally keeps one global heavy session/render state. Preserve that bound by introducing a controlled switch coordinator rather than cloning that state into every tab.

Required transition:

1. Snapshot outgoing lightweight selection into the outgoing tab: selected session ID, model, agent, provider connection/model presentation, and bounded view hints that are already safe to retain.
2. Increment an `active_view_epoch` or equivalent generation before starting any incoming load.
3. Mark the incoming tab as pending and select its tab ID for presentation without yet applying stale heavy data.
4. If the tab has a selected session, request session load/attach through `CoreClient`; require the returned canonical binding to match the tab's project/workspace.
5. Apply the heavy `SessionState`, route, messages, model/agent/provider selection, Git refresh trigger, and related compatibility fields only when `(tab_id, project_id, workspace_id, session_id, active_view_epoch)` still match.
6. If the tab has no selected session, clear active session-specific heavy state through one explicit helper while preserving global daemon/provider/dialog state.
7. On failure, keep the tab selected with an actionable load error and do not restore the outgoing project's heavy state under the incoming tab identity.

This switch transaction is a temporary compatibility bridge. It must be documented so Milestone 003 can systematically route remaining async/event paths and Session Projections Milestone 004 can later replace bespoke session reconstruction.

### 6.5 Project-local session list and selection

Add a per-tab bounded session summary cache and request state. Do not reuse one global session dialog list across projects without scope.

Session list requests must:

- use the tab's stable project ID;
- carry tab ID, project ID, selected workspace ID, request generation, and reconnect epoch;
- clamp the requested limit;
- validate each returned canonical binding;
- exclude or explicitly mark sessions bound to another workspace when the tab is workspace-specific;
- retain archived sessions only when the picker explicitly requests them;
- avoid loading messages for list rows.

Selecting a session must:

- reject a session whose canonical project/workspace binding mismatches the active tab;
- update the tab's selected session only through the active-view switch/load transition;
- use existing session load/attach operations;
- refresh tab presentation from the daemon-owned model/agent/provider selection after load;
- preserve identical session titles across projects without collision.

New session creation must pass the active tab's canonical project/workspace context. No directory-only create fallback is allowed when `project_catalog.v1` is supported.

### 6.6 Tab strip and navigation actions

Render a compact bounded tab strip or equivalent project-tab indicator using `ProjectTabs::ordered()`:

- active tab visually distinct;
- labels truncated safely by Unicode boundary;
- duplicate labels disambiguated with a short stable suffix or workspace summary, never a path-derived ID;
- archived/unavailable/pending/error state indicated without unbounded text;
- overflow handled by a sliding visible window around the active tab rather than horizontal unbounded allocation;
- narrow terminals retain a usable active-tab label and prompt.

Add configurable actions for:

- open project picker (`Space f` is the required default);
- next project tab;
- previous project tab;
- close active project tab;
- optionally select a visible tab by index when compatible with the current input model.

The implementation must inventory existing keybindings and choose non-conflicting defaults through the existing keybinding schema. Do not hardcode input handling outside the action/keybinding system. Document all final defaults.

Closing behavior:

- remove only frontend tab state;
- invalidate its pending requests and active-view epoch;
- never issue session/project deletion or archive requests;
- if closing the active tab, choose the adjacent previous tab when available, otherwise the next, using one documented deterministic rule;
- if the last tab closes, create or retain one bounded empty/compatibility view rather than leaving render paths with invalid assumptions;
- load the fallback tab through the same controlled switch transaction.

### 6.7 One-off local registration

Provide an explicit registration path accessible from the picker when the catalog does not contain the desired current/local workspace.

Safe sequence:

1. Accept an existing local directory only from a trusted local TUI context. Do not create it.
2. Canonicalize/validate through the daemon's `WorkspaceRegister`; the TUI does not establish identity itself.
3. Receive the canonical `WorkspaceId`.
4. Collect bounded display name and optional bounded metadata.
5. Call `ProjectRegister` with that workspace ID.
6. Refresh the catalog and open/focus the returned project.

Restrictions:

- Raw path registration is disabled over transports that cannot prove they address the daemon's local filesystem context.
- Remote clients may register only an already-known `WorkspaceId`, if the protocol/client context exposes one safely.
- No `mkdir`, clone, discovery scan, path-to-project conversion, or automatic repository merge.
- Errors from missing path, permission denial, duplicate/reconciled project, archived workspace, or stale registration must be actionable and bounded.

### 6.8 Async task ownership and cancellation

Extend the existing `spawn_registered_tui_task` pattern. Every new completion must contain enough identity to reject stale results:

- picker/catalog generation;
- `ProjectTabId` where applicable;
- canonical project/workspace/session IDs;
- request ID;
- active-view/reconnect epoch.

Closing a tab or picker cancels/invalidate UI-owned tasks where possible. It does not cancel daemon jobs, runs, turns, or project activation unless a separate explicit operation owns that cancellation.

Milestone 003 will perform the broad audit of existing streaming events and all legacy async commands. Milestone 002 must nevertheless make all newly introduced operations fully scoped and stale-safe.

### 6.9 Compatibility

When `project_catalog.v1` is unsupported:

- `Space f` opens an explicit unsupported/single-project compatibility notice or leaves the action disabled with discoverable help;
- the existing compatibility tab and session workflow continue;
- tab navigation actions remain harmless when only one tab exists;
- no synthetic catalog is built from cwd or local session rows.

When catalog capability is supported but the list is empty:

- show an empty state and local registration action when allowed;
- preserve the current canonical compatibility tab if it is already bound;
- never infer a project from its label or locator.

## 7. Ordered work packages

### Work package A — State and transition extensions

Intent: extend Milestone 001 state without introducing multiple heavy session authorities.

Required changes:

- add picker state and dialog variant;
- add per-tab project detail/session summary/load state;
- add active-view epoch and switch coordinator;
- add tab capacity and label bounds;
- define deterministic close fallback;
- document outgoing/incoming lightweight versus heavy state ownership.

Acceptance evidence:

- state unit tests cover capacity, existing-project focus, workspace choice, close fallback, and stale switch completion;
- only one heavy active session state exists;
- no direct database/filesystem project authority is added.

### Work package B — Picker filtering and rendering

Intent: expose the bounded daemon catalog through `Space f`.

Required changes:

- keybinding/action registration;
- picker component and render path;
- deterministic bounded search/filter;
- loading/error/unsupported/truncated/empty/archive states;
- project detail and workspace phase transitions;
- modal/focus integration.

Acceptance evidence:

- input and snapshot tests for open/filter/move/select/cancel;
- narrow-terminal render tests;
- catalog refresh while picker open cannot apply stale selection.

### Work package C — Open/focus/switch tabs

Intent: make project selection change the active frontend context safely.

Required changes:

- `open_or_focus_project`;
- controlled active-view switch transaction;
- visible tab strip;
- next/previous/direct/close actions;
- fallback/empty tab behavior;
- outgoing lightweight selection capture.

Acceptance evidence:

- several tabs can be opened and navigated;
- duplicate project selection focuses existing tab;
- closing a tab never sends destructive daemon requests;
- slow session load cannot overwrite a later tab selection.

### Work package D — Project-local sessions

Intent: bind session navigation to the active project/workspace.

Required changes:

- per-tab session list request/cache;
- project-local session picker integration;
- binding validation;
- session load/attach and new-session context;
- model/agent/provider presentation reconciliation.

Acceptance evidence:

- identical titles across projects remain distinct;
- cross-project or cross-workspace session result is rejected;
- changing tabs restores the selected session presentation without retaining every full history.

### Work package E — One-off registration

Intent: allow an explicit local existing workspace to enter the catalog without weakening identity boundaries.

Required changes:

- local/trusted transport gate;
- `WorkspaceRegister` then `ProjectRegister` flow;
- bounded input and error state;
- refresh/open result;
- remote path registration denial.

Acceptance evidence:

- local existing directory registration succeeds through daemon-issued IDs;
- nonexistent and unauthorized paths fail without directory creation;
- remote raw path attempt is rejected;
- duplicate/reconciled project focuses the canonical result.

### Work package F — Regression, bounds, and documentation

Intent: close the capability without destabilizing the current TUI.

Required changes:

- focused fake-client and renderer tests;
- keybinding collision audit;
- task/cancellation tests;
- update `architecture/tui.md`, protocol/client notes where needed, help/keybinding documentation, and troubleshooting;
- add static review guidance against tab-to-daemon ID confusion and TUI path authority.

Acceptance evidence:

- focused and broad suites pass;
- tab/picker/session lists remain bounded;
- existing single-project workflow and terminal cleanup remain green.

## 8. Failure, cancellation, restart, and contention semantics

- Catalog refresh failure leaves existing tabs usable and exposes a bounded retryable error.
- Project detail failure does not create a partially bound tab.
- Workspace selection cancellation returns to the project list without mutation.
- Session list/load failure leaves the selected tab in an actionable unloaded state; it cannot show the previous project's heavy state under the new tab label.
- A stale project detail, session list, or session load completion is dropped when its picker generation, tab ID, canonical IDs, request ID, reconnect epoch, or active-view epoch no longer matches.
- Repeated selection of one project coalesces to one open/focus transition.
- Tab capacity failure is explicit and does not evict another tab automatically.
- Closing a tab invalidates frontend requests but preserves daemon operations.
- Daemon reconnect clears or revalidates catalog/project/session summaries before permitting destructive or session-creation actions.
- This milestone does not persist tabs; process restart returns through the existing one-tab startup path. Persistent restoration remains Milestone 004.
- Two TUI clients have independent tab sets and do not acquire a daemon-level frontend tab lock.

## 9. Required tests

### State and reducer unit tests

- picker query/filter ordering and bounds;
- project detail/workspace phase transitions;
- project already open focuses existing tab;
- open-tab capacity;
- deterministic next/previous/close fallback;
- duplicate labels are safely disambiguated;
- outgoing lightweight selection capture;
- stale active-view epoch rejection;
- final-tab empty/compatibility behavior.

### Fake-client integration tests

- capability/list/get/open flow;
- one-workspace automatic selection;
- multi-workspace explicit selection;
- zero-workspace/unavailable project state;
- project-local session list and selection;
- session binding mismatch rejection;
- slow load after tab switch is dropped;
- local workspace/project registration sequence;
- remote raw path registration denial;
- unsupported daemon compatibility.

### Input/focus/render tests

- `Space f` opens picker;
- filter, navigation, select, escape, and nested workspace selection;
- tab strip at wide and narrow terminal widths;
- next/previous/close key actions;
- modal priority and prompt focus restoration;
- no accidental prompt input while picker owns focus.

### Regression tests

- existing single-project startup and session flow;
- prompt submission and active turn behavior;
- session dialog, provider/model/agent selection;
- Git sidebar refresh;
- terminal cleanup and panic restoration;
- no-session startup;
- local/inproc/socket/stdio client compilation.

### Security and negative tests

- no cwd lookup or path-derived project identity;
- no direct project/session database reads in TUI;
- raw path registration prohibited remotely;
- nonexistent path is not created;
- archived/mismatched IDs fail actionably;
- tab/debug rendering contains no credentials or asset bodies;
- close action emits no delete/archive/cancel protocol request.

## 10. Required verification commands

Run the exact available targets after inspecting current `main`; do not preserve obsolete target names from older plans.

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings

cargo test -p codegg --lib tui::app::state::project_tabs -- --test-threads=1
cargo test -p codegg --lib tui:: -- --test-threads=1
cargo test --test tui_project_tabs -- --test-threads=1
cargo test --test tui -- --test-threads=1
cargo test --test tui_render -- --test-threads=1
cargo test --test single_daemon_lifecycle -- --test-threads=1
cargo test --test session_selection -- --test-threads=1
cargo test -p codegg-protocol -- --test-threads=1

python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
git diff --check
```

Add and run focused targets for the picker, tab switching, and registration workflow. Use the repository's resource-constrained full-suite convention for broad validation rather than increasing process/thread fan-out.

## 11. Documentation updates

Update:

- `architecture/tui.md` — picker phases, tab strip, active-view switch transaction, lightweight/heavy state boundary, session selection, close semantics, compatibility mode, and deferred Milestone 003 audit.
- `architecture/client.md` or protocol client guidance — project list/get/session list and local registration usage where necessary.
- keybinding/help documentation — final picker/tab defaults and conflict resolution.
- troubleshooting — unsupported catalog, unavailable project/workspace, registration denial, stale load, and tab capacity.
- source comments in `project_tabs.rs` — update Milestone 001-only wording where the collection becomes user-operable.

## 12. Acceptance criteria

- `Space f` opens a bounded searchable daemon-backed project picker.
- Selecting a project with one workspace opens or focuses one project tab.
- Selecting a project with several workspaces requires explicit workspace choice.
- Several distinct project tabs are visible and navigable in one TUI process.
- Opening an already-open project focuses the existing tab.
- Tab next/previous/direct/close actions are configurable, tested, and focus-correct.
- Closing a tab preserves all daemon-owned project/session/job/run state.
- The active tab can list and select only canonically matching project/workspace sessions.
- Switching tabs restores selected session/model/agent/provider presentation through one controlled heavy-view transition.
- Slow/stale completions cannot overwrite a later active tab.
- New sessions use explicit project/workspace context when catalog capability is supported.
- Local one-off registration uses `WorkspaceRegister` then `ProjectRegister`; no path becomes identity and no directory is created.
- Remote raw path registration is rejected.
- No cwd mutation, direct database authority, frontend project synthesis, or per-inactive-tab full session history is introduced.
- Existing single-project behavior remains functional.
- Milestone 003 can audit and complete project-correct event/async lifecycle without redesigning picker/tab ownership.

## 13. Stop conditions

Stop and report rather than improvising when:

- the current protocol cannot return canonical project/workspace/session bindings required for validation;
- switching tabs would require keeping multiple mutable full `SessionState` authorities without a coherent ownership model;
- a picker action requires direct catalog/session database access;
- registration requires creating directories, accepting untrusted remote paths, or inferring project identity from a locator;
- keybinding integration would bypass the existing configurable action system;
- completing the milestone requires systematic migration of every streaming event and background task beyond the new operations introduced here;
- a remote TUI projection redesign is required before basic local navigation can work;
- project/session authority conflicts with the closed daemon contracts.

## 14. Closure evidence required

- implementation commit(s);
- picker/dialog state and action inventory;
- final keybindings and collision audit;
- active-view switch ownership diagram or field matrix;
- fake-client evidence for list/get/workspace/session/open/focus/close/registration flows;
- stale completion and rapid-switch evidence;
- proof close emits no destructive daemon operation;
- proof local registration uses daemon-issued IDs and remote raw path registration is denied;
- bounded render and narrow-terminal evidence;
- existing TUI regression evidence;
- static authority/secret guard results;
- exact verification commands and results;
- list of deferred Milestones 003–004 and Session Projections frontend-adoption work;
- closure recommendation.

## 15. Handoff notes

- Build on `ProjectTabs` and `ProjectCatalogState`; do not replace the Milestone 001 seam.
- Preserve one active heavy view. Do not solve navigation by cloning full session state into every tab.
- Use the existing `start_*` / `apply_*` async command pattern and `spawn_registered_tui_task` for every new network operation.
- Treat `ProjectGet` as the workspace-selection authority and session canonical binding as the session-selection authority.
- `Space f` is the required picker entry point. Route all other tab keys through the configurable action system after a collision audit.
- Session Projections Milestone 001 is available as a compatibility target, not as a reason to pull durable replay/frontend adoption into this pass.
- Record the actual production baseline and any changed protocol names in the closure record.
