---
name: shell_session
description: Shell session metadata management for terminal sessions in codegg
version: 1.2.0
tags:
  - shell
  - session
  - terminal
---

# Shell Session Module Guide

This skill covers the shell_session module in codegg for shell session metadata management.

## Overview

The `shell_session` module manages in-memory metadata for shell/terminal sessions. It does **NOT** create actual PTY sessions - that is handled by the `tool::terminal` module.

**Location**: `src/shell_session/`

## Key Types

### ShellSession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSession {
    pub id: String,           // UUID
    pub project_id: String,   // Project identifier
    pub cwd: String,          // Current working directory
    pub shell: String,         // Shell name (e.g., "bash")
    pub cols: u16,            // Terminal columns
    pub rows: u16,            // Terminal rows
    pub created_at: i64,      // Milliseconds since epoch
}
```

### CreateShellSession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShellSession {
    pub project_id: String,
    pub cwd: Option<String>,     // Defaults to "."
    pub shell: Option<String>,   // Defaults to "bash"
    pub cols: Option<u16>,      // Defaults to 80
    pub rows: Option<u16>,      // Defaults to 24
}
```

### ShellResize

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResize {
    pub cols: u16,
    pub rows: u16,
}
```

## ShellManager

```rust
pub struct ShellManager {
    sessions: Arc<RwLock<HashMap<String, ShellSession>>>,
}
```

### Methods

| Method | Description |
|--------|-------------|
| `new()` | Create a new ShellManager |
| `create(input)` | Create a new session, returns `ShellSession` with generated ID |
| `get(id)` | Get session by ID, returns `Option<ShellSession>` |
| `update_cwd(id, cwd)` | Update session's working directory, returns `Result<ShellSession, StorageError>` |
| `list(project_id)` | List all sessions for a project |
| `resize(id, resize)` | Update terminal dimensions |
| `delete(id)` | Delete a session |

## Error Handling

Methods return `StorageError` on failure:

```rust
StorageError::NotFound(format!("shell session {id}"))
```

## Usage Example

```rust
use crate::shell_session::{ShellManager, CreateShellSession};

let manager = ShellManager::new();

// Create a session
let input = CreateShellSession {
    project_id: "my-project".to_string(),
    cwd: Some("/home/user".to_string()),
    shell: Some("zsh".to_string()),
    cols: Some(120),
    rows: Some(40),
};
let session = manager.create(input).await?;

// Get session
if let Some(s) = manager.get(&session.id).await {
    println!("Session {} at {}", s.id, s.cwd);
}

// Update cwd
let updated = manager.update_cwd(&session.id, "/tmp").await?;

// List project sessions
let sessions = manager.list("my-project").await;

// Resize terminal
manager.resize(&session.id, ShellResize { cols: 100, rows: 30 }).await?;

// Delete session
manager.delete(&session.id).await?;
```

## Notes

- Sessions are **in-memory only** - they do not persist across restarts
- The module does NOT spawn actual shell processes
- Actual shell execution is handled by `tool::terminal`
- Use `project_id` to group sessions by project
- Terminal dimensions (cols/rows) are stored for future use when actual PTY support is added
- Unit tests provided: 11 tests covering create, get, update, list, resize, delete operations

## Relationship to Other Modules

- **tool::terminal** - Actually executes shell commands (not the same as this shell_session module)
- **session/** - Manages agent conversation sessions (different from shell sessions)