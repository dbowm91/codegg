# Runtime Assets Milestone 002 — Explicit-Context Agent and Instruction Resolution

Status: implemented (closure evidence: `plans/closure/runtime-assets/002-status.md`)

Repository baseline: `3ce0a7ea7c1a8baa41a2618eb293291435e9f9f0` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `84d92f0` — canonical project/workspace storage and explicit binding interfaces.
- `a2db5e4` — durable project catalog service.
- `f9db5c3` — source-aware project asset registry and portable skill discovery.

Source roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-2--explicit-context-agent-and-instruction-resolution`

Long-term requirements:

- `plans/000-long-term-specification.md#12-skills-agents-and-cross-harness-interoperability`
- `plans/001-terminology-and-domain-model.md` — AgentDefinition, Skill, ProjectInstructions, ProjectAssetSnapshot, source, digest, generation, and shadowing.
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`

Applicable closure evidence:

- `plans/closure/runtime-assets/001-status.md`
- `plans/closure/domain-identity/002-status.md`
- `plans/closure/project-catalog/001-status.md`

Applicable ADRs:

- None. The canonical specification already requires explicit project/workspace context and one versioned project asset snapshot. Stop for an ADR only if implementation would alter the established built-in/config/project agent overlay semantics or introduce foreign-agent formats as authoritative.

Primary class: invariant

## 1. Objective

Remove `PWD` and process-global directory inference from project agent resolution, centralize agents, skills, and project instructions into one deterministic project/workspace-scoped snapshot, and expose source provenance plus content digests for every effective runtime asset.

The milestone succeeds when two concurrently active projects can resolve different agents, skills, and instructions without cross-contamination; all daemon-owned consumers receive assets from an explicit context; and existing CodeGG built-in/global/project/config overlay behavior remains compatible.

This milestone builds immutable snapshot content and migrates current consumers. It does not implement lifecycle-triggered refresh, manual `/reload` commands, generation publication, file watchers, or in-flight turn pinning; those remain Milestones 003–004.

## 2. Why this milestone is ready

The hard dependency is closed:

- Runtime Assets Milestone 001 provides deterministic, bounded, source-aware skill discovery and an immutable `AssetRegistry`.

A stable explicit-context interface also exists:

- Domain Identity Milestone 002 provides canonical `ProjectId + WorkspaceId` bindings and workspace roots through `ProjectStorage`/workspace records.
- Project Catalog Milestone 001 provides project lifecycle and project/workspace lookup without activation or scanning.

The work can therefore remove `PWD` from `AgentRegistry` without inventing a temporary project identity mechanism.

## 3. Current implementation evidence

At the repository baseline:

- `src/skills/` contains `AssetRegistry`, `AssetDiscoveryConfig`, source kinds, parser, diagnostics, effective-skill digests, shadowed alternatives, bounded resource inventory, and `SkillIndexCompat`.
- The legacy `SkillIndex` remains in `src/skills/mod.rs`; direct consumers still exist and do not see all foreign harness skill locations.
- `src/agent/registry.rs::AgentRegistry::load(&Config)` resolves compiled built-ins, global CodeGG agent files, project `.codegg/agents/*.toml`, config-agent overlays, and config modes.
- Project agent lookup currently reads `std::env::var("PWD")` and joins `.codegg/agents`, making project selection process-global and unsafe for multiple active projects.
- `AgentSource`, `AgentSourceKind`, `ResolvedAgent`, and `AgentDiagnostic` already preserve partial provenance, but effective agent definitions have no stable content digest and are not part of a unified project snapshot.
- Existing built-in/global/project/config overlay semantics are well tested and must remain behaviorally compatible.
- Runtime Assets Milestone 001 intentionally left `AssetKind`, project instructions, agent integration, protocol, refresh, and generation for later work.
- The canonical long-term specification requires one `ProjectAssetSnapshot` containing effective agents, skills, project instructions, provenance, content digests, diagnostics, and a future monotonically increasing generation.

## 4. Invariants that must not regress

- Daemon-owned agent, skill, and instruction resolution uses explicit project/workspace context only.
- `PWD`, `current_dir`, frontend cwd, and server-global project roots cannot select project assets.
- Compiled built-ins remain available unless explicitly disabled through the existing overlay policy.
- Existing CodeGG global/project/config agent overlay and replace/disable semantics remain deterministic.
- Skills retain Milestone 001 precedence, bounds, diagnostics, and foreign-directory read-only behavior.
- Unknown foreign metadata cannot grant permissions or tool authority.
- Snapshot content is immutable after construction.
- Asset digests must be deterministic and must not contain secrets or unstable absolute-path data unless the path is separately represented as provenance.
- Two project/workspace contexts cannot share mutable project-local registry state.

## 5. Scope

### In scope

- An explicit `ProjectAssetContext` or equivalent carrying stable project/workspace IDs, workspace root, configured global roots, and relevant config snapshot/revision.
- Explicit-context `AgentRegistry` construction.
- One immutable `ProjectAssetSnapshot` content model covering agents, skills, and project instructions.
- Deterministic agent-definition digests and combined snapshot fingerprint.
- Agent, skill, and instruction provenance plus diagnostics.
- Centralized project-instruction loading for currently supported CodeGG instruction sources.
- Migration of daemon/runtime consumers away from direct `AgentRegistry::load` + legacy `SkillIndex` state.
- Compatibility wrappers for non-daemon embedding/tests where necessary.
- Two-project isolation, deterministic-build, security, and compatibility tests.
- Static guards against project-agent `PWD` inference.

### Explicitly out of scope

- Refresh triggers on project/session lifecycle.
- Manual `/reload`, `/skills refresh`, or `/agents refresh` commands.
- Protocol refresh requests/events or TUI generation views.
- Monotonic publication generation ownership; Milestone 003 owns publication and refresh coordination.
- In-flight turn snapshot pinning and audit recording; Milestone 004 owns those semantics.
- File watchers.
- Distributed node manifests.
- Executing skill scripts or instruction-referenced commands.
- Broad foreign-agent format compatibility. Foreign agent formats are not standardized and require separate review.
- Team authorization.

## 6. Required production changes

### Core asset context

Define a typed context value constructed from authoritative daemon/project services, containing at least:

- `ProjectId`;
- `WorkspaceId`;
- canonical workspace root/locator supplied by the workspace registry;
- project lifecycle/binding revision or equivalent staleness token;
- explicit global asset roots;
- effective CodeGG config snapshot/revision;
- optional session-level overrides supplied explicitly by the caller.

The context must be clonable and immutable. It must not query `PWD` or `current_dir` during construction or use a path as project identity.

### Unified snapshot model

Introduce a project/workspace-scoped immutable snapshot content type, for example `ProjectAssetSnapshot` or `ProjectAssetSnapshotContent`, with at least:

- stable `project_id` and `workspace_id`;
- effective agents indexed by normalized name;
- effective skills through `Arc<AssetRegistry>` or an equivalent immutable view;
- ordered project-instruction fragments plus effective merged instruction text;
- source manifest/provenance for every asset;
- deterministic content digest for every effective agent, skill, and instruction fragment;
- combined snapshot fingerprint/digest;
- diagnostics partitioned by agent/skill/instruction and severity;
- shadowed/disabled alternatives where supported;
- build timestamp/duration only as non-digest metadata;
- no publication generation yet, or an explicit unassigned generation state that Milestone 003 will own.

The combined fingerprint must be derived from normalized semantic content and stable source kinds/order, not wall-clock time, map iteration order, or absolute-path spelling.

### Agent registry

Refactor `AgentRegistry` so the primary constructor requires explicit context, for example:

```text
AgentRegistry::load_for_context(config, asset_context)
```

Required behavior:

- compiled built-ins load first;
- global CodeGG agent files load from explicit configured roots;
- project CodeGG agent files load from `asset_context.workspace_root/.codegg/agents`;
- existing overlay/replace/disable semantics remain unchanged;
- config agents/modes and explicit session/project overrides preserve current precedence;
- all sources retain source kind/path/name and validation diagnostics;
- each effective agent receives a deterministic digest over normalized effective fields, including prompt, model/fallback, variant, sampling settings, runtime kind, permissions, hidden/steps, and other behavior-affecting options;
- source paths are provenance fields but do not destabilize semantic digests;
- prompt files are resolved under explicit allowed roots with traversal/symlink/size bounds;
- invalid higher-precedence project definitions must follow the existing documented agent policy; do not silently broaden behavior beyond current semantics.

Keep `AgentRegistry::load(&Config)` only as a clearly marked compatibility API if required. It must not remain in protected daemon/runtime paths. Prefer a compatibility constructor that requires a caller-supplied project root instead of reading `PWD`.

### Skills integration

- Replace new daemon/runtime use of legacy `SkillIndex` with `AssetRegistry`/`SkillIndexCompat` backed by explicit context.
- Preserve direct Markdown CodeGG compatibility and all foreign harness skill locations from Milestone 001.
- Avoid duplicate scanning within one snapshot build.
- Keep large skill resources metadata-only and scripts inert.
- Include effective skill digests and shadowed alternatives in the unified snapshot manifest.

### Project instructions

Centralize project-instruction loading into one bounded resolver.

Minimum requirements:

- inventory and preserve the repository's currently supported CodeGG instruction sources, including the root `AGENTS.md` convention and any existing CodeGG-native instruction/config source;
- walk or scope instruction lookup only within the declared project/workspace boundary;
- define deterministic nearest/root ordering and duplicate behavior;
- represent each instruction fragment with source kind, source path, digest, diagnostics, and precedence/order;
- bound file count, individual file size, total merged bytes, nesting depth, and UTF-8 parsing;
- reject symlink/path escape;
- never execute referenced scripts or commands;
- do not automatically import `CLAUDE.md`, OpenCode instruction files, or another foreign instruction convention unless an existing supported adapter is found and explicitly documented during implementation;
- merge instructions deterministically into the snapshot while retaining original fragments for inspection.

If existing instruction behavior is materially ambiguous, preserve the narrowest current behavior and document it rather than inventing a new precedence policy.

### Snapshot builder/service seam

Add one builder/service responsible for:

1. validating explicit context;
2. building skills once;
3. resolving agents once;
4. loading instructions once;
5. collecting diagnostics and source manifests;
6. computing stable digests;
7. returning an immutable snapshot candidate.

Milestone 003 will own refresh coalescing and atomic publication. This milestone's builder must therefore be side-effect-free except bounded filesystem reads and must be safe to call concurrently for different project/workspace contexts.

### Runtime consumer migration

Review and migrate the current consumers that assemble agent/skill/instruction state, including:

- root application/session initialization;
- daemon turn-runtime construction;
- agent selection/listing and subagent spawn inputs;
- skill tool activation and prompt assembly;
- system-prompt/project-instruction assembly;
- tests and embedding constructors.

All daemon-owned consumers should accept a snapshot or explicit asset context rather than independently loading files.

Do not implement refresh/pinning semantics yet. A consumer may hold one immutable snapshot for its current lifetime, with the lifecycle coordinator added in Milestone 003.

### Security and authorization

- Preserve all Milestone 001 path, symlink, size, and resource bounds.
- Add equivalent bounds to agent files, prompt files, and instruction files.
- Treat permissions in an agent definition as configuration subject to existing authority intersection; asset metadata alone cannot widen runtime authority.
- Ensure digest/debug/diagnostic output excludes secrets and bounded prompt/instruction content where full text is unnecessary.
- Foreign harness roots remain read-only.

### Documentation and static guards

Update at least:

- `architecture/skills.md`;
- `architecture/agents.md` or current agent architecture document;
- `architecture/session.md`;
- `architecture/core.md`;
- `architecture/config.md`;
- project-instruction documentation;
- runtime/turn prompt-assembly documentation.

Add or extend static checks to reject:

- `std::env::var("PWD")` and `current_dir()` in project agent/instruction resolution;
- direct project-local agent path construction outside the snapshot builder;
- new daemon/runtime construction of legacy `SkillIndex`;
- project asset loading without explicit project/workspace context.

## 7. Ordered work packages

### Work package A — Explicit asset context

Intent: establish the sole input contract for project asset resolution.

Required changes:

- define typed context and validation;
- integrate project/workspace binding/root lookup;
- represent global roots and config revision explicitly;
- remove process-global inference from the primary path.

Acceptance evidence:

- context creation rejects mismatched/unbound project/workspace pairs;
- no protected asset path reads `PWD` or `current_dir`;
- two contexts can coexist in one process.

### Work package B — Context-aware agent registry

Intent: preserve behavior while eliminating `PWD`.

Required changes:

- add explicit-context constructor;
- preserve built-in/global/project/config/session overlay order;
- add stable effective-agent digests;
- bound prompt-file loading and retain provenance/diagnostics;
- keep compatibility wrapper outside daemon authority.

Acceptance evidence:

- existing agent registry tests pass unchanged or with additive context fixtures;
- two project roots with same-named agent overlays resolve independently;
- repeated builds produce identical digests.

### Work package C — Project instruction resolver

Intent: make instructions a first-class, inspectable asset.

Required changes:

- inventory current instruction sources;
- implement bounded deterministic loading/merging;
- add source/digest/diagnostic records;
- enforce project-boundary and symlink containment.

Acceptance evidence:

- nearest/root ordering is explicit and tested;
- malformed/oversized/escaping files produce diagnostics without contaminating other projects;
- unchanged instructions produce identical digests.

### Work package D — Unified snapshot builder

Intent: produce one immutable candidate containing all runtime assets.

Required changes:

- combine agents, skills, and instructions;
- compute stable combined fingerprint;
- retain per-kind manifests and diagnostics;
- avoid duplicate filesystem scans where practical;
- keep publication generation unassigned for Milestone 003.

Acceptance evidence:

- snapshot equality/fingerprint stability across unchanged builds;
- changed agent/skill/instruction changes only the expected digest/fingerprint;
- no mutable project-local state is shared.

### Work package E — Runtime consumer migration

Intent: stop independent asset loading in daemon/runtime paths.

Required changes:

- migrate prompt, agent, subagent, and skill consumers;
- remove daemon use of direct `AgentRegistry::load` and legacy `SkillIndex`;
- pass snapshot/context through constructors explicitly;
- preserve existing CLI/embedding compatibility through narrow adapters.

Acceptance evidence:

- turn and subagent construction use the same snapshot candidate;
- skill activation sees foreign harness skills through the new registry;
- project A cannot select project B's agents or instructions.

### Work package F — Guards, docs, and closure evidence

Intent: prevent regression and prepare Milestone 003.

Required changes:

- add static guards and negative fixtures;
- document snapshot ownership and future generation/publication seam;
- enumerate remaining compatibility APIs and removal prerequisites.

Acceptance evidence:

- guards pass production code and fail deliberate `PWD`/direct-loader fixtures;
- docs show exact source precedence and digest semantics.

## 8. Failure, cancellation, restart, and contention semantics

- Missing project/global asset directories are empty sources, not fatal errors.
- Invalid individual assets produce bounded diagnostics and follow their existing per-kind fallback policy.
- Fatal context/config/root errors fail snapshot construction and return diagnostics; no partial snapshot is published by this milestone.
- Concurrent builds for different project/workspace contexts must not share mutable registries or overwrite one another.
- Concurrent builds for the same context may duplicate work in this milestone; Milestone 003 will coalesce refresh. Results must nevertheless be deterministic.
- Cancellation before builder completion returns no candidate and leaves the caller's prior state unchanged.
- Daemon restart reconstructs identical snapshot content from unchanged files/config; publication generation is assigned later.
- Prompt/instruction/agent file changes during a scan must resolve through a documented consistency policy: either read-one-pass with metadata/digest verification or fail the affected candidate as changed-during-read. Do not produce a digest for content different from the returned body.

## 9. Compatibility and migration

- Existing built-in agents and config overlays remain available.
- Existing `.codegg/agents/*.toml` semantics remain compatible.
- Existing skill layouts and legacy direct Markdown support remain available.
- `AgentRegistry::load(&Config)` may remain only as a deprecated/testing compatibility path; protected daemon code must not call it.
- Legacy `SkillIndex` remains for compatibility but daemon/runtime consumers migrate to `AssetRegistry` or the unified snapshot.
- Do not silently enable foreign agent/instruction formats.
- No SQLite migration is expected unless durable digest metadata is proven necessary before Milestone 003; publication generation and session initialization metadata remain later work.
- No protocol change is required in this milestone.

## 10. Required tests

### Focused unit tests

- explicit asset-context validation.
- agent precedence/replace/disable compatibility.
- deterministic agent digest normalization.
- prompt-file size/path/symlink bounds.
- instruction ordering, size bounds, malformed UTF-8/frontmatter behavior, and digest stability.
- combined snapshot fingerprint stability.

### Integration tests

- two concurrent projects with same-named agents/skills/instructions remain isolated.
- project/global agent overlays match pre-milestone behavior.
- root application and daemon turn runtime consume the unified snapshot.
- skill tool sees `.agents`, `.opencode`, and `.claude` portable skills through the migrated path.
- subagent construction receives the same effective agent definition/digest as root selection.

### Restart and recovery tests

- unchanged files/config reconstruct identical semantic digests after restart.
- missing or temporarily unreadable source returns deterministic diagnostics.
- changed-during-read does not produce mismatched content/digest.

### Contention and cancellation tests

- concurrent builds for two workspaces do not cross-contaminate.
- cancellation returns no partially visible snapshot.
- repeated concurrent builds for one context produce equal fingerprints.

### Security and negative tests

- `PWD`/`current_dir` cannot affect effective project agents.
- symlink/path escape for agents, prompt files, and instructions is rejected.
- oversized aggregate instruction content is bounded.
- `allowed-tools`/foreign metadata cannot widen permission authority.
- foreign harness directories remain unmodified.

### Compatibility tests

- existing `agent::registry` suite passes.
- existing `tests/skills.rs` and `tests/skills_registry.rs` pass.
- compatibility constructors are explicitly tested and isolated from daemon paths.

## 11. Required verification commands

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test agent::registry
rtk cargo test --test skills
rtk cargo test --test skills_registry
rtk cargo test skills
rtk cargo test -p codegg-core
rtk cargo test --lib agent::
rtk cargo test --lib skills::
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk git diff --check
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

Known unrelated/pre-existing verification findings from the prior closure must be reported honestly if they recur: the `SessionSelectionDto` size warning and the flaky Eggpool provisioning timing test.

## 12. Documentation updates

- Document the explicit asset-context type and construction authority.
- Document the unified snapshot fields, digest inputs, and immutability.
- Document exact agent overlay order and preserved compatibility behavior.
- Document project-instruction sources, precedence/order, and bounds.
- Mark legacy `AgentRegistry::load` and `SkillIndex` paths as compatibility-only.
- Document the interface Milestone 003 will use to assign generations and publish refreshes.

## 13. Acceptance criteria

- No protected daemon/runtime project-agent or instruction path reads `PWD` or `current_dir`.
- One immutable snapshot candidate contains effective agents, skills, and project instructions.
- Every effective agent/instruction/skill has deterministic digest and provenance.
- Existing CodeGG agent overlay semantics remain compatible.
- Foreign harness portable skills remain available through migrated runtime consumers.
- Two active project/workspace contexts cannot leak assets into one another.
- Snapshot reconstruction over unchanged state yields the same semantic fingerprint.
- Agent/prompt/instruction path and size bounds prevent escape or unbounded reads.
- No refresh, watcher, protocol, or in-flight pinning behavior is falsely claimed.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- project/workspace context cannot be resolved through the closed identity interfaces;
- preserving current agent overlay behavior requires a material precedence-policy change;
- implementation requires adopting an undocumented foreign agent/instruction format;
- a path or process cwd would become project identity or asset-selection authority;
- permission semantics would be widened by asset metadata;
- work expands into refresh commands/protocol, watcher behavior, distributed manifests, or in-flight turn pinning;
- a durable schema is proposed without a demonstrated requirement before Milestone 003.

## 15. Closure evidence required

The closure record must contain:

- exact implementation commit(s);
- explicit-context type and construction call graph;
- before/after review of `AgentRegistry` `PWD` behavior;
- source/precedence compatibility matrix for agents, skills, and instructions;
- digest normalization specification and stability evidence;
- two-project isolation evidence;
- consumer migration inventory, including remaining compatibility call sites;
- security-bound tests and static-guard output;
- full verification command log and known unrelated failures;
- interface handed to Runtime Assets Milestone 003.

## 16. Handoff notes

- Treat `3ce0a7e` as the reviewed production baseline; inspect current `main` before editing.
- Domain Identity 003 may land in parallel. Consume the stable project/workspace storage interface and avoid depending on unfinished protocol details.
- Preserve the repository's current resource-conscious test configuration.
- Do not rewrite foreign harness directories.
- Do not execute skill resources, prompt-referenced scripts, or instruction-referenced commands during resolution.
- Keep publication generation ownership cleanly deferred to Milestone 003.
