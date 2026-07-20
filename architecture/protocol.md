# Protocol Module

The `protocol` module defines the shared request/response envelopes and message types used for communication between TUI, client, server, and core components.

## Overview

## Provider-connection lifecycle operations

Lifecycle requests and responses are additive and redacted: detail/status
DTOs expose IDs, endpoint authority, state, revisions, health, and bounded
diagnostic codes, never credential material. Rotation carries an opaque local
secret-input handle. `ConnectionStateChanged` and `ConnectionRotated` carry
actor seams and revisions for audit-ready projection.

**Location**: `crates/codegg-protocol/` (the `codegg-protocol` crate)

Domain identity types live in `codegg-core::identity`, not in this wire crate.
Protocol DTOs intentionally keep string-backed `project_id`, `workspace_id`,
and `directory` fields in this milestone. Core conversions may use typed
relations internally, but serialization remains unchanged and a directory is
never promoted to a durable project identity.

**Re-export**: `codegg::protocol` via `pub use codegg_protocol as protocol` in `src/lib.rs`

**Key Responsibilities**:
- Define `CoreRequest` and `CoreResponse` for TUI/Core communication
- Define `TuiMessage` for server-to-TUI event streaming
- Provide versioned `RequestEnvelope` and `EventEnvelope` wrappers
- Establish the protocol boundary between UI and backend

## Module Structure

```
crates/codegg-protocol/src/
├── lib.rs     # Module exports
├── core.rs    # CoreRequest, CoreResponse, CoreEvent, envelopes
├── dto.rs     # Shared DTOs (Session, Message, etc.)
├── provider.rs # Secret-safe provider connection/provisioning DTOs
├── frames.rs  # ClientCapabilities, RequestEnvelope, EventEnvelope
├── plugin.rs  # PluginManifestDto, PluginInvocation, PluginResponse, PLUGIN_PROTOCOL_VERSION
├── projection/ # Frontend-neutral session projection contract (Milestone 1)
├── tui.rs     # TuiMessage, QuestionSpec, RemoteTuiStateSnapshot, REMOTE_TUI_PROTOCOL_VERSION
└── ui.rs      # UiNode, UiEffect, UiEffectEnvelope, UiLimits, validation, degradation
```

## Session projection contract (Milestone 1)

The `projection/` submodule under `codegg-protocol` defines the
frontend-neutral, versioned, bounded session projection contract.
It exposes:

- `ProjectionCapabilities` and `PROJECTION_PROTOCOL_VERSION = 1`
  for capability negotiation (`caps.rs`),
- bounded payload and collection limits plus string truncation
  helpers (`limits.rs`),
- bounded summaries for sessions, turns, messages, tools, runs,
  jobs, permissions, questions, artifacts, and the agent-tree
  placeholder (`dto.rs`),
- typed `ProjectionEvent` variants and `ProjectionEnvelope`
  (`event.rs`),
- `SessionProjectionSnapshot` and `ProjectionDiagnostic`
  (`snapshot.rs`),
- a deterministic canonical reducer `ProjectionReducer` plus
  `ReducerEventInput` and `ReducerConfig` (`reducer.rs`),
- adapters from existing `CoreResponse` snapshots and `CoreEvent`
  families (`adapters.rs`),
- golden fixtures (`fixtures.rs`).

The reducer is pure, deterministic, and never performs I/O. It
honours event sequence ordering, deduplicates by `event_seq`, and
records diagnostics for impossible or out-of-order transitions
rather than panicking. Unknown optional fields and variants are
tolerated when the negotiated version is within the declared
range; required version mismatches produce an explicit
`ReducerError::UnsupportedProtocolVersion`.

The contract is described in detail in `architecture/projection.md`
and exercised by `cargo test -p codegg-protocol` and
`cargo test --test session_projection_consumer`.

## Protocol Versions

```rust
// crates/codegg-protocol/src/core.rs
pub const PROTOCOL_VERSION: u32 = 2;

// crates/codegg-protocol/src/tui.rs
pub const REMOTE_TUI_PROTOCOL_VERSION: u32 = 3;

// crates/codegg-protocol/src/plugin.rs
pub const PLUGIN_PROTOCOL_VERSION: u32 = 1;

// crates/codegg-protocol/src/projection/caps.rs
pub const PROJECTION_PROTOCOL_VERSION: u32 = 1;
pub const PROJECTION_PROTOCOL_VERSION_MIN: u32 = 1;
pub const PROJECTION_CAPABILITY: &str = "session_projection.v1";
```

**Version history:**
- `PROTOCOL_VERSION`: bumped 1 → 2 in Phase 15 to accommodate `CoreEvent::PluginUiEffect { envelope: UiEffectEnvelope }`.
- `REMOTE_TUI_PROTOCOL_VERSION`: bumped 2 → 3 in Phase 15 to accommodate `TuiMessage::PluginUiEffect { envelope: UiEffectEnvelope }`.
- `PLUGIN_PROTOCOL_VERSION`: stable at 1; plugin wire format is independent of core/TUI protocol versions.
- `PROJECTION_PROTOCOL_VERSION`: initial value 1, introduced with the
  frontend-neutral session projection contract (Session Projections
  Milestone 1). The projection contract is additive — older clients
  ignore unknown optional variants and continue to function.

## Envelopes

### RequestEnvelope

```rust
pub struct RequestEnvelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub payload: T,
}
```

Wraps all requests with protocol version and a unique request ID for tracing/reply correlation.

### EventEnvelope

```rust
pub struct EventEnvelope<T> {
    pub protocol_version: u32,
    pub event_seq: u64,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: T,
}
```

Wraps events with sequence number, timestamp, and optional session/turn context for ordered delivery.

## CoreRequest Enum

Located in `crates/codegg-protocol/src/core.rs`. The durable-job surface is
daemon-authoritative: clients submit bounded typed job requests, then use
job queries, wait/cancel operations, and bounded scheduler snapshots. Clients
do not provide attempt IDs, daemon generations, resolved paths, or permits.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreRequest { ... }
```

### Session Lifecycle (19 variants)
- `Initialize` - Initialize session
- `Subscribe { session_id }` - Subscribe to session events
- `Resume { session_id, from_event_seq }` - Resume from event sequence
- `SessionList { project_id, show_archived, limit }` - List sessions
- `SessionCreate { directory, title }` - Create new session
- `SessionAttach { session_id }` - Attach to session
- `SessionLoad { session_id }` - Load session data
- `SessionMessagesLoad { session_id }` - Load session messages
- `SessionMessageCounts { session_ids }` - Get message counts
- `SessionFork { session_id }` - Fork session
- `SessionDelete { session_id, permanent }` - Delete session
- `SessionArchive { session_id, unarchive }` - Archive/unarchive session
- `SessionRestore { session_id }` - Restore session
- `SessionShare { session_id }` - Share session
- `SessionUnshare { session_id }` - Unshare session
- `SessionRename { session_id, new_title }` - Rename session
- `SessionExport { session_id }` - Export session
- `SessionImportData { data }` - Import session data
- `SessionCreateFromTemplate { template, project_id, directory }` - Create from template

### Provider connection lifecycle

`EggpoolConnectionCreate` carries a bounded `SecretInput` and is intended only
for local authenticated core IPC. Its debug/display projection is redacted,
and the remote core WebSocket rejects this secret-bearing request. The daemon
returns only `EggpoolConnectionCreated`, provisioning status, redacted
connection summaries, and bounded model DTOs. `EggpoolConnectionCancel`,
`EggpoolConnectionStatus`, `ProviderConnectionList`, and
`ProviderConnectionModels` are secret-free follow-up operations. These
operations create connection metadata but do not migrate session
provider/model selection.

### Turn Lifecycle (5 variants)
- `TurnSubmit { session_id, text, plan_mode, model, agents, current_agent_idx, messages }` - Submit a turn
- `TurnCancel { session_id, turn_id }` - Cancel a turn
- `TurnSteer { session_id, turn_id, text }` - Steer turn with text
- `AgentSelect { session_id, agent_name }` - Select agent
- `ModelSelect { session_id, model }` - Select model

### Model Operations (1 variant)
- `ModelsRefresh` - Refresh model list

### Permission/Question (2 variants)
- `PermissionRespond { id, choice }` - Respond to permission request
- `QuestionRespond { id, answers }` - Respond to question

### Memory Operations (4 variants)
- `MemorySearch { query }` - Search memory
- `MemoryList { namespace }` - List memory entries
- `MemoryRemember { text, namespace }` - Remember to memory
- `MemoryForget { id }` - Forget from memory

### Task Operations (3 variants)
- `TaskList` - List tasks
- `TaskSchedule { session_id, interval_secs, message }` - Schedule task
- `TaskDelete { id }` - Delete task

### Worktree Operations (1 variant)
- `WorktreeList { project_dir }` - List worktrees

### Workspace Operations (4 variants — Phase 2)
- `WorkspaceRegister { root }` - Register/lookup a workspace by canonical root
- `WorkspaceList { include_archived }` - List registered workspaces
- `WorkspaceArchive { workspace_id }` - Archive a workspace (turns to its sessions are rejected until rebound)
- `WorkspaceSnapshotRequest { workspace_id }` - Request a `WorkspaceSnapshot` for the given workspace

> **Phase 2.** `WorkspaceRegister` returns the canonical `WorkspaceRecord`
> for the requested root (creating one if none exists). `SessionCreate`
> continues to accept a `directory` for backward compatibility; the daemon
> resolves it to a `WorkspaceId` and rejects unbound sessions at
> `TurnSubmit`. See [`architecture/core.md`](core.md) and
> [`architecture/session.md`](session.md) for the binding model.

### Project Catalog Operations (Project Catalog Milestone 4)

Project catalog requests are daemon-authoritative and explicitly scoped. The
server does not supply a process-global project directory as identity.

- `ProjectList { include_archived, limit }` — return bounded project summaries.
- `ProjectGet { project_id }` — return one project with bounded workspace and
  health summaries.
- `ProjectRegister { workspace_id, display_name, description, tags }` — bind
  an already-registered local workspace to a durable project using the catalog
  registration service.
- `ProjectArchive { project_id }` / `ProjectRestore { project_id }` — perform
  logical, retry-safe lifecycle changes without deleting sessions or
  workspaces.
- `ProjectHealth { project_id, workspace_id }` — read the bounded aggregate
  catalog/workspace/asset/service health without activating services.

The matching responses contain only bounded DTOs and stable string-backed
identities. Lifecycle and health changes are represented by project-scoped
events. Clients and servers negotiate a project-catalog capability; legacy
clients continue to receive workspace/session projections, while locator-only
compatibility requests are resolved uniquely or fail with an actionable typed
context error. Paths are locators and display data, never project IDs.

### Durable execution operations (Phase 5 cutover)

- `JobSubmit { spec }` — validate and idempotently submit a durable job
- `JobWait { job_id, timeout_ms }` — await a bounded terminal completion projection
- `SchedulerSnapshot` — request bounded queue, resource, workspace, and executor state

The existing `JobGet`, `JobList`, `JobAttempts`, `JobCancel`, and schedule
operations remain available. `JobSubmitDto.submission_key` is an opaque,
length-bounded retry key scoped to the current daemon generation.

## CoreResponse Enum

Located in `crates/codegg-protocol/src/core.rs` (CoreResponse section).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse { ... }
```

| Variant | Fields | Purpose |
|---------|--------|---------|
| `Ack` | - | Empty acknowledgement |
| `Json` | `data: Value` | Generic JSON payload |
| `Session` | `session: Session` | Full session object (carries `workspace_id` — Phase 2) |
| `SessionMessages` | `session_id, messages` | Session message history |
| `SessionMessageCounts` | `counts: HashMap<String, usize>` | Message counts per session |
| `SessionList` | `sessions: Vec<Session>` | List of sessions |
| `WorkspaceList` | `workspaces: Vec<WorkspaceSnapshot>` | List of registered workspaces (Phase 2; snapshots include `services_active`, `active_leases`, `config_revision` from Phase 3) |
| `WorkspaceSnapshot` | `workspace: WorkspaceSnapshot` | Single workspace snapshot (Phase 2; fields extended by Phase 3) |
| `WorkspaceServicesSnapshot` | `services: Vec<WorkspaceServiceHealthDto>` | List every active workspace bundle (Phase 3) |
| `WorkspaceConfigReload` | `workspace_id, previous_revision, new_revision, diagnostics: Vec<ConfigDiagnosticDto>` | Result of `WorkspaceConfigReload` request (Phase 3) |
| `RunList` | `runs: Vec<RunSummaryDto>` | List runs visible to a workspace (Phase 3) |
| `RunGet` | `run: Option<RunRecordDto>` | Fetch a single run (Phase 3) |
| `RunArtifactChunk` | `data_b64, byte_offset, total_bytes` | Base64-encoded chunk of a run artifact (Phase 3) |
| `Error` | `code, message` | Error response |
| `JobSubmitted` | `job_id` | Durable submission acknowledgement |
| `JobWaited` | `job_id, status, summary, run_id` | Bounded terminal completion projection |
| `SchedulerSnapshot` | `snapshot: Value` | Bounded scheduler state for operator clients |
| `SnapshotDaemon` | `..., scheduler_snapshot` | Daemon snapshot with optional bounded scheduler state |

`SchedulerSnapshot` is intentionally a JSON projection rather than a
client-owned control object. Full job and attempt records are fetched through
their dedicated bounded query operations.

## CoreEvent Enum

Located in `crates/codegg-protocol/src/core.rs` (CoreEvent section).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent { ... }
```

### Snapshot Events (3)
- `SnapshotSession { session_id }` - Session state snapshot (carries `workspace_id` and `directory` — Phase 2)
- `SnapshotWorkspace { project_dir }` - Workspace snapshot (carries `workspace_id`, `canonical_root`, `display_name`, `active_sessions` — Phase 2)
- `SnapshotModels { current_model, models }` - Model list snapshot

### Turn Events (7)
- `TurnStarted { session_id, turn_id }` - Turn started
- `TurnTextDelta { session_id, turn_id, delta }` - Text delta
- `TurnReasoningDelta { session_id, turn_id, delta }` - Reasoning delta
- `TurnCompleted { session_id, turn_id, stop_reason }` - Turn completed
- `TurnFailed { session_id, turn_id, message }` - Turn failed (turn_id is `Option<String>`)
- `ToolStarted { session_id, turn_id, tool_name, tool_id, arguments }` - Tool started (turn_id is `Option<String>`)
- `ToolCompleted { session_id, turn_id, tool_id, output, success }` - Tool completed (turn_id is `Option<String>`)

### Permission/Question Events (2)
- `PermissionPending { id, tool, path }` - Permission pending
- `QuestionPending { id, questions }` - Question pending

### Session Events (2)
- `SessionUpdated { session_id }` - Session updated
- `FileChanged { path, action }` - File changed

### Subagent Events (4)
- `SubagentStarted { session_id, task_id, agent, description }` - Subagent started
- `SubagentProgress { session_id, task_id, agent, message }` - Subagent progress
- `SubagentCompleted { session_id, task_id, agent, result_summary }` - Subagent completed
- `SubagentFailed { session_id, task_id, agent, error }` - Subagent failed

### Error Events (1)
- `Error { code, message }` - Error occurred

## TuiMessage Enum

Located in `crates/codegg-protocol/src/tui.rs`.

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub enum TuiMessage { ... }
```

### Client-to-Server (3)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `Input` | `text` | User text input |
| `KeyDown` | `key, modifiers` | Key press event |
| `MouseClick` | `x, y` | Mouse click event |

### Connection Management (3)
- `Resize { w, h }` - Terminal resize
- `Resume { from_event_seq }` - Resume from event sequence
- `RequestSnapshot` - Request a full state snapshot from the daemon

### Response Messages (2)
- `PermissionResponse { id, choice }` - Permission response
- `QuestionResponse { id, answers }` - Question response

### Server-to-Client Events (10)
| Variant | Fields | Purpose |
|---------|--------|---------|
| `RenderFrame` | `content` | ❌ unsupported — returns `Error` with code `unsupported_render_frame` |
| `TextDelta` | `delta` | Text delta for streaming |
| `PermissionPending` | `id, tool, path` | Permission request |
| `QuestionPending` | `id, questions` | Question request |
| `SessionInfo` | `id, model` | Session information |
| `SessionEnded` | `stop_reason` | Session ended |
| `ToolCallStarted` | `tool_name, tool_id, arguments` | Tool call started |
| `ToolResult` | `tool_id, output, success` | Tool result |
| `Error` | `message` | Error message |
| `StateSnapshot` | `snapshot: RemoteTuiStateSnapshot` | Full state snapshot for remote rendering |

### Special (1)
- `ResyncRequired { reason, pending_permissions, pending_questions }` - Resync needed

### QuestionSpec

```rust
pub struct QuestionSpec {
    pub id: String,
    pub prompt: String,
    pub default: Option<String>,
}
```

### RemoteTuiStateSnapshot

```rust
pub struct RemoteTuiStateSnapshot {
    pub protocol_version: u32,
    pub sequence: u64,
    pub session_id: Option<String>,
    pub route: String,
    pub model: String,
    pub agent: String,
    pub status: String,
    pub messages: Vec<RemoteMessageView>,
    pub prompt: String,
    pub dialog: Option<String>,
    pub toasts: Vec<RemoteToastView>,
}

pub struct RemoteMessageView {
    pub role: String,
    pub content_preview: String,
    pub tool_calls: Vec<RemoteToolCallView>,
}

pub struct RemoteToolCallView {
    pub tool_id: String,
    pub tool_name: String,
    pub status: String,
}

pub struct RemoteToastView {
    pub message: String,
    pub level: String,
}
```

Frontend-neutral DTO containing render-relevant state. Produced by `App::remote_snapshot()` as a pure, nonblocking read of current `App` state. Used by the remote TUI event/state-driven protocol (Phase 8).

## Request/Response Flow

### In-Process Flow (InprocCoreClient)

```
TUI/App
  │
  │ request(RequestEnvelope<CoreRequest>)
  ▼
InprocCoreClient::request()
  │
  ├─► match CoreRequest variant
  │     └─► Database / SessionStore / AgentLoop
  │           └─► CoreResponse
  │
  └─► subscribe() → GlobalEventBus → EventEnvelope<CoreEvent>
```

### Stdio/Socket Flow

```
TUI/App
  │
  │ request(RequestEnvelope<CoreRequest>)
  ▼
StdioCoreClient / SocketCoreClient
  │
  │ JSONL over stdin/stdout or Unix socket
  ▼
core-stdio process / remote server
  │
  └─► Response written back as JSONL
```

### Remote TUI Flow (Server)

```
Remote TUI Client
  │
  │ WebSocket / HTTP
  ▼
Server (Axum)
  │
  ├─► CoreClient.request() → CoreResponse
  │
  └─► TuiMessage events → WebSocket push
```

## Versioning

The protocol uses explicit versioning constants in `crates/codegg-protocol/src/`:

- `PROTOCOL_VERSION = 2` in `core.rs` (core request/response/event envelopes)
- `REMOTE_TUI_PROTOCOL_VERSION = 3` in `tui.rs` (remote TUI WebSocket protocol)
- `PLUGIN_PROTOCOL_VERSION = 1` in `plugin.rs` (plugin wire format)

Envelopes (`RequestEnvelope`, `EventEnvelope`) include `protocol_version` to detect mismatches between client and server. See "Phase 15: Plugin UI Multi-Frontend" below for the rationale behind the version bumps.

## ClientHello/ServerHello Handshake

Both types live in `crates/codegg-protocol/src/frames.rs` and are variants of `CoreFrame`.

### ClientHello

```rust
pub struct ClientHello {
    pub client_name: String,
    pub client_kind: ClientKind,       // Tui | Gui | Web | Cli | Automation
    pub protocol_version: u32,
    pub capabilities: ClientCapabilities,
}
```

`ClientCapabilities` includes fields for visual/desktop notifications, audio, TTS, multi-session view, and plugin UI capability flags (dialog, toast, panel, status_item, table, markdown, code, progress). All default to `false` via `#[serde(default)]`.

### ServerCapabilities

`ServerCapabilities` is the daemon's view of what the local core can serve. Clients inspect these flags to decide whether to enable workspace UI affordances.

```rust
pub struct ServerCapabilities {
    pub event_replay: bool,
    pub session_management: bool,
    pub permission_routing: bool,
    /// Phase 2: daemon supports `WorkspaceRegister`/`WorkspaceList`/
    /// `WorkspaceArchive` requests. Legacy clients without this flag
    /// fall back to `SnapshotWorkspace { project_dir }`.
    #[serde(default)]
    pub workspace_registration: bool,
    /// Phase 2: daemon emits `WorkspaceSnapshot` records in turn
    /// snapshots when available.
    #[serde(default)]
    pub workspace_snapshots: bool,
}
```

### ServerHello

```rust
pub struct ServerHello {
    pub daemon_id: String,             // per-process 8-hex suffix identity
    pub protocol_version: u32,
    pub server_capabilities: ServerCapabilities,
    pub client_id: String,             // negotiated client_id assigned by daemon
}
```

`daemon_id` is the short per-process identity (e.g. `codegg-aabbccdd`). The full `generation` UUID is kept in the on-disk metadata file (`daemon.json`), not in the wire protocol. `client_id` is assigned by the daemon's `ClientRegistry` during the handshake.

### SnapshotDaemon

`CoreRequest::SnapshotDaemon` is a read-only probe. The daemon responds with `CoreResponse::SnapshotDaemon`:

```rust
CoreResponse::SnapshotDaemon {
    event_seq: u64,
    daemon_id: String,
    uptime_secs: u64,
    active_sessions: Vec<SessionSnapshot>,
    connected_clients: Vec<ClientSnapshot>,
}
```

This is used by `connect_or_start_daemon` for readiness verification and by `daemon status` for diagnostics. The `generation` and `started_at` fields are not on the wire — they are read from the on-disk `daemon.json` metadata file by the CLI.

## Implementation Notes

- `CoreRequest` and `CoreResponse` use `#[serde(tag = "type")]` for JSON discrimination
- `TuiMessage` similarly uses `#[serde(tag = "type")]`
- All enums use `rename_all = "snake_case"` for JSON compatibility
- The core module handles `CoreRequest` variants in `src/core/mod.rs`
- Subagent events (`SubagentStarted`, `SubagentProgress`, `SubagentCompleted`, `SubagentFailed`) exist in both `CoreEvent` and the event bus, and `map_app_event_to_core_event` DOES map all four subagent events (see `src/core/mod.rs:795-838`)

## Client Capabilities

Defined in `crates/codegg-protocol/src/frames.rs`:

```rust
pub struct ClientCapabilities {
    pub visual_notifications: bool,
    pub desktop_notifications: bool,
    pub audio: bool,
    pub tts: bool,
    pub multi_session_view: bool,
    pub plugin_ui_dialogs: bool,
    pub plugin_ui_panels: bool,
    pub plugin_ui_status_items: bool,
    pub plugin_ui_tables: bool,
    pub plugin_ui_markdown: bool,
    pub plugin_ui_code: bool,
    pub plugin_ui_progress: bool,
}
```

All fields default to `false` via `#[serde(default)]`. `ClientCapabilities::plugin_ui_capabilities()` converts the `plugin_ui_*` fields into a `PluginUiCapabilities` struct for capability-aware degradation.

## Phase 15: Plugin UI Multi-Frontend

Phase 15 turned the protocol-level plugin UI foundation into a stable multi-frontend contract supporting embedded TUI, remote TUI, CLI/automation, and future GUI/web/mobile frontends.

### UiEffectEnvelope

All plugin UI effects crossing the core/TUI/remote boundary are wrapped in a typed envelope that carries source attribution:

```rust
pub struct UiEffectEnvelope {
    pub session_id: Option<String>,
    pub source: UiEffectSource,
    pub invocation_id: Option<String>,
    pub effect: UiEffect,
}

pub enum UiEffectSource {
    Plugin { plugin_id: String },
    Core,
    Tui,
}
```

The envelope replaces the previous flat `PluginUiEffect` event/message shape in both `CoreEvent` and `TuiMessage`. Source attribution enables:

- **Session scoping**: effects with `session_id` set are filtered to clients subscribed to that session.
- **Ownership enforcement**: `source.plugin_id` must match durable surface id namespace (e.g. panel id `my-plugin:stats` must come from `plugin_id == "my-plugin"`).
- **Diagnostics**: `invocation_id` and `source` surface in management/doctor views.

### UiLimits and Validation

`UiLimits` in `crates/codegg-protocol/src/ui.rs` defines bounded resource caps to prevent plugin output from destabilizing clients:

```rust
pub struct UiLimits {
    pub max_effects_per_response: usize,
    pub max_effect_bytes: usize,
    pub max_node_depth: usize,
    pub max_table_rows: usize,
    pub max_table_columns: usize,
    pub max_string_len: usize,
    pub max_panels_per_plugin: usize,
    pub max_status_items_per_plugin: usize,
    pub max_open_dialogs_global: usize,
    pub max_snapshot_body_bytes: usize,
}
```

Presets:

- `UiLimits::balanced()` — default for embedded and remote TUI clients.
- `UiLimits::text_only()` — for CLI/automation clients with no rich UI support.

Validation functions:

- `validate_ui_effect(effect, limits) -> Result<(), UiValidationError>` — single-effect check.
- `validate_ui_effects(effects, limits) -> Result<(), UiValidationError>` — batch check (enforces `max_effects_per_response`).
- `validate_ui_node(node, limits, depth)` — recursive node validation against depth/string/table caps.

`UiValidationError` is an enum with `Display + Error` implementations. Validation rejects or truncates with diagnostics rather than panicking.

### Degradation Helpers

Deterministic degradation ensures unsupported clients receive appropriate output:

- `degrade_effect(effect, caps) -> Option<UiEffect>` — maps an effect to its degraded form for the given capability set. Returns `None` if the effect has no degraded equivalent (e.g. `ClosePanel` for a text-only client).
- `degrade_node_to_text(node) -> Option<String>` — flattens a `UiNode` tree to text for clients that cannot render structured nodes.
- `effect_summary(effect) -> Option<String>` — short human-readable summary for log lines.

Degradation matrix:

| Effect | Full UI client | Text-only client | Unsupported/automation |
| --- | --- | --- | --- |
| `EmitChat` | visible UI/chat surface | stdout/log text | log text |
| `ShowToast` | toast | prefixed text line | optional log |
| `OpenDialog` | modal/dialog | title + body text | log/report |
| `OpenPanel` | panel | heading + body text | omit or log summary |
| `AddStatusItem` | status bar | optional line | omit |
| `UpdatePanel` | update existing panel | text update | omit |
| `Close*` | close surface | no-op | no-op |

### Snapshot Durability

`RemoteTuiStateSnapshot` (in `crates/codegg-protocol/src/tui.rs`) now carries durable plugin surface metadata:

```rust
pub struct RemotePanelView {
    pub id: String,
    pub title: String,
    pub placement: String,
    pub source_plugin_id: Option<String>,
    pub body: Option<UiNode>,
}

pub struct RemoteStatusItemView {
    pub id: String,
    pub label: String,
    pub placement: String,
    pub source_plugin_id: Option<String>,
    pub body: Option<UiNode>,
}
```

Both `source_plugin_id` and `body` are optional with `#[serde(default, skip_serializing_if = "Option::is_none")]`, so legacy snapshots without these fields deserialize cleanly.

The snapshot builder (`App::build_remote_snapshot` in `src/tui/app/mod.rs`) populates:

- `source_plugin_id` — extracted from the surface id via `plugin_id_from_surface_id()`.
- `body` — included only when the serialized size is ≤ `SNAPSHOT_BODY_LIMIT` (16 KiB, mirrors `UiLimits::max_snapshot_body_bytes`). Bodies exceeding the cap are omitted; the metadata alone is sufficient for clients to fetch the body via replay/resync.

### Transport Rules

**Session-scoped effects**: Plugin effects belonging to a session flow through core event transport:

```
PluginRuntime → PluginResponse.effects → AppEvent::PluginUiEffect → CoreEvent::PluginUiEffect → subscribed clients
```

The bridge in `src/core/mod.rs` wraps `AppEvent::PluginUiEffect` into a `UiEffectEnvelope` before publishing as `CoreEvent::PluginUiEffect`.

**Local-only effects**: Effects produced by purely local TUI commands (e.g. `/plugins` listing) may use `UiEffectSource::Tui` and stay local unless they should be visible to other clients.

**Durable surfaces**: Panels and status items are durable enough to include in snapshots. Dialogs and toasts are transient — they are not persisted and are not included in reconnect snapshots.

**Ordering**: Effects from one plugin response preserve order via the monotonic `event_seq` counter in the event log. No separate sequence system is used.

### Multi-Client Behavior

When multiple frontends are connected:

- Session-scoped plugin effects are delivered to subscribers of that session.
- Durable state changes update snapshots for new/reconnected clients via `RequestSnapshot`.
- Local-only effects (e.g. from `UiEffectSource::Tui`) do not leak to remote clients.
- Automation clients with limited `ClientCapabilities` receive degraded text or have effects ignored safely.
- Unsupported clients never block on UI effects — validation/degradation is synchronous.

### Canonical Entry Point

`App::apply_plugin_ui_envelope(envelope)` in `src/tui/app/mod.rs` is the canonical entry point for all plugin UI effects regardless of transport:

1. Derives `source_plugin_id` from the envelope.
2. Runs session guard (drops effects for non-matching session).
3. Validates against `UiLimits::balanced()`.
4. Enforces surface-ownership rules.
5. Delegates to `App::apply_plugin_ui_effect(effect, plugin_id_opt)`.

`App::validate_plugin_ui_effects(effects)` is the batch validator used by lifecycle hooks and the event bridge.
## Identity-aware additive protocol

Protocol version 2 remains wire-compatible. `ProjectContextDto` and
`SessionBindingDto` expose stable project/workspace identities while legacy
directory fields remain explicitly compatibility data. `SessionCreate` and
`SessionCreateFromTemplate` accept optional canonical IDs; omitted IDs are
accepted only through deterministic lookup of an existing unique locator.
`ServerCapabilities.identity_aware_context` advertises support.
