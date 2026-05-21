# Snapshot Module Override

This file contains snapshot-specific guidance and overrides root AGENTS.md.

## FileChanged Event (Updated 2026-05-02)

The `AppEvent::FileChanged` event now includes `old_content: Option<String>` for snapshot checkpointing:

```rust
pub enum AppEvent {
    // ...
    FileChanged {
        path: String,
        action: String,
        old_content: Option<String>,  // New field for snapshots
    },
}
```

### Publishing old_content

Tools that modify files should publish the old content before modification:

- `write.rs` - Publishes `old_content: Some(old_content)` or `None` if file didn't exist
- `edit.rs` - Publishes `old_content: Some(old_content)` after reading file
- `replace.rs` - Publishes `old_content: Some(content)` after reading file (fixed 2026-05-02)

### Snapshot Integration

The `old_content` enables snapshot creation for file modifications:
1. When a file is modified, the `FileChanged` event includes the old content
2. Snapshots can use this to capture file state before modification
3. Enables undo/redo capabilities for file modifications

## Restore API (Added 2026-05-21)

SnapshotManager now supports restore operations:

```rust
// Restore files to project root
pub async fn restore(&self, snapshot: &SnapshotView) -> Result<(), String>

// Restore files to a custom path (for migration/testing)
pub async fn restore_to_path(
    &self,
    snapshot: &SnapshotView,
    target_path: &Path,
) -> Result<(), String>

// Delete specific snapshot
pub async fn delete_snapshot(&self, id: &str) -> Result<(), String>

// Delete all snapshots for a session
pub async fn delete_all_for_session(&self, session_id: &str) -> Result<(), String>
```

### Usage Example

```rust
// Get latest snapshot for session
if let Some(snapshot) = snapshot_manager.latest(session_id).await? {
    // Restore files to project root
    snapshot_manager.restore(&snapshot).await?;
}
```

## Note on SnapshotManager Mutation

`capture()` takes `&mut self` while `capture_incremental()` takes `&self`. This inconsistency exists but `capture()` doesn't actually mutate self (the pool is cloneable), so this is a design quirk rather than a functional issue.
