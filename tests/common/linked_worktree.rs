//! A worktree creator that produces a **real** linked worktree.
//!
//! The M011 composed two-agent race needs a shared checkout that Git itself
//! recognises as a linked worktree — `git worktree list` has to report it on
//! the recorded branch — because the worktree service validates the checkout
//! it reserved before it hands it to a run. A hand-written `managed_worktree`
//! row would not survive that validation, so the row has to be earned.
//!
//! This fixture earns it by running the same two Git operations the contained
//! child runs: the checkout is created **detached** and the branch is
//! attached afterwards with `git symbolic-ref`. That order matters —
//! `git worktree add <path> <existing-branch>` rewrites the branch ref, which
//! a contained child may not do.
//!
//! What this fixture is **not**: it is not a containment proof. It creates the
//! linked worktree directly, without the sandboxed child and its grant, so it
//! claims nothing about the C008 containment contract. It exists so that a
//! suite can exercise the *shared checkout* mechanics against a checkout that
//! is real.

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use codegg_core::worktree_service::{
    ContainedWorktreeCreator, ContainedWorktreeOutcome, ContainedWorktreeRequest,
    WorktreeServiceError,
};

/// The Git executable the production contained child uses.
fn git_binary() -> std::path::PathBuf {
    codegg::git_contained_worktree::resolve_git_binary().expect("a pinned Git executable")
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new(git_binary())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Creates a real linked worktree at the path the worktree service reserved.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinkedWorktreeCreator;

impl LinkedWorktreeCreator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ContainedWorktreeCreator for LinkedWorktreeCreator {
    async fn create(
        &self,
        request: &ContainedWorktreeRequest,
    ) -> Result<ContainedWorktreeOutcome, WorktreeServiceError> {
        // The branch is the daemon's: it is created before the child runs, so
        // the child attaches to an existing branch.
        run_git(
            &request.repository_root,
            &[
                "branch",
                request.branch.as_str(),
                request.base_commit.as_str(),
            ],
        );

        // The production shape: the worktree is created detached, and the
        // branch is attached afterwards.
        let administration_dir = request.repository_gitdir.join("worktrees").join(
            request
                .worktree_path
                .file_name()
                .expect("worktree name")
                .to_string_lossy()
                .to_string(),
        );
        run_git(
            &request.repository_gitdir,
            &[
                "--git-dir",
                request.repository_gitdir.to_str().expect("gitdir"),
                "worktree",
                "add",
                "--detach",
                request.worktree_path.to_str().expect("worktree path"),
                request.base_commit.as_str(),
            ],
        );
        run_git(
            &administration_dir,
            &[
                "--git-dir",
                administration_dir.to_str().expect("administration dir"),
                "symbolic-ref",
                "HEAD",
                &format!("refs/heads/{}", request.branch),
            ],
        );

        Ok(ContainedWorktreeOutcome {
            administration_dir,
            backend: "fixture-linked-worktree".into(),
            abi: None,
            // This fixture runs Git directly; it has no sandbox of its own and
            // therefore enforces no network policy. The containment contract
            // is C008's, proved by its own suite.
            network_enforced: false,
        })
    }
}
