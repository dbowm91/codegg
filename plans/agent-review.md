# Agent Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| AgentLoop struct has `agents: HashMap<String, Agent>` | VERIFIED | loop.rs:549 |
| AgentLoop struct has `state: AgentLoopState` | VERIFIED | loop.rs:550 |
| AgentLoop struct has `limits: ExecutionLimits` | VERIFIED | loop.rs:551 |
| AgentLoop struct has `provider: Box<dyn Provider>` | VERIFIED | loop.rs:552 |
| AgentLoop struct has `permission_checker: PermissionChecker` | VERIFIED | loop.rs:553 |
| AgentLoop struct has `tool_registry: ToolRegistry` | VERIFIED | loop.rs:554 |
| AgentLoop struct has `hook_registry: Option<Arc<HookRegistry>>` | VERIFIED | loop.rs:555 |
| AgentLoop struct has `context_tracker: ContextTracker` | VERIFIED | loop.rs:556 |
| AgentLoop struct has `doom_detector: DoomLoopDetector` | VERIFIED | loop.rs:557 |
| AgentLoop struct has `steering: AtomicBool` | VERIFIED | loop.rs:558 |
| AgentLoop struct has `follow_up_tx/rx` | VERIFIED | loop.rs:559-560 |
| AgentLoop struct has `config: Config` | VERIFIED | loop.rs:561 |
| AgentLoop struct has `question_tx/rx: Option<oneshot>` | VERIFIED | loop.rs:562-563 |
| AgentLoop struct has `plugin_service: Option<Arc<PluginService>>` | VERIFIED | loop.rs:564 |
| AgentLoop struct has `session_id: String` | VERIFIED | loop.rs:565 |
| AgentLoop struct has `mcp_service: Option<Arc<RwLock<McpService>>>` | VERIFIED | loop.rs:566 |
| AgentLoop struct has `tool_def_cache: Option<ToolDefCache>` | VERIFIED | loop.rs:567 |
| AgentLoop struct has `model_router: ModelRouter` | VERIFIED | loop.rs:568 |
| AgentLoop struct has `snapshot_manager: Option<SnapshotManager>` | VERIFIED | loop.rs:569 |
| AgentLoop struct has `file_change_rx: broadcast::Receiver<AppEvent>` | VERIFIED | loop.rs:570 |
| `run()` method exists | VERIFIED | loop.rs:1242 |
| `run_with_prompt()` method exists | VERIFIED | loop.rs:2066 |
| `check_tool_permission()` method exists | VERIFIED | loop.rs:389 |
| `compact_if_needed()` method exists | VERIFIED | loop.rs:1130 |
| `build_tool_definitions()` method exists | VERIFIED | loop.rs:1008 |
| `execute_tool_calls()` method exists | VERIFIED | loop.rs:1621 |
| `stream_with_retry()` method exists | VERIFIED | loop.rs:792 |
| `process_event()` method exists | INCORRECT | No such method exists in AgentLoop |
| CompactionStrategy has TruncateToolOutputs | VERIFIED | compaction.rs:219 |
| CompactionStrategy has SummarizeOldTurns | VERIFIED | compaction.rs:220 |
| CompactionStrategy has DropMiddleMessages | VERIFIED | compaction.rs:221 |
| `detect_overflow()` function exists | VERIFIED | compaction.rs:464 |
| `prune_tool_outputs()` function exists | VERIFIED | compaction.rs:490 |
| `compact_messages_sync()` function exists | VERIFIED | compaction.rs:228 |
| `compact_messages_async()` function exists | VERIFIED | compaction.rs:266 |
| `auto_compact_async()` function exists | VERIFIED | compaction.rs:621 |
| `llm_summarize()` function exists | VERIFIED | compaction.rs:344 |
| ModelRouter has simple/medium/complex_model | VERIFIED | router.rs:22-25 |
| TaskComplexity enum (Simple/Medium/Complex) | VERIFIED | router.rs:4-8 |
| SubAgentPool has max_concurrent (default: 5) | VERIFIED | worker.rs:63,89 |
| SubAgentPool has max_depth (default: 3) | VERIFIED | worker.rs:64,94 |
| RAII guard pattern (ActiveCountGuard) | VERIFIED | worker.rs:224-241 |
| SubagentStarted event published | VERIFIED | worker.rs:472 |
| SubagentProgress event published | VERIFIED | worker.rs:485 |
| SubagentCompleted event published | VERIFIED | worker.rs:516 |
| SubagentFailed event published | VERIFIED | worker.rs:496,531 |
| start_workers() removed | VERIFIED | Confirmed not in worker.rs |
| BackgroundScheduler uses task.id for task_id | VERIFIED | task.rs:228 |
| ToolExecuteBefore hook at loop.rs:1764 | VERIFIED | loop.rs:1764 |
| ToolExecuteAfter hook at loop.rs:1806 | VERIFIED | loop.rs:1806 |
| Agent struct fields match | VERIFIED | mod.rs:27-42 |
| AgentMode enum (Primary, Subagent, All) | VERIFIED | mod.rs:46-51 |
| AgentLoopState fields match | VERIFIED | loop.rs:523-530 |
| team.rs / teams.rs exist | VERIFIED | Both files exist |
| prompt.rs handles templates | VERIFIED | prompt.rs:9-16 |
| processor.rs handles ChatEvents | VERIFIED | processor.rs |
| SubAgentPool workers start in constructors | VERIFIED | worker.rs:124,175 |

## Bugs Found

### Critical

1. **Missing `@agent` mention routing implementation**
   - Location: mention.rs
   - The architecture doc mentions "Handles `@agent` mentions for routing to specific agents" for mention.rs
   - The module provides `parse_mention()` and `filter_agents()` functions
   - However, there is no integration in `AgentLoop::run()` to actually route messages to different agents based on mentions
   - The `run()` method never calls `parse_mention()` or checks for agent mentions in user messages
   - This feature appears to be declared but not implemented

### Medium

2. **Hard-coded model in `llm_summarize()`**
   - Location: compaction.rs:414
   - `llm_summarize()` hardcodes `"gpt-4o-mini"` for summarization
   - Should use the configured model or a config-specified summarization model
   - Could result in using an expensive model for simple summarization tasks

3. **Permission version hash computed on every call**
   - Location: loop.rs:727-737
   - `permission_version()` serializes the entire permission config to JSON and computes a hash on every call
   - Called during tool definition caching (line 1031)
   - Should cache the hash and recompute only when permissions change

4. **MCP tool count as sole cache invalidation**
   - Location: loop.rs:1033-1037 (comment acknowledges limitation)
   - Tool definition cache uses `mcp_tool_count` as proxy for MCP tool changes
   - If MCP tools change without count changing (same number, different tools), cache is stale
   - MCP service would need to expose a version/hash for precise invalidation

5. **Tool definition cache uses `try_read()` on mcp_service**
   - Location: loop.rs:1021-1028
   - Uses `try_read()` to avoid blocking, but could race with MCP tool updates
   - Comment acknowledges this is intentional but it's still a potential consistency issue

### Low

6. **BackgroundScheduler task_id fallback to random**
   - Location: task.rs:228
   - Uses `task.id.parse().unwrap_or_else(|_| rand::random())` instead of proper error handling
   - If `task.id` is not a valid u64, silently generates random ID
   - Should propagate error or use a deterministic fallback

7. **Context tracker reset during compaction may lose track**
   - Location: loop.rs:1236-1237
   - After compaction, `context_tracker.reset()` is called followed by `add_messages()`
   - If `add_messages()` fails partway through, tracker is in inconsistent state
   - No error handling for partial failures

## Improvement Suggestions

### Performance

1. **Cache permission version hash**
   - Currently recomputes JSON serialization and hash on every `build_tool_definitions()` call
   - Should cache and invalidate only when config changes

2. **Add MCP tool version/hash for cache invalidation**
   - Rather than using count as proxy, MCP service should expose a version or content hash
   - Would enable precise cache invalidation without blocking reads

3. **Consider parallel tool execution tuning**
   - Currently joins all futures at once (loop.rs:1836)
   - For large batches, could use chunked execution to limit memory pressure

4. **Snapshot capture optimization**
   - `capture_snapshot_if_needed()` and `capture_incremental_snapshot_if_needed()` both exist
   - Consider combining or reducing frequency of incremental captures

### Correctness

5. **Implement `@agent` mention routing**
   - The architecture claims this feature exists
   - Should either implement it or remove the claim from docs

6. **Use configurable model for summarization**
   - `llm_summarize()` hardcodes `gpt-4o-mini`
   - Should respect `config.compaction.summarize_model` or similar

7. **Handle parse errors in BackgroundScheduler**
   - `task.id.parse()` failure silently uses random
   - Should either fail the task or use a proper default

### Maintainability

8. **Document ToolDefCache tuple structure**
   - The `ToolDefCache` type alias (loop.rs:60-67) uses a tuple with positional fields
   - Consider making it a named struct for clarity

9. **Add integration test for agent mention routing**
   - If implemented, should have test coverage
   - Would catch regressions in the feature

10. **Extract constants to Config**
    - Various hardcoded values: `MAX_RETRY_DELAY`, `STREAM_IDLE_TIMEOUT`, etc.
    - Should be configurable via `Config`

## Priority Actions (top 5 items to fix)

1. **Implement `@agent` mention routing** (Critical - Feature gap)
   - Either integrate `parse_mention()` into `AgentLoop::run()` or document that it's not implemented
   - Update architecture doc to reflect actual state

2. **Add model configuration to `llm_summarize()`** (Medium - Correctness)
   - Use config-specified model instead of hardcoded `gpt-4o-mini`
   - Prevents unexpected costs/compatibility issues

3. **Cache permission version hash** (Medium - Performance)
   - Avoid repeated JSON serialization in hot path
   - Cache invalidation on config change

4. **Fix BackgroundScheduler task_id error handling** (Low - Correctness)
   - Replace silent `rand::random()` fallback with proper error handling
   - Use deterministic ID or propagate error

5. **Document ToolDefCache structure** (Low - Maintainability)
   - Convert tuple type alias to named struct
   - Makes cache invalidation logic clearer