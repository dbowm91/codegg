---
name: event-bus
description: GlobalEventBus publish/subscribe and event types for inter-component communication
tags: [bus, events, pubsub, tui, agent]
---

# Event Bus System Guide

This skill covers the GlobalEventBus and event types used for pub/sub communication between components in opencode-rs.

## GlobalEventBus (`src/bus/global.rs`)

Singleton event bus for pub/sub communication:

```rust
pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl GlobalEventBus {
    pub fn publish(event: AppEvent) {
        let _ = GLOBAL_BUS.tx.send(event);
    }

    pub fn subscribe() -> broadcast::Receiver<AppEvent> {
        GLOBAL_BUS.tx.subscribe()
    }
}
```

### Usage Pattern

```rust
use opencode_rs::bus::global::GlobalEventBus;
use opencode_rs::bus::events::AppEvent;

// Subscribe to events (e.g., in TUI or tests)
let mut rx = GlobalEventBus::subscribe();

// Publish events (from AgentLoop, tools, etc.)
GlobalEventBus::publish(AppEvent::TextDelta { ... });

// Receive events
if let Ok(event) = rx.try_recv() {
    // Handle event
}
```

## AppEvent Types (`src/bus/events.rs`)

The `AppEvent` enum defines all events in the system:

### Session Events
```rust
SessionCreated { id: String, project_id: String },
SessionUpdated { id: String },
SessionArchived { id: String },
SessionForked { parent_id: String, child_id: String },
SessionShared { id: String, url: String },
SessionUnshared { id: String },
SessionReverted { id: String, to_message: String },
```

### Message Events
```rust
MessageAdded { session_id: String, message_id: String },
MessageDeleted { session_id: String, message_id: String },
```

### Tool Events
```rust
ToolCalled { tool: String, session_id: String },
ToolResult { tool_id: String, tool_name: String, session_id: String, output: String, success: bool },
```

### Permission Events
```rust
PermissionRequested { tool: String, path: Option<String> },
PermissionGranted { tool: String, persist: bool },
PermissionDenied { tool: String },
PermissionPending { session_id: String, perm_id: String, tool: String, path: Option<String>, args: Option<serde_json::Value> },
PermissionResponded { session_id: String, tool: String, allowed: bool },
```

### Question Events
```rust
QuestionPending { session_id: String, questions: String },
QuestionAnswered { session_id: String, answers: String },
```

### Agent/Model Events
```rust
AgentChanged { name: String },
ModelChanged { model: String },
CompactionTriggered { session_id: String },
```

### Streaming Events
```rust
TextDelta { session_id: Arc<str>, delta: Arc<str> },
ReasoningDelta { session_id: Arc<str>, delta: String },
ToolCallStarted { session_id: String, tool_name: String, tool_id: String, arguments: String },
AgentFinished { session_id: String, stop_reason: String },
```

### Subagent Events
```rust
SubagentStarted { session_id: String, task_id: u64, agent: String, description: String },
SubagentProgress { session_id: String, task_id: u64, agent: String, message: String },
SubagentCompleted { session_id: String, task_id: u64, agent: String, result_summary: String },
SubagentFailed { session_id: String, task_id: u64, agent: String, error: String },
```

### Other Events
```rust
McpServerConnected { name: String },
McpServerDisconnected { name: String },
McpToolListChanged { name: String },
ConfigChanged,
TodoUpdated { session_id: String },
FileChanged { path: String, action: String },
Error { message: String },
Info { message: String },
DiffPending { session_id: String, path: String, old_content: String, new_content: String },
DiffResponded { session_id: String, path: String, accepted: bool },
```

### Event Type Helper

Get string representation for assertions:

```rust
impl AppEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            AppEvent::TextDelta { .. } => "text:delta",
            AppEvent::ToolCallStarted { .. } => "tool_call:started",
            AppEvent::ToolResult { .. } => "tool:result",
            AppEvent::AgentFinished { .. } => "agent:finished",
            // ... other variants
        }
    }
}
```

## EventCollector Helper (Packet 11, Updated in Packet 1)

For collecting and asserting events in tests (`tests/agent_loop_harness.rs`):

```rust
struct EventCollector {
    rx: broadcast::Receiver<AppEvent>,
    events: Vec<AppEvent>,
    session_id: Option<String>,  // NEW in Packet 1
}

impl EventCollector {
    /// Creates collector that captures ALL events (no filtering)
    fn new() -> Self {
        let rx = GlobalEventBus::subscribe();
        Self { rx, events: Vec::new(), session_id: None }
    }

    /// Creates collector that filters events by session_id (Packet 1)
    fn with_session(session_id: String) -> Self {
        let rx = GlobalEventBus::subscribe();
        Self { rx, events: Vec::new(), session_id: Some(session_id) }
    }

    /// Drains all available events from the receiver, filtering by session_id if set
    fn collect(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if self.event_matches_session(&event) {
                        self.events.push(event);
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    panic!("EventCollector lagged by {} events", n);
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
    }

    /// Check if event matches our session filter
    fn event_matches_session(&self, event: &AppEvent) -> bool {
        match &self.session_id {
            None => true,  // No filter
            Some(sid) => {
                // Check session_id for various event types
                match event {
                    AppEvent::ToolCalled { session_id, .. } => session_id == sid,
                    AppEvent::ToolResult { session_id, .. } => session_id == sid,
                    AppEvent::TextDelta { session_id, .. } => session_id.as_ref() == sid,
                    // ... other event types with session_id
                    _ => false,
                }
            }
        }
    }

    fn events(&self) -> &[AppEvent] { &self.events }

    fn find_event(&self, f: impl Fn(&AppEvent) -> bool) -> Option<&AppEvent> {
        self.events.iter().find(|e| f(e))
    }

    /// Asserts that event types appear in order (not necessarily consecutively)
    fn assert_event_order(&self, expected_types: &[&str]) {
        let actual_types: Vec<&str> = self.events.iter().map(|e| e.event_type()).collect();
        // Verify expected_types appear in order
    }
}
```

### Example Usage in Tests

```rust
#[tokio::test]
async fn test_event_order() {
    let mut event_collector = EventCollector::new();

    // Run agent loop...
    let result = agent_loop.run(request).await;

    event_collector.collect();

    // Assert event order
    event_collector.assert_event_order(&[
        "text:delta",
        "tool_call:started",
        "tool:result",
        "agent:finished",
    ]);

    // Find specific events
    let tool_result = event_collector.find_event(|e| matches!(e, AppEvent::ToolResult { .. }));
    assert!(tool_result.is_some());

    if let AppEvent::ToolResult { tool_id, success, .. } = tool_result.unwrap() {
        assert_eq!(tool_id, "call_1");
        assert!(*success);
    }
}
```

## TUI Integration (`src/tui/mod.rs`)

The TUI subscribes to GlobalEventBus and handles events in the event loop:

```rust
pub async fn run_event_loop(app: &mut App) -> Result<(), AppError> {
    let mut bus_rx = GlobalEventBus::subscribe();

    tokio::select! {
        biased;

        Some(result) = reader.next() => { /* keyboard/mouse */ }

        Ok(event) = bus_rx.recv() => {
            match event {
                AppEvent::TextDelta { delta, .. } => {
                    app.messages_state.messages.add_assistant_text(delta);
                }
                AppEvent::ToolCallStarted { tool_name, tool_id, arguments, .. } => {
                    app.messages_state.messages.add_tool_call(tool_id, tool_name, arguments);
                }
                AppEvent::AgentFinished { .. } => {
                    app.session_state.session_status = SessionStatus::Idle;
                }
                // ... handle other events
            }
        }
    }
}
```

## Related Skills

- See `.opencode/skills/agent-loop/SKILL.md` for AgentLoop event publishing
- See `.opencode/skills/tui/SKILL.md` for TUI event handling
- See `tests/agent_loop_harness.rs` for EventCollector usage examples
