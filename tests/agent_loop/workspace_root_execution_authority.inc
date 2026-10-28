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
    build_executor_with_pool(marker, pool).await
}

async fn build_executor_with_pool(
    marker: &str,
    pool: sqlx::SqlitePool,
) -> (
    SubagentJobExecutor,
    Arc<Mutex<Option<String>>>,
    Arc<codegg::agent::worker::SubAgentPool>,
) {
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
        worktrees.clone(),
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

#[tokio::test(flavor = "current_thread")]
async fn explicit_read_only_isolated_new_refuses_a_repositoryless_workspace() {
    let workspace = tempfile::tempdir().expect("non-repository workspace");
    let managed_root = tempfile::tempdir().expect("managed worktree root");
    let run_store = Arc::new(InMemoryAgentRunStore::new());
    let task_id = AgentTaskId::new();
    let run_id = AgentRunId::new();
    let workspace_id = WorkspaceId::new_unchecked("a001-repositoryless-workspace");
    run_store
        .create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: "a001-repositoryless-session".into(),
                originating_turn_id: Some("a001-repositoryless-turn".into()),
                project_id: ProjectId::new(),
                repository_id: None,
                workspace_id: workspace_id.clone(),
                requested_agent: CHILD_AGENT.into(),
                delegation_key: "a001-repositoryless-placement".into(),
                request_fingerprint: "a001-repositoryless-fingerprint".into(),
                description: "read-only isolated placement without a repository".into(),
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
                authority_digest: "a001-repositoryless-authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create durable run");
    let (_, _, pool) = build_executor("README").await;
    let worktrees = codegg_core::worktree_service::WorktreeService::memory(
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    );
    let executor = SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
        pool,
        run_store.clone(),
        None,
        worktrees.clone(),
    );
    let mut job = subagent_job_without_allowed_paths();
    job.job_id = JobId::new_unchecked("a001-repositoryless-isolated-new");
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
        delegation_key: "a001-repositoryless-placement".into(),
        base_commit: None,
        placement: codegg_core::run_placement::RunPlacement::IsolatedNew,
        default_working_directory: None,
    };
    let context = JobExecutionContext {
        job,
        attempt_id: AttemptId::new_unchecked("a001-repositoryless-attempt"),
        daemon_generation: DaemonGeneration::new_unchecked("a001-repositoryless-generation"),
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
        .expect("mark durable run queued before dispatch");
    let completion = executor.execute(context).await;
    assert_eq!(
        completion.status,
        codegg::scheduler::executor::ExecutorStatus::Failed,
        "explicit isolation cannot execute in place without repository identity"
    );
    let run = run_store.get_run(&run_id).await.unwrap().unwrap();
    assert!(run.status.is_terminal());
    assert_eq!(
        run.failure_class.as_deref(),
        Some("explicit_isolation_unavailable")
    );
    assert!(run
        .failure_message
        .as_deref()
        .unwrap_or_default()
        .contains("requires a registered repository"));
    assert!(worktrees.list(Default::default()).await.unwrap().is_empty());
}

/// A001 strict-success regression: explicit read-only placement must produce
/// a real, distinct checkout through the production contained creator before
/// the read-only child runs. The test is intentionally host-qualified: if the
/// platform cannot obtain filesystem containment, a successful in-place child
/// would be a security regression and the executor must fail closed.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn explicit_read_only_isolated_new_uses_two_distinct_contained_checkouts() {
    use codegg::agent::run_control::RunControlService;
    use codegg::core::writer_fence::DaemonWriterFenceFactory;
    use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
    use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
    use codegg_core::agent_run_group::AgentRunGroupService;
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobPriority, JobSource, JobStore,
        NewJob, ResourceRequest, RetryPolicy,
    };
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use codegg_core::worktree_service::WorktreeService;
    use codegg_core::writer_permit::WriterPermitStore;

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
    let permit_pool = common::pool::isolated_pool().await;
    let run_store = Arc::new(codegg_core::agent_run::SqliteAgentRunStore::new(
        permit_pool.clone(),
    ));
    let writer_permits = WriterPermitService::new(
        WriterPermitStore::new(permit_pool.clone()),
        WorktreePermitAuthority::new(worktrees.clone()),
    );
    let writer_fences = DaemonWriterFenceFactory::with_team_state(
        writer_permits,
        worktrees.clone(),
        run_store.clone(),
        None,
        None,
    ) as Arc<dyn codegg::tool::writer_fence::WriterFenceFactory>;
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace_record = workspace_registry
        .get_or_register(workspace.path())
        .await
        .expect("register parent workspace");
    let workspace_id = workspace_record.id.clone();
    let project_id = ProjectId::new();
    let repository_id = RepositoryId::new();
    let workspace_services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let mut checkout_paths = Vec::new();

    for ordinal in 0..2 {
        let (_, observed_tool_result, pool) =
            build_executor_with_pool("README", permit_pool.clone()).await;
        let executor = SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
            pool.clone(),
            run_store.clone(),
            None,
            worktrees.clone(),
        )
        .with_authority(codegg::scheduler::executors::SubagentAuthority {
            writer_fences: Some(writer_fences.clone()),
            ..Default::default()
        });
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        run_store
            .create_or_get(
                NewAgentTask {
                    task_id: task_id.clone(),
                    parent_task_id: None,
                    originating_session_id: "a001-contained-session".into(),
                    originating_turn_id: Some("a001-contained-turn".into()),
                    project_id: project_id.clone(),
                    repository_id: Some(repository_id.clone()),
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
        let durable_runs: Arc<dyn AgentRunStore> = run_store.clone();
        pool.configure_durable_delegation(
            submission.clone(),
            durable_runs.clone(),
            RunControlService::in_memory(durable_runs.clone()),
            AgentRunGroupService::in_memory(durable_runs),
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
        if completion.state != JobState::Completed {
            let failed_run = run_store.get_run(&run_id).await.unwrap();
            panic!(
                "contained creator or read-only child failed: job={completion:#?}; run={failed_run:#?}"
            );
        }
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
        assert_eq!(record.project_id, project_id);
        assert_eq!(record.repository_id, repository_id);
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

async fn submit_existing_managed_read_only_case(
    run_store: &Arc<codegg_core::agent_run::SqliteAgentRunStore>,
    submission: &codegg::scheduler::JobSubmissionService,
    workspace_id: &WorkspaceId,
    project_id: &ProjectId,
    repository_id: &RepositoryId,
    worktree_id: codegg_core::identity::WorktreeId,
    case_id: &str,
) -> (AgentRunId, JobId) {
    use codegg_core::agent_run::AgentRunStatus;
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, JobPriority, JobSource, NewJob, ResourceRequest,
        RetryPolicy,
    };

    let task_id = AgentTaskId::new();
    let run_id = AgentRunId::new();
    let delegation_key = format!("a002-{case_id}");
    run_store
        .create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: "a002-parent-session".into(),
                originating_turn_id: Some("a002-parent-turn".into()),
                project_id: project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: workspace_id.clone(),
                requested_agent: CHILD_AGENT.into(),
                delegation_key: delegation_key.clone(),
                request_fingerprint: format!("a002-{case_id}-fingerprint"),
                description: "read one existing managed checkout".into(),
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
                authority_digest: "a002-read-only-authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create durable delegated run");
    run_store
        .transition(&run_id, AgentRunStatus::Queued)
        .await
        .expect("mark run queued");

    let payload = JobPayload::SubagentRun {
        prompt: "Read the tracked README.".into(),
        agent: CHILD_AGENT.into(),
        model: Some(format!("{PROVIDER_ID}/model")),
        parent_id: None,
        denied_tools: MUTATING_TOOLS.iter().map(|name| (*name).into()).collect(),
        allowed_paths: Vec::new(),
        max_tool_calls: None,
        task_id,
        run_id: run_id.clone(),
        delegation_key,
        base_commit: None,
        placement: codegg_core::run_placement::RunPlacement::ExistingManagedReadOnly {
            worktree_id,
        },
        default_working_directory: None,
    };
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
        .expect("submit selected checkout through production scheduler");
    (run_id, submitted.job_id)
}

async fn wait_for_scheduled_job(
    job_store: &Arc<codegg_core::jobs::InMemoryJobStore>,
    job_id: &JobId,
) -> JobRecord {
    use codegg_core::jobs::JobStore;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let record = job_store
            .get_job(job_id)
            .await
            .expect("read scheduled job")
            .expect("submitted job exists");
        if record.state.is_terminal() {
            return record;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "job timed out: {record:#?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_managed_read_only_runs_through_the_production_scheduler() {
    use codegg::agent::run_control::RunControlService;
    use codegg::core::writer_fence::DaemonWriterFenceFactory;
    use codegg::core::writer_permit::{WorktreePermitAuthority, WriterPermitService};
    use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
    use codegg_core::agent_run_group::AgentRunGroupService;
    use codegg_core::jobs::{
        ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobPriority, JobSource, JobStore,
        NewJob, ResourceRequest, RetryPolicy,
    };
    use codegg_core::project_catalog::{ProjectCatalog, RegisterLocalProject};
    use codegg_core::project_storage::ProjectStorage;
    use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };
    use codegg_core::worktree_service::{
        ContainedWorktreeCreator, ContainedWorktreeOutcome, ContainedWorktreeRequest,
        CreateWorktreeRequest, ManagedWorktreeState, WorktreeHealth, WorktreeService,
    };
    use codegg_core::writer_permit::WriterPermitStore;

    let workspace = tempfile::tempdir().expect("parent workspace");
    common::blackboard::init_repository(workspace.path());
    assert!(std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://example.test/a002/fixture.git"
        ])
        .current_dir(workspace.path())
        .status()
        .expect("configure stable repository identity")
        .success());
    std::fs::write(workspace.path().join("README"), "parent-only\n").unwrap();
    let managed_root = tempfile::tempdir().expect("managed root");
    // The selected checkout is pre-provisioned by the fixture. This creator
    // exercises WorktreeService's record/lease path on every host; it is not
    // containment evidence and is never used by production code.
    struct FixtureWorktreeCreator;
    #[async_trait]
    impl ContainedWorktreeCreator for FixtureWorktreeCreator {
        async fn create(
            &self,
            request: &ContainedWorktreeRequest,
        ) -> Result<ContainedWorktreeOutcome, codegg_core::worktree_service::WorktreeServiceError>
        {
            let branch = std::process::Command::new("git")
                .args(["branch", &request.branch, &request.base_commit])
                .current_dir(&request.repository_root)
                .output()
                .map_err(|error| {
                    codegg_core::worktree_service::WorktreeServiceError::Git(error.to_string())
                })?;
            if !branch.status.success() {
                return Err(codegg_core::worktree_service::WorktreeServiceError::Git(
                    String::from_utf8_lossy(&branch.stderr).into_owned(),
                ));
            }
            let added = std::process::Command::new("git")
                .args(["worktree", "add", "--quiet"])
                .arg(&request.worktree_path)
                .arg(&request.branch)
                .current_dir(&request.repository_root)
                .output()
                .map_err(|error| {
                    codegg_core::worktree_service::WorktreeServiceError::Git(error.to_string())
                })?;
            if !added.status.success() {
                return Err(codegg_core::worktree_service::WorktreeServiceError::Git(
                    String::from_utf8_lossy(&added.stderr).into_owned(),
                ));
            }
            Ok(ContainedWorktreeOutcome {
                administration_dir: request.repository_gitdir.join("worktrees"),
                backend: "test-fixture-only".into(),
                abi: None,
                network_enforced: false,
            })
        }
    }
    let pool = common::pool::isolated_pool().await;
    let run_store = Arc::new(codegg_core::agent_run::SqliteAgentRunStore::new(
        pool.clone(),
    ));
    let workspace_store = Arc::new(SqliteWorkspaceStore::new(pool.clone()));
    let workspace_registry = WorkspaceRegistry::load(workspace_store.clone())
        .await
        .expect("workspace registry");
    let parent_workspace = workspace_registry
        .get_or_register(workspace.path())
        .await
        .expect("register parent workspace");
    let catalog = ProjectCatalog::new(pool.clone());
    let project = catalog
        .register_local_project(
            RegisterLocalProject {
                display_name: "A002 existing checkout fixture".into(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &parent_workspace.id,
            "a002-read-only-fixture",
        )
        .await
        .expect("register fixture project");
    let project_storage = ProjectStorage::new(pool.clone());
    let binding = project_storage
        .workspace_binding(&parent_workspace.id)
        .await
        .unwrap()
        .unwrap();
    let repository_id = binding.repository_id.unwrap();
    let worktrees = WorktreeService::sqlite(
        pool.clone(),
        managed_root.path().to_path_buf(),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    )
    .with_contained_creator(Arc::new(FixtureWorktreeCreator));
    let owner = AgentRunId::new();
    let (created, creation_lease) = worktrees
        .create(&CreateWorktreeRequest {
            project_id: project.project_id.clone(),
            repository_id: repository_id.clone(),
            workspace_id: parent_workspace.id.clone(),
            node_id: None,
            repository_root: workspace.path().to_path_buf(),
            base_commit: None,
            base_ref: None,
            target_branch: None,
            base_path: Some(workspace.path().to_path_buf()),
            owner_run_id: owner.clone(),
        })
        .await
        .expect("create the selected managed checkout");
    worktrees
        .release_lease(&created.worktree_id, &owner, creation_lease.generation)
        .await
        .expect("release fixture creation lease");
    let selected_workspace = workspace_registry
        .get_or_register(&created.path)
        .await
        .expect("register selected checkout workspace");
    project_storage
        .bind_checkout_workspace(
            &selected_workspace,
            &project.project_id,
            &repository_id,
            &created.worktree_id,
            "a002-read-only-fixture",
        )
        .await
        .expect("bind checkout to source project");

    let writer_permits = WriterPermitService::new(
        WriterPermitStore::new(pool.clone()),
        WorktreePermitAuthority::new(worktrees.clone()),
    );
    let writer_fences = DaemonWriterFenceFactory::with_team_state(
        writer_permits,
        worktrees.clone(),
        run_store.clone(),
        None,
        None,
    ) as Arc<dyn codegg::tool::writer_fence::WriterFenceFactory>;
    let (_, observed_tool_result, executor_pool) =
        build_executor_with_pool("README", pool.clone()).await;
    let executor = SubagentJobExecutor::new_with_agent_runs_and_control_and_worktree(
        executor_pool.clone(),
        run_store.clone(),
        None,
        worktrees.clone(),
    )
    .with_authority(codegg::scheduler::executors::SubagentAuthority {
        writer_fences: Some(writer_fences),
        origin_attribution_pool: Some(pool.clone()),
        ..Default::default()
    });
    let task_id = AgentTaskId::new();
    let run_id = AgentRunId::new();
    run_store
        .create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: "a002-parent-session".into(),
                originating_turn_id: Some("a002-parent-turn".into()),
                project_id: project.project_id.clone(),
                repository_id: Some(repository_id.clone()),
                workspace_id: parent_workspace.id.clone(),
                requested_agent: CHILD_AGENT.into(),
                delegation_key: "a002-existing-read-only".into(),
                request_fingerprint: "a002-existing-read-only-v1".into(),
                description: "read one existing managed checkout".into(),
            },
            NewAgentRun {
                run_id: run_id.clone(),
                parent_run_id: None,
                depth: 1,
                workspace_id: parent_workspace.id.clone(),
                agent_name: CHILD_AGENT.into(),
                agent_digest: None,
                provider: PROVIDER_ID.into(),
                model: format!("{PROVIDER_ID}/model"),
                authority_digest: "a002-read-only-authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create durable delegated run");
    let payload = JobPayload::SubagentRun {
        prompt: "Read the tracked README.".into(),
        agent: CHILD_AGENT.into(),
        model: Some(format!("{PROVIDER_ID}/model")),
        parent_id: None,
        denied_tools: MUTATING_TOOLS.iter().map(|name| (*name).into()).collect(),
        allowed_paths: Vec::new(),
        max_tool_calls: None,
        task_id,
        run_id: run_id.clone(),
        delegation_key: "a002-existing-read-only".into(),
        base_commit: None,
        placement: codegg_core::run_placement::RunPlacement::ExistingManagedReadOnly {
            worktree_id: created.worktree_id.clone(),
        },
        default_working_directory: None,
    };
    let job_store = Arc::new(InMemoryJobStore::new());
    let generation = DaemonGeneration::new_unchecked("a002-existing-read-only");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let scheduler = JobScheduler::new(
        job_store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    scheduler
        .register_executor(Arc::new(executor))
        .await
        .expect("register subagent executor");
    let submission =
        JobSubmissionService::new(job_store.clone(), scheduler.clone(), services, generation);
    let durable_runs: Arc<dyn AgentRunStore> = run_store.clone();
    executor_pool.configure_durable_delegation(
        submission.clone(),
        durable_runs.clone(),
        RunControlService::in_memory(durable_runs.clone()),
        AgentRunGroupService::in_memory(durable_runs),
    );
    run_store
        .transition(&run_id, codegg_core::agent_run::AgentRunStatus::Queued)
        .await
        .expect("mark run queued");
    let scheduler_task = scheduler.clone().spawn_run();
    let submitted = submission
        .submit(
            None,
            NewJob {
                workspace_id: parent_workspace.id.clone(),
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
        .expect("submit selected checkout through production scheduler");
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
            "job timed out: {record:#?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    if completion.state != JobState::Completed {
        panic!(
            "existing checkout run failed: job={completion:#?}; run={:#?}",
            run_store.get_run(&run_id).await.unwrap()
        );
    }

    // Exercise the executor's fail-closed matrix through the same production
    // submission and scheduler path as the successful placement above.
    let (missing_run, missing_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        codegg_core::identity::WorktreeId::parse("missing-a002-managed-worktree").unwrap(),
        "missing-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &missing_job).await.state,
        JobState::Failed
    );
    let missing_record = run_store.get_run(&missing_run).await.unwrap().unwrap();
    assert_eq!(
        missing_record.failure_class.as_deref(),
        Some("existing_worktree_unavailable"),
        "missing managed checkout refusal: {missing_record:#?}"
    );

    let selected = worktrees.get(&created.worktree_id).await.unwrap();
    let dirty_marker = selected.path.join("a002-untracked-dirty-marker");
    std::fs::write(&dirty_marker, "untracked").unwrap();
    worktrees.refresh(&created.worktree_id).await.unwrap();
    let (dirty_run, dirty_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "dirty-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &dirty_job).await.state,
        JobState::Failed
    );
    assert_eq!(
        run_store
            .get_run(&dirty_run)
            .await
            .unwrap()
            .unwrap()
            .failure_class
            .as_deref(),
        Some("existing_worktree_unavailable")
    );
    std::fs::remove_file(&dirty_marker).unwrap();
    worktrees.refresh(&created.worktree_id).await.unwrap();
    sqlx::query(
        "UPDATE managed_worktree SET state = 'ready', health = 'clean' WHERE worktree_id = ?",
    )
    .bind(created.worktree_id.as_str())
    .execute(&pool)
    .await
    .unwrap();

    let lease_owner = AgentRunId::new();
    let held_lease = worktrees
        .acquire_lease(&created.worktree_id, &lease_owner)
        .await
        .expect("hold checkout for the busy refusal case");
    let (busy_run, busy_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "busy-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &busy_job).await.state,
        JobState::Failed
    );
    assert_eq!(
        run_store
            .get_run(&busy_run)
            .await
            .unwrap()
            .unwrap()
            .failure_class
            .as_deref(),
        Some("existing_worktree_in_use")
    );
    worktrees
        .release_lease(&created.worktree_id, &lease_owner, held_lease.generation)
        .await
        .expect("release fixture busy-case lease");

    let session_store = codegg_core::session::store::SessionStore::new(pool.clone());
    let active_session = session_store
        .create(codegg_core::session::models::CreateSession {
            project_id: project.project_id.as_str().to_owned(),
            directory: selected.path.to_string_lossy().into_owned(),
            workspace_id: Some(selected_workspace.id.as_str().to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    let (session_run, session_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "active-session-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &session_job).await.state,
        JobState::Failed
    );
    let session_run = run_store.get_run(&session_run).await.unwrap().unwrap();
    assert_eq!(
        session_run.failure_class.as_deref(),
        Some("existing_worktree_in_use"),
        "active session refusal: {session_run:#?}"
    );
    session_store.archive(&active_session.id).await.unwrap();

    // Catalog metadata is modified only inside this isolated fixture. The
    // scheduler must refuse a managed record that claims another project.
    sqlx::query("UPDATE managed_worktree SET project_id = ? WHERE worktree_id = ?")
        .bind("foreign-a002-project")
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let (foreign_run, foreign_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "foreign-project-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &foreign_job).await.state,
        JobState::Failed
    );
    assert_eq!(
        run_store
            .get_run(&foreign_run)
            .await
            .unwrap()
            .unwrap()
            .failure_class
            .as_deref(),
        Some("existing_worktree_unavailable")
    );
    sqlx::query("UPDATE managed_worktree SET project_id = ? WHERE worktree_id = ?")
        .bind(project.project_id.as_str())
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("UPDATE managed_worktree SET repository_id = ? WHERE worktree_id = ?")
        .bind("foreign-a002-repository")
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let (foreign_repository_run, foreign_repository_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "foreign-repository-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &foreign_repository_job)
            .await
            .state,
        JobState::Failed
    );
    let foreign_repository_run = run_store
        .get_run(&foreign_repository_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        foreign_repository_run.failure_class.as_deref(),
        Some("existing_worktree_unavailable"),
        "foreign repository refusal: {foreign_repository_run:#?}"
    );
    sqlx::query("UPDATE managed_worktree SET repository_id = ? WHERE worktree_id = ?")
        .bind(repository_id.as_str())
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("UPDATE managed_worktree SET workspace_id = ? WHERE worktree_id = ?")
        .bind("foreign-a002-workspace")
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let (foreign_workspace_run, foreign_workspace_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "foreign-workspace-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &foreign_workspace_job)
            .await
            .state,
        JobState::Failed
    );
    let foreign_workspace_run = run_store
        .get_run(&foreign_workspace_run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        foreign_workspace_run.failure_class.as_deref(),
        Some("existing_worktree_unavailable"),
        "foreign workspace refusal: {foreign_workspace_run:#?}"
    );
    sqlx::query("UPDATE managed_worktree SET workspace_id = ? WHERE worktree_id = ?")
        .bind(parent_workspace.id.as_str())
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("UPDATE managed_worktree SET managed = 0 WHERE worktree_id = ?")
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let (unmanaged_run, unmanaged_job) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "unmanaged-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &unmanaged_job)
            .await
            .state,
        JobState::Failed
    );
    assert_eq!(
        run_store
            .get_run(&unmanaged_run)
            .await
            .unwrap()
            .unwrap()
            .failure_class
            .as_deref(),
        Some("existing_worktree_unavailable")
    );
    sqlx::query("UPDATE managed_worktree SET managed = 1 WHERE worktree_id = ?")
        .bind(created.worktree_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

    let active_job = job_store
        .create_job(NewJob {
            workspace_id: selected_workspace.id.clone(),
            session_id: None,
            turn_id: None,
            kind: JobKind::Build,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::ManagedArgv {
                argv: vec!["true".into()],
                cwd: Some(selected.path.to_string_lossy().into_owned()),
                sandbox_profile: None,
                network_intent: Default::default(),
            },
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
        })
        .await
        .unwrap();
    let active_attempt = job_store
        .begin_attempt(
            &active_job.job_id,
            &DaemonGeneration::new_unchecked("a002-active-job"),
        )
        .await
        .unwrap();
    job_store
        .mark_attempt_running(&active_attempt.attempt_id)
        .await
        .unwrap();
    let (active_job_run, active_job_submission) = submit_existing_managed_read_only_case(
        &run_store,
        &submission,
        &parent_workspace.id,
        &project.project_id,
        &repository_id,
        created.worktree_id.clone(),
        "active-job-target",
    )
    .await;
    assert_eq!(
        wait_for_scheduled_job(&job_store, &active_job_submission)
            .await
            .state,
        JobState::Failed
    );
    let active_job_run = run_store.get_run(&active_job_run).await.unwrap().unwrap();
    assert_eq!(
        active_job_run.failure_class.as_deref(),
        Some("existing_worktree_in_use"),
        "active scheduler job refusal: {active_job_run:#?}"
    );

    scheduler
        .shutdown(codegg::scheduler::SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    scheduler_task.await.expect("join scheduler");
    let observed = observed_tool_result
        .lock()
        .await
        .clone()
        .unwrap_or_default();
    assert!(
        observed.contains("base") && !observed.contains("parent-only"),
        "child read should resolve in the selected checkout: {observed}"
    );
    let run = run_store.get_run(&run_id).await.unwrap().unwrap();
    assert_eq!(run.worktree_id.as_ref(), Some(&created.worktree_id));
    assert!(
        run.read_only_worktree,
        "the effective placement survives reload"
    );
    let selected = worktrees.get(&created.worktree_id).await.unwrap();
    assert_eq!(selected.state, ManagedWorktreeState::Ready);
    assert_eq!(selected.health, WorktreeHealth::Clean);
    assert_eq!(selected.owner_run_id, None);
    assert!(!worktrees
        .has_read_only_lease(&created.worktree_id)
        .await
        .unwrap());
    assert_eq!(
        std::fs::read_to_string(selected.path.join("README")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("README")).unwrap(),
        "parent-only\n"
    );
    assert_eq!(
        worktrees.list(Default::default()).await.unwrap().len(),
        1,
        "existing placement must not allocate a second checkout"
    );
}
