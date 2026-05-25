# Command Module Architecture Review (2026-05-25)

## Verified Correct Items

1. **Command struct** (`src/command/mod.rs:9-18`) - Accurate
2. **CommandConfig struct** (`src/config/schema.rs:396-402`) - Accurate
3. **TUI Command struct** (`src/tui/command.rs:25-37`) - Accurate
4. **TUI CommandRegistry** (`src/tui/command.rs:72-309`) - Accurately documented
5. **Template processing** (`src/command/mod.rs:160-170`) - `execute_command_template` uses sorted keys for deterministic ordering
6. **Validation rules** (`src/command/mod.rs:65-76`) - Empty, whitespace, leading `/` correctly rejected
7. **PluginCommand enum** (`src/command/plugin.rs:5-19`) - Matches documentation
8. **`subtask` field deprecated** - Correctly marked with `#[deprecated]` attribute
9. **Orphaned `src/tui/app/commands.rs` removed** - Verified not present
10. **Priority order** (built-in > config > file) - Correctly implemented

## Incorrect/Stale Items

### 1. Command Count Mismatch (BUG)
- **Line 52**: "41 hardcoded commands" - CORRECT
- **Line 115**: "### Built-in Commands (36 total)" - WRONG, it's **41 commands**

**Fix**: Update line 115 to say "41 total"

### 2. Async File Loading Misleading (DOCUMENTATION ISSUE)
- **Line 52**: States "41 hardcoded commands"
- **Lines 199-206**: Claims both `find_command_files()` and `load_command_from_file()` are async using `tokio::fs`

**Actual behavior**:
- `find_command_files()` (line 20) wraps `find_command_files_sync()`
- `find_command_files_sync()` (line 27) uses **blocking** `std::fs::read_dir`
- `load_command_from_file()` (line 78) uses **async** `tokio::fs::read_to_string` - TRUE
- `load_command_from_file_sync()` (line 85) uses blocking `std::fs::read_to_string`

**TUI usage** (`src/tui/command.rs:206-217`):
```rust
let dynamic_commands = std::thread::scope(|s| {
    s.spawn(|| {
        crate::command::find_command_files_sync(&base)
```
The TUI uses blocking I/O via thread scope, NOT the async version.

**Fix**: Update lines 199-206 to clarify:
- `load_command_from_file()` is truly async
- `find_command_files()` is a sync wrapper (blocking fs operations)
- TUI uses blocking thread-spawn approach

## Bugs Found in Related Code

### 1. Async `find_command_files()` is Misleading
The async function at line 20-25 just wraps sync version:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base)
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}
```
This is not truly async I/O - it's a sync function called in an async context. Should either:
- Be made truly async with `tokio::fs`, or
- Be removed/deprecated

### 2. No Async File Loading in TUI
The `CommandRegistry::new()` at line 206-217 uses blocking `std::thread::scope` with `find_command_files_sync`. If async loading is a goal, this should use `tokio::fs` in an async context.

## Line-Specific Updates Needed

| Line | Issue | Fix |
|------|-------|-----|
| 52 | Correct - "41 hardcoded commands" | No change |
| 115 | Says "36 total" | Change to "41 total" |
| 199-206 | Overstates async nature | Clarify `load_command_from_file` is async, `find_command_files` is sync wrapper |

## Summary

The architecture doc is mostly accurate. The main issue is:
1. **Command count error**: 36 vs 41 (line 115)
2. **Async claims overstated**: Only `load_command_from_file()` is truly async; `find_command_files()` is a sync wrapper
3. **No actual bugs** in the documented functionality itself - implementation matches doc intent
