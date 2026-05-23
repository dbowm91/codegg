# Command Module

The `command` module provides slash command registry loaded from markdown files and configuration.

## Overview

**Location**: `src/command/`

**Key Responsibilities**:
- Slash command registration from markdown files (`command/` and `commands/` directories)
- Command resolution from configuration (`opencode.jsonc`)
- Template variable substitution with deterministic ordering
- Command name validation

## Key Types

### Command (src/command/mod.rs:9-18)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    #[deprecated(since = "2026-05-22", note = "subtask field is not yet implemented")]
    pub subtask: Option<bool>,
    pub source: String,
}
```

Note: The TUI `Command` struct with aliases is in `src/tui/command.rs:26-37`.
```

### CommandConfig (src/config/schema.rs)

```rust
pub struct CommandConfig {
    pub template: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
}
```

## Command Loading

### Sources (in priority order)

1. **Built-in commands**: 36 hardcoded commands (highest priority)
2. **Config commands**: From `opencode.jsonc` `commands` section
3. **File commands**: From `command/` or `commands/` directories in CWD

### File Format (Markdown with YAML Frontmatter)

```markdown
---
description: A test command
agent: build
template: "Review the file: {file}"
---
Fallback body template if template not specified in frontmatter
```

**Note**: If `template:` is empty or missing in frontmatter, the markdown body is used as the template.

### Validation Rules

Command names must:
- Not be empty
- Not contain whitespace
- Not start with `/`

Invalid commands are logged and skipped with a warning.

## Template Processing

### Variable Substitution

```rust
pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String
```

- Supports both `{{variable}}` and `{variable}` syntax
- **Deterministic ordering**: Keys are sorted before replacement to ensure consistent output
- Missing variables remain as literal placeholders (e.g., `{name}` stays if `name` not provided)

### Available Variables (TUI Execution)

Currently only `args` is available during TUI execution:
- `{args}` - Everything after the command name (space-separated arguments)

## TUI Integration

### CommandRegistry (src/tui/command.rs:25-37)

```rust
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: CommandCategory,
    pub dialog: Option<Dialog>,
    pub template: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
    pub source: Option<String>,
}
```

### Built-in Commands (36 total)

| Command | Aliases | Description |
|---------|---------|-------------|
| `/connect` | | Connect provider |
| `/exit` | `quit`, `q` | Exit the app |
| `/status` | | View status |
| `/themes` | | Switch theme |
| `/help` | | Help |
| `/sessions` | `resume`, `continue` | Switch session |
| `/new` | `clear` | New session |
| `/share` | | Share session |
| `/unshare` | | Unshare session |
| `/rename` | | Rename session |
| `/compact` | `summarize` | Compact session |
| `/timeline` | | Jump to message |
| `/fork` | | Fork from message |
| `/undo` | | Undo previous message |
| `/redo` | | Redo |
| `/export` | | Export session transcript |
| `/import` | | Import session |
| `/timestamps` | `toggle-timestamps` | Toggle timestamps |
| `/thinking` | `toggle-thinking` | Toggle thinking |
| `/models` | | Switch model |
| `/models-refresh` | `refresh-models` | Refresh model list |
| `/variants` | | Switch model variant |
| `/agents` | | Switch agent |
| `/mcps` | | Manage MCP servers |
| `/workspaces` | | Manage workspaces |
| `/tree` | | Show file tree |
| `/editor` | | Open editor |
| `/keybinds` | | Customize keybindings |
| `/context` | | View context window usage |
| `/cost` | | View token usage and cost |
| `/usage` | | View rate limits and quota |
| `/tui` | `fullscreen` | Toggle fullscreen mode |
| `/loop` | | Schedule periodic task |
| `/tasks` | | List background tasks |
| `/task-del` | | Delete background task |
| `/memory` | | Memory dashboard |
| `/memory-search` | | Search memories |
| `/memory-list` | | List memories |
| `/memory-remember` | | Remember something |
| `/memory-forget` | | Forget a memory |
| `/memory-consolidate` | | Consolidate session into memories |

### Dynamic Commands

Dynamic commands from config and files are appended to built-in commands. **Built-in commands take precedence** - duplicates are skipped.

### Plugin Commands (`src/command/plugin.rs`)

Plugin commands via the `/plugin` subcommand:

```rust
#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins
    List,
    /// Search available plugins
    Search { query: String },
    /// Install a plugin
    Install { source: String },
}
```

### Command Execution (src/tui/app/mod.rs)

When a command with a template is executed:

1. If command has `dialog` set → open that dialog
2. If command has `template`:
   - Extract `args` from user input after command name
   - Render template with `{args}` variable
   - Add rendered text as user message
   - Trigger agent processing

## Error Handling

- **File read failures**: Logged with `tracing::warn`
- **Parse failures**: Logged and skipped
- **Invalid command names**: Logged and skipped
- **Config load failures**: Falls back to empty config (non-fatal)

## Async File Operations

Both `find_command_files()` and `load_command_from_file()` are async functions using `tokio::fs` for proper non-blocking I/O:

```rust
pub async fn find_command_files(base: &Path) -> Vec<Command>
pub async fn load_command_from_file(path: &Path) -> Result<Command, String>
```

## Recent Changes (2026-05-22)

- **Async file loading**: `find_command_files()` and `load_command_from_file()` now use `tokio::fs` for async I/O
- **`subtask` field deprecated**: Added `#[deprecated]` attribute to `subtask` field as it's not yet implemented
- Fixed unused variable warnings in `load_command_from_file()` - refactored to tuple destructuring
- Removed orphaned `src/tui/app/commands.rs` file (was never module-declared, contained duplicate command handlers)
- Fixed non-deterministic HashMap iteration in template substitution (keys now sorted)
- Added command name validation (rejects empty, whitespace, leading `/`)
- Added logging for command loading failures
- Empty `template:` in frontmatter now correctly falls back to markdown body
- Improved duplicate detection across command sources

## See Also

- [.opencode/skills/command/SKILL.md](../.opencode/skills/command/SKILL.md) - Agent guidance for command module
- [.opencode/docs/command/AGENTS.override.md](../.opencode/docs/command/AGENTS.override.md) - Module-specific override
- [tui.md](tui.md) - TUI command input handling
- [agent-loop/SKILL.md](../.opencode/skills/agent-loop/SKILL.md) - Agent execution with command templates