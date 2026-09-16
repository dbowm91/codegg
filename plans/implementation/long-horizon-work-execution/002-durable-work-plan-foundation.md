# Long-Horizon Work Execution M002 — Durable WorkPlan Foundation

Status: blocked

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`

Primary class: infrastructure / invariant

Hard dependency: M001 closure.

## 1. Objective

Add a bounded, revisioned, restart-durable `WorkPlan`/`WorkItem` domain in `codegg-core` that can represent detailed execution state for large tasks without turning Goal, TodoState, or context checkpoints into a general workflow database.

This milestone lands storage/domain semantics only; model-facing plan tools and automatic completion arbitration belong to M003.

## 2. Why this milestone is blocked

ADR-0003 fixes the ownership decision, but ordered handoff requires M001 closure so Goal blocker/progress semantics are stable before WorkPlan binds to Goal state. Once M001 closes, no other hard dependency remains.

## 3. Current implementation evidence

- Goal is durable and revisioned but detailed completed/remaining item lists are appended into free-form `progress_summary`.
- TodoState is bounded, revisioned, and intentionally model-facing.
- AgentTask/AgentRun and scheduler Job/Attempt already own delegated execution identity; WorkPlan must reference, not replace, those types.
- Session schema has sequential migrations and multiple typed stores suitable as patterns.
- Continuation checkpoints have their own prepared/installed/aborted lifecycle and must remain distinct.

## 4. Invariants that must not regress

- WorkPlan is detailed work state, not execution authority.
- `WorkPlanId`/`WorkItemId` are distinct from `AgentTaskId`, `AgentRunId`, `JobId`, Goal ID, and Todo ID.
- Goal status/budget remains authoritative when a plan is Goal-bound.
- TodoState stays bounded and independent in storage.
- WorkPlan mutation uses revision/CAS; stale writers cannot overwrite newer state.
- Completed acceptance evidence cannot be created by arbitrary model text.
- Plan/item counts and every serialized text/list field are bounded before persistence/projection.
- No hidden reasoning is stored.

## 5. Scope

### In scope

- core typed IDs and enums;
- WorkPlan/WorkItem storage and additive migration;
- plan lifecycle/status and optional Goal/origin-turn binding;
- dependency/actionability calculation;
- bounded acceptance criteria/evidence refs/blocker fields;
- owner/run/job provenance refs;
- CAS updates and restart loading;
- minimal store/service API and tests;
- architecture docs.

### Explicitly out of scope

- model-facing WorkPlan tools;
- Todo projection/writing integration;
- completion continuation logic;
- fresh context epochs;
- arbitrary condition expressions, cron, loops, retries-as-workflow, or user automation;
- automatic plan generation for all turns;
- collaborative multi-principal plan editing UI.

## 6. Required production changes

### Core/domain

Add the domain under `crates/codegg-core` in a clearly owned module. The exact type names may vary, but the minimum semantic shape is:

```text
WorkPlan
  id, revision
  session_id, project_id
  origin_turn_id?
  goal_id?
  objective + origin digest/provenance
  status
  current_phase?
  current_item_id?
  created/updated/completed timestamps

WorkItem
  id, plan_id, revision/order
  parent_item_id?
  dependencies[]
  status
  description
  acceptance[]
  evidence[]
  owner AgentRun/Job refs?
  attempts
  blocker?
  next_action?
```

Use a small closed set of statuses sufficient for execution: pending/actionable/in-progress/blocked/completed/cancelled (exact naming may differ). Do not infer completion from dependency absence alone.

Acceptance criteria need a typed host-evidence disposition such as unmet/satisfied/requires-user-judgment plus bounded explanatory metadata. Evidence refs should point to existing canonical artifacts/jobs/runs/tests/commits where possible rather than duplicate payloads.

### Storage and migrations

Add the next sequential SQLite migration and bump the storage layout version. Prefer normalized plan/item/dependency/criterion/evidence tables or bounded JSON columns only where ownership/query behavior remains simple. Avoid one unbounded JSON blob containing the entire plan.

Store APIs must support at least:

- create active plan for an owning scope;
- load by ID and active-for-session/goal;
- list bounded items;
- update plan/item if revision matches;
- transition item status with validation;
- compute actionable items deterministically;
- cancel/complete plan through guarded transitions;
- bind/unbind optional Goal only through validated ownership;
- correlate owner run/job/evidence refs without granting authority.

Define hard bounds in domain validation, not just UI.

### Protocol and DTOs

No broad protocol is required in M002. If tests/daemon services require DTOs, add only internal/additive bounded forms; public model/frontend projection belongs to M003.

### Runtime and concurrency

No agent loop behavior change. Establish one store/service owner and CAS semantics. Simultaneous updates must return explicit stale revision/conflict rather than last-write-wins.

### Frontend or operator surface

None beyond optional diagnostics.

### Security and authorization

Store operations require owning session/project context. A referenced AgentRun/Job does not grant write permission to a plan. Validate IDs and cross-scope references.

### Documentation and static guards

Add `architecture/work_plan.md` and update Goal/Todo architecture with ownership boundaries. Add a static guard only if needed to prevent direct SQL/storage duplication outside the core owner.

## 7. Ordered work packages

### Work package A — Domain contract and bounds

Define IDs, statuses, validation rules, objective provenance, item/criterion/evidence shapes, scope/lifecycle, and serialization. Prove all bounds and invalid transitions with unit tests.

### Work package B — Additive durable store

Add migration/tables/indexes and typed store with CAS revision. Include transaction semantics for plan creation/replacement and item/dependency updates.

### Work package C — Actionability and evidence references

Implement deterministic dependency gating and canonical evidence-reference validation. Do not implement semantic criterion inference.

### Work package D — Goal binding seam

Allow optional exact Goal ID/session binding with no change to Goal runtime behavior. Replacement/cancel must not orphan authoritative identities silently.

## 8. Failure, cancellation, restart, and contention semantics

- Transaction failure leaves the prior plan revision intact.
- Stale revision returns an explicit conflict and no mutation.
- Deleting/cancelling an owning Goal does not silently delete historical WorkPlan evidence; active behavior is stopped/cancelled through explicit transition.
- Missing referenced Job/Run/Artifact is represented as unavailable evidence, not satisfied evidence.
- Restart loads durable state exactly; `in_progress` does not imply that a side effect should be replayed.
- Dependency cycles are rejected at creation/update or classified as invalid/blocked; they must never cause unbounded traversal.
- Concurrent item completion and plan cancellation serialize deterministically.

## 9. Compatibility and migration

Existing DBs gain additive empty WorkPlan tables. No existing Goal/Todo/session rows are backfilled into speculative plans. Legacy sessions simply have no active WorkPlan. Schema readers must tolerate absent optional Goal/origin-turn bindings.

## 10. Required tests

### Focused unit tests

- ID/status/transition validation;
- item/list/text bounds;
- dependency ordering and cycle rejection;
- actionability calculation;
- evidence-reference validation;
- Goal/session/project scope mismatch rejection.

### Integration tests

- create/load/update/complete/cancel plan;
- large bounded plan round trip;
- CAS conflict;
- optional Goal binding;
- owner AgentRun/Job refs remain provenance only.

### Restart and recovery tests

- reload active plan and all item revisions after DB reopen;
- in-progress item survives restart without replay semantics;
- missing evidence target remains explicit.

### Contention and cancellation tests

- concurrent writers on same item/plan;
- cancel racing completion;
- goal replacement/binding race.

### Security and negative tests

- cross-session/project references rejected;
- oversized/cyclic/malformed plans rejected before persistence;
- no hidden/model reasoning payload field exists.

### Migration and compatibility tests

- pre-migration database opens and gains empty WorkPlan tables;
- existing Goal/Todo/session data unchanged.

## 11. Required verification commands

```bash
cargo test -p codegg-core -- work_plan
cargo test -p codegg-core -- migration
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Use current equivalent targets if names change; record actual commands in closure.

## 12. Documentation updates

- new `architecture/work_plan.md`;
- `architecture/goal.md`;
- `architecture/model_profile_task_state.md`;
- `architecture/session.md` if storage ownership changes are described there.

## 13. Acceptance criteria

- WorkPlan/WorkItems survive restart with stable IDs/revisions.
- A bounded plan larger than TodoState's projection cap is valid and queryable.
- Dependencies deterministically identify actionable items.
- Stale writes cannot overwrite newer plan state.
- Evidence refs point to canonical host-owned identities or are explicitly unavailable.
- No model-visible/runtime behavior changes yet.

## 14. Stop conditions

Stop if implementation requires a general workflow scheduler, redefines AgentTask/AgentRun/Job identity, makes WorkPlan the execution authority, or requires rewriting Goal/Todo storage instead of additive integration.

## 15. Closure evidence required

- implementation/migration commits;
- schema and bound table;
- transition/actionability/CAS requirement matrix;
- migration/restart/contention test outcomes;
- architecture ownership evidence;
- exact verification commands and residual findings.

## 16. Handoff notes

Keep the first schema deliberately small. M003 can add model/frontend projections after the durable semantics are proven; avoid designing speculative UI fields into the core tables now.
