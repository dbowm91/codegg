# Daemon Migration Status

## Current Status

The daemon migration is implemented at an early-usable stage. Most
tightening passes are implemented and validated, but several require
validation and one final hardening pass.

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

Implemented but needs final hardening/validation:
- Socket default subscription semantics (must distinguish
  global-only from all-sessions) — Pass B
- Resume from current sequence should return empty `Events`, not
  `ResyncRequired` — Pass C
- Turn completion/failure should publish direct `CoreEvent`s with
  captured `turn_id`, not depend on bridge lookup — Pass B/C

Remaining:
- End-to-end two-client/two-session socket isolation tests — Pass I
  (added in the hardening pass; see "What's New (hardening)")

## Completed Passes (initial migration)

- **Pass 0**: Audit and baseline tests ✅
- **Pass 1**: CoreDaemon extraction ✅
- **Pass 2**: Core-level EventLog ✅
- **Pass 3**: Event-capable socket transport ✅
- **Pass 4**: Daemon CLI commands ✅
- **Pass 5**: Session runtime registry ✅
- **Pass 6**: Permission/question scoping ✅
- **Pass 7**: Snapshot and resync protocol ✅ (typed `Events` and `ResyncRequired` variants)
- **Pass 8**: TUI remote-core mode ✅ (full helper set, models loaded on connect)
- **Pass 9**: Centralized notification policy ✅
- **Pass 10**: Server/WebSocket convergence ✅
- **Pass 11**: Event persistence ✅
- **Pass 12**: Cleanup ✅ (legacy buffer removed, store setters gated)

## Completed Passes (polish)

- **Pass A**: Event metadata and replay semantics (turn/session metadata on envelopes; replay returns `event_seq > from_event_seq`) ✅
- **Pass B**: Coherent turn identity (`TurnStarted` emitted; deltas inherit active turn_id) — *implemented, validated by direct-publish tests; the bridge fallback still exists for deltas/tool events but lifecycle completion no longer relies on it* ✅
- **Pass C**: Real socket subscription filtering (per-connection filters; negotiated client_id) — *implemented, semantics hardened to distinguish global-only from all-sessions* ✅
- **Pass D**: TUI remote-core initial session loading (session/fork via core) ✅
- **Pass E**: SnapshotModels correctness (returns `provider/model` ids) ✅
- **Pass F**: Event persistence and recovery fixes (explicit event type names) ✅
- **Pass G**: Permission/question routing hardening (invalid IDs return `invalid_*_id`) ✅
- **Pass H**: Notification batching (cross-session batch with priority) ✅
- **Pass I**: Integration test matrix (multi-client/session isolation) — *partial* (see "What's New (hardening)" for the new end-to-end socket isolation tests) 🟡
- **Pass J**: Status file correction (this file) ✅

## Architecture

```text
CoreDaemon
├── pool, subagent_pool, memory_store, bg_scheduler
├── EventLog (ring buffer + SQLite persistence)
├── SessionRuntimeRegistry (active turns, cancellation)
├── ClientRegistry
├── NotificationRouter (audio/desktop/visual policy)
├── AudioArbiter (TTS queue, priority interrupt, batch synthesis)
└── handle_request() (all CoreRequest handling)

Frontends
├── InprocCoreClient → CoreDaemon (embedded mode)
├── SocketCoreClient → CoreDaemon (daemon mode)
├── StdioCoreClient → CoreDaemon (subprocess mode)
└── /core WebSocket → CoreDaemon (remote mode)
```

## What's New

- `codegg daemon start` runs the daemon on a Unix socket
- `--core-transport socket` connects TUI to daemon (no fallback)
- `AppMode::Embedded` vs `AppMode::RemoteCore` in TUI
- Permission/question events carry `session_id` and `turn_id`
- `PermissionRegistry`/`QuestionRegistry` store `session_id` and `turn_id`
  (scoped `respond_scoped` / `answer_question_scoped` with
  `get_pending_for_session` filter)
- `EventLog` persists important events to SQLite
- `NotificationRouter` coalesces and prioritizes notifications
- TTS routes through daemon via `CoreRequest::NotificationSpeak` /
  `NotificationStop` in `RemoteCore` mode
- `App::load_sessions_via_core`, `load_messages_via_core`,
  `load_models_via_core`, and `load_tasks_via_core` route TUI
  session/message/model/task loading through `CoreClient`
- `App::init_remote_core` is called after socket connect to populate
  models from the daemon via `CoreRequest::SnapshotModels`
- `CoreFrame` protocol for WebSocket clients
- Typed `CoreResponse::Events` and `CoreResponse::ResyncRequired`
  variants replace the previous `Json` and `error-code` envelopes
- `EventLog::current_seq()` returns the latest assigned sequence
  number (was off-by-one, returning the next-to-be-assigned value)
- Legacy TUI replay buffer (`TUI_EVENT_BUFFER`, `record_tui_event`,
  `replay_tui_events`, `convert_app_event`) removed from
  `src/server/ws.rs`; the `/tui` endpoint now subscribes directly
  to `daemon.subscribe()` and uses the daemon's `EventLog` for
  replay/resume
- TUI store setters (`set_session_store`, `set_message_store`,
  `set_memory_store`, `set_preferences`) gate on `AppMode` and
  no-op in `RemoteCore` mode

## What's New (polish)

- `core_event_metadata()` helper extracts `(session_id, turn_id)` for
  every `CoreEvent` variant; the event bridge now publishes with
  inferred metadata instead of `(None, None)`
- `EventLog::replay_from()` returns `event_seq > from_event_seq`;
  `from_event_seq` is documented as "last event sequence already
  seen by the client" and `0` replays from the beginning
- `TurnStarted` is emitted at turn creation; deltas, tool events,
  and completion/failure inherit the active turn id
- `TurnCancel { turn_id }` validates the supplied id against the
  active runtime turn and rejects mismatches
- `ServerHello` returns a negotiated `client_id`; socket connections
  keep per-connection `EventFilter`s and `forward_events()` filters
  live events using the same match logic as replay
- `SnapshotModels` returns concrete `provider/model` ids by reusing
  the local `ModelDiscoveryService` path, not provider names
- `core_event_type()` emits stable string names (e.g. `turn_started`)
  stored in `core_event_log.event_type`; recovery queries use those
  names plus `turn_id IS NOT NULL` and dedupe to a single event
- `PermissionRespond`/`QuestionRespond` parse full protocol IDs
  safely and return `invalid_permission_id` / `invalid_question_id`
  on malformed input; two sessions can hold pending permissions
  without cross-response
- `AudioArbiter::next_speech_batch()` synthesizes cross-session
  messages ("Three sessions need attention...") with priority
  ordering and a max queue length
- `App::load_initial_session_via_core` handles `--session`,
  `--continue`, `--new`, and `--fork` startup paths through
  `CoreClient` without touching local stores in socket mode

## What's New (hardening)

- **Socket subscription semantics hardened**: a `session_id: None`
  Subscribe frame produces a global-only filter (`include_global:
  false`) and no longer matches every session. A `session_id:
  Some(sid)` Subscribe frame produces a session-scoped filter
  (`include_global: true`) so subscribers still see sessionless
  events. `event_matches_filter()` and the in-memory `filter_matches`
  helper in `event_log.rs` are the single source of truth for
  matching and are mirrored by `replay_from_db` for SQL queries.
- **Resume coverage semantics**: `EventLog::covers_from()` and
  `db_covers_from()` distinguish "no new events" from "the
  requested sequence is too old to replay". `CoreRequest::Resume`
  returns `Events { events: [], current_seq }` for already-caught-up
  or future seqs and only returns `ResyncRequired` when the ring
  and DB together cannot satisfy the request.
- **Direct turn completion/failure**: the `TurnSubmit` spawn task
  now publishes `CoreEvent::TurnCompleted` / `TurnFailed` directly
  with the captured `turn_id`, before clearing
  `runtime.active_turn`. `AppEvent::AgentFinished` is still
  published (and still used by the bridge to update token counts
  and emit notifications) but is no longer mapped to a
  `CoreEvent::TurnCompleted`; this prevents duplicate lifecycle
  events.
- **`SocketCoreClient::subscribe_session_events`**: an inherent
  helper that sends a session-scoped `Subscribe` frame using the
  negotiated `client_id`. Used by tests and by any socket-only
  client that wants to opt in to a specific session after
  `ServerHello`.
- **End-to-end socket isolation tests**: three new tests in
  `src/core/transport/daemon_socket_integration_tests.rs` prove
  the daemon socket path actually isolates events across
  clients/sessions:
  - `two_socket_session_filter_isolation` (existing) — two
    clients, two sessions, no cross-talk.
  - `global_only_subscription_does_not_receive_session_events` —
    a global-only client receives sessionless events and never
    receives session events.
  - `resume_replay_uses_same_filter_as_live_forwarding` —
    replay on Subscribe filters with the same semantics as live
    forwarding.

## What's Preserved

- Inproc mode works exactly as before
- All existing tests pass; targeted test runs of every modified
  module are green (44 tests in protocol/core/daemon/event_log/
  notification/session_runtime/bus/tui::app — see Test Coverage)
- TUI behavior unchanged for inproc mode
- `/tui` endpoint retained for legacy remote TUI clients; internally
  uses daemon's `EventLog` exclusively (no in-server replay buffer)
- `StdioCoreClient` retained for external embedding
- Backward-compatible unscoped `PermissionRegistry`/`QuestionRegistry`
  API retained (delegates to scoped methods with `DEFAULT_SESSION_ID`);
  the existing `CoreClient` trait shape and `CoreRequest`/`CoreResponse`
  envelopes are unchanged for inproc and stdio transports

## Test Coverage

Final counts after the initial migration (tests in `src/`):

- `src/core/`: 31 unit tests (was 29 — 2 new in `daemon.rs`:
  `resume_returns_typed_resync_when_seq_too_old`,
  `resume_returns_typed_events_on_success`).
- `src/protocol/`: 8 unit tests (was 6 — 2 new round-trip
  serialization tests: `response_serializes_events`,
  `response_serializes_resync_required`).
- `src/bus/`: 1 unit test
- `src/tui/app/`: 15 unit tests (was 12 — 3 new in
  `remote_core_loader_tests`: `load_models_via_core_populates_state`,
  `load_tasks_via_core_returns_tasks_array`,
  `load_models_via_core_fails_without_core_client`).

Polish pass added coverage for metadata, replay semantics, socket
filtering, identity negotiation, turn lifecycle, recovery, and
notification batching:

- `src/core/`: additional tests for `core_event_metadata` envelope
  extraction, `replay_from` last-seen semantics, recovery dedupe of
  interrupted turns, and per-connection socket filter isolation
  between two simulated clients
- `src/protocol/`: round-trip tests for `ServerHello.client_id` and
  the stable `core_event_type` string mapping
- `src/notification/`: tests for `next_speech_batch` priority
  ordering, dedupe, and cross-session synthesis
- `src/bus/`: test confirming `TurnCancel` rejects a wrong turn id

Hardening pass added coverage for the new semantics:

- `src/core/daemon.rs`:
  - `resume_from_current_seq_returns_empty_events_not_resync`
  - `resume_from_future_seq_returns_empty_events`
  - `resume_from_too_old_seq_returns_resync`
  - `bridge_no_longer_maps_agent_finished_to_turn_completed`
  - `direct_turn_completion_uses_runtime_turn_id`
- `src/core/event_log.rs`:
  - `covers_from_current_seq_is_true`
  - `covers_from_too_old_ring_seq_is_false_without_db`
  - `covers_from_too_old_ring_seq_is_true_with_db`
  - `replay_from_current_seq_returns_empty`
- `src/core/transport/daemon_socket.rs`: filter unit tests renamed
  and extended to assert that a global filter never matches session
  events and that a session filter can opt into global events via
  `include_global`.
- `src/core/transport/daemon_socket_integration_tests.rs`:
  - `global_only_subscription_does_not_receive_session_events`
  - `resume_replay_uses_same_filter_as_live_forwarding`

`cargo check --all-features` and `cargo clippy --all-features` are
clean for all modified files; no new warnings introduced.
