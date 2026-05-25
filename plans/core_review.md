# Core Module Architecture Review

**Status**: NEW REVIEW

**Date**: 2026-05-25

---

## Summary

Reviewed `architecture/core.md` against source code in `src/core/` and `src/protocol/core.rs`. The architecture document is **generally accurate** but has several undocumented types and some misleading claims about event publishing.

---

## Verification Results

### 1. CoreClient Trait (✓ Accurate)

The documented trait signature matches exactly:
```rust
// architecture/core.md:19-29
// src/core/mod.rs:13-20
#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(&self, request: RequestEnvelope<CoreRequest>) -> Result<CoreResponse, AppError>;
    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}
```

### 2. Core Client Types (✓ Accurate)

| Type | Architecture Doc | Implementation |
|------|------------------|----------------|
| `InprocCoreClient` | Runs core in current process, publishes CoreEvent | `src/core/mod.rs:22-44` ✓ |
| `StdioCoreClient` | Spawns `codegg core-stdio`, JSONL over stdin/stdout | `src/core/transport/stdio.rs` ✓ |
| `SocketCoreClient` | Connects to Unix socket, JSONL request/response | `src/core/transport/socket.rs` ✓ |

### 3. Protocol Envelopes (✓ Accurate)

| Type | Architecture Doc | Implementation |
|------|------------------|----------------|
| `RequestEnvelope<T>` | protocol_version, request_id | `src/protocol/core.rs:5-10` ✓ |
| `EventEnvelope<T>` | sequence, timestamp, session/turn metadata | `src/protocol/core.rs:12-20` ✓ |
| `CoreRequest` | sessions, turns, memory, tasks, worktrees, permissions | `src/protocol/core.rs:48-175` ✓ |
| `CoreResponse` | acknowledgements, JSON payloads, sessions, errors | `src/protocol/core.rs:22-46` ✓ |
| `CoreEvent` | Core-side event stream | `src/protocol/core.rs:177-272` ✓ |

### 4. Request Families (△ Partially Documented)

**Documented** (architecture/core.md:56-60):
- Session lifecycle: list, create, load, attach, fork, delete, archive, restore, share, unshare, rename, export, import, create-from-template ✓
- Turn lifecycle: submit, cancel, steer, agent select, model select ✓
- Session data: message loading and message counts ✓
- Operational helpers: model refresh, permission/question response, memory CRUD, task CRUD/scheduling, worktree listing ✓

**NOT Documented** (but present in `CoreRequest` enum at `src/protocol/core.rs`):
- `Initialize` (line 51)
- `Subscribe { session_id }` (lines 52-54)
- `Resume { session_id, from_event_seq }` (lines 55-58)
- `TurnCancel { session_id, turn_id }` (lines 124-127)
- `TurnSteer { session_id, turn_id, text }` (lines 128-132)
- `AgentSelect { session_id, agent_name }` (lines 133-136)
- `ModelSelect { session_id, model }` (lines 137-140)

### 5. Transport Modes (✓ Mostly Accurate)

**In-Process**: Correct - default mode, routes through `InprocCoreClient`.

**Stdio**: Correct - hidden `core-stdio` command, JSONL over stdin/stdout.

**Socket**: Correct - JSONL framing with reconnect-and-retry-once strategy at `src/core/transport/socket.rs:49-123`.

### 6. Startup Selection (△ External)

The environment variables `--core-transport` / `CODEGG_CORE_TRANSPORT` and `--core-endpoint` / `CODEGG_CORE_ENDPOINT` are documented but the actual selection logic is in the TUI, not the core module. This is not a discrepancy - just noting the implementation location.

### 7. Implementation Notes (△ Partially Misleading)

**Note 1**: "The in-process client publishes `CoreEvent` into the global event bus"

This is **slightly misleading**. Looking at `src/core/mod.rs:158-174`, events are published from **within `tokio::spawn`**:
```rust
tokio::spawn(async move {
    if let Err(e) = agent_loop.run(request).await {
        // Error event published here
    } else {
        // AgentFinished event published here
    }
});
```

The `InprocCoreClient::subscribe()` method (lines 702-725) does NOT publish events - it **subscribes** to the GlobalEventBus and forwards events to the channel receiver. The actual publishing of `AgentFinished` and `Error` events happens inside the spawned task.

**Note 2**: "Local TUI flows should prefer `CoreClient` over direct store access when a request already exists in `CoreRequest`"

This is sound guidance and matches the implementation pattern.

### 8. Protocol Version (✓ Accurate)

`PROTOCOL_VERSION = 1` at `src/protocol/core.rs:3`.

---

## Discrepancies Found

### Discrepancy 1: Missing Request Variants in Documentation

**Severity**: LOW

Several `CoreRequest` variants are not mentioned in the "Request Families" section:
- `Initialize`, `Subscribe`, `Resume` - client lifecycle
- `TurnCancel`, `TurnSteer` - turn control
- `AgentSelect`, `ModelSelect` - selection helpers

**Recommendation**: Update architecture/core.md lines 56-60 to include these variants or add a note that the list is not exhaustive.

### Discrepancy 2: Misleading Event Publishing Description

**Severity**: LOW

The architecture doc implies `InprocCoreClient` publishes events to the global event bus. In reality:
- `subscribe()` **reads** from GlobalEventBus
- Event publishing happens inside `tokio::spawn` in the `TurnSubmit` handler

**Recommendation**: Clarify that the in-process client subscribes to the GlobalEventBus and forwards events to callers, while turn execution (spawned async) publishes `AgentFinished`/`Error` events.

---

## Bugs Identified

**No bugs found** in the core module implementation. The code correctly implements:
- All documented request/response handling
- Transport adapters with proper error handling
- Event subscription and forwarding
- Protocol versioning

---

## Additional Findings

### Finding 1: Unused Match Arm in InprocCoreClient

At `src/core/mod.rs:698`:
```rust
_ => Ok(CoreResponse::Ack),
```

Unhandled `CoreRequest` variants silently return `Ack`. While not a bug (may be intentional), it could mask missing implementations. Consider logging unknown variants or returning a specific error code.

### Finding 2: Subscribe Channel Never Closes

In `InprocCoreClient::subscribe()` at `src/core/mod.rs:702-725`, the spawned task runs indefinitely with no cancellation mechanism. If the receiver is dropped, the task continues running until the bus closes. This is likely fine but worth noting.

### Finding 3: Stdio/Socket Clients Return Empty Receiver

As documented (line 31): "The stdio and socket clients currently expose request/response transport and return an empty receiver."

This is implemented correctly at:
- `src/core/transport/stdio.rs:88-91`
- `src/core/transport/socket.rs:126-129`

---

## Recommendations

### For Documentation

1. **Update Request Families section** to explicitly list or reference all `CoreRequest` variants, not just a summary.
2. **Clarify event flow** - explain that `subscribe()` reads from GlobalEventBus and turn execution publishes events.
3. **Add reference** to `src/protocol/core.rs` for complete type definitions.

### For Code

1. **Consider logging unknown variants** in the catch-all match arm at line 698 instead of silently returning `Ack`.
2. **Document `new_request` helper function** at `src/core/mod.rs:799-805` if intended for public use.

---

## Conclusion

The architecture document is **largely accurate** with minor omissions and one misleading claim about event publishing. The implementation quality is good with no bugs identified. The document should be updated to reflect the complete list of request variants and clarify the event publishing model.
