//! The daemon-owned boundary for creating executable jobs.
//!
//! Callers must not create a durable job and then invoke an executor
//! themselves. `JobSubmissionService` validates the workspace, applies the
//! canonical resource profile, creates exactly one durable record, and wakes
//! the scheduler. The in-memory idempotency index protects transport retries
//! during one daemon lifetime; the job id remains the durable source of truth
//! returned to the caller.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use codegg_core::jobs::{
    DaemonGeneration, JobId, JobKind, JobPayload, JobRecord, JobState, JobStore, KeyedQuotaRequest,
    NewJob, ResourceClass, ResourceRequest, GOAL_PROVENANCE_LABEL_KEY,
};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace_services::WorkspaceServiceRegistry;
use thiserror::Error;
use tokio::sync::Mutex;

use super::scheduler::{JobScheduler, JobSchedulerError};

const MAX_SUBMISSION_KEY_BYTES: usize = 256;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_SUBMISSION_LABELS: usize = 64;
const MAX_LABEL_KEY_BYTES: usize = 128;
const MAX_LABEL_VALUE_BYTES: usize = 4096;

/// Caller-provided retry identity. It is deliberately opaque: the daemon
/// never parses it or treats it as a database identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubmissionKey(String);

impl SubmissionKey {
    pub fn new(value: impl Into<String>) -> Result<Self, JobSubmissionError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_SUBMISSION_KEY_BYTES {
            return Err(JobSubmissionError::InvalidSubmissionKey);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Minimal metadata returned after a job is durably submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJob {
    pub job_id: JobId,
    pub state: JobState,
    pub workspace_id: WorkspaceId,
    pub priority: codegg_core::jobs::JobPriority,
}

/// Errors raised before a caller can receive a submission handle.
#[derive(Debug, Error)]
pub enum JobSubmissionError {
    #[error("scheduler is disabled; daemon-owned work cannot bypass admission")]
    SchedulerDisabled,
    #[error("workspace validation failed: {0}")]
    Workspace(String),
    #[error("job store error: {0}")]
    Store(#[from] codegg_core::jobs::JobStoreError),
    #[error("scheduler enqueue error: {0}")]
    Scheduler(#[from] JobSchedulerError),
    #[error("submission key must be between 1 and {MAX_SUBMISSION_KEY_BYTES} bytes")]
    InvalidSubmissionKey,
    #[error("submission key was reused for a different job request")]
    SubmissionKeyConflict,
    #[error("job payload exceeds the {MAX_PAYLOAD_BYTES}-byte limit")]
    PayloadTooLarge,
    #[error("job metadata exceeds its bounded label limits")]
    LabelsTooLarge,
    #[error("unsupported job payload for kind '{0}'")]
    InvalidPayload(String),
    #[error("active goal lookup failed: {0}")]
    Goal(String),
    /// M005 corrective C005: a process-class job was refused because the
    /// target checkout is reserved by a live agent-run group.
    ///
    /// The refusal happens at submission, before the job is durably
    /// created, so a refused submission leaves no job row and no attempt.
    #[error("process-class job refused: {0}")]
    ProcessDenied(String),
    #[error("repository-scoped build admission requires verified repository lineage: {0}")]
    RepositoryIdentityUnavailable(String),
}

#[derive(Clone)]
struct IdempotentSubmission {
    fingerprint: String,
    job_id: JobId,
}

/// Whether a job kind starts an OS process.
///
/// The classification is by **job kind**, which is the only thing the
/// submission boundary knows. A job that starts no process — an agent
/// turn, a subagent, a research job — is never refused on the process
/// ground, because the refusal is about the capability and not about the
/// checkout.
fn is_process_class_job(kind: codegg_core::jobs::JobKind) -> bool {
    use codegg_core::jobs::JobKind;
    matches!(
        kind,
        JobKind::Build
            | JobKind::Test
            | JobKind::Lint
            | JobKind::Format
            | JobKind::Shell
            | JobKind::ManagedProcess
            | JobKind::Python
            | JobKind::ToolProgram
    )
}

/// Single production facade for durable job creation and scheduler enqueue.
pub struct JobSubmissionService {
    store: Arc<dyn JobStore>,
    scheduler: Arc<JobScheduler>,
    workspaces: Arc<WorkspaceServiceRegistry>,
    /// Kept in the facade so all callers share one retry boundary. The
    /// generation is retained here for provenance and future store-backed
    /// submission-key indexing.
    daemon_generation: DaemonGeneration,
    goal_store: Option<Arc<codegg_core::goal::GoalStore>>,
    idempotency: Mutex<HashMap<SubmissionKey, IdempotentSubmission>>,
    created_sink: parking_lot::RwLock<Option<tokio::sync::mpsc::Sender<JobRecord>>>,
    /// M005 corrective C005: the daemon's process admission authority.
    ///
    /// `None` means the daemon has no durable reservation authority, so no
    /// checkout is ever reserved and no submission is refused on this
    /// ground.
    process_admission:
        parking_lot::RwLock<Option<Arc<codegg_core::process_admission::ProcessAdmissionService>>>,
    context_resolver:
        parking_lot::RwLock<Option<Arc<codegg_core::context::ProjectContextResolver>>>,
}

impl JobSubmissionService {
    pub fn new(
        store: Arc<dyn JobStore>,
        scheduler: Arc<JobScheduler>,
        workspaces: Arc<WorkspaceServiceRegistry>,
        daemon_generation: DaemonGeneration,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            scheduler,
            workspaces,
            daemon_generation,
            goal_store: None,
            idempotency: Mutex::new(HashMap::new()),
            created_sink: parking_lot::RwLock::new(None),
            process_admission: parking_lot::RwLock::new(None),
            context_resolver: parking_lot::RwLock::new(None),
        })
    }

    /// Construct a submission boundary that can attach the active goal
    /// snapshot to supervised Test/Subagent jobs. The goal is read at the
    /// host submission boundary; callers cannot supply or override it.
    pub fn new_with_goal_store(
        store: Arc<dyn JobStore>,
        scheduler: Arc<JobScheduler>,
        workspaces: Arc<WorkspaceServiceRegistry>,
        daemon_generation: DaemonGeneration,
        goal_store: Arc<codegg_core::goal::GoalStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            scheduler,
            workspaces,
            daemon_generation,
            goal_store: Some(goal_store),
            idempotency: Mutex::new(HashMap::new()),
            created_sink: parking_lot::RwLock::new(None),
            process_admission: parking_lot::RwLock::new(None),
            context_resolver: parking_lot::RwLock::new(None),
        })
    }

    pub fn scheduler(&self) -> &Arc<JobScheduler> {
        &self.scheduler
    }

    pub fn daemon_generation(&self) -> &DaemonGeneration {
        &self.daemon_generation
    }

    /// Install the daemon-owned post-admission observer. It is deliberately
    /// notification-only: a closed observer cannot fail durable submission.
    pub fn set_created_sink(&self, sink: tokio::sync::mpsc::Sender<JobRecord>) {
        *self.created_sink.write() = Some(sink);
    }

    /// M005 corrective C005: install the daemon's process admission
    /// authority.
    ///
    /// A process-class job submitted for a checkout reserved by a live
    /// agent-run group is refused here, before the job is durably created.
    /// A job that starts no process — an agent turn, a subagent, a read —
    /// is not refused on this ground.
    pub fn set_process_admission(
        &self,
        service: Arc<codegg_core::process_admission::ProcessAdmissionService>,
    ) {
        *self.process_admission.write() = Some(service);
    }

    /// Install the daemon-owned project/workspace resolver used to bind
    /// opt-in repository quotas from canonical catalog lineage.
    pub fn set_context_resolver(
        &self,
        resolver: Arc<codegg_core::context::ProjectContextResolver>,
    ) {
        *self.context_resolver.write() = Some(resolver);
    }

    pub async fn workspace_id_for_root(
        &self,
        root: &Path,
    ) -> Result<WorkspaceId, JobSubmissionError> {
        self.workspaces
            .workspaces()
            .get_or_register(root)
            .await
            .map(|record| record.id.clone())
            .map_err(|e| JobSubmissionError::Workspace(e.to_string()))
    }

    /// Resolve the current daemon-owned workspace path-policy identity for a
    /// root. Tool Program uses this at its submission boundary to reject a
    /// stale policy projection before persisting executable source or a job.
    pub async fn workspace_path_policy_fingerprint_for_root(
        &self,
        root: &Path,
    ) -> Result<String, JobSubmissionError> {
        let workspace_id = self.workspace_id_for_root(root).await?;
        let lease = self
            .workspaces
            .acquire(&workspace_id)
            .await
            .map_err(|e| JobSubmissionError::Workspace(e.to_string()))?;
        Ok(lease.path_policy().authority_fingerprint())
    }

    /// Fetch a durable job by id, if it exists.
    ///
    /// The schedule materializer uses this for the crash-replay path: an
    /// occurrence whose job row was written before the daemon stopped must
    /// not be materialized a second time. The job id is a pure function of
    /// the occurrence, so the lookup is exact.
    pub async fn existing_job(
        &self,
        job_id: &JobId,
    ) -> Result<Option<JobRecord>, JobSubmissionError> {
        Ok(self.store.get_job(job_id).await?)
    }

    pub async fn submit(
        &self,
        key: Option<SubmissionKey>,
        spec: NewJob,
    ) -> Result<SubmittedJob, JobSubmissionError> {
        self.submit_with_labels(key, spec, HashMap::new()).await
    }

    /// Submit a job with caller-owned, bounded metadata that must be
    /// available to the executor at first dispatch. These labels are
    /// persisted atomically with the job before it is enqueued.
    pub async fn submit_with_labels(
        &self,
        key: Option<SubmissionKey>,
        mut spec: NewJob,
        caller_labels: HashMap<String, String>,
    ) -> Result<SubmittedJob, JobSubmissionError> {
        if !self.scheduler.is_enabled() {
            return Err(JobSubmissionError::SchedulerDisabled);
        }

        validate_payload(spec.kind, &spec.payload)?;
        let encoded = serde_json::to_vec(&spec.payload)
            .map_err(|e| JobSubmissionError::InvalidPayload(e.to_string()))?;
        if encoded.len() > MAX_PAYLOAD_BYTES {
            return Err(JobSubmissionError::PayloadTooLarge);
        }

        // Acquiring and immediately dropping a lease validates registration
        // and canonical workspace identity without pinning a service bundle
        // for the lifetime of the job.
        let lease = self
            .workspaces
            .acquire(&spec.workspace_id)
            .await
            .map_err(|e| JobSubmissionError::Workspace(e.to_string()))?;
        let canonical_root = lease.path_policy().canonical_root.clone();
        drop(lease);

        // M005 corrective C005: a process-class job for a checkout reserved
        // by a live agent-run group is refused here, before the job is
        // durably created. A job that starts no process is not refused on
        // this ground: the refusal is about the *capability*, not about the
        // checkout.
        // c005-denial: scheduler-submission
        if is_process_class_job(spec.kind) {
            let service = self.process_admission.read().clone();
            if let Some(service) = service {
                let admission = service
                    .admit_scope(
                        &canonical_root,
                        codegg_core::process_admission::ProcessKind::ScheduledJob,
                    )
                    .await
                    .map_err(|error| {
                        JobSubmissionError::ProcessDenied(format!(
                            "the process admission authority could not be consulted: {error}"
                        ))
                    })?;
                if let Some(denial) = admission.denial() {
                    return Err(JobSubmissionError::ProcessDenied(denial.to_string()));
                }
            }
        }

        apply_resource_policy(&mut spec);
        if matches!(spec.kind, JobKind::Build | JobKind::Format)
            && self
                .scheduler
                .config()
                .quota
                .max_concurrent_builds_per_repository
                .is_some()
        {
            let resolver = self.context_resolver.read().clone().ok_or_else(|| {
                JobSubmissionError::RepositoryIdentityUnavailable(
                    "project binding authority is unavailable".into(),
                )
            })?;
            let repository_id = resolver
                .repository_for_workspace(&spec.workspace_id)
                .await
                .map_err(|error| {
                    JobSubmissionError::RepositoryIdentityUnavailable(error.to_string())
                })?
                .ok_or_else(|| {
                    JobSubmissionError::RepositoryIdentityUnavailable(
                        "workspace has no canonical repository identity".into(),
                    )
                })?;
            spec.resource_request.keyed_quotas.retain(|quota| {
                quota.namespace() != Some(ResourceClass::RepositoryBuild.namespace())
            });
            spec.resource_request
                .keyed_quotas
                .push(KeyedQuotaRequest::for_repository(
                    ResourceClass::RepositoryBuild,
                    &repository_id,
                ));
        }
        let mut labels = self.goal_provenance_labels(&spec).await?;
        for (name, value) in caller_labels {
            labels.entry(name).or_insert(value);
        }
        if labels.len() > MAX_SUBMISSION_LABELS
            || labels.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > MAX_LABEL_KEY_BYTES
                    || value.len() > MAX_LABEL_VALUE_BYTES
            })
        {
            return Err(JobSubmissionError::LabelsTooLarge);
        }
        let fingerprint = fingerprint(&spec);

        // Check the in-memory retry index without holding its lock across
        // durable reads. The durable job remains authoritative after return.
        if let Some(key_ref) = key.as_ref() {
            let indexed = {
                let idempotency = self.idempotency.lock().await;
                idempotency.get(key_ref).cloned()
            };
            if let Some(existing) = indexed {
                if existing.fingerprint != fingerprint {
                    return Err(JobSubmissionError::SubmissionKeyConflict);
                }
                if let Some(job) = self.store.get_job(&existing.job_id).await? {
                    return Ok(to_submitted(&job));
                }
                let mut idempotency = self.idempotency.lock().await;
                if idempotency
                    .get(key_ref)
                    .is_some_and(|current| current.job_id == existing.job_id)
                {
                    idempotency.remove(key_ref);
                }
            }

            // Rebuild the retry index after a daemon restart from the
            // durable payloads. The in-memory map is only a fast path; it
            // must not be the source of invocation identity.
            let existing_jobs = self
                .store
                .list_job_records(codegg_core::jobs::store::JobStoreQuery {
                    workspace_id: Some(spec.workspace_id.clone()),
                    limit: Some(256),
                    ..Default::default()
                })
                .await?;
            for existing_job in existing_jobs {
                if !payload_matches_submission_key(&existing_job.payload, key_ref.as_str()) {
                    continue;
                }
                if fingerprint_record(&existing_job) != fingerprint {
                    return Err(JobSubmissionError::SubmissionKeyConflict);
                }
                let mut idempotency = self.idempotency.lock().await;
                if let Some(current) = idempotency.get(key_ref) {
                    if current.fingerprint != fingerprint {
                        return Err(JobSubmissionError::SubmissionKeyConflict);
                    }
                } else {
                    idempotency.insert(
                        key_ref.clone(),
                        IdempotentSubmission {
                            fingerprint: fingerprint.clone(),
                            job_id: existing_job.job_id.clone(),
                        },
                    );
                }
                return Ok(to_submitted(&existing_job));
            }
        }

        // Serialize only the create/enqueue boundary for keyed submissions;
        // the potentially large durable scans above happen without this
        // mutex held.
        let mut idempotency = if key.is_some() {
            Some(self.idempotency.lock().await)
        } else {
            None
        };
        // Re-validate the retry index under the creation lock. Concurrent
        // submissions with the same key all miss the unlocked checks above
        // and serialize here; without this second check each waiter would
        // create its own durable job. See DVR M010.
        if let (Some(key_ref), Some(idempotency)) = (key.as_ref(), idempotency.as_mut()) {
            if let Some(existing) = idempotency.get(key_ref).cloned() {
                if existing.fingerprint != fingerprint {
                    return Err(JobSubmissionError::SubmissionKeyConflict);
                }
                if let Some(job) = self.store.get_job(&existing.job_id).await? {
                    return Ok(to_submitted(&job));
                }
                idempotency.remove(key_ref);
            }
        }
        let job = self.store.create_job_with_labels(spec, labels).await?;
        crate::test_failpoint::hit("tool_program_after_job_persist");
        if let Err(error) = self.scheduler.enqueue_existing(job.clone()).await {
            // Durable creation can succeed even when queue admission/wake-up
            // fails. Cancel the record before returning so a transport retry
            // cannot accidentally execute a job that the caller was told did
            // not submit successfully.
            let _ = self
                .store
                .request_cancel(
                    &job.job_id,
                    codegg_core::jobs::CancelReason::new("submission", "scheduler enqueue failed"),
                )
                .await;
            if let (Some(key), Some(idempotency)) = (key.as_ref(), idempotency.as_mut()) {
                idempotency.insert(
                    key.clone(),
                    IdempotentSubmission {
                        fingerprint,
                        job_id: job.job_id.clone(),
                    },
                );
            }
            return Err(error.into());
        }
        if let (Some(key), Some(idempotency)) = (key, idempotency.as_mut()) {
            idempotency.insert(
                key,
                IdempotentSubmission {
                    fingerprint,
                    job_id: job.job_id.clone(),
                },
            );
        }
        let created_sink = self.created_sink.read().clone();
        if let Some(sink) = created_sink {
            if let Err(error) = sink.send(job.clone()).await {
                tracing::warn!(job_id = %job.job_id, %error, "job lifecycle projection receiver is unavailable; snapshot rebuild will heal it");
            }
        }
        Ok(to_submitted(&job))
    }

    async fn goal_provenance_labels(
        &self,
        spec: &NewJob,
    ) -> Result<HashMap<String, String>, JobSubmissionError> {
        if !matches!(spec.kind, JobKind::Test | JobKind::Subagent) {
            return Ok(HashMap::new());
        }
        let Some(goal_store) = self.goal_store.as_ref() else {
            return Ok(HashMap::new());
        };
        let Some(session_id) = spec.session_id.as_deref() else {
            return Ok(HashMap::new());
        };
        let Some(goal) = goal_store
            .active_for_session(session_id)
            .await
            .map_err(|error| JobSubmissionError::Goal(error.to_string()))?
        else {
            return Ok(HashMap::new());
        };
        if !goal.is_active() {
            return Ok(HashMap::new());
        }
        let mut labels = HashMap::new();
        labels.insert(GOAL_PROVENANCE_LABEL_KEY.to_string(), goal.id);
        Ok(labels)
    }

    /// M002: reconcile a durable submission after a lost acknowledgement.
    ///
    /// Queries the durable job store by submission key instead of creating
    /// duplicate work. Returns the canonical durable job when the key is
    /// already present (in-memory fast path, then durable scan for
    /// post-restart recovery), or `None` when no job exists and the caller
    /// may proceed to [`Self::submit`]. A fingerprint mismatch is a
    /// [`JobSubmissionError::SubmissionKeyConflict`], never a silent
    /// duplicate. Scheduler-owned jobs retain their canonical
    /// attempt/reconciliation semantics; this helper never mutates them.
    pub async fn reconcile_by_key(
        &self,
        key: &SubmissionKey,
        workspace_id: &WorkspaceId,
        fingerprint_hint: Option<&str>,
    ) -> Result<Option<SubmittedJob>, JobSubmissionError> {
        {
            let idempotency = self.idempotency.lock().await;
            if let Some(existing) = idempotency.get(key).cloned() {
                if let Some(hint) = fingerprint_hint {
                    if existing.fingerprint != hint {
                        return Err(JobSubmissionError::SubmissionKeyConflict);
                    }
                }
                if let Some(job) = self.store.get_job(&existing.job_id).await? {
                    tracing::debug!(
                        submission_key = key.as_str(),
                        job_id = %job.job_id.as_str(),
                        "scheduler reconciliation hit in-memory index"
                    );
                    return Ok(Some(to_submitted(&job)));
                }
            }
        }
        let existing_jobs = self
            .store
            .list_job_records(codegg_core::jobs::store::JobStoreQuery {
                workspace_id: Some(workspace_id.clone()),
                limit: Some(256),
                ..Default::default()
            })
            .await?;
        for existing_job in existing_jobs {
            if !payload_matches_submission_key(&existing_job.payload, key.as_str()) {
                continue;
            }
            if let Some(hint) = fingerprint_hint {
                if fingerprint_record(&existing_job) != hint {
                    return Err(JobSubmissionError::SubmissionKeyConflict);
                }
            }
            tracing::debug!(
                submission_key = key.as_str(),
                job_id = %existing_job.job_id.as_str(),
                "scheduler reconciliation hit durable store"
            );
            return Ok(Some(to_submitted(&existing_job)));
        }
        Ok(None)
    }
}

fn to_submitted(job: &JobRecord) -> SubmittedJob {
    SubmittedJob {
        job_id: job.job_id.clone(),
        state: job.state,
        workspace_id: job.workspace_id.clone(),
        priority: job.priority,
    }
}

/// Match a durable payload against a caller submission key. Only payload
/// variants carrying a retry identity participate in post-restart
/// recovery: ToolProgram stores its key explicitly; Python submissions
/// derive `python:{source_hash}` at the call site.
fn payload_matches_submission_key(payload: &JobPayload, key: &str) -> bool {
    match payload {
        JobPayload::ToolProgram { submission_key, .. } => submission_key == key,
        JobPayload::SubagentRun { delegation_key, .. } => delegation_key == key,
        JobPayload::AgentTurn { submission_key, .. } => submission_key.as_deref() == Some(key),
        JobPayload::Python {
            source_hash: Some(hash),
            ..
        } => key == format!("python:{hash}"),
        _ => false,
    }
}

fn validate_payload(kind: JobKind, payload: &JobPayload) -> Result<(), JobSubmissionError> {
    let valid = match (kind, payload) {
        (JobKind::Test, JobPayload::Test { argv, .. }) => !argv.is_empty(),
        (
            JobKind::Build | JobKind::Lint | JobKind::Format,
            JobPayload::ManagedArgv { argv, .. },
        ) => !argv.is_empty(),
        (JobKind::AgentTurn, JobPayload::AgentTurn { prompt, agent, .. }) => {
            !prompt.is_empty() && !agent.is_empty()
        }
        (JobKind::Subagent, JobPayload::Subagent { .. })
        | (JobKind::Subagent, JobPayload::SubagentRun { .. })
        | (JobKind::Shell, JobPayload::Shell { .. })
        | (JobKind::Python, JobPayload::Python { .. })
        | (JobKind::Maintenance, JobPayload::Maintenance { .. }) => true,
        (JobKind::ManagedProcess, JobPayload::ManagedArgv { argv, .. }) => !argv.is_empty(),
        (JobKind::GitRead | JobKind::GitMutation, JobPayload::Git { argv, .. }) => !argv.is_empty(),
        (JobKind::ToolProgram, JobPayload::ToolProgram { program_id, .. }) => {
            !program_id.is_empty()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(JobSubmissionError::InvalidPayload(
            kind.as_str().to_string(),
        ))
    }
}

fn apply_resource_policy(spec: &mut NewJob) {
    if spec.resource_request == ResourceRequest::default() {
        spec.resource_request = ResourceRequest::for_kind(spec.kind);
    }
    // Legacy callers may carry the old unscoped names. Normalize them at the
    // single submission boundary so admission sees the intended keys.
    for key in &mut spec.resource_request.exclusivity_keys {
        if !key.starts_with("exclusive:") {
            *key = format!("exclusive:{key}");
        }
    }
    // M007: bind the resource class scope to the canonical workspace.
    //
    // The kind-derived policy names the class but cannot scope it —
    // the workspace is only known here. Binding it here is what makes
    // two unrelated workspaces independent: before M007 the build class
    // shared the *global* `exclusive:workspace-mutation` key, so a build
    // in one workspace excluded a build in every other workspace.
    //
    // The binding is applied unconditionally, not only for a defaulted
    // request: a caller that supplied its own resource request must not
    // be able to opt out of the bound it is measured against. The legacy
    // global key is dropped for the same reason — leaving it in place
    // would keep the global coupling this milestone removes.
    if let Some(class) = ResourceClass::for_job_kind(spec.kind) {
        spec.resource_request
            .exclusivity_keys
            .retain(|key| key != "exclusive:workspace-mutation");
        spec.resource_request
            .keyed_quotas
            .retain(|quota| quota.namespace() != Some(class.namespace()));
        spec.resource_request
            .keyed_quotas
            .push(KeyedQuotaRequest::for_workspace(class, &spec.workspace_id));
    }
}

fn fingerprint(spec: &NewJob) -> String {
    let payload = serde_json::to_string(&spec.payload).unwrap_or_else(|_| "<invalid>".into());
    format!(
        "{}|{}|{}|{}|{}|{:?}",
        spec.workspace_id,
        spec.kind.as_str(),
        spec.session_id.as_deref().unwrap_or_default(),
        spec.priority.as_str(),
        payload,
        spec.timeout
    )
}

fn fingerprint_record(job: &codegg_core::jobs::JobRecord) -> String {
    let payload = serde_json::to_string(&job.payload).unwrap_or_else(|_| "<invalid>".into());
    format!(
        "{}|{}|{}|{}|{}|{:?}",
        job.workspace_id,
        job.kind.as_str(),
        job.session_id.as_deref().unwrap_or_default(),
        job.priority.as_str(),
        payload,
        job.timeout
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobPayload, JobPriority, JobSource,
        JobStore, ResourceRequest, RetryPolicy, SqliteJobStore,
    };
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use std::sync::Arc;

    struct RepositoryQuotaGateExecutor {
        started: tokio::sync::mpsc::UnboundedSender<String>,
        release: Arc<tokio::sync::Semaphore>,
    }

    #[async_trait::async_trait]
    impl crate::scheduler::executor::JobExecutor for RepositoryQuotaGateExecutor {
        fn kind(&self) -> crate::scheduler::executor::ExecutorKind {
            crate::scheduler::executor::ExecutorKind::ManagedArgv
        }

        fn supports(&self, kind: JobKind) -> bool {
            kind == JobKind::Build
        }

        async fn execute(
            &self,
            ctx: crate::scheduler::executor::JobExecutionContext,
        ) -> crate::scheduler::executor::ExecutorCompletion {
            let _ = self.started.send(ctx.workspace_id.to_string());
            let _permit = self.release.clone().acquire_owned().await.unwrap();
            crate::scheduler::executor::ExecutorCompletion {
                status: crate::scheduler::executor::ExecutorStatus::Completed,
                summary: "repository quota gate released".into(),
                run_id: None,
                metrics: Default::default(),
            }
        }
    }

    async fn wait_for_job_terminal(store: &dyn JobStore, job_id: &JobId) {
        tokio::time::timeout(std::time::Duration::from_secs(180), async {
            loop {
                if store
                    .get_job(job_id)
                    .await
                    .expect("read submitted job")
                    .is_some_and(|job| job.state.is_terminal())
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("scheduler execution reaches terminal state");
    }

    async fn goal_test_pool() -> sqlx::SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:submission_goal_test_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let options = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            // The opt-in A004 matrix runs longer than SqlitePool's default
            // connection lifetime. Retiring its sole shared-memory SQLite
            // connection would discard the catalog schema mid-experiment.
            .min_connections(1)
            .max_lifetime(None)
            .idle_timeout(None)
            .connect_with(options)
            .await
            .expect("connect sqlite");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, '[]', ?, ?)",
        )
        .bind("project-1")
        .bind("/tmp")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert project");
        sqlx::query(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 'test', '/tmp', 'Test', '1', ?, ?)",
        )
        .bind("session-1")
        .bind("project-1")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert session");
        pool
    }

    #[tokio::test(flavor = "current_thread")]
    async fn opt_in_repository_quota_is_bound_from_catalog_not_request_metadata() {
        use std::process::Command;

        let root = tempfile::tempdir().expect("workspace");
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .status()
            .expect("git init")
            .success());
        std::fs::write(root.path().join("README.md"), "repository quota fixture\n")
            .expect("seed repository");
        assert!(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(root.path())
            .status()
            .expect("git add")
            .success());
        assert!(Command::new("git")
            .args([
                "-c",
                "user.name=CodeGG Test",
                "-c",
                "user.email=codegg-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ])
            .current_dir(root.path())
            .status()
            .expect("git commit")
            .success());
        assert!(Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://example.test/team/build.git"
            ])
            .current_dir(root.path())
            .status()
            .expect("git remote")
            .success());

        let pool = goal_test_pool().await;
        let workspace_store: Arc<dyn codegg_core::workspace::WorkspaceStore> = Arc::new(
            codegg_core::workspace::SqliteWorkspaceStore::new(pool.clone()),
        );
        let workspace_registry = WorkspaceRegistry::load(workspace_store.clone())
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        let project = catalog
            .register_local_project(
                codegg_core::project_catalog::RegisterLocalProject {
                    display_name: "Build repository".into(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &workspace.id,
                "scheduler-submission-test",
            )
            .await
            .expect("register project binding");
        let project_binding = codegg_core::project_storage::ProjectStorage::new(pool.clone())
            .workspace_binding(&workspace.id)
            .await
            .unwrap()
            .unwrap();
        let repository_id = project_binding.repository_id.unwrap();

        let checkout_parent = tempfile::tempdir().expect("worktree parent");
        let sibling_root = checkout_parent.path().join("sibling");
        assert!(Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&sibling_root)
            .arg("HEAD")
            .current_dir(root.path())
            .status()
            .expect("create sibling linked worktree")
            .success());
        let sibling_workspace = workspace_registry
            .get_or_register(&sibling_root)
            .await
            .expect("register sibling workspace");
        codegg_core::project_storage::ProjectStorage::new(pool.clone())
            .bind_checkout_workspace(
                &sibling_workspace,
                &project.project_id,
                &repository_id,
                &codegg_core::identity::WorktreeId::new(),
                "scheduler-submission-test",
            )
            .await
            .expect("bind sibling to the same repository lineage");
        let resolver = Arc::new(codegg_core::context::ProjectContextResolver::new(
            codegg_core::project_storage::ProjectStorage::new(pool.clone()),
            catalog,
            workspace_store,
        ));

        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.quota.max_concurrent_builds_per_repository = Some(1);
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            config,
            DaemonGeneration::new_unchecked("repository-quota-test"),
        );
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        scheduler
            .register_executor(Arc::new(RepositoryQuotaGateExecutor {
                started: started_tx,
                release: release.clone(),
            }))
            .await
            .expect("register gated build executor");
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler.clone(),
            services,
            DaemonGeneration::new_unchecked("repository-quota-test"),
        );
        submission.set_context_resolver(resolver.clone());

        let mut build = spec(workspace.id.clone());
        build.kind = JobKind::Build;
        build.payload = JobPayload::ManagedArgv {
            argv: vec!["cargo".into(), "check".into()],
            cwd: Some(root.path().to_string_lossy().into_owned()),
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        };
        // Caller-supplied requests and labels cannot choose or suppress the
        // host-derived repository scope.
        build.resource_request.keyed_quotas =
            vec![KeyedQuotaRequest::new("repository-build:fake", 1)];
        let submitted = submission.submit(None, build).await.expect("submit build");
        let persisted = store.get_job(&submitted.job_id).await.unwrap().unwrap();
        let resolved_repository_id = resolver
            .repository_for_workspace(&workspace.id)
            .await
            .unwrap()
            .unwrap();
        assert!(persisted.resource_request.keyed_quotas.iter().any(|quota| {
            quota.key == format!("repository-build:{}", resolved_repository_id.as_str())
        }));
        assert!(!persisted
            .resource_request
            .keyed_quotas
            .iter()
            .any(|quota| quota.key == "repository-build:fake"));

        let mut sibling_build = spec(sibling_workspace.id.clone());
        sibling_build.kind = JobKind::Build;
        sibling_build.payload = JobPayload::ManagedArgv {
            argv: vec!["cargo".into(), "check".into()],
            cwd: Some(sibling_root.to_string_lossy().into_owned()),
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        };
        let sibling_submission = submission
            .submit(None, sibling_build)
            .await
            .expect("submit sibling worktree build");
        let sibling_persisted = store
            .get_job(&sibling_submission.job_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            persisted
                .resource_request
                .keyed_quotas
                .iter()
                .find(|quota| quota.namespace() == Some("repository-build"))
                .map(|quota| quota.key.as_str()),
            sibling_persisted
                .resource_request
                .keyed_quotas
                .iter()
                .find(|quota| quota.namespace() == Some("repository-build"))
                .map(|quota| quota.key.as_str()),
            "distinct linked worktree workspaces must bind the same repository bucket"
        );
        assert_ne!(
            persisted.workspace_id, sibling_persisted.workspace_id,
            "the fixture must use distinct workspace identities"
        );

        scheduler.reconcile().await.expect("queue submitted builds");
        assert_eq!(
            Arc::clone(&scheduler).admit_and_dispatch_batch().await,
            1,
            "only one same-repository build may be admitted"
        );
        let first_workspace = started_rx
            .recv()
            .await
            .expect("first repository build starts");
        let first_job_id = if first_workspace == persisted.workspace_id.to_string() {
            submitted.job_id.clone()
        } else {
            sibling_submission.job_id.clone()
        };
        let second_job_id = if first_workspace == persisted.workspace_id.to_string() {
            sibling_submission.job_id.clone()
        } else {
            submitted.job_id.clone()
        };
        assert_eq!(
            Arc::clone(&scheduler).admit_and_dispatch_batch().await,
            0,
            "the sibling build remains queued while the repository slot is held"
        );
        assert!(
            scheduler.snapshot().await.admission_blocks.total > 0,
            "the real scheduler records the repository quota wait"
        );
        assert!(
            started_rx.try_recv().is_err(),
            "the second workspace must not reach its executor early"
        );

        release.add_permits(1);
        wait_for_job_terminal(&*store, &first_job_id).await;
        scheduler.reconcile().await.expect("requeue waiting build");
        assert_eq!(
            Arc::clone(&scheduler).admit_and_dispatch_batch().await,
            1,
            "the queued sibling is admitted after the first execution releases its slot"
        );
        let second_workspace = started_rx
            .recv()
            .await
            .expect("sibling repository build starts after release");
        assert_ne!(first_workspace, second_workspace);
        release.add_permits(1);
        wait_for_job_terminal(&*store, &second_job_id).await;
    }

    /// A003 composed measurement: real Rust builds must cross the normal
    /// submission boundary and execute through ManagedArgvExecutor. Separate
    /// target directories keep this experiment from authorizing a shared
    /// writable Cargo cache; the quota under study is scheduler admission.
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "A003 native production Cargo admission and resource measurement"]
    async fn repository_quota_controls_production_cargo_build_admission() {
        use std::process::Command;
        use std::time::Instant;

        let source_revision = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("read source revision");
        assert!(source_revision.status.success());
        let rustc_version = Command::new("rustc")
            .args(["--version", "--verbose"])
            .output()
            .expect("read rustc version");
        assert!(rustc_version.status.success());
        let cargo_version = Command::new("cargo")
            .arg("--version")
            .output()
            .expect("read cargo version");
        assert!(cargo_version.status.success());
        let host = Command::new("uname")
            .arg("-a")
            .output()
            .expect("read host facts");
        assert!(host.status.success());
        println!(
            "A003 host: source={} os={} rustc={} cargo={} target_dirs=fresh,separate dependencies=none,offline",
            String::from_utf8_lossy(&source_revision.stdout).trim(),
            String::from_utf8_lossy(&host.stdout).trim(),
            String::from_utf8_lossy(&rustc_version.stdout).trim().replace('\n', "; "),
            String::from_utf8_lossy(&cargo_version.stdout).trim(),
        );

        #[derive(Clone, Debug)]
        struct BuildCase {
            workspace_id: WorkspaceId,
            root: std::path::PathBuf,
            target: std::path::PathBuf,
            report: std::path::PathBuf,
            output: std::path::PathBuf,
        }

        fn init_rust_repository(root: &std::path::Path) {
            std::fs::create_dir_all(root.join("src")).expect("create fixture source");
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"admission-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )
            .expect("write fixture manifest");
            std::fs::write(
                root.join("Cargo.lock"),
                "version = 4\n\n[[package]]\nname = \"admission-fixture\"\nversion = \"0.1.0\"\n",
            )
            .expect("write fixture lockfile");
            let mut source = String::new();
            for index in 0..2_000_u32 {
                source.push_str(&format!(
                    "pub fn generated_{index}(value: u64) -> u64 {{ value.wrapping_mul({}).wrapping_add({index}) }}\n",
                    index + 3
                ));
            }
            std::fs::write(root.join("src/lib.rs"), source).expect("write Rust fixture");
            assert!(Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(root)
                .status()
                .expect("git init")
                .success());
            for (key, value) in [
                ("user.name", "CodeGG quota fixture"),
                ("user.email", "codegg-quota@example.invalid"),
            ] {
                assert!(Command::new("git")
                    .args(["config", key, value])
                    .current_dir(root)
                    .status()
                    .expect("git config")
                    .success());
            }
            assert!(Command::new("git")
                .args(["add", "."])
                .current_dir(root)
                .status()
                .expect("git add")
                .success());
            assert!(Command::new("git")
                .args(["commit", "--quiet", "-m", "Rust admission fixture"])
                .current_dir(root)
                .status()
                .expect("git commit")
                .success());
            let repository_name = root
                .file_name()
                .expect("fixture repository name")
                .to_string_lossy();
            assert!(Command::new("git")
                .args(["remote", "add", "origin"])
                .arg(format!("https://example.test/a003/{repository_name}.git"))
                .current_dir(root)
                .status()
                .expect("git remote add")
                .success());
        }

        fn target_bytes(root: &std::path::Path) -> u64 {
            let mut pending = vec![root.to_path_buf()];
            let mut total = 0_u64;
            while let Some(path) = pending.pop() {
                let Ok(entries) = std::fs::read_dir(path) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_dir() {
                            pending.push(path);
                        } else {
                            total = total.saturating_add(metadata.len());
                        }
                    }
                }
            }
            total
        }

        async fn run_build_pair(
            services: Arc<WorkspaceServiceRegistry>,
            resolver: Arc<codegg_core::context::ProjectContextResolver>,
            repository_quota: Option<u32>,
            cases: [BuildCase; 2],
            expect_single_admission: bool,
        ) -> (u128, Vec<String>, Vec<u64>) {
            let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
            let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
            config.quota.max_concurrent_builds_per_repository = repository_quota;
            config.resources.max_process_slots = 4;
            let generation = DaemonGeneration::new_unchecked(format!(
                "repository-build-measurement-{}",
                uuid::Uuid::new_v4()
            ));
            let scheduler =
                JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
            scheduler
                .register_executor(Arc::new(
                    crate::scheduler::executors::ManagedArgvExecutor::new("a003_cargo"),
                ))
                .await
                .expect("register production managed process executor");
            let submission =
                JobSubmissionService::new(store.clone(), scheduler.clone(), services, generation);
            submission.set_context_resolver(resolver);
            let mut job_ids = Vec::new();
            for (ordinal, case) in cases.iter().enumerate() {
                let mut build = spec(case.workspace_id.clone());
                build.kind = JobKind::Build;
                build.resource_request = ResourceRequest::for_kind(JobKind::Build);
                build.payload = JobPayload::ManagedArgv {
                    argv: vec![
                        "/usr/bin/time".into(),
                        "-f".into(),
                        "wall=%e user=%U system=%S maxrss_kb=%M".into(),
                        "-o".into(),
                        case.report.to_string_lossy().into_owned(),
                        "/usr/bin/env".into(),
                        format!("CARGO_TARGET_DIR={}", case.target.display()),
                        "CARGO_BUILD_JOBS=2".into(),
                        "CARGO_TERM_COLOR=never".into(),
                        "/bin/sh".into(),
                        "-c".into(),
                        "exec cargo check --locked --offline > \"$1\" 2>&1".into(),
                        "a003-cargo".into(),
                        case.output.to_string_lossy().into_owned(),
                    ],
                    cwd: Some(case.root.to_string_lossy().into_owned()),
                    sandbox_profile: None,
                    network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
                };
                build.idempotency = IdempotencyClass::SafeRepeat;
                build.priority = JobPriority::Interactive;
                build.source = JobSource::Interactive;
                build.timeout = Some(std::time::Duration::from_secs(180));
                let submitted = submission
                    .submit(None, build)
                    .await
                    .unwrap_or_else(|error| panic!("submit Cargo build {ordinal}: {error}"));
                job_ids.push(submitted.job_id);
            }

            scheduler.reconcile().await.expect("queue measured builds");
            let started = Instant::now();
            let first_admission = Arc::clone(&scheduler).admit_and_dispatch_batch().await;
            if expect_single_admission {
                assert_eq!(first_admission, 1, "same-repository quota admits one build");
                let mut first_id = None;
                for id in &job_ids {
                    if store
                        .get_job(id)
                        .await
                        .unwrap()
                        .is_some_and(|job| job.state != codegg_core::jobs::JobState::Queued)
                    {
                        first_id = Some(id.clone());
                        break;
                    }
                }
                let first_id = first_id.expect("one build is admitted");
                wait_for_job_terminal(&*store, &first_id).await;
                scheduler.reconcile().await.expect("requeue quota waiter");
                assert_eq!(
                    Arc::clone(&scheduler).admit_and_dispatch_batch().await,
                    1,
                    "waiting sibling resumes after the first Cargo process exits"
                );
                let second_id = job_ids.iter().find(|id| **id != first_id).unwrap();
                wait_for_job_terminal(&*store, second_id).await;
            } else {
                assert_eq!(first_admission, 2, "independent builds dispatch together");
                for job_id in &job_ids {
                    wait_for_job_terminal(&*store, job_id).await;
                }
            }
            let wall_ms = started.elapsed().as_millis();
            let mut reports = Vec::new();
            let mut target_sizes = Vec::new();
            let mut cargo_lock_waits = Vec::new();
            for (case, job_id) in cases.iter().zip(job_ids.iter()) {
                let job = store.get_job(job_id).await.unwrap().unwrap();
                assert_eq!(
                    job.state,
                    codegg_core::jobs::JobState::Completed,
                    "{job:#?}"
                );
                let report = std::fs::read_to_string(&case.report).expect("GNU time report");
                for field in ["wall=", "user=", "system=", "maxrss_kb="] {
                    assert!(
                        report.contains(field),
                        "incomplete resource report: {report}"
                    );
                }
                reports.push(report);
                let output = std::fs::read_to_string(&case.output).expect("Cargo output log");
                assert!(
                    !output.is_empty(),
                    "Cargo output log stayed empty: {case:#?}"
                );
                cargo_lock_waits.push(
                    output
                        .lines()
                        .filter(|line| line.contains("Blocking waiting for file lock"))
                        .count(),
                );
                let bytes = target_bytes(&case.target);
                assert!(bytes > 0, "Cargo target directory stayed empty: {case:#?}");
                target_sizes.push(bytes);
            }
            scheduler
                .shutdown(crate::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
                .await;
            println!("Cargo lock waits per job: {cargo_lock_waits:?}");
            (wall_ms, reports, target_sizes)
        }

        let fixture = tempfile::tempdir().expect("measurement fixture");
        let repository = fixture.path().join("same-repository");
        init_rust_repository(&repository);
        let sibling_root = fixture.path().join("sibling-worktree");
        assert!(Command::new("git")
            .args(["worktree", "add", "--quiet", "--detach"])
            .arg(&sibling_root)
            .arg("HEAD")
            .current_dir(&repository)
            .status()
            .expect("create sibling build worktree")
            .success());
        let unrelated_root = fixture.path().join("unrelated-repository");
        init_rust_repository(&unrelated_root);

        let pool = goal_test_pool().await;
        let workspace_store: Arc<dyn codegg_core::workspace::WorkspaceStore> = Arc::new(
            codegg_core::workspace::SqliteWorkspaceStore::new(pool.clone()),
        );
        let workspace_registry = WorkspaceRegistry::load(workspace_store.clone())
            .await
            .expect("workspace registry");
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        let project_storage = codegg_core::project_storage::ProjectStorage::new(pool.clone());
        let same_workspace = workspace_registry
            .get_or_register(&repository)
            .await
            .expect("register source workspace");
        let same_project = catalog
            .register_local_project(
                codegg_core::project_catalog::RegisterLocalProject {
                    display_name: "A003 same repository".into(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &same_workspace.id,
                "a003-production-build-measurement",
            )
            .await
            .expect("register source repository");
        let same_binding = project_storage
            .workspace_binding(&same_workspace.id)
            .await
            .unwrap()
            .unwrap();
        let repository_id = same_binding.repository_id.unwrap();
        let sibling_workspace = workspace_registry
            .get_or_register(&sibling_root)
            .await
            .expect("register sibling workspace");
        project_storage
            .bind_checkout_workspace(
                &sibling_workspace,
                &same_project.project_id,
                &repository_id,
                &codegg_core::identity::WorktreeId::new(),
                "a003-production-build-measurement",
            )
            .await
            .expect("bind sibling to source repository");
        let unrelated_workspace = workspace_registry
            .get_or_register(&unrelated_root)
            .await
            .expect("register unrelated workspace");
        catalog
            .register_local_project(
                codegg_core::project_catalog::RegisterLocalProject {
                    display_name: "A003 unrelated repository".into(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &unrelated_workspace.id,
                "a003-production-build-measurement",
            )
            .await
            .expect("register unrelated repository");
        let resolver = Arc::new(codegg_core::context::ProjectContextResolver::new(
            project_storage,
            catalog,
            workspace_store,
        ));
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let target_root = fixture.path().join("targets");
        std::fs::create_dir_all(&target_root).expect("create isolated target roots");
        let make_cases =
            |workspaces: [WorkspaceId; 2], roots: [std::path::PathBuf; 2], tag: &str| {
                std::array::from_fn(|index| BuildCase {
                    workspace_id: workspaces[index].clone(),
                    root: roots[index].clone(),
                    target: target_root.join(format!("{tag}-{index}")),
                    report: fixture.path().join(format!("{tag}-{index}.time")),
                    output: fixture.path().join(format!("{tag}-{index}.cargo.log")),
                })
            };

        let baseline_cases = make_cases(
            [same_workspace.id.clone(), sibling_workspace.id.clone()],
            [repository.clone(), sibling_root.clone()],
            "quota-disabled-same-repository",
        );
        let (baseline_wall_ms, baseline_reports, baseline_targets) = run_build_pair(
            services.clone(),
            resolver.clone(),
            None,
            baseline_cases,
            false,
        )
        .await;
        let limited_cases = make_cases(
            [same_workspace.id.clone(), sibling_workspace.id.clone()],
            [repository.clone(), sibling_root.clone()],
            "quota-one-same-repository",
        );
        let (limited_wall_ms, limited_reports, limited_targets) = run_build_pair(
            services.clone(),
            resolver.clone(),
            Some(1),
            limited_cases,
            true,
        )
        .await;
        let unrelated_cases = make_cases(
            [same_workspace.id.clone(), unrelated_workspace.id.clone()],
            [repository, unrelated_root],
            "quota-one-unrelated-repositories",
        );
        let (unrelated_wall_ms, unrelated_reports, unrelated_targets) =
            run_build_pair(services, resolver, Some(1), unrelated_cases, false).await;

        println!("A003 production Cargo admission, Linux, CARGO_BUILD_JOBS=2");
        println!("quota disabled, sibling worktrees: wall={baseline_wall_ms}ms reports={baseline_reports:?} target_bytes={baseline_targets:?}");
        println!("repository quota=1, sibling worktrees: wall={limited_wall_ms}ms reports={limited_reports:?} target_bytes={limited_targets:?}");
        println!("repository quota=1, unrelated repositories: wall={unrelated_wall_ms}ms reports={unrelated_reports:?} target_bytes={unrelated_targets:?}");
    }

    /// A004's explicitly enabled, resource-intensive compiler-cache pilot.
    /// The real repository is checked from two detached sibling worktrees,
    /// each job gets its own target directory, and both quota policies cross
    /// JobSubmissionService -> JobScheduler -> ManagedArgvExecutor.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "A004 opt-in real Cargo/sccache economics pilot"]
    async fn a004_cargo_economics_through_production_scheduler() {
        use std::process::Command;
        use std::time::Instant;

        if std::env::var("CODEGG_REGENERATE_ASSETS").as_deref() != Ok("1") {
            panic!("set CODEGG_REGENERATE_ASSETS=1 to authorize Git worktree metadata writes");
        }

        fn directory_bytes(root: &std::path::Path) -> u64 {
            let mut pending = vec![root.to_path_buf()];
            let mut total = 0_u64;
            while let Some(path) = pending.pop() {
                let Ok(entries) = std::fs::read_dir(path) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_dir() {
                            pending.push(path);
                        } else {
                            total = total.saturating_add(metadata.len());
                        }
                    }
                }
            }
            total
        }

        if std::env::var_os("CODEGG_RUN_A004_CARGO_ECONOMICS").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            panic!("set CODEGG_RUN_A004_CARGO_ECONOMICS=1 to authorize the bounded pilot");
        }
        let sccache = std::env::var_os("CODEGG_A004_SCCACHE")
            .map(std::path::PathBuf::from)
            .expect("CODEGG_A004_SCCACHE must name the pinned sccache executable");
        let cargo = std::env::var_os("CODEGG_A004_CARGO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "cargo".into());
        let rustc = std::env::var_os("CODEGG_A004_RUSTC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "rustc".into());
        let version = Command::new(&sccache)
            .arg("--version")
            .output()
            .expect("run pinned sccache version check");
        assert!(version.status.success());
        let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
        assert_eq!(
            version, "sccache 0.18.0",
            "A004 cache binary must be pinned"
        );
        let cargo_version = Command::new(&cargo)
            .arg("--version")
            .output()
            .expect("record pinned Cargo version");
        assert!(cargo_version.status.success());
        let rustc_version = Command::new(&rustc)
            .args(["--version", "--verbose"])
            .output()
            .expect("record pinned rustc version");
        assert!(rustc_version.status.success());

        let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let revision = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repository)
            .output()
            .expect("read frozen source revision");
        assert!(revision.status.success());
        let revision = String::from_utf8_lossy(&revision.stdout).trim().to_owned();
        let fixture = tempfile::tempdir().expect("A004 owned scratch root");
        let first_root = fixture.path().join("sibling-a");
        let second_root = fixture.path().join("sibling-b");
        for path in [&first_root, &second_root] {
            let output = Command::new("git")
                .args(["worktree", "add", "--quiet", "--detach"])
                .arg(path)
                .arg(&revision)
                .current_dir(&repository)
                .output()
                .expect("create frozen sibling worktree");
            assert!(
                output.status.success(),
                "create A004 worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        struct RemoveA004Worktrees {
            repository: std::path::PathBuf,
            worktrees: [std::path::PathBuf; 2],
        }
        impl Drop for RemoveA004Worktrees {
            fn drop(&mut self) {
                for worktree in &self.worktrees {
                    let _ = Command::new("git")
                        .args(["worktree", "remove", "--force"])
                        .arg(worktree)
                        .current_dir(&self.repository)
                        .status();
                }
                let _ = Command::new("git")
                    .args(["worktree", "prune"])
                    .current_dir(&self.repository)
                    .status();
            }
        }
        let _remove_worktrees = RemoveA004Worktrees {
            repository: repository.clone(),
            worktrees: [first_root.clone(), second_root.clone()],
        };
        let cache_root = fixture.path().join("sccache");
        std::fs::create_dir_all(&cache_root).expect("create task-owned cache");
        let cache_env = [
            ("SCCACHE_DIR", cache_root.to_string_lossy().into_owned()),
            ("SCCACHE_CONF", "/dev/null".to_owned()),
            ("SCCACHE_CACHE_SIZE", "3G".to_owned()),
            (
                "SCCACHE_BASEDIRS",
                format!("{}:{}", first_root.display(), second_root.display()),
            ),
        ];
        let started_server = Command::new(&sccache)
            .arg("--start-server")
            .envs(cache_env.iter().cloned())
            .output()
            .expect("start task-scoped local sccache server");
        assert!(
            started_server.status.success(),
            "sccache server unavailable: {}",
            String::from_utf8_lossy(&started_server.stderr)
        );
        struct StopSccache {
            binary: std::path::PathBuf,
            cache: std::path::PathBuf,
        }
        impl Drop for StopSccache {
            fn drop(&mut self) {
                let _ = Command::new(&self.binary)
                    .arg("--stop-server")
                    .env("SCCACHE_DIR", &self.cache)
                    .status();
            }
        }
        let _stop_server = StopSccache {
            binary: sccache.clone(),
            cache: cache_root.clone(),
        };
        let sccache_stats_before = Command::new(&sccache)
            .arg("--zero-stats")
            .envs(cache_env.iter().cloned())
            .output()
            .expect("reset scoped sccache counters");
        assert!(sccache_stats_before.status.success());

        let pool = goal_test_pool().await;
        let workspace_store: Arc<dyn codegg_core::workspace::WorkspaceStore> = Arc::new(
            codegg_core::workspace::SqliteWorkspaceStore::new(pool.clone()),
        );
        let workspace_registry = WorkspaceRegistry::load(workspace_store.clone())
            .await
            .expect("workspace registry");
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        let project_storage = codegg_core::project_storage::ProjectStorage::new(pool.clone());
        let source_workspace = workspace_registry
            .get_or_register(&repository)
            .await
            .expect("register source workspace");
        let project = catalog
            .register_local_project(
                codegg_core::project_catalog::RegisterLocalProject {
                    display_name: "A004 compiler cache pilot".into(),
                    description: None,
                    tags: Vec::new(),
                    primary_repository_id: None,
                },
                &source_workspace.id,
                "a004-cargo-economics",
            )
            .await
            .expect("register source repository");
        let source_binding = project_storage
            .workspace_binding(&source_workspace.id)
            .await
            .unwrap()
            .unwrap();
        let repository_id = source_binding.repository_id.unwrap();
        let mut sibling_workspaces = Vec::new();
        for path in [&first_root, &second_root] {
            let workspace = workspace_registry
                .get_or_register(path)
                .await
                .expect("register sibling checkout");
            project_storage
                .bind_checkout_workspace(
                    &workspace,
                    &project.project_id,
                    &repository_id,
                    &codegg_core::identity::WorktreeId::new(),
                    "a004-cargo-economics",
                )
                .await
                .expect("bind sibling to source repository");
            sibling_workspaces.push(workspace.id.clone());
        }
        let resolver = Arc::new(codegg_core::context::ProjectContextResolver::new(
            project_storage,
            catalog,
            workspace_store,
        ));
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let mut rows = Vec::new();
        let target_root = fixture.path().join("targets");
        std::fs::create_dir_all(&target_root).expect("create target root");
        let measurement_wrapper = fixture.path().join("measure.py");
        std::fs::write(
            &measurement_wrapper,
            "import json, resource, subprocess, sys, time\n"
                .to_owned()
                + "report, log = sys.argv[1:3]\n"
                + "start = time.perf_counter()\n"
                + "with open(log, 'wb') as output: code = subprocess.run(sys.argv[3:], stdout=output, stderr=subprocess.STDOUT).returncode\n"
                + "usage = resource.getrusage(resource.RUSAGE_CHILDREN)\n"
                + "with open(report, 'w') as output: json.dump({'wall_s': time.perf_counter()-start, 'user_s': usage.ru_utime, 'system_s': usage.ru_stime, 'maxrss': usage.ru_maxrss}, output)\n"
                + "sys.exit(code)\n",
        )
        .expect("write resource measurement wrapper");

        let arms = [
            (false, None),
            (true, None),
            (false, Some(1)),
            (true, Some(1)),
        ];
        for repetition in 0..3_u8 {
            let mut seed = usize::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock after epoch")
                    .as_nanos(),
            )
            .expect("nanosecond seed fits this host")
                ^ usize::from(repetition);
            let mut arm_order = [0, 1, 2, 3];
            for index in (1..arm_order.len()).rev() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                arm_order.swap(index, seed % (index + 1));
            }
            println!("A004_ARM_ORDER repetition={repetition} order={arm_order:?}");
            for arm_index in arm_order {
                let (cache_enabled, repository_quota) = arms[arm_index];
                let pair_tag = format!(
                    "r{repetition}-cache{}-quota{}",
                    u8::from(cache_enabled),
                    repository_quota.unwrap_or(0)
                );
                let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
                let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
                config.quota.max_concurrent_builds_per_repository = repository_quota;
                config.resources.max_process_slots = 4;
                let generation = DaemonGeneration::new_unchecked(format!(
                    "a004-{pair_tag}-{}",
                    uuid::Uuid::new_v4()
                ));
                let scheduler =
                    JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
                scheduler
                    .register_executor(Arc::new(
                        crate::scheduler::executors::ManagedArgvExecutor::new("a004_cargo"),
                    ))
                    .await
                    .expect("register production managed argv executor");
                let submission = JobSubmissionService::new(
                    store.clone(),
                    scheduler.clone(),
                    services.clone(),
                    generation,
                );
                submission.set_context_resolver(resolver.clone());
                let mut job_ids = Vec::new();
                let mut reports = Vec::new();
                let mut logs = Vec::new();
                let pair_started = Instant::now();
                for index in 0..2 {
                    let worktree_index = usize::from((index + repetition) % 2);
                    let target = target_root.join(format!("{pair_tag}-{index}"));
                    let report = fixture.path().join(format!("{pair_tag}-{index}.json"));
                    let log = fixture.path().join(format!("{pair_tag}-{index}.log"));
                    let mut argv = vec![
                        "python3".to_owned(),
                        measurement_wrapper.to_string_lossy().into_owned(),
                        report.to_string_lossy().into_owned(),
                        log.to_string_lossy().into_owned(),
                        cargo.to_string_lossy().into_owned(),
                        "check".into(),
                        "--locked".into(),
                        "--offline".into(),
                        "-p".into(),
                        "codegg-core".into(),
                    ];
                    let mut env = vec![
                        format!("CARGO_TARGET_DIR={}", target.display()),
                        "CARGO_INCREMENTAL=0".into(),
                        "CARGO_BUILD_JOBS=2".into(),
                        "CARGO_TERM_COLOR=never".into(),
                        format!("RUSTC={}", rustc.display()),
                    ];
                    if cache_enabled {
                        env.extend(
                            cache_env
                                .iter()
                                .map(|(key, value)| format!("{key}={value}")),
                        );
                        env.push(format!("RUSTC_WRAPPER={}", sccache.display()));
                        // Client-side mode keeps compiler CPU and RSS under the
                        // managed Cargo process tree, so the resource wrapper
                        // measures compiler work rather than only the sccache
                        // server client.
                        env.push("SCCACHE_CLIENT_SIDE=1".into());
                    } else {
                        // An explicit empty value disables any user Cargo wrapper
                        // only within this opt-in experiment process.
                        env.push("RUSTC_WRAPPER=".into());
                    }
                    let mut command = vec!["/usr/bin/env".into()];
                    if !cache_enabled {
                        command.extend(["-u".into(), "SCCACHE_CLIENT_SIDE".into()]);
                    }
                    command.append(&mut env);
                    command.append(&mut argv);
                    let mut build = spec(sibling_workspaces[worktree_index].clone());
                    build.kind = JobKind::Build;
                    build.resource_request = ResourceRequest::for_kind(JobKind::Build);
                    build.payload = JobPayload::ManagedArgv {
                        argv: command,
                        cwd: Some(
                            [first_root.clone(), second_root.clone()][worktree_index]
                                .to_string_lossy()
                                .into_owned(),
                        ),
                        sandbox_profile: None,
                        network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
                    };
                    build.idempotency = IdempotencyClass::SafeRepeat;
                    build.timeout = Some(std::time::Duration::from_secs(900));
                    let submitted = submission
                        .submit(None, build)
                        .await
                        .unwrap_or_else(|error| panic!("submit A004 Cargo job: {error}"));
                    job_ids.push(submitted.job_id);
                    reports.push(report);
                    logs.push(log);
                }
                let scheduler_task = scheduler.clone().spawn_run();
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1800);
                loop {
                    let mut terminal = true;
                    for job_id in &job_ids {
                        let record = store
                            .get_job(job_id)
                            .await
                            .expect("read A004 job")
                            .expect("submitted A004 job exists");
                        if !record.state.is_terminal() {
                            terminal = false;
                        }
                    }
                    if terminal {
                        break;
                    }
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "A004 pair timed out: {pair_tag}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                let mut pair_rows = Vec::new();
                for index in 0..2 {
                    let record = store.get_job(&job_ids[index]).await.unwrap().unwrap();
                    assert_eq!(
                        record.state,
                        JobState::Completed,
                        "A004 Cargo failure: {record:#?}"
                    );
                    let measurement: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(&reports[index]).expect("read resource measurement"),
                    )
                    .expect("parse resource measurement");
                    let output = std::fs::read_to_string(&logs[index]).expect("read Cargo log");
                    assert!(
                        output.contains("Finished"),
                        "Cargo did not report completion: {output}"
                    );
                    let target = target_root.join(format!("{pair_tag}-{index}"));
                    let target_bytes = directory_bytes(&target);
                    assert!(target_bytes > 0, "Cargo target directory was empty");
                    let row = serde_json::json!({
                        "cache_enabled": cache_enabled,
                        "repository_quota": repository_quota,
                        "repetition": repetition,
                        "worktree": index,
                        "wall_s": measurement["wall_s"],
                        "user_s": measurement["user_s"],
                        "system_s": measurement["system_s"],
                        "maxrss": measurement["maxrss"],
                        "target_bytes": target_bytes,
                        "cargo_lock_waits": output.lines().filter(|line| line.contains("Blocking waiting for file lock")).count(),
                        "cargo_finished_line": output.lines().find(|line| line.contains("Finished")).unwrap_or_default(),
                    });
                    println!("A004_SAMPLE {row}");
                    pair_rows.push(row);
                }
                println!(
                    "A004_PAIR {{\"tag\":\"{pair_tag}\",\"scheduler_wall_ms\":{},\"rows\":{}}}",
                    pair_started.elapsed().as_millis(),
                    serde_json::Value::Array(pair_rows.clone())
                );
                scheduler
                    .shutdown(crate::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
                    .await;
                scheduler_task.await.expect("join A004 scheduler");
                rows.extend(pair_rows);
            }
        }
        let stats = Command::new(&sccache)
            .arg("--show-stats")
            .envs(cache_env.iter().cloned())
            .output()
            .expect("read scoped sccache counters");
        assert!(stats.status.success());
        println!(
            "A004_HOST source={revision} sccache={version} cargo={} rustc={} host={} rows={} cache_bytes={} stats:\n{}",
            String::from_utf8_lossy(&cargo_version.stdout).trim(),
            String::from_utf8_lossy(&rustc_version.stdout)
                .trim()
                .replace('\n', "; "),
            Command::new("uname")
                .arg("-a")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default(),
            rows.len(),
            directory_bytes(&cache_root),
            String::from_utf8_lossy(&stats.stdout)
        );
        assert_eq!(rows.len(), 24, "three paired samples per four policy arms");
        let _ = Command::new(&sccache)
            .arg("--stop-server")
            .envs(cache_env.iter().cloned())
            .status();
    }

    fn spec(workspace_id: WorkspaceId) -> NewJob {
        NewJob {
            workspace_id,
            session_id: None,
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::Test {
                command: "echo ok".into(),
                argv: vec!["echo".into(), "ok".into()],
                cwd: Some("/tmp".into()),
                scope: None,
                parent_run_id: None,
                sandbox_profile: None,
                network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: ExecutionTarget::default(),
        }
    }

    #[test]
    fn submission_keys_are_bounded() {
        assert!(SubmissionKey::new("").is_err());
        assert!(SubmissionKey::new("x".repeat(MAX_SUBMISSION_KEY_BYTES + 1)).is_err());
        assert!(SubmissionKey::new("request-1").is_ok());
    }

    #[test]
    fn legacy_exclusivity_names_are_normalized() {
        let ws = WorkspaceId::new_unchecked("ws");
        let mut value = spec(ws);
        value.resource_request.exclusivity_keys = vec!["workspace-mutation".into()];
        apply_resource_policy(&mut value);
        assert_eq!(
            value.resource_request.exclusivity_keys,
            vec!["exclusive:workspace-mutation"]
        );
    }

    #[test]
    fn resource_profiles_are_centralized_and_nonzero() {
        for kind in [
            JobKind::Test,
            JobKind::Build,
            JobKind::Lint,
            JobKind::Format,
            JobKind::Subagent,
            JobKind::GitMutation,
        ] {
            let profile = ResourceRequest::for_kind(kind);
            assert!(profile.cpu_weight > 0, "{kind:?} must not be zero-cost");
            assert!(profile.io_weight > 0, "{kind:?} must reserve IO");
            assert!(profile.process_slots > 0, "{kind:?} must reserve a process");
        }
        // M007: the build class no longer carries the *global*
        // `exclusive:workspace-mutation` key. That key serialized every
        // build in every workspace; it is replaced by the counted,
        // workspace-scoped build quota below.
        assert!(!ResourceRequest::for_kind(JobKind::Build)
            .exclusivity_keys
            .iter()
            .any(|key| key == "exclusive:workspace-mutation"));
        let build = ResourceRequest::for_kind(JobKind::Build);
        assert_eq!(build.keyed_quotas.len(), 1);
        assert_eq!(build.keyed_quotas[0].namespace(), Some("build"));
        assert_eq!(build.keyed_quotas[0].scope(), "");
        assert!(ResourceRequest::for_kind(JobKind::GitMutation)
            .exclusivity_keys
            .iter()
            .any(|key| key == "exclusive:worktree-mutation"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_scheduler_rejects_submission() {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let config = crate::scheduler::config::ResolvedSchedulerConfig {
            enabled: false,
            ..crate::scheduler::config::ResolvedSchedulerConfig::default()
        };
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            config,
            DaemonGeneration::new_unchecked("disabled-generation"),
        );
        let submission = JobSubmissionService::new(
            store,
            scheduler,
            services,
            DaemonGeneration::new_unchecked("disabled-generation"),
        );
        let error = submission
            .submit(None, spec(workspace.id.clone()))
            .await
            .expect_err("disabled daemon must reject heavy work");
        assert!(matches!(error, JobSubmissionError::SchedulerDisabled));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn same_submission_key_creates_one_durable_job() {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services,
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );
        let (created_tx, mut created_rx) = tokio::sync::mpsc::channel(8);
        submission.set_created_sink(created_tx);
        let key = SubmissionKey::new("retry-1").expect("key");
        let mut first_spec = spec(workspace.id.clone());
        first_spec.session_id = Some("owner-session".into());
        let first = submission
            .submit(Some(key.clone()), first_spec)
            .await
            .expect("first submission");
        let mut retry_spec = spec(workspace.id.clone());
        retry_spec.session_id = Some("owner-session".into());
        let second = submission
            .submit(Some(key), retry_spec)
            .await
            .expect("retry submission");

        assert_eq!(first.job_id, second.job_id);
        let published = created_rx
            .recv()
            .await
            .expect("post-admission lifecycle event");
        assert_eq!(published.job_id, first.job_id);
        assert_eq!(published.session_id.as_deref(), Some("owner-session"));
        assert!(
            created_rx.try_recv().is_err(),
            "idempotent retry must not republish create"
        );
        let jobs = store
            .list_jobs(codegg_core::jobs::store::JobStoreQuery::default())
            .await
            .expect("list jobs");
        assert_eq!(jobs.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn active_goal_provenance_is_host_written_for_test_and_subagent_jobs() {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let pool = goal_test_pool().await;
        let goal = codegg_core::goal::GoalStore::new(pool.clone())
            .create_active(
                "session-1",
                "project-1",
                "Goal",
                "Do work",
                None,
                None,
                Vec::new(),
            )
            .await
            .expect("create goal");
        let store: Arc<dyn JobStore> = Arc::new(SqliteJobStore::new(pool.clone()));
        let generation = DaemonGeneration::new_unchecked("goal-provenance-generation");
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
            generation.clone(),
        );
        let submission = JobSubmissionService::new_with_goal_store(
            store.clone(),
            scheduler,
            services,
            generation,
            Arc::new(codegg_core::goal::GoalStore::new(pool.clone())),
        );

        let mut test_spec = spec(workspace.id.clone());
        test_spec.session_id = Some("session-1".to_string());
        let test_job = submission
            .submit(None, test_spec)
            .await
            .expect("submit test");
        let test_record = store
            .get_job(&test_job.job_id)
            .await
            .expect("get test")
            .expect("test exists");
        assert_eq!(
            test_record.labels.get(GOAL_PROVENANCE_LABEL_KEY),
            Some(&goal.id)
        );

        let mut subagent_spec = spec(workspace.id.clone());
        subagent_spec.session_id = Some("session-1".to_string());
        subagent_spec.kind = JobKind::Subagent;
        subagent_spec.payload = JobPayload::Subagent {
            prompt: "inspect".into(),
            agent: "reviewer".into(),
            model: None,
            parent_id: Some("session-1".into()),
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            max_tool_calls: None,
        };
        let subagent_job = submission
            .submit(None, subagent_spec)
            .await
            .expect("submit subagent");
        let subagent_record = store
            .get_job(&subagent_job.job_id)
            .await
            .expect("get subagent")
            .expect("subagent exists");
        assert_eq!(
            subagent_record.labels.get(GOAL_PROVENANCE_LABEL_KEY),
            Some(&goal.id)
        );

        codegg_core::goal::GoalStore::new(pool.clone())
            .clear_active_for_session("session-1")
            .await
            .expect("clear goal");
        let mut no_goal_spec = spec(workspace.id.clone());
        no_goal_spec.session_id = Some("session-1".to_string());
        let no_goal_job = submission
            .submit(None, no_goal_spec)
            .await
            .expect("submit without goal");
        let no_goal_record = store
            .get_job(&no_goal_job.job_id)
            .await
            .expect("get no-goal job")
            .expect("no-goal job exists");
        assert!(!no_goal_record
            .labels
            .contains_key(GOAL_PROVENANCE_LABEL_KEY));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn python_submission_key_recovers_after_restart() {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services.clone(),
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );

        let source_hash = "abc123".to_string();
        let mut python_spec = spec(workspace.id.clone());
        python_spec.kind = JobKind::Python;
        python_spec.payload = JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "analyze".into(),
            source: Some("print(1)".into()),
            source_hash: Some(source_hash.clone()),
            cwd: Some("/tmp".into()),
            timeout_secs: None,
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        };

        let key = SubmissionKey::new(format!("python:{source_hash}")).expect("key");
        let first = submission
            .submit(Some(key.clone()), python_spec.clone())
            .await
            .expect("first submission");

        // Simulate a daemon restart: a fresh facade over the same durable
        // store and services has an empty in-memory idempotency index.
        let restarted_scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );
        let restarted = JobSubmissionService::new(
            store.clone(),
            restarted_scheduler,
            services,
            codegg_core::jobs::DaemonGeneration::new_unchecked("generation-test"),
        );
        let second = restarted
            .submit(Some(key), python_spec)
            .await
            .expect("retry submission after restart");

        assert_eq!(first.job_id, second.job_id);
        let jobs = store
            .list_jobs(codegg_core::jobs::store::JobStoreQuery::default())
            .await
            .expect("list jobs");
        assert_eq!(jobs.len(), 1);
    }

    /// M007: the submission boundary binds the build class to the
    /// canonical workspace and drops the legacy global key.
    #[test]
    fn the_build_class_scope_is_bound_at_the_submission_boundary() {
        let workspace = codegg_core::workspace::WorkspaceId::new_unchecked("ws-bound");
        let mut job = spec(workspace.clone());
        job.kind = JobKind::Build;
        job.payload = JobPayload::ManagedArgv {
            argv: vec!["cargo".into(), "build".into()],
            cwd: None,
            // Legacy-shaped payload: no captured profile ceiling and the
            // unrestricted network intent, which is what these payloads
            // decode as when the fields are absent.
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        };
        apply_resource_policy(&mut job);
        assert_eq!(job.resource_request.keyed_quotas.len(), 1);
        assert_eq!(
            job.resource_request.keyed_quotas[0].key,
            format!("build:{workspace}")
        );
        assert!(job
            .resource_request
            .exclusivity_keys
            .iter()
            .all(|key| key != "exclusive:workspace-mutation"));
    }

    /// M007: a caller that supplies its own resource request cannot opt
    /// out of the build bound it is measured against.
    #[test]
    fn a_caller_supplied_request_cannot_escape_the_build_bound() {
        let workspace = codegg_core::workspace::WorkspaceId::new_unchecked("ws-escape");
        let mut job = spec(workspace.clone());
        job.kind = JobKind::Build;
        job.payload = JobPayload::ManagedArgv {
            argv: vec!["cargo".into(), "build".into()],
            cwd: None,
            // Legacy-shaped payload: no captured profile ceiling and the
            // unrestricted network intent, which is what these payloads
            // decode as when the fields are absent.
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        };
        // A request that names the legacy global key, or a quota of its
        // own, must not survive the boundary.
        job.resource_request = ResourceRequest {
            exclusivity_keys: vec!["exclusive:workspace-mutation".to_string()],
            keyed_quotas: vec![codegg_core::jobs::KeyedQuotaRequest::new(
                "build:elsewhere",
                8,
            )],
            ..ResourceRequest::default()
        };
        apply_resource_policy(&mut job);
        assert_eq!(job.resource_request.keyed_quotas.len(), 1);
        assert_eq!(
            job.resource_request.keyed_quotas[0].key,
            format!("build:{workspace}")
        );
        assert!(job
            .resource_request
            .exclusivity_keys
            .iter()
            .all(|key| key != "exclusive:workspace-mutation"));
    }

    /// M007: an unsupported kind keeps its existing admission and
    /// carries no quota.
    #[test]
    fn an_unsupported_kind_carries_no_quota() {
        let workspace = codegg_core::workspace::WorkspaceId::new_unchecked("ws-plain");
        let mut job = spec(workspace);
        apply_resource_policy(&mut job);
        assert!(job.resource_request.keyed_quotas.is_empty());
    }
}
