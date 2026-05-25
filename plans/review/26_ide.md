# IDE Module Architecture Review (2026-05-25)

## Verified Correct Items

| Item | Location | Notes |
|------|----------|-------|
| `is_vscode()` | `src/ide/mod.rs:80-84` | Exact match to doc |
| `is_jetbrains()` | `src/ide/mod.rs:86-91` | Exact match to doc |
| `is_ide()` | `src/ide/mod.rs:93-95` | Exact match to doc |
| `generate_unified_diff()` | `src/ide/mod.rs:371-397` | Signature and behavior match |
| `generate_side_by_side()` | `src/ide/mod.rs:399-420` | Signature and behavior match |
| VS Code temp file pattern | `src/ide/mod.rs:131-178` | Creates temp files, flushes, drops before IDE invoke |
| JetBrains paths | `src/ide/mod.rs:222-245` | `$JETBRAINS_TOOL`, `/opt/intellij/bin/idea.sh`, `/usr/local/bin/idea`, Windows path |
| `open_diff_generic()` PATH search | `src/ide/mod.rs:260-261` | Uses `std::env::split_paths()` correctly |
| IdeServer `run_socket()` | `src/mcp/ide_server.rs:121-144` | Uses tokio `UnixListener` with `handle_connection()` |
| IdeServer `clone_for_connection()` | `src/mcp/ide_server.rs:146-153` | Properly clones Arc fields for new connection |

## Incorrect/Stale Items

### 1. VS Code Integration Code Example (lines 78-95)
**Problem**: The code example shows a simplified version that doesn't match `open_diff_vscode()`:
- Doc shows `original_file.write_all(original_content.as_bytes())` but actual uses `original_file.write_all(original_content.as_bytes())` with a different structure (scoped blocks)
- Actual function uses `TempFilesGuard` which is not shown
- Error format in actual code is `"{} failed (exit {})"` not `"vscode diff failed (exit {}): {}"`

**Fix**: Update example to reflect actual `open_diff_vscode()` at `src/ide/mod.rs:131-178`

### 2. Generic Fallback Description (line 109)
**Problem**: Says "Unlike IDE-specific handlers that use the original file paths" - but `open_diff()` always reads original files first (lines 103-106), slices if needed, then creates temp files. The IDE-specific handlers don't use original file paths either; they all use temp files.

**Fix**: Clarify that all handlers use temp files after slicing.

### 3. IdeServer `run_stdio()` Example (lines 119-125)
**Problem**: Shows `let (reader, mut writer) = (tokio::io::stdin(), tokio::io::stdout());` which doesn't match actual implementation at `src/mcp/ide_server.rs:78-81`:
```rust
let stdin = BufReader::new(tokio::io::stdin());
let mut stdout = tokio::io::stdout();
```

**Fix**: Update example to match actual implementation.

### 4. IdeServer `run_socket()` Response Handling (lines 131-143)
**Problem**: The doc shows a simplified loop structure. Actual implementation at `src/mcp/ide_server.rs:155-194` uses `handle_connection()` which splits the stream and processes line-by-line with proper error handling.

**Fix**: Either remove the detailed code example or update to reflect `handle_connection()` pattern.

### 5. Line Range Slicing in `open_diff()` (lines 46-56)
**Problem**: The function signature shows `_original: &str, _modified: &str` but these are file paths, not content. The doc doesn't clarify this.

**Fix**: Add clarification that parameters are file paths, not content strings.

## Minor Issues

| Issue | Location | Fix |
|-------|----------|-----|
| Indentation error in `open_diff_generic()` | `src/ide/mod.rs:302` | `run_command_with_timeout` call is indented incorrectly (extra 4 spaces), though functionally correct |
| Doc doesn't mention `TempFilesGuard` | N/A | RAII cleanup mechanism not documented |
| Doc doesn't mention `IDE_COMMAND_TIMEOUT` | N/A | 30-second timeout not documented |
| Doc doesn't mention `register_panic_cleanup()` | N/A | Panic hook for temp file cleanup not documented |

## Summary

The architecture doc is **mostly accurate** but has several code examples that don't match the actual implementation. The most significant issues are:
1. VS Code integration code example needs updating to show `TempFilesGuard` usage
2. IdeServer examples need updating to match actual tokio I/O patterns
3. Generic fallback description needs clarification about temp file usage

No bugs found in the actual code - the implementation matches the intended behavior described in the skill file.