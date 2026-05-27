# Util & Worktree Architecture Review

## Verified Claims

### Util Module
- **clipboard.rs**: Functions `copy_to_clipboard()` and `read_from_clipboard()` signatures correct (lines 4-28)
- **clipboard.rs**: Feature gate `#[cfg(feature = "arboard")]` correctly implemented
- **fuzzy.rs**: Functions `fuzzy_match()` and `fuzzy_score()` signatures correct (lines 3-42)
- **fuzzy.rs**: `fuzzy_match` uses Levenshtein distance from `strsim` crate, sorts by score ascending (lower is better) - MATCHES DOC
- **fuzzy.rs**: `fuzzy_score` is case-insensitive with bonuses for start-of-string and consecutive matches - MATCHES DOC
- **truncate.rs**: Functions `truncate_lines()` and `truncate_bytes()` signatures correct (lines 1-27)
- **truncate.rs**: `truncate_lines` keeps `max_lines/2` from start and end, shows "[X lines truncated]" - MATCHES DOC
- **truncate.rs**: `truncate_bytes` safely truncates at UTF-8 character boundary, appends "... [truncated]" - MATCHES DOC
- **metrics.rs**: All struct definitions match documentation (lines 1-157)
- **metrics.rs**: Histogram bounded at 1000 elements (lines 122-124) - matches AGENTS.md "✅ FIXED" status

### Worktree Module
- **Worktree struct** (lines 7-13): All 4 fields (`path`, `branch`, `is_current`, `is_detached`) match doc
- **list_worktrees()** (line 49): Signature `pub fn list_worktrees(git_root: &Path) -> Result<Vec<Worktree>, AppError>` - MATCHES
- **create_worktree()** (lines 107-135): Signature with all 4 params matches doc
- **remove_worktree()** (line 137): Signature with `force` param matches doc
- **find_git_root()** (line 158): Signature `pub fn find_git_root(start: &Path) -> Option<PathBuf>` - MATCHES
- **is_git_worktree()** (line 180): Signature `pub fn is_git_worktree(dir: &Path) -> bool` - MATCHES
- **is_git_file()** (line 172): Signature `pub fn is_git_file(git_path: &Path) -> bool` - MATCHES
- **is_locked and is_main not implemented**: Correct - these fields do NOT exist in Worktree struct

## Incorrect/Stale Claims

### Util Module
1. **Missing documentation for `pricing.rs`**: The architecture/util.md only covers clipboard, fuzzy, truncate, and metrics. However, `src/util/pricing.rs` exists with `ModelPricing` struct and `PricingService` (84 lines). This module is completely undocumented.

### Worktree Module
1. **Stale line number references** in architecture/worktree.md line 117:
   - Doc claims "`is_git_file()` at line 36, `is_git_worktree()` at line 56"
   - Actual code: `is_git_file()` is at **line 172**, `is_git_worktree()` is at **line 180**
   - Off by ~120 lines

2. **Stale file path references** in architecture/worktree.md lines 117-118:
   - Claims `src/server/routes/workspace.rs` and `src/server/routes/project.rs` use these functions
   - These paths may be outdated - needs verification against current codebase

## Bugs Found

### Util Module
1. **None identified** - All code matches documentation or documentation is simply incomplete (missing pricing.rs)

### Worktree Module
1. **None identified** - All function signatures and implementations are correct

## Improvements Identified

### Util Module
1. **Add documentation for `pricing.rs`**: This module provides `ModelPricing` struct and `PricingService::calculate_cost()` for computing LLM API costs. Should be documented in architecture/util.md

### Worktree Module
1. **Update line number references**: Lines 117-118 in worktree.md have incorrect line numbers
2. **Verify server route references**: Check if `src/server/routes/workspace.rs` and `src/server/routes/project.rs` still exist and use the claimed functions

## Stale References

1. **architecture/worktree.md:117-118**: References to `src/server/routes/workspace.rs` and `src/server/routes/project.rs` need verification
2. **architecture/worktree.md:117**: Line numbers 36 and 56 for `is_git_file()` and `is_git_worktree()` are incorrect (actual lines 172 and 180)

## Recommendations

1. **Update architecture/util.md** to include the `pricing.rs` module covering:
   - `ModelPricing` struct with fields: `input_per_m`, `output_per_m`, `cached_discount`
   - `PricingService::new()` with hardcoded rates for OpenAI, Anthropic, Google, Minimax models
   - `PricingService::calculate_cost()` method signature

2. **Update architecture/worktree.md line references**:
   - Change line 117 from "`is_git_file()` at line 36, `is_git_worktree()` at line 56" to "`is_git_file()` at line 172, `is_git_worktree()` at line 180"

3. **Verify server route files** mentioned in worktree.md exist at:
   - `src/server/routes/workspace.rs`
   - `src/server/routes/project.rs`
   
   If they don't exist, remove or update those references.
