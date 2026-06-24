---
name: event-bus
description: GlobalEventBus publish/subscribe and event types for inter-component communication
version: 1.1.0
tags: [bus, events, pubsub, tui, agent]
---

# Event Bus System Guide

This skill covers the GlobalEventBus and event types used for pub/sub communication between components.

## GlobalEventBus (`src/bus/global.rs`)

> Module now lives in `crates/codegg-core/`. Root `src/bus/` is a re-export shim.

Singleton event bus using tokio broadcast channel (capacity 2048):

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

## Usage Pattern

```rust
use crate::bus::global::GlobalEventBus;
use crate::bus::events::AppEvent;

// Subscribe (e.g., in TUI or tests)
let mut rx = GlobalEventBus::subscribe();

// Publish events (from AgentLoop, tools, etc.)
GlobalEventBus::publish(AppEvent::TextDelta { session_id: session_id.clone(), delta: delta.into() });

// Receive
if let Ok(event) = rx.try_recv() {
    // Handle event
}
```

## AppEvent Types (`src/bus/events.rs`)

All 41 event variants with `event_type()` helper:

| Category | Events |
|----------|--------|
| Session (7) | `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted` |
| Message (2) | `MessageAdded`, `MessageDeleted` |
| Tool (3) | `ToolCalled`, `ToolResult`, `ToolCallStarted` |
| MCP (3) | `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged` |
| Permission (2) | `PermissionPending`, `PermissionResponded` |
| Question (2) | `QuestionPending`, `QuestionAnswered` |
| Streaming (3) | `TextDelta` (Arc<str>), `ReasoningDelta`, `AgentFinished` |
| Subagent (4) | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| Diff (2) | `DiffPending`, `DiffResponded` |
| Goal (4) | `GoalUpdated`, `GoalUsageUpdated`, `GoalBudgetLimited`, `GoalCompleted` |
| Other (9) | `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`, `ContextUpdated` |

Note: `event_type()` method returns string discriminants like `"session:created"` for SSE filtering.

## PermissionRegistry & QuestionRegistry (`src/bus/mod.rs`)

Request/response pattern for async user decisions:

```rust
// PermissionRegistry - keyed by perm_id (format: "{tool_call_id}-{tool_name}")
PermissionRegistry::register(perm_id.clone(), resp_tx);
GlobalEventBus::publish(AppEvent::PermissionPending { session_id, perm_id, tool, path, args });
let choice = tokio::time::timeout(Duration::from_secs(300), resp_rx).await??;
PermissionRegistry::unregister(&perm_id);

// QuestionRegistry - keyed by session_id
QuestionRegistry::register(session_id.clone(), tx);
GlobalEventBus::publish(AppEvent::QuestionPending { session_id, questions });
let answers = tokio::time::timeout(Duration::from_secs(300), rx).await??;
QuestionRegistry::unregister(&session_id);
```

### Key Patterns

- **Registration-before-publish**: Always register the responder channel BEFORE publishing the pending event
- **TTL cleanup**: Both registries auto-expire entries after 300 seconds
- **Unregister after response**: Clean up registry after receiving response or timeout

## SSE Handler (`src/server/routes/event.rs`)

The SSE endpoint at `/api/event` subscribes to GlobalEventBus:

```rust
pub async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = crate::bus::global::GlobalEventBus::subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            if let Ok(json) = serde_json::to_string(&event) {
                let line = format!("event: {}\ndata: {}\n\n", event.event_type(), json);
                Some(Ok(Event::default().data(line)))
            } else { None }
        }
        Err(_) => None,
    });
    let heartbeat = tokio_stream::wrappers::IntervalStream::new(
        tokio::time::interval(Duration::from_secs(15))
    ).map(|_| Ok(Event::default().comment("heartbeat")));
    Sse::new(stream.merge(heartbeat))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)))
}
```

Note: The SSE handler takes NO parameters - it subscribes directly to `GlobalEventBus::subscribe()`.

## Related Skills

- `.opencode/skills/agent-loop/SKILL.md` - AgentLoop event publishing
- `.skills/tui/SKILL.md` - TUI event handling
- `.opencode/skills/permission/SKILL.md` - Permission flow with PermissionRegistry
