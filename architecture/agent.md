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

**ChatEvent Types**:
- `Text` - Plain text response
- `ToolCall` - Request to execute a tool
- `ToolResult` - Tool execution result
- `Error` - Error occurred
- `Eom` - End of message marker

### team.rs / worker.rs - Subagent Pool

Manages concurrent subagent execution:

```rust
pub struct SubAgentPool {
    semaphore: Semaphore,
    max_concurrent: usize,  // default: 5
}
```

**Key Functions**:
- `process_request()` - Execute subagent task
- Returns `SubAgentResult::success()` with `SubagentCompleted` event

### mention.rs - Agent Mentions

Handles `@agent` mentions for routing to specific agents.

### prompt.rs - Prompt Templates

Loads and manages prompt templates from files.

## Key Types

### Agent

```rust
pub struct Agent {
    pub id: String,
    pub name: String,
    pub model: String,
    pub system_prompt: String,
    pub permission_level: PermissionLevel,
    pub mode: AgentMode,
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

1. **`process_request()` is implemented** - Properly publishes `SubagentStarted`/`SubagentCompleted` events
2. **`SubAgentPool` bounded concurrency** - Uses semaphore with default of 5
3. **Tool definition caching** - Cache key includes version for proper invalidation
4. **DoomLoop doc mismatch** - Comment says "consecutive" but implementation uses window-based counting

## See Also

- [provider.md](provider.md) - LLM provider interface
- [tool.md](tool.md) - Tool registry and execution
- [permission.md](permission.md) - Access control
- [event-bus.md](event-bus.md) - Event publishing
- [hooks.md](hooks.md) - Lifecycle hooks
