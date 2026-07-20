# Multi-Project TUI Milestone 001 — Project-Aware State Foundation

Status: ready

Repository baseline: `fbae374a2cd6172505204b1bc1bee1ef247afd5f` (production-code baseline; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-1--project-aware-state-and-catalog-client`

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- None. Stop if implementation requires making the TUI authoritative for project/session identity or replacing the native protocol boundary.

Primary class: infrastructure

## 1. Objective

Refactor the TUI state model so it can represent a daemon-level project catalog and several independent project-tab/session contexts, while preserving current single-project behavior. Add asynchronous catalog loading and one compatibility initial tab, but do not yet implement the `Space f` picker or full tab navigation.

## 2. Why this milestone is ready

Hard dependencies:

- Runtime Assets refresh interfaces expose project/workspace asset generation and session-open refresh sequencing through the closed Runtime Assets milestone.
- Project Catalog protocol/server migration is closed at `d1e5b70` and exposes stable project list/get and project/workspace summaries.

The agent must not create TUI-local project discovery, direct database access, or path-keyed pseudo-projects to bypass these dependencies.

## 3. Current implementation evidence

- The TUI defaults to `DaemonClient` and routes most session/history/task/memory/worktree operations through `CoreClient`.
- `architecture/tui.md` documents spawn-and-complete async commands, `AsyncUiRequestState`, stale completion protection, `TuiTaskRegistry`, project-sensitive Git sidebar refresh, and current remote snapshot behavior.
- Application state is split into several domains but retains one principal selected session/project context.
- Existing handlers and render components frequently access current session state directly.
- Remote and bus events filter mainly by current session rather than project-tab identity.
- There is no global catalog cache, `ProjectTabId`, ordered open-tab model, per-tab request generations, or tab restoration state.

## 4. Invariants that must not regress

- The TUI remains a daemon client and never becomes storage authority.
- No project authority is inferred from TUI process cwd.
- Existing single-project startup and session workflows remain functional.
- Async completions cannot mutate the wrong project/tab/session.
- Closing or replacing a UI view does not delete durable sessions.
- Heavy details are not loaded for every catalog entry or inactive tab.
- Existing task lifecycle, stale-generation, modal priority, and terminal restoration behavior remain correct.

## 5. Scope

### In scope

- `ProjectTabId` or equivalent stable frontend-local tab identity.
- Separation of:
  - global daemon/connection/capability state;
  - project catalog cache/request state;
  - ordered open project-tab state;
  - active tab selection;
  - per-project selected workspace/session/model/agent/presentation summaries.
- Async project list/get commands through `CoreClient`.
- Per-tab request-generation/state scaffolding.
- Compatibility migration of current startup into one initial project tab.
- Accessor/reducer seams allowing existing components to operate on the active tab without immediate broad rewrite.
- Catalog loading/error/empty state suitable for future picker.
- Focused tests and architecture documentation.

### Explicitly out of scope

- `Space f` picker UI.
- Tab bar rendering and next/previous/close keybindings.
- Persistent tab restoration.
- Presence, observer mode, chat, or ACP.
- Project discovery or registration implementation.
- Canonical session replay/projection.
- Loading all session messages, Git state, or services for inactive projects.

## 6. Required production changes

### Core/domain

No core domain authority changes are expected. Consume typed project/workspace/session IDs from protocol DTOs. Frontend-local tab IDs must never be reused as daemon project IDs.

### Storage and migrations

No database migration. Lightweight TUI preference persistence is deferred. Existing session storage remains daemon-owned.

### Protocol and DTOs

Consume negotiated project catalog/list/get capabilities and bounded summaries. Add no TUI-specific authoritative protocol variants. If existing `CoreClient` abstractions lack catalog methods, extend the native request/response client interface consistently across socket/inproc/stdio transports.

### Runtime and concurrency

Refactor `App`/state modules to contain:

- catalog request/cache state;
- ordered tab collection keyed by stable tab IDs;
- active tab ID;
- `ProjectTabState` with project/workspace/session summaries and request states;
- helper methods for active tab/session access;
- event/command identity envelopes carrying project/tab/session where needed.

Use existing async command patterns and `spawn_registered_tui_task`. Catalog completions carry request IDs and are rejected when stale. No renderer performs network or filesystem operations.

### Frontend or operator surface

Provide the state and basic catalog load/refresh behavior, including actionable unsupported/error/empty diagnostics. Existing UI may still render only the active compatibility tab.

### Security and authorization

Display only catalog/session entries returned by the daemon. Do not cache or synthesize hidden projects. Treat stale authorization/not-found responses as invalidation signals. Never place credentials in tab state.

### Documentation and static guards

Update TUI architecture with the new state hierarchy and authority boundaries. Add review guidance against direct `App` global session fields for new code and against TUI cwd project inference.

## 7. Ordered work packages

### Work package A — State inventory and migration design

Intent: identify current global/current-session fields and create a safe compatibility migration path.

Required changes:

- inventory App/state fields by global, project, session, presentation, and modal ownership;
- define `ProjectTabState` and active-tab accessors;
- identify components that can continue through accessors versus requiring immediate identity propagation;
- add no-op/single-tab compatibility construction.

Acceptance evidence:

- documented field ownership map;
- existing startup/session tests compile through one active tab;
- no duplicate durable state is introduced.

### Work package B — Project catalog client state

Intent: load bounded project summaries asynchronously from the daemon.

Required changes:

- add catalog cache and `AsyncUiRequestState` or equivalent generation;
- add start/completion command variants and handlers;
- extend `CoreClient` transport implementations if needed;
- handle unsupported capability, empty catalog, errors, and refresh;
- avoid activation or detailed session loading during list.

Acceptance evidence:

- fake client tests for success/error/stale completion;
- event loop remains responsive;
- catalog data remains bounded summaries.

### Work package C — Ordered tab container and active compatibility tab

Intent: represent several project contexts without exposing navigation yet.

Required changes:

- ordered tab storage and stable active tab ID;
- create/update/remove internal tab operations;
- initialize from current selected project/session through daemon IDs;
- per-tab selected workspace/session/model/agent summaries;
- tab-local request state and lightweight presentation fields;
- active-tab accessors used by existing session/render paths where practical.

Acceptance evidence:

- unit tests create several tabs with identical session titles without collision;
- switching active ID in tests changes accessors cleanly;
- removal selects deterministic fallback and never deletes daemon sessions.

### Work package D — Identity-correct async seams and docs

Intent: prevent future picker/navigation work from inheriting global stale-completion bugs.

Required changes:

- attach project/tab/session identity to relevant new async completions;
- establish reducer/helper pattern that drops completions for removed/rebound tabs;
- update architecture docs and TUI stats/debug summaries;
- add compatibility notes for old daemon capability behavior.

Acceptance evidence:

- stale catalog/tab completion tests;
- current session async tests remain green;
- no cwd mutation or direct project DB access.

## 8. Failure, cancellation, restart, and contention semantics

- Catalog load failure leaves the current compatibility tab usable and records an actionable error.
- Unsupported project-catalog capability enters explicit single-project compatibility mode; it does not invent a catalog.
- Repeated catalog refresh invalidates older completions through request generations.
- Removing a tab invalidates tab-local UI requests but does not cancel daemon jobs or delete sessions.
- Reconnect clears/revalidates daemon-derived summaries and preserves only frontend-local tab intent where safe.
- Several TUI clients remain independent; no frontend tab lock exists in the daemon.

## 9. Compatibility and migration

- Current CLI/startup path opens one active tab associated with the daemon-resolved current session/workspace/project.
- Existing commands/renderers may temporarily use active-tab compatibility accessors.
- Do not retain two writable sources of selected session/model/agent state; establish one authoritative tab/session projection and deprecate old fields deliberately.
- Older daemons receive explicit compatibility behavior.
- No persistent tab-state format is introduced yet.

## 10. Required tests

### Focused unit tests

- tab ID/container operations;
- active-tab fallback/removal;
- identical session titles across projects;
- per-tab selected model/agent/session state;
- catalog cache/request state.

### Integration tests

- fake `CoreClient` project list/get;
- startup compatibility tab;
- two project tabs represented without cwd changes;
- all transport clients compile with catalog methods.

### Restart and recovery tests

- daemon reconnect invalidates/reloads catalog summaries;
- unsupported capability fallback.

### Contention and cancellation tests

- stale catalog completions;
- tab removed/rebound before completion;
- several concurrent catalog/detail requests remain scoped.

### Security and negative tests

- TUI displays only daemon-returned projects;
- invalid project/session IDs fail without path fallback;
- no credentials or full secret-bearing config in tab state/debug output.

### Migration and compatibility tests

- existing TUI async command tests;
- existing session selection and Git sidebar tests;
- current single-project keybindings and render snapshots where stable.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test tui::
cargo test --test core_transport
cargo test --test single_daemon
cargo test -p codegg-protocol
python3 scripts/check_daemon_cwd_usage.py
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add a focused project-tab state test module and fake-client integration target; run those explicitly before the broad TUI filter.

## 12. Documentation updates

- `architecture/tui.md` state hierarchy and async identity routing;
- `architecture/protocol.md` catalog client capability usage if needed;
- TUI compatibility mode and diagnostics;
- developer guidance for active-tab accessors and new component ownership.

## 13. Acceptance criteria

- `App` can represent a project catalog and several independent project-tab states.
- Existing startup and single-project use work through one compatibility tab.
- Catalog loading is daemon-backed, asynchronous, bounded, and stale-safe.
- Project/session/model/agent state is tab scoped.
- No cwd mutation, direct storage access, or frontend-authoritative project identity is introduced.
- The next milestone can add picker/navigation without redesigning state ownership.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- project catalog protocol or runtime asset generation interfaces are unavailable;
- existing TUI state cannot be migrated without creating dual authority;
- completing the milestone requires project discovery or direct DB access in the TUI;
- a broad renderer rewrite would exceed one coherent pass;
- remote TUI canonical projection must be redesigned before Phase 5;
- state ownership conflicts with daemon/session authority.

## 15. Closure evidence required

- implementation commit(s);
- old-to-new TUI field ownership map;
- catalog async/stale-completion test evidence;
- compatibility startup and existing TUI regression evidence;
- proof no cwd/direct storage project authority was introduced;
- exact verification commands/results;
- list of deferred picker/navigation/restoration/projection work;
- closure recommendation.

## 16. Handoff notes

- This plan is ready for handoff now that runtime-asset and project-catalog interfaces are closed.
- Preserve modal priority, task lifecycle, and terminal cleanup behavior.
- Use active-tab accessors as a migration seam, not as a permanent global-state disguise.
- Inspect current `main` before implementation and record the actual production baseline in closure.
