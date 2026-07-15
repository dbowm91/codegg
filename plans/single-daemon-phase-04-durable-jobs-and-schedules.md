# Single-Daemon Phase 4: Durable Jobs and Schedules

## Status

Proposed fourth implementation phase for the single-daemon multi-project orchestration roadmap.

Phases 1-3 establish one production daemon, typed workspace execution context, and daemon-owned workspace services. This phase introduces a durable execution-control domain: jobs, attempts, schedules, dependencies, cancellation, leases, retry policy, and restart recovery. The scheduler may initially execute through a compatibility path; the primary objective is to establish authoritative identity and lifecycle before adding global admission policy in Phase 5.

## 1. Problem statement

Codegg currently has several overlapping notions of work:

- an agent turn identified by `turn_id`;
- a subagent task persisted through `TaskStore` and dispatched through `SubAgentPool`;
- `BackgroundScheduler` entries stored in an in-memory vector and optionally persisted in the `task` table;
- TestRunner `RunId` and test lifecycle events;
- RunStore records for shell, Python, Git, tests, and other execution backends;
- long-horizon goals and session todos;
- frontend-local command invocations.

These identities are not one orchestration model. In particular:

- `BackgroundTask::new` creates UUID string IDs while recurring dispatch parses them as `u64`;
- `TaskDelete` accepts a numeric ID and converts it to string;
- `TaskSchedule` can enqueue an immediate subagent using `task_id: 0` independently of the recurring record;
- daemon startup does not provide one complete durable recovery path for scheduled work;
- `running` state is not protected by a daemon generation lease;
- retries, dependencies, overlap policy, idempotency, and missed schedules are not modeled explicitly;
- RunStore describes execution artifacts but is not the source of truth for queue lifecycle.

The daemon needs a durable control record that exists before execution begins and remains authoritative through restart.

## 2. Goals

### 2.1 Functional goals

- Introduce stable typed IDs for jobs, attempts, schedules, and dependencies.
- Persist every lifecycle transition before or atomically with externally visible state changes.
- Represent immediate, deferred, and recurring work through one job model.
- Separate a logical job from one or more execution attempts.
- Add cancellation requests that work while queued or running.
- Add retry, timeout, idempotency, overlap, and missed-run policy.
- Add daemon generation leases so stale `running` attempts can be recovered deterministically.
- Link jobs to workspace, optional session, optional turn, and eventual RunStore records.
- Expose list/get/cancel/retry/schedule operations through the core protocol.
- Replace new uses of `BackgroundScheduler` with durable schedule-to-job creation.

### 2.2 Correctness goals

- No UUID/numeric ID conversion ambiguity.
- A schedule firing creates at most one job for a given scheduled occurrence.
- A logical job has at most one active attempt unless explicit parallel-attempt policy is introduced later.
- Restart recovery never leaves an attempt indefinitely `running` under an expired daemon generation.
- Cancellation is monotonic and cannot be cleared by a stale worker.
- Retry creates a new attempt and preserves previous attempt history.
- Terminal state is written once and cannot regress to a nonterminal state.
- RunStore failure does not cause the command to execute again.

### 2.3 Maintainability goals

- Keep scheduling persistence in `codegg-core` without UI/server dependencies.
- Use a transition API rather than scattered SQL updates.
- Make state-machine tests exhaustive and table-driven.
- Preserve existing TestRunner/RunStore execution semantics through adapters.

## 3. Non-goals

This phase does not:

- implement weighted fairness or machine-wide resource budgets;
- require every executor to be migrated immediately;
- add remote distributed workers;
- guarantee exactly-once effects across process crashes for arbitrary commands;
- infer perfect idempotency automatically;
- implement cron syntax beyond what can be validated safely in this phase;
- replace goals or todos with jobs;
- make RunStore the queue database;
- automatically retry destructive or non-idempotent operations.

## 4. Domain model

### 4.1 Typed identifiers

Add newtypes with validated serialization:

```rust
pub struct JobId(String);
pub struct AttemptId(String);
pub struct ScheduleId(String);
pub struct DependencyId(String);
pub struct DaemonGeneration(String);
```

Use UUID v4 or UUID v7 consistently. IDs are opaque and never parsed into integers.

### 4.2 Job kind

```rust
pub enum JobKind {
    AgentTurn,
    Subagent,
    Build,
    Test,
    Lint,
    Format,
    Shell,
    ManagedProcess,
    Python,
    GitRead,
    GitMutation,
    Research,
    Maintenance,
}
```

The enum must be forward-compatible in persisted form. Unknown future kinds should produce a typed unsupported state rather than being executed as shell.

### 4.3 Job source and priority

```rust
pub enum JobSource {
    Interactive,
    Scheduled { schedule_id: ScheduleId, occurrence: DateTime<Utc> },
    AgentDelegated,
    Retry { prior_attempt_id: AttemptId },
    Maintenance,
    Api,
}

pub enum JobPriority {
    Urgent,
    Interactive,
    Normal,
    Background,
    Maintenance,
}
```

Priority affects Phase 5 admission. In this phase it is persisted and validated.

### 4.4 Job specification

```rust
pub struct JobSpec {
    pub job_id: JobId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: JobKind,
    pub source: JobSource,
    pub priority: JobPriority,
    pub payload: JobPayload,
    pub resource_request: ResourceRequest,
    pub timeout: Option<Duration>,
    pub retry_policy: RetryPolicy,
    pub idempotency: IdempotencyClass,
    pub created_at: DateTime<Utc>,
    pub not_before: Option<DateTime<Utc>>,
    pub deadline: Option<DateTime<Utc>>,
}
```

`ResourceRequest` may be stored before Phase 5 uses it. Persist explicit defaults so later scheduler changes do not reinterpret old jobs unexpectedly.

### 4.5 Payload

Use typed payload variants rather than opaque shell strings:

```rust
pub enum JobPayload {
    AgentTurn(AgentTurnJob),
    Subagent(SubagentJob),
    Test(TestJob),
    ManagedArgv(ManagedArgvJob),
    Shell(ShellJob),
    Python(PythonJob),
    Git(GitJob),
    Research(ResearchJob),
    Maintenance(MaintenanceJob),
}
```

Persist enough data to rerun safely without consulting stale client state. Secret material must not be persisted in plaintext. Use credential references and existing redaction/audit-safe argv policy.

### 4.6 Job and attempt states

```rust
pub enum JobState {
    Scheduled,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
    Blocked,
    Expired,
}

pub enum AttemptState {
    Created,
    Admitted,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}
```

`JobState::Running` means one attempt is active. The current attempt ID must be stored explicitly.

### 4.7 Transition rules

Implement a central state machine. At minimum:

```text
Scheduled -> Queued | Cancelled | Expired
Queued -> Running | Cancelled | Expired | Blocked
Running -> Completed | Failed | Cancelled | TimedOut | Interrupted
Failed -> Queued only through explicit retry decision
Interrupted -> Queued only when recovery policy permits
Blocked -> Queued when dependencies become satisfied
Terminal states do not transition except by creating a new job or attempt.
```

Do not expose generic `set_state` methods to callers. Provide intent-specific operations such as `enqueue`, `begin_attempt`, `mark_running`, `complete`, `fail`, `request_cancel`, `interrupt_generation`, and `retry`.

### 4.8 Attempts and leases

```rust
pub struct JobAttempt {
    pub attempt_id: AttemptId,
    pub job_id: JobId,
    pub sequence: u32,
    pub state: AttemptState,
    pub daemon_generation: DaemonGeneration,
    pub executor: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub run_id: Option<RunId>,
    pub error: Option<JobErrorRecord>,
}
```

A running attempt is valid only while its daemon generation matches the active daemon generation and its lease/heartbeat is within policy.

Initial implementation may update heartbeat on significant progress rather than a high-frequency timer. Do not write continuously to SQLite for every output line.

## 5. Resource request and exclusivity metadata

Persist the Phase 5 scheduler request now:

```rust
pub struct ResourceRequest {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<ResourceKey>,
}
```

Examples:

- `cargo test`: process slot 1, high CPU, high memory hint, `cargo-target:<canonical-target-dir>` key when needed;
- Git mutation: process slot 1, `git-worktree:<canonical-root>:mutation`;
- format: process slot 1, workspace mutation key;
- agent turn: model/network slot plus potential nested-job policy;
- read-only native tool: possibly zero process slots if in-process.

Use conservative defaults. Jobs with unknown resource needs should not be assigned zero cost.

## 6. Schedule model

### 6.1 Schedule record

```rust
pub struct ScheduleRecord {
    pub schedule_id: ScheduleId,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<String>,
    pub kind: ScheduleKind,
    pub job_template: JobTemplate,
    pub state: ScheduleState,
    pub overlap_policy: OverlapPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_occurrence_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Initial schedule kinds:

```rust
pub enum ScheduleKind {
    OneShot { run_at: DateTime<Utc> },
    Interval { every: Duration, anchor: DateTime<Utc> },
}
```

Calendar/cron syntax can be added later after timezone and DST semantics are specified.

### 6.2 Occurrence identity

A schedule occurrence must have a unique persisted key:

```text
(schedule_id, scheduled_for)
```

Create a uniqueness constraint so restart or duplicate ticks cannot enqueue the same occurrence twice.

### 6.3 Overlap policy

```rust
pub enum OverlapPolicy {
    SkipIfRunning,
    QueueOne,
    Allow,
}
```

Default scheduled work to `SkipIfRunning` or `QueueOne` based on job kind. Do not allow unlimited overlap by default.

### 6.4 Missed-run policy

```rust
pub enum MissedRunPolicy {
    Skip,
    RunOnceNow,
    CatchUpBounded { max_occurrences: u32 },
}
```

Default to `RunOnceNow` for safe idempotent maintenance and `Skip` for potentially expensive recurring agent work. Never unboundedly replay missed intervals after a long shutdown.

## 7. Persistence schema

Add migrations under the existing core/session schema system or a dedicated scheduler migration module.

Suggested tables:

```sql
CREATE TABLE job (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
    kind TEXT NOT NULL,
    source_json TEXT NOT NULL,
    priority TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    resource_json TEXT NOT NULL,
    retry_json TEXT NOT NULL,
    idempotency TEXT NOT NULL,
    state TEXT NOT NULL,
    current_attempt_id TEXT,
    not_before INTEGER,
    deadline INTEGER,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER
);

CREATE TABLE job_attempt (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    state TEXT NOT NULL,
    daemon_generation TEXT NOT NULL,
    executor TEXT,
    run_id TEXT,
    heartbeat_at INTEGER,
    time_started INTEGER,
    time_completed INTEGER,
    error_json TEXT,
    UNIQUE(job_id, sequence)
);

CREATE TABLE job_dependency (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    depends_on_job_id TEXT NOT NULL,
    condition TEXT NOT NULL,
    UNIQUE(job_id, depends_on_job_id)
);

CREATE TABLE schedule (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    session_id TEXT,
    kind_json TEXT NOT NULL,
    job_template_json TEXT NOT NULL,
    state TEXT NOT NULL,
    overlap_policy TEXT NOT NULL,
    missed_run_policy_json TEXT NOT NULL,
    next_run_at INTEGER,
    last_occurrence_at INTEGER,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL
);

CREATE TABLE schedule_occurrence (
    schedule_id TEXT NOT NULL,
    scheduled_for INTEGER NOT NULL,
    job_id TEXT,
    status TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    PRIMARY KEY(schedule_id, scheduled_for)
);
```

Add indexes for queue scans by state/priority/not-before/workspace, attempts by job, and schedules by next-run time.

Use transactions for lifecycle transitions that update job and attempt together.

## 8. Core service APIs

Add a UI-independent `JobStore` trait/implementation in `codegg-core`:

```rust
#[async_trait]
pub trait JobStore: Send + Sync {
    async fn create_job(&self, spec: NewJob) -> Result<JobRecord, JobStoreError>;
    async fn get_job(&self, id: &JobId) -> Result<Option<JobRecord>, JobStoreError>;
    async fn list_jobs(&self, query: JobQuery) -> Result<Vec<JobSummary>, JobStoreError>;
    async fn enqueue(&self, id: &JobId) -> Result<JobRecord, JobStoreError>;
    async fn create_attempt(&self, id: &JobId, generation: &DaemonGeneration) -> Result<JobAttempt, JobStoreError>;
    async fn mark_attempt_running(&self, attempt: &AttemptId) -> Result<(), JobStoreError>;
    async fn finish_attempt(&self, completion: AttemptCompletion) -> Result<JobRecord, JobStoreError>;
    async fn request_cancel(&self, id: &JobId, reason: CancelReason) -> Result<CancelResult, JobStoreError>;
    async fn recover_generation(&self, stale: &DaemonGeneration, policy: RecoveryPolicy) -> Result<RecoveryReport, JobStoreError>;
}
```

Provide a real SQLite implementation and an in-memory implementation for state-machine/scheduler tests.

Add a `ScheduleStore` with transactional due-occurrence claiming.

## 9. Compatibility execution adapter

Before Phase 5 admission exists, add a narrow compatibility runner:

```rust
pub trait JobDispatcher {
    async fn dispatch_created_job(&self, job: JobRecord) -> Result<(), DispatchError>;
}
```

The adapter may immediately create an attempt and invoke the existing backend, but must still use durable lifecycle transitions. It must not be exposed as the final scheduler design.

Recommended initial integrations:

- create a durable job for `TaskSchedule` and schedule firings;
- create durable subagent job records before sending to `SubAgentPool`;
- optionally wrap direct TestRunner launches in job records for validation;
- retain existing direct turn execution until Phase 5/6.

Do not create a job after execution has already started. The durable record precedes dispatch.

## 10. RunStore linkage

Job and RunStore serve different purposes:

- JobStore owns queue/lifecycle/control state.
- RunStore owns execution provenance, output, artifacts, changes, and rerun descriptors.

When an executor successfully calls `RunStore::begin_run`, persist the returned `RunId` on the attempt. If RunStore begin fails:

- keep the already-created job/attempt;
- execute or fail according to existing executor policy;
- record a structured persistence warning;
- never retry the process merely to obtain a RunId.

At completion, attempt state and RunStore completion should be coordinated best-effort without pretending they are one atomic datastore transaction. Record reconciliation diagnostics when one side succeeds and the other fails.

## 11. Cancellation model

### Queued jobs

A cancellation request transitions the job directly to `Cancelled` if no attempt has started.

### Running jobs

- persist `cancel_requested_at` and reason;
- notify the active executor through a `CancellationToken` owned by the scheduler/runtime;
- executor performs process-group or cooperative cancellation;
- terminal transition records whether cancellation was acknowledged or the process completed first.

### Race semantics

Define deterministic precedence:

- if completion is persisted before cancel request, job remains completed;
- if cancel request is persisted first but process exits successfully before termination, policy decides whether terminal state is completed-with-cancel-request or cancelled; choose one consistent representation and test it;
- stale workers may not overwrite a terminal state.

## 12. Retry and idempotency

### Idempotency classes

```rust
pub enum IdempotencyClass {
    ReadOnly,
    SafeRepeat,
    Conditional,
    NonIdempotent,
    Destructive,
}
```

Default retry policy:

- read-only: bounded retry eligible;
- safe repeat: bounded retry eligible for transient failure;
- conditional: requires executor-specific validation;
- non-idempotent/destructive: no automatic retry.

Persist the chosen class in the job record. Do not infer it again at restart from potentially changed code/config.

### Retry policy

```rust
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: BackoffPolicy,
    pub retryable_failures: Vec<FailureClass>,
}
```

Retries create a new attempt sequence. The job retains all prior attempt summaries.

## 13. Restart recovery

At daemon startup:

1. generate a new `DaemonGeneration`;
2. find attempts in `Created`, `Admitted`, or `Running` under other generations;
3. mark them `Interrupted` with a recovery reason;
4. update parent jobs;
5. requeue only when persisted idempotency and retry policy permit;
6. reconcile due schedules using occurrence uniqueness and missed-run policy;
7. emit recovery events and a summary in `SnapshotDaemon`.

Do not attempt to reconnect to arbitrary orphaned child processes in this phase. Process adoption requires a separate explicit design.

## 14. Protocol and event model

Add requests:

```rust
JobSubmit { spec: JobSubmitDto }
JobGet { job_id: String }
JobList { query: JobQueryDto }
JobCancel { job_id: String, reason: Option<String> }
JobRetry { job_id: String }
ScheduleCreate { spec: ScheduleCreateDto }
ScheduleList { workspace_id: Option<String> }
SchedulePause { schedule_id: String }
ScheduleResume { schedule_id: String }
ScheduleDelete { schedule_id: String }
```

Add events:

```text
JobCreated
JobQueued
JobBlocked
JobAttemptCreated
JobStarted
JobProgress
JobCancelRequested
JobCompleted
JobFailed
JobCancelled
JobTimedOut
JobInterrupted
JobRetried
ScheduleCreated
ScheduleOccurrenceQueued
ScheduleSkipped
SchedulePaused
ScheduleResumed
ScheduleDeleted
```

Events must include job ID, workspace ID, optional session/turn ID, attempt ID where applicable, and bounded summary fields. Large logs remain RunStore artifacts.

`SnapshotDaemon` should add queued/running/blocked counts and recovery summary. `SnapshotWorkspace` should add active and recent job summaries.

## 15. Migration from existing tasks

### Background tasks

Migrate persisted pending background rows into schedules where the interval can be parsed reliably. Record migration provenance. Rows with ambiguous prompt-encoded intervals should remain visible as migration failures rather than silently defaulting to one hour.

### Subagent task records

Keep existing task result/history tables for agent-facing semantics during migration, but add `job_id` linkage. The job controls execution lifecycle; task records remain the model-facing report/result abstraction until a later consolidation is justified.

### Protocol compatibility

Continue accepting `TaskList`, `TaskSchedule`, and `TaskDelete` temporarily, implemented as adapters over `ScheduleStore`/`JobStore`. Return stable opaque string IDs in new responses. If the old numeric protocol cannot represent them, add new variants and deprecate old calls rather than parsing UUIDs as numbers.

## 16. Testing plan

Use `--test-threads=1` for Rust tests.

### State-machine unit tests

- every valid transition;
- every invalid transition rejected;
- terminal-state monotonicity;
- concurrent completion/cancellation race;
- retry creates increasing attempt sequence;
- stale attempt cannot overwrite current attempt;
- unknown persisted enum value fails safely;
- deadline and not-before validation.

### Store tests

- create/get/list filters;
- transactional job/attempt transition;
- concurrent attempt creation yields one active attempt;
- cancellation while queued/running;
- dependency blocking/unblocking;
- schedule occurrence uniqueness;
- overlap policies;
- missed-run policies with bounded catch-up;
- generation recovery;
- in-memory and SQLite implementations pass a common conformance suite.

### Migration tests

- UUID background task imports correctly;
- malformed numeric/string IDs do not disappear;
- ambiguous duration is reported;
- repeated migration is idempotent;
- `TaskSchedule` compatibility creates one schedule and no `task_id: 0` side execution;
- `TaskDelete` maps through opaque IDs safely.

### Fault-injection tests

- crash after job creation before attempt;
- crash after attempt creation before process start;
- crash after process completion before RunStore completion;
- crash after RunStore completion before JobStore completion;
- restart recovers each state without double execution;
- schedule tick repeated after crash creates one occurrence.

### Integration tests

Use synthetic executors with marker files/counters to prove:

- one job dispatch per attempt;
- cancellation is delivered;
- retries create new attempts without erasing history;
- non-idempotent jobs are not auto-requeued;
- frontend disconnect does not cancel durable jobs;
- daemon restart emits interrupted/requeued events correctly.

## 17. Acceptance criteria

Phase 4 is complete when:

- all new scheduled/deferred work uses typed durable job and schedule IDs;
- job and attempt lifecycle transitions are authoritative and transactionally enforced;
- schedule occurrences are deduplicated across ticks and restarts;
- daemon generation recovery marks stale work interrupted and requeues only policy-eligible jobs;
- cancellation works in queued and compatibility-running states;
- retries preserve attempt history and respect persisted idempotency;
- RunStore IDs link to attempts without becoming queue authority;
- legacy `TaskSchedule` no longer creates UUID/numeric mismatches or `task_id: 0` duplicate behavior;
- protocol snapshots/events expose durable job and schedule state;
- state-machine, migration, race, and crash-recovery tests pass.

## 18. Handoff checklist

- [ ] Add typed job/schedule/attempt/generation IDs.
- [ ] Define job kinds, payloads, sources, priorities, resources, retries, and idempotency.
- [ ] Implement explicit job/attempt state machines.
- [ ] Add SQLite schema/migrations and indexes.
- [ ] Implement SQLite and in-memory JobStore conformance.
- [ ] Implement ScheduleStore and occurrence claiming.
- [ ] Add daemon generation recovery.
- [ ] Add cancellation and retry APIs.
- [ ] Add compatibility dispatcher before Phase 5 scheduler.
- [ ] Link attempts to RunStore records without double execution.
- [ ] Migrate/adapt background tasks and subagent tasks.
- [ ] Add protocol requests, responses, snapshots, and events.
- [ ] Add state-machine, race, migration, fault-injection, and restart tests.
- [ ] Update task, core, protocol, RunStore, and recovery documentation.
