---
name: jobs
description: Durable jobs, attempts, schedules, recovery, and idempotency for the single-daemon orchestration layer
version: 1.0.0
tags:
  - jobs
  - schedules
  - recovery
  - idempotency
  - phase4
---

# Durable Jobs and Schedules Skill

This skill covers the Phase 4 durable execution-control domain in `crates/codegg-core/src/jobs/` and root-crate dispatchers.

## When to Load

Load this skill when working on:
- `JobStore` or `ScheduleStore` implementations (SQLite or in-memory)
- Adding new `JobKind` variants or `JobPayload` variants
- The `JobDispatcher` trait or `SubAgentJobDispatcher`
- Debugging job state transitions or recovery behavior
- Adding protocol events/requests for jobs or schedules
- Background task migration (`src/background_task_migration.rs`)
- Testing job lifecycle, cancellation, or retry logic

## Module Map

| File | Key Types |
|------|-----------|
| `crates/codegg-core/src/jobs/mod.rs` | `JobId`, `AttemptId`, `ScheduleId`, `DependencyId`, `DaemonGeneration`, `JobKind`, `JobSource`, `JobPriority`, `ResourceRequest`, `RetryPolicy`, `IdempotencyClass`, `JobState`, `AttemptState`, `JobPayload`, `NewJob`, `JobRecord`, `JobAttempt`, `CancelReason`, `CancelResult`, `CancelOutcome`, `RecoveryPolicy`, `RecoveryReport`, `AttemptCompletion`, `JobStore` trait, `JobStoreError` |
| `crates/codegg-core/src/jobs/schedule.rs` | `ScheduleState`, `ScheduleKind`, `OverlapPolicy`, `MissedRunPolicy`, `ScheduleRecord`, `ScheduleSummary`, `ScheduleTemplate`, `ScheduleQuery`, `OccurrenceStatus`, `ScheduleError`, `ScheduleStore` trait, `OccurrenceMaterializer`, `ClaimedOccurrence`, `JobTemplate`, `compute_next_run`, `missed_run_targets` |
| `crates/codegg-core/src/jobs/schedule_store.rs` | `InMemoryScheduleStore`, `SqliteScheduleStore` |
| `crates/codegg-core/src/jobs/store.rs` | `InMemoryJobStore`, `SqliteJobStore`, `JobStoreQuery`, `JobSummary`, `validate_state_transition`, `job_state_transitions`, `attempt_state_transitions`, `validate_attempt_transition` |
| `src/job_dispatcher.rs` | `JobDispatcher` trait, `SubAgentJobDispatcher`, `NullJobDispatcher` |
| `src/job_recovery.rs` | `recover_jobs_at_startup` |
| `src/background_task_migration.rs` | `migrate_legacy_background_tasks` |

## Quick Reference

### Create a Job

```rust
use codegg_core::jobs::*;

let store: Arc<dyn JobStore> = ...;
let job = store.create_job(NewJob {
    workspace_id: ws_id,
    session_id: None,
    turn_id: None,
    kind: JobKind::Subagent,
    source: JobSource::Interactive,
    priority: JobPriority::Normal,
    payload: JobPayload::Subagent { prompt, agent, ... },
    resource_request: ResourceRequest::default(),
    timeout: None,
    retry_policy: RetryPolicy::no_retry(),
    idempotency: IdempotencyClass::SafeRepeat,
    not_before: None,
    deadline: None,
    schedule_id: None,
    depends_on: vec![],
}).await?;
```

### Schedule Recurring Work

```rust
let schedule_store: Arc<dyn ScheduleStore> = ...;
let record = schedule_store.create(ScheduleTemplate {
    workspace_id: ws_id,
    session_id: None,
    kind: ScheduleKind::Interval { every: Duration::from_secs(3600), anchor: Utc::now() },
    job_template: JobTemplate::for_subagent(JobKind::Subagent, prompt, agent, session_id),
    overlap_policy: OverlapPolicy::SkipIfRunning,
    missed_run_policy: MissedRunPolicy::RunOnceNow,
    next_run_at: None,
    labels: HashMap::new(),
}).await?;
```

### Add a New JobKind

1. Add variant to `JobKind` in `crates/codegg-core/src/jobs/mod.rs`
2. Add match arm to `as_str()`, `from_str_lossy()`
3. Add `ResourceRequest::for_kind()` entry
4. Add `JobPayload` variant if the new kind needs a distinct payload
5. Add protocol request/response/event variants if TUI-facing
6. Update `architecture/jobs.md`

## State Machines

### JobState Transitions

```text
Scheduled  → Queued | Cancelled | Expired
Queued     → Running | Cancelled | Expired | Blocked
Running    → Completed | Failed | Cancelled | TimedOut | Interrupted
Failed     → Queued (retry only)
Interrupted → Queued (recovery only)
Blocked    → Queued | Cancelled | Expired
```

Terminal states: `Completed`, `Failed`, `Cancelled`, `TimedOut`, `Expired`.

### AttemptState Transitions

```text
Created|Admitted → Running | Failed | Cancelled | Interrupted
Running          → Completed | Failed | Cancelled | TimedOut | Interrupted
```

## Common Pitfalls

### ID Parsing

IDs are opaque UUIDs wrapped in newtypes. Never parse them as integers. Use `JobId::new_unchecked(id)` only for legacy compat — prefer `JobStore::create_job` for new jobs.

### Idempotency

The idempotency class is persisted at creation time. It is never re-inferred from code at restart. If you change defaults for a job kind, existing persisted jobs retain their original classification.

- `ReadOnly` and `SafeRepeat` are auto-retry eligible
- `Conditional`, `NonIdempotent`, `Destructive` are never auto-retried

### Race Conditions

- `claim_due` uses `PRIMARY KEY(schedule_id, scheduled_for)` to prevent double-firing after restart
- `finish_attempt` atomically updates both attempt and job state — no partial completion visible
- `request_cancel` is monotonic: once a job is terminal, cancellation is rejected
- Stale workers (wrong `DaemonGeneration`) cannot overwrite terminal states

### Recovery

`recover_generation` marks non-terminal attempts from a stale generation as `Interrupted`. The parent job is requeued only if `RecoveryPolicy` permits based on `IdempotencyClass`. Default policy requeues `ReadOnly` and `SafeRepeat` only.

### RunStore vs JobStore

JobStore owns queue/lifecycle. RunStore owns execution artifacts. They are NOT a single atomic transaction. Attempt completion and RunStore completion are coordinated best-effort. Never assume RunStore is the queue authority.

### Boundary Enforcement

`crates/codegg-core/src/jobs/` is UI-, server-, plugin-, and auth-free. Run `scripts/check-core-boundary.sh` after touching this module.

## Testing

```bash
cargo test --test durable_jobs_phase4          # 42 integration tests
cargo test -p codegg-core jobs                 # unit tests for state machines, store
cargo test -p codegg-core schedule             # unit tests for schedule logic
```

All tests use `current_thread` Tokio runtime. In-memory stores for state-machine tests; SQLite stores for integration tests.
