# Snapshot Module Architecture Review Findings

## Verified Claims

### SnapshotOptions (snapshot/mod.rs:9-13)
```rust
pub struct SnapshotOptions {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}
```
Default values at mod.rs:16-22:
- max_files: 5_000 ✓
- max_file_bytes: 1_000_000 (1MB) ✓
- max_total_bytes: 20_000_000 (20MB) ✓

### FileSnapshot Struct (mod.rs:25-31)
```rust
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,
    pub timestamp: i64,
}
```
All fields match exactly.

### Snapshot Struct (mod.rs:33-40)
```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub data: String, // JSON serialized HashMap<String, FileSnapshot>
}
```
All fields match. Note correctly documents that `data` is JSON-serialized.

### SnapshotView Struct (mod.rs:42-49)
```rust
pub struct SnapshotView {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}
```
All fields match exactly.

### SnapshotManager Struct (mod.rs:51-55)
```rust
pub struct SnapshotManager {
    pool: SqlitePool,
    project_root: PathBuf,
    options: SnapshotOptions,
}
```
All fields match exactly.

### SnapshotManager Methods (mod.rs:57-360)
All documented methods exist:
- `new()` - Line 58
- `new_with_options()` - Line 66
- `capture()` - Line 83
- `capture_incremental()` - Line 119
- `get()` - Line 181
- `list_for_session()` - Line 205
- `latest()` - Line 228
- `restore()` - Line 267
- `restore_to_path()` - Line 302
- `delete_snapshot()` - Line 343
- `delete_all_for_session()` - Line 352

### Database Schema (snapshot table - schema.rs:481-503)
```sql
CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
```
Matches documentation. Index also matches (snapshot_session_idx).

### Excluded Directories (mod.rs:384)
```rust
if name == ".git" || name == "node_modules" || name == "target" || name == ".codegg" {
    continue;
}
```
Matches documentation.

### Path Traversal Prevention (mod.rs:274-284 for restore, mod.rs:313-323 for restore_to_path)
```rust
if !canonical_path.starts_with(&canonical_project_root) {
    return Err(format!("path traversal attempt detected: {}", full_path.display()));
}
```
Verified in both methods.

### Atomic Write Pattern (mod.rs:330-334)
```rust
let temp_path = full_path.with_extension("tmp");
std::fs::write(&temp_path, &file_snapshot.content)
    .map_err(...)?;
std::fs::rename(&temp_path, &full_path)
    .map_err(...)?;
```
Verified correct.

### Diff Module (diff.rs:1-144)
All documented types and functions exist:
- `FileDiff` struct (diff.rs:4-7)
- `DiffHunk` struct (diff.rs:10-14)
- `DiffLine` struct (diff.rs:17-20)
- `DiffKind` enum (diff.rs:23-27): Context, Added, Removed
- `diff_files()` function (diff.rs:29-128)
- `format_unified_diff()` function (diff.rs:130-144)

## Stale Information

### AgentLoop Integration Example (snapshot.md:157-180)
The documentation shows code example of `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()` methods on AgentLoop. However, these method signatures don't appear in the snapshot module itself - they would be in the agent module if implemented. This appears to be a placeholder/example rather than actual code, and correctly notes that integration is the responsibility of AgentLoop.

## Bugs Found

### Hash Algorithm Inconsistency
- **checkpoint.rs:compute_checksum** uses SHA256 for working file verification
- **snapshot/mod.rs:142** uses MD5 for file snapshot hashing

Both are cryptographic hashes used for integrity verification, but using different algorithms could cause confusion. This is not a functional bug but an inconsistency worth noting.

## Cross-Module Issues

1. **Snapshot table defined in session module**: The snapshot table is created by `session::schema::migrate_v13()` (schema.rs:481-503), not in the snapshot module itself. This is correctly documented but means the snapshot module depends on the session module's schema.

2. **Restore not integrated with agent error handling**: The documentation correctly notes that automatic rollback on tool failure is NOT implemented. Snapshots are captured but restore must be triggered manually.

## Documentation Notes

The snapshot.md documentation is well-structured and accurately describes:
1. The capture flow (before tool execution)
2. Incremental capture (via file change events)
3. Manual restore availability
4. Security measures (path traversal, atomic writes)
5. Configuration options

The key caveat - that restore is available but not automatically triggered - is clearly documented at lines 118 and 147-153.
