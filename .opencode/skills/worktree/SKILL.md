---
name: worktree
description: Git worktree management for listing, creating, and removing worktrees in opencode-rs
version: 1.1.0
tags:
  - git
  - worktree
  - repository
---

# Worktree Module Guide

This skill covers the worktree module in opencode-rs for Git worktree management.

## Overview

The `worktree` module provides Git worktree operations. It wraps `git worktree` CLI commands.

**Location**: `src/worktree/`

## Key Types

### Worktree

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,       // Worktree directory path
    pub branch: String,    // Branch name (empty if detached)
    pub is_current: bool,  // True if this is the current worktree
    pub is_detached: bool, // True if HEAD is detached
}
```

Note: `is_locked` and `is_main` shown in some older docs are **not implemented**.

## Key Functions

### list_worktrees()

```rust
pub fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError>
```

Lists all worktrees by parsing `git worktree list --porcelain` output. Identifies the current worktree by comparing canonical paths.

### create_worktree()

```rust
pub fn create_worktree(
    git_root: &Path,
    path: &Path,
    branch: &str,
    create_branch: bool,
) -> Result<(), AppError>
```

Creates a new worktree. If `create_branch` is true, creates a new branch with `-b` flag.

### remove_worktree()

```rust
pub fn remove_worktree(git_root: &Path, path: &Path) -> Result<(), AppError>
```

Removes a worktree via `git worktree remove`. Note: Does not support `force` parameter.

### find_git_root()

```rust
pub fn find_git_root(start: &Path) -> Option<PathBuf>
```

Walks up the directory tree looking for `.git` directory OR `.git` file (which indicates a worktree). Returns the path containing the git entry, or `None` if not found.

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

## Error Handling

All functions return `AppError::Worktree(String)` on failure:

```rust
AppError::Worktree("failed to run git worktree list: ...".to_string())
```

## Usage Example

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

    // Remove a worktree
    remove_worktree(&git_root, &Path::new("/path/to/new"))?;

    // Check if a directory is a worktree
    if is_git_worktree(&Path::new("/some/dir")) {
        println!("This is a worktree!");
    }
}
```

## TUI Integration

The worktree module is accessed via the `/worktree` TUI command, implemented in `src/tui/app/mod.rs:handle_worktree_command()`.

## Server Integration

The worktree module provides `is_git_worktree()` and `find_git_root()` which are used by server routes:
- `src/server/routes/workspace.rs` - Uses `is_git_worktree()` for workspace detection
- `src/server/routes/project.rs` - Uses `find_git_root()` for project git root discovery

## Notes

- Worktree parsing strips `refs/heads/` prefix from branch names
- Detached HEAD state is formatted as `detached@<sha>` or `detached@<path>`
- `find_git_root()` correctly handles both `.git` directories (main repo) and `.git` files (worktrees)
- The `is_current` detection uses canonical path comparison to handle relative paths correctly
- `is_git_worktree()` only returns `true` for worktrees (`.git` files), not regular repos (`.git` directories)

## Relationship to Other Modules

- **session/** - Stores `worktree` field in project table (git root path)
- **server/routes/workspace.rs** - Uses `is_git_worktree()` for workspace detection
- **server/routes/project.rs** - Uses `find_git_root()` for project git root discovery
- **tool** - Git operations tool uses worktree functions indirectly