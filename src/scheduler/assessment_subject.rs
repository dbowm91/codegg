//! Scheduler-owned current source-subject capture for M002 assessment.
//!
//! Historical execution provenance stays attempt-scoped (M001): the
//! scheduler captures S1 before execution and seals at the
//! live-execution or snapshot-materialization boundary. M002 additionally
//! needs the exact *current* subject to decide whether historical evidence
//! still applies now (staged assessment adoption).
//!
//! This entry point performs governed `egggit` capture rooted at a
//! caller-supplied canonical workspace root and builds the
//! [`ExecutionSubjectRevision`] with the same `codegg-workspace:{id}`
//! identity the scheduler persists at execution time, so current and
//! historical subjects compare on equal terms.
//!
//! Callers must supply the canonical root (workspace catalog or scheduler
//! lease). This module never consults process-global CWD, never writes
//! provenance, and never falls back to the current worktree for historical
//! records.

use codegg_core::jobs::{
    ExecutionSubjectKind, ExecutionSubjectRevision, ExecutionSubjectState,
    ExecutionSubjectUnavailableReason,
};
use codegg_core::workspace::WorkspaceId;
use std::path::Path;

/// Capture the current source subject for assessment against `workspace_root`.
///
/// Returns the revision on success. `NotGit` (and only `NotGit`) tells the
/// caller the workspace is positively not a Git subject; every other
/// failure is a typed unavailability the caller must treat as fail-closed.
pub async fn capture_current_assessment_subject(
    workspace_root: &Path,
    workspace_id: &WorkspaceId,
) -> Result<ExecutionSubjectRevision, ExecutionSubjectUnavailableReason> {
    match egggit::capture_git_source_subject(workspace_root).await {
        Ok(subject) => Ok(ExecutionSubjectRevision {
            schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
            subject_kind: ExecutionSubjectKind::Git,
            repository_identity: format!("codegg-workspace:{}", workspace_id.as_str()),
            revision: subject.revision,
            state: if subject.dirty_digest.is_some() {
                ExecutionSubjectState::Dirty
            } else {
                ExecutionSubjectState::Clean
            },
            dirty_digest: subject.dirty_digest,
        }),
        Err(egggit::SubjectCaptureError::NotGit) => Err(ExecutionSubjectUnavailableReason::NotGit),
        Err(egggit::SubjectCaptureError::UnsafePath) => {
            Err(ExecutionSubjectUnavailableReason::UnsafePath)
        }
        Err(egggit::SubjectCaptureError::BoundsExceeded) => {
            Err(ExecutionSubjectUnavailableReason::BoundsExceeded)
        }
        Err(_) => Err(ExecutionSubjectUnavailableReason::CaptureFailed),
    }
}
