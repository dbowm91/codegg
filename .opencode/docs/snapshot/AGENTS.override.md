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
