# Review: Batch 5 - Session, Storage, Snapshot, and Git

**Reviewed**: 2026-05-28
**Files**: architecture/session.md, architecture/storage.md, architecture/snapshot.md, architecture/git.md, architecture/worktree.md

## Summary

The documentation across these five files is largely accurate. The database schema tables, column definitions, migration versions (v1-v15), and struct definitions all match the source code. The primary issues found are: (1) `storage.md` shows an incorrect `init()` code example that doesn't match the actual implementation, (2) `session.md` has a wrong tool name in the `redact_for_export` list, and (3) several public methods in `SessionStore` are undocumented. Line number references in `worktree.md` are confirmed correct (172/180).

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | `session.md` | 524 | `redact_for_export` lists "tail" as a redacted tool name | UPDATE: Code uses "terminal" not "tail" (`src/session/import.rs:133`) |
| 2 | `session.md` | 357-387 | SessionStore methods table is incomplete | UPDATE: Missing `create_from_template` (line 130), `list_all_with_offset` (line 239), `list_deleted` (line 720), `set_tags` (line 732) |
| 3 | `storage.md` | 38-45 | `init()` code example shows calling `Database::new()` then `session::schema::migrate()` | UPDATE: Actual `init()` calls `connect_and_configure()` directly, never calls `Database::new()` or `migrate()` separately (`src/storage/mod.rs:85-129`) |
| 4 | `storage.md` | 119 | v15 described as "Additional fields" | UPDATE: v15 creates the `usage` table for token/cost tracking (session.md already has this correct) |
| 5 | `snapshot.md` | 229-258 | Diff module types listed but `DiffKind` enum variants not shown in doc | CONFIRMED: Variants (Context, Added, Removed) match `src/snapshot/diff.rs:22-27` |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | session | `redact_for_export` tool list contains "tail" which doesn't exist as a tool name | `src/session/import.rs:133` - uses "terminal" instead | Low (cosmetic, redact still works for "terminal") |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | session | Document `create_from_template`, `list_all_with_offset`, `list_deleted`, `set_tags` methods | Completes SessionStore API documentation |
| 2 | storage | Document the two code paths: `Database::new()` (runs migrations) vs `init()` (does NOT run migrations) | Prevents confusion about when migrations execute |
| 3 | snapshot | Document that `diff_files` always returns a single-element Vec<FileDiff> | Clarifies API contract for consumers |
| 4 | snapshot | Add note that `capture_incremental()` returns `Ok(None)` when no valid file changes are provided | Documents edge case behavior |
| 5 | worktree | Consider documenting `find_git_root` behavior when starting path is already a git root | Clarifies edge case (returns current dir immediately) |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | `storage.md` | Code example showing `init()` calling `session::schema::migrate()` | Misleading - `init()` never calls migrate; only `Database::new()` does |

## Verification Results

| Claim | Status | Source |
|-------|--------|--------|
| 15 migration versions (v1-v15) | CONFIRMED | `src/session/schema.rs` lines 25-69, 81-97 |
| Session struct fields | CONFIRMED | `src/session/models.rs:6-28` matches exactly |
| PartData enum: Text, Reasoning, ToolCall, Image, File | CONFIRMED | `src/session/message.rs:38-59` - 5 variants |
| ToolStatus: Pending, Running, Completed, Error | CONFIRMED | `src/session/message.rs:63-69` - 4 variants |
| SessionStatus: Idle, Busy, Error, Compacting, Exporting | CONFIRMED | `src/session/status.rs:5-12` - 5 variants |
| WAL mode + 8 pragmas | CONFIRMED | `src/storage/mod.rs:66-76` - all 8 match |
| Max connections = 10, acquire_timeout = 30s | CONFIRMED | `src/storage/mod.rs:60-61` |
| SHA256 used for snapshot hashing | CONFIRMED | `src/snapshot/mod.rs:143,421,435` - sha2::Sha256 used consistently |
| Atomic write (temp + rename) in restore | CONFIRMED | `src/snapshot/mod.rs:292-298` and `:335-339` |
| Snapshot excluded dirs: .git, node_modules, target, .codegg | CONFIRMED | `src/snapshot/mod.rs:389` |
| Import limits: 100K msgs, 500K parts, 500MB | CONFIRMED | `src/session/import.rs:68-70` |
| CheckpointStore: save, load, load_latest, list, delete, delete_all, has_checkpoint | CONFIRMED | `src/session/checkpoint.rs:53-147` - 7 methods |
| GitSession struct fields | CONFIRMED | `src/git/mod.rs:7-13` - 5 fields match |
| GitStatus struct fields | CONFIRMED | `src/git/mod.rs:16-21` - 4 fields match |
| Worktree struct: path, branch, is_current, is_detached | CONFIRMED | `src/worktree/mod.rs:8-13` |
| `is_git_file()` at line 172, `is_git_worktree()` at line 180 | CONFIRMED | `src/worktree/mod.rs:172,180` - exact match |
| DiffKind enum: Context, Added, Removed | CONFIRMED | `src/snapshot/diff.rs:23-27` |
| redact_for_export tool names: bash, write, read, edit, replace, multiedit, tail, git, webfetch, apply_patch | UPDATE | "tail" should be "terminal" (`src/session/import.rs:133`) |
| storage.md `init()` returns `Result<SqlitePool, StorageError>` | CONFIRMED | `src/storage/mod.rs:85` |
| storage.md `init()` calls `Database::new()` | UPDATE | Actually calls `connect_and_configure()` directly, no `Database::new()` |
| SessionStore: SessionStore lines 44-1548 | CONFIRMED (approx) | `impl SessionStore` starts at line 48, ends around line 1550 |
| TodoStore: lines 1550-1753 | CONFIRMED (approx) | `impl TodoStore` at line 1554, ends around line 1757 |
| MessageStore: lines 1755-1878 | CONFIRMED (approx) | `impl MessageStore` at line 1759, ends around line 1882 |
| PartStore: lines 1880-1996 | CONFIRMED (approx) | `impl PartStore` at line 1884, ends around line 2000 |
| PermissionStore: lines 1998-2061 | CONFIRMED (approx) | `impl PermissionStore` at line 2002, ends around line 2065 |
| UsageStore: lines 2063-2192 | CONFIRMED (approx) | `impl UsageStore` at line 2067, ends at line 2192 |

## Notes

- The `snapshot.md` documentation correctly notes that automatic rollback on tool failure is NOT implemented - verified against source.
- The `worktree.md` note about `is_locked` and `is_main` not being implemented is correct - Worktree struct only has 4 fields.
- The `git.md` claim that all git commands use `env_clear()` with only `PATH` is confirmed across all Command invocations in `src/git/mod.rs`.
- The `session.md` import flow description matches the actual implementation in `store.rs` (validate, deserialize, transaction, remap IDs).
