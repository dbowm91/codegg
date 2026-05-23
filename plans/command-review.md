# Command Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| Command struct has name, description, template, agent, model, subtask, source | VERIFIED | src/command/mod.rs:9-18 |
| CommandConfig has template, description, agent, model, subtask | VERIFIED | src/config/schema.rs:363-369 |
| 36 built-in commands (highest priority) | VERIFIED | src/tui/command.rs:78-163 (counted 36 entries) |
| Config commands (middle priority) | VERIFIED | src/tui/command.rs:181-202 |
| File commands from command/ and commands/ directories | VERIFIED | src/command/mod.rs:23-55 |
| Markdown format with YAML frontmatter | VERIFIED | src/command/mod.rs:154-166 |
| Empty template falls back to markdown body | VERIFIED | src/command/mod.rs:110 |
| Validation: not empty, no whitespace, not start with / | VERIFIED | src/command/mod.rs:57-68 |
| execute_command_template function with HashMap | VERIFIED | src/command/mod.rs:142-152 |
| Supports both {{variable}} and {variable} syntax | VERIFIED | src/command/mod.rs:148-149 |
| Deterministic ordering (sorted keys) | VERIFIED | src/command/mod.rs:144-145 |
| Missing variables remain as placeholders | VERIFIED | src/command/mod.rs:148-149 (no replacement if key missing) |
| TUI integration via CommandRegistry | VERIFIED | src/tui/command.rs:72-308 |
| find_by_name_or_alias method | VERIFIED | src/tui/command.rs:253-262 |
| filter method with fuzzy match | VERIFIED | src/tui/command.rs:264-299 |
| Dynamic commands appended to built-ins | VERIFIED | src/tui/command.rs:169-213 |
| Built-in commands take precedence | VERIFIED | src/tui/command.rs:185 (`if !seen.contains_key(&normalized)`) |
| Async file operations with tokio::fs | VERIFIED | src/command/mod.rs:26,71 |
| Error handling logged with tracing::warn | VERIFIED | src/command/mod.rs:29,39,46 |
| subtask field deprecated | VERIFIED | src/command/mod.rs:15-16 |
| Command execution flow (dialog then template) | UNABLE TO_VERIFY | Requires runtime tracing of execution path |

## Bugs Found

### High

**Blocking call in async context (src/tui/command.rs:204)**
```rust
let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
```
Uses blocking `std::env::current_dir()` inside `append_dynamic_commands_async` which is called from a `block_on`. While this happens to work because we're already in a Tokio runtime context, it's not idiomatic async Rust. Should use `tokio::fs::canonicalize()` or `tokio::env::current_dir()` instead.

### Medium

**Runtime handle check is fragile (src/tui/command.rs:205-209)**
```rust
if let Ok(runtime_handle) = tokio::runtime::Handle::try_current() {
    let rt = runtime_handle;
    rt.block_on(async {
        Self::append_dynamic_commands_async(&base, &mut seen, &mut new_commands).await;
    });
}
```
If called outside a Tokio runtime, dynamic commands silently fail to load with no error indication. Should either panic with clear error or log a warning.

**Plugin command system is separate implementation**
The `src/command/plugin.rs` defines its own `PluginCommand` enum and `run_plugin_command` function that is completely independent from the main `CommandRegistry` system. This means:
- Different command structures
- No unified command help/status
- No fuzzy matching or alias support
- No shared error handling

### Low

**Template replacement order issue (src/command/mod.rs:148-149)**
```rust
result = result.replace(&format!("{{{{{key}}}}}", ), value);
result = result.replace(&format!("{{{key}}}"), value);
```
If a value contains braces (e.g., value = "{$foo}"), the first pass replaces `{{name}}` with that value, then the second pass might inadvertently replace braces in the value itself if it happens to match another key. This is unlikely but possible edge case.

**No validation of template variable syntax**
If a template contains malformed placeholders like `{name` (missing closing brace), no validation catches this. The malformed placeholder remains unchanged.

## Improvement Suggestions

### Performance

1. **Pre-compute normalized names in CommandRegistry**
   The `normalize_name()` function is called repeatedly in `find_by_name_or_alias` and `filter`. Consider caching these normalized forms.

2. **Lazy loading of file-based commands**
   Currently all file commands are loaded synchronously at startup via `block_on`. Consider loading them lazily on first command invocation.

3. **Avoid double parsing in load_command_from_file**
   First tries `CommandConfig` parsing, then falls back to raw YAML value parsing. These could potentially be combined.

### Correctness

1. **Add template validation**
   Validate that all `{{variable}}` and `{variable}` placeholders are properly formed (balanced braces).

2. **Handle values containing placeholder patterns**
   When substituting `{name}`, if the value itself contains `{name}`, it could cause double-replacement issues. Consider escaping or a more robust templating approach.

3. **Consistent error propagation for runtime unavailability**
   When `tokio::runtime::Handle::try_current()` fails, log a warning instead of silently skipping dynamic commands.

### Maintainability

1. **Unify Command types**
   There are two distinct `Command` structs:
   - `crate::command::Command` (src/command/mod.rs) - for file/config commands
   - `crate::tui::command::Command` (src/tui/command.rs) - for TUI built-in commands

   These have overlapping but not identical fields. Consider creating a shared `Command` type in a common location.

2. **Document plugin command integration**
   The `src/command/plugin.rs` file is a standalone plugin system, not integrated with the main command registry. Document this clearly or consider integration.

3. **Add integration tests for template execution**
   Current tests cover individual functions but lack integration tests that verify the full command flow from loading through template execution.

## Priority Actions (top 5 items to fix)

1. **[HIGH] Replace blocking `std::env::current_dir()` with async alternative**
   File: `src/tui/command.rs:204`
   Change to use `tokio::env::current_dir()` or `tokio::fs::canonicalize()`

2. **[MEDIUM] Add error handling when runtime unavailable**
   File: `src/tui/command.rs:205-209`
   Log a warning when dynamic commands cannot be loaded due to missing runtime

3. **[MEDIUM] Consider unifying Command types**
   Create a shared Command type or clearly document why two separate types exist

4. **[LOW] Add template placeholder validation**
   Validate that placeholders are well-formed before/during substitution

5. **[LOW] Improve plugin command integration**
   Either integrate `PluginCommand` into the main registry or document it as a separate system