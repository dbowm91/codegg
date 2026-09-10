# Multi-Project TUI Frontend Convergence M005 — Project Execution Context and Command Scope

Status: ready for handoff

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Original closed milestone/closure that this corrects:

- `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`
- `plans/closure/tui-project-sessions/004-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none. This plan restores the already-selected project/workspace authority model.

Primary class: invariant / correctness

Closure record to create: `plans/closure/tui-project-sessions/005-status.md`

## 1. Objective

Remove process `cwd` as an implicit project/workspace selector from production TUI operations and project-local command discovery. Establish one explicit, immutable-per-dispatch project execution context derived from the active project tab/routing/session state, then route every project-scoped TUI action found by the implementation census through it.

This is a corrective milestone. The prior M004 closure asserted that the TUI no longer used path/current-focus authority; the current source still has allowlisted `std::env::current_dir()` execution/discovery paths. The implementation must fix the underlying paths and the guard weakness that allowed the mismatch to survive.

## 2. Why this milestone is ready

Project identity/catalog, workspace registration, session binding, routing registry, tab epochs, session projections, scheduler ownership, provider connections, and runtime assets are already closed. No new daemon contract is required to know the active project/workspace/session. M005 can therefore converge frontend routing without waiting on another subsystem.

## 3. Current implementation evidence

Before editing, perform and record a complete TUI ambient-context census. At minimum inspect:

- `src/tui/command.rs`: `COMMAND_REGISTRY`, `CommandRegistry::new`, `append_dynamic_commands`, dynamic command precedence and plugin command additions;
- `src/tui/components/dialogs/command.rs`: global registry consumption by the palette;
- `src/tui/commands/shell.rs`: human shell `cwd` selection;
- `src/tui/commands/test.rs`: `/test` workdir construction and subsequent `WorkspaceRegister`/`JobSubmit` flow;
- `src/tui/commands/plugins.rs`: process-backed command `PluginContext.project_dir`;
- editor, formatter, Git, worktree, LSP, research/local-source, terminal, import/export, asset and agent helper paths for any `current_dir`, `project_dir`, or active-session fallback that can select a project/workspace;
- `src/tui/app/state/project_tabs.rs`, routing/view-switch state, session state, and canonical workspace/session locators already available;
- `scripts/check_tui_project_authority.py` and its allowlist;
- `.github/workflows/ci.yml` and `scripts/verify.sh` to reconcile the guard's documented invocation.

Classify every process-cwd read as one of:

1. bootstrap/CLI locator boundary — may remain, with explicit comment;
2. presentation-only fallback — migrate if it can affect project behavior;
3. project/workspace authority — MUST migrate in M005;
4. test-only fixture — may remain under test scope.

Do not mechanically delete `current_dir()` when it is legitimately the one-time bootstrap locator.

## 4. Invariants that must not regress

- `ProjectId`, `WorkspaceId`, and `SessionId` retain their existing semantics; a path is never promoted to durable identity.
- The active tab/routing registry is the frontend source for active project selection; daemon/session/workspace records remain authoritative for durable binding/root locators.
- No `set_current_dir`/`chdir` is introduced to simulate project switching.
- TUI actions still pass through existing daemon authorization and scheduler/workspace execution boundaries.
- Standalone/CLI compatibility modes remain usable, but their bootstrap cwd is captured explicitly at the composition boundary and not reread as mutable project state.
- Inactive-tab events/completions remain route/epoch scoped.
- Dynamic commands/plugins do not gain authority by being discoverable.
- Existing command aliases, plugin precedence, permissions, command parsing, and runtime-asset precedence remain compatible unless current behavior is demonstrably startup-cwd leakage.

## 5. Scope

### In scope

- A small explicit context value/resolver, e.g. `ProjectExecutionContext`, containing the canonical active project/workspace/session locators required by TUI-local actions.
- Migration of all production TUI project/workspace selectors identified by the census.
- Human shell, `/test`, process-backed project command/plugin, dynamic slash-command discovery, and any equivalent project-sensitive editor/Git helper found by the census.
- Refactoring command registry construction so immutable built-ins/global definitions can remain shared while project-local dynamic definitions are scoped to project/workspace/asset generation.
- Correct invalidation/refresh on project activation, tab switch where required, asset refresh/reload, and project command source change according to existing runtime-asset semantics.
- Tightening `check_tui_project_authority.py` and aligning its documentation/normal verification invocation.

### Explicitly out of scope

- New command types or command UI taxonomy (M010).
- Prompt/session asynchronous lifecycle (M006).
- Modal ownership (M007).
- App-wide decomposition (M008).
- Changing daemon workspace identity or project catalog storage.
- Eliminating all global immutable command definitions.
- Watching the filesystem continuously for command changes if existing asset refresh semantics are sufficient.

## 6. Required production changes

### Core/domain

No new daemon domain owner. Prefer a frontend-only immutable context assembled from existing typed/string DTO identities and canonical workspace locator. If a helper currently only exposes path text where the daemon already has WorkspaceId, preserve the ID alongside the locator rather than passing path alone.

### Storage and migrations

None expected. Do not persist project-local command bodies in the TUI manifest. Runtime assets/project command files remain source-owned and rediscovered through existing refresh behavior.

### Protocol and DTOs

No protocol change expected. Stop if the TUI cannot obtain a canonical workspace binding/root through existing project/session/workspace APIs without inventing identity from path text; document the missing protocol field before adding an additive capability-negotiated field.

### Runtime and concurrency

Resolve context before spawning asynchronous work and move the resolved value into the task. Do not let a background task call a mutable `active_*` accessor after the user may have switched tabs. Stale completion still validates existing route/request epochs where it mutates presentation state.

### Frontend/operator surface

The visible behavior should be simple: active project B means shell/tests/project commands/plugins operate in B even if CodeGG was launched from A. Command palette results must not contain A-only project commands while B is active.

### Security and authorization

Explicit project/workspace locators are routing context, not authorization. Preserve daemon checks and plugin/tool permissions. Do not place secrets in the context value or logs.

### Documentation/static guards

Update `architecture/tui.md`, `architecture/command.md`, and `.opencode/skills/tui/SKILL.md` as needed. Narrow the static guard allowlist to explicit bootstrap/test cases rather than syntactic forms such as any `let cwd = current_dir`. The guard should fail on a new production TUI project-sensitive `current_dir()` call without requiring a fragile exhaustive list of approved source lines.

Because this is a durable Phase-4 invariant and the guard already claims normal integration, add the corrected guard to the existing `scripts/verify.sh quick` and existing single CI `verify` job if it is not already transitively executed. This is one extra command in an existing lane, not a new lane/framework.

## 7. Ordered work packages

### Work package A — Ambient-context census and canonical resolver

Intent: establish exactly which path reads are authority and exactly which existing state resolves the active project/workspace.

Required changes:

- record every production `current_dir`/global `project_dir`/active-session fallback under `src/tui`;
- map each affected operation to ProjectId/WorkspaceId/SessionId/root source;
- implement one narrow resolver/value object with explicit failure diagnostics;
- add unit tests for no-active-tab, no-workspace, and correctly bound tab/session cases.

Acceptance evidence: closure includes census table and all project-authority operations use the resolver or an equivalent explicit context passed from a canonical caller.

### Work package B — Execution call-site migration

Migrate shell, `/test`, process-plugin/project-command and every additional project-sensitive call site from WP-A. Preserve existing execution owner and request shape beyond the context fields.

Acceptance evidence: no migrated task rereads ambient cwd after dispatch; focused tests capture workspace/project locators.

### Work package C — Project-scoped dynamic command catalog

Separate static built-ins from project-local discovery. Preserve deterministic aliases/collision policy. Key project-local state by canonical project/workspace and, where appropriate, runtime-asset/command generation. Refresh on the existing explicit activation/reload seams; inactive project catalogs remain bounded or evictable.

Acceptance evidence: A-only command disappears after A->B switch; B-only command appears without process `chdir`; switching back restores A deterministically; duplicate names have documented winner/diagnostic.

### Work package D — Two-project regression fixture

Create a fake/in-process frontend fixture with OS cwd fixed at A, active tab switched to B, distinct project command files/workspace roots, and captured daemon requests.

Exercise at least human shell, `/test`, process-backed project command/plugin, and command palette/catalog. Include every additional authority call identified in WP-A when testable without a live provider.

Acceptance evidence: every operation targets B; no test uses `set_current_dir` as the mechanism of success.

### Work package E — Guard and docs correction

Tighten the TUI project-authority guard, remove broad exemptions, add focused positive/negative fixtures or self-tests if the script already supports them, align its header with actual invocation, and add it to the existing quick/CI command sequence.

Acceptance evidence: injecting a representative production `current_dir()` authority pattern causes the guard test to fail; documented bootstrap exception remains accepted.

## 8. Failure, cancellation, restart, and contention semantics

Failure to resolve active project/workspace fails the user action with an actionable toast/error; it must not silently fall back to process cwd. Background tasks capture immutable context at dispatch. If the tab closes/switches before completion, existing route/request guards decide whether UI results apply; daemon work already submitted is not implicitly cancelled unless the action's existing semantics say so.

Restart/restoration revalidates tab/session/workspace state before actions become available. A restored stale locator must not be converted into authority by falling back to launch cwd.

Concurrent TUIs remain independent because each resolves its own active frontend tab while all daemon authorization/admission remains canonical.

## 9. Compatibility and migration

Preserve one-time bootstrap behavior: when launched in a directory, CodeGG may use that directory to discover/register the initial project according to existing startup semantics. After the active tab is established, the process cwd is no longer the selector.

Preserve built-in/global command names and supported project-command formats. If the old global `COMMAND_REGISTRY` is public within tests, provide a built-ins/default compatibility accessor where necessary, but production project-local discovery must be instance/scoped rather than frozen static state.

## 10. Required tests

### Focused unit tests

- explicit context resolution for active project/workspace/session;
- missing/ambiguous binding fails closed;
- project command catalog filtering/collision/invalidation;
- guard allow/deny cases.

### Integration tests

- cwd=A / active=B matrix for shell, test, project process command/plugin, command palette/catalog;
- A->B->A switching without `chdir`;
- asset/project command refresh changes only the owning project.

### Restart/recovery tests

- restored active B still routes to B when startup cwd is A;
- stale restored workspace fails explicitly rather than falling back.

### Contention/cancellation tests

- background operation captures B and does not retarget when user switches to A before completion;
- tab close suppresses stale UI mutation according to existing route tokens.

### Security/negative tests

- project-local command cannot escape the canonical project root merely through discovery context;
- no secret-bearing values are added to context diagnostics.

### Migration/compatibility tests

- standalone/CLI bootstrap still resolves initial cwd once;
- immutable built-in command tests remain stable.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- tui::command
cargo test -p codegg --lib -- tui::app::state
cargo test --test tui_project_tabs
cargo test --test tui_project_routing
# run the new two-project execution-context integration target
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Adjust focused target names to actual files created. Do not add a new CI job.

## 12. Documentation updates

- `architecture/tui.md`: authoritative active-context flow and allowed bootstrap cwd use.
- `architecture/command.md`: built-in/global versus project-scoped dynamic command registry lifecycle.
- `.opencode/skills/tui/SKILL.md`: source locations/authority rules if changed.
- `scripts/check_tui_project_authority.py`: truthful header/allowlist rationale.
- roadmap/registry/closure status only after implementation evidence exists.

## 13. Acceptance criteria

M005 may close only when process cwd A can remain unchanged while project B is active and every audited project-scoped TUI operation targets B; project-local command discovery follows B; stale operations do not retarget; the corrected guard would catch reintroduction; and no daemon/scheduler/protocol authority was duplicated.

## 14. Stop conditions

Stop and report rather than improvise if:

- existing protocol/state truly cannot identify the active canonical workspace;
- fixing a call site requires daemon ownership redesign or schema migration;
- command precedence cannot be preserved without an unresolved compatibility decision;
- a proposed context object starts accumulating services/credentials/global state and becoming a generic service locator;
- implementation proposes process `chdir` as the fix;
- the baseline has materially changed such that the cited authority defects no longer exist.

## 15. Closure evidence required

The closure record must include:

- implementation commits;
- full ambient-context census with disposition of every `current_dir`/project-dir authority candidate;
- requirement-to-evidence matrix;
- two-project cwd=A/active=B test results;
- command-catalog precedence/invalidation evidence;
- corrected guard behavior and where it runs;
- focused + broad verification actually executed;
- compatibility review for bootstrap/standalone/project commands;
- unresolved findings with severity;
- recommendation: closed, conditionally closed, or corrective pass required.

## 16. Handoff notes

Inspect current `main` before editing. Preserve unrelated user changes. Prefer a small context value and explicit arguments over a global accessor used deep inside background tasks. Do not optimize for removing every textual occurrence of `current_dir`; optimize for eliminating ambient authority while preserving intentional bootstrap locators.