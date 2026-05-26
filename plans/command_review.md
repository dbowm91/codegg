# Command Architecture Review

## Summary

The command architecture documentation is comprehensive and accurate. 41 built-in commands correctly documented and verified. Found stale line references and minor documentation inconsistencies that should be corrected.

## Verified Correct

- **Command struct** (`src/command/mod.rs:8-18`): All fields match (name, description, template, agent, model, subtask with deprecation, source)
- **Built-in command count** (`src/tui/command.rs:78-163`): 41 commands correctly implemented and verified at lines 79-163 in CommandRegistry::new()
- **Built-in commands match table** (lines 79-163): All commands from doc table `/connect` through `/memory-consolidate` present and match descriptions
- **Validation rules** (`src/command/mod.rs:65-76`): Empty, whitespace, leading `/` validation confirmed
- **Template substitution** (`src/command/mod.rs:160-170`): Deterministic ordering with `sorted_keys.sort()` at line 163 confirmed; both `{{var}}` and `{var}` syntax supported
- **Async file loading** (`src/command/mod.rs:20-25,78-83`): Async wrappers using `tokio::fs` confirmed - `find_command_files()` at line 20-25 wraps sync, `load_command_from_file()` at line 78-83 uses `tokio::fs::read_to_string`
- **subtask deprecation** (`src/command/mod.rs:15`): `#[deprecated]` attribute confirmed
- **PluginCommand enum** (`src/command/plugin.rs:5-19`): `List`, `Search`, `Install` variants match documentation
- **Dynamic commands priority** (`src/tui/command.rs:165`): `append_dynamic_commands()` called in `new()`, built-ins precede dynamic commands confirmed
- **File format (frontmatter)** (`src/command/mod.rs:172-184`): `parse_frontmatter()` correctly parses markdown with YAML frontmatter
- **Fallback to body** (`src/command/mod.rs:128`): `template.unwrap_or_else(|| body.trim().to_string())` confirmed
- **CommandRegistry struct** (`src/tui/command.rs:25-37`): All fields match doc (name, aliases, description, category, dialog, template, agent@, model, subtask, source)
- **resolve_commands_from_config** (`src/command/mod.rs:142-158`): Function exists and correctly maps CommandConfig to Command

## Discrepancies Found

- **TUI Command struct line reference** (doc: `src/tui/command.rs:26-37`, actual: lines 25-37): Documentation line numbers are essentially correct but off by 1-2 lines due to struct comments
- **Command struct line reference** (doc: `src/command/mod.rs:9-18`, actual: lines 8-18): Off by 1 line

## Stale Items in Architecture Doc

- **Built-in commands line reference** (doc: lines 79-163 table shows inline, actual: same): The documentation's table format doesn't include file/line references for individual commands - should add file references for clarity
- **"Recent Changes (2026-05-22)" section** (doc: entire section 208-218): These changes may no longer be "recent" and could be removed or datestamped for historical reference
- **Duplicate detection** (doc: "Improved duplicate detection", actual: `src/tui/command.rs:169-238`): The doc claims improved duplicate detection but doesn't specify the mechanism (normalized name HashMap in `append_dynamic_commands`)

## Bugs Identified

None. The implementation is correct and well-tested.

## Improvement Suggestions

- **Add line reference to CommandRegistry**: The doc references `src/tui/command.rs:25-37` for the TUI Command struct but doesn't reference `CommandRegistry` at line 72 - should clarify this
- **Document the normalization mechanism**: The duplicate detection uses `normalize_name()` which lowercases and strips leading `/` - this behavioral detail could be documented
- **Clarify PluginCommand execution**: `run_plugin_command()` in `src/command/plugin.rs:21` exists but documentation doesn't show how it's invoked from the CLI/slash command system
- **Remove "Recent Changes" or make historical**: Section 208-218 with datestamp "2026-05-22" should either be integrated into the main doc or moved to a changelog
- **Consider documenting test coverage**: Tests at lines 186-267 in `src/command/mod.rs` provide good coverage for template substitution and frontmatter parsing - could be mentioned
