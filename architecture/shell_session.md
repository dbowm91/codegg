# Shell Session Module

The `shell_session` module provides shell session metadata management for terminal sessions.

## Overview

**Location**: `src/shell_session/`

**Note**: This module does NOT create actual PTY sessions. It only manages in-memory session metadata. Actual shell execution is handled by `tool::terminal` which spawns a shell process directly.

## Key Types

### ShellSession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSession {
    pub id: String,
    pub project_id: String,
    pub cwd: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at: i64,
}
```

### CreateShellSession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShellSession {
    pub project_id: String,
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
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

### ShellManager

```rust
pub struct ShellManager {
    sessions: Arc<RwLock<HashMap<String, ShellSession>>>,
}

impl ShellManager {
    pub fn new() -> Self;
    pub fn default() -> Self;
    pub async fn create(&self, input: CreateShellSession) -> Result<ShellSession, StorageError>;
    pub async fn get(&self, id: &str) -> Option<ShellSession>;
    pub async fn update_cwd(&self, id: &str, cwd: &str) -> Result<ShellSession, StorageError>;
    pub async fn list(&self, project_id: &str) -> Vec<ShellSession>;
    pub async fn resize(&self, id: &str, resize: ShellResize) -> Result<(), StorageError>;
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>;
}
```

## Notes

- Sessions are stored in-memory only (no persistence)
- `created_at` uses milliseconds since epoch (i64)
- `cwd` is stored as `String`, not `PathBuf`
- Default terminal size is 80 columns x 24 rows
- Default shell is `bash`
- Unit tests present in `src/shell_session/session.rs` (11 tests covering all ShellManager operations)

## See Also

- [tool.md](tool.md) - Terminal tool that executes shell commands