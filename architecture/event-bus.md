# Event Bus Module

The `bus` module provides the inter-component communication system using an event-driven architecture.

## Overview

**Location**: `src/bus/`

**Key Responsibilities**:
- Global event publishing and subscribing
- Permission request/response pattern
- Question/answer request/response pattern

## Components

### global.rs - GlobalEventBus

The central event distribution system using Tokio's broadcast channel:

```rust
pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
    rx: broadcast::Receiver<AppEvent>,
}
```

**Key Methods**:
- `publish(event)` - Broadcast event to all subscribers
- `subscribe()` - Get a new receiver for events
- `send(event, tx)` - Send event and await response (oneshot)

**Implementation Notes**:
- Uses `tokio::sync::broadcast` channel
- If `REDIS_URL` is set → uses Redis; otherwise → uses in-memory broadcast
- All subscribers receive events after publish

### events.rs - AppEvent Enum

Defines all 40+ event types in the system:

**Categories**:

#### Session Events
- `SessionStarted`, `SessionEnded`, `SessionSelected`
- `SessionCreated`, `SessionDeleted`, `SessionRenamed`

#### Message Events
- `MessageAdded`, `MessageDeleted`, `MessageEdited`
- `UserMessage`, `AssistantMessage`, `SystemMessage`

#### Tool Events
- `ToolCallRequested`, `ToolCalled`, `ToolResult`
- `ToolPermissionPending`, `ToolPermissionGranted`, `ToolPermissionDenied`

#### Permission Events
- `PermissionPending` - Awaiting user decision
- `PermissionGranted`, `PermissionDenied`
- `PermissionSaved` - Decision cached

#### MCP Events
- `McpServerStarted`, `McpServerStopped`, `McpServerError`
- `McpToolCall`, `McpToolResult`

#### Subagent Events
- `SubagentStarted`, `SubagentCompleted`, `SubagentFailed`

#### UI Events
- `Notification`, `Toast`, `Indicator`, `Progress`

### mod.rs - PermissionRegistry & QuestionRegistry

Enables request/response pattern for async decisions:

#### PermissionRegistry

```rust
pub struct PermissionRegistry {
    pending: Arc<Mutex<HashMap<String, PendingPermission>>>,
}

pub struct PendingPermission {
    pub choice_tx: oneshot::Sender<PermissionChoice>,
    pub details: PermissionDetails,
}
```

**Flow**:
1. Register permission request with responder channel BEFORE publishing
2. Publish `PermissionPending` event
3. Wait on channel for user response
4. Return decision

#### QuestionRegistry

Similar pattern for user questions:
```rust
pub struct QuestionRegistry {
    pending: Arc<Mutex<HashMap<String, PendingQuestion>>>,
}

pub enum QuestionOption {
    Label(String),
    Description(String),
}
```

### PermissionChoice Enum

```rust
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

## Key Patterns

### Registration-Before-Publish Pattern

When publishing `PermissionPending` or `QuestionPending`:

```rust
// CORRECT: Register responder BEFORE publishing
let (tx, rx) = oneshot::channel();
registry.register(request_id, tx)?;
bus.publish(PermissionPending { ... });
let choice = rx.await?;

// WRONG: Publish first, then register (race condition)
bus.publish(PermissionPending { ... });
registry.register(request_id, tx)?;  // Might miss response
```

### Broadcast vs Direct Send

- `publish()` - Fire-and-forget to all subscribers
- `send()` - Direct message to specific recipient with response

## Events Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         AgentLoop                                │
│                                                                  │
│  ToolCallRequested ──► PermissionChecker::check()               │
│                              │                                   │
│         ┌───────────────────┼───────────────────┐                │
│         ▼                   ▼                   ▼                │
│  ┌────────────┐      ┌────────────┐      ┌────────────┐          │
│  │   Allow    │      │    Ask     │      │    Deny    │          │
│  │  (cached)  │      │ (pending)  │      │  (cached)  │          │
│  └────────────┘      └─────┬──────┘      └────────────┘          │
│                           │                                      │
│                           ▼                                      │
│                  PermissionRegistry::register()                  │
│                           │                                      │
│                           ▼                                      │
│              GlobalEventBus::publish(PermissionPending)         │
│                           │                                      │
│                           ▼                                      │
│                  TUI receives event                              │
│                           │                                      │
│                           ▼                                      │
│                  User makes decision                            │
│                           │                                      │
│                           ▼                                      │
│              PermissionRegistry::respond()                      │
│                           │                                      │
└───────────────────────────┼───────────────────────────────────────┘
                            ▼
                    Tool execution proceeds
```

## Configuration

No specific configuration options - uses system defaults for channel sizes.

## See Also

- [agent.md](agent.md) - Agent loop that publishes/consumes events
- [permission.md](permission.md) - Permission system using PermissionRegistry
- [tui.md](tui.md) - TUI that subscribes to events and shows permission dialogs
