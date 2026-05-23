# Command Module Architecture Review

## Verified Claims (what matches)

### Command struct in `src/command/mod.rs`
- Fields: `name`, `description`, `template`, `agent`, `model`, `subtask` (deprecated), `source` - all match
- `#[deprecated]` attribute on `subtask` field is correctly documented

### CommandConfig in `src/config/schema.rs`
- Fields: `template`, `description`, `agent`, `model`, `subtask` - all match

### Template Processing
- `execute_command_template()` correctly supports both `{{variable}}` and `{variable}` syntax
- Keys are sorted before replacement (deterministic ordering) - verified at line 145
- Missing variables remain as literal placeholders - correct

### File Format (Markdown with YAML Frontmatter)
- Empty `template:` in frontmatter correctly falls back to markdown body - verified at line 86-90
- Missing frontmatter returns error - correct

### Validation Rules
- Empty name rejected - verified at line 58-60
- Whitespace in name rejected - verified at line 61-63
- Leading `/` rejected - verified at line 64-66
- Invalid commands are logged and skipped with warning - verified at line 39

### Async File Operations
- Both `find_command_files()` and `load_command_from_file()` are `async` and use `tokio::fs` - correct

### Built-in Commands (36 total)
All 36 commands listed in the architecture doc are present in `src/tui/command.rs:79-162`, correctly implemented with aliases where specified.

### CommandRegistry (TUI)
- `new()`, `commands()`, `find_by_name_or_alias()`, `filter()` methods all exist and match documentation

---

## Bugs/Discrepancies Found

### 1. **Alias Formatting Inconsistent** (Low Priority)
**Documentation says:** `/exit | /quit, /q`  
**Actual code:** aliases stored as `["quit", "q"]` (no leading `/`)

The `CommandRegistry::new()` at line 81-83 stores aliases without the leading slash, but the documentation format shows slashes on all aliases.

### 2. **CommandRegistry Documentation Uses Wrong Struct** (Medium Priority)
**Documentation shows in "Key Types" section:**
```rust
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    ...
}
```

**This is `src/command/mod.rs::Command`, but the documentation section is titled "CommandRegistry (src/tui/command.rs)"** which actually uses a different `Command` struct (`src/tui/command.rs:25-37`) with fields: `name`, `aliases`, `description`, `category`, `dialog`, `template`, `agent`, `model`, `subtask`, `source`.

The architecture doc shows the wrong `Command` struct for the TUI integration section.

### 3. **Plugin Commands Not Documented** (Medium Priority)
`src/command/plugin.rs` exists with `PluginCommand` enum and `run_plugin_command()` function, but the architecture document doesn't mention plugin commands at all. The file provides `/plugin list`, `/plugin search <query>`, `/plugin install <source>` subcommands.

### 4. **Command Priority Order Differs from Implementation** (Low Priority)
**Documentation says:** Built-in → Config → File (built-in highest priority)  
**Actual implementation:** Built-in commands are initialized first, then config commands are appended (skipping duplicates), then file commands are appended (skipping duplicates).

The actual precedence is correct (built-in wins), but the documentation implies a linear priority chain that doesn't reflect the actual `append_dynamic_commands()` flow at lines 169-218.

### 5. **CommandRegistry filter() Returns Vec of Tuples** (Low Priority - Documentation Inaccuracy)
**Documentation says:** `pub fn filter(&self, query: &str) -> Vec<(&Command, usize)>;  // Fuzzy match`

This is correct - it returns `(&Command, usize)` tuples. However, the comment only says "Fuzzy match" when the implementation actually does scoring and sorting as well (lines 269-304).

---

## Improvement Suggestions

### High Priority

1. **Document Plugin Commands**  
   Add a section in `architecture/command.md` covering the `PluginCommand` enum and `run_plugin_command()` in `src/command/plugin.rs`. This is an independent command subsystem that's currently undocumented.

2. **Fix CommandRegistry Type Reference**  
   The "CommandRegistry (src/tui/command.rs)" section should document the actual `Command` struct at `src/tui/command.rs:25-37` (with `aliases`, `category`, `dialog` fields), not the `Command` struct from `src/command/mod.rs`.

### Medium Priority

3. **Clarify Alias Format in Table**  
   Either:
   - Change `/exit | /quit, /q` to `/exit | quit, q` (without leading slashes)
   - Or document that aliases are stored without leading slashes and the `/` prefix is added programmatically

4. **Clarify Command Precedence in Loading Process**  
   The "Sources (in priority order)" section should clarify that all three sources are merged, with built-in commands being the baseline and duplicates skipped (not that one takes absolute priority over another).

### Low Priority

5. **Document filter() Behavior More Completely**  
   The `filter()` method returns scored results sorted by relevance and truncated to 10 items. The documentation could be more explicit about this.

---

## Summary

The core `src/command/mod.rs` implementation matches the architecture document well. The main discrepancies are:
- **Plugin commands completely undocumented** (medium impact)
- **CommandRegistry section shows wrong struct type** (medium impact)
- Minor inconsistencies in alias formatting and precedence description (low impact)

The implementation is correct; the documentation needs updating to reflect the actual state of the codebase.