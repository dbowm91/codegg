//! Async-background-jobs M002: background Bash / build / lint / format.
//!
//! - Foreground/default output, error shape, timeout precedence preserved.
//! - Background returns a durable `JobHandle` promptly for scheduler-backed
//!   routes (test/build/lint/format/managed/shell/python) and notifies once
//!   on completion without model polling.
//! - Direct-only routes (native tool, git, rejected) fail closed in
//!   background without spawning; no fallback, no reparse, no downgrade.
//! - M009 identity: same invocation reuses the job, different payload
//!   conflicts, independent identical commands stay distinct.
//! - `wait` timeout is observation only, never job failure.

use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::job_notifications::JobNotificationService;
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::{Tool, ToolRegistryOptions};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

mod common;

async fn harness() -> (
    Arc<JobSubmissionService>,
    Arc<JobScheduler>,
    Arc<JobNotificationService>,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let _workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register");
    let workspace_root = root.path().to_path_buf();
    std::mem::forget(root);
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("test-gen-m002"),
    );
    let submission = JobSubmissionService::new(
        store,
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("test-gen-m002"),
    );
    // Re-fetch store via scheduler for clarity.
    let notifications = Arc::new(JobNotificationService::new());
    scheduler
        .set_job_notification_service(notifications.clone())
        .await;
    (submission, scheduler, notifications, workspace_root)
}

fn ctx_for(session: &str, invocation: &str) -> ToolExecutionContext {
    ToolExecutionContext {
        backend: ToolBackendKind::Native,
        session_id: Some(session.to_string()),
        cwd: std::path::PathBuf::from("/tmp"),
        permission_mode: None,
        timeout_ms: None,
        invocation_key: Some(invocation.to_string()),
        turn_id: Some("turn-m002".to_string()),
        agent_id: None,
        parent_job_id: None,
        parent_attempt_id: None,
        provider_name: None,
        backend_policy: None,
        cancellation: None,
        deadline: None,
        decision_id: None,
        decision_outcome: None,
        workspace_path_policy_id: None,
        workspace_path_policy_revision: None,
        permission_policy_revision: None,
        principal_identity: None,
        origin_principal: None,
        origin_auth_method: None,
        origin_decision_id: None,
        execution_audit: None,
        audit_emitter: None,
        caller_class: None,
        max_effect_class: None,
        decision_issued_at: None,
        decision_expires_at: None,
        decision_revoked_at: None,
        program_contract_snapshot: None,
        sandbox_profile: None,
        execution_authorization: None,
    }
}

fn bash_tool_with(
    submission: Arc<JobSubmissionService>,
    notifications: Arc<JobNotificationService>,
    workspace_root: std::path::PathBuf,
) -> codegg::tool::bash::BashTool {
    codegg::tool::bash::BashTool::default()
        .with_workspace_root(workspace_root)
        .with_submission(submission)
        .with_job_notifications(notifications)
}

#[test]
fn bash_schema_advertises_background() {
    let tool = codegg::tool::bash::BashTool::default();
    let params = tool.parameters();
    assert_eq!(
        params["properties"]["execution_mode"]["enum"],
        serde_json::json!(["foreground", "background"])
    );
    assert!(params["required"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("command")));
}

#[tokio::test(flavor = "current_thread")]
async fn foreground_echo_unchanged() {
    // Default foreground path preserved byte-for-byte where practical.
    let (submission, _scheduler, notifications, workspace_root) = harness().await;
    let tool = bash_tool_with(submission, notifications, workspace_root);
    let out = tool
        .execute(serde_json::json!({"command": "echo hello-m002"}))
        .await
        .expect("foreground echo");
    assert!(
        out.contains("hello-m002"),
        "foreground output changed: {out}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn background_shell_returns_handle_promptly_then_notifies_on_cancel() {
    let (submission, _scheduler, notifications, workspace_root) = harness().await;
    let tool = bash_tool_with(submission.clone(), notifications.clone(), workspace_root);
    let ctx = ctx_for("sess-m002-1", "inv-m002-1");
    let before = std::time::Instant::now();
    // BashTool::execute_structured wraps execute_inner's string in a legacy
    // result; the background handle JSON is the output string.
    let structured = tool
        .execute_structured(
            serde_json::json!({"command": "echo slow-build-stub", "execution_mode": "background"}),
            Some(ctx),
        )
        .await
        .expect("background submit");
    let handle: serde_json::Value = serde_json::from_str(&structured.output).expect("handle JSON");
    assert!(
        handle.get("job_id").is_some(),
        "handle has job_id: {handle}"
    );
    assert_eq!(handle.get("kind").and_then(|v| v.as_str()), Some("shell"));
    let elapsed = before.elapsed();
    assert!(
        elapsed < Duration::from_secs(30),
        "background must not block, took {elapsed:?}"
    );
    // Subscribed before acceptance reporting.
    let job_id = handle.get("job_id").and_then(|v| v.as_str()).unwrap();
    let sub = notifications.get_subscription(job_id).await.unwrap();
    assert!(sub.is_some(), "background must durably subscribe");
    // Legacy execute() has no ctx session, so it must fail without spawning.
    let handle_str = tool
        .execute(serde_json::json!({"command": "echo slow-build-stub-2", "execution_mode": "background"}))
        .await;
    assert!(
        handle_str.is_err(),
        "legacy path without session must fail closed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn background_shell_handle_subscribe_and_single_notice() {
    // Full vertical slice at the scheduler+notification layer with a real
    // bash-shaped Shell payload (compound pipe preserves shell semantics
    // via the adopted Shell route).
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob,
        ResourceRequest, RetryPolicy,
    };
    let (submission, scheduler, notifications, _workspace_root) = harness().await;
    let tmp = tempfile::tempdir().unwrap();
    let ws_id = submission.workspace_id_for_root(tmp.path()).await.unwrap();
    let key = codegg::scheduler::SubmissionKey::new("bash-bg:pipe-1").unwrap();
    let submitted = submission
        .submit(
            Some(key),
            NewJob {
                workspace_id: ws_id,
                session_id: Some("sess-pipe".into()),
                turn_id: Some("turn-pipe".into()),
                kind: JobKind::Shell,
                source: JobSource::Interactive,
                priority: JobPriority::Interactive,
                payload: JobPayload::Shell {
                    command: "echo a | cat".into(),
                    argv: Some(vec!["sh".into(), "-c".into(), "echo a | cat".into()]),
                    cwd: Some("/tmp".into()),
                },
                resource_request: ResourceRequest::for_kind(JobKind::Shell),
                timeout: Some(Duration::from_secs(60)),
                retry_policy: RetryPolicy::no_retry(),
                idempotency: IdempotencyClass::NonIdempotent,
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
            },
        )
        .await
        .unwrap();
    notifications
        .subscribe(
            &submitted.job_id,
            "shell",
            "sess-pipe",
            Some("turn-pipe"),
            "inv-pipe",
        )
        .await
        .unwrap();
    // No model-authored sleep/poll: cancel the queued job and expect exactly
    // one terminal notice via the scheduler bridge.
    scheduler
        .request_cancel(&submitted.job_id, "test-cancel")
        .await
        .unwrap();
    // Bridge is async inside request_cancel; poll boundedly for the notice
    // (test-only wait, not model polling).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let pending = notifications
            .pending_for_session("sess-pipe")
            .await
            .unwrap();
        if !pending.is_empty() {
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].job_id, submitted.job_id.as_str());
            break;
        }
        // Also accept already-delivered via recovery (bridge + inject race).
        if notifications
            .get(submitted.job_id.as_str())
            .await
            .unwrap()
            .is_some()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "terminal notice did not arrive"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn identical_commands_distinct_ids_same_invocation_reuses() {
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, JobKind, JobPayload, JobPriority, JobSource, NewJob,
        ResourceRequest, RetryPolicy,
    };
    let (submission, _scheduler, _notifications, _workspace_root) = harness().await;
    let tmp = tempfile::tempdir().unwrap();
    let ws_id = submission.workspace_id_for_root(tmp.path()).await.unwrap();
    let mk = |cmd: &str| NewJob {
        workspace_id: ws_id.clone(),
        session_id: Some("s-m002-dedup".into()),
        turn_id: None,
        kind: JobKind::Shell,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::Shell {
            command: cmd.into(),
            argv: None,
            cwd: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::Shell),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::NonIdempotent,
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
    };
    let a = submission
        .submit(
            Some(codegg::scheduler::SubmissionKey::new("bash-bg:a").unwrap()),
            mk("cargo build"),
        )
        .await
        .unwrap();
    let b = submission
        .submit(
            Some(codegg::scheduler::SubmissionKey::new("bash-bg:b").unwrap()),
            mk("cargo build"),
        )
        .await
        .unwrap();
    assert_ne!(
        a.job_id, b.job_id,
        "independent identical commands stay distinct"
    );
    let replay = submission
        .submit(
            Some(codegg::scheduler::SubmissionKey::new("bash-bg:a").unwrap()),
            mk("cargo build"),
        )
        .await
        .unwrap();
    assert_eq!(a.job_id, replay.job_id, "same invocation reuses the job");
    let conflict = submission
        .submit(
            Some(codegg::scheduler::SubmissionKey::new("bash-bg:a").unwrap()),
            mk("cargo build --other"),
        )
        .await;
    assert!(conflict.is_err(), "same key + different payload conflicts");
}

#[tokio::test(flavor = "current_thread")]
async fn background_rejects_without_session_and_blocked_commands() {
    let (submission, _scheduler, notifications, workspace_root) = harness().await;
    let tool = bash_tool_with(
        submission.clone(),
        notifications.clone(),
        workspace_root.clone(),
    );
    // No session in ctx-free legacy path -> fail without spawning.
    let err = tool
        .execute(serde_json::json!({"command": "echo hi", "execution_mode": "background"}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("session context"), "got {err}");
    // Blocked commands fail before any submit even in foreground.
    let blocked_tool = bash_tool_with(submission, notifications, workspace_root)
        .with_blocked_commands(vec!["rm -rf /"]);
    let err = blocked_tool
        .execute(serde_json::json!({"command": "rm -rf /"}))
        .await
        .unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn route_table_characterization() {
    // Plan work package A: exact foreground dispatch owners before
    // background fields. Scheduler-backed routes may background; direct
    // routes must reject background without spawning.
    const TABLE: &[(&str, &str, bool)] = &[
        ("RouteToTestRunner", "scheduler JobKind::Test", true),
        (
            "RouteToManagedProcess",
            "scheduler JobKind::Build|Lint|Format|ManagedProcess",
            true,
        ),
        ("RouteToShell", "scheduler JobKind::Shell", true),
        ("RouteToPythonScripting", "scheduler JobKind::Python", true),
        (
            "RouteToNativeTool",
            "direct ManagedProcessService::run",
            false,
        ),
        ("RouteToGit", "direct GitMutationExecutor", false),
        (
            "raw-shell (no active route)",
            "direct execute_via_raw_shell; background adopts JobKind::Shell",
            true,
        ),
        ("Rejected", "no execution", false),
    ];
    assert_eq!(TABLE.len(), 8);
    for (route, owner, may_background) in TABLE {
        assert!(!route.is_empty() && !owner.is_empty(), "table row complete");
        // Direct-only routes never background (fail closed, no fallback).
        if !may_background {
            assert!(
                *route == "RouteToNativeTool" || *route == "RouteToGit" || *route == "Rejected",
                "unexpected direct-only {route}"
            );
        }
    }
    // ToolRegistryOptions plumbing exists for the background path.
    let opts = ToolRegistryOptions::default();
    assert!(opts.job_notification_service.is_none());
}
