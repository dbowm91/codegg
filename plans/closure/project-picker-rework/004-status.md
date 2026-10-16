# Project Picker Rework Milestone 004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/project-picker-rework/004-rename-and-repath.md`
Source subsystem roadmap: `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
Repository baseline reviewed: `533b4a34`
Implementation commit: `5d60fabd` — project rename, validated workspace rebind, and picker edit form

## 1. Executive finding

M004 is closed. Pressing `e` on a project row loads its current name and
directory into an edit form. The update transaction renames the logical
project, consolidates legacy multi-workspace bindings to the selected
workspace, and moves every bound session's workspace and compatibility
directory fields with it. A bound root owned by another project is refused
with the owner's name; stale edit revisions conflict instead of overwriting
newer changes. Open tabs and their project/session surfaces receive the new
name and root. Local focused suites and `scripts/verify.sh quick` passed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Finding |
|---|---|---|
| Rename, rebind, preserve session identity and graduate multi-workspace projects to one root | `cargo test -p codegg-core --test project_catalog project_update` — 4 passed, including session follow and consolidation | Satisfied |
| Refuse a bound root with the owning project's name | Catalog conflict test asserts the owner name and verifies no rename occurred | Satisfied |
| Reject stale and concurrent updates; roll back injected failure | Catalog tests cover sequential stale token, concurrent compare-and-swap winner/loser, and injected insert failure preserving old name/binding | Satisfied |
| Absolute, non-symlink selected root | `cargo test -p codegg-core workspace::tests` — 7 passed, including relative path and symlink-root refusal | Satisfied |
| Update form copy and visible name/root fields | `cargo test -p codegg --lib tui::components::dialogs::project_picker::tests::project_edit_shows_name_root_and_binding_only_copy` — 1 passed | Satisfied |
| Authz denial and old/new audit metadata | Revoked-membership boundary test — 1 passed; `project_update_audit_records_actor_old_and_new_values` — 1 passed | Satisfied |
| Focused project, picker, and key-contract suites | `cargo test -p codegg-core -p codegg --lib project` — 617 passed; `cargo test -p codegg --lib tui::` — 1,708 passed; modifier chords — 16 passed | Satisfied |
| Server build and repository sanity | Server-feature `cargo check` passed; `scripts/verify.sh quick` passed, including workspace all-targets check and HTTP/audit/authz guards | Satisfied |

## 3. Production implementation evidence

- Added additive `ProjectUpdate` request/response/event and a dedicated
  `project_update` authorization operation. The server also exposes
  `PATCH /api/projects/{id}` under shared project authorization.
- Added transactional `ProjectCatalog::update_project` with an
  `updated_at` compare-and-swap token. Concurrent SQLite lock races retry
  and then resolve to one winner plus a conflict for stale editors.
- Rebound session project/workspace rows and compatibility directory
  fields in the same transaction as the logical project update. Rebinding
  to a workspace already owned by another project fails before mutation.
- Added the picker edit form, inline failure display, cancel/save behavior,
  and event/result projection into open tabs, labels, roots, session lists,
  and the manifest.
- Reused `WorkspaceRegistry` registration for canonical roots and aligned
  its validation with ADR-0033: roots must be absolute, and the selected
  root cannot itself be a symlink. Updated `architecture/workspace.md` to
  record the contract. No migration was needed.
- Added post-success audit metadata for actor, previous/new name and root,
  and rebound session count.

The protocol choice is a new `ProjectUpdate`, rather than an upsert through
`ProjectRegister`: registration deliberately returns an existing project
and has no revision token, so it cannot safely express edit conflicts or
atomic session rebinding.

## 4. Verification executed

Commands ran locally on implementation commit `5d60fabd`. Rust builds used
`PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig CARGO_BUILD_JOBS=2` because the
default pkg-config paths select arm64 MacPorts libraries while the active
Rust target is x86_64.

- `cargo test -p codegg-core --test project_catalog project_update` — 4 passed.
- `cargo test -p codegg-core workspace::tests` — 7 passed.
- `cargo test -p codegg-core -p codegg --lib project` — 617 passed.
- `cargo test -p codegg --lib tui::` — 1,708 passed.
- `cargo test -p codegg --test modifier_chords` — 16 passed.
- `cargo test -p codegg --test identity_m003_daemon_authorization membership_revocation_takes_effect_at_boundary` — 1 passed.
- `cargo test -p codegg --test identity_m005_audit_instrumentation project_update_audit_records_actor_old_and_new_values` — 1 passed.
- Project-edit form render test — 1 passed.
- `cargo check -p codegg --lib --features server` — passed.
- `scripts/verify.sh quick` — passed, including format, static guards, and locked workspace all-target check.
- `git diff --check` and `cargo fmt --all --check` — passed before the implementation commit.

Several initial root test builds stopped before running because concurrent
workspace builds exhausted the shared filesystem. After those processes
ended, `cargo clean -p codegg` removed only generated package artifacts
(43.7 GiB); all focused tests and quick verification then completed
successfully. Hosted CI and the manual reinstall/daemon-restart TUI
walkthrough were not run.

## 5. Failure, recovery, and compatibility review

Rename/repath keeps the old project name, workspace binding, and session
binding intact if any transactional write fails. The injected SQL trigger
test proves rollback. A monotonic millisecond revision prevents two
successful edits in the same clock millisecond from reusing a concurrency
token. Existing multi-workspace projects remain readable and are
consolidated only by an explicit operator rebind. Workspace records for
the former roots remain registered but are no longer bound to the project.
The protocol addition is backward-compatible, and storage layout is
unchanged.

## 6. Security and authority review

Core and HTTP paths require `project.configure` through `project_update`.
The update path rejects relative roots and a selected root that is itself a
symlink, canonicalizes through `WorkspaceRegistry`, and refuses a root
bound to another project. Audit data records the trusted principal and
bounded old/new values. The daemon process cwd is untouched; only client
tab execution contexts receive the new root.

## 7. Unresolved findings

None within M004 scope. Hosted CI and the interactive restart walkthrough
remain operational evidence limits.

## 8. Downstream dependency audit

- M005's hard M2–M4 closure gate is satisfied; ADR-0033 remains accepted.
  M005 is moved from blocked to ready for the next sequential step.
- No other registered project-picker milestone became eligible.
- Multi-cwd split panels remain deferred; this update changes one
  project's binding and per-tab context only.

## 9. Registry updates

- M004 moved to closed and this record was added to recently closed work.
- M005 moved to ready after its M2–M4 dependencies closed.
- The project-picker roadmap remains active until M005 closes.
