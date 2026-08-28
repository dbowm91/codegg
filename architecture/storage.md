# Storage Module

## Purpose

The `storage` module provides SQLite database initialization, connection
pooling, WAL configuration, and the platform-resolved path layout for
the user-scoped daemon catalog. It also hosts the `UserPreferences`
key/value store backed by `user_preferences` table.

## Where It Lives

```
crates/codegg-core/src/storage/
├── mod.rs          # Database wrapper, init functions, pragmas,
│                   # STORAGE_LAYOUT_VERSION, deprecated init()
├── paths.rs        # DaemonPaths — single source of truth for
│                   # catalog and asset paths
└── preferences.rs  # UserPreferences — persistent key/value store
```

## How It Works

### Database Initialization

There are **four** entry points for getting a `SqlitePool`:

| Function | Path | Purpose |
|----------|------|---------|
| `init_daemon_catalog(paths)` | `catalog_db_path()` | User-scoped daemon catalog, no migrations |
| `init_migrated_daemon_catalog(paths)` | Same as above | Runs migrations on a single-connection bootstrap pool, closes it, then opens the normal catalog pool |
| `init_legacy_project_store(root)` | `<root>/.codegg/sessions.db` | Legacy project-local store for backward compat |
| `init_pool_at(db_path)` | Caller-supplied path | Generic pool at an arbitrary path |
| `init(project_dir)` (deprecated) | Empty → config dir; non-empty → legacy | Retained for tests; new code MUST NOT use |

`init_migrated_daemon_catalog` is the **production daemon bootstrap
authority**. It creates a single-connection pool for migration, runs
all schema migrations, closes the migration pool, then opens the normal
10-connection pool via `init_daemon_catalog`.

### Path Layout (`DaemonPaths`)

`DaemonPaths` (`storage/paths.rs:20`) is the single source of truth.
Defaults follow OS conventions with fallbacks:

| Platform | Data root | Config root |
|----------|-----------|-------------|
| macOS | `~/Library/Application Support/codegg/` | `~/Library/Application Support/codegg/` |
| Linux | `$XDG_DATA_HOME/codegg/` or `~/.local/share/codegg/` | `$XDG_CONFIG_HOME/codegg/` or `~/.config/codegg/` |
| Fallback | `~/.codegg/` | `~/.config/codegg/` |

Override via `CODEGG_DATA_HOME` env var (data root only).

```rust
pub struct DaemonPaths {
    pub data_root: Option<PathBuf>,   // override or platform default
    pub config_root: Option<PathBuf>, // override or platform default
}
```

Key derived paths:
- `catalog_db_path()` → `<data_root>/codegg.db`
- `catalog_db_wal_path()` → `<data_root>/codegg.db-wal`
- `agents_dir()` → `<config_root>/agents/`
- `credentials_path()` → `<config_root>/credentials.json`
- `workspace_local_artifact_root(ws)` → `<ws>/.codegg/`

### SQLite Configuration

Applied in `connect_and_configure()` (`storage/mod.rs:214`) as a
single batched query:

| Pragma | Value | Purpose |
|--------|-------|---------|
| `journal_mode` | `WAL` | Write-Ahead Logging for concurrency |
| `wal_autocheckpoint` | `1000` | Checkpoint every 1000 pages |
| `busy_timeout` | `5000` | 5s timeout on busy |
| `synchronous` | `NORMAL` | Balanced performance/safety |
| `mmap_size` | `268435456` | 256MB memory-mapped I/O |
| `cache_size` | `-2000` | 2MB page cache |
| `temp_store` | `MEMORY` | Temp tables in RAM |
| `foreign_keys` | `ON` | FK enforcement |

### Connection Pool

`connect_and_configure()` creates the pool via `SqlitePoolOptions`:
- Normal pools: `max_connections(10)`, `acquire_timeout(30s)`
- Migration pools: `max_connections(1)`

### Database Wrapper

```rust
pub struct Database { pool: SqlitePool }
```

Methods:
- `new(path)` — Open + migrate + WAL checkpoint + background integrity
  check (5s delay)
- `pool()` — Borrow the underlying `SqlitePool`
- `migrate()` — Re-run schema migrations (idempotent)
- `health_check()` — `SELECT 1`
- `close()` — WAL checkpoint + pool shutdown

### WAL Checkpoint and Integrity

`Database::new()` triggers:
1. `try_checkpoint_wal()` — non-fatal WAL checkpoint
2. `spawn_background_integrity_check()` — `PRAGMA quick_check` after
   5s delay

### STORAGE_LAYOUT_VERSION

`STORAGE_LAYOUT_VERSION = 36` (`storage/mod.rs:39`) is exported and
referenced from `MigrationMarker.storage_layout_version` for the
migration tooling that imports legacy project databases.

## Key Types & APIs

### Database (`storage/mod.rs:41`)

```rust
impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError>;
    pub fn pool(&self) -> &SqlitePool;
    pub async fn migrate(&self) -> Result<(), StorageError>;
    pub async fn health_check(&self) -> Result<(), StorageError>;
    pub async fn close(self);
}
```

### DaemonPaths (`storage/paths.rs:20`)

```rust
impl DaemonPaths {
    pub fn default() -> Self;
    pub fn with_overrides(data_root, config_root) -> Self;
    pub fn data_root(&self) -> PathBuf;
    pub fn config_root(&self) -> PathBuf;
    pub fn catalog_db_path(&self) -> PathBuf;
    pub fn catalog_db_wal_path(&self) -> PathBuf;
    pub fn agents_dir(&self) -> PathBuf;
    pub fn credentials_path(&self) -> PathBuf;
    pub fn workspace_local_artifact_root(&self, ws: &Path) -> PathBuf;
}
```

### UserPreferences (`storage/preferences.rs:25`)

```rust
impl UserPreferences {
    pub fn new(pool: SqlitePool) -> Self;
    pub async fn get(&self, key: &str) -> Result<Option<String>>;
    pub async fn set(&self, key: &str, value: &str) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<u64>;
    pub async fn updated_at(&self, key: &str) -> Result<Option<i64>>;
}
```

Known keys:
- `KEY_THEME_ACTIVE` = `"theme.active"` — active theme id
- `KEY_MODEL_LAST_USED` = `"model.last_used"` — last-used model id

### Init Functions (`storage/mod.rs`)

```rust
pub async fn init_daemon_catalog(paths: &DaemonPaths)
    -> Result<SqlitePool, StorageError>;
pub async fn init_migrated_daemon_catalog(paths: &DaemonPaths)
    -> Result<SqlitePool, StorageError>;
pub async fn init_legacy_project_store(project_root: &Path)
    -> Result<SqlitePool, StorageError>;
pub async fn init_pool_at(db_path: &Path)
    -> Result<SqlitePool, StorageError>;
#[deprecated] pub async fn init(project_dir: &str)
    -> Result<SqlitePool, StorageError>;
```

## Configuration Surface

- `CODEGG_DATA_HOME` env var overrides the default data root in
  `DaemonPaths::default_data_root()` (`storage/paths.rs:41`)

## Invariants & Gotchas

1. **Single daemon invariant**: The catalog database is user-scoped,
   not project-scoped. Exactly one daemon owns it per OS user.
2. **Migration pool isolation**: `init_migrated_daemon_catalog` uses a
   single-connection pool for migration, closes it, then opens the
   normal pool. This prevents self-deadlocking during bootstrap.
3. **init() is deprecated**: `init(project_dir)` routes to either the
   user config directory or a legacy project store. New code MUST NOT
   use it.
4. **init_daemon_catalog vs init_migrated_daemon_catalog**: The former
   returns a pool without running migrations. The latter is the
   production bootstrap path.
5. **init_pool_at creates directories**: It calls `create_dir_all` and
   checks for read-only directories before connecting.
6. **Integrity check is non-fatal**: `spawn_background_integrity_check`
   logs warnings but does not fail startup.
7. **Pool max connections is 10**: Hardcoded in `connect_and_configure`.
   Migration pools use `max_connections(1)`.
8. **Catalog DB vs workspace-local artifacts**: The catalog owns
   sessions, jobs, notification history. Workspace-local artifacts
   (run store data) live under `<workspace>/.codegg/runs/`.

## Migrations (Storage Layout Context)

Migrations are implemented in `session/schema.rs`, not in the storage
module. The storage module calls `session::schema::migrate()` during
initialization.

Key storage-layout migrations:
- **v22**: Workspace table, `session.workspace_id` column — Phase 2
  workspace registry
- **v23**: Durable jobs tables — Phase 4 job orchestration
- **v24**: `provider_connections` — daemon-owned connection metadata
- **v25**: Canonical project/repository authority — additive identity
  tables
- **v26**: Provider provisioning, health, model catalog
- **v27**: Session selection columns (connection ID, revision, model)
- **v28**: Project catalog tables (locators, health, legacy markers)
- **v29**: Discovery roots, scans, observations
- **v30**: Runtime asset refresh provenance
- **v31**: Provider lifecycle, reference, tombstone, audit
- **v32**: Projection streams, events, checkpoints
- **v33–v34**: Tool Program domain, notification claims
- **v35**: Nullable typed lineage columns for child jobs
- **v36**: Durable per-job execution timeouts

## Testing

```bash
cargo test -p codegg-core --lib storage
cargo test -p codegg-core --lib storage::paths
cargo test -p codegg-core --lib storage::preferences
cargo test -p codegg-core --test storage_migrations
```

## Related Docs

- [session.md](session.md) — Schema migrations, session tables
- [workspace_services.md](workspace_services.md) — Workspace-local
  run store, catalog migration
- `crates/codegg-core/src/migration.rs` — Legacy database import
  tooling
- `crates/codegg-core/src/jobs/` — Durable job store (v23+)
