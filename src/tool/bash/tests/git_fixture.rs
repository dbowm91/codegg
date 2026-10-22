//! A temporary Git repository owned by exactly one test.
//!
//! This fixture lives in a `tests/` directory for a reason that is not
//! stylistic. `src/tool/bash.rs` is a **canonical finite path** in
//! `docs/execution-ownership.toml`, and the execution-ownership guard
//! forbids a direct process spawn anywhere in such a file — including
//! inside an inline `#[cfg(test)] mod tests` block — because that file is
//! where the bounded, typed process execution lives. The fixture needs
//! `git init`, so it cannot live there.
//!
//! A directory named `tests` under `src/` is how the guard classifies a
//! fixture as test-only *by definition*: it skips those trees entirely
//! rather than classifying them line by line. The module is pulled into
//! `src/tool/bash.rs` with an explicit `#[path]`, so the dependency is
//! visible at the use site.
//!
//! The same convention already carries `src/security/workflow/report.rs`,
//! which the manifest declares `test_only` because its `mod tests` block
//! builds Git fixtures the same way.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Initialise a Git repository at `root` with one committed file.
///
/// The repository is left with a single commit and no uncommitted change,
/// so a test that needs a dirty working tree writes its own change.
pub fn init_repo_with_marker(root: &Path, marker: &str) {
    let git = |args: &[&str]| {
        // execution-ownership: test_only
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("tracked.txt"), marker).expect("write fixture file");
    git(&["add", "."]);
    git(&["commit", "-m", "initial"]);
}

/// A temporary Git repository owned by exactly one test.
///
/// The `TempDir` is returned alongside the path so the repository stays
/// alive for the whole test and is removed afterwards.
pub fn temp_git_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp git repo");
    let root = dir.path().to_path_buf();
    init_repo_with_marker(&root, "tracked\n");
    (dir, root)
}
