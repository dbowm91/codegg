# Worktree Module

The `worktree` module provides Git worktree management.

## Overview

**Location**: `src/worktree/`

**Key Responsibilities**:
- List git worktrees
- Create new worktrees
- Remove worktrees
- Find git root (walks up directory tree looking for `.git`)

## Key Functions

### list_worktrees()

```rust
pub fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError>
```

Parses `git worktree list --porcelain` output to return worktree list.

### create_worktree()

```rust
pub fn create_worktree(
    git_root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
) -> Result<(), AppError>
```

Creates a new worktree. If `create_branch` is true, passes `-b` to create a new branch.

### remove_worktree()

```rust
pub fn remove_worktree(git_root: &Path, path: &Path) -> Result<(), AppError>
```

Removes a worktree via `git worktree remove`.

### find_git_root()

```rust
pub fn find_git_root(start: &Path) -> Option<PathBuf>
```

Walks up the directory tree looking for `.git` directory or `.git` file (which indicates a worktree). Returns the path containing the `.git` entry.

## Data Structures

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,          // Worktree directory path
    pub branch: String,        // Branch name (empty if detached), "refs/heads/" stripped
    pub is_current: bool,      // True if this is the current worktree
    pub is_detached: bool,     // True if HEAD is detached
}
```

Note: `is_locked` and `is_main` shown in some older docs are **not implemented**.

## See Also

- [tool.md](tool.md) - Git operations tool
- `src/server/routes/workspace.rs` - Server workspace routes with `is_git_worktree()` helper