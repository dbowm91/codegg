# Core Runtime

The `core` module is the request/response facade that separates TUI transport from the underlying agent and session logic.

## Overview

**Location**: `src/core/`

**Key Responsibilities**:
- Provide a typed request/response boundary for UI and transport adapters
- Centralize session, memory, task, worktree, permission, and question operations
- Support in-process, stdio, and socket-backed execution modes
- Bridge core events into the global event bus when running in-process

## Public API

### `CoreClient`

```rust
#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}
```

`subscribe()` is event-capable for the in-process client. The stdio and socket clients currently expose request/response transport and return an empty receiver.

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process. Contains 4 fields: `subagent_pool` (`Option<Arc<SubAgentPool>>`), `memory_store` (`Option<Arc<MemoryStore>>`), `bg_scheduler` (`Option<Arc<BackgroundScheduler>>`), and `pool` (`Option<sqlx::SqlitePool>` -- not wrapped in `Arc`, unlike the other three). `subscribe()` reads from GlobalEventBus and forwards events to the channel. Turn execution (spawned async) publishes `AgentFinished`/`Error` events to the bus. |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to a Unix socket endpoint and exchanges JSONL requests/responses |

## Protocol

Defined in `src/protocol/core.rs`.

### Envelopes

| Type | Purpose |
|------|---------|
| `RequestEnvelope<T>` | Wraps requests with `protocol_version` and `request_id` |
| `EventEnvelope<T>` | Wraps events with sequence, timestamp, and optional session/turn metadata |
| `CoreRequest` | Typed requests for sessions, turns, memory, tasks, worktrees, permissions, questions, and model refresh |
| `CoreResponse` | Typed responses for acknowledgements, JSON payloads, sessions, and errors |
| `CoreEvent` | Core-side event stream for in-process subscribers |

### Request Families

- Session lifecycle: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, create-from-template, initialize, subscribe, resume
- Turn lifecycle: submit, cancel, steer, agent select, model select
- Session data: message loading and message counts
- Operational helpers: model refresh, permission/question response, memory CRUD, task CRUD/scheduling, worktree listing

#### Explicit CoreRequest Variants

The `CoreRequest` enum (in `src/protocol/core.rs`) contains these variants:

**Session Lifecycle:**
- `Initialize` - Initialize session
- `Subscribe { session_id }` - Subscribe to session events
- `Resume { session_id, from_event_seq }` - Resume from event sequence
- `SessionList { project_id, show_archived, limit }` - List sessions
- `SessionCreate { directory, title }` - Create new session
- `SessionAttach { session_id }` - Attach to session
- `SessionLoad { session_id }` - Load session data
- `SessionFork { session_id }` - Fork session
- `SessionDelete { session_id, permanent }` - Delete session
- `SessionArchive { session_id, unarchive }` - Archive/unarchive session
- `SessionRestore { session_id }` - Restore session
- `SessionShare { session_id }` - Share session
- `SessionUnshare { session_id }` - Unshare session
- `SessionRename { session_id, new_title }` - Rename session
- `SessionExport { session_id }` - Export session
- `SessionImportData { data }` - Import session data
- `SessionCreateFromTemplate { template, project_id, directory }` - Create from template

**Turn Lifecycle:**
- `TurnSubmit { session_id, text, plan_mode, model, agents, current_agent_idx, messages }` - Submit a turn for execution. `session_id` identifies the session, `text` is user input, `plan_mode` enables planning, `model` specifies the LLM model, `agents` is the agent configuration array, `current_agent_idx` selects active agent, and `messages` is conversation history.
- `TurnCancel { session_id, turn_id }` - Cancel a turn
- `TurnSteer { session_id, turn_id, text }` - Steer with text
- `AgentSelect { session_id, agent_name }` - Select agent
- `ModelSelect { session_id, model }` - Select model

**Session Data:**
- `SessionMessagesLoad { session_id }` - Load session messages
- `SessionMessageCounts { session_ids }` - Get message counts

**Operational Helpers:**
- `ModelsRefresh` - Refresh model list
- `PermissionRespond { id, choice }` - Respond to permission request
- `QuestionRespond { id, answers }` - Respond to question
- `MemorySearch { query }` - Search memory
- `MemoryList { namespace }` - List memory entries
- `MemoryRemember { text, namespace }` - Remember to memory
- `MemoryForget { id }` - Forget from memory
- `TaskList` - List tasks
- `TaskSchedule { session_id, interval_secs, message }` - Schedule task
- `TaskDelete { id }` - Delete task
- `WorktreeList { project_dir }` - List worktrees

#### Request Handler Behavior

**Handled variants** (produce meaningful response):
- `TurnSubmit` - Spawns agent loop, returns `Ack` immediately
- `SessionMessagesLoad` - Returns session messages
- `SessionMessageCounts` - Returns message counts
- `SessionCreate` - Creates and returns session
- `SessionLoad` / `SessionAttach` - Loads and returns session
- All other session variants (List, Fork, Delete, Archive, Restore, Share, Unshare, Rename, Export, Import, CreateFromTemplate)
- `PermissionRespond` / `QuestionRespond` - Registry responses
- `ModelsRefresh` - Returns refreshed model list
- `TaskList` / `TaskSchedule` / `TaskDelete` - Task operations
- `MemoryList` / `MemorySearch` / `MemoryRemember` / `MemoryForget` - Memory operations
- `WorktreeList` - Returns worktree list

**Fallthrough variants** (return `Ack` without processing):
- `Initialize`
- `Subscribe`
- `Resume`
- `TurnCancel`
- `TurnSteer`
- `AgentSelect`
- `ModelSelect`

#### CoreEvent Enum

The `CoreEvent` enum (in `src/protocol/core.rs`) is published by the core and received by in-process subscribers via `subscribe()`.

**Note**: Snapshot events (`SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`) are defined in `CoreEvent` but are **not published** via `map_app_event_to_core_event` at `src/core/mod.rs:733-848`. The mapping function returns `None` for snapshot events (they fall through to the catch-all `_ => None` case). This is intentional because snapshot events are handled directly by the snapshot system in `src/snapshot/mod.rs` - they bypass the normal event publication flow and are instead triggered through explicit `CoreRequest::SnapshotSession`/`SnapshotWorkspace`/`SnapshotModels` requests. The snapshot subsystem manages its own event emission separately from the global event bus.

**Snapshot Events:**
- `SnapshotSession { session_id }` - Session state snapshot requested
- `SnapshotWorkspace { project_dir }` - Workspace snapshot requested
- `SnapshotModels { current_model, models }` - Model list snapshot

**Turn Events:**
- `TurnStarted { session_id, turn_id }` - Turn execution started
- `TurnTextDelta { session_id, turn_id, delta }` - Text delta received
- `TurnReasoningDelta { session_id, turn_id, delta }` - Reasoning delta received
- `TurnCompleted { session_id, turn_id, stop_reason }` - Turn completed
- `TurnFailed { session_id, turn_id, message }` - Turn failed

**Tool Events:**
- `ToolStarted { session_id, turn_id, tool_name, tool_id, arguments }` - Tool execution started
- `ToolCompleted { session_id, turn_id, tool_id, output, success }` - Tool completed

**Permission/Question Events:**
- `PermissionPending { id, tool, path }` - Permission request pending
- `QuestionPending { id, questions }` - Question pending response

**Session Events:**
- `SessionUpdated { session_id }` - Session was updated
- `FileChanged { path, action }` - File changed event

**Subagent Events:**
- `SubagentStarted { session_id, task_id, agent, description }` - Subagent started
- `SubagentProgress { session_id, task_id, agent, message }` - Subagent progress
- `SubagentCompleted { session_id, task_id, agent, result_summary }` - Subagent completed
- `SubagentFailed { session_id, task_id, agent, error }` - Subagent failed

**Error Events:**
- `Error { code, message }` - Error occurred

## Transport Modes

### In-Process

The default mode keeps the core in the same binary and routes requests through `InprocCoreClient`. This is the simplest local development path and preserves event publication.

### Stdio

The stdio adapter is started with the hidden `core-stdio` command. It reads `RequestEnvelope<CoreRequest>` values from stdin, writes `CoreResponse` values to stdout, and initializes the full core backend in-process.

### Socket

The socket adapter connects to a Unix socket endpoint, currently using JSONL request/response framing with a reconnect-and-retry-once strategy.

## Startup Selection

The TUI chooses the core transport from:

1. `--core-transport`
2. `CODEGG_CORE_TRANSPORT`
3. Default `inproc`

If socket mode is selected, `--core-endpoint` or `CODEGG_CORE_ENDPOINT` must provide the Unix socket path.

## Implementation Notes

- Local TUI flows should prefer `CoreClient` over direct store access when a request already exists in `CoreRequest`.
- The in-process client **subscribes** to the GlobalEventBus (via `subscribe()`) and forwards events to the channel receiver. The actual event publishing (`AgentFinished`, `Error`) happens inside `tokio::spawn` within turn execution handlers.
- The core protocol version is currently `1`.
