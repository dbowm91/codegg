# Event-Bus Architecture Review

## Architecture Document
- Path: architecture/event-bus.md

## Source Code Location
- src/bus/

## Verification Summary
**Partial** - Core implementation matches, but documentation has an event count error and is missing several public API methods.

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| GlobalEventBus uses tokio broadcast channel (capacity 2048) | Pass | `global.rs:13` confirms `broadcast::channel(2048)` |
| LazyLock singleton pattern | Pass | `global.rs:5` uses `LazyLock<GlobalEventBus>` |
| publish() returns subscriber count on success | Pass | `global.rs:19-27` correctly returns n via `Ok(n)` |
| trace level for normal events | Pass | Updated from `warn` to `trace` (per 2026-05-22 review) |
| debug level for no subscribers (n=0) | Pass | `global.rs:20-23` uses `tracing::debug` |
| warn level for channel closed | Pass | `global.rs:29-32` uses `tracing::warn` |
| subscribe() returns broadcast::Receiver | Pass | `global.rs:36-38` |
| subscriber_count() returns receiver_count | Pass | `global.rs:40-42` |
| AppEvent has 36 variants | Pass | Counted 36 unique variants in events.rs |
| event_type() method for SSE filtering | Pass | events.rs:150-189 returns string discriminants |
| PermissionRegistry uses DashMap with oneshot | Pass | mod.rs:12 |
| QuestionRegistry uses DashMap with oneshot | Pass | mod.rs:75 |
| Both registries use 300-second TTL | Pass | mod.rs:59, 122 |
| cleanup() called on each register() | Pass | mod.rs:23, 86 |
| PermissionChoice enum exists | Pass | permission/mod.rs:129-134 |
| Registration-before-publish pattern | Pass | Correctly documented |
| SSE handler at /api/event | Pass | http.rs:236 routes to sse_handler |
| SSE handler subscribes to GlobalEventBus | Pass | event.rs:13 |
| SSE handler with 15-second heartbeat | Pass | event.rs:26-28 |
| SSE handler formats: event: {event_type}\ndata: {json}\n\n | Pass | event.rs:17 |

## Issues Found

### Bugs

1. **Event count mismatch in "Other Events"** (architecture/event-bus.md:83)
   - **Issue**: Lists "Other Events (9)" but only 8 events are shown
   - **Listing**: ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged, **AgentFinished** (9 items)
   - **Actual**: AgentFinished is already counted in "Streaming Events" (line 77), so "Other Events" should be 8 items
   - **Correct total**: 7+2+3+3+2+2+3+4+2+8 = 36 variants
   - **Impact**: Documentation inconsistency; total count still happens to be correct (36)

### Inconsistencies

2. **Tool Events count in architecture doc** (architecture/event-bus.md:69)
   - **Doc says**: "Tool Events (3): ToolCalled, ToolResult, ToolCallStarted"
   - **Line 77 says**: "Streaming Events (3): TextDelta (Arc<str>), ReasoningDelta, AgentFinished"
   - **Issue**: ToolCallStarted appears in Tool Events, but is not mentioned in the streaming category
   - **Verification**: events.rs confirms ToolCallStarted is a distinct event at lines 102-107

### Missing Documentation

3. **Undocumented PermissionRegistry methods**
   - `unregister(perm_id: &str)` - mod.rs:41-43
   - `is_registered(perm_id: &str) -> bool` - mod.rs:45-47
   - `pending_permission_ids()` - mod.rs:49-56

4. **Undocumented QuestionRegistry methods**
   - `unregister(question_id: &str)` - mod.rs:104-106
   - `is_registered(question_id: &str) -> bool` - mod.rs:108-110
   - `pending_question_ids()` - mod.rs:112-119

5. **PermissionChoice::allowed() and persist() methods**
   - permission/mod.rs:136-150 has helper methods on PermissionChoice
   - Not mentioned in architecture doc

6. **TTL cleanup mechanism detail**
   - Both registries use `cleanup()` which calls `DashMap::retain()` to remove expired entries
   - This is an automatic cleanup on each register() call, not a background task
   - Important for understanding memory behavior under load

### Improvement Opportunities

7. **Skill document alignment**
   - `.opencode/skills/event-bus/SKILL.md` line 84 correctly says "Other (8)" but lists the same 9 items as the architecture doc
   - Same count bug exists in the skill document

8. **Error handling for respond() and answer_question()**
   - Both methods return `bool` indicating success/failure
   - When `tx.send()` fails, they log a warning and return false
   - This could be documented as the caller should handle "response not received" case

9. **Event flow diagram simplification**
   - The diagram at lines 128-150 shows AgentLoop → PermissionRegistry → GlobalEventBus → TUI
   - This is correct for permission flow but doesn't show question flow
   - Could be expanded or noted as "Permission flow shown; question flow similar"

## Recommendations

1. **Fix event count**: Change "Other Events (9)" to "Other Events (8)" and remove AgentFinished from that listing (it belongs only to Streaming Events)

2. **Document missing methods**: Add sections for unregister(), is_registered(), and pending_*_ids() methods in both PermissionRegistry and QuestionRegistry

3. **Add TTL mechanism documentation**: Note that cleanup happens on each register() call via DashMap::retain(), not via background task

4. **Sync skill document**: Fix the skill doc at `.opencode/skills/event-bus/SKILL.md` line 84 to match the corrected count

5. **Document return value behavior**: Note that respond() and answer_question() returning false indicates the receiver dropped the channel (user may have cancelled)

## Verification Complete
- Core GlobalEventBus implementation matches documentation exactly
- AppEvent enum has 36 variants as documented (but "Other Events" sub-count is wrong)
- PermissionRegistry and QuestionRegistry implementations are correct
- SSE handler implementation matches documentation
- Registration-before-publish pattern is correctly documented
