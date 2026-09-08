# Post-Audit Maintainability and Surface Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md` remains authoritative where tool-program execution is involved.
- No new ADR is required for this roadmap because it preserves the existing daemon, scheduler, broker, provider, search, Git, and tool-program ownership boundaries. If implementation discovers that one of those canonical owners must change, the affected milestone must stop and request an ADR rather than improvising.

## 1. Purpose and ownership boundary

This roadmap converts the September 2026 repository audit into bounded maintainability work after the major architecture-convergence campaigns have substantially closed.

It owns five related cleanup boundaries:

1. compatibility paths that duplicate a canonical implementation or model-facing name;
2. the size and ambiguity of the model-visible default tool surface;
3. physical decomposition of the remaining oversized agent-runtime source modules without creating a new orchestration layer;
4. physical decomposition of the oversized Bash tool without changing process-execution authority;
5. removal of mutable process-global service state where it currently impairs test isolation or daemon/session composition.

This workstream does not re-litigate already accepted subsystem ownership. Its purpose is to make the existing architecture easier to reason about, test, and evolve.

## 2. Work classification

### Invariants

- Every operation continues to have one canonical implementation owner.
- Compatibility adapters MUST delegate to the canonical owner; they MUST NOT accumulate independent behavior.
- Removing an alias MUST NOT silently change permission, risk, tool-contract, or serialized-protocol semantics.
- Model-facing tool minimization MUST preserve discoverability through the existing profile/policy/catalog/tool-search machinery.
- Agent-loop decomposition MUST preserve turn lifecycle, cancellation, scheduler ownership, context/compaction semantics, run attribution, tool brokerage, and projection ordering.
- Bash decomposition MUST preserve sandbox, command-intent, scheduler, child-Git, output-bound, cancellation, process-tree, and permission semantics.
- Runtime service state MUST be explicit enough that two independently constructed test/runtime contexts do not overwrite each other's configuration.

### Capabilities

- Models receive a smaller, less ambiguous default tool palette while specialist capabilities remain callable/discoverable.
- Developers can identify the canonical name and owner for each compatibility path.
- Runtime services can be composed and tested without mutable process-wide slot replacement.

### Infrastructure

- A compatibility inventory with explicit retain/remove criteria.
- Existing `ToolRegistryOptions`, model profiles, catalog/broker, and `tool_search` used as the disclosure mechanism rather than a new router.
- Smaller agent-runtime modules grouped by existing responsibility boundaries.
- Smaller Bash modules grouped by policy/classification, supervised execution, and output/result concerns.
- An explicit runtime-services/context seam for search/MCP state and any closely related mutable globals proven by the milestone inventory.

### Polish

- Removal of deprecated re-exports, dead modules, stale aliases, obsolete comments, duplicated docs, and tests that only preserve unnecessary compatibility.
- Architecture docs updated to describe only the surviving canonical surfaces.

## 3. Non-goals

- Another daemon rewrite or coordinator abstraction.
- Replacing the existing `ToolBroker`, `ToolRegistry`, model-profile system, or `tool_search`.
- General provider HTTP-client unification.
- Rewriting Git ownership or worktree orchestration.
- Replacing eggsearch or introducing persistent search indexing.
- Changing ACP/native protocol ownership.
- Removing compatibility solely to reduce line count when there is evidence of a real external consumer.
- Splitting code into tiny files with no stable responsibility boundary.
- Introducing dependency-injection frameworks or service-locator crates.
- New CI lanes, coverage gates, benchmark gates, or binary-size gates.

## 4. Current state at audit baseline

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`.

The workspace is now structurally decomposed into multiple crates and canonical subsystem owners, but several compatibility and physical-maintenance tails remain.

Known model/tool compatibility examples include:

- `codesearch`, retained as a coding-focused compatibility alias over eggsearch `repo_search`;
- a legacy `TodoTool` compatibility default alongside explicit `todoread` / `todowrite` paths;
- task command compatibility such as `get` mapping to status semantics;
- `multiedit`, which exists in the source module tree but is intentionally absent from the default registry;
- research/search functionality exposed through several adjacent model-facing verbs (`websearch`, `webfetch`, `repo_search`/`codesearch`, `research`, `research_search`, `batch_fetch`, evidence-oriented wrappers), even though backend execution has largely converged.

The default registry is already configurable through `ToolRegistryOptions`; tool definitions support `defer_loading`, `expose_in_definitions`, profile filtering, and `tool_search`. The needed progressive-disclosure machinery therefore exists.

Physical concentration remains substantial:

- `src/agent/loop.rs` is approximately 203 KiB;
- `src/agent/mod.rs` is approximately 132 KiB;
- `src/tool/bash.rs` is approximately 125 KiB.

This is no longer primarily ownership duplication; it is locality/testability debt inside canonical implementations.

Search runtime state is still process-global. `src/search_backend/state.rs` stores an `McpService` and resolved `SearchConfig` in mutable static `RwLock<Option<...>>` slots. Production startup writes them once, while tests overwrite/reset them. This design exists because default tools are constructed before MCP bootstrap, but it creates cross-test/process-global coupling and is a poor fit for multiple independently composed runtime contexts.

## 5. Target architecture

### Compatibility surface

Every surviving compatibility item has one of three explicit states:

```text
canonical       — the name/type/path new code and docs use
compatibility   — retained adapter with named consumer/reason and removal condition
removed         — no supported consumer; canonical replacement is documented
```

A compatibility adapter contains no divergent execution logic. Pre-1.0 aliases without demonstrated external/config/protocol consumers should normally be removed rather than preserved indefinitely.

### Model-visible tools

The default prompt should contain the smallest broadly useful set of unambiguous tools. Specialist/research/evidence variants may remain registered but be deferred, profile-specific, or discoverable through `tool_search`. This is a disclosure change, not a capability deletion.

Canonical tool names and permission/risk contracts remain stable within a turn. Compatibility names, where retained, should not be simultaneously advertised unless a model/profile specifically needs them.

### Agent runtime modules

`AgentLoop` remains the orchestration object, but large implementation blocks move into modules that map to already-recognized responsibilities, such as:

- turn/run lifecycle and completion state;
- tool-call execution/batching/broker integration;
- context and compaction scheduling/application;
- delegation/team/subagent coordination;
- prompt/model-call construction and response handling where not already extracted.

The milestone must favor moving coherent behavior with tests over introducing traits solely to reduce file size.

### Bash modules

`BashTool` remains the model-facing tool. Its internal responsibilities are separated into stable modules, expected approximately as:

- command parsing/classification/policy and destructive checks;
- supervised child-process construction/execution/cancellation;
- output capture/truncation/projection/persistence and result shaping.

Existing execution/sandbox owners remain unchanged.

### Runtime service context

Runtime-created tools receive the services/configuration they need through an explicit context/options object. Mutable global installation/reset is eliminated for production paths and, where practical, for tests. Immutable true process constants/caches may remain global.

The milestone is evidence-driven: it must inventory mutable global service/config slots and migrate only those that affect runtime composition or test isolation. It must not mechanically eliminate every `static` or cache.

## 6. Dependency graph

```text
M001 Compatibility-surface rationalization
    |
    +--> M002 Model-visible tool-surface minimization
    |
    +---- soft ----> M003 Agent-runtime physical decomposition

M004 Bash-tool physical decomposition

M002 ----+
         +--> M005 Explicit runtime-service context / mutable-global cleanup
M003 ----+        (interface dependency: final agent/tool construction seams)
```

Dependency classifications:

- M001 has no hard dependency and is ready.
- M002 is hard-dependent on M001 because it must advertise canonical names after compatibility disposition.
- M003 has only a soft dependency on M001 and may be implemented independently if merge conflicts are controlled.
- M004 is independent of M001–M003 except for ordinary shared verification.
- M005 is hard-dependent on M002's final tool-construction/disclosure contract and has an interface dependency on M003's final agent construction seams.

## 7. Milestones

### M001 — Compatibility-surface rationalization

Class: polish / invariant.

Objective: inventory transitional aliases/re-exports/legacy modules, remove those without a supported consumer, and document explicit removal conditions for those retained.

Exit conditions:

- every touched compatibility item has a canonical replacement and consumer/removal rationale;
- compatibility adapters contain no independent execution behavior;
- obsolete permission/risk/docs entries are removed together with aliases;
- no user-facing or serialized compatibility path is removed without evidence and migration notes;
- focused tests prove canonical paths retain behavior.

### M002 — Model-visible tool-surface minimization

Class: capability / polish.

Objective: reduce default model tool-selection entropy while preserving specialist capabilities through existing progressive-disclosure mechanisms.

Exit conditions:

- a documented canonical core tool palette exists;
- overlapping research/evidence/specialist tools are either deferred/profile-specific or intentionally core with rationale;
- `tool_search` can discover deferred tools and returns enough metadata for correct selection;
- plan mode, agent profiles, permission classes, and tool-program callability remain correct;
- no new routing framework is added.

### M003 — Agent-runtime physical decomposition

Class: polish.

Objective: reduce physical concentration in `src/agent/loop.rs` and `src/agent/mod.rs` by extracting coherent existing responsibilities without changing runtime ownership or public behavior.

Exit conditions:

- the dominant orchestration responsibilities are in named modules with focused tests;
- `AgentLoop` remains thin enough that turn lifecycle can be understood without navigating unrelated helper families;
- no duplicate state machine or new generic coordinator is introduced;
- existing agent runtime, cancellation, compaction, tool, delegation, and projection tests remain green.

### M004 — Bash-tool physical decomposition

Class: polish / invariant.

Objective: split `src/tool/bash.rs` along existing policy, execution, and result boundaries while preserving all command safety/resource semantics.

Exit conditions:

- command policy/classification is independently testable;
- supervised execution/cancellation is independently testable;
- bounded output/result persistence is independently testable;
- `BashTool` remains the single model-facing owner;
- no second process executor, sandbox policy, or scheduler path is introduced.

### M005 — Explicit runtime-service context and mutable-global cleanup

Class: infrastructure / invariant.

Objective: remove mutable process-global service/config installation where it currently couples tool construction, search/MCP startup, and tests.

Exit conditions:

- production search/web tools consume explicit runtime-owned service/config references rather than `install_*`/resettable process-global slots;
- two independently constructed runtime contexts can coexist in tests without overwriting one another;
- test serialization that existed solely for mutable global service replacement is removed or materially reduced;
- immutable caches/constants remain global only where justified;
- no dependency-injection framework is added.

## 8. Cross-cutting requirements

### Storage and migration

No production schema migration is expected. If compatibility removal touches persisted configuration, M001 must preserve parsing/migration or stop and split that work into a dedicated migration plan.

### Protocol and compatibility

Protocol DTOs are out of scope unless a supposedly internal alias is proven to be serialized. Such evidence turns the change into compatibility/migration work and requires explicit treatment rather than deletion.

### Security and authorization

Tool disclosure changes must never widen authority. Deferred/discovered tools are still filtered through ordinary agent definition, permission, broker, workspace, and child-agent policies. Bash extraction must preserve all sensitive-path, destructive-command, sandbox, child-Git, scheduler, and output-bound checks.

### Concurrency, cancellation, and recovery

Physical extraction must not change cancellation ownership, spawned-task lifetime, tool batching, process-group termination, turn snapshots, or restart/replay semantics. Runtime-service context must be immutable or deliberately synchronized after construction; it must not recreate ad hoc per-call global mutation.

### Observability and audit

Existing tool/run attribution, tracing, dropped-event diagnostics, and provenance remain intact. Compatibility removals should not erase historical persisted names without a migration requirement.

### Performance and resource use

No milestone requires a performance framework. Avoid additional cloning/locking in hot paths; M005 should generally reduce shared global locking. Broad verification remains the repository's existing minimal contract.

### Documentation and operations

Update `architecture/tool.md`, `architecture/agent-tool-surface.md`, `architecture/search_backend.md`, `architecture/agent.md` or their current equivalents only where implementation changes the described surface. Remove stale compatibility language when the corresponding path is removed.

## 9. Verification strategy

Each milestone starts with focused tests for the moved/dispositioned behavior. Broad closure uses the repository's existing bounded commands:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Where current CI intentionally uses a narrower default-feature Clippy/test command, the closure record should distinguish local all-feature evidence from hosted evidence rather than rewriting CI.

Static guards may be added only when they enforce a durable ownership invariant and are materially simpler than regression tests.

## 10. Risks and decision points

- A compatibility name may have an undocumented external consumer. M001 must search config, protocol, docs, CLI examples, tests, and migration code before removal.
- A smaller prompt tool surface can accidentally make a capability unreachable. M002 must verify actual discovery/invocation, not only definition counts.
- Mechanical module extraction can increase indirection without improving ownership. M003/M004 must reject extraction that creates pass-through layers or duplicated state.
- Runtime context work can become a dependency-injection rewrite. M005 is limited to mutable service/config globals with demonstrated composition/test-isolation cost.

No decision above currently requires an ADR. A proposed canonical owner change does.

## 11. Completion definition

This roadmap closes when M001–M005 have accepted closure records and the repository has:

- one documented canonical compatibility/tool naming surface;
- a smaller default model-visible palette with preserved discoverability;
- materially smaller and responsibility-oriented agent/Bash implementation modules;
- no mutable global search/MCP installation requirement in normal production tool construction;
- no new orchestration, routing, or verification framework introduced to achieve the cleanup.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready | `plans/implementation/post-audit-maintainability-surface/001-compatibility-surface-rationalization.md` | — | — |
| M002 | blocked | `plans/implementation/post-audit-maintainability-surface/002-model-visible-tool-surface-minimization.md` | — | hard on M001 |
| M003 | ready | `plans/implementation/post-audit-maintainability-surface/003-agent-runtime-physical-decomposition.md` | — | soft merge dependency on M001 |
| M004 | ready | `plans/implementation/post-audit-maintainability-surface/004-bash-tool-physical-decomposition.md` | — | — |
| M005 | blocked | `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md` | — | hard on M002; interface on M003 |
