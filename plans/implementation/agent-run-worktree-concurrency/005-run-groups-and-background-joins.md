# Agent Run, Async Delegation, and Worktree Concurrency Milestone 005 — Run Groups and Background Joins

Status: blocked

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m005--run-groups-join-policies-and-scheduler-backed-background-handles`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: capability

Hard blockers: M002 and M004 must close.

## 1. Objective

Add bounded multi-run fan-out and deterministic join semantics so a parent can launch several independent child runs, continue its own work, and later await `all`, `any_successful`, `first_completed`, or safely `detached` completion. Reuse the same durable wait/notification semantics for existing scheduler-backed long-running operations where asynchronous lifetime is valuable.

This is intentionally not a general workflow language and does not turn ordinary cheap tools into detached jobs.

## 2. Why this milestone becomes ready after M002 and M004

M002 provides durable ordered status/control/wait/notification semantics. M004 provides safe worktree isolation and structured child results for mutating fan-out. Existing scheduler infrastructure already provides durable jobs, dependencies, cancellation, resource admission, fairness, retries where explicitly allowed, and bounded completions. Tool Programs already own deterministic multi-call orchestration for predictable tool loops and should not be duplicated here.

## 3. Current implementation evidence

Reconfirm before editing:

- current nested-delegation policy already has depth/fan-out/active descendant/tool-call budget seams;
- parents can spawn multiple tasks asynchronously but do not have a first-class durable group identity or join policy contract;
- long-term plans already name `all`, `any_successful`, `first_completed`, and `detached` policies;
- scheduler jobs have dependency/cancellation/wait mechanics and durable terminal state;
- tests/builds/lints/formats/research/Tool Programs/subagents already have scheduler-owned or scheduler-migrated execution paths with independent lifetimes;
- ordinary `ToolBatchExecutor` already executes independent tool calls in parallel and should remain the default for turn-local work.

## 4. Invariants that must not regress

- A run group is a thin durable coordination object over existing runs/jobs, not a second scheduler or general DAG engine.
- Group width, descendants, active jobs, waiters, result summaries, and notification volume are bounded.
- Group creation does not bypass per-root delegation policy or scheduler resource admission.
- Join outcome is deterministic from member terminal states and policy.
- Parent/group cancellation propagates according to one documented rule and cannot cancel unrelated roots.
- `detached` means durable scheduler/run ownership continues after the spawning parent turn completes; it does not mean unowned background Tokio work.
- Detached work remains observable, cancellable, restart-recoverable, resource-bounded, and associated with a root session/principal/service.
- Scheduler-backed background handles are offered only when an operation already has or receives durable job ownership.
- Normal parallel tool batches remain synchronous-to-the-turn result collection and are not routed through background handles unnecessarily.
- Tool Program orchestration remains the canonical deterministic programmatic tool-loop mechanism; run groups do not duplicate its IR/interpreter.

## 5. Scope

### In scope

- durable `AgentRunGroupId`/record if a distinct stable group identity is required;
- group member relation/order, root owner, join policy, status, timestamps, bounded terminal summary;
- APIs/model-facing actions to spawn multiple children from a bounded list or create a group from accepted run handles;
- join policies: `all`, `any_successful`, `first_completed`, `detached`;
- policy-specific cancellation of remaining members when appropriate and explicitly configured;
- bounded `wait_group`/status/result aggregation;
- push of group terminal/attention notification through M002;
- restart recovery of group status from durable member states;
- asynchronous handle/wait/notification adapters for eligible scheduler-backed jobs: tests/builds/lints/formats, research, Tool Programs, and other already-durable operations with independent lifetime;
- clear model/tool guidance on when to use normal direct/parallel tools versus background jobs/runs;
- resource/fan-out guards preventing a model from spawning unlimited children/jobs.

### Explicitly out of scope

- arbitrary user-defined dependency DAGs beyond existing scheduler dependency support;
- loops/conditionals/workflow scripting (Tool Programs already cover bounded deterministic tool programs);
- speculative model racing as a default policy;
- automatic best-result ranking by another model unless an explicit parent turn chooses to review results;
- converting filesystem reads/searches/simple Git status/diff into background jobs;
- cross-session/global notification UX redesign;
- project chat or sibling free-form messaging.

## 6. Required production changes

### Core/domain

Define a group record only if it provides durable value not already expressible by a parent task. A representative contract:

```rust
pub enum RunJoinPolicy {
    All,
    AnySuccessful,
    FirstCompleted,
    Detached,
}

pub struct AgentRunGroupRecord {
    pub group_id: AgentRunGroupId,
    pub root_run_id: AgentRunId,
    pub owner_run_id: AgentRunId,
    pub member_run_ids: Vec<AgentRunId>,
    pub join_policy: RunJoinPolicy,
    pub cancel_remaining_on_satisfaction: bool,
    pub status: RunGroupStatus,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}
```

Keep member counts bounded and normalized if the repository’s storage conventions prefer a relation table.

### Join semantics

Define exact satisfaction rules:

- `all`: terminal when all members terminal; group success policy should report aggregate success/failure counts rather than hiding partial failure.
- `any_successful`: terminal-success on first successful member; if all members terminal without success, terminal-failure. Remaining active members may be cancelled only if configured.
- `first_completed`: terminal when any member reaches a terminal state, regardless of success; optional cancellation of remaining members.
- `detached`: spawn returns after durable acceptance; no automatic parent-turn wait. Group remains active until all members terminal or explicitly cancelled. Owner receives bounded completion/attention notification.

Do not infer cancellation policy from name alone; persist it explicitly.

### Group service

Create a small service that:

1. validates owner/root and member count;
2. creates members through canonical M001 run service, preserving M004 isolation policy;
3. persists group/member relation before declaring accepted;
4. recomputes group status from authoritative member run states;
5. exposes bounded wait/status/result aggregation;
6. issues M002 notifications on satisfaction/attention;
7. handles cancel according to group policy;
8. recovers after restart without re-spawning terminal members.

Avoid a polling loop per group. Recompute on member terminal events plus bounded query/reconciliation.

### Model-facing delegation surface

Prefer extending the existing task/delegation tool with bounded actions/fields rather than adding many tools. Possible shapes:

- `spawn_many` with at most configured N child requests and a join policy;
- `wait_group`/`status_group`/`cancel_group`;
- or `spawn` returning a group when requests array is supplied.

Choose the smallest schema compatible with current model profiles and tool-surface complexity. Existing single `spawn` remains canonical for one child.

### Scheduler-backed background handles

For eligible existing job-producing tools, add an optional asynchronous mode only where the normal operation already creates a durable scheduler job or can cleanly do so. The returned handle should be the canonical job/run ID, not a new detached-task abstraction.

Representative behavior:

```text
run_test(background=true) -> JobHandle
...parent continues...
wait/status or completion notification -> bounded result/artifact handle
```

If current model-facing test/build tools already submit and wait internally, factor submission from waiting rather than duplicating execution.

Research and Tool Programs may expose existing durable IDs through the same wait/notification facade. Do not bypass their domain-specific result/recovery authority.

### Resource policy

- group width bounded by configuration and root delegation budget;
- each member independently admitted by scheduler;
- no “reserve all resources for entire group” requirement that would deadlock/underutilize the scheduler;
- parent may continue while children are queued/running;
- optional join waiting consumes bounded waiter state, not a process slot.

### Result aggregation

Return member statuses and structured result handles from M004, not concatenated unbounded child transcripts. Example summary:

```text
Group complete: 3/4 successful, 1 failed
- run A: completed, commit ..., validation pass
- run B: completed, read-only finding ...
- run C: failed, preparation/worktree error ...
- run D: completed, commit ..., validation pass
```

Detailed diffs/logs remain behind artifacts/run IDs.

## 7. Ordered work packages

### A — Join contract and fixtures

Define exact policy/cancellation semantics and add pure state-machine tests.

Acceptance evidence:

- every combination of running/success/failure/cancel terminal states has deterministic policy outcome;
- no policy can remain nonterminal after its satisfaction condition is met.

### B — Durable group store/service

Implement bounded group/member state, create/query/recompute/cancel, and restart reconciliation.

Acceptance evidence:

- duplicate group creation with stable call identity is idempotent;
- member terminal event updates group once;
- restart recomputes without respawn.

### C — Model-facing fan-out

Add bounded spawn-many/group actions and result summaries.

Acceptance evidence:

- parent launches several read-only or mutating children with one bounded request;
- mutating members receive M004 isolated worktrees;
- root budgets/fan-out limits still apply.

### D — Wait/notification/cancellation

Integrate M002 bounded wait and completion push.

Acceptance evidence:

- parent can continue work and receive group completion;
- cancel-group cannot affect unrelated runs;
- first/any policies optionally cancel remaining members exactly as configured.

### E — Scheduler-backed background job adapter

Add optional handle-return mode to the smallest useful existing long-running job surfaces.

Start with one or two production consumers (for example test and research/Tool Program) and generalize only if the contract is genuinely shared.

Acceptance evidence:

- background submission returns durable existing job/run ID;
- parent can wait/status/cancel through canonical job/run service;
- terminal notification arrives without polling;
- normal synchronous behavior remains default/compatible.

### F — Docs/tool guidance

Document when to use child fan-out, Tool Programs, background jobs, or ordinary parallel tools.

## 8. Failure, cancellation, restart, and contention semantics

- Member submission partial failure: group records exactly which members were accepted and which requests failed; either fail atomically before any submission if that is practical, or expose partial acceptance explicitly. Do not silently lose accepted children.
- `all`: one member failure does not automatically cancel others unless policy explicitly says fail-fast; default should favor collecting useful independent results.
- `any_successful`: if configured to cancel remaining after success, cancellation is persisted before live signals and terminalizes members normally.
- `first_completed`: first durable terminal sequence wins; simultaneous completions resolve by authoritative event/order identity, not wall-clock race in memory.
- `detached`: parent turn/session disconnect does not cancel; root/session deletion/shutdown policy remains explicit.
- Restart: group status recomputed from member records; completed members are never respawned.
- Background job wait timeout returns still-running, not failure.
- Scheduler contention naturally staggers member starts; group service must not busy-poll or reserve unavailable resources.

## 9. Compatibility and migration

- Existing single spawn/wait/get behavior remains.
- Existing task-tool schemas for models with fragile tool calling should gain fan-out features only if model profile can handle the added schema; tool exposure may remain curated/minimal where appropriate.
- Existing synchronous test/build/research behavior remains the default. Background mode is additive.
- Tool Programs remain separate and may be awaited/notified through shared job handles but are not represented as agent-run groups.
- No existing scheduler job IDs are rewritten.

## 10. Required tests

### Focused unit tests

- join satisfaction truth tables;
- group state transitions;
- member-count/result-size bounds;
- cancel-remaining policy;
- idempotent duplicate group creation.

### Integration tests

- three child runs with `all` and mixed success/failure;
- `any_successful` with cancellation of slow siblings;
- `first_completed` race;
- detached group completion after parent turn ends;
- mutating group members each use distinct worktrees;
- background test/research/Tool Program handle and notification.

### Restart and recovery tests

- restart with partially terminal group;
- restart after satisfaction before parent consumes result;
- detached group across daemon restart;
- no terminal member respawn.

### Contention and cancellation tests

- wide group exceeding configured width rejected/bounded;
- scheduler resource contention across several groups/projects;
- cancel group while some members queued and some running;
- unrelated group/root unaffected.

### Security and negative tests

- group cannot include unrelated existing run IDs without ownership;
- background mode cannot expose new authority/tool capability;
- result aggregation does not leak hidden reasoning/secrets.

## 11. Required verification commands

Expected focused shape after blockers close:

```bash
cargo test --lib agent
cargo test --lib scheduler
cargo test --test scheduler_contention
cargo test --test scheduler_cancellation
cargo test --test scheduler_restart_recovery
cargo fmt --all -- --check
```

Run direct tests for whichever test/research/Tool Program consumer gains background mode. Use one current quick broad pass at closure; do not add workflow matrices or performance gates.

## 12. Documentation updates

- `architecture/agent.md` — group/fan-out semantics.
- `architecture/scheduler.md` — groups as coordination over independent jobs, not admission owner.
- Tool Program docs — clarify distinction from run groups.
- test/research/tool docs for optional background handles.
- source roadmap status after closure.

## 13. Acceptance criteria

1. Parent can create a bounded group of durable child runs without bypassing root/scheduler limits.
2. `all`, `any_successful`, `first_completed`, and `detached` have deterministic documented outcomes.
3. Group cancellation affects only owned members and follows explicit remaining-member policy.
4. Detached work remains scheduler-owned, observable, cancellable, restart-safe, and bounded.
5. Group results aggregate structured run handles/summaries rather than transcripts.
6. Mutating fan-out inherits M004 worktree isolation automatically.
7. At least one useful scheduler-backed non-agent operation can return a background handle and later notify/wait without polling loops.
8. Ordinary direct/parallel tool execution remains the default and does not regress.
9. Tool Programs remain the programmatic tool-loop authority.
10. Focused join/restart/contention tests pass.

## 14. Stop conditions

Stop if:

- M002/M004 are not closed;
- group semantics require a second scheduler or independent resource reservation system;
- detached work would exist only as unowned Tokio handles;
- the model-facing schema expands into a generic workflow language;
- background mode requires converting cheap direct tools into durable jobs with no independent-lifetime benefit;
- partial group submission cannot be represented safely and no atomic/explicit-partial contract can be established.

## 15. Closure evidence required

- implementation/review commits;
- join truth table and cancellation policy;
- all/any/first/detached production-shaped fixtures;
- restart/detached evidence;
- fan-out worktree isolation evidence;
- scheduler-backed background consumer evidence;
- proof ordinary parallel tools/Tool Programs remain separate authorities;
- exact verification results and unresolved findings.

## 16. Handoff notes

Keep this milestone intentionally thin. The user-facing win is parallel delegation with predictable joins and less babysitting, not a new orchestration DSL. Reuse scheduler/job/run identities everywhere possible.
