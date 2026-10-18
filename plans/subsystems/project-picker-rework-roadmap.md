# Project Picker Rework Roadmap

Status: closing

Long-term references:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md` — project tab, session, workspace, frontend state
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md` (accepted 2026-10-16; gates M3–M5)

## 1. Purpose and ownership boundary

This workstream owns the TUI project-picker experience and the
directory-tied project model: how the picker opens, how it renders and
filters, which per-project actions it offers, how a project binds to
exactly one root directory, and how the client tracks one working
directory per project tab.

It consumes the daemon project catalog (`ProjectList`/`ProjectGet`),
workspace registration, session listing, the existing async
generation/request guards, and the `/help` dialog interaction pattern.
It must not own project discovery, session storage authority, daemon
scheduling, provider/model selection, or daemon process working-directory
state. The daemon remains a user-scoped singleton; per-tab working
directories are client-side `ExecutionContext`, never daemon chdir.

## 2. Work classification

### Invariants

- Each newly bound root directory maps to exactly one project. Historical
  multi-root project bindings remain readable until an M4 rebind graduates
  them to one root.
- Selecting a project switches only the client's working directory
  (`ExecutionContext`); the daemon process cwd never moves.
- Archiving never deletes sessions; destroying a project deletes its
  sessions only behind an explicit confirm dialog that names the count.
- Async completions stay generation/request scoped and cannot mutate the
  wrong tab after switching (inherited from the closed
  `tui-project-sessions` roadmap).
- Closing a tab does not delete or archive its durable sessions or project.

### Capabilities

- `Shift+F` (Normal mode) or `/projects` opens a `/help`-style project
  picker; filtering and selection work fully in Normal mode.
- The status line shows the active project name plus its cwd/worktree
  path; the sessions panel shows each session's project name.
- Per-project row actions: `d` deactivate (recoverable) / `a` reactivate /
  `d` on inactive destroys with confirm; `e` renames and repaths (rebind).
- Opening an unseen directory auto-creates a project named for the
  directory; reopening a seen directory loads the bound project.

### Infrastructure

- `ProjectDelete` daemon operation with cascading session delete, confirm
  payload (session count), and dedicated authz op.
- Project update path for rename plus root rebind (extend
  `ProjectRegister` to upsert or add `ProjectUpdate`; ADR-0033 decides).

### Polish

- Removal of the `Ctrl+\` opener (M1).
- Inactive projects sink to the bottom of the picker list.

## 3. Non-goals

- Multi-cwd split panels and rapid split switching (deferred long-term;
  `Shift+H/J/K/L` is owned by panel navigation and cannot be reused
  without a separate design).
- Project discovery/scanning changes beyond explicit launch and picker
  directory opens.
- Team presence, observer mode, remote workspaces.
- Hard project deletion without confirmation or without naming affected
  sessions.
- Giving the TUI direct database or workspace-service ownership.

## 4. Current state

The closed `plans/subsystems/tui-project-sessions-roadmap.md` (M001–M004)
delivered the bounded catalog cache, tab routing, and manifest restore.
M1–M5 now provide the help-style picker and project/session surfaces,
lifecycle and confirmed destroy, rename/repath, directory-named
auto-create/load, and client-owned per-tab working directories. Startup
and local picker opens share `ensure_bound_project_context`; repository
lineage remains metadata and does not merge distinct roots. The daemon cwd
is unchanged.

## 5. Target architecture

```text
Picker (help-style dialog: title / filter / rows / footer + NOR|INS)
  |-- catalog phase: filter + select in NOR, inactive sink to bottom
  |-- row actions: d/a lifecycle, e rename+repath form, destroy confirm
  +-- selection commits ExecutionContext { project, session, workspace_root }

Status line: active project name + cwd/worktree path (middle-truncated)
Sessions panel: per-row project name (or active-project header)

Daemon: ProjectArchive/Restore (kept) + ProjectDelete (cascade + count)
        + rename/repath update; authz ops per mutation
Storage: 1 canonical_root <-> 1 project; repath to a bound root refused
```

## 6. Dependency graph

```text
M1 (remove Ctrl+\)
  |
  +--> M2 (help-style picker + surfaces)
            |
            +--> M3 (d/a lifecycle + destroy) --\
            |                                    +--> blocked on ADR-0033
            +--> M4 (e rename + repath) ---------/
            |
            `--> M5 (1:1 identity + auto-load + client cwd switch, ADR-0033)
```

M1 is a hard dependency of M2 (shared key/help/description surface).
M3/M4/M5 each carry a hard dependency on accepted ADR-0033 (new
protocol, new authz semantics, identity change). M3/M4 are interface
dependents of M2 (row-action hooks in the new dialog). M6 (multi-cwd
splits) is deferred and not planned here.

## 7. Milestones

### Milestone 1 — Remove the `Ctrl+\` opener

Class: polish

Objective: `Shift+F` (Normal) and `/projects` become the only picker
openers; no `Ctrl+\` binding, help text, or description remains.

Dependencies: none (hard deps closed; builds on the landed
`Shift+F` + `/projects` surface).

Deliverable boundary: keymaps, help entries, command descriptions,
docs, and key tests updated; picker behavior otherwise unchanged.

User or operator value: one discoverable opener; `Ctrl+\` freed.

Exit conditions: no `Ctrl+\` reference in code/docs/tests; `Shift+F`
NOR + `/projects` open the picker; quick green.

Deferred work: everything else in this roadmap.

### Milestone 2 — Help-style picker with Normal-mode filter/select plus cwd and project surfaces

Class: capability

Objective: picker renders in the `/help` format with full Normal-mode
filter/select; the status line shows project name + cwd; the sessions
panel shows project names.

Dependencies: M1 closed (hard).

Deliverable boundary: dialog render + key handling rework, status-line
field, sessions-panel project name; no protocol or storage change;
generation guards preserved.

User or operator value: filterable/selectable picker without leaving
Normal mode; always-visible cwd and project context.

Exit conditions: NOR-only filter+select flow; Insert path unchanged;
inactive sink to bottom; focused dialog/key tests green.

Deferred work: row mutations (M3/M4), identity change (M5).

### Milestone 3 — Deactivate/reactivate lifecycle plus confirmed destroy

Class: capability

Objective: `d` archives (recoverable, sinks to bottom), `a` restores,
`d` on inactive destroys behind a confirm box naming the session count.

Dependencies: ADR-0033 accepted (hard: new `ProjectDelete` protocol +
authz + cascade semantics); M2 interface stable.

Deliverable boundary: new daemon op + authz + catalog cascade, confirm
dialog, picker lifecycle keys, catalog refresh.

User or operator value: safe recoverable deactivation; deliberate
destroy path that cannot surprise-delete sessions.

Exit conditions: archive→bottom→restore round-trip; destroy confirms
with count, deletes sessions, closes tab; negative tests (dismiss keeps
everything).

Deferred work: rename/repath (M4).

### Milestone 4 — Rename and repath (rebind) via `e`

Class: capability

Objective: `e` edits project name and path, rebinding the project to a
different workspace root/environment.

Dependencies: ADR-0033 accepted (hard: rebind semantics, bound-root
refusal rule, session-binding follow rule); M2 interface stable.

Deliverable boundary: rename/repath form, path validation
(exists/canonicalized/deduped), daemon update path, session bindings
follow the project.

User or operator value: projects can move between checkouts/worktrees
without losing sessions.

Exit conditions: rename updates display everywhere; repath rebinds and
tools resolve under the new root; repath to a bound root refused with a
hint.

Deferred work: 1:1 enforcement for unseen/seen directories (M5).

### Milestone 5 — Directory-tied 1:1 identity, auto-create/load, client cwd switch

Class: invariant + capability

Objective: one directory is one project; opening an unseen directory
creates it (named for the directory), reopening loads it; selection
switches the client's working directory.

Dependencies: ADR-0033 accepted (hard); M2–M4 closed (hard: picker and
mutation surfaces must exist first).

Deliverable boundary: canonical-root↔project 1:1 enforcement, picker +
launch auto-create/load unification, `ExecutionContext` switch on
selection; daemon cwd untouched and guarded.

User or operator value: directories and projects stop diverging; each
tab works in its own directory.

Exit conditions: open-A→B→A restores the same project; tool workdir and
shell resolve under the tab root; `check_daemon_cwd_usage.py` clean;
restart/resume preserves bindings.

Deferred work: multi-cwd splits (future design, new chord required).

## 8. Cross-cutting requirements

### Storage and migration

M5 enforces 1:1 for new workspace↔project bindings. Existing
multi-workspace rows remain readable and require operator rebind under M4;
no migration or `STORAGE_LAYOUT_VERSION` change was needed.

### Protocol and compatibility

M3 adds `ProjectDelete` (+ session-count payload for confirm); M4 adds
rename/repath update. Both additive; no capability negotiation change.
`SessionUpdated`-style additive DTO evolution preferred.

### Security and authorization

New authz ops for delete and update; destroy requires explicit
confirmation naming affected sessions. No secret handling change.

### Concurrency, cancellation, and recovery

Picker keeps generation/request scoping; destroy/update are
idempotent-safe behind confirm; restart mid-destroy must not leave
half-deleted projects (cascade in one transaction or fail closed).

### Observability and audit

Lifecycle mutations publish catalog events; audit trail records
deactivate/reactivate/destroy/rename/repath with actor and counts.

### Performance and resource use

Picker caps (`MAX_PROJECT_LIST_ITEMS`, visible rows) preserved;
inactive sink is a sort change, not a larger fetch.

### Documentation and operations

`docs/tui.md`, command docs, and key help updated per milestone;
install + daemon restart noted for manual verification.

## 9. Verification strategy

Focused `tui::input::` / `tui::command::` / dialog suites plus
`modifier_chords` per milestone; `scripts/verify.sh quick` as the gate;
reinstall (`cargo install --locked --path .`) + daemon restart for
manual passes. M3 adds destroy-confirm and cascade tests; M4 adds
rebind tests; M5 adds open-A→B→A restore, workdir-resolution, and
daemon-cwd-guard evidence.

## 10. Risks and decision points

- ADR-0033 was accepted on 2026-10-16 and the M2–M4 sequencing gates are
  satisfied. If a successor ADR supersedes it, any new work stops under the
  individual plan stop conditions.
- Destroy cascade semantics (sessions deleted) is the one irreversible
  step; confirm copy and authz scope need maintainer sign-off in the ADR.
- Repath-to-bound-root refusal vs adoption (ADR-0033 recommends refusal).
- Grandfathering existing N-workspace projects under 1:1 (ADR-0033).
- `Shift+H/J/K/L` cannot serve split switching later; a new chord needs
  its own design (explicitly out of scope here).

## 11. Completion definition

The roadmap closes when M1–M5 have strict closure records, ADR-0033 is
accepted, the picker opens only via `Shift+F` / `/projects`,
lifecycle/rename/repath work end-to-end against the daemon, 1:1 directory
identity holds with auto-create/load, and no milestone carries unresolved
high/medium findings. M5 local acceptance is complete; its installed-binary
daemon-restart walkthrough remains outstanding for strict closure.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/project-picker-rework/001-remove-ctrl-backslash-opener.md` | `plans/closure/project-picker-rework/001-status.md` | none |
| 2 | closed | `plans/implementation/project-picker-rework/002-help-style-picker-and-surfaces.md` | `plans/closure/project-picker-rework/002-status.md` | strict local evidence; M3 and M4 unblocked |
| 3 | closed | `plans/implementation/project-picker-rework/003-lifecycle-and-confirmed-destroy.md` | `plans/closure/project-picker-rework/003-status.md` | none |
| 4 | closed | `plans/implementation/project-picker-rework/004-rename-and-repath.md` | `plans/closure/project-picker-rework/004-status.md` | none |
| 5 | conditionally closed | `plans/implementation/project-picker-rework/005-directory-identity-and-client-cwd.md` | `plans/closure/project-picker-rework/005-status.md` | Local acceptance and quick verification pass; installed-binary daemon-restart walkthrough remains outstanding |
