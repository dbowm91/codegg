# Snapshot Architecture Review

## Architecture Document
- Path: architecture/snapshot.md

## Source Code Location
- src/snapshot/

## Verification Summary
**Partial Pass** - The architecture document is largely accurate but has several undocumented features and one inconsistency with the restore flow.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| SnapshotOptions struct with max_files, max_file_bytes, max_total_bytes | Pass | Exact match at src/snapshot/mod.rs:9-12 |
| SnapshotOptions defaults (5000, 1MB, 20MB) | Pass | Exact match at mod.rs:17-21 |
| FileSnapshot struct (path, content, hash, timestamp) | Pass | Exact match at mod.rs:26-31 |
| Snapshot struct with data as JSON string | Pass | Exact match at mod.rs:34-40 |
| SnapshotView struct with files HashMap | Pass | Exact match at mod.rs:43-49 |
| SnapshotManager::new(pool, project_root) | Pass | Exact match at mod.rs:58-64 |
| SnapshotManager::new_with_options(...) | Pass | Exact match at mod.rs:66-81 |
| capture() signature and behavior | Pass | Returns SnapshotView, stores files as JSON |
| capture_incremental() signature | Pass | Matches at mod.rs:119-179 |
| get() signature and behavior | Pass | Returns Option<SnapshotView> |
| list_for_session() signature | Pass | Returns Vec<SnapshotView> |
| latest() signature | Pass | Returns Option<SnapshotView> |
| restore() signature | Pass | Takes SnapshotView, not id as in old doc |
| restore_to_path() with path traversal prevention | Pass | Exact match with canonicalize check at mod.rs:305-332 |
| delete_snapshot() signature | Pass | Returns Result<(), String> |
| delete_all_for_session() signature | Pass | Returns Result<(), String> |
| collect_files_sync() excludes .git, node_modules, target, .codegg | Pass | Exact match at mod.rs:376-378 |
| Database schema in src/session/schema.rs migration v13 | Pass | Exact match at schema.rs:481-503 |
| Diff module with FileDiff, DiffHunk, DiffLine, DiffKind | Pass | Exact match at diff.rs:4-27 |
| diff_files() function | Pass | Signature matches at diff.rs:29-128 |
| format_unified_diff() function | Pass | Signature matches at diff.rs:130-144 |
| Configuration via snapshot/snapshot_config | Pass | Matches schema.rs:53-54 |

## Issues Found

### Bugs
- **None identified** - Core implementation is correct and matches documentation

### Inconsistencies
1. **restore() flow is reversed**: Architecture doc shows "Tool execution → capture → execute tool → if error restore" (lines 100-116). But actual implementation calls `capture_snapshot_if_needed()` BEFORE tool execution (loop.rs:1655), then `capture_incremental_snapshot_if_needed()` AFTER (loop.rs:1853). The restore would need to happen via explicit user action, not automatically on error.

2. **restore_to_path() uses different write strategy**: Uses atomic rename via temp file (mod.rs:322-326) which is more robust than plain write used by restore(). This is undocumented - it's actually a better approach.

### Missing Documentation
1. **SnapshotManager::to_relative_path()**: Private helper method not documented (mod.rs:252-265)
2. **collect_files_sync()**: Private function not documented (mod.rs:354-441)
3. **Default trait implementation for SnapshotConfig**: Not documented (schema.rs:74+)
4. **zero-value validation in new_with_options()**: Logs warnings when max_files/max_file_bytes/max_total_bytes are 0, treating 0 as 1 (mod.rs:67-75)
5. **empty file handling**: collect_files_sync() creates FileSnapshot with empty content and empty string hash for empty files (mod.rs:397-413)
6. **utf-8 validation**: collect_files_sync() skips non-UTF8 files (mod.rs:417-419)
7. **AgentLoop subscribes to FileChanged events**: file_change_rx at loop.rs:668 for incremental snapshots
8. **restore_to_path() atomic write**: Uses temp file + rename for atomic writes, not just path validation (mod.rs:322-326)

### Improvement Opportunities
1. **restore() could use atomic writes too**: The basic restore() uses plain std::fs::write while restore_to_path() uses atomic rename. Consistency would be better.
2. **restore() could validate paths**: restore() does NOT check if files would escape project_root, but restore_to_path() does. This is an asymmetry - restore_to_path is safer but undocumented as such.
3. **collect_files_sync error handling**: Silently skips unreadable directories and files. Could be logged at trace level.
4. **SnapshotManager new_with_options() zero validation**: Logs warnings but proceeds - could be more consistent to just use Default when 0.

## Recommendations
1. Update architecture/snapshot.md to show correct capture flow: capture BEFORE tool execution, incremental AFTER
2. Document restore_to_path() as using atomic writes (better than restore())
3. Add SnapshotManager::to_relative_path() to API docs
4. Consider documenting the subscription to FileChanged events for incremental captures
5. Consider adding path validation to restore() for consistency with restore_to_path()
6. Update SKILL.md to reflect actual usage patterns
