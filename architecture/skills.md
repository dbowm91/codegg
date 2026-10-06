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
| Promotion | `src/skills/promotion.rs` — user-authorized proposal requests/store and publication provenance |
| Publication | `src/skills/publish.rs` — host-only validated proposal publisher, atomic CodeGG-owned writes, reconciliation |
| Resources | `src/skills/resource.rs` — `ResourceHandle`, `ResourceReadLimits`, bounded reads |
| Diagnostics | `src/skills/diagnostic.rs` — `Diagnostic`, `Severity` |
| Compat adapter | `src/skills/compat.rs` — `SkillIndexCompat` wrapping `AssetRegistry` |
| Tests | `tests/skills.rs`, `tests/skills_registry.rs` |

## How It Works

### Discovery pipeline

1. `AssetRegistry::build(config, project_root, global_roots)` resolves
   `SourceRoot` entries from the config, project root, and global roots.
2. For each root, `discover_in_root` (registry.rs:308) reads directory
   entries, validates symlink boundaries, and calls `parser::parse_candidate`
   (parser.rs:51) for each `SKILL.md` (or direct `.md` for CodeGG-native compat).
3. `resolve` (registry.rs:450) groups candidates by normalized name, sorts by
   precedence rank, selects the winner (first valid), records shadowed alternatives.
4. Returns `AssetRegistry { effective, diagnostics, sources }`.

### Precedence

Lower rank wins. Project-local always beats global.

| Rank | SourceKind | Location Pattern |
|------|-----------|-----------------|
| 0 | `CodeGGProject` | `<project>/.codegg/skills/<name>/SKILL.md` |
| 10 | `AgentsProject` | `<project>/.agents/skills/<name>/SKILL.md` |
| 20 | `OpenCodeProject` | `<project>/.opencode/skills/<name>/SKILL.md` |
| 30 | `ClaudeProject` | `<project>/.claude/skills/<name>/SKILL.md` |
| 35 | `Plugin` | Plugin contribution (project-native sources outrank) |
| 40 | `CodeGGGlobal` | `<config>/codegg/skills/<name>/SKILL.md` |
| 50 | `AgentsGlobal` | `<config>/agents/skills/<name>/SKILL.md` |
| 60 | `OpenCodeGlobal` | `<config>/opencode/skills/<name>/SKILL.md` |
| 70 | `ClaudeGlobal` | `<config>/claude/skills/<name>/SKILL.md` |
| 80 | `CodeGGNativeCompat` | `<project>/.codegg/skills/*.md` (direct markdown) |

`<config>` is the platform configuration directory (`dirs::config_dir()`, e.g.
`~/.config` on Linux and `~/Library/Application Support` on macOS). The
foreign-harness global roots are config-dir-relative, **not** `$HOME`-relative:
`AssetRegistry::build` takes each global root as the *parent* and appends
`<vendor>/skills` (`src/skills/registry.rs:250-303`), and callers pass
`default_global_discovery_root()` — exactly `dirs::config_dir()`
(`src/agent/asset_context.rs`) as that parent. The daemon does so at
`src/core/daemon_refresh.rs:187-191`.

Passing an already-joined path such as `<config>/codegg/skills` is a silent
failure: it resolves to `<config>/codegg/skills/codegg/skills`, which does not
exist, and `resolve_source_roots` skips a root whose path is not a directory
without emitting a diagnostic. The user-visible symptom is only that global
skills never appear. `already_joined_global_root_discovers_nothing` in
`registry.rs` pins the failure mode and `global_discovery_root_is_the_unjoined_config_dir`
in `src/agent/asset_context.rs` pins the accessor.

Every registry construction site builds its own `AssetRegistry`, so every one
of them must honor the parent-directory contract:

| Construction site | Purpose |
|---|---|
| `src/core/daemon_refresh.rs:187-191,375-379` | Daemon-side registry; owns the shared `AssetContext` global roots |
| `src/agent/asset_snapshot_builder.rs` | `ProjectAssetSnapshotBuilder::build_skills`; adds plugin sources |
| `src/tool/skill.rs:85-92` | The `skill` model tool; chains the context roots with the platform default |
| `src/tool/skill_proposal.rs:132-136` | Collision diagnostics for a submitted proposal |
| `src/tui/app/mod.rs:4482-4489,4641-4652` | `/skill-promote` listing and `/skill-proposal` live-collision view |
| `src/skills/compat.rs:32-44` | `SkillIndexCompat::load`, the legacy facade |

CodeGGProject also discovers direct `.md` files in `.codegg/skills/`,
treating them as `CodeGGNativeCompat` entries; the skill name falls back to
the markdown file's stem when the frontmatter omits `name`
(`src/skills/parser.rs:105-110`).

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

### Digest computation (parser.rs:377)

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

Methods: `build`, `build_with_plugin_sources`, `get`, `list`,
`find_matching`, `build_system_prompt`, `activate`, `resource_handle`.

### EffectiveSkill (candidate.rs:33)

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

Enum with 10 variants. Methods: `precedence_rank`, `is_project_local`,
`is_global`, `directory_name`, `is_foreign`.

### AssetDiscoveryConfig (source.rs:91)

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
derives global roots from `default_global_discovery_root()`
(`dirs::config_dir()`).

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
  that escape the source root (`validate_symlink_boundary`, registry.rs:419,
  called at `:354` and `:377`)
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

## Proposal and publication boundary (M002–M003)

A proposal is not an effective skill. `validate_portable_document` is the
single portable frontmatter/body seam shared by filesystem discovery
(`parse_candidate`) and `SkillPromotionStore::submit`; no temporary file
or package enumeration is used for proposals. Generated proposals accept
one `SKILL.md` only: required portable `name`/`description`, optional
`license`/safe `metadata`, Markdown body. `allowed-tools`, unsupported
frontmatter fields, and explicit `scripts/`/`resources/`/`package.json`
or `mcp:`/`plugin:` sidecar declarations are rejected; ordinary prose
that merely mentions plugins is not. Proposal creation, validation,
rejection, and preview never write a skill root and never invoke
`AssetRefreshCoordinator`, so the effective set and generation are
unchanged. Collision with an existing same-name skill is an advisory
warning with source provenance.

M003 adds the host/TUI-only `/skill-proposal publish <id> project|global`
operation. The model-facing proposal tool has no publication action and
cannot supply the approval request. The host derives the destination from
the closed scope enum: project publication writes only
`<project>/.codegg/skills/<normalized-name>/SKILL.md`, while global
publication writes only `<config>/codegg/skills/<normalized-name>/SKILL.md`.
The publisher revalidates the current proposal, digest, parser restrictions,
and root/package/destination symlink boundaries while holding a per-root
lock. Existing different content is rejected; new content is written to a
same-directory temporary file, synced, and atomically renamed. Proposal
publication provenance and the habit `Promoted` state are persisted only
after the rename. A crash between rename and metadata persistence can be
completed with `SkillPublicationService::reconcile`, which verifies the
destination digest and never rewrites the file.

Foreign effective skills require a preview at the same proposal revision;
publication records the shadowing source without writing any foreign root.
On success the TUI invokes the existing daemon-owned `/reload` path. That
path alone creates the next immutable runtime asset generation, so active
turns retain their pinned snapshot and subsequent turns observe a successful
refresh. A failed refresh retains the previous valid generation while the
published file remains on disk and the refresh diagnostic is shown.

## Testing

```bash
cargo test -p codegg skills               # legacy SkillIndex tests
cargo test --test skills_registry          # AssetRegistry integration tests
cargo test --test skill_publication        # approved publication safety/precedence tests
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
- `.opencode/skills/*/SKILL.md` — Canonical developer skill-guide location;
  `.skills` and `.agents/skills` are repository symlinks to it (harness
  compatibility). These guides are on-demand module references for agents,
  not user-installed skill packages; keep each guide aligned with its
  `architecture/` contract when the module changes.

## Source verification

Re-verified 2026-10-06 against all 10 files in `src/skills/`
(`mod.rs`, `registry.rs`, `source.rs`, `parser.rs`, `candidate.rs`,
`promotion.rs`, `publish.rs`, `resource.rs`, `diagnostic.rs`, `compat.rs`),
`src/agent/asset_context.rs`, and the three named test files. Corrected the
global-root contract: the doc recorded only the daemon's correct behavior,
while five other construction sites passed an already-joined
`<config>/codegg/skills` path. `AssetRegistry::build` appends
`<vendor>/skills` to each root, so those sites resolved to
`<config>/codegg/skills/codegg/skills`, which does not exist, and
`resolve_source_roots` skips a missing root without a diagnostic — global
skills were silently dropped everywhere except the daemon. `src/skills/compat.rs`
was subtly different: it took `.parent()` of the already-joined path, landing on
`<config>/codegg` and still double-joining. `default_global_skills_root()` is now
`default_global_discovery_root()` (exactly `dirs::config_dir()`), all six sites
pass the parent directory, and two regression tests pin both halves of the
contract. Added the construction-site table, since the daemon was previously
documented as if it were the only registry builder.

Corrected 4 items in the earlier pass: the digest ref
`parser.rs:308` → `:377` (`compute_digest`; the CRLF-normalization and
stability tests are at `:411` and `:420`), the symlink-containment ref
`registry.rs:328` → `validate_symlink_boundary` at `:419` (called from
`discover_in_root` at `:354` and `:377`; `:328` is only the per-root
truncation diagnostic), and the addition of the undocumented
`AssetRegistry::build_with_plugin_sources` (`registry.rs:26`) to the method
list. Added call-site refs for the discovery pipeline
(`discover_in_root` `:308`, `resolve` `:450`, `parse_candidate`
`parser.rs:51`).
Confirmed correct as written: every struct/enum line ref
(`AssetRegistry` `registry.rs:11`, `EffectiveSkill` `candidate.rs:33`,
`SourceKind` `source.rs:6`, `AssetDiscoveryConfig` `source.rs:91`,
`ResourceHandle` `resource.rs:44`, `ResourceReadLimits` `resource.rs:8`,
`SkillIndexCompat` `compat.rs:11`, `Diagnostic` `diagnostic.rs:22`), the
exact 10 `SourceKind` variants with their literal discriminants
`0/10/20/30/35/40/50/60/70/80`, the 12-field `EffectiveSkill` listing, the
5 `SourceKind` methods, the 7 `ResourceHandle` methods, the
`AssetDiscoveryConfig` defaults (256 KiB / 64 KiB / 256 / 64 / 128 / 2048,
`source.rs:115-120`), the `ResourceReadLimits` defaults (1 MiB / 64 KiB,
`resource.rs:16-17`), `resource.rs:89` for relative-path validation, and the
M002/M003 symbols (`validate_portable_document` `parser.rs:171`,
`SkillPromotionStore::submit` `promotion.rs:429`,
`SkillPublicationService` `publish.rs:71`, `reconcile` `publish.rs:133`).
