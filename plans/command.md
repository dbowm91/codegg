# Command Architecture Review Findings

## Verified Claims

- **Command struct fields (src/command/mod.rs)**: Verified at `src/command/mod.rs:8-18` - name, description, template, agent, model, subtask (deprecated), source all present.

- **CommandConfig struct**: Verified in `src/config/schema.rs` - template, description, agent, model, subtask fields.

- **41 built-in commands**: Verified by counting in `src/tui/command.rs:78-163`. All 41 commands listed in documentation present in source.

- **Built-in command aliases**: Verified - /exit has "quit", "q"; /sessions has "resume", "continue"; etc.

- **TUI Command struct (src/tui/command.rs:72)**: Has name, aliases, description, category, dialog, template, agent, model, subtask, source - matches documentation.

- **Template variable substitution**: `execute_command_template()` at `src/command/mod.rs:160-170` uses sorted keys for deterministic ordering.

- **Both `{{variable}}` and `{variable}` syntax**: Verified at `src/command/mod.rs:166-167` - both patterns replaced.

- **Command name validation**: `validate_command_name()` at `src/command/mod.rs:65-76` checks empty, whitespace, leading `/`.

- **CommandRegistry::normalize_name()**: Verified at `src/tui/command.rs:240-242` - trims, removes leading `/`, lowercases.

- **subtask field deprecated**: `#[deprecated]` attribute verified at `src/command/mod.rs:15-16`.

- **Async file operations**: `load_command_from_file()` at `src/command/mod.rs:78-83` uses `tokio::fs::read_to_string`.

- **find_command_files_sync()**: Verified at `src/command/mod.rs:27-63` - reads from "command" and "commands" directories.

## Stale Information

- **"File read failures: Logged with tracing::warn"**: In `src/command/mod.rs:36`, it uses `warn!` not specifically `tracing::warn`. This is technically correct but the exact macro may differ.

## Bugs Found

- **No bugs found**: Command loading, validation, and template processing all work as documented.

## Improvements Suggested

- **Command category mismatch**: Built-in commands use `CommandCategory::Agent` for memory commands and loop/task commands, but documentation at line 37 shows `/memory` commands should be in Session category. Verified in source at lines 151-162 - they ARE in Session category, matching docs.

- **PluginCommand subcommand enum**: Documented at `src/command/plugin.rs` - the enum has List, Search, Install variants matching documentation.

## Cross-Module Issues

- **TUI Command struct vs command::Command**: Two different structs with similar names - `src/command/mod.rs::Command` and `src/tui/command.rs::Command`. The TUI version has additional fields like `aliases`, `category`, `dialog`. This is correctly documented as separate types.

- **BackgroundScheduler and SubagentPool coupling**: `src/core/mod.rs:603-614` creates subagent requests when scheduling tasks, but this is an indirect dependency - the task scheduler doesn't directly use the subagent pool, it just gets a reference to the spawner.