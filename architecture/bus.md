# Event Bus Module

The `bus` module provides inter-component communication via an event-driven architecture.

## Overview

**Location**: `src/bus/`

**Key Responsibilities**:
- Global event publishing and subscribing via broadcast channel
- Permission request/response pattern via PermissionRegistry
- Question/answer request/response pattern via QuestionRegistry

**Event Count**: 36 event variants in `AppEvent` enum (see below for categories)

**Files**:
- `global.rs` - GlobalEventBus singleton
- `events.rs` - AppEvent enum (36 variants) with `event_type()` method for SSE filtering
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
        match GLOBAL_BUS.tx.send(event) {
            Ok(0) => tracing::debug!(
                "No subscribers for event: {:?}",
                std::mem::discriminant(&event)
            ),
            Ok(n) => tracing::trace!(
                "Event published to {} subscribers: {:?}",
                n,
                std::mem::discriminant(&event)
            ),
            Err(e) => tracing::warn!(
                "Failed to publish event (channel closed): {:?}",
                e
            ),
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

36 event variants across categories:

**Session Events (7)**: `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted`

**Message Events (2)**: `MessageAdded`, `MessageDeleted`

**Tool Events (3)**: `ToolCalled`, `ToolResult`, `ToolCallStarted`

**MCP Events (3)**: `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged`

**Permission Events (2)**: `PermissionPending`, `PermissionResponded`

**Question Events (2)**: `QuestionPending`, `QuestionAnswered`

**Streaming Events (3)**: `TextDelta` (Arc<str>), `ReasoningDelta`, `AgentFinished`

**Subagent Events (4)**: `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`

**Diff Events (2)**: `DiffPending`, `DiffResponded`

**Other Events (8)**: `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`

Note: `session_id` is `Arc<str>` in most events for efficiency, but `String` in events that originate from user input or require ownership.

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

Defined in `src/permission/mod.rs`:

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
pub async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = crate::bus::global::GlobalEventBus::subscribe();
    // Formats events as: event: {event_type}\ndata: {json}\n\n
    // Merged with 15-second heartbeat
}
```

Note: SSE handler takes NO parameters - subscribes directly to `GlobalEventBus::subscribe()`.

## Configuration

No specific configuration - uses tokio broadcast defaults with 2048 channel capacity.

## See Also

- [agent.md](agent.md) - Agent loop publishes/consumes events
- [permission.md](permission.md) - Permission system using PermissionRegistry
- [tui.md](tui.md) - TUI subscribes to events and shows permission dialogs
- [server.md](server.md) - Server SSE endpoint architecture