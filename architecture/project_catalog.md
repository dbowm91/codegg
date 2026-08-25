# Project Catalog

The daemon-owned project catalog service sits above `ProjectStorage`
and provides list, get, register, archive, and restore operations for
logical projects. Catalog identity is stable and path-independent.
Catalog listing does not trigger expensive services.

## Purpose

Provide a durable catalog of logical projects and repositories with
archive/restore, lifecycle/health/locator placeholders, explicit
local registration, conservative legacy association, restart
hydration, and bounded project discovery. The catalog is read/list/manage
from internal/diagnostic surfaces only.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-core/src/project_catalog.rs` | Catalog service (list/get/register/archive/restore, locators, health, hydration, legacy association) |
| `crates/codegg-core/src/project_storage.rs` | `ProjectStorage` layer above which the catalog sits |
| `crates/codegg-core/src/project_discovery.rs` | Bounded metadata-only scanner (deterministic, no I/O) |
| `crates/codegg-core/src/project_discovery_service.rs` | Daemon coordinator for discovery (persistence, reconciliation) |

## How It Works

### Catalog Service

`ProjectCatalog` wraps a `SqlitePool` and provides:

- `list_projects(include_archived)` — list all or active-only.
- `get_project(project_id)` — single project by ID.
- `get_project_with_health(project_id)` — project + health record.
- `register_local_project(input, workspace_id, source)` — explicit
  local registration. Creates a new `ProjectId` and binds the
  workspace, or returns an existing project if the workspace is
  already bound.
- `archive_project(project_id, source)` — logical archive (sets
  `lifecycle = 'archived'`, writes `archived_at`). Retries up to 3
  times on SQLite `database is locked`.
- `restore_project(project_id, source)` — clears archive fields.
- `list_workspaces_for_project(project_id)` — bound workspaces.
- `list_sessions_for_project(project_id)` — session count.
- `list_locators(project_id)` — all locators for a project.
- `attach_locator(project_id, locator, source)` — add a locator.
- `detach_locator(locator_id)` — remove a locator.
- `set_health(project_id, status, source)` — upsert health.
- `get_health(project_id)` — read health record.
- `mark_opened(project_id)` — update `time_last_opened`.
- `count_by_lifecycle()` — active/archived/total counts.
- `restart_hydration()` — aggregate counts at startup.

### Locator Kinds

Locators are inert data — they never trigger filesystem probing or
remote execution.

| Kind | Fields | Summary format |
|------|--------|----------------|
| `Local` | `workspace_id`, `canonical_root` | `local:/path/to/root` |
| `Ssh` | `host`, `port`, `user`, `path`, `label` | `ssh:user@host:port (label)` |
| `LinkedNode` | `node_id`, `alias`, `path_hint` | `node:id (alias) [hint]` |

`attach_locator` validates workspace binding only for `Local`
variants. `Ssh` and `LinkedNode` arms extract `None` for all local
path fields — no code path calls `.canonical_root` or `.as_path()`
on remote locators.

`Locator::validate()` enforces:
- Non-empty, bounded fields (max `MAX_LOCATOR_FIELD_LENGTH = 512`).
- No control characters.
- SSH port must be non-zero.
- SSH path must not contain a URL with embedded credentials.

### Health Placeholder Model

`HealthStatus` is operator-set, not probed. The catalog never calls
filesystem, Git, LSP, or provider APIs to compute health. Health
rows are upserted by callers who already know the status.

Variants: `Unknown`, `Available`, `Unavailable`, `Unsupported`,
`Stale`, `Error`.

### Archive/Restore Semantics

Archive is logical and non-destructive. `archive_project` sets
`lifecycle` to `Archived` and writes `archived_at`. `restore_project`
clears those fields. The catalog never deletes workspaces,
repositories, sessions, locators, health rows, or files on disk.

Archive retries up to 3 times on `database is locked` errors
(retrying after 10 ms). The project may already be archived on
entry; the method is idempotent.

### Restart Hydration

`restart_hydration()` reads only aggregate counts from catalog tables.
No filesystem probing, Git scanning, LSP initialization, or provider
API calls. Returns `HydrationReport` with `active_project_count`,
`total_project_count`, `locator_count`, `health_count`.

### Conservative Legacy Association

`conservative_legacy_association()` uses `repository_lineage` to
associate unambiguous workspaces to canonical projects. Ambiguous
cases record diagnostics in `identity_diagnostic` without merging.
The operation is idempotent: the `legacy_catalog_association_marker`
table records which sources have already been processed.

### Bounded Discovery (M2)

`project_discovery` is the core-only discovery boundary. It accepts
only explicitly configured local roots and produces bounded metadata
candidates. It does not activate workspace services, run LSP/index/
build/provider work, or write inside candidate repositories.

The scanner is deterministic, does not follow symlinks by default,
skips heavy directories, and stops at finite depth, entry, candidate,
elapsed-time, diagnostic, and Git-probe limits.

Default limits: depth 4, 10,000 visited entries, 1,000 candidates,
5 seconds, stat concurrency 4, Git-probe concurrency 2.

Reconciliation uses exact/canonical workspace evidence first, then
unique local Git lineage key. Remote-only, fork-like, ambiguous, and
plain-directory move evidence remains unresolved.

### Discovery Coordinator (M2)

`DiscoveryCoordinator` is the daemon-owned coordinator. It:

- Persists scan metadata in `discovery_root`, `discovery_scan`, and
  `discovery_observation` tables.
- Deduplicates concurrent scans of the same root (followers await
  the leader's result).
- Runs scans via `tokio::task::spawn_blocking` with cancellation.
- Reconciles candidates against `KnownProject` records via
  `reconcile_candidate()`.
- Prunes old scan generations (retention = 20).

`DiscoveryRootRecord` converts from `DiscoveryRootConfig` in the
config schema. `roots_from_config()` validates all roots before the
coordinator starts.

### Lazy Activation and Health (M3)

Project activation is explicit and scoped by `(ProjectId, WorkspaceId)`
plus a bounded owner identifier. `CoreDaemon::activate_project_workspace`
resolves the typed catalog binding, then acquires an owner-scoped
`ProjectActivationLease`. The lease has a finite lifetime and releases
the underlying `WorkspaceServicesLease` on drop, explicit release,
or bounded expiry eviction.

Activation never creates a second service authority. Workspace bundle
construction and same-workspace single-flight remain owned by
`WorkspaceServiceRegistry`.

`CoreDaemon::project_health` is a read-only, bounded aggregate of
catalog, workspace, runtime-asset, and service state. Health output
contains only typed IDs, layer states, bounded codes/messages, and
bounded diagnostics.

### Protocol-Facing Identity Boundary (M4)

Catalog-backed routes return the durable logical `ProjectId` in
`ProjectInfo.id`. The local path is a separately named compatibility
locator. Catalog and session counts are read from canonical
bindings. Local registration first registers the workspace locator
before delegating to `ProjectCatalog::register_local_project`.

## Key Types & APIs

| Type | File:line | Purpose |
|------|-----------|---------|
| `ProjectCatalog` | `project_catalog.rs:432` | Service struct |
| `ProjectCatalogRecord` | `project_catalog.rs:291` | Extended project row |
| `Locator` | `project_catalog.rs:67` | Typed reference enum (Local/Ssh/LinkedNode) |
| `CatalogLocatorRecord` | `project_catalog.rs:89` | Stored locator row |
| `HealthStatus` | `project_catalog.rs:238` | Operator-set health enum |
| `ProjectHealthRecord` | `project_catalog.rs:276` | Per-project health row |
| `WorkspaceSummary` | `project_catalog.rs:317` | Compact workspace ref |
| `LifecycleCounts` | `project_catalog.rs:329` | Active/archived/total |
| `HydrationReport` | `project_catalog.rs:342` | Restart hydration output |
| `LegacyAssociationReport` | `project_catalog.rs:355` | Legacy association output |
| `RegisterLocalProject` | `project_catalog.rs:377` | Registration input |
| `CatalogError` | `project_catalog.rs:406` | Error enum (Database/NotFound/InvalidValue/Conflict/AlreadyExists) |
| `Scanner` | `project_discovery.rs:406` | Reusable bounded scanner |
| `DiscoveryRoot` | `project_discovery.rs:99` | Explicit local root config |
| `ScanLimits` | `project_discovery.rs:144` | Finite work limits |
| `DiscoveryCandidate` | `project_discovery.rs:231` | One metadata-only candidate |
| `ReconciliationOutcome` | `project_discovery.rs:348` | Pure reconciliation decision |
| `DiscoveryCoordinator` | `project_discovery_service.rs:191` | Daemon coordinator |

## Configuration Surface

Discovery is opt-in under `discovery` in the config schema. Key
fields per root: `id`, `path`, `mode` (Git/Directory/Mixed),
`max_depth`, `max_visited_entries`, `max_candidates`,
`max_elapsed_ms`, `ignore`, `directory_markers`, `direct_child_only`,
`include_hidden`, `stat_concurrency`, `git_probe_concurrency`.

Safe defaults: disabled, depth 4, 10,000 entries, 1,000 candidates,
5 seconds.

## Invariants & Gotchas

1. **Path-independent identity**: `ProjectId` is never derived from
   a filesystem path.

2. **Archive is non-destructive**: Never deletes workspaces,
   repositories, sessions, locators, or files.

3. **Listing is probe-free**: No LSP, Git, indexer, provider, or
   build initialization on catalog reads.

4. **Remote locators are inert**: `Ssh` and `LinkedNode` have no
   local-path coercion.

5. **Discovery never writes below roots**: The scanner is
   read-only. The coordinator delegates workspace/project authority
   to `ProjectStorage`.

6. **Symlinks not followed**: Discovery never follows symlinks.
   Symlinked entries produce a diagnostic.

7. **Reconciliation is deterministic**: `reconcile_candidate()` is
   pure. Fork conflicts and ambiguous lineages remain unresolved.

## Testing

```bash
# Catalog unit tests
cargo test -p codegg-core -- project_catalog

# Discovery unit tests
cargo test -p codegg-core -- project_discovery

# Discovery service integration tests
cargo test -p codegg-core -- project_discovery_service

# Adversarial tests
cargo test --test command_routing_adversarial
```

## Static Guards

```bash
python3 scripts/check_project_catalog_invariants.py --verbose
python3 scripts/check_discovery_invariants.py
```

## Schema and Migration

Schema v28 is additive. It creates:

- `project_locator` table (locator kinds, workspace/SSH/node fields,
  display summary, source, timestamps).
- `project_health` table (project-keyed status, error fields, source,
  evaluation timestamp).
- `legacy_catalog_association_marker` table (source, completion
  timestamp, counts).

Adds five columns to `logical_project`: `archived_at`, `description`,
`tags`, `registration_source`, `time_last_opened`.

Scan generations persisted in schema v29 (`discovery_root`,
`discovery_scan`, `discovery_observation` tables).

Idempotent re-runs accept `duplicate column name` errors for
`ALTER TABLE` statements and use `CREATE TABLE IF NOT EXISTS` /
`CREATE INDEX IF NOT EXISTS` for new tables and indexes.

## Related Docs

- `architecture/project_identity_storage.md` — ProjectStorage layer
- `architecture/identity.md` — Typed identity foundation
- `architecture/workspace.md` — Workspace registry
- `architecture/storage.md` — SQLite storage layer
