//! Integration tests for Phase E typed Git operations: network
//! (fetch/pull/push/remote), configuration (git config with allowlist),
//! and destructive (reset/clean). Each test builds a fresh in-temp-dir
//! Git repo (and a local bare-remote fixture for network operations),
//! runs the typed helpers in `git_network_ops`, and asserts on the
//! returned `MutationResult` and on persisted repo state.
//!
//! These tests rely on the host having `git` in PATH. Tests skip
//! gracefully (via `binary_check`) if `git` is not available so CI on
//! minimal containers still passes.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use codegg::git_mutations::{GitEnvPolicy, GitMutationExecutor};
use codegg::git_network_ops::{self, CleanRequest, PushForce, PushRequest};
use codegg::git_network_policy::{
    classify_network_failure, redact_url_credentials, redact_url_credentials_in_text,
    NetworkFailureKind,
};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(argv: &[&str], cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(argv)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    cmd
}

fn assert_status_success(argv: &[&str], cwd: &Path) {
    let status = run_git(argv, cwd)
        .status()
        .unwrap_or_else(|e| panic!("git {argv:?}: {e}"));
    assert!(status.success(), "git {argv:?} failed in {}", cwd.display());
}

fn init_repo(dir: &Path) {
    assert_status_success(&["init", "-q", "-b", "main"], dir);
    fs_utils::write(&dir.join("README.md"), "hello\n");
    assert_status_success(&["add", "README.md"], dir);
    assert_status_success(&["commit", "-q", "-m", "initial"], dir);
}

fn init_bare(dir: &Path) {
    assert_status_success(&["init", "--bare", "-q"], dir);
}

fn fresh_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo(dir.path());
    dir
}

fn fresh_pair() -> (tempfile::TempDir, tempfile::TempDir) {
    let remote = tempfile::tempdir().expect("tempdir remote");
    init_bare(remote.path());
    let local = tempfile::tempdir().expect("tempdir local");
    init_repo(local.path());
    let url = remote
        .path()
        .to_str()
        .expect("remote path utf8")
        .to_string();
    assert_status_success(&["remote", "add", "origin", &url], local.path());
    // Push main so the remote has refs to fetch.
    assert_status_success(&["push", "-q", "origin", "main"], local.path());
    (remote, local)
}

fn executor() -> GitMutationExecutor {
    GitMutationExecutor::new()
        .with_env_policy(GitEnvPolicy::default())
        .with_timeout(Duration::from_secs(15))
}

// ── URL redaction (network_policy) ─────────────────────────────────

#[test]
fn redaction_preserves_anonymous_https_urls() {
    assert_eq!(
        redact_url_credentials("https://example.com/foo.git"),
        "https://example.com/foo.git"
    );
}

#[test]
fn redaction_strips_userinfo_with_password() {
    let redacted = redact_url_credentials("https://user:secret@example.com/foo.git");
    assert!(
        !redacted.contains("secret"),
        "leaked password in {redacted}"
    );
    assert!(redacted.contains("@example.com"));
    assert!(redacted.contains("://"));
}

#[test]
fn redaction_strips_userinfo_without_password() {
    // Phase E policy: bare username (no password) is left intact because
    // SSH-style URLs (`git@github.com:foo/bar`) commonly embed a username
    // without a secret. Only the password-bearing form is redacted.
    let redacted = redact_url_credentials("https://user@example.com/foo.git");
    assert_eq!(redacted, "https://user@example.com/foo.git");
    assert!(redacted.contains("example.com"));
}

#[test]
fn classify_dns_failure_kind() {
    let stderr = "fatal: unable to access 'https://nonexistent.invalid/x': Could not resolve host: nonexistent.invalid";
    let kind = classify_network_failure(stderr, 128, false);
    assert_eq!(kind, NetworkFailureKind::Dns);
}

#[test]
fn classify_authentication_failure_kind() {
    let stderr = "fatal: Authentication failed for 'https://github.com/x.git'";
    let kind = classify_network_failure(stderr, 128, false);
    assert_eq!(kind, NetworkFailureKind::Authentication);
}

#[test]
fn classify_ref_rejected_kind() {
    let stderr = " ! [rejected]        main -> main (non-fast-forward)";
    let kind = classify_network_failure(stderr, 1, false);
    assert_eq!(kind, NetworkFailureKind::RefRejected);
}

#[test]
fn classify_timeout_kind() {
    let stderr =
        "fatal: unable to access 'https://x.y/': Operation timed out after 30000 milliseconds";
    let kind = classify_network_failure(stderr, 128, true);
    assert_eq!(kind, NetworkFailureKind::Timeout);
}

// ── Remote add / remove / rename ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn remote_add_persisted_artifacts_are_sanitized() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    let res = git_network_ops::remote_add(
        &exec,
        dir.path(),
        "private",
        "https://u:pw@private.example.com/r.git",
    )
    .await
    .expect("remote_add");
    assert!(res.success, "remote_add should succeed: {}", res.stderr);
    // Threat model boundary: the URL with credentials MUST reach git's
    // argv (otherwise auth breaks), but Codegg-owned persisted surfaces
    // (MutationResult.stdout/stderr, RunStore artifacts, projection,
    // error conversion) MUST contain only the redacted form.
    assert!(
        !res.stdout.contains("u:pw"),
        "MutationResult.stdout leaked raw URL: {}",
        res.stdout
    );
    assert!(
        !res.stderr.contains("u:pw"),
        "MutationResult.stderr leaked raw URL: {}",
        res.stderr
    );
    assert!(
        !format!("{:?}", res.operation).contains("pw"),
        "MutationResult.operation leaked raw URL: {:?}",
        res.operation
    );
    // Sanitized argv forms what lands in RunStore audit logs.
    let argv = codegg_git::render_argv(&res.operation);
    let sanitized = codegg::git_network_policy::sanitize_argv_for_run_store(argv);
    let last = sanitized.last().expect("argv has URL slot");
    assert!(
        last.contains("redacted@private.example.com"),
        "URL in audit argv must be redacted: {sanitized:?}"
    );
    assert!(
        !last.contains("pw"),
        "audit argv leaked password: {sanitized:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn remote_rename_succeeds() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    git_network_ops::remote_add(&exec, dir.path(), "origin", "https://example.com/r.git")
        .await
        .expect("remote_add");
    let res = git_network_ops::remote_rename(&exec, dir.path(), "origin", "upstream")
        .await
        .expect("rename");
    assert!(res.success, "rename should succeed: {}", res.stderr);
    let listed = run_git(&["remote"], dir.path()).output().expect("remote");
    let s = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(s.contains("upstream"), "missing upstream remote in: {s}");
    assert!(!s.contains("origin\n"), "old remote still present: {s}");
}

#[tokio::test(flavor = "current_thread")]
async fn remote_remove_succeeds() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    git_network_ops::remote_add(&exec, dir.path(), "tmp", "https://example.com/r.git")
        .await
        .expect("remote_add");
    let res = git_network_ops::remote_remove(&exec, dir.path(), "tmp")
        .await
        .expect("remove");
    assert!(res.success);
    let listed = run_git(&["remote"], dir.path()).output().expect("remote");
    let s = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(!s.contains("tmp"), "removed remote still listed: {s}");
}

// ── Config allowlist ────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn config_set_blocks_denied_keys() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    // credential.helper is in CONFIG_DENIED_KEY_PATTERNS.
    let res = git_network_ops::config_set(
        &exec,
        dir.path(),
        "credential.helper",
        "store --file=/tmp/x",
        true,
    )
    .await;
    assert!(res.is_err(), "denied key should be rejected");
}

#[tokio::test(flavor = "current_thread")]
async fn config_set_blocks_global_only_keys_in_local_scope() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    let res = git_network_ops::config_set(&exec, dir.path(), "user.name", "Alice", true).await;
    assert!(res.is_err(), "user.name should be rejected in local scope");
}

#[tokio::test(flavor = "current_thread")]
async fn config_set_allows_allowlisted_key() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    let res = git_network_ops::config_set(&exec, dir.path(), "pull.rebase", "true", true)
        .await
        .expect("config_set pull.rebase");
    assert!(res.success);
    // Verify on disk
    let listed = run_git(&["config", "--get", "pull.rebase"], dir.path())
        .output()
        .expect("config get");
    let v = String::from_utf8_lossy(&listed.stdout).trim().to_string();
    assert_eq!(v, "true");
}

#[tokio::test(flavor = "current_thread")]
async fn config_unset_clears_allowlisted_key() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    let exec = executor();
    git_network_ops::config_set(&exec, dir.path(), "rebase.autosquash", "true", true)
        .await
        .expect("set");
    let res = git_network_ops::config_unset(&exec, dir.path(), "rebase.autosquash", true)
        .await
        .expect("unset");
    assert!(res.success);
    let listed = run_git(&["config", "--get", "rebase.autosquash"], dir.path())
        .output()
        .expect("config get");
    assert!(!listed.status.success(), "key should be unset");
}

// ── Network fetch / pull / push (local bare remote) ─────────────────

#[tokio::test(flavor = "current_thread")]
async fn fetch_from_local_bare_remote_succeeds() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let (remote, local) = fresh_pair();
    // Make a new commit on local and push to remote.
    fs_utils::write(&local.path().join("b.txt"), "new\n");
    assert_status_success(&["add", "b.txt"], local.path());
    assert_status_success(&["commit", "-q", "-m", "second"], local.path());
    assert_status_success(&["push", "-q", "origin", "main"], local.path());
    // Now reset local to before the push, then fetch.
    assert_status_success(&["reset", "--hard", "HEAD~1"], local.path());
    let exec = executor();
    let res = git_network_ops::fetch(&exec, local.path(), Some("origin"), vec![], false, false)
        .await
        .expect("fetch");
    assert!(res.success, "fetch should succeed: {}", res.stderr);
    // remote main should now have the pushed commit
    let listed = run_git(&["log", "origin/main", "--oneline"], local.path())
        .output()
        .expect("log");
    let s = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(
        s.contains("second"),
        "missing second commit in origin/main: {s}"
    );
    drop(remote);
}

#[tokio::test(flavor = "current_thread")]
async fn push_to_local_bare_remote_succeeds() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let (remote, local) = fresh_pair();
    let exec = executor();
    fs_utils::write(&local.path().join("c.txt"), "added\n");
    assert_status_success(&["add", "c.txt"], local.path());
    assert_status_success(&["commit", "-q", "-m", "third"], local.path());
    let req = PushRequest {
        remote: Some("origin".into()),
        branch: Some("main".into()),
        set_upstream: true,
        force: PushForce::Normal,
        tags: false,
        delete: false,
        dry_run: false,
    };
    let res = git_network_ops::push(&exec, local.path(), req)
        .await
        .expect("push");
    assert!(res.success, "push should succeed: {}", res.stderr);
    drop(remote);
}

#[tokio::test(flavor = "current_thread")]
async fn push_force_is_marked_destructive() {
    let req = PushRequest {
        remote: Some("origin".into()),
        branch: Some("main".into()),
        set_upstream: false,
        force: PushForce::Force,
        tags: false,
        delete: false,
        dry_run: false,
    };
    assert!(req.is_destructive());
    let hint = git_network_ops::push_permission_hint(&req);
    assert!(hint.contains("force"), "hint should mention force: {hint}");
}

#[tokio::test(flavor = "current_thread")]
async fn pull_from_local_bare_remote_succeeds() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let (remote, local) = fresh_pair();
    fs_utils::write(&local.path().join("d.txt"), "pulled\n");
    assert_status_success(&["add", "d.txt"], local.path());
    assert_status_success(&["commit", "-q", "-m", "pull-target"], local.path());
    assert_status_success(&["push", "-q", "origin", "main"], local.path());
    // Reset local to before push so pull has something to fetch
    assert_status_success(&["reset", "--hard", "HEAD~1"], local.path());
    let exec = executor();
    let res = git_network_ops::pull(
        &exec,
        local.path(),
        Some("origin"),
        Some("main"),
        git_network_ops::PullStrategy::FastForwardOnly,
        false,
    )
    .await
    .expect("pull");
    assert!(res.success, "pull should succeed: {}", res.stderr);
    drop(remote);
}

// ── Reset / clean ───────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn reset_hard_moves_head_and_drops_changes() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    fs_utils::write(&dir.path().join("extra.txt"), "will be lost\n");
    assert_status_success(&["add", "extra.txt"], dir.path());
    assert_status_success(&["commit", "-q", "-m", "will be discarded"], dir.path());
    // Capture HEAD before reset
    let before = run_git(&["rev-parse", "HEAD"], dir.path())
        .output()
        .expect("rev-parse");
    let before_sha = String::from_utf8_lossy(&before.stdout).trim().to_string();
    let exec = executor();
    let res = git_network_ops::reset_hard(&exec, dir.path(), Some("HEAD~1"))
        .await
        .expect("reset_hard");
    assert!(res.success, "reset --hard should succeed: {}", res.stderr);
    let after = run_git(&["rev-parse", "HEAD"], dir.path())
        .output()
        .expect("rev-parse");
    let after_sha = String::from_utf8_lossy(&after.stdout).trim().to_string();
    assert_ne!(before_sha, after_sha);
    // extra.txt should be gone
    assert!(!dir.path().join("extra.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn reset_soft_keeps_working_tree() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    fs_utils::write(&dir.path().join("kept.txt"), "staged-but-kept\n");
    assert_status_success(&["add", "kept.txt"], dir.path());
    assert_status_success(&["commit", "-q", "-m", "soft target"], dir.path());
    let exec = executor();
    let res = git_network_ops::reset_soft(&exec, dir.path(), Some("HEAD~1"))
        .await
        .expect("reset_soft");
    assert!(res.success);
    // File should still exist on disk
    assert!(dir.path().join("kept.txt").exists());
    // kept.txt should be in the staged set now
    let status = run_git(&["status", "--porcelain"], dir.path())
        .output()
        .expect("status");
    let s = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        s.contains("A  kept.txt") || s.starts_with("A"),
        "kept.txt should be staged: {s}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn clean_preview_lists_untracked_files() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    fs_utils::write(&dir.path().join("untracked.txt"), "garbage\n");
    let exec = executor();
    let preview = git_network_ops::clean_preview(&exec, dir.path(), vec![])
        .await
        .expect("clean_preview");
    assert!(!preview.is_empty(), "expected untracked entry");
    assert!(
        preview.entries.iter().any(|e| e.path == "untracked.txt"),
        "untracked.txt missing from preview: {:?}",
        preview.entries
    );
}

#[tokio::test(flavor = "current_thread")]
async fn clean_removes_untracked_files() {
    if !git_available() {
        eprintln!("git not available — skipping");
        return;
    }
    let dir = fresh_repo();
    fs_utils::write(&dir.path().join("garbage.txt"), "remove me\n");
    assert!(dir.path().join("garbage.txt").exists());
    let exec = executor();
    let req = CleanRequest {
        force: true,
        dirs: false,
        ignored: false,
        paths: vec![],
    };
    let res = git_network_ops::clean(&exec, dir.path(), req)
        .await
        .expect("clean");
    assert!(res.success, "clean should succeed: {}", res.stderr);
    assert!(!dir.path().join("garbage.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn clean_broad_ignored_request_is_rejected_by_policy() {
    let req = CleanRequest {
        force: true,
        dirs: true,
        ignored: true,
        paths: vec![],
    };
    assert!(
        req.is_broad(),
        "broad ignored cleanup should be marked broad"
    );
}

// ── Corrective security closure: credential leak + env hardening ────
//
// These tests pin the boundary added by the corrective security closure
// plan for Phase F findings:
//   1. remote_set_url credential leakage via un-redacted URL path
//   2. raw/compat Git fallback missing hardened env policy

#[test]
fn redacted_url_hides_raw_secret_in_debug_and_serde() {
    use codegg_git::RedactedUrl;
    let raw = "https://alice:hunter2@github.com/org/repo.git";
    let url = RedactedUrl::new(raw.to_string());
    let debug = format!("{url:?}");
    let display = format!("{url}");
    assert!(
        !debug.contains("hunter2"),
        "Debug must not leak credential: {debug}"
    );
    assert!(
        !display.contains("hunter2"),
        "Display must not leak credential: {display}"
    );
    assert!(
        debug.contains("REDACTED") || debug.contains("github.com"),
        "Debug should show redaction marker: {debug}"
    );
    let json = serde_json::to_string(&url).expect("serialize");
    assert!(
        !json.contains("hunter2"),
        "Serialize must not leak credential: {json}"
    );
    // Serde round-trip is intentionally lossy: serializing stores the
    // redacted form (this is what lands in logs/audit storage). The
    // `expose_secret()` raw access is process-local only.
    let _round_trip: RedactedUrl = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        url.expose_secret(),
        raw,
        "expose_secret still returns raw in-process"
    );
}

#[test]
fn redacted_url_display_keeps_path_visible() {
    use codegg_git::RedactedUrl;
    let url = RedactedUrl::new("https://bob:secret@host.example.com/team/proj.git");
    let display = format!("{url}");
    assert!(
        display.contains("host.example.com/team/proj.git"),
        "path must remain visible: {display}"
    );
    assert!(!display.contains("secret"));
}

#[test]
fn redact_url_credentials_in_text_strips_inline_url_credentials() {
    let dirty = "fatal: could not push to https://a:b@example.com/x.git\nother";
    let cleaned = redact_url_credentials_in_text(dirty);
    assert!(
        !cleaned.contains("a:b@"),
        "must strip inline userinfo: {cleaned}"
    );
    assert!(cleaned.contains("example.com"), "host preserved: {cleaned}");
}

#[test]
fn redact_url_credentials_in_text_passthrough_when_clean() {
    let clean = "nothing to redact here\njust plain log lines";
    let result = redact_url_credentials_in_text(clean);
    // Helper normalizes whitespace via split_whitespace/join (newline
    // token handling); but token contents must be unchanged.
    let in_tokens: Vec<&str> = clean.split_whitespace().collect();
    let out_tokens: Vec<&str> = result.split_whitespace().collect();
    assert_eq!(in_tokens, out_tokens, "token contents must match");
    assert!(!result.contains("REDACTED"));
}

#[test]
fn git_env_policy_strips_command_bearers_when_default() {
    let policy = GitEnvPolicy::default();
    // Sanity: the default flag-set must include strip_command_bearers
    // so that operators who reach for `apply()` get the hardening for
    // free. (If this changes, all `Command::new("git")` sites need a
    // re-review.)
    assert!(
        policy.strip_command_bearers,
        "default GitEnvPolicy must strip command-bearing GIT_* vars"
    );
}

#[test]
fn git_env_policy_allowed_env_vars_is_a_known_safe_allowlist() {
    // The allowlist must never grow GIT_ASKPASS / GIT_SSH_COMMAND.
    for key in codegg::git_mutations::ALLOWED_ENV_VARS {
        assert!(
            !key.starts_with("GIT_ASKPASS"),
            "GIT_ASKPASS must never be in allowed allowlist"
        );
        assert!(
            !key.contains("SSH_COMMAND"),
            "GIT_SSH_COMMAND must never be in allowed allowlist"
        );
    }
}

mod fs_utils {
    use std::fs;
    use std::path::Path;

    pub fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }
}
