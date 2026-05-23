# Storage Architecture Review

## Architecture Document
- Path: architecture/storage.md

## Source Code Location
- src/storage/
- src/session/schema.rs (migrations)

## Verification Summary
**Pass**

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Database struct wraps SqlitePool | Pass | mod.rs:14-16 |
| pool() method returns &SqlitePool | Pass | mod.rs:25-27 |
| migrate() calls session::schema::migrate() | Pass | mod.rs:29-31 |
| health_check() uses SELECT 1 | Pass | mod.rs:33-39 |
| close() uses pool.close().await | Pass | mod.rs:41-43 |
| init() path resolution: {project_dir}/.codegg/sessions.db | Pass | mod.rs:46-56 |
| init() fallback to ~/.config/codegg/sessions.db | Pass | mod.rs:50-52 |
| init() creates directory if not exists | Pass | mod.rs:93-102 |
| init() checks read-only filesystem | Pass | mod.rs:104-118 |
| connect_and_configure() max_connections=10 | Pass | mod.rs:59-60 |
| acquire_timeout(Duration::from_secs(30)) | Pass | mod.rs:61 |
| Pragma: journal_mode=WAL | Pass | mod.rs:68 |
| Pragma: wal_autocheckpoint=1000 | Pass | mod.rs:69 |
| Pragma: busy_timeout=5000 | Pass | mod.rs:70 |
| Pragma: synchronous=NORMAL | Pass | mod.rs:71 |
| Pragma: mmap_size=268435456 | Pass | mod.rs:72 |
| Pragma: cache_size=-2000 | Pass | mod.rs:73 |
| Pragma: temp_store=MEMORY | Pass | mod.rs:74 |
| Pragma: foreign_keys=ON | Pass | mod.rs:75 |
| Pragma batched in single query | Pass | mod.rs:66-80 |
| Migrations v1-v14 supported | Pass | schema.rs:25-66 |
| Migration v1 creates project, session, message, part, todo, permission, session_share tables | Pass | schema.rs:122-295 |
| Migration v2 adds indexes | Pass | schema.rs:297-309 |
| Migration v3 creates cached_models table | Pass | schema.rs:311-336 |
| Migration v4 adds session_time_updated_idx | Pass | schema.rs:338-345 |
| Migration v5 adds share_expires_at column | Pass | schema.rs:347-354 |
| Migration v6 adds indexes | Pass | schema.rs:356-372 |
| Migration v7 adds session.tags column | Pass | schema.rs:374-386 |
| Migration v8 adds part.part_type generated column | Pass | schema.rs:388-402 |
| Migration v9 creates task table | Pass | schema.rs:404-437 |
| Migration v10 creates checkpoints table | Pass | schema.rs:439-461 |
| Migration v11 adds idx_session_directory | Pass | schema.rs:463-470 |
| Migration v12 adds session.time_deleted column | Pass | schema.rs:472-479 |
| Migration v13 creates snapshot table | Pass | schema.rs:481-504 |
| Migration v14 adds task.allowed_paths column | Pass | schema.rs:506-513 |

## Issues Found

### Bugs
None found.

### Inconsistencies
None found. The architecture document is well-synchronized with the implementation.

### Missing Documentation
1. **Database::new() does NOT call migrate()** - The architecture shows `init()` as the public API which calls `Database::new()` but then shows `Database::new()` as running migrations itself (mod.rs lines 19-23). However, the actual `init()` function (mod.rs:85-129) creates the pool via `connect_and_configure()` and does NOT go through `Database::new()`. So the architecture's `init()` example showing `Database::new()` followed by `session::schema::migrate()` is misleading - the real code path uses `connect_and_configure()` directly and still runs migrations.

2. **StorageError variants not documented** - The architecture doesn't mention the actual StorageError enum variants (Database, Migration, NotFound, LlmOperation, Import, Export). These are relevant to callers of init(), health_check(), and other public functions.

3. **Database::new() is not used by init()** - The public `init()` function (mod.rs:85) calls `connect_and_configure()` directly rather than `Database::new()`. This is an implementation detail that makes the architecture slightly misleading about the relationship between init() and Database::new().

### Improvement Opportunities
1. Add StorageError variant documentation to the architecture
2. Clarify that `init()` uses `connect_and_configure()` directly rather than `Database::new()`
3. Document that migrations run during Database::new() as well as during init()

## Recommendations
1. Update architecture to document StorageError variants (Database, Migration, NotFound, LlmOperation, Import, Export)
2. Clarify the relationship between init(), connect_and_configure(), and Database::new() - either show the actual call chain or simplify the diagram to avoid confusion
3. Consider adding a note that both `init()` and `Database::new()` run migrations, ensuring schema is always up-to-date
