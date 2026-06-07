# Daemon Migration Status

## Completed Passes

- **Pass 0**: Audit and baseline tests ✅
- **Pass 1**: CoreDaemon extraction ✅
- **Pass 2**: Core-level EventLog ✅
- **Pass 3**: Event-capable socket transport ✅
- **Pass 4**: Daemon CLI commands ✅
- **Pass 5**: Session runtime registry ✅
- **Pass 6**: Permission/question scoping ✅
- **Pass 7**: Snapshot and resync protocol ✅
- **Pass 8**: TUI remote-core mode ✅
- **Pass 9**: Centralized notification policy ✅
- **Pass 10**: Server/WebSocket convergence ✅
- **Pass 11**: Event persistence ✅
- **Pass 12**: Cleanup ✅

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
- Permission/question events carry session_id and turn_id
- PermissionRegistry/QuestionRegistry store session_id and turn_id
  (scoped `respond_scoped` / `answer_question_scoped` with
  `get_pending_for_session` filter)
- EventLog persists important events to SQLite
- NotificationRouter coalesces and prioritizes notifications
- TTS routes through daemon via `CoreRequest::NotificationSpeak` /
  `NotificationStop` in `RemoteCore` mode
- `App::load_sessions_via_core` and `App::load_messages_via_core`
  helpers route TUI session/message loading through CoreClient
- CoreFrame protocol for WebSocket clients

## What's Preserved

- Inproc mode works exactly as before
- All existing tests pass (35 core tests, 12 TUI app tests, 1 bus test)
- TUI behavior unchanged for inproc mode
- Server /tui and /ws endpoints unchanged
- Backward-compatible unscoped PermissionRegistry/QuestionRegistry API
  retained (delegates to scoped methods with `DEFAULT_SESSION_ID`)
