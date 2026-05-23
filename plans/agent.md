# Agent Architecture Review

## Architecture Document
- Path: architecture/agent.md

## Source Code Location
- src/agent/

## Verification Summary
Pass

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| AgentLoop struct fields (agents, state, limits, provider, permission_checker, etc.) | Pass | All 20+ fields match exactly |
| AgentLoopState has current_agent, turn_count, total_tokens, start_time, plan_mode, plan_topic | Pass | All 6 fields present at line 523-530 |
| ExecutionLimits has max_turns, max_tokens, timeout | Pass | Present at lines 532-536 |
| Key methods: run(), run_with_prompt(), process_event(), check_tool_permission(), compact_if_needed(), build_tool_definitions(), execute_tool_calls(), stream_with_retry() | Pass | All exist, run() at line 1248, run_with_prompt() at line 2072, check_tool_permission() at line 389, compact_if_needed() at line 1130, build_tool_definitions() at line 1008, execute_tool_calls() at line 1627, stream_with_retry() at line 792 |
| ContextTracker, CompactionStrategy, detect_overflow(), prune_tool_outputs(), compact_messages_sync(), compact_messages_async(), auto_compact_async(), llm_summarize() | Pass | All present in compaction.rs |
| ModelRouter struct with enabled, simple_model, medium_model, complex_model | Pass | Present in router.rs lines 21-26 |
| TaskComplexity enum with Simple, Medium, Complex | Pass | Present at router.rs lines 3-8 |
| classify(), route_model(), is_enabled() methods on ModelRouter | Pass | Present at router.rs lines 53, 154, 57 |
| SubAgentPool struct fields (shutdown_tx, active_count, max_concurrent, max_depth, task_store, workers, request_tx, agents, provider_registry, config, session_store, cancel_token, active_handles, pool) | Pass | All 14 fields match lines 60-75 |
| SubAgentRequest fields (task_id, prompt, agent, parent_id, denied_tools, allowed_paths, description, depth) | Pass | All 9 fields at lines 18-28 |
| SubAgentResult fields (task_id, success, result) | Pass | Present at lines 30-35 |
| SubAgentSpawner exists with send()/send_async() | Pass | Present at lines 361-456 |
| RAII guard ActiveCountGuard pattern | Pass | Implemented at lines 224-241 in worker.rs |
| Subagent events (SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed) | Pass | Published at lines 472-536 |
| BackgroundTask struct (id, interval, message, last_run, created_at, session_id, db_id) | Pass | All 7 fields at lines 31-39 in task.rs |
| BackgroundScheduler struct | Pass | Present at lines 90-95 in task.rs |
| BackgroundScheduler uses task.id.parse() for task_id | Pass | Line 228 in task.rs |
| start_workers() method removed | Pass | No longer exists in worker.rs |
| Agent struct fields (name, description, mode, mode_name, model, variant, temperature, top_p, color, steps, system_prompt, permissions, hidden) | Pass | All 13 fields in mod.rs lines 28-42 |
| AgentMode enum (Primary, Subagent, All) | Pass | Present at lines 44-51 in mod.rs |
| ToolExecuteBefore/After hooks invoked in execute_tool_calls() | Pass | Both invoked at lines 1770 and 1812 (documented as 1764/1806 - close) |
| Tool definition cache uses mcp_tool_count, permission_version | Pass | Lines 1044-1052 verify cache key includes these |
| Events published: SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed, TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, AgentFinished, PermissionPending, QuestionPending | Pass | All published via GlobalEventBus |
| AgentStarted/AgentEnded/CompactionStarted/CompactionEnded NOT published | Pass | Confirmed - hooks handle lifecycle events instead |
| ModelRouter from_config uses config.small_model, config.medium_model, config.model | Pass | Lines 45-50 in router.rs |

## Issues Found

### Inconsistencies

1. **ToolExecuteBefore/After hook line numbers**
   - Architecture doc states hooks at loop.rs:1764 and 1806
   - Actual lines: 1770 and 1812
   - Impact: Minor - the behavior is correct, just line number documentation off by ~6 lines
   - Recommendation: Update AGENTS.md or architecture doc to reflect actual lines

2. **SubAgentPool clone_impl concern**
   - worker.rs lines 340-359 implements Clone for SubAgentPool
   - Contains `cancel_token: self.cancel_token.clone()` which is CancellationToken
   - CancellationToken is !Clone, but the manual implementation uses .clone() which works because CancellationToken uses Arc internally
   - This is actually correct, not a bug, but worth noting

### Missing Documentation

1. **TeamCreateTool, SendMessageTool, ListMessagesTool, TeamStatusTool, ListTeamsTool**
   - These tool implementations in teams.rs are not mentioned in architecture doc
   - These provide team coordination capabilities via tools
   - Recommendation: Add to architecture doc under team coordination

2. **SharedTaskList and IdleNotifier in teams.rs**
   - Supporting infrastructure for team coordination not documented
   - Recommendation: Document these utilities

3. **TeamMessage::timestamp() method**
   - Uses SystemTime to generate millisecond timestamps
   - Not documented in architecture

4. **parse_mention(), filter_agents(), find_mention_trigger() in mention.rs**
   - @agent mention handling for agent routing
   - Not documented in architecture

5. **render_prompt_template(), select_provider_prompt(), assemble_system_prompt() in prompt.rs**
   - Prompt template rendering system
   - Builtin prompts map (debug, refactor, review, test, document)
   - select_provider_prompt() includes files from prompts/ directory
   - Not documented in architecture

6. **ToolTimeoutConfig struct in loop.rs**
   - Defines timeouts per tool type (bash, read, write, edit, etc.)
   - Not documented

7. **harden_history() function in loop.rs**
   - Ensures tool messages match tool calls, drops orphans
   - Important for compaction invariant
   - Not documented in architecture

8. **is_file_modifying_tool(), extract_path_from_tool_call(), extract_bash_command(), extract_git_subcommand() in loop.rs**
   - Helper functions for tool permission checking
   - Not documented

9. **redact_local_paths() function in loop.rs**
   - Path redaction for privacy in tool outputs
   - Not documented

10. **send_subagent() vs send_async_subagent() distinction**
    - Both exist but not clearly documented what the difference is
    - Looking at code, send() spawns handler and returns immediately, send_async() appears identical
    - The architecture says "both are async now" but they appear functionally identical

### Improvement Opportunities

1. **SubAgentSpawner send() vs send_async()**
   - Both methods appear functionally identical (lines 434-444 and 446-456)
   - Architecture doc says they "share implementation via handle_response()"
   - This is true but both spawn a handler that calls handle_response
   - Recommendation: Determine if both are needed or if one is deprecated

2. **provider_registry parameter in SubAgentPool**
   - worker.rs line 69 stores provider_registry
   - But execute_agent_task at line 568 uses provider_registry.get() with provider name from agent.model
   - Architecture should note this lookup mechanism

3. **BackgroundScheduler load_tasks() parsing**
   - Line 279-283 has complex logic extracting id from parent_id field
   - Falls back to UUID if parent_id is None
   - This is an unusual design choice (using parent_id as task id)
   - Not documented why

4. **Tool definition cache invalidation**
   - Uses mcp_tool_count as proxy for tool changes
   - Known limitation noted in code comment line 1033-1037
   - Architecture doc mentions this limitation at line 276
   - Suggestion: Consider exposing a version/hash from MCP service

## Recommendations

1. **Update line numbers in documentation** - ToolExecuteBefore/After are at lines 1770 and 1812, not 1764/1806

2. **Add team tools to architecture** - TeamCreateTool, SendMessageTool, etc. should be documented

3. **Add prompt.rs capabilities** - Document builtin prompts and template rendering

4. **Clarify send() vs send_async()** - Either consolidate or document functional difference

5. **Document BackgroundScheduler id field** - The parent_id as task_id design choice needs explanation

6. **Consider adding hardened history to compaction docs** - The harden_history() function ensures compaction invariant
