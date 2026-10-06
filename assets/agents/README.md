# Built-in Agent Definitions

This directory contains TOML definitions for all 10 compiled built-in agents.

Each file defines one agent's metadata and permissions. Prompt text lives in
`../prompts/agents/` as Markdown files (referenced by `prompt_file` or by
convention matching the agent name).

**Do not edit generated Rust files directly.** Edit these TOML sources and
re-run `python3 scripts/generate_builtin_agents.py` to regenerate.

## Agents

| Agent | Mode | Hidden | Description |
|-------|------|--------|-------------|
| build | Primary | No | Default agent with full permissions |
| plan | Primary | No | Read-only agent for planning |
| general | Subagent | No | Subagent without todo/goal management |
| explore | All | No | Read-only exploration agent |
| title | Subagent | Yes | Generates session titles |
| summary | Subagent | Yes | Generates session summaries |
| compaction | Subagent | Yes | Context compaction agent |
| security-review | Subagent | No | Defensive security review |
| research | All | No | Long-horizon research agent |
| verifier | Subagent | Yes | Independent read-only semantic verifier |

## Schema

```toml
[agent]
name = "agent-name"
role = "role_name"
description = "What the agent does"
mode = "Primary" | "Subagent" | "All"   # built-in TOML files use capitalized modes
hidden = false
color = "magenta"          # optional
temperature = 0.2          # optional
steps = 24                 # optional
runtime_kind = "standard"  # optional; see below
prompt_file = "prompts/agents/name.md"  # optional, overrides convention

[agent.permissions]
tool_name = "allow" | "deny" | "ask"
```

`runtime_kind` selects Rust-defined runtime behavior and is not free-form.
The accepted values are `standard` (the default), `security_review`,
`research`, `compaction`, `title`, and `summary`; any other value is a load
error. Leave it unset for a normal agent.

> **Note:** Built-in agent TOML files (this directory) use capitalized mode values (`Primary`, `Subagent`, `All`). User-defined agent TOML files loaded at runtime require lowercase mode values (`primary`, `subagent`, `all`). This is because built-in files are compiled by the Python generator into Rust enum variants, while user files pass through `parse_mode()` which only accepts lowercase.

`python3 scripts/generate_builtin_agents.py --check` verifies that the TOML
sources and prompt files still match the generated Rust output. Run it without
`--check` to regenerate. CI runs the same check and fails if the generated file
is stale.

The full key list accepted at runtime, and the overlay flags (`extends`,
`merge`, `replace`, `disable`) available to user-defined agents, are documented
in `docs/agents-skills.md`.
