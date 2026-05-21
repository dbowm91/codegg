# Command Module Override

This file contains command-specific guidance and overrides root AGENTS.md.

## Command Loading

Commands are loaded from two sources:
1. **`opencode.jsonc`** `commands` section (highest priority)
2. **Markdown files** in `command/` or `commands/` directories in CWD

### File Format

```markdown
---
description: Optional description
agent: optional-agent
model: optional-model
template: "Template with {args} or {{variable}}"
---
Fallback body (used if template is empty or missing)
```

### Template Substitution

- Both `{var}` and `{{var}}` syntax supported
- **Only `args` variable available** during TUI execution
- Keys are **sorted before replacement** for deterministic output

## Common Issues

### Template rendering inconsistent

If a template produces different output on different runs, check that:
1. Variable values don't contain other variable names that could be double-replaced
2. The sort order of HashMap keys is deterministic (fixed: keys are now sorted)

### Command not found

Built-in commands take precedence over dynamic commands. If a file-based command has the same name as a built-in, it will be silently skipped.

### Empty template fallback not working

If a markdown file has `template:` in frontmatter but it's empty, the body is NOT used. Only when `template:` is completely absent from frontmatter does it fall back to body.