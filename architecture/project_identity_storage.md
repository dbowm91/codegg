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
convergence. Binding revisions are incremented on successful rebinds and are
required by `ProjectStorage::rebind_workspace` and `rebind_session`.

## Reconciliation rules

`ProjectStorage::reconcile_workspace_path` probes only the supplied registered
workspace. Git probing is local-only, bounded, non-interactive, and performed
before the SQLite write transaction. A unique normalized remote is the only
repository lineage match. Equivalent remote spellings reuse the same
repository and project; path renames do not create new identity. Missing Git
metadata creates a valid project without a repository. Conflicting, redacted,
or insufficient evidence creates an explicit ambiguous/rebind-required
diagnostic and does not merge records.

`reconcile_catalog` processes explicit workspace records in bounded batches and
marks sessions without a resolvable workspace as `rebind_required`. It is
safe to rerun: resolved workspace bindings and resolved session bindings are
retained, and uniqueness constraints prevent duplicate authority.

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

## Verification

```text
cargo test -p codegg-core project_storage
cargo test -p codegg-core repository_lineage
cargo test -p codegg-core migration
cargo test --test storage_migrations
python3 scripts/check_identity_path_usage.py
python3 scripts/check_identity_path_usage.py --fixture scripts/fixtures/identity_path_usage/path_derived.rs  # expected rejection
```
