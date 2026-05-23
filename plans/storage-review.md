# Storage Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `Database` struct wraps `SqlitePool` | VERIFIED | `src/storage/mod.rs:14-16` - struct has single `pool: SqlitePool` field |
| `Database::new(path)` returns `Result<Self, StorageError>` | VERIFIED | `src/storage/mod.rs:19` - signature matches |
| `Database::pool()` returns `&SqlitePool` | VERIFIED | `src/storage/mod.rs:25-27` - returns reference to pool |
| `Database::migrate()` exists | VERIFIED | `src/storage/mod.rs:29-31` - method exists |
| `Database::health_check()` executes `SELECT 1` | VERIFIED | `src/storage/mod.rs:33-39` - query matches |
| `Database::close()` consumes self for exactly-once cleanup | VERIFIED | `src/storage/mod.rs:41-43` - takes `self`, calls `pool.close().await` |
| `init()` returns `Result<SqlitePool, StorageError>` | VERIFIED | `src/storage/mod.rs:85` - signature matches |
| Path resolution: non-empty project_dir → `{project_dir}/.codegg/sessions.db` | VERIFIED | `src/storage/mod.rs:47-48` - `PathBuf::from(project_dir).join(".codegg").join("sessions.db")` |
| Path resolution: empty project_dir → `~/.config/codegg/sessions.db` | VERIFIED | `src/storage/mod.rs:49-52` - uses `dirs::config_dir()` with fallback to `.codegg` |
| Pragmas batched in single query | VERIFIED | `src/storage/mod.rs:66-80` - single `sqlx::query()` with 8 PRAGMAs |
| All 8 pragmas present (journal_mode, wal_autocheckpoint, busy_timeout, synchronous, mmap_size, cache_size, temp_store, foreign_keys) | VERIFIED | `src/storage/mod.rs:68-75` - all pragmas present with correct values |
| WAL pragma value is `WAL` | VERIFIED | `src/storage/mod.rs:68` - `PRAGMA journal_mode=WAL;` |
| wal_autocheckpoint value is `1000` | VERIFIED | `src/storage/mod.rs:69` - `PRAGMA wal_autocheckpoint = 1000;` |
| busy_timeout value is `5000` | VERIFIED | `src/storage/mod.rs:70` - `PRAGMA busy_timeout=5000;` |
| synchronous value is `NORMAL` | VERIFIED | `src/storage/mod.rs:71` - `PRAGMA synchronous = NORMAL;` |
| mmap_size value is `268435456` (256MB) | VERIFIED | `src/storage/mod.rs:72` - `PRAGMA mmap_size = 268435456;` |
| cache_size value is `-2000` (2MB) | VERIFIED | `src/storage/mod.rs:73` - `PRAGMA cache_size = -2000;` |
| temp_store value is `MEMORY` | VERIFIED | `src/storage/mod.rs:74` - `PRAGMA temp_store = MEMORY;` |
| foreign_keys value is `ON` | VERIFIED | `src/storage/mod.rs:75` - `PRAGMA foreign_keys = ON;` |
| Hardcoded max connections of 10 | VERIFIED | `src/storage/mod.rs:60` - `.max_connections(10)` |
| acquire_timeout is 30 seconds | VERIFIED | `src/storage/mod.rs:61` - `.acquire_timeout(Duration::from_secs(30))` |
| `Database::new()` calls `migrate()` internally | VERIFIED | `src/storage/mod.rs:21` - calls `crate::session::schema::migrate(&pool)` |
| `init()` calls `migrate()` after pool creation | VERIFIED | `src/storage/mod.rs:124` - calls `crate::session::schema::migrate(&pool)` |
| Migrations v1-v14 supported | VERIFIED | `src/session/schema.rs:25-66` - all 14 migrations implemented |
| Migrations implemented in `src/session/schema.rs` | VERIFIED | `src/session/schema.rs` contains full migration implementation |
| v1: Initial schema (project, session, message, part, todo, permission, session_share tables) | VERIFIED | `src/session/schema.rs:122-295` - all tables created |
| v2: Additional indexes | VERIFIED | `src/session/schema.rs:297-308` - session_title_idx, session_slug_idx |
| v3: cached_models table | VERIFIED | `src/session/schema.rs:311-335` - table and index created |
| v4: Additional indexes | VERIFIED | `src/session/schema.rs:338-345` - session_time_updated_idx |
| v5: Additional indexes | VERIFIED | `src/session/schema.rs:347-354` - adds share_expires_at column |
| v6: Additional indexes | VERIFIED | `src/session/schema.rs:356-372` - permission_time_idx, session_project_archived_idx |
| v7: session.tags column | VERIFIED | `src/session/schema.rs:374-386` - ALTER TABLE adds tags column |
| v8: part.part_type generated column | VERIFIED | `src/session/schema.rs:388-402` - ALTER TABLE adds generated column |
| v9: task table | VERIFIED | `src/session/schema.rs:404-437` - task table created |
| v10: checkpoints table | VERIFIED | `src/session/schema.rs:439-461` - checkpoints table created |
| v11: Additional indexes | VERIFIED | `src/session/schema.rs:463-470` - idx_session_directory |
| v12: session.time_deleted column | VERIFIED | `src/session/schema.rs:472-479` - ALTER TABLE adds column |
| v13: snapshot table | VERIFIED | `src/session/schema.rs:481-504` - snapshot table created |
| v14: task.allowed_paths column | VERIFIED | `src/session/schema.rs:506-513` - ALTER TABLE adds column |

## Bugs Found

### Critical
None identified.

### High
None identified.

### Medium

**1. `Database::new()` duplicates migration logic**

`Database::new()` at `src/storage/mod.rs:19-23` calls `crate::session::schema::migrate(&pool)` internally, but `init()` at lines 85-132 also calls migrations after pool creation. If `Database::new()` is called directly, migrations run once. If `init()` is called, migrations run twice on the same pool.

- **File**: `src/storage/mod.rs:21`
- **Impact**: Unnecessary migration overhead; if schema changes between calls, behavior is undefined
- **Severity**: Medium - double migration is wasteful but not correctness-breaking since migrations are idempotent

**2. `get_db_path()` fallback could create unexpected location**

When `project_dir` is empty and `dirs::config_dir()` returns `None`, the fallback `PathBuf::from(".codegg")` resolves to a relative path (`.codegg` in current working directory), not a persistent config location.

- **File**: `src/storage/mod.rs:52`
- **Impact**: Database could be created in unexpected location if no config dir exists
- **Severity**: Medium - unusual edge case but could cause data loss if user expects persistent storage

### Low

**3. No validation that database file is actually a SQLite database**

`init()` checks directory permissions but doesn't validate the database file itself (e.g., checking for SQLite header) before connecting.

- **File**: `src/storage/mod.rs:104-118`
- **Impact**: Could open/corrupt a non-SQLite file if user mistakenly points to wrong path
- **Severity**: Low - sqlx will fail gracefully, but error message may be confusing

**4. No cleanup of WAL files on database close**

SQLite WAL files (`sessions.db-wal`, `sessions.db-shm`) are not explicitly checkpointed or cleaned up when `Database::close()` is called.

- **File**: `src/storage/mod.rs:41-43`
- **Impact**: WAL data may not be flushed to main database, potentially causing data loss on abnormal termination
- **Severity**: Low - SQLite usually recovers, but explicit checkpoint would be safer

## Improvement Suggestions

### Performance

1. **Consider increasing `max_connections` for read-heavy workloads**
   - Current: 10 connections
   - Suggestion: Allow configuration via `SqlitePoolOptions` instead of hardcoded value
   - Benefit: Better throughput for concurrent read operations

2. **Consider `read_only` mode for health checks**
   - Current: `SELECT 1` for health check
   - Suggestion: Could use `PRAGMA query_only` to ensure health check doesn't trigger writes
   - Benefit: More accurate health representation for read-only scenarios

### Correctness

1. **Add database file validation before opening**
   - Suggestion: Check for SQLite header (`SQLite format 3\0`) before connecting
   - Benefit: Fail fast with clear error if file is not a valid SQLite database

2. **Add explicit WAL checkpoint before close**
   - Suggestion: Execute `PRAGMA wal_checkpoint(TRUNCATE)` before closing
   - Benefit: Ensure all WAL data is flushed to main database file

3. **Consider adding `busy_timeout` explanation in comments**
   - Current: pragma is set but undocumented
   - Suggestion: Comment that this allows 5-second wait for locked database
   - Benefit: Future maintainers understand the behavior

### Maintainability

1. **Add `From<&SqlitePool>` for `Database` to avoid duplication**
   - Current: `Database::new()` duplicates `init()` logic
   - Suggestion: Create `Database::from_pool()` for existing pools
   - Benefit: DRY principle, single source of migration truth

2. **Consider moving `get_db_path()` logic to a separate function with tests**
   - Current: Path resolution logic is embedded in `init()`
   - Suggestion: Extract to testable helper function
   - Benefit: Easier to verify path resolution correctness

3. **Document the interaction between `Database::new()` and `init()`**
   - Current: Unclear which function callers should use
   - Suggestion: Add doc comments clarifying that `init()` is the preferred entry point
   - Benefit: Prevents accidental double-migration

4. **Consider adding error context for permission failures**
   - Current: `dir_metadata.permissions().readonly()` returns generic error
   - Suggestion: Include path in error message for debugging
   - Benefit: Easier to diagnose permission issues in production

## Priority Actions (top 5 items to fix)

1. **Fix double migration in `Database::new()`**: Remove duplicate `migrate()` call so `Database::new()` only handles pool creation. This prevents wasted work and potential future bugs from double-execution.

2. **Add explicit WAL checkpoint before close**: Call `PRAGMA wal_checkpoint(TRUNCATE)` in `close()` to ensure WAL data is flushed before shutdown.

3. **Add SQLite header validation**: Check for `SQLite format 3` header before opening to fail fast with clear errors on invalid databases.

4. **Document `Database::new()` vs `init()` usage**: Add doc comments clarifying `init()` is the standard entry point and `Database::new()` is for cases needing the wrapper.

5. **Add path resolution tests**: Extract `get_db_path()` to a testable function with unit tests covering empty project_dir, valid/invalid config dirs, and relative/absolute paths.