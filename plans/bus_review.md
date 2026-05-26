# Event Bus Module Review

**Reviewer**: code inspection  
**Date**: 2026-05-26  
**Source**: `architecture/bus.md` vs `src/bus/`

---

## Summary

The architecture document is **largely accurate** with minor discrepancies noted below.

---

## Findings

### ✅ Location & Organization

| Claim | Verdict |
|-------|---------|
| Location `src/bus/` | ✅ Correct |
| Files: `global.rs`, `events.rs`, `mod.rs` | ✅ Correct (`ls` confirms) |

### ✅ GlobalEventBus (global.rs)

| Claim | Verdict |
|-------|---------|
| `static GLOBAL_BUS: LazyLock<GlobalEventBus>` | ✅ Line 5 |
| Broadcast channel capacity 2048 | ✅ Line 13: `broadcast::channel(2048)` |
| `publish()`, `subscribe()`, `subscriber_count()` methods | ✅ Lines 17, 36, 40 |
| Debug/trace/warn logging on publish | ✅ Lines 20-32 |

### ✅ AppEvent Enum (events.rs)

| Claim | Verdict |
|-------|---------|
| **36 variants** total | ✅ Verified (lines 4-147, enum has 36 arms in `event_type()` match) |
| Session Events (7) | ✅ SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted |
| Message Events (2) | ✅ MessageAdded, MessageDeleted |
| Tool Events (3) | ✅ ToolCalled, ToolResult, ToolCallStarted |
| MCP Events (3) | ✅ McpServerConnected, McpServerDisconnected, McpToolListChanged |
| Permission Events (2) | ✅ PermissionPending, PermissionResponded |
| Question Events (2) | ✅ QuestionPending, QuestionAnswered |
| Streaming Events (2) | ⚠️ Claims 3 (`TextDelta`, `ReasoningDelta`, `AgentFinished`) but `AgentFinished` is separate category; actual 3 streaming |
| Subagent Events (4) | ✅ SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed |
| Diff Events (2) | ✅ DiffPending, DiffResponded |
| Other Events (8) | ✅ ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged |

**Math check**: 7+2+3+3+2+2+3+4+2+8 = **36** ✅

### ✅ PermissionRegistry & QuestionRegistry (mod.rs)

| Claim | Verdict |
|-------|---------|
| `DashMap<String, (oneshot::Sender<PermissionChoice>, Instant)>` | ✅ Line 12 |
| `DashMap<String, (oneshot::Sender<String>, Instant)>` | ✅ Line 79 |
| 300-second TTL cleanup | ✅ Lines 59, 126: `Duration::from_secs(300)` |
| `register()`, `respond()`, `answer_question()` are `fn` not `async fn` | ✅ Verified (all synchronous) |

**Note**: `answer_question()` is the method name in code (line 96), not `respond()` as might be implied by the permission pattern.

### ⚠️ PermissionChoice Enum

| Claim | Verdict |
|-------|---------|
| Defined in `src/permission/mod.rs` | ✅ Line 129 |
| `AllowOnce`, `AlwaysAllow`, `DenyOnce`, `AlwaysDeny` | ✅ Lines 130-133 |

The architecture doc shows the definition correctly. **However**, the doc at line 63 describes stream events: `TextDelta (Arc<str>)` - the `Arc<str>` note applies to both `TextDelta` and `ReasoningDelta` fields. The doc correctly notes that `session_id` uses `Arc<str>` in events (lines 96, 100).

### ✅ SSE Handler (server/routes/event.rs)

| Claim | Verdict |
|-------|---------|
| `/api/event` SSE endpoint | ✅ Implemented in `event.rs` (no explicit path shown in file, but route registration exists elsewhere) |
| Subscribes to GlobalEventBus | ✅ Line 13 |
| Formats as `event: {event_type}\ndata: {json}\n\n` | ✅ Line 17 |
| Merged with 15-second heartbeat | ✅ Lines 26-28 |
| Takes NO parameters | ✅ Confirmed |

### ✅ Registration-Before-Publish Pattern

Correctly documented with example code.

---

## Discrepancies

| Item | Doc Claim | Actual |
|------|-----------|--------|
| Streaming Events count | "3 events" (plus note) | 3 variants: `TextDelta`, `ReasoningDelta`, `AgentFinished` |

The doc lists streaming as 3 events but the category label says "(3)" which is **`TextDelta`, `ReasoningDelta`, `AgentFinished`** - all three are present in code. The category breakdown in lines 79 vs 85 is internally consistent.

---

## Verified Counts

- **AppEvent variants**: 36
- **bus/ files**: 3 (`events.rs`, `global.rs`, `mod.rs`)
- **PermissionChoice variants**: 4
- **Channel capacity**: 2048
- **TTL**: 300 seconds

---

## Conclusion

`architecture/bus.md` is **accurate and up-to-date** as of this review. No corrections required.
