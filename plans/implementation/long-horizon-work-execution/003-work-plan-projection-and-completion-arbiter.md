# Long-Horizon Work Execution M003 — WorkPlan Projection and Completion Arbiter

Status: ready for handoff

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`

Primary class: capability

Hard dependency: M002 closure.

## 1. Objective

Make the durable WorkPlan usable by the primary agent without flooding context: expose bounded plan read/update operations, project current/actionable work into TodoState, correlate host evidence, and add a host-owned completion arbiter that prevents an ordinary or Goal-bound long task from silently ending while required actionable work remains.

## 2. Why this milestone is blocked

M002 must first establish stable WorkPlan/WorkItem storage, validation, revisions, and actionability. This milestone may refine model-facing mechanics but must not invent a second plan representation before the durable owner closes.

## 3. Current implementation evidence

- `TodoWriteTool`/`TodoReadTool` already provide bounded model-facing task state controlled by model profiles and injection cadence.
- Goal runtime and continuation inject bounded current objective/progress state.
- GoalVerification already assembles canonical job/todo evidence and treats model `tests_run`/`files_changed` as explanatory claims rather than authority.
- Agent loop already has follow-up/recovery paths capable of issuing a bounded continuation/correction when a provider attempts to finish prematurely.
- Context projection/artifact handles support progressive disclosure for large evidence.

## 4. Invariants that must not regress

- WorkPlan is authoritative detailed plan state; TodoState is a projection.
- Model-facing tools cannot mark host-only evidence satisfied by assertion.
- Todo injection remains bounded according to TaskStatePolicy.
- The completion arbiter cannot force unbounded continuation; existing turn/tool/time/token/recovery limits still apply.
- Natural-language/user-judgment criteria are never guessed deterministically.
- Goal completion still requires GoalVerification; the WorkPlan arbiter is an additional prerequisite, not a replacement.
- Subagent/Job/Run ownership and scheduler authority are unchanged.

## 5. Scope

### In scope

- bounded `work_plan_get` and plan/item update tool surface or equivalent;
- model-visible projection of current phase/current item/next actionable items;
- WorkPlan-to-Todo synchronization with one-way authority rules;
- host evidence attachment/update seams;
- ordinary-turn and Goal-bound completion arbiter;
- bounded “required work remains” continuation/replan feedback;
- events/projections/docs/tests.

### Explicitly out of scope

- full plan editor UI;
- arbitrary model rewrite of dependencies/objective/evidence;
- automatic semantic proof of natural-language criteria;
- fresh context epoch/reset (M004);
- retry/permission architecture;
- general workflow scheduling.

## 6. Required production changes

### Core/domain

Add a pure/read-only `WorkPlanCompletionAssessment` derived from plan/items plus canonical evidence. At minimum distinguish:

- `Complete` — all required items/criteria satisfied;
- `ActionableWorkRemaining` — host-known required work can proceed;
- `Blocked` — required work cannot proceed without a named unresolved blocker;
- `AwaitingUserJudgment` — only criteria requiring human/semantic judgment remain;
- `InFlight` — canonical work is still running/waitable.

`Blocked` here is a WorkPlan assessment/item state, not a new GoalStatus.

### Model-facing tools and projections

Expose a compact read surface that defaults to current/actionable items and supports bounded pagination/lookup for larger plans. The model should not receive all completed history on every turn.

Mutation API must:

- require expected plan/item revision;
- validate status/dependency transitions;
- permit progress notes/next action/blocker updates within policy;
- forbid direct fabrication of host evidence or immutable objective provenance;
- return stale conflicts clearly.

### Todo integration

Define one authoritative direction:

- WorkPlan selects current/actionable items;
- TodoState projects a small subset according to the resolved TaskStatePolicy;
- permitted Todo status changes may be translated back into validated WorkPlan updates only when item identity/revision mapping is exact;
- completed host-only acceptance criteria are never inferred from a Todo `completed` flag alone.

Do not run an eventually-consistent two-way free-form sync.

### Completion arbiter

Integrate at the terminal-answer boundary before the agent/goal is considered done. When assessment says ActionableWorkRemaining, inject one bounded control message describing current item, unmet condition, and next action. Feed repeated failure/no-progress through existing AutonomyState/RecoveryController instead of recursively continuing forever.

For Goal-bound plans:

1. WorkPlan assessment must be Complete/AwaitingUserJudgment as appropriate;
2. GoalVerification remains authoritative for Goal status transition;
3. verifier NotMet evidence may create/update an actionable WorkItem when a bounded existing item mapping exists, or return bounded feedback without mutating the plan if not.

For ordinary turn-scoped plans, completion of the turn closes/completes the WorkPlan only after the host arbiter passes. If runtime budgets expire first, preserve remaining state/report rather than mark complete.

### Protocol/frontends

Add bounded WorkPlan snapshot/progress fields/events if useful for TUI status. Frontends remain read/projection consumers; model tools/daemon service own mutations.

### Security and authorization

Plan mutation respects owning principal/session/agent. Child AgentRuns can report only against host-assigned items/capabilities. Evidence refs are validated against scope/goal/run provenance.

### Documentation and static guards

Update WorkPlan, Goal, Todo, agent architecture. Add guards/tests preventing direct model mutation of host evidence and unbounded full-plan injection.

## 7. Ordered work packages

### Work package A — Read/update service and model tools

Implement bounded plan retrieval, item lookup, revision-checked allowed mutation, schemas/descriptions, and deterministic errors.

Acceptance evidence: plans much larger than Todo cap remain usable without full dump; stale updates fail.

### Work package B — Todo projection contract

Map WorkPlan active/actionable items into existing TodoState policy and establish exact identity/revision mapping for permitted feedback.

Acceptance evidence: GuidedCurrentTask/SparsePlan/ExplicitTodo stay within their existing bounds and cannot erase plan history.

### Work package C — Host evidence integration

Create narrow adapters from existing GoalVerification/job/run/test/artifact evidence into WorkItem acceptance/evidence refs. Avoid duplicating stores or parsing final prose.

Acceptance evidence: passing/failed/in-flight canonical tests produce correct assessment; claimed test text alone does not.

### Work package D — Completion arbiter

Hook terminal-answer/Goal-completion boundaries. Provide bounded continuation/replan feedback and integrate with existing recovery limits.

Acceptance evidence: unfinished required plan cannot be marked complete; truly complete plan exits without extra model turn.

## 8. Failure, cancellation, restart, and contention semantics

- Stale model plan writes fail without silently reapplying to newer plan revision.
- If plan/evidence read fails at completion, do not mark complete; surface typed inconclusive/storage error.
- User cancel/Goal pause cancels continuation but preserves durable remaining items.
- If an InFlight job becomes terminal between assessment and next turn, reload and use terminal evidence rather than duplicate work.
- Multiple children reporting the same item use revision/CAS and host ownership; duplicate completion is idempotent where evidence identity matches.
- Restart reconstructs projection from WorkPlan/Todo stores; transcript summary is not used to rediscover item state.

## 9. Compatibility and migration

- Existing sessions without WorkPlan use existing behavior.
- Existing Todo tools remain available; WorkPlan-aware sessions add identity metadata/projection rather than changing public names unnecessarily.
- Goal completion behavior for Goals without a bound WorkPlan remains the existing verifier path.

## 10. Required tests

### Focused unit tests

- completion assessment state matrix;
- bounded/paginated projection;
- WorkPlan-to-Todo policy for all Todo modes;
- invalid/stale model mutations;
- evidence authority rules.

### Integration tests

- ordinary long turn attempts early final answer -> arbiter continues -> plan completes;
- Goal-bound plan completion requires both WorkPlan and Goal verifier;
- failed test remains unmet/actionable;
- in-flight job yields InFlight/verified-wait behavior;
- user-judgment criterion returns control without fabricated success.

### Restart and recovery tests

- projection reconstructed after daemon restart;
- active current item and remaining evidence preserved across resume.

### Contention and cancellation tests

- child result race with parent update;
- user cancellation during arbiter continuation;
- stale Todo mapping cannot mutate newer WorkItem.

### Security and negative tests

- model cannot submit arbitrary evidence as host-satisfied;
- child cannot update unassigned sibling item;
- projection cannot exceed configured byte/item cap.

### Migration and compatibility tests

- WorkPlan-absent session/Goal follows legacy behavior.

## 11. Required verification commands

```bash
cargo test -p codegg-core -- work_plan
cargo test --test agent_loop_harness -- todo
cargo test --test goal_verification
cargo test --test agent_loop_harness -- completion
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Record exact current equivalents in closure.

## 12. Documentation updates

- `architecture/work_plan.md`
- `architecture/model_profile_task_state.md`
- `architecture/goal.md`
- `architecture/agent.md`
- model tool documentation/catalog as appropriate.

## 13. Acceptance criteria

- A durable plan can exceed Todo projection size without context bloat.
- Model can inspect/update the actionable plan through bounded tools.
- Todo remains a compact projection, not a second plan authority.
- Host evidence controls deterministic acceptance.
- Final-answer boundary detects required actionable work and continues/replans within existing bounds.
- Complete plans do not receive needless continuation.
- GoalVerification remains final authority for Goal completion.

## 14. Stop conditions

Stop if implementation requires automatic semantic proof of arbitrary criteria, duplicate job/run stores, an unbounded plan prompt, a generic workflow scheduler, or weakening GoalVerification.

## 15. Closure evidence required

- implementation commits and tool schemas;
- plan/Todo authority matrix;
- completion-assessment evidence matrix;
- early-final regression test;
- restart/contention/security results;
- exact verification commands and residual findings.

## 16. Handoff notes

Prefer a narrow model-facing tool set. If existing Todo tools can carry current-item feedback cleanly, avoid proliferating verbs; nevertheless, durable WorkPlan reads/updates must remain explicit enough that the model can recover after context loss without parsing Todo prose.
