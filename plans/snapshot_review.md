# Snapshot Module Review

## Summary

Reviewed `architecture/snapshot.md`, `src/snapshot/mod.rs`, `src/snapshot/diff.rs`, `.opencode/skills/snapshot/SKILL.md`, and integration points in `src/agent/loop.rs`. The architecture document is **largely accurate** but has several minor discrepancies and gaps. The skill guide exists at v1.1.0 but was never linked from the architecture doc.

---

## Verified Correct Items

| Item | Status |
|------|--------|
| `SnapshotOptions` fields (`max_files`, `max_file_bytes`, `max_total_bytes`) with defaults | Verified |
| `FileSnapshot` struct (path, content, hash, timestamp) | Verified |
| `Snapshot` struct (`data: String` - JSON serialized) | Verified |
| `SnapshotView` struct (`files: HashMap<String, FileSnapshot>`) | Verified |
| `SnapshotManager::new()` and `new_with_options()` signatures | Verified |
| `capture()`, `capture_incremental()`, `get()`, `list_for_session()`, `latest()` | Verified |
| `restore()`, `restore_to_path()`, `delete_snapshot()`, `delete_all_for_session()` | Verified |
| Path traversal prevention using `canonicalize()` check | Verified (2026-05-23 fix) |
| Database table in `src/session/schema.rs` migration v13 | Verified |
| Snapshot table schema (id, session_id, created_at, label, data) with index | Verified |
| Diff types (`FileDiff`, `DiffHunk`, `DiffLine`, `DiffKind`) | Verified |
| `diff_files()` and `format_unified_diff()` functions | Verified |
| `restore_to_path()` uses atomic write (temp file + rename) | Verified |
| File count and size limits enforced in `collect_files_sync()` | Verified |
| Skips `.git`, `node_modules`, `target`, `.codegg` directories | Verified |
| Skips files larger than `max_file_bytes` | Verified |
| `old_content: Option<String>` available in `FileChanged` events | Verified |

---

## Discrepancies

### 1. **restore() is never called in AgentLoop** (Medium)
- **Architecture**: `loop.rs:118` and `loop.rs:155` show `SnapshotManager::restore(snapshot_view)` called on error
- **Actual Code**: `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()` exist, but **no code path calls `restore()`** on error. The snapshots are captured but never used for rollback.
- **Location**: `src/agent/loop.rs:1559-1624`
- **Impact**: Snapshot capture is wired but restore-on-error is not. The safety net promised by the architecture does not exist.
- **Recommendation**: Either implement error-triggered restore or remove the restore flow from the architecture diagram.

### 2. **capture_incremental signature mismatch** (Low - Docs Correct, Code Allows More)
- **Architecture**: `file_changes: Vec<(String, Option<String>)>` with note "For each (path, old_content)"
- **Actual Code**: The implementation at `mod.rs:123` accepts `Vec<(String, Option<String>)>`. The `None` variant is skipped (line 129: `let Some(content) = old_content else { continue; }`), so it correctly behaves as documented.
- **Status**: Code is more general but behaves correctly.

### 3. **restore() uses blocking I/O in spawn_blocking but does not handle errors** (Low)
- **Location**: `mod.rs:291`
- **Issue**: `std::fs::write()` can fail after the path traversal check passes (e.g., permission denied, disk full). The error message is returned but the function continues processing remaining files.
- **Status**: Minor - errors are collected but partial restoration can occur.

### 4. **Integration flow references non-existent code paths** (Medium)
- **Architecture** lines 97-119 show error-triggered restore: `If error → SnapshotManager::restore(snapshot_view)`
- **Architecture** lines 139-156 show same error-restore flow for full capture
- **Actual**: No error handler calls restore. The flow ends at `capture_snapshot_if_needed()`.
- **Recommendation**: Update architecture to reflect actual two-phase capture (pre-execution capture + post-execution incremental capture) without the restore step.

### 5. **Skill version outdated** (Low)
- **Skill**: v1.1.0, last updated 2026-05-23
- **AGENTS.md** shows v1.1.0 as current
- Status: Skill is up to date.

---

## Documentation Gaps

### 1. Missing from architecture/snapshot.md
- No mention that `restore()` is not currently called by AgentLoop
- `restore_to_path()` atomic write technique (temp file + rename) not documented
- Error handling in restore operations not documented (partial failure possible)
- `collect_files_sync()` limits and exclusions not documented

### 2. Missing from SKILL.md
- No information about atomic write in `restore_to_path()`
- `delete_snapshot()` and `delete_all_for_session()` not listed in API
- No mention of config schema integration (`snapshot` and `snapshot_config` in config)

---

## Bugs and Issues in Code

### 1. **restore() continues after write failure** (Low)
- **File**: `src/snapshot/mod.rs:291-292`
- **Issue**: If `std::fs::write()` fails, the error is logged but processing continues to the next file
- **Impact**: Partial restore possible without clear indication
- **Recommendation**: Either fail fast on first error or document that partial restoration can occur

### 2. **restore_to_path() has race condition window** (Low)
- **File**: `src/snapshot/mod.rs:318-319`
- **Issue**: Between `canonicalize()` check and `std::fs::write()`, the file system could change (TOCTOU)
- **Status**: Unavoidable without OS-level support; acceptable risk

### 3. **Test file has dead code** (Very Low)
- **File**: `tests/snapshot.rs`
- `create_test_manager()` defined at line 15 but `create_test_manager_with_pool()` at line 79 is used by all tests
- `create_test_manager()` is dead code (lines 15-22)
- **Recommendation**: Remove `create_test_manager()` or use it

---

## Recommendations

### For Architecture Document
1. Remove the error-restore flows from diagrams (lines 97-119, 139-156) since `restore()` is never called
2. Document that `restore()` and `restore_to_path()` exist but are not yet integrated into the agent error-handling loop
3. Document the atomic write pattern in `restore_to_path()`
4. Document `collect_files_sync()` exclusions and limits
5. Add link to `.opencode/skills/snapshot/SKILL.md`

### For Code
1. Consider integrating `restore()` into the error-handling path if snapshot rollback is intended
2. Consider adding a failure flag to stop processing remaining files on error in restore
3. Clean up dead `create_test_manager()` function in tests

### For Skill
1. Add `delete_snapshot()` and `delete_all_for_session()` to API listing
2. Add note about atomic write in `restore_to_path()`
3. Add config integration details (`snapshot`, `snapshot_config`)

---

## File References

| File | Lines | Issue |
|------|-------|-------|
| `src/snapshot/mod.rs` | 267-299 | `restore()` never called from AgentLoop |
| `src/snapshot/mod.rs` | 291-292 | Continues after write failure |
| `src/snapshot/mod.rs` | 301-340 | `restore_to_path()` atomic write not documented |
| `src/agent/loop.rs` | 1559-1624 | Capture methods exist but restore not wired |
| `architecture/snapshot.md` | 97-119 | Error-restore flow does not exist in code |
| `architecture/snapshot.md` | 139-156 | Error-restore flow does not exist in code |
| `tests/snapshot.rs` | 15-22 | Dead `create_test_manager()` function |
