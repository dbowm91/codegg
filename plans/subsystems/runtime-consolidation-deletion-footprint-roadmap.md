# Runtime Consolidation, Deletion, and Footprint Roadmap

Status: active

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/tool.md`
- `architecture/scheduler.md`
- `architecture/testing.md`
- `architecture/overview.md`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- no new ADR is required for this roadmap because it preserves the existing single-daemon, scheduler-authority, structured tool-execution, PromptCompiler, and single-binary decisions;
- implementation MUST stop and require a separate ADR if it discovers that a public protocol, executable topology, scheduler ownership, durable storage model, or user-visible compatibility contract must change.

## 1. Purpose and ownership boundary

This workstream converts the current post-migration architecture into a smaller and more authoritative runtime by deleting superseded compatibility implementations and consolidating policy ownership.

The August 11, 2026 repository audit found that CodeGG's principal architectural pieces are now present, but several migrations left both the new canonical mechanism and its predecessor alive. The resulting cost is visible in duplicated scheduling paths, typed tool outcomes that still coexist with string-derived recovery semantics, a roughly 287 KiB `src/agent/loop.rs`, legacy prompt/instruction assembly beside `PromptCompiler`, and a growing set of source-regex verification ratchets.

This roadmap owns:

- removal or adapterization of the legacy `BackgroundScheduler` path so durable daemon scheduling is the only production scheduling implementation;
- correction and consolidation of progress/recovery semantics around structured tool execution outcomes and observable effects;
- decomposition of `AgentLoop` into a small orchestration driver plus existing canonical subsystem owners, without inventing a new framework;
- deletion of superseded prompt, provider-compatibility, process-CWD, and history-repair paths where current production callers no longer require them;
- review and deletion of migration-only static guards when types, crate/module visibility, constructors, tests, or canonical execution boundaries now enforce the same invariant;
- measured binary/dependency cleanup only after reachable obsolete code has been removed;
- one final integration and documentation closure pass.

It consumes but does not redefine:

- single user-scoped daemon ownership;
- durable job/schedule stores and scheduler admission control;
- project/workspace/session identity;
- Tool Broker authority and Tool Program contracts;
- PromptCompiler/runtime-asset snapshot semantics;
- ACP/native protocol boundaries;
- permission and child-authority intersection rules;
- manual release cadence and the existing one-job CI topology.

Governing rule:

> Prefer deletion and one authoritative owner over additional compatibility layers, policy wrappers, source scanners, or speculative abstractions.

## 2. Work classification

### Invariants

- Production daemon scheduling has one authoritative durable implementation.
- A compatibility API may adapt to the durable scheduler but MUST NOT own a second independent scheduling loop or persistence interpretation.
- Tool execution status is structured internally; model-facing text is presentation, not authority.
- Permission denial, timeout, cancellation, protocol failure, and ordinary tool failure MUST NOT be rediscovered from arbitrary result prose when typed status is known.
- Recovery progress is tied primarily to observable execution/evidence effects; volatile output text alone MUST NOT permit infinite autonomous progress.
- Provider protocol repair remains separate from semantic tool recovery.
- `AgentLoop` orchestrates canonical subsystem owners and MUST NOT become a second implementation of their policies.
- Prompt compilation remains deterministic and owned by `PromptCompiler` plus immutable runtime assets.
- Process-global CWD MUST NOT regain authority in daemon-owned active turns.
- Routine CI remains one bounded job; manual release cadence remains unchanged.
- Dependency or binary-size work MUST preserve user-visible capability and supported production features.

### Capabilities

- Background/durable scheduled work continues to function through one scheduler contract.
- Autonomous execution can distinguish denied, timed-out, cancelled, protocol-failed, tool-failed, successful-no-progress, and successful-progress outcomes without text heuristics where structured information exists.
- Existing primary, follow-up, and descendant agent execution behavior remains available after `AgentLoop` decomposition.
- Existing project instructions, agent definitions, model adapters, and runtime assets continue to compile into the same effective prompt contract after legacy code deletion.
- Existing local verification remains sufficient to prevent regressions without retaining migration-only machinery.

### Infrastructure and polish

- Large agent files may be split into concrete internal modules when that produces clear ownership; the work MUST NOT add a generic event-sourcing, workflow, actor, middleware, or policy framework.
- Capability metadata may move to tool contracts/catalog records when that eliminates hard-coded name lists or timeout tables.
- Architecture documentation should describe ownership and invariants rather than duplicate implementation field inventories.
- Binary-size measurements are evidence, not CI gates.

## 3. Explicit non-goals

This roadmap MUST NOT:

- redesign the daemon, project identity, distributed coordinator/leaf model, ACP, session projection, Git/worktree orchestration, provider connection system, or Tool Programs;
- split daemon and TUI into separate production binaries;
- replace Tokio, SQLx, Reqwest, Rustls, RustPython, Wasmtime, Ratatui, or other major dependencies solely for size or freshness;
- upgrade Wasmtime away from the current LTS line without a concrete compatibility/security/feature requirement;
- implement new model-specific giant prompts;
- introduce an autonomous planner framework or generalized workflow engine;
- increase default recovery budgets in the name of robustness;
- add hidden retries for non-idempotent mutations;
- add CI matrices, scheduled audits, benchmark gates, coverage gates, binary-size gates, automatic dependency update bots, artifact workflows, release automation, or fixed release cadence;
- add new source-regex guards merely to enforce preferences that can be represented in Rust types or tests;
- reopen already-closed roadmap work unless current repository evidence demonstrates a regression relevant to this consolidation.

## 4. Current-state summary

At the reviewed baseline:

- `src/agent/task.rs` creates UUID-string `BackgroundTask.id` values, while `BackgroundScheduler::spawn_loop()` attempts `task.id.parse::<u64>()` and skips nonnumeric IDs. The production SQLite daemon already disables this legacy compatibility scheduler in favor of durable scheduling, making the defect strong evidence that the duplicate implementation should be removed rather than expanded.
- `CoreRuntimeDeps::LegacyAgentRuntimeDeps` explicitly labels the old subagent/background scheduler container transitional and disables its compatibility request surface in production SQLite construction.
- `ToolExecutionStatus`/`ToolExecutionOutcome` distinguish structured outcomes, but progress recovery still has legacy text-derived error classification and underuses typed statuses.
- `RecoveryController::detect()` can classify `EquivalentResult` based on a matching result fingerprint anywhere in history after the same action has occurred, rather than restricting equivalence to the same action/tool identity.
- `ProgressSignal` declares more semantic states than `observe()` currently emits; `new_evidence`, `state_changed`, and `child_advanced` collapse to `StateChanged`, while `DifferentTool` is not produced by that path.
- `src/agent/loop.rs` is approximately 287 KiB and owns provider streaming, context packing/policy, tool exposure, permissions, tool execution, MCP handling, recovery, compaction, history repair, snapshots, usage, hooks, task state, goal state, steering, and projection concerns.
- `src/agent/prompt.rs` still contains deprecated process-CWD instruction discovery, legacy prompt loaders, remote instruction fetch compatibility, provider-prompt selection, and wrappers alongside the canonical compiler/runtime-asset path.
- the repository has accumulated many Python/shell static scanners for migration invariants. Routine CI itself is already appropriately small at one bounded job; the concern is retained ratchet machinery, not workflow topology.
- release profile already uses `lto = true`, `strip = true`, and `codegen-units = 1`; server, Wasmtime plugin, and image surfaces are feature-gated. Size work therefore should start by deleting reachable obsolete code and narrowing proven feature defaults, not linker experimentation.

## 5. Target architecture

The desired runtime is intentionally simple:

```text
TurnRuntime
  -> PromptCompiler + immutable runtime assets
  -> provider/model adapter
  -> AgentLoop orchestration
       -> ToolBroker structured batch execution
       -> one Autonomy/Recovery state machine
       -> Context/Compaction owner
       -> scheduler/subagent owner
  -> terminal/public projection
```

Scheduling:

```text
compat request (if retained)
      |
      v
Durable ScheduleStore / JobSubmissionService
      |
      v
JobScheduler
```

There is no independent background timer loop interpreting the `task` table differently from durable schedules.

Tool/recovery authority:

```text
Tool call
  -> permission/authority decision
  -> ToolBroker
  -> StructuredToolOutcome
       status
       model_text
       effect/progress facts
       provenance/contract metadata
  -> recovery decision
  -> model-facing text projection
```

Prompt authority:

```text
explicit ExecutionContext + ProjectAssetSnapshot + model profile
      -> PromptCompiler
      -> ContextPlan
      -> provider adapter projection
```

Provider-specific compatibility changes wire representation; it does not mutate canonical durable history or introduce an alternative prompt owner.

## 6. Dependency graph

```text
M001 scheduler compatibility deletion --------+
M002 structured recovery authority -----------+--> M003 AgentLoop decomposition ----+
M004 prompt/history legacy deletion ----------+------------------------------------+
M005 verification-ratchet cleanup ------------+------------------------------------+--> M006 measured dependency/footprint pass --> M007 integration/closure
```

Dependency classes:

- M001, M002, M004, and M005 are independently executable against the reviewed baseline after rebasing on current `main`.
- M003 has a hard dependency on M002 because loop extraction must target the final structured execution/recovery boundary rather than freeze the transitional one. Its initial pass established seams but requires the corrective physical-extraction plan before M006 can proceed.
- M003 has soft dependencies on M001 and M004 because deleted compatibility code reduces the amount of loop/prompt state that must be moved; it may begin once M002 closes if merge ordering is managed explicitly.
- M006 has hard dependencies on M001-M005. Its audit is recorded against the post-M005 tree, but strict closure remains blocked until M003 corrective physical extraction completes; final accepted dependency/feature changes must be based on the consolidated tree.
- M007 has hard dependencies on M001-M006 and is the only milestone that may close this roadmap.

## 7. Ordered milestones

### M001 — Legacy background scheduler deletion and durable-schedule convergence

Status: closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/001-legacy-background-scheduler-deletion.md`

Remove the independent legacy timer/persistence/dispatch implementation or reduce compatibility requests to a thin durable-scheduler adapter. Preserve external behavior only where a live compatibility caller exists.

### M002 — Structured execution outcome and recovery convergence

Status: closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/002-structured-outcome-recovery-convergence.md`

Fix result-equivalence scoping, make known typed statuses authoritative, define effect/progress facts, and collapse unused recovery semantics rather than extending heuristics.

### M003 — AgentLoop ownership decomposition

Status: closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/003-agent-loop-ownership-decomposition.md`

Reduce `AgentLoop` to a turn driver by extracting existing concrete ownership domains for tool batches, context/prompt policy, and provider/recovery orchestration while preserving behavior and avoiding a new framework.

### M004 — Prompt, provider-compatibility, and history legacy deletion

Status: closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/004-prompt-provider-history-legacy-deletion.md`

Audit callers and remove superseded process-CWD prompt loaders, old provider prompt selection, remote-instruction compatibility, and canonical-history mutation where adapter projection can provide compatibility instead.

### M005 — Static verification ratchet retirement and documentation contraction

Status: closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/005-verification-ratchet-retirement.md`

Closure: `plans/closure/runtime-consolidation-deletion-footprint/005-status.md`

Classify static guards as permanent invariants or temporary migration ratchets, delete ratchets whose underlying boundary is now structural/tested, and contract architecture docs that duplicate source internals. Keep routine CI as one job.

### M006 — Measured dependency and binary-footprint cleanup

Status: ready

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/006-measured-dependency-binary-cleanup.md`

Measure release contributors and feature graph after code deletion, narrow only proven unnecessary features/dependencies, evaluate ordinary upstream maintenance items, and stop when further reduction would require feature loss or architectural churn.

### M007 — Integration, verification, and strict closure

Status: conditionally closed

Plan: `plans/implementation/runtime-consolidation-deletion-footprint/007-integration-verification-closure.md`

Reconcile architecture docs, run one broad existing verification pass, capture final footprint evidence, confirm no capability regression, and write the closure record.

## 8. Verification posture

Each implementation milestone owns focused tests for the behavior it changes plus `scripts/verify.sh quick` when production Rust or manifests change. Broad workspace/hosted evidence is centralized in M007 except where a milestone cannot safely merge without a specific workspace-level check such as Clippy after a major module move.

Do not create a new CI workflow, lane, matrix, dispatch mechanism, scheduled audit, or continuous binary-size gate.

Prefer behavioral/property tests and compiler-enforced boundaries over source scanners. A retained static guard must state the invariant it uniquely enforces and why Rust visibility/types/tests cannot enforce it more directly.

## 9. Security, compatibility, storage, protocol, and migration

Security:

- permission and child-authority intersection cannot weaken;
- denial/cancellation/timeout statuses must remain authoritative through recovery;
- mutation retry policy must respect idempotency/effect contracts;
- removing guards is allowed only after equivalent or stronger structural/test enforcement is demonstrated.

Compatibility:

- public CLI names, configuration, native protocol, ACP behavior, supported tools, provider semantics, and TUI-visible capability remain stable;
- deprecated internal Rust APIs may be removed after proving no supported caller depends on them;
- compatibility requests may remain as adapters but not as independent implementations.

Storage:

- no schema migration is expected. M001 should reuse durable job/schedule tables instead of introducing another migration.

Protocol:

- no wire-format change is planned. If M001 discovers a live compatibility request that cannot map to the durable API without wire change, stop and document it rather than silently altering protocol semantics.

Migration:

- internal source/module moves and deletion of obsolete scripts/docs are expected;
- no user/operator migration should be required.

## 10. Exit conditions

This workstream is complete only when:

- the legacy background scheduler no longer owns an independent production loop/persistence model;
- the UUID-to-`u64` dispatch defect is impossible because the duplicate path is removed or compatibility uses the durable scheduler's typed identity;
- structured tool status is authoritative wherever execution knows it;
- recovery equivalence is scoped to the intended action/effect identity;
- recovery cannot treat volatile text novelty alone as unbounded meaningful progress;
- unused/fictional progress states are either implemented with real semantics or removed;
- `AgentLoop` no longer owns substantial implementations of context policy, tool execution policy, provider compatibility, and recovery policy simultaneously;
- prompt construction has one production compiler/runtime-asset path, with only explicitly justified compatibility adapters remaining;
- canonical durable history is not mutated solely to satisfy a provider wire grammar when projection can do so safely;
- migration-only static guards identified in M005 are deleted or converted to a simpler structural/test invariant;
- architecture documentation no longer carries stale field-by-field implementation inventories as normative architecture;
- final release-size and feature-tree measurements are captured after deletion;
- no major dependency replacement occurs without measured no-feature-loss benefit;
- routine CI remains one bounded job and manual release remains unchanged;
- `scripts/verify.sh quick`, required focused tests, workspace Clippy/tests in the existing verification path, and one existing hosted `CI / verify` run pass on the accepted final tree;
- a closure record under `plans/closure/runtime-consolidation-deletion-footprint/007-status.md` contains a requirement-to-evidence matrix and classifies all remaining findings.

## 11. Deferred work

Outside this roadmap unless new evidence makes it necessary:

- binary topology split;
- generalized provider/HTTP client unification;
- distributed scheduler redesign;
- new agent planning language or workflow engine;
- arbitrary non-UTF-8 transport redesign;
- major Ratatui/TUI redesign;
- replacing SQLx/Tokio/Reqwest/RustPython/Wasmtime solely for footprint;
- automatic dependency updates;
- scheduled security or size workflows;
- release automation;
- unrelated code-style cleanup discovered during module moves.
