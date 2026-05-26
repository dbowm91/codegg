# Core Architecture Review Findings

## Verified Claims

- **CoreClient trait**: Verified at `src/core/mod.rs:13-20` with `request()` and `subscribe()` methods.

- **InprocCoreClient fields**: All 4 fields verified at `src/core/mod.rs:22-28` - `subagent_pool`, `memory_store`, `bg_scheduler`, `pool`. All wrapped in `Option<Arc<...>>`.

- **CoreRequest variants**: All session lifecycle, turn lifecycle, session data, and operational helper variants verified in `src/protocol/core.rs:50-175`. Complete list matches documentation.

- **CoreEvent enum**: Verified in `src/protocol/core.rs:177-272`. All snapshot, turn, tool, permission/question, session, subagent, and error events present.

- **subscribe() implementation**: Verified at `src/core/mod.rs:702-725` - spawns async task that subscribes to GlobalEventBus and forwards events via channel.

- **Protocol version 1**: Verified at `src/protocol/core.rs:3` - `PROTOCOL_VERSION = 1`.

- **Transport modes**: Inproc, Stdio, Socket all implemented in `src/core/transport/`.

## Stale Information

- **TurnSubmit fields**: Documentation mentions `text` and `plan_mode` but doesn't mention other fields. Actual `TurnSubmit` at `protocol/core.rs:115-123` also has `session_id`, `model`, `agents`, `current_agent_idx`, `messages`.

- **CoreRequest::Initialize handling**: Document says `Initialize` falls through to `Ack` but doesn't explicitly state this. This is correct per `src/core/mod.rs:698`.

## Bugs Found

- **map_app_event_to_core_event incomplete mapping**: At `src/core/mod.rs:728-797`, many AppEvent variants are mapped to `None` and dropped. Specifically:
  - `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` are NOT mapped to CoreEvent despite both enums having corresponding variants
  - `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels` are NOT handled
  - `TurnStarted`, `TurnFailed` are NOT handled
  
  This means in-process subscribers miss many events that could be useful for UI.

## Improvements Suggested

- **Complete CoreEvent mapping**: The `map_app_event_to_core_event()` function should be completed to map all relevant AppEvent variants to CoreEvent equivalents, enabling full event flow for in-process subscribers.

- **CoreRequest fallthrough documentation**: Explicitly document which CoreRequest variants are handled vs fall through to Ack.

## Cross-Module Issues

- **Subagent events not flowing to CoreEvent**: `SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed` are published to GlobalEventBus (and thus visible to SSE clients via `/api/event`) but don't appear in CoreEvent (used by in-process `subscribe()`). This creates inconsistent event visibility.

- **CoreRequest::Subscribe and CoreRequest::Resume**: These variants exist in protocol but InprocCoreClient doesn't handle them - they fall through to Ack silently at line 698. This may cause issues for remote TUI resume functionality.