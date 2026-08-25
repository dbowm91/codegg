# Snapshot Module

The `snapshot` module provides file state capture and restore
functionality for safety during tool execution.

## Purpose

Capture the state of project files before modifications so they can
be restored if needed. Supports both full-project captures and
incremental captures of specific changed files.

## Where It Lives

- **Core types & manager**: `crates/codegg-core/src/snapshot/mod.rs`
- **Diff computation**: `crates/codegg-core/src/snapshot/diff.rs`
- **DB schema**: `crates/codegg-core/src/session/schema.rs`
  (migration v13)
- **Python script snapshots**: `src/python_script/snapshot.rs`
  (separate, metadata-only)

The `python_script` module has its own lightweight
`WorkspaceSnapshot` that tracks file metadata (size, mtime) rather
than content. Do not confuse the two.

## How It Works

### Full Capture

`SnapshotManager::capture()` walks the project root using a
depth-first stack, collecting file contents up to configured limits.
Results are JSON-serialized into the `snapshot.data` column.

### Incremental Capture

`SnapshotManager::capture_incremental()` accepts pre-collected
`(path, old_content)` pairs. It validates paths are safe (no `..`,
no absolute paths) and stores only valid entries. Returns `None` if
no files are valid.

### Restore

`restore()` and `restore_to_path()` are available but **not
automatically called on tool failure**. They must be triggered
explicitly (e.g., via a `/restore` command or direct API call).

Restore uses an atomic write pattern (temp file + rename) and
validates path containment via `ensure_contained_parent()` which
rejects symlinks and canonicalizes paths.

## Key Types & APIs

### SnapshotOptions (`crates/codegg-core/src/snapshot/mod.rs:9`)

```rust
pub struct SnapshotOptions {
    pub max_files: usize,        // default: 5_000
    pub max_file_bytes: u64,     // default: 1_000_000 (1MB)
    pub max_total_bytes: u64,    // default: 20_000_000 (20MB)
}
```

Zero values are clamped to 1 with a warning.

### FileSnapshot (`:26`)

```rust
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,       // SHA-256 hex
    pub timestamp: i64,
}
```

### Snapshot (`:34`)

```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub data: String,       // JSON HashMap<String, FileSnapshot>
}
```

### SnapshotView (`:43`)

```rust
pub struct SnapshotView {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}
```

### SnapshotManager (`:52`)

| Method | Line | Description |
|--------|------|-------------|
| `new(pool, root)` | :59 | Default options |
| `new_with_options(pool, root, opts)` | :67 | Custom options |
| `capture(session_id, label)` | :92 | Full project capture |
| `capture_incremental(sid, label, changes)` | :129 | Incremental capture |
| `get(id)` | :194 | Fetch by ID |
| `list_for_session(sid)` | :218 | List all for session |
| `latest(sid)` | :241 | Latest for session |
| `restore(snapshot)` | :280 | Restore to project root |
| `restore_to_path(snapshot, target)` | :337 | Restore to custom path |
| `delete_snapshot(id)` | :392 | Delete by ID |
| `delete_all_for_session(sid)` | :401 | Delete all for session |

### Diff Module (`crates/codegg-core/src/snapshot/diff.rs`)

```rust
pub struct FileDiff { pub path: String, pub hunks: Vec<DiffHunk> }
pub struct DiffHunk { pub old_start, new_start, lines }
pub struct DiffLine { pub kind: DiffKind, pub content: String }
pub enum DiffKind { Context, Added, Removed }
```

Functions:
- `diff_files(old, new, path)` (:29) — structured diff
- `format_unified_diff(old, new, old_path, new_path)` (:130) — unified format

Uses the `similar` crate for text diffing.

## Database Schema

```sql
CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS snapshot_session_idx ON snapshot(session_id);
```

Defined in migration v13 (`session/schema.rs:669`).

## Security

### Path Traversal Prevention

- `is_safe_relative_path()` (:416) rejects `..`, root dir, Windows
  prefixes, and empty paths.
- `ensure_contained_parent()` (:435) validates parent directories
  after `mkdir` but before writing, rejecting symlinks and checking
  canonical containment. This shrinks the TOCTOU window.
- `capture_incremental()` rejects absolute paths and unsafe relative
  paths before storing.

### Atomic Write Pattern

`restore()` and `restore_to_path()` write to a `.tmp` file first,
then atomically rename. This prevents partial writes on interruption.

## Configuration Surface

```json
{
  "snapshot": true,
  "snapshot_config": {
    "max_files": 5000,
    "max_file_bytes": 1000000,
    "max_total_bytes": 20000000
  }
}
```

## Invariants & Gotchas

- **No automatic rollback**: Snapshots are captured for safety but
  `restore()` is not called automatically on tool failure. This is a
  planned enhancement.
- Excluded directories: `.git`, `node_modules`, `target`, `.codegg`.
- Binary files (non-UTF-8) are silently skipped during full capture.
- Empty files are captured with a hash of the empty string.
- `capture()` uses `spawn_blocking` for the filesystem walk.
- `restore()` also uses `spawn_blocking` with a oneshot channel to
  bridge sync/async.
- The `snapshot` table FK cascades on session delete.

## Testing

```bash
cargo test -p codegg-core -- snapshot
```

## Related Docs

- [agent.md](agent.md) — integration with agent loop
- [tool.md](tool.md) — file-modifying tools
