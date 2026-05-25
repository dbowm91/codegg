# Snapshot Module Architecture Review

**Review date**: 2026-05-25
**Reviewer**: Architecture review

## Verified Correct Items

- **SnapshotOptions** (lines 17-26): All fields match (`max_files`, `max_file_bytes`, `max_total_bytes` with correct defaults)
- **FileSnapshot** (lines 33-40): All fields match (`path`, `content`, `hash`, `timestamp`)
- **Snapshot** (lines 47-54): All fields match (`id`, `session_id`, `created_at`, `label`, `data`)
- **SnapshotView** (lines 61-68): All fields match (`id`, `session_id`, `files: HashMap`, `created_at`, `label`)
- **SnapshotManager::new()** signature matches
- **SnapshotManager::new_with_options()** signature matches
- **restore()** signature matches (takes `&SnapshotView`, not `&Snapshot`)
- **restore_to_path()** signature matches
- **delete_snapshot()** and **delete_all_for_session()** signatures match
- **Database schema** (lines 187-197): SQL matches migration v13 in `src/session/schema.rs:484-498`
- **File traversal prevention** logic described correctly (lines 201-213)
- **Atomic write pattern** in `restore_to_path()` described correctly (lines 215-227)
- **Diff module types** (lines 229-258): All types (`FileDiff`, `DiffHunk`, `DiffLine`, `DiffKind`) and functions (`diff_files`, `format_unified_diff`) match
- **Configuration section** (lines 260-273): Config schema with `snapshot` and `snapshot_config` matches
- **Excluded directories** (line 121): `.git`, `node_modules`, `target`, `.codegg` matches `collect_files_sync()` at `src/snapshot/mod.rs:384`
- **Snapshot table location note** (line 184): Correctly notes table is defined in `src/session/schema.rs` migration v13

## Incorrect/Stale Items

1. **Integration with AgentLoop section (lines 155-180)**: The code example shows inline `impl AgentLoop` with `capture_snapshot_if_needed` and `capture_incremental_snapshot_if_needed`, but the actual implementation is in `src/agent/loop.rs` at different line numbers:
   - `capture_snapshot_if_needed`: line 1559-1576
   - `capture_incremental_snapshot_if_needed`: line 1596-1620
   - `drain_file_change_events()`: line 1578-1594 (not shown in doc)

2. **capture() takes `&mut self`**: Documentation shows `pub async fn capture(&mut self, ...)` at line 83 - this is correct, but the `new_with_options()` validation (lines 67-75) warns on zero values is not documented.

3. **capture_incremental() takes `&self`**: Documentation shows `capture_incremental` at line 84 with `&self` - matches source.

## Bugs Found in Related Code

No bugs found. All method signatures, struct fields, and SQL schema are accurate.

## Line Numbers Needing Updates

| Section | Lines | Update Required |
|---------|-------|-----------------|
| Integration with AgentLoop | 155-180 | Update line references to actual locations in `src/agent/loop.rs` (1559-1620) or remove inline example and reference the source file |
| Excluded directories | 121 | Line number for `collect_files_sync()` in `src/snapshot/mod.rs` is 384, not shown in doc |
| Snapshot table definition | 184 | Line number 484 in `src/session/schema.rs` (was previously documented at different line) |

## Summary

The architecture document is **largely accurate**. The main issues are:
1. The Integration with AgentLoop section shows an inline code example with approximate line references rather than pointing to the actual source location
2. Some internal implementation details (zero-value warnings in `new_with_options`) are not documented but are minor

The document correctly describes all public API types, methods, database schema, security mechanisms, and configuration options.