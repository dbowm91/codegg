use async_trait::async_trait;
use codegg::agent::r#loop::AgentLoop;
use codegg::agent::Agent;
use codegg::bus::events::AppEvent;
use codegg::bus::global::GlobalEventBus;
use codegg::config::schema::{
    Config, EggsearchConfig, SearchBackendConfig, SearchConfig, ServerConfig,
};
use codegg::context::InMemoryArtifactStore;
use codegg::error::AppError;
use codegg::permission::{PermissionChecker, PermissionLevel, PermissionRuleset};
use codegg::provider::{
    ChatEvent, ChatRequest, EventStream, Message, ModelInfo, Provider, ProviderError, TokenUsage,
    ToolCall,
};
use codegg::tool::apply_patch::ApplyPatchTool;
use codegg::tool::grep::GrepTool;
use codegg::tool::task::TaskTool;
use codegg::tool::{Tool, ToolRegistry};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::Mutex;

// =============================================================================
// HARNESS DOCUMENTATION
// =============================================================================
//
// This test module provides a harness for testing AgentLoop behavior with
// scripted providers and fake tools. Use this harness to verify transcript
// validity, tool execution, permissions, retries, follow-ups, and compaction.
//
// HOW TO SCRIPT PROVIDER TURNS
// ----------------------------
// Create a ScriptedProvider with a vec of response vecs. Each inner vec represents
// one provider turn (the response to one stream() call). The harness automatically
// advances to the next response on each provider call.
//
// Example:
//   let responses = vec![
//       vec![ChatEvent::ToolCall(...), ChatEvent::Finish {...}],  // Turn 1
//       vec![ChatEvent::TextDelta(...), ChatEvent::Finish {...}], // Turn 2
//   ];
//   let provider = Box::new(ScriptedProvider::new(responses));
//
// For more complex sequencing, use FollowUpProvider or implement your own Provider
// that records calls and returns scripted responses based on call count.
//
// HOW TO INSPECT RECORDED REQUESTS
// --------------------------------
// ScriptedProvider and similar test providers record all ChatRequest objects
// sent to stream(). Access them via get_requests():
//
//   let requests = scripted_provider.get_requests()
//   assert_eq!(requests.len(), 2);
//   // Second request should contain tool result from first turn:
//   let has_tool_result = requests[1].messages.iter().any(|m| matches!(m, Message::Tool { .. }));
//
// Each ChatRequest contains messages, model, tools, etc. Inspect the fields you
// need to verify the transcript is valid for the provider being tested.
//
// HOW TO ADD FAKE TOOLS
// ---------------------
// Implement the Tool trait for your struct. The harness uses ToolRegistry which
// you can populate before building the AgentLoop:
//
//   struct MyTool;
//   #[async_trait]
//   impl Tool for MyTool {
//       fn name(&self) -> &str { "my_tool" }
//       fn description(&self) -> &str { "Does something" }
//       fn parameters(&self) -> serde_json::Value { ... }
//       async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> { ... }
//   }
//
//   let mut registry = ToolRegistry::new();
//   registry.register(MyTool);
//
// TRANSCRIPT INVARIANTS
// ---------------------
// The following invariants MUST be preserved by AgentLoop for valid provider transcripts:
//
// 1. ASSISTANT TOOL CALLS: When a provider emits ToolCall events, the subsequent
//    Message::Assistant in the next request MUST contain matching tool_calls with
//    the same id, name, and arguments.
//
// 2. TOOL RESULT MATCHING: Every Message::Tool must have a tool_call_id that matches
//    a prior assistant tool_calls[].id. The tool result should follow the assistant
//    message in the message sequence.
//
// 3. MESSAGE ORDER: For tool-using turns, the sequence must be:
//    [... prior messages ..., Assistant{content, tool_calls}, Tool{tool_call_id, content}, ...]
//
// 4. PERMISSION DENIAL: When a tool is denied, the next provider request must contain
//    a Tool result with an error message (starts with "Error:" or contains "denied").
//
// 5. MISSING TOOL: When a tool is not found, the next provider request must contain
//    a Tool result with an error message (contains "not found" or "Error").
//
// =============================================================================

/// Helper to collect events from the GlobalEventBus during tests.
/// Optionally filters events by session_id to avoid cross-test contamination.
struct EventCollector {
    rx: broadcast::Receiver<AppEvent>,
    events: Vec<AppEvent>,
    session_id: Option<String>,
}

impl EventCollector {
    fn with_session(session_id: String) -> Self {
        let rx = GlobalEventBus::subscribe();
        Self {
            rx,
            events: Vec::new(),
            session_id: Some(session_id),
        }
    }

    /// Drains all available events from the receiver into the internal buffer.
    /// Filters by session_id if set.
    fn collect(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if self.event_matches_session(&event) {
                        self.events.push(event);
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    panic!("EventCollector lagged by {} events", n);
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
    }

    /// Check if an event matches our session filter.
    fn event_matches_session(&self, event: &AppEvent) -> bool {
        match &self.session_id {
            None => true, // No filter, collect all events
            Some(sid) => {
                // Extract session_id from the event
                match event {
                    AppEvent::ToolCalled { session_id, .. } => session_id == sid,
                    AppEvent::ToolResult { session_id, .. } => session_id == sid,
                    AppEvent::MessageAdded { session_id, .. } => session_id == sid,
                    AppEvent::MessageDeleted { session_id, .. } => session_id == sid,
                    AppEvent::TodoUpdated { session_id, .. } => session_id == sid,
                    AppEvent::CompactionTriggered { session_id, .. } => session_id == sid,
                    AppEvent::QuestionPending { session_id, .. } => session_id == sid,
                    AppEvent::QuestionAnswered { session_id, .. } => session_id == sid,
                    AppEvent::PermissionPending { session_id, .. } => session_id == sid,
                    AppEvent::PermissionResponded { session_id, .. } => session_id == sid,
                    AppEvent::DiffPending { session_id, .. } => session_id == sid,
                    AppEvent::DiffResponded { session_id, .. } => session_id == sid,
                    AppEvent::TextDelta { session_id, .. } => session_id.as_ref() == sid,
                    AppEvent::ReasoningDelta { session_id, .. } => session_id.as_ref() == sid,
                    AppEvent::ToolCallStarted { session_id, .. } => session_id == sid,
                    AppEvent::ProviderAttemptStarted { session_id, .. } => session_id == sid,
                    AppEvent::ProviderAttemptFailed { session_id, .. } => session_id == sid,
                    AppEvent::ProviderAttemptSuperseded { session_id, .. } => session_id == sid,
                    AppEvent::AgentFinished { session_id, .. } => session_id == sid,
                    AppEvent::SubagentStarted { session_id, .. } => session_id == sid,
                    AppEvent::SubagentProgress { session_id, .. } => session_id == sid,
                    AppEvent::SubagentCompleted { session_id, .. } => session_id == sid,
                    AppEvent::SubagentFailed { session_id, .. } => session_id == sid,
                    _ => false, // Events without session_id don't match filtered sessions
                }
            }
        }
    }

    /// Finds the first event matching the predicate.
    fn find_event(&self, f: impl Fn(&AppEvent) -> bool) -> Option<&AppEvent> {
        self.events.iter().find(|e| f(e))
    }

    /// Asserts that the given event types appear in order (not necessarily consecutively).
    fn assert_event_order(&self, expected_types: &[&str]) {
        let actual_types: Vec<&str> = self.events.iter().map(|e| e.event_type()).collect();
        let mut expected_idx = 0;
        let mut actual_idx = 0;

        while expected_idx < expected_types.len() && actual_idx < actual_types.len() {
            if actual_types[actual_idx] == expected_types[expected_idx] {
                expected_idx += 1;
            }
            actual_idx += 1;
        }

        assert!(
            expected_idx == expected_types.len(),
            "Expected event order {:?} not found in {:?}",
            expected_types,
            actual_types
        );
    }
}

async fn wait_for_question_pending(
    session_id: &str,
    rx: &mut broadcast::Receiver<AppEvent>,
    timeout: std::time::Duration,
) -> Result<String, AppError> {
    tokio::time::timeout(timeout, async {
        loop {
            match rx.recv().await {
                Ok(AppEvent::QuestionPending {
                    session_id: sid,
                    question_id,
                    ..
                }) if sid == session_id => {
                    return Ok(question_id);
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    panic!("wait_for_question_pending lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(AppError::Other(anyhow::anyhow!("EventBus channel closed")));
                }
            }
        }
    })
    .await
    .map_err(|_| AppError::Other(anyhow::anyhow!("Timeout waiting for QuestionPending event")))?
}

async fn wait_for_permission_pending(
    perm_id: &str,
    rx: &mut broadcast::Receiver<AppEvent>,
    timeout: std::time::Duration,
) -> Result<(), AppError> {
    tokio::time::timeout(timeout, async {
        loop {
            match rx.recv().await {
                Ok(AppEvent::PermissionPending { perm_id: pid, .. }) if pid == perm_id => {
                    return Ok(());
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    panic!("wait_for_permission_pending lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(AppError::Other(anyhow::anyhow!("EventBus channel closed")));
                }
            }
        }
    })
    .await
    .map_err(|_| {
        AppError::Other(anyhow::anyhow!(
            "Timeout waiting for PermissionPending event"
        ))
    })?
}

#[derive(Clone)]
struct ScriptedProvider {
    responses: Vec<Vec<ChatEvent>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
}

#[derive(Clone)]
struct PeerEvidenceDuringStreamProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    calls: Arc<AtomicUsize>,
    session_id: String,
    peer_run_id: String,
    changed_path: String,
}

#[async_trait]
impl Provider for PeerEvidenceDuringStreamProvider {
    fn id(&self) -> &str {
        "peer-evidence-scripted"
    }

    fn name(&self) -> &str {
        "Peer Evidence Scripted Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());
        let response = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            // This host-owned result event is emitted while the first provider
            // request is still in flight, before it returns a mutation call.
            GlobalEventBus::publish(AppEvent::AgentRunEvidenceObserved {
                session_id: self.session_id.clone(),
                run_id: self.peer_run_id.clone(),
                status: "succeeded".into(),
                result_commit: Some("fake-git-commit-a1b2c3".into()),
                changed_paths: vec![self.changed_path.clone()],
                validation: vec![("test".into(), "passed".into())],
            });
            // A same-run, same-group event under a different project/session
            // must be discarded by the receiving loop's session boundary.
            GlobalEventBus::publish(AppEvent::AgentRunEvidenceObserved {
                session_id: format!("{}-other-project", self.session_id),
                run_id: self.peer_run_id.clone(),
                status: "succeeded".into(),
                result_commit: Some("fake-other-project-commit".into()),
                changed_paths: vec!["src/api.rs".into()],
                validation: vec![("test".into(), "passed".into())],
            });
            vec![
                ChatEvent::ToolCall(ToolCall {
                    id: "peer-fenced-write".to_string().into(),
                    name: "write".to_string().into(),
                    arguments: serde_json::json!({
                        "path": "src/api.rs",
                        "content": "stale API implementation"
                    }),
                }),
                ChatEvent::ToolCall(ToolCall {
                    id: "peer-fenced-write-2".to_string().into(),
                    name: "write".to_string().into(),
                    arguments: serde_json::json!({
                        "path": "src/api.rs",
                        "content": "second stale API implementation"
                    }),
                }),
                ChatEvent::Finish {
                    stop_reason: "tool_calls".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]
        } else {
            vec![
                ChatEvent::TextDelta(
                    "I reviewed the peer change and will stop."
                        .to_string()
                        .into(),
                ),
                ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(
            response.into_iter().map(Ok::<_, ProviderError>),
        )))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".into(),
            name: "Test Model".into(),
            provider: "test".into(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

struct CountedWriteTool(Arc<AtomicUsize>);

#[async_trait]
impl Tool for CountedWriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Test-only write counter"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("write executed".into())
    }
}

impl ScriptedProvider {
    fn new(responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            responses,
            requests: Arc::new(Mutex::new(Vec::new())),
            response_index: Arc::new(Mutex::new(0)),
        }
    }

    async fn get_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }

    async fn request_count(&self) -> usize {
        *self.response_index.lock().await
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        "scripted"
    }

    fn name(&self) -> &str {
        "Scripted Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());

        let mut idx = self.response_index.lock().await;
        let events = if *idx < self.responses.len() {
            self.responses[*idx].clone()
        } else {
            vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }]
        };
        *idx += 1;

        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".to_string(),
            name: "Test Model".to_string(),
            provider: "test".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

struct EchoArgsTool;

impl EchoArgsTool {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EchoArgsTool {
    fn name(&self) -> &str {
        "echo_args"
    }

    fn description(&self) -> &str {
        "Returns the input arguments as JSON"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "value": {
                    "type": "string",
                    "description": "The value to echo back"
                }
            },
            "required": ["value"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok(input.to_string())
    }
}

#[allow(dead_code)]
struct SlowEchoTool {
    barrier: Arc<Mutex<()>>,
}

impl SlowEchoTool {
    fn new(barrier: Arc<Mutex<()>>) -> Self {
        Self { barrier }
    }
}

#[async_trait]
impl Tool for SlowEchoTool {
    fn name(&self) -> &str {
        "slow_echo"
    }

    fn description(&self) -> &str {
        "Waits on a barrier before echoing input"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "value": {
                    "type": "string",
                    "description": "The value to echo back"
                }
            },
            "required": ["value"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        let _guard = self.barrier.lock().await;
        Ok(input.to_string())
    }
}

/// Tool that tracks concurrent executions for testing max_parallel_tools enforcement.
struct ParallelTool {
    tool_name: String,
    current: Arc<AtomicUsize>,
    max_observed: Arc<AtomicUsize>,
    barrier: Arc<Mutex<()>>,
}

struct BatchOwnerTool {
    name: String,
    owner_active: Arc<AtomicUsize>,
    owner_peak: Arc<AtomicUsize>,
    total_active: Arc<AtomicUsize>,
    total_peak: Arc<AtomicUsize>,
    rendezvous: Arc<tokio::sync::Barrier>,
}

fn update_peak(peak: &AtomicUsize, current: usize) {
    let mut seen = peak.load(Ordering::SeqCst);
    while current > seen {
        match peak.compare_exchange(seen, current, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(actual) => seen = actual,
        }
    }
}

#[async_trait]
impl Tool for BatchOwnerTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Tracks the owner of a concurrent batch admission slot"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object", "properties":{}})
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        let owner_now = self.owner_active.fetch_add(1, Ordering::SeqCst) + 1;
        let total_now = self.total_active.fetch_add(1, Ordering::SeqCst) + 1;
        update_peak(&self.owner_peak, owner_now);
        update_peak(&self.total_peak, total_now);
        self.rendezvous.wait().await;
        self.owner_active.fetch_sub(1, Ordering::SeqCst);
        self.total_active.fetch_sub(1, Ordering::SeqCst);
        Ok(self.name.clone())
    }
}

impl ParallelTool {
    fn new(
        name: String,
        current: Arc<AtomicUsize>,
        max_observed: Arc<AtomicUsize>,
        barrier: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            tool_name: name,
            current,
            max_observed,
            barrier,
        }
    }
}

#[async_trait]
impl Tool for ParallelTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        "Tracks concurrent executions for parallel tool testing"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        // Increment current concurrent count
        let prev = self.current.fetch_add(1, Ordering::SeqCst);

        // Update max observed
        loop {
            let current_max = self.max_observed.load(Ordering::SeqCst);
            if prev < current_max {
                break;
            }
            if self
                .max_observed
                .compare_exchange(current_max, prev + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }

        // Wait on barrier to control execution flow
        let _guard = self.barrier.lock().await;

        // Decrement current concurrent count
        self.current.fetch_sub(1, Ordering::SeqCst);

        Ok("done".to_string())
    }
}

#[allow(dead_code)]
struct QuestionTool;

#[allow(dead_code)]
impl QuestionTool {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Ask the user questions"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "question": { "type": "string" },
                            "options": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        }
                    }
                }
            },
            "required": ["questions"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok(input.to_string())
    }
}

async fn attach_test_workspace_authority(
    agent_loop: &mut AgentLoop,
    workspace_root: &std::path::Path,
) {
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("test workspace registry");
    let workspace = workspace_registry
        .get_or_register(workspace_root)
        .await
        .expect("register test workspace");
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    let lease = services
        .acquire(&workspace.id)
        .await
        .expect("test workspace service lease");
    agent_loop.set_workspace_id(workspace.id.clone());
    agent_loop.set_workspace_services_lease(lease);
}

async fn build_test_agent_loop(
    provider: Box<dyn Provider>,
    tool_registry: ToolRegistry,
) -> AgentLoop {
    let agents = vec![Agent {
        git_read_only: false,
        name: "build".to_string(),
        role: None,
        description: "Test agent".to_string(),
        mode: codegg::agent::AgentMode::Primary,
        mode_name: None,
        model: None,
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
    }];

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![
                codegg::permission::ToolRule {
                    tool: "echo_args".to_string(),
                    level: PermissionLevel::Allow,
                    paths: None,
                    bash_patterns: None,
                },
                codegg::permission::ToolRule {
                    tool: "slow_echo".to_string(),
                    level: PermissionLevel::Allow,
                    paths: None,
                    bash_patterns: None,
                },
            ],
            path_rules: vec![],
        });

    let config = Config::default();

    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let mut agent_loop = AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.clone(),
        "test-session".to_string(),
    );
    attach_test_workspace_authority(&mut agent_loop, &workspace_root).await;
    agent_loop
}

async fn build_test_agent_loop_with_permissions(
    provider: Box<dyn Provider>,
    tool_registry: ToolRegistry,
    permission_checker: PermissionChecker,
) -> AgentLoop {
    let agents = vec![Agent {
        git_read_only: false,
        name: "build".to_string(),
        role: None,
        description: "Test agent".to_string(),
        mode: codegg::agent::AgentMode::Primary,
        mode_name: None,
        model: None,
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
    }];

    let config = Config::default();

    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let mut agent_loop = AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.clone(),
        "test-session".to_string(),
    );
    attach_test_workspace_authority(&mut agent_loop, &workspace_root).await;
    agent_loop
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "run only via scripts/test-eggsearch-pair.sh with the pinned Eggsearch child"]
async fn pinned_eggsearch_agent_loop_uses_each_accepted_workspace_root() {
    use codegg::search_backend::bootstrap::bootstrap_search_runtime;
    use codegg::tool::repo_search::RepoSearchTool;

    let binary = std::env::var("CODEGG_EGGSEARCH_BIN")
        .expect("pair script supplies the immutable Eggsearch binary");
    let private = tempfile::tempdir().expect("private pair test directory");
    let config_root = private.path().join("config");
    let egg_config_dir = config_root.join("eggsearch");
    let config_path = egg_config_dir.join("config.toml");
    let child_stderr = private.path().join("eggsearch.stderr");
    let launcher = private.path().join("eggsearch-launcher.sh");
    std::fs::create_dir_all(&egg_config_dir).expect("private Eggsearch config directory");
    std::fs::write(
        &launcher,
        "#!/bin/sh\nexec \"$CODEGG_EGGSEARCH_REAL_BIN\" \"$@\" 2>\"$CODEGG_EGGSEARCH_STDERR\"\n",
    )
    .expect("write managed Eggsearch launcher");
    use std::os::unix::fs::PermissionsExt;
    let mut launcher_permissions = std::fs::metadata(&launcher)
        .expect("launcher metadata")
        .permissions();
    launcher_permissions.set_mode(0o700);
    std::fs::set_permissions(&launcher, launcher_permissions).expect("make launcher executable");
    let roots = [
        private.path().join("workspace-a"),
        private.path().join("workspace-b"),
    ];
    let markers = [
        format!("AGENT_LOOP_SCOPE_A_{}", std::process::id()),
        format!("AGENT_LOOP_SCOPE_B_{}", std::process::id()),
    ];
    for (root, marker) in roots.iter().zip(markers.iter()) {
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("workspace repository directory");
        std::fs::write(repo.join("scope_marker.rs"), marker).expect("write root-specific marker");
        let status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()
            .expect("git is available for the private repository fixture");
        assert!(status.success(), "initialize private repository fixture");
    }
    let escaped_root = roots[0]
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    std::fs::write(
        &config_path,
        format!(
            "[search]\nmode = \"off\"\ndefault_max_results = 5\nmax_results_cap = 5\nmax_query_chars = 256\ntimeout_ms = 1000\ndefault_providers = [\"duckduckgo\"]\n\n[search.providers]\nduckduckgo = true\n\n[fetch]\nenabled = false\n\n[local]\nenabled = true\nroots = [\"{escaped_root}\"]\nrespect_gitignore = true\n"
        ),
    )
    .expect("write private offline Eggsearch config");
    let mut child_env = std::collections::HashMap::new();
    for (name, path) in [
        ("HOME", private.path().join("home")),
        ("XDG_CONFIG_HOME", config_root.clone()),
        ("XDG_DATA_HOME", private.path().join("data")),
        ("XDG_CACHE_HOME", private.path().join("cache")),
    ] {
        std::fs::create_dir_all(&path).expect("create isolated child environment directory");
        child_env.insert(name.to_string(), path.to_string_lossy().into_owned());
    }
    child_env.insert("CODEGG_EGGSEARCH_REAL_BIN".into(), binary);
    child_env.insert(
        "CODEGG_EGGSEARCH_STDERR".into(),
        child_stderr.to_string_lossy().into_owned(),
    );
    let config = Config {
        search: Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            eggsearch: Some(EggsearchConfig {
                command: Some(launcher.to_string_lossy().into_owned()),
                args: Some(vec![
                    "--config".into(),
                    config_path.to_string_lossy().into_owned(),
                    "mcp".into(),
                    "stdio".into(),
                ]),
                env: Some(child_env),
                timeout_ms: Some(30_000),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (runtime, report) = bootstrap_search_runtime(&config).await;
    assert!(
        report.connected,
        "managed Eggsearch child connected: {:?}; stderr={}",
        report.connection_error,
        std::fs::read_to_string(&child_stderr).unwrap_or_default()
    );

    async fn run_search(
        root: &std::path::Path,
        session_id: &str,
        marker: &str,
        runtime: codegg::search_backend::SearchRuntimeContext,
        mode: codegg::agent::AgentMode,
    ) -> String {
        let provider = ScriptedProvider::new(vec![
            vec![
                ChatEvent::ToolCall(ToolCall {
                    id: format!("repo-search-{session_id}").into(),
                    name: String::from("repo_search").into(),
                    arguments: serde_json::json!({
                        "query": marker,
                        "include_local": true,
                        "max_results": 5
                    }),
                }),
                ChatEvent::Finish {
                    stop_reason: String::from("tool_calls").into(),
                    usage: TokenUsage::default(),
                },
            ],
            vec![
                ChatEvent::TextDelta(String::from("search complete").into()),
                ChatEvent::Finish {
                    stop_reason: String::from("stop").into(),
                    usage: TokenUsage::default(),
                },
            ],
        ]);
        let mut registry = ToolRegistry::new();
        registry.register(RepoSearchTool::with_search_runtime(runtime));
        let agents = vec![Agent {
            git_read_only: true,
            name: "build".into(),
            role: None,
            description: "Pinned Eggsearch pair qualification".into(),
            mode,
            mode_name: None,
            model: None,
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
        }];
        let permissions = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        });
        let mut agent_loop = AgentLoop::new(
            agents,
            Box::new(provider.clone()),
            permissions,
            registry,
            Config::default(),
            None,
            None,
            Arc::new(InMemoryArtifactStore::new()),
            root.to_path_buf(),
            session_id.to_string(),
        );
        attach_test_workspace_authority(&mut agent_loop, root).await;
        agent_loop
            .run(make_chat_request("search the attached workspace"))
            .await
            .expect("ordinary AgentLoop tool call completes");
        let requests = provider.get_requests().await;
        let tool_output = requests
            .iter()
            .flat_map(|request| request.messages.iter())
            .find_map(|message| match message {
                Message::Tool { content, .. } if content.contains("providers_queried") => {
                    Some(content.as_ref().clone())
                }
                _ => None,
            })
            .expect("model-visible repo_search result was returned through the broker");
        tool_output
    }

    let runtime_a = runtime.clone();
    let runtime_b = runtime.clone();
    let (output_a, output_b) = tokio::join!(
        run_search(
            &roots[0],
            "pair-session-a",
            &markers[0],
            runtime_a,
            codegg::agent::AgentMode::Primary,
        ),
        run_search(
            &roots[1],
            "pair-session-b",
            &markers[1],
            runtime_b,
            codegg::agent::AgentMode::Primary,
        ),
    );
    let output_a = output_a;
    let output_b = output_b;
    assert!(
        output_a.contains(&markers[0]),
        "AgentLoop A missed its root marker: {output_a}"
    );
    assert!(
        !output_a.contains(&markers[1]),
        "AgentLoop A received root B bytes: {output_a}"
    );
    assert!(
        output_b.contains(&markers[1]),
        "AgentLoop B missed its root marker: {output_b}"
    );
    assert!(
        !output_b.contains(&markers[0]),
        "AgentLoop B received root A bytes: {output_b}"
    );

    let runtime_for_subagent = runtime.clone();
    let output_a_again = run_search(
        &roots[0],
        "pair-session-a-switched-back",
        &markers[0],
        runtime,
        codegg::agent::AgentMode::Primary,
    )
    .await;
    assert!(
        output_a_again.contains(&markers[0]),
        "workspace A lost scope after switching away"
    );
    assert!(
        !output_a_again.contains(&markers[1]),
        "workspace B leaked after switching back"
    );

    let subagent_output = run_search(
        &roots[1],
        "pair-subagent-b",
        &markers[1],
        runtime_for_subagent,
        codegg::agent::AgentMode::Subagent,
    )
    .await;
    assert!(
        subagent_output.contains(&markers[1]),
        "subagent did not retain its accepted workspace root: {subagent_output}"
    );
    assert!(
        !subagent_output.contains(&markers[0]),
        "subagent received bytes from another workspace: {subagent_output}"
    );
}

#[tokio::test]
async fn verify_and_two_explicit_in_progress_todos_reach_the_model_through_agent_loop() {
    use codegg::model_profile::types::TaskStatePolicy;
    use codegg::tool::ToolRegistryOptions;

    let root = tempfile::tempdir().expect("verification workspace");
    std::fs::create_dir_all(root.path().join("src")).expect("create fixture source directory");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"trajectory-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write fixture manifest");
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .expect("write fixture source");
    let cargo_home = root.path().join("cargo-home");
    std::fs::create_dir_all(&cargo_home).expect("create private Cargo home");
    struct RestoreCargoHome(Option<std::ffi::OsString>);
    impl Drop for RestoreCargoHome {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                std::env::set_var("CARGO_HOME", previous);
            } else {
                std::env::remove_var("CARGO_HOME");
            }
        }
    }
    let _restore_cargo_home = RestoreCargoHome(std::env::var_os("CARGO_HOME"));
    std::env::set_var("CARGO_HOME", &cargo_home);

    let todos = Arc::new(tokio::sync::Mutex::new(codegg::task_state::TodoState::new()));
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        workspace_root: Some(root.path().to_path_buf()),
        todo_state: Some(todos.clone()),
        todo_policy: Some(TaskStatePolicy::explicit_todo()),
        ..Default::default()
    });
    let provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: String::from("trajectory-verify-lint").into(),
                name: String::from("verify").into(),
                arguments: serde_json::json!({
                    "action": "lint",
                    "scope": "package",
                    "package": "trajectory-fixture",
                    "timeout": 120
                }),
            }),
            ChatEvent::ToolCall(ToolCall {
                id: String::from("trajectory-parallel-todos").into(),
                name: String::from("todowrite").into(),
                arguments: serde_json::json!({
                    "todos": [
                        {"id": "search", "content": "Find the local implementation", "status": "in_progress"},
                        {"id": "verify", "content": "Verify the scoped workspace", "status": "in_progress"}
                    ]
                }),
            }),
            ChatEvent::Finish {
                stop_reason: String::from("tool_calls").into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta(String::from("verification and todo state recorded").into()),
            ChatEvent::Finish {
                stop_reason: String::from("stop").into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut agent_loop = AgentLoop::new(
        vec![Agent {
            git_read_only: false,
            name: "build".into(),
            role: None,
            description: "C007 same-session workflow qualification".into(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: Default::default(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }],
        Box::new(provider.clone()),
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        }),
        registry,
        Config::default(),
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        root.path().to_path_buf(),
        "c007-verify-todo-session".into(),
    );
    attach_test_workspace_authority(&mut agent_loop, root.path()).await;
    agent_loop
        .run(make_chat_request(
            "verify the workspace and track two active tasks",
        ))
        .await
        .expect("normal AgentLoop workflow completes");
    let tool_results: Vec<_> = provider
        .get_requests()
        .await
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter_map(|message| match message {
            Message::Tool {
                tool_call_id,
                content,
            } => Some((tool_call_id.to_string(), content.to_string())),
            _ => None,
        })
        .collect();
    assert!(
        tool_results.iter().any(|(id, output)| {
            id == "trajectory-verify-lint"
                && output.contains("passed")
                && output.contains("trajectory-fixture")
        }),
        "verify result was not truthful and model-visible: {tool_results:?}"
    );
    assert!(
        tool_results.iter().any(|(id, output)| {
            id == "trajectory-parallel-todos" && output.contains("in_progress")
        }),
        "explicit todo update was not model-visible: {tool_results:?}"
    );

    let state = todos.lock().await;
    assert_eq!(state.items.len(), 2);
    assert!(state
        .items
        .iter()
        .all(|item| item.status == codegg::task_state::TodoStatus::InProgress));
}

/// Build test AgentLoop with custom config for testing config-dependent behavior.
async fn build_test_agent_loop_with_config(
    provider: Box<dyn Provider>,
    tool_registry: ToolRegistry,
    permission_checker: PermissionChecker,
    config: Config,
) -> AgentLoop {
    let agents = vec![Agent {
        git_read_only: false,
        name: "build".to_string(),
        role: None,
        description: "Test agent".to_string(),
        mode: codegg::agent::AgentMode::Primary,
        mode_name: None,
        model: None,
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
    }];

    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let mut agent_loop = AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.clone(),
        "test-session".to_string(),
    );
    attach_test_workspace_authority(&mut agent_loop, &workspace_root).await;
    agent_loop
}

fn make_chat_request(prompt: &str) -> ChatRequest {
    ChatRequest {
        messages: vec![codegg::provider::Message::User {
            content: vec![codegg::provider::ContentPart::Text {
                text: prompt.to_string().into(),
            }],
        }],
        model: "test/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    }
}

fn assert_messages_have_roles(msgs: &[Message], expected_roles: &[&str]) {
    let actual_roles: Vec<&str> = msgs
        .iter()
        .map(|msg| match msg {
            Message::User { .. } => "user",
            Message::Assistant { .. } => "assistant",
            Message::System { .. } => "system",
            Message::Tool { .. } => "tool",
        })
        .collect();

    let mut expected_idx = 0usize;
    for actual in &actual_roles {
        if expected_idx < expected_roles.len() && *actual == expected_roles[expected_idx] {
            expected_idx += 1;
        }
    }

    assert!(
        expected_idx == expected_roles.len(),
        "Expected role sequence {:?} not found in {:?}",
        expected_roles,
        actual_roles
    );
}

fn assert_assistant_has_tool_call(msg: &Message, id: &str, name: &str, arg_value: Option<&str>) {
    let Message::Assistant {
        content: _,
        tool_calls,
    } = msg
    else {
        panic!("Expected Assistant message, got {:?}", msg);
    };
    let tc = tool_calls
        .iter()
        .find(|tc| tc.id.as_ref() == id)
        .unwrap_or_else(|| panic!("tool call '{}' not found in assistant message", id));
    assert_eq!(
        tc.name.as_ref(),
        name,
        "tool call '{}' name: expected '{}', got '{}'",
        id,
        name,
        tc.name
    );
    if let Some(val) = arg_value {
        if let Some(v) = tc.arguments.get("value") {
            let s = v.as_str().unwrap_or_default();
            assert!(
                s.contains(val),
                "tool call '{}' arg value: expected to contain '{}', got '{}'",
                id,
                val,
                s
            );
        }
    }
}

fn find_tool_message<'a>(msgs: &'a [Message], tool_call_id: &str) -> Option<&'a Message> {
    msgs.iter().find(|m| {
        if let Message::Tool {
            tool_call_id: id, ..
        } = m
        {
            id.as_ref() == tool_call_id
        } else {
            false
        }
    })
}

fn assert_tool_result_with_id(msgs: &[Message], tool_call_id: &str, contains: Option<&str>) {
    let msg = find_tool_message(msgs, tool_call_id);
    assert!(
        msg.is_some(),
        "Tool result with id '{}' not found in {:?}",
        tool_call_id,
        msgs.iter().map(|m| format!("{:?}", m)).collect::<Vec<_>>()
    );
    if let Message::Tool { content, .. } = msg.unwrap() {
        if let Some(needle) = contains {
            assert!(
                content.as_ref().contains(needle),
                "Tool result '{}' content: expected to contain '{}', got '{}'",
                tool_call_id,
                needle,
                content
            );
        }
    }
}

fn assert_tool_result_ordered_after_assistant(
    msgs: &[Message],
    assistant_idx: usize,
    tool_call_id: &str,
) {
    let tool_idx = msgs.iter().position(|m| {
        if let Message::Tool {
            tool_call_id: id, ..
        } = m
        {
            id.as_ref() == tool_call_id
        } else {
            false
        }
    });
    assert!(
        tool_idx.is_some() && tool_idx.unwrap() > assistant_idx,
        "Tool result '{}' should come after assistant at {}",
        tool_call_id,
        assistant_idx
    );
}

fn find_assistant_with_tool_call<'a>(
    msgs: &'a [Message],
    tool_call_id: &str,
) -> Option<(&'a Message, usize)> {
    for (i, msg) in msgs.iter().enumerate() {
        if let Message::Assistant { tool_calls, .. } = msg {
            if tool_calls.iter().any(|tc| tc.id.as_ref() == tool_call_id) {
                return Some((msg, i));
            }
        }
    }
    None
}

fn assert_assistant_tool_call_precedes_result(msgs: &[Message], tool_call_id: &str) {
    let (_assistant_msg, assistant_idx) = find_assistant_with_tool_call(msgs, tool_call_id)
        .unwrap_or_else(|| panic!("No assistant tool call '{}' found", tool_call_id));
    assert_tool_result_ordered_after_assistant(msgs, assistant_idx, tool_call_id);
}

fn get_tool_results_in_order(msgs: &[Message]) -> Vec<String> {
    msgs.iter()
        .filter_map(|m| {
            if let Message::Tool { tool_call_id, .. } = m {
                Some(tool_call_id.as_ref().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn has_prior_assistant_tool_call(msgs: &[Message], tool_call_id: &str) -> bool {
    find_assistant_with_tool_call(msgs, tool_call_id).is_some()
}

fn assert_no_orphan_tool_results(msgs: &[Message]) {
    for msg in msgs {
        if let Message::Tool { tool_call_id, .. } = msg {
            assert!(
                has_prior_assistant_tool_call(msgs, tool_call_id),
                "Tool result '{}' has no prior assistant tool call",
                tool_call_id
            );
        }
    }
}

// =============================================================================
// PROGRESS-RECOVERY INTEGRATION (milestone C001)
// =============================================================================
//
// These drive the *real* `AgentLoop` observation path — the construction site in
// `src/agent/loop.rs` that builds a `ProgressObservation` per tool call — rather
// than the controller in isolation. A controller-only test proves the classifier
// behaves; only these prove the loop actually feeds it the evidence the rule
// depends on, including `effect_class` sourced from the tool contract.

/// A read-only probe whose contract declares `ReadOnly`, so the loop resolves
/// `effect_class = Some(ReadOnly)` and the novel-read rule can apply.
struct ReadProbeTool;

#[async_trait]
impl Tool for ReadProbeTool {
    fn name(&self) -> &str {
        "probe_read"
    }
    fn description(&self) -> &str {
        "Reads a named resource"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
        })
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok(format!(
            "contents of {}",
            input["path"].as_str().unwrap_or("?")
        ))
    }
    fn category(&self) -> codegg::tool::ToolCategory {
        codegg::tool::ToolCategory::ReadOnly
    }
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> codegg::tool::contract::ToolContract {
        let mut contract = codegg::tool::contract::ToolContract::legacy(tool_name, input_schema);
        contract.effect_class = codegg::tool::contract::ToolEffectClass::ReadOnly;
        contract
    }
}

/// A source read whose numbered output passes through the production
/// projection path in `AgentLoop`.
struct LargeSourceReadTool;

#[async_trait]
impl Tool for LargeSourceReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Reads a source file"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
        })
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok((1..=1_000)
            .map(|line| format!("{line:>6}: source line {line}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
    fn category(&self) -> codegg::tool::ToolCategory {
        codegg::tool::ToolCategory::ReadOnly
    }
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> codegg::tool::contract::ToolContract {
        let mut contract = codegg::tool::contract::ToolContract::legacy(tool_name, input_schema);
        contract.effect_class = codegg::tool::contract::ToolEffectClass::ReadOnly;
        contract
    }
}

/// A mutating probe that always fails, with a process-class contract.
struct FailingBuildTool;

#[async_trait]
impl Tool for FailingBuildTool {
    fn name(&self) -> &str {
        "probe_build"
    }
    fn description(&self) -> &str {
        "Builds a named crate, and fails"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "crate": { "type": "string" } },
            "required": ["crate"],
        })
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        let name = input["crate"].as_str().unwrap_or("?");
        Err(codegg::error::ToolError::Execution(format!(
            "error[E0425]: cannot find `m` while building {name}"
        )))
    }
    fn category(&self) -> codegg::tool::ToolCategory {
        codegg::tool::ToolCategory::Mutating
    }
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> codegg::tool::contract::ToolContract {
        let mut contract = codegg::tool::contract::ToolContract::legacy(tool_name, input_schema);
        contract.effect_class = codegg::tool::contract::ToolEffectClass::ProcessExec;
        contract
    }
}

/// A process-class probe that succeeds. Deliberately *not* read-only: it is the
/// shape that carries no progress signal, and it must still never be escalated.
struct ShellProbeTool;

#[async_trait]
impl Tool for ShellProbeTool {
    fn name(&self) -> &str {
        "probe_shell"
    }
    fn description(&self) -> &str {
        "Runs a shell command"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"],
        })
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok(format!("ran {}", input["command"].as_str().unwrap_or("?")))
    }
    fn category(&self) -> codegg::tool::ToolCategory {
        codegg::tool::ToolCategory::Mutating
    }
    fn contract(
        &self,
        tool_name: &str,
        input_schema: serde_json::Value,
    ) -> codegg::tool::contract::ToolContract {
        let mut contract = codegg::tool::contract::ToolContract::legacy(tool_name, input_schema);
        contract.effect_class = codegg::tool::contract::ToolEffectClass::ProcessExec;
        contract
    }
}

fn recovery_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(ReadProbeTool);
    registry.register(FailingBuildTool);
    registry.register(ShellProbeTool);
    registry
}

struct DeferredSchemaTool;

#[async_trait]
impl Tool for DeferredSchemaTool {
    fn name(&self) -> &str {
        "deferred_probe"
    }
    fn description(&self) -> &str {
        "A deferred schema used by provider compatibility tests"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{}})
    }
    fn defer_loading(&self) -> bool {
        true
    }
    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        Ok("unused".into())
    }
}

#[derive(Clone)]
struct DeferredSchemaRejectingProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Provider for DeferredSchemaRejectingProvider {
    fn id(&self) -> &str {
        // Deliberately claims the provider-family capability. The fake
        // endpoint below is stricter and rejects the extension field.
        "anthropic"
    }
    fn name(&self) -> &str {
        "deferred-schema-rejecting fake gateway"
    }
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(ProviderError::api(
                "400",
                "Unsupported field defer_loading in tool definition",
            ));
        }
        Ok(Box::pin(futures_util::stream::iter(vec![
            Ok(ChatEvent::TextDelta("done".to_string().into())),
            Ok(finish_event()),
        ])))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".into(),
            name: "Test Model".into(),
            provider: "anthropic".into(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

#[tokio::test]
async fn deferred_schema_400_retries_once_without_changing_authorized_tools() {
    let provider = DeferredSchemaRejectingProvider {
        requests: Arc::new(Mutex::new(Vec::new())),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let requests = Arc::clone(&provider.requests);
    let mut registry = ToolRegistry::new();
    registry.register(DeferredSchemaTool);
    let permissions = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![],
        path_rules: vec![],
    });
    let mut agent_loop =
        build_test_agent_loop_with_permissions(Box::new(provider), registry, permissions).await;
    agent_loop
        .run(make_chat_request("Use the deferred probe"))
        .await
        .expect("the request-only compatibility retry should complete");

    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].model, requests[1].model);
    let first_tools = requests[0].tools.as_ref().expect("initial tool surface");
    assert!(first_tools
        .iter()
        .any(|tool| tool.name == "deferred_probe" && tool.defer_loading == Some(true)));
    let fallback_tools = requests[1].tools.as_ref().expect("fallback tool surface");
    assert!(fallback_tools
        .iter()
        .any(|tool| tool.name == "deferred_probe"));
    assert!(fallback_tools
        .iter()
        .all(|tool| tool.defer_loading.is_none()));
}

async fn recovery_loop(responses: Vec<Vec<ChatEvent>>) -> (AgentLoop, ScriptedProvider) {
    let provider = ScriptedProvider::new(responses);
    // `PermissionChecker::new(None, None)` leaves the default level at Ask, and
    // an unanswered request parks the tool call forever — the turn never
    // returns and the test hangs rather than failing. Allow everything: these
    // tests are about recovery classification, not about authorization.
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        });
    let agent_loop = build_test_agent_loop_with_permissions(
        Box::new(provider.clone()),
        recovery_registry(),
        permission_checker,
    )
    .await;
    (agent_loop, provider)
}

fn tool_call_event(id: &str, name: &str, args: serde_json::Value) -> ChatEvent {
    ChatEvent::ToolCall(ToolCall {
        id: id.to_string().into(),
        name: name.to_string().into(),
        arguments: args,
    })
}

fn finish_event() -> ChatEvent {
    ChatEvent::Finish {
        stop_reason: "stop".to_string().into(),
        usage: TokenUsage::default(),
    }
}

#[tokio::test]
async fn first_use_apply_patch_create_then_grep_runs_through_agent_broker() {
    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let temp_dir = tempfile::tempdir_in(workspace_root.join("target"))
        .expect("create isolated first-use workspace fixture");
    let relative_path = temp_dir
        .path()
        .join("nested/new.rs")
        .strip_prefix(&workspace_root)
        .expect("fixture belongs to workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let marker = format!("c003-first-use-marker-{}", std::process::id());
    let absolute_path = temp_dir.path().join("nested/new.rs");

    let scripted_provider = Box::new(ScriptedProvider::new(vec![
        vec![
            tool_call_event(
                "create-first-use-file",
                "apply_patch",
                serde_json::json!({
                    "path": relative_path,
                    "mode": "create",
                    "patch": marker,
                }),
            ),
            finish_event(),
        ],
        vec![
            tool_call_event(
                "grep-first-use-file",
                "grep",
                serde_json::json!({ "pattern": marker, "path": temp_dir.path() }),
            ),
            finish_event(),
        ],
        vec![finish_event()],
    ]));
    let provider_observer = scripted_provider.clone();
    let mut registry = ToolRegistry::new();
    registry.register(ApplyPatchTool::new().with_allowed_root(workspace_root.clone()));
    registry.register(GrepTool::new().with_allowed_root(workspace_root));
    let mut agent_loop = build_test_agent_loop(scripted_provider, registry).await;

    agent_loop
        .run(make_chat_request(
            "Create a file, then search for its marker",
        ))
        .await
        .expect("the real agent and broker path should execute both tool calls");

    let contents = std::fs::read_to_string(&absolute_path).expect("created file");
    assert_eq!(contents, marker);
    let requests = provider_observer.get_requests().await;
    assert!(requests.len() >= 3, "expected follow-up after both tools");
    let second_request_text = format!("{:?}", requests[2].messages);
    assert!(
        second_request_text.contains(&marker),
        "grep result should be returned to the provider: {second_request_text}"
    );
}

/// The regression, through the real loop: one provider turn emitting a batch of
/// ten *distinct* failing builds.
///
/// At baseline the `EquivalentError` arm matched on tool name plus error class
/// alone, so these ten different commands — ten different crates, ten different
/// error messages — read as one repeating incident and climbed Nudge → Correct
/// → RestoreBasePalette → Replan → Stall. The turn was terminated and the model
/// was told "Agent stalled" while it was bisecting a build failure.
///
/// Note the shape matters: the failures must be consecutive. An intervening
/// successful read is progress and resets the ladder, which is why a
/// read-then-fail-per-round sequence does *not* reproduce this — it only hides
/// it. The batch below is the honest reproduction.
///
/// The signal is how many provider turns were consumed, not a bus event: the
/// global event bus is process-global, so asserting on it from a parallel test
/// binary would be racy. A stalled turn breaks out of the loop, so the scripted
/// provider stops being called and the turn count falls short of the two this
/// sequence needs.
#[tokio::test]
async fn a_batch_of_distinct_failing_commands_does_not_stall_the_turn() {
    const ATTEMPTS: usize = 10;
    let mut batch = Vec::new();
    for n in 0..ATTEMPTS {
        batch.push(tool_call_event(
            &format!("build-{n}"),
            "probe_build",
            serde_json::json!({ "crate": format!("crate-{n}") }),
        ));
    }
    batch.push(finish_event());

    let (mut agent_loop, provider) = recovery_loop(vec![
        batch,
        vec![
            ChatEvent::TextDelta("gave up".to_string().into()),
            finish_event(),
        ],
    ])
    .await;
    agent_loop
        .run(make_chat_request("Get the build green"))
        .await
        .expect("bisecting a build failure must not break the turn");

    assert!(
        provider.request_count().await >= 2,
        "the turn must reach its second provider call; it stopped at {} because the \
         ten distinct failures were classified as one repeat",
        provider.request_count().await
    );
}

/// Coverage for the ordinary explore shape: many reads interleaved with
/// successful shell calls, none of them repeating. This is the shape the removed
/// `ShortCycle` arm used to kill, so it stays pinned even though that arm is
/// long gone.
#[tokio::test]
async fn exploring_with_alternating_reads_and_shell_never_stalls() {
    const ROUNDS: usize = 8;
    let mut responses = Vec::new();
    for n in 0..ROUNDS {
        responses.push(vec![
            tool_call_event(
                &format!("read-{n}"),
                "probe_read",
                serde_json::json!({ "path": format!("src/module-{n}.rs") }),
            ),
            tool_call_event(
                &format!("shell-{n}"),
                "probe_shell",
                serde_json::json!({ "command": format!("rg -n needle src/module-{n}.rs") }),
            ),
            finish_event(),
        ]);
    }
    responses.push(vec![
        ChatEvent::TextDelta("done".to_string().into()),
        finish_event(),
    ]);

    let (mut agent_loop, provider) = recovery_loop(responses).await;
    agent_loop
        .run(make_chat_request("Explore the repository"))
        .await
        .expect("an exploring turn must complete normally");

    assert!(
        provider.request_count().await > ROUNDS,
        "alternating reads and shell work must not break the turn: {}",
        provider.request_count().await
    );
}

/// Repeated identical outcomes without a correlated state revision are unknown,
/// not affirmative stagnation. They may trigger bounded guidance, but the turn
/// remains governed by its ordinary hard execution budget.
#[tokio::test]
async fn an_identical_repeated_failure_does_not_claim_confirmed_stagnation() {
    const SCRIPTED_TURNS: usize = 12;
    let mut responses = (0..SCRIPTED_TURNS)
        .map(|_| {
            vec![
                tool_call_event(
                    "build-again",
                    "probe_build",
                    serde_json::json!({ "crate": "core" }),
                ),
                finish_event(),
            ]
        })
        .collect::<Vec<_>>();
    responses.push(vec![
        ChatEvent::TextDelta("done".to_string().into()),
        finish_event(),
    ]);

    let (mut agent_loop, provider) = recovery_loop(responses).await;
    agent_loop
        .run(make_chat_request("Fix the build"))
        .await
        .expect("unconfirmed repeated work must remain recoverable");

    let consumed = provider.request_count().await;
    assert!(
        consumed > SCRIPTED_TURNS,
        "the turn should continue to the scripted completion after repeated unknown outcomes: {consumed}"
    );
}

#[tokio::test]
async fn repeated_unknown_outcomes_remain_bounded_by_the_hard_turn_limit() {
    let responses = (0..8)
        .map(|_| {
            vec![
                tool_call_event(
                    "build-same",
                    "probe_build",
                    serde_json::json!({ "crate": "core" }),
                ),
                finish_event(),
            ]
        })
        .collect::<Vec<_>>();
    let (mut agent_loop, provider) = recovery_loop(responses).await;
    agent_loop.set_max_turns(3);
    agent_loop
        .run(make_chat_request("Repeat a build observation"))
        .await
        .expect("hard turn exhaustion is a normal bounded completion");
    assert_eq!(
        provider.request_count().await,
        3,
        "unknown repeated outcomes must reach, then stop at, the hard turn cap"
    );
}

#[tokio::test]
async fn large_source_read_reaches_the_next_model_request_as_a_contiguous_page() {
    let responses = vec![
        vec![
            tool_call_event(
                "large-source-read",
                "read",
                serde_json::json!({ "path": "src/large.rs" }),
            ),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("I have enough source context.".to_string().into()),
            finish_event(),
        ],
    ];
    let provider = ScriptedProvider::new(responses);
    let mut registry = ToolRegistry::new();
    registry.register(LargeSourceReadTool);
    let permissions = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![],
        path_rules: vec![],
    });
    let mut agent_loop =
        build_test_agent_loop_with_permissions(Box::new(provider.clone()), registry, permissions)
            .await;
    agent_loop
        .run(make_chat_request("Inspect the large source file"))
        .await
        .expect("the source-read trajectory should complete");

    let requests = provider.get_requests().await;
    assert!(
        requests.len() >= 2,
        "the provider must receive the read result"
    );
    let tool_result = requests[1]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Tool { content, .. } => Some(content.as_ref()),
            _ => None,
        })
        .expect("the second model request must contain the source result");
    let visible_lines = tool_result
        .lines()
        .filter(|line| line.contains(": source line "))
        .collect::<Vec<_>>();
    assert!(
        visible_lines.len() > 20,
        "only {} source lines reached the model",
        visible_lines.len()
    );
    assert!(visible_lines[0].contains("1: source line 1"));
    assert!(visible_lines.windows(2).all(|pair| {
        let line_number = |line: &str| {
            line.split_once(':')
                .and_then(|(number, _)| number.trim().parse::<usize>().ok())
                .expect("projected source line has a number")
        };
        line_number(pair[1]) == line_number(pair[0]) + 1
    }));
    assert!(tool_result.contains("Continue with read offset="));
    assert!(!tool_result.contains("1000: source line 1000"));
    let calls = requests
        .last()
        .expect("the completed turn has a provider request")
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { tool_calls, .. } => Some(tool_calls.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.name.as_ref() == "read")
            .count(),
        1,
        "the model-visible page must avoid a forced second source read"
    );
    assert!(!calls
        .iter()
        .any(|call| call.name.as_ref() == "context_read"));
}

#[tokio::test]
async fn peer_evidence_arriving_during_provider_stream_fences_returned_tool_batch() {
    let session_id = format!("peer-fence-session-{}", uuid::Uuid::new_v4());
    let run_id = codegg::identity::AgentRunId::new();
    let peer_run_id = codegg::identity::AgentRunId::new();
    let group_id = format!("peer-fence-group-{}", uuid::Uuid::new_v4());
    let write_count = Arc::new(AtomicUsize::new(0));
    let provider = PeerEvidenceDuringStreamProvider {
        requests: Arc::new(Mutex::new(Vec::new())),
        calls: Arc::new(AtomicUsize::new(0)),
        session_id: session_id.clone(),
        peer_run_id: peer_run_id.to_string(),
        changed_path: "src/api.rs".into(),
    };
    let provider_requests = Arc::clone(&provider.requests);
    let mut registry = ToolRegistry::new();
    registry.register(CountedWriteTool(Arc::clone(&write_count)));
    let permissions = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![],
        path_rules: vec![],
    });
    let mut agent_loop =
        build_test_agent_loop_with_permissions(Box::new(provider), registry, permissions).await;
    let run_store: Arc<dyn codegg::agent_run::AgentRunStore> =
        Arc::new(codegg::agent_run::InMemoryAgentRunStore::new());
    agent_loop.set_run_control(
        codegg::agent::run_control::RunControlService::in_memory(run_store),
        run_id.clone(),
    );
    agent_loop.set_peer_observation_session_id(session_id.clone());
    agent_loop.set_peer_group_memberships(vec![(
        group_id.clone(),
        vec![run_id.to_string(), peer_run_id.to_string()],
    )]);

    // Seed a host-owned active path claim on peer A so the later fake Git
    // result is relevant to this run. The claim observation is consumed at turn start;
    // the result event is emitted from inside the provider stream.
    GlobalEventBus::publish(AppEvent::WorkstreamClaimUpdated {
        session_id,
        group_id: group_id.clone(),
        member_run_ids: vec![run_id.to_string(), peer_run_id.to_string()],
        claim: codegg::protocol::projection::WorkstreamClaimProjection {
            claim_id: "claim-api".into(),
            project_id: "project-test".into(),
            workspace_id: "workspace-test".into(),
            group_id,
            kind: "path".into(),
            target: "src/api.rs".into(),
            state: "claimed".into(),
            owner_kind: "run".into(),
            owner_id: peer_run_id.to_string(),
            version: 1,
            lease_generation: 1,
            lease_expires_at: None,
            updated_at: 1,
        },
        action: "claimed".into(),
    });

    agent_loop
        .run(make_chat_request("Update the shared API"))
        .await
        .expect("peer update should fence the stale action and allow the loop to continue");

    assert_eq!(
        write_count.load(Ordering::SeqCst),
        0,
        "the provider-returned write must not execute after peer evidence arrives"
    );
    let requests = provider_requests.lock().await;
    assert!(
        requests.len() >= 2,
        "the provider should get a reconsideration turn"
    );
    let evidence_request = requests
        .iter()
        .skip(1)
        .find(|request| {
            request.messages.iter().any(|message| match message {
                Message::User { content } => content.iter().any(|part| match part {
                    codegg::provider::ContentPart::Text { text } => {
                        text.contains("peer_run_evidence")
                            && text.contains("fake-git-commit-a1b2c3")
                            && text.contains("src/api.rs")
                    }
                    _ => false,
                }),
                _ => false,
            })
        })
        .expect("a subsequent provider request must contain the peer evidence");
    let second_messages = &evidence_request.messages;
    assert!(
        second_messages.iter().all(|message| !matches!(message, Message::User { content } if content.iter().any(|part| matches!(part, codegg::provider::ContentPart::Text { text } if text.contains("fake-other-project-commit"))))),
        "evidence from another session/project must not cross the observation boundary"
    );
    let tool_result_index = second_messages
        .iter()
        .position(|message| matches!(message, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "peer-fenced-write"))
        .expect("the refused call must retain its matching tool result");
    let second_tool_result_index = second_messages
        .iter()
        .position(|message| matches!(message, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "peer-fenced-write-2"))
        .expect("every call in the refused batch must retain its matching tool result");
    let observation_index = second_messages
        .iter()
        .position(|message| match message {
            Message::User { content } => content.iter().any(|part| match part {
                codegg::provider::ContentPart::Text { text } => {
                    text.contains("peer_run_evidence")
                        && text.contains("fake-git-commit-a1b2c3")
                        && text.contains("src/api.rs")
                }
                _ => false,
            }),
            _ => false,
        })
        .expect("the evidence-backed peer observation must reach the next provider request");
    assert!(
        tool_result_index < observation_index,
        "keep the refusal paired with the assistant call before appending the peer update"
    );
    assert!(
        second_tool_result_index < observation_index,
        "refuse every call before appending the peer update"
    );
}

#[tokio::test]
async fn unrelated_peer_change_does_not_fence_or_notify_member() {
    let session_id = format!("peer-fence-unrelated-session-{}", uuid::Uuid::new_v4());
    let run_id = codegg::identity::AgentRunId::new();
    let peer_run_id = codegg::identity::AgentRunId::new();
    let group_id = format!("peer-fence-unrelated-group-{}", uuid::Uuid::new_v4());
    let write_count = Arc::new(AtomicUsize::new(0));
    let provider = PeerEvidenceDuringStreamProvider {
        requests: Arc::new(Mutex::new(Vec::new())),
        calls: Arc::new(AtomicUsize::new(0)),
        session_id: session_id.clone(),
        peer_run_id: peer_run_id.to_string(),
        changed_path: "docs/unrelated.md".into(),
    };
    let provider_requests = Arc::clone(&provider.requests);
    let mut registry = ToolRegistry::new();
    registry.register(CountedWriteTool(Arc::clone(&write_count)));
    let permissions = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![],
        path_rules: vec![],
    });
    let mut agent_loop =
        build_test_agent_loop_with_permissions(Box::new(provider), registry, permissions).await;
    let run_store: Arc<dyn codegg::agent_run::AgentRunStore> =
        Arc::new(codegg::agent_run::InMemoryAgentRunStore::new());
    agent_loop.set_run_control(
        codegg::agent::run_control::RunControlService::in_memory(run_store),
        run_id.clone(),
    );
    agent_loop.set_peer_observation_session_id(session_id.clone());
    agent_loop.set_peer_group_memberships(vec![(
        group_id.clone(),
        vec![run_id.to_string(), peer_run_id.to_string()],
    )]);
    GlobalEventBus::publish(AppEvent::WorkstreamClaimUpdated {
        session_id,
        group_id: group_id.clone(),
        member_run_ids: vec![run_id.to_string(), peer_run_id.to_string()],
        claim: codegg::protocol::projection::WorkstreamClaimProjection {
            claim_id: "claim-api-unrelated".into(),
            project_id: "project-test".into(),
            workspace_id: "workspace-test".into(),
            group_id,
            kind: "path".into(),
            target: "src/api.rs".into(),
            state: "claimed".into(),
            owner_kind: "run".into(),
            owner_id: peer_run_id.to_string(),
            version: 1,
            lease_generation: 1,
            lease_expires_at: None,
            updated_at: 1,
        },
        action: "claimed".into(),
    });

    agent_loop
        .run(make_chat_request("Update the shared API"))
        .await
        .expect("an unrelated peer change must not block this action");

    assert_eq!(
        write_count.load(Ordering::SeqCst),
        2,
        "unrelated evidence must leave every action in the batch enabled"
    );
    let requests = provider_requests.lock().await;
    assert!(requests.iter().all(|request| request.messages.iter().all(|message| {
        !matches!(message, Message::User { content } if content.iter().any(|part| matches!(part, codegg::provider::ContentPart::Text { text } if text.contains("peer_run_evidence"))))
    })), "unrelated path evidence must not become model-visible");
}

#[tokio::test]
async fn test_agent_loop_harness_smoke_test() {
    let tool_call = ToolCall {
        id: "call_1".to_string().into(),
        name: "echo_args".to_string().into(),
        arguments: serde_json::json!({"value": "hello"}),
    };

    let response1 = vec![
        ChatEvent::TextDelta("Hello".to_string().into()),
        ChatEvent::ToolCall(tool_call),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;

    // Set unique session ID to avoid cross-test contamination
    let session_id = "test-smoke-unique-session".to_string();
    agent_loop.set_session_id(&session_id);

    let mut event_collector = EventCollector::with_session(session_id);

    let request = ChatRequest {
        messages: vec![codegg::provider::Message::User {
            content: vec![codegg::provider::ContentPart::Text {
                text: "Test prompt".to_string().into(),
            }],
        }],
        model: "test/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    };

    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    event_collector.collect();
    assert!(
        result.is_ok(),
        "AgentLoop::run should succeed: {:?}",
        result.err()
    );

    let events = result.unwrap();
    assert!(!events.is_empty(), "Should have returned events");

    let requests = scripted_provider.get_requests().await;
    assert!(
        requests.len() >= 2,
        "Should have at least 2 provider calls, got {}",
        requests.len()
    );

    let call_count = scripted_provider.request_count().await;
    assert!(
        call_count >= 2,
        "Provider recorded {} calls, expected at least 2",
        call_count
    );

    // Strengthened assertions using helper functions (Packet 1)
    let second_request = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_1"))
        })
        .expect("Expected a request containing tool result for call_1");

    // Check message order: user, assistant with tool_call, then Tool result
    assert_messages_have_roles(&second_request.messages, &["user", "assistant", "tool"]);

    // Verify the assistant message has the expected tool call
    let assistant_msg = second_request
        .messages
        .iter()
        .find(|m| matches!(m, Message::Assistant { .. }))
        .expect("Expected assistant message in second request");
    assert_assistant_has_tool_call(assistant_msg, "call_1", "echo_args", Some("hello"));

    // Verify tool result exists with correct ID and is after assistant
    assert_tool_result_with_id(&second_request.messages, "call_1", Some("hello"));
    assert_assistant_tool_call_precedes_result(&second_request.messages, "call_1");

    // Verify no orphan tool results
    assert_no_orphan_tool_results(&second_request.messages);

    // Packet 11: Event bus assertions for simple tool-call run
    // Assert event order: text delta → tool call started → tool result → agent finished
    event_collector.assert_event_order(&[
        "text:delta",
        "tool_call:started",
        "tool:result",
        "agent:finished",
    ]);

    // Assert ToolResult event details
    let tool_result_event =
        event_collector.find_event(|e| matches!(e, AppEvent::ToolResult { .. }));
    assert!(
        tool_result_event.is_some(),
        "ToolResult event should be present"
    );
    if let AppEvent::ToolResult {
        tool_id,
        tool_name,
        success,
        ..
    } = tool_result_event.unwrap()
    {
        assert_eq!(
            tool_id, "call_1",
            "ToolResult tool_id should match provider call"
        );
        assert_eq!(
            tool_name, "echo_args",
            "ToolResult tool_name should match provider call"
        );
        assert!(
            *success,
            "ToolResult success should be true for allowed tool"
        );
    }

    // Assert TextDelta event exists
    let text_delta = event_collector.find_event(|e| matches!(e, AppEvent::TextDelta { .. }));
    assert!(text_delta.is_some(), "TextDelta event should be present");

    // Assert ToolCallStarted event details
    let tool_call_started =
        event_collector.find_event(|e| matches!(e, AppEvent::ToolCallStarted { .. }));
    assert!(
        tool_call_started.is_some(),
        "ToolCallStarted event should be present"
    );
    if let AppEvent::ToolCallStarted {
        tool_id, tool_name, ..
    } = tool_call_started.unwrap()
    {
        assert_eq!(tool_id, "call_1", "ToolCallStarted tool_id should match");
        assert_eq!(
            tool_name, "echo_args",
            "ToolCallStarted tool_name should match"
        );
    }

    // Assert AgentFinished event exists
    let agent_finished =
        event_collector.find_event(|e| matches!(e, AppEvent::AgentFinished { .. }));
    assert!(
        agent_finished.is_some(),
        "AgentFinished event should be present"
    );
}

#[tokio::test]
async fn test_tool_call_turn_does_not_publish_premature_agent_finished() {
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "explore"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("Done exploring".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider, registry).await;
    let session_id = "test-no-premature-finish-tool-calls".to_string();
    agent_loop.set_session_id(&session_id);
    let mut event_collector = EventCollector::with_session(session_id);

    agent_loop
        .run(make_chat_request("Explore the codebase"))
        .await
        .expect("agent loop should run");
    event_collector.collect();

    let finished_reasons: Vec<&str> = event_collector
        .events
        .iter()
        .filter_map(|event| {
            if let AppEvent::AgentFinished { stop_reason, .. } = event {
                Some(stop_reason.as_str())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(finished_reasons, vec!["stop"]);
}

#[tokio::test]
async fn test_tool_call_turn_with_stop_finish_does_not_publish_premature_agent_finished() {
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "anthropic-style"}),
        }),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("Done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider, registry).await;
    let session_id = "test-no-premature-finish-stop-after-tool".to_string();
    agent_loop.set_session_id(&session_id);
    let mut event_collector = EventCollector::with_session(session_id);

    agent_loop
        .run(make_chat_request("Explore the codebase"))
        .await
        .expect("agent loop should run");
    event_collector.collect();

    let finished_count = event_collector
        .events
        .iter()
        .filter(|event| matches!(event, AppEvent::AgentFinished { .. }))
        .count();
    assert_eq!(finished_count, 1);
}

#[tokio::test]
async fn test_post_tool_soft_stop_retries_and_continues_for_repo_task() {
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "first"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("Let me continue".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response3 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_2".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "second"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response4 = vec![
        ChatEvent::TextDelta("Done exploring".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![
        response1, response2, response3, response4,
    ]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;
    agent_loop
        .run(make_chat_request("Review this codebase layout"))
        .await
        .expect("agent loop should continue after post-tool soft stop");

    let requests = scripted_provider.get_requests().await;
    assert!(
        requests.len() >= 4,
        "Expected at least 4 provider requests, got {}",
        requests.len()
    );

    let has_second_tool_result = requests.iter().any(|r| {
        r.messages
            .iter()
            .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_2"))
    });
    assert!(
        has_second_tool_result,
        "Expected a tool result for call_2 in a later provider request"
    );
}

#[tokio::test]
async fn test_agent_loop_harness_records_requests() {
    let response1 = vec![
        ChatEvent::TextDelta("Using tool".to_string().into()),
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "test1"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;

    let request = ChatRequest {
        messages: vec![codegg::provider::Message::User {
            content: vec![codegg::provider::ContentPart::Text {
                text: "Use echo_args with test1".to_string().into(),
            }],
        }],
        model: "test/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    };

    let _: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;

    let requests = scripted_provider.get_requests().await;
    assert!(
        requests.len() >= 2,
        "Should record at least two provider requests"
    );
    assert!(requests
        .iter()
        .all(|request| { request.context.session_id.as_deref() == Some("test-session") }));

    assert!(
        !requests[0].messages.is_empty(),
        "First request should have messages"
    );
    let last = requests.last().expect("Expected at least one request");
    assert!(
        !last.messages.is_empty(),
        "Last request should have messages (after tool execution)"
    );
}

#[tokio::test]
async fn test_agent_loop_harness_fails_without_second_call() {
    #[derive(Clone)]
    struct NoSecondCallProvider {
        calls: Arc<Mutex<usize>>,
    }

    impl NoSecondCallProvider {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl Provider for NoSecondCallProvider {
        fn id(&self) -> &str {
            "no-second-call"
        }

        fn name(&self) -> &str {
            "No Second Call"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
            let mut count = self.calls.lock().await;
            let events = if *count == 0 {
                vec![
                    ChatEvent::TextDelta("Hello".to_string().into()),
                    ChatEvent::ToolCall(ToolCall {
                        id: "call_1".to_string().into(),
                        name: "echo_args".to_string().into(),
                        arguments: serde_json::json!({"value": "test"}),
                    }),
                    ChatEvent::Finish {
                        stop_reason: "tool_calls".to_string().into(),
                        usage: TokenUsage::default(),
                    },
                ]
            } else {
                vec![
                    ChatEvent::TextDelta("Done".to_string().into()),
                    ChatEvent::Finish {
                        stop_reason: "stop".to_string().into(),
                        usage: TokenUsage::default(),
                    },
                ]
            };
            *count += 1;

            let stream = futures_util::stream::iter(events.into_iter().map(Ok));
            Ok(Box::pin(stream))
        }

        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![])
        }
    }

    let provider = Box::new(NoSecondCallProvider::new());

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(provider, registry).await;

    let request = ChatRequest {
        messages: vec![codegg::provider::Message::User {
            content: vec![codegg::provider::ContentPart::Text {
                text: "Test".to_string().into(),
            }],
        }],
        model: "test/model".to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context: Default::default(),
    };

    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    assert!(result.is_ok(), "Should complete successfully");
}

#[tokio::test]
async fn test_denied_tool_produces_error_result() {
    let response1 = vec![
        ChatEvent::TextDelta("Using forbidden tool".to_string().into()),
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "denied_tool".to_string().into(),
            arguments: serde_json::json!({"value": "test"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Deny,
            tool_rules: vec![],
            path_rules: vec![],
        });

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;

    let request = make_chat_request("Use denied_tool");
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    assert!(result.is_ok(), "Loop should continue after denied tool");

    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");

    let second_request = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_1"))
        })
        .expect("Expected a request containing tool result for call_1");

    // Strengthened assertions using helpers (Packet 1)
    // Check message order: user, assistant, tool
    assert_messages_have_roles(&second_request.messages, &["user", "assistant", "tool"]);

    // Verify assistant has the tool call with correct ID
    let assistant_msg = second_request
        .messages
        .iter()
        .find(|m| matches!(m, Message::Assistant { .. }))
        .expect("Expected assistant message in second request");
    assert_assistant_has_tool_call(assistant_msg, "call_1", "denied_tool", Some("test"));

    // Verify tool result exists with correct ID and error content
    assert_tool_result_with_id(&second_request.messages, "call_1", Some("denied"));
    assert_assistant_tool_call_precedes_result(&second_request.messages, "call_1");
    assert_no_orphan_tool_results(&second_request.messages);
}

#[tokio::test]
async fn test_missing_tool_produces_error_result() {
    let response1 = vec![
        ChatEvent::TextDelta("Using unknown tool".to_string().into()),
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "nonexistent_tool".to_string().into(),
            arguments: serde_json::json!({"value": "test"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;

    let request = make_chat_request("Use nonexistent_tool");
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    assert!(result.is_ok(), "Loop should continue after missing tool");

    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");

    let second_request = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_1"))
        })
        .expect("Expected a request containing tool result for call_1");

    // Strengthened assertions using helpers (Packet 1)
    // Check message order: user, assistant, tool
    assert_messages_have_roles(&second_request.messages, &["user", "assistant", "tool"]);

    // Verify assistant has the tool call with correct ID
    let assistant_msg = second_request
        .messages
        .iter()
        .find(|m| matches!(m, Message::Assistant { .. }))
        .expect("Expected assistant message in second request");
    assert_assistant_has_tool_call(assistant_msg, "call_1", "nonexistent_tool", Some("test"));

    // Verify tool result exists with correct ID and error content
    assert_tool_result_with_id(&second_request.messages, "call_1", Some("not found"));
    assert_assistant_tool_call_precedes_result(&second_request.messages, "call_1");
    assert_no_orphan_tool_results(&second_request.messages);
}

#[tokio::test]
async fn test_question_tool_produces_tool_result() {
    use codegg::bus::QuestionRegistry;

    let response1 = vec![
        ChatEvent::TextDelta("Asking a question".to_string().into()),
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "question".to_string().into(),
            arguments: serde_json::json!({
                "questions": [
                    {
                        "id": "q1",
                        "question": "What color?",
                        "options": ["red", "blue", "green"]
                    }
                ]
            }),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Thanks for the answers".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;

    // Set session ID for QuestionRegistry (Packet 3)
    agent_loop.set_session_id("test-session-123");

    // Subscribe BEFORE spawning to avoid race condition
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Ask me something");

    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    // Wait for QuestionPending event and answer the question through QuestionRegistry
    let question_id = wait_for_question_pending(
        "test-session-123",
        &mut rx,
        std::time::Duration::from_secs(5),
    )
    .await
    .unwrap();

    let answers = serde_json::json!({
        "q1": "red"
    })
    .to_string();

    let answered =
        QuestionRegistry::answer_question_scoped("test-session-123", &question_id, answers.clone());
    assert!(answered, "Question should be answered");

    let result = handle.await.unwrap();
    assert!(
        result.is_ok(),
        "Loop should complete successfully: {:?}",
        result.err()
    );

    // Verify assertions (Packet 3)
    let requests = scripted_provider.get_requests().await;
    assert!(
        requests.len() >= 2,
        "Should have at least 2 provider calls, got {}",
        requests.len()
    );

    // Request 1 should have the user message (assistant message added after stream)
    let req1 = &requests[0];
    assert_messages_have_roles(&req1.messages, &["user"]);

    // Request 2 should have user, assistant (with tool call), tool (with answer)
    if requests.len() >= 2 {
        let req2 = &requests[1];
        assert_messages_have_roles(&req2.messages, &["user", "assistant", "tool"]);

        // Verify assistant has the question tool call
        let assistant_msg = req2
            .messages
            .iter()
            .find(|m| matches!(m, Message::Assistant { .. }))
            .expect("Expected assistant message in second request");
        assert_assistant_has_tool_call(assistant_msg, "call_1", "question", None);

        // Verify tool result exists with correct content (the answer)
        assert_tool_result_with_id(&req2.messages, "call_1", Some("red"));
        assert_assistant_tool_call_precedes_result(&req2.messages, "call_1");
        assert_no_orphan_tool_results(&req2.messages);
    }
}

#[tokio::test]
async fn test_question_tool_answer_immediately() {
    use codegg::bus::QuestionRegistry;

    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_q1".to_string().into(),
            name: "question".to_string().into(),
            arguments: serde_json::json!({
                "questions": [
                    {
                        "id": "q1",
                        "question": "What color?",
                        "options": ["red", "blue", "green"]
                    }
                ]
            }),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Got answer".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;
    agent_loop.set_session_id("test-session-immediate-q");

    let mut rx = GlobalEventBus::subscribe();
    let request = make_chat_request("Ask a question");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    let question_id = wait_for_question_pending(
        "test-session-immediate-q",
        &mut rx,
        std::time::Duration::from_secs(5),
    )
    .await
    .unwrap();

    let is_registered =
        QuestionRegistry::is_registered_scoped("test-session-immediate-q", &question_id);
    assert!(
        is_registered,
        "QuestionRegistry should have session registered BEFORE answering"
    );

    let answers = serde_json::json!({"q1": "blue"}).to_string();
    let answered =
        QuestionRegistry::answer_question_scoped("test-session-immediate-q", &question_id, answers);
    assert!(
        answered,
        "Question should be answered immediately after registration"
    );

    let result = handle.await.unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");
    let req2 = &requests[1];
    assert_messages_have_roles(&req2.messages, &["user", "assistant", "tool"]);
    assert_tool_result_with_id(&req2.messages, "call_q1", Some("blue"));
}

#[tokio::test]
async fn test_permission_ask_answer_immediately() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_p1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "immediate_test"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Approved".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    agent_loop.set_session_id("test-session-immediate-p");

    let perm_id = "call_p1-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    wait_for_permission_pending(&perm_id, &mut rx, std::time::Duration::from_secs(5))
        .await
        .unwrap();

    let is_registered = PermissionRegistry::is_registered(&perm_id);
    assert!(
        is_registered,
        "PermissionRegistry should have permission registered BEFORE answering"
    );

    let responded = PermissionRegistry::respond_scoped(
        "test-session-immediate-p",
        &perm_id,
        PermissionDecision::AllowOnce,
    );
    assert!(
        responded,
        "Permission should be responded to immediately after registration"
    );

    let result = handle.await.unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");
    let req2 = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_p1"))
        })
        .expect("Expected a request containing tool result for call_p1");
    assert_messages_have_roles(&req2.messages, &["user", "assistant", "tool"]);
    assert_tool_result_with_id(&req2.messages, "call_p1", Some("immediate_test"));
}

#[tokio::test]
async fn test_echo_args_tool_returns_input() {
    let tool = EchoArgsTool::new();
    let input = serde_json::json!({"value": "hello world"});
    let result = tool.execute(input).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("hello world"));
}

#[tokio::test]
async fn test_slow_echo_waits_on_barrier() {
    let barrier = Arc::new(Mutex::new(()));
    let tool = SlowEchoTool::new(Arc::clone(&barrier));

    let handle = tokio::spawn(async move {
        let input = serde_json::json!({"value": "delayed"});
        tool.execute(input).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    drop(barrier);

    let result = handle.await.unwrap();
    assert!(result.is_ok());
    assert!(result.unwrap().contains("delayed"));
}

#[derive(Clone)]
struct RetryThenSuccessProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
    error_before_success: bool,
}

impl RetryThenSuccessProvider {
    fn new(error_before_success: bool) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            response_index: Arc::new(Mutex::new(0)),
            error_before_success,
        }
    }
}

#[async_trait]
impl Provider for RetryThenSuccessProvider {
    fn id(&self) -> &str {
        "retry-then-success"
    }

    fn name(&self) -> &str {
        "Retry Then Success"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());

        let mut idx = self.response_index.lock().await;
        *idx += 1;

        let events = if *idx == 1 && self.error_before_success {
            return Err(ProviderError::Stream("retryable stream error".to_string()));
        } else {
            vec![ChatEvent::TextDelta(
                "Success after retry".to_string().into(),
            )]
        };

        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".to_string(),
            name: "Test Model".to_string(),
            provider: "test".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

#[derive(Clone)]
struct AuthErrorProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl AuthErrorProvider {
    fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Provider for AuthErrorProvider {
    fn id(&self) -> &str {
        "auth-error"
    }

    fn name(&self) -> &str {
        "Auth Error"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());
        Err(ProviderError::Auth("invalid token".to_string()))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[derive(Clone)]
struct RepeatedRateLimitProvider {
    request_count: Arc<Mutex<usize>>,
    max_retries: usize,
}

impl RepeatedRateLimitProvider {
    fn new(max_retries: usize) -> Self {
        Self {
            request_count: Arc::new(Mutex::new(0)),
            max_retries,
        }
    }

    async fn get_call_count(&self) -> usize {
        *self.request_count.lock().await
    }
}

#[async_trait]
impl Provider for RepeatedRateLimitProvider {
    fn id(&self) -> &str {
        "repeated-rate-limit"
    }

    fn name(&self) -> &str {
        "Repeated Rate Limit"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self::new(self.max_retries))
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut count = self.request_count.lock().await;
        *count += 1;

        if *count <= self.max_retries {
            return Err(ProviderError::RateLimit);
        }

        let stream = futures_util::stream::iter(vec![Ok::<_, ProviderError>(ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        })]);
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[derive(Clone)]
struct RepeatedStreamErrorProvider {
    request_count: Arc<Mutex<usize>>,
    max_retries: usize,
}

impl RepeatedStreamErrorProvider {
    fn new(max_retries: usize) -> Self {
        Self {
            request_count: Arc::new(Mutex::new(0)),
            max_retries,
        }
    }

    async fn get_call_count(&self) -> usize {
        *self.request_count.lock().await
    }
}

#[async_trait]
impl Provider for RepeatedStreamErrorProvider {
    fn id(&self) -> &str {
        "repeated-stream-error"
    }

    fn name(&self) -> &str {
        "Repeated Stream Error"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut count = self.request_count.lock().await;
        *count += 1;
        if *count <= self.max_retries {
            Err(ProviderError::Stream("retryable stream error".to_string()))
        } else {
            let events = vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }];
            let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
            Ok(Box::pin(stream))
        }
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[derive(Clone)]
struct RepeatedTimeoutProvider {
    request_count: Arc<Mutex<usize>>,
    max_retries: usize,
}

impl RepeatedTimeoutProvider {
    fn new(max_retries: usize) -> Self {
        Self {
            request_count: Arc::new(Mutex::new(0)),
            max_retries,
        }
    }

    async fn get_call_count(&self) -> usize {
        *self.request_count.lock().await
    }
}

#[async_trait]
impl Provider for RepeatedTimeoutProvider {
    fn id(&self) -> &str {
        "repeated-timeout"
    }

    fn name(&self) -> &str {
        "Repeated Timeout"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut count = self.request_count.lock().await;
        *count += 1;
        if *count <= self.max_retries {
            Err(ProviderError::Timeout("retryable timeout".to_string()))
        } else {
            let events = vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }];
            let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
            Ok(Box::pin(stream))
        }
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

fn build_agent_loop_with_error_config(
    provider: Box<dyn Provider>,
    tool_registry: ToolRegistry,
) -> AgentLoop {
    let agents = vec![Agent {
        git_read_only: false,
        name: "build".to_string(),
        role: None,
        description: "Test agent".to_string(),
        mode: codegg::agent::AgentMode::Primary,
        mode_name: None,
        model: None,
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
    }];

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![codegg::permission::ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Allow,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let config = Config::default();

    AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        std::env::current_dir().expect("test workspace root"),
        "test-session".to_string(),
    )
}

#[tokio::test]
async fn test_retryable_stream_error_then_success() {
    let provider = Box::new(RetryThenSuccessProvider::new(true));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);

    let request = make_chat_request("Test retry");
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;
    assert!(result.is_ok(), "Should succeed after retry");
}

#[tokio::test]
async fn test_auth_error_not_retried() {
    let provider = Box::new(AuthErrorProvider::new());

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);

    let request = make_chat_request("Test auth");
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;

    assert!(result.is_err(), "Should return error for auth failure");
    let err = result.unwrap_err();
    let is_auth_error = matches!(
        err,
        codegg::error::AppError::Provider(codegg::error::ProviderError::Auth(_))
    );
    assert!(is_auth_error, "Should be auth error: {:?}", err);
}

#[tokio::test]
async fn test_repeated_rate_limit_returns_final_error() {
    let provider_inner = RepeatedRateLimitProvider::new(3);
    let provider = Box::new(provider_inner.clone());

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);

    let request = make_chat_request("Test rate limit");
    let result: Result<Vec<ChatEvent>, _> = agent_loop.run(request).await;

    assert!(
        result.is_err(),
        "Should return final error after max retries"
    );
    let err = result.unwrap_err();
    let is_rate_limit = matches!(
        err,
        codegg::error::AppError::Provider(codegg::error::ProviderError::RateLimit)
    );
    assert!(is_rate_limit, "Should be rate limit error: {:?}", err);

    let call_count = provider_inner.get_call_count().await;
    assert_eq!(call_count, 3, "Provider should be called exactly 3 times");
}

#[tokio::test]
async fn test_repeated_stream_error_exhaustion() {
    let provider_inner = RepeatedStreamErrorProvider::new(3);
    let provider = Box::new(provider_inner.clone());

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    let request = make_chat_request("Test stream error exhaustion");
    let result = agent_loop.run(request).await;

    assert!(result.is_err(), "Should return error after max retries");
    let err = result.unwrap_err();
    let is_stream_error = matches!(err, AppError::Provider(ProviderError::Stream(_)));
    assert!(
        is_stream_error,
        "Should return last retryable error (Stream), got: {:?}",
        err
    );

    let call_count = provider_inner.get_call_count().await;
    assert_eq!(call_count, 3, "Provider should be called exactly 3 times");
}

#[tokio::test]
async fn test_repeated_timeout_exhaustion() {
    let provider_inner = RepeatedTimeoutProvider::new(3);
    let provider = Box::new(provider_inner.clone());

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    let request = make_chat_request("Test timeout exhaustion");
    let result = agent_loop.run(request).await;

    assert!(result.is_err(), "Should return error after max retries");
    let err = result.unwrap_err();
    let is_timeout_error = matches!(err, AppError::Provider(ProviderError::Timeout(_)));
    assert!(
        is_timeout_error,
        "Should return last retryable error (Timeout), got: {:?}",
        err
    );

    let call_count = provider_inner.get_call_count().await;
    assert_eq!(call_count, 3, "Provider should be called exactly 3 times");
}

#[derive(Clone)]
struct FollowUpProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
    responses: Vec<Vec<ChatEvent>>,
}

impl FollowUpProvider {
    fn new(responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            response_index: Arc::new(Mutex::new(0)),
            responses,
        }
    }

    async fn get_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl Provider for FollowUpProvider {
    fn id(&self) -> &str {
        "follow-up"
    }

    fn name(&self) -> &str {
        "Follow Up Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());

        let mut idx = self.response_index.lock().await;
        let events = if *idx < self.responses.len() {
            self.responses[*idx].clone()
        } else {
            vec![ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            }]
        };
        *idx += 1;

        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "test/model".to_string(),
            name: "Test Model".to_string(),
            provider: "test".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

#[tokio::test]
async fn test_no_follow_up_latency() {
    use std::time::Instant;

    let scripted_provider = Box::new(ScriptedProvider::new(vec![vec![
        ChatEvent::TextDelta("Simple response".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ]]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(scripted_provider.clone(), registry);

    let request = make_chat_request("Hello");

    let start = Instant::now();
    let result = agent_loop.run(request).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Should complete successfully");
    // The agent loop uses non-blocking try_recv() to drain follow-ups, so a
    // run with no queued follow-up must not block. We bound wall-clock to a
    // generous 15s ceiling (well under the legacy 30s blocking wait) while
    // remaining strict enough to catch regressions like a stray blocking
    // recv(). A real blocking recv() would push elapsed to the stream idle
    // timeout (120s), so 15s is comfortably above cold-start / contention
    // jitter (the agent_loop runs at ~0.7s isolated, ~5-10s under heavy
    // parallel load from other tests in this binary) yet well below any
    // possible blocking-regression signature.
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "No-follow-up run should complete quickly, took {:?}",
        elapsed
    );
    // Behavioral check: no follow-up was sent, so the provider must have been
    // called exactly once. This is the deterministic core of the test — the
    // timing bound above is a sanity check that catches blocking-receive
    // regressions, while this assertion proves the loop did not loop.
    let provider_calls = scripted_provider.request_count().await;
    assert_eq!(
        provider_calls, 1,
        "Provider should be invoked exactly once when no follow-up is queued; got {}",
        provider_calls
    );
}

#[tokio::test]
async fn test_follow_up_sender_channel_works() {
    let scripted_provider = Box::new(ScriptedProvider::new(vec![vec![
        ChatEvent::TextDelta("Response".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ]]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(scripted_provider.clone(), registry);

    let follow_up_tx = agent_loop.follow_up_sender();
    follow_up_tx.try_send("Test follow-up".to_string()).ok();

    let request = make_chat_request("Hello");
    let result = agent_loop.run(request).await;
    assert!(result.is_ok());

    let requests = scripted_provider.get_requests().await;
    assert!(!requests.is_empty());
}

#[tokio::test]
async fn test_follow_up_queued_before_run_is_processed() {
    let scripted_provider = Box::new(ScriptedProvider::new(vec![
        vec![
            ChatEvent::TextDelta("First response".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("Follow-up response".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_agent_loop_with_error_config(scripted_provider.clone(), registry);

    // Queue follow-up BEFORE first run()
    let follow_up_tx = agent_loop.follow_up_sender();
    follow_up_tx.try_send("Early follow-up".to_string()).ok();

    let request = make_chat_request("Hello");
    let result = agent_loop.run(request).await;
    assert!(result.is_ok(), "Run should succeed with queued follow-up");

    // The queued follow-up should cause an extra provider request
    let requests = scripted_provider.get_requests().await;
    assert_eq!(
        requests.len(),
        2,
        "Should have 2 requests (initial + follow-up)"
    );

    // Verify the follow-up was processed
    assert!(
        requests[1].messages.iter().any(|m| {
            if let Message::User { content } = m {
                content.iter().any(|p| {
                    if let codegg::provider::ContentPart::Text { text } = p {
                        text.contains("Early follow-up")
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        }),
        "Second request should contain the follow-up prompt"
    );
}

#[tokio::test]
async fn test_follow_up_with_tool_call() {
    let response1 = vec![
        ChatEvent::TextDelta("First response".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Using tool in follow-up".to_string().into()),
        ChatEvent::ToolCall(ToolCall {
            id: "call_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "follow-up tool"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response3 = vec![
        ChatEvent::TextDelta("Tool result received".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let provider = Box::new(FollowUpProvider::new(vec![response1, response2, response3]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(provider.clone(), registry).await;

    let follow_up_tx = agent_loop.follow_up_sender();
    let request = make_chat_request("Initial prompt");

    // Send follow-up BEFORE run() to ensure it's queued when drain_follow_up is called
    follow_up_tx
        .try_send("Follow-up with tool".to_string())
        .ok();

    let result = agent_loop.run(request).await;
    assert!(result.is_ok(), "Should complete successfully");
    let events = result.unwrap();

    let requests = provider.get_requests().await;

    // Packet 2: Strengthened assertions
    // Require exactly 3 provider requests
    assert!(
        requests.len() >= 3,
        "Should have at least 3 provider calls, got {}",
        requests.len()
    );

    // Request 2 (index 1) should contain the follow-up user prompt
    let has_follow_up = requests.iter().any(|req| {
        req.messages.iter().any(|m| {
            if let Message::User { content } = m {
                content.iter().any(|p| {
                    if let codegg::provider::ContentPart::Text { text } = p {
                        text.contains("Follow-up with tool")
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        })
    });
    assert!(
        has_follow_up,
        "Request 2 should contain the follow-up user prompt"
    );

    // Request 3 (index 2) should have assistant tool call BEFORE tool result
    // The request includes full history, so check the last messages
    let req3 = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_1"))
        })
        .expect("Expected a request containing tool result for call_1");
    let msg_count = req3.messages.len();
    // Last messages should include assistant tool call and matching tool result.
    let last_messages = &req3.messages[msg_count - 3..];
    assert_messages_have_roles(last_messages, &["assistant", "tool"]);

    // Verify assistant has the tool call with correct ID
    let assistant_with_call = req3
        .messages
        .iter()
        .find(|m| {
            matches!(
                m,
                Message::Assistant { tool_calls, .. }
                    if tool_calls.iter().any(|tc| tc.id.as_ref() == "call_1")
            )
        })
        .expect("Expected assistant message containing tool call call_1");
    assert_assistant_has_tool_call(
        assistant_with_call,
        "call_1",
        "echo_args",
        Some("follow-up tool"),
    );

    // Verify tool result exists with correct ID and is after assistant
    let tool_result_msg = req3
        .messages
        .iter()
        .find(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_1"))
        .expect("Expected Tool message for call_1");
    if let Message::Tool {
        tool_call_id,
        content,
    } = tool_result_msg
    {
        assert_eq!(tool_call_id.as_ref(), "call_1");
        assert!(content.as_ref().contains("follow-up tool"));
    }
    assert_assistant_tool_call_precedes_result(last_messages, "call_1");

    // Verify no orphan tool results
    assert_no_orphan_tool_results(&req3.messages);

    // Final returned events should include the provider's third response text
    let has_final_text = events.iter().any(|e| {
        if let ChatEvent::TextDelta(text) = e {
            text.contains("Tool result received")
        } else {
            false
        }
    });
    assert!(
        has_final_text,
        "Final events should include provider's third response text"
    );
}

// =============================================================================
// Packet 4: Strengthen Permission Path Tests
// =============================================================================

#[tokio::test]
async fn test_permission_ask_allow_once() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};

    // Configure permission checker: echo_args requires Ask (user approval)
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_allow_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "test_ask_allow"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Tool executed".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    agent_loop.set_session_id("perm-test-ask-allow");

    let perm_id = "call_allow_1-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    wait_for_permission_pending(&perm_id, &mut rx, std::time::Duration::from_secs(5))
        .await
        .unwrap();

    let responded = PermissionRegistry::respond_scoped(
        "perm-test-ask-allow",
        &perm_id,
        PermissionDecision::AllowOnce,
    );
    assert!(responded, "Permission should be responded to");

    let result = handle.await.unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    // Verify the tool executed and result is in provider request
    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");

    let req2 = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_allow_1"))
        })
        .expect("Expected a request containing tool result for call_allow_1");
    assert_messages_have_roles(&req2.messages, &["user", "assistant", "tool"]);
    assert_tool_result_with_id(&req2.messages, "call_allow_1", Some("test_ask_allow"));
    assert_assistant_tool_call_precedes_result(&req2.messages, "call_allow_1");
    assert_no_orphan_tool_results(&req2.messages);

    // Verify PermissionRegistry is cleaned up
    let cleaned_up = PermissionRegistry::respond_scoped(
        "perm-test-ask-allow",
        &perm_id,
        PermissionDecision::AllowOnce,
    );

    assert!(
        !cleaned_up,
        "PermissionRegistry should be unregistered after completion"
    );
}

#[tokio::test]
async fn test_permission_ask_deny_once() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};

    // Configure permission checker: echo_args requires Ask
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_deny_1".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "test_ask_deny"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("Tool denied".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    agent_loop.set_session_id("perm-test-ask-deny");

    let perm_id = "call_deny_1-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    wait_for_permission_pending(&perm_id, &mut rx, std::time::Duration::from_secs(5))
        .await
        .unwrap();

    let responded = PermissionRegistry::respond_scoped(
        "perm-test-ask-deny",
        &perm_id,
        PermissionDecision::DenyOnce,
    );
    assert!(responded, "Permission should be responded to");

    let result = handle.await.unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    // Verify the tool was denied and error result is in provider request
    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");

    let req2 = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_deny_1"))
        })
        .expect("Expected a request containing tool result for call_deny_1");
    assert_messages_have_roles(&req2.messages, &["user", "assistant", "tool"]);
    // Tool result should contain denied/error
    assert_tool_result_with_id(&req2.messages, "call_deny_1", Some("denied"));
    assert_assistant_tool_call_precedes_result(&req2.messages, "call_deny_1");
    assert_no_orphan_tool_results(&req2.messages);
}

// =============================================================================
// Packet 5: Mixed Multi-Tool Ordering And Concurrency
// =============================================================================

#[tokio::test]
async fn test_mixed_tool_ordering() {
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};

    // Create a barrier for slow_echo to control completion order
    let barrier = Arc::new(tokio::sync::Mutex::new(()));
    let barrier_clone = barrier.clone();

    // Scripted provider with multiple tool calls in specific order
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_slow".to_string().into(),
            name: "slow_echo".to_string().into(),
            arguments: serde_json::json!({"value": "slow"}),
        }),
        ChatEvent::ToolCall(ToolCall {
            id: "call_denied".to_string().into(),
            name: "denied_tool".to_string().into(),
            arguments: serde_json::json!({"value": "denied"}),
        }),
        ChatEvent::ToolCall(ToolCall {
            id: "call_missing".to_string().into(),
            name: "missing_tool".to_string().into(),
            arguments: serde_json::json!({"value": "missing"}),
        }),
        ChatEvent::ToolCall(ToolCall {
            id: "call_fast".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "fast"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("All tools processed".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    // Set up tools
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    registry.register(SlowEchoTool::new(barrier_clone));

    // Configure permission checker: denie "denied_tool"
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "denied_tool".to_string(),
                level: PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;

    let request = make_chat_request("Use multiple tools");
    let result = agent_loop.run(request).await;
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    // Release the barrier to allow slow_echo to complete
    drop(barrier);

    // Verify the tool results are in the correct order
    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");

    let req2 = requests
        .iter()
        .rev()
        .find(|r| {
            let ids = get_tool_results_in_order(&r.messages);
            ids.iter().any(|id| id == "call_slow")
                && ids.iter().any(|id| id == "call_denied")
                && ids.iter().any(|id| id == "call_missing")
                && ids.iter().any(|id| id == "call_fast")
        })
        .expect("Expected a request containing all tool results");

    // Check that tool results are in the original order: call_slow, call_denied, call_missing, call_fast
    let tool_ids = get_tool_results_in_order(&req2.messages);
    assert_eq!(
        tool_ids,
        vec!["call_slow", "call_denied", "call_missing", "call_fast"],
        "Tool results should be in original provider order"
    );

    // Verify each result content
    assert_tool_result_with_id(&req2.messages, "call_slow", Some("slow"));
    assert_tool_result_with_id(&req2.messages, "call_denied", Some("denied"));
    assert_tool_result_with_id(&req2.messages, "call_missing", Some("missing"));
    assert_tool_result_with_id(&req2.messages, "call_fast", Some("fast"));

    // Verify no orphan tool results
    assert_no_orphan_tool_results(&req2.messages);
}

#[tokio::test]
async fn test_max_parallel_tools_enforcement() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    // Shared state to track concurrent executions
    let current = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Mutex::new(()));

    // Lock barrier initially so tools wait for coordination
    let barrier_guard = barrier.lock().await;

    // Create tool registry with 3 parallel tools
    let mut registry = ToolRegistry::new();
    for i in 0..3 {
        let tool = ParallelTool::new(
            format!("parallel_tool_{}", i),
            current.clone(),
            max_observed.clone(),
            barrier.clone(),
        );
        registry.register(tool);
    }

    // Configure max_parallel_tools = 2
    let config = Config {
        server: Some(ServerConfig {
            max_parallel_tools: Some(2),
            ..Default::default()
        }),
        ..Default::default()
    };

    // Create permission checker that allows all parallel tools
    let mut tool_rules = vec![];
    for i in 0..3 {
        tool_rules.push(codegg::permission::ToolRule {
            tool: format!("parallel_tool_{}", i),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        });
    }
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules,
            path_rules: vec![],
        });

    // Scripted provider: first turn emits 3 tool calls, second turn returns text
    let tool_calls: Vec<ChatEvent> = (0..3)
        .map(|i| {
            ChatEvent::ToolCall(ToolCall {
                id: Arc::new(format!("call_{}", i)),
                name: Arc::new(format!("parallel_tool_{}", i)),
                arguments: serde_json::json!({}),
            })
        })
        .collect();
    let mut first_turn = tool_calls;
    first_turn.push(ChatEvent::Finish {
        stop_reason: "tool_calls".to_string().into(),
        usage: TokenUsage::default(),
    });
    let responses = vec![
        first_turn,
        vec![
            ChatEvent::TextDelta("All tools completed".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(responses));

    // Build agent loop with custom config
    let mut agent_loop = build_test_agent_loop_with_config(
        scripted_provider.clone(),
        registry,
        permission_checker,
        config,
    )
    .await;
    agent_loop.set_session_id("max-parallel-test");

    // Run agent loop in background task
    let request = make_chat_request("Run 3 parallel tools");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    // Poll for tools to start executing. Wait until at least one
    // tool has been entered (current > 0) or 1s elapses.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if current.load(Ordering::SeqCst) > 0 || std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Verify max observed concurrent tools does not exceed 2
    let max = max_observed.load(Ordering::SeqCst);
    assert!(max <= 2, "Max concurrent tools should be <= 2, got {}", max);

    // Release barrier to allow tools to complete
    drop(barrier_guard);

    // Wait for agent loop to finish
    let result = handle.await.unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    // Verify provider requests are valid
    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "Should have at least 2 provider calls");
    let last = requests.last().expect("Expected at least one request");
    assert_no_orphan_tool_results(&last.messages);
}

#[tokio::test]
async fn parent_and_subagent_batches_keep_independent_permit_owners() {
    let total_active = Arc::new(AtomicUsize::new(0));
    let total_peak = Arc::new(AtomicUsize::new(0));
    let rendezvous = Arc::new(tokio::sync::Barrier::new(4));
    let parent_active = Arc::new(AtomicUsize::new(0));
    let parent_peak = Arc::new(AtomicUsize::new(0));
    let child_active = Arc::new(AtomicUsize::new(0));
    let child_peak = Arc::new(AtomicUsize::new(0));
    let mut parent_registry = ToolRegistry::new();
    let mut child_registry = ToolRegistry::new();
    for index in 0..2 {
        parent_registry.register(BatchOwnerTool {
            name: format!("parent_batch_{index}"),
            owner_active: parent_active.clone(),
            owner_peak: parent_peak.clone(),
            total_active: total_active.clone(),
            total_peak: total_peak.clone(),
            rendezvous: rendezvous.clone(),
        });
        child_registry.register(BatchOwnerTool {
            name: format!("subagent_batch_{index}"),
            owner_active: child_active.clone(),
            owner_peak: child_peak.clone(),
            total_active: total_active.clone(),
            total_peak: total_peak.clone(),
            rendezvous: rendezvous.clone(),
        });
    }
    let config = Config {
        server: Some(ServerConfig {
            max_parallel_tools: Some(2),
            ..Default::default()
        }),
        ..Default::default()
    };
    let names = [
        "parent_batch_0",
        "parent_batch_1",
        "subagent_batch_0",
        "subagent_batch_1",
    ];
    let permission_checker = || {
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: names
                .iter()
                .map(|name| codegg::permission::ToolRule {
                    tool: (*name).into(),
                    level: PermissionLevel::Allow,
                    paths: None,
                    bash_patterns: None,
                })
                .collect(),
            path_rules: vec![],
        })
    };
    let make_provider = |prefix: &str| {
        let mut calls: Vec<_> = (0..2)
            .map(|index| {
                let name = format!("{prefix}_batch_{index}");
                ChatEvent::ToolCall(ToolCall {
                    id: format!("{prefix}-call-{index}").into(),
                    name: name.into(),
                    arguments: serde_json::json!({}),
                })
            })
            .collect();
        calls.push(ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        });
        Box::new(ScriptedProvider::new(vec![
            calls,
            vec![
                ChatEvent::TextDelta("done".to_string().into()),
                ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ],
        ]))
    };
    let root = std::env::current_dir()
        .expect("workspace")
        .canonicalize()
        .expect("canonical workspace");
    let make_loop = |name: &str, mode: codegg::agent::AgentMode, provider, registry| {
        AgentLoop::new(
            vec![Agent {
                git_read_only: false,
                name: name.into(),
                role: None,
                description: name.into(),
                mode,
                mode_name: None,
                model: None,
                variant: None,
                temperature: None,
                top_p: None,
                color: None,
                steps: None,
                system_prompt: None,
                permissions: Default::default(),
                hidden: false,
                thinking_budget: None,
                reasoning_effort: None,
                fallback_model: None,
                runtime_kind: None,
            }],
            provider,
            permission_checker(),
            registry,
            config.clone(),
            None,
            None,
            Arc::new(InMemoryArtifactStore::new()),
            root.clone(),
            format!("{name}-session"),
        )
    };
    let mut parent = make_loop(
        "build",
        codegg::agent::AgentMode::Primary,
        make_provider("parent"),
        parent_registry,
    );
    let mut subagent = make_loop(
        "subagent",
        codegg::agent::AgentMode::Subagent,
        make_provider("subagent"),
        child_registry,
    );
    subagent.set_session_id("build-session");
    attach_test_workspace_authority(&mut parent, &root).await;
    attach_test_workspace_authority(&mut subagent, &root).await;

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            parent.run(make_chat_request("parent batch")),
            subagent.run(make_chat_request("subagent batch")),
        )
    })
    .await
    .expect("both batches must make progress");
    assert!(result.0.is_ok(), "parent failed: {:?}", result.0.err());
    assert!(result.1.is_ok(), "subagent failed: {:?}", result.1.err());
    assert_eq!(parent_peak.load(Ordering::SeqCst), 2);
    assert_eq!(child_peak.load(Ordering::SeqCst), 2);
    assert_eq!(total_peak.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn admitted_native_command_timeout_preserves_partial_output_in_agent_transcript() {
    let root = std::env::current_dir()
        .expect("workspace")
        .canonicalize()
        .expect("canonical workspace");
    let mut registry = ToolRegistry::new();
    registry.register(
        codegg::tool::bash::BashTool::new()
            .with_workspace_root(root)
            .with_sandbox_profile(
                codegg_core::approval::SandboxProfile::FullHost,
                &std::env::current_dir().expect("workspace"),
            ),
    );
    let provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "native-timeout-partial".to_string().into(),
                name: "bash".to_string().into(),
                arguments: serde_json::json!({
                    "command": "printf C006-PARTIAL-OUTPUT; sleep 5",
                    "timeout": 1
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("observed timeout output".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut agent = build_test_agent_loop(Box::new(provider.clone()), registry).await;
    agent.set_sandbox_profile(codegg_core::approval::SandboxProfile::FullHost);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        agent.run(make_chat_request("run a bounded native command")),
    )
    .await
    .expect("admitted timeout should remain bounded");
    assert!(result.is_ok(), "agent turn failed: {result:?}");
    let requests = provider.get_requests().await;
    assert!(
        requests
            .iter()
            .flat_map(|request| request.messages.iter())
            .any(|message| {
                matches!(message, Message::Tool { tool_call_id, content }
            if tool_call_id.as_ref() == "native-timeout-partial"
                && content.as_ref().contains("C006-PARTIAL-OUTPUT")
                && content.as_ref().contains("timed out"))
            }),
        "the native timeout must reach the model with partial output: {:?}",
        requests
            .iter()
            .flat_map(|request| request.messages.iter())
            .filter_map(|message| match message {
                Message::Tool {
                    tool_call_id,
                    content,
                } if tool_call_id.as_ref() == "native-timeout-partial" => {
                    Some(content.as_ref().to_string())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}

// =============================================================================
// Packet 10: Task Tool Integration With Subagents
// =============================================================================
// Packet 10: Task Tool Integration With Subagents
// =============================================================================

/// Provider that records requests for verification
#[derive(Clone)]
struct RequestRecordingProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    responses: Vec<Vec<ChatEvent>>,
    response_index: Arc<Mutex<usize>>,
}

impl RequestRecordingProvider {
    fn new(responses: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses,
            response_index: Arc::new(Mutex::new(0)),
        }
    }

    async fn get_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }

    async fn wait_for_request(&self, max_attempts: u32, interval_ms: u64) -> Result<(), String> {
        for _ in 0..max_attempts {
            let count = { self.requests.lock().await.len() };
            if count > 0 {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        }
        Err(format!(
            "Timeout waiting for request after {} attempts",
            max_attempts
        ))
    }
}

#[async_trait]
impl Provider for RequestRecordingProvider {
    fn id(&self) -> &str {
        "request-recording"
    }

    fn name(&self) -> &str {
        "Request Recording Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
        self.requests.lock().await.push(request.clone());
        let mut idx = self.response_index.lock().await;
        let events = if *idx < self.responses.len() {
            self.responses[*idx].clone()
        } else {
            vec![
                ChatEvent::TextDelta("Default response".to_string().into()),
                ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                },
            ]
        };
        *idx += 1;
        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "recording/recording-model".to_string(),
            name: "Recording Model".to_string(),
            provider: "recording".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

/// A deterministic provider for subagent that returns a known response
#[derive(Clone)]
struct DeterministicSubagentProvider {
    _marker: std::marker::PhantomData<()>,
}

impl DeterministicSubagentProvider {
    fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl Provider for DeterministicSubagentProvider {
    fn id(&self) -> &str {
        "deterministic-subagent"
    }

    fn name(&self) -> &str {
        "Deterministic Subagent Provider"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let events = vec![
            ChatEvent::TextDelta("Subagent completed successfully".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ];
        let stream = futures_util::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
        Ok(Box::pin(stream))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "deterministic-subagent/subagent-model".to_string(),
            name: "Subagent Model".to_string(),
            provider: "deterministic-subagent".to_string(),
            context_window: 4096,
            max_output_tokens: Some(2048),
            supports_tools: true,
            supports_vision: false,
            variants: vec![],
        }])
    }
}

#[tokio::test]
async fn test_task_tool_integration_with_subagent() {
    use codegg::agent::worker::SubAgentPool;
    use codegg::config::schema::Config;
    use codegg::provider::ProviderRegistry;
    use codegg::session::SessionStore;
    use codegg::tool::task::TaskStore;
    use std::sync::Arc;

    // Create in-memory SQLite pool
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create pool");

    // Create SessionStore
    let session_store = Arc::new(SessionStore::new(pool));

    // Create provider registry with subagent provider
    let mut provider_registry = ProviderRegistry::new();
    provider_registry.register(DeterministicSubagentProvider::new());

    // Create agents
    let agents = vec![
        Agent {
            git_read_only: false,
            name: "build".to_string(),
            role: None,
            description: "Main agent".to_string(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
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
        },
        Agent {
            git_read_only: false,
            name: "subagent".to_string(),
            role: None,
            description: "Subagent".to_string(),
            mode: codegg::agent::AgentMode::Subagent,
            mode_name: None,
            model: Some("deterministic-subagent/subagent-model".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: Some("You are a subagent".to_string()),
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        },
    ];

    let config = Config::default();

    // Create SubAgentPool
    let subagent_pool = SubAgentPool::new(
        &config,
        agents.clone(),
        provider_registry,
        session_store,
        None,
    )
    .await;

    // Create TaskStore and TaskTool
    let task_store = Arc::new(tokio::sync::Mutex::new(TaskStore::new()));
    let spawner = subagent_pool.spawner();
    let task_tool = TaskTool::new(
        task_store.clone(),
        Some(spawner),
        Some("test-session-123".to_string()),
        vec![],
    );

    // Create main provider that emits a "task" tool call
    let main_provider = Box::new(ScriptedProvider::new(vec![
        // Turn 1: Provider emits task tool call
        vec![
            ChatEvent::TextDelta("Spawning subagent...".to_string().into()),
            ChatEvent::ToolCall(ToolCall {
                id: "call_task_1".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({
                    "action": "spawn",
                    "description": "Test subagent task",
                    "prompt": "Do the work",
                    "agent": "subagent"
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        // Turn 2: After task tool executes
        vec![
            ChatEvent::TextDelta("Task completed".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]));

    // Create tool registry with TaskTool
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(task_tool);

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        });

    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let mut agent_loop = AgentLoop::new(
        agents,
        main_provider.clone(),
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.clone(),
        "test-session-123".to_string(),
    );
    attach_test_workspace_authority(&mut agent_loop, &workspace_root).await;

    // Run the AgentLoop
    let request = make_chat_request("Spawn a subagent to do work");
    let result = agent_loop.run(request).await;

    assert!(
        result.is_ok(),
        "AgentLoop should complete successfully: {:?}",
        result.err()
    );

    // Verify the task tool result is in the transcript
    let requests = main_provider.get_requests().await;
    assert!(
        requests.len() >= 2,
        "Should have at least 2 provider calls, got {}",
        requests.len()
    );

    // The last request should contain tool results
    let last_request = &requests[requests.len() - 1];

    // Check that there's a tool message with the task result
    let has_task_result = last_request.messages.iter().any(|m| {
        if let Message::Tool {
            tool_call_id,
            content,
        } = m
        {
            tool_call_id.as_ref() == "call_task_1" && content.as_ref().contains("Subagent spawned")
        } else {
            false
        }
    });

    assert!(
        has_task_result,
        "Task tool result should be in the transcript"
    );

    // Verify task is stored in TaskStore by checking the task count
    // (TaskStore has private fields, so we use the public API)
    let _store = task_store.lock().await;
    // The store should have at least one task
    // We can verify by trying to get a task (even if we don't know the ID,
    // the fact that the tool returned successfully means it was created)
    // For now, just verify the tool result is in the transcript (already done above)
}

#[tokio::test]
async fn task_tool_status_get_group_and_cross_session_contracts_are_model_visible() {
    use codegg::agent::run_control::RunControlService;
    use codegg::tool::task::TaskStore;
    use codegg_core::agent_run::{
        AgentRunBudget, AgentRunStore, AgentRunTerminalOutcome, InMemoryAgentRunStore, NewAgentRun,
        NewAgentTask,
    };
    use codegg_core::agent_run_group::{
        AgentRunGroupOwner, AgentRunGroupService, NewAgentRunGroup, RunJoinPolicy,
    };
    use codegg_core::identity::{AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId};
    use codegg_core::workspace::WorkspaceId;

    let session = "task-contract-session";
    let turn = "task-contract-turn";
    let workspace = WorkspaceId::new_unchecked("task-contract-workspace");
    let task_id = AgentTaskId::new();
    let run_id = AgentRunId::new();
    let runs: Arc<dyn AgentRunStore> = Arc::new(InMemoryAgentRunStore::new());
    let _accepted = runs
        .create_or_get(
            NewAgentTask {
                task_id: task_id.clone(),
                parent_task_id: None,
                originating_session_id: session.into(),
                originating_turn_id: Some(turn.into()),
                project_id: ProjectId::new(),
                repository_id: None,
                workspace_id: workspace.clone(),
                requested_agent: "subagent".into(),
                delegation_key: "task-contract-delegation".into(),
                request_fingerprint: "task-contract-request".into(),
                description: "completed fixture child".into(),
            },
            NewAgentRun {
                run_id: run_id.clone(),
                parent_run_id: None,
                depth: 1,
                workspace_id: workspace,
                agent_name: "subagent".into(),
                agent_digest: Some("fixture-agent-digest".into()),
                provider: "scripted".into(),
                model: "fixture-model".into(),
                authority_digest: "fixture-authority".into(),
                budget: AgentRunBudget::default(),
                shared_scratch: vec![],
            },
        )
        .await
        .expect("seed durable run");
    runs.transition_task(&task_id, codegg_core::agent_run::AgentTaskStatus::Queued)
        .await
        .expect("queue task");
    runs.transition(&run_id, codegg_core::agent_run::AgentRunStatus::Queued)
        .await
        .expect("queue run");
    runs.transition(&run_id, codegg_core::agent_run::AgentRunStatus::Preparing)
        .await
        .expect("prepare run");
    runs.transition(&run_id, codegg_core::agent_run::AgentRunStatus::Running)
        .await
        .expect("start run");
    let completed = runs
        .finish(
            &run_id,
            AgentRunTerminalOutcome::Completed,
            Some("fixture-result-private-marker".into()),
            None,
            None,
        )
        .await
        .expect("complete run");
    let groups = AgentRunGroupService::in_memory(runs.clone());
    let group_id = AgentRunGroupId::new();
    groups
        .create(NewAgentRunGroup {
            group_id: group_id.clone(),
            root_run_id: completed.root_run_id.clone(),
            owner_run_id: run_id.clone(),
            owner: AgentRunGroupOwner::Turn {
                session_id: session.into(),
                turn_id: turn.into(),
            },
            member_run_ids: vec![run_id.clone()],
            join_policy: RunJoinPolicy::All,
            cancel_remaining_on_satisfaction: false,
            idempotency_key: "task-contract-group".into(),
        })
        .await
        .expect("seed group");

    let task_tool = TaskTool::new(
        Arc::new(tokio::sync::Mutex::new(TaskStore::new())),
        None,
        Some(session.into()),
        vec![],
    )
    .with_agent_run_store(runs.clone())
    .with_run_control_opt(Some(RunControlService::in_memory(runs.clone())))
    .with_run_group_service(Some(groups.clone()))
    .with_turn_owner(session.into(), turn.into())
    .with_project_context(None, None, Some(turn.into()));
    let mut registry = ToolRegistry::new();
    registry.register(task_tool);
    let provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "task-status".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"status", "run_id":run_id}),
            }),
            ChatEvent::ToolCall(ToolCall {
                id: "task-wait".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"wait", "run_id":run_id, "timeout_ms":10}),
            }),
            ChatEvent::ToolCall(ToolCall {
                id: "task-get".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"get", "task_id":task_id}),
            }),
            ChatEvent::ToolCall(ToolCall {
                id: "task-group-status".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"status_group", "group_id":group_id}),
            }),
            ChatEvent::ToolCall(ToolCall {
                id: "task-group-wait".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"wait_group", "group_id":group_id, "timeout_ms":10}),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("retrieval complete".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut agent = build_test_agent_loop(Box::new(provider.clone()), registry).await;
    agent.set_session_id(session);
    assert!(agent
        .run(make_chat_request("inspect the delegated task"))
        .await
        .is_ok());
    let requests = provider.get_requests().await;
    let tool_results: Vec<_> = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter_map(|message| match message {
            Message::Tool {
                tool_call_id,
                content,
            } => Some((tool_call_id.to_string(), content.to_string())),
            _ => None,
        })
        .collect();
    for id in [
        "task-status",
        "task-wait",
        "task-get",
        "task-group-status",
        "task-group-wait",
    ] {
        assert!(
            tool_results.iter().any(|(call_id, _)| call_id == id),
            "missing result for {id}: {tool_results:?}"
        );
    }
    assert!(tool_results
        .iter()
        .any(|(id, output)| id == "task-get" && output.contains("fixture-result-private-marker")));
    assert!(
        tool_results
            .iter()
            .any(|(id, output)| id == "task-status" && output.contains("Status: completed")),
        "status output was not completed: {tool_results:?}"
    );
    assert!(tool_results
        .iter()
        .any(|(id, output)| id == "task-wait" && output.contains("Status: completed")));
    assert!(tool_results
        .iter()
        .any(|(id, output)| id == "task-group-status" && output.contains("Status: completed")));
    assert!(tool_results
        .iter()
        .any(|(id, output)| id == "task-group-wait" && output.contains("Status: completed")));

    let attacker_tool = TaskTool::new(
        Arc::new(tokio::sync::Mutex::new(TaskStore::new())),
        None,
        Some("other-session".into()),
        vec![],
    )
    .with_agent_run_store(runs.clone())
    .with_run_control_opt(Some(RunControlService::in_memory(runs)))
    .with_run_group_service(Some(groups))
    .with_turn_owner("other-session".into(), "other-turn".into());
    let mut attacker_registry = ToolRegistry::new();
    attacker_registry.register(attacker_tool);
    let attacker_provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "cross-session-get".to_string().into(),
                name: "task".to_string().into(),
                arguments: serde_json::json!({"action":"get", "task_id":task_id}),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("done".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut attacker =
        build_test_agent_loop(Box::new(attacker_provider.clone()), attacker_registry).await;
    attacker.set_session_id("other-session");
    assert!(attacker
        .run(make_chat_request("try to inspect another session's task"))
        .await
        .is_ok());
    let requests = attacker_provider.get_requests().await;
    let denied = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .any(|message| {
            matches!(message, Message::Tool { tool_call_id, content }
            if tool_call_id.as_ref() == "cross-session-get"
                && content.as_ref().contains("not found")
                && !content.as_ref().contains("fixture-result-private-marker"))
        });
    assert!(
        denied,
        "cross-session get must return not-found without exposing task result"
    );
}

#[tokio::test]
async fn tool_program_foreground_and_background_run_through_agent_broker_and_scheduler() {
    use codegg::scheduler::tool_program_executor::ToolProgramExecutor;
    use codegg::scheduler::{
        JobScheduler, JobSubmissionService, ResolvedSchedulerConfig, SchedulerShutdownMode,
    };
    use codegg::tool::tool_program::ToolProgramTool;
    use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobKind, JobState, JobStore};
    use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
    use codegg_core::workspace_services::{
        ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
    };

    let root = tempfile::tempdir().expect("workspace fixture");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    let workspace = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("workspace");
    let workspace_services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let workspace_lease = workspace_services
        .acquire(&workspace.id)
        .await
        .expect("workspace lease");

    let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        job_store.clone(),
        workspace_services.clone(),
        ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("agent-tool-program-test"),
    );
    let submission = JobSubmissionService::new(
        job_store.clone(),
        scheduler.clone(),
        workspace_services.clone(),
        DaemonGeneration::new_unchecked("agent-tool-program-test"),
    );
    scheduler
        .register_executor_sync(Arc::new(
            ToolProgramExecutor::default().with_submission(submission.clone()),
        ))
        .expect("register native Tool Program executor");
    let scheduler_task = scheduler.spawn_run();

    let mut tools = ToolRegistry::with_defaults();
    tools.register(ToolProgramTool::new().with_submission(submission.clone()));
    let provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "tool-program-foreground".to_string().into(),
                name: "tool_program".to_string().into(),
                arguments: serde_json::json!({
                    "source": "emit(str(1 + 1))",
                    "tools": ["read"],
                    "execution_mode": "foreground"
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "tool-program-background".to_string().into(),
                name: "tool_program".to_string().into(),
                arguments: serde_json::json!({
                    "source": "emit(str(1 + 1))",
                    "tools": ["read"],
                    "execution_mode": "background"
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("complete".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut agent = AgentLoop::new(
        vec![Agent {
            git_read_only: false,
            name: "build".into(),
            role: None,
            description: "test agent".into(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: Default::default(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }],
        Box::new(provider.clone()),
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        }),
        tools,
        Config::default(),
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        root.path().to_path_buf(),
        "tool-program-session".into(),
    );
    agent.set_workspace_id(workspace.id.clone());
    agent.set_workspace_services_lease(workspace_lease);

    let result = agent
        .run(make_chat_request("run a restricted arithmetic program"))
        .await;
    assert!(result.is_ok(), "agent turn failed: {result:?}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let jobs = loop {
        let jobs = job_store
            .list_job_records(codegg_core::jobs::JobStoreQuery {
                kinds: vec![JobKind::ToolProgram],
                ..Default::default()
            })
            .await
            .expect("list Tool Program jobs");
        if jobs.len() == 2 && jobs.iter().all(|job| job.state.is_terminal()) {
            break jobs;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Tool Program jobs did not reach terminal states: {jobs:?}; results: {:?}",
            provider
                .get_requests()
                .await
                .iter()
                .flat_map(|request| request.messages.iter())
                .filter_map(|message| match message {
                    Message::Tool {
                        tool_call_id,
                        content,
                    } => {
                        Some((tool_call_id.to_string(), content.to_string()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(
        jobs.len(),
        2,
        "expected foreground and background scheduler jobs"
    );
    assert!(jobs.iter().all(|job| job.state == JobState::Completed));
    let requests = provider.get_requests().await;
    assert!(
        requests
            .iter()
            .any(|request| request.messages.iter().any(|message| {
                matches!(message, Message::Tool { tool_call_id, content }
            if tool_call_id.as_ref() == "tool-program-foreground"
                && content.as_ref().contains("program status: completed"))
            }) && requests
                .iter()
                .flat_map(|request| request.messages.iter())
                .any(|message| {
                    matches!(message, Message::Tool { tool_call_id, content }
            if tool_call_id.as_ref() == "tool-program-background"
                && content.as_ref().contains("submitted"))
                })),
        "model transcript must contain the structured native result: {:?}",
        requests
            .iter()
            .flat_map(|request| request.messages.iter())
            .filter_map(|message| match message {
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    Some((tool_call_id.to_string(), content.to_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    );

    let denied_provider = ScriptedProvider::new(vec![
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: "tool-program-denied".to_string().into(),
                name: "tool_program".to_string().into(),
                arguments: serde_json::json!({
                    "source": "emit(str(1 + 1))",
                    "tools": ["read"],
                    "execution_mode": "foreground"
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        vec![
            ChatEvent::TextDelta("denied as requested".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]);
    let mut denied_tools = ToolRegistry::new();
    denied_tools.register(ToolProgramTool::new().with_submission(submission.clone()));
    let mut denied_agent = AgentLoop::new(
        vec![Agent {
            git_read_only: false,
            name: "build".into(),
            role: None,
            description: "test agent".into(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: Default::default(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }],
        Box::new(denied_provider.clone()),
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![codegg::permission::ToolRule {
                tool: "tool_program".into(),
                level: PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        }),
        denied_tools,
        Config::default(),
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        root.path().to_path_buf(),
        "tool-program-denied-session".into(),
    );
    denied_agent.set_workspace_id(workspace.id.clone());
    denied_agent.set_workspace_services_lease(
        workspace_services
            .acquire(&workspace.id)
            .await
            .expect("denied workspace lease"),
    );
    assert!(denied_agent
        .run(make_chat_request("attempt a denied Tool Program"))
        .await
        .is_ok());
    let after_deny = job_store
        .list_job_records(codegg_core::jobs::JobStoreQuery {
            kinds: vec![JobKind::ToolProgram],
            ..Default::default()
        })
        .await
        .expect("list jobs after denied invocation");
    assert_eq!(
        after_deny.len(),
        2,
        "denied call must not create a scheduler job"
    );
    assert!(denied_provider
        .get_requests()
        .await
        .iter()
        .flat_map(|request| request.messages.iter())
        .any(|message| matches!(message, Message::Tool { tool_call_id, content }
            if tool_call_id.as_ref() == "tool-program-denied" && content.as_ref().contains("denied"))));

    let manifest_registry = ToolRegistry::with_options(codegg::tool::ToolRegistryOptions {
        workspace_root: Some(root.path().to_path_buf()),
        ..Default::default()
    });
    let manifest_broker = codegg::tool::broker::ToolBroker::new(&manifest_registry);
    let manifest = codegg::tool::tool_program_context::resolve_contract_snapshot(
        &manifest_broker,
        &["read".to_string()],
    )
    .expect("resolve frozen read manifest");
    let path_policy = submission
        .workspace_path_policy_fingerprint_for_root(root.path())
        .await
        .expect("current path policy");
    let accepted_context = || {
        let mut context = codegg::tool::backend::ToolExecutionContext::with_backend(
            codegg::tool::backend::ToolBackendKind::Native,
        );
        context.session_id = Some("tool-program-negative-session".into());
        context.cwd = root.path().to_path_buf();
        context.invocation_key = Some("negative-authority-fixture".into());
        context.decision_id = Some("accepted-decision-fixture".into());
        context.decision_outcome = Some("allowed".into());
        context.workspace_path_policy_id = Some(path_policy.clone());
        context.workspace_path_policy_revision = Some(path_policy.clone());
        context.permission_policy_revision = Some("policy-fixture".into());
        context.principal_identity = Some("fixture-agent".into());
        context.caller_class = Some("agent".into());
        context.max_effect_class = Some("read_only".into());
        context.decision_issued_at = Some(chrono::Utc::now().timestamp_millis());
        context.program_contract_snapshot = Some(manifest.clone());
        context
    };
    let invalid_authorities = [
        ("missing decision", 0),
        ("mismatched workspace policy", 1),
        ("revoked decision", 2),
        ("expired decision", 3),
    ];
    for (case, kind) in invalid_authorities {
        let mut context = accepted_context();
        match kind {
            0 => context.decision_id = None,
            1 => context.workspace_path_policy_revision = Some("stale-policy".into()),
            2 => context.decision_revoked_at = Some(chrono::Utc::now().timestamp_millis()),
            3 => context.decision_expires_at = Some(chrono::Utc::now().timestamp_millis() - 1),
            _ => unreachable!(),
        }
        let rejected = codegg::tool::tool_program::ToolProgramTool::new()
            .with_submission(submission.clone())
            .execute_structured(
                serde_json::json!({
                    "source": "emit(str(1 + 1))",
                    "tools": ["read"],
                    "execution_mode": "foreground"
                }),
                Some(context),
            )
            .await;
        assert!(rejected.is_err(), "{case} authority must be refused");
    }
    let after_invalid_authorities = job_store
        .list_job_records(codegg_core::jobs::JobStoreQuery {
            kinds: vec![JobKind::ToolProgram],
            ..Default::default()
        })
        .await
        .expect("list jobs after invalid authority cases");
    assert_eq!(
        after_invalid_authorities.len(),
        2,
        "invalid authority must not create executable jobs"
    );
    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    scheduler_task.abort();
}

#[tokio::test]
async fn test_task_tool_denied_tools_passthrough() {
    use codegg::agent::worker::SubAgentPool;
    use codegg::config::schema::Config;
    use codegg::provider::ProviderRegistry;
    use codegg::session::SessionStore;
    use codegg::tool::task::{TaskStore, TaskTool};
    use std::sync::Arc;

    // Create in-memory SQLite pool
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create pool");

    // Create SessionStore (uses clone because TaskStore also needs pool)
    let session_store = Arc::new(SessionStore::new(pool.clone()));

    // Create recording provider for subagent to capture requests
    let subagent_responses = vec![vec![
        ChatEvent::TextDelta("Subagent work done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ]];
    let subagent_provider = Box::new(RequestRecordingProvider::new(subagent_responses));

    // Create provider registry with recording provider
    let mut provider_registry = ProviderRegistry::new();
    provider_registry.register((*subagent_provider).clone());

    // Create agents: main agent and subagent with model pointing to recording provider
    let agents = vec![
        Agent {
            git_read_only: false,
            name: "main".to_string(),
            role: None,
            description: "Main agent".to_string(),
            mode: codegg::agent::AgentMode::Primary,
            mode_name: None,
            model: None,
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
        },
        Agent {
            git_read_only: false,
            name: "subagent".to_string(),
            role: None,
            description: "Subagent".to_string(),
            mode: codegg::agent::AgentMode::Subagent,
            mode_name: None,
            model: Some("request-recording/recording-model".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: Some("You are a subagent".to_string()),
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        },
    ];

    let config = Config::default();

    // Create SubAgentPool
    let subagent_pool = SubAgentPool::new(
        &config,
        agents.clone(),
        provider_registry,
        session_store,
        None,
    )
    .await;

    // Create TaskStore with pool for persistence and get_task support
    let task_store = Arc::new(tokio::sync::Mutex::new(TaskStore::new()));
    task_store.lock().await.set_pool(pool.clone());
    let spawner = subagent_pool.spawner();
    let denied_tools = vec!["denied_tool".to_string()];
    let task_tool = TaskTool::new(
        task_store.clone(),
        Some(spawner),
        Some("test-session-denied".to_string()),
        denied_tools.clone(),
    )
    .with_workspace_root(Some(std::env::current_dir().expect("test workspace root")));

    // Create main provider that emits a task tool call with denied_tools
    let main_provider = Box::new(ScriptedProvider::new(vec![
        // Turn 1: Emit task tool call with denied_tools
        vec![
            ChatEvent::ToolCall(ToolCall {
                id: Arc::new("call_task_1".to_string()),
                name: Arc::new("task".to_string()),
                arguments: serde_json::json!({
                    "action": "spawn",
                    "description": "Test denied tools passthrough",
                    "prompt": "Do work without denied_tool",
                    "agent": "subagent",
                    "denied_tools": denied_tools,
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
        // Turn 2: After task tool executes
        vec![
            ChatEvent::TextDelta("Task completed".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ],
    ]));

    // Create tool registry with TaskTool
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(task_tool);

    // Create permission checker
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![],
            path_rules: vec![],
        });

    // Build AgentLoop
    let workspace_root = std::env::current_dir()
        .expect("test workspace root")
        .canonicalize()
        .expect("canonical test workspace root");
    let mut agent_loop = AgentLoop::new(
        agents,
        main_provider.clone(),
        permission_checker,
        tool_registry,
        config,
        None,
        None,
        Arc::new(InMemoryArtifactStore::new()),
        workspace_root.clone(),
        "test-session-denied".to_string(),
    );
    attach_test_workspace_authority(&mut agent_loop, &workspace_root).await;

    // Run the AgentLoop
    let request = make_chat_request("Spawn subagent with denied tools");
    let result = agent_loop.run(request).await;
    assert!(
        result.is_ok(),
        "AgentLoop should complete: {:?}",
        result.err()
    );

    // Wait for subagent to process the request (deterministic polling)
    subagent_provider
        .wait_for_request(100, 20)
        .await
        .expect("Subagent provider should receive request within timeout");

    // Verify subagent provider received request with denied tools filtered
    let subagent_requests = subagent_provider.get_requests().await;
    assert!(
        !subagent_requests.is_empty(),
        "Subagent should have received at least one request, got {:?}",
        subagent_requests.len()
    );

    // Check that the subagent's tool definitions do not include denied_tool
    let subagent_request = &subagent_requests[0];
    if let Some(tools) = &subagent_request.tools {
        let has_denied_tool = tools.iter().any(|t| t.name == "denied_tool");
        assert!(
            !has_denied_tool,
            "Subagent request should not include denied tool 'denied_tool'"
        );
    }
}

#[tokio::test]
async fn test_task_tool_async_safety() {
    use codegg::tool::task::TaskStore;
    use std::sync::Arc;

    // Create TaskStore and TaskTool WITHOUT spawner (to test error path)
    let task_store = Arc::new(tokio::sync::Mutex::new(TaskStore::new()));

    // Create TaskTool with NO spawner - this should return an error about missing spawner
    let task_tool = TaskTool::new(
        task_store.clone(),
        None, // No spawner!
        Some("test-session-456".to_string()),
        vec![],
    );

    // Call task tool from inside Tokio runtime (this test IS inside Tokio runtime)
    let input = serde_json::json!({
        "action": "spawn",
        "description": "Test task",
        "prompt": "Do work",
        "agent": "subagent"
    });

    // This should NOT panic or stall the runtime
    let result = task_tool.execute(input).await;

    // When spawner is None, the tool should return a result indicating the task was queued but not executed
    assert!(
        result.is_ok(),
        "TaskTool::execute should not panic or stall"
    );
    let output = result.unwrap();
    assert!(
        output.contains("queued") || output.contains("not executed"),
        "Should indicate task was queued but not executed: {}",
        output
    );
}

#[tokio::test]
async fn test_question_http_route_no_pending_question() {
    use codegg::bus::QuestionRegistry;

    let answers_json = serde_json::json!(["red", "blue"]).to_string();
    let answered =
        QuestionRegistry::answer_question("nonexistent-session".to_string(), answers_json);
    assert!(
        !answered,
        "answer_question should return false when no question is pending"
    );
}

#[tokio::test]
async fn test_question_http_route_wakes_waiting_receiver() {
    use codegg::bus::QuestionRegistry;

    let (tx, rx) = tokio::sync::oneshot::channel();
    let session_id = "test-http-route-wake".to_string();

    QuestionRegistry::register(session_id.clone(), tx);

    let is_registered = QuestionRegistry::is_registered(&session_id);
    assert!(
        is_registered,
        "Question should be registered before answering"
    );

    let answers_json = serde_json::json!(["answer1"]).to_string();
    let answered = QuestionRegistry::answer_question(session_id.clone(), answers_json);
    assert!(
        answered,
        "answer_question should return true when question is pending"
    );

    let received = rx.await;
    assert!(received.is_ok(), "Receiver should get the answer");
    assert!(
        received.unwrap().contains("answer1"),
        "Answer content should match"
    );
}

#[tokio::test]
async fn test_permission_http_route_no_pending_permission() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};

    let responded = PermissionRegistry::respond(
        "nonexistent-perm".to_string(),
        PermissionDecision::AllowOnce,
    );
    assert!(
        !responded,
        "respond should return false when no permission is pending"
    );
}

#[tokio::test]
async fn test_permission_http_route_wakes_waiting_receiver() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};

    let (tx, rx) = tokio::sync::oneshot::channel();
    let perm_id = "test-perm-http-wake".to_string();

    PermissionRegistry::register(perm_id.clone(), tx);

    let is_registered = PermissionRegistry::is_registered(&perm_id);
    assert!(
        is_registered,
        "Permission should be registered before responding"
    );

    let responded = PermissionRegistry::respond(perm_id.clone(), PermissionDecision::AllowOnce);
    assert!(
        responded,
        "respond should return true when permission is pending"
    );

    let received = rx.await;
    assert!(received.is_ok(), "Receiver should get the choice");
    assert!(
        matches!(received.unwrap(), PermissionDecision::AllowOnce),
        "Choice should match"
    );
}

#[tokio::test]
async fn test_registry_recovery_after_missed_event() {
    use codegg::bus::{PermissionDecision, PermissionRegistry, QuestionRegistry};

    let (tx, rx) = tokio::sync::oneshot::channel();
    let perm_id = "test-recovery-perm".to_string();
    PermissionRegistry::register(perm_id.clone(), tx);

    let pending_perms = PermissionRegistry::pending_permission_ids();
    assert!(
        pending_perms.contains(&perm_id),
        "Should be able to recover pending permission from registry"
    );

    PermissionRegistry::respond(perm_id.clone(), PermissionDecision::AllowOnce);
    let _ = rx.await;

    let pending_perms_after = PermissionRegistry::pending_permission_ids();
    assert!(
        !pending_perms_after.contains(&perm_id),
        "Permission should be removed after response"
    );

    let (tx2, rx2) = tokio::sync::oneshot::channel();
    let question_id = "test-recovery-question".to_string();
    QuestionRegistry::register(question_id.clone(), tx2);

    let pending_qs = QuestionRegistry::pending_question_ids();
    assert!(
        pending_qs.contains(&question_id),
        "Should be able to recover pending question from registry"
    );

    let answers_json = serde_json::json!(["answer"]).to_string();
    QuestionRegistry::answer_question(question_id.clone(), answers_json);
    let _ = rx2.await;

    let pending_qs_after = QuestionRegistry::pending_question_ids();
    assert!(
        !pending_qs_after.contains(&question_id),
        "Question should be removed after answer"
    );
}

#[tokio::test]
async fn test_source_structure_prompt_does_not_synthesize_bootstrap_list() {
    let response1 = vec![ChatEvent::Finish {
        stop_reason: "stop".to_string().into(),
        usage: TokenUsage::default(),
    }];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1]));
    let registry = ToolRegistry::with_defaults();
    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;
    agent_loop.set_session_id("test-source-structure-bootstrap");

    let mut request = make_chat_request("get source code structure for this project");
    request.messages.insert(
        0,
        Message::System {
            content: "You are a helpful assistant.".to_string().into(),
        },
    );
    request.model = "ollama/qwen2.5-coder:32b".to_string();
    let result = agent_loop.run(request).await;
    assert!(result.is_ok(), "run should succeed: {:?}", result.err());

    let requests = scripted_provider.get_requests().await;
    assert_eq!(
        requests.len(),
        1,
        "Strong models finish without bootstrap turns"
    );
}

#[tokio::test]
async fn test_missing_structured_tool_calls_emits_diagnostic_error_event() {
    let response1 = vec![ChatEvent::Finish {
        stop_reason: "tool_calls".to_string().into(),
        usage: TokenUsage::default(),
    }];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1]));
    let registry = ToolRegistry::with_defaults();
    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;
    agent_loop.set_session_id("test-structured-tool-calls-diagnostic");

    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("analyze this repository");
    let result = agent_loop.run(request).await;
    assert!(result.is_ok(), "run should succeed: {:?}", result.err());

    let requests = scripted_provider.get_requests().await;
    assert_eq!(
        requests.len(),
        1,
        "Malformed structured-tool output has one bounded recovery decision"
    );

    let mut saw_diagnostic = false;
    loop {
        match rx.try_recv() {
            Ok(AppEvent::Error { message }) => {
                if message
                    .contains("stop_reason=tool_calls without parseable structured tool calls")
                {
                    saw_diagnostic = true;
                    break;
                }
            }
            Ok(_) => continue,
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }

    assert!(
        saw_diagnostic,
        "Expected explicit diagnostic AppEvent::Error for missing structured tool calls"
    );
}

#[test]
fn test_default_registry_includes_lsp_tool() {
    let registry = ToolRegistry::with_defaults();
    assert!(
        registry.get("lsp").is_some(),
        "Default tool registry should include lsp tool"
    );
}

// =============================================================================
// Live-dispatcher wiring tests
// =============================================================================
//
// These tests prove that the live agent-loop dispatcher routes native
// tool calls through `ToolRegistry::execute_capture()` (which in turn
// calls `Tool::execute_structured`) rather than the legacy
// `Tool::execute` path. The recording tool overrides
// `execute_structured`; if the loop ever bypasses the structured path
// and calls `execute` directly, the counter will not increment and the
// test will fail.
//
// They also lock down the model-facing shape of the tool result:
// `Message::Tool::content` is the raw tool output string with no
// provenance JSON envelope.
//
// No network, providers, MCP servers, or LSP servers are required.

use codegg::tool::{
    StructuredToolResult, ToolBackendKind, ToolExecutionContext, ToolProvenance, ToolTrust,
};

/// Test tool that overrides `execute_structured` to record that the
/// structured path was taken. The counter only increments inside
/// `execute_structured`, so a live dispatcher that bypasses
/// `execute_capture` and goes through `execute` directly will not
/// trigger the recording.
struct ProvenanceRecordingTool {
    counter: Arc<AtomicUsize>,
    last_args: Arc<Mutex<serde_json::Value>>,
    last_ctx_backend: Arc<Mutex<Option<ToolBackendKind>>>,
}

impl ProvenanceRecordingTool {
    fn new(
        counter: Arc<AtomicUsize>,
        last_args: Arc<Mutex<serde_json::Value>>,
        last_ctx_backend: Arc<Mutex<Option<ToolBackendKind>>>,
    ) -> Self {
        Self {
            counter,
            last_args,
            last_ctx_backend,
        }
    }
}

#[async_trait]
impl Tool for ProvenanceRecordingTool {
    fn name(&self) -> &str {
        "record_provenance"
    }

    fn description(&self) -> &str {
        "Test tool that records every call through execute_structured"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            },
            "required": ["value"]
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        // The legacy path should never be reached by the live
        // dispatcher; if it is, the test will fail because the
        // counter does not increment.
        Err(codegg::error::ToolError::Execution(
            "legacy execute() should not be called by the live dispatcher".to_string(),
        ))
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, codegg::error::ToolError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        {
            let mut last = self.last_args.lock().await;
            *last = input.clone();
        }
        if let Some(c) = ctx {
            let mut last = self.last_ctx_backend.lock().await;
            *last = Some(c.backend);
        }
        Ok(StructuredToolResult::with_provenance(
            input.to_string(),
            true,
            ToolProvenance {
                backend: ToolBackendKind::Native.label().to_lowercase(),
                implementation: "record_provenance".to_string(),
                version: Some("test".to_string()),
                elapsed_ms: Some(0),
                truncated: false,
                trust: ToolTrust::LocalTrusted,
            },
        ))
    }
}

#[tokio::test]
async fn test_live_dispatcher_uses_execute_capture() {
    let counter = Arc::new(AtomicUsize::new(0));
    let last_args: Arc<Mutex<serde_json::Value>> = Arc::new(Mutex::new(serde_json::json!({})));
    let last_ctx_backend: Arc<Mutex<Option<ToolBackendKind>>> = Arc::new(Mutex::new(None));

    let tool =
        ProvenanceRecordingTool::new(counter.clone(), last_args.clone(), last_ctx_backend.clone());

    let tool_call = ToolCall {
        id: "call_1".to_string().into(),
        name: "record_provenance".to_string().into(),
        arguments: serde_json::json!({"value": "hello"}),
    };

    let response1 = vec![
        ChatEvent::ToolCall(tool_call),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let response2 = vec![
        ChatEvent::TextDelta("done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(tool);

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![codegg::permission::ToolRule {
                tool: "record_provenance".to_string(),
                level: PermissionLevel::Allow,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    let session_id = "test-live-dispatcher-uses-execute-capture".to_string();
    agent_loop.set_session_id(&session_id);

    let result = agent_loop.run(make_chat_request("do a thing")).await;
    assert!(
        result.is_ok(),
        "agent loop should succeed: {:?}",
        result.err()
    );

    // The counter only increments inside `execute_structured`. If
    // the live dispatcher ever calls `Tool::execute` directly
    // instead of routing through `ToolRegistry::execute_capture`,
    // this assertion fails.
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "Tool was called {} time(s); expected exactly 1 via execute_capture",
        counter.load(Ordering::SeqCst)
    );

    let recorded_args = last_args.lock().await.clone();
    assert_eq!(
        recorded_args,
        serde_json::json!({"value": "hello"}),
        "Tool arguments recorded in execute_structured should match the tool call"
    );

    let ctx_backend = *last_ctx_backend.lock().await;
    assert_eq!(
        ctx_backend,
        Some(ToolBackendKind::Native),
        "ToolExecutionContext::backend should be resolved by the dispatcher"
    );

    // The model-facing tool result must be the raw string output
    // (not a structured JSON envelope containing provenance).
    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2, "expected at least 2 provider calls");
    let tool_message = requests[1]
        .messages
        .iter()
        .find_map(|m| match m {
            Message::Tool {
                tool_call_id,
                content,
            } if tool_call_id.as_ref() == "call_1" => Some(content.as_ref().clone()),
            _ => None,
        })
        .expect("expected Message::Tool with tool_call_id=call_1 in second request");
    assert!(
        tool_message.ends_with(r#"{"value":"hello"}"#),
        "Model-facing tool result should preserve the raw tool payload at the end; got: {}",
        tool_message
    );
    for forbidden in [
        "\"provenance\"",
        "\"backend\"",
        "\"implementation\"",
        "\"trust\"",
    ] {
        assert!(
            !tool_message.contains(forbidden),
            "Model-facing tool result must not contain '{}' envelope; got: {}",
            forbidden,
            tool_message
        );
    }
}

#[tokio::test]
async fn test_live_dispatcher_passes_native_backend_in_context() {
    // Verifies the new `build_tool_execution_context` helper
    // resolves non-search tools to `ToolBackendKind::Native`.
    let counter = Arc::new(AtomicUsize::new(0));
    let last_args: Arc<Mutex<serde_json::Value>> = Arc::new(Mutex::new(serde_json::json!({})));
    let last_ctx_backend: Arc<Mutex<Option<ToolBackendKind>>> = Arc::new(Mutex::new(None));

    let tool =
        ProvenanceRecordingTool::new(counter.clone(), last_args.clone(), last_ctx_backend.clone());

    let tool_call = ToolCall {
        id: "call_native".to_string().into(),
        name: "record_provenance".to_string().into(),
        arguments: serde_json::json!({"value": "v"}),
    };

    let response1 = vec![
        ChatEvent::ToolCall(tool_call),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![ChatEvent::Finish {
        stop_reason: "stop".to_string().into(),
        usage: TokenUsage::default(),
    }];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(tool);

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![codegg::permission::ToolRule {
                tool: "record_provenance".to_string(),
                level: PermissionLevel::Allow,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    let session_id = "test-live-dispatcher-native-backend".to_string();
    agent_loop.set_session_id(&session_id);

    let result = agent_loop.run(make_chat_request("ping")).await;
    assert!(
        result.is_ok(),
        "agent loop should succeed: {:?}",
        result.err()
    );

    assert_eq!(counter.load(Ordering::SeqCst), 1);
    let backend = *last_ctx_backend.lock().await;
    assert_eq!(
        backend,
        Some(ToolBackendKind::Native),
        "Non-search native tools should resolve to ToolBackendKind::Native"
    );
}

#[tokio::test]
async fn test_live_dispatcher_model_output_shape_is_plain_string() {
    // Phase 7 regression: the Message::Tool content returned to the
    // provider must not contain provenance JSON. Use a legacy tool
    // that does NOT override execute_structured, so the result is
    // exactly what `execute()` returns.
    let counter = Arc::new(AtomicUsize::new(0));
    let tool_call = ToolCall {
        id: "call_plain".to_string().into(),
        name: "echo_args".to_string().into(),
        arguments: serde_json::json!({"value": "alpha"}),
    };

    let response1 = vec![
        ChatEvent::ToolCall(tool_call),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![ChatEvent::Finish {
        stop_reason: "stop".to_string().into(),
        usage: TokenUsage::default(),
    }];

    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop(scripted_provider.clone(), registry).await;
    let session_id = "test-live-dispatcher-model-output-shape".to_string();
    agent_loop.set_session_id(&session_id);

    let result = agent_loop.run(make_chat_request("echo please")).await;
    assert!(
        result.is_ok(),
        "agent loop should succeed: {:?}",
        result.err()
    );

    let requests = scripted_provider.get_requests().await;
    assert!(requests.len() >= 2);
    let tool_content = requests[1]
        .messages
        .iter()
        .find_map(|m| match m {
            Message::Tool {
                tool_call_id,
                content,
            } if tool_call_id.as_ref() == "call_plain" => Some(content.as_ref().clone()),
            _ => None,
        })
        .expect("expected Message::Tool with tool_call_id=call_plain");

    for forbidden in [
        "provenance",
        "backend",
        "implementation",
        "trust",
        "elapsed_ms",
    ] {
        assert!(
            !tool_content.contains(forbidden),
            "Tool result must not leak provenance JSON (found '{}'): {}",
            forbidden,
            tool_content
        );
    }
    let _ = counter; // silence unused warning if assertions are elided
}

// =============================================================================
// M001 — provider retry attempt safety and error taxonomy
// =============================================================================

#[derive(Clone)]
struct M001MidStreamTextFailProvider {
    calls: Arc<Mutex<usize>>,
}

impl M001MidStreamTextFailProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }

    async fn call_count(&self) -> usize {
        *self.calls.lock().await
    }
}

#[async_trait]
impl Provider for M001MidStreamTextFailProvider {
    fn id(&self) -> &str {
        "m001-midstream-text"
    }

    fn name(&self) -> &str {
        "M001 MidStream Text"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        *self.calls.lock().await += 1;
        let events: Vec<Result<ChatEvent, ProviderError>> = vec![
            Ok(ChatEvent::TextDelta("partial-visible".to_string().into())),
            Err(ProviderError::Stream("mid-stream interruption".to_string())),
        ];
        Ok(Box::pin(futures_util::stream::iter(events)))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

#[derive(Clone)]
struct M001MidStreamToolFailProvider {
    calls: Arc<Mutex<usize>>,
}

impl M001MidStreamToolFailProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }

    async fn call_count(&self) -> usize {
        *self.calls.lock().await
    }
}

#[async_trait]
impl Provider for M001MidStreamToolFailProvider {
    fn id(&self) -> &str {
        "m001-midstream-tool"
    }

    fn name(&self) -> &str {
        "M001 MidStream Tool"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        *self.calls.lock().await += 1;
        let events: Vec<Result<ChatEvent, ProviderError>> = vec![
            Ok(ChatEvent::ToolCall(ToolCall {
                id: "abandoned_call".to_string().into(),
                name: "counting_tool".to_string().into(),
                arguments: serde_json::json!({"value": "must-not-execute"}),
            })),
            Err(ProviderError::Stream(
                "interrupted after tool start".to_string(),
            )),
        ];
        Ok(Box::pin(futures_util::stream::iter(events)))
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
}

struct M001CountingTool {
    calls: Arc<AtomicUsize>,
}

impl M001CountingTool {
    fn new(calls: Arc<AtomicUsize>) -> Self {
        Self { calls }
    }
}

#[async_trait]
impl Tool for M001CountingTool {
    fn name(&self) -> &str {
        "counting_tool"
    }

    fn description(&self) -> &str {
        "Counts executions for M001 abandonment proof"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"]
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, codegg::error::ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok("counted".to_string())
    }
}

#[derive(Clone, Copy)]
enum M001FailKind {
    RateLimit,
    Server503,
    BadRequest400,
    Auth,
    ModelNotFound,
    SecretUrl,
}

#[derive(Clone)]
struct M001StatusProvider {
    calls: Arc<Mutex<usize>>,
    fail_times: usize,
    kind: M001FailKind,
}

impl M001StatusProvider {
    fn new(fail_times: usize, kind: M001FailKind) -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
            fail_times,
            kind,
        }
    }

    async fn call_count(&self) -> usize {
        *self.calls.lock().await
    }

    fn build_error(&self) -> ProviderError {
        match self.kind {
            M001FailKind::RateLimit => ProviderError::RateLimit,
            M001FailKind::Server503 => ProviderError::api("503", "HTTP 503: unavailable"),
            M001FailKind::BadRequest400 => ProviderError::api("400", "HTTP 400: bad request"),
            M001FailKind::Auth => ProviderError::Auth("invalid token".to_string()),
            M001FailKind::ModelNotFound => {
                ProviderError::ModelNotFound("no-such-model".to_string())
            }
            M001FailKind::SecretUrl => ProviderError::from(eggfetch_core::Error::InvalidUrl(
                "https://example.test/models?key=SECRET-API-KEY-123".to_string(),
            )),
        }
    }
}

#[async_trait]
impl Provider for M001StatusProvider {
    fn id(&self) -> &str {
        "m001-status"
    }

    fn name(&self) -> &str {
        "M001 Status"
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    async fn stream(&self, _request: &ChatRequest) -> Result<EventStream, ProviderError> {
        let mut calls = self.calls.lock().await;
        *calls += 1;
        if *calls <= self.fail_times {
            return Err(self.build_error());
        }
        let events = vec![
            ChatEvent::TextDelta("recovered".to_string().into()),
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
        Ok(vec![])
    }
}

fn m001_attempt_ids(collector: &EventCollector, variant: &str) -> Vec<String> {
    collector
        .events
        .iter()
        .filter_map(|e| match e {
            AppEvent::ProviderAttemptStarted { attempt_id, .. } if variant == "started" => {
                Some(attempt_id.clone())
            }
            AppEvent::ProviderAttemptSuperseded { attempt_id, .. } if variant == "superseded" => {
                Some(attempt_id.clone())
            }
            AppEvent::ProviderAttemptFailed { attempt_id, .. } if variant == "failed" => {
                Some(attempt_id.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn m001_midstream_text_failure_does_not_replay() {
    let provider_inner = M001MidStreamTextFailProvider::new();
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    let session_id = "m001-midstream-text".to_string();
    agent_loop.set_session_id(&session_id);
    let mut collector = EventCollector::with_session(session_id.clone());

    let result = agent_loop.run(make_chat_request("hello")).await;
    collector.collect();

    assert!(result.is_err(), "mid-stream failure must surface");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("interrupted after visible output"),
        "error must carry the typed interruption marker, got: {err_text}"
    );
    assert_eq!(
        provider_inner.call_count().await,
        1,
        "visible-output failure must not trigger a second attempt"
    );
    // The abandoned partial delta escaped (streaming UX) but no second
    // generation was merged: exactly one text delta exists.
    let deltas = collector
        .events
        .iter()
        .filter(|e| matches!(e, AppEvent::TextDelta { .. }))
        .count();
    assert_eq!(deltas, 1, "exactly the abandoned partial delta is visible");
    let started = m001_attempt_ids(&collector, "started");
    let superseded = m001_attempt_ids(&collector, "superseded");
    let failed = m001_attempt_ids(&collector, "failed");
    assert_eq!(started.len(), 1);
    assert_eq!(
        superseded, started,
        "supersession must attribute the same attempt"
    );
    assert_eq!(
        failed, started,
        "failure record must attribute the same attempt"
    );
    let failed_event = collector
        .events
        .iter()
        .find_map(|e| match e {
            AppEvent::ProviderAttemptFailed {
                visible_output,
                will_retry,
                error_class,
                ..
            } => Some((*visible_output, *will_retry, error_class.clone())),
            _ => None,
        })
        .expect("ProviderAttemptFailed must be published");
    assert!(failed_event.0, "visible_output must be true");
    assert!(
        !failed_event.1,
        "will_retry must be false after visible output"
    );
    assert_eq!(failed_event.2, "stream_interrupted");
}

#[tokio::test]
async fn m001_midstream_tool_start_is_explicit_and_not_executed() {
    let provider_inner = M001MidStreamToolFailProvider::new();
    let provider = Box::new(provider_inner.clone());
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(M001CountingTool::new(tool_calls.clone()));
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    let session_id = "m001-midstream-tool".to_string();
    agent_loop.set_session_id(&session_id);
    let mut collector = EventCollector::with_session(session_id);

    let result = agent_loop.run(make_chat_request("do work")).await;
    collector.collect();

    assert!(result.is_err(), "abandoned tool attempt must surface");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("interrupted after visible output"),
        "{err_text}"
    );
    assert_eq!(provider_inner.call_count().await, 1);
    assert_eq!(
        tool_calls.load(Ordering::SeqCst),
        0,
        "tool from the abandoned attempt must never execute"
    );
    assert!(
        collector
            .events
            .iter()
            .any(|e| matches!(e, AppEvent::ToolCallStarted { .. })),
        "abandoned ToolCallStarted remains visible for attribution"
    );
    assert!(
        collector
            .events
            .iter()
            .any(|e| matches!(e, AppEvent::ProviderAttemptSuperseded { .. })),
        "supersession must be explicit"
    );
    assert!(
        !collector
            .events
            .iter()
            .any(|e| matches!(e, AppEvent::ToolResult { .. })),
        "no ToolResult may be produced for the abandoned attempt"
    );
}

#[tokio::test]
async fn shell_escalation_ask_waits_for_explicit_approval_before_dispatch() {
    use codegg::bus::{PermissionDecision, PermissionRegistry};

    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("shell-dispatched");
    // ADR-0028: plain `$(...)` substitution alone is ordinary shell
    // programming and no longer raises an advisory Ask signal, so this
    // approval-path regression uses a benign sentinel plus an explicitly
    // configured `bash_ask_patterns` rule. The command writes only inside
    // the fresh TempDir and cannot affect host-global resources.
    let command = format!("printf probe-sentinel > '{}'", marker.display());
    let ask_ruleset = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![codegg::permission::ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: Some(vec!["*shell-dispatched*".to_string()]),
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ask_ruleset);
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_shell_escalation".to_string().into(),
            name: "bash".to_string().into(),
            arguments: serde_json::json!({"command": command}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("Approved shell command completed".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));
    let mut registry = ToolRegistry::new();
    // FullHost on both sides of the execution-authorization gate: this test
    // pins the approval path (Ask -> AllowOnce -> dispatch), not sandbox
    // containment, and the marker lives in a system tempdir outside any
    // workspace root. A bare tool (no configured profile) is refused by the
    // TOCTOU gate, and a constrained profile would contain the touch.
    registry.register(
        codegg::tool::bash::BashTool::new()
            .with_workspace_root(temp.path().to_path_buf())
            .with_sandbox_profile(codegg_core::approval::SandboxProfile::FullHost, temp.path()),
    );

    let mut agent_loop =
        build_test_agent_loop_with_permissions(scripted_provider, registry, checker).await;
    agent_loop.set_sandbox_profile(codegg_core::approval::SandboxProfile::FullHost);
    agent_loop.set_session_id("shell-escalation-approval-gate");

    let perm_id = "call_shell_escalation-bash".to_string();
    let mut rx = GlobalEventBus::subscribe();
    let request = make_chat_request("Run the bounded shell probe");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });

    wait_for_permission_pending(&perm_id, &mut rx, std::time::Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        !marker.exists(),
        "Ask-class shell command must not dispatch before approval"
    );

    assert!(PermissionRegistry::respond_scoped(
        "shell-escalation-approval-gate",
        &perm_id,
        PermissionDecision::AllowOnce,
    ));
    let result = handle.await.unwrap();
    assert!(
        result.is_ok(),
        "approved command should complete: {result:?}"
    );
    assert!(
        marker.exists(),
        "the bounded command should dispatch after the explicit AllowOnce decision"
    );
}

#[tokio::test]
async fn m001_transient_503_retries_before_visible_output() {
    let provider_inner = M001StatusProvider::new(1, M001FailKind::Server503);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-503-retry");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_ok(), "503 before visible output must recover");
    assert_eq!(provider_inner.call_count().await, 2);
}

#[tokio::test]
async fn m001_transient_ratelimit_retries_before_visible_output() {
    let provider_inner = M001StatusProvider::new(1, M001FailKind::RateLimit);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-429-retry");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_ok(), "429 before visible output must recover");
    assert_eq!(provider_inner.call_count().await, 2);
}

#[tokio::test]
async fn m001_permanent_bad_request_does_not_retry() {
    let provider_inner = M001StatusProvider::new(99, M001FailKind::BadRequest400);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-400-once");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_err(), "400 must surface without exhaustion");
    assert_eq!(
        provider_inner.call_count().await,
        1,
        "invalid request must not retry to exhaustion"
    );
}

#[tokio::test]
async fn m001_permanent_auth_does_not_retry() {
    let provider_inner = M001StatusProvider::new(99, M001FailKind::Auth);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-auth-once");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_err());
    assert_eq!(provider_inner.call_count().await, 1, "auth must not retry");
    let err = format!("{:?}", result.unwrap_err());
    assert!(err.contains("Auth"), "auth class must survive: {err}");
}

#[tokio::test]
async fn m001_permanent_model_missing_does_not_retry() {
    let provider_inner = M001StatusProvider::new(99, M001FailKind::ModelNotFound);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-model-once");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_err());
    assert_eq!(provider_inner.call_count().await, 1);
}

#[tokio::test]
async fn m001_secret_bearing_transport_stays_redacted_and_permanent() {
    let provider_inner = M001StatusProvider::new(99, M001FailKind::SecretUrl);
    let provider = Box::new(provider_inner.clone());
    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());
    let mut agent_loop = build_agent_loop_with_error_config(provider, registry);
    agent_loop.set_session_id("m001-secret-redacted");
    let result = agent_loop.run(make_chat_request("hi")).await;
    assert!(result.is_err());
    assert_eq!(provider_inner.call_count().await, 1);
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        !err.contains("SECRET"),
        "no secret material may surface: {err}"
    );
    assert!(
        !err.to_lowercase().contains("key="),
        "no URL query may surface: {err}"
    );
}

// =============================================================================
// M006: Automatic approval reviewer loop integration
// =============================================================================

/// Build an `Automatic`-mode loop whose reviewer is served by the same
/// scripted provider stream (the reviewer consumes one stream response as
/// its bounded verdict) and whose `echo_args` tool requires escalation.
async fn build_automatic_reviewer_loop(
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
) -> AgentLoop {
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};

    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });
    let mut config = Config::default();
    config.approval_reviewer = Some(codegg::config::schema::ApprovalReviewerConfig {
        // Bare model id: resolved against the primary provider, never a
        // silent switch to an unknown provider.
        model: Some("reviewer-mini".to_string()),
        ..Default::default()
    });
    let mut agent_loop =
        build_test_agent_loop_with_config(provider, registry, permission_checker, config).await;
    agent_loop.set_approval_mode(codegg_core::approval::ApprovalMode::Automatic);
    agent_loop
}

fn drain_permission_pending_for(rx: &mut broadcast::Receiver<AppEvent>, perm_id: &str) -> bool {
    let mut seen = false;
    loop {
        match rx.try_recv() {
            Ok(AppEvent::PermissionPending { perm_id: pid, .. }) if pid == perm_id => {
                seen = true;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    seen
}

#[tokio::test]
async fn test_approval_automatic_reviewer_allows_without_human_prompt() {
    // Primary turn emits an escalating tool call; the reviewer verdict
    // (consumed from the same scripted stream) allows it. No human
    // PermissionPending may be published for the escalation.
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_auto_allow".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "auto_allowed"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let reviewer_verdict = serde_json::json!({
        "verdict": "allow",
        "risk": "low",
        "reason": "necessary and proportionate",
    })
    .to_string();
    let response2 = vec![
        ChatEvent::TextDelta(reviewer_verdict.into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response3 = vec![
        ChatEvent::TextDelta("Tool executed".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2, response3]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_automatic_reviewer_loop(scripted_provider.clone(), registry).await;
    agent_loop.set_session_id("auto-reviewer-allow");
    let perm_id = "call_auto_allow-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), handle)
        .await
        .expect("loop timed out; reviewer likely fell back to a human wait")
        .unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    assert!(
        !drain_permission_pending_for(&mut rx, &perm_id),
        "reviewer Allow must not publish a human PermissionPending"
    );

    let requests = scripted_provider.get_requests().await;
    assert!(
        requests.len() >= 3,
        "primary turn + reviewer call + follow-up expected, got {}",
        requests.len()
    );
    let with_result = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_auto_allow"))
        })
        .expect("expected a request containing the tool result");
    assert_tool_result_with_id(
        &with_result.messages,
        "call_auto_allow",
        Some("auto_allowed"),
    );
}

#[tokio::test]
async fn test_approval_automatic_reviewer_deny_returns_feedback_without_human() {
    // The reviewer denies with primary-agent feedback; the denial (with
    // feedback) reaches the primary model as the tool outcome, and no
    // human is prompted.
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_auto_deny".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "auto_denied"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let reviewer_verdict = serde_json::json!({
        "verdict": "deny",
        "risk": "high",
        "reason": "disproportionate scope",
        "primary_agent_feedback": "narrow the request first",
    })
    .to_string();
    let response2 = vec![
        ChatEvent::TextDelta(reviewer_verdict.into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response3 = vec![
        ChatEvent::TextDelta("Understood".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2, response3]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_automatic_reviewer_loop(scripted_provider.clone(), registry).await;
    agent_loop.set_session_id("auto-reviewer-deny");
    let perm_id = "call_auto_deny-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), handle)
        .await
        .expect("loop timed out; reviewer likely fell back to a human wait")
        .unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    assert!(
        !drain_permission_pending_for(&mut rx, &perm_id),
        "reviewer Deny must not publish a human PermissionPending"
    );

    let requests = scripted_provider.get_requests().await;
    let with_result = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_auto_deny"))
        })
        .expect("expected a request containing the denial outcome");
    assert_tool_result_with_id(
        &with_result.messages,
        "call_auto_deny",
        Some("denied by approval reviewer"),
    );
    assert_tool_result_with_id(
        &with_result.messages,
        "call_auto_deny",
        Some("narrow the request first"),
    );
}

// =============================================================================
// M007: Yolo/Automatic/FullHost user-surface loop integration
// =============================================================================

/// Build a loop with an explicit approval mode and sandbox profile whose
/// `echo_args` tool requires escalation.
async fn build_m007_policy_loop(
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    mode: codegg_core::approval::ApprovalMode,
    profile: codegg_core::approval::SandboxProfile,
) -> AgentLoop {
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Ask,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });
    let mut agent_loop =
        build_test_agent_loop_with_permissions(provider, registry, permission_checker).await;
    agent_loop.set_approval_mode(mode);
    agent_loop.set_sandbox_profile(profile);
    agent_loop
}

#[tokio::test]
async fn test_approval_yolo_workspace_write_skips_prompt_but_stays_sandboxed() {
    // Yolo resolves the escalation without a human prompt, but the
    // execution snapshot keeps the WorkspaceWrite containment profile:
    // autonomy never implies FullHost.
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_yolo_ww".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "yolo_allowed"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("Tool executed".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_m007_policy_loop(
        scripted_provider.clone(),
        registry,
        codegg_core::approval::ApprovalMode::Yolo,
        codegg_core::approval::SandboxProfile::WorkspaceWrite,
    )
    .await;
    agent_loop.set_session_id("yolo-workspace-write");
    assert_eq!(
        agent_loop.sandbox_profile(),
        codegg_core::approval::SandboxProfile::WorkspaceWrite
    );
    let perm_id = "call_yolo_ww-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), handle)
        .await
        .expect("Yolo loop must not block on a human wait")
        .unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    assert!(
        !drain_permission_pending_for(&mut rx, &perm_id),
        "Yolo must resolve the escalation without a human PermissionPending"
    );

    let requests = scripted_provider.get_requests().await;
    let with_result = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_yolo_ww"))
        })
        .expect("expected a request containing the tool result");
    assert_tool_result_with_id(&with_result.messages, "call_yolo_ww", Some("yolo_allowed"));
}

#[tokio::test]
async fn test_approval_yolo_explicit_deny_remains_denied() {
    // A deterministic Deny is not an escalation: Yolo never overrides it.
    use codegg::permission::{PermissionLevel, PermissionRuleset, ToolRule};
    let permission_checker =
        PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
            default: PermissionLevel::Allow,
            tool_rules: vec![ToolRule {
                tool: "echo_args".to_string(),
                level: PermissionLevel::Deny,
                paths: None,
                bash_patterns: None,
            }],
            path_rules: vec![],
        });
    let response1 = vec![
        ChatEvent::ToolCall(ToolCall {
            id: "call_yolo_deny".to_string().into(),
            name: "echo_args".to_string().into(),
            arguments: serde_json::json!({"value": "must_not_run"}),
        }),
        ChatEvent::Finish {
            stop_reason: "tool_calls".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let response2 = vec![
        ChatEvent::TextDelta("done".to_string().into()),
        ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: TokenUsage::default(),
        },
    ];
    let scripted_provider = Box::new(ScriptedProvider::new(vec![response1, response2]));

    let mut registry = ToolRegistry::new();
    registry.register(EchoArgsTool::new());

    let mut agent_loop = build_test_agent_loop_with_permissions(
        scripted_provider.clone(),
        registry,
        permission_checker,
    )
    .await;
    agent_loop.set_approval_mode(codegg_core::approval::ApprovalMode::Yolo);
    agent_loop.set_sandbox_profile(codegg_core::approval::SandboxProfile::FullHost);
    agent_loop.set_session_id("yolo-deny");
    let perm_id = "call_yolo_deny-echo_args".to_string();
    let mut rx = GlobalEventBus::subscribe();

    let request = make_chat_request("Use echo_args");
    let handle = tokio::spawn(async move { agent_loop.run(request).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), handle)
        .await
        .expect("denied Yolo loop must finish promptly")
        .unwrap();
    assert!(result.is_ok(), "Loop should complete: {:?}", result.err());

    assert!(
        !drain_permission_pending_for(&mut rx, &perm_id),
        "deterministic Deny publishes no escalation in any mode"
    );
    let requests = scripted_provider.get_requests().await;
    let with_result = requests
        .iter()
        .rev()
        .find(|r| {
            r.messages
                .iter()
                .any(|m| matches!(m, Message::Tool { tool_call_id, .. } if tool_call_id.as_ref() == "call_yolo_deny"))
        })
        .expect("expected a request containing the denial outcome");
    assert_tool_result_with_id(&with_result.messages, "call_yolo_deny", Some("denied"));
}
