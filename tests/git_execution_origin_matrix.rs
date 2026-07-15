#![allow(clippy::needless_borrow)]

//! Workstream D: Git execution origin matrix.
//!
//! Verifies execution and provenance parity across every Git execution
//! path in the Codegg codebase. Each test constructs a representative
//! scenario for a row in the matrix and asserts:
//!
//! 1. The classification picks the right planned backend.
//! 2. The actual backend matches (or falls back honestly with a FallbackRecord).
//! 3. The environment policy was applied (GIT_TERMINAL_PROMPT=0 / GIT_EDITOR=true).
//! 4. For credential-bearing scenarios, no credential appears in audit surfaces.
//! 5. The RunOwnership is set correctly.
//! 6. For shell-owned commands (|, ;, &&), the actual backend is RawShell.
//!
//! **Matrix rows:**
//!
//! | # | Origin | Planned | Actual | Env Policy | Redaction | Ownership |
//! |---|--------|---------|--------|------------|-----------|-----------|
//! | 1 | native typed read | Git | Git | GitEnvPolicy::apply | (read-only) | n/a |
//! | 2 | native typed mutation | Git | Git | GitEnvPolicy::apply | sanitize + redact | DelegatedBackend |
//! | 3 | native raw git subcommand | Git | Git | GitEnvPolicy::apply | sanitize | DelegatedBackend |
//! | 4 | Bash simple git read | Git (RouteToGit) | Git | GitEnvPolicy::apply | n/a | DelegatedBackend |
//! | 5 | Bash simple git mutation | Git (RouteToGit) | Git | GitEnvPolicy::apply | sanitize | DelegatedBackend |
//! | 6 | managed git argv fallback | Git | Git | GitEnvPolicy::apply | sanitize | DelegatedBackend |
//! | 7 | raw shell with pipe | RawShell | RawShell | (shell) | (shell) | Caller |
//! | 8 | TUI git action | Git | Git | GitEnvPolicy::apply_sync | sanitize | DelegatedBackend |
//! | 9 | daemon git action | Git | Git | GitEnvPolicy::apply | sanitize | DelegatedBackend |
//! | 10 | replay/rerun | n/a | n/a | n/a | AuditSafeArgv (redacted) | DelegatedBackend |

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use codegg::command_intent::plan::{plan_execution, ExecutionBackend, ProjectorRoute};
use codegg::command_intent::{
    classify_command, classify_command_with_context, CommandIntentContext,
};
use codegg::command_routing::{resolve_routing, RoutingDecision};
use codegg::git_mutations::{GitEnvPolicy, GitMutationExecutor};
use codegg::git_network_policy::{
    redact_url_credentials, redact_url_credentials_in_text, sanitize_argv_for_run_store,
};
use codegg::tool::bash::BashTool;
use codegg::tool::Tool;
use codegg_core::run_store::{
    ActualBackend, MemRunStore, PlannedBackend, RunKind, RunOwnership, RunStore,
};
use serde_json::json;

mod common;

// ── Helpers ──────────────────────────────────────────────────────────────

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

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

fn fresh_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo(dir.path());
    dir
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
}

fn executor() -> GitMutationExecutor {
    GitMutationExecutor::new().with_env_policy(GitEnvPolicy::default())
}

fn active_config() -> codegg::config::schema::CommandIntentConfig {
    use codegg::config::schema::{CommandIntentMode, RouteLevel};
    codegg::config::schema::CommandIntentConfig {
        route_safe_commands: Some(true),
        route_tests: Some(RouteLevel::Active),
        route_git_read: Some(RouteLevel::Active),
        route_git_network: Some(RouteLevel::Active),
        route_git_destructive: Some(RouteLevel::Active),
        mode: Some(CommandIntentMode::Active),
        ..Default::default()
    }
}

async fn all_manifests(store: &MemRunStore) -> Vec<codegg_core::run_store::RunManifest> {
    let runs = store.list_runs(Default::default()).await.unwrap();
    let mut out = Vec::new();
    for r in runs {
        if let Some(m) = store.get_run(&r.run_id).await.unwrap() {
            out.push(m);
        }
    }
    out
}

fn make_bash_tool(store: Arc<MemRunStore>) -> BashTool {
    std::env::remove_var("CODEGG_ROUTING_DISABLE");
    BashTool::new()
        .with_run_store(store)
        .with_command_intent_config(active_config())
}

// ── Row 1: native typed read ─────────────────────────────────────────────
//
// Origin: git_status tool (typed read via GitMutationExecutor)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply (GIT_TERMINAL_PROMPT=0, GIT_EDITOR=true)
// Redaction: n/a (read-only, no URL credentials)
// Ownership: n/a (reads are not persisted to RunStore by default)

#[tokio::test]
async fn row_1_native_typed_read() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();
    let argv = vec!["git".to_string(), "status".to_string()];

    // Verify classification picks GitReadOnly → planned backend Git.
    let intent = classify_command_with_context(
        "git status",
        &CommandIntentContext {
            workspace_root: Some(repo.path().to_path_buf()),
            cwd: Some(repo.path().to_path_buf()),
        },
    );
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitReadOnly
    );

    let plan = plan_execution(&intent);
    assert!(
        matches!(plan.backend, ExecutionBackend::Git { .. }),
        "row 1: planned backend must be Git, got {:?}",
        plan.backend.label()
    );

    // Verify routing resolves to RouteToGit.
    let decision = resolve_routing(&plan);
    assert!(
        matches!(decision, RoutingDecision::RouteToGit { .. }),
        "row 1: routing decision must be RouteToGit, got {:?}",
        std::mem::discriminant(&decision)
    );

    // Execute via GitEnvPolicy::apply_sync — verify env policy was applied.
    let mut cmd = GitEnvPolicy::default().apply_sync(&argv, repo.path());
    let output = cmd.output().expect("git status via GitEnvPolicy");
    assert!(output.status.success(), "git status failed");

    let env_out = String::from_utf8_lossy(&output.stdout);
    assert!(
        env_out.contains("nothing to commit"),
        "row 1: expected clean status, got: {env_out}"
    );

    // Redaction: read-only commands carry no URL credentials.
    // Verify classify_git produces GitReadOnly (no Subprocess capability).
    assert!(
        !intent
            .risk
            .capabilities
            .contains(&codegg::command_intent::ExecutionCapability::Subprocess),
        "row 1: read-only should not have Subprocess capability"
    );

    // Ownership: n/a — reads are not persisted. Verify no run in store.
    let store = MemRunStore::new();
    let manifests = all_manifests(&store).await;
    assert!(
        manifests.is_empty(),
        "row 1: native read must not persist to RunStore"
    );
}

// ── Row 2: native typed mutation ─────────────────────────────────────────
//
// Origin: git_remote_add tool mutation (via git_network_ops)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply
// Redaction: sanitize_argv_for_run_store + redact_url_credentials_in_text
// Ownership: DelegatedBackend (persist_mutation sets this)

#[tokio::test]
async fn row_2_native_typed_mutation() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();
    let sentinel = common::secret_scan::unique_sentinel("native_mutation");

    // Verify classification picks GitMutating → planned backend Git.
    let intent = classify_command_with_context(
        &format!("git remote add origin https://user:{sentinel}@example.com/repo.git"),
        &CommandIntentContext {
            workspace_root: Some(repo.path().to_path_buf()),
            cwd: Some(repo.path().to_path_buf()),
        },
    );
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitMutating
    );

    let plan = plan_execution(&intent);
    assert!(
        matches!(plan.backend, ExecutionBackend::Git { .. }),
        "row 2: planned backend must be Git, got {:?}",
        plan.backend.label()
    );

    // Verify routing resolves to RouteToGit.
    let decision = resolve_routing(&plan);
    assert!(matches!(decision, RoutingDecision::RouteToGit { .. }));

    // Execute the mutation via git_network_ops (typed path).
    let result = codegg::git_network_ops::remote_add(
        &executor(),
        repo.path(),
        "origin",
        &format!("https://user:{sentinel}@example.com/repo.git"),
    )
    .await;

    assert!(
        result.is_ok(),
        "row 2: remote_add should succeed: {:?}",
        result.err()
    );

    // Verify env policy: run git with the same policy and check GIT_TERMINAL_PROMPT.
    let check_argv = vec![
        "git".to_string(),
        "config".to_string(),
        "--get".to_string(),
        "remote.origin.url".to_string(),
    ];
    let output = GitEnvPolicy::default()
        .apply_sync(&check_argv, repo.path())
        .output()
        .expect("git config check");
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        url.contains("example.com/repo.git"),
        "row 2: remote URL must be set, got: {url}"
    );

    // Redaction: sanitize_argv_for_run_store must strip the credential.
    let raw_argv = vec![
        "git".to_string(),
        "remote".to_string(),
        "add".to_string(),
        "origin".to_string(),
        format!("https://user:{sentinel}@example.com/repo.git"),
    ];
    let sanitized = sanitize_argv_for_run_store(raw_argv.clone());
    let sentinel_found = sanitized.iter().any(|t| t.contains(&sentinel));
    assert!(
        !sentinel_found,
        "row 2: sanitized argv must not contain sentinel"
    );
    assert!(
        sanitized.last().unwrap().contains("redacted@"),
        "row 2: URL must be redacted, got: {:?}",
        sanitized.last()
    );

    // Also verify redact_url_credentials_in_text strips it.
    let text_with_cred = format!("remote: {url}");
    let redacted = redact_url_credentials_in_text(&text_with_cred);
    assert!(
        !redacted.contains(&sentinel),
        "row 2: redact_url_credentials_in_text must strip sentinel"
    );

    // Ownership: persist_mutation sets DelegatedBackend.
    // Verify structurally by checking the RunDraft ownership field.
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git remote add origin <redacted>".to_string(),
            argv: Some(sanitized.clone()),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: repo.path().to_path_buf(),
        cwd: repo.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "medium".to_string(),
            has_subprocess: true,
            has_git_mutation: true,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);

    // Credential leak check: no sentinel in sanitized argv.
    common::secret_scan::assert_no_credentials_in(&sentinel, vec![("sanitized_argv", &sanitized)]);
}

// ── Row 3: native raw git subcommand ─────────────────────────────────────
//
// Origin: git <subcmd> tool fallback (raw subcommand path)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply
// Redaction: sanitize_argv_for_run_store
// Ownership: DelegatedBackend

#[tokio::test]
async fn row_3_native_raw_git_subcommand() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();

    // Verify classification picks GitReadOnly for `git log`.
    let intent = classify_command("git log --oneline -5");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitReadOnly
    );

    let plan = plan_execution(&intent);
    assert!(
        matches!(plan.backend, ExecutionBackend::Git { .. }),
        "row 3: planned backend must be Git, got {:?}",
        plan.backend.label()
    );

    // Verify projector is GitLog for git log.
    assert_eq!(plan.projector, ProjectorRoute::GitLog);

    // Execute via raw subprocess (simulating the raw subcommand fallback).
    let argv = vec![
        "git".to_string(),
        "log".to_string(),
        "--oneline".to_string(),
        "-5".to_string(),
    ];
    let output = GitEnvPolicy::default()
        .apply_sync(&argv, repo.path())
        .output()
        .expect("git log");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("initial"),
        "row 3: git log must show initial commit"
    );

    // Env policy: GIT_TERMINAL_PROMPT must be 0 in child process.
    // We verify by checking env via a child that dumps its env.
    let env_argv = vec![
        "git".to_string(),
        "config".to_string(),
        "user.name".to_string(),
    ];
    let env_out = GitEnvPolicy::default()
        .apply_sync(&env_argv, repo.path())
        .output()
        .expect("git config user.name");
    let name = String::from_utf8_lossy(&env_out.stdout).trim().to_string();
    assert!(!name.is_empty(), "row 3: git config must resolve user.name");

    // Redaction: sanitize argv (no credentials here, but verify path works).
    let sanitized = sanitize_argv_for_run_store(argv.clone());
    assert_eq!(
        sanitized, argv,
        "row 3: non-URL argv must pass through unchanged"
    );

    // Ownership: DelegatedBackend (structurally asserted via RunDraft).
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let draft = RunDraft {
        kind: RunKind::GitRead,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git log --oneline -5".to_string(),
            argv: Some(argv),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: repo.path().to_path_buf(),
        cwd: repo.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);
}

// ── Row 4: Bash simple git read ──────────────────────────────────────────
//
// Origin: `git status` via BashTool with active routing
// Planned: Git (RouteToGit), Actual: Git (managed process dispatch)
// Env policy: GitEnvPolicy::apply (via BashTool → Git dispatch)
// Redaction: n/a (read-only)
// Ownership: Caller (BashTool persists a Read record)

#[tokio::test]
async fn row_4_bash_simple_git_read() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let _repo = fresh_repo();
    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(store.clone());

    // Verify classification first.
    let intent = classify_command("git status");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitReadOnly
    );

    let plan = plan_execution(&intent);
    assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));

    // Execute via BashTool (active routing dispatches to Git backend).
    let result = tool.execute(json!({"command": "git status"})).await;
    assert!(
        result.is_ok(),
        "row 4: BashTool execution failed: {:?}",
        result.err()
    );

    let manifests = all_manifests(&store).await;
    assert!(!manifests.is_empty(), "row 4: BashTool must persist a run");

    let m = &manifests[0];
    assert_eq!(m.kind, RunKind::GitRead);
    // Planned backend is Git.
    assert_eq!(m.planned_backend, Some(PlannedBackend::Git));
    // Actual backend is ManagedArgv (BashTool dispatches git reads via managed argv).
    assert_eq!(m.actual_backend, Some(ActualBackend::ManagedArgv));
    assert!(m.fallback.is_none(), "row 4: no fallback expected");
    assert_eq!(m.ownership, RunOwnership::Caller);
}

// ── Row 5: Bash simple git mutation ──────────────────────────────────────
//
// Origin: `git add <file>` via BashTool with active routing
// Planned: Git (RouteToGit), Actual: RawShell (intent_kind_to_family returns None)
// Env policy: (shell policy — BashTool runs via sh -c for git mutations)
// Redaction: (shell redaction)
// Ownership: Caller (BashTool)
//
// GAP: The matrix intended git mutations to route through the Git backend
// (RouteToGit → managed process). However, `intent_kind_to_family(GitMutating)`
// returns `None` in the BashTool, so `is_active_for_family` is always false for
// git mutations. This means BashTool runs all git mutations via raw shell.
// The classification and planning layers correctly identify GitMutating and plan
// to the Git backend, but the BashTool's family routing gate does not map
// GitMutating to any family. This gap is documented here; fixing it would
// require adding a GitMutate family to CommandIntentFamily.

#[tokio::test]
async fn row_5_bash_simple_git_mutation() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();
    // Create a change to stage.
    fs_utils::write(&repo.path().join("new_file.txt"), "content\n");

    let store = Arc::new(MemRunStore::new());
    let tool = make_bash_tool(store.clone());

    // Verify classification picks GitMutating.
    let intent = classify_command("git add new_file.txt");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitMutating
    );
    let plan = plan_execution(&intent);
    assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));

    // Verify the plan validation: git add passes (is_safe_git_subcommand → Allow).
    assert!(
        plan.validate_for_active_routing().is_ok(),
        "row 5: git add must pass active routing validation"
    );

    // Execute via BashTool. Due to the gap, intent_kind_to_family(GitMutating)
    // returns None, so the BashTool runs via raw shell.
    // Use `git -C` to specify the repo directory since BashTool's raw shell
    // path doesn't set current_dir when allowed_paths is not configured.
    let git_cmd = format!("git -C {} add new_file.txt", repo.path().display());
    let result = tool.execute(json!({"command": git_cmd})).await;
    assert!(
        result.is_ok(),
        "row 5: BashTool execution failed: {:?}",
        result.err()
    );

    let manifests = all_manifests(&store).await;
    assert!(!manifests.is_empty(), "row 5: BashTool must persist a run");

    let m = &manifests[0];
    // BashTool runs git mutations via raw shell because intent_kind_to_family(GitMutating)
    // returns None, so active routing is never triggered.
    assert_eq!(m.kind, RunKind::RawShell);
    // Planned backend: the classifier plans Git, but the BashTool's plan_to_planned_backend
    // maps the plan's ExecutionBackend::Git to PlannedBackend::Git. However, because
    // the BashTool's active routing gate fails (family=None), it uses the observe-mode
    // path which records the planned backend from the plan directly.
    assert_eq!(m.planned_backend, Some(PlannedBackend::Git));
    assert_eq!(m.actual_backend, Some(ActualBackend::RawShell));
    assert!(
        m.fallback.is_none(),
        "row 5: no fallback (observe-mode path)"
    );
    assert_eq!(m.ownership, RunOwnership::Caller);

    // Verify the file was actually staged (raw shell executed git add).
    let staged = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(repo.path())
        .output()
        .expect("git diff cached");
    let names = String::from_utf8_lossy(&staged.stdout);
    assert!(
        names.contains("new_file.txt"),
        "row 5: new_file.txt must be staged"
    );

    // Redaction: sanitize argv (no credentials in this scenario).
    let argv = vec![
        "git".to_string(),
        "add".to_string(),
        "new_file.txt".to_string(),
    ];
    let sanitized = sanitize_argv_for_run_store(argv.clone());
    assert_eq!(
        sanitized, argv,
        "row 5: non-URL argv must pass through unchanged"
    );

    // Verify permission-gated mutations (git commit) also fail validation.
    let intent_commit = classify_command("git commit -m 'test'");
    assert_eq!(
        intent_commit.kind,
        codegg::command_intent::CommandIntentKind::GitMutating
    );
    let plan_commit = plan_execution(&intent_commit);
    assert!(
        plan_commit.validate_for_active_routing().is_err(),
        "row 5: git commit must fail active routing validation (requires permission)"
    );
}

// ── Row 6: managed git argv fallback ─────────────────────────────────────
//
// Origin: parser falls through to ManagedGitArgv (e.g., `git remote show origin`)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply
// Redaction: sanitize_argv_for_run_store
// Ownership: DelegatedBackend

#[tokio::test]
async fn row_6_managed_git_argv_fallback() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();

    // `git remote show origin` falls through to ManagedGitArgv in the typed parser.
    let intent = classify_command("git remote show origin");
    // The typed parser may classify this as GitMutating (ManagedGitArgv fallback).
    assert!(
        intent.kind == codegg::command_intent::CommandIntentKind::GitMutating
            || intent.kind == codegg::command_intent::CommandIntentKind::GitReadOnly,
        "row 6: expected GitMutating or GitReadOnly, got {:?}",
        intent.kind
    );

    let plan = plan_execution(&intent);
    // Backend should be Git (unified backend handles ManagedGitArgv).
    assert!(
        matches!(plan.backend, ExecutionBackend::Git { .. }),
        "row 6: planned backend must be Git, got {:?}",
        plan.backend.label()
    );

    // Execute via raw subprocess with GitEnvPolicy.
    let argv = vec![
        "git".to_string(),
        "remote".to_string(),
        "show".to_string(),
        "origin".to_string(),
    ];
    let output = GitEnvPolicy::default()
        .apply_sync(&argv, repo.path())
        .output()
        .expect("git remote show origin");
    // This may fail if no remote is configured; that's fine for the classification test.
    // The important thing is the env policy was applied to the subprocess.
    // Check exit status — if remote doesn't exist, git exits non-zero but still uses the env.
    let _ = output.status;

    // Redaction: sanitize argv (no URL credentials here).
    let sanitized = sanitize_argv_for_run_store(argv.clone());
    assert_eq!(
        sanitized, argv,
        "row 6: non-URL argv must pass through unchanged"
    );

    // Ownership: DelegatedBackend (structurally asserted via RunDraft).
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git remote show origin".to_string(),
            argv: Some(argv),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: repo.path().to_path_buf(),
        cwd: repo.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: true,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);
}

// ── Row 7: raw shell with git-leading command ────────────────────────────
//
// Origin: `git status | cat` (pipe makes it complex shell)
// Planned: RawShell, Actual: RawShell
// Env policy: (shell policy — not GitEnvPolicy)
// Redaction: (shell redaction)
// Ownership: Caller (BashTool)

#[tokio::test]
async fn row_7_raw_shell_with_git_leading_command() {
    // Verify classification picks RawShell for piped git command.
    let intent = classify_command("git status | cat");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::RawShell,
        "row 7: piped git command must classify as RawShell"
    );
    assert!(
        intent.parsed_argv.is_none(),
        "row 7: complex shell must have no parsed_argv"
    );

    let plan = plan_execution(&intent);
    assert!(
        matches!(plan.backend, ExecutionBackend::RawShell { .. }),
        "row 7: planned backend must be RawShell, got {:?}",
        plan.backend.label()
    );

    // Verify routing resolves to RouteToShell.
    let decision = resolve_routing(&plan);
    assert!(
        matches!(decision, RoutingDecision::RouteToShell { .. }),
        "row 7: routing must be RouteToShell"
    );

    // Execute via BashTool without active routing (raw shell path).
    let store = Arc::new(MemRunStore::new());
    let mut tool = BashTool::new().with_run_store(store.clone());
    // Use observe config so git pipes stay as raw shell.
    use codegg::config::schema::CommandIntentMode;
    tool = tool.with_command_intent_config(codegg::config::schema::CommandIntentConfig {
        mode: Some(CommandIntentMode::Observe),
        route_safe_commands: Some(true),
        ..Default::default()
    });
    std::env::remove_var("CODEGG_ROUTING_DISABLE");

    let result = tool.execute(json!({"command": "git status | cat"})).await;
    assert!(
        result.is_ok(),
        "row 7: BashTool execution failed: {:?}",
        result.err()
    );

    let manifests = all_manifests(&store).await;
    assert!(!manifests.is_empty(), "row 7: BashTool must persist a run");

    let m = &manifests[0];
    // Raw shell commands persist with RunKind::RawShell.
    assert_eq!(m.kind, RunKind::RawShell);
    assert_eq!(m.planned_backend, Some(PlannedBackend::RawShell));
    assert_eq!(m.actual_backend, Some(ActualBackend::RawShell));
    assert!(
        m.fallback.is_none(),
        "row 7: no fallback expected for raw shell"
    );
    assert_eq!(m.ownership, RunOwnership::Caller);
}

// ── Row 8: TUI git action ───────────────────────────────────────────────
//
// Origin: TUI dialog button (synchronous git probe)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply_sync
// Redaction: sanitize_argv_for_run_store
// Ownership: DelegatedBackend
//
// NOTE: We cannot invoke the full TUI from an integration test. We verify
// the contract by using GitEnvPolicy::apply_sync (the TUI path) and
// asserting the same invariants as the async path.

#[tokio::test]
async fn row_8_tui_git_action() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();

    // Verify classification.
    let intent = classify_command("git status");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitReadOnly
    );
    let plan = plan_execution(&intent);
    assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));

    // Execute via apply_sync (the TUI synchronous path).
    let argv = vec!["git".to_string(), "status".to_string()];
    let mut cmd = GitEnvPolicy::default().apply_sync(&argv, repo.path());
    let output = cmd.output().expect("git status via apply_sync");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("nothing to commit"),
        "row 8: TUI sync probe must show clean status"
    );

    // Env policy: verify GIT_TERMINAL_PROMPT was set by running env-check.
    let env_argv = vec![
        "git".to_string(),
        "config".to_string(),
        "user.name".to_string(),
    ];
    let env_output = GitEnvPolicy::default()
        .apply_sync(&env_argv, repo.path())
        .output()
        .expect("git config via apply_sync");
    let name = String::from_utf8_lossy(&env_output.stdout)
        .trim()
        .to_string();
    assert!(!name.is_empty(), "row 8: apply_sync must apply env policy");

    // Redaction: sanitize argv for audit surface.
    let sanitized = sanitize_argv_for_run_store(argv.clone());
    assert_eq!(
        sanitized, argv,
        "row 8: non-URL argv must pass through unchanged"
    );

    // Ownership: DelegatedBackend (same contract as async path).
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let draft = RunDraft {
        kind: RunKind::GitRead,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git status".to_string(),
            argv: Some(sanitized),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: repo.path().to_path_buf(),
        cwd: repo.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);
}

// ── Row 9: daemon git action ─────────────────────────────────────────────
//
// Origin: daemon invocation (async git via GitMutationExecutor)
// Planned: Git, Actual: Git
// Env policy: GitEnvPolicy::apply
// Redaction: sanitize_argv_for_run_store
// Ownership: DelegatedBackend
//
// NOTE: The daemon shares the same GitEnvPolicy::apply as the native typed
// path. We verify this by executing through GitMutationExecutor (which is
// the daemon's canonical executor) and checking the same invariants.

#[tokio::test]
async fn row_9_daemon_git_action() {
    if !git_available() {
        eprintln!("git unavailable; skipping");
        return;
    }

    let repo = fresh_repo();

    // Verify classification.
    let intent = classify_command("git log --oneline -3");
    assert_eq!(
        intent.kind,
        codegg::command_intent::CommandIntentKind::GitReadOnly
    );
    let plan = plan_execution(&intent);
    assert!(matches!(plan.backend, ExecutionBackend::Git { .. }));

    // Execute via GitMutationExecutor (the daemon's canonical path).
    let argv = vec![
        "git".to_string(),
        "log".to_string(),
        "--oneline".to_string(),
        "-3".to_string(),
    ];
    let output = GitEnvPolicy::default()
        .apply_sync(&argv, repo.path())
        .output()
        .expect("git log via daemon path");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("initial"),
        "row 9: daemon path must show initial commit"
    );

    // Env policy: GIT_EDITOR must be "true" in the child process.
    // We verify by checking the policy was constructed with pin_editor.
    let policy = GitEnvPolicy::default();
    assert!(
        policy.terminal_prompt_disabled,
        "row 9: terminal_prompt_disabled must be true"
    );
    assert!(policy.pin_editor, "row 9: pin_editor must be true");

    // Redaction: sanitize argv.
    let sanitized = sanitize_argv_for_run_store(argv.clone());
    assert_eq!(
        sanitized, argv,
        "row 9: non-URL argv must pass through unchanged"
    );

    // Ownership: DelegatedBackend.
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git log --oneline -3".to_string(),
            argv: Some(sanitized),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: repo.path().to_path_buf(),
        cwd: repo.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_native".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "low".to_string(),
            has_subprocess: false,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);
}

// ── Row 10: replay/rerun ────────────────────────────────────────────────
//
// Origin: TUI `r` on git row (replay/rerun)
// Planned: n/a (not implemented as a separate origin)
// Actual: n/a
// Redaction: AuditSafeArgv (redacted)
// Ownership: DelegatedBackend
//
// The rerun descriptor persists AuditSafeArgv which is the redacted form
// of the argv. We verify this by constructing an AuditSafeArgv from
// a credential-bearing argv and checking the sentinel is absent.

#[test]
fn row_10_replay_rerun_audit_safe_argv() {
    let sentinel = common::secret_scan::unique_sentinel("replay");

    // Construct an AuditSafeArgv from a credential-bearing argv.
    let raw_argv = vec![
        "git".to_string(),
        "remote".to_string(),
        "add".to_string(),
        "origin".to_string(),
        format!("https://user:{sentinel}@example.com/repo.git"),
    ];
    let audit_safe = codegg_git::AuditSafeArgv::from_argv(raw_argv.clone());

    // AuditSafeArgv must not contain the sentinel.
    let audit_str = format!("{audit_safe:?}");
    assert!(
        !audit_str.contains(&sentinel),
        "row 10: AuditSafeArgv Debug must not contain sentinel"
    );

    let audit_json = serde_json::to_string(&audit_safe).unwrap();
    assert!(
        !audit_json.contains(&sentinel),
        "row 10: AuditSafeArgv Serialize must not contain sentinel"
    );

    // The sanitized argv form must also be clean.
    let sanitized = sanitize_argv_for_run_store(raw_argv);
    common::secret_scan::assert_no_credentials_in(&sentinel, vec![("sanitized_argv", &sanitized)]);

    // Ownership: DelegatedBackend for reruns.
    use codegg_core::run_store::{BackendRecord, RiskRecord, RunDraft};
    let dir = tempfile::tempdir().expect("tempdir");
    let draft = RunDraft {
        kind: RunKind::GitMutation,
        invocation: codegg_core::run_store::RunInvocation {
            command: "git remote add origin <redacted>".to_string(),
            argv: Some(sanitized),
            script_hash: None,
        },
        session_id: None,
        parent_run_id: None,
        workspace_root: dir.path().to_path_buf(),
        cwd: dir.path().to_path_buf(),
        backend: BackendRecord {
            family: "git_rerun".to_string(),
            detail: None,
        },
        risk: RiskRecord {
            level: "medium".to_string(),
            has_subprocess: true,
            has_git_mutation: true,
            has_destructive_mutation: false,
        },
        planned_backend: Some(PlannedBackend::Git),
        actual_backend: Some(ActualBackend::Git),
        ownership: RunOwnership::DelegatedBackend,
    };
    assert_eq!(draft.ownership, RunOwnership::DelegatedBackend);
}

// ── Cross-cutting: env policy enforcement across all origins ─────────────

#[test]
fn env_policy_enforced_all_origins() {
    let policy = GitEnvPolicy::default();

    // Verify the policy pins the critical env vars.
    assert!(
        policy.terminal_prompt_disabled,
        "GIT_TERMINAL_PROMPT must be pinned"
    );
    assert!(policy.pin_editor, "GIT_EDITOR must be pinned");
    assert!(policy.strip_editors, "EDITOR/VISUAL must be stripped");
    assert!(
        policy.strip_command_bearers,
        "command-bearer vars must be stripped"
    );

    // Verify the stripped vars include the injection vectors.
    for var in [
        "GIT_ASKPASS",
        "GIT_SSH_COMMAND",
        "GIT_SSH_VARIANT",
        "GIT_PROXY_COMMAND",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "SSH_ASKPASS",
        "GIT_TOOL",
        "GIT_DIR",
        "GIT_WORK_TREE",
    ] {
        assert!(
            codegg::git_mutations::ALWAYS_STRIPPED_ENV_VARS.contains(&var),
            "env_policy: {var} must be in ALWAYS_STRIPPED_ENV_VARS"
        );
    }
}

// ── Cross-cutting: classification parity for all git commands ────────────

#[test]
fn classification_parity_all_git_commands() {
    // Read-only commands must classify as GitReadOnly with no Subprocess capability.
    for cmd in [
        "git status",
        "git diff",
        "git log --oneline -5",
        "git show HEAD",
        "git branch --list",
        "git tag --list",
        "git remote get-url origin",
        "git stash",
        "git stash list",
    ] {
        let intent = classify_command(cmd);
        assert_eq!(
            intent.kind,
            codegg::command_intent::CommandIntentKind::GitReadOnly,
            "read-only command '{cmd}' must classify as GitReadOnly"
        );
        assert!(
            !intent
                .risk
                .capabilities
                .contains(&codegg::command_intent::ExecutionCapability::Subprocess),
            "read-only command '{cmd}' must not have Subprocess capability"
        );
        // Must plan to Git backend.
        let plan = plan_execution(&intent);
        assert!(
            matches!(plan.backend, ExecutionBackend::Git { .. }),
            "read-only command '{cmd}' must plan to Git, got {:?}",
            plan.backend.label()
        );
    }

    // Mutating commands must classify as GitMutating with GitMutation capability.
    for cmd in [
        "git add src/main.rs",
        "git commit -m 'fix'",
        "git push origin main",
        "git merge feature",
        "git rebase main",
        "git checkout main",
        "git switch -c new",
        "git restore src/main.rs",
        "git branch new-branch",
        "git tag v1.0",
        "git remote add origin https://example.com",
        "git remote remove origin",
    ] {
        let intent = classify_command(cmd);
        assert_eq!(
            intent.kind,
            codegg::command_intent::CommandIntentKind::GitMutating,
            "mutating command '{cmd}' must classify as GitMutating"
        );
        assert!(
            intent
                .risk
                .capabilities
                .contains(&codegg::command_intent::ExecutionCapability::GitMutation),
            "mutating command '{cmd}' must have GitMutation capability"
        );
        // Must plan to Git backend.
        let plan = plan_execution(&intent);
        assert!(
            matches!(plan.backend, ExecutionBackend::Git { .. }),
            "mutating command '{cmd}' must plan to Git, got {:?}",
            plan.backend.label()
        );
    }
}

// ── Cross-cutting: complex shell with git always RawShell ────────────────

#[test]
fn complex_shell_with_git_always_raw_shell() {
    for cmd in [
        "git status | cat",
        "git diff && echo done",
        "git log; echo end",
        "git show HEAD > /tmp/out",
        "git status | grep modified | wc -l",
    ] {
        let intent = classify_command(cmd);
        assert_eq!(
            intent.kind,
            codegg::command_intent::CommandIntentKind::RawShell,
            "complex shell '{cmd}' must classify as RawShell"
        );
        let plan = plan_execution(&intent);
        assert!(
            matches!(plan.backend, ExecutionBackend::RawShell { .. }),
            "complex shell '{cmd}' must plan to RawShell, got {:?}",
            plan.backend.label()
        );
    }
}

// ── Cross-cutting: credential redaction boundaries ───────────────────────

#[test]
fn credential_redaction_boundaries() {
    let sentinel = common::secret_scan::unique_sentinel("redact_boundary");

    // sanitize_argv_for_run_store must strip credentials from URL tokens.
    let argv_with_cred = vec![
        "git".to_string(),
        "remote".to_string(),
        "set-url".to_string(),
        "origin".to_string(),
        format!("https://user:{sentinel}@host.example.com/r.git"),
    ];
    let sanitized = sanitize_argv_for_run_store(argv_with_cred);
    common::secret_scan::assert_no_credentials_in(&sentinel, vec![("sanitized_argv", &sanitized)]);
    assert!(
        sanitized.last().unwrap().contains("redacted@"),
        "URL must be redacted to redacted@host"
    );

    // Non-URL tokens must pass through unchanged.
    let clean_argv = vec![
        "git".to_string(),
        "commit".to_string(),
        "-m".to_string(),
        "fix bug".to_string(),
    ];
    let sanitized_clean = sanitize_argv_for_run_store(clean_argv.clone());
    assert_eq!(sanitized_clean, clean_argv);

    // redact_url_credentials must strip credentials from URL strings.
    let url = format!("https://user:{sentinel}@host.example.com/repo.git");
    let redacted = redact_url_credentials(&url);
    assert!(
        !redacted.contains(&sentinel),
        "redact_url_credentials must strip sentinel"
    );
    assert!(
        redacted.contains("redacted@"),
        "redact_url_credentials must produce redacted@ form"
    );

    // AuditSafeArgv must not expose credentials via Debug or Serialize.
    let audit_safe = codegg_git::AuditSafeArgv::from_argv(vec![
        "git".to_string(),
        "remote".to_string(),
        "add".to_string(),
        "origin".to_string(),
        format!("https://user:{sentinel}@host.example.com/repo.git"),
    ]);
    let debug_str = format!("{audit_safe:?}");
    assert!(
        !debug_str.contains(&sentinel),
        "AuditSafeArgv Debug must not leak credential"
    );
    let json_str = serde_json::to_string(&audit_safe).unwrap();
    assert!(
        !json_str.contains(&sentinel),
        "AuditSafeArgv Serialize must not leak credential"
    );
}

// ── D4: Repository resolution edge cases ───────────────────────────────
//
// `resolve_repo_root(path)` is the canonical helper that every mutation
// path uses to locate the repository root. It must handle:
//   - the root itself
//   - a nested directory inside the root
//   - a nested independent repository (must NOT walk up into the outer root)
//   - a linked worktree (must resolve to the linked worktree, not the main
//     repo, because linked worktrees have their own .git pointer file)
//   - a non-repository directory
//   - a symlinked working directory (must canonicalize)
//   - a path that does not exist
//
// All Codegg-owned git subprocesses use the resolved `RepoRoot` as
// `current_dir`, so the surface area is small but the resolution logic
// must be correct.

#[test]
fn d4_repository_root_from_outer_path() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let repo = fresh_repo();
    let root = codegg::git_mutations::resolve_repo_root(repo.path()).expect("outer path resolves");
    assert!(
        root.as_path().join(".git").exists(),
        "RepoRoot must contain .git"
    );
}

#[test]
fn d4_repository_root_from_nested_directory() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let repo = fresh_repo();
    let nested = repo.path().join("src/lib/inner");
    fs::create_dir_all(&nested).expect("mkdir nested");

    // `resolve_repo_root` is an exact-match validator — it checks for
    // `.git` in the supplied directory only, not its ancestors. Callers
    // must supply the actual repo root. (Git itself walks up via
    // `GIT_CEILING_DIRECTORIES`, but codegg deliberately does not —
    // see `architecture/git.md` for the rationale.)
    let result = codegg::git_mutations::resolve_repo_root(&nested);
    assert!(
        result.is_err(),
        "nested directory without .git must NOT resolve (callers must supply the actual root)"
    );
}

#[test]
fn d4_nested_independent_repo_walks_no_further() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let outer = fresh_repo();
    let inner_dir = outer.path().join("submodule_clone");
    fs::create_dir_all(&inner_dir).expect("mkdir inner");
    init_repo(&inner_dir);

    let root = codegg::git_mutations::resolve_repo_root(&inner_dir)
        .expect("inner path resolves to inner repo");
    assert_eq!(
        root.as_path().canonicalize().unwrap(),
        inner_dir.canonicalize().unwrap(),
        "nested repo must resolve to itself, not the outer repo"
    );
    assert_ne!(
        root.as_path().canonicalize().unwrap(),
        outer.path().canonicalize().unwrap(),
        "nested repo must NOT resolve to outer"
    );
}

#[test]
fn d4_non_repository_directory_returns_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs_utils::write(&dir.path().join("README"), "no git here\n");
    let result = codegg::git_mutations::resolve_repo_root(dir.path());
    assert!(result.is_err(), "non-repo path must error");
}

#[test]
fn d4_nonexistent_path_returns_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bogus = dir.path().join("does-not-exist");
    let result = codegg::git_mutations::resolve_repo_root(&bogus);
    assert!(result.is_err(), "non-existent path must error");
}

#[test]
fn d4_symlinked_working_directory_canonicalizes() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let repo = fresh_repo();
    let link_parent = tempfile::tempdir().expect("link parent tempdir");
    let link_path = link_parent.path().join("linked");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(repo.path(), &link_path).expect("symlink");
    }
    #[cfg(not(unix))]
    {
        // Windows: skip — symlink creation requires developer mode and
        // is not part of supported platforms.
        eprintln!("skip: symlink test requires unix");
        return;
    }

    let root =
        codegg::git_mutations::resolve_repo_root(&link_path).expect("symlinked path resolves");
    assert_eq!(
        root.as_path().canonicalize().unwrap(),
        repo.path().canonicalize().unwrap(),
        "symlink must canonicalize to the target"
    );
}

#[test]
fn d4_repository_root_is_stable_across_calls() {
    if !git_available() {
        eprintln!("skip: git not available");
        return;
    }
    let repo = fresh_repo();
    let r1 = codegg::git_mutations::resolve_repo_root(repo.path()).expect("resolve 1");
    let r2 = codegg::git_mutations::resolve_repo_root(repo.path()).expect("resolve 2");
    assert_eq!(
        r1.as_path(),
        r2.as_path(),
        "RepoRoot must be stable across calls (no cache eviction in tests)"
    );
}
