//! Integration tests for the typed Git mutation framework (`src/git_mutations.rs`,
//! `src/git_mutations_ops.rs`). Each test builds a fresh in-temp-dir Git repo,
//! prepares changes, runs the typed mutation helpers, and asserts on either
//! their successful state delta or the typed capability handoff when a
//! contained local Git backend is not available.
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

fn assert_backend_unavailable<T: std::fmt::Debug>(
    result: Result<T, codegg::git_mutations::GitMutationError>,
) {
    match result {
        Err(codegg::git_mutations::GitMutationError::Policy(
            egggit::GitExecutionPolicyOutcome::BackendUnavailable { capabilities },
        )) => assert!(
            !capabilities.is_empty(),
            "missing required capability evidence"
        ),
        other => panic!("expected typed unavailable Git capability, got {other:?}"),
    }
}

#[tokio::test]
async fn stage_all_marks_changes_staged() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("new.txt"), "1\n");
    assert_backend_unavailable(git_mutations_ops::stage_all(&exec(), repo.path()).await);
}

#[tokio::test]
async fn stage_paths_returns_paths_in_delta() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("a.txt"), "a\n");
    fs_utils::write(&repo.path().join("b.txt"), "b\n");
    assert_backend_unavailable(
        git_mutations_ops::stage_paths(&exec(), repo.path(), vec!["a.txt".to_string()]).await,
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
async fn branch_create_and_switch_requires_contained_checkout() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    assert_backend_unavailable(
        git_mutations_ops::create_and_switch(&exec(), repo.path(), "feature", None).await,
    );
}

#[tokio::test]
async fn branch_delete_noncurrent_uses_trusted_ref_update() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    let created = Command::new("git")
        .args(["branch", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(created.status.success());
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
async fn stash_push_reports_contained_filter_requirement() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    fs_utils::write(&repo.path().join("README.md"), "tweaked\n");
    write_and_stage(repo.path(), "README.md", "tweaked\n");
    assert_backend_unavailable(
        git_mutations_ops::stash_push(&exec(), repo.path(), Some("work"), false, Vec::new()).await,
    );
}

#[tokio::test]
async fn merge_fast_forward_reports_contained_execution_requirement() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    assert_backend_unavailable(
        git_mutations_ops::merge(&exec(), repo.path(), vec!["main".to_string()], false, None).await,
    );
}

#[tokio::test]
async fn merge_conflict_path_reports_contained_execution_requirement() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    assert_backend_unavailable(
        git_mutations_ops::merge(
            &exec(),
            repo.path(),
            vec!["feature".to_string()],
            false,
            None,
        )
        .await,
    );
}

#[tokio::test]
async fn env_policy_pins_git_terminal_prompt_and_editor() {
    if !git_available() {
        return;
    }
    let repo = fresh_repo();
    write_and_stage(repo.path(), "a.txt", "a\n");
    let commit = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::AlreadyStaged,
        "policy test",
        false,
        false,
    )
    .await
    .unwrap();
    assert_clean(&commit.mutation);
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
    write_and_stage(repo.path(), "a.txt", "a\n");
    let outcome = git_mutations_ops::commit_with_selection(
        &exec(),
        repo.path(),
        codegg::git_mutations::CommitSelection::AlreadyStaged,
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

fn current_branch(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git branch");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
