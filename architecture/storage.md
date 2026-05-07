# Storage Module

The `storage` module handles SQLite database initialization and connection pooling.

## Overview

**Location**: `src/storage/`

**Key Responsibilities**:
- Database initialization
- Connection pooling
- WAL mode configuration
- Running migrations

## Key Types

### Database

```rust
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: &Path) -> Result<Self>;
    pub fn pool(&self) -> SqlitePool;
}
```

## Initialization

```rust
pub async fn init(db_path: &Path) -> Result<Database> {
    let database = Database::new(db_path).await?;

    // Run migrations
    session::schema::migrate(&database.pool).await?;

    Ok(database)
}
```

## SQLite Configuration

Applied pragmas:

```rust
// WAL mode for better concurrency
PRAGMA journal_mode = WAL;

// Foreign keys enabled
PRAGMA foreign_keys = ON;

// Optimized for performance
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -64000;  // 64MB cache
PRAGMA mmap_size = 268435456; // 256MB memory-mapped I/O
```

## Connection Pool

Uses `sqlx::SqlitePool` for connection pooling:

```rust
let pool = SqlitePool::connect(&database_url).await?;
```

Default pool size is calculated based on CPU cores.

## See Also

- [session.md](session.md) - Uses storage for session data
