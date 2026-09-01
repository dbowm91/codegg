# Agent Run, Async Delegation, and Worktree Concurrency M007 — Durable Lineage, Owner Context, Fan-Out, and Authorization Corrective Pass

Status: ready

Repository baseline: `b87d1d5b65aca96c700deb27e579374b3d158545`

Source corrective roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-corrective-closure-addendum.md#m007--durable-lineage-owner-context-fan-out-and-authorization-corrective-pass`

Historical source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

Superseded strict subsystem closure disposition:

- `plans/closure/agent-run-worktree-concurrency/006-status.md`

Historical predecessor closure records retained:

- `plans/closure/agent-run-worktree-concurrency/001-status.md`
- `plans/closure/agent-run-worktree-concurrency/002-status.md`
- `plans/closure/agent-run-worktree-concurrency/003-status.md`
- `plans/closure/agent-run-worktree-concurrency/004-status.md`
- `plans/closure/agent-run-worktree-concurrency/005-status.md`
- `plans/closure/agent-run-worktree-concurrency/006-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent task, agent run, worktree, budget, execution context
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: invariant / correctness / capability

Hard dependencies: historical M001–M006 implementations remain present and ADR-0002 remains accepted.

## 1. Objective

Repair the production integration boundary between root turn orchestration, durable delegated runs, scheduler execution, nested TaskTool construction, run groups, control authorization, depth budgets, and worktree isolation.

After this milestone:

- the primary/root agent can use `spawn_many` without being represented as a fake delegated `AgentRun`;
- a durable child that delegates descendants is unambiguously the owner of those descendants;
- parent/root/depth/project/repository/workspace/turn context survives every scheduler/worker hop;
- nested mutating descendants can receive correct isolated worktrees;
- max-depth is enforced from durable state rather than a reset worker counter;
- control authority matches ADR-0002 instead of relying on same-session reachability;
- nested and root run-group membership validates against explicit owner scope.

This milestone does not own tool-call idempotency or projection-depth finalization; those are M008 so closure verification cannot be conflated with the primary identity repair.

## 2. Why the previous verification missed this

M001–M006 had strong unit/focused coverage of their individual stores and services, but the tests did not sufficiently exercise the exact production composition from a normal root TaskTool through scheduler-owned child execution and then back into a nested child TaskTool.

The missed boundary is specifically:

```text
root TurnRuntime
  -> session TaskTool
  -> JobPayload::SubagentRun
  -> SubagentJobExecutor
  -> SubAgentRequest
  -> child AgentLoop
  -> nested TaskTool
  -> descendant durable submission/group/control
```

Individual components accepted valid fixtures, but production adapters supplied incomplete or ambiguous ownership metadata.

M007 must add tests at this full seam rather than only testing stores/services in isolation.

## 3. Current implementation evidence to reconfirm before editing

The implementation agent MUST inspect the current head and confirm or update these findings before changing code.

### 3.1 Root TaskTool has services but no orchestration owner

`src/agent/turn_runtime.rs` passes the root session/turn, run store, control service, group service, project ID, and repository ID into `build_session_tool_registry`.

`src/tool/factory.rs` configures the normal TaskTool with:

- scheduler submission;
- durable run store;
- run-control service;
- run-group service;
- project/repository/turn context.

It does not configure a current/owner run ID because the root turn is not a delegated run.

`src/tool/task.rs` currently gates group actions on `parent_run_id`, causing root `spawn_many`/group operations to reject with “group actions require a durable parent run”.

### 3.2 Scheduler child request loses current-owner semantics

`src/scheduler/executors.rs` receives durable `run_id` from `JobPayload::SubagentRun` and uses it for lifecycle/worktree/result persistence, but the `SubAgentRequest` construction currently sets:

- `run_id` to the current durable child;
- `parent_run_id` to `None`;
- `depth` to `1`.

This is incompatible with multilevel durable execution.

### 3.3 Child TaskTool is built from the wrong run field

`src/agent/worker.rs` builds a child TaskTool with `.with_parent_run_id(request.parent_run_id.clone())`.

For descendant spawning, the correct owner is the currently executing child run (`request.run_id`), not that current run’s own parent.

### 3.4 Nested TaskTool context is incomplete

The child TaskTool path configures model/depth/workspace/allowed paths and, when available, durable submission/store/control. It does not currently carry the same complete project/repository/turn/group context installed on root tools.

### 3.5 Depth is not durable authority

`AgentRunBudget.max_depth` exists, but `AgentRunRecord` does not currently make actual depth authoritative and scheduler-owned child requests reset depth.

### 3.6 Control authorization is too broad and directionally incorrect

`RunControlService::authorize` currently:

1. authorizes on matching session ID alone; then
2. when using run lineage, walks upward from the actor and accepts when the actor’s current record names the target as parent.

That second case authorizes child→parent rather than parent→child. The session shortcut also permits same-session unrelated/sibling control.

### 3.7 Group store assumes every owner is an AgentRun

`AgentRunGroupRecord` currently requires `root_run_id` and `owner_run_id`, and `AgentRunGroupService::create` requires every member to have `parent_run_id == owner_run_id`.

That works for a nested run-owned group but cannot represent a legitimate group owned directly by a root turn.

## 4. Invariants that MUST NOT regress

- `AgentTask` remains a delegated unit of intent. Do not create a synthetic delegated task merely to give the root turn an ID.
- Durable `AgentRun` remains the execution of a delegated task under ADR-0002.
- The global scheduler remains the only daemon machine-resource admission authority.
- A child’s effective authority never exceeds its parent/turn authority.
- Current run identity, parent run identity, and root run identity are distinct concepts and MUST NOT share one ambiguous field.
- Root turn ownership is scoped to one session + one turn, not the entire session.
- Run-owner control is limited to direct children and owned groups under the initial ADR-0002 contract.
- Same-session siblings cannot control each other merely because their tasks share a session.
- Child→parent control is denied.
- Descendant depth is monotonic and bounded before scheduler execution.
- Nested mutation-capable runs receive isolated worktrees; no shared-write fallback is allowed.
- Worktree allocation uses canonical repository identity and a defined base commit/ref; it does not infer repository ownership from arbitrary display paths.
- Parent integration remains explicit and typed.
- Existing legacy numeric TaskStore compatibility remains readable but is not execution authority.

## 5. Required production design

### 5.1 Introduce explicit orchestration ownership

Replace the overloaded meaning of `parent_run_id` in TaskTool with a typed owner concept.

Representative shape:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
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

Use repository naming conventions if a better existing type name exists.

Rules:

- root/session TaskTool: `Turn { session_id, turn_id }`;
- child TaskTool: `Run { run_id: current_run_id }`;
- `parent_run_id` on `AgentRunRecord` continues to describe lineage of the current run;
- root turn must not be inserted into `agent_task`/`agent_run` solely for orchestration ownership.

The TaskTool should expose owner-specific helpers rather than accepting a loosely related collection of optional fields that can form invalid combinations.

### 5.2 Define one durable descendant execution context

Create or formalize one typed context propagated from durable run acceptance through the scheduler executor into child runtime construction.

Representative fields:

```rust
pub struct DelegatedAgentExecutionContext {
    pub current_run_id: AgentRunId,
    pub root_run_id: AgentRunId,
    pub parent_run_id: Option<AgentRunId>,
    pub depth: u32,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub project_id: ProjectId,
    pub repository_id: Option<RepositoryId>,
    pub workspace_id: WorkspaceId,
    pub worktree_id: Option<WorktreeId>,
}
```

Add only additional bounded authority/budget fields that are genuinely required by descendant admission. Do not serialize secrets or entire permission bodies.

The canonical source should be durable task/run/worktree records, not `parent_id` display strings.

### 5.3 Persist authoritative run depth

Add an additive migration and storage field for actual run depth, unless an already-authoritative equivalent exists by implementation time.

Rules:

- top-level delegated run accepted by a root turn: depth `1`;
- descendant of run depth `N`: depth `N + 1`;
- parent/root/depth relation is validated transactionally with task/run creation;
- root run ID for a descendant comes from the parent’s `root_run_id`;
- `AgentRunBudget.max_depth` or the resolved delegation limit is checked before creating/submitting a child;
- descendant request uses persisted depth; scheduler executor MUST NOT hard-code `1`;
- semantic `SubAgentPool` depth checks consume the durable depth for scheduler-owned requests.

If both configuration max depth and durable root budget exist, use the narrower limit and document precedence.

### 5.4 Root-turn and run-owned group support

Evolve the group owner contract to represent both valid owner kinds without weakening validation.

Suggested domain shape:

```rust
pub enum AgentRunGroupOwner {
    Turn {
        session_id: String,
        turn_id: String,
    },
    Run {
        run_id: AgentRunId,
    },
}
```

Storage may use `owner_kind` plus nullable owner columns or another normalized representation consistent with existing SQLite conventions.

Validation rules:

#### Turn-owned group

- all members are durable runs;
- each member task has the same originating `session_id` and `turn_id` as owner;
- each member has `parent_run_id == None`;
- each member was accepted from that root TaskTool invocation/fan-out path;
- group cannot absorb arbitrary historical top-level runs from the same session unless explicitly passed and authorized under the same turn owner.

#### Run-owned group

- owner run exists and is nonterminal when creating new child members unless detached semantics explicitly permit otherwise;
- every member is a direct child: `member.parent_run_id == Some(owner_run_id)`;
- every member shares owner `root_run_id`;
- depth is exactly owner depth + 1 for newly spawned members.

Keep `RunJoinPolicy`, result aggregation, restart recomputation, and scheduler admission behavior from M005.

### 5.5 Root `spawn_many`

Root TaskTool must be able to perform `spawn_many` using turn ownership.

The path should:

1. receive one accepted root tool invocation;
2. derive bounded per-member descendant identities;
3. submit each child through canonical durable TaskTool/scheduler acceptance;
4. collect only successfully accepted durable run IDs;
5. persist a turn-owned group over those runs;
6. report rejected members explicitly;
7. never require a fake root run.

M008 will finalize canonical model tool-call idempotency. M007 may use explicit test keys or temporary existing keys but MUST leave a clean call-identity seam.

### 5.6 Nested TaskTool construction

When executing durable run `R`, configure its TaskTool with:

- orchestration owner `Run(R)`;
- durable run store;
- run-control service;
- run-group service;
- scheduler submission service;
- project/repository/workspace/turn context loaded from durable records;
- current worktree/workspace root and allowed-path ceiling;
- authoritative current depth and max-depth/root budget;
- parent model and existing resolved child-authority restrictions.

Do not use `request.parent_run_id` as the TaskTool owner.

### 5.7 Parent/root context propagation in scheduler payloads

Prefer carrying stable IDs in `JobPayload::SubagentRun` and loading richer context from the authoritative store inside the executor. Do not duplicate mutable provenance in payload JSON unless needed for restart execution.

At minimum the executor must be able to reconstruct:

- current run/task;
- parent/root/depth;
- originating session/turn;
- project/repository/workspace;
- current worktree if already attached;
- authority/budget data required by worker construction.

Any duplicated payload field must be checked against the store and fail on mismatch rather than silently selecting one source.

### 5.8 Correct control authorization

Replace session-wide short-circuit authorization with explicit owner semantics.

Representative API:

```rust
pub enum ControlActor {
    Turn { session_id: String, turn_id: String },
    Run { run_id: AgentRunId },
}
```

or equivalent typed fields that cannot be forged into contradictory combinations.

Rules:

- `Turn(S,T)` may target a top-level run whose task originated from exactly `(S,T)` and whose `parent_run_id` is `None`;
- `Run(P)` may target run `C` only when `C.parent_run_id == Some(P)`;
- a run cannot target itself through parent control;
- child→parent is denied;
- sibling→sibling is denied;
- same session but different turn is denied;
- arbitrary session ID without the current root-turn context is not authorization;
- group operations validate the actor against the persisted group owner;
- cancellation cascade below an authorized direct child continues through existing scheduler/run subtree semantics.

Add negative tests first so the current behavior fails before the authorization implementation changes.

### 5.9 Nested worktree allocation

For a nested mutation-capable descendant:

- retain the canonical `RepositoryId` from durable task context;
- identify the canonical repository/worktree Git common-dir/root through existing Git/worktree services rather than treating an arbitrary nested checkout path as a new repository identity;
- base the descendant worktree on the owning parent run’s effective current commit/HEAD when semantics require “continue from parent child work”; otherwise use the explicitly selected durable base;
- persist the selected base commit in the new worktree record;
- allocate a distinct worktree/index before child AgentLoop construction;
- narrow child filesystem/Git authority to the new worktree;
- keep push/history rewrite/parent integration separate.

If the existing `WorktreeService::CreateWorktreeRequest` cannot express the required canonical repository root + explicit base commit/ref, extend it additively rather than path-inferencing around the limitation.

## 6. Storage and migration

Expected additive migration work may include:

- authoritative `agent_run.depth`;
- run-group owner kind and root-turn owner fields;
- optional constraints/indexes needed for owner lookup and restart reconstruction.

Migration requirements:

- preserve existing M001–M006 rows;
- existing run-owned groups remain readable and map to `Run` owner;
- do not fabricate turn ownership for historical groups where it cannot be proven;
- schema upgrade is idempotent;
- old rows default conservatively and projection/compatibility code must handle missing historical metadata explicitly;
- no destructive rewrite of legacy numeric `task` data.

If storage can represent the new owner contract without a migration through an existing normalized relation, document and test that choice instead.

## 7. Protocol and compatibility effects

Prefer internal additive changes first.

- Existing single `task spawn` schema remains valid.
- Existing `run_id` handles remain stable.
- Existing `spawn_many` action remains model-facing; this milestone makes it reachable from root turns.
- Existing run-owned groups remain supported.
- If projection DTOs need additive owner-kind fields for debugging/TUI, defer final projection normalization to M008 unless required to keep code compiling.
- Legacy numeric `get` remains compatibility-only.
- Do not remove old subagent lifecycle events in this corrective milestone.

## 8. Ordered work packages

### A — Failing production-path fixtures

Before production changes, add tests that demonstrate:

1. root TaskTool `spawn_many` currently fails for lack of owner run;
2. durable child → grandchild loses parent/root/depth under the current scheduler request path;
3. nested mutating child loses repository context or fails worktree preparation;
4. same-session sibling control is currently accepted;
5. child→parent control is currently accepted by the lineage path or otherwise not rejected by the intended rule;
6. max depth can be bypassed by scheduler hop depth reset.

Tests may be introduced under focused integration modules rather than one enormous fixture file.

### B — Orchestration owner and durable context types

- introduce typed root-turn/run owner;
- introduce/normalize delegated execution context;
- replace ambiguous TaskTool `parent_run_id` ownership usage;
- add constructors/builders that make invalid combinations difficult;
- keep compatibility helpers only where needed by existing standalone tests.

### C — Durable depth and relation enforcement

- migrate/store depth;
- compute depth/root from parent transactionally;
- enforce max depth before acceptance;
- propagate depth through scheduler executor/worker;
- reconcile or conservatively mark historical missing-depth rows.

### D — Root and nested TaskTool wiring

- root tool receives `Turn(session, turn)` owner;
- child tool receives `Run(current_run)` owner;
- nested project/repository/turn/group/store/control/submission context is complete;
- remove any dependency on display `parent_id` for durable lineage decisions.

### E — Group owner migration and fan-out reachability

- update group domain/store/service owner contract;
- make root `spawn_many` create a turn-owned group;
- preserve nested run-owned groups;
- retain deterministic joins and restart recomputation;
- reject owner/member mismatches.

### F — Control authorization correction

- implement exact direct-owner rules;
- remove same-session authorization shortcut;
- correct/invert current ancestry behavior rather than layering additional checks around it;
- ensure terminal status/wait read access has the intended scope separately from mutating control if repository conventions distinguish observation from control.

### G — Nested worktree correctness

- propagate repository identity;
- resolve canonical repository root/common dir;
- select explicit nested base commit;
- allocate distinct worktree;
- retain dirty/conflicted state on failure/cancel.

### H — Documentation and handoff state

Update at least:

- `architecture/agent.md`;
- `architecture/scheduler.md`;
- `architecture/worktree.md`;
- `architecture/git.md` if nested base/integration behavior changes;
- task-tool/model guidance if group owner semantics are visible;
- corrective roadmap/registry/plan status when implementation lands.

Do not mark subsystem closed in M007. M008 owns strict closure.

## 9. Required tests

### Core/store

- top-level run depth = 1;
- child depth = parent + 1 and root propagates;
- inconsistent parent/root/depth is rejected;
- max-depth boundary accepts last permitted level and rejects next;
- SQLite and in-memory implementations agree;
- old rows/migrations remain readable.

### Control authorization

- root turn controls its top-level child;
- same session, different turn denied;
- sibling denied;
- child controlling parent denied;
- parent run controls direct child;
- unrelated root denied;
- group owner may cancel owned group only;
- forged/mismatched owner fields denied.

### Run groups

- root `spawn_many` accepts 2–3 top-level children and creates turn-owned group;
- nested child `spawn_many` creates a run-owned group of direct descendants;
- run-owned group rejects sibling/historical unrelated member;
- turn-owned group rejects another turn’s top-level member;
- restart reload preserves owner kind/membership/join state.

### Scheduler/worker

- durable executor passes authoritative current/root/parent/depth to child runtime;
- scheduled path still skips pool machine-capacity semaphore;
- semantic descendant limits remain active;
- cancellation remains bounded and releases ownership exactly once.

### Worktree

- two top-level mutating children get distinct worktrees;
- nested mutating grandchild gets a third distinct worktree;
- nested base commit is the expected parent-effective commit;
- repository identity remains the same logical `RepositoryId` across all worktrees;
- path authority is narrowed to descendant worktree;
- dirty/conflicted nested worktree survives failure/cancel.

### Production-shaped end to end

Use deterministic/mock providers where needed:

```text
root turn
  -> spawn_many(A, B)
  -> group created and parent continues
A (durable run depth 1)
  -> spawn C (durable depth 2)
  -> C mutates in isolated worktree
root waits group
  -> A/B terminal aggregation
```

Also exercise a child-owned group with two grandchildren.

## 10. Verification commands

Implementation agent should adapt exact test names to current repository layout but keep scope bounded.

Expected focused commands include equivalents of:

```text
cargo test -p codegg-core agent_run --locked -- --test-threads=1
cargo test -p codegg-core agent_run_group --locked -- --test-threads=1
cargo test --lib agent::run_control --locked -- --test-threads=1
cargo test --lib agent::worker --locked -- --test-threads=1
cargo test --lib scheduler --locked -- --test-threads=1
cargo test --test subagent --locked -- --test-threads=1
cargo test --test scheduler_cancellation --locked -- --test-threads=1
cargo test --test scheduler_contention --locked -- --test-threads=1
cargo test --test scheduler_restart_recovery --locked -- --test-threads=1
cargo test --test worktree --locked -- --test-threads=1
```

Run the repository’s existing static ownership/boundary guards relevant to the touched code, including scheduler bypass, execution ownership, core boundary, daemon cwd/path identity, Git forbidden patterns, and projection disclosure if projection code is touched incidentally.

M007 does not require adding a new CI workflow or running a larger verification matrix than the repository already uses.

## 11. Acceptance criteria

M007 may be recommended closed only when all are true:

1. Root TaskTool can `spawn_many` through a legitimate turn-owned context.
2. No synthetic root delegated task/run is introduced.
3. Durable child→grandchild records persist correct `parent_run_id`, `root_run_id`, and depth.
4. Scheduler/worker path never resets durable depth to a constant.
5. Durable max-depth is enforced before descendant execution.
6. Nested TaskTool receives complete project/repository/workspace/turn/store/control/group/submission context.
7. Nested mutating descendant receives correct isolated worktree and base commit.
8. Root turn and run-owned groups validate membership against explicit owner scope.
9. Same-session sibling control is denied.
10. Child→parent control is denied.
11. Parent/run owner direct-child control works.
12. Cancellation and join semantics from M001/M002/M005 remain correct.
13. Scheduler remains sole machine-capacity admission authority.
14. Historical M001–M006 records remain unchanged except additive cross-reference if repository convention requires it.
15. M008 is promoted from blocked to ready in the registry only after a closure record accepts M007.

## 12. Closure evidence required

Create:

- `plans/closure/agent-run-worktree-concurrency/007-status.md`

The record MUST contain:

- implementation commit(s);
- each post-closure finding F1–F5 mapped to production code and tests (F6/F7 remain M008 where applicable);
- migration evidence;
- root and nested fan-out evidence;
- authorization negative-test evidence;
- nested worktree/base evidence;
- max-depth evidence;
- cancellation/restart/contention review;
- exact focused verification outcomes;
- unresolved findings by severity;
- recommendation.

Do not claim final subsystem closure from M007.

## 13. Stop conditions

Stop and propose an ADR if implementation appears to require:

- scheduling the root primary turn as a durable job solely to solve group ownership;
- redefining `AgentTask` as a generic root-turn record;
- unrestricted sibling communication;
- a new scheduler/admission layer;
- automatic integration of child commits;
- weakening path/Git/worktree isolation;
- replacing existing durable stores/projection architecture rather than adapting their ownership context.

If a smaller additive type/migration can satisfy the contract, prefer it over a broad rewrite.