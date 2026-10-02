//! Application-layer execution-subject capture (Eggplan M003 C001 §5).
//!
//! This module owns the single authoritative attempt capture helper used by
//! the scheduler-owned attempt-start path and every attempt-seal path. Each
//! capture returns an [`ExecutionSubjectRevision`] v2 carrying:
//!
//! - the governed CodeGG-native subject (`egggit` capture, unchanged M001
//!   semantics, plain form without administrative exclusion);
//! - optionally, the exact Eggplan-compatible dirty digest captured at the
//!   same boundary from the repository-local Eggplan state root.
//!
//! The Eggplan side is best-effort: when the canonical workspace has no
//! repository-local `.eggplan` administrative root, or the Eggplan capture
//! is unavailable or disagrees on revision/clean-dirty state, the native
//! CodeGG provenance is still returned and the Eggplan field stays absent.
//! A missing Eggplan digest fails closed later at bound dirty translation
//! (`legacy_dirty_subject_missing_eggplan_digest`); it is never backfilled
//! from the current worktree after the attempt.
//!
//! The Eggplan digest is consumed only here and in the M003 binding identity
//! proof/translation (`crate::work_plan_repository_binding`). No other
//! module may capture Eggplan subjects (enforced by
//! `scripts/check_execution_subject_ownership.py`).

use std::path::Path;

use codegg_core::jobs::{
    ExecutionSubjectKind, ExecutionSubjectRevision, ExecutionSubjectState,
    ExecutionSubjectUnavailableReason,
};
use codegg_core::workspace::WorkspaceId;

/// Repository-local Eggplan administrative state root, relative to the
/// canonical workspace root. Mirrors
/// [`crate::work_plan_repository_binding::EGGPLAN_STATE_DIR`]; the binding
/// module remains the only owner of Eggplan store handles.
const EGGPLAN_STATE_DIR: &str = ".eggplan";

/// Capture the attempt revision for one workspace at an authoritative
/// execution boundary (attempt start or attempt seal).
///
/// The returned reason is `Some` exactly when no revision could be captured;
/// the Eggplan-compatible digest inside a returned revision is `Some`
/// exactly when the workspace carries repository-local Eggplan state whose
/// capture agrees with the native capture on revision and clean/dirty
/// state. Persisted provenance contains stable IDs/digests only — no
/// paths, contents, or credentials.
pub async fn capture_attempt_revision(
    workspace_root: &Path,
    workspace_id: &WorkspaceId,
) -> (
    Option<ExecutionSubjectRevision>,
    Option<ExecutionSubjectUnavailableReason>,
) {
    let native = match egggit::capture_git_source_subject(workspace_root).await {
        Ok(subject) => subject,
        Err(egggit::SubjectCaptureError::NotGit) => {
            return (None, Some(ExecutionSubjectUnavailableReason::NotGit));
        }
        Err(egggit::SubjectCaptureError::UnsafePath) => {
            return (None, Some(ExecutionSubjectUnavailableReason::UnsafePath));
        }
        Err(egggit::SubjectCaptureError::BoundsExceeded) => {
            return (
                None,
                Some(ExecutionSubjectUnavailableReason::BoundsExceeded),
            );
        }
        Err(_) => {
            return (None, Some(ExecutionSubjectUnavailableReason::CaptureFailed));
        }
    };
    let eggplan_digest = capture_eggplan_compatible_digest(workspace_root, &native);
    let revision = ExecutionSubjectRevision {
        schema_version: ExecutionSubjectRevision::SCHEMA_VERSION,
        subject_kind: ExecutionSubjectKind::Git,
        repository_identity: format!("codegg-workspace:{}", workspace_id.as_str()),
        revision: native.revision,
        state: if native.dirty_digest.is_some() {
            ExecutionSubjectState::Dirty
        } else {
            ExecutionSubjectState::Clean
        },
        dirty_digest: native.dirty_digest,
        eggplan_dirty_digest: eggplan_digest,
    };
    (Some(revision), None)
}

/// Best-effort Eggplan-compatible dirty digest for one native capture.
///
/// Returns `Some` only when all of these hold:
///
/// - `<workspace_root>/.eggplan` is a directory (otherwise `None` without
///   any store I/O, keeping non-Eggplan workspaces on the exact M001 path);
/// - the repository-local store opens read-only and its subject source
///   captures successfully;
/// - the Eggplan capture agrees with the native capture on HEAD revision
///   and on clean/dirty state (both owners must describe the same Git
///   state; the two digest *bytes* are intentionally never compared);
/// - the Eggplan digest is present (dirty) and in the required
///   `sha256:<hex>` form.
///
/// Any disagreement or failure yields `None`: native CodeGG provenance stays
/// available for unbound use while bound dirty translation fails closed.
fn capture_eggplan_compatible_digest(
    workspace_root: &Path,
    native: &egggit::GitSourceSubject,
) -> Option<String> {
    if !workspace_root.join(EGGPLAN_STATE_DIR).is_dir() {
        return None;
    }
    let store =
        eggplan_repo::RepositoryStore::open_read_only(workspace_root.join(EGGPLAN_STATE_DIR))
            .ok()?;
    let subject = store.subject_source().capture().ok()?;
    if subject.revision != native.revision {
        return None;
    }
    let native_dirty = native.dirty_digest.is_some();
    let eggplan_dirty = subject.state == eggplan_core::SubjectState::Dirty;
    if native_dirty != eggplan_dirty {
        return None;
    }
    let digest = subject.dirty_digest?;
    if !ExecutionSubjectRevision::is_valid_eggplan_dirty_digest(&digest) {
        return None;
    }
    Some(digest)
}
