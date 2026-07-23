# Skills Module

The `skills` module provides specialized capabilities activated via `/skill:` commands.

## Overview

**Location**: `src/skills/`

**Key Responsibilities**:
- Source-aware asset discovery from CodeGG, `.agents`, OpenCode, and Claude-compatible harness locations
- Portable `SKILL.md` package parsing with YAML frontmatter
- Deterministic precedence and duplicate resolution
- Content digest computation for change detection
- Security-bounded discovery (symlink escape, path traversal, bounded sizes)
- Skill activation via `/skill:<name>` commands
- System prompt augmentation with skill content
- Backward-compatible `SkillIndex` facade for existing consumers

## Architecture

### Asset Registry

The `AssetRegistry` is the primary public type. It is constructed once at startup via `AssetRegistry::build(config, project_root, global_roots)` and produces an immutable, source-aware snapshot of all discovered skills.

```rust
pub struct AssetRegistry {
    pub effective: Vec<EffectiveSkill>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<SourceSummary>,
}
```

Key methods:
- `get(name)` — exact name lookup (case-insensitive normalized)
- `list()` — all effective skills
- `find_matching(query)` — partial match across name, description, metadata
- `build_system_prompt()` — formatted skill listing for agent prompts
- `activate(name)` — retrieve skill body by name
- `resource_handle(skill, relative_path, limits)` — create a lazy,
  containment-checked handle for an inventoried resource; the body is not
  read during discovery

### Source Kinds and Precedence

Each discovered skill is tagged with a `SourceKind` indicating its origin. Lower precedence rank wins:

| Rank | SourceKind | Location |
|------|-----------|----------|
| 0 | `CodeGGProject` | `<project>/.codegg/skills/<name>/SKILL.md` |
| 10 | `AgentsProject` | `<project>/.agents/skills/<name>/SKILL.md` |
| 20 | `OpenCodeProject` | `<project>/.opencode/skills/<name>/SKILL.md` |
| 30 | `ClaudeProject` | `<project>/.claude/skills/<name>/SKILL.md` |
| 40 | `CodeGGGlobal` | `<config>/codegg/skills/<name>/SKILL.md` |
| 50 | `AgentsGlobal` | `~/.agents/skills/<name>/SKILL.md` |
| 60 | `OpenCodeGlobal` | `~/.config/opencode/skills/<name>/SKILL.md` |
| 70 | `ClaudeGlobal` | `~/.claude/skills/<name>/SKILL.md` |
| 80 | `CodeGGNativeCompat` | `<project>/.codegg/skills/*.md` (direct markdown) |

Project-local sources always take precedence over global sources. The CodeGG-native direct markdown path has the lowest precedence.

### Portable Skill Schema

Skills are stored as markdown files with YAML frontmatter:

```markdown
---
name: my-skill
description: A portable skill
license: MIT
compatibility: ">=1.0"
metadata:
  author: someone
allowed-tools:
  - bash
  - read
---

# Skill body content
```

**Required fields**: `name`, `description`
**Optional fields**: `license`, `compatibility`, `metadata` (preserved as-is), `allowed-tools` (preserved as metadata only, never expanded into permissions)

### Native Compatibility

CodeGG-native skills in `.codegg/skills/` (legacy compatibility location) continue to accept the legacy frontmatter shape:

```markdown
---
name: my-skill
version: 1.0.0
tags: [vcs, git]
---
```

The parser auto-detects portable vs native frontmatter. Direct `.md` files in `.codegg/skills/` are treated as `CodeGGNativeCompat` entries.

### Digest Computation

Content digests are SHA-256 hashes computed over:
1. Canonical frontmatter (serialized YAML)
2. Body with CRLF→LF normalization

This ensures format-stable, content-stable digests across platforms.

### Diagnostics

Invalid skills produce `Diagnostic` entries (severity + reason + location) without aborting the registry. The registry always completes; diagnostics are available for inspection.

- `Severity::Error` — skill is invalid, skipped
- `Severity::Warning` — non-fatal issue (oversized description, allowed-tools metadata)
- `Severity::Info` — informational (shadowing notification)

### Refresh lifecycle

The daemon refreshes the immutable asset snapshot on session lifecycle and
through the native `/reload` command. The command and its focused aliases
share the `AssetRefresh` protocol request; they do not scan or mutate assets
in the TUI. Refresh reports are bounded to names, digests, counts, and
diagnostics. A failed or cancelled candidate leaves the previous generation
published, and a turn retains the `Arc` captured before prompt construction.

### Bounded resource handles

`ResourceHandle` (`src/skills/resource.rs`) is the only resource-body read
surface for discovered skill resources. Handles accept relative paths only,
canonicalize the package root and candidate at construction/read time, reject
symlink escape, and enforce both a maximum resource size and maximum returned
bytes. `read_text()` additionally rejects malformed UTF-8. Discovery records
resource names and sizes only; it never eagerly reads or executes resource
bodies.

### Security Bounds

- `AssetDiscoveryConfig` carries all configurable bounds with safe defaults:
  - `max_skill_file_size`: 256 KB
  - `max_frontmatter_size`: 64 KB
  - `max_skills_per_root`: 256
  - `max_resources_per_skill`: 64
  - `max_skill_name_length`: 128
  - `max_description_length`: 2048
  - `enabled_sources`: all source kinds enabled
- Symlink escape containment: canonicalize paths and reject candidates that escape the source root
- Resource path traversal: relative paths only, no `..` components
- Script files are inventoried (name + size) but never executed
- Resource bodies are lazy and bounded by `ResourceReadLimits` (1 MiB file
  size and 64 KiB read defaults)

### Compatibility Adapter

`SkillIndexCompat` wraps `AssetRegistry` behind the legacy `SkillIndex` API:

```rust
pub struct SkillIndexCompat {
    registry: Arc<AssetRegistry>,
}
```

This preserves the existing `load(&str)` → `get()` → `activate()` flow used by `src/main.rs` and `src/tool/skill.rs`. The adapter uses `std::env::current_dir()` fallback for the `load` method (grandfathered), while the new `AssetRegistry::build` API requires explicit roots.

## Key Types

### Skill (legacy, preserved for backward compatibility)

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

### SkillIndex (legacy, preserved for backward compatibility)

```rust
pub struct SkillIndex {
    skills: Vec<Skill>,
}
```

Methods: `new()`, `load(&str)`, `get(&str)`, `list()`, `find_matching(&str)`, `build_system_prompt()`, `activate(&str)`.

## File Layout

```
src/skills/
  mod.rs           — pub re-exports; legacy Skill + SkillIndex
  registry.rs      — AssetRegistry, build logic, resolution
  source.rs        — SourceKind, SourceRoot, SourceSummary, AssetDiscoveryConfig
  parser.rs        — frontmatter parsing, candidate construction, digest computation
  candidate.rs     — SkillCandidate, EffectiveSkill, ResolvedRegistry, ResourceDescriptor
  resource.rs      — ResourceHandle, ResourceReadLimits, bounded resource reads
  diagnostic.rs    — Diagnostic, Severity
  compat.rs        — SkillIndexCompat adapter
```

## Loading Locations

### Project-local
- `.opencode/skills/<name>/SKILL.md` (canonical portable package)
- `.codegg/skills/<name>/SKILL.md` (portable package, legacy)
- `.codegg/skills/*.md` (native compat direct markdown, legacy)
- `.agents/skills/<name>/SKILL.md` (symlink to `.opencode/skills/`)
- `.claude/skills/<name>/SKILL.md`

### Global
- `<config>/codegg/skills/<name>/SKILL.md`
- `~/.agents/skills/<name>/SKILL.md`
- `~/.config/opencode/skills/<name>/SKILL.md`
- `~/.claude/skills/<name>/SKILL.md`

### Absent directories are harmless

Missing global or foreign directories produce no errors.

## Activation

User activates skill via `/skill:` command:

```
/skill:git
```

The `SkillTool` (`src/tool/skill.rs`) handles runtime skill loading.

## Usage in Agent

```rust
// In main.rs - load at startup using compat adapter
let mut skills = SkillIndex::new();
skills.load(&project_dir).await?;

// Activate from CLI flag
if let Some(skill_body) = skills.activate(skill_name) {
    app.prompt_state.prompt.set_text(skill_body);
}
```

For new code, prefer the primary `AssetRegistry` API:

```rust
let config = AssetDiscoveryConfig::default();
let registry = AssetRegistry::build(&config, &project_root, &global_roots);
if let Some(skill_body) = registry.activate("my-skill") {
    // ...
}
```

## See Also

- [tool.md](tool.md) - `/skill:` tool
- `src/skills/` - Runtime loader implementation
- `tests/skills.rs` - Legacy SkillIndex tests
- `tests/skills_registry.rs` - AssetRegistry integration tests
