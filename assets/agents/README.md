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
model = "provider/model"   # optional
fallback_model = "provider/model"  # optional
runtime_kind = "standard"  # optional; see below
prompt_file = "prompts/agents/name.md"  # optional, overrides convention
git_read_only = true       # optional; see below

[agent.permissions]
tool_name = "allow" | "deny" | "ask"
```

`runtime_kind` selects Rust-defined runtime behavior and is not free-form.
The accepted values are `standard` (the default), `security_review`,
`research`, `compaction`, `title`, and `summary`; any other value is a load
error. Leave it unset for a normal agent.

`git_read_only` grants `git` for inspection only: every invocation
`codegg-git` does not classify read-only is refused at the tool boundary.
`[agent.permissions]` is per *tool*, so `git = "allow"` would also authorize
`git push`, and `git = "deny"` costs the agent the repository facts it needs.
The flag never widens anything — an agent that declares it must also be
permitted to use `git` at all, and any `deny` on `git` still applies.

> **Note:** Built-in agent TOML files (this directory) use capitalized mode values (`Primary`, `Subagent`, `All`). User-defined agent TOML files loaded at runtime pass through `parse_mode()`, which lowercases its input before matching, so it accepts either casing. The capitalization is therefore a *generator* requirement, not a runtime one: the Python generator only accepts the capitalized forms, and user files are parsed at runtime.

### Permission keys are tool names

Every key under `[agent.permissions]` is matched against the name of a
registered tool, so the key must be spelled exactly as the tool names it.
A key that matches no registered tool is **inert**: it is never consulted and
never an error, which means a stale key is a silent no-op rather than a
failure. Two rules follow:

- Use the tool's own name (`python_script`, `question`), not a near-miss
  (`python`, `permission`). Both of the latter were once written into
  `verifier.toml` and denied nothing.
- A denied tool that no longer exists is dead weight; remove it rather than
  leaving it to read as a guarantee.

`"*"` is the only non-tool key: it is a wildcard that matches **every** tool
name, so `compaction`'s single `"*" = "deny"` rule denies everything.

`python3 scripts/generate_builtin_agents.py --check` verifies that the TOML
sources and prompt files still match the generated Rust output. Run it without
`--check` to regenerate. CI runs the same check and fails if the generated file
is stale.

The full key list accepted at runtime, and the overlay flags (`extends`,
`merge`, `replace`, `disable`) available to user-defined agents, are documented
in `docs/agents-skills.md`.

## Source verification

**2026-10-23** — audited read-only against the repository working tree:

- `python3 scripts/generate_builtin_agents.py --check` passes: `src/agent/builtins/generated.rs` and `src/agent/builtins/mod.rs` match the TOML sources.
- All 10 TOML files parse and satisfy the generator's schema: known top-level keys, a valid `mode`, a valid permission action, a valid `runtime_kind`, and a `prompt_file` that exists.
- Every `[agent.permissions]` key across all 10 agents was checked against the 62 production tool names derived from `impl Tool for … name()` across `src/`. Two keys in `verifier.toml` matched no tool — `python` and `permission` — and were removed; `python_script` and `question` were already denied under their real names.
- The note on mode casing was corrected: `parse_mode()` lowercases its input (`src/agent/definition.rs`), so runtime-loaded agent files accept either casing. Only the generator is case-sensitive, and it accepts the capitalized forms.
