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
    DaemonGeneration, JobId, JobKind, JobPayload, JobRecord, JobState, JobStore, NewJob,
    ResourceRequest,
};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace_services::WorkspaceServiceRegistry;
use thiserror::Error;
use tokio::sync::Mutex;

use super::scheduler::{JobScheduler, JobSchedulerError};

const MAX_SUBMISSION_KEY_BYTES: usize = 256;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

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
    #[error("unsupported job payload for kind '{0}'")]
    InvalidPayload(String),
}

struct IdempotentSubmission {
    fingerprint: String,
    job_id: JobId,
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
    idempotency: Mutex<HashMap<SubmissionKey, IdempotentSubmission>>,
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
            idempotency: Mutex::new(HashMap::new()),
        })
    }

    pub fn scheduler(&self) -> &Arc<JobScheduler> {
        &self.scheduler
    }

    pub fn daemon_generation(&self) -> &DaemonGeneration {
        &self.daemon_generation
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

    pub async fn submit(
        &self,
        key: Option<SubmissionKey>,
        mut spec: NewJob,
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
        drop(lease);

        apply_resource_policy(&mut spec);
        let fingerprint = fingerprint(&spec);

        // Serializing submissions with the same facade makes the
        // create/enqueue boundary idempotent under concurrent transport
        // retries. The durable job is still authoritative after return.
        let mut idempotency = self.idempotency.lock().await;
        if let Some(key_ref) = key.as_ref() {
            if let Some(existing) = idempotency.get(key_ref) {
                if existing.fingerprint != fingerprint {
                    return Err(JobSubmissionError::SubmissionKeyConflict);
                }
                if let Some(job) = self.store.get_job(&existing.job_id).await? {
                    return Ok(to_submitted(&job));
                }
                idempotency.remove(key_ref);
            }
        }

        let job = self.store.create_job(spec).await?;
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
            if let Some(key) = key.as_ref() {
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
        if let Some(key) = key {
            idempotency.insert(
                key,
                IdempotentSubmission {
                    fingerprint,
                    job_id: job.job_id.clone(),
                },
            );
        }
        Ok(to_submitted(&job))
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

fn validate_payload(kind: JobKind, payload: &JobPayload) -> Result<(), JobSubmissionError> {
    let valid = match (kind, payload) {
        (JobKind::Test, JobPayload::Test { argv, .. }) => !argv.is_empty(),
        (
            JobKind::Build | JobKind::Lint | JobKind::Format,
            JobPayload::ManagedArgv { argv, .. },
        ) => !argv.is_empty(),
        (JobKind::Subagent, JobPayload::Subagent { .. })
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
}

fn fingerprint(spec: &NewJob) -> String {
    let payload = serde_json::to_string(&spec.payload).unwrap_or_else(|_| "<invalid>".into());
    format!(
        "{}|{}|{}|{}|{}",
        spec.workspace_id,
        spec.kind.as_str(),
        spec.session_id.as_deref().unwrap_or_default(),
        spec.priority.as_str(),
        payload
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        IdempotencyClass, InMemoryJobStore, JobPriority, JobSource, JobStore, ResourceRequest,
        RetryPolicy,
    };
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use std::sync::Arc;

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
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
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
        assert!(ResourceRequest::for_kind(JobKind::Build)
            .exclusivity_keys
            .iter()
            .any(|key| key == "exclusive:workspace-mutation"));
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
        let key = SubmissionKey::new("retry-1").expect("key");
        let first = submission
            .submit(Some(key.clone()), spec(workspace.id.clone()))
            .await
            .expect("first submission");
        let second = submission
            .submit(Some(key), spec(workspace.id.clone()))
            .await
            .expect("retry submission");

        assert_eq!(first.job_id, second.job_id);
        let jobs = store
            .list_jobs(codegg_core::jobs::store::JobStoreQuery::default())
            .await
            .expect("list jobs");
        assert_eq!(jobs.len(), 1);
    }
}
