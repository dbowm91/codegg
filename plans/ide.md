# IDE Architecture Review

## Architecture Document
- Path: architecture/ide.md

## Source Code Location
- src/ide/

## Verification Summary
Pass (with one bug identified)

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| `is_vscode()` checks `VSCODE_IPC_HOOK`, `VSCODE_INJECTED_ENVIRONMENT`, `TERM_PROGRAM=="vscode"` | Pass | Exact match at mod.rs:44-48 |
| `is_jetbrains()` checks `JETBRAINS_REMOTE`, `JB_PRODUCT_READINESS`, `IDEA_INITIAL_DIRECTORY`, `WEBCLBROWSER_HOST` | Pass | Exact match at mod.rs:50-55 |
| `is_ide()` returns `is_vscode() \|\| is_jetbrains()` | Pass | Exact match at mod.rs:57-59 |
| `open_diff()` signature and behavior | Partial | Signature matches, but bug: line range slicing is ignored when no IDE is detected (calls `open_diff_generic(_original, _modified)` with original paths instead of sliced content) |
| `generate_unified_diff()` generates unified diff with `--- a/path`, `+++ b/path` format | Pass | Exact match at mod.rs:364-390 |
| `generate_side_by_side()` generates ANSI-colored side-by-side diff | Pass | Exact match at mod.rs:392-413 |
| VS Code integration: temp files flushed before IDE reads | Pass | mod.rs:109-124 |
| VS Code integration: file handles dropped before invoking IDE | Pass | mod.rs:132-133 |
| VS Code error messages include exit status and stderr | Pass | mod.rs:144-150 |
| JetBrains integration: `$JETBRAINS_TOOL` env var override | Pass | mod.rs:198-201 |
| JetBrains integration: Unix paths `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea` | Pass | mod.rs:200-203 |
| JetBrains integration: Windows `%PROGRAMFILES%\JetBrains\<product>\bin\idea.bat` | Pass | mod.rs:204-216 |
| JetBrains integration: falls back to `idea` in PATH | Pass | mod.rs:221 |
| Generic fallback: uses `std::env::split_paths()` for PATH parsing | Pass | mod.rs:247-248, 304-305 |
| Generic fallback: creates temp files with content | Pass | mod.rs:251-287, 308-344 |

## Issues Found

### Bugs

**Line range slicing ignored in generic fallback (High)**
- **Location**: `src/ide/mod.rs:91`
- **Issue**: When no IDE is detected, `open_diff()` calls `open_diff_generic(_original, _modified)` with the **original file paths** instead of the sliced content (`original_content`, `modified_content`)
- **Impact**: When `open_diff()` is called with `original_lines` or `modified_lines` parameters, the line range filtering is applied to `original_content` and `modified_content`, but the generic fallback ignores this and reads from the original files instead
- **Example**: Calling `open_diff("file.rs", "file.rs", Some((1, 10)), None)` will correctly slice to lines 1-10 for VS Code/JetBrains, but the generic fallback will open the entire file
- **Fix**: Change line 91 from `open_diff_generic(_original, _modified)` to `open_diff_generic(&original_content, &modified_content)`

### Inconsistencies

**Architecture doc does not mention the generic fallback path behavior fully**
- The `open_diff_generic()` description mentions "creating temporary files with the content (applying line range slicing if provided)" but this is not actually implemented - the function receives original file paths, not sliced content

### Missing Documentation

**TempFilesGuard struct undocumented**
- The `TempFilesGuard` struct (lines 7-27) ensures temp files are cleaned up on drop, but this RAII pattern is not mentioned in the architecture doc

**register_panic_cleanup() undocumented**
- The `register_panic_cleanup()` function (lines 29-42) sets up panic hook to clean temp files, but is not documented

**`open_diff_generic()` helper function not listed in Key Functions**
- The architecture doc lists 6 key functions but omits `open_diff_generic()`, `open_diff_vscode()`, and `open_diff_jetbrains()`

**No mention of temp file prefix naming**
- Temp files use `codegg_original_` and `codegg_modified_` prefixes (lines 99-106, 160-167) - not documented

## Improvement Opportunities

1. **Add missing functions to architecture doc**: Document `open_diff_generic()`, `open_diff_vscode()`, and `open_diff_jetbrains()` as internal helpers
2. **Document RAII cleanup pattern**: Explain `TempFilesGuard` ensures temp files are deleted when functions return or panic
3. **Document panic cleanup**: Explain the `register_panic_cleanup()` mechanism for cleaning stray temp files on panic
4. **Add `generate_side_by_side()` output format**: Document the `=== path ===` header and `─────────────────────────────────────────────────` separator format

## Recommendations

1. **Critical**: Fix the line range slicing bug in `open_diff()` at line 91 - change `open_diff_generic(_original, _modified)` to `open_diff_generic(&original_content, &modified_content)`
2. Update architecture doc to clarify that `open_diff_generic()` receives file paths (not sliced content) and cannot apply line range slicing in the generic fallback case, OR fix the bug to pass sliced content
3. Add documentation for the helper functions and cleanup mechanisms
4. Consider adding `#[cfg(test)]` coverage for `open_diff_vscode`, `open_diff_jetbrains`, and `open_diff_generic` with mocked IDE detection
