# PTY Module

The `pty` module provides shell session metadata management.

## Overview

**Location**: `src/pty/`

**Note**: This module does NOT create actual PTY sessions. It only manages in-memory session metadata.

## Key Types

### PtySession

```rust
pub struct PtySession {
    pub id: String,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}
```

### SessionManager

```rust
pub struct SessionManager {
    sessions: RwLock<HashMap<String, PtySession>>,
}

impl SessionManager {
    pub fn create(&self, cwd: &Path) -> Result<String>;
    pub fn get(&self, id: &str) -> Option<PtySession>;
    pub fn update_cwd(&self, id: &str, cwd: &Path) -> Result<()>;
    pub fn destroy(&self, id: &str) -> Result<()>;
}
```

## Note

Actual shell execution is handled by `tool::bash` which spawns a shell process directly.

## See Also

- [tool.md](tool.md) - Bash tool that executes shell commands
