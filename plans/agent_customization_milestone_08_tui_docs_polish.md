# Agent Customization Milestone 8: TUI Commands, Diagnostics, and Documentation Polish

## Goal

Expose the generated built-in and customizable agent system through usable TUI/CLI commands, source-aware diagnostics, and clear documentation.

This milestone turns the lower-level registry work into a practical OpenCode/Codex-style customization experience while preserving Codegg-specific defaults and safety behavior.

## Commands

Add or complete:

```text
/agents
/agents --all
/agents show <name>
/agents diff <name>
/agents validate
/agents reload
/agents create
/agent <name>
```

Subagent mention support should work with custom agents where applicable:

```text
@security-review review the current diff
@research investigate this library choice
@rust-reviewer check this module
```

## `/agents`

Default view should show visible primary/all agents and visible subagents grouped separately:

```text
Primary agents:
  build      Implementation agent
  plan       Planning agent
  research   Research agent

Subagents:
  general
  explore
  security-review
  rust-reviewer
```

`/agents --all` should include hidden/system agents and disabled agents, clearly marked.

## `/agents show <name>`

Show resolved agent metadata:

```text
name: security-review
role: security_reviewer
mode: subagent
runtime: security_review
model: tier.frontier -> eggpool/gpt-5.5
hidden: false
sources:
  builtin: generated
  project: .codegg/agents/security-review.toml
permissions:
  read: allow
  security: allow
  lsp: allow
  edit: deny
  bash: ask
prompt:
  builtin prompt + project override metadata
```

Do not dump very long prompts by default. Provide a preview and source path. Add a verbose flag later if needed.

## `/agents diff <name>`

This is the most important diagnostics command. It should show how overlays changed a built-in or prior layer.

Example:

```text
security-review

source stack:
  builtin: generated
  project: .codegg/agents/security-review.toml

changed fields:
  model: None -> tier.frontier
  temperature: 0.1 -> 0.05

unchanged critical fields:
  runtime.kind: security_review
  edit: deny
  write: deny
  security: allow
  lsp: allow
```

For custom-only agents, show that no built-in base exists.

For `replace = true`, show replacement prominently:

```text
warning: project file replaced built-in security-review
```

## `/agents validate`

Print registry diagnostics:

```text
ok: 9 built-ins loaded
ok: 2 project agents loaded
warning: .codegg/agents/foo.toml: unknown field "modell"
warning: .codegg/agents/bar.toml: prompt_file missing; agent skipped
```

Return/emit a failure status if validation is invoked from a CLI/headless context and error-level diagnostics exist.

## `/agents reload`

Reload user/project agent files without restarting the TUI where feasible. Built-ins do not reload because they are compiled.

Reload should:

1. Re-read global/project agent files.
2. Rebuild the registry.
3. Preserve current agent if it still exists and remains selectable.
4. Fall back to default agent if current agent disappeared or became disabled.
5. Emit diagnostics.

## `/agents create`

Interactive creator should write a minimal safe custom agent file. Default to least privilege:

```text
mode: subagent
edit/write/apply_patch: deny
bash: ask
task: deny
read/search/list: allow
```

Offer templates:

```text
read-only reviewer
research agent
test runner
implementation helper
security reviewer override
```

The first implementation can generate TOML only. Markdown prompt file generation can be added as an option.

## `/agent <name>`

Select active primary/all agent for future turns.

Behavior:

- Validate the agent exists.
- Reject subagent-only agents with a clear message.
- Persist active selection in session state if session persistence exists.
- Emit an event/message showing selected agent and model.

Example:

```text
selected agent: research (model: tier.frontier -> eggpool/gpt-5.5)
```

## Mention/task UX

Mention completion should include visible subagent/all agents and exclude primary-only agents unless explicitly allowed.

Task tool validation should reject attempts to spawn unknown or primary-only agents.

## Documentation

Add docs covering three surfaces:

### Built-in maintainer assets

```text
assets/agents/*.toml
assets/prompts/**/*.md
scripts/generate_builtin_agents.py
src/agent/builtins/generated.rs
```

Explain that generated Rust is committed and normal users do not need asset files at runtime.

### User/global agents

```text
~/.config/codegg/agents/*.toml
~/.config/codegg/agents/*.md
```

### Project agents

```text
.codegg/agents/*.toml
.codegg/agents/*.md
```

Document merge-by-default behavior, explicit replacement, disabling, prompt files, permissions, and model inheritance.

## Examples

Add example files:

```text
examples/agents/rust-reviewer.toml
examples/agents/docs-writer.toml
examples/agents/security-review-override.toml
examples/agents/research-frontier.toml
```

Each example should include a brief explanation of when to use it.

## Tests

Add command-level tests where feasible:

- `/agents` lists built-ins and custom agents.
- `/agents --all` includes hidden agents.
- `/agents show security-review` includes source stack and permissions.
- `/agents diff security-review` shows project override fields.
- `/agents validate` reports invalid files.
- `/agent build` succeeds.
- `/agent security-review` fails if subagent-only.
- Mention completion includes custom subagents.

## Acceptance criteria

- Users can inspect, validate, reload, and select agents from the TUI/CLI.
- Source-aware diagnostics are visible through commands.
- Customization docs exist and include examples.
- Built-in maintainer generation docs exist.
- Agent selection rejects invalid mode usage.
- Mention/task UX respects custom agents and mode constraints.

## Handoff notes

Do not dump full prompts in normal TUI views. Prompts can be long and should remain inspectable through explicit verbose/debug paths. Prioritize source stack, effective model, effective permissions, runtime kind, and diagnostics.
