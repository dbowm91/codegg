# Agent Architecture Review Findings

## Verified Claims

- **AgentLoop struct fields (lines 24-47)**: All fields verified in `src/agent/loop.rs:1-2170`. Fields include `agents`, `state`, `limits`, `provider`, `permission_checker`, `tool_registry`, `hook_registry`, `context_tracker`, `doom_detector`, `steering`, `follow_up_tx/rx`, `config`, `question_tx/rx`, `plugin_service`, `session_id`, `mcp_service`, `tool_def_cache`, `model_router`, `snapshot_manager`, `file_change_rx`.

- **ToolDefCache type alias (lines 60-73)**: Verified at `src/agent/loop.rs:60-67` with exact tuple structure.

- **ToolExecuteBefore/After hooks invoked**: Both hooks verified at `src/agent/loop.rs:1770` (before) and `src/agent/loop.rs:1814` (after) in `execute_tool_calls()`.

- **ChatEvent types (processor.rs)**: Verified in `src/agent/processor.rs:28-57` - TextDelta, ReasoningDelta, ToolCall, Finish, Error all present.

- **SubAgentPool struct**: Verified in `src/agent/worker.rs` with `max_concurrent: 5` and `max_depth: 3` defaults.

- **BackgroundTask/BackgroundScheduler**: Verified in `src/agent/task.rs:171-186` with correct fields.

- **ModelRouter struct**: Verified in `src/agent/router.rs:99-110` with `simple_model`, `medium_model`, `complex_model` fields and `TaskComplexity` enum.

- **Agent struct**: Verified in `src/agent/mod.rs:214-229` with all fields matching.

- **AgentMode enum**: Verified in `src/agent/mod.rs:234-239` - Primary, Subagent, All.

- **AgentLoopState**: Correctly tracks `current_agent`, `turn_count`, `total_tokens`, `start_time`, `plan_mode`, `plan_topic`.

- **Events published**: SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed, TextDelta, ReasoningDelta, ToolCallStarted, ToolResult, AgentFinished, PermissionPending, QuestionPending - all verified.

- **`start_workers()` removed**: Confirmed - workers start in constructors, not via separate method.

- **Tool definition caching**: Cache includes `mcp_tool_count` as proxy for invalidation - confirmed at `loop.rs:60-67`.

- **DoomLoop detection**: Window-based counting confirmed in `src/permission/mod.rs`.

## Stale Information

- **Line reference for ToolExecuteBefore hook (agent.md:296)**: States "at loop.rs:1777 and 1814" - actual lines are 1770 and 1814. Off by several lines but hook invocation confirmed.

## Bugs Found

- **No bugs found**: Implementation matches documentation. Hooks are properly invoked, event publishing is correct.

## Improvements Suggested

- **Line numbers in documentation are fragile**: Consider removing specific line number references as they become stale quickly. Instead reference method names or describe behavior.

- **Missing EventProcessor documentation**: The `processor.rs` file handles ChatEvent processing but isn't fully documented in architecture - only mentioned briefly.

## Cross-Module Issues

- **CoreRequest handler gap**: `Initialize`, `Subscribe`, `Resume` variants in `CoreRequest` (`src/protocol/core.rs:51-58`) are not handled in `InprocCoreClient` and fall through to `CoreResponse::Ack` at `src/core/mod.rs:698`. This may be intentional but should be documented.

- **AgentLoop creates AgentLoop directly**: While `AgentLoop` is constructed in `src/core/mod.rs:138-146`, it requires providers to be registered via `register_builtin_with_config` - this tight coupling may cause issues if provider initialization order changes.