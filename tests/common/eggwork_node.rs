//! A scripted Eggwork node client for integration tests.
//!
//! The fake implements the whole [`EggworkNodeClient`] seam so a test can
//! stand up a node without a TLS fixture. Only the host telemetry is
//! scripted in detail (that is the surface under test); the execution paths
//! answer with an explicit "not implemented" error rather than a plausible
//! success, so a test can never mistake the fake for a working node.
//!
//! The fake also counts its telemetry reads. That counter is what proves
//! the daemon-owned coordinator coalesces concurrent readers onto one
//! upstream read instead of amplifying the node's poll of its provider.

// Every integration test binary that declares `mod common;` compiles this
// module, but only the ones that stand up a scripted node use it. The
// shared-module convention (see `pool.rs`) is to silence that here rather
// than in each consumer.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::scheduler::eggwork::{EggworkClientFactory, EggworkNodeClient, ResolvedEggworkNode};
use eggfetch_core::BoxBytesStream;
use eggwork_client::{ClientError as EggworkClientError, WorkspaceReady};
use eggwork_core::{
    ArtifactRecord, BlobDigest, ExecutionHandle, ExecutionId, ExecutionSnapshot, ExecutionSpec,
    NodeCapabilities, NodeId, NodeStatus, NodeTelemetry, WorkspaceManifest, WorkspaceManifestPatch,
};

/// The explicit "this fake does not run jobs" answer.
fn not_implemented() -> EggworkClientError {
    EggworkClientError::Api {
        status: 501,
        code: "not_implemented".to_string(),
        message: "the scripted node does not execute jobs".to_string(),
    }
}

/// A scripted telemetry failure.
///
/// [`eggwork_client::ClientError`] is not `Clone`, so the fake stores the
/// *kind* of failure and rebuilds the error on each read.
#[derive(Debug, Clone, Copy)]
pub enum ScriptedTelemetryError {
    /// The node does not implement the telemetry contract.
    Unsupported,
    /// The node answered with something the client rejected.
    InvalidResponse,
    /// The node could not be reached.
    Transport,
    /// The node answered with a schema this client does not understand.
    ProtocolIncompatible,
}

impl ScriptedTelemetryError {
    fn into_client_error(self) -> EggworkClientError {
        match self {
            Self::Unsupported => EggworkClientError::TelemetryUnsupported,
            Self::InvalidResponse => EggworkClientError::InvalidResponse,
            Self::Transport => EggworkClientError::Transport(eggfetch_core::Error::Connect(
                "connection refused".to_string(),
            )),
            Self::ProtocolIncompatible => EggworkClientError::ProtocolIncompatible,
        }
    }
}

/// A node whose host telemetry is scripted.
pub struct ScriptedNodeClient {
    /// The scripted telemetry document. `None` means the node does not
    /// implement the telemetry contract at all.
    telemetry: Mutex<Option<NodeTelemetry>>,
    /// A scripted failure, used to model an old node or a refused read.
    telemetry_error: Mutex<Option<ScriptedTelemetryError>>,
    /// How many telemetry reads this node has served.
    telemetry_reads: AtomicUsize,
}

impl Default for ScriptedNodeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedNodeClient {
    pub fn new() -> Self {
        Self {
            telemetry: Mutex::new(None),
            telemetry_error: Mutex::new(None),
            telemetry_reads: AtomicUsize::new(0),
        }
    }

    /// Script a telemetry document for the node.
    pub fn with_telemetry(self, telemetry: NodeTelemetry) -> Self {
        *self.telemetry.lock().unwrap() = Some(telemetry);
        self
    }

    /// Script a telemetry failure for the node.
    pub fn with_telemetry_error(self, error: ScriptedTelemetryError) -> Self {
        *self.telemetry_error.lock().unwrap() = Some(error);
        self
    }

    /// How many telemetry reads the node has served.
    pub fn telemetry_reads(&self) -> usize {
        self.telemetry_reads.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EggworkNodeClient for ScriptedNodeClient {
    async fn capabilities(&self) -> Result<NodeCapabilities, EggworkClientError> {
        Err(not_implemented())
    }

    async fn status(&self) -> Result<NodeStatus, EggworkClientError> {
        Err(not_implemented())
    }

    async fn telemetry(&self, expected: &NodeId) -> Result<NodeTelemetry, EggworkClientError> {
        self.telemetry_reads.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.telemetry_error.lock().unwrap().clone() {
            return Err(error.into_client_error());
        }
        match self.telemetry.lock().unwrap().clone() {
            None => Err(EggworkClientError::TelemetryUnsupported),
            Some(document) => {
                // The identity check is part of the upstream contract: a
                // document addressed to another node is refused, never
                // returned and never joined to the requested node.
                if document.node_id != *expected {
                    return Err(EggworkClientError::InvalidResponse);
                }
                Ok(document)
            }
        }
    }

    async fn find_missing_blobs(
        &self,
        _digests: &[BlobDigest],
    ) -> Result<Vec<BlobDigest>, EggworkClientError> {
        Err(not_implemented())
    }

    async fn upload_blob(
        &self,
        _digest: &BlobDigest,
        _declared_length: u64,
        _stream: BoxBytesStream,
    ) -> Result<(), EggworkClientError> {
        Err(not_implemented())
    }

    async fn create_workspace(
        &self,
        _workspace_id: &eggwork_core::WorkspaceId,
        _handle: &ExecutionHandle,
        _manifest: &WorkspaceManifest,
    ) -> Result<WorkspaceReady, EggworkClientError> {
        Err(not_implemented())
    }

    async fn create_workspace_derived(
        &self,
        _workspace_id: &eggwork_core::WorkspaceId,
        _handle: &ExecutionHandle,
        _base_manifest_digest: &BlobDigest,
        _patch: &WorkspaceManifestPatch,
    ) -> Result<WorkspaceReady, EggworkClientError> {
        Err(not_implemented())
    }

    async fn execute_in_workspace(
        &self,
        _spec: &ExecutionSpec,
        _handle: &ExecutionHandle,
        _workspace_id: &eggwork_core::WorkspaceId,
    ) -> Result<codegg::scheduler::eggwork::BoxEventStream, EggworkClientError> {
        Err(not_implemented())
    }

    async fn observe_generation(
        &self,
        _id: &ExecutionId,
        _generation: u64,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        Err(not_implemented())
    }

    async fn cancel(
        &self,
        _handle: &ExecutionHandle,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        Err(not_implemented())
    }

    async fn renew(
        &self,
        _handle: &ExecutionHandle,
        _renewal_id: &str,
    ) -> Result<ExecutionSnapshot, EggworkClientError> {
        Err(not_implemented())
    }

    async fn artifacts(
        &self,
        _execution_id: &ExecutionId,
        _generation: u64,
    ) -> Result<Vec<ArtifactRecord>, EggworkClientError> {
        Err(not_implemented())
    }

    async fn download_artifact(
        &self,
        _artifact: &ArtifactRecord,
    ) -> Result<BoxBytesStream, EggworkClientError> {
        Err(not_implemented())
    }
}

/// A factory that hands every node the same scripted client.
pub struct ScriptedNodeFactory {
    client: Arc<ScriptedNodeClient>,
}

impl ScriptedNodeFactory {
    pub fn new(client: Arc<ScriptedNodeClient>) -> Self {
        Self { client }
    }

    /// The client this factory hands out.
    pub fn client(&self) -> &Arc<ScriptedNodeClient> {
        &self.client
    }
}

impl EggworkClientFactory for ScriptedNodeFactory {
    fn client_for(
        &self,
        _node: &ResolvedEggworkNode,
    ) -> Result<Arc<dyn EggworkNodeClient>, String> {
        Ok(self.client.clone() as Arc<dyn EggworkNodeClient>)
    }
}
