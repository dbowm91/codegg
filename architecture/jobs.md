# Durable Jobs and Schedules (Phase 4)

Phase 4 introduced a durable execution-control domain: jobs, attempts,
schedules, dependencies, cancellation, retry policy, and restart recovery.
Phase 5 adds a daemon-owned submission boundary and makes the global
scheduler the admission authority for scheduler-backed work. Durable jobs
remain the queue/lifecycle authority; RunStore remains the artifact and
execution-provenance authority.

## Purpose

Provide a daemon- and workspace-scoped durable queue with transactional
lifecycle transitions so every scheduled or deferred piece of work
survives daemon restarts, retries, and cancellations without silent
data loss.

## Where It Lives

| Module | Purpose |
|--------|---------|
| `crates/codegg-core/src/jobs/mod.rs` | Typed IDs, domain types (`JobKind`, `JobSource`, `JobPriority`, `ResourceRequest`, `RetryPolicy`, `IdempotencyClass`, `JobState`, `AttemptState`, `JobPayload`, `NewJob`, `JobRecord`, `JobAttempt`, `CancelReason`, `CancelResult`, `CancelOutcome`, `RecoveryPolicy`, `RecoveryReport`, `AttemptCompletion`), `JobStore` trait, `JobStoreError`, `recover_at_startup` |
| `crates/codegg-core/src/jobs/store.rs` | `InMemoryJobStore`, `SqliteJobStore`, `JobStoreQuery`, `JobSummary`, `validate_state_transition`, `job_state_transitions`, `attempt_state_transitions`, `validate_attempt_transition` |
| `crates/codegg-core/src/jobs/schedule.rs` | `ScheduleState`, `ScheduleKind`, `OverlapPolicy`, `MissedRunPolicy`, `ScheduleRecord`, `ScheduleSummary`, `ScheduleTemplate`, `ScheduleQuery`, `OccurrenceStatus`, `ScheduleError`, `ScheduleStore` trait, `OccurrenceMaterializer` trait, `ClaimedOccurrence`, `MaterializerError`, `JobTemplate`, `compute_next_run`, `missed_run_targets` |
| `crates/codegg-core/src/jobs/schedule_store.rs` | `InMemoryScheduleStore`, `SqliteScheduleStore` |
| `src/scheduler/submission.rs` | `JobSubmissionService`, `SubmissionKey`, idempotent durable create/enqueue boundary |
| `src/managed_process.rs` | Canonical managed argv execution: environment policy, process groups, cancellation, bounded output, and provenance |
| `src/job_dispatcher.rs` | `JobDispatcher` trait, `SubAgentJobDispatcher`, `NullJobDispatcher` |
| `src/job_recovery.rs` | `recover_jobs_at_startup` helper, `RecoveryReportSummary` |
| `src/background_task_migration.rs` | `migrate_legacy_background_tasks` |

## How It Works

A caller submits a `NewJob` through `JobSubmissionService`, which
validates the payload, applies the canonical resource profile, creates
one durable `JobRecord` in `Queued` state, and wakes the scheduler.
The scheduler's fair queue picks it up on the next reconcile tick,
asks the admission controller for permits, and dispatches to a typed
executor. The executor creates an `AttemptRecord`, runs the work, and
persists exactly one terminal `AttemptCompletion` that atomically
advances the parent job to a terminal state.

## Key Types & APIs

### Typed Identifiers (`mod.rs:340–461`)

All identifiers are opaque UUID v4 strings wrapped in newtypes. They
are never parsed as integers.

```rust
pub struct JobId(String);        // line 342
pub struct AttemptId(String);    // line 369
pub struct ScheduleId(String);   // line 390
pub struct DependencyId(String); // line 411
pub struct DaemonGeneration(String); // line 435
```

`DaemonGeneration::new()` (line 438) produces a fresh UUID at each
daemon startup. An attempt is valid only while its stored generation
matches the active daemon generation.

### Job Kinds (`mod.rs:468`)

```rust
pub enum JobKind {
    AgentTurn, Subagent, Build, Test, Lint, Format, Shell,
    ManagedProcess, Python, GitRead, GitMutation, Research,
    Maintenance, ToolProgram,
    #[serde(other)] Unsupported,
}
```

Unknown future kinds deserialize into `Unsupported` for forward
compatibility. The daemon refuses to execute `Unsupported` jobs but
persists them so newer daemons can pick them up.

### Job Source and Priority (`mod.rs:548, 580`)

`JobSource` distinguishes `Interactive`, `Scheduled`, `AgentDelegated`,
`Retry`, `Maintenance`, and `Api` origins. `JobPriority` has five
buckets (`Urgent` through `Maintenance`) — persisted and validated but
not yet used for admission ordering.

### Job Payload (`mod.rs:941`)

Typed payload variants (`JobPayload`) carry enough data to rerun safely
without consulting stale client state. Secret material must never be
embedded — use credential references.

### JobState Machine (`store.rs:81`)

```
Scheduled → Queued | Cancelled | Expired
Queued    → Running | Cancelled | Expired | Blocked
Running   → Completed | Failed | Cancelled | TimedOut | Interrupted
Failed    → Queued (retry only)
TimedOut  → Queued (retry only)
Interrupted → Queued (recovery only)
Blocked   → Queued | Cancelled | Expired
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`,
`Expired`) never transition. Transitions go through `JobStore` methods
— no generic `set_state`.

### AttemptState Machine (`store.rs:99`)

```
Created|Admitted → Running | Failed | Cancelled | Interrupted
Running          → Completed | Failed | Cancelled | TimedOut | Interrupted
```

Terminal states never transition. `AttemptState::Interrupted` is used
during daemon generation recovery.

### JobStore Trait (`mod.rs:1244`)

16 methods on `JobStore`:

| Method | Purpose |
|--------|---------|
| `create_job(NewJob)` | Persist a new job, generate `JobId` |
| `get_job(JobId)` | Fetch by id |
| `list_jobs(JobStoreQuery)` | Filter by workspace/state/kind/session |
| `list_attempts(JobId)` | All attempts for a job, ordered by sequence |
| `enqueue(JobId)` | `Scheduled`/`Blocked` → `Queued` |
| `begin_attempt(JobId, DaemonGeneration)` | Create attempt, transition job to `Running` |
| `mark_attempt_running(AttemptId)` | `Created`/`Admitted` → `Running` |
| `set_attempt_executor(AttemptId, executor)` | Persist executor provenance before an attempt enters `Running` |
| `record_heartbeat(AttemptId, DateTime)` | Persist heartbeat timestamp |
| `finish_attempt(AttemptCompletion)` | Atomically persist attempt + job completion |
| `request_cancel(JobId, CancelReason)` | Apply or record cancellation request |
| `retry_job(JobId, DaemonGeneration, AttemptId)` | Create new attempt for retry |
| `block_job(JobId)` | Transition to `Blocked` state for dependency waiting |
| `recover_generation(DaemonGeneration, RecoveryPolicy)` | Mark stale attempts `Interrupted`, requeue eligible jobs |
| `find_descendants(JobId) → Vec<JobSummary>` | Find all non-terminal child jobs of a parent (M012) |
| `cancel_descendants(JobId, CancelReason) → usize` | Cancel all non-terminal descendants; returns count (M012) |

### ScheduleStore Trait (`schedule.rs:231`)

6 methods on `ScheduleStore`:

| Method | Purpose |
|--------|---------|
| `create(ScheduleTemplate)` | Persist a new schedule |
| `set_state(ScheduleId, ScheduleState)` | Pause/resume/archive |
| `delete(ScheduleId)` | Remove schedule |
| `get(ScheduleId)` | Fetch by id |
| `list(ScheduleQuery)` | Filter by workspace/state |
| `claim_due(DateTime, &dyn OccurrenceMaterializer)` | Atomically claim due occurrences, create jobs |

### `claim_due` Semantics (`schedule_store.rs:519`)

`claim_due` scans schedules where `next_run_at <= now` and state is
`Active`. For each due schedule, it:
1. Computes `missed_run_targets` based on `MissedRunPolicy`
2. Checks overlap policy against existing running/queued occurrences
3. Atomically inserts `schedule_occurrence` rows with
   `PRIMARY KEY(schedule_id, scheduled_for)` — duplicate claims fail
   with `DuplicateOccurrence`
4. Calls `OccurrenceMaterializer::materialize` to create the job
   from the `JobTemplate`
5. Updates `schedule.next_run_at` via `compute_next_run`

The `PRIMARY KEY(schedule_id, scheduled_for)` constraint prevents
double-firing after restart.

## Configuration Surface

Job records carry their configuration at creation time. Key defaults
are centralized in `ResourceRequest::for_kind` (`mod.rs:644`):

| Kind | CPU | Memory hint | Processes | IO | Network | Default conflict |
|---|---:|---:|---:|---:|---:|---|
| AgentTurn | 1 | 512 MB | 1 | 1 | 1 | — |
| Subagent | 1 | 512 MB | 1 | 1 | 1 | — |
| Research | 1 | 512 MB | 1 | 1 | 1 | — |
| Build | 3 | 2048 MB | 1 | 3 | 0 | exclusive:workspace-mutation |
| Lint | 1 | 768 MB | 1 | 1 | 0 | — |
| Format | 1 | 256 MB | 1 | 1 | 0 | exclusive:workspace-mutation |
| Test | 2 | 1024 MB | 1 | 2 | 0 | — |
| Shell/ManagedProcess | 1 | 256 MB | 1 | 1 | 0 | — |
| Python | 1 | 512 MB | 1 | 1 | 0 | — |
| GitRead | 1 | 128 MB | 1 | 1 | 0 | — |
| GitMutation | 1 | 256 MB | 1 | 1 | 0 | exclusive:worktree-mutation |
| Maintenance | 1 | 128 MB | 1 | 1 | 0 | — |
| ToolProgram | 1 | 512 MB | 1 | 1 | 0 | — |

`RecoveryPolicy` defaults (`mod.rs:1210`): requeue `ReadOnly` and
`SafeRepeat`; never auto-retry `Conditional`, `NonIdempotent`, or
`Destructive`.

## Invariants & Gotchas

### Recovery Contract (`mod.rs:1410`, `store.rs:694`)

At daemon startup (`recover_generation`):

1. All attempts in non-terminal states whose `daemon_generation` ≠
   the current generation are marked `Interrupted`
2. Parent jobs whose `RecoveryPolicy` permits requeue for the job's
   `IdempotencyClass` are transitioned to `Queued`; otherwise they
   are left in `Interrupted`
3. Default `RecoveryPolicy`: requeue `ReadOnly` and `SafeRepeat` jobs;
   never auto-retry `Conditional`, `NonIdempotent`, or `Destructive`
4. A `RecoveryReport` is returned summarizing interrupted attempts,
   requeued jobs, terminal jobs, and schedules reconciled

The idempotency class is persisted at creation time — it is never
re-inferred from code at restart.

### `recover_at_startup` integration (`scheduler.rs:1281`)

`JobScheduler::recover_at_startup` calls `JobStore::recover_generation`
once at daemon startup and wakes the scheduler with
`WokeReason::Reconciled` so the fair queue is rebuilt from durable
state. `src/job_recovery.rs` provides a thin
`recover_jobs_at_startup` wrapper used by `CoreDaemon::recover_jobs`.
The return type is `RecoveryReportSummary` (`src/job_recovery.rs:18`),
a compact summary suitable for operator-facing logging.

### InMemory vs SQLite recovery (`store.rs:694, 1632`)

Both implementations were historically divergent. The SQLite version
was canonical; the in-memory version had inverted comparison logic
(interrupting attempts whose generation *matched* the new generation
rather than those that differed). This was fixed so both implementations
agree: the `stale` parameter is the *new* daemon generation and
attempts whose stored generation differs are interrupted.

### Cancellation Race Semantics (`store.rs:557`)

Deterministic precedence rules (`request_cancel`):
- **Queued/Blocked/Scheduled job, no attempt started**: transition
  directly to `Cancelled`
- **Running job**: persist `cancel_requested_at` and reason; return
  `CancelOutcome::Requested` to caller; the active executor is
  notified via `CancellationToken`
- **Terminal job**: reject with `CancelOutcome::AlreadyTerminal`

If completion is persisted before cancel request, the job remains
completed. If cancel is persisted first but the process exits
successfully, the terminal state is `Completed` (not `Cancelled`).
Stale workers may not overwrite a terminal state.

### Descendant Cancellation (`scheduler.rs:929, 1258`)

When a parent attempt terminates (timeout, failure, cancel, interrupt),
the scheduler calls `cancel_descendants` to ensure children do not
outlive the parent. This runs both in the executor-completion task and
in `request_cancel`.

### RunStore Linkage (`mod.rs:1144`)

`JobAttempt.run_id: Option<RunId>` links an attempt to a RunStore
record. The two stores serve different purposes:
- **JobStore**: queue/lifecycle/control state
- **RunStore**: execution provenance, output, artifacts, changes,
  rerun descriptors

When an executor calls `RunStore::begin_run`, the returned `RunId` is
persisted on the attempt. If RunStore begin fails, the job/attempt
record is kept and a structured persistence warning is recorded — the
process is never retried solely to obtain a `RunId`.

### Boundary Enforcement

`crates/codegg-core/src/jobs/` is UI-, server-, plugin-, and auth-free:
it is the lowest level at which the daemon reasons about queued and
scheduled work. Run `scripts/check-core-boundary.sh` after touching
this module.

## Protocol Additions

Phase 4 adds:
- **13 `CoreRequest` variants**: `JobSubmit`, `JobGet`, `JobList`,
  `JobCancel`, `JobRetry`, `ScheduleCreate`, `ScheduleList`,
  `SchedulePause`, `ScheduleResume`, `ScheduleDelete`, `ScheduleGet`,
  `JobListAttempts`, `JobRecover`
- **13 `CoreResponse` variants**: matching responses for each request
- **18 `CoreEvent` variants**: `JobCreated`, `JobQueued`, `JobBlocked`,
  `JobAttemptCreated`, `JobStarted`, `JobProgress`,
  `JobCancelRequested`, `JobCompleted`, `JobFailed`, `JobCancelled`,
  `JobTimedOut`, `JobInterrupted`, `JobRetried`, `ScheduleCreated`,
  `ScheduleOccurrenceQueued`, `ScheduleSkipped`, `SchedulePaused`,
  `ScheduleResumed`, `ScheduleDeleted`
- **11 DTOs**: `JobSubmitDto`, `JobQueryDto`, `JobSummaryDto`,
  `JobRecordDto`, `JobAttemptDto`, `ScheduleCreateDto`,
  `ScheduleSummaryDto`, `ScheduleRecordDto`, `RecoveryReportDto`,
  `CancelResultDto`, `AttemptCompletionDto`
- **2 `ServerCapabilities` fields**: `durable_jobs`, `schedule_support`

Phase 5 adds `JobWait`, `SchedulerSnapshot`, the optional bounded
scheduler projection on `SnapshotDaemon`, and `submission_key` on
`JobSubmitDto`.

### Scheduler submission and execution

The active TUI task schedule/list/delete commands use the durable
`ScheduleCreate`, `ScheduleList`, and `ScheduleDelete` requests. They
carry the daemon-resolved workspace and session authority and use opaque
durable schedule IDs. The retained legacy `Task*` requests are an
explicit rejection boundary for old external clients; they do not start
or reach an independent background scheduler.

`JobSubmissionService` validates payload size and kind, resolves the
canonical workspace, applies the central resource profile and
exclusivity rules, then creates and enqueues the durable job as one
logical operation. A repeated `SubmissionKey` with the same request
fingerprint returns the original job; the in-memory idempotency index
is intentionally scoped to one daemon generation.

`ManagedArgvExecutor` is only an adapter. Non-shell argv work delegates
to `ManagedProcessService`, which supplies sanitized noninteractive
environment defaults, process-group/session cleanup,
timeout/cancellation handling, bounded output, and
`CODEGG_JOB_ID`/`CODEGG_ATTEMPT_ID` provenance. The service retains
independent stdout/stderr head-plus-tail buffers (256 KiB per stream
by default), drains both pipes concurrently, and reports truncation,
timeout, cancellation, output-limit termination, sandbox-helper
failure, and cleanup diagnostics distinctly. Explicit sandbox launches
use the installation-owned helper sibling, an owner-only system-temp
launch spec capped at 64 KiB, and a private versioned status pipe
capped at 16 KiB; target output is never a control channel. On Unix,
finite executions run in a child session; cancellation and timeout
send SIGTERM, wait a bounded grace period, then SIGKILL the verified
process group and reap the direct child. Other platforms retain
direct-child cleanup only. Shell, TestRunner, and SubAgentPool retain
their domain semantics behind typed executors; TestRunner is an
explicit lifecycle exemption because it streams parser input into
durable line-oriented test logs and owns stall-timeout semantics.

### Tool program child-job composition (M007)

Tool programs may submit scheduler-owned child jobs via
`submit_job(op, config)` in restricted-Python source. The
`ExecuteChildJob` IR opcode triggers
`BrokerCallback::submit_child_job()`, which maps `ChildJobOp` variants
(`Test`, `Build`, `Lint`, `Format`) to typed `JobKind`/`JobPayload`
combinations and submits through `JobSubmissionService`. Child jobs
inherit the parent program's workspace, authority, and deadlines. They
use `IdempotencyClass::SafeRepeat` and `RetryPolicy::no_retry()`. The
broker adapter waits for completion and returns a `ChildJobResult` with
per-op typed details. See `architecture/tool_programs.md` §M007 for
the full contract.

### Tool Program ownership closure (M011)

Tool Program jobs are admitted only through `JobSubmissionService` and
are validated before attempt creation. Their durable payload includes
the generated program identity, explicit retry invocation key,
authority digest, frozen tool manifest, and serialized execution
context. The scheduler supplies the outer deadline and a durable
heartbeat sink. A child job uses the parent program/sequence in its
submission key, inherits parent session and turn identity, and
receives the narrower of its requested and parent deadlines.

The Tool Program executor persists per-call reservations, completions,
and interpreter checkpoints before advancing. A typed result record is
written before the scheduler completion is projected, and terminal
background notifications are derived from that record. Missing or
divergent context, source, authority, or call identity fails closed.

## Testing

| Category | Coverage |
|----------|----------|
| State-machine unit tests | Every valid and invalid transition, terminal-state monotonicity, concurrent completion/cancellation races, retry sequence numbering |
| Store tests | Create/get/list filters, transactional job/attempt transitions, concurrent attempt creation, cancellation while queued/running, dependency blocking, schedule occurrence uniqueness, overlap/missed-run policies, generation recovery; in-memory and SQLite implementations share a conformance suite |
| Migration tests | UUID background task imports, malformed IDs reported, ambiguous durations warned, idempotent re-migration |
| Fault-injection tests | Crash after job creation before attempt, after attempt creation before process start, after process completion before RunStore completion, after RunStore completion before JobStore completion; restart recovery at each state |
| Integration tests | Synthetic executors with marker files: one dispatch per attempt, cancellation delivery, retry history preservation, non-idempotent job non-requeue, frontend disconnect does not cancel durable jobs |

42 integration tests in `tests/durable_jobs_phase4.rs`.

### Narrowest run commands

```bash
cargo test -p codegg-core jobs                              # unit tests
cargo test -p codegg-core schedule                          # schedule tests
cargo test --test durable_jobs_phase4                       # integration tests
python3 scripts/check-core-boundary.sh                      # boundary guard
```

## Related Docs

- `architecture/scheduler.md` — Phase 5 admission and dispatch
- `architecture/overview.md` — full module map
- `.opencode/skills/jobs/SKILL.md` — skill reference
- `architecture/tool_programs.md` — M007/M011 tool program contracts
