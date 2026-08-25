---
name: skills
description: Skills module for specialized capabilities activated via /skill: commands
version: 2.1.0
tags:
  - skills
  - asset-registry
  - loading
  - activation
---

# Skills Module Guide

This skill covers the skills system in codegg for discovering, loading, and activating specialized capabilities.

## Overview

The `skills` module (`src/skills/`) provides:
- Source-aware skill discovery from CodeGG, `.agents`, OpenCode, and Claude-compatible harness locations (project and global)
- Portable `SKILL.md` package parsing with YAML frontmatter
- Deterministic precedence and duplicate/shadow resolution
- Content digests for change detection
- Security-bounded discovery (symlink escape, path traversal, bounded sizes)
- Skill activation via `/skill:<name>` commands and the `skill` model tool
- System prompt augmentation with skill content

This repository also keeps agent-facing maintenance copies of its own skill docs in `.opencode/skills/` (`.skills` and `.agents/skills` are symlinks to it). Keep those aligned with runtime behavior documented here.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Legacy `Skill`, `SkillIndex` facade + re-exports |
| `registry.rs` | `AssetRegistry` — primary public type; builds an immutable source-aware snapshot (`effective`, `diagnostics`, `sources`) |
| `candidate.rs` | `SkillCandidate`, `EffectiveSkill`, `ResourceDescriptor`, `ShadowedAlternative` |
| `source.rs` | `SourceKind`, `SourceRoot`, `SourceSummary`, `AssetDiscoveryConfig` |
| `parser.rs` | Frontmatter/package parsing |
| `resource.rs` | `ResourceHandle`, `ResourceReadLimits`, bounded resource reads |
| `compat.rs` | `SkillIndexCompat` — backward-compatible bridge to the legacy `SkillIndex` API |
| `diagnostic.rs` | `Diagnostic`, `Severity` |

## Discovery Sources and Precedence

`SourceKind` defines ordered roots (lowest rank wins conflicts; shadowed alternatives are recorded, not hidden):

| Rank | Source |
|------|--------|
| 0 | `.codegg/skills/` (project) |
| 10 | `.agents/skills/` (project) |
| 20 | `.opencode/skills/` (project) |
| 30 | `.claude/skills/` (project) |
| 40–70 | Same four roots under the global config directory |
| 80 | CodeGG native compat location |

Discovery is bounded by `AssetDiscoveryConfig` (max file size, max frontmatter size, max skills per root, max resources per skill, name/description length caps). Skill metadata such as `allowed-tools` never grants permissions.

## Key Types

### Skill (legacy facade)

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,
}
```

### AssetRegistry (primary)

```rust
pub struct AssetRegistry {
    pub effective: Vec<EffectiveSkill>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<SourceSummary>,
}

impl AssetRegistry {
    pub fn build(config: AssetDiscoveryConfig, project_root: &Path, global_roots: &[PathBuf]) -> Self;
}
```

Constructed once at startup in `src/tool/skill.rs` (`AssetRegistry::build(&asset_config, workspace_root, &global_roots)`) and surfaced to agents through the asset snapshot (`src/agent/asset_snapshot*.rs`).

### SkillIndex (legacy facade)

```rust
impl SkillIndex {
    pub fn new() -> Self;
    pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError>; // global ~/.config/codegg/skills + project .codegg/skills
    pub fn get(&self, name: &str) -> Option<&Skill>;
    pub fn list(&self) -> &[Skill];
    pub fn find_matching(&self, query: &str) -> Vec<&Skill>;
    pub fn build_system_prompt(&self) -> String;
    pub fn activate(&self, name: &str) -> Option<String>;
}
```

`SkillIndexCompat` adapts registry output for existing consumers. New code should prefer `AssetRegistry`.

## Skill File Format

Skills are markdown files with YAML frontmatter:

```markdown
---
name: git
description: Advanced git operations
version: 1.0.0
tags: [vcs, git]
---

# Git Skill
...
```

Loading accepts direct `.md` files and directories containing `SKILL.md` (directory name becomes the skill name when `name` is absent).

## Runtime Activation

`SkillTool` (`src/tool/skill.rs`) provides runtime skill loading:

```rust
// Execute with {"name": "<skill>"}
let result = skill_tool.execute(json!({"name": "git"})).await;
// Returns JSON with name, description, body, and resources
```

Resource enumeration happens inside `render_skill()`: it iterates
`skill.resources` (excluding `SKILL.md`) and returns resource names alongside
the rendered body. There is no standalone `list_skill_resources()` function.

## Integration Points

| Location | Usage |
|----------|-------|
| `src/tool/skill.rs` | Builds `AssetRegistry` at startup; provides the `skill` tool |
| `src/agent/asset_snapshot_builder.rs` / `asset_snapshot.rs` | Surface effective skills to agents |
| `src/core/daemon.rs` | Daemon-side registry construction |
| `src/agent/prompt.rs` | `assemble_system_prompt_with_profile(ctx: PromptContext)` — skill names reach the prompt through the `PromptContext` profile |

## Skills vs System Prompts

- **Skills**: Loaded on-demand via `/skill:` command or `skill` tool; contain specialized instructions
- **System Prompts**: Agent-level instructions baked into `Agent.system_prompt`
- **Instructions**: Global instructions from `config.instructions` applied to all agents

## Adding New Skills

1. Create a directory with `SKILL.md` under one of the discovery roots (project `.codegg/skills/<name>/SKILL.md` is canonical)
2. Use YAML frontmatter with `name`, `description`, and optional `version` and `tags`
3. Add skill body content after frontmatter

See `architecture/skills.md` for the authoritative module contract.
