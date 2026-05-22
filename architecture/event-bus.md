# Event Bus Module

The `bus` module provides inter-component communication via an event-driven architecture.

## Overview

**Location**: `src/bus/`

**Key Responsibilities**:
- Global event publishing and subscribing via broadcast channel
- Permission request/response pattern via PermissionRegistry
- Question/answer request/response pattern via QuestionRegistry

**Files**:
- `global.rs` - GlobalEventBus singleton
- `events.rs` - AppEvent enum (40+ variants)
- `mod.rs` - PermissionRegistry and QuestionRegistry

## Components

### global.rs - GlobalEventBus

Central event distribution using tokio broadcast channel (capacity 2048):

```rust
static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn publish(event: AppEvent) {
        if GLOBAL_BUS.tx.send(event.clone()).is_err() {
            tracing::warn!("No subscribers for event");
        }
    }

    pub fn subscribe() -> broadcast::Receiver<AppEvent> {
        GLOBAL_BUS.tx.subscribe()
    }

    pub fn subscriber_count() -> usize {
        GLOBAL_BUS.tx.receiver_count()
    }
}
```

### events.rs - AppEvent Enum

40+ event variants across categories:

**Session Events**: `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted`

**Message Events**: `MessageAdded`, `MessageDeleted`

**Tool Events**: `ToolCalled`, `ToolResult`, `ToolCallStarted`

**Permission Events**: `PermissionRequested`, `PermissionGranted`, `PermissionDenied`, `PermissionPending`, `PermissionResponded`

**Question Events**: `QuestionPending`, `QuestionAnswered`

**Streaming Events**: `TextDelta` (Arc<str>), `ReasoningDelta` (Arc<str>), `AgentFinished`

**Subagent Events**: `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`

**Other**: `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`, `DiffPending`, `DiffResponded`, `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged`

### mod.rs - PermissionRegistry & QuestionRegistry

Request/response pattern for async decisions using oneshot channels:

```rust
pub struct PermissionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<PermissionChoice>, Instant)>,
}

pub struct QuestionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<String>, Instant)>,
}
```

Both use 300-second TTL with cleanup on each `register()` call.

## Key Patterns

### Registration-Before-Publish Pattern

```rust
// CORRECT: Register before publishing
let (tx, rx) = oneshot::channel();
PermissionRegistry::register(perm_id.clone(), tx);
bus.publish(PermissionPending { ... });
let choice = rx.await?;

// WRONG: Race condition
bus.publish(PermissionPending { ... });
PermissionRegistry::register(perm_id.clone(), tx);
```

### Permission Choice Enum

```rust
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

## Events Flow Diagram

```
AgentLoop                    PermissionRegistry               GlobalEventBus                   TUI
   │                               │                                │                           │
   │──► check() ──────────────────►│                                │                           │
   │                               │                                │                           │
   │   (if pending)                │                                │                           │
   │◄─────── cached ───────────────│                                │                           │
   │                               │                                │                           │
   │                               │ register(perm_id, tx)         │                           │
   │                               │◄───────────────────────────────│                           │
   │                               │                                │                           │
   │                               │                    publish(PermissionPending)            │
   │                               │                                │◄──────────────────────────│
   │                               │                                │                           │
   │                               │                                │              show dialog  │
   │                               │                                │                           │◄── user decision
   │                               │                                │                           │
   │                               │        respond(perm_id, ch) ──►│                           │
   │◄──────────────────────────────│                                │                           │
   │   (choice)                    │                                │                           │
```

## SSE Handler (`server/routes/event.rs`)

The `/api/event` SSE endpoint subscribes to the global event bus:

```rust
pub async fn sse_handler(State(_bus): State<GlobalEventBus>) -> Sse<impl Stream<Item=Result<Event, Infallible>>> {
    let mut rx = crate::bus::global::GlobalEventBus::subscribe();
    // Formats events as: event: {event_type}\ndata: {json}\n\n
    // Merged with 15-second heartbeat
}
```

Note: SSE handler subscribes directly to the global bus, not the State parameter.

## Configuration

No specific configuration - uses tokio broadcast defaults with 2048 channel capacity.

## See Also

- [agent.md](agent.md) - Agent loop publishes/consumes events
- [permission.md](permission.md) - Permission system using PermissionRegistry
- [tui.md](tui.md) - TUI subscribes to events and shows permission dialogs
- [server.md](server.md) - Server SSE endpoint architecture