# Job Dispatcher

Bridges durable jobs to existing execution backends.

## Purpose

`src/job_dispatcher.rs` defines the `JobDispatcher` trait and implementations that take durable `JobRecord` jobs from the `JobStore` and dispatch them to the appropriate execution backend (subagent pool, managed process, test runner).

## Key Types

### JobDispatcher Trait

```rust
#[async_trait]
pub trait JobDispatcher: Send + Sync {
    async fn dispatch_created_job(&self, job: JobRecord) -> Result<(), DispatchError>;
}
```

### DispatchError

| Variant | Description |
|---------|-------------|
| `UnsupportedKind` | Job kind not supported by this dispatcher |
| `Subagent` | Error dispatching to subagent pool |
| `JobStore` | Error updating job store state |

### SubAgentJobDispatcher

Dispatches `JobPayload::Subagent` jobs to the `SubAgentPool` via a spawner channel. This is the primary dispatcher for agent-based work.

### NullJobDispatcher

No-op implementation that always returns `Ok(())`. Used in tests or when dispatch is handled externally.

## Dispatch Flow

```
JobScheduler
    │
    ▼
JobRecord (created in JobStore)
    │
    ▼
JobDispatcher::dispatch_created_job()
    │
    ├── JobPayload::Subagent → SubAgentJobDispatcher → SubAgentPool
    ├── JobPayload::ManagedArgv → ManagedProcessService
    └── JobPayload::Test → TestRunner
    │
    ▼
Job attempt created, execution begins
```

## Job Recovery

`src/job_recovery.rs` handles daemon-generation recovery:

- `recover_jobs_at_startup()` scans for stale job attempts
- Attempts whose `daemon_generation` != current generation are marked `Interrupted`
- Requeued if `RecoveryPolicy` permits based on `IdempotencyClass`

## See Also

- [Jobs](jobs.md) — Durable job domain model
- [Scheduler](scheduler.md) — Admission control and job lifecycle
- [Managed Process](managed_process.md) — ManagedArgv execution backend
- [Agent](agent.md) — SubAgent dispatch
