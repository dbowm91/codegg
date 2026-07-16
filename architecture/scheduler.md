# Scheduler (Phase 5)

Codegg's Phase 5 introduces a global admission-control scheduler that
sits between the durable job store (Phase 4) and the typed executors
that actually run work. The scheduler is the single authority for
admitting jobs, enforcing fairness, and dispatching them through the
canonical subsystems (TestRunner, ManagedArgv, SubAgentPool).

## Goals

1. **Single dispatch authority.** `tool::test`,
   `tool::bash::dispatch_to_test_runner`, `tui::commands::test`, and
   the subagent pool submit jobs through `JobScheduler::submit` instead
   of calling subsystems directly.
2. **Bounded concurrency.** Per-workspace, per-class, and global
   process-slot caps. Requests that exceed budget are returned
   immediately with `UnschedulableReason`; requests that are
   temporarily blocked return `BlockReason`.
3. **Fairness without starvation.** Interactive work gets an
   interactive floor (`max_high_priority_burst`); background work
   cannot starve interactive work. Aging promotes stale entries
   without mutating the persisted `JobPriority`.
4. **Bounded retries.** Each attempt owns a `ResourcePermitGuard`; the
   scheduler releases resources only when the executor signals
   completion (success, failure, cancellation, or timeout).

## Module layout

```
src/scheduler/
├── mod.rs             # public surface, re-exports
├── types.rs           # QueueEntry, PriorityClass, WorkspaceLane, LaneQueue, ...
├── fair_queue.rs      # FairJobQueue: 3-level hierarchy, round-robin, aging
├── config.rs          # ResolvedSchedulerConfig, validation, default budgets
├── permit.rs          # PermitDimensions, ResourcePermitGuard (Arc-attached)
├── admission.rs       # AdmissionController, BlockReason, UnschedulableReason
├── executor.rs        # JobExecutor trait, ExecutorRegistry, ExecutorKind
├── executors.rs       # TestJobExecutor, ManagedArgvExecutor, SubagentJobExecutor
├── events.rs          # SchedulerEvent enum, event sink wiring
├── snapshot.rs        # SchedulerSnapshot, per-workspace summaries
└── scheduler.rs       # JobScheduler main loop, wake, reconcile, dispatch
```

## Public API

```rust
use codegg::scheduler::*;

// Build a scheduler from the resolved config + durable stores.
let scheduler: Arc<JobScheduler> = JobScheduler::new(
    job_store,                  // Arc<dyn JobStore>
    workspace_services,         // Arc<WorkspaceServiceRegistry>
    resolved_config,            // ResolvedSchedulerConfig
    daemon_generation,          // DaemonGeneration
);

// Register typed executors.
let mut registry = ExecutorRegistry::new();
registry.register(Arc::new(TestJobExecutor::new(run_store, sink)))?;
scheduler.register_executors_blocking(executor_set).await?;

// Wire an event sink (mpsc::Sender<SchedulerEvent>).
scheduler.set_event_sink(tx).await;

// Submit a job.
scheduler.submit(job_record).await?;

// Cancel a running attempt.
scheduler.request_cancel(&attempt_id).await?;

// Snapshot for TUI / server introspection.
let snap = scheduler.snapshot().await;
```

## Lifecycle

1. **Construction.** Daemon builds the scheduler at startup using
   `CoreRuntimeDeps.scheduler: Option<Arc<JobScheduler>>`. When
   `[scheduler].enabled = true`, the daemon also spawns the main loop
   via `scheduler.spawn_run()` (returns `JoinHandle`). When disabled,
   the scheduler is built as a placeholder so snapshots and config
   introspection still work.
2. **Main loop.** `tokio::select!` over `notify.notified()`,
   `sleep(reconcile_interval)`, and `shutdown.cancelled()`. Each tick
   calls `reconcile()` and then `admit_and_dispatch_batch()`.
3. **Admission.** `AdmissionController::try_admit_arc` is the only path
   that reserves permits. `try_admit` (non-Arc) returns an orphan
   guard for callers that don't need controller-attached release.
4. **Dispatch.** The scheduler looks up `ExecutorKind` via
   `executor_kind_for_job` and calls the registered executor's
   `execute(JobExecutionContext)`. The executor owns the guard for
   its lifetime; resources are released on completion.
5. **Shutdown.** `scheduler.shutdown()` cancels the cancellation
   token; the main loop exits on the next select arm.

## Fairness

* **Priority classes**: `Urgent > Interactive > Normal > Background >
  Maintenance`. The class is derived from `JobPriority` and may be
  promoted by aging (without mutating persisted state).
* **Per-class round-robin**: when a class has multiple lanes (one per
  workspace), the cursor advances so each workspace gets a turn before
  any workspace gets two consecutive admissions.
* **Anti-starvation**: after `max_high_priority_burst` consecutive
  high-priority admissions, the queue forces a non-high-priority
  admission if any eligible entry exists.
* **Aging**: entries older than `aging_secs` are promoted through
  priority classes until they reach `Interactive` (where the
  anti-starvation cap applies).

## Admission control

`AdmissionController` reserves six dimensions per request:

| Dimension | Field | Notes |
|-----------|-------|-------|
| CPU weight | `cpu_weight: u32` | Soft reservation |
| Memory hint | `memory_mb_hint: u64` | Hint, not enforced by cgroups |
| Process slots | `process_slots: u16` | Hard cap on concurrent processes |
| IO weight | `io_weight: u32` | Soft reservation |
| Network slots | `network_slots: u16` | Hard cap on concurrent network ops |
| Exclusivity keys | `exclusivity_keys: Vec<String>` | Keys prefixed `exclusive:` block conflicting requests |

Impossible requests (e.g. `process_slots > max_process_slots`) return
`AdmissionDecision::Impossible(UnschedulableReason)`. Temporarily
saturated requests return
`AdmissionDecision::TemporarilyBlocked(BlockReason)`. Only successful
admissions return a `ResourcePermitGuard`.

## Executors

Each `JobExecutor` is responsible for:

1. **Validation.** Synchronous check that the `JobRecord` is well-formed
   and supported by the executor.
2. **Execution.** Async `execute(JobExecutionContext) -> ExecutorCompletion`.
   The executor owns the permit guard; resources are released when
   the executor returns (or drops the guard).
3. **RunStore linkage.** When the executor persists a run (e.g.
   `TestJobExecutor` calls `resolve_and_run_test` with a RunStore
   reference), it returns the `RunId` on the `ExecutorCompletion`.
   The scheduler writes it onto the `JobAttempt.run_id` field.

| Executor | JobKind | Backend |
|----------|---------|---------|
| `TestJobExecutor` | `JobKind::Test` | `test_runner::runner::resolve_and_run_test` |
| `ManagedArgvExecutor` | `JobKind::Build \| JobKind::Lint \| JobKind::Format` | `tokio::process::Command::new(argv[0]).args(&argv[1..])` (no shell) |
| `SubagentJobExecutor` | `JobKind::Subagent` | `crate::agent::worker::SubAgentPool::spawner().send(...)` |

## Events

`SchedulerEvent` is the bounded-delta surface the scheduler emits to
the daemon's event log:

| Variant | Emitted when |
|---------|--------------|
| `AdmissionBlocked { job_id, reason }` | Admission refused a permit |
| `JobAdmitted { job_id, attempt_id, run_id }` | An attempt was admitted and dispatched |
| `JobResourceReleased { job_id, attempt_id }` | Permit guard dropped (executor finished) |
| `SchedulerOverloaded { queued, cap }` | Queue rejected an insert |
| `SchedulerQueueChanged { ready_window, durable_queued }` | Queue size changed by a meaningful delta |
| `ExecutorUnavailable { executor, reason }` | An executor reported degraded/unavailable |
| `SchedulerQueueReconciled { ... }` | Reconciliation tick completed |
| `SchedulerWoke { reason }` | Wake arrived (debug-build only) |
| `Progress { job_id, message }` | Executor progress message |

Full state is exposed via `SchedulerSnapshot`, not events.

## Static guard

`scripts/check_scheduler_bypass.py` scans `src/` and `tests/` for
direct calls to `test_runner::resolve_and_run_test`,
`dispatch_to_test_runner`, and `SubAgentJobDispatcher` outside of:

* `src/scheduler/**` (the scheduler subsystem)
* `src/tool/bash.rs` (the migration bridge)
* `src/job_dispatcher.rs` (the legacy dispatcher site)
* `src/background_task_migration.rs` (Phase 4 migration)
* `tests/**` (test fixtures)
* `docs/**` and `architecture/**` (documentation)

Production paths that submit work MUST go through the scheduler.

## Testing

* **Unit tests:** `cargo test -p codegg --lib scheduler` (33 tests
  covering admission, fair queue, executor registry, scheduler
  lifecycle, and resource accounting).
* **Integration tests:** `cargo test --test scheduler_phase5` (11 tests
  covering two-workspace isolation, fairness, admission budget,
  exclusivity keys, executor wiring, and durable-jobstore
  compatibility).

## Migration plan (Stage A → E)

| Stage | Rollout mode | Tool-level change |
|-------|-------------|-------------------|
| A (current) | `observe` | Scheduler exists, snapshots emitted, tool paths unchanged |
| B | `observe` | Optional event-sink wiring, surface `[scheduler].enabled` toggle |
| C | `active` | `tool::test`, `tool::bash::dispatch_to_test_runner`, and TUI submit through scheduler |
| D | `active` | Subagent jobs route through scheduler (drop `SubAgentJobDispatcher`) |
| E | `mandatory` | Scheduler is the only path; legacy dispatcher is feature-gated |
