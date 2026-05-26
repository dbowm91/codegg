# Bus Architecture Review Findings

## Verified Claims

- **36 event variants in AppEvent**: Verified in `src/bus/events.rs:5-147`. Count confirmed by enumerating all variants.

- **Session Events (7)**: SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted - all verified in `events.rs:7-19`.

- **Message Events (2)**: MessageAdded (line 21), MessageDeleted (line 26) - verified.

- **Tool Events (3)**: ToolCalled (line 31), ToolResult (line 33), ToolCallStarted (line 102) - verified.

- **MCP Events (3)**: McpServerConnected (line 41), McpServerDisconnected (line 43), McpToolListChanged (line 45) - verified.

- **Permission Events (2)**: PermissionPending (line 68), PermissionResponded (line 89) - verified.

- **Question Events (2)**: QuestionPending (line 61), QuestionAnswered (line 66) - verified.

- **Streaming Events (3)**: TextDelta (line 95), ReasoningDelta (line 100), AgentFinished (line 109) - verified.

- **Subagent Events (4)**: SubagentStarted (line 120), SubagentProgress (line 127), SubagentCompleted (line 134), SubagentFailed (line 141) - verified.

- **Diff Events (2)**: DiffPending (line 76), DiffResponded (line 83) - verified.

- **Other Events (8)**: ConfigChanged (line 47), AgentChanged (line 51), ModelChanged (line 53), CompactionTriggered (line 55), Error (line 57), Info (line 59), TodoUpdated (line 49), FileChanged (line 114) - verified. Total is 36.

- **GlobalEventBus broadcast capacity 2048**: Verified at `src/bus/global.rs:13` - `broadcast::channel(2048)`.

- **PermissionRegistry/QuestionRegistry TTL 300 seconds**: Both verified at `src/bus/mod.rs:59` and `mod.rs:126` - `Duration::from_secs(300)`.

- **PermissionChoice enum**: Defined in `src/permission/mod.rs:1142-1145` as AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny.

- **event_type() method**: Verified at `src/bus/events.rs:150-189` - returns string discriminants for SSE filtering.

## Stale Information

- **Event Count accuracy**: The 36 event variant count is accurate. No staleness detected.

## Bugs Found

- **No bugs found**: Event system correctly implemented with proper registration-before-publish pattern documented.

## Improvements Suggested

- **PermissionRegistry/QuestionRegistry session_id filtering**: Per AGENTS.md, these registries don't store `session_id` in their keys, making `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` unable to properly filter by session. This is documented as a known limitation but not in this architecture doc.

- **SSE handler documentation**: The SSE handler at `server/routes/event.rs` is mentioned but the actual route handler code path should be verified - architecture doc states it takes NO parameters but WebSocket setup may differ.

## Cross-Module Issues

- **AppEvent::ReasoningDelta uses String not Arc<str>**: Unlike `TextDelta` which uses `Arc<str>` for session_id and delta, `ReasoningDelta` uses owned `String` for delta (line 100). This inconsistency may affect performance for high-frequency events.

- **FileChanged event**: Has `old_content: Option<String>` field - this wasn't mentioned in the architecture doc's event categories.