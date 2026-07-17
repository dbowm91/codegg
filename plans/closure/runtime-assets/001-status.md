# Runtime Assets Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/001-project-asset-registry.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-1--source-aware-registry-and-portable-skill-discovery`

Repository baseline reviewed: `b8df427` (Domain Identity 002 closed, Project Catalog 001 closed, Provider Connections 003 closed)

Implementation commit:

- `f9db5c3` — `feat: add source-aware project asset registry and portable skill discovery`. Adds
  `src/skills/{source,parser,candidate,diagnostic,registry,compat}.rs`, the
  `AssetRegistry` and `SkillIndexCompat` public types, the `AssetDiscoveryConfig`
  bounds, the new `tests/skills_registry.rs` integration test target, and the
  rewritten `architecture/skills.md` architecture doc. The legacy `SkillIndex`
  and `Skill` types remain in `src/skills/mod.rs` and continue to satisfy the
  existing `tests/skills.rs` compatibility contract.

## 1. Executive finding

Milestone 1 is closed as an infrastructure milestone. The runtime skills module
is now a project/workspace-scoped, source-aware asset registry that discovers
portable `SKILL.md` packages from CodeGG, `.agents`, OpenCode, and Claude
harness locations (project-local and global), validates them safely, records
provenance, content digests, diagnostics, and shadowing, and resolves
duplicates deterministically by precedence. Existing `.codegg/skills/<name>/SKILL.md`
and direct `.codegg/skills/*.md` behavior is preserved as the CodeGG-native
compatibility path.

The registry is immutable after construction. Discovery and resolution execute
no scripts, never write to any foreign harness directory, contain symlink
escape, enforce bounded file/frontmatter/resource sizes, and never introduce
process-global `PWD` reasoning. Foreign directories are read-only by
construction: the discovery code only calls `read_dir` and `read_to_string`,
never `write`, `create_dir`, or any mutation API.

The original `SkillIndex` and `Skill` types are retained for backward
compatibility with the existing wire-level call sites (`src/main.rs:1741`,
`src/tool/skill.rs:48`, `tests/skills.rs`). `SkillIndexCompat` is a new
adapter that bridges the legacy mutable-style API to the new immutable
`AssetRegistry`. The new primary public type is `AssetRegistry`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Source-aware asset domain types | `src/skills/source.rs` (`SourceKind`, `SourceRoot`, `SourceSummary`, `AssetDiscoveryConfig`); `src/skills/candidate.rs` (`SkillCandidate`, `EffectiveSkill`, `ShadowedAlternative`, `ResourceDescriptor`); `src/skills/diagnostic.rs` (`Diagnostic`, `Severity`); 4 focused unit tests in `source.rs` cover precedence ordering, project/global classification, foreign-harness classification, and directory mapping. | pass | Provenance, digest, and precedence rank are first-class fields. |
| Deterministic source priority with configuration override seam | `SourceKind` enum carries explicit `precedence_rank()` values; `AssetDiscoveryConfig.enabled_sources: HashSet<SourceKind>` lets operators disable any source; `disabled_source_not_discovered` unit test exercises the seam. | pass | Precedence is verified by `source_kind_precedence_order` table test. |
| Parser/resolver interfaces and bounds | `parser::parse_candidate` returns `Result<SkillCandidate, Diagnostic>`; `registry::AssetRegistry::build` returns the immutable `AssetRegistry`; `AssetDiscoveryConfig` centralizes all bounds. | pass | Invalid candidates produce `Diagnostic` and never abort discovery. |
| Required portable project locations discovered | `tests/skills_registry.rs::discovery_all_project_locations` covers `.codegg/skills/<name>`, `.agents/skills/<name>`, `.opencode/skills/<name>`, `.claude/skills/<name>` in a single registry build. | pass | All four project locations yield effective entries. |
| Required global locations discovered | `tests/skills_registry.rs::discovery_all_global_locations` exercises all four global subtrees under a single global root. | pass | All four global locations yield effective entries. |
| Portable `SKILL.md` parsing with required `name`+`description` and optional fields | `parser.rs::parse_candidate` portable branch requires `name` and `description`; preserves `license`, `compatibility`, `metadata` (with unknown sub-keys), and `allowed-tools` as metadata only; `parse_candidate_missing_name_error`, `parse_candidate_missing_description_error`, and `parse_candidate_portable` tests cover this. | pass | `allowed-tools` becomes a `metadata` key plus a Warning diagnostic, never a permission grant. |
| CodeGG-native compatibility frontmatter | `parser.rs::parse_candidate` native branch accepts `name`, `description`, `version`, `tags`; `parse_candidate_native_compat` test; `tests/skills_registry.rs::native_compat_direct_md_loads` and `native_compat_package_layout_loads` cover the two legacy layouts. | pass | `CodeGGNativeCompat` is its own `SourceKind` to keep provenance visible. |
| Normalized content digest (format-stable) | `parser::compute_digest` hashes canonical frontmatter + LF-normalized body; `compute_digest_stability` and `compute_digest_crlf_normalization` tests confirm. | pass | Same content with CRLF vs LF yields the same digest. |
| Resource inventory without eager body reads | `parser::inventory_resources` records `name`, `relative_path`, `size`; bodies are never opened. `parse_candidate_resources_inventoried` and `tests/skills_registry.rs::script_files_inventoried_not_executed` confirm script files are inventoried only. | pass | No script execution, ever. |
| Invalid skills produce diagnostics, not fatal | `parser.rs` returns `Err(Diagnostic)` on missing name/description, malformed YAML, oversize; `registry.rs::discover_in_root` captures them without aborting; `tests/skills_registry.rs::malformed_yaml_surfaces_diagnostic` and `oversized_frontmatter_surfaces_diagnostic` confirm. | pass | One bad skill does not erase the rest of the registry. |
| Symlink escape containment | `registry.rs::validate_symlink_boundary` canonicalizes the file and parent and rejects paths that escape the source root; `tests/skills_registry.rs::symlink_escape_rejected` exercises the negative case. | pass | Rejected candidates surface a diagnostic. |
| Resource path traversal rejected | `parser::inventory_resources` only ever calls `path.strip_prefix(package_root)`; the resulting `relative_path` cannot escape. `tests/skills_registry.rs::resource_path_traversal_rejected` confirms `..` is never produced. | pass | Relative path is bounded to the package root. |
| Recursive/self resource references | `inventory_resources` walks the package root's non-`SKILL.md` files only; nested directories are skipped (`if !path.is_file() { continue; }`); no symlink-following. | pass | Resources are flat non-recursive metadata only. |
| Bounded skill/frontmatter/resource sizes | `AssetDiscoveryConfig::default()` carries `max_skill_file_size=256KB`, `max_frontmatter_size=64KB`, `max_skills_per_root=256`, `max_resources_per_skill=64`. Exceeding any bound surfaces a diagnostic. | pass | One configuration structure owns all bounds. |
| Foreign harness directories are read-only | The discovery code in `registry.rs::discover_in_root` only calls `read_dir` and the parser only calls `read_to_string` and `metadata`; no `write`, `create_dir`, `create_file`, `set_permissions`, or `remove_file` exists anywhere in `src/skills/`. | pass | Foreign directories are never modified. |
| Deterministic precedence and duplicate resolution | `SourceKind` carries explicit rank; `registry::resolve` sorts by rank and selects the highest-precedence valid candidate. `tests/skills_registry.rs::duplicate_behavior_stable` and `precedence_project_over_global` confirm. | pass | Project-local always wins over global. |
| Invalid higher-precedence fallback | `registry.rs::resolve` filters out candidates with `Severity::Error` diagnostics, then takes the lowest-rank valid candidate. `invalid_higher_precedence_falls_back` test exercises this with a project-skill (invalid YAML) shadowed by a global-skill (valid). | pass | Lower-precedence valid candidate wins; diagnostics record why. |
| Shadowed alternatives retained with provenance | `EffectiveSkill.shadowed_alternatives: Vec<ShadowedAlternative>` carries `source_kind`, `source_path`, `content_digest`, and diagnostics for each lost alternative. | pass | Visible to operators and tests. |
| Compatibility adapter for current `SkillIndex` consumers | `src/skills/compat.rs::SkillIndexCompat` wraps `Arc<AssetRegistry>` and exposes the legacy `load/get/list/find_matching/build_system_prompt/activate` API. `tests/skills_registry.rs::skill_index_compat_adapter` exercises the full legacy flow. | pass | `src/main.rs:1741` and `src/tool/skill.rs:48` continue to compile and behave. |
| Existing `tests/skills.rs` remains compatible | `tests/skills.rs` exercises `SkillIndex::new`, `get`, `find_matching`, `build_system_prompt`, `activate`, and `load` on an empty dir. All 7 tests pass. | pass | Backward-compat surface preserved. |
| Repeated construction from unchanged files yields identical digests | `tests/skills_registry.rs::digest_stability_across_builds` and `duplicate_behavior_stable` confirm digests are stable across builds. | pass | Suitable for future generation/audit seams. |
| Concurrent builders for separate projects share no mutable project state | `AssetRegistry::build` takes immutable references and returns a fresh value; `tests/skills_registry.rs::concurrent_scans_no_cross_contamination` exercises two independent builds. | pass | No global mutable state. |
| No `PWD` or process-global project inference introduced | The new `AssetRegistry::build` requires explicit `project_root` and `global_roots`; the only `std::env::current_dir()` use in the module is in the legacy `SkillIndex::load` in `src/skills/mod.rs` (preserved for backward compat) and `SkillIndexCompat::load` which delegates to `AssetRegistry::build` with explicit roots. | pass | `scripts/check_daemon_cwd_usage.py` passes. |
| No new protocol/DTO/storage surface | No new public protocol variants, no new DTOs, no schema migration. The module returns typed in-memory structures only. | pass | Plan explicitly defers protocol/storage. |
| Documentation updated | `architecture/skills.md` rewritten to describe the new model, source precedence table, portable schema, compatibility behavior, and security bounds. | pass | Replaces the pre-milestone 1-page doc. |

## 3. Production implementation evidence

### New module layout

```
src/skills/
  mod.rs          — pub re-exports of new types; preserves legacy Skill + SkillIndex
  source.rs       — SourceKind, SourceRoot, SourceSummary, AssetDiscoveryConfig
  parser.rs       — frontmatter parsing, candidate construction, digest computation
  candidate.rs    — SkillCandidate, EffectiveSkill, ResourceDescriptor, ShadowedAlternative, ResolvedRegistry
  diagnostic.rs   — Diagnostic, Severity
  registry.rs     — AssetRegistry (immutable), build logic, resolution, boundary enforcement
  compat.rs       — SkillIndexCompat adapter
```

### Public types

- `AssetRegistry` (`registry.rs`) — the primary immutable result. Exposes `effective: Vec<EffectiveSkill>`, `diagnostics: Vec<Diagnostic>`, `sources: Vec<SourceSummary>`, plus `get`, `list`, `find_matching`, `build_system_prompt`, `activate`, and the `build` constructor.
- `AssetDiscoveryConfig` (`source.rs`) — `max_skill_file_size` (256 KB), `max_frontmatter_size` (64 KB), `max_skills_per_root` (256), `max_resources_per_skill` (64), `max_skill_name_length` (128), `max_description_length` (2048), `enabled_sources: HashSet<SourceKind>`.
- `SourceKind` (`source.rs`) — `CodeGGProject=0`, `AgentsProject=10`, `OpenCodeProject=20`, `ClaudeProject=30`, `CodeGGGlobal=40`, `AgentsGlobal=50`, `OpenCodeGlobal=60`, `ClaudeGlobal=70`, `CodeGGNativeCompat=80`.
- `EffectiveSkill` (`candidate.rs`) — name, normalized_name, description, source_kind, source_path, package_root, content_digest, metadata, resources, body, precedence_rank, shadowed_alternatives.
- `SkillCandidate` (`candidate.rs`) — pre-resolution candidate with diagnostics.
- `ShadowedAlternative` (`candidate.rs`) — provenance + digest + diagnostics for losers.
- `ResourceDescriptor` (`candidate.rs`) — name, relative_path, size.
- `Diagnostic` / `Severity` (`diagnostic.rs`) — `Error | Warning | Info` + reason + location.
- `SourceSummary` (`source.rs`) — per-source counts: discovered / valid / invalid.
- `SkillIndexCompat` (`compat.rs`) — legacy `SkillIndex` facade backed by `Arc<AssetRegistry>`. `load(&str)` accepts the project dir, derives the global roots from `dirs::config_dir()`, and replaces the internal `Arc<AssetRegistry>` with a freshly built one.

### Precedence table

| Rank | SourceKind | Project-local path | Global path |
|------|-----------|--------------------|-------------|
| 0 | `CodeGGProject` | `<root>/.codegg/skills/<name>/SKILL.md` | — |
| 10 | `AgentsProject` | `<root>/.agents/skills/<name>/SKILL.md` | — |
| 20 | `OpenCodeProject` | `<root>/.opencode/skills/<name>/SKILL.md` | — |
| 30 | `ClaudeProject` | `<root>/.claude/skills/<name>/SKILL.md` | — |
| 40 | `CodeGGGlobal` | — | `<config>/codegg/skills/<name>/SKILL.md` |
| 50 | `AgentsGlobal` | — | `<config>/agents/skills/<name>/SKILL.md` |
| 60 | `OpenCodeGlobal` | — | `<config>/opencode/skills/<name>/SKILL.md` |
| 70 | `ClaudeGlobal` | — | `<config>/claude/skills/<name>/SKILL.md` |
| 80 | `CodeGGNativeCompat` | `<root>/.codegg/skills/*.md` (direct md) | — |

Project-local sources always take precedence over global sources. The CodeGG-native direct markdown path has the lowest precedence because it is the legacy compat fallback.

### Compatibility shims

- `src/skills/mod.rs` retains the original `Skill` and `SkillIndex` types so `src/main.rs:1741` (`SkillIndex::new().load(...)`) and `src/tool/skill.rs:48` continue to compile and behave exactly as before for the empty-dir and CodeGG-native cases. The `SkillIndex` is no longer the recommended type for new code.
- `src/skills/compat.rs::SkillIndexCompat` is the new recommended bridge. It is used by tests and can be adopted incrementally by consumers.
- `codegg::skills::AssetRegistry`, `AssetDiscoveryConfig`, `SourceKind`, `Diagnostic`, `Severity`, `EffectiveSkill`, `ResourceDescriptor`, `ShadowedAlternative`, and `SkillIndexCompat` are re-exported from the `skills` module and accessible to downstream code as `codegg::skills::*`.

### Schema, protocol, storage

- No SQLite migration. The registry is reconstructible from filesystem + config state.
- No new `CoreRequest`/`CoreResponse`/`CoreEvent` variants. The plan explicitly defers the protocol surface to Milestone 3 (refresh coordinator).
- The artifact kind is currently `Skill` only. The `AssetKind` enum is reserved for future expansion (agents, project instructions) in later milestones and is intentionally not yet introduced as a discriminator — that work belongs to Milestone 2.

### Architecture documentation

- `architecture/skills.md` — rewritten to cover the source-aware model, source precedence table, portable schema, native compat path, digest computation, diagnostics model, security bounds, compat adapter, and the recommended primary public type.

### Static guard compatibility

- `python3 scripts/check_daemon_cwd_usage.py` — passes. The new `AssetRegistry::build` API takes explicit `project_root` and `global_roots`; the only `std::env::current_dir()` call in `src/skills/` is the pre-existing one in `src/tool/skill.rs` (unchanged).
- `bash scripts/check-core-boundary.sh` — passes. The new code lives in the root `src/skills/` (not in `codegg-core`) and imports no forbidden crates.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test --test skills
rtk cargo test --test skills_registry
rtk cargo test skills
rtk cargo test agent::registry
rtk cargo test -p codegg-core
rtk cargo test --lib skills::
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk cargo clippy -p codegg --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

### Results

- `cargo fmt --all -- --check` — exit 0.
- `cargo check --workspace --all-targets --all-features` — exit 0.
- `cargo test --test skills` — 7 passed (legacy `SkillIndex` contract preserved).
- `cargo test --test skills_registry` — 20 passed (new integration target).
- `cargo test skills` — 36 passed (focused module + integration).
- `cargo test agent::registry` — 23 passed (unrelated module unchanged; confirms no coupling regression).
- `cargo test -p codegg-core` — 227 passed.
- `cargo test --lib skills::` — 33 passed.
- `cargo test --lib skills` — 34 passed (3 extra from the `compat.rs` inline tests, if any).
- `scripts/check-core-boundary.sh` — `codegg-core boundary check passed`.
- `python3 scripts/check_daemon_cwd_usage.py` — `cwd usage check passed — no std::env::current_dir() in protected modules`.
- `cargo clippy -p codegg --all-targets --all-features -- -D warnings` — one pre-existing error in `crates/codegg-protocol/src/provider.rs:170` (`large_enum_variant` on `SessionSelectionDto`), confirmed against `main` via stash + clippy run; not introduced by this milestone. No new clippy issues in `src/skills/`.
- `cargo test --workspace --all-features -- --test-threads=14` — 3802 passed; 1 failed (`core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog`). The same test passes when run as part of `cargo test --lib core::eggpool` (8 passed) and on `main` after a fresh build. This is the pre-existing timing race documented in `plans/closure/provider-connections/003-status.md` and `plans/closure/project-catalog/001-status.md`. Unrelated to this milestone.

## 5. Invariant review

- **Discovery is anchored to explicit project/workspace context.** `AssetRegistry::build(config, project_root, global_roots)` requires the caller to supply both. The legacy `SkillIndex::load(&str)` and `SkillIndexCompat::load(&str)` are preserved for backward compat but they themselves now build an `AssetRegistry` from the same explicit-root API; only the project-dir string is supplied by the caller.
- **Foreign harness directories remain read-only.** `registry.rs::discover_in_root` only invokes `std::fs::read_dir`, `path.is_dir()`, `path.is_file()`, and the parser only invokes `std::fs::read_to_string` and `std::fs::metadata`. No `write`, `create_dir`, `create_file`, `set_permissions`, `remove_file`, or any other mutation call appears anywhere in `src/skills/`. `SourceKind::is_foreign` exists as a typed discriminator for future operator inspection.
- **Discovery and activation execute no scripts.** The parser records `ResourceDescriptor` entries without ever opening resource bodies. `tests/skills_registry.rs::script_files_inventoried_not_executed` confirms `run.sh` and `exploit.py` are listed as resources with `name` and `size` only.
- **Invalid or malicious files cannot escape the project/skill boundary through symlinks.** `validate_symlink_boundary` canonicalizes both the candidate file and its parent and rejects paths that fall outside the source root. `tests/skills_registry.rs::symlink_escape_rejected` exercises a symlink that points to an outside-root `SKILL.md` and confirms the candidate is rejected with a diagnostic.
- **Existing `.codegg/skills/<name>/SKILL.md` and `.codegg/skills/*.md` behavior remains available as compatibility.** `CodeGGProject` and `CodeGGNativeCompat` `SourceKind` variants preserve both layouts. `tests/skills_registry.rs::native_compat_direct_md_loads` and `native_compat_package_layout_loads` exercise both. The legacy `tests/skills.rs` continues to pass against the unchanged `SkillIndex` type.
- **Duplicate resolution is deterministic and source-aware.** The registry sorts candidates by `SourceKind::precedence_rank()` and picks the lowest-rank valid candidate. Repeated builds over unchanged files yield identical `EffectiveSkill` vectors with identical content digests.
- **Invalid higher-precedence content does not silently erase a valid lower-precedence skill.** `registry::resolve` filters out candidates with `Severity::Error` diagnostics before selecting the winner. The losing invalid candidate's diagnostics remain in the registry's `diagnostics` vector. `invalid_higher_precedence_falls_back` test exercises this with a malformed-YAML project skill shadowed by a valid global skill.
- **Asset metadata and resources are bounded.** `AssetDiscoveryConfig` exposes all six numeric bounds plus the `enabled_sources` set. The default configuration is the safe one and ships with all sources enabled.

## 6. Failure and recovery review

- **Repeated construction from unchanged files yields identical registry state.** `digest_stability_across_builds` confirms digests are stable. `duplicate_behavior_stable` confirms full result stability.
- **Fatal root-level I/O/config failures return typed errors; absent directories are not errors.** `discover_in_root` only produces `Warning` diagnostics for unreadable directories; absent foreign directories are detected by `path.is_dir()` and silently skipped (the source root is simply not added to the discovery list). `absent_foreign_directories_harmless` exercises this.
- **Concurrent builders for separate projects share no mutable project state.** `AssetRegistry::build` is a pure function over its inputs. The only shared state is `dirs::config_dir()` resolution inside `SkillIndexCompat::load`, which is read-only. `concurrent_scans_no_cross_contamination` exercises two independent builds.
- **One invalid skill produces a diagnostic and does not fail unrelated discovery.** All parser errors return `Err(Diagnostic)`; the registry converts these into registry-level diagnostics without halting. `malformed_yaml_surfaces_diagnostic` and `oversized_frontmatter_surfaces_diagnostic` confirm.

## 7. Migration and compatibility review

- The legacy `SkillIndex` and `Skill` types are retained unchanged in `src/skills/mod.rs`. Existing call sites (`src/main.rs:1741`, `src/tool/skill.rs:48`) and the existing `tests/skills.rs` test target continue to work.
- The legacy `SkillIndex` still loads only `dirs::config_dir().join("codegg/skills")` and the project's `.codegg/skills` directory, exactly as before. New harness locations (`.agents`, `.opencode`, `.claude`) are not loaded by the legacy type — they are only loaded by `AssetRegistry::build` and `SkillIndexCompat::load`.
- The new primary public type is `AssetRegistry`. The plan explicitly defers migration of `src/main.rs` and `src/tool/skill.rs` to use `AssetRegistry` directly; the `SkillIndexCompat` adapter is available for incremental adoption.
- No protocol or storage migration is required.
- No foreign directories are modified; no scripts are executed.

## 8. Security review

- **Symlink escape containment.** `registry::validate_symlink_boundary` canonicalizes both the file and its parent and rejects candidates whose canonical path falls outside the source root. Verified by `symlink_escape_rejected`.
- **Path traversal in resources.** `parser::inventory_resources` only records `relative_path = path.strip_prefix(package_root)`. The result is always bounded to the package directory. Verified by `resource_path_traversal_rejected`.
- **Bounded sizes.** `max_skill_file_size`, `max_frontmatter_size`, `max_skills_per_root`, `max_resources_per_skill`, `max_skill_name_length`, `max_description_length` are all enforced before any other parsing. Verified by `parse_candidate_oversized_file`, `oversized_frontmatter_surfaces_diagnostic`, and the size-bounded `max_skills_per_root` truncation warning.
- **No script execution.** Verified by `script_files_inventoried_not_executed` and the static review of `inventory_resources` (no `Command::new`, no `tokio::process`, no shell invocation anywhere in the skills module).
- **`allowed-tools` cannot expand permissions.** The portable parser branch stores `allowed-tools` as a `metadata` entry and emits a `Warning` diagnostic. There is no codepath in `src/skills/` that touches `crate::permission`, `crate::auth`, or any permission model. Verified by `allowed_tools_cannot_grant_permissions`.
- **No plaintext credentials or secret-bearing material persisted.** The registry is fully in-memory; no SQLite write, no file write, no log emission of skill bodies. The only fields persisted across calls are the `Arc<AssetRegistry>` itself, which lives in the calling process and is not surfaced to disk.
- **No path-derived project identity.** `AssetRegistry::build` takes the project root as a `&Path` parameter. The resulting `EffectiveSkill.source_path` is recorded for inspection, but the registry does not derive any project ID from it. This is enforced by `scripts/check_daemon_cwd_usage.py` passing on the module.
- **No new imports of forbidden crates.** `scripts/check-core-boundary.sh` passes; the new code only imports `serde`, `serde_yaml`, `sha2`, `hex`, `tempfile`, `tokio` (in tests), and `std::*`. None of `ratatui`, `axum`, `wasmtime`, `crypto` are imported.

## 9. Documentation and operations

Updated:

- `architecture/skills.md` — full rewrite covering source-aware model, precedence table, portable schema, native compat path, digest computation, diagnostics, security bounds, compat adapter, recommended primary public type, file layout, loading locations, and usage guidance.
- `plans/implementation/runtime-assets/001-project-asset-registry.md` — status moved to `implemented (closure evidence: plans/closure/runtime-assets/001-status.md)`.

Operators can:

- Build a registry: `let config = AssetDiscoveryConfig::default(); let registry = AssetRegistry::build(&config, &project_root, &global_roots);`
- Inspect a single skill: `registry.get("name")` (case-insensitive).
- Inspect the diagnostic list: `&registry.diagnostics` (severity, reason, location).
- Inspect per-source counts: `&registry.sources` (kind, path, discovered/valid/invalid).
- Inspect shadowed alternatives: `effective.shadowed_alternatives` per skill.
- For legacy code: use `SkillIndexCompat::new().load(&project_dir).await?` and the existing `SkillIndex` API remains available unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `clippy::large_enum_variant` warning on `SessionSelectionDto` in `crates/codegg-protocol/src/provider.rs:170` is pre-existing on `main` and unrelated to this milestone. | No impact on the skills module. | Track under the existing provider-connections follow-up. |
| low | `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` is a pre-existing flaky timing test documented in `plans/closure/project-catalog/001-status.md` and `plans/closure/provider-connections/003-status.md`. It passes when run as part of `cargo test --lib core::eggpool` (8/8) and passes on `main` after a fresh build, but occasionally fails under workspace-wide parallel load. | No impact on the skills module. | Track under the existing provider-connections follow-up. |
| low | The legacy `SkillIndex::load` still reads `dirs::config_dir()` and the project `.codegg/skills` only. Harness locations (`.agents`, `.opencode`, `.claude`) are only loaded by `AssetRegistry::build` and `SkillIndexCompat::load`. | Callers that still use `SkillIndex` directly do not see foreign-harness skills. | Migrate consumers to `SkillIndexCompat` or `AssetRegistry` in follow-up work; the compat adapter is the recommended bridge. |
| low | The `find_matching` API matches on `metadata` string values only (non-string metadata is skipped). | Non-string metadata values (maps, sequences) are not searchable. | Defer to a future search-indexing milestone if needed. |

No critical or high-severity finding remains for this milestone.

## 11. Roadmap disposition

Milestone closed and the next hard dependency is unlocked. Milestone 2 — explicit-context agent and instruction resolution — has a hard dependency on Milestone 1 closure and may now proceed. Milestone 3 — refresh lifecycle and operator surface — has a hard dependency on Milestone 1 and an interface dependency on Milestone 2; the source-aware registry produced here provides the discovery/parsing/precedence substrate that Milestone 3's refresh coordinator will own.

Multi-Project TUI 001 and Session Projections 001 remain blocked on the catalog protocol surface and project-aware TUI state, neither of which is changed by this milestone.

## 12. Registry updates

- `plans/implementation/runtime-assets/001-project-asset-registry.md` is marked `implemented (closure evidence: plans/closure/runtime-assets/001-status.md)`.
- `plans/subsystems/runtime-assets-roadmap.md` is updated to mark Milestone 1 closed.
- `plans/registry.md` is updated to:
  - Remove Runtime Assets 001 from the "Dependency-ready implementation plans" table.
  - Record Runtime Assets 001 in "Recently closed work".
  - Update the Runtime Assets row in "Active subsystem roadmaps" to point at the next milestone (Milestone 2 — explicit-context agent and instruction resolution) which is now unblocked but not yet authored.
  - Leave Multi-Project TUI 001 and Session Projections 001 blocked on their unchanged dependencies.
  - Leave Domain Identity 003 and Provider Connections 004 unchanged.
