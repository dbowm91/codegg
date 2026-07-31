# Agent Runtime, Model Adaptation, and ACP Milestone 011 — Integration Evidence and Closure

Status: blocked — requires Milestones 004 through 010 closure

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-011--integration-evidence-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#30-completion-criteria`
- `plans/003-planning-process.md#2.5-closure-records`

Primary class: closure

## 1. Objective

Perform the final cross-milestone correctness, ownership, compatibility, documentation, and evidence pass for the agent-runtime/model-adaptation/ACP roadmap. This milestone does not add new product scope. It verifies that the separately implemented pieces form one coherent production path, removes or clearly classifies superseded compatibility paths, and creates an independent closure record with a requirement-to-evidence matrix.

Strict closure requires no unresolved high/medium correctness, authority, cancellation, privacy, or protocol finding. Existing unrelated repository failures must be reported accurately but must not trigger broad scope expansion.

## 2. Dependencies

Hard dependencies:

- M001-M003 closed: prompt/agent correctness, resolved tool surface, bounded nesting;
- M004-M005 closed: specialized security/research runtimes;
- M006 closed: progress/recovery controller;
- M007-M008 closed: declarative adapters and Laguna reasoning vertical slice;
- M009 closed: actual context-plan consumption;
- M010 closed: ACP v1 adapter.

Closure must be performed by a review pass separate from the final implementation changes where feasible. The implementation agent may prepare evidence, but strict disposition should not rely only on self-attestation.

## 3. Closure questions

The reviewer must answer, with concrete evidence:

1. Do root and child turns use one prompt compiler, agent resolver, model adapter, tool surface, and context-plan path?
2. Can custom agents extend built-ins without losing runtime kind, permissions, prompts, or safety ceilings?
3. Can a three-level read-only delegation tree execute, cancel, join, and return resources to baseline?
4. Can any child widen parent tools, paths, shell/Git/network authority, or delegation budgets?
5. Do security and research runtime kinds invoke real host behavior and typed reports?
6. Does the recovery controller nudge/correct/restore before bounded stalled termination without inspecting hidden reasoning?
7. Are built-in model adapters compiled from strict TOML through Cargo and pinned/fingerprinted per turn?
8. Can Laguna-style interleaved reasoning round-trip without entering projections, ACP, logs, or audit metadata?
9. Does the actual provider request consume one context plan with correct chronology and reversible reductions?
10. Does ACP v1 operate through the singleton daemon and canonical projections with protocol-pure stdout?
11. Do cancellation, restart/interruption, duplicate delivery, slow consumers, and partial failures produce deterministic terminal ownership?
12. Are documentation, configuration examples, and runtime diagnostics consistent with production behavior?

## 4. Invariants that must be reverified

- Daemon/scheduler/tool broker/permission authority is singular.
- Child authority never widens.
- Prompt/tool/schema/runtime state agrees.
- Active turns pin asset, adapter, prompt, and tool/context identities.
- Nested work is bounded and cancellation propagates.
- Specialized agents use ordinary runtime ownership.
- Recovery remains bounded and observable-action-only.
- Private reasoning remains provider-round-trip-only.
- Context reduction is reversible and cannot omit required protocol state.
- ACP remains an adapter, not a second runtime or durable state owner.
- Secret/private/large content remains redacted, omitted, bounded, or handle-backed.

## 5. Scope

### In scope

- Audit all production call sites for superseded prompt, agent, tool-filter, model-profile, context-packer, descendant-spawner, and ACP mapping paths.
- Remove dead paths or classify them as bounded compatibility seams with owner/removal criteria.
- Add only missing regression tests/static guards needed to prove already-planned invariants.
- Run focused milestone suites and one agreed broad local verification set.
- Execute production-shaped end-to-end fixtures spanning multiple milestones.
- Reconcile architecture docs, roadmap, registry, examples, command help, config references, and package manifests.
- Verify crate/package asset inclusion for model adapters and ACP binary/subcommand.
- Create `plans/closure/agent-runtime-model-adaptation-acp/011-status.md`.
- Update the subsystem roadmap and registry according to the final disposition.

### Out of scope

- New model adapters beyond correction of the accepted Laguna/generic set.
- ACP v2 or network transports.
- Final durable agent-run database/worktree/team authorization implementation.
- New security scanners/search backends.
- Broad performance optimization without a measured blocker.
- CI/release redesign.

## 6. Required integration scenarios

### Scenario A — Canonical root execution

- load a project with built-in plus custom inherited agent;
- capture immutable asset snapshot;
- resolve model adapter/tool surface/context plan;
- run a root tool call and completion;
- prove prompt/schema/backend fingerprints agree and no legacy prompt path is used.

### Scenario B — Nested custom security flow

- root agent delegates to a customized security-review agent;
- security runtime runs deterministic preflight;
- security agent delegates to one approved read-only specialist;
- report distinguishes finding versus review prompt;
- cancellation/authority/resource evidence is captured.

### Scenario C — Nested research flow

- research coordinator uses explicit workspace context;
- spawns two bounded scouts;
- deduplicates sources, records a conflict, validates citations, and synthesizes;
- no mutation or cwd dependency occurs.

### Scenario D — Recovery flow

- mock model repeats an equivalent tool error;
- controller nudges, corrects alias/schema or restores base surface;
- model either progresses or returns typed stalled outcome;
- no hidden reasoning or secret result is logged.

### Scenario E — Laguna flow

- resolve compiled Laguna adapter;
- execute captured two-round interleaved reasoning/tool fixture;
- preserve private reasoning provider-side;
- verify projection/ACP/log negatives;
- verify canonical permission handling through wire aliases.

### Scenario F — ACP flow

- start/attach singleton daemon and `codegg acp`;
- initialize/new/prompt;
- stream tool and permission updates;
- cancel nested work;
- load/replay-to-live, resume if advertised, and close;
- prove stdout purity and subscription/task cleanup.

## 7. Ordered work packages

### A — Static ownership and call-site audit

- search for all legacy/duplicate prompt loaders, role/name tool filters, model substring branches, direct descendant loop construction, observation-only request builders, private reasoning serialization, and ACP-independent state;
- classify each as removed, canonical, or compatibility seam;
- add narrowly targeted guards only for stable high-value boundaries.

### B — Cross-milestone fixture completion

- implement/run Scenarios A-F;
- ensure fixtures use production factories/transports where the milestone claims production behavior;
- avoid replacing mechanism evidence with mocks except provider/model responses and external editor endpoints.

### C — Failure/resource review

- exercise cancellation, timeout, duplicate spawn/request, child failure, slow ACP client, invalid adapter/config, compaction/reduction fallback, and daemon disconnect/restart interruption;
- compare tasks, permits, subscriptions, lineage counters, artifact handles, and queues to baseline.

### D — Documentation/package reconciliation

- update architecture docs and config examples;
- confirm generated adapter inputs are packaged;
- confirm ACP command/help and dependency features;
- reconcile roadmap/registry statuses and remove stale deferred ACP entry;
- document remaining long-term agent-run/worktree/team limitations.

### E — Independent closure record

- produce requirement-to-evidence matrix by milestone;
- list exact commits/commands/outcomes;
- classify findings by severity;
- recommend closed, conditionally closed, corrective pass required, or blocked;
- do not mark closed while a reproducible high/medium finding remains.

## 8. Verification strategy

Use focused commands from M001-M010 plus one final broad local set. Avoid multiplying repeated stress runs without a concrete flake hypothesis.

Minimum broad local set, adapted to actual crates/features:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --lib
cargo test --test subagent
cargo test --test agent_loop_harness
cargo test --test session_projection_consumer
cargo test --test acp_stdio
cargo test --test projection_transport_real --features server
cargo package --allow-dirty --no-verify
```

Run clippy only if it is part of the repository's current canonical local verification contract; do not introduce a new all-features CI gate in this closure milestone.

Optional/manual evidence must be labeled:

- live Laguna serving validation;
- live ACP editor integration;
- provider prompt-cache telemetry.

These may strengthen evidence but are not substitutes for deterministic local fixtures and are not routine CI blockers.

## 9. Required static/negative evidence

- no production root/child bypass of canonical prompt compiler;
- no role/name-based authority filter outside compatibility tests;
- no wire alias execution before canonical normalization/permission check;
- no descendant loop without shared spawner when delegation is advertised;
- no daemon production research service rooted in process cwd;
- no private reasoning in projections/ACP/log/audit serializers;
- no ACP adapter creation of independent provider/agent/session authority;
- no unbounded ACP outbound queue;
- model adapter build inputs included in package;
- no arbitrary executable adapter transform.

## 10. Acceptance criteria

- All closure questions have concrete affirmative or explicitly bounded/deferred answers.
- Scenarios A-F pass with mechanism-faithful evidence.
- Focused and broad local verification results are recorded truthfully.
- Dead/duplicate paths are removed or have explicit compatibility owner/removal criteria.
- Architecture/config/help/package documentation matches production behavior.
- No unresolved high/medium correctness, authority, cancellation, privacy, or ACP protocol finding remains.
- Final closure record and registry/roadmap statuses agree.
- Deferred final durable agent-run/worktree/team work is not misrepresented as closed.

## 11. Stop conditions

Stop and require a corrective plan if:

- any canonical production path still bypasses prompt/tool/adapter/context resolution;
- three-level delegation or cancellation leaks authority/resources;
- specialized runtimes are still prompt-only;
- recovery can loop indefinitely or exposes hidden reasoning;
- Laguna reasoning leaks outside provider history;
- context reduction can remove required protocol state;
- ACP uses a second runtime/state owner or corrupts stdout framing;
- broad verification reveals an in-scope reproducible high/medium defect.

Do not absorb unrelated repository failures without evidence that this roadmap caused them.

## 12. Required closure record

Create `plans/closure/agent-runtime-model-adaptation-acp/011-status.md` with:

- reviewed baseline/head and implementation commit list;
- milestone requirement-to-evidence matrix;
- Scenarios A-F outcomes;
- focused/broad command table with exact outcomes;
- ownership/authority/cancellation/resource evidence;
- privacy/disclosure evidence;
- compatibility/package/documentation evidence;
- unresolved findings by severity;
- known deferred long-term work;
- final recommendation.
