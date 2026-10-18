# Workspace Authority and Session Navigation Corrective Addendum

Status: active (W001 closed; W002/W003 not started)

Long-term references:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`
- `plans/002-long-term-roadmap.md#phase-8--read-only-observation-mode`

Related ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
  (accepted 2026-10-16). W001 and W002 are **conformance work against this
  accepted decision**; they introduce no new architectural decision. W003 is a
  capability that must be consistent with ADR-0033 but does not amend it.

## 1. Purpose and ownership boundary

This addendum owns two things that share one root cause: **the working
directory that tool execution actually uses must be the tab's execution
context, and the user must be able to see and act on the sessions and pending
decisions that context governs.**

- W001/W002 own the execution-side authority: `BashTool` and the other execution
  tools' workdir default, the daemon-cwd static guard, and session-switch root
  adoption in the TUI.
- W003 owns the presentation surface: the workspace dashboard's session
  enumeration and the cross-session pending approval/question list and jump.

It does **not** own project identity or catalog semantics (ADR-0033 /
`project-picker-rework`), provider/session projection storage, or the approval
authorization model itself.

## 2. Work classification

### Invariants

- A tool's working directory is the propagated `ExecutionContext.workspace_root`.
  There is **no** code path in daemon-owned execution where a working directory
  falls back to process-global cwd.
- Absence of a resolved workspace root is a **fail-closed** condition, never a
  silent substitution of the daemon's launch directory.
- Switching to a session whose directory differs from the active tab's root
  rebinds the tab's execution context, workdir default, and displayed path, as
  ADR-0033 requires.
- Daemon process cwd is never mutated and never becomes an input to tool
  execution.

### Capabilities

- The workspace surface enumerates the active sessions of a workspace and
  supports selecting one, switching to it.
- The workspace surface enumerates pending approvals and questions across
  sessions, and selecting one surfaces that session's decision UI.

### Infrastructure

- `PermissionRegistry` / `QuestionRegistry` already hold `session_id` and
  `turn_id`; the desktop bridge already exposes cross-session
  `pending_permissions` / `pending_questions` projections. The TUI does not
  consume them.

### Polish

- Documentation of daemon cwd inheritance and the cross-session surfaces follows
  the correctness boundary.

## 3. Non-goals

- Daemon `chdir`. ADR-0033 Option B was explicitly rejected; the daemon's
  process cwd staying at its launch directory is correct behaviour, not the
  defect.
- Changing ADR-0033's 1:1 root↔project rule, project archive/destroy, or
  repath. Those are owned by `project-picker-rework`.
- Split-panel working directories and the `Shift+H/J/K/L` chord.
- A second projection transport. The existing desktop projection is the model.
- Re-answering or auto-answering another session's pending decision from a list
  without switching to that session's context.
- Any change to permission authorization semantics.

## 4. Current state

### W001 — execution-side cwd authority

`BashTool` resolves `workdir` from the argument, else `self.workspace_root`,
else nothing. When it resolves to nothing, six sites in `src/tool/bash/process.rs`
fall back to `std::env::current_dir()`. Because the daemon is a user-scoped
singleton spawned without an explicit working directory, that value is pinned
for the daemon's lifetime to the directory of whichever launch first started it.

The observable result is reproduced on the review baseline: a daemon whose
process cwd is one tree serves a TUI launched from a different tree, and shell
tool execution resolves against the daemon's tree instead of the session's.

The session's own `workspace_root` **is** correct — it follows the launch
directory through workspace registration, session creation, runtime binding, and
per-turn `ExecutionContext` construction. The defect is the fallback, not the
session root.

The static guard `scripts/check_daemon_cwd_usage.py` does not catch this, and
cannot: its allowlist exempts `ToolRegistryOptions::default()` on the stated
premise that "the canonical daemon path populates them via
`ToolRegistryOptions.workspace_root`". That premise is exactly what is
unproven — the guard proves no *new* call sites, not that every constructed tool
has a root. Its own docstring states the Phase 2 requirement that working
directories come from `ExecutionContext` rather than process-global cwd, so the
fallback contradicts the guard's stated intent as well as ADR-0033. The guard's
`PROTECTED_GLOBS` also omits `src/main.rs` and `src/exec.rs` by omission rather
than by an explicit, documented entry, even though the docstring names them as
intentionally permitted bootstrap readers.

### W002 — session-switch root adoption

`App::set_session` adopts the session's directory into the active tab only when
the tab has no root. A tab normally already has one, so switching to a session
belonging to a different directory leaves the tab's execution context pointing at
the previous root. This contradicts ADR-0033 §2, which defines switching the
working directory as switching "the tab's execution context, tool workdir
default, and displayed path".

`/sessions` is additionally project-scoped: it requests a session list for the
active project, so it cannot enumerate a session from another project in the
first place.

### W003 — workspace surface capability

The `/workspace` command opens the project Work-Orders dashboard
(`WorkspaceDashboardState`), not a session picker. Its state documents rows as
per-project activity summaries "used for navigation only". Rows render name,
project id, a coarse status, and expansion to Work-Order tasks. Pressing Enter
opens or focuses the project and shows the task view; it neither switches nor
creates a session.

Rows already carry `pending_permission_count` and `pending_question_count`,
rendered as `permission×N question×N`, with an explicit comment that these are
"counts only; never the underlying permission/question content". No list of
which session holds which pending decision exists anywhere in the TUI.

The pieces a jump-through surface needs mostly exist and are currently
discarded or unused:

- `CoreEvent::PermissionPending` and `QuestionPending` already carry `session_id`
  and `turn_id`; the TUI binds them away.
- `PermissionRegistry` / `QuestionRegistry` are process-global and expose
  session identity.
- `CoreRequest::PermissionRespond` is already session-agnostic and the daemon
  resolves ownership from the permission id, enforcing owning-session authority
  and the controller lease.
- The permission queue entry carries no session id, while the question dialog
  already does.
- The TUI subscribes only to per-session projection streams, whereas the desktop
  bridge already renders cross-session pending lists.

## 5. Target architecture

Execution resolves working directories only from propagated context and fails
closed when absent, so the daemon's launch directory becomes observationally
irrelevant to tool execution. Session switching rebinds the tab context
atomically. The workspace surface becomes a navigation surface over sessions and
their pending decisions, reusing the existing session-agnostic response request
and the existing cross-session projection rather than inventing a transport.

## 6. Dependency graph

| Milestone | Depends on | Kind | Notes |
|---|---|---|---|
| W001 execution-side fail-closed authority | ADR-0033 (accepted) | interface | Contract is stable and accepted. No other unclosed dependency. |
| W002 session-switch root rebinding | W001 | hard | Adopting a session root is only safe once no execution path silently substitutes a different directory. |
| W003 workspace session navigation and cross-session pending decisions | W002 | hard | Switching workspaces from the dashboard must not reintroduce the W002 defect. Also depends on a cross-project session enumeration that does not exist today. |

W001 and W002 are ordered. W003 additionally requires a UX decision recorded in
§10.

## 7. Milestones

### Milestone W001 — Workspace-root execution authority (fail closed)

- **Class:** invariant
- **Objective:** Remove every process-cwd fallback from daemon-owned execution
  and make absence of a resolved root a typed, fail-closed outcome.
- **Deliverable boundary:** execution tool workdir resolution, the shared process
  launch path, and `scripts/check_daemon_cwd_usage.py`.
- **Exit conditions:**
  - No daemon-owned execution path resolves a working directory from process cwd.
  - A tool constructed without a workspace root returns a typed error naming the
    missing context rather than running somewhere.
  - A session whose root differs from the daemon's launch directory executes
    every tool against the session's root.
  - The guard covers the intended bootstrap files explicitly and fails when a
    production-constructed tool can lack a workspace root.
  - A regression test drives a session in a directory different from the
    daemon's and asserts resolved paths land in the session's directory.

### Milestone W002 — Session-switch working-directory rebinding

- **Class:** invariant
- **Objective:** Make switching sessions re-establish the tab's execution
  context, workdir default, and displayed path, per ADR-0033 §2.
- **Deliverable boundary:** session-switch adoption in the TUI, the project
  execution-context resolution, and `/sessions` enumeration scope.
- **Exit conditions:**
  - Switching to a session in a different directory rebinds the tab root; the
    existing `workspace_root.is_none()` guard no longer leaves a stale root.
  - The displayed path and the tool workdir default agree after the switch.
  - Switching to a session in the same directory is a no-op with respect to
    root.
  - `/sessions` can enumerate across the projects the user is entitled to switch
    between, or the UI states plainly that it is project-scoped.
  - A regression test switches A → B → A and asserts root restoration.

### Milestone W003 — Workspace surface session and pending-decision navigation

- **Class:** capability
- **Objective:** Make `/workspace` a navigation surface over each workspace's
  active sessions and their pending approvals and questions.
- **Deliverable boundary:** workspace dashboard rendering and key handling, the
  pending-decision enumeration for the TUI, and session-id propagation for
  permission dialogs.
- **Exit conditions:**
  - Each workspace row shows its active sessions and can be expanded to select
    one; selecting switches to that session through W002.
  - Pending approvals and questions are listed with the owning session, and
    selecting one switches to that session and surfaces its decision UI with the
    request's own arguments recovered correctly.
  - A pending decision from a non-active session is visible without switching.
  - Counts and lists cannot disagree: one source of truth for both.
  - Answering is only offered in the owning session's context, and daemon
    owning-session authority is unchanged.
  - A test drives a two-session scenario where one session holds a pending
    approval and asserts it is listed, selectable, and answerable.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema change. W003 reads existing registry and projection state.

### Protocol and compatibility

W001 and W002 are additive at most. W003 may add an enumerated pending-decision
query but must reuse `PermissionRespond` unchanged — it is already
session-agnostic and its daemon-side ownership resolution is the security
boundary that must not be weakened.

### Security and authorization

- Fail-closed resolution must not become a denial-of-service on legitimate
  standalone or subagent construction; those paths must receive a root through
  the context rather than by falling back.
- W003 must not let one session answer another's pending decision. Switching
  context first is the mechanism, not presentation.
- `CoreEvent::PermissionPending` carries no `args`; recovering the command for
  display must reuse the existing transcript recovery and must not be widened to
  expose arguments the request did not carry.

### Concurrency, cancellation, and recovery

A pending decision may be withdrawn between listing and selection. Selection must
tolerate a vanished entry rather than mounting an orphaned dialog.

### Observability and audit

Fail-closed tool resolution should log the missing context, never the path it
declined to guess.

### Documentation and operations

`docs/daemon.md` and `architecture/session.md` currently contain no statement of
daemon cwd inheritance. That fact must be documented, including that the daemon's
launch directory is deliberately not a tool-execution input after W001.
`architecture/tui.md` §file-tree root resolution must be reconciled with the
turn/tab root so the two are not read as the same contract.

## 9. Verification strategy

- Focused tests for workdir resolution, including a session root that differs from
  the daemon's.
- An integration test over the real daemon plus a TUI attached from a different
  directory, asserting resolved tool paths.
- Static guard runs unchanged and passing, plus the strengthened assertions.
- `cargo fmt`, `cargo clippy -D warnings`, `scripts/verify.sh quick`.
- No hosted CI or native cross-platform claim unless actually executed and cited.

## 10. Risks and decision points

- **W001 is a behaviour change for any caller that relied on the fallback**,
  including subagent and standalone tool construction. Those paths must be found
  and given a root explicitly; this is the bulk of the work and the main source
  of churn.
- **W002 changes what a session switch means**, which may surprise users who
  expect a session to stay in place. The displayed path must change with the
  context so the change is never silent.
- **W003 UX is genuinely undecided:** whether sessions nest under a workspace row
  or are a second level; whether a pending decision is answered in place or after
  switching; and what `/workspace` does when a workspace has both many sessions
  and many pending decisions. This is a product decision, not an implementation
  detail, and is recorded as a decision point rather than assumed.
- **W003 could regress the daemon approval boundary** if cross-session answering
  is ever allowed. The switch-first mechanism is the mitigation and must be
  enforced by test.

## 11. Completion definition

W001, W002, and W003 closed, with closure evidence that tool execution is
independent of the daemon's launch directory, that session switching re-establishes
the correct context, and that a pending decision held by a background session is
discoverable and answerable through the workspace surface.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| W001 | closed | `plans/implementation/workspace-authority-and-session-navigation/001-workspace-root-execution-authority.md` | `plans/closure/workspace-authority-and-session-navigation/001-status.md` | None |
| W002 | not started | — | — | W001 |
| W003 | not started | — | — | W002; UX decision (§10) |
