# Agent Runtime, Model Adaptation, and ACP Corrective Closure Addendum

Status: closed — Milestones 012–017 strictly closed under DVR M007

Repository baseline reviewed: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Superseded strict closure claim:

- `plans/closure/agent-runtime-model-adaptation-acp/011-status.md`

Corrective disposition:

- `plans/closure/agent-runtime-model-adaptation-acp/011-corrective-status.md`

Source roadmap retained for historical scope:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md`

Related implementation plans:

- `plans/implementation/agent-runtime-model-adaptation-acp/012-acp-turn-lifecycle-and-correlation-correctness.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/013-specialized-runtime-finalization-and-research-coordination.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/014-canonical-prompt-and-context-plan-convergence.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/015-adapter-driven-reasoning-safety.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/016-descendant-admission-cancellation-and-execution-context.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md`

## 1. Purpose

Milestones 001–011 established the intended architecture, but a post-closure production-path audit found several correctness gaps that invalidate strict subsystem closure. This addendum owns a narrow corrective pass. It does not redesign the daemon, provider interface, scheduler, projection subsystem, durable AgentRun roadmap, team authorization, or release apparatus.

The corrective pass closes only these findings:

1. ACP prompt lifecycle can acknowledge cancellation or close before a native turn ID exists without reliably delivering cancellation later; session-scoped projection events can also bind or complete the wrong active prompt, and session replay loses message-role semantics.
2. Security and research validation helpers exist but are not authoritative production finalizers. Research planning remains prompt-led rather than a bounded host-owned child/evidence coordinator.
3. Root prompt construction still appends memory, security, research, goal, LSP, Git, and a duplicate plan-mode contract after canonical compilation, leaving those blocks outside compiler identity and cache planning.
4. Laguna/provider-private reasoning truncation is not UTF-8 safe, and Laguna request behavior is still activated by model-name substring checks instead of the resolved adapter contract.
5. Descendant active-admission checks are not reserved atomically, cancellation ownership is pool-global rather than root-scoped, and the canonical native tool execution envelope still derives cwd from process-global state.
6. The prior closure record relied on focused evidence while broad local verification remained unresolved; final closure must state exactly which gates are green, blocked, or unrelated.

## 2. Ownership boundary

This addendum owns:

- ACP request/native-turn correlation and pending-cancellation delivery;
- ACP load/replay role correctness and bounded subscription teardown;
- typed security/research output finalization through the ordinary agent runtime;
- bounded host-owned research child execution and evidence aggregation;
- complete prompt/context block construction before compiler fingerprinting;
- adapter-driven provider request transforms and safe private-reasoning handling;
- atomic descendant admission reservations and root-scoped cancellation;
- explicit workspace identity in native tool execution context;
- focused corrective fixtures and independent closure evidence.

It consumes without redefining:

- singleton daemon and native session/turn ownership;
- canonical session projections and replay cursors;
- `PromptCompiler`, `ResolvedToolSurface`, `ContextPlan`, and model-adapter registry;
- ordinary `AgentLoop`, scheduler, tool broker, permission checker, and subagent pool;
- security/research evidence contracts created by Milestones 004–005;
- development-verification/release ownership for unrelated broad repository failures.

It does not own:

- durable AgentRun persistence or daemon-restart recovery;
- worktree allocation for mutation-capable descendants;
- team/principal authorization completion;
- ACP v2, editor-specific extensions, or network ACP transport;
- browser automation, mandatory live model testing, or third-party security scanners;
- automated releases or a larger CI matrix.

## 3. Corrective invariants

### ACP

- One ACP prompt request is correlated to exactly one native submission and one native turn.
- Events from another turn in the same session cannot bind, update, or complete the active prompt.
- Cancellation or close requested before the native turn ID is observed remains pending and is delivered immediately when the matching turn becomes identifiable.
- A closed ACP session cannot continue emitting prompt updates.
- `session/load` replays user and assistant content according to ACP semantics; it does not relabel every stored text part as assistant output.
- ACP remains a transient adapter and does not become a second durable session authority.

### Specialized runtimes

- Provider schema requests are advisory; local parse and validation are authoritative.
- Unsupported security findings cannot leave the runtime as confirmed findings.
- Research children return typed evidence records, not authoritative essays.
- Research source, evidence, claim, conflict, limitation, and citation relationships are host-validated before completion.
- Security/research use ordinary permission, tool, scheduler, cancellation, and projection ownership.

### Prompt/context

- Every system-context block that affects provider behavior is assembled before prompt/compiler fingerprinting.
- Plan-mode guidance appears once.
- Context/cache identity changes when any effective prompt, adapter, tool surface, runtime evidence, or reasoning mode changes.
- Provider message chronology and tool protocol pairing remain lossless.

### Adaptation and reasoning

- Private reasoning is truncated on valid UTF-8 boundaries and remains private.
- Provider request transforms are selected from the resolved adapter, not raw model-name substring checks.
- Adapter data cannot grant permissions or execute arbitrary code.
- Unknown models retain conservative generic behavior.

### Descendant and execution ownership

- Active descendant capacity is reserved atomically before enqueue acceptance.
- Reservation rollback and completion release are exact and idempotent.
- Cancelling one root lineage does not cancel unrelated roots; parent cancellation reaches all accepted descendants of that root.
- Tool execution cwd comes from explicit workspace/execution context, never process-global cwd.

## 4. Milestone sequence

### Milestone 012 — ACP turn lifecycle and correlation correctness

Primary class: protocol/correctness.

Correct ACP native-turn binding, pending cancellation, close teardown, terminal matching, replay roles, and negative lifecycle behavior.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/012-acp-turn-lifecycle-and-correlation-correctness.md`

Exit conditions:

- pre-turn-ID cancel and close are delivered once the matching turn appears;
- stale/replayed/session-neighbor events cannot bind or terminate the active prompt;
- load/replay preserves message roles and bounded tool/message semantics;
- ACP stdio remains protocol-pure and existing supported methods remain compatible.

### Milestone 013 — Specialized runtime finalization and research coordination

Primary class: capability/correctness.

Make security and research output parsing/validation authoritative and implement bounded host-owned research child/evidence coordination.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/013-specialized-runtime-finalization-and-research-coordination.md`

Dependencies:

- Milestone 012 strict closure, so ACP/projection consumers observe one stable terminal contract while specialized terminal output is corrected.

Exit conditions:

- security final output is parsed and passed through local evidence validation;
- research plans can execute bounded configured child roles through the shared pool;
- child reports are typed and aggregated into a locally validated final report;
- partial failure, cancellation, source deduplication, conflicts, and citation gaps are explicit.

### Milestone 014 — Canonical prompt and context-plan convergence

Primary class: invariant/correctness.

Move all effective runtime context into typed prompt blocks before compiler and cache identity are finalized.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/014-canonical-prompt-and-context-plan-convergence.md`

Dependencies:

- Milestone 013 strict closure, because specialized evidence/finalizer inputs must have stable typed contracts before prompt/context convergence.

Exit conditions:

- root and child prompt inputs are complete before compilation;
- memory, goal, LSP, Git, security, research, skills, and instructions have explicit block identity/classification;
- the duplicate plan-mode contract is removed;
- compiler/context/cache fingerprints change for every behavior-affecting input.

### Milestone 015 — Adapter-driven reasoning safety

Primary class: provider/correctness.

Make private-reasoning accumulation UTF-8 safe and apply reasoning/thinking/request transforms from the resolved adapter rather than model-name checks.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/015-adapter-driven-reasoning-safety.md`

Dependencies:

- Milestone 014 strict closure, so adapter and reasoning fingerprints feed the final prompt/context identity without another migration.

Exit conditions:

- multibyte reasoning near the limit cannot panic;
- Laguna-compatible behavior is activated by resolved adapter capabilities/transforms;
- aliases and exact/custom model IDs receive the same adapter-selected behavior;
- private reasoning remains absent from public serialization, projections, ACP, logs, and diagnostics.

### Milestone 016 — Descendant admission, cancellation, and execution context

Primary class: authority/concurrency.

Correct active descendant reservation, lineage-scoped cancellation, cleanup, and explicit workspace ownership in native tool execution envelopes.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/016-descendant-admission-cancellation-and-execution-context.md`

Dependencies:

- Milestone 015 strict closure, preserving a single sequential production-path handoff and avoiding concurrent edits to the agent loop/provider boundary.

Exit conditions:

- active-descendant admission cannot oversubscribe under concurrent enqueue;
- reservations release exactly once on rejection, completion, cancellation, timeout, or shutdown;
- root cancellation is isolated and cascades only through its lineage;
- no production native tool dispatch obtains cwd from `std::env::current_dir()`.

### Milestone 017 — Corrective integration evidence and closure

Primary class: closure.

Independently audit Milestones 012–016, reconcile the prior M011 status, and produce truthful focused and broad evidence.

Plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md`

Dependencies:

- strict closure records for Milestones 012–016.

Exit conditions:

- all post-closure findings have production-path evidence and regression tests;
- no unresolved critical/high/medium finding remains in addendum scope;
- broad verification failures are either fixed in scope or attributed with reproducible ownership, never described as green;
- registry, roadmap/addendum, architecture, and closure records agree.

## 5. Compatibility and migration

- No storage migration is expected for Milestones 012–015.
- Milestone 016 may add transient lineage reservation/cancellation structures but must not invent the final durable AgentRun schema.
- Native protocol changes are additive only when existing ACP events cannot provide unambiguous submission/turn correlation.
- Existing agent TOML, model-adapter TOML, TUI behavior, provider selection, and session projections remain compatible.
- If an additive correlation field is required, older consumers ignore it and ACP negotiates only behavior actually implemented.
- Existing M001–M011 closure records remain historical evidence; `011-corrective-status.md` governs current strict disposition.

## 6. Verification strategy

Verification is focused and local-first:

- each milestone adds deterministic unit tests for pure state machines/contracts;
- each milestone adds one production-shaped integration fixture for its corrected path;
- ACP tests use a real stdio process plus captured native events;
- research/security tests use local/captured evidence and mock providers, not live paid services;
- concurrency tests use barriers/semaphores and bounded Tokio fixtures rather than long stress loops;
- the final milestone runs the repository's canonical quick/focused checks and one truthful broad workspace command;
- no new release workflow, external scanner, model matrix, or platform matrix is added.

## 7. Stop conditions

Stop and report rather than expanding scope if:

- ACP correctness requires replacing canonical session projections rather than adding a bounded correlation seam;
- research coordination requires browser automation, a new crawler, persistent knowledge graph, or arbitrary recursive workflows;
- specialized finalization requires provider-specific report logic outside the existing adapter/structured-output boundary;
- prompt convergence requires redesigning provider DTOs beyond additive block/fingerprint metadata;
- reasoning safety requires exposing hidden reasoning to users or frontends;
- descendant correctness requires implementing durable AgentRun/worktree/team authorization;
- broad verification failures are unrelated and owned by the development-verification/release subsystem.

## 8. Final closure rule

The addendum may return the subsystem to `closed` only after Milestone 017 records an independent requirement-to-evidence matrix and no unresolved high or medium finding remains. Until then, the original M011 closure is conditionally accepted historical implementation evidence, not strict subsystem closure.
