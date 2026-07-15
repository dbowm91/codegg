# Single-Daemon Phase 5: Global Admission Control and Initial Executor Integration

## Status

Proposed fifth implementation phase for the single-daemon multi-project orchestration roadmap.

Phases 1-4 establish one production daemon, workspace-aware execution, shared workspace services, and durable job records. This phase adds the first actual machine-wide scheduler: queue selection, weighted fairness, resource permits, exclusivity keys, bounded overload behavior, executor registration, cancellation propagation, and initial migration of tests, builds/lints/formats, and subagents.

The phase deliberately begins with execution families that are already structured and resource-heavy. Agent turns, arbitrary raw shell, Python, Git mutations, research, and plugin-spawned work are migrated in later phases after the scheduler contract is proven.

## 1. Problem statement

Codegg currently has only partial global resource control:

- `SubAgentPool` uses one semaphore for subagent concurrency;
- TestRunner supervises an individual test process but does not coordinate with other projects;
- Bash routing can invoke TestRunner, managed argv, raw shell, Python, Git, and native tools directly;
- each session permits one active turn, but many sessions can start heavy work simultaneously;
- scheduled work sends directly into subagents;
- RunStore records what happened but does not decide when execution may begin;
- configuration fields such as tool timeouts or subagent concurrency are executor-local rather than one machine-wide budget.

The desired daemon must prevent several projects from collectively overwhelming CPU, memory, process, I/O, network, Cargo target directories, or mutable workspace state.

## 2. Goals

### 2.1 Functional goals

- Implement one daemon-owned `JobScheduler` consuming durable queued jobs.
- Add bounded priority/fairness queues across workspaces and job sources.
- Add machine-wide weighted resource permits.
- Add workspace/repository/build-target exclusivity keys.
- Add a typed executor registry and dispatch admitted attempts without shell reconstruction.
- Route TestRunner jobs through scheduler admission.
- Route build, lint, and format managed-argv jobs through scheduler admission.
- Route subagent work through scheduler admission and remove its independent global semaphore as the authoritative limit.
- Propagate queued/running cancellation to executor-specific control mechanisms.
- Emit queue, admission, progress, and resource events/snapshots.
- Preserve RunStore ownership and no-double-execution behavior.

### 2.2 Fairness goals

- Prevent one workspace from monopolizing all process capacity.
- Prefer interactive work over background/scheduled work without permanently starving lower classes.
- Preserve FIFO order within equivalent workspace/priority classes where practical.
- Bound the number of queued jobs globally and per workspace.
- Make queue ordering deterministic enough for tests and diagnostics.

### 2.3 Safety goals

- Never admit work without a persisted active attempt.
- Never launch an executor twice for one attempt.
- Never release resource permits before the executor has fully stopped.
- Never hold scheduler locks across arbitrary executor awaits.
- Never retry a failed dispatch through another backend after execution may have started.
- Preserve permission, preflight, sandbox, Git, TestRunner, and command-routing policy.
- Reject or retain queued jobs cleanly when resource requests exceed configured hard capacity.

## 3. Non-goals

This phase does not:

- migrate every execution family;
- use live kernel cgroups, containers, or OS job objects for accounting;
- guarantee exact memory use;
- preempt running jobs to admit higher-priority work;
- distribute jobs across machines;
- permit multiple active attempts for one job;
- replace executor-specific timeouts and process-group cleanup;
- add arbitrary user-defined scheduler plugins;
- infer resource requests from model-generated prose.

## 4. Scheduler architecture

### 4.1 Top-level service

Add a daemon-owned service:

```rust
pub struct JobScheduler {
    store: Arc<dyn JobStore>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    executors: Arc<ExecutorRegistry>,
    admission: Arc<AdmissionController>,
    queue: Arc<FairJobQueue>,
    running: DashMap<AttemptId, RunningAttempt>,
    wake_tx: mpsc::Sender<SchedulerWake>,
    shutdown: CancellationToken,
    config: Arc<ResolvedSchedulerConfig>,
}
```

The scheduler owns queue selection and attempt lifecycle. Executors do not pull directly from JobStore.

### 4.2 Main loop

The scheduler loop should:

1. claim newly queued/due jobs from JobStore in bounded batches;
2. insert them into the in-memory fair queue using idempotent job IDs;
3. inspect queue heads for admissible work;
4. reserve resource/exclusivity permits atomically;
5. create or mark an attempt as admitted through JobStore;
6. acquire a workspace service lease;
7. dispatch to the typed executor in a spawned supervised task;
8. retain a `RunningAttempt` with cancellation and permit guards;
9. persist `Running` before or immediately around external process start using a clearly documented boundary;
10. on completion, persist terminal attempt/job state, reconcile RunStore, emit events, and drop permits/leases;
11. wake again when jobs, permits, schedules, cancellations, or executor completions change state.

Avoid polling at high frequency. Use notifications plus a low-frequency reconciliation tick.

### 4.3 Running attempt

```rust
pub struct RunningAttempt {
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub workspace_id: WorkspaceId,
    pub started_at: DateTime<Utc>,
    pub cancellation: CancellationToken,
    pub resources: ResourcePermitGuard,
    pub workspace_lease: WorkspaceServicesLease,
    pub join: JoinHandle<ExecutorCompletion>,
}
```

Store only metadata needed for cancellation/status. Do not expose raw join handles through protocol.

## 5. Fair queue design

### 5.1 Queue hierarchy

Use a hierarchy that prevents one workspace from flooding a global FIFO:

```text
Priority class
  -> workspace lane
      -> ordered jobs
```

Recommended initial policy: deficit or weighted round robin across workspace lanes within each priority class, with aging across priority classes.

A simpler implementation is acceptable if it satisfies deterministic fairness tests:

- urgent interactive lane;
- interactive lane;
- normal lane;
- background/scheduled lane;
- maintenance lane;
- round-robin workspaces within each lane;
- after a bounded number of higher-priority admissions, allow one eligible lower-priority admission.

### 5.2 Weights and aging

Persist source/priority on the job, but scheduler weights are daemon configuration:

```toml
[daemon.scheduler.fairness]
interactive_weight = 8
normal_weight = 4
background_weight = 2
maintenance_weight = 1
max_high_priority_burst = 8
aging_secs = 300
```

Aging should elevate selection eligibility, not mutate the persisted original priority.

### 5.3 Queue bounds

Configuration:

```toml
[daemon.scheduler.queue]
max_total = 256
max_per_workspace = 64
max_interactive_per_session = 8
claim_batch = 32
```

Behavior:

- interactive submission beyond capacity returns a typed overload error;
- scheduled occurrences follow their overlap/missed policy and may remain durable but not in the in-memory active window;
- maintenance submissions may be rejected or deferred first;
- existing queued durable jobs are never silently deleted to make room;
- snapshots report durable queued count separately from in-memory ready-window count.

## 6. Admission controller

### 6.1 Resource budget

```rust
pub struct ResourceBudget {
    pub max_cpu_weight: u32,
    pub max_memory_mb_hint: u64,
    pub max_process_slots: u16,
    pub max_io_weight: u32,
    pub max_network_slots: u16,
}
```

The controller tracks available capacity and an exclusivity-key map.

```rust
pub struct AdmissionController {
    budget: ResourceBudget,
    state: Mutex<AdmissionState>,
    notify: Notify,
}
```

### 6.2 Atomic permit acquisition

One job must acquire all requested dimensions and exclusivity keys atomically. Do not acquire CPU, then await memory, then await a key; that can deadlock and produce head-of-line resource retention.

Provide a nonblocking decision:

```rust
pub enum AdmissionDecision {
    Admitted(ResourcePermitGuard),
    TemporarilyBlocked(BlockReason),
    Impossible(UnschedulableReason),
}
```

The scheduler tries candidates and moves past temporarily blocked jobs where fairness rules permit, avoiding a single large job blocking all smaller work.

### 6.3 Oversized jobs

If a request exceeds hard capacity:

- mark the job blocked/unschedulable with explicit reason;
- do not clamp silently;
- allow an administrator/config change or explicit override to requeue it;
- do not execute outside admission merely because it is interactive.

### 6.4 Resource defaults

Add a central classifier for initial job families:

| Job kind | CPU | Memory hint | Process | I/O | Network | Keys |
|---|---:|---:|---:|---:|---:|---|
| Rust workspace test | 4 | 4096 MB | 1 | 3 | 0 | cargo target key |
| Python test | 2 | 2048 MB | 1 | 2 | 0 | optional workspace test key |
| build | 4 | 4096 MB | 1 | 3 | 0 | cargo target/build key |
| lint/check | 3 | 3072 MB | 1 | 2 | 0 | cargo target key |
| format check | 1 | 512 MB | 1 | 1 | 0 | none/read key |
| format mutation | 1 | 512 MB | 1 | 2 | 0 | workspace mutation key |
| subagent | 1 | 512 MB hint plus model slot | 0 until child jobs | 1 | 1 | session/subagent policy |

These are initial policy defaults, not claims about actual usage. Permit project/profile overrides within configured safe bounds.

### 6.5 Cargo target contention

Rust projects often share a target directory or use workspace-local `target/`. Resolve a canonical build-resource key:

```text
cargo-target:<canonical target dir>
```

At minimum, serialize high-contention Cargo operations targeting the same directory when repository experience shows uncontrolled concurrency causes memory/process explosions. Allow read-only metadata operations to use a lighter key/policy.

Respect the existing repository rationale for resource-capped tests and `--test-threads=1`; the scheduler does not replace test-harness thread controls.

## 7. Executor registry

### 7.1 Trait

```rust
#[async_trait]
pub trait JobExecutor: Send + Sync {
    fn kind(&self) -> JobKind;
    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError>;
    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion;
}
```

`JobExecutionContext` includes:

```rust
pub struct JobExecutionContext {
    pub job: JobRecord,
    pub attempt: JobAttempt,
    pub workspace: WorkspaceServicesLease,
    pub cancellation: CancellationToken,
    pub progress: Arc<dyn JobProgressSink>,
}
```

Executors may call existing subsystem functions. They must not reclassify a typed payload into arbitrary shell.

### 7.2 Registry behavior

- one executor per supported `JobKind` or an explicit family mapping;
- duplicate registration is an error;
- unsupported job kinds remain blocked, not shell-fallback;
- executor availability/health is visible in daemon status;
- tests can inject synthetic executors.

### 7.3 Completion

```rust
pub struct ExecutorCompletion {
    pub status: ExecutorStatus,
    pub summary: String,
    pub run_id: Option<RunId>,
    pub retry_class: FailureClass,
    pub metrics: ExecutorMetrics,
}
```

Large output remains in RunStore. Summary fields are bounded and sanitized.

## 8. Initial executor migration: TestRunner

### 8.1 Submission path

Every daemon-owned test request creates a `JobKind::Test` job before execution. Sources include:

- model-facing TestTool;
- TUI `/test`;
- Bash command-intent routing to TestRunner;
- scheduled test jobs;
- future CI/API submission.

The caller receives a job ID and may choose synchronous-wait or asynchronous behavior. The underlying execution is one scheduler attempt.

### 8.2 Test executor

`TestJobExecutor` converts the typed payload to `TestRunRequest` using workspace context and invokes existing `resolve_and_run_test`/`run_resolved_test`.

Preserve:

- custom-command validation;
- `BashDispatch` prevalidated argv semantics;
- timeout/stall timeout;
- process-group cleanup;
- streaming event sink;
- report projection;
- previous-failures index compatibility;
- canonical RunStore record ownership.

If TestRunner begins its own RunStore record, return the `run_id` so JobStore links the attempt. Scheduler/adapter must not write a duplicate run record.

### 8.3 Synchronous tool compatibility

Existing tool APIs return a string. Add a scheduler client method:

```rust
submit_and_wait(job, cancellation) -> JobTerminalResult
```

This waits on job events/state without executing locally. It must tolerate client/turn cancellation and must not cancel the daemon job automatically unless policy says the job is turn-scoped.

For an agent tool call, default the test job to turn-scoped cancellation initially, but persist the semantics explicitly. User-launched/scheduled tests may outlive a TUI connection.

## 9. Initial executor migration: build, lint, and format

### 9.1 Typed managed argv

Command-intent planning already distinguishes Build, Lint, and Format families. Extend typed payloads so active routing creates jobs containing validated argv, cwd relative to workspace, timeout, environment profile, mutation classification, and planned backend.

Do not persist raw secrets or arbitrary inherited environment.

### 9.2 Managed process executor

Create a shared `ManagedArgvExecutor` that:

- uses direct `Command::new` with argv;
- applies the workspace execution environment policy;
- sets canonical cwd;
- creates a process group/session where supported;
- streams bounded progress;
- captures output to RunStore;
- honors cancellation and timeout;
- reports actual backend as managed argv;
- never falls back to raw shell after process start or spawn ambiguity.

### 9.3 Mutation keys

- build/lint/check: target-directory exclusivity according to configured policy;
- format check: read-only/no mutation key;
- format write: workspace mutation key;
- generated-code build steps remain classified conservatively and may require workspace mutation key if effects are possible.

If classification cannot establish safe managed argv, leave the command on the existing direct path until the later raw-shell migration rather than partially routing it.

## 10. Initial executor migration: subagents

### 10.1 Replace independent admission

`SubAgentPool` currently owns `max_concurrent` and a semaphore. After migration:

- JobScheduler owns whether a subagent may start;
- `SubAgentPool` becomes an executor/worker implementation or is simplified into `SubagentJobExecutor`;
- queue capacity and fairness are scheduler-owned;
- the pool may retain an internal implementation semaphore only as a defensive cap equal to or above scheduler capacity, not as a competing scheduling policy.

### 10.2 Job identity

Each model task/subagent request creates a durable `JobKind::Subagent` linked to the parent session/turn/task record.

Preserve existing task result/report semantics and events, but add job/attempt IDs. TaskStore remains model-facing state; JobStore is execution authority.

### 10.3 Nested execution

Subagents may invoke TestTool/Bash/Python/etc. Define an initial nested-job policy:

- subagent executor holds a model/network admission permit;
- child heavy jobs are submitted separately and acquire their own process/resource permits;
- parent subagent must not reserve all potential child resources while waiting;
- cancellation propagates parent -> child through explicit dependency/ownership links;
- avoid scheduler self-deadlock when all slots are occupied by parents waiting for children.

One practical initial rule is to separate model slots from process slots and ensure child process jobs do not require the parent's model slot class.

### 10.4 Depth and limits

Preserve max-depth and max-tool-call controls. Scheduler admission does not weaken agent safety envelopes or denied-tool policy.

## 11. Cancellation and timeout integration

### Queued

Scheduler cancellation removes/marks the queued job terminal in JobStore and removes it from the in-memory lane.

### Admitted but not started

Cancel before process/model invocation, release permits, and mark attempt cancelled.

### Running tests/managed processes

Use process-group-aware cancellation already present in TestRunner or add equivalent managed-process support. Wait for bounded graceful termination, then escalate according to existing platform policy.

### Running subagents

Use `CancellationToken` and the existing cooperative cancellation path. Child jobs receive linked tokens.

### Scheduler shutdown

Implement drain modes:

```rust
pub enum SchedulerShutdownMode {
    DrainQueuedUntil(Instant),
    StopAcceptingAndCancelQueued,
    ImmediateInterrupt,
}
```

Normal daemon shutdown should stop accepting, cancel/defer queued jobs according to durable policy, request cancellation for running attempts, and mark uncompleted attempts interrupted at deadline.

## 12. Event and snapshot model

Extend events with admission metadata:

```text
JobAdmissionBlocked { reason, requested, available }
JobAdmitted { attempt_id, resources, keys }
JobResourceReleased
SchedulerOverloaded
SchedulerQueueChanged
ExecutorUnavailable
```

Do not emit high-frequency full queue snapshots. Events should be bounded deltas; clients can request snapshots.

Add `SchedulerSnapshot`:

- durable queued count;
- ready-window count;
- running attempts;
- counts by priority/workspace/kind;
- total/used/available resources;
- held exclusivity keys, with sensitive paths represented by stable redacted labels;
- oldest queued age;
- overload/rejection counts;
- executor health.

Add per-workspace queue/running summaries to `SnapshotWorkspace` and aggregate fields to `SnapshotDaemon`.

## 13. Configuration

Add validated scheduler configuration:

```toml
[daemon.scheduler]
enabled = true
reconcile_interval_ms = 1000

[daemon.scheduler.resources]
max_process_slots = 4
max_cpu_weight = 8
max_memory_mb_hint = 8192
max_io_weight = 8
max_network_slots = 4

[daemon.scheduler.queue]
max_total = 256
max_per_workspace = 64
claim_batch = 32

[daemon.scheduler.fairness]
interactive_weight = 8
normal_weight = 4
background_weight = 2
maintenance_weight = 1
max_high_priority_burst = 8
aging_secs = 300
```

Validation:

- all maxima must be positive when scheduler enabled;
- per-workspace queue cap cannot exceed global cap unless explicitly treated as no-op;
- weights must be positive;
- claim batch must be bounded;
- resource requests generated by defaults must fit the default budget;
- config reload affects future admission but does not revoke permits from running jobs.

Provide a scheduler-disabled compatibility mode only during rollout. It must be explicit and diagnostically visible.

## 14. Rollout plan

### Stage A: observe-only scheduler

- create durable jobs and calculate queue/admission decisions;
- continue immediate compatibility dispatch;
- log/emit what would have been blocked/admitted;
- compare expected concurrency and resource requests.

Observe-only mode must not create a second attempt or duplicate execution.

### Stage B: active TestRunner admission

- route tests from TestTool/TUI/Bash through scheduler;
- retain builds/subagents on compatibility path;
- validate long-running cancellation and RunStore ownership.

### Stage C: active build/lint/format admission

- enable managed argv executor for validated command families;
- preserve fallback to existing path only before a job is created/started and according to no-double-execution rules.

### Stage D: active subagent admission

- route all new subagent requests through JobScheduler;
- retain defensive pool cap temporarily;
- remove direct background scheduler -> subagent pool dispatch.

### Stage E: make scheduler mandatory for migrated families

- reject direct daemon execution for migrated kinds;
- add static checks and tests preventing bypass.

## 15. Static bypass prevention

Add a repository validation script that flags direct daemon-owned calls to:

- `resolve_and_run_test` outside TestJobExecutor and tests;
- `tokio::process::Command` for migrated Build/Lint/Format families outside ManagedArgvExecutor;
- `SubAgentPool::spawner().send` outside scheduler/subagent adapter;
- direct compatibility dispatch after the active cutover flag is enabled.

Use focused allowlists for standalone mode and test fixtures.

## 16. Testing plan

Use `--test-threads=1` for Rust tests.

### Fair queue tests

- round-robin across two or more workspaces;
- FIFO within one workspace lane;
- interactive preference;
- lower-priority progress after bounded high-priority burst;
- aging behavior;
- removed/cancelled jobs do not remain in lanes;
- duplicate durable claims do not duplicate queue entries;
- queue bounds and overload results.

### Admission tests

- atomic multidimensional permit acquisition;
- no partial permit retention on block;
- exclusivity key contention;
- unrelated keys run concurrently;
- oversized job becomes unschedulable;
- permit release on success/failure/timeout/cancel/panic;
- blocked large job does not prevent smaller eligible jobs;
- config reload affects only new admissions.

### Scheduler lifecycle tests

- persisted attempt precedes executor invocation;
- one executor invocation per attempt;
- executor panic is captured and terminal state persisted;
- cancellation in queued/admitted/running phases;
- shutdown drain and forced interruption;
- restart recovery with queued/running attempts;
- workspace lease retained while executor runs;
- no scheduler lock held during executor await.

### TestRunner integration

- TestTool/TUI/Bash all create one test job and one RunStore record;
- long-running test cancellation kills process group;
- two projects respect process/memory budget;
- shared Cargo target key serializes configured operations;
- independent Python tests may run concurrently when capacity permits;
- delegated RunId links to attempt and suppresses duplicate persistence.

### Build/lint/format integration

- validated commands execute via managed argv;
- format mutation key blocks conflicting workspace mutation;
- format check remains concurrent with safe reads;
- routing failure before start produces one clear terminal result;
- no raw-shell retry after spawn ambiguity;
- actual backend/provenance remains accurate.

### Subagent integration

- global subagent concurrency follows scheduler budget across sessions/workspaces;
- fairness prevents one session from consuming all slots;
- parent cancellation cancels child subagent job;
- subagent can submit a child test job without deadlock;
- max-depth and safety envelope remain enforced;
- task result links to job/attempt IDs.

### Contention scenario

Run one daemon with at least three temporary projects:

- project A queues several Rust tests/builds;
- project B submits an interactive lint/test;
- project C runs scheduled subagents;
- assert configured process and weighted budgets are never exceeded;
- assert B receives service without starvation;
- assert C still progresses after bounded delay;
- assert all cwd/artifacts/RunStore records remain workspace-correct.

Use synthetic executors for exhaustive fairness tests and a small bounded set of real subprocess tests to protect CI resources.

## 17. Acceptance criteria

Phase 5 is complete when:

- one daemon-owned scheduler is authoritative for queued/admitted/running state;
- TestRunner, validated build/lint/format managed processes, and subagents cannot bypass scheduler admission in daemon mode;
- machine-wide process, CPU-weight, memory-hint, I/O, network, and exclusivity-key budgets are enforced;
- workspace fairness and priority aging pass deterministic tests;
- queue capacity failures are explicit and durable jobs are not silently dropped;
- cancellation and shutdown release permits only after executors stop;
- every admitted attempt invokes exactly one executor;
- TestRunner and delegated backends produce exactly one canonical RunStore record;
- nested subagent child jobs do not deadlock the scheduler;
- daemon/workspace snapshots expose queue, resource, and executor state;
- static bypass checks prevent regression for migrated execution families;
- multi-project contention tests demonstrate bounded resource use and lack of starvation.

## 18. Handoff checklist

- [ ] Implement `FairJobQueue` with workspace lanes, priorities, burst limits, and aging.
- [ ] Implement atomic `AdmissionController` and RAII permit guards.
- [ ] Add resource defaults and exclusivity-key derivation.
- [ ] Add `JobExecutor` trait, registry, health, and synthetic test executor.
- [ ] Implement scheduler main loop, wake/reconcile path, and running-attempt registry.
- [ ] Add queued/admitted/running cancellation and shutdown modes.
- [ ] Add scheduler events and snapshots.
- [ ] Add validated scheduler configuration and rollout modes.
- [ ] Implement `TestJobExecutor` and route TestTool/TUI/Bash test requests.
- [ ] Implement managed argv executor for Build/Lint/Format.
- [ ] Convert subagent execution to scheduler authority and define nested-job behavior.
- [ ] Preserve RunStore ownership/no-double-execution semantics.
- [ ] Add static direct-execution bypass checks.
- [ ] Add fairness, admission, lifecycle, executor, contention, and cancellation tests.
- [ ] Update core, task, TestRunner, command routing, agent, RunStore, protocol, and operational documentation.
