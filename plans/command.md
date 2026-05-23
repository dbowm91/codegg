# Command Architecture Review

## Architecture Document
- Path: architecture/command.md

## Source Code Location
- src/command/ (mod.rs, plugin.rs)
- src/tui/command.rs (TUI CommandRegistry)

## Verification Summary
Partial

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| Command struct at src/command/mod.rs:9-18 | Pass | Exact match |
| CommandConfig struct at src/config/schema.rs | Pass | Exact match |
| TUI Command struct at src/tui/command.rs:25-37 | Pass | Exact match |
| Template processing (sorted keys, {{var}} and {var}) | Pass | execute_command_template() at mod.rs:142-152 correctly implements sorted deterministic ordering |
| Async file loading (tokio::fs) | Pass | Both functions are async |
| Built-in commands count (36) | Pass | All 36 commands present in CommandRegistry::new() |
| Validation rules (empty, whitespace, leading /) | Pass | validate_command_name() at mod.rs:57-68 |
| PluginCommand enum structure | Pass | Exact match at plugin.rs:5-19 |
| Error handling (warn logging, non-fatal config failures) | Pass | Matches implementation |
| Sources priority (built-in, config, file) | Pass | Built-in first, then config commands, then file commands |
| Dynamic commands appended after built-in | Pass | append_dynamic_commands() adds new_commands after built-ins |
| subtask field deprecated | Pass | #[deprecated] attribute present |

## Issues Found

### Inconsistencies

1. **Duplicate command tables**: The architecture document has two identical tables listing built-in commands (lines 117-159 and 161-205). They are exact duplicates.

2. **Alias format discrepancies**: The document shows aliases with leading slashes (e.g., `/resume`, `/continue`, `/clear`) but actual implementation uses aliases WITHOUT leading slashes (e.g., `resume`, `continue`, `clear`).
   - `/sessions` alias: doc says `/resume`, `/continue`; actual: `resume`, `continue`
   - `/new` alias: doc says `/clear`; actual: `clear`
   - `/timestamps` alias: doc says `/toggle-timestamps`; actual: `toggle-timestamps`
   - `/thinking` alias: doc says `/toggle-thinking`; actual: `toggle-thinking`
   - `/models-refresh` alias: doc says `/refresh-models`; actual: `refresh-models`
   - `/tui` alias: doc says `/fullscreen`; actual: `fullscreen`

### Missing Documentation

1. **CommandCategory enum**: Not documented in architecture. Only shown in TUI integration code but should be in the Key Types section. Values are Session, Agent, System.

2. **CommandRegistry methods**:
   - `find_by_name_or_alias()` - not documented
   - `filter()` - not documented
   - `commands()` - not documented

3. **Global static**: `COMMAND_REGISTRY` at src/tui/command.rs:313 is a `LazyLock<CommandRegistry>` used globally but not documented.

4. **Helper functions**: `normalize_name()` and `to_slash_name()` at lines 242-252 are undocumented.

5. **Async variant**: `append_dynamic_commands_async()` at lines 220-240 is not documented.

6. **Plugin tier display**: PluginCommand shows tier (Free/Pro/Enterprise) in output but this is not documented.

7. **load_command_from_file fallback behavior**: The function correctly falls back to markdown body when template is empty/missing (line 110), but this behavior is documented in the File Format section while the implementation actually handles this case at line 86-90 with `cfg.template.is_empty()` check before falling back.

### Improvement Opportunities

1. **Remove duplicate table**: The second table (lines 161-205) is redundant and should be removed.

2. **Fix alias examples**: Either update document to show aliases without leading slashes, or update implementation to include leading slashes in aliases (the latter would be a breaking change).

3. **Add CommandCategory documentation**: Document the three categories (Session, Agent, System) used to organize commands.

4. **Document CommandRegistry public API**: Add method documentation for find_by_name_or_alias, filter, and commands.

5. **Document execution flow more clearly**: The "Command Execution (src/tui/app/mod.rs)" section references handler code but the actual handler logic is in tui/command.rs, not app/mod.rs.

6. **Plugin tier documentation**: The PluginCommand enum at plugin.rs:62-78 shows tier output but architecture only shows enum structure, not the output format.

## Recommendations

1. Remove duplicate command listing table in architecture/command.md
2. Update alias examples in the table to show actual implementation values (without leading slash)
3. Add CommandCategory enum to Key Types section
4. Document CommandRegistry public methods
5. Verify whether aliases should have leading slashes - if implementation is correct, update doc; if doc is correct, fix implementation
