# architecture/core.md Review

**Reviewer**: Claude Code
**Date**: 2026-05-26
**Source**: architecture/core.md vs src/core/mod.rs, src/protocol/core.rs, src/bus/events.rs

---

## Summary

The architecture document is largely accurate but has several discrepancies and missing details that should be corrected.

---

## Verified Claims

| Claim | Location | Status |
|-------|----------|--------|
| Protocol version is 1 | `src/protocol/core.rs:3` | ✅ CORRECT |
| `CoreClient` trait with `request` + `subscribe` | `src/core/mod.rs:13-20` | ✅ CORRECT |
| `InprocCoreClient` has 4 fields | `src/core/mod.rs:22-28` | ✅ CORRECT |
| Field names: subagent_pool, memory_store, bg_scheduler, pool | `src/core/mod.rs:24-27` | ✅ CORRECT |
| StdioCoreClient spawns subprocess | `src/core/transport/stdio.rs:17-46` | ✅ CORRECT |
| SocketCoreClient reconnect-and-retry-once | `src/core/transport/socket.rs:50-119` | ✅ CORRECT |
| Subscribe returns empty receiver for stdio/socket | `stdio.rs:88-91`, `socket.rs:126-129` | ✅ CORRECT |
| InprocCoreClient subscribes to GlobalEventBus | `src/core/mod.rs:702-725` | ✅ CORRECT |
| Turn execution spawns async task | `src/core/mod.rs:158` | ✅ CORRECT |
| AgentFinished/Error published to bus | `src/core/mod.rs:161-173` | ✅ CORRECT |
| CoreRequest enum in protocol/core.rs | `src/protocol/core.rs:48-175` | ✅ CORRECT |
| All CoreRequest variants listed | See table below | ⚠️ SEE BELOW |
| CoreEvent enum in protocol/core.rs | `src/protocol/core.rs:177-272` | ✅ CORRECT |
| All CoreEvent variants listed | See table below | ⚠️ SEE BELOW |
| Fallthrough variants return `Ack` | `src/core/mod.rs:698` | ✅ CORRECT |

---

## Discrepancies

### 1. InprocCoreClient Field Wrapping (Minor)

**Doc states**: "Contains 4 fields: `subagent_pool` (SubAgentPool...), `memory_store` (MemoryStore...)..."

**Actual** (`src/core/mod.rs:22-28`):
```rust
pub struct InprocCoreClient {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    pub pool: Option<sqlx::SqlitePool>,
}
```

All fields are wrapped in `Option<Arc<...>>`. The documentation should reflect this.

---

### 2. CoreRequest TurnSubmit Field Count

**Doc states** (line 86): TurnSubmit has 7 fields: `session_id`, `text`, `plan_mode`, `model`, `agents`, `current_agent_idx`, `messages`

**Actual** (`src/protocol/core.rs:115-123`):
```rust
TurnSubmit {
    session_id: String,
    text: String,
    plan_mode: bool,
    model: String,
    agents: Vec<crate::agent::Agent>,
    current_agent_idx: usize,
    messages: Vec<crate::provider::Message>,
},
```

7 fields - **CORRECT**.

---

### 3. SessionLifecycle Request Count

The document lists 18 session lifecycle variants (lines 67-83). Let me verify:

| Variant | Line in protocol/core.rs |
|---------|--------------------------|
| Initialize | 51 |
| Subscribe | 52-54 |
| Resume | 55-58 |
| SessionList | 59-63 |
| SessionCreate | 64-67 |
| SessionAttach | 68-70 |
| SessionLoad | 71-73 |
| SessionMessagesLoad | 74-76 |
| SessionMessageCounts | 77-79 |
| SessionFork | 80-82 |
| SessionDelete | 83-86 |
| SessionArchive | 87-90 |
| SessionRestore | 91-93 |
| SessionShare | 94-96 |
| SessionUnshare | 97-99 |
| SessionRename | 100-103 |
| SessionExport | 104-106 |
| SessionImportData | 107-109 |
| SessionCreateFromTemplate | 110-114 |

**Count: 19 variants**. The document lists 18. The discrepancy is that `SessionMessagesLoad` and `SessionMessageCounts` are listed under "Session Data" (lines 93-94) rather than Session Lifecycle, which is a correct categorization. So session lifecycle has 17 variants (Initialize through SessionCreateFromTemplate excluding the two "Session Data" variants).

Wait - let me recount session lifecycle as the document defines it (lines 66-83):
Initialize, Subscribe, Resume, SessionList, SessionCreate, SessionAttach, SessionLoad, SessionFork, SessionDelete, SessionArchive, SessionRestore, SessionShare, SessionUnshare, SessionRename, SessionExport, SessionImportData, SessionCreateFromTemplate

**Count: 17 variants**. The document says "Session lifecycle" contains these variants. This matches.

---

### 4. CoreEvent Snapshot Events - NOT MAPPED

**Doc states** (lines 137-140): SnapshotSession, SnapshotWorkspace, SnapshotModels are CoreEvent variants.

**Actual**: They exist in `src/protocol/core.rs:180-189` but `map_app_event_to_core_event` (`src/core/mod.rs:728-841`) does NOT map any AppEvent to these CoreEvent variants. These events appear to be defined but not published.

---

### 5. CoreEvent SessionUpdated/FileChanged - NOT MAPPED

**Doc states** (lines 157-159): SessionUpdated and FileChanged are CoreEvent variants.

**Actual**: These exist in `src/protocol/core.rs:237-242` but `map_app_event_to_core_event` does NOT convert any AppEvent to these CoreEvent types.

---

### 6. TurnCancel and TurnSteer Handler

**Doc states** (lines 124-131): TurnCancel, TurnSteer fallthrough to `Ack`.

**Actual**: These variants exist in `src/protocol/core.rs:124-132` and fallthrough at `src/core/mod.rs:698`. **CORRECT** - but the handler is a no-op. This is correctly documented as a fallthrough.

---

### 7. AppEvent to CoreEvent Mapping - Partial

The `map_app_event_to_core_event` function (`src/core/mod.rs:728-841`) maps the following AppEvents to CoreEvents:

| AppEvent | CoreEvent |
|----------|-----------|
| TextDelta | TurnTextDelta |
| ReasoningDelta | TurnReasoningDelta |
| ToolCallStarted | ToolStarted |
| ToolResult | ToolCompleted |
| PermissionPending | PermissionPending |
| QuestionPending | QuestionPending |
| AgentFinished | TurnCompleted |
| Error | Error |
| SubagentStarted | SubagentStarted |
| SubagentProgress | SubagentProgress |
| SubagentCompleted | SubagentCompleted |
| SubagentFailed | SubagentFailed |

**NOT MAPPED** (exist in CoreEvent but no AppEvent source):
- SnapshotSession
- SnapshotWorkspace  
- SnapshotModels
- TurnStarted
- TurnFailed
- SessionUpdated
- FileChanged

---

### 8. AppEvent Count

The document does not mention AppEvent count, but for reference:
- `src/bus/events.rs:5-147` defines **36 AppEvent variants** (verified 2026-05-26 per AGENTS.md)

---

## Recommendations

1. **Update InprocCoreClient field documentation** to show `Option<Arc<T>>` wrapping for all fields.

2. **Add note about Snapshot events**: SnapshotSession, SnapshotWorkspace, SnapshotModels are defined in the protocol but not currently published via `map_app_event_to_core_event`.

3. **Clarify TurnStarted event**: The CoreEvent includes `TurnStarted` at `protocol/core.rs:190-193`, but nothing in `map_app_event_to_core_event` produces this event.

4. **SessionUpdated/FileChanged mapping**: Either implement the mapping or remove these variants from the CoreEvent enum documentation if they are unused.

5. **Consider adding a mapping for TurnStarted and TurnFailed** if these are needed by consumers.

---

## Line Number References

| Topic | Doc Line | Actual Location |
|-------|----------|-----------------|
| CoreClient trait | 19-28 | `src/core/mod.rs:13-20` |
| InprocCoreClient struct | 37 | `src/core/mod.rs:22-28` |
| InprocCoreClient subscribe | 37 | `src/core/mod.rs:702-725` |
| StdioCoreClient | 38 | `src/core/transport/stdio.rs:11-46` |
| SocketCoreClient | 39 | `src/core/transport/socket.rs:11-40` |
| Protocol version | 198 | `src/protocol/core.rs:3` |
| CoreRequest enum | 64 | `src/protocol/core.rs:48-175` |
| CoreEvent enum | 133 | `src/protocol/core.rs:177-272` |
| RequestEnvelope | 49 | `src/protocol/core.rs:5-10` |
| EventEnvelope | 50 | `src/protocol/core.rs:12-20` |
| map_app_event_to_core_event | 197 | `src/core/mod.rs:728-841` |
| Fallthrough variants | 124-131 | `src/core/mod.rs:698` |