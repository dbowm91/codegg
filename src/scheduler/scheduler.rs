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
use codegg_core::jobs::{
    AttemptCompletion, AttemptId, AttemptState, CancelReason, DaemonGeneration, FailureClass,
    JobErrorRecord, JobId, JobRecord, JobState, JobStore, JobStoreError,
};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace_services::WorkspaceServiceRegistry;
use tokio::sync::{mpsc, Mutex as AsyncMutex, Notify};
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

pub struct JobScheduler {
    store: Arc<dyn JobStore>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    executors: Arc<AsyncMutex<crate::scheduler::executor::ExecutorRegistry>>,
    admission: Arc<AdmissionController>,
    queue: Arc<AsyncMutex<FairJobQueue>>,
    running: Arc<AsyncMutex<HashMap<AttemptId, RunningAttempt>>>,
    /// Recent in-process completions let daemon clients receive the same
    /// bounded executor projection that completed the work. Durable job and
    /// attempt state remains authoritative across restart.
    completions: Arc<AsyncMutex<HashMap<JobId, ExecutorCompletion>>>,
    /// Per-workspace running attempts (denormalized for snapshot).
    running_per_workspace: Arc<AsyncMutex<HashMap<WorkspaceId, usize>>>,
    /// Per-priority ready-window counts (denormalized).
    ready_counts: Arc<AsyncMutex<BTreeMap<String, usize>>>,
    /// Total running count.
    running_total: Arc<AtomicU64>,
    /// Total admit blocks recorded.
    admission_blocks: Arc<AtomicU64>,
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
    /// Optional channel for emitting events. `None` in standalone /
    /// test mode; the daemon sets a real bus sink.
    event_tx: Arc<AsyncMutex<Option<mpsc::Sender<SchedulerEvent>>>>,
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
        let running_per_workspace = Arc::new(AsyncMutex::new(HashMap::new()));
        let ready_counts = Arc::new(AsyncMutex::new(BTreeMap::new()));
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
            running,
            completions: Arc::new(AsyncMutex::new(HashMap::new())),
            running_per_workspace,
            ready_counts,
            running_total: Arc::new(AtomicU64::new(0)),
            admission_blocks: Arc::new(AtomicU64::new(0)),
            admission_impossible: Arc::new(AtomicU64::new(0)),
            queue_overflows: Arc::new(AtomicU64::new(0)),
            oldest_queued_age_secs,
            notify: Arc::new(Notify::new()),
            shutdown: CancellationToken::new(),
            config: Arc::new(config),
            daemon_generation,
            event_tx,
        })
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
    /// [`SchedulerEvent`]s to this sender; the daemon bridges them to
    /// the core event log.
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

    /// Synchronous default-executor registration helper. Builds the
    /// test/managed-argv/subagent executors with no RunStore / event
    /// sink wiring (the daemon reconnects them at runtime) and
    /// installs them on the registry. Used by daemon construction
    /// when the scheduler's event loop is being spawned synchronously.
    pub fn register_default_executors_sync(
        &self,
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
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
            registry.register(Arc::new(SubagentJobExecutor::new(pool)))?;
        }
        let kinds = registry.kinds();
        let execs: Vec<Arc<dyn JobExecutor>> =
            kinds.into_iter().filter_map(|k| registry.get(k)).collect();
        // Push them through the async bulk-register using try_lock;
        // construction-time callers are single-writer so this is safe.
        if let Ok(mut g) = self.executors.try_lock() {
            for exec in execs {
                if let Err(e) = g.register(exec) {
                    tracing::warn!(
                        target: "scheduler.executor",
                        "duplicate executor at default registration: {e}"
                    );
                }
            }
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
                let g = event_tx.lock().await;
                if let Some(tx) = g.as_ref() {
                    let _ = tx
                        .send(SchedulerEvent::SchedulerWoke {
                            reason: reason_clone,
                        })
                        .await;
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
            if let Some(completion) = self.completions.lock().await.get(job_id).cloned() {
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
            tokio::time::sleep(Duration::from_millis(20)).await;
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
        let durable = self.store.list_jobs(query).await?;
        let durable_ids: std::collections::HashSet<JobId> =
            durable.iter().map(|j| j.job_id.clone()).collect();

        // 1. Insert durable jobs not already in the queue.
        for summary in durable {
            // Skip if not eligible (not_before / deadline / dependencies).
            if let Some(job) = self.store.get_job(&summary.job_id).await? {
                if !job_eligible(&job) {
                    continue;
                }
                let mut entry = QueueEntry::from_job(&job);
                entry.recompute_aging(&self.config, Utc::now());
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
        }

        // 2. Remove queue entries whose durable state is no longer
        // Queued (cancelled, completed, etc).
        let q = self.queue.lock().await;
        let to_remove: Vec<JobId> = {
            // Walk every entry and check durable state. For a small
            // queue this is fine; for a large queue a watermark
            // index would be preferable.
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
        drop(q);
        for id in to_remove {
            let mut q = self.queue.lock().await;
            if q.remove(&id, QueueRemovalReason::Dropped).is_some() {
                removed += 1;
            }
        }

        // 3. Update oldest-queued-age.
        {
            let mut q = self.queue.lock().await;
            q.recompute_aging(Utc::now());
            let mut oldest = self.oldest_queued_age_secs.lock().await;
            *oldest = q
                .lanes()
                .values()
                .flat_map(|lane_q| lane_q.lanes.values())
                .flat_map(|lane| lane.entries.iter())
                .map(|e| (Utc::now() - e.submitted_at).num_seconds().max(0) as u64)
                .min();
        }

        // 4. Update ready-window counts by priority.
        {
            let q = self.queue.lock().await;
            let mut counts = self.ready_counts.lock().await;
            counts.clear();
            for (class, lane_q) in q.lanes() {
                let label = format!("{:?}", class);
                let total = lane_q.total();
                if total > 0 {
                    *counts.entry(label).or_insert(0) += total;
                }
            }
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
        let mut dispatched = 0;
        let max_batch = 4usize; // bounded; the loop above will run again
                                // Inspect a small candidate window instead of treating a
                                // temporarily blocked head job as global back-pressure. This lets
                                // an unrelated workspace make progress while the contended job is
                                // requeued for the next reconciliation tick.
        for _ in 0..(max_batch * 2) {
            if dispatched >= max_batch {
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
                self.emit_event(SchedulerEvent::AdmissionBlocked {
                    job_id: job.job_id.to_string(),
                    reason,
                })
                .await;
                // Re-insert the entry so we try again next tick.
                let mut q = self.queue.lock().await;
                let _ = q.insert(entry);
                return Ok(false);
            }
            AdmissionDecision::Impossible(reason) => {
                self.admission_impossible.fetch_add(1, Ordering::SeqCst);
                // Mark the job Failed in durable state.
                let _ = self
                    .mark_unschedulable(&job, &format!("{:?}", reason))
                    .await;
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
                let _ = q.insert(entry);
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
            cancellation: cancellation.clone(),
            progress: Arc::new(NoopSink),
            resources: permit,
        };

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

        // Dispatch via the already-validated executor; record completion.
        let me = self.clone();
        let attempt_id = attempt.attempt_id.clone();
        let job_id_for_task = job.job_id.clone();
        let lease_for_task = lease;
        {
            let executor = Arc::clone(&exec);
            let store = self.store.clone();
            let running = self.running.clone();
            let running_total = self.running_total.clone();
            let rpw = self.running_per_workspace.clone();
            let completions = self.completions.clone();
            let event_tx = self.event_tx.clone();
            let notify = self.notify.clone();
            tokio::spawn(async move {
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
                    executor.execute(ctx).await
                };
                {
                    let mut completions_guard = completions.lock().await;
                    completions_guard.insert(job_id_for_task.clone(), completion.clone());
                    if completions_guard.len() > 1024 {
                        if let Some(oldest) = completions_guard.keys().next().cloned() {
                            completions_guard.remove(&oldest);
                        }
                    }
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
                // Unregister running.
                {
                    let mut rg = running.lock().await;
                    if let Some(ra) = rg.remove(&attempt_id) {
                        let mut rpw_g = rpw.lock().await;
                        if let Some(c) = rpw_g.get_mut(&ra.workspace_id) {
                            *c = c.saturating_sub(1);
                        }
                    }
                }
                running_total.fetch_sub(1, Ordering::SeqCst);
                drop(lease_for_task);
                // Forward event.
                if completion.run_id.is_some() {
                    let g = event_tx.lock().await;
                    if let Some(tx) = g.as_ref() {
                        let _ = tx
                            .send(SchedulerEvent::JobAdmitted {
                                job_id: job_id_for_task.to_string(),
                                attempt_id: attempt_id.clone(),
                                run_id: completion.run_id.clone(),
                            })
                            .await;
                    }
                }
                let g = event_tx.lock().await;
                if let Some(tx) = g.as_ref() {
                    let _ = tx
                        .send(SchedulerEvent::JobResourceReleased {
                            job_id: job_id_for_task.to_string(),
                            attempt_id: attempt_id.clone(),
                        })
                        .await;
                }
                me.wake(WokeReason::ExecutorCompleted);
                notify.notify_one();
            });
        }
        Ok(true)
    }

    /// Snapshot of externally visible state. Composed from the
    /// queue, admission, running, and executor registry.
    pub async fn snapshot(&self) -> SchedulerSnapshot {
        let q = self.queue.lock().await;
        let admission = self.admission.snapshot();
        let running = self.running.lock().await;
        let rpw = self.running_per_workspace.lock().await;
        let ready_counts = self.ready_counts.lock().await;
        let executors = self.executors.lock().await;
        let oldest = self.oldest_queued_age_secs.lock().await;

        let mut by_priority = BTreeMap::new();
        for (label, count) in ready_counts.iter() {
            by_priority.insert(label.clone(), *count);
        }
        let per_workspace: Vec<_> = rpw
            .iter()
            .map(
                |(ws, running)| crate::scheduler::snapshot::PerWorkspaceSummary {
                    workspace_id: ws.clone(),
                    queued: q.per_workspace().get(ws).copied().unwrap_or(0),
                    running: *running,
                    ready_window: q.per_workspace().get(ws).copied().unwrap_or(0),
                },
            )
            .collect();

        let ready_window_count = q.total();
        let durable_queued_count = q.total();

        let executors_snap: Vec<ExecutorHealthSnapshot> = executors
            .health_snapshot()
            .into_iter()
            .map(|(k, h)| ExecutorHealthSnapshot {
                executor: k.as_str().to_string(),
                health: h,
                total_invocations: 0,
                total_failures: 0,
            })
            .collect();

        let by_kind_local = BTreeMap::<String, usize>::new();
        for ra in running.values() {
            // attempt kind is on the job; we don't have the job
            // here, so leave by_kind empty in the snapshot. The
            // ready-window counts use job kinds via queue entries;
            // the scheduler does not currently persist kind in the
            // queue entry.
            let _ = ra;
        }

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
            running_attempts: running.len(),
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
                by_reason: BTreeMap::new(),
            },
            oldest_queued_age_secs: *oldest,
            rollout_mode: format!("{:?}", self.config.rollout),
            enabled: self.config.enabled,
        }
    }

    /// Initiate a drain shutdown. Honours the supplied mode. Returns
    /// when the running attempts have been signalled and the main
    /// loop has exited.
    pub async fn shutdown(&self, mode: SchedulerShutdownMode) {
        match mode {
            SchedulerShutdownMode::ImmediateInterrupt => {
                self.shutdown.cancel();
            }
            SchedulerShutdownMode::StopAcceptingAndCancelQueued => {
                // Cancel queued.
                let q = self.queue.lock().await;
                let ids: Vec<JobId> = q
                    .lanes()
                    .values()
                    .flat_map(|lq| lq.lanes.values())
                    .flat_map(|l| l.entries.iter().map(|e| e.job_id.clone()))
                    .collect();
                drop(q);
                for id in ids {
                    let _ = self.request_cancel(&id, "shutdown stop-accepting").await;
                }
                // Cancel running.
                let running = self.running.lock().await;
                for ra in running.values() {
                    ra.cancellation.cancel();
                }
                self.shutdown.cancel();
            }
            SchedulerShutdownMode::DrainQueuedUntil(deadline) => {
                let deadline_at = Instant::now() + deadline;
                while Instant::now() < deadline_at && self.running_total.load(Ordering::SeqCst) > 0
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                // After deadline, cancel any remaining.
                let running = self.running.lock().await;
                for ra in running.values() {
                    ra.cancellation.cancel();
                }
                self.shutdown.cancel();
            }
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
            .await
            .ok();
        if let Some(a) = attempt {
            let _ = self.store.mark_attempt_running(&a.attempt_id).await;
            let completion = AttemptCompletion {
                attempt_id: a.attempt_id.clone(),
                state: AttemptState::Failed,
                error: Some(JobErrorRecord {
                    class: FailureClass::Validation,
                    message: reason.to_string(),
                    transient: false,
                }),
                run_id: None,
            };
            let _ = self.store.finish_attempt(completion).await;
        }
        Ok(())
    }

    async fn emit_event(&self, event: SchedulerEvent) {
        let g = self.event_tx.lock().await;
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(event).await;
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
        let result = self.store.request_cancel(job_id, cancel).await?;
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
        // Wake the scheduler so the requeued work is considered
        // during the next reconcile pass.
        self.wake(crate::scheduler::events::WokeReason::Reconciled);
        Ok(report)
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

fn job_eligible(job: &JobRecord) -> bool {
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
    if !job.depends_on.is_empty() {
        return false;
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

struct NoopSink;
#[async_trait::async_trait]
impl JobProgressSink for NoopSink {}

// `ExecutorHealth` and `ExecutorMetrics` are referenced by the
// snapshot builders; the type-level re-exports keep them alive.
#[allow(dead_code)]
fn _silence_executor_health() {}
#[allow(dead_code)]
fn _silence_executor_metrics() {}
