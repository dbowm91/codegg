# Snapshot Module

The `snapshot` module provides file state capture and restore functionality.

## Overview

**Location**: `src/snapshot/`

**Key Responsibilities**:
- Capture file state before modifications (full or incremental)
- Store snapshots in SQLite with JSON-serialized file data
- Restore files to previous state
- Snapshot comparison via diff module

## Key Types

### SnapshotOptions

Limits for capture operations:

```rust
pub struct SnapshotOptions {
    pub max_files: usize,        // default: 5_000
    pub max_file_bytes: u64,      // default: 1_000_000 (1MB)
    pub max_total_bytes: u64,     // default: 20_000_000 (20MB)
}
```

### FileSnapshot

Individual file state within a snapshot:

```rust
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,
    pub timestamp: i64,
}
```

### Snapshot

Database representation (raw data field):

```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,          // milliseconds since epoch
    pub label: Option<String>,
    pub data: String,            // JSON serialized HashMap<String, FileSnapshot>
}
```

### SnapshotView

Deserialized snapshot with file map (returned by API):

```rust
pub struct SnapshotView {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}
```

### SnapshotManager

```rust
pub struct SnapshotManager {
    pool: SqlitePool,
    project_root: PathBuf,
    options: SnapshotOptions,
}

impl SnapshotManager {
    pub fn new(pool: SqlitePool, project_root: PathBuf) -> Self;
    pub fn new_with_options(pool: SqlitePool, project_root: PathBuf, options: SnapshotOptions) -> Self;
    
    pub async fn capture(&mut self, session_id: &str, label: Option<String>) -> Result<SnapshotView, String>;
    pub async fn capture_incremental(&self, session_id: &str, label: Option<String>, file_changes: Vec<(String, Option<String>)>) -> Result<Option<SnapshotView>, String>;
    pub async fn get(&self, id: &str) -> Result<Option<SnapshotView>, String>;
    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<SnapshotView>, String>;
    pub async fn latest(&self, session_id: &str) -> Result<Option<SnapshotView>, String>;
    pub async fn restore(&self, snapshot: &SnapshotView) -> Result<(), String>;
    pub async fn restore_to_path(&self, snapshot: &SnapshotView, target_path: &Path) -> Result<(), String>;
    pub async fn delete_snapshot(&self, id: &str) -> Result<(), String>;
    pub async fn delete_all_for_session(&self, session_id: &str) -> Result<(), String>;
}
```

## Usage Flow

### Two-Phase Capture

The snapshot system uses a two-phase capture approach integrated with the AgentLoop:

#### Phase 1: Pre-Execution Capture (loop.rs:1655)
```
Before tool execution
    │
    ▼
AgentLoop::capture_snapshot_if_needed()
    │
    ▼
SnapshotManager::capture(session_id, None)
    │
    ▼
Store full project state as snapshot
    │
    ▼
Execute tool modification
```

**Note**: Snapshot capture is wired but `restore()` is not currently called on error. Snapshots are captured for safety but the rollback feature is not yet integrated.

#### Phase 2: Post-Execution Incremental Capture (loop.rs:1853)
```
After tool execution (success)
    │
    ▼
AgentLoop::capture_incremental_snapshot_if_needed()
    │
    ▼
SnapshotManager::capture_incremental(session_id, label, changes)
    │
    ▼
For each file change:
  - Validate path is within project_root
  - Store incremental changes
```

### Full Capture

```
Tool execution (edit, write, delete)
    │
    ▼
SnapshotManager::capture(session_id, label)
    │
    ▼
Collect all files from project_root (respecting limits)
    │
    ▼
Store as JSON in snapshot.data column
    │
    ▼
Execute tool modification
```

**Note**: Same as Phase 1 - `restore()` is not called on error. Snapshot rollback is not yet integrated.

### Incremental Capture

```
FileChanged event with old_content
    │
    ▼
AgentLoop drains file change events
    │
    ▼
SnapshotManager::capture_incremental(session_id, label, changes)
    │
    ▼
For each (path, old_content):
  - Validate path is within project_root
  - Store in snapshot if valid
    │
    ▼
If no files, return None
```

## Integration with AgentLoop

```rust
impl AgentLoop {
    async fn capture_snapshot_if_needed(&mut self) {
        if let Some(ref mut snapshot_manager) = self.snapshot_manager {
            let snapshot = snapshot_manager
                .capture(&self.session_id, None)
                .await?;
            tracing::info!("Snapshot captured: {}", snapshot.id);
        }
    }

    async fn capture_incremental_snapshot_if_needed(&mut self, label: Option<String>) {
        let changes = self.drain_file_change_events();
        if changes.is_empty() {
            return;
        }
        if let Some(ref snapshot_manager) = self.snapshot_manager {
            snapshot_manager
                .capture_incremental(&self.session_id, label, changes)
                .await?;
        }
    }
}
```

## Database Schema

> **Note**: The `snapshot` table is defined in `src/session/schema.rs` (migration v13), not in the snapshot module itself.

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

## Security

### Path Traversal Prevention

`restore_to_path()` validates that restored paths don't escape the target directory:

```rust
let canonical_target = std::fs::canonicalize(&target)?;
let canonical_path = full_path.canonicalize()?;
if !canonical_path.starts_with(&canonical_target) {
    return Err("path traversal attempt detected");
}
```

This prevents attacks like `../../etc/passwd` from writing files outside the intended target directory.

## Diff Module (`src/snapshot/diff.rs`)

Provides diff computation for snapshot comparison:

```rust
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
}

pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

pub struct DiffLine {
    pub kind: DiffKind,
    pub content: String,
}

pub enum DiffKind {
    Context,
    Added,
    Removed,
}

pub fn diff_files(old: &str, new: &str, path: &str) -> Vec<FileDiff>;
pub fn format_unified_diff(old: &str, new: &str, old_path: &str, new_path: &str) -> String;
```

## Configuration

Snapshot can be enabled via config:

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

## See Also

- [agent.md](agent.md) - Integration with agent loop
- [tool.md](tool.md) - File-modifying tools
- `.opencode/skills/snapshot/SKILL.md` - Skill guide for agents