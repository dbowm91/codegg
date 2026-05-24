# Command Module Review

## Overview

Reviewed `architecture/command.md` against the actual implementation in `src/command/` and `src/tui/command.rs`, plus the skill at `.opencode/skills/command/SKILL.md`.

---

## Summary of Verification

| Item | Status | Notes |
|------|--------|-------|
| Command struct fields | VERIFIED | `src/command/mod.rs:8-18` matches docs |
| CommandConfig struct | VERIFIED | `src/config/schema.rs:396-402` matches docs |
| TUI Command struct | VERIFIED | `src/tui/command.rs:25-37` matches docs |
| Template execution | VERIFIED | `execute_command_template()` at `mod.rs:160-170` with sorted keys |
| Async file loading | VERIFIED | `find_command_files()` and `load_command_from_file()` are async |
| Command name validation | VERIFIED | `validate_command_name()` at `mod.rs:65-76` |
| Frontmatter parsing | VERIFIED | `parse_frontmatter()` at `mod.rs:172-184` |
| Deprecated subtask field | VERIFIED | `#[deprecated]` attribute present at `mod.rs:15` |
| PluginCommand enum | VERIFIED | `src/command/plugin.rs:5-19` |
| Built-in command count | **MISMATCH** | Docs say 36, actual count is 41 (see below) |

---

## Discrepancies Found

### 1. Built-in Command Count Incorrect

**Issue**: Documentation states "36 hardcoded commands" but the actual count is **41 commands**.

**Reference**: `src/tui/command.rs:78-163`

Counting `Command::new()` invocations in `CommandRegistry::new()`:
- Lines 79-80: `/connect`
- Lines 81-83: `/exit` (with aliases `quit`, `q`)
- Line 84: `/status`
- Line 85: `/themes`
- Line 86: `/help`
- Lines 87-89: `/sessions` (with aliases `resume`, `continue`)
- Lines 90-92: `/new` (with alias `clear`)
- Lines 93-94: `/share`
- Lines 95-96: `/unshare`
- Lines 97-98: `/rename`
- Lines 99-101: `/compact` (with alias `summarize`)
- Lines 102-103: `/timeline`
- Lines 104-105: `/fork`
- Lines 106-107: `/undo`
- Line 108: `/redo`
- Lines 109-110: `/export`
- Lines 111-112: `/import`
- Lines 113-115: `/timestamps` (with alias `toggle-timestamps`)
- Lines 116-118: `/thinking` (with alias `toggle-thinking`)
- Lines 119-120: `/models` (with Dialog::Model)
- Lines 121-123: `/models-refresh` (with alias `refresh-models`)
- Lines 124-125: `/variants`
- Lines 126-127: `/agents` (with Dialog::Agent)
- Lines 128-129: `/mcps` (with Dialog::Mcp)
- Lines 130-131: `/workspaces`
- Line 132: `/tree`
- Line 133: `/editor`
- Lines 134-135: `/keybinds` (with Dialog::Keybind)
- Lines 136-137: `/context`
- Lines 138-139: `/cost`
- Lines 140-141: `/usage`
- Lines 142-144: `/tui` (with alias `fullscreen`)
- Lines 145-146: `/loop`
- Lines 147-148: `/tasks`
- Lines 149-150: `/task-del`
- Lines 151-152: `/memory`
- Lines 153-154: `/memory-search`
- Lines 155-156: `/memory-list`
- Lines 157-158: `/memory-remember`
- Lines 159-160: `/memory-forget`
- Lines 161-162: `/memory-consolidate`

**Total: 41 commands** (not 36)

**Affected Documents**:
- `architecture/command.md:52` - says "36 hardcoded commands"
- `architecture/command.md:115` - section title says "36 total"
- `.opencode/skills/command/SKILL.md:41` - says "36 built-in slash commands"

### 2. Skill Line Number Reference Inaccurate

**Issue**: The skill at `.opencode/skills/command/SKILL.md:173` references `src/command/mod.rs:141` for template execution, but the actual line is `mod.rs:160`.

**Actual location**: `src/command/mod.rs:160-170`

This is a minor documentation drift due to code changes adding functions before `execute_command_template`.

### 3. Skill Line Number for Frontmatter Parsing Off

**Issue**: The skill at `.opencode/skills/command/SKILL.md:174` says frontmatter parsing is at `mod.rs:153` but actual is `mod.rs:172`.

**Actual location**: `src/command/mod.rs:172-184`

### 4. Skill Line Number for CommandRegistry Off

**Issue**: The skill at `.opencode/skills/command/SKILL.md:175` says CommandRegistry is at `src/tui/command.rs:78` but actual line for built-in commands is `src/tui/command.rs:78` - this one is actually correct (the 36 vs 41 issue is the real discrepancy here).

---

## Code Bugs Found

### 1. Bug: `find_command_files()` panics on Error

**Location**: `src/command/mod.rs:20-25`

```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base).into_iter().map(|r| r.unwrap_or_else(|e| {
        warn!("Failed to load command: {}", e);
        panic!("expected")  // <-- BUG: This panics!
    })).collect()
}
```

**Problem**: When a command file fails to load, the code logs a warning but then immediately panics with `panic!("expected")`. This is clearly a bug - error handling was not completed properly. The function should either:
1. Skip the failed command (like the sync version does) and continue
2. Return an empty Vec or error

**Fix**: Change `panic!("expected")` to simply skip the failed command:
```rust
pub async fn find_command_files(base: &Path) -> Vec<Command> {
    find_command_files_sync(base).into_iter().filter_map(|r| r.ok()).collect()
}
```

**Note**: The sync version `find_command_files_sync()` correctly returns `Vec<Result<Command, String>>` and lets callers handle errors gracefully. The async wrapper should do the same or filter out errors.

---

## Recommendations

### For Documentation

1. **Update built-in command count**: Change "36" to "41" in:
   - `architecture/command.md:52`
   - `architecture/command.md:115` 
   - `.opencode/skills/command/SKILL.md:41`

2. **Update line number references in skill**:
   - Template execution: change `141` to `160`
   - Frontmatter parsing: change `153` to `172`

3. **Add documentation for `find_command_files_sync()`**: This function exists but is undocumented in the architecture. It's currently used by `CommandRegistry` for dynamic command loading.

### For Code

1. **Fix `find_command_files()` panic bug**: As described above at `mod.rs:23`

2. **Consider documenting the CommandRegistry static initialization**: The `CommandRegistry` uses `LazyLock` to initialize on first access. This is an implementation detail worth noting.

---

## Verified Correct Items

The following items were confirmed correct after direct inspection:

- **Command struct**: All fields match between docs and code (`src/command/mod.rs:8-18`)
- **CommandConfig struct**: All fields match (`src/config/schema.rs:396-402`)
- **TUI Command struct**: All fields match (`src/tui/command.rs:25-37`)
- **Template substitution**: Both `{{var}}` and `{var}` syntax work, keys are sorted for determinism
- **Async file loading**: Both `find_command_files()` and `load_command_from_file()` use `tokio::fs`
- **Command name validation**: Empty names, whitespace, and leading `/` are all rejected
- **Deprecated subtask field**: Has proper `#[deprecated]` attribute
- **Plugin commands**: `List`, `Search`, `Install` all implemented in `plugin.rs`
- **Frontmatter fallback**: Body used when template is empty/missing (but NOT when template key is absent from frontmatter entirely - see skill note at line 210)
- **Built-in precedence**: Built-in commands skip duplicates from config and file commands
- **Error handling for file errors**: Logged with `tracing::warn`
- **`/search` command**: Special case handled separately from registry at `app/mod.rs:3637-3643`

---

## File:Line References for Issues

| Issue | File | Line(s) |
|-------|------|---------|
| `find_command_files()` panic bug | `src/command/mod.rs` | 23 |
| Wrong command count in arch doc | `architecture/command.md` | 52, 115 |
| Wrong command count in skill | `.opencode/skills/command/SKILL.md` | 41 |
| Wrong line ref (template exec) | `.opencode/skills/command/SKILL.md` | 173 |
| Wrong line ref (frontmatter) | `.opencode/skills/command/SKILL.md` | 174 |
| CommandRegistry built-ins | `src/tui/command.rs` | 78-163 |
