# Long-Horizon Work Execution Roadmap

Status: active

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Foundation that remains closed:

- `plans/subsystems/context-continuity-compaction-roadmap.md`
- `plans/closure/context-continuity-compaction/004-status.md`
- `architecture/context-compaction-ownership.md`
- `architecture/goal.md`
- `architecture/model_profile_task_state.md`

External research basis:

- OpenAI Codex current compaction/goal continuation implementation and public failure reports around repeated compaction and autonomous waiting.
- Anthropic, “Effective harnesses for long-running agents,” emphasizing external structured task state, incremental progress, and verification.
- Anthropic, “Harness design for long-running application development,” describing structured handoff and context-reset behavior for long trajectories.

## 1. Purpose and ownership boundary

This workstream improves CodeGG's ability to execute large coding plans over many tool calls, continuations, compactions, subagent jobs, and restarts without losing trajectory or declaring completion prematurely.

It owns:

- correctness of Goal continuation progress/blocker decisions;
- durable detailed `WorkPlan`/`WorkItem` state beneath an objective;
- bounded Todo projection from that durable plan;
- host-owned completion arbitration for ordinary long work and Goal-bound work;
- integration of WorkPlan state into the already-closed continuation-checkpoint model;
- optional fresh provider context epochs reconstructed from authoritative state;
- long-horizon trajectory/fault qualification.

It consumes but does not redefine:

- canonical context reduction and rollover (`src/context`);
- Goal budget/status/verification authority;
- Todo model-profile policy;
- scheduler/AgentRun/Worktree ownership;
- provider transport/retry policy;
- approval/sandbox authority;
- Git integration and test execution.

The governing rule is:

> A model context is a projection of the work; it is never the sole authority for what work remains or whether a large plan is complete.

## 2. Work classification

### Invariants

- `src/context/compaction.rs` and `src/context/rollover.rs` remain the canonical compaction/rollover owners.
- An installed continuation checkpoint remains a bounded handoff, not a second plan database.
- Goal remains authoritative for autonomous-goal status/budget; WorkPlan cannot silently bypass pause/cancel/budget state.
- TodoState remains bounded and model-facing; it is not expanded into an unbounded durable project graph.
- Host-owned evidence, not model prose, satisfies deterministic acceptance criteria.
- Fresh context epochs never clear or recreate workspace/Git/job state and never grant authority.
- No-progress recovery is bounded and cannot consume unbounded continuation turns.
- Existing agent-run/job/worktree identities are referenced for provenance; WorkPlan does not create duplicate execution ownership.

### Capabilities

- A large implementation plan can survive multiple compactions and restart while preserving objective, current work, dependencies, blockers, evidence, and next action.
- The primary agent can finish a long todo/plan without repeatedly rediscovering completed work.
- Autonomous Goals stop/replan when no authoritative progress is occurring instead of blindly continuing while budget remains.
- Ordinary long work receives a completion check without requiring the user to opt into Goal mode.
- Models that benefit from a clean prompt can start a new context epoch from a structured handoff at safe boundaries.

### Infrastructure

- Bounded revisioned WorkPlan/WorkItem storage in `codegg-core`.
- WorkPlan-to-Todo and WorkPlan-to-continuation projections.
- Reusable completion/progress fingerprinting derived from existing stores.
- Context-epoch policy and reconstruction using existing prompt/context owners.

### Polish

- TUI/status projection of active phase/current work/blocker state.
- Diagnostics describing why continuation stopped, replanned, waited, or entered `AwaitingUser`.
- Architecture documentation and static guards preventing duplicate compaction/plan authority.

## 3. Non-goals

This roadmap does not authorize:

- a general workflow engine, BPM system, cron engine, or arbitrary condition language;
- a vector database or second conversation-history store;
- rewriting the closed context-continuity implementation;
- replacing Goal, TodoState, AgentTask/AgentRun, scheduler Job, or Worktree concepts;
- a full project-management UI;
- automatic generation of a WorkPlan for every trivial question or edit;
- unbounded plan item counts or model-visible plan dumps;
- storing hidden chain-of-thought;
- model-only completion certification;
- model-independent deterministic decomposition of every user task.

## 4. Current state

The baseline contains strong continuity machinery but several long-horizon gaps:

1. The context-continuity M001-M004 workstream is closed. It already provides durable continuation checkpoints, authoritative Goal/Todo projection, exact recovery references, transactional rollover, restart injection, and forced eight-compaction qualification. New work must build on it.
2. `Goal` owns objective, status, budget, phase, progress summary, next action, completion criteria, and open questions. `GoalProgressUpdate` accepts `completed_items` and `remaining_items`, but `GoalStore::update_progress_with_revision()` appends those names into free-form `progress_summary`; they are not durable structured current-state fields.
3. `GoalRuntime::should_continue()` returns Continue whenever the Goal is Active and budget remains. It has no host-observed progress/wait/stall predicate.
4. The Goal continuation prompt says the runtime counts consecutive blocked turns before allowing a `Blocked` status. `GoalStatus` has no `Blocked` variant and no such runtime transition.
5. `maybe_continue_goal()` is guarded by `MAX_CONTINUATIONS = 32`; this prevents infinity but can still spend many turns on a stalled trajectory.
6. `RecoveryController` already identifies `NewEvidence`, `StateChanged`, `ChildAdvanced`, no-progress incidents, bounded correction/replan, and Stall. This should be reused for long-horizon semantics instead of inventing a parallel recovery framework.
7. TodoState provides exactly the right bounded prompt-oriented behavior but is intentionally capped and unsuitable as the full source of truth for a very large plan.
8. Goal completion verification already uses host-owned jobs/todos/evidence and refuses to trust claimed tests/files. That verifier pattern is the correct basis for long-plan completion arbitration.
9. Current continuation checkpoints can preserve plan-path/digest and Todo/Goal state but do not have a distinct durable WorkPlan revision to revalidate.
10. Context rollover preserves continuity, but CodeGG lacks an explicit production policy for starting a genuinely fresh provider-visible context epoch after a verified phase boundary or repeated compactions.

## 5. Target architecture

```text
immutable user objective / provenance
              |
              v
          WorkPlan (durable)
          revision + status
              |
      +-------+---------+
      |                 |
      v                 v
 WorkItems          Goal binding (optional)
 deps/status/       budget/status/continuation
 acceptance/evidence
      |
      +-------> bounded Todo projection
      |
      +-------> completion/progress arbiter
      |
      +-------> continuation checkpoint projection
                         |
                         v
                   provider context epoch
```

### 5.1 WorkPlan scope

A WorkPlan belongs to one session/project and records immutable objective provenance. It may be:

- turn-scoped for an ordinary substantial user task; or
- Goal-bound for explicit autonomous multi-turn work.

Only one active plan per owning scope is allowed. Replacement/cancel is explicit and revisioned.

### 5.2 WorkItem semantics

The initial WorkItem contract is intentionally small: bounded description, parent/order, dependencies, status, acceptance criteria, evidence refs, optional owner/run correlation, attempts, blocker, and next action. Dependencies gate actionability; they do not form a generic rule engine.

### 5.3 Projection rather than duplication

TodoState projects active/next WorkItems according to the resolved model's task-state policy. Completion and evidence fields remain WorkPlan/host-owned. Model todo edits can update allowed short-horizon fields but cannot manufacture completed host criteria or rewrite objective/dependencies without a validated WorkPlan mutation.

### 5.4 Completion arbitration

Before accepting a terminal model answer for an active long plan, the host examines current plan and canonical evidence. If required actionable work remains, the loop gets a bounded continuation/replan instruction. If only user-judgment criteria remain, it exits to the user. Goal-bound plans additionally pass existing GoalVerification before Goal completion.

### 5.5 Progress and waiting

Progress classification uses existing recovery vocabulary plus durable revisions. `VerifiedWait` is allowed only when a named canonical job/process/agent handle is live and the next safe action is to await/poll that same handle. Repeated equivalent no-progress states trigger bounded recovery and eventually `AwaitingUser` for a Goal or terminal blocked report for an ordinary turn.

### 5.6 Context epochs

A context epoch is provider-visible context, not durable task state. At a safe policy-selected boundary the next epoch is rebuilt from canonical system/runtime instructions, objective, WorkPlan/Goal/Todo projections, installed continuation checkpoint, bounded latest user steering, and exact recovery handles. Epoch reset never rewrites durable session history or execution state.

## 6. Dependency graph

```text
M001 Goal continuation progress correctness
      |
      +-------------------+
      |                   |
      v                   v
M002 durable WorkPlan foundation
      |
      v
M003 projection + completion arbiter
      |
      v
M004 context-epoch reset/handoff integration
      |
      v
M005 long-horizon trajectory qualification
```

Dependency classification:

- M001 has no hard predecessor beyond the closed Goal/runtime foundations and is dependency-ready.
- M002 has an interface dependency on ADR-0003 and closed session/storage infrastructure; it may proceed after M001 starts, but closure sequencing is M001 then M002.
- M003 has a hard dependency on M002.
- M004 has hard dependencies on M003 and closed context-continuity M004.
- M005 has hard dependencies on M001-M004.

## 7. Milestones

### M001 — Goal progress, waiting, and continuation correctness

Class: invariant/capability

Plan: `plans/implementation/long-horizon-work-execution/001-goal-progress-and-continuation-correctness.md`

Status: ready.

Correct the nonexistent Blocked-status prompt contract, compute host-owned progress/wait/no-progress signals for Goal continuation, reuse existing recovery semantics, and bound replan/AwaitingUser transitions.

Exit condition: an Active Goal continues only while useful progress or a verified live wait exists; repeated stagnation cannot consume the full 32-turn guard without an earlier recovery/stop transition.

### M002 — Durable WorkPlan foundation

Class: infrastructure/invariant

Plan: `plans/implementation/long-horizon-work-execution/002-durable-work-plan-foundation.md`

Status: blocked on M001 closure for ordered handoff.

Add bounded WorkPlan/WorkItem types, additive storage/migration, revision/CAS, lifecycle semantics, optional Goal binding, and provenance/evidence reference shape without changing model-visible behavior yet.

Exit condition: a large bounded plan can be created, updated, reloaded after restart, dependency-filtered for actionable items, and protected from stale concurrent mutation.

### M003 — WorkPlan projection and completion arbitration

Class: capability

Plan: `plans/implementation/long-horizon-work-execution/003-work-plan-projection-and-completion-arbiter.md`

Status: blocked on M002.

Expose bounded model-facing plan operations/projections, integrate TodoState as short-horizon projection, connect canonical evidence, and prevent premature final completion when required actionable items remain.

Exit condition: ordinary long work and Goal-bound work share one authoritative work-remaining test while Todo context stays bounded.

### M004 — Context-epoch reset and structured handoff integration

Class: infrastructure/capability

Plan: `plans/implementation/long-horizon-work-execution/004-context-epoch-reset-and-handoff-integration.md`

Status: blocked on M003.

Extend continuation checkpoints with WorkPlan identity/revision projection and add an optional model-profile/policy-aware fresh context epoch path using the existing context/rollover owners.

Exit condition: a safe phase/compaction boundary can rebuild a fresh provider context from authoritative state without losing later steering, plan state, evidence handles, or execution authority.

### M005 — Long-horizon trajectory and recovery qualification

Class: invariant/closure

Plan: `plans/implementation/long-horizon-work-execution/005-long-horizon-trajectory-and-recovery-qualification.md`

Status: blocked on M001-M004.

Exercise large plans across compaction, epoch reset, restart, waits, failures, subagent/test evidence, and premature-finish attempts using deterministic/scripted harnesses.

Exit condition: closure evidence demonstrates coherent completion of a representative long plan across repeated context transitions without duplicate work, lost required items, false completion, or unbounded continuation.

## 8. Cross-cutting requirements

### Storage and migration

- Use additive sequential migrations in the existing core session/storage schema.
- Bound item count, description, acceptance/evidence list size, and aggregate serialized payload.
- Use CAS revision for plan/item mutation and explicit stale diagnostics.
- Existing databases without WorkPlans load normally.

### Protocol and compatibility

- Any plan snapshot/event added to core protocol is additive and bounded.
- TUI/ACP clients that ignore WorkPlan fields remain functional.
- Existing Goal and Todo tools continue to work during migration.

### Security and authorization

- Plan mutation uses the owning session/principal/agent authority; a WorkPlan reference never grants a tool capability.
- Evidence references are redacted/handle-based and cannot expose hidden reasoning or credentials.
- A delegated child may report against assigned WorkItems but cannot alter sibling/parent plan authority beyond its host-granted contract.

### Concurrency, cancellation, and recovery

- Goal pause/cancel/replacement immediately stops autonomous continuation regardless of WorkPlan state.
- Cancellation does not mark unfinished items complete.
- Restart reconstructs from durable revisions and canonical job/run state; it does not replay a completed non-idempotent operation merely because an item was InProgress.
- A stale context checkpoint cannot install a superseded WorkPlan revision as current.

### Observability and audit

- Record bounded reason codes for Continue, VerifiedWait, Replan, AwaitingUser, completion accepted/rejected, epoch reset, and stale revision.
- Do not log full sensitive plan/evidence content by default.

### Performance and resource use

- WorkPlan model projection is bounded independently of total plan size.
- No extra model call is required merely to read host completion state.
- Context epoch reset must not become a periodic unconditional summarizer call.

### Documentation and operations

Update `architecture/goal.md`, `architecture/model_profile_task_state.md`, `architecture/agent.md`, `architecture/context-compaction-ownership.md` (integration only), and add a WorkPlan architecture document once production types land.

## 9. Verification strategy

Subsystem closure requires:

- focused WorkPlan model/store/CAS tests;
- Goal continuation progress/wait/no-progress tests;
- Todo projection bounds and model-profile tests;
- completion-arbiter tests against unfinished, blocked, failed-test, passing-test, and user-judgment states;
- restart tests around active WorkItems and Goal binding;
- multi-compaction and context-epoch tests preserving objective/revision/steering/evidence;
- concurrency tests for stale plan revisions and child reporting;
- cancellation tests proving unfinished work is not converted to success;
- static guards confirming no second compaction owner/history store/general workflow scheduler is introduced;
- existing `scripts/verify.sh quick` or the repository's then-current proportional verification, without adding a broad new CI matrix.

## 10. Risks and decision points

- If WorkPlan becomes a generic arbitrary workflow graph, stop and revisit ADR-0003 rather than expanding scope.
- If Goal/WorkPlan relation requires changing Goal identity/status compatibility, record a new ADR or explicit amendment before implementation.
- If fresh epochs require provider-private state that cannot be represented through current context owners, keep the capability disabled for that provider rather than introducing a provider-specific second history model.
- If host completion evidence cannot decide a natural-language criterion, return user judgment/inconclusive rather than model inference.

## 11. Completion definition

This roadmap closes only when:

- Goal continuation has truthful bounded progress/blocker behavior;
- detailed long-plan state is durable and distinct from Todo/Goal/transcript authority;
- Todo projection remains bounded;
- ordinary and Goal-bound long work cannot silently finish with actionable unmet required items;
- fresh context epochs, where enabled, reconstruct from authoritative host state;
- representative long-running work survives restart and repeated context transitions with deterministic evidence;
- no new workflow engine or second compaction/history authority was introduced.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready | `plans/implementation/long-horizon-work-execution/001-goal-progress-and-continuation-correctness.md` | — | — |
| M002 | blocked | `plans/implementation/long-horizon-work-execution/002-durable-work-plan-foundation.md` | — | M001 closure |
| M003 | blocked | `plans/implementation/long-horizon-work-execution/003-work-plan-projection-and-completion-arbiter.md` | — | M002 closure |
| M004 | blocked | `plans/implementation/long-horizon-work-execution/004-context-epoch-reset-and-handoff-integration.md` | — | M003 + closed context-continuity foundation |
| M005 | blocked | `plans/implementation/long-horizon-work-execution/005-long-horizon-trajectory-and-recovery-qualification.md` | — | M001-M004 closure |
