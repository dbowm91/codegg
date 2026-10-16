# Project Identity Storage and Migration

Domain Identity Milestone 002 owns durable logical-project and repository
authority in `crates/codegg-core/src/project_storage.rs`.

## Canonical tables

Migration v25 is additive and leaves the historical `project` table and
string-backed session fields intact.

| Table | Authority |
|---|---|
| `logical_project` | Stable `ProjectId`, display metadata, lifecycle, timestamps |
| `repository` | Stable `RepositoryId`, VCS kind, bounded normalized lineage evidence |
| `project_repository` | Primary project-to-repository relation |
| `workspace_project_binding` | One authoritative project binding per workspace, optional repository, status, source, locator, revision |
| `session_project_binding` | One authoritative project/workspace binding per session, status, source, revision |
| `identity_diagnostic` | Redacted, reason-coded migration and rebinding evidence |

Foreign keys and primary/unique keys are the final authority for relation
convergence. New workspace bindings are one canonical root to one logical
project. Historical projects with multiple workspace bindings remain readable
until an operator uses project rebind to select one root. Binding revisions
are incremented on successful rebinds and are required by
`ProjectStorage::rebind_workspace` and `rebind_session`.

## Reconciliation rules

`ProjectStorage::reconcile_workspace_path` probes only the supplied registered
workspace. Git probing is local-only, bounded, non-interactive, and performed
before the SQLite write transaction. A unique normalized remote is the only
repository lineage match. Equivalent remote spellings reuse repository
metadata, while each unseen workspace gets a distinct logical project. Missing
Git metadata creates a valid project without a repository. Conflicting, redacted,
or insufficient evidence creates an explicit ambiguous/rebind-required
diagnostic and does not merge records.

`reconcile_catalog` processes explicit workspace records in bounded batches and
marks sessions without a resolvable workspace as `rebind_required`. It is
safe to rerun: resolved workspace bindings and resolved session bindings are
retained, and uniqueness constraints prevent duplicate authority.

## Directory open registration

A TUI launch and local picker use the shared
`ensure_bound_project_context` flow: `WorkspaceRegister`, followed by
`ProjectRegister`. The project catalog returns an existing workspace binding
on reopen and creates a directory-named project for a new root. Registration
does not use `reconcile_workspace_path` for interactive opens. The catalog
registration path applies these lineage rules:

- The local Git probe runs before any write transaction, and only unique
  usable remote evidence produces a `repository` row.
- A binding that already carries a repository is authoritative and is returned
  unchanged; nothing is created for it.
- A binding recorded with `status = 'resolved'` but a NULL repository is
  **healed**: the probed lineage is attached to the existing project, which is
  never churned. The `UPDATE` is guarded on `repository_id IS NULL`.
- Without a binding, the directory gets a new project named for its workspace.
  A clone or worktree with the same remote gets its own project while reusing
  the repository metadata.

Healing matters because `reconcile_workspace_once` deliberately treats a
`resolved` binding as authoritative and never re-probes. A binding written
without a repository would otherwise stay empty forever, and a delegated run
needing managed isolation would fail `isolation_unavailable` in a repository
with perfectly good lineage evidence.

## Legacy import

`migrate_legacy_project_database` first establishes the canonical workspace
binding, then imports source session/message IDs unchanged and writes
`session_project_binding`. The source database is never modified. Historical
`session.project_id` and `session.directory` values are copied as compatibility
projections only; they are not parsed into `ProjectId`.

## Operator inspection and rebinding

Use `ProjectStorage::inspect_workspace` to view the project/repository IDs,
workspace locator, binding status, session status counts, and diagnostics.
Use `list_diagnostics_for_workspace` for redacted reason-coded evidence. A
manual rebind must name the typed target IDs and expected revision; a stale
caller receives `ProjectStorageError::RevisionConflict`.

## Project Catalog Layer

The catalog service in `codegg_core::project_catalog` sits above
`ProjectStorage` and provides list, get, register, archive, and restore
operations on logical projects, with locator placeholders, health
placeholders, and restart hydration. See `architecture/project_catalog.md`.

## Discovery reconciliation boundary

Milestone 2 adds discovery as a caller of the existing catalog/workspace
authority. Candidates carry canonical locator observations and optional,
redacted local lineage evidence; their paths never become `ProjectId` values.
Unique lineage can preserve a project across a path move, while ambiguous
lineage, fork-like evidence, and plain-directory moves require explicit
association. Discovery never creates a second SQL authority for project or
workspace bindings.

## Verification

```text
cargo test -p codegg-core project_storage
cargo test -p codegg-core repository_lineage
cargo test -p codegg-core migration
cargo test --test storage_migrations
python3 scripts/check_identity_path_usage.py
python3 scripts/check_identity_path_usage.py --fixture scripts/fixtures/identity_path_usage/path_derived.rs  # expected rejection
```

## Context resolution and session writes

New daemon/server session writes use `SessionStore::create_with_binding`, which
commits the legacy projections and `session_project_binding` in one transaction
after validating the resolved workspace binding. `session_project_binding` and
`workspace_project_binding` remain authoritative; historical `session.project_id`
and `session.directory` values are compatibility projections.

## Source verification

Verified 2026-10-06 against
`crates/codegg-core/src/project_storage.rs`,
`crates/codegg-core/src/migration.rs`, and
`crates/codegg-core/src/session/schema.rs`. No corrections were needed —
this doc was accurate. Verified accurate: all six canonical v25 tables
(`logical_project`, `repository`, `project_repository`,
`workspace_project_binding`, `session_project_binding`,
`identity_diagnostic`), the `ProjectStorage` API surface
(`reconcile_workspace_path`, `reconcile_catalog`, `rebind_workspace`,
`rebind_session`, `inspect_workspace`,
`list_diagnostics_for_workspace`), `migrate_legacy_project_database` in
`migration.rs:128` establishing the workspace binding before importing
session IDs, the four `ProjectStorageError` variants including
`RevisionConflict`, and the `SessionStore::create_with_binding` seam. The
doc carries no numeric or line-ref claims, so nothing required
correction.
