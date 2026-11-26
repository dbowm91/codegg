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

Use `/agent <name>` to select an agent and `/agents` to inspect or manage agent definitions. The built-in choices are available in the TUI picker. Custom agent files can extend or replace built-ins.

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

Permission values are `allow`, `deny`, or `ask`. Other values are treated as
`ask`. To make an agent read-only, explicitly deny every write or execution
tool it must not use. Omitted tools inherit the ambient configuration. See
`docs/tools.md` for the permission surfaces.

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

The flat form also accepts structured permission sections: `[bash_permission]`
with `action`, `allow_patterns`, and `deny_patterns`, plus `[path_permission]`
with `allow` and `deny` glob lists. Both sections are read from the top level,
so they belong in the flat form:

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
macOS, so the global path is `~/.config/codegg/agents` and
`~/Library/Application Support/codegg/agents` respectively.

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
| `allowed-tools` | Optional metadata only; it grants no tool permission |

`allowed-tools` is preserved as metadata only. It does **not** grant the
listed tools any permission. Loading a skill never widens the permission
envelope, and an `allowed-tools` entry produces a diagnostic telling you so.
Permissions come from the config and the active agent's permission table. See
`docs/tools.md`.

CodeGG retains native `version` and `tags` fields as descriptive metadata.
Unknown fields such as `process` remain inert data; CodeGG does not run vendor
hooks, commands, or skill scripts.

### Where skills live

Project roots, all optional and all relative to the project:

```text
<project>/.codegg/skills
<project>/.agents/skills
<project>/.opencode/skills
<project>/.claude/skills
<project>/.cursor/skills
<project>/.gemini/skills
<project>/.github/skills
<project>/.pi/skills
<project>/.cline/skills
<project>/.clinerules/skills
<project>/.roo/skills
<project>/.factory/skills
```

Global roots:

```text
<config dir>/<vendor>/skills
```

Supported vendor roots include `codegg`, `agents`, `opencode`, `claude`,
`cline`, `pi`, `roo`, `copilot`, and `factory`. The CodeGG skill directory is
`~/.config/codegg/skills` on Linux and
`~/Library/Application Support/codegg/skills` on macOS. Use the config directory
as the root; CodeGG appends `<vendor>/skills` once. Do not add an extra
`codegg` directory. See `architecture/skills.md`.

Home discovery also checks `~/.agents/skills`, `~/.claude/skills`,
`~/.config/opencode/skills`, `~/.cline/skills`, `~/.pi/agent/skills`,
`~/.roo/skills`, `~/.copilot/skills`, and `~/.factory/skills`. These
directories are read-only inputs. Symlink aliases to the same physical root
are scanned once with deterministic source priority. Mode-specific Roo skill
roots and arbitrary nested project roots are excluded.

Project discovery also checks eligible ancestors of the selected project
directory through its nearest `.git` file or directory, up to eight levels.
The nearest project scope wins same-name conflicts; sibling directories and
paths above the Git boundary are not scanned. `/skills [name]` inspects the
active snapshot, and `/reload` affects later turns while running turns keep
their captured snapshot.

These layouts were checked against the vendors' current official skill
documentation: [Cursor](https://cursor.com/docs/skills),
[Cline](https://github.com/cline/cline/blob/main/docs/customization/skills.mdx),
[Pi](https://pi.dev/docs/latest/skills),
[Roo Code](https://roocodeinc.github.io/Roo-Code/advanced-usage/available-tools/skill/),
[Factory Droid](https://docs.factory.com/harness/skills),
[Gemini CLI](https://geminicli.com/docs/cli/using-agent-skills/), and
[GitHub Copilot](https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/customize-cloud-agent/add-skills).

You can add your own directories with the `skills.paths` config key. Each entry
is used as a skills directory **directly**, so `"paths": ["/opt/skills"]`
discovers `/opt/skills/<name>/SKILL.md` rather than `/opt/skills/codegg/skills`:

```jsonc
{ "skills": { "paths": ["/opt/skills"], "enabled": true } }
```

Set `"enabled": false` to turn skill discovery off entirely. Configured roots
rank below every built-in root, so one of them can never shadow a standard
skill name.

A missing directory is skipped silently, so you only create the ones you use.
Discovery is bounded and containment-checked: a skill that resolves through a
symlink escaping its source root is rejected.

The four vendor layouts mean a skill directory written for another agent tool
usually works here unchanged.

Each skill is `<root>/<name>/SKILL.md`.

### Name fallback

For the portable form, both `name` and `description` are required and a skill
missing either is reported as an error.

The native compatibility form is more forgiving, but only under the CodeGG
roots. When the frontmatter does not carry both portable fields there, CodeGG
falls back to the native shape, where a missing `name` is taken from the file
stem and a missing `description` defaults to empty. That is why a directory
named `release-checklist` holding `SKILL.md` can resolve to the name `SKILL` if
its frontmatter omits `name` entirely. Under the `.agents`, `.opencode`, and
`.claude` roots the portable path is the only one, so a skill missing `name` or
`description` is an error there. Give portable skills an explicit `name` and
`description` and you avoid the ambiguity either way.

### Discovery bounds

Discovery is a startup scan with explicit caps: a maximum number of skills per
root, a maximum `SKILL.md` size, a maximum frontmatter size, and a recommended
maximum description length. Exceeding a size bound is an error; exceeding the
recommended description length is a warning. The caps live in
`AssetDiscoveryConfig` (`src/skills/source.rs`) and are currently fixed at their
built-in defaults; they are not configurable from the config file.

## Working with skills

The model loads a skill with the `skill` tool by name. The tool returns the
skill body and its resource files. For example, ask the agent to load
`release-checklist`; the model will call the tool when the skill is relevant.
Activations are recorded for `/skill-promote`.

Use the read-only TUI `/skills [name]` command to inspect effective skill
source and shadow counts from the active asset snapshot; `/reload` refreshes
assets for subsequent turns.

The slash commands in this area are about authoring:

- `/skill-promote <habit id>`: draft a skill proposal from a ready workflow.
- `/skill-proposals`: list proposals.
- `/skill-proposal <id>`: preview, publish, or reject a proposal. Publishing
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

The `docs-writer.toml` and `test-writer.toml` examples place
`path_permission` and `bash_permission` inside `[agent]`, but the loader reads
those sections only at the top level. Copy the flat-form layout above when you
want these restrictions to apply.

`examples/plugins/` shows process and WASM plugins, another extension point
that can contribute tools and commands. See `docs/PLUGINS.md`.

## See also

- `architecture/agent.md`: agent loop, routing, and built-in generation
- `architecture/skills.md`: discovery, parsing, promotion, and bounds
- `docs/tools.md`: what agent permissions gate
- `docs/tui.md`: approval and sandbox selection
- `docs/security-semantics.md`: deterministic security gating

## Source verification

## Source verification

Checked 2026-11-23 against `src/agent/file_agents.rs`, `src/agent/registry.rs`, `src/skills/registry.rs`, `src/skills/source.rs`, `crates/codegg-config/src/schema.rs`, and `crates/codegg-tui/src/command.rs`. Confirmed agent overlays and permissions, skill discovery roots, configured skill paths, and the TUI authoring commands.
