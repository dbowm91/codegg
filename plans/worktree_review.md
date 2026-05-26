# Worktree Architecture Review

**Review date**: 2026-05-26
**Reviewer**: Claude Code
**Source file reviewed**: `architecture/worktree.md` (118 lines)
**Source code**: `src/worktree/mod.rs` (183 lines)

---

## Summary

The architecture documentation is **mostly accurate** with minor gaps noted below.

---

## Verified Claims

| Claim | Status | Source Location |
|-------|--------|-----------------|
| Location: `src/worktree/` | ✓ Correct | `src/worktree/mod.rs` |
| Key responsibilities (5 items) | ✓ All implemented | Functions present in source |
| Function signatures | ✓ All match | Lines 49, 107, 137, 158, 180, 172 |
| Worktree struct (4 fields) | ✓ Correct | Lines 7-13 |
| Note about `is_locked`/`is_main` not implemented | ✓ Correct | Struct has only 4 fields |
| Total documentation lines | ✓ 118 lines | Verified |

---

## Function Signature Verification

| Function | Doc Signature | Actual Signature | Match |
|----------|--------------|------------------|-------|
| `list_worktrees` | `pub fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError>` | Lines 49-105 | ✓ |
| `create_worktree` | `pub fn create_worktree(git_root: &Path, path: &Path, branch: &str, create_branch: bool)` | Lines 107-135 | ✓ |
| `remove_worktree` | `pub fn remove_worktree(git_root: &Path, path: &Path, force: bool) -> Result<(), AppError>` | Lines 137-156 | ✓ |
| `find_git_root` | `pub fn find_git_root(start: &Path) -> Option<PathBuf>` | Lines 158-170 | ✓ |
| `is_git_worktree` | `pub fn is_git_worktree(dir: &Path) -> bool` | Lines 180-183 | ✓ |
| `is_git_file` | `pub fn is_git_file(git_path: &Path) -> bool` | Lines 172-178 | ✓ |

---

## Worktree Struct Verification

**Documentation (lines 73-81)**:
```rust
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_detached: bool,
}
```

**Actual (lines 7-13)**: Exact match. The note stating `is_locked` and `is_main` are not implemented is accurate.

---

## Usage Reference Verification

### `src/server/routes/workspace.rs`

| Doc Claim | Actual Usage | Line |
|-----------|-------------|------|
| Uses `is_git_worktree()` for workspace detection | `crate::worktree::is_git_worktree(&path)` | 64 |
| | `crate::worktree::is_git_worktree(&validated)` | 93 |

**Note**: Lines 36 and 56 also use `is_git_file()` (not documented in "See Also").

### `src/server/routes/project.rs`

| Doc Claim | Actual Usage | Line |
|-----------|-------------|------|
| Uses `find_git_root()` for project git root discovery | `crate::worktree::find_git_root(...)` | 35, 107 |

---

## Minor Gaps Found

1. **`is_git_file` not referenced in See Also**: The function is documented (lines 63-69) but the See Also section does not mention any files using it. `workspace.rs` uses `is_git_file()` at lines 36 and 56.

2. **`is_git_file` used instead of `is_git_worktree` in some places**: At `workspace.rs:36` and `workspace.rs:56`, `is_git_file` is called on a `.git` path directly rather than `is_git_worktree` on a directory. This is functionally equivalent but not documented.

---

## Module Organization

The worktree module contains **6 public functions** and **1 private helper**:
- `push_parsed_worktree()` (private, lines 15-47)
- `list_worktrees()`
- `create_worktree()`
- `remove_worktree()`
- `find_git_root()`
- `is_git_file()`
- `is_git_worktree()`

No submodules exist. This matches the simple structure implied by the documentation.

---

## Test Coverage

Tests exist at `tests/worktree.rs` (188 lines) with 9 test functions covering:
- `test_worktree_struct` - Basic struct construction
- `test_worktree_detached` - Detached HEAD handling
- `test_find_git_root_with_git_dir` - Finding .git directory
- `test_find_git_root_with_git_file` - Finding .git file (worktree)
- `test_list_worktrees_non_git_dir` - Error on non-git directory
- `test_list_worktrees_parses_current_and_detached` - Full parsing
- `test_create_and_remove_worktree` - Create and remove cycle
- `test_is_git_worktree_with_git_dir` - Regular .git dir not detected as worktree
- `test_is_git_worktree_with_git_file` - .git file detected as worktree
- `test_is_git_worktree_non_git_dir` - Non-git directory
- `test_is_git_file_with_gitdir_prefix` - gitdir: prefix detection
- `test_is_git_file_without_gitdir_prefix` - Non-gitdir file

---

## Conclusion

The architecture documentation is **accurate and well-maintained**. The only minor issue is the missing `is_git_file()` reference in the See Also section, since `workspace.rs` uses it directly. All function signatures, struct definitions, and claims about unimplemented fields (`is_locked`, `is_main`) are correct.