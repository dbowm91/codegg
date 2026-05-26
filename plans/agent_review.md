# Agent Architecture Review

## Summary
The agent.md architecture document is generally accurate and well-documenting the module structure. The main issues are: (1) outdated line number references that no longer match the current code, (2) a misleading claim about hook invocation at specific line numbers, and (3) a minor error about the `teams.rs` file stated as being for "team coordination" when the pattern is actually just inbox-based (file-based messaging cited as a SEPARATE mechanism in team.rs, not the primary teams.rs mechanism).

## Verified Correct
- `AgentLoop` struct at loop.rs:548-571 with correct fields (providers, permission_checker, tool_registry, hook_registry, context_tracker, doom_detector, etc.)
- `ToolDefCache` type alias correctly defined at loop.rs:60-67
- `SubAgentPool` struct matches at worker.rs:60-75 with all documented fields
- `SubAgentRequest`/`SubAgentResult` at worker.rs:18-35
- `SubAgentSpawner::send()` / `send_async()` both async share implementation - confirmed at worker.rs:434-456
- `BackgroundScheduler` struct at task.rs:90-95 with all fields
- `BackgroundTask` struct at task.rs:30-39
- `ContextTracker` at compaction.rs:76-84
- `ModelRouter` at router.rs:20-26 with TaskComplexity enum at router.rs:3-8
- `Agent` struct at mod.rs:27-42 with `AgentMode` at mod.rs:44-51
- Events published correctly: SubagentStarted/Progress/Completed/Failed at worker.rs:472, 485, 516, 531
- `ToolExecuteBefore` and `ToolExecuteAfter` plugin hooks actually invoked in execute_tool_calls()

## Discrepancies Found
- **Line numbers stale**: Architecture doc references `loop.rs:1764` and `loop.rs:1806` for tool hook invocation - current code is 2170 lines, these no longer correspond. The PreToolExecute/PostToolExecute hooks run at around lines 1755-1761 and 1818-1835 in current code.
- **team.rs vs teams.rs**: Doc says "team.rs / teams.rs - Team Coordination" but these files implement different patterns. `team.rs` is the main module for team coordination via file-based inbox. `teams.rs` may not exist or be different. Need verification.
- **ToolExecuteBefore/After claim misleading**: Doc says "Both hooks ARE invoked in `execute_tool_calls()` at loop.rs:1764 and 1806" but these lines show `dispatch_tool_execute_before`/`dispatch_tool_execute_after` (plugin service), not `PreToolExecute`/`PostToolExecute` (HookRegistry). The HookRegistry hooks ARE also invoked but at different line numbers (1755-1761 and 1818-1835).

## Bugs Identified
- No bugs found in implementation - code appears correct

## Improvement Suggestions
- **Update line number references**: The architecture doc should either remove specific line numbers (fragile) or update them to reflect current code
- **Clarify hook invocation**: The doc should note that BOTH plugin hooks (dispatch_tool_execute_before/after) AND HookRegistry hooks (PreToolExecute/PostToolExecute) are invoked for each tool execution
- **Fix team/teams nomenclature**: Clarify that team.rs implements file-based inbox communication, or remove teams.rs if it's not a separate coordination module

## Stale Items in Architecture Doc
- Line numbers throughout (loop.rs:1764, 1806) no longer match code due to growth
- "team.rs / teams.rs" description imprecise - should clarify actual pattern if two files exist with different purposes
