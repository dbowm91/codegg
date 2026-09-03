# Agent Convergence and Independent Verification Roadmap

Status: active planning — M002 ready

Repository baseline reviewed: `1bee32578566cc6cdf4025002af781309d8f29f4`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`
- `plans/003-planning-process.md`

Existing decisions and closed dependencies:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`
- `plans/closure/agent-run-worktree-concurrency/009-status.md`
- `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md`
- `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`
- `architecture/agent.md`
- `architecture/worktree.md`
- `architecture/goal.md`

External design input:

- MiniMax, “MiniMax Agent Team: Built for Long-Running Tasks and Continuous Evolution,” 2026-05-27, `https://www.minimax.io/blog/minimax-agent-team-long-running-1779893953`.
- MiniMax, “MiniMax M2.7: Early Echoes of Self-Evolution,” 2026-03-18, `https://www.minimax.io/blog/minimax-m27`.
- MiniMax Agent, “AI Website Builder,” `https://agent.minimax.io/tools/ai-website-builder`.

These sources are design input only. CodeGG's daemon, scheduler, authority, Git/worktree, goal-verification, provider, and projection contracts remain canonical.

## 1. Purpose and ownership boundary

CodeGG already has the hard infrastructure required for reliable multi-agent execution: scheduler-owned durable `AgentRun` records, bounded run groups, run-control mailboxes, worktree isolation for mutating children, structured `AgentRunResult` records, explicit Git integration, and host-owned goal completion verification. The missing capability is a small control policy that turns those primitives into an independent produce/verify loop with explicit stop and repair decisions.

This subsystem owns:

- a durable, bounded convergence state machine above existing `AgentRun` and `AgentRunGroupService`;
- explicit producer and independent verifier roles for one delivery objective;
- a bounded verifier input packet derived from authoritative run/Git/validation evidence rather than producer hidden reasoning;
- typed semantic verdicts and owner decisions between cycles;
- bounded repair/replan cycles after the single-cycle contract is proven;
- projection of convergence status for TUI/ACP/observer consumers through the existing session-projection path;
- conservative model-profile/configuration hints governing when automatic convergence is appropriate.

It consumes, but does not redefine:

- scheduler admission, fairness, cancellation, and job attempts;
- durable agent-run identity, ownership, lineage, idempotency, and run groups;
- run-control `message`, `interrupt`, `wait`, and `cancel` semantics;
- `WorktreeService`, worktree leases, child commit authority, and explicit integration;
- `AgentRunResult`, validation evidence, findings, artifacts, and repository state;
- the existing `RecoveryController`, which detects lack of execution progress rather than artifact correctness;
- `GoalVerificationService`, which remains the only authority that can accept host-owned evidence for a goal completion transition;
- model profiles, prompt compilation, agent definitions, permissions, and runtime asset snapshots.

The governing rule is:

> Convergence coordinates already-authorized durable runs; it never becomes a second scheduler, never grants authority, and never turns an LLM verifier into completion authority.

## 2. Why this is worth adding

MiniMax's public Agent Team design makes two points that map cleanly onto CodeGG.

First, a worker that produced an artifact should not be the only judge of whether the artifact is deliverable. MiniMax therefore separates Leader, Worker, and Verifier roles and treats Worker and Verifier as adversarial quality roles. Their Team Engine manages producing, verifying, and retry transitions rather than relying on a prompt that says “double-check your work.”

Second, MiniMax explicitly warns that unstructured multi-agent execution can consume materially more time and tokens without improving quality. Their design uses state transitions, acceptance criteria, role isolation, and stop conditions. This is consistent with CodeGG's existing preference for durable host-owned state over open-ended model loops.

CodeGG should adopt the transferable control policy while retaining stricter host authority:

```text
owner turn/run
    |
    v
producer AgentRun(s)
    |
    v
structured AgentRunResult + host evidence
    |
    v
independent read-only verifier AgentRun
    |
    v
semantic verdict
    |
    v
explicit owner decision
    |-------------------------------|
    |                               |
  accept / stop               bounded repair / replan
    |                               |
    v                               v
host goal verifier          new scheduler-owned run(s)
(if goal completion)                 |
                                    `-> next bounded verification cycle
```

The producer/verifier model loop may recommend that work is acceptable. It does not prove goal completion, silently merge a child branch, waive a failed test, or mutate a parent checkout.

## 3. Work classification

### Invariants

- The daemon scheduler remains the sole production admission/resource authority for every producer, verifier, repair, and replan run.
- `AgentRunGroupService` remains coordination over already accepted runs; convergence must not make it an executor or workflow scheduler.
- Every convergence operation has one exact owner: either the originating turn or one durable run. Owner/ancestor authorization must reuse the existing run-control/run-group ownership model.
- Child authority remains monotonic narrowing. Verifier authority is strictly read-only in the initial implementation.
- A verifier verdict is advisory semantic evidence. It cannot transition `GoalStatus::Complete`, override deterministic failed/missing host evidence, merge Git history, or approve permissions.
- Producer transcripts and hidden reasoning are not verifier authority and are not copied wholesale into verifier context.
- Verifier input and persisted verdicts are bounded. Large diffs/logs/artifacts remain behind existing handles or targeted reads.
- The exact bounded convergence objective and acceptance criteria are durable structured state (or an existing durable bounded artifact referenced by that state), so detached/restarted verification never has to reconstruct authority from transcript prose.
- Convergence has hard cycle, fan-out, token, tool, and wall-clock bounds inherited from the owning run/session plus subsystem-specific tighter caps.
- User cancellation, pause, steering, and permission decisions outrank model/engine continuation decisions.
- Restart must reconstruct convergence state from durable run/group/result records plus convergence records. It must not infer success from missing in-memory callbacks.
- A repair cycle may continue from a producer result only through an explicit Git commit/base identity. Dirty, conflicted, missing, or unverifiable repository state must not be silently carried into another worktree.

### Capabilities

- A root or delegated owner can request an independent review of a completed producer result without manually reconstructing the evidence packet.
- A single-cycle convergence run can move through produce -> verify -> owner decision with durable status and restart-safe identity.
- Later milestones can request bounded repair/replan cycles when the verifier returns actionable findings.
- The owner can inspect, steer, cancel, or stop convergence through the same run-control authority used for ordinary children.
- TUI/ACP/session projections can show the current cycle, producer/verifier run identities, verdict class, decision state, and remaining bounds without exposing hidden reasoning.
- Models with weak delegation/role adherence can remain on ordinary single-agent/task flows; convergence can be explicit or gated rather than universal.

### Infrastructure

- convergence record/store and deterministic state machine;
- cycle record with producer group/run, verifier run, verdict, decision, and provenance;
- bounded verifier evidence packet;
- convergence coordinator that submits through existing `TaskTool`/scheduler-owned delegation services rather than constructing agent loops directly;
- projection adapter and restart reconciliation.

### Polish

- concise model-facing descriptions explaining when independent verification is worth its cost;
- TUI status labels such as `producing`, `verifying`, `awaiting decision`, and `repairing`;
- diagnostics explaining why a convergence request was rejected, exhausted, or escalated.

## 4. Explicit non-goals

This roadmap does not:

- introduce a general DAG/workflow language;
- create a second scheduler, worker pool, admission semaphore, or resource authority;
- revive or extend the legacy file-backed `.opencode/team` inbox/outbox implementation;
- implement unrestricted sibling chat or a generic agent social network;
- allow a verifier to mutate files, run arbitrary shell commands, commit, push, merge, rebase, or answer permissions in the initial capability;
- automatically treat verifier prose as a passed test or host-owned completion criterion;
- auto-merge a producer or repair branch after a passing semantic verdict;
- retry until success without a fixed maximum number of cycles;
- make every ordinary task use multiple models/agents;
- assume MiniMax, M3, or any other model family is convergence-capable merely because a vendor markets native Agent Teams;
- require a new CI lane, benchmark gate, coverage gate, or release process.

A one-off second opinion remains an ordinary read-only `task spawn`; it does not need a convergence record unless the caller requests the formal produce/verify contract.

## 5. Current-state evidence

At baseline `1bee3257`:

- `crates/codegg-core/src/agent_run_group.rs` persists bounded groups of up to 16 already-accepted child runs and explicitly states that a group never admits work or owns an executor. Turn- and run-owned groups, deterministic joins, cancellation, notifications, and SQLite persistence already exist.
- `src/tool/task.rs` already exposes durable `spawn`, `spawn_many`, `status`, `message`, `interrupt`, `wait`, `cancel`, and group operations. New convergence operations should extend this orchestration surface rather than add a competing team tool family unless implementation evidence proves the schema would become materially less safe or comprehensible.
- `crates/codegg-core/src/run_result.rs` persists bounded machine-oriented run results containing result/base commits, changed paths, validation evidence, findings, artifacts, repository state, retryability, and recovery guidance. Producer transcript prose is explicitly not repository authority.
- `src/agent/run_control.rs` and the durable run-control store already provide owner-scoped message/interrupt/cancel delivery at safe boundaries with restart replay.
- mutation-capable children receive generation-fenced managed worktree leases; child integration into the parent remains explicit and requires recorded base/result identity.
- `crates/codegg-core/src/goal/verification.rs` has a stateless host verifier. Model claims are proposals; failed/in-flight tests and delegated runs remain authoritative, and unsupported free-form criteria currently require the user.
- `src/agent/progress_recovery.rs` detects repetitive/no-progress execution and provides bounded nudge/correct/replan/stall behavior. That machinery should inform convergence stall handling but is not an independent artifact reviewer.
- model profiles describe tool-call, instruction, and patch reliability plus tool limits, but have no explicit orchestration capability tier.
- the MiniMax model adapter is intentionally conservative (`medium` tool/instruction/patch reliability, max two parallel tools). Vendor Agent Team claims therefore must not become an implicit CodeGG default.
- the legacy `src/agent/team.rs` / `src/agent/teams.rs` file-backed team implementation is not the durable execution authority and is outside this roadmap.

The missing boundary is therefore narrow: durable convergence state, independent verifier construction, verdict/decision semantics, and bounded repair chaining.

## 6. Target domain and state machine

### 6.1 Convergence record

Introduce a small core domain whose exact names may vary, but whose semantics are equivalent to:

```rust
struct ConvergenceRecord {
    id: ConvergenceId,
    owner: AgentOrchestrationOwner,
    objective: String,
    criteria: Vec<String>,
    objective_digest: String,
    criteria_digest: String,
    status: ConvergenceStatus,
    current_cycle: u8,
    max_cycles: u8,
    created_at: i64,
    updated_at: i64,
    terminal_at: Option<i64>,
    idempotency_key: String,
}

enum ConvergenceStatus {
    Pending,
    Producing,
    Verifying,
    AwaitingDecision,
    Repairing,
    Replanning,
    Completed,
    Failed,
    Cancelled,
    Exhausted,
}
```

The persisted objective/criteria are the convergence-specific bounded execution specification, not the user's entire prompt or conversation. Recommended initial hard bounds are approximately 8 KiB for the objective, <=32 criteria, and <=1 KiB per criterion; implementation may choose tighter values. Digests support request fingerprinting/audit and never replace the durable bounded text needed after detach/restart.

`ConvergenceId` is a service-local typed durable handle. It does not replace `AgentRunId`, `AgentRunGroupId`, `JobId`, or any canonical identity relation. If implementation can use an existing typed correlation without semantic overload, prefer that; do not add a new global identity merely for display.

### 6.2 Cycle record

Each cycle records only bounded structural evidence:

```rust
struct ConvergenceCycleRecord {
    convergence_id: ConvergenceId,
    ordinal: u8,
    producer_group_id: Option<AgentRunGroupId>,
    producer_run_ids: Vec<AgentRunId>,
    verifier_run_id: Option<AgentRunId>,
    verdict: Option<SemanticVerificationVerdict>,
    decision: Option<ConvergenceDecision>,
    source_base_commit: Option<String>,
    result_commit: Option<String>,
}
```

The record does not persist complete transcripts, model reasoning, full diffs, secrets, or tool output.

### 6.3 Semantic verifier verdict

The verifier must return a typed bounded result such as:

```rust
enum SemanticVerificationVerdict {
    Pass {
        summary: String,
        evidence_refs: Vec<String>,
    },
    Revise {
        findings: Vec<AgentRunFinding>,
        repair_requests: Vec<String>,
    },
    Inconclusive {
        reason: String,
        missing_evidence: Vec<String>,
    },
}
```

A verifier `Pass` means only “the independent semantic reviewer found no blocking issue within its supplied scope.” It is not `GoalVerificationVerdict::Met`, not a permission approval, and not a Git integration authorization.

### 6.4 Owner decision

After semantic verification, the owner chooses from a host-enforced state-dependent set:

```text
Pass         -> accept | stop | escalate
Revise       -> repair | replan | stop | escalate
Inconclusive -> replan | stop | escalate
```

M002 initially implements the single-cycle subset. M003 enables `repair`/`replan` only after safe result-commit chaining is implemented.

The decision is a durable control operation, not implicit parsing of the Leader model's next prose message.

### 6.5 Verifier evidence packet

The host assembles the verifier input from:

- the durable bounded objective and acceptance criteria;
- producer `AgentRunResult`;
- base/result commit identity;
- changed-path list;
- validation statuses;
- structured findings/artifact handles;
- bounded diff summaries or targeted diff handles;
- relevant project instructions/skill digests already present in the verifier's pinned runtime asset snapshot.

Do not forward the producer's full chat history by default. The verifier should be independent enough to challenge assumptions instead of inheriting the producer's self-justification.

## 7. Repair and replan semantics

A verifier rejection must not cause an in-place mutable child loop to continue forever.

For code-producing runs, the preferred repair chain is:

```text
cycle N producer result_commit Cn
        |
        v
new scheduler-owned repair AgentRun
        |
        +-- new managed worktree
        `-- explicit base = Cn
        |
        v
result_commit Cn+1
        |
        v
new independent verifier
```

This preserves one owner per worktree lease and gives each cycle an auditable Git base/result relation. If the prior producer has no clean `result_commit`, is conflicted, or the commit cannot be resolved in the same repository, the host must refuse commit-based repair and return `escalate`/`replan` guidance.

A replan may start from the convergence's original parent base or another explicitly recorded clean base chosen by the owner. It must not guess from the current UI checkout.

Recommended defaults after M003:

```text
max_cycles = 2
hard_max_cycles = 4
max_producers_per_cycle = 3
```

The exact defaults may be configurable, but the hard upper bounds must live in host code and remain lower than or equal to existing run-group/tree limits.

## 8. Model-profile and invocation policy

MiniMax's own research says reliable Agent Teams require stable role boundaries, adversarial reasoning, protocol adherence, and behavioral differentiation. Those are model capabilities, not merely prompt instructions. CodeGG therefore should not expose “automatic team mode” indiscriminately.

M001 and M002 require no new model-profile fields. Explicitly requested convergence can operate with any configured verifier/producer models that satisfy ordinary agent resolution and permission checks; quality remains observable rather than assumed.

M003 may add one coarse profile/config contract, for example:

```rust
enum OrchestrationTier {
    SoloPreferred,
    DelegationCapable,
    ConvergenceCapable,
}
```

or an equivalent single field. Default unknown/custom profiles conservatively avoid automatic convergence. User configuration may override the tier. Built-in vendor profiles should be upgraded only with repository-owned evaluation or clear operational evidence, not marketing claims alone.

Automatic use, if implemented, is opt-in and budget-aware. The ordinary `task spawn` and solo agent paths remain the default for short, deterministic, low-risk tasks.

## 9. Dependency graph

```text
closed agent-run/worktree M009
closed goal-verification M013
closed model-adaptation/runtime assets
              |
              v
M001 durable convergence state + evidence contract
              |
              v
M002 single-cycle producer -> verifier -> owner decision
              |
              v
M003 bounded repair/replan + commit chaining + model/profile policy + projection polish
```

Dependency classes:

- M001 hard-depends on closed agent-run/worktree M009 and goal-verification M013. Those dependencies are closed, so M001 is ready.
- M002 hard-depends on M001 and has interface dependencies on the existing task/delegation, run result, run-control, agent-resolution, permission, and projection APIs.
- M003 hard-depends on M002 and has interface dependencies on managed worktree creation from explicit commits, model-profile configuration, and existing integration/projection services.

The memory-to-skill roadmap is independent. It may proceed in parallel and must not become a hard dependency for convergence.

## 10. Milestones

### M001 — Durable convergence cycle foundation

Status: closed

Class: invariant/infrastructure

Plan:

- `plans/implementation/agent-convergence/001-durable-convergence-cycle-foundation.md`

Objective:

Add the bounded convergence/cycle domain, durable bounded objective/criteria specification, SQLite/in-memory stores, state transition rules, idempotency, restart reconciliation, and verifier evidence DTO without launching any new model run from the convergence service.

Exit conditions:

- convergence and cycle records have bounded durable identity and exact owner provenance;
- objective/criteria survive detach/restart without transcript reconstruction;
- invalid/stale transitions fail closed;
- restart reconstructs nonterminal state from durable records and existing run/group/result state;
- verifier evidence can be assembled from `AgentRunResult` without copying full transcript content;
- no scheduler/tool/permission/goal authority changes;
- focused tests and `scripts/verify.sh quick` pass.

### M002 — Independent verifier and explicit owner decision

Status: ready

Class: capability/invariant

Plan:

- `plans/implementation/agent-convergence/002-independent-verifier-and-owner-decision.md`

Objective:

Implement one complete produce -> independent read-only verify -> durable owner-decision cycle using existing scheduler-owned child runs and task/run-control authority.

Exit conditions:

- a convergence request produces one scheduler-owned producer run, waits through existing bounded run/group mechanisms, then creates one independent verifier run;
- verifier authority excludes mutation, arbitrary shell, commit, integration, permission response, and goal completion;
- producer result/host evidence, not producer hidden reasoning, forms the verifier packet;
- verdict is typed/bounded and persisted;
- the exact owner can accept/stop/escalate, while repair/replan remain rejected until M003;
- passing semantic verification does not auto-merge or complete a goal;
- cancellation/restart/duplicate invocation races are deterministic.

### M003 — Bounded repair/replan, safe result chaining, and orchestration policy

Status: blocked on M002

Class: capability/reliability/polish

Plan:

- `plans/implementation/agent-convergence/003-bounded-repair-replan-and-model-gating.md`

Objective:

Add a hard-bounded repair/replan loop based on explicit result commits, multi-producer support where useful, coarse model-profile/config gating for automatic use, and complete projection/diagnostic behavior.

Exit conditions:

- repair starts a new durable run from an explicitly recorded clean result commit in a new managed worktree;
- unresolved dirty/conflicted/no-commit producer state cannot be silently reused;
- each new cycle gets a fresh independent verifier and consumes bounded remaining budget;
- hard cycle/fan-out limits cannot be expanded by model messages;
- automatic convergence is opt-in and restricted by a coarse orchestration capability/config policy;
- projection shows cycle/verdict/decision/remaining-budget summaries without hidden reasoning or oversized content;
- goal completion still requires existing host-owned verification.

## 11. Storage, protocol, migration, and compatibility

M001 will require a small SQLite migration for convergence and cycle records plus equivalent in-memory stores used by tests. Tables must use bounded strings/JSON, foreign/correlation references to existing run/group identities, and indexes only for demonstrated owner/status/recovery lookups. The durable bounded objective/criteria specification belongs to convergence storage (or an existing durable bounded artifact explicitly referenced by it); do not reconstruct it from model transcript text after restart.

Protocol/session-projection additions should be additive summaries. Old clients may ignore unknown convergence fields/events. Do not require a new transport or separate event bus.

Existing `task spawn`, `spawn_many`, group actions, manual worktrees, agent definitions, and goal behavior remain compatible. Convergence is an additional composition. The legacy file-backed team code is not migrated by this roadmap and must not become a dependency.

No existing run, group, goal, or worktree row needs retrospective conversion into a convergence record.

## 12. Security, cancellation, restart, and contention

- Convergence requests must authorize the owner through the same turn/run lineage used by task/run-group operations.
- Verifier agents receive the intersection of owner/session policy and a hard read-only verifier ceiling; a custom verifier agent cannot widen that ceiling.
- Verifier inputs must not contain secrets merely because a producer tool output did. Use structured results and existing redaction/artifact handles.
- Cancellation of the convergence cancels only its active owned producer/verifier/repair runs using existing run-control/scheduler paths. It must not cancel unrelated sibling work.
- A daemon restart must not rerun a completed producer or verifier merely because an in-memory callback was lost. Reconciliation reads durable terminal state and advances at most once.
- Concurrent owner decisions use a revision/compare-and-set or equivalent first-valid-transition rule. Stale `repair`/`accept` decisions cannot both win.
- The convergence coordinator must not hold scheduler permits while waiting for children. Waiting is durable coordination, not a process slot.
- Resource contention is handled by the existing scheduler. A convergence request cannot reserve a private pool of workers outside fairness/admission.

## 13. Verification posture

Keep verification focused and proportional. Each implementation plan defines its narrow commands. The roadmap-level minimum after a coherent milestone is:

```bash
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
```

Focused tests must cover state transitions, idempotency, restart reconciliation, owner authorization, verifier permission ceiling, evidence bounds, cancellation races, result-commit chaining, and the host-goal-verifier boundary as applicable.

Do not add a new CI workflow, live-provider dependency, benchmark gate, or large multi-model evaluation suite as a closure prerequisite. Model-quality evaluation can inform later profile defaults without becoming an implementation gate for the core authority contract.

## 14. Static guards and documentation

Implementation should update:

- `architecture/agent.md` with convergence ownership and lifecycle;
- `architecture/tool.md` if the task tool gains convergence actions;
- `architecture/worktree.md` when M003 adds explicit repair-base worktree construction;
- `architecture/model-adapters.md` if M003 adds orchestration capability metadata;
- `architecture/goal.md` to state that semantic convergence remains advisory to host completion verification;
- `architecture/overview.md` index when a dedicated convergence architecture document is added.

Static/review guards should ensure:

- convergence production code does not construct its own scheduler or raw `AgentLoop` execution path;
- verifier built-in permissions cannot include mutating/shell/Git-write actions;
- model-facing verdicts cannot call a direct goal-complete transition;
- no new use of the legacy `.opencode/team` state is introduced.

Prefer extending existing ownership guards/tests rather than creating another standalone verification framework.

## 15. Risks and deferred work

Risks:

- token/time amplification if convergence is used indiscriminately;
- false confidence from a verifier that shares the producer's assumptions or insufficient evidence;
- repair-chain complexity if Git base/result provenance is weak;
- user confusion if semantic `Pass` is presented as stronger than deterministic host verification;
- model-role instability on small or tool-fragile models.

Mitigations are explicit role/context isolation, structured evidence, fixed bounds, conservative defaults, and host-owned final authority.

Deferred beyond M003:

- domain-specific verifier fleets or consensus/debate among several verifiers;
- automatic profile promotion based on telemetry;
- cross-daemon/distributed convergence;
- arbitrary workflow graphs;
- self-modifying verifier prompts/agents;
- automatic merge after semantic pass;
- replacing deterministic goal criteria with LLM adjudication.

## 16. User-visible roadmap exit

This roadmap may close when a user can ask CodeGG to independently verify a substantial delegated implementation, observe a bounded producer/verifier cycle, receive actionable independent findings, optionally run a bounded repair cycle on explicit Git provenance, and retain final control over acceptance/integration while all execution remains scheduler-owned and goal completion remains host-verified.
