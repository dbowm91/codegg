# Worktree Module

The `worktree` module provides Git worktree management.

## Overview

**Location**: `src/worktree/`

**Key Responsibilities**:
- List git worktrees
- Create new worktrees
- Remove worktrees
- Find git root (walks up directory tree looking for `.git`)
- Check if a directory is a git worktree

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
pub fn remove_worktree(git_root: &Path, path: &Path, force: bool) -> Result<(), AppError>
```

Removes a worktree via `git worktree remove`. Pass `force=true` to use the `--force` flag, which removes a worktree even if it has untracked or modified files.

### find_git_root()

```rust
pub fn find_git_root(start: &Path) -> Option<PathBuf>
```

Walks up the directory tree looking for `.git` directory or `.git` file (which indicates a worktree). Returns the path containing the `.git` entry.

### is_git_worktree()

```rust
pub fn is_git_worktree(dir: &Path) -> bool
```

Checks if a directory is a Git worktree by detecting a `.git` file with `gitdir:` prefix. Returns `true` if the directory is a worktree, `false` otherwise (including regular git repos with `.git` directories).

### is_git_file()

```rust
pub fn is_git_file(git_path: &Path) -> bool
```

Checks if a `.git` path is a file (indicating a worktree) rather than a directory. Returns `true` if the file exists and starts with `gitdir:`.

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

## Usage

```rust
use crate::worktree::{list_worktrees, create_worktree, remove_worktree, find_git_root, is_git_worktree, Worktree};

// Find the git root for a directory
if let Some(git_root) = find_git_root(&some_path) {
    // List all worktrees
    let worktrees = list_worktrees(&git_root)?;
    for wt in worktrees {
        println!("{} - {} (current: {})", wt.path, wt.branch, wt.is_current);
    }

    // Create a new worktree
    create_worktree(&git_root, &Path::new("/path/to/new"), "feature-branch", true)?;

    // Remove a worktree (force=false)
    remove_worktree(&git_root, &Path::new("/path/to/new"), false)?;

    // Or force removal even with untracked/modified files
    remove_worktree(&git_root, &Path::new("/path/to/new"), true)?;

    // Check if a directory is a worktree
    if is_git_worktree(&Path::new("/some/dir")) {
        println!("This is a worktree!");
    }
}
```

## See Also

- [tool.md](tool.md) - Git operations tool
- `src/worktree/mod.rs` - Contains `is_git_file()` at line 172, `is_git_worktree()` at line 180
- `src/server/routes/workspace.rs` - Uses `is_git_worktree()` for workspace detection
- `src/server/routes/project.rs` - Uses `find_git_root()` for project git root discovery