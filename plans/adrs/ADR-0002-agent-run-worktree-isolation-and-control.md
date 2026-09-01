# ADR-0002: Scheduler-Owned Agent Runs and Worktree-Isolated Delegated Mutation

Status: accepted

Date: 2026-09-01

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent task, agent run, worktree, execution context, job, attempt, artifact
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Affected subsystem roadmaps:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

External design input:

- Meta AI, “Introducing Muse Code and Muse Spark 1.2,” `https://research.meta.ai/blog/introducing-muse-code-and-muse-spark-1-2`

## Context

CodeGG already has a daemon-owned durable scheduler, resource admission, cancellation and restart recovery, parallel tool batches, a bounded nested-subagent pool, typed Git mutation/recovery services, worktree add/list/remove support, session projections, and structured recovery for malformed or repetitive tool behavior.

The missing boundary is not raw concurrency. It is a coherent durable unit that composes delegated agent execution, scheduler ownership, parent/child communication, workspace isolation, Git state, completion evidence, and recovery.

Today a delegated child is represented primarily through `SubAgentTask`, `SubAgentRequest`, `TaskStore`, `SubAgentPool`, scheduler jobs, derived subagent events, and an explicit `workspace_root`. Daemon production submission is scheduler-owned, but `SubAgentPool` still contains a second semaphore/admission layer. Child communication is mainly spawn-and-poll. Worktree operations are low-level and are not leased or bound to an agent run. Mutating children therefore either share a workspace or must be serialized/restricted, and child commits are currently hard-denied.

Meta’s Muse Code provides useful external validation for a different composition: long-running asynchronous child agents can operate independently, communicate useful progress/results back to a primary agent, recover from persisted execution history, and use separate Git worktrees for parallel implementation. CodeGG should adopt the transferable execution semantics without introducing a second scheduler, provider-specific orchestration framework, or broad always-on workflow engine.

## Decision drivers

- The singleton daemon and global scheduler must remain the sole durable/heavy-work admission authority.
- Delegated agent runs need stable identity, lineage, cancellation, restart, status, and result semantics independent of transient worker handles.
- Parallel write-capable children should not share an index or working tree by default.
- Parent agents should not have to poll repeatedly merely to learn that a child progressed or completed.
- Communication must remain bounded, attributable, and incapable of silently expanding child authority.
- Git isolation should simplify orchestration rather than move merge/conflict complexity into ad-hoc filesystem coordination.
- Child results should be structured enough to integrate without parsing arbitrary final prose.
- Existing `SubAgentPool`, `TaskStore`, subagent events, worktree helpers, and task tool behavior need a bounded migration path.
- Background execution should be limited to scheduler-owned operations with meaningful independent lifetimes, not every cheap tool call.

## Considered options

### Option A — Keep subagents and worktrees independent

Continue improving `SubAgentPool` and separately add richer worktree commands. Parents would decide when to create worktrees and pass paths to children.

Benefits:

- smallest immediate code change;
- preserves current abstractions.

Costs and failure modes:

- parent models must reason about physical isolation details;
- parallel mutation remains easy to misconfigure;
- child identity, worktree ownership, cleanup, and Git provenance stay loosely correlated;
- two concurrency/admission layers remain;
- restart recovery cannot reliably reconstruct ownership from transient handles and paths.

Rejected as the end state.

### Option B — Make every tool call a detached asynchronous job

Convert ordinary tools to background handles and let the model await or poll them.

Benefits:

- uniform surface;
- maximal theoretical concurrency.

Costs and failure modes:

- unnecessary latency and storage for cheap reads;
- pervasive ordering/race complexity;
- degraded reasoning because immediately useful tool results no longer return in the normal turn;
- duplicates capabilities already provided by parallel tool batches and Tool Programs.

Rejected. Normal tool batches remain canonical for cheap/turn-local work.

### Option C — Durable AgentRun as the delegated execution unit, with automatic worktree leases for mutation-capable parallel children

Represent delegated/background agent execution as a durable `AgentRun` linked to an `AgentTask`, scheduler job/attempt, explicit execution context, optional worktree lease, bounded mailbox, stable-boundary event journal, and structured result. Read-only descendants may inherit a parent worktree. Mutation-capable delegated runs receive a daemon-owned isolated worktree by default when concurrent mutation is possible. The scheduler remains the sole admission authority.

Selected.

## Decision

CodeGG will implement scheduler-owned durable `AgentTask`/`AgentRun` execution with these properties:

1. `AgentTaskId` identifies a durable delegated objective/request; `AgentRunId` identifies an execution of that task. A run records root/parent lineage, session/turn, project/repository/workspace/worktree/node, agent definition/digest, provider/model, authority ceiling, budgets, scheduler job/attempt lineage, status, timestamps, and output references.
2. In daemon production paths the global scheduler is the sole admission and machine-resource concurrency authority. `SubAgentPool` may remain temporarily as an execution/compatibility adapter but must not remain an independent daemon admission policy after migration.
3. Delegated runs are asynchronous by default: spawn returns a stable handle. Parents may continue work, wait with a bounded policy, cancel, inspect status, or send bounded control messages.
4. Parent/child communication uses a durable run mailbox. Initial authority is parent↔direct-child plus parent→owned-group broadcast. Free-form sibling communication is deferred.
5. Mailbox delivery occurs at safe agent-loop boundaries. `message` queues context/instruction for the next safe boundary. `interrupt` requests trajectory reconsideration at the next cancellable boundary; it does not magically preempt an irreversible tool side effect.
6. A stable-boundary append-only `AgentRunEvent` journal records execution lifecycle and recoverable control state. It is execution evidence, not hidden model reasoning and not a replacement for the derived session projection stream.
7. Read-only children may inherit the parent worktree when their effective capabilities prohibit mutation. A mutation-capable delegated run that may execute concurrently must acquire its own daemon-owned `WorktreeLease` before its `AgentLoop` starts.
8. A worktree lease binds one `WorktreeId` to repository, base commit, path, branch/ref strategy, owning run/principal, state, generation, and timestamps. Dirty/conflicted worktrees are never force-removed merely because a run failed or the daemon restarted.
9. A child with inherited `GitWrite` authority may create commits only inside its owned worktree. Push, force/history rewrite, mutation of another worktree, and integration into the parent branch remain separately authorized and are not implied by child commit authority.
10. Child completion returns a typed `AgentRunResult` containing status, summary, worktree/base/result commit identities when applicable, changed paths, validation evidence, findings, artifacts, unresolved conflict state, and retry/recovery classification. Final prose may supplement but does not define the machine contract.
11. Integration is explicit. The runtime may offer typed merge/rebase/cherry-pick/handoff operations through existing Git services, but it does not silently merge successful child work into the parent branch.
12. Run groups support bounded join policies (`all`, `any_successful`, `first_completed`, `detached`) only after durable ownership, cancellation, notification, and cleanup are implemented. Detached work remains scheduler-owned and observable.
13. Background handles are available only for existing/new scheduler-backed operations with independent lifetimes, such as agent runs, tests/builds/lints/formats, research, Tool Programs, and comparable durable jobs. Ordinary cheap read tools continue to use normal parallel batches.
14. Completion/progress notification is pushed into the owning session/run control path when useful, with bounded summaries and artifact handles. The model should not need repeated `get` polling for normal completion discovery.
15. Session projections, TUI/ACP consumers, and audit seams derive their run/worktree views from the authoritative run/worktree stores and journal. They remain non-authoritative presentation layers.

## Consequences

### Positive

- Parallel implementation agents stop sharing a mutable working tree/index by default.
- Parent planning can delegate semantically independent work without micromanaging disjoint file paths solely to avoid physical collisions.
- Child commits become safe, attributable integration artifacts rather than being globally prohibited.
- Parent agents can steer or cancel ongoing children and receive completion without polling loops.
- Scheduler resource/fairness limits apply uniformly across roots, sessions, projects, and child agents.
- Restart recovery can reconstruct durable run/worktree ownership instead of guessing from Tokio handles and path strings.
- Existing typed Git conflict and recovery machinery becomes the integration boundary for concurrent work.

### Negative

- Durable run, mailbox, journal, and worktree records add storage/migration/retention obligations.
- Worktree creation has filesystem and Git overhead and must be bounded; it is inappropriate for every read-only scout.
- Integration conflicts still exist semantically and move to an explicit Git boundary rather than disappearing.
- Existing subagent compatibility types and projection events require a staged migration.

### Neutral or deferred

- Cross-daemon agent runs and remote worktrees remain future distributed-execution work.
- Team principals and project authorization remain separate; local-owner authority is sufficient for the first implementation milestones.
- Automatic speculative multi-model racing is not required.
- Direct sibling mailboxes are deferred.
- Arbitrary provider-specific multi-agent workflow protocols may map into these contracts later but are not canonical.
- Rewind to arbitrary historical points is deferred; checkpoint-based rewind can be considered after run/worktree checkpoints are correct.

## Compatibility and migration

- Existing `task` tool spawn/get semantics remain available through aliases/adapters while new run actions are added.
- Existing `SubAgentTask`, `TaskStore`, `SubAgentRequest`, and `SubAgentPool` remain until the durable service has production consumers and restart/cancellation evidence.
- Existing `SubagentStarted/Progress/Completed/Failed` events remain derivable during the compatibility window from authoritative run transitions.
- Existing worktree CLI/TUI operations remain available; daemon-owned leases add ownership rather than removing manual user worktrees.
- Existing sessions/tasks are not required to be retroactively promoted into fully recoverable historical runs. Migration must preserve readable legacy records and mark missing provenance explicitly.

## Security and reliability implications

- Child authority is always the intersection of principal/session/parent agent/resolved child agent/workspace/worktree/tool policy; worktree ownership never grants broader tool or network authority.
- Mailbox messages cannot widen capability sets, paths, model budgets, or Git rights.
- Worktree paths are daemon-generated or validated under a configured CodeGG worktree root and cannot escape repository/project policy through symlinks or crafted branch/path inputs.
- Scheduler cancellation propagates through run descendants and owned jobs. Worktree cleanup happens only after execution ownership is terminal and safety checks pass.
- Stable-boundary journal replay must never repeat a completed non-idempotent tool or Git mutation. Recovery resumes from durable state or fails explicitly when replay safety cannot be proven.
- Shared resources such as build caches, integration databases, ports, and repository-level Git mutations remain scheduler-exclusivity concerns even when filesystem worktrees are isolated.

## Verification

Implementations conform only when tests prove:

- one delegated task produces one durable task/run identity and one scheduler-owned execution lineage;
- duplicate delegation is idempotent or returns an explicit conflict without duplicate children;
- parent cancellation reaches descendants and terminal state is recorded exactly once;
- parent messages/interrupts are ordered, bounded, restart-safe, and never widen authority;
- two concurrent mutation-capable children receive distinct worktrees and indexes;
- read-only children can reuse a worktree without acquiring mutation authority;
- child commit is permitted only in the owned worktree and push/history rewrite remains denied unless separately authorized;
- dirty/conflicted worktrees survive failure/restart for inspection rather than being force-deleted;
- a completed child exposes typed commit/diff/validation evidence;
- join policies terminate deterministically across success, failure, cancellation, and restart;
- scheduler-backed background operations notify owners without requiring unbounded polling;
- session projection/TUI state can reconstruct the run tree from authoritative records/events;
- compatibility adapters can be removed without leaving a daemon bypass or second admission authority.

## Supersession

None.
