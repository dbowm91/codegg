# Storage Module Architecture Review (2026-05-26)

## Summary

The `architecture/storage.md` document is **fully accurate** and matches the implementation in `src/storage/mod.rs` and `src/session/schema.rs`. No errors found.

## Verified Correct Items

### Database Struct (lines 17-31)
- `Database` struct with `pool: SqlitePool` field ✓
- `new(path: &str)` constructor ✓
- `pool()` accessor returning `&SqlitePool` ✓
- `migrate()` for running migrations ✓
- `health_check()` using `SELECT 1` query ✓
- `close(self)` consuming the struct ✓

### Note About Wrapper (line 33)
- Correctly notes `Database` is a wrapper around `SqlitePool` and most code uses `init()` directly ✓

### init() Function (lines 35-46)
- Signature `pub async fn init(project_dir: &str) -> Result<SqlitePool, StorageError>` ✓
- Delegates to `Database::new()` internally ✓
- Runs migrations via `session::schema::migrate()` ✓

### Path Resolution (lines 48-50)
- Non-empty `project_dir` → `{project_dir}/.codegg/sessions.db` ✓
- Empty `project_dir` → `~/.config/codegg/sessions.db` with fallback to `.codegg` ✓

### SQLite Pragmas (lines 52-76)
All 8 pragmas correctly documented with exact values:
- `journal_mode=WAL` ✓
- `wal_autocheckpoint = 1000` ✓
- `busy_timeout=5000` ✓
- `synchronous = NORMAL` ✓
- `mmap_size = 268435456` (256MB) ✓
- `cache_size = -2000` (2MB) ✓
- `temp_store = MEMORY` ✓
- `foreign_keys = ON` ✓

### Connection Pool (lines 78-83)
- Max connections: 10 ✓
- `acquire_timeout(Duration::from_secs(30))` ✓

### health_check() (lines 86-93)
- Executes `SELECT 1` ✓
- Returns `Ok(())` on success ✓
- Returns `StorageError::Database` on failure ✓

### close() (lines 95-101)
- Takes `self` by value to ensure single cleanup ✓
- Uses async pool shutdown via `pool.close().await` ✓

### Migrations (lines 102-119)
- Migrations implemented in `src/session/schema.rs` ✓
- Migration versions v1-v14 correctly listed ✓
- Each migration's purpose accurately described ✓

## Incorrect/Stale Items

**None found.** The document is accurate and up-to-date.

## Bugs Found in Related Code

**None found.** The storage module implementation is correct:
- `Database::new()` properly initializes pool and runs migrations
- `connect_and_configure()` correctly batches all pragmas
- `init()` handles directory creation, permissions check, and async fs operations
- `health_check()` properly returns `StorageError::Database` (not generic error)
- `close()` properly consumes self via `pool.close().await`

## Line Numbers Referenced in Doc vs Actual

| Item | Doc Line | Actual Location |
|------|----------|-----------------|
| Database struct | 20-22 | `src/storage/mod.rs:14-16` |
| Database::new | 25 | `src/storage/mod.rs:19` |
| Database::pool | 26 | `src/storage/mod.rs:25` |
| Database::migrate | 27 | `src/storage/mod.rs:29` |
| Database::health_check | 28 | `src/storage/mod.rs:33` |
| Database::close | 29 | `src/storage/mod.rs:41` |
| init() | 38 | `src/storage/mod.rs:85` |
| connect_and_configure | N/A (internal) | `src/storage/mod.rs:58` |
| Pragmas | 56-65 | `src/storage/mod.rs:66-77` |
| get_db_path | N/A (internal) | `src/storage/mod.rs:46` |
| migrate() | 104 | `src/session/schema.rs:5` |
| migrate_v1-v14 | 106-118 | `src/session/schema.rs:122-512` |

## Conclusion

The architecture document is **accurate and requires no changes**. All claims were verified against the actual implementation.
