# Snapshot Module Architecture Review

## Verified Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| SnapshotOptions has `max_files`, `max_file_bytes`, `max_total_bytes` with defaults 5_000, 1MB, 20MB | VERIFIED | `src/snapshot/mod.rs:8-23` matches exactly |
| `FileSnapshot` has `path`, `content`, `hash`, `timestamp` fields | VERIFIED | `src/snapshot/mod.rs:25-31` matches exactly |
| `Snapshot` struct has `id`, `session_id`, `created_at`, `label`, `data` (JSON serialized) | VERIFIED | `src/snapshot/mod.rs:33-40` matches exactly |
| `SnapshotView` has `files: HashMap<String, FileSnapshot>` | VERIFIED | `src/snapshot/mod.rs:42-49` matches exactly |
| `SnapshotManager::new()` takes `(pool, project_root)` | VERIFIED | `src/snapshot/mod.rs:58-64` matches |
| `SnapshotManager::new_with_options()` takes `(pool, project_root, options)` | VERIFIED | `src/snapshot/mod.rs:66-72` matches |
| `capture()` is `&mut self` | VERIFIED | `src/snapshot/mod.rs:74` shows `&mut self` |
| `capture_incremental()` is `&self` | VERIFIED | `src/snapshot/mod.rs:110` shows `&self` |
| `capture_incremental()` takes `file_changes: Vec<(String, Option<String>)>` | VERIFIED | `src/snapshot/mod.rs:114` matches |
| `restore()` signature is `&self, snapshot: &SnapshotView` | VERIFIED | `src/snapshot/mod.rs:252` matches |
| `restore_to_path()` signature is `&self, snapshot: &SnapshotView, target_path: &Path` | VERIFIED | `src/snapshot/mod.rs:276-279` matches |
| Snapshot table defined in `src/session/schema.rs` migration v13 | VERIFIED | `src/session/schema.rs:481-504` shows v13 migration |
| Snapshot table has `id`, `session_id`, `created_at`, `label`, `data` columns | VERIFIED | `src/session/schema.rs:484-491` matches schema |
| Index `snapshot_session_idx` on `session_id` exists | VERIFIED | `src/session/schema.rs:498` matches |
| Path traversal prevention uses `canonicalize()` check | VERIFIED | `src/snapshot/mod.rs:289-295` matches description |
| Diff module has `FileDiff`, `DiffHunk`, `DiffLine`, `DiffKind` types | VERIFIED | `src/snapshot/diff.rs:3-27` matches exactly |
| `diff_files()` and `format_unified_diff()` functions exist | VERIFIED | `src/snapshot/diff.rs:29-144` both exist |
| Snapshot config supports `max_files`, `max_file_bytes`, `max_total_bytes` | VERIFIED | `src/config/schema.rs:68-82` |
| `capture_incremental()` returns `Option<SnapshotView>` | VERIFIED | Implementation at `src/snapshot/mod.rs:115` |
| Integration with AgentLoop via `capture_snapshot_if_needed()` | VERIFIED | `src/agent/loop.rs:1559-1576` |
| `drain_file_change_events()` returns `Vec<(String, Option<String>)>` | VERIFIED | `src/agent/loop.rs:1578-1593` |
| `capture_incremental_snapshot_if_needed()` wired to snapshot_manager | VERIFIED | `src/agent/loop.rs:1596-1614` |

## Bugs/Discrepancies Found

### Critical

1. **Error swallowing in `restore()` and `restore_to_path()`** (high priority)

   In `src/snapshot/mod.rs:272-273`:
   ```rust
   .await
   .map_err(|e| e.to_string())?
   ```
   The `?` operator only propagates join errors. If `spawn_blocking` succeeds (Returns `Ok(...)`) but the inner closure returns `Err(...)`, that error is silently discarded.

   Same issue at line 308 for `restore_to_path()`.

   **Fix**:
   ```rust
   let result = tokio::task::spawn_blocking(move || { ... }).await;
   match result {
       Ok(Ok(())) => Ok(()),
       Ok(Err(e)) => Err(e),
       Err(e) => Err(format!("join error: {e}")),
   }
   ```

### Medium

2. **TOCTOU race in `restore_to_path()`** (medium priority)

   At `src/snapshot/mod.rs:289-302`, the path traversal check using `canonicalize()` happens before the file write. Between these operations, symlinks could theoretically be created/modified.

   **Fix**: Consider checking for symlink presence before write, or using `O_NOFOLLOW` in open flags.

3. **`to_relative_path()` silently falls back to absolute path** (medium priority)

   At `src/snapshot/mod.rs:243-250`, if `strip_prefix` fails, the function returns the absolute path without logging. This could cause restore issues if paths are stored inconsistently.

   **Fix**: Log a warning when fallback occurs.

4. **No validation for zero limits in `SnapshotOptions`** (low-medium priority)

   If `max_files: 0` is set, `collect_files_sync()` returns empty immediately at line 342. No error is returned, so the caller may think capture succeeded with zero files.

   **Fix**: Validate in `new_with_options()` that limits are > 0.

## Improvement Suggestions

### High Priority

1. **Add unit tests for `restore()` and `restore_to_path()`**
   - No tests exist in `src/snapshot/` (confirmed via grep for `#[test]`)
   - Missing: test for restore functionality, path traversal prevention, delete operations

2. **Add test for binary/invalid UTF-8 content in `capture_incremental()`**
   - `capture()` validates UTF-8 at line 393
   - `capture_incremental()` passes through content without validation

3. **Extract duplicated restore logic into shared helper**
   - `restore()` (lines 258-274) and `restore_to_path()` (lines 286-309) share nearly identical file-writing logic

### Medium Priority

4. **Add logging to snapshot operations**
   - `capture()`, `restore()`, etc. don't log their operations
   - `AgentLoop` already logs snapshot capture (line 1565), but module itself doesn't

5. **Document snapshot lifecycle/retention policy**
   - No information on when snapshots are cleaned up
   - No retention limits configured

6. **Configuration schema not documented in architecture**
   - `src/config/schema.rs:68-82` shows `SnapshotConfig` but architecture doc doesn't explicitly list it

### Low Priority

7. **Use async file I/O (`tokio::fs`) instead of `spawn_blocking`**
   - Current implementation uses sync `std::fs` with `spawn_blocking`
   - For many small files, `tokio::fs` may be more efficient

8. **Consider parallel file writes in `restore()`**
   - Currently sequential; parallel writes could improve performance for large snapshots

9. **Document magic numbers in `collect_files_sync()`**
   - Hardcoded limits and exclusion directory names (`.git`, `node_modules`, etc.) could be named constants

## Summary

The architecture document at `architecture/snapshot.md` is **accurate and up-to-date**. All types, method signatures, and behaviors match the implementation. The skill guide at `.opencode/skills/snapshot/SKILL.md` is also accurate.

**Key finding**: The only significant bug is the error swallowing issue in `restore()` and `restore_to_path()` where inner operation errors are discarded. This should be fixed to ensure partial restore failures are properly reported.

**No documentation updates required** - existing documentation correctly reflects the implementation.