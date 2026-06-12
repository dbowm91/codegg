# Codegg Core Daemon Final Hardening Plan

## Purpose

This plan follows the latest review of the daemon/multi-frontend work. The current codebase has implemented most of the daemon tightening plan: `CoreDaemon`, metadata helpers, replay-after-last-seen, explicit persisted event type names, socket client identity, per-connection socket filters, remote-core initial session loading, model snapshots, and notification batching.

The remaining work is focused and correctness-oriented. Do not broaden the architecture. Do not add GUI/mobile/cloud/TCP work. Do not add advanced TTS. This pass should harden the existing daemon path enough to trust it as the foundation for multi-session TUI and centralized audio.

## Current Assessment

The daemon path is now a plausible early implementation. The most important remaining issues are:

1. Socket clients currently send a default global subscription that effectively receives all session events.
2. `Resume` can return `ResyncRequired` when the client is already current, because it checks whether newer events exist rather than whether the requested sequence is still covered.
3. Turn completion/failure identity still depends on bridge-time lookup of `runtime.active_turn`, which is race-prone.
4. End-to-end socket isolation tests are still needed.
5. `plans/daemon_migration_status.md` is partially contradictory: it lists several items as both “needs hardening” and “completed.”

## Non-Goals

Do not rewrite the daemon.

Do not change the public TUI UX except where necessary to fix socket subscription semantics.

Do not add TCP remote networking.

Do not remove compatibility APIs.

Do not refactor large unrelated modules.

Do not change provider/model architecture beyond fixing remote-core correctness if a test exposes a defect.

## Pass 1: Fix Socket Subscription Semantics

### Problem

`SocketCoreClient` sends this after `ServerHello`:

```rust
CoreFrame::Subscribe {
    client_id: hello.client_id.clone(),
    session_id: None,
    from_event_seq: Some(0),
}
```

On the server, `session_id: None` becomes an `EventFilter` with `include_global: true`. The current match logic treats this as matching every event, so a normal socket TUI receives all live session events by default.

### Goal

A normal socket TUI should not receive every session’s events unless it explicitly subscribes to all events. A frontend should receive global/sessionless events when it subscribes globally, session A events when it subscribes to session A, and session B events only if it subscribes to session B.

### Recommended Design

Do not overload `session_id: None` to mean “all sessions.” For this pass:

- `session_id: Some(sid)` means session-specific subscription.
- `session_id: None` means global/sessionless-only subscription.
- Do not implement all-sessions subscription unless a current frontend requires it.
- If all-sessions is needed later, add a distinct frame or explicit field.

### Tasks

1. Change `event_matches_filter()` in `src/core/transport/daemon_socket.rs`.

Recommended semantics:

```rust
fn event_matches_filter(event: &EventEnvelope<CoreEvent>, filter: &EventFilter) -> bool {
    match (&filter.session_id, filter.include_global) {
        (Some(sid), true) => {
            event.session_id.as_deref() == Some(sid.as_str()) || event.session_id.is_none()
        }
        (Some(sid), false) => {
            event.session_id.as_deref() == Some(sid.as_str())
        }
        (None, _) => event.session_id.is_none(),
    }
}
```

2. Update comments around `EventFilter.include_global`.

Recommended interim semantics:

```rust
pub struct EventFilter {
    pub session_id: Option<String>,
    pub client_id: Option<String>,
    /// When `session_id` is Some, also include global/sessionless events.
    /// When `session_id` is None, this does not mean all sessions.
    pub include_global: bool,
}
```

3. Update `daemon_socket.rs` `Subscribe` handling.

Recommended:

```rust
let new_filter = if let Some(sid) = session_id.clone() {
    EventFilter {
        session_id: Some(sid),
        client_id: Some(client_id.clone()),
        include_global: true,
    }
} else {
    EventFilter {
        session_id: None,
        client_id: Some(client_id.clone()),
        include_global: false,
    }
};
```

4. Keep the current default `Subscribe { session_id: None }` from `SocketCoreClient` for now. After this fix, it means global-only rather than all-session. This is the lowest-churn safe behavior.

5. Add an inherent method on `SocketCoreClient` for explicit session subscription if not already present:

```rust
impl SocketCoreClient {
    pub async fn subscribe_session_events(
        &self,
        session_id: String,
        from_event_seq: Option<u64>,
    ) -> Result<(), AppError> {
        let client_id = self.client_id().await.ok_or_else(|| {
            AppError::Other(anyhow::anyhow!("socket client has not received ServerHello"))
        })?;
        let frame = CoreFrame::Subscribe {
            client_id,
            session_id: Some(session_id),
            from_event_seq,
        };
        self.send_frame(frame).await
    }
}
```

If adding a helper `send_frame()` to `SocketCoreClient` is needed, keep it private and small.

Do not extend the `CoreClient` trait unless necessary. An inherent method is sufficient for socket-specific tests.

### Acceptance Criteria

- A global-only socket subscription receives only sessionless events.
- A session-specific socket subscription receives only that session’s events plus sessionless events if `include_global` is true.
- A client subscribed to session A does not receive session B events.
- The default socket TUI connection no longer receives every session’s live events.

### Tests

Add or adjust unit tests in `daemon_socket.rs`:

```rust
#[test]
fn global_filter_rejects_session_event() { ... }

#[test]
fn global_filter_matches_global_event() { ... }

#[test]
fn session_filter_rejects_other_session() { ... }

#[test]
fn session_filter_can_include_global_event_if_configured() { ... }
```

## Pass 2: Fix Resume Coverage Semantics

### Problem

`CoreRequest::Resume` currently uses:

```rust
if !self.event_log.has_events_from(from_event_seq).await {
    return Ok(CoreResponse::ResyncRequired { ... });
}
```

But `has_events_from()` returns true only if there are events with `event_seq > from_event_seq`. If the client is already current, `from_event_seq == current_seq`, there are no newer events. That should return `CoreResponse::Events { events: vec![], current_seq }`, not `ResyncRequired`.

### Goal

`ResyncRequired` should mean “the requested sequence is too old to replay from available event storage,” not “there are no new events.”

### Recommended Semantics

- `from_event_seq == current_seq`: success with empty events.
- `from_event_seq > current_seq`: success with empty events and current seq, or a structured future-sequence error. Prefer success with empty events unless this causes client bugs.
- `from_event_seq < earliest_available_seq - 1`: `ResyncRequired`.
- Otherwise replay events with `event_seq > from_event_seq`.

### Tasks

1. Replace `has_events_from()` usage in `CoreDaemon::handle_request(CoreRequest::Resume)`.

Recommended:

```rust
let current_seq = self.event_log.current_seq();

if from_event_seq >= current_seq {
    return Ok(CoreResponse::Events {
        events: Vec::new(),
        current_seq,
    });
}

if !self.event_log.covers_from(from_event_seq).await {
    return Ok(CoreResponse::ResyncRequired {
        from_event_seq,
        current_seq,
        session_id,
    });
}

let events = self.event_log.replay_from(from_event_seq, &filter).await;
Ok(CoreResponse::Events { events, current_seq })
```

2. Add `EventLog::covers_from(from_event_seq)`.

Suggested behavior:

```rust
pub async fn covers_from(&self, from_event_seq: u64) -> bool {
    let current = self.current_seq();

    if from_event_seq >= current {
        return true;
    }

    {
        let ring = self.ring.lock().await;
        if let Some(front) = ring.front() {
            // To replay events after from_event_seq, the earliest needed event is from_event_seq + 1.
            if front.event_seq <= from_event_seq.saturating_add(1) {
                return true;
            }
        }
    }

    if self.pool.is_some() {
        return self.db_covers_from(from_event_seq).await;
    }

    false
}
```

3. Add `db_covers_from(from_event_seq)`.

Simplest DB check:

```sql
SELECT MIN(event_seq), MAX(event_seq) FROM core_event_log
```

Return true if:

- `min_seq <= from_event_seq + 1`
- `max_seq >= from_event_seq + 1`

The `from_event_seq >= current_seq` case should be handled before DB checks.

4. Keep `has_events_from()` if other call sites use it, but do not use it for resync decisions.

5. Update docs/comments.

### Acceptance Criteria

- Resume from current seq returns `Events { events: [], current_seq }`.
- Resume from future seq returns empty events or a clear structured error; pick one and test it.
- Resume from `0` returns all available events if still covered.
- Resume from too-old sequence returns `ResyncRequired`.
- Session filter still applies to replayed events.

### Tests

Add to `event_log.rs`:

```rust
#[tokio::test]
async fn covers_from_current_seq_is_true() { ... }

#[tokio::test]
async fn covers_from_too_old_ring_seq_is_false_without_db() { ... }

#[tokio::test]
async fn replay_from_current_seq_returns_empty() { ... }
```

Add to `daemon.rs`:

```rust
#[tokio::test]
async fn resume_from_current_seq_returns_empty_events_not_resync() { ... }

#[tokio::test]
async fn resume_from_too_old_seq_returns_resync() { ... }
```

## Pass 3: Emit Turn Completion/Failure With Explicit Turn IDs

### Problem

`TurnStarted` is now emitted directly with a real turn ID. However, `TurnCompleted` still comes from `AppEvent::AgentFinished`, which maps to `CoreEvent::TurnCompleted { turn_id: String::new() }`. The bridge tries to backfill the active turn ID from `runtime.active_turn`.

That is race-prone because the spawned task publishes `AppEvent::AgentFinished` and then clears `runtime.active_turn`. The event bridge runs in another task and may process the finish event after the runtime has already been cleared.

### Goal

Turn lifecycle events should not depend on bridge-time lookup. The turn task already knows `turn_id`; it should emit completion/failure with that exact ID before clearing runtime state.

### Recommended Fix

Publish `CoreEvent::TurnCompleted` and `CoreEvent::TurnFailed` directly from the `TurnSubmit` spawned task, using a cloned `Arc<EventLog>` and the captured `turn_id`.

### Tasks

1. In `CoreDaemon::handle_request(CoreRequest::TurnSubmit)`, before spawning:

```rust
let event_log_for_spawn = Arc::clone(&self.event_log);
let turn_id_for_spawn = turn_id.clone();
```

2. In the spawned task, publish directly on success:

```rust
event_log_for_spawn
    .publish(
        Some(session_id_for_spawn.clone()),
        Some(turn_id_for_spawn.clone()),
        CoreEvent::TurnCompleted {
            session_id: session_id_for_spawn.clone(),
            turn_id: turn_id_for_spawn.clone(),
            stop_reason: "completed".to_string(),
        },
    )
    .await;
```

3. Publish directly on failure:

```rust
event_log_for_spawn
    .publish(
        Some(session_id_for_spawn.clone()),
        Some(turn_id_for_spawn.clone()),
        CoreEvent::TurnFailed {
            session_id: session_id_for_spawn.clone(),
            turn_id: Some(turn_id_for_spawn.clone()),
            message: format!("Agent error: {}", e),
        },
    )
    .await;
```

4. Avoid duplicate completion events.

Recommended transitional approach:

- Continue publishing `AppEvent::AgentFinished` only if needed for legacy token/status consumers.
- Remove or gate the `AppEvent::AgentFinished -> CoreEvent::TurnCompleted` mapping so the daemon does not emit duplicate `TurnCompleted` events.
- If in doubt, update `map_app_event_to_core_event()` so `AgentFinished` returns `None`, and verify inproc/remote event behavior through tests.

5. Preserve runtime token count updates. If token counts are only available via `AppEvent::AgentFinished`, keep that bus event but do not map it into another core lifecycle event.

6. Clear `runtime.active_turn` only after direct completion/failure publication.

7. Add comments documenting the ordering invariant.

### Acceptance Criteria

- Successful turn emits exactly one `TurnStarted` and exactly one `TurnCompleted` with the same turn ID.
- Failed turn emits exactly one `TurnStarted` and exactly one `TurnFailed` with the same turn ID.
- No empty turn IDs in lifecycle events.
- Clearing `active_turn` happens after completion/failure publication.
- Bridge fallback remains for deltas/tool events but lifecycle completion no longer relies on it.

### Tests

If full agent-loop testing is heavy, use a test-only helper or synthetic path.

Minimum:

```rust
#[tokio::test]
async fn direct_turn_completion_uses_runtime_turn_id() { ... }
```

If not practical, add tests around an extracted helper that publishes turn completion/failure from captured IDs.

## Pass 4: Add End-to-End Socket Isolation Tests

### Goal

Prove the daemon socket path actually isolates events across clients/sessions.

### Files

Create:

```text
tests/daemon_socket_isolation.rs
```

or add under `src/core/transport/daemon_socket.rs` if integration setup is difficult.

### Test 1: Two clients, two sessions, no cross-talk

Pseudo-flow:

1. Create `CoreDaemon::new(None, None, None, None)` if DB is not needed.
2. Start `run_core_socket()` on a temp Unix socket.
3. Connect client A.
4. Connect client B.
5. Subscribe A to session `s1`.
6. Subscribe B to session `s2`.
7. Publish event for `s1`:
   ```rust
   daemon.event_log.publish(
       Some("s1".into()),
       Some("t1".into()),
       CoreEvent::TurnTextDelta { ... },
   ).await;
   ```
8. Assert A receives it within timeout.
9. Assert B does not receive it within a short timeout.
10. Publish event for `s2`.
11. Assert B receives it and A does not.

### Test 2: Global-only does not receive session events

1. Client subscribes with `session_id: None`.
2. Publish session event.
3. Assert client does not receive it.
4. Publish global event.
5. Assert client receives it.

### Test 3: Resume replay uses same filter as live forwarding

1. Publish `s1` and `s2` events before subscribe.
2. Client subscribes to `s1` with `from_event_seq: Some(0)`.
3. Assert replay includes `s1` only.

### Implementation Notes

`SocketCoreClient` may not expose a session-subscription method yet. If not, add the inherent method described in Pass 1.

Use `tokio::time::timeout()` to avoid hanging tests.

Be careful with default global subscription sent by `SocketCoreClient`; it may receive global events. Published session-scoped events should not match the global-only subscription after Pass 1.

### Acceptance Criteria

- End-to-end tests prove no session cross-talk.
- Tests fail under the previous default “global receives all sessions” behavior.
- Tests are deterministic enough for CI.

## Pass 5: Status File Cleanup

### Problem

`plans/daemon_migration_status.md` currently says the migration skeleton is usable and lists items under “Needs hardening,” but later lists polish passes A–J as completed. This is confusing.

### Goal

Separate implementation progress from validation status.

### Tasks

Update `plans/daemon_migration_status.md` to distinguish:

- Implemented and validated.
- Implemented but needs hardening.
- Remaining.

Suggested language:

```markdown
## Current Status

The daemon migration is implemented at an early-usable stage. Most tightening passes are implemented, but several require validation and one final hardening pass.

Implemented:
- CoreDaemon extraction
- EventLog ring buffer + SQLite persistence
- Replay-after-last-seen semantics
- Event metadata inference
- TurnStarted emission
- Socket client_id negotiation
- Per-connection socket filters
- SnapshotModels provider/model IDs
- Remote-core initial session loading
- Notification batching

Known remaining hardening:
- Socket default subscription semantics must distinguish global-only from all-sessions.
- Resume from current sequence should return empty Events, not ResyncRequired.
- Turn completion/failure should publish direct CoreEvents with captured turn_id.
- End-to-end two-client/two-session socket isolation tests are required.
```

Update the old “Completed Passes (polish)” section:

- Mark Pass A, E, F, H as implemented.
- Mark Pass B and C as implemented but needing final hardening/validation.
- Mark Pass I as partial unless actual end-to-end socket integration tests are present.

### Acceptance Criteria

- Status file no longer claims complete validation where tests are not present.
- A future agent can immediately see the remaining work.

## Recommended Implementation Order

1. Pass 1: Fix socket subscription semantics.
2. Pass 2: Fix resume coverage semantics.
3. Pass 3: Emit turn completion/failure with explicit turn IDs.
4. Pass 4: Add end-to-end socket isolation tests.
5. Pass 5: Status file cleanup.

Do not do Pass 5 first. The status file should reflect the actual implementation after the code is fixed.

## Definition of Done

This follow-up pass is complete when:

1. A socket client with only global subscription does not receive session-scoped events.
2. A socket client subscribed to session A does not receive session B events.
3. `Resume { from_event_seq: current_seq }` returns `CoreResponse::Events { events: [], current_seq }`.
4. Too-old resume requests still return `ResyncRequired`.
5. Turn lifecycle completion/failure events carry the same non-empty turn ID emitted in `TurnStarted`.
6. Completion/failure does not rely on bridge lookup after `runtime.active_turn` may have been cleared.
7. End-to-end socket tests cover two clients and two sessions.
8. `daemon_migration_status.md` accurately distinguishes implemented, validated, and remaining work.

## Notes for Implementation Agents

Keep the patch narrow.

Prefer explicit tests over broad refactors.

Do not extend the `CoreClient` trait unless necessary. An inherent method on `SocketCoreClient` is acceptable for socket-specific tests.

Do not implement an all-sessions subscription unless an existing frontend requires it. If needed later, add a distinct protocol field or frame rather than overloading `session_id: None`.

The highest-risk area is silent event leakage between sessions. Treat that as the main regression target.

