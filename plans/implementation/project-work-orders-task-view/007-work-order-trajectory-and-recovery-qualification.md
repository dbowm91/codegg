# Project Work Orders and Task View M007 — End-to-End Trajectory, Recovery, Contention, and Security Qualification

Status: implemented (original closure: `plans/closure/project-work-orders-task-view/007-status.md`; final UX-fidelity regression closure: `plans/closure/project-work-orders-task-view-corrective/001-status.md`)

Repository baseline: `3ed785618bfac5a85f504813bdb7fc3a923e8679`

Source roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Applicable ADRs:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: invariant / capability closure

Hard dependency: M001-M006 closure.

## Post-closure correction note

M007 remains valid point-in-time qualification of the durable WorkOrder architecture and composed backend behavior. A later product-level review found three human-surface fidelity gaps that M007's milestone-local qualification did not reject: bare-Tab composer selection, direct focused scheduling-queue reorder, and human external-trigger setup/recovery. Corrective C001 fixed those gaps and reran the relevant trigger, trajectory, TUI routing, authorization, ownership, formatting, Clippy, and quick-verification suites. Current final capability evidence is therefore M007 plus the closed C001 corrective, not M007 alone.

## 1. Objective

Qualify the complete project WorkOrder/task-view capability under restart, duplicate delivery, concurrent team mutation, policy/model drift, worktree conflict, cancellation, permission wait, sequential failure, external-trigger replay, and agent-created batch trajectories.

This is a closure-oriented milestone. It may add narrow corrective production changes discovered by qualification, but it must not redesign the WorkOrder domain, create a second scheduler, or broaden scope into a general workflow engine.

## 2. Why this milestone exists

The WorkOrder feature spans several existing authoritative subsystems:

- project identity and authorization;
- durable WorkOrder/occurrence/lane/trigger state;
- scheduler admission and job attempts;
- canonical session lifecycle;
- model/provider/runtime-preference resolution;
- approval and sandbox policy;
- managed worktree lifecycle;
- TUI project/global projections;
- external trigger authentication;
- model-visible WorkOrder creation.

Each earlier milestone can prove its local contract, but the feature is only trustworthy if the composed system preserves exactly-once materialization, authority ceilings, sequence semantics, isolation truthfulness, and recoverability across process failure and concurrent actors.

## 3. Current implementation evidence expected before start

Do not begin until closure records for M001-M006 exist and the registry marks this milestone dependency-ready.

At start, inspect the actual landed implementations and closure evidence rather than assuming the implementation plans were followed literally. Record any material deviation in this plan's closure record.

Expected substrate:

- M001: durable WorkOrder/Occurrence/SequenceLane domain, CAS mutation, project-scoped protocol/authorization, bounded projections;
- M002: WorkOrderCoordinator, release gates, repeat semantics, exactly-once claim/materialization, managed worktree/session/AgentTurn integration;
- M003: Task composer/scheduling sheet/project Task view and human task-model preference;
- M004: global Workspace dashboard/project activity projection;
- M005: narrow external trigger endpoint, secret verifier lifecycle, idempotency/replay protection;
- M006: dedicated model-visible WorkOrder tool and atomic bounded batch creation.

## 4. Invariants that must be proven

- Exactly one durable scheduler/admission owner remains in production.
- One WorkOrder occurrence materializes at most one canonical session and one initial AgentTurn submission despite retries/restarts/duplicate wakes.
- Waiting WorkOrders are never represented by fake Session rows.
- Once materialized, ordinary Session/Turn/AgentRun behavior owns interaction, steering, permission/question response, cancellation, observation, and history.
- Sequence-lane ordering is CAS-safe; stale concurrent reorder/update cannot overwrite newer state.
- Sequence failure/needs-attention holds downstream work by default.
- Gate satisfaction is latched per occurrence and duplicate time/trigger/reconcile signals cannot double-release.
- Finite repeat creates distinct occurrences/sessions/jobs and stops exactly at its configured bound.
- Requested model/approval/sandbox/workspace policy cannot widen at execution; stale/removed model identity produces explicit attention rather than silent substitution.
- Git mutation-capable parallel task sessions receive distinct managed worktrees under default isolation.
- Non-Git shared execution is never labeled isolated.
- External trigger tokens cannot authorize project/session operations beyond satisfying their one gate.
- Agent-created WorkOrders cannot exceed parent/project authority or fan out recursively without hard bounds.
- Global/project projections do not leak unauthorized project/task/session existence.
- Restart reconciliation never replays an already committed mutating side effect merely to repair metadata.

## 5. Scope

### In scope

- deterministic fault-injection around every WorkOrder lifecycle boundary;
- SQLite close/reopen and daemon-generation restart tests;
- concurrent principal/client reorder/edit/cancel/trigger races;
- duplicate scheduler wake/claim/materialization attempts;
- removed/unavailable selected model and provider lifecycle changes;
- approval/sandbox policy narrowing between creation and execution;
- permission/question waiting and normal-session steering after materialization;
- managed-worktree dirty/conflicted/orphan attention states;
- sequence predecessor success/failure/cancel/attention behavior;
- repeat count exhaustion and inter-occurrence delay semantics;
- external-trigger duplicate/replay/revoke/expiry/max-fire races;
- agent WorkOrder batch idempotency/fan-out/depth limits;
- project/global TUI projection boundedness and stale-completion routing;
- representative 15-plan sequential queue trajectory;
- only narrow corrective code/docs/tests needed to close discovered defects.

### Explicitly out of scope

- new release-gate kinds;
- cron/general recurrence language;
- automatic merge/integration of completed worktrees;
- cross-node/distributed execution redesign;
- non-Git copy/overlay isolation backend;
- generic webhook platform;
- project-management/Kanban features;
- broad TUI redesign unrelated to WorkOrder correctness.

## 6. Qualification matrix

### A — Creation and default immediate behavior

Prove:

1. human Task composer with only default zero-delay/immediate gate creates one WorkOrder;
2. coordinator claims it exactly once;
3. one canonical session is created;
4. one initial AgentTurn enters the existing scheduler;
5. the project Task row transitions from future/starting to running session identity without creating a second interaction model;
6. cancellation/steering thereafter routes through normal session controls.

Inject process failure after each durable step and verify restart converges without duplicate session/job creation.

### B — Sequential queue behavior

Build a lane with at least 15 WorkOrders. Verify:

- stable order/revision after restart;
- `j/k`/API reorder of future rows only;
- running/claimed predecessor pinned;
- stale revision conflict has zero mutation;
- predecessor success releases only the next eligible item;
- predecessor failure, cancellation, permission attention, model-unavailable attention, and worktree conflict hold downstream items by default;
- explicit authorized retry/skip/cancel/advance follows documented policy;
- no mass release occurs from one terminal transition.

### C — Release gates and boolean join

For Immediate/Delay/NotBefore/SequenceReady/ExternalTrigger:

- each gate independently releases at its defined boundary;
- `All` waits for all enabled gates;
- `Any` releases on first satisfied gate;
- satisfaction latches once per occurrence;
- duplicate timer/reconcile/trigger events are harmless;
- timezone/timestamp serialization is explicit and stable;
- restart before/after satisfaction preserves the correct state;
- invalid or impossible configurations fail validation rather than spin.

### D — Finite repeat

Verify exact counts for 1, 2, maximum allowed, and invalid over-bound repeats. Later occurrences must have distinct occurrence/session/job identities. Test restart between occurrences, delay anchoring, predecessor attention, cancellation of remaining repeats, and no accidental unbounded recurrence.

### E — Exactly-once materialization fault injection

Inject failure at minimum:

1. before occurrence claim commit;
2. after claim but before worktree allocation;
3. after worktree reservation but before ready binding;
4. after session create but before occurrence stores session ID;
5. after occurrence stores session ID but before AgentTurn job submit;
6. after job creation but before occurrence stores job ID;
7. after job submit acknowledgement but before coordinator completion marker;
8. during daemon shutdown/restart while running.

Recovery must use durable identities/idempotency keys to converge forward or mark explicit attention. It must never create a second canonical session or initial job because metadata linkage was incomplete.

### F — Workspace/worktree isolation

For one Git project, start at least three mutation-capable WorkOrders concurrently. Assert distinct managed worktree/workspace identity and paths, independent leases, correct base provenance, and no shared write root.

Exercise:

- one clean completion;
- one dirty retained worktree;
- one conflicted/attention worktree;
- cancellation during preparation;
- restart with active lease;
- cleanup/archive after terminal state according to existing worktree rules.

For non-Git mutation work, assert serialized/shared classification or explicit unsafe-shared opt-in and truthful UI/projection labeling.

### G — Model/provider and policy drift

Create waiting WorkOrders, then before execution:

- remove selected model from catalog;
- disable/tombstone provider connection;
- narrow project approval policy;
- narrow sandbox profile/available filesystem authority;
- revoke creating principal's relevant project capability where semantics require reauthorization.

Expected behavior must match M002/M001 policy: no silent model/provider substitution, no authority widening from stale snapshots, explicit attention/deny states, and immutable original attribution.

### H — Permission/question and steering lifecycle

For a materialized running WorkOrder session:

- reach interactive permission request;
- verify project/global dashboard attention signal;
- answer from an authorized controller and deny from unauthorized observer;
- steer the active turn through ordinary session steering;
- cancel the session/task and verify job/process/agent descendants follow existing cancellation rules;
- ensure downstream sequential work does not release while predecessor remains unresolved/attention unless explicit policy says otherwise.

### I — External trigger security and replay

Test:

- valid POST fires only the intended gate;
- GET is side-effect free/rejected;
- query-string secret is not accepted if prohibited by M005;
- stored secret is verifier/hash only;
- wrong secret and wrong public trigger ID reveal no sensitive project detail;
- duplicate POST and duplicate `Idempotency-Key` return stable semantics;
- concurrent distinct idempotency keys cannot double-release one latched occurrence;
- expired/revoked/max-fire triggers fail safely;
- trigger secret cannot call ordinary project/session/job APIs;
- logs/audit never contain the bearer secret.

### J — Agent WorkOrder tool

Use one normal agent to:

1. list/read a synthetic ordered set of plan files using ordinary file tools;
2. submit one bounded atomic WorkOrder batch;
3. request first immediate release and remaining sequential release;
4. retry the exact model tool invocation and prove no duplicate batch;
5. attempt invalid item N and prove no partial commit;
6. attempt cross-project/broader-yolo/full-host/model authority and prove rejection;
7. attempt repeated nested WorkOrder creation until host bounds stop it;
8. restart and prove durable fan-out/depth counters still apply.

Existing delegated `TaskTool` behavior must remain unchanged.

### K — Team contention and privacy

With at least two principals/clients:

- concurrent reorder on the same lane produces one success + one revision conflict or deterministic equivalent;
- edit vs claim race cannot mutate an already claimed immutable execution payload;
- cancel vs claim/completion resolves to one documented durable outcome;
- authorized viewers see only allowed project/task summaries;
- unauthorized project/task lookups use the existing not-found/privacy convention;
- global Workspace dashboard counts exclude unauthorized projects;
- observer cannot respond to another session's permission or steer it without explicit control authority.

### L — TUI stale-completion and navigation

Exercise rapid project/dashboard/task/session switching while asynchronous snapshots are in flight. Existing epoch/request-generation guards must prevent late WorkOrder/project results from mutating the newly active project/session. Vim navigation and Task composer state must remain project-correct.

## 7. Representative long trajectory

Create a realistic fixture with 15 medium WorkOrders representing sequential implementation plans. The trajectory should include:

- first task immediate;
- at least one delayed task;
- at least one task gated by NotBefore + sequence;
- one external-trigger-gated task;
- one repeated task;
- two parallel independent tasks where policy permits;
- one permission wait resolved by user;
- one intentional failure/attention requiring retry or explicit skip;
- one daemon restart while a task is running;
- one restart while several future tasks remain;
- one agent-created batch segment.

Closure evidence should show that all intended tasks eventually reach the expected terminal/attention state in the right order, no occurrence duplicates, and each materialized session can be independently inspected/steered as a normal session.

## 8. Observability and audit review

Confirm bounded events/metrics make it possible to diagnose:

- WorkOrder created/edited/reordered/cancelled;
- occurrence gate state and ready/claim transitions;
- materialization session/job/worktree identity;
- sequence hold reason;
- model/provider/policy attention reason;
- external trigger fire/reject/replay without secret leakage;
- agent-created WorkOrder lineage;
- recovery reconciliation decisions.

Do not add high-cardinality prompt content or secret values to metric labels. Structural IDs belong in trace/audit records according to existing bounded policy.

## 9. Static ownership and duplication audit

Re-run and, if useful, extend static guards proving:

- no second scheduler/background loop was added;
- WorkOrder execution reaches `JobSubmissionService` rather than direct agent execution;
- WorkOrder persistence has one core owner;
- no raw `std::env::current_dir()`/path-derived project identity entered the new domain;
- existing delegated TaskTool remains semantically distinct;
- external trigger endpoint cannot call general privileged handlers using trigger identity;
- authorization matrix covers all new protocol operations;
- project/global projections are read-only adapters.

Prefer extending existing guards over adding many one-off scripts.

## 10. Corrective-change policy

If qualification finds defects:

- fix small local defects in this milestone with focused regression tests;
- if a defect invalidates an ADR/domain ownership decision, stop and create a new ADR/corrective plan rather than silently redesigning;
- if an earlier milestone is materially incomplete, produce a corrective plan/closure status referencing that milestone instead of masking it inside M007;
- do not add unrelated cleanup or feature polish merely because affected files are open.

## 11. Required verification commands

Use exact current target names discovered at implementation time. Minimum expected set:

```bash
cargo test -p codegg-core -- work_order
cargo test -p codegg --lib -- work_order
cargo test --test authorization -- work_order
cargo test --test worktree -- work_order
cargo test --test tui -- work_order
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test -p codegg-protocol
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Also run the focused fault/restart/trigger/agent-batch integration targets introduced by M001-M006. Record the exact commands and test counts in closure.

Do not introduce an oversized verification framework. Reuse deterministic in-process/SQLite/temp-repository fixtures and the repo's existing quick verification convention.

## 12. Documentation reconciliation

Audit/update at least:

- `architecture/jobs.md`;
- `architecture/scheduler.md`;
- new WorkOrder architecture doc from M001/M002;
- `architecture/session.md`;
- `architecture/worktree.md`;
- `architecture/authorization.md`;
- `architecture/tui.md`;
- protocol docs;
- README/user-facing task/workspace commands if they landed;
- planning registry/roadmap statuses and closure links.

Historical closure records remain immutable except for explicit errata conventions.

## 13. Acceptance criteria

- M001-M006 have accepted closure evidence or any remaining defect is explicitly classified and planned.
- Exactly-once occurrence materialization survives all tested crash windows without duplicate canonical sessions/jobs.
- Sequence order/reorder/failure-hold semantics remain correct across restart and concurrent principals.
- Gate/repeat/trigger semantics are deterministic, finite, and replay-safe.
- Parallel Git mutation tasks use distinct managed worktrees; non-Git isolation is described truthfully.
- Model/provider/policy drift cannot silently substitute or widen authority.
- Permission/question/steering/cancellation use ordinary session/runtime mechanisms after materialization.
- External trigger capabilities remain narrow, verifier-only at rest, idempotent, revocable, and secret-clean in logs/audit.
- Agent batch creation is atomic/idempotent/bounded and cannot recursively self-replicate beyond policy.
- Global/project TUI projections are bounded, project-correct, privacy-preserving, and resistant to stale async completion.
- Representative 15-plan trajectory completes/holds exactly as designed through injected failures and restart.
- No second scheduler, task runtime, session runtime, or worktree lifecycle owner exists.
- No unresolved medium-or-higher correctness/security finding remains; lower findings are explicitly documented.

## 14. Stop conditions

Stop and create a corrective architecture plan if qualification shows that closing the feature requires:

- a second scheduler/admission loop;
- treating WorkOrder as Session/Job/AgentTask/WorkPlan identity;
- disabling CAS or idempotency to resolve races;
- storing trigger secrets reversibly/plaintext;
- bypassing daemon authorization for team usability;
- silent model/provider fallback contrary to ADR-0005;
- sharing Git mutation workspaces while labeling them isolated;
- unbounded repeat or agent WorkOrder recursion;
- broad distributed-execution redesign.

## 15. Closure evidence required

The closure record must include:

- implementation/corrective commits;
- M001-M006 closure links;
- lifecycle/fault-window requirement-to-evidence matrix;
- exactly-once materialization evidence with durable identity correlations;
- sequence/reorder/contention matrix;
- release-gate/repeat matrix;
- worktree isolation and non-Git truthfulness evidence;
- model/provider/policy-drift matrix;
- permission/steering/cancel trajectory evidence;
- external trigger security/replay/secret-leak matrix;
- agent batch/fan-out/idempotency matrix;
- team privacy/authorization matrix;
- TUI stale-completion/navigation evidence;
- representative 15-plan trajectory log/summary;
- exact verification commands/results;
- unresolved findings with severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.
