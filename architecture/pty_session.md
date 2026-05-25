# PTY Module

The `pty` module provides shell session metadata management for terminal sessions.

## Overview

**Location**: `src/pty_session/`

**Note**: This module does NOT create actual PTY sessions. It only manages in-memory session metadata. Actual shell execution is handled by `tool::terminal` which spawns a shell process directly.

## Key Types

### PtySession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtySession {
    pub id: String,
    pub project_id: String,
    pub cwd: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at: i64,
}
```

### CreatePtySession

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePtySession {
    pub project_id: String,
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}
```

### PtyResize

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyResize {
    pub cols: u16,
    pub rows: u16,
}
```

### PtyManager

```rust
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self;
    pub fn default() -> Self;
    pub async fn create(&self, input: CreatePtySession) -> Result<PtySession, StorageError>;
    pub async fn get(&self, id: &str) -> Option<PtySession>;
    pub async fn update_cwd(&self, id: &str, cwd: &str) -> Result<PtySession, StorageError>;
    pub async fn list(&self, project_id: &str) -> Vec<PtySession>;
    pub async fn resize(&self, id: &str, resize: PtyResize) -> Result<(), StorageError>;
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>;
}
```

## Notes

- Sessions are stored in-memory only (no persistence)
- `created_at` uses milliseconds since epoch (i64)
- `cwd` is stored as `String`, not `PathBuf`
- Default terminal size is 80 columns x 24 rows
- Default shell is `bash`
- Unit tests added (11 tests covering all PtyManager operations)

## See Also

- [tool.md](tool.md) - Terminal tool that executes shell commands