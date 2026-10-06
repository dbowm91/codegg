# Agents and Skills

CodeGG has two related extension points. **Agents** are named personalities
with their own model settings and tool permissions. **Skills** are portable
instruction packages the agent can load on demand.

Neither one escapes the runtime's safety envelope: the approval mode, sandbox
profile, permission rules, and security gates described in `docs/tui.md` and
`docs/security-semantics.md` apply to custom agents exactly as they do to
built-in ones.

The authoritative contracts are `architecture/agent.md` and
`architecture/skills.md`.

## Built-in agents

Ten agents ship with CodeGG. Their source of truth is `assets/agents/*.toml`.

| Agent | Role | Mode | Purpose |
|-------|------|------|---------|
| `build` | `executor` | Primary | The default agent, with full permissions |
| `plan` | `planner` | Primary | Read-only agent for planning (`write` is denied) |
| `explore` | `explorer` | All | Read-only exploration agent (`write` is denied) |
| `general` | `executor` | Subagent | Subagent without todo/goal management |
| `research` | `researcher` | All | Long-horizon research via the `research` tool, with `websearch` for quick lookups |
| `security-review` | `security_reviewer` | Subagent | Defensive security review of changed code, using `securityContext` and deterministic scanning |
| `verifier` | `semantic_verifier` | Subagent | Independent, read-only semantic verification of a producer result |
| `compaction` | `compactor` | Subagent | Context compaction (hidden, runtime-driven) |
| `summary` | `summarizer` | Subagent | Session summaries (hidden, runtime-driven) |
| `title` | `title` | Subagent | Session titles (hidden, runtime-driven) |

`mode` controls where an agent may be used: `Primary` for direct selection,
`Subagent` for delegation only, `All` for both. The four hidden agents are
driven by CodeGG itself and are not shown in the normal picker; select them
explicitly if you need to.

Switch agents in the TUI with `/agent <name>` or by mentioning one with
`@<name>`; `/agents` lists and manages them, including `show`, `diff`, and
`validate` for a named agent.

### The generated file

Built-in agents are **generated into Rust**. `src/agent/builtins/generated.rs`
is derived from `assets/agents/*.toml` and the prompt files under
`assets/prompts/agents/`.

Never edit `generated.rs` by hand. After changing an agent TOML or a prompt
file, regenerate:

```bash
python3 scripts/generate_builtin_agents.py
```

CI runs the same script in `--check` mode and fails if the generated file is
stale.

## Writing a custom agent

Agent files are TOML. Two layouts are accepted: a flat one where keys sit at
the top level, and a wrapped one where they live under an `[agent]` table. The
wrapped form is what the built-ins use and is the better default to copy.

```toml
# A read-only documentation auditor.
#
# Project use:  <project>/.codegg/agents/docs-auditor.toml
# Global use:   <config dir>/codegg/agents/docs-auditor.toml

[agent]
name = "docs-auditor"
role = "docs_auditor"
description = "Audits documentation for drift against the code it describes."
mode = "subagent"
color = "cyan"
hidden = false

[agent.permissions]
read = "allow"
glob = "allow"
grep = "allow"
list = "allow"
lsp = "allow"
websearch = "ask"
bash = "deny"
write = "deny"
edit = "deny"
apply_patch = "deny"
replace = "deny"
terminal = "deny"
commit = "deny"
image = "deny"
todowrite = "deny"
todoread = "deny"
plan_enter = "deny"
plan_exit = "deny"
```

`name` and `description` are what the model and the agent picker see, so write
the description as a capability statement rather than a label.

Recognized `[agent]` keys: `name`, `role`, `description`, `mode`, `model`,
`fallback_model`, `variant`, `temperature`, `top_p`, `prompt`, `prompt_file`,
`color`, `steps`, `hidden`, `disable`, `permissions`, `runtime_kind`,
`extends`.

Permission values are `allow`, `deny`, or `ask`. Anything else is treated as
`ask`. Denying a tool is the reliable way to make an agent genuinely
read-only — leave it out of the table and it inherits the ambient
configuration, which is not the same thing.

`runtime_kind` is not free-form: it selects the agent's runtime and is one of
`standard`, `security_review`, `research`, `compaction`, `title`, or `summary`.
Leave it unset for a normal agent. `prompt_file` points at a Markdown prompt
relative to the prompt asset root; if it is missing the agent still loads, with
a warning.

### Extending and overriding

Because an agent file is an overlay, a project file can tighten a built-in
without restating it. Use the flat form and set `extends`:

```toml
# Tighten the built-in build agent for this project only.
extends = "build"
description = "Project build agent with commits gated on approval."

[permission]
bash = "ask"
commit = "ask"
write = "ask"
edit = "ask"
```

Overlay control flags sit at the top level:

| Flag | Effect |
|------|--------|
| `merge` | Merge into the existing definition (the default) |
| `replace` | Replace the existing definition outright |
| `disable` | Remove the agent from resolution entirely |

The flat form also takes structured permission sections — `[bash_permission]`
with `action`, `allow_patterns`, and `deny_patterns`, and `[path_permission]`
with `allow` and `deny` glob lists. Note that these two sections are read from
the top level only, so they belong in the flat form:

```toml
name = "docs-writer"
description = "Writes and maintains documentation."
mode = "subagent"

[permission]
read = "allow"
write = "allow"
edit = "allow"
bash = "ask"

[path_permission]
allow = ["docs/**", "*.md", "README*", "CHANGELOG*"]
deny = [".git/**", "target/**"]

[bash_permission]
action = "ask"
allow_patterns = ["cargo fmt*", "cargo test*"]
deny_patterns = ["rm -rf*"]
```

### Where agents live

Agents are discovered from two directories, plus the config file:

| Scope | Location |
|-------|----------|
| Global | `<config dir>/codegg/agents/` |
| Project | `<project>/.codegg/agents/` |

`<config dir>` is `~/.config` on Linux and `~/Library/Application Support` on
macOS, so the global path is `~/.config/codegg/codegg/agents` and
`~/Library/Application Support/codegg/codegg/agents` respectively.

Resolution order, lowest priority first: built-ins, then global agent files,
then project agent files, then the config file's `agent` map. A later
definition with the same `name` overrides an earlier one, so a project file can
override a global file can override a built-in. This is what lets a repository
constrain an agent it did not author.

Run `/reload agents` in the TUI to pick up edits without restarting.

## Skills

A skill is a directory containing a `SKILL.md` file. The agent loads the body
only when the skill is relevant, so a skill is the right shape for a large,
infrequently-needed procedure.

### Format

```markdown
---
name: release-checklist
description: Steps for cutting and verifying a release.
license: MIT
compatibility: Requires a tagged git revision.
---

# Release Checklist

1. Confirm the working tree is clean.
2. Run the full verification suite.
3. Tag the revision and push the tag.
4. Watch the installer's version probe report the expected release.
```

Frontmatter fields:

| Field | Notes |
|-------|-------|
| `name` | The skill's identifier. Normalized on load |
| `description` | What the skill is for; this is what drives selection |
| `license` | Optional; preserved as metadata |
| `compatibility` | Optional; preserved as metadata |
| `metadata` | Optional map of arbitrary extra keys |
| `allowed-tools` | Optional, but see below — it grants nothing |

`allowed-tools` is preserved as metadata only. It does **not** grant the
listed tools any permission. Loading a skill never widens the permission
envelope, and a `allowed-tools` entry produces a diagnostic telling you so.
Permissions come from the permission configuration and the active agent's
permission table — see `docs/tools.md`.

### Where skills live

Project roots, all optional and all relative to the project:

```text
<project>/.codegg/skills
<project>/.agents/skills
<project>/.opencode/skills
<project>/.claude/skills
```

Global roots:

```text
<config dir>/<vendor>/skills
```

for vendors `codegg`, `agents`, `opencode`, and `claude` — so on Linux the
CodeGG one is `~/.config/codegg/codegg/skills`, and on macOS it is
`~/Library/Application Support/codegg/codegg/skills`.

A missing directory is skipped silently, so you only create the ones you use.
Discovery is bounded and containment-checked: a skill that resolves through a
symlink escaping its source root is rejected.

The four vendor layouts mean a skill directory written for another agent tool
usually works here unchanged.

Each skill is `<root>/<name>/SKILL.md`.

### Name fallback

For the portable form, both `name` and `description` are required and a skill
missing either is reported as an error.

The native compatibility form is more forgiving. When the frontmatter does not
carry both portable fields, CodeGG falls back to the native shape, where a
missing `name` is taken from the file stem and a missing `description`
defaults to empty. That is why a directory named `release-checklist` holding
`SKILL.md` can resolve to the name `SKILL` if its frontmatter omits `name`
entirely — give portable skills an explicit `name` and you avoid the ambiguity.

### Discovery bounds

Discovery is a startup scan with explicit caps: a maximum number of skills per
root, a maximum `SKILL.md` size, a maximum frontmatter size, and a recommended
maximum description length. Exceeding a size bound is an error; exceeding the
recommended description length is a warning. Tune these in the config's
`skills` block, which also accepts extra `paths` on top of the roots above.

## Working with skills

Skills are loaded by the **model**, not by a slash command. The `skill` tool
takes a skill `name` and returns the skill body plus a list of its resource
files. Point the agent at a skill by name — "load the `release-checklist`
skill" — and it will call the tool when the skill is relevant. Each activation
is recorded, which is what feeds `/skill-promote` below.

The slash commands in this area are about authoring:

- `/skill-promote <habit id>` — draft one skill proposal from a ready workflow
  habit.
- `/skill-proposals` — list proposals.
- `/skill-proposal <id>` — preview, publish, or reject a proposal. Publishing
  targets `project` or `global` scope.

Proposals are validated and previewed before installation and are never
installed automatically. Generated proposals cannot contain `allowed-tools`.

`/reload skills` (an alias of `/reload`) rescans discovery after you add or
edit files.

## Examples

`examples/agents/` contains runnable agent definitions worth copying:

| File | Shows |
|------|-------|
| `code-reviewer.toml` | A read-only reviewer with `ask` on `bash` and `task` |
| `custom-build-override.toml` | Overriding a built-in with the flat `extends` form |
| `docs-writer.toml` | A writer agent with write access and a rich deny list |
| `test-writer.toml` | A test writer with bash patterns for common runners |
| `markdown-agent.md` | A Markdown-defined agent |
| `README.md` | How the examples fit together |

Note that `docs-writer.toml` and `test-writer.toml` place their
`path_permission` and `bash_permission` sections inside `[agent]`, where the
loader does not read them — those sections take effect only at the top level.
Copy the flat-form layout shown above when you want the structured narrowing
to apply.

`examples/plugins/` shows the other extension point — process and WASM plugins
that can contribute their own tools and commands. See `docs/PLUGINS.md`.

## See also

- `architecture/agent.md` — agent loop, routing, built-in generation
- `architecture/skills.md` — discovery, parsing, promotion, bounds
- `docs/tools.md` — what an agent's permissions actually gate
- `docs/tui.md` — approval and sandbox selection
- `docs/security-semantics.md` — deterministic security gating
