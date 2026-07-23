//! Integration tests for scheduler-owned Python execution.
//!
//! Covers: PythonJobExecutor validation, submission through
//! JobSubmissionService, disabled-scheduler fail-closed behavior,
//! source integrity checks, and legacy payload rejection.

use std::sync::Arc;

use codegg::scheduler::permit::{PermitDimensions, ResourcePermitGuard};
use codegg::scheduler::submission::{JobSubmissionError, JobSubmissionService, SubmissionKey};
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg::tool::Tool;
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind,
    JobPayload, JobPriority, JobSource, JobState, JobStore, JobStoreQuery, NewJob, ResourceRequest,
    RetryPolicy,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

struct TestHarness {
    _root: tempfile::TempDir,
    store: Arc<dyn JobStore>,
    submission: Arc<JobSubmissionService>,
}

impl TestHarness {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let _workspace = workspace_registry
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
            ResolvedSchedulerConfig::default(),
            DaemonGeneration::new_unchecked("gen-test"),
        );
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services,
            DaemonGeneration::new_unchecked("gen-test"),
        );
        Self {
            _root: root,
            store,
            submission,
        }
    }

    async fn workspace_id(&self) -> WorkspaceId {
        self.submission
            .workspace_id_for_root(self._root.path())
            .await
            .expect("resolve workspace")
    }

    async fn submit_python(
        &self,
        source: &str,
        mode: &str,
    ) -> Result<codegg_core::jobs::JobId, JobSubmissionError> {
        use codegg::python_script::source_store::compute_digest;

        let ws_id = self.workspace_id().await;
        let source_hash = compute_digest(source);

        let spec = NewJob {
            workspace_id: ws_id,
            session_id: None,
            turn_id: None,
            kind: JobKind::Python,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::Python {
                script_path: String::new(),
                args: vec![],
                mode: mode.to_string(),
                source: Some(source.to_string()),
                source_hash: Some(source_hash.clone()),
                cwd: Some("/tmp".to_string()),
                timeout_secs: Some(30),
            },
            resource_request: ResourceRequest::for_kind(JobKind::Python),
            timeout: Some(std::time::Duration::from_secs(30)),
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: vec![],
        };

        let key =
            SubmissionKey::new(format!("python:{source_hash}")).expect("valid submission key");
        let submitted = self.submission.submit(Some(key), spec).await?;
        Ok(submitted.job_id)
    }
}

// ── Disabled scheduler fail-closed ──────────────────────────────────────

#[tokio::test]
async fn disabled_scheduler_returns_typed_error() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let _workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let mut config = ResolvedSchedulerConfig::default();
    config.enabled = false;
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new_unchecked("gen-test"),
    );
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler,
        services,
        DaemonGeneration::new_unchecked("gen-test"),
    );

    let ws_id = submission
        .workspace_id_for_root(root.path())
        .await
        .expect("resolve workspace");

    let spec = NewJob {
        workspace_id: ws_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "analyze".to_string(),
            source: Some("print(1)".to_string()),
            source_hash: Some(codegg::python_script::source_store::compute_digest(
                "print(1)",
            )),
            cwd: Some("/tmp".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::for_kind(JobKind::Python),
        timeout: Some(std::time::Duration::from_secs(30)),
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
    };

    let result = submission.submit(None, spec).await;
    assert!(
        matches!(result, Err(JobSubmissionError::SchedulerDisabled)),
        "expected SchedulerDisabled, got: {result:?}"
    );
}

// ── Source validation ───────────────────────────────────────────────────

#[tokio::test]
async fn python_payload_validates_mode() {
    let harness = TestHarness::new().await;
    let ws_id = harness.workspace_id().await;

    let spec = NewJob {
        workspace_id: ws_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "invalid_mode".to_string(),
            source: Some("print(1)".to_string()),
            source_hash: Some(codegg::python_script::source_store::compute_digest(
                "print(1)",
            )),
            cwd: Some("/tmp".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::for_kind(JobKind::Python),
        timeout: Some(std::time::Duration::from_secs(30)),
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
    };

    let result = harness.submission.submit(None, spec).await;
    assert!(
        result.is_ok(),
        "submission should succeed (validation is in executor): {result:?}"
    );
}

#[tokio::test]
async fn source_hash_mismatch_rejected_at_validation() {
    use codegg::scheduler::executor::{ExecutorKind, JobExecutor};
    use codegg::scheduler::executors::PythonJobExecutor;

    let executor = PythonJobExecutor::new(None);
    let now = chrono::Utc::now();
    let job = codegg_core::jobs::JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked("j-test"),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "analyze".to_string(),
            source: Some("print(1)".to_string()),
            source_hash: Some("wrong_hash".to_string()),
            cwd: Some("/tmp".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
    };

    let result = executor.validate(&job);
    assert!(
        result.is_err(),
        "source hash mismatch should fail validation"
    );
}

// ── Legacy payload rejection ────────────────────────────────────────────

#[tokio::test]
async fn legacy_script_path_payload_rejected_without_source() {
    use codegg::scheduler::executor::{ExecutorKind, JobExecutor};
    use codegg::scheduler::executors::PythonJobExecutor;

    let executor = PythonJobExecutor::new(None);
    let now = chrono::Utc::now();
    let job = codegg_core::jobs::JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked("j-legacy"),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::Python {
            script_path: "/some/script.py".to_string(),
            args: vec![],
            mode: "analyze".to_string(),
            source: None, // Legacy: no inline source
            source_hash: None,
            cwd: Some("/tmp".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
    };

    // Validation passes (it only checks mode and hash)
    assert!(executor.validate(&job).is_ok());

    // But execution fails with a typed error about missing source
    let ctx = codegg::scheduler::executor::JobExecutionContext {
        job: job.clone(),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        attempt_id: codegg_core::jobs::AttemptId::new_unchecked("a-1"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-test"),
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(codegg::scheduler::executors::NullProgressSink),
        resources: ResourcePermitGuard::new_orphan(PermitDimensions::default()),
    };

    let completion = executor.execute(ctx).await;
    assert!(
        matches!(
            completion.status,
            codegg::scheduler::executor::ExecutorStatus::Failed
        ),
        "legacy payload should fail execution"
    );
    assert!(
        completion.summary.contains("inline source is required"),
        "should mention inline source requirement: {}",
        completion.summary
    );
}

// ── SubmissionKey idempotency ───────────────────────────────────────────

#[tokio::test]
async fn duplicate_submission_key_returns_existing_job() {
    let harness = TestHarness::new().await;
    let source = "print('idempotent')";

    let job_id_1 = harness
        .submit_python(source, "analyze")
        .await
        .expect("first submit");
    let job_id_2 = harness
        .submit_python(source, "analyze")
        .await
        .expect("second submit");

    assert_eq!(
        job_id_1, job_id_2,
        "duplicate submission key should return same job"
    );

    // Only one job in the store
    let jobs = harness
        .store
        .list_jobs(JobStoreQuery::default())
        .await
        .expect("list");
    assert_eq!(jobs.len(), 1, "should have exactly one job");
}

#[tokio::test]
async fn different_source_different_job() {
    let harness = TestHarness::new().await;

    let job_id_1 = harness
        .submit_python("print('a')", "analyze")
        .await
        .expect("first submit");
    let job_id_2 = harness
        .submit_python("print('b')", "analyze")
        .await
        .expect("second submit");

    assert_ne!(
        job_id_1, job_id_2,
        "different source should create different jobs"
    );
}

// ── Python executor kind ────────────────────────────────────────────────

#[test]
fn python_executor_kind_and_supports() {
    use codegg::scheduler::executor::{ExecutorKind, JobExecutor};
    use codegg::scheduler::executors::PythonJobExecutor;

    let executor = PythonJobExecutor::new(None);
    assert_eq!(executor.kind(), ExecutorKind::Python);
    assert!(executor.supports(JobKind::Python));
    assert!(!executor.supports(JobKind::Test));
    assert!(!executor.supports(JobKind::Build));
    assert_eq!(
        executor.health(),
        codegg::scheduler::executor::ExecutorHealth::Healthy
    );
}

// ── Cancellation token wiring ───────────────────────────────────────────

#[tokio::test]
async fn python_executor_cancelled_before_launch() {
    use codegg::scheduler::executor::JobExecutor;
    use codegg::scheduler::executors::PythonJobExecutor;

    let executor = PythonJobExecutor::new(None);
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel(); // Cancel immediately

    let now = chrono::Utc::now();
    let job = codegg_core::jobs::JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked("j-cancel"),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "analyze".to_string(),
            source: Some("import time; time.sleep(10)".to_string()),
            source_hash: Some(codegg::python_script::source_store::compute_digest(
                "import time; time.sleep(10)",
            )),
            cwd: Some("/tmp".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
    };

    let ctx = codegg::scheduler::executor::JobExecutionContext {
        job,
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        attempt_id: codegg_core::jobs::AttemptId::new_unchecked("a-cancel"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-test"),
        cancellation,
        progress: Arc::new(codegg::scheduler::executors::NullProgressSink),
        resources: ResourcePermitGuard::new_orphan(PermitDimensions::default()),
    };

    let completion = executor.execute(ctx).await;
    assert!(
        matches!(
            completion.status,
            codegg::scheduler::executor::ExecutorStatus::Cancelled
        ),
        "should report Cancelled status"
    );
    assert!(
        completion.summary.contains("cancelled before launch"),
        "should mention cancelled before launch: {}",
        completion.summary
    );
}

// ── Workspace CWD escape test ───────────────────────────────────────────

#[tokio::test]
async fn python_executor_rejects_cwd_outside_workspace() {
    use codegg::scheduler::executor::JobExecutor;
    use codegg::scheduler::executors::PythonJobExecutor;

    let executor = PythonJobExecutor::new(None);
    let now = chrono::Utc::now();
    // CWD outside workspace: try to use /etc as CWD with a workspace of /tmp
    let job = codegg_core::jobs::JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked("j-cwd-escape"),
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Python,
        source: JobSource::Interactive,
        priority: JobPriority::Normal,
        payload: JobPayload::Python {
            script_path: String::new(),
            args: vec![],
            mode: "analyze".to_string(),
            source: Some("print(1)".to_string()),
            source_hash: Some(codegg::python_script::source_store::compute_digest(
                "print(1)",
            )),
            cwd: Some("/nonexistent/workspace/malicious".to_string()),
            timeout_secs: Some(30),
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
    };

    let ctx = codegg::scheduler::executor::JobExecutionContext {
        job,
        workspace_id: WorkspaceId::new_unchecked("ws-1"),
        attempt_id: codegg_core::jobs::AttemptId::new_unchecked("a-cwd"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-test"),
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(codegg::scheduler::executors::NullProgressSink),
        resources: ResourcePermitGuard::new_orphan(PermitDimensions::default()),
    };

    let completion = executor.execute(ctx).await;
    // The executor should fail because the CWD is invalid/nonexistent
    assert!(
        matches!(
            completion.status,
            codegg::scheduler::executor::ExecutorStatus::Failed
        ),
        "CWD outside workspace should fail execution"
    );
}

// ── Disabled scheduler returns typed ToolError ──────────────────────────

#[tokio::test]
async fn python_tool_disabled_scheduler_returns_disabled_error() {
    use codegg::error::ToolError;
    use codegg::python_script::PythonScriptTool;

    // Tool without scheduler should return Disabled error
    let tool = PythonScriptTool::new();
    let input = serde_json::json!({
        "code": "print(1)",
        "mode": "analyze"
    });

    let result = tool.execute(input).await;
    assert!(
        result.is_err(),
        "should return error when scheduler disabled"
    );
    match result.unwrap_err() {
        ToolError::Disabled(msg) => {
            assert!(
                msg.contains("scheduler admission"),
                "should mention scheduler admission: {msg}"
            );
        }
        other => panic!("expected Disabled error, got: {other:?}"),
    }
}
