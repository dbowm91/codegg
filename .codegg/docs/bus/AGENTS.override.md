# Bus Module Override

This file contains event bus-specific guidance and overrides root AGENTS.md.

## Event Bus System

### GlobalEventBus
- Process-global event bus for publishing/subscribing `AppEvent` instances
- Use `GlobalEventBus::subscribe()` to get a receiver *before* spawning tasks that emit events to avoid missing events
- `AppEvent` variants include `ToolResult`, `QuestionPending`, `PermissionPending`, `AgentLoopDone`, etc.

### Session-Scoped Event Filtering
- For test isolation, use `EventCollector` with `with_session()` constructor to filter events by session ID
- All event assertions in tests should use session-filtered collectors to avoid cross-test contamination under parallel execution

### PermissionRegistry
- Registry for pending permission requests, keyed by `perm_id`
- Helpers: `register(perm_id, resp_tx)`, `answer_permission(perm_id, choice)`, `pending_permission_ids()`, `is_registered(perm_id)`
- Critical: Register responder *before* publishing `PermissionPending` event to avoid race conditions

### QuestionRegistry
- Registry for pending question requests, keyed by session ID
- Helpers: `register(session_id, resp_tx)`, `answer_question(session_id, answers_json)`, `pending_question_ids()`, `is_registered(session_id)`
- Supports session-specific pending state recovery for HTTP/websocket clients that miss events

## Known Issues

### Dead Letter Channels (HIGH)
**File:** `src/bus/mod.rs:21-89`

When a sender (permission or question response) is dropped without being answered, the entry remains in `DashMap` forever. No TTL-based cleanup mechanism exists. This can cause memory leaks in long-running sessions.

**Current behavior:**
```rust
pub async fn respond(perm_id: String, choice: PermissionChoice) -> bool {
    if let Some((_, tx)) = PERMISSION_REGISTRY.senders.remove(&perm_id) {
        let _ = tx.send(choice);  // Silent failure if receiver already dropped
        true
    } else {
        false
    }
}
```

**Recommendation:** Add a background cleanup task that removes stale entries based on age, or use a channel-based approach that automatically cleans up on drop.