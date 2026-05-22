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
    agent: Agent,
    client: Client,
    provider: Arc<ProviderRegistry>,
    tool_registry: Arc<ToolRegistry>,
    permission: Arc<PermissionChecker>,
    event_bus: GlobalEventBus,
    hooks: HookRegistry,
    // ...
}
```

**Key Methods**:
- `run()` - Main event loop
- `process_event()` - Handle incoming ChatEvents
- `check_tool_permission()` - Verify tool execution is allowed
- `should_compact()` - Determine if context needs compaction

### compaction.rs - Context Management

Handles context window management for long conversations:

**Key Types**:
- `ContextTracker` - Tracks token usage and message count
- `CompactionStrategy` - Determines how to compact context

**Key Functions**:
- `should_compact()` - Check if compaction is needed
- `compact()` - Perform context compaction
- Token estimation using `provider::count_tokens()`

### router.rs - Model Router

Auto-routes tasks to appropriate model complexity:

```rust
pub struct ModelRouter {
    config: RouterConfig,
    provider: Arc<ProviderRegistry>,
}
```

**Key Functions**:
- `route()` - Select appropriate model for task
- `estimate_complexity()` - Analyze task complexity

### processor.rs - Event Processor

Processes `ChatEvent` types from the provider:

**ChatEvent Types** (from `provider/mod.rs`):
- `TextDelta` - Text content streamed
- `ReasoningDelta` - Reasoning content streamed  
- `ToolCall` - Request to execute a tool
- `ToolResult` - Tool execution result with tool_call_id and content
- `Finish` - End of message marker with stop_reason and token usage
- `Error` - Error occurred

### team.rs / worker.rs - Subagent Pool

Manages concurrent subagent execution:

```rust
pub struct SubAgentPool {
    shutdown_tx: broadcast::Sender<()>,
    active_count: Arc<AtomicUsize>,
    max_concurrent: usize,  // default: 5
    max_depth: usize,      // default: 3
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
- `SubAgentPool::new()` - Creates new pool with semaphore-based concurrency control
- `SubAgentSpawner::send()` - Sends subagent request (async version available)
- Subagent execution publishes `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`/`SubagentFailed` events
- RAII guard pattern (`ActiveCountGuard`) ensures proper active_count management

### mention.rs - Agent Mentions

Handles `@agent` mentions for routing to specific agents.

### prompt.rs - Prompt Templates

Loads and manages prompt templates from files.

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
- Turn count
- Token usage
- Plan mode status
- Last activity timestamp

## Interactions

```
AgentLoop
├── ProviderRegistry → LLM calls
├── ToolRegistry → Tool execution
├── PermissionChecker → Access control
├── GlobalEventBus → Event publishing
├── HookRegistry → Lifecycle hooks
├── Snapshot → File state capture
└── Session → Message history
```

## Events Published

- `SubagentStarted` - When subagent begins
- `SubagentCompleted` - When subagent finishes
- `AgentStarted` - Agent loop started
- `AgentEnded` - Agent loop ended
- `CompactionStarted` - Context compaction triggered
- `CompactionEnded` - Context compaction complete

## Configuration

Related configurations in `config/`:
- `agent.model` - Default model
- `agent.compaction_tokens` - Token threshold for compaction
- `router.enabled` - Enable model auto-routing
- `router.thresholds` - Complexity thresholds

## Known Implementation Notes

1. **Subagent event publishing** - `SubagentStarted`/`SubagentCompleted`/`SubagentFailed` events properly published via `GlobalEventBus`
2. **`SubAgentPool` bounded concurrency** - Uses semaphore with default of 5, RAII guard pattern for active_count
3. **Tool definition caching** - Cache key includes mcp_tool_count, permission_version for proper invalidation
4. **DoomLoop detection** - Uses window-based counting (not consecutive), correctly documented
5. **ToolExecuteBefore/After hooks** - Both hooks ARE invoked in `execute_tool_calls()` at loop.rs:1764 and 1806

## See Also

- [provider.md](provider.md) - LLM provider interface
- [tool.md](tool.md) - Tool registry and execution
- [permission.md](permission.md) - Access control
- [event-bus.md](event-bus.md) - Event publishing
- [hooks.md](hooks.md) - Lifecycle hooks
