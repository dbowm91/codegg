# Git Module

The `git` module provides Git session management for tracking repository state and worktree operations.

## Overview

**Location**: `src/git/`

**Key Responsibilities**:
- Track Git session state (branch, dirty status, commit hash, stash count)
- Manage per-session worktrees for isolated file operations
- Provide git status information for prompt injection
- Integrate with the `worktree` module for worktree creation/removal

## Key Types

### GitSession

```rust
pub struct GitSession {
    pub session_id: String,
    pub worktree_path: Option<PathBuf>,
    pub git_root: PathBuf,
    pub status: GitStatus,
    pub auto_worktree: bool,
}
```

Represents a Git session tied to a CodeGG session. Optionally creates a worktree under `.git/worktrees/{session_id}` for isolated file operations.

### GitStatus

```rust
pub struct GitStatus {
    pub branch: String,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
    pub stash_count: usize,
}
```

Current repository state snapshot.

## Components

### GitSession Creation

`GitSession::new()` initializes a session by:
1. Running `git status` to get branch, dirty status, commit hash, and stash count
2. Optionally setting a worktree path if `auto_worktree` is enabled
3. All git commands use `env_clear()` with only `PATH` set for security

### Status Refresh

`refresh_status()` re-runs `git status` to update the session's state snapshot.

### Worktree Operations

- `create_worktree(branch)` - Delegates to `worktree::create_worktree()`
- `remove_worktree()` - Delegates to `worktree::remove_worktree()`
- Both require `auto_worktree` to be enabled (worktree_path is Some)

### Prompt Injection

`format_for_prompt()` generates a formatted string for inclusion in system prompts:
```
[Git Info]
Branch: main
Status: dirty (uncommitted changes)
Commit: abc1234
Stash: 2 entries
Worktree: /path/to/worktree/
```

### Git Root Discovery

`get_git_root(start)` delegates to `worktree::find_git_root()` to walk up the directory tree.

## Integration with Other Modules

### Worktree Module

`GitSession` delegates worktree creation and removal to `worktree::create_worktree()` and `worktree::remove_worktree()`.

### Agent Loop

Git status is injected into system prompts via `format_for_prompt()` during prompt assembly.

### Session Module

Git session state is associated with CodeGG sessions for tracking repository context.

## Implementation Notes

- All git commands use `env_clear()` with only `PATH` inherited for security
- Branch detection returns "detached" for detached HEAD states
- Stash count is determined by counting lines starting with "stash@"
- Worktree path is `.git/worktrees/{session_id}` format

## See Also

- [worktree.md](worktree.md) - Git worktree creation and management
- [agent.md](agent.md) - Git info injection into prompts
