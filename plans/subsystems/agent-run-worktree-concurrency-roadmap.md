# Agent Run, Async Delegation, and Worktree Concurrency Roadmap

Status: active

Repository baseline reviewed: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Long-term references:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent task, agent run, worktree, execution context, job, attempt, artifact
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

External design input:

- Meta AI, “Introducing Muse Code and Muse Spark 1.2,” for persistent asynchronous child agents, direct agent communication, event-log recovery, and Git worktree isolation. CodeGG adopts the transferable execution semantics while preserving its own scheduler, Git, permission, protocol, and storage ownership.

## 1. Purpose and ownership boundary

This subsystem combines the long-term durable agent hierarchy and worktree-native concurrency phases at the execution boundary where they are most useful together.

It owns:

- durable `AgentTask` and `AgentRun` records and lineage;
- scheduler-owned delegated-agent execution and the migration away from `SubAgentPool` as a second daemon admission layer;
- bounded asynchronous parent/child control and completion delivery;
- stable-boundary execution journaling needed for restart/recovery;
- durable daemon-owned worktree records and leases for delegated runs;
- automatic worktree isolation for mutation-capable concurrent children;
- child commit/checkpoint/result contracts and explicit integration handoff;
- bounded run groups/join policies and scheduler-backed background handles where independent lifetime is useful;
- projection/TUI protocol integration for run/worktree state;
- compatibility migration from `SubAgentTask`, `TaskStore`, `SubAgentRequest`, `SubAgentPool`, and legacy subagent events.

It consumes, but does not redefine:

- project/repository/workspace identity;
- the singleton daemon and global scheduler;
- canonical permission/tool-surface authority;
- runtime asset snapshots and agent resolution;
- typed Git operations, mutation snapshots, conflict detection, recovery, and credential policy;
- session projection as a derived frontend contract;
- Tool Programs and ordinary parallel tool batches;
- future principal/team authorization and distributed execution.

The governing rule is:

> A delegated run is a durable scheduler-owned execution object. If it can mutate concurrently, filesystem/Git isolation is part of run construction rather than optional planning advice to the model.

## 2. Work classification

### Invariants

- The singleton daemon scheduler is the only daemon production admission/resource authority for delegated runs and scheduler-backed background operations.
- One delegated objective has stable durable identity; duplicate delivery cannot create uncontrolled duplicate children.
- Child authority is monotonic narrowing from parent authority and cannot expand through mailbox messages, worktree ownership, retry, restart, or integration.
- Production run/worktree ownership uses typed project/repository/workspace/worktree/run IDs, not path concatenation or display session IDs.
- Mutation-capable delegated runs that may execute concurrently do not share a working tree/index by default.
- A worktree lease has one explicit owner at a time and cleanup never destroys dirty/conflicted state merely to make bookkeeping convenient.
- Child Git commit authority is scoped to its owned worktree and does not imply push, force/history rewrite, or parent-branch integration authority.
- Completion and recovery state is durable at stable execution boundaries; session projections remain derived and non-authoritative.
- Cancellation propagates downward and releases scheduler/worktree ownership exactly once.
- Background execution is limited to scheduler-owned operations with independent lifetimes; cheap turn-local tools retain normal direct/parallel execution.

### Capabilities

- A primary agent can spawn several children asynchronously and continue its own trajectory.
- The primary can inspect status, wait, message, interrupt, or cancel a child without terminating unrelated siblings.
- Useful child progress/completion can be delivered to the parent without repeated polling.
- Parallel implementation children automatically receive separate worktrees and can create bounded commits/checkpoints.
- A child returns structured changed-path, commit, validation, finding, artifact, and conflict evidence.
- The primary can explicitly integrate or reject a child result using typed Git operations.
- A primary can create a bounded run group and use `all`, `any_successful`, `first_completed`, or `detached` join semantics once durability is proven.
- Eligible long-running scheduler jobs can return background handles and later notify the owning run/session.
- TUI/ACP/session projections can show the durable run tree, worktree/branch identity, progress, terminal result, and cancellation state.

### Infrastructure

- durable run/task stores and migration adapters;
- run mailbox and stable-boundary event journal;
- daemon worktree service and lease store;
- structured `AgentRunResult` and checkpoint metadata;
- scheduler-backed run/group wait and notification primitives.

### Polish

- concise agent-facing descriptions that teach the model isolation is automatic;
- TUI run/worktree labels and branch/commit summaries;
- removal of redundant transient admission and polling paths after compatibility closure.

## 3. Non-goals

This roadmap must not:

- introduce a second scheduler, workflow engine, or independent worker admission authority;
- convert every tool call into a detached asynchronous job;
- require worktrees for read-only scouts or serialized single-run work without a mutation/isolation need;
- implement unrestricted sibling-to-sibling agent chat in the first pass;
- automatically merge all successful child branches into the parent;
- grant children push, force-push, arbitrary reset/clean, credential, or network authority merely because they own a worktree;
- implement cross-daemon children, SSH worktrees, remote execution targets, or distributed artifact transport;
- complete final team authorization/audit/chat;
- expose hidden reasoning in run journals, projections, or mailboxes;
- implement arbitrary historical rewind before safe checkpoint semantics exist;
- add new CI matrices, scheduled verification, coverage gates, benchmark gates, or release automation.

## 4. Current state

At baseline `b08d33b7`:

- `AgentRunId`, `AgentTaskId`, and `WorktreeId` already exist as typed domain identities, but final durable run/worktree stores are not implemented.
- `src/agent/worker.rs` provides `SubAgentPool`, `SubAgentSpawner`, lineage compatibility IDs, depth/fan-out/active/tool-call admission, root-lineage cancellation, worker handles, timeout handling, and fresh child `AgentLoop` construction.
- daemon production `task` submission can create durable `JobKind::Subagent` jobs through `JobSubmissionService`; the scheduler owns global resource admission, fair queuing, attempts, cancellation, and startup reconciliation.
- the scheduler’s subagent executor still hands execution to `SubAgentPool`, which has its own semaphore/admission registry. This duplicates daemon concurrency policy and complicates blocked-vs-rejected semantics.
- the model-facing `task` tool exposes primarily `spawn` and `get`. `send_async()` is effectively fire-and-return, but completion discovery is polling-oriented and there is no first-class durable message/interrupt/wait/cancel control surface.
- global app/projection events expose `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, and `SubagentFailed`, but these are derived/transient lifecycle events rather than an authoritative run journal.
- `AgentLoop` already owns follow-up, steering, question, cancellation, provider/tool recovery, and bounded parallel tool execution seams that can receive durable run-control input at safe boundaries.
- `src/agent/team.rs` contains file-backed team inbox/outbox messaging, but it is not the right authority for durable run control and should not be expanded into the execution mailbox.
- `crates/codegg-core/src/worktree.rs` and `crates/egggit/src/worktree.rs` provide list/create/remove/detection helpers and hardened Git environment policy. Worktree create/remove are not durable leases and have no agent-run owner, generation, base commit, or recovery state.
- typed Git execution is mature: structured reads, mutation snapshots/deltas, conflict detection, network policy, recovery operations, RunStore persistence, path/ref safety, and credential redaction already exist.
- child execution currently hard-denies `commit`, which is appropriate for shared-workspace children but prevents an isolated child from returning a commit as a clean integration artifact.
- session projection already has bounded subagent/job/run concepts, durable replay, visibility classes, and deterministic reducers. It can be evolved additively rather than replaced.
- the long-term roadmap already requires durable multilevel runs and worktree-native concurrency as consecutive phases. This subsystem refines those phases into a coupled implementation sequence without changing their end-state requirements.

## 5. Target architecture

### 5.1 Durable run ownership

The canonical delegated path becomes:

```text
model task/delegate call
    -> AgentTask create/idempotency
    -> AgentRun create
    -> scheduler JobSubmit
    -> scheduler admission + Attempt
    -> execution-context/worktree preparation
    -> AgentRunExecutor
    -> AgentLoop
    -> stable-boundary journal/result
    -> terminal run/task state
```

`AgentTask` represents the durable objective. `AgentRun` represents one execution of that objective. Retries/restarts create or resume runs according to explicit idempotency/recovery rules rather than overloading transient task IDs.

### 5.2 Mailbox and journal

Each active run has a bounded durable mailbox keyed by run ID and monotonically ordered message/control IDs. Initial operations are `message`, `interrupt`, `cancel`, `status`, and bounded `wait`. Delivery is translated into existing `AgentLoop` follow-up/steering/cancellation seams at safe boundaries.

The authoritative journal stores lifecycle/control/checkpoint events, not token deltas or hidden reasoning. Representative events:

```text
TaskAccepted
RunCreated
Queued
WorktreeRequested
WorktreePrepared
Started
MessageQueued
MessageDelivered
InterruptRequested
ToolBoundaryReached
CheckpointCreated
ValidationRecorded
CompletionProduced
CancelRequested
Completed | Failed | Interrupted
CleanupStarted
CleanupCompleted
```

The session projection adapter consumes these events and produces bounded frontend state.

### 5.3 Worktree lease service

A daemon worktree service owns durable `WorktreeRecord`/`WorktreeLease` state and uses existing typed/hardened Git services. A lease includes:

- `WorktreeId`;
- repository/project/workspace/node identity;
- path;
- branch/ref strategy;
- base commit;
- owner run/principal;
- lease generation;
- lifecycle/health state;
- dirty/conflict summary;
- creation/last-used timestamps.

Concurrent mutation-capable delegated runs receive distinct leases before their `AgentLoop` is constructed. Read-only runs may reuse the parent workspace/worktree. Shared repository/build resources remain scheduler-controlled independently of worktree isolation.

### 5.4 Child mutation/result contract

An isolated child with inherited `GitWrite` may stage/commit inside its lease. It cannot mutate sibling/parent worktrees or implicitly push/integrate.

Terminal output includes a typed result roughly equivalent to:

```text
AgentRunResult
  status
  summary
  worktree_id
  base_commit
  result_commit
  changed_paths
  validation_results
  findings
  artifacts
  conflict_state
  retryability/recovery_hint
```

Integration uses typed Git operations and is explicit. Conflict results are returned as structured evidence; automatic conflict resolution is not required.

### 5.5 Run groups and background jobs

`AgentRunGroup` is a thin scheduler/run-service composition, not a general workflow language. It owns a bounded list of runs plus a join policy and cancellation rule. Join policies are `all`, `any_successful`, `first_completed`, and `detached`.

The same notification/wait mechanism may be used for scheduler-backed tests/builds/lints/formats/research/Tool Programs. Ordinary direct tool calls remain ordinary direct or parallel batch calls.

### 5.6 Compatibility convergence

During migration:

```text
TaskTool compatibility surface
      |
      v
AgentRunService
      |
      +--> scheduler
      +--> mailbox/journal
      +--> worktree service
      `--> AgentRunExecutor

SubAgentPool / TaskStore / legacy events
      `--> bounded adapters only
```

The final daemon path should not require both scheduler admission and a second subagent semaphore/admission decision.

## 6. Dependency graph

```text
M001 durable AgentTask/AgentRun ownership + scheduler convergence
   |\
   | +---------------------> M003 durable worktree lease service
   |
   `-----------------------> M002 mailbox, journal, async control/completion
                                |                 |
M003 ---------------------------+-----------------+
                                v
                 M004 isolated mutation, commits, structured results
                                |
                                v
                 M005 run groups and scheduler-backed background joins
                                |
                                v
                 M006 projection, compatibility deletion, closure
```

Dependency classes:

- M001 has hard dependencies only on already-closed domain identity, runtime assets, scheduler, and current agent-runtime foundations. It is dependency-ready.
- M002 has a hard dependency on M001 because mailbox/journal state must attach to durable run identity.
- M003 has a hard dependency on M001 for owner identity and an interface dependency on existing typed Git/worktree services.
- M004 has hard dependencies on M002 and M003 because isolated mutation must have durable run control/recovery plus owned worktrees.
- M005 has hard dependencies on M002 and M004; it must not add detached/group semantics before ownership, notification, cancellation, and child-result contracts are reliable.
- M006 has hard dependencies on M001-M005 and is the only milestone that may remove compatibility admission/polling paths and close the roadmap.

## 7. Milestones

### M001 — Durable AgentTask/AgentRun ownership and scheduler convergence

Class: invariant/infrastructure

Status: active

Plan: `plans/implementation/agent-run-worktree-concurrency/001-durable-agent-run-foundation.md`

Objective:

Introduce durable task/run records with typed lineage and scheduler/job/attempt linkage; make the scheduler-owned `AgentRunExecutor` the canonical daemon execution boundary while retaining `SubAgentPool` only as a compatibility/execution adapter where necessary.

User/operator value:

Delegated work becomes restart-inspectable and globally attributable instead of existing primarily as transient task/pool state.

Exit conditions:

- one accepted delegation creates one durable task/run identity and one scheduler lineage;
- duplicate submission is deterministic;
- cancellation/terminal state is recorded exactly once;
- daemon admission does not depend on a second independent pool concurrency decision;
- first-level existing `task` behavior remains compatible.

Deferred work:

Mailbox control, worktree leases, child commit authority, group joins, and final adapter deletion.

### M002 — Run mailbox, stable-boundary journal, and asynchronous completion delivery

Class: capability/infrastructure

Status: blocked on M001

Plan: `plans/implementation/agent-run-worktree-concurrency/002-run-mailbox-journal-and-async-control.md`

Objective:

Add durable ordered parent/child control plus safe-boundary `message`, `interrupt`, `cancel`, `status`, and bounded `wait`; push bounded progress/completion into the owning parent/session path without polling loops.

Exit conditions:

- message ordering and restart replay are deterministic;
- interrupt is applied only at a safe/cancellable boundary;
- cancellation races resolve to one terminal state;
- completion can notify an active parent without repeated `get` calls;
- mailbox content cannot widen authority.

### M003 — Durable daemon worktree service and leases

Class: infrastructure/invariant

Status: blocked on M001

Plan: `plans/implementation/agent-run-worktree-concurrency/003-durable-worktree-service-and-leases.md`

Objective:

Promote low-level worktree helpers into durable daemon-owned records/leases with owner identity, base commit, lifecycle/health state, restart reconciliation, and safe cleanup.

Exit conditions:

- worktree ownership is durable and typed;
- concurrent lease creation handles branch/path collisions deterministically;
- restart detects and reconciles live/orphaned worktrees;
- dirty/conflicted worktrees are never force-removed by automatic cleanup;
- existing manual worktree commands remain compatible.

### M004 — Automatic mutation isolation, child commits, structured results, and integration handoff

Class: capability/invariant

Status: blocked on M002 and M003

Plan: `plans/implementation/agent-run-worktree-concurrency/004-isolated-mutation-and-structured-results.md`

Objective:

Bind mutation-capable delegated runs to isolated worktree leases by default, permit scoped child commits in owned worktrees, return typed result/checkpoint evidence, and expose explicit parent integration/handoff without automatic merging.

Exit conditions:

- two concurrent mutating children never share a working tree/index;
- read-only children can avoid unnecessary worktree allocation;
- child commit succeeds only inside its owned worktree under inherited `GitWrite`;
- push/history rewrite remains independently denied/authorized;
- parent receives base/result commit, changed paths, validation, artifacts, and conflict state;
- integration conflicts are structured and recoverable.

### M005 — Run groups, join policies, and scheduler-backed background handles

Class: capability

Status: blocked on M002 and M004

Plan: `plans/implementation/agent-run-worktree-concurrency/005-run-groups-and-background-joins.md`

Objective:

Add bounded run groups and `all`/`any_successful`/`first_completed`/`detached` semantics, and reuse the same wait/notification contract for eligible scheduler-backed long-running jobs without turning ordinary tools into detached jobs.

Exit conditions:

- group joins terminate deterministically across mixed terminal states;
- parent/group cancellation propagates correctly;
- detached work remains durable, bounded, visible, and reclaimable;
- eligible tests/builds/research/Tool Programs can notify owners asynchronously;
- ordinary tool-batch behavior does not regress.

### M006 — Projection, compatibility simplification, and strict closure

Class: capability/polish/invariant

Status: blocked on M001-M005

Plan: `plans/implementation/agent-run-worktree-concurrency/006-projection-compatibility-and-closure.md`

Objective:

Project durable run/worktree state through native protocol/TUI/ACP-compatible session projection seams, migrate legacy subagent events/task polling, remove redundant daemon admission paths once proven unused, reconcile architecture docs/guards, and gather closure evidence.

Exit conditions:

- frontends reconstruct the run tree/worktree/result state from authoritative records/events;
- legacy `task get` and subagent events remain compatible or have an explicit migration window;
- daemon production has one scheduler admission authority;
- no high/medium correctness, security, cancellation, restart, worktree-leak, or integration finding remains;
- focused tests plus the repository’s deliberately minimal broad verification pass are green;
- roadmap, plans, registry, architecture docs, and closure record agree.

## 8. Cross-cutting requirements

### Storage and migration

- Add schema versions/migrations for task/run, mailbox/journal, worktree lease, and group/result data only in the milestone that owns each contract.
- Do not require fabricated backfill for historical tasks that lack authoritative run/worktree provenance. Preserve them as legacy records with explicit missing fields.
- Journal/result payloads are bounded; large tool/test/diff output uses existing artifact/RunStore handles.

### Protocol and compatibility

- Prefer additive protocol DTOs/events and capability negotiation already used by session projections.
- Do not create a second frontend reducer or expose raw SQLite rows.
- `task` model-facing names/actions should migrate additively so model prompts do not need a disruptive one-step rename.

### Security and authorization

- Worktree ownership is not permission. Every tool/Git action still traverses the normal resolved capability and permission boundary.
- Mailbox senders must be authorized by run lineage/ownership; message text cannot alter runtime authority fields.
- Worktree paths/ref names use existing safe typed Git/path validation and hardened process environment policy.
- No secrets or hidden reasoning enter journal/projection payloads.

### Concurrency, cancellation, and recovery

- Scheduler permits and exclusivity keys remain global across worktrees.
- Never hold store/lease locks across provider calls, tool execution, Git subprocesses, or long waits.
- Cancellation is downward by default and terminal-state transitions are idempotent.
- Restart reconciliation distinguishes queued/running/interrupted runs and ready/dirty/orphaned worktrees without replaying completed non-idempotent operations.

### Observability and audit

- Every run state transition includes stable run/task/job/attempt/worktree correlation where applicable.
- Projection summaries are bounded; detailed logs/results remain artifact-backed.
- Future audit instrumentation should be able to consume journal/run/worktree identity without schema redesign.

### Performance and resource use

- Worktrees are allocated only when isolation is useful.
- Worktree count, active run count, active descendants, group width, mailbox depth, journal size, waiters, and background jobs are bounded.
- Reuse Git object storage; do not clone full repositories per child.
- Normal parallel tool execution remains available and should not be routed through durable jobs unnecessarily.

### Documentation and operations

- Keep `architecture/agent.md`, `architecture/scheduler.md`, `architecture/worktree.md`, `architecture/git.md`, `architecture/projection.md`, and relevant tool docs synchronized as ownership moves.
- Document orphan inspection/cleanup and how a user can retain or remove failed child worktrees.

## 9. Verification strategy

Milestones use focused tests first and avoid expanding routine verification machinery.

Subsystem closure must cover:

- task/run identity, lineage, duplicate submission, and terminal-state idempotency;
- parent/child message ordering, interrupt, cancellation, restart, and notification;
- worktree create/lease/reconcile/release under contention;
- two or more parallel mutating children on one repository;
- read-only child reuse without mutation leakage;
- child commit/push/history-rewrite policy boundaries;
- dirty/conflicted failure and daemon restart;
- structured result/checkpoint/integration conflict behavior;
- run-group join/cancel/detached semantics;
- scheduler-backed background completion notification;
- session projection replay/resync of run/worktree state;
- compatibility migration from existing subagent/task paths;
- static scheduler/execution ownership guards.

Broad verification remains the repository’s existing minimal posture: focused cargo tests and guards during milestones, then one `scripts/verify.sh quick`-class pass (or the then-current documented equivalent) at closure. Do not add CI lanes or fixed release gates for this subsystem.

## 10. Risks and decision points

- SQLite event-journal volume: keep stable-boundary events small; do not persist token deltas/reasoning.
- Worktree branch naming under crashes/retries: use run identity/generation and typed collision handling rather than display names.
- Cargo/build-cache contention: worktree isolation does not isolate shared caches; scheduler exclusivity/resource hints remain authoritative.
- Git submodules/LFS/large generated files may make worktree preparation expensive. Initial support should preserve existing Git behavior and report unsupported/slow cases rather than invent clone strategies.
- Child commit signing/hooks may block noninteractive execution. Existing hardened Git environment policy remains authoritative; automatic child commits must fail clearly if repository policy requires unavailable interactive signing/hooks.
- The current task table’s ID/autoincrement/compatibility semantics may not map one-to-one to `AgentTaskId`. M001 must design a migration rather than reuse accidental numeric identities as the durable contract.
- If M001 requires changing canonical definitions of AgentTask/AgentRun or scheduler ownership, stop and supersede ADR-0002 before implementation.

## 11. Completion definition

This roadmap closes only when:

1. delegated objectives/runs have durable typed identity and one scheduler-owned daemon execution path;
2. parents can asynchronously spawn, observe, steer/message, wait, and cancel children with restart-safe bounded semantics;
3. mutating concurrent children receive owned worktrees automatically and read-only children do not pay that cost unnecessarily;
4. isolated children can produce bounded commits/checkpoints and structured validation/result evidence without gaining push/history authority;
5. explicit integration/handoff reports conflicts through typed Git state;
6. run groups/background eligible jobs use deterministic join/notification semantics;
7. TUI/native projection consumers can inspect the same authoritative run/worktree tree after reconnect/restart;
8. legacy pool/task/event adapters are removed or reduced to a documented compatibility boundary with no second daemon admission authority;
9. no unresolved high or medium finding remains in scheduler ownership, authority narrowing, cancellation, restart, worktree cleanup, Git isolation, or result integration;
10. closure evidence, roadmap, implementation plans, architecture docs, and registry agree.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | active | `plans/implementation/agent-run-worktree-concurrency/001-durable-agent-run-foundation.md` | — | — |
| M002 | blocked | `plans/implementation/agent-run-worktree-concurrency/002-run-mailbox-journal-and-async-control.md` | — | M001 |
| M003 | blocked | `plans/implementation/agent-run-worktree-concurrency/003-durable-worktree-service-and-leases.md` | — | M001 |
| M004 | blocked | `plans/implementation/agent-run-worktree-concurrency/004-isolated-mutation-and-structured-results.md` | — | M002, M003 |
| M005 | blocked | `plans/implementation/agent-run-worktree-concurrency/005-run-groups-and-background-joins.md` | — | M002, M004 |
| M006 | blocked | `plans/implementation/agent-run-worktree-concurrency/006-projection-compatibility-and-closure.md` | — | M001-M005 |
