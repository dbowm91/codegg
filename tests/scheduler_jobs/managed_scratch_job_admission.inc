use std::sync::Arc;

use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::tool::backend::{ToolBackendKind, ToolExecutionContext};
use codegg::tool::task::{TaskStore, TaskTool};
use codegg::tool::Tool;
use codegg_core::agent_run::{AgentRunStore, InMemoryAgentRunStore};
use codegg_core::identity::ScratchId;
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobPayload, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio::sync::Mutex;

#[tokio::test(flavor = "current_thread")]
async fn task_spawn_persists_explicit_scratch_grant_in_admitted_job() {
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
    let generation = DaemonGeneration::new_unchecked("managed-scratch-m002");
    let scheduler = JobScheduler::new(
        job_store.clone(),
        services.clone(),
        ResolvedSchedulerConfig::default(),
        generation.clone(),
    );
    let submission = JobSubmissionService::new(job_store.clone(), scheduler, services, generation);
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    let task = TaskTool::new(
        Arc::new(Mutex::new(TaskStore::new())),
        None,
        Some("session-managed-scratch".into()),
        vec![],
    )
    .with_submission(submission, workspace.path().to_path_buf())
    .with_agent_run_store(runs.clone())
    .with_turn_owner("session-managed-scratch".into(), "turn-m002".into())
    .with_project_context(None, None, Some("turn-m002".into()));

    let created = task
        .execute_structured(
            serde_json::json!({
                "action": "scratch_create",
                "scratch_alias": "handoff"
            }),
            Some(tool_context("scratch-create")),
        )
        .await
        .expect("create parent-owned scratch");
    let scratch_id = created
        .output
        .lines()
        .find_map(|line| line.strip_prefix("ScratchId: "))
        .and_then(|id| ScratchId::parse(id).ok())
        .expect("opaque scratch id returned");

    task.execute_structured(
        serde_json::json!({
            "action": "spawn",
            "description": "consume shared scratch",
            "prompt": "Read the explicitly granted artifact",
            "agent": "general",
            "idempotency_key": "spawn-m002",
            "shared_scratch": [{ "id": scratch_id.to_string(), "alias": "handoff" }]
        }),
        Some(tool_context("task-spawn")),
    )
    .await
    .expect("spawn submits durable subagent job");

    let run = runs
        .list_by_session("session-managed-scratch")
        .await
        .expect("list accepted child run")
        .into_iter()
        .next()
        .expect("child run persisted");
    assert_eq!(run.shared_scratch.len(), 1);
    assert_eq!(run.shared_scratch[0].scratch_id, scratch_id);

    let job_id = run.job_id.expect("scheduler attached admitted job");
    let job = job_store
        .get_job(&job_id)
        .await
        .expect("read job")
        .expect("job persisted");
    let JobPayload::SubagentRun { run_id, .. } = job.payload else {
        panic!("task spawn must use durable subagent payload");
    };
    assert_eq!(run_id, run.run_id);
}

fn tool_context(invocation_key: &str) -> ToolExecutionContext {
    let mut context = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    context.session_id = Some("session-managed-scratch".into());
    context.turn_id = Some("turn-m002".into());
    context.invocation_key = Some(invocation_key.into());
    context
}
