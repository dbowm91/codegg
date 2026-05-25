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
| `InprocCoreClient` | Runs the core in the current process. `subscribe()` reads from GlobalEventBus and forwards events to the channel. Turn execution (spawned async) publishes `AgentFinished`/`Error` events to the bus. |
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
- `Initialize` - Initialize session
- `Subscribe { session_id }` - Subscribe to session events
- `Resume { session_id, from_event_seq }` - Resume from event sequence
- `TurnCancel { session_id, turn_id }` - Cancel a turn
- `TurnSteer { session_id, turn_id, text }` - Steer with text
- `AgentSelect { session_id, agent_name }` - Select agent
- `ModelSelect { session_id, model }` - Select model
- (Plus additional variants for list, create, load, etc.)

See `src/protocol/core.rs` for complete enum definition.

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
