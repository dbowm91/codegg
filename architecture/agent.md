# Agent Module Architecture

## Overview

The `agent` module (`src/agent/`) is the core orchestration engine for Codegg. It manages the main execution cycle that processes messages from the LLM provider, executes tools via the `ToolRegistry`, handles permissions via `PermissionChecker`, and manages context compaction when token limits are approached.

## Module Structure

```
src/agent/
├── mod.rs          # Agent struct, AgentMode enum, builtin_agents, agent resolution
├── loop.rs         # AgentLoop - main execution cycle
├── processor.rs    # EventProcessor - processes ChatEvents from provider
├── compaction.rs  # ContextTracker, compaction strategies
├── worker.rs       # SubAgentPool, SubAgentSpawner - background task execution
├── router.rs       # ModelRouter - automatic model selection based on task complexity
├── team.rs         # Team, TeamMessage, AgentRole - multi-agent coordination
├── teams.rs        # TeamManager, SharedTaskList, team tools (team_create, send_message, etc.)
├── mention.rs      # @mention parsing and agent filtering
├── prompt.rs       # System prompt assembly, instruction file loading
├── task.rs         # BackgroundTask, BackgroundScheduler
└── prompts/        # Provider-specific system prompts
    ├── anthropic.txt
    ├── beast.txt
    ├── codex.txt
    ├── default.txt
    ├── gemini.txt
    ├── gpt.txt
    ├── kimi.txt
    └── trinity.txt
```

---

## 1. AgentLoop (`loop.rs`)

### Purpose

`AgentLoop` is the main orchestration struct that manages the conversation cycle between the LLM and tools. It handles message streaming, tool execution, permission checks, context tracking, and hook dispatching.

### Key Fields

```rust
pub struct AgentLoop {
    agents: HashMap<String, Agent>,                    // Available agents
    state: AgentLoopState,                             // Turn count, tokens, plan mode
    limits: ExecutionLimits,                           // Max turns, tokens, timeout
    provider: Box<dyn Provider>,                       // LLM provider
    permission_checker: PermissionChecker,             // Permission enforcement
    tool_registry: ToolRegistry,                       // Tool execution
    hook_registry: Option<Arc<HookRegistry>>,          // Hook system
    context_tracker: ContextTracker,                   // Token usage monitoring
    doom_detector: DoomLoopDetector,                   // Repetitive tool call detection
    steering: AtomicBool,                              // User interruption signal
    follow_up_tx: mpsc::UnboundedSender<String>,       // Follow-up prompt sender
    follow_up_rx: mpsc::UnboundedReceiver<String>,     // Follow-up prompt receiver
    config: Config,                                    // App configuration
    question_tx: Option<oneshot::Sender<String>>,      // Question response sender
    question_rx: Option<oneshot::Receiver<String>>,    // Question response receiver
    plugin_service: Option<Arc<PluginService>>,        // WASM plugin hooks
    session_id: String,                                // Current session ID
    mcp_service: Option<Arc<RwLock<McpService>>>,     // MCP client service
    tool_def_cache: Option<ToolDefCache>,              // Cached tool definitions
    deferred_tool_definitions: Vec<ToolDefinition>,    // Deferred tool definitions
    model_router: ModelRouter,                         // Auto-routing
    snapshot_manager: Option<SnapshotManager>,         // File state snapshots
    file_change_rx: broadcast::Receiver<AppEvent>,     // File change events
    usage_store: Option<Arc<UsageStore>>,              // Token usage tracking
    pricing_service: PricingService,                   // Cost calculation
    security_service: SecurityService,                 // Security service
    recent_findings: Vec<SecurityFinding>,             // Recent security findings
    todo_state: Arc<Mutex<TodoState>>,                 // Todo state
    task_state_policy: TaskStatePolicy,                // Task state policy
    todo_pool: Option<SqlitePool>,                     // Todo database pool
    event_store: Option<Arc<EventStore>>,              // Event store for replay
    active_tool_timings: HashMap<String, Instant>,     // Tool execution timings
    execution_policy: Option<ExecutionPolicy>,         // Execution policy
    original_user_prompt: Option<String>,              // Original user prompt
    subagent_pool: Option<Arc<SubAgentPool>>,          // Subagent pool
    max_tool_calls: Option<usize>,                     // Max tool calls limit
    goal_store: Option<Arc<GoalStore>>,                // Goal store
    goal_wall_clock: Mutex<GoalWallClock>,             // Goal wall clock
}
```

### AgentLoopState

Tracks execution state:

```rust
pub struct AgentLoopState {
    pub current_agent: String,    // Active agent name
    pub turn_count: usize,         // Incremented each turn
    pub total_tokens: usize,      // Running token total
    pub start_time: Instant,      // Session start time
    pub plan_mode: bool,          // Plan mode flag
    pub plan_topic: Option<String>, // Plan mode topic
    pub tool_call_count: usize,   // Tool calls this session
    pub last_turn_input_tokens: i64,  // Per-turn tokens for goal accounting
    pub last_turn_output_tokens: i64,
}
```

### ExecutionLimits

Bounds on execution:

```rust
pub struct ExecutionLimits {
    pub max_turns: usize,      // Default: 100
    pub max_tokens: usize,     // Default: 1,000,000
    pub timeout: Duration,     // Default: 600 seconds
}
```

### Main Execution Flow (`run()` method)

```
1. Pre-execution hooks (SessionStart)
2. Apply auto-routing (ModelRouter)
3. Apply agent config (model, temperature, top_p, thinking_budget)
4. Build tool definitions (with MCP tools, plugin hooks)
5. Add system prompt modifications for MiniMax models

Main Loop:
6. Check limits (max turns, tokens, timeout, steering)
7. Pre-turn hooks (AgentStart)
8. Compact if needed (context overflow detection)
9. Harden history (fix orphan tool messages)
10. Stream with retry (provider communication)
11. Process events (EventProcessor)
12. Handle missing structured tool calls (fallback to text parsing)
13. Bootstrap tool for repo tasks (if conditions met)
14. Execute tool calls (permission check → parallel execution)
15. Publish tool results to event bus
16. Detect plan mode changes
17. Post-turn hooks (AgentEnd)
18. Repeat until no tool calls

Post-loop:
19. Drain follow-up prompts
20. SessionEnd hooks
```

### Tool Execution Flow (`execute_tool_calls()`)

1. **Permission Check** (`check_tool_permission`):
   - Empty tool name → deny
   - `question` tool → register with QuestionRegistry, publish QuestionPending event
   - Record tool call in DoomLoopDetector
   - Check doom loop (repeated identical calls)
   - Route to appropriate permission checker (bash/git/general)
   - Auto-accept read-only tools within working directory
   - For `Ask` permissions: register with PermissionRegistry, publish PermissionPending

2. **Snapshot Capture**:
   - Before file-modifying tools (write, edit, replace, multiedit, apply_patch)
   - Drains file change events to only checkpoint this batch

3. **MCP Tool Handling**:
   - Parse MCP tool names (`mcp__server__tool`)
   - Call MCP service via `try_read()` (non-blocking)
   - Fall back gracefully if service is write-locked

4. **Parallel Execution**:
   - Semaphore-controlled concurrency (default max 100)
   - Per-tool timeout via `get_tool_timeout()`
   - Hook dispatch: plugin hook → ToolExecuteBefore → tool execution → ToolExecuteAfter
   - Native tools execute through `ToolRegistry::execute_capture(name, input, ctx)` (which calls `Tool::execute_structured` internally). The `ToolExecutionContext` is built by `AgentLoop::build_tool_execution_context(tc, timeout_ms)`; the `ToolBackendKind` is resolved by `AgentLoop::resolve_native_backend(name)` (most tools → `Native`, `websearch`/`webfetch` → `Mcp` when `[search].backend = eggsearch`, otherwise `BuiltinLegacy`). After the call returns, a `tracing::debug!` line summarises the `ToolProvenance` (backend, implementation, elapsed_ms, trust). MCP tools (`mcp__server__tool`) are dispatched separately through `McpService::call_tool` and never go through `execute_capture`.

5. **Question Handling**:
   - Wait for question_rx (300s timeout)
   - Format answers via `format_question_answers()`
   - Handle cancelled/timeout cases

### Stream Handling

- **Timeout**: 120 seconds for stream initiation
- **Idle Timeout**: 90 seconds between events
- **Retry Logic**: 3 retries with exponential backoff (1s, 2s, 4s, max 30s)
- **Retry Condition**: Only retryable `ProviderError` instances

### Path Redaction

Redacts local paths in tool outputs:
- `/home/[user]`, `/Users/[user]`, `/var/[user]`, `/tmp/[user]`
- `C:\Users\[user]`, `C:\Program Files\[user]`, `C:\Windows\[user]`
- Current working directory and HOME replaced with `[CWD]` and `[HOME]`

### History Hardening

Ensures tool results match tool calls (no orphans):
- Matches `tool_call_id` between assistant tool calls and tool results
- Drops orphan tool messages with debug logging
- Flushes pending tool calls at message boundaries

---

## 2. Agent Struct and AgentMode (`mod.rs`)

### Agent Struct

```rust
pub struct Agent {
    pub name: String,              // "build", "plan", "explore", etc.
    pub description: String,       // Human-readable description
    pub mode: AgentMode,           // Primary, Subagent, or All
    pub mode_name: Option<String>, // Mode label (e.g., "review", "debug")
    pub model: Option<String>,     // Override model
    pub variant: Option<String>,    // Model variant
    pub temperature: Option<f64>,   // Temperature override
    pub top_p: Option<f64>,        // Top-p override
    pub color: Option<String>,     // UI color hint
    pub steps: Option<usize>,      // Max steps limit
    pub system_prompt: Option<String>, // Custom system prompt
    pub permissions: HashMap<String, String>, // Tool/path permissions
    pub hidden: bool,              // Hidden from agent list
    pub thinking_budget: Option<usize>,   // Thinking budget override
    pub reasoning_effort: Option<String>, // Reasoning effort override
}
```

### AgentMode Enum

```rust
pub enum AgentMode {
    Primary,  // Full access agent
    Subagent, // Limited agent (no todo management)
    All,      // Combines multiple agents
}
```

### Permission Ruleset Generation

`Agent::permission_ruleset()` converts the permissions HashMap to a `PermissionRuleset`:

- `"allow"` → `PermissionLevel::Allow`
- `"deny"` → `PermissionLevel::Deny`
- `_*` → `PermissionLevel::Ask`
- Special `"paths"` key creates `PathRule` with `PermissionLevel::Ask`

### Mode Application

- `with_mode()`: Applies a `ModeDefinition` to an agent
- `with_config_mode()`: Applies a `ModeConfig` from config file

### Built-in Agents (9 total)

| Name | Mode | Permissions | Hidden | Purpose |
|------|------|-------------|--------|---------|
| **build** | Primary | None (full access) | No | Default build agent |
| **plan** | Primary | deny: write, edit, bash | No | Read-only planning |
| **general** | Subagent | deny: todowrite | No | Subagent without todos |
| **explore** | All | deny: write, edit | No | Read-only exploration |
| **title** | Subagent | None | Yes | Generate session titles |
| **summary** | Subagent | None | Yes | Generate session summaries |
| **compaction** | Subagent | deny: * (all) | Yes | Context compaction agent |
| **security-review** | Subagent | deny: write, edit | No | Defensive security review |
| **research** | All | deny: image | No | Long-horizon research |

### Agent Resolution (`resolve_agents()`)

Loads agents from multiple sources (in priority order):

1. Built-in agents (base)
2. `~/.config/codegg/agents/*.md` (user config)
3. `.codegg/agents/*.md` (project config)
4. Config file `agent` section
5. Config file `mode` section (creates agents from modes)

Markdown agent files use YAML frontmatter:

```yaml
---
name: CustomAgent
mode: primary
description: Custom agent description
model: gpt-4o
temperature: 0.7
permission:
  bash: allow
  write: deny
---
Agent-specific instructions or markdown content
```

---

## 3. Compaction (`compaction.rs`)

### ContextTracker

Monitors token usage and determines when compaction is needed:

```rust
pub struct ContextTracker {
    current_tokens: usize,      // Running token count
    context_limit: usize,      // Max context (default 128,000)
    threshold: f64,            // Compaction threshold (default 0.85)
    message_token_counts: Vec<usize>, // Per-message token counts
    max_messages: Option<usize>,    // Optional message cap
    max_total_bytes: Option<usize>, // Optional byte cap
    model: Option<String>,          // Model for tokenizer selection
}
```

**Token Estimation**:
- Uses tiktoken for base encoding
- Model-specific multipliers:
  - `Cl100kBase` (GPT models): 1.0x
  - `Claude`: 1.4x
  - `Gemini`: 1.2x
  - `O200kBase`: 1.0x

**Key Methods**:
- `needs_compaction()`: Current tokens > limit × threshold
- `needs_overflow_protection(reserved)`: Current tokens > limit - reserved
- `reset()`: Clears counts for post-compaction

### Compaction Strategies

Three strategies defined in `CompactionStrategy` enum:

1. **`TruncateToolOutputs`**: Truncates tool results > 500 chars to 500 + "...[truncated]"

2. **`DropMiddleMessages`**: Keeps first 2 and last 2 non-system messages

3. **`SummarizeOldTurns`**: Uses LLM to create a summary (async only)

### Compaction Invariants

All compaction must preserve:
1. No orphan `Message::Tool` (every tool result needs matching tool call)
2. No tool-call without its required tool results
3. Relative order of tool call/result pairs
4. `tool_call_id` field unchanged
5. Multi-tool pair order preserved

### Auto-Compaction Flow

```
detect_overflow() → prune_tool_outputs() → reset tracker
                                            ↓
                    if still needs compaction:
                        dispatch SessionCompacting hook
                        if not blocked:
                            select_compaction_strategy()
                            if SummarizeOldTurns + provider: async compaction
                            else: sync compaction (DropMiddleMessages fallback)
                        reset tracker and re-add messages
```

**Auto-compaction selection logic**:
- `has_long_tool_outputs` (>2000 chars) AND `non_system_count > 6` → TruncateToolOutputs
- `non_system_count > 8` → SummarizeOldTurns
- Otherwise → DropMiddleMessages

---

## 4. Worker - SubAgentPool (`worker.rs`)

### SubAgentRequest

```rust
pub struct SubAgentRequest {
    pub task_id: u64,
    pub prompt: String,
    pub agent: String,              // Agent name to use
    pub parent_id: Option<String>,   // Parent session ID
    pub denied_tools: Vec<String>,   // Tools to exclude
    pub allowed_paths: Vec<String>,  // Path restrictions
    pub description: String,
    pub depth: usize,               // Nesting depth (max_depth check)
}
```

### SubAgentResult

```rust
pub struct SubAgentResult {
    pub task_id: u64,
    pub success: bool,
    pub result: String,
}
```

### SubAgentPool

Manages a pool of background worker tasks:

```rust
pub struct SubAgentPool {
    shutdown_tx: broadcast::Sender<()>,
    active_count: Arc<AtomicUsize>,    // Currently running tasks
    max_concurrent: usize,             // Default: 5
    max_depth: usize,                   // Default: 3
    task_store: Arc<TokioMutex<TaskStore>>,
    request_tx: mpsc::Sender<WorkerRequest>,
    // ...
}
```

**Key Methods**:
- `new()`: Creates pool with TaskStore initialization
- `new_with_store()`: Uses provided TaskStore
- `spawner()`: Returns `SubAgentSpawner` for enqueuing tasks
- `shutdown()`: Graceful shutdown with 10x 100ms waits, then abort

### ActiveCountGuard (RAII)

Ensures active count is decremented even on panic:

```rust
struct ActiveCountGuard {
    active_count: Arc<AtomicUsize>,
}
impl Drop for ActiveCountGuard {
    fn drop(&mut self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }
}
```

### SubAgentSpawner

Enqueues subagent requests:

```rust
pub struct SubAgentSpawner {
    pool: SubAgentPool,
}
```

- `send()`: Fire-and-forget with result handler
- `send_async()`: Same as send (both spawn async task)

### Execution Flow (`execute_agent_task()`)

1. Publish `SubagentStarted` event
2. Update task status to `Running`
3. Resolve agent and provider
4. Create `ToolRegistry` (filtering denied tools)
5. Build permission ruleset (allow specific paths, deny others)
6. Create `AgentLoop` with filtered registry
7. Set session ID, enter plan mode if needed
8. Run agent loop with messages
9. Extract text output from events
10. Publish `SubagentCompleted` or `SubagentFailed`
11. Update task store

### Depth Limiting

Prevents infinite nesting:
- `SubAgentSpawner::enqueue_request()` checks `request.depth >= max_depth`
- Returns error if exceeded

---

## 5. Router - ModelRouter (`router.rs`)

### TaskComplexity Enum

```rust
pub enum TaskComplexity {
    Simple,   // Read-only, low cognitive load
    Medium,   // Edit, write, moderate complexity
    Complex,  // Debug, analyze, high cognitive load
}
```

### ModelRouter

Routes requests to appropriate models based on task complexity:

```rust
pub struct ModelRouter {
    enabled: bool,
    simple_model: Option<String>,   // e.g., gpt-4o-mini
    medium_model: Option<String>,    // e.g., gpt-4o
    complex_model: Option<String>,   // e.g., o1-preview
}
```

**Configuration** (`from_config()`):
- `enabled`: `config.auto_route_models.unwrap_or(false)`
- `simple_model`: `config.small_model.clone()`
- `medium_model`: `config.medium_model.clone()`
- `complex_model`: `config.model.clone()` (default model)

### Classification

**By Tool Name**:
- `Simple`: read, cat, ls, glob, list
- `Medium`: edit, write, grep, search
- `Complex`: debug, plan, review, architect, analyze

**By Content** (keyword matching):
- 2+ complex keywords OR "debug this"/"analyze the" → Complex
- 1 complex keyword → Medium
- 2+ medium keywords → Medium
- 2+ simple keywords OR prompt < 50 chars → Simple
- Otherwise → Medium

### Routing

If enabled, `apply_auto_routing()` modifies `request.model` based on classified complexity.

---

## 6. Team Coordination (`team.rs`, `teams.rs`)

### Team (`team.rs`)

File-based message passing between agents:

```rust
pub struct Team {
    name: String,
    agents: Vec<AgentRole>,
    inbox_dir: PathBuf,   // .opencode/team/{team}/inbox/{agent}
    outbox_dir: PathBuf,  // .opencode/team/{team}/outbox/{agent}
    status_file: PathBuf, // .opencode/team/{team}/status.json
}
```

**AgentRole**:
```rust
pub struct AgentRole {
    pub name: String,
    pub instructions: String,
    pub capabilities: Vec<String>,
}
```

**Message Delivery**:
- `send_message()`: Writes JSON to recipient's inbox
- `deliver_messages()`: Reads and marks messages as delivered
- `mark_completed()`: Updates message status to Completed

### TeamManager (`teams.rs`)

In-memory team management:

```rust
pub struct TeamManager {
    teams: RwLock<HashMap<String, Arc<Team>>>,
    team_configs: RwLock<HashMap<String, TeamConfig>>,
    shutdown_txs: RwLock<HashMap<String, broadcast::Sender<()>>>,
}
```

**Operations**:
- `create_team()`: Creates team and registers shutdown sender
- `get_team()`: Lookup by name
- `list_teams()`: All team names
- `shutdown_team()`: Sends shutdown signal, removes from maps
- `send_message()`: Delegates to Team
- `deliver_messages()`: Delegates to Team
- `get_team_status()`: Delegates to Team

### Team Tools

Implements tool interface for team operations:

- **`team_create`**: Create a team with agents
- **`send_message`**: Send message to team agent
- **`list_messages`**: List pending messages for agent
- **`team_status`**: Get team status
- **`list_teams`**: List all teams

### SharedTaskList

Task dependency tracking:

```rust
pub struct SharedTaskList {
    tasks: RwLock<HashMap<String, TaskDependency>>,
    completed: RwLock<HashMap<String, bool>>,
}
```

- `add_task(task_id, depends_on)`: Register task with dependencies
- `mark_completed(task_id)`: Mark task done
- `is_completed(task_id)`: Check completion status
- `can_start(task_id)`: All dependencies satisfied?
- `get_pending_tasks()`: Non-completed tasks

### IdleNotifier

Agent idle notification:

```rust
pub struct IdleNotifier {
    listeners: RwLock<HashMap<String, broadcast::Sender<()>>>,
}
```

- `register(agent_name)`: Returns receiver for idle notifications
- `notify_idle(agent_name)`: Send notification

### GracefulShutdown

Coordinates team shutdown:

```rust
pub struct GracefulShutdown {
    shutdown_tx: broadcast::Sender<TeamShutdownSignal>,
    teams: Arc<TeamManager>,
}
```

---

## 7. EventProcessor (`processor.rs`)

Accumulates streaming ChatEvents:

```rust
pub struct EventProcessor {
    accumulated_text: String,
    accumulated_reasoning: String,
    tool_calls: Vec<ToolCall>,
    tool_results: Vec<(String, String)>,
    stop_reason: Option<String>,
    input_tokens: usize,
    output_tokens: usize,
    cached_tokens: Option<usize>,
    is_complete: bool,
}
```

**Processing**:
- `TextDelta` → append to `accumulated_text`
- `ReasoningDelta` → append to `accumulated_reasoning`
- `ToolCall` → add to `tool_calls`
- `ToolResult` → add to `tool_results`
- `Finish` → set stop_reason, tokens, `is_complete = true`

**Output Methods**:
- `to_assistant_message()`: Converts accumulated content to `Message::Assistant`
- `to_tool_messages()`: Converts tool_results to `Vec<Message::Tool>`

---

## 8. Hooks Integration

### Hook Types Dispatched

| Hook Event | Plugin Service Method | Purpose |
|------------|----------------------|---------|
| `SessionStart` | `dispatch_session_start()` | Before main loop |
| `AgentStart` | `dispatch_agent_start()` | Before each turn |
| `ToolExecuteBefore` | `dispatch_tool_execute_before()` | Before each tool |
| `ToolExecuteAfter` | `dispatch_tool_execute_after()` | After each tool |
| `AgentEnd` | `dispatch_agent_end()` | After each turn |
| `SessionEnd` | `dispatch_session_end()` | After main loop |
| `SessionCompacting` | `dispatch_session_compacting()` | Before compaction |

### Plugin Service Hooks

Tool definition hooks:
- `dispatch_tool_definition()`: Modify tool list before sending to model

Tool execution hooks:
- `dispatch_tool_execute_before()`: Can block tool execution
- `dispatch_tool_execute_after()`: Post-execution processing

---

## 9. Goal Runtime Integration (`goal/runtime.rs`)

### Purpose

The goal runtime provides autonomous long-horizon work. When a user sets a goal via `/goal set <objective>`, the agent loop can continue working across multiple turns and sessions, with budget enforcement and automatic continuation.

### AgentLoop Goal Fields

```rust
pub goal_store: Option<Arc<GoalStore>>,   // SQLite goal persistence
pub goal_wall_clock: Mutex<GoalWallClock>, // Wall-clock tracking for budget
```

### Turn Lifecycle with Goals

```
Turn ends
  → ChatEvent::Finish captures input_tokens/output_tokens
  → publish_agent_finished() emits AgentFinished
  → account_goal_for_turn() advances usage counters
  → maybe_continue_goal() decides:
      Continue → queue build_continuation_prompt(), drain follow_up
      BudgetLimited → queue build_budget_wrap_up_prompt(), stop
      Terminal/NoGoal → exit
```

### Per-Turn Token Tracking

`AgentLoopState` tracks `last_turn_input_tokens` and `last_turn_output_tokens`, written on each `ChatEvent::Finish` inside `stream_once(&mut self, ...)`. These are reset to 0 before each continuation turn so deltas are per-turn, not cumulative.

### Continuation Loop Safety

`maybe_continue_goal()` caps at `MAX_CONTINUATIONS = 32` per `run()` invocation to prevent infinite loops.

### Prompt Steering

`goal_and_todos_contract()` in `agent/prompt.rs` instructs the model about two planning surfaces:
- **Todos** (`todo` tool): in-flight steps the user can check off
- **Goals** (`goal_set`/`goal_update_progress`/`goal_request_completion`): long-horizon work spanning sessions

See [goal.md](goal.md) for full architecture.

---

## 10. Prompt Assembly (`prompt.rs`)

### Provider Prompt Selection

Selects model-specific system prompt:

```rust
pub fn select_provider_prompt(model_id: &str) -> &'static str {
    // GPT-4, O1, O3, O4 → beast.txt
    // Codex → codex.txt
    // GPT → gpt.txt
    // Gemini → gemini.txt
    // Claude, Sonnet, Opus, Haiku → anthropic.txt
    // Trinity → trinity.txt
    // Kimi → kimi.txt
    // Default → default.txt
}
```

### System Prompt Assembly

`assemble_system_prompt()` builds system prompt from:
1. Agent's custom system prompt (if any)
2. Agent name and description
3. Available tools list
4. Available skills list
5. Model name (if set)
6. Config instructions
7. Custom instructions (passed at runtime)

### Instruction File Loading

Primary instruction files (via `INSTRUCTION_FILES` constant):
1. `AGENTS.md`
2. `CLAUDE.md`
3. `CONTEXT.md`

Secondary/fallback paths (via `find_instructions_file()`):
1. `.codegg/instructions.md` (project)
2. `INSTRUCTIONS.md` (project root)
3. `~/.config/codegg/instructions.md` (global)

Searches from CWD to git root, plus config dir.

Remote URLs in config instructions are fetched asynchronously.

### Subagent Output Contracts

`subagent_output_contract()` in `prompt.rs` returns role-specific output format guidance. These contracts define the expected shape of subagent responses to improve result parsing and quality.

```rust
pub fn subagent_output_contract(role: &str) -> &'static str {
    match role {
        "explore" | "explorer" => "Output contract: Return a compact report with: files examined, key symbols/modules found, relevant relationships, and uncertainties. Do not include raw file contents.",
        "review" | "reviewer" => "Output contract: Return findings by severity (critical/high/medium/low/info). For each: file path, line number if applicable, title, rationale, and suggested patch scope. Prioritize correctness and security over style.",
        "debug" => "Output contract: Return: commands/logs that revealed the issue, failure signature, root-cause candidates ranked by likelihood, and next experiment to try.",
        "test" => "Output contract: Return: tests added or run, pass/fail status per test, coverage gaps identified, and any flaky or skipped tests.",
        "security" | "security_reviewer" => "Output contract: Return findings with: severity, confidence, title, file path, line, evidence (code locations + risk markers + call paths), reasoning, recommendation, and suggested tests. Return review prompts (marker-only) separately from evidence-based findings. Do not inflate severity without exploitability evidence.",
        "planner" => "Output contract: Return: implementation plan with ordered steps, estimated complexity per step, dependencies between steps, files to create/modify, and verification criteria.",
        "executor" | _ => "Output contract: Return a compact summary with: work performed, key findings, files touched, and suggested next steps.",
    }
}
```

The output contract is injected into both `assemble_system_prompt_with_profile()` (used with model profiles) and `base_prompt_parts()` (used in `load_agent_prompt()` for production paths). It is appended after the role contract, giving subagents explicit guidance on response format.

---

## 10. Background Tasks (`task.rs`)

### BackgroundTask

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
```

- `is_expired()`: Created > 3 days ago
- `should_fire()`: Since last_run >= interval

### BackgroundScheduler

Manages periodic task execution:

```rust
pub struct BackgroundScheduler {
    tasks: Arc<RwLock<Vec<BackgroundTask>>>,
    shutdown_tx: broadcast::Sender<()>,
    callback: Option<TaskCallback>,
    pool: Option<SqlitePool>,
}
```

- `add()`: Add task (optionally persist to DB)
- `remove()`: Remove task
- `tick()`: Return tasks that should fire now
- `spawn_loop()`: Start background loop using SubAgentPool

### Duration Parsing

Supports: `30s`, `5m`, `5min`, `1h`, `1d`

---

## 11. Mention Parsing (`mention.rs`)

### MentionContext

```rust
pub struct MentionContext {
    pub trigger_pos: usize,  // Position of @ in input
    pub query: String,       // Full mention including @ (e.g., "@build")
}
```

### Parsing Rules

- Must be at start of input or after whitespace
- `@` must not be part of another word (e.g., `user@host`)
- Query includes `@` prefix (e.g., `@build`)

### Agent Filtering

`filter_agents()` matches by name or description (case-insensitive).

---

## 12. Interaction with Other Modules

### Provider Module

- `AgentLoop` holds `Box<dyn Provider>`
- `stream()` method for LLM communication
- `ChatEvent` types for streaming responses
- Tool definitions passed in `ChatRequest`

### Tool Module

- `ToolRegistry` for tool lookup and execution
- `Tool` trait: `name()`, `description()`, `parameters()`, `execute()`
- 27 built-in tools (including ImageTool)
- Tool filtering based on model capabilities and plan mode

### Permission Module

- `PermissionChecker` for tool access control
- `DoomLoopDetector` for repetitive call detection
- `PermissionRegistry` for pending permission handling
- `QuestionRegistry` for question tool handling

### Bus Module

- `GlobalEventBus` for event publishing
- Key events:
  - `ToolCallStarted`, `ToolResult` - tool lifecycle
  - `TextDelta`, `ReasoningDelta` - streaming text
  - `AgentFinished` - session completion
  - `PermissionPending`, `QuestionPending` - pending user input
  - `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` - subagent lifecycle

### Session Module

- `UsageStore` for tracking token usage and cost
- `SessionStore` for session persistence
- `snapshot` integration for file state capture

### Config Module

- `Config` struct for all settings
- Agent config, mode config, compaction config
- Server config for timeouts and limits

### Plugin Module

- `PluginService` for WASM hook dispatch
- `HookRegistry` for hook management

---

## 13. Key Implementation Details

### Tool Definition Caching

`ToolDefCache` tuple:
```rust
(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)
// model, plan_mode, lsp_enabled, mcp_count, perm_ver, definitions
```

Invalidated when any component changes. MCP tool count used as proxy for changes (limitation noted in code).

### File-Modifying Tool Detection

```rust
fn is_file_modifying_tool(name: &str) -> bool {
    matches!(name, "write" | "edit" | "replace" | "multiedit" | "apply_patch")
}
```

Snapshots captured before these tools execute.

### Doom Loop Detection

Counts identical tool calls. If threshold exceeded (default 20, configurable):
- Tool is denied even if permission would allow it
- Message indicates potential doom loop

### Auto-Accept Read-Only Tools

Read-only tools (`read`, `glob`, `grep`, `list`, `webfetch`, `websearch`, `codesearch`) that target paths within the working directory are auto-accepted without user prompt.

### MiniMax Model Handling

Models containing "minimax" get special system prompt modification:
```
Tool-use contract: For repository/file/code/doc tasks, emit structured tool calls before giving conclusions.
```

Also avoids late system messages for MiniMax.

### Parallel Tool Execution

- Semaphore-controlled (max configurable, default 100)
- Per-tool timeout via `get_tool_timeout()`
- MCP tools executed separately from regular tools

### Follow-up Prompt Handling

- `follow_up_sender()` returns channel for queuing prompts
- `drain_follow_up()` processes queued follow-ups
- Non-blocking `try_recv()` - late follow-ups require new `run()` call

---

## 14. Snapshot Integration

### Snapshot Capture Flow

1. **Pre-change snapshot** (`capture_snapshot_if_needed()`):
   - Before file-modifying tools
   - Drains file change events to only capture current batch

2. **Incremental snapshot** (`capture_incremental_snapshot_if_needed()`):
   - After file-modifying tools complete
   - Captures file changes since last snapshot

### File Change Events

`FileChanged` events are drained from the event bus subscription:
- `path`: File path
- `action`: Change type
- `old_content`: Previous content (if available)

---

## 15. ExecutionPolicy (`policy.rs`)

### Purpose

`ExecutionPolicy` is a per-turn execution configuration derived from the active model's `ResolvedModelProfile`. It centralizes parameters that control tool exposure, context budgeting, parallelism, and behavioral toggles — ensuring each turn adapts to the model's capabilities.

### Struct

```rust
pub struct ExecutionPolicy {
    pub model: String,                          // Model identifier
    pub prompt_profile: PromptProfileKind,      // Profile classification
    pub context_window: usize,                  // Max context tokens (default 128k)
    pub compaction_threshold: f64,              // When to trigger compaction (default 0.85)
    pub reserved_output_tokens: usize,          // Tokens reserved for output (default 12k)
    pub max_tool_result_tokens: usize,          // Max tokens per tool result (default 8k)
    pub max_parallel_tools: usize,              // Max concurrent tool executions (default 10)
    pub expose_tool_search: bool,               // Always true
    pub initial_tool_mode: ToolExposureMode,    // Tool exposure filter mode
    pub allow_bootstrap_tool: bool,             // Whether bootstrap tool is enabled
    pub allow_post_tool_continue_nudge: bool,   // Whether post-tool nudge is enabled
    pub prefer_user_control_messages: bool,     // Use user-role for control messages
    pub supports_late_system_messages: bool,    // Provider supports late system messages
    pub disabled_tools: Option<Vec<String>>,    // Tools to remove from exposure
    pub task_state_policy: TaskStatePolicy,     // Todo injection behavior
}
```

### Construction

Created via `ExecutionPolicy::from_profile(profile, config)`. Config values override profile defaults when present (e.g., `config.compaction.max_tokens` overrides `profile.context_window`).

### Defaults by Profile

| Profile | Context | Threshold | Reserved | Max Result | Max Parallel | Tool Mode |
|---------|---------|-----------|----------|------------|--------------|-----------|
| FrontierReasoning/FrontierExecutor | 128k | 0.85 | 12k | 8k | 10 | Curated |
| LongContextPlanner | 512k | 0.70 | 16k | 8k | 8 | Curated |
| FastExecutor/ToolFragile | 128k | 0.70 | 8k | 4k | 2 | MinimalWithDiscovery |
| LocalStrict | 32k | 0.65 | 4k | 2k | 1 | MinimalWithDiscovery |
| Reviewer | 128k | 0.80 | 10k | 6k | 4 | Curated |
| Summarizer | 64k | 0.75 | 4k | 2k | 1 | MinimalWithDiscovery |
| Default | 128k | 0.85 | 10k | 6k | 6 | Full |

---

## 16. Tool Exposure Modes (`policy.rs`)

### ToolExposureMode Enum

Controls which tools are visible to the LLM for a given turn:

```rust
pub enum ToolExposureMode {
    Full,
    Curated,
    MinimalWithDiscovery,
}
```

### Mode Definitions

| Mode | Tools Included | Use Case |
|------|---------------|----------|
| **Full** | All registered tools | Default/unknown models |
| **Curated** | read, list, grep, glob, codesearch, edit, apply_patch, bash, git, diff, todoread, todowrite, question, tool_search, skill | Frontier reasoning/executor models, long-context planners, reviewers |
| **MinimalWithDiscovery** | read, list, grep, codesearch, edit, apply_patch, bash, question, todowrite, todoread, tool_search | Fast/fragile models, local strict, summarizers |

### Application

Applied in `AgentLoop::apply_tool_exposure_filter()` during `build_tool_definitions()`:

1. Match `policy.initial_tool_mode` → filter tool definitions to the allowed set
2. Then apply `policy.disabled_tools` → remove any additional tools the profile disables
3. Returns filtered definitions before MCP tools are appended

The `allow_bootstrap_tool` flag is `true` for `MinimalWithDiscovery` or when `profile.requires_explicit_tool_contract` is set.

---

## 17. Profile-Aware Tool Filtering (`policy.rs`)

### `filter_tool_definitions_for_profile()`

A standalone function that removes tools listed in `ResolvedModelProfile.disabled_tools` from the tool definition list. Called in subagent execution flows (e.g., `agent_loop.rs:1859`) to apply per-model tool restrictions.

```rust
pub fn filter_tool_definitions_for_profile(
    defs: Vec<ToolDefinition>,
    profile: &ResolvedModelProfile,
) -> Vec<ToolDefinition>
```

This is separate from `apply_tool_exposure_filter()` (which handles exposure mode). The two are applied in sequence:

- **`apply_tool_exposure_filter()`**: Mode-based filter (Full/Curated/MinimalWithDiscovery) + disabled_tools
- **`filter_tool_definitions_for_profile()`**: Standalone disabled_tools filter for subagent/provider request construction

---

## 18. ContextFrame (`context_frame.rs`)

### Purpose

`ContextFrame` is a deterministic context snapshot injected into the conversation after compaction. It preserves the session's essential state across context window resets, ensuring the LLM retains awareness of goals, progress, and open issues.

### Struct

```rust
pub struct ContextFrame {
    pub user_goal: Option<String>,          // Original user prompt
    pub current_task: Option<String>,       // In-progress todo item
    pub constraints: Vec<String>,           // Known constraints
    pub decisions: Vec<String>,             // Decisions made so far
    pub touched_files: Vec<String>,         // Files modified in session
    pub commands_run: Vec<String>,          // Commands executed
    pub test_results: Vec<String>,          // Test outcomes
    pub unresolved_errors: Vec<String>,     // Open issues
    pub security_findings: Vec<String>,     // Security findings (capped at 5)
    pub next_steps: Vec<String>,            // Pending todo items (capped at 3)
}
```

### Population

Built by `AgentLoop::build_context_frame()` which populates fields from:

- `user_goal` ← `self.original_user_prompt`
- `current_task` ← First in-progress todo item
- `next_steps` ← Up to 3 pending todo items
- `security_findings` ← Up to 5 recent findings from `self.recent_findings`
- Other fields: Currently empty vectors (populated by future enhancements)

### Injection

After compaction completes (`compact_if_needed()` in `loop.rs:1780`):

1. `build_context_frame()` constructs the frame
2. If non-empty, `to_control_text()` renders it as a human-readable block
3. `push_control_instruction()` injects it as a control message into the message history
4. Optionally followed by a todo reminder if `task_state_policy.inject_after_compaction` is set

### Output Format

`to_control_text()` produces lines like:

```
Current session context:
- Goal: Fix the failing test
- Active task: Investigate test_output
- Touched files: src/main.rs, src/lib.rs
- Commands/tests: cargo test
- Test results: 2 passed, 0 failed
- Security findings: [SSRF] Internal IP access attempted
- Next steps: Fix regex; Add integration test
```

---

## 19. SubAgentReport (`worker.rs`)

### Purpose

`SubAgentReport` is a typed, structured result from subagent execution. It provides a richer alternative to the raw `result: String` in `SubAgentResult`, enabling programmatic consumption of subagent outputs.

### Struct

```rust
pub struct SubAgentReport {
    pub summary: String,                     // High-level summary
    pub files_examined: Vec<String>,         // Files inspected
    pub commands_run: Vec<String>,           // Commands executed
    pub findings: Vec<SubAgentFinding>,      // Structured findings
    pub next_steps: Vec<String>,             // Recommended follow-ups
    pub confidence: Option<String>,          // Confidence level (e.g., "high", "medium")
}
```

### SubAgentFinding

```rust
pub struct SubAgentFinding {
    pub severity: Option<String>,   // "critical", "high", "medium", "low", "info"
    pub file: Option<String>,       // Source file path
    pub line: Option<u32>,          // Line number
    pub title: String,              // Finding title
    pub rationale: String,          // Explanation
}
```

### SubAgentResult Integration

`SubAgentResult` wraps the report:

```rust
pub struct SubAgentResult {
    pub task_id: u64,
    pub success: bool,
    pub result: String,
    pub report: Option<SubAgentReport>,
}
```

Construction methods:
- `success(task_id, result)` — report is `None`
- `success_with_report(task_id, result, report)` — report is `Some`

### `to_compact_text()`

Serializes the report to a compact text format:

```
Summary text
Files: file1.rs, file2.rs
Commands: cargo test, cargo build
[critical] Title (file.rs:42): Rationale
[medium] Another finding: Rationale
Next: Add tests; Fix regex
Confidence: high
```

Used for including structured subagent output in parent agent context.

---

## Summary

The `agent` module is the central coordinator for Codegg's AI-powered task execution. It orchestrates:

1. **Message handling** via `AgentLoop` with streaming provider communication
2. **Tool execution** via `ToolRegistry` with permission enforcement
3. **Context management** via `ContextTracker` with automatic compaction
4. **Background tasks** via `SubAgentPool` for parallel work
5. **Multi-agent teams** via `Team` and `TeamManager` with file-based messaging
6. **Model routing** via `ModelRouter` for automatic model selection
7. **Execution policies** via `ExecutionPolicy` for per-turn model-aware configuration
8. **Tool exposure filtering** via `ToolExposureMode` and profile-aware disabled tool lists
9. **Post-compaction context** via `ContextFrame` for deterministic state preservation
10. **Structured subagent results** via `SubAgentReport` for typed, parseable outputs
11. **Hook system** integration for extensibility

The module maintains strict boundaries with other components through clear interfaces (Provider trait, Tool trait, PermissionChecker), enabling testability and modularity.
