# Protocol Architecture Review

**Review Date:** 2026-05-26
**Reviewer:** Claude Code
**Source:** `architecture/protocol.md` vs `src/protocol/core.rs`, `src/protocol/tui.rs`, `src/core/mod.rs`

---

## Summary

| Section | Document Claims | Actual | Status |
|---------|----------------|--------|--------|
| CoreRequest variants | 35 | 35 | ✓ CORRECT |
| CoreEvent variants | 20 | 21 | ✗ WRONG |
| TuiMessage variants | 14 | 14 | ✓ CORRECT |
| Protocol version | 1 | 1 | ✓ CORRECT |

---

## CoreRequest (Line 50, core.rs)

### Variant Count: ✓ CORRECT (35)

Counted from source:
- Initialize, Subscribe, Resume (connection: 3)
- SessionList, SessionCreate, SessionAttach, SessionLoad, SessionMessagesLoad, SessionMessageCounts, SessionFork, SessionDelete, SessionArchive, SessionRestore, SessionShare, SessionUnshare, SessionRename, SessionExport, SessionImportData, SessionCreateFromTemplate (session: 16)
- TurnSubmit, TurnCancel, TurnSteer, AgentSelect, ModelSelect (turn: 5)
- ModelsRefresh (model: 1)
- PermissionRespond, QuestionRespond (permission: 2)
- MemorySearch, MemoryList, MemoryRemember, MemoryForget (memory: 4)
- TaskList, TaskSchedule, TaskDelete (task: 3)
- WorktreeList (worktree: 1)

**Total: 35 ✓**

### Session Lifecycle Categorization Issue

The document organizes Session Lifecycle as 16 variants but actually lists **19 items** including Initialize, Subscribe, and Resume which are separate connection-management variants in the code.

- Lines 61-89: Document says "(16 variants)" but lists 19 items
- Code shows Initialize (line 51), Subscribe (line 52), Resume (line 55) are at the same nesting level as Session* variants, not a subgroup

**Verdict:** Categorization is misleading. The 16 count likely refers only to Session* variants (excluding the 3 connection variants), but the document groups them together visually.

### Turn Lifecycle: ✓ CORRECT (5 variants)
Lines 90-95 correctly list 5 variants.

### Model Operations: ✓ CORRECT (1 variant)
Line 98: `ModelsRefresh` - correct.

### Permission/Question: ✓ CORRECT (2 variants)
Lines 100-102: correct.

### Memory Operations: ✓ CORRECT (4 variants)
Lines 104-108: correct.

### Task Operations: ✓ CORRECT (3 variants)
Lines 110-113: correct.

### Worktree Operations: ✓ CORRECT (1 variant)
Line 115-116: correct.

---

## CoreResponse (Line 24, core.rs)

### Table: ✓ CORRECT (7 variants)

| Variant | Fields | Documented | Actual |
|---------|--------|------------|--------|
| `Ack` | - | ✓ | ✓ (unnamed variant) |
| `Json` | `data: Value` | ✓ | ✓ |
| `Session` | `session: Session` | ✓ | ✓ |
| `SessionMessages` | `session_id, messages` | ✓ | ✓ |
| `SessionMessageCounts` | `counts: HashMap<String, usize>` | ✓ | ✓ |
| `SessionList` | `sessions: Vec<Session>` | ✓ | ✓ |
| `Error` | `code, message` | ✓ | ✓ |

All field names match source code.

---

## CoreEvent (Line 179, core.rs)

### Variant Count: ✗ WRONG

**Document claims: 20 variants**
**Actual: 21 variants**

| Category | Document | Actual | Items |
|----------|----------|--------|-------|
| Snapshot Events | 3 | 3 | SnapshotSession, SnapshotWorkspace, SnapshotModels ✓ |
| Turn Events | 5 | 7 | **Document missed TurnReasoningDelta (line 199) and TurnCompleted (line 227)** |
| Tool Events | 2 | 2 | ToolStarted, ToolCompleted ✓ |
| Permission/Question | 2 | 2 | PermissionPending, QuestionPending ✓ |
| Session Events | 2 | 2 | SessionUpdated, FileChanged ✓ |
| Subagent Events | 4 | 4 | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed ✓ |
| Error Events | 1 | 1 | Error ✓ |

**Discrepancy:** The document undercounts by 1. Turn Events should be 7, not 5.

---

## TuiMessage (tui.rs)

### Variant Count: ✓ CORRECT (14 variants)

### Categorization Issues

**Line 191-196 (Client-to-Server):** ✓ CORRECT (3)
- Input, KeyDown, MouseClick

**Line 198-200 (Connection Management):** ✓ CORRECT
- `Resize` and `Resume` correctly placed in connection management

**Line 202-204 (Response Messages):** ✓ CORRECT (2)
- PermissionResponse, QuestionResponse

**Line 206-217 (Server-to-Client Events):** ✗ WRONG COUNT
- Document says **9 variants** but there are actually **10 variants**:
  - RenderFrame, TextDelta, PermissionPending, QuestionPending, SessionInfo, SessionEnded, ToolCallStarted, ToolResult, Error, ResyncRequired
- ResyncRequired is listed in the "Special" section (line 219-221) but is actually a server-to-client event
- **The 10th variant is `ResyncRequired` which is categorized separately but functions as server-to-client**

### Special (2): ✓ CORRECT
- EventEnvelope and ResyncRequired are correctly listed as Special variants

---

## QuestionSpec

### Fields: ✓ CORRECT

Document (lines 224-230) matches source (tui.rs:77-82):

```rust
pub struct QuestionSpec {
    pub id: String,           // ✓
    pub prompt: String,        // ✓
    pub default: Option<String>, // ✓
}
```

---

## Envelopes

### RequestEnvelope: ✓ CORRECT (lines 34-40)

```rust
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,  // ✓
    pub request_id: String,     // ✓
    pub payload: T,             // ✓
}
```

### EventEnvelope: ✓ CORRECT (lines 46-55)

```rust
pub struct EventEnvelope<T> {
    pub protocol_version: u32,   // ✓
    pub event_seq: u64,          // ✓
    pub timestamp_ms: i64,       // ✓
    pub session_id: Option<String>, // ✓
    pub turn_id: Option<String>,    // ✓
    pub payload: T,              // ✓
}
```

---

## Implementation Notes

### Line 291: ✓ CORRECT

> "Subagent events (`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`) exist in both `CoreEvent` and the event bus, and `map_app_event_to_core_event` DOES map all four subagent events (see `src/core/mod.rs:795-838`)"

Verified: `src/core/mod.rs:795-838` shows all four Subagent events are mapped:
- Lines 795-805: SubagentStarted
- Lines 806-816: SubagentProgress
- Lines 817-827: SubagentCompleted
- Lines 828-838: SubagentFailed

### Version: ✓ CORRECT
`PROTOCOL_VERSION = 1` at line 3 of core.rs matches document.

---

## Issues Found

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| CoreEvent count | Medium | Line 139 | Claims 20, actual is 21 |
| Turn events count | Medium | Line 153 | Claims 5, actual is 7 (missing TurnReasoningDelta and TurnCompleted) |
| TuiMessage Server-to-Client count | Low | Line 206 | Claims 9, actual is 10 |
| Session Lifecycle grouping | Low | Lines 61-89 | Lists 19 items under "16 variants" - misleading categorization |

---

## Recommendations

1. **Update CoreEvent count** from 20 to 21
2. **Update Turn Events** section to include TurnReasoningDelta and TurnCompleted
3. **Clarify Session Lifecycle grouping** - either:
   - Move Initialize/Subscribe/Resume to separate "Connection Management" section, OR
   - Update count to 19 and retitle to "Session & Connection Lifecycle"
4. **Reclassify ResyncRequired** as Server-to-Client (10 variants) or clarify why it's in "Special"
5. **Update Server-to-Client count** from 9 to 10

---

## Verified Correct Items

- ✓ Protocol version constant location and value
- ✓ Envelope field counts and names
- ✓ CoreResponse variant count (7) and all field names
- ✓ CoreRequest variant count (35)
- ✓ TuiMessage total variant count (14)
- ✓ QuestionSpec fields
- ✓ All serde attributes documented correctly
- ✓ Subagent event mapping verification line number
- ✓ Flow diagrams (accurate as architectural overview)