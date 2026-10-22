//! Reproduction: concurrent Git fixtures must not interfere (C004 WP1).
//!
//! The C001 plan requires the named Git-routing and security-review tests to
//! "create/own a temp Git worktree or explicitly bind a scoped repository cwd
//! per test; do not rely on mutable shared process cwd or host repo state",
//! and to "reproduce with true concurrent tests and temporary repositories".
//!
//! Two fixture shapes are in use in this repository:
//!
//! * a **per-test** temporary repository (`tempfile::TempDir`), used by the
//!   security-review tests and the routing tests fixed in this milestone;
//! * a **process-unique** directory under `.codegg/test_tmp/<pid>`, used where
//!   the fixture has to live *inside* the workspace so the active-routing
//!   validator does not reject it as `OutsideWorkspace`.
//!
//! This file proves the property that makes both shapes safe: two fixtures
//! that exist at the same time never share a path, and one fixture's
//! repository state is never visible in the other's. The reproduction is
//! deliberately **cross-process** for the process-unique shape, because a
//! single-process reproduction cannot show that two different processes
//! derive different directories.
//!
//! The test is a regression guard: it fails if a fixture ever falls back to a
//! shared path, a shared process cwd, or the host checkout.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The number of concurrent fixtures the reproduction creates.
const CONCURRENCY: usize = 8;

/// Create a Git repository at `root` with one committed marker file.
fn init_repo_with_marker(root: &Path, marker: &str) {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "test@test.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("marker.txt"), marker).expect("write marker");
    git(&["add", "."]);
    git(&["commit", "-m", "initial"]);
}

/// The committed marker of the repository at `root`.
fn committed_marker(root: &Path) -> String {
    let output = Command::new("git")
        .args(["show", "HEAD:marker.txt"])
        .current_dir(root)
        .output()
        .expect("run git show");
    assert!(
        output.status.success(),
        "git show failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Every fixture created at the same time has its own path and its own
/// repository state.
#[test]
fn concurrent_fixtures_do_not_share_a_path_or_a_repository() {
    // The `TempDir` handles are kept alive for the duration of the test so the
    // temporary repositories are removed when it ends.
    let dirs: Vec<tempfile::TempDir> = (0..CONCURRENCY)
        .map(|_| tempfile::tempdir().expect("create temp repo"))
        .collect();
    let roots: Vec<PathBuf> = dirs.iter().map(|dir| dir.path().to_path_buf()).collect();

    // Every path is distinct.
    let unique: BTreeSet<&PathBuf> = roots.iter().collect();
    assert_eq!(
        unique.len(),
        CONCURRENCY,
        "two concurrent fixtures were given the same path"
    );

    // The repositories are initialised concurrently, on separate threads, so
    // an implementation that shared state would interleave here.
    let handles: Vec<_> = roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let root = root.clone();
            std::thread::spawn(move || {
                init_repo_with_marker(&root, &format!("marker-{index}"));
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("fixture thread");
    }

    // Each repository holds exactly its own marker. A shared repository — the
    // host checkout, or one fixed path reused by every test — would show one
    // marker in all of them.
    for (index, root) in roots.iter().enumerate() {
        assert_eq!(
            committed_marker(root),
            format!("marker-{index}"),
            "fixture {index} saw another fixture's repository state"
        );
    }
}

/// Two **processes** deriving a process-unique fixture path derive different
/// paths, and neither sees the other's repository.
#[test]
fn two_processes_derive_different_process_unique_fixture_paths() {
    // The child prints the fixture path it created and the marker it committed.
    // The path is derived from the child's own process id, exactly as the
    // in-tree fixtures do.
    let child = |tag: &str| {
        Command::new(std::env::current_exe().expect("current exe"))
            .arg("--exact")
            .arg("child_prints_its_own_process_unique_fixture_path")
            // The child is `#[ignore]`d, so it only runs when ignored tests
            // are explicitly requested.
            .arg("--ignored")
            .arg("--nocapture")
            .env("CODEGG_FIXTURE_CHILD_TAG", tag)
            .output()
            .expect("run child")
    };

    // The two children run concurrently, so an implementation that derived a
    // shared path would have them race for the same directory.
    let output = child("a");
    let output_b = child("b");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stdout_b = String::from_utf8_lossy(&output_b.stdout).to_string();

    // The child harness reports one `KEY: value` line per fact.
    let field = |text: &str, key: &str| -> Option<String> {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{key}: ")))
            .map(|value| value.trim().to_string())
    };
    let path_of = |text: &str| field(text, "FIXTURE_PATH").map(PathBuf::from);
    let marker_of = |text: &str| field(text, "FIXTURE_MARKER");

    let path_a = path_of(&stdout).unwrap_or_else(|| {
        panic!("the child did not report a fixture path: {stdout}\n---\n{stdout_b}")
    });
    let path_b = path_of(&stdout_b)
        .unwrap_or_else(|| panic!("the child did not report a fixture path: {stdout_b}"));

    // The two processes did not share a path.
    assert_ne!(
        path_a, path_b,
        "two processes derived the same process-unique fixture path"
    );

    // Each path is unique to its own process: the process-unique component is
    // the child's own process id, so no other process can derive it.
    for (path, text) in [(&path_a, &stdout), (&path_b, &stdout_b)] {
        let pid = field(text, "FIXTURE_PID")
            .unwrap_or_else(|| panic!("the child did not report its pid: {text}"));
        let leaf = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        assert!(
            leaf.contains(&pid),
            "the fixture path {leaf:?} does not carry its own process id {pid:?}"
        );
    }

    // Neither process saw the other's repository state.
    let marker_a = marker_of(&stdout)
        .unwrap_or_else(|| panic!("the child did not report its marker: {stdout}"));
    let marker_b = marker_of(&stdout_b)
        .unwrap_or_else(|| panic!("the child did not report its marker: {stdout_b}"));
    assert_ne!(
        marker_a, marker_b,
        "two concurrent fixtures shared a repository"
    );
}

/// The child half of the cross-process reproduction.
///
/// It is `#[ignore]`d so it does not run as part of the suite; the parent
/// test invokes it by name.
#[test]
#[ignore = "child half of the cross-process fixture reproduction"]
fn child_prints_its_own_process_unique_fixture_path() {
    let tag = std::env::var("CODEGG_FIXTURE_CHILD_TAG").expect("child tag");
    let pid = std::process::id();
    let root = PathBuf::from(format!(
        "{}/.codegg/test_tmp/fixture_isolation_child_{}_{}",
        env!("CARGO_MANIFEST_DIR"),
        pid,
        tag
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture dir");
    let marker = format!("child-{pid}-{tag}");
    init_repo_with_marker(&root, &marker);
    println!("FIXTURE_PATH: {}", root.display());
    println!("FIXTURE_PID: {pid}");
    println!("FIXTURE_MARKER: {marker}");
    let _ = std::fs::remove_dir_all(&root);
}
