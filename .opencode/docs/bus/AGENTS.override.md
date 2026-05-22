# Bus Module Override

Event bus-specific guidance and overrides root AGENTS.md.

## Event Bus System

### GlobalEventBus (`src/bus/global.rs`)
- Process-global event bus using tokio broadcast channel (2048 capacity)
- `GlobalEventBus::subscribe()` returns a new receiver for events
- `GlobalEventBus::publish()` logs warning if no subscribers (fire-and-forget)
- `GlobalEventBus::subscriber_count()` for testing/monitoring

### AppEvent (`src/bus/events.rs`)
- 40+ event variants across: Session, Message, Tool, Permission, Question, Streaming, Subagent, File, Diff, MCP, Config, Agent categories
- Each event has `event_type()` method returning string like "text:delta", "tool_call:started"
- Uses `Arc<str>` for `TextDelta` and `ReasoningDelta` to reduce cloning

### PermissionRegistry (`src/bus/mod.rs`)
- Registry for pending permission requests, keyed by `perm_id` (format: `"{tool_call_id}-{tool_name}"`)
- **Helpers**: `register()`, `respond()`, `unregister()`, `is_registered()`, `pending_permission_ids()`
- Critical: Register responder **BEFORE** publishing `PermissionPending` event
- TTL: 300 seconds, cleanup runs on each `register()` call

### QuestionRegistry (`src/bus/mod.rs`)
- Registry for pending question requests, keyed by `session_id`
- **Helpers**: `register()`, `answer_question()`, `unregister()`, `is_registered()`, `pending_question_ids()`
- Session-level flow: `has_pending_question` flag in AgentLoop ensures one question at a time per session
- TTL: 300 seconds, cleanup runs on each `register()` call

### SSE Handler (`src/server/routes/event.rs`)
- SSE endpoint at `/api/event` subscribes directly to `crate::bus::global::GlobalEventBus::subscribe()`
- Does NOT use the State parameter's isolated `GlobalEventBus` (that was a bug - now fixed)
- Format: `event: {event_type}\ndata: {json}\n\n` with 15-second heartbeats

## Key Patterns

### Registration-Before-Publish
```rust
// CORRECT
let (tx, rx) = oneshot::channel();
PermissionRegistry::register(perm_id.clone(), tx);  // Register first
bus.publish(AppEvent::PermissionPending { ... });    // Then publish
let choice = rx.await?;

// WRONG - race condition
bus.publish(AppEvent::PermissionPending { ... });
PermissionRegistry::register(perm_id.clone(), tx);  // Might miss response
```

### Event Collection in Tests
Use `EventCollector` from `tests/agent_loop_harness.rs` with session filtering:
```rust
let mut event_collector = EventCollector::with_session(session_id.clone());
// Run test...
event_collector.collect();
event_collector.assert_event_order(&["text:delta", "tool_call:started", "tool:result"]);
```

## Findings from Review (2026-05-22)

1. **SSE bus isolation bug fixed**: SSE handler now uses global bus directly
2. **PermissionRegistry::respond() cleanup**: Entry removed by `.remove()` even if send fails - no stale entries
3. **QuestionRegistry uses session_id as key**: Safe because AgentLoop processes questions sequentially per session
4. **cleanup() runs on register()**: O(n) on map size but only 300s TTL, acceptable for small registry sizes