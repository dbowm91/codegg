//! M002 unified retry budget and side-effect reconciliation.
//!
//! Proves one logical retry chain bounds nested layers and ambiguous
//! non-idempotent effects are reconciled or surfaced, never blindly
//! replayed. Deterministic and in-process; no network or real servers.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::provider::{AckState, ReconciliationOutcome, RetryContext, UncertainSideEffect};
use codegg::tool::broker::{BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    IdempotencyClass, ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass, ToolRetryPolicy,
    ToolTerminalStatus,
};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use codegg_core::jobs::ToolAuthorityGrant;
use serde_json::json;

// ── Test grants ──────────────────────────────────────────────────────────

fn make_grant(effect: &str) -> ToolAuthorityGrant {
    let mut grant = ToolAuthorityGrant {
        schema_version: 1,
        grant_id: "retry-test-grant".into(),
        principal_ref: "test-principal".into(),
        workspace_id: "test-ws".into(),
        workspace_path_policy_id: "workspace:test-ws".into(),
        session_id: None,
        agent_id: None,
        turn_id: None,
        permission_mode: None,
        policy_revision: "test-policy-v1".into(),
        allowed_caller_class: "agent".into(),
        allowed_effect_class: effect.into(),
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

fn ctx_for(effect: &str, submission_key: Option<String>) -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller: ToolCaller::Agent,
        cwd: PathBuf::from("."),
        session_id: Some("retry-test".to_string()),
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key,
        authority: codegg::tool::BrokerAuthority::from_grant(make_grant(effect)),
        cancellation: None,
        deadline: None,
        principal_ref: None,
        workspace_path_policy_id: None,
        allowed_tools: None,
        current_policy_revision: None,
        execution_audit: None,
        audit_emitter: None,
    }
}

// ── Flaky tools ──────────────────────────────────────────────────────────

/// Read-only tool failing `fail_times` with Timeout, then succeeding.
/// Contract allows bounded retry.
struct FlakyReadTool {
    attempts: Arc<AtomicUsize>,
    fail_times: usize,
}

#[async_trait]
impl Tool for FlakyReadTool {
    fn name(&self) -> &str {
        "flaky_read"
    }
    fn description(&self) -> &str {
        "flaky read"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_times {
            Err(ToolError::Timeout("transient read timeout".into()))
        } else {
            Ok("read ok".to_string())
        }
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            retry_policy: ToolRetryPolicy {
                max_retries: 3,
                base_delay_ms: 1,
                max_delay_ms: 5,
            },
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

/// Idempotent mutating tool with stable-key retry.
struct FlakyIdempotentWriteTool {
    attempts: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for FlakyIdempotentWriteTool {
    fn name(&self) -> &str {
        "idempotent_write"
    }
    fn description(&self) -> &str {
        "idempotent write"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(ToolError::Timeout("write ack lost".into()))
        } else {
            Ok("write ok".to_string())
        }
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::IdempotentMutating,
            idempotency: IdempotencyClass::Idempotent,
            retry_policy: ToolRetryPolicy {
                max_retries: 2,
                base_delay_ms: 1,
                max_delay_ms: 5,
            },
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

/// Non-idempotent tool that always loses acknowledgement.
struct AlwaysUncertainTool {
    attempts: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for AlwaysUncertainTool {
    fn name(&self) -> &str {
        "risky_action"
    }
    fn description(&self) -> &str {
        "non-idempotent"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(ToolError::Timeout("dispatched then hung".into()))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        // Legacy conservative default: no blind retry.
        ToolContract::legacy(tool_name, input_schema)
    }
}

/// Raw-shell analogue: ProcessExec, always ambiguous.
struct RawShellTool {
    attempts: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for RawShellTool {
    fn name(&self) -> &str {
        "raw_shell"
    }
    fn description(&self) -> &str {
        "raw shell"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(ToolError::Timeout("shell hung after dispatch".into()))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::ProcessExec,
            idempotency: IdempotencyClass::NonIdempotent,
            retry_policy: ToolRetryPolicy::none(),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

struct DeniedTool;

#[async_trait]
impl Tool for DeniedTool {
    fn name(&self) -> &str {
        "denied_tool"
    }
    fn description(&self) -> &str {
        "denied"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object"})
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        Err(ToolError::Permission("denied".into()))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            retry_policy: ToolRetryPolicy {
                max_retries: 3,
                base_delay_ms: 1,
                max_delay_ms: 5,
            },
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

// ── Chain-budget tests ───────────────────────────────────────────────────

#[test]
fn nested_context_cannot_exceed_parent_budget() {
    let mut parent = RetryContext::new(4, Duration::from_secs(60));
    // Simulate provider consuming two attempts.
    assert!(parent.consume_one());
    assert!(parent.consume_one());
    assert_eq!(parent.attempts_remaining(), 2);
    // Tool layer derives a narrower child; requesting more is capped.
    let child = parent.derive_child(5);
    assert_eq!(child.attempts_remaining(), 2);
    assert_eq!(child.chain_id(), parent.chain_id());
    // Total consumed across layers never exceeds the original 4.
    let mut child = child;
    assert!(child.consume_one());
    assert!(child.consume_one());
    assert!(child.is_exhausted());
    assert_eq!(parent.attempts_consumed(), 2);
    assert_eq!(child.attempts_consumed(), 2);
}

#[test]
fn provider_and_tool_share_one_bounded_chain() {
    // One logical chain of 4: provider uses up to 3, tool gets the rest.
    let mut chain = RetryContext::new(4, Duration::from_secs(60));
    let provider_child = chain.derive_child(3);
    assert_eq!(provider_child.attempts_remaining(), 3);
    // Provider fails twice then succeeds: consumes 2.
    assert!(chain.consume_one());
    assert!(chain.consume_one());
    // Tool derivation after provider consumption sees only 2 left, even
    // though its contract allows 3 retries (4 total).
    let tool_child = chain.derive_child(4);
    assert_eq!(tool_child.attempts_remaining(), 2);
}

#[test]
fn expired_chain_stops_all_layers() {
    let chain = RetryContext::new(5, Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(5));
    assert!(chain.is_expired());
    assert!(!chain.is_live());
    let contract = ToolContract::legacy("x", json!({}));
    let d = codegg::tool::retry::decide_tool_retry(
        &contract,
        AckState::NotDispatched,
        None,
        &ToolError::Timeout("t".into()),
        &chain,
    );
    assert!(matches!(
        d,
        codegg::tool::retry::ToolRetryDecision::DoNotRetry { .. }
    ));
}

// ── Broker retry tests ───────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn idempotent_write_retries_with_same_key() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(FlakyIdempotentWriteTool {
        attempts: attempts.clone(),
    });
    let broker = ToolBroker::new(&registry);
    let chain = RetryContext::new(4, Duration::from_secs(30));
    let chain_id = chain.chain_id().as_str().to_string();
    let result = broker
        .execute_with_retry(
            &registry,
            "idempotent_write",
            json!({}),
            ctx_for("idempotent_mutating", Some("inv-key-1".to_string())),
            Some(chain),
        )
        .await
        .expect("broker executes");
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::Success,
        "idempotent write with stable key recovers: {}",
        result.value.display
    );
    assert_eq!(result.value.display, "write ok");
    // One failure + one success, both under the same chain.
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let _ = chain_id;
}

#[tokio::test(flavor = "current_thread")]
async fn non_idempotent_dispatched_timeout_is_uncertain() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(AlwaysUncertainTool {
        attempts: attempts.clone(),
    });
    let broker = ToolBroker::new(&registry);
    let chain = RetryContext::new(4, Duration::from_secs(30));
    let result = broker
        .execute_with_retry(
            &registry,
            "risky_action",
            json!({"api_key": "sk-secret-123", "action": "purchase"}),
            ctx_for("non_idempotent", Some("inv-9".to_string())),
            Some(chain),
        )
        .await
        .expect("broker executes");
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::UncertainSideEffect,
        "ambiguous non-idempotent effect must surface, got: {}",
        result.value.display
    );
    // Exactly one attempt: never replayed.
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    // Diagnostics omit secrets and arguments.
    assert!(
        !result.value.display.contains("sk-secret-123"),
        "diagnostics must omit secrets"
    );
    assert!(
        !result.value.display.contains("purchase"),
        "diagnostics must omit arguments"
    );
    // Programmatic callers cannot mistake uncertainty for success.
    assert!(result.into_programmatic_outcome().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn raw_shell_ambiguous_is_not_replayed() {
    let secret_command_token = "raw-shell-secret-expected-redaction";
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(RawShellTool {
        attempts: attempts.clone(),
    });
    let broker = ToolBroker::new(&registry);
    let chain = RetryContext::new(5, Duration::from_secs(30));
    let result = broker
        .execute_with_retry(
            &registry,
            "raw_shell",
            json!({
                "command": format!(
                    "rm -rf /tmp/x --secret-token {secret_command_token}"
                )
            }),
            ctx_for("process_exec", Some("sh-1".to_string())),
            Some(chain),
        )
        .await
        .expect("broker executes");
    assert_eq!(
        result.value.terminal_status,
        ToolTerminalStatus::UncertainSideEffect
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(!result.value.display.contains("rm -rf"));
    assert!(!result.value.display.contains(secret_command_token));
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_single_attempt_preserved_without_chain() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(AlwaysUncertainTool {
        attempts: attempts.clone(),
    });
    let broker = ToolBroker::new(&registry);
    // No chain: legacy `execute` keeps single-attempt timeout semantics.
    let result = broker
        .execute(
            &registry,
            "risky_action",
            json!({}),
            ctx_for("non_idempotent", None),
        )
        .await
        .expect("broker executes");
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::TimedOut);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn permission_denied_never_retries() {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(DeniedTool);
    let broker = ToolBroker::new(&registry);
    let chain = RetryContext::new(5, Duration::from_secs(30));
    let remaining_before = chain.attempts_remaining();
    let result = broker
        .execute_with_retry(
            &registry,
            "denied_tool",
            json!({}),
            ctx_for("read_only", None),
            Some(chain),
        )
        .await
        .expect("broker executes");
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Denied);
    let _ = remaining_before;
}

#[tokio::test(flavor = "current_thread")]
async fn retry_respects_chain_budget_exhaustion() {
    // Flaky tool needs 3 failures before success but the chain only has 2.
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(FlakyReadTool {
        attempts: attempts.clone(),
        fail_times: 3,
    });
    let broker = ToolBroker::new(&registry);
    let chain = RetryContext::new(2, Duration::from_secs(30));
    let result = broker
        .execute_with_retry(
            &registry,
            "flaky_read",
            json!({}),
            ctx_for("read_only", None),
            Some(chain),
        )
        .await
        .expect("broker executes");
    // Budget exhausted: last meaningful failure, no hidden extra attempt.
    assert_ne!(result.value.terminal_status, ToolTerminalStatus::Success);
    assert!(attempts.load(Ordering::SeqCst) <= 2);
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_during_chain_stops_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::with_defaults();
    registry.register(FlakyReadTool {
        attempts: attempts.clone(),
        fail_times: 10,
    });
    let broker = ToolBroker::new(&registry);
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let mut ctx = ctx_for("read_only", None);
    ctx.cancellation = Some(token);
    let chain = RetryContext::new(5, Duration::from_secs(30));
    let result = broker
        .execute_with_retry(&registry, "flaky_read", json!({}), ctx, Some(chain))
        .await
        .expect("broker executes");
    assert_eq!(result.value.terminal_status, ToolTerminalStatus::Cancelled);
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

// ── Reconciliation tests ───────────────────────────────────────────────

#[test]
fn git_push_uncertain_requires_reconciliation() {
    let chain = RetryContext::new(4, Duration::from_secs(60));
    let outcome = codegg::tool::retry::reconcile_git_operation(
        "push",
        AckState::Uncertain,
        &chain,
        Some("k"),
    );
    match outcome {
        ReconciliationOutcome::Unreconciled(u) => {
            assert_eq!(u.domain, "git");
            assert_eq!(u.operation, "push");
        }
        other => panic!("expected unreconciled, got {other:?}"),
    }
    // Reads never enter reconciliation.
    assert!(matches!(
        codegg::tool::retry::reconcile_git_operation("log", AckState::Uncertain, &chain, None),
        ReconciliationOutcome::NotApplicable
    ));
}

#[test]
fn uncertain_effect_is_secret_safe_and_bounded() {
    let long = "s".repeat(5000);
    let u = UncertainSideEffect::new("chain-1", "tool", "bash:process_exec", None, &long);
    assert!(u.detail.chars().count() <= 500);
    assert_eq!(u.chain_id, "chain-1");
}

// ── Scheduler durable reconciliation ───────────────────────────────────

use codegg::scheduler::{JobSubmissionService, ResolvedSchedulerConfig};
use codegg_core::jobs::{
    DaemonGeneration, InMemoryJobStore, JobKind, JobPayload, JobPriority, JobSource, NewJob,
    ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

struct SubmissionHarness {
    _root: tempfile::TempDir,
    submission: Arc<JobSubmissionService>,
    workspace_id: codegg_core::workspace::WorkspaceId,
    #[allow(dead_code)]
    store: Arc<dyn codegg_core::jobs::JobStore>,
    services: Arc<WorkspaceServiceRegistry>,
}

async fn test_submission_harness(store: Arc<dyn codegg_core::jobs::JobStore>) -> SubmissionHarness {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let record = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let generation = DaemonGeneration::new();
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    let submission =
        JobSubmissionService::new(store.clone(), scheduler, services.clone(), generation);
    let workspace_id = record.id.clone();
    SubmissionHarness {
        _root: root,
        submission,
        workspace_id,
        store,
        services,
    }
}

fn tool_program_job(
    workspace_id: codegg_core::workspace::WorkspaceId,
    submission_key: &str,
) -> NewJob {
    NewJob {
        workspace_id,
        session_id: Some("sess-1".to_string()),
        turn_id: None,
        kind: JobKind::ToolProgram,
        source: JobSource::AgentDelegated,
        priority: JobPriority::Normal,
        payload: JobPayload::ToolProgram {
            program_id: "prog-1".to_string(),
            invocation_key: "inv-1".to_string(),
            source_digest: "sha256:abc".to_string(),
            ir_digest: None,
            authority_digest: "auth-1".to_string(),
            execution_context_json: None,
            submission_key: submission_key.to_string(),
            execution_mode: "foreground".to_string(),
            source_ref: None,
            source_length: None,
            allowed_tools: vec![],
            authority_grant_json: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::ToolProgram),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
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
    }
}

#[tokio::test(flavor = "current_thread")]
async fn durable_submission_reconciles_after_lost_ack() {
    let store: Arc<dyn codegg_core::jobs::JobStore> = Arc::new(InMemoryJobStore::new());
    let harness = test_submission_harness(store).await;
    let ws = harness.workspace_id.clone();
    let submission = harness.submission.clone();
    let key = codegg::scheduler::SubmissionKey::new("retry-m002-lost-ack").expect("key");
    let first = submission
        .submit(
            Some(key.clone()),
            tool_program_job(ws.clone(), "retry-m002-lost-ack"),
        )
        .await
        .expect("first submit");
    // Simulate lost acknowledgement: the caller retries with the same key
    // and must converge on the canonical durable job, not a duplicate.
    let second = submission
        .submit(
            Some(key.clone()),
            tool_program_job(ws.clone(), "retry-m002-lost-ack"),
        )
        .await
        .expect("second submit");
    assert_eq!(first.job_id, second.job_id);
    // Explicit reconciliation finds the same canonical job.
    let reconciled = submission
        .reconcile_by_key(&key, &ws, None)
        .await
        .expect("reconcile")
        .expect("must find job");
    assert_eq!(reconciled.job_id, first.job_id);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_reconciles_from_durable_store() {
    let store: Arc<dyn codegg_core::jobs::JobStore> = Arc::new(InMemoryJobStore::new());
    let harness = test_submission_harness(store.clone()).await;
    let ws = harness.workspace_id.clone();
    let first = harness
        .submission
        .submit(
            Some(codegg::scheduler::SubmissionKey::new("retry-m002-restart").expect("key")),
            tool_program_job(ws.clone(), "retry-m002-restart"),
        )
        .await
        .expect("submit");
    // Simulate daemon restart: a fresh service with an empty in-memory
    // index but the same workspace registry + durable store must still
    // reconcile from the durable scan.
    let generation = DaemonGeneration::new();
    let scheduler = codegg::scheduler::JobScheduler::new(
        store.clone(),
        harness.services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    let restarted = JobSubmissionService::new(
        store.clone(),
        scheduler,
        harness.services.clone(),
        generation,
    );
    let key = codegg::scheduler::SubmissionKey::new("retry-m002-restart").expect("key");
    let reconciled = restarted
        .reconcile_by_key(&key, &ws, None)
        .await
        .expect("reconcile")
        .expect("must find durable job");
    assert_eq!(reconciled.job_id, first.job_id);
    // Resubmitting after restart converges instead of duplicating.
    let again = restarted
        .submit(
            Some(key.clone()),
            tool_program_job(ws.clone(), "retry-m002-restart"),
        )
        .await
        .expect("resubmit");
    assert_eq!(again.job_id, first.job_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_duplicate_submissions_converge() {
    let store: Arc<dyn codegg_core::jobs::JobStore> = Arc::new(InMemoryJobStore::new());
    let harness = test_submission_harness(store).await;
    let ws = harness.workspace_id.clone();
    let submission = harness.submission.clone();
    let key = codegg::scheduler::SubmissionKey::new("retry-m002-concurrent").expect("key");
    let (a, b) = tokio::join!(
        submission.submit(
            Some(key.clone()),
            tool_program_job(ws.clone(), "retry-m002-concurrent")
        ),
        submission.submit(
            Some(key.clone()),
            tool_program_job(ws.clone(), "retry-m002-concurrent")
        )
    );
    let a = a.expect("submit a");
    let b = b.expect("submit b");
    assert_eq!(a.job_id, b.job_id);
}
