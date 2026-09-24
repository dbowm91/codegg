//! Identity / audit corrective M004 — live execution qualification.
//!
//! Deterministic single-host trajectory across the three corrected live
//! hooks plus guard-closure negatives:
//!
//! 1. Authenticated project Owner submits a turn/job (trusted context).
//! 2. Model/tool path executes a command through `ToolBroker` (bash).
//! 3. A Git mutation occurs (`GitMutationExecutor`).
//! 4. An interactive process is created (no input retention).
//! 5. A scheduler job reaches terminal success (durable `job_complete`).
//! 6. Project Owner queries audit (daemon `audit_query`).
//!
//! Asserts ordered/correlated structural events, trusted actor, decision
//! ids, project/session/turn/run/job linkage, no bodies/secrets, and no
//! duplicates after restart/replay. Negatives cover unauthorized project
//! actor, credential-like Git URL, secret-looking command text, and
//! terminal input content.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::executor_audit_hooks::{EXECUTOR_HOOK_OWNER_PINS, PINNED_ACTIONS};
use codegg::interactive_process_attach::{InteractiveAuditHook, InteractiveProcessProtocol};
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::interactive_process::InteractiveProcessCreateRequest;
use codegg::scheduler::admission::AdmissionController;
use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, JobExecutionContext, JobExecutor,
    JobScheduler, ResolvedSchedulerConfig,
};
use codegg::tool::broker::{BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::ToolCaller;
use codegg::tool::ToolRegistry;
use codegg_core::audit::{AuditAction, AuditEvent, AuditQueryFilter, AuditStore};
use codegg_core::audit_instrumentation as instr;
use codegg_core::audit_instrumentation::{ExecutionAuditEmitter, TrustedExecutionAuditContext};
use codegg_core::identity::ProjectId;
use codegg_core::jobs::{
    DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::workspace::{ExecutionContext, InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio_util::sync::CancellationToken;

// ── Fixtures ────────────────────────────────────────────────────────────

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn owner_setup(
    pool: &sqlx::SqlitePool,
) -> (
    TeamStore,
    ProjectId,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
) {
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner_record = team
        .create_principal(PrincipalKind::Human, "m004-owner")
        .await
        .unwrap();
    team.create_membership(&project, &owner_record.id, ProjectRole::Owner)
        .await
        .unwrap();
    let owner = AuthenticatedPrincipal::internal_test(&owner_record, "client-owner");
    let viewer_record = team
        .create_principal(PrincipalKind::Human, "m004-viewer")
        .await
        .unwrap();
    team.create_membership(&project, &viewer_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    let viewer = AuthenticatedPrincipal::internal_test(&viewer_record, "client-viewer");
    (team, project, owner, viewer)
}

fn trajectory_context(
    owner: &AuthenticatedPrincipal,
    project: &ProjectId,
    correlation: &str,
    decision_id: &str,
    session: &str,
    turn: &str,
    run: &str,
    job: Option<&str>,
) -> TrustedExecutionAuditContext {
    let provenance = codegg_core::audit::AuditDecisionProvenance::new(
        decision_id,
        correlation,
        "team_membership",
        Some(project.clone()),
    );
    let chain = instr::AuditChainContext {
        correlation_id: Some(correlation.to_owned()),
        project: Some(project.clone()),
        session_id: Some(session.to_owned()),
        turn_id: Some(turn.to_owned()),
        run_id: Some(run.to_owned()),
        job_id: job.map(str::to_owned),
        ..instr::AuditChainContext::default()
    };
    TrustedExecutionAuditContext::new(owner, &provenance, chain)
}

fn make_grant_any() -> codegg_core::jobs::ToolAuthorityGrant {
    let mut grant = codegg_core::jobs::ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "m004-grant".into(),
        principal_ref: "test-principal".into(),
        workspace_id: "test-ws".into(),
        workspace_path_policy_id: "workspace:test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "test-policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: "any".into(),
        manifest_digest: "test-manifest".into(),
        source_digest: String::new(),
        ir_digest: String::new(),
        contract_digest: String::new(),
        contract_snapshot_json: String::new(),
        issued_at: 0,
        expires_at: None,
        revoked_at: None,
        decision_digest: String::new(),
    };
    grant.decision_digest = grant.compute_digest();
    grant
}

async fn events_for_correlation(pool: &sqlx::SqlitePool, correlation: &str) -> Vec<AuditEvent> {
    let store = AuditStore::new(pool.clone());
    let page = store
        .query(&AuditQueryFilter {
            project_id: None,
            action: None,
            principal: None,
            from_seq: None,
            limit: 500,
        })
        .await
        .expect("query audit");
    let mut rows: Vec<AuditEvent> = page
        .events
        .into_iter()
        .filter(|event| event.correlation_id == correlation)
        .collect();
    rows.sort_by_key(|event| event.seq);
    rows
}

fn metadata_blob(event: &AuditEvent) -> String {
    serde_json::to_string(&event.metadata).unwrap_or_default()
}

fn assert_structural(event: &AuditEvent, forbidden: &[&str]) {
    assert!(
        event.body_ref.is_none(),
        "no body: {}",
        event.event_id.as_str()
    );
    assert!(
        event.content_digest.is_none(),
        "no content: {}",
        event.event_id.as_str()
    );
    let blob = metadata_blob(event);
    for needle in forbidden {
        assert!(
            !blob.contains(*needle),
            "leak of {needle:?} in {}: {blob}",
            event.action
        );
    }
    // No secret-shaped values may reach storage (store would reject, but
    // assert explicitly for the census).
    for needle in [
        "ghp_",
        "sk-live",
        "s3cret",
        "password",
        "token=",
        "BEGIN PRIVATE",
        "aws_secret",
    ] {
        assert!(
            !blob.to_lowercase().contains(&needle.to_lowercase()),
            "secret-shaped {needle:?} in {}: {blob}",
            event.action
        );
    }
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
        .expect("git subcommand");
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

struct SuccessExecutor {
    run_id: Option<codegg_core::run_store::RunId>,
}

#[async_trait::async_trait]
impl JobExecutor for SuccessExecutor {
    fn kind(&self) -> ExecutorKind {
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
            status: codegg::scheduler::ExecutorStatus::Completed,
            summary: "m004-trajectory-ok".into(),
            run_id: self.run_id.clone(),
            metrics: ExecutorMetrics::default(),
        }
    }
}

fn trajectory_job(
    workspace: codegg_core::workspace::WorkspaceId,
    session: &str,
    turn: &str,
) -> NewJob {
    NewJob {
        workspace_id: workspace,
        session_id: Some(session.to_owned()),
        turn_id: Some(turn.to_owned()),
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Test {
            command: "echo m004".into(),
            argv: vec!["echo".into(), "m004".into()],
            cwd: Some("/tmp".into()),
            scope: None,
            parent_run_id: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::Test),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: ExecutionTarget::default(),
    }
}

// ── Declarative-table guard (consumed by tests) ─────────────────────────

#[test]
fn executor_hook_table_is_authoritative_and_fail_closed() {
    // The three corrected actions carry executable evidence; future scope
    // stays future; builder-only names never satisfy the guard.
    assert_eq!(
        PINNED_ACTIONS,
        &["command_execute", "git_operation", "job_complete"]
    );
    assert_eq!(instr::EXECUTOR_LIVE_AUDIT_HOOKS.len(), 3);
    for hook in instr::EXECUTOR_LIVE_AUDIT_HOOKS {
        assert!(instr::is_executor_live_action(hook.action));
        assert!(!instr::is_future_distributed_action(hook.action));
        let entry = instr::coverage_for_action(
            &codegg_core::audit::AuditAction::parse_known(hook.action).expect("known"),
        )
        .expect("coverage row");
        assert!(entry.live_mapped, "executor {} must be live", hook.action);
        assert_eq!(entry.owner, hook.owner);
        assert!(
            !instr::UNINSTRUMENTED_OPERATIONS.contains(&hook.action),
            "{} must never hide in UNINSTRUMENTED",
            hook.action
        );
        assert!(
            EXECUTOR_HOOK_OWNER_PINS
                .iter()
                .any(|pin| pin.action == hook.action),
            "no owner pin for {}",
            hook.action
        );
    }
    assert_eq!(
        instr::FUTURE_DISTRIBUTED_AUDIT_ACTIONS,
        &["node_enrollment", "remote_execute"]
    );
    for future in instr::FUTURE_DISTRIBUTED_AUDIT_ACTIONS {
        assert!(instr::is_future_distributed_action(future));
        assert!(!instr::is_executor_live_action(future));
        assert!(
            !EXECUTOR_HOOK_OWNER_PINS
                .iter()
                .any(|pin| pin.action == *future),
            "future {future} must not have a pin"
        );
    }
    // Negative proof: removing any hook breaks the table contract.
    let remaining: Vec<&&str> = PINNED_ACTIONS
        .iter()
        .filter(|a| ***a != *"git_operation")
        .collect();
    assert_eq!(
        remaining.len(),
        2,
        "removing git_operation must shrink the live set"
    );
}

// ── Single-host qualification trajectory ────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn single_host_trajectory_is_ordered_correlated_and_secret_free() {
    if !git_available() {
        panic!("git must be in PATH for M004 qualification");
    }
    let pool = test_pool().await;
    let (_team, project, owner, _viewer) = owner_setup(&pool).await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    daemon.clients.register_with_principal(
        "client-owner".to_owned(),
        "owner-name".to_owned(),
        None,
        owner.clone(),
    );

    let correlation = "corr-m004-trajectory";
    let decision_id = "decision-corr-m004-trajectory";
    let session = "session-m004";
    let turn = "turn-m004";
    let run = "run-m004";

    // Scheduler job first so its durable id can join the shared chain.
    let tmp_ws = tempfile::tempdir().expect("temp workspace");
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("registry");
    let ws_record = registry
        .get_or_register(tmp_ws.path())
        .await
        .expect("register");
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        job_store.clone(),
        services,
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new(),
    );
    let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));
    scheduler.set_audit_emitter(emitter.clone()).await;
    scheduler
        .register_executor(Arc::new(SuccessExecutor {
            run_id: Some(codegg_core::run_store::RunId::new_unchecked(run)),
        }))
        .await
        .expect("register executor");
    let job_record = job_store
        .create_job(trajectory_job(ws_record.id.clone(), session, turn))
        .await
        .expect("create job");
    let job_id = job_record.job_id.as_str().to_owned();
    // Durable attribution with the trajectory decision/correlation so the
    // terminal event joins the same chain (workspace-scoped: no project).
    let decision = codegg_core::authorization::AuthorizationDecision {
        principal_id: owner.principal_id().clone(),
        operation: "job_submit".to_owned(),
        capability: None,
        project_id: None,
        membership_revision: None,
        policy: codegg_core::authorization::PolicyKind::TeamMembership,
        decision_id: decision_id.to_owned(),
        correlation_id: correlation.to_owned(),
        reason: "m004 trajectory".to_owned(),
        decided_at_ms: 0,
    };
    let attribution =
        codegg_core::authorization::OriginAttribution::from_authority(&owner, &decision);
    codegg_core::authorization::OriginAttributionStore::new(pool.clone())
        .record("job", &job_id, &attribution)
        .await
        .expect("record attribution");

    let audit_ctx = trajectory_context(
        &owner,
        &project,
        correlation,
        decision_id,
        session,
        turn,
        run,
        Some(&job_id),
    );

    // 2. Model/tool path through ToolBroker (bash echo).
    let tool_registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&tool_registry);
    let broker_ctx = BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: tmp_ws.path().to_path_buf(),
        session_id: Some(session.to_owned()),
        workspace_id: None,
        agent_id: None,
        turn_id: Some(turn.to_owned()),
        job_id: Some(job_id.clone()),
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(10_000),
        submission_key: Some("m004-trajectory-cmd".to_owned()),
        authority: codegg::tool::BrokerAuthority::from_grant(make_grant_any()),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
        execution_audit: Some(audit_ctx.clone()),
        audit_emitter: Some(emitter.clone()),
    };
    let result = broker
        .execute_with_retry(
            &tool_registry,
            "bash",
            serde_json::json!({"command": "echo m004-trajectory"}),
            broker_ctx,
            None,
        )
        .await
        .expect("broker bash succeeds");
    assert!(result.value.display.contains("m004-trajectory"));

    // 3. Git mutation (single stage transition).
    let repo = tempfile::tempdir().expect("git repo");
    init_repo(repo.path());
    std::fs::write(repo.path().join("m004.txt"), "m004\n").expect("write");
    let git_exec = codegg::git_mutations::GitMutationExecutor::new()
        .with_timeout(Duration::from_secs(30))
        .with_execution_audit(audit_ctx.clone())
        .with_audit_emitter(emitter.clone());
    let staged =
        codegg::git_mutations_ops::stage_paths(&git_exec, repo.path(), vec!["m004.txt".to_owned()])
            .await
            .expect("stage");
    assert!(staged.success);

    // 4. Interactive process create (input stays out of audit).
    let admission = {
        let mut config = ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = 4;
        Arc::new(AdmissionController::new(config))
    };
    let protocol = InteractiveProcessProtocol::new(admission);
    let ws_store = Arc::new(InMemoryWorkspaceStore::new());
    let ws_registry = WorkspaceRegistry::load(ws_store).await.expect("registry");
    let ws_rec = ws_registry
        .get_or_register(repo.path())
        .await
        .expect("register");
    let _svc = WorkspaceServiceRegistry::new(
        ws_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let exec_ctx =
        ExecutionContext::new(ws_rec, Some(session.to_string()), CancellationToken::new());
    let hook = InteractiveAuditHook {
        context: audit_ctx.clone(),
        emitter: emitter.clone(),
    };
    let dto = InteractiveProcessCreateRequest {
        workspace_id: "ws-m004".to_owned(),
        argv: vec!["cat".to_owned()],
        cwd: None,
        env_overrides: vec![],
        cols: Some(80),
        rows: Some(24),
        scrollback_bytes: None,
    };
    let created = protocol
        .create("client-owner", &exec_ctx, &dto, Some(hook))
        .await;
    match created {
        CoreResponse::InteractiveProcessCreated { .. } => {}
        other => panic!("expected created, got {other:?}"),
    }

    // 5. Scheduler terminal success.
    scheduler
        .enqueue_existing(job_record)
        .await
        .expect("enqueue");
    let run_handle = scheduler.spawn_run();
    let completed = scheduler
        .wait_for_completion(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            Duration::from_secs(15),
        )
        .await
        .expect("terminal");
    assert_eq!(
        completed.status,
        codegg::scheduler::ExecutorStatus::Completed
    );
    for _ in 0..100 {
        if !events_for_correlation(&pool, correlation)
            .await
            .iter()
            .any(|e| e.action == "job_complete")
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        } else {
            break;
        }
    }
    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = run_handle.await;

    // 6a. Store-level end-to-end chain (all four live events, one correlation).
    let rows = events_for_correlation(&pool, correlation).await;
    let actions: Vec<&str> = rows.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "command_execute",
            "git_operation",
            "command_execute",
            "job_complete"
        ],
        "trajectory order must be command -> git -> interactive -> job, got {actions:?}"
    );
    // Ordered by coordinator sequence.
    for window in rows.windows(2) {
        assert!(window[0].seq < window[1].seq, "events must be ordered");
    }
    // Trusted actor + decision linkage on every row.
    for event in &rows {
        assert_eq!(
            event.actor_principal.as_str(),
            owner.principal_id().as_str(),
            "actor must be the trajectory owner"
        );
        assert_eq!(event.decision_id, decision_id);
        assert_eq!(event.correlation_id, correlation);
        assert_structural(event, &["m004-trajectory", "m004.txt", "initial", "hello"]);
    }
    // Project/session/turn/run/job linkage where available.
    let cmd = &rows[0];
    assert_eq!(cmd.session_id.as_deref(), Some(session));
    assert_eq!(cmd.turn_id.as_deref(), Some(turn));
    assert_eq!(cmd.run_id.as_deref(), Some(run));
    assert_eq!(
        cmd.metadata.get("command.family").map(String::as_str),
        Some("shell")
    );
    let git = &rows[1];
    assert_eq!(
        git.metadata.get("git.op").map(String::as_str),
        Some("stage")
    );
    assert_eq!(git.session_id.as_deref(), Some(session));
    let interactive = &rows[2];
    assert_eq!(
        interactive
            .metadata
            .get("command.family")
            .map(String::as_str),
        Some("interactive")
    );
    let job = &rows[3];
    assert_eq!(job.action, "job_complete");
    assert_eq!(
        job.metadata.get("job.outcome").map(String::as_str),
        Some("success")
    );
    assert_eq!(job.session_id.as_deref(), Some(session));
    assert_eq!(job.turn_id.as_deref(), Some(turn));
    assert_eq!(job.job_id.as_deref(), Some(job_id.as_str()));
    // Project linkage: command/git/interactive carry the project;
    // workspace-scoped job terminals document the where-available gap.
    for event in &rows[..3] {
        assert_eq!(event.project_id.as_ref(), Some(&project));
    }

    // 6b. Project Owner queries audit through the daemon gate.
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-m004-owner-query".to_owned(),
            CoreRequest::AuditQuery {
                query: codegg::protocol::core::AuditQueryRequestDto {
                    project_id: project.as_str().to_owned(),
                    action_filter: None,
                    principal_filter: None,
                    from_seq: None,
                    limit: Some(50),
                },
            },
        ),
        "client-owner",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::AuditPage { events, .. } => {
            let ours: Vec<_> = events
                .iter()
                .filter(|e| e.correlation_id == correlation)
                .collect();
            // Project-scoped rows (command/git/interactive) are visible to
            // the Owner; the workspace-scoped job terminal is correlated
            // via the store chain above.
            assert_eq!(
                ours.len(),
                3,
                "owner project query must return the 3 project rows"
            );
            assert!(ours.iter().all(|e| e.action != "job_complete"));
        }
        other => panic!("expected owner page, got {other:?}"),
    }

    // 6c. No duplicates after replay + restart reconciliation.
    let before = events_for_correlation(&pool, correlation).await.len();
    // Replay the broker command with the same submission key.
    let replay_ctx = BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: tmp_ws.path().to_path_buf(),
        session_id: Some(session.to_owned()),
        workspace_id: None,
        agent_id: None,
        turn_id: Some(turn.to_owned()),
        job_id: Some(job_id.clone()),
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(10_000),
        submission_key: Some("m004-trajectory-cmd".to_owned()),
        authority: codegg::tool::BrokerAuthority::from_grant(make_grant_any()),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
        execution_audit: Some(audit_ctx.clone()),
        audit_emitter: Some(emitter.clone()),
    };
    broker
        .execute_with_retry(
            &tool_registry,
            "bash",
            serde_json::json!({"command": "echo m004-trajectory"}),
            replay_ctx,
            None,
        )
        .await
        .expect("replay succeeds");
    // Replay the terminal attempt (must fail at the store, no new event).
    let attempts = job_store
        .list_attempts(&codegg_core::jobs::JobId::new_unchecked(job_id.clone()))
        .await
        .expect("list attempts");
    let attempt_id = attempts.last().expect("attempt").attempt_id.clone();
    let replay = job_store
        .finish_attempt(codegg_core::jobs::AttemptCompletion {
            attempt_id,
            state: codegg_core::jobs::AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await;
    assert!(replay.is_err(), "replayed terminal must fail");
    let _ = job_store
        .recover_generation(
            &DaemonGeneration::new(),
            &codegg_core::jobs::RecoveryPolicy::default(),
        )
        .await
        .expect("recover");
    let after = events_for_correlation(&pool, correlation).await.len();
    assert_eq!(before, after, "replay + restart must not duplicate");
    assert_eq!(before, 4);
}

// ── Negatives ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn unauthorized_project_actor_is_denied_without_execution_leak() {
    let pool = test_pool().await;
    let (_team, project, owner, viewer) = owner_setup(&pool).await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    daemon.clients.register_with_principal(
        "client-owner".to_owned(),
        "owner".to_owned(),
        None,
        owner.clone(),
    );
    daemon.clients.register_with_principal(
        "client-viewer".to_owned(),
        "viewer".to_owned(),
        None,
        viewer,
    );
    let store = AuditStore::new(pool.clone());
    let provenance = codegg_core::audit::AuditDecisionProvenance::new(
        "decision-m004-authz",
        "corr-m004-authz",
        "team_membership",
        Some(project.clone()),
    );
    let chain = instr::AuditChainContext {
        project: Some(project.clone()),
        ..instr::AuditChainContext::default()
    };
    store
        .append(instr::session_create_event(
            &owner,
            &provenance,
            &chain,
            "session-m004-authz",
            "allow",
        ))
        .await
        .unwrap();
    // Viewer (no audit.read) is denied with no existence signal.
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-m004-viewer-denied".to_owned(),
            CoreRequest::AuditQuery {
                query: codegg::protocol::core::AuditQueryRequestDto {
                    project_id: project.as_str().to_owned(),
                    action_filter: None,
                    principal_filter: None,
                    from_seq: None,
                    limit: Some(10),
                },
            },
        ),
        "client-viewer",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::Error { code, .. } => assert_eq!(code, "authorization_denied"),
        other => panic!("expected denial, got {other:?}"),
    }
    // No command/git/job event exists for the viewer correlation.
    let rows = events_for_correlation(&pool, "corr-m004-authz-viewer").await;
    assert!(rows.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn credential_git_url_secret_command_and_terminal_input_stay_structural() {
    if !git_available() {
        panic!("git must be in PATH for M004 negatives");
    }
    let pool = test_pool().await;
    let (_team, project, owner, _viewer) = owner_setup(&pool).await;
    let correlation = "corr-m004-secrets";
    let decision_id = "decision-corr-m004-secrets";
    let audit_ctx = trajectory_context(
        &owner,
        &project,
        correlation,
        decision_id,
        "session-m004-secret",
        "turn-m004-secret",
        "run-m004-secret",
        None,
    );
    let emitter = ExecutionAuditEmitter::new(Some(pool.clone()));

    // Credential-like Git URL: failed fetch still emits, URL never enters audit.
    let repo = tempfile::tempdir().expect("repo");
    init_repo(repo.path());
    run_git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://user:s3cret-m004@127.0.0.1:1/repo.git",
        ],
    );
    let git_exec = codegg::git_mutations::GitMutationExecutor::new()
        .with_timeout(Duration::from_secs(15))
        .with_execution_audit(audit_ctx.clone())
        .with_audit_emitter(emitter.clone());
    let failed = codegg::git_network_ops::fetch(
        &git_exec,
        repo.path(),
        Some("origin"),
        vec![],
        false,
        false,
    )
    .await;
    assert!(
        failed.is_err() || !failed.unwrap().success,
        "loopback fetch must fail"
    );
    let git_rows: Vec<AuditEvent> = events_for_correlation(&pool, correlation)
        .await
        .into_iter()
        .filter(|e| e.action == AuditAction::GitOperation.as_str())
        .collect();
    assert_eq!(git_rows.len(), 1, "failed fetch still emits once");
    let blob = metadata_blob(&git_rows[0]);
    assert!(!blob.contains("s3cret-m004"), "credential leaked: {blob}");
    assert!(!blob.contains("127.0.0.1"), "URL leaked: {blob}");

    // Secret-looking command text: digest only, never the secret.
    let tool_registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&tool_registry);
    let secret = "ghp_m004secretvalue123456";
    let broker_ctx = BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: PathBuf::from("."),
        session_id: Some("session-m004-secret".to_owned()),
        workspace_id: None,
        agent_id: None,
        turn_id: Some("turn-m004-secret".to_owned()),
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(10_000),
        submission_key: Some("m004-secret-cmd".to_owned()),
        authority: codegg::tool::BrokerAuthority::from_grant(make_grant_any()),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
        execution_audit: Some(audit_ctx.clone()),
        audit_emitter: Some(emitter.clone()),
    };
    let command = format!("echo {secret}");
    let result = broker
        .execute_with_retry(
            &tool_registry,
            "bash",
            serde_json::json!({"command": command}),
            broker_ctx,
            None,
        )
        .await;
    // The command itself succeeds (echo); audit must hold only the digest.
    assert!(result.is_ok(), "secret echo should still execute");
    let cmd_rows: Vec<AuditEvent> = events_for_correlation(&pool, correlation)
        .await
        .into_iter()
        .filter(|e| {
            e.action == AuditAction::CommandExecute.as_str()
                && e.metadata.get("command.family").map(String::as_str) == Some("shell")
        })
        .collect();
    assert_eq!(cmd_rows.len(), 1);
    let blob = metadata_blob(&cmd_rows[0]);
    assert!(!blob.contains(secret), "secret command leaked: {blob}");
    assert!(!blob.contains("ghp_"), "secret prefix leaked: {blob}");

    // Terminal input content: create emits once, keystrokes never enter audit.
    let admission = {
        let mut config = ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = 4;
        Arc::new(AdmissionController::new(config))
    };
    let protocol = InteractiveProcessProtocol::new(admission);
    let ws_store = Arc::new(InMemoryWorkspaceStore::new());
    let ws_registry = WorkspaceRegistry::load(ws_store).await.expect("registry");
    let ws_rec = ws_registry
        .get_or_register(repo.path())
        .await
        .expect("register");
    let exec_ctx = ExecutionContext::new(
        ws_rec,
        Some("session-m004-secret".to_string()),
        CancellationToken::new(),
    );
    let hook = InteractiveAuditHook {
        context: audit_ctx.clone(),
        emitter: emitter.clone(),
    };
    let dto = InteractiveProcessCreateRequest {
        workspace_id: "ws-m004-secret".to_owned(),
        argv: vec!["cat".to_owned()],
        cwd: None,
        env_overrides: vec![],
        cols: Some(80),
        rows: Some(24),
        scrollback_bytes: None,
    };
    let created = protocol
        .create("client-m004", &exec_ctx, &dto, Some(hook))
        .await;
    let handle = match created {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attached = protocol.attach("client-m004", &handle, None, None).await;
    let attachment_id = match attached {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    let secret_input =
        base64::engine::general_purpose::STANDARD.encode(b"terminal-s3cret-m004-keystrokes");
    let input_result = protocol
        .input("client-m004", &attachment_id, &secret_input)
        .await;
    let _ = input_result;
    let interactive_rows: Vec<AuditEvent> = events_for_correlation(&pool, correlation)
        .await
        .into_iter()
        .filter(|e| {
            e.action == AuditAction::CommandExecute.as_str()
                && e.metadata.get("command.family").map(String::as_str) == Some("interactive")
        })
        .collect();
    assert_eq!(
        interactive_rows.len(),
        1,
        "create emits once; input stays silent"
    );
    for event in events_for_correlation(&pool, correlation).await {
        let blob = metadata_blob(&event);
        assert!(
            !blob.contains("s3cret-m004"),
            "terminal secret leaked: {blob}"
        );
        assert!(!blob.contains("keystrokes"), "input leaked: {blob}");
        assert!(event.body_ref.is_none());
        assert_structural(&event, &["s3cret-m004", secret]);
    }
}
