//! The daemon scheduler owns global job admission, fair queuing, and executor dispatch.
//! It is UI/server/plugin/auth-free; see `architecture/scheduler.md`.

pub mod admission;
pub mod ci_watch;
pub mod ci_watch_idle;
pub mod config;
pub mod eggwork;
pub mod eggwork_limits;
pub mod eggwork_telemetry;
pub mod events;
pub mod executor;
pub mod executors;
pub mod fair_queue;
pub mod idle_continuation;
pub mod job_complete_audit;
pub mod job_notifications;
pub mod permit;
pub mod schedule_driver;
pub mod schedule_materializer;
#[allow(clippy::module_inception)]
pub mod scheduler;
pub mod snapshot;
pub mod submission;
pub mod tool_program_executor;
pub mod tool_program_notifications;
pub mod types;

pub use admission::{
    AdmissionController, AdmissionDecision, AdmissionState, BlockReason, UnschedulableReason,
};
pub use config::{
    ResolvedSchedulerConfig, SchedulerConfig, SchedulerConfigError, SchedulerRolloutMode,
};
pub use eggwork::{
    to_eggwork_handle, BoxEventStream, EggworkClientFactory, EggworkExecutor,
    EggworkExecutorConfig, EggworkNodeClient, NodeClientFactory, ResolvedEggworkNode,
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
