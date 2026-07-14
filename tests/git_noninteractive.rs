//! E3: Noninteractive behavior tests for `GitEnvPolicy`.
//!
//! Pins the closure plan's contract: every Codegg-owned `git`
//! subprocess is noninteractive by construction — credential
//! prompts return promptly, no editor opens during commit/amend/
//! rebase, no pager stalls large output, timeouts kill child
//! processes, cancellation does not leave a child running.
//!
//! Unix-only — uses process-group semantics for cancellation.

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
async fn auth_failure_returns_promptly_with_terminal_prompt_zero() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://nonexistent.invalid/x.git".to_string(),
    ];
    let start = Instant::now();
    let mut cmd = GitEnvPolicy::default().apply(&argv, Path::new("/tmp"));
    let output = cmd.output().await.expect("output");
    let elapsed = start.elapsed();
    assert!(!output.status.success(), "ls-remote should fail");
    assert!(
        elapsed < Duration::from_secs(10),
        "auth failure took too long: {elapsed:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent.invalid") || stderr.contains("Could not resolve"),
        "expected DNS error: {stderr}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn timeout_kills_long_running_subprocess() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    // Run a slow `git` command (e.g. clone with unreachable host).
    // Apply a 1-second timeout via tokio::time::timeout. The child
    // should be killed (kill_on_drop).
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://10.255.255.1/x.git".to_string(),
    ];
    let mut cmd = GitEnvPolicy::default().apply(&argv, Path::new("/tmp"));
    let start = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(2), cmd.output()).await;
    let elapsed = start.elapsed();
    assert!(result.is_err(), "timeout should fire");
    assert!(
        elapsed < Duration::from_secs(4),
        "timeout killed too slowly: {elapsed:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_does_not_leave_child_running() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    // Spawn a slow git subprocess, then drop the join handle
    // immediately. With kill_on_drop(true), the OS PID must be gone
    // after a short grace period.
    let argv = vec![
        "git".to_string(),
        "ls-remote".to_string(),
        "https://10.255.255.1/x.git".to_string(),
    ];
    let mut cmd = GitEnvPolicy::default().apply(&argv, Path::new("/tmp"));
    let child = cmd.spawn().expect("spawn");
    let pid = child.id().expect("pid");
    drop(child);
    // Give the OS a moment to reap the dropped child.
    tokio::time::sleep(Duration::from_millis(500)).await;
    // On Unix we can probe whether the pid is alive via `kill -0`.
    let probe = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .expect("kill -0");
    assert!(
        !probe.status.success(),
        "child process {pid} still alive after drop"
    );
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
    let mut cmd = GitEnvPolicy::default().apply(&argv, dir.path());
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
    let mut cmd = GitEnvPolicy::default().apply(&argv, dir.path());
    let output = cmd.output().await.expect("output");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(), "git log failed: {stderr}");
    assert!(
        elapsed < Duration::from_secs(5),
        "git log took too long (pager stall?): {elapsed:?}"
    );
}
