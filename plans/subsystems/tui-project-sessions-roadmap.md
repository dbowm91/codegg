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

Milestone 1 added a bounded project catalog cache, stable frontend-local project tab IDs, an ordered tab collection, one compatibility startup tab, and active-tab accessors while preserving the existing single-project render path.

Milestone 2 closed at `f569386`. The TUI now has a bounded project picker, explicit workspace selection, local-only one-off registration, configurable next/previous/close actions, a visible bounded tab strip, project-local session summaries, and a `ViewSwitchCoordinator` with an active-view epoch. Existing single-project compatibility remains intact.

The remaining correctness boundary is that heavy active-session state and many command/event reducers are still global/current-session shaped. Milestone 3 must make all asynchronous completions, live events, dialogs, tasks, subscriptions, and resource lifecycles project/session correct. Milestone 4 then owns safe persistent restoration, long-running bounds, legacy frontend-authority cleanup, and subsystem closure.

The project catalog, runtime assets, session storage, and execution services remain daemon-owned prerequisites; this roadmap must not reimplement them inside the TUI.

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

Heavy messages, artifacts, diff bodies, and services remain daemon-side or load on demand. Events route by project/session identity before touching tab state. Tab IDs remain stable across reorder during one process; persistent restoration revalidates canonical daemon IDs and does not treat frontend tab IDs as durable authority.

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

- Milestone 1 had hard dependencies on Project Catalog Milestone 4 and Runtime Assets Milestone 3 interfaces; both are closed.
- Milestone 2 had a hard dependency on Milestone 1 and is closed.
- Milestone 3 has a hard dependency on Milestones 1–2 and is ready for handoff.
- Milestone 4 has a hard dependency on Milestone 3 and is authored but blocked.

## 7. Milestones

### Milestone 1 — Project-aware state and catalog client

Status: closed; see `plans/closure/tui-project-sessions/001-status.md`.

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

### Milestone 2 — Project picker and tab navigation

Status: closed at `f569386`; see `plans/closure/tui-project-sessions/002-status.md`.

Class: capability

Objective: implement `Space f`, open project tabs, switching, ordering, closure, and per-project session selection.

Dependencies: hard on Milestone 1.

Deliverable boundary: picker dialog, bounded filtering, keybindings, tab bar, next/previous/close actions, project-local session selection, explicit workspace selection, and local-safe one-off registration.

Exit conditions:

- users can open and navigate several projects in one process;
- closing a tab preserves durable sessions;
- selecting a tab restores its selected session/model/agent presentation;
- switching causes no cwd mutation;
- keyboard/focus semantics are covered by regression tests.

### Milestone 3 — Project-correct event routing and lifecycle

Status: ready for handoff.

Implementation plan: `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md`.

Class: invariant / correctness

Objective: make asynchronous completions, streaming events, Git status, dialogs, session mutations, tasks, subscriptions, and resource cleanup project/tab correct under rapid switching and multiple TUIs.

Dependencies: hard on Milestones 1–2; both are closed.

Deliverable boundary:

- typed route tokens containing tab/project/workspace/session identity, active-view epoch, reconnect epoch, and request generation;
- a central pure event-routing classifier;
- canonical session-to-tab indexing;
- explicit active heavy-view load/commit/suspend transitions;
- bounded inactive-tab activity summaries;
- ownership-safe permission/question foregrounding;
- task/subscription/lease ownership and cleanup;
- refresh/resync behavior for stale, rebound, archived, ambiguous, or unknown scope;
- multi-client and rapid-switch race tests.

Exit conditions:

- every project/session-scoped completion validates explicit identity and epoch before mutation;
- events update only the intended tab/session;
- rapidly switching or closing tabs cannot apply stale completions elsewhere;
- inactive activity remains bounded and does not materialize full histories;
- pending permissions/questions never steal focus across sessions;
- closing/inactivating a tab releases frontend resources but does not cancel daemon execution;
- several TUIs operating different projects remain isolated;
- the routing boundary can later consume canonical projection events without a second tab model.

Deferred work: persistence/restoration and canonical projection-primary frontend adoption.

### Milestone 4 — Persistent restoration, resource bounds, and closure

Status: blocked on strict Milestone 3 closure.

Implementation plan: `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`.

Class: capability / polish / closure

Objective: complete practical multi-project UX, safe restoration, long-running resource correctness, legacy frontend-authority cleanup, and roadmap closure.

Dependencies: hard on Milestone 3.

Deliverable boundary:

- a versioned bounded frontend manifest containing safe canonical ID intent only;
- atomic, permission-safe, symlink-safe, debounced local persistence;
- daemon-authoritative restore validation;
- lazy reconstruction of lightweight tabs and exactly one heavy active view;
- explicit recovery for missing, archived, rebound, unsupported, corrupt, and disconnected cases;
- inactive-tab memory/task/subscription/lease caps and soak tests;
- removal or narrowing of obsolete path/current-focus/single-project frontend authority;
- documentation and closure evidence.

Exit conditions:

- restart restores valid tabs without eagerly activating every project;
- stale/archived/unavailable projects restore with actionable state;
- invalid or corrupt persisted state cannot prevent safe startup;
- no secrets, histories, outputs, file bodies, logs, subscriptions, or leases are persisted;
- inactive tabs remain within bounded memory and service leases;
- long-running switching/open/close/reconnect behavior remains within documented caps;
- all Phase 4 exit criteria are evidenced.

## 8. Cross-cutting requirements

### Storage and migration

Persist only lightweight frontend intent in user application state. Durable project/session data remains daemon-owned. Invalid restored IDs are tolerated, diagnosed, and revalidated; paths are not identity.

### Protocol and compatibility

Consume native project/session APIs and capability negotiation. Do not create TUI-only authoritative project behavior. Older daemons produce a clear unsupported-capability path or a bounded single-project compatibility mode.

### Security and authorization

Future authorization must be able to filter catalog/session lists; avoid caching unauthorized data beyond invalidation. TUI actions never bypass daemon checks. Persistent manifests contain no secrets or content bodies.

### Concurrency, cancellation, and recovery

Every asynchronous operation carries tab/project/session identity plus request generation and view/reconnect epochs. Closing a tab invalidates frontend completions and releases frontend resources but does not cancel daemon jobs unless explicitly requested. Reconnect rehydrates summaries through daemon authority.

### Observability and audit

Expose active tab count, catalog generation, per-tab task/request state, stale completion counters, routing/resync diagnostics, restoration outcomes, and bounded resource counts where useful.

### Performance and resource use

Use bounded summaries and lazy details. Do not load all session messages, Git status, diffs, LSP state, or artifacts for inactive tabs. Limit picker rendering, restore concurrency, background probes, task/subscription counts, and manifest write frequency.

### Documentation and operations

Update TUI architecture, keybindings, commands, remote compatibility, session behavior, persistence security, resource caps, and troubleshooting docs.

## 9. Verification strategy

Use reducer/state unit tests, input/focus regression tests, fake `CoreClient` multi-project fixtures, rapid-switch stale-completion tests, several-client daemon integration tests, tab restoration fixtures, archived/unavailable/rebound behavior, corruption/security tests, and memory/task/subscription/lease bound assertions.

## 10. Risks and decision points

- Existing components may read global `App` session fields directly. Migrate through route-token/accessor seams instead of a broad unsafe rewrite.
- Remote TUI snapshot shape may lag behind local state. Session Projections Milestone 4 owns canonical projection adoption; this roadmap must keep a reusable routing boundary without embedding raw render frames.
- Persistent tab state can accidentally become authoritative. Persist canonical IDs as intent and always revalidate with the daemon.
- If tab-local model/agent state conflicts with session-owned state, session/daemon authority wins.
- Closing a frontend tab must not be conflated with cancelling daemon-owned execution.

## 11. Completion definition

This roadmap closes when one TUI can use several project tabs and several sessions per project through one daemon, with project-correct async/event behavior, no cwd authority, bounded inactive state, safe restoration, and no regression of the existing single-project workflow.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/tui-project-sessions/001-project-aware-state.md` | `plans/closure/tui-project-sessions/001-status.md` | — |
| 2 | closed | `plans/implementation/tui-project-sessions/002-project-picker-tab-navigation.md` | `plans/closure/tui-project-sessions/002-status.md` | —; closed at `f569386` |
| 3 | ready | `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md` | — | Milestones 1–2 closed |
| 4 | blocked | `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md` | — | Strict Milestone 3 closure |