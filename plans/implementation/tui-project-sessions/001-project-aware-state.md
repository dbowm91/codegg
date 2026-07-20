# Multi-Project TUI Milestone 001 — Project-Aware State and Catalog Client

Status: ready for handoff

Repository baseline: `1df5ef88665889307b9db636b9742febd86a2f50` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `ec42dce` — canonical daemon request context, identity-aware session bindings, and additive project/workspace protocol identity.
- `972c286` — runtime-asset refresh protocol, session lifecycle triggers, and generation/status reporting.
- `2293a11` — immutable runtime-asset generation pinning and bounded asset resources.
- `d1e5b70` — `project_catalog.v1`, bounded project DTOs, native catalog operations, request-scoped server context, and removal of `ServerState.project_dir` authority.
- `1df5ef8` — merge/closure state confirming Project Catalog 004 is closed and this milestone is the sole dependency-ready downstream handoff.

Source roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-1--project-aware-state-and-catalog-client`

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md` — project tab, project/workspace/session identity, frontend state, compatibility projection, and daemon authority.
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable closure evidence:

- `plans/closure/domain-identity/003-corrective-status.md`
- `plans/closure/domain-identity/004-status.md`
- `plans/closure/runtime-assets/003-status.md`
- `plans/closure/runtime-assets/004-status.md`
- `plans/closure/project-catalog/004-status.md`

Applicable ADRs:

- None. The canonical documents already establish the TUI as a daemon client and project tabs as frontend-local projections. Stop for an ADR if implementation requires making the TUI authoritative for project/session identity, changing native catalog semantics, or persisting heavy/durable session state in the frontend.

Primary class: infrastructure

## 1. Objective

Refactor the TUI state model so one frontend can represent the daemon-level project catalog and several independent project-tab contexts while preserving the existing single-project workflow.

Add bounded asynchronous catalog capability/list/get loading, a stable frontend-local tab container, one canonical startup tab derived from daemon identities, and active-tab accessor seams for existing components. Do not implement the `Space f` picker, visible tab navigation, persistent restoration, or the full project/session event reducer in this milestone.

The milestone succeeds when:

- `App` can hold a daemon catalog projection and several distinct project tabs;
- startup and existing session workflows operate through one canonical active tab;
- project/workspace/session/model/agent display state is scoped to the intended tab;
- catalog completions and catalog lifecycle/health events cannot mutate the wrong project after refresh, reconnect, or tab removal;
- no TUI operation infers project authority from cwd, `project_dir`, a path, or direct database access;
- Milestone 002 can add the picker and visible tab navigation without redesigning ownership.

## 2. Why this milestone is ready

All hard dependencies are closed.

Project Catalog 004 provides:

- `PROJECT_CATALOG_CAPABILITY` / `project_catalog.v1`;
- `CoreRequest::ProjectCatalogCapabilities`;
- `CoreRequest::ProjectList { include_archived, limit }`;
- `CoreRequest::ProjectGet { project_id }`;
- bounded `ProjectSummaryDto`, `ProjectDetailsDto`, `ProjectWorkspaceSummaryDto`, and health DTOs;
- matching bounded `CoreResponse` variants;
- project-scoped lifecycle and health events with stable IDs;
- request-scoped server and transport authority without `ServerState.project_dir`.

Runtime Assets 003–004 provide project/workspace refresh scope, asset generation/status reports, session-open refresh sequencing, and immutable turn pinning.

Domain Identity 003–004 provide canonical session bindings and explicit compatibility failures. No temporary path-keyed TUI catalog or frontend-local discovery is required.

## 3. Current implementation evidence

At the repository baseline:

- `CoreClient` remains a transport-neutral `request`/`subscribe` trait. In-process, socket, and stdio transports all carry the same `CoreRequest`, `CoreResponse`, and `CoreEvent` types.
- Project Catalog 004 added the native catalog request/response family, but the TUI has no typed catalog client adapter, catalog cache, or command/reducer integration.
- `src/tui/app/state/session.rs::SessionState` still owns one `session`, one `project_dir`, one Git sidebar state, one changed-file collection, and other current-session data.
- `src/tui/app/types.rs::TuiMsg` contains current session/model/agent and provider lifecycle messages but no project-catalog or tab-state messages.
- Existing TUI asynchronous operations use tracked spawn-and-complete patterns, request generations, and stale-result rejection in several domains.
- Existing renderers and handlers commonly reach one current session/project context directly.
- Current event filtering remains primarily session/global-shaped. Project Catalog 004 records that project lifecycle/health payloads carry explicit IDs, but downstream clients must route them before applying them to project state.
- There is no `ProjectTabId`, ordered tab collection, active-tab ID, catalog request state, tab-local request generation, or project-specific lightweight presentation cache.
- The current TUI can continue to render one active project during this milestone; visible multi-tab controls belong to Milestone 002.

## 4. Invariants that must not regress

- The daemon and native protocol remain the sole project, workspace, and session authority.
- A frontend-local tab ID is never serialized as a daemon `ProjectId`, `WorkspaceId`, or `SessionId`.
- Paths remain display/locator data only; the TUI never derives identity from cwd, `SessionState.project_dir`, Git roots, or file paths.
- Catalog list/get remains bounded and does not activate projects, start services, scan roots, or load session histories.
- Existing single-project startup, prompt submission, session selection, provider selection, Git sidebar refresh, modal priority, task tracking, and terminal restoration remain functional.
- Async results carry enough request/tab/project identity to reject stale or misrouted completion.
- Closing or replacing a frontend tab never deletes, archives, or cancels daemon-owned sessions/jobs unless a separate explicit action requests it.
- Heavy messages, artifacts, diffs, LSP state, and full Git state are not loaded for every catalog entry or inactive tab.
- The TUI stores no credentials, provider secrets, asset bodies, or unrestricted daemon-local paths in catalog/tab debug state.
- Catalog lifecycle/health events are applied only to matching project IDs; unscoped global application is prohibited.

## 5. Scope

### In scope

- `ProjectTabId` or equivalent stable opaque frontend-local identifier.
- A global project catalog projection and capability/request state.
- An ordered tab container and active-tab identity.
- Lightweight `ProjectTabState` keyed by stable daemon project identity.
- Selected workspace/session IDs and bounded session/model/agent/provider presentation summaries per tab.
- A single active heavy session/view payload associated with explicit tab/project/session identity, or another design that demonstrably avoids duplicating heavy state and dual authority.
- Typed TUI-side catalog request helpers over the existing generic `CoreClient` trait.
- Asynchronous `ProjectCatalogCapabilities`, `ProjectList`, and `ProjectGet` operations.
- `truncated`, empty, unsupported, archived, unavailable, and error states.
- One canonical startup tab from the current daemon session binding or explicitly resolved startup context.
- Active-tab accessors/reducer seams for incremental migration of existing handlers/renderers.
- Project-scoped application of catalog lifecycle and health events.
- Reconnect/catalog refresh invalidation and stale-result protection.
- Focused state, fake-client, transport, and regression tests.
- TUI architecture and developer-boundary documentation.

### Explicitly out of scope

- `Space f` project picker UI.
- Visible tab bar, next/previous/reorder/close keybindings, or focus semantics.
- Project-local session picker UX beyond the existing active-session compatibility surface.
- Persistent tab restoration.
- Full project/session streaming-event reducer migration.
- Presence, observer mode, project chat, ACP, raw screen replacement, or canonical replay.
- Project discovery, registration, archive/restore, activation, or health mutation implementation inside the TUI.
- Loading all session messages, Git state, diffs, artifacts, LSP state, or services for inactive tabs.
- Changing catalog protocol semantics or introducing TUI-specific authoritative protocol variants.
- A database migration.

## 6. Required design and production changes

### 6.1 State ownership map

Before moving fields, produce and commit an explicit ownership inventory covering at least:

- daemon connection, capability, and reconnect state;
- catalog cache and catalog request state;
- current project/workspace/session identifiers;
- `SessionState.project_dir` and other path-shaped fields;
- model, agent, provider connection, and selected session presentation;
- Git sidebar, changed files, indexed files, prompt state, token counters, dialogs, notifications, and task registry;
- state that must remain global;
- state that becomes tab scoped;
- heavy active-view state that is loaded only for the active tab/session;
- compatibility accessors and their planned removal milestone.

The implementation must establish one writable owner for each selected project/workspace/session/model/agent value. Do not keep global and tab-local copies that can diverge.

### 6.2 Frontend-local tab identity and container

Introduce a bounded opaque `ProjectTabId` with no conversion to daemon IDs.

Add a container equivalent to:

```text
ProjectTabs
|-- ordered: Vec<ProjectTabId>
|-- by_id: Map<ProjectTabId, ProjectTabState>
`-- active: Option<ProjectTabId>
```

`ProjectTabState` should contain only bounded project-specific state needed by this milestone, including:

- canonical `project_id`;
- selected/available workspace summaries;
- selected workspace ID;
- selected session ID and bounded session summary references;
- cached selected model, agent, and provider-connection presentation where required by existing UI;
- project summary/details and health projection;
- asset generation/status summary;
- tab-local request generations and error/loading states;
- lightweight presentation/focus placeholders needed by later navigation.

Do not use project display name, path, workspace root, or session title as a key.

Container operations must be deterministic:

- open or focus by canonical project ID according to explicit policy;
- identical display names remain distinct;
- removal selects a deterministic fallback;
- removal invalidates tab-local UI requests;
- removal does not delete daemon data;
- reordering, if internally supported, does not change tab identity.

### 6.3 Heavy session/view state migration seam

The existing `SessionState` contains heavy and path-sensitive state. Choose and document one bounded migration model:

1. move one heavy session/view state into the active `ProjectTabState` and keep inactive tabs lightweight/unloaded; or
2. retain one active heavy view cache keyed by `(ProjectTabId, ProjectId, WorkspaceId, SessionId)` while all tab selections live in `ProjectTabState`.

Either model must:

- eliminate `SessionState.project_dir` as project authority;
- validate that path-shaped Git/file operations use the selected workspace context returned by the daemon;
- prevent heavy state from being applied after the active tab/session key changes;
- avoid cloning full histories/diffs/indexes across every tab;
- provide active-tab/session accessors so existing components can migrate incrementally;
- avoid a second writable selected-session/model/agent source.

A broad renderer rewrite is not required. Compatibility accessors may expose the active tab/session to existing code, but new code must not add more direct global-current-session dependencies.

### 6.4 Catalog client adapter

Keep `CoreClient` transport-neutral. Prefer a small typed TUI-side adapter/helper over changing every transport implementation because the trait already accepts generic requests.

Support:

- `ProjectCatalogCapabilities` → `CoreResponse::ProjectCatalogCapabilities`;
- `ProjectList { include_archived: false, limit }` → bounded `CoreResponse::ProjectList { projects, truncated }`;
- `ProjectGet { project_id }` → `CoreResponse::ProjectGet { project }`.

Requirements:

- create request envelopes with unique request IDs and the negotiated protocol version;
- validate the expected response variant and convert protocol errors to bounded TUI diagnostics;
- preserve the server's `truncated` indication;
- never list with an unbounded/zero limit that relies on accidental server defaults;
- avoid activation, health mutation, discovery scans, or session-history loads during list;
- avoid storing canonical workspace roots unless the active local workflow requires a bounded locator; IDs remain authority.

### 6.5 Global catalog state

Add a global state equivalent to:

```text
ProjectCatalogState
|-- capability: Unknown | Supported(bounds) | Unsupported
|-- request_generation
|-- loading/error/last_loaded
|-- ordered summaries
|-- details cache keyed by ProjectId
|-- truncated
`-- reconnect_epoch
```

The cache is a frontend projection only. Every reconnect or capability change invalidates data according to explicit policy.

Catalog list and detail operations must use existing tracked TUI task infrastructure. Completion payloads carry:

- request generation;
- reconnect epoch;
- requested project ID for detail requests;
- expected active/request state;
- bounded response data or error.

Stale completions are dropped without mutating current state.

### 6.6 Startup compatibility tab

Preserve current startup without creating a path-keyed pseudo-project.

Resolution order:

1. if the current/loaded session has a canonical `SessionBindingDto`, create/focus the initial tab from its project/workspace IDs;
2. otherwise consume an explicitly resolved canonical startup context already returned by the daemon/server path;
3. if no canonical context exists, load the catalog and show a bounded no-active-project state rather than choosing the first project or interpreting cwd as identity;
4. an older daemon lacking `project_catalog.v1` enters explicit single-project compatibility mode only when an existing canonical session/workspace context is available; otherwise report unsupported capability actionably.

Do not choose the first catalog row when multiple projects exist. Do not create a project or workspace merely to make startup succeed.

The initial tab may still be the only rendered tab in this milestone.

### 6.7 Catalog command/reducer integration

Add bounded start/completion messages or commands for:

- capability load;
- catalog list/refresh;
- project detail load;
- startup tab establishment;
- catalog lifecycle/health event application.

Large DTO payloads should be boxed or stored behind compact state transitions if adding them directly would worsen existing large-enum pressure.

Reducers must check generation/epoch and project identity before mutation. A detail response for project A must never populate project B's tab after switching or reuse.

### 6.8 Project-scoped catalog events

Consume only the project catalog lifecycle and health events needed to keep summaries coherent.

Requirements:

- inspect stable project/workspace IDs in the event payload before applying;
- update the matching catalog summary/detail and matching open tabs only;
- archive/removal-like lifecycle changes mark tabs stale/actionable rather than silently switching them;
- health events update only the matching project/workspace projection;
- unknown/unopened project events may update the bounded global catalog cache if present but must not create a tab automatically;
- event sequence/reconnect gaps trigger catalog revalidation rather than speculative local reconciliation.

Full turn/tool/session event routing remains Milestone 003. Do not broaden this pass into the Phase 5 projection reducer.

### 6.9 Active-tab compatibility accessors

Provide explicit helpers for existing code, such as semantic equivalents of:

- `active_project_tab()` / mutable variant;
- `active_project_id()`;
- `active_workspace_id()`;
- `active_session_id()`;
- `active_session_view_key()`;
- `with_active_session_view(...)`.

Accessors must fail or return `None` when no canonical active context exists. They must not fall back to `project_dir`, cwd, the first catalog row, or the most recently loaded session.

Document which existing global fields are deprecated and forbid new call sites through a focused static/review guard where practical.

### 6.10 Reconnect, failure, and cancellation

Define and implement:

- catalog load failure leaves an already validated active compatibility tab usable but stale-marked;
- capability unsupported state is explicit and does not synthesize catalog entries;
- reconnect increments an epoch, invalidates in-flight completions, and revalidates daemon summaries;
- tab removal invalidates local work but does not cancel daemon jobs;
- project archived/not-found invalidates affected tabs and selected session presentation without choosing another project;
- truncated catalog results remain visible as truncated and do not imply complete absence;
- cancellation/drop of a TUI task leaves the prior valid catalog projection active;
- several TUI clients maintain independent frontend tab state while consuming the same daemon catalog.

## 7. Ordered work packages

### Work package A — Inventory and state skeleton

Intent: establish ownership before migration.

Required changes:

- commit the old-to-new field ownership map;
- define `ProjectTabId`, `ProjectTabState`, `ProjectTabs`, and `ProjectCatalogState`;
- add bounded constructors and deterministic container operations;
- add active-tab accessors;
- add no-op/single-tab compatibility construction.

Acceptance evidence:

- container tests cover multiple projects with identical names/titles;
- tab IDs are not daemon IDs;
- removal/fallback behavior is deterministic;
- no duplicate writable selected-project/session state is introduced.

### Work package B — Native catalog client and async state

Intent: consume the closed project catalog protocol without modifying authority.

Required changes:

- implement typed request helpers over `CoreClient`;
- negotiate `project_catalog.v1`;
- implement bounded capability/list/get async commands;
- preserve `truncated`, unsupported, empty, loading, and error states;
- attach generation and reconnect epoch to completions.

Acceptance evidence:

- fake-client tests cover expected responses, protocol error variants, wrong response variants, unsupported capability, truncation, and stale completion;
- socket/inproc/stdio clients require no divergent behavior;
- list/get performs no activation or direct storage access.

### Work package C — Canonical startup tab and heavy-state seam

Intent: preserve the current workflow through canonical tab ownership.

Required changes:

- establish initial tab from session binding or explicit canonical context;
- associate active heavy session/view state with an explicit tab/project/workspace/session key;
- remove `project_dir` as TUI project authority;
- migrate selected session/model/agent/provider presentation to one tab-owned source;
- route existing render/command paths through active-tab compatibility accessors where needed.

Acceptance evidence:

- existing startup/session tests pass through one canonical tab;
- missing/ambiguous context does not use cwd or choose the first project;
- switching active tab in state tests changes the active accessor/view key cleanly;
- heavy state for one tab cannot apply to another.

### Work package D — Catalog lifecycle/health routing

Intent: close the downstream event-boundary finding from Project Catalog 004 without implementing full session projections.

Required changes:

- add project-scoped lifecycle/health event handling;
- check payload project/workspace IDs before mutation;
- stale/archive/not-found handling for affected open tabs;
- reconnect/gap revalidation path.

Acceptance evidence:

- events for project A do not mutate project B;
- unopened project events do not auto-open tabs;
- archived project state remains actionable and does not silently switch;
- event gap/reconnect invalidates and reloads summaries.

### Work package E — Guards, documentation, and regression closure

Intent: ensure the state foundation is safe for the visible-navigation milestone.

Required changes:

- update TUI architecture and field ownership documentation;
- add a focused guard against new TUI cwd/path project authority and direct project storage access;
- add debug/stats summaries for catalog state, open tab count, active tab identity, and stale completion counters without paths/secrets;
- run focused and broad regressions.

Acceptance evidence:

- guards pass with negative fixtures;
- existing modal/task/terminal/session/provider/Git sidebar tests remain green;
- documentation names deferred picker/navigation/event/restoration work.

## 8. Required tests

### State and reducer unit tests

- `ProjectTabId` opacity and non-conversion to daemon identity.
- open/focus/remove/fallback ordering.
- identical project display names and identical session titles across projects.
- selected workspace/session/model/agent/provider projection remains tab scoped.
- active-tab accessors return no fallback when state is absent.
- heavy session/view key rejects another tab/project/session.
- archived/stale/error states.

### Catalog client tests

- capability supported with advertised bounds.
- capability unsupported.
- bounded list success and `truncated = true`.
- empty catalog.
- project get success/not-found/error/wrong response variant.
- stale generation and stale reconnect epoch rejection.
- cancellation preserves prior valid cache.

### Startup and compatibility tests

- canonical session binding creates the initial tab.
- explicit canonical startup context creates the initial tab.
- missing or ambiguous context does not use cwd/path fallback.
- multiple catalog projects do not cause first-row implicit selection.
- older daemon compatibility mode is explicit and bounded.
- existing single-project startup/session creation/attach/load remains functional.

### Event-routing tests

- project A lifecycle/health event updates only project A cache/tab.
- unopened project event does not create a tab.
- archive marks the matching tab stale/actionable.
- reconnect/event gap triggers revalidation.
- several clients maintain independent tab state.

### Regression and negative tests

- current TUI async request-generation tests.
- provider connection selection/lifecycle dialogs.
- Git sidebar generation and workspace scope.
- modal priority and terminal cleanup.
- no TUI direct database/project-catalog store calls.
- no cwd/`project_dir` project authority.
- no credential or asset-body material in catalog/tab debug serialization.

## 9. Required verification commands

Use repository-standard wrappers/caps where present and record exact results.

```bash
rtk cargo fmt -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test -p codegg-protocol
rtk cargo test -p codegg --lib tui -- --test-threads=1
rtk cargo test --test core_transport -- --test-threads=1
rtk cargo test --test single_daemon -- --test-threads=1
rtk env CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_identity_path_usage.py
rtk git diff --check
```

Add and run focused targets for project-tab state, catalog fake-client behavior, canonical startup, and project-scoped catalog event routing.

If repository-wide tests encounter an environmental restriction, separate it from changed-scope failures and provide focused passing evidence; do not claim a broad pass that did not complete.

## 10. Documentation updates

Update at least:

- `architecture/tui.md` — state hierarchy, active heavy-view ownership, catalog async flow, startup behavior, and event routing boundary;
- `architecture/protocol.md` or `architecture/client.md` — TUI consumption of `project_catalog.v1` where useful;
- developer guidance for active-tab accessors and prohibited new global-current-session dependencies;
- compatibility diagnostics for older daemons and missing canonical startup context;
- the implementation plan status and closure record.

## 11. Acceptance criteria

1. `App` can represent a bounded daemon catalog and at least two independent project-tab states.
2. Existing startup and single-project behavior execute through one canonical active tab.
3. Catalog capabilities/list/get are daemon-backed, asynchronous, bounded, transport-neutral, and stale-safe.
4. `ProjectSummaryDto`/`ProjectDetailsDto` identity is retained as stable IDs; paths are never tab keys or authority.
5. Project/workspace/session/model/agent/provider presentation state has one tab-scoped writable owner.
6. Heavy session/view state is explicitly keyed and cannot leak across tabs.
7. Catalog lifecycle/health events mutate only matching project/workspace state.
8. Reconnect, truncation, archive, unsupported capability, empty catalog, and errors are represented actionably.
9. No cwd mutation, direct database access, frontend discovery, or path-derived pseudo-project is introduced.
10. Existing session, provider, Git sidebar, modal, task-lifecycle, and terminal cleanup behavior remains compatible.
11. The state/accessor/client seams are sufficient for Milestone 002 to add picker/tab navigation without another ownership redesign.
12. Closure evidence records exact implementation commits, tests, guards, deferred work, and any remaining global-state migration debt.

## 12. Stop conditions

Stop and report rather than improvising when:

- current `main` no longer exposes the documented `project_catalog.v1` operations or asset-generation contracts;
- migration requires retaining two writable selected project/session/model/agent authorities;
- completing the milestone requires TUI-local discovery, direct database access, or path-derived project identity;
- a renderer-wide rewrite is required instead of an accessor-based migration seam;
- the chosen heavy-state model would keep unbounded histories/artifacts for every inactive tab;
- project catalog event routing requires redesigning the canonical Phase 5 session projection/replay model;
- compatibility with the existing single-project workflow cannot be preserved within one coherent pass;
- implementation requires a wire-breaking protocol change rather than additive client consumption.

## 13. Closure evidence required

- implementation commit(s) and actual reviewed baseline;
- old-to-new field ownership map;
- state/container and heavy-view ownership tests;
- catalog capability/list/get fake-client and stale-completion evidence;
- canonical startup and older-daemon compatibility evidence;
- project-scoped catalog lifecycle/health event-routing evidence;
- proof that no cwd/path/direct-storage project authority was introduced;
- focused and broad verification commands/results;
- inventory of deferred picker/navigation/full-event-routing/restoration work;
- closure recommendation and next dependency decision.

## 14. Handoff notes

- This is the sole dependency-ready plan at the repository baseline.
- Do not create the project picker or visible tab navigation in this pass.
- Reuse the generic `CoreClient::request` boundary; do not fork transport-specific catalog implementations.
- Treat `SessionState.project_dir` as a locator compatibility concern, not identity.
- Use active-tab accessors as a controlled migration seam, not as a permanent disguise for global state.
- Preserve project-catalog list/get probe-free behavior and runtime-asset refresh ownership.
- Inspect current `main` before implementation and update closure evidence if production code advanced after the planning baseline.
