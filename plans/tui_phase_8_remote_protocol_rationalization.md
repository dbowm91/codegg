# TUI Phase 8: Remote TUI Protocol Rationalization

## Objective

Clarify and harden the remote TUI protocol so daemon/multi-frontend work has a stable foundation. The current code has remote-mode plumbing and `RenderFrame` handling, but the product direction needs an explicit protocol model: either remote clients receive state/event deltas and render locally, or they receive frame-level render snapshots. Mixing both without a contract will create fragile behavior.

## Current Shape

The embedded TUI is event-driven: it owns `App`, mutates state on commands/events, and renders ratatui frames locally. Remote mode exists conceptually through `AppMode::RemoteCore`, remote event channels, and remote event handling, but it is not yet a fully specified rendering protocol.

The risk is that a remote client could appear connected but receive incomplete or ambiguous updates, especially if `RenderFrame` is unsupported or only logged. Before adding richer frontend clients, remote behavior should be explicit and testable.

## Decision Point

Choose one primary protocol model.

### Option A: Event/State-Driven Remote UI

The daemon sends typed app state snapshots or deltas. Remote clients render independently.

Advantages:

- Better for desktop/web/mobile clients.
- Avoids terminal-frame coupling.
- Fits long-running core daemon architecture.
- Allows frontend-specific layouts.

Costs:

- Requires stable DTOs for UI state.
- Requires versioning and resync semantics.
- Ratatui-specific render behavior is not automatically shared.

### Option B: Frame-Driven Remote UI

The daemon renders terminal frames or frame-like snapshots and remote clients display them.

Advantages:

- Closer to current ratatui render pipeline.
- Potentially simpler for terminal remoting.
- Local and remote TUI visuals can match exactly.

Costs:

- Harder for GUI/mobile/web clients.
- Terminal dimensions and input semantics become protocol concerns.
- Frame snapshots can be large and inefficient.

## Recommendation

Adopt **Option A: event/state-driven remote UI** as the main protocol. Treat `RenderFrame` as unsupported legacy/prototype behavior unless a specific terminal-remoting client needs it later. The code should fail loudly and clearly if a remote peer sends a frame-driven message that is not supported.

## Protocol Contract

Define a versioned protocol envelope:

```rust
pub struct RemoteTuiEnvelope<T> {
    pub protocol_version: u32,
    pub sequence: u64,
    pub session_id: Option<String>,
    pub payload: T,
}
```

Define payload categories:

```rust
pub enum RemoteTuiMessage {
    Hello(RemoteHello),
    HelloAck(RemoteHelloAck),
    StateSnapshot(RemoteTuiStateSnapshot),
    StateDelta(RemoteTuiStateDelta),
    UserInput(RemoteUserInput),
    Command(RemoteTuiCommand),
    ResyncRequest { reason: String },
    Error(RemoteTuiError),
}
```

Exact names should align with existing protocol modules.

## State Snapshot DTO

Add a frontend-neutral snapshot DTO that contains only render-relevant state, not internal `App` references.

Suggested shape:

```rust
pub struct RemoteTuiStateSnapshot {
    pub route: String,
    pub session: Option<RemoteSessionSummary>,
    pub status: RemoteStatus,
    pub model: String,
    pub agent: String,
    pub messages: Vec<RemoteMessageView>,
    pub prompt: RemotePromptState,
    pub sidebar: RemoteSidebarState,
    pub dialog: Option<RemoteDialogState>,
    pub toasts: Vec<RemoteToast>,
    pub diagnostics: Option<RemoteTuiDiagnostics>,
}
```

Do not expose large internal message/tool structures if the remote client only needs a rendered view. Keep the DTO stable and compact.

## Delta Semantics

Deltas should be optional at first. A snapshot-only implementation is acceptable if it is correct. If deltas are implemented, define:

- monotonic sequence numbers
- last-applied sequence acknowledgement from client
- resync request on gaps
- full snapshot after reconnect
- stale delta ignore rules

Do not implement unversioned ad hoc partial updates.

## Unsupported `RenderFrame` Handling

If a `RenderFrame` payload exists, change behavior from silent log/no-op to an explicit protocol response:

```rust
RemoteTuiError {
    code: "unsupported_render_frame",
    message: "Frame-driven remote rendering is not supported; request state snapshots instead",
    recoverable: true,
}
```

Also document this in `architecture/tui.md`.

## Implementation Steps

### 1. Locate and inventory existing remote protocol types

Search for:

- `RemoteTui`
- `RenderFrame`
- `remote_event_rx`
- `remote_event_tx`
- `handle_remote_event`
- `AppMode::RemoteCore`

Document each path in comments or architecture notes.

### 2. Decide and document the protocol model

Update `architecture/tui.md` and any agent skill files:

- remote mode is event/state-driven
- embedded ratatui remains local terminal rendering
- frame-driven rendering is unsupported unless explicitly revived
- remote clients should request snapshots/resyncs

### 3. Add protocol version constants

Add a protocol version constant near remote protocol types:

```rust
pub const REMOTE_TUI_PROTOCOL_VERSION: u32 = 1;
```

Handshake should reject incompatible major versions or degrade explicitly.

### 4. Add snapshot builder

Implement a pure function/method:

```rust
impl App {
    pub fn remote_snapshot(&self) -> RemoteTuiStateSnapshot { ... }
}
```

This method must not block, perform filesystem I/O, call core, or mutate state. It should only read current `App` state and produce owned DTOs.

### 5. Add resync path

When a remote client connects or sequence gaps are detected, send a full snapshot. Add a `RemoteTuiCommand::RequestSnapshot` or equivalent command if not already present.

### 6. Clarify remote input handling

Remote user input should convert into the same input/command paths as local input when possible:

- key input maps through input layer
- command input becomes `TuiCommand`
- prompt submission uses the same prompt submission classification

Avoid separate semantics for remote and embedded clients unless necessary.

### 7. Add tests

Unit tests should cover:

- snapshot builder does not panic on empty app
- snapshot builder includes route/status/model/agent/messages/dialog/toasts
- unsupported `RenderFrame` returns explicit error
- sequence gap triggers resync request or snapshot
- stale deltas are ignored if deltas exist
- remote input command maps to the expected local command

## Acceptance Criteria

- Remote TUI protocol model is documented as state/event-driven.
- Unsupported frame rendering behavior is explicit, not silent.
- `App::remote_snapshot()` or equivalent exists and is pure/nonblocking.
- Remote reconnect/resync behavior is defined.
- Versioning exists for remote protocol messages.
- Tests cover snapshot creation and unsupported frame behavior.
- Workspace checks pass.

## Out of Scope

- Full desktop/web/mobile frontend implementation.
- Efficient binary protocol optimization.
- Terminal frame streaming.
- Multi-client collaboration semantics.
- Authentication/authorization for remote clients beyond existing daemon security model.
