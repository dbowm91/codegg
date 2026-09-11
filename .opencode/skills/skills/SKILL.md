---
name: skills
description: Skills module for specialized capabilities activated via /skill: commands
version: 2.2.0
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
| `source.rs` | `SourceKind` (10 variants), `SourceRoot`, `SourceSummary`, `AssetDiscoveryConfig` |
| `parser.rs` | Frontmatter/package parsing, SHA-256 digests, `validate_portable_document` shared proposal seam |
| `resource.rs` | `ResourceHandle`, `ResourceReadLimits`, bounded resource reads |
| `compat.rs` | `SkillIndexCompat` — backward-compatible bridge to the legacy `SkillIndex` API |
| `diagnostic.rs` | `Diagnostic`, `Severity` |
| `promotion.rs` | User-authorized proposal requests/store and publication provenance (no filesystem writes) |
| `publish.rs` | Host-only validated proposal publisher, atomic CodeGG-owned writes, reconciliation |

## Discovery Sources and Precedence

`SourceKind` defines ordered roots (lowest rank wins conflicts; shadowed alternatives are recorded, not hidden). There are 10 variants:

| Rank | Source |
|------|--------|
| 0 | `.codegg/skills/` (project) |
| 10 | `.agents/skills/` (project) |
| 20 | `.opencode/skills/` (project) |
| 30 | `.claude/skills/` (project) |
| 35 | `Plugin` contributions (project-native sources outrank) |
| 40 | CodeGG global (`<config>/codegg/skills/`) |
| 50 | Agents global (`~/.agents/skills/`) |
| 60 | OpenCode global (`~/.config/opencode/skills/`) |
| 70 | Claude global (`~/.claude/skills/`) |
| 80 | CodeGG native compat (direct `.md` files in `.codegg/skills/`) |

Discovery is bounded by `AssetDiscoveryConfig` (max file size 256 KiB, max frontmatter 64 KiB, max 256 skills per root, max 64 resources per skill, name/description length caps). Skill metadata such as `allowed-tools` never grants permissions.

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
    pub fn build(config: &AssetDiscoveryConfig, project_root: &Path, global_roots: &[PathBuf]) -> Self;
    pub fn build_with_plugin_sources(...) -> Self; // includes Plugin (rank 35) contributions
}
```

Built via `ProjectAssetSnapshotBuilder::build_skills` (`src/agent/asset_snapshot_builder.rs`), which layers plugin contributions over the filesystem registry, and surfaced to agents through the asset snapshot (`src/agent/asset_snapshot*.rs`). The daemon refreshes the immutable snapshot on session lifecycle and through `/reload`; refresh reports carry names, digests, counts, and diagnostics only. A failed refresh retains the previous valid generation while the published file stays on disk.

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

Portable skills are markdown `SKILL.md` packages with YAML frontmatter.
`name` and `description` are required; `license`, `compatibility`,
`metadata`, and `allowed-tools` are optional (`allowed-tools` is preserved
as metadata only, never expanded into permissions):

```markdown
---
name: my-skill
description: What this skill does
allowed-tools:
  - bash
  - read
---

# Skill body content
```

CodeGG-native `.codegg/skills/` additionally accepts legacy frontmatter
(`name`, `version`, `tags`) and direct `.md` files (directory name becomes
the skill name when `name` is absent). The parser auto-detects portable vs
native shape by checking for the portable required fields. Digests are
SHA-256 over frontmatter bytes + `\n` + LF-normalized body.

## Proposal and Publication Boundary

A proposal is not an effective skill. `validate_portable_document` is the
single frontmatter/body seam shared by filesystem discovery and proposal
submission: generated proposals accept one `SKILL.md` only, and
`allowed-tools`, unsupported frontmatter fields, and `scripts/` /
`resources/` / sidecar declarations are rejected. Proposal validation never
writes a skill root and never triggers a refresh.

Publication is host/TUI-only (`/skill-proposal publish <id> project|global`):
project writes go only to `<project>/.codegg/skills/<name>/SKILL.md`,
global writes only to `<config>/codegg/skills/<name>/SKILL.md`, via
same-directory temp file + atomic rename with per-root locking. The
model-facing proposal tool has no publication action and cannot approve.
On success the TUI invokes the daemon `/reload` path; active turns keep
their pinned snapshot, later turns observe the refresh.

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
| `src/tool/skill.rs` | The `skill` model tool; renders effective skills and their bounded resources |
| `src/agent/asset_snapshot_builder.rs` | `ProjectAssetSnapshotBuilder::build_skills` — filesystem registry + plugin sources |
| `src/core/daemon.rs` | Daemon-side registry construction and `/reload` refresh |
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
