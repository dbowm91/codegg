//! Read-only Git and worktree facts.
//!
//! `egggit` exposes a small async API for inspecting a Git repository:
//! branch, status, diff summary, changed files, log, blame, refs, worktree
//! facts, operation state, and patch validation. It does **not** mutate the
//! repository: commit, worktree create/remove, and other mutating workflows
//! stay with the host application, which owns its own permission and
//! approval policy.
//!
//! ## Scope
//!
//! The supported contract is read-only repository facts plus deterministic
//! pure helpers (conflict-marker classification, patch validation). The
//! low-level [`process`] module is unprivileged plumbing shared by the
//! structured read operations: it builds shell-free `git` commands with a
//! hardened environment policy but enforces no permission policy itself.
//! Callers that need mutating Git behavior own that policy outside this
//! crate.
//!
//! `egggit` has no dependency on any host application. One consumer is
//! CodeGG's workflow layer, which keeps mutation and permission checks
//! outside this crate.
// C001 hosted compiler-cache measurement: ordinary unrelated source change.

pub mod blame;
pub mod conflict;
pub mod diff;
pub mod log;
pub mod operation_state;
pub mod process;
pub mod refs;
pub mod status;
pub mod status_v2;
pub mod subject;
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
pub use process::{GitEnvPolicy, ALLOWED_ENV_VARS, ALWAYS_STRIPPED_ENV_VARS};
pub use refs::{
    list_branches, list_remotes, list_tags, resolve_commit, BranchInfo, RemoteInfo, TagInfo,
};
pub use status::RepoStatus;
pub use status_v2::{DirtySummary, OperationState, RichRepoStatus, StatusEntry};
pub use subject::{capture_git_source_subject, GitSourceSubject, SubjectCaptureError};
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

    #[error("git output exceeded the configured byte bound")]
    OutputTooLarge,
}
