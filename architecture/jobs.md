# Durable Jobs and Schedules (Phase 4)

Phase 4 introduces a durable execution-control domain: jobs, attempts, schedules, dependencies, cancellation, retry policy, and restart recovery. The scheduler may initially execute through a compatibility path; the primary objective is to establish authoritative identity and lifecycle before adding global admission policy in Phase 5.

## Module Map

| Module | Purpose |
|--------|---------|
| `crates/codegg-core/src/jobs/mod.rs` | Typed IDs, domain types (`JobKind`, `JobSource`, `JobPriority`, `ResourceRequest`, `RetryPolicy`, `IdempotencyClass`, `JobState`, `AttemptState`, `JobPayload`, `NewJob`, `JobRecord`, `JobAttempt`, `CancelReason`, `CancelResult`, `CancelOutcome`, `RecoveryPolicy`, `RecoveryReport`, `AttemptCompletion`), `JobStore` trait, `JobStoreError`, `recover_at_startup` |
| `crates/codegg-core/src/jobs/schedule.rs` | `ScheduleState`, `ScheduleKind`, `OverlapPolicy`, `MissedRunPolicy`, `ScheduleRecord`, `ScheduleSummary`, `ScheduleTemplate`, `ScheduleQuery`, `OccurrenceStatus`, `ScheduleError`, `ScheduleStore` trait, `OccurrenceMaterializer` trait, `ClaimedOccurrence`, `MaterializerError`, `JobTemplate`, `compute_next_run`, `missed_run_targets` |
| `crates/codegg-core/src/jobs/schedule_store.rs` | `InMemoryScheduleStore`, `SqliteScheduleStore` |
| `crates/codegg-core/src/jobs/store.rs` | `InMemoryJobStore`, `SqliteJobStore`, `JobStoreQuery`, `JobSummary`, `validate_state_transition`, `job_state_transitions`, `attempt_state_transitions`, `validate_attempt_transition` |
| `src/job_dispatcher.rs` | `JobDispatcher` trait, `SubAgentJobDispatcher`, `NullJobDispatcher` |
| `src/job_recovery.rs` | `recover_jobs_at_startup` helper |
| `src/background_task_migration.rs` | `migrate_legacy_background_tasks` |

## Domain Model

### Typed Identifiers

All identifiers are opaque UUID v4 strings wrapped in newtypes. They are never parsed as integers.

```rust
pub struct JobId(String);
pub struct AttemptId(String);
pub struct ScheduleId(String);
pub struct DependencyId(String);
pub struct DaemonGeneration(String);
```

`DaemonGeneration::new()` produces a fresh UUID at each daemon startup. An attempt is valid only while its stored generation matches the active daemon generation.

### Job Kinds

```rust
pub enum JobKind {
    AgentTurn, Subagent, Build, Test, Lint, Format, Shell,
    ManagedProcess, Python, GitRead, GitMutation, Research,
    Maintenance,
    #[serde(other)] Unsupported,
}
```

Unknown future kinds deserialize into `Unsupported` for forward compatibility. The daemon refuses to execute `Unsupported` jobs but persists them so newer daemons can pick them up.

### Job Source and Priority

`JobSource` distinguishes `Interactive`, `Scheduled`, `AgentDelegated`, `Retry`, `Maintenance`, and `Api` origins. `JobPriority` has five buckets (`Urgent` through `Maintenance`) — persisted and validated in Phase 4 but not yet used for admission ordering.

### Job Payload

Typed payload variants (`JobPayload`) carry enough data to rerun safely without consulting stale client state. Secret material must never be embedded — use credential references.

### JobState Machine

```
Scheduled → Queued | Cancelled | Expired
Queued    → Running | Cancelled | Expired | Blocked
Running   → Completed | Failed | Cancelled | TimedOut | Interrupted
Failed    → Queued (retry only)
Interrupted → Queued (recovery only)
Blocked   → Queued | Cancelled | Expired
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`, `Expired`) never transition. Transitions go through `JobStore` methods — no generic `set_state`.

### AttemptState Machine

```
Created|Admitted → Running | Failed | Cancelled | Interrupted
Running          → Completed | Failed | Cancelled | TimedOut | Interrupted
```

Terminal states never transition. `AttemptState::Interrupted` is used during daemon generation recovery.

## Storage Schema

Migration v23 adds five tables. `STORAGE_LAYOUT_VERSION = 24`.

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
    attempt_count INTEGER NOT NULL DEFAULT 0,
    not_before INTEGER,
    deadline INTEGER,
    schedule_id TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_terminal INTEGER,
    cancel_requested_at INTEGER,
    cancel_reason TEXT,
    labels_json TEXT NOT NULL DEFAULT '{}'
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
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    UNIQUE(job_id, sequence)
);

CREATE TABLE job_dependency (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    depends_on_job_id TEXT NOT NULL,
    condition TEXT NOT NULL DEFAULT 'completed',
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
    time_updated INTEGER NOT NULL,
    labels_json TEXT NOT NULL DEFAULT '{}'
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

Indexes for queue scans by state/priority/not-before/workspace, attempts by job, and schedules by next-run time are added alongside the tables.

## JobStore Trait

12 methods on `JobStore` (`crates/codegg-core/src/jobs/mod.rs`):

| Method | Purpose |
|--------|---------|
| `create_job(NewJob)` | Persist a new job, generate `JobId` |
| `get_job(JobId)` | Fetch by id |
| `list_jobs(JobStoreQuery)` | Filter by workspace/state/kind/session |
| `list_attempts(JobId)` | All attempts for a job, ordered by sequence |
| `enqueue(JobId)` | `Scheduled`/`Blocked` → `Queued` |
| `begin_attempt(JobId, DaemonGeneration)` | Create attempt, transition job to `Running` |
| `mark_attempt_running(AttemptId)` | `Created`/`Admitted` → `Running` |
| `record_heartbeat(AttemptId, DateTime)` | Persist heartbeat timestamp |
| `finish_attempt(AttemptCompletion)` | Atomically persist attempt + job completion |
| `request_cancel(JobId, CancelReason)` | Apply or record cancellation request |
| `retry_job(JobId, DaemonGeneration, AttemptId)` | Create new attempt for retry |
| `recover_generation(DaemonGeneration, RecoveryPolicy)` | Mark stale attempts `Interrupted`, requeue eligible jobs |

## ScheduleStore Trait

6 methods on `ScheduleStore` (`crates/codegg-core/src/jobs/schedule.rs`):

| Method | Purpose |
|--------|---------|
| `create(ScheduleTemplate)` | Persist a new schedule |
| `set_state(ScheduleId, ScheduleState)` | Pause/resume/archive |
| `delete(ScheduleId)` | Remove schedule |
| `get(ScheduleId)` | Fetch by id |
| `list(ScheduleQuery)` | Filter by workspace/state |
| `claim_due(DateTime, &dyn OccurrenceMaterializer)` | Atomically claim due occurrences, create jobs |

### `claim_due` Semantics

`claim_due` scans schedules where `next_run_at <= now` and state is `Active`. For each due schedule, it:
1. Computes `missed_run_targets` based on `MissedRunPolicy`
2. Checks overlap policy against existing running/queued occurrences
3. Atomically inserts `schedule_occurrence` rows with `PRIMARY KEY(schedule_id, scheduled_for)` — duplicate claims fail with `DuplicateOccurrence`
4. Calls `OccurrenceMaterializer::materialize` to create the job from the `JobTemplate`
5. Updates `schedule.next_run_at` via `compute_next_run`

The `PRIMARY KEY(schedule_id, scheduled_for)` constraint prevents double-firing after restart.

## Recovery Contract

At daemon startup (`recover_generation`):

1. All attempts in non-terminal states whose `daemon_generation` ≠ the current generation are marked `Interrupted`
2. Parent jobs are updated: if `RecoveryPolicy` permits requeue for the job's `IdempotencyClass`, the job is transitioned to `Queued`; otherwise it is left in `Interrupted`
3. Default `RecoveryPolicy`: requeue `ReadOnly` and `SafeRepeat` jobs; never auto-retry `Conditional`, `NonIdempotent`, or `Destructive` jobs
4. A `RecoveryReport` is returned summarizing interrupted attempts, requeued jobs, terminal jobs, and schedules reconciled

The idempotency class is persisted at creation time — it is never re-inferred from code at restart.

## Cancellation Race Semantics

Deterministic precedence rules (`request_cancel`):
- **Queued job, no attempt started**: transition directly to `Cancelled`
- **Running job**: persist `cancel_requested_at` and reason; return `CancelOutcome::Requested` to caller; the active executor is notified via `CancellationToken`
- **Terminal job**: reject with `CancelOutcome::AlreadyTerminal`

If completion is persisted before cancel request, the job remains completed. If cancel is persisted first but the process exits successfully, the terminal state is `Completed` (not `Cancelled`). Stale workers may not overwrite a terminal state.

## JobDispatcher Integration

`JobDispatcher` (`src/job_dispatcher.rs`) bridges durable jobs to existing executors:

- `SubAgentJobDispatcher`: wraps `SubAgentPool`; dispatches `JobPayload::Subagent` to the subagent spawner
- `NullJobDispatcher`: no-op for tests

The dispatcher is invoked after `create_job` + `enqueue` + `begin_attempt`. The durable record must precede dispatch — no job is created after execution starts.

## RunStore Linkage

`JobAttempt.run_id: Option<RunId>` links an attempt to a RunStore record. The two stores serve different purposes:
- **JobStore**: queue/lifecycle/control state
- **RunStore**: execution provenance, output, artifacts, changes, rerun descriptors

When an executor calls `RunStore::begin_run`, the returned `RunId` is persisted on the attempt. If RunStore begin fails, the job/attempt record is kept and a structured persistence warning is recorded — the process is never retried solely to obtain a `RunId`.

## Background Task Migration

`migrate_legacy_background_tasks` (`src/background_task_migration.rs`) reads from the legacy `task` table, parses interval durations via `parse_duration`, creates `ScheduleRecord` entries in the `ScheduleStore`, and marks source tasks as `interrupted`.

- Malformed durations are surfaced as warnings and skip the task (never silently default)
- Migration is idempotent: `migration_marker` rows keyed by `legacy_background_task:<id>` prevent re-migration
- Repeated invocations are safe — already-migrated tasks are skipped

## Protocol Additions

Phase 4 adds:
- **13 `CoreRequest` variants**: `JobSubmit`, `JobGet`, `JobList`, `JobCancel`, `JobRetry`, `ScheduleCreate`, `ScheduleList`, `SchedulePause`, `ScheduleResume`, `ScheduleDelete`, `ScheduleGet`, `JobListAttempts`, `JobRecover`
- **13 `CoreResponse` variants**: matching responses for each request
- **18 `CoreEvent` variants**: `JobCreated`, `JobQueued`, `JobBlocked`, `JobAttemptCreated`, `JobStarted`, `JobProgress`, `JobCancelRequested`, `JobCompleted`, `JobFailed`, `JobCancelled`, `JobTimedOut`, `JobInterrupted`, `JobRetried`, `ScheduleCreated`, `ScheduleOccurrenceQueued`, `ScheduleSkipped`, `SchedulePaused`, `ScheduleResumed`, `ScheduleDeleted`
- **11 DTOs**: `JobSubmitDto`, `JobQueryDto`, `JobSummaryDto`, `JobRecordDto`, `JobAttemptDto`, `ScheduleCreateDto`, `ScheduleSummaryDto`, `ScheduleRecordDto`, `RecoveryReportDto`, `CancelResultDto`, `AttemptCompletionDto`
- **2 `ServerCapabilities` fields**: `durable_jobs`, `schedule_support`

## Testing Strategy

| Category | Coverage |
|----------|----------|
| State-machine unit tests | Every valid and invalid transition, terminal-state monotonicity, concurrent completion/cancellation races, retry sequence numbering |
| Store tests | Create/get/list filters, transactional job/attempt transitions, concurrent attempt creation, cancellation while queued/running, dependency blocking, schedule occurrence uniqueness, overlap/missed-run policies, generation recovery; in-memory and SQLite implementations share a conformance suite |
| Migration tests | UUID background task imports, malformed IDs reported, ambiguous durations warned, idempotent re-migration |
| Fault-injection tests | Crash after job creation before attempt, after attempt creation before process start, after process completion before RunStore completion, after RunStore completion before JobStore completion; restart recovery at each state |
| Integration tests | Synthetic executors with marker files: one dispatch per attempt, cancellation delivery, retry history preservation, non-idempotent job non-requeue, frontend disconnect does not cancel durable jobs |

42 integration tests in `tests/durable_jobs_phase4.rs`.

## Acceptance Criteria

Phase 4 is complete when (cross-reference plan §17):

- All new scheduled/deferred work uses typed durable job and schedule IDs
- Job and attempt lifecycle transitions are authoritative and transactionally enforced
- Schedule occurrences are deduplicated across ticks and restarts
- Daemon generation recovery marks stale work interrupted and requeues only policy-eligible jobs
- Cancellation works in queued and running states
- Retries preserve attempt history and respect persisted idempotency
- RunStore IDs link to attempts without becoming queue authority
- Legacy `TaskSchedule` no longer creates UUID/numeric mismatches or `task_id: 0` duplicate behavior
- Protocol snapshots/events expose durable job and schedule state
- State-machine, migration, race, and crash-recovery tests pass
