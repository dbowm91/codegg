//! W001 — workspace-root execution authority for delegated subagents.
//!
//! The daemon is a user-scoped singleton: it inherits the directory it was
//! launched from and keeps that directory for its whole lifetime (ADR-0033).
//! A job rooted anywhere else must never inherit it.
//!
//! [`SubagentJobExecutor`] is the only production constructor of
//! `SubAgentRequest.workspace_root`, so these tests drive the real executor
//! rather than the registry helper it calls. A job whose payload carries no
//! `allowed_paths` is a live production state, not a fixture artefact: the
//! daemon's chat-collaboration action and the schedule materializer both emit
//! `allowed_paths: Vec::new()`.
//!
//! The child is scripted to make exactly one tool call, a `read` of a
//! *relative* path, and the assertion is on the tool result the child
//! actually received. That matters: a relative read is the one request whose
//! resolution is unobservable unless a root was supplied, so the test cannot
//! pass unless the read resolved against the job's workspace root — and it
//! cannot pass for the wrong reason, because the child's own final text is
//! never consulted.

use super::common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codegg::agent::{Agent, AgentMode};
use codegg::config::schema::Config;
use codegg::provider::{
    ChatEvent, ChatRequest, EventStream, Message, ModelInfo, Provider, ProviderError,
    ProviderRegistry, TokenUsage, ToolCall,
};
use codegg::scheduler::executor::{JobExecutionContext, JobExecutor, NoopProgressSink};
use codegg::scheduler::executors::SubagentJobExecutor;
use codegg::scheduler::permit::ResourcePermitGuard;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
};
use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId, RepositoryId};
use codegg_core::jobs::{
    AttemptId, DaemonGeneration, ExecutionTarget, IdempotencyClass, JobId, JobKind, JobPayload,
    JobPriority, JobRecord, JobSource, JobState, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Every tool `SubAgentPool::requires_isolation` treats as mutating. A child
/// that denies all of them classifies as read-only, which keeps this test on
/// the shared-workspace path rather than the managed-worktree path — the
/// worktree path carries its own root and would not exercise the resolution
/// under test.
const MUTATING_TOOLS: [&str; 9] = [
    "write",
    "edit",
    "apply_patch",
    "replace",
    "terminal",
    "bash",
    "git",
    "commit",
    "lsp_preview_apply",
];

const CHILD_AGENT: &str = "w001-read-only-child";
const PROVIDER_ID: &str = "w001probe";

/// Records the tool result the child was handed, so assertions read what
/// `read` actually returned rather than what the child claimed about it.
#[derive(Clone)]
struct RelativeReadProvider {
    marker: String,
    turn: Arc<Mutex<usize>>,
    observed_tool_result: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl Provider for RelativeReadProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn name(&self) -> &str {
        "W001 relative-read probe"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        for message in &request.messages {
            if let Message::Tool { content, .. } = message {
                *self.observed_tool_result.lock().await = Some(content.to_string());
            }
        }

        let mut turn = self.turn.lock().await;
        let events = if *turn == 0 {
            *turn += 1;
            vec![
                ChatEvent::ToolCall(ToolCall {
                    id: "w001-read".to_string().into(),
                    name: "read".to_string().into(),
                    arguments: serde_json::json!({ "path": self.marker }),
                }),
                ChatEvent::Finish {
                    stop_reason: "tool_calls".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]
        } else {
            vec![
                ChatEvent::TextDelta("done".to_string().into()),
                ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok::<_, ProviderError>),
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: format!("{PROVIDER_ID}/model"),
            name: "W001 model".to_string(),
            provider: PROVIDER_ID.to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: Vec::new(),
        }])
    }
}

fn read_only_child() -> Agent {
    let mut agent = Agent {
        git_read_only: false,
        name: CHILD_AGENT.to_string(),
        role: None,
        description: "W001 workspace-root probe".to_string(),
        mode: AgentMode::Primary,
        mode_name: None,
        model: Some(format!("{PROVIDER_ID}/model")),
        variant: None,
        temperature: None,
        top_p: None,
        color: None,
        steps: None,
        system_prompt: None,
        permissions: std::collections::HashMap::new(),
        hidden: false,
        thinking_budget: None,
        reasoning_effort: None,
        fallback_model: None,
        runtime_kind: None,
    };
    for tool in MUTATING_TOOLS {
        agent
            .permissions
            .insert(tool.to_string(), "deny".to_string());
    }
    agent
        .permissions
        .insert("read".to_string(), "allow".to_string());
    agent
}

/// A `JobPayload::Subagent` with `allowed_paths` empty — the shape the
/// daemon's chat-collaboration action and the schedule materializer emit.
fn subagent_job_without_allowed_paths() -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked("w001-subagent"),
        workspace_id: WorkspaceId::new_unchecked("w001-workspace"),
        session_id: None,
        turn_id: None,
        kind: JobKind::Subagent,
        source: JobSource::AgentDelegated,
        priority: JobPriority::Interactive,
        payload: JobPayload::Subagent {
            prompt: "Read the marker file.".to_string(),
            agent: CHILD_AGENT.to_string(),
            model: Some(format!("{PROVIDER_ID}/model")),
            parent_id: None,
            denied_tools: Vec::new(),
            allowed_paths: Vec::new(),
            max_tool_calls: None,
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::NonIdempotent,
        state: JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        not_before: None,
        deadline: None,
        schedule_id: None,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        depends_on: vec![],
        labels: std::collections::HashMap::new(),
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: ExecutionTarget::default(),
    }
}

async fn build_executor(
    marker: &str,
) -> (
    SubagentJobExecutor,
    Arc<Mutex<Option<String>>>,
    Arc<codegg::agent::worker::SubAgentPool>,
) {
    let pool = common::pool::isolated_pool().await;
    let provider = RelativeReadProvider {
        marker: marker.to_string(),
        turn: Arc::new(Mutex::new(0)),
        observed_tool_result: Arc::new(Mutex::new(None)),
    };
    let observed_tool_result = Arc::clone(&provider.observed_tool_result);

    let mut registry = ProviderRegistry::new();
    registry.register(provider);

    let config = Config::default();
    let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));
    let subagent_pool = Arc::new(
        codegg::agent::worker::SubAgentPool::new(
            &config,
            vec![read_only_child()],
            registry,
            session_store,
            Some(pool),
        )
        .await,
    );

    (
        SubagentJobExecutor::new(subagent_pool.clone()),
        observed_tool_result,
        subagent_pool,
    )
}

/// W001 W2 — a delegated subagent job whose payload carries no
/// `allowed_paths` must still resolve a workspace root, and it must be the
/// job's own daemon-owned root.
///
/// Before the fix the executor derived the child's root solely from
/// `allowed_paths.first()`, so this job produced `workspace_root: None`, the
/// registry fell through to `Tool::default()`, and every workspace tool took
/// its `allowed_root` from the daemon's process cwd. The relative `read`
/// then resolved in the daemon's launch directory instead of the job's
/// workspace — the reported symptom.
#[tokio::test(flavor = "multi_thread")]
async fn a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root() {
    let workspace = tempfile::tempdir().expect("job workspace");
    let marker_name = "w001-workspace-root-marker.txt";
    let marker_body = "resolved-against-the-job-workspace-root";
    std::fs::write(workspace.path().join(marker_name), marker_body).expect("seed marker");

    // Premise: the marker is reachable *only* from the job's workspace. If it
    // were visible to the process cwd this test could pass for the wrong
    // reason, which is the failure mode the whole milestone exists to catch.
    assert!(
        !std::path::Path::new(marker_name).exists(),
        "marker {marker_name} must not be visible from the process cwd \
         ({}); otherwise this test cannot distinguish the two roots",
        std::env::current_dir().expect("process cwd").display()
    );

    let (exec, observed_tool_result, _) = build_executor(marker_name).await;

    let ctx = JobExecutionContext {
        job: subagent_job_without_allowed_paths(),
        attempt_id: AttemptId::new_unchecked("w001-attempt"),
        daemon_generation: DaemonGeneration::new_unchecked("w001-generation"),
        workspace_id: WorkspaceId::new_unchecked("w001-workspace"),
        workspace_root: workspace.path().to_path_buf(),
        run_store: None,
        subject_store: None,
        source_subject_started: None,
        cancellation: CancellationToken::new(),
        progress: Arc::new(NoopProgressSink),
        resources: ResourcePermitGuard::new_orphan(Default::default()),
        process_admission: None,
    };

    // Bounded: an unanswered permission request parks a child forever, so an
    // unbounded wait would hang the suite instead of failing it.
    let completion = tokio::time::timeout(Duration::from_secs(120), exec.execute(ctx))
        .await
        .expect("subagent executor must not hang");

    assert_eq!(
        completion.status,
        codegg::scheduler::executor::ExecutorStatus::Completed,
        "child run failed before the read was attempted: {}",
        completion.summary
    );

    let observed = observed_tool_result
        .lock()
        .await
        .clone()
        .expect("child must have received a `read` tool result");
    assert!(
        observed.contains(marker_body),
        "the child's relative read did not resolve against the job's workspace root \
         {}.\nThe job's root is {} and its payload carries no `allowed_paths`, so the \
         executor had to fall back to something else — the daemon's process cwd.\n\
         Tool result was: {observed}",
        workspace.path().display(),
        workspace.path().display(),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_read_only_isolated_new_fails_closed_without_contained_creator() {
    let workspace = tempfile::tempdir().expect("job workspace");
    common::blackboard::init_repository(workspace.path());
    let managed_root = tempfile::tempdir().expect("managed worktree root");
    let run_store = Arc::new(InMemoryAgentRunStore::new());
    let task_id = AgentTaskId::new();
    let run_id = AgentRunId::new();
    let workspace_id = WorkspaceId::new_unchecked("a001-explicit-workspace");
    let repository_id = RepositoryId::new();
    run_store
        .create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: "a001-session".into(),
                originating_turn_id: Some("a001-turn".into()),
                project_id: ProjectId::new(),
                repository_id: Some(repository_id),
                workspace_id: workspace_id.clone(),
                requested_agent: CHILD_AGENT.into(),
                delegation_key: "a001-explicit-placement".into(),
                request_fingerprint: "a001-explicit-placement-fingerprint".into(),
                description: "read-only explicit placement regression".into(),
            },
            NewAgentRun {
                run_id: run_id.clone(),
                parent_run_id: None,
                depth: 1,
                workspace_id: workspace_id.clone(),
                agent_name: CHILD_AGENT.into(),
                agent_digest: None,
                provider: PROVIDER_ID.into(),
                model: format!("{PROVIDER_ID}/model"),
                authority_digest: "a001-authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create durable run");

    let (_, _, pool) = build_executor("unused").await;
    let worktrees = codegg_core::worktree_service::WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let executor = SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
        pool,
        run_store.clone(),
        None,
        worktrees,
    );
    let mut job = subagent_job_without_allowed_paths();
    job.job_id = JobId::new_unchecked("a001-explicit-isolated-new");
    job.workspace_id = workspace_id.clone();
    job.payload = JobPayload::SubagentRun {
        prompt: "Read without changing files.".into(),
        agent: CHILD_AGENT.into(),
        model: Some(format!("{PROVIDER_ID}/model")),
        parent_id: None,
        denied_tools: MUTATING_TOOLS.iter().map(|name| (*name).into()).collect(),
        allowed_paths: Vec::new(),
        max_tool_calls: None,
        task_id,
        run_id: run_id.clone(),
        delegation_key: "a001-explicit-placement".into(),
        base_commit: None,
        placement: codegg_core::run_placement::RunPlacement::IsolatedNew,
        default_working_directory: None,
    };
    let context = JobExecutionContext {
        job,
        attempt_id: AttemptId::new_unchecked("a001-explicit-attempt"),
        daemon_generation: DaemonGeneration::new_unchecked("a001-generation"),
        workspace_id,
        workspace_root: workspace.path().to_path_buf(),
        run_store: None,
        subject_store: None,
        source_subject_started: None,
        cancellation: CancellationToken::new(),
        progress: Arc::new(NoopProgressSink),
        resources: ResourcePermitGuard::new_orphan(Default::default()),
        process_admission: None,
    };

    run_store
        .transition(&run_id, codegg_core::agent_run::AgentRunStatus::Queued)
        .await
        .expect("scheduler queued the durable run before dispatch");
    let completion = executor.execute(context).await;
    assert_eq!(
        completion.status,
        codegg::scheduler::executor::ExecutorStatus::Failed,
        "unexpected executor outcome: {completion:#?}"
    );
    let run = run_store.get_run(&run_id).await.unwrap().unwrap();
    assert!(
        run.status.is_terminal(),
        "run did not finish after executor failure: {run:#?}"
    );
    assert_eq!(
        run.failure_class.as_deref(),
        Some("explicit_isolation_unavailable")
    );
    let reason = run.failure_message.as_deref().unwrap_or_default();
    assert!(
        reason.contains("Git backend unavailable"),
        "the in-memory WorktreeService has no configured ContainedWorktreeCreator, so explicit isolation must report that typed failure: {reason}"
    );
}

/// A001 strict-success regression: explicit read-only placement must produce
/// a real, distinct checkout through the production contained creator before
/// the read-only child runs. The test is intentionally host-qualified: if the
/// platform cannot obtain filesystem containment, a successful in-place child
/// would be a security regression and the executor must fail closed.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn explicit_read_only_isolated_new_uses_two_distinct_contained_checkouts() {
    use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobPriority, JobSource, JobStore,
        NewJob, ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use codegg_core::worktree_service::WorktreeService;

    let workspace = tempfile::tempdir().expect("job workspace");
    common::blackboard::init_repository(workspace.path());
    // Diverge the parent working file from HEAD. A read of "base" proves the
    // model-visible tool resolved in the managed checkout rather than in the
    // unchanged parent path.
    std::fs::write(workspace.path().join("README"), "parent-only\n")
        .expect("mark the parent checkout distinctly");
    let managed_root = tempfile::tempdir().expect("managed worktree root");
    let scratch_root = tempfile::tempdir().expect("contained Git scratch root");
    let creator = Arc::new(
        codegg::git_contained_worktree::ContainedGitWorktreeCreator::new(
            codegg::git_contained_worktree::resolve_git_binary().expect("pinned Git binary"),
            scratch_root.path().to_path_buf(),
        ),
    );
    let worktrees = WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    )
    .with_contained_creator(creator);
    let run_store = Arc::new(InMemoryAgentRunStore::new());
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace_record = workspace_registry
        .get_or_register(workspace.path())
        .await
        .expect("register parent workspace");
    let workspace_id = workspace_record.id.clone();
    let workspace_services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let mut checkout_paths = Vec::new();

    for ordinal in 0..2 {
        let (_, observed_tool_result, pool) = build_executor("README").await;
        let executor = SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
            pool,
            run_store.clone(),
            None,
            worktrees.clone(),
        );
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        run_store
            .create_or_get(
                NewAgentTask {
                    task_id: task_id.clone(),
                    parent_task_id: None,
                    originating_session_id: "a001-contained-session".into(),
                    originating_turn_id: Some("a001-contained-turn".into()),
                    project_id: ProjectId::new(),
                    repository_id: Some(RepositoryId::new()),
                    workspace_id: workspace_id.clone(),
                    requested_agent: CHILD_AGENT.into(),
                    delegation_key: format!("a001-contained-{ordinal}"),
                    request_fingerprint: format!("a001-contained-fingerprint-{ordinal}"),
                    description: "read-only explicit isolated placement success".into(),
                },
                NewAgentRun {
                    run_id: run_id.clone(),
                    parent_run_id: None,
                    depth: 1,
                    workspace_id: workspace_id.clone(),
                    agent_name: CHILD_AGENT.into(),
                    agent_digest: None,
                    provider: PROVIDER_ID.into(),
                    model: format!("{PROVIDER_ID}/model"),
                    authority_digest: "a001-contained-authority".into(),
                    budget: AgentRunBudget::default(),
                    shared_scratch: Vec::new(),
                },
            )
            .await
            .expect("create durable run");

        let payload = JobPayload::SubagentRun {
            prompt: "Read the tracked README without changing files.".into(),
            agent: CHILD_AGENT.into(),
            model: Some(format!("{PROVIDER_ID}/model")),
            parent_id: None,
            denied_tools: MUTATING_TOOLS.iter().map(|name| (*name).into()).collect(),
            allowed_paths: Vec::new(),
            max_tool_calls: None,
            task_id,
            run_id: run_id.clone(),
            delegation_key: format!("a001-contained-{ordinal}"),
            base_commit: None,
            placement: codegg_core::run_placement::RunPlacement::IsolatedNew,
            default_working_directory: None,
        };
        let job_store = Arc::new(InMemoryJobStore::new());
        let generation = DaemonGeneration::new_unchecked(format!("a001-contained-{ordinal}"));
        let scheduler = JobScheduler::new(
            job_store.clone(),
            workspace_services.clone(),
            ResolvedSchedulerConfig::default(),
            generation.clone(),
        );
        scheduler
            .register_executor(Arc::new(executor))
            .await
            .expect("register production subagent executor");
        let submission = JobSubmissionService::new(
            job_store.clone(),
            scheduler.clone(),
            workspace_services.clone(),
            generation,
        );
        run_store
            .transition(&run_id, codegg_core::agent_run::AgentRunStatus::Queued)
            .await
            .expect("mark durable run queued before scheduler dispatch");
        let scheduler_task = scheduler.clone().spawn_run();
        let submitted = submission
            .submit(
                None,
                NewJob {
                    workspace_id: workspace_id.clone(),
                    session_id: None,
                    turn_id: None,
                    kind: JobKind::Subagent,
                    source: JobSource::AgentDelegated,
                    priority: JobPriority::Interactive,
                    payload,
                    resource_request: ResourceRequest::default(),
                    timeout: None,
                    retry_policy: RetryPolicy::no_retry(),
                    idempotency: IdempotencyClass::NonIdempotent,
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
                    target: ExecutionTarget::default(),
                },
            )
            .await
            .expect("submit isolated child through live scheduler");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let completion = loop {
            let record = job_store
                .get_job(&submitted.job_id)
                .await
                .expect("read scheduled job")
                .expect("submitted job exists");
            if record.state.is_terminal() {
                break record;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "live scheduler did not finish explicit isolated child: {record:#?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(
            completion.state,
            JobState::Completed,
            "contained creator or read-only child failed: {completion:#?}"
        );
        scheduler
            .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
            .await;
        scheduler_task.await.expect("join test scheduler");
        let read_result = observed_tool_result
            .lock()
            .await
            .clone()
            .unwrap_or_default();
        assert!(
            read_result.contains("base") && !read_result.contains("parent-only"),
            "child must read the tracked marker from its checkout: {read_result}"
        );
        let run = run_store.get_run(&run_id).await.unwrap().unwrap();
        let worktree_id = run
            .worktree_id
            .expect("successful isolated run has a worktree");
        let record = worktrees
            .get(&worktree_id)
            .await
            .expect("managed worktree record");
        assert_ne!(record.path, workspace.path());
        assert!(record.path.starts_with(managed_root.path()));
        assert!(record.path.join("README").is_file());
        assert_eq!(
            record.state,
            codegg_core::worktree_service::ManagedWorktreeState::Ready
        );
        checkout_paths.push(record.path);
    }

    assert_ne!(checkout_paths[0], checkout_paths[1]);
    assert!(workspace.path().join("README").is_file());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("README")).unwrap(),
        "parent-only\n",
        "the parent checkout remains unchanged"
    );
}
