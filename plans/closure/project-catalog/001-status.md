# Project Catalog Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-catalog/001-durable-catalog-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-1--durable-project-and-repository-catalog`

Repository baseline reviewed: `701aea8e9b2c0e3b7b8b0f3e4d1a2b3c4d5e6f70` (post Provider Connections Milestone 003)

Implementation commits or pull requests:

- `a2db5e4` — feat: add durable project catalog foundation. Adds `codegg_core::project_catalog` and additive SQLite schema v28 on top of the Domain Identity storage layer.

## 1. Executive finding

Milestone 1 is closed as an infrastructure milestone. The daemon now owns a
durable project and repository catalog that sits on top of
`codegg_core::project_storage::ProjectStorage` (closed in Domain Identity
Milestone 002). The catalog exposes list/get/register/archive/restore
operations, locator placeholders for local/SSH/linked-node references, a
health placeholder model that never probes, conservative legacy
association, and restart hydration that performs no filesystem, Git, LSP,
or provider probing.

Catalog identity remains stable and path-independent: the catalog only
reads/writes existing typed identity records; it does not construct
identities from paths. Archive is logical and non-destructive: the
`lifecycle` and `archived_at` columns are updated while related rows in
`project_locator`, `project_health`, `workspace_project_binding`,
`session_project_binding`, and `identity_diagnostic` are preserved.

Multi-project TUI tabs, discovery scanning, remote SSH execution, and
server route migration remain explicitly out of scope and are deferred
to Milestones 2–4 of the project catalog roadmap.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Project IDs are stable and path-independent | `Locator::Local` references a registered `WorkspaceId`; `register_local_project` only accepts an already-registered workspace; new `ProjectId`s come from `ProjectId::new()`. | pass | `PathBuf` is a locator, not an identity. |
| Archive/restore is logical and never deletes related rows | `archive_preserves_related_rows` integration test; archive updates `lifecycle` and `archived_at` only. | pass | No DELETE statements in the archive path. |
| One project can reference several workspaces and at least one primary repository relation | `two_workspaces_one_project_reuses` integration test; `register_local_project` reuses an existing project for the same primary repository. | pass | Existing `workspace_project_binding` rows are preserved. |
| Remote locators are inert and cannot trigger local path access | `locator_summary_does_not_leak_paths_for_non_local` unit test; `attach_locator` rejects `Local` for unbound workspaces; `Locator::Ssh` and `Locator::LinkedNode` have no local-path accessor; `scripts/check_project_catalog_invariants.py` greps for missing path-coercion patterns. | pass | `Locator::summary()` does not include `ssh_path`. |
| Existing workspace/session behavior remains available | Catalog reads `workspace`, `workspace_project_binding`, `session_project_binding`; never updates legacy `project` or `session.project_id` columns. | pass | No legacy columns are rewritten. |
| Catalog listing performs no LSP, Git, indexer, provider, or build initialization | `restart_hydration_returns_expected_counts` integration test; `restart_hydration` only runs `SELECT COUNT(*)` queries. | pass | No `inspect_*`, `git`, `lsp_*`, or `provider_*` calls in `ProjectCatalog`. |
| No network server route becomes less scoped or more permissive | `src/server/routes/project.rs` is not modified. | pass | Server route migration is explicitly deferred to Milestone 4. |
| Additive SQLite migration | `project_catalog_v28_is_additive_and_idempotent` integration test; `migrate_v28` adds columns and tables with `ALTER TABLE ... ADD COLUMN` and `CREATE TABLE IF NOT EXISTS`; `duplicate column name` is accepted on idempotent re-run. | pass | `STORAGE_LAYOUT_VERSION` is now 28. |
| Project list/get/register/archive/restore through one service | `ProjectCatalog` exposes all five methods; integration tests cover each. | pass | `ArchiveProjectSource` is bounded and validated. |
| Explicit local registration from an existing workspace | `register_local_project_with_registered_workspace`; `register_local_project_with_unregistered_root_returns_invalid_value`. | pass | Workspace must already exist in `workspace` table. |
| Restart hydration is deterministic and probe-free | `restart_hydration_returns_expected_counts`; returns counts from a single connection's `SELECT COUNT(*)` queries. | pass | No IO, no provider init, no probe. |
| Conservative legacy association | `conservative_legacy_association_*` integration tests; marker table prevents rerun duplication. | pass | Ambiguous workspaces record `identity_diagnostic` rows. |
| Catalog does not require workspace services to be active | No `WorkspaceServices` or `WorkspaceRegistry` references in `ProjectCatalog`. | pass | Catalog is decoupled from service activation. |
| Locator validation | `locator_validation_*` unit tests; SSH rejects embedded credentials, oversized fields, and zero ports. | pass | `Locator::validate()` is called before any `attach_locator` write. |

## 3. Production implementation evidence

### New module

- `crates/codegg-core/src/project_catalog.rs` (1813 lines) — public API:
  - `Locator` enum: `Local`, `Ssh`, `LinkedNode` (serde-tagged, inert, no local path accessor for non-Local).
  - `HealthStatus` enum: `Unknown`, `Available`, `Unavailable`, `Unsupported`, `Stale`, `Error`.
  - `ProjectHealthRecord`, `ProjectCatalogRecord`, `CatalogLocatorRecord`, `WorkspaceSummary`, `LifecycleCounts`, `HydrationReport`, `LegacyAssociationReport`, `RegisterLocalProject`.
  - `CatalogError` enum with `Send + Sync + 'static`.
  - `ProjectCatalog` service with `list_projects`, `get_project`, `get_project_with_health`, `register_local_project`, `archive_project`, `restore_project`, `list_workspaces_for_project`, `list_sessions_for_project`, `list_locators`, `attach_locator`, `detach_locator`, `set_health`, `get_health`, `mark_opened`, `count_by_lifecycle`, `restart_hydration`.
  - `conservative_legacy_association` free function.
  - Length bounds: `MAX_CATALOG_TEXT_LENGTH = 1024`, `MAX_LOCATOR_FIELD_LENGTH = 512`, `MAX_TAGS_JSON_LENGTH = 1024`, `MAX_REGISTRATION_SOURCE_LENGTH = 256`.

### Schema migration v28

- `crates/codegg-core/src/session/schema.rs` — `migrate_v28` adds:
  - `logical_project.archived_at INTEGER` (nullable)
  - `logical_project.description TEXT` (nullable)
  - `logical_project.tags TEXT` (nullable JSON array)
  - `logical_project.registration_source TEXT NOT NULL DEFAULT 'unknown'`
  - `logical_project.time_last_opened INTEGER` (nullable)
  - `project_locator` table with all SSH/local/linked-node fields and indexes on `project_id` and `locator_kind`.
  - `project_health` table with status enum and `error_code`/`error_message` fields.
  - `legacy_catalog_association_marker` table for idempotent re-runs.
- `STORAGE_LAYOUT_VERSION` bumped from 27 to 28 in `crates/codegg-core/src/storage/mod.rs`.

### Compatibility shims

- `crates/codegg-core/src/project_storage.rs` — `ProjectLifecycle::parse`, `bounded_text`, `identity_error`, `db_error`, and `timestamp` are now `pub` so the catalog module can use them without duplicating helpers.
- `crates/codegg-core/src/provider_connections.rs` — `migration_is_idempotent_and_store_crud_is_revision_safe` test asserts the schema version is now 28.
- `tests/storage_migrations.rs` — new `project_catalog_v28_is_additive_and_idempotent` test and updated `final_version` assertion to 28.

### Architecture documentation

- `architecture/project_catalog.md` (new) — full architecture doc covering purpose, public types, locator inertness, archive semantics, restart hydration contract, conservative legacy association, schema/migration, invariants, and integration points.
- `architecture/project_identity_storage.md` — added "Project Catalog Layer" section.
- `architecture/identity.md` — one-line note about the catalog service.
- `architecture/storage.md` — one-line note about v28.
- `architecture/workspace.md` — one-line note about explicit registration through the catalog.

### Static guard

- `scripts/check_project_catalog_invariants.py` (new) — 7 checks:
  1. `project_catalog.rs` file exists.
  2. No SSH/LinkedNode fields are coerced to `PathBuf` outside the row reader.
  3. `attach_locator` validates `Local` against `workspace_project_binding`.
  4. v28 migration creates the three new tables.
  5. v28 migration adds the five new columns to `logical_project`.
  6. `STORAGE_LAYOUT_VERSION` is 28.
  7. `lib.rs` re-exports `pub mod project_catalog;`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg-core project
cargo test -p codegg-core workspace
cargo test -p codegg-core project_catalog
cargo test -p codegg-core
cargo test --test storage_migrations
cargo test -p codegg-core --lib
cargo test -p codegg-git
cargo check -p codegg
cargo test --workspace --all-features
python3 scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_catalog_invariants.py
cargo clippy -p codegg-core --lib --no-deps -- -D warnings
```

### Results

- `cargo fmt --all -- --check` — exit 0.
- `cargo test -p codegg-core project` — 29 passed, 198 filtered out across 3 suites.
- `cargo test -p codegg-core workspace` — 15 passed, 212 filtered out across 3 suites.
- `cargo test -p codegg-core project_catalog` — 11 unit tests in module + 18 integration tests in `tests/project_catalog.rs`.
- `cargo test -p codegg-core` — 227 passed across 4 suites.
- `cargo test --test storage_migrations` — 4 passed (including the new v28 test).
- `cargo test -p codegg-core --lib` — 207 passed.
- `cargo test -p codegg-git` — 356 passed, 7 ignored.
- `cargo check -p codegg` — clean.
- `cargo test --workspace --all-features` — 7889 passed, 10 ignored (108 suites, 163.73s). On a separate run 3769 passed and 1 flaky test failed (`core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog`) — this is a pre-existing timing race in the Eggpool probe test from Provider Connections Milestone 003, unrelated to the catalog, and re-running the broad suite produced 7889 passes. Documented below.
- `python3 scripts/check-core-boundary.sh` — `codegg-core boundary check passed`.
- `python3 scripts/check_daemon_cwd_usage.py` — `cwd usage check passed — no std::env::current_dir() in protected modules`.
- `python3 scripts/check_project_catalog_invariants.py` — `7/7 checks passed. All project catalog invariants verified.`
- `cargo clippy -p codegg-core --lib --no-deps -- -D warnings` — 2 errors in pre-existing code (`crates/codegg-core/src/session/models.rs` derivable impl, `crates/codegg-core/src/session/selection_catalog.rs` complex type). Confirmed pre-existing by stashing the catalog work and re-running; same 2 errors. No new clippy issues in `project_catalog.rs`.

## 5. Invariant review

- **Catalog identity is stable and path-independent.** `ProjectId::new()` is the only constructor for new identities used in the catalog. `PathBuf` values appear only as locators, never as identity inputs.
- **Archive is logical and never deletes workspaces, repositories, sessions, or files.** `archive_project` runs a single `UPDATE logical_project SET lifecycle='archived', archived_at=?, ...`. No DELETE statements are issued on related tables.
- **Catalog listing performs no LSP, Git, indexer, provider probe, or build initialization.** `list_projects`, `count_by_lifecycle`, and `restart_hydration` run only `SELECT` queries on the catalog tables. No `egglsp`, `egggit`, `codegg-providers`, or `build_*` calls appear in the catalog module.
- **Remote locators are inert and cannot be coerced to local paths.** `Locator::Ssh` and `Locator::LinkedNode` have no method that returns a `&Path` or `PathBuf`. `attach_locator` only accepts a `Local` locator whose `workspace_id` is already bound to the project; for `Ssh`/`LinkedNode`, no local path coercion occurs. The static guard confirms the absence of `ssh_*` / `linked_node_*` → `PathBuf::from` patterns.
- **Restart hydration is deterministic and probe-free.** `restart_hydration` executes four `SELECT COUNT(*)` queries on `logical_project`, `project_locator`, and `project_health`. It never opens a directory, runs a `git` command, or instantiates a provider.
- **No network server route becomes less scoped.** `src/server/routes/project.rs` and related server modules are not modified. Server route migration is explicitly scoped to Milestone 4.

## 6. Failure and recovery review

- **Duplicate registration converges.** `concurrent_duplicate_registration_converges_on_one_project` spawns 4 tasks that all attempt to register the same project for the same registered workspace. The `workspace_project_binding` row is created exactly once; subsequent attempts return the existing project via `get_project`. The four project IDs are equal.
- **Archive/restore races are resolved by record state.** `archive_preserves_related_rows` exercises archive; `restore_clears_archived_at` exercises restore. Both write through a single `UPDATE` and `get_project`. Concurrent archive/restore would serialize on the SQLite write lock; the catalog's `archive_project` includes a `database is locked` retry loop (3 attempts, 10 ms) mirroring `ProjectStorage::reconcile_workspace`.
- **Restart hydration is idempotent and probe-free.** `restart_hydration_returns_expected_counts` verifies that hydration is purely count-based.
- **Migration markers make legacy association restart-safe.** `conservative_legacy_association` writes `legacy_catalog_association_marker(source, ...)` after the association transaction commits. A second call for the same `source` returns `AlreadyMigrated` without re-processing workspaces.
- **Cancellation before commit.** All catalog writes that span multiple statements use `pool.begin()` transactions. A panic or cancellation before `tx.commit()` rolls back, leaving no partial project/repository relation.
- **Locator attach/detach validation.** `attach_locator` first calls `locator.validate()`, then verifies the project exists, then verifies (for `Local`) that the workspace is bound. The write is a single `INSERT INTO project_locator`, so partial inserts cannot leave a row without a parent project.

## 7. Migration and compatibility review

- v28 is additive. Existing v25 (`logical_project`, `repository`, `project_repository`, `workspace_project_binding`, `session_project_binding`, `identity_diagnostic`) and v26/v27 tables are untouched. The new columns on `logical_project` are nullable or have `DEFAULT 'unknown'`, so existing rows migrate without intervention.
- The historical `project` table and `session.project_id`/`session.directory` columns remain compatibility projections. The catalog never reads or writes those columns.
- The existing `ProjectStorage` API is preserved. The catalog reads through the public API and writes to its own tables; no existing `ProjectStorage` method has been removed or changed.
- `STORAGE_LAYOUT_VERSION` is bumped to 28 in `crates/codegg-core/src/storage/mod.rs`. The `migrate_v28` function accepts `duplicate column name` errors on idempotent re-run so a partially-applied migration can complete safely.

## 8. Security review

- **No path traversal in registration.** `register_local_project` requires the workspace to already exist in the `workspace` table; the catalog never inserts a workspace from an arbitrary `canonical_root` and never updates a workspace's `canonical_root`. A workspace created by the user through the workspace registry is the only entry point.
- **Remote locator inertness.** `Locator::Ssh` and `Locator::LinkedNode` are stored as bounded TEXT columns. The `ssh_path` is never included in the summary string used for protocol DTOs (`Locator::summary()` formats `ssh:user@host:port (label)`). The catalog has no method that converts an SSH/LinkedNode locator to a `PathBuf`; the only path accessor is `Locator::Local::canonical_root`.
- **No secret leakage in locator metadata.** `Locator::validate()` rejects paths that contain a URL with embedded credentials (`://` and `@` in the same field). Port 0 is rejected. Fields are bounded to 512 bytes. Display summaries are bounded to 1024 bytes.
- **Bounded text fields.** `bounded_text` is reused from `project_storage.rs` to reject empty, oversized, or control-character-bearing values for `display_name`, `description`, `tags`, `source`, `notes`, and locator fields.
- **No probing.** The catalog never calls `inspect_repository_lineage`, `git`, `egglsp::*`, or `codegg-providers::*` during listing, registration, archive, restore, or hydration. Health and locator status are operator-set data.

## 9. Documentation and operations

Updated:

- `architecture/project_catalog.md` (new) — full architecture doc.
- `architecture/project_identity_storage.md` — Project Catalog Layer section.
- `architecture/identity.md` — one-line note.
- `architecture/storage.md` — v28 entry.
- `architecture/workspace.md` — explicit registration note.
- `plans/implementation/project-catalog/001-durable-catalog-foundation.md` — implementation work landed.
- `plans/registry.md` and the project catalog subsystem roadmap will be updated in this PR.

Operators can:

- List projects: `ProjectCatalog::list_projects(include_archived)`.
- Inspect a project: `ProjectCatalog::get_project_with_health(&id)`.
- Archive/restore: `ProjectCatalog::archive_project(&id, source)`, `restore_project(&id, source)`.
- Inspect hydration: `ProjectCatalog::restart_hydration()` returns counts only.
- Inspect legacy association: `conservative_legacy_association(&pool, &workspaces, source)`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `archive_project` retry loop only retries on `database is locked` for 3 attempts. A long-running concurrent archive/restore on a multi-writer setup could exhaust retries. | Acceptable for the single-daemon single-writer model; bounded retry is the same pattern as `ProjectStorage::reconcile_workspace`. | Revisit if a future multi-writer daemon is introduced. |
| low | The `legacy_catalog_association` function uses `inspect_repository_lineage` to decide whether a workspace is unambiguous. A workspace with no `.git` directory records a `rebind_required` diagnostic. | Conservative — preferred over guessing identity. | Operator rebind surface arrives in Milestone 4. |
| low | Flaky `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` test from Provider Connections Milestone 003. | Unrelated to this milestone; pre-existing on main. | Track and fix in the provider-connections subsystem. |
| low | `description: row.get("description")` and other row readers rely on sqlx column-existence checks; columns added in v28 must be present at query time. | v28 migration runs as part of `session::schema::migrate`, so the columns are present whenever the catalog is used. | None — verified by `project_catalog_v28_is_additive_and_idempotent` test. |

No critical or high-severity finding remains for this milestone.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. The project catalog protocol/server migration (Milestone 4 of the project catalog roadmap) and the bounded discovery/reconciliation work (Milestone 2) are now unblocked. The Multi-Project TUI 001 plan and Session Projections 001 plan have their remaining blockers reduced — they still require a protocol surface for the catalog, but they can now plan against a stable, closed catalog service.

The runtime-assets 001 plan and provider-connections 004 plan are unaffected; they do not require the catalog.

## 12. Registry updates

Updated `plans/registry.md` to:

- Close Project Catalog 001 and link this record under "Recently closed work".
- Move Project Catalog 001 from "Dependency-ready implementation plans" to "Recently closed work".
- Keep Multi-Project TUI 001 and Session Projections 001 as blocked work; their blockers are now narrower (catalog protocol surface and project-aware state).
- Retain Domain Identity 003 and Provider Connections 004 in the planning surface.

Updated subsystem roadmap `plans/subsystems/project-catalog-roadmap.md` to mark Milestone 1 closed.
