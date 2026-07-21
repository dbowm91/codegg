//! Projection artifact registry (M3, WP D).
//!
//! Provides an in-memory metadata store backed by [`RunStore`] for
//! artifact content. The daemon holds an `Arc<dyn ProjectionArtifactRegistry>`
//! to authorize and serve artifact reads through the projection seam.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::error::RunStoreError;
use crate::projection_replay::artifacts::{
    ArtifactContentType, ArtifactKind, ArtifactReadRequest, ArtifactReadResponse, HandleLifecycle,
    ProjectionArtifactHandle,
};
use crate::run_store::{ArtifactChunk, ArtifactId, ByteRange, RunStore};

// ── Error ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, thiserror::Error)]
pub enum ArtifactRegistryError {
    #[error("artifact not found")]
    NotFound,
    #[error("revision mismatch: requested {requested}, current {current}")]
    RevisionMismatch { requested: u64, current: u64 },
    #[error("source content missing")]
    SourceMissing,
    #[error("run store error: {0}")]
    RunStore(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl From<RunStoreError> for ArtifactRegistryError {
    fn from(e: RunStoreError) -> Self {
        match e {
            RunStoreError::NotFound(msg) => ArtifactRegistryError::RunStore(msg),
            other => ArtifactRegistryError::RunStore(other.to_string()),
        }
    }
}

// ── Handle ID newtype ────────────────────────────────────────────────

/// Opaque artifact handle identifier. Wraps a UUID-based string
/// minted by [`crate::projection_replay::artifacts::HandleRegistrar`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HandleId(pub String);

impl HandleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HandleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── In-memory handle entry ───────────────────────────────────────────

/// Metadata stored for each issued artifact handle. The actual
/// content lives in [`RunStore`]; only the bounding descriptor is
/// held here.
#[derive(Debug, Clone)]
pub struct HandleEntry {
    pub handle: ProjectionArtifactHandle,
    /// The RunStore artifact id that backs this handle. When
    /// `None`, the handle is a synthetic placeholder (e.g. for
    /// logs or diffs not backed by RunStore).
    pub run_store_artifact_id: Option<ArtifactId>,
}

// ── Trait ─────────────────────────────────────────────────────────────

/// Projection artifact registry contract.
///
/// The daemon holds an `Arc<dyn ProjectionArtifactRegistry>` and
/// passes it through [`super::seam::ProjectionDisclosureContext`].
/// Implementations MUST be `Send + Sync`.
#[async_trait]
pub trait ProjectionArtifactRegistry: Send + Sync {
    /// Issue a new artifact handle for the given run and kind.
    /// The registry stores metadata and returns a bounded
    /// public descriptor that carries no authority by itself.
    async fn issue_for_run(
        &self,
        project_id: &str,
        run_id: &str,
        kind: ArtifactKind,
        content_type: ArtifactContentType,
        total_bytes: u64,
        source_record_id: &str,
        public_summary: Option<String>,
        revision: u64,
    ) -> Result<ProjectionArtifactHandle, ArtifactRegistryError>;

    /// List all artifact handles for a project.
    async fn list(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectionArtifactHandle>, ArtifactRegistryError>;

    /// Read artifact content through an authorized request. The
    /// implementation MUST clamp the read window to
    /// [`ArtifactReadRequest::MAX_READ_BYTES`], verify revision
    /// consistency, and return a bounded slice.
    async fn read(
        &self,
        request: &ArtifactReadRequest,
        project_id: &str,
    ) -> Result<ArtifactReadResponse, ArtifactRegistryError>;
}

// ── In-memory + RunStore implementation ──────────────────────────────

/// Artifact registry that stores metadata in a [`DashMap`] and
/// delegates content reads to a [`RunStore`] instance.
///
/// For M3 this is sufficient; durable metadata storage is a future
/// plan. The `HandleId` → `HandleEntry` map is process-local and
/// does not survive daemon restarts.
pub struct RunStoreProjectionArtifactRegistry {
    handles: DashMap<HandleId, HandleEntry>,
    run_store: Arc<dyn RunStore>,
}

impl std::fmt::Debug for RunStoreProjectionArtifactRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunStoreProjectionArtifactRegistry")
            .field("handles_len", &self.handles.len())
            .finish_non_exhaustive()
    }
}

impl RunStoreProjectionArtifactRegistry {
    pub fn new(run_store: Arc<dyn RunStore>) -> Self {
        Self {
            handles: DashMap::new(),
            run_store,
        }
    }
}

#[async_trait]
impl ProjectionArtifactRegistry for RunStoreProjectionArtifactRegistry {
    async fn issue_for_run(
        &self,
        project_id: &str,
        _run_id: &str,
        kind: ArtifactKind,
        content_type: ArtifactContentType,
        total_bytes: u64,
        source_record_id: &str,
        public_summary: Option<String>,
        revision: u64,
    ) -> Result<ProjectionArtifactHandle, ArtifactRegistryError> {
        let registrar = crate::projection_replay::artifacts::HandleRegistrar::new();
        let handle = registrar.issue(
            kind,
            project_id,
            None,
            source_record_id,
            content_type,
            Some(total_bytes),
            revision,
            public_summary,
            None,
        );

        // Link to RunStore artifact when the source_record_id
        // maps to a known artifact. If it doesn't, the handle is
        // still valid for metadata-only use.
        let artifact_id = ArtifactId::new_unchecked(source_record_id.to_string());
        let run_store_artifact_id =
            match self.run_store.read_artifact(&artifact_id, None).await {
                Ok(_) => Some(artifact_id),
                Err(_) => None,
            };

        let entry = HandleEntry {
            handle: handle.clone(),
            run_store_artifact_id,
        };
        self.handles.insert(HandleId::new(&handle.handle_id), entry);

        Ok(handle)
    }

    async fn list(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectionArtifactHandle>, ArtifactRegistryError> {
        let result: Vec<ProjectionArtifactHandle> = self
            .handles
            .iter()
            .filter(|e| e.value().handle.project_id == project_id)
            .map(|e| e.value().handle.clone())
            .collect();
        Ok(result)
    }

    async fn read(
        &self,
        request: &ArtifactReadRequest,
        project_id: &str,
    ) -> Result<ArtifactReadResponse, ArtifactRegistryError> {
        let handle_id = HandleId::new(&request.handle_id);

        let entry = self
            .handles
            .get(&handle_id)
            .ok_or(ArtifactRegistryError::NotFound)?;

        // Project scope check
        if entry.handle.project_id != project_id {
            return Err(ArtifactRegistryError::NotFound);
        }

        // Revision check
        if entry.handle.revision != request.expected_revision {
            return Err(ArtifactRegistryError::RevisionMismatch {
                requested: request.expected_revision,
                current: entry.handle.revision,
            });
        }

        // Check lifecycle
        match entry.handle.lifecycle() {
            HandleLifecycle::Active => {}
            HandleLifecycle::Expired => {
                return Err(ArtifactRegistryError::InvalidRequest(
                    "artifact expired".into(),
                ));
            }
            HandleLifecycle::Revoked => {
                return Err(ArtifactRegistryError::InvalidRequest(
                    "artifact revoked".into(),
                ));
            }
            HandleLifecycle::StaleRevision => {
                return Err(ArtifactRegistryError::InvalidRequest(
                    "artifact stale".into(),
                ));
            }
            HandleLifecycle::SourceMissing => {
                return Err(ArtifactRegistryError::SourceMissing);
            }
        }

        let Some(ref artifact_id) = entry.run_store_artifact_id else {
            return Err(ArtifactRegistryError::SourceMissing);
        };

        let (start, end) = request.normalize();
        let range = ByteRange {
            start: start as usize,
            end: end as usize,
        };

        let chunk: ArtifactChunk = self
            .run_store
            .read_artifact(artifact_id, Some(range))
            .await?;

        // Convert bytes to UTF-8, lossy for binary content
        let content = String::from_utf8_lossy(&chunk.data).to_string();
        let truncated = (end - start) < chunk.total_bytes;

        Ok(ArtifactReadResponse {
            handle_id: request.handle_id.clone(),
            revision: entry.handle.revision,
            start,
            end: start + chunk.data.len() as u64,
            content_type: entry.handle.content_type,
            content,
            redacted: false,
            truncated,
            note: if truncated {
                Some(format!(
                    "truncated to {} bytes of {} total",
                    chunk.data.len(),
                    chunk.total_bytes
                ))
            } else {
                None
            },
        })
    }
}

// ── ProjectionArtifactHandle lifecycle helper ────────────────────────

impl ProjectionArtifactHandle {
    /// Compute lifecycle from handle metadata. This mirrors the
    /// existing pattern in `artifacts.rs` but adds expiry/revoke
    /// checks.
    pub fn lifecycle(&self) -> HandleLifecycle {
        if self.handle_id.is_empty() || self.source_record_id.is_empty() {
            return HandleLifecycle::SourceMissing;
        }
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if now > expires_at {
                return HandleLifecycle::Expired;
            }
        }
        HandleLifecycle::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RunStoreError;
    use crate::projection_replay::artifacts::{ArtifactContentType, ArtifactKind};
    use crate::run_store::{
        ArtifactId, ArtifactInput, ByteRange, RunDraft, RunHandle, RunManifest, RunQuery,
        RunSummary,
    };

    // ── Mock RunStore ────────────────────────────────────────────────

    struct MockRunStore {
        artifacts: DashMap<String, Vec<u8>>,
    }

    impl MockRunStore {
        fn new() -> Self {
            Self {
                artifacts: DashMap::new(),
            }
        }

        fn insert(&self, id: &str, data: Vec<u8>) {
            self.artifacts.insert(id.to_string(), data);
        }
    }

    #[async_trait]
    impl RunStore for MockRunStore {
        async fn begin_run(&self, _draft: RunDraft) -> Result<RunHandle, RunStoreError> {
            unimplemented!()
        }
        async fn write_artifact(
            &self,
            _run: &RunHandle,
            _artifact: ArtifactInput,
        ) -> Result<crate::run_store::ArtifactRef, RunStoreError> {
            unimplemented!()
        }
        async fn complete_run(
            &self,
            _run: RunHandle,
            _completion: crate::run_store::RunCompletion,
        ) -> Result<RunManifest, RunStoreError> {
            unimplemented!()
        }
        async fn get_run(&self, _id: &crate::run_store::RunId) -> Result<Option<RunManifest>, RunStoreError> {
            unimplemented!()
        }
        async fn read_artifact(
            &self,
            id: &ArtifactId,
            range: Option<ByteRange>,
        ) -> Result<ArtifactChunk, RunStoreError> {
            let data = self
                .artifacts
                .get(id.as_str())
                .ok_or_else(|| RunStoreError::NotFound(id.as_str().to_string()))?;

            let total_bytes = data.len() as u64;
            let (start, end) = match range {
                Some(r) => (r.start, r.end.min(data.len())),
                None => (0, data.len()),
            };

            Ok(ArtifactChunk {
                artifact_id: id.clone(),
                data: data[start..end].to_vec(),
                total_bytes,
                byte_offset: start,
            })
        }
        async fn list_runs(&self, _query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError> {
            unimplemented!()
        }
    }

    // ── Tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn issue_and_read_round_trip() {
        let mock = Arc::new(MockRunStore::new());
        mock.insert("rec-1", b"hello world".to_vec());
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        let handle = registry
            .issue_for_run(
                "p1",
                "run-1",
                ArtifactKind::RunOutput,
                ArtifactContentType::Text,
                11,
                "rec-1",
                Some("test output".into()),
                1,
            )
            .await
            .unwrap();

        assert!(handle.is_public_descriptor_safe());
        assert_eq!(handle.project_id, "p1");

        let request = ArtifactReadRequest {
            handle_id: handle.handle_id.clone(),
            start: 0,
            end: Some(11),
            expected_revision: 1,
        };

        let response = registry.read(&request, "p1").await.unwrap();
        assert_eq!(response.content, "hello world");
        assert_eq!(response.revision, 1);
        assert!(!response.truncated);
    }

    #[tokio::test]
    async fn read_not_found_handle() {
        let mock = Arc::new(MockRunStore::new());
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        let request = ArtifactReadRequest {
            handle_id: "art_nonexistent".into(),
            start: 0,
            end: Some(100),
            expected_revision: 1,
        };

        let err = registry.read(&request, "p1").await.unwrap_err();
        assert!(matches!(err, ArtifactRegistryError::NotFound));
    }

    #[tokio::test]
    async fn read_wrong_project_denied() {
        let mock = Arc::new(MockRunStore::new());
        mock.insert("rec-1", b"data".to_vec());
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        let handle = registry
            .issue_for_run(
                "p1",
                "run-1",
                ArtifactKind::ToolOutput,
                ArtifactContentType::Text,
                4,
                "rec-1",
                None,
                1,
            )
            .await
            .unwrap();

        let request = ArtifactReadRequest {
            handle_id: handle.handle_id.clone(),
            start: 0,
            end: Some(4),
            expected_revision: 1,
        };

        let err = registry.read(&request, "p2").await.unwrap_err();
        assert!(matches!(err, ArtifactRegistryError::NotFound));
    }

    #[tokio::test]
    async fn revision_mismatch_returns_error() {
        let mock = Arc::new(MockRunStore::new());
        mock.insert("rec-1", b"data".to_vec());
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        let handle = registry
            .issue_for_run(
                "p1",
                "run-1",
                ArtifactKind::LogTail,
                ArtifactContentType::Text,
                4,
                "rec-1",
                None,
                1,
            )
            .await
            .unwrap();

        let request = ArtifactReadRequest {
            handle_id: handle.handle_id.clone(),
            start: 0,
            end: Some(4),
            expected_revision: 2, // wrong revision
        };

        let err = registry.read(&request, "p1").await.unwrap_err();
        assert!(
            matches!(err, ArtifactRegistryError::RevisionMismatch { requested: 2, current: 1 }),
            "expected RevisionMismatch, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn oversized_read_clamped_to_max() {
        let mock = Arc::new(MockRunStore::new());
        let big_data = vec![b'x'; 256 * 1024]; // 256KB
        mock.insert("rec-1", big_data);
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        let handle = registry
            .issue_for_run(
                "p1",
                "run-1",
                ArtifactKind::RunOutput,
                ArtifactContentType::Text,
                256 * 1024,
                "rec-1",
                None,
                1,
            )
            .await
            .unwrap();

        // Request beyond MAX_READ_BYTES
        let request = ArtifactReadRequest {
            handle_id: handle.handle_id.clone(),
            start: 0,
            end: Some(512 * 1024),
            expected_revision: 1,
        };

        let response = registry.read(&request, "p1").await.unwrap();
        // normalize() clamps end to start + MAX_READ_BYTES
        assert!(
            response.content.len() <= ArtifactReadRequest::MAX_READ_BYTES as usize,
            "response should be clamped to MAX_READ_BYTES"
        );
        assert!(response.truncated);
    }

    #[tokio::test]
    async fn list_returns_only_project_handles() {
        let mock = Arc::new(MockRunStore::new());
        mock.insert("rec-a", b"a".to_vec());
        mock.insert("rec-b", b"b".to_vec());
        let registry = RunStoreProjectionArtifactRegistry::new(mock);

        registry
            .issue_for_run(
                "p1",
                "run-1",
                ArtifactKind::RunOutput,
                ArtifactContentType::Text,
                1,
                "rec-a",
                None,
                1,
            )
            .await
            .unwrap();

        registry
            .issue_for_run(
                "p2",
                "run-2",
                ArtifactKind::ToolOutput,
                ArtifactContentType::Text,
                1,
                "rec-b",
                None,
                1,
            )
            .await
            .unwrap();

        let p1_handles = registry.list("p1").await.unwrap();
        assert_eq!(p1_handles.len(), 1);
        assert_eq!(p1_handles[0].project_id, "p1");

        let p2_handles = registry.list("p2").await.unwrap();
        assert_eq!(p2_handles.len(), 1);
        assert_eq!(p2_handles[0].project_id, "p2");

        let p3_handles = registry.list("p3").await.unwrap();
        assert!(p3_handles.is_empty());
    }
}
