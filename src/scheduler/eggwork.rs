//! Fixed-target Eggwork remote execution (M001).
//!
//! The [`EggworkExecutor`] runs explicitly targeted finite jobs
//! ([`ExecutionTarget::EggworkNode`]) on one named Eggwork node. Routing is
//! owned by [`executor_kind_for_job`](crate::scheduler::executor::executor_kind_for_job):
//! remote-targeted jobs always land here, `Local` jobs never do, and remote
//! failure never falls back to local execution.
//!
//! Scope (see `plans/implementation/eggwork-fixed-target-remote-execution/`):
//!
//! - eligible payloads: `Build`/`Lint`/`Format`/`ManagedProcess`/`Shell`
//!   with canonical argv (`ManagedArgv`, or `Shell` with explicit argv);
//! - `Test` remote execution is deferred (typed validation failure);
//! - one CodeGG attempt maps to at most one accepted Eggwork execution;
//! - the remote handle is persisted via `set_attempt_remote_handle` before
//!   any upload/execute side effect so restart reconciles instead of
//!   resubmitting;
//! - workspace transfer is bounded and never writes remote results back
//!   into the local workspace;
//! - cancellation propagates to the exact remote handle; the scheduler
//!   permit is held until remote terminal convergence.
//!
//! This module never spawns a local process: execution happens on the
//! Eggwork node via `eggwork-client`. See `docs/execution-ownership.toml`
//! (`src/scheduler/` is scheduler-owned definition).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use codegg_core::jobs::{
    AttemptId, ExecutionTarget, JobKind, JobPayload, JobRecord, JobStore, RemoteExecutionHandle,
};
use codegg_core::run_store::{
    ActualBackend, ArtifactInput, ArtifactKind, BackendRecord, RiskRecord, RunCompletion, RunDraft,
    RunInvocation, RunKind, RunOwnership, RunStatus,
};
use eggfetch_core::{BoxBytesStream, Error as EggfetchError};
use eggwork_client::{ClientError as EggworkClientError, NodeClient};
use eggwork_core::{
    ArtifactRecord, BlobDigest, CommandSpec, ExecutionEvent, ExecutionEventKind,
    ExecutionGeneration, ExecutionHandle, ExecutionId, ExecutionResult, ExecutionSnapshot,
    ExecutionSpec, ExecutionState, IsolationRequirement, LeaseId, NetworkRequirement,
    NodeCapabilities, NodeStatus, OutputPolicy, RelativePath, Requirement, ResourceRequirements,
    StdinPolicy, WorkspaceEntry, WorkspaceId as EggworkWorkspaceId, WorkspaceManifest,
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use crate::scheduler::executor::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, ExecutorValidationError,
    JobExecutionContext, JobExecutor,
};

/// Maximum workspace manifest entries (matches the Eggwork server bound).
const MAX_WORKSPACE_ENTRIES: usize = 4096;
/// Maximum total logical bytes transferred for one attempt.
const MAX_WORKSPACE_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum bytes read/uploaded for a single file.
const MAX_WORKSPACE_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum traversal depth below the workspace root.
const MAX_WORKSPACE_DEPTH: usize = 128;
/// Upper bound for the mapped remote timeout (Eggwork maximum is 7 days).
const MAX_REMOTE_TIMEOUT_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Remote timeout when the job carries none.
const DEFAULT_REMOTE_TIMEOUT_MILLIS: u64 = 300_000;
/// Maximum progress messages forwarded to the sink per execution.
const MAX_PROGRESS_MESSAGES: usize = 200;
/// Maximum characters per forwarded progress message.
const MAX_PROGRESS_MESSAGE_CHARS: usize = 2048;
/// Maximum characters in the terminal summary.
const MAX_SUMMARY_CHARS: usize = 2000;
/// Maximum artifacts imported into the RunStore per execution.
const MAX_ARTIFACTS: usize = 16;
/// Maximum bytes imported per artifact.
const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
/// Interval between lease renewals while the remote execution is live.
const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(30);
/// Per-operation network bound (the scheduler also wraps `execute`).
const OP_TIMEOUT: Duration = Duration::from_secs(60);
/// Eggwork feature the node must advertise for argv execution.
const REQUIRED_EXEC_FEATURE: &str = "exec.argv.v1";

/// One resolved Eggwork node: endpoint plus local file references for TLS
/// trust and client identity. Key material is never loaded into this
/// struct — paths are handed to `eggwork-client` at use time.
#[derive(Debug, Clone)]
pub struct ResolvedEggworkNode {
    pub node_id: String,
    pub endpoint: String,
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
    pub required_capabilities: Vec<String>,
}

/// Executor configuration: named nodes plus the durable store used for
/// remote-handle persistence and restart reconciliation.
pub struct EggworkExecutorConfig {
    pub nodes: HashMap<String, ResolvedEggworkNode>,
    pub store: Option<Arc<dyn JobStore>>,
    /// Interval between lease renewals while the remote execution is live.
    pub lease_renew_interval: Duration,
}

impl Default for EggworkExecutorConfig {
    fn default() -> Self {
        Self {
            nodes: HashMap::new(),
            store: None,
            lease_renew_interval: LEASE_RENEW_INTERVAL,
        }
    }
}

impl EggworkExecutorConfig {
    /// Resolve the daemon `eggwork` table. Relative secret paths are
    /// rejected (fail closed): node trust material must be absolute so a
    /// daemon working-directory change can never redirect trust.
    pub fn from_daemon_config(cfg: &codegg_config::schema::EggworkConfig) -> Result<Self, String> {
        let mut nodes = HashMap::new();
        if let Some(ref table) = cfg.nodes {
            for (name, profile) in table {
                let node_id = profile.effective_node_id(name).to_string();
                if node_id.is_empty() {
                    return Err(format!("eggwork node '{name}': node id is empty"));
                }
                let endpoint = profile
                    .endpoint
                    .clone()
                    .ok_or_else(|| format!("eggwork node '{name}': endpoint is not configured"))?;
                let resolve = |label: &str, value: &Option<String>| -> Result<PathBuf, String> {
                    let raw = value.clone().ok_or_else(|| {
                        format!("eggwork node '{name}': {label} is not configured")
                    })?;
                    let path = PathBuf::from(&raw);
                    if !path.is_absolute() {
                        return Err(format!(
                            "eggwork node '{name}': {label} must be an absolute path"
                        ));
                    }
                    Ok(path)
                };
                nodes.insert(
                    name.clone(),
                    ResolvedEggworkNode {
                        node_id,
                        endpoint,
                        ca_cert: resolve("ca_cert_path", &profile.ca_cert_path)?,
                        client_cert: resolve("client_cert_path", &profile.client_cert_path)?,
                        client_key: resolve("client_key_path", &profile.client_key_path)?,
                        required_capabilities: profile
                            .required_capabilities
                            .clone()
                            .unwrap_or_default(),
                    },
                );
            }
        }
        Ok(Self {
            nodes,
            store: None,
            lease_renew_interval: LEASE_RENEW_INTERVAL,
        })
    }

    pub fn with_store(mut self, store: Arc<dyn JobStore>) -> Self {
        self.store = Some(store);
        self
    }
}

/// Boxed live event stream from one remote execution.
pub type BoxEventStream =
    futures_util::stream::BoxStream<'static, Result<ExecutionEvent, EggworkClientError>>;

/// Seam over the Eggwork node API. The production implementation wraps
/// [`NodeClient`]; tests substitute a scripted fake so no TLS fixture or
/// live node is required.
#[async_trait]
pub trait EggworkNodeClient: Send + Sync {
    async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError>;
    async fn status(&self) -> Result<NodeStatus, EggworkClientError>;
    async fn find_missing_blobs(
        &self,
        digests: &[BlobDigest],
    ) -> Result<Vec<BlobDigest>, EggworkClientError>;
    async fn upload_blob(
        &self,
        digest: &BlobDigest,
        declared_length: u64,
        stream: BoxBytesStream,
    ) -> Result<(), EggworkClientError>;
    async fn create_workspace(
        &self,
        workspace_id: &EggworkWorkspaceId,
        handle: &ExecutionHandle,
        manifest: &WorkspaceManifest,
    ) -> Result<eggwork_client::WorkspaceReady, EggworkClientError>;
    async fn execute_in_workspace(
        &self,
        spec: &ExecutionSpec,
        handle: &ExecutionHandle,
        workspace_id: &EggworkWorkspaceId,
    ) -> Result<BoxEventStream, EggworkClientError>;
    async fn observe_generation(
        &self,
        id: &ExecutionId,
        generation: u64,
    ) -> Result<ExecutionSnapshot, EggworkClientError>;
    async fn cancel(
        &self,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionSnapshot, EggworkClientError>;
    async fn renew(
        &self,
        handle: &ExecutionHandle,
        renewal_id: &str,
    ) -> Result<ExecutionSnapshot, EggworkClientError>;
    async fn artifacts(
        &self,
        execution_id: &ExecutionId,
        generation: u64,
    ) -> Result<Vec<ArtifactRecord>, EggworkClientError>;
    async fn download_artifact(
        &self,
        artifact: &ArtifactRecord,
    ) -> Result<BoxBytesStream, EggworkClientError>;
}

/// Builds the node client for a resolved profile.
pub trait EggworkClientFactory: Send + Sync {
    fn client_for(&self, node: &ResolvedEggworkNode) -> Result<Arc<dyn EggworkNodeClient>, String>;
}

/// Production factory: mutual-TLS [`NodeClient`] from profile file paths.
pub struct NodeClientFactory;

impl EggworkClientFactory for NodeClientFactory {
    fn client_for(&self, node: &ResolvedEggworkNode) -> Result<Arc<dyn EggworkNodeClient>, String> {
        let client = NodeClient::from_pem_files(
            &node.endpoint,
            &node.ca_cert,
            &node.client_cert,
            &node.client_key,
        )
        .map_err(|e| {
            format!(
                "eggwork node '{}': client construction failed for endpoint '{}': {}",
                node.node_id,
                node.endpoint,
                describe_client_error(&e)
            )
        })?;
        Ok(Arc::new(NodeClientAdapter { client }))
    }
}

struct NodeClientAdapter {
    client: NodeClient,
}

#[async_trait]
impl EggworkNodeClient for NodeClientAdapter {
    async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError> {
        self.client.capabilities().await
    }

    async fn status(&self) -> Result<NodeStatus, EggworkClientError> {
        self.client.status().await
    }

    async fn find_missing_blobs(
        &self,
        digests: &[BlobDigest],
    ) -> Result<Vec<BlobDigest>, EggworkClientError> {
        self.client.find_missing_blobs(digests).await
    }

    async fn upload_blob(
        &self,
        digest: &BlobDigest,
        declared_length: u64,
        stream: BoxBytesStream,
    ) -> Result<(), EggworkClientError> {
        self.client
            .upload_blob(digest, declared_length, stream)
            .await
    }

    async fn create_workspace(
        &self,
        workspace_id: &EggworkWorkspaceId,
        handle: &ExecutionHandle,
        manifest: &WorkspaceManifest,
    ) -> Result<eggwork_client::WorkspaceReady, EggworkClientError> {
        self.client
            .create_workspace(workspace_id, handle, manifest)
            .await
    }

    async fn execute_in_workspace(
        &self,
        spec: &ExecutionSpec,
        handle: &ExecutionHandle,
        workspace_id: &EggworkWorkspaceId,
    ) -> Result<BoxEventStream, EggworkClientError> {
        let stream = self
            .client
            .execute_in_workspace(spec, handle, workspace_id)
            .await?;
        Ok(stream.into_events().boxed())
    }

    async fn observe_generation(
        &self,
        id: &ExecutionId,
        generation: u64,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.client.observe_generation(id, generation).await
    }

    async fn cancel(
        &self,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.client.cancel(handle).await
    }

    async fn renew(
        &self,
        handle: &ExecutionHandle,
        renewal_id: &str,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        self.client.renew(handle, renewal_id).await
    }

    async fn artifacts(
        &self,
        execution_id: &ExecutionId,
        generation: u64,
    ) -> Result<Vec<ArtifactRecord>, EggworkClientError> {
        self.client.artifacts(execution_id, generation).await
    }

    async fn download_artifact(
        &self,
        artifact: &ArtifactRecord,
    ) -> Result<BoxBytesStream, EggworkClientError> {
        self.client.download_artifact(artifact).await
    }
}

/// Fixed-target Eggwork executor. Construct with resolved daemon config;
/// register on the scheduler so `ExecutorKind::Eggwork` dispatches here.
pub struct EggworkExecutor {
    config: EggworkExecutorConfig,
    factory: Arc<dyn EggworkClientFactory>,
}

impl EggworkExecutor {
    pub fn new(config: EggworkExecutorConfig) -> Self {
        Self {
            config,
            factory: Arc::new(NodeClientFactory),
        }
    }

    pub fn with_factory(
        config: EggworkExecutorConfig,
        factory: Arc<dyn EggworkClientFactory>,
    ) -> Self {
        Self { config, factory }
    }

    fn node_id(job: &JobRecord) -> Option<&str> {
        match &job.target {
            ExecutionTarget::EggworkNode { node_id } => Some(node_id.as_str()),
            ExecutionTarget::Local => None,
        }
    }

    /// Extract the canonical argv/cwd/timeout from an eligible payload.
    fn remote_command(job: &JobRecord) -> Option<(Vec<String>, Option<String>, Option<Duration>)> {
        match &job.payload {
            JobPayload::ManagedArgv { argv, cwd } => {
                if argv.is_empty() {
                    return None;
                }
                Some((argv.clone(), cwd.clone(), job.timeout))
            }
            JobPayload::Shell {
                argv: Some(argv), ..
            } => {
                if argv.is_empty() {
                    return None;
                }
                // Shell payloads carry no separate cwd; run at the workspace root.
                Some((argv.clone(), None, job.timeout))
            }
            _ => None,
        }
    }
}

#[async_trait]
impl JobExecutor for EggworkExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Eggwork
    }

    fn supports(&self, kind: JobKind) -> bool {
        matches!(
            kind,
            JobKind::Build
                | JobKind::Lint
                | JobKind::Format
                | JobKind::ManagedProcess
                | JobKind::Shell
        )
    }

    fn validate(&self, job: &JobRecord) -> Result<(), ExecutorValidationError> {
        let node_id = match &job.target {
            ExecutionTarget::EggworkNode { node_id } => node_id,
            ExecutionTarget::Local => {
                return Err(ExecutorValidationError::UnsupportedKind {
                    executor: "eggwork".to_string(),
                    kind: format!("{:?} with local target", job.kind),
                });
            }
        };
        if node_id.is_empty() {
            return Err(ExecutorValidationError::InvalidPayload(
                "eggwork target node id is empty".to_string(),
            ));
        }
        match (&job.kind, &job.payload) {
            (
                JobKind::Build | JobKind::Lint | JobKind::Format | JobKind::ManagedProcess | JobKind::Shell,
                JobPayload::ManagedArgv { argv, .. },
            ) if !argv.is_empty() => Ok(()),
            (
                JobKind::ManagedProcess | JobKind::Shell,
                JobPayload::Shell {
                    argv: Some(argv), ..
                },
            ) if !argv.is_empty() => Ok(()),
            (JobKind::Test, _) => Err(ExecutorValidationError::InvalidPayload(
                "remote Test execution is deferred in M001: TestRunner durable evidence cannot be preserved across the remote adapter yet".to_string(),
            )),
            _ => Err(ExecutorValidationError::UnsupportedKind {
                executor: "eggwork".to_string(),
                kind: format!("{:?}", job.kind),
            }),
        }
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let started = std::time::Instant::now();
        let outcome = self.execute_inner(&ctx).await;
        let metrics = ExecutorMetrics {
            elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            ..Default::default()
        };
        match outcome {
            ExecutionOutcome::Done(mut completion) => {
                completion.metrics = metrics;
                completion
            }
            ExecutionOutcome::Failed(summary) => ExecutorCompletion {
                status: ExecutorStatus::Failed,
                summary: truncate_chars(&summary, MAX_SUMMARY_CHARS),
                run_id: None,
                metrics,
            },
            ExecutionOutcome::Cancelled(summary) => ExecutorCompletion {
                status: ExecutorStatus::Cancelled,
                summary: truncate_chars(&summary, MAX_SUMMARY_CHARS),
                run_id: None,
                metrics,
            },
            ExecutionOutcome::Interrupted(summary) => ExecutorCompletion {
                status: ExecutorStatus::Interrupted,
                summary: truncate_chars(&summary, MAX_SUMMARY_CHARS),
                run_id: None,
                metrics,
            },
        }
    }
}

enum ExecutionOutcome {
    Done(ExecutorCompletion),
    Failed(String),
    Cancelled(String),
    Interrupted(String),
}

impl EggworkExecutor {
    async fn execute_inner(&self, ctx: &JobExecutionContext) -> ExecutionOutcome {
        if let Err(e) = ctx.validate_runtime() {
            return ExecutionOutcome::Failed(format!("eggwork: runtime validation failed: {e}"));
        }
        let job = &ctx.job;
        let node_id = match Self::node_id(job) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                return ExecutionOutcome::Failed(
                    "eggwork: job does not carry an EggworkNode target".to_string(),
                );
            }
        };
        let (argv, cwd, timeout) = match Self::remote_command(job) {
            Some(cmd) => cmd,
            None => {
                return ExecutionOutcome::Failed(format!(
                    "eggwork: job kind {:?} has no mappable canonical argv",
                    job.kind
                ));
            }
        };
        let node = match self.config.nodes.get(&node_id) {
            Some(node) => node.clone(),
            None => {
                return ExecutionOutcome::Failed(format!(
                    "eggwork: unknown node '{node_id}': no matching [eggwork.nodes] profile"
                ));
            }
        };
        if ctx.cancellation.is_cancelled() {
            return ExecutionOutcome::Cancelled(format!(
                "eggwork: cancelled before contacting node '{node_id}'"
            ));
        }
        let client = match self.factory.client_for(&node) {
            Ok(client) => client,
            Err(e) => return ExecutionOutcome::Failed(format!("eggwork: {e}")),
        };

        // Restart reconciliation: a persisted handle for this attempt means a
        // previous execute() already bound a remote identity. Observe it
        // instead of submitting a second execution.
        if let Some(handle) = self.persisted_handle(job, &ctx.attempt_id).await {
            if handle.node_id != node_id {
                return ExecutionOutcome::Failed(format!(
                    "eggwork: persisted handle targets node '{}' but job targets '{node_id}'",
                    handle.node_id
                ));
            }
            return self
                .reconcile_persisted(ctx, &client, &node, &handle, &argv)
                .await;
        }

        // Fresh path: derive the deterministic handle and persist it before
        // any upload/execute side effect.
        let (handle, durable) = match derive_handle(job, &ctx.attempt_id) {
            Ok(pair) => pair,
            Err(e) => return ExecutionOutcome::Failed(format!("eggwork: {e}")),
        };
        if let Err(e) = self.persist_handle(&ctx.attempt_id, Some(&durable)).await {
            return ExecutionOutcome::Failed(format!(
                "eggwork: cannot persist remote handle before side effects: {e}"
            ));
        }
        self.run_remote(ctx, &client, &node, &handle, &argv, cwd.as_deref(), timeout)
            .await
    }

    /// Load the persisted remote handle for this attempt, if any.
    async fn persisted_handle(
        &self,
        job: &JobRecord,
        attempt_id: &AttemptId,
    ) -> Option<RemoteExecutionHandle> {
        let store = self.config.store.as_ref()?;
        let attempts = store.list_attempts(&job.job_id).await.ok()?;
        attempts
            .into_iter()
            .find(|a| a.attempt_id == *attempt_id)
            .and_then(|a| a.remote_handle)
    }

    async fn persist_handle(
        &self,
        attempt_id: &AttemptId,
        handle: Option<&RemoteExecutionHandle>,
    ) -> Result<(), String> {
        let Some(ref store) = self.config.store else {
            return Ok(());
        };
        store
            .set_attempt_remote_handle(attempt_id, handle)
            .await
            .map_err(|e| e.to_string())
    }

    /// Reconcile a persisted handle: observe the accepted execution instead
    /// of resubmitting. Terminal snapshots map to completions; a still-live
    /// execution is cancelled under its own handle and reported interrupted
    /// so a requeue creates a fresh attempt (and therefore a fresh remote
    /// identity).
    async fn reconcile_persisted(
        &self,
        ctx: &JobExecutionContext,
        client: &Arc<dyn EggworkNodeClient>,
        node: &ResolvedEggworkNode,
        handle: &RemoteExecutionHandle,
        argv: &[String],
    ) -> ExecutionOutcome {
        let eggwork_handle = match to_eggwork_handle(handle) {
            Ok(h) => h,
            Err(e) => {
                return ExecutionOutcome::Failed(format!(
                    "eggwork: persisted remote handle failed validation: {e}"
                ));
            }
        };
        let snapshot = match op_timeout(client.observe_generation(
            &eggwork_handle.execution_id,
            eggwork_handle.generation.get(),
        ))
        .await
        {
            Ok(snapshot) => snapshot,
            Err(e) => {
                return ExecutionOutcome::Interrupted(format!(
                    "eggwork: cannot observe persisted execution '{}': {}; treating attempt as interrupted",
                    handle.execution_id, e
                ));
            }
        };
        if is_terminal(&snapshot.state) {
            let completion = self
                .finish_from_snapshot(ctx, client, node, &eggwork_handle, &snapshot, argv, true)
                .await;
            return ExecutionOutcome::Done(completion);
        }
        // Still live: cancel under the exact persisted handle, then report
        // interrupted. Never resubmit under a new identity for this attempt.
        let _ = op_timeout(client.cancel(&eggwork_handle)).await;
        ExecutionOutcome::Interrupted(format!(
            "eggwork: persisted execution '{}' was still live after restart; cancelled under its own handle",
            handle.execution_id
        ))
    }

    /// Full fresh-execution path: pre-flight, snapshot/upload, submit,
    /// stream, terminal mapping, artifact import.
    #[allow(clippy::too_many_arguments)]
    async fn run_remote(
        &self,
        ctx: &JobExecutionContext,
        client: &Arc<dyn EggworkNodeClient>,
        node: &ResolvedEggworkNode,
        handle: &ExecutionHandle,
        argv: &[String],
        cwd: Option<&str>,
        timeout: Option<Duration>,
    ) -> ExecutionOutcome {
        if let Err(e) = preflight(client, node).await {
            return ExecutionOutcome::Failed(e);
        }
        if ctx.cancellation.is_cancelled() {
            return self
                .cancel_before_submit(ctx, "cancelled after pre-flight, before upload")
                .await;
        }
        let remote_cwd = match map_remote_cwd(&ctx.workspace_root, cwd) {
            Ok(cwd) => cwd,
            Err(e) => return ExecutionOutcome::Failed(format!("eggwork: {e}")),
        };
        let snapshot = match build_snapshot(&ctx.workspace_root) {
            Ok(snapshot) => snapshot,
            Err(e) => return ExecutionOutcome::Failed(format!("eggwork: workspace snapshot: {e}")),
        };
        ctx.progress
            .progress(
                &ctx.job.job_id,
                &format!(
                    "[eggwork {}] snapshot files={} bytes={} skipped_non_regular={} skipped_oversize={}",
                    node.node_id,
                    snapshot.files.len(),
                    snapshot.total_bytes,
                    snapshot.skipped_non_regular,
                    snapshot.skipped_oversize
                ),
            )
            .await;
        let workspace_id = deterministic_workspace_id(&ctx.job.job_id, &ctx.attempt_id);
        if let Err(e) = upload_snapshot(client, ctx, &snapshot).await {
            if ctx.cancellation.is_cancelled() {
                return self.cancel_before_submit(ctx, &e).await;
            }
            return ExecutionOutcome::Failed(e);
        }
        if ctx.cancellation.is_cancelled() {
            return self
                .cancel_before_submit(ctx, "cancelled after upload, before submit")
                .await;
        }
        if let Err(e) =
            create_remote_workspace(client, &workspace_id, handle, &snapshot.manifest).await
        {
            return ExecutionOutcome::Failed(e);
        }
        let spec = match build_spec(argv, remote_cwd, timeout) {
            Ok(spec) => spec,
            Err(e) => return ExecutionOutcome::Failed(format!("eggwork: {e}")),
        };
        let mut events =
            match op_timeout(client.execute_in_workspace(&spec, handle, &workspace_id)).await {
                Ok(stream) => stream,
                Err(e) => {
                    return ExecutionOutcome::Failed(format!(
                        "eggwork: submit failed on node '{}': {}",
                        node.node_id, e
                    ));
                }
            };

        // Lease renewal for the execution lifetime.
        let renewal = spawn_renewal(
            client.clone(),
            handle.clone(),
            &ctx.attempt_id,
            self.config.lease_renew_interval,
        );
        let mut collector = StreamCollector::new(&ctx.job.job_id, &node.node_id, &ctx.progress);
        loop {
            tokio::select! {
                biased;
                _ = ctx.cancellation.cancelled() => {
                    renewal.abort();
                    let _ = op_timeout(client.cancel(handle)).await;
                    return ExecutionOutcome::Cancelled(format!(
                        "eggwork: cancelled execution '{}' on node '{}'",
                        short_id(&handle.execution_id),
                        node.node_id
                    ));
                }
                event = events.next() => {
                    match event {
                        Some(Ok(ev)) => collector.ingest(&ev).await,
                        Some(Err(e)) => {
                            renewal.abort();
                            return ExecutionOutcome::Interrupted(format!(
                                "eggwork: event stream failed on node '{}': {}",
                                node.node_id, e
                            ));
                        }
                        None => break,
                    }
                }
            }
        }
        renewal.abort();
        drop(events);

        // The stream ending is not itself terminal evidence: the
        // authoritative snapshot decides.
        let snapshot = match op_timeout(
            client.observe_generation(&handle.execution_id, handle.generation.get()),
        )
        .await
        {
            Ok(snapshot) => snapshot,
            Err(e) => {
                return ExecutionOutcome::Interrupted(format!(
                    "eggwork: lost execution '{}' on node '{}': {}",
                    short_id(&handle.execution_id),
                    node.node_id,
                    e
                ));
            }
        };
        let completion = self
            .finish_from_snapshot(ctx, client, node, handle, &snapshot, argv, false)
            .await;
        ExecutionOutcome::Done(completion)
    }

    /// Cancellation before any remote side effect: no execution was
    /// submitted, so there is nothing to cancel remotely.
    async fn cancel_before_submit(
        &self,
        ctx: &JobExecutionContext,
        detail: &str,
    ) -> ExecutionOutcome {
        let _ = self.persist_handle(&ctx.attempt_id, None).await;
        ExecutionOutcome::Cancelled(format!("eggwork: {detail}"))
    }

    /// Map an authoritative terminal snapshot to an `ExecutorCompletion`,
    /// importing artifacts into the RunStore.
    async fn finish_from_snapshot(
        &self,
        ctx: &JobExecutionContext,
        client: &Arc<dyn EggworkNodeClient>,
        node: &ResolvedEggworkNode,
        handle: &ExecutionHandle,
        snapshot: &ExecutionSnapshot,
        argv: &[String],
        reconciled: bool,
    ) -> ExecutorCompletion {
        let result = snapshot.result.clone();
        let (status, mut detail) = map_terminal(&snapshot.state, result.as_ref());
        let exec_id = short_id(&handle.execution_id);
        let mut summary = format!(
            "eggwork node={} execution={} state={:?}",
            node.node_id, exec_id, snapshot.state
        );
        if let Some(ref r) = result {
            if let Some(code) = r.exit_code {
                summary.push_str(&format!(" exit={code}"));
            }
            if r.stdout_omitted > 0 || r.stderr_omitted > 0 {
                summary.push_str(&format!(
                    " omitted_stdout={} omitted_stderr={}",
                    r.stdout_omitted, r.stderr_omitted
                ));
            }
        }
        if reconciled {
            summary.push_str(" reconciled=true");
        }
        if !detail.is_empty() {
            summary.push_str(&format!(" {detail}"));
            detail.clear();
        }
        let (run_id, artifact_note) = self
            .import_artifacts(ctx, client, node, handle, snapshot, argv, &status)
            .await;
        if !artifact_note.is_empty() {
            summary.push_str(&format!(" {artifact_note}"));
        }
        ExecutorCompletion {
            status,
            summary: truncate_chars(&summary, MAX_SUMMARY_CHARS),
            run_id,
            metrics: ExecutorMetrics::default(),
        }
    }

    /// Import declared artifacts plus bounded captured stdio into the
    /// RunStore. Best-effort: import failures are reported in the returned
    /// note, never in place of the terminal status.
    async fn import_artifacts(
        &self,
        ctx: &JobExecutionContext,
        client: &Arc<dyn EggworkNodeClient>,
        node: &ResolvedEggworkNode,
        handle: &ExecutionHandle,
        snapshot: &ExecutionSnapshot,
        argv: &[String],
        status: &ExecutorStatus,
    ) -> (Option<codegg_core::run_store::RunId>, String) {
        let Some(ref store) = ctx.run_store else {
            return (None, "runstore=absent".to_string());
        };
        let draft = RunDraft {
            kind: RunKind::ManagedProcess,
            invocation: RunInvocation {
                command: argv.join(" "),
                argv: Some(argv.to_vec()),
                script_hash: None,
            },
            session_id: ctx.job.session_id.clone(),
            parent_run_id: None,
            workspace_root: ctx.workspace_root.clone(),
            cwd: ctx.workspace_root.clone(),
            backend: BackendRecord {
                family: "eggwork".to_string(),
                detail: Some(format!(
                    "node={} execution={}",
                    node.node_id,
                    handle.execution_id.as_str()
                )),
            },
            risk: RiskRecord {
                level: "medium".to_string(),
                has_subprocess: true,
                has_git_mutation: false,
                has_destructive_mutation: false,
            },
            planned_backend: None,
            actual_backend: Some(ActualBackend::Eggwork),
            ownership: RunOwnership::DelegatedBackend,
            asset_provenance: None,
        };
        let run = match store.begin_run(draft).await {
            Ok(run) => run,
            Err(e) => {
                return (
                    None,
                    format!(
                        "runstore=begin_failed({})",
                        truncate_chars(&e.to_string(), 120)
                    ),
                );
            }
        };
        let mut imported = 0usize;
        let mut skipped = 0usize;
        // Bounded captured stdio from the event stream is unavailable here
        // by design (streaming sink only); declared artifacts carry output.
        let records: Vec<ArtifactRecord> =
            op_timeout(client.artifacts(&handle.execution_id, handle.generation.get()))
                .await
                .unwrap_or_default();
        for record in records.iter().take(MAX_ARTIFACTS) {
            if record.size_bytes > MAX_ARTIFACT_BYTES {
                skipped += 1;
                continue;
            }
            let bytes = match collect_bounded(
                match op_timeout(client.download_artifact(record)).await {
                    Ok(stream) => stream,
                    _ => {
                        skipped += 1;
                        continue;
                    }
                },
                MAX_ARTIFACT_BYTES,
            )
            .await
            {
                Ok(bytes) => bytes,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let (kind, mime) = artifact_kind_for_path(record.path.as_str());
            let input = ArtifactInput {
                kind,
                data: bytes,
                mime_type: mime.to_string(),
                safe_for_model: false,
            };
            match store.write_artifact(&run, input).await {
                Ok(_) => imported += 1,
                Err(_) => skipped += 1,
            }
        }
        if snapshot
            .result
            .as_ref()
            .map(|r| r.artifact_count)
            .unwrap_or(0)
            > imported as u32
        {
            skipped += 1;
        }
        let run_status = match status {
            ExecutorStatus::Completed => RunStatus::Complete,
            ExecutorStatus::Failed => RunStatus::Failed,
            ExecutorStatus::Cancelled => RunStatus::Cancelled,
            ExecutorStatus::TimedOut => RunStatus::TimedOut,
            ExecutorStatus::Interrupted => RunStatus::Incomplete,
        };
        let completion = RunCompletion {
            status: run_status,
            completed_at: chrono::Utc::now(),
            permissions: Vec::new(),
            sandbox: None,
            projection: None,
            changes: Vec::new(),
            rerun: None,
            actual_backend: Some(ActualBackend::Eggwork),
            fallback: None,
        };
        let run_id = match store.complete_run(run, completion).await {
            Ok(manifest) => Some(manifest.run_id),
            Err(_) => None,
        };
        (
            run_id,
            format!("artifacts_imported={imported} artifacts_skipped={skipped}"),
        )
    }
}

/// Derive the deterministic Eggwork identity for one CodeGG attempt:
/// stable across retransmission, distinct across attempts.
///
/// The lease token is generated exactly once per fresh attempt and shared
/// verbatim between the live [`ExecutionHandle`] and the durable
/// [`RemoteExecutionHandle`]. Eggwork stores `lease_hash(lease_id)` at
/// acceptance and fences cancel/renew against it, so minting a second
/// token for the durable record would leave restart reconciliation
/// unauthorized (`invalid_lease`). Retransmission of the same attempt must
/// NOT call this again: the persisted handle is the identity.
fn derive_handle(
    job: &JobRecord,
    attempt_id: &AttemptId,
) -> Result<(ExecutionHandle, RemoteExecutionHandle), String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codegg-eggwork-v1/");
    hasher.update(job.job_id.as_str().as_bytes());
    hasher.update(b"/");
    hasher.update(attempt_id.as_str().as_bytes());
    let digest = hex::encode(hasher.finalize());
    let execution_id = format!("codegg-{digest}");
    let eggwork_id = ExecutionId::new(&execution_id)
        .map_err(|e| format!("derived execution id invalid: {e:?}"))?;
    let generation =
        ExecutionGeneration::new(1).map_err(|e| format!("generation 1 invalid: {e:?}"))?;
    let lease_id = LeaseId::new(format!("codegg-lease-{}", uuid::Uuid::new_v4().simple()))
        .map_err(|e| format!("derived lease id invalid: {e:?}"))?;
    let node_id = match &job.target {
        ExecutionTarget::EggworkNode { node_id } => node_id.clone(),
        ExecutionTarget::Local => return Err("job has no EggworkNode target".to_string()),
    };
    // Single-source lease identity: the durable record copies the exact
    // execution id, generation, and lease id from the live handle built
    // above. No second random token is minted here.
    let live = ExecutionHandle {
        execution_id: eggwork_id,
        generation,
        lease_id,
    };
    let durable = RemoteExecutionHandle::from_parts(
        node_id,
        live.execution_id.as_str().to_string(),
        live.generation.get(),
        live.lease_id.as_str().to_string(),
    );
    Ok((live, durable))
}

/// Rebuild the fenced Eggwork handle from durable provenance.
///
/// The reconstruction is byte-exact: execution id, generation, and lease
/// id are copied verbatim so the rebuilt handle authorizes cancel/renew
/// against the accepted execution. Any divergence is a typed validation
/// failure, never a silent reinterpretation.
pub fn to_eggwork_handle(handle: &RemoteExecutionHandle) -> Result<ExecutionHandle, String> {
    Ok(ExecutionHandle {
        execution_id: ExecutionId::new(&handle.execution_id)
            .map_err(|e| format!("execution id invalid: {e:?}"))?,
        generation: ExecutionGeneration::new(handle.generation)
            .map_err(|e| format!("generation invalid: {e:?}"))?,
        lease_id: LeaseId::new(&handle.lease_id).map_err(|e| format!("lease id invalid: {e:?}"))?,
    })
}

/// Pre-flight: node reachable, protocol compatible (enforced by the
/// client), required capabilities present, node not draining or saturated.
async fn preflight(
    client: &Arc<dyn EggworkNodeClient>,
    node: &ResolvedEggworkNode,
) -> Result<(), String> {
    let capabilities = match op_timeout(client.capabilities()).await {
        Ok(caps) => caps,
        Err(e) => {
            return Err(format!(
                "eggwork: node '{}' capability probe failed: {}",
                node.node_id, e
            ));
        }
    };
    if !capabilities
        .features
        .iter()
        .any(|f| f == REQUIRED_EXEC_FEATURE)
    {
        return Err(format!(
            "eggwork: node '{}' lacks required capability '{REQUIRED_EXEC_FEATURE}'",
            node.node_id
        ));
    }
    for required in &node.required_capabilities {
        if !capabilities.features.iter().any(|f| f == required) {
            return Err(format!(
                "eggwork: node '{}' lacks configured capability '{required}'",
                node.node_id
            ));
        }
    }
    let status = match op_timeout(client.status()).await {
        Ok(status) => status,
        Err(e) => {
            return Err(format!(
                "eggwork: node '{}' status probe failed: {}",
                node.node_id, e
            ));
        }
    };
    if status.draining {
        return Err(format!("eggwork: node '{}' is draining", node.node_id));
    }
    if status.active_executions >= status.capabilities.max_active_executions {
        return Err(format!(
            "eggwork: node '{}' is busy ({}/{})",
            node.node_id, status.active_executions, status.capabilities.max_active_executions
        ));
    }
    Ok(())
}

/// Map a payload cwd to an Eggwork-relative cwd rooted at the workspace
/// lease root. Absolute paths must stay inside the lease root; anything
/// else is a typed failure, never silent reinterpretation.
fn map_remote_cwd(
    workspace_root: &Path,
    cwd: Option<&str>,
) -> Result<Option<RelativePath>, String> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    let path = Path::new(cwd);
    if path.is_absolute() {
        let rel = path
            .strip_prefix(workspace_root)
            .map_err(|_| "payload cwd escapes the workspace lease root".to_string())?;
        if rel.as_os_str().is_empty() {
            return Ok(None);
        }
        let rel_str = rel
            .to_str()
            .ok_or_else(|| "payload cwd is not UTF-8".to_string())?;
        return RelativePath::new(rel_str)
            .map(Some)
            .map_err(|e| format!("payload cwd invalid for remote execution: {e:?}"));
    }
    RelativePath::new(cwd)
        .map(Some)
        .map_err(|e| format!("payload cwd invalid for remote execution: {e:?}"))
}

struct SnapshotFile {
    bytes: Vec<u8>,
    executable: bool,
}

struct WorkspaceSnapshot {
    manifest: WorkspaceManifest,
    files: HashMap<String, SnapshotFile>,
    skipped_non_regular: usize,
    skipped_oversize: usize,
    total_bytes: u64,
}

/// Build a bounded workspace manifest from the scheduler-owned lease
/// root. Symlinks and non-regular files are skipped (never followed);
/// traversal outside the root is a hard failure.
fn build_snapshot(root: &Path) -> Result<WorkspaceSnapshot, String> {
    let mut files: HashMap<String, SnapshotFile> = HashMap::new();
    let mut dirs: HashSet<String> = HashSet::new();
    let mut total_bytes: u64 = 0;
    let mut skipped_non_regular = 0usize;
    let mut skipped_oversize = 0usize;

    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(MAX_WORKSPACE_DEPTH)
        .into_iter();
    for entry in walker {
        let entry = entry.map_err(|e| format!("workspace walk failed: {e}"))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|_| "workspace entry escapes the lease root".to_string())?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let rel_str = rel
            .to_str()
            .ok_or_else(|| "workspace entry path is not UTF-8".to_string())?;
        // Reject `..` even though strip_prefix succeeded (defense in depth;
        // walkdir never yields these, but the manifest must not).
        if rel_str.split('/').any(|c| c == "..") {
            return Err("workspace entry escapes the lease root".to_string());
        }
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            skipped_non_regular += 1;
            continue;
        }
        if file_type.is_dir() {
            RelativePath::new(rel_str)
                .map_err(|e| format!("workspace directory invalid: {e:?}"))?;
            dirs.insert(rel_str.to_string());
            continue;
        }
        if !file_type.is_file() {
            skipped_non_regular += 1;
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|e| format!("workspace stat failed: {e}"))?;
        let size = metadata.len();
        if size > MAX_WORKSPACE_FILE_BYTES {
            skipped_oversize += 1;
            continue;
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_WORKSPACE_TOTAL_BYTES {
            return Err(format!(
                "workspace snapshot exceeds {MAX_WORKSPACE_TOTAL_BYTES} byte bound"
            ));
        }
        #[cfg(unix)]
        let executable =
            std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 != 0;
        #[cfg(not(unix))]
        let executable = false;
        let bytes = std::fs::read(path).map_err(|e| format!("workspace read failed: {e}"))?;
        if bytes.len() as u64 != size {
            return Err("workspace file changed during snapshot".to_string());
        }
        files.insert(rel_str.to_string(), SnapshotFile { bytes, executable });
    }

    // Parent directories must exist in the manifest.
    let rels: Vec<String> = files.keys().chain(dirs.iter()).cloned().collect();
    for rel in &rels {
        let mut current = Path::new(rel.as_str());
        while let Some(parent) = current.parent() {
            let parent_str = parent.to_str().unwrap_or("");
            if parent_str.is_empty() {
                break;
            }
            dirs.insert(parent_str.to_string());
            current = parent;
        }
    }

    let mut manifest_entries = Vec::new();
    let mut sorted_dirs: Vec<&String> = dirs.iter().collect();
    sorted_dirs.sort();
    for dir in sorted_dirs {
        let path =
            RelativePath::new(dir).map_err(|e| format!("workspace directory invalid: {e:?}"))?;
        manifest_entries.push(WorkspaceEntry::Directory { path });
    }
    let mut sorted_files: Vec<&String> = files.keys().collect();
    sorted_files.sort();
    for rel in sorted_files {
        let file = &files[rel.as_str()];
        let path =
            RelativePath::new(rel).map_err(|e| format!("workspace file path invalid: {e:?}"))?;
        manifest_entries.push(WorkspaceEntry::File {
            path,
            digest: BlobDigest::from_bytes(&file.bytes),
            size_bytes: file.bytes.len() as u64,
            executable: file.executable,
        });
    }
    if manifest_entries.len() > MAX_WORKSPACE_ENTRIES {
        return Err(format!(
            "workspace snapshot exceeds {MAX_WORKSPACE_ENTRIES} entry bound"
        ));
    }
    let manifest = WorkspaceManifest {
        schema_version: 1,
        entries: manifest_entries,
    };
    manifest
        .validate()
        .map_err(|e| format!("workspace manifest invalid: {e:?}"))?;
    Ok(WorkspaceSnapshot {
        manifest,
        files,
        skipped_non_regular,
        skipped_oversize,
        total_bytes,
    })
}

/// Upload missing blobs, then create the deterministic remote workspace.
async fn upload_snapshot(
    client: &Arc<dyn EggworkNodeClient>,
    ctx: &JobExecutionContext,
    snapshot: &WorkspaceSnapshot,
) -> Result<(), String> {
    let mut digests: Vec<BlobDigest> = snapshot
        .files
        .values()
        .map(|f| BlobDigest::from_bytes(&f.bytes))
        .collect();
    digests.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    digests.dedup_by(|a, b| a.as_str() == b.as_str());
    let missing = match op_timeout(client.find_missing_blobs(&digests)).await {
        Ok(missing) => missing,
        Err(e) => return Err(format!("eggwork: blob probe failed: {e}")),
    };
    let by_digest: HashMap<String, &SnapshotFile> = snapshot
        .files
        .values()
        .map(|f| (BlobDigest::from_bytes(&f.bytes).as_str().to_string(), f))
        .collect();
    for digest in &missing {
        if ctx.cancellation.is_cancelled() {
            return Err("cancelled during blob upload".to_string());
        }
        let Some(file) = by_digest.get(digest.as_str()).copied() else {
            return Err("workspace digest changed during upload".to_string());
        };
        let stream: BoxBytesStream = futures_util::stream::iter(vec![Ok::<Bytes, EggfetchError>(
            Bytes::from(file.bytes.clone()),
        )])
        .boxed();
        match op_timeout(client.upload_blob(digest, file.bytes.len() as u64, stream)).await {
            Ok(()) => {}
            Err(e) => {
                return Err(format!("eggwork: blob upload failed: {e}"));
            }
        }
    }
    Ok(())
}

async fn create_remote_workspace(
    client: &Arc<dyn EggworkNodeClient>,
    workspace_id: &EggworkWorkspaceId,
    handle: &ExecutionHandle,
    manifest: &WorkspaceManifest,
) -> Result<(), String> {
    match op_timeout(client.create_workspace(workspace_id, handle, manifest)).await {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("eggwork: workspace creation failed: {e}")),
    }
}

/// Deterministic remote workspace id for one attempt.
fn deterministic_workspace_id(
    job_id: &codegg_core::jobs::JobId,
    attempt_id: &AttemptId,
) -> EggworkWorkspaceId {
    let mut hasher = Sha256::new();
    hasher.update(b"codegg-eggwork-ws-v1/");
    hasher.update(job_id.as_str().as_bytes());
    hasher.update(b"/");
    hasher.update(attempt_id.as_str().as_bytes());
    let digest = hex::encode(hasher.finalize());
    EggworkWorkspaceId::new(format!("codegg-ws-{}", &digest[..32]))
        .expect("workspace id charset is fixed")
}

/// Build the remote command: argv boundaries preserved exactly, explicit
/// (empty) environment allowlist, mapped timeout, no shell reconstruction.
fn build_spec(
    argv: &[String],
    cwd: Option<RelativePath>,
    timeout: Option<Duration>,
) -> Result<ExecutionSpec, String> {
    let timeout_millis = timeout
        .map(|t| t.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(DEFAULT_REMOTE_TIMEOUT_MILLIS)
        .clamp(1, MAX_REMOTE_TIMEOUT_MILLIS);
    let command = CommandSpec {
        argv: argv.to_vec(),
        cwd,
        environment: Vec::new(),
        stdin: StdinPolicy::Null,
        timeout_millis,
        output: OutputPolicy::default(),
        declared_outputs: Vec::new(),
        resources: ResourceRequirements {
            memory_bytes: Requirement::NotRequested,
            cpu_millis: Requirement::NotRequested,
            pids: Requirement::NotRequested,
        },
        isolation: IsolationRequirement::BestEffort,
        network: NetworkRequirement::Disabled,
    };
    let spec = ExecutionSpec {
        schema_version: 1,
        command,
        metadata: Vec::new(),
    };
    spec.validate()
        .map_err(|e| format!("remote command invalid: {e:?}"))?;
    Ok(spec)
}

/// Bounded collector for live progress plus truncated stdio capture.
struct StreamCollector<'a> {
    job_id: codegg_core::jobs::JobId,
    node_id: String,
    progress: &'a Arc<dyn crate::scheduler::executor::JobProgressSink>,
    messages: usize,
    omitted: usize,
}

impl<'a> StreamCollector<'a> {
    fn new(
        job_id: &codegg_core::jobs::JobId,
        node_id: &str,
        progress: &'a Arc<dyn crate::scheduler::executor::JobProgressSink>,
    ) -> Self {
        Self {
            job_id: job_id.clone(),
            node_id: node_id.to_string(),
            progress,
            messages: 0,
            omitted: 0,
        }
    }

    async fn ingest(&mut self, event: &ExecutionEvent) {
        let text = match &event.kind {
            ExecutionEventKind::State(state) => format!("state={state:?}"),
            ExecutionEventKind::Stdout(bytes) => {
                format!(
                    "stdout: {}",
                    truncate_chars(&String::from_utf8_lossy(bytes), 512)
                )
            }
            ExecutionEventKind::Stderr(bytes) => {
                format!(
                    "stderr: {}",
                    truncate_chars(&String::from_utf8_lossy(bytes), 512)
                )
            }
            ExecutionEventKind::Diagnostic(message) => {
                format!("diagnostic: {}", truncate_chars(message, 512))
            }
        };
        if self.messages >= MAX_PROGRESS_MESSAGES {
            self.omitted += 1;
            return;
        }
        self.messages += 1;
        let message = truncate_chars(
            &format!("[eggwork {}] {text}", self.node_id),
            MAX_PROGRESS_MESSAGE_CHARS,
        );
        self.progress.progress(&self.job_id, &message).await;
    }
}

/// Spawn lease renewal for the execution lifetime. The renewal id is
/// stable per attempt, so renewal replay is idempotent server-side.
fn spawn_renewal(
    client: Arc<dyn EggworkNodeClient>,
    handle: ExecutionHandle,
    attempt_id: &AttemptId,
    interval_duration: Duration,
) -> tokio::task::JoinHandle<()> {
    let mut hasher = Sha256::new();
    hasher.update(b"codegg-eggwork-renew-v1/");
    hasher.update(attempt_id.as_str().as_bytes());
    let renewal_id = format!("rn-{}", &hex::encode(hasher.finalize())[..32]);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(interval_duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match op_timeout(client.renew(&handle, &renewal_id)).await {
                Ok(snapshot) if is_terminal(&snapshot.state) => break,
                Ok(_) => {}
                // Renewal failure is advisory: the authoritative snapshot
                // at the end decides. Stop renewing on transport loss to
                // avoid log spam; execution continues under its lease.
                _ => break,
            }
        }
    })
}

fn is_terminal(state: &ExecutionState) -> bool {
    matches!(
        state,
        ExecutionState::Succeeded
            | ExecutionState::Failed
            | ExecutionState::Cancelled
            | ExecutionState::TimedOut
            | ExecutionState::Interrupted
    )
}

/// Map an authoritative terminal snapshot to a scheduler status.
/// Non-terminal snapshots are never success: they map to interrupted.
fn map_terminal(
    state: &ExecutionState,
    result: Option<&ExecutionResult>,
) -> (ExecutorStatus, String) {
    match state {
        ExecutionState::Succeeded => (ExecutorStatus::Completed, String::new()),
        ExecutionState::Failed => {
            let code = result
                .and_then(|r| r.exit_code)
                .map(|c| format!(" exit={c}"))
                .unwrap_or_default();
            (
                ExecutorStatus::Failed,
                format!("remote command failed{code}"),
            )
        }
        ExecutionState::Cancelled | ExecutionState::Cancelling => (
            ExecutorStatus::Cancelled,
            "remote execution cancelled".to_string(),
        ),
        ExecutionState::TimedOut => (
            ExecutorStatus::TimedOut,
            "remote execution timed out".to_string(),
        ),
        ExecutionState::Interrupted
        | ExecutionState::Accepted
        | ExecutionState::Preparing
        | ExecutionState::Running => (
            ExecutorStatus::Interrupted,
            format!("remote execution ended non-terminally ({state:?})"),
        ),
    }
}

fn artifact_kind_for_path(path: &str) -> (ArtifactKind, &'static str) {
    if path.ends_with(".json") {
        (ArtifactKind::StructuredJson, "application/json")
    } else if path.ends_with(".diff") || path.ends_with(".patch") {
        (ArtifactKind::UnifiedDiff, "text/x-patch")
    } else if path.ends_with(".log") || path.ends_with(".txt") {
        (ArtifactKind::CombinedLog, "text/plain")
    } else {
        (ArtifactKind::CombinedLog, "application/octet-stream")
    }
}

async fn collect_bounded(mut stream: BoxBytesStream, cap: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("artifact download failed: {e}"))?;
        if out.len() as u64 + chunk.len() as u64 > cap {
            return Err("artifact exceeds import bound".to_string());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn op_timeout<F, T>(future: F) -> Result<T, EggworkClientError>
where
    F: std::future::Future<Output = Result<T, EggworkClientError>>,
{
    match tokio::time::timeout(OP_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(EggworkClientError::Api {
            status: 0,
            code: "codegg_operation_timeout".to_string(),
            message: "eggwork operation timed out".to_string(),
        }),
    }
}

/// Render a client error without secret material. `ClientError::Debug`
/// already redacts transport bodies; this keeps endpoint-free messages
/// for summaries while preserving the API code.
fn describe_client_error(e: &EggworkClientError) -> String {
    match e {
        EggworkClientError::InvalidEndpoint => "invalid endpoint".to_string(),
        EggworkClientError::InvalidRoute => "invalid route".to_string(),
        EggworkClientError::Transport(_) => "transport failed".to_string(),
        EggworkClientError::Api { status, code, .. } => {
            format!("HTTP {status}: {code}")
        }
        EggworkClientError::InvalidResponse => "invalid response".to_string(),
        EggworkClientError::ProtocolIncompatible => "protocol incompatible".to_string(),
        EggworkClientError::UnsupportedCapability => "unsupported capability".to_string(),
    }
}

fn short_id(id: &ExecutionId) -> String {
    let s = id.as_str();
    if s.len() > 16 {
        s[..16].to_string()
    } else {
        s.to_string()
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobPriority, JobSource,
        ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::WorkspaceId;
    use std::sync::Mutex;

    struct FakeNodeClient {
        events: Vec<ExecutionEvent>,
        final_snapshot: ExecutionSnapshot,
        capabilities: NodeCapabilities,
        status: NodeStatus,
        uploaded: Mutex<Vec<(String, u64)>>,
        cancelled: Mutex<Vec<String>>,
        renewed: Mutex<Vec<String>>,
        artifacts: Vec<ArtifactRecord>,
    }

    impl FakeNodeClient {
        fn succeeding() -> Self {
            let execution_id = ExecutionId::new("exec-1").unwrap();
            Self {
                events: vec![ExecutionEvent {
                    sequence: eggwork_core::EventSequence::new(1),
                    kind: ExecutionEventKind::Stdout(b"hello\n".to_vec()),
                    metadata: eggwork_core::EventMetadata { fields: Vec::new() },
                }],
                final_snapshot: ExecutionSnapshot {
                    schema_version: 1,
                    execution_id: execution_id.clone(),
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
                },
                capabilities: NodeCapabilities {
                    protocol: eggwork_core::ProtocolVersionRange {
                        min: eggwork_core::ProtocolVersion { major: 1, minor: 0 },
                        max: eggwork_core::ProtocolVersion { major: 1, minor: 5 },
                    },
                    features: vec![REQUIRED_EXEC_FEATURE.to_string()],
                    max_active_executions: 4,
                },
                status: NodeStatus {
                    node_id: eggwork_core::NodeId::new("node-1").unwrap(),
                    draining: false,
                    active_executions: 0,
                    capabilities: NodeCapabilities {
                        protocol: eggwork_core::ProtocolVersionRange {
                            min: eggwork_core::ProtocolVersion { major: 1, minor: 0 },
                            max: eggwork_core::ProtocolVersion { major: 1, minor: 5 },
                        },
                        features: vec![REQUIRED_EXEC_FEATURE.to_string()],
                        max_active_executions: 4,
                    },
                },
                uploaded: Mutex::new(Vec::new()),
                cancelled: Mutex::new(Vec::new()),
                renewed: Mutex::new(Vec::new()),
                artifacts: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl EggworkNodeClient for FakeNodeClient {
        async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError> {
            Ok(self.capabilities.clone())
        }

        async fn status(&self) -> Result<NodeStatus, EggworkClientError> {
            Ok(self.status.clone())
        }

        async fn find_missing_blobs(
            &self,
            digests: &[BlobDigest],
        ) -> Result<Vec<BlobDigest>, EggworkClientError> {
            Ok(digests.to_vec())
        }

        async fn upload_blob(
            &self,
            digest: &BlobDigest,
            declared_length: u64,
            _stream: BoxBytesStream,
        ) -> Result<(), EggworkClientError> {
            self.uploaded
                .lock()
                .unwrap()
                .push((digest.as_str().to_string(), declared_length));
            Ok(())
        }

        async fn create_workspace(
            &self,
            workspace_id: &EggworkWorkspaceId,
            handle: &ExecutionHandle,
            manifest: &WorkspaceManifest,
        ) -> Result<eggwork_client::WorkspaceReady, EggworkClientError> {
            Ok(eggwork_client::WorkspaceReady {
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
            _spec: &ExecutionSpec,
            _handle: &ExecutionHandle,
            _workspace_id: &EggworkWorkspaceId,
        ) -> Result<BoxEventStream, EggworkClientError> {
            let events = self.events.clone();
            let stream = futures_util::stream::iter(events.into_iter().map(Ok));
            Ok(stream.boxed())
        }

        async fn observe_generation(
            &self,
            _id: &ExecutionId,
            _generation: u64,
        ) -> Result<ExecutionSnapshot, EggworkClientError> {
            Ok(self.final_snapshot.clone())
        }

        async fn cancel(
            &self,
            handle: &ExecutionHandle,
        ) -> Result<ExecutionSnapshot, EggworkClientError> {
            self.cancelled
                .lock()
                .unwrap()
                .push(handle.execution_id.as_str().to_string());
            Ok(self.final_snapshot.clone())
        }

        async fn renew(
            &self,
            _handle: &ExecutionHandle,
            renewal_id: &str,
        ) -> Result<ExecutionSnapshot, EggworkClientError> {
            self.renewed.lock().unwrap().push(renewal_id.to_string());
            Ok(self.final_snapshot.clone())
        }

        async fn artifacts(
            &self,
            _execution_id: &ExecutionId,
            _generation: u64,
        ) -> Result<Vec<ArtifactRecord>, EggworkClientError> {
            Ok(self.artifacts.clone())
        }

        async fn download_artifact(
            &self,
            _artifact: &ArtifactRecord,
        ) -> Result<BoxBytesStream, EggworkClientError> {
            let stream: BoxBytesStream =
                futures_util::stream::iter(vec![Ok::<Bytes, EggfetchError>(Bytes::from_static(
                    b"data",
                ))])
                .boxed();
            Ok(stream)
        }
    }

    struct FakeFactory {
        client: Arc<FakeNodeClient>,
    }

    impl EggworkClientFactory for FakeFactory {
        fn client_for(
            &self,
            _node: &ResolvedEggworkNode,
        ) -> Result<Arc<dyn EggworkNodeClient>, String> {
            Ok(self.client.clone())
        }
    }

    fn managed_argv_job(target: ExecutionTarget) -> JobRecord {
        let now = chrono::Utc::now();
        JobRecord {
            job_id: codegg_core::jobs::JobId::new_unchecked("j-egg-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["echo".to_string(), "hello".to_string()],
                cwd: None,
            },
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
            target,
        }
    }

    fn test_context(job: JobRecord, root: PathBuf) -> JobExecutionContext {
        let controller = Arc::new(crate::scheduler::admission::AdmissionController::new(
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
        ));
        let mut resources =
            crate::scheduler::permit::ResourcePermitGuard::new_orphan(Default::default());
        resources.install_controller(controller);
        JobExecutionContext {
            job,
            attempt_id: AttemptId::new_unchecked("att-1"),
            daemon_generation: DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            workspace_root: root,
            run_store: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
            progress: Arc::new(crate::scheduler::executor::NoopProgressSink),
            resources,
        }
    }

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

    #[test]
    fn validate_accepts_eligible_remote_job() {
        let exec = EggworkExecutor::new(EggworkExecutorConfig::default());
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        assert!(exec.validate(&job).is_ok());
    }

    /// C001 regression: the live handle and the durable record must carry
    /// one identical lease token. The pre-corrective implementation minted
    /// two independent random tokens, so this equality fails against it.
    #[test]
    fn lease_identity_is_single_source() {
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        let attempt = AttemptId::new_unchecked("att-lease-1");
        let (live, durable) = derive_handle(&job, &attempt).expect("derive handle");
        assert_eq!(live.execution_id.as_str(), durable.execution_id);
        assert_eq!(live.generation.get(), durable.generation);
        assert_eq!(live.lease_id.as_str(), durable.lease_id);
        assert_eq!(durable.node_id, "node-1");
    }

    /// Reconstruction from the durable record must rebuild the exact
    /// fenced tuple, and repeated reconstruction must be stable.
    #[test]
    fn reconstruction_is_exact_and_stable() {
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        let attempt = AttemptId::new_unchecked("att-lease-2");
        let (live, durable) = derive_handle(&job, &attempt).expect("derive handle");
        let first = to_eggwork_handle(&durable).expect("reconstruct");
        let second = to_eggwork_handle(&durable).expect("reconstruct again");
        for rebuilt in [&first, &second] {
            assert_eq!(rebuilt.execution_id.as_str(), live.execution_id.as_str());
            assert_eq!(rebuilt.generation.get(), live.generation.get());
            assert_eq!(rebuilt.lease_id.as_str(), live.lease_id.as_str());
        }
    }

    /// The single-source constructor copies a fixed canonical tuple
    /// verbatim. Fully deterministic: no randomness is involved, so this
    /// pins the invariant without any statistical UUID comparison.
    #[test]
    fn from_parts_copies_canonical_tuple_verbatim() {
        let durable = RemoteExecutionHandle::from_parts(
            "node-9".to_string(),
            "codegg-fixed-execution".to_string(),
            1,
            "codegg-lease-fixed".to_string(),
        );
        assert_eq!(
            durable.schema_version,
            RemoteExecutionHandle::SCHEMA_VERSION
        );
        assert_eq!(durable.node_id, "node-9");
        assert_eq!(durable.execution_id, "codegg-fixed-execution");
        assert_eq!(durable.generation, 1);
        assert_eq!(durable.lease_id, "codegg-lease-fixed");
        let rebuilt = to_eggwork_handle(&durable).expect("reconstruct fixed tuple");
        assert_eq!(rebuilt.execution_id.as_str(), "codegg-fixed-execution");
        assert_eq!(rebuilt.generation.get(), 1);
        assert_eq!(rebuilt.lease_id.as_str(), "codegg-lease-fixed");
    }

    /// Execution identity derives deterministically from job + attempt:
    /// the same attempt re-derives the same execution id (so retransmission
    /// must reuse the persisted lease rather than minting), while a new
    /// attempt derives a distinct execution id. Both properties are pure
    /// SHA-256 digests — no randomness is compared.
    #[test]
    fn execution_identity_deterministic_per_attempt() {
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        let attempt = AttemptId::new_unchecked("att-det-1");
        let (first_live, _) = derive_handle(&job, &attempt).expect("derive first");
        let (second_live, _) = derive_handle(&job, &attempt).expect("derive second");
        assert_eq!(
            first_live.execution_id.as_str(),
            second_live.execution_id.as_str()
        );
        let other_attempt = AttemptId::new_unchecked("att-det-2");
        let (other_live, _) = derive_handle(&job, &other_attempt).expect("derive other");
        assert_ne!(
            first_live.execution_id.as_str(),
            other_live.execution_id.as_str()
        );
    }

    #[test]
    fn validate_rejects_local_target() {
        let exec = EggworkExecutor::new(EggworkExecutorConfig::default());
        let job = managed_argv_job(ExecutionTarget::Local);
        assert!(matches!(
            exec.validate(&job),
            Err(ExecutorValidationError::UnsupportedKind { .. })
        ));
    }

    #[test]
    fn validate_defers_test_with_typed_error() {
        let exec = EggworkExecutor::new(EggworkExecutorConfig::default());
        let mut job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        job.kind = JobKind::Test;
        job.payload = JobPayload::Test {
            command: "cargo test".to_string(),
            argv: vec!["cargo".to_string(), "test".to_string()],
            cwd: None,
            scope: None,
            parent_run_id: None,
        };
        assert!(matches!(
            exec.validate(&job),
            Err(ExecutorValidationError::InvalidPayload(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_succeeds_and_persists_handle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), b"fn main() {}\n").unwrap();
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        let record = store
            .create_job(codegg_core::jobs::NewJob {
                workspace_id: job.workspace_id.clone(),
                session_id: None,
                turn_id: None,
                kind: job.kind,
                source: job.source,
                priority: job.priority,
                payload: job.payload.clone(),
                resource_request: job.resource_request.clone(),
                timeout: None,
                retry_policy: job.retry_policy.clone(),
                idempotency: job.idempotency,
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
                target: ExecutionTarget::EggworkNode {
                    node_id: "node-1".to_string(),
                },
            })
            .await
            .unwrap();
        let attempt = store
            .begin_attempt(&record.job_id, &DaemonGeneration::new_unchecked("gen-1"))
            .await
            .unwrap();
        let mut exec_job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        exec_job.job_id = record.job_id.clone();
        let controller = Arc::new(crate::scheduler::admission::AdmissionController::new(
            crate::scheduler::config::ResolvedSchedulerConfig::default(),
        ));
        let mut resources =
            crate::scheduler::permit::ResourcePermitGuard::new_orphan(Default::default());
        resources.install_controller(controller);
        let ctx = JobExecutionContext {
            job: exec_job,
            attempt_id: attempt.attempt_id.clone(),
            daemon_generation: DaemonGeneration::new_unchecked("gen-1"),
            workspace_id: WorkspaceId::new_unchecked("ws-egg"),
            workspace_root: dir.path().to_path_buf(),
            run_store: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
            progress: Arc::new(crate::scheduler::executor::NoopProgressSink),
            resources,
        };
        let fake = Arc::new(FakeNodeClient::succeeding());
        let mut nodes = HashMap::new();
        nodes.insert("node-1".to_string(), test_node());
        let exec = EggworkExecutor::with_factory(
            EggworkExecutorConfig {
                nodes,
                store: Some(store.clone()),
                ..Default::default()
            },
            Arc::new(FakeFactory {
                client: fake.clone(),
            }),
        );
        let completion = exec.execute(ctx).await;
        assert_eq!(completion.status, ExecutorStatus::Completed);
        assert!(completion.summary.contains("node-1"));
        // Exactly one blob uploaded (main.rs); handle persisted for the attempt.
        assert_eq!(fake.uploaded.lock().unwrap().len(), 1);
        let attempts = store.list_attempts(&record.job_id).await.unwrap();
        let persisted = attempts
            .iter()
            .find(|a| a.attempt_id == attempt.attempt_id)
            .and_then(|a| a.remote_handle.clone())
            .expect("remote handle persisted");
        assert_eq!(persisted.node_id, "node-1");
        assert!(!persisted.execution_id.is_empty());
        assert!(!persisted.lease_id.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_node_fails_without_remote_contact() {
        let dir = tempfile::tempdir().unwrap();
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "ghost".to_string(),
        });
        let ctx = test_context(job, dir.path().to_path_buf());
        let fake = Arc::new(FakeNodeClient::succeeding());
        let exec = EggworkExecutor::with_factory(
            EggworkExecutorConfig::default(),
            Arc::new(FakeFactory {
                client: fake.clone(),
            }),
        );
        let completion = exec.execute(ctx).await;
        assert_eq!(completion.status, ExecutorStatus::Failed);
        assert!(completion.summary.contains("unknown node"));
        assert!(fake.uploaded.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn precancelled_job_produces_no_remote_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let job = managed_argv_job(ExecutionTarget::EggworkNode {
            node_id: "node-1".to_string(),
        });
        let ctx = test_context(job, dir.path().to_path_buf());
        ctx.cancellation.cancel();
        let fake = Arc::new(FakeNodeClient::succeeding());
        let mut nodes = HashMap::new();
        nodes.insert("node-1".to_string(), test_node());
        let exec = EggworkExecutor::with_factory(
            EggworkExecutorConfig {
                nodes,
                store: None,
                ..Default::default()
            },
            Arc::new(FakeFactory {
                client: fake.clone(),
            }),
        );
        let completion = exec.execute(ctx).await;
        assert_eq!(completion.status, ExecutorStatus::Cancelled);
        assert!(fake.uploaded.lock().unwrap().is_empty());
        assert!(fake.cancelled.lock().unwrap().is_empty());
    }
}
