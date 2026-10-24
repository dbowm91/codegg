//! The session catalog is unchanged by a delegated run (ADR-0051 option B).
//!
//! A delegated run is not a session. It has no `session` row, it appears in no
//! session enumeration, and the catalog reads identically before and after it
//! runs. Its conversation lives in the delegated transcript store, keyed by
//! the run's durable `AgentRunId`.

use async_trait::async_trait;
use codegg::agent::run_control::RunControlService;
use codegg::agent::worker::{SubAgentPool, SubAgentRequest};
use codegg::agent::{Agent, AgentMode};
use codegg::config::schema::Config;
use codegg::provider::{
    ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, ProviderRegistry,
    TokenUsage,
};
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::session::SessionStore;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask, SqliteAgentRunStore,
};
use codegg_core::agent_run_group::AgentRunGroupService;
use codegg_core::agent_run_transcript::{
    AgentRunTranscriptRead, AgentRunTranscriptStore, SqliteAgentRunTranscriptStore,
};
use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use sqlx::SqlitePool;
use std::sync::Arc;

/// A deterministic provider that returns one scripted response.
#[derive(Clone)]
struct CatalogProvider {
    id: String,
}

#[async_trait]
impl Provider for CatalogProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        "Catalog Test Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let events = vec![
            ChatEvent::TextDelta("the child's final answer".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ];
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok::<_, ProviderError>),
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: format!("{}/model", self.id),
            name: "Test Model".to_string(),
            provider: self.id.clone(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

fn create_test_agent(name: &str, model: &str) -> Agent {
    Agent {
        git_read_only: false,
        name: name.to_string(),
        role: None,
        description: format!("Test agent {}", name),
        mode: AgentMode::Primary,
        mode_name: None,
        model: Some(model.to_string()),
        variant: None,
        temperature: None,
        top_p: None,
        color: None,
        steps: None,
        system_prompt: Some("You are a delegated test agent".to_string()),
        permissions: std::collections::HashMap::new(),
        hidden: false,
        thinking_budget: None,
        reasoning_effort: None,
        fallback_model: None,
        runtime_kind: None,
    }
}

/// A delegated run with a durable identity, as the durable path creates it.
async fn create_durable_run(pool: &SqlitePool) -> AgentRunId {
    let runs = SqliteAgentRunStore::new(pool.clone());
    let submission = runs
        .create_or_get(
            NewAgentTask {
                task_id: AgentTaskId::new(),
                parent_task_id: None,
                originating_session_id: "parent-session".into(),
                originating_turn_id: Some("turn".into()),
                project_id: ProjectId::new(),
                repository_id: None,
                workspace_id: WorkspaceId::new_unchecked("workspace"),
                requested_agent: "catalog-agent".into(),
                delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: "fingerprint".into(),
                description: "a delegated task".into(),
            },
            NewAgentRun {
                run_id: AgentRunId::new(),
                parent_run_id: None,
                depth: 1,
                workspace_id: WorkspaceId::new_unchecked("workspace"),
                agent_name: "catalog-agent".into(),
                agent_digest: None,
                provider: "test".into(),
                model: "test/model".into(),
                authority_digest: "authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: Vec::new(),
            },
        )
        .await
        .expect("create the delegated run");
    submission.run.run_id
}

/// A subagent pool with the durable delegation boundary installed.
async fn durable_pool(pool: SqlitePool, agent: Agent) -> Arc<SubAgentPool> {
    let runs: Arc<dyn AgentRunStore> = Arc::new(SqliteAgentRunStore::new(pool.clone()));
    let run_control = RunControlService::in_memory(runs.clone());
    let run_groups = AgentRunGroupService::with_pool(runs.clone(), pool.clone());

    let root = std::env::current_dir().expect("workspace root");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    workspace_registry
        .get_or_register(root.as_path())
        .await
        .expect("register the workspace");
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
        DaemonGeneration::new_unchecked("gen-catalog-test"),
    );
    let submission = JobSubmissionService::new(
        store,
        scheduler,
        services,
        DaemonGeneration::new_unchecked("gen-catalog-test"),
    );

    let mut registry = ProviderRegistry::new();
    registry.register(CatalogProvider {
        id: "test".to_string(),
    });
    let subagent_pool = SubAgentPool::new(
        &Config::default(),
        vec![agent],
        registry,
        Arc::new(SessionStore::new(pool.clone())),
        Some(pool),
    )
    .await;
    subagent_pool.configure_durable_delegation(submission, runs, run_control, run_groups);
    Arc::new(subagent_pool)
}

/// Every session id the catalog enumerates.
async fn catalog_ids(session_store: &SessionStore) -> Vec<String> {
    let mut ids: Vec<String> = session_store
        .list_all_sessions(Some(256))
        .await
        .expect("enumerate the session catalog")
        .into_iter()
        .map(|session| session.id)
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_delegated_run_appears_in_no_session_enumeration_and_leaves_the_catalog_unchanged() {
    let pool = crate::common::pool::isolated_pool().await;
    let session_store = SessionStore::new(pool.clone());

    // A real session, so the catalog is not trivially empty and a change to
    // it would be visible.
    session_store
        .create(codegg_core::session::CreateSession {
            project_id: "project".into(),
            directory: std::env::current_dir()
                .expect("workspace root")
                .to_string_lossy()
                .into_owned(),
            title: Some("the parent session".into()),
            ..Default::default()
        })
        .await
        .expect("create the parent session");
    let before = catalog_ids(&session_store).await;
    assert_eq!(
        before.len(),
        1,
        "the catalog must not be empty to begin with"
    );

    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("catalog-agent", "test/model"),
    )
    .await;
    let run_id = create_durable_run(&pool).await;
    let request = SubAgentRequest {
        child_instance_id: String::new(),
        originating_turn_id: None,
        task_id: 7,
        run_id: Some(run_id.clone()),
        prompt: "the delegated prompt".into(),
        agent: "catalog-agent".into(),
        parent_id: Some("parent-session".into()),
        parent_run_id: None,
        denied_tools: vec![],
        allowed_paths: vec![],
        description: "a delegated task".into(),
        depth: 0,
        max_tool_calls: None,
        parent_model: None,
        workspace_root: Some(std::env::current_dir().expect("workspace root")),
        workspace_id: None,
        workspace_locks: None,
        parent_sandbox_profile: None,
        sandbox_profile: None,
        process_admission: None,
        writer_fence: None,
        execution_audit: None,
    };
    let task_store = subagent_pool.task_store();
    let spawner = subagent_pool.spawner();
    let task_id = task_store
        .lock()
        .await
        .create_task(
            request.description.clone(),
            request.prompt.clone(),
            request.agent.clone(),
            request.parent_id.clone(),
            request.denied_tools.clone(),
            request.allowed_paths.clone(),
        )
        .await;
    let mut request = request;
    request.task_id = task_id;
    spawner
        .send_async(request)
        .await
        .expect("the delegated request is accepted");
    for _ in 0..200 {
        let store = task_store.lock().await;
        let task = store.get_task(task_id).await;
        drop(store);
        if let Some(task) = task {
            if matches!(
                task.status,
                codegg::tool::task::TaskStatus::Completed
                    | codegg::tool::task::TaskStatus::Failed
                    | codegg::tool::task::TaskStatus::Interrupted
            ) {
                assert!(
                    matches!(task.status, codegg::tool::task::TaskStatus::Completed),
                    "the delegated run must complete, got {:?}",
                    task.status
                );
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // The delegated run really happened: it wrote its transcript. Without
    // this, "the catalog is unchanged" would be true of a run that never
    // started.
    let store = SqliteAgentRunTranscriptStore::new(pool.clone());
    let transcript = store.read(&run_id, 0, 256).await.unwrap();
    assert!(
        matches!(transcript, AgentRunTranscriptRead::Present(_)),
        "the delegated run must have recorded a transcript"
    );

    let after = catalog_ids(&session_store).await;
    assert_eq!(
        after, before,
        "the session catalog must be identical before and after a delegated run"
    );
    let synthetic = format!("parent-session-sub-{task_id}");
    assert!(
        !after.contains(&synthetic),
        "a delegated run must appear in no session enumeration, found {synthetic}"
    );
}
