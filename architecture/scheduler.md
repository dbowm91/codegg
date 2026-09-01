# Scheduler-owned execution

Codegg's daemon is a scheduler-owned execution service. A daemon
operation that starts a process, runs a test/build/lint/format command,
dispatches a subagent, or consumes a constrained machine resource must
enter the durable job store before it runs.

## Purpose

Provide a single daemon-owned admission authority that guarantees
one-attempt-per-job, resource-bounded concurrency, workspace fairness,
and typed executor dispatch — no process or subagent bypasses the
scheduler.

## Where It Lives

| Module | Purpose |
|--------|---------|
| `src/scheduler/scheduler.rs` | `JobScheduler`: main loop, reconcile, admit, dispatch, cancel, snapshot, shutdown |
| `src/scheduler/submission.rs` | `JobSubmissionService`: durable create/enqueue boundary, payload validation, resource policy, idempotency |
| `src/scheduler/admission.rs` | `AdmissionController`: atomic multi-dimension permit acquisition |
| `src/scheduler/fair_queue.rs` | `FairJobQueue`: priority-class lanes, anti-starvation aging, workspace fairness |
| `src/scheduler/config.rs` | `ResolvedSchedulerConfig`: validated resource budgets, queue caps, fairness weights |
| `src/scheduler/executor.rs` | `JobExecutor` trait, `ExecutorRegistry`, `ExecutorKind`, `JobExecutionContext` |
| `src/scheduler/executors.rs` | `TestJobExecutor`, `ManagedArgvExecutor`, `SubagentJobExecutor` |
| `src/scheduler/events.rs` | `SchedulerEvent`, `WokeReason` |
| `src/scheduler/snapshot.rs` | `SchedulerSnapshot`, per-workspace summaries, executor health |
| `src/managed_process.rs` | Canonical managed argv process policy and provenance |

## How It Works

The production path is:

~~~text
frontend/tool/TUI request
  -> CoreDaemon / JobSubmissionService
  -> durable JobRecord
  -> JobScheduler fair queue
  -> JobAttempt + ResourcePermitGuard
  -> typed JobExecutor
  -> canonical domain service
  -> durable terminal attempt/job state + bounded completion
~~~

`--standalone` and the hidden stdio compatibility mode are explicit
non-daemon harnesses. They may retain local compatibility services, but
they do not provide the machine-wide singleton or global admission
guarantees.

Worktree lifecycle mutations use the same `exclusive:worktree-mutation`
resource class as typed Git mutations and the workspace repository lock. The
durable M003 `WorktreeService` is the lifecycle authority: it records reserve,
lease-generation, health, reconciliation, and cleanup state while the
scheduler remains the machine-capacity/admission authority. No worktree
operation bypasses the hardened Git environment policy.

### Main loop (`scheduler.rs:625`)

The scheduler loop runs until shutdown. Each iteration:
1. Wait for reconcile interval, a wake signal, or cancellation
2. `reconcile()` — pull durable `Queued` jobs into the fair queue,
   remove stale entries, update aging
3. `admit_and_dispatch_batch()` — try up to 4 candidates per tick

### Reconciliation (`scheduler.rs:435`)

Reconcile pulls durable queued jobs in bounded batches
(`config.queue.claim_batch`), deduplicates by `JobId`, and applies
aging. It also removes queue entries whose durable state is no longer
`Queued` (confirmed via direct store read so valid queued jobs beyond
the batch are never evicted).

### Admission (`scheduler.rs:680`)

For each candidate, the scheduler:
1. Pops from the fair queue
2. Fetches the durable record
3. Asks `AdmissionController::try_admit_arc` for permits
4. Acquires a workspace services lease
5. Resolves and validates the executor
6. Creates an `AttemptRecord`, marks it `Running`
7. Registers cancellation before spawning the executor task
8. Dispatches to the typed executor

### Key Types & APIs

#### JobScheduler (`scheduler.rs:99`)

```rust
pub struct JobScheduler { ... }
```

Core fields: `store`, `workspaces`, `executors`, `admission`, `queue`,
`running`, `completions`, `running_per_workspace`, `ready_counts`,
`running_total`, `admission_blocks`, `queue_overflows`,
`oldest_queued_age_secs`, `notify`, `shutdown`, `config`,
`daemon_generation`, `event_tx`.

Key methods:
- `spawn_run()` — spawn the main loop on the Tokio runtime
- `reconcile()` — rebuild in-memory queue from durable state
- `admit_and_dispatch_batch()` — try to admit and dispatch up to 4
  candidates
- `try_dispatch_next()` — pop one entry, admit, and dispatch
- `recover_at_startup(policy)` — call `recover_generation`, wake with
  `WokeReason::Reconciled`
- `request_cancel(job_id, reason)` — cancel queued or running jobs
- `snapshot()` — compose `SchedulerSnapshot` from queue, admission,
  running, and registry
- `shutdown(mode)` — drain, stop-accepting, or immediate-interrupt

#### JobSubmissionService (`submission.rs:83`)

```rust
pub struct JobSubmissionService { ... }
```

Single production facade for durable job creation and scheduler
enqueue. Validates workspace service lease, kind/payload coherence,
payload size, applies `ResourceRequest::for_kind` and exclusivity-key
normalization, creates exactly one durable `JobRecord`, and wakes the
scheduler.

`SubmissionKey` (line 31) is a caller-provided opaque retry identity.
Same key with same fingerprint → returns the original job. The
in-memory idempotency index is scoped to one daemon generation; the
durable job ID remains authoritative after restart.

#### AdmissionController (`admission.rs:27`)

Atomic admission decision. All requested dimensions and exclusivity
keys are reserved together, or none:

| Variant | Meaning |
|---------|---------|
| `Admitted(ResourcePermitGuard)` | All resources and keys reserved |
| `TemporarilyBlocked(BlockReason)` | Resources temporarily unavailable; advance to next candidate |
| `Impossible(UnschedulableReason)` | Request exceeds configured budget; never silently clamped |

`BlockReason` variants: `InsufficientProcessSlots`,
`InsufficientCpuWeight`, `InsufficientMemory`, `InsufficientIoWeight`,
`InsufficientNetworkSlots`, `KeyContended`, `QueueFull`.

`UnschedulableReason` variants: `ProcessSlotsExceedBudget`,
`CpuWeightExceedsBudget`, `MemoryExceedsBudget`,
`IoWeightExceedsBudget`, `NetworkSlotsExceedsBudget`.

#### FairJobQueue (`fair_queue.rs`)

Priority-class lanes with workspace sub-lanes. Anti-starvation aging
promotes long-waiting entries. Workspace fairness prevents one workspace
from starving unrelated work.

#### ExecutorRegistry (`executor.rs`)

Keyed by `ExecutorKind` (`Test`, `ManagedArgv`, `Subagent`,
`BashDispatch`, `Python`, `ToolProgram`, `Synthetic`). Duplicate kinds
are rejected. `for_job(&JobRecord)` resolves the best executor.

#### Execution Context (`executor.rs`)

Every executor context contains a typed `AttemptId`, the active daemon
generation, a workspace lease, a cancellation token, and a live
`ResourcePermitGuard`. Runtime validation rejects an empty identity or
a controller-less permit before executor code runs. The scheduler
records the executor name on the attempt before marking it running.

## Configuration Surface

### Durable delegated-agent execution

`JobPayload::SubagentRun` carries typed `AgentTaskId`/`AgentRunId` values and
the stable delegation key. `SubagentJobExecutor` attaches the scheduler
attempt, transitions the run through `Preparing` and `Running`, invokes the
existing child runtime, and records one bounded terminal outcome. The
`AgentRunStore` is also wired into scheduler cancellation and startup
generation recovery, including queued cancellation before an attempt exists.

The scheduler remains the only daemon machine-capacity authority. A scheduled
child can still be rejected by explicit semantic delegation policy, but it is
not rejected merely because the compatibility pool's local semaphore is full.
`JobRecord`/`JobAttempt` remain authoritative for queue state and retry/recovery;
the run record is the attributable delegated-agent ownership layer.

The on-disk schema lives in `crates/codegg-config/src/schema.rs`:

```toml
[scheduler]
enabled = true
rollout = "mandatory"
reconcile_interval_ms = 1000

[scheduler.resources]
max_process_slots = 4
max_cpu_weight = 8
max_memory_mb_hint = 8192
max_io_weight = 8
max_network_slots = 4

[scheduler.queue]
max_total = 256
max_per_workspace = 64
max_interactive_per_session = 8
claim_batch = 32

[scheduler.fairness]
interactive_weight = 8
normal_weight = 4
background_weight = 2
maintenance_weight = 1
max_high_priority_burst = 8
aging_secs = 300
```

`ResolvedSchedulerConfig::from_input` validates and freezes these
defaults. In daemon mode, `enabled = false` creates an explicit
rejecting placeholder; it does not restore unscheduled execution.

The scheduler default is enabled and mandatory. Explicit
`enabled = false` creates an introspection placeholder whose submission
API returns `SchedulerDisabled`; daemon tools do not fall back to direct
process creation. `observe` and `active` remain accepted configuration
labels for staged deployments and diagnostics, but they do not restore
bypass execution.

### Resource profiles (`mod.rs:644`)

Admission reserves soft CPU/memory/IO hints, process slots, network
slots, and typed exclusivity keys. Hints are accounting inputs, not
OS-enforced resource limits. Conservative defaults are centralized in
`ResourceRequest::for_kind`:

| Kind | CPU | Memory hint | Processes | IO | Network | Default conflict |
|---|---:|---:|---:|---:|---:|---|
| Test | 2 | 1024 MB | 1 | 2 | 0 | — |
| Build | 3 | 2048 MB | 1 | 3 | 0 | exclusive:workspace-mutation |
| Lint | 1 | 768 MB | 1 | 1 | 0 | — |
| Format | 1 | 256 MB | 1 | 1 | 0 | exclusive:workspace-mutation |
| Subagent | 1 | 512 MB | 1 | 1 | 1 | — |
| Git mutation | 1 | 256 MB | 1 | 1 | 0 | exclusive:worktree-mutation |
| AgentTurn | 1 | 512 MB | 1 | 1 | 1 | — |
| Research | 1 | 512 MB | 1 | 1 | 1 | — |
| Python | 1 | 512 MB | 1 | 1 | 0 | — |
| Shell | 1 | 256 MB | 1 | 1 | 0 | — |
| Maintenance | 1 | 128 MB | 1 | 1 | 0 | — |
| ToolProgram | 1 | 512 MB | 1 | 1 | 0 | — |

Impossible requests fail before executor invocation. Temporarily blocked
requests are requeued, and the bounded candidate window prevents one
blocked workspace from stopping unrelated work.

## Canonical process policy

`src/managed_process.rs` is the shared noninteractive argv service. It
owns sanitized inherited environment and noninteractive defaults,
job/attempt provenance variables, process session creation, descendant
cleanup, cancellation and timeout termination, drained
head/tail-bounded stdout/stderr, and typed exit/termination
classification.

`ManagedArgvExecutor` is only an adapter. It does not call
`tokio::process::Command` and never falls back to a shell after
admission or spawn failure. The explicit shell route is represented as
a `JobKind::Shell` payload and still uses the scheduler plus the
managed process service.

TestRunner remains the domain authority for framework discovery, stall
timeouts, reports, artifacts, and RunStore persistence. It is invoked
only by `TestJobExecutor`. TestTool, Bash test translation, and the
TUI `/test` command submit durable test jobs. TUI/server clients use
`WorkspaceRegister`, `JobSubmit`, and `JobWait` rather than
constructing TestRunner locally.

## Execution-surface inventory

| Production caller | Target kind | Executor/service | Status |
|---|---|---|---|
| `src/tool/test.rs` | Test | TestRunner | Scheduler submission |
| `src/tool/bash.rs` test translation | Test | TestRunner | Scheduler submission |
| `src/tool/bash.rs` build/lint/format/managed routes | matching kind | ManagedProcessService | Scheduler submission |
| `src/tool/bash.rs` explicit shell route | Shell | ManagedProcessService with sh -c payload | Scheduler submission |
| `src/tui/commands/test.rs` | Test | daemon protocol + TestRunner | Scheduler submission |
| server `CoreRequest::JobSubmit` | typed caller kind | daemon submission facade | Scheduler submission |
| scheduler subagent adapter | Subagent | SubAgentPool | Scheduler admission; waits for worker result |
| `src/job_dispatcher.rs` | Subagent | SubAgentPool | Definition retained; no daemon production wiring |
| durable `ScheduleStore` | Subagent | scheduler admission | Sole production scheduling owner |
| typed Git services / native Git read fallback | GitRead/mutation | egggit/Git service | Domain-specific compatibility path; migration remains |
| interactive terminal/editor/formatter helpers | explicit user/local action | local process API | Not daemon heavy-job submission yet |

The last three rows are deliberately documented rather than hidden
behind the static guard: they are compatibility or domain-specific
surfaces whose full scheduler submission requires additional
RunStore/PTY integration. They must not be described as covered by the
daemon invariant until migrated.

## Lifecycle and recovery

Scheduler dispatch creates an attempt, persists executor provenance,
marks it running, registers cancellation before spawn, and persists
exactly one terminal completion. Completion records are bounded in
memory for waiters; full artifacts remain in RunStore.

Cancellation removes queued entries and signals matching running
attempts. Managed-process cancellation kills the process session and
descendants before the permit is released. A completion that races
cancellation follows the durable store's terminal-state precedence.

At startup, `recover_at_startup` (`scheduler.rs:1281`) calls
`JobStore::recover_generation` once and wakes the scheduler with
`WokeReason::Reconciled` so the fair queue is rebuilt from durable
state. Queue reconciliation rebuilds the in-memory fair queue from
durable queued jobs. Schedule occurrence uniqueness is enforced by
`(schedule_id, scheduled_for)`; legacy background tasks are migrated
to `ScheduleStore`, while standalone compatibility task loops remain
explicitly outside daemon guarantees.

### Shutdown (`scheduler.rs:1102`)

Three shutdown modes:
- `DrainQueuedUntil(Duration)` — let admitted attempts finish, cancel
  queued
- `StopAcceptingAndCancelQueued` — cancel queued and running
- `ImmediateInterrupt` — cancel everything and abort

All modes wait up to `SHUTDOWN_CLEANUP_GRACE` (5s) for running
attempts to complete.

## Operator visibility

`SchedulerSnapshot` is bounded and includes queued/running counts,
per-workspace counts, configured resource budgets, current usage,
executor health, admission-block counters, queue overflow counters, and
oldest queued age. `SchedulerEvent` carries bounded deltas and IDs;
clients fetch full job and attempt records through protocol requests.
`JobWait` returns a bounded completion summary and optional RunStore
ID.

`WokeReason` variants (`events.rs:68`): `JobEnqueued`,
`ExecutorCompleted`, `CancellationRequested`, `ScheduleTick`,
`ScheduleClaimed`, `Manual`, `RetryRequested`, `Reconciled`.

## Static guards

Two static guards enforce the scheduler invariant at source level:

- `scripts/check_scheduler_bypass.py` rejects direct TestRunner calls
  outside scheduler executors and test fixtures, rejects production use
  of the old `dispatch_to_test_runner` name, and rejects direct
  subagent pool sends and background scheduler loop starts. Each bypass
  site must carry a `// scheduler-audit: <reason>` inline annotation
  (recognized reasons: `scheduler-owned`, `standalone-compat`,
  `definition-site`, `test-only`). Whole-file exemptions are restricted
  to subsystem definition files whose process-spawn entries are owned
  by the scheduler; `src/agent/loop.rs` no longer carries a blanket
  exemption — its standalone-compat fallback uses a per-line annotation.

- `scripts/check_execution_ownership.py` enforces the machine-readable
  manifest at `docs/execution-ownership.toml`. Every production source
  location in `src/` and `crates/` that can spawn a process, send work
  to a worker pool, start a test runner, start a background loop, invoke
  a domain-specific process service, create or enqueue a durable job, or
  acquire scheduler permits must be declared with an explicit owner
  classification. Owner classes: `scheduler`, `interactive`,
  `standalone_compat`, `definition_or_adapter`,
  `deferred_domain_executor`, `test_only`, `forbidden_bypass`.

Both guards run in CI and locally via:

~~~bash
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
~~~

## Closure evidence

The scheduler authority is validated by both static checks and a
comprehensive runtime test suite:

### Runtime proof

- **Resource admission**: permits are conserved across admit/drop
  (`tests/scheduler_permit_lifecycle.rs`)
- **Submission atomicity + idempotency**: one submission key produces
  one job; duplicate keys coalesce
  (`tests/scheduler_submission_idempotency.rs`)
- **Authority matrix**: one job produces one attempt and one executor
  entry (`tests/scheduler_authority_matrix.rs`)
- **Cancellation chain**: cancel signals propagate through process
  trees, terminal states are never overwritten
  (`tests/scheduler_cancellation.rs`)
- **Restart recovery**: fault injection at each durability boundary,
  stale attempts are interrupted, eligible jobs are requeued
  (`tests/scheduler_restart_recovery.rs`)
- **Multi-workspace contention**: fairness, exclusivity keys,
  starvation prevention (`tests/scheduler_contention.rs`)
- **Process-tree isolation**: SIGTERM → SIGKILL escalation,
  descendant cleanup (`tests/managed_process_descendants.rs`)
- **Resource profiles**: budget audit for all job kinds
  (`tests/scheduler_resource_profiles.rs`)
- **Protocol consistency**: snapshot, JobWait, JobList, error taxonomy
  (`tests/scheduler_protocol_consistency.rs`)
- **Existing coverage**: unit behaviour, two-workspace fairness,
  disabled-scheduler behaviour, managed-process timeout, bounded output,
  durable recovery (`tests/scheduler_phase5.rs`,
  `tests/durable_jobs_phase4.rs`)

### Startup recovery

`JobScheduler::recover_at_startup` is called once at daemon startup.
It delegates to `JobStore::recover_generation`, which marks stale
attempts as `Interrupted` and requeues eligible jobs based on the
persisted `RecoveryPolicy` and `IdempotencyClass`. The scheduler is
woken with `WokeReason::Reconciled` so the fair queue is rebuilt from
durable state before admitting new work.

### Invariant-by-invariant status

| Invariant | Enforcement |
|-----------|-------------|
| Heavy work routes through `JobSubmissionService` | `check_execution_ownership.py` + `check_scheduler_bypass.py` |
| One job → one attempt → one executor entry | `tests/scheduler_authority_matrix.rs` |
| Permit conservation across admit/drop | `tests/scheduler_permit_lifecycle.rs` |
| Terminal states never regress | `tests/scheduler_cancellation.rs` + `tests/scheduler_restart_recovery.rs` |
| Cancellation kills process trees | `tests/managed_process_descendants.rs` |
| Submission idempotency within daemon generation | `tests/scheduler_submission_idempotency.rs` |
| Multi-workspace fairness and starvation prevention | `tests/scheduler_contention.rs` |
| Resource budgets match declared profiles | `tests/scheduler_resource_profiles.rs` |
| Protocol snapshots consistent with queue state | `tests/scheduler_protocol_consistency.rs` |
| Stale attempts interrupted on restart | `tests/scheduler_restart_recovery.rs` |
| All process-spawn sites classified | `docs/execution-ownership.toml` |

### Narrowest run commands

~~~bash
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
cargo test -p codegg --lib scheduler
cargo test -p codegg --lib managed_process
cargo test --test scheduler_phase5
cargo test --test durable_jobs_phase4
cargo test --test scheduler_submission_idempotency
cargo test --test scheduler_permit_lifecycle
cargo test --test scheduler_cancellation
cargo test --test scheduler_restart_recovery
cargo test --test scheduler_contention
cargo test --test scheduler_authority_matrix
cargo test --test managed_process_descendants
cargo test --test scheduler_resource_profiles
cargo test --test scheduler_protocol_consistency
~~~

## Related Docs

- `architecture/jobs.md` — Phase 4 durable job/schedule domain
- `architecture/overview.md` — full module map
- `.opencode/skills/scheduler/SKILL.md` — skill reference
