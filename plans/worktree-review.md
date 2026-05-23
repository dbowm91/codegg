# Worktree Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError>` parses `git worktree list --porcelain` output | VERIFIED | `src/worktree/mod.rs:49-108` uses `git worktree list --porcelain` and parses line-by-line with `worktree`, `branch`, `HEAD`, and `detached` prefixes |
| `create_worktree(git_root, path, branch, create_branch) -> Result<(), AppError>` with `create_branch` flag passes `-b` to create new branch | VERIFIED | `src/worktree/mod.rs:110-138` adds `-b` flag conditionally based on `create_branch` parameter |
| `remove_worktree(git_root, path) -> Result<(), AppError>` removes via `git worktree remove` | VERIFIED | `src/worktree/mod.rs:140-155` calls `git worktree remove` with path argument |
| `find_git_root(start: &Path) -> Option<PathBuf>` walks up directory tree looking for `.git` | VERIFIED | `src/worktree/mod.rs:157-169` traverses parent directories checking for `.git` directory or file |
| `is_git_worktree(dir: &Path) -> bool` checks for `.git` file with `gitdir:` prefix | VERIFIED | `src/worktree/mod.rs:179-182` returns true only when `.git` exists AND is a file (not directory) starting with `gitdir:` |
| `is_git_file(git_path: &Path) -> bool` checks if `.git` path is a file with `gitdir:` prefix | VERIFIED | `src/worktree/mod.rs:171-177` reads file content and checks `starts_with("gitdir:")` |
| `Worktree` struct has `path`, `branch`, `is_current`, `is_detached` fields | VERIFIED | `src/worktree/mod.rs:7-13` struct matches exactly |
| `is_locked` and `is_main` are NOT implemented | VERIFIED | Confirmed - these fields do not exist in `Worktree` struct |
| `find_git_root()` correctly detects worktrees by checking if `.git` is a file containing `gitdir:` | VERIFIED | `src/worktree/mod.rs:157-169` calls `is_git_file()` which checks for `gitdir:` prefix |
| Worktree module used by `src/server/routes/workspace.rs` for workspace detection | VERIFIED | `workspace.rs:64,93` uses `is_git_worktree()` |
| Worktree module used by `src/server/routes/project.rs` for git root discovery | VERIFIED | `project.rs:35` uses `find_git_root()` |

## Bugs Found

### Critical

None identified.

### High

1. **Race condition in `create_worktree()`**: No validation that the target path doesn't already exist or isn't already a worktree. Git will error, but the error message won't be user-friendly.

2. **`remove_worktree()` lacks force flag**: The function doesn't support `git worktree remove --force`, which is needed when the worktree has untracked files or is in a broken state.

3. **No handling of git worktree locked state**: `git worktree lock` can lock a worktree; `remove_worktree()` should handle the locked case (either by unlocking or reporting the lock info).

### Medium

1. **`create_worktree()` doesn't return the created worktree path**: Callers may need the actual path where the worktree was created, but the function returns `()`. Git may canonicalize or modify the path.

2. **No timeout on git commands**: `list_worktrees`, `create_worktree`, and `remove_worktree` all spawn git processes with no timeout. Long-running git operations could hang indefinitely.

3. **Error messages include raw stderr without context**: When git fails, the error message is just the stderr output. Should include the git command that was attempted.

## Improvement Suggestions

### Performance

1. **Cache worktree list**: `list_worktrees()` spawns a new git process each time. For repeated calls (e.g., in a UI that refreshes), caching with invalidation on file system changes could reduce overhead.

2. **Parallel directory scanning**: When `is_git_worktree()` is called on multiple directories (e.g., scanning project directories), parallelize the checks using `tokio::task::spawn_blocking`.

### Correctness

1. **Handle symlinks in `find_git_root()`**: The function uses `current.pop()` which loses symlink information. If `start` is a symlink, the function may resolve it implicitly via `.join(".git")` but not canonicalize the result. Consider using `std::fs::canonicalize()` to ensure consistent path handling.

2. **`list_worktrees()` edge case - empty worktree list**: The function pushes a worktree after each `worktree ` line, but the final `push_parsed_worktree()` call happens after the loop. If git returns no worktrees, the function returns an empty Vec which is correct behavior, but there's no error if `git_root` isn't actually a git repository (the command succeeds with empty output).

3. **Path canonicalization in `list_worktrees()`**: `git_root_canonical` is computed once before the loop, but `Path::new(path).canonicalize()` is called for each worktree. If the git root path contains symlinks, this comparison may fail unexpectedly.

### Maintainability

1. **Add integration tests for edge cases**:
   - Worktree with spaces in path
   - Worktree with non-ASCII characters in branch name
   - Simultaneous worktree operations
   - Git config `worktree prune` behavior

2. **Add documentation for error conditions**:
   - What happens when git is not installed
   - What happens when git version is too old (worktree support added in git 2.5)

3. **Consider adding `worktree prune` support**: Git worktrees can become "prunable" after branch deletion. A `prune_worktrees()` function would help maintain clean state.

4. **`is_git_file()` could be made pub(crate)**: The function is only used by `is_git_worktree()` and `find_git_root()` internally. If it's not needed externally, consider restricting visibility.

## Priority Actions (top 5 items to fix)

1. **Add timeout to git command executions** - Prevents indefinite hangs on git operations that may hang on network-mounted repos or slow filesystems.

2. **Add force flag to `remove_worktree()`** - Allows removal of worktrees with untracked files or broken states. Signature: `pub fn remove_worktree(git_root: &Path, path: &Path, force: bool) -> Result<(), AppError>`

3. **Improve error messages with command context** - Wrap git stderr with the actual command that failed for better debugging.

4. **Add `prune_worktrees()` function** - Provides maintenance capability for cleaning up stale worktree references.

5. **Add `is_git_worktree()` async variant for server routes** - The sync version is called in async route handlers; using blocking I/O in async context is suboptimal. Consider: `pub async fn is_git_worktree_async(dir: &Path) -> bool`