# ADR-0034: Directory-tied project identity and client working directory

Status: accepted

Date: 2026-10-16 (accepted 2026-10-16 by owner direction; gates
project-picker-rework M3–M5)

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Affected subsystem roadmaps:

- `plans/subsystems/project-picker-rework-roadmap.md`
- `plans/subsystems/tui-project-sessions-roadmap.md` (closed; invariants amended narrowly, history preserved)

## Context

Today a project is an opaque catalog record (`ProjectCatalogRecord`)
bound to workspaces through `workspace_project_binding`, and the model
permits N workspaces per project. Launch auto-creates a project named
for the directory, but picker selection opens a tab without switching
any working directory, so directories and projects diverge. The closed
TUI roadmap invariants say switching tabs never mutates process cwd,
and the protocol DTO notes a locator cannot become identity. The owner
direction (2026-10-16) requires: one directory is one project (one git
repo = one project), opening an unseen directory auto-creates it,
reopening loads it, selection switches the working directory, rows
support deactivate/reactivate (`d`/`a`), confirmed destroy (`d` on
inactive, sessions deleted), and rename plus path rebind (`e`).
`ProjectArchive`/`ProjectRestore` exist and keep sessions; no project
destroy, rename, or repath operation exists. The daemon is a user-scoped
singleton serving many tabs and TUIs.

## Decision drivers

- Directories and projects must stop diverging (1:1 rule).
- Destroy must be possible but never surprising (confirm + named count).
- Projects must be rebindable to a different checkout/worktree.
- A per-tab working directory must exist without breaking the singleton
  daemon or the daemon-cwd guard.
- Multi-cwd split panels are long-term; this decision must not pre-claim
  `Shift+H/J/K/L` (owned by panel navigation).

## Considered options

### Option A — 1:1 root↔project with client-owned working directory (recommended)

Each canonical root binds exactly one project. The TUI tab owns an
`ExecutionContext { project, session, workspace_root }`; selection
switches that context only. Daemon process cwd never moves. Destroy is a
new `ProjectDelete` op cascading to sessions in one transaction behind a
confirm dialog. Rename/repath is a validated update; repath to an
already-bound root is refused with a hint.

Benefits: matches the owner model; singleton-safe; destroy is explicit;
guard-compatible. Costs: new protocol + authz; migration rule needed for
existing N-workspace projects. Failure modes: half-cascade on crash
(mitigated by single-transaction-or-fail-closed).

### Option B — Daemon process cwd follows the active tab

Selection chdirs the daemon to the project root.

Benefits: trivially consistent tool resolution. Costs: incoherent for a
singleton serving many tabs/TUIs; breaks `check_daemon_cwd_usage.py`;
every background job and concurrent turn races one global cwd. Failure
modes: cross-tab file operations landing in the wrong tree. Rejected.

### Option C — No working-directory switch; tabs stay cwd-agnostic

Keep tabs as pure session selectors; tools resolve against launch cwd.

Benefits: no change. Costs: directly contradicts the required
per-project working directory and the split-panel future. Rejected.

## Decision

Adopt Option A:

1. Canonical root ↔ project is 1:1. Unseen roots auto-create a project
   named for the directory; seen roots load the bound project.
2. The working directory is client state per tab (`ExecutionContext`);
   the daemon never chdirs. "Switch cwd" in all plans means switching the
   tab's execution context, tool workdir default, and displayed path.
3. `d` archives (sessions kept, sinks inactive to list bottom); `a`
   restores; `d` on inactive destroys via new `ProjectDelete`, cascading
   to sessions atomically, behind a confirm dialog naming the session
   count, with a dedicated authz op.
4. `e` renames the display name and repaths the root binding. Repath
   validates existence, canonicalization, and dedup; repath onto an
   already-bound root is refused with an adopt-hint. Session bindings
   follow the project across rebind.
5. Existing N-workspace projects are grandfathered read-only until an
   operator rebinds them; new bindings are 1:1 only.
6. `Shift+H/J/K/L` remain panel navigation; split switching needs a
   future chord (out of scope).

This amends the closed TUI roadmap's "tabs never mutate cwd" to "tabs
never mutate *daemon* cwd; each tab owns a client working directory",
and grants the narrow 1:1 exception to "locator cannot become identity".
Neither predecessor record is rewritten.

## Consequences

### Positive

- Directory ↔ project correspondence becomes stable and explainable.
- Safe recoverable deactivation plus a deliberate, confirmed destroy.
- Rebindable projects without session loss.
- Singleton daemon and cwd guard stay intact.

### Negative

- New daemon mutation surface (`ProjectDelete`, update path) with authz
  and cascade testing burden.
- One irreversible operation exists (confirmed destroy deletes sessions).

### Neutral or deferred

- N-workspace grandfathering is accepted tech debt until rebind.
- Split panels and their switch chord are deferred entirely.

## Compatibility and migration

Additive protocol only (`ProjectDelete`, rename/repath update with
session-count payloads). Storage migration constrains future bindings to
1:1; existing multi-bindings grandfathered. No config format change.
Refusal copy for bound-root repath must name the owning project.

## Security and reliability implications

Destroy and repath require dedicated authz ops; destroy additionally
requires in-dialog confirmation naming the session count. Cascade runs
in one storage transaction or fails closed. Path validation reuses
`WorkspaceRegister` canonicalization/dedup (symlink/relative aliases
rejected). No secret handling change.

## Verification

M3 proves archive→bottom→restore, confirm-with-count destroy, cascade
atomicity, and dismiss-keeps-everything. M4 proves rename propagation,
rebind workdir resolution, and bound-root refusal. M5 proves
open-A→B→A identity restore, per-tab workdir isolation, restart/resume
binding preservation, and a clean `check_daemon_cwd_usage.py` run.

## Supersession

None.
