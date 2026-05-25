# TUI-Core Architectural Separation Plan

## Status: COMPLETED

All phases have been implemented. The TUI is now decoupled from core execution/state, with the terminal UI functioning as a client via the `CoreClient` facade.

---

## Goal

Separate the TUI from core execution/state so the terminal UI is a client, not the owner of agent/session logic.

## Non-Goals

- Rewrite Ratatui components.
- Redesign command UX.
- Replace existing server/attach flows in one cut.

## Current Coupling (Observed)

The local TUI previously owned:

- Session/message CRUD and loading.
- AgentLoop creation and turn execution.
- Permission/question response wiring.
- Memory operations and consolidation commands.
- Model discovery refresh.
- Subagent/task operations.
- Workspace indexing/git/worktree queries.

This meant transport alone (`stdio`/WS) was insufficient; ownership had to move first.

## Target Architecture

### Core Owns

- Authoritative session/turn state.
- Agent execution and tool/provider orchestration.
- Permission and question lifecycle.
- Memory/task/subagent services.
- Event sequencing, session-scoped subscriptions, reconnect snapshots.

### TUI Owns

- Rendering and local interaction state.
- Input mapping to semantic client requests.
- Displaying snapshots/deltas from core.

## Migration Strategy

Performed an interface-first extraction:

1. Defined transport-neutral protocol messages (`protocol/core.rs`).
2. Defined core facade trait (`CoreClient`) used by TUI.
3. Introduced in-process adapter implementing facade (`InprocCoreClient`).
4. Migrated TUI features slice-by-slice behind facade.
5. Added transport adapters (`stdio`, local socket/pipe, then WS remote).

## Phase Summary

### Phase 1 - Protocol and Facade Foundation ✓

- `protocol/core.rs` containing semantic request/event envelopes.
- `core` module containing facade trait + in-process adapter skeleton.
- Explicit protocol versioning and request correlation.

### Phase 2 - Turn Execution Slice ✓

- Move prompt submit/turn run path behind facade.
- Core becomes source of truth for turn lifecycle events.

### Phase 3 - Session and History Slice ✓

- Session list/create/load/fork/archive/share via facade.
- Message history loading via facade snapshots.

### Phase 4 - Interactive Gate Slice ✓

- Permission/question pending + responses routed via facade.
- Session-scoped subscription for pending items.

### Phase 5 - Auxiliary Services Slice ✓

- Memory commands, task commands, model refresh via facade.
- Workspace/git/worktree info sourced from core snapshot/events.

### Phase 6 - Transport Adapters ✓

- `stdio` transport adapter implementing `CoreClient`.
- Local daemon adapter (Unix socket).
- WS adapter aligned to protocol for remote mode.

## Protocol Shape (v1)

### Envelope Metadata

All requests/events carry:

- `protocol_version`
- `request_id` (for command responses)
- `session_id` (when applicable)
- `turn_id` (when applicable)
- `event_seq` (events)
- `timestamp_ms`

### Client Requests

- `initialize`
- `subscribe`
- `resume`
- `session.list`
- `session.create`
- `session.attach`
- `session.load`
- `session.fork`
- `session.archive`
- `turn.submit`
- `turn.cancel`
- `turn.steer`
- `agent.select`
- `model.select`
- `models.refresh`
- `permission.respond`
- `question.respond`
- `memory.search`
- `memory.remember`
- `memory.forget`
- `task.list`
- `task.delete`

### Core Events

- `snapshot.session`
- `snapshot.workspace`
- `snapshot.models`
- `turn.started`
- `turn.text_delta`
- `turn.reasoning_delta`
- `tool.started`
- `tool.completed`
- `permission.pending`
- `question.pending`
- `turn.completed`
- `turn.failed`
- `session.updated`
- `file.changed`
- `subagent.started`
- `subagent.progress`
- `subagent.completed`
- `subagent.failed`

## Implementation Details

### Core Clients

| Type | Purpose |
|------|---------|
| `InprocCoreClient` | Runs the core in the current process and publishes `CoreEvent` envelopes from the global event bus |
| `StdioCoreClient` | Spawns `codegg core-stdio` and exchanges JSONL requests/responses over stdin/stdout |
| `SocketCoreClient` | Connects to a Unix socket endpoint and exchanges JSONL requests/responses with reconnect logic |

### Startup Selection

The TUI chooses the core transport from:

1. `--core-transport` CLI flag
2. `CODEGG_CORE_TRANSPORT` environment variable
3. Default `inproc`

For socket mode, `--core-endpoint` or `CODEGG_CORE_ENDPOINT` provides the Unix socket path.

## Risks and Mitigations

- Risk: dual sources of truth during migration.
- Mitigation: one slice at a time; each slice removes direct calls before next slice.

- Risk: event mismatch between local and remote.
- Mitigation: protocol-first with in-process adapter as canonical behavior.

- Risk: reconnect/lossy stream behavior.
- Mitigation: sequence numbers + resumable subscription contract.

## Immediate Next Steps

1. Run manual parity pass across `inproc`, `stdio`, and `socket` transport modes.
2. Optionally persist WS replay buffer across process restarts if cross-restart resume is required.
