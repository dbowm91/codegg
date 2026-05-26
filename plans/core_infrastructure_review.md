# Core Infrastructure Review - Improvement Plan

**Review Date**: 2026-05-26
**Modules Reviewed**: `core`, `protocol`, `bus`, `config`

---

## Summary of Verification

This review systematically verified all claims in `architecture/core.md`, `architecture/protocol.md`, `architecture/bus.md`, and `architecture/config.md` against the actual source code in `src/`. Overall, the documentation is highly accurate, but several specific claims were found to be stale or incorrect.

---

## Module: core.md

### Verified as Correct

| Item | Status | Location |
|------|--------|----------|
| Protocol version = 1 | ✅ Verified | `src/protocol/core.rs:3` |
| CoreClient trait (request + subscribe) | ✅ Verified | `src/core/mod.rs:13-20` |
| InprocCoreClient fields (all Option<Arc<...>> wrapped) | ✅ Verified | `src/core/mod.rs:22-28` |
| TurnSubmit handling returns Ack immediately | ✅ Verified | `src/core/mod.rs:52-176` |
| All CoreRequest variants listed | ✅ Verified | `src/protocol/core.rs:50-175` |
| All CoreEvent variants listed | ✅ Verified | `src/protocol/core.rs:177-272` |
| SubagentPool/MemoryStore/BackgroundScheduler/SqlitePool | ✅ Verified | `src/core/mod.rs:24-27` |
| map_app_event_to_core_event function exists | ✅ Verified | `src/core/mod.rs:728-841` |

### Stale Items

1. **Subagent events mapping claim (Line 291)**
   - **Docusaurus claim**: "map_app_event_to_core_event does NOT map subagent events (they fall through to None)"
   - **Actual code**: Subagent events ARE mapped at `src/core/mod.rs:795-838`
   - **Impact**: Medium - incorrectly states subagent events are not mapped
   - **Fix**: Remove/update the note that incorrectly claims subagent events are not mapped

2. **CoreClients table description (Line 37)**
   - **Docusaurus claim**: `InprocCoreClient` contains 4 fields: `subagent_pool`, `memory_store`, `bg_scheduler`, and `pool`
   - **Actual code**: All fields are `Option<Arc<...>>` wrapped which is correctly noted in description, but the documentation says "Contains 4 fields" without noting they are wrapped in Option<Arc>
   - **Impact**: Low - description is technically accurate but could be clearer
   - **Fix**: Clarify that all fields are wrapped in `Option<Arc<T>>`

---

## Module: protocol.md

### Verified as Correct

| Item | Status | Location |
|------|--------|----------|
| PROTOCOL_VERSION = 1 | ✅ Verified | `src/protocol/core.rs:3` |
| RequestEnvelope structure | ✅ Verified | `src/protocol/core.rs:6-10` |
| EventEnvelope structure | ✅ Verified | `src/protocol/core.rs:12-20` |
| CoreRequest variant count (35) | ✅ Verified | `src/protocol/core.rs:50-175` |
| CoreResponse variants | ✅ Verified | `src/protocol/core.rs:22-46` |
| CoreEvent variants | ✅ Verified | `src/protocol/core.rs:177-272` |
| TuiMessage variants | ✅ Verified | `src/protocol/tui.rs:1-75` |
| QuestionSpec structure | ✅ Verified | `src/protocol/tui.rs:77-82` |

### Stale Items

1. **Subagent events note in Implementation Notes (Line 291)**
   - **Docusaurus claim**: "Subagent events (`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`) exist in both `CoreEvent` and the event bus, but `map_app_event_to_core_event` does NOT map subagent events (they fall through to `None`)"
   - **Actual code**: Subagent events ARE mapped at `src/core/mod.rs:795-838`
   - **Impact**: Medium - directly contradicts source code
   - **Fix**: Remove or correct the incorrect statement about subagent events not being mapped

---

## Module: bus.md

### Verified as Correct

| Item | Status | Location |
|------|--------|----------|
| GlobalEventBus using broadcast channel capacity 2048 | ✅ Verified | `src/bus/global.rs:13` |
| PermissionRegistry using oneshot + TTL 300s | ✅ Verified | `src/bus/mod.rs:58-62` |
| QuestionRegistry using oneshot + TTL 300s | ✅ Verified | `src/bus/mod.rs:125-129` |
| Registration-before-publish pattern documented | ✅ Verified | Called correctly in codebase |
| PermissionChoice enum variants | ✅ Verified | `src/permission/mod.rs` |
| SSE handler subscribes to GlobalEventBus | ✅ Verified | `src/bus/global.rs:36-38` |

### Stale Items

1. **Event count claim (Line 14 and Line 65)**
   - **Docusaurus claim**: 36 event variants in `AppEvent` enum
   - **Actual count**: 34 variants in `src/bus/events.rs:5-147`
   - **Breakdown**:
     - Session Events: 7 (SessionCreated, SessionUpdated, SessionArchived, SessionForked, SessionShared, SessionUnshared, SessionReverted)
     - Message Events: 2 (MessageAdded, MessageDeleted)
     - Tool Events: 3 (ToolCalled, ToolResult, ToolCallStarted)
     - MCP Events: 3 (McpServerConnected, McpServerDisconnected, McpToolListChanged)
     - Permission Events: 2 (PermissionPending, PermissionResponded)
     - Question Events: 2 (QuestionPending, QuestionAnswered)
     - Streaming Events: 3 (TextDelta, ReasoningDelta, AgentFinished)
     - Subagent Events: 4 (SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed)
     - Diff Events: 2 (DiffPending, DiffResponded)
     - Other Events: 8 (ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged)
   - **Total**: 34, not 36
   - **Impact**: Low - documentation mismatch
   - **Fix**: Update count from 36 to 34

2. **Arc<str> session_id claim (Line 87)**
   - **Docusaurus claim**: "session_id is Arc<str> in most events for efficiency"
   - **Actual code**: Only `TextDelta` and `ReasoningDelta` use `Arc<str>` for session_id; mostly it's `String`
   - **Impact**: Low - inaccurate generalization
   - **Fix**: Clarify that only streaming events (TextDelta, ReasoningDelta) use Arc<str> for efficiency

---

## Module: config.md

### Verified as Correct

| Item | Status | Location |
|------|--------|----------|
| Config struct fields | ✅ Verified | `src/config/schema.rs:22-64` |
| ProviderConfig struct fields | ✅ Verified | `src/config/schema.rs:167-180` |
| api_key() method checks env vars first | ✅ Verified | `src/config/schema.rs:183-205` |
| ConfigWatcher struct fields | ✅ Verified | `src/config/watcher.rs:12-21` |
| ProviderConfig::merge() method | ✅ Verified | `src/config/schema.rs:207-244` |
| ServerConfig::merge() method | ✅ Verified | `src/config/schema.rs:133-162` |
| Config discovery order | ✅ Verified | `src/config/paths.rs:12-39` |
| JSONC/JSON5 parsing | ✅ Verified | `src/config/paths.rs:98-162` |
| Encryption master key lookup order | ✅ Verified | `src/config/encryption.rs` |
| Hot reload decryption (known issue fixed) | ✅ Verified | `src/config/watcher.rs:163` |
| All validation rules | ✅ Verified | `src/config/schema.rs:587-773` |

### Stale Items

1. **Known Issues section (Lines 229-249)**
   - **Issue**: The "Known Issues Fixed" section documents fixes with dates but doesn't indicate current status
   - The issues listed (encrypted keys decryption, provider config merge, medium_model validation, dead tui_config code) all appear to be fixed in current code
   - **Impact**: Low - section becomes increasingly stale over time
   - **Suggestion**: Consider migrating known issues to a CHANGELOG rather than architecture docs

2. **Config loading flow comment (Lines 165-166)**
   - **Comment**: merge_configs() comment says "later files override earlier"
   - **Actual behavior**: For HashMap fields (agents, mcp, commands, modes), later files do full replace, not merge. Provider configs do field-level merge. Instructions concatenate.
   - **Impact**: Low - misleading simplification
   - **Suggestion**: Document the actual merge semantics per field type

---

## Bug Reports

### BUG-1: Subagent Events ARE Mapped (Not Unmapped as Docs Claim)

**File**: `architecture/protocol.md:291`
**Severity**: Medium
**Description**: The documentation claims subagent events fall through to `None` in `map_app_event_to_core_event`, but the actual code at `src/core/mod.rs:795-838` shows all 4 subagent variants ARE mapped:
- `AppEvent::SubagentStarted` → `CoreEvent::SubagentStarted`
- `AppEvent::SubagentProgress` → `CoreEvent::SubagentProgress`
- `AppEvent::SubagentCompleted` → `CoreEvent::SubagentCompleted`
- `AppEvent::SubagentFailed` → `CoreEvent::SubagentFailed`

**Impact**: Documentation contradicts implementation, could mislead developers troubleshooting event flow.

---

## Improvement Suggestions

### IMP-1: Create Centralized Architecture Drift Detection

**Suggestion**: Consider adding a CI check that validates architecture docs against source code:
- Script that counts enum variants and compares to documented counts
- Regex-based extraction of struct fields from source and comparison with docs
- This would catch drift before documentation becomes stale

### IMP-2: Add Architecture Doc Test Coverage

**Suggestion**: Add unit tests that verify the documented behavior matches actual implementation:
- Test that `map_app_event_to_core_event` covers all documented mappings
- Test that registry methods have expected signatures
- Tests would serve as both validation and documentation

### IMP-3: Strengthen Event Bus Documentation

**Suggestion**:
1. Update the event count from 36 to 34
2. Clarify which specific events use `Arc<str>` vs `String` for session_id
3. Consider adding a table mapping AppEvent → CoreEvent for clarity

### IMP-4: Config Merge Semantics Documentation

**Suggestion**: The `merge_configs()` function has nuanced behavior:
- HashMap fields (agents, mcp, commands, modes): Full replace, not merge
- ProviderConfig: Field-level merge via ProviderConfig::merge()
- Instructions: Concatenation
- ServerConfig: Merged field-by-field

Document this per-field rather than as a single generic statement.

### IMP-5: Remove or Update Implementation Notes About Unmapped Events

**Suggestion**: The protocol.md note about subagent events not being mapped appears to be from an older version. Consider either:
1. Removing the note entirely if it's no longer accurate
2. Updating it to document the actual mapping behavior
3. Adding a "Historical Notes" appendix if old behavior matters for migration

### IMP-6: Document Transport Module

**Suggestion**: The `src/core/transport/` module (containing socket.rs and stdio.rs) isn't documented in core.md. Consider adding:
- Brief description of StdioCoreClient and SocketCoreClient
- Link to transport module for detailed implementation

### IMP-7: Permission/Question Registry Key Format

**Suggestion**: The `PermissionRegistry` and `QuestionRegistry` don't store `session_id` in their keys (as documented in AGENTS.md). This means `get_pending_permissions_for_session()` cannot properly filter by session_id. Consider:
1. Documenting this limitation
2. Using composite keys that include session_id
3. Or adding accessor methods that filter in-memory

---

## Files Referenced

| File | Line(s) | Notes |
|------|---------|-------|
| `src/core/mod.rs` | 13-20 | CoreClient trait |
| `src/core/mod.rs` | 22-28 | InprocCoreClient fields |
| `src/core/mod.rs` | 728-841 | map_app_event_to_core_event |
| `src/protocol/core.rs` | 1-272 | CoreRequest, CoreResponse, CoreEvent |
| `src/protocol/tui.rs` | 1-82 | TuiMessage, QuestionSpec |
| `src/bus/events.rs` | 1-190 | AppEvent enum (34 variants) |
| `src/bus/mod.rs` | 1-141 | PermissionRegistry, QuestionRegistry |
| `src/bus/global.rs` | 1-60 | GlobalEventBus |
| `src/config/schema.rs` | 22-816 | Config, ProviderConfig, validation |
| `src/config/paths.rs` | 1-766 | Config loading, merging |
| `src/config/watcher.rs` | 1-226 | ConfigWatcher |
