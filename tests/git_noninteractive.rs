//! E3: Noninteractive behavior tests for `GitEnvPolicy`.
//!
//! Pins the trusted Git contract: safe local operations remain noninteractive,
//! while remote operations report a missing contained transport capability
//! before any child process is created.
//!
//! Unix-only — the child cleanup behavior remains covered by egggit's process tests.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use codegg::git_mutations::GitEnvPolicy;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

#[tokio::test(flavor = "current_thread")]
async fn remote_request_returns_typed_transport_unavailability() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://nonexistent.invalid/x.git".to_string(),
    ];
    assert!(matches!(
        GitEnvPolicy::default().apply(&argv, Path::new("/tmp")),
        Err(egggit::GitExecutionPolicyOutcome::BackendUnavailable { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_remote_request_returns_without_waiting_for_network() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://10.255.255.1/x.git".to_string(),
    ];
    let start = Instant::now();
    let result = GitEnvPolicy::default().apply(&argv, Path::new("/tmp"));
    let elapsed = start.elapsed();
    assert!(matches!(
        result,
        Err(egggit::GitExecutionPolicyOutcome::BackendUnavailable { .. })
    ));
    assert!(
        elapsed < Duration::from_secs(1),
        "policy must resolve before attempting remote I/O: {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_remote_request_cannot_spawn_a_child() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://10.255.255.1/x.git".to_string(),
    ];
    assert!(matches!(
        GitEnvPolicy::default().apply(&argv, Path::new("/tmp")),
        Err(egggit::GitExecutionPolicyOutcome::BackendUnavailable { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn editor_does_not_open_during_commit() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = tempfile::TempDir::new().expect("tempdir");
    // Initialize a repo with a configured marker "editor" that would
    // fail loudly if invoked.
    let sentinel_editor = dir.path().join("sentinel_editor.sh");
    std::fs::write(
        &sentinel_editor,
        "#!/bin/sh\necho EDITOR_INVOKED >&2\nexit 99\n",
    )
    .expect("write");
    let mut perm = std::fs::metadata(&sentinel_editor)
        .expect("meta")
        .permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&sentinel_editor, perm).expect("chmod");

    // Init repo with the sentinel editor as GIT_EDITOR. If our policy
    // doesn't pin GIT_EDITOR=true, the commit would invoke this
    // script (because we don't pass -m).
    let setup = |argv: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(argv)
            .current_dir(dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_EDITOR", &sentinel_editor)
            .env("VISUAL", &sentinel_editor);
        cmd
    };
    setup(&["init", "-q", "-b", "main"]).status().expect("init");
    std::fs::write(dir.path().join("a.txt"), "hello").expect("write");
    setup(&["add", "a.txt"]).status().expect("add");

    // Now run `git commit` (no -m) through the policy. The policy
    // should override GIT_EDITOR=true so the editor does not open.
    let argv = vec!["git".to_string(), "commit".to_string()];
    let mut cmd = GitEnvPolicy::default()
        .apply(&argv, dir.path())
        .expect("trusted non-executable commit");
    let output = cmd.output().await.expect("output");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("EDITOR_INVOKED"),
        "sentinel editor was invoked: {combined}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pager_does_not_stall_large_log_output() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = tempfile::TempDir::new().expect("tempdir");
    let setup = |argv: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(argv)
            .current_dir(dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        cmd
    };
    setup(&["init", "-q", "-b", "main"]).status().expect("init");
    std::fs::write(dir.path().join("a.txt"), "hello").expect("write");
    setup(&["add", "a.txt"]).status().expect("add");
    setup(&["commit", "-q", "-m", "initial"])
        .status()
        .expect("commit");

    // Run `git log` (no --no-pager). Policy pins GIT_PAGER=cat so
    // output streams without a TTY.
    let argv = vec![
        "git".to_string(),
        "log".to_string(),
        "--pretty=oneline".to_string(),
    ];
    let start = Instant::now();
    let mut cmd = GitEnvPolicy::default()
        .apply(&argv, dir.path())
        .expect("trusted non-executable log");
    let output = cmd.output().await.expect("output");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(), "git log failed: {stderr}");
    assert!(
        elapsed < Duration::from_secs(5),
        "git log took too long (pager stall?): {elapsed:?}"
    );
}
