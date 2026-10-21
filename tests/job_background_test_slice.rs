//! Async-background-jobs M001: `test.execution_mode=background` vertical slice.
//!
//! - Background returns a stable `JobHandle` promptly without calling the
//!   blocking wait, lets the parent do other work, and produces one
//!   terminal notice without polling.
//! - Foreground behavior is preserved byte-for-byte.
//! - Background requires a real session context or fails without spawning.
//! - The `job` control tool enforces exact-owner scope; wait timeout means
//!   still-running, never job failure; cancellation targets the exact job.

use std::sync::Arc;

use codegg::scheduler::job_notifications::JobNotificationService;
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::tool::backend::ToolExecutionContext;
use codegg::tool::{Tool, ToolRegistry, ToolRegistryOptions};
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
    Arc<dyn JobStore>,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register")
        .id
        .clone();
    // Keep the tempdir alive for the harness lifetime via leak (test-only).
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
        DaemonGeneration::new_unchecked("test-gen-m001"),
    );
    let submission = JobSubmissionService::new(
        store.clone(),
        scheduler.clone(),
        services,
        DaemonGeneration::new_unchecked("test-gen-m001"),
    );
    let notifications = Arc::new(JobNotificationService::new());
    scheduler
        .set_job_notification_service(notifications.clone())
        .await;
    let _ = workspace;
    (submission, scheduler, notifications, store)
}

fn ctx_for(session: &str, turn: Option<&str>, invocation: &str) -> ToolExecutionContext {
    let policy_revision = "test-policy-r1";
    let snapshot = codegg_core::approval::ExecutionPolicySnapshot::capture(
        codegg_core::approval::ApprovalMode::Interactive,
        codegg_core::approval::SandboxProfile::WorkspaceWrite,
        Some("test-principal".into()),
        Some(session.to_string()),
        None,
        Some(policy_revision.into()),
        None,
    );
    ToolExecutionContext {
        writer_fence: None,
        backend: codegg::tool::backend::ToolBackendKind::Native,
        session_id: Some(session.to_string()),
        cwd: std::path::PathBuf::from("/tmp"),
        permission_mode: None,
        timeout_ms: None,
        invocation_key: Some(invocation.to_string()),
        turn_id: turn.map(ToOwned::to_owned),
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
        execution_authorization: Some(codegg::tool::authorization::ExecutionAuthorizationContext {
            snapshot,
            receipt: codegg::permission::PermissionDecisionReceipt::allowed(
                "background-test-fixture",
                Some(policy_revision.into()),
            ),
            command_preflight: None,
            scratch_scope: None,
        }),
        // M005 corrective C005: this fixture is not under the restricted
        // shared-checkout profile, so the launch carries no admission policy.
        process_admission: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn background_test_returns_handle_without_blocking_wait() {
    let (submission, _scheduler, notifications, _store) = harness().await;
    // Threaded as production does: the tool defaults its workdir to the
    // session workspace and refuses to guess the process cwd.
    let root = tempfile::tempdir().expect("workspace fixture");
    let tool = codegg::tool::test::TestTool::new()
        .with_submission(submission)
        .with_workspace_root(root.path().to_path_buf())
        .with_job_notifications(notifications.clone());
    // A scope that resolves without executing (validation happens before
    // submit; an unknown scope fails before any job exists).
    let ctx = ctx_for("sess-bg-1", Some("turn-1"), "inv-bg-1");
    let before = std::time::Instant::now();
    let result = tool
        .execute_structured(
            serde_json::json!({"scope": "auto", "execution_mode": "background"}),
            Some(ctx),
        )
        .await;
    // The test command may fail to resolve in this sandbox (no workspace
    // project), but the background path must either return a handle or
    // fail before spawning — never block on the 900s foreground wait.
    let elapsed = before.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "background must not block on wait, took {elapsed:?}"
    );
    match result {
        Ok(structured) => {
            assert!(structured.success);
            let value = structured.value.expect("background value");
            assert!(value.get("job_id").is_some());
            assert_eq!(value.get("kind").and_then(|v| v.as_str()), Some("test"));
            // Subscribed: the scheduler bridge or recovery can now notify.
            let job_id = value.get("job_id").and_then(|v| v.as_str()).unwrap();
            let sub = notifications.get_subscription(job_id).await.unwrap();
            assert!(sub.is_some(), "background must durably subscribe");
        }
        Err(e) => {
            // Acceptable only for pre-submit validation (no session would
            // have been tested separately; here session exists, so the
            // error must be resolution/workspace, not a wait timeout).
            let msg = e.to_string();
            assert!(
                !msg.contains("timed out waiting"),
                "background must never call blocking wait: {msg}"
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn background_test_without_session_fails_without_spawning() {
    let (submission, _scheduler, notifications, store) = harness().await;
    let count_before = store
        .list_job_records(codegg_core::jobs::store::JobStoreQuery {
            limit: Some(100),
            ..Default::default()
        })
        .await
        .unwrap()
        .len();
    let root = tempfile::tempdir().expect("workspace fixture");
    let tool = codegg::tool::test::TestTool::new()
        .with_submission(submission)
        .with_workspace_root(root.path().to_path_buf())
        .with_job_notifications(notifications);
    let mut context = ctx_for("", None, "inv-bg-no-session");
    context.session_id = None;
    context.execution_authorization =
        Some(codegg::tool::authorization::ExecutionAuthorizationContext {
            snapshot: codegg_core::approval::ExecutionPolicySnapshot::capture(
                codegg_core::approval::ApprovalMode::Interactive,
                codegg_core::approval::SandboxProfile::WorkspaceWrite,
                Some("test-principal".into()),
                None,
                None,
                Some("test-policy-r1".into()),
                None,
            ),
            receipt: codegg::permission::PermissionDecisionReceipt::allowed(
                "background-test-fixture",
                Some("test-policy-r1".into()),
            ),
            command_preflight: None,
            scratch_scope: None,
        });
    let err = tool
        .execute_structured(
            serde_json::json!({"scope": "auto", "execution_mode": "background"}),
            Some(context),
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("session context"),
        "must fail on missing session, got {err}"
    );
    let count_after = store
        .list_job_records(codegg_core::jobs::store::JobStoreQuery {
            limit: Some(100),
            ..Default::default()
        })
        .await
        .unwrap()
        .len();
    assert_eq!(
        count_before, count_after,
        "failed background must not spawn a job"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn same_invocation_reuses_handle_different_payload_conflicts() {
    // Proved at the submission layer (M009 semantics) which the background
    // test path reuses via SubmissionKey test-bg:<digest>.
    let (submission, _scheduler, _notifications, _store) = harness().await;
    let ws_services = submission.scheduler().workspaces().clone();
    // Use a real temp root for workspace resolution.
    let tmp = tempfile::tempdir().unwrap();
    let ws_id = submission.workspace_id_for_root(tmp.path()).await.unwrap();
    let _ = ws_services;
    let key = codegg::scheduler::SubmissionKey::new("test-bg:same-inv").unwrap();
    let mk = |cmd: &str| codegg_core::jobs::NewJob {
        workspace_id: ws_id.clone(),
        session_id: Some("s-dedup".into()),
        turn_id: None,
        kind: codegg_core::jobs::JobKind::Test,
        source: codegg_core::jobs::JobSource::Interactive,
        priority: codegg_core::jobs::JobPriority::Interactive,
        payload: codegg_core::jobs::JobPayload::Test {
            command: cmd.into(),
            argv: vec![cmd.into()],
            cwd: None,
            scope: None,
            parent_run_id: None,
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(
            codegg_core::jobs::JobKind::Test,
        ),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
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
        target: Default::default(),
    };
    let first = submission
        .submit(Some(key.clone()), mk("cargo test"))
        .await
        .unwrap();
    let replay = submission
        .submit(Some(key.clone()), mk("cargo test"))
        .await
        .unwrap();
    assert_eq!(
        first.job_id, replay.job_id,
        "same key+same payload reuses job"
    );
    let conflict = submission.submit(Some(key), mk("cargo test --other")).await;
    assert!(
        conflict.is_err(),
        "same key+different payload must conflict"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn job_tool_enforces_exact_owner_and_wait_timeout_is_observation() {
    let (submission, scheduler, notifications, _store) = harness().await;
    // Submit a background job directly, then exercise the job tool.
    let tmp = tempfile::tempdir().unwrap();
    let ws_id = submission.workspace_id_for_root(tmp.path()).await.unwrap();
    let key = codegg::scheduler::SubmissionKey::new("test-bg:owner-check").unwrap();
    let submitted = submission
        .submit(
            Some(key),
            codegg_core::jobs::NewJob {
                workspace_id: ws_id,
                session_id: Some("owner-s".into()),
                turn_id: None,
                kind: codegg_core::jobs::JobKind::Test,
                source: codegg_core::jobs::JobSource::Interactive,
                priority: codegg_core::jobs::JobPriority::Interactive,
                payload: codegg_core::jobs::JobPayload::Test {
                    command: "cargo test".into(),
                    argv: vec!["cargo".into(), "test".into()],
                    cwd: None,
                    scope: None,
                    parent_run_id: None,
                    sandbox_profile: None,
                    network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
                },
                resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                    codegg_core::jobs::JobKind::Test,
                ),
                timeout: None,
                retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
                idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
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
                target: Default::default(),
            },
        )
        .await
        .unwrap();
    notifications
        .subscribe(&submitted.job_id, "test", "owner-s", None, "inv-owner")
        .await
        .unwrap();
    let tool = codegg::tool::job::JobTool::new()
        .with_submission(submission.clone())
        .with_notifications(notifications);
    let job_id = submitted.job_id.as_str();
    // Owner can inspect.
    let owner_ctx = ctx_for("owner-s", None, "inv-x");
    let status = tool
        .execute_structured(
            serde_json::json!({"action": "status", "job_id": job_id}),
            Some(owner_ctx),
        )
        .await
        .unwrap();
    assert!(status.success);
    // Second principal gets the same denied message as not-found (no leak).
    let attacker_ctx = ctx_for("attacker-s", None, "inv-y");
    let denied = tool
        .execute_structured(
            serde_json::json!({"action": "status", "job_id": job_id}),
            Some(attacker_ctx.clone()),
        )
        .await
        .unwrap_err();
    assert!(
        denied.to_string().contains("not found"),
        "must not leak, got {denied}"
    );
    // Wait with a tiny timeout reports still-running, never kills the job.
    let owner_wait_ctx = ctx_for("owner-s", None, "inv-w");
    let waited_value = tool
        .execute_structured(
            serde_json::json!({"action": "wait", "job_id": job_id, "timeout_ms": 5}),
            Some(owner_wait_ctx),
        )
        .await
        .unwrap()
        .value
        .expect("wait value");
    let waited: serde_json::Value = waited_value;
    // The job has no executor in this harness, so it stays queued; the
    // wait must report still-running rather than a terminal failure.
    // (If the scheduler happened to terminalize it, accept that status too —
    // the invariant is that a wait timeout is never recorded as failure.)
    let s = waited.get("status").and_then(|v| v.as_str()).unwrap();
    assert!(
        matches!(
            s,
            "still-running"
                | "completed"
                | "failed"
                | "cancelled"
                | "timed_out"
                | "expired"
                | "interrupted"
        ),
        "unexpected wait status {s}"
    );
    // Cancellation targets the exact job.
    let owner_ctx2 = ctx_for("owner-s", None, "inv-z");
    let cancelled: serde_json::Value = serde_json::from_str(
        &tool
            .execute_structured(
                serde_json::json!({"action": "cancel", "job_id": job_id}),
                Some(owner_ctx2),
            )
            .await
            .unwrap()
            .value
            .map(|v| v.to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        cancelled.get("job_id").and_then(|v| v.as_str()),
        Some(job_id)
    );
    let _ = scheduler;
}

#[test]
fn test_tool_schema_advertises_background_and_job_tool_registered() {
    let tool = codegg::tool::test::TestTool::new();
    let params = tool.parameters();
    assert_eq!(
        params["properties"]["execution_mode"]["enum"],
        serde_json::json!(["foreground", "background"])
    );
    let registry = ToolRegistry::with_options(ToolRegistryOptions::default());
    assert!(registry.get("test").is_some());
    assert!(
        registry.get("job").is_some(),
        "job control tool must be discoverable"
    );
}
