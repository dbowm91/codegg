---
name: scheduler
description: Global admission control scheduler for the single-daemon orchestration layer
version: 1.0.0
tags:
  - scheduler
  - admission-control
  - fair-queue
  - executors
  - phase5
---

# Scheduler Skill (Phase 5)

This skill covers Codegg's global admission control scheduler: the
single authority between the durable job store (Phase 4) and the typed
executors that actually run work.

## When to Load

Load this skill when working on:
- The fair queue (`src/scheduler/fair_queue.rs`) — priority classes, lanes, aging
- The admission controller (`src/scheduler/admission.rs`) — atomic permit acquisition
- Adding a new typed executor (`src/scheduler/executor.rs` + `src/scheduler/executors.rs`)
- Wiring a tool or TUI command through the scheduler instead of calling subsystems directly
- Snapshots / events emitted by the scheduler
- Rollout mode transitions (`observe` → `active` → `mandatory`)

## Module Map

| File | Key Types |
|------|-----------|
| `src/scheduler/mod.rs` | public surface, re-exports |
| `src/scheduler/types.rs` | `QueueEntry`, `PriorityClass`, `WorkspaceLane`, `LaneQueue`, `QueueInsertError`, `QueueRemovalReason` |
| `src/scheduler/fair_queue.rs` | `FairJobQueue`, `SelectionOutcome`, round-robin, anti-starvation, aging |
| `src/scheduler/config.rs` | `ResolvedSchedulerConfig`, `ResolvedSchedulerConfig::from_input`, validation, default budgets |
| `src/scheduler/permit.rs` | `PermitDimensions`, `ResourcePermitGuard`, `try_admit` vs `try_admit_arc`, `detach` semantics |
| `src/scheduler/admission.rs` | `AdmissionController`, `AdmissionDecision`, `BlockReason`, `UnschedulableReason` |
| `src/scheduler/executor.rs` | `JobExecutor` trait, `ExecutorRegistry`, `ExecutorKind`, `ExecutorCompletion`, `executor_kind_for_job` |
| `src/scheduler/executors.rs` | `TestJobExecutor`, `ManagedArgvExecutor`, `SubagentJobExecutor`, `register_default_executors` |
| `src/scheduler/submission.rs` | `JobSubmissionService`, `SubmissionKey`, idempotent durable submission |
| `src/managed_process.rs` | canonical managed argv process policy and provenance |
| `src/scheduler/events.rs` | `SchedulerEvent`, `WokeReason` |
| `src/scheduler/snapshot.rs` | `SchedulerSnapshot`, per-workspace summaries, executor health |
| `src/scheduler/scheduler.rs` | `JobScheduler`, main loop, `wake`, `reconcile`, `admit_and_dispatch_batch` |
| `tests/scheduler_phase5.rs` | integration tests (two-workspace fairness, admission budget, exclusivity keys, executor wiring) |
| `scripts/check_scheduler_bypass.py` | static lint guarding direct TestRunner, subagent, and legacy background dispatch bypasses |

## Quick Reference

### Submit a job through the scheduler

```rust
use codegg::scheduler::*;

let submission: Arc<JobSubmissionService> = ...; // from CoreRuntimeDeps
let submitted = submission.submit(key, new_job).await?;
// The facade creates/enqueues exactly once; the scheduler owns admission.
```

### Check admission without submitting

```rust
use codegg::scheduler::*;

let controller = scheduler.admission();
let dims = PermitDimensions {
    cpu_weight: 1,
    memory_mb_hint: 0,
    process_slots: 1,
    io_weight: 0,
    network_slots: 0,
    exclusivity_keys: vec!["exclusive:run-tests".into()],
};
match controller.try_admit_arc(&dims) {
    AdmissionDecision::Admitted(permit) => {
        // permit drops on scope exit; resources released automatically
    }
    AdmissionDecision::TemporarilyBlocked(BlockReason::KeyContended { .. }) => {
        // backoff and retry later
    }
    AdmissionDecision::Impossible(why) => {
        // request is structurally infeasible; log and drop
    }
}
```

### Register a typed executor

```rust
use codegg::scheduler::*;

let mut registry = ExecutorRegistry::new();
registry.register(Arc::new(MyExecutor))?;   // duplicate kinds rejected
scheduler.register_executor(my_exec).await?;
```

## Configuration

The on-disk schema lives in `crates/codegg-config/src/schema.rs`:

```toml
[scheduler]
enabled = true                       # default: true
rollout = "mandatory"               # mandatory (default); observe/active are staged values
reconcile_interval_ms = 1000         # wake tick interval

[scheduler.resources]
max_process_slots = 4                # global concurrent processes
max_cpu_weight = 8                   # soft cap
max_memory_mb_hint = 8192            # hint, not enforced
max_io_weight = 8                    # soft cap
max_network_slots = 4                # hard cap

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

`ResolvedSchedulerConfig::from_input` validates and freezes these defaults.
In daemon mode, `enabled = false` creates an explicit rejecting placeholder;
it does not restore unscheduled execution.

## Gotchas

1. **`WorkspaceId` is not `Ord`/`Default`.** The fair queue's inner
   lane map uses `HashMap`/`VecDeque` with deterministic string-key
   ordering for tests. New lane-based code must follow the same
   convention.
2. **`try_admit` vs `try_admit_arc`.** The non-Arc variant returns an
   orphan guard whose Drop does NOT release the controller. Use the
   Arc variant whenever the caller wants permit-on-drop semantics.
   `detach()` does NOT release either — the caller takes ownership.
3. **Duplicate executor registration is rejected.** Re-registering the
   same `ExecutorKind` returns `ExecutorRegistryError::Duplicate` so
   a misconfiguration surfaces immediately rather than silently
   overriding the existing executor.
4. **Mandatory means no fallback.** In daemon mode, scheduler-backed
   callers return a typed `SchedulerDisabled`/submission error when admission
   is unavailable. They do not execute directly.
5. **Managed argv is an adapter.** `ManagedArgvExecutor` delegates to
   `ManagedProcessService`, which owns environment sanitization, process-group
   cleanup, bounded output, timeout/cancellation, and job/attempt provenance.
6. **Compatibility paths are explicit.** Standalone/test-only adapters may
   use the legacy subagent or background machinery, but ordinary daemon TUI,
   tool, and protocol flows must submit through `JobSubmissionService`.
7. **`executor_kind_for_job` is the single source of truth** for
   `JobRecord → ExecutorKind` mapping. The bash-dispatch path uses
   `ExecutorKind::BashDispatch`; reads do NOT persist to RunStore
   (matching the native tool behaviour).
8. **Static guard exemption model.** `check_scheduler_bypass.py` no
   longer accepts whole-file exemptions for files that contain both
   scheduler-owned and compatibility paths (e.g. `src/agent/loop.rs`).
   Each compatibility call site must carry a per-line
   `// scheduler-audit: <reason>` annotation. The recognized reasons
   are `scheduler-owned`, `standalone-compat`, `definition-site`, and
   `test-only`.
9. **Execution ownership manifest.** `docs/execution-ownership.toml`
   is the machine-readable inventory of all production process-spawn
   sites. `scripts/check_execution_ownership.py` greps for spawn
   patterns and fails CI on unclassified sites. New production spawn
   sites must be declared in the manifest before they pass CI.

## Recent changes

- **Recovery at startup**: `JobScheduler::recover_at_startup` calls
  `JobStore::recover_generation` once at daemon startup and wakes the
  scheduler with `WokeReason::Reconciled`. Called from
  `CoreDaemon::recover_jobs`.
- **`WokeReason::Reconciled`** variant added to `src/scheduler/events.rs`.
- **`RecoveryReportSummary`** type added to `src/job_recovery.rs` for
  compact operator-facing recovery summaries.
- **`InMemoryJobStore::recover_generation` bug fixed**: the in-memory
  implementation had inverted comparison logic relative to the SQLite
  version. Both now agree that the `stale` parameter is the *new*
  generation and attempts whose stored generation differs are interrupted.
- **New integration tests** (closure pass):
  - `tests/scheduler_submission_idempotency.rs` (11 tests)
  - `tests/scheduler_permit_lifecycle.rs` (18 tests)
  - `tests/scheduler_cancellation.rs` (10 tests)
  - `tests/scheduler_restart_recovery.rs` (15 tests)
  - `tests/scheduler_contention.rs` (14 tests)
  - `tests/scheduler_authority_matrix.rs` (13 tests)
  - `tests/managed_process_descendants.rs` (5 tests)
  - `tests/scheduler_resource_profiles.rs` (8 tests)
  - `tests/scheduler_protocol_consistency.rs` (13 tests)
- **Static guard changes**:
  - `check_scheduler_bypass.py` now supports inline
    `// scheduler-audit: <reason>` annotations and has removed the
    whole-file exemption for `src/agent/loop.rs` in favor of per-line
    annotations.
  - `check_execution_ownership.py` enforces the machine-readable
    `docs/execution-ownership.toml` manifest.

## Migration Stakes

| Stage | Rollout | Production behaviour |
|-------|---------|---------------------|
| A | `observe` | Historical comparison mode; not the daemon default |
| C | `active` | Category-by-category routing validation |
| E | `mandatory` | Current daemon default; scheduler-backed submission is authoritative |
