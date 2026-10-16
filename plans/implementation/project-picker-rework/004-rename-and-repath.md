# Project Picker Rework Milestone 004 — Rename and repath (rebind) via `e`

Status: closed

Repository baseline: `533b4a34` (M3 strict closure; sequential handoff)

Source roadmap:

- `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
  (Milestone 4)

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
  (accepted 2026-10-16 — satisfied; governs rebind semantics,
  bound-root refusal, session-follow rule)

Primary class: capability

## 1. Objective

Let `e` on a picker row edit the project name and path, rebinding the
project to a different workspace root/environment, with sessions
following the project and repath onto an already-bound root refused.

## 2. Why this milestone is ready

ADR-0033 is accepted, and M2 is strictly closed at
`plans/closure/project-picker-rework/002-status.md`. The picker now has
stable per-row selection context, a help-style dialog, and a documented
Normal/Insert key contract for rename/repath actions to extend. M3 and
M4 are independent after M2 and may be implemented sequentially.

## 3. Current implementation evidence

- `ProjectCatalogRecord` carries `display_name` (+ description/tags);
  title renames elsewhere recompute slugs in the same `UPDATE`
  (precedent for rename propagation).
- `WorkspaceRegistry` (`crates/codegg-core/src/workspace.rs`)
  canonicalizes roots, dedups, and rejects symlink/relative aliases —
  the validation to reuse for repath input.
- `workspace_project_binding` links workspace↔project; session bindings
  are project-scoped (`session_project_binding`).
- No rename or repath operation exists; `ProjectRegisterRequestDto`
  takes a `workspace_id`, never a path. No edit form exists in the
  picker (M2 reserves `e`).

## 4. Invariants that must not regress

- Sessions follow the project across rename and rebind (no session
  left bound to the old root).
- Repath onto an already-bound root is refused with a hint naming the
  owning project (1:1 preserved; no silent adoption).
- Path validation is exactly `WorkspaceRegister`-grade (canonicalized,
  deduped, alias-rejecting).
- Generation/request scoping for picker async flows; daemon cwd
  untouched.

## 5. Scope

### In scope

- Daemon rename + repath update path (new `ProjectUpdate` request or
  `ProjectRegister` upsert — implementer picks within ADR bounds;
  record the choice in closure).
- `e` edit form (name + path fields), validation errors inline,
  commit + catalog refresh.
- Dedicated authz op for project update; audit entries with old/new values.
- Bound-root refusal copy naming the owning project.

### Explicitly out of scope

- Lifecycle/destroy (M3). Identity enforcement for unseen/seen
  directories and cwd switching (M5).
- Moving session message history or rewriting directories on disk
  (rebind changes bindings, never file contents).

## 6. Required production changes

### Core/domain

- Catalog update: rename display name (+ description/tags if the form
  carries them); rebind workspace↔project link to the validated root's
  workspace (registering it idempotently first).

### Storage and migrations

- No schema change expected (rebind rewrites binding rows); if one
  proves necessary, migrate + bump `STORAGE_LAYOUT_VERSION`.

### Protocol and DTOs

- Additive update request/response + `ProjectUpdated` event; no
  negotiation change.

### Runtime and concurrency

- Daemon handler; concurrent rename + rebind serialize on the project
  id (last commit wins with revision check; no lost-update silently).

### Frontend or operator surface

- `e` form dialog on M2 row hooks; inline validation errors; success
  refreshes catalog, tab label, status line, and sessions panel.

### Security and authorization

- New `project_update` authz op; audit old/new name + root.

### Documentation and static guards

- `docs/tui.md` edit flow, refusal rule, session-follow guarantee.

## 7. Ordered work packages

### Work package A — Daemon update path

Intent: validated rename + rebind with session follow.

Required changes: catalog update, validation reuse, event, authz op.

Acceptance evidence: storage tests (rename propagation, rebind moves
bindings, sessions intact, bound-root refusal, alias-path rejection).

### Work package B — Edit form + picker key

Intent: safe, guided editing with inline errors.

Required changes: form dialog, `e` wiring, refresh of all surfaces.

Acceptance evidence: dialog tests (commit, validation error display,
cancel changes nothing); surface-refresh tests.

### Work package C — Docs and audit

Intent: operators trust rebind.

Required changes: docs, audit entries.

Acceptance evidence: audit test with old/new values.

## 8. Failure, cancellation, restart, and contention semantics

Update is transactional or fails closed; cancel/close of the form
issues no request. Concurrent updates to one project serialize with a
revision check (stale commit reports conflict, never silently
overwrites). Crash mid-commit leaves old bindings intact.

## 9. Compatibility and migration

Additive protocol; old frontends ignore the new event. No config
migration. Rebind never touches on-disk files.

## 10. Required tests

### Focused unit tests

- Catalog: rename, rebind, session-follow, refusal, alias rejection,
  revision-conflict behavior.
- Authz: update denied without grant.

### Integration tests

- Form: commit flow, inline errors, cancel-keeps-everything.
- Surfaces: tab label, status line, sessions panel refresh after
  rename/repath.

### Restart and recovery tests

- Crash/injected failure mid-commit leaves old bindings; retry clean.

### Contention and cancellation tests

- Concurrent updates to one project; cancel mid-edit.

### Security and negative tests

- Ungranted update refused; repath onto bound root refused with owning
  project named; symlink/relative input rejected.

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

- `docs/tui.md`: edit flow, what rebind does and does not do (bindings
  only, no file moves), refusal rule, session-follow guarantee.

## 13. Acceptance criteria

- `e` renames and repaths with validation; sessions follow; all
  surfaces refresh.
- Bound-root repath refused with the owning project named.
- Authz, audit, focused suites, and quick verification pass.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M2 has not closed (form/key seams differ from §3);
- rebind cannot reuse `WorkspaceRegister` validation safely;
- ADR-0033 is superseded or its rebind semantics change;
- scope would expand into lifecycle (M3) or identity (M5).

## 15. Closure evidence required

- Focused/integration/restart/contention/security suite results and
  quick verification (local vs CI labeled).
- The update-path choice (new request vs upsert) recorded with rationale.
- Manual note after reinstall + daemon restart: rename + repath flow,
  refusal hint, session continuity.
- Unblock audit: M5 readiness impact recorded.

## 16. Handoff notes

- Rebase onto the M2 closure head (and M3 head if M3 landed — the
  dialog then carries confirm flows; coordinate, do not duplicate).
- Rebind changes bindings, never files: any design that moves bytes on
  disk is out of scope — stop and report.
