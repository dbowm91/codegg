# Skills Module

Source-aware skill discovery, portable SKILL.md package parsing,
precedence resolution, and bounded resource access for on-demand
skill activation via `/skill:<name>` commands.

## Purpose

Discovers skill packages from multiple harness-compatible locations
(CodeGG, .agents, OpenCode, Claude), resolves name conflicts by
precedence, computes content digests for change detection, and
provides lazy, security-bounded resource access for skill assets.

## Where It Lives

| Layer | Path |
|-------|------|
| Module root | `src/skills/mod.rs` — re-exports + legacy `Skill`/`SkillIndex` |
| Registry | `src/skills/registry.rs` — `AssetRegistry::build`, resolution |
| Sources | `src/skills/source.rs` — `SourceKind`, `SourceRoot`, `AssetDiscoveryConfig` |
| Parser | `src/skills/parser.rs` — frontmatter parsing, digest, resource inventory, `validate_portable_document` in-memory seam |
| Candidates | `src/skills/candidate.rs` — `SkillCandidate`, `EffectiveSkill`, `ResolvedRegistry` |
| Promotion | `src/skills/promotion.rs` — user-authorized proposal requests/store, no publisher, no skill-root writes |
| Resources | `src/skills/resource.rs` — `ResourceHandle`, `ResourceReadLimits`, bounded reads |
| Diagnostics | `src/skills/diagnostic.rs` — `Diagnostic`, `Severity` |
| Compat adapter | `src/skills/compat.rs` — `SkillIndexCompat` wrapping `AssetRegistry` |
| Tests | `tests/skills.rs`, `tests/skills_registry.rs` |

## How It Works

### Discovery pipeline

1. `AssetRegistry::build(config, project_root, global_roots)` resolves
   `SourceRoot` entries from the config, project root, and global roots.
2. For each root, `discover_in_root` reads directory entries, validates
   symlink boundaries, and calls `parser::parse_candidate` for each
   `SKILL.md` (or direct `.md` for CodeGG-native compat).
3. `resolve` groups candidates by normalized name, sorts by precedence
   rank, selects the winner (first valid), records shadowed alternatives.
4. Returns `AssetRegistry { effective, diagnostics, sources }`.

### Precedence

Lower rank wins. Project-local always beats global.

| Rank | SourceKind | Location Pattern |
|------|-----------|-----------------|
| 0 | `CodeGGProject` | `<project>/.codegg/skills/<name>/SKILL.md` |
| 10 | `AgentsProject` | `<project>/.agents/skills/<name>/SKILL.md` |
| 20 | `OpenCodeProject` | `<project>/.opencode/skills/<name>/SKILL.md` |
| 30 | `ClaudeProject` | `<project>/.claude/skills/<name>/SKILL.md` |
| 40 | `CodeGGGlobal` | `<config>/codegg/skills/<name>/SKILL.md` |
| 50 | `AgentsGlobal` | `~/.agents/skills/<name>/SKILL.md` |
| 60 | `OpenCodeGlobal` | `~/.config/opencode/skills/<name>/SKILL.md` |
| 70 | `ClaudeGlobal` | `~/.claude/skills/<name>/SKILL.md` |
| 80 | `CodeGGNativeCompat` | `<project>/.codegg/skills/*.md` (direct markdown) |

CodeGGProject also discovers direct `.md` files in `.codegg/skills/`,
treating them as `CodeGGNativeCompat` entries.

### Portable SKILL.md schema

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

**Required**: `name`, `description`.
**Optional**: `license`, `compatibility`, `metadata`, `allowed-tools`
(preserved as metadata only, never expanded into permissions).

### Native compat

CodeGG-native `.codegg/skills/` also accepts legacy frontmatter
(`name`, `version`, `tags`). The parser auto-detects portable vs
native shape by checking for portable required fields.

### Digest computation (parser.rs:308)

SHA-256 over: frontmatter bytes + `\n` + body with CRLF→LF
normalization. Format-stable across platforms.

### Resource access (resource.rs)

`ResourceHandle` provides lazy, bounded reads of files inside a
skill package:

- Accepts relative paths only (no `..`, no absolute, no backslash)
- Canonicalizes at construction AND read time
- Rejects symlink escape (canonical must stay under package root)
- Enforces `max_resource_size` (default 1 MiB) and
  `max_bytes_returned` (default 64 KiB)
- `read_text()` additionally rejects malformed UTF-8

## Key Types & APIs

### AssetRegistry (registry.rs:11)

```rust
pub struct AssetRegistry {
    pub effective: Vec<EffectiveSkill>,
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Vec<SourceSummary>,
}
```

Methods: `build`, `get`, `list`, `find_matching`, `build_system_prompt`,
`activate`, `resource_handle`.

### EffectiveSkill (candidate.rs:32)

```rust
pub struct EffectiveSkill {
    pub name: String,
    pub normalized_name: String,
    pub description: String,
    pub source_kind: SourceKind,
    pub source_path: PathBuf,
    pub package_root: PathBuf,
    pub content_digest: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub resources: Vec<ResourceDescriptor>,
    pub body: String,
    pub precedence_rank: u32,
    pub shadowed_alternatives: Vec<ShadowedAlternative>,
}
```

### SourceKind (source.rs:6)

Enum with 9 variants. Methods: `precedence_rank`, `is_project_local`,
`is_global`, `directory_name`, `is_foreign`.

### AssetDiscoveryConfig (source.rs:84)

```rust
pub struct AssetDiscoveryConfig {
    pub max_skill_file_size: u64,          // 256 KiB
    pub max_frontmatter_size: usize,       // 64 KiB
    pub max_skills_per_root: usize,        // 256
    pub max_resources_per_skill: usize,    // 64
    pub max_skill_name_length: usize,      // 128
    pub max_description_length: usize,     // 2048
    pub enabled_sources: HashSet<SourceKind>,
}
```

### ResourceHandle (resource.rs:44)

```rust
pub struct ResourceHandle {
    package_root: PathBuf,
    relative_path: PathBuf,
    limits: ResourceReadLimits,
}
```

Methods: `new`, `validate_relative_path`, `read_bytes`, `read_text`,
`package_root`, `relative_path`, `limits`.

### ResourceReadLimits (resource.rs:8)

```rust
pub struct ResourceReadLimits {
    pub max_resource_size: u64,      // default 1 MiB
    pub max_bytes_returned: usize,   // default 64 KiB
}
```

### SkillIndexCompat (compat.rs:11)

Wraps `Arc<AssetRegistry>` behind the legacy `SkillIndex` API.
Used by `src/main.rs` and `src/tool/skill.rs`. The `load` method
derives global roots from `dirs::config_dir()`.

### Legacy types (mod.rs)

`Skill` (name, description, version, tags, body, source) and
`SkillIndex` (legacy facade) are preserved for backward compatibility.

### Diagnostic (diagnostic.rs:22)

```rust
pub struct Diagnostic {
    pub severity: Severity,  // Error | Warning | Info
    pub reason: String,
    pub location: Option<String>,
}
```

## Security Bounds

- Symlink escape containment: canonicalize paths, reject candidates
  that escape the source root (registry.rs:328)
- Resource path traversal: relative paths only, no `..` (resource.rs:89)
- Script files inventoried (name + size) but never executed
- Resource bodies lazy and bounded by `ResourceReadLimits`
- `max_skills_per_root` cap prevents pathological directories
- Invalid skills produce `Diagnostic` entries without aborting the
  registry

## Refresh lifecycle

The daemon refreshes the immutable asset snapshot on session lifecycle
and through the native `/reload` command. Refresh reports are bounded
to names, digests, counts, and diagnostics. A failed candidate leaves
the previous generation published.

## Proposal boundary (M002, pre-publication)

A proposal is not an effective skill. `validate_portable_document` is the
single portable frontmatter/body seam shared by filesystem discovery
(`parse_candidate`) and `SkillPromotionStore::submit`; no temporary file
or package enumeration is used for proposals. Generated proposals accept
one `SKILL.md` only: required portable `name`/`description`, optional
`license`/safe `metadata`, Markdown body. `allowed-tools`, unsupported
frontmatter fields, and explicit `scripts/`/`resources/`/`package.json`
or `mcp:`/`plugin:` sidecar declarations are rejected; ordinary prose
that merely mentions plugins is not. The publisher does not exist yet:
proposal creation, validation, rejection, and preview never write a skill
root and never invoke `AssetRefreshCoordinator`, so the effective set and
generation are unchanged. Collision with an existing same-name skill is
an advisory warning with source provenance. Explicit publication into
CodeGG-owned roots is reserved for M003.

## Testing

```bash
cargo test -p codegg skills               # legacy SkillIndex tests
cargo test --test skills_registry          # AssetRegistry integration tests
```

`tests/skills_registry.rs` covers: empty project, all 4 project source
kinds, global skills, precedence (project over global), native compat
direct .md, invalid fallback, disabled sources, get/list/find_matching,
build_system_prompt, activate, symlink escape rejection, oversized
frontmatter, malformed YAML, resource inventory, allowed-tools metadata
warning, digest stability, digest CRLF normalization, name validation.

## Related Docs

- [tool.md](tool.md) — `/skill:` tool
- `src/skills/` — Runtime implementation
- `.opencode/skills/*/SKILL.md` — Skill package location
