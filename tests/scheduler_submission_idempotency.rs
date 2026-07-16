use std::sync::Arc;

use codegg::scheduler::submission::{JobSubmissionError, JobSubmissionService, SubmissionKey};
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, CancelReason, DaemonGeneration, IdempotencyClass,
    InMemoryJobStore, JobKind, JobPayload, JobPriority, JobSource, JobState, JobStore,
    JobStoreQuery, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

fn test_spec(workspace_id: WorkspaceId) -> NewJob {
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

    async fn job_count(&self) -> usize {
        self.store
            .list_jobs(JobStoreQuery::default())
            .await
            .expect("list jobs")
            .len()
    }

    async fn complete_job(&self, job_id: &codegg_core::jobs::JobId) {
        let gen = DaemonGeneration::new_unchecked("gen-test");
        let attempt = self
            .store
            .begin_attempt(job_id, &gen)
            .await
            .expect("begin attempt");
        self.store
            .mark_attempt_running(&attempt.attempt_id)
            .await
            .expect("mark running");
        self.store
            .finish_attempt(AttemptCompletion {
                attempt_id: attempt.attempt_id.clone(),
                state: AttemptState::Completed,
                error: None,
                run_id: None,
            })
            .await
            .expect("finish attempt");
    }

    async fn cancel_job(&self, job_id: &codegg_core::jobs::JobId) {
        self.store
            .request_cancel(job_id, CancelReason::new("test", "test cancellation"))
            .await
            .expect("cancel");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_identical_submissions_return_one_job_id() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;
    let key = SubmissionKey::new("concurrent-key").expect("key");

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let sub = h.submission.clone();
            let ws = ws_id.clone();
            let k = key.clone();
            tokio::spawn(async move { sub.submit(Some(k), test_spec(ws)).await })
        })
        .collect();

    let mut job_ids = Vec::new();
    for handle in handles {
        let result = handle.await.expect("task panicked").expect("submit failed");
        job_ids.push(result.job_id);
    }

    let first = &job_ids[0];
    assert!(
        job_ids.iter().all(|id| id == first),
        "all concurrent submissions must return the same JobId"
    );
    assert_eq!(h.job_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_submission_after_response_loss_returns_same_job_id() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;
    let key = SubmissionKey::new("retry-key").expect("key");

    let first = h
        .submission
        .submit(Some(key.clone()), test_spec(ws_id.clone()))
        .await
        .expect("first submit");

    let first_id = first.job_id.clone();
    drop(first);

    let second = h
        .submission
        .submit(Some(key), test_spec(ws_id))
        .await
        .expect("second submit");

    assert_eq!(first_id, second.job_id);
    assert_eq!(h.job_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn same_key_different_workspace_returns_conflict() {
    let root_a = tempfile::tempdir().expect("workspace a");
    let root_b = tempfile::tempdir().expect("workspace b");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let ws_a = workspace_registry
        .get_or_register(root_a.path())
        .await
        .expect("register a");
    let ws_b = workspace_registry
        .get_or_register(root_b.path())
        .await
        .expect("register b");
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

    let key = SubmissionKey::new("ws-conflict-key").expect("key");
    submission
        .submit(Some(key.clone()), test_spec(ws_a.id.clone()))
        .await
        .expect("first submit");

    let err = submission
        .submit(Some(key), test_spec(ws_b.id.clone()))
        .await
        .expect_err("different workspace must conflict");
    assert!(matches!(err, JobSubmissionError::SubmissionKeyConflict));
    assert_eq!(
        store
            .list_jobs(JobStoreQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn same_key_different_argv_returns_conflict() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;

    let mut spec_a = test_spec(ws_id.clone());
    spec_a.payload = JobPayload::Test {
        command: "echo ok".into(),
        argv: vec!["echo".into(), "ok".into()],
        cwd: Some("/tmp".into()),
        scope: None,
    };

    let mut spec_b = test_spec(ws_id);
    spec_b.payload = JobPayload::Test {
        command: "echo fail".into(),
        argv: vec!["echo".into(), "fail".into()],
        cwd: Some("/tmp".into()),
        scope: None,
    };

    let key = SubmissionKey::new("argv-conflict-key").expect("key");
    h.submission
        .submit(Some(key.clone()), spec_a)
        .await
        .expect("first submit");

    let err = h
        .submission
        .submit(Some(key), spec_b)
        .await
        .expect_err("different argv must conflict");
    assert!(matches!(err, JobSubmissionError::SubmissionKeyConflict));
    assert_eq!(h.job_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn retry_after_terminal_completion_resolves_to_same_job() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;
    let key = SubmissionKey::new("terminal-key").expect("key");

    let first = h
        .submission
        .submit(Some(key.clone()), test_spec(ws_id.clone()))
        .await
        .expect("first submit");

    h.complete_job(&first.job_id).await;

    let second = h
        .submission
        .submit(Some(key), test_spec(ws_id))
        .await
        .expect("resubmit after completion");

    assert_eq!(first.job_id, second.job_id);
    assert_eq!(second.state, JobState::Completed);
    assert_eq!(h.job_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn retry_after_cancellation_resolves_to_same_job() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;
    let key = SubmissionKey::new("cancel-key").expect("key");

    let first = h
        .submission
        .submit(Some(key.clone()), test_spec(ws_id.clone()))
        .await
        .expect("first submit");

    h.cancel_job(&first.job_id).await;

    let second = h
        .submission
        .submit(Some(key), test_spec(ws_id))
        .await
        .expect("resubmit after cancel");

    assert_eq!(first.job_id, second.job_id);
    assert_eq!(second.state, JobState::Cancelled);
    assert_eq!(h.job_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn fresh_service_with_same_store_does_not_see_prior_key() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let ws = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let gen = DaemonGeneration::new_unchecked("gen-test");

    let scheduler_a = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        gen.clone(),
    );
    let submission_a =
        JobSubmissionService::new(store.clone(), scheduler_a, services.clone(), gen.clone());

    let scheduler_b = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        gen.clone(),
    );
    let submission_b = JobSubmissionService::new(store.clone(), scheduler_b, services, gen);

    let key = SubmissionKey::new("cross-service-key").expect("key");
    let first = submission_a
        .submit(Some(key.clone()), test_spec(ws.id.clone()))
        .await
        .expect("submit via A");

    let second = submission_b
        .submit(Some(key), test_spec(ws.id.clone()))
        .await
        .expect("submit via B");

    assert_ne!(
        first.job_id, second.job_id,
        "fresh service must not see prior idempotency key"
    );
    assert_eq!(
        store
            .list_jobs(JobStoreQuery::default())
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test(flavor = "current_thread")]
async fn payload_too_large_rejects_without_creating_job() {
    let h = TestHarness::new().await;
    let ws_id = h.workspace_id().await;

    let large_argv: Vec<String> = (0..50_000).map(|i| format!("arg-{i:08}")).collect();
    let spec = NewJob {
        workspace_id: ws_id,
        session_id: None,
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command: "oversized".into(),
            argv: large_argv,
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
    };

    let err = h
        .submission
        .submit(None, spec)
        .await
        .expect_err("oversized payload must be rejected");
    assert!(matches!(err, JobSubmissionError::PayloadTooLarge));
    assert_eq!(h.job_count().await, 0);
}

#[test]
fn empty_submission_key_is_invalid() {
    let result = SubmissionKey::new("");
    assert!(matches!(
        result,
        Err(JobSubmissionError::InvalidSubmissionKey)
    ));
}

#[test]
fn oversized_submission_key_is_invalid() {
    let result = SubmissionKey::new("x".repeat(257));
    assert!(matches!(
        result,
        Err(JobSubmissionError::InvalidSubmissionKey)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_scheduler_rejects_submission() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let ws = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let config = ResolvedSchedulerConfig {
        enabled: false,
        ..ResolvedSchedulerConfig::default()
    };
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        config,
        DaemonGeneration::new_unchecked("disabled-gen"),
    );
    let submission = JobSubmissionService::new(
        store,
        scheduler,
        services,
        DaemonGeneration::new_unchecked("disabled-gen"),
    );

    let err = submission
        .submit(None, test_spec(ws.id.clone()))
        .await
        .expect_err("disabled must reject");
    assert!(matches!(err, JobSubmissionError::SchedulerDisabled));
}
