# Agent Customization Milestone 5: User and Project Custom Agents

## Goal

Allow users and projects to add or customize agents using TOML and Markdown files while preserving compiled built-ins as safe defaults.

This milestone introduces the main user-facing customization surface:

```text
~/.config/codegg/agents/*.toml
~/.config/codegg/agents/*.md
.codegg/agents/*.toml
.codegg/agents/*.md
```

TOML is the canonical structured format. Markdown remains useful for prompt-heavy agents with frontmatter.

## Required behavior

Global agents load from:

```text
~/.config/codegg/agents/
```

Project agents load from:

```text
.codegg/agents/
```

Resolution order remains:

```text
compiled built-ins
  -> global files
  -> project files
  -> config.agent
  -> config.mode
```

Project files should override global files. Config-level overrides should override file-based overlays.

## TOML agent format

Support minimal TOML like:

```toml
schema_version = 1
name = "rust-reviewer"
mode = "subagent"
description = "Rust-focused correctness reviewer."
prompt_file = "prompts/rust-reviewer.md"

[permission]
read = "allow"
grep = "allow"
glob = "allow"
list = "allow"
lsp = "allow"
edit = "deny"
write = "deny"
bash = "ask"
```

Support inline prompt as an escape hatch:

```toml
prompt = """
You are a focused Rust reviewer. Check ownership, async cancellation, error handling, and API boundaries.
"""
```

If both `prompt` and `prompt_file` are present, prefer `prompt` unless an explicit `append_prompt_file = true` or similar flag is later introduced. Emit a warning for ambiguous prompt sources.

## Markdown agent format

Support Markdown with frontmatter:

```markdown
---
schema_version: 1
name: rust-reviewer
mode: subagent
description: Rust-focused correctness reviewer.
permission:
  read: allow
  grep: allow
  glob: allow
  list: allow
  lsp: allow
  edit: deny
---

You are a focused Rust reviewer. Check ownership, async cancellation, error handling, and API boundaries.
```

Fix body-as-prompt semantics: after frontmatter parsing, the Markdown body must become the prompt unless `prompt` or `prompt_file` explicitly overrides it.

This is important because current Markdown agent loading can parse frontmatter but should not discard the body content.

## Prompt file resolution

For user/global agents, resolve relative prompt paths against the global Codegg config directory.

For project agents, resolve relative prompt paths against the project `.codegg/` directory or the containing agent file directory. Pick one convention and document it. Recommended:

```text
prompt_file is resolved relative to the directory containing the agent file.
```

This allows portable project layouts:

```text
.codegg/agents/rust-reviewer.toml
.codegg/agents/prompts/rust-reviewer.md
```

## Error handling

User/project file problems should not panic. Emit diagnostics and skip invalid files when possible.

Diagnostics should include:

- File path.
- Severity.
- Field name if known.
- Clear message.
- Suggested fix when obvious.

Examples:

```text
warning: .codegg/agents/rust-reviewer.toml: prompt_file "prompts/rust-reviewer.md" does not exist; agent skipped
warning: ~/.config/codegg/agents/foo.toml: invalid mode "worker"; expected primary, subagent, or all
```

## Registry integration

Agent files should load into `AgentSpec` values and then be merged/resolved by `AgentRegistry`. Do not bypass registry resolution.

Add source records:

```text
GlobalFile(path)
ProjectFile(path)
```

Ensure `/agents validate` can eventually print these diagnostics.

## Tests

Add tests for:

- Global TOML custom agent loads.
- Project TOML custom agent loads.
- Markdown frontmatter custom agent loads.
- Markdown body becomes prompt.
- TOML `prompt_file` loads relative to the agent file.
- Invalid TOML produces diagnostics and does not panic.
- Project agent overrides global agent with same name.
- Built-ins remain available when no user/project files exist.

Use temporary directories to avoid relying on the developer's real config directory.

## Acceptance criteria

- Codegg supports `.toml` and `.md` custom agents in global and project directories.
- Markdown body-as-prompt behavior works.
- Prompt file resolution is documented and tested.
- Invalid custom files produce diagnostics rather than panics.
- Custom agents are loaded through `AgentRegistry`.
- Built-ins remain compiled and cannot be broken by missing runtime asset files.

## Handoff notes

This milestone should make customization possible, but not yet perfect. Merge-by-default semantics and richer permissions are handled in the next milestone. Keep replacement rules conservative until overlay behavior is explicitly implemented.
