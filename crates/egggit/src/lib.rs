//! Read-only git and worktree facts.
//!
//! `egggit` exposes a small async API for inspecting a git repository:
//! branch, status, diff summary, changed files, log, blame, refs, and patch
//! validation. It does **not** mutate the repository; commit and worktree
//! create/remove stay in Codegg under the permission flow.

pub mod blame;
pub mod conflict;
pub mod diff;
pub mod log;
pub mod operation_state;
pub mod refs;
pub mod status;
pub mod status_v2;
pub mod worktree;

pub use blame::{blame_file, BlameEntry, BlameResult};
pub use conflict::{
    buffer_contains_conflict_markers, classify_conflict_code, default_actions_for, looks_binary,
    ConflictEntry, ConflictKind, ConflictObjectId, ConflictReport, ConflictShape,
    RecommendedConflictAction,
};
pub use diff::{
    changed_files, diff_summary, diff_text, file_diff, validate_patch, ChangedFile, DiffMode,
    DiffSummary, FileDiff, PatchValidation,
};
pub use log::{log_commits, CommitInfo};
pub use operation_state::{
    detect_operation_state_for_root, detect_repository_operation_state, ApplyState, BisectState,
    MergeState, OperationFamily, RebaseState, RecoveryAction, RepositoryOperationState,
    SequenceState, SequencerState, UnknownOperationState,
};
pub use refs::{list_branches, list_remotes, list_tags, BranchInfo, RemoteInfo, TagInfo};
pub use status::RepoStatus;
pub use status_v2::{DirtySummary, OperationState, RichRepoStatus, StatusEntry};
pub use worktree::WorktreeInfo;

use thiserror::Error;

/// Errors returned by the `egggit` API.
#[derive(Debug, Error)]
pub enum EgggitError {
    #[error("io error: {0}")]
    Io(String),

    #[error("git command failed: {0}")]
    Git(String),

    #[error("not a git repository: {0}")]
    NotARepository(String),

    #[error("invalid base ref: {0}")]
    InvalidBaseRef(String),

    #[error("task join error: {0}")]
    Join(String),
}
