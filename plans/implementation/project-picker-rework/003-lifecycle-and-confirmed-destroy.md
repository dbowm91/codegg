# Project Picker Rework Milestone 003 — Deactivate/reactivate lifecycle plus confirmed destroy

Status: blocked — hard dependencies M2 interface stability (M1→M2 chain)

Repository baseline: `b4e9a92d` (rebase onto the M2 closure head before
handoff — see stop conditions)

Source roadmap:

- `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
  (Milestone 3)

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
  (accepted 2026-10-16 — satisfied; governs destroy cascade, confirm
  copy, and authz)

Primary class: capability

## 1. Objective

Give each picker row a safe lifecycle: `d` deactivates (archive,
recoverable, sinks inactive to the bottom), `a` reactivates (restore),
and `d` on an inactive project destroys it behind a confirm dialog that
names the affected session count — cascading to its sessions atomically.

## 2. Why this milestone is ready

Blocked, not ready: ADR-0033 is accepted (satisfied), but the M2 picker
interface (row selection context, help-style dialog, key contract) must
be stable first — this milestone hangs actions on M2's per-row hooks.

## 3. Current implementation evidence

- Daemon supports `ProjectArchive`/`ProjectRestore`
  (`src/core/daemon_projects.rs` → `daemon_refresh.rs`
  `project_lifecycle_request()` → `ProjectCatalog::{archive_project,
  restore_project}` + `ProjectArchived`/`ProjectRestored` events; HTTP
  `POST /api/projects/{id}/archive|restore`; authz ops
  `project_archive`/`restore`). Archive is one logical-row flip;
  sessions are kept. Session lists hide archived; context resolve
  rejects archived; restore skips archived tabs.
- Project detail already carries a session count
  (`ProjectDetailsDto` via `ProjectGet`) — the confirm dialog's count
  source; no new query path expected.
- No project destroy exists (only `SessionDelete { permanent }` for
  sessions). No per-row picker actions exist (M2 reserves `d`/`a`/`e`).

## 4. Invariants that must not regress

- Archive/restore never creates, deletes, or moves sessions.
- Destroy deletes the project's sessions only after explicit confirm;
  dismissing the dialog changes nothing (negative pin required).
- Cascade runs in one storage transaction or fails closed — no
  half-deleted projects on crash/restart.
- Generation/request scoping for all picker async flows.
- Daemon process cwd untouched; cwd guard clean.

## 5. Scope

### In scope

- New `ProjectDelete` daemon request + DTO (project id; response carries
  deleted session count for audit), catalog cascade implementation,
  `ProjectDeleted` event, HTTP route, dedicated authz op.
- Confirm dialog: names project + session count, explicit confirm vs
  dismiss; destroy executes only on confirm.
- Picker keys `d` (archive / destroy-when-inactive), `a` (restore),
  inactive sink to bottom (M2 ordering extended with lifecycle states),
  catalog refresh after each mutation, tab close on destroy of the
  active project.
- Audit entries for deactivate/reactivate/destroy with actor + counts.

### Explicitly out of scope

- Rename/repath (M4). Identity/auto-load/cwd switch (M5).
- Bulk multi-project actions. Undelete after destroy (destroy is final;
  say so in the confirm copy).

## 6. Required production changes

### Core/domain

- `ProjectCatalog::delete_project` (or equivalent): single-transaction
  cascade over project rows, workspace bindings, session bindings, and
  sessions; returns deleted session count.

### Storage and migrations

- No schema change expected (reuse existing tables); if the cascade
  needs one, add the migration and bump `STORAGE_LAYOUT_VERSION`
  (check `check_project_catalog_invariants.py`).

### Protocol and DTOs

- Additive `ProjectDelete` request/response + `ProjectDeleted` event;
  no negotiation change.

### Runtime and concurrency

- Daemon handler publishes the event; in-flight turns on destroyed
  sessions fail closed with a typed reason (no new execution authority).

### Frontend or operator surface

- Picker `d`/`a` handling on the M2 row hooks; confirm dialog component
  with count copy; catalog refetch + tab-close behavior on destroy.

### Security and authorization

- New `project_delete` authz op; destroy requires both the grant and
  the in-dialog confirmation. Confirm copy must state sessions are
  deleted permanently.

### Documentation and static guards

- `docs/tui.md` lifecycle keys + destroy warning; command/arch docs if
  they enumerate project ops.

## 7. Ordered work packages

### Work package A — Daemon delete path

Intent: atomic, audited project destroy.

Required changes: catalog cascade, request handler, event, HTTP route,
authz op.

Acceptance evidence: storage tests (cascade count, atomicity on
injected failure, archived-project destroy), authz denial test.

### Work package B — Confirm dialog + picker keys

Intent: no surprise deletes; dismiss is safe.

Required changes: confirm dialog with session count from `ProjectGet`,
`d`/`a` wiring, refresh + tab close.

Acceptance evidence: dialog key tests (confirm destroys, dismiss keeps
everything — sessions intact), picker refresh tests.

### Work package C — Docs and audit

Intent: operators understand the lifecycle.

Required changes: docs, audit entries.

Acceptance evidence: audit test asserting entries with actor + counts.

## 8. Failure, cancellation, restart, and contention semantics

Destroy is transactional or fails closed; crash mid-destroy leaves the
project intact or fully gone, never partial. Duplicate delete is
idempotent-safe (second call reports not-found, no error cascade).
Dismissed confirms issue no request. Concurrent archive + delete: delete
wins only if it commits; archive of a deleted project reports
not-found.

## 9. Compatibility and migration

Additive protocol; old frontends ignore the new event. No config
migration. Destroy is new capability, not a behavior change to archive.

## 10. Required tests

### Focused unit tests

- Catalog cascade: count accuracy, session/binding removal, atomicity
  under injected failure.
- Authz: delete denied without grant; archive/restore unchanged.

### Integration tests

- Picker/dialog: `d` archives and sinks; `a` restores; `d` on inactive
  opens confirm with count; confirm destroys + closes tab; dismiss
  changes nothing (sessions verified intact).
- Event: `ProjectDeleted` refreshes catalog projection.

### Restart and recovery tests

- Crash between confirm and commit (or injected commit failure) leaves
  no partial state; retry reports cleanly.

### Contention and cancellation tests

- Duplicate delete; archive-vs-delete race.

### Security and negative tests

- Ungranted delete refused; confirm copy names sessions as permanently
  deleted (copy test).

### Migration and compatibility tests

- Only if a migration proves necessary (§6).

## 11. Required verification commands

```bash
cargo test -p codegg-core -p codegg --lib project
cargo test -p codegg --lib tui::
cargo test -p codegg --test modifier_chords
scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `docs/tui.md`: `d`/`a`/destroy keys, confirm behavior, irreversibility
  note.
- `architecture/` project docs if they enumerate lifecycle operations.

## 13. Acceptance criteria

- `d` deactivates recoverably and sinks the row; `a` reactivates.
- `d` on inactive opens a confirm naming the session count; confirming
  deletes project + sessions atomically and closes affected tabs;
  dismissing changes nothing.
- Authz, audit, focused suites, and quick verification pass.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M2 has not closed (row hooks/key contract differ from §3);
- the cascade cannot be made single-transaction safely;
- ADR-0033 is superseded or its destroy semantics change;
- scope would expand into rename/repath (M4) or identity (M5).

## 15. Closure evidence required

- Focused/integration/restart/contention/security suite results and
  quick verification (local vs CI labeled).
- Manual note after reinstall + daemon restart: full lifecycle flow
  incl. confirm-with-count destroy and dismiss-keeps-everything.
- Unblock audit: M4 interface impact (none expected beyond shared
  dialog hooks) recorded.

## 16. Handoff notes

- Rebase onto the M2 closure head; the dialog/key seams this plan
  assumes come from M2.
- The confirm dialog is the safety-critical surface: copy, count
  accuracy, and dismiss-safety deserve more test weight than the keys.
- Do not invent undelete; destroy finality is a decided ADR point.
