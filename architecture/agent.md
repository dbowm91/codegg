# Agent Module

The `agent` module is the core of the AI coding agent, responsible for orchestrating conversation between the LLM and tools.

## Overview

**Location**: `src/agent/`

**Key Responsibilities**:
- Main agent loop (`AgentLoop`)
- Message processing and event handling
- Subagent pool management
- Context compaction for long conversations
- Model auto-routing based on task complexity
- Team coordination for multi-agent tasks

## Components

### loop.rs - AgentLoop

The central orchestrator that runs the conversation:

```rust
pub struct AgentLoop {
    agents: HashMap<String, Agent>,
    state: AgentLoopState,
    limits: ExecutionLimits,
    provider: Box<dyn crate::provider::Provider>,
    permission_checker: PermissionChecker,
    tool_registry: ToolRegistry,
    hook_registry: Option<Arc<HookRegistry>>,
    context_tracker: ContextTracker,
    doom_detector: DoomLoopDetector,
    steering: AtomicBool,
    follow_up_tx: mpsc::UnboundedSender<String>,
    follow_up_rx: mpsc::UnboundedReceiver<String>,
    config: Config,
    question_tx: Option<tokio::sync::oneshot::Sender<String>>,
    question_rx: Option<tokio::sync::oneshot::Receiver<String>>,
    plugin_service: Option<Arc<crate::plugin::service::PluginService>>,
    session_id: String,
    mcp_service: Option<Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
    tool_def_cache: Option<ToolDefCache>,
    model_router: ModelRouter,
    snapshot_manager: Option<crate::snapshot::SnapshotManager>,
    file_change_rx: tokio::sync::broadcast::Receiver<AppEvent>,
}
```

**Key Methods**:
- `run()` - Main event loop
- `run_with_prompt()` - Convenience method for simple prompts
- `process_event()` - Handle incoming ChatEvents
- `check_tool_permission()` - Verify tool execution is allowed
- `compact_if_needed()` - Context compaction if needed
- `build_tool_definitions()` - Build tool definitions with caching
- `execute_tool_calls()` - Execute tools with permission checks
- `stream_with_retry()` - Stream with exponential backoff retry

### compaction.rs - Context Management

Handles context window management for long conversations:

**Key Types**:
- `ContextTracker` - Tracks token usage and message count
- `CompactionStrategy` - Determines how to compact context (TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages)

**Key Functions**:
- `detect_overflow()` - Check if overflow protection is needed
- `prune_tool_outputs()` - Prune long tool outputs
- `compact_messages_sync()` - Sync compaction
- `compact_messages_async()` - Async compaction with LLM summarization
- `auto_compact_async()` - Auto-compact with adaptive strategy
- `llm_summarize()` - LLM-based summarization for context

### router.rs - Model Router

Auto-routes tasks to appropriate model complexity:

```rust
pub struct ModelRouter {
    enabled: bool,
    simple_model: Option<String>,
    medium_model: Option<String>,
    complex_model: Option<String>,
}

pub enum TaskComplexity {
    Simple,
    Medium,
    Complex,
}
```

**Key Functions**:
- `classify()` - Classify task complexity by tool name and content keywords
- `route_model()` - Select appropriate model for task
- `is_enabled()` - Check if routing is enabled

### processor.rs - Event Processor

Processes `ChatEvent` types from the provider:

**ChatEvent Types** (from `provider/mod.rs`):
- `TextDelta(Arc<String>)` - Text content streamed
- `ReasoningDelta(Arc<String>)` - Reasoning content streamed
- `ToolCall(ToolCall)` - Request to execute a tool
- `Finish{ stop_reason, usage }` - End of message marker
- `Error(Arc<String>)` - Error occurred

### worker.rs - Subagent Pool

Manages concurrent subagent execution:

```rust
pub struct SubAgentPool {
    shutdown_tx: broadcast::Sender<()>,
    active_count: Arc<AtomicUsize>,
    max_concurrent: usize,  // default: 5
    max_depth: usize,       // default: 3
    task_store: Arc<TokioMutex<TaskStore>>,
    workers: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>,
    request_tx: mpsc::Sender<WorkerRequest>,
    agents: Arc<Vec<Agent>>,
    provider_registry: Arc<ProviderRegistry>,
    config: Arc<Config>,
    session_store: Arc<SessionStore>,
    cancel_token: CancellationToken,
    active_handles: Arc<TokioMutex<Vec<tokio::task::JoinHandle<()>>>>,
    pool: Option<SqlitePool>,
}
```

**Key Types**:
- `SubAgentRequest` - Task request with task_id, prompt, agent, parent_id, denied_tools, allowed_paths, description, depth
- `SubAgentResult` - Task result with task_id, success, result
- `SubAgentSpawner` - Cloneable handle for sending requests to pool

**Key Functions**:
- `SubAgentPool::new()` / `new_with_store()` - Creates new pool with semaphore-based concurrency control, worker loop started immediately
- `SubAgentSpawner::send()` / `send_async()` - Sends subagent request (both are async now, share implementation via `handle_response()`)
- `SubAgentPool::shutdown()` - Clean shutdown with cooperative cancellation
- Subagent execution publishes `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`/`SubagentFailed` events
- RAII guard pattern (`ActiveCountGuard`) ensures proper active_count management

**Note**: `start_workers()` method was removed (was a dead no-op method)

### task.rs - Background Tasks

Background task scheduling for periodic work:

```rust
pub struct BackgroundTask {
    pub id: String,
    pub interval: Duration,
    pub message: String,
    pub last_run: Option<i64>,
    pub created_at: i64,
    pub session_id: String,
    pub db_id: Option<i64>,
}

pub struct BackgroundScheduler {
    tasks: Arc<RwLock<Vec<BackgroundTask>>>,
    shutdown_tx: broadcast::Sender<()>,
    callback: Option<TaskCallback>,
    pool: Option<SqlitePool>,
}
```

**Key Functions**:
- `add()` - Add a new background task
- `tick()` - Get all tasks that should fire
- `spawn_loop()` - Spawn the background task loop
- `load_tasks()` / `save_task()` - SQLite persistence

**Note**: BackgroundScheduler parses `task.id` and skips tasks with invalid IDs instead of using a random fallback. If parsing fails, the task is logged and skipped.

### mention.rs - Agent Mentions

Handles `@agent` mentions for routing to specific agents.

### prompt.rs - Prompt Templates

Loads and manages prompt templates from files.

### team.rs / teams.rs - Team Coordination

Multi-agent team coordination via file-based inbox communication.

## Key Types

### Agent

```rust
pub struct Agent {
    pub name: String,
    pub description: String,
    pub mode: AgentMode,
    pub mode_name: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub color: Option<String>,
    pub steps: Option<usize>,
    pub system_prompt: Option<String>,
    pub permissions: HashMap<String, String>,
    pub hidden: bool,
}
```

### AgentMode

```rust
pub enum AgentMode {
    Primary,    // Main agent handling user input
    Subagent,   // Helper agent for specific tasks
    All,        // Respond to all messages
}
```

### AgentLoopState

Tracks runtime state:
- `current_agent` - Current agent name
- `turn_count` - Number of turns
- `total_tokens` - Total tokens used
- `start_time` - Start timestamp
- `plan_mode` - Whether in plan mode
- `plan_topic` - Current plan topic

## Interactions

```
AgentLoop
├── Provider → LLM calls
├── ToolRegistry → Tool execution
├── PermissionChecker → Access control
├── GlobalEventBus → Event publishing
├── HookRegistry → Lifecycle hooks
├── Snapshot → File state capture
└── Session → Message history
```

## Events Published

- `SubagentStarted` - When subagent begins
- `SubagentProgress` - Subagent progress update
- `SubagentCompleted` - When subagent finishes successfully
- `SubagentFailed` - When subagent fails
- `TextDelta` - Text stream delta
- `ReasoningDelta` - Reasoning stream delta
- `ToolCallStarted` - Tool call initiated
- `ToolResult` - Tool execution result
- `AgentFinished` - Agent finished with stop reason
- `PermissionPending` - Permission requested
- `QuestionPending` - Question pending user response

**Note**: `AgentStarted`, `AgentEnded`, `CompactionStarted`, `CompactionEnded` are NOT published - hooks run these lifecycle events instead.

## Configuration

Related configurations in `config/`:
- `agent.model` - Default model
- `agent.compaction_tokens` - Token threshold for compaction
- `router.enabled` - Enable model auto-routing
- `router.thresholds` - Complexity thresholds
- `subagent.max_concurrent` - Max concurrent subagents (default: 5)
- `subagent.max_depth` - Max recursion depth (default: 3)

## Known Implementation Notes

1. **Subagent event publishing** - `SubagentStarted`/`SubagentCompleted`/`SubagentFailed` events properly published via `GlobalEventBus`
2. **`SubAgentPool` bounded concurrency** - Uses semaphore with default of 5, RAII guard pattern for active_count
3. **Tool definition caching** - Cache key includes mcp_tool_count, permission_version for proper invalidation (uses mcp_tool_count as proxy - known limitation)
4. **DoomLoop detection** - Uses window-based counting (not consecutive), correctly documented
5. **ToolExecuteBefore/After hooks** - Both hooks ARE invoked in `execute_tool_calls()` at loop.rs:1764 and 1806
6. **BackgroundScheduler task_id** - Uses `task.id.parse()` to use actual background task ID instead of random
7. **`start_workers()` removed** - Dead no-op method was removed, workers start in constructors

## See Also

- [provider.md](provider.md) - LLM provider interface
- [tool.md](tool.md) - Tool registry and execution
- [permission.md](permission.md) - Access control
- [event-bus.md](event-bus.md) - Event publishing
- [hooks.md](hooks.md) - Lifecycle hooks
- `.opencode/skills/subagent/SKILL.md` - Subagent infrastructure details
- `.opencode/skills/compaction/SKILL.md` - Compaction system details
- `.opencode/skills/router/SKILL.md` - Model routing details