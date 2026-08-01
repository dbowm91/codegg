# Agent Runtime, Model Adaptation, and ACP Milestone 016 — Descendant Admission, Cancellation, and Execution Context

Status: implemented

Repository baseline: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-016--descendant-admission-cancellation-and-execution-context`

Historical plans corrected by this milestone:

- `plans/implementation/agent-runtime-model-adaptation-acp/003-bounded-nested-agent-delegation.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/002-resolved-capability-and-tool-surface.md`

Primary class: authority/concurrency

## 1. Objective

Correct descendant admission and cancellation ownership so active-capacity limits cannot be oversubscribed under concurrent enqueue, every reservation is released exactly once, cancellation is scoped to one root lineage rather than the entire pool, and every native tool execution receives cwd/workspace identity from explicit execution context rather than process-global state.

The milestone must remain a bounded compatibility correction over the existing `SubAgentPool`, scheduler, task store, tool broker, and `ExecutionContext`. It must not implement the deferred durable AgentRun schema, worktree-native mutation isolation, or team authorization.

## 2. Dependencies

Hard dependency:

- Milestone 015 strict closure, preserving one sequential handoff across agent-loop/provider changes and ensuring adapter/tool alias behavior is stable before concurrency/authority closure.

Existing foundations:

- `SubAgentPool` owns one shared request queue, worker semaphore, task store, accepted identities, delegation keys, fan-out/tool/time limits, and shutdown token;
- child loops receive the shared pool and explicit denied tools/allowed paths/model/depth;
- `TaskTool` and scheduler-backed submission are the existing descendant entry points;
- `AgentLineageContext` and transient lineage identifiers exist;
- `AgentLoop` has optional `workspace_root` and production root turns have an explicit `ExecutionContext`;
- native tools receive `ToolExecutionContext`, but its cwd is currently populated from `std::env::current_dir()` in the ordinary loop;
- static ownership guards already check several daemon/scheduler/cwd boundaries.

## 3. Current implementation evidence

Re-audit at implementation time. At the reviewed baseline:

- enqueue checks `active_count() >= max_active_descendants` before placing work on the queue;
- `active_count` increments later inside the spawned worker task, so multiple concurrent enqueue calls can all pass the limit before any increment occurs;
- the worker semaphore limits concurrent execution but does not make the configured active-descendant admission claim exact;
- direct-child counts and cumulative child tool-call reservations have several rollback paths, but lifecycle ownership is distributed and must be audited for completion/cancellation/timeouts;
- pool shutdown uses one pool-global `CancellationToken`; a root/parent-specific cancellation tree is not the clear authority path;
- accepted task/delegation identities and some counters are retained in shared maps without a single RAII admission lease;
- `AgentLoop::build_tool_execution_context` populates cwd from process-global current directory even when `workspace_root` is available;
- root turns own an explicit `ExecutionContext`, while descendant loops carry allowed paths and sometimes a workspace root but do not have one canonical tool-execution identity source.

## 4. Invariants that must not regress

- The shared pool/scheduler remains the only descendant admission authority.
- Admission limits are enforced atomically before a request is accepted.
- A rejected request consumes no persistent capacity, fan-out count, identity, or tool budget.
- An accepted request releases every active reservation exactly once on completion, failure, cancellation, timeout, queue failure, worker panic/abort, or shutdown.
- Duplicate task/delegation identity remains idempotently rejected or reused according to the existing contract.
- Parent/root cancellation reaches all accepted descendants of that lineage.
- Cancelling one root lineage does not cancel unrelated roots.
- Pool-wide shutdown still cancels all lineages and joins/aborts as a bounded fallback.
- Child authority remains the intersection of parent/session/config/agent/path/tool/hard limits.
- Native tool cwd and workspace identity come from explicit execution context.
- No production tool dispatch uses process-global cwd as an authority source.
- Mutation-capable parallel children are not represented as safely isolated without worktrees.

## 5. Scope

### In scope

- Introduce an atomic active-descendant admission reservation/permit.
- Consolidate admission identity, fan-out, active, and tool-budget rollback/release through an RAII lease or equivalent single owner.
- Clarify whether direct-child limit means total accepted fan-out or concurrently active children and implement/document it consistently.
- Add root/lineage-scoped cancellation tokens and a bounded registry/tree of accepted descendants.
- Cascade cancellation through queued and running descendants of one root.
- Keep pool-wide shutdown as an independent global cancellation path.
- Ensure timeout and abort paths release all reservations.
- Carry explicit `ExecutionContext` or a minimal immutable child execution context into descendant loops and native tool dispatch.
- Replace `std::env::current_dir()` in production `ToolExecutionContext` construction with explicit workspace root.
- Add concurrency, cancellation-isolation, cleanup, and cwd ownership fixtures.
- Update architecture/config/static guards and closure records.

### Explicitly out of scope

- Durable AgentRun persistence, restart recovery, or lineage database migration.
- Worktree creation/cleanup for mutation-capable descendants.
- Team/principal authorization or multi-user policy completion.
- New scheduler implementation or specialized descendant queue.
- Distributed/multi-daemon execution.
- Broad performance stress infrastructure.
- Changing ordinary tool semantics unrelated to execution identity.

## 6. Required production changes

### Atomic admission lease

Create one admission operation that validates and reserves all transient resources before queue acceptance:

- global enabled/target policy;
- depth;
- active descendant capacity;
- direct-child/fan-out capacity;
- task/delegation identity;
- cumulative child tool-call budget;
- optional root/lineage capacity.

Use an RAII lease or explicit state object whose drop/finalize semantics release active-only reservations. Separate lifetime/cumulative counters from active counters intentionally.

A possible shape:

```rust
struct DescendantAdmissionLease {
    root_id: AgentLineageId,
    task_id: u64,
    delegation_id: DelegationId,
    active_permit: OwnedSemaphorePermit,
    active_fanout_reservation: Option<FanoutReservation>,
    budget_reservation: BudgetReservation,
    registry: Arc<AdmissionRegistry>,
    released: AtomicBool,
}
```

The exact type may differ. Do not rely on “check then increment later.”

### Capacity semantics

Define and document distinct limits:

- `max_concurrent`: workers actively executing provider/tool work;
- `max_active_descendants`: accepted queued + running descendants across the pool or per root, as configured;
- `max_direct_children`: either total accepted children per parent lineage or active direct children—choose one explicit meaning and name/document it accordingly;
- `max_total_child_tool_calls`: cumulative root/pool budget, not an active count.

Do not overload one atomic counter for multiple semantics. Preserve defaults and avoid adding many new knobs unless needed to distinguish existing ambiguous behavior.

### Root-scoped cancellation

- create or register one cancellation token per root lineage;
- derive child tokens from the root/parent token;
- queued workers select on their lineage token and global shutdown token;
- running child loops receive the lineage cancellation signal through their ordinary loop/tool cancellation seam;
- root completion removes the registry entry after all accepted descendants are terminal or detached according to policy;
- cancel root A does not signal root B;
- global shutdown signals all root tokens.

If the native root turn has an existing cancellation token/channel, bridge it rather than creating a parallel source of truth.

### Reservation release

Audit and centralize release for:

- queue send failure;
- cancellation while waiting for worker semaphore;
- ordinary success/failure;
- wall-clock timeout;
- provider/tool error;
- task/worker panic or join abort;
- pool shutdown fallback.

Use idempotent release and tests that inspect counts/maps after every terminal path. Cumulative tool-call budgets may remain consumed after accepted execution if that is the intended root budget; document the distinction.

### Explicit descendant execution context

Extend `SubAgentRequest`/worker construction or an internal context object with:

- workspace root/project identity;
- allowed path ceiling;
- optional runtime asset snapshot/pin identity;
- parent/root lineage identity;
- cancellation token/deadline;
- provider/model/tool-surface identity where already available.

Do not reconstruct workspace from `parent_id`, session display strings, allowed-path first element, or process cwd.

### Native tool execution context

`AgentLoop::build_tool_execution_context` must use `self.workspace_root` or an explicit execution context captured at loop construction. If a legacy/test loop has no workspace root:

- require the caller to supply one for production execution;
- use a clearly non-authoritative test default only in isolated tests;
- return a typed error rather than silently using process cwd where authority matters.

Populate workspace ID/path-policy identity from the same explicit context. Audit terminal, batch, task, Git, research, and MCP/native dispatch paths for consistent context propagation.

### Static ownership guard

Extend the existing cwd/ownership guard narrowly to reject production `std::env::current_dir()` use in agent/tool execution modules while permitting explicit CLI/bootstrap/test locations. Prefer a source-aware allowlist over a repository-wide brittle string ban.

## 7. Ordered work packages

### Work package A — Admission semantics and lease

- document current/new limit meanings;
- implement atomic active reservation;
- consolidate identity/fan-out/budget admission;
- make queue acceptance transfer lease ownership to worker lifecycle.

Acceptance evidence:

- concurrent barrier fixture cannot accept more than configured active capacity;
- rejected requests leave all counters/maps unchanged;
- duplicate identity behavior remains deterministic.

### Work package B — Release and timeout correctness

- centralize terminal release;
- cover queue, semaphore, success, failure, timeout, cancellation, abort, and shutdown;
- distinguish cumulative versus active reservations;
- add invariant assertions/diagnostics.

Acceptance evidence:

- every terminal fixture returns active counters and registry to expected state;
- release is idempotent;
- no underflow or leaked permit remains.

### Work package C — Root-scoped cancellation

- register lineage/root cancellation tokens;
- bridge native parent cancellation;
- cancel queued/running descendants of one root;
- isolate unrelated roots;
- retain global shutdown.

Acceptance evidence:

- cancel root A interrupts A children only;
- root B completes normally;
- pre-start and running child cancellation are both covered;
- pool shutdown cancels all and terminates boundedly.

### Work package D — Explicit execution context

- propagate workspace/root identity to descendants;
- replace process-cwd tool execution context;
- audit all native dispatch sites;
- update cwd/ownership guard.

Acceptance evidence:

- two concurrent projects dispatch tools with distinct correct cwd values;
- changing process cwd cannot affect a running/root/child tool context;
- missing production workspace context fails explicitly.

### Work package E — Documentation and closure handoff

- update agent/config/tool/scheduler/workspace architecture;
- create M016 closure record only after independent review;
- promote M017 only on strict closure.

## 8. Failure, cancellation, restart, and contention semantics

- Admission is atomic: either every required reservation is acquired and the request is accepted, or no reservation remains.
- Queue failure releases the lease immediately.
- Cancellation while queued or waiting for execution releases active reservations and writes one interrupted terminal task state.
- Timeout cancels child execution and releases active reservations after bounded cleanup.
- Worker panic/abort is detected by owned join/lease cleanup and cannot permanently consume capacity.
- Root cancellation is idempotent and may be requested before children start.
- Global shutdown supersedes root tokens and cancels all work.
- Daemon restart may interrupt transient descendants; durable reconciliation remains deferred and must not be claimed.
- Concurrent admission tests remain bounded and deterministic; avoid probabilistic soak tests.

## 9. Compatibility and migration

- Existing subagent configuration remains readable. Rename/document ambiguous fields only with serde aliases/backward compatibility.
- Existing `SubAgentRequest` callers are updated internally; external native protocol should not change unless execution context already crosses it additively.
- No durable storage migration is required.
- Existing task/session display IDs remain compatible and are not promoted to durable AgentRun identity.
- Existing global shutdown behavior remains available.
- Mutation-capable child behavior remains restricted by current authority/worktree limitations.

## 10. Required tests

### Admission/concurrency tests

- concurrent N enqueues with limit K accept at most K active descendants;
- worker semaphore and active-descendant limits remain distinct;
- duplicate task/delegation identity;
- direct-child limit semantics;
- cumulative tool budget boundary;
- queue-full/send-failure rollback.

### Release tests

- ordinary success;
- provider/tool failure;
- wall-clock timeout;
- cancellation before semaphore;
- cancellation during execution;
- worker abort/panic fixture;
- pool shutdown;
- double-release/idempotency;
- counters/maps/permits return to expected state.

### Cancellation-isolation tests

- two roots with children; cancel one root only;
- parent cancellation before child start;
- nested child cancellation cascade;
- sibling completion/failure independence;
- global shutdown cancels both roots;
- no completed result after cancellation.

### Execution-context tests

- explicit workspace root in every native tool context;
- simultaneous projects use distinct cwd/workspace IDs;
- process cwd mutation has no effect;
- descendant inherits correct workspace and narrower path ceiling;
- missing workspace context fails in production path;
- Git/shell/read/write/task/batch representative tools receive same context owner.

### Negative/security tests

- child cannot widen allowed paths/tools/model/depth;
- wire alias cannot alter canonical permission result;
- root cancellation cannot cancel another root;
- no production `current_dir()` in guarded execution modules;
- no worktree-isolation claim for parallel mutation.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test --test subagent -- --test-threads=4
cargo test --test agent_loop_harness -- --test-threads=4
cargo test -p codegg agent::worker
cargo test -p codegg tool::task
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_tool_broker_boundary.py
```

Add deterministic barrier-based tests rather than long stress/chaos suites. Run the canonical quick verification command at handoff; do not add a platform matrix.

## 12. Documentation updates

- `architecture/agent.md`: admission lease, lineage cancellation, and transient limits;
- `architecture/config.md`: exact limit semantics and defaults;
- tool/scheduler/workspace architecture: explicit execution context ownership;
- test/verification documentation for the narrow cwd guard;
- corrective addendum, registry, and M016 closure record.

## 13. Acceptance criteria

- Active descendant admission is atomic and cannot oversubscribe.
- Every accepted request owns one idempotent lifecycle lease.
- All rejection/terminal paths release active reservations exactly once.
- Root cancellation reaches queued/running descendants of that root only.
- Global shutdown still cancels all work boundedly.
- Direct-child, active, concurrent, and cumulative tool-budget semantics are distinct and documented.
- Root and child native tool execution use explicit workspace context.
- Production agent/tool execution no longer derives cwd from process-global state.
- Authority/path/tool/model ceilings remain narrowing-only.
- Focused concurrency/cancellation/context tests and static guards pass.

## 14. Stop conditions

Stop and report if:

- correct cancellation requires implementing durable AgentRun persistence/restart recovery;
- mutation-capable concurrency requires worktree allocation;
- team/principal authorization must be invented rather than preserving its seam;
- the existing scheduler cannot carry cancellation/execution context without a separate redesign;
- a repository-wide cwd ban would break legitimate CLI/bootstrap code and cannot be scoped safely;
- distributed or multi-daemon scheduling becomes necessary.

## 15. Required closure evidence

The closure record must include:

- limit semantics table;
- atomic admission barrier fixture results;
- reservation release matrix for every terminal path;
- root-cancellation isolation and global-shutdown evidence;
- two-project explicit cwd/workspace evidence;
- static guard results;
- focused command results and exact commits;
- explicit deferred durable AgentRun/worktree limitations;
- remaining low-severity findings;
- recommendation to promote or block Milestone 017.
