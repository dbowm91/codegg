# Runtime Assets Milestone 001 — Project Asset Registry and Portable Skill Discovery

Status: ready for handoff

Repository baseline: `fbae374a2cd6172505204b1bc1bee1ef247afd5f` (production-code baseline; subsequent planning-only commits do not alter implementation state)

Source roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-1--source-aware-registry-and-portable-skill-discovery`

Long-term requirements:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`

Applicable ADRs:

- None. The implementing agent must not change the canonical precedence or refresh invariants without an accepted ADR and explicit long-term revision.

Primary class: infrastructure

## 1. Objective

Replace the current mutable, CodeGG-only skill index with a project/workspace-scoped, source-aware asset registry that discovers portable `SKILL.md` packages from CodeGG and compatible harness locations, validates them safely, records provenance/digests/diagnostics, and resolves duplicates deterministically.

This milestone establishes discovery and registry correctness only. Session lifecycle refresh, manual commands, agent loader migration, and turn snapshot pinning are later milestones.

## 2. Why this milestone is ready

Hard dependency:

- `plans/implementation/domain-identity/001-typed-identity-foundation.md` is
  closed, and Domain Identity Milestone 002 is implemented at `84d92f0`.
  `ProjectStorage` plus `WorkspaceBindingRecord` provide the authoritative
  explicit project/workspace context interface required for asset-service
  construction.

The plan may be reviewed now, but production implementation must not begin by inventing temporary path-derived project identity or reading `PWD`.

## 3. Current implementation evidence

- `src/skills/mod.rs` defines `Skill` and `SkillIndex`, stores a mutable `Vec<Skill>`, clears/reloads it on `load(project_dir)`, and searches only:
  - platform config `codegg/skills`;
  - project `.codegg/skills`.
- It accepts direct `.md` files and immediate child directories containing `SKILL.md`.
- The current parser recognizes CodeGG frontmatter fields `name`, `description`, `version`, and `tags`, reads the entire file, and stores the body/source path.
- There is no source-kind model, precedence policy, duplicate diagnostic, content digest, generation, candidate snapshot, bounded file/resource policy, or foreign harness discovery.
- `architecture/skills.md` documents only CodeGG global and project locations.
- `src/agent/registry.rs` already demonstrates provenance and diagnostic concepts that should inform, but not be blindly coupled to, the asset registry.

## 4. Invariants that must not regress

- Discovery is anchored to explicit project/workspace context.
- Foreign harness directories remain read-only.
- Discovery and activation execute no scripts.
- Invalid or malicious files cannot escape the project/skill boundary through symlinks or resource paths.
- Existing `.codegg/skills/*.md` and `.codegg/skills/<name>/SKILL.md` behavior remains available as compatibility.
- Duplicate resolution is deterministic and source-aware.
- Invalid higher-precedence content does not silently erase a valid lower-precedence skill.
- Asset metadata and resources are bounded.

## 5. Scope

### In scope

- Core asset domain types: asset kind, source kind, provenance, digest/fingerprint, diagnostic, shadowing, validation state.
- A project/workspace-scoped registry builder that accepts explicit roots and source configuration.
- Project discovery for:
  - `.codegg/skills/<name>/SKILL.md`;
  - `.agents/skills/<name>/SKILL.md`;
  - `.opencode/skills/<name>/SKILL.md`;
  - `.claude/skills/<name>/SKILL.md`;
  - direct `.codegg/skills/*.md` compatibility files.
- Global discovery for CodeGG, `.agents`, OpenCode, and Claude-compatible locations.
- Portable Agent Skills frontmatter support:
  - required `name` and `description`;
  - optional `license`, `compatibility`, `metadata`, and experimental `allowed-tools` preserved safely;
  - CodeGG-native optional extensions preserved under namespaced metadata.
- Deterministic precedence, duplicate/shadow diagnostics, fallback behavior, and source enable/disable configuration seam.
- Metadata-first resource inventory with bounded counts/sizes and path validation.
- Unit/integration/security tests and documentation.

### Explicitly out of scope

- Refresh triggers, `/reload`, watchers, and snapshot generations.
- Refactoring `AgentRegistry` away from `PWD`.
- Injecting skill bodies into active turns.
- Executing scripts or validating their runtime dependencies.
- Distributed node manifests or asset blob transfer.
- Team authorization.
- Writing skills into foreign harness directories.

## 6. Required production changes

### Core/domain

Introduce a source-aware asset model in a dependency-appropriate module. The effective skill representation should retain:

- normalized logical name;
- description and portable metadata;
- source kind and canonical source path;
- package root;
- content digest;
- modification fingerprint where useful;
- validation diagnostics;
- precedence rank;
- shadowed alternatives;
- resource descriptors without eagerly loading resource bodies.

Keep parsing separate from precedence resolution. Build candidates first, validate them, then select effective entries.

### Storage and migrations

No database migration is required. The registry is reconstructible from filesystem/config state. Content digests must be stable enough for later generation/audit seams.

### Protocol and DTOs

No public protocol change is required. Internal serializable summaries may be introduced if needed for future refresh reports, but do not expose unfinished APIs.

### Runtime and concurrency

Discovery should be async or executed off the critical event loop where appropriate. Apply bounded directory/file concurrency. Avoid global singleton state. The result should be immutable after construction.

### Frontend or operator surface

No command is required. Tests or developer diagnostics should allow inspection of discovered/effective/shadowed/invalid assets.

### Security and authorization

- Canonicalize source/package roots safely.
- Reject or contain symlink escape.
- Bound skill file size, frontmatter size, number of skills, resources per skill, and resource metadata reads.
- Reject path traversal and recursive self-reference.
- Never execute or source scripts.
- Preserve `allowed-tools` as metadata only; it cannot expand CodeGG permissions.

### Documentation and static guards

Update skills architecture and config examples. Document all locations, precedence, portable schema, compatibility behavior, and foreign-directory read-only policy.

## 7. Ordered work packages

### Work package A — Asset contracts and source configuration

Intent: establish explicit source/provenance types before changing discovery.

Required changes:

- define asset/source/diagnostic/digest types;
- define deterministic source priority and configuration override seam;
- define parser and resolver interfaces;
- define bounds in one configuration structure with safe defaults.

Acceptance evidence:

- source-priority table tests;
- serde/debug behavior does not expose full bodies unnecessarily;
- no dependency on cwd or global project state.

### Work package B — Portable parser and package validation

Intent: parse portable `SKILL.md` packages while preserving CodeGG compatibility.

Required changes:

- parse required/optional portable frontmatter;
- retain unknown metadata safely;
- support native direct Markdown compatibility;
- calculate normalized content digest;
- inventory optional package resources without reading/executing them;
- produce diagnostics instead of aborting the entire registry for one invalid skill.

Acceptance evidence:

- valid fixtures for each location/schema variant;
- malformed YAML, missing required fields, oversized files, and invalid names are isolated and diagnosed.

### Work package C — Discovery and boundary enforcement

Intent: discover all required global/project locations safely.

Required changes:

- explicit root walk bounded by project/worktree boundary;
- global source resolution by platform;
- source-specific read-only semantics;
- symlink and resource boundary checks;
- deterministic ordering independent of filesystem enumeration order.

Acceptance evidence:

- all required locations covered;
- nested directory/worktree fixtures remain within boundary;
- foreign directories are unchanged after tests.

### Work package D — Resolution, fallback, and compatibility adapter

Intent: select effective skills with inspectable conflict behavior.

Required changes:

- apply precedence after validation;
- retain shadowed candidates/provenance;
- fall back to lower-precedence valid candidate when higher candidate is invalid;
- provide compatibility adapter for current `SkillIndex` consumers without preserving mutable global semantics;
- add developer inspection helpers.

Acceptance evidence:

- duplicate matrix tests;
- existing skill activation tests remain compatible through the adapter;
- deterministic results across repeated scans.

## 8. Failure, cancellation, restart, and contention semantics

- One invalid skill produces a diagnostic and does not fail unrelated discovery.
- Fatal root-level I/O/config failures return typed errors; optional absent directories are not errors.
- Cancellation before registry completion publishes nothing; this milestone does not manage publication yet.
- Repeated construction from unchanged files yields identical effective entries/digests.
- Concurrent builders for separate projects share no mutable project state.
- Concurrent builders for the same project may duplicate work in this milestone, but results must be deterministic; coalescing belongs to refresh coordination.

## 9. Compatibility and migration

- Preserve existing native global and `.codegg/skills` behavior.
- Preserve direct `.md` support only for CodeGG-native sources.
- Do not require users to copy portable skills into `.codegg`.
- Existing `/skill:<name>` consumers may use a compatibility facade; body/resource activation semantics must remain explicit.
- Document any stricter name/frontmatter validation and provide diagnostics rather than silent disappearance.

## 10. Required tests

### Focused unit tests

- portable frontmatter fields and unknown metadata;
- native compatibility frontmatter;
- normalized digest stability;
- name/description validation;
- deterministic source ordering and duplicate resolution;
- invalid higher-precedence fallback.

### Integration tests

- every required project/global location;
- several sources in one repository;
- nested workspace/worktree boundary;
- existing `tests/skills.rs` compatibility.

### Restart and recovery tests

- repeated reconstruct produces equivalent registry and digests.

### Contention and cancellation tests

- concurrent independent project scans do not cross-contaminate;
- bounded concurrency is respected under many skill packages.

### Security and negative tests

- symlink escape;
- resource path traversal;
- recursive/self resource references;
- oversized skill/frontmatter/resource inventory;
- malformed YAML/UTF-8;
- script files are inventoried only and never executed;
- `allowed-tools` cannot change permission state.

### Migration and compatibility tests

- direct `.codegg/skills/*.md` loads;
- current package layout loads;
- absent foreign directories are harmless;
- duplicate behavior is documented and stable.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test --test skills
cargo test skills
cargo test agent::registry
cargo test -p codegg-core
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add and run a focused new integration target for asset discovery/security rather than relying only on generic module filters.

## 12. Documentation updates

- `architecture/skills.md`;
- agent/skill interoperability section in architecture/config or a new runtime-assets document;
- example directory layouts and precedence table;
- security notes for scripts/resources and foreign read-only directories;
- compatibility note for direct CodeGG Markdown skills.

## 13. Acceptance criteria

- A caller with explicit project/workspace roots receives an immutable source-aware registry.
- All required portable locations are discovered.
- Duplicate and invalid-source behavior is deterministic and inspectable.
- Existing CodeGG-native skill layouts remain compatible.
- Foreign skill packages are never modified or executed.
- Security bounds and path checks have direct negative-test evidence.
- No `PWD` or process-global project inference is introduced.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- authoritative project/workspace identity is not available;
- portable skill format requires executing code during discovery;
- supporting a foreign agent format becomes necessary to complete skill discovery;
- precedence conflicts with the canonical long-term specification;
- existing `SkillIndex` consumers require in-flight mutation or global singleton publication;
- the work expands into session refresh, turn pinning, authorization, or distributed transport.

## 15. Closure evidence required

- implementation commit(s);
- discovery-location and precedence matrix;
- security-bound configuration and tests;
- exact verification commands/results;
- compatibility evidence for current CodeGG skills;
- proof foreign directories remain unchanged and scripts unexecuted;
- list of deferred refresh/agent/snapshot integration seams;
- closure recommendation.

## 16. Handoff notes

- This plan remains blocked until the domain-identity/project-context dependency is closed.
- Do not solve the blocker by passing raw path strings everywhere or reading `PWD`.
- Metadata-first progressive disclosure is important for large skill libraries.
- Preserve unrelated agent-registry behavior; its explicit-context migration is the next subsystem milestone.
