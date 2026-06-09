---
name: agent-loop
description: Guide for AgentLoop integration with TUI and event-driven architecture
version: 2.0.0
tags:
  - agent
  - loop
  - streaming
  - tools
  - provider
  - permissions
  - questions
---

# Agent Loop Integration Guide

This skill covers integrating the `AgentLoop` with the TUI's event loop using the GlobalEventBus for proper event-driven architecture.

## Key Components

### AgentLoop (`src/agent/loop.rs`)

The main orchestration struct for agent execution:

```rust
pub struct AgentLoop {
    agents: HashMap<String, Agent>,
    state: AgentLoopState,
    limits: ExecutionLimits,
    provider: Box<dyn crate::provider::Provider>,  // Note: uses Box<dyn Provider>
    permission_checker: PermissionChecker,
    tool_registry: ToolRegistry,
    tool_def_cache: Option<ToolDefCache>,  // Caches tool definitions for performance
    session_id: String,
    // ...
}

impl AgentLoop {
    pub async fn run(&mut self, request: ChatRequest) -> Result<Vec<ChatEvent>, AppError>
}
```

### Provider Trait (`src/provider/mod.rs`)

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn clone_box(&self) -> Box<dyn Provider>;  // Required for cloning
    async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError>;
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
}
```

**All providers must implement `Clone` and `clone_box()` for AgentLoop integration.**

### Message Types with Arc<String>

The `Message` enum and `ToolCall` struct use `Arc<String>` for content fields to reduce cloning overhead:

```rust
pub enum Message {
    System { content: Arc<String> },
    User { content: Vec<ContentPart> },
    Assistant { content: Vec<ContentPart> },
    Tool { tool_call_id: Arc<String>, content: Arc<String> },
}

pub struct ToolCall {
    pub id: Arc<String>,
    pub name: Arc<String>,
    pub arguments: serde_json::Value,
}

pub enum ChatEvent {
    TextDelta(Arc<String>),
    ReasoningDelta(Arc<String>),
    ToolCall(ToolCall),
    ToolResult { tool_call_id: Arc<String>, content: Arc<String> },
    Finish { stop_reason: Arc<String>, usage: TokenUsage },
    Error(Arc<String>),
}
```

When creating these types, use `.into()` to convert `String` to `Arc<String>`:
```rust
Message::System { content: "hello".into() }
ContentPart::Text { text: some_string.into() }
```

When comparing `Arc<String>` with `&str`, use `&*arc_string == "literal"` or `arc_string.as_str() == "literal"`.

### GlobalEventBus (`src/bus/global.rs`)

Singleton event bus for pub/sub communication:

```rust
pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn publish(event: AppEvent) { ... }
    pub fn subscribe() -> broadcast::Receiver<AppEvent> { ... }
}
```

**All components can publish events and subscribe to receive them.**

### AppEvent Types (`src/bus/events.rs`)

```rust
pub enum AppEvent {
    TextDelta { session_id: String, delta: String },
    ReasoningDelta { session_id: String, delta: String },
    ToolCallStarted { session_id: String, tool_name: String, tool_id: String, arguments: String },
    AgentFinished { session_id: String, stop_reason: String },
    PermissionPending { session_id: String, perm_id: String, tool: String, path: Option<String>, args: Option<serde_json::Value> },
    QuestionPending { session_id: String, questions: String },
    // ...
}
```

## Architecture

### Event Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                         TUI Event Loop                              │
│  tokio::select! {                                                   │
│      reader.next() → keyboard/mouse events                         │
│      bus_rx.recv() → AppEvents from GlobalEventBus                 │
│  }                                                                  │
└─────────────────────────────────────────────────────────────────────┘
                              ↑ GlobalEventBus
                              │
┌─────────────────────────────────────────────────────────────────────┐
│                         AgentLoop                                   │
│  1. Send user message                                              │
│  2. Stream from provider                                            │
│  3. On ChatEvent::TextDelta → GlobalEventBus::publish(TextDelta)   │
│  4. On ChatEvent::ToolCall → GlobalEventBus::publish(ToolCallStarted)│
│  5. On permission check needed →                                     │
│     - GlobalEventBus::publish(PermissionPending)                    │
│     - Register with PermissionRegistry                             │
│     - Wait for response via oneshot channel                         │
│  6. On question tool →                                              │
│     - GlobalEventBus::publish(QuestionPending)                      │
│     - Register with QuestionRegistry                                │
│     - Wait for answers via oneshot channel                         │
│  7. Execute tools with permission checks                            │
└─────────────────────────────────────────────────────────────────────┘
                              ↓
                    ┌─────────────────┐
                    │  ToolRegistry   │
                    │  PermissionChecker│
                    │  Provider        │
                    └─────────────────┘
```

### Permission Flow (Hard Guardrails)

```
AgentLoop::run() → permission_checker.check(tool_name, extracted_path)
                          │
                          ├─ Allow → execute tool
                          ├─ Deny → return error to LLM
                          └─ Ask(req) → 
                                     │
                                     ├── GlobalEventBus::publish(PermissionPending)
                                     ├── PermissionRegistry::register(perm_id, resp_tx)
                                     ├── await resp_rx (300s timeout)
                                     ├── if timeout → DenyOnce
                                     └── if response → AllowOnce/AlwaysAllow/DenyOnce/AlwaysDeny
```

**Path Extraction**: The `permission_checker.check()` receives path extracted from tool arguments:
- `read`, `write`, `edit`, `glob`, `grep`, `list` → `arguments["path"]`
- `apply_patch` → `arguments["patch_path"]`

This is implemented via `extract_path_from_tool_call()` in `src/agent/loop.rs`.

### Question Tool Flow

```
AgentLoop detects "question" tool call
    │
    ├── Parse questions from arguments
    ├── GlobalEventBus::publish(QuestionPending { session_id, questions_json })
    ├── QuestionRegistry::register(session_id, tx)
    ├── await rx for answers
    └── Format answers and return to LLM
```

## TUI Integration

### In `src/tui/mod.rs`:

```rust
pub async fn run_event_loop(app: &mut App) -> Result<(), AppError> {
    let mut bus_rx = GlobalEventBus::subscribe();  // Subscribe to events
    
    tokio::select! {
        biased;
        
        Some(result) = reader.next() => { /* keyboard/mouse */ }
        
        Ok(event) = bus_rx.recv() => {
            match event {
                AppEvent::TextDelta { delta, .. } => {
                    app.messages_state.messages.add_assistant_text(delta);
                }
                AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => {
                    let args: serde_json::Value = serde_json::from_str(&arguments).unwrap();
                    app.messages_state.messages.add_tool_call(tool_id, tool_name, args);
                }
                AppEvent::PermissionPending { perm_id, tool, path, args } => {
                    app.show_permission_dialog(perm_id, PermissionRequest { tool, path, args });
                }
                AppEvent::QuestionPending { session_id, questions } => {
                    let questions: Vec<QuestionSpec> = serde_json::from_str(&questions).unwrap();
                    app.show_question_dialog(questions, session_id);
                }
                AppEvent::AgentFinished { .. } => {
                    app.session_state.session_status = SessionStatus::Idle;
                }
                // ...
            }
        }
    }
}
```

### Spawning AgentLoop

```rust
processing_task = Some(tokio::spawn({
    let model = app.agent_state.current_model.clone();
    let messages = build_conversation_context(&app.messages_state.messages.messages);
    let session_id = app.session_state.session.as_ref().map(|s| s.id.clone()).unwrap_or_default();
    let agents = app.agent_state.agents.clone();
    let config = config.clone();

    async move {
        let registry = ProviderRegistry::new();
        crate::provider::register_builtin_with_config(&mut registry, &config);
        
        let provider_name = model.split('/').next().unwrap_or("openai").to_string();
        let model_name = model.split('/').next_back().unwrap_or(&model).to_string();
        
        if let Some(base_provider) = registry.get(&provider_name) {
            let provider = base_provider.clone_box();
            let tool_registry = ToolRegistry::with_defaults();
            let permission_checker = PermissionChecker::new(Some(&config), None);

            let mut agent_loop = AgentLoop::new(
                agents,
                provider,
                permission_checker,
                tool_registry,
                config,
                None, // mcp_service
                None, // pool
            );
            agent_loop.set_session_id(&session_id);

            let request = ChatRequest {
                messages,
                model: model_name,
                tools: None,  // AgentLoop sets tools via build_tool_definitions()
                system: Some(system_prompt),
                temperature: None,
                top_p: None,
                max_tokens: None,
            };

            if let Err(e) = agent_loop.run(request).await {
                tracing::error!("Agent loop error: {}", e);
            }
        }
    }
}));
```

## PermissionRegistry Pattern

For handling permission responses from TUI:

```rust
// In src/bus/mod.rs
use crate::bus::PermissionDecision;

pub struct PermissionRegistry {
    senders: DashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>,
}

impl PermissionRegistry {
    // Note: These are synchronous functions (NOT async)
    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionDecision>) {
        PERMISSION_REGISTRY.senders.insert(perm_id, tx);
    }

    pub fn respond(perm_id: String, choice: PermissionDecision) -> bool {
        if let Some((_, tx)) = PERMISSION_REGISTRY.senders.remove(&perm_id) {
            let _ = tx.send(choice);
            true
        } else {
            false
        }
    }

    pub fn is_registered(perm_id: &str) -> bool {
        PERMISSION_REGISTRY.senders.contains_key(perm_id)
    }
}

// In TUI, when user responds to permission dialog:
pub fn submit_permission_response(&mut self, allowed: bool) {
    if let Some(perm_id) = self.dialog_state.permission_perm_id {
        // Call sync respond function (NOT async)
        PermissionRegistry::respond(
            perm_id,
            match allowed {
                true => PermissionDecision::AllowOnce,
                false => PermissionDecision::DenyOnce,
            },
        );
    }
}
```

## QuestionRegistry Pattern

For handling question answers from TUI:

```rust
// In src/bus/mod.rs
pub struct QuestionRegistry {
    senders: DashMap<String, tokio::sync::oneshot::Sender<String>>,
}

impl QuestionRegistry {
    // Note: These are synchronous functions (NOT async)
    pub fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>) {
        QUESTION_REGISTRY.senders.insert(question_id, tx);
    }

    pub fn answer_question(question_id: String, answers: String) -> bool {
        if let Some((_, tx)) = QUESTION_REGISTRY.senders.remove(&question_id) {
            let _ = tx.send(answers);
            true
        } else {
            false
        }
    }
}

// In TUI, when user submits question answers:
pub fn submit_question_answers(&mut self) {
    if let Some(session_id) = self.dialog_state.question_session_id.take() {
        let answers = self.dialog_state.question_dialog.as_ref().unwrap().answers_json();
        // Call sync answer function (NOT async)
        QuestionRegistry::answer_question(session_id, answers);
    }
}
```

## Feature Flags

Debug logging is controlled by the `debug-logging` feature flag:

```toml
[features]
debug-logging = []
```

When enabled, debug file I/O is active in TUI components. Default is disabled.

## Async Patterns

### Preferred: Direct async/await
Always prefer making functions async and awaiting directly:
```rust
// ✅ Good - async function with direct await
async fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
    plugin_svc.dispatch_tool_definition(input).await
}

// ✅ Good - async function for compaction hooks
async fn compact_if_needed(&mut self, messages: &mut Vec<Message>) {
    if let Some(ref plugin_svc) = self.plugin_service {
        plugin_svc.dispatch_hook(ctx).await
    }
}
```

### Avoid: block_in_place + block_on
This pattern causes thread starvation and deadlocks:
```rust
// ❌ Bad - causes deadlock risk
tokio::task::block_in_place(|| {
    tokio::runtime::Handle::current().block_on(plugin_svc.dispatch_hook(ctx))
})
```

### tokio::select! with biased
When shutdown must take priority over other branches, use `biased`:
```rust
tokio::select! {
    biased;
    _ = shutdown.notified() => break,  // Shutdown always wins first
    chunk = stream.next() => { /* handle chunk */ }
    _ = tokio::time::sleep(Duration::from_millis(100)) => { /* fallback */ }
}
```

### Duration in async contexts
Use `tokio::time::Duration` in async functions:
```rust
// ✅ Good
tokio::time::sleep(tokio::time::Duration::from_secs(30))

// ❌ Bad - std::time::Duration blocks the thread
tokio::time::sleep(std::time::Duration::from_secs(30))
```

### Proper shutdown with Notify
For task cancellation, prefer `tokio::sync::Notify` over polling loops:
```rust
// ✅ Good - clean shutdown via Notify
let shutdown = Arc::clone(&self.shutdown);
tokio::spawn(async move {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.notified() => break,
            chunk = stream.next() => { /* handle */ }
        }
    }
});

// ❌ Bad - busy-wait polling
loop {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            if *shutdown.lock() { break; }
        }
        chunk = stream.next() => { /* handle */ }
    }
}
```

## Common Issues

### Issue: Provider doesn't implement Provider trait

**Error**: `the trait 'Provider' is not implemented for '&dyn Provider'`

**Cause**: `ProviderRegistry::get()` returns `&dyn Provider`, but AgentLoop now uses `Box<dyn Provider>`.

**Solution**: Use `provider.clone_box()` to get an owned `Box<dyn Provider>`.

### Issue: Events not appearing in UI

**Debug**: Verify GlobalEventBus subscription is active in `run_event_loop`.

**Debug**: Check that AgentLoop is publishing events (should happen automatically now).

### Issue: Permission dialog doesn't appear

**Debug**: Check that TUI handles `AppEvent::PermissionPending` in its `bus_rx` match arm.

**Debug**: Verify `PermissionRegistry::respond()` is being called when user responds.

### Issue: Questions not working

**Debug**: Check that `QuestionRegistry` is being registered when `question` tool is called.

**Debug**: Verify TUI calls `QuestionRegistry::answer_question()` on submit.

## Provider Implementation Best Practices

### Client Reuse

Providers should store `reqwest::Client` as a struct field rather than creating one per request:

```rust
pub struct OpenAiCompatibleProvider {
    id: String,
    name: String,
    config: OpenAiCompatibleConfig,
    client: reqwest::Client,  // Store for reuse
}

impl OpenAiCompatibleProvider {
    pub fn new(id: &str, name: &str, config: OpenAiCompatibleConfig) -> Self {
        Self {
            // ...
            client: reqwest::Client::new(),
        }
    }
}
```

### Bounded Streaming Buffers

When implementing streaming providers, prevent unbounded buffer growth:

```rust
const MAX_BUFFER_SIZE: usize = 1024 * 1024;  // 1MB limit

// In stream implementation:
loop {
    if buffer.len() > MAX_BUFFER_SIZE {
        return Some((Err(ProviderError::Stream("buffer exceeded limit".to_string())), stream));
    }
    // Process chunk...
}
```

## Adaptive Compaction Strategy

The compaction system in `src/agent/compaction.rs` supports adaptive strategy selection based on message characteristics:

### CompactionStrategy Enum

```rust
#[derive(Debug, PartialEq)]
pub enum CompactionStrategy {
    TruncateToolOutputs,  // Truncates tool result content to 500 chars
    SummarizeOldTurns,    // Uses LLM to summarize old turns (async)
    DropMiddleMessages,   // Removes middle messages, keeping first and last pairs
}
```

### Async LLM Summarization (Wave 3.1)

The `SummarizeOldTurns` strategy uses `llm_summarize()` to generate contextual summaries:

```rust
pub async fn llm_summarize(
    messages: &[Message],
    provider: &Arc<dyn Provider>,
) -> Result<String, AppError>
```

The `compact_messages_async()` and `auto_compact_async()` functions use this strategy with proper fallback:

```rust
pub async fn auto_compact_async(
    messages: &[Message],
    context_limit: usize,
    threshold: f64,
    prune: bool,
    provider: Option<&Arc<dyn Provider>>,
) -> Vec<Message> {
    // Falls back to DropMiddleMessages if provider unavailable
    if strategy == CompactionStrategy::SummarizeOldTurns {
        if let Some(p) = provider {
            result = compact_messages_async(result, strategy, p).await;
        } else {
            result = compact_messages_sync(result, CompactionStrategy::DropMiddleMessages);
        }
    }
}
```

**Key functions:**
- `compact_messages_sync()` - sync version, uses placeholder summary
- `compact_messages_async()` - async version, uses LLM summarization
- `auto_compact_sync()` - sync auto-compaction
- `auto_compact_async()` - async auto-compaction with provider

### Sync Version for Compatibility

For contexts where async is not available, use `auto_compact_sync()` or `compact_messages_sync()`:

```rust
pub fn compact_messages_sync(
    messages: Vec<Message>,
    strategy: CompactionStrategy,
) -> Vec<Message>
```

### Adaptive Selection in select_compaction_strategy

The `select_compaction_strategy()` function picks the best strategy:

```rust
fn select_compaction_strategy(messages: &[Message]) -> CompactionStrategy {
    let non_system_count = count_non_system_messages(messages);
    let has_long_tools = has_long_tool_outputs(messages, 2000);

    if has_long_tools && non_system_count > 6 {
        CompactionStrategy::TruncateToolOutputs
    } else if non_system_count > 8 {
        CompactionStrategy::SummarizeOldTurns
    } else {
        CompactionStrategy::DropMiddleMessages
    }
}
```

**Selection criteria:**
- **TruncateToolOutputs**: When messages have long tool outputs (>2000 chars) AND there are many messages (>6)
- **SummarizeOldTurns**: When there are many messages (>8 non-system) without long tool outputs
- **DropMiddleMessages**: Default for smaller conversations

## Agent Loop Test Harness (Agent Harness Hardening)

The test harness in `tests/agent_loop_harness.rs` provides utilities for testing AgentLoop behavior with scripted providers and fake tools.

### ScriptedProvider

A test provider that returns pre-scripted responses for deterministic testing:

```rust
#[derive(Clone)]
struct ScriptedProvider {
    responses: Vec<Vec<ChatEvent>>,  // Each inner vec is one provider turn
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
}

impl ScriptedProvider {
    fn new(responses: Vec<Vec<ChatEvent>>) -> Self;
    async fn get_requests(&self) -> Vec<ChatRequest>;
    async fn request_count(&self) -> usize;
}
```

Usage:
```rust
let responses = vec![
    vec![ChatEvent::ToolCall(...), ChatEvent::Finish {...}],  // Turn 1
    vec![ChatEvent::TextDelta(...), ChatEvent::Finish {...}], // Turn 2
];
let provider = Box::new(ScriptedProvider::new(responses));
```

### Test Tools

**EchoArgsTool** - Returns input arguments as JSON:
```rust
struct EchoArgsTool;
// name: "echo_args"
// Returns input arguments as formatted JSON string
```

**SlowEchoTool** - Waits on a barrier before echoing (for async testing):
```rust
struct SlowEchoTool {
    barrier: Arc<Mutex<()>>,
}
// Useful for testing concurrent operations
```

### Helper Functions (Packet 1)

Located in `tests/agent_loop_harness.rs`:

```rust
// Assert message roles match expected order
fn assert_messages_have_roles(msgs: &[Message], expected_roles: &[&str]);

// Assert assistant message has specific tool call
fn assert_assistant_has_tool_call(msg: &Message, id: &str, name: &str, arg_value: Option<&str>);

// Find and assert tool result with specific ID
fn assert_tool_result_with_id(msgs: &[Message], tool_call_id: &str, contains: Option<&str>);

// Assert tool call precedes its result in message sequence
fn assert_assistant_tool_call_precedes_result(msgs: &[Message], tool_call_id: &str);

// Assert no orphan tool results (all have prior assistant tool call)
fn assert_no_orphan_tool_results(msgs: &[Message]);

// Get tool result IDs in order they appear
fn get_tool_results_in_order(msgs: &[Message]) -> Vec<String>;

// Assert tool results match expected order
fn assert_tool_results_match_order(msgs: &[Message], expected_ids: &[&str]);
```

### PermissionRegistry and QuestionRegistry Testing

```rust
// Set session ID for QuestionRegistry (Packet 3)
agent_loop.set_session_id("test-session-123");

// Register and answer questions via QuestionRegistry
QuestionRegistry::answer_question("test-session-123".to_string(), answers_json).await;

// Check if permission is registered
let registered = PermissionRegistry::is_registered(&perm_id);
```

### EventCollector Helper (Packet 11)

Collects and asserts GlobalEventBus events during tests:

```rust
struct EventCollector {
    rx: broadcast::Receiver<AppEvent>,
    events: Vec<AppEvent>,
}

impl EventCollector {
    fn new() -> Self;
    fn collect(&mut self);  // Drain available events
    fn events(&self) -> &[AppEvent];
    fn find_event(&self, f: impl Fn(&AppEvent) -> bool) -> Option<&AppEvent>;
    fn assert_event_order(&self, expected_types: &[&str]);  // Verify event order
}
```

Example usage:
```rust
let mut event_collector = EventCollector::new();
// ... run agent loop ...
event_collector.collect();
event_collector.assert_event_order(&[
    "text:delta",
    "tool_call:started",
    "tool:result",
    "agent:finished",
]);
```

## Packet Completion Status (Agent Harness Hardening)

| Packet | Description | Status |
|--------|-------------|--------|
| 1 | Helper functions for transcript validation | ✅ Complete |
| 2 | Tool result ordering tests | ✅ Complete |
| 3 | Question tool flow with set_session_id() | ✅ Complete |
| 4 | PermissionRegistry.is_registered() | ✅ Complete |
| 5 | Retry semantics (RetryThenSuccessProvider) + max parallel tools test | ✅ Complete |
| 6 | Follow-up contract tests (FollowUpProvider) | ✅ Complete |
| 7 | Provider transcript golden tests | ✅ Complete |
| 8 | Compaction safety tests | ✅ Complete |
| 9 | Event bus observability (EventCollector) | ✅ Complete |
| 10 | Subagent lifecycle tests + denied tools passthrough test | ✅ Complete |
| 11 | All harness tests integrated | ✅ Complete |

## New Tests Added (2026-05-01)

### `test_max_parallel_tools_enforcement`
- Verifies `max_parallel_tools` config is enforced
- Uses `ParallelTool` test tool to track concurrent executions
- Asserts max observed concurrent tools ≤ configured limit

### `test_task_tool_denied_tools_passthrough`
- Verifies denied tools are passed from task request to subagent execution
- Uses `RequestRecordingProvider` to capture subagent provider requests
- Asserts denied tools are filtered from subagent tool registry

### New Test Tools
- `ParallelTool`: Tracks concurrent executions with atomic counters
- `RequestRecordingProvider`: Records all ChatRequests for verification

## Follow-Up Packets (2026-05-01)

### Packet 1: Follow-Up Latency Fix
- `drain_follow_up()` now uses non-blocking `try_recv()` instead of 5-second timeout
- New test `test_no_follow_up_latency` verifies no-follow-up runs complete quickly (< 1s)
- Follow-up test updated to queue before `run()` to work with non-blocking behavior

### Packet 2: Deterministic Coordination
New helper functions for event-based coordination:
```rust
// Wait for QuestionPending event via GlobalEventBus
async fn wait_for_question_pending(
    session_id: &str,
    rx: &mut broadcast::Receiver<AppEvent>,
    timeout: Duration,
) -> Result<(), AppError>

// Wait for PermissionPending event via GlobalEventBus
async fn wait_for_permission_pending(
    perm_id: &str,
    rx: &mut broadcast::Receiver<AppEvent>,
    timeout: Duration,
) -> Result<(), AppError>
```

### Packet 3: Subagent Test Helpers (tests/subagent.rs)
New helper functions for deterministic subagent testing:
```rust
// Bounded completion polling for task terminal states
async fn wait_for_task_result(
    task_store: &Arc<Mutex<TaskStore>>,
    task_id: u64,
    max_attempts: u32,
    interval: Duration,
) -> Result<SubAgentResult, AppError>

// Create task before sending subagent request
fn create_task_and_send(
    task_store: &Arc<Mutex<TaskStore>>,
    spawner: &SubAgentSpawner,
    request: SubAgentRequest,
) -> Result<u64, String>
```

## Additional Packets Completed (2026-05-01 Wave 2)

### Packet 1: Fix Event Subscription Race
Tests that wait for `GlobalEventBus` events (question/permission) must subscribe BEFORE spawning the producing task:
```rust
// ✅ Correct - subscribe before spawn
let mut rx = GlobalEventBus::subscribe();
let handle = tokio::spawn(async move { agent_loop.run(request).await });
wait_for_question_pending("session", &mut rx, timeout).await;

// ❌ Wrong - subscribe after spawn (race condition)
let handle = tokio::spawn(async move { agent_loop.run(request).await });
let mut rx = GlobalEventBus::subscribe();  // May miss events!
```

### Packet 2: Remove Task Tool Denied-Tools Sleep
Replaced arbitrary 2-second sleep with deterministic polling via `RequestRecordingProvider::wait_for_request()`:
```rust
// Instead of: tokio::time::sleep(Duration::from_secs(2)).await;
// Use:
subagent_provider.wait_for_request(100, 20).await
    .expect("Subagent provider should receive request within timeout");
```

### Packet 4: Follow-Up Contract Clarified
- Follow-ups queued BEFORE `run()` are processed by that `run()`
- Follow-ups arriving AFTER `run()` returns are NOT consumed by the completed run
- `drain_follow_up()` uses non-blocking `try_recv()`

## Related Skills

- See `.opencode/skills/tui/SKILL.md` for TUI development overview
- See `.opencode/skills/permission/SKILL.md` for PermissionDecision (bus-owned type) and registry usage
- See `.opencode/skills/provider/SKILL.md` for ScriptedProvider and transcript tests
- See `AGENTS.md` for project-wide patterns
