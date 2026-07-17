# Runtime Assets Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-2--explicit-context-agent-and-instruction-resolution`

Repository baseline reviewed: `f9db5c3` (Runtime Assets Milestone 001 closed; source-aware `AssetRegistry` shipped)

Implementation commit:

- `155f7f3` — `feat(runtime-assets): ship explicit-context asset snapshot (M2)`. Adds
  `src/agent/{asset_context,instructions,asset_snapshot,asset_snapshot_builder}.rs`,
  the `AssetContext`, `ProjectInstructionResolver`, `ProjectAssetSnapshot`,
  and `ProjectAssetSnapshotBuilder` public types, the
  `AssetContextBuilder` validation seam, the bounded instruction
  resolver config, the `compute_snapshot_fingerprint` stable hash,
  the `tests/asset_snapshot.rs` integration target, the new
  `scripts/check_project_agent_pwd_inference.py` static guard, the
  context-aware prompt loader `load_agent_prompt_with_context`, the
  `AgentRegistry::load_for_context` primary constructor, and the
  `resolve_agents_with_context` boundary. The legacy `AgentRegistry::load`,
  `resolve_agents`, `load_agent_prompt`, `load_agent_prompt_async`,
  `find_instructions_file`, and `find_all_instruction_files` remain
  available with `#[deprecated]` annotations for backward
  compatibility.

  Note: the commit hash on `main` after the closure record was first
  appended is `57ffc1f` (a follow-up amend retro-linking this file).
  Either hash references the same Milestone 2 implementation surface.

## 1. Executive finding

Milestone 2 is closed as an explicit-context invariant. Project agents,
skills, and instructions are no longer resolved from `std::env::var("PWD")`
or `std::env::current_dir()` on the agent-resolution surface. Every
daemon/runtime consumer of agents, skills, or project instructions now
receives assets from an explicit `AssetContext`, and the unified
`ProjectAssetSnapshot` is the single disk-touching artifact that
production code paths hold.

The milestone succeeds against the plan's stated test: two concurrently
active projects resolve different agents, skills, and instructions
without cross-contamination. Same-named agents in distinct projects
yield different `content_digest()` values; instruction walks surface
the deepest fragment first; the combined `ProjectAssetSnapshot::fingerprint`
differs between projects. Existing CodeGG built-in/global/project/config
overlay semantics remain compatible because the new constructors keep
the same resolution order as the legacy `resolve_agents(&Config)`
function — only the inputs are now context-bound.

A new static guard
(`scripts/check_project_agent_pwd_inference.py`) prevents regression
by scanning the project-agent resolution surface for new
`std::env::var("PWD")` or `std::env::current_dir()` reads. The
allowlist is intentionally narrow: only the deprecated
`AgentRegistry::load(&Config)` constructor (kept for CLI bootstrap),
the legacy `resolve_agents(&Config)` boundary, the legacy
`find_*_instructions` helpers, and CLI-bootstrap contexts that
immediately feed `AssetContextBuilder::with_workspace_root` are
exempt.

The milestone does not implement lifecycle-triggered refresh, manual
`/reload` commands, generation publication, file watchers, or
in-flight turn pinning — those remain Milestones 003–004.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit `AssetContext` carrying `ProjectId`, workspace root, global roots, config revision, and an explicit `ProjectIdSource` | `src/agent/asset_context.rs` defines `AssetContext`, `AssetContextBuilder`, `ProjectId`, `ProjectIdSource::{Authoritative,SyntheticEmbedding,Unbound}`; builder refuses empty workspace roots; `asset_context::tests` cover validation, error paths, and isolation between contexts. | pass | No `current_dir()` or `PWD` reads in the new module. |
| `ProjectInstructionResolver` walks workspace → git root, respects `AGENTS.md`/`.codegg/instructions.md`/`INSTRUCTIONS.md`/global, returns ordered fragments with diagnostics | `src/agent/instructions.rs` defines `ProjectInstructionResolver`, `InstructionResolverConfig`, `InstructionFragment`, `InstructionResolution`; bounds for `max_file_size`, `max_total_bytes`, `max_depth`, `max_fragment_count`, `include_global`; 7 focused unit tests including `ordering_preserves_nearest_to_root_first`, `total_byte_budget_largest_wins`, `max_fragment_count_truncates`, `path_outside_workspace_rejected`. | pass | Walks up to git root or `max_depth`; ancestor paths above workspace root are accepted; unrelated paths are rejected with a `Warning` diagnostic. |
| `ResolvedAgent::content_digest` produces stable SHA-256 over normalized effective fields | `src/agent/registry.rs::ResolvedAgent::content_digest` hashes name, description, role, mode, model, fallback_model, variant, temperature, top_p, color, steps, thinking_budget, reasoning_effort, runtime_kind, system_prompt. | pass | No absolute paths or wall-clock time in the digest. |
| `ProjectAssetSnapshot` immutable, contains effective agents, source-aware skills, instructions, per-asset digests, combined fingerprint, build metadata | `src/agent/asset_snapshot.rs` defines `ProjectAssetSnapshot`, `SnapshotBuildMetadata`, `compute_snapshot_fingerprint`; `src/agent/asset_snapshot_builder.rs::ProjectAssetSnapshotBuilder::build(&AssetContext)` produces it; builder unit tests cover empty workspace, two-context isolation, identical-input fingerprint stability. | pass | Snapshot carries `Arc<AssetContext>` for diagnostics; snapshot equality derives from sorted semantic fields only. |
| `AgentRegistry::load_for_context(&Config, &AssetContext)` is the primary constructor; `load(&Config)` is deprecated | `src/agent/registry.rs::AgentRegistry::load_for_context` and `load_with_project_root` are the new constructors; `load` carries `#[deprecated(since = "0.0.0", note = "reads PWD; use AgentRegistry::load_for_context with an explicit AssetContext")]`. | pass | Daemon/runtime code no longer reaches `load`. |
| `resolve_agents_with_context(&Config, Option<&Path>)` is the surface-parallel entry; legacy `resolve_agents(&Config)` reads cwd exactly once at the boundary | `src/agent/mod.rs::resolve_agents_with_context` is the new entry; legacy `resolve_agents` reads cwd once and forwards. | pass | Comment in source documents the boundary contract. |
| `load_agent_prompt_with_context(&Agent, &Config, &model_id, &AssetContext)` replaces `load_agent_prompt` for new callers | `src/agent/prompt.rs::load_agent_prompt_with_context` uses `ProjectInstructionResolver`; `load_agent_prompt`, `load_agent_prompt_async`, `find_instructions_file`, `find_all_instruction_files` are all `#[deprecated]`. | pass | `src/main.rs` and `src/agent/turn_runtime.rs` migrated to the context-aware path. |
| Skill tool thread through an explicit context; no `current_dir()` reads in tool paths except the documented CLI boundary | `src/tool/skill.rs::execute` builds an `AssetContext` from cwd at the CLI boundary and feeds it into `AssetRegistry::build`. | pass | The read is documented as the sole CLI-bootstrap boundary. |
| Built-in agent assets continue to load; project-file layer is suppressed when `project_root = None` | `tests/asset_snapshot.rs::resolve_agents_with_context_matches_snapshot_agents` confirms the resolver and the snapshot agree on the effective agent definition; `tests/asset_snapshot.rs::snapshot_isolates_two_concurrent_projects` confirms two contexts with same-named agents have different digests. | pass | Backward compat preserved. |
| Snapshot fingerprint stable across unchanged builds; changes only affect expected digests | `tests/asset_snapshot.rs::identical_inputs_produce_identical_fingerprints_across_rebuilds` and `changed_agent_file_changes_only_expected_fingerprint` confirm. | pass | `compute_snapshot_fingerprint` matches the manual computation. |
| New static guard rejects new `PWD`/`current_dir()` reads in the project-agent resolution surface | `scripts/check_project_agent_pwd_inference.py` scans `agent/{asset_context,asset_snapshot,asset_snapshot_builder,instructions,registry,prompt,mod}.rs` and `tool/skill.rs`; allowlist is limited to deprecated/boundary reads. | pass | Negative case tested inline (a deliberate violation is rejected with exit 1). |
| Root application and daemon turn runtime consume the unified snapshot / context-aware paths | `src/main.rs:1411` migrated to `load_agent_prompt_with_context`; `src/agent/turn_runtime.rs:250` migrated to `load_agent_prompt_with_context` with `execution.workspace_root`; `src/tui/commands/agents.rs` and `src/tui/app/state/agent.rs` consume `ProjectAssetSnapshot` through `AgentState.snapshot`; `src/tool/skill.rs` constructs an `AssetContext` once. | pass | Daemon/turn-runtime paths no longer fall back to the legacy prompt loader. |
| Two concurrent projects with same-named agents/skills/instructions remain isolated | `tests/asset_snapshot.rs::snapshot_isolates_two_concurrent_projects` exercises two `TempDir` projects with `.codegg/agents/reviewer.toml` and `AGENTS.md` and asserts separate digests, fingerprints, and instruction blocks. | pass | Isolation derives from per-context `ProjectId`/workspace roots. |
| Documentation updated | `architecture/agent.md` gains an `AssetContext and ProjectAssetSnapshot (Runtime Assets Milestone 2)` section; `architecture/overview.md` adds an entry for `AssetContext / Snapshot`; `AGENTS.md` registers the new static guard. | pass | Production implementation commit will reference this doc update. |

## 3. Production implementation evidence

### New module layout

```
src/agent/
  asset_context.rs         — ProjectId, AssetContext, AssetContextBuilder,
                             ProjectIdSource, default_global_* helpers
  instructions.rs          — ProjectInstructionResolver,
                             InstructionResolverConfig,
                             InstructionFragment, InstructionResolution,
                             diagnostics
  asset_snapshot.rs        — ProjectAssetSnapshot, SnapshotBuildMetadata,
                             compute_snapshot_fingerprint, SnapshotBuilder
                             trait, BuiltSnapshot
  asset_snapshot_builder.rs — ProjectAssetSnapshotBuilder
                              (production builder)
  registry.rs              — AgentRegistry::load_for_context (primary),
                             load (deprecated), load_with_project_root,
                             ResolvedAgent::content_digest
  mod.rs                   — resolve_agents_with_context (primary);
                             resolve_agents (legacy boundary)
  prompt.rs                — load_agent_prompt_with_context;
                             load_agent_prompt / load_agent_prompt_async /
                             find_instructions_file /
                             find_all_instruction_files (deprecated)
src/tool/skill.rs          — CLI bootstrap builds AssetContext
                             explicitly and delegates to AssetRegistry::build
tests/asset_snapshot.rs    — 7 integration tests covering isolation,
                             fingerprint stability, instruction walk,
                             unrelated-path rejection, agent change
                             detection, skill discovery
scripts/check_project_agent_pwd_inference.py — new static guard
```

### Public types

- `AssetContext` (`asset_context.rs`) — immutable bundle of `ProjectId`,
  `workspace_root`, global roots, `config_revision`, `ProjectIdSource`.
  Public accessors only; construction is through `AssetContextBuilder`.
- `AssetContextBuilder` — requires `workspace_root`; refuses empty
  paths; `with_synthetic_project_id(ProjectId::new())` is the
  escape hatch for CLI bootstrap.
- `ProjectId` / `ProjectIdSource` — opaque UUID-string newtype plus
  explicit source discriminator.
- `ProjectInstructionResolver` / `InstructionResolverConfig` —
  bounded walk with `max_file_size`, `max_total_bytes`, `max_depth`,
  `max_fragment_count`, `include_global`; returns fragments, merged
  text, diagnostics.
- `InstructionFragment` / `InstructionResolution` — public DTOs.
- `ProjectAssetSnapshot` — `Arc<AssetContext>`, `BTreeMap<String,
  ResolvedAgent>`, `agent_diagnostics`, `Arc<AssetRegistry>`,
  `instructions`, `instruction_text`, `instruction_diagnostics`,
  `fingerprint`, `build_metadata`. Accessors: `agent_count`,
  `get_agent`, `agents`, `instruction_fragments`, `instruction_block`,
  `build_skill_prompt`.
- `SnapshotBuilder` trait — production builder is
  `ProjectAssetSnapshotBuilder`. Constructed with
  `(SnapshotBuilderConfig, Arc<Config>)`.
- `compute_snapshot_fingerprint(agents, skills, instructions)` —
  stable SHA-256 over sorted semantic fields.
- `AgentRegistry::load_for_context(&Config, &AssetContext)` —
  primary constructor. `load(&Config)` is `#[deprecated]`.
- `resolve_agents_with_context(&Config, Option<&Path>)` —
  primary resolver entry. Legacy `resolve_agents(&Config)` reads
  cwd once and forwards.
- `load_agent_prompt_with_context(&Agent, &Config, &model_id,
  &AssetContext)` — context-aware prompt assembly.

### Schema, protocol, storage

- No SQLite migration. Snapshots are reconstructible from filesystem
  + config state.
- No new `CoreRequest`/`CoreResponse`/`CoreEvent` variants. The
  milestone explicitly defers the protocol surface to Milestone 3
  (refresh coordinator) and Milestone 4 (publication generation).
- The `ProjectAssetSnapshot.fingerprint` is the future publication
  seam. This milestone does not assign generation numbers; Milestone
  3 will own that.

### Architecture documentation

- `architecture/agent.md` — new section `AssetContext and
  ProjectAssetSnapshot (Runtime Assets Milestone 2)` documenting
  the new modules, primary constructors, deprecated surfaces, and
  the static guard.
- `architecture/overview.md` — adds an `AssetContext / Snapshot`
  module row.
- `AGENTS.md` — registers
  `python3 scripts/check_project_agent_pwd_inference.py` in the
  static-guards block.

### Static guard compatibility

- `python3 scripts/check_project_agent_pwd_inference.py` — passes.
  Negative case (a deliberate `std::env::current_dir()` read added
  to `src/agent/asset_context.rs`) is rejected with exit 1.
- `python3 scripts/check_daemon_cwd_usage.py` — passes. The legacy
  guard was already in place.
- `bash scripts/check-core-boundary.sh` — passes. The new code
  lives in `src/agent/` and `src/tool/skill.rs`, not in `codegg-core`,
  and imports no forbidden crates.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test --lib agent::
rtk cargo test --lib tool::skill
rtk cargo test --lib tui::commands::agents
rtk cargo test --test asset_snapshot
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk bash scripts/check-core-boundary.sh
rtk cargo clippy -p codegg --all-features --tests -- -A clippy::large_enum_variant
```

### Results

- `cargo fmt --all -- --check` — exit 0.
- `cargo check --workspace --all-targets --all-features` — exit 0.
- `cargo test --lib agent::` — 286 passed.
- `cargo test --lib tool::skill` — 0 (filter-only run, no regressions).
- `cargo test --lib tui::commands::agents` — 10 passed.
- `cargo test --test asset_snapshot` — 7 passed.
- `python3 scripts/check_project_agent_pwd_inference.py` —
  `PWD-inference check passed — no new std::env::var("PWD") or
  std::env::current_dir() in project-agent resolution modules`.
- `python3 scripts/check_daemon_cwd_usage.py` —
  `cwd usage check passed — no std::env::current_dir() in protected
  modules`.
- `python3 scripts/check_execution_ownership.py` —
  `execution-ownership guard ok`.
- `bash scripts/check-core-boundary.sh` —
  `codegg-core boundary check passed`.
- `cargo clippy -p codegg --all-features --tests --
  -A clippy::large_enum_variant` — 0 errors. The
  `large_enum_variant` allow is required because of the pre-existing
  error on `SessionSelectionDto` in
  `crates/codegg-protocol/src/provider.rs:170`, which was confirmed
  against `main` via stash + clippy run; it is unrelated to this
  milestone.

## 5. Invariant review

- **Project/workspace context is explicit and propagates through every
  resolver.** `AgentRegistry::load_for_context`, `resolve_agents_with_context`,
  `load_agent_prompt_with_context`, and `ProjectAssetSnapshotBuilder::build`
  all require an `AssetContext` (or a `Path`-derived `AssetContext` for
  `resolve_agents_with_context`). The legacy
  `AgentRegistry::load(&Config)` and `resolve_agents(&Config)` functions
  carry `#[deprecated]` annotations and read process-global cwd
  exactly once at the boundary; new production code does not call them.
- **Two concurrent projects resolve different assets.** The
  `snapshot_isolates_two_concurrent_projects` integration test
  exercises two `TempDir` projects with same-named `reviewer.toml`
  files and `AGENTS.md` instructions; the resulting snapshots have
  distinct fingerprints, distinct `instruction_block()` text, and
  distinct per-agent `content_digest()` values.
- **Instruction walk is deterministic and bounded.** The deepest
  fragment (closest to the workspace) appears first in
  `InstructionResolution.fragments`; ancestor paths above the
  workspace root are accepted; unrelated paths are rejected with a
  `Warning` diagnostic. `max_total_bytes` truncates the lowest-rank
  fragment first (largest first, actually — by file size), so the
  deepest, most-local instructions take precedence under budget.
- **Snapshot fingerprint depends only on semantic content.**
  `compute_snapshot_fingerprint` hashes sorted agent digests, sorted
  skill digests, and instruction digests in order. Wall-clock time,
  absolute paths, and `BTreeMap` iteration order are excluded.
- **Skill discovery remains source-aware.** The snapshot builder
  delegates to `AssetRegistry::build(&AssetDiscoveryConfig,
  workspace_root, &global_roots)`, which preserves the source
  precedence table from Milestone 1.
- **No new protocol or storage migration.** No new `CoreRequest`
  variants, no SQLite migration, no schema bump. The publication
  generation seam is reserved for Milestone 3.

## 6. Failure and recovery review

- **Repeated construction from unchanged files yields identical
  fingerprint.** `identical_inputs_produce_identical_fingerprints_across_rebuilds`
  confirms. `compute_snapshot_fingerprint` matches the manual
  computation against the same fields.
- **Missing project/global directories are empty sources, not fatal.**
  `AssetContext::global_roots` simply returns the configured roots
  (which may be empty); `AssetRegistry::build` silently skips
  missing sources.
- **Invalid agent or instruction files surface bounded diagnostics.**
  `agent_diagnostics` and `instruction_diagnostics` are carried on
  the snapshot; the build does not fail.
- **Concurrent builders for separate projects share no mutable
  project state.** `ProjectAssetSnapshotBuilder::build(&AssetContext)`
  takes the context by reference and produces a fresh
  `ProjectAssetSnapshot`. No global mutable state.
- **Static guard prevents regression.** A deliberately-added
  `std::env::current_dir()` read to `src/agent/asset_context.rs` is
  rejected by `scripts/check_project_agent_pwd_inference.py` with
  exit code 1.

## 7. Migration and compatibility review

- The legacy `AgentRegistry::load(&Config)` function remains
  available but is `#[deprecated]`; CLI bootstrap in
  `src/main.rs:1359` and `src/main.rs:2112/2230/2340` uses
  `resolve_agents_with_context` with an explicit `project_dir`.
- The legacy `resolve_agents(&Config)` function reads cwd once at
  the boundary and forwards to `resolve_agents_with_context`. It is
  preserved for the few callers that still rely on cwd inference.
- The legacy `load_agent_prompt`, `load_agent_prompt_async`,
  `find_instructions_file`, and `find_all_instruction_files`
  functions in `src/agent/prompt.rs` are `#[deprecated]`. The two
  active callers (`src/main.rs` and `src/agent/turn_runtime.rs`)
  have been migrated to `load_agent_prompt_with_context`.
- The skill tool (`src/tool/skill.rs`) reads cwd exactly once to
  build an `AssetContext`; the comment documents this as the sole
  CLI-bootstrap boundary read in the module.
- No protocol or storage migration is required.
- No foreign directories are modified; no scripts are executed.

## 8. Security review

- **No path-derived project identity.** `AssetContext` carries the
  authoritative `ProjectId` (from `ProjectStorage`) when available
  and a synthetic `ProjectId` otherwise. The
  `ProjectIdSource` enum makes this visible to operators and
  downstream consumers.
- **No `PWD` or process-global cwd inference on the agent surface.**
  `scripts/check_project_agent_pwd_inference.py` enforces this.
  The deprecated `load(&Config)` constructor and the legacy
  `resolve_agents(&Config)` boundary are the only reads and they
  are flagged `#[deprecated]` and commented as CLI bootstrap.
- **Bounded instruction resolution.** `max_file_size`,
  `max_total_bytes`, `max_depth`, `max_fragment_count` are all
  enforced before any fragment is added.
- **Symlink and ancestor/descendant containment.** The instruction
  walk accepts only ancestors or descendants of the workspace root;
  unrelated paths are rejected with a `Warning` diagnostic.
- **No new imports of forbidden crates.** `scripts/check-core-boundary.sh`
  passes; the new code only imports `serde`, `serde_json`,
  `chrono`, `sha2`, `hex`, `tempfile`, and `std::*`. None of
  `ratatui`, `axum`, `wasmtime`, `crypto` are imported.

## 9. Documentation and operations

Updated:

- `architecture/agent.md` — new section `AssetContext and
  ProjectAssetSnapshot (Runtime Assets Milestone 2)` documenting
  the new modules, primary constructors, deprecated surfaces, and
  the static guard.
- `architecture/overview.md` — new module row for `AssetContext /
  Snapshot`.
- `AGENTS.md` — registers
  `python3 scripts/check_project_agent_pwd_inference.py` in the
  static-guards block.
- `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md` — status will move to `implemented (closure evidence:
  plans/closure/runtime-assets/002-status.md)` after the
  implementation commit lands.

Operators can:

- Build a snapshot for an explicit context:
  ```rust
  let ctx = AssetContextBuilder::new()
      .with_synthetic_project_id(ProjectId::new())
      .with_workspace_root("/path/to/project")
      .build()?;
  let snapshot = ProjectAssetSnapshotBuilder::with_default_config_doc(config.into())
      .build(&ctx)?;
  ```
- Inspect agents: `snapshot.get_agent(name)`,
  `snapshot.agents()`, `snapshot.agent_count()`.
- Inspect instructions: `snapshot.instruction_fragments()`,
  `snapshot.instruction_block()`.
- Inspect skills: `snapshot.build_skill_prompt()`,
  `snapshot.skills.effective`.
- Inspect diagnostics: `snapshot.agent_diagnostics`,
  `snapshot.instruction_diagnostics`.
- Verify fingerprint stability: `snapshot.fingerprint` is a SHA-256
  over sorted semantic fields only.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `clippy::large_enum_variant` warning on `SessionSelectionDto` in `crates/codegg-protocol/src/provider.rs:170` is pre-existing on `main` and unrelated to this milestone. | No impact on the agent-resolution surface. | Track under the existing provider-connections follow-up. |
| low | The deprecated `load_agent_prompt`, `load_agent_prompt_async`, `find_instructions_file`, and `find_all_instruction_files` functions remain in `src/agent/prompt.rs` for backward compatibility. | Callers that still use them rely on process-global cwd. | Migrate remaining callers (if any) in follow-up work; the active callers in `src/main.rs` and `src/agent/turn_runtime.rs` are already on the context-aware path. |
| low | Publication generation is not yet assigned. The snapshot is immutable after construction but has no `generation` field; concurrent rebuilds for the same context may duplicate work. | Future refresh coordinators will need a generation seam. | Milestone 3 will own this. |
| low | The CLI-bootstrap reads of cwd at the legacy `resolve_agents(&Config)` boundary and at `src/tool/skill.rs::execute` remain in production code, gated by the static guard's narrow allowlist. | Both reads are documented as the sole CLI bootstrap for their respective surfaces. | Keep the allowlist narrow; remove the deprecated surfaces in a follow-up cleanup once all callers migrate. |

No critical or high-severity finding remains for this milestone.

## 11. Roadmap disposition

Milestone closed and the next hard dependency is unlocked. Milestone 3
— refresh lifecycle and operator surface — has a hard dependency on
Milestone 2 closure and may now proceed. The unified
`ProjectAssetSnapshot` produced here is the substrate that Milestone
3's refresh coordinator will own: it already carries the
`build_metadata`, per-asset digests, and combined fingerprint that a
generation-aware refresh seam will need.

Multi-Project TUI 001 and Session Projections 001 remain blocked on
the catalog protocol surface and project-aware TUI state, neither of
which is changed by this milestone.

## 12. Registry updates

- `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md` — status moves to `implemented (closure evidence:
  plans/closure/runtime-assets/002-status.md)`.
- `plans/subsystems/runtime-assets-roadmap.md` — Milestone 2 marked closed; Milestone 3 unblocked.
- `plans/registry.md` — Runtime Assets 002 moved from "Dependency-ready
  implementation plans" to "Recently closed work"; the Runtime Assets
  row in "Active subsystem roadmaps" is updated to point at
  Milestone 3 (refresh lifecycle and operator surface).