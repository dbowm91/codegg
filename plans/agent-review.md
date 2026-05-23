# Agent Module Architecture Review

## Verified Claims

### AgentLoop (loop.rs)
- **AgentLoop struct fields**: All fields match exactly between doc and implementation (lines 548-571):
  - `agents: HashMap<String, Agent>` ✓
  - `state: AgentLoopState` ✓
  - `limits: ExecutionLimits` ✓
  - `provider: Box<dyn crate::provider::Provider>` ✓
  - `permission_checker: PermissionChecker` ✓
  - `tool_registry: ToolRegistry` ✓
  - `hook_registry: Option<Arc<HookRegistry>>` ✓
  - `context_tracker: ContextTracker` ✓
  - `doom_detector: DoomLoopDetector` ✓
  - `steering: AtomicBool` ✓
  - `follow_up_tx/rx: mpsc::UnboundedSender/Receiver<String>` ✓
  - `config: Config` ✓
  - `question_tx/rx: Option<oneshot::Sender/Receiver<String>>` ✓
  - `plugin_service: Option<Arc<PluginService>>` ✓
  - `session_id: String` ✓
  - `mcp_service: Option<Arc<RwLock<McpService>>>` ✓
  - `tool_def_cache: Option<ToolDefCache>` ✓
  - `model_router: ModelRouter` ✓
  - `snapshot_manager: Option<SnapshotManager>` ✓
  - `file_change_rx: broadcast::Receiver<AppEvent>` ✓

- **Key Methods**: `run()`, `run_with_prompt()`, `process_event()` (via EventProcessor), `check_tool_permission()`, `compact_if_needed()`, `build_tool_definitions()`, `execute_tool_calls()`, `stream_with_retry()` all exist and are documented accurately.

### Agent struct (mod.rs)
- All fields match exactly (lines 28-42):
  - `name`, `description`, `mode`, `mode_name`, `model`, `variant`, `temperature`, `top_p`, `color`, `steps`, `system_prompt`, `permissions`, `hidden` ✓

### AgentMode (mod.rs)
- Variants `Primary`, `Subagent`, `All` with `#[default] = Primary` match doc (lines 46-51) ✓

### AgentLoopState (loop.rs:523-530)
- Fields `current_agent`, `turn_count`, `total_tokens`, `start_time`, `plan_mode`, `plan_topic` match doc ✓

### SubAgentPool (worker.rs:60-75)
- All fields match doc (lines 60-75):
  - `shutdown_tx: broadcast::Sender<()>` ✓
  - `active_count: Arc<AtomicUsize>` ✓
  - `max_concurrent: usize` (default 5) ✓
  - `max_depth: usize` (default 3) ✓
  - `task_store: Arc<TokioMutex<TaskStore>>` ✓
  - `workers: Arc<TokioMutex<Vec<JoinHandle<()>>>>` ✓
  - `request_tx: mpsc::Sender<WorkerRequest>` ✓
  - `agents: Arc<Vec<Agent>>` ✓
  - `provider_registry: Arc<ProviderRegistry>` ✓
  - `config: Arc<Config>` ✓
  - `session_store: Arc<SessionStore>` ✓
  - `cancel_token: CancellationToken` ✓
  - `active_handles: Arc<TokioMutex<Vec<JoinHandle<()>>>>` ✓
  - `pool: Option<SqlitePool>` ✓

### SubAgentPool key behaviors (worker.rs)
- **Bounded concurrency via semaphore**: Line 194 creates semaphore with `max_concurrent` permits ✓
- **RAII guard pattern**: Lines 224-241 implement `ActiveCountGuard` that increments on creation and decrements on drop ✓
- **`start_workers()` method removed**: No such method exists; worker loop is started directly in constructors `new()` and `new_with_store()` ✓
- **`SubAgentSpawner::send()` and `send_async()`**: Both exist and share implementation via `enqueue_request()` and `handle_response()` helpers (lines 412-456) ✓

### BackgroundScheduler (task.rs)
- **BackgroundTask struct fields**: `id`, `interval`, `message`, `last_run`, `created_at`, `session_id`, `db_id` all match (lines 31-39) ✓
- **BackgroundScheduler fields**: `tasks`, `shutdown_tx`, `callback`, `pool` match (lines 90-95) ✓
- **task_id uses `task.id.parse()`**: Line 228 uses `task.id.parse().unwrap_or_else(|_| rand::random::<u64>())` for SubAgentRequest task_id field ✓

### Compaction Types (compaction.rs)
- **ContextTracker**: Fields `current_tokens`, `context_limit`, `threshold`, `message_token_counts`, `max_messages`, `max_total_bytes`, `model` match (lines 76-84) ✓
- **CompactionStrategy**: `TruncateToolOutputs`, `SummarizeOldTurns`, `DropMiddleMessages` variants match (lines 218-222) ✓
- **Key Functions**: `detect_overflow()`, `prune_tool_outputs()`, `compact_messages_sync()`, `compact_messages_async()`, `auto_compact_async()`, `llm_summarize()` all exist and are documented accurately ✓

### ModelRouter (router.rs)
- Struct fields: `enabled`, `simple_model`, `medium_model`, `complex_model` match (lines 21-26) ✓
- **TaskComplexity enum**: `Simple`, `Medium`, `Complex` variants match (lines 4-8) ✓
- **Key Functions**: `classify()`, `route_model()`, `is_enabled()` all exist and work as documented ✓

### Processor (processor.rs)
- **ChatEvent types handled**: `TextDelta`, `ReasoningDelta`, `ToolCall`, `ToolResult`, `Finish`, `Error` - all match the provider ChatEvent types described in doc ✓

### Events Published
- **SubagentStarted/Progress/Completed/Failed**: All published in worker.rs:472, 485, 516, 531 via `GlobalEventBus::publish()` ✓
- **TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, AgentFinished, PermissionPending, QuestionPending**: All published in loop.rs ✓
- **AgentStarted/Ended/CompactionStarted/Ended NOT published**: Correct - hooks run these lifecycle events instead via `hook_registry.run_hooks()` ✓

### Hook Integration (loop.rs)
- **ToolExecuteBefore hook**: Called at line 1770 via `plugin_svc.dispatch_tool_execute_before()` ✓
- **ToolExecuteAfter hook**: Called at line 1812 via `plugin_svc.dispatch_tool_execute_after()` ✓
- **PreToolExecute and PostToolExecute hooks**: Called via `hook_registry.run_hooks()` at lines 1756 and 1829 respectively ✓

### Tool Definition Cache (loop.rs:1039-1080)
- **Cache invalidation uses mcp_tool_count as proxy**: Comment at line 1033-1037 confirms this is a known limitation ✓

## Bugs/Discrepancies Found

### 1. Documentation Mentions `process_event()` But Implementation Uses `processor.process()`

**Severity**: Low (documentation style issue)

**Location**: `architecture/agent.md` line 54 says `process_event()` is a key method, but the actual method is `EventProcessor::process()` called from within `run()`.

**Impact**: Documentation implies `AgentLoop` has a `process_event()` method, but it doesn't exist directly on `AgentLoop`. The event processing happens via `EventProcessor` internally.

### 2. Architecture Document Shows `run_with_prompt()` But Not `run()` Signature

**Severity**: Low

**Location**: `architecture/agent.md` lines 51-52 show:
```
- `run()` - Main event loop
- `run_with_prompt()` - Convenience method for simple prompts
```

Both exist correctly. No issue here - this is verified correct.

### 3. `SubAgentRequest` Description Mismatch

**Severity**: Low (documentation imprecision)

**Location**: `architecture/agent.md` line 135 says SubAgentRequest has fields including "prompt", but the actual struct has `prompt: String`. This is correct.

The doc also says "parent_id" but implementation uses `parent_id: Option<String>`. This is correct.

### 4. Team Coordination Files Not Fully Documented

**Severity**: Medium

**Location**: `architecture/agent.md` mentions "team.rs / teams.rs - Team Coordination" but provides no details about:
- `Team`, `TeamMessage`, `MessageStatus`, `TeamStatus` types
- `TeamManager`, `SharedTaskList`, `IdleNotifier`, `GracefulShutdown` types
- Team-based tools: `team_create`, `send_message`, `list_messages`, `team_status`, `list_teams`

**Impact**: The team coordination system is quite substantial but undocumented in the architecture.

### 5. `BackgroundScheduler::spawn_loop()` task_id Parsing Has Fallback to Random

**Severity**: Low (intentional behavior)

**Location**: `task.rs:228`:
```rust
task_id: task.id.parse().unwrap_or_else(|_| rand::random::<u64>()),
```

**Issue**: While the architecture doc states "Uses `task.id.parse()` to use actual background task ID", there is a fallback to `rand::random()` if parsing fails. This is minor but worth noting.

## Improvement Suggestions

### Priority: High

#### 1. Add Team Coordination Documentation

**File**: `architecture/agent.md`

Add a new section documenting:
- `Team` struct and file-based inbox/outbox communication pattern
- `TeamMessage` and `MessageStatus` types
- `TeamManager` for team lifecycle management
- Team-based tools (`team_create`, `send_message`, etc.)

**Rationale**: The team system is a significant feature with ~680 lines of code across `team.rs` and `teams.rs` but is barely mentioned in the architecture doc.

#### 2. Document `EventProcessor` in Architecture

**File**: `architecture/agent.md`

The `processor.rs` module handles `ChatEvent` processing but is not mentioned at all. Add documentation about:
- `EventProcessor` struct and its accumulation fields
- How tool calls, text deltas, and stop reasons are tracked
- `to_assistant_message()` and `to_tool_messages()` conversion methods

**Rationale**: `EventProcessor` is central to the agent loop's operation.

### Priority: Medium

#### 3. Add Prompt Template System Documentation

**File**: `architecture/agent.md`

Document `prompt.rs` module:
- `render_prompt_template()` function
- `BUILTIN_PROMPTS` map
- `select_provider_prompt()` for model-specific prompts
- `assemble_system_prompt()` for building agent prompts
- `find_instructions_file()` and `find_all_instruction_files()` for instruction loading

**Rationale**: The prompt system is important for agent behavior but poorly documented.

#### 4. Document Mention System

**File**: `architecture/agent.md`

Document `mention.rs` module:
- `parse_mention()` for detecting `@agent` triggers
- `filter_agents()` for agent filtering by name/description
- `MentionContext` struct

**Rationale**: Mention/agent routing is a user-facing feature.

### Priority: Low

#### 5. Clarify `BackgroundScheduler` task_id Parsing

**File**: `architecture/agent.md` line 177

Update to note that `task.id.parse()` is used with a fallback to `rand::random()` if parsing fails:
> "Uses `task.id.parse()` to use actual background task ID (with fallback to random if parse fails)"

#### 6. Add `process_event()` as Alias or Update Documentation

**File**: Either `loop.rs` or `architecture/agent.md`

Option A: Add a delegating method to `AgentLoop`:
```rust
pub async fn process_event(&mut self, event: ChatEvent) {
    // delegate to internal processor
}
```

Option B: Update architecture doc to say "Event processing via `EventProcessor`" instead of `process_event()`.

#### 7. Document Tool Definition Cache Invalidation Limitation

**File**: `architecture/agent.md`

The known limitation about `mcp_tool_count` as proxy for MCP tool changes should be explicitly documented:
> "Tool definition caching uses `mcp_tool_count` as a proxy for MCP tool identity changes. If MCP tools change without count changing, cache may be stale."

#### 8. Add `prompt.rs` to Module List

**File**: `architecture/agent.md`

Line 183-184 lists `prompt.rs` but provides no description. Add brief description:
> ### prompt.rs - Prompt Templates
> Loads and manages prompt templates from files and provides model-specific system prompts.

## Summary

The agent module architecture documentation is **largely accurate**. All major types and methods match between documentation and implementation. The main gaps are:

1. **Missing documentation** for substantial subsystems (Team coordination, Prompt system, Mention system, EventProcessor)
2. **Known limitations** properly documented (tool cache staleness, doom loop detection)
3. **Minor precision issues** (task_id fallback, process_event naming)

The implementation quality is high with proper:
- RAII patterns for resource management (ActiveCountGuard)
- Error handling with proper error propagation
- Event publishing for observability
- Hook integration for extensibility