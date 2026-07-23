//! Phase 5 of the single-daemon multi-project orchestration roadmap:
//! the global admission control scheduler.
//!
//! This module owns the queue and admission layer that sits between the
//! durable [`JobStore`] and the typed [`JobExecutor`](super::executor) registry.
//! It is the daemon's authoritative answer to "may this work begin now?"
//! and "which work goes next?"
//!
//! Three boundaries are enforced here:
//!
//! 1. **Queue identity.** The in-memory queue is rebuilt from
//!    `JobStore::list_jobs(Queued)` on every reconciliation pass. Job
//!    IDs are deduplicated, so an attempt to enqueue the same job
//!    twice (e.g. by a duplicate scheduler wake) does not produce a
//!    duplicate queue entry.
//! 2. **Fairness.** Selection is deterministic: priority class >
//!    workspace lane > FIFO. Aging elevates a job's effective priority
//!    without mutating the persisted original priority.
//! 3. **Resource admission.** Every admission decision is atomic: all
//!    requested dimensions and exclusivity keys are reserved together,
//!    or none of them. A blocked job never holds a partial reservation
//!    that could starve smaller eligible work.
//!
//! The scheduler runs in the root crate. It is intentionally
//! UI-/server-/plugin-/auth-free and is wired through
//! [`CoreRuntimeDeps`](crate::core::runtime_deps::CoreRuntimeDeps).
//! Concrete dispatch to executors is performed by the
//! [`JobScheduler`](super::scheduler::JobScheduler) main loop, never
//! by the queue or admission controller in isolation.

pub mod admission;
pub mod config;
pub mod events;
pub mod executor;
pub mod executors;
pub mod fair_queue;
pub mod permit;
#[allow(clippy::module_inception)]
pub mod scheduler;
pub mod snapshot;
pub mod submission;
pub mod tool_program_executor;
pub mod types;

pub use admission::{
    AdmissionController, AdmissionDecision, AdmissionState, BlockReason, UnschedulableReason,
};
pub use config::{
    ResolvedSchedulerConfig, SchedulerConfig, SchedulerConfigError, SchedulerRolloutMode,
};
pub use executor::{
    ExecutorAvailability, ExecutorCompletion, ExecutorHealth, ExecutorKind, ExecutorMetrics,
    ExecutorRegistry, ExecutorRegistryError, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor, JobProgressSink, NoopProgressSink,
};
pub use fair_queue::{FairJobQueue, LaneQueue, PriorityClass, SelectionOutcome, WorkspaceLane};
pub use permit::{PermitDimensions, ResourcePermit, ResourcePermitGuard};
pub use scheduler::{JobScheduler, SchedulerShutdownMode, SchedulerWake};
pub use snapshot::{
    AdmissionBlockSummary, ExecutorHealthSnapshot, OverloadSummary, PerWorkspaceSummary,
    ResourceSummary, SchedulerSnapshot, SnapshotCounts,
};
pub use submission::{JobSubmissionError, JobSubmissionService, SubmissionKey, SubmittedJob};
pub use types::{LaneInsert, QueueEntry, QueueInsertError, QueueRemovalReason};
