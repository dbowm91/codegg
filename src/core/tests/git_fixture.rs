//! A temporary Git repository owned by exactly one test.
//!
//! This fixture lives in a `tests/` directory for the same reason as
//! `src/tool/bash/tests/git_fixture.rs`: the execution-ownership guard
//! classifies a `tests/` tree as test-only by definition and skips it,
//! whereas a direct process spawn in ordinary source has to be declared in
//! `docs/execution-ownership.toml`. The fixture needs `git init`, so it
//! cannot live in ordinary source.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run one `git` command in `root` and require it to succeed.
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "CodeGG Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "CodeGG Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A temporary Git repository with one commit on `branch`, three staged
/// changes, and one remote.
///
/// The remote matters: the project catalog only records a *repository*
/// identity for a workspace whose remote lineage is uniquely identifiable,
/// so a fixture without one would exercise the fallback resolution path
/// rather than the canonical one.
pub fn temp_git_repo(branch: &str) -> (tempfile::TempDir, PathBuf) {
    temp_git_repo_with_remote(branch, true)
}

/// A temporary Git repository with one commit on `branch` and **no** remote.
///
/// The project catalog records no repository identity for a workspace whose
/// remote lineage is not uniquely identifiable, so this fixture exercises
/// the fallback resolution path.
pub fn temp_git_repo_without_remote(branch: &str) -> (tempfile::TempDir, PathBuf) {
    temp_git_repo_with_remote(branch, false)
}

fn temp_git_repo_with_remote(branch: &str, with_remote: bool) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(root.join("README.md"), "fixture\n").expect("write fixture file");
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "--quiet", "-m", "initial"]);
    if with_remote {
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/org/fixture.git",
            ],
        );
    }
    git(&root, &["checkout", "--quiet", "-b", branch]);
    // Three staged files, so a diff read has more rows than a caller may
    // ask for: the cap is exercised by the fixture, not by a mock.
    for file in ["CHANGELOG.md", "NOTES.md", "TODO.md"] {
        std::fs::write(root.join(file), "changed\n").expect("write changed file");
        git(&root, &["add", file]);
    }
    (dir, root)
}

/// Write one skill package under `<project_root>/.codegg/skills/<name>/`.
pub fn write_project_skill(project_root: &Path, name: &str, description: &str) {
    let package = project_root.join(".codegg").join("skills").join(name);
    std::fs::create_dir_all(&package).expect("create skill package directory");
    let document = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nFixture body.\n"
    );
    std::fs::write(package.join("SKILL.md"), document).expect("write skill document");
}
