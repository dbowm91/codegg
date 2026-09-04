//! Portable Git workflow result types.
//!
//! These types are the boundary between Git mutation execution and CodeGG's
//! durable/session adapters. They carry repository state and bounded output,
//! but deliberately do not depend on `codegg-core`, RunStore, permissions, or
//! UI types. This keeps future rerun/integration consumers on one stable Git
//! workflow vocabulary.

use crate::GitOperation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A snapshot of repository state captured before or after a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    pub head: String,
    pub branch: String,
    pub detached: bool,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflicted_count: usize,
    pub captured_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_status: Option<String>,
}

/// State changes observed around one mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDelta {
    pub before: RepoSnapshot,
    pub after: RepoSnapshot,
    #[serde(default)]
    pub commits_created: Vec<String>,
    #[serde(default)]
    pub refs_created: Vec<String>,
    #[serde(default)]
    pub refs_deleted: Vec<String>,
    #[serde(default)]
    pub paths_staged: Vec<String>,
    #[serde(default)]
    pub paths_unstaged: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

impl StateDelta {
    pub fn is_noop(&self) -> bool {
        self.commits_created.is_empty()
            && self.refs_created.is_empty()
            && self.refs_deleted.is_empty()
            && self.paths_staged.is_empty()
            && self.paths_unstaged.is_empty()
            && self.conflicts.is_empty()
            && self.before.head == self.after.head
            && self.before.branch == self.after.branch
            && self.before.staged_count == self.after.staged_count
            && self.before.unstaged_count == self.after.unstaged_count
    }
}

/// High-level outcome of a Git mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationOutcome {
    Completed,
    NoOp,
    FastForward { from: String, to: String },
    Conflict,
    Rejected { reason: String },
}

impl MutationOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NoOp => "no-op",
            Self::FastForward { .. } => "fast-forward",
            Self::Conflict => "conflict",
            Self::Rejected { .. } => "rejected",
        }
    }
}

/// Bounded, typed result returned by a Git mutation workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    pub operation: GitOperation,
    pub subcommand: String,
    pub delta: StateDelta,
    pub outcome: MutationOutcome,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration_ms: u64,
}
