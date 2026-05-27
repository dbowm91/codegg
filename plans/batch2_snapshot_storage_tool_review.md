# Snapshot & Storage & Tool Architecture Review

## Verified Claims

### Snapshot Module
- **Location**: `src/snapshot/` - CORRECT
- **SnapshotOptions struct**: Fields `max_files`, `max_file_bytes`, `max_total_bytes` with defaults 5_000, 1_000_000, 20_000_000 - CORRECT
- **FileSnapshot struct**: Fields `path`, `content`, `hash`, `timestamp` - CORRECT
- **Snapshot struct**: Fields `id`, `session_id`, `created_at`, `label`, `data` - CORRECT
- **SnapshotView struct**: Fields `id`, `session_id`, `files` (HashMap), `created_at`, `label` - CORRECT
- **SnapshotManager fields**: `pool`, `project_root`, `options` - CORRECT
- **SnapshotManager methods exist and signatures match**: `new`, `new_with_options`, `capture`, `capture_incremental`, `get`, `list_for_session`, `latest`, `restore`, `restore_to_path`, `delete_snapshot`, `delete_all_for_session` - CORRECT
- **File collection excludes**: `.git`, `node_modules`, `target`, `.codegg` - CORRECT
- **Database schema in schema.rs v13**: CORRECT
- **Path traversal prevention**: Implemented correctly in both `restore()` and `restore_to_path()` - CORRECT
- **Atomic write pattern in restore_to_path()**: Uses temp file + rename - CORRECT

### Storage Module
- **Location**: `src/storage/` - CORRECT
- **Database struct**: Single field `pool: SqlitePool` - CORRECT
- **Database methods exist**: `new`, `pool`, `migrate`, `health_check`, `close` - CORRECT
- **init function path resolution**: `{project_dir}/.codegg/sessions.db` or `~/.config/codegg/sessions.db` - CORRECT
- **SQLite pragmas**: All 8 pragmas correctly listed (journal_mode=WAL, wal_autocheckpoint=1000, busy_timeout=5000, synchronous=NORMAL, mmap_size=268435456, cache_size=-2000, temp_store=MEMORY, foreign_keys=ON) - CORRECT
- **Connection pool**: max_connections=10, acquire_timeout=30s - CORRECT
- **Migrations in session::schema**: migrate() calls session::schema::migrate() - CORRECT

### Tool Module
- **Location**: `src/tool/` - CORRECT
- **Tool trait at lines 54-60**: Signature `fn name()`, `fn description()`, `fn parameters()`, `async fn execute()` - CORRECT
- **ToolRegistry takes `&dyn Tool`**: Line 122 `pub fn register(&mut self, tool: impl Tool + 'static)` - CORRECT
- **ToolResult struct**: Fields `tool_name`, `input`, `output`, `success` - CORRECT
- **File structure summary**: All files listed exist except `teams.rs` is NOT in mod.rs - CORRECT (for non-teams files)
- **ToolError variants**: NotFound, Execution, Timeout, Permission, Format, Disabled, Io, Network - CORRECT
- **ToolExecutor deprecated**: Deprecation note at line 8 - CORRECT
- **with_defaults() count**: 26 direct register calls + 1 search_tool = 27 total - CORRECT

## Incorrect/Stale Claims

### Snapshot Module
1. **Inconsistent hash algorithm**: `snapshot.md:431` says "Uses MD5 in `collect_files_sync` (line 431), SHA256 elsewhere" - This is CORRECT as documented. However, line 431 in source uses MD5 for file content hashing, while incremental capture (line 143) uses SHA256. This is a documentation/expectation mismatch worth noting.

### Tool Module
1. **Tool count mismatch**: `tool.md:11` says "27 tools in `with_defaults()`" but ImageTool is NOT in with_defaults() AND is NOT in the mod.rs module declarations. Source shows only 26 direct registrations + tool_search = 27 tools, but ImageTool is missing from the registration sequence entirely despite having a complete implementation at `src/tool/image.rs`. The documentation is inconsistent:
   - Line 46 says "27 total in default registry"
   - Line 190 says "ImageTool is NOT in with_defaults()"
   - Line 11 claims 27 built-in tools

2. **ImageTool module status**: `src/tool/image.rs` exists as a complete implementation but `pub mod image` is NOT in `src/tool/mod.rs`. However, `src/tui/components/image.rs` exists for TUI image viewing. This appears to be dead code or incomplete integration.

3. **teams.rs in file structure**: `tool.md:424` lists `teams.rs` but `teams.rs` is NOT exposed via `pub mod teams` in mod.rs. It exists only as a re-export wrapper from `src/agent/teams`.

4. **tool_search registered with catalog at lines 184-185**: Documentation at line 153 says "Registration in with_defaults() (lines 89-119)" but the tool_search registration happens at lines 184-185, which is after the main registration block ends at line 119. Minor documentation placement issue.

## Bugs Found

### Snapshot Module
1. **Inconsistent hashing**: `collect_files_sync()` at line 431 uses MD5 for file hashes:
   ```rust
   let hash = format!("{:x}", md5::compute(content.as_bytes()));
   ```
   But `capture_incremental()` at line 143 uses SHA256:
   ```rust
   let hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
   ```
   Same file, same struct, different hash algorithms. Also, for empty files (line 417), it uses SHA256 of empty slice.

2. **restore() missing atomic write**: The `restore()` method (line 292) writes directly without the temp file + rename pattern that `restore_to_path()` uses. If interrupted, file could be corrupted.

### Tool Module  
1. **ImageTool never registered**: `src/tool/image.rs` exists with full implementation but is never registered. It is NOT in mod.rs module declarations and NOT in with_defaults(). Despite existing complete implementation, it cannot be used.

## Improvements Identified

### Snapshot Module
1. **Consider using single hash algorithm**: MD5 in collect_files_sync vs SHA256 in capture_incremental is confusing. MD5 is used for content hashing (not security), so either is fine, but consistency aids debugging.

2. **restore() could use atomic write**: Match the pattern in restore_to_path() for consistency and safety.

### Storage Module
1. **No issues identified** - Implementation matches documentation.

### Tool Module
1. **ImageTool integration**: Either register ImageTool in with_defaults() or remove the dead code at `src/tool/image.rs`, or properly integrate it if it's meant to be provider-gated.

2. **teams.rs documentation placement**: If tool.md wants to list teams.rs in file structure, note it's a re-export from agent module, not a directly registered tool.

## Stale References

### Snapshot Module
- No stale references found - all file paths, line numbers, and struct definitions match source.

### Storage Module
- **Migration version mismatch**: `storage.md:106` says "Migration versions v1-v14 are supported" but schema.rs shows v1-v15 (line 67-69 shows check for < 15). The usage table is outdated.

### Tool Module
- **Migration note at tool.md:119**: Says "multiedit module exists and is registered via `pub mod multiedit` in `mod.rs`" - This is TRUE, `pub mod multiedit` IS in mod.rs at line 23. So the documentation is correct.

## Recommendations

1. **Fix ImageTool registration or removal**: This is a significant gap. Either:
   - Add `pub mod image` to mod.rs and register `ImageTool::default()` in with_defaults(), OR
   - Add a comment explaining why it's intentionally excluded (feature-gated, provider-configured, etc.)

2. **Update storage.md migration version**: Change "v1-v14" to "v1-v15" to reflect actual schema.

3. **Consider unifying hash algorithm in snapshot**: Use SHA256 everywhere for consistency, or document why MD5 is acceptable for collect_files_sync.

4. **Add atomic write to restore()**: For safety, match restore_to_path() pattern.

5. **Clarify tool count in documentation**: Line 11 says "27 tools in with_defaults()", line 190 says ImageTool is NOT in with_defaults(). If ImageTool is excluded, update tool count to 26 or clarify which tools are "built-in" vs "available".
