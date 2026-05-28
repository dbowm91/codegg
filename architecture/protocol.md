# Protocol Module

The `protocol` module defines the shared request/response envelopes and message types used for communication between TUI, client, server, and core components.

## Overview

**Location**: `src/protocol/`

**Key Responsibilities**:
- Define `CoreRequest` and `CoreResponse` for TUI/Core communication
- Define `TuiMessage` for server-to-TUI event streaming
- Provide versioned `RequestEnvelope` and `EventEnvelope` wrappers
- Establish the protocol boundary between UI and backend

## Module Structure

```
protocol/
├── mod.rs     # Module exports
├── core.rs    # CoreRequest, CoreResponse, CoreEvent, envelopes
└── tui.rs     # TuiMessage, QuestionSpec
```

## Protocol Version

```rust
pub const PROTOCOL_VERSION: u32 = 1;
```

## Envelopes

### RequestEnvelope

```rust
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}
```

Wraps all requests with protocol version and a unique request ID for tracing/reply correlation.

### EventEnvelope

```rust
pub struct EventEnvelope<T> {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: T,
}
```

Wraps events with sequence number, timestamp, and optional session/turn context for ordered delivery.

## CoreRequest Enum

Located in `src/protocol/core.rs`. Variant count: **35**.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreRequest { ... }
```

### Session Lifecycle (19 variants)
- `Initialize` - Initialize session
- `Subscribe { session_id }` - Subscribe to session events
- `Resume { session_id, from_event_seq }` - Resume from event sequence
- `SessionList { project_id, show_archived, limit }` - List sessions
- `SessionCreate { directory, title }` - Create new session
- `SessionAttach { session_id }` - Attach to session
- `SessionLoad { session_id }` - Load session data
- `SessionMessagesLoad { session_id }` - Load session messages
- `SessionMessageCounts { session_ids }` - Get message counts
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

### Turn Lifecycle (5 variants)
- `TurnSubmit { session_id, text, plan_mode, model, agents, current_agent_idx, messages }` - Submit a turn
- `TurnCancel { session_id, turn_id }` - Cancel a turn
- `TurnSteer { session_id, turn_id, text }` - Steer turn with text
- `AgentSelect { session_id, agent_name }` - Select agent
- `ModelSelect { session_id, model }` - Select model

### Model Operations (1 variant)
- `ModelsRefresh` - Refresh model list

### Permission/Question (2 variants)
- `PermissionRespond { id, choice }` - Respond to permission request
- `QuestionRespond { id, answers }` - Respond to question

### Memory Operations (4 variants)
- `MemorySearch { query }` - Search memory
- `MemoryList { namespace }` - List memory entries
- `MemoryRemember { text, namespace }` - Remember to memory
- `MemoryForget { id }` - Forget from memory

### Task Operations (3 variants)
- `TaskList` - List tasks
- `TaskSchedule { session_id, interval_secs, message }` - Schedule task
- `TaskDelete { id }` - Delete task

### Worktree Operations (1 variant)
- `WorktreeList { project_dir }` - List worktrees

## CoreResponse Enum

Located in `src/protocol/core.rs` (CoreResponse section).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse { ... }
```

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Ack` | - | Empty acknowledgement |
| `Json` | `data: Value` | Generic JSON payload |
| `Session` | `session: Session` | Full session object |
| `SessionMessages` | `session_id, messages` | Session message history |
| `SessionMessageCounts` | `counts: HashMap<String, usize>` | Message counts per session |
| `SessionList` | `sessions: Vec<Session>` | List of sessions |
| `Error` | `code, message` | Error response |

## CoreEvent Enum

Located in `src/protocol/core.rs` (CoreEvent section).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent { ... }
```

### Snapshot Events (3)
- `SnapshotSession { session_id }` - Session state snapshot
- `SnapshotWorkspace { project_dir }` - Workspace snapshot
- `SnapshotModels { current_model, models }` - Model list snapshot

### Turn Events (7)
- `TurnStarted { session_id, turn_id }` - Turn started
- `TurnTextDelta { session_id, turn_id, delta }` - Text delta
- `TurnReasoningDelta { session_id, turn_id, delta }` - Reasoning delta
- `TurnCompleted { session_id, turn_id, stop_reason }` - Turn completed
- `TurnFailed { session_id, turn_id, message }` - Turn failed (turn_id is `Option<String>`)
- `ToolStarted { session_id, turn_id, tool_name, tool_id, arguments }` - Tool started (turn_id is `Option<String>`)
- `ToolCompleted { session_id, turn_id, tool_id, output, success }` - Tool completed (turn_id is `Option<String>`)

### Permission/Question Events (2)
- `PermissionPending { id, tool, path }` - Permission pending
- `QuestionPending { id, questions }` - Question pending

### Session Events (2)
- `SessionUpdated { session_id }` - Session updated
- `FileChanged { path, action }` - File changed

### Subagent Events (4)
- `SubagentStarted { session_id, task_id, agent, description }` - Subagent started
- `SubagentProgress { session_id, task_id, agent, message }` - Subagent progress
- `SubagentCompleted { session_id, task_id, agent, result_summary }` - Subagent completed
- `SubagentFailed { session_id, task_id, agent, error }` - Subagent failed

### Error Events (1)
- `Error { code, message }` - Error occurred

## TuiMessage Enum

Located in `src/protocol/tui.rs`.

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub enum TuiMessage { ... }
```

### Client-to-Server (3)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text` | User text input |
| `KeyDown` | `key, modifiers` | Key press event |
| `MouseClick` | `x, y` | Mouse click event |

### Connection Management (2)
- `Resize { w, h }` - Terminal resize
- `Resume { from_event_seq }` - Resume from event sequence

### Response Messages (2)
- `PermissionResponse { id, choice }` - Permission response
- `QuestionResponse { id, answers }` - Question response

### Server-to-Client Events (9)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `RenderFrame` | `content` | Rendered frame content |
| `TextDelta` | `delta` | Text delta for streaming |
| `PermissionPending` | `id, tool, path` | Permission request |
| `QuestionPending` | `id, questions` | Question request |
| `SessionInfo` | `id, model` | Session information |
| `SessionEnded` | `stop_reason` | Session ended |
| `ToolCallStarted` | `tool_name, tool_id, arguments` | Tool call started |
| `ToolResult` | `tool_id, output, success` | Tool result |
| `Error` | `message` | Error message |

### Special (1)
- `ResyncRequired { reason, pending_permissions, pending_questions }` - Resync needed

### QuestionSpec

```rust
pub struct QuestionSpec {
    pub id: String,
    pub prompt: String,
    pub default: Option<String>,
}
```

## Request/Response Flow

### In-Process Flow (InprocCoreClient)

```
TUI/App
  │
  │ request(RequestEnvelope<CoreRequest>)
  ▼
InprocCoreClient::request()
  │
  ├─► match CoreRequest variant
  │     └─► Database / SessionStore / AgentLoop
  │           └─► CoreResponse
  │
  └─► subscribe() → GlobalEventBus → EventEnvelope<CoreEvent>
```

### Stdio/Socket Flow

```
TUI/App
  │
  │ request(RequestEnvelope<CoreRequest>)
  ▼
StdioCoreClient / SocketCoreClient
  │
  │ JSONL over stdin/stdout or Unix socket
  ▼
core-stdio process / remote server
  │
  └─► Response written back as JSONL
```

### Remote TUI Flow (Server)

```
Remote TUI Client
  │
  │ WebSocket / HTTP
  ▼
Server (Axum)
  │
  ├─► CoreClient.request() → CoreResponse
  │
  └─► TuiMessage events → WebSocket push
```

## Versioning

The protocol uses explicit versioning via `PROTOCOL_VERSION = 1` in `src/protocol/core.rs`. Envelopes include `protocol_version` to detect mismatches between client and server.

## Implementation Notes

- `CoreRequest` and `CoreResponse` use `#[serde(tag = "type")]` for JSON discrimination
- `TuiMessage` similarly uses `#[serde(tag = "type")]`
- All enums use `rename_all = "snake_case"` for JSON compatibility
- The core module handles `CoreRequest` variants in `src/core/mod.rs`
- Subagent events (`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`) exist in both `CoreEvent` and the event bus, and `map_app_event_to_core_event` DOES map all four subagent events (see `src/core/mod.rs:795-838`)