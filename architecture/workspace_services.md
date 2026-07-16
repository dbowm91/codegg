# Workspace Services and Storage (Phase 3)

This module describes Phase 3 of the single-daemon multi-project
orchestration roadmap. Phase 3 turns each registered workspace into a
**daemon-owned service domain**: every workspace has a bundle of services
that the daemon activates on demand, leases to consumers (tools, TUI,
servers) via a single-flight registry, and tears down under a policy.

It also establishes the **user-scoped daemon catalog** as the
authoritative storage layer for daemon-owned state, and ships tooling to
import legacy project-local session databases into that catalog.

The implementation lives in:

- `crates/codegg-core/src/workspace_services.rs` — bundle, lease, registry,
  factory, lock table, policy, config snapshot, reports.
- `crates/codegg-core/src/migration.rs` — idempotent
  legacy-project-database importer.
- `crates/codegg-core/src/storage/mod.rs` and `storage/paths.rs` —
  user-scoped catalog, legacy project store, layout marker.

## Why it exists

Phase 2 (`architecture/workspace.md`) established the workspace
identity and the immutable `ExecutionContext` that flows through every
daemon execution path. But the resources that the daemon attaches to a
workspace — the `RunStore`, the path policy, the per-repository lock
table, the resolved configuration snapshot — were still constructed
**ad-hoc** by every consumer. Two callers hitting the same workspace
could end up with two different `FsRunStore` instances pointing at the
same directory, racing on each other's index cache. The TUI and the
server each built their own.

Phase 3 centralizes that construction inside a per-workspace bundle.
The bundle is keyed by `WorkspaceId`, lazily activated on first
acquisition, and shared across every consumer through an `Arc`. The
registry enforces single-flight activation (only one bundle is
constructed per workspace even under concurrent first acquisition) and a
configurable cap on the number of simultaneously-active workspaces.

The storage refactor in the same phase moves the daemon's authoritative
SQLite store out of `<workspace>/.codegg/sessions.db` and into a single
**user-scoped** location:

- macOS: `~/Library/Application Support/codegg/codegg.db`
- Linux: `$XDG_DATA_HOME/codegg/codegg.db` (or `~/.local/share/`)

The legacy project-local file is retained as a backward-compat surface
and as the source for the migration tooling.

## Key types

### Bundle and lease

- `WorkspaceServices` — the per-workspace bundle. Owns the
  `Arc<dyn RunStore>`, the `Arc<WorkspacePathPolicy>`, the
  `Arc<WorkspaceLockTable>`, the `Arc<WorkspaceConfigSnapshot>`, and the
  bookkeeping counters (`activated_at`, `last_used_at`,
  `active_leases`).
- `WorkspaceServicesLease` — RAII handle returned by
  `WorkspaceServiceRegistry::acquire`. On drop the registry decrements
  the bundle's active-lease counter, marking it eligible for idle
  eviction. Accessors expose the bundle's services, the workspace id,
  the artifact root path, etc.
- `WorkspacePathPolicy::for_workspace` — minimal path policy mirroring
  `ExecutionContext`'s allowed roots. Future phases add symlink policy,
  sandbox mode, and platform capability fields.

### Registry and policy

- `WorkspaceServicePolicy` — `max_active_workspaces` (default 16) and
  `idle_evict_after` (default 30 minutes).
- `WorkspaceServiceRegistry` — the daemon-owned registry of active
  bundles. Backed by `DashMap<WorkspaceId, Arc<WorkspaceServices>>` and
  `DashMap<WorkspaceId, Arc<AsyncMutex<()>>>` for single-flight
  activation. Exposes:
  - `acquire(workspace_id) -> WorkspaceServicesLease` — lazy activation
    with lease accounting.
  - `activate(workspace_id) -> Arc<WorkspaceServices>` — lazy activation
    without lease (for inspection / configuration reload).
  - `peek(workspace_id) -> Option<WorkspaceServiceSnapshot>` — snapshot
    of the bundle's state.
  - `list_active() -> Vec<WorkspaceServiceSnapshot>`.
  - `reload_config(workspace_id) -> ReloadResult` — bumps the bundle's
    `WorkspaceConfigSnapshot.revision` and returns any diagnostics.
  - `evict_idle(now) -> EvictionReport` — evicts bundles whose
    `last_used_at` is past `idle_evict_after` AND whose lease count is
    zero. Returns the workspaces it considered, skipped (because they
    have active leases), and evicted.
  - `shutdown_all(deadline) -> ShutdownReport` — drains every active
    bundle, force-terminating those with outstanding leases past the
    deadline. Returns the drained and force-terminated workspace ids
    plus a `deadline_hit` flag.

### Factory

- `WorkspaceServicesFactory` — trait with a single
  `build(workspace: Arc<WorkspaceRecord>) -> Result<Arc<WorkspaceServices>, String>`
  method. Production callers use `ProductionWorkspaceServicesFactory`,
  which wires `FsRunStore` at `<workspace>/.codegg/runs/`, the canonical
  `WorkspacePathPolicy`, a fresh `WorkspaceLockTable`, and a default
  `WorkspaceConfigSnapshot { revision: 0 }`. Tests inject a counting
  factory that observes activation count and identity.

### Lock table

- `WorkspaceLockTable` — `DashMap<PathBuf, Arc<AsyncMutex<()>>>`
  keyed by canonical repository root. `acquire_repository(repo_root)
  -> WorkspaceRepositoryGuard` returns an RAII guard that holds the
  per-repository lock for the duration of the critical section. The
  Phase F Git service and the Bash-translation dispatcher both call
  `acquire_repository` with the canonical repository root so they
  contend on the same lock instead of racing each other. The
  `WorkspaceRepositoryGuard` is an owned `tokio::sync::MutexGuard`,
  which is `Send`-safe across awaits.

### Configuration snapshot

- `WorkspaceConfigSnapshot` — `{ revision: u64, loaded_at, source_files,
  diagnostics }`. The `revision` field is bumped by `reload_config` so
  observers can detect that a configuration change happened.
- `ConfigDiagnostic` / `ConfigDiagnosticSeverity` — severity-classified
  diagnostics (Warning / Error) emitted while loading or reloading
  workspace configuration. Surfaced through the
  `WorkspaceServicesReload` response so the TUI and server can render
  them.

### Reports

- `EvictionReport { evicted, skipped_active, evaluated }` — returned by
  `evict_idle`.
- `ShutdownReport { drained, force_terminated, deadline_hit }` —
  returned by `shutdown_all`.
- `ReloadResult { workspace_id, previous_revision, new_revision, diagnostics }`
  — returned by `reload_config`.

## Storage ownership (Phase 3)

`crates/codegg-core/src/storage/mod.rs` splits the SQLite storage layer
into two entry points:

| Entry point | Location | Purpose |
|-------------|----------|---------|
| `init_daemon_catalog(&DaemonPaths)` | user-scoped, e.g. `~/Library/Application Support/codegg/codegg.db` | Workspace records, session catalog and messages, notification history, durable jobs (Phase 4+), daemon-global metadata. |
| `init_legacy_project_store(project_root)` | `<root>/.codegg/sessions.db` | Backward compat with pre-Phase 3 installs; source for migration. |
| `init_pool_at(db_path)` | arbitrary | Test-friendly escape hatch used by integration tests. |
| `init(project_dir)` *(deprecated)* | ambiguous | Routes to one of the above based on whether `project_dir` is empty or a real directory. New code MUST NOT use this. |

`STORAGE_LAYOUT_VERSION` is now `24` and is referenced from
`MigrationMarker.storage_layout_version` so the migration tooling can
report which layout a legacy database was imported under.

`DaemonPaths` is the single source of truth for catalog and asset paths:

- `DaemonPaths::default()` — platform-default data root.
- `DaemonPaths::with_overrides(data_root, config_root)` — explicit
  overrides for tests and tooling.
- Accessors: `data_root()`, `config_root()`, `catalog_db_path()`,
  `catalog_db_wal_path()`, `agents_dir()`, `credentials_path()`,
  `workspace_local_artifact_root(workspace_root)`.

## Migration tooling

`crates/codegg-core/src/migration.rs` provides:

- `find_legacy_project_db(project_root) -> Option<PathBuf>` — locates the
  legacy database at `<project_root>/.codegg/sessions.db` if present.
- `verify_session_schema(pool) -> Result<bool, _>` — confirms the source
  pool has a recognizable `session` table.
- `ensure_migration_marker_table(pool)` — creates the catalog's
  `migration_marker` table idempotently.
- `fetch_marker(pool, source_path) -> Option<MigrationMarker>`.
- `list_migration_markers(pool) -> Vec<MigrationMarker>`.
- `migrate_legacy_project_database(catalog_pool, registry, project_root)
  -> MigrationOutcome` — the canonical entry point. Opens the source DB,
  registers a workspace for the project root, imports sessions, writes
  a marker row, and returns one of:
  - `SourceMissing` — no `<root>/.codegg/sessions.db` exists.
  - `InvalidSchema(source_path)` — source DB does not look like a
    codegg session store.
  - `Imported { workspace_id, sessions, messages }` — first successful
    import.
  - `AlreadyMigrated` — the source path already has a marker row in the
    catalog; the function is a no-op and returns the cached outcome.
- `MigrationMarker { source_path, imported_at, sessions_count,
  messages_count, storage_layout_version }` — provenance record written
  after a successful import.

The source database is **never modified**; only the catalog gains the
workspace record, the imported sessions, and the marker row.

## Protocol surface

`crates/codegg-protocol/src/core.rs` and `dto.rs` gain the following
variants:

- `CoreRequest::WorkspaceServicesSnapshot` — returns every active
  workspace bundle snapshot.
- `CoreRequest::WorkspaceConfigReload { workspace_id }` — bumps the
  workspace's config revision and returns the diagnostics.
- `CoreRequest::RunList { workspace_id, query: RunQueryDto }` — lists
  runs visible to a workspace.
- `CoreRequest::RunGet { workspace_id, run_id }` — fetches a single run.
- `CoreRequest::RunArtifactRead { workspace_id, artifact_id, start,
  end }` — reads a base64-encoded chunk from a run artifact.
- `CoreResponse::WorkspaceServicesSnapshot { services:
  Vec<WorkspaceServiceHealthDto> }`.
- `CoreResponse::WorkspaceConfigReload { workspace_id, previous_revision,
  new_revision, diagnostics: Vec<ConfigDiagnosticDto> }`.
- `CoreResponse::RunList { runs: Vec<RunSummaryDto> }`.
- `CoreResponse::RunGet { run: Option<RunRecordDto> }`.
- `CoreResponse::RunArtifactChunk { data_b64, byte_offset, total_bytes }`.

`WorkspaceSnapshot` (Phase 2) gains:

- `services_active: bool` — whether a bundle is currently active.
- `active_leases: usize` — count of leases currently held on the
  bundle.
- `config_revision: u64` — current configuration revision.

DTOs:

- `WorkspaceServiceHealthDto { workspace_id, revision,
  last_used_at_unix, active_leases }`.
- `ConfigDiagnosticDto { severity, source, message }`.
- `RunQueryDto { kind: Option<RunKind>, limit: usize,
  workspace_id: Option<String> }`.
- `RunSummaryDto`, `RunRecordDto`, `RunArtifactSummaryDto` — minimal
  projections of `RunManifest`, `RunRecord`, and `ArtifactRef`.

## Propagation

`CoreRuntimeDeps` (`src/core/runtime_deps.rs`) gains two new fields:

- `workspace_services: Option<Arc<WorkspaceServiceRegistry>>` — when
  `None`, `CoreDaemon::with_deps_and_identity` constructs the default
  registry with `ProductionWorkspaceServicesFactory` and the policy
  from `WorkspaceServicePolicy::default()`. Tests can inject a
  registry directly via `with_workspace_services`.
- `workspace_service_policy: WorkspaceServicePolicy` — the policy the
  daemon's default-constructed registry uses. Tests override via
  `with_workspace_service_policy`.

`CoreDaemon::handle_request` arms for the new requests dispatch through
`workspace_services`. The `WorkspaceList` response uses
`workspace_record_with_services_to_dto` to populate
`services_active` and `active_leases` from `peek` snapshots.

## Static guard

`scripts/check-core-boundary.sh` continues to enforce the codegg-core
boundary. The new `workspace_services` and `migration` modules do not
introduce any UI-, server-, plugin-, or auth-crate dependencies. The
`scripts/check_daemon_cwd_usage.py` static guard continues to forbid
`std::env::current_dir()` in protected modules.

## Tests

- `tests/workspace_services_isolation.rs` — eleven tests covering:
  - Two-workspace bundle isolation (different `RunStore` and lock-table
    instances, separate activations).
  - Concurrent acquire producing exactly one bundle despite N racers.
  - Lease drop decrementing the bundle's `active_leases` counter.
  - `evict_idle` skipping bundles with active leases.
  - `peek` / `acquire` returning `NotFound` for unknown workspaces.
  - `WorkspaceLockTable::acquire_repository` serializing concurrent
    callers.
  - `reload_config` bumping the snapshot revision while preserving
    subsequent leases.
  - `shutdown_all` force-terminating bundles with outstanding leases
    and draining idle bundles.
  - `ProductionWorkspaceServicesFactory` creating the `.codegg/`
    parent of the artifact root.
  - `DaemonPaths::default()` resolving the user-scoped catalog path.
  - `migrate_legacy_project_database` importing a legacy project DB,
    writing a marker row, and reporting the layout version.
- `crates/codegg-core/src/workspace_services.rs` inline tests — cover
  `acquire`, `activate` (existing + new bundle paths), `reload_config`,
  `shutdown_all` (with and without active leases).
- `crates/codegg-core/src/migration.rs` inline tests — cover
  `source_missing_is_reported`, `invalid_schema_is_rejected`, and
  `idempotent_marker_is_recorded`.

Run the narrowest scope that covers your change:

```bash
cargo test --test workspace_services_isolation
cargo test -p codegg-core workspace_services
cargo test -p codegg-core migration
python3 scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
```

## See Also

- [`architecture/workspace.md`](workspace.md) — Phase 2 workspace
  identity, `WorkspaceRegistry`, `ExecutionContext`, and path policy.
- [`architecture/storage.md`](storage.md) — Storage layout and migration
  index (now `STORAGE_LAYOUT_VERSION = 24`).
- [`architecture/run_store.md`](run_store.md) — `RunStore` and
  `RunManifest` semantics used by the bundle.
- [`architecture/protocol.md`](protocol.md) — Phase 3 protocol variants
  and DTO additions.
- [`architecture/core.md`](core.md) — `CoreDaemon` and `CoreRuntimeDeps`
  wiring (workspace_services section).
- `architecture/workspace_services.md`
  — Full Phase 3 contract.