//! Read-only git and worktree facts.
//!
//! `egggit` exposes a small async API for inspecting a git repository:
//! branch, status, diff summary, changed files, log, blame, refs, and patch
//! validation. It does **not** mutate the repository; commit and worktree
//! create/remove stay in Codegg under the permission flow.

pub mod blame;
pub mod diff;
pub mod log;
pub mod refs;
pub mod status;
pub mod status_v2;
pub mod worktree;

pub use blame::{blame_file, BlameEntry, BlameResult};
pub use diff::{
    changed_files, diff_summary, diff_text, file_diff, validate_patch, ChangedFile, DiffMode,
    DiffSummary, FileDiff, PatchValidation,
};
pub use log::{log_commits, CommitInfo};
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
