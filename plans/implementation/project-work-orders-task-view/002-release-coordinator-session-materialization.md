# Project Work Orders and Task View M002 — Release Coordinator, Session Materialization, and Workspace Isolation

Status: blocked

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADRs:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: capability / invariant

Hard dependency: M001 closure.

## 1. Objective

Turn ready WorkOrder occurrences into ordinary CodeGG sessions exactly once, through a small daemon-owned release coordinator that reuses the existing global scheduler, managed worktree service, session creation path, model/runtime-preference resolution, authorization ceilings, and `JobSubmissionService`.

M002 establishes execution semantics for immediate, delay, not-before, sequence-ready, and finite-repeat WorkOrders. It does not add the new TUI yet.

## 2. Current implementation evidence

- The global scheduler is already the sole durable admission authority.
- `JobSubmissionService` validates/persists/enqueues jobs and wakes the scheduler.
- `JobKind::AgentTurn` exists and ordinary sessions already create/execute turns through canonical runtime ownership.
- `WorktreeService` owns managed worktree records, leases, restart reconciliation, dirty/conflict retention, and mutation-capable child isolation.
- Session creation can atomically bind project/workspace identity.
- Runtime preference/model selection is daemon-owned and stable-model identity is re-resolved against the provider catalog.
- Approval mode and sandbox profile are separate, snapshot-based, and can only narrow under project/parent policy ceilings.
- Scheduler/job restart recovery already distinguishes safe repeat from non-idempotent/destructive work.

## 3. Invariants that must not regress

- `WorkOrderCoordinator` is a readiness/materialization coordinator, not a scheduler or executor.
- Every initial turn still enters `JobSubmissionService` and the existing global scheduler.
- One occurrence creates at most one canonical Session and one canonical initial AgentTurn submission.
- Restart/duplicate wake cannot duplicate a session/job for the same occurrence.
- Running WorkOrder sessions are ordinary sessions; all steering, permission, question, cancel, observation, WorkPlan, and continuation behavior stays on existing paths.
- Model/approval/sandbox/workspace requests are revalidated at execution and cannot widen current policy.
- Managed worktree allocation is lazy at occurrence claim/start, not at WorkOrder authoring time.
- Non-Git mutation is not claimed to be isolated.
- Failed/cancelled/needs-attention predecessors hold sequence by default.

## 4. Scope

### In scope

- release-gate evaluator and persisted gate latching;
- `next_check_at`/timer wake integration;
- sequence-ready evaluation;
- finite-repeat occurrence creation;
- exactly-once claim/materialization state machine;
- project/workspace/repository resolution;
- `AutoIsolated`/shared-safe workspace policy resolution;
- managed worktree acquisition for Git mutation tasks;
- canonical session creation and origin attribution;
- model/provider/approval/sandbox re-resolution and attention states;
- initial AgentTurn job submission through existing scheduler boundary;
- occurrence→session/job/workspace/worktree linkage;
- cancellation propagation and sequence progression/hold;
- restart reconciliation for every partial materialization stage;
- structural events/diagnostics and architecture docs;
- fault-injection tests around the materialization transaction boundary.

### Explicitly out of scope

- Task composer/project Task UI;
- global Workspace dashboard;
- external HTTP trigger endpoint (the gate shape may remain unsatisfied until M005);
- model-visible WorkOrder creation tool;
- automatic merge/application of worktree changes;
- new non-Git copy/overlay isolation backend;
- general cron/expression language.

## 5. WorkOrderCoordinator ownership

Add one daemon-owned coordinator, ideally near existing scheduler/project services, with explicit inputs:

- WorkOrder store/service;
- SessionStore/project binding service;
- project/workspace/repository resolvers;
- WorktreeService;
- runtime preference/model selection services;
- approval/sandbox policy resolver;
- JobSubmissionService/scheduler wake handle;
- audit/event publisher.

It may react to:

- timer expiry/`next_check_at`;
- WorkOrder create/update/reorder;
- predecessor occurrence terminal transition;
- external gate latch from M005;
- daemon startup/reconnect reconciliation.

It must not poll every WorkOrder on every scheduler tick. Maintain an indexed/bounded due set or `next_check_at` query and explicit wakes.

## 6. Gate evaluation semantics

Implement deterministic evaluation over M001's closed gate set.

### Immediate / zero delay

Satisfied immediately after WorkOrder/occurrence activation.

### Delay

Anchor is explicit. For first occurrence use creation/activation semantics defined in M001; for repeated occurrences, use prior terminal/release timestamp according to repeat policy. Persist the calculated deadline so restart does not recalculate against a new clock origin.

### NotBefore

Satisfied once daemon wall clock reaches the persisted timestamp. Clock going backwards must not un-satisfy a latched gate.

### SequenceReady

Satisfied when all earlier lane members required by the lane continuation policy are terminal/successful enough to advance. Default policy holds on failed/cancelled/needs-attention predecessor.

### ExternalTrigger

M002 understands the latch field but does not expose the network endpoint. Tests may satisfy it through an internal service method.

### All/Any

All enabled gates use one join policy. Gate latches are per occurrence. Once an occurrence has moved to claiming/terminal state, later gate changes cannot create another execution.

## 7. Claim/materialization state machine

Define a restart-safe monotonic state machine. Suggested phases:

```text
waiting -> ready -> claiming -> workspace_preparing -> session_created
        -> job_submitted -> running -> terminal/needs_attention
```

The exact persisted decomposition may be smaller, but closure evidence must prove recovery from these externally meaningful boundaries.

Preferred approach:

1. atomically CAS occurrence `waiting/ready -> claiming` with a claim generation/idempotency key;
2. resolve current executable policy/model/workspace;
3. allocate/lease managed worktree when required and persist its canonical ID;
4. create the canonical session with a deterministic occurrence-derived idempotency key or recoverable linkage;
5. persist session link before submitting the initial turn;
6. submit one AgentTurn with deterministic submission key derived from occurrence/session;
7. persist job link and advance to running/materialized state;
8. on restart, reconcile each partial state by querying canonical stores before creating anything new.

Do not require one giant SQLite transaction across filesystem Git worktree creation and job execution. Use durable intent + idempotent reconciliation around side effects.

## 8. Workspace policy

Define at least:

- `AutoIsolated` — default for Task mode;
- `SharedReadOnly` or equivalent safe sharing;
- `SharedSerialized` for mutation where scheduler contention ensures one writer;
- optional explicitly unsafe shared-write mode only if existing policy/warnings can represent it truthfully.

For a local Git repository with mutation-capable execution, `AutoIsolated` acquires a managed worktree using the current WorktreeService lifecycle and branch/base semantics. The resulting workspace/root must be carried through immutable execution context just like other managed agent work.

For non-Git mutation, default to serialized/shared-safe behavior or `NeedsAttention(isolation_unavailable)` depending on project policy. Never copy directories ad hoc or claim worktree-equivalent isolation.

## 9. Model/provider and execution-policy resolution

At claim:

- resolve the requested stable provider connection/model identity against current catalog;
- if removed/unavailable and no explicit policy permits fallback, set `NeedsAttention(model_unavailable)`;
- resolve requested approval/sandbox snapshot against current principal/project/admin ceilings;
- policy tightening may narrow or block;
- later convenience preference changes must not widen/change an existing WorkOrder;
- capture the effective policy/model on the created session/turn through existing canonical selection paths.

Do not duplicate provider selection logic inside WorkOrderCoordinator.

## 10. Session creation and runtime behavior

Create a normal session using existing project/workspace binding APIs and origin attribution. Title may derive from bounded WorkOrder title/prompt but must not be identity.

Store `work_order_id`/occurrence correlation through a dedicated linkage table/column/event rather than encoding it into session title. Additive session metadata is acceptable if it does not make session lifecycle depend on WorkOrder store availability.

After initial submission:

- foreground/future TUI can attach normally;
- steering creates ordinary session turns/messages;
- permission/question handling is unchanged;
- user cancel/stop reaches existing run/job/session control then reflects terminal/attention state back into the occurrence;
- WorkPlan may be created by the agent exactly as for any other substantial session.

## 11. Sequence progression and repeat

When an occurrence reaches its terminal state:

- update occurrence atomically from canonical result;
- if repeat count remains, create the next occurrence exactly once with persisted gate deadlines reset/re-armed according to policy;
- evaluate lane progression;
- emit a coordinator wake for newly eligible downstream work.

Default lane policy:

- completed/success advances;
- failed/cancelled/needs-attention holds;
- an authorized explicit resolution operation may retry, skip/cancel, or advance if M001/M003 expose it.

Do not treat model final text alone as success; use canonical session/job terminal state.

## 12. Cancellation and attention

Cancellation cases:

- before claim: occurrence becomes cancelled, no workspace/session/job created;
- during worktree preparation: cancel durable intent; release/archive according to WorktreeService safe rules once preparation settles;
- after session creation but before job submit: mark cancel and reconcile without submitting new work;
- running: route through canonical job/session cancellation, then project result back;
- terminal: idempotent already-terminal result.

Attention codes should be closed/bounded, e.g. model unavailable, policy denied, workspace unavailable, isolation unavailable, worktree conflict/dirty retention, scheduler submission failure requiring reconciliation, predecessor hold.

## 13. Ordered work packages

### A — Due-set/gate evaluator

Implement indexed due queries, deterministic gate latches, sequence-ready checks, repeat bounds, and explicit wakes.

### B — Claim/materialization coordinator

Implement durable claim generations and reconciliation-friendly state transitions with fault-injection seams.

### C — Workspace/worktree resolution

Integrate repository/workspace lookup and managed worktree lease allocation; define non-Git safe fallback/attention behavior.

### D — Session + scheduler submission

Create canonical session/linkage and submit initial AgentTurn through `JobSubmissionService` with deterministic keys.

### E — Result propagation/recovery

Project canonical terminal/cancel/attention outcomes back to occurrence, advance repeats/sequence, and reconcile startup partial states.

### F — Documentation/static guards

Add/update `architecture/work_orders.md`, scheduler/worktree/session docs, and a guard preventing direct agent execution or a second scheduler owner from the WorkOrder module.

## 14. Required tests

Gate/repeat:

- immediate default;
- delay deadline persists across reopen;
- not-before and backward-clock latch behavior;
- All/Any combinations;
- sequence success advances and failure holds;
- finite repeat creates distinct occurrence IDs and exhausts exactly.

Materialization:

- one occurrence → one session → one initial AgentTurn job;
- duplicate coordinator wake;
- concurrent coordinators/claims if test harness permits;
- crash/reopen after claim, worktree create, session create, and job submit;
- deterministic recovery finds existing side effect instead of duplicating it.

Isolation:

- two concurrent Git mutation WorkOrders get distinct managed worktrees/workspaces;
- read-only sharing remains safe;
- non-Git mutation follows documented serialized/attention path;
- dirty/conflicted worktree remains retained-for-attention.

Policy/model:

- removed model -> needs attention, no silent fallback;
- approval/sandbox preference changed after scheduling does not widen task;
- project policy tightening narrows/blocks at start;
- revoked principal/project authority fails closed before new execution.

Cancellation:

- every pre/post materialization boundary;
- completion-vs-cancel race has deterministic precedence;
- held sequence remains held after failed/cancelled predecessor.

## 15. Required verification

```bash
cargo test -p codegg-core -- work_order
cargo test -p codegg --lib -- work_order
cargo test -p codegg --lib -- scheduler
cargo test -p codegg-core -- worktree_service
cargo test --test worktree
cargo test --test execution_reliability
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 16. Acceptance criteria

- Immediate WorkOrder materializes exactly one normal session/job.
- Delay/not-before/sequence/repeat semantics survive restart.
- Duplicate wakes and partial-recovery paths do not duplicate sessions/jobs.
- Mutation-capable same-project Git tasks can execute concurrently in separate managed worktrees.
- Non-Git behavior is truthful/safe.
- Model/policy resolution cannot silently fallback/widen.
- Sequence failure holds later tasks by default.
- Existing scheduler remains the only admission authority.
- No TUI dependency is required for correctness.

## 17. Stop conditions

Stop if the implementation needs a second scheduler loop/queue, direct `AgentLoop` construction from WorkOrderCoordinator, speculative Session rows, ad-hoc directory copying as “isolation,” or silent provider/policy fallback.

## 18. Closure evidence required

- implementation commits;
- occurrence/materialization transition matrix;
- exact idempotency keys and recovery reconciliation description;
- restart fault-injection evidence at each side-effect boundary;
- scheduler ownership/static-guard evidence;
- worktree concurrency/non-Git behavior evidence;
- model/policy narrowing tests;
- cancellation/sequence/repeat matrix;
- exact verification results and residual findings.
