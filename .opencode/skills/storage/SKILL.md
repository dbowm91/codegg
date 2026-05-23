---
name: storage
description: SQLite database initialization and connection pooling for opencode-rs
version: 1.1.0
tags:
  - storage
  - sqlite
  - database
  - sqlx
  - migrations
---

# Storage Module Guide

This skill covers the storage module in opencode-rs for SQLite database initialization and connection pooling.

## Overview

**Location**: `src/storage/mod.rs`

**Key Responsibilities**:
- Database path resolution
- Connection pool creation and configuration
- WAL mode and SQLite pragma configuration
- Delegating migrations to session module

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

**Note**: `Database` is a simple wrapper around `SqlitePool`. Most code uses `init()` directly to get the pool.

## Public API

### init()

```rust
pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError>
```

Initializes the database:
1. Resolves database path from `project_dir`
2. Creates parent directory if needed
3. Checks if directory is read-only
4. Creates and configures connection pool
5. Runs migrations via `session::schema::migrate()`

**Path Resolution**:
- If `project_dir` is non-empty: `{project_dir}/.codegg/sessions.db`
- If empty: `~/.config/codegg/sessions.db` (falls back to `.codegg` if config dir unavailable)

### get_db_path()

```rust
fn get_db_path(project_dir: &str) -> PathBuf
```

Internal helper for path resolution.

## SQLite Pragmas

The module configures SQLite with these pragmas (batched into single query):

```sql
PRAGMA journal_mode=WAL;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA busy_timeout=5000;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 268435456;  -- 256MB
PRAGMA cache_size = -2000;     -- 2MB
PRAGMA temp_store = MEMORY;
PRAGMA foreign_keys = ON;
```

| Pragma | Value | Purpose |
|--------|-------|---------|
| `journal_mode` | `WAL` | Write-Ahead Logging |
| `wal_autocheckpoint` | `1000` | Checkpoint every 1000 pages |
| `busy_timeout` | `5000` | 5s timeout when database busy |
| `synchronous` | `NORMAL` | Balanced performance/safety |
| `mmap_size` | `268435456` | 256MB memory-mapped I/O |
| `cache_size` | `-2000` | 2MB cache |
| `temp_store` | `MEMORY` | Temp tables in memory |
| `foreign_keys` | `ON` | Foreign key enforcement |

## Connection Pool

- Uses `sqlx::SqlitePool`
- Hardcoded `max_connections(10)`
- `acquire_timeout(Duration::from_secs(30))` for connection acquisition

### health_check()

```rust
pub async fn health_check(&self) -> Result<(), StorageError>
```

Verifies database connectivity by executing `SELECT 1`. Returns `Ok(())` if healthy, or `StorageError::Database` on failure.

### close()

```rust
pub async fn close(self)
```

Gracefully closes the connection pool. Uses async pool shutdown. The `self` parameter consumes the struct to ensure cleanup happens exactly once.

## Migrations

Migrations are **NOT** in the storage module - they are in `src/session/schema.rs`:

```rust
pub async fn migrate(pool: &SqlitePool) -> Result<(), StorageError>
```

Supported versions v1-v14. See [session skill](../session/SKILL.md) for schema details.

## Error Handling

All errors return `StorageError`:

```rust
pub enum StorageError {
    #[error("database error: {0}")]
    Database(String),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("llm operation failed: {operation}: {message}")]
    LlmOperation { operation: String, message: String },
    #[error("import error: {0}")]
    Import(String),
    #[error("export error: {0}")]
    Export(String),
}
```

## Usage Example

```rust
use crate::storage::{self, StorageError};

// Initialize database
let pool = storage::init(project_dir).await?;

// Or use Database struct directly
let db = Database::new("sqlite:sessions.db").await?;
let pool = db.pool();
```

## Architecture Notes

- Storage module is a thin layer over SQLite/sqlx
- Most database logic lives in `session/` module
- `init()` returns `SqlitePool`, not `Database` - callers don't need the wrapper
- Pragmas are batched to reduce round-trips

## See Also

- [session.md](../../architecture/session.md) - Database schema and stores
- [schema.rs](../../src/session/schema.rs) - Migration implementation