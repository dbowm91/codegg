# Storage Module

The `storage` module handles SQLite database initialization and connection pooling.

## Overview

**Location**: `crates/codegg-core/src/storage/`

**Key Responsibilities**:
- Database initialization
- Connection pooling
- WAL mode configuration
- Running migrations (delegated to session module)

## Key Types

### Database

```rust
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError>;
    pub fn pool(&self) -> &SqlitePool;
    pub async fn migrate(&self) -> Result<(), StorageError>;
    pub async fn health_check(&self) -> Result<(), StorageError>;
    pub async fn close(self);
}
```

**Note**: The `Database` struct is a simple wrapper around `SqlitePool`. Most code uses `init()` directly to get the pool.

## Initialization

```rust
pub async fn init_daemon_catalog(paths: &DaemonPaths) -> Result<SqlitePool, StorageError>
pub async fn init_legacy_project_store(project_root: &Path) -> Result<SqlitePool, StorageError>
pub async fn init_pool_at(db_path: &Path) -> Result<SqlitePool, StorageError>

#[deprecated]
pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError>
```

Note: `init_daemon_catalog`, `init_legacy_project_store`, and
`init_pool_at` all call `connect_and_configure()` directly and return a
bare `SqlitePool` (not a `Database` struct). The `Database` struct is a
separate wrapper used when you need `health_check()` or `migrate()`
methods. `init` is retained as a deprecated wrapper that routes to one
of the new entry points based on whether `project_dir` is empty or a
real directory; new code MUST NOT use it.

**Path Resolution (Phase 3 split)**:

| Entry point | Database path |
|-------------|---------------|
| `init_daemon_catalog(paths)` | `paths.catalog_db_path()` — `~/Library/Application Support/codegg/codegg.db` on macOS, `$XDG_DATA_HOME/codegg/codegg.db` on Linux. |
| `init_legacy_project_store(root)` | `<root>/.codegg/sessions.db`. |
| `init(project_dir)` (deprecated) | Empty → user config directory + `codegg/sessions.db`. Non-empty → legacy project store. |

`STORAGE_LAYOUT_VERSION = 24` is exported from `storage::mod` and is
referenced from `MigrationMarker.storage_layout_version` so the
migration tooling can report which layout a legacy database was
imported under.

`DaemonPaths` (in `crates/codegg-core/src/storage/paths.rs`) is the
single source of truth for catalog and asset paths:

```rust
pub struct DaemonPaths {
    pub data_root: Option<PathBuf>,
    pub config_root: Option<PathBuf>,
}

impl DaemonPaths {
    pub fn default() -> Self;                                 // platform-default
    pub fn with_overrides(data_root, config_root) -> Self;    // explicit overrides
    pub fn data_root(&self) -> PathBuf;
    pub fn config_root(&self) -> PathBuf;
    pub fn catalog_db_path(&self) -> PathBuf;
    pub fn catalog_db_wal_path(&self) -> PathBuf;
    pub fn agents_dir(&self) -> PathBuf;
    pub fn credentials_path(&self) -> PathBuf;
    pub fn workspace_local_artifact_root(&self, workspace_root: &Path) -> PathBuf;
}
```

## SQLite Configuration

Applied pragmas (batched in single query):

```sql
PRAGMA journal_mode=WAL;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA busy_timeout=5000;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 268435456;  -- 256MB memory-mapped I/O
PRAGMA cache_size = -2000;     -- 2MB cache
PRAGMA temp_store = MEMORY;
PRAGMA foreign_keys = ON;
```

| Pragma | Value | Purpose |
|--------|-------|---------|
| `journal_mode` | `WAL` | Write-Ahead Logging for better concurrency |
| `wal_autocheckpoint` | `1000` | Checkpoint every 1000 pages |
| `busy_timeout` | `5000` | 5 second timeout when database is busy |
| `synchronous` | `NORMAL` | Balanced performance/safety |
| `mmap_size` | `268435456` | 256MB memory-mapped I/O |
| `cache_size` | `-2000` | 2MB cache |
| `temp_store` | `MEMORY` | Temp tables stored in memory |
| `foreign_keys` | `ON` | Foreign key enforcement enabled |

## Connection Pool

Uses `sqlx::SqlitePool` with:
- Hardcoded max connections of 10
- `acquire_timeout(Duration::from_secs(30))` for connection acquisition timeout

## Additional Methods

### health_check()

```rust
pub async fn health_check(&self) -> Result<(), StorageError>
```

Verifies database connectivity by executing `SELECT 1`. Returns `Ok(())` if healthy, or `StorageError::Database` on failure.

### close()

```rust
pub async fn close(self)
```

Gracefully closes the connection pool using async pool shutdown. The `self` parameter consumes the struct to ensure cleanup happens exactly once.

## Migrations

Migrations are implemented in `src/session/schema.rs`, not in the storage module itself. The storage module calls `session::schema::migrate()` during initialization.

Migration versions v1-v22 are supported, covering:
- v1: Initial schema (project, session, message, part, todo, permission, session_share tables)
- v2: Additional indexes
- v3: cached_models table
- v4-v6: Additional indexes
- v7: session.tags column
- v8: part.part_type generated column
- v9: task table
- v10: checkpoints table
- v11: Additional indexes
- v12: session.time_deleted column
- v13: snapshot table
- v14: task.allowed_paths column
- v15: Creates `usage` table for token/cost tracking
- v16: Creates `goal` table (goal lifecycle tracking)
- v17: Creates `session_events` table (event journal)
- v18: Creates `research_run` table (research artifact metadata)
- v19: Creates `user_preferences` table (theme/model persistence)
- v20: Creates `core_event_log` table (daemon core event sequence)
- v21: Creates `notification_history` table (TUI notification backlog)
- v22: Creates `workspace` table, adds `workspace_id` column to `session`, creates `idx_session_workspace_repair` index (Phase 2 single-daemon plan: workspace registry + execution context binding).
- v23 (storage layout marker, not a session migration): the catalog
  schema itself moves from `<workspace>/.codegg/sessions.db` to a
  user-scoped location. The catalog gains a `migration_marker` table
  written by `crates/codegg-core/src/migration.rs`. Legacy project
  databases are imported into the catalog via
  `migrate_legacy_project_database(catalog_pool, registry, project_root)`.
  See [`workspace_services.md`](workspace_services.md) for the full
  contract.

## See Also

- [session.md](session.md) - Uses storage for session data
- [schema.rs](../crates/codegg-core/src/session/schema.rs) - Migration implementation
