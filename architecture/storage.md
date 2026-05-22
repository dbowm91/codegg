# Storage Module

The `storage` module handles SQLite database initialization and connection pooling.

## Overview

**Location**: `src/storage/`

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
}
```

**Note**: The `Database` struct is a simple wrapper around `SqlitePool`. Most code uses `init()` directly to get the pool.

## Initialization

```rust
pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError> {
    let database = Database::new(db_path).await?;

    // Run migrations
    session::schema::migrate(&database.pool).await?;

    Ok(database)
}
```

**Path Resolution**:
- If `project_dir` is non-empty: `{project_dir}/.codegg/sessions.db`
- If empty: `~/.config/codegg/sessions.db` (falls back to `.codegg` if config dir unavailable)

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

Uses `sqlx::SqlitePool` with hardcoded max connections of 10.

## Migrations

Migrations are implemented in `src/session/schema.rs`, not in the storage module itself. The storage module calls `session::schema::migrate()` during initialization.

Migration versions v1-v14 are supported, covering:
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

## See Also

- [session.md](session.md) - Uses storage for session data
- [schema.rs](../src/session/schema.rs) - Migration implementation
