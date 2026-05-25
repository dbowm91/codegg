---
name: command
description: Slash command registry, template processing, and TUI command handling
version: 1.1.0
tags:
  - commands
  - slash-commands
  - templates
  - tui
---

# Command Module Guide

Slash command system for the TUI, including loading from markdown files, config, and template processing.

## Key Components

### Core Command Module (`src/command/`)

Handles loading and parsing commands from files and config.

```rust
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    #[deprecated(since = "2026-05-22", note = "subtask field is not yet implemented")]
    pub subtask: Option<bool>,
    pub source: String,  // "config" or file path
}
```

**Files:**
- `src/command/mod.rs` - Command loading, template processing
- `src/command/plugin.rs` - Plugin marketplace commands (List, Search, Install)

### TUI CommandRegistry (`src/tui/command.rs`)

Manages built-in commands and dynamic commands with 41 built-in slash commands.

```rust
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: CommandCategory,
    pub dialog: Option<Dialog>,
    pub template: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,  // deprecated
    pub source: Option<String>,
}
```

**Categories:** `Session`, `Agent`, `System`

### CommandPalette (`src/tui/components/dialogs/command.rs`)

TUI dialog for fuzzy-filtering and executing commands.

## Command Loading

### Sources (in priority order)

1. **Built-in commands**: 41 hardcoded commands (highest priority)
2. **Config commands**: From `opencode.jsonc` `commands` section
3. **File commands**: From `command/` or `commands/` directories in CWD

### File Format (Markdown with YAML Frontmatter)

```markdown
---
description: A test command
agent: build
template: "Review the file: {file}"
---
Fallback body template if template not specified
```

**Note:** If `template:` is empty or missing, the markdown body is used as the template.

### Validation Rules

Command names must:
- Not be empty
- Not contain whitespace
- Not start with `/`

Invalid commands are logged and skipped.

## Template Processing

### `execute_command_template()` (`src/command/mod.rs:141`)

```rust
pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String
```

- Supports both `{{variable}}` and `{variable}` syntax
- **Deterministic ordering**: Keys are sorted before replacement
- Missing variables remain as literal placeholders

## TUI Command Execution (`src/tui/app/mod.rs`)

### Entry Points

1. **Direct slash command** in prompt (line 3634):
   - `handle_slash_command(text)` extracts command name, looks up in `COMMAND_REGISTRY`
   - Calls `execute_command(cmd, Some(trimmed))`

2. **Command palette selection** (line 2671):
   - Executes selected command from palette

### `execute_command()` Implementation (line 2702)

```rust
fn execute_command(&mut self, cmd: &Command, raw_input: Option<&str>) {
    // 1. If command has dialog -> open that dialog, return
    if let Some(dialog) = &cmd.dialog { ... open dialog ... return; }

    // 2. If command has template -> render and queue for agent
    if let Some(template) = &cmd.template {
        // Extract args after command name
        // Render template with {args} variable
        // Add as user message, set pending_send = true
        // TUI event loop detects pending_send and spawns AgentLoop
        return;
    }

    // 3. Built-in commands without template (match on cmd.name)
    match cmd.name.as_str() {
        "/exit" | "/quit" | "/q" => { ... }
        "/help" => { ... }
        // etc.
    }
}
```

## Built-in Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/connect` | | Connect provider |
| `/exit` | `/quit`, `/q` | Exit the app |
| `/status` | | View status |
| `/themes` | | Switch theme |
| `/help` | | Help |
| `/sessions` | `/resume`, `/continue` | Switch session |
| `/new` | `/clear` | New session |
| `/compact` | `/summarize` | Compact session |
| `/models` | | Switch model |
| `/agents` | | Switch agent |
| `/mcps` | | Manage MCP servers |
| `/tree` | | Show file tree |
| `/editor` | | Open editor |
| `/keybinds` | | Customize keybindings |
| `/context` | | View context window usage |
| `/cost` | | View token usage and cost |
| `/usage` | | View rate limits and quota |
| `/loop` | | Schedule periodic task |
| `/tasks` | | List background tasks |
| `/task-del` | | Delete background task |

## Key File Locations

| Component | File | Line |
|-----------|------|------|
| Command struct (core) | `src/command/mod.rs` | 9 |
| Command loading | `src/command/mod.rs` | 70, 123 |
| Template execution | `src/command/mod.rs` | 160 |
| Frontmatter parsing | `src/command/mod.rs` | 172 |
| CommandRegistry (41 built-ins) | `src/tui/command.rs` | 72 |
| Dynamic command loading | `src/tui/command.rs` | 169, 207 |
| CommandPalette dialog | `src/tui/components/dialogs/command.rs` | 15 |
| Command execution (TUI) | `src/tui/app/mod.rs` | 2808 |
| Slash command handling | `src/tui/app/mod.rs` | 3732 |
| CommandConfig schema | `src/config/schema.rs` | 396 |

## Architecture Notes

1. **Two command systems exist**: The `src/command/` module handles markdown-file/config commands for the agent, while `src/tui/command.rs` provides the TUI's built-in slash commands

2. **Async file loading**: `find_command_files()` and `load_command_from_file()` are now async functions using `tokio::fs`

3. **Template substitution is deterministic**: Keys are sorted before replacement (fixed from non-deterministic HashMap ordering)

4. **Commands with `dialog` field bypass template execution**: They immediately open the specified dialog

5. **`pending_send` acts as a guard**: Prevents concurrent command executions while a prompt is being processed

6. **`subtask` field is deprecated**: The `subtask` field on `Command` is deprecated and should not be used - it has no execution behavior

## Common Issues

### Template rendering inconsistent

If a template produces different output on different runs, check that:
1. Variable values don't contain other variable names that could be double-replaced
2. The sort order of HashMap keys is deterministic (keys are now sorted)

### Command not found

Built-in commands take precedence over dynamic commands. If a file-based command has the same name as a built-in, it will be silently skipped.

### Empty template fallback not working

If a markdown file has `template:` in frontmatter but it's empty, the body is NOT used. Only when `template:` is completely absent from frontmatter does it fall back to body.

## Recent Changes

- **2026-05-22**: `find_command_files()` and `load_command_from_file()` are now async using `tokio::fs`
- **2026-05-22**: `subtask` field on `Command` is now deprecated with `#[deprecated]` attribute
- **2026-05-22**: Fixed unused variable warnings in `load_command_from_file()` - refactored to tuple destructuring
- **2026-05-22**: Removed orphaned `src/tui/app/commands.rs` file (was never module-declared)
- **2026-05-21**: Fixed non-deterministic HashMap iteration (keys now sorted)
- **2026-05-21**: Added command name validation (rejects empty, whitespace, leading `/`)
- **2026-05-21**: Empty `template:` in frontmatter now correctly falls back to markdown body

## See Also

- `.opencode/docs/command/AGENTS.override.md` - Module-specific override guidance
- `.skills/tui/SKILL.md` - TUI development overview
- `architecture/command.md` - Architecture documentation
