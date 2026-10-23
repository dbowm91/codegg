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

`ProjectStorageError` has five variants: `Database`, `NotFound`,
`InvalidValue`, `RevisionConflict` (carries `kind`, `expected`, `actual`),
and `Conflict`.

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

`reconcile_workspace_path` is the public probe-and-reconcile entry point;
it calls `inspect_repository_lineage` **before** any write transaction and
then delegates to `reconcile_workspace`, which retries
`reconcile_workspace_once` up to three times on a
`database is locked` error (10 ms backoff) and otherwise returns the single
result. `reconcile_workspace_once` is private and is the arm that treats a
`resolved` binding as authoritative without re-probing.

## Managed checkout binding (M006)

A checkout is a second working directory of a project, not a new project. The
auto-provisioning rule above gives every previously unseen workspace root its
own logical project, which is right for a directory the user opens and wrong
for a launch. `ProjectStorage::bind_checkout_workspace` is the explicit
alternative: the checkout's workspace joins the launch's project, shares the
launch's repository, and records the managed `WorktreeId` it belongs to.

The call is **refused** when the workspace is already bound to a *different*
project (`ProjectStorageError::Conflict`) — a checkout that belongs to
another project is not adopted by a launch; the caller must name that
project. A workspace bound with a non-`resolved` status is refused the same
way. Because the transaction reads the existing binding before writing the
row, it opens with `BEGIN IMMEDIATE` (`crate::storage::immediate::ImmediateWrite`),
not a deferred transaction.

`ProjectStorage::record_workspace_worktree` is the narrower seam that
attaches a `WorktreeId` to an already-`resolved` workspace binding and
updates `workspace_project_binding.worktree_id`.

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
`ProjectStorage` and provides list, get, register, archive, restore, update,
and delete operations on logical projects, plus locator attach/detach, health
placeholders, session/workspace listings, `mark_opened`, `count_by_lifecycle`,
and restart hydration. See `architecture/project_catalog.md`.

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
# `storage_migrations` is a member of the `core_daemon` family, not a target.
cargo test --test core_daemon -- storage_migrations
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

Verified 2026-10-23 against
`crates/codegg-core/src/project_storage.rs`,
`crates/codegg-core/src/project_catalog.rs`,
`crates/codegg-core/src/migration.rs`,
`crates/codegg-core/src/session/schema.rs`,
`crates/codegg-core/src/session/store.rs`,
`crates/codegg-protocol/src/core.rs`, and
`src/tui/commands/project_picker.rs`.

Corrections made in this pass:

- **`ProjectStorageError` variant count: 4 → 5.** The enum now also carries
  `Conflict` alongside `Database`, `NotFound`, `InvalidValue`, and
  `RevisionConflict`.
- **Added the managed-checkout binding section.** `bind_checkout_workspace`
  and `record_workspace_worktree` were missing entirely, although
  `project_storage.rs` owns them and the M006 checkout-is-a-second-working-
  directory rule is load-bearing for a launch.
- **Test target fix.** `cargo test --test storage_migrations` selects
  nothing: `storage_migrations` is a member of the root package's
  `core_daemon` family (`tests/core_daemon/storage_migrations.inc`).
  Corrected to `cargo test --test core_daemon -- storage_migrations`.
- **Added the `reconcile_workspace` / `reconcile_workspace_once` split**
  (public probe-then-reconcile, private authoritative arm with a bounded
  `database is locked` retry), which the "Directory open registration"
  section referenced by name without stating where it lives.
- **Catalog layer operation list** now names `update`/`delete` and the
  locator/health/hydration surfaces that `project_catalog.rs` actually owns.

Verified accurate, no change: all six canonical v25 tables
(`logical_project`, `repository`, `project_repository`,
`workspace_project_binding`, `session_project_binding`,
`identity_diagnostic`), the `ProjectStorage` API surface
(`reconcile_workspace_path`, `reconcile_catalog`, `rebind_workspace`,
`rebind_session`, `inspect_workspace`,
`list_diagnostics_for_workspace`), `migrate_legacy_project_database` in
`migration.rs:128` establishing the workspace binding before importing
session IDs, and the `SessionStore::create_with_binding` seam in
`session/store.rs:179`. The `ensure_bound_project_context` flow really does
issue `CoreRequest::WorkspaceRegister` then `CoreRequest::ProjectRegister`,
and the catalog's healing `UPDATE` really is guarded on
`repository_id IS NULL` (`project_catalog.rs:846`).
