//! Invariant tests for artifact handle safety (M3 WP D).
//!
//! Validates handle descriptor safety, ID uniqueness, summary
//! truncation, read request normalization, and registry error
//! behavior.

use std::sync::Arc;

use codegg_core::projection_replay::artifact_registry::{
    ArtifactRegistryError, ProjectionArtifactRegistry, RunStoreProjectionArtifactRegistry,
};
use codegg_core::projection_replay::artifacts::{
    ArtifactContentType, ArtifactKind, ArtifactReadRequest, HandleRegistrar, HandleRegistry,
    ProjectionArtifactHandle,
};

// ── 1. is_public_descriptor_safe rejects unsafe handles ──────────────

#[test]
fn source_record_id_starting_with_slash_rejected() {
    let handle = ProjectionArtifactHandle {
        handle_id: "art_abc123".into(),
        kind: ArtifactKind::RunOutput,
        project_id: "p1".into(),
        workspace_id: None,
        session_id: None,
        source_record_id: "/etc/passwd".into(),
        content_type: ArtifactContentType::Text,
        total_bytes: Some(100),
        created_at: 0,
        expires_at: None,
        revision: 1,
        public_summary: None,
    };
    assert!(
        !handle.is_public_descriptor_safe(),
        "source_record_id starting with '/' must be rejected"
    );
}

#[test]
fn source_record_id_with_dotdot_rejected() {
    let handle = ProjectionArtifactHandle {
        handle_id: "art_abc123".into(),
        kind: ArtifactKind::RunOutput,
        project_id: "p1".into(),
        workspace_id: None,
        session_id: None,
        source_record_id: "../secret".into(),
        content_type: ArtifactContentType::Text,
        total_bytes: Some(100),
        created_at: 0,
        expires_at: None,
        revision: 1,
        public_summary: None,
    };
    assert!(
        !handle.is_public_descriptor_safe(),
        "source_record_id containing '..' must be rejected"
    );
}

#[test]
fn handle_id_with_slash_rejected() {
    let handle = ProjectionArtifactHandle {
        handle_id: "art/abc123".into(),
        kind: ArtifactKind::RunOutput,
        project_id: "p1".into(),
        workspace_id: None,
        session_id: None,
        source_record_id: "rec-1".into(),
        content_type: ArtifactContentType::Text,
        total_bytes: Some(100),
        created_at: 0,
        expires_at: None,
        revision: 1,
        public_summary: None,
    };
    assert!(
        !handle.is_public_descriptor_safe(),
        "handle_id containing '/' must be rejected"
    );
}

#[test]
fn handle_id_with_dotdot_rejected() {
    let handle = ProjectionArtifactHandle {
        handle_id: "art..abc123".into(),
        kind: ArtifactKind::RunOutput,
        project_id: "p1".into(),
        workspace_id: None,
        session_id: None,
        source_record_id: "rec-1".into(),
        content_type: ArtifactContentType::Text,
        total_bytes: Some(100),
        created_at: 0,
        expires_at: None,
        revision: 1,
        public_summary: None,
    };
    assert!(
        !handle.is_public_descriptor_safe(),
        "handle_id containing '..' must be rejected"
    );
}

#[test]
fn safe_descriptor_passes_validation() {
    let registrar = HandleRegistrar::new();
    let handle = registrar.issue(
        ArtifactKind::RunOutput,
        "p1",
        Some("s1".into()),
        "rec-1",
        ArtifactContentType::Text,
        Some(1024),
        1,
        Some("test output".into()),
        Some(60_000),
    );
    assert!(
        handle.is_public_descriptor_safe(),
        "properly minted handle must pass validation"
    );
}

// ── 2. HandleRegistrar::mint() returns unique IDs ────────────────────

#[test]
fn handle_registrar_mint_returns_unique_ids_across_10000_invocations() {
    let registrar = HandleRegistrar::new();
    let mut ids = std::collections::HashSet::new();
    for _ in 0..10_000 {
        let id = registrar.mint();
        assert!(ids.insert(id.clone()), "duplicate handle ID minted: {}", id);
    }
    assert_eq!(ids.len(), 10_000);
}

// ── 3. HandleRegistrar::issue() truncates oversized summaries ─────────

#[test]
fn issue_truncates_oversized_public_summary() {
    let registrar = HandleRegistrar::new();
    let long_summary = "x".repeat(1024); // >512 char limit
    let handle = registrar.issue(
        ArtifactKind::ToolOutput,
        "p1",
        None,
        "rec-1",
        ArtifactContentType::Text,
        None,
        1,
        Some(long_summary),
        None,
    );
    let summary = handle.public_summary.as_ref().expect("summary must be set");
    assert!(
        summary.len() <= 512,
        "summary must be truncated to <=512 chars; got {}",
        summary.len()
    );
}

#[test]
fn issue_preserves_short_summary() {
    let registrar = HandleRegistrar::new();
    let short_summary = "short".to_string();
    let handle = registrar.issue(
        ArtifactKind::RunOutput,
        "p1",
        None,
        "rec-1",
        ArtifactContentType::Text,
        None,
        1,
        Some(short_summary.clone()),
        None,
    );
    assert_eq!(
        handle.public_summary.as_deref(),
        Some(short_summary.as_str()),
        "short summary must be preserved as-is"
    );
}

// ── 4. ArtifactReadRequest::normalize() clamps and rejects ───────────

#[test]
fn normalize_clamps_end_to_max_read_bytes() {
    let req = ArtifactReadRequest {
        handle_id: "art_abc".into(),
        start: 0,
        end: Some(1024 * 1024), // 1 MiB, far beyond MAX_READ_BYTES
        expected_revision: 1,
    };
    let (start, end) = req.normalize();
    assert_eq!(start, 0);
    assert_eq!(
        end,
        ArtifactReadRequest::MAX_READ_BYTES,
        "end must be clamped to MAX_READ_BYTES"
    );
}

#[test]
fn normalize_clamps_large_end_relative_to_start() {
    let req = ArtifactReadRequest {
        handle_id: "art_abc".into(),
        start: 1000,
        end: Some(1000 + 2 * ArtifactReadRequest::MAX_READ_BYTES),
        expected_revision: 1,
    };
    let (start, end) = req.normalize();
    assert_eq!(start, 1000);
    assert_eq!(
        end,
        1000 + ArtifactReadRequest::MAX_READ_BYTES,
        "end must be clamped to start + MAX_READ_BYTES"
    );
}

#[test]
fn normalize_start_equals_end_is_valid() {
    let req = ArtifactReadRequest {
        handle_id: "art_abc".into(),
        start: 42,
        end: Some(42),
        expected_revision: 1,
    };
    let (start, end) = req.normalize();
    assert_eq!(start, 42);
    assert_eq!(end, 42);
}

#[test]
fn normalize_end_before_start_is_clamped() {
    let req = ArtifactReadRequest {
        handle_id: "art_abc".into(),
        start: 100,
        end: Some(50), // end < start
        expected_revision: 1,
    };
    let (start, end) = req.normalize();
    assert_eq!(start, 100);
    // end.min(start + MAX_READ_BYTES) = 50.min(100 + 64KB) = 50
    assert_eq!(end, 50, "end before start is kept as-is by normalize");
}

// ── 5. ArtifactRegistry read for unknown handle returns NotFound ──────

#[tokio::test]
async fn artifact_registry_read_unknown_handle_returns_not_found() {
    let mock_store = Arc::new(MockRunStore::new());
    let registry = RunStoreProjectionArtifactRegistry::new(mock_store);
    let req = ArtifactReadRequest {
        handle_id: "art_nonexistent".into(),
        start: 0,
        end: Some(100),
        expected_revision: 1,
    };
    let result = registry.read(&req, "p1").await;
    assert!(
        matches!(result, Err(ArtifactRegistryError::NotFound)),
        "unknown handle must return NotFound; got {:?}",
        result
    );
}

// ── Mock RunStore for tests ──────────────────────────────────────────

use async_trait::async_trait;
use codegg_core::error::RunStoreError;
use codegg_core::run_store::{
    ArtifactChunk, ArtifactId, ArtifactInput, ByteRange, RunDraft, RunHandle, RunManifest,
    RunQuery, RunStore, RunSummary,
};
use dashmap::DashMap;

struct MockRunStore {
    #[allow(dead_code)]
    artifacts: DashMap<String, Vec<u8>>,
}

impl MockRunStore {
    fn new() -> Self {
        Self {
            artifacts: DashMap::new(),
        }
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
    ) -> Result<codegg_core::run_store::ArtifactRef, RunStoreError> {
        unimplemented!()
    }
    async fn complete_run(
        &self,
        _run: RunHandle,
        _completion: codegg_core::run_store::RunCompletion,
    ) -> Result<RunManifest, RunStoreError> {
        unimplemented!()
    }
    async fn get_run(
        &self,
        _id: &codegg_core::run_store::RunId,
    ) -> Result<Option<RunManifest>, RunStoreError> {
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
