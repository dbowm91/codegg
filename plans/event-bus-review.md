# Event Bus Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| GlobalEventBus uses tokio broadcast channel with capacity 2048 | VERIFIED | `global.rs:13` - `broadcast::channel(2048)` |
| `GLOBAL_BUS` is `LazyLock<GlobalEventBus>` | VERIFIED | `global.rs:5` - `static GLOBAL_BUS: LazyLock<GlobalEventBus>` |
| `publish()` returns subscriber count via `Ok(n)` | VERIFIED | `global.rs:19` - `tx.send()` returns `Result<usize, SendError>` |
| `publish()` logs `debug!` when no subscribers | INCORRECT | Code uses `debug!` at line 20, doc claims it uses `trace!` |
| `subscribe()` returns `broadcast::Receiver<AppEvent>` | VERIFIED | `global.rs:36-38` |
| `subscriber_count()` returns `receiver_count()` | VERIFIED | `global.rs:40-42` |
| AppEvent has 38 variants | INCORRECT | Code has 36 variants (counted in `events.rs:5-147`) |
| Session Events (7) | VERIFIED | SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted |
| Message Events (2) | VERIFIED | MessageAdded, MessageDeleted |
| Tool Events (3) | VERIFIED | ToolCalled, ToolResult, ToolCallStarted |
| MCP Events (3) | VERIFIED | McpServerConnected, McpServerDisconnected, McpToolListChanged |
| Permission Events (2) | VERIFIED | PermissionPending, PermissionResponded |
| Question Events (2) | VERIFIED | QuestionPending, QuestionAnswered |
| Streaming Events (3) | VERIFIED | TextDelta, ReasoningDelta, AgentFinished |
| Subagent Events (4) | VERIFIED | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed |
| Diff Events (2) | VERIFIED | DiffPending, DiffResponded |
| Other Events (8) | INCORRECT | Code has 8: ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged. But total is 36 not 38. |
| PermissionRegistry uses `DashMap` | VERIFIED | `mod.rs:12` - `senders: DashMap<...>` |
| QuestionRegistry uses `DashMap` | VERIFIED | `mod.rs:75` - `senders: DashMap<...>` |
| 300-second TTL for registries | VERIFIED | `mod.rs:59, 122` - `Duration::from_secs(300)` |
| `event_type()` method for SSE filtering | VERIFIED | `events.rs:150-189` |
| SSE handler subscribes to GlobalEventBus directly | VERIFIED | `event.rs:13` - `GlobalEventBus::subscribe()` |
| SSE handler merges with 15-second heartbeat | VERIFIED | `event.rs:26-28` - `interval(Duration::from_secs(15))` |
| SSE format: `event: {event_type}\ndata: {json}\n\n` | VERIFIED | `event.rs:17` |
| Registration-before-publish pattern documented | VERIFIED | Pattern is correctly documented in arch doc |

### Event Count Discrepancy

The arch doc claims 38 variants but the actual code has 36:

**Session (7)**: SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted

**Message (2)**: MessageAdded, MessageDeleted

**Tool (3)**: ToolCalled, ToolResult, ToolCallStarted

**MCP (3)**: McpServerConnected, McpServerDisconnected, McpToolListChanged

**Permission (2)**: PermissionPending, PermissionResponded

**Question (2)**: QuestionPending, QuestionAnswered

**Streaming (3)**: TextDelta, ReasoningDelta, AgentFinished

**Subagent (4)**: SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed

**Diff (2)**: DiffPending, DiffResponded

**Other (8)**: ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged

**Total: 36 variants**

## Bugs Found

### Critical

None identified.

### High

None identified.

### Medium

1. **Event count mismatch**: Architecture doc states 38 variants but code has 36. Should update `architecture/event-bus.md:17` line from "38 variants" to "36 variants" and recalculate category totals.

2. **Log level mismatch**: `global.rs:20` uses `tracing::debug!` for no subscribers, but the arch doc at lines 35-38 shows `trace!`. While both are valid, consistency should be verified. Actually, re-reading the doc - it shows `trace!` for normal events but the code uses `debug!`. This is a minor documentation inconsistency.

## Improvement Suggestions

### Performance

1. **Consider bounded reception for SSE**: The SSE handler in `event.rs` creates an unbounded broadcast receiver. If a client disconnects silently, events accumulate in memory. Consider adding backpressure or channel sized appropriately for SSE connections.

2. **Event serialization optimization**: `event.rs:16` calls `serde_json::to_string(&event)` on every event. For high-frequency events like `TextDelta` and `ReasoningDelta`, this could be a bottleneck. Consider:
   - Pre-serializing event types to strings
   - Caching serialized event type names in `AppEvent::event_type()` variants

### Correctness

1. **SSE error handling**: In `event.rs:14-23`, `BroadcastStream` errors are silently dropped with `Err(_) => None`. This makes debugging SSE issues difficult. Consider logging at trace/debug level.

### Maintainability

1. **Missing unit tests for AppEvent**: `events.rs` has no tests. Consider adding tests for `event_type()` method to ensure all variants return correct string values.

2. **Missing unit tests for registries**: `mod.rs` has no tests for `PermissionRegistry` and `QuestionRegistry`. Consider adding tests for TTL cleanup, concurrent access, and error cases.

3. **Missing integration tests for GlobalEventBus**: `global.rs:51-59` has one basic test for subscriber count, but no tests for:
   - Concurrent publish/subscribe
   - Channel closure handling
   - Multiple simultaneous subscribers

4. **Hardcoded TTL in cleanup()**: Both registries have `Duration::from_secs(300)` duplicated in `cleanup()`. Consider extracting to a constant like `const REGISTRY_TTL_SECS: u64 = 300;`.

5. **No metric collection**: The event bus could benefit from observability metrics (events published per second, subscriber count over time, etc.) for production debugging.

## Priority Actions (top 5 items to fix)

1. **Update architecture document event count**: Change "38 variants" to "36 variants" and verify all category totals sum correctly in `architecture/event-bus.md`.

2. **Add AppEvent::event_type() unit tests**: Ensure all 36 variants return correct, non-empty strings. This is a low-effort, high-value test to add.

3. **Add PermissionRegistry/QuestionRegistry unit tests**: Test TTL cleanup behavior, concurrent register/respond scenarios, and edge cases like double-unregister.

4. **Extract TTL constant**: Create `const REGISTRY_TTL_SECS: u64 = 300;` and use in both cleanup() functions to avoid duplication.

5. **Add GlobalEventBus integration tests**: Test concurrent publish/subscribe, channel closure behavior, and multiple simultaneous subscribers.

## Summary

The event bus implementation is solid and well-architected. The main finding is a **documentation discrepancy** where the architecture doc claims 38 event variants but the code contains 36. No actual bugs in the implementation were found - the code correctly implements the patterns described in the documentation. The SSE handler implementation matches the documented behavior exactly.