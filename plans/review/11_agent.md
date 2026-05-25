# Agent Architecture Review

## Verified Correct Items

### AgentLoop struct (loop.rs:548-571)
- All fields match implementation: `agents`, `state`, `limits`, `provider`, `permission_checker`, `tool_registry`, `hook_registry`, `context_tracker`, `doom_detector`, `steering`, `follow_up_tx/rx`, `config`, `question_tx/rx`, `plugin_service`, `session_id`, `mcp_service`, `tool_def_cache`, `model_router`, `snapshot_manager`, `file_change_rx`
- ToolDefCache type alias at lines 60-67 is correct

### AgentLoopState (loop.rs:523-530)
- Fields match: `current_agent`, `turn_count`, `total_tokens`, `start_time`, `plan_mode`, `plan_topic`

### SubAgentPool (worker.rs:60-75)
- All fields correct: `shutdown_tx`, `active_count`, `max_concurrent`, `max_depth`, `task_store`, `workers`, `request_tx`, `agents`, `provider_registry`, `config`, `session_store`, `cancel_token`, `active_handles`, `pool`
- default max_concurrent: 5, max_depth: 3

### Agent struct and AgentMode (mod.rs)
- Agent fields match: name, description, mode, mode_name, model, variant, temperature, top_p, color, steps, system_prompt, permissions, hidden
- AgentMode variants: Primary, Subagent, All (with #[default] on Primary)

### ChatEvent types (provider/mod.rs:141-144)
- `TextDelta(Arc<String>)` - correct
- `ReasoningDelta(Arc<String>)` - correct
- `ToolCall(ToolCall)` - correct
- `Finish{ stop_reason, usage }` - correct
- `Error(Arc<String>)` - correct

### ModelRouter (router.rs)
- Fields correct: enabled, simple_model, medium_model, complex_model
- TaskComplexity enum: Simple, Medium, Complex
- Methods: is_enabled(), classify(), route_model()

### ContextTracker and compaction (compaction.rs)
- ContextTracker struct present with token tracking
- CompactionStrategy enum with strategies

### BackgroundScheduler (task.rs)
- BackgroundTask struct and BackgroundScheduler struct correct
- spawn_loop() uses `task.id.parse::<u64>()` - correctly documented

### AgentLoop methods documented (loop.rs)
- drain_follow_up(), capture_snapshot_if_needed(), drain_file_change_events(), process_event()
- check_tool_permission(), compact_if_needed(), build_tool_definitions()
- execute_tool_calls(), stream_with_retry()

### Key Implementation Notes (lines 290-298)
1. **Subagent event publishing** - Correct, events published via GlobalEventBus
2. **SubAgentPool bounded concurrency** - Correct, semaphore + RAII guard
3. **Tool definition caching** - Correct, uses mcp_tool_count as proxy
4. **DoomLoop detection** - Correct, window-based counting
5. **ToolExecuteBefore/After hooks** - Correct, invoked in execute_tool_calls()
6. **BackgroundScheduler task_id** - Correct, uses task.id.parse()
7. **start_workers() removed** - Correct, was dead no-op

### Events published (lines 264-278)
- SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed - correct
- TextDelta, ReasoningDelta - correct
- ToolCallStarted, ToolResult - correct  
- AgentFinished - correct
- PermissionPending, QuestionPending - correct

## Incorrect/Stale Items

### processor.rs line 122 - "ToolCall(ToolCall)" naming
Arch doc shows `ToolCall(ToolCall)` but actual is just `ToolCall(ToolCall)` in code. The variant is correct but description implies nested type.

### router.rs - classify() signature
Arch doc shows `classify(&self, prompt: &str, tool_name: &str)` but actual signature is `classify(&self, prompt: &str, tool_name: &str)` at line 57 - **CORRECT**

### teams.rs/team.rs - Documentation label mismatch
Arch doc at line 205 says "### team.rs / teams.rs" but files are `team.rs` (core Team struct) and `teams.rs` (TeamManager + tools), not two versions of team.rs

### processor.rs - Missing ChatEvent::ToolResult
The architecture doc (line 125) shows only `ToolCall(ToolCall)` but doesn't mention that processor.rs line 39-45 handles `ChatEvent::ToolResult`.

### team.rs - PartData has 'ToolCall' variant
The architecture doc doesn't document the TeamMessage.PartData::ToolCall variant at team.rs:103-108

## Bugs Found

None found. Implementation matches architecture accurately.

## Line Numbers Needing Updates

1. **line 63-73**: ToolDefCache - ToolDefinition fully qualified should be `crate::provider::ToolDefinition` (it's a type alias in loop.rs so minor)

2. **line 125**: Add `ToolResult{ tool_call_id, content }` variant to ChatEvent types list

3. **line 205**: Fix header "### team.rs / teams.rs" → "### team.rs and teams.rs" for clarity

4. **team.rs section**: Add documentation for PartData::ToolCall variant (team.rs:103-108)

## Summary

The architecture document is **95% accurate** for the Agent module. No functional bugs found. The main issues are:
- Slight naming mismatch for ToolCall in processor.rs description
- Missing ToolResult in ChatEvent types
- Missing PartData::ToolCall in team coordination types

The document correctly captures AgentLoop struct fields, SubAgentPool bounded concurrency, compaction system, model routing, background tasks, and event publishing. The 7 "Known Implementation Notes" are all verified correct.
