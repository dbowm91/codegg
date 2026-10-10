# Skills Module

Source-aware skill discovery, portable SKILL.md package parsing,
precedence resolution, and bounded resource access for on-demand
skill activation through the `skill` model tool.

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
| 31 | `CursorProject` | `<project>/.cursor/skills/<name>/SKILL.md` |
| 32 | `GeminiProject` | `<project>/.gemini/skills/<name>/SKILL.md` |
| 33 | `CopilotProject` | `<project>/.github/skills/<name>/SKILL.md` |
| 34 | `CodexProject` | `<project>/.codex/skills/<name>/SKILL.md` |
| 35 | `Plugin` | Plugin contribution (project-native sources outrank) |
| 40 | `CodeGGGlobal` | `<config>/codegg/skills/<name>/SKILL.md` |
| 50 | `AgentsGlobal` | `<config>/agents/skills/<name>/SKILL.md` or `~/.agents/skills` |
| 60 | `OpenCodeGlobal` | `<config>/opencode/skills/<name>/SKILL.md` or `~/.config/opencode/skills` |
| 70 | `ClaudeGlobal` | `<config>/claude/skills/<name>/SKILL.md` or `~/.claude/skills` |
| 71 | `CursorGlobal` | `~/.cursor/skills/<name>/SKILL.md` |
| 72 | `GeminiGlobal` | `~/.gemini/skills/<name>/SKILL.md` |
| 73 | `CodexGlobal` | `~/.codex/skills/<name>/SKILL.md` |
| 74 | `CopilotGlobal` | `~/.copilot/skills/<name>/SKILL.md` |
| 80 | `CodeGGNativeCompat` | `<project>/.codegg/skills/*.md` (direct markdown) |
| 90 | `Configured` | Each entry of `skills.paths`, used as a skills directory directly |

Cursor and Gemini also recognize their documented home directories (`~/.cursor/skills`
and `~/.gemini/skills`). Codex and Copilot recognize `~/.codex/skills` and
`~/.copilot/skills`. Cline, Pi, Roo, Factory, Windsurf,
and Devin layouts remain unsupported until their skill package paths and
scope are confirmed against maintained vendor documentation.

`<config>` is the platform configuration directory (`dirs::config_dir()`, e.g.
`~/.config` on Linux and `~/Library/Application Support` on macOS). The finite
global candidate table includes config-relative vendor directories and
documented home locations; callers pass only the config and home directories
from `default_global_discovery_roots()`. The resolver never scans either
directory recursively.

### Configured roots (`skills.paths`)

`skills.paths` adds extra skills directories. Unlike the global roots above,
each entry is a skills directory **itself** — `resolve_source_roots` appends
nothing — so `"/opt/skills"` discovers `/opt/skills/<name>/SKILL.md` and is not
read as a parent expecting `/opt/skills/codegg/skills`. Entries are canonicalized
through the same bounds as every other root, and a non-directory is skipped.
`Configured` ranks 90, below every built-in root, so a shared or
machine-wide path can never shadow a well-known root's skill name; a collision
is recorded as a shadowed alternative diagnostic instead.

`skills.enabled = false` clears `enabled_sources` entirely, disabling discovery
on every path.

Both keys are applied through `asset_discovery_config_from`
(`src/agent/asset_context.rs`), which every `AssetRegistry` construction site
calls. `ProjectAssetSnapshotBuilder::new` takes only `Arc<Config>` and derives
the discovery config itself rather than accepting an injected one, so a config
key cannot be honoured in one construction path and ignored in another. This
mirrors the earlier defect where `SkillsConfig.paths` and `.enabled` were parsed
and merged but read by no production code path.

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
Portable names use 1–64 lowercase ASCII letters and digits with single hyphens
as separators. Claude packages may omit `name`; their directory name is used.
Unknown vendor fields are retained as inert metadata under the frontmatter size
bound.

### Native compat

CodeGG-native `.codegg/skills/` also accepts legacy frontmatter
(`name`, `version`, `tags`). The parser auto-detects portable vs
native shape by checking for portable required fields.

### Digest computation (parser.rs:377)

SHA-256 over: frontmatter bytes + `\n` + body with CRLF→LF
normalization. Format-stable across platforms.

### Resource access (resource.rs)

`ResourceHandle` provides lazy, bounded reads of inventoried files inside a
skill package. Inventory includes nested package files to fixed depth and
entry limits; symlink entries are not followed.

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

Enum with 19 variants. Methods: `precedence_rank`, `is_project_local`,
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

`tests/skills_registry.rs` covers: empty project, all supported project source
kinds, global skills, precedence (project over global), native compat
direct .md, invalid fallback, disabled sources, get/list/find_matching,
build_system_prompt, activate, symlink escape rejection, oversized
frontmatter, malformed YAML, resource inventory, allowed-tools metadata
warning, digest stability, digest CRLF normalization, name validation.

## Related Docs

- [tool.md](tool.md) — model-facing `skill` tool
- `src/skills/` — Runtime implementation
- `.opencode/skills/*/SKILL.md` — Canonical developer skill-guide location;
  `.skills` and `.agents/skills` are repository symlinks to it (harness
  compatibility). These guides are on-demand module references for agents,
  not user-installed skill packages; keep each guide aligned with its
  `architecture/` contract when the module changes.

## Source verification

Verified against the current `src/skills/` implementation, asset snapshot
builder, all registry construction sites, and `tests/skills_registry.rs`.
Project discovery supports CodeGG, Agent Skills, OpenCode, Claude, Cursor,
Gemini CLI, and GitHub Copilot project roots. Global candidates are limited to
platform config/home roots and a fixed vendor path table; canonical roots are
deduplicated before parsing. `AssetRegistry::build_for_workspace_scope` and
the snapshot builder walk at most 16 explicit ancestors, stopping at the
nearest `.git` boundary. Claude alone may derive a missing portable `name`
from its package directory. Portable names are validated separately from the
CodeGG native compatibility parser. Resource inventory is recursive but
bounded by depth, entry, and per-skill count limits, and does not follow
symlinks. `/skills` is a registered read-only TUI report and redacts physical
source paths. The actual model activation surface remains the `skill` tool.
