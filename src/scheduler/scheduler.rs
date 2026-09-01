//! The global `JobScheduler`.
//!
//! This is the daemon-owned orchestrator. It owns the
//! `FairJobQueue`, `AdmissionController`, `ExecutorRegistry`, and
//! the running-attempts map. It loops, claiming newly-queued jobs
//! from `JobStore`, walking the fair queue, asking the admission
//! controller for permits, and dispatching to typed executors.
//!
//! Lifecycle invariants (enforced here, validated by tests):
//!
//! 1. **One executor invocation per attempt.** The scheduler records
//!    the attempt in `Admitted` before spawning the executor task.
//! 2. **Permits released only after executor stops.** The
//!    `ResourcePermitGuard` is held by the executor's `JobExecutionContext`;
//!    when the executor returns and the result is persisted, the
//!    guard is dropped and capacity is released.
//! 3. **No scheduler lock across executor await.** The scheduler
//!    drops the queue/admission locks before calling the executor.
//! 4. **No retry after executor start.** If the executor starts, the
//!    attempt is committed; the dispatcher does not fall through to
//!    another backend.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use codegg_core::agent_run::{AgentRunStore, AgentRunTerminalOutcome};
use codegg_core::jobs::{
    AttemptCompletion, AttemptId, AttemptState, CancelReason, DaemonGeneration, FailureClass,
    JobErrorRecord, JobId, JobRecord, JobState, JobStore, JobStoreError,
};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace_services::WorkspaceServiceRegistry;
use futures_util::FutureExt;
use tokio::sync::{mpsc, Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::scheduler::admission::{AdmissionController, AdmissionDecision};
use crate::scheduler::config::ResolvedSchedulerConfig;
use crate::scheduler::events::{SchedulerEvent, WokeReason};
use crate::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, JobExecutionContext,
    JobExecutor, JobProgressSink,
};
use crate::scheduler::fair_queue::FairJobQueue;
use crate::scheduler::permit::PermitDimensions;
use crate::scheduler::snapshot::ExecutorHealthSnapshot;
use crate::scheduler::snapshot::{SchedulerSnapshot, SnapshotCounts};
use crate::scheduler::types::QueueEntry;
use crate::scheduler::types::QueueRemovalReason;

const EXECUTOR_CLEANUP_GRACE: Duration = Duration::from_secs(5);
const SHUTDOWN_CLEANUP_GRACE: Duration = Duration::from_secs(5);

/// Wake signal sent to the scheduler's main loop. Cheap; the loop
/// uses a `Notify` to coalesce wakes and avoid spinning.
#[derive(Debug, Clone, Copy)]
pub struct SchedulerWake {
    pub reason: WokeReason,
    pub at: Instant,
}

impl SchedulerWake {
    pub fn new(reason: WokeReason) -> Self {
        Self {
            reason,
            at: Instant::now(),
        }
    }
}

/// Drain mode for `JobScheduler::shutdown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerShutdownMode {
    /// Allow queued jobs already admitted to finish; cancel the
    /// rest. Wait up to the supplied deadline for admitted attempts
    /// to complete.
    DrainQueuedUntil(Duration),
    /// Stop accepting new dispatches; cancel queued and running
    /// attempts.
    StopAcceptingAndCancelQueued,
    /// Cancel everything and abort the main loop.
    ImmediateInterrupt,
}

/// Per-attempt metadata held in the scheduler's `running_attempts`
/// map. Used for cancellation propagation and diagnostics; not
/// exposed through the protocol.
#[derive(Debug)]
pub struct RunningAttempt {
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub workspace_id: WorkspaceId,
    pub started_at: Instant,
    pub cancellation: CancellationToken,
}

struct CompletionCache {
    entries: HashMap<JobId, (u64, ExecutorCompletion)>,
    order: BTreeMap<u64, JobId>,
}

impl CompletionCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
        }
    }

    fn insert(&mut self, job_id: JobId, seq: u64, completion: ExecutorCompletion) {
        if let Some((old_seq, _)) = self.entries.get(&job_id) {
            self.order.remove(old_seq);
        }
        self.entries.insert(job_id.clone(), (seq, completion));
        self.order.insert(seq, job_id);

        while self.entries.len() > 1024 {
            if let Some((old_seq, oldest_job_id)) = self.order.pop_first() {
                if self
                    .entries
                    .get(&oldest_job_id)
                    .is_some_and(|(seq, _)| *seq == old_seq)
                {
                    self.entries.remove(&oldest_job_id);
                }
            }
        }
    }
}

pub struct JobScheduler {
    store: Arc<dyn JobStore>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    executors: Arc<AsyncMutex<crate::scheduler::executor::ExecutorRegistry>>,
    admission: Arc<AdmissionController>,
    queue: Arc<AsyncMutex<FairJobQueue>>,
    dispatch: AsyncMutex<()>,
    running: Arc<AsyncMutex<HashMap<AttemptId, RunningAttempt>>>,
    /// Join handles for executor tasks, drained during shutdown.
    running_tasks: Arc<AsyncMutex<HashMap<AttemptId, JoinHandle<()>>>>,
    /// Recent in-process completions let daemon clients receive the same
    /// bounded executor projection that completed the work. Durable job and
    /// attempt state remains authoritative across restart. Values carry the
    /// insertion sequence so eviction is oldest-first.
    completions: Arc<AsyncMutex<CompletionCache>>,
    /// Monotonic insertion counter for `completions`.
    completions_seq: Arc<AtomicU64>,
    /// Per-workspace running attempts (denormalized for snapshot).
    running_per_workspace: Arc<AsyncMutex<HashMap<WorkspaceId, usize>>>,
    /// Per-priority ready-window counts (denormalized).
    ready_counts: Arc<AsyncMutex<BTreeMap<String, usize>>>,
    /// Job-kind counts for queued and running jobs, refreshed during
    /// reconciliation so snapshots avoid a hot-path store query.
    job_kind_counts: Arc<AsyncMutex<BTreeMap<String, usize>>>,
    /// Total running count.
    running_total: Arc<AtomicU64>,
    /// Total admit blocks recorded.
    admission_blocks: Arc<AtomicU64>,
    /// Admit-block counters by reason label.
    admission_block_reasons: Arc<AsyncMutex<BTreeMap<String, u64>>>,
    /// Total admit impossible.
    admission_impossible: Arc<AtomicU64>,
    /// Total queue overflows recorded.
    queue_overflows: Arc<AtomicU64>,
    /// Oldest queued age in seconds (refreshed on wake).
    oldest_queued_age_secs: Arc<AsyncMutex<Option<u64>>>,
    notify: Arc<Notify>,
    shutdown: CancellationToken,
    config: Arc<ResolvedSchedulerConfig>,
    daemon_generation: DaemonGeneration,
    /// Optional channel for emitting events. `None` when no runtime consumer
    /// is installed, including standalone/test mode.
    event_tx: Arc<AsyncMutex<Option<mpsc::Sender<SchedulerEvent>>>>,
    /// Durable delegated-run ownership. The scheduler updates this store
    /// for queued cancellation and generation recovery so a run cannot
    /// remain live after its authoritative job has become terminal.
    agent_runs: Arc<AsyncMutex<Option<Arc<dyn AgentRunStore>>>>,
}

impl JobScheduler {
    /// Construct a scheduler. The config is validated by
    /// `ResolvedSchedulerConfig::validate` upstream.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Arc<dyn JobStore>,
        workspaces: Arc<WorkspaceServiceRegistry>,
        config: ResolvedSchedulerConfig,
        daemon_generation: DaemonGeneration,
    ) -> Arc<Self> {
        let admission = Arc::new(AdmissionController::new(config.clone()));
        let queue = Arc::new(AsyncMutex::new(FairJobQueue::new(config.clone())));
        let running = Arc::new(AsyncMutex::new(HashMap::new()));
        let running_tasks = Arc::new(AsyncMutex::new(HashMap::new()));
        let running_per_workspace = Arc::new(AsyncMutex::new(HashMap::new()));
        let ready_counts = Arc::new(AsyncMutex::new(BTreeMap::new()));
        let job_kind_counts = Arc::new(AsyncMutex::new(BTreeMap::new()));
        let oldest_queued_age_secs = Arc::new(AsyncMutex::new(None));
        let event_tx = Arc::new(AsyncMutex::new(None));
        Arc::new(Self {
            store,
            workspaces,
            executors: Arc::new(AsyncMutex::new(
                crate::scheduler::executor::ExecutorRegistry::new(),
            )),
            admission,
            queue,
            dispatch: AsyncMutex::new(()),
            running,
            running_tasks,
            completions: Arc::new(AsyncMutex::new(CompletionCache::new())),
            completions_seq: Arc::new(AtomicU64::new(0)),
            running_per_workspace,
            ready_counts,
            job_kind_counts,
            running_total: Arc::new(AtomicU64::new(0)),
            admission_blocks: Arc::new(AtomicU64::new(0)),
            admission_block_reasons: Arc::new(AsyncMutex::new(BTreeMap::new())),
            admission_impossible: Arc::new(AtomicU64::new(0)),
            queue_overflows: Arc::new(AtomicU64::new(0)),
            oldest_queued_age_secs,
            notify: Arc::new(Notify::new()),
            shutdown: CancellationToken::new(),
            config: Arc::new(config),
            daemon_generation,
            event_tx,
            agent_runs: Arc::new(AsyncMutex::new(None)),
        })
    }

    /// Install the daemon-owned delegated-run store after construction.
    /// Construction remains usable by focused scheduler tests that do not
    /// build the agent runtime.
    pub async fn configure_agent_run_store(&self, store: Arc<dyn AgentRunStore>) {
        *self.agent_runs.lock().await = Some(store);
    }

    pub fn configure_agent_run_store_sync(
        &self,
        store: Arc<dyn AgentRunStore>,
    ) -> Result<(), JobSchedulerError> {
        let mut guard = self
            .agent_runs
            .try_lock()
            .map_err(|_| JobSchedulerError::Internal("agent-run store is busy".into()))?;
        *guard = Some(store);
        Ok(())
    }

    pub fn config(&self) -> &ResolvedSchedulerConfig {
        &self.config
    }

    pub fn admission(&self) -> &Arc<AdmissionController> {
        &self.admission
    }

    pub fn store(&self) -> &Arc<dyn JobStore> {
        &self.store
    }

    pub fn workspaces(&self) -> &Arc<WorkspaceServiceRegistry> {
        &self.workspaces
    }

    pub fn daemon_generation(&self) -> &DaemonGeneration {
        &self.daemon_generation
    }

    /// Whether this scheduler can accept daemon-owned work. A disabled
    /// scheduler is an introspection placeholder only; it is never a
    /// license for callers to execute through a legacy bypass.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn is_mandatory(&self) -> bool {
        matches!(
            self.config.rollout,
            crate::scheduler::config::SchedulerRolloutMode::Mandatory
        )
    }

    /// Install an event sink. The scheduler forwards
    /// [`SchedulerEvent`]s to this sender for a runtime consumer.
    pub async fn set_event_sink(&self, tx: mpsc::Sender<SchedulerEvent>) {
        let mut g = self.event_tx.lock().await;
        *g = Some(tx);
    }

    /// Synchronous event-sink setter using `try_lock`. Returns `true`
    /// if the sink was installed successfully. Daemon construction
    /// uses this entry point because it is invoked from a sync
    /// `with_deps_and_identity`; runtime callers should use the async
    /// variant.
    pub fn set_event_sink_blocking(&self, tx: mpsc::Sender<SchedulerEvent>) -> bool {
        if let Ok(mut g) = self.event_tx.try_lock() {
            *g = Some(tx);
            true
        } else {
            false
        }
    }

    /// Register a typed executor. Duplicate kinds are rejected.
    pub async fn register_executor(
        &self,
        exec: Arc<dyn JobExecutor>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        let mut g = self.executors.lock().await;
        g.register(exec)
    }

    /// Bulk-register a set of executors from a synchronous construction
    /// helper. Returns the first duplicate error, if any.
    pub async fn register_executors_blocking(
        &self,
        execs: Vec<Arc<dyn JobExecutor>>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        let mut g = self.executors.lock().await;
        for exec in execs {
            g.register(exec)?;
        }
        Ok(())
    }

    /// Register one executor during synchronous daemon construction. The
    /// construction path runs before the scheduler loop is spawned, so a
    /// failed `try_lock` is a wiring error rather than a condition to defer.
    pub fn register_executor_sync(
        &self,
        exec: Arc<dyn JobExecutor>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        let mut g = self
            .executors
            .try_lock()
            .map_err(|_| crate::scheduler::executor::ExecutorRegistryError::Busy)?;
        g.register(exec)
    }

    /// Synchronous default-executor registration helper. Builds the
    /// test/managed-argv/subagent executors with no RunStore / event
    /// sink wiring (the daemon reconnects them at runtime) and
    /// installs them on the registry. Used by daemon construction
    /// when the scheduler's event loop is being spawned synchronously.
    pub fn register_default_executors_sync(
        &self,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        self.register_default_executors_sync_with_agent_runs(subagent_pool, None)
    }

    pub fn register_default_executors_sync_with_agent_runs(
        &self,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        agent_runs: Option<Arc<dyn codegg_core::agent_run::AgentRunStore>>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        self.register_default_executors_sync_with_agent_runs_and_control(
            subagent_pool,
            agent_runs,
            None,
        )
    }

    pub fn register_default_executors_sync_with_agent_runs_and_control(
        &self,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        agent_runs: Option<Arc<dyn codegg_core::agent_run::AgentRunStore>>,
        run_control: Option<Arc<crate::agent::run_control::RunControlService>>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        self.register_default_executors_sync_with_agent_runs_and_control_and_worktree(
            subagent_pool,
            agent_runs,
            run_control,
            None,
        )
    }

    pub fn register_default_executors_sync_with_agent_runs_and_control_and_worktree(
        &self,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        agent_runs: Option<Arc<dyn codegg_core::agent_run::AgentRunStore>>,
        run_control: Option<Arc<crate::agent::run_control::RunControlService>>,
        worktree_service: Option<Arc<codegg_core::worktree_service::WorktreeService>>,
    ) -> Result<(), crate::scheduler::executor::ExecutorRegistryError> {
        use crate::scheduler::executors::{
            ManagedArgvExecutor, SubagentJobExecutor, TestJobExecutor,
        };
        let mut registry = crate::scheduler::executor::ExecutorRegistry::new();
        registry.register(Arc::new(TestJobExecutor::new(None, None)))?;
        // One typed executor owns the Build/Lint/Format family. Registering
        // three instances under the same ExecutorKind silently discarded
        // the latter two in the old construction path.
        registry.register(Arc::new(ManagedArgvExecutor::new("managed_argv")))?;
        if let Some(pool) = subagent_pool {
            let executor = match agent_runs {
                Some(store) => match worktree_service {
                    Some(worktree_service) => {
                        SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
                            pool,
                            store,
                            run_control,
                            worktree_service,
                        )
                    }
                    None => SubagentJobExecutor::new_with_agent_runs_and_control(
                        pool,
                        store,
                        run_control,
                    ),
                },
                None => SubagentJobExecutor::new(pool),
            };
            registry.register(Arc::new(executor))?;
        }
        let kinds = registry.kinds();
        let execs: Vec<Arc<dyn JobExecutor>> =
            kinds.into_iter().filter_map(|k| registry.get(k)).collect();
        // Push them through the async bulk-register using try_lock;
        // construction-time callers are single-writer so this is safe.
        let mut g = self
            .executors
            .try_lock()
            .map_err(|_| crate::scheduler::executor::ExecutorRegistryError::Busy)?;
        for exec in execs {
            g.register(exec)?;
        }
        Ok(())
    }

    /// Snapshot of the available executor kinds.
    pub async fn executor_kinds(&self) -> Vec<ExecutorKind> {
        let g = self.executors.lock().await;
        g.kinds()
    }

    /// Wake the scheduler. The next reconciliation tick will run.
    pub fn wake(&self, reason: WokeReason) {
        self.notify.notify_one();
        // Best-effort event emission; the channel may be full or
        // closed in tests.
        let tx_clone = {
            let event_tx = self.event_tx.clone();
            let reason_clone = reason;
            tokio::spawn(async move {
                let result = std::panic::AssertUnwindSafe(async move {
                    let g = event_tx.lock().await;
                    if let Some(tx) = g.as_ref() {
                        if let Err(error) = tx
                            .send(SchedulerEvent::SchedulerWoke {
                                reason: reason_clone,
                            })
                            .await
                        {
                            tracing::debug!(?error, "scheduler wake event receiver is unavailable");
                        }
                    }
                })
                .catch_unwind()
                .await;
                if let Err(e) = result {
                    tracing::error!(panic = ?e, "scheduler wake event task panicked");
                }
            })
        };
        // Detach the spawned future; it self-completes.
        drop(tx_clone);
    }

    /// Submit a new job to the underlying `JobStore` and wake the
    /// scheduler. The job is created in `Queued` state and the
    /// scheduler picks it up on the next tick. If the queue is at
    /// capacity, returns a typed error.
    ///
    /// WARNING: this is NOT the admission boundary. It skips every
    /// `JobSubmissionService` check (payload validation, resource
    /// policy, leases, idempotency, size caps) and exists for scheduler
    /// subsystem internals and test fixtures. Production callers must
    /// submit through `JobSubmissionService::submit`
    /// (`scripts/check_scheduler_bypass.py` enforces this).
    pub async fn submit(
        &self,
        spec: codegg_core::jobs::NewJob,
    ) -> Result<JobRecord, JobSchedulerError> {
        if !self.is_enabled() {
            return Err(JobSchedulerError::SchedulerDisabled);
        }
        let job = self.store.create_job(spec).await?;
        self.enqueue_existing(job.clone()).await?;
        Ok(job)
    }

    /// Make an already-persisted job visible to the scheduler. This is the
    /// second half of [`JobSubmissionService`](super::submission::JobSubmissionService)'s
    /// create/enqueue operation and intentionally does not create another
    /// durable record.
    pub async fn enqueue_existing(&self, job: JobRecord) -> Result<(), JobSchedulerError> {
        if !self.is_enabled() {
            return Err(JobSchedulerError::SchedulerDisabled);
        }
        if !matches!(job.state, JobState::Queued) {
            return Err(JobSchedulerError::Internal(format!(
                "cannot enqueue job {} in state {:?}",
                job.job_id, job.state
            )));
        }
        self.wake(WokeReason::JobEnqueued);
        Ok(())
    }

    /// Wait for a scheduler-owned completion without executing the job in a
    /// caller task. The timeout is a client wait bound, not the job's own
    /// process timeout.
    pub async fn wait_for_completion(
        &self,
        job_id: &JobId,
        wait_timeout: Duration,
    ) -> Result<ExecutorCompletion, JobSchedulerError> {
        let deadline = Instant::now() + wait_timeout;
        loop {
            let notified = self.notify.notified();
            if let Some(completion) = self
                .completions
                .lock()
                .await
                .entries
                .get(job_id)
                .map(|(_, c)| c)
                .cloned()
            {
                return Ok(completion);
            }
            if let Some(job) = self.store.get_job(job_id).await? {
                if job.state.is_terminal() {
                    let status = match job.state {
                        JobState::Completed => ExecutorStatus::Completed,
                        JobState::Cancelled => ExecutorStatus::Cancelled,
                        JobState::TimedOut => ExecutorStatus::TimedOut,
                        JobState::Interrupted => ExecutorStatus::Interrupted,
                        _ => ExecutorStatus::Failed,
                    };
                    return Ok(ExecutorCompletion {
                        status,
                        summary: job
                            .cancel_reason
                            .clone()
                            .unwrap_or_else(|| format!("job finished in {:?}", job.state)),
                        run_id: None,
                        metrics: ExecutorMetrics::default(),
                    });
                }
            } else {
                return Err(JobSchedulerError::Internal(format!(
                    "job {} disappeared while waiting",
                    job_id
                )));
            }
            if Instant::now() >= deadline {
                return Err(JobSchedulerError::Internal(format!(
                    "timed out waiting for job {}",
                    job_id
                )));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return Err(JobSchedulerError::Internal(format!(
                    "timed out waiting for job {}",
                    job_id
                )));
            }
        }
    }

    /// Reconcile the in-memory queue against the durable store. Pull
    /// queued jobs in bounded batches, deduplicate by `JobId`, and
    /// apply aging. Idempotent.
    pub async fn reconcile(&self) -> Result<ReconcileReport, JobSchedulerError> {
        let mut added = 0;
        let mut removed = 0;
        let mut duplicates = 0;
        let limit = self.config.queue.claim_batch;

        // Pull durable queued jobs (state = Queued). Filter out
        // dependent / blocked.
        let query = codegg_core::jobs::store::JobStoreQuery {
            states: vec![JobState::Queued],
            workspace_id: None,
            kinds: vec![],
            limit: Some(limit as u32),
            session_id: None,
        };
        let durable = self.store.list_job_records(query).await?;
        let durable_ids: std::collections::HashSet<JobId> =
            durable.iter().map(|j| j.job_id.clone()).collect();

        let dependency_ids: std::collections::HashSet<JobId> = durable
            .iter()
            .flat_map(|job| job.depends_on.iter().cloned())
            .collect();
        let dependency_states: std::collections::HashMap<JobId, JobState> = self
            .store
            .get_jobs(&dependency_ids.into_iter().collect::<Vec<_>>())
            .await?
            .into_iter()
            .map(|job| (job.job_id, job.state))
            .collect();

        // 1. Insert durable jobs not already in the queue.
        for job in durable {
            // Skip if not eligible (not_before / deadline / dependencies).
            if !job_eligible(&job, &dependency_states) {
                continue;
            }
            let entry = QueueEntry::from_job(&job);
            let mut q = self.queue.lock().await;
            match q.insert(entry) {
                Ok(Some(_)) => duplicates += 1,
                Ok(None) => added += 1,
                Err(_) => {
                    // Overflow; record and emit event.
                    self.queue_overflows.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        // 2. Remove queue entries whose durable state is no longer
        // Queued (cancelled, completed, etc). Entries missing from the
        // bounded page above are confirmed via a direct store read so
        // valid queued jobs beyond `claim_batch` are never evicted.
        let candidates: Vec<JobId> = {
            let q = self.queue.lock().await;
            let mut v = Vec::new();
            for queue in q.lanes().values() {
                for lane in queue.lanes.values() {
                    for e in &lane.entries {
                        if !durable_ids.contains(&e.job_id) {
                            v.push(e.job_id.clone());
                        }
                    }
                }
            }
            v
        };
        let queued_ids: std::collections::HashSet<JobId> = self
            .store
            .get_jobs(&candidates)
            .await?
            .into_iter()
            .filter(|job| matches!(job.state, JobState::Queued))
            .map(|job| job.job_id)
            .collect();
        let to_remove: Vec<JobId> = candidates
            .into_iter()
            .filter(|job_id| !queued_ids.contains(job_id))
            .collect();
        {
            let mut q = self.queue.lock().await;
            for id in to_remove {
                if q.remove(&id, QueueRemovalReason::Dropped).is_some() {
                    removed += 1;
                }
            }
        }

        // 3. Update oldest-queued-age and ready-window counts without
        // holding the queue lock while acquiring the statistics locks.
        let queue_snapshot = {
            let mut q = self.queue.lock().await;
            let now = Utc::now();
            q.recompute_aging(now);
            q.lanes()
                .iter()
                .map(|(class, lane_q)| {
                    let oldest = lane_q
                        .lanes
                        .values()
                        .flat_map(|lane| lane.entries.iter())
                        .map(|e| {
                            now.signed_duration_since(e.submitted_at)
                                .num_seconds()
                                .max(0) as u64
                        })
                        .max();
                    (format!("{:?}", class), lane_q.total(), oldest)
                })
                .collect::<Vec<_>>()
        };
        let oldest = queue_snapshot.iter().filter_map(|(_, _, age)| *age).max();
        let ready_counts = queue_snapshot
            .into_iter()
            .filter_map(|(class, total, _)| (total > 0).then_some((class, total)))
            .collect::<BTreeMap<_, _>>();
        *self.oldest_queued_age_secs.lock().await = oldest;
        *self.ready_counts.lock().await = ready_counts;

        // 4. Refresh the job-kind distribution used by snapshots.
        match self
            .store
            .count_jobs_by_kind_state(&[JobState::Queued, JobState::Running])
            .await
        {
            Ok(counts) => *self.job_kind_counts.lock().await = counts,
            Err(e) => {
                tracing::debug!("reconcile: job-kind listing failed: {e}");
            }
        }

        // Periodic source store orphan cleanup: collect active Python job
        // digests and clean up stale source files across workspaces.
        if let Err(e) = self.cleanup_python_source_orphans().await {
            tracing::debug!("python source orphan cleanup failed: {e}");
        }

        Ok(ReconcileReport {
            added,
            removed,
            duplicates,
        })
    }

    /// Spawn the main loop on the current Tokio runtime and return the
    /// `JoinHandle`. The handle is held by the daemon as a tokio task;
    /// `shutdown()` triggers a clean exit.
    pub fn spawn_run(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = Arc::clone(self);
        tokio::spawn(async move { me.run().await })
    }

    /// Clean up orphaned Python source files across all workspaces that
    /// have active or queued Python jobs. Queries the job store for
    /// non-terminal Python jobs, collects their source hashes, and removes
    /// any source files not referenced by those jobs.
    async fn cleanup_python_source_orphans(&self) -> Result<(), JobSchedulerError> {
        use codegg_core::jobs::JobKind;

        // Query for active Python jobs (all states except terminal)
        let query = codegg_core::jobs::store::JobStoreQuery {
            states: vec![
                JobState::Scheduled,
                JobState::Queued,
                JobState::Running,
                JobState::Blocked,
            ],
            workspace_id: None,
            kinds: vec![JobKind::Python],
            limit: Some(1000),
            session_id: None,
        };
        let jobs = self.store.list_job_records(query).await?;

        // Collect active source hashes grouped by workspace root
        let mut workspace_digests: std::collections::HashMap<
            WorkspaceId,
            std::collections::HashSet<String>,
        > = std::collections::HashMap::new();
        for job in &jobs {
            if let codegg_core::jobs::JobPayload::Python {
                source_hash: Some(hash),
                ..
            } = &job.payload
            {
                workspace_digests
                    .entry(job.workspace_id.clone())
                    .or_default()
                    .insert(hash.clone());
            }
        }

        // Clean up each workspace's source store
        for (workspace_id, digests) in &workspace_digests {
            let Some(workspace) = self.workspaces.workspaces().resolve(workspace_id).await else {
                tracing::debug!(%workspace_id, "skipping orphan cleanup for unknown workspace");
                continue;
            };
            let active: Vec<&str> = digests.iter().map(|s| s.as_str()).collect();
            let removed = crate::python_script::source_store::PythonSourceStore::cleanup_stale(
                &workspace.canonical_root,
                &active,
            );
            if removed > 0 {
                tracing::info!(
                    %workspace_id,
                    root = %workspace.canonical_root.display(),
                    "python source orphan cleanup: removed {removed} files"
                );
            }
        }

        Ok(())
    }

    /// Main loop. Runs until `shutdown` is cancelled. On each
    /// iteration: reconcile, then admit up to `max_high_priority_burst`
    /// candidates, dispatch them, and wait for the next wake.
    pub async fn run(self: Arc<Self>) {
        let reconcile_interval = Duration::from_millis(self.config.reconcile_interval_ms);
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    break;
                }
                _ = tokio::time::sleep(reconcile_interval) => {
                    // Tick
                }
                _ = self.notify.notified() => {
                    // Woken
                }
            }
            if let Err(e) = self.clone().reconcile().await {
                tracing::warn!("scheduler reconcile failed: {e}");
            }
            self.clone().admit_and_dispatch_batch().await;
        }
    }

    /// Try to admit and dispatch up to a small batch of candidates.
    /// The batch size is bounded by remaining capacity so we never
    /// over-admit.
    pub async fn admit_and_dispatch_batch(self: Arc<Self>) -> usize {
        let _dispatch = self.dispatch.lock().await;
        if self.shutdown.is_cancelled() {
            return 0;
        }
        let mut dispatched = 0;
        let max_batch = 4usize; // bounded; the loop above will run again
                                // Inspect a small candidate window instead of treating a
                                // temporarily blocked head job as global back-pressure. This lets
                                // an unrelated workspace make progress while the contended job is
                                // requeued for the next reconciliation tick.
        for _ in 0..(max_batch * 2) {
            if dispatched >= max_batch || self.shutdown.is_cancelled() {
                break;
            }
            match self.clone().try_dispatch_next().await {
                Ok(true) => dispatched += 1,
                Ok(false) => continue,
                Err(e) => {
                    tracing::debug!("scheduler dispatch error: {e}");
                    break;
                }
            }
        }
        dispatched
    }

    /// Pop one entry, ask the admission controller for a permit, and
    /// dispatch to the typed executor. Returns `Ok(true)` if one
    /// was dispatched, `Ok(false)` if the queue is empty or the
    /// admission controller is full.
    async fn try_dispatch_next(self: Arc<Self>) -> Result<bool, JobSchedulerError> {
        // Pop one entry from the queue.
        let entry = {
            let mut q = self.queue.lock().await;
            match q.select_next() {
                Some(outcome) => outcome.entry,
                None => return Ok(false),
            }
        };
        // Fetch the durable record.
        let Some(job) = self.store.get_job(&entry.job_id).await? else {
            return Ok(false);
        };
        if !matches!(job.state, JobState::Queued) {
            return Ok(false);
        }

        // Build permit dimensions from job.resource_request.
        let dims = build_permit_dimensions(&job);

        // Atomic admission.
        let permit = match Arc::clone(&self.admission).try_admit_arc(&dims) {
            AdmissionDecision::Admitted(p) => p,
            AdmissionDecision::TemporarilyBlocked(reason) => {
                self.admission_blocks.fetch_add(1, Ordering::SeqCst);
                {
                    let label = format!("{:?}", reason);
                    let mut reasons = self.admission_block_reasons.lock().await;
                    *reasons.entry(label).or_insert(0) += 1;
                }
                self.emit_event(SchedulerEvent::AdmissionBlocked {
                    job_id: job.job_id.to_string(),
                    reason,
                })
                .await;
                // Re-insert the entry so we try again next tick.
                let mut q = self.queue.lock().await;
                if let Err(error) = q.insert(entry) {
                    tracing::error!(
                        job_id = %job.job_id,
                        %error,
                        "scheduler failed to requeue an admission-blocked job"
                    );
                    return Err(JobSchedulerError::Internal(format!(
                        "failed to requeue admission-blocked job {}: {error}",
                        job.job_id
                    )));
                }
                return Ok(false);
            }
            AdmissionDecision::Impossible(reason) => {
                self.admission_impossible.fetch_add(1, Ordering::SeqCst);
                // Mark the job Failed in durable state.
                self.mark_unschedulable(&job, &format!("{:?}", reason))
                    .await?;
                return Ok(false);
            }
        };

        // Acquire workspace services lease.
        let lease = match self.workspaces.acquire(&job.workspace_id).await {
            Ok(l) => l,
            Err(e) => {
                // Release permit, requeue, log.
                drop(permit);
                tracing::warn!(
                    "scheduler: workspace lease failed for {}: {e}",
                    job.workspace_id
                );
                let mut q = self.queue.lock().await;
                if let Err(error) = q.insert(entry) {
                    tracing::error!(
                        job_id = %job.job_id,
                        %error,
                        "scheduler failed to requeue a job after workspace lease failure"
                    );
                    return Err(JobSchedulerError::Internal(format!(
                        "failed to requeue job {} after workspace lease failure: {error}",
                        job.job_id
                    )));
                }
                return Ok(false);
            }
        };

        // Resolve and validate the executor before creating an attempt. A
        // missing or mismatched executor must not create a Running attempt
        // that can never be completed.
        let exec = {
            let g = self.executors.lock().await;
            g.for_job(&job)
        };
        let Some(exec) = exec else {
            drop(permit);
            drop(lease);
            self.mark_unschedulable(&job, "no executor registered for job kind")
                .await?;
            return Ok(true);
        };
        if !exec.supports(job.kind) {
            drop(permit);
            drop(lease);
            self.mark_unschedulable(&job, "registered executor does not support job kind")
                .await?;
            return Ok(true);
        }
        if let Err(error) = exec.validate(&job) {
            drop(permit);
            drop(lease);
            self.mark_unschedulable(&job, &error.to_string()).await?;
            return Ok(true);
        }

        // Begin the attempt: this creates a fresh attempt in
        // Created state.
        let attempt = self
            .store
            .begin_attempt(&job.job_id, &self.daemon_generation)
            .await?;

        // Spawn the executor task. Permit is moved into the task.
        let cancellation = CancellationToken::new();
        let ctx = JobExecutionContext {
            job: job.clone(),
            attempt_id: attempt.attempt_id.clone(),
            daemon_generation: self.daemon_generation.clone(),
            workspace_id: job.workspace_id.clone(),
            workspace_root: lease.path_policy().canonical_root.clone(),
            cancellation: cancellation.clone(),
            progress: Arc::new(DurableProgressSink {
                store: self.store.clone(),
                attempt_id: attempt.attempt_id.clone(),
            }),
            resources: permit,
        };

        let execution_timeout = job
            .deadline
            .map(|deadline| (deadline - Utc::now()).to_std().unwrap_or(Duration::ZERO))
            .or(job.timeout);

        self.store
            .set_attempt_executor(&attempt.attempt_id, exec.kind().as_str())
            .await?;

        // Persist `Admitted` on the attempt and update job state to
        // Running before the executor starts. This is the
        // persisted-attempt-precedes-executor-invocation invariant.
        // Note: `mark_attempt_running` transitions Created -> Running.
        // For the Admitted transition we record via the in-memory
        // state and a label; the durable attempt begins in Created
        // and moves to Running here. The semantic of "admitted" is
        // held by the scheduler's running map below.
        self.store.mark_attempt_running(&attempt.attempt_id).await?;

        // Register the attempt before exposing the executor task. This
        // closes the admitted-before-spawn cancellation window.
        let cancel_token = cancellation;
        {
            let mut running = self.running.lock().await;
            running.insert(
                attempt.attempt_id.clone(),
                RunningAttempt {
                    job_id: job.job_id.clone(),
                    attempt_id: attempt.attempt_id.clone(),
                    workspace_id: job.workspace_id.clone(),
                    started_at: Instant::now(),
                    cancellation: cancel_token.clone(),
                },
            );
        }
        // A cancellation request can arrive after `begin_attempt` but
        // before the running-map insertion. Re-read durable state now that
        // request_cancel can see the running attempt, and propagate any
        // already-recorded request before the executor task is spawned.
        if let Some(current) = self.store.get_job(&job.job_id).await? {
            if current.cancel_requested_at.is_some() {
                cancel_token.cancel();
            }
        }
        // Update per-workspace running counter.
        {
            let mut rpw = self.running_per_workspace.lock().await;
            *rpw.entry(job.workspace_id.clone()).or_insert(0) += 1;
        }
        self.running_total.fetch_add(1, Ordering::SeqCst);

        self.emit_event(SchedulerEvent::JobAdmitted {
            job_id: job.job_id.to_string(),
            attempt_id: attempt.attempt_id.clone(),
            run_id: None,
        })
        .await;

        // Dispatch via the already-validated executor; record completion.
        let me = self.clone();
        let attempt_id = attempt.attempt_id.clone();
        let job_id_for_task = job.job_id.clone();
        let workspace_id_for_task = job.workspace_id.clone();
        let lease_for_task = lease;
        let executor_kind = exec.kind();
        let executor_stats = {
            let executors = self.executors.lock().await;
            executors.stats(executor_kind)
        };
        if let Some(stats) = &executor_stats {
            stats.total_invocations.fetch_add(1, Ordering::Relaxed);
        }
        let running_tasks = self.running_tasks.clone();
        let task_attempt_id = attempt_id.clone();
        {
            let executor = Arc::clone(&exec);
            let store = self.store.clone();
            let running = self.running.clone();
            let running_total = self.running_total.clone();
            let rpw = self.running_per_workspace.clone();
            let completions = self.completions.clone();
            let completions_seq = self.completions_seq.clone();
            let event_tx = self.event_tx.clone();
            let notify = self.notify.clone();
            let executor_stats = executor_stats.clone();
            let task = tokio::spawn(async move {
                let completion = if ctx.cancellation.is_cancelled() {
                    ExecutorCompletion {
                        status: ExecutorStatus::Cancelled,
                        summary: "cancelled before executor start".into(),
                        run_id: None,
                        metrics: Default::default(),
                    }
                } else if let Err(error) = ctx.validate_runtime() {
                    ExecutorCompletion {
                        status: ExecutorStatus::Failed,
                        summary: error.to_string(),
                        run_id: None,
                        metrics: Default::default(),
                    }
                } else {
                    let mut execution = Box::pin(executor.execute(ctx));
                    match execution_timeout {
                        Some(timeout) => {
                            match tokio::time::timeout(timeout, execution.as_mut()).await {
                                Ok(completion) => completion,
                                Err(_) => {
                                    cancel_token.cancel();
                                    match tokio::time::timeout(
                                        EXECUTOR_CLEANUP_GRACE,
                                        execution.as_mut(),
                                    )
                                    .await
                                    {
                                        Ok(mut completion) => {
                                            completion.status = ExecutorStatus::TimedOut;
                                            completion
                                        }
                                        Err(_) => ExecutorCompletion {
                                            status: ExecutorStatus::TimedOut,
                                            summary: "scheduler execution deadline exceeded; executor cleanup did not finish"
                                                .into(),
                                            run_id: None,
                                            metrics: Default::default(),
                                        },
                                    }
                                }
                            }
                        }
                        None => execution.await,
                    }
                };
                {
                    let mut completions_guard = completions.lock().await;
                    let seq = completions_seq.fetch_add(1, Ordering::Relaxed);
                    completions_guard.insert(job_id_for_task.clone(), seq, completion.clone());
                }
                // The permit is dropped when ctx is consumed
                // above; we no longer hold it here.
                // Persist terminal state.
                if let Err(error) = persist_completion(&store, &attempt_id, &completion).await {
                    tracing::error!(
                        job_id = %job_id_for_task,
                        attempt_id = %attempt_id,
                        %error,
                        "executor completed but durable completion persistence failed"
                    );
                }
                if !matches!(completion.status, ExecutorStatus::Completed) {
                    if let Some(stats) = &executor_stats {
                        stats.total_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // M012-F04/C-13: Cancel active descendants when the
                // parent attempt terminates (timeout, failure, cancel,
                // interrupt). This ensures children are cleaned up
                // independently of executor-future liveness.
                if !matches!(completion.status, ExecutorStatus::Completed) {
                    if let Err(error) = store
                        .cancel_descendants(
                            &job_id_for_task,
                            CancelReason::new(
                                "scheduler",
                                format!(
                                    "parent attempt {} terminated: {}",
                                    attempt_id, completion.summary
                                ),
                            ),
                        )
                        .await
                    {
                        tracing::warn!(
                            job_id = %job_id_for_task,
                            attempt_id = %attempt_id,
                            %error,
                            "failed to cancel descendants after executor completion"
                        );
                    }
                }
                // Unregister running.
                let workspace_id = {
                    let mut rg = running.lock().await;
                    rg.remove(&attempt_id)
                        .map(|ra| ra.workspace_id)
                        .unwrap_or(workspace_id_for_task)
                };
                {
                    let mut rpw_g = rpw.lock().await;
                    if let Some(c) = rpw_g.get_mut(&workspace_id) {
                        *c = c.saturating_sub(1);
                    }
                }
                drop(lease_for_task);
                let g = event_tx.lock().await;
                if let Some(tx) = g.as_ref() {
                    if let Err(error) = tx
                        .send(SchedulerEvent::JobResourceReleased {
                            job_id: job_id_for_task.to_string(),
                            attempt_id: attempt_id.clone(),
                        })
                        .await
                    {
                        tracing::debug!(
                            ?error,
                            "scheduler resource-release event receiver is unavailable"
                        );
                    }
                }
                // Keep the running count visible until the final release
                // event has been emitted, so shutdown cannot mistake an
                // in-flight task for fully cleaned-up work.
                running_total.fetch_sub(1, Ordering::SeqCst);
                me.wake(WokeReason::ExecutorCompleted);
                notify.notify_one();
                running_tasks.lock().await.remove(&attempt_id);
            });
            self.running_tasks
                .lock()
                .await
                .insert(task_attempt_id, task);
        }
        Ok(true)
    }

    /// Snapshot of externally visible state. Composed from the
    /// queue, admission, running, and executor registry.
    pub async fn snapshot(&self) -> SchedulerSnapshot {
        let (queued_per_workspace, ready_window_count) = {
            let q = self.queue.lock().await;
            (q.per_workspace().clone(), q.total())
        };
        let admission = self.admission.snapshot();
        let running_attempts = { self.running.lock().await.len() };
        let rpw = { self.running_per_workspace.lock().await.clone() };
        let ready_counts = { self.ready_counts.lock().await.clone() };
        let executors_snap: Vec<ExecutorHealthSnapshot> = {
            let executors = self.executors.lock().await;
            executors
                .health_snapshot_with_stats()
                .into_iter()
                .map(|(k, h, stats)| ExecutorHealthSnapshot {
                    executor: k.as_str().to_string(),
                    health: h,
                    total_invocations: stats.total_invocations.load(Ordering::Relaxed),
                    total_failures: stats.total_failures.load(Ordering::Relaxed),
                })
                .collect()
        };
        let oldest = { *self.oldest_queued_age_secs.lock().await };

        let mut by_priority = BTreeMap::new();
        for (label, count) in ready_counts.iter() {
            by_priority.insert(label.clone(), *count);
        }
        let mut workspace_ids: std::collections::BTreeSet<WorkspaceId> =
            queued_per_workspace.keys().cloned().collect();
        workspace_ids.extend(rpw.keys().cloned());
        let per_workspace: Vec<_> = workspace_ids
            .into_iter()
            .map(|ws| {
                let queued = queued_per_workspace.get(&ws).copied().unwrap_or(0);
                crate::scheduler::snapshot::PerWorkspaceSummary {
                    workspace_id: ws.clone(),
                    queued,
                    running: rpw.get(&ws).copied().unwrap_or(0),
                    // The fair queue is the scheduler's ready window. The
                    // legacy `queued` field is retained for wire compatibility.
                    ready_window: queued,
                }
            })
            .collect();

        let durable_queued_count = ready_window_count;

        let by_kind_local = self.job_kind_counts.lock().await.clone();

        let resources = crate::scheduler::snapshot::ResourceSummary::from_admission(
            &admission,
            &crate::scheduler::snapshot::ResourceBudgetView {
                max_process_slots: self.config.resources.max_process_slots,
                max_cpu_weight: self.config.resources.max_cpu_weight,
                max_memory_mb_hint: self.config.resources.max_memory_mb_hint,
                max_io_weight: self.config.resources.max_io_weight,
                max_network_slots: self.config.resources.max_network_slots,
            },
        );

        SchedulerSnapshot {
            ready_window_count,
            durable_queued_count,
            running_attempts,
            per_priority: SnapshotCounts {
                by_priority,
                by_kind: by_kind_local,
            },
            per_workspace,
            resources,
            executors: executors_snap,
            overload: crate::scheduler::snapshot::OverloadSummary {
                rejected_admissions: self.admission_blocks.load(Ordering::SeqCst),
                impossible_admissions: self.admission_impossible.load(Ordering::SeqCst),
                queue_overflows: self.queue_overflows.load(Ordering::SeqCst),
            },
            admission_blocks: crate::scheduler::snapshot::AdmissionBlockSummary {
                total: self.admission_blocks.load(Ordering::SeqCst),
                by_reason: self.admission_block_reasons.lock().await.clone(),
            },
            oldest_queued_age_secs: oldest,
            rollout_mode: format!("{:?}", self.config.rollout),
            enabled: self.config.enabled,
        }
    }

    /// Initiate a drain shutdown. Honours the supplied mode. Returns
    /// when the running attempts have been signalled and the main
    /// loop has exited.
    pub async fn shutdown(&self, mode: SchedulerShutdownMode) {
        self.shutdown.cancel();
        let _dispatch = self.dispatch.lock().await;
        match mode {
            SchedulerShutdownMode::ImmediateInterrupt => {
                self.cancel_pending_and_running("shutdown immediate-interrupt")
                    .await;
                self.wait_for_running_cleanup(SHUTDOWN_CLEANUP_GRACE).await;
            }
            SchedulerShutdownMode::StopAcceptingAndCancelQueued => {
                self.cancel_pending_and_running("shutdown stop-accepting")
                    .await;
                self.wait_for_running_cleanup(SHUTDOWN_CLEANUP_GRACE).await;
            }
            SchedulerShutdownMode::DrainQueuedUntil(deadline) => {
                let deadline_at = Instant::now() + deadline;
                while Instant::now() < deadline_at
                    && (self.running_total.load(Ordering::SeqCst) > 0
                        || !self.running_tasks.lock().await.is_empty())
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                if self.running_total.load(Ordering::SeqCst) > 0
                    || !self.running_tasks.lock().await.is_empty()
                {
                    if self.running_total.load(Ordering::SeqCst) > 0 {
                        self.cancel_running().await;
                    }
                    self.wait_for_running_cleanup(SHUTDOWN_CLEANUP_GRACE).await;
                }
            }
        }
    }

    async fn cancel_pending_and_running(&self, reason: &str) {
        let mut ids = std::collections::HashSet::new();
        {
            let queue = self.queue.lock().await;
            ids.extend(
                queue
                    .lanes()
                    .values()
                    .flat_map(|lane_queue| lane_queue.lanes.values())
                    .flat_map(|lane| lane.entries.iter().map(|entry| entry.job_id.clone())),
            );
        }
        let query = codegg_core::jobs::store::JobStoreQuery {
            states: vec![JobState::Scheduled, JobState::Queued, JobState::Blocked],
            workspace_id: None,
            kinds: vec![],
            limit: None,
            session_id: None,
        };
        if let Ok(jobs) = self.store.list_jobs(query).await {
            ids.extend(jobs.into_iter().map(|job| job.job_id));
        }
        for id in ids {
            if let Err(error) = self.request_cancel(&id, reason).await {
                tracing::warn!(job_id = %id, %error, "scheduler failed to cancel a job during shutdown");
            }
        }
        self.cancel_running().await;
    }

    async fn wait_for_running_cleanup(&self, cleanup_grace: Duration) {
        let mut handles: Vec<_> = self
            .running_tasks
            .lock()
            .await
            .drain()
            .map(|(_, handle)| handle)
            .collect();
        if tokio::time::timeout(cleanup_grace, async {
            for handle in &mut handles {
                if let Err(error) = handle.await {
                    tracing::warn!(%error, "scheduler executor task failed during shutdown cleanup");
                }
            }
        })
        .await
        .is_err()
        {
            for handle in &handles {
                handle.abort();
            }
            for handle in handles {
                if let Err(error) = handle.await {
                    tracing::debug!(%error, "scheduler executor task ended after shutdown abort");
                }
            }
            tracing::warn!(
                running = self.running_total.load(Ordering::SeqCst),
                "scheduler shutdown cleanup deadline exceeded; executor tasks aborted"
            );
        }
    }

    async fn cancel_running(&self) {
        let cancellations: Vec<_> = self
            .running
            .lock()
            .await
            .values()
            .map(|attempt| attempt.cancellation.clone())
            .collect();
        for cancellation in cancellations {
            cancellation.cancel();
        }
    }

    async fn mark_unschedulable(
        &self,
        job: &JobRecord,
        reason: &str,
    ) -> Result<(), JobSchedulerError> {
        // The job remains in JobStore; mark the attempt as failed.
        // We use begin_attempt to create the attempt if needed.
        let attempt = self
            .store
            .begin_attempt(&job.job_id, &self.daemon_generation)
            .await?;
        self.store.mark_attempt_running(&attempt.attempt_id).await?;
        let completion = AttemptCompletion {
            attempt_id: attempt.attempt_id.clone(),
            state: AttemptState::Failed,
            error: Some(JobErrorRecord {
                class: FailureClass::Validation,
                message: reason.to_string(),
                transient: false,
            }),
            run_id: None,
        };
        self.store.finish_attempt(completion).await?;
        Ok(())
    }

    async fn emit_event(&self, event: SchedulerEvent) {
        let g = self.event_tx.lock().await;
        if let Some(tx) = g.as_ref() {
            if let Err(error) = tx.send(event).await {
                tracing::warn!(?error, "scheduler event receiver is unavailable");
            }
        }
    }

    /// Cancel a specific job. If the job is queued, the durable
    /// state is updated and the queue entry is removed. If the
    /// job is running, the executor's `CancellationToken` is
    /// triggered.
    pub async fn request_cancel(
        &self,
        job_id: &JobId,
        reason: &str,
    ) -> Result<codegg_core::jobs::CancelResult, JobSchedulerError> {
        let cancel = CancelReason::new("scheduler", reason);
        let job_before_cancel = self.store.get_job(job_id).await?;
        let result = self.store.request_cancel(job_id, cancel).await?;
        if matches!(result.state, codegg_core::jobs::CancelOutcome::Cancelled) {
            if let (
                Some(JobRecord {
                    payload: codegg_core::jobs::JobPayload::SubagentRun { run_id, .. },
                    ..
                }),
                Some(agent_runs),
            ) = (job_before_cancel, self.agent_runs.lock().await.clone())
            {
                let _ = agent_runs
                    .finish(
                        &run_id,
                        AgentRunTerminalOutcome::Cancelled,
                        None,
                        Some("cancelled_before_admission".into()),
                        Some(reason.to_string()),
                    )
                    .await;
            }
        }
        // Remove from in-memory queue if present.
        let mut q = self.queue.lock().await;
        let _ = q.remove(job_id, QueueRemovalReason::Cancelled);
        drop(q);
        // If running, signal cancellation.
        let running = self.running.lock().await;
        for ra in running.values() {
            if ra.job_id == *job_id {
                ra.cancellation.cancel();
            }
        }
        drop(running);
        // M012-F04/C-13: Cancel active descendants independently of
        // the executor future. This ensures children are cleaned up
        // even when the executor has already exited or the future is
        // dropped.
        if let Err(error) = self
            .store
            .cancel_descendants(job_id, CancelReason::new("scheduler", reason))
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                %error,
                "failed to cancel descendants during request_cancel"
            );
        }
        crate::test_failpoint::hit("tool_program_after_descendant_cancel");
        Ok(result)
    }

    /// Recover durable jobs whose attempts originated from a prior
    /// daemon generation. Called once at startup before the main
    /// loop begins admitting work. The recovery report is also
    /// exposed via `CoreRequest::JobRecoveryReport`.
    pub async fn recover_at_startup(
        &self,
        policy: &codegg_core::jobs::RecoveryPolicy,
    ) -> Result<codegg_core::jobs::RecoveryReport, JobSchedulerError> {
        // The `recover_generation` API expects the *new* generation
        // and interrupts any non-terminal attempt whose generation
        // does not match.
        let new_gen = self.daemon_generation.clone();
        let report = self.store.recover_generation(&new_gen, policy).await?;
        self.reconcile_agent_runs_after_recovery().await?;
        // Wake the scheduler so the requeued work is considered
        // during the next reconcile pass.
        self.wake(crate::scheduler::events::WokeReason::Reconciled);
        Ok(report)
    }

    async fn reconcile_agent_runs_after_recovery(&self) -> Result<(), JobSchedulerError> {
        let Some(agent_runs) = self.agent_runs.lock().await.clone() else {
            return Ok(());
        };
        let jobs = self
            .store
            .list_job_records(codegg_core::jobs::store::JobStoreQuery {
                kinds: vec![codegg_core::jobs::JobKind::Subagent],
                ..Default::default()
            })
            .await?;
        for job in jobs {
            let codegg_core::jobs::JobPayload::SubagentRun { run_id, .. } = job.payload else {
                continue;
            };
            let outcome = match job.state {
                JobState::Cancelled => Some(AgentRunTerminalOutcome::Cancelled),
                JobState::Interrupted => Some(AgentRunTerminalOutcome::Interrupted),
                JobState::Failed | JobState::TimedOut => Some(AgentRunTerminalOutcome::Interrupted),
                _ => None,
            };
            if let Some(outcome) = outcome {
                let _ = agent_runs
                    .finish(
                        &run_id,
                        outcome,
                        None,
                        Some("scheduler_recovery".into()),
                        Some(format!("job recovered as {}", job.state.as_str())),
                    )
                    .await;
            }
        }
        Ok(())
    }
}

/// Result of a reconcile pass. Useful for tests and diagnostics.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReconcileReport {
    pub added: usize,
    pub removed: usize,
    pub duplicates: usize,
}

/// Top-level scheduler errors.
#[derive(Debug, thiserror::Error)]
pub enum JobSchedulerError {
    #[error("job store error: {0}")]
    Store(#[from] JobStoreError),
    #[error("executor registry error: {0}")]
    Registry(#[from] crate::scheduler::executor::ExecutorRegistryError),
    #[error("workspace services error: {0}")]
    Workspace(String),
    #[error("scheduler is disabled; daemon-owned work cannot bypass admission")]
    SchedulerDisabled,
    #[error("internal: {0}")]
    Internal(String),
}

fn job_eligible(
    job: &JobRecord,
    dependency_states: &std::collections::HashMap<JobId, JobState>,
) -> bool {
    if !matches!(job.state, JobState::Queued) {
        return false;
    }
    let now = Utc::now();
    if let Some(nb) = job.not_before {
        if now < nb {
            return false;
        }
    }
    if let Some(dl) = job.deadline {
        if now > dl {
            return false;
        }
    }
    for dependency in &job.depends_on {
        if !matches!(dependency_states.get(dependency), Some(JobState::Completed)) {
            return false;
        }
    }
    true
}

fn build_permit_dimensions(job: &JobRecord) -> PermitDimensions {
    PermitDimensions {
        cpu_weight: job.resource_request.cpu_weight,
        memory_mb_hint: job.resource_request.memory_mb_hint,
        process_slots: job.resource_request.process_slots,
        io_weight: job.resource_request.io_weight,
        network_slots: job.resource_request.network_slots,
        exclusivity_keys: job.resource_request.exclusivity_keys.clone(),
    }
}

async fn persist_completion(
    store: &Arc<dyn JobStore>,
    attempt_id: &AttemptId,
    completion: &ExecutorCompletion,
) -> Result<(), JobStoreError> {
    let state = match completion.status {
        ExecutorStatus::Completed => AttemptState::Completed,
        ExecutorStatus::Failed => AttemptState::Failed,
        ExecutorStatus::Cancelled => AttemptState::Cancelled,
        ExecutorStatus::TimedOut => AttemptState::TimedOut,
        ExecutorStatus::Interrupted => AttemptState::Interrupted,
    };
    let err = if state != AttemptState::Completed {
        Some(JobErrorRecord {
            class: FailureClass::Execution,
            message: completion.summary.clone(),
            transient: false,
        })
    } else {
        None
    };
    let ac = AttemptCompletion {
        attempt_id: attempt_id.clone(),
        state,
        error: err,
        run_id: completion.run_id.clone(),
    };
    store.finish_attempt(ac).await.map(|_| ())
}

struct DurableProgressSink {
    store: Arc<dyn JobStore>,
    attempt_id: AttemptId,
}

#[async_trait::async_trait]
impl JobProgressSink for DurableProgressSink {
    async fn progress(&self, _job_id: &JobId, _message: &str) {
        if let Err(error) = self
            .store
            .record_heartbeat(&self.attempt_id, Utc::now())
            .await
        {
            tracing::debug!(%error, attempt_id = %self.attempt_id, "failed to persist executor heartbeat");
        }
    }
}

// `ExecutorHealth` and `ExecutorMetrics` are referenced by the
// snapshot builders; the type-level re-exports keep them alive.
#[allow(dead_code)]
fn _silence_executor_health() {}
#[allow(dead_code)]
fn _silence_executor_metrics() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion() -> ExecutorCompletion {
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary: "ok".to_string(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }

    #[test]
    fn completion_cache_evicts_oldest_and_replaces_in_order() {
        let mut cache = CompletionCache::new();
        for seq in 0..1025 {
            cache.insert(
                JobId::new_unchecked(format!("job-{seq}")),
                seq,
                completion(),
            );
        }

        assert_eq!(cache.entries.len(), 1024);
        assert!(!cache.entries.contains_key(&JobId::new_unchecked("job-0")));
        assert!(cache.entries.contains_key(&JobId::new_unchecked("job-1")));

        cache.insert(JobId::new_unchecked("job-1024"), 1025, completion());
        assert_eq!(cache.entries.len(), 1024);
        assert!(cache.entries.contains_key(&JobId::new_unchecked("job-1")));
        assert_eq!(
            cache
                .entries
                .get(&JobId::new_unchecked("job-1024"))
                .map(|v| v.0),
            Some(1025)
        );

        cache.insert(JobId::new_unchecked("job-1025"), 1026, completion());
        assert_eq!(cache.entries.len(), 1024);
        assert!(!cache.entries.contains_key(&JobId::new_unchecked("job-1")));
    }
}
