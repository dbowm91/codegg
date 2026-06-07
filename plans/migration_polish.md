# Codegg Core Daemon Tightening Plan

## Purpose

This plan tightens the current daemon implementation after the first migration pass. The repo now has the right high-level pieces: `CoreDaemon`, `EventLog`, socket transport, session runtime registry, client registry, daemon CLI, scoped permission/question registries, snapshot responses, and daemon-owned notification/audio policy.

The next work should not broaden the architecture. It should make the existing daemon path reliable for multi-session, multi-client workflows.

Primary goals:

1. Correct event metadata and replay semantics.
2. Make turn identity coherent across lifecycle events.
3. Make socket subscriptions real and filtered.
4. Fix remote-core model snapshot behavior.
5. Fix persisted event type names and recovery.
6. Improve notification coalescing for multi-session workflows.
7. Add integration tests that prove two attached clients do not cross-talk.

## Current State Summary

The implementation is meaningfully advanced. `InprocCoreClient` delegates to `CoreDaemon`; `CoreDaemon` owns event log, sessions, clients, notification router, and optional audio arbiter; `SocketCoreClient` uses a background reader, pending response map, and event bus; `daemon_socket.rs` accepts Unix socket clients and speaks `CoreFrame`; `SessionRuntimeRegistry` tracks active turn, selected model/agent, pending permission/question IDs, attached clients, token counts, and subagent count; and TUI socket mode avoids initializing local DB/provider/scheduler/memory resources.

The remaining issues are not structural absence; they are semantic and correctness issues.

## Highest Priority Bugs

### Bug 1: Event envelopes lose session/turn metadata in the daemon bridge

`CoreDaemon::start_event_bridge()` maps `AppEvent` to `CoreEvent`, then publishes using `daemon.event_log.publish(None, None, core_event).await`. This makes the event envelope `session_id` and `turn_id` `None` even when the payload contains a session ID. `EventLog::replay_from()` filters by envelope metadata, not payload metadata, so session-filtered replay misses these events.

### Bug 2: Turn identity is not coherent

`TurnSubmit` creates a real runtime `turn_id`, but bridge-mapped deltas/completion events frequently use `String::new()` or `None` for turn ID. There is also no clear `TurnStarted` event emitted immediately when a turn begins.

### Bug 3: Live socket events are not filtered by subscription

`daemon_socket.rs` subscribes every client to `daemon.event_log.subscribe()` immediately after connection and forwards all live events. `CoreFrame::Subscribe` only replays events and marks attachment; it does not affect live forwarding.

### Bug 4: Client identity is inconsistent

The socket server generates an internal `client_id`, but `CoreFrame::Subscribe` accepts a client-provided `client_id`. Normal `SocketCoreClient::subscribe()` does not send a subscribe frame, so attached sessions may not be recorded for normal TUI clients.

### Bug 5: `SnapshotModels` returns provider IDs rather than model IDs

`CoreRequest::SnapshotModels` currently maps `registry.list().iter().map(|p| p.id())`, which appears to return provider IDs. The remote TUI likely needs concrete model IDs matching the existing `provider/model` style.

### Bug 6: Event persistence stores opaque discriminants

`EventLog::publish()` stores `format!("{:?}", std::mem::discriminant(&envelope.payload))`. Recovery queries look for strings like `%TurnStarted%`, which likely will not match discriminant debug output. Persist explicit event type names instead.

### Bug 7: Replay semantics are ambiguous and likely duplicate last event

`EventLog::replay_from()` currently uses `event_seq >= from_event_seq`. If `from_event_seq` means “last seen sequence,” the correct condition is `event_seq > from_event_seq`.

Recommended convention: `from_event_seq` means “last event sequence already seen by the client”; replay returns events with `event_seq > from_event_seq`.

### Bug 8: Notification coalescing is per-session, not cross-session

Current coalescing only merges events with the same kind and same session ID. The original use case needs burst coalescing across sessions: “three sessions need attention.”

## Pass A: Event Metadata and Replay Correctness

### Goal

Ensure every core event has correct envelope metadata and replay behavior is predictable.

### Files

Likely affected:

- `src/core/mod.rs`
- `src/core/daemon.rs`
- `src/core/event_log.rs`
- `src/protocol/core.rs`

### Tasks

1. Add a metadata helper near core event conversion code:

```rust
pub(crate) fn core_event_metadata(
    event: &crate::protocol::core::CoreEvent,
) -> (Option<String>, Option<String>) {
    use crate::protocol::core::CoreEvent;

    match event {
        CoreEvent::TurnStarted { session_id, turn_id } => {
            (Some(session_id.clone()), Some(turn_id.clone()))
        }
        CoreEvent::TurnTextDelta { session_id, turn_id, .. } => {
            (Some(session_id.clone()), Some(turn_id.clone()))
        }
        CoreEvent::TurnReasoningDelta { session_id, turn_id, .. } => {
            (Some(session_id.clone()), Some(turn_id.clone()))
        }
        CoreEvent::ToolStarted { session_id, turn_id, .. } => {
            (Some(session_id.clone()), turn_id.clone())
        }
        CoreEvent::ToolCompleted { session_id, turn_id, .. } => {
            (Some(session_id.clone()), turn_id.clone())
        }
        CoreEvent::PermissionPending { session_id, turn_id, .. } => {
            (Some(session_id.clone()), turn_id.clone())
        }
        CoreEvent::QuestionPending { session_id, turn_id, .. } => {
            (Some(session_id.clone()), turn_id.clone())
        }
        CoreEvent::TurnCompleted { session_id, turn_id, .. } => {
            (Some(session_id.clone()), Some(turn_id.clone()))
        }
        CoreEvent::TurnFailed { session_id, turn_id, .. } => {
            (Some(session_id.clone()), turn_id.clone())
        }
        CoreEvent::SessionUpdated { session_id } => {
            (Some(session_id.clone()), None)
        }
        CoreEvent::SubagentStarted { session_id, .. }
        | CoreEvent::SubagentProgress { session_id, .. }
        | CoreEvent::SubagentCompleted { session_id, .. }
        | CoreEvent::SubagentFailed { session_id, .. } => {
            (Some(session_id.clone()), None)
        }
        _ => (None, None),
    }
}
```

2. Update `CoreDaemon::start_event_bridge()`:

```rust
if let Some(core_event) = super::map_app_event_to_core_event(app_event.clone()) {
    let (session_id, turn_id) = super::core_event_metadata(&core_event);
    daemon.event_log.publish(session_id, turn_id, core_event).await;
}
```

3. Update any other direct `event_log.publish(None, None, event)` call where event metadata can be inferred.

4. Adopt replay-after-last-seen semantics:
   - `from_event_seq` means the last sequence already processed by the client.
   - Replay returns `event_seq > from_event_seq`.
   - Snapshot flow subscribes/resumes from `snapshot.event_seq`.

5. Change ring and DB replay filters from `>=` to `>`.

6. Rename comments/docs to avoid ambiguity:
   - “Replay events after `from_event_seq`.”
   - “Pass `0` to replay from the beginning.”

### Acceptance Criteria

- Session-filtered replay returns session events bridged from `GlobalEventBus`.
- Global events still replay when `include_global = true`.
- `Resume { from_event_seq: 0 }` returns the first event.
- `Resume { from_event_seq: last_seen }` does not duplicate `last_seen`.
- Tests cover both ring-buffer replay and DB-backed replay if DB-backed replay exists.

### Suggested Tests

Add to `event_log.rs`:

```rust
#[tokio::test]
async fn replay_filters_by_envelope_session_id() { ... }

#[tokio::test]
async fn replay_after_last_seen_does_not_duplicate() { ... }

#[tokio::test]
async fn replay_from_zero_returns_first_event() { ... }
```

Add to `daemon.rs`:

```rust
#[tokio::test]
async fn event_bridge_preserves_session_metadata() { ... }
```

## Pass B: Coherent Turn Identity

### Goal

All events belonging to a turn must carry the same `turn_id`.

### Files

Likely affected:

- `src/core/daemon.rs`
- `src/core/mod.rs`
- `src/bus/events.rs`
- `src/agent/loop.rs`
- `src/protocol/core.rs`

### Tasks

1. In `TurnSubmit`, after creating the runtime `turn_id`, publish a `CoreEvent::TurnStarted` directly to the daemon `EventLog` before spawning the agent loop.

2. Ensure `AgentLoop` knows the active turn ID.

Preferred API:

```rust
agent_loop.set_turn_id(turn_id.clone());
```

Then `AgentLoop` includes this turn ID in emitted `AppEvent`s.

Fallback: in `start_event_bridge()`, if a mapped event lacks a turn ID, look up `daemon.sessions.get(session_id).active_turn` and attach its `turn_id`.

3. Extend `AppEvent` variants to include `turn_id` where missing, if manageable:
   - `TextDelta`
   - `ReasoningDelta`
   - `ToolCallStarted`
   - `ToolResult`
   - `AgentFinished`
   - session-scoped errors
   - possibly subagent lifecycle if tied to a parent turn

4. Update `map_app_event_to_core_event()` so it never emits `turn_id: String::new()` for turn-scoped events.

5. Ensure `TurnCompleted` and `TurnFailed` use the same runtime turn ID.

6. For error result from `agent_loop.run()`, publish `TurnFailed` directly with the active turn ID, not only generic `AppEvent::Error`.

7. Change `TurnCancel { turn_id }` to verify that the requested turn ID matches the active turn ID instead of ignoring it.

### Acceptance Criteria

- Every turn emits exactly one `TurnStarted`.
- `TurnStarted.turn_id == TurnCompleted.turn_id` for successful turns.
- `TurnStarted.turn_id == TurnFailed.turn_id` for failed turns.
- Deltas/tool events during the turn carry the same turn ID or a documented optional value.
- `TurnCancel { turn_id }` rejects wrong turn IDs.

## Pass C: Real Socket Subscription Filtering

### Goal

Clients receive only events matching their subscriptions, unless they explicitly subscribe globally.

### Files

Likely affected:

- `src/core/transport/daemon_socket.rs`
- `src/core/transport/socket.rs`
- `src/core/client_registry.rs`
- `src/protocol/frames.rs`

### Target Behavior

Each socket connection has subscription state:

```rust
struct ClientConnectionState {
    client_id: String,
    subscriptions: Arc<RwLock<Vec<EventFilter>>>,
}
```

Live event forwarding checks whether an event matches at least one filter.

### Tasks

1. Add negotiated client ID to `ServerHello`:

```rust
pub struct ServerHello {
    pub daemon_id: String,
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
    pub client_id: String,
}
```

2. In `SocketCoreClient`, store `client_id` after receiving `ServerHello`.

3. Make subscription explicit. Preferred: extend `CoreClient` with:

```rust
async fn subscribe_session(
    &self,
    session_id: Option<String>,
    from_event_seq: Option<u64>,
) -> Result<mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>, AppError>;
```

If this causes too much churn, make `SocketCoreClient::connect()` send a global subscribe frame by default, then add session-specific subscription later.

4. In `daemon_socket.rs`, keep per-connection filters:

```rust
let filters = Arc::new(RwLock::new(Vec::<EventFilter>::new()));
```

5. Modify `forward_events()` to accept filters and only forward matching events.

6. Add event match helper:

```rust
fn event_matches_filter(event: &EventEnvelope<CoreEvent>, filter: &EventFilter) -> bool {
    if let Some(ref sid) = filter.session_id {
        event.session_id.as_deref() == Some(sid.as_str())
    } else {
        filter.include_global || event.session_id.is_none()
    }
}
```

7. On `CoreFrame::Subscribe`, update that connection’s filters and replay matching events.

8. On disconnect, unregister the negotiated client ID.

9. Use the client name from `ClientHello`; do not register Unix socket clients as `"websocket"`.

### Acceptance Criteria

- Client subscribed to session A does not receive live session B events.
- Client subscribed globally receives intended global/all events.
- Client registry shows actual client name and attached session.
- `Subscribe` replay and live forwarding use the same matching logic.
- `SocketCoreClient::subscribe()` sends a real subscribe frame or a documented global default.

## Pass D: TUI Remote-Core Initial Session Loading

### Goal

Make `AttachDaemon --session` and remote-core session startup actually load through core.

### Tasks

1. After `app.set_core_client(core_client)` and `app.init_remote_core().await`, handle session startup through `CoreClient`.

2. Add helper:

```rust
async fn load_initial_session_via_core(app: &mut App, cli: &Cli, project_dir: &str)
```

3. Cases:
   - `--session <id>`: call `SessionAttach` or `SnapshotSession`, set app session, load messages.
   - `--continue`: call `SessionList { project_id, show_archived: false, limit: 1 }`, attach first if present.
   - `--new` or no session: create/open new session according to current UX.
   - `--fork <id>`: call `SessionFork`, then load returned/forked session.

4. Ensure socket mode never initializes local stores just to load initial sessions.

### Acceptance Criteria

- `codegg attach-daemon --session <id>` opens that session in the TUI.
- `codegg attach-daemon --new` creates or opens a usable session according to intended UX.
- Remote-core mode can load messages without local stores.
- No local DB is initialized in socket mode.

## Pass E: Snapshot Model Correctness

### Goal

Remote-core TUI model list should match local/inproc model list semantics.

### Tasks

1. Replace current `SnapshotModels` provider-ID list with actual model IDs.

2. Extract/reuse local model discovery logic:

```rust
async fn load_model_ids(
    config: &Config,
    pool: Option<SqlitePool>,
) -> Vec<String>
```

3. Prefer the existing `ModelDiscoveryService` path used in local startup.

4. If no pool is available, fall back to configured provider model lists.

5. Return `current_model` from session runtime or config if available.

### Acceptance Criteria

- `SnapshotModels` returns entries like `provider/model`, not only provider names.
- Remote TUI model selector has usable model IDs.
- `App::init_remote_core()` populates the same shape as local mode.

## Pass F: Event Persistence and Recovery Fixes

### Goal

Make persisted event recovery actually work.

### Tasks

1. Add explicit event type naming:

```rust
pub fn core_event_type(event: &CoreEvent) -> &'static str {
    match event {
        CoreEvent::TurnStarted { .. } => "turn_started",
        CoreEvent::TurnCompleted { .. } => "turn_completed",
        CoreEvent::TurnFailed { .. } => "turn_failed",
        CoreEvent::ToolStarted { .. } => "tool_started",
        CoreEvent::ToolCompleted { .. } => "tool_completed",
        CoreEvent::PermissionPending { .. } => "permission_pending",
        CoreEvent::QuestionPending { .. } => "question_pending",
        CoreEvent::SessionUpdated { .. } => "session_updated",
        CoreEvent::SubagentStarted { .. } => "subagent_started",
        CoreEvent::SubagentCompleted { .. } => "subagent_completed",
        CoreEvent::SubagentFailed { .. } => "subagent_failed",
        CoreEvent::Error { .. } => "error",
        _ => "other",
    }
}
```

2. Store that string in `core_event_log.event_type`.

3. Update recovery queries to use exact event type strings.

4. Add `turn_id IS NOT NULL` when looking for interrupted turns.

5. Deduplicate recovery output so one interrupted turn produces one recovery event.

### Acceptance Criteria

- Persisted `core_event_log.event_type` is readable and stable.
- `recover_state()` detects an intentionally inserted `turn_started` without completion.
- `recover_state()` does not detect a completed turn as interrupted.
- Tests use a real temp SQLite pool.

## Pass G: Permission/Question Routing Hardening

### Goal

Make scoped permission/question handling usable across the daemon path, not only available in registry APIs.

### Tasks

1. Audit call sites of:
   - `PermissionRegistry::register(...)`
   - `QuestionRegistry::register(...)`
   - `PermissionRegistry::respond(...)`
   - `QuestionRegistry::answer_question(...)`

2. Convert session-aware call sites to scoped methods.

3. Preserve backward-compatible calls only for genuinely sessionless/legacy paths.

4. In `CoreDaemon::PermissionRespond`, parse full protocol ID safely. Avoid `unwrap_or_default()`. Return `invalid_permission_id` for malformed IDs.

5. Same for `QuestionRespond`.

6. Decide whether registry keys should be full protocol IDs or simple IDs. If simple IDs remain, document that they must be globally unique. Prefer full protocol IDs if feasible.

### Acceptance Criteria

- Invalid permission/question IDs return `invalid_*_id`.
- Two sessions can have simultaneous permissions/questions without cross-response.
- `get_pending_for_session()` returns only relevant entries.

## Pass H: Notification Coalescing for Multi-Session Workflows

### Goal

Make notification/audio behavior fit the “8 sessions active” use case.

### Target Behavior

Within `coalesce_window_ms`, collect speakable notifications and synthesize a concise aggregate message.

Examples:
- One event: “parser-refactor completed.”
- Three completion events: “Three sessions completed: parser-refactor, tui-polish, search-agent.”
- Mixed events: “Three sessions need attention. search-agent failed. tui-polish awaits input. parser-refactor completed.”

### Tasks

1. Add session display-name resolution. Initially use session ID prefix; later use session title.

2. Add batch collection to `NotificationRouter` or `AudioArbiter`:

```rust
pub async fn next_speech_batch(&self, max_items: usize) -> Option<Vec<NotificationEvent>>
```

3. Add renderer:

```rust
fn render_speech_batch(events: &[NotificationEvent]) -> String
```

4. Preserve priority:
   - Urgent/high events first.
   - Low-priority completion events can be batched or dropped if queue is large.
   - Permission/question/failed events dominate completion notices.

5. Add max queue length and drop policy.

### Acceptance Criteria

- Burst of five completions produces one speech call, not five.
- Urgent permission event interrupts or supersedes low-priority completion batch.
- Repeated dedupe keys are suppressed.
- Tests cover batching and priority.

## Pass I: Integration Test Matrix

### Goal

Prevent regressions in daemon correctness.

### Minimum Tests

1. Event metadata:
   - Publish bridged session event.
   - Replay by session.
   - Assert event returned.

2. Replay semantics:
   - Publish event 1 and 2.
   - Resume from 0 returns 1 and 2.
   - Resume from 1 returns only 2.
   - Resume from 2 returns empty.

3. Socket filtering:
   - Client A subscribes session A.
   - Client B subscribes session B.
   - Publish session A event.
   - Assert only A receives it.
   - Publish session B event.
   - Assert only B receives it.

4. Client identity:
   - Connect client with `ClientHello`.
   - Assert registry shows negotiated client ID and client name.
   - Subscribe to session.
   - Assert registry attached sessions includes that session.

5. Turn lifecycle:
   - Assert `TurnStarted` contains real turn ID.
   - Cancel with wrong turn ID fails.
   - Cancel with correct turn ID succeeds.

6. Recovery:
   - Insert `turn_started` row with no completion.
   - Run `recover_state()`.
   - Assert `TurnFailed` emitted or persisted.

7. Remote model snapshot:
   - Assert models list contains model-like IDs, not only provider names.

8. Notification batching:
   - Emit multiple speakable events.
   - Assert one rendered batch message.

## Pass J: Status File Correction

### Goal

Make `plans/daemon_migration_status.md` reflect actual state.

### Suggested Wording

```markdown
## Current Status

The daemon migration skeleton is implemented and usable for early testing.

Completed / mostly complete:
- CoreDaemon extraction
- EventLog ring buffer and basic persistence
- Socket frame protocol
- Daemon CLI
- SessionRuntimeRegistry skeleton
- Scoped permission/question registry APIs
- Basic snapshot/resume responses
- RemoteCore mode avoids local heavy initialization
- NotificationRouter and AudioArbiter first pass

Needs hardening:
- Event envelope metadata
- Coherent turn IDs
- Socket live-event filtering
- Client identity negotiation
- SnapshotModels correctness
- Recovery event type persistence
- Remote-core initial session loading
- Cross-session notification batching
- Integration tests for multi-client/session isolation
```

## Recommended Implementation Order

1. Pass A: Event metadata and replay semantics.
2. Pass B: Coherent turn identity.
3. Pass C: Socket subscription filtering and client identity.
4. Pass D: TUI remote-core initial session loading.
5. Pass E: SnapshotModels correctness.
6. Pass F: Event persistence/recovery.
7. Pass G: Permission/question routing hardening.
8. Pass H: Notification batching.
9. Pass I: Integration test matrix.
10. Pass J: Status file correction.

The first three passes are the foundation. Do not spend time polishing notification batching before event routing and turn identity are correct.

## Definition of Done

The tightened daemon path is acceptable when:

1. One daemon can serve two TUI clients simultaneously.
2. Each client can subscribe to a different session and only receive relevant live events.
3. Reconnect/resume does not duplicate the last seen event.
4. Session-filtered replay works for bridged events.
5. Turn IDs are consistent from start through completion/failure.
6. Permission/question responses cannot cross sessions.
7. Remote-core TUI can load initial sessions without local stores.
8. Model selector in remote-core mode shows usable model IDs.
9. Daemon recovery detects interrupted turns using persisted event types.
10. Audio notifications are centralized and do not overlap.

## Notes for Smaller Implementation Agents

Keep each pass narrow. Avoid renaming broad modules or changing UX while fixing transport semantics.

Do not introduce TCP networking, cloud sync, GUI work, mobile work, or advanced TTS in this tightening pass.

Do not remove compatibility paths until the socket daemon path has integration tests.

When uncertain, prefer adding tests around current behavior before changing it. The highest-risk areas are event ordering, session filtering, and ID parsing.

