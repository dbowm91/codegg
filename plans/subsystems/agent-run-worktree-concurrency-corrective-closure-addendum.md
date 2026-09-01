# Agent Run, Async Delegation, and Worktree Concurrency Corrective Closure Addendum

Status: active — M007 implemented; closure pending; M008 blocked on M007 closure

Repository baseline reviewed: `b87d1d5b65aca96c700deb27e579374b3d158545`

Superseded strict subsystem closure claim:

- `plans/closure/agent-run-worktree-concurrency/006-status.md`

Historical source roadmap retained:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

Corrective implementation plans:

- `plans/implementation/agent-run-worktree-concurrency/007-durable-lineage-context-and-fanout-corrective-pass.md`
- `plans/implementation/agent-run-worktree-concurrency/008-call-identity-projection-and-strict-closure.md`

Related accepted ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Long-term references:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent task, agent run, budget, worktree, execution context
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`
- `plans/003-planning-process.md#7-corrective-passes`

## 1. Purpose and corrective disposition

M001–M006 landed substantial durable-run, mailbox, worktree, result, group, and projection infrastructure. The M006 closure record then declared strict subsystem closure. A post-closure production-path audit of the exact closed head found integration defects that invalidate the user-visible multilevel asynchronous orchestration claim even though much of the underlying machinery is correct.

This addendum does not rewrite or delete M001–M006 history. Their implementation and closure records remain evidence of what each pass established. The strict subsystem disposition from M006 is superseded until this corrective addendum closes.

The corrective work is intentionally narrow. It must make the already-built components compose correctly before any further agent-workflow features are added.

## 2. Post-closure findings

### F1 — root fan-out has no valid owner context

The normal root/session `TaskTool` receives the durable run store, run-control service, run-group service, project/repository context, and turn ID, but it is not given an owning `AgentRunId`. Group actions currently require `parent_run_id`, so root-agent `spawn_many`/group actions fail even though they are advertised on the model-facing tool surface.

The correction MUST NOT fabricate a delegated `AgentTask`/scheduler-owned `AgentRun` merely to represent the primary turn. ADR-0002 defines durable `AgentTask`/`AgentRun` as the delegated execution unit. Root coordination therefore needs an explicit turn-owned orchestration context while nested coordination remains run-owned.

### F2 — current-run and parent-run identity are conflated across the scheduler/worker boundary

`SubagentJobExecutor` carries the durable child `run_id`, but constructs the child request with `parent_run_id: None` and resets `depth` to `1`. Child runtime construction then configures its nested `TaskTool` from `request.parent_run_id` instead of the currently executing `request.run_id`.

The result is that a durable child does not reliably become the owner of descendants it spawns. Grandchildren can lose parent/root lineage and associated control/group semantics.

### F3 — nested execution context is incomplete

Nested `TaskTool` construction does not propagate all durable project/repository/turn/group context used by the root tool. A nested mutation-capable child can therefore reach worktree preparation without repository identity, and nested group operations do not receive the complete group service/owner context.

### F4 — durable depth exists as a budget seam but is not authoritative

`AgentRunBudget.max_depth` exists, but durable scheduler hops reset worker depth to `1`. Depth is not persisted as authoritative run state and the durable budget is not consistently enforced before child acceptance. This weakens bounded recursion and makes projected depth unreliable.

### F5 — control authorization does not match ADR-0002

`RunControlService::authorize` currently accepts any actor whose session ID equals the target task session before considering run lineage. Its ancestry walk then climbs from the actor and accepts when the actor is a child of the target, which is the reverse of the intended parent-to-child relationship.

ADR-0002 allows initial parent↔direct-child control plus parent→owned-group control and explicitly defers unrestricted sibling messaging. Same-session authority is therefore too broad, while the lineage check is directionally incorrect.

### F6 — default tool-call idempotency collapses distinct operations

The normal tool execution framework already supplies a stable per-model-tool-call `ToolExecutionContext.invocation_key`. `TaskTool` does not consume it. Instead:

- control operations default to an idempotency key based only on action + target run;
- delegated spawn identity is derived from session/turn/request content rather than the model tool-call identity;
- group creation defaults to request-content hashing.

This can collapse two intentional calls with the same target or payload in one turn, while requiring the model to invent idempotency keys to get reliable communication.

### F7 — projection depth is not authoritative

The durable projection DTO can represent arbitrary depth, but scheduler publication currently supplies only `0` or `1` based on whether `parent_run_id` is present. A valid multilevel tree therefore cannot be represented accurately even if storage lineage is correct.

## 3. Ownership boundary

This corrective addendum owns:

- explicit orchestration ownership for root turns versus durable delegated runs;
- unambiguous current/root/parent run identity across TaskTool, scheduler payload, executor, worker, and child AgentLoop construction;
- durable run depth and max-depth enforcement;
- propagation of project/repository/workspace/worktree/session/turn/group context to descendants;
- root and nested `spawn_many` reachability using the existing run-group service;
- direct parent/child and turn-owner control authorization consistent with ADR-0002;
- nested mutation worktree allocation using canonical repository lineage and the correct parent/base state;
- canonical model tool-call identity for delegation/control/group idempotency;
- authoritative projection depth and related reconnect/resync evidence;
- strict corrective closure and registry reconciliation.

It consumes without redefining:

- singleton daemon and global scheduler machine-resource authority;
- `AgentTask`/`AgentRun` durable delegated-execution semantics from ADR-0002;
- `WorktreeService`, typed Git operations, child local-commit policy, and explicit integration service;
- run mailbox/journal persistence and stable-boundary delivery;
- existing projection reducer/replay architecture;
- Tool Programs and normal parallel tool batches;
- legacy numeric task compatibility during the existing compatibility window.

It does not own:

- making the primary root turn a scheduler job;
- a new workflow language or DAG engine;
- unrestricted sibling communication;
- cross-session/project chat;
- cross-daemon agents or remote worktrees;
- automatic parent-branch integration;
- general rewind/checkpoint UX;
- new CI/release automation, scanners, coverage gates, or benchmark gates.

## 4. Corrective architecture

### 4.1 Explicit orchestration owner

Use one typed orchestration-owner contract instead of overloading `parent_run_id`:

```rust
pub enum AgentOrchestrationOwner {
    Turn {
        session_id: String,
        turn_id: String,
    },
    Run {
        run_id: AgentRunId,
    },
}
```

Names may vary to fit existing conventions, but semantics MUST remain explicit.

- Root/session TaskTool instances use `Turn` ownership.
- A durable child AgentLoop uses `Run { current_run_id }` ownership.
- `parent_run_id` remains lineage data about the current run; it is not reused to mean “the run whose TaskTool is executing.”
- No synthetic delegated task/run is created for the primary turn.

### 4.2 Durable delegated execution context

Carry one typed context across the scheduler/worker boundary containing at least:

- current `AgentRunId` when durable;
- `root_run_id` and `parent_run_id`;
- authoritative depth;
- session and originating turn;
- project/repository/workspace/worktree identity;
- resolved workspace root/current checkout root;
- authority/budget references or immutable bounded values needed by descendant admission.

The child runtime must derive its nested TaskTool from this context rather than rediscovering identity from display session IDs or paths.

### 4.3 Run-group ownership

The existing group contract must support both legitimate owner forms:

- root turn-owned group: members are top-level durable runs accepted from the same session/turn and have no parent run;
- run-owned group: members are direct durable children of the owning run and share its durable root lineage.

Persist owner kind and owner identity explicitly. Do not make `owner_run_id` mandatory for a root-turn group and do not weaken nested group validation to “same session.”

### 4.4 Control authorization

Authorization must be based on explicit owner/lineage:

- a root turn owner may control only top-level runs originating from that same session + turn, plus groups it owns;
- a run owner may control its direct child runs and groups it owns;
- child→parent control is denied unless a future ADR explicitly adds it;
- sibling control is denied;
- session equality alone is never sufficient;
- group cancellation/broadcast uses group ownership, not same-session reachability.

Parent cancellation may still cascade through descendants through the existing scheduler/run cancellation mechanism.

### 4.5 Depth

Persist authoritative run depth or another equally direct bounded representation.

- first delegated child of a root turn: depth `1`;
- child of durable run at depth `N`: depth `N + 1`;
- `root_run_id` propagates from the parent;
- configured/root budget `max_depth` is checked before durable task/run creation or scheduler submission;
- store/service relation validation rejects inconsistent parent/root/depth records;
- worker semantic depth uses the durable value and never resets scheduler-owned requests to `1`.

### 4.6 Call identity

Use accepted model tool-call identity (`ToolExecutionContext.invocation_key`) as the default retry/idempotency identity for model-originated TaskTool actions. Explicit API/model-provided idempotency keys remain supported as overrides.

Distinct tool calls with identical payloads MUST remain distinct. A transport/provider retry of the same tool-call identity MUST resolve to the same accepted operation where the operation supports retry deduplication.

## 5. Milestone sequence

### M007 — durable lineage, owner context, fan-out, and authorization corrective pass

Primary class: invariant / correctness / capability.

Plan:

- `plans/implementation/agent-run-worktree-concurrency/007-durable-lineage-context-and-fanout-corrective-pass.md`

Status: implemented; closure evidence pending.

Hard dependencies:

- historical M001–M006 implementations remain present;
- ADR-0002 remains accepted.

Exit conditions:

- root `spawn_many` is reachable through a turn-owned group context;
- child→grandchild durable lineage has correct parent/root/depth;
- nested group fan-out works from the currently executing run;
- nested mutating descendants receive distinct correctly based worktrees with repository identity intact;
- max-depth admission fails before child scheduler execution;
- root-turn and run-owner control authorization rejects siblings, children controlling parents, unrelated turns, and forged same-session actors;
- scheduler remains the sole machine-resource authority.

### M008 — call identity, authoritative projection, and strict corrective closure

Primary class: correctness / compatibility / closure.

Plan:

- `plans/implementation/agent-run-worktree-concurrency/008-call-identity-projection-and-strict-closure.md`

Status: blocked on M007 strict closure.

Exit conditions:

- two distinct sequential messages to one child are delivered as two messages without model-authored idempotency keys;
- retry of the same model tool-call identity does not duplicate spawn/control/group acceptance;
- two intentional identical spawn calls with different tool-call IDs create distinct children;
- `spawn_many` derives deterministic per-member call identities from the parent invocation;
- projection depth comes from authoritative durable run state and supports depth >= 2 through snapshot, incremental replay, reconnect, and resync;
- focused production-shaped regression tests pass;
- repository quick verification and existing ownership/static guards pass;
- a new closure record independently reviews M007/M008 and explicitly supersedes the M006 subsystem disposition without rewriting M001–M006 evidence.

## 6. Dependency graph

```text
historical M001-M006 implementation
          |
          v
M007 lineage/owner/fan-out/auth correction
          |
          v
M008 call identity/projection/strict closure
```

No unrelated subsystem is reopened.

## 7. Required production-shaped regression scenarios

At corrective closure, tests must cover at least:

1. root turn → `spawn_many` → three durable children → group wait completes;
2. root turn → child → grandchild with exact parent/root/depth persisted;
3. child at depth 1 → `spawn_many` → two direct grandchildren accepted into a run-owned group;
4. configured max depth rejects the next descendant before job execution;
5. nested mutation child receives a distinct managed worktree and preserves canonical repository identity/base lineage;
6. parent can message/cancel direct child; sibling cannot message sibling; child cannot control parent; another turn in the same session cannot control the target by session ID alone;
7. two distinct message tool calls to the same run both arrive in order;
8. replay/retry of one invocation key produces one accepted operation;
9. two identical spawn payloads with distinct invocation keys produce two runs;
10. projection snapshot/replay contains depth 0/1/2+ as actually represented by authoritative records rather than by presence/absence of a parent field.

Tests should use deterministic/mock providers and repository fixtures where possible. A mandatory live-provider test is not required.

## 8. Verification posture

Keep verification proportional to this corrective scope.

Required focused verification:

- core durable run/lineage/depth tests;
- run-control authorization/mailbox tests;
- run-group owner/join tests;
- scheduler subagent execution/cancellation/contention tests affected by context propagation;
- worktree nested-isolation tests;
- task-tool production-path tests;
- projection reducer/replay/snapshot tests;
- existing scheduler-bypass, execution-ownership, core-boundary, Git forbidden-pattern, identity/path, projection-disclosure, and daemon-cwd guards that touch this boundary.

Required broad verification at final M008 closure:

- repository `scripts/verify.sh quick` or its current documented equivalent;
- format/diff check if not already included.

Do not add new CI lanes, dependency bots, broad fuzz infrastructure, coverage thresholds, benchmark gates, binary-size gates, release workflows, or recurring hosted verification solely for this corrective pass.

## 9. Closure governance

M001–M006 closure records are historical evidence and MUST NOT be rewritten to hide this audit.

M007 and M008 each require their own closure record. M008’s closure record must include:

- the M006 record as the superseded strict subsystem disposition;
- exact implementation commits;
- requirement-to-evidence matrix for every F1–F7 finding;
- focused regression results;
- broad quick-verification result;
- authorization/security review;
- restart/cancellation/contention review;
- migration/backward-compatibility evidence;
- unresolved findings by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

Subsystem status returns to `closed` only after M008 closure is accepted.

## 10. Stop conditions

Stop and propose an ADR rather than improvising if implementation would require:

- converting primary turns into scheduler jobs;
- redefining `AgentTask` away from a delegated unit of intent;
- creating a second scheduler/admission authority;
- enabling unrestricted sibling or cross-session control;
- weakening worktree isolation or Git authority to make nested delegation easier;
- changing the canonical long-term meaning of `AgentRun` or worktree ownership.

No ADR is currently required for the corrective design above.
