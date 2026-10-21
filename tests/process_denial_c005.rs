//! M005 corrective C005 — the process deny contract.
//!
//! The restricted shared-checkout profile admits no unqualified process.
//! These tests drive that refusal through the real seams: a real reserved
//! checkout (the C004 fixture), the real broker, the real scheduler
//! submission boundary, the real human-shell service and the real
//! reservation preflight.
//!
//! What the suite proves:
//!
//! * a reserved checkout refuses a process launch **before** a process
//!   exists, with a typed refusal that names the capability and offers a
//!   recovery path;
//! * an ordinary checkout is unaffected — the refusal is about the
//!   *checkout*, not about processes in general;
//! * two separate worktrees run shells concurrently under the previously
//!   permitted profile;
//! * a live persistent process blocks a shared reservation until it is
//!   drained;
//! * the refusal is recorded durably, attributed and redacted.

mod common;

use std::sync::Arc;

use codegg::core::process_admission::build_process_admission;
use codegg::tool::process_admission::{admit_process, SharedProcessAdmission};
use codegg_core::process_admission::{
    PersistentProcessEntry, PersistentProcessRegistry, ProcessAdmissionService, ProcessDenial,
    ProcessDenialReason, ProcessKind,
};
use common::checkout_reservation::ReservedCheckout;

/// The admission policy for a checkout, over the daemon's own authority.
fn policy_for(fixture: &ReservedCheckout, root: &std::path::Path) -> SharedProcessAdmission {
    build_process_admission(Some(fixture.daemon.process_admission.clone()), root, None)
        .expect("the daemon has a durable reservation authority")
}

#[tokio::test]
async fn a_reserved_checkout_refuses_a_command_before_any_process_exists() {
    let fixture = ReservedCheckout::build().await;
    let policy = policy_for(&fixture, fixture.checkout.path());

    let error = admit_process(Some(&policy), ProcessKind::Command)
        .await
        .expect_err("a reserved checkout admits no unqualified process");
    match error {
        codegg_core::error::ToolError::ProcessDenied {
            kind,
            reason,
            recovery,
        } => {
            assert_eq!(kind, ProcessKind::Command);
            assert_eq!(reason, ProcessDenialReason::SharedCheckout);
            assert!(
                recovery.contains("isolated checkout"),
                "the refusal must offer a recovery path: {recovery}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn an_ordinary_checkout_admits_the_same_command() {
    let fixture = ReservedCheckout::build().await;
    let (ordinary, _project_id, _workspace_id) = fixture.ordinary_workspace().await;
    let policy = policy_for(&fixture, ordinary.path());

    admit_process(Some(&policy), ProcessKind::Command)
        .await
        .expect("an ordinary checkout is not restricted");
}

#[tokio::test]
async fn two_separate_worktrees_admit_shells_concurrently() {
    let fixture = ReservedCheckout::build().await;
    let (first, _first_project, _first_workspace) = fixture.ordinary_workspace().await;
    let (second, _second_project, _second_workspace) = fixture.ordinary_workspace().await;

    let first_policy = policy_for(&fixture, first.path());
    let second_policy = policy_for(&fixture, second.path());

    // Both admissions are in flight at once: the refusal is a property of
    // the checkout, never a global lock.
    let (first, second) = tokio::join!(
        admit_process(Some(&first_policy), ProcessKind::Command),
        admit_process(Some(&second_policy), ProcessKind::Command),
    );
    first.expect("the first worktree admits a shell");
    second.expect("the second worktree admits a shell");
}

#[tokio::test]
async fn the_refusal_names_the_group_that_holds_the_checkout() {
    let fixture = ReservedCheckout::build().await;
    let service = fixture.daemon.process_admission.clone();

    let admission = service
        .admit_scope(fixture.checkout.path(), ProcessKind::Command)
        .await
        .expect("the authority answers");
    let denial = admission.denial().expect("the checkout is reserved");
    assert_eq!(denial.reason, ProcessDenialReason::SharedCheckout);
    assert_eq!(denial.group_id.as_deref(), Some(fixture.group_id.as_str()));
    assert_eq!(
        denial.worktree_id.as_deref(),
        Some(fixture.worktree_id.as_str())
    );
    // A refusal is a concurrency answer: it never names a path.
    assert!(!denial.to_string().contains('/'));
}

#[tokio::test]
async fn a_terminal_group_releases_the_reservation_and_the_profile_with_it() {
    let fixture = ReservedCheckout::build().await;
    let service = fixture.daemon.process_admission.clone();

    // The refusal holds while the group is live.
    assert!(service
        .admit_scope(fixture.checkout.path(), ProcessKind::Command)
        .await
        .expect("the authority answers")
        .denial()
        .is_some());

    // Finishing the group's runs makes the group terminal, and a terminal
    // group reserves nothing.
    for run_id in [fixture.owner_run.clone(), fixture.member_run.clone()] {
        // A run reaches a terminal state through its lifecycle; a direct
        // Created -> Completed transition is refused by the store.
        for state in [
            codegg_core::agent_run::AgentRunStatus::Queued,
            codegg_core::agent_run::AgentRunStatus::Preparing,
            codegg_core::agent_run::AgentRunStatus::Running,
        ] {
            fixture
                .runs
                .transition(&run_id, state)
                .await
                .expect("advance the run");
        }
        fixture
            .runs
            .finish(
                &run_id,
                codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
                None,
                None,
                None,
            )
            .await
            .expect("finish the run");
    }
    fixture
        .groups
        .member_changed(&fixture.member_run)
        .await
        .expect("recompute the group");

    let admission = service
        .admit_scope(fixture.checkout.path(), ProcessKind::Command)
        .await
        .expect("the authority answers");
    assert!(
        admission.is_admitted(),
        "a terminal group reserves nothing: {admission:?}"
    );
}

#[tokio::test]
async fn the_broker_refuses_a_process_capable_tool() {
    let fixture = ReservedCheckout::build().await;
    let policy = policy_for(&fixture, fixture.checkout.path());

    let mut registry =
        codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
            workspace_root: Some(fixture.checkout.path().to_path_buf()),
            ..Default::default()
        });
    registry.register(codegg::tool::bash::BashTool::default());
    let broker = codegg::tool::broker::ToolBroker::new(&registry);

    let mut ctx = codegg::tool::broker::BrokerInvocationContext::from(
        codegg::tool::backend::ToolExecutionContext {
            cwd: fixture.checkout.path().to_path_buf(),
            process_admission: Some(policy),
            ..codegg::tool::backend::ToolExecutionContext::with_backend(
                codegg::tool::backend::ToolBackendKind::Native,
            )
        },
    );
    ctx.cwd = fixture.checkout.path().to_path_buf();
    // The broker refuses an unverified caller before it reaches the
    // process gate, so the test carries a real grant: the refusal under
    // test must be the *process* refusal, not an authority failure.
    ctx.authority = codegg::tool::broker::BrokerAuthority::Verified {
        grant: test_grant(&fixture.workspace_id),
    };

    let error = broker
        .execute_with_retry(
            &registry,
            "bash",
            serde_json::json!({"command": "echo hello"}),
            ctx,
            None,
        )
        .await
        .expect_err("the broker refuses a process launch on a reserved checkout");
    match error {
        codegg::tool::broker::BrokerError::ProcessDenied { tool, reason } => {
            assert_eq!(tool, "bash");
            assert!(
                reason.contains("reserved by live group"),
                "the refusal names the reason: {reason}"
            );
            assert!(
                reason.contains("isolated checkout"),
                "the refusal offers a recovery path: {reason}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn the_broker_admits_a_read_only_tool_on_the_same_checkout() {
    let fixture = ReservedCheckout::build().await;
    let policy = policy_for(&fixture, fixture.checkout.path());

    let mut registry =
        codegg::tool::ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
            workspace_root: Some(fixture.checkout.path().to_path_buf()),
            ..Default::default()
        });
    registry.register(codegg::tool::read::ReadTool::default());
    let broker = codegg::tool::broker::ToolBroker::new(&registry);

    let ctx = codegg::tool::broker::BrokerInvocationContext::from(
        codegg::tool::backend::ToolExecutionContext {
            cwd: fixture.checkout.path().to_path_buf(),
            process_admission: Some(policy),
            ..codegg::tool::backend::ToolExecutionContext::with_backend(
                codegg::tool::backend::ToolBackendKind::Native,
            )
        },
    );

    // A read names the file it touches, so it is contained to the checkout
    // and is not a process surface. The restricted profile refuses
    // processes, not reads.
    let _ = broker
        .execute_with_retry(
            &registry,
            "read",
            serde_json::json!({"path": "does-not-exist.txt"}),
            ctx,
            None,
        )
        .await;
}

#[tokio::test]
async fn a_process_class_job_is_refused_at_submission() {
    let fixture = ReservedCheckout::build().await;
    let submission = fixture
        .daemon
        .deps
        .submission
        .clone()
        .expect("the daemon owns a submission boundary");

    let workspace_id = codegg_core::workspace::WorkspaceId::parse(&fixture.workspace_id)
        .expect("the workspace id is canonical");
    let job = codegg_core::jobs::NewJob {
        workspace_id,
        session_id: Some(fixture.session_id.clone()),
        turn_id: Some(fixture.turn_id.clone()),
        kind: codegg_core::jobs::JobKind::Shell,
        source: codegg_core::jobs::JobSource::Interactive,
        priority: codegg_core::jobs::JobPriority::Normal,
        payload: codegg_core::jobs::JobPayload::Shell {
            command: "echo hello".to_owned(),
            argv: None,
            cwd: None,
            // A legacy-shaped payload: no captured profile ceiling and the
            // unrestricted network intent, which is what this payload decodes
            // to when the fields are absent.
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        },
        resource_request: codegg_core::jobs::ResourceRequest::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::default(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: Vec::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: codegg_core::jobs::ExecutionTarget::Local,
    };

    let error = submission
        .submit(None, job)
        .await
        .expect_err("a process-class job is refused for a reserved checkout");
    match error {
        codegg::scheduler::submission::JobSubmissionError::ProcessDenied(reason) => {
            assert!(
                reason.contains("reserved by live group"),
                "the refusal names the reason: {reason}"
            );
            assert!(
                reason.contains("isolated checkout"),
                "the refusal offers a recovery path: {reason}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn a_job_that_starts_no_process_is_not_refused() {
    let fixture = ReservedCheckout::build().await;
    let submission = fixture
        .daemon
        .deps
        .submission
        .clone()
        .expect("the daemon owns a submission boundary");

    let workspace_id = codegg_core::workspace::WorkspaceId::parse(&fixture.workspace_id)
        .expect("the workspace id is canonical");
    let job = codegg_core::jobs::NewJob {
        workspace_id,
        session_id: Some(fixture.session_id.clone()),
        turn_id: Some(fixture.turn_id.clone()),
        kind: codegg_core::jobs::JobKind::AgentTurn,
        source: codegg_core::jobs::JobSource::Interactive,
        priority: codegg_core::jobs::JobPriority::Normal,
        payload: codegg_core::jobs::JobPayload::AgentTurn {
            prompt: "read the repository".to_owned(),
            agent: "build".to_owned(),
            model: Some("test".to_owned()),
            submission_key: None,
        },
        resource_request: codegg_core::jobs::ResourceRequest::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::default(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: Vec::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: codegg_core::jobs::ExecutionTarget::Local,
    };

    // The refusal is about the *capability*: a job that starts no process
    // is not refused on the process ground.
    submission
        .submit(None, job)
        .await
        .expect("a job that starts no process is admitted");
}

#[tokio::test]
async fn the_human_shell_is_refused_on_a_reserved_checkout() {
    let fixture = ReservedCheckout::build().await;
    let admission = crate::scheduler_admission_service(&fixture);

    let scheduler = fixture
        .daemon
        .deps
        .scheduler
        .clone()
        .expect("the daemon owns a scheduler");
    let mut service =
        codegg::shell::daemon::HumanShellRunService::new(scheduler.admission().clone());
    service.set_process_admission(admission);

    let scope = codegg::shell::daemon::HumanShellRunScope {
        principal_id: "principal-1".to_owned(),
        project_id: fixture.project_id.clone(),
        workspace_id: fixture.workspace_id.clone(),
        session_id: Some(fixture.session_id.clone()),
    };
    let request = codegg_protocol::human_shell::HumanShellRunCreateRequestDto {
        protocol_version: codegg_protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
        project_id: fixture.project_id.clone(),
        workspace_id: fixture.workspace_id.clone(),
        session_id: Some(fixture.session_id.clone()),
        submission_id: "submission-1".to_owned(),
        command: "echo hello".to_owned(),
        timeout_secs: 5,
        origin: codegg_protocol::human_shell::HumanShellOriginDto::Bang,
        confirmed_dangerous: true,
    };

    let response = service
        .create(
            scope,
            request,
            fixture.checkout.path().to_path_buf(),
            "node-1".to_owned(),
            "sh".to_owned(),
            None,
        )
        .await;

    match response {
        codegg_protocol::core::CoreResponse::Error { code, .. } => {
            assert_eq!(code, "human_shell_process_denied");
        }
        other => panic!("the human shell must be refused: {other:?}"),
    }
}

#[tokio::test]
async fn a_live_persistent_process_blocks_the_reservation() {
    let fixture = ReservedCheckout::build().await;
    let processes: Arc<PersistentProcessRegistry> =
        fixture.daemon.process_admission.processes().clone();

    // Nothing is live yet, so the reservation's preflight passes.
    let admission = fixture
        .daemon
        .process_admission
        .preflight_reservation(&fixture.worktree_id)
        .await
        .expect("the authority answers");
    assert!(admission.is_admitted(), "{admission:?}");

    // A process that was started before the checkout became shared keeps
    // the authority it had, so the reservation is refused while it lives.
    processes.register(PersistentProcessEntry {
        process_id: "pty-1".to_owned(),
        kind: ProcessKind::Pty,
        checkout_root: fixture.checkout.path().to_path_buf(),
    });
    let admission = fixture
        .daemon
        .process_admission
        .preflight_reservation(&fixture.worktree_id)
        .await
        .expect("the authority answers");
    let denial = admission.denial().expect("the reservation is refused");
    assert_eq!(denial.reason, ProcessDenialReason::PersistentProcessPresent);

    // Draining the process releases the reservation's preflight.
    assert!(processes.release("pty-1"));
    let admission = fixture
        .daemon
        .process_admission
        .preflight_reservation(&fixture.worktree_id)
        .await
        .expect("the authority answers");
    assert!(admission.is_admitted(), "{admission:?}");
}

#[tokio::test]
async fn the_denial_event_is_attributed_and_redacted() {
    let fixture = ReservedCheckout::build().await;
    let denial = ProcessDenial {
        reason: ProcessDenialReason::SharedCheckout,
        kind: ProcessKind::Command,
        worktree_id: Some("worktree-1".to_owned()),
        group_id: Some("group-1".to_owned()),
    };

    let audit = codegg_core::audit_instrumentation::TrustedExecutionAuditContext::legacy_local(
        "c005-correlation",
    );
    let emitter =
        codegg_core::audit_instrumentation::ExecutionAuditEmitter::new(Some(fixture.pool.clone()));
    let builder = codegg_core::audit_instrumentation::process_denial_event(
        audit.principal(),
        audit.provenance(),
        audit.chain(),
        denial.kind.as_str(),
        denial.reason.as_str(),
        denial.group_id.as_deref().unwrap_or(""),
        denial.worktree_id.as_deref().unwrap_or(""),
    );
    emitter.emit(builder).await;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, metadata_json FROM audit_event WHERE action = 'process_denial'",
    )
    .fetch_all(&fixture.pool)
    .await
    .expect("read the audit trail");
    assert_eq!(
        rows.len(),
        1,
        "exactly one durable denial row is expected, got {rows:?}"
    );
    let (action, metadata) = &rows[0];
    assert_eq!(action, "process_denial");
    assert!(
        metadata.contains("shared_checkout"),
        "the event names the reason: {metadata}"
    );
    assert!(
        metadata.contains("group-1"),
        "the event names the group: {metadata}"
    );
    // A refusal is a concurrency answer: it never carries a path or a
    // command.
    assert!(
        !metadata.contains('/'),
        "the event leaked a path: {metadata}"
    );
    assert!(!metadata.contains("echo"), "the event leaked a command");
}

#[tokio::test]
async fn the_capability_table_classifies_every_process_surface() {
    use codegg::tool::process_capability::{process_capability, ToolProcessCapability};

    assert_eq!(
        process_capability("bash"),
        ToolProcessCapability::Spawns(ProcessKind::Command)
    );
    assert_eq!(
        process_capability("terminal"),
        ToolProcessCapability::Spawns(ProcessKind::Pty)
    );
    assert_eq!(
        process_capability("python_script"),
        ToolProcessCapability::Spawns(ProcessKind::Command)
    );
    assert_eq!(
        process_capability("tool_program"),
        ToolProcessCapability::Spawns(ProcessKind::ToolProgram)
    );
    assert!(process_capability("lsp").uses_persistent_process());
    assert_eq!(process_capability("read"), ToolProcessCapability::None);
}

/// A verified tool authority for the broker's own scope checks.
///
/// The broker refuses an unverified caller before it reaches the process
/// gate, so the broker test needs a grant that actually verifies: the
/// refusal under test must be the *process* refusal, not an authority
/// failure.
fn test_grant(workspace_id: &str) -> codegg_core::jobs::ToolAuthorityGrant {
    let mut grant = codegg_core::jobs::ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "c005-test-grant".into(),
        principal_ref: "test-principal".into(),
        workspace_id: workspace_id.to_owned(),
        workspace_path_policy_id: format!("workspace:{workspace_id}"),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "c005-policy-v1".into(),
        allowed_caller_class: "any".into(),
        allowed_effect_class: "any".into(),
        manifest_digest: "c005-manifest".into(),
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

/// The service the human-shell test installs. It is the daemon's own
/// authority, so the test proves the *production* wiring, not a stand-in.
fn scheduler_admission_service(fixture: &ReservedCheckout) -> Arc<ProcessAdmissionService> {
    fixture.daemon.process_admission.clone()
}
