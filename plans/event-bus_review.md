# Event Bus Module Review

## Summary

Reviewed `architecture/bus.md`, `src/bus/` implementation, and `.opencode/skills/event-bus/SKILL.md`. The documentation is **highly accurate** and the implementation matches the specifications. No critical bugs found.

---

## Verified Items

### 1. GlobalEventBus (`src/bus/global.rs`)

| Item | Status | Notes |
|------|--------|-------|
| Singleton pattern via `LazyLock` | Verified | Line 5: `static GLOBAL_BUS: LazyLock<GlobalEventBus>` |
| Broadcast channel capacity 2048 | Verified | Line 13: `broadcast::channel(2048)` |
| `publish()` method | Verified | Lines 17-34 |
| `subscribe()` method | Verified | Lines 36-38 |
| `subscriber_count()` method | Verified | Lines 40-42 |
| `Ok(0)` uses `debug!` log level | Verified | Line 20-23 |
| `Ok(n)` uses `trace!` log level | Verified | Line 24-28 |
| `Err` uses `warn!` log level | Verified | Line 29-32 |

### 2. AppEvent Enum (`src/bus/events.rs`)

**Total: 36 variants** (verified by counting `event_type()` match arms)

| Category | Count | Verified |
|----------|-------|----------|
| Session Events | 7 | SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted |
| Message Events | 2 | MessageAdded, MessageDeleted |
| Tool Events | 3 | ToolCalled, ToolResult, ToolCallStarted |
| MCP Events | 3 | McpServerConnected, McpServerDisconnected, McpToolListChanged |
| Permission Events | 2 | PermissionPending, PermissionResponded |
| Question Events | 2 | QuestionPending, QuestionAnswered |
| Streaming Events | 3 | TextDelta, ReasoningDelta, AgentFinished |
| Subagent Events | 4 | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed |
| Diff Events | 2 | DiffPending, DiffResponded |
| Other Events | 8 | ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged |
| **Total** | **36** | |

### 3. PermissionRegistry (`src/bus/mod.rs`)

| Item | Status | Location |
|------|--------|----------|
| Struct with DashMap | Verified | Lines 11-13 |
| 300-second TTL | Verified | Line 59: `Duration::from_secs(300)` |
| `register()` method | Verified | Lines 22-27 |
| `respond()` method | Verified | Lines 29-39 |
| `unregister()` method | Verified | Lines 41-43 |
| `is_registered()` method | Verified | Lines 45-47 |
| `pending_permission_ids()` method | Verified | Lines 49-56 |
| `cleanup()` on each register | Verified | Line 23 |

### 4. QuestionRegistry (`src/bus/mod.rs`)

| Item | Status | Location |
|------|--------|----------|
| Struct with DashMap | Verified | Lines 74-76 |
| 300-second TTL | Verified | Line 122: `Duration::from_secs(300)` |
| `register()` method | Verified | Lines 85-90 |
| `answer_question()` method | Verified | Lines 92-102 |
| `unregister()` method | Verified | Lines 104-106 |
| `is_registered()` method | Verified | Lines 108-110 |
| `pending_question_ids()` method | Verified | Lines 112-119 |
| `cleanup()` on each register | Verified | Line 86 |

### 5. SSE Handler (`src/server/routes/event.rs`)

| Item | Status | Location |
|------|--------|----------|
| Subscribes directly to `GlobalEventBus::subscribe()` | Verified | Line 13 |
| Uses `BroadcastStream` | Verified | Line 14 |
| 15-second heartbeat | Verified | Lines 26-28 |
| Formats as `event: {event_type}\ndata: {json}\n\n` | Verified | Line 17 |
| No parameters in handler | Verified | Line 12 |

### 6. Registration-Before-Publish Pattern

Verified in `src/agent/loop.rs`:
- **Question pending** (lines 401-406): `QuestionRegistry::register()` called BEFORE `GlobalEventBus::publish(AppEvent::QuestionPending {...})`
- **Permission pending** (lines 475-482): `PermissionRegistry::register()` called BEFORE `GlobalEventBus::publish(AppEvent::PermissionPending {...})`

---

## Discrepancies

### Minor Documentation Issues

1. **`PermissionChoice` enum location**: The architecture doc shows `PermissionChoice` as if it is in `mod.rs` but it is actually defined in `src/permission/mod.rs` and re-exported via the `use crate::permission::PermissionChoice` import at `src/bus/mod.rs:4`. This is a minor documentation clarity issue.

2. **"38 variants" in AGENTS.md**: The AGENTS.md file states "AppEvent count corrected: 38 variants (was incorrectly documented as 40+)" but the actual count is **36**. This appears to be a stale note that was not updated. The architecture document correctly states 36.

---

## Recommendations

### For Documentation

1. **AGENTS.md already updated**: The note about "38 variants" was incorrect - actual count is 36, which is already documented correctly in `architecture/bus.md`.

2. **Clarify `PermissionChoice` location**: Consider noting that `PermissionChoice` is defined in `src/permission/mod.rs` for clarity.

### For Code

No bugs or issues found in the implementation.

---

## Conclusion

The event bus module is **well-implemented and accurately documented**. All core functionality matches the architecture documentation:

- GlobalEventBus singleton with broadcast channel (capacity 2048)
- 36 AppEvent variants with `event_type()` for SSE filtering
- PermissionRegistry and QuestionRegistry with 300-second TTL cleanup
- Registration-before-publish pattern correctly implemented in agent loop
- SSE handler properly subscribed to GlobalEventBus

No fixes required.
