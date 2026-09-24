//! Eggwork corrective C001 live-node qualification (Linux).
//!
//! Exercises the production [`NodeClientFactory`](codegg::scheduler::EggworkClientFactory)
//! path against a real Eggwork node over mTLS: exact lease-fenced
//! renew/cancel, wrong-lease rejection, and restart reconciliation through
//! the `EggworkExecutor` (no scripted fake).
//!
//! The node runs as a bounded `codegg-eggwork-test-node` subprocess (built
//! from `crates/eggwork-test-node`, same immutable Eggwork revision as
//! production). An in-process fixture is impractical: `eggwork-server`
//! links `rusqlite -> libsqlite3-sys 0.38`, which cannot resolve in the
//! same cargo graph as the workspace's `sqlx-sqlite -> libsqlite3-sys
//! 0.28` (`links = "sqlite3"` must be unique). The subprocess uses the
//! same immutable revision with deterministic lifecycle and guaranteed
//! cleanup, and `LocalProcessRunner` remains the node's canonical process
//! owner — no second production process owner is created in CodeGG.
//!
//! Linux-only: `eggwork-runner` depends on Linux-only `landlock`, so the
//! helper cannot compile on macOS. On other platforms this target builds
//! zero tests; Linux CI qualifies the gate.

#![cfg(target_os = "linux")]

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use codegg::scheduler::{
    to_eggwork_handle, AdmissionController, BoxEventStream, EggworkClientFactory, EggworkExecutor,
    EggworkExecutorConfig, EggworkNodeClient, ExecutorStatus, JobExecutionContext, JobExecutor,
    JobScheduler, JobSubmissionService, NodeClientFactory, ResolvedEggworkNode,
    ResolvedSchedulerConfig, ResourcePermitGuard,
};
use codegg_core::jobs::{
    AttemptId, DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobId,
    JobKind, JobPayload, JobPriority, JobRecord, JobSource, JobStore, NewJob,
    RemoteExecutionHandle, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;
use eggfetch_core::{BoxBytesStream, Error as EggfetchError};
use eggwork_client::{ClientError as EggworkClientError, WorkspaceReady};
use eggwork_core::{
    ArtifactRecord, BlobDigest, ExecutionHandle, ExecutionId, ExecutionSnapshot, ExecutionSpec,
    ExecutionState, LeaseId, NodeCapabilities, NodeStatus, WorkspaceId as EggworkWorkspaceId,
    WorkspaceManifest,
};
use futures_util::StreamExt;
use tokio::io::AsyncBufReadExt;

// ── TLS material (ephemeral test CA) ─────────────────────────────────────────

fn init_crypto() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

struct IssuedIdentity {
    cert_pem: String,
    key_pem: String,
}

fn issue_identity(
    issuer: &rcgen::Certificate,
    issuer_key: &rcgen::KeyPair,
    sans: Vec<String>,
) -> IssuedIdentity {
    let key = rcgen::KeyPair::generate().unwrap();
    let params = rcgen::CertificateParams::new(sans).unwrap();
    let cert = params.signed_by(&key, issuer, issuer_key).unwrap();
    IssuedIdentity {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
    }
}

fn test_ca() -> (Vec<u8>, rcgen::Certificate, rcgen::KeyPair) {
    init_crypto();
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca_params =
        rcgen::CertificateParams::new(vec!["eggwork live-test CA".to_string()]).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let root = ca.der().to_vec();
    (root, ca, ca_key)
}

fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let lines = encoded
        .as_bytes()
        .chunks(64)
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

fn write_private(path: &Path, contents: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    use std::io::Write;
    options
        .open(path)
        .unwrap()
        .write_all(contents.as_bytes())
        .unwrap();
}

// ── Live fixture (bounded helper subprocess) ───────────────────────────────────

/// Resolve the fixture helper binary, building it on demand. The helper is
/// a separate workspace (own lockfile) so it resolves `eggwork-server`
/// without this workspace's `sqlx` sqlite linkage. `CODEGG_EGGWORK_TEST_NODE`
/// overrides the path explicitly (e.g. a prebuilt binary in CI images).
fn helper_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CODEGG_EGGWORK_TEST_NODE") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates/eggwork-test-node/target/debug/codegg-eggwork-test-node")
}

async fn ensure_helper_built(path: &Path) {
    use tokio::sync::OnceCell;
    static BUILT: OnceCell<()> = OnceCell::const_new();
    BUILT
        .get_or_init(|| async {
            if path.exists() {
                return;
            }
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("crates/eggwork-test-node/Cargo.toml");
            let output = tokio::time::timeout(
                Duration::from_secs(600),
                tokio::process::Command::new(cargo)
                    .arg("build")
                    .arg("--locked")
                    .arg("--manifest-path")
                    .arg(&manifest)
                    .output(),
            )
            .await
            .expect("fixture helper build must complete")
            .expect("failed to run cargo for fixture helper");
            assert!(
                output.status.success() && path.exists(),
                "fixture helper build failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        })
        .await;
}

struct LiveNode {
    _temp: tempfile::TempDir,
    child: Option<tokio::process::Child>,
    node: ResolvedEggworkNode,
    factory: Arc<CountingFactory>,
}

impl LiveNode {
    async fn start() -> Self {
        init_crypto();
        let temp = tempfile::TempDir::new().unwrap();
        let (root, ca, ca_key) = test_ca();
        let server_identity = issue_identity(&ca, &ca_key, vec!["localhost".to_string()]);
        let client_identity =
            issue_identity(&ca, &ca_key, vec!["eggwork-live-controller".to_string()]);

        // Trust/identity material lives only in temp files; key files are
        // created with restrictive permissions and never logged.
        let ca_path = temp.path().join("ca.pem");
        let client_cert_path = temp.path().join("client.pem");
        let client_key_path = temp.path().join("client-key.pem");
        let server_cert_path = temp.path().join("server.pem");
        let server_key_path = temp.path().join("server-key.pem");
        std::fs::write(&ca_path, pem("CERTIFICATE", &root)).unwrap();
        std::fs::write(&client_cert_path, &client_identity.cert_pem).unwrap();
        write_private(&client_key_path, &client_identity.key_pem);
        std::fs::write(&server_cert_path, &server_identity.cert_pem).unwrap();
        write_private(&server_key_path, &server_identity.key_pem);

        let helper = helper_binary();
        ensure_helper_built(&helper).await;
        let mut child = tokio::process::Command::new(&helper)
            .arg("--node-id")
            .arg("live-node-1")
            .arg("--bind")
            .arg("127.0.0.1:0")
            .arg("--db")
            .arg(temp.path().join("node.sqlite"))
            .arg("--exec-root")
            .arg(temp.path().join("exec-root"))
            .arg("--blob-root")
            .arg(temp.path().join("blobs"))
            .arg("--workspace-root")
            .arg(temp.path().join("workspaces"))
            .arg("--server-cert")
            .arg(&server_cert_path)
            .arg("--server-key")
            .arg(&server_key_path)
            .arg("--ca")
            .arg(&ca_path)
            .arg("--client-cert")
            .arg(&client_cert_path)
            .arg("--principal")
            .arg("live-test-controller")
            .arg("--lease-ttl-secs")
            .arg("300")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn fixture helper");

        // Readiness protocol: the helper prints `READY port=<N>`.
        // Anything else on stdout is diagnostic context for failures.
        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let port: u16 = tokio::time::timeout(Duration::from_secs(60), async {
            let mut last_line = String::new();
            loop {
                match lines.next_line().await.expect("helper stdout") {
                    Some(line) => {
                        if let Some(rest) = line.strip_prefix("READY port=") {
                            return rest.trim().parse().expect("READY port parses");
                        }
                        last_line = line;
                    }
                    None => {
                        let status = child.wait().await.ok();
                        panic!(
                            "helper exited before READY \
                             (status={status:?}, last={last_line:?})"
                        );
                    }
                }
            }
        })
        .await
        .expect("helper must become ready");
        // Drain stderr so server-side logging can never block the node on
        // a full pipe; startup failures surface through stdout instead.
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut sink = vec![0u8; 8192];
                let mut reader = stderr;
                while reader.read(&mut sink).await.map(|n| n > 0).unwrap_or(false) {}
            });
        }

        let node = ResolvedEggworkNode {
            node_id: "live-node-1".to_string(),
            endpoint: format!("https://localhost:{port}"),
            ca_cert: ca_path,
            client_cert: client_cert_path,
            client_key: client_key_path,
            required_capabilities: Vec::new(),
        };
        Self {
            _temp: temp,
            child: Some(child),
            node,
            factory: Arc::new(CountingFactory::new()),
        }
    }

    /// Production client built by the real `NodeClientFactory`.
    fn client(&self) -> Arc<dyn EggworkNodeClient> {
        NodeClientFactory
            .client_for(&self.node)
            .expect("production client")
    }

    fn executor(&self, store: Option<Arc<dyn JobStore>>) -> (EggworkExecutor, Arc<AtomicUsize>) {
        let mut nodes = HashMap::new();
        nodes.insert("live-node-1".to_string(), self.node.clone());
        let submits = self.factory.submits();
        let exec = EggworkExecutor::with_factory(
            EggworkExecutorConfig {
                nodes,
                store,
                ..Default::default()
            },
            self.factory.clone(),
        );
        (exec, submits)
    }

    async fn shutdown(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

// ── Counting wrapper: production factory stays in the path ───────────────────

struct CountingFactory {
    inner: NodeClientFactory,
    submits: Arc<AtomicUsize>,
    submitted_handles: Arc<Mutex<Vec<ExecutionHandle>>>,
}

impl CountingFactory {
    fn new() -> Self {
        Self {
            inner: NodeClientFactory,
            submits: Arc::new(AtomicUsize::new(0)),
            submitted_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn submits(&self) -> Arc<AtomicUsize> {
        self.submits.clone()
    }
}

impl EggworkClientFactory for CountingFactory {
    fn client_for(&self, node: &ResolvedEggworkNode) -> Result<Arc<dyn EggworkNodeClient>, String> {
        let client = self.inner.client_for(node)?;
        Ok(Arc::new(CountingClient {
            inner: client,
            submits: self.submits.clone(),
            submitted_handles: self.submitted_handles.clone(),
        }))
    }
}

struct CountingClient {
    inner: Arc<dyn EggworkNodeClient>,
    submits: Arc<AtomicUsize>,
    submitted_handles: Arc<Mutex<Vec<ExecutionHandle>>>,
}

#[async_trait]
impl EggworkNodeClient for CountingClient {
    async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError> {
        self.inner.capabilities().await
    }

    async fn status(&self) -> Result<NodeStatus, EggworkClientError> {
        self.inner.status().await
    }

    async fn find_missing_blobs(
        &self,
        digests: &[BlobDigest],
    ) -> Result<Vec<BlobDigest>, EggworkClientError> {
        self.inner.find_missing_blobs(digests).await
    }

    async fn upload_blob(
        &self,
        digest: &BlobDigest,
        declared_length: u64,
        stream: BoxBytesStream,
    ) -> Result<(), EggworkClientError> {
        self.inner
            .upload_blob(digest, declared_length, stream)
            .await
    }

    async fn create_workspace(
        &self,
        workspace_id: &EggworkWorkspaceId,
        handle: &ExecutionHandle,
        manifest: &WorkspaceManifest,
    ) -> Result<WorkspaceReady, EggworkClientError> {
        self.inner
            .create_workspace(workspace_id, handle, manifest)
            .await
    }

    async fn execute_in_workspace(
        &self,
        spec: &ExecutionSpec,
        handle: &ExecutionHandle,
        workspace_id: &EggworkWorkspaceId,
    ) -> Result<BoxEventStream, EggworkClientError> {
        self.submits.fetch_add(1, Ordering::SeqCst);
        self.submitted_handles.lock().unwrap().push(handle.clone());
        self.inner
            .execute_in_workspace(spec, handle, workspace_id)
            .await
    }

    async fn observe_generation(
        &self,
        id: &ExecutionId,
        generation: u64,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.inner.observe_generation(id, generation).await
    }

    async fn cancel(
        &self,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.inner.cancel(handle).await
    }

    async fn renew(
        &self,
        handle: &ExecutionHandle,
        renewal_id: &str,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.inner.renew(handle, renewal_id).await
    }

    async fn artifacts(
        &self,
        execution_id: &ExecutionId,
        generation: u64,
    ) -> Result<Vec<ArtifactRecord>, EggworkClientError> {
        self.inner.artifacts(execution_id, generation).await
    }

    async fn download_artifact(
        &self,
        artifact: &ArtifactRecord,
    ) -> Result<BoxBytesStream, EggworkClientError> {
        self.inner.download_artifact(artifact).await
    }
}

// ── Job helpers ──────────────────────────────────────────────────────────────

fn live_target() -> ExecutionTarget {
    ExecutionTarget::EggworkNode {
        node_id: "live-node-1".to_string(),
    }
}

fn argv_record(argv: Vec<String>) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked("j-live"),
        workspace_id: WorkspaceId::new_unchecked("ws-live"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Build,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv { argv, cwd: None },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        state: codegg_core::jobs::JobState::Queued,
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
        target: live_target(),
    }
}

fn live_context(job: JobRecord, attempt: AttemptId, root: PathBuf) -> JobExecutionContext {
    let controller = Arc::new(AdmissionController::new(ResolvedSchedulerConfig::default()));
    let mut resources = ResourcePermitGuard::new_orphan(Default::default());
    resources.install_controller(controller);
    JobExecutionContext {
        job,
        attempt_id: attempt,
        daemon_generation: DaemonGeneration::new_unchecked("gen-live"),
        workspace_id: WorkspaceId::new_unchecked("ws-live"),
        workspace_root: root,
        run_store: None,
        cancellation: tokio_util::sync::CancellationToken::new(),
        progress: Arc::new(codegg::scheduler::NoopProgressSink),
        resources,
    }
}

async fn create_remote_job(
    store: &Arc<dyn JobStore>,
    argv: Vec<String>,
) -> (JobRecord, codegg_core::jobs::JobAttempt) {
    let record = store
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-live"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv { argv, cwd: None },
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
            target: live_target(),
        })
        .await
        .unwrap();
    let attempt = store
        .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-live"))
        .await
        .unwrap();
    (record, attempt)
}

async fn wait_for_persisted(
    store: &Arc<dyn JobStore>,
    job: &JobId,
    attempt: &AttemptId,
) -> RemoteExecutionHandle {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let attempts = store.list_attempts(job).await.unwrap();
        if let Some(handle) = attempts
            .iter()
            .find(|a| a.attempt_id == *attempt)
            .and_then(|a| a.remote_handle.clone())
        {
            return handle;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for persisted remote handle"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_acceptance(client: &Arc<dyn EggworkNodeClient>, handle: &RemoteExecutionHandle) {
    let id = ExecutionId::new(&handle.execution_id).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if client
            .observe_generation(&id, handle.generation)
            .await
            .is_ok()
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for remote acceptance"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_terminal(
    client: &Arc<dyn EggworkNodeClient>,
    handle: &RemoteExecutionHandle,
) -> ExecutionSnapshot {
    let id = ExecutionId::new(&handle.execution_id).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = client
            .observe_generation(&id, handle.generation)
            .await
            .expect("observe persisted execution");
        if matches!(
            snapshot.state,
            ExecutionState::Succeeded
                | ExecutionState::Failed
                | ExecutionState::Cancelled
                | ExecutionState::TimedOut
                | ExecutionState::Interrupted
        ) {
            return snapshot;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for terminal state (last={:?})",
            snapshot.state
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn assert_no_secret_in_summary(summary: &str) {
    assert!(
        !summary.contains("BEGIN PRIVATE KEY"),
        "completion summary must never carry key material"
    );
    assert!(
        !summary.contains("BEGIN CERTIFICATE"),
        "completion summary must never carry certificate material"
    );
}

// ── D1 + D2 + valid cancel + terminal observe ─────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_lease_fencing_renew_reject_cancel() {
    let live = LiveNode::start().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), b"live\n").unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let (record, attempt) =
        create_remote_job(&store, vec!["/bin/sleep".to_string(), "20".to_string()]).await;
    let (exec, submits) = live.executor(Some(store.clone()));
    let mut job = argv_record(vec!["/bin/sleep".to_string(), "20".to_string()]);
    job.job_id = record.job_id.clone();
    let ctx = live_context(job, attempt.attempt_id.clone(), dir.path().to_path_buf());
    let running = tokio::spawn(async move { exec.execute(ctx).await });

    // The executor persists the handle before side effects; wait until the
    // node has actually accepted the execution.
    let durable = wait_for_persisted(&store, &record.job_id, &attempt.attempt_id).await;
    let client = live.client();
    wait_for_acceptance(&client, &durable).await;
    let accepted = to_eggwork_handle(&durable).expect("reconstruct persisted handle");

    // D1: renew with the exact accepted/persisted handle succeeds.
    client
        .renew(&accepted, "rn-live-valid")
        .await
        .expect("valid renew succeeds before expiry");

    // D2: a cloned id/generation with a replaced lease is fenced out with
    // the typed contract error — never a display-string match.
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
        "expected typed invalid_lease on cancel, got {cancel_err:?}"
    );
    let renew_err = client.renew(&forged, "rn-live-forged").await.unwrap_err();
    assert!(
        matches!(
            renew_err,
            EggworkClientError::Api { status: 403, ref code, .. } if code == "invalid_lease"
        ),
        "expected typed invalid_lease on renew, got {renew_err:?}"
    );

    // Valid cancel under the persisted handle terminates the execution.
    client
        .cancel(&accepted)
        .await
        .expect("valid cancel succeeds");
    let terminal = wait_for_terminal(&client, &durable).await;
    assert_eq!(terminal.state, ExecutionState::Cancelled);
    let completion = tokio::time::timeout(Duration::from_secs(60), running)
        .await
        .expect("executor converges")
        .expect("join");
    assert_eq!(completion.status, ExecutorStatus::Cancelled);
    assert_no_secret_in_summary(&completion.summary);
    // Exactly one remote execution was accepted for the attempt.
    assert_eq!(submits.load(Ordering::SeqCst), 1);
    live.shutdown().await;
}

// ── D4: live restart reconciliation ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_restart_live_reconciliation_cancels_under_persisted_handle() {
    let live = LiveNode::start().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), b"live\n").unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let (record, attempt) =
        create_remote_job(&store, vec!["/bin/sleep".to_string(), "30".to_string()]).await;
    let (exec1, submits) = live.executor(Some(store.clone()));
    let mut job = argv_record(vec!["/bin/sleep".to_string(), "30".to_string()]);
    job.job_id = record.job_id.clone();
    let ctx1 = live_context(
        job.clone(),
        attempt.attempt_id.clone(),
        dir.path().to_path_buf(),
    );
    let first = tokio::spawn(async move { exec1.execute(ctx1).await });

    let durable = wait_for_persisted(&store, &record.job_id, &attempt.attempt_id).await;
    let probe = live.client();
    wait_for_acceptance(&probe, &durable).await;

    // Daemon-restart analogue: a fresh executor reconciles the same
    // attempt. It must observe (not resubmit), cancel under the exact
    // persisted handle, and report the conservative disposition.
    let (exec2, _) = live.executor(Some(store.clone()));
    let ctx2 = live_context(job, attempt.attempt_id.clone(), dir.path().to_path_buf());
    let reconciled = exec2.execute(ctx2).await;
    assert_eq!(reconciled.status, ExecutorStatus::Interrupted);
    assert_no_secret_in_summary(&reconciled.summary);

    // The pre-restart run converges as cancelled once its execution is
    // cancelled out from under it; the node reaches terminal cancellation
    // under a bounded deadline.
    let first_completion = tokio::time::timeout(Duration::from_secs(60), first)
        .await
        .expect("first run converges")
        .expect("join");
    assert_eq!(first_completion.status, ExecutorStatus::Cancelled);
    let terminal = wait_for_terminal(&probe, &durable).await;
    assert_eq!(terminal.state, ExecutionState::Cancelled);

    // No second remote execution was accepted on reconnect, and the
    // submitted tuple is exactly the persisted tuple.
    assert_eq!(submits.load(Ordering::SeqCst), 1);
    let submitted: Vec<ExecutionHandle> =
        live.factory.submitted_handles.lock().unwrap().clone();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].execution_id.as_str(), durable.execution_id);
    assert_eq!(submitted[0].generation.get(), durable.generation);
    assert_eq!(submitted[0].lease_id.as_str(), durable.lease_id);
    live.shutdown().await;
}

// ── D5: terminal restart reconciliation ──────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn live_restart_terminal_reconciliation_returns_result_without_resubmit() {
    let live = LiveNode::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "printf live-ok".to_string(),
    ];
    let (record, attempt) = create_remote_job(&store, argv.clone()).await;
    let (exec1, submits) = live.executor(Some(store.clone()));
    let mut job = argv_record(argv.clone());
    job.job_id = record.job_id.clone();
    let first = exec1
        .execute(live_context(
            job.clone(),
            attempt.attempt_id.clone(),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(first.status, ExecutorStatus::Completed);
    assert_no_secret_in_summary(&first.summary);

    // Fresh executor, same attempt: terminal snapshot maps back without a
    // second submit.
    let (exec2, _) = live.executor(Some(store.clone()));
    let second = exec2
        .execute(live_context(
            job,
            attempt.attempt_id.clone(),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(second.status, ExecutorStatus::Completed);
    assert!(
        second.summary.contains("reconciled=true"),
        "expected terminal reconciliation, got {}",
        second.summary
    );
    assert_eq!(submits.load(Ordering::SeqCst), 1);
    live.shutdown().await;
}

// ── Connection, capabilities, isolation ──────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn live_node_connection_capabilities_and_workspace_isolation() {
    let live = LiveNode::start().await;
    // Production factory connection plus capability/status projection.
    let client = live.client();
    let capabilities = client.capabilities().await.expect("capabilities");
    assert!(
        capabilities.features.iter().any(|f| f == "exec.argv.v1"),
        "node must advertise argv execution"
    );
    let status = client.status().await.expect("status");
    assert_eq!(status.node_id.as_str(), "live-node-1");
    assert!(!status.draining);

    // Bounded argv executes; remote mutation must not touch the local
    // workspace lease root.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.txt"), b"keep\n").unwrap();
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "touch REMOTE_ONLY_MARKER".to_string(),
    ];
    let (record, attempt) = create_remote_job(&store, argv.clone()).await;
    let (exec, _) = live.executor(Some(store.clone()));
    let mut job = argv_record(argv);
    job.job_id = record.job_id.clone();
    let completion = exec
        .execute(live_context(
            job,
            attempt.attempt_id.clone(),
            dir.path().to_path_buf(),
        ))
        .await;
    assert_eq!(completion.status, ExecutorStatus::Completed);
    assert_no_secret_in_summary(&completion.summary);
    assert!(
        !dir.path().join("REMOTE_ONLY_MARKER").exists(),
        "remote execution must not mutate the local workspace"
    );
    assert!(dir.path().join("keep.txt").exists());
    live.shutdown().await;
}

// ── Scheduler end-to-end on the live node (no local fallback) ────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_scheduler_end_to_end_remote_only() {
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };

    let live = LiveNode::start().await;
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
        DaemonGeneration::new_unchecked("gen-live-e2e"),
    );
    // Only the Eggwork executor is registered: a remote failure cannot
    // silently fall back to local execution.
    let mut nodes = HashMap::new();
    nodes.insert("live-node-1".to_string(), live.node.clone());
    scheduler
        .register_executor(Arc::new(EggworkExecutor::with_factory(
            EggworkExecutorConfig {
                nodes,
                store: Some(store.clone()),
                ..Default::default()
            },
            live.factory.clone(),
        )))
        .await
        .unwrap();
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("gen-live-e2e"),
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
                target: live_target(),
            },
        )
        .await
        .unwrap();
    let _handle = scheduler.clone().spawn_run();
    let completed = scheduler
        .wait_for_completion(&submitted.job_id, Duration::from_secs(90))
        .await
        .unwrap();
    assert_eq!(
        completed.status,
        codegg::scheduler::ExecutorStatus::Completed
    );
    let attempts = store.list_attempts(&submitted.job_id).await.unwrap();
    assert!(
        attempts
            .iter()
            .any(|a| a.executor.as_deref() == Some("eggwork")),
        "expected eggwork executor provenance"
    );
    let durable = attempts
        .iter()
        .find_map(|a| a.remote_handle.clone())
        .expect("persisted remote handle");
    assert_eq!(durable.node_id, "live-node-1");
    assert!(!durable.lease_id.is_empty());
    // The persisted handle still authorizes observation of the terminal
    // execution on the real node.
    let client = live.client();
    let rebuilt = to_eggwork_handle(&durable).expect("reconstruct");
    let snapshot = client
        .observe_generation(&rebuilt.execution_id, rebuilt.generation.get())
        .await
        .expect("observe terminal execution");
    assert_eq!(snapshot.state, ExecutionState::Succeeded);
    live.shutdown().await;
}

// ── Blob/workspace round-trip through the production client ──────────────────

#[tokio::test(flavor = "current_thread")]
async fn live_blob_upload_and_workspace_materialization() {
    let live = LiveNode::start().await;
    let client = live.client();
    let bytes = b"live blob payload".to_vec();
    let digest = BlobDigest::from_bytes(&bytes);
    let missing = client
        .find_missing_blobs(std::slice::from_ref(&digest))
        .await
        .expect("blob probe");
    assert_eq!(missing, vec![digest.clone()]);
    let stream: BoxBytesStream =
        futures_util::stream::iter(vec![Ok::<bytes::Bytes, EggfetchError>(bytes::Bytes::from(
            bytes.clone(),
        ))])
        .boxed();
    client
        .upload_blob(&digest, bytes.len() as u64, stream)
        .await
        .expect("blob upload");
    let missing_after = client
        .find_missing_blobs(std::slice::from_ref(&digest))
        .await
        .expect("blob probe after upload");
    assert!(missing_after.is_empty());
    live.shutdown().await;
}
