# Git Module

The `git` module previously provided Git session management for tracking repository state and worktree operations. As of the [native tool crate extraction](native_crates.md), this module has been **removed** from `src/`. Read-only git facts (`repo_status`, `diff_summary`, `changed_files`, `file_diff`, `validate_patch`) now live in the `egggit` workspace crate (`crates/egggit/`). Mutating worktree operations live in `src/worktree/`.

The Codegg `git` tool (`src/tool/git.rs`) remains a low-level command wrapper and continues to expose the model-facing `git` name unchanged. The `commit` and `review` tools consume `egggit` for diff facts and keep their mutation/permission flow in codegg.

## Overview

**Location**: `crates/egggit/` (read-only git facts) and `src/worktree/` (mutating worktree operations)

**Key Responsibilities**:
- Read-only git facts via the `egggit` crate (status, diff summary, changed files, file diff, patch validation, worktree list)
- Per-session worktree management (creation/removal) in `src/worktree/`
- Git status information for prompt injection (consumed from `egggit::repo_status`)
- Mutating operations (commit, worktree create/remove) stay in codegg under the permission flow

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
