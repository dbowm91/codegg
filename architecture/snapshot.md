# Snapshot Module

The `snapshot` module provides file state capture and restore functionality.

## Overview

**Location**: `src/snapshot/`

**Key Responsibilities**:
- Capture file state before modifications
- Store snapshots in SQLite
- Restore files to previous state
- Snapshot comparison

## Key Types

### Snapshot

```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub description: Option<String>,
}

pub struct SnapshotFile {
    pub snapshot_id: String,
    pub path: String,
    pub content: String,
    pub hash: String,
}
```

### SnapshotView

Complete snapshot with all files:

```rust
pub struct SnapshotView {
    pub snapshot: Snapshot,
    pub files: Vec<SnapshotFile>,
}
```

### SnapshotManager

```rust
pub struct SnapshotManager {
    store: SqlitePool,
}

impl SnapshotManager {
    pub async fn create(&self, session_id: &str, paths: &[&Path]) -> Result<SnapshotView>;
    pub async fn get(&self, id: &str) -> Result<Option<SnapshotView>>;
    pub async fn restore(&self, id: &str) -> Result<()>;
    pub async fn list(&self, session_id: &str) -> Result<Vec<Snapshot>>;
    pub async fn delete(&self, id: &str) -> Result<()>;
}
```

## Usage Flow

```
Tool execution (edit, write, delete)
    │
    ▼
SnapshotManager::create(session_id, paths)
    │
    ▼
Store files in snapshot (content + hash)
    │
    ▼
Execute tool modification
    │
    ▼
If error → SnapshotManager::restore(id)
```

## Integration with AgentLoop

```rust
impl AgentLoop {
    async fn execute_tool(&self, tool: &dyn Tool, params: Value) -> ToolResult {
        // Capture state before modification
        if tool.modifies_files() {
            let snapshot = self.snapshot_manager
                .create(&self.session_id, tool.affected_paths(&params))
                .await?;

            // Store snapshot_id in context for potential rollback
        }

        // Execute tool
        let result = tool.execute(params, context).await;

        // If failed, restore snapshot
        if result.is_err() {
            self.snapshot_manager.restore(&snapshot.id).await?;
        }

        result
    }
}
```

## See Also

- [agent.md](agent.md) - Integration with agent loop
- [tool.md](tool.md) - File-modifying tools
