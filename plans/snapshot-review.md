# Snapshot Module Architecture Review

## Verification Results

### Claims (table format: Claim | Status | Evidence)

| Claim | Status | Evidence |
|-------|--------|----------|
| SnapshotOptions has `max_files`, `max_file_bytes`, `max_total_bytes` with defaults 5_000, 1MB, 20MB | VERIFIED | `src/snapshot/mod.rs:8-23` matches exactly |
| `FileSnapshot` has `path`, `content`, `hash`, `timestamp` fields | VERIFIED | `src/snapshot/mod.rs:25-31` matches exactly |
| `Snapshot` struct has `id`, `session_id`, `created_at`, `label`, `data` (JSON serialized) | VERIFIED | `src/snapshot/mod.rs:33-40` matches exactly |
| `SnapshotView` has `files: HashMap<String, FileSnapshot>` | VERIFIED | `src/snapshot/mod.rs:42-49` matches exactly |
| `SnapshotManager::new()` takes `(pool, project_root)` | VERIFIED | `src/snapshot/mod.rs:58-64` matches |
| `SnapshotManager::new_with_options()` takes `(pool, project_root, options)` | VERIFIED | `src/snapshot/mod.rs:66-72` matches |
| `capture()` is `&mut self` | VERIFIED | `src/snapshot/mod.rs:74-108` shows `&mut self` |
| `capture_incremental()` is `&self` | VERIFIED | `src/snapshot/mod.rs:110-170` shows `&self` |
| `capture_incremental()` takes `file_changes: Vec<(String, Option<String>)>` | VERIFIED | `src/snapshot/mod.rs:114` matches |
| `restore()` signature is `&self, snapshot: &SnapshotView` | VERIFIED | `src/snapshot/mod.rs:252-274` matches |
| `restore_to_path()` signature is `&self, snapshot: &SnapshotView, target_path: &Path` | VERIFIED | `src/snapshot/mod.rs:276-309` matches |
| Snapshot table defined in `src/session/schema.rs` migration v13 | VERIFIED | `src/session/schema.rs:481-504` shows v13 migration |
| Snapshot table has `id`, `session_id`, `created_at`, `label`, `data` columns | VERIFIED | `src/session/schema.rs:484-491` matches schema |
| Index `snapshot_session_idx` on `session_id` exists | VERIFIED | `src/session/schema.rs:498` matches |
| Path traversal prevention uses `canonicalize()` check | VERIFIED | `src/snapshot/mod.rs:289-295` matches description |
| Diff module has `FileDiff`, `DiffHunk`, `DiffLine`, `DiffKind` types | VERIFIED | `src/snapshot/diff.rs:3-27` matches exactly |
| `diff_files()` and `format_unified_diff()` functions exist | VERIFIED | `src/snapshot/diff.rs:29-144` both exist |
| Snapshot config supports `max_files`, `max_file_bytes`, `max_total_bytes` | VERIFIED | Config schema in `src/config/schema.rs:53-54` |
| `capture_incremental()` returns `Option<SnapshotView>` | VERIFIED | Implementation at `src/snapshot/mod.rs:115` returns `Result<Option<SnapshotView>, String>` |

## Bugs Found

### Critical

**None identified**

### High

1. **Restore error swallows original error**: In `restore()` and `restore_to_path()`, when `spawn_blocking` returns an `Err(e)`, it's converted to string with `e.to_string()` at line 273 and 308. However, the `Ok(())` from `spawn_blocking` is not explicitly returned - it falls through. If the spawn succeeds but the inner operation returned an `Err(...)`, that error is lost because `map_err` only handles join errors, not the inner result.

```rust
// Line 258-273: restore()
tokio::task::spawn_blocking(move || {
    for (rel_path, file_snapshot) in files {
        // ... if this returns Err, it's discarded
    }
    Ok(())
})
.await
.map_err(|e| e.to_string())?  // Only handles join errors, not inner errors
```

The `?` operator only propagates join errors. If `spawn_blocking` succeeds but the closure returns `Err(...)`, it's discarded.

**Fix**: Should check the inner result:
```rust
let result = tokio::task::spawn_blocking(move || { ... }).await;
match result {
    Ok(Ok(())) => Ok(()),
    Ok(Err(e)) => Err(e),
    Err(e) => Err(format!("join error: {e}")),
}
```

2. **TOCTOU race in `restore_to_path()` path validation**: Path traversal check happens at line 289-295, but file write happens at line 302. Between canonicalize and write, symlinks could be created/modified (though OS-level protections may apply). This is a low-probability race but still a bug.

**Fix**: Consider checking for symlink presence before write or using `O_NOFOLLOW` in open flags.

### Medium

3. **`to_relative_path()` silently falls back to absolute path**: Line 243-250 uses `strip_prefix` and falls back to absolute path if prefix doesn't match. This means if `project_root` changes or has different path representation, files could be stored with absolute paths, breaking restore.

**Fix**: Log warning when fallback occurs, or normalize paths consistently.

4. **Empty file hash is incorrect**: Line 384 uses `md5::compute([])` for empty files, but line 398 uses `md5::compute(content.as_bytes())`. While both are technically valid MD5, there's inconsistency in that empty file content doesn't use the same code path. However, this is by design since empty files skip the read/content path. Not a bug, but worth documenting.

5. **No validation on `max_files`, `max_file_bytes`, `max_total_bytes`**: If someone sets `max_files: 0`, the early return at line 342 would cause immediate return with empty files. No error is returned.

**Fix**: Validate options in `new_with_options()` or `collect_files_sync()`.

6. **`collect_files_sync` exits early on limits without returning partial results**: At line 342-343, if `max_files` or `max_total_bytes` is exceeded during iteration, it returns the `files` collected so far. This is correct behavior, but the function name suggests complete collection. Could be confusing.

**Fix**: The current behavior is actually correct for incremental capture - partial snapshots are useful.

## Improvement Suggestions

### Performance

1. **Use async file I/O instead of `spawn_blocking`**: All file operations in `restore()` and `restore_to_path()` use `spawn_blocking` with sync `std::fs`. While this is correct for blocking operations, consider using `tokio::fs` for better integration with async runtime, especially for many small files.

2. **Parallel file writes in `restore()`**: Currently files are written sequentially. For snapshots with many files, parallel writes could be significantly faster.

3. **Streaming JSON deserialization**: `serde_json::from_str` loads entire snapshot into memory. For very large snapshots, consider using a streaming JSON parser (e.g., `serde_json::from_slice` with manual parsing, or `simdjson`).

### Correctness

1. **Missing unit tests for `restore()` and `restore_to_path()`**: The test file has no tests for restore functionality, only capture tests.

2. **No test for path traversal prevention**: No test verifying that `restore_to_path()` actually rejects malicious paths like `../../etc/passwd`.

3. **Missing test for `delete_snapshot()` and `delete_all_for_session()`**: No verification these actually delete data.

4. **No test for binary file handling in `capture_incremental()`**: The `capture()` function properly skips binary files (UTF-8 validation at line 393), but `capture_incremental()` passes through whatever content is provided without validation. If `old_content` contains invalid UTF-8, it will be stored as-is.

### Maintainability

1. **Duplicated code between `restore()` and `restore_to_path()`**: Both functions have nearly identical file-writing logic (lines 259-269 and 286-303). Extract to helper method.

2. **Error message format inconsistency**: `restore()` uses `format!("failed to write {}: {}", ...)` while `restore_to_path()` uses same format. This is fine, but could be centralized.

3. **No logging of snapshot operations**: `capture()`, `restore()`, etc. don't log their operations. Adding tracing would help debugging.

4. **Magic numbers**: The hardcoded limits (5_000, 1_000_000, 20_000_000) in `SnapshotOptions::default()` are not documented. Consider adding constants with documentation.

5. **Missing documentation on snapshot lifecycle**: When are snapshots cleaned up? What's the retention policy? Documentation doesn't address cleanup.

## Priority Actions (top 5 items to fix)

1. **Fix error propagation bug in `restore()` and `restore_to_path()`** - Inner operation errors are being silently discarded. This could cause partial restores to appear successful.

2. **Add missing tests for restore functionality** - Critical missing coverage for security-sensitive path traversal prevention and restore operations.

3. **Add test for binary/invalid UTF-8 content in `capture_incremental()`** - Ensure invalid content is handled properly.

4. **Extract duplicated restore logic into shared helper** - Reduce code duplication between `restore()` and `restore_to_path()`.

5. **Add validation for `SnapshotOptions`** - Prevent edge cases like `max_files: 0` causing silent empty captures.

## Additional Notes

- Architecture documentation is accurate and up-to-date
- Skill guide (.opencode/skills/snapshot/SKILL.md) correctly documents the implementation
- Code quality is generally good with proper error handling patterns
- The `diff.rs` module is well-implemented with proper context window handling
- Integration with AgentLoop is properly wired via `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()`