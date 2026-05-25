# Core Architecture Review (2026-05-25)

## Verified Correct

- **Location**: `src/core/` and `src/protocol/core.rs` match doc
- **CoreClient trait**: `request()` and `subscribe()` signatures correct (mod.rs:14-20)
- **Transport types**: `InprocCoreClient`, `StdioCoreClient`, `SocketCoreClient` all present and match doc
- **Protocol envelopes**: `RequestEnvelope<T>`, `EventEnvelope<T>` fields match (protocol/core.rs:5-20)
- **CoreRequest enum**: All documented request families present; complete enum at protocol/core.rs:50-175
- **CoreResponse enum**: `Ack`, `Json`, `Session`, `SessionMessages`, `SessionMessageCounts`, `SessionList`, `Error` - matches (protocol/core.rs:22-46)
- **Transport selection**: `--core-transport`, `CODEGG_CORE_TRANSPORT`, default `inproc` - matches doc
- **Socket reconnect**: Stdout/stderr show "reconnect" - but `SocketCoreClient::reconnect()` exists at socket.rs:29 (not documented)
- **`new_request()` helper**: Present at mod.rs:799-805

## Incorrect/Stale Items

### 1. `CoreRequest` variant list incomplete (lines 62-74)
The doc lists individual variants but omits many:
- Missing from explicit list: `SessionList`, `SessionCreate`, `SessionAttach`, `SessionDelete`, `SessionArchive`, `SessionRestore`, `SessionShare`, `SessionUnshare`, `SessionRename`, `SessionExport`, `SessionImportData`
- Added variants in actual code vs doc: `TurnSubmit` (has `text`, `plan_mode`, `model`, `agents`, `current_agent_idx`, `messages`), `TurnCancel`, `TurnSteer`, `AgentSelect`, `ModelSelect`, `ModelsRefresh`, `PermissionRespond`, `QuestionRespond`
- Memory variants: `MemorySearch`, `MemoryList`, `MemoryRemember`, `MemoryForget`
- Task variants: `TaskList`, `TaskSchedule`, `TaskDelete`
- Worktree: `WorktreeList`

### 2. `InprocCoreClient` implementation incomplete (lines 33-37)
Doc shows empty fields (`{}`), but actual has specific optional fields:
```
pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
pub pool: Option<sqlx::SqlitePool>,
```

### 3. `TurnSubmit` fields differ (lines 114-123)
Doc does not list individual `TurnSubmit` fields. Actual fields at protocol/core.rs:115-123:
- `session_id: String`
- `text: String`
- `plan_mode: bool`
- `model: String`
- `agents: Vec<crate::agent::Agent>`
- `current_agent_idx: usize`
- `messages: Vec<crate::provider::Message>`

### 4. Missing `CoreEvent` enum documentation (lines 177-272)
The doc mentions `CoreEvent` but doesn't list variants. Actual enum at protocol/core.rs:177-272 contains:
- Snapshot variants: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`
- Turn variants: `TurnStarted`, `TurnTextDelta`, `TurnReasoningDelta`, `TurnCompleted`, `TurnFailed`
- Tool variants: `ToolStarted`, `ToolCompleted`
- Permission/Question: `PermissionPending`, `QuestionPending`
- Session/File: `SessionUpdated`, `FileChanged`
- Subagent events: `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`
- Error: `Error`

### 5. `subscribe()` event mapping undocumented (mod.rs:728-796)
`map_app_event_to_core_event()` function exists but is not documented. This maps `AppEvent` → `CoreEvent` for the event bus bridge.

### 6. QuestionRespond signature differs
Doc shows `QuestionRespond { id, choice }` but actual is `QuestionRespond { id, answers: serde_json::Value }` (protocol/core.rs:146-149)

## Bugs Found in Related Code

### 1. Missing `Initialize` handling in InprocCoreClient
`CoreRequest::Initialize` is defined (protocol/core.rs:51) but returns `_ => Ok(CoreResponse::Ack)` at mod.rs:698 - no actual initialization performed.

### 2. Missing `TurnCancel` handling
`TurnCancel` is defined in `CoreRequest` (protocol/core.rs:124-127) but not handled in `InprocCoreClient::request()` match - falls through to `_ => Ok(CoreResponse::Ack)`.

### 3. Missing `TurnSteer` handling
`TurnSteer` defined (protocol/core.rs:128-132) but not handled - falls through to `_ => Ok(CoreResponse::Ack)`.

### 4. Missing `AgentSelect` and `ModelSelect` handling
Both defined (protocol/core.rs:133-140) but not handled - fall through to Ack.

### 5. `StdioCoreClient::subscribe()` returns empty channel
Per doc (line 31: "stdio and socket clients currently expose request/response transport and return an empty receiver") - this is correct behavior, not a bug, but worth noting the empty receiver is intentional.

### 6. `SessionAttach` is identical to `SessionLoad`
Both `SessionLoad { session_id }` and `SessionAttach { session_id }` execute the same match arm at mod.rs:240 - the join is intentional but undocumented.

## Line Numbers Requiring Updates

| Location | Issue |
|----------|-------|
| core.md:33-37 | `InprocCoreClient` fields documented as `{}` - should list actual fields |
| core.md:62-74 | CoreRequest variant list incomplete - add missing session/message/task/memory variants |
| core.md:115-123 | Need to document `TurnSubmit` fields explicitly |
| core.md:177-272 | `CoreEvent` variants not documented - add complete enum listing |
| protocol/core.rs:51 | `Initialize` handling is missing (returns `Ack` no-op) |
| protocol/core.rs:124-132 | `TurnCancel`, `TurnSteer`, `AgentSelect`, `ModelSelect` not handled |
