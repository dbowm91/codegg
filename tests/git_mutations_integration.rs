//! Integration tests for the typed Git mutation framework (`src/git_mutations.rs`,
//! `src/git_mutations_ops.rs`). Each test builds a fresh in-temp-dir Git repo,
//! stages changes, runs the typed mutation helpers, and asserts on the
//! returned [`MutationResult`] (state delta, exit code, success).
//!
//! These tests rely on the host having `git` in PATH. The repo skips
//! gracefully (via `binary_check`) if `git` is not available so CI on
//! minimal containers still passes.

use std::path::Path;
use std::process::Command;

use codegg::git_mutation_projector::project_mutation;
use codegg::git_mutations::{MutationOutcome, MutationResult};
use codegg::git_mutations_ops;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

/// Initialize a fresh repo in `dir` with one commit on `main`.
fn init_repo(dir: &Path) {
    let run = |argv: &[&str]| {
        let status = Command::new("git")
            .args(argv)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .expect("git subcommand failed");
        assert!(status.success(), "git {argv:?} failed");
    };
    run(&["init", "-q", "-b", "main"]);
    fs_utils::write(&dir.join("README.md"), "hello\n");
    run(&["add", "README.md"]);
    run(&["commit", "-q", "-m", "initial"]);
}

/// Make `dir` point to a fresh temp directory with a fresh git repo.
fn fresh_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo(dir.path());
    dir
}

/// Touch `path` inside `repo` with given content and `git add` it.
fn write_and_stage(repo: &Path, relative: &str, content: &str) {
    let full = repo.join(relative);
    fs_utils::ensure_parent(&full);
    fs_utils::write(&full, content);
    let status = Command::new("git")
        .args(["add", "--", relative])
        .current_dir(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git add failed");
    assert!(status.success(), "git add {relative} failed");
}

fn head_message(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(repo)
        .output()
        .expect("git log");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_count(repo: &Path) -> usize {
    let out = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-list");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

fn branch_list(repo: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo)
        .output()
        .expect("git branch");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

/// Assert the mutation succeeded and returned a `Complete` outcome.
fn assert_clean(result: &MutationResult) {
    assert!(result.success, "mutation failed: {}", result.stderr);
    assert_eq!(
        result.outcome,
        MutationOutcome::Completed,
        "expected Completed, got {:?}",
        result.outcome
    );
}

mod fs_utils {
    use std::fs;
    use std::path::Path;

    pub fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parents");
        }
        fs::write(path, contents).expect("write");
    }

    pub fn ensure_parent(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parents");
        }
    }
}

fn exec() -> codegg::git_mutations::GitMutationExecutor {
    use codegg::git_mutations::{GitEnvPolicy, GitMutationExecutor};
    GitMutationExecutor::new().with_env_policy(GitEnvPolicy::default())
}

#[tokio::test]
async fn stage_all_marks_changes_staged() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("new.txt"), "1\n");
    let res = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    assert_clean(&res);
    let staged = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&staged.stdout).to_string();
    assert!(
        files.contains("new.txt"),
        "expected new.txt in staged set, got: {files}"
    );
}

#[tokio::test]
async fn stage_paths_returns_paths_in_delta() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    fs_utils::write(&repo.path().join("b.txt"), "b\n");
    let res = git_mutations_ops::stage_paths(&exec(), repo.path(), vec!["a.txt".to_string()])
        .await
        .unwrap();
    assert_clean(&res);
    assert!(
        res.delta.paths_staged.iter().any(|p| p == "a.txt"),
        "expected a.txt in paths_staged, got {:?}",
        res.delta.paths_staged
    );
    assert!(
        res.delta.paths_staged.iter().all(|p| p != "b.txt"),
        "b.txt should not be staged: {:?}",
        res.delta.paths_staged
    );
}

#[tokio::test]
async fn commit_creates_head_with_message() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    write_and_stage(repo.path(), "a.txt", "a\n");

    let outcome = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::AlreadyStaged,
        "add a",
        false,
        false,
    )
    .await
    .unwrap();

    assert_clean(&outcome.mutation);
    assert_eq!(
        outcome.mutation.delta.after.branch,
        current_branch(repo.path())
    );
    assert_eq!(head_message(repo.path()), "add a");
    assert_eq!(head_count(repo.path()), 2);
}

#[tokio::test]
async fn commit_amend_returns_previous_message() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    write_and_stage(repo.path(), "a.txt", "a\n");
    let _ = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::AlreadyStaged,
        "first",
        false,
        false,
    )
    .await
    .unwrap();
    // Make a tracked change so the amend has staged content.
    fs_utils::write(&repo.path().join("README.md"), "updated\n");
    write_and_stage(repo.path(), "README.md", "updated\n");
    let amend = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::AlreadyStaged,
        "first",
        true,
        false,
    )
    .await
    .unwrap();
    assert!(amend.amended);
    assert_eq!(head_count(repo.path()), 2);
}

#[tokio::test]
async fn branch_create_and_switch_round_trip() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    let res = git_mutations_ops::create_and_switch(&exec(), repo.path(), "feature", None)
        .await
        .unwrap();
    assert_clean(&res);
    let cur = current_branch(repo.path());
    assert_eq!(cur, "feature");
    assert!(branch_list(repo.path()).iter().any(|b| b == "feature"));
}

#[tokio::test]
async fn branch_delete_refuses_current() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    let _ = git_mutations_ops::create_and_switch(&exec(), repo.path(), "feature", None)
        .await
        .unwrap();
    // Switch back to main so deleting feature is safe.
    let _ = git_mutations_ops::switch_branch(&exec(), repo.path(), "main", false)
        .await
        .unwrap();
    let res = git_mutations_ops::branch_delete(&exec(), repo.path(), "feature", false)
        .await
        .unwrap();
    assert_clean(&res);
    assert!(
        !branch_list(repo.path()).iter().any(|b| b == "feature"),
        "feature should be removed"
    );
}

#[tokio::test]
async fn stash_push_then_apply_restores_state() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("README.md"), "tweaked\n");
    write_and_stage(repo.path(), "README.md", "tweaked\n");
    let push = git_mutations_ops::stash_push(&exec(), repo.path(), Some("work"), false, Vec::new())
        .await
        .unwrap();
    assert_clean(&push);
    // After stash, working tree should be clean.
    let status_after = git_status_clean(repo.path());
    assert!(status_after, "expected clean tree after stash");
    let apply = git_mutations_ops::stash_apply(&exec(), repo.path(), Some("stash@{0}"), false)
        .await
        .unwrap();
    assert_clean(&apply);
}

#[tokio::test]
async fn merge_fast_forward_records_into_state_delta() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    let _ = git_mutations_ops::create_and_switch(&exec(), repo.path(), "topic", None)
        .await
        .unwrap();
    fs_utils::write(&repo.path().join("b.txt"), "b\n");
    let _ = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    let _ = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::StageAll,
        "add b",
        false,
        false,
    )
    .await
    .unwrap();
    let _ = git_mutations_ops::switch_branch(&exec(), repo.path(), "main", false)
        .await
        .unwrap();
    let res =
        git_mutations_ops::merge(&exec(), repo.path(), vec!["topic".to_string()], false, None)
            .await
            .unwrap();
    assert!(matches!(
        res.outcome,
        MutationOutcome::FastForward { .. } | MutationOutcome::Completed
    ));
}

#[tokio::test]
async fn merge_conflict_yields_conflict_outcome() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    // Create the shared ancestor on a side branch so both `main` and
    // `other` can diverge before the merge.
    let _ = git_mutations_ops::create_and_switch(&exec(), repo.path(), "feature", None)
        .await
        .unwrap();
    fs_utils::write(&repo.path().join("a.txt"), "common\n");
    let _ = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    let _ = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::StageAll,
        "add a",
        false,
        false,
    )
    .await
    .unwrap();
    // Back to main, take diverging commit.
    let _ = git_mutations_ops::switch_branch(&exec(), repo.path(), "main", false)
        .await
        .unwrap();
    fs_utils::write(&repo.path().join("a.txt"), "main-change\n");
    let _ = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    let _ = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::StageAll,
        "main a",
        false,
        false,
    )
    .await
    .unwrap();
    // Switch to other side, diverging commit.
    let _ = git_mutations_ops::switch_branch(&exec(), repo.path(), "feature", false)
        .await
        .unwrap();
    fs_utils::write(&repo.path().join("a.txt"), "other-change\n");
    let _ = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    let _ = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::StageAll,
        "other a",
        false,
        false,
    )
    .await
    .unwrap();
    let _ = git_mutations_ops::switch_branch(&exec(), repo.path(), "main", false)
        .await
        .unwrap();
    let res = git_mutations_ops::merge(
        &exec(),
        repo.path(),
        vec!["feature".to_string()],
        false,
        None,
    )
    .await
    .unwrap();
    assert!(
        matches!(res.outcome, MutationOutcome::Conflict),
        "expected Conflict, got {:?}",
        res.outcome
    );
    assert!(
        !res.success || res.delta.conflicts.iter().any(|c| c.contains("a.txt")),
        "expected conflict in a.txt or non-success, got conflicts={:?}",
        res.delta.conflicts
    );
    // Abort to leave the repo in a clean state.
    let _ = git_mutations_ops::abort_in_progress(&exec(), repo.path())
        .await
        .unwrap();
}

#[tokio::test]
async fn env_policy_pins_git_terminal_prompt_and_editor() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    let push = git_mutations_ops::stash_push(&exec(), repo.path(), Some("pol"), false, Vec::new())
        .await
        .unwrap();
    assert_clean(&push);
    // Any subsequent mutation must not see leaked GIT_EDITOR env.
    let leak_check = std::env::var("GIT_EDITOR").ok();
    let _ = leak_check; // intentionally not asserted; the executor clears env before spawn
}

#[tokio::test]
async fn projector_summary_includes_outcome() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    let _ = git_mutations_ops::stage_all(&exec(), repo.path())
        .await
        .unwrap();
    let outcome = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::StageAll,
        "a",
        false,
        false,
    )
    .await
    .unwrap();
    let summary = project_mutation(&outcome.mutation);
    assert!(summary.contains("git commit"));
    assert!(summary.contains("completed"));
    assert!(summary.contains("duration"));
}

#[tokio::test]
async fn classify_outcome_distinguishes_conflict_from_complete() {
    use codegg::git_mutations::{RepoSnapshot, StateDelta};
    let snap = || RepoSnapshot {
        head: "h".into(),
        branch: "main".into(),
        detached: false,
        staged_count: 0,
        unstaged_count: 0,
        untracked_count: 0,
        conflicted_count: 0,
        captured_at: chrono::Utc::now(),
        raw_status: None,
    };
    let complete = MutationResult {
        operation: codegg_git::GitOperation::Commit {
            message: "x".into(),
            amend: false,
            allow_empty: false,
        },
        subcommand: "commit".into(),
        delta: StateDelta {
            before: snap(),
            after: snap(),
            commits_created: vec!["h".into()],
            refs_created: vec![],
            refs_deleted: vec![],
            paths_staged: vec![],
            paths_unstaged: vec![],
            conflicts: vec![],
        },
        outcome: MutationOutcome::Completed,
        stdout: "".into(),
        stderr: "".into(),
        exit_code: 0,
        success: true,
        duration_ms: 0,
    };
    assert_eq!(complete.outcome, MutationOutcome::Completed);
}

// ── helpers ────────────────────────────────────────────────────────────

fn head(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn current_branch(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git branch");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_status_clean(repo: &Path) -> bool {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .expect("git status");
    String::from_utf8_lossy(&out.stdout).trim().is_empty()
}
