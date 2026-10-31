//! Read-only git facts exposed to frontends over the core protocol.
//!
//! These DTOs describe **facts about a repository**, never a repository
//! location. A request carries a `project_id`; the daemon resolves the
//! repository root from its own durable project binding, so there is no
//! client-supplied path for a caller to point at an unrelated tree.
//!
//! Every collection here is bounded. The daemon clamps the row count to
//! [`MAX_GIT_READ_ROWS`] and reports `truncated` when it had to drop rows,
//! so a renderer can draw a complete picture from a single response without
//! a follow-up round trip.

use serde::{Deserialize, Serialize};

/// Maximum number of rows (changed files, branches) one git read returns.
///
/// The daemon clamps a client-requested `max_rows` to this value, so a client
/// can narrow the bound but never widen it.
pub const MAX_GIT_READ_ROWS: usize = 500;

/// Which read-only fact set a git read asks for.
///
/// The set is closed on purpose: every variant names a fact the daemon
/// already knows how to read from the read-only git fact layer. There is
/// deliberately no "raw arguments" variant, because a client-supplied
/// argument list would let a caller smuggle an option (or a path) into the
/// underlying git invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReadOperationDto {
    /// The current branch, whether the working tree is dirty, the `HEAD`
    /// commit, and the stash count.
    Status,
    /// A per-file change summary of the working tree against `HEAD`.
    Diff,
    /// The local branches with their upstream tracking information.
    Branches,
}

/// One read-only git request, scoped to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitReadRequestDto {
    /// The project whose bound repository is read.
    ///
    /// The daemon resolves the repository root from its own durable project
    /// binding; this request carries no filesystem path.
    pub project_id: String,
    /// Which fact set to read.
    pub operation: GitReadOperationDto,
    /// An optional client-requested row cap.
    ///
    /// The daemon clamps this to [`MAX_GIT_READ_ROWS`]. `None` means "the
    /// daemon's cap".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<usize>,
}

/// The result of a git read.
///
/// Tagged by `kind` so a renderer switches on one field instead of probing
/// every variant. Each variant carries every field of its shape — nothing is
/// omitted — so the rendered result never needs a second round trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitReadResultDto {
    /// The repository's status facts.
    Status(GitRepoStatusDto),
    /// The working tree's diff summary.
    Diff(GitDiffSummaryDto),
    /// The repository's local branch list.
    Branches(GitBranchListDto),
}

/// The daemon's git revert request (M007-H, work package C).
///
/// The daemon resolves the checkout from its own durable project binding,
/// so this request carries identifiers and repository-relative paths — never
/// a filesystem path a caller could point at an unrelated tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRevertRequestDto {
    /// The project whose bound checkout is reverted.
    pub project_id: String,
    /// The workspace whose checkout is reverted.
    pub workspace_id: String,
    /// The session the revert was invoked from, when one is attached.
    ///
    /// The session is what the daemon resolves the acting turn and the
    /// checkout's writer attribution from. It is never a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The repository-relative paths restored from `HEAD`.
    pub paths: Vec<String>,
}

/// The result of a git revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRevertResultDto {
    /// The project whose checkout was reverted.
    pub project_id: String,
    /// The workspace whose checkout was reverted.
    pub workspace_id: String,
    /// The repository-relative paths the revert restored.
    pub paths: Vec<String>,
    /// The typed outcome of the underlying mutation: `completed`, `no-op`,
    /// `fast-forward`, `conflict` or `rejected`.
    pub outcome: String,
}

/// The daemon's diff request (M007-I, work package A).
///
/// The daemon resolves the checkout from its own durable project binding, so
/// this request carries identifiers and a repository-relative path — never a
/// filesystem path a caller could point at an unrelated tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiffRequestDto {
    /// The project whose bound checkout is diffed.
    pub project_id: String,
    /// The workspace whose checkout is diffed.
    pub workspace_id: String,
    /// The session the diff was invoked from, when one is attached.
    ///
    /// The session is what the daemon resolves the acting turn from. It is
    /// never a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The repository-relative path to diff.
    ///
    /// `None` is the **bare** `/diff` branch: the whole working tree against
    /// `HEAD`. `Some(path)` is the `/diff <path>` branch: that one file
    /// against `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The result of a diff read.
///
/// Both branches of the diff view carry their own bytes, so the frontend
/// renders the answer the daemon produced without reading the working tree
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiffResultDto {
    /// The project whose checkout was diffed.
    pub project_id: String,
    /// The workspace whose checkout was diffed.
    pub workspace_id: String,
    /// The repository-relative path that was diffed, when the request named
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The bytes as they stood at `HEAD`.
    ///
    /// The bare branch has no `HEAD` side, so it is empty there.
    pub old_content: String,
    /// The bytes as they stand in the working tree.
    pub new_content: String,
    /// The title the diff view carries.
    pub title: String,
}

/// The status of one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRepoStatusDto {
    /// The checked-out branch, or `"unknown"` when the read could not
    /// determine it.
    pub branch: String,
    /// Whether the working tree carries uncommitted changes.
    pub is_dirty: bool,
    /// The `HEAD` commit hash, when the repository has one.
    pub commit_hash: Option<String>,
    /// How many stashes the repository holds.
    pub stash_count: usize,
    /// The consistency class the read was obtained under.
    pub consistency: GitReadConsistencyDto,
}

/// The object-generation consistency class of a git read.
///
/// A read is only as trustworthy as the generation it observed, so the class
/// travels with the facts rather than being left to the reader's memory. This
/// is the wire form of the read-only fact layer's consistency class: the
/// daemon reports what the observation is worth, and a renderer cannot
/// receive the bytes without also receiving that answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReadConsistencyDto {
    /// The read served objects from one materialized generation.
    ImmutableGeneration,
    /// The read served objects from a capture that was immutable when it was
    /// taken and may now be stale relative to the repository.
    CapturedNotCurrent,
    /// The read served objects directly from a concurrently mutable
    /// repository, so the observation may be torn.
    LiveMutable,
    /// The read consulted no object store, so it observed no object
    /// generation at all.
    NoObjectGeneration,
}

/// A bounded summary of the working tree's changes against `HEAD`.
///
/// The summary is a **count-and-name** view: it never carries patch text, so
/// a diff of any size stays a bounded response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiffSummaryDto {
    /// How many files changed.
    pub files_changed: usize,
    /// How many lines were added.
    pub insertions: usize,
    /// How many lines were removed.
    pub deletions: usize,
    /// The changed files, capped at the daemon's row bound.
    pub files: Vec<GitChangedFileDto>,
    /// How many files changed before the cap was applied.
    pub total_files: usize,
    /// Whether `files` was shortened by the cap.
    pub truncated: bool,
    /// The consistency class the read was obtained under.
    pub consistency: GitReadConsistencyDto,
}

/// One changed file in a diff summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChangedFileDto {
    /// The repository-relative path of the changed file.
    pub path: String,
    /// What happened to the file.
    pub kind: GitChangeKindDto,
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeKindDto {
    /// The file was added.
    Added,
    /// The file's content changed.
    Modified,
    /// The file was removed.
    Deleted,
    /// The file was renamed.
    Renamed,
    /// The file was copied.
    Copied,
    /// The read-only fact layer could not classify the change.
    Other,
}

/// A bounded list of the repository's local branches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBranchListDto {
    /// The branches, capped at the daemon's row bound.
    pub branches: Vec<GitBranchDto>,
    /// How many branches there were before the cap was applied.
    pub total: usize,
    /// Whether `branches` was shortened by the cap.
    pub truncated: bool,
    /// The consistency class the read was obtained under.
    pub consistency: GitReadConsistencyDto,
}

/// One local branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBranchDto {
    /// The branch name.
    pub name: String,
    /// Whether this is the checked-out branch.
    pub is_current: bool,
    /// Whether the repository is in a detached `HEAD` state.
    pub is_detached: bool,
    /// The upstream tracking ref, when the branch has one.
    pub upstream: Option<String>,
    /// How many commits the branch is ahead of its upstream.
    pub ahead: Option<i32>,
    /// How many commits the branch is behind its upstream.
    pub behind: Option<i32>,
    /// The branch's `HEAD` commit hash, when it has one.
    pub head: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_read_request_ignores_a_client_supplied_path() {
        // The request shape has no path field: a client that sends one is
        // ignored, not honoured. This is the wire-level half of "the
        // repository root is resolved server-side".
        let json = serde_json::json!({
            "project_id": "project-1",
            "operation": "status",
            "path": "/etc",
            "repository_root": "/etc",
        });
        let request: GitReadRequestDto =
            serde_json::from_value(json).expect("the request decodes without a path field");
        assert_eq!(request.project_id, "project-1");
        assert_eq!(request.operation, GitReadOperationDto::Status);
        assert_eq!(request.max_rows, None);
        let rendered = serde_json::to_value(&request).expect("the request re-renders");
        assert!(
            rendered.get("path").is_none(),
            "a client-supplied path must not survive into the request"
        );
        assert!(rendered.get("repository_root").is_none());
    }

    #[test]
    fn git_read_request_clamps_are_left_to_the_daemon() {
        // The request carries the client's requested bound; the daemon owns
        // the clamp, so the wire value round-trips unchanged and the daemon
        // is the only place the bound is decided.
        let request = GitReadRequestDto {
            project_id: "project-1".into(),
            operation: GitReadOperationDto::Diff,
            max_rows: Some(10),
        };
        let json = serde_json::to_value(&request).expect("the request renders");
        assert_eq!(
            json.get("max_rows").and_then(|value| value.as_u64()),
            Some(10)
        );
        let decoded: GitReadRequestDto =
            serde_json::from_value(json).expect("the request decodes back");
        assert_eq!(decoded, request);
    }

    #[test]
    fn git_read_result_is_tagged_by_kind() {
        let result = GitReadResultDto::Status(GitRepoStatusDto {
            branch: "main".into(),
            is_dirty: true,
            commit_hash: Some("abc123".into()),
            stash_count: 2,
            consistency: GitReadConsistencyDto::CapturedNotCurrent,
        });
        let json = serde_json::to_value(&result).expect("the result renders");
        assert_eq!(
            json.get("kind").and_then(|value| value.as_str()),
            Some("status")
        );
        assert_eq!(
            json.get("branch").and_then(|value| value.as_str()),
            Some("main")
        );
        let decoded: GitReadResultDto =
            serde_json::from_value(json).expect("the result decodes back");
        assert_eq!(decoded, result);
    }

    #[test]
    fn diff_summary_reports_its_truncation() {
        let result = GitReadResultDto::Diff(GitDiffSummaryDto {
            files_changed: 2,
            insertions: 3,
            deletions: 1,
            files: vec![GitChangedFileDto {
                path: "src/lib.rs".into(),
                kind: GitChangeKindDto::Modified,
            }],
            total_files: 900,
            truncated: true,
            consistency: GitReadConsistencyDto::ImmutableGeneration,
        });
        let json = serde_json::to_value(&result).expect("the result renders");
        assert_eq!(
            json.get("truncated").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            json.get("total_files").and_then(|value| value.as_u64()),
            Some(900)
        );
    }

    #[test]
    fn every_read_carries_its_consistency_class() {
        // The class travels with the facts: a renderer cannot receive the
        // bytes without also receiving what the observation is worth.
        // A read that dropped it on the way to the wire would make the
        // class unfalsifiable from the client's side.
        for (class, wire) in [
            (
                GitReadConsistencyDto::ImmutableGeneration,
                "immutable_generation",
            ),
            (
                GitReadConsistencyDto::CapturedNotCurrent,
                "captured_not_current",
            ),
            (GitReadConsistencyDto::LiveMutable, "live_mutable"),
            (
                GitReadConsistencyDto::NoObjectGeneration,
                "no_object_generation",
            ),
        ] {
            let result = GitReadResultDto::Diff(GitDiffSummaryDto {
                files_changed: 0,
                insertions: 0,
                deletions: 0,
                files: Vec::new(),
                total_files: 0,
                truncated: false,
                consistency: class,
            });
            let json = serde_json::to_value(&result).expect("the result renders");
            assert_eq!(
                json.get("consistency").and_then(|value| value.as_str()),
                Some(wire),
                "the class must survive onto the wire"
            );
            let decoded: GitReadResultDto =
                serde_json::from_value(json).expect("the result decodes back");
            assert_eq!(decoded, result);
        }
    }
}
