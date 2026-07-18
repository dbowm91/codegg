# Project Catalog

The daemon-owned project catalog service sits above `ProjectStorage` and
provides list, get, register, archive, and restore operations for logical
projects. Catalog identity is stable and path-independent. Catalog listing
does not trigger expensive services.

## Ownership

`codegg_core::project_catalog` is daemon-owned, UI/server/plugin-free, and
lives in `crates/codegg-core/src/project_catalog.rs`. It is layered above
`ProjectStorage` (`crates/codegg-core/src/project_storage.rs`) and reads
from the same SQLite catalog pool.

## Public types

| Type | Summary |
|---|---|
| `Locator` | Typed reference enum: `Local` (workspace + path), `Ssh` (placeholder), `LinkedNode` (placeholder). |
| `HealthStatus` | Operator-set enum: `Unknown`, `Available`, `Unavailable`, `Unsupported`, `Stale`, `Error`. |
| `ProjectCatalogRecord` | Extended project row with description, tags, lifecycle, registration source, timestamps. |
| `ProjectHealthRecord` | Per-project health row with status, error code/message, source, and evaluation timestamp. |
| `CatalogLocatorRecord` | Stored locator row with project ID, locator kind, display summary, source, and timestamps. |
| `WorkspaceSummary` | Compact workspace reference: ID, display name, canonical root path. |
| `LifecycleCounts` | Aggregate counts by lifecycle: active, archived, total. |
| `HydrationReport` | Restart hydration output: active/total project counts, locator count, health count. |
| `LegacyAssociationReport` | Conservative legacy association output: projects associated, diagnostics recorded. |
| `RegisterLocalProject` | Input for explicit local registration: name, description, tags, primary repository. |
| `CatalogError` | Error enum: `Database`, `NotFound`, `InvalidValue`, `AlreadyExists`, `Migration`. |
| `ProjectCatalog` | Service struct: `new(pool)`, list/get/register/archive/restore, locator attach, health upsert, restart hydration, legacy association. |

## Locator kinds and inertness

Locators are inert data — they never trigger filesystem probing or remote
execution.

- **`Local`**: a workspace-scoped local path reference. The `canonical_root`
  field is a real filesystem path and the `workspace_id` must be bound to the
  project in `workspace_project_binding`.
- **`Ssh`**: an SSH placeholder with host, port, user, path, and label. The
  `path` field is never exposed in `summary()` or DTO responses. No local path
  accessor exists.
- **`LinkedNode`**: a linked-node placeholder with node ID, alias, and
  `path_hint`. No local path accessor exists.

`attach_locator` validates workspace binding only for `Local` variants. The
`Ssh` and `LinkedNode` arms extract `None` for all local path fields in the
storage tuple — there is no code path that calls `.canonical_root` or
`.as_path()` on remote locators.

## Health placeholder model

`HealthStatus` is operator-set, not probed. The catalog never calls
filesystem, Git, LSP, or provider APIs to compute health. Health rows
are upserted by callers who already know the status (e.g., daemon startup
probes, manual diagnostics). The catalog only reads, stores, and returns
the operator-provided value.

## Archive/restore semantics

Archive is logical and non-destructive. `archive_project` sets `lifecycle`
to `Archived` and writes `archived_at`. `restore_project` clears those
fields. The catalog never deletes:

- workspace records
- repository records
- session records
- locator rows
- health rows
- any files on disk

## Restart hydration contract

`restart_hydration()` reads only aggregate counts from the catalog tables.
It performs no filesystem probing, no Git scanning, no LSP initialization,
and no provider API calls. The daemon calls this at startup to repopulate a
small in-memory index of active project counts, locator counts, and health
counts.

## Conservative legacy association

`legacy_association()` uses `repository_lineage` to associate unambiguous
workspaces to canonical projects. Ambiguous cases record diagnostics in
`identity_diagnostic` without merging. The operation is idempotent: the
`legacy_catalog_association_marker` table records which sources have already
been processed and skips re-runs.

## Schema and migration

Schema v28 is additive. It creates:

- `project_locator` table (locator kinds, workspace/SSH/node fields, display
  summary, source, timestamps)
- `project_health` table (project-keyed status, error fields, source,
  evaluation timestamp)
- `legacy_catalog_association_marker` table (source, completion timestamp,
  counts)

It also adds five columns to `logical_project`: `archived_at`,
`description`, `tags`, `registration_source`, `time_last_opened`.

Idempotent re-runs accept `duplicate column name` errors for `ALTER TABLE`
statements and use `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT
EXISTS` for new tables and indexes.

## Invariants

1. Catalog identity is stable and path-independent — `ProjectId` is never
   derived from a filesystem path.
2. Archive is logical and never deletes workspaces, repositories, sessions,
   locators, or files.
3. Listing catalog records performs no LSP, Git, indexer, provider probe,
   or build initialization.
4. Remote locators (`Ssh`, `LinkedNode`) are inert data; no local-path
   coercion exists.

## Integration points

- `architecture/project_identity_storage.md` — ProjectStorage layer above
  which the catalog sits.
- `architecture/identity.md` — Typed identity foundation providing
  `ProjectId`, `RepositoryId`, `WorkspaceId`.
- `architecture/workspace.md` — Workspace registry, execution context, and
  path policy.
- `architecture/storage.md` — SQLite storage layer and migration index.

## Source documents

- `plans/implementation/project-catalog/001-durable-catalog-foundation.md`
  — Implementation plan for the durable catalog foundation.
- `plans/closure/project-catalog/001-status.md` — Closure record (link to
  be added when available).

## Static guard

```bash
python3 scripts/check_project_catalog_invariants.py --verbose
```

Verifies module existence, locator safety invariants, migration schema,
storage layout version, and lib.rs re-export.

## Bounded discovery (Milestone 2)

`codegg_core::project_discovery` is the core-only discovery boundary. It accepts
only explicitly configured local roots and produces bounded metadata candidates;
it does not activate workspace services, run LSP/index/build/provider work, or
write inside candidate repositories. The scanner is deterministic, does not
follow symlinks by default, skips heavy directories, and stops at finite depth,
entry, candidate, elapsed-time, diagnostic, and Git-probe limits.

Discovery configuration is opt-in under `discovery` in the config schema. Safe
defaults are disabled, depth 4, 10,000 visited entries, 1,000 candidates,
10 seconds, stat concurrency 4, and Git-probe concurrency 2. Reconciliation
uses exact/canonical workspace evidence first, then a unique local Git lineage
key. Remote-only, fork-like, ambiguous, and plain-directory move evidence
remains unresolved rather than being merged by path or remote URL. Missing
candidates and unavailable roots update observations only; they do not archive
or delete catalog authority. Scan generations are persisted in schema v29.

## See Also

- [`architecture/project_identity_storage.md`](project_identity_storage.md)
- [`architecture/identity.md`](identity.md)
- [`architecture/workspace.md`](workspace.md)
- [`architecture/storage.md`](storage.md)
