# Worktree Module

The `worktree` module provides Git worktree management.

## Overview

**Location**: `src/worktree/`

**Key Responsibilities**:
- List git worktrees
- Create new worktrees
- Remove worktrees
- Find git root

## Key Functions

### list_worktrees()

```rust
pub fn list_worktrees(git_dir: &Path) -> Result<Vec<Worktree>> {
    // Parse `git worktree list` output
}

pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_locked: bool,
}
```

### create_worktree()

```rust
pub fn create_worktree(git_dir: &Path, branch: &str, path: &Path) -> Result<()>;
```

### remove_worktree()

```rust
pub fn remove_worktree(git_dir: &Path, path: &Path, force: bool) -> Result<()>;
```

### find_git_root()

```rust
pub fn find_git_root(start: &Path) -> Result<PathBuf>;
```

Walks up directory tree looking for `.git` directory.

## See Also

- [tool.md](tool.md) - Git operations tool
