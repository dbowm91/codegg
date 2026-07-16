# Single-Daemon Correctness, Contention, and Closure Audit

## Status

Proposed targeted closure pass following implementation of:

- the single-daemon lifecycle and default client transport;
- workspace identity and immutable execution contexts;
- daemon-owned workspace services and user-scoped storage;
- durable jobs, attempts, schedules, dependencies, cancellation, and recovery;
- global admission control and fair scheduling;
- scheduler-backed test, build, lint, format, task, subagent, and security-review execution;
- canonical managed-process execution;
- daemon job submission, waiting, and scheduler snapshot protocol surfaces.

This pass is not a new architecture phase. Its purpose is to prove that the architecture already implemented is correct under failure, contention, restart, cancellation, and multi-workspace load; close any remaining production-daemon bypasses for the migrated job classes; and produce objective evidence that the single-daemon execution invariant is operationally true.

---

## 1. Closure objective

The implementation is complete only when the following invariant is demonstrated, not merely documented:

```text
Any daemon-owned operation in a migrated heavy-work class
    -> creates or resolves one durable JobRecord
    -> is submitted through JobSubmissionService
    -> is admitted exactly once by JobScheduler
    -> owns one durable attempt and one resource permit
    -> executes through its canonical typed subsystem
    -> reaches one terminal state
    -> releases all permits, locks, child processes, and workspace leases
```

The migrated heavy-work classes in scope are:

- tests;
- builds;
- lint;
- formatting;
- scheduler-backed subagents;
- scheduled/background jobs;
- task-tool submissions;
- agent security-review jobs;
- structured Bash routes for the above classes.

The pass must also verify that explicit standalone compatibility modes remain clearly separated from the production singleton-daemon guarantee.

---

## 2. Primary goals

### 2.1 Correctness goals

- Prove that one client request cannot produce more than one durable job or more than one execution attempt unless retry policy explicitly requests another attempt.
- Prove that no migrated production-daemon execution path bypasses `JobSubmissionService` or `JobScheduler`.
- Prove that admission resource accounting is exact across success, failure, timeout, cancellation, executor panic, spawn failure, channel closure, and daemon shutdown.
- Prove that job, attempt, RunStore, scheduler event, and protocol projections remain mutually consistent.
- Prove that process trees are terminated and reaped under cancellation and timeout.
- Prove that restart recovery does not double-run completed work, lose queued work, or leave attempts permanently consuming capacity.
- Prove that scheduler-disabled daemon operation rejects heavy work instead of falling back to direct execution.

### 2.2 Contention goals

- Demonstrate bounded global concurrency across multiple workspaces.
- Demonstrate per-workspace fairness under sustained interactive and background load.
- Demonstrate that exclusivity keys prevent conflicting work while allowing unrelated projects to proceed.
- Demonstrate that one large or pathological workspace cannot indefinitely starve other projects.
- Demonstrate that queue saturation, impossible resource requests, and temporary resource pressure produce explicit bounded failures or blocking states.

### 2.3 Closure goals

- Remove obsolete production bypasses and narrow static-guard exemptions.
- Classify every remaining process-owning path as scheduler-owned, intentionally interactive, standalone-only, or deferred with an explicit follow-up issue/plan.
- Bring architecture documentation, code comments, protocol behavior, and defaults into agreement.
- Produce reproducible local and CI evidence for the full closure matrix.

---

## 3. Non-goals

This audit does not:

- redesign the daemon, workspace registry, job store, or scheduler;
- introduce distributed scheduling across multiple hosts;
- require cgroups, containers, namespaces, or platform-specific hard resource enforcement;
- force interactive terminal/editor sessions into durable batch jobs;
- replace typed Git execution with generic managed argv execution;
- move every lightweight read-only process into the scheduler in this pass;
- change provider routing, model selection, prompt policy, or agent semantics;
- optimize peak throughput at the cost of predictable bounded behavior;
- remove explicit standalone or stdio compatibility modes used for tests and embedding.

---

## 4. Audit baseline and required inventory

Before changing code, create a checked-in audit inventory covering every production source location that can:

- spawn a process;
- send work to a worker pool;
- start a test runner;
- start a background loop;
- invoke a domain-specific process service;
- create or enqueue a durable job;
- acquire scheduler permits or workspace locks.

The inventory should classify each site as one of:

```rust
pub enum ExecutionOwnershipClass {
    SchedulerOwned,
    InteractiveSession,
    StandaloneCompatibility,
    DefinitionOrAdapter,
    TestOnly,
    DeferredDomainExecutor,
    ForbiddenBypass,
}
```

A Markdown inventory is acceptable, but the preferred deliverable is a machine-readable manifest consumed by the static guard, for example:

```toml
[[site]]
path = "src/tool/test.rs"
owner = "scheduler"
kind = "test"
entrypoint = "JobSubmissionService::submit"

[[site]]
path = "src/shell_session.rs"
owner = "interactive"
reason = "long-lived user-controlled PTY"
```

The inventory must include at minimum:

- `src/tool/bash.rs`;
- `src/tool/test.rs`;
- `src/tool/task.rs`;
- `src/tui/commands/test.rs`;
- `src/tui/commands/tasks.rs`;
- `src/agent/loop.rs`;
- `src/agent/worker.rs`;
- `src/scheduler/**`;
- `src/managed_process.rs`;
- `src/test_runner/**`;
- `src/job_dispatcher.rs`;
- `src/background_task_migration.rs`;
- Git execution services;
- Python execution services;
- plugin/MCP subprocess launchers;
- shell and terminal session services;
- server/HTTP endpoints that can submit work.

### Acceptance criteria

- Every process-owning production site is classified.
- No unclassified process-spawn or worker-dispatch site remains under `src/`.
- Static validation fails when a new unclassified site is added.

---

## 5. Workstream A — submission atomicity and idempotency

### A1. Audit the submission transaction

Inspect `src/scheduler/submission.rs` and the production `JobStore` implementation to prove the logical operation:

```text
validate request
resolve workspace/session
resolve idempotency key
create or return existing job
persist queued state
wake scheduler
return stable JobId
```

cannot leave a durable job created but not discoverable by the scheduler, or enqueue two different jobs for the same bounded client retry key.

Where SQLite is authoritative, use a database transaction for idempotency-key lookup, job creation, and initial queue-state persistence. Scheduler wakeup may occur after commit, but periodic reconciliation must guarantee eventual discovery if the wakeup is lost.

### A2. Define idempotency scope

The key must be scoped tightly enough to avoid cross-client or cross-workspace collisions. Recommended identity:

```text
(client_id, workspace_id, operation_family, retry_key)
```

If protocol clients do not have stable authenticated identity yet, use the strongest available daemon-trusted client identity and document limitations.

Define:

- maximum key length;
- retention interval;
- behavior after terminal completion;
- behavior when the same key is reused with a different payload;
- response when the original job is cancelled or failed;
- cleanup/indexing policy.

Payload mismatch must produce a conflict error rather than silently returning an unrelated job.

### A3. Adversarial tests

Add tests for:

- two concurrent identical submissions;
- repeated submission after response loss;
- same key with different workspace;
- same key with different argv/test scope;
- wakeup lost after durable commit;
- database failure before commit;
- scheduler unavailable after commit;
- daemon restart between commit and admission;
- retry after terminal success;
- retry after cancellation.

### Acceptance criteria

- Concurrent duplicate requests return one stable `JobId`.
- The job executes at most once unless explicit retry policy creates another attempt.
- No committed queued job depends solely on an in-memory wakeup.
- Payload mismatch is explicit and deterministic.

---

## 6. Workstream B — scheduler authority and bypass closure

### B1. Strengthen the static guard

Extend `scripts/check_scheduler_bypass.py` to detect more than direct calls to a small set of known functions.

The guard should identify:

- `tokio::process::Command` and `std::process::Command` construction;
- direct TestRunner execution;
- direct `SubAgentPool` send/spawn operations;
- direct legacy background scheduler starts;
- direct creation/enqueue of durable jobs outside `JobSubmissionService` and migration code;
- direct executor invocation outside `JobScheduler`;
- direct managed-process execution outside registered scheduler executors or explicitly interactive/standalone surfaces.

Use the ownership inventory as the allowlist source rather than embedding broad path exemptions in the script.

### B2. Remove broad exemptions

Review current exemptions for:

- `src/tool/bash.rs`;
- `src/job_dispatcher.rs`;
- `src/background_task_migration.rs`;
- `src/agent/worker.rs`;
- standalone constructors.

Narrow each exemption to a specific function or eliminate it. A whole-file exemption is not acceptable where the file contains both scheduler-owned and compatibility paths.

Prefer annotations or a manifest entry such as:

```rust
// scheduler-audit: standalone-compat reason="explicit --standalone mode"
```

only when machine-readable checking can verify the annotation is attached to the exact call site.

### B3. Runtime proof

Add a test-only execution hook or counter at each canonical subsystem boundary:

- TestRunner entry;
- ManagedProcessService entry;
- SubAgentPool delegated execution;
- schedule occurrence materialization.

For scheduler-backed daemon tests, assert:

```text
job submissions = 1
attempts begun = 1
scheduler admissions = 1
executor entries = 1
terminal completions = 1
```

Also assert no raw-shell or direct legacy executor marker fires.

### Acceptance criteria

- No migrated production-daemon path can execute without a durable job and permit.
- Static guard exemptions are exact and justified.
- Runtime tests prove one authority chain from request to executor.
- Scheduler-disabled daemon mode returns `SchedulerDisabled` for every migrated class.

---

## 7. Workstream C — permit accounting and lifecycle invariants

### C1. Define permit conservation

For every resource dimension:

```text
available + reserved == configured capacity
```

except while a reservation mutation is held under the admission controller lock.

Dimensions include:

- CPU weight;
- memory hint;
- process slots;
- IO weight;
- network slots;
- exclusivity keys.

Add debug/test assertions that no counter underflows or exceeds capacity.

### C2. Enumerate terminal paths

Create a table-driven lifecycle test matrix covering:

| Failure point | Expected job state | Expected attempt state | Permit release | Process cleanup |
|---|---|---|---|---|
| validation rejection | rejected/not queued | none | no permit acquired | none |
| impossible request | blocked/failed per policy | none | no permit acquired | none |
| executor unavailable | queued or failed per retry policy | terminal or none | released | none |
| attempt creation failure | queued/failed | none | released | none |
| process spawn failure | failed | failed | released | none |
| executor returns error | failed/retryable | failed | released | reaped |
| timeout | failed/timed out | timed out | released | process group killed |
| cancellation before admission | cancelled | none | no permit | none |
| cancellation after admission before spawn | cancelled | cancelled | released | none |
| cancellation while running | cancelled | cancelled | released | process group killed |
| executor panic | failed/interrupted | interrupted | released | cleanup guard runs |
| daemon graceful shutdown | interrupted/recoverable | interrupted | released by process exit/recovery | children terminated |
| daemon abrupt termination | recovered next generation | interrupted | reconstructed from durable state | stale children absent or detected |

### C3. Guard ownership audit

Review all uses of `ResourcePermitGuard`, especially:

- `detach()`;
- movement into spawned tasks;
- executor futures;
- cancellation branches;
- scheduler shutdown;
- panic/unwind boundaries;
- channel send failure.

Eliminate any path where a detached guard can be lost without an explicit owner recorded in code and tests.

### C4. Attempt/executor consistency

Verify `set_attempt_executor` is called exactly once after executor selection and before execution starts. Ensure failure to persist executor identity prevents execution or leaves a clearly recoverable state; do not execute work whose durable attempt cannot identify its executor.

### Acceptance criteria

- Permit counters return to baseline after every test case.
- Exclusivity keys are always released after terminal completion.
- Executor panic and channel closure do not leak capacity.
- No job remains `running` without an active attempt owner after controlled shutdown.

---

## 8. Workstream D — cancellation and process-tree correctness

### D1. Cancellation propagation chain

Trace and test:

```text
CoreRequest / tool cancellation
    -> JobSubmissionService or JobScheduler request_cancel
    -> durable cancel_requested state
    -> running attempt CancellationToken
    -> typed executor
    -> TestRunner / ManagedProcessService / SubAgentPool
    -> child process group or delegated worker
    -> terminal cancelled state
```

Cancellation acknowledgment must not be returned merely because the request was persisted. Distinguish:

- cancellation requested;
- cancellation delivered;
- executor acknowledged;
- process tree terminated;
- job terminally cancelled.

### D2. Managed process cleanup

For Unix targets, verify process-group creation and termination for commands that spawn descendants. Test with a fixture process that:

- spawns one child and one grandchild;
- ignores or delays SIGTERM;
- writes its PIDs to a temporary file;
- remains alive long enough for cancellation.

The service should use a bounded escalation policy such as:

```text
SIGTERM process group
wait grace interval
SIGKILL process group
reap direct child
confirm known descendants gone
```

Do not claim Windows parity unless implemented and tested. Document platform-specific behavior precisely.

### D3. Subagent cancellation

Ensure scheduler cancellation reaches queued and active subagent work. A cancelled subagent attempt must not later report success and overwrite the terminal cancelled state.

### D4. Cancellation races

Add deterministic tests for:

- cancel immediately after submit;
- cancel during admission;
- cancel between attempt creation and executor start;
- completion racing cancellation;
- timeout racing cancellation;
- daemon shutdown racing cancellation;
- duplicate cancel requests.

Define terminal-state precedence. Recommended rule:

- a completion durably committed before cancellation wins;
- a cancellation durably observed by the executor before completion produces cancelled;
- terminal states are immutable.

### Acceptance criteria

- No descendant process survives cancellation/timeout fixtures.
- Duplicate cancellation is idempotent.
- Terminal job and attempt states cannot be overwritten by late executor results.
- Permit release occurs only after executor/process cleanup ownership ends.

---

## 9. Workstream E — restart and generation recovery

### E1. Recovery state model

Audit `recover_generation` and scheduler startup ordering. Required order:

```text
acquire singleton lock
open authoritative database
create new DaemonGeneration
recover stale prior-generation attempts
hydrate workspace registry/services as needed
construct scheduler and executors
reconcile durable queued/retryable jobs
bind socket and advertise readiness
```

Do not advertise daemon readiness before recovery has established a consistent queue and resource baseline.

### E2. Recovery classification

For each stale attempt, classify based on job kind, retry policy, and evidence:

- safe to retry;
- interrupted and requires user action;
- terminal failure;
- already completed with durable result;
- cancellation requested before crash.

Never retry a potentially mutating operation solely because the daemon restarted. Build/test/lint/format may be retryable according to policy; future Git mutation or deployment executors require stricter semantics.

### E3. Crash-point tests

Introduce fault-injection hooks at:

- after job creation before enqueue response;
- after queue persistence before wake;
- after attempt creation before admission event;
- after permit reservation before `mark_attempt_running`;
- after `mark_attempt_running` before process spawn;
- after process completion before terminal persistence;
- after terminal attempt persistence before client response;
- during schedule occurrence materialization.

Restart a daemon instance against the same temporary database and assert:

- no duplicate job execution;
- queued jobs are rediscovered;
- stale attempts become interrupted/retryable as defined;
- permits start at zero reserved;
- schedule occurrence uniqueness prevents double fire;
- idempotency keys still resolve to the original job.

### E4. Orphan process policy

Document and test what happens if the daemon crashes while a child process survives. At minimum:

- process groups should receive parent-death behavior where safely available, or
- startup should detect known attempt/process metadata and mark possible orphan state, and
- the daemon must not assume the old process is gone solely because generation changed.

If full orphan detection is not implemented, record this as an explicit residual risk rather than claiming complete process recovery.

### Acceptance criteria

- Recovery is deterministic across repeated restarts.
- Completed jobs never re-execute.
- Queued jobs are not lost.
- Stale running attempts do not retain logical permits.
- Schedule occurrences remain exactly-once materialized.

---

## 10. Workstream F — multi-workspace contention and fairness

### F1. Deterministic contention harness

Add an integration harness that creates at least three temporary workspaces:

- Workspace A: long-running CPU/process-heavy build fixture;
- Workspace B: short interactive test jobs;
- Workspace C: background maintenance/lint jobs.

Use test executors or controlled helper binaries so timing is deterministic and does not depend on network access.

The harness must record:

- submission order;
- admission order;
- start/end times;
- workspace lane;
- priority class;
- permits held;
- block reason;
- executor selected;
- terminal state.

### F2. Fairness scenarios

Test:

1. **Round-robin within class** — A and B each have multiple normal jobs; neither receives two admissions while the other has eligible work unless resource incompatibility explains it.
2. **Interactive floor** — B interactive jobs begin despite sustained A background load.
3. **Background progress** — C maintenance work eventually runs despite a stream of high-priority jobs, respecting `max_high_priority_burst` and aging.
4. **Workspace saturation** — A reaches its per-workspace cap while B continues.
5. **Global process cap** — total active process slots never exceed configuration.
6. **Impossible request** — a job larger than global capacity fails explicitly and does not remain queued forever.
7. **Temporary block** — a job blocked on capacity is admitted after permit release without manual wakeup.
8. **Queue overflow** — bounded rejection/event behavior is deterministic.

### F3. Exclusivity scenarios

Define canonical keys for at least:

```text
exclusive:workspace:<WorkspaceId>:mutation
exclusive:cargo-target:<canonical-target-dir-hash>
exclusive:git-worktree:<canonical-root-hash>
```

Test:

- two jobs sharing one Cargo target directory do not overlap when configured exclusive;
- unrelated Cargo target directories can run concurrently;
- workspace mutation blocks conflicting format/build/test work according to policy;
- read-only work proceeds where safe;
- cancellation releases the key;
- restart does not preserve stale logical ownership.

### F4. Starvation and bounded wait assertions

Do not use fragile exact timing thresholds. Instead assert scheduling-order invariants and bounded admission counts. Example:

```text
Within N admissions, at least one eligible non-high-priority job must start.
```

### Acceptance criteria

- Global and workspace caps hold under concurrent submissions.
- Fairness and anti-starvation behavior matches configuration.
- Exclusivity conflicts block only the intended jobs.
- One project cannot monopolize the build machine indefinitely.

---

## 11. Workstream G — resource profile calibration and pressure behavior

### G1. Profile audit

Review default `ResourceRequest` values for:

- test;
- build;
- lint;
- format;
- subagent;
- scheduled maintenance;
- security review.

Profiles must be conservative and documented. Avoid assigning every job identical weights merely to satisfy the type system.

### G2. Runtime observations

Add optional bounded observations to completed attempts where portable:

- wall-clock duration;
- maximum direct-child count or observed process count;
- exit status;
- timeout/cancellation;
- output bytes captured/truncated;
- peak RSS when available without privileged platform dependencies.

These are telemetry inputs, not scheduler truth. Missing metrics must not fail jobs.

### G3. Pressure policy

Define behavior when the host is under unexpected pressure:

- stop admitting background/maintenance work;
- retain an interactive reserve;
- emit one bounded overload event per state transition rather than flooding;
- recover automatically when pressure subsides;
- do not kill admitted work solely from a soft estimate in this pass.

If host-pressure sampling is not yet implemented, document the scheduler as reservation-based and avoid claims of hard CPU/memory enforcement.

### Acceptance criteria

- Default profiles are explicit and tested.
- Documentation distinguishes hard process/network slot caps from soft CPU/memory/IO hints.
- Metrics collection cannot block terminal persistence or leak resources.

---

## 12. Workstream H — protocol, snapshots, and persistence consistency

### H1. Job protocol audit

Verify bounded behavior for:

- `JobSubmit`;
- `JobWait`;
- cancellation requests;
- scheduler snapshot requests;
- daemon snapshot scheduler projection.

Check:

- maximum payload/argv lengths;
- maximum retry-key length;
- wait timeout bounds;
- no unbounded logs or attempt history in responses;
- workspace/session authorization and binding;
- stable error taxonomy.

### H2. State consistency tests

For each terminal result, compare:

- `JobRecord.state`;
- latest `JobAttempt.state`;
- `JobAttempt.executor`;
- `JobAttempt.run_id`;
- RunStore record when present;
- scheduler snapshot active/queued counts;
- emitted scheduler/core events;
- `JobWait` response.

No two surfaces should disagree about whether a job is queued, running, cancelled, failed, or completed after eventual consistency settles.

### H3. Event boundedness

Stress repeated admission blocking and progress updates. Confirm:

- events are delta-oriented;
- repeated identical blocked states are coalesced or rate-limited;
- event-log growth is bounded by existing retention policy;
- snapshots remain the full-state authority.

### Acceptance criteria

- Protocol responses remain bounded under pathological jobs.
- Job/attempt/RunStore/snapshot state converges consistently.
- Scheduler events cannot create an unbounded high-frequency log storm.

---

## 13. Workstream I — remaining process-owner classification

Audit process-owning domains not fully migrated in the current cutover:

- typed Git execution;
- Python tool/script execution;
- plugin and MCP local server launch;
- search/research helpers;
- language-server startup;
- interactive shell/PTY sessions;
- hook execution;
- compression or external helper backends.

For each, record one decision:

1. scheduler admission required now;
2. scheduler admission required in a future dedicated executor phase;
3. interactive long-lived service, intentionally outside durable jobs;
4. lightweight daemon service with its own bounded lifecycle;
5. standalone/test-only;
6. obsolete and removable.

Do not force all domains through `ManagedProcessService`. Preserve typed subsystem ownership. Future pattern:

```text
JobScheduler admits
    -> domain JobExecutor
    -> canonical domain service
```

Examples:

```text
GitJobExecutor -> GitExecutionService
PythonJobExecutor -> PythonScript executor
PluginServerExecutor -> plugin process supervisor
```

### Acceptance criteria

- Every remaining process owner has an explicit classification and rationale.
- No ambiguous “temporary” bypass remains undocumented.
- Deferred scheduler integrations have concrete follow-up references.

---

## 14. Workstream J — static, unit, integration, and operational evidence

### J1. Required static checks

Run and preserve output for:

```bash
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Add any new audit-manifest checker to CI.

### J2. Required narrow tests

At minimum:

```bash
cargo test -p codegg --lib scheduler
cargo test -p codegg --lib managed_process
cargo test -p codegg-core jobs
cargo test --test scheduler_phase5
cargo test --test workspace_isolation
cargo test --test single_daemon_lifecycle
```

Add dedicated suites such as:

```text
tests/scheduler_submission_idempotency.rs
tests/scheduler_permit_lifecycle.rs
tests/scheduler_cancellation.rs
tests/scheduler_restart_recovery.rs
tests/scheduler_contention.rs
tests/scheduler_authority_matrix.rs
```

### J3. Full suite

Run the repository's canonical full validation path under the configured resource-conscious test settings. Do not silently increase test parallelism beyond what the build environment can safely support.

Record:

- command;
- commit SHA;
- host OS/architecture;
- Rust version;
- test thread/nextest settings;
- start/end timestamps;
- pass/fail counts;
- any quarantined or ignored tests;
- static-check results.

### J4. CI evidence

Ensure direct pushes or the closure PR produce visible GitHub Actions status checks on the exact implementation commit. If CI cannot run the full contention suite on hosted runners, split it into:

- deterministic CI-safe integration tests;
- an explicit local/Ubuntu operational script producing a machine-readable report.

The absence of external status evidence must be reported as a remaining verification gap.

### Acceptance criteria

- All static guards pass.
- Narrow scheduler and lifecycle suites pass.
- Full repository validation passes or every failure is documented and resolved before closure.
- Exact commit-level CI evidence is visible, or the residual evidence limitation is explicitly recorded.

---

## 15. Deliverables

The implementation agent should produce:

1. machine-readable execution ownership inventory;
2. strengthened scheduler-bypass checker;
3. submission/idempotency tests and any required transaction fixes;
4. permit lifecycle and panic/failure-path tests;
5. cancellation/process-tree fixtures and tests;
6. restart/fault-injection recovery tests;
7. deterministic multi-workspace contention harness;
8. exclusivity-key tests;
9. remaining process-owner classification document;
10. updated scheduler/jobs/protocol/managed-process architecture documentation;
11. closure evidence report under `plans/` or `docs/verification/`;
12. removal or narrowing of obsolete compatibility exemptions;
13. any corrective source changes required by the audit.

Recommended evidence file:

```text
plans/single-daemon-correctness-contention-closure-status.md
```

It should contain an invariant-by-invariant table with:

- status: proven / failed / deferred;
- test or static-check reference;
- commit SHA;
- residual risk;
- follow-up reference where applicable.

---

## 16. Required implementation sequence

Execute in this order to avoid hiding defects behind broader stress tests:

### Step 1 — Inventory and static authority map

- enumerate process/worker/job ownership sites;
- implement machine-readable classifications;
- strengthen static checks;
- establish current failures before changing behavior.

### Step 2 — Submission and idempotency correctness

- test concurrent duplicates and crash boundaries;
- correct transaction boundaries;
- validate retry-key payload matching.

### Step 3 — Permit and terminal-state correctness

- add lifecycle matrix;
- fix guard ownership, executor identity, and terminal immutability;
- prove all counters return to baseline.

### Step 4 — Cancellation and process cleanup

- add descendant-process fixture;
- test timeout/cancel races;
- correct process-group and subagent cancellation behavior.

### Step 5 — Restart recovery

- add fault injection;
- restart against persistent temporary databases;
- verify queue, attempts, schedules, and idempotency.

### Step 6 — Multi-workspace contention

- run deterministic fairness, capacity, exclusivity, and starvation tests;
- calibrate resource profiles where evidence shows poor defaults.

### Step 7 — Remaining process-owner audit

- classify Git, Python, plugin, LSP, interactive, and helper processes;
- create explicit follow-up references rather than broad exemptions.

### Step 8 — Documentation and evidence closure

- update architecture docs;
- run narrow and full validation;
- publish status/evidence file;
- remove temporary audit hooks that are not useful as permanent regression tests.

---

## 17. Stop-the-line findings

The implementation must not be declared closed if any of the following are observed:

- one request can start the same work twice without explicit retry;
- a scheduler-backed route falls back to raw shell after submission/execution failure;
- a permit or exclusivity key leaks after a terminal path;
- a cancelled or timed-out managed process leaves descendants alive;
- a late executor result overwrites a terminal cancelled/failed state;
- daemon restart re-executes completed work;
- queued jobs disappear after restart;
- scheduler-disabled daemon mode launches direct heavy work;
- one workspace can indefinitely starve another despite eligible work;
- a production process-spawn site remains unclassified;
- the static bypass checker requires a broad whole-directory exemption for normal production code;
- full validation or CI evidence is unavailable without an explicit residual-risk record.

Any stop-the-line issue should be corrected in this pass, not deferred as polish.

---

## 18. Definition of done

This closure pass is complete only when all of the following are true:

- the production singleton daemon has one authoritative submission and admission chain for every migrated heavy-work class;
- idempotency and crash-boundary tests prove at-most-once job creation/execution semantics, except explicit retry attempts;
- resource permits and exclusivity keys are conserved across every lifecycle path;
- cancellation and timeout terminate delegated work and process trees;
- restart recovery preserves queued work and does not duplicate completed work;
- deterministic multi-workspace tests prove fairness, bounded concurrency, and anti-starvation;
- remaining process owners are explicitly classified;
- static guards reject new unclassified bypasses;
- scheduler, job, attempt, RunStore, event, snapshot, and protocol states converge consistently;
- architecture documentation matches actual runtime behavior;
- narrow tests, full validation, and commit-level evidence are recorded;
- no stop-the-line finding remains open.

At that point, the first single-daemon multi-project orchestration roadmap can be considered operationally closed. Further work—such as scheduler admission for typed Git, Python, plugin servers, or host-pressure feedback—can proceed as separately scoped domain integrations rather than correctness repairs to the core daemon architecture.
