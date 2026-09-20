//! Identity / audit corrective M002 — live execution audit hooks.
//!
//! Regression matrix for command, interactive-process, and Git live
//! audit hooks at their canonical owners:
//!
//! - shell/process command success + failure/timeout (bash, terminal);
//! - one direct native tool process path (terminal);
//! - scheduler-owned test dispatch (test tool with an immediate
//!   executor);
//! - interactive process create (no terminal input retention; other
//!   lifecycle ops emit nothing);
//! - Git stage/branch mutation, network fetch/push, and recovery;
//! - credential redaction across every Git path (negative);
//! - bash-routed Git convergence (one `git_operation`, zero
//!   `command_execute`);
//! - denied operations emit no execution event;
//! - retry/replay reuses the deterministic event id (no duplicates).
//!
//! All events carry structural digests and bounded labels only: argv,
//! command text, URLs, paths, messages, and subprocess output never
//! enter audit metadata.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;

use codegg::interactive_process_attach::{InteractiveAuditHook, InteractiveProcessProtocol};
use codegg::protocol::interactive_process::InteractiveProcessCreateRequest;
use codegg::scheduler::admission::AdmissionController;
use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, JobExecutionContext, JobExecutor,
    JobScheduler, ResolvedSchedulerConfig,
};
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::Tool;
use codegg_core::audit::{AuditAction, AuditQueryFilter, AuditStore};
use codegg_core::audit_instrumentation::{
    self as instr, ExecutionAuditEmitter, TrustedExecutionAuditContext,
};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobKind, JobStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::workspace::{ExecutionContext, InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio_util::sync::CancellationToken;

// ── Shared fixtures ───────────────────────────────────────────────

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn trusted_context(correlation: &str) -> TrustedExecutionAuditContext {
    let principal = AuthenticatedPrincipal::local_owner("client-m002");
    let provenance = codegg_core::audit::AuditDecisionProvenance::new(
        format!("decision-{correlation}"),
        correlation,
        "local_owner_broad",
        None,
    );
    let chain = instr::AuditChainContext {
        correlation_id: Some(correlation.to_owned()),
        session_id: Some(format!("session-{correlation}")),
        turn_id: Some(format!("turn-{correlation}")),
        run_id: Some(format!("run-{correlation}")),
        ..instr::AuditChainContext::default()
    };
    TrustedExecutionAuditContext::new(&principal, &provenance, chain)
}

fn tool_ctx(
    pool: &sqlx::SqlitePool,
    correlation: &str,
    invocation_key: &str,
) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    ctx.session_id = Some(format!("session-{correlation}"));
    ctx.invocation_key = Some(invocation_key.to_owned());
    ctx.apply_execution_audit(&trusted_context(correlation));
    ctx.apply_audit_emitter(&ExecutionAuditEmitter::new(Some(pool.clone())));
    ctx
}

async fn events_for(
    pool: &sqlx::SqlitePool,
    action: &str,
    correlation: &str,
) -> Vec<codegg_core::audit::AuditEvent> {
    let store = AuditStore::new(pool.clone());
    let page = store
        .query(&AuditQueryFilter {
            project_id: None,
            action: Some(action.to_owned()),
            principal: None,
            from_seq: None,
            limit: 200,
        })
        .await
        .expect("query audit");
    page.events
        .into_iter()
        .filter(|event| event.correlation_id == correlation)
        .collect()
}

fn metadata_blob(event: &codegg_core::audit::AuditEvent) -> String {
    serde_json::to_string(&event.metadata).unwrap_or_default()
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(dir: &Path, argv: &[&str]) {
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
}

fn init_repo(dir: &Path) {
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::write(dir.join("README.md"), "hello\n").expect("write readme");
    run_git(dir, &["add", "README.md"]);
    run_git(dir, &["commit", "-q", "-m", "initial"]);
}

fn audited_executor(
    pool: &sqlx::SqlitePool,
    correlation: &str,
) -> codegg::git_mutations::GitMutationExecutor {
    codegg::git_mutations::GitMutationExecutor::new()
        .with_timeout(Duration::from_secs(30))
        .with_execution_audit(trusted_context(correlation))
        .with_audit_emitter(ExecutionAuditEmitter::new(Some(pool.clone())))
}

// ── Shell / process command hooks ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn bash_shell_success_emits_one_structural_event() {
    let pool = test_pool().await;
    let tool = codegg::tool::bash::BashTool::new();
    let ctx = tool_ctx(&pool, "corr-bash-ok", "inv-bash-ok");
    let out = tool
        .execute_structured(serde_json::json!({"command": "echo m002-ok"}), Some(ctx))
        .await
        .expect("bash echo succeeds");
    assert!(
        out.output.contains("m002-ok"),
        "unexpected output: {}",
        out.output
    );

    let rows = events_for(&pool, AuditAction::CommandExecute.as_str(), "corr-bash-ok").await;
    assert_eq!(rows.len(), 1, "one dispatch → one event");
    let event = &rows[0];
    assert_eq!(
        event.metadata.get("command.family").map(String::as_str),
        Some("shell")
    );
    assert_eq!(
        event.metadata.get("decision.outcome").map(String::as_str),
        Some("success")
    );
    let expected_digest = instr::structural_digest(b"echo m002-ok");
    assert_eq!(
        event.metadata.get("command.digest").map(String::as_str),
        Some(expected_digest.as_str())
    );
    // Chain causation preserved from the trusted context.
    assert_eq!(event.session_id.as_deref(), Some("session-corr-bash-ok"));
    assert_eq!(event.turn_id.as_deref(), Some("turn-corr-bash-ok"));
    assert_eq!(event.run_id.as_deref(), Some("run-corr-bash-ok"));
    // No command text, ever.
    let blob = metadata_blob(event);
    assert!(!blob.contains("m002-ok"), "command text leaked: {blob}");
    assert!(!blob.contains("echo"), "command text leaked: {blob}");
}

#[tokio::test(flavor = "current_thread")]
async fn bash_failure_timeout_and_denial_matrix() {
    let pool = test_pool().await;
    let tool = codegg::tool::bash::BashTool::new();

    // Failure: real dispatch, nonzero exit, distinguishable outcome.
    let ctx = tool_ctx(&pool, "corr-bash-fail", "inv-bash-fail");
    let out = tool
        .execute_structured(serde_json::json!({"command": "exit 3"}), Some(ctx))
        .await
        .expect("exit 3 still returns output");
    assert!(
        out.output.contains("[exit code: 3]"),
        "output: {}",
        out.output
    );
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-bash-fail",
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].metadata.get("decision.outcome").map(String::as_str),
        Some("failure")
    );

    // Timeout: spawned then reaped by the deadline.
    let ctx = tool_ctx(&pool, "corr-bash-timeout", "inv-bash-timeout");
    let err = tool
        .execute_structured(
            serde_json::json!({"command": "sleep 30", "timeout": 1}),
            Some(ctx),
        )
        .await
        .expect_err("sleep must time out");
    assert!(
        err.to_string().to_lowercase().contains("timeout"),
        "err: {err}"
    );
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-bash-timeout",
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].metadata.get("decision.outcome").map(String::as_str),
        Some("timeout")
    );

    // Denied: blocked before any spawn → no execution event. The
    // denial itself is an authorization surface, not a command event.
    let ctx = tool_ctx(&pool, "corr-bash-denied", "inv-bash-denied");
    let err = tool
        .execute_structured(serde_json::json!({"command": "rm -rf /"}), Some(ctx))
        .await
        .expect_err("blocked command must fail");
    assert!(!err.to_string().is_empty());
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-bash-denied",
    )
    .await;
    assert!(rows.is_empty(), "denied dispatch must not emit");
}

#[tokio::test(flavor = "current_thread")]
async fn bash_retry_with_same_invocation_key_does_not_duplicate() {
    let pool = test_pool().await;
    let tool = codegg::tool::bash::BashTool::new();
    for _ in 0..2 {
        // One logical idempotent invocation, replayed: same key,
        // same command, same terminal outcome.
        let ctx = tool_ctx(&pool, "corr-bash-retry", "inv-bash-retry");
        tool.execute_structured(serde_json::json!({"command": "echo retry"}), Some(ctx))
            .await
            .expect("echo succeeds");
    }
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-bash-retry",
    )
    .await;
    assert_eq!(
        rows.len(),
        1,
        "replay must reuse the deterministic event id"
    );

    // A distinct logical invocation of the same command is a distinct
    // real execution and gets its own row.
    let ctx = tool_ctx(&pool, "corr-bash-retry", "inv-bash-retry-2");
    tool.execute_structured(serde_json::json!({"command": "echo retry"}), Some(ctx))
        .await
        .expect("echo succeeds");
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-bash-retry",
    )
    .await;
    assert_eq!(rows.len(), 2, "distinct invocations stay distinct");
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_direct_native_process_path_emits_once() {
    let pool = test_pool().await;
    let tool = codegg::tool::terminal::TerminalTool::new();
    let ctx = tool_ctx(&pool, "corr-terminal", "inv-terminal");
    let out = tool
        .execute_structured(
            serde_json::json!({"command": "echo direct-native"}),
            Some(ctx),
        )
        .await
        .expect("terminal echo succeeds");
    assert!(
        out.output.contains("direct-native"),
        "output: {}",
        out.output
    );

    let rows = events_for(&pool, AuditAction::CommandExecute.as_str(), "corr-terminal").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].metadata.get("command.family").map(String::as_str),
        Some("process")
    );
    assert_eq!(
        rows[0].metadata.get("decision.outcome").map(String::as_str),
        Some("success")
    );
    let blob = metadata_blob(&rows[0]);
    assert!(
        !blob.contains("direct-native"),
        "command text leaked: {blob}"
    );
}

// ── Scheduler-owned test dispatch ─────────────────────────────────

struct ImmediateTestExecutor;

struct TerminalTestExecutor(codegg::scheduler::executor::ExecutorStatus);

#[async_trait::async_trait]
impl JobExecutor for ImmediateTestExecutor {
    fn kind(&self) -> codegg::scheduler::ExecutorKind {
        ExecutorKind::Test
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Test)
    }
    fn validate(
        &self,
        _job: &codegg_core::jobs::JobRecord,
    ) -> Result<(), codegg::scheduler::executor::ExecutorValidationError> {
        Ok(())
    }
    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        ExecutorCompletion {
            status: codegg::scheduler::executor::ExecutorStatus::Completed,
            summary: "immediate-test-ok".into(),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

#[async_trait::async_trait]
impl JobExecutor for TerminalTestExecutor {
    fn kind(&self) -> codegg::scheduler::ExecutorKind {
        ExecutorKind::Test
    }
    fn supports(&self, kind: JobKind) -> bool {
        matches!(kind, JobKind::Test)
    }
    fn validate(
        &self,
        _job: &codegg_core::jobs::JobRecord,
    ) -> Result<(), codegg::scheduler::executor::ExecutorValidationError> {
        Ok(())
    }
    async fn execute(&self, _ctx: JobExecutionContext) -> ExecutorCompletion {
        ExecutorCompletion {
            status: self.0,
            summary: format!("terminal-{:?}", self.0),
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn scheduler_test_dispatch_emits_test_family_event() {
    let pool = test_pool().await;
    let root = tempfile::tempdir().expect("temp workspace");
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    registry
        .get_or_register(root.path())
        .await
        .expect("register");
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    scheduler
        .register_executor(Arc::new(ImmediateTestExecutor))
        .await
        .expect("register executor");
    let run_handle = scheduler.spawn_run();
    let submission = codegg::scheduler::submission::JobSubmissionService::new(
        store,
        scheduler.clone(),
        services,
        DaemonGeneration::new(),
    );

    let tool = codegg::tool::test::TestTool::new().with_submission(submission);
    let mut ctx = tool_ctx(&pool, "corr-testtool", "inv-testtool");
    ctx.session_id = Some("session-corr-testtool".to_owned());
    let out = tool
        .execute_structured(
            serde_json::json!({
                "scope": "custom",
                "command": "cargo test --version",
                "workdir": root.path().to_string_lossy(),
            }),
            Some(ctx),
        )
        .await
        .expect("immediate test dispatch succeeds");
    assert!(
        out.output.contains("immediate-test-ok"),
        "output: {}",
        out.output
    );

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = run_handle.await;

    let rows = events_for(&pool, AuditAction::CommandExecute.as_str(), "corr-testtool").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].metadata.get("command.family").map(String::as_str),
        Some("test")
    );
    assert_eq!(
        rows[0].metadata.get("decision.outcome").map(String::as_str),
        Some("success")
    );
    let blob = metadata_blob(&rows[0]);
    assert!(
        !blob.contains("cargo test --version"),
        "command text leaked: {blob}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_tool_without_scheduler_emits_nothing() {
    let pool = test_pool().await;
    let tool = codegg::tool::test::TestTool::new();
    let ctx = tool_ctx(&pool, "corr-testtool-denied", "inv-testtool-denied");
    tool.execute_structured(
        serde_json::json!({"scope": "auto", "workdir": "/tmp"}),
        Some(ctx),
    )
    .await
    .expect_err("scheduler admission is required");
    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-testtool-denied",
    )
    .await;
    assert!(rows.is_empty(), "undispatched test must not emit");
}

#[tokio::test(flavor = "current_thread")]
async fn scheduler_test_cancel_and_interrupt_stay_distinguishable() {
    // Cancelled and interrupted terminal outcomes emit with their own
    // bounded vocabulary — never invented success.
    for (status, outcome) in [
        (
            codegg::scheduler::executor::ExecutorStatus::Cancelled,
            "cancelled",
        ),
        (
            codegg::scheduler::executor::ExecutorStatus::Interrupted,
            "uncertain",
        ),
    ] {
        let pool = test_pool().await;
        let root = tempfile::tempdir().expect("temp workspace");
        let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("registry");
        registry
            .get_or_register(root.path())
            .await
            .expect("register");
        let services = WorkspaceServiceRegistry::new(
            registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            ResolvedSchedulerConfig::default(),
            DaemonGeneration::new(),
        );
        scheduler
            .register_executor(Arc::new(TerminalTestExecutor(status)))
            .await
            .expect("register executor");
        let run_handle = scheduler.spawn_run();
        let submission = codegg::scheduler::submission::JobSubmissionService::new(
            store,
            scheduler.clone(),
            services,
            DaemonGeneration::new(),
        );
        let tool = codegg::tool::test::TestTool::new().with_submission(submission);
        let correlation = format!("corr-testtool-{outcome}");
        let ctx = tool_ctx(&pool, &correlation, &format!("inv-testtool-{outcome}"));
        tool.execute_structured(
            serde_json::json!({
                "scope": "custom",
                "command": "cargo test --version",
                "workdir": root.path().to_string_lossy(),
            }),
            Some(ctx),
        )
        .await
        .expect("terminal dispatch returns");
        scheduler
            .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
            .await;
        let _ = run_handle.await;
        let rows = events_for(&pool, AuditAction::CommandExecute.as_str(), &correlation).await;
        assert_eq!(rows.len(), 1, "one {outcome} dispatch → one event");
        assert_eq!(
            rows[0].metadata.get("decision.outcome").map(String::as_str),
            Some(outcome)
        );
        assert_eq!(
            rows[0].metadata.get("command.family").map(String::as_str),
            Some("test")
        );
    }
}

// ── Interactive process create ────────────────────────────────────

fn test_admission(slots: u32) -> Arc<AdmissionController> {
    let mut config = ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = slots;
    Arc::new(AdmissionController::new(config))
}

async fn interactive_ctx(root: &std::path::Path) -> Arc<ExecutionContext> {
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.expect("registry");
    let record = registry.get_or_register(root).await.expect("register");
    let _services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    ExecutionContext::new(
        record,
        Some("session-interactive".to_string()),
        CancellationToken::new(),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn interactive_create_emits_without_input_retention() {
    let pool = test_pool().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let protocol = InteractiveProcessProtocol::new(test_admission(4));
    let ctx = interactive_ctx(dir.path()).await;
    let hook = InteractiveAuditHook {
        context: trusted_context("corr-interactive"),
        emitter: ExecutionAuditEmitter::new(Some(pool.clone())),
    };
    let dto = InteractiveProcessCreateRequest {
        workspace_id: "ws-interactive".to_owned(),
        argv: vec!["cat".to_owned()],
        cwd: None,
        env_overrides: vec![],
        cols: Some(80),
        rows: Some(24),
        scrollback_bytes: None,
    };
    let response = protocol.create("client-m002", &ctx, &dto, Some(hook)).await;
    let handle = match response {
        codegg::protocol::core::CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };

    // Attach + input + resize + detach + list exercise the remaining
    // lifecycle: none of them may emit command-content events.
    let attached = protocol.attach("client-m002", &handle, None, None).await;
    let attachment_id = match attached {
        codegg::protocol::core::CoreResponse::InteractiveProcessAttached {
            attachment_id, ..
        } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    let secret_input = base64::engine::general_purpose::STANDARD.encode(b"s3cret-keystrokes");
    let _ = protocol
        .input("client-m002", &attachment_id, &secret_input)
        .await;
    let _ = protocol.resize("client-m002", &attachment_id, 80, 24).await;
    let _ = protocol.detach("client-m002", &attachment_id).await;
    let _ = protocol.list("client-m002", None, None).await;

    let rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-interactive",
    )
    .await;
    assert_eq!(rows.len(), 1, "create emits once; lifecycle stays silent");
    let event = &rows[0];
    assert_eq!(
        event.metadata.get("command.family").map(String::as_str),
        Some("interactive")
    );
    assert_eq!(
        event.metadata.get("command.digest").map(String::as_str),
        Some(instr::structural_digest(b"cat").as_str())
    );
    let blob = metadata_blob(event);
    assert!(!blob.contains("s3cret-keystrokes"), "input leaked: {blob}");
    assert!(!blob.contains("cat"), "argv text leaked: {blob}");
}

// ── Git operation hooks ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn git_stage_and_branch_mutation_emit_once_each() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let exec = audited_executor(&pool, "corr-git-mutation");

    std::fs::write(repo.path().join("staged.txt"), "staged\n").expect("write");
    let staged =
        codegg::git_mutations_ops::stage_paths(&exec, repo.path(), vec!["staged.txt".to_owned()])
            .await
            .expect("stage");
    assert!(staged.success, "stage must succeed");
    codegg::git_mutations_ops::branch_create(&exec, repo.path(), "feature-m002", None, false)
        .await
        .expect("branch_create");

    let rows = events_for(
        &pool,
        AuditAction::GitOperation.as_str(),
        "corr-git-mutation",
    )
    .await;
    assert_eq!(
        rows.len(),
        2,
        "stage + branch_create → two events, got {}",
        rows.len()
    );
    let ops: Vec<&str> = rows
        .iter()
        .map(|event| {
            event
                .metadata
                .get("git.op")
                .map(String::as_str)
                .unwrap_or("?")
        })
        .collect();
    assert!(ops.contains(&"stage"), "ops: {ops:?}");
    assert!(ops.contains(&"branch_create"), "ops: {ops:?}");
    for event in &rows {
        // Bounded outcome vocabulary, trusted chain preserved.
        assert!(
            ["completed", "no-op", "fast-forward", "conflict", "rejected"].contains(
                &event
                    .metadata
                    .get("decision.outcome")
                    .map(String::as_str)
                    .unwrap_or("?")
            ),
            "unexpected outcome: {}",
            metadata_blob(event)
        );
        assert_eq!(
            event.session_id.as_deref(),
            Some("session-corr-git-mutation")
        );
        let blob = metadata_blob(event);
        assert!(!blob.contains("staged.txt"), "path list leaked: {blob}");
        assert!(
            !blob.contains("feature-m002") && !blob.contains("feature"),
            "ref text leaked (digest only): {blob}"
        );
        assert!(!blob.contains("initial"), "message leaked: {blob}");
    }
    // Stage carries no ref material: the empty structural digest.
    let stage = rows
        .iter()
        .find(|event| event.metadata.get("git.op").map(String::as_str) == Some("stage"))
        .expect("stage row");
    assert_eq!(
        stage.metadata.get("git.ref_digest").map(String::as_str),
        Some(instr::structural_digest(b"").as_str())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn git_network_push_fetch_and_credential_url_stay_structural() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    // Local bare remote: real network-path push/fetch without internet.
    let upstream = tempfile::tempdir().expect("upstream");
    run_git(upstream.path(), &["init", "-q", "--bare"]);
    run_git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            upstream.path().to_str().expect("utf8"),
        ],
    );
    let exec = audited_executor(&pool, "corr-git-network");

    codegg::git_network_ops::push(
        &exec,
        repo.path(),
        codegg::git_network_ops::PushRequest {
            remote: Some("origin".to_owned()),
            branch: Some("main".to_owned()),
            set_upstream: true,
            force: codegg::git_network_ops::PushForce::Normal,
            tags: false,
            delete: false,
            dry_run: false,
        },
    )
    .await
    .expect("push to local bare remote");
    codegg::git_network_ops::fetch(&exec, repo.path(), Some("origin"), vec![], false, false)
        .await
        .expect("fetch from local bare remote");

    // Re-point the remote at a credential-bearing URL on loopback (fast
    // refusal, no network): the failed fetch still emits, and the URL
    // must not appear anywhere in audit metadata.
    run_git(
        repo.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:s3cret-m002@127.0.0.1:1/repo.git",
        ],
    );
    let failed =
        codegg::git_network_ops::fetch(&exec, repo.path(), Some("origin"), vec![], false, false)
            .await;
    assert!(
        failed.is_err() || !failed.unwrap().success,
        "loopback fetch must fail"
    );

    let rows = events_for(
        &pool,
        AuditAction::GitOperation.as_str(),
        "corr-git-network",
    )
    .await;
    assert_eq!(
        rows.len(),
        3,
        "push + fetch + failed fetch, got {}",
        rows.len()
    );
    let ops: Vec<String> = rows
        .iter()
        .map(|event| event.metadata.get("git.op").cloned().unwrap_or_default())
        .collect();
    assert_eq!(
        ops.iter().filter(|op| op.as_str() == "push").count(),
        1,
        "ops: {ops:?}"
    );
    assert_eq!(
        ops.iter().filter(|op| op.as_str() == "fetch").count(),
        2,
        "ops: {ops:?}"
    );
    for event in &rows {
        let blob = metadata_blob(event);
        assert!(!blob.contains("s3cret-m002"), "credential leaked: {blob}");
        assert!(!blob.contains("127.0.0.1"), "URL material leaked: {blob}");
        assert!(!blob.contains("user"), "URL userinfo leaked: {blob}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn git_recovery_transition_emits_once() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    // Build a real merge conflict through raw git (unaudited fixture).
    std::fs::write(repo.path().join("conflict.txt"), "base\n").expect("write");
    run_git(repo.path(), &["add", "conflict.txt"]);
    run_git(repo.path(), &["commit", "-q", "-m", "base file"]);
    run_git(repo.path(), &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.path().join("conflict.txt"), "side\n").expect("write");
    run_git(repo.path(), &["commit", "-q", "-am", "side change"]);
    run_git(repo.path(), &["checkout", "-q", "main"]);
    std::fs::write(repo.path().join("conflict.txt"), "main\n").expect("write");
    run_git(repo.path(), &["commit", "-q", "-am", "main change"]);
    let merge_status = Command::new("git")
        .args(["merge", "side"])
        .current_dir(repo.path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git merge");
    assert!(!merge_status.success(), "fixture must conflict");

    let exec = audited_executor(&pool, "corr-git-recovery");
    let result = codegg::git_recovery::abort_in_progress_typed(&exec, repo.path())
        .await
        .expect("abort recovery");
    assert!(result.success, "abort must succeed");

    let rows = events_for(
        &pool,
        AuditAction::GitOperation.as_str(),
        "corr-git-recovery",
    )
    .await;
    assert_eq!(rows.len(), 1, "one recovery transition → one event");
    assert_eq!(
        rows[0].metadata.get("git.op").map(String::as_str),
        Some("merge")
    );
    assert_eq!(
        rows[0].metadata.get("decision.outcome").map(String::as_str),
        Some("completed")
    );
}

// ── Bash-routed Git convergence ───────────────────────────────────
//
// Native Git and bash-routed Git converge on one audit event. The
// typed route (`git add` inside the workspace) emits the single
// executor-owned `git_operation` while the shell layer stays silent;
// the managed-argv route (`git -C`, unparsed plumbing) emits a single
// shell `command_execute` while the executor stays silent.

#[tokio::test(flavor = "current_thread")]
async fn bash_routed_git_mutation_emits_single_git_event() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    // Typed active routing validates the repo against the workspace,
    // so the fixture repo lives under the package working directory
    // (mirrors the row_5c fixture); it is removed before and after.
    let repo_root = std::env::current_dir()
        .expect("cwd")
        .join(".codegg/test_tmp/m002-routed-git");
    let _ = std::fs::remove_dir_all(&repo_root);
    std::fs::create_dir_all(&repo_root).expect("create fixture repo");
    init_repo(&repo_root);
    std::fs::write(repo_root.join("routed.txt"), "routed\n").expect("write");

    let cic = codegg::config::schema::CommandIntentConfig {
        route_safe_commands: Some(true),
        mode: Some(codegg::config::schema::CommandIntentMode::Active),
        route_git_local_mutation: Some(codegg::config::schema::RouteLevel::Active),
        ..Default::default()
    };
    let tool = codegg::tool::bash::BashTool::new()
        .with_routing_disabled_env(false)
        .with_command_intent_config(cic);
    let ctx = tool_ctx(&pool, "corr-git-routed", "inv-git-routed");
    let out = tool
        .execute_structured(
            serde_json::json!({
                "command": "git add routed.txt",
                "workdir": repo_root.to_string_lossy(),
                "timeout": 30,
            }),
            Some(ctx),
        )
        .await
        .expect("routed git add succeeds");
    assert!(
        out.output.contains("[exit code: 0]"),
        "output: {}",
        out.output
    );
    // The file really was staged through the typed executor.
    let staged = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&repo_root)
        .output()
        .expect("git diff cached");
    assert!(
        String::from_utf8_lossy(&staged.stdout).contains("routed.txt"),
        "routed.txt must be staged"
    );

    let git_rows = events_for(&pool, AuditAction::GitOperation.as_str(), "corr-git-routed").await;
    let cmd_rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-git-routed",
    )
    .await;
    assert_eq!(git_rows.len(), 1, "executor owns the single event");
    assert_eq!(
        git_rows[0].metadata.get("git.op").map(String::as_str),
        Some("stage")
    );
    assert!(
        cmd_rows.is_empty(),
        "shell layer must stay silent on the Git route (found {})",
        cmd_rows.len()
    );
    let _ = std::fs::remove_dir_all(&repo_root);
}

#[tokio::test(flavor = "current_thread")]
async fn bash_managed_git_plumbing_emits_single_shell_event() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    std::fs::write(repo.path().join("plumbing.txt"), "plumbing\n").expect("write");

    // The `-C` form parses as managed argv (no typed executor): the
    // shell layer owns the single `command_execute` event.
    let cic = codegg::config::schema::CommandIntentConfig {
        route_safe_commands: Some(true),
        mode: Some(codegg::config::schema::CommandIntentMode::Active),
        route_git_local_mutation: Some(codegg::config::schema::RouteLevel::Active),
        ..Default::default()
    };
    let tool = codegg::tool::bash::BashTool::new()
        .with_routing_disabled_env(false)
        .with_command_intent_config(cic);
    let ctx = tool_ctx(&pool, "corr-git-plumbing", "inv-git-plumbing");
    let command = format!("git -C {} add plumbing.txt", repo.path().to_string_lossy());
    let out = tool
        .execute_structured(
            serde_json::json!({"command": command, "timeout": 30}),
            Some(ctx),
        )
        .await
        .expect("managed git add succeeds");
    assert!(
        out.output.contains("[exit code: 0]"),
        "output: {}",
        out.output
    );

    let git_rows = events_for(
        &pool,
        AuditAction::GitOperation.as_str(),
        "corr-git-plumbing",
    )
    .await;
    let cmd_rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-git-plumbing",
    )
    .await;
    assert!(git_rows.is_empty(), "managed plumbing emits no git event");
    assert_eq!(cmd_rows.len(), 1, "shell owns the single event");
    assert_eq!(
        cmd_rows[0]
            .metadata
            .get("command.family")
            .map(String::as_str),
        Some("process")
    );
}

// ── Denied / read-only negatives ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn denied_git_mutation_emits_no_execution_event() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let tool = codegg::tool::git::GitTool::new()
        .with_workdir(repo.path().to_path_buf())
        .with_child_policy(codegg::tool::git::ChildGitPolicy::LocalCommitOnly);
    let ctx = tool_ctx(&pool, "corr-git-denied", "inv-git-denied");
    tool.execute_structured(
        serde_json::json!({"mutation": "push", "remote": "origin"}),
        Some(ctx),
    )
    .await
    .expect_err("child policy denies network mutations");
    let git_rows = events_for(&pool, AuditAction::GitOperation.as_str(), "corr-git-denied").await;
    let cmd_rows = events_for(
        &pool,
        AuditAction::CommandExecute.as_str(),
        "corr-git-denied",
    )
    .await;
    assert!(git_rows.is_empty(), "denied mutation must not emit");
    assert!(cmd_rows.is_empty(), "denied mutation must not emit");
}

#[tokio::test(flavor = "current_thread")]
async fn read_only_git_operations_emit_nothing() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let tool = codegg::tool::git::GitTool::new().with_workdir(repo.path().to_path_buf());
    let ctx = tool_ctx(&pool, "corr-git-read", "inv-git-read");
    tool.execute_structured(
        serde_json::json!({"subcommand": "status", "args": []}),
        Some(ctx),
    )
    .await
    .expect("status succeeds");
    let git_rows = events_for(&pool, AuditAction::GitOperation.as_str(), "corr-git-read").await;
    let cmd_rows = events_for(&pool, AuditAction::CommandExecute.as_str(), "corr-git-read").await;
    assert!(git_rows.is_empty(), "read-only git must not emit");
    assert!(cmd_rows.is_empty(), "read-only git must not emit");
}

#[tokio::test(flavor = "current_thread")]
async fn git_replay_of_same_transition_does_not_duplicate() {
    if !git_available() {
        eprintln!("skipping: git not in PATH");
        return;
    }
    let pool = test_pool().await;
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let exec = audited_executor(&pool, "corr-git-replay");
    // Branch creation is idempotent in audit identity terms only when
    // the committed state matches; replay the label/scope math
    // directly to prove deterministic identity.
    let op = codegg_git::GitOperation::BranchCreate {
        name: codegg_git::ref_name::BranchName::new("replay").expect("branch"),
        start_point: None,
        force: false,
    };
    codegg::git_mutations_ops::branch_create(&exec, repo.path(), "replay", None, false)
        .await
        .expect("branch_create");
    let label = codegg::git_mutations::git_audit_op_label(&op).expect("label");
    let ref_digest = codegg::git_mutations::git_audit_ref_digest(&op);
    let audit = trusted_context("corr-git-replay");
    let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));
    for _ in 0..2 {
        codegg::git_mutations::emit_git_operation_parts(
            &audit,
            &emitter,
            label,
            &ref_digest,
            "completed",
            &format!("{label}|{ref_digest}|completed|fixed-state"),
        )
        .await;
    }
    let rows = events_for(&pool, AuditAction::GitOperation.as_str(), "corr-git-replay").await;
    // One row for the live branch_create plus one row for the
    // twice-emitted replay scope (idempotent store dedup).
    assert_eq!(
        rows.len(),
        2,
        "replay must not duplicate, got {}",
        rows.len()
    );
}

// ── Ownership matrix guard ────────────────────────────────────────

#[test]
fn command_execute_ownership_matrix() {
    // Canonical owners per execution family. ToolBroker itself never
    // emits: each family emits at the narrowest owner that knows a
    // real dispatch occurred.
    assert_eq!(
        codegg::live_execution_audit::command_family_for_tool("bash"),
        Some("shell")
    );
    assert_eq!(
        codegg::live_execution_audit::command_family_for_tool("terminal"),
        Some("process")
    );
    assert_eq!(
        codegg::live_execution_audit::command_family_for_tool("test"),
        Some("test")
    );
    assert_eq!(
        codegg::live_execution_audit::command_family_for_tool("git"),
        None
    );
    assert_eq!(
        codegg::live_execution_audit::command_family_for_tool("read"),
        None
    );
}
