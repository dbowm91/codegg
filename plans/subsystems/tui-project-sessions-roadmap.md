# Multi-Project TUI and Session Management Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md` — project tab, session, workspace, frontend state
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs:

- None required initially. The canonical documents already select the TUI as the reference frontend and define Helix-style project navigation.

## 1. Purpose and ownership boundary

This subsystem owns the TUI's global daemon connection state, project catalog projection, open project tabs, per-project session selection, tab restoration, focus/navigation behavior, inactive-tab lifecycle, and the UI integration needed to refresh project assets before turn execution.

It consumes project-catalog protocol operations, session APIs, workspace-service health, asset generations, jobs, provider/model selection, and existing TUI async command infrastructure. It must not own project discovery, session storage authority, daemon scheduling, ACP behavior, team presence, or frontend-neutral session projection persistence.

## 2. Work classification

### Invariants

- The TUI is a client projection of daemon state, not an authority tied to process cwd.
- Project, workspace, and session state remain distinct.
- Switching tabs never mutates process cwd or leaks one project's model/agent/session state into another.
- Closing a tab does not delete or archive its durable sessions.
- Inactive tabs do not retain unnecessary exclusive service leases or unbounded task/state memory.
- Async completions are generation/request scoped and cannot mutate the wrong tab after switching.

### Capabilities

- `Space f` opens a project picker.
- Projects open as tabs and support next/previous/close/restoration behavior.
- Each project has several sessions and independent selected session/model/agent state.
- Several TUIs can use different projects through one daemon.
- Project tabs show bounded health, active-session/job counts, and asset generation.

### Infrastructure

- Global/project/tab/session TUI state separation.
- Project catalog async commands and cache.
- Project-aware event routing and request generations.
- Persisted lightweight tab restoration.
- Inactive-tab lease/task lifecycle.

### Polish

- Keyboard discoverability and configurable bindings.
- Tab labels, badges, empty states, and diagnostics.
- Large project/session list performance.

## 3. Non-goals

- Implementing project discovery or catalog storage.
- Team collaborator presence or observer mode.
- Raw screen mirroring to remote clients.
- Redesigning all existing TUI components at once.
- Keeping full message histories for every inactive tab resident in memory.
- Giving the TUI direct database or workspace-service ownership.

## 4. Current state

The TUI already routes most high-latency session, history, task, memory, and worktree operations through `CoreClient`, with daemon-client mode as the default. This is the correct architectural base. Existing async command handlers use spawn-and-complete patterns, `AsyncUiRequestState`, stale-generation protection, and a tracked background-task registry.

Current application state is organized into several domains but remains centered on one selected session/project context. Git sidebar state, dialogs, and event filtering are keyed primarily to the current session. Remote TUI snapshots represent one active view. There is no global project catalog state, project tab model, per-tab async generation, or restoration of multiple open projects.

The project catalog and asset service are therefore hard prerequisites; this roadmap should not reimplement them inside the TUI.

## 5. Target architecture

Add a global frontend state model resembling:

```text
App
|-- daemon connection and negotiated capabilities
|-- authenticated/local principal placeholder
|-- project catalog cache and request state
|-- ordered open ProjectTabState values
|-- active tab ID
|-- global dialogs, notifications, jobs, providers
`-- task lifecycle registry

ProjectTabState
|-- ProjectId and selected WorkspaceId
|-- selected SessionId
|-- bounded session summaries
|-- project health and asset generation
|-- selected model/agent/provider connection
|-- per-tab async request generations
|-- per-tab view/focus state
`-- lightweight cached presentation state
```

Heavy messages, artifacts, diff bodies, and services remain daemon-side or load on demand. Events route by project/session identity before touching tab state. Tab IDs remain stable across reorder and restoration.

`Space f` opens a searchable project picker backed by the catalog. Opening a project acquires only the minimal activation/session data needed. Session open/attach triggers asset refresh and waits for a valid generation before creating the next turn runtime.

## 6. Dependency graph

```text
Milestone 1: project-aware TUI state and catalog client
        |
        v
Milestone 2: picker, tabs, and project/session navigation
        |
        v
Milestone 3: project-correct event routing and async lifecycle
        |
        v
Milestone 4: restoration, inactive resource bounds, badges, and closure
```

- Milestone 1 has hard dependencies on Project Catalog Milestone 4 and Runtime Assets Milestone 3 interfaces.
- Milestone 2 has a hard dependency on Milestone 1.
- Milestone 3 has a hard dependency on Milestones 1–2.
- Milestone 4 has a hard dependency on Milestones 2–3.

## 7. Milestones

### Milestone 1 — Project-aware state and catalog client

Class: infrastructure

Objective: separate global daemon state, project-tab state, and session state while adding asynchronous catalog loading.

Dependencies: hard on catalog protocol and asset-generation summaries.

Deliverable boundary: typed tab IDs/state, catalog cache, project list/get commands, per-tab request states, migration of current single-project state into one initial tab, and compatibility startup behavior.

User or operator value: the TUI can represent more than one project without yet exposing full navigation.

Exit conditions:

- current startup produces one project tab using daemon identities;
- no new TUI state uses cwd as project authority;
- catalog loads asynchronously with stale-result protection;
- identical session titles across projects remain distinguishable;
- existing single-project workflows remain functional.

Deferred work: picker and tab keybindings.

### Milestone 2 — Project picker and tab navigation

Class: capability

Objective: implement `Space f`, open project tabs, switching, ordering, closure, and per-project session selection.

Dependencies: hard on Milestone 1.

Deliverable boundary: picker dialog, fuzzy/filter navigation, keybindings, tab bar, next/previous/close actions, project-local session picker, and explicit one-off registration path integration.

Exit conditions:

- users can open and navigate several projects in one process;
- closing a tab preserves durable sessions;
- selecting a tab restores its selected session/model/agent presentation;
- switching causes no cwd mutation;
- keyboard/focus semantics are covered by regression tests.

Deferred work: persistent restoration and advanced badges.

### Milestone 3 — Event routing and lifecycle correctness

Class: invariant

Objective: make asynchronous completions, streaming events, Git status, dialogs, and session mutations project/tab correct under rapid switching and multiple TUIs.

Dependencies: hard on Milestones 1–2.

Deliverable boundary: project/session-routed reducers, per-tab request generations, stale completion rejection, tab-aware task cancellation, session attach/asset refresh sequencing, and multi-client integration tests.

Exit conditions:

- events update only the intended tab/session;
- rapidly switching or closing tabs cannot apply stale completions elsewhere;
- session-open refresh completes or fails actionably before the next turn;
- several TUIs operating different projects remain isolated;
- no tab owns daemon resources directly.

Deferred work: canonical cross-frontend session projection.

### Milestone 4 — Restoration, bounds, badges, and closure

Class: capability

Objective: complete practical multi-project UX and resource correctness.

Dependencies: hard on Milestones 2–3.

Deliverable boundary: restoration of open tabs/active tab, inactive-tab memory/lease policy, health/job/session/asset badges, empty/error states, performance tests, documentation, and closure evidence.

Exit conditions:

- restart restores valid tabs without eagerly activating every project;
- stale/archived/unavailable projects restore with actionable state;
- inactive tabs remain within bounded memory and service leases;
- all Phase 4 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Persist only lightweight frontend preferences/tab identifiers in user configuration or TUI state storage. Durable project/session data remains daemon-owned. Invalid restored IDs are tolerated and diagnosed.

### Protocol and compatibility

Consume native project/session APIs and capability negotiation. Do not create TUI-only authoritative project behavior. Older daemons should produce a clear unsupported-capability path or a bounded single-project compatibility mode.

### Security and authorization

Future authorization must be able to filter catalog/session lists; avoid caching unauthorized data beyond invalidation. TUI actions never bypass daemon checks.

### Concurrency, cancellation, and recovery

Every asynchronous operation carries tab/project/session identity plus request generation. Closing a tab invalidates relevant UI completions but does not cancel daemon jobs unless explicitly requested. Reconnect rehydrates summaries.

### Observability and audit

Expose active tab count, catalog generation, per-tab task/request state, stale completion counters, and restoration diagnostics through TUI stats where useful.

### Performance and resource use

Use bounded summaries and lazy details. Do not load all session messages, Git status, diffs, LSP state, or artifacts for inactive tabs. Limit picker result rendering and background probes.

### Documentation and operations

Update TUI architecture, keybindings, commands, remote TUI compatibility, session behavior, and troubleshooting docs.

## 9. Verification strategy

Use reducer/state unit tests, input/focus regression tests, fake `CoreClient` multi-project fixtures, rapid-switch stale completion tests, several-client daemon integration tests, tab restoration fixtures, archived/unavailable project behavior, and memory/lease bound assertions.

## 10. Risks and decision points

- Existing components may read global `App` session fields directly. Migrate through accessor seams instead of a broad unsafe rewrite.
- Remote TUI snapshot shape may lag behind local state. Phase 5 owns canonical projection; Phase 4 should keep compatibility without embedding raw render frames.
- Persistent tab state can accidentally become authoritative. Store only locators/IDs and always revalidate with the daemon.
- If tab-local model/agent state conflicts with session-owned state, session/daemon authority wins.

## 11. Completion definition

This roadmap closes when one TUI can use several project tabs and several sessions per project through one daemon, with project-correct async/event behavior, no cwd authority, bounded inactive state, safe restoration, and no regression of the existing single-project workflow.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | blocked | `plans/implementation/tui-project-sessions/001-project-aware-state.md` | — | Runtime asset refresh and project catalog protocol |
| 2 | not started | — | — | Milestone 1 closure |
| 3 | not started | — | — | Milestones 1–2 closure |
| 4 | not started | — | — | Milestones 2–3 closure |
