# IDE Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `is_vscode()` checks VSCODE_IPC_HOOK, VSCODE_INJECTED_ENVIRONMENT, TERM_PROGRAM | VERIFIED | mod.rs:6-10 matches exactly |
| `is_jetbrains()` checks JETBRAINS_REMOTE, JB_PRODUCT_READINESS, IDEA_INITIAL_DIRECTORY, WEBCLBROWSER_HOST | VERIFIED | mod.rs:12-17 matches exactly |
| `is_ide()` returns is_vscode() \|\| is_jetbrains() | VERIFIED | mod.rs:19-21 matches exactly |
| `open_diff()` uses temp files for JetBrains and VS Code diffs | VERIFIED | mod.rs:48-54, temp files created in open_diff_vscode/open_diff_jetbrains |
| `open_diff()` slices content when line ranges provided | VERIFIED | mod.rs:34-46 correctly slices using 1-indexed to 0-indexed conversion |
| `generate_unified_diff()` produces --- a/path, +++ b/path format | VERIFIED | mod.rs:302-328 matches exactly |
| `generate_side_by_side()` produces ANSI colored output | VERIFIED | mod.rs:330-351 uses \u{001b}[32m style codes |
| VS Code Integration uses --diff CLI with temp files flushed before IDE reads | VERIFIED | mod.rs:57-109: flush() called, then drop(original_temp) before Command::new() |
| JetBrains Integration supports $JETBRAINS_TOOL env var override | VERIFIED | mod.rs:147-170 checks env var first |
| JetBrains Integration checks Unix paths (/opt/intellij/bin/idea.sh, /usr/local/bin/idea) | VERIFIED | mod.rs:149-152 matches doc |
| JetBrains Integration checks Windows %PROGRAMFILES%\JetBrains\... path | VERIFIED | mod.rs:153-168 matches doc |
| Generic fallback creates temp files with content and passes to IDE | VERIFIED | mod.rs:192-299: creates temp files with original_content/modified_content |
| Generic fallback uses std::env::split_paths() for PATH parsing | VERIFIED | mod.rs:193-194, 246-247 use split_paths() |
| Error messages include exit status and stderr | VERIFIED | mod.rs:100-106, 181-187 include exit status and stderr |
| Temp files are dropped before invoking IDE | VERIFIED | mod.rs:88-89 (vscode), 144-145 (jetbrains), 229-230 (generic) |

## Bugs Found

### High

**1. Line range slice panic on invalid input (mod.rs:34-46)**
```rust
let start_idx = start.saturating_sub(1).min(lines.len());
let end_idx = end.min(lines.len());
original_content = lines[start_idx..end_idx].join("\n").to_string();
```
If `start > end` and `start > lines.len()`, the slice `lines[start_idx..end_idx]` will panic with "range start index out of range". No validation that `start <= end`.

**2. No timeout on external command execution (mod.rs:91-98, 172-179, 232-239, 285-292)**
All `Command::new(...).output()` calls have no timeout. If VS Code or JetBrains hangs (e.g., modal dialog, license acceptance), the calling code blocks indefinitely.

**3. open_diff_generic() PATH parsing edge case (mod.rs:193, 246)**
```rust
std::env::var("PATH").unwrap_or_default()
```
If PATH is empty or not set, `split_paths("")` returns an iterator over one empty string, which `join("code")` becomes just "code" (current directory). An attacker could place a malicious `code` binary in current directory.

### Medium

**4. generate_side_by_side() hardcoded context of 3 (mod.rs:336)**
```rust
for op in diff.grouped_ops(3) {
```
This is not configurable. Large changes won't show full context in side-by-side view.

**5. Windows IDEA detection missing .cmd extension (mod.rs:247)**
Checks for `idea` and `idea.bat` but not `idea.cmd` which is the actual Windows executable.

**6. Windows code detection missing .exe (mod.rs:194)**
Checks for `code` and `code.exe` but Windows also uses `.cmd` extension.

**7. generate_unified_diff() has_changes check is incomplete (mod.rs:318-325)**
Only checks for `+` or `-` at start of line, but context lines (spaces) aren't counted as changes. If a diff has only context changes (no actual additions/deletions), it returns "(no changes)" correctly, but the logic is fragile.

### Low

**8. No cleanup of temp files on panic**
Temp files persist if process crashes between creation and IDE invocation. No RAII guard or cleanup handler registered.

**9. open_diff() reads entire files when only one side has line range**
If `original_lines=Some(...)` and `modified_lines=None`, the code still reads the full modified file into memory, potentially wasting memory for large files.

**10. Error messages don't suggest fixes**
"failed to open vscode: ..." doesn't tell user to install VS Code or verify it's in PATH.

## Improvement Suggestions

### Performance

1. **Add timeout to Command::output()** - Wrap in `tokio::process::Command` with `output().await` timeout, or use `std::process::Command` with `output()` inside `spawn_blocking` + timeout.

2. **Lazy file reading** - Only read files when needed (when IDE is detected). Currently `open_diff` always reads both files regardless of IDE availability.

3. **Cache IDE detection** - `is_vscode()` and `is_jetbrains()` are called multiple times but env vars don't change during runtime. Could cache result.

### Correctness

1. **Validate line range input** - Add check that `start <= end` and both are >= 1.

2. **Add .cmd extension for Windows** - Detection should check `idea.cmd` and `code.cmd` on Windows.

3. **Make side-by-side context configurable** - Add parameter or config option for grouped_ops context size.

### Maintainability

1. **Extract common temp file creation** - VS Code and generic fallback duplicate identical temp file creation logic. Extract to helper function.

2. **Extract common command execution** - VS Code, JetBrains, and generic all have similar `.output().map_err(...).and_then(check_status)` patterns. Could use helper.

3. **Add integration tests** - Only unit tests exist. Should test actual VS Code/JetBrains invocation with mock binaries.

4. **Document that open_diff() modifies original_content/modified_content** - The line range slicing modifies the content variables in place. Calling code may not expect mutation.

5. **Add feature flag for IDE integration** - Could gate IDE functionality behind `ide` feature flag to reduce binary size for environments without IDE support.

## Priority Actions (top 5 items to fix)

1. **Add line range validation (start <= end)** - Prevents panic on invalid input in mod.rs:34-46. High priority.

2. **Add command execution timeout** - Prevent hangs when IDE blocks. Use 30-second timeout.

3. **Fix Windows extension detection (.cmd)** - Add `idea.cmd` and `code.cmd` to PATH searches on Windows.

4. **Add PATH edge case handling** - Handle empty PATH more securely in `open_diff_generic()`.

5. **Extract temp file creation helper** - Reduce code duplication between VS Code and generic fallback functions.

## Additional Notes

- The architecture document is accurate and matches implementation
- Temp file handle bug (drop before IDE reads) is correctly implemented
- Error messages include stderr as documented
- All 9 architecture claims verified as correct