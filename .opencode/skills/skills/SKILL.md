---
name: skills
description: Skills module for specialized capabilities activated through the skill tool
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
- Source-aware skill discovery from CodeGG, Agent Skills, OpenCode, Claude, Cursor, Gemini CLI, and GitHub Copilot locations
- Portable `SKILL.md` package parsing with YAML frontmatter
- Deterministic precedence and duplicate/shadow resolution
- Content digests for change detection
- Security-bounded discovery (symlink escape, path traversal, bounded sizes)
- Skill activation via the `skill` model tool with a `name` argument
- System prompt augmentation with skill content

This repository also keeps agent-facing maintenance copies of its own skill docs in `.opencode/skills/` (`.skills` and `.agents/skills` are symlinks to it). Keep those aligned with runtime behavior documented here.

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Legacy `Skill`, `SkillIndex` facade + re-exports |
| `registry.rs` | `AssetRegistry` — primary public type; builds an immutable source-aware snapshot (`effective`, `diagnostics`, `sources`) |
| `candidate.rs` | `SkillCandidate`, `EffectiveSkill`, `ResourceDescriptor`, `ShadowedAlternative` |
| `source.rs` | `SourceKind` (19 variants), `SourceRoot`, `SourceSummary`, `AssetDiscoveryConfig` |
| `parser.rs` | Frontmatter/package parsing, SHA-256 digests, `validate_portable_document` shared proposal seam |
| `resource.rs` | `ResourceHandle`, `ResourceReadLimits`, bounded resource reads |
| `compat.rs` | `SkillIndexCompat` — backward-compatible bridge to the legacy `SkillIndex` API |
| `diagnostic.rs` | `Diagnostic`, `Severity` |
| `promotion.rs` | User-authorized proposal requests/store and publication provenance (no filesystem writes) |
| `publish.rs` | Host-only validated proposal publisher, atomic CodeGG-owned writes, reconciliation |

## Discovery Sources and Precedence

`SourceKind` defines ordered roots (lowest rank wins conflicts; shadowed alternatives are recorded, not hidden). It covers project and global locations, native CodeGG compatibility, plugins, and configured paths:

| Rank | Source |
|------|--------|
| 0 | `.codegg/skills/` (project) |
| 10 | `.agents/skills/` (project) |
| 20 | `.opencode/skills/` (project) |
| 30 | `.claude/skills/` (project) |
| 31 | `.cursor/skills/` (project) |
| 32 | `.gemini/skills/` (project) |
| 33 | `.github/skills/` (project) |
| 34 | `.codex/skills/` (project) |
| 35 | `Plugin` contributions (project-native sources outrank) |
| 40 | CodeGG global (`<config>/codegg/skills/`) |
| 50 | Agents global (`<config>/agents/skills/`) |
| 60 | OpenCode global (`<config>/opencode/skills/`) |
| 70 | Claude global (`<config>/claude/skills/`) |
| 71 | Cursor global (`~/.cursor/skills/`) |
| 72 | Gemini global (`~/.gemini/skills/`) |
| 73 | Codex global (`~/.codex/skills/`) |
| 74 | Copilot global (`~/.copilot/skills/`) |
| 80 | CodeGG native compat (direct `.md` files in `.codegg/skills/`) |
| 90 | `Configured` — each `skills.paths` entry, used as a skills directory directly |

Discovery is bounded by `AssetDiscoveryConfig` (max file size 256 KiB, max frontmatter 64 KiB, max 256 skills per root, max 64 resources per skill, name/description length caps). Skill metadata such as `allowed-tools` never grants permissions.

### Global roots are parent directories

`resolve_source_roots` appends only known vendor paths to each supplied global
parent. `default_global_discovery_roots()` (`src/agent/asset_context.rs`)
returns the platform config and home directories. Discovery does not recurse
through either directory.

Passing an already-joined path silently discovers nothing: the root resolves to
`<config>/codegg/skills/codegg/skills`, which does not exist, and
`resolve_source_roots` skips a root whose path is not a directory without
emitting a diagnostic. There is no user-visible symptom other than global
skills simply never appearing. `already_joined_global_root_discovers_nothing`
in `registry.rs` pins this failure mode.

## Configured roots (`skills.paths`)

`skills.paths` adds extra skills directories, and each entry is a skills
directory **itself** — `resolve_source_roots` appends nothing, so `"/opt/skills"`
discovers `/opt/skills/<name>/SKILL.md` rather than `/opt/skills/codegg/skills`.
This differs deliberately from the global-root parent contract above.
Non-directories are skipped. `Configured` ranks 90, below every built-in root,
so a configured path can never shadow a well-known root's skill name.

`skills.enabled = false` clears `enabled_sources`, disabling discovery on every
path.

Both keys flow through `asset_discovery_config_from`
(`src/agent/asset_context.rs`), which every `AssetRegistry` construction site
calls; `ProjectAssetSnapshotBuilder::new` takes only `Arc<Config>` and derives
the discovery config itself, so a key cannot be honoured in one construction
path and ignored in another. Previously `SkillsConfig.paths` and `.enabled`
were parsed and merged but read by no production path. The
`SkillsConfig.urls` key was removed rather than left accepted-but-inert,
because remote skill fetching is unimplemented.

Pinned by `configured_root_is_used_as_a_skills_directory`,
`configured_root_is_not_treated_as_a_global_parent`,
`disabled_sources_suppress_project_skills`, and
`nonexistent_configured_root_is_skipped` in `registry.rs`.

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
(`name`, `version`, `tags`) and direct `.md` files (the skill file's stem
becomes the skill name when `name` is absent). The parser auto-detects
portable vs native shape by checking for the portable required fields.
Digests are SHA-256 over frontmatter bytes + `\n` + LF-normalized body.

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
| `src/tool/skill_proposal.rs` | Collision diagnostics for a submitted proposal |
| `src/tui/app/mod.rs` | `/skill-promote` habit listing and `/skill-proposal` live-collision view |
| `src/skills/compat.rs` | `SkillIndexCompat::load` — the legacy facade's own registry build |
| `src/agent/asset_snapshot_builder.rs` | `ProjectAssetSnapshotBuilder::build_skills` — filesystem registry + plugin sources |
| `src/core/daemon_refresh.rs` | Daemon-side registry construction and `/reload` refresh |
| `src/agent/prompt.rs` | `assemble_system_prompt_with_profile(ctx: PromptContext)` — skill names reach the prompt through the `PromptContext` profile |

Every one of these constructs its own `AssetRegistry`, so every one of them must
honor the parent-directory contract above.

## Skills vs System Prompts

- **Skills**: Loaded on-demand via the `skill` tool with `{"name":"<skill-name>"}`; contain specialized instructions
- **System Prompts**: Agent-level instructions baked into `Agent.system_prompt`
- **Instructions**: Global instructions from `config.instructions` applied to all agents

## Adding New Skills

1. Create a directory with `SKILL.md` under one of the discovery roots (project `.codegg/skills/<name>/SKILL.md` is canonical)
2. Use YAML frontmatter with `name`, `description`, and optional `version` and `tags`
3. Add skill body content after frontmatter

See `architecture/skills.md` for the authoritative module contract.

## Source verification

Current source contract is summarized in `architecture/skills.md`. Discovery
uses a finite vendor path table and explicit config/home roots; portable
validation, Claude directory-name fallback, bounded nested inventory, and
workspace scoping are covered by focused registry/parser tests. Resource
scripts and vendor metadata remain inert. The TUI `/skills` report reads the
same registry and omits physical source paths.
