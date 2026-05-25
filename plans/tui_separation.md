# TUI-Core Architectural Separation Plan

## Goal

Separate the TUI from core execution/state so the terminal UI is a client, not the owner of agent/session logic.

## Non-Goals

- Rewrite Ratatui components.
- Redesign command UX.
- Replace existing server/attach flows in one cut.

## Current Coupling (Observed)

The local TUI currently owns:

- Session/message CRUD and loading.
- AgentLoop creation and turn execution.
- Permission/question response wiring.
- Memory operations and consolidation commands.
- Model discovery refresh.
- Subagent/task operations.
- Workspace indexing/git/worktree queries.

This means transport alone (`stdio`/WS) is insufficient; ownership must move first.

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

Perform an interface-first extraction:

1. Define transport-neutral protocol messages.
2. Define core facade trait (`CoreClient`) used by TUI.
3. Introduce in-process adapter implementing facade.
4. Migrate TUI features slice-by-slice behind facade.
5. Add transport adapters (`stdio`, local socket/pipe, then WS remote).

## Phases

## Phase 1 - Protocol and Facade Foundation

### Deliverables

- `protocol/core.rs` containing semantic request/event envelopes.
- `core` module containing facade trait + in-process adapter skeleton.
- Explicit protocol versioning and request correlation.

### Acceptance Criteria

- Protocol types compile and serialize/deserialize.
- Core facade compiles without changing behavior.
- No runtime behavior change yet.

## Phase 2 - Turn Execution Slice

### Deliverables

- Move prompt submit/turn run path behind facade.
- Core becomes source of truth for turn lifecycle events.

### Acceptance Criteria

- TUI no longer constructs AgentLoop directly.
- Turn events are delivered through facade event stream.

## Phase 3 - Session and History Slice

### Deliverables

- Session list/create/load/fork/archive/share via facade.
- Message history loading via facade snapshots.

### Acceptance Criteria

- TUI no longer calls SessionStore/MessageStore directly.

## Phase 4 - Interactive Gate Slice

### Deliverables

- Permission/question pending + responses routed via facade.
- Session-scoped subscription for pending items.

### Acceptance Criteria

- TUI no longer calls PermissionRegistry/QuestionRegistry directly.

## Phase 5 - Auxiliary Services Slice

### Deliverables

- Memory commands, task commands, model refresh, MCP status via facade.
- Workspace/git/worktree info sourced from core snapshot/events.

### Acceptance Criteria

- TUI no longer performs service mutations directly.

## Phase 6 - Transport Adapters

### Deliverables

- `stdio` transport adapter implementing `CoreClient`.
- Local daemon adapter (Unix socket / Windows named pipe).
- WS adapter aligned to protocol for remote mode.

### Acceptance Criteria

- TUI can run in `inproc` and `stdio` modes with same behavior.
- Reconnect supported via `resume_from_seq` in daemon/WS modes.

## Protocol Shape (v1 Draft)

## Envelope Metadata

All requests/events carry:

- `protocol_version`
- `request_id` (for command responses)
- `session_id` (when applicable)
- `turn_id` (when applicable)
- `event_seq` (events)
- `timestamp_ms`

## Client Requests

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

## Core Events

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

## Risks and Mitigations

- Risk: dual sources of truth during migration.
- Mitigation: one slice at a time; each slice removes direct calls before next slice.

- Risk: event mismatch between local and remote.
- Mitigation: protocol-first with in-process adapter as canonical behavior.

- Risk: reconnect/lossy stream behavior.
- Mitigation: sequence numbers + resumable subscription contract.

## Tracking Checklist

- [x] Create separation plan.
- [x] Phase 1 protocol + facade scaffolding.
- [x] Phase 2 prep: isolate local turn execution path into dedicated helper.
- [x] Phase 2 cutover: core-enabled mode submits turns via `CoreClient`.
- [x] Phase 2 migrate turn execution.
- [x] Phase 3 foundation: add typed `CoreResponse` for request/response flows.
- [x] Phase 3 slice: route session message loading via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route session list/reload via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route session mutation actions (`archive`, `delete`, `fork`) via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route local session creation via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route session message-count aggregation via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route session restore/share/unshare/rename via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route session export/import/template creation via `CoreClient` in core-enabled mode.
- [x] Phase 3 slice: route memory consolidation message loading via `CoreClient` in core-enabled mode.
- [x] Phase 3 hardening: remove direct-store fallback branches for migrated session/history flows in `tui/mod.rs`.
- [x] Phase 3 slice: route tree dialog data loading via `CoreClient` snapshots (`session.list` + `session.message_counts`).
- [x] Phase 3 migrate session/history.
- [x] Phase 4 slice: route permission/question responses via `CoreClient` requests in local mode.
- [x] Phase 4 migrate permission/question.
- [x] Phase 5 slice: route task list/delete operations via `CoreClient`.
- [x] Phase 5 slice: route model refresh via `CoreClient`.
- [x] Phase 5 slice: route memory list/search/remember/forget operations via `CoreClient`.
- [x] Phase 5 slice: route task scheduling (`/loop`) via `CoreClient`.
- [x] Phase 5 slice: route worktree listing (`/worktree`) via `CoreClient`.
- [x] Phase 5 migrate auxiliary services.
- [x] Phase 6 slice: add `core::transport` module with `stdio` CoreClient adapter.
- [x] Phase 6 slice: add socket adapter scaffold for daemon transport.
- [x] Phase 6 slice: add in-process event subscription bridge (`AppEvent` -> `CoreEvent` envelopes).
- [x] Phase 6 slice: wire startup transport selection (`CODEGG_CORE_TRANSPORT=inproc|stdio`) and hidden `core-stdio` endpoint.
- [x] Phase 6 slice: add explicit CLI transport selection (`--core-transport inproc|stdio`).
- [x] Phase 6 slice: implement Unix-socket request/response + reconnect in `SocketCoreClient`.
- [x] Phase 6 slice: wire socket startup selection and endpoint resolution (`--core-transport socket`, `--core-endpoint`, `CODEGG_CORE_ENDPOINT`).
- [x] Phase 6 slice: add WS resume handshake message (`resume`) with server-side resync response.
- [x] Phase 6 slice: add sequence-based WS replay envelopes (`event_seq` + buffered replay on `resume`).
- [x] Phase 6 add transport adapters.

## Immediate Next Steps

1. Run manual parity pass across `inproc`, `stdio`, and `socket` transport modes.
2. Optionally persist WS replay buffer across process restarts if cross-restart resume is required.
