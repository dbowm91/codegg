# Skills Module Override

This file contains skills-specific guidance and overrides root AGENTS.md.

## Overview

Skills provide specialized instructions and workflows for specific tasks. They are located in `.opencode/skills/<skill_name>/SKILL.md`.

## Skill Loading

Skills are loaded by opencode when a task matches their triggers:

```yaml
# In opencode.json
skills:
  - name: agent-loop
    description: "Guide for AgentLoop integration with TUI and event-driven architecture"
```

## Key Skills

| Skill | Purpose |
|-------|---------|
| `agent-loop` | AgentLoop integration, streaming, Provider trait, async patterns |
| `client` | Remote TUI client via WebSocket, connection flow, timeouts |
| `event-bus` | GlobalEventBus pub/sub, all AppEvent types |
| `mcp` | MCP client/server, local/remote connections, OAuth |
| `permission` | Permission system, DoomLoopDetector, Mode system |
| `provider` | LLM provider system, 25+ providers, ScriptedProvider |
| `snapshot` | File state capture and restore (restore added 2026-05-21) |
| `subagent` | SubAgentPool, bounded concurrency, shutdown patterns |
| `tui` | TUI development with Ratatui, Component trait, dialogs |
| `tool` | Tool trait, registry, built-in tools, ToolExecutor |

## Updating Skills

When making changes to a module:

1. Update the relevant `SKILL.md` in `.opencode/skills/<module>/`
2. Update the relevant `AGENTS.override.md` in `.opencode/docs/<module>/`
3. Ensure AGENTS.md index references are correct

## Skill Triggers

Each skill defines triggers that cause opencode to load it:

```yaml
---
name: skill-name
description: What the skill covers
triggers:
  - keyword1
  - keyword2
---
```