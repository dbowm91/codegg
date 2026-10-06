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

When the daemon has an active goal for the submitted session, the host
submission boundary attaches the reserved `goal_id` label to supervised
`Test` and `Subagent` jobs before enqueueing them. Callers do not provide
this label, and model/tool payloads cannot override it. Child evidence jobs
use the active-goal snapshot read at their own host submission boundary;
there is no inherited ambient or later-resolved goal identity. The label is
durable job metadata used for evidence correlation only and grants no
execution authority.

## Key Types & APIs

### Typed Identifiers (`mod.rs:346-465`)

All identifiers are opaque UUID v4 strings wrapped in newtypes. They
are never parsed as integers.

```rust
pub struct JobId(String);        // line 346
pub struct AttemptId(String);    // line 373
pub struct ScheduleId(String);   // line 394
pub struct DependencyId(String); // line 415
pub struct DaemonGeneration(String); // line 439
```

`DaemonGeneration::new()` (line 442) produces a fresh UUID at each
daemon startup. An attempt is valid only while its stored generation
matches the active daemon generation.

### Job Kinds (`mod.rs:472`, 15 variants)

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

### Job Source and Priority (`mod.rs:619, 651`)

`JobSource` distinguishes `Interactive`, `Scheduled`, `AgentDelegated`,
`Retry`, `Maintenance`, and `Api` origins. `JobPriority` has five
buckets (`Urgent` through `Maintenance`) — persisted and validated but
not yet used for admission ordering.

### Job Payload (`mod.rs:1012`)

Typed payload variants (`JobPayload`) carry enough data to rerun safely
without consulting stale client state. Secret material must never be
embedded — use credential references.

### Execution Target (`mod.rs:517`, schema v66)

`ExecutionTarget` selects where an attempt runs: `Local` (default) or
`EggworkNode { node_id }` (one named Eggwork node). It is persisted on
`NewJob`/`JobRecord` (`target_kind`/`target_node_id` columns; historical
rows read back as `Local`). The job stores only the node id; endpoints
and key material stay in daemon configuration (`[eggwork.nodes]`).

`RemoteExecutionHandle` (schema version 1) persists the bound remote
identity on `JobAttempt` (`remote_handle_json`): node id, execution id,
generation, lease id. The Eggwork executor writes it before any
upload/execute side effect and reconciles it on restart instead of
resubmitting, so one CodeGG attempt maps to at most one accepted remote
execution. Bearer lease material lives only in this attempt-scoped row,
never in job payloads, progress text, or audit metadata. The M003
derived-transfer optimization cache is deliberately NOT persisted here:
it is an in-memory, node-scoped, bounded hint that starts cold after
restart and returns the executor to full materialization.

### Eggwork lease-fencing contract (corrective C001)

The persisted tuple must be byte-exact with the live Eggwork
`ExecutionHandle` accepted by the node. Eggwork stores
`lease_hash(lease_id)` at acceptance and fences control operations
against it: cancel/renew with a divergent lease token is rejected with
HTTP 403 `invalid_lease` (a wrong principal yields 403 `forbidden`).
Consequences for CodeGG:

- `derive_handle` mints exactly one lease token per fresh CodeGG attempt
  and copies it verbatim into both the live handle and the durable
  record (`RemoteExecutionHandle::from_parts`); no second token exists.
- `to_eggwork_handle` reconstructs the exact fenced tuple from durable
  state; retransmission of the same attempt reuses the persisted handle
  and never re-derives.
- A new CodeGG attempt derives a distinct execution id (deterministic
  digest over job id + attempt id) and therefore a fresh lease.
- Restart reconciliation observes the persisted execution: terminal
  snapshots map to completions, still-live executions are cancelled
  under their own persisted handle and the attempt reports
  `Interrupted`. Loss of the exact execution is a typed
  interruption/failure, never a local fallback.
- The scripted `ScriptedClient` seam optionally enforces the same
  fencing (`invalid_lease`); `tests/eggwork_remote_execution_live.rs`
  qualifies the production `NodeClientFactory` path against a real
  loopback node with mTLS (Linux).
- Execution policy is daemon-owned node configuration, not durable job
  intent. `none` + `unrestricted` preserves legacy behavior and is
  explicitly diagnosed. `required` filesystem isolation and `disabled`
  networking require fresh authenticated capability/status agreement
  before workspace upload; the resulting spec follows the configured
  policy with no silent downgrade. M002a qualifies `required` over the
  production mTLS path against the corrected Eggwork pin: workspace access
  succeeds, outside-workspace read/write is denied, and terminal evidence
  reports applied `workspace_rw`. Network-disabled execution remains
  unsupported and is refused before upload.

### JobState Machine (`mod.rs:895`)

`JobState` has 10 variants: `Scheduled`, `Queued`, `Running`, `Completed`,
`Failed`, `Cancelled`, `TimedOut`, `Interrupted`, `Blocked`, `Expired`.
The transition table lives in `job_state_transitions()` (`store.rs:86`) and
is enforced by `validate_state_transition()` (`store.rs:70`):

```
Scheduled → Queued | Cancelled | Expired
Queued    → Running | Cancelled | Expired | Blocked
Running   → Completed | Failed | Cancelled | TimedOut | Interrupted
Completed → — (terminal)
Failed    → Queued (retry only)
Cancelled → — (terminal)
TimedOut  → Queued (retry only)
Interrupted → Queued (recovery only)
Blocked   → Queued | Cancelled | Expired
Expired   → — (terminal)
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`,
`Expired`) never transition. Transitions go through `JobStore` methods
— no generic `set_state`.

### AttemptState Machine (`mod.rs:956`)

`AttemptState` has 8 variants: `Created`, `Admitted`, `Running`,
`Completed`, `Failed`, `Cancelled`, `TimedOut`, `Interrupted`.

```
Created|Admitted → Running | Failed | Cancelled | Interrupted
Running          → Completed | Failed | Cancelled | TimedOut | Interrupted
```

Terminal states (`Completed`, `Failed`, `Cancelled`, `TimedOut`,
`Interrupted`) never transition. `AttemptState::Interrupted` is used
during daemon generation recovery.

The transition table is authoritative in
`attempt_state_transitions()` (`store.rs:104`) and is enforced by
`validate_attempt_transition()` (`store.rs:119`):

| From | Allowed next states |
|------|---------------------|
| `Created` | `Admitted`, `Running`, `Failed`, `Cancelled`, `Interrupted` |
| `Admitted` | `Running`, `Failed`, `Cancelled`, `Interrupted` |
| `Running` | `Completed`, `Failed`, `Cancelled`, `TimedOut`, `Interrupted` |
| `Completed` | — (terminal) |
| `Failed` | — (terminal) |
| `Cancelled` | — (terminal) |
| `TimedOut` | — (terminal) |
| `Interrupted` | — (terminal) |

`Created → Admitted` is therefore a legal modeled transition, but it is not
a step the production scheduler performs durably:
`mark_attempt_running`'s SQL accepts `state IN ('created', 'admitted')`
(`store.rs:1815`), and the scheduler records admission in its in-memory
running-attempts map and calls `mark_attempt_running` directly
(`src/scheduler/scheduler.rs:1079`), so the durable attempt goes
`Created → Running`. `Admitted` remains decodable for external writers and
the projection replay path (`projection_replay/publication.rs:514`).

### JobStore Trait (`mod.rs:1668`)

24 methods on `JobStore`:

| Method | Purpose |
|--------|---------|
| `create_job(NewJob)` | Persist a new job, generate `JobId` |
| `set_job_labels(JobId, HashMap)` | Persist host-owned labels for a newly created job |
| `create_job_with_labels(NewJob, HashMap)` | Create a job and attach host-owned labels atomically |
| `get_job(JobId)` | Fetch by id |
| `get_jobs(&[JobId])` | Fetch multiple jobs in one store operation |
| `list_jobs(JobStoreQuery)` | Filter by workspace/state/kind/session |
| `count_jobs_by_kind_state(&[JobState])` | Count jobs grouped by kind for requested states |
| `list_job_records(JobStoreQuery)` | List full job records matching a query |
| `list_attempts(JobId)` | All attempts for a job, ordered by sequence |
| `enqueue(JobId)` | `Scheduled`/`Blocked` → `Queued` |
| `begin_attempt(JobId, DaemonGeneration)` | Create attempt, transition job to `Running` |
| `mark_attempt_running(AttemptId)` | `Created`/`Admitted` → `Running` |
| `set_attempt_executor(AttemptId, executor)` | Persist executor provenance before an attempt enters `Running` |
| `set_attempt_remote_handle(AttemptId, Option<RemoteExecutionHandle>)` | Persist the bound remote identity before remote side effects (Eggwork M001) |
| `set_attempt_source_subject_started(AttemptId, &ExecutionSubjectProvenance)` | Record the attempt-scoped execution source when it starts materializing |
| `seal_attempt_source_subject(AttemptId, &ExecutionSubjectProvenance)` | Seal the execution subject (v67 `source_subject_json`) |
| `record_heartbeat(AttemptId, DateTime)` | Persist heartbeat timestamp |
| `finish_attempt(AttemptCompletion)` | Atomically persist attempt + job completion |
| `request_cancel(JobId, CancelReason)` | Apply or record cancellation request |
| `retry_job(JobId, DaemonGeneration, AttemptId)` | Create new attempt for retry |
| `block_job(JobId)` | Transition to `Blocked` state for dependency waiting |
| `recover_generation(DaemonGeneration, RecoveryPolicy)` | Mark stale attempts `Interrupted`, requeue eligible jobs |
| `find_descendants(JobId) → Vec<JobSummary>` | Find all non-terminal child jobs of a parent (M012) |
| `cancel_descendants(JobId, CancelReason) → usize` | Cancel all non-terminal descendants; returns count (M012) |

### ScheduleStore Trait (`schedule.rs:235`)

6 methods on `ScheduleStore` (`create`, `set_state`, `delete`, `get`,
`list`, `claim_due`):

| Method | Purpose |
|--------|---------|
| `create(ScheduleTemplate)` | Persist a new schedule |
| `set_state(ScheduleId, ScheduleState)` | Pause/resume/archive |
| `delete(ScheduleId)` | Remove schedule |
| `get(ScheduleId)` | Fetch by id |
| `list(ScheduleQuery)` | Filter by workspace/state |
| `claim_due(DateTime, &dyn OccurrenceMaterializer)` | Atomically claim due occurrences, create jobs |

### `claim_due` Semantics (`schedule.rs:262`, impls `schedule_store.rs:243`, `:521`)

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
are centralized in `ResourceRequest::for_kind` (`mod.rs:713`):

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

`RecoveryPolicy` defaults (`mod.rs:1630`): requeue `ReadOnly` and
`SafeRepeat`; never auto-retry `Conditional`, `NonIdempotent`, or
`Destructive`.

## Invariants & Gotchas

### Recovery Contract (`mod.rs:1622`, `store.rs:915`)

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

### `recover_at_startup` integration (`src/scheduler/scheduler.rs:1764`)

`JobScheduler::recover_at_startup` calls `JobStore::recover_generation`
once at daemon startup and wakes the scheduler with
`WokeReason::Reconciled` so the fair queue is rebuilt from durable
state. `src/job_recovery.rs` provides a thin
`recover_jobs_at_startup` wrapper used by `CoreDaemon::recover_jobs`.
The return type is `RecoveryReportSummary` (`src/job_recovery.rs:19`),
a compact summary suitable for operator-facing logging.

### InMemory vs SQLite recovery (`store.rs:915, 2263`)

Both implementations were historically divergent. The SQLite version
was canonical; the in-memory version had inverted comparison logic
(interrupting attempts whose generation *matched* the new generation
rather than those that differed). This was fixed so both implementations
agree: the `stale` parameter is the *new* daemon generation and
attempts whose stored generation differs are interrupted.

### Cancellation Race Semantics (`store.rs:770`)

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

### Descendant Cancellation (`src/scheduler/scheduler.rs:1346, 1747`)

When a parent attempt terminates (timeout, failure, cancel, interrupt),
the scheduler calls `cancel_descendants` to ensure children do not
outlive the parent. This runs both in the executor-completion task and
in `request_cancel`.

### RunStore Linkage (`JobAttempt` at `mod.rs:1255`)

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
Synchronous callers use the bounded `run_blocking` adapter and streaming
foreground callers use `run_streaming`; neither adapter admits durable work.
See `architecture/process-tool-execution-ownership.md` for the complete
spawn-site disposition and protocol exceptions.

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

45 integration tests in `tests/durable_jobs_phase4.rs`.

### Narrowest run commands

```bash
cargo test -p codegg-core jobs                              # unit tests
cargo test -p codegg-core schedule                          # schedule tests
cargo test --test durable_jobs_phase4                       # integration tests
bash scripts/check-core-boundary.sh                        # boundary guard
```

## Related Docs

- `architecture/scheduler.md` — Phase 5 admission and dispatch
- `architecture/overview.md` — full module map
- `.opencode/skills/jobs/SKILL.md` — skill reference
- `architecture/tool_programs.md` — M007/M011 tool program contracts

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/jobs/{mod,store,schedule,schedule_store}.rs`,
`src/scheduler/scheduler.rs`, and `src/job_recovery.rs`. Corrected every
line ref: the prior set pointed at `jobs/store.rs` for both state enums,
which actually live in `jobs/mod.rs` (`JobState` `:895`, `AttemptState`
`:956`) — `store.rs` only holds the transition tables
(`job_state_transitions` `:86`, `attempt_state_transitions` `:104`);
`JobStore` trait `:1274` → `:1668`, `ScheduleStore` `:231` → `:235`,
`JobSource`/`JobPriority` `:554,586` → `:619,651`, `JobPayload` `:947` →
`:1012`, `ExecutionTarget` `:515` → `:517`, `for_kind` `:648` → `:713`,
`RecoveryPolicy` defaults `:1240` → `:1630`, `request_cancel` `:557` →
`:770`, `recover_generation` `:694` → `:915` (SQLite `:2263`). Corrected
the scheduler refs to the real path `src/scheduler/scheduler.rs`
(`recover_at_startup` `:1764`, `cancel_descendants` `:1346, 1747`) and
`RecoveryReportSummary` `job_recovery.rs:18` → `:19`. Corrected
`JobStore` method count 21 → 24 and added the two undocumented
`set_attempt_source_subject_started` / `seal_attempt_source_subject`
methods with their real signatures. Refuted the prior review's
"omits `Created → Admitted`" claim only in part: the transition table at
`store.rs:104` *does* allow `Created → Admitted`, but the production
scheduler never writes it durably (it calls `mark_attempt_running`
directly, `store.rs:1815` accepts `'created'`/`'admitted'`), so both facts
are now documented. Also added the 10-variant `JobState` and 8-variant
`AttemptState` counts with their authoritative transition tables, and the
15-variant `JobKind` count.
# Execution source provenance

`JobAttempt.source_subject` is the authority for the source state consumed by
one execution. It is a versioned bounded envelope; a retry captures a new
subject and never inherits the preceding attempt's value. `JobRecord` labels
are not provenance authority. The additive schema v67 column is nullable so
legacy attempts remain unavailable; migration never consults a workspace.

The scheduler captures S1 under the canonical workspace lease before executor
side effects. Local live-workspace execution seals S2 after executor cleanup
and before terminal persistence. Equal subjects are Stable; differences are
Drifted while the execution status remains truthful. Eggwork seals around its
materialized source snapshot before upload/submit; later local edits do not
rewrite that immutable input identity. M003 preserves this ordering
exactly: capture S1, build the immutable full snapshot, seal the full
manifest digest, then choose full/derived transport. A derived patch is
never persisted as the historical source identity. Capture failure, non-Git workspaces,
legacy NULLs, and unsupported remote boundaries remain unavailable.

## Source verification (second pass)

Verified 2026-10-06 against `crates/codegg-core/src/jobs/{mod,store,schedule,schedule_store}.rs`,
`src/scheduler/scheduler.rs`, `src/job_recovery.rs`,
`crates/codegg-core/src/projection_replay/publication.rs`, and
`tests/durable_jobs_phase4.rs`. Four residuals left by the first pass were
corrected: the `claim_due` trait ref `schedule.rs:243` → `:262` (line 243 is
the first *implementation* in `schedule_store.rs`, not the trait method, and a
second impl sits at `:521`); the in-memory-scheduler ref
`scheduler.rs:1071-1078` → `:1079` (the actual `mark_attempt_running` call,
preceded by an explanatory comment at `:1074`); and the integration test count
`42` → `45` `#[test]`/`#[tokio::test]` functions in `tests/durable_jobs_phase4.rs`.
The `scheduler.rs` refs in this file are correct and were **not** affected by
the `JobScheduler` `:99` → `:135` drift seen in `scheduler.md` — this file
cites `recover_at_startup` `:1764`, `cancel_descendants` `:1346` and `:1747`,
and `mark_attempt_running` `:1079`, all verified in place.
Independently confirmed the first pass's corrections: the five typed-ID
newtypes (`JobId` `:346`, `AttemptId` `:373`, `ScheduleId` `:394`,
`DependencyId` `:415`, `DaemonGeneration` `:439`, with `new()` at `:442`),
the 15-variant `JobKind` (`:472`), `JobSource`/`JobPriority` (`:619`, `:651`),
`JobPayload` (`:1012`), `ExecutionTarget` (`:517`), `JobState` (`:895`),
`AttemptState` (`:956`), `JobAttempt` (`:1255`), `RemoteExecutionHandle`
(`:1536`), `RecoveryPolicy` (`:1622`, `Default` at `:1630`), the `JobStore`
trait (`:1668`) with exactly **24** methods matching the documented table in
order, the `ScheduleStore` trait (`schedule.rs:235`) with exactly **6**
methods, the transition tables (`store.rs:86`, `:104`) and validators
(`:70`, `:119`), `request_cancel` (`:770`), `recover_generation` (`:915`,
SQLite `:2263`), `mark_attempt_running`'s `state IN ('created', 'admitted')`
SQL (`store.rs:1815`), the generation-mismatch recovery predicate
(`store.rs:2277`), and the `"admitted"` projection replay arm
(`projection_replay/publication.rs:514`).
