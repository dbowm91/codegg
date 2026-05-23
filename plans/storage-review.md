# Storage Module Architecture Review

## Verified Claims

### Database Struct
- `Database` struct wraps `SqlitePool` correctly
- `pool()` returns `&SqlitePool` - matches
- `health_check()` executes `SELECT 1` and returns `Result<(), StorageError>` - matches
- `close()` consumes `self` and calls `pool.close().await` - matches

### init() Function
- Returns `Result<SqlitePool, StorageError>` - matches
- Creates directory if not exists using `tokio::fs::create_dir_all` - matches
- Checks if directory is read-only before proceeding - matches
- Uses `tokio::fs::metadata()` for read-only check (async, not blocking) - matches

### Path Resolution
- Non-empty `project_dir` → `{project_dir}/.codegg/sessions.db` - matches
- Empty `project_dir` → `dirs::config_dir()/codegg/sessions.db` (falls back to `.codegg`) - matches

### SQLite Pragmas
All 8 pragmas match exactly:
- `journal_mode=WAL` ✓
- `wal_autocheckpoint = 1000` ✓
- `busy_timeout=5000` ✓
- `synchronous = NORMAL` ✓
- `mmap_size = 268435456` ✓
- `cache_size = -2000` ✓
- `temp_store = MEMORY` ✓
- `foreign_keys = ON` ✓

### Connection Pool
- `max_connections(10)` - matches
- `acquire_timeout(Duration::from_secs(30))` - matches

### Migrations
- v1-v14 all present and correctly implemented in `src/session/schema.rs`
- All table/column additions match documentation descriptions

## Bugs/Discrepancies Found

### 1. Redundant Migration Execution (Medium)
**Location**: `src/storage/mod.rs:19-23` and `src/storage/mod.rs:122-124`

`Database::new()` calls `crate::session::schema::migrate(&pool).await?` at line 21. But `init()` at line 124 also calls `crate::session::schema::migrate(&pool).await?`.

When `init()` is called, it:
1. Calls `connect_and_configure()` which creates `Database::new()` internally (line 122 → 58 → 21)
2. `Database::new()` runs migrations (line 21)
3. Then `init()` runs migrations AGAIN at line 124

This is redundant but not functionally broken (migrations are idempotent with `CREATE TABLE IF NOT EXISTS`). However, it wastes time on every init.

### 2. Documentation Inconsistency - init() Example (Low)
**Location**: `architecture/storage.md:38-46`

The `init()` example shows:
```rust
let database = Database::new(db_path).await?;
session::schema::migrate(&database.pool).await?;
```

But `Database::new()` already calls `migrate()` internally. The example would run migrations twice. The documentation should either:
- Remove the explicit `migrate()` call since `Database::new()` handles it
- Or clarify that this is showing the manual pattern separate from `init()`

### 3. get_db_path() Not Exported (Low)
**Location**: `src/storage/mod.rs:46-56`

The `get_db_path()` function is private (`fn`). The architecture doc doesn't mention it, so this isn't a bug, just noting it's internal-only. Callers must use `init()`.

## Improvement Suggestions

### Priority: Low

1. **Eliminate double migration in init()**: Consider having `init()` return `Database` instead of `SqlitePool`, so callers who need the pool get it via `database.pool()`. This avoids the redundant migration call and is cleaner API design.

   ```rust
   pub async fn init(project_dir: &str) -> Result<Database, StorageError> {
       let db_path = get_db_path(project_dir);
       // ... directory setup ...
       Database::new(&db_path_str).await
   }
   ```

2. **Update init() example in doc**: Remove the explicit `session::schema::migrate()` call from the example since `Database::new()` handles it internally.

3. **Add example for Database usage pattern**: Show how to use `Database` directly for cases where callers need the wrapper (e.g., calling `health_check()` or `close()`).

## Summary

The storage module implementation largely matches the architecture documentation. The pragma configuration, pool settings, path resolution, and migration structure are all correctly documented. The main finding is the redundant migration execution when using `init()`, which is a minor inefficiency rather than a functional bug.