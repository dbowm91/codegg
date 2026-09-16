# ADR-0003: Host-Owned Long-Horizon Work State and Context Epochs

Status: accepted

Date: 2026-09-16

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — session, turn, agent task, agent run, job, attempt, artifact

Affected subsystem roadmaps:

- `plans/subsystems/long-horizon-work-execution-roadmap.md`
- `plans/subsystems/context-continuity-compaction-roadmap.md` (closed foundation; not reopened)

Related accepted work:

- `plans/subsystems/context-continuity-compaction-roadmap.md`
- `plans/closure/context-continuity-compaction/004-status.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- `plans/subsystems/agent-runtime-goal-verification-addendum.md`
- `architecture/context-compaction-ownership.md`
- `architecture/goal.md`
- `architecture/model_profile_task_state.md`

External design input:

- OpenAI Codex compaction and goal-continuation implementation, including structured goal state and context-window transitions.
- Anthropic, “Effective harnesses for long-running agents,” `https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents`.
- Anthropic, “Harness design for long-running application development,” `https://www.anthropic.com/engineering/harness-design-long-running-apps`.

## Context

CodeGG now has a strong transactional compaction foundation. The closed context-continuity workstream established one canonical compaction owner, durable continuation checkpoints, authoritative goal/todo projection, exact recovery references, transactional prepare/verify/install rollover, restart injection, and forced multi-compaction qualification. That work should remain closed.

The remaining long-horizon problem sits above compaction. A large coding objective can contain dozens of dependent implementation steps, validation requirements, delegated child jobs, and evidence obligations. `TodoState` is intentionally small and model-facing, while `Goal` is intentionally an objective/budget/completion surface. Neither is currently a durable detailed work graph:

- `TodoState` is bounded to a small number of items and is optimized for prompt projection and short-horizon orientation;
- `Goal` durably owns objective, phase, progress summary, next action, criteria, and open questions, but `GoalProgressUpdate.completed_items` and `remaining_items` are collapsed into free-form `progress_summary` text;
- `GoalRuntime::should_continue()` currently decides from status and remaining budget, not host-observed progress;
- the continuation prompt refers to a `Blocked` goal status and consecutive-blocker behavior that do not exist in `GoalStatus` or the runtime;
- `RecoveryController` already provides turn-local typed progress/no-progress vocabulary, but goal continuation does not reuse it;
- host-owned goal verification can reject premature completion, but ordinary long multi-step turns do not yet share a durable plan/completion contract;
- repeated compaction is safe, but some long-running model trajectories benefit from a genuinely fresh provider context reconstructed from authoritative host state rather than recursively carrying a compacted transcript forever.

A new durable generic workflow engine would violate CodeGG's non-goals. Conversely, making transcript summaries, TodoState, or the goal Markdown journal authoritative for large plans would preserve the failure mode this work is intended to eliminate.

## Decision drivers

- Compaction must remain a context-reduction mechanism, not the authority for task completion.
- Goal must remain the objective/budget/autonomous-continuation authority rather than becoming a general workflow database.
- TodoState must remain a compact model-facing projection rather than expanding into an unbounded project-management surface.
- Long plans need host-owned revisions, dependencies, acceptance criteria, blocker state, and evidence references that survive compaction and restart.
- Ordinary substantial turns and explicit autonomous goals should share completion semantics where possible.
- Existing GoalVerification, Job/Run stores, AgentRun evidence, TodoState, RecoveryController, continuation checkpoints, and artifact handles should be reused rather than duplicated.
- Fresh context epochs must never clear workspace/Git/process state, silently alter user intent, or grant new authority.
- The model may propose plan mutations, but model prose is not authoritative evidence that an acceptance criterion is satisfied.

## Considered options

### Option A — Expand TodoState into the durable long-plan database

Add dependencies, evidence, large item counts, nested tasks, attempt history, and completion criteria directly to TodoState.

Benefits:

- one familiar model-facing object;
- minimal new vocabulary.

Costs and failure modes:

- destroys TodoState's current bounded-prompt purpose;
- pushes large historical state repeatedly into model context;
- creates model-write semantics for state that should sometimes be host-owned;
- complicates model-profile policies that intentionally use sparse/current-task todo projections.

Rejected.

### Option B — Expand Goal into the detailed work graph

Add a large list of steps/dependencies/evidence directly to each Goal row and make goals the only long-horizon mechanism.

Benefits:

- reuses durable goal storage and autonomous continuation;
- no new top-level type.

Costs and failure modes:

- forces ordinary multi-step work into autonomous Goal semantics;
- mixes objective/budget/completion authority with detailed execution state;
- makes one Goal row a high-contention mutable aggregate;
- weakens the existing distinction between Goal and TodoState.

Rejected.

### Option C — Durable WorkPlan with bounded Todo projection and optional Goal binding

Introduce a host-owned, revisioned `WorkPlan`/`WorkItem` domain beneath a user objective. A WorkPlan may be turn-scoped for ordinary long work or bound to an explicit Goal for autonomous multi-turn work. TodoState remains a bounded projection of the currently actionable portion. Completion is decided from structured plan/evidence state, with existing GoalVerification patterns reused. Context compaction and fresh context epochs consume projections of this state but never own it.

Selected.

## Decision

CodeGG will adopt the following long-horizon execution contract:

1. `codegg-core` owns durable `WorkPlan` and `WorkItem` records. Stable identifiers are `WorkPlanId` and `WorkItemId`; they are distinct from `AgentTaskId`/`AgentRunId` and scheduler `JobId`.
2. A WorkPlan records session/project identity, immutable origin objective/provenance, optional origin turn, optional bound Goal ID, status, monotonic revision, current phase/current item, timestamps, and bounded plan metadata.
3. A WorkItem records parent/ordering metadata, dependencies, status, bounded description, acceptance criteria, evidence references, owner/run correlation when delegated, attempt count, blocker classification, and next action. Item counts and text sizes are explicitly bounded.
4. The first implementation is not a general DAG workflow engine. Dependencies only gate actionability/completion. There is no cron, arbitrary condition language, distributed transaction graph, or generic user automation runtime.
5. TodoState remains the model-facing short-horizon projection. It may project the current WorkItem and a bounded number of next actionable items. Todo writes cannot silently rewrite immutable objective, host evidence, dependencies, or completed acceptance predicates.
6. Explicit Goal mode may bind one active WorkPlan. Goal continues to own budget, pause/resume, status, and autonomous continuation. WorkPlan owns detailed execution progress. Goal completion requires the bound WorkPlan to have no actionable/unmet work in addition to existing GoalVerification requirements.
7. Ordinary substantial turns may use a turn-scoped WorkPlan without enabling Goal auto-continuation. Before accepting a model final answer, the host may inject a bounded continuation/replan when the WorkPlan proves actionable required work remains. Existing agent turn/tool/time budgets remain authoritative.
8. Completion evidence is host-correlated. Model-provided summaries, file names, or claimed tests may explain progress but do not satisfy acceptance criteria unless a canonical host-owned source establishes the fact or the criterion explicitly requires user judgment.
9. Long-horizon progress reuses the semantics of `ProgressSignal`/`RecoveryController` rather than creating a second generic recovery framework. The durable layer may compute a compact progress fingerprint from WorkPlan revision, Todo revision, goal revision, completed/failed job/test state, workspace mutation generation, and evidence additions.
10. Autonomous continuation distinguishes at least `Progress`, `VerifiedWait`, and `NoProgress`. A live scheduler/process/agent handle may justify a verified wait; repeating the same model reasoning without authoritative state change does not.
11. Repeated no-progress continuations follow bounded recovery: nudge/correct or replan, then transition the owning Goal to existing `AwaitingUser` when a genuine unresolved blocker persists. This ADR does not add a duplicate `Blocked` Goal status.
12. The existing continuation checkpoint remains a bounded handoff, not WorkPlan storage. Checkpoints capture or reference the current WorkPlan ID/revision and bounded active-work projection, and rollover revalidates the revision before install.
13. CodeGG may begin a fresh provider-visible **context epoch** at a verified phase boundary, repeated-compaction threshold, or model-profile/policy decision. A fresh epoch is rebuilt from immutable prompt/runtime instructions, current Goal/WorkPlan/Todo projections, installed continuation state, bounded recent user steering, and recovery handles. It does not reset workspace, Git, jobs, budgets, permissions, or durable session history.
14. Context-epoch reset is model/profile aware and optional. It is not a universal “reset every N turns” rule. Provider/model behavior and observed continuity determine whether it is useful.
15. Frontends render plan/progress projections but do not own WorkPlan state or determine completion.

## Consequences

### Positive

- Multi-compaction work retains structured execution state independently of transcript summaries.
- Todo prompts can stay small even when the full plan is large.
- Goal continuation can stop/replan on host-observed stagnation rather than consuming up to 32 useless turns.
- Ordinary long turns and autonomous Goals can share one “work remains / work satisfied” contract.
- Plan evidence can point directly to existing jobs, runs, tests, artifacts, commits, or host observations.
- Fresh context epochs become safe because the important state is reconstructible from host-owned records.

### Negative

- A new durable domain requires additive storage, migrations, retention, protocol projections, and CAS/restart tests.
- Synchronization between WorkPlan and Todo projection must avoid dual authority.
- Completion arbitration becomes more explicit and may expose previously hidden premature-finish behavior.

### Neutral or deferred

- Collaborative multi-principal editing of one WorkPlan is deferred; the initial store uses revision/CAS semantics.
- Distributed coordinator replication of WorkPlans follows the same future ownership work as sessions/agent runs and is not introduced here.
- Arbitrary user-defined workflows, conditional branches, recurring automation, and project-management UI are out of scope.
- A future UI may expose full WorkPlan editing, but model/TUI projections are initially sufficient.

## Compatibility and migration

- Existing sessions, Goals, todos, and continuation checkpoints remain readable without WorkPlans.
- WorkPlan storage is additive; absence means legacy/current behavior.
- Goal rows may gain an optional WorkPlan reference or use a relation table; existing Goal IDs/status semantics do not change.
- Existing TodoState wire/tool contracts remain available. New projection metadata must be additive.
- Existing continuation-checkpoint schema evolves additively or through the next version with backward-readable prior checkpoints.
- No closed context-continuity milestone is reopened. New epoch reset behavior must call the existing compaction/rollover owners rather than fork them.

## Security and reliability implications

- Plan text and evidence inherit session sensitivity and must obey existing size/redaction/artifact rules.
- WorkItem ownership cannot expand child/subagent authority; owner/run references are provenance only.
- Model plan mutations are validated, bounded, revision-checked, and cannot fabricate host evidence.
- Restart reconstructs active work from durable WorkPlan/Goal/Todo/job state; prepared/uncommitted compaction checkpoints never become plan authority.
- Concurrent plan updates use monotonic revision/CAS and return explicit stale-state diagnostics.
- Context reset never changes approval/sandbox/model-selection authority.

## Verification

Conformance requires evidence that:

- a WorkPlan with many dependent items survives restart and at least eight forced compactions without losing objective, actionable work, blocker state, or acceptance evidence;
- Todo projection remains bounded while the durable plan is larger;
- a model final answer cannot close a plan with unmet actionable required items;
- a completed plan does not trigger spurious continuation;
- repeated no-progress Goal continuations replan and then enter `AwaitingUser` without a nonexistent `Blocked` status;
- a verified live wait does not restart or duplicate the awaited operation;
- host-owned test/job/run evidence cannot be forged by model text;
- a fresh context epoch reconstructs the same authoritative WorkPlan/Goal trajectory and preserves later user steering;
- stale WorkPlan revisions cannot overwrite newer progress.

## Supersession

None.
