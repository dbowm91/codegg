//! Eggwork fixed-target remote execution (M001) integration tests.
//!
//! All remote behavior runs against a scripted in-process fake implementing
//! the public [`EggworkNodeClient`](codegg::scheduler::EggworkNodeClient)
//! seam: no TLS fixture, live node, or internet service is required.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use codegg::scheduler::executor::executor_kind_for_job;
use codegg::scheduler::{
    AdmissionController, EggworkClientFactory, EggworkExecutor, EggworkExecutorConfig,
    EggworkNodeClient, ExecutorKind, ExecutorStatus, JobExecutionContext, JobExecutor,
    JobScheduler, JobSubmissionService, ResolvedEggworkNode, ResolvedSchedulerConfig,
};
use codegg_core::jobs::{
    store::JobStoreQuery, AttemptId, DaemonGeneration, ExecutionTarget, IdempotencyClass,
    InMemoryJobStore, JobId, JobKind, JobPayload, JobPriority, JobRecord, JobSource, JobState,
    JobStore, NewJob, ResourceRequest, RetryPolicy, SqliteJobStore,
};
use codegg_core::run_store::MemRunStore;
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use eggfetch_core::{BoxBytesStream, Error as EggfetchError};
use eggwork_client::{ClientError as EggworkClientError, WorkspaceReady};
use eggwork_core::{
    ArtifactId, ArtifactRecord, ArtifactType, BlobDigest, EventMetadata, EventSequence,
    ExecutionEvent, ExecutionEventKind, ExecutionGeneration, ExecutionHandle, ExecutionId,
    ExecutionResult, ExecutionSnapshot, ExecutionSpec, ExecutionState, LeaseId, NodeCapabilities,
    NodeId, NodeStatus, ProtocolVersion, ProtocolVersionRange, RelativePath,
    WorkspaceId as EggworkWorkspaceId, WorkspaceManifest,
};
use futures_util::StreamExt;

// ── Scripted fake ──────────────────────────────────────────────────────────

struct ScriptedClient {
    events: Vec<ExecutionEvent>,
    snapshot: Mutex<ExecutionSnapshot>,
    capabilities: Mutex<NodeCapabilities>,
    status: Mutex<NodeStatus>,
    submitted_argv: Mutex<Vec<Vec<String>>>,
    /// Full submitted handles in acceptance order. The persisted-handle
    /// tests compare the submitted lease against the durable lease so a
    /// divergent-lease regression cannot hide behind id-only assertions.
    submitted_handles: Mutex<Vec<ExecutionHandle>>,
    uploads: AtomicUsize,
    submits: AtomicUsize,
    cancels: Mutex<Vec<String>>,
    cancel_leases: Mutex<Vec<String>>,
    renews: AtomicUsize,
    renew_leases: Mutex<Vec<String>>,
    artifacts: Mutex<Vec<ArtifactRecord>>,
    artifact_bytes: Mutex<Vec<u8>>,
    hang_events: Mutex<bool>,
    /// When enabled, the fake fences cancel/renew on the accepted lease
    /// token like the real Eggwork server: a mismatched lease is rejected
    /// with typed `invalid_lease` instead of succeeding.
    fencing: AtomicBool,
    accepted_leases: Mutex<HashMap<String, String>>,
}

fn test_capabilities(features: Vec<String>) -> NodeCapabilities {
    NodeCapabilities {
        protocol: ProtocolVersionRange {
            min: ProtocolVersion { major: 1, minor: 0 },
            max: ProtocolVersion { major: 1, minor: 5 },
        },
        features,
        max_active_executions: 4,
    }
}

fn test_status(draining: bool, active: u32) -> NodeStatus {
    NodeStatus {
        node_id: NodeId::new("node-1").unwrap(),
        draining,
        active_executions: active,
        capabilities: test_capabilities(vec!["exec.argv.v1".to_string()]),
    }
}

fn succeeded_snapshot() -> ExecutionSnapshot {
    ExecutionSnapshot {
        schema_version: 1,
        execution_id: ExecutionId::new("exec-1").unwrap(),
        generation: ExecutionGeneration::new(1).unwrap(),
        state: ExecutionState::Succeeded,
        result: Some(ExecutionResult {
            state: ExecutionState::Succeeded,
            exit_code: Some(0),
            failure: None,
            stdout_bytes: 6,
            stderr_bytes: 0,
            stdout_omitted: 0,
            stderr_omitted: 0,
            cleanup_warning: None,
            finalization_failure: None,
            artifact_count: 0,
            sandbox: None,
            resources: None,
        }),
    }
}

impl ScriptedClient {
    fn succeeding() -> Self {
        Self {
            events: vec![ExecutionEvent {
                sequence: EventSequence::new(1),
                kind: ExecutionEventKind::Stdout(b"hello\n".to_vec()),
                metadata: EventMetadata { fields: Vec::new() },
            }],
            snapshot: Mutex::new(succeeded_snapshot()),
            capabilities: Mutex::new(test_capabilities(vec!["exec.argv.v1".to_string()])),
            status: Mutex::new(test_status(false, 0)),
            submitted_argv: Mutex::new(Vec::new()),
            submitted_handles: Mutex::new(Vec::new()),
            uploads: AtomicUsize::new(0),
            submits: AtomicUsize::new(0),
            cancels: Mutex::new(Vec::new()),
            cancel_leases: Mutex::new(Vec::new()),
            renews: AtomicUsize::new(0),
            renew_leases: Mutex::new(Vec::new()),
            artifacts: Mutex::new(Vec::new()),
            artifact_bytes: Mutex::new(Vec::new()),
            hang_events: Mutex::new(false),
            fencing: AtomicBool::new(false),
            accepted_leases: Mutex::new(HashMap::new()),
        }
    }

    fn enable_lease_fencing(&self) {
        self.fencing.store(true, Ordering::SeqCst);
    }

    /// Typed `invalid_lease` rejection mirroring the real Eggwork server
    /// (HTTP 403, code `invalid_lease`).
    fn invalid_lease() -> EggworkClientError {
        EggworkClientError::Api {
            status: 403,
            code: "invalid_lease".to_string(),
            message: "execution lease is invalid".to_string(),
        }
    }

    fn check_lease(&self, handle: &ExecutionHandle) -> Result<(), EggworkClientError> {
        if !self.fencing.load(Ordering::SeqCst) {
            return Ok(());
        }
        let accepted = self.accepted_leases.lock().unwrap();
        match accepted.get(handle.execution_id.as_str()) {
            Some(expected) if expected == handle.lease_id.as_str() => Ok(()),
            _ => Err(Self::invalid_lease()),
        }
    }
}

#[async_trait]
impl EggworkNodeClient for ScriptedClient {
    async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError> {
        Ok(self.capabilities.lock().unwrap().clone())
    }

    async fn status(&self) -> Result<NodeStatus, EggworkClientError> {
        Ok(self.status.lock().unwrap().clone())
    }

    async fn find_missing_blobs(
        &self,
        digests: &[BlobDigest],
    ) -> Result<Vec<BlobDigest>, EggworkClientError> {
        Ok(digests.to_vec())
    }

    async fn upload_blob(
        &self,
        _digest: &BlobDigest,
        _declared_length: u64,
        _stream: BoxBytesStream,
    ) -> Result<(), EggworkClientError> {
        self.uploads.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn create_workspace(
        &self,
        workspace_id: &EggworkWorkspaceId,
        handle: &ExecutionHandle,
        manifest: &WorkspaceManifest,
    ) -> Result<WorkspaceReady, EggworkClientError> {
        Ok(WorkspaceReady {
            schema_version: 1,
            workspace_id: workspace_id.clone(),
            execution_id: handle.execution_id.clone(),
            generation: handle.generation,
            manifest_digest: manifest.digest().expect("manifest digest"),
            logical_bytes: manifest.logical_bytes(),
        })
    }

    async fn execute_in_workspace(
        &self,
        spec: &ExecutionSpec,
        handle: &ExecutionHandle,
        _workspace_id: &EggworkWorkspaceId,
    ) -> Result<codegg::scheduler::BoxEventStream, EggworkClientError> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        self.submitted_argv
            .lock()
            .unwrap()
            .push(spec.command.argv.clone());
        self.submitted_handles.lock().unwrap().push(handle.clone());
        self.accepted_leases.lock().unwrap().insert(
            handle.execution_id.as_str().to_string(),
            handle.lease_id.as_str().to_string(),
        );
        if *self.hang_events.lock().unwrap() {
            Ok(
                futures_util::stream::pending::<Result<ExecutionEvent, EggworkClientError>>()
                    .boxed(),
            )
        } else {
            Ok(futures_util::stream::iter(self.events.clone().into_iter().map(Ok)).boxed())
        }
    }

    async fn observe_generation(
        &self,
        _id: &ExecutionId,
        _generation: u64,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        Ok(self.snapshot.lock().unwrap().clone())
    }

    async fn cancel(
        &self,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.check_lease(handle)?;
        self.cancels
            .lock()
            .unwrap()
            .push(handle.execution_id.as_str().to_string());
        self.cancel_leases
            .lock()
            .unwrap()
            .push(handle.lease_id.as_str().to_string());
        Ok(self.snapshot.lock().unwrap().clone())
    }

    async fn renew(
        &self,
        handle: &ExecutionHandle,
        _renewal_id: &str,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.check_lease(handle)?;
        self.renews.fetch_add(1, Ordering::SeqCst);
        self.renew_leases
            .lock()
            .unwrap()
            .push(handle.lease_id.as_str().to_string());
        Ok(self.snapshot.lock().unwrap().clone())
    }

    async fn artifacts(
        &self,
        _execution_id: &ExecutionId,
        _generation: u64,
    ) -> Result<Vec<ArtifactRecord>, EggworkClientError> {
        Ok(self.artifacts.lock().unwrap().clone())
    }

    async fn download_artifact(
        &self,
        _artifact: &ArtifactRecord,
    ) -> Result<BoxBytesStream, EggworkClientError> {
        let bytes = bytes::Bytes::from(self.artifact_bytes.lock().unwrap().clone());
        Ok(futures_util::stream::iter(vec![Ok::<bytes::Bytes, EggfetchError>(bytes)]).boxed())
    }
}

struct ScriptedFactory {
    client: Arc<ScriptedClient>,
}

impl EggworkClientFactory for ScriptedFactory {
    fn client_for(
        &self,
        _node: &ResolvedEggworkNode,
    ) -> Result<Arc<dyn EggworkNodeClient>, String> {
        Ok(self.client.clone())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn test_node() -> ResolvedEggworkNode {
    ResolvedEggworkNode {
        node_id: "node-1".to_string(),
        endpoint: "https://node-1:8443".to_string(),
        ca_cert: PathBuf::from("/tmp/ca.pem"),
        client_cert: PathBuf::from("/tmp/client.pem"),
        client_key: PathBuf::from("/tmp/client.key"),
        required_capabilities: Vec::new(),
    }
}

fn eggwork_target() -> ExecutionTarget {
    ExecutionTarget::EggworkNode {
        node_id: "node-1".to_string(),
    }
}

fn build_record(target: ExecutionTarget) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked("j-egg-int"),
        workspace_id: WorkspaceId::new_unchecked("ws-egg"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv: vec!["echo".to_string(), "hello world".to_string()],
            cwd: None,
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
        depends_on: Vec::new(),
        labels: HashMap::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target,
    }
}

fn bound_context(job: JobRecord, root: PathBuf) -> JobExecutionContext {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let mut resources = codegg::scheduler::ResourcePermitGuard::new_orphan(Default::default());
    resources.install_controller(controller);
    JobExecutionContext {
        job,
        attempt_id: AttemptId::new_unchecked("att-int-1"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-int"),
        workspace_id: WorkspaceId::new_unchecked("ws-egg"),
        workspace_root: root,
        run_store: None,
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(codegg::scheduler::NoopProgressSink),
        resources,
    }
}

fn executor_with(
    client: &Arc<ScriptedClient>,
    store: Option<Arc<dyn JobStore>>,
) -> EggworkExecutor {
    let mut nodes = HashMap::new();
    nodes.insert("node-1".to_string(), test_node());
    EggworkExecutor::with_factory(
        EggworkExecutorConfig {
            nodes,
            store,
            ..Default::default()
        },
        Arc::new(ScriptedFactory {
            client: client.clone(),
        }),
    )
}

// ── Routing matrix ─────────────────────────────────────────────────────────

#[test]
fn routing_sends_remote_target_to_eggwork_and_keeps_local() {
    let remote = build_record(eggwork_target());
    assert_eq!(executor_kind_for_job(&remote), Some(ExecutorKind::Eggwork));
    let local = build_record(ExecutionTarget::Local);
    assert_eq!(
        executor_kind_for_job(&local),
        Some(ExecutorKind::ManagedArgv)
    );
    // Even Test payloads route to Eggwork for remote targets (validation
    // defers them with a typed error rather than running locally).
    let mut remote_test = build_record(eggwork_target());
    remote_test.kind = JobKind::Test;
    remote_test.payload = JobPayload::Test {
        command: "cargo test".to_string(),
        argv: vec!["cargo".to_string(), "test".to_string()],
        cwd: None,
        scope: None,
        parent_run_id: None,
    };
    assert_eq!(
        executor_kind_for_job(&remote_test),
        Some(ExecutorKind::Eggwork)
    );
}

#[test]
fn eggwork_executor_is_unreachable_for_local_jobs() {
    let exec = EggworkExecutor::new(EggworkExecutorConfig::default());
    let local = build_record(ExecutionTarget::Local);
    assert!(exec.validate(&local).is_err());
}

// ── Durable target round-trip ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn sqlite_target_and_handle_round_trip() {
    let pool = common::pool::isolated_pool().await;
    let store = SqliteJobStore::new(pool);
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg-sql"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let loaded = store.get_job(&record.job_id).await.unwrap().unwrap();
    assert!(matches!(
        loaded.target,
        ExecutionTarget::EggworkNode { ref node_id } if node_id == "node-1"
    ));
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-sql"))
        .await
        .unwrap();
    let handle = codegg_core::jobs::RemoteExecutionHandle {
        schema_version: codegg_core::jobs::RemoteExecutionHandle::SCHEMA_VERSION,
        node_id: "node-1".to_string(),
        execution_id: "exec-sql-1".to_string(),
        generation: 1,
        lease_id: "lease-sql-1".to_string(),
    };
    store
        .set_attempt_remote_handle(&attempt.attempt_id, Some(&handle))
        .await
        .unwrap();
    let attempts = store.list_attempts(&record.job_id).await.unwrap();
    let persisted = attempts
        .iter()
        .find(|a| a.attempt_id == attempt.attempt_id)
        .and_then(|a| a.remote_handle.clone())
        .unwrap();
    assert_eq!(persisted.execution_id, "exec-sql-1");
    assert_eq!(persisted.generation, 1);
    assert_eq!(persisted.lease_id, "lease-sql-1");
}

// ── Lease identity through durable stores ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn in_memory_handle_round_trip_preserves_lease() {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-mem"))
        .await
        .unwrap();
    let handle = codegg_core::jobs::RemoteExecutionHandle::from_parts(
        "node-1".to_string(),
        "exec-mem-1".to_string(),
        1,
        "lease-mem-1".to_string(),
    );
    store
        .set_attempt_remote_handle(&attempt.attempt_id, Some(&handle))
        .await
        .unwrap();
    let attempts = store.list_attempts(&record.job_id).await.unwrap();
    let persisted = attempts
        .iter()
        .find(|a| a.attempt_id == attempt.attempt_id)
        .and_then(|a| a.remote_handle.clone())
        .unwrap();
    assert_eq!(persisted.execution_id, "exec-mem-1");
    assert_eq!(persisted.generation, 1);
    assert_eq!(persisted.lease_id, "lease-mem-1");
    // The public reconstruction path rebuilds the exact fenced tuple.
    let rebuilt = codegg::scheduler::to_eggwork_handle(&persisted).unwrap();
    assert_eq!(rebuilt.execution_id.as_str(), "exec-mem-1");
    assert_eq!(rebuilt.generation.get(), 1);
    assert_eq!(rebuilt.lease_id.as_str(), "lease-mem-1");
}

/// C001: the lease submitted to the node must equal the persisted lease,
/// and retransmission of the same attempt must reuse it without a second
/// submit. Under the pre-corrective two-token implementation the first
/// assertion fails (submitted != persisted).
#[tokio::test(flavor = "current_thread")]
async fn submitted_lease_matches_persisted_handle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(
            &record.job_id,
            &DaemonGeneration::new_unchecked("gen-lease"),
        )
        .await
        .unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, Some(store.clone()));
    let mut job = build_record(eggwork_target());
    job.job_id = record.job_id.clone();
    let mut first_ctx = bound_context(job.clone(), dir.path().to_path_buf());
    first_ctx.attempt_id = attempt.attempt_id.clone();
    let first = exec.execute(first_ctx).await;
    assert_eq!(first.status, ExecutorStatus::Completed);
    let attempts = store.list_attempts(&record.job_id).await.unwrap();
    let persisted = attempts
        .iter()
        .find(|a| a.attempt_id == attempt.attempt_id)
        .and_then(|a| a.remote_handle.clone())
        .expect("remote handle persisted");
    let submitted: Vec<ExecutionHandle> = client.submitted_handles.lock().unwrap().clone();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].execution_id.as_str(), persisted.execution_id);
    assert_eq!(submitted[0].generation.get(), persisted.generation);
    assert_eq!(submitted[0].lease_id.as_str(), persisted.lease_id);
    // Retransmission of the same attempt reuses the exact persisted lease.
    let mut second_ctx = bound_context(job, dir.path().to_path_buf());
    second_ctx.attempt_id = attempt.attempt_id.clone();
    let second = exec.execute(second_ctx).await;
    assert_eq!(second.status, ExecutorStatus::Completed);
    assert_eq!(client.submits.load(Ordering::SeqCst), 1);
}

/// The fenced scripted seam rejects a tampered lease with typed
/// `invalid_lease` while the accepted lease still controls the execution.
#[tokio::test(flavor = "current_thread")]
async fn scripted_fencing_rejects_wrong_lease() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    client.enable_lease_fencing();
    let exec = executor_with(&client, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Completed);
    let accepted = client.submitted_handles.lock().unwrap()[0].clone();
    let forged = ExecutionHandle {
        execution_id: accepted.execution_id.clone(),
        generation: accepted.generation,
        lease_id: LeaseId::new("codegg-lease-forged-wrong-token").unwrap(),
    };
    let cancel_err = client.cancel(&forged).await.unwrap_err();
    assert!(
        matches!(
            cancel_err,
            EggworkClientError::Api { status: 403, ref code, .. } if code == "invalid_lease"
        ),
        "expected typed invalid_lease, got {cancel_err:?}"
    );
    let renew_err = client.renew(&forged, "rn-1").await.unwrap_err();
    assert!(
        matches!(
            renew_err,
            EggworkClientError::Api { status: 403, ref code, .. } if code == "invalid_lease"
        ),
        "expected typed invalid_lease, got {renew_err:?}"
    );
    // The accepted lease still authorizes control operations.
    assert!(client.cancel(&accepted).await.is_ok());
    assert!(client.renew(&accepted, "rn-1").await.is_ok());
}

// ── Pre-flight failures ────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn capability_mismatch_fails_before_upload() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    *client.capabilities.lock().unwrap() = test_capabilities(vec!["other.v9".to_string()]);
    let exec = executor_with(&client, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Failed);
    assert!(completion.summary.contains("capability"));
    assert_eq!(client.uploads.load(Ordering::SeqCst), 0);
    assert_eq!(client.submits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn busy_and_draining_nodes_stay_distinguishable() {
    let dir = tempfile::tempdir().unwrap();
    let busy = Arc::new(ScriptedClient::succeeding());
    *busy.status.lock().unwrap() = test_status(false, 4);
    let exec = executor_with(&busy, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Failed);
    assert!(completion.summary.contains("busy"));

    let draining = Arc::new(ScriptedClient::succeeding());
    *draining.status.lock().unwrap() = test_status(true, 0);
    let exec = executor_with(&draining, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Failed);
    assert!(completion.summary.contains("draining"));
}

// ── Fixture: argv preserved exactly ────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn remote_echo_argv_fixture_preserves_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Completed);
    let submitted = client.submitted_argv.lock().unwrap();
    assert_eq!(submitted.len(), 1);
    assert_eq!(
        submitted[0],
        vec!["echo".to_string(), "hello world".to_string()]
    );
}

// ── Idempotency: one attempt, one execution ────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn duplicate_execute_reuses_persisted_handle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-dup"))
        .await
        .unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, Some(store.clone()));
    let mut job = build_record(eggwork_target());
    job.job_id = record.job_id.clone();
    let mut first_ctx = bound_context(job.clone(), dir.path().to_path_buf());
    first_ctx.attempt_id = attempt.attempt_id.clone();
    let first = exec.execute(first_ctx).await;
    assert_eq!(first.status, ExecutorStatus::Completed);
    // Second execute() for the SAME attempt observes instead of resubmitting.
    let mut second_ctx = bound_context(job, dir.path().to_path_buf());
    second_ctx.attempt_id = attempt.attempt_id.clone();
    let second = exec.execute(second_ctx).await;
    assert_eq!(second.status, ExecutorStatus::Completed);
    assert_eq!(client.submits.load(Ordering::SeqCst), 1);
    let _ = attempt;
}

// ── Restart reconciliation ─────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_after_remote_acceptance_does_not_resubmit() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-r1"))
        .await
        .unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, Some(store.clone()));
    let mut job = build_record(eggwork_target());
    job.job_id = record.job_id.clone();
    // First execution persists the handle (simulates pre-restart acceptance).
    let mut first_ctx = bound_context(job.clone(), dir.path().to_path_buf());
    first_ctx.attempt_id = attempt.attempt_id.clone();
    let first = exec.execute(first_ctx).await;
    assert_eq!(first.status, ExecutorStatus::Completed);
    assert_eq!(client.submits.load(Ordering::SeqCst), 1);
    // "Restarted" executor with a fresh client that would record a second
    // submit: reconciliation must observe instead.
    let restarted = Arc::new(ScriptedClient::succeeding());
    let exec2 = executor_with(&restarted, Some(store.clone()));
    let mut ctx = bound_context(job, dir.path().to_path_buf());
    ctx.attempt_id = attempt.attempt_id.clone();
    let second = exec2.execute(ctx).await;
    assert_eq!(second.status, ExecutorStatus::Completed);
    assert!(second.summary.contains("reconciled=true"));
    assert_eq!(restarted.submits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_with_live_remote_execution_cancels_and_interrupts() {
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string()],
                cwd: None,
            },
            resource_request: ResourceRequest::default(),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: eggwork_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-r2"))
        .await
        .unwrap();
    // Persist a handle as if a pre-restart execution accepted it, then
    // observe reports it still Running.
    let handle = codegg_core::jobs::RemoteExecutionHandle {
        schema_version: codegg_core::jobs::RemoteExecutionHandle::SCHEMA_VERSION,
        node_id: "node-1".to_string(),
        execution_id: "exec-live-1".to_string(),
        generation: 1,
        lease_id: "lease-live-1".to_string(),
    };
    store
        .set_attempt_remote_handle(&attempt.attempt_id, Some(&handle))
        .await
        .unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    *client.snapshot.lock().unwrap() = ExecutionSnapshot {
        state: ExecutionState::Running,
        result: None,
        ..succeeded_snapshot()
    };
    let exec = executor_with(&client, Some(store));
    let mut job = build_record(eggwork_target());
    job.job_id = record.job_id.clone();
    let mut ctx = bound_context(job, dir.path().to_path_buf());
    ctx.attempt_id = attempt.attempt_id.clone();
    let completion = exec.execute(ctx).await;
    assert_eq!(completion.status, ExecutorStatus::Interrupted);
    assert_eq!(client.submits.load(Ordering::SeqCst), 0);
    assert_eq!(client.cancels.lock().unwrap().as_slice(), ["exec-live-1"]);
}

// ── Cancellation ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_running_cancels_exact_handle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    *client.hang_events.lock().unwrap() = true;
    let exec = executor_with(&client, None);
    let ctx = bound_context(build_record(eggwork_target()), dir.path().to_path_buf());
    let token = ctx.cancellation.clone();
    let run = tokio::spawn(async move { exec.execute(ctx).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    token.cancel();
    let completion = run.await.unwrap();
    assert_eq!(completion.status, ExecutorStatus::Cancelled);
    assert_eq!(client.submits.load(Ordering::SeqCst), 1);
    assert_eq!(client.cancels.lock().unwrap().len(), 1);
}

// ── Lease renewal ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_is_renewed_while_execution_is_live() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    *client.hang_events.lock().unwrap() = true;
    // Observe reports Running so the run stays live until we cancel.
    *client.snapshot.lock().unwrap() = ExecutionSnapshot {
        state: ExecutionState::Running,
        result: None,
        ..succeeded_snapshot()
    };
    let mut nodes = HashMap::new();
    nodes.insert("node-1".to_string(), test_node());
    let exec = EggworkExecutor::with_factory(
        EggworkExecutorConfig {
            nodes,
            store: None,
            lease_renew_interval: Duration::from_millis(50),
        },
        Arc::new(ScriptedFactory {
            client: client.clone(),
        }),
    );
    let ctx = bound_context(build_record(eggwork_target()), dir.path().to_path_buf());
    let token = ctx.cancellation.clone();
    let run = tokio::spawn(async move { exec.execute(ctx).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    token.cancel();
    let completion = run.await.unwrap();
    assert_eq!(completion.status, ExecutorStatus::Cancelled);
    assert!(
        client.renews.load(Ordering::SeqCst) >= 1,
        "expected at least one lease renewal"
    );
}

// ── Workspace isolation ────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cwd_outside_workspace_lease_root_fails() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, None);
    let mut job = build_record(eggwork_target());
    job.payload = JobPayload::ManagedArgv {
        argv: vec!["echo".to_string()],
        cwd: Some("/etc".to_string()),
    };
    let completion = exec
        .execute(bound_context(job, dir.path().to_path_buf()))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Failed);
    assert_eq!(client.submits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn symlink_entries_are_skipped_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), b"data").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();
    let client = Arc::new(ScriptedClient::succeeding());
    let exec = executor_with(&client, None);
    let completion = exec
        .execute(bound_context(
            build_record(eggwork_target()),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Completed);
    // Only the regular file uploads; the symlink is skipped.
    assert_eq!(client.uploads.load(Ordering::SeqCst), 1);
}

// ── Artifact import ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn declared_artifacts_import_into_runstore() {
    let dir = tempfile::tempdir().unwrap();
    let run_store = Arc::new(MemRunStore::new());
    let client = Arc::new(ScriptedClient::succeeding());
    *client.artifacts.lock().unwrap() = vec![ArtifactRecord {
        artifact_id: ArtifactId::new("art-1").unwrap(),
        execution_id: ExecutionId::new("exec-1").unwrap(),
        generation: ExecutionGeneration::new(1).unwrap(),
        path: RelativePath::new("out/result.json").unwrap(),
        kind: ArtifactType::File,
        digest: BlobDigest::from_bytes(b"{}"),
        size_bytes: 2,
        executable: false,
        created_unix_ms: 0,
        expires_unix_ms: u64::MAX,
    }];
    *client.artifact_bytes.lock().unwrap() = b"{}".to_vec();
    let exec = executor_with(&client, None);
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let mut resources = codegg::scheduler::ResourcePermitGuard::new_orphan(Default::default());
    resources.install_controller(controller);
    let ctx = JobExecutionContext {
        job: build_record(eggwork_target()),
        attempt_id: AttemptId::new_unchecked("att-art-1"),
        daemon_generation: DaemonGeneration::new_unchecked("gen-art"),
        workspace_id: WorkspaceId::new_unchecked("ws-egg"),
        workspace_root: dir.path().to_path_buf(),
        run_store: Some(run_store),
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(codegg::scheduler::NoopProgressSink),
        resources,
    };
    let completion = exec.execute(ctx).await;
    assert_eq!(completion.status, ExecutorStatus::Completed);
    assert!(completion.run_id.is_some());
    assert!(completion.summary.contains("artifacts_imported=1"));
}

// ── Scheduler end-to-end (no local fallback) ───────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_end_to_end_remote_only() {
    let root = tempfile::tempdir().unwrap();
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .unwrap();
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .unwrap();
    std::fs::write(root.path().join("build.sh"), b"#!/bin/sh\necho ok\n").unwrap();
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
        DaemonGeneration::new_unchecked("gen-e2e"),
    );
    // Only the Eggwork executor is registered: a remote failure cannot
    // silently fall back to local execution (the job would go unschedulable).
    let client = Arc::new(ScriptedClient::succeeding());
    let mut nodes = HashMap::new();
    nodes.insert("node-1".to_string(), test_node());
    scheduler
        .register_executor(Arc::new(EggworkExecutor::with_factory(
            EggworkExecutorConfig {
                nodes,
                store: Some(store.clone()),
                ..Default::default()
            },
            Arc::new(ScriptedFactory {
                client: client.clone(),
            }),
        )))
        .await
        .unwrap();
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("gen-e2e"),
    );
    let submitted = submission
        .submit(
            None,
            NewJob {
                workspace_id: workspace.id.clone(),
                session_id: None,
                turn_id: None,
                kind: JobKind::Build,
                source: JobSource::Interactive,
                priority: JobPriority::Interactive,
                payload: JobPayload::ManagedArgv {
                    argv: vec!["sh".to_string(), "build.sh".to_string()],
                    cwd: None,
                },
                resource_request: ResourceRequest::default(),
                timeout: None,
                retry_policy: RetryPolicy::no_retry(),
                idempotency: IdempotencyClass::SafeRepeat,
                not_before: None,
                deadline: None,
                schedule_id: None,
                depends_on: Vec::new(),
                parent_job_id: None,
                parent_attempt_id: None,
                parent_call_id: None,
                parent_program_id: None,
                parent_instruction_sequence: None,
                relation_kind: None,
                target: eggwork_target(),
            },
        )
        .await
        .unwrap();
    let _handle = scheduler.clone().spawn_run();
    let completed = scheduler
        .wait_for_completion(&submitted.job_id, Duration::from_secs(20))
        .await
        .unwrap();
    assert_eq!(
        completed.status,
        codegg::scheduler::ExecutorStatus::Completed
    );
    // Provenance shows the Eggwork executor owned the attempt.
    let attempts = store.list_attempts(&submitted.job_id).await.unwrap();
    assert!(
        attempts
            .iter()
            .any(|a| a.executor.as_deref() == Some("eggwork")),
        "expected eggwork executor provenance, got {:?}",
        attempts.iter().map(|a| &a.executor).collect::<Vec<_>>()
    );
    assert!(
        attempts
            .iter()
            .any(|a| a.remote_handle.as_ref().map(|h| h.node_id.as_str()) == Some("node-1")),
        "expected persisted remote handle"
    );
    let jobs = store.list_jobs(JobStoreQuery::default()).await.unwrap();
    assert!(jobs
        .iter()
        .find(|j| j.job_id == submitted.job_id)
        .map(|j| j.state.is_terminal())
        .unwrap_or(false));
}
