# Event Bus Module

The `bus` module provides inter-component communication via an event-driven architecture.

## Overview

**Location**: `src/bus/`

**Key Responsibilities**:
- Global event publishing and subscribing via broadcast channel
- Permission request/response pattern via PermissionRegistry
- Question/answer request/response pattern via QuestionRegistry

**Event Count**: 41 event variants in `AppEvent` enum

**Files**:
- `src/bus/global.rs` - GlobalEventBus singleton using tokio broadcast channel
- `src/bus/events.rs` - AppEvent enum (41 variants) with `event_type()` method
- `src/bus/mod.rs` - PermissionRegistry and QuestionRegistry

## Components

### GlobalEventBus (`src/bus/global.rs`)

Central event distribution using tokio broadcast channel with capacity 2048:

```rust
static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn publish(event: AppEvent) {
        match GLOBAL_BUS.tx.send(event) {
            Ok(0) => tracing::debug!("No subscribers for event: {:?}", discriminant),
            Ok(n) => tracing::trace!("Event published to {} subscribers: {:?}", n, discriminant),
            Err(e) => tracing::warn!("Failed to publish event (channel closed): {:?}", e),
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

**Key characteristics**:
- Uses `LazyLock` for zero-cost singleton initialization
- Broadcast channel allows multiple subscribers
- `publish()` is synchronous (not async)
- Returns subscriber count for debugging

### AppEvent Enum (`src/bus/events.rs`)

All 41 event variants with `event_type()` method for SSE filtering:

| Category | Count | Events |
|----------|-------|--------|
| **Session** | 7 | `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted` |
| **Message** | 2 | `MessageAdded`, `MessageDeleted` |
| **Tool** | 3 | `ToolCalled`, `ToolResult`, `ToolCallStarted` |
| **MCP** | 3 | `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged` |
| **Permission** | 2 | `PermissionPending`, `PermissionResponded` |
| **Question** | 2 | `QuestionPending`, `QuestionAnswered` |
| **Streaming** | 3 | `TextDelta`, `ReasoningDelta`, `AgentFinished` |
| **Subagent** | 4 | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| **Diff** | 2 | `DiffPending`, `DiffResponded` |
| **Goal** | 4 | `GoalUpdated`, `GoalUsageUpdated`, `GoalBudgetLimited`, `GoalCompleted` |
| **Other** | 9 | `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`, `ContextUpdated` |

#### Event Type Strings

Each event variant has a string discriminator for SSE filtering:

```rust
pub fn event_type(&self) -> &'static str {
    match self {
        AppEvent::SessionCreated { .. } => "session:created",
        AppEvent::SessionUpdated { .. } => "session:updated",
        // ... etc
    }
}
```

#### Arc Optimization

Events use `Arc<str>` for `session_id` and `delta` fields where possible for efficiency:
```rust
TextDelta { session_id: Arc<str>, delta: Arc<str> }
ReasoningDelta { session_id: Arc<str>, delta: String }
```

### PermissionRegistry (`src/bus/mod.rs`)

**IMPORTANT**: All methods are `fn` (synchronous), NOT `async fn`.

The registry uses `PermissionDecision`, a bus-owned DTO for permission responses. This is distinct from `PermissionChoice` (the domain type in `src/permission/mod.rs`). Bidirectional `From` impls connect the two.

```rust
static PERMISSION_REGISTRY: Lazy<PermissionRegistry> = Lazy::new(PermissionRegistry::new);

pub struct PermissionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<PermissionDecision>, Instant)>,
}

impl PermissionRegistry {
    // Synchronous methods - do NOT use await
    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionDecision>)
    pub fn respond(perm_id: String, choice: PermissionDecision) -> bool
    pub fn unregister(perm_id: &str)
    pub fn is_registered(perm_id: &str) -> bool
    pub fn pending_permission_ids() -> Vec<String>
    fn cleanup()
}
```

**Registry Key Format**: `"{tool_call_id}-{tool_name}"`

Example: `"call_abc123-write"`

**Important Limitation**: The key does NOT include `session_id`. This means `get_pending_permissions_for_session()` cannot properly filter by session.

**TTL**: 310 seconds (5 minutes 10 seconds), cleanup runs on each `register()` call.

**Registration Pattern**:
```rust
// 1. Create oneshot channel
let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();

// 2. Register BEFORE publishing (critical!)
let perm_id = format!("{}-{}", tc.id, tc.name);
PermissionRegistry::register(perm_id.clone(), resp_tx);

// 3. Publish event
GlobalEventBus::publish(AppEvent::PermissionPending {
    session_id: self.session_id.clone(),
    perm_id: perm_id.clone(),
    tool: req.tool.clone(),
    path: req.path.clone(),
    args: req.args.clone(),
});

// 4. Wait for response with timeout (agent loop waits 300s; registry TTL is 310s)
let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
    Ok(Ok(choice)) => choice,
    _ => PermissionDecision::DenyOnce,  // timeout defaults to deny
};

// 5. Unregister after response or timeout
PermissionRegistry::unregister(&perm_id);
```

**PermissionDecision** (`src/bus/mod.rs`):
```rust
pub enum PermissionDecision {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}

impl PermissionDecision {
    pub fn allowed(&self) -> bool
    pub fn persist(&self) -> bool
}
```

`PermissionDecision` is the bus-owned DTO. It has bidirectional `From` impls with `PermissionChoice` (`src/permission/mod.rs`), which is the domain type used elsewhere in the permission module. The registry API uses `PermissionDecision` so the bus module does not depend on the permission module.

### QuestionRegistry (`src/bus/mod.rs`)

**IMPORTANT**: All methods are `fn` (synchronous), NOT `async fn`.

```rust
static QUESTION_REGISTRY: Lazy<QuestionRegistry> = Lazy::new(QuestionRegistry::new);

pub struct QuestionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<String>, Instant)>,
}

impl QuestionRegistry {
    // Synchronous methods - do NOT use await
    pub fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>)
    pub fn answer_question(question_id: String, answers: String) -> bool
    pub fn unregister(question_id: &str)
    pub fn is_registered(question_id: &str) -> bool
    pub fn pending_question_ids() -> Vec<String>
    fn cleanup()
}
```

**Registry Key Format**: `session_id` only

**Important Limitation**: The key does NOT include additional context. This means `get_pending_questions_for_session()` cannot properly filter when multiple questions are pending for the same session.

**TTL**: 310 seconds (5 minutes 10 seconds), cleanup runs on each `register()` call.

**Registration Pattern**:
```rust
// 1. Create oneshot channel
let (tx, rx) = tokio::sync::oneshot::channel();

// 2. Register BEFORE publishing
QuestionRegistry::register(self.session_id.clone(), tx);

// 3. Publish event
GlobalEventBus::publish(AppEvent::QuestionPending {
    session_id: self.session_id.clone(),
    questions: questions_json,
});

// 4. Wait for response with timeout
self.question_rx = Some(rx);

// 5. Later: let answers = self.question_rx.take().unwrap().await;
```

## Registry Limitations

### No Session ID in Registry Keys

Both PermissionRegistry and QuestionRegistry do NOT store `session_id` in their keys:

- **PermissionRegistry**: Key format is `"{tool_call_id}-{tool_name}"` (e.g., `"call_abc-write"`)
- **QuestionRegistry**: Key format is just `session_id`

This means:
- `get_pending_permissions_for_session(session_id)` cannot filter by session
- `get_pending_questions_for_session(session_id)` cannot filter by session
- Returns empty lists when session_id filtering is attempted

From `src/server/routes/permission.rs:57-73`:
```rust
/// NOTE: PermissionRegistry does not store session_id in keys, so proper session-based
/// filtering is not possible without extending the registry.
pub fn get_pending_permissions_for_session(session_id: &str) -> serde_json::Value {
    let pending_ids = crate::bus::PermissionRegistry::pending_permission_ids();
    // Keys are "{tool_call_id}-{tool_name}" not "{session_id}-..."
    // Return empty to indicate filtering is not possible.
    let permissions: Vec<serde_json::Value> = Vec::new();
    serde_json::json!({ "permissions": permissions })
}
```

## Event Flow Between Components

### Permission Flow

```
AgentLoop                      PermissionRegistry              GlobalEventBus              TUI/Server
   │                                  │                               │                        │
   │ check_tool_permission()          │                               │                        │
   │───────────────────────────────►   │                               │                        │
   │                                  │                               │                        │
   │   (if Ask)                       │                               │                        │
   │◄────── cached decision ──────────│                               │                        │
   │                                  │                               │                        │
   │   create oneshot channel         │                               │                        │
   │                                  │                               │                        │
   │   register(perm_id, tx)           │                               │                        │
   │───────────────────────────────►  │                               │                        │
   │                                  │                               │                        │
   │                                  │  publish(PermissionPending)  │                        │
   │                                  │────────────────────────────►   │                        │
   │                                  │                               │                        │
   │                                  │                               │        show dialog     │
   │                                  │                               │ ◄──────────────────────│
   │                                  │                               │                        │
   │                                  │                               │         user decision  │
   │                                  │                               │ ◄──────────────────────│
   │                                  │                               │                        │
   │                                  │  respond(perm_id, choice)    │                        │
   │                                  │ ◄──────────────────────────── │                        │
   │◄───────────────────────────────  │                               │                        │
   │   choice                        │                               │                        │
```

### Question Flow

```
AgentLoop                      QuestionRegistry                GlobalEventBus              TUI/Server
   │                                  │                               │                        │
   │ check_tool_permission()          │                               │                        │
   │ (question tool detected)         │                               │                        │
   │                                  │                               │                        │
   │   create oneshot channel         │                               │                        │
   │                                  │                               │                        │
   │   register(session_id, tx)       │                               │                        │
   │───────────────────────────────►  │                               │                        │
   │                                  │                               │                        │
   │                                  │  publish(QuestionPending)    │                        │
   │                                  │────────────────────────────►  │                        │
   │                                  │                               │                        │
   │                                  │                               │        show dialog     │
   │                                  │                               │ ◄──────────────────────│
   │                                  │                               │                        │
   │                                  │                               │         user answers   │
   │                                  │                               │ ◄──────────────────────│
   │                                  │                               │                        │
   │                                  │  answer_question(session_id, answers)                 │
   │                                  │ ◄──────────────────────────── │                        │
   │◄───────────────────────────────  │                               │                        │
   │   answers                       │                               │                        │
```

## SSE Handler (`src/server/routes/event.rs`)

The `/api/event` SSE endpoint subscribes directly to GlobalEventBus:

```rust
pub async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = GlobalEventBus::subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            if let Ok(json) = serde_json::to_string(&event) {
                let line = format!("event: {}\ndata: {}\n\n", event.event_type(), json);
                Some(Ok(Event::default().data(line)))
            } else { None }
        }
        Err(_) => None,
    });
    let heartbeat = IntervalStream::new(interval(Duration::from_secs(15)))
        .map(|_| Ok(Event::default().comment("heartbeat")));
    Sse::new(stream.merge(heartbeat))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
```

**Note**: SSE handler takes NO parameters - subscribes directly to `GlobalEventBus::subscribe()`.

## TTL Behavior

Both registries auto-expire entries after 310 seconds (5 minutes 10 seconds):

```rust
fn cleanup() {
    let ttl = Duration::from_secs(310);
    PERMISSION_REGISTRY
        .senders
        .retain(|_, (_, created)| created.elapsed() < ttl);
}
```

Cleanup is called on each `register()` operation. This prevents stale entries from accumulating.

## Key Implementation Notes

1. **Synchronous Registry Methods**: `PermissionRegistry::register()`, `PermissionRegistry::respond()`, `QuestionRegistry::register()`, and `QuestionRegistry::answer_question()` are ALL `fn` (synchronous), NOT `async fn`. Do NOT use `await` when calling these.

2. **Registration-Before-Publish Pattern**: Always register the responder channel BEFORE publishing the pending event. This prevents race conditions where the event arrives before the listener is ready.

3. **Registry Keys Lack Session ID**: Permission IDs are `"{tool_call_id}-{tool_name}"` and question IDs are just `session_id`. Neither contains enough information to filter by session in multi-session scenarios.

4. **Timeout Handling**: The agent loop waits up to 300 seconds for a permission/question response (separate from the 310-second registry TTL). On timeout, the operation defaults to deny/empty response.

5. **Unregister After Response**: Always call `unregister()` after receiving a response or after a timeout to prevent memory leaks.

## Usage Examples

### Publishing an Event

```rust
use crate::bus::global::GlobalEventBus;
use crate::bus::events::AppEvent;

// From any component (AgentLoop, tool, etc.)
GlobalEventBus::publish(AppEvent::TextDelta {
    session_id: session_id.into(),
    delta: delta.into(),
});
```

### Subscribing to Events

```rust
use crate::bus::global::GlobalEventBus;

let mut rx = GlobalEventBus::subscribe();
while let Ok(event) = rx.recv().await {
    match event {
        AppEvent::TextDelta { session_id, delta } => { /* handle */ }
        AppEvent::Error { message } => { /* handle */ }
        _ => { /* ignore */ }
    }
}
```

### Responding to Permission Request

```rust
use crate::bus::PermissionRegistry;
use crate::bus::PermissionDecision;

// In server handler or TUI
if PermissionRegistry::respond(perm_id.clone(), PermissionDecision::AllowOnce) {
    // Success
} else {
    // No pending permission found
}
```

## See Also

- [agent.md](agent.md) - AgentLoop publishes tool events, text deltas, subagent events
- [permission.md](permission.md) - Permission system using PermissionRegistry
- [tui.md](tui.md) - TUI subscribes to events and displays permission/question dialogs
- [server.md](server.md) - Server SSE endpoint architecture
- [.opencode/skills/event-bus/SKILL.md](../.opencode/skills/event-bus/SKILL.md) - Quick reference skill guide
