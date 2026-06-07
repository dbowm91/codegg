# Codegg Core Daemon / Multi-Frontend Architecture Migration Plan

## Purpose

Move Codegg from a primarily TUI-owned runtime with an in-process core facade into a long-lived core daemon that can coordinate multiple frontends, multiple active sessions, event replay/resume, permission routing, background tasks, and centralized notification/audio policy.

This migration is expected to take several passes. Do not attempt to convert the full application in one large patch. The correct strategy is staged extraction with behavior-preserving intermediate states.

The end state should support:

1. A single long-lived Codegg core daemon per user/project scope.
2. Multiple frontend connectors attached simultaneously:
   - Local TUI.
   - Multiple TUI panes/windows.
   - Terminal multiplexer workflows.
   - Future GUI frontend.
   - Future mobile/web client.
   - Headless automation client.
3. Multiple active sessions managed by the daemon.
4. Session-scoped event streams with replay/resume.
5. Centralized permission/question routing.
6. Centralized notification/audio policy so sound/TTS does not overlap across sessions/frontends.
7. In-process mode retained as a development and simple-local fallback.
8. Socket daemon mode as the preferred long-running local architecture.

## Current Architecture Summary

The current repo already has useful seams:

- `CoreClient` exists as the typed request/response boundary between the TUI and core logic.
- Core protocol types exist in `src/protocol/core.rs`.
- The CLI already exposes `--core-transport` with `inproc`, `stdio`, and `socket`.
- The TUI startup path already chooses a `CoreClient` implementation.
- `InprocCoreClient` handles real core operations today.
- `StdioCoreClient` and `SocketCoreClient` exist, but are currently request/response only.
- `subscribe()` only works meaningfully for the in-process path; socket/stdio return empty receivers.
- The `server` module has WebSocket/SSE ideas and replay buffering, but it is not yet unified with the core daemon protocol.
- TTS currently lives in TUI state and is macOS `say`-oriented, which is the wrong ownership model for multi-session notification arbitration.
- Permission/question registries have ID scoping limitations that should be fixed before serious multi-client use.

The core migration is therefore not a rewrite. It is an ownership inversion:

Current approximate model:

```text
TUI process
  ├─ initializes DB/config/providers/memory/subagents/scheduler
  ├─ owns UI state and much operational state
  ├─ embeds or connects to CoreClient
  └─ may speak/play notifications locally
```

Target model:

```text
Core daemon process
  ├─ owns DB/config/providers/memory/subagents/scheduler
  ├─ owns session runtimes and active turns
  ├─ owns event log and subscription fanout
  ├─ owns permission/question routing
  ├─ owns notification/audio policy
  └─ exposes typed protocol to frontends

Frontend process
  ├─ owns rendering/input/local UI state
  ├─ attaches to one or more sessions
  ├─ receives snapshots/events
  └─ sends semantic requests to daemon
```

## Non-Goals

Do not implement a full distributed system. This is a local or LAN-capable daemon architecture, not a clustered coordinator.

Do not make the first pass depend on a GUI, mobile app, cloud sync, or remote auth overhaul.

Do not make TTS/cloud audio part of the critical path. Notification/audio should be a backend behind a daemon-owned policy layer.

Do not preserve split authority between TUI and daemon. During migration, temporary duplication is acceptable, but each pass should reduce it.

Do not silently fall back from explicit daemon/socket mode to in-process mode. That creates split-brain behavior.

## Design Invariants

These invariants should hold in the final architecture and should guide each intermediate patch.

### Authority

The core daemon is the authority for:

- Session lifecycle.
- Active turns.
- Model/agent selection for a session.
- Background scheduler.
- Subagent pool.
- Tool execution runtime.
- Permission/question registries.
- Memory store.
- Event sequencing.
- Notification/audio arbitration.

The frontend is the authority for:

- Rendering.
- Focus and modal stack.
- Scroll position.
- Input mode.
- Local keybindings.
- Layout.
- Per-frontend visual preferences.

### Event Semantics

Every event emitted by the daemon must have:

- Monotonic `event_seq`.
- Timestamp.
- Optional `session_id`.
- Optional `turn_id`.
- Typed payload.
- Enough metadata for clients to filter and route.

Events must be replayable from a bounded ring buffer. Later, important events may be persisted to SQLite, but an in-memory ring buffer is sufficient for the first implementation.

### Request Semantics

Every client request must have:

- `request_id`.
- `protocol_version`.
- Optional `client_id`.
- Typed payload.
- Structured response or structured error.

Long-running operations must return `Ack` quickly and emit progress/completion events.

### Session Semantics

A session can have zero, one, or many attached clients.

A client can observe multiple sessions.

A session can have at most one active turn unless explicitly designed otherwise.

A session should have explicit control semantics:

```rust
enum AttachMode {
    Observe,
    Control,
    ExclusiveControl,
}
```

Only a controller should submit prompts, cancel turns, steer turns, or answer permissions/questions by default.

### Notification Semantics

Only the daemon should decide whether to play sound/TTS for core events.

Frontends may display notifications, but they should not independently speak global session events.

Audio/TTS must be queued, deduplicated, coalesced, and interruptible by priority.

## Target Module Layout

This does not need to be exact, but it gives a stable target for implementers.

```text
src/
  core/
    mod.rs
    daemon.rs
    state.rs
    request.rs
    event_log.rs
    session_runtime.rs
    client_registry.rs
    notification.rs
    transport/
      mod.rs
      inproc.rs
      socket.rs
      stdio.rs
  protocol/
    core.rs
    frames.rs
  notify/
    mod.rs
    policy.rs
    audio.rs
    tts.rs
  tui/
    ...
```

Alternative: keep `tts` where it is for now, but move ownership/invocation into daemon-side notification policy.

## Pass 0: Audit and Baseline Tests

### Goal

Create a baseline so later refactors can be verified.

### Tasks

1. Run the current test suite:
   ```sh
   cargo test
   cargo check --all-features
   cargo clippy --all-targets --all-features
   ```

2. Add focused tests around existing core protocol behavior if missing:
   - `SessionCreate`.
   - `SessionLoad`.
   - `SessionMessagesLoad`.
   - `TurnSubmit` returns `Ack`.
   - Permission response path.
   - Question response path.

3. Add a small architecture note under `plans/` or `architecture/` stating that daemon migration is in progress.

### Acceptance Criteria

- No behavior change.
- Tests/checks pass or known failures are documented.
- Baseline output is recorded in the handoff notes.

### Do Not

- Do not introduce socket multiplexing yet.
- Do not modify TUI behavior yet.
- Do not move TTS yet.

## Pass 1: Extract `CoreDaemon` Without Behavior Change

### Goal

Move request handling out of `InprocCoreClient` into a daemon-owned object, while keeping existing in-process behavior intact.

### Rationale

Currently, `InprocCoreClient` is both a client adapter and the core implementation. That prevents socket/stdio/server transports from sharing identical behavior without duplication. The first major seam is to make `InprocCoreClient` a thin wrapper over `CoreDaemon`.

### New Types

Create `src/core/daemon.rs`:

```rust
pub struct CoreDaemon {
    state: Arc<CoreState>,
    event_log: Arc<EventLog>,
    sessions: Arc<SessionRuntimeRegistry>,
    clients: Arc<ClientRegistry>,
    notifications: Arc<NotificationRouter>,
}

impl CoreDaemon {
    pub async fn new(config: Config, pool: SqlitePool) -> Result<Self, AppError>;

    pub async fn handle_request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;

    pub fn subscribe(
        &self,
        filter: EventFilter,
    ) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}
```

Create `src/core/state.rs`:

```rust
pub struct CoreState {
    pub config: Config,
    pub pool: SqlitePool,
    pub session_store: SessionStore,
    pub message_store: MessageStore,
    pub memory_store: Arc<MemoryStore>,
    pub provider_registry: ProviderRegistry,
    pub subagent_pool: Arc<SubAgentPool>,
    pub bg_scheduler: Arc<BackgroundScheduler>,
}
```

The exact fields may need adjustment to avoid clone/ownership issues. Prefer `Arc` around long-lived services. Avoid storing values that are cheap and safer to construct per request unless they are expensive or need shared state.

### Refactor Steps

1. Add `CoreDaemon` and `CoreState`.
2. Move the request match from `InprocCoreClient::request()` into `CoreDaemon::handle_request()`.
3. Change `InprocCoreClient` to hold `Arc<CoreDaemon>`.
4. Keep `CoreClient` trait unchanged for now.
5. Preserve the existing TUI startup flow as much as possible.
6. Keep global event bus integration working.

### Transitional Constructor

The first pass may expose:

```rust
impl CoreDaemon {
    pub fn from_existing_parts(
        config: Config,
        pool: SqlitePool,
        memory_store: Option<Arc<MemoryStore>>,
        subagent_pool: Option<Arc<SubAgentPool>>,
        bg_scheduler: Option<Arc<BackgroundScheduler>>,
    ) -> Self;
}
```

This allows the current TUI startup code to pass the resources it already builds. Later passes should move resource initialization into daemon startup.

### Acceptance Criteria

- In-process TUI still works.
- Existing core requests behave the same.
- `InprocCoreClient::request()` is thin.
- No socket/stdio behavior change required.
- `cargo check` passes.

### Do Not

- Do not try to solve multi-client behavior in this pass.
- Do not remove the global event bus yet.
- Do not modify the external protocol in this pass unless absolutely necessary.

## Pass 2: Add Core-Level Event Log and Subscription API

### Goal

Move event sequencing/replay into core rather than server-specific WebSocket state.

### Rationale

The server currently has a bounded replay buffer concept for TUI events. The daemon needs this capability at the core layer so socket, stdio, TUI, GUI, and web clients share identical resume behavior.

### New Types

Create `src/core/event_log.rs`:

```rust
pub struct EventLog {
    next_seq: AtomicU64,
    ring: Mutex<VecDeque<EventEnvelope<CoreEvent>>>,
    tx: broadcast::Sender<EventEnvelope<CoreEvent>>,
    capacity: usize,
}

pub struct EventFilter {
    pub session_id: Option<String>,
    pub client_id: Option<String>,
    pub include_global: bool,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self;

    pub fn publish(
        &self,
        session_id: Option<String>,
        turn_id: Option<String>,
        payload: CoreEvent,
    ) -> u64;

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope<CoreEvent>>;

    pub fn replay_from(
        &self,
        from_event_seq: u64,
        filter: &EventFilter,
    ) -> Vec<EventEnvelope<CoreEvent>>;
}
```

### Behavior

- All daemon events should go through `EventLog::publish()`.
- The event log assigns sequence numbers.
- The ring buffer stores recent events.
- Subscribers receive live events through broadcast.
- Resume requests can replay events where `event_seq > from_event_seq`.

### Bridge From Existing `GlobalEventBus`

Initially, do not rip out `GlobalEventBus`.

Instead:

1. Add a bridge task in `CoreDaemon::new()` that subscribes to `GlobalEventBus`.
2. Convert `AppEvent` to `CoreEvent` with existing or extracted mapping logic.
3. Publish mapped events into `EventLog`.
4. Preserve old TUI inproc behavior during transition.

This avoids refactoring every producer in one pass.

### CoreRequest Handling

Implement real handling for:

```rust
CoreRequest::Subscribe { session_id }
CoreRequest::Resume { session_id, from_event_seq }
```

Possible response shape:

```rust
CoreResponse::Json {
    data: json!({
        "events": replayed_events,
        "current_seq": latest_seq
    })
}
```

If changing `CoreResponse` is cleaner, add:

```rust
CoreResponse::Events {
    events: Vec<EventEnvelope<CoreEvent>>,
    current_seq: u64,
}
```

Prefer typed response if it does not cause broad churn.

### Acceptance Criteria

- Inproc subscribers still receive events.
- Replay from `from_event_seq` returns expected events.
- Filtering by `session_id` works for session events.
- Ring buffer capacity is bounded.
- Lag/resync behavior is defined.

### Do Not

- Do not persist all events to SQLite yet.
- Do not require all event producers to publish directly to `EventLog` yet.
- Do not delete `GlobalEventBus`.

## Pass 3: Implement Event-Capable Socket Transport

### Goal

Make `SocketCoreClient` capable of concurrent request/response and event subscription.

### Rationale

The current socket client serializes one request, waits for one response, and returns an empty subscription receiver. That cannot support a daemon-connected TUI. Socket mode needs multiplexed frames and background reader/writer tasks.

### Protocol Frames

Create `src/protocol/frames.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreFrame {
    ClientHello(ClientHello),
    ServerHello(ServerHello),

    Request(RequestEnvelope<CoreRequest>),
    Response {
        request_id: String,
        response: CoreResponse,
    },

    Subscribe {
        client_id: String,
        session_id: Option<String>,
        from_event_seq: Option<u64>,
    },

    Event(EventEnvelope<CoreEvent>),

    Error {
        request_id: Option<String>,
        code: String,
        message: String,
    },

    Ping,
    Pong,
}
```

Add:

```rust
pub struct ClientHello {
    pub client_name: String,
    pub client_kind: ClientKind,
    pub protocol_version: u32,
    pub capabilities: ClientCapabilities,
}

pub struct ServerHello {
    pub daemon_id: String,
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
}

pub struct ClientCapabilities {
    pub visual_notifications: bool,
    pub desktop_notifications: bool,
    pub audio: bool,
    pub tts: bool,
    pub multi_session_view: bool,
}
```

### Socket Server

Add a daemon socket listener:

```rust
pub async fn run_core_socket(
    daemon: Arc<CoreDaemon>,
    endpoint: &str,
) -> Result<(), AppError>;
```

Behavior:

1. Bind Unix socket.
2. Accept many clients.
3. For each client:
   - Read frames.
   - Handle `ClientHello`.
   - Register client in `ClientRegistry`.
   - Handle requests by calling `CoreDaemon::handle_request()`.
   - Handle subscribe by replaying requested events and forwarding live events.
4. Clean up client registry on disconnect.

### Socket Client

Refactor `SocketCoreClient`:

- Spawn reader task.
- Spawn writer task.
- Maintain `pending: DashMap<RequestId, oneshot::Sender<CoreResponse>>`.
- Maintain `events_tx: mpsc::UnboundedSender<EventEnvelope<CoreEvent>>`.
- `request()` sends a `CoreFrame::Request` and awaits the pending response.
- `subscribe()` returns receiver backed by the reader task.
- Add reconnect later; first pass can fail cleanly on disconnect.

### Acceptance Criteria

- A TUI can connect with `--core-transport socket`.
- `request()` still works.
- `subscribe()` receives live turn events.
- Two clients can subscribe simultaneously.
- Disconnecting one client does not kill the daemon.
- Events are sequence-tagged and replayable.

### Do Not

- Do not add TCP remote networking in this pass.
- Do not overbuild auth; local Unix socket permissions are sufficient for this phase.
- Do not silently fall back to inproc when explicit socket mode fails.

## Pass 4: Add Explicit Daemon Commands and Lifecycle

### Goal

Expose a clear daemon UX and avoid accidental split-brain behavior.

### CLI Commands

Add:

```text
codegg daemon start
codegg daemon stop
codegg daemon status
codegg daemon logs
codegg attach --session <id>
codegg attach --new
```

Or, if preserving current naming is easier:

```text
codegg server --daemon
codegg attach unix://...
```

But `daemon` is clearer.

### Socket Paths

Use per-user runtime paths.

Suggested defaults:

Linux:

```text
$XDG_RUNTIME_DIR/codegg/core.sock
```

macOS:

```text
~/Library/Application Support/codegg/core.sock
```

Fallback:

```text
~/.local/share/codegg/core.sock
```

Avoid:

```text
/tmp/codegg-core.sock
```

unless explicitly configured.

### Startup Semantics

Configuration:

```toml
[daemon]
enabled = false
auto_start = false
socket = "auto"
project_scope = "cwd" # or "user"
```

Behavior:

- Default plain `codegg` can remain inproc initially.
- `codegg --core-transport socket` should fail if socket unavailable.
- `codegg daemon start` starts daemon explicitly.
- `daemon.auto_start = true` permits TUI to auto-spawn daemon.
- Explicit socket mode must not silently fall back to inproc.

### Status Output

`codegg daemon status` should show:

- Running/not running.
- Socket path.
- Daemon PID if known.
- Active sessions.
- Connected clients.
- Event sequence.
- Uptime.

### Acceptance Criteria

- User can start daemon.
- User can attach a TUI to daemon.
- Explicit socket failures are visible.
- No silent split-brain fallback.
- Daemon can be stopped cleanly.

## Pass 5: Session Runtime Registry

### Goal

Make the daemon the owner of active session runtime state.

### Rationale

Persistent session data lives in SQLite, but active turns, cancellation handles, attached clients, pending prompts, and runtime selection state need daemon-owned memory structures.

### New Types

Create `src/core/session_runtime.rs`:

```rust
pub struct SessionRuntimeRegistry {
    sessions: DashMap<String, Arc<SessionRuntime>>,
}

pub struct SessionRuntime {
    pub session_id: String,
    pub project_id: String,
    pub directory: PathBuf,

    pub status: RwLock<RuntimeSessionStatus>,
    pub selected_model: RwLock<Option<String>>,
    pub selected_agent: RwLock<Option<String>>,

    pub active_turn: RwLock<Option<TurnHandle>>,
    pub attached_clients: DashMap<ClientId, AttachMode>,

    pub pending_permissions: DashSet<String>,
    pub pending_questions: DashSet<String>,
}

pub struct TurnHandle {
    pub turn_id: String,
    pub cancel_tx: watch::Sender<bool>,
    pub steer_tx: Option<mpsc::UnboundedSender<String>>,
    pub started_at: DateTime<Utc>,
}
```

### Implement Real Request Handling

Update request handling for:

- `SessionAttach`.
- `TurnSubmit`.
- `TurnCancel`.
- `TurnSteer`.
- `AgentSelect`.
- `ModelSelect`.

`TurnSubmit`:

1. Ensure runtime exists.
2. Reject if active turn exists unless policy allows queueing.
3. Create `turn_id`.
4. Store `TurnHandle`.
5. Emit `TurnStarted`.
6. Spawn agent loop.
7. On completion/failure:
   - Clear active turn.
   - Emit `TurnCompleted` or `TurnFailed`.
   - Trigger notification event.

`TurnCancel`:

1. Find session runtime.
2. Find active turn by ID.
3. Signal cancellation.
4. Emit cancellation event.
5. Return `Ack`.

If the current `AgentLoop` cannot yet accept cancellation, add the runtime plumbing and return a structured error or best-effort cancellation. Do not pretend cancellation works if it does not.

`AgentSelect` and `ModelSelect`:

1. Update runtime state.
2. Persist preference if appropriate.
3. Emit `SessionUpdated` or dedicated model/agent changed event.

### Acceptance Criteria

- `TurnCancel` is no longer a no-op.
- Active turn state is visible through a snapshot/debug request.
- Multiple attached clients see turn lifecycle events.
- Submitting a second turn to the same active session gets a clear error or defined queue behavior.

### Do Not

- Do not allow uncontrolled concurrent turns in one session.
- Do not let each TUI maintain its own model/agent authority in daemon mode.

## Pass 6: Fix Permission and Question Scoping

### Goal

Make permission/question routing safe for multiple sessions and multiple clients.

### Current Problem

Permission registry keys do not include `session_id`, so filtering pending permissions by session is unreliable.

Question registry keys use only `session_id`, so multiple pending questions for the same session are ambiguous.

### New ID Scheme

Use IDs like:

```text
perm:{session_id}:{turn_id}:{tool_call_id}:{tool_name}
question:{session_id}:{turn_id}:{question_id}
```

Add typed metadata:

```rust
pub struct PendingPermission {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub path: Option<String>,
    pub args: serde_json::Value,
    pub created_at: Instant,
}

pub struct PendingQuestion {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub questions: serde_json::Value,
    pub created_at: Instant,
}
```

### Protocol Updates

Update `CoreEvent::PermissionPending` to include:

- `session_id`.
- `turn_id`.
- `id`.
- `tool`.
- `path`.
- optionally `args`.

Update `CoreEvent::QuestionPending` to include:

- `session_id`.
- `turn_id`.
- `id`.
- `questions`.

Update `CoreRequest::PermissionRespond` and `QuestionRespond` handling to use full IDs.

### Client Routing

Only controller clients for a session should see interactive permission/question prompts by default.

Observer clients may receive a non-interactive status event like “permission pending” but should not display an answer dialog unless configured.

### Acceptance Criteria

- Two sessions can have simultaneous permission prompts.
- Two questions in one session do not collide.
- The correct session/client receives the prompt.
- Wrong or stale IDs return structured errors.
- Timeout cleanup remains safe.

## Pass 7: Snapshot and Resync Protocol

### Goal

Allow frontends to reconstruct complete state after attach, reconnect, or event lag.

### Rationale

Events are incremental. A frontend attaching late needs a snapshot, then events after the snapshot sequence.

### Add Requests

If not already present, add or fully implement:

```rust
CoreRequest::SnapshotSession { session_id: String }
CoreRequest::SnapshotWorkspace { project_dir: String }
CoreRequest::SnapshotModels
CoreRequest::SnapshotDaemon
```

Add response types:

```rust
CoreResponse::SnapshotSession {
    event_seq: u64,
    snapshot: SessionSnapshot,
}

CoreResponse::SnapshotDaemon {
    event_seq: u64,
    snapshot: DaemonSnapshot,
}
```

Possible snapshot structures:

```rust
pub struct SessionSnapshot {
    pub session: Session,
    pub messages: Vec<Message>,
    pub status: RuntimeSessionStatus,
    pub active_turn: Option<TurnSnapshot>,
    pub selected_model: Option<String>,
    pub selected_agent: Option<String>,
    pub pending_permissions: Vec<PendingPermissionSnapshot>,
    pub pending_questions: Vec<PendingQuestionSnapshot>,
    pub token_usage: TokenUsageSnapshot,
    pub subagents: Vec<SubagentSnapshot>,
}

pub struct DaemonSnapshot {
    pub daemon_id: String,
    pub uptime_secs: u64,
    pub current_event_seq: u64,
    pub active_sessions: Vec<SessionRuntimeSnapshot>,
    pub connected_clients: Vec<ClientSnapshot>,
}
```

### Attach Flow

A frontend should:

1. Connect.
2. Send `ClientHello`.
3. Attach to session(s).
4. Request snapshot.
5. Subscribe from `snapshot.event_seq`.
6. Apply replayed/live events.

### Acceptance Criteria

- A frontend can attach to an already-running session and render useful state.
- A frontend can reconnect after disconnect and replay events.
- If requested event seq is too old, daemon returns `ResyncRequired`.
- Resync path requests snapshot and resumes from snapshot seq.

## Pass 8: Reduce TUI Operational Ownership in Daemon Mode

### Goal

Make socket/daemon-connected TUI a true frontend rather than a second core runtime.

### Refactor

In `launch_tui`, separate local embedded mode from remote daemon mode.

For inproc mode:

- Build or embed `CoreDaemon`.
- Use `InprocCoreClient`.
- It is acceptable to initialize local resources.

For socket/daemon mode:

- Do not initialize:
  - SQLite pool.
  - SessionStore.
  - MessageStore.
  - ProviderRegistry.
  - ModelDiscoveryService.
  - SubAgentPool.
  - BackgroundScheduler.
  - MemoryStore.
- Instead:
  - Connect to daemon.
  - Send hello.
  - Request model/session snapshots.
  - Subscribe to events.
  - Render from snapshots/events.

### Transitional Strategy

If the current TUI requires stores for many paths, migrate incrementally:

1. Add `AppMode::Embedded` vs `AppMode::RemoteCore`.
2. In remote mode, make store-dependent actions call `CoreClient`.
3. Add typed helper methods on `App`:
   - `load_sessions_via_core`.
   - `load_messages_via_core`.
   - `load_models_via_core`.
   - `load_tasks_via_core`.
4. Remove direct store use from daemon mode paths first.
5. Later remove direct store use entirely where sensible.

### Acceptance Criteria

- TUI attached to socket daemon does not create its own scheduler/subagent pool/provider registry.
- Session list works via daemon.
- Loading messages works via daemon.
- Submitting prompts works via daemon.
- TUI state remains rendering/input-focused.

## Pass 9: Centralized Notification and Audio Policy

### Goal

Make daemon-side notification routing responsible for sound/TTS, preventing overlap across sessions and frontends.

### Rationale

Audio overlap is a symptom of per-frontend notification ownership. The daemon should coalesce and prioritize events.

### New Module

Create `src/core/notification.rs` or `src/notify/`.

Types:

```rust
pub enum NotificationKind {
    TurnCompleted,
    TurnFailed,
    AwaitingInput,
    PermissionRequired,
    QuestionRequired,
    SubagentCompleted,
    SubagentFailed,
    Error,
}

pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Urgent,
}

pub struct NotificationEvent {
    pub id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: NotificationKind,
    pub priority: NotificationPriority,
    pub message: String,
    pub dedupe_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub struct NotificationPolicy {
    pub enabled: bool,
    pub visual: bool,
    pub desktop: bool,
    pub audio: bool,
    pub tts: bool,
    pub coalesce_window_ms: u64,
    pub quiet_hours: Option<QuietHours>,
    pub speak_kinds: HashSet<NotificationKind>,
}
```

### Audio Arbiter

Implement a single daemon-owned queue:

```rust
pub struct AudioArbiter {
    queue: Mutex<VecDeque<NotificationEvent>>,
    current: Mutex<Option<AudioPlayback>>,
    policy: NotificationPolicy,
}
```

Behavior:

- Coalesce bursts.
- Deduplicate repeated status events.
- Do not overlap speech.
- High-priority events can interrupt low-priority speech.
- Low-priority events may be dropped if queue is full.
- Support mute/unmute.

### Backends

Initial backend priority:

1. No audio.
2. System beep/sound.
3. Command TTS:
   - macOS: `say`.
   - Linux: `spd-say` or `espeak-ng`.
   - Windows: PowerShell/System.Speech.
4. Later: local neural TTS.

Config:

```toml
[notifications]
enabled = true
coalesce_window_ms = 1200

[notifications.audio]
enabled = true
backend = "command"
command = "say"
args = ["{message}"]
speak = ["permission_required", "question_required", "turn_failed", "turn_completed"]
interrupt_on = ["permission_required", "question_required", "turn_failed"]
```

### Events That Should Trigger Notifications

- `TurnCompleted`: low/normal priority depending on whether frontend is focused.
- `TurnFailed`: high.
- `PermissionPending`: urgent/high.
- `QuestionPending`: urgent/high.
- `SubagentCompleted`: low unless it blocks parent turn.
- `SubagentFailed`: high.
- `Error`: high.

### Acceptance Criteria

- If multiple sessions complete at once, daemon speaks one coalesced message.
- TTS does not overlap itself.
- Multiple attached TUIs do not each trigger audio.
- Visual events still reach frontends.
- Audio can be disabled independently from visual notifications.

## Pass 10: Server/WebSocket Protocol Convergence

### Goal

Avoid maintaining two divergent control planes: `CoreRequest/CoreEvent` and `TuiMessage`.

### Options

Option A: Make `/ws` speak canonical `CoreFrame`.

Option B: Keep `/tui` for remote terminal remoting, but internally translate to `CoreRequest/CoreEvent`.

Recommended:

- Canonical protocol: `CoreFrame`.
- Compatibility protocol: `TuiMessage`.
- Future GUI/mobile should use `CoreFrame`, not keypress-oriented `TuiMessage`.

### Refactor

1. Add a WebSocket endpoint that uses `CoreFrame`.
2. Route requests to `CoreDaemon`.
3. Route subscriptions through `EventLog`.
4. Keep old `/tui` path temporarily.
5. Mark old direct TUI remoting protocol as compatibility/legacy once GUI/web protocol exists.

### Acceptance Criteria

- WebSocket clients can use the same semantic protocol as socket clients.
- Replay/resume semantics match socket daemon.
- Server-specific replay buffer is removed or delegates to core `EventLog`.

## Pass 11: Persistence and Recovery Improvements

### Goal

Improve daemon resilience after the main architecture works.

### Possible Additions

1. Persist important events to SQLite:
   - Turn started/completed/failed.
   - Permission/question pending/resolved.
   - Session status changes.
   - Notification records.
2. Restore active session metadata on daemon restart.
3. Mark interrupted turns as failed/interrupted.
4. Store client-independent notification history.

### Acceptance Criteria

- Restarting daemon does not corrupt sessions.
- Previously active turns are marked interrupted.
- Session history remains accurate.
- Clients get clear resync after daemon restart.

## Pass 12: Cleanup and Removal of Legacy Paths

### Goal

Reduce maintenance burden once daemon/socket path is stable.

### Cleanup Candidates

- Remove or demote duplicate server replay buffer.
- Remove direct TUI store access for daemon-capable flows.
- Make `CoreDaemon` the only implementation of core request handling.
- Keep `InprocCoreClient` as embedded daemon adapter.
- Keep stdio only if tests or external embedding justify it.
- Remove UI-owned TTS for global notifications; keep message-readout TTS if desired as a separate per-client accessibility feature.

### Acceptance Criteria

- Clear module ownership.
- No duplicate request handling.
- No duplicate event replay implementations.
- No duplicate notification/audio arbitration.

## Testing Plan

### Unit Tests

Add tests for:

- `EventLog` sequence assignment.
- `EventLog` bounded ring behavior.
- Event filtering by session.
- Replay from sequence.
- Replay too old -> resync required.
- Permission ID generation.
- Question ID generation.
- Notification coalescing.
- Audio queue priority behavior.

### Integration Tests

Add tests for:

- Inproc client request + subscribe.
- Socket client request + subscribe.
- Two socket clients attached to same session.
- Two socket clients attached to different sessions.
- Permission prompt routes to correct session.
- Question prompt routes to correct session.
- Reconnect/resume receives missed events.
- Explicit socket mode fails if daemon unavailable.
- Daemon auto-start works only when configured.

### Manual Tests

1. Start daemon:
   ```sh
   codegg daemon start
   ```

2. Open two TUIs:
   ```sh
   codegg attach --new
   codegg attach --new
   ```

3. Start a long-running prompt in each.

4. Confirm:
   - Both sessions stream independently.
   - One TUI can observe one or both sessions.
   - Completion events are not duplicated.
   - Permission prompts appear only for controller clients.
   - Audio notification does not overlap.

5. Kill one TUI:
   - Daemon continues running.
   - Other TUI remains connected.
   - Session continues if no exclusive control policy blocks it.

6. Reattach:
   - Snapshot loads.
   - Missed events replay or resync happens cleanly.

## Suggested Commit / PR Breakdown

### PR 1: CoreDaemon extraction

- Add `CoreDaemon`.
- Move request handling from `InprocCoreClient`.
- Keep behavior unchanged.

### PR 2: EventLog

- Add core event log.
- Bridge global bus to event log.
- Implement replay API.
- Add tests.

### PR 3: Socket frame protocol

- Add `CoreFrame`.
- Refactor socket client/server.
- Implement live event subscription.

### PR 4: Daemon CLI

- Add `codegg daemon start/status/stop`.
- Add socket path resolution.
- Remove silent fallback for explicit socket mode.

### PR 5: SessionRuntimeRegistry

- Add runtime registry.
- Implement active turn handles.
- Make cancel/model/agent selection real.

### PR 6: Permission/question scoping

- Fix IDs.
- Add metadata.
- Update events and responses.

### PR 7: Snapshot/resync

- Add snapshot requests/responses.
- Make attach/reconnect robust.

### PR 8: TUI remote-core cleanup

- Avoid local resource initialization in daemon mode.
- Route more actions through core.

### PR 9: Notification/audio daemon policy

- Add notification router.
- Add audio arbiter.
- Move global TTS/sound decisions into daemon.

### PR 10: Server/WebSocket convergence

- Make WebSocket use canonical core frames.
- Keep TUI remoting compatibility temporarily.

## Risk Register

### Risk: Split-brain runtime

If socket connection fails and TUI silently falls back to inproc, users may unknowingly run separate cores.

Mitigation: explicit socket mode must fail loudly. Auto-start must be opt-in.

### Risk: Event loss

Bounded buffers can drop events under high volume.

Mitigation: expose `ResyncRequired`; clients request snapshots when replay is incomplete.

### Risk: Permission misrouting

Existing permission IDs are insufficiently scoped.

Mitigation: fix permission/question IDs before serious multi-client release.

### Risk: TUI assumes direct stores

The TUI currently has many paths that assume local stores/resources.

Mitigation: use `AppMode::Embedded` vs `AppMode::RemoteCore`; migrate actions gradually.

### Risk: Audio annoyance

Speech notifications can become noisy.

Mitigation: daemon-side coalescing, priority, quiet hours, per-session mute, and default conservative policy.

### Risk: Protocol churn

Multiple protocols can diverge.

Mitigation: make `CoreFrame` canonical. Treat `TuiMessage` as compatibility or terminal-remoting-specific.

## Final Target User Experience

Simple local mode:

```sh
codegg
```

This can remain embedded/inproc by default initially.

Daemon mode:

```sh
codegg daemon start
codegg attach --new
codegg attach --session <session-id>
```

Terminal multiplexer workflow:

```sh
# Pane 1
codegg attach --session parser-refactor

# Pane 2
codegg attach --session tui-polish

# Pane 3
codegg attach --session search-agent
```

Future multi-session TUI:

```sh
codegg attach
# Home screen shows all daemon sessions and active statuses.
# User can open/split/focus sessions in one terminal UI.
```

Notification behavior:

```text
One daemon receives:
  parser-refactor completed
  tui-polish awaiting input
  search-agent failed

Daemon emits visual notifications to attached clients.
Daemon speaks one coalesced audio message:
  "Codegg: three sessions need attention. Search-agent failed."
```

## Implementation Guidance for Smaller Agents

When implementing a pass, do not broaden scope. Each pass should compile and preserve existing behavior unless the pass explicitly changes behavior.

Before editing:

1. Locate the relevant files.
2. Read current types and call sites.
3. Add narrow tests when possible.
4. Make the smallest viable change.
5. Run `cargo check`.
6. Document any skipped behavior or known limitation.

When uncertain, prefer adding a new compatibility layer over deleting an old path. Cleanup should happen after the daemon path is proven stable.

The most important order is:

1. Extract daemon.
2. Centralize events.
3. Make socket event-capable.
4. Add lifecycle commands.
5. Move runtime ownership.
6. Fix permission routing.
7. Move notifications/audio.

Do not start with TTS. Audio becomes simple once daemon ownership and event arbitration are correct.

