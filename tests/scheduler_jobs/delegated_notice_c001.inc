//! C001: the orchestrator always hears about its child.
//!
//! A delegated run (a subagent spawned by an orchestrator turn) has no
//! durable record of its terminal state unless the delegated job is
//! subscribed for a durable terminal notice. These tests drive the real
//! `task` tool's durable spawn path and assert that:
//!
//! - the subscription carries the parent's owner (kind, session, turn,
//!   invocation key, disposition, principal, workspace);
//! - the child's terminal state produces a notice row that names the run;
//! - the notice survives a parent turn that ended, and a dropped in-band
//!   push, and a daemon restart between submit and subscribe;
//! - two children terminating concurrently produce one notice each;
//! - repeated injection passes do not duplicate the message;
//! - `task action=get` returns the recorded result, not only a pointer.

use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg::agent::job_notification_recovery::inject_recoverable_job_notifications;
use codegg::scheduler::job_notifications::{
    format_job_notice_content, JobCompletionDisposition, JobNotificationService, JobTerminalNotice,
};
use codegg::scheduler::{
    ExecutorCompletion, ExecutorKind, ExecutorMetrics, ExecutorStatus, JobExecutionContext,
    JobExecutor, JobScheduler, JobSubmissionService, ResolvedSchedulerConfig,
};
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::task::{TaskStore, TaskTool};
use codegg::tool::Tool;
use codegg_core::agent_run::{AgentRunStore, InMemoryAgentRunStore};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio::sync::Mutex;

use super::common;

const PARENT_SESSION: &str = "sess-c001-parent";

/// A delegated executor that completes immediately with a recorded outcome.
///
/// It mirrors the real delegated executor's run bookkeeping — the run walks
/// to `Running`, the outcome is saved in the run-result store, and the run
/// finishes with the `agent-run-result:<run_id>` reference — so the tests
/// exercise the same durable records the production path produces.
struct ImmediateDelegatedExecutor {
    runs: Arc<dyn AgentRunStore>,
    /// Whether the child left a recorded result behind.
    record_result: bool,
}

#[async_trait::async_trait]
impl JobExecutor for ImmediateDelegatedExecutor {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Subagent
    }

    fn supports(&self, kind: codegg_core::jobs::JobKind) -> bool {
        matches!(kind, codegg_core::jobs::JobKind::Subagent)
    }

    async fn execute(&self, ctx: JobExecutionContext) -> ExecutorCompletion {
        let summary = "the child finished its work".to_string();
        let run_id = match &ctx.job.payload {
            codegg_core::jobs::JobPayload::SubagentRun { run_id, .. } => Some(run_id.clone()),
            _ => None,
        };
        if let Some(run_id) = run_id {
            self.runs
                .transition(&run_id, codegg_core::agent_run::AgentRunStatus::Preparing)
                .await
                .expect("the delegated run enters preparing");
            self.runs
                .transition(&run_id, codegg_core::agent_run::AgentRunStatus::Running)
                .await
                .expect("the delegated run starts");
            if self.record_result {
                self.runs
                    .save_result(codegg_core::run_result::AgentRunResult {
                        run_id: run_id.clone(),
                        status: codegg_core::run_result::AgentRunResultStatus::Succeeded,
                        summary: summary.clone(),
                        worktree_id: None,
                        base_commit: None,
                        result_commit: None,
                        changed_paths: vec![],
                        validation: vec![],
                        findings: vec![],
                        artifacts: vec![],
                        repository_state: codegg_core::run_result::RepositoryState::Clean,
                        retryability: codegg_core::run_result::Retryability::NotRetryable,
                        recovery_hint: None,
                    })
                    .await
                    .expect("the child records its outcome");
            }
            self.runs
                .finish(
                    &run_id,
                    codegg_core::agent_run::AgentRunTerminalOutcome::Completed,
                    Some(format!("agent-run-result:{run_id}")),
                    None,
                    None,
                )
                .await
                .expect("the delegated run finishes");
        }
        ExecutorCompletion {
            status: ExecutorStatus::Completed,
            summary,
            run_id: None,
            metrics: ExecutorMetrics::default(),
        }
    }
}

struct Harness {
    scheduler: Arc<JobScheduler>,
    notifications: Arc<JobNotificationService>,
    store: codegg_core::session::EventStore,
    tool: TaskTool,
    job_store: Arc<dyn JobStore>,
    runs: Arc<dyn AgentRunStore>,
    services: Arc<WorkspaceServiceRegistry>,
    generation: DaemonGeneration,
    workspace_path: std::path::PathBuf,
    _scheduler_handle: tokio::task::JoinHandle<()>,
    _workspace: tempfile::TempDir,
}

/// The parent's execution context: the identity the subscription must carry.
fn tool_context(call_id: &str) -> ToolExecutionContext {
    let mut context = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    context.session_id = Some(PARENT_SESSION.to_string());
    context.turn_id = Some("turn-c001".to_string());
    context.origin_principal = Some("principal-c001".to_string());
    context.invocation_key = Some(call_id.to_string());
    context
}

async fn harness() -> Harness {
    harness_with_result(true).await
}

/// A harness whose `task` tool has no durable notification service.
///
/// The delegated child still runs and still terminates; it simply has no
/// subscription to attribute a terminal notice to. That is the state a
/// daemon restart leaves behind: the durable job and its subscription are
/// on disk, but the notice was never written.
async fn harness_without_tool_notifications() -> Harness {
    harness_with_options(true, false).await
}

/// A harness whose child leaves a recorded result behind.
async fn harness_with_result(record_result: bool) -> Harness {
    harness_with_options(record_result, true).await
}

async fn harness_with_options(record_result: bool, subscribe_tool: bool) -> Harness {
    let pool = common::pool::isolated_pool().await;
    let notifications = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let store = codegg_core::session::EventStore::new(pool);

    let workspace = tempfile::tempdir().expect("workspace");
    let registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    registry
        .get_or_register(workspace.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );

    let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let generation = DaemonGeneration::new_unchecked("delegated-notice-c001");
    let scheduler = JobScheduler::new(
        job_store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    scheduler
        .register_executor(Arc::new(ImmediateDelegatedExecutor {
            runs: runs.clone(),
            record_result,
        }))
        .await
        .expect("register the delegated executor");
    scheduler.set_job_notification_service_sync(notifications.clone());
    let scheduler_handle = scheduler.spawn_run();
    let submission = JobSubmissionService::new(
        job_store.clone(),
        scheduler.clone(),
        services.clone(),
        generation.clone(),
    );
    let tool = TaskTool::new(
        Arc::new(Mutex::new(TaskStore::new())),
        None,
        Some(PARENT_SESSION.to_string()),
        vec![],
    )
    .with_submission(submission, workspace.path().to_path_buf())
    .with_agent_run_store(runs.clone())
    .with_project_context(None, None, Some("turn-c001".to_string()));
    let tool = if subscribe_tool {
        tool.with_job_notifications(Some(notifications.clone()))
    } else {
        tool
    };

    Harness {
        scheduler,
        notifications,
        store,
        tool,
        job_store,
        runs,
        services,
        generation,
        workspace_path: workspace.path().to_path_buf(),
        _scheduler_handle: scheduler_handle,
        _workspace: workspace,
    }
}

fn spawn_input(key: &str, description: &str) -> serde_json::Value {
    serde_json::json!({
        "action": "spawn",
        "description": description,
        "prompt": "do the delegated work",
        "agent": "general",
        "idempotency_key": key,
    })
}

/// Spawn a delegated child through the real tool and return its output.
async fn spawn(tool: &TaskTool, key: &str, description: &str) -> String {
    tool.execute_structured(spawn_input(key, description), Some(tool_context(key)))
        .await
        .expect("the delegated spawn succeeds")
        .output
}

fn field(output: &str, label: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{label}: ")))
        .unwrap_or_else(|| panic!("{label} in output:\n{output}"))
        .trim()
        .to_string()
}

/// `session_events.session_id` references `session(id)`, which references
/// `project(id)`. Hermetic pools need both rows before the recovery loop can
/// append the durable parent-session event.
async fn ensure_session(pool: &sqlx::SqlitePool, session_id: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES ('proj-c001', '/tmp', 1, 1, '[]')",
    )
    .execute(pool)
    .await
    .expect("project row");
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, 'proj-c001', 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .execute(pool)
    .await
    .expect("session row");
}

async fn wait_for_completion(harness: &Harness, job_id: &str) {
    harness
        .scheduler
        .wait_for_completion(
            &codegg_core::jobs::JobId::new_unchecked(job_id.to_string()),
            Duration::from_secs(10),
        )
        .await
        .expect("the delegated run terminates");
}

/// Wait until the parent session holds at least `expected` pending notices.
///
/// `wait_for_completion` waits for the *execution* to finish, and the
/// scheduler publishes that completion before it records the job's durable
/// terminal notice. A read taken the instant the completion returns
/// therefore measures that window rather than a lost notice.
///
/// The guarantee under test is that a notice is never lost: the delegated
/// spawn subscribes its job, and a child that terminated before the
/// subscription landed has its terminal held until the subscription does.
/// So the test waits for the durable notice with a bounded poll and then
/// asserts on what it finds.
async fn pending_notices(harness: &Harness, expected: usize) -> Vec<JobTerminalNotice> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pending = harness
            .notifications
            .pending_for_session(PARENT_SESSION)
            .await
            .expect("pending notices");
        if pending.len() >= expected {
            return pending;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected} pending notice(s) for {PARENT_SESSION}, saw {}",
            pending.len()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The delegated spawn must subscribe the job for its own durable notice,
/// under the parent's owner.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_spawn_subscribes_the_job_for_its_terminal_notice() {
    let harness = harness().await;
    let output = spawn(&harness.tool, "spawn-c001-subscribe", "subscribe the child").await;
    let job_id = field(&output, "Scheduler job");
    let run_id = field(&output, "Run");

    let subscription = harness
        .notifications
        .get_subscription(&job_id)
        .await
        .expect("subscription lookup")
        .expect("the delegated job is subscribed");
    assert_eq!(subscription.kind, "subagent");
    assert_eq!(subscription.session_id, PARENT_SESSION);
    assert_eq!(subscription.turn_id.as_deref(), Some("turn-c001"));
    assert_eq!(
        subscription.disposition,
        JobCompletionDisposition::NotifyOnly,
        "a delegated run asks for a notice, never for a continuation"
    );
    assert_eq!(
        subscription.owner_principal.as_deref(),
        Some("principal-c001")
    );
    assert!(
        !subscription.invocation_key.is_empty(),
        "the subscription must carry the invocation key the submit used"
    );
    assert!(
        !run_id.is_empty(),
        "the spawn must report the delegated run, got {run_id}"
    );
}

/// The central case: the child terminates, the notice row exists, and the
/// next turn boundary injects it as a system message that names the run.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_run_produces_a_notice_that_names_it() {
    let harness = harness().await;
    let output = spawn(&harness.tool, "spawn-c001-central", "the central case").await;
    let job_id = field(&output, "Scheduler job");
    let run_id = field(&output, "Run");

    wait_for_completion(&harness, &job_id).await;

    let pending = pending_notices(&harness, 1).await;
    assert_eq!(pending.len(), 1, "one notice per delegated run");
    let notice = &pending[0];
    assert_eq!(notice.kind, "subagent");
    assert_eq!(
        notice.run_id.as_deref(),
        Some(run_id.as_str()),
        "the notice must name the delegated run"
    );
    let content = format_job_notice_content(notice);
    assert!(
        content.contains(&run_id),
        "the notice content must identify the run, got: {content}"
    );
    assert!(
        content.contains("the child finished its work"),
        "the notice content must carry the recorded outcome, got: {content}"
    );

    // The next turn boundary injects it.
    ensure_session(&harness.store.pool(), PARENT_SESSION).await;
    let mut messages: Vec<String> = vec![];
    let report = inject_recoverable_job_notifications(
        Some(&harness.store),
        &harness.notifications,
        PARENT_SESSION,
        |text| messages.push(text),
    )
    .await;
    assert_eq!(report.injected, 1, "one injection at the next boundary");
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].contains(&run_id),
        "the injected message must identify the run, got: {}",
        messages[0]
    );
}

/// The parent's turn ended before the child terminated: the notice is still
/// pending, and the next turn still receives it.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_notice_survives_a_parent_turn_that_ended() {
    let harness = harness().await;
    let output = spawn(&harness.tool, "spawn-c001-ended", "the parent turn ends").await;
    let job_id = field(&output, "Scheduler job");
    let run_id = field(&output, "Run");

    // The parent turn ends here: nothing is listening, no in-band receiver
    // exists, and the only channel left is the durable notice.
    wait_for_completion(&harness, &job_id).await;

    let pending = pending_notices(&harness, 1).await;
    assert_eq!(
        pending.len(),
        1,
        "an ended parent turn must not lose the notice"
    );
    assert_eq!(pending[0].run_id.as_deref(), Some(run_id.as_str()));

    ensure_session(&harness.store.pool(), PARENT_SESSION).await;
    let mut messages: Vec<String> = vec![];
    inject_recoverable_job_notifications(
        Some(&harness.store),
        &harness.notifications,
        PARENT_SESSION,
        |text| messages.push(text),
    )
    .await;
    assert_eq!(messages.len(), 1, "the next turn still receives the notice");
    assert!(messages[0].contains(&run_id));
}

/// A dropped in-band push costs latency, never the notice.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_notice_is_delivered_when_the_in_band_push_is_dropped() {
    let harness = harness().await;
    let output = spawn(&harness.tool, "spawn-c001-dropped", "the push is dropped").await;
    let job_id = field(&output, "Scheduler job");

    // No live parent turn is registered, so the in-band push has nowhere to
    // land and is dropped. The durable notice is unaffected.
    wait_for_completion(&harness, &job_id).await;

    let pending = pending_notices(&harness, 1).await;
    assert_eq!(
        pending.len(),
        1,
        "a dropped in-band push must not lose the durable notice"
    );
}

/// A daemon restart between submit and subscribe: the retry with the same
/// invocation key completes the subscription, and exactly one notice exists
/// afterwards.
#[tokio::test(flavor = "current_thread")]
async fn a_retried_spawn_completes_the_subscription_after_a_restart() {
    let harness = harness().await;
    let first = spawn(&harness.tool, "spawn-c001-restart", "the daemon restarts").await;
    let job_id = field(&first, "Scheduler job");
    let run_id = field(&first, "Run");

    // The daemon restarts: a fresh submission service and a fresh notifier
    // view over the same durable store, and the same tool retried with the
    // same invocation key.
    let pool = harness.store.pool();
    let restarted_notifications = Arc::new(JobNotificationService::with_pool(pool.clone()));
    let restarted_submission = JobSubmissionService::new(
        harness.job_store.clone(),
        harness.scheduler.clone(),
        harness.services.clone(),
        harness.generation.clone(),
    );
    let restarted = TaskTool::new(
        Arc::new(Mutex::new(TaskStore::new())),
        None,
        Some(PARENT_SESSION.to_string()),
        vec![],
    )
    .with_submission(restarted_submission, harness.workspace_path.clone())
    .with_agent_run_store(harness.runs.clone())
    .with_project_context(None, None, Some("turn-c001".to_string()))
    .with_job_notifications(Some(restarted_notifications.clone()));
    let second = spawn(&restarted, "spawn-c001-restart", "the daemon restarts").await;
    assert_eq!(
        field(&second, "Task"),
        field(&first, "Task"),
        "the retry must reuse the same delegated task"
    );
    assert_eq!(field(&second, "Run"), run_id);

    let subscription = restarted_notifications
        .get_subscription(&job_id)
        .await
        .expect("subscription lookup")
        .expect("the retried spawn completes the subscription");
    assert_eq!(subscription.session_id, PARENT_SESSION);

    wait_for_completion(&harness, &job_id).await;

    let pending = pending_notices(&harness, 1).await;
    assert_eq!(pending.len(), 1, "exactly one notice after the retry");
}

/// Two children terminating concurrently: one notice each, none lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_delegated_runs_produce_one_notice_each() {
    let harness = harness().await;
    let mut job_ids = vec![];
    for index in 0..2 {
        let output = spawn(
            &harness.tool,
            &format!("spawn-c001-concurrent-{index}"),
            "a concurrent child",
        )
        .await;
        job_ids.push(field(&output, "Scheduler job"));
    }

    for job_id in &job_ids {
        wait_for_completion(&harness, job_id).await;
    }

    let pending = pending_notices(&harness, 2).await;
    assert_eq!(pending.len(), 2, "one notice per delegated run");
    let mut noticed: Vec<String> = pending.iter().map(|notice| notice.job_id.clone()).collect();
    noticed.sort();
    noticed.dedup();
    assert_eq!(noticed.len(), 2, "no notice is lost or duplicated");
}

/// A daemon that restarted between a child's termination and its
/// subscription heals the gap from the durable rows: the job is terminal,
/// the subscription exists, and the notice is backfilled — naming the run.
#[tokio::test(flavor = "current_thread")]
async fn the_daemon_backfill_names_the_run_of_a_delegated_job_that_terminated_first() {
    let harness = harness_without_tool_notifications().await;
    let output = spawn(
        &harness.tool,
        "spawn-c001-backfill",
        "a child that terminates before its subscription",
    )
    .await;
    let job_id = field(&output, "Scheduler job");
    let run_id = field(&output, "Run");

    wait_for_completion(&harness, &job_id).await;

    // A fresh service over the same durable store stands in for the daemon
    // after a restart: the in-memory hold is gone with the old process.
    let restarted = Arc::new(JobNotificationService::with_pool(harness.store.pool()));
    restarted
        .subscribe(
            &codegg_core::jobs::JobId::new_unchecked(job_id.clone()),
            codegg::scheduler::job_notifications::DELEGATED_JOB_KIND,
            PARENT_SESSION,
            Some("turn-c001"),
            "spawn-c001-backfill",
        )
        .await
        .expect("the retried spawn completes the subscription");

    // Nothing was written at the terminal transition: the subscription
    // landed after it. The durable backfill is what heals the gap.
    assert!(
        restarted
            .pending_for_session(PARENT_SESSION)
            .await
            .expect("pending notices")
            .is_empty(),
        "the backfill is what creates the notice"
    );
    let recovered = restarted
        .recover_from_store(&harness.job_store)
        .await
        .expect("the durable backfill runs");
    assert_eq!(recovered, 1, "one backfilled notice for the terminal job");

    let pending = restarted
        .pending_for_session(PARENT_SESSION)
        .await
        .expect("pending notices");
    assert_eq!(pending.len(), 1, "one notice after the backfill");
    assert_eq!(
        pending[0].run_id.as_deref(),
        Some(run_id.as_str()),
        "a backfilled delegated notice must name the run it is about"
    );
    let content = format_job_notice_content(&pending[0]);
    assert!(
        content.contains(&run_id),
        "the backfilled notice content must identify the run, got: {content}"
    );
}

/// Repeated injection passes do not duplicate the message.
#[tokio::test(flavor = "current_thread")]
async fn repeated_injection_passes_do_not_duplicate_the_notice() {
    let harness = harness().await;
    let output = spawn(&harness.tool, "spawn-c001-once", "one notice only").await;
    let job_id = field(&output, "Scheduler job");
    wait_for_completion(&harness, &job_id).await;
    // The injection pass consumes pending notices, so the notice has to be
    // durable before the passes run.
    pending_notices(&harness, 1).await;

    ensure_session(&harness.store.pool(), PARENT_SESSION).await;
    let mut messages: Vec<String> = vec![];
    for _ in 0..3 {
        inject_recoverable_job_notifications(
            Some(&harness.store),
            &harness.notifications,
            PARENT_SESSION,
            |text| messages.push(text),
        )
        .await;
    }
    assert_eq!(
        messages.len(),
        1,
        "repeated injection passes must not duplicate the message"
    );
    let events = harness
        .store
        .list_for_session(PARENT_SESSION)
        .await
        .expect("session events");
    assert_eq!(events.len(), 1, "one durable event per logical terminal");
}

/// `task action=get` returns the recorded result, not only a pointer.
#[tokio::test(flavor = "current_thread")]
async fn task_get_returns_the_recorded_result_and_the_result_ref() {
    let harness = harness_with_result(false).await;
    let output = spawn(&harness.tool, "spawn-c001-get", "read the recorded result").await;
    let task_id = field(&output, "Task");
    let run_id = field(&output, "Run");
    let job_id = field(&output, "Scheduler job");
    wait_for_completion(&harness, &job_id).await;

    // Record the child's outcome in the durable run-result store.
    let run = harness
        .runs
        .get_run_for_task(&codegg_core::identity::AgentTaskId::parse(&task_id).expect("task id"))
        .await
        .expect("run lookup")
        .expect("the delegated run");
    harness
        .runs
        .save_result(codegg_core::run_result::AgentRunResult {
            run_id: run.run_id.clone(),
            status: codegg_core::run_result::AgentRunResultStatus::Succeeded,
            summary: "the child's recorded answer".to_string(),
            worktree_id: None,
            base_commit: None,
            result_commit: None,
            changed_paths: vec![],
            validation: vec![],
            findings: vec![],
            artifacts: vec![],
            repository_state: codegg_core::run_result::RepositoryState::Clean,
            retryability: codegg_core::run_result::Retryability::NotRetryable,
            recovery_hint: None,
        })
        .await
        .expect("save the recorded result");

    let got = harness
        .tool
        .execute(serde_json::json!({ "action": "get", "task_id": task_id }))
        .await
        .expect("the get action succeeds");
    let parsed: serde_json::Value =
        serde_json::from_str(&got).unwrap_or_else(|_| panic!("get returns JSON, got: {got}"));
    assert_eq!(
        parsed["result_ref"].as_str(),
        Some(format!("agent-run-result:{run_id}").as_str()),
        "the result_ref line must survive, got: {got}"
    );
    assert_eq!(
        parsed["recorded_result"].as_str(),
        Some("the child's recorded answer"),
        "the recorded result must be returned, got: {got}"
    );
    assert_eq!(parsed["recorded_result_state"].as_str(), Some("recorded"));
    let limit = parsed["recorded_result_limit_bytes"]
        .as_u64()
        .expect("the bound");
    assert!(
        limit > 0,
        "the recorded result's bound must be observable, got: {got}"
    );
    assert!(
        parsed["recorded_result_bytes"].as_u64().expect("the size") <= limit,
        "the returned result must stay inside the store's bound, got: {got}"
    );
}

/// A run with no recorded result says so plainly.
#[tokio::test(flavor = "current_thread")]
async fn task_get_on_a_run_with_no_recorded_result_states_it_plainly() {
    let harness = harness_with_result(false).await;
    let output = spawn(&harness.tool, "spawn-c001-empty", "no recorded result").await;
    let task_id = field(&output, "Task");
    let job_id = field(&output, "Scheduler job");
    wait_for_completion(&harness, &job_id).await;

    let got = harness
        .tool
        .execute(serde_json::json!({ "action": "get", "task_id": task_id }))
        .await
        .expect("the get action succeeds");
    let parsed: serde_json::Value =
        serde_json::from_str(&got).unwrap_or_else(|_| panic!("get returns JSON, got: {got}"));
    assert_eq!(parsed["recorded_result_state"].as_str(), Some("absent"));
    assert!(
        parsed["recorded_result"].is_null(),
        "an absent result must not be an empty success, got: {got}"
    );
    assert_eq!(
        parsed["recorded_result_note"].as_str(),
        Some("this run recorded no result"),
        "the absence must be stated plainly, got: {got}"
    );
}

/// The delegated job keeps the retry and idempotency policy it always had.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_job_keeps_its_retry_and_idempotency_policy() {
    let harness = harness().await;
    let output = spawn(
        &harness.tool,
        "spawn-c001-policy",
        "the policy is unchanged",
    )
    .await;
    let job_id = field(&output, "Scheduler job");

    let job = harness
        .job_store
        .get_job(&codegg_core::jobs::JobId::new_unchecked(job_id))
        .await
        .expect("job lookup")
        .expect("the delegated job");
    assert_eq!(
        job.retry_policy,
        codegg_core::jobs::RetryPolicy::no_retry(),
        "the subscription must not change the delegated job's retry policy"
    );
    assert_eq!(
        job.idempotency,
        codegg_core::jobs::IdempotencyClass::NonIdempotent,
        "the subscription must not change the delegated job's idempotency class"
    );
}

/// The subscription changes nothing about how the child runs: same tool
/// policy, same path set, same sandbox, same worktree ceiling.
#[tokio::test(flavor = "current_thread")]
async fn the_delegated_run_policy_is_unchanged_by_the_subscription() {
    let harness = harness().await;
    let output = harness
        .tool
        .execute_structured(
            serde_json::json!({
                "action": "spawn",
                "description": "the policy is unchanged",
                "prompt": "do the delegated work",
                "agent": "general",
                "idempotency_key": "spawn-c001-policy-detail",
                "allowed_paths": ["src"],
            }),
            Some(tool_context("spawn-c001-policy-detail")),
        )
        .await
        .expect("the delegated spawn succeeds")
        .output;
    let job_id = field(&output, "Scheduler job");

    let job = harness
        .job_store
        .get_job(&codegg_core::jobs::JobId::new_unchecked(job_id))
        .await
        .expect("job lookup")
        .expect("the delegated job");
    match &job.payload {
        codegg_core::jobs::JobPayload::SubagentRun {
            allowed_paths,
            denied_tools,
            ..
        } => {
            assert_eq!(
                allowed_paths,
                &vec!["src".to_string()],
                "the delegated job's path set must be unchanged"
            );
            assert!(
                denied_tools.is_empty(),
                "the delegated job's tool policy must be unchanged"
            );
        }
        other => panic!("the delegated job keeps its delegated payload, got {other:?}"),
    }
    assert_eq!(
        job.resource_request,
        codegg_core::jobs::ResourceRequest::for_kind(codegg_core::jobs::JobKind::Subagent),
        "the delegated job's resource request (and its sandbox/worktree ceiling) must be unchanged"
    );
}
