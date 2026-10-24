//! The durable delegated-run transcript (ADR-0051 option B).
//!
//! These tests drive a real delegated run through the real worker and read
//! the transcript back out of the durable store, keyed by the run's durable
//! `AgentRunId`. A delegated run is not a session: no `session` row is
//! created for it.

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
    AgentRunTranscriptRead, AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind,
    TranscriptRole, MAX_TRANSCRIPT_ROW_BYTES,
};
use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A deterministic provider that returns scripted responses.
#[derive(Clone)]
struct ScriptedProvider {
    responses: Vec<Vec<ChatEvent>>,
    response_index: Arc<Mutex<usize>>,
    id: String,
}

impl ScriptedProvider {
    fn new(id: &str, responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            responses,
            response_index: Arc::new(Mutex::new(0)),
            id: id.to_string(),
        }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        "Delegated Transcript Test Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut index = self.response_index.lock().await;
        let events = if *index < self.responses.len() {
            self.responses[*index].clone()
        } else {
            vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }]
        };
        *index += 1;
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
                requested_agent: "transcript-agent".into(),
                delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                request_fingerprint: "fingerprint".into(),
                description: "a delegated task".into(),
            },
            NewAgentRun {
                run_id: AgentRunId::new(),
                parent_run_id: None,
                depth: 1,
                workspace_id: WorkspaceId::new_unchecked("workspace"),
                agent_name: "transcript-agent".into(),
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

/// A subagent pool with the durable delegation boundary installed, so a
/// delegated run with a durable identity can resolve its own run record.
async fn durable_pool(
    pool: SqlitePool,
    agent: Agent,
    provider_registry: ProviderRegistry,
) -> Arc<SubAgentPool> {
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
        DaemonGeneration::new_unchecked("gen-transcript-test"),
    );
    let submission = JobSubmissionService::new(
        store,
        scheduler,
        services,
        DaemonGeneration::new_unchecked("gen-transcript-test"),
    );

    let subagent_pool = SubAgentPool::new(
        &Config::default(),
        vec![agent],
        provider_registry,
        Arc::new(SessionStore::new(pool.clone())),
        Some(pool),
    )
    .await;
    subagent_pool.configure_durable_delegation(submission, runs, run_control, run_groups);
    Arc::new(subagent_pool)
}

fn request(run_id: AgentRunId, prompt: &str) -> SubAgentRequest {
    SubAgentRequest {
        child_instance_id: String::new(),
        originating_turn_id: None,
        task_id: 1,
        run_id: Some(run_id),
        prompt: prompt.to_string(),
        agent: "transcript-agent".to_string(),
        parent_id: Some("parent-session".to_string()),
        parent_run_id: None,
        denied_tools: vec![],
        allowed_paths: vec![],
        description: "a delegated task".to_string(),
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
    }
}

/// A child workspace root holding one file the delegated child can read.
///
/// The child needs a workspace identity too: a tool call is authorized
/// against a workspace identity, and a child with none is refused.
struct ChildWorkspace {
    root: std::path::PathBuf,
    workspace_id: WorkspaceId,
}

impl ChildWorkspace {
    fn new(file_name: &str, contents: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "codegg-delegated-transcript-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create the child workspace root");
        std::fs::write(root.join(file_name), contents).expect("write the child workspace file");
        Self {
            root,
            workspace_id: WorkspaceId::new(),
        }
    }
}

impl Drop for ChildWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A delegated request bound to the child workspace.
fn request_in(run_id: AgentRunId, prompt: &str, workspace: &ChildWorkspace) -> SubAgentRequest {
    let mut request = request(run_id, prompt);
    request.workspace_root = Some(workspace.root.clone());
    request.workspace_id = Some(workspace.workspace_id.clone());
    request
}

/// Run one delegated task and wait for it to reach a terminal state.
async fn run_delegated(subagent_pool: &Arc<SubAgentPool>, request: SubAgentRequest) {
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
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the delegated run did not reach a terminal state");
}

async fn transcript_entries(
    pool: &SqlitePool,
    run_id: &AgentRunId,
) -> Vec<codegg_core::agent_run_transcript::AgentRunTranscriptEntry> {
    let store = SqliteAgentRunTranscriptStore::new(pool.clone());
    match store.read(run_id, 0, 256).await.unwrap() {
        AgentRunTranscriptRead::Present(page) => page.entries,
        AgentRunTranscriptRead::Absent => Vec::new(),
    }
}

#[tokio::test]
async fn a_delegated_run_records_its_prompt_and_final_response() {
    let pool = crate::common::pool::isolated_pool().await;
    let provider = ScriptedProvider::new(
        "test",
        vec![vec![
            ChatEvent::TextDelta("the child's final answer".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]],
    );
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request(run_id.clone(), "the delegated prompt"),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    let bodies: Vec<&str> = entries
        .iter()
        .map(|entry| entry.redacted_body.as_str())
        .collect();
    assert!(
        bodies.contains(&"the delegated prompt"),
        "the transcript must hold the child's prompt, got: {bodies:?}"
    );
    assert!(
        bodies.contains(&"the child's final answer"),
        "the transcript must hold the child's final response, got: {bodies:?}"
    );
    let prompt_row = entries
        .iter()
        .find(|entry| entry.redacted_body == "the delegated prompt")
        .expect("the prompt row");
    assert_eq!(prompt_row.role, TranscriptRole::Orchestrator);
    assert_eq!(prompt_row.kind, TranscriptKind::ModelMessage);
    let response_row = entries
        .iter()
        .find(|entry| entry.redacted_body == "the child's final answer")
        .expect("the final response row");
    assert_eq!(response_row.role, TranscriptRole::Orchestrator);
    assert!(
        response_row.ordinal > prompt_row.ordinal,
        "the final response must come after the prompt"
    );
}

/// A provider that asks for one `read` of `path` and then answers.
fn reading_provider(path: &str) -> ScriptedProvider {
    ScriptedProvider::new(
        "test",
        vec![
            vec![
                ChatEvent::ToolCall(codegg::provider::ToolCall {
                    id: "call-1".to_string().into(),
                    name: "read".to_string().into(),
                    arguments: serde_json::json!({ "path": path }),
                }),
                ChatEvent::Finish {
                    stop_reason: "tool_calls".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ],
            vec![
                ChatEvent::TextDelta("the child's final answer".to_string().into()),
                ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ],
        ],
    )
}

#[tokio::test]
async fn a_delegated_run_records_the_tool_result_its_child_executed() {
    let pool = crate::common::pool::isolated_pool().await;
    let workspace = ChildWorkspace::new("notes.txt", "the delegated child read this line\n");
    let mut registry = ProviderRegistry::new();
    registry.register(reading_provider("notes.txt"));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the notes", &workspace),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    let tool_result = entries
        .iter()
        .find(|entry| entry.kind == TranscriptKind::ToolResult)
        .expect("the tool result row");
    assert!(
        tool_result.redacted_body.contains("tool=read"),
        "the tool result row must name the tool, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result
            .redacted_body
            .contains("the delegated child read this line"),
        "the tool result row must hold the output the tool produced, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result.redacted_body.contains("success=true"),
        "the tool result row must say the tool succeeded, got: {:?}",
        tool_result.redacted_body
    );

    // The call and its result are both present, in that order, naming the same call.
    let tool_call = entries
        .iter()
        .find(|entry| entry.kind == TranscriptKind::ToolCall)
        .expect("the tool call row");
    assert!(
        tool_call.ordinal < tool_result.ordinal,
        "the tool call must come before its result, got call {} and result {}",
        tool_call.ordinal,
        tool_result.ordinal
    );
    for row in [tool_call, tool_result] {
        assert!(
            row.redacted_body.contains("tool_call_id=call-1"),
            "the call and its result must name the same call id, got: {:?}",
            row.redacted_body
        );
    }
}

#[tokio::test]
async fn a_delegated_run_records_a_bounded_tool_result_with_truncation_marked() {
    let pool = crate::common::pool::isolated_pool().await;
    let oversized = "t".repeat(MAX_TRANSCRIPT_ROW_BYTES * 3);
    let workspace = ChildWorkspace::new("big.txt", &oversized);
    let mut registry = ProviderRegistry::new();
    registry.register(reading_provider("big.txt"));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the big file", &workspace),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    let tool_result = entries
        .iter()
        .find(|entry| entry.kind == TranscriptKind::ToolResult)
        .expect("the tool result row");
    assert_eq!(
        tool_result.byte_len, MAX_TRANSCRIPT_ROW_BYTES as i64,
        "the tool result row must be bounded by the per-row limit"
    );
    assert!(
        tool_result
            .redacted_body
            .ends_with(codegg_core::agent_run_transcript::TRANSCRIPT_ROW_TRUNCATION_MARKER),
        "a bounded tool result must carry the truncation marker, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result.redacted_body.contains("output_truncated=true"),
        "a tool output the loop's own bound cut must say so, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result
            .redacted_body
            .contains(codegg::agent::r#loop::RETAINED_TOOL_OUTPUT_TRUNCATION_MARKER.trim()),
        "the loop's own truncation marker must survive the row bound, got: {:?}",
        tool_result.redacted_body
    );
}

#[tokio::test]
async fn a_delegated_run_redacts_a_secret_in_the_tool_output() {
    let pool = crate::common::pool::isolated_pool().await;
    let secret = "fixture-transcript-secret-value";
    let workspace = ChildWorkspace::new("secret.txt", &format!("api_key={secret}\n"));
    let mut registry = ProviderRegistry::new();
    registry.register(reading_provider("secret.txt"));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the secret file", &workspace),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    let tool_result = entries
        .iter()
        .find(|entry| entry.kind == TranscriptKind::ToolResult)
        .expect("the tool result row");
    assert!(
        !tool_result.redacted_body.contains(secret),
        "the secret must be absent from the stored row, got: {:?}",
        tool_result.redacted_body
    );
    assert!(
        tool_result.redacted_body.contains("[REDACTED"),
        "the stored row must show that a redaction happened, got: {:?}",
        tool_result.redacted_body
    );
}

#[tokio::test]
async fn a_delegated_run_that_executes_no_tool_records_no_tool_result() {
    let pool = crate::common::pool::isolated_pool().await;
    let provider = ScriptedProvider::new(
        "test",
        vec![vec![
            ChatEvent::TextDelta("the child's final answer".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]],
    );
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request(run_id.clone(), "answer without a tool"),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    assert!(
        !entries
            .iter()
            .any(|entry| entry.kind == TranscriptKind::ToolResult),
        "a run that executed no tool must record no tool result, got: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == TranscriptKind::ModelMessage),
        "the run must still have recorded its messages, got: {entries:?}"
    );
}

#[tokio::test]
async fn a_transcript_write_failure_does_not_fail_the_delegated_run() {
    let pool = crate::common::pool::isolated_pool().await;
    let provider = ScriptedProvider::new(
        "test",
        vec![vec![
            ChatEvent::TextDelta("the child's final answer".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]],
    );
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    // The transcript store is unavailable: the table it writes to is gone.
    sqlx::query("DROP TABLE agent_run_transcript")
        .execute(&pool)
        .await
        .expect("drop the transcript table");

    run_delegated(
        &subagent_pool,
        request(run_id.clone(), "the delegated prompt"),
    )
    .await;

    let task_store = subagent_pool.task_store();
    let task = task_store
        .lock()
        .await
        .get_task(1)
        .await
        .expect("the delegated task");
    assert!(
        task.result
            .as_ref()
            .is_some_and(|result| result.contains("the child's final answer")),
        "the delegated run must still record its result, got: {:?}",
        task.result
    );
    let entries = transcript_entries(&pool, &run_id).await;
    assert!(
        entries.is_empty(),
        "a failed transcript write must not leave a partial row, got: {entries:?}"
    );
}
