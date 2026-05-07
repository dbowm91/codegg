# Command Module

The `command` module provides slash command registry loaded from markdown files.

## Overview

**Location**: `src/command/`

**Key Responsibilities**:
- Slash command registration
- Command template processing
- Variable substitution

## Key Types

### Command

```rust
pub struct Command {
    pub name: String,
    pub description: String,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<String>,
}
```

### CommandRegistry

```rust
pub struct CommandRegistry {
    commands: RwLock<HashMap<String, Command>>,
}

impl CommandRegistry {
    pub fn load_from_dir(&self, path: &Path) -> Result<()>;
    pub fn get(&self, name: &str) -> Option<Command>;
    pub fn list(&self) -> Vec<Command>;
}
```

## Command File Format

```markdown
---
name: review
description: Start a code review session
agent: reviewer
model: claude-opus-4
---

# Code Review Command

Perform a comprehensive code review of the changes:

1. Run `git diff` to see changes
2. Analyze for bugs, security issues, style
3. Provide actionable feedback

Variables:
- {{files}}: Files to review (default: all changed)
```

## Variable Substitution

Commands support `{{variable}}` syntax:

```rust
pub fn substitute(command: &Command, vars: &HashMap<String, String>) -> String {
    let mut result = command.template.clone();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}
```

## Built-in Commands

| Command | Description |
|---------|-------------|
| `/help` | Show help |
| `/new` | Start new session |
| `/session` | Manage sessions |
| `/model` | Switch model |
| `/skill:<name>` | Activate skill |

## See Also

- [tui.md](tui.md) - TUI command input
