# Event Bus Module

The `bus` module provides inter-component communication via an
event-driven architecture and two synchronous request/response
registries.

## Purpose

Global event publishing and subscribing via a broadcast channel, plus
synchronous permission-request and question-request registries that
pair a pending event with a oneshot response channel.

## Where It Lives

| Path | Role |
|------|------|
| `crates/codegg-core/src/bus/global.rs` | `GlobalEventBus` singleton |
| `crates/codegg-core/src/bus/events.rs` | `AppEvent` enum (45 variants) |
| `crates/codegg-core/src/bus/mod.rs` | `PermissionRegistry`, `QuestionRegistry`, `PermissionDecision`, pending-info types |

## How It Works

### GlobalEventBus

Central event distribution using a tokio broadcast channel
(capacity 4096):

```rust
static GLOBAL_BUS: LazyLock<GlobalEventBus> = LazyLock::new(GlobalEventBus::new);

pub struct GlobalEventBus {
    tx: broadcast::Sender<AppEvent>,
}
```

- `publish()` is synchronous (not async).
- `subscribe()` returns a `broadcast::Receiver<AppEvent>`.
- `subscriber_count()` reports the receiver count for debugging.
- Uses `std::sync::LazyLock` for zero-cost singleton init.

### AppEvent Enum

45 variants across these categories:

| Category | Count | Variants |
|----------|-------|----------|
| Session | 7 | `SessionCreated`, `SessionUpdated`, `SessionArchived`, `SessionForked`, `SessionShared`, `SessionUnshared`, `SessionReverted` |
| Message | 2 | `MessageAdded`, `MessageDeleted` |
| Tool | 3 | `ToolCalled`, `ToolResult`, `ToolCallStarted` |
| MCP | 3 | `McpServerConnected`, `McpServerDisconnected`, `McpToolListChanged` |
| Permission | 2 | `PermissionPending`, `PermissionResponded` |
| Question | 2 | `QuestionPending`, `QuestionAnswered` |
| Streaming | 3 | `TextDelta`, `ReasoningDelta`, `AgentFinished` |
| Subagent | 4 | `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` |
| TestRun | 3 | `TestRunStarted`, `TestRunProgress`, `TestRunCompleted` |
| Diff | 2 | `DiffPending`, `DiffResponded` |
| Goal | 4 | `GoalUpdated`, `GoalUsageUpdated`, `GoalBudgetLimited`, `GoalCompleted` |
| Other | 9 | `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged`, `ContextUpdated`, `PluginUiEffect` |

Each variant has an `event_type()` method returning a `&'static str`
discriminator for SSE filtering (e.g. `"session:created"`,
`"tool:delta"`, `"permission:pending"`).

Events use `Arc<str>` for hot-path fields (`session_id` and `delta`
on `TextDelta`; `session_id` on `ReasoningDelta`).

### PermissionRegistry

**All methods are `fn` (synchronous), NOT `async fn`.**

The registry stores a `DashMap<String, PendingPermission>` keyed by
`perm_id` (format `"{tool_call_id}-{tool_name}"`, e.g.
`"call_abc123-write"`). Each `PendingPermission` carries:

- `session_id: String` — the session that registered the permission.
- `turn_id: Option<String>` — optional turn scoping.
- `tx: oneshot::Sender<PermissionDecision>` — the response channel.
- `created_at: Instant` — for TTL expiry.

**Scoped API** (preferred for new call sites):

```rust
PermissionRegistry::register_with_session(
    session_id, turn_id, perm_id, tx,
);
PermissionRegistry::respond_scoped(session_id, perm_id, choice);
PermissionRegistry::unregister_scoped(session_id, perm_id);
PermissionRegistry::is_registered_scoped(session_id, perm_id);
PermissionRegistry::get_pending_for_session(session_id);
```

**Legacy API** (backward-compatible; uses `session_id = "default"`):

```rust
PermissionRegistry::register(perm_id, tx);
PermissionRegistry::respond(perm_id, choice);
PermissionRegistry::unregister(perm_id);
PermissionRegistry::is_registered(perm_id);
```

`PermissionDecision` is the bus-owned DTO:

```rust
pub enum PermissionDecision {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

Has `allowed()` and `persist()` helpers plus bidirectional `From`
impls with `PermissionChoice` (the domain type in
`src/permission/mod.rs`).

### QuestionRegistry

Same pattern as `PermissionRegistry` but for interactive questions.
All methods are synchronous. Key is `question_id`; each
`PendingQuestion` carries `session_id` and `turn_id`.

**Scoped API**:

```rust
QuestionRegistry::register_with_session(
    session_id, turn_id, question_id, tx,
);
QuestionRegistry::answer_question_scoped(session_id, question_id, answers);
QuestionRegistry::unregister_scoped(session_id, question_id);
QuestionRegistry::get_pending_for_session(session_id);
```

**Legacy API**:

```rust
QuestionRegistry::register(question_id, tx);
QuestionRegistry::answer_question(question_id, answers);
```

### Cleanup and TTL

Both registries auto-expire entries after 310 seconds (5 minutes
10 seconds). Cleanup is throttled to run at most once every 30
seconds (`CLEANUP_THROTTLE_MS`) via an `AtomicU64` timestamp
checked on each `register` / `pending_*_ids` call.

The `cleanup_now()` method forces an immediate sweep.

### Server Route Limitation

The `/api/permission` and `/api/question` SSE routes currently call
the legacy `pending_permission_ids()` / `pending_question_ids()`
methods, which return all pending entries without session filtering.
Because the registry keys are `perm_id` / `question_id` (not
session-scoped), these routes return empty lists to indicate
filtering is not possible. The scoped `get_pending_for_session()`
methods exist on both registries and properly filter by session.

## Key Types & APIs

| Type | File:line | Purpose |
|------|-----------|---------|
| `GlobalEventBus` | `bus/global.rs:7` | Broadcast singleton |
| `AppEvent` | `bus/events.rs:60` | 45-variant event enum |
| `PermissionRegistry` | `bus/mod.rs:88` | Permission request/response |
| `QuestionRegistry` | `bus/mod.rs:252` | Question request/response |
| `PermissionDecision` | `bus/mod.rs:11` | Bus-owned permission DTO |
| `PendingPermission` | `bus/mod.rs:46` | Stored permission with session/turn |
| `PendingPermissionInfo` | `bus/mod.rs:57` | Read-only view of pending permission |
| `PendingQuestion` | `bus/mod.rs:68` | Stored question with session/turn |
| `PendingQuestionInfo` | `bus/mod.rs:79` | Read-only view of pending question |
| `DEFAULT_SESSION_ID` | `bus/mod.rs:40` | Sentinel `"default"` for legacy calls |

## Configuration Surface

No configuration file. The broadcast channel capacity (4096) and
TTL (310 s) are compile-time constants. The cleanup throttle (30 s)
is also a constant.

## Invariants & Gotchas

1. **Registration-before-publish**: When publishing
   `PermissionPending` or `QuestionPending`, register the responder
   channel BEFORE publishing the event. This prevents race conditions
   where the event arrives before the listener is ready.

2. **Synchronous registries**: `register()`, `respond()`,
   `answer_question()` are `fn`, not `async fn`. Never `.await`
   them.

3. **Registry keys lack session_id**: The DashMap key is `perm_id`
   or `question_id`, not a session-scoped composite. The
   `session_id` is stored in the value struct and used by the
   `*_scoped` methods for atomic check-and-remove via
   `DashMap::remove_if`.

4. **Timeout handling**: The agent loop waits up to 300 seconds for
   a response. On timeout, the operation defaults to deny/empty. The
   registry TTL is 310 seconds (slightly longer to avoid stale
   entries outliving the waiting loop).

5. **Unregister after response**: Always call `unregister()` after
   receiving a response or after a timeout to prevent memory leaks.

6. **Backward compatibility**: Legacy `register(perm_id, tx)` uses
   `session_id = "default"`. Legacy `respond(perm_id, choice)`
   also matches against `session_id = "default"`. New call sites
   MUST use the `*_with_session` / `*_scoped` variants.

## Event Flow

### Permission Flow

```
AgentLoop                  PermissionRegistry         GlobalEventBus         TUI/Server
  │ check_tool_permission()                            │                    │
  │──────────────────────►                             │                    │
  │  (if Ask)                                          │                    │
  │◄───── cached decision ─────                        │                    │
  │                                                    │                    │
  │  register_with_session(sid,tid,perm_id,tx)         │                    │
  │──────────────────────►                             │                    │
  │                                                    │                    │
  │                      publish(PermissionPending) ──►│                    │
  │                                                    │───────────────────►│
  │                                                    │                    │
  │                                                    │  respond_scoped() │
  │                      ◄──────────────────────────── │◄───────────────────│
  │◄──────────────────────                             │                    │
  │  choice                                            │                    │
```

### Question Flow

Same pattern: register BEFORE publish, wait on oneshot with 300 s
timeout, unregister after completion.

## Testing

```bash
# Unit tests for bus, events, registries
cargo test -p codegg-core -- bus

# SSE handler (server feature)
cargo test --test server -- permission question
```

## Related Docs

- `architecture/permission.md` — permission domain
- `architecture/tui.md` — TUI event subscription
- `architecture/server.md` — SSE endpoint
- `architecture/agent.md` — agent loop event publishing
