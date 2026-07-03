# Example Agent Files

This directory contains example agent definitions demonstrating the agent customization system.

## Files

| File | Format | Purpose |
|------|--------|---------|
| `code-reviewer.toml` | TOML `[agent]` | Focused code review subagent |
| `test-writer.toml` | TOML `[agent]` | Test-writing subagent with bash patterns |
| `docs-writer.toml` | TOML `[agent]` | Documentation subagent with path permissions |
| `custom-build-override.toml` | TOML flat | Override built-in `build` agent with stricter permissions |
| `markdown-agent.md` | Markdown | Agent defined using YAML frontmatter + markdown body |

## Installation

### Global (all projects)

Copy to `~/.config/codegg/agents/`:

```bash
cp examples/agents/code-reviewer.toml ~/.config/codegg/agents/
```

### Project-specific

Copy to `.codegg/agents/` in your project root:

```bash
cp examples/agents/custom-build-override.toml .codegg/agents/
```

## Usage

Once installed, agents are available immediately:

- **Switch agent**: `/agent code-reviewer` or press `Ctrl+X A` and select
- **Spawn as subagent**: Type `@code-reviewer review this code` in the prompt
- **List all agents**: `/agents` or `/agents --all` (includes hidden)
- **View agent details**: `/agents show code-reviewer`
- **Compare against built-in**: `/agents diff code-reviewer`
- **Validate configuration**: `/agents validate`
- **Reload after changes**: `/agents reload`

## TOML Format

### Wrapped format (recommended)

```toml
[agent]
name = "my-agent"
mode = "subagent"          # case-insensitive: Primary, SUBAGENT, All, etc.
description = "A custom agent"

[agent.permissions]
read = "allow"
write = "deny"
bash = "ask"

[agent.bash_permission]    # fine-grained bash control (TOML only)
action = "ask"
allow_patterns = ["git diff*", "cargo test*"]
deny_patterns = ["rm *", "sudo *"]

[agent.path_permission]    # fine-grained file access control (TOML only)
allow = ["src/**", "tests/**"]
deny = [".git/**", "target/**"]
```

### Flat format

```toml
name = "my-agent"
mode = "subagent"
description = "A custom agent"

[permission]
read = "allow"
write = "deny"
```

## Markdown Format

```markdown
---
name: my-agent
mode: subagent
description: A custom agent
---

You are a helpful assistant.
```

> **Note:** Markdown is a **prompt-first, merge-only** format. It supports flat `permission` maps and `disable` in frontmatter but does not support overlay flags (`replace`, `merge`) or structured permission sections (`[bash_permission]`, `[path_permission]`). Use TOML for those features.

## Overlay Flags

Control how file-based agents interact with built-in agents (TOML only):

```toml
name = "build"
mode = "primary"
description = "Custom build override"

# replace = true   → Full replacement (discard built-in entirely)
# replace = false  → Merge mode (default): overlay fields applied on top
# merge = true     → Explicitly enable merge mode (same as default)
# disable = true   → Remove agent from resolution entirely
```

## Permission Actions

| Action | Behavior |
|--------|----------|
| `allow` | Execute without asking |
| `ask` | Prompt for user confirmation |
| `deny` | Block execution entirely |

## Resolution Order

1. Compiled built-ins (9 agents)
2. Global files (`~/.config/codegg/agents/`)
3. Project files (`.codegg/agents/`)
4. Config `agent` overrides
5. Config `mode` compatibility overrides

Later layers override earlier layers. Within layers, field-level merge is used.
