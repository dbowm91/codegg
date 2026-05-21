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

### Command (src/command/mod.rs)

```rust
pub struct Command {
    pub name: String,           // Command name (e.g., "review")
    pub description: Option<String>,
    pub template: String,         // Template with {{variable}} or {variable} placeholders
    pub agent: Option<String>,   // Optional agent to route to
    pub model: Option<String>,   // Optional model override
    pub subtask: Option<bool>,   // Optional subtask flag
    pub source: String,          // "config" or file path
}
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

1. **Config commands**: From `opencode.jsonc` `commands` section
2. **File commands**: From `command/` or `commands/` directories in CWD

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

### CommandRegistry (src/tui/command.rs)

```rust
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    pub fn new() -> Self;
    pub fn commands(&self) -> &[Command];
    pub fn find_by_name_or_alias(&self, name: &str) -> Option<&Command>;
    pub fn filter(&self, query: &str) -> Vec<(&Command, usize)>;  // Fuzzy match
}
```

### Built-in Commands

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

### Dynamic Commands

Dynamic commands from config and files are appended to built-in commands. **Built-in commands take precedence** - duplicates are skipped.

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

## Recent Changes (2026-05-21)

- Fixed non-deterministic HashMap iteration in template substitution (keys now sorted)
- Added command name validation (rejects empty, whitespace, leading `/`)
- Added logging for command loading failures
- Empty `template:` in frontmatter now correctly falls back to markdown body
- Improved duplicate detection across command sources

## See Also

- [tui.md](tui.md) - TUI command input handling
- [agent-loop/SKILL.md](../skills/agent-loop/SKILL.md) - Agent execution with command templates
