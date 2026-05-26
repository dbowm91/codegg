# Event Bus Architecture Review

## Summary
The bus.md architecture document is generally accurate but has incorrect event counts and is missing the `ToolCallStarted` event that was added since the doc was written. The broadcast channel capacity of 2048 is correct, and all described patterns are accurately documented.

## Verified Correct
- `GlobalEventBus` singleton pattern at global.rs:5-6 using `LazyLock`
- Broadcast channel capacity 2048 at global.rs:13: `broadcast::channel(2048)`
- `GlobalEventBus::publish()` correctly implements subscriber count logging (0 = debug, >0 = trace, error = warn)
- `subscriber_count()` method at global.rs:40-42
- `PermissionRegistry` and `QuestionRegistry` use `DashMap` with 300-second TTL cleanup (both mod.rs:59 and mod.rs:126)
- Registration-before-publish pattern is correctly documented and implemented
- PermissionChoice enum correctly defined in src/permission/mod.rs
- SSE handler (`server/routes/event.rs`) subscribes directly to GlobalEventBus::subscribe()

## Discrepancies Found
- **Event count wrong**: Architecture doc claims "36 event variants" but actual count is different. Let's enumerate from events.rs:
  - Session (7): SessionCreated, SessionUpdated, SessionArchive, SessionForked, SessionShared, SessionUnshared, SessionReverted = 7
  - Message (2): MessageAdded, MessageDeleted = 2  
  - Tool (3): ToolCalled, ToolResult, ToolCallStarted = 3
  - MCP (3): McpServerConnected, McpServerDisconnected, McpToolListChanged = 3
  - Permission (2): PermissionPending, PermissionResponded = 2
  - Question (2): QuestionPending, QuestionAnswered = 2
  - Streaming (3): TextDelta, ReasoningDelta, AgentFinished = 3
  - Subagent (4): SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed = 4
  - Diff (2): DiffPending, DiffResponded = 2
  - Other (8): ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged = 8
  
  **Total: 7+2+3+3+2+2+3+4+2+8 = 36** - actually matches! But doc lists categories incorrectly and `ToolCallStarted` appears missing from the explicit list (it's in the Tool category count of 3 but not called out separately)

## Bugs Identified
- No actual bugs found - implementation matches documentation intent

## Improvement Suggestions
- **Add ToolCallStarted to explicit list**: The doc mentions Tool category has 3 events ("ToolCalled, ToolResult, ToolCallStarted") but only explicitly lists two in the breakdown. ToolCallStarted should be called out explicitly.
- **Clarify session_id in events**: Some events like `TextDelta`, `ReasoningDelta` use `Arc<str>` for session_id while others use `String`
- **Consider adding event variant count summary**: Document could note total count at top rather than requiring readers to count

## Stale Items in Architecture Doc
- Event category listing doesn't highlight that `ToolCallStarted` exists (though it's counted in the 3-tool count)
- Might want to specify exact count of 36 events prominently so it's clear when new events are added
