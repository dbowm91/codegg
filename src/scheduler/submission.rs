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
    ResourceRequest, GOAL_PROVENANCE_LABEL_KEY,
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
    #[error("active goal lookup failed: {0}")]
    Goal(String),
}

#[derive(Clone)]
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
    goal_store: Option<Arc<codegg_core::goal::GoalStore>>,
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
            goal_store: None,
            idempotency: Mutex::new(HashMap::new()),
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
        let labels = self.goal_provenance_labels(&spec).await?;
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
        IdempotencyClass, InMemoryJobStore, JobPayload, JobPriority, JobSource, JobStore,
        ResourceRequest, RetryPolicy, SqliteJobStore,
    };
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use std::sync::Arc;

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
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
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
}
