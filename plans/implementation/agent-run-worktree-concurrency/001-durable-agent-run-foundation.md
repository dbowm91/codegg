# Agent Run, Async Delegation, and Worktree Concurrency Milestone 001 — Durable Agent Run Foundation

Status: active

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m001--durable-agenttaskagentrun-ownership-and-scheduler-convergence`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent task, agent run, execution context, job, attempt
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: invariant/infrastructure

## 1. Objective

Make durable `AgentTask` and `AgentRun` identity the canonical ownership model for delegated agent execution and converge the daemon production path on one scheduler-owned execution boundary.

One accepted delegation must create or resolve exactly one durable task identity, one durable run identity for the selected execution, and one attributable scheduler job/attempt lineage. The run record must carry enough explicit execution context and authority/budget metadata for later mailbox, worktree, restart, projection, and audit milestones without changing the agent-facing delegation contract again.

This milestone does not yet implement worktree allocation or parent/child messaging. It establishes the durable ownership boundary they require.

## 2. Why this milestone is ready

Closed hard dependencies already exist:

- typed `AgentTaskId`, `AgentRunId`, project/repository/workspace identities and migration conventions;
- immutable runtime asset/agent resolution infrastructure;
- mandatory daemon scheduler with durable `JobRecord`/`AttemptRecord`, resource admission, idempotent submission keys, cancellation, and startup reconciliation;
- bounded nested-delegation compatibility logic, parent/child depth and authority narrowing seams;
- explicit workspace-root construction and current agent-runtime correctness work;
- session projection and RunStore/artifact foundations.

Stable interface dependencies:

- `JobSubmissionService` and `SubagentJobExecutor`;
- `SubAgentRequest`/`SubAgentSpawner`/`SubAgentPool` compatibility execution path;
- `TaskTool` model-facing spawn/get contract;
- `AgentLineageContext`/delegation-key compatibility identity;
- `ResolvedAgentExecutionProfile`, resolved capability/tool surface, workspace root, and scheduler execution context.

No unresolved external dependency prevents implementing a local-owner, single-daemon durable run store.

## 3. Current implementation evidence

The implementation agent must re-inspect the repository baseline and confirm at least these facts before editing:

- `crates/codegg-core/src/identity.rs` exposes opaque typed `AgentTaskId` and `AgentRunId` values, but no authoritative final task/run persistence service owns them.
- `src/tool/task.rs` maintains `SubAgentTask` state through an in-memory `HashMap` plus optional legacy `task` table persistence. Numeric task IDs are model-facing compatibility identifiers, not the long-term typed identity contract.
- `SubAgentRequest` carries prompt, target agent, parent ID, denied tools, allowed paths, depth, max tool calls, parent model, and explicit workspace root, but not a durable task/run execution record.
- `SubAgentPool` owns a request queue, a Tokio semaphore, an `AdmissionRegistry`, worker handles, lineage cancellation tokens, and task-store updates.
- daemon `TaskTool` paths can submit `JobKind::Subagent` through `JobSubmissionService`; the scheduler then invokes the subagent executor and holds its scheduler attempt until the worker result completes.
- the scheduler already distinguishes temporarily blocked admission from impossible work and owns global fairness/resource permits.
- `SubAgentPool` admission can independently reject active descendants before/inside scheduler execution, creating a second daemon concurrency decision and making durable queueing semantics less clear.
- the existing compatibility delegation hash is useful evidence for idempotency but does not by itself constitute a durable `AgentTaskId`/`AgentRunId` schema.
- child `AgentLoop` construction already receives explicit workspace root, resolved agent/model profile, inherited denied/path scope, and cancellation tokens; these inputs should be attached to the durable run rather than rediscovered later.

## 4. Invariants that must not regress

- The daemon scheduler remains the sole global production admission/resource authority.
- Standalone compatibility mode may use local execution adapters but must remain explicitly outside daemon guarantees.
- Child authority can never exceed parent/session/config/hard-deny authority.
- Durable task/run identity must not be derived from mutable path text, display titles, concatenated session labels, or numeric hashes used only for compatibility display.
- Exactly one terminal state wins for a run even when worker completion, timeout, cancellation, or daemon shutdown race.
- Duplicate model retries/frontend retransmission do not create duplicate durable tasks/runs when the same delegation identity is supplied.
- A failed enqueue/admission does not leak run counters, scheduler permits, worker leases, or active descendant accounting.
- Existing first-level `task` calls continue to produce usable model-facing handles/results during migration.
- No new production process/worker bypass is introduced outside scheduler ownership.
- Hidden reasoning, credentials, full prompts beyond existing retention policy, and unbounded tool output are not copied into new run records.

## 5. Scope

### In scope

- Define durable `AgentTaskRecord` and `AgentRunRecord` domain types using typed IDs.
- Define clear status machines for task and run lifecycle.
- Persist root/parent lineage, originating session/turn, project/repository/workspace/worktree seam, node seam, agent name/digest, provider/model, scheduler job/attempt relation, authority/budget summary, creation/update/start/finish timestamps, result/artifact references, and failure/cancellation classification.
- Add a store/service with atomic create/get/update/list-by-session/list-by-root operations and transition validation.
- Add stable delegation/idempotency identity so duplicate spawn requests resolve the same task/run or return a typed conflict.
- Make scheduler subagent execution create/resolve/update run state as part of one canonical daemon path.
- Introduce `AgentRunExecutor` or equivalent typed scheduler executor boundary that owns run lifecycle and invokes the existing child runtime through a narrow compatibility adapter.
- Move daemon concurrency ownership away from the `SubAgentPool` semaphore/admission policy where the scheduler already owns the same dimension. It is acceptable for the pool to retain standalone-only or root-descendant semantic budgets that are not scheduler resource permits, but daemon blocked resource capacity must queue at the scheduler rather than fail at the pool.
- Adapt `TaskTool` spawn/get to durable IDs while preserving existing numeric/display compatibility as needed.
- Add typed cancellation linkage from run to scheduler job/attempt and root lineage.
- Add architecture/protocol seams for later worktree/mailbox fields without fabricating values before those milestones.

### Explicitly out of scope

- Durable run mailbox/message journal.
- Worktree creation/leases or child commit authority.
- Run groups/join policies beyond what is necessary to preserve current behavior.
- Final session projection/TUI run tree; only minimal additive IDs/status needed for compatibility are allowed.
- Team principal authorization beyond existing local/session authority.
- Cross-daemon execution.
- General retry policy for failed model/tool work; initial run retry/restart behavior must be conservative and explicit.
- Deleting every `SubAgentPool`/`TaskStore` type in this milestone.

## 6. Required production changes

### Core/domain

Introduce durable types in an appropriate `codegg-core` module rather than root-only ad-hoc structs. Use names aligned with terminology, for example:

```rust
pub struct AgentTaskRecord {
    pub task_id: AgentTaskId,
    pub root_task_id: AgentTaskId,
    pub parent_task_id: Option<AgentTaskId>,
    pub originating_session_id: String,
    pub originating_turn_id: Option<String>,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    pub requested_agent: String,
    pub delegation_key: String,
    pub status: AgentTaskStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct AgentRunRecord {
    pub run_id: AgentRunId,
    pub task_id: AgentTaskId,
    pub root_run_id: AgentRunId,
    pub parent_run_id: Option<AgentRunId>,
    pub workspace_id: WorkspaceId,
    pub worktree_id: Option<WorktreeId>,
    pub node_id: Option<NodeId>,
    pub job_id: Option<JobId>,
    pub attempt_id: Option<AttemptId>,
    pub agent_name: String,
    pub agent_digest: Option<String>,
    pub provider: String,
    pub model: String,
    pub authority_digest: String,
    pub budget: AgentRunBudget,
    pub status: AgentRunStatus,
    pub terminal: Option<AgentRunTerminal>,
    pub result_ref: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub updated_at: i64,
}
```

Exact fields may differ to match existing core types. Store bounded digests/summaries rather than serializing entire permission maps or prompts when existing authoritative records already exist.

Define transition methods instead of allowing arbitrary status overwrite. Representative run states: `Created`, `Queued`, `Preparing`, `Running`, `Waiting`, `Cancelling`, terminal `Completed`, `Failed`, `Interrupted`, `Cancelled`. Avoid unnecessary states that no production consumer uses.

### Storage and migrations

Add migration(s) for task/run persistence with:

- stable string primary IDs;
- indexed session/root/parent/job/workspace/status fields needed for active-run lookup and recovery;
- unique delegation identity at the correct scope;
- terminal/result references;
- bounded JSON only for small versioned metadata where normalized columns are not warranted.

Do not rewrite the existing legacy `task` table into fake typed history. Either:

1. retain it as compatibility storage and create new canonical tables; or
2. migrate rows with explicitly nullable/missing provenance and a migration-source marker.

Prefer the simpler approach that preserves history and makes new writes canonical.

### Protocol and DTOs

Add only the minimal additive protocol needed by current TaskTool/server consumers:

- stable task/run IDs in scheduler/job completion or task status results;
- query by run/task ID;
- cancellation request by run ID if no existing job-ID API cleanly covers the user-facing operation.

Do not design the full run-tree projection here; M006 owns that surface.

### Runtime and concurrency

Create one daemon `AgentRunService`/executor boundary responsible for:

1. resolve/create task identity;
2. create run and persist `Created/Queued`;
3. submit/link scheduler job;
4. on executor start, attach `AttemptId` and mark `Running` only after scheduler ownership is established;
5. invoke child runtime through a narrow trait/adapter;
6. map typed child outcome/cancellation/timeout/panic into one terminal transition;
7. persist bounded result reference/summary;
8. release compatibility lineage/accounting exactly once.

The scheduler must not enqueue into a second daemon semaphore that can reject merely because current capacity is full. If `SubAgentPool` remains the executor implementation, separate semantic delegation limits (depth, per-root fan-out/tool budget) from machine resource/concurrency permits already represented by scheduler resources.

Preserve root-lineage cancellation. Prefer storing cancellation intent in durable run/job state and using `CancellationToken` only as the live transport.

### Frontend or operator surface

Keep model-facing output compact, for example returning both compatibility task number (while retained) and durable IDs:

```text
Task queued
Task: <AgentTaskId>
Run: <AgentRunId>
Status: queued
```

Do not expose database schema or raw internal digests.

### Security and authorization

- Compute authority digest/summary from the already-resolved child execution ceiling; do not create a second permission evaluator.
- Persist only non-secret authority metadata suitable for later audit comparisons.
- Validate parent/root IDs and session/workspace relation; a caller cannot attach a child to an unrelated root merely by supplying an ID string.
- Cancellation/query operations must be scoped to the owning session/principal authority seam already available.

### Documentation and static guards

Update at least:

- `architecture/agent.md`;
- `architecture/scheduler.md`;
- relevant storage architecture docs;
- `architecture/identity.md` if durable semantics need clarification.

Update `scripts/check_scheduler_bypass.py` / `scripts/check_execution_ownership.py` only if source ownership moved. Do not add redundant new static scripts if existing ownership manifests can express the boundary.

## 7. Ordered work packages

### Work package A — Domain and migration contract

Intent:

Establish typed task/run state and schema before changing execution wiring.

Required changes:

- define statuses, terminal classification, records, budgets, lineage references, idempotency key type;
- add store and migration;
- add transition/idempotency tests;
- document legacy task-table disposition.

Acceptance evidence:

- records round-trip through SQLite;
- illegal terminal-to-running or cross-root transitions fail;
- duplicate delegation key resolves deterministically.

### Work package B — Canonical AgentRunService

Intent:

Centralize task/run creation and lifecycle ownership.

Required changes:

- add service APIs for create/resolve, attach job/attempt, mark started/terminal, query, cancellation intent;
- ensure service methods are idempotent under duplicate completion/cancellation;
- use typed IDs end to end internally.

Acceptance evidence:

- two concurrent identical delegation submissions yield one canonical task/run or one accepted identity plus explicit typed duplicate result;
- terminal races cannot overwrite the first accepted terminal state.

### Work package C — Scheduler executor convergence

Intent:

Make scheduler execution the canonical daemon child-run path.

Required changes:

- register `AgentRunExecutor` or evolve `SubagentJobExecutor` to own durable run lifecycle;
- attach job/attempt/run provenance before child execution;
- remove or neutralize duplicate machine-capacity rejection in `SubAgentPool` for scheduler-owned paths;
- retain semantic root limits and standalone compatibility where needed.

Acceptance evidence:

- scheduler capacity produces queued/blocked state rather than subagent-pool capacity failure;
- one scheduler attempt maps to one run execution attempt;
- cancellation propagates to the live child and durable terminal state.

### Work package D — TaskTool compatibility adapter

Intent:

Preserve model-facing behavior while moving authority underneath.

Required changes:

- route daemon spawn through `AgentRunService`/scheduler;
- return stable IDs;
- make `get` resolve durable state first and legacy state only for legacy tasks;
- stop deriving durable identity from a hash of scheduler job bytes.

Acceptance evidence:

- existing spawn/get fixtures continue to pass or have explicitly updated additive output assertions;
- no new direct pool send is introduced in daemon production.

### Work package E — Recovery/cancellation integration

Intent:

Make startup/shutdown semantics explicit enough for M002/M003.

Required changes:

- reconcile stale `Running/Preparing` run records against scheduler attempt/job recovery;
- mark unrecoverable in-flight compatibility children interrupted rather than silently restarting non-idempotent work;
- ensure queued jobs remain queue-owned;
- cascade root cancellation through current descendant relation.

Acceptance evidence:

- injected daemon restart at pre-start/running/terminal boundaries produces deterministic run state;
- no duplicate child execution occurs after restart.

### Work package F — Documentation and ownership guards

Intent:

Record the new authority boundary without increasing routine verification burden.

Acceptance evidence:

- architecture docs describe AgentTask/AgentRun versus scheduler Job/Attempt correctly;
- execution ownership manifest/static guard accepts only the canonical production route.

## 8. Failure, cancellation, restart, and contention semantics

- Store failure before job submission: return failure; no scheduler job exists and task/run remains failed or is rolled back according to one documented transaction boundary.
- Job submission failure after task/run creation: terminalize or explicitly leave retryable `Created` state; never leave ambiguous `Queued` without a job.
- Worker/executor panic: scheduler attempt and run become interrupted/failed exactly once; compatibility leases/counters release.
- Cancellation before admission: scheduler cancels queued job and run becomes cancelled without starting child runtime.
- Cancellation while running: set durable cancellation intent, signal live token, wait boundedly for executor cleanup, then terminalize using scheduler precedence.
- Completion racing cancellation: use one documented terminal precedence consistent with scheduler store semantics; do not let a late adapter overwrite terminal state.
- Daemon restart: recover scheduler generation first, then reconcile task/run records against durable job/attempt state. Do not replay a completed child from scratch.
- Concurrent roots: scheduler global fairness/resource limits remain authoritative; per-root semantic fan-out budgets may block/reject only based on explicit delegation policy, not machine capacity already represented by scheduler resources.

## 9. Compatibility and migration

- Preserve existing `task` action names `spawn` and `get`.
- Numeric task IDs may remain as a short-lived display alias but must not be used as database/run authority. Prefer returning typed IDs immediately and document alias-removal criteria.
- Existing `SubAgentTask` and `TaskStore` may wrap/read the canonical service or remain only for legacy records. New daemon tasks must not require two independent durable stores to agree.
- `SubAgentPool` remains available for standalone/tests until M006; daemon production must pass through scheduler and durable run service.
- Existing `Subagent*` app events remain emitted for compatibility, derived from run lifecycle where practical.
- No protocol field removal in this milestone.

## 10. Required tests

### Focused unit tests

- task/run status transition table;
- typed ID and store round trips;
- delegation-key uniqueness/idempotency;
- root/parent relation validation;
- authority/budget metadata bounds;
- first-terminal-wins behavior.

### Integration tests

- TaskTool spawn -> durable task/run -> scheduler job -> child -> terminal result;
- scheduler capacity contention queues multiple child runs without pool-capacity rejection;
- existing first-level task get compatibility;
- three-level lineage metadata where current nested delegation already supports it.

### Restart and recovery tests

- restart before scheduler admission;
- restart after attempt created but before child start;
- restart during child run;
- restart after child completion before parent consumes result;
- no duplicate execution of terminal run.

### Contention and cancellation tests

- duplicate spawn from concurrent callers;
- root cancellation with multiple descendants;
- cancellation while queued and while running;
- sibling failure does not corrupt root accounting.

### Security and negative tests

- forged parent/root/session relation rejected;
- child authority digest cannot be supplied by model input;
- secret-bearing permission/config values absent from persisted run metadata.

### Migration and compatibility tests

- legacy task records remain readable;
- new task/run records do not require backfilling unverifiable historical provenance;
- standalone compatibility path remains explicit.

## 11. Required verification commands

Use focused commands appropriate to files actually changed. Expected minimum shape:

```bash
cargo test -p codegg-core agent_run
cargo test --lib agent::worker
cargo test --lib scheduler
cargo test --test scheduler_authority_matrix
cargo test --test scheduler_restart_recovery
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
```

Run `scripts/verify.sh quick` only once the milestone implementation is coherent or when the current repository verification policy requires it. Do not add new CI jobs, matrices, coverage gates, or release automation.

## 12. Documentation updates

- `architecture/agent.md` — durable task/run ownership and compatibility adapters.
- `architecture/scheduler.md` — AgentRunExecutor relationship to Job/Attempt and single admission authority.
- storage architecture doc — schema/recovery/retention.
- `architecture/identity.md` — only if needed to distinguish identity existence from durable subsystem ownership.
- source roadmap milestone status after closure evidence is accepted.

## 13. Acceptance criteria

1. A new daemon delegated task has stable `AgentTaskId` and `AgentRunId` records before execution.
2. Each run is linked to one scheduler job and execution attempt lineage without transient numeric/hash identity as authority.
3. Duplicate delegation is idempotent or fails with an explicit typed conflict; it never silently creates uncontrolled duplicate runs.
4. Scheduler capacity contention queues rather than being converted into a second pool-capacity failure.
5. Parent/root/session/workspace/agent/model/authority/budget provenance is durable and bounded.
6. Cancellation and completion races produce exactly one terminal run state.
7. Restart reconciliation does not repeat completed child work.
8. Existing first-level `task spawn/get` remains usable through a compatibility adapter.
9. No daemon process/worker execution bypass is added.
10. Focused verification and existing ownership guards are green.

## 14. Stop conditions

Stop and report rather than improvising if:

- implementing durable runs requires redefining `AgentTask`/`AgentRun` contrary to canonical terminology;
- scheduler Job/Attempt ownership would need to be replaced or bypassed;
- the current database migration framework cannot add durable tables without destructive history loss;
- parent authority cannot be represented without persisting secrets/full permission bodies;
- removing duplicate pool admission would eliminate a semantic delegation limit not represented anywhere else and no narrow replacement is evident;
- changes expand into worktree lease behavior owned by M003 or mailbox semantics owned by M002.

## 15. Closure evidence required

The later closure record must contain:

- implementation commit(s) and reviewed head;
- schema/migration summary;
- requirement-to-evidence matrix for acceptance criteria 1–10;
- exact focused tests/guards run and outcomes;
- duplicate submission, contention, cancellation-race, and restart evidence;
- compatibility evidence for existing task calls and legacy records;
- proof that daemon scheduler admission is not bypassed or duplicated for machine capacity;
- unresolved findings with severity;
- explicit recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

Keep the first milestone narrow. Do not build the mailbox, worktree service, run groups, or full frontend tree early merely because the schema has seams for them. Prefer one canonical service and compatibility adapters over dual-write synchronization. Preserve the repository’s current minimal verification philosophy; correctness here comes from focused transition/concurrency/restart tests and existing execution-ownership guards, not from adding broad CI machinery.
