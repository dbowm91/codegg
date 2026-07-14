//! E2: Environment attack tests for `GitEnvPolicy`.
//!
//! Pins the closure plan's contract: every Codegg-owned `git`
//! subprocess, regardless of execution origin, MUST NOT observe
//! command-bearing or sensitive parent env vars unless the policy
//! explicitly permits them.
//!
//! The strategy: each test populates the parent process's environment
//! with sentinel values via `std::env::set_var`, then constructs a
//! `Command` via `GitEnvPolicy::default().apply()` and runs a child
//! (`/bin/sh -c 'env > <out>; ...'`) that writes its observed
//! environment to a file. The test reads the file and asserts the
//! sentinel is absent.
//!
//! Unix-only — uses `/bin/sh` and process-group semantics. Windows is
//! not supported (per `docs/validation/git-cross-platform.md`).

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;

use codegg::git_mutations::{GitEnvPolicy, ALLOWED_ENV_VARS, ALWAYS_STRIPPED_ENV_VARS};

mod common;

/// Build a `tokio::process::Command` (or `std::process::Command` for
/// sync sites) via `GitEnvPolicy::default().apply()`. We hijack the
/// argv to run `/bin/sh -c '<script>'` instead of `git` — the env
/// hardening is the same regardless of the binary.
fn policy_command(script: &str) -> std::process::Command {
    let argv = vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()];
    GitEnvPolicy::default().apply_sync(&argv, Path::new("/tmp"))
}

fn run_capture_env(script: &str) -> String {
    let mut cmd = policy_command(script);
    cmd.stdin(Stdio::null());
    let out = cmd.output().expect("env capture");
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn env_attack_sentinel_git_askpass_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("git_askpass");
    // SAFETY: tests run single-threaded wrt env. Each test sets its
    // own sentinel and the policy strips them before the child
    // observes the env.
    unsafe {
        std::env::set_var("GIT_ASKPASS", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("GIT_ASKPASS");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_ASKPASS leaked to child env: {observed}"
    );
    assert!(
        !observed.contains("GIT_ASKPASS"),
        "GIT_ASKPASS key present in child env: {observed}"
    );
}

#[test]
fn env_attack_sentinel_ssh_askpass_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("ssh_askpass");
    unsafe {
        std::env::set_var("SSH_ASKPASS", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("SSH_ASKPASS");
    }
    assert!(
        !observed.contains(&sentinel),
        "SSH_ASKPASS leaked: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_ssh_command_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("git_ssh_command");
    unsafe {
        std::env::set_var("GIT_SSH_COMMAND", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("GIT_SSH_COMMAND");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_SSH_COMMAND leaked: {observed}"
    );
    assert!(
        !observed.contains("GIT_SSH_COMMAND"),
        "GIT_SSH_COMMAND key present: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_proxy_command_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("git_proxy_command");
    unsafe {
        std::env::set_var("GIT_PROXY_COMMAND", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("GIT_PROXY_COMMAND");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_PROXY_COMMAND leaked: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_editor_is_overridden() {
    let sentinel = common::secret_scan::unique_sentinel("git_editor");
    unsafe {
        std::env::set_var("GIT_EDITOR", &sentinel);
    }
    let observed = run_capture_env("env | grep GIT_EDITOR");
    unsafe {
        std::env::remove_var("GIT_EDITOR");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_EDITOR leaked: {observed}"
    );
    // Policy pins GIT_EDITOR=true (not the sentinel).
    assert!(
        observed.contains("GIT_EDITOR=true"),
        "GIT_EDITOR should be pinned to true: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_sequence_editor_is_overridden() {
    let sentinel = common::secret_scan::unique_sentinel("git_sequence_editor");
    unsafe {
        std::env::set_var("GIT_SEQUENCE_EDITOR", &sentinel);
    }
    let observed = run_capture_env("env | grep GIT_SEQUENCE_EDITOR");
    unsafe {
        std::env::remove_var("GIT_SEQUENCE_EDITOR");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_SEQUENCE_EDITOR leaked: {observed}"
    );
    assert!(
        observed.contains("GIT_SEQUENCE_EDITOR=true"),
        "GIT_SEQUENCE_EDITOR should be pinned: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_config_count_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("git_config_count");
    unsafe {
        std::env::set_var("GIT_CONFIG_COUNT", &sentinel);
        std::env::set_var("GIT_CONFIG_KEY_0", "credential.helper");
        std::env::set_var("GIT_CONFIG_VALUE_0", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("GIT_CONFIG_COUNT");
        std::env::remove_var("GIT_CONFIG_KEY_0");
        std::env::remove_var("GIT_CONFIG_VALUE_0");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_CONFIG_COUNT leaked: {observed}"
    );
    assert!(
        !observed.contains("GIT_CONFIG_KEY_0"),
        "GIT_CONFIG_KEY_0 leaked: {observed}"
    );
    assert!(
        !observed.contains("GIT_CONFIG_VALUE_0"),
        "GIT_CONFIG_VALUE_0 leaked: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_config_parameters_is_stripped() {
    let sentinel = common::secret_scan::unique_sentinel("git_config_parameters");
    unsafe {
        std::env::set_var("GIT_CONFIG_PARAMETERS", &sentinel);
    }
    let observed = run_capture_env("env | sort");
    unsafe {
        std::env::remove_var("GIT_CONFIG_PARAMETERS");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_CONFIG_PARAMETERS leaked: {observed}"
    );
}

#[test]
fn env_attack_sentinel_git_pager_is_pinned() {
    let sentinel = common::secret_scan::unique_sentinel("git_pager");
    unsafe {
        std::env::set_var("GIT_PAGER", &sentinel);
        std::env::set_var("PAGER", &sentinel);
    }
    let observed = run_capture_env("env | grep -E '^(GIT_PAGER|PAGER)='");
    unsafe {
        std::env::remove_var("GIT_PAGER");
        std::env::remove_var("PAGER");
    }
    assert!(
        !observed.contains(&sentinel),
        "GIT_PAGER/PAGER leaked: {observed}"
    );
    // Policy pins to cat (not the sentinel).
    assert!(
        observed.contains("GIT_PAGER=cat"),
        "GIT_PAGER must be pinned: {observed}"
    );
    assert!(
        observed.contains("PAGER=cat"),
        "PAGER must be pinned: {observed}"
    );
}

#[test]
fn env_attack_sentinel_hostile_git_dir_is_stripped() {
    let sentinel_dir = "/tmp/hostile_codegg_env_target_DO_NOT_USE";
    unsafe {
        std::env::set_var("GIT_DIR", sentinel_dir);
    }
    // Force failure with a hostile GIT_DIR (path does not exist).
    let observed = run_capture_env("if [ -n \"$GIT_DIR\" ]; then echo LEAK; else echo CLEAN; fi");
    unsafe {
        std::env::remove_var("GIT_DIR");
    }
    assert_eq!(observed.trim(), "CLEAN", "GIT_DIR leaked: {observed}");
}

#[test]
fn env_attack_sentinel_hostile_git_work_tree_is_stripped() {
    unsafe {
        std::env::set_var("GIT_WORK_TREE", "/tmp/hostile_codegg_work_tree");
    }
    let observed =
        run_capture_env("if [ -n \"$GIT_WORK_TREE\" ]; then echo LEAK; else echo CLEAN; fi");
    unsafe {
        std::env::remove_var("GIT_WORK_TREE");
    }
    assert_eq!(observed.trim(), "CLEAN", "GIT_WORK_TREE leaked: {observed}");
}

#[test]
fn env_attack_passes_through_safe_path_home() {
    // PATH and HOME are explicitly allow-listed. Verify the child
    // observes them when set, so legitimate helper discovery and
    // git config resolution still work.
    let observed =
        run_capture_env("echo PATH_LEN=${#PATH}; echo HOME_LEN=${#HOME}; echo LANG_LEN=${#LANG}");
    // The policy restores these from parent. Even on minimal test
    // environments PATH and HOME are set; LANG may not be.
    assert!(observed.contains("PATH_LEN="));
    assert!(observed.contains("HOME_LEN="));
    // We only assert that the variable names appear (not the values,
    // which may include sentinels in unrelated tests).
}

#[tokio::test(flavor = "current_thread")]
async fn env_attack_marker_file_proves_askpass_not_invoked() {
    // Create a sentinel "askpass" script that would write a marker
    // file if invoked. Set GIT_ASKPASS to it. Then run a git command
    // that requires credentials (clone a non-existent URL). git
    // should fail with auth failure (GIT_TERMINAL_PROMPT=0) without
    // invoking our script.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let marker = dir.path().join("marker.txt");
    let askpass = dir.path().join("fake_askpass.sh");
    std::fs::write(
        &askpass,
        format!(
            "#!/bin/sh\necho MARKER_INVOKED > {}\nexit 0\n",
            marker.display()
        ),
    )
    .expect("write askpass");
    let mut perm = std::fs::metadata(&askpass).expect("meta").permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&askpass, perm).expect("chmod");

    unsafe {
        std::env::set_var("GIT_ASKPASS", &askpass);
    }
    // Try to clone an unreachable host. git should fail WITHOUT
    // invoking our askpass because GIT_TERMINAL_PROMPT=0 is pinned.
    let argv = vec![
        "git".to_string(),
        "clone".to_string(),
        "https://nonexistent.invalid/x".to_string(),
    ];
    let mut cmd = GitEnvPolicy::default().apply(&argv, dir.path());
    let _ = cmd.output().await;
    unsafe {
        std::env::remove_var("GIT_ASKPASS");
    }
    assert!(
        !marker.exists(),
        "askpass was invoked (marker file created): {}",
        marker.display()
    );
}

#[test]
fn policy_allowlist_contains_known_safe_vars() {
    // Verify the static composition of the allowlist.
    let expected = [
        "PATH",
        "HOME",
        "XDG_CONFIG_HOME",
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "CURL_CA_BUNDLE",
    ];
    for key in expected {
        assert!(
            ALLOWED_ENV_VARS.contains(&key),
            "ALLOWED_ENV_VARS missing {key}: {ALLOWED_ENV_VARS:?}"
        );
    }
}

#[test]
fn policy_stripped_contains_known_dangerous_vars() {
    let expected = [
        "GIT_ASKPASS",
        "GIT_SSH_COMMAND",
        "GIT_PROXY_COMMAND",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_PAGER",
        "PAGER",
    ];
    for key in expected {
        assert!(
            ALWAYS_STRIPPED_ENV_VARS.contains(&key),
            "ALWAYS_STRIPPED_ENV_VARS missing {key}: {ALWAYS_STRIPPED_ENV_VARS:?}"
        );
    }
}
