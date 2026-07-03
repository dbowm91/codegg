# Built-in Agent Definitions

This directory contains TOML definitions for all 9 compiled built-in agents.

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

## Schema

```toml
[agent]
name = "agent-name"
role = "role_name"
description = "What the agent does"
mode = "Primary" | "Subagent" | "All"
hidden = false
color = "magenta"          # optional
temperature = 0.2          # optional
steps = 24                 # optional
prompt_file = "prompts/agents/name.md"  # optional, overrides convention

[agent.permissions]
tool_name = "allow" | "deny" | "ask"
```

Run `python3 scripts/check_builtin_agents.py` to verify TOML sources match
the generated Rust output.
