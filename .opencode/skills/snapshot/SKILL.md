---
name: snapshot
description: Snapshot support for file state capture and restore
version: 1.0.0
tags:
  - snapshot
  - checkpoint
  - file-state
---

# Snapshot Module Guide

This skill covers the snapshot system in opencode-rs for capturing and restoring file states.

## Overview

The `snapshot/` module provides:
- **File Snapshots**: Capture individual file state (path, content, hash, timestamp)
- **Session Snapshots**: Group file snapshots by session
- **Checkpointing**: Infrastructure for file modification tracking

## Architecture#

### Snapshot Manager (`src/snapshot/mod.rs`)

```rust
pub struct SnapshotManager {
    snapshots: Vec<Snapshot>,
    project_root: PathBuf,
}

impl SnapshotManager {
    pub fn new(project_root: PathBuf) -> Self;
    pub async fn capture(&mut self, session_id: &str, label: Option<String>) -> Result<Snapshot, String>;
    pub async fn capture_incremental(&self, session_id: &str, label: Option<String>, file_changes: Vec<(String, Option<String>)>) -> Result<Option<Snapshot>, String>;
    pub async fn restore(&self, snapshot: &SnapshotView) -> Result<(), String>;  // NEW
    pub async fn restore_to_path(&self, snapshot: &SnapshotView, target_path: &Path) -> Result<(), String>;  // NEW
    pub async fn delete_snapshot(&self, id: &str) -> Result<(), String>;  // NEW
    pub async fn delete_all_for_session(&self, session_id: &str) -> Result<(), String>;  // NEW
    pub fn get(&self, id: &str) -> Option<&Snapshot>;
    pub fn list_for_session(&self, session_id: &str) -> Vec<&Snapshot>;
    pub fn latest(&self, session_id: &str) -> Option<&Snapshot>;
}
```

### Snapshot Struct

```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}

pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,
    pub timestamp: i64,
}
```

## FileChanged Event Extension (Updated 2026-05-02)#

The `AppEvent::FileChanged` now includes `old_content: Option<String>`:

```rust
pub enum AppEvent {
    FileChanged {
        path: String,
        action: String,
        old_content: Option<String>,  // Added 2026-05-02
    },
    // ... other events
}
```

This enables snapshot checkpointing by making the old file content available via the event bus.

## Integration with AgentLoop#

The `SnapshotManager` is now wired to `AgentLoop` (`src/agent/loop.rs`):

- **Field**: `snapshot_manager: Option<SnapshotManager>` in AgentLoop struct
- **Initialization**: Created in `AgentLoop::new()` based on `config.snapshot` setting
- **Capture trigger**: `capture_snapshot_if_needed()` called before file-modifying tools
- **Config**: Enable via `snapshot: true` in config

File-modifying tools that trigger snapshots:
- `write`
- `edit`
- `replace`
- `multiedit`
- `apply_patch`

## Integration with Write/Edit Tools#

The following tools now publish `old_content` in `FileChanged` events:

### WriteTool (`src/tool/write.rs`)

The `WriteTool::execute()`:
1. Reads existing file content (`old_content`)
2. Writes new content
3. Publishes `AppEvent::FileChanged` with `old_content: Some(old_content)`

### EditTool (`src/tool/edit.rs`)

The `EditTool::execute()`:
1. Reads existing file content (`content`)
2. Applies edit
3. Writes new content
4. Publishes `AppEvent::FileChanged` with `old_content: Some(content)`

### ReplaceTool (`src/tool/replace.rs`)

The `ReplaceTool::execute()`:
1. Reads existing file content (`content`)
2. Applies regex replacement
3. Writes new content
4. Publishes `AppEvent::FileChanged` with `old_content: Some(content)` (fixed 2026-05-02)

## Usage for Checkpointing#

1. Subscribe to `AppEvent::FileChanged` events
2. When `old_content` is `Some(content)`, capture a snapshot
3. Associate with session_id
4. Use for revert/restore operations

## Restore Functionality (Implemented 2026-05-21)#

SnapshotManager now supports restore operations:

```rust
pub async fn restore(&self, snapshot: &SnapshotView) -> Result<(), String> {
    // Restores all files from snapshot to project root
    for (rel_path, file_snapshot) in files {
        let full_path = project_root.join(&rel_path);
        std::fs::write(&full_path, &file_snapshot.content)?;
    }
    Ok(())
}

pub async fn restore_to_path(
    &self,
    snapshot: &SnapshotView,
    target_path: &Path,
) -> Result<(), String> {
    // Restores files to a custom target path (for migration/testing)
}

pub async fn delete_snapshot(&self, id: &str) -> Result<(), String> {
    // Delete a specific snapshot
}

pub async fn delete_all_for_session(&self, session_id: &str) -> Result<(), String> {
    // Delete all snapshots for a session (cleanup)
}
```

## Future Work#

- ~~Create actual snapshot objects from `FileChanged` events~~ (Done: SnapshotManager wired to AgentLoop)
- ~~Implement revert functionality using snapshots~~ (Done: restore() and restore_to_path() added 2026-05-21)
- ~~Add snapshot cleanup (limit number of snapshots per session)~~ (Done: delete_snapshot() and delete_all_for_session() added)
- Add snapshot UI (list, restore, delete)

Base directory for this skill: file:///Users/davidbowman/projects/codegg/.opencode/skills/snapshot
Relative paths in this skill (e.g., scripts/, reference/) are relative to this base directory.
Note: file list is sampled.
