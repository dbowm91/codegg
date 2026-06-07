# Daemon Migration Status

## Completed Passes

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

## Architecture

```text
CoreDaemon
├── pool, subagent_pool, memory_store, bg_scheduler
├── EventLog (ring buffer + SQLite persistence)
├── SessionRuntimeRegistry (active turns, cancellation)
├── ClientRegistry
├── NotificationRouter (audio/desktop/visual policy)
├── AudioArbiter (TTS queue, priority interrupt)
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
  API retained (delegates to scoped methods with `DEFAULT_SESSION_ID`)

## Test Coverage

Final counts after this work (tests in `src/`):

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

`cargo check --all-features` and `cargo clippy --all-features` are
clean for all modified files; no new warnings introduced.
