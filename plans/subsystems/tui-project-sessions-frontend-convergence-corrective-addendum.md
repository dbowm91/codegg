# Multi-Project TUI Frontend Convergence Corrective Addendum

Status: active

Repository audit baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Parent roadmap and closure:

- `plans/subsystems/tui-project-sessions-roadmap.md` — M001-M004 remain historically closed.
- `plans/closure/tui-project-sessions/004-status.md` — closure evidence is preserved; this addendum records defects discovered after that closure.

Long-term references:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Related closed foundations:

- `plans/subsystems/session-projections-roadmap.md`
- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`
- `plans/subsystems/presence-observation-roadmap.md`
- `plans/subsystems/project-collaboration-roadmap.md`
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`

No new ADR is required. The work restores already-selected ownership: the daemon/protocol own durable project/workspace/session/agent state; the TUI renders and submits explicitly scoped intents. If implementation discovers that a canonical owner or wire/storage contract must change, the affected milestone must stop and request a separate ADR/plan.

## 1. Purpose and corrective trigger

The original multi-project TUI roadmap closed after establishing project tabs, routing epochs, bounded restoration, and a static guard intended to prevent ambient-path authority. A September 10, 2026 source audit found that several active TUI actions still derive execution/discovery context from process `cwd`, that prompt submission may await session creation directly in the main event loop, and that modal/focus state has parallel writable representations. The same audit found that the backend already projects richer agent-run/tree state than the keyboard-oriented TUI exposes.

These are post-closure defects and underdeveloped frontend surfaces, not grounds to rewrite the daemon or reopen M001-M004. This addendum owns the smallest coherent convergence pass that makes the TUI match the architecture already documented by those milestones.

## 2. Work classification

### Invariants

- Active project/workspace/session identity is never inferred from process `cwd` after bootstrap.
- Every TUI operation that can read, execute, discover, mutate, spawn, or persist project-scoped state receives explicit project/workspace/session context.
- A stale async completion cannot mutate a newly active tab/session.
- High-latency daemon calls do not block the terminal event/render loop.
- Every modal has one writable state owner; focus/navigation state cannot diverge from rendered modal state.
- Frontend changes do not bypass daemon authorization, scheduler admission, workspace ownership, projection replay, or observer read-only policy.
- Large run/tool/agent details remain on-demand; sidebar/project views remain bounded.

### Capabilities

- Human shell, supervised tests, process-backed project commands/plugins, and project-local command discovery execute against the visibly active project/workspace.
- Prompt submission remains responsive while a session is being created and is delivered at most once to the correct session.
- Keyboard users can traverse and inspect sidebar activity, including the durable/nested agent tree, without requiring mouse hover.
- Command discovery reflects the active project and presents the large command surface in coherent user-facing domains.

### Infrastructure

- One explicit TUI `ProjectExecutionContext` (name illustrative) or equivalent resolver backed by the active project tab/routing/session registry.
- Project-scoped dynamic command catalogs rather than one process-global startup-cwd catalog.
- Request/route-token state for deferred session creation and prompt continuation.
- One modal state/stack authority.
- Responsibility-oriented physical decomposition of the `App` implementation after the state contracts stabilize.

### Polish

- Reverse focus traversal wraps consistently.
- Sidebar focus/selection, collapse/expand, narrow-terminal behavior, help text, keybinding coverage, and command metadata are coherent and discoverable.
- Stale architecture/help comments that describe global or duplicate state are removed.

## 3. Non-goals

- A new frontend framework, actor system, Elm/TEA runtime, service bus, or generic state-management library.
- Replacing Ratatui or changing the application binary topology.
- Rewriting the daemon, scheduler, projection protocol, agent-run store, worktree service, collaboration service, or authorization model.
- Merging semantically distinct `goal`, `plan`, `todo`, `job`, `run`, `schedule`, and `agent run` domain models merely because the UI currently exposes many nouns.
- Loading full run logs, agent outputs, Git diffs, or histories into the sidebar.
- New web/desktop/mobile frontends.
- New CI lanes, snapshot frameworks, coverage gates, benchmark gates, or line-count gates.

## 4. Current-state evidence

At the audit baseline:

- `src/tui/command.rs` constructs a process-global `LazyLock<CommandRegistry>` and project-local dynamic command discovery starts from `std::env::current_dir()`. The command palette consumes that global registry, so commands discovered for the startup directory can outlive project-tab changes.
- `src/tui/commands/shell.rs` chooses human-shell `cwd` from `std::env::current_dir()`.
- `src/tui/commands/test.rs` constructs `/test` workdir from `std::env::current_dir()` before daemon workspace registration/submission.
- `src/tui/commands/plugins.rs` populates process-backed command `PluginContext.project_dir` from `std::env::current_dir()`.
- `scripts/check_tui_project_authority.py` intends to reject ambient-path authority, but broad allowlist expressions cover the remaining `current_dir()` forms; it also claims CI integration that is absent from the current `ci.yml` and `scripts/verify.sh` normal paths.
- the terminal loop calls `ensure_local_session(app).await` before dispatching a pending prompt when no session exists; the helper awaits `CoreClient::request`, unlike the documented spawn-and-complete pattern used by other high-latency TUI operations.
- `Dialog`, `DialogType`, `ui_state.dialog`, separately stored dialog components, and `FocusManager` overlap. `replace_top_dialog` exists specifically to synchronize a separately mutated dialog into a cloned focus-stack component, while `Collaborators` and `ProjectChat` are modal `DialogType`s that map back to `Dialog::None`.
- reverse Tab focus uses `saturating_sub(1)` while forward Tab wraps.
- `src/tui/app/mod.rs` remains about 722 KiB / roughly 15K lines after broader repository decomposition; `TuiCommand` and many render/input/session/plugin/modal responsibilities remain concentrated there.
- `SidebarWidget` already exposes goal/plan/todo/file/tool-program/agent-run/convergence sections, but keyboard-focused methods are stubs and several rows are display-only.
- the projection protocol carries a bounded `agent_tree`/`AgentTreeNodeProjection`, while the TUI primarily shows a flat `AgentRuns` list and subagent count. This is below the canonical TUI target behavior, which explicitly includes an agent tree and `Space a` interaction concept.
- the slash-command surface has grown beyond one hundred built-ins while `CommandCategory` remains only Session/Agent/System and the palette shows a small flat fuzzy result list.

## 5. Target architecture

The target keeps `App` as the frontend composition root but makes authority explicit:

```text
App
|-- daemon/client + projection controller
|-- project catalog + ProjectTabs + RoutingRegistry
|-- active ProjectExecutionContext resolver
|-- modal stack/state owner
|-- bounded per-tab presentation state
|-- command catalogs: immutable built-ins + active-project dynamic catalog
|-- async task/request lifecycle
`-- render/input composition

ProjectExecutionContext
|-- ProjectId
|-- WorkspaceId / canonical workspace root locator
|-- optional SessionId
|-- active tab/view epoch
`-- asset/catalog generation where discovery requires it
```

A user intent resolves this context once at dispatch and carries it through asynchronous work. Filesystem roots are locators obtained from canonical workspace/session state, not a substitute for project identity.

Dynamic command discovery is project-scoped and refreshed/invalidation-aware. The global built-in command definition set may remain immutable/static; project/plugin additions must not be frozen to startup `cwd`.

Modal state follows one-owner semantics: either the modal stack owns live components or it stores stable handles into one modal-state store. A second mutable clone is not authoritative. `Dialog`/`DialogType` may retain compatibility adapters during migration, but only one discriminator drives opening/rendering/closing.

Async prompt/session continuation uses a start -> completion -> apply state machine. Rendering/input processing continues while the daemon request is in flight. Completion validates the originating tab/session/view/request generation before binding or submitting.

The sidebar becomes a keyboard-operable bounded activity tree. Agent hierarchy consumes the existing projection/run surfaces; detail opens existing on-demand run/artifact/source views instead of duplicating data.

## 6. Dependency graph

```text
M005 Project execution context + scoped command discovery
   |\
   | \ hard
   |  `--> M006 Nonblocking session-create/prompt lifecycle
   |
   `------> M010 Command discovery + keybinding convergence

M007 Modal/focus single ownership
   |\
   | \ hard
   |  `--> M009 Keyboard sidebar + agent-tree inspector
   |
   `------> M008 App/domain physical decomposition

M005 + M006 + M007 --hard--> M008
M005 --soft--> M009
M009 --soft--> M010
```

M005-M007 have no unresolved hard dependencies and are closed. M006 is
hard-dependent on M005 because prompt continuation must resolve/bind the
correct project/workspace context. M008 is now ready after M005-M007 so
physical movement does not fossilize transitional ownership. M009 is now
ready after M007 and consumes already-closed projection/agent-run interfaces.
M010 depends on M005's final scoped command-catalog contract; M009 remains
only a soft sequencing preference.

## 7. Milestones

### M005 — Project execution context and project-scoped command discovery

Class: invariant / correctness.

Implementation: `plans/implementation/tui-project-sessions/005-project-execution-context-command-scope.md`

Objective: remove ambient `cwd` as active-project authority from production TUI execution/discovery paths and make dynamic command discovery follow the active project/workspace.

Exit conditions:

- human shell, `/test`, process-backed project commands/plugins, editor/Git helpers discovered by the census, and dynamic command discovery receive explicit active project/workspace context;
- startup/CLI compatibility reads of process `cwd` are isolated and documented as boundary-only;
- switching A -> B without process `chdir` causes every project-scoped action to target B;
- command catalog refresh/invalidation follows project/asset generation and cannot leak A-only commands into B;
- the TUI project-authority guard detects newly introduced ambient-cwd authority and is aligned with normal verification without adding a CI lane.

### M006 — Nonblocking session creation and prompt-submit continuation

Class: invariant / correctness / polish.

Implementation: `plans/implementation/tui-project-sessions/006-nonblocking-session-submit-lifecycle.md`

Objective: remove direct daemon waits from the terminal event loop when a prompt requires session creation, with exactly-once continuation and stale-route protection.

Exit conditions:

- session creation is registered async work and the TUI continues processing render/input/tick events while it is pending;
- duplicate submit, tab switch/close, reconnect, failure, and shutdown have deterministic behavior;
- a successful completion binds/submits only to the originating valid project/tab context;
- prompt text is not lost on failure and is not delivered twice.

### M007 — Modal/focus single-state ownership

Class: invariant / polish.

Implementation: `plans/implementation/tui-project-sessions/007-modal-focus-state-convergence.md`

Objective: establish one writable owner for modal component state and one consistent focus contract while preserving existing modal behavior.

Exit conditions:

- no normal modal requires clone/mutate/`replace_top_dialog` synchronization between two writable copies;
- one canonical modal discriminator/owner controls open/render/update/close semantics;
- plugin/info/collaboration/provider dialogs continue to work;
- forward/reverse focus traversal wrap consistently and modal focus cannot leak to the prompt.

### M008 — App responsibility decomposition and intent/effect contract

Class: polish / maintainability.

Implementation: `plans/implementation/tui-project-sessions/008-app-domain-decomposition-intent-effect-boundaries.md`

Objective: physically decompose the oversized TUI composition module along already-established responsibilities after M005-M007 close, and make the `TuiMsg`/`TuiCommand` directionality explicit without inventing a new state framework.

Exit conditions:

- `App` remains the composition root but major render, modal, project/session, prompt/turn, plugin/remote, and command-completion clusters live in coherent modules discovered by a responsibility census;
- synchronous component/user intent and asynchronous app/effect completion have documented, testable directionality;
- duplicate variants with no semantic distinction are removed or explicitly justified;
- no behavior, daemon/protocol ownership, cancellation, or routing semantics change merely for source layout.

### M009 — Keyboard sidebar and durable agent-tree inspector

Class: capability / polish.

Implementation: `plans/implementation/tui-project-sessions/009-keyboard-sidebar-agent-tree-inspector.md`

Objective: make the existing sidebar/activity surface first-class for keyboard users and expose the already-available nested agent hierarchy through it.

Exit conditions:

- sidebar rows have stable focus targets and keyboard navigation/collapse/inspect semantics;
- agent runs render parent/child hierarchy, status/attention and bounded worktree/branch context from canonical projection/run state;
- detail opens existing bounded/on-demand detail surfaces;
- observer/read-only mode exposes only permitted inspection and cannot steer/cancel/mutate through the inspector;
- narrow terminal and rapid projection-update behavior remain bounded.

### M010 — Command discovery and keybinding convergence

Class: capability / polish.

Implementation: `plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md`

Objective: make the large slash-command/keybinding surface discoverable without merging distinct backend domains or adding another router.

Exit conditions:

- command metadata has coherent user-facing domains/source/scope and active-project availability;
- palette/help/keybinding surfaces share canonical metadata where practical rather than maintaining divergent labels;
- project-local/plugin command collisions are deterministic and visible;
- every intended configurable `InputAction` is either represented in the keybinding config surface or explicitly classified non-configurable with rationale;
- power-user slash command compatibility remains intact.

## 8. Cross-cutting requirements

### Storage and migration

No daemon database migration is expected. Existing tab manifests remain bounded presentation intent. If project-scoped command metadata requires persistence, prefer recomputation from runtime assets/plugin registry; do not make the TUI a durable command owner.

### Protocol and compatibility

Use existing project/workspace/session/projection protocol fields. Do not add a TUI-only authoritative protocol. Additive protocol fields are permitted only if current canonical projections lack data required by the target UX; such a finding must be justified and capability-negotiated.

### Security and authorization

Explicit TUI context narrows routing but does not authorize operations. Daemon-side authorization remains mandatory. Observer mode remains fail-closed. Plugin/project command context may include locators but no secrets. Agent-tree details must respect projection visibility/redaction.

### Concurrency, cancellation, and recovery

Every new async continuation carries request generation plus tab/project/workspace/session identity sufficient to reject stale completion. Closing a tab invalidates frontend work without implicitly cancelling daemon-owned jobs. Shutdown cancels TUI-owned tasks. Reconnect must resync through canonical projection/session state.

### Observability

Existing tracing/task counters are sufficient. Add focused stale-context/session-create diagnostics where useful; do not create a metrics subsystem.

### Performance and resource use

No render path may synchronously perform filesystem discovery, Git, daemon/network, or command execution. Dynamic command catalogs, sidebar nodes, and agent trees are bounded and updated incrementally or by cached snapshots. Detail remains lazy.

### Documentation and verification

Update `architecture/tui.md`, `architecture/command.md`, relevant help/keybinding docs, and `.opencode/skills/tui/SKILL.md` when source/interaction contracts change. Keep verification proportional: focused tests, narrow ownership guard, all-feature Clippy, and `scripts/verify.sh quick`; no new lane/framework.

## 9. Verification strategy

The key end-to-end fixture for this addendum is a two-project frontend test with process `cwd` deliberately left at project A while project B is active. It should exercise every production TUI operation found by the M005 census that can select a workspace/project. The assertion is identity/locator correctness, not process `chdir`.

Additional focused coverage should include slow/failing fake core clients for event-loop responsiveness, modal open/update/close/focus traversal, stale completion under rapid switching, keyboard sidebar operation, nested agent-tree projection/reconnect, command-catalog refresh/collision behavior, and narrow render widths.

Broad closure posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Hosted CI is required only where the repository's existing closure convention calls for it; this addendum must not create additional workflow jobs.

## 10. Risks and decision points

- A `current_dir()` call may be a legitimate bootstrap/CLI locator rather than project authority. M005 must classify each call rather than globally ban the function.
- Dynamic project commands may currently be merged with plugin/global commands in ways not obvious from `CommandRegistry`; preserve documented precedence while making scope explicit.
- Modal convergence can become a broad UI rewrite. M007 should migrate one ownership seam and retain adapters temporarily rather than rewriting every dialog API at once.
- App decomposition before state contracts close would create churn. M008 is intentionally downstream.
- Agent-tree detail may tempt duplication of run/projection storage. M009 must consume canonical summaries/handles and open existing detail surfaces.
- Command taxonomy is presentation metadata, not a new backend domain model.

No current decision requires an ADR. Changing daemon ownership, projection authority, or command execution authority does.

## 11. Completion definition

This corrective addendum closes only when M005-M010 have accepted closure records and the TUI can be described truthfully as a projection-driven, multi-project frontend whose active project determines all project-scoped user actions without ambient-cwd dependence; whose event loop remains responsive during daemon work; whose modal state has one owner; and whose keyboard UI exposes the existing activity/agent architecture coherently.

The original M001-M004 closure records remain historical evidence and must not be rewritten to conceal these post-closure findings.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M005 | closed | `plans/implementation/tui-project-sessions/005-project-execution-context-command-scope.md` | `plans/closure/tui-project-sessions/005-status.md` | — |
| M006 | closed | `plans/implementation/tui-project-sessions/006-nonblocking-session-submit-lifecycle.md` | `plans/closure/tui-project-sessions/006-status.md` | — |
| M007 | closed | `plans/implementation/tui-project-sessions/007-modal-focus-state-convergence.md` | `plans/closure/tui-project-sessions/007-status.md` | — |
| M008 | ready | `plans/implementation/tui-project-sessions/008-app-domain-decomposition-intent-effect-boundaries.md` | `plans/closure/tui-project-sessions/008-status.md` | — |
| M009 | ready | `plans/implementation/tui-project-sessions/009-keyboard-sidebar-agent-tree-inspector.md` | `plans/closure/tui-project-sessions/009-status.md` | soft: M005; M009 hard dependency M007 is closed |
| M010 | ready | `plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md` | `plans/closure/tui-project-sessions/010-status.md` | soft: M009 |
