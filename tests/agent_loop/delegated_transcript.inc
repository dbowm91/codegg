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
use codegg::tool::task::TaskStatus;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, NewAgentRun, NewAgentTask, SqliteAgentRunStore,
};
use codegg_core::agent_run_group::AgentRunGroupService;
use codegg_core::agent_run_transcript::{
    AgentRunTranscriptEntry, AgentRunTranscriptRead, AgentRunTranscriptStore,
    SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole, MAX_TRANSCRIPT_ROW_BYTES,
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
    /// The failure this provider raises once its scripted responses run out.
    ///
    /// `None` keeps the default behaviour: a request past the end of the
    /// script is answered with a plain stop. A provider that fails instead
    /// makes the child's *second* turn the one that fails, which is how a
    /// delegated run fails after it has already observed something.
    fail_after_responses: Option<String>,
}

impl ScriptedProvider {
    fn new(id: &str, responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            responses,
            response_index: Arc::new(Mutex::new(0)),
            id: id.to_string(),
            fail_after_responses: None,
        }
    }

    /// Fail every request past the end of the script instead of answering it.
    fn failing_after_responses(mut self, message: &str) -> Self {
        self.fail_after_responses = Some(message.to_string());
        self
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
        if *index >= self.responses.len() {
            if let Some(message) = &self.fail_after_responses {
                *index += 1;
                return Err(ProviderError::Auth(message.clone()));
            }
        }
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
    let attempts = codegg_core::agent_run_attempt::SqliteAgentRunAttemptStore::new(pool.clone());
    codegg_core::agent_run_attempt::AgentRunAttemptStore::append_attempt(
        &attempts,
        codegg_core::agent_run_attempt::NewAgentRunAttempt {
            run_id: submission.run.run_id.clone(),
            attempt_ordinal: 1,
            event: codegg_core::agent_run_attempt::AgentRunAttemptEvent::Attempt,
            attempt_kind: codegg_core::agent_run_attempt::AgentRunAttemptKind::Original,
            continued_from_ordinal: None,
            principal: "parent-session".into(),
            principal_turn_id: "turn".into(),
            authority_digest: "authority".into(),
            transcript_digest: None,
            failure_class: None,
            ceiling_json: None,
            outcome: None,
            detail: None,
        },
    )
    .await
    .expect("record the delegated run's first attempt");
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

/// A child workspace whose only entry is a FIFO the child's `read` blocks on.
///
/// The FIFO is the test's blocking mechanism: reading a FIFO waits for a
/// writer, so the child's tool stays inside the call until the test writes
/// to it. `mkfifo` is a Unix tool, so every use of this helper is
/// `#[cfg(unix)]`.
#[cfg(unix)]
fn child_workspace_with_fifo(name: &str) -> (ChildWorkspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "codegg-delegated-transcript-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("create the child workspace root");
    let fifo = root.join(name);
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(
        status.success(),
        "mkfifo must create the fifo at {:?}",
        fifo
    );
    (
        ChildWorkspace {
            root,
            workspace_id: WorkspaceId::new(),
        },
        fifo,
    )
}

/// The writer end of the child's FIFO, held by a dedicated OS thread.
///
/// Opening a FIFO for writing blocks until a reader opens it. The `open`
/// returning is therefore the test's proof that the child's `read` has
/// already opened the pipe and is blocked inside it: the pipe holds neither
/// data nor EOF until the test releases it, so the tool cannot have returned.
#[cfg(unix)]
struct FifoWriter {
    attached: tokio::sync::oneshot::Receiver<()>,
    release: std::sync::mpsc::Sender<()>,
    handle: std::thread::JoinHandle<()>,
}

#[cfg(unix)]
impl FifoWriter {
    fn spawn(fifo: &std::path::Path) -> Self {
        let (attached_tx, attached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let fifo = fifo.to_path_buf();
        let handle = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new()
                .write(true)
                .open(&fifo)
                .expect("open the fifo for writing");
            let _ = attached_tx.send(());
            // The release is optional: if the test unwinds, the writer's
            // sender is dropped with it, and closing the pipe without a
            // payload ends the reader's read instead of leaving the
            // runtime waiting on a blocked tool.
            if release_rx.recv().is_ok() {
                std::io::Write::write_all(&mut pipe, b"the delegated child read this line\n")
                    .expect("write the released payload");
            }
            // Dropping the pipe closes it, which is the EOF the reader waits
            // for before its read returns.
        });
        Self {
            attached: attached_rx,
            release: release_tx,
            handle,
        }
    }

    /// Wait until the child's `read` has opened the pipe.
    async fn wait_for_reader(&mut self) {
        match tokio::time::timeout(std::time::Duration::from_secs(30), &mut self.attached).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => panic!("the fifo writer ended before the child's read attached"),
            Err(_) => panic!("the child's read never opened the fifo"),
        }
    }

    /// Release the blocked read and wait for the writer to finish.
    fn release(self) {
        self.release
            .send(())
            .expect("signal the release to the fifo writer");
        self.handle
            .join()
            .expect("the fifo writer thread must finish");
    }
}

/// Start one delegated task and return the task id it runs under.
///
/// The task is handed to the pool but not waited for: a caller that needs to
/// observe the run *while it is in flight* reads the transcript and the task
/// status before the run reaches a terminal state.
async fn start_delegated(subagent_pool: &Arc<SubAgentPool>, request: SubAgentRequest) -> u64 {
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
    task_id
}

/// Wait for a delegated task to reach a terminal state and return its status.
async fn wait_for_terminal(subagent_pool: &Arc<SubAgentPool>, task_id: u64) -> TaskStatus {
    let task_store = subagent_pool.task_store();
    for _ in 0..600 {
        let store = task_store.lock().await;
        let task = store.get_task(task_id).await;
        drop(store);
        if let Some(task) = task {
            if matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Interrupted
            ) {
                return task.status;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the delegated run did not reach a terminal state");
}

/// The status of a delegated task, read without waiting for it to finish.
async fn task_status(subagent_pool: &Arc<SubAgentPool>, task_id: u64) -> TaskStatus {
    let task_store = subagent_pool.task_store();
    let store = task_store.lock().await;
    store
        .get_task(task_id)
        .await
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Pending)
}

/// Run one delegated task and wait for it to reach a terminal state.
async fn run_delegated(subagent_pool: &Arc<SubAgentPool>, request: SubAgentRequest) {
    let task_id = start_delegated(subagent_pool, request).await;
    let status = wait_for_terminal(subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Completed),
        "the delegated run must complete, got {:?}",
        status
    );
}

async fn transcript_entries(
    pool: &SqlitePool,
    run_id: &AgentRunId,
) -> Vec<AgentRunTranscriptEntry> {
    let store = SqliteAgentRunTranscriptStore::new(pool.clone());
    match store.read(run_id, 0, 256).await.unwrap() {
        AgentRunTranscriptRead::Present(page) => page.entries,
        AgentRunTranscriptRead::Absent => Vec::new(),
    }
}

/// Read the transcript until `accepted` accepts it, or the bound expires.
///
/// The bound is a budget on the *wait*, not an expectation about how long the
/// child takes: the poll waits for a state, and the failure message names the
/// rows that were actually stored.
async fn transcript_until(
    pool: &SqlitePool,
    run_id: &AgentRunId,
    what: &str,
    mut accepted: impl FnMut(&[AgentRunTranscriptEntry]) -> bool,
) -> Vec<AgentRunTranscriptEntry> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let entries = transcript_entries(pool, run_id).await;
        if accepted(&entries) {
            return entries;
        }
        if std::time::Instant::now() >= deadline {
            panic!("{what} within 10s, got: {entries:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
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

/// A provider that asks for one `read` of `path` under `call_id` and then
/// answers.
fn reading_provider_with_call_id(call_id: &str, path: &str) -> ScriptedProvider {
    ScriptedProvider::new(
        "test",
        vec![
            vec![
                ChatEvent::ToolCall(codegg::provider::ToolCall {
                    id: call_id.to_string().into(),
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

/// A provider that asks for one `read` of `path` and then answers.
fn reading_provider(path: &str) -> ScriptedProvider {
    reading_provider_with_call_id("call-1", path)
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
    // The call's own row, its order against the result, and the shared call
    // id are the subject of
    // `the_transcript_records_a_call_and_its_result_in_ordinal_order_with_one_row_per_call`.
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

/// The central acceptance criterion: the capture happens **in execution**.
///
/// A delegated child's tool call is committed while the tool is still
/// running, so a reader watching the run sees a call that is pending rather
/// than a transcript that only appears once the child finishes.
///
/// The blocking mechanism is a FIFO in the child's workspace. The child's
/// `read` opens it, and a FIFO read waits for a writer, so the tool stays
/// inside the call until the test writes to the pipe. The state this test
/// observes is a real one, not an inference from elapsed time: the writer's
/// `open` on a FIFO returns only once a reader has opened the pipe, and the
/// pipe holds neither data nor EOF until the test releases it, so the tool
/// provably cannot have returned.
///
/// `mkfifo` is a Unix tool, so this test is `#[cfg(unix)]`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_child_has_committed_its_tool_call_before_the_tool_returns() {
    let pool = crate::common::pool::isolated_pool().await;
    let pipe_name = format!("pipe-{}", uuid::Uuid::new_v4().simple());
    let (workspace, fifo) = child_workspace_with_fifo(&pipe_name);
    let mut writer = FifoWriter::spawn(&fifo);
    let mut registry = ProviderRegistry::new();
    registry.register(reading_provider_with_call_id("call-live", &pipe_name));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    let task_id = start_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the pipe", &workspace),
    )
    .await;

    // The child's `read` has opened the pipe: the tool is inside the call.
    writer.wait_for_reader().await;

    // The call is committed while the tool is still running.
    let entries = transcript_until(
        &pool,
        &run_id,
        "the child's tool call row must be committed while its tool runs",
        |entries| {
            entries.iter().any(|entry| {
                entry.kind == TranscriptKind::ToolCall
                    && entry.redacted_body.contains("tool_call_id=call-live")
            })
        },
    )
    .await;
    assert!(
        !entries.iter().any(|entry| {
            entry.kind == TranscriptKind::ToolResult
                && entry.redacted_body.contains("tool_call_id=call-live")
        }),
        "a tool that has not returned must have no result row, got: {entries:?}"
    );
    let status = task_status(&subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Pending | TaskStatus::Running),
        "the delegated run must still be in flight while its tool runs, got: {status:?}"
    );

    // Release the tool and let the run finish.
    writer.release();
    let status = wait_for_terminal(&subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Completed),
        "the delegated run must complete once its tool returns, got: {status:?}"
    );

    let entries = transcript_entries(&pool, &run_id).await;
    let call = entries
        .iter()
        .find(|entry| {
            entry.kind == TranscriptKind::ToolCall
                && entry.redacted_body.contains("tool_call_id=call-live")
        })
        .expect("the tool call row");
    let result = entries
        .iter()
        .find(|entry| {
            entry.kind == TranscriptKind::ToolResult
                && entry.redacted_body.contains("tool_call_id=call-live")
        })
        .expect("the tool result row");
    assert!(
        call.ordinal < result.ordinal,
        "the call must be committed before its result, got call {} and result {}",
        call.ordinal,
        result.ordinal
    );
}

/// A provider that says something and then asks for one `read` of `path`.
///
/// The text delta is what gives the child's committed transcript a
/// **second** row before its tool runs: the advisory for the first row
/// publishes immediately, so the row behind it is the one that only a
/// coalesced flush can release.
fn chatting_reading_provider(call_id: &str, path: &str) -> ScriptedProvider {
    ScriptedProvider::new(
        "test",
        vec![
            vec![
                ChatEvent::TextDelta("the child is about to read the pipe".to_string().into()),
                ChatEvent::ToolCall(codegg::provider::ToolCall {
                    id: call_id.to_string().into(),
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

/// C001: the daemon publishes the advance while the child is still blocked.
///
/// The child is inside a tool that cannot return until the test releases
/// it. Its rows are committed before the tool runs, and the advisory that
/// says "this run advanced" must reach the daemon's canonical event log
/// **while the child is still inside that call** — not only when the run
/// finishes.
///
/// Before the producer's bounded flush, a quiet run's coalesced rows were
/// held until the next append, the recording-stopped state or the run's
/// completion. A follower watching a blocked child therefore saw the run's
/// first row and nothing after it, for as long as the child was quiet.
///
/// The blocking mechanism is the same FIFO the live-child test uses, so
/// the tool provably cannot have returned: the writer's `open` returns only
/// once a reader has opened the pipe, and the pipe holds neither data nor
/// EOF until the test releases it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_daemon_publishes_the_advance_while_the_child_is_still_blocked() {
    let pool = crate::common::pool::isolated_pool().await;
    let pipe_name = format!("pipe-{}", uuid::Uuid::new_v4().simple());
    let (workspace, fifo) = child_workspace_with_fifo(&pipe_name);
    let mut writer = FifoWriter::spawn(&fifo);
    let mut registry = ProviderRegistry::new();
    registry.register(chatting_reading_provider("call-quiet", &pipe_name));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    // The daemon's canonical event log, and the publisher that reaches it.
    let event_log = Arc::new(codegg::core::event_log::EventLog::new(64));
    let mut events = event_log.subscribe();
    let runs: Arc<dyn AgentRunStore> = Arc::new(SqliteAgentRunStore::new(pool.clone()));
    let sink = Arc::new(
        codegg::agent::transcript_publisher::EventLogTranscriptAdvanceSink::new(
            event_log.clone(),
            runs,
        ),
    );
    subagent_pool.set_transcript_publisher(Arc::new(
        codegg::agent::transcript_publisher::CoalescingTranscriptPublisher::new(sink),
    ));

    let run_id = create_durable_run(&pool).await;
    let task_id = start_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the pipe", &workspace),
    )
    .await;

    // The child's `read` has opened the pipe: the tool is inside the call.
    writer.wait_for_reader().await;

    // The rows are committed while the tool is still running.
    let entries = transcript_until(
        &pool,
        &run_id,
        "the child's tool call row must be committed while its tool runs",
        |entries| {
            entries.iter().any(|entry| {
                entry.kind == TranscriptKind::ToolCall
                    && entry.redacted_body.contains("tool_call_id=call-quiet")
            })
        },
    )
    .await;
    let call_ordinal = entries
        .iter()
        .find(|entry| {
            entry.kind == TranscriptKind::ToolCall
                && entry.redacted_body.contains("tool_call_id=call-quiet")
        })
        .expect("the tool call row")
        .ordinal;
    assert!(
        call_ordinal >= 1,
        "the tool call must sit behind the child's own first row, so its advisory is a coalesced one, got ordinal {call_ordinal}"
    );

    // The advisory must reach the event log while the child is still
    // inside the tool.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut advised: Option<i64> = None;
    while advised.is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the advance for the committed rows must reach the event log within 10s"
        );
        let Ok(Ok(envelope)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), events.recv()).await
        else {
            continue;
        };
        if let codegg::protocol::core::CoreEvent::AgentRunTranscriptAdvanced {
            run_id: advised_run,
            ordinal,
            ..
        } = envelope.payload
        {
            if advised_run == run_id.as_ref() && ordinal >= call_ordinal {
                advised = Some(ordinal);
            }
        }
    }
    let status = task_status(&subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Pending | TaskStatus::Running),
        "the delegated run must still be in flight while its tool runs, got: {status:?}"
    );

    // Release the tool and let the run finish.
    writer.release();
    let status = wait_for_terminal(&subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Completed),
        "the delegated run must complete once its tool returns, got: {status:?}"
    );
}

/// A failed child keeps the observations it made before the failure.
///
/// The child's first turn observes something and its second turn fails, so
/// the transcript must hold the earlier call and close with the failure note.
#[tokio::test]
async fn a_failed_child_leaves_the_observations_it_made_before_the_failure() {
    let pool = crate::common::pool::isolated_pool().await;
    let workspace = ChildWorkspace::new("notes.txt", "the delegated child read this line\n");
    let mut registry = ProviderRegistry::new();
    registry.register(
        ScriptedProvider::new(
            "test",
            vec![vec![
                ChatEvent::ToolCall(codegg::provider::ToolCall {
                    id: "call-failed".to_string().into(),
                    name: "read".to_string().into(),
                    arguments: serde_json::json!({ "path": "notes.txt" }),
                }),
                ChatEvent::Finish {
                    stop_reason: "tool_calls".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]],
        )
        .failing_after_responses("the scripted provider refuses the second turn"),
    );
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    let task_id = start_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the notes", &workspace),
    )
    .await;
    let status = wait_for_terminal(&subagent_pool, task_id).await;
    assert!(
        matches!(status, TaskStatus::Failed),
        "a child whose second turn fails must leave a failed task, got: {status:?}"
    );

    let entries = transcript_entries(&pool, &run_id).await;
    let call = entries
        .iter()
        .find(|entry| {
            entry.kind == TranscriptKind::ToolCall
                && entry.redacted_body.contains("tool_call_id=call-failed")
        })
        .expect("the tool call the child made before the failure");
    assert_eq!(
        call.kind,
        TranscriptKind::ToolCall,
        "the observation made before the failure must stay a tool call row, got: {:?}",
        call.redacted_body
    );
    let last = entries.last().expect("the transcript's last row");
    assert_eq!(
        last.kind,
        TranscriptKind::TerminalNote,
        "a failed child must close its transcript with a terminal note, got: {:?}",
        last.redacted_body
    );
    assert!(
        last.redacted_body.starts_with("Subagent task failed:"),
        "the terminal note must name the failure, got: {:?}",
        last.redacted_body
    );
    assert!(
        last.redacted_body
            .contains("[class=provider_authentication]"),
        "the terminal note must name the failure's class, got: {:?}",
        last.redacted_body
    );
}

/// A child's reasoning is never recorded.
///
/// The provider's reasoning is the provider's private channel: the transcript
/// holds what the child acted on, so the reasoning text must appear nowhere
/// in it.
#[tokio::test]
async fn a_child_records_no_reasoning_row() {
    let pool = crate::common::pool::isolated_pool().await;
    let reasoning = "the child's private reasoning about the answer";
    let provider = ScriptedProvider::new(
        "test",
        vec![vec![
            ChatEvent::ReasoningDelta(reasoning.to_string().into()),
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
    let message_rows: Vec<&AgentRunTranscriptEntry> = entries
        .iter()
        .filter(|entry| entry.kind == TranscriptKind::ModelMessage)
        .collect();
    assert!(
        !message_rows
            .iter()
            .any(|entry| entry.redacted_body.contains(reasoning)),
        "a model message row must never carry the provider's reasoning, got: {message_rows:?}"
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.redacted_body.contains(reasoning)),
        "the reasoning text must appear nowhere in the transcript, got: {entries:?}"
    );
    // The visible answer was captured, so the transcript is not empty: the
    // reasoning is absent from a recorded response, not absent because
    // nothing was recorded.
    assert!(
        message_rows
            .iter()
            .any(|entry| entry.redacted_body == "the child's final answer"),
        "the child's visible answer must still be recorded, got: {message_rows:?}"
    );
}

/// One call is one row, and the result follows the call.
#[tokio::test]
async fn the_transcript_records_a_call_and_its_result_in_ordinal_order_with_one_row_per_call() {
    let pool = crate::common::pool::isolated_pool().await;
    let workspace = ChildWorkspace::new("order.txt", "the delegated child read this line\n");
    let mut registry = ProviderRegistry::new();
    registry.register(reading_provider_with_call_id("call-ordered", "order.txt"));
    let subagent_pool = durable_pool(
        pool.clone(),
        create_test_agent("transcript-agent", "test/model"),
        registry,
    )
    .await;

    let run_id = create_durable_run(&pool).await;
    run_delegated(
        &subagent_pool,
        request_in(run_id.clone(), "read the notes in order", &workspace),
    )
    .await;

    let entries = transcript_entries(&pool, &run_id).await;
    let calls: Vec<&AgentRunTranscriptEntry> = entries
        .iter()
        .filter(|entry| entry.kind == TranscriptKind::ToolCall)
        .collect();
    let results: Vec<&AgentRunTranscriptEntry> = entries
        .iter()
        .filter(|entry| entry.kind == TranscriptKind::ToolResult)
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "one accepted call must be exactly one call row, got: {calls:?}"
    );
    assert_eq!(
        results.len(),
        1,
        "one executed call must be exactly one result row, got: {results:?}"
    );
    assert!(
        calls[0].ordinal < results[0].ordinal,
        "the call must be committed before its result, got call {} and result {}",
        calls[0].ordinal,
        results[0].ordinal
    );
    for row in [calls[0], results[0]] {
        assert!(
            row.redacted_body.contains("tool_call_id=call-ordered"),
            "the call and its result must name the same call id, got: {:?}",
            row.redacted_body
        );
    }
}
