# TUI Module

The `tui` module provides the terminal user interface using Ratatui.

## Purpose

Renders the interactive terminal UI: message display, prompt input, modal
dialogs, sidebar, status bar, completions, and remote TUI protocol. Routes
user input through `TuiCommand` dispatch to session/memory/research/plugin
backends via `CoreClient`.

## Where It Lives

`src/tui/` — ~14,358 lines in `app/mod.rs` alone.

## How It Works

### Core Integration

The TUI routes session, history, task, memory, and worktree actions through
`CoreClient` so the same logic can run in-process, over stdio, or over a
socket transport.

### Daemon event stream (socket mode)

`CoreClient::subscribe()` is the TUI's only view of daemon-pushed state. In
socket mode the agent loop runs inside the daemon, so the in-process
`GlobalEventBus` carries nothing: streaming text, turn completion, and turn
failure exist **only** on this stream.

`main.rs` subscribes once during startup (`App::set_core_event_rx`) and
`runtime/event_loop.rs` selects on it alongside the other inputs.
`App::apply_core_event` translates the daemon events into view state:

| `CoreEvent` | Effect |
|---|---|
| `TurnStarted` | finalize any streaming message |
| `TurnTextDelta` / `TurnReasoningDelta` | append to the visible transcript |
| `ToolStarted` / `ToolCompleted` | add / finalize a tool-call row |
| `TurnCompleted` | `Idle`, or `Error` when `stop_reason == "error"`; also applies `input_tokens`/`output_tokens` to the footer |
| `TurnFailed` | `Error` + the daemon message as a toast |
| `Error` | `Error` + `code: message` as a toast |
| `PermissionPending` / `QuestionPending` | open the matching dialog |
| `SessionUpdated` | apply the carried title directly; id-only events fall back to a bounded single-session metadata refresh |

Usage reaches the footer on `TurnCompleted`, not on
`AppEvent::AgentFinished`: that bus event is intentionally not mapped across
the daemon boundary (see `core::map_app_event_to_core_event`), so it is never
observed in the default socket-daemon mode. `TurnCompleted` carries optional
`input_tokens` / `output_tokens` / `cached_tokens` / `reasoning_tokens`, and
every field is `skip_serializing_if = "Option::is_none"`, so the wire payload
is unchanged when a provider reports no usage.

`TurnCompleted` also carries an optional `context_limit`: the window the agent
actually resolved for that turn (`ExecutionPolicy::context_window`, installed
into the context tracker by `set_execution_policy`). `AppEvent::ContextUpdated`
publishes the same number but is equally unmapped across the boundary, so the
field is what makes the footer's context percentage real in daemon mode.
`SessionState::context_limit` therefore starts at **0, not a default window**:
`0` means "unknown". Every reader treats that as unknown rather than
dividing — the footer prints `unknown` instead of a percentage
(`format_token_line`), the stats dialog prints `Context: unknown`, and the
timeline readout prints `unknown`. Seeding a fabricated 128k would display a
confident percentage against a window the selected model may not have. The
number is deliberately the one that gates compaction, so the meter cannot
disagree with the behaviour it describes.

`SessionUpdated` carries an optional title: auto-title events publish
`Some(title)` and the TUI applies it directly with no round trip.
Id-only events (`title: None` — agent/model selection, legacy publishers)
still require a re-read. That re-read is the `SessionRefreshed` command:
it is generation-guarded, carries the active tab's project key (an empty
id is never sent — the daemon rejects it with `project_context_required`),
updates only the title, and deliberately avoids `App::set_session` — that
would navigate and re-arm the event stream on a background metadata refresh.
It must write to both `dialog_state.session_dialog` and the focus-manager
instance, because `open_dialog(Dialog::Session)` pushes a clone and
`render_dialog` paints the clone.

Anything else returns `false` (no redraw) and is ignored, so a newer daemon
cannot break an older TUI. `TurnFailed` is the load-bearing arm: the TUI sets
`SessionStatus::Working` optimistically at submit time, so without a terminal
event a failed turn looks like an eternal "working" with no error anywhere.
There is no polling fallback — `SessionMessagesLoad` is user-triggered.

`remote_event_rx` (`handle_remote_event`, `RemoteTuiMessage`) is the separate
**server/WS** path and is `None` in local socket mode.

### Transport identity (`App::is_ws_attach_transport`)

`AppMode::RemoteCore` names **two different frontends**, and conflating them
causes silent data loss:

| Frontend | `remote_send_tx` | Submits via | Creates sessions locally |
|---|---|---|---|
| WebSocket attach (served over `/tui`) | `Some` | `remote_send_tx` | no — owns its own session lifecycle |
| Socket-daemon (plain `codegg`) | `None` | `core_client` | yes |

So `AppMode` alone must never gate the submit path. `is_ws_attach_transport()`
is the single predicate, and both `send_prompt` and the event loop's
`pending_send` block use it. Gating on the mode alone previously caused two
distinct bugs: prompts pushed into an absent channel (pinned on "working"
forever), and — after that was fixed — session creation disabled for the
socket path, so a **brand-new repo could never submit anything**: the
continuation was armed by `send_prompt` and never started by the event loop.

The same rule governs **every** `remote_send_tx` send, not just prompts.
`SubmitPermission` and `SubmitQuestionAnswers` had regressed to a mode-only
gate: on the socket path the answer was handed to `send_remote_message`, whose
`None` arm drops it after only a `tracing::warn!`. Because the dialog is torn
down on submit, the daemon-side waiter was then left parked on an **unbounded**
human wait with no dialog left to answer it — approving a tool silently hung the
turn forever. `CommandAction::Template` had the identical defect on the prompt
path. `scripts/check_tui_transport_authority.py` fails the build if a
`send_remote_message` call is ever gated on the mode again; it is part of
`verify.sh quick`.

### Project execution context

Project-scoped TUI actions resolve `ProjectExecutionContext` from the active
project tab before dispatch. The context carries the tab's project,
workspace, and session identities plus its canonical workspace-root locator,
and is moved into background tasks as an immutable value. TUI code must not
use `session_state.project_dir` or process `current_dir()` to choose a
project, and must never change process cwd. The launch directory is used only
once by the CLI/bootstrap composition path to create the initial compatibility
tab.

The local project picker flow (`WorkspaceRegister` -> `ProjectRegister`)
runs under the implicit/proven LocalOwner principal and continues to work
locally. Remote/team transports carrying a team principal receive the
canonical daemon denial (`authorization_scope_required`) as a bounded picker
error rather than a silent fallback to local path interpretation; no process
`cwd` inference is reintroduced.

The project picker uses the help-dialog layout: title, bounded filter/count
row with an `INS`/`NOR` badge, result rows, and a mode-specific footer. It
opens in the caller's input mode. In Normal mode, printable keys filter while
`j`/`k` navigate even with a query; `i` enters Insert so those letters can
be typed, and `Esc` steps Insert -> Normal -> clear filter -> close. The
picker remains capped and preserves its async generation guards. Active
projects sort before archived projects, with fuzzy score ordering retained
within each group. The status bar and sessions dialog show the active tab's
project name and workspace-root context, resolved from tab/catalog state.
They never derive a root from process cwd.

The command catalog follows the same boundary: built-ins/global definitions
remain shared, while project-local command files are loaded by the active
workspace root and refreshed on tab activation or explicit asset reload.

Command discovery is presentation-only. `CommandRegistry` retains the legacy
`CommandCategory` for compatibility, and adds canonical domain, scope, source,
and keyword metadata for the palette and help surfaces. The palette searches
names, aliases, descriptions, domains, and keywords with a bounded result
window; dynamic project/plugin entries include bounded source labels so
same-named commands are understandable. Metadata does not bypass command
dispatch, observer mode, daemon authorization, or permission gates.

Local transport selection is handled by `CoreRuntimeMode` (default
`DaemonClient`):

- `DaemonClient` (default) — connects to or auto-starts the user-scoped
  singleton daemon via the root compatibility API
  `connect_or_start_daemon` (`src/core/instance.rs`). Its connect/start and
  CoreFrame transport work is implemented in `codegg-client`; the TUI
  composition supplies its explicit `ClientKind::Tui` descriptor.
- `StandaloneInproc` — keeps the core in the same process via
  `InprocCoreClient`. Requires `--standalone`.
- `StandaloneStdio` — spawns `codegg core-stdio` via `StdioCoreClient`.
  Requires `--stdio`.

Legacy `--core-transport inproc|stdio` flags still parse but emit a
deprecation warning.

### Async Command Pattern

High-latency `TuiCommand` handlers use a spawn-and-complete pattern:

1. **Start**: `start_*` function performs immediate UI mutation, clones
   inputs, and spawns a Tokio task. Lifecycle-tracked work uses
   `spawn_registered_tui_task`; `spawn_tui_task` is for fire-and-forget.
2. **Complete**: The spawned task sends a typed completion `TuiCommand`
   back through the command channel.
3. **Apply**: The event loop receives the completion and applies results
   to UI state synchronously.

**Stale protection**: `AsyncUiRequestState` request IDs. Completions with
a stale or cancelled ID are silently ignored.

**Converted handlers**: `ReloadSessions`, `LoadSessionMessages`,
`OpenTreeDialog`, `PreviewImport`, `ConfirmImport`, `ResearchListRuns`,
`ResearchLoadRun`, `ResearchLoadSection`, `MemorySummary`, `MemorySearch`,
`MemoryRemember`, `MemoryForget`, `RunDoctor`, all session mutations,
goal operations, task operations, worktree list, template create,
notification send, plugin commands, project-catalog refresh pair, test
run, asset refresh, provider connection lifecycle, and session selection.

**Prompt/session continuation**: `App::send_prompt` inserts the user message
once and, when no session is attached, captures an immutable
`ProjectExecutionContext` plus `UiRouteToken` in `PromptState`. The event loop
only starts `commands::prompt::start_session_create_for_prompt`; it never
awaits `CoreClient::request`. The registered task returns
`TuiCommand::PromptSessionCreated`, whose apply handler validates the request,
tab/project/workspace/view/reconnect route, calls canonical `set_session`, and
submits the captured text once. A second Enter is coalesced. Create failures
restore the editable prompt and mark the next explicit retry so the visible
user message is not duplicated. Tab close/switch, reconnect, and shutdown
invalidate the frontend continuation without deleting a session that may
already have been committed by the daemon.

**File diff pipeline**: `FileDiffStatsReady` uses a separate
`spawn_sidebar_diff_stats()` in `src/tui/file_diff.rs`. Bounded by
semaphore (max 2 concurrent), 1 MiB size caps, binary detection, and
stale-generation protection.

See `src/tui/async_cmd.rs` for `spawn_tui_task` and
`spawn_registered_tui_task`.

### AsyncUiRequestState

`AsyncUiRequestState` (`src/tui/app/state/async_request.rs:20`) is a
reusable state machine for async dialog lifecycle:

```rust
pub struct AsyncUiRequestState {
    request_id: u64,        // Monotonically increasing generation counter
    loading: bool,          // Whether a request is currently in flight
    cancelled: bool,        // Whether the current request was cancelled
    last_error: Option<String>, // Last error message from a failed request
}
```

**Methods**: `begin() -> u64`, `cancel()`, `finish(request_id) -> bool`,
`fail(request_id, error) -> bool`, `is_current(request_id) -> bool`,
`clear_loading()`, `is_loading()`, `is_cancelled()`, `request_id()`,
`last_error()`.

**DialogState fields using AsyncUiRequestState**:
22 fields total; the exhaustive list is in the `DialogState` section below.

**Dialog close integration**: `close_dialog()` cancels async request
states for Import, ResearchBrowser, and Session dialogs.

**Completion semantics**: All async apply handlers follow:

```rust
if !app.dialog_state.<field>.finish(request_id) {
    return;  // stale or cancelled, ignore
}
```

Never mix `is_current()` + manual mutation; always use `finish`/`fail`.

### Background Task Lifecycle

TUI-owned background tasks tracked via `TuiTaskRegistry`
(`src/tui/task_lifecycle.rs:133`) on `App`.

**Key types**:
- `TuiTaskId(u64)` — monotonically increasing task ID
- `TuiTaskKind` — 13-variant category enum: `Command`, `FileDiff`, `Shell`,
  `Research`, `Memory`, `Notification`, `SecurityReview`, `Indexer`,
  `GitStatus`, `Workspace` (view-owned dashboard refresh/expand fetches; the
  only kind cancelled on Workspace close), `Editor`, `FileTree`, `Other`
- `TuiTaskRecord` — stores name, kind, started_at, abort_handle,
  completion flag

**Registry operations**: `spawn(kind, name, future)`, `cancel(id)`,
`cancel_kind(kind)`, `cancel_all()`, `reap_finished()`, `is_finished(id)`,
`active_count()`, `summary()`.

**Outcome accounting**: `completed_count` increments on `reap_finished`
only. `cancelled_count` increments on `cancel_kind`/`cancel`.
`panicked_count` is reserved (stays at 0 with abort-handle design).

**Integration**: `spawn_tui_task()` is fire-and-forget;
`spawn_registered_tui_task(tx, registry, kind, name, fut)` is tracked.

**Reaping**: Event loop calls `app.task_registry.reap_finished()` on
every iteration including idle.

**Shutdown**: `App::prepare_shutdown()` cancels all registered tasks and
kills shell handles.

Human-shell promotion UI state is only a projection of the authoritative
session owner. The TUI waits for the stage acknowledgement before presenting
`!!` or `/shell-include` evidence as staged, reloads pending records when a
session is attached, and sends stable promotion IDs with the next turn. The
daemon claims those IDs in the same SQLite transaction as the session turn
controller lease and durable turn-admission receipt. Turn retries reuse the
request envelope ID and receive the same receipt, never a second runtime turn.
After daemon restart, a committed but nonterminal receipt is marked interrupted
and is not replayed; the event projection reports the failed turn. While
pending, the TUI omits its display copy from submitted history because the
daemon appends the canonical payload; after acceptance, the projection remains
ordinary conversation history. A disconnected local owner or the WebSocket
prompt-only transport leaves evidence in the composer as an unsubmitted draft.

Finite `!`/`!!` execution is daemon-owned as well. The TUI negotiates the
typed human-shell protocol and adapts bounded output cursors into existing
`ShellEvent`/`ShellOutputStore` rendering. The daemon resolves workspace and
project/session bindings, checks `command.execute`, applies the daemon user's
sandbox profile, and shares scheduler process admission. The attaching TUI's
cwd and environment are not sent. Remote-node workspaces are refused. The
daemon retains handles/output ephemerally for ten minutes and expires commands
after five minutes; restart drops them without replay. PTY operations remain
under the separate `InteractiveProcess*` owner.

**Diagnostics**: `/tui-stats` includes task registry stats and shell
handle count.

### Cached Git Sidebar State

Sidebar git metadata computed in background, never on render frame.

**Storage**: `GitSidebarState` (`src/tui/app/state/session.rs:134`)
holds `root`, `branch`, `dirty`, `staged_count`, `unstaged_count`,
`untracked_count`, `conflicted_count`, `ahead`, `behind`,
`operation_state_label`, `available_actions`, `conflicted_paths`,
`last_refreshed`, `loading`, `error`, and `generation: u64`.

**Refresh pipeline**:
1. `start_refresh_git_sidebar(app)` resolves a **filesystem directory** —
   `session.directory`, else `App::active_workspace_root()` — bumps generation
   via `git_sidebar.begin_refresh()`, spawns a registered task.
   It must not use `session.project_id`: that is a stable string identity (a
   UUID), not a directory, so walking up from it could never find `.git` and
   the sidebar reported "not a git repo" inside a real repository.
2. Probe runs `egggit::status::repo_status` inside
   `tokio::time::timeout` (3s). A path that is not in a repository produces a
   named error (`no git repository at or above <path>`), never an empty one.
3. Probe posts `TuiCommand::GitSidebarRefreshFinished`.
4. `apply_git_sidebar_refresh` calls `git_sidebar.apply_refresh(...)` or
   `apply_refresh_error(...)`. Both return `false` for stale generations.

**Triggers**: `SelectSession`, `App::set_session`, session reload, and
`ensure_tui_cmd_channel` when a session was already bound.

**The channel must exist before the first trigger.** `set_session` ends with
`start_refresh_git_sidebar`, and `spawn_registered_tui_task` returns `None`
when `tui_cmd_tx` is `None` — the probe is dropped with no diagnostic. `main`
used to call `set_session` first and `ensure_tui_cmd_channel` afterwards, so
the startup probe never ran, `branch` stayed `None` for the life of the
process, and the sidebar read "not a git repo" inside a real repository.
`ensure_tui_cmd_channel` now re-issues the probe when it finds a session
already bound, making the ordering mistake self-correcting.

**Three distinct sidebar states, not two.** The widget renders
`checking git…` while `loading`, the actual probe failure when `error` is
`Some`, and `not a git repo` only after a clean probe that found no
repository. Collapsing every failure into "not a git repo" made a dropped,
slow, or timed-out probe indistinguishable from a genuine detection failure.

**Remote TUI**: `RemoteTuiStateSnapshot.git: Option<RemoteGitInfo>`
carries cached sidebar state to remote clients.

### Collaborator Presence (Presence M002)

Daemon-owned presence rendered as a bounded per-project projection.
The TUI owns no presence truth.

- **State**: `PresenceState` (`src/tui/app/state/presence.rs`) keyed by
  `project_id` with per-project `request_id` + global `reconnect_epoch`
  stale guards, LRU eviction at 16 projects, stable activity-rank + id
  ordering, and identical `Unavailable` rendering for unauthorized and
  feature-absent projects.
- **Fetch**: `src/tui/commands/presence.rs` negotiates
  `PresenceCapabilities` then `PresenceSnapshotGet` in one registered
  task. `PresenceHint { project_id }` (from
  `CoreEvent::PresenceUpdated`) only flags resync; the active project
  re-fetches, inactive tabs refresh on foreground.
- **Surface**: header shows `👥 N` only for authorized data (hidden
  otherwise); `/collaborators` (`/presence`, `refresh` subcommand) opens
  a scrollable `InfoType::Collaborators` dialog with coarse activity
  labels and empty/loading/error states. Focus follows the standard
  info-dialog convention and never mutates sessions. `/collaborators`
  is ephemeral presence only; durable membership administration lives
  under `/team` (next section).
- **Lifecycle**: tab switch refreshes the new active project; tab close
  drops unheld projects; `App::on_projection_reconnect` resyncs the
  active project. See `architecture/presence.md` for the full contract.

### Read-Only Session Observation (Presence M003)

One authorized session followed read-only through the existing
projection subscription path. The TUI owns no observation truth.

- **State**: `ObserverState` (`src/tui/app/state/observe.rs`) holds at
  most one `ObservedTarget` (project/session locators, observer-owned
  subscription id, authoritative cursor, watermarks) with per-observation
  `request_id` + global `reconnect_epoch` stale guards. Denied and
  unsupported targets render identically as unavailable; `stop`/`clear`
  drops cached state so hidden data is never rendered locally.
- **Flow**: `src/tui/commands/observe.rs` negotiates
  `ProjectionCapabilities`, then `ProjectionSubscribe` (session scope),
  `ProjectionResume` for reconnect/lag/resync, and
  `ProjectionUnsubscribe` on stop — all on registered
  `TuiTaskKind::Command` tasks with `TuiCommand::StartObserve` /
  `ObserveSubscribed` / `ObserveResumed` / `StopObserving` /
  `ObserveUnsubscribed` dispatch arms in
  `src/tui/runtime/command_dispatch.rs`. `/observe <session-id>`
  (`/watch`) resolves the project from the active tab; `/stop-observing`
  (`/unwatch`) tears down only the observer-owned subscription. The
  `/collaborators` panel lists session locators plus the bounded
  `/observe` hint (chooser-to-action seam; no chat storage).
- **Read-only enforcement** (central, fail-closed): `ObserverState::blocks_command`
  (narrow allowlist in `is_observer_allowed_command`; everything else
  denied) is checked at the top of `App::execute_command`;
  `blocks_prompt_submit` routes bare insert-mode input in
  `App::send_prompt` to project chat via the M002 collaboration seam
  (`route_observer_insert_to_chat`; only `Chat*` core requests, zero
  turn/permission control), with the collaboration placeholder as the
  no-project fallback; permission and
  question answers are rejected in `submit_permission_response`,
  `on_permission_confirm`, and `submit_question_answers`. The header
  renders `👁 OBSERVING <session> (read-only) · <state>` (locator +
  coarse status only, never content).
- **Reconnect**: `App::on_projection_reconnect` bumps the observer epoch
  and resumes from the authoritative cursor; revocation denies as
  `project_not_found` daemon-side. Full matrix in
  `architecture/presence.md`.

### Project Chat (Project Collaboration M002)

Project-scoped chat rendered from daemon `chat.v1` state; the TUI owns
no durable messages. Full contract in `architecture/collaboration.md`.

- **State**: `ChatState` (`src/tui/app/state/chat.rs`) keyed by
  `project_id` with per-project `request_id` + global `reconnect_epoch`
  stale guards, LRU eviction at 16 projects, a 100-message sliding
  window per active channel, per-project drafts, forward-only read
  markers with derived unread counts, and expiry-filtered composing
  leases. Unauthorized and feature-absent projects render the identical
  `Unavailable` panel and clear cached content.
- **Flow**: `src/tui/commands/chat.rs` negotiates `ChatCapabilities`
  then channel ensure / history / sync / send / edit / redact /
  read-marker / composing in registered `TuiTaskKind::Command` tasks
  with `TuiCommand::RefreshChat` / `ChatHistoryLoaded` /
  `ChatSyncLoaded` / `ChatMessageSent` / `ChatEditFinished` /
  `ChatRedactFinished` / `ChatReadMarkerSet` / `ChatComposingLoaded` /
  `ChatHint` dispatch arms. Live `ChatMessageCommitted/Edited/Redacted`
  payloads merge into the cached active channel; composing hints only
  re-fetch the snapshot while the panel is showing.
- **Surface**: header shows `💬 N` (plus unread) only for authorized
  data. `/chat` opens the interactive `ChatDialog`
  (`components/dialogs/chat.rs`) — a self-contained window, not the
  read-only `InfoType::ProjectChat` panel it replaces. It is two-modal,
  following the `Route::Workspace` normal-mode convention:
  - **Normal** — `j`/`k`/`↑`/`↓` move the message cursor, `g`/`G` and
    `Home`/`End` jump to first/last, `PgUp`/`PgDn` (and `Ctrl+U`/`Ctrl+D`)
    page; `i`/`a`/`Enter` open the composer, `r` replies to the selected
    message (prefills `@author `), `e` edits it, `X` redacts it (two
    presses — the first arms, the second confirms, because redaction is
    irreversible); `m` marks read, `u` loads older, `R` reloads the
    channel, `[`/`]` switch channels, `?` expands the key bar, `Esc`
    closes, `q` also closes. No unbound character falls through to the
    main input.
  - **Compose/Edit** — printable keys, `Backspace`/`Delete`, and
    `←`/`→`/`Home`/`End` edit the buffer; `Enter` submits, `Esc`
    cancels with the buffer intact. `Ctrl+C` closes from any mode.

  The commands remain the supported surface and are the way to compare
  behavior: `/chat-send`, `/chat-reply`, `/chat-history`, `/chat-sync`,
  `/chat-read`, `/chat-edit`, `/chat-redact`, `/chat-composing`, plus
  M003's `/chat-action-task`, `/chat-action-review`, `/chat-action-list`
  (structured actions only; free text never becomes an action). While
  observing, `/chat*` commands are allowlisted and bare insert-mode
  text routes to the observed project's chat; observer-target
  disconnect leaves chat usable. Action submits while observing go
  through the same daemon gate plus the semantic capability check.
- **Intent boundary**: the dialog owns presentation and local view state
  only — mode, message cursor, channel highlight, composer buffer. Every
  action is a `TuiMsg` intent (`ChatSend`/`ChatEdit`/`ChatRedact`/
  `ChatMarkRead`/`ChatLoadOlder`/`ChatRefresh`/`ChatSwitchChannel`) that
  `App::process_msg` routes into the same `start_chat_*` functions the
  commands call, so the daemon round-trip, the authorization decision,
  and the request-id/reconnect-epoch guards exist in exactly one place.
  The dialog never reaches `CoreClient` and never mutates chat state.
- **Authorship**: the window offers edit/redact on the selected row
  regardless of author. The TUI never learns its own principal (the
  daemon binds it per connection), so gating the affordance on a
  client-side guess would hide actions the daemon would allow and imply a
  check the client is not performing.
- **Buffer ownership**: the composer buffer lives in the window and is
  mirrored from the per-project draft on every snapshot. `apply_snapshot`
  never overwrites the buffer while the user is typing, so a live event
  landing mid-sentence cannot destroy it. Opening the composer does not
  clear it either — that is what makes a draft retained by a failed send
  recoverable from the one place the user can fix it. Cursor position is
  deliberately *not* in `ChatState`: it is meaningless once the window
  closes.
- **Paging**: `u` is gated on `ChatCapabilitiesDto::backward_paging`;
  against an older daemon it reports that plainly rather than issuing a
  request the daemon cannot decode. The initial load prefers the
  unanchored backward query, which returns the **newest** page — the
  forward `ChatHistory` with no cursor returns the *oldest* page, which
  made `/chat` open on ancient messages. Because the window retains only
  100 messages, paging backward trims the newest end; `R` re-fetches the
  live head.
- **Channels**: only the active channel's message window is retained —
  caching every channel would multiply the documented memory bound by the
  channel count. A switch therefore refetches rather than restoring from
  a local copy. The channel pointer moves *before* the fetch, so a
  message typed immediately after switching targets the channel the user
  is looking at; only daemon-listed channels are selectable.
- **Lifecycle**: tab switch refreshes the new active project when stale;
  `App::on_projection_reconnect` resumes the M001 cursor or resyncs the
  bounded window. Failed sends retain the editable draft with the typed
  error and fabricate nothing. Failed actions retain the typed error;
  the bounded action projection (`⚡ id kind job [status] title`)
  renders in the panel footer.

### Team Administration (Team Collaboration M003)

Durable project-team administration rendered from daemon `team.v1`
state; the TUI owns no membership truth. Full contract in
`architecture/identity.md`.

- **State**: `DialogState::team_request` (`AsyncUiRequestState`)
  guards every `/team` round-trip; completions additionally check the
  captured project against the active tab and the captured
  `reconnect_epoch`, so stale completions drop. No chat/team cache is
  introduced: views re-fetch through the authorized list paths.
- **Flow**: `src/tui/commands/team.rs` negotiates `TeamCapabilities`
  then membership list (+ `ChatPolicyGet` summary) in one registered
  task, with `TuiCommand::TeamMembershipLoaded` /
  `TeamPrincipalsLoaded` / `TeamTokensLoaded` / `TeamMutationFinished`
  / `TeamTokenCreated` dispatch arms in
  `src/tui/runtime/command_dispatch.rs`.
- **Surface**: bare `/team` opens a FocusManager-backed
  `InfoType::Team` dialog (member list with revisions + M002
  chat-policy summary). Mutations require the visible revision
  (`/team add <principal> <role>`, `/team role|suspend|reactivate|
  revoke <principal> <rev>`); stale revisions report
  `team_revision_conflict` with a reload hint and change nothing.
  Chat overrides ride the M002 operations
  (`/team chat-allow|chat-deny|chat-clear`, `/team chat-restrict|
  chat-inherit`, `/team chat`).
- **LocalOwner device flow**: `/team principals`,
  `/team principal-create`, `/team tokens`, `/team token-create`
  (credential rendered exactly once in the secret-safe
  `DeviceSecretDialog`, redacted `Debug`, close destroys the frontend
  copy), `/team token-revoke`. Ordinary team principals fail closed
  on these paths. `/collaborators` remains the ephemeral presence
  view and never shows durable membership detail.

### Shared-Session Control (Team Collaboration M004)

Turn-scoped controller lease rendered from daemon
`session_control.v1` state; the TUI owns no control truth. Full
contract in `architecture/authorization.md` (ADR-0007).

- **State**: `DialogState::control_request` (`AsyncUiRequestState`)
  guards every `/control` round-trip; completions additionally check
  the captured session against the active tab and the captured
  `reconnect_epoch`, so stale completions drop.
  `DialogState::control_last` caches the last-known lease for the
  active session (display only; refreshed via `/control`).
- **Flow**: `src/tui/commands/control.rs` issues `SessionControlGet`
  (lease plus inert requests), `SessionControlRequest` (inert),
  `SessionControlTransfer` (controller-only, CAS),
  `SessionControlRelease` (controller-only, CAS), and
  `SessionControlTakeover` (Maintainer/Owner, bounded reason) in
  registered tasks, with `TuiCommand::ControlLoaded` /
  `ControlMutationFinished` dispatch arms in
  `src/tui/runtime/command_dispatch.rs`. Mutations refresh the lease
  view so the indicator and revision stay coherent.
- **Surface**: bare `/control` opens a scrollable `InfoType::Control`
  dialog (controller principal, turn, revision, last action/actor/
  reason, plus bounded inert requests). Subcommands:
  `/control request [message]`, `/control transfer <principal-id>
  <revision> [--reason <text>]`, `/control release <revision>`,
  `/control takeover <revision> <reason>`. The status bar shows
  `control:<principal>` for the active session while a cached lease
  is held.
- **Observer enforcement**: `/control` (every subcommand, including
  read-only get) is absent from the observer allowlist, so the
  central `execute_command` gate rejects it while observing;
  permission/question answers stay blocked by `ObserverState`
  exactly as before. Controller identity remains visible through
  the session projection.

### Chat activity indicator, working strip, and todo strip

Strips bracket the chat viewport and are owned by `App`, not by the sidebar
widget.

**Spinner frames.** One table, one cadence, in `tui::components::spinner`.
`spinner_frame(elapsed)` is a pure function of elapsed time — `SPINNER_FRAMES`
indexed by `elapsed / SPINNER_FRAME_INTERVAL` (100ms) — so the animation is
frame-rate independent, reproducible under `TestBackend`, and immune to a
dropped render tick because there is no mutable counter to drift. It used to
index a ten-frame table by **whole elapsed seconds**, so a full rotation took
ten seconds while the 80ms pump repainted an unchanged glyph eleven times out
of twelve. A second, unrelated `SpinnerWidget` lived on `App` as `busy_spinner`
with a different glyph family and an 80ms cadence, rendering a duplicate
indicator into the status prefix; both are gone. Never add a second frame table
or a second cadence — one slow indicator and one fast one is how the fast one
stops being the visible one.

**Activity phase** (`ActivityPhase`). A closed enum, not a free label:
`Queued`, `WaitingForApproval`, `Thinking`, `Writing`, `RunningTool`,
`Retrying { attempt, total, class }`. It was a `String`,
which meant the describable states were only the ones somebody remembered to
pass in — and the two that matter most when a turn looks stuck were the missing
ones: a turn parked on a permission prompt, and a provider backing off between
retries. Both read as a silent screen. A closed set also means the renderer
cannot be handed a phase it does not know how to draw.

Every phase describes the **main agent's own stream**, and each is announced by
an event that has to be wired to it. A main agent **blocked on delegated work is
deliberately not a phase here**: it emits nothing while it waits, so an
event-set phase reads as whatever the last event said — or as nothing. There was
a `WaitingOnSubagent { name }` variant for exactly this that nothing ever
constructed, and row 0 said the literal word `idle` for the whole fan-out, which
is indistinguishable from a hung session. It is now **derived** at row 0 from
`App::awaiting_task_counts`, which counts live subagents and live jobs with the
*same* predicates the overlay renders from (`SubagentRow::is_running`,
`tasks::is_live_state`) so the two surfaces cannot disagree. Derivation is the
point: an event can be dropped, coalesced, or simply not wired, and the wait is
exactly the window in which that goes unnoticed.

**Both inputs to that count are projection-derived, so the daemon-attached TUI
depends on the projection actually reaching the controller.** It now does:
`ProjectionSubscribed`'s snapshot is installed rather than discarded, live
envelopes are consumed from `CoreEvent::ProjectionStreamEvent`, and
`active_snapshot()` does not require a project tab that this frontend never
switches. See `architecture/projection.md` "Daemon-attached seeding and live
delivery" — when that chain is broken every derived status reads `idle`, which is
indistinguishable from a healthy idle session.

`ui_state.subagents` is written from **two** render paths — the session panel
(so the sidebar list and the strip agree on one vector) and `render_working_strip`
itself. The strip's copy is what makes row 0 independent of the sidebar being
drawn: the strip is the main buffer's only status surface, so a layout that hides
the panel must not turn "waiting on 3 subagents" back into `idle`.

**Working strip** (`render_working_strip`). The main buffer's **single status
surface**, and row 0 is **reserved unconditionally** — it reads `idle` when
nothing is running and nothing is delegated, and `awaiting N task[s]` (with the
per-kind counts appended) when the main agent is blocked on subagents or jobs.
This height is subtracted from the transcript
(`session_layout` splits `prompt_height` against `Min(10)`), so gating row 0 on
"there is something to say" reflowed the transcript on every submit and again on
every completion. Reserving it permanently turns that off for good: the strip's
*content* can change freely — idle, awaiting, working, stopped, failed — without the
transcript viewport ever changing height. Subagent rows stay content-driven,
because N subagents genuinely need N rows.

Row 0 resolves in strict precedence order: a latched `last_error`, then
`stopped_turn`, then the live phase, then `idle`. The error outranks everything
because it is the one thing that explains why nothing is progressing, and by the
time it is readable the toast that also said so has expired. Stopped and error
both draw a **static** glyph — nothing is running, so an animated one would lie
about it. The trailing half names the key that changes the situation (`x to
stop`, `press x again to stop agent` while a confirmation is armed, `press x or
/start` once stopped, a running subagent count), because a long pause with no
way out is what reads as a dropped connection. The rows below it are clickable
and their hit rects are rebuilt every frame in `UiState::working_area` /
`SubagentState::hits`, so a click resolves against the geometry the user saw.

The `queued` suffix (M004) is appended in the same left-aligned `" · "` chain
as the subagent and job counts, and is text on the existing row — never an extra
row and never a condition on row 0's existence. It sits *after* the phase
selection so a latched `last_error` still outranks it: the failure is what
explains why nothing is progressing, and a waiting-prompt count is not.
`ClickTarget::Working` is hit-tested **before** `ClickTarget::Prompt` — the
strip is carved out of the prompt chunk, so the reverse order would resolve
every subagent click to click-to-focus.

**Side panels are mouse surfaces like the buffer.** Every occupied column
records its rect on each render (`App::panel_areas`), and
`ClickTarget::Panel { placement }` resolves a click inside one. A panel that is
not painted is not clickable, because the list is rebuilt from the same
`regions.occupied()` pass that drew them — a click can never name a column that
has moved or closed.

Three gestures, and each one has to be right:

- **Click focuses the column** — `TuiMsg::FocusPanelAt`, the pointer's answer to
  `Shift+H/J/K/L`, going through the same composer-mode park/restore so pointer
  and keyboard cannot leave the composer in different modes. Deliberately focus
  *only*: a click must not also fire a panel's row activation, which would make
  one press open something the user only pointed at.
- **Drag selects, and release copies** — panel columns join the viewport in the
  deferred-press arm of `on_mouse`, because they are the same *kind* of region
  (text being read, possibly wanted). The highlight sweep in `finish_render` and
  `selection::extract` are already region-agnostic, so this needed no new copy
  machinery. Excluding them is what made a docked terminal — the densest output
  in the app — look unselectable next to a buffer that was not.
- **Wheel scrolls that column**, via `App::scroll_panel`. Falling through to the
  transcript — which an unhandled target did — moved a buffer the user was not
  pointing at, so scrolling a terminal column appeared to scroll the
  conversation. Only the Terminal panel has its own scrollback today; for the
  rest this is a no-op rather than a fallback, because the wrong surface moving
  is worse than nothing moving.

`ClickTarget::Sidebar` is still resolved first, so the Session panel keeps its
own hover, collapse and scroll behaviour untouched. Rows are bounded by
`MAX_WORKING_STRIP_SUBAGENTS` and truncation is announced with a `… +N more`
row, which `working_strip_rows` reserves space for.

**Turn failures are latched, not toasted-and-forgotten.** `report_turn_error`
writes `SessionState::last_error` *and* raises the toast; row 0 shows the
latched text. It clears on exactly one event — `TurnStarted` — not on a timer,
so a user reading the reason is never interrupted mid-sentence and a user who
has resumed has already moved past it. Only genuine turn failures route through
it; shell staging, clipboard, and `/theme` errors are transient command feedback
and would otherwise report a turn failure that never happened. The full text
stays in the toast, and `/logs` retains 50 of them via `ToastManager::history`.

Rows come from `sync_subagents_from_snapshot`, which reads the same
`active_turn` (falling back to the newest `recent_turns`) agent tree the
sidebar renders. Every field on a row — including `run_id`, `display_name`,
`profile`, and `usefulness` — is copied **verbatim off the projected node**;
nothing is joined, looked up, or synthesized here. An earlier revision
described this as being "joined to durable runs by exact canonical task id only
to attach a `run_id`", and that join never worked: the node's `task_id` is a
daemon-local `u64` rendered in decimal while `AgentRunSummary::task_id` is a UUID
string, so the two could never match and `run_id` was permanently `None`. The
durable id now arrives inside `CoreEvent::SubagentStarted` and is carried through
the projection as a field.
A snapshot that *arrives* replaces the rows; **a missing snapshot does not**,
because absence is not evidence that no subagents exist and clearing on it
would blank the strip mid-turn.

**Descriptors are carried, never resolved.** `SubagentRow::profile` and
`usefulness` are the curated pool entry's one-line character description and
operational guidance. They are drawn once at spawn by `SubagentNameLease::entry`
— which resolves the entry behind **its own leased name**, so the descriptor is
by construction the one that was leased — and put on
`AppEvent::SubagentStarted`, from where the projection adapter copies them
verbatim. The reducer has no pool and cannot re-derive them; a re-drawn value
would make replay non-deterministic, the same failure `display_name` already had.

The TUI resolves nothing. `SubagentRow::descriptor()` returns only what arrived.
A display name is not a stable key: the registry appends numeric variants
(`Sally-2`), a name outside the pool must render *no* descriptor rather than a
guessed one, and a released name can be re-drawn to a different subagent while a
stale snapshot is still on screen. `scripts/check_tui_subagent_name_pool_isolation.py`
enforces this and carries a `--self-test` proving it fails on a planted
violation; run the guard, not the absence of a complaint.

**One row, five surfaces, and a deliberate trade.** The strip, the sidebar panel,
the `Ctrl+T` overlay, the `/sessions` dialog, and the subagent tab all render the
same derived vector. The four one-line surfaces show `SubagentRow::topic()` — the
descriptor when the projection carried one, the task description otherwise —
because a single terminal line cannot legibly carry two pieces of prose and
whichever is ellipsized second reads as noise. The sidebar additionally drops the
descriptor entirely below `MIN_SIDEBAR_DESCRIPTOR_CHARS`, since a three-character
stub reads as a rendering fault; a narrow column shows exactly what it showed
before. The **subagent tab is the full-fidelity surface**: it renders `Task` and
`Agent` side by side, wrapped and untruncated, which is what makes the
one-line trade lossless. A node recorded before descriptors existed has neither
field, so every surface falls back and the tab omits the `Agent` heading rather
than leaving one over an empty body.

**Subagent buffer** (`SubagentState::open`). Clicking a row replaces the
transcript in the viewport rect rather than opening a modal, so `Esc` has one
unambiguous meaning — return to the main buffer. The `Esc` arm sits below the
modal short-circuit (so `Esc` still closes a dialog in front of it) and above
every other handler (so nothing can swallow the key that dismisses it). The
view holds its own captured row, so a run dropping out of the next snapshot
does not yank the buffer out from under the reader.

**Todo strip** (`render_todo_strip`). Compact form is a single
`Todo list: 2/8 completed · Ctrl+T expand` row; `Ctrl+T` expands to a header
plus one status-bubbled row per task, bounded by `MAX_TODO_STRIP_TASKS`. Task
titles are ellipsized (display-width aware) rather than wrapped, so the strip
height is predictable. The strip's rows are added to the prompt allocation in
`session_layout` and then carved back off, so the input box keeps the height it
requested and the viewport absorbs the difference.

**Tasks overlay** (`Ctrl+T`, `state/tasks.rs`). One overlay over todos,
subagents, and jobs in that fixed order, each under its own header. A group
with nothing in it contributes **no header at all** — the alternative, an
empty "Jobs" heading above nothing, is what makes an overlay feel broken even
when it is accurate. Group order is a property of `build_task_rows`, not of the
order the sources happen to arrive in.

Filtering here is by *liveness*, and it is deliberately different from the
session panel's (§ "Keyboard sidebar and agent-tree inspection"). The overlay
answers "what is this turn waiting on", so finished subagents and completed
jobs are dropped; the panel is a persistent index, where a row disappearing
while you read it would be worse than a stale one. `is_live_state` is the
shared rule and it is asymmetric on purpose: an empty or unrecognised state
counts as **live**, because showing a job that turns out to be done costs a
glance while hiding a running one costs the fact the user asked for.

`bounded` applies one global row budget across all three groups rather than a
per-group cap, so a turn with sixty todos cannot starve the two subagent rows
that explain what the sixty todos are waiting on.

The overlay's keyboard path is gated on `InputMode::Normal` and sits **above**
the subagent-tab `Esc` arm, because the overlay is the topmost thing on screen
and `Esc` must dismiss what the user is looking at. The gate is load-bearing
rather than cosmetic: the user switches back to `INS` to keep typing with the
overlay open, and an ungated `j`/`k`/`Enter` there would break the composer.

`move_tasks_selection` starts at row 0 with no cursor yet **in both
directions**. Starting at the second row on `j` would silently swallow the top
item on an overlay that has only ever been opened once, and the user would
`Enter` the wrong subagent.

`open_selected_task` only succeeds for rows `activatable_target` recognises —
subagent rows. A todo or job row returns `false` and the overlay stays open,
rather than inventing a destination. On success the overlay **closes**: the
thing the user asked to look at now owns the main buffer, and a selection cursor
left behind it would be a control they are no longer looking at.

**An expanded overlay always claims at least one row.** `tasks_strip_height`
returns 0 only when collapsed; an expanded-but-empty overlay reserves the row
for `render_tasks_strip`'s `Nothing pending` line. Height, not content, decides
whether that line can paint — the render is handed `tasks_area`, and it returns
at `None` before reaching the empty branch. Returning 0 for "nothing to show"
therefore made the line unreachable *and* `Ctrl+T` a silently dead key on an
idle session, which is the first thing a user tries. The same trap applies to
any "draw an empty-state message" affordance: **reserve the row, then draw the
message into it.**

**Subagent tab set** (`Shift+N` / `Shift+P` / `Shift+X`,
`state/subagent_tabs.rs`). Three properties are load-bearing and each closes a
hole the obvious implementation leaves:

- **Identity is the projection's `task_id`**, not a fresh UUID. A subagent
  already carries a stable snapshot-derived identity; minting a second one
  would mean carrying a mapping between the two for no benefit. It is also why
  the module can promise nothing across a restart: `task_id` is a daemon-local
  `u64`, and the per-turn tree is cleared when the turn ends.
- **`active_tab_id == None` *is* the main conversation.** The direct analogue
  of `ProjectTabs::close_fallback_tab`: projects must always have some tab to
  render, but the main buffer already has a surface, so the fallback is to stop
  selecting a subagent. Closing the last tab needs no special case and cannot
  leave the renderer with nothing to draw.
- **`SubagentTabLiveness::Detached` is a state, not an error.** Because the
  tree is per-turn, a tab's subject can vanish while the tab is open.
  `is_running` refuses to report `true` for a detached tab, so the buffer can
  never keep claiming a run is live after the projection stopped vouching for
  it. A tab that outlives its subject is exactly the "panel outliving the
  tree" shape D3 asks to watch for, so it is modelled rather than papered over.

`MAX_OPEN_SUBAGENT_TABS` (16) **refuses** a new tab rather than evicting one.
Silently closing a tab the user is reading is a worse failure than declining
the open, and the caller then has a defined outcome to report — `App::
open_subagent_tab` toasts only when a row exists *and* the set is at capacity.
Note that `open_task` requires the row to still exist: a panel row alone cannot
open a tab, because a tab with nothing to render is not a tab.

### Addressing a reply to a running subagent

`/subagent-send` arms the composer to address one running subagent
(`state/subagent_addressee.rs`, `commands/subagent_steer.rs`). This is ADR-0041
**Option A**, and three properties are load-bearing:

- **The addressee is composer data, not a focus mode.** `InputTarget` stays
  `{Build, Chat}`; arming changes what Enter *means* on submit, never where
  keystrokes go. A variant here would have made the badge lie about who holds
  the keyboard.
- **The recipient is the controlling parent turn, not the subagent.** The
  request is `TurnSteer { session_id, turn_id }` where `turn_id` is
  `session_state.active_turn_id` — read at send time, never the subagent's
  `task_id`. `TurnSteer` is keyed on a *session turn*; naming a subagent's
  daemon-local `u64` would resolve to `turn_id_mismatch` every time. The
  subagent is named inside the payload as an addressee, and the parent decides
  whether to relay (ADR-0041, *Indirect*).
- **An armed composer cannot fall through to `TurnSubmit`.** The branch sits
  after the observer return and before `pending_send`, so an addressed reply
  that is refused reports in the steering vocabulary instead of silently
  starting a new turn with a message written for a subagent.

`SubagentAddressee` is invalidated three ways, all read from evidence rather
than guessed: `from_tab`/`from_row` refuse a finished or detached run at arm
time; `reconcile_addressee` clears it on **every** arriving snapshot when the
run has left the tree *or* is still present but no longer running (presence
alone is not liveness); and a `TurnStarted` drops it, because a subagent
dispatched by the previous turn is not one this turn's agent is running. The
single funnel is `App::apply_subagent_rows` — both `set_rows` call sites route
through it, so a second call site cannot forget the rule.

**A refused reply has no durable home, so the compositor restores it.**
`TuiCommand::SubagentSteerFinished` carries the user's own words back, and the
applier puts them into an *empty* composer. Unlike a queued prompt, a steer is
never stored: it goes to the loop's `pending_steer` and into the in-flight
provider transcript, nothing else. That is also why the TUI writes **no**
transcript message for an accepted reply — one would render as durable on
screen and vanish on reload. The toast is the record, and every sentence names
the parent as recipient: the vocabulary is `send_now_outcome_message` with
`SteerConsequence::ComposerOnly`, one function shared with M006's send-now
rather than a second table that could drift on the codes.

Addressing binds **no key**. A mode the user must be able to see, name, and undo
from the palette is a command; a chord would give it none of those. Arming
issues no request at all, so it cannot cancel, interrupt, or evict anything
(I5).

These keys name the buffer itself, adjacent to `Tab` for project tabs, but the
two axes never consult each other — subagents are inside one session, projects
are beside it. Reusing `Ctrl+Alt+N`/`Ctrl+Alt+B` would have collapsed two
independent navigations onto one gesture and made project-tab muscle memory
fire a subagent switch instead.

### Viewport click vs. drag

Inside the message viewport a press does both jobs and only the release
decides which one it was: `Down` arms a potential selection, `Drag` extends it,
and `Up` on the anchor cell is a *click* (it reaches `on_click`, whose viewport
arm expands reasoning or tool output) while a release elsewhere is a *drag*
that copies to the clipboard. Resolving on `Down` instead made every click a
selection and left click-to-expand unreachable. All other surfaces keep
immediate press behaviour via the shared `dispatch_click`.

### Pending approvals are a queue, and the queued row is the way back

A tool batch can raise several approvals at once. Exactly one may be on
screen; the rest live in `DialogState::permission_queue` (a
`VecDeque<PendingPermission>` of `perm_id` + `PermissionRequest`).

- `App::show_permission_dialog` **queues** when a dialog is already mounted
  and ignores a `perm_id` it is already tracking. It used to assign straight
  into `permission_dialog`, so each new request destroyed the previous one
  along with its `perm_id` — the daemon-side waiter stayed parked on a human
  answer with no UI pointing at it, and the tool later timed out denied for a
  reason the user never saw.
- Every answer path (`SubmitPermission` in `input.rs`,
  `submit_permission_response`, `on_permission_confirm`) clears the dialog
  fields **before** `close_dialog()`, then calls `promote_next_permission()`.
- `close_dialog`'s `Dialog::Permission` arm calls `requeue_open_permission()`:
  anything still mounted at close time was dismissed *without* being answered
  and goes back to the front of the queue. That arm did not exist, so a
  `CloseDialog` from anywhere popped the dialog while `permission_perm_id`
  stayed set — and the next approval then queued behind a dialog that no
  longer existed. The answer paths clearing state first is what keeps an
  answered request from being requeued.
- `mount_permission_dialog` recovers `args` from the transcript
  (`latest_pending_tool_arguments`) when the event carried none.
  `CoreEvent::PermissionPending` has no `args` field, so an approval that
  crossed the daemon used to render "No additional details" — you cannot
  meaningfully approve a command you cannot read.
- `sync_pending_permission_markers` pushes the set of tools with an open
  approval into `MessagesWidget::pending_permission_tools`. A `Pending` tool
  row in that set renders `queued — waiting on your approval · Enter to review`
  in `theme.warning` instead of the generic admission text, and the row is
  actionable: `App::open_pending_permission_for_message` runs from both the
  click path and the keyboard twin of click-to-expand. If another request owns
  the screen it moves to the queue front and warns rather than tearing the open
  dialog down unanswerably.

The marker is what keeps the affordance honest: without it the row would
advertise an action that may have nothing behind it.

### Reasoning must reach `add_reasoning`

Both transports must feed reasoning deltas to
`MessagesWidget::add_reasoning`, which opens a real `MsgPart::Reasoning`
block. `add_assistant_text` is a parser for *literal* `<think>` tags; a raw
delta carries no such tag and falls through to `MsgPart::Text`, so the model's
thinking renders as ordinary assistant prose with no `Thought for` header and
no way to collapse it. `apply_core_event`'s `TurnReasoningDelta` arm previously
did exactly that while the standalone `AppEvent::ReasoningDelta` arm did not.

### A refused submit is not a stall

`TurnFailed` covers a turn the daemon *accepted* and then failed. It does not
cover a turn the daemon never took. `send_prompt` is optimistic: it stamps
`Working`, appends the message to the transcript, and clears the composer
before the daemon has acknowledged anything, so **every** way acceptance can
fail has to undo that state explicitly. There are three such paths, and all
three used to be silent:

| Failure | Carrier | Before |
|---|---|---|
| `turn.submit` refused / socket dead | `TuiCommand::TurnSubmissionFinished { outcome }` | the arm handled `error.is_none()` and discarded `Some(_)`, recording it only in a `tracing::debug!` |
| No core client | `dispatch_turn_submit_request` | bare `return` |
| No session bound | `dispatch_turn_submit_request` | bare `return` |

The result was identical in all three: the composer kept its message, the
session stayed pinned on `Working`, and the user saw no output and no error
until `check_turn_stall` fired five minutes later with a generic "the core
produced no event" — a diagnosis that named none of the three actual causes.

`App::fail_unstarted_turn` is the release path for a submit that started no
turn: it clears `working_since` (so the watchdog stops counting a turn that
never started), drops `active_turn_id`, sets `SessionStatus::Error`, stops the
thinking indicator, finalizes streaming, **releases `pending_send` so a retry is
possible**, clears the prompt waiting flag, and latches the reason through
`report_turn_error`.

### Turn-contention refusal is not an unstarted turn

The daemon returns `turn_already_active` when a turn is already running — for
both the in-memory collision and a durable controller-lease conflict
(ADR-0007). Running every refusal through `fail_unstarted_turn` therefore made
the frontend assert the opposite of the truth at the exact moment a user was
waiting: it nulled `active_turn_id` and `working_since`, discarding the
identity and elapsed age of a **live** turn, stopped the thinking indicator,
finalized the activity stream of a turn that was still producing events, and
set `Error` on a session that was actively working. The queue milestone that
follows has to consult turn state to decide between queueing and steering, so a
path that invalidates that state is a correctness problem, not a cosmetic one.

`src/tui/app/turn_submit.rs` classifies the outcome **once**, at the only point
that knows how the daemon answered, and the typed value travels to the
completion arm as data — the arm never parses prose:

| Outcome | Meaning | Turn state | Prompt |
|---|---|---|---|
| `Accepted` | `Ack` or `TurnAdmissionReceipt` | untouched | consumed |
| `Contended` | `turn_already_active` | **untouched** — identity, age, status, spinner all kept | restored |
| `Rejected` | any other daemon refusal code | torn down, error latched | restored |
| `TransportFailure` | `CoreClient::request` returned `Err` | torn down, error latched | restored |
| `Unexpected` | a response shape this frontend cannot read | torn down, error latched | **not** restored |

Three of those decisions are deliberate:

- **Contention restores the prompt but does not latch.** Row 0 gives
  `last_error` absolute precedence (see "Working strip"), so latching here
  would replace the live turn's spinner and elapsed time with a stale `✗` line
  — the same state corruption in a different costume. Contention is not a turn
  failure: the turn continues, so there is no dead turn for a latched reason to
  explain. The refusal is reported as a warning toast, which is transient by
  design and is retained in `/logs`. `App::fail_contended_turn` owns this.
- **An unknown daemon code is `Rejected`, not `Unexpected`.** `Rejected` is the
  only class that keeps the user's text. The daemon adds refusal codes over
  time; routing an unrecognised one to a class that refuses to restore would
  lose the user's words the first time a new reason appeared. Contention is a
  deliberate exact-code allowlist (`CONTENTION_CODES`), never a default and
  never a resemblance match.
- **`Unexpected` does not restore the prompt.** An uninterpretable response may
  already have been accepted, so offering a resubmit is how one prompt becomes
  two turns.

**Every recoverable refusal restores the composed text through one shared
shape.** `PromptState::restore_prompt_with_stash` is that shape: it displaces
any newer draft into the bounded stash (`PROMPT_STASH_BOUND`), puts the refused
prompt back **whole** — the bound applies to how many drafts are kept, never to
the text, because truncating would change what the user wrote — and calls
`mark_retry_without_message` so resubmitting produces **one** transcript entry
rather than two. Session-create failure, reconnect, and refused submission all
route through it; the implementation was previously a literal `100` repeated in
three places, which made the bound three promises rather than one. The prompt
travels in `PendingTurnSubmission::prompt`, never across the command channel and
never re-read from the composer, which the user may have kept typing into.

The frontend never resubmits on the user's behalf. Handing the text back is the
whole recovery; anything more would be a queue nobody approved.

### The queue projection is pushed, not polled (M003, ADR-0038)

`SessionState::user_prompt_queue` is a **projection**, never an authoritative
copy: it is replaced wholesale and the TUI owns no queue state that could drift.
Before M003 it was read on exactly one edge — the session change
(`load_user_prompt_queue`, called from the same handler as the promotion
projection). A session that never changes re-read it never, which was harmless
while nothing could mutate the queue from the daemon side.

Delivery broke that. The daemon claims an entry and admits a turn with **no
reply channel**, so no client-initiated read can discover it: an entry the user
queued would sit displayed as `pending` while its turn ran, and nothing on
screen would ever say it was no longer waiting. `CoreEvent::UserPromptQueue` is
therefore pushed after every delivery attempt that changed the queue, and
`App::apply_core_event` adopts it through the **same**
`apply_user_prompt_queue_loaded` applier the read path uses — so a pushed
projection and a read projection cannot diverge in how they are filtered, the
superseded-session guard included.

Two properties the push depends on:

- The event carries the entry's **real** `status`, not a constant.
  `user_prompt_queue_entry_to_dto` hardcoded `"pending"` on the reasoning that
  every row in `list_live` was pending by construction — true until M003 gave
  `delivering` a producer. A `delivering` entry is visible and **not** editable,
  and a client can only grey that row if the status reaches it. The daemon
  refuses an edit to it with `user_prompt_queue_entry_delivering`, never
  `user_prompt_queue_expired`.
- `CoreEvent::UserPromptQueue` is classified `ClientLocal`, never `Safe`. It
  carries user prompt text, and `event_log.publish` feeds the projection sink as
  well as the frontend stream — marking it `Safe` would put a user's queued
  prompt into the durable projection store behind a check the
  `UserPromptQueueList` descriptor never ran.

### The `/queue` box (M004)

M003 made the projection exist. M004 is the first milestone a user can
actually operate: `/queue` mounts `components/dialogs/queue.rs`, a
FocusManager-owned list of the session's waiting prompts.

It is a **view**, and every deliberate omission is a refusal to imply authority
the TUI does not have:

- The widget **never sorts**. `SessionState::user_prompt_queue` is the daemon's
  order verbatim; a locally re-sorted list would claim an order the queue does
  not have. Selection follows `entry_id` across a fresh projection rather than
  the index, because a projection that lost a row would otherwise slide the
  cursor onto a prompt the user never looked at — and a `d` there would delete
  it.
- **A `delivering` row is listed and offered to `d`.** The daemon refuses it
  with `user_prompt_queue_entry_delivering`; the box does not pre-empt that with
  a local rule, because the refusal is the authority's answer to a real
  attempt. It reports it as *being delivered*, never as *expired* — a
  delivering entry has not expired, and telling the user otherwise is a lie they
  might act on.
- **A delete applies the projection that delete produced.**
  `queue_projection` re-reads after a successful write and returns it in the
  same response, so the frontend issues no second `UserPromptQueueList`. The
  rendered list therefore cannot disagree with the write the user just made.
- **The box advertises exactly the keys it implements.** M004 shipped `j/k`,
  `r`, `d`; M005 added `a`, `e`, and `[`/`]`; M006 added `s`. Naming a key this
  widget does not implement — in the hint *or* in the empty state — is the
  specific dishonesty the milestone plan forbids, and the test that enforces it
  has now rejected the box twice: once when M005 landed a verb the hint did not
  teach, and once when M006 did.
  - **M007 adds a second guard arm to the same test.** `HINT_SCHEDULE` states
    the duration grammar (`30s/5min/2h/1d`) *and* the empty case, because the
    box is the only place that grammar is written down — a user who must leave
    the box to learn `1h` has already lost the prompt they were composing. `Esc` is answered by `handle_key` but
  deliberately absent from the hint, for the same reason `TreeDialog` omits it:
  the modal host claims `Esc` as its universal exit before consulting the
  widget.

Two wiring facts that are easy to get wrong:

- `Dialog::Queue` has **no key arms** in `handle_dialog_key_legacy`, and must
  not get any. That router is `#[cfg(any())]`-disabled and `FocusManager` is
  the sole key authority, so such an arm would compile and do nothing.
  `Dialog::Queue` is also absent from `App::dock_target`, so the box is a modal
  and never docks even though it declares `docks_as_viewer()`.
- `close_dialog`'s `Dialog::Queue` arm cancels `DialogState::queue_delete_request`,
  `queue_add_request`, `queue_edit_request`, `queue_reorder_request`, and (M006)
  `queue_send_request`, so a mutation still in flight when the box closes cannot
  rewrite a projection nobody is looking at. Each slot is separate on purpose: a
  shared one would let a slow send-now be completed by — or overwrite — a later
  reorder. Close does **not** requeue anything: the box has no pending human
  decision, only a cursor.

`/queue` is **blocked for observers** by omission in the fail-closed
`is_observer_allowed_command` allowlist. No code registers it there, and
`tests/user_prompt_queue_box.rs` proves it by executing the predicate against
`/queue` and the M005 subcommand names, with `/stats` asserted allowed so the
suite cannot pass against an always-false stub.

#### Authoring: `a`, `e`, `[`/`]` (M005)

M005 makes the box both a navigator and a text field. That is the whole hazard
of the milestone, and the seam that manages it is
`QueueDialog::is_text_step()`.

**The guard is structural, not per-letter.** `handle_key` matches the text step
**first, as a separate match**, and returns from it. The browsing arm is then
unreachable while a composer is open, so a sixth action added later cannot be
live in a text step by forgetting one guard. The alternative — `if
!self.is_text_step()` on each action arm — is the shape that produces the
defect: typing `d` into a prompt deletes a prompt.

There is no shared `captures_text` / `wants_text` predicate on `App`. The only
precedent is `ConnectDialog::is_text_input_step`, one method on one dialog, and
`is_text_step()` is that method again. Opening an `App`-level predicate is an
explicit non-goal.

Text lives in the **widget**, not in `DialogState`. There is therefore no window
in which a later `/queue` can commit text an abandoned step left behind — the
buffer is dropped by the same unmount that drops the box.

- **`a` — add.** Composes text, commits on `Enter`. The `entry_id` is minted at
  **commit** (`start_queue_add`), not when the step opens, because
  `entry_id` is the protocol's at-most-once key: a retry carrying the same id
  converges on the same row instead of duplicating the user's sentence. An
  abandoned step therefore burns no id. The response's projection is applied
  directly — no second read — and the cursor follows the minted id.
- **`e` — edit in place.** Seeded with the selected entry's current text, so an
  edit starts from what the user can see on the row. It commits the entry's
  **existing** id: minting a new one would turn "correct this prompt" into "add
  a second prompt and orphan the first". Identity and ordinal are the daemon's
  to hold, and the box only renders its answer.
- **`[`/`]` — reorder earlier/later.** Not `Shift+K`/`Shift+J`: this box
  already uses `j`/`k` for movement, and a shifted variant of the movement key
  that does something *other* than move is a pairing people misfire.

**The permutation is built from `pending` rows only.** This is the milestone's
sharpest edge. The rendered projection contains `pending` **and** `delivering`
rows (`list_projected_in`, `LIVE_STATUS_PREDICATE`), but `reorder` validates
against `list_live_in`, whose `WHERE` is `OPERABLE_STATUS_PREDICATE`
(`status = 'pending'`), and `is_permutation` demands **exact length equality**.
A client that sends every id it rendered is refused with
`user_prompt_queue_order_mismatch` the moment one entry is mid-delivery.
`QueueDialog::pending_permutation` therefore permutes the operable subset and
never computes an ordinal — the daemon owns those.

**One local refusal, and it is the exception.** M004's rule is "let the daemon
answer a real attempt". Reorder cannot follow it: with a `delivering` selection
the permutation would be built from the pending subset, **succeed**, and reorder
nothing — a silent no-op. So `reorder_selected` refuses locally and names
which end it is at, via `TuiMsg::QueueReorderUnavailable`. There is no daemon
call that can express "this particular row is not yours to move".

**`order_mismatch` re-reads once and reports.** It never retries: the check is
exact-length set equality, so a blind retry against a stale projection turns one
refusal into a contention source. Re-reading surfaces the truth (usually another
frontend's mutation) and the user's next `[`/`]` is built from the corrected
order.

**Refusals keep their own sentence and the user's words.** A bounds refusal
(`entry_too_large`, `too_many_entries`, `invalid`) restores the composed text
into the composer, because the fix is to shorten it. An ownership, delivery-state
or expiry refusal does not — retrying cannot work there, and restoring text
would suggest it can. The `unowned` sentence says the repair is a separate
explicit operation outside this box and deliberately does **not** name a slash
command, because no command registers one.

**Composed text travels exactly as far as it must.** It rides in
`UserPromptQueueAddFinished`/`EditFinished` so a refusal can restore it, and
nowhere else: no toast, no hint, no `tracing` call. The dialog renders the
buffer bound by width with an announced truncation.

#### The duration step (M007, ADR-0039)

`a` and `e` are **two** steps, not one. `Enter` on the prompt text advances to a
duration field; `Enter` there commits. The third state — text, duration, and a
committed box — is what the user sees.

**A separate step, not an inline suffix.** The obvious cheaper design is a
delimiter in the text (`ship the docs | 1h`). It was rejected because prompt text
is user data the box must not reinterpret: any delimiter is a character the user
cannot type, so `ship the docs | 1h` and `ship the docs | 1h | check the 1h
build` are not distinguishable. A second step makes the due time a first-class,
editable field instead, and it is what lets `e` edit an existing schedule
rather than only new ones.

**The duration step is inside `is_text_step`, not beside it.** That is the whole
safety argument: it inherits every property that keeps a stray `d` from deleting
a prompt while someone is typing `2d`. Its `Esc` goes **back** to the text step
rather than cancelling, because the prompt is still in the buffer and making
`Esc` discard it would mean a mistyped `2d` costs the whole prompt.

**An unparseable duration is refused in the box, with no request.** The grammar
is a UI-level fact the box already owns; sending a string to the daemon to be
told "that is not a duration" spends a round trip to deliver a message the box
can give first. A bare number is refused rather than guessed. The refusal quotes
what the user typed — which is a duration token, never prompt text — and the
field keeps its contents so the fix is an edit.

**Rows are marked by character, not by colour.** `@2h` / `@30m` for waiting,
`@·` for due, blank for immediate. The `@·` state is the one that earns the
marker: "scheduled for 09:00" and "09:00 has passed and it still has not run"
look identical otherwise, and they are very different states for a user deciding
whether to press `s`.

**Remaining time rounds up, and rolls into the next unit.** `format_remaining`
never prints `60m`: an entry 59m59s out reads `1h`. Flooring would make a prompt
the user scheduled "for an hour" read back as "59m" and look like an off-by-one.

#### `s`: send now (M006, ADR-0036)

`s` is the only queue key that reaches **into a live turn** instead of writing to
the queue. Everything else in this box authors, reorders, or discards rows; `s`
hands one row to a turn that already exists.

**The request names the entry and nothing else.** No text, no turn id. The daemon
loads the stored prompt — ADR-0035's durable-entry rule is what makes that safe,
because the text cannot differ from what the user queued — and resolves the
active turn from its **own** runtime handle. A client therefore cannot submit
text no queue entry accounts for, and cannot steer a turn that ended a moment ago
by naming the id it saw.

**Four outcomes, not two, and that is the whole design.** The steer happens
*before* the settlement, so "the turn refused it" and "the turn took it but the
queue could not record that" are different states with different correct
copy. `UserPromptQueueSendNowResult` is `Accepted`, `AcceptedNotSettled`,
`Refused { code }`, `EntryNotFound`. Reporting the third state as an error would
tell the user to resend a prompt that has already run — the exact duplication
ADR-0036 forbids. Reporting it as success would hide that the row is still
`pending`.

**Six outcomes, six sentences, and no sentence promises more than acceptance.**
`s` is a *handover*: the prompt joins the running turn's message stream. Whether
the model reads it, believes it, or acts on it this turn is not knowable at the
moment of acceptance, so no copy says "the model will…", "forced", or
"guaranteed". A user told "sent" for a channel that was `Full` has been lied to;
a user told the same for a `Closed` channel cannot tell their prompt went
nowhere. `send_now_outcome_message` owns all of that mapping, and
`user_prompt_queue_the_send_now_sentences_are_all_different` asserts the six are
pairwise distinct and quote no prompt text.

**Two local refusals, and they are different kinds of fact.** `delivering` is a
fact about the *row* — it is already running as its own turn, and no retry
changes that — so the box refuses without a round trip, exactly as reorder does.
No-active-turn is a fact about the *box's own snapshot*
(`session_state.active_turn_id`), and it is only a fast path: a stale snapshot
sends a request the daemon can refuse truthfully, which is the safe direction.
The daemon remains the authority.

**Send-now is not a `QueueMutation`.** ADR-0037 deliberately excludes steering
from the durable owner binding, so gating `s` on `ControllerScope::QueueMutation`
would refuse the legitimate case of a principal steering its own turn in a session
it does not own. The dispatch arm is its own handler, gated by the turn
controller, with the queue-ownership question re-asked **inside the settlement
transaction** — where it answers a different question ("may this principal change
this queue?") about a different write.

**The projection rides along on every outcome, refusals included.** A refusal may
mean another frontend already moved or removed the row, and a box left showing it
is showing something the daemon no longer holds. This is the same
"apply the projection this write produced" rule M004 established for mutations,
extended to the case where the write did *not* succeed.

**No retry, ever.** Every refusal is a fact about the turn — full, closing,
unsupported, not yours — and retrying a request whose answer is "definitely no"
is how one refusal becomes a contention source.

#### What a released delivery does (C001)

`renumber_dense` used to displace **every row in the session, status-blind** and
then re-densify only the client's pending rows, so a `delivering` row was pushed
above the dense band and nothing ever wrote its ordinal back. M005 measured the
consequence and recorded it as accepted-and-documented: the row sorted **last**,
and a released delivery returned to `pending` at the back of the queue carrying
`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET` (1 000 000) instead of a dense ordinal.

**C001 fixed it.** `renumber_dense` now writes the whole **live** set, folding
the caller's desired pending order together with each non-pending live row held
at **the rank it already had**
(`merge_operable_into_live`, `crates/codegg-core/src/session/user_prompt_queue.rs`).
The displacement step is unchanged — terminal rows still hold ordinals under the
session-wide `UNIQUE`, so they still have to be pushed clear of the band — and
the client's permutation contract is unchanged: it still sends **pending ids
only**.

The box is unaffected and stays correct: it renders the daemon's ordinal verbatim
and never renumbers locally, which is why this was always a store defect and never
a TUI one. A released prompt now resumes where the user put it.

### Turn-submit staleness

`App::turn_submit_request` is an `AsyncUiRequestState`, and
`TuiCommand::TurnSubmissionFinished` carries its `generation` alongside the
daemon's `request_id`. These are different questions and stay separate:

- `request_id` is the **daemon's idempotency key**, derived from the submitted
  payload. An identical retry reuses it so daemon receipts prevent replay.
- `generation` is the **frontend's local staleness marker**.
  `apply_turn_submission_finished` calls `finish(generation)` first and drops the
  completion when it returns `false`.

That check was missing. The old arm cleared `pending_turn_submission` only when
the ids matched but then tore down turn state *unconditionally*, so a late
completion from a superseded submission could still null a live turn's identity.
`App::cancel_turn_submit` invalidates an in-flight submit on tab switch,
reconnect, and project change — the daemon turn is not undone, it keeps running
and its events still arrive, but the completion must not act on state the user
has moved past. On reconnect the submitted prompt is handed back alongside it:
the daemon may well have accepted the submit before the socket died, so the user
resubmits explicitly, and `retry_without_message` plus daemon-side turn
admission keep that from becoming a second transcript entry or a second turn.

The stall watchdog remains the backstop for the case where the daemon accepted
the turn and then went quiet — a different failure, and still measured by
silence.

### Turn-stall watchdog vs. long tool calls

`check_turn_stall` measures *silence*, and `bash` is silent by construction —
it delivers its output only on completion, so a multi-minute build emits no
events at all. `SessionState::running_tool` (set on `ToolStarted`, cleared on a
matching `ToolCompleted`, in both transports) lets the watchdog tell "working"
from "wedged": an in-flight call re-arms the timer instead of failing the
turn, up to `TURN_STALL_TOOL_GRACE`, which matches the shell timeout ceiling
so a runaway tool still trips the watchdog.

Todo updates reach the frontend as `CoreEvent::TodoListUpdated`. The bus event
that originates them (`AppEvent::TodoUpdated`) is published on the
process-local `GlobalEventBus`, which does not cross the daemon process
boundary — a connected TUI only ever saw whatever it happened to poll at
startup until this variant was added.

### Text wrapping

`ratatui::widgets::Paragraph::line_count` is behind the unstable
`rendered-line-info` feature, so wrapped scrolling cannot be delegated to the
widget. `src/tui/wrap.rs` owns the shared implementation: `wrap_to_strings` /
`wrap_count` for word-wrapping with hard-break for overlong tokens, and
`ellipsize_to_width` for single-row truncation. The chat viewport pre-wraps
message lines with it (the widget paints no `Paragraph::wrap`, so a line wider
than the viewport is clipped at the edge); the `/logs` dialog scrolls in
wrapped rows rather than logical lines.

Every tool/shell output path wraps to `width - TOOL_OUTPUT_INDENT`, including
the pre-numbered `read` path — that block carries no fence chrome, so its line
numbers are its own gutter and the wrap budget is `width - gutter`. A path that
skips the wrap is not merely clipped, it is *unreachable*: there is no
horizontal scrollbar and `scroll_left`/`scroll_right` are no-ops.

### Chat viewport scrolling

The row index (`src/tui/components/messages/layout.rs`) is the contract between
what is drawn and where you can scroll. `MessageLayoutCache` holds the rows the
renderer actually emitted and derives every offset from them, so `scroll` is a
true row index into the drawn buffer.

This is deliberately *not* an estimate. `MessagesWidget::estimate_msg_lines`
used to re-derive row counts by re-implementing the emit loops, and it drifted
on two tool-output paths — it counted wrapped rows for collapsed output while
the renderer wrapped the first N *logical* lines, and it counted wrapped rows
for `read` output while the renderer emitted one unwrapped row per line. Since
the disagreement is per message it accumulated down the buffer: the further back
you scrolled, the further the cached offset sat from the row actually drawn.
Symptomatically that is a transcript that clips off the edge and gets worse the
longer it gets, with most of the history unreachable.

Two rules follow:

- **There is one row producer.** `MessagesWidget::build_message_rows` is it.
  Anything that needs a row count builds rows; anything that draws rows uses
  the same call. Do not add a cheaper counter beside it.
- **Rebuilds are suffix-scoped.** `rows_dirty_from` is a watermark, and
  `MessageLayoutCache::truncate_from` drops only that suffix. Streaming appends
  to the last message, so a token costs one message rather than the whole
  transcript; `invalidate_rows_from` is the general form and
  `invalidate_tail_rows` its `len-1` shorthand. A suffix (rather than a set of
  dirty indices) is what lets the blank-collapse across a message boundary be
  recomputed correctly during a partial rebuild.
- **`invalidate_tail_rows` is only correct for a mutation that *is* the tail.**
  It used to be documented as "the streaming/tool-result path", and
  `update_tool_call` used it while searching every message for the matching tool
  id — which is the tell. A suffix rebuild is only valid when the write landed
  at or after the suffix start, and a tool result does not: `add_tool_call`
  pushes a **new message per call**, so when a turn issues several tool calls
  the earlier ones are not the tail by the time their results land. Those rows
  stayed cached with the running styling, so the row kept its "working" colour
  until the user clicked it — and clicking happened to invalidate the *correct*
  index via `invalidate_rows_from`, which is why a click appeared to fix the
  colour. **Any mutator that searches for its target must invalidate from the
  index it actually found**, the way `mark_tool_call_running` already did.

`messages.max_scroll()`, `scroll_position()`, `scrollbar_state()` and the render
all read that one index, so they cannot disagree. `MessageLayoutCache::new` and
`find_visible_range` no longer exist — the render slices rows directly instead
of rebuilding a window of neighbouring messages and converting a cache offset
into a position inside it.

### Long Output → Info Dialog

`App::show_short_or_info(info_type, lines)` routes output to short toast
(≤3 lines) or scrollable `InfoDialog`. Dialog reused if already open.

### Interactive Terminals (M003)

Reference TUI experience over the M002 attach/resume protocol. The
TUI owns no PTY/process state: `App::interactive_terminals`
(`src/tui/interactive_terminal.rs`) is a bounded projection of daemon
handles/output, and `src/tui/commands/interactive_terminal.rs` issues
every mutation as a daemon `CoreRequest::InteractiveProcess*` operation
through the TUI `CoreClient`.

**The transport boundary: interactive terminals are delivered on the
socket-daemon frontend, and refused by name everywhere else.** Every
interactive-process operation needs a `CoreClient`, and the `/tui`
WebSocket attach frontend never has one — `src/client/attach.rs` opens
that socket and speaks `protocol::tui::TuiMessage`, so no
`CoreFrame::InteractiveProcess*` can ever be sent from it. That
frontend's daemon is present and healthy; it simply does not carry
terminal traffic to that client, and a refusal must say so. All nine
entry points therefore classify the fault through one seam,
`terminal_core_client` → `TerminalRefusal`, rather than each asking
"is there a client?" and reporting a missing daemon:

| `TerminalRefusal` | Meaning | Message |
|---|---|---|
| `TransportUnsupported` | the `/tui` attach frontend, which carries no interactive-process traffic | names the transport, says it does not carry interactive terminals, points at `codegg` / `codegg daemon attach` on the daemon host |
| `NoDaemonConnection` | a terminal-capable frontend with no live connection | `Daemon connection unavailable; <operation>` — unchanged, because it is the truth there |

The discriminator is `App::is_ws_attach_transport()`, never the app mode:
the socket-daemon frontend is `RemoteCore` too and *does* own a
`CoreClient`, so a mode-only gate would misreport that frontend as
unsupported. `scripts/check_tui_transport_authority.py` enforces this.

This boundary is about the *frontend*. The `/core` WebSocket endpoint is
a separate surface that routes arbitrary `CoreRequest`s, interactive
processes included; its remote authorization posture is tracked by
`plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md`
and is not settled by anything here.

- **Slash commands**: `/terminal [handle]` (starts a shell when nothing live is
  behind it), `/terminal-create <cmd> [args...]`, `/terminal-list`,
  `/terminal-attach <handle>`, `/terminal-show <handle>`,
  `/terminal-focus [handle]`, `/terminal-send <handle> <text>`,
  `/terminal-resize <handle> <cols> <rows>`, `/terminal-resume [handle]`,
  `/terminal-detach [handle]`, `/terminal-terminate [handle]`,
  `/terminal-remove [handle]`, `/terminal-clear [handle]`.
- **Focus gate — one key per meaning.** `/terminal` opens the box **already in
  INS**, so the first character a user types reaches the process. This is the
  contract, and it is deliberately stateless:

  | Mode | Meaning | Keys |
  |---|---|---|
  | **INS** | the process owns the keyboard | every printable key, `Enter`, `Backspace`, `Tab`, arrows, `Ctrl-C`/`Ctrl-D` → PTY. `Esc` → NOR. `Ctrl+]` → forward a real `0x1b` and stay in INS |
  | **NOR** | the reader owns the keyboard | `hjkl`/arrows move, `Ctrl+u/d` page, `e`/`b` word, `v` chars, `x` lines, `d` delete, `space+y` yank, `C` clear, `D` diagnostics. `i`/`Enter` → INS. `Esc` closes with detach (or drops an armed selection first) |

  `Esc` used to be both a mode key and a pass-through byte, disambiguated by a
  hidden `pending_escape` latch: one `Esc` went to the program, two left. That
  made the destination of a keystroke depend on state the user could not see,
  which is the specific reason the surface read as unusable — there was no way
  to tell from what you pressed where your keys were going. The latch is gone;
  the capability it protected is not. A full-screen program that needs its own
  escape gets `Ctrl+]`, so `Esc` is free to mean exactly one thing on every
  keyboard.

  A prompt is never submitted from terminal focus (`classify_key`, via
  `InteractiveTerminalController::classify_focused`).
- **Cursor**: INS draws the *program's* cursor (`TerminalScreen::program_cursor`
  — where the child accepts the next character); NOR draws the *reader's*
  (`hjkl`/`v`/`x` move it). `styled_lines` paints the cursor cell even when it
  is blank, because a shell prompt's cursor sits on an empty cell by definition
  and trimming trailing blanks hid the caret exactly when the user was typing.
- **Caret blink (INS only).** The caret's visibility is
  `spinner::cursor_blink_visible(elapsed)` — a pure sibling of `spinner_frame`
  in the one frame table, half a second visible and half a second hidden. It is
  threaded in as a phase (`PanelCtx::caret_blink_on`,
  `render_view(.., caret_blink_on, ..)`) rather than counted on a view, so two
  renders at one instant agree, a coalesced tick cannot desync, and nothing
  survives an attachment change. **The hidden phase drops the `REVERSED`
  modifier and nothing else**: the cell stays painted, because a caret that
  stopped reserving its cell would let the line trim back to its last character
  and re-extend, and at the end of a prompt the whole line would blink away.
  **NOR never blinks.** There the cursor is a selection anchor — `x`, `v` and
  `d` all act from it — so it stays solid; a blinking anchor turns "which line
  am I about to delete" into a coin flip. This is a reading of the existing
  100ms frame clock, not a second ticker: the armed pull (below) is what repaints
  it, and no new interval, thread or `tokio::time::interval` exists for it.
- **Reader-cursor placement on `Esc`.** Leaving Insert is the one moment the
  reader's cursor is *placed* rather than kept, and it happens in
  `InteractiveTerminalController::escape` so both surfaces get it from one call:
  **the last row that has content, or row 0 when nothing has.** The rejected
  alternative was a fixed "one row above the bottom", which trades one wrong
  place for another — a shell whose prompt sits on row 0 of a 24-row screen
  would have the cursor parked in blank space *below* the only line on screen.
  Anchoring on content puts it on the prompt line after a short command and on
  the last line of the block after a long one, which is where the reader was
  either way. `set_cursor` is used, not `move_to`, so placement is bookkeeping
  and does not extend a `v` selection; an armed selection keeps its anchor,
  because Esc from Insert is not a gesture against it. The `Hidden → Viewing`
  sweep stays a separate pass and never touches `nav`: a hidden view becoming
  visible is not a gesture the reader made.
- **Bounds**: 64 KiB scrollback window per view (matches the M002
  default read), ≤16 views (matches the per-client attachment cap),
  32 KiB per input write, sizes 1..=1000 (all mirror M001/M002
  constants). Rapid input coalesces; rapid resize is last-wins.

**Liveness is a bounded pull on the existing event-loop cadence.** Neither
surface may show a terminal it cannot read: `service_terminal_output_pull`
runs from the pump tick that already services the editor's semantic refresh —
one cadence for the TUI, never a second timer for the terminal. It skips
unless a *mounted* surface holds a view with a live attachment and no read is
outstanding for that attachment, and it issues the byte-identical
`InteractiveProcessResume` with the daemon's own default byte cap and the view's
current cursor. An idle shell costs one bounded request per tick and no bytes,
because the ring answers `Resumed` with an empty chunk for an unchanged cursor.
There is no push channel: the daemon's only interactive event,
`CoreEvent::InteractiveProcessExited`, is bytes-free by contract, and output
never enters the bus, a projection, the transcript, or session observation.

**"On the existing cadence" is a claim about the *arming*, and the pull being
reachable is the thing to check.** `service_terminal_output_pull` has exactly
one call site: the timed arm of the loop's `select!`. That arm parks on
`futures_util::future::pending()` whenever `next_wake` is `None`, and `None`
does not mean "sleep a while" — it means the arm never runs again until some
other event wakes the loop. `next_wake` is built from
`runtime::event_loop::animation_interval`, which answers `Some` only for a
streaming turn, a working turn, a live toast, or — since C003 — a mounted
terminal surface. The first three are all transient: a turn finishes, its toast
expires, and the frame goes completely idle. A terminal that was never listed
there was therefore read only while a toast happened to be alive, a keystroke
arrived, or a resize was debouncing, so a shell's output became visible when
the user pressed the *next* key. The bytes were already in the daemon's ring the
whole time; nothing was fetching them.

`animation_interval` is a pure function of `&App` rather than an inline
expression for exactly this reason: before it was extracted, the only way to
observe the arming decision was to run a real terminal, which is how a surface
could be declared live while the arm carrying it was dark. Two rules keep the
contributor honest:

- **Mount, never read state.** `terminal_pull_armed` must not consult
  `begin_output_read`, `output_read_in_flight`, a recorded refusal, or
  `TerminalLinkState`. Gating the arm on any of those latches the loop off — the
  arm goes dark, so nothing wakes it to issue the read that would clear the
  condition, and the surface is dead until a keystroke arrives. Those checks
  belong to the pull, where refusing one pass is harmless.
- **Mounted means on screen.** `mounted_terminal_surface` is box first, then
  panel; a background view that is attached, alive, and on no surface is not
  read, because nothing it produces would reach a screen. A test pins the
  negative so "adopted" cannot silently widen into "mounted".

The cadence is `TERMINAL_PULL_RENDER_INTERVAL` (100ms), keyed on visibility
rather than focus. Focus is precisely what changes when nobody is looking at
the panel, so keying on it is what made the docked panel stop.

Three consequences are load-bearing rules rather than details:

- **A surface with the keyboard is not a viewer.** `H`/`J`/`K`/`L` and
  Shift+arrows are the panel-docking gesture, and every other surface gates
  that gesture on "nothing is being typed" — the session dialog on its filter
  being empty, the help panel on Normal mode, the panel host on Normal mode.
  The terminal box and the docked panel were the two that did not, so in INS
  an ordinary shell letter was read as "move the panel": the character never
  reached the process and the column moved out from under the user. The gate
  has to consult `TerminalFocus`, not `InputMode`, because a terminal in INS
  still reports the app's `Normal` mode.
- **Keystrokes are ordered, one operation at a time.** `begin_input` claims a
  single slot per attachment and the cadence drains pending input into it.
  Flushing per keystroke spawned one concurrent daemon request per character,
  and those requests raced, so fast typing could arrive scrambled — silent,
  character-level loss with no error to point at. The slot has one more
  obligation because of that ordering: a refusal must be classified **before**
  `begin_input`, never after. Claiming the slot and then discovering there is no
  client drops the bytes *and* leaves `input_in_flight` set with nothing in
  flight to release it, so each later keystroke parks behind a flag nobody will
  ever clear — one visible refusal followed by permanent silence. The same
  applies to the drained resize: a refused resize goes back on the coalescing
  queue rather than being consumed by a refusal.
- **Resize to the size you already are is a client-side no-op.**
  `queue_resize` drops an unchanged size, and the cadence drains what remains.
  A PTY resize flushes the tty's pending input, so a per-frame resize storm is
  indistinguishable from the shell eating the user's half-typed word.

**The badge reports the mode the user is in, not the mode the composer is
in.** `[NOR]` / `[INS]` above the main input is a statement about *where the
keyboard is*, and `App::ui_state.input_mode` is not that: `focus_panel` forces
it to `Normal` whenever a panel takes focus and `blur_panel` restores the
parked mode. A terminal holding a live PTY in Insert therefore reported `[NOR]`
for as long as the user typed into it, and the terminal panel's own Insert hint
branch was unreachable dead code, because that same field is what it branched
on — one field, read twice, both wrong.

`App::composer_input_mode` resolves it once. "Holds the keyboard" has two
halves and neither can answer the other — a box is not a side panel, and a panel
is not a dialog — so the accessor reads exactly the two facts key routing
already reads:

| Holder | Seam | Answers |
|---|---|---|
| terminal **box** | `App::terminal_box_owns_keyboard` | active dialog is `Terminal` **and** that handle is focused in the controller |
| docked **panel** | `commands::panels::panel_claims_insert_keys` → `PanelHost::claims_insert_keys` | the host is focused **and** the panel's `claims_insert_keys` holds |

Either one wins; otherwise `ui_state.input_mode` is the truth. That is the same
shape as `composer_destination_title` and as the editor route's
`buffer_focused(input_mode)` badge, so this is a third instance of one rule
rather than a new mechanism. `terminal_box_owns_keyboard` is the extracted form
of the gate the docking gesture in `on_key` already used inline — naming it is
what let the badge reuse it instead of restating it.

The accessor takes `&mut self` only because `panel_claims_insert_keys` has to
build a `PanelCtx`, whose grants are `&mut`; **it writes nothing.** The frame
must never be able to move the keyboard by describing it — a badge that forced
`input_mode` to match what it displayed would make the state it reported into
the state it caused.

**The composer's caret is derived from the same two facts, and there is no
`focused` flag.** `App::composer_holds_keyboard` answers "does the composer
hold the keyboard", and `PromptWidget` has no focus flag to disagree with it:
the widget's old `focused` field was **dead in production** — `PromptWidget::blur()`'s
only caller in the tree was a test — so "show no cursor while another surface
has the keyboard" was unimplementable as written, and a flag wired to a stored
value would have reintroduced exactly the drift the mode resolution exists to
kill.

Ownership is a **function of app state**, derived in the same shape as
`composer_input_mode` and read from the same seams. Its order mirrors
`App::on_key`'s priority chain — the first surface that would consume a key is
the surface that owns the keyboard:

| Order | Holder | Seam | Answers |
|---|---|---|---|
| 1 | **mounted modal** | `App::focus_manager` | a modal owns the keyboard outright; `on_key` short-circuits to `handle_dialog_key` |
| 2 | **focused side panel** | `App::panel_owns_keyboard` | the same condition `on_key` uses to consult the panel before the composer |
| 3 | **the timeline overlay** | `App::ui_state.timeline_visible` | the jump-to-message picker claims the keys before the composer sees them |
| 4 | **the editor's buffer** | `App::editor_buffer_owns_keyboard` | the editor route keeps the composer editable unless the document was explicitly focused |
| 5 | **otherwise** | — | the composer owns the keyboard |

`App::panel_owns_keyboard` is the extracted form of the `panel_active`
condition that used to be inline in `on_key`. Extracting it is what lets the
key chain and the caret rules read **one** derivation: a second copy of the
panel rule is where a drift would start. The terminal box needs no arm of its
own — it is a modal, so arm 1 already covers it. The overlays that claim only
*some* keys — the tasks list, the completion popup, the change review — are
deliberately absent: they let everything they do not claim fall through to the
composer, so the composer is still where the text goes.

The caret itself has exactly two shapes, and they are mutually exclusive by
construction:

| Mode | Caret | Source |
|---|---|---|
| **Insert** | blinking | the **terminal's own cursor**, positioned with `Frame::set_cursor_position` |
| **Normal** | solid block | painted by `PromptWidget` at the caret cell |

The blink is the terminal's, not the app's. The app runs **one** animation
cadence (the 100 ms working-strip spinner, one `ActivityPhase` enum), and a
second timer for a blinking caret is not allowed; ratatui hides the terminal
cursor whenever a frame did not position it, so "position nothing" is both the
correct and the sufficient way to show no terminal cursor. A terminal
configured not to blink therefore shows a steady Insert cursor — the
Insert/Normal distinction is enforced by **shape**, not by blink.

`PromptWidget::caret_cell` is the single computation of "where is the caret on
screen", used by both the painted block and the terminal-cursor positioning,
so the two cannot disagree about the caret's position. It returns `None` when
the caret is not on screen (its line scrolled off the top, or the area is too
small), and a `None` caret cell means "no terminal cursor", not a panic.

The rule the badge and the caret share, stated once: **when the composer holds
the keyboard, the badge says `[INS]` exactly when the terminal cursor is
positioned and `[NOR]` exactly when the block is painted; when the composer
does not hold the keyboard, neither caret is present at all.** The badge
reports the mode, and the caret reports the mode *and* the ownership, and both
read the same derivation.

**The composer owns a closed set of Normal-mode letters, and a claimed letter
never falls through.** `src/tui/composer_claims.rs` is the single, enumerable
statement of what the composer owns. The app's global binding map
(`src/tui/input.rs`) is consulted only for letters the composer does **not**
claim.

The rule that makes the table worth having is the **fallthrough rule**:

> A claimed letter is consumed by the composer even when the composer's verb
> is currently a no-op. It must not silently become an unrelated app action.

The interactive terminal panel already solved the same problem on a
non-document surface, and its dispatch comment records the failure mode
verbatim — *"a focused panel is consulted before Normal-mode actions, so an
unclaimed `d` would silently become `OpenDiff` and never delete anything."*
The panel's answer is to claim `d` unconditionally, even with an empty
selection. The composer takes the same answer, and takes it **before any verb
exists**.

Every verb in the table is **reserved** today, with one exception. M004
(motion), M005 (selection and clipboard) and M006 (undo, redo, insert
transitions) land the behaviour. A reserved claim is still consumed, so a
claimed letter cannot leak to an app-level action while its verb is pending.
That is why the table is established *before* the verbs: the leak is cheapest
to prevent now and most expensive to discover later.

**`i` is the exception, and it is live.** `i` is the app's **only** route back
to Insert mode: the transition is not a binding at all, it is the `None` arm
of the key chain, reached when a key resolves to no action. Claiming `i` and
leaving the verb reserved would consume the key, do nothing, and strand the
user in Normal mode with no way to type. The verb is therefore live, and it
is the app's **existing** gesture moved into the table — Helix `i` is
"insert before the caret", the caret stays where it is, and the next character
is typed at it, which is exactly what the gesture did. The table is the one
place that says who owns the key; the `None` arm keeps the gesture for the
surfaces the composer does not own (the editor route with its buffer
unfocused, a focused panel, the Workspace view's chat pane).

| Letter | Verb | Condition | Lands with |
|---|---|---|---|
| `b` | word back | always | M004 |
| `d` | delete selection | always | M005 |
| `e` | word end | always | M004 |
| `i` | insert before the caret | always | **live** — the app's existing transition |
| `j` | line down | **composer buffer has more than one line** | M004 |
| `k` | line up | **composer buffer has more than one line** | M004 |
| `o` | open line below | always | M006 |
| `p` | paste | always | M005 |
| `u` | undo | always | M006 |
| `U` | redo | always (both encodings) | M006 |
| `v` | select characters | always | M005 |
| `w` | word forward | always | M004 |
| `x` | select line | always | M005 |

Three letters are **not** in the table, and each absence is deliberate.
`space` keeps opening the help menu — ADR-0042's amendment puts the Helix
copy gesture in the box that chord opens, so the composer claims neither
`space` nor `y`, and one gesture has one owner. `g` keeps `GoToTop`; if a
later milestone claims it, that milestone owns the re-spelling. `h` and `l`
are not re-spelled: the existing actions already move the composer caret,
which is exactly Helix `h` and `l`.

`j` and `k` are the table's one **conditional** pair, and the condition is
written on the entry rather than inferred. They are claimed only when the
composer buffer has more than one line; on a single-line buffer they fall
through to transcript scroll unchanged, which is why their app-level binding
survives. A condition that is not written down is how a leak reappears: a
claim that quietly stops applying looks exactly like a claim that was never
made.

The claim step is consulted **before** the global binding map, in the
composer branch of the key chain, so it inherits the chain's priority: a
mounted modal, a focused side panel, the Workspace view and the editor route
are all consulted first. With a dialog open, a claimed letter reaches the
dialog. The Workspace view is excluded for the same reason it owns its own
Normal-mode keys: its dashboard filter is a different surface with a
different composer, and a claimed letter there would stop the filter
accepting it. The gate is the **resolved** mode and the derived ownership
(`composer_holds_keyboard`, `composer_input_mode`) — never the raw
`UiState::input_mode` field.

A claimed **capital** letter answers to both encodings. Terminals disagree
about whether a shifted key carries the `SHIFT` modifier, and the app already
binds some capitals twice for that reason (`insert_panel_direction_bindings`).
`U` follows the same convention, or the redo verb would work on one terminal
and silently do nothing on another. A lowercase claim is the bare key only:
the shifted spelling of a lowercase letter is a *different* character, and it
is not the composer's.

**Three app-level chords moved, and the capabilities did not.** The composer
claimed the letters `x`, `d` and `i`, so the actions that owned them moved to
chords that are not letters:

| Action | Was | Is |
|---|---|---|
| stop / resume the turn | `x` | `Ctrl+Alt+X` |
| open diff | `d` (vim map) | `Ctrl+Alt+D` |
| focus prompt | `i` (vim map) | `Ctrl+Alt+E` |

`Ctrl+Alt+<letter>` is the app's legacy-safe chord family: the terminal
delivers `ESC` + the control byte, which crossterm reports as the letter with
CONTROL|ALT, the same shape as the six chords that already exist. `Ctrl+Alt+I`
is **not** a usable spelling — the terminal delivers it as `ESC` + `0x09`,
which crossterm reports as `Tab` with ALT, so the verb would be unreachable on
every terminal; `Ctrl+Alt+E` is the deliverable alternative. Each chord is in
**both** maps, so the capability does not depend on which map is active, and
each chord's `(modifiers, key)` pair is not one the table claims, so the
composer cannot take it back.

**The default map's bare `/` stays.** The composer does not claim `/`, so that
spelling has no conflict with the claim table — ADR-0042's conflict table names
the *vim* map's `i` as the focus-prompt conflict, and the re-spelling exists
because the composer claims that letter. The plan's invariant is explicit: *a
letter the composer does not claim must resolve to the same app action it
resolves to today*, so `/` must keep resolving to `FocusPrompt`. It is also
load-bearing beyond the action: `/` is the gesture that **enters command
mode**, and command mode is what makes the rest of a typed slash command land
as text. Removing it took away "type `/panels` from Normal mode and press
Enter" — the only way back into the panel manager from a focused panel, which
owns every key by contract (`tests/tui_side_panels.rs`).

Two consequences of the move are recorded rather than hidden. `ToggleAgentRun`
left `is_normal_only`, because its only spelling is now a chord, and a chord
cannot be composed as text; the entry used to exist for the bare `x`. And the
guard's rule for a re-spelled action is "not a **claimed** key" rather than "not
a bare character": the three old letters — `x`, `d` and `i` — are all claimed,
so the claim rule already forbids their bare spellings, while a bare spelling
of a letter the composer does not claim is exactly what the invariant above
requires to keep working.

`scripts/check_tui_composer_key_authority.py` polices the table. It reads the
claim table and the two binding maps, and fails when a letter is claimed
twice, when a claimed letter can still reach an app-level action, when a
re-spelled chord is missing from a map, when a re-spelled action has no chord
to reach it by or still answers to a claimed key, when a capital claim misses
an encoding, and when a claimed key bound under `SHIFT` has no `is_normal_only`
entry. It carries a `--self-test` that injects each
violation class, and it is wired into `scripts/verify.sh quick` and CI.

The composer, the editor and the terminal panel now have three separate key
tables. They **share** selection semantics and geometry — `helix_selection.rs`
is one module — but they do **not** share key ownership. Each surface owns
its own keys, and each has its own guard: the editor's
`check_tui_editor_key_authority.py` and the composer's
`check_tui_composer_key_authority.py` are siblings, not one guard.

**The VT screen retains a bounded history, and the viewport is a controller
concern.** `TerminalScreen::new` passes `TERMINAL_SCROLLBACK_LINES` (5,000) to
`vt100::Parser::new`. A zero-length history is not a tuning choice: it destroys
every row the moment it scrolls off the top, which makes scrolling, selection,
and copying output impossible — the reader can only ever address the current
screen, which is precisely the text that is about to scroll away. Bounded rather
than unbounded because a terminal view is long-lived and daemon-backed.

`viewport_offset` (0 = following the tail) and the Helix cursor live on
`InteractiveTerminalView`, not on either surface, because box and panel share one
controller. Selection is an anchor/head pair in **viewport** coordinates rather
than absolute buffer coordinates: scrolling shifts the viewport by `delta`, so
stored points shift by the same `delta` through `SelectionState::translate`,
which keeps a selection on the same text while the bounded history evicts rows
underneath it. `sync_viewport` runs at the head of every read and clamps the
offset first — `Grid::visible_rows` computes `scrollback.len() - offset`
unguarded, so a stale offset is a panic rather than a clamp, and
`clear_scrollback` is exactly how one arises (it rebuilds the emulator, so it
also resets the cursor, offset, selection, and scrub overlay; a held selection
would otherwise silently re-point at whatever lands on those cells).

**The Helix selection rules live in one coordinate-agnostic module.**
`src/tui/helix_selection.rs` owns `SelectionState<P>` and nothing else: it does
not know what a point *is*. The terminal instantiates `P = (u16, u16)` viewport
cells; the editor would instantiate it with byte offsets, which is why the two
surfaces can share the rules without sharing coordinates. Four consequences are
load-bearing, and all four are rules rather than drawing details:

- **The state is a *set* with a distinguished primary.** Helix selection is
  several cursors, and a verb acts on all of them; `SelectionState` therefore
  holds `Vec<Selection<P>>` plus a `primary` index rather than
  `Option<Selection<P>>`. Roughly fifteen verbs (`C`, `;`, `S`, `%`, the regex
  surface) are not merely unbound without a set — they are *undefined*.
- **The single-selection API was kept, primary-scoped, rather than replaced.**
  `selection()`, `is_selected()`, `span()`, and `line_range()` still have their
  exact signatures and now answer for the primary. This is the load-bearing
  choice: it is what lets the terminal panel and the editor keep running one
  implementation of `v`/`x`/motion instead of forking one per surface, and it
  is why `SelectionState` lost `Copy` (a `Vec` is heap-allocated) without
  costing any caller. A caller that wants the whole set must say so with
  `selections()`.
- **Members are ordered by `bounds()`, and the primary travels with its
  member.** `normalize_set` is the single place that sorts and re-clamps the
  primary index, shared by the live rules and by a surface restoring a set from
  storage or from an undo entry. Sorting by bounds rather than by insertion is
  what makes "members are in document order" true, and carrying the primary
  rather than pinning its index is what stops a sort from silently redefining
  which member is primary.
- **`v` is a mode, `x` is an action.** Character selection stores a head that
  follows the cursor, so a bare movement extends it. Line selection pins its
  head at arm time, so a bare movement only relocates the cursor and disarms
  growth. That is what lets "press `x` again for the next line" and "press `x`
  on the line I moved to" be the same key — the `growing` flag distinguishes
  them, and comparing the cursor to the anchor cannot, because after one growth
  the cursor is legitimately no longer on the anchor.
- **The head is stored, not read from the cursor.** Storing it is what lets
  line mode refuse to follow the cursor at all.
- **Re-pinning after a scroll is a separate verb.** `translate` moves the anchor
  and head with the content, then `repin_cursor` puts the cursor back on the
  edge it walked off — following the head only in character mode, or walking
  past the top edge would quietly select rows the reader never crossed.

**Set-wide verbs in the editor, and why they are one transaction.**
`selection_ranges()` resolves every member to a byte range and then *merges*
them. Merging is a correctness requirement rather than a tidy-up:
`codegg_document`'s transaction validator rejects overlapping edits outright
(`OverlappingEdits`), and `invert` only produces an exact inverse for edits
that ascend and never overlap — so two cursors on the same word must collapse
to one range before submission or the controller refuses the whole edit.
`delete_ranges` returns a `RangeSetEdit` rather than `Option<EditorEdit>` so
that "nothing to do" and "too many selections for one transaction" stay
distinguishable: the second must be a visible refusal, because a *truncated*
multi-cursor delete is exactly the state neither `u` nor redo can describe.
The cap is `MAX_DOCUMENT_EDITS` (256) and it refuses rather than truncates.

**An accepted edit consumes the selection set; undo restores it.** This is one
rule, in the `submit_edit` funnel, rather than the previous rule where only
`r`/`R`/`~`/`` ` ``/`p`/`P` cleared the selection and every other verb left a
stale highlight over text that had moved. `EditorUndoEntry` carries
`selections_before` beside `cursor_before` for the same reason it carries the
cursor. For the same reason `apply_helix_verb` does **not** clear the selection
when it reports a delete or yank effect: the geometry layer reports, the App
layer consumes, and that is what lets the undo entry record the members the
reader actually had.

**Read-only refusal is narrow on purpose.** `apply_helix` gates `v` and `x` —
the verbs that *ask* for growth — and reports the refusal with the existing
`EditorNotice`. It deliberately does **not** gate `Moved`: extending a character
selection is a side effect of navigation, and refusing it would take movement
away from a reader looking at a file they cannot edit. Before this, `v` and `x`
armed and grew selections on a read-only buffer in silence.

**Terminal-panel parity is preserved by construction, not by exemption.** The
panel holds `SelectionState<TerminalPoint>` and never more than one member, so
`move_to` over a one-member set is identical to `move_to` over an `Option`, and
its ~25 call sites keep compiling and keep meaning what they meant. The
evidence is that the panel's selection tests pass unmodified.

**Selection and navigation are claimed by the panel before the global map.**
`classify_nav_key` answers `j/k/h/l` and arrows, `Ctrl+u`/`Ctrl+d`, `e`/`b`,
`v`, `x`, `d`, `space+y`, `C`, and `D`. `d` and `C` are deliberately in that table
rather than left to the global bindings: a focused panel is consulted *before*
Normal-mode actions, so an unclaimed `d` would fall through to `OpenDiff`. `d`
is the **delete operator** and `C` took over the clear it used to be, because a
key that means "clear the whole view" and a key that means "remove what I
selected" cannot be the same key. A bare `d` with nothing selected is a no-op
that is still *consumed*, for exactly the precedence reason above. `C` is bound
with and without `SHIFT` because terminals disagree on the encoding, and `D`
follows the same four-spelling rule for the same reason. Paging
matches the global orientation — `Ctrl+u` toward older output, `Ctrl+d` toward
newer. `space+y` is state, not a chord: legacy xterm encoding delivers `Space`
and `y` as separate events, so `prefix_pending` remembers the first key and any
second key other than `y` disarms it and is then handled normally.

#### The selection-manipulation verbs (M008)

Eighteen verbs resolve over a **set**. They split into three groups, and the
split is what keeps them from growing into one large switch:

| Group | Verbs | Lives in |
|---|---|---|
| Set algebra | `;`, `Alt-;`, `Alt-:`, `,`, `Alt-,`, `(`/`)` | `helix_selection` |
| Ad-hoc construction | `C`, `Alt-C`, `%` | `editor.rs`, needs line geometry |
| Content mutation | `&`, `Alt-(`/`Alt-)`, `S`, `K`, `Alt-K` | `editor.rs`, needs bytes |

**The split is by what a verb needs to know, not by convenience.** Set algebra
is pure: it reads points and answers `bool`. That is why those eight methods
live in `helix_selection` and work unchanged on the terminal panel's
`(row, col)` points. Everything that needs to resolve "which byte is the next
line", "how wide is this on screen" or "does this text match" is geometry or
content, and stays on the surface — which is the same reason the module has no
opinion about coordinates.

`merge_consecutive` is the interesting case: it is a *set rule* (group adjacent
members, keep the rest) that needs a *geometry* answer (are these two
adjacent?). It takes an `adjacent` closure and stays pure. `merge_all` needs no
such help because every member of the set is adjacent to the span by
definition.

**Every set verb reports whether it changed anything**, and the answer is
load-bearing. A verb that changed nothing returns `false` / `NothingToDo` and
the App layer shows it once; it never submits an edit. That is the M004
`case_range` rule applied to the set, and it is why `,` on one selection, `;`
on nothing, and `Alt-:` on an already-forward set are all *reported* rather
than rewritten. `EditorNotice::Note` exists for this: these are answers, not
errors, and rendering them in error wording would tell a reader their buffer is
broken when it is fine.

**Rotate-focus and rotate-contents are the easiest pair in the keymap to
confuse, so they are separate types rather than one verb with a flag.**
`RotateSelections { member_delta }` moves `primary` and the cursor, changes no
text, and pushes no undo entry. `RotateSelectionContents { member_delta }`
rewrites each member's bytes — one transaction of `n` *replace* edits over the
original coordinates, so no intermediate text is observable — and goes through
`submit_edit` like every other edit.

**`merge_all` bridges gaps on purpose.** The plan's wording ("merge every
member into the primary's range") admits a second reading that would fold the
others into the primary's own range and silently discard them. Merging the
*span* is both Helix's behaviour and the only reading that does not lose text.
`merge_all` also takes the coarser mode if **any** member was a line selection,
because a merged character selection would leave the rows a line member covered
unrepresented.

**`&` aligns in display columns, and says so when it cannot.** Width is measured
through `bounded_display_width`, so a member wider than the 32 KiB per-line
budget costs the budget rather than the line — and the truncation is disclosed
through `RangeSetEdit::Edit(_, Option<String>)` rather than papered over, because
a member padded to a width measured over a prefix of it is genuinely
misaligned. Padding is appended after each member, which equalises widths and
lines up what follows; it is a display-column decision and `EditorPoint` ordering
is by byte, so tabs and CJK align on screen, not in the source.

**Bounded work is a per-verb budget, not a global one.** `regex` gets a
compile-time `size_limit` (`MAX_REGEX_PROGRAM_BYTES`) because a pathological
pattern is the documented way to turn one keystroke into a hang, and refusing it
at compile time is the only point where it can be refused. Each member's match
window is capped at the line budget, the whole set at `MAX_SET_VERB_READ_BYTES`,
and rotating contents refuses above that rather than allocating the document.

**The regex verbs reuse the `:` command line rather than building a prompt.**
`S`, `K` and `Alt-K` open `open_command_line("split-regex ")`. A prompt would
have needed a new input mode, a new pending field, and a new disarm contract on
buffer switch, reload and focus change; the `:` line already has all three. The
consequence that matters is that **M008 added no `TuiDocumentPresentation` field
whose name contains `prefix`**, so `scripts/check_tui_editor_prefix_state.py`
still sees exactly the one field it saw before — M002's central claim is
untouched rather than weakened.

**`HelixVerb` is no longer `Copy`.** Three variants carry a `String` pattern, and
a verb that owns text is not a unit value. Every call site constructed one and
passed it straight through, so the boundary moved rather than being papered
over with a `Copy` impl that only held for the other fifteen variants.

**Terminal-panel parity: recorded exemption on `C`.** M007 preserved parity by
construction because it added no chords. M008 adds eighteen, and the panel has
its own Normal-mode table (`classify_nav_key`, consulted *before* Normal-mode
actions), so this is the first milestone in the roadmap where parity can break
at all. Checked against that table, **exactly one** of the eighteen chords
diverges:

- **`C` — recorded, not fixed.** In the panel `C` is `TerminalNavAction::Clear`
  (clear scrollback and screen); in the editor it adds a cursor on the next
  line. They are different objects, and the panel has no document to put a
  cursor in, so the divergence is a consequence of the surfaces having
  different subjects rather than a choice either surface could make. The panel
  also claims lowercase `c` (all four spellings), which the editor leaves to the
  composer.
- **The other seventeen are unclaimed by the panel and behave safely.**
  `apply_nav` returns `NavOutcome::Unhandled` for them and `handle_key` maps
  that to `PanelKey::Consumed`, so they are swallowed rather than forwarded as
  PTY input or falling through to the composer. That is the right failure: a
  selection verb pressed with the terminal focused does nothing visibly wrong.

The panel gains none of the eighteen. Its `SelectionState<TerminalPoint>` is
still single-member, and every new set method is additive — no existing panel
call site changed, which is why its selection tests pass unmodified.

#### The unimpaired grammar (Helix-parity M006)

Naming note: `M006` here is the **tui-editor-helix-parity** milestone, not the
TUI's own `M006-A`…`M006-F` durable-agent-run series elsewhere in this document.

Four chords, and the milestone is smaller than it looks because three of the
things it needed already existed. `[` and `]` had been armed-but-empty since
M002; M006 gave them `OPEN_BRACKET_LEAVES` and `CLOSE_BRACKET_LEAVES` and
nothing else about the arming changed.

**`PrefixAction` grew an `Edit` arm, and that is the load-bearing design
decision.** `]Space` inserts a newline, and `editor_motion` is the one function
in the dispatch whose contract is that it never touches text. Putting it in the
`Motion` arm would have been a three-line change and a broken promise; a third
variant makes the leaf say what kind of thing it is. The arm calls
`editor_editing_command`, so `]Space` lands in the same read-only gate, the same
undo entry and the same document transaction as `x`, `d` and `y`. It
deliberately does **not** call `note_motion` — see the repeat-slot rule below.

**`resolve_pending_prefix` running first is the whole precedence argument.**
`Space` arms a prefix of its own, so if the bare-key arms were consulted before
the leaf lookup, `]` then `Space` would leave a `space` armed. It does not,
because `handle_normal_key` resolves the pending prefix before it matches
anything else — so `]Space` is a newline by construction rather than by a guard
added later. The M002 test that a *mismatched* key still reaches the composer
passes **unmodified**: adding leaves narrows what is bound, not what is
escaped.

**`AddNewlineBelow`/`AddNewlineAbove` are separate variants from
`OpenLineBelow`/`OpenLineAbove`, and Helix keeps them apart.** `add_newline_impl`
inserts a line ending at `line_to_char(end + 1)` and moves no selection;
`open_below` splits the terminator, puts the cursor on the new line and hands
the keyboard to the document. Collapsing them would make `]Space` silently
enter Insert. What they share is the transaction, so they share one
implementation — `stay_insert` is the only difference and it belongs to the
caller that changes mode.

**`o` and `O` reach that implementation through the shared edit funnel.**
`editor_open_line` is not a second funnel: it shares `editor_editing_command`'s
read-only gate, its snapshot and its `resolve_edit`, and differs only in what
it returns. It returns `BufferKey::EnterInsert` when the controller **accepted**
the transaction, and `BufferKey::Consumed` when the attachment refused — a
verb that writes claims its key and reports the refusal, exactly as `d`/`D`/`x`
do, while `i`/`a`/`I`/`A` are mode transitions that leave the key unclaimed
and let the status row's "read-only" be the report. The acceptance check is
what keeps the presentation's mode and the App's `input_mode` in agreement:
`submit_edit` writes the presentation's side of it inside the accepted branch,
and the caller returns the value the App reads.

**The open-line cursor is the start of the opened line, and the two cases
reach it by different arithmetic.** With a terminator to split, the insertion
is one newline at the start of the *next* line and that newline is the one
that ends the opened line, so the opened line starts where the insertion did.
With no terminator to split, the insertion is a full blank line — two
newlines — and the first ends the current line while the second ends the
opened one, so the opened line starts one byte further on. Getting this wrong
is invisible until the reader types: the cursor lands on the line *after* the
opened one, and the text they type goes there.

**A paragraph motion is resolved in `apply_motion`, not `apply_motion_inner`,
for the same reason a find is.** `]p` at the last paragraph is a no-op that has
to be able to say so, and "the cursor did not move" is not an answer. So it is
intercepted next to the find interception, the scan runs **once**, and
`ParagraphNote` is written next to the resolution that produced it rather than
in the dispatch layer that pressed the key. The command layer lifts it into
`state.notice` and clears it in the same step, so a disclosure can only ever
describe the keystroke that just happened.

**`ParagraphOutcome` has three variants because a bounded scan has a third
outcome.** `Bounded` is not `NoParagraph`: reporting "there is none" about a
document that has one is the worst of the three answers, because it is the one
a reader has no way to distrust.

**Boundedness is a total budget, not a per-line one.** `bounded_prefix_end`
bounds a single line at `MAX_RENDER_LINE_BYTES`, and a paragraph walk visits
lines until it finds a boundary — so on a 200 MB file with no blank line the
per-line cap alone is still a 200 MB scan behind one keystroke.
`MAX_PARAGRAPH_SCAN_BYTES` is the whole-walk budget, set to the frame's own
`MAX_FRAME_SOURCE_BYTES` so the worst case is "one keystroke costs what one
screenful costs".

**A line it cannot read all of is content, not a separator.** When
`line_blankness` sees an all-whitespace prefix that runs past the budget, it
cannot prove blankness — and calling an unprovable line blank would *invent a
paragraph boundary that is not in the file*. A multi-megabyte indented line is
almost always content anyway, so this is also the answer that is almost always
right.

**Deliberate divergence from Helix, recorded rather than matched.** With no
blank line anywhere, Helix's `move_next_paragraph` skips every remaining line
and lands on the end of the buffer (`test_behaviour_when_moving_to_next_paragraph_single`
in `helix-core/src/movement.rs`). CodeGG reports "no next paragraph" instead.
An end-of-buffer jump looks like the key did something and hides that it found
nothing, which is the failure the note exists to prevent.

**The repeat slot is one slot with three typed doors.** Helix documents
`Alt-.` as "repeat last motion (`f`, `t`, `m`, `[` or `]`)", so `]p` is
repeatable. `last_find`, `last_selection_motion` and `last_unimpaired` are
three *types* of motion, so they are three fields — each recorder clears the
other two, exactly as `note_motion` already cleared one. They are mutually
exclusive by construction, so `Alt-.` may consult them in any order without
inventing a precedence. `]Space` records nothing: an edit is not a motion, and
`echo`]Space Alt-.`` inserting two lines would be the bug.

**The box header is compact by default; `D` is the diagnostics dump.**
`header_lines` leads with one line — command, link state, and whether the
terminal is taking keys — because the dump it used to print unconditionally
(handle, workspace, attachment id, size, sequence cursors) is six lines of
exactly what a bug report needs and almost nothing a reader who just asked for
a shell needs, and on a short dialog it pushed the terminal rows off the end.
`full_header_lines` keeps every field and `D` folds it in or out per view.
Anything that changes what the visible rows *mean* — a resync, a degraded VT
screen, an exit — stays on screen in both forms; hiding "exited, input is
rejected" behind a toggle is exactly how a dead terminal reads as a live one.

**`d` deletes locally, and the overlay is keyed on text.** `vt100::Screen` is
read-only — there is no API to remove a row — so deletion means the selected
cells stop being rendered *and* stop being copyable. `Redactions` remembers each
scrubbed range against the **row's text**, not its position, because a bounded
scrollback has no stable row identity: output arriving shifts every row below it
and eviction removes rows from the top, so a position-keyed overlay would drift
onto whatever scrolls into the same cells. Resolving entries against the current
viewport on every read keeps a deletion attached to the line it was applied to,
so scrolling away and back does not silently un-delete it. Entries claim
distinct rows in order, so deleting two identical lines blanks the two of them.
The overlay is bounded (`MAX_TERMINAL_REDACTIONS`), and `clear_scrollback` drops
it: the text it was keyed on is gone, and carrying it forward would hide
whichever new output reuses that wording.

The verbs live on the controller and are applied through `apply_nav`, which
returns `ClearRequested` rather than clearing: only the command layer can issue
the daemon operation, so a key press can never clear a view the user is not
looking at. Esc drops a selection before it leaves either surface.

**A docked terminal matches the PTY to the column.** `terminal_viewport_size`
derives the initial size from the *outer* terminal, which is far wider than a
side panel; without a resize the program wraps to columns nobody can see and the
column shows the left edge of a much wider screen. The panel queues the real body
rect through `queue_resize` (coalesced last-wins, so a drag costs one round trip)
and the size is drained by `flush_terminal_resize`, shared with the input flush —
the render path has no `App`, so it cannot send it itself. `queue_resize`'s typed
error is discarded there: a panel showing an exited process must keep rendering
rather than toast every frame.

**Clear asks the process to clear, then wipes locally — in that order.** `C`
used to be a purely local wipe (`clear_scrollback` plus
`screen.reset(rows, cols)`), which left a live shell sitting on a blank screen
with no prompt: the shell had no reason to redraw, because nothing had been
typed at it. `clear_terminal` now queues `CLEAR_THROUGH_PROCESS`
(`ESC [ 2 J ESC [ H \r`) through `queue_input` — the same bounded, permissioned,
live-link path as any keystroke, never a privileged write — **before** the local
wipe, and only wipes once the bytes are admitted. A typed refusal reports itself
and leaves the screen intact: a local wipe whose repaint was never sent is a
blank screen the reader did not ask for.

Both halves of the sequence are load-bearing. The erase-display plus cursor-home
is what a terminal means by "clear", but sent alone it leaves the screen blank,
because a shell draws `PS1` after a *command*, not after an escape; the newline
completes an empty command line so the shell repaints its prompt. The literal
command `clear` was rejected because it depends on a `clear` binary on the
child's `PATH`, and its absence would show the reader `clear: command not
found` — an error for a keystroke they never typed.

**Nothing on the cleared screen is fabricated.** Every byte the reader ends up
looking at came out of the process. The local wipe is retained because it is
also the *history* semantic: `clear_scrollback` drops the retained scrollback,
the selection and the scrub overlay, while preserving `next_seq` so the next
honest chunk arrives as an ordinary continuation rather than a gap notice the
reader did not cause.

**A view with no live process still clears, and is not made to claim
otherwise.** Exited, gone and detached views take the local wipe alone. The
plan's failure semantics suggested refusing and leaving the screen intact on
every input refusal, which is right for every refusal *except* this one: there
is no process left to print a prompt, so nothing is being suppressed, and
refusing would take away the only way to clear the output of a shell that has
finished. What must not happen is a *pretence* — no warning about a refused
write, because none was attempted.

**A newly created terminal shows its own prompt with no keystroke.** The prompt
was always arriving on attach; what was missing was anything *reading* it, and
that is the armed pull above rather than work here. Until it arrives the panel
keeps its labelled empty state (`" no output — press i to type"`) and the box
`(no output yet)` — product text that says so, never a fake prompt.

- **Reconnect/exit**: transport reconnect marks live views
  reconnecting with scrollback retained (`on_projection_reconnect`);
  re-attach resumes from the last cursor. Process exit renders the
  terminal state and rejects input; daemon restart marks non-exited
  views gone. TUI close detaches unless the user explicitly terminates.
- **Routing**: the workspace comes from the active session's canonical
  binding (never ambient cwd); views filter per project.
- **No observation coupling**: headers describe without carrying
  output; raw terminal bytes never enter session/projection state.

### Remote TUI Snapshot Sequencing

`App::remote_sequence: u64` is monotonically increasing.

- `remote_snapshot()` — non-mutating, returns most recent snapshot.
- `next_remote_snapshot()` — mutating, increments and returns new snapshot.
- `build_remote_snapshot(sequence)` — builder parameterised by sequence.

**Resume semantics**: `from_event_seq == 0` → invalid resume;
`> remote_sequence` → client ahead; `<= remote_sequence` → fresh snapshot.

### Remote Plugin UI Effects

Two independent routes: `TuiMessage::PluginUiEffect` via WebSocket
and `AppEvent::PluginUiEffect` via `GlobalEventBus`. Both apply session
filtering before `apply_plugin_ui_effect()`. (`RemoteTuiMessage` is only a
local import alias for `TuiMessage`, e.g. `src/tui/app/mod.rs:79`.)

### Command Dispatch

`dispatch_tui_command` (`src/tui/runtime/command_dispatch.rs:84`) is
`pub(crate) async fn`. The match is overwhelmingly synchronous, but it
contains five `.await` points, all of the same shape:
`match core_client.request(req).await` inside the durable-editor arms
(`EditUndoLatest`, `EditReapplyLatest`, `EditUndo`, `EditReapply`,
`EditCheckpointList`, at lines 2109/2160/2214/2268/2320). Those arms
await an in-process `CoreClient` round trip and return a completion
`TuiCommand`. Every other arm is non-async; handlers needing latency use
spawn-and-complete or fire-and-forget instead.

## Key Types & APIs

### App (`src/tui/app/mod.rs:222`)

```rust
pub struct App {
    pub ui_state: UiState,
    pub session_state: SessionState,
    pub prompt_state: PromptState,
    pub messages_state: MessagesState,
    pub dialog_state: DialogState,
    pub agent_state: AgentState,
    pub sidebar: SidebarWidget,
    pub panel_host: crate::tui::panels::SidePanelHost,
    pub status_bar: StatusBarWidget,
    pub session_store: Option<Arc<SessionStore>>,
    pub message_store: Option<Arc<MessageStore>>,
    pub memory_store: Option<Arc<MemoryStore>>,
    pub run_store: Option<Arc<dyn RunStore>>,
    pub preferences: Option<UserPreferences>,
    pub core_client: Option<Arc<dyn CoreClient>>,
    pub config_watcher: Option<ConfigWatcher>,
    pub theme_registry: Arc<ThemeRegistry>,
    pub subagent_pool: Option<Arc<SubAgentPool>>,
    pub focus_manager: FocusManager,
    pub busy_spinner: SpinnerWidget,
    pub active_goal: Option<GoalSnapshot>,
    pub lsp_tool: Option<Arc<LspTool>>,
    pub security_review_running: Option<SecurityReviewTaskState>,
    pub shell_store: ShellOutputStore,
    pub command_run_store: CommandOutputStore,
    pub shell_handles: HashMap<u64, ShellHandle>,
    pub task_registry: TuiTaskRegistry,
    pub plugin_ui_state: PluginUiState,
    pub plugin_manager: Option<PluginManager>,
    pub remote_sequence: u64,
    // ... viewport, scroll, click, hover, border areas, etc.
}
```

### State Domains (`src/tui/app/state/`)

27 state modules (28 `.rs` files including `mod.rs`):

| Module | Purpose |
|--------|---------|
| `agent.rs` | Agents, models, selection, plan mode, project asset snapshot |
| `async_request.rs` | `AsyncUiRequestState` reusable state machine |
| `change_review.rs` | M006-E agent-change review state machine |
| `chat.rs` | Project-scoped chat projection |
| `diagnostics.rs` | `TuiDiagnostics` runtime counters |
| `dialog.rs` | All dialog instances, async request states, pending operations |
| `editor.rs` | M006-A editor attachment, status, and notice presentation |
| `execution_context.rs` | Explicit project execution context |
| `file_tree.rs` | M006-D project-scoped file tree view state |
| `manifest.rs` | Manifest handling |
| `messages.rs` | Message history, toasts, spinner |
| `observe.rs` | Read-only session observation |
| `persistence.rs` | State persistence |
| `plugin_ui.rs` | Plugin dialog/panel/status storage |
| `presence.rs` | Collaborator presence projection |
| `project_picker.rs` | Project picker state (Milestone 2) |
| `project_tabs.rs` | Multi-project tab state (Milestone 1) |
| `projection_client.rs` | Projection client state |
| `prompt.rs` | Prompt text, completions |
| `restore.rs` | State restore |
| `routing.rs` | Route routing helpers |
| `session.rs` | Session, token counts, history, changed files, git sidebar, rate limits |
| `snapshot.rs` | Remote snapshot building |
| `ui.rs` | Theme, layout, routes, dialog, input mode, keybindings, TTS, diagnostics |
| `view_switch.rs` | Active-view switch coordinator |
| `work_orders.rs` | M003/C001 task + work-order state, `ComposerMode` |
| `workspace_dashboard.rs` | M005 Workspace primary-view cache |

### Durable agent-run inspection (M006)

Naming note: this "M006" is the TUI's own durable-agent-run milestone and
is unrelated to the Desktop Frontend and IDE Foundation roadmap, whose
M006 was decomposed into M006-A through M006-E. Renaming either is a
terminology amendment, not a corrective edit; see the M006 TUI
presentation audit, section 12.

The sidebar consumes the canonical session projection's bounded durable-run,
worktree, and group summaries. It shows concurrent child status, stable run
identity, agent, branch/worktree, result commit, and attention state without
loading transcripts or logs. Reconnect and resync replace the same projection
state, so the TUI does not maintain a second run tree. Detailed worktree
inspection and cleanup continue through the existing authorized commands.

### Workspace file manager (`/files`, `src/tui/components/dialogs/files.rs`)

`/files` opens a **navigator** box over the workspace root, and `Files` is a
hosted panel so the same box docks into the side column with `Shift+L`. It is
one implementation: the box, the hosted panel, and the editor-side Ctrl+T
pane all run the *same* `FileTreeState` navigation, so a fix in one is a fix
in all three.

| Key | Meaning |
|---|---|
| `j` / `k`, `↓` / `↑` | move the cursor |
| `Ctrl+d` / `Ctrl+u` | page down / page up — two thirds of the box's viewport |
| `Enter` on a directory | expand/collapse **in place** — the cursor does not move |
| `l` / `→` | **make that directory the listing's root** and list its contents |
| `h` / `←` | **back one level**: leave the current root; at the workspace root, collapse an open directory in place, otherwise step to the parent row |
| `Enter` on a file | open it in the editor |
| `r` | re-walk the current root |
| `Esc` | close |

Enter and `l` are deliberately different verbs: Enter reveals contents and
leaves the cursor where the user put it, while `l` moves the *pane*. Making a
directory the root means the listing has a top level again, so entering a
deeply nested structure is the same operation at every depth — there is no
"walked down six expanded folders" mode to keep coherent, and no cursor that
has to survive a relist.

That also means the pane has **one** keyboard axis for scope
(`FileTreeState::root_history`, whose last entry is the current root) rather
than a prefix field and an expansion set that can disagree. `h` pops the
chain and lands the cursor on the directory just left behind, so stepping
out and back in returns to the same place. A jump deeper than one level keeps
the ancestry (`h` walks back out along the way it came); a jump outside the
current root resets it, because a chain claiming the pane arrived from a
directory it never listed would make `h` go somewhere unpredictable.

Re-rooting is a **daemon round-trip**, not a local filter. The scope travels
as `WorkspaceDirectoryListRequestDto::prefix`, the daemon walks that subtree
with the same bounded walker, and the echoed prefix is checked against the
one the request asked for — `l` twice in quick succession issues two requests
whose generations may both still be current, and only the scope says which
answered. Filtering the already-loaded listing would have been free, and
wrong: it would silently drop everything the original walk hit its bounds on
and present the remainder as if it were the directory.

Three rules keep this a navigator rather than a second document surface:

1. **The box holds no file text and performs no reads.** The listing arrives
   as daemon-origin metadata; `r` emits `TuiMsg::FilesRefresh` and the
   command layer owns the async daemon round-trip. Rendering never touches
   the filesystem.
2. **Opening a row is an intent, not an attachment.** `Enter` on a file emits
   `TuiMsg::FilesOpenSelected { path }`, handled in `App::process_msg` by
   calling `open_editor` — the same seam `/open` and the Ctrl+T pane use, so
   lexical validation and daemon authorization are not restated here. Only a
   *file* emits an intent; a directory has no document to open. Re-rooting is
   an intent for the same reason: `l`/`h` emit `TuiMsg::FilesDescend` /
   `TuiMsg::FilesAscend` rather than mutating the box's own copy.
3. **A widget cannot reach `App` from `render`, so the box holds a copy of the
   shared state**, seeded from `App::file_tree_state` at open time — which is
   why `/files` resumes where a previous browse left off — and re-synced from
   the generation-guarded completion in `commands::file_tree::apply_listing`.
   The copy is made only when a box is actually mounted.

Authority is unchanged from the Ctrl+T pane: the listing request and the
open both resolve the tab binding through `App::project_execution_context()`,
never `session_state.project_dir` or the process cwd — and the listing
itself is enumerated on the daemon host, never the TUI host. Files opened
from `/files` are therefore `DocumentScope::Workspace`, and the editor's
`[project]` badge is the honest reading.

#### Depth is display arithmetic

`TreeNode::depth` counts from **the walk's own root**, and
`walk_directory(&root.join(prefix), prefix, ..)` gives a subtree walk a fresh
count. So the top level of a listing scoped with `l` arrives at exactly the
depth the workspace root's top level arrives at — the two are the same number,
and both renderers used to draw it as the same thing. Entering a directory
therefore made the listing look like the root again, which is the one thing
re-rooting exists to stop doing.

`FileTreeState::display_depth` is the fix: `scope_depth() + row.depth`, clamped
to `MAX_INDENT_DEPTH`. `scope_depth` is the component count of `root_prefix()`,
so what the walk cannot know — how deep the pane itself is sitting — comes from
the state that *did* know, and the walk is left alone. It is a read: it touches
no expansion, no selection and no scroll, which is what keeps a presentation fix
out of navigation.

`FileTreeState::indent_for` pairs that depth with last-ness (`└─` over `├─`) in
one place, because both renderers need them together. Last-ness reads the
**immediately next** row: deeper means the row has children still coming,
same-or-shallower means it was the last entry in its directory. The rows are a
pre-order flattening, so that is the whole question — and testing the rest of
the slice instead would close every level at once.

`file_icons::tree_indent` turns the pair into glyphs and sizes them. Box drawing
only (`│` `├` `─` `└`, all BMP), deliberately **not** behind `CODEGG_FILE_ICONS`:
"which directory does this row belong to" cannot depend on a patched font the
user may not have, so `plain` and `nerd` render the same hierarchy. When the
column is too narrow, levels are dropped from the outside in — the guide to the
immediate parent is what tells a reader what the row belongs to, so that is the
part that survives — and the guide goes away entirely before the name does. The
name is content; the guide is orientation.

Both renderers call that one function. They used to carry their own
`"  ".repeat(row.depth.min(8))` and their own private `MAX_INDENT_DEPTH`, which
is how the box and the pane came to draw different ladders for one tree.

The box's title is a **breadcrumb** — `workspace › src › tui` — for the same
reason. The flat `Files · src/tui (n shown)` carried the same information and
hid which component `h` steps back through, so stepping out looked arbitrary.

### Swappable side panel (`src/tui/panels/`)

The panel column is a **panel host**, not a widget chosen by an `if` in the render
path. `SidePanelHost` (`src/tui/panels/mod.rs`) owns three pieces of state: which
edge each panel is docked to, which edge the keyboard gestures act on
(`selected`), and whether that edge's panel holds focus.

#### Edges and slots

`PanelPlacement` is `{Right, Left, Bottom}`, with **`Right` the default** — the
layout that shipped first, so a user with no stored preference sees no change.
`Bottom` is a full-width row beneath the main input box.

`SidePanelHost` holds **one `PanelSlot` per edge**, and a slot is
`{panel, visible, history}`. Panel *state* stays keyed by `SidePanelId` (one
`ChatPanel`, one `SessionPanel`, …); the slot only records which edge is
showing which id. That split is what lets a panel move between edges without
losing its cursor, filter or scroll: the state travels with the id.

`visible` is **per-slot, not a global flag**. The whole point of a swap is that
the edge a panel left and the edge it arrived at can be in *different* states
at the same time — one up, one collapsed. A shared bool would collapse both or
neither. `ui_state.sidebar_visible` survives only as a derived mirror of
`any_visible()`, because the session manifest persists a sidebar-visibility
preference that predates placements; **no behavior branches on it**, since a
read there silently answers a coarser question than "which edge is up".

#### The swap

`SidePanelHost::dock(id, target)` is the only way a panel changes edges, and it
**moves** rather than copies. A panel is therefore never loaded on two edges at
once, which is what stops one widget state rendering twice.

The edge being vacated pushes its current state onto its own bounded history
(`PANEL_HISTORY_DEPTH`, 3 — a convenience memory, not an undo stack) and then
**restores** what it remembers, collapsed-ness included. An edge with an empty
history restores to collapsed, which is the honest answer: nothing was there
before, so there is nothing to bring back.

So `/chat` then `Shift+H` with the chat panel on the right *walks it left*, and
the right column comes back to whatever it was — the session panel if that was
up, collapsed if the user had put it away. Re-docking a panel to the edge it is
already on is a no-op rather than a self-push, or the next swap would "restore"
a panel that never left.

#### Resize mode: outward only, and the chord that arms it

`Ctrl+N` arms a yazi-style resize mode. `Esc`, `Ctrl+N` again, or `Enter` leave
it; it stays armed across repeated keys, because the gesture is "drag several
borders in one visit" rather than "press a key, get one step". Normal mode only,
like the panel manager — `h/j/k/l` are ordinary letters while composing, and a
frame-level mode that could be armed mid-sentence would swallow the next four.

`Ctrl+N` was `NewSession`; it is now the bare `n`, which the binding map already
carried, so the remap costs nothing. The chord is deliberately **not** an
`InputAction`: the frame is not a composable action, and a configurable binding
would let it be pinned to a key a panel already owns.

While armed, `h`/`j`/`k`/`l` name the focused surface's *own* border in that
direction, and drag it **outward only**:

| subject | `h` | `j` | `k` | `l` |
|---|---|---|---|---|
| right-docked panel | left border out → grows | frame edge | frame edge | frame edge |
| left-docked panel | frame edge | frame edge | frame edge | right border out → grows |
| bottom-docked panel | frame edge | frame edge | top border up → grows | frame edge |
| main buffer (`LLM`) | left panel shrinks | bottom panel shrinks | nothing is above it | right panel shrinks |

Two rules make this one rule rather than four cases. **Only shared borders
move**: a column already owns its left edge to the frame, so a right-docked
panel grows with `h` and *not* with `l` — the mirror of a left-docked panel
growing with `l`. **Growth is relative**: the border belongs to both surfaces,
so growing one shrinks the other. `resolve_resize` answers "which panel's size
changes, and in which direction", so the same key cannot mean two things
depending on which side the reader was standing on.

The geometry is `src/tui/panel_resize.rs`, which knows nothing about `App`, and
it is resolved **ahead of the focused panel** in `App::on_key`. Both orderings
are forced: a focused panel consumes every key it is shown, so `h/j/k/l` would
otherwise move the terminal cursor instead of a border; and the main buffer is
the other half of the gesture, so `Ctrl+N` has to work with the keyboard *out*
of a panel too. A resize key naming a frame edge is consumed and ignored rather
than passed through — letting `l` resize on some rows and navigate on others is
what makes a mode feel haunted.

Column widths are **per edge** (`LayoutConfig::{left,right}_panel_width`,
falling back to the shared `sidebar_width`), because "make the right column
wider" and "make the left column wider" are different requests and one number
can only satisfy one at a time. The bottom panel keeps `bottom_panel_ratio` as
its *default* shape and records an exact `bottom_panel_rows` the first time its
edge is dragged: the ratio is integer-divided against the frame height, so a
one-point change is 0 rows on a short frame and 2 on a tall one — a key that
appears dead for no stated reason. `TuiLayout::observe_frame` records the last
rendered height purely so that conversion has a denominator. Sizes are
runtime-only for this pass; only `ui.panel_placement` is persisted.

`select(id)` goes through `slot.occupy`, so displacing a panel by selecting
another one is remembered exactly as docking is. A user who walks to Chat in
the manager and then pushes it left gets their session panel back.

This replaced a deliberate **refusal**: a directional naming an empty edge used
to be turned down with a toast naming the key that worked instead. That was
honest but a dead end — the user had said where they wanted the panel, the
frame had a good place to put it, and the answer was "no". Swapping keeps the
gesture truthful: pushing a box left *moves* it left.

| Placement | Panel region | Enters with | Leaves with | List cycles with |
|---|---|---|---|---|
| `Right` | column, `sidebar_width` wide | `Shift+L` / `Shift+→` | `Shift+H` / `Shift+←` | `Shift+J`/`K`, `Shift+↓`/`↑` |
| `Left` | column, `sidebar_width` wide | `Shift+H` / `Shift+←` | `Shift+L` / `Shift+→` | `Shift+J`/`K`, `Shift+↓`/`↑` |
| `Bottom` | full-width row, `bottom_panel_ratio`% (min 8, floor 12 for main) | `Shift+J` / `Shift+↓` | `Shift+K` / `Shift+↑` | `Shift+L`/`H`, `Shift+→`/`←` |

The rule is **geometry resolved against what is on screen**, not a per-side
branch. `move_directionally` asks in order:

1. a box is open → dock it to the edge the direction names (`dock_open_box`);
2. the keyboard is in the main buffer and the direction names an edge that is
   **showing** a panel → take the keyboard there, re-pointing `selected` if it
   is a different edge;
3. the keyboard is in a panel and the direction is the one leading **away** from
   its edge → return to the main buffer;
4. otherwise `cycle_step(dir)` → walk the loaded panel types.

**The main buffer is a stop, not a gap.** The second column is not "the other
end of the line" from the first — the buffer sits *between* them. So step 3
fires even when the far column is populated: two `Shift+H` from Chat reach
Terminal, and one lands in the buffer the user was sitting in. That is the
difference between "go left" and "go to the left panel", and it is why the
dispatcher asks *where the keyboard is* rather than which edge is selected —
the answer is a position on a line, not a pair of edges.

Step 2 is what makes a second column reachable with the same keys that moved
the first one there. Step 3 is also what preserves the single-column behavior:
with one right-hand panel, `Shift+H` finds nothing to the left and returns to
the composer. `Shift+K` names no edge at all (`PanelPlacement::named_by`
returns `None`), so it can never shadow the way out of a bottom panel.

`TuiLayout::split` returns a **named** `PanelRegions { main, left, right,
bottom }` rather than a positional `Vec<Rect>`. A vector cannot express "left
*and* right are both up" without the caller guessing which index means which
edge, and a caller that guesses wrong paints the panel over the transcript.
Named regions make that a compile error instead of a mangled frame.

Splitting still refuses when the frame cannot host a panel honestly — too
narrow for another column, or shorter than `MIN_MAIN_PANEL_HEIGHT +
MIN_BOTTOM_PANEL_HEIGHT` — matching the original no-dead-strip rule: a panel
that cannot render is not shown. A second column therefore yields to the
transcript's floor rather than squeezing it.

Placement is persisted as `ui.panel_placement` (`right` / `left` / `bottom`),
read at startup into `SidePanelHost::with_placement` and written by
`persist_panel_placement`. An unknown value falls back to `Right` rather than
failing the boot — the same tolerance a bad panel hotkey gets.

The panel manager carries a **display-only layout strip** above the panel list,
with `p` cycling the edge. It is deliberately not rows in the navigable list:
which edge the panel lives on and which panel is loaded are different
questions, and merging them would renumber every panel row and make `Enter`
mean something new depending on cursor drift. The bracketed edge also names
its own collapsed state (`[Left collapsed]`) — only the live edge does, because
three states in a 44-column box wrap, and a wrapped strip loses the entry the
user has not tried yet.

**Opening the manager must not change the frame.** It used to force the selected
edge visible, on the reasoning that a strip describing a collapsed column is
confusing. The cost was that a panel the user had deliberately put away with
`Ctrl+B` came back by itself the moment they reached for `Shift+S`, which reads
as the TUI undoing their last action. Naming the state is the honest fix;
changing the frame to match a strip is not.

#### Panels

Five panels are **bespoke** — they own live projections or a controller and
need their own key model:

| Panel | Title badge | Holds |
|---|---|---|
| `Session` | `[SESSIONS]` | the `SidebarWidget` projection |
| `Chat` | `[CHAT]` | interactive project chat |
| `Terminal` | `[TERMINAL]` | the live PTY view |
| `Help` | `[HELP]` | the keybinding reference |
| `Agents` | `[AGENTS]` | the M006-F management rows |

The other **27** are **hosted**: each is a `/` command that brings up a box,
and the panel holds that box's component rather than a lookalike. They are one
implementation (`panels/hosted.rs`), not 28:

| Group | Panels |
|---|---|
| Lists | `Tree` `Sessions` `Tasks` `Plan` `Goal` `Collaborators` `Memory` `Research` |
| Reports | `Logs` `Context` `Cost` `Usage` `Stats` `Doctor` `ShellShow` `Security` |
| Pickers | `Models` `Mcp` `Keybinds` `Themes` `Plugins` `Team` `Control` `TaskView` `Diff` `Review` |
| Browsers | `Files` |

Which panel is loaded is chosen by the **panel manager box**
(`Shift+S` in Normal mode, or `/panels`) — a dialog, not an overlay drawn
inside the column. It replaces the old two-column grid selector, which could
not be filtered and showed unlabelled buttons in 30 columns. It renders
[`SidePanelId::ALL`] directly, so the list it shows *is* the registry.

Adding a **hosted** panel is an enum variant, an entry in `SidePanelId::ALL`,
and `label`/`title`/`hint`/`load_command`/`config_key` arms, **plus an arm in
`commands::panels::open_panel_box`** — that match has no catch-all, so it is
the one site outside this file that fails to compile if missed — and a
`dock_target` arm in `App` so `L` knows where the box goes. `title` must be
all-ASCII uppercase and `config_key` snake_case and unique; both are enforced.
The host constructs the backing `HostPanel` from `ALL` filtered on
`SidePanelId::is_bespoke()`, so there is no host field, no `panel()` arm and no
render-path branch. Adding a **bespoke** panel means adding it to
`is_bespoke()`, writing a `SidePanel` impl, adding a host field, and adding arms
in `panel()`, `panel_is_empty()` and `composer_hints()`.

That filter is **load-bearing, not bookkeeping**. `materialize_hosted_panel`
proceeds only for a panel whose `hosted_panel_has_content` is `Some(false)` —
a hosted panel with no box adopted — and its first move is `open_panel_box`,
which returns through `select_panel` into `materialize_hosted_panel` again. A
bespoke panel that keeps a hosted placeholder therefore answers `Some(false)`
and loops: `/agents` overflowed the stack and aborted the process. `is_bespoke`
is the single list both the constructor and the bespoke-or-hosted test read,
because when they were two separate `matches!` literals they drifted the moment
a fifth bespoke panel was added, and the test agreed with the bug by comparing
against the same stale literal.

**A box and its panel open in different modes.** `HelpDialog::new_with_mode`
takes the starting mode from its caller, and the two callers disagree on
purpose. `/help` arrives in **Insert** — it is opened to type a query. The
docked Help panel arrives in **Normal** — it is read and scrolled first, and
`i`/`a`/`Enter` reach the filter in one keystroke when the user wants it. In
Insert `j`/`k` are filter text, so a panel hardcoded to Insert does not
"refuse" to scroll; it types `j` into a filter nobody is watching, which is
indistinguishable from a dead pane. The panel owns no copy of this logic: it
hosts the dialog and hands it a mode.

**The Terminal panel always has a way in.** `i` routes through
`type_intent()`: with a handle it emits `TerminalPanelFocus`, and the command
layer decides what that means. A live process that merely lost its attachment
is re-attached; an exited or gone one is replaced with a fresh shell. With no
handle at all the panel emits `TuiMsg::StartTerminal` instead. The distinction
lives in the command layer because only it can tell the cases apart, and a
panel is structurally unable to see app state. `App::terminal_panel_spawn`
carries *which surface* the result lands on across the asynchronous
create → attach pair, so the panel is not covered by a box it did not ask for;
every terminal failure path clears it, because a flag left set by a failed
create would divert the next unrelated terminal into the panel.

**`/terminal` produces a terminal.** With no live view behind it the command
starts `default_shell_argv()` (`$SHELL`, falling back to `sh`/`cmd.exe` — the
same answer `shell::ShellRuntime` gives human `!cmd`) and shows the result. An
*explicitly named* handle is still honoured as typed, so `/terminal <handle>`
never silently substitutes a shell for a command the user actually asked for.
`/terminal-create` remains for running a specific program.

`PanelCtx` grants a panel the state it renders from and nothing else. The
Terminal panel is granted `&mut InteractiveTerminalController` — the *same*
object the `/terminal` box drives, because two VT parsers for one PTY would
disagree about what is on screen. That grant carries presentation state only:
no client, no locator, no authority to choose a project or a terminal.

**A focused PTY claims `Esc`, but never `q`.** `owns_exit_key` takes `ctx`
because the claim depends on state the panel does not own. The Terminal panel
returns true for `Esc` **only while its view is focused**, so it routes into
`handle_key` instead of `SidePanelHost::blur`. Declining it split the two focus
flags: the host dropped panel focus while `TerminalFocus::Focused` stayed set on
the view, so `Esc` looked like it left and the PTY still held the keyboard — and
because `panel_active` requires `is_focused()`, the panel then saw no keys at
all while still reporting INS.

`q` was claimed alongside it, which is why this section could not simply keep
the old pair: in INS the panel *is* a terminal, and `q` is a character the user
is typing. Claiming it meant the shell could never receive the most common
letter in most commands. Only the mode key is claimed now.

**A panel that fronts a process claims Insert-mode keys.**
`SidePanel::claims_insert_keys` is the opt-in that makes `panel_active` consult
a focused panel in Insert as well as Normal. The host's default is right for
every panel that renders its own text field — there, Insert means "type into
that field" and the composer is the correct target — but it is exactly wrong
for the Terminal panel, which renders a *process`. Without the opt-in, docking
the terminal with `Shift+L` and typing sent every keystroke to a composer the
user was not looking at, and the panel looked frozen. The claim is scoped to
the focused panel only, so it cannot take keys from an unrelated surface, and
it is keyed on `ctx.terminal.is_focused(...)` so a terminal that is still
attaching or has exited leaves those keys visible and recoverable in the
composer instead of dropping them.

`handle_key` owns the exit and ends in `TuiMsg::TerminalPanelBlur`, the one call
that clears the view's focus. In NOR the panel declines, so `Esc`/`q` remain
cheap focus exits.

**A focused panel owns navigation, not text — and only in Normal mode.** Two
rules replace the original "consumes every key" contract:

1. **Mode gate.** The host is consulted only when `input_mode == Normal` (or
   the selector overlay is up). This is load-bearing, not an optimisation: the
   host claims `Esc` and `q` as focus exits, so consulting it in Insert mode
   would make the letter `q` untypable and `Esc` unable to leave Insert. In
   Insert mode the composer owns the keyboard, which is what lets one bottom
   composer deliver to two destinations.
2. **Navigation vs text.** While focused in Normal mode the panel consumes
   arrows, `j`/`k`, paging, channel switching and its own verbs, so a stray
   `j` is never read as a keystroke elsewhere. It does not own text. A verb
   with nothing to prefill (`i`, `a`, `Enter`) returns
   `PanelKey::PassThrough` so the shared composer's `FocusPrompt` opens Insert
   mode; a verb that carries context (`r`, `e`) returns
   `PanelKey::Intent(TuiMsg::ChatComposerSeed)` instead, because falling
   through would strand the reply target or the body being replaced.
   `PassThrough` is only legal for a panel that holds no draft of its own —
   the chat panel has none, and a panel with a live buffer must still consume
   everything so falling text is never edited in one buffer while rendered in
   another.

**Panels advertise their own keys.** `SidePanel::composer_hints` is the
generalisation seam: a panel declares the verbs it owns, and they appear on
the shared hint line above the composer. It takes a narrow `PanelHints`
(`focused`, `placement`, `chat_project`, `input_mode`) rather than the full
`PanelCtx`, so the composer row can ask for a string during render without
taking `&mut` borrows of the sidebar and chat state. Adding the next panel — a
multiplexer-style region that brings up a box where `/chat` used to — is still
an enum variant, an entry in `SidePanelId::ALL`, and one `SidePanel` impl.

**A hosted box's verbs come from the box, not from the host.** `HostPanel`
holds a `Box<dyn Component>`, and a widget that knows its own keymap answers
`Component::keymap_hint()` — the string sits next to the `handle_key` arms that
implement it, so the two cannot disagree. The host only keeps a generic
`"j/k scroll · i act"` fallback for widgets that answer `None`. That fallback is
not a default worth keeping in place: it named `i`, and `i` is not a verb any
adopted box dispatches, so every box-derived panel advertised keys that did
nothing. `/files` re-roots with `l`, expands with `enter` and pages with
`^d`/`^u`, and its panel now says so; the `InfoDialog` family says `j/k scroll`,
with `a publish` added only for `/project-init`, which is the one `InfoType`
that arms it.

**A hint describes the *hosted* verb set, which is not the modal one.** More arms
of a widget's `handle_key` are unreachable in a column than M002a first found, and
a hint that named them would be teaching keys that do not work:

- `Esc` never arrives. `SidePanelHost::handle_key` claims `Esc`/`q` as the
  universal exit and blurs before the panel is consulted, and a hosted panel
  does not override `owns_exit_key`.
- `Enter` arrives and closes nothing. Docking *pops* the box off the modal
  stack, so `TuiMsg::CloseDialog` runs `close_dialog()` against an empty stack.
  `Shift+←` is the way back to a modal, and the host already says so.
- **The arrow keys and bare uppercase `H`/`J`/`K`/`L` never arrive either.**
  `SidePanelHost::handle_key` resolves a panel `InputAction` and routes it to
  `MoveDirection` *before* the panel is consulted, and
  `insert_panel_direction_bindings` binds the four arrows plus bare **and**
  shifted `H`/`J`/`K`/`L`. This is the finding that made the keymap half of
  M002b non-trivial: every `↑/↓ navigate` in the twelve dockable boxes was dead,
  and so was `/tasks`'s advertised `J/K reorder` and `/security-review`'s
  `H copy hunk`. Lowercase `j`/`k` are *not* panel directions and do arrive, which
  is why every hint spells them that way.
- `Home`, `End`, `PageUp` and `PageDown` are not panel directions, so a widget
  that dispatches them does deliver them from a column.

For the same reason a `set_custom_footer` string is **not** replayed into the
hint: `/project-init`'s `"a: publish   Esc: cancel"` and `/shell-show`'s
six-verb footer describe the modal box, where `Esc` really does close.
Reusing them would re-advertise exactly the dead `Esc` the hint exists to stop
describing. M002b retired twelve in-panel footers and four copies of a
border-title keymap on the same reasoning. The one widget that used to keep a
footer this way — `PanelPicker` — was retired with its last caller, so there is
no surviving exception.

**The host owns the chrome, the widget owns the rows.** `render_hosted` paints
the border and the ` ● Files ` title — the `●` being the only thing on screen
that says which panel has the keyboard — and then calls
`Component::render_in_panel`, which defaults to `render`. A widget that draws a
border and a title of its own overrides `render_in_panel` instead: painted over
the same rect, its chrome did not layer but *overwrite*, taking the title and
the focus dot with it, so a focused panel and an unfocused one rendered
identically. `/files` also drops its in-box keymap footer there, because the
composer row carries those bytes and a column too narrow to hold a keymap gives
its rows to the listing instead.

Every dockable box has now adopted this: fifteen `render_in_panel` overrides
(thirteen dialogs plus `Placeholder`) and fourteen `keymap_hint` overrides,
counted by grepping `src/` for the method names and excluding
`components/component.rs` where the seam is declared. `GENERIC_KEYS` remains as
the fallback for the bespoke panels, where the host owns the whole surface, and
it is now correct for no dockable box at all.

**A box's colours come from the theme it is handed, never from one it cached.**
`Component::render` and `render_in_panel` receive the live `&Arc<Theme>` on every
frame. A body that stores an `Arc<Theme>` and refreshes it only when the box is
opened goes stale the moment the user changes theme: seven sites reassign
`UiState::theme` and none of them re-pushes it into an open box, so the rest of
the UI follows the new theme while the box keeps the old one. Four boxes did
exactly that — `review`, `plugin`, `security_review`, and the `diff` dialog via
`DiffViewer`.

Two shapes of the same mistake look different in a source count but are the
same hazard. `mcp` and `research` cached a theme *and* reassigned it from inside
their own render body, so they were correct — a cache the render overwrites
every frame. They were also the two highest `self.theme.` read counts in the
codebase, which is how a source-count audit ranks the defect's "worst
instances" as the two boxes that are actually fine. Read against a rendered
buffer they pass; read against grep they lead the chart.

`Widget` cannot carry a theme at all, so a widget that is *only* a `Widget` has
no way to reach the live one. That is why `DiffViewer::render_in`,
`PlanDialog::render_legacy` and `DiffDialog::render_legacy` are inherent methods
rather than trait impls: a trait impl would have to read a cached field, which
is the defect. They render byte-identically to the impls they replaced.

The picker is the deliberate exception: `/theme` previews each candidate
palette, so its column deliberately paints colours that are not the active
theme's. "Everything follows the live theme" would destroy the feature.
`tests/tui_theme_source_of_truth.rs` encodes both rules — each box must follow a
theme change, and the picker must keep showing candidates.

**A chrome-free body must still compute one viewport.** `InfoDialog` overrides
both seams by passing a flag to a single private routine rather than writing a
second render path, and that is load-bearing rather than tidy. Its `Down`/`j` arm
deliberately does not bound `scroll`, and `scroll_to_end` stores a `usize::MAX`
sentinel instead of an offset, because a line that wraps occupies several rows
and the reachable maximum is not `lines.len()`. Both hand the clamp to the
render pass. Two passes — one clamping against the modal rect, one against a
column inset by a different amount — reach a different last wrapped row, and
nothing reports it: the panel just stops scrolling early, with no error and
nothing to show in a screenshot. One routine makes the number the clamp uses and
the number the paragraph paints into the same value by construction.

M002b carried that rule to the whole set, and it caught a second instance of the
identical defect. `ResearchBrowserDialog`'s report view stored a row offset that
no render path clamped, so its `Home` arm — which sets `usize::MAX` — reached
ratatui as `u16::MAX` through the field's `as u16` and rendered the report
**blank**, with no way back out because the offset only ever grew. The clamp now
lives in the shared body routine, in both paths, against the rect actually being
filled. This is the one place in M002b where modal output changes, in one state
(after `Home`), and it is the milestone's own structural rule rather than a new
feature: the alternative was shipping a box whose End key erases its content.

The general rule for a viewport, since the widgets do not agree on who owns it:

- **Render-derived.** `/files` and `/diff` compute their visible-row count from
  the rect they are filling on every pass, so the modal and the column are both
  correct by construction.
- **Externally-owned.** `/tree` and `/theme` carry a `visible_height` field that
  **no production code ever sets** — only `tests/tui.rs` calls
  `set_visible_height`, so it stays at its `new()` default of 10 however tall the
  box is. A 12-row `/tree` modal therefore offers 10 rows into 6 usable ones and
  ratatui clips the tail. That mismatch predates this work and was deliberately
  left alone: re-deriving the field from the rect would change modal rendering,
  and a single field shared by the render pass and the selection clamp is still
  one source of truth, so the two paths cannot disagree with each other. It only
  disagrees with the box's own height.

**Dead render paths, not dead widgets.** Five dialogs still carry a legacy
`impl Widget for &XDialog` that duplicates the whole modal render, and `/diff` and
`/plan` still expose a public `render(&self, frame, area)` alongside
`Component::render`. None of them is called from anywhere in `src/` or `tests/`.
They were left in place deliberately: they are duplicated surface rather than a
hosted path, and removing them is the later de-duplication milestone's work, not
this one's.

**Key ownership precedence** in `App::on_key`: modal dialogs (`FocusManager`) →
timeline → command mode → completions → change review → **side panel (Normal
mode only)** → Workspace view → Editor view → composer. The panel sits above
the non-modal primary views because those also claim printable keys, and a
chat panel must see them as navigation rather than as dashboard input.

**Panels are presentation plus intent**, exactly like `Component` dialogs.
`PanelCtx` grants named borrows only — theme, clock, the sidebar widget, the
chat state, and a resolved `chat_project`. There is deliberately no `&App`,
so a panel cannot reach a locator or a client. Every mutation is an emitted
`TuiMsg` routed by `App::process_msg` into an existing command seam; the chat
panel uses the same `start_chat_*` seams as the `/chat` window, so there is
one send path rather than two that could disagree.

**Project binding** is resolved by `commands/panels.rs`, never by a panel:
the Workspace dashboard's selected row when that view is open, otherwise the
active tab's explicit project id. `App::chat_panel_project` stays the single
field the chat commands read; the panel's `bound` flag is kept in step with
it so `SidePanelHost::focus` (which has no app access) can refuse focus to an
unbound chat panel. Changing the binding drops the panel's per-view state, so
a composer buffer cannot survive into a different project's channel list.

**Focus moves and the input target.** Two things are deliberately *not* stored
state. `App::input_target()` is derived from the host on every read
(`Chat` iff the Chat panel is focused), and `SidePanelHost::focused` decides
which region has the keyboard. Because the target is derived rather than
latched, moving focus away re-arms `Build` automatically — there is no path
that leaves the composer pointed at chat after the user has left the panel.

| Key | Action | Mode |
|---|---|---|
| `Shift+Right` / `Shift+L` | `FocusSidePanel` | Normal only |
| `Shift+Left` / `Shift+H` | `FocusMainBuffer` | Normal only |

These are **Normal-mode gestures**, enforced by `is_normal_only` in
`handle_key_with_bindings`: the binding map is shared across modes, so without
that guard a `Shift+H` would claim the letter in Insert mode. They are
Shift-*qualified* so bare arrows keep their existing meaning — `Up`/`Down`
scroll the transcript, `Left`/`Right` move the caret. `Shift+Up`/`Down` and
`Shift+J`/`K` are deliberately left unbound until a layout with stacked panels
exists; binding keys that cannot change anything would advertise a capability
the UI does not have.

Each letter is bound twice, with and without the `SHIFT` modifier, because
terminals disagree about how to encode a shifted letter. Binding both is safe
(uppercase `H`/`L` are otherwise unbound in Normal mode) and is the same
legacy-encoding hazard that made every `Ctrl+Shift+X` chord undecodable — see
`tests/modifier_chords.rs`.

**A built-in `Shift+<capital>` is composed text in Insert mode.** Every mnemonic
in this file is a single capital letter, which is exactly what makes it
dangerous: it is a letter the user is in the middle of typing. Two separate
guards keep that true, and they are not interchangeable.

`is_normal_only` is keyed on the **action**, so it can only say "this action is
Normal-only" — it cannot say "this *spelling* is". It is right for the panel
directions and for `x`/`ToggleAgentRun`, which binds no modifier at all. It is
wrong for `OpenProjectPicker` and `OpenWorkspaceDashboard`, because those
actions also own a `Ctrl` chord (`Ctrl+\`, `Ctrl+O`) that must keep working in
Insert mode; listing them would have silently taken the chord out too.

The shifted-capitals rule therefore lives in the Insert branch of
`handle_key_with_bindings`, at the **key**: a text-modifier key whose binding
matches either shipped map (`DEFAULT_BINDINGS` or `VIM_BINDINGS`) is text.
Both maps matter — `Shift+G` exists only in the vim one. Comparing the binding
rather than the map's identity is what keeps the neighbouring arm's promise: an
explicit *user* binding for a shifted capital still fires. The old gate was
`bindings.is_none()`, which is dead in the real app: `App::on_key` always passes
`Some(&self.ui_state.bindings)`, so the guard never ran and a capital `F`
mid-word opened the project picker in the **default** configuration.
`builtin_shift_letters_are_composed_text_in_insert_mode` sweeps both keymaps so
the next mnemonic inherits the fix rather than rediscovering the hole; the
sweep also asserts it saw all thirteen letters, so deleting a binding cannot
make it pass vacuously, and
`a_user_binding_for_a_shift_letter_still_fires_in_insert_mode` asserts the fix
was not bought by making text unwinnable.

**The composer is the only input surface.** The chat panel holds no composer
buffer; verbs that need text hand the keyboard to the bottom composer. The
composer renders `[NOR|INS] [agent] [BUILD] [CHAT]` with the armed target
emphasised, and `send_prompt` reads the same derived value, so the badge and
the dispatch cannot disagree. When the chat target is armed, text routes
through `commands::chat::route_composer_to_chat` using
`App::chat_panel_project` — never a path or the process cwd — and if the
target is armed but unbound the text is **dropped with a warning** rather than
rerouted to the agent, since that silent reroute is the exact mistake the
badge exists to prevent.

### One chat browser, two surfaces (`src/tui/chat_browser.rs`)

`/chat` (`ChatDialog`) and the chat side panel (`ChatPanel`) are two
renderings of **one** browser. `ChatBrowser` owns everything the two
previously duplicated: the message cursor, the channel highlight, the armed
redact, the Normal-mode keymap and what each key means, row rendering, the
viewport anchor, and the key hints.

They were peers once, with the keymap copied between them, and drifted until
the panel could neither reply nor edit and opened scrolled to the oldest
message so a populated channel looked empty. A fix applied to one surface and
not the other is a fix that comes back; there is now one place to fix.

What the browser deliberately does not own is **text entry**. It holds no
text. A verb that needs typing resolves to `ChatKeyAction::Compose` carrying a
`ChatComposeSeed`, and the host materializes the editor:

| Host | `Compose` becomes |
|---|---|
| `ChatDialog` | its own bounded composer (`ChatMode::Compose` / `Edit`) |
| `ChatPanel` | `TuiMsg::ChatComposerSeed` — arms the shared composer |

That is the entire difference between the surfaces: where the caret lives,
not what the keys do.

**The armed seed.** `PromptState::chat_seed` records what the composer's text
*means* (a reply target, or a body being replaced with its captured
revision) — not a second draft. While armed it **outranks panel focus** in
`App::input_target`: pressing `r` means "this is a reply to *that* message",
so honouring a later focus move would turn the pre-filled `@name ` into an
agent prompt. The badge shows `[CHAT]` plus `[REPLY]`/`[EDIT]` for exactly as
long as the seed is armed, and `Esc` in Normal mode cancels it — clearing the
*meaning*, never the text. `route_composer_to_chat` consumes the seed and
clears it after the `start_chat_send`/`start_chat_edit` seam runs, so the
target belongs to exactly one send.

**`@` is not the same trigger in both destinations.** Against the agent
session `@` opens the agent/file completion popup and Enter accepts. Against
chat it is a mention. The completion layer claims Enter first, so leaving the
popup enabled in chat would make Enter insert an agent name instead of
sending — and since `r` seeds the composer with `@author `, every reply
started by one keypress would have had its Enter eaten. `update_completions`
skips the `@` branch when `input_target() == Chat`.

**Selection contrast.** `theme.selection` is a *background*; every selected
surface in the TUI pairs it with a foreground. The chat surfaces used it as a
foreground, which painted the author's name dark-on-dark — styled, and
unreadable. Selection is now `fg(foreground).bg(selection)` and padded to the
row width so it reads as a full-width bar. Unselected bodies are `foreground`
rather than `muted`: dimming the whole transcript whenever nothing was unread
made the common case the unreadable one.

**Tail-follow.** `ChatBrowser::follow_tail` starts `true`, so a surface opens
on the newest message and keeps following arrivals. `k` unpins (history stops
moving under the user), `j` past the newest re-pins, `G` re-pins. `sync` runs
on every render and every snapshot, not just on navigation, so a message
arriving while the user is pinned moves them with it.

**Contextual keys above the input.** `SidePanel::composer_hints` returns the
verbs the active panel owns; `App::composer_hint_text` prefers an armed seed
(`↵ send reply · Esc cancel reply`) over panel hints, and `render.rs` reserves
one row for the line directly above the composer — inside the prompt
allocation, so focusing a panel cannot shrink the input. Panels advertise
their keys once, next to the input those keys modify, rather than growing
per-panel footers.

**A docked panel advertises itself before you go in — but only a *hosted* one.**
The host used to return `None` for any unfocused panel, so a docked `/files`
showed its keymap only after the user had already guessed the keys correctly. It
now asks every panel regardless of focus and passes honest focus state plus the
edge it is docked to, so the line can lead with the gesture that makes the verbs
live — `Files · Shift+→/Shift+L in · j/k move · …` — and swap that for
`Shift+← back` once the panel holds the keyboard. The gesture comes from
`PanelPlacement::toward`/`away` and `PanelDirection::gesture_key`, the same
values the dispatcher parses, so it cannot name a key that does not work there.

**The exception is by kind, and the Terminal panel is on the declining side.**
That "advertise without the keyboard" contract belongs to a **hosted** panel —
an adopted box whose verbs exist nowhere else. A **bespoke** panel holds a live
projection the reader can already see and whose chrome names it, so it declines
when it does not hold the keyboard, and the row goes away. The terminal panel
was the one panel keeping an unfocused branch, and the branch outlived its
reasoning: `"TERM · focus to scroll, select and type"` — and the
`Shift+←/→` gesture it advertised — all need the keyboard the composer had just
taken, so the row persisted above the LLM buffer describing a terminal the
reader was not in. `TerminalPanel::composer_hints` now answers `None` when
`!hints.focused`.

Two things follow, and both are the point rather than a side effect:

- **The input box does not move.** `session_layout` gives the prompt chunk a
  fixed, bottom-anchored height, and the working strip, hint line and todo strip
  are carved off its **top** in that order. Removing the hint therefore shrinks
  the carved region and hands the row to the transcript (`Min(10)`) — the only
  chunk that absorbs slack — while the composer stays on its row. Stated
  explicitly because the arithmetic reads as though the opposite should happen:
  a hint row that appeared or vanished and dragged the input with it would
  reflow the frame under the user's hands every time a panel took or lost focus,
  which is the same defect the working strip's reserved row 0 exists to prevent.
- **The working strip's reserved row 0 is untouched.** It is reserved
  unconditionally, and `Min(10)` for the transcript holds either way — the hint
  row is a separate row carved from the prompt chunk.

What each panel does when it does *not* have the keyboard is the panel's own
decision, and the split is deliberate. The session, chat, help and agents
panels decline — a pre-docked session column is on screen from the first frame
of every session, and making it announce itself would put a key line nobody
asked for permanently under the composer. A panel the user *docked* speaks.
An empty panel says nothing either way: `focus` refuses one, so its verbs are
unreachable and naming the command that would load it is advice for someone who
is not in that column.

**Panel manager** (`Shift+S`): a box listing every
registered panel with its `label`, its `hint`, and the digit already bound to
it. It carries **NOR/INS**, like every other dockable box, because it has to
answer two questions whose keys collide. It opens in **Normal** — it is reached
with a Normal-mode chord and is read before it is typed into, and opening it in
Insert made `j`/`k` filter text, which is indistinguishable from a dead pane.

- **Normal** — `j`/`k`/`↑`/`↓`/`PgUp`/`PgDn` move the highlight;
  `Shift+H/J/K/L` or `Shift+←/→/↑/↓` **put the highlighted panel on that edge**;
  `Enter` loads it onto the bracketed edge; `1`-`9`/`0` bind and persist it;
  `p` steps the bracketed edge; `i`/`a` starts the filter.
- **Insert** — type to filter, `Backspace` edits, `Esc` returns to Normal with
  the query intact. `Esc` from Normal clears the filter first and closes only
  when there is nothing to clear.

The Normal directional is the reason the box needs a mode of its own. `Shift+H`
is the letter `H`, so resolving it as a gesture unconditionally is wrong in a
box with a query buffer — but gating it on Normal buys something the other
boxes do not have, because there the gesture is free to mean *move the
highlighted panel* rather than *dock this box*. It is deliberately **not**
`TuiMsg::MoveDirection`: that resolves the keyboard position between the edges
already in use, which is the wrong question while the manager is modal, since
no panel is focused to move.

Moving and loading stay separate acts (`MovePanelTo` vs `SelectPanel`) for the
same reason walking the panel list never materializes an empty panel: a gesture
that silently starts a daemon load is not a positioning gesture. The box stays
open across a move — arranging three panels is one continuous act, and closing
would throw away the list mid-arrangement.

The highlight indexes [`SidePanelId::ALL`], not the filtered view, so filtering
narrows what is *visible* without renumbering what `Enter` and the digits act
on.

Three ordering rules, all load-bearing:

- **The manager hotkey is answered before the panel and before the modal.**
  A focused panel would otherwise swallow the manager key and trap the user
  inside the panel they opened it to leave; a *dialog* would swallow it too
  and stack a second manager behind the first. `App::on_key` therefore
  intercepts `InputAction::OpenPanelManager` ahead of the modal
  short-circuit when the manager is the active dialog, and
  `commands::panels::handle_panel_key` matches it before delegating to the
  panel. This is load-bearing rather than tidy: `Ctrl+Alt+W` is retired, so
  `Shift+S` is the only *chord*, and a panel that consumed it would leave no
  way to reach the switcher from inside a panel at all. `/panels` is the
  second route — the chord alone would strand the manager in Normal mode,
  since `Shift+S` is the letter `S` in Insert and has to stay typeable.
- **A toast's height is `wrapped_lines + 2`.** The live column is a fixed
  60×10 overlay at the top right (`App::render`), so each row a toast spends
  is a row of transcript it covers. The countdown bar that used to sit under
  the message cost one row per toast to restate the expiry, and read as a
  progress indicator for whatever was beside it. Nothing is lost: the
  retained ring in `/logs` is the durable record.
- **Opening the manager does not take focus; committing does.** If opening
  stole the keyboard, a user who pressed it from the composer and then
  cancelled would find their next keystroke going to a panel they never
  chose.
- **A panel refuses focus only when it has nothing at all to show.** Chat with
  no bound project leaves the keyboard in the composer rather than swallowing
  it. Terminal is deliberately the exception: `i` either takes the keyboard of
  a terminal that exists or starts a shell, so it always has something to do.
  Refusing there was the dead end — an empty panel takes no keys, so the `i`
  that would have fixed it never arrived.

**Saved hotkeys.** The manager's digits persist as `panel_hotkeys` in
`codegg.json` (`{"1": "chat"}`), written through `Config::save_global` — the
theme's path, because a panel hotkey is a per-user preference and
`Config::save` would confine it to the nearest project config. They are
recalled with `Ctrl+1`…`Ctrl+0`, resolved in `App::on_key` *before* the
focused panel: a panel that swallowed the only key that leaves it would trap
the user. An unbound slot is consumed anyway, so the digit range stays the
manager's and never leaks into the composer.

These bindings are deliberately **not** in the static binding table. That
table is keyed by key name and configurable via `ActionKey`; these are
assigned at runtime by digit, which no `ActionKey` can express.

**Why `Ctrl`, not `Cmd`.** `Cmd+1` is the natural spelling and it cannot work
in a terminal — crossterm reports no Super/Meta modifier here, `parse_key`
understands only CONTROL/SHIFT/ALT, and terminals that pass a meta byte
through disagree about the encoding. `Ctrl` is the family that is reliably
delivered, the same reason the chord family is `{CONTROL, ALT}`.

Committing to Chat also re-resolves the project binding first: the host has
no app access, so it would otherwise find the panel unbound and refuse to
focus it, leaving the selection looking inert.

`Ctrl+B` toggles the panel on the **selected** edge and focuses it; it leaves
every other edge alone, and only hands back the keyboard when the edge it
collapsed was the one holding it. `InputAction::ToggleSidebar` keeps its
original name so persisted user keybindings do not break. `Ctrl+Alt+S`
enters sidebar focus; `Shift+S` (Normal mode only) or `/panels` from anywhere
opens the panel manager.

**Chord contract.** Multi-modifier chords are `{CONTROL, ALT}` only. A bare
`Ctrl+<letter>` is unusable because every free bare Ctrl letter is a terminal
control character (Ctrl+H backspace, Ctrl+I tab, Ctrl+J newline, Ctrl+M enter,
Ctrl+V literal-next, Ctrl+Z suspend). `Ctrl+Shift+<letter>` is **also**
unusable, for a subtler reason: the app never pushes keyboard-enhancement
flags (`src/tui/terminal.rs` enables raw mode, bracketed paste, and mouse
capture only), so the terminal is in legacy xterm encoding where
`Ctrl+Shift+W` and `Ctrl+W` are the same byte `0x17`. crossterm reports
`Char('w')` with CONTROL only, the `(CONTROL|SHIFT, Char('W'))` binding is
unreachable, and the key silently falls through to whichever action owns the
bare-Ctrl twin — the panel switcher fired `CloseSession`. `Ctrl+Alt+<letter>`
has no such collision: legacy encoding sends `ESC` plus one control byte, so
crossterm reports CONTROL|ALT and the pair is unambiguous against both bare
`Ctrl+W` and bare `Alt+W`. `tests/modifier_chords.rs` asserts the whole set
stays disjoint from their unshifted twins.

This affects every multi-modifier chord, not just the panel switcher: sidebar
focus (`Ctrl+Alt+S`), model cycling (`Ctrl+P` / `Ctrl+Alt+P`), TTS stop
(`Ctrl+Alt+Y`), fullscreen (`Ctrl+Alt+F`), and project-tab navigation
(`Ctrl+Alt+N` / `Ctrl+Alt+B`). Function keys and `Ctrl+]` were rejected as
fallbacks: F1–F12 are intercepted by macOS for brightness/volume unless
"Use F1, F2 as standard function keys" is enabled, and `Ctrl+]` is already
the leave-terminal-focus chord.

Entering `Route::Workspace` still auto-selects the Chat panel, preserving the
M005 behaviour, but the choice is the user's afterwards: `Shift+S` can
switch away and back without re-entering the view.

### Chat side panel (`src/tui/panels/chat.rs`)

A peer of the `/chat` window (`components/dialogs/chat.rs`) — the same
workflow in a more convenient place — built on the **same**
`ChatBrowser`, not a parallel implementation. The slash commands stay
available so the two can be compared. Both read the same `ChatState`
projection and emit the same `TuiMsg` intents through the same `start_chat_*`
seams, and both have the full verb set: `j`/`k` move, `g`/`G`/`Home`/`End`
jump, PageUp/PageDown and `Ctrl+U`/`Ctrl+D` page, `r` reply, `e` edit, `X`
redact, `m` mark-read, `u` load older, `R` refresh, `[`/`]` switch channel.

**The panel holds no composer buffer — and that is the only difference.**
The bottom composer is the single input surface for both targets, and two live
text buffers would put two cursors on screen with nothing to say which one
`Enter` would use. `i`/`a`/`Enter` (nothing to prefill) pass through to the
composer; `r`/`e` emit `TuiMsg::ChatComposerSeed`, which arms the composer and
switches it to Insert — see
[One chat browser, two surfaces](#one-chat-browser-two-surfaces-src-tuichat_browserrs).

Bodies hard-wrap through `wrap::wrap_to_strings` with continuation lines
indented under the first body column. The panel has **no in-panel keybar**:
the keys appear on the shared hint line above the composer, so they sit next
to the input they modify rather than inside the panel they apply to.

Invariants carried over from the window:

- **The destination row is unconditional and names the armed target.** It
  draws `▸ CHAT ↵ sends here · Shift+← back` when focused and bound, drops the
  `▸` marker when blurred, and shows `No project bound` in the body rather
  than an armed-looking row when nothing is bound. Collapsing this row is what
  previously made "which panel has the keyboard" unobservable.
- **The panel's own `bound` flag gates rendering**, not just the context's
  project. `sync_chat_panel_project` sets both in lockstep, but a panel that
  has been told it is unbound must not render an armed `CHAT` destination.
- **Paging anchors on the selected message's last wrapped line.** A long
  message can span more rows than the body has, so anchoring on its first
  line would scroll the selection off-screen entirely.

Deliberate presentation divergences from the `/chat` window — the
*interaction* is now identical, and every one of these is about fitting a
narrow column:

- **Enter on an empty composer emits nothing.** `start_chat_send` rejects an
  empty body with a warning toast, so forwarding it would make a stray Enter
  on an untouched composer shout about a message the user never wrote.
- **The status row leads with `✗` and no "send failed:" label.** At 28 inner
  columns the label would push the reason — the part the user needs — off the
  end of the row.
- **There is no manual refresh key.** The `/chat` window keeps `R`; the panel
  does not.

**The destination row is unconditional** — see the chat panel section above. It
replaced the panel's own `NOR`/`INS` composer row once the panel became a
viewer; the badge moved to the bottom composer, which now renders
`[NOR|INS] [agent] [BUILD] [CHAT]` and owns both the mode and the armed
destination.

**The `/chat` window keeps its own mode badge and composer.** It reserves the
composer row in *both* modes (it previously reserved zero rows in Normal mode)
and draws an inverse-video `NOR`/`INS` badge, with `press i to type` in Normal
mode. Two further rules there are load-bearing:

- The composer renders `before + caret + after`. It used to assign the buffer
  into a `text` local and then drop it: the empty case rendered a bare caret
  and the non-empty case rendered only the slice *after* the caret, so every
  character typed left of the cursor was never drawn. That, not the missing
  row, is why "typing in the input box does not display" reproduced even in
  Compose mode.
- The placeholder is a fallback shown only when the buffer is empty, dimmed so
  it cannot be mistaken for typed text.

Making either visible is a support-question fix, not decoration: a mode the
user cannot observe is indistinguishable from a broken input.

**Refresh is event-driven.** The panel renders `ChatState` live rather than
holding a snapshot, and `refresh_chat_panel` re-binds the panel to the current
project before returning. Daemon events (`on_chat_message_committed` /
`_edited` / `_redacted`) already push into the reducer, so a message committed by
another client appears without a keystroke. `R` was removed because a visible
refresh command implies a staleness the projection does not have — a manual sync
is still available as `/chat-history`.

#### Help dialog search

`HelpDialog` is a **searchable** keybinding list, not a scrolling wall of text.
Finding one binding used to require scrolling an undifferentiated list, which is
the wrong shape for a list whose length is set by the build.

Layout is four rows: title, filter, content, footer. The filter row is always
visible and reads `type to filter keybindings…` when idle, so the affordance is
discoverable without pressing anything first. While filtering it shows the query,
a caret, and an `n/total` match count.

- **Printable keys filter.** Whitespace-separated terms are ANDed against the
  key, action, section, and condition fields, case-insensitively, so `ctrl alt`
  narrows rather than widens. `j`/`k` are typeable rather than navigational —
  they have to be searchable to find "jump" — so scrolling is on arrows,
  PageUp/PageDown, Home, and End.
- **Esc is two-stage.** The first Esc clears the filter; the second closes. A
  single Esc never discards a query the user is midway through typing.
- **The query is bounded** at 64 characters so a held-down key cannot grow
  dialog state without limit.
- **New query resets scroll to the top.** A narrowed list is otherwise left
  scrolled past its own end.

Rows come from `help_entries_for_mode`, with the vim-mode gating rule applied
identically to `build_help_lines` so filtering cannot surface an entry the
unfiltered view hides. When the query is empty the dialog renders
`build_help_lines` output unchanged, so the idle view is byte-identical to the
pre-filter dialog.

`HelpDialog::new` accepts pre-rendered `Vec<String>` lines and therefore has no
structured rows; filtering is inert there and scrolling still works.

### Keyboard sidebar and agent-tree inspection (M009)

`SidebarWidget` is a bounded projection/index, not an owner of run, job, or
worktree state. Its `SidebarFocusTarget` identities are logical rows (section,
task, run, tool-program, or convergence IDs), so keyboard navigation and mouse
hit testing resolve the same target even when scrolling or collapse changes
rendered line positions. Selection is presentation-only and is cleared when
the active project scope changes.

Normal-mode `Space` opens help — a **searchable** keybinding list (see
[Help dialog search](#help-dialog-search)) — and `Ctrl+Alt+S` enters
sidebar focus; `j/k` or the arrow keys move, PageUp/PageDown move by a bounded
viewport step, `h/l` collapse/expand or move to an agent parent, Space toggles
a collapsible row, Enter inspects, and Esc returns focus to the prompt.
`Ctrl+B` toggles the sidebar and focuses it when opened; `Ctrl+T` is reserved
for the todo strip above the input. `Ctrl+Alt+S` is used for sidebar focus
under the chord contract above. Modal dialogs remain above this surface under
the canonical `FocusManager`.

The Agent Runs section renders the active projection turn's bounded
`agent_tree` in deterministic parent-first order with indentation, status,
attention, progress, and joined branch/worktree/result-commit hints. A tree
node joins a durable run only when its projection `task_id` exactly matches
the durable summary's `task_id`; otherwise it remains a partial task row.
Detached durable runs remain inspectable. Enter delegates to the existing lazy
`OpenRunDetail`/RunStore surface and never copies transcripts, logs, or diffs
into sidebar state. The tree is capped independently at 64 source nodes and
inherits the projection's reconnect/replay bounds.

**Subagent and job sections** sit between `Todos` and the file-change/MCP
sections, so the three "what is this waiting on" surfaces read as one block and
match the overlay's todos → subagents → jobs order. Rows come from the **same
`SubagentRow` vector** already handed to `SubagentState::set_rows` for the main
buffer strip and the overlay: one derived value rendered three ways, which is
what guarantees one name per subagent across surfaces. Jobs come from
`snapshot.jobs` filtered by the shared `state::tasks::is_live_state`, so
"waiting on" is one rule rather than two that drift.

Four properties here are deliberate, and each has a test named after it:

- **The agent tree keeps `Enter`.** Adding a second adjacent subagent surface
  made reusing `Enter` on the tree rows tempting; refusing it keeps a working
  surface working. "What happened to this run" (run-detail dialog) and "show me
  its transcript" (buffer tab) stay separate questions on separate rows
  (`the_new_sections_do_not_take_over_the_agent_tree_activation`).
- **A job row activates to nothing.** There is no job transcript and no job
  dialog, so `Enter` consumes the key via `SidebarActivation::None` rather than
  fabricating a destination
  (`a_job_row_is_read_only_and_never_reaches_the_composer`).
- **Job focus targets key on `job_id`, not list index.** Jobs are upserted
  *and* filtered to live states, so an earlier job finishing shifts every
  later index down by one; an index-keyed target would silently move focus onto
  a different job and then activate it. Todos and file changes still use
  indices because the projection only appends to them.
- **The subagent section keeps terminal rows.** Filtering here — as the overlay
  does — would make a finished subagent vanish while its tab may still be open,
  which is the exact failure D3 asks to watch for. Status is drawn per row
  instead, and when the tree drops the node the row goes and the tab shows its
  `Detached` state. This settles D3 in favour of agreement with the tab set.

A missing snapshot does **not** clear the subagent section: `set_subagents` is
called only with rows an arriving snapshot reported, matching
`SubagentState::set_rows`. A projection that has not spoken is not evidence that
no subagents exist. Jobs do clear, consistent with the sidebar's neighbouring
setters.

Routing is the ordinary panel path — `SidebarActivation::InspectSubagent` →
`PanelKey::Intent(TuiMsg::OpenSubagentTab)` → `App::open_subagent_tab`. The
panel never calls `process_msg` itself; only `App::on_mouse`'s
`ClickTarget::Sidebar` arm does, which is what keeps the click and Enter routes
identical (`a_clicked_subagent_row_opens_the_same_tab_as_enter`).

### UiState (`src/tui/app/state/ui.rs:40`)

```rust
pub struct UiState {
    pub theme: Arc<Theme>,
    pub layout: TuiLayout,
    pub sidebar_visible: bool,
    pub auto_scroll: bool,
    pub show_thinking: bool,
    /// Per-turn extended-thinking selection (`/reasoning`).
    /// Frontend-local; sent on each `TurnSubmit`, never persisted.
    ///
    /// When the cycled mode actually requests reasoning
    /// (`ReasoningMode::requests_reasoning`) and the selected model's provider
    /// strips `reasoning_effort` on the wire, the confirmation is raised as a
    /// warning instead of an info toast. The provider is derived from the
    /// first segment of the `provider/model` string via
    /// `App::selected_provider_strips_reasoning`, which consults
    /// `ProviderCapabilities::for_provider` — never a TUI-maintained list. An
    /// unresolvable provider stays silent rather than warning speculatively.
    pub reasoning_mode: ReasoningMode,
    pub show_timestamps: bool,
    pub routes: RouteManager,
    pub dialog: Dialog,
    pub command_mode: bool,
    pub input_mode: InputMode,
    pub shutdown_tx: Option<broadcast::Sender<()>>,
    pub help_lines: Vec<String>,
    pub bindings: HashMap<(KeyModifiers, KeyCode), InputAction>,
    pub keybinds: Option<KeybindConfig>,
    pub vim_mode: bool,
    pub mode: AppMode,           // Embedded | RemoteCore { endpoint }
    pub remote_status: Option<String>,
    pub running: bool,
    pub timeline_visible: bool,
    pub timeline_selected: usize,
    pub render_panic_count: usize,
    pub last_render_error: Option<String>,
    pub tts: Tts,
    pub tts_enabled: bool,
    pub fullscreen: bool,
    pub dirty_regions: Vec<Rect>,
    pub resize_debounce: Option<std::time::Instant>,
    pub tts_via_daemon: bool,
    pub diagnostics: TuiDiagnostics,
    pub plugin_ui_caps: PluginUiCapabilities,
}
```

### SessionState (`src/tui/app/state/session.rs:71`)

```rust
pub struct SessionState {
    pub session: Option<Session>,
    pub session_status: SessionStatus,
    pub token_in: u64,
    pub token_out: u64,
    pub live_output_tokens: u64,
    pub live_output_text: String,
    pub reasoning_tokens: usize,
    pub cached_tokens: u64,
    pub history: VecDeque<HistoryEntry>,
    pub history_pos: Option<usize>,
    pub indexed_files: Arc<RwLock<Vec<String>>>,
    pub project_dir: String,
    pub last_edited_file: Option<String>,
    pub changed_files: Vec<ChangedFile>,
    pub mcp_servers: Vec<(String, String)>,
    pub context_tokens: usize,
    pub context_limit: usize,
    pub compaction_count: usize,
    pub rpm_limit: Option<u64>,
    pub tpm_limit: Option<u64>,
    pub rpm_remaining: Option<u64>,
    pub tpm_remaining: Option<u64>,
    pub permission_pending: bool,
    pub subagent_count: usize,
    pub git_sidebar: GitSidebarState,
}
```

### AgentState (`src/tui/app/state/agent.rs:6`)

```rust
pub struct AgentState {
    pub snapshot: Option<Arc<ProjectAssetSnapshot>>,
    pub agents: Vec<Agent>,        // Legacy cache; prefer snapshot
    pub current_agent: usize,
    pub current_model: String,
    pub models: Vec<String>,
    pub model_idx: usize,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
}
```

### DialogState (`src/tui/app/state/dialog.rs`)

`DialogState` owns non-visual request metadata, pending authorization, and
async request generations. Mounted modal components are owned by
`FocusManager`; it is the only writable owner used for render and input.
The command palette remains prompt-completion state rather than a modal.

Async request states (all 22 `AsyncUiRequestState` fields):
`import_request`, `research_request`, `session_reload_request`,
`task_list_request`, `task_delete_request`, `task_sheet_request`,
`work_order_create_request`, `work_order_list_request`,
`work_order_reorder_request`, `work_order_mutation_request`,
`work_order_occurrence_request`, `task_model_pref_request`,
`team_request`, `control_request`, `trigger_create_request`,
`trigger_manage_request`, `worktree_list_request`,
`template_create_request`, `session_mutation_request`,
`session_messages_request`, `test_run_request`, `terminal_request`.

`DialogState` also holds the M003 Task sheet draft
(`task_schedule_draft`, `None` = closed), the bounded project Task
view cache (`task_view`: rows/summary/lanes/selection/generation —
display only, daemon owns truth), and the M005 Workspace primary-view
cache (`workspace_dashboard: Option<WorkspaceDashboardState>` — rows,
filter/selection, generation, one inline expansion; `None` = view
closed, display only, daemon owns truth). The selected project is an
explicit routing locator (`selected_project_id`, generation + reconnect
guarded); `workspace_focus` (`Composer`/`Chat`) owns input focus while
`Route::Workspace` is active.

Pending fields: `permission_perm_id`, `permission_queue` (approvals waiting
behind the one on screen — see *Pending approvals are a queue*),
`question_session_id`,
`pending_delete_session`, `pending_archive_session`,
`pending_bulk_delete`, `pending_bulk_delete_ids`,
`pending_bulk_archive`, `pending_bulk_archive_ids`,
`pending_shell_command`, `pending_connection_lifecycle`,
`shell_detail_id`, `terminal_detail_handle`.

Plugin dialogs stored in `PluginUiState`, not `DialogState`.
A single `Dialog::Plugin` variant handles all plugin dialogs.

### Dialog Variants (`src/tui/app/types.rs:2`)

```rust
pub enum Dialog {
    None, Model, Agent, Session, Help, Tree, Files, Theme,
    Question, Permission, PanelManager, Mcp, Keybind,
    Share, Import, Template, Connect, ConnectionSelection,
    Context, Cost, Usage, Stats, Goto, Plan, Diff, Confirm,
    Review, ResearchBrowser, SecurityReview, SourcePreview,
    ShellShow, Terminal, TaskList, WorktreeList, GoalShow, MemoryResults,
    DoctorReport, Logs, Plugin, RunDetail, ProjectPicker, Collaborators, ProjectChat,
    Team, Control, TeamTokenSecret, TriggerSecret,
    TaskSchedule, TaskView, WorkspaceDashboard, PolicySelector,
}
```

51 variants.

The `Terminal` dialog renders one interactive terminal view (M003): an
`InfoDialog` with `InfoType::TerminalShow` driven by the live
`FocusManager` component and `DialogState::terminal_detail_handle`. It is a
projection of the M002 protocol, not a PTY widget: headers describe
link/focus/size/cursor state, output shows the newest bounded lines,
and keyboard bytes reach the process only under explicit focus
(`i` focuses, `Esc` unfocuses, `Esc` again closes with detach).

### DialogType (`src/tui/components/component.rs:22`)

```rust
pub enum DialogType {
    Share, Model, Agent, Session, Help, Tree, Files, Theme,
    Permission, PanelManager, Mcp, Question, Diff, Import, Template,
    Connect, ConnectionSelection, Keybind,
    Context, Cost, Usage, Stats, Goto, Plan, Review, Confirm,
    ResearchBrowser, SecurityReview, SourcePreview, ShellShow, Terminal,
    TaskList, WorktreeList, GoalShow, MemoryResults,
    DoctorReport, Logs, Plugin, RunDetail, Collaborators, ProjectChat,
    Team, Control, TeamTokenSecret, ProjectPicker, TaskSchedule, TaskView,
    TriggerSecret, WorkspaceDashboard, PolicySelector, None,
}
```

51 variants, matching `Dialog` 1:1 through the exhaustive compatibility
conversions.

`TaskSchedule` is the WorkOrder scheduling sheet
(`components/dialogs/task_schedule.rs`): seeded buffers for
delay/not-before/repeat, sequential + lane cycling, All/Any join,
catalog-bounded model cycling, policy/workspace/queue summaries, and a
disabled external-trigger row. Tab/Shift+Tab move sheet focus (consumed
by the modal, never the agent selector); `j`/`k` stay unbound. Enter on
Confirm emits `TuiMsg::TaskScheduleConfirm` for App-side validation.

### `/connect` vs `/connections`

`/connect` (`components/dialogs/connect.rs`) adds and configures provider
connections: catalog loading state, provider-list selection with keyboard
(arrows/`j`/`k`/`Enter`/`Esc`) and mouse hit-testing, then the typed form
for the chosen entry (secret-only, optional endpoint override, required
endpoint, or the Eggpool host/port/TLS preset, plus an API-key/bearer
choice when the catalog admits both). Completion is secret-free with
operation-ID stale protection and refreshes the existing
connections/model projections. `/connections`
(`components/dialogs/connection_selection.rs`) only inspects, manages,
and selects already-durable connections; it never collects secrets.
While `loading` is true the dialog renders only the
"Loading connections and models…" placeholder, so `handle_key` accepts
dismissal alone — `d` (delete) and `p` (purge) are single-key destructive
actions with no confirmation, and a blind press during the load window must
not be able to act on rows the user cannot see.

`TaskView` is the project Task view
(`components/dialogs/task_view.rs`): renders the `TaskViewSnapshot`
synced from `DialogState.task_view` (RUNNING / FUTURE-WAITING /
NEEDS-ATTENTION / RECENT). `j`/`k`/arrows navigate, Shift+J/K reorder
waiting lane members, Enter opens materialized sessions (future rows
fetch detail), `r`/`d`/`x`/`u` refresh/detail/cancel/resume.

`WorkspaceDashboard` is the M005 Workspace primary view
(`Route::Workspace`, `commands/workspace_dashboard.rs`,
`app/state/workspace_dashboard.rs`): a non-modal top-level view that
keeps the ordinary bottom Session/Task composer editable and shows
project chat for the selected project in the sidebar region (existing
`ChatState`/`chat.v1`, no second cache). One `WorkspaceDashboard`
aggregate per refresh (never N+1); Up/Down/PgUp/PgDn/Home/End and
Normal-mode `j`/`k`/`g`/`G` navigate, Normal-mode unbound text filters,
`Space` expands one project's running tasks (single bounded
`WorkOrderList`, `Tab` stays composer-mode owned), `Ctrl+R` refreshes,
`Enter` with empty prompt descends to the Task view while `Enter` with
text submits the composer for the selected project, `Esc` blurs chat to
the composer first and leaves the view only from composer focus.
Composer routing resolves the selected project through an open tab and
fails visibly without cwd/hidden-session fallback; chat focus owns
draft editing/Enter send per project. Event hints mark rows dirty
without focus theft; reconnect resyncs; revocation clears the row plus
that project's chat cache. Opened by `/workspace` and the configurable
`OpenWorkspaceDashboard` action (`Ctrl+O`). The obsolete
`Dialog::WorkspaceDashboard` modal is retained only as a compatibility
shim and is never pushed for normal navigation.

**Workspace cancellation ownership** (post-closure M002): leaving the
view (`leave_workspace_view` or the legacy `Dialog::WorkspaceDashboard`
teardown) cancels only `TuiTaskKind::Workspace` tasks — the dashboard
refresh and inline-expansion fetches spawned by
`commands/workspace_dashboard.rs`. Unrelated generic `Command` tasks
(chat, team administration, control, provider, project/session,
diagnostics, WorkOrders) keep running, per-project chat drafts/cache in
`ChatState` survive, and no daemon turn/job is ever cancelled.
Cancellation is best-effort resource hygiene; the correctness boundary
remains generation/request/reconnect fencing, so a completion racing
with close observes the dropped dashboard state and does nothing.

`Dialog` and `DialogType` have exhaustive compatibility conversions. The
canonical lifecycle identity is the live component's `DialogType`; the
`ui_state.dialog` value is only a derived compatibility mirror.

### Component Trait (`src/tui/components/component.rs:183`)

```rust
pub trait Component: Send + Any + AsAny {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn handle_paste(&mut self, text: String) -> Option<TuiMsg> { None }
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool { self.dialog_type().is_modal() }
    fn hit_test(&self, rel_y: usize) -> Option<usize> { None }
    fn set_selected(&mut self, idx: usize) {}
    fn focus_next(&mut self) {}
    fn focus_prev(&mut self) {}
    fn focusable_count(&self) -> usize { 1 }
    fn focused_index(&self) -> usize { 0 }
    fn set_focused(&mut self, idx: usize) {}
}
```

### FocusManager (`src/tui/components/component/focus.rs`)

```rust
pub struct FocusManager {
    stack: VecDeque<Box<dyn Component>>,
}
```

Key methods: `push(component)`, `pop()`, `top()`/`top_mut()`,
`with_dialog(_mut)`, `with_component`, `handle_key(key)`,
`active_dialog_type()`, `len()`. Focus belongs to each component, so nested
push/pop preserves the underlying modal's selection. `Tab` and `Shift-Tab`
wrap against the current component count, including zero and one-control
dialogs.

### TuiMsg (`src/tui/app/types.rs:123`)

116 variants — synchronous intent from TUI to App, consumed by
`App::process_msg`. Grouped by intent:

| Group | Variants |
|---|---|
| Prompt/navigation | `SubmitPrompt`, `NavigateUp`/`Down`/`Left`/`Right`, `PageUp`, `PageDown`, `Search`, `SearchNext`, `SearchPrev`, `ClearSearch`, `FocusPrompt`, `StashPrompt`, `RestorePrompt` |
| Exit | `Quit` (`q` in Normal mode, `/exit`, Ctrl+Q), `ForceQuit` (Ctrl+C) |
| Text editing | `CharInput`, `Backspace`, `Delete`, `CursorLeft`/`Right`/`Home`/`End` |
| Dialog opening | `OpenModelDialog`, `OpenAgentDialog`, `OpenSessionDialog`, `OpenHelpDialog`, `OpenTreeDialog`, `OpenThemeDialog`, `OpenShareDialog`, `OpenImportDialog`, `OpenDiffDialog`, `OpenSourcePreview`, `OpenRunDetail`, `OpenProjectPicker`, `OpenWorkspaceDashboard`, `CloseDialog` |
| Dialog selection | `SelectModel`, `SelectAgent`, `SelectSession`, `SelectTheme`, `ThemePreviewChanged`, `ThemeCommit`, `ThemeRevert`, `KeybindChanged`, `SelectTemplate`, `GotoMessage`, `CopyShareUrl`, `McpAction`, `ConnectionLifecycle`, `ProviderConnectionsUpdated`, `ProviderConnectionModelsUpdated`, `OpenConnectionRotation` |
| Dialog submit/confirm | `SubmitConnect`, `SubmitPermission`, `SubmitQuestionAnswers`, `SubmitImportPreview`, `ConfirmImport`, `ConfirmResult`, `ConfirmDeleteSession`, `ConfirmArchiveSession`, `ConfirmBulkDelete`, `ConfirmBulkArchive`, `SubmitSelectionUpdate`, `SessionSelectionUpdated` |
| Session lifecycle | `NewSession`, `CloseSession`, `ClearSession`, `CycleAgent`, `CycleModelForward`, `CycleModelBackward`, `ForkSession`, `ForkTreeSession`, `SelectTreeSession`, `UndoDelete`, `Quit` |
| Toggles | `ToggleSidebar`, `ToggleTasksList`, `ToggleFullscreen`, `ToggleReasoning`, `ToggleTts`, `ToggleComposerMode` |
| Project tabs | `NextProjectTab`, `PreviousProjectTab`, `CloseProjectTab`, `SelectProjectTabByIndex` |
| Research/shell/git | `ResearchOpenRun`, `ResearchRefreshRuns`, `ResearchLoadSection`, `SecurityReviewJump`, `ReviewOpenDiff`, `ExternalEditor`, `CopyMessage`, `ShellInclude`, `ShellAsk`, `ShellRerun`, `ShellKill`, `RunRerun`, `RunPromote`, `RunCopyId` |
| Workspace dashboard | `WorkspaceDashboardMove`, `WorkspaceDashboardOpen`, `WorkspaceDashboardRefresh`, `WorkspaceDashboardToggleExpand` |
| Task view | `TaskViewMove`, `TaskViewReorder`, `TaskViewOpen`, `TaskViewRefresh`, `TaskViewCancel`, `TaskViewResume`, `TaskViewDetail` |
| Task schedule / triggers | `TaskScheduleConfirm`, `TaskScheduleMove`, `TaskTriggerSetup`, `TaskTriggerRevoke`, `TaskTriggerRotate`, `TaskTriggerRefresh`, `TriggerSecretClose`, `TeamTokenSecretClose` |

### TuiCommand (`src/tui/app/commands.rs:15`)

### Esc never quits; exiting is explicit

`Esc` is `InputAction::Cancel`, and `App::cancel()` unwinds exactly one
layer — close the top dialog, else dismiss the completion popup, else
Insert → Normal, else discard a non-empty draft — and then stops. It has
no exit branch. That is deliberate: `Esc` is the reflexive "get me out
of this" key, and binding the last layer to program exit meant one stray
press in Normal mode tore down the whole session and its scrollback.

Exiting is a separate, explicit action with its own binding:

| Key | Action | Notes |
|---|---|---|
| `q` | `Quit` | Normal mode only; Insert mode skips bare-char bindings so `q` still types |
| `/exit` | `Quit` | `command.rs:488`; `quit` and `q` are aliases |
| `Ctrl+C` | `ForceQuit` | Unwinds the same layers as `Esc`, then exits |

`Ctrl+C` and `Esc` are distinct `InputAction`s (`ForceQuit` was added
for this) so the back-out key and the terminate key cannot drift into
the same behavior again. Like `Esc`, `Ctrl+C` dismisses an open dialog
before it exits, so a dialog needs two presses to leave.

### Model selection is durable

The daemon resolves every turn from its durable `SessionSelection`, not
from what the TUI displays. Committing a choice in the `/model` dialog
therefore performs two distinct writes:

1. `App::persist_model_selection` — the last-used **preference**
   (`KEY_MODEL_LAST_USED`), which the next launch restores.
2. `App::persist_durable_model_selection` — `CoreRequest::ModelSelect`,
   the compatibility adapter over the durable `SelectionService` that maps
   a `provider/model` catalog id onto the explicit connection + model the
   daemon stores. It carries the same authorization, audit and CAS
   revision checks as a `SessionSelectionUpdate`.

Only (2) makes the choice take effect on the next turn. When it existed
before (1) alone, the status line, the tab and the preference all showed
the newly chosen model while the turn still ran the previously durable
one — a silent divergence with no error anywhere. A refusal is reported
through `TuiCommand::ModelSelectPersisted`, which re-synchronizes the
display with the authoritative durable value instead of leaving a model
on screen that will not run.

`App::adopt_persisted_tab_model` and
`App::reconcile_durable_selection_model` remain the inbound direction:
the daemon's durable selection converging on the display.

Pinned by `scripts/check_model_select_convergence.py` and
`tests/tui_model_selection_persist.rs`.

Async commands sent via channel — 205 variants covering: session CRUD,
archive, fork, bulk ops, share, export, rename, undo delete, import,
template creation, session message loading, subagent spawn, task/worktree
operations, memory operations, goal lifecycle, research browser, doctor,
security review, git sidebar, plugin commands, test run, shell operations,
provider connection lifecycle, project catalog, file diff stats, and
completion/result variants for each async operation. Interactive
terminal operations (M003) follow the same pattern:
`TerminalShow` (synchronous render) plus `TerminalCreateFinished`,
`TerminalListFinished`, `TerminalAttachFinished`,
`TerminalResumeFinished`, and `TerminalOpFinished`
(input/resize/detach/terminate/remove) completions, all guarded by
`DialogState::terminal_request` so stale completions are dropped.

Project Work Orders M003 completions follow the same pattern through
`src/tui/commands/work_orders.rs`: `TaskSheetPrefetched`,
`WorkOrderCreated`, `TaskViewRefreshed`, `TaskOccurrenceLoaded`,
`LaneReordered`, `TaskMutationFinished`, `TaskSessionFocus`,
`TaskModelPrefetched`, `TaskModelPrefSaved`, plus C001 trigger
completions `WorkOrderTriggerCreated`, `WorkOrderTriggersListed`, and
`WorkOrderTriggerRevoked`. Every completion carries its captured
`UiRouteToken` plus request/generation identity; stale ones (tab
switch/close, rebind, reconnect, superseding refresh) drop at apply
time. Slash surface: `/tasks` and `/task` open the WorkOrder Task view
(capability-gated, legacy fallback with diagnostic), `/schedules`
keeps low-level schedule diagnostics.

### Routes (`src/tui/route.rs`)

```rust
pub enum Route {
    Home,
    Session(String),
    Workspace,  // Team Collaboration M005
    Editor,     // Desktop Frontend M006-A
}
```

`Workspace` and `Editor` are non-modal primary views: both keep the
ordinary Session/Task composer editable and the sidebar rendering while
they are active, and both identify themselves only by this route plus
their own state record. A route never confers project authority; the
project and workspace come from the active tab's explicit
`ProjectExecutionContext`.

### Document editor (M006-A)

The `Route::Editor` primary view renders a buffer from the shared M005
`DocumentController` optimistic replica. The ownership model is ADR-0011:
the daemon owns canonical text, the controller owns the local replica, and
the frontend owns presentation and undo/redo.

**Single-buffer invariant.** The frontend holds no text. Every render reads
the replica through `DocumentController::try_snapshot()`, whose
`DocumentSnapshot` is a handle to the controller's own buffer and is dropped
with the frame. `try_attachment_info()` returns line count, writer lease,
and conflict flags for chrome that must survive a frame, and carries no
text. `scripts/check_tui_editor_text_authority.py` fails the build on a
whole-document field or a filesystem read anywhere in the editor path, and
passes for a scoped `&DocumentSnapshot` borrow and for the bounded
`TextTransaction` undo history.

**Presentation record.** `TuiDocumentPresentation` owns cursor byte,
selection range, viewport line and column, buffer mode, a two-byte pending
command prefix, the open insert session, and the **edit history**. The
history is **one ordered list with a cursor in it** — `history`,
`history_bytes` and `history_cursor` — not two stacks. `history_cursor` is
the number of *applied* entries: `0` is the beginning of the history
(everything is redoable) and `history.len()` is the tip (nothing is
redoable). An entry never moves between containers, so "undo" and "redo" are
the same movement in opposite directions, and an entry cannot lose the
direction it is not currently using.

Undo entries are capped at **256 entries and 4 MiB in total** — one cap over
the history, not one per direction — and retain *both* the forward and
inverse transactions, so a traversal in either direction is exact. The
two-stack model enforced the bounds twice, so its worst case was twice this;
the single history is what makes the code match the documented bound.

**One history and a time cursor.** `u` and `U` move the cursor back and
forward one entry. `Alt+u` and `Alt+U` — Helix's `earlier` and `later` — do
the same and **report** their boundary: `Alt+u` at the oldest change says
"no earlier change", and `Alt+U` at the newest says "no later change". `u`
and `U` stay silent at the boundary, as they always have. That divergence is
a CodeGG convention, not a Helix behaviour — in Helix all four are the same
movement — and it is recorded as one.

The cursor's position is **observable**: while it is not at the tip, the
editor's banner row says `history <at>/<of>`, and at the tip it says
nothing, so the ordinary case draws exactly what it drew before the cursor
existed. A cursor nobody can see is a rename.

The cursor moves **only after** the apply is accepted. A refused apply
leaves the document, the history and the cursor exactly where they were: a
cursor that moved without a matching change of text would leave the history
describing a document that does not exist.

**A typing burst is one undo step.** The open insert session
(`TuiDocumentPresentation::insert_session`) accumulates the edits of one
`INS` burst and commits them as a single `EditorUndoEntry` when insert mode
ends, so `u` after leaving `INS` takes back the whole burst rather than its
last keystroke. That is Helix's undo checkpoint at the same granularity, and
it is why the session exists at all: without it, every character of a word
would be a separate step, and one `u` would take back one letter.

A run is **one contiguous insertion**. A deletion, a replacement, or an
insertion anywhere but the end of the run ends it, and the parts become
separate steps, because they are separate edits. The session keeps the
individual `TextTransaction`s it was given and never a copy of the text they
produced; the merged insertion is built once, at commit time.

**The first edit of a run is asked the same question as every later one.**
`absorb_insert_edit` asks whether an insertion continues the run by comparing
its start with the end of the run so far. The *first* edit had no such
question asked of it — it opened the run wherever it landed — and the
open-line verbs are the first edit to need one. `o` inserts its newline at the
**end** of the line it opens and leaves the cursor at its **start**, so the
text the reader types goes in *before* the newline. A run whose text does not
end where the cursor is cannot be continued, so `submit_edit` now asks
`typing_run_continues` before folding an edit into a run: the cursor must end
where the inserted text ends. The open-line edit is therefore not a typing
edit — it is its own undo step, and the typing that follows opens the run. The
question is on the **cursor**, not on the mode: a mode check would also have
excluded the open-line edit, but it would have excluded the literal insertion
as well, and the literal insertion is how the model's own burst test drives a
run.

A reconciliation **discards** an open session rather than committing it. The
text the session described is the pre-reload text, and committing it would
record a step that restores a document the reader is no longer looking at.
The redo tail is cleared when the session **opens** and only then: a reader
who undoes and then types must not have the redo tail resurrected by the
next keystroke of the new burst.

**Reconciliation clears history.** Open, reload, resync, reconnect, and
close all clear undo and redo. Save does not: it writes the same local text,
so undo stays meaningful. A divergent draft is retained by the controller in
a recovery state; the editor surfaces that state and never auto-reloads,
auto-replays, or auto-discards.

**One mode axis, not two.** The editor route used to carry an `EditorFocus`
(`Composer`/`Buffer`) *beside* `EditorMode`, and the two could disagree. They
did: a fresh open defaulted to `Composer`, which made `handle_editor_key` claim
nothing except `Ctrl+E` while the bare-text gate swallowed every printable —
so the file rendered and no key reached it, which reads as "the editor does not
work" rather than as two toggles. `EditorFocus` is gone.

The single axis is `App::ui_state.input_mode`, the same `NOR`/`INS` the
composer badge and every dockable box already show. On `Route::Editor`:

| Mode | Keyboard | Typing goes to |
|---|---|---|
| `Insert` | the document | the text box — the cursor is drawn there |
| `Normal` | the Helix verbs (`hjkl`, arrows, `Ctrl+d`/`Ctrl+u`, `e`/`b`, `v`, `x`, `d`, `space y`, `0`, `$`, `gg`, `G`, `:`, `/`, `n`, `N`, `i/a/I/A/o/O`, `u`, `U`, `Alt+u`, `Alt+U`) | the bottom input box |

The `Insert` row's keyboard is the document's own: printable characters, the
arrow keys, and the Helix insert-mode chords — `Ctrl+h`, `Ctrl+d`, `Ctrl+j`,
`Ctrl+k`, `Ctrl+u`, `Ctrl+w`, `Alt+Backspace`, `Alt+d`, `Alt+Delete` — plus
`Ctrl+x` for the completion popup. A chord may mean one thing in `Normal` and
another in `Insert` (`Ctrl+d` pages down in one and deletes one character
forward in the other); each still has exactly one owner, and the owner is
the stage that claims it.

**A file opens in `Normal`, not `Insert`.** This is the same choice
`/terminal`, the panel manager, and the chat box make: they are read before
they are typed into, and `i` is one keystroke away. The editor used to enter
`Insert` on open, which cost two concrete things. The first keystroke of a
mistaken open became an edit to the file. And the reader arrived on a
surface that looks like a reader and answers `j` by inserting a newline —
every navigation key silently became an edit key until `Esc`, a chord
nothing on screen named. `open_document` and `:new` both call
`enter_editor_normal`; nothing puts a freshly opened document in `Insert`.

**Normal returns `BufferKey::Unhandled` for every key it does not own**, which
is the mechanism, not a fallback: the editor claims navigation and editing and
everything else stays a composer keystroke. This is the same contract the
interactive terminal runs on — NOR reads, INS types — and the same rule
`focus_panel` follows for a side panel.

**Insert is the same contract, and so is every panel.** In `Insert` a
printable is text, and it reaches the document whatever else is on screen. The
completion popup and the LSP diagnostics panel used to claim bare characters
and `Enter` regardless of mode, so a reader typing into a file with a panel
open lost the character to a panel that had no claim on it. Both are now
mode-aware: their `j`/`k`/`r`/`Enter` claims are Normal-mode claims, and in
`Insert` the popup's `Enter` inserts a newline, its `↑`/`↓` move the cursor,
and its `Tab`/`Shift+Tab` move the candidate selection.

`Esc` is a **mode step, never an exit**. Insert → Normal. Normal → a toast
naming `:q`. It never leaves the route: the only ways out are `:q` and `:wq`,
and the frame says so. `Esc` closes an open `/` search line first, then an
open `:` command line, then steps the mode — most recent request first.

**`/` searches the document, not the command catalog.** On this route the
composer arms bare text with `:` (`composer_refuses_bare_text`), so an
unclaimed `/` used to become `:/` and resolve to no command: a key that looks
like it should do something and silently did not. `handle_normal_key` claims
it, seeds the line from the word under the cursor, and owns every key while
open — the same modal contract as `:`, including the mode-independent `Ctrl`
chords, because a leaked printable must not edit the document behind a line
the user believes is capturing it.

It searches **as you type** (`/host` → `/host_status` → `/host_status fn`),
which is the whole interaction: a needle being refined one character at a
time is not a reason to make every intermediate refinement its own round
trip. `Enter` accepts and leaves the matches painted; `Esc` abandons the whole
search, highlight included, because the match set is derived state the user
has just disowned. `n`/`N` then walk the accepted matches and are claimed
**only while a search exists**, so an `n` with no search still reaches the
composer as the start of a session name. Matches are bounded
(`MAX_EDITOR_SEARCH_MATCHES`, `MAX_EDITOR_SEARCH_BYTES`) and a partial result
says so in the status row rather than reading as a complete one.

`Ctrl+d` **continues to the last line** when a half-page would land past the
end. Half a window from the last page is an ordinary place to be, and a
motion that quietly refuses there reads as a dropped keypress rather than as
the end of the document.

Insertion entries (`i`/`a`/`I`/`A`/`o`/`O`) return `BufferKey::EnterInsert`
and the App layer adopts `Insert` mode; a read-only attachment returns
`Unhandled` so the refusal is visible rather than a dead key.

**Helix, not vi.** The buffer speaks the same dialect `/terminal` speaks in its
viewing mode, so the two surfaces share one set of instincts. vi's `dd`/`dw`
are gone — `x` then `d` is the same gesture in Helix — and `D` stays as
delete-to-end-of-line because it has no Helix meaning here. Bare `d` is
**claimed even with nothing selected**: an unclaimed `d` escapes to the global
`OpenDiff` binding. `space y` is remembered prefix state, not a chord, because
legacy xterm encoding delivers it as two events.

#### Key dispatch authority: ordered stages, one owner per chord

`editor_buffer_key` (`src/tui/commands/editor.rs`) is a **sequence of ordered
stages**, and a chord declared in a later stage is unreachable the moment an
earlier stage claims it. The order is the contract:

1. **Modal lines.** An open `/` search line, then an open `:` command line. Each
   owns *every* key, including the mode-independent chords — a half-typed `:q`
   must not be written to the file by `Ctrl+S`.
2. **Mode-independent chords.** `Ctrl+S` → `RequestSave`, `Ctrl+R` →
   `RequestResync`. Both in every mode. Stage 2 used to claim a third, the
   Normal-only `R` → `RequestReload`; M004 removed it, because Helix gives `R`
   to `replace_with_yanked` and a Normal-only guard-block `R` would have
   shadowed the verb while surviving it. Reload is `:e`
   (`EditorCommandName::Reload`), which reaches `reload_editor` directly.
   Helix gives `Ctrl+s` an undo checkpoint and `Ctrl+r` a register prompt in
   `Insert` mode. CodeGG declines both: the checkpoint is covered by the
   insert session's commit, and a document has no register to prompt for.
3. **Mode dispatch.** `handle_insert_key`, or `handle_normal_key`.

The rule that follows: **every chord has exactly one owner, and that owner is
the first stage that claims it.**

This was not a prose promise. `handle_normal_key` declared
`(KeyModifiers::CONTROL, KeyCode::Char('r')) => editor_redo`, which stage 2
already owned — so the arm was dead code and Normal mode had undo (`u`) with no
reachable redo. The suite stayed green because all three redo tests called
`editor_redo(&mut state)` **directly** instead of pressing a key: a binding test
that bypasses dispatch tests the helper, not the binding.

Redo is now `U`, which is Helix's own chord (`u` undo, `U` redo; Helix binds
`Ctrl+r` in *Insert* mode only). That keeps `Ctrl+R` as resync in every mode
with no invented chord and no second owner, so `RequestSave`/`RequestResync`
never lose a path. Two things enforce the rule:

- `scripts/check_tui_editor_key_authority.py` attributes every chord to its
  stage and fails when a mode table re-declares a chord an earlier stage owns,
  or when one table declares the same chord twice. It treats an arm shape it
  cannot resolve as an **error**, never as a pass, and asserts the stages still
  exist so it cannot pass vacuously. Its conflict rule compares **exact
  modifier bitmasks**, not modifier names: an arm that accepts exactly `ALT`
  and one that accepts exactly `ALT | SHIFT` are disjoint, exactly as Rust's
  `==` on the bitflags says they are, and the guard must not call them a
  duplicate.
- `scripts/check_tui_editor_leaf_authority.py` polices the **leaf tables**,
  which the key-authority guard scopes itself away from. It fails on a
  duplicated glyph inside one table, on a glyph that is not printable ASCII,
  on one glyph claimed by two tables of the same prefix, on a leaf whose chord
  the guard block already claims, and on a table no prefix binds. It reads the
  guard-block chords through the key-authority guard rather than re-parsing the
  dispatch chain, because two parsers for one shape is two models of it. A
  **leaf** is consulted only while its prefix is armed, and the resolver
  disarms before consulting it, so a leaf cannot shadow a directly-bound
  chord — the earlier rule that said otherwise was demonstrably false on the
  shipped tree and was replaced.
- The dispatch-path tests in `src/tui/commands/editor.rs` drive every
  Normal-mode arm through `editor_buffer_key` and assert both the reported
  `BufferKey` and the observable state change, so a shadowed arm fails on the
  state change even when the reported effect looks plausible. An
  Insert-mode sweep separately asserts that no Normal-mode binding reaches the
  Insert path and swallows a printable character, in all three spellings a
  terminal may send a capital (`Char('R')` with `NONE` *or* `SHIFT`).

**The capital-encoding rule.** A terminal may report a capital letter as the
shifted glyph (`Char('U')`) or as the plain glyph plus a `SHIFT` modifier
(`Char('u')` with `SHIFT`), and the two are not the same key event. Every
arm that claims a capital therefore accepts both spellings — `U` takes
`Char('U')` with `NONE` *or* `SHIFT`, and `Alt+U` takes `Char('U')` with
`ALT` *or* `ALT | SHIFT` **and** `Char('u')` with `ALT | SHIFT`. The two
spellings of `Alt+U` differ in the *key code*, not only in the modifier, so
they cannot share one arm; a chord that is live on one terminal and dead on
the next is the M013 failure mode, and the tests drive both.

One stage sits above this chain: `handle_editor_key` answers the App layer's
Normal-only pane gestures (`Shift+L`/`Shift+H`) before the buffer sees the key.
That overlap is deliberate and is documented at its call site.

#### The pending-prefix grammar: one prefix, one terminal key

A multi-key gesture is *remembered state*, not a chord: `space y` arrives as
two key events in legacy xterm encoding, exactly as it does in `/terminal`.
That state lives in one field, `TuiDocumentPresentation::pending_prefix`, typed
`EditorPrefix` (`src/tui/editor.rs`).

**The rule: one pending prefix, one terminal key consumed.** A key arriving
while a prefix is armed either matches a leaf or disarms it; nothing consumes a
second key, and no prefix is ever armed on top of another.
`TuiDocumentPresentation::arm_prefix` therefore *replaces* rather than stacks.

The resolution is deliberately a separate function,
`resolve_pending_prefix`, rather than a block at the top of
`handle_normal_key`: the key-authority guard scopes itself to that function's
first `match (key.modifiers, key.code)`, and a prefix resolution shaped like a
key table would make it parse the wrong one.

**The two migrated gestures disagreed, and still do.** `gg` **consumes** an
unmatched second key — it is half of a chord, so the key belongs to the chord
whether or not it completes it. `space y` **disarms and retries** it — the
prefix modifies the next key, so an unmapped second key is an ordinary
normal-mode verb. That is why `EditorPrefix::swallows_unmatched` is a property
of the *prefix* rather than a single machine-wide rule: collapsing the two
would have changed a gesture while looking like a refactor.

| Prefix | Armed by | Leaves | Unmatched key | Indicator |
|---|---|---|---|---|
| `Goto` | `g` | `g`, `s`, `e`, `h`, `l`, `j`, `k` | consumed | `g` |
| `Space` | `space` | `y` (`space y`) | retried | `space` |
| `OpenBracket` | `[` | none until M006 | retried | `[` |
| `CloseBracket` | `]` | none until M006 | retried | `]` |
| `FindNext`/`FindPrevious`/`FindTillNext`/`FindTillPrevious` | `f`/`F`/`t`/`T` | none — the next key is data | retried | the key itself |
| `ReplaceChar` | `r` | none — the next key is data | retried | `r` |
| `Match` | `m` | `m`, `s`, `r`, `d`, `a`, `i` | retried | `m` |
| `MatchArg(arg)` | `m` then a command | none — the next key is data | retried | `m s`, `m r`, `m r (`, `m a`, `m i` |

The unimpaired prefixes retry rather than swallow because their leaves are two
milestones away: swallowing there would mean a `[` followed by a letter lost
that letter on its way to the composer, and a prefix that cannot be escaped is
worse than one that occasionally passes a key through.

**Leaves are a table, not one glyph.** `EditorPrefix::leaves() -> &'static
[PrefixLeaf]` replaced M002's `leaf() -> Option<char>`. The one-glyph shape was
correct only while goto mode had exactly one leaf; Helix's has twenty and space
mode has thirty, and a single `Some` would have meant either a special case per
prefix or a second mechanism beside the state machine — which is precisely the
regression M002 deleted. A leaf row pairs a glyph with a `PrefixAction`, which
is `Motion(EditorCommand)` or `YankSelection`: a motion resolves against the
document and returns nothing, while a yank must hand a byte range back out to
the owner of the clipboard, so collapsing the two would have meant inventing a
command whose resolution is a side effect.

Every goto leaf resolves through the **raw** motion path (`editor::apply_motion`),
not the Helix selection machine. That is how `gg` has always behaved, and
routing the newer leaves through the selection machine instead would have made
`gg` the odd one out. `apply_motion` does not touch `selection` at all, so a leaf
neither extends a held selection the way `j`/`w` do nor clears one.

**That last point is a known divergence from Helix**, where a goto collapses the
selection. It is recorded rather than fixed: `apply_motion` is shared with the
bare `G`, `w` and `b`, so clearing the selection there is a change to existing
behaviour well beyond this milestone. `every_goto_leaf_leaves_a_held_selection_exactly_as_gg_does`
pins the contract that *is* owned here — parity with `gg` — so a future change
to `apply_motion` cannot quietly make the new leaves behave differently from
the old one.

**Goto leaves that are deliberately unbound.** `gd`/`gy`/`gr`/`gi` are the
language-server jumps and belong with the LSP read surface. `gf`/`ga`/`gm`/`gn`/
`gp` and `g|` need alternate buffers or a count entry point. `gt`/`gc`/`gb` —
top, middle and bottom **of the screen** — are excluded for a reason worth
stating: motions resolve against `NOMINAL_VISIBLE_LINES`, a fixed guess at the
viewport height, so "the top of the screen" would be a line that is not the top
of the screen. Binding them before the real viewport is threaded through would
ship a key that looks broken; the omission is recorded here rather than left as
silence.

**Disarm points.** A prefix is retired whenever the thing it was aimed at
stops being the thing on screen. A prefix that outlives its target is a
gesture applied to the wrong buffer — the "relative path resolved against the
wrong root" defect in gesture form.

| Trigger | Disarmed at |
|---|---|
| Opening a document | `TuiDocumentPresentation::reset_for_open` |
| Reconciliation (`resync`, `reload_from_disk`, `close`) | `editor::clear_history` |
| Leaving the editor route | `leave_editor_view` |
| Focus moving to the tree pane | `file_tree::toggle_tree`, `file_tree::set_focus` |

Two consequences worth stating. `clear_history` disarming the prefix is not a
coincidence of naming: the gesture was aimed at text that reconciliation has
just replaced underneath it. And before M002 that disarm covered only
`pending_command` — a `space` typed before a reload stayed armed and yanked in
the reloaded buffer, so the merged path removes that asymmetry rather than
preserving it.

**The indicator is part of the contract.** `space` previously had no indicator
at all, because its state was a `bool` with nothing to render. Every pending
prefix now renders a distinct label bounded by `MAX_EDITOR_PENDING_PREFIX`, and
`space` renders as the word because a literal space would put nothing on
screen. A pending prefix the user cannot see is a prefix they cannot predict.

The terminal panel (`src/tui/commands/interactive_terminal.rs`) keeps its own
`prefix_pending`/`YankPrefix` for the same gesture. Converging the two onto
one shared state machine is desirable and deliberately **not** done here.

**Terminal-panel parity for the M005 surface is an explicit exemption, not an
omission.** The panel spells the same keys (`e`/`b` by word, `space y` to copy)
but resolves them through its **own** machinery: `next_word_end`/`prev_word_start`
scan a `&str` by column index over scrollback, not by byte offset over a
document, so they are not the editor's `word_end`/`word_backward` and never
were. It does **not** gain `W`/`B`/`E` or any goto leaf, and does not need to:
it has no `g` prefix at all, its `j`/`k` scroll a viewport rather than moving
through document lines, and "first non-whitespace" has no meaning over a cell
grid. `long_word_*` was deliberately **not** introduced there — a second
word-end definition for a surface that has no long-word key would create
exactly the divergence the roadmap forbids. The one consequence to record: M005
fixed the editor scanners for non-ASCII, which does **not** fix the panel's, so
the two surfaces now agree on ASCII and may differ on multi-byte scrollback.
That is a known gap, owned by M008's panel verification, not a claim of parity.

`scripts/check_tui_editor_prefix_state.py` keeps the deletion honest: it fails
if `pending_command`, `yank_prefix_pending`, `MAX_EDITOR_PENDING_COMMAND`,
`bounded_pending`, or `push_pending` reappear anywhere under `src/`, and — the
part a fixed name list cannot express — if `TuiDocumentPresentation` declares
any prefix field other than exactly one `pending_prefix`. It ignores comments,
because the type's own doc comment is where the history of those two fields
belongs.

#### Word motions, WORD motions, and the desired column

Three scanners resolve word-shaped motions against the local replica, and the
difference between the first two is a **classification choice**, not an
optimisation:

| Helper | Classifier | Helix |
|---|---|---|
| `word_forward` / `word_backward` / `word_end` | `is_word_byte`: ASCII alphanumeric, `_`, or `>= 0x80` | `w` / `b` / `e` |
| `long_word_forward` / `long_word_backward` / `long_word_end` | `is_long_word_byte`: anything that is not ASCII whitespace | `W` / `B` / `E` |
| `first_non_whitespace` | ASCII whitespace, bounded by `bounded_prefix_end` | `gs` |

`is_word_byte`'s `>= 0x80` clause is deliberate for its own scanner: it lets a
scan classify a multi-byte character without decoding it. It is **wrong** for a
WORD, because it would make every non-ASCII byte and every ASCII punctuation
mark part of the same word as its neighbours — collapsing `W` onto `w` on
precisely the text where the distinction matters.

**All six scanners classify with `leading_byte`, never a one-byte read.**
`read_bytes(offset, offset + 1)` needs a character boundary at *both* ends, so
it is `Err` for every byte of a multi-byte character — its **first** byte as
much as its interior. Every scanner read that as "not a word byte" and walked
straight past every non-ASCII character: on `日本語 text`, `w` from byte 0
landed on the **end of the document** instead of on `text`, and `e` did the
same. It was invisible because ASCII single-byte characters are the only kind
that reads back, so the surface had no non-ASCII test at all. This is the
defect class M003 eliminated in `find_char` (see its closure); the word
scanners were never given the fix, and a `word_end` doc comment asserted the
opposite — that walking `offset + 1` "can cross the interior of a multi-byte
character and still classify it correctly" — which was not true of the code it
described.

Both `*_end` scanners step **one whole character at a time** for the same
reason. Probing `cursor + 1` was doubly wrong: the byte is unreadable inside a
multi-byte character, and `next_char_boundary` returns `None` (not
`Some(len_bytes())`) when the remaining bytes are one multi-byte character, so
the `.unwrap_or(cursor)` fallback had nowhere to advance and spun. That was an
unbounded loop on any document ending in a multi-byte character, found by
sampling the running test binary's stack rather than by reading the code.

`first_non_whitespace` inherits the C002 discipline of `line_content_end`: it
scans at most the line's *rendered* prefix (`bounded_prefix_end`, i.e.
`MAX_RENDER_LINE_BYTES`) and stops there. A line indented past that bound
resolves to the last visible byte, and such a line is already drawn with
`TRUNCATED_LINE_SUFFIX`, so the answer is disclosed on screen rather than
silently wrong. A blank line resolves to its own start, so `gs` round-trips with
`0` instead of appearing stuck.

**The desired column is kept, not re-derived.** `TuiDocumentPresentation::desired_column`
is `Option<usize>`, rewritten by every motion in exactly one place
(`desired_column_after`), with an asymmetry that is the whole feature:

- `MoveLeft` / `MoveRight` **re-seed** it from the column the cursor landed on —
  that is the column the reader has chosen.
- `MoveUp` / `MoveDown` **keep** the column they were aiming for. Landing on a
  short line clamps the cursor to that line's end; re-seeding there would mean
  `j` past a short line quietly rewrote the target, so `k` would return to the
  short line's end instead of to where the run started.
- Everything else clears it. `gg`, `G`, `w`, `e`, `$`, `gs`, a find, a page, a
  search jump and a LSP jump all say "this is not part of a vertical run", and a
  column carried across them would aim the next `j` at a column chosen for a line
  the cursor no longer sits on.

`apply_helix_verb` resolves motions on a **throwaway probe** and writes the real
presentation once, so the desired column is seeded into the probe and copied back
unconditionally — unlike the find record, which is written only when the motion
was a find. The column is also cleared by `reset_for_open` and `clear_history`
alongside the prefix: after a reconciliation a column measured against the *old*
text is not an error the user can see, it just aims the next `j` at a column of a
line that is no longer there. At use time `offset_on_line` clamps it into the
target line and back onto a character boundary, so a stale value can never place
the cursor outside a line or mid-codepoint.

#### Capture prefixes: `f`/`t`/`F`/`T` and `r` take their next key as data

`f`, `F`, `t` and `T` are `EditorPrefix` variants, so they arm through the same
machine and disarm at the same points. They are one case where the grammar's
"one terminal key" is **data** rather than a leaf glyph: after `f`, the character
typed is the target. `r` is the second such case — after `r`, the character
typed is the one to write. `EditorPrefix::leaves` is empty for all five, so there
is no glyph that could complete them, and `capture_char` in `commands/editor`
does the capture for both. It is deliberately a different path from the
leaf lookup — a key that is a capture never resolves against the keymap, and a
key that is a leaf never becomes a character.

`EditorPrefix::is_capture` is the single classification. Two answers —
`is_find` plus an `== ReplaceChar` test at each call site — would let a third
capture grammar be added to the dispatch without also being added to the
classifier, and it would then reach the leaf lookup and swallow a keystroke.
`scripts/check_tui_editor_prefix_state.py` asserts both `ReplaceChar` and
`is_capture` by name for exactly that reason.

Three rules that are easy to get wrong and are each covered by a test:

- **A non-character key is retried, not swallowed.** `f` then `Left` moves left,
  and `r` then `Left` does the same. An armed `f` that ate the arrow would be an
  arrow the composer never sees, on a route where unclaimed keys are supposed to
  fall through to it.
- **`:` and `/` outrank a pending target.** They open their modal lines rather
  than becoming the character to search for or write. This is stricter than
  taking the next key literally; the counter-argument (a colon after `f` *should*
  mean find-colon) is recorded in the M003 closure record, and `r :` inherits
  the same trade rather than diverging from its sibling grammar.
- **Insert mode never captures.** `handle_insert_key` does not consult the
  prefix, so the armed `f` is left intact for Normal mode rather than disarmed.

**The scan is bounded and the bound is disclosed.** `MAX_EDITOR_FIND_BYTES` is
`MAX_EDITOR_SEARCH_BYTES` rather than a second number to reason about, but it is
measured *from the cursor* — `[cursor, cursor + MAX)` forward,
`[cursor - MAX, cursor)` back — because a find jumps toward a target rather than
enumerating the document. `FindOutcome` keeps `Absent` (the whole direction was
scanned) distinct from `Bounded` (the window ran out), and `FindNote` renders the
difference in the banner row. Reporting both as "not found" would be a search
that lies by omission, which is the `search_partial` defect one layer down.

**Read in aligned chunks, never a byte at a time.** `DocumentSnapshot::read_bytes`
refuses a range that does not start *and* end on a character boundary, so a
per-byte scan cannot read byte 1 of `"aé…"` at all — it would walk past every
multi-byte target. The first implementation of `find_char` did exactly that and
failed silently on any document containing a non-ASCII character. It now reads
`FIND_SCAN_CHUNK`-sized windows with a `target.len_utf8() - 1` overlap so a
multi-byte target straddling a chunk edge is still wholly inside one of them.
Overlapping means a target can be seen twice, which is harmless: the first
occurrence is returned and the scan stops.

**`t`/`T` land on the target, as `f`/`F` do.** In Helix the head of a till-motion
lands *past* the match so the selection covers it; with a single cursor that
translates to "cursor on the target". The two pairs therefore differ in nothing
observable at this milestone, and the distinction belongs to M007's selection
set. It is spelled out in the `EditorCommand` doc comments rather than collapsed,
because collapsing the two arms would have quietly adopted the vi reading the plan
rejects.

**`Alt-.` is a single motion slot, not a find stack.** `last_find` holds one
`FindMotion`, written by `apply_motion` on every find — including one that found
nothing, because Helix sets the repeat slot by *running* the command. Every other
Normal-mode motion clears it through `note_motion`, which is why `f x`, `j`,
`Alt-.` repeats nothing. `note_motion` is deliberately not folded into
`editor_motion`: that is also Insert mode's cursor nudge, and an arrow inside a
sentence is not a gesture `Alt-.` should forget. `apply_helix_verb` carries the
record and the note back from its throwaway probe alongside the cursor, because a
find is the one motion that also writes state about the *attempt*.

`Alt-.` is claimed only while a `last_find` exists, for the same reason `n` is
gated on an existing search: a binding that consumes the key and then does
nothing is indistinguishable from a hung editor, and it steals the keystroke from
the composer.

**Where this keymap is published, and where it is not.** The authoritative
user-facing statement of the editor's chords is `docs/tui.md` ("Editor
language features") plus the table above. The shipped `/help` surface
(`components::dialogs::help::HelpDialog`, fed by `input::default_help_entries`)
lists global bindings only and names no editor chord at all, so it cannot
drift here or there. Note also that `src/tui/components/help_overlay.rs`
existed only as dead source — it was absent from `components/mod.rs`, so it was
never compiled, and its `editor_bindings()` table advertised `Ctrl+R (redo)`
and a `dd dw` row for bindings that do not exist. C001 deleted it and moved its
one unique asset, the editor key table, into `docs/tui.md`.

**Selection is the shared state machine.** `tui::helix_selection::SelectionState`
is instantiated with `EditorPoint { byte, line }` — byte offsets plus the line
each byte sits on — and `SelectionState::restore` lets the editor drive it
while keeping its own byte-offset cursor. The *rules* (`v` is a mode, `x` is an
action, movement extends only in character mode) therefore cannot drift from
the terminal; `editor.rs` owns only the geometry, which is what the shared
module deliberately does not know. `editor::selection_range` resolves a held
`Selection` to the byte range the two effects and the renderer consume:
character mode is inclusive of the head, line mode spans whole lines including
their terminators so a delete leaves no trailing blank line.

**Hard wrap.** One logical line is exactly one screen row, with horizontal
scrolling for long lines. This is what makes the line-number gutter, the
current-line highlight, and cursor arithmetic deterministic. Soft wrap
changes the line-to-row mapping and is deferred.

**Bounded per-frame work.** Only the visible slice is materialized, bounded
by viewport height, and each visible line is read with one bounded
`read_bytes` call. A multi-megabyte file renders exactly like a small one.
A whole-document `Vec<Line>` is never built, which the static guard also
enforces.

**The cursor is drawn, and it is drawn last.** `line_spans` resolves four
layers into one set of spans, each overriding only the fields it owns:

1. **Syntax** — an LSP semantic token where a language server supplied one,
   otherwise the built-in scan below. Foreground only.
2. **Search** — a `/` match, `selection_dim` background.
3. **Selection** — a held Helix selection, `selection` background.
4. **Cursor** — painted *last*, so nothing can bury the position.

**Every background layer pins the foreground to `theme.foreground` at the
same time.** A themed background carrying a syntax foreground chosen for a
*different* background is exactly the dark-on-dark case, so "patch only the
background" was never a safe compromise — it was the same bug one layer
down. Two concrete defects came out of this:

- The current line used to render as `fg(theme.alternate_bg)` — a
  *background* colour used as a foreground. On every dark theme that is
  near-black on near-black, so the cursor's own line was the one line a
  reader could not read. Since a file opens in `Normal`, this was the
  default state, not an edge case. It is now `bg(theme.alternate_bg)`: a
  tint that says "this one" and leaves the syntax colours alone.
- The **document area had no theme background at all.** The block paints
  `theme.background`, then `render_text` renders `Clear` over the text area
  — which resets every cell to the terminal's own default — and a bare
  `Paragraph` on top of it inherits that default. Both `Paragraph`s now
  restate `bg(theme.background).fg(theme.foreground)`.

**The cursor paints differently per mode, for a reason.** In `Normal` it is
**reverse video** — `fg(theme.background)` on `bg(theme.foreground)`. The
cursor must stay legible over whatever the highlighter painted underneath,
and a fixed accent background cannot promise that: a dark syntax foreground
over a dark accent is the unreadable pairing again, one layer down. Swapping
the theme's own two colours guarantees contrast for every token color and
every theme without the renderer having to know which of them is light. In
`Insert` it is a thin bar (`fg(primary)` + `UNDERLINED`), because a block
would cover the character being typed over — the one thing Insert exists to
show.

A cursor sitting **past the last character** still owns a cell, rendered as
one space past the end of the text. That is the position a reader reaches
most often — the end of a line they are about to append to — so it is the
one that must never be invisible. It is only drawn when the row is
horizontally in view, so a cursor scrolled off to the left does not become a
ghost.

**Syntax highlighting has a server-free floor.** LSP semantic tokens stay
authoritative, but a language server is optional: with none installed, every
file rendered flat, which made "syntax highlighting" a property of the
machine rather than of the editor. `tui::editor_highlight::EditorHighlight`
is a bounded `syntect` scan underneath the tokens, keyed by path, code
theme, and document revision. It lives on `EditorState::highlight`, is
passed to the widget as a scoped `&mut`, and holds no document text — only
byte ranges and colors, so the text-authority guard is unaffected.

Its scan is **sequential and forward-only** because `syntect`'s parser is a
state machine: line N depends on whether line N-1 opened a block comment.
Already-scanned lines are cached; a backwards scroll or a jump resets the
machine and replays from the top, bounded by `MAX_HIGHLIGHT_SCAN_LINES`
(5000), past which highlighting switches **itself off** and says so. An
unknown extension is a plain render, not an error. `theme.code_theme` is
already light/dark-correct (`select_code_theme`), so the scan's palette
cannot invert against the surrounding UI.

**Async discipline.** Every controller round trip (open, save, reload,
resync, close) is spawned with `spawn_scoped_registered_tui_task` and carries
both a request id and a generation counter. A completion whose generation
does not match the current attachment is discarded, so a late result can
never be applied to a later document.

**Project authority.** `/open <workspace-relative path>` resolves the project
and workspace from the active tab's `ProjectExecutionContext` and refuses
when they are absent. The command performs only a *lexical* path check — no
filesystem access, no path resolution; containment, `file.read`
authorization, and symlink policy remain the daemon's decision in
`DocumentOpen`.

#### The `m` grammar: one prefix, two levels, one field

Helix's `m` is the only **two-level** grammar in this editor: `m` names a
command, and for most of those commands the key after it names an argument
(`m s (`, `m r ( [`, `m a w`). One pending-prefix field cannot hold that
directly — but it does not have to.

**The sub-state rides in the variant.** `EditorPrefix` is
`EditorPrefix::Match` for the first level and `EditorPrefix::MatchArg(arg)`
for the second, with `MatchArg` carrying *which* argument is pending. The
presentation still has exactly one prefix field, so
`scripts/check_tui_editor_prefix_state.py` stays green — and the alternative
(a second `pending_*` field) is what that guard exists to prevent.

**The leaf table does not carry the verb.** `MATCH_LEAVES` pairs a glyph with
a `PrefixAction`, and the two-level commands' leaves carry
`PrefixAction::Capture(MatchArg)`, which says **which argument is being
waited for** and nothing else. The verb is produced when the *capture*
resolves. That is what keeps `PrefixAction` and `PrefixLeaf` `Copy` and the
leaf tables `const`: the payload is a `Copy` enum, not a `HelixVerb` — which
is not `Copy`, because the regex verbs carry a `String`.

**The two halves of `m r` must not look alike.** `m r` takes two characters.
`MatchArg::SurroundReplaceFrom` and `::SurroundReplaceTo(ch)` would both
render `"m r"`, and a reader who cannot tell them apart cannot predict what
the next key does — so `EditorPrefix::display()` returns a
`Cow<'static, str>` and the second half renders `"m r ("`, with the
character already captured. `"m r ("` is five bytes, which is exactly
`MAX_EDITOR_PENDING_PREFIX`, so the indicator bound is unchanged.

**A capital is reconciled, not passed through.** The object vocabulary
includes `W`, and terminals disagree about how a capital is encoded: some
send `Char('W')` with no modifier, others `Char('w')` plus `SHIFT`. The
`m`-grammar capture upper-cases a `SHIFT` glyph, the way `leaf_lookup_char`
does for a capital leaf. The find and replace captures take a character as
*data* and do **not** reconcile case — a latent difference, recorded rather
than fixed, because changing the find capture's behaviour is not this
milestone's to change.

**The `m` grammar retries, it does not swallow.** `m z` is not a gesture, so
`z` is what the reader asked for: the key reaches the composer and the
machine is left disarmed. `EditorPrefix::Match::swallows_unmatched` is
`false` for that reason.

**Pair scanning is byte-derived and bounded.** `src/tui/editor_match.rs` owns
the delimiter table (`PAIRS`: `(`, `[`, `{`, `<`, `'`, `"`, `` ` ``) and the
scans. Every scan runs inside a **window** of at most
`MAX_SET_VERB_READ_BYTES` (256 KiB) centred on the cursor and snapped to
character boundaries, so one keystroke cannot cost more than one screenful
of the document. The window's offsets are window-relative and are mapped
back with the window's base.

**`m m` is a delimiter match, not a syntax match.** Helix marks it `TS`; this
editor counts brackets over bytes. The divergence is deliberate and
recorded — the alternative is a key that does nothing. A syntax-aware `m m`
is M014's slice, behind ADR-0034.

**Three refusals, not one.** "No bracket under the cursor" and "no matching
bracket" are different answers and both are reported: the first says *move
the cursor*, the second says *the code is unbalanced*. The disclosure lives
on `TuiDocumentPresentation::match_note`, beside the find and paragraph
notes, and is lifted into the notice by `lift_motion_notes` immediately
after the motion — so it describes exactly one keystroke and cannot outlive
it.

**The surround verbs edit through the shared funnel, and then hold a
selection.** `submit_edit` clears every selection on success, because a
selection is a set of byte offsets into text that no longer exists. The
surround verbs mean to leave one behind — `m s` grows it over the new
delimiters — so `HelixVerbOutcome::EditAndSelect` carries the members, and
the caller applies them **after** the submission. The members are in
post-edit coordinates, and they are computed rather than read: a post-edit
offset past the old end of the document cannot be read from the pre-edit
snapshot at all, because `clamp_offset` would pull it back to the old end.

**`m d` and `m r` disagree about which pair they mean, on purpose.** `m r`
replaces the **outermost** enclosing pair, so the nesting inside survives
(`(a(b))` → `[a(b)]`). `m d` removes the **innermost** pair, which is the
one immediately around the selection, so unwrapping is local.

**A text object is a selection, and it is gated like one.** `m a` and `m i`
enlarge what the reader holds, so they are refused on a read-only
attachment, for the same reason `v` and `x` are.

#### The change verbs, and why `p` reads the system clipboard

`r`, `R`, `~`, `` ` ``, `Alt-` `` ` ``, `p`, `P` and bare `y` are M004. Six of
the eight change the text, and all six reach it through one funnel.

**There is no editor-local register.** `y` and `space y` are the *same* code
path — `yank_range` reports a byte range, `yank_editor_range` copies it — and
both write to the OS clipboard through `crate::util::clipboard`, which is
`arboard` behind a `Result`/`Option`. Nothing in `TuiDocumentPresentation`
stores yanked text. Two consequences a maintainer should not have to
rediscover:

- `p` pastes **whatever is on the OS clipboard**, including text copied from
  another application. That is Helix's own behaviour with system-clipboard
  integration, and it is the reason Helix's `R` is `replace_with_yanked`
  rather than a register operation: with no register, the clipboard *is* the
  yanked text.
- The clipboard is **not editor state**, so it is read by the App layer. The
  buffer reports `BufferKey::PasteAfter` / `PasteBefore` / `ReplaceWithClipboard`
  carrying a `ClipboardVerb`, exactly as it reports `YankRange` carrying a
  range. A second clipboard abstraction was not invented.

Three outcomes are reported separately, because a `p` that silently does
nothing is indistinguishable from a keypress that was never delivered: a
read-only or busy buffer (the same notice every edit verb uses), a **failed**
read (`None` from the system), and an **empty** clipboard (`Some("")`, a
successful read of nothing). `apply_clipboard_text` takes the read as a
parameter rather than calling `read_from_clipboard` itself, which is what makes
all three reachable in a test without a test writing to — and stealing — the
developer's real clipboard.

**`r`, `R`, `~`, `` ` `` and `` Alt-` `` share one target rule.** Each acts on
the held selection when there is one, and on the character under the cursor
when there is not: Helix names the target as *the selection*, and a bare cursor
in this editor is an offset rather than a selection. With nothing selected the
two readings coincide for a single character, which is why `~` flips one letter
or a whole phrase by the same code. At the end of the buffer there is no
character, so the range is empty and all five are no-ops — the same answer
`DeleteChar` gives.

**`~` flips; `` ` `` and `` Alt-` `` set.** One `case_range` with a `CaseMode`
drives all three, and the modes differ in a way that is observable on the second
press: `Flip` maps each character to the other case, so `~ ~` returns the
original, while `Lower`/`Upper` map to a named case, so pressing them again is
a no-op and is *reported* as one rather than submitted as an edit that pushes an
undo entry changing nothing.

Unicode case mapping **expands**, and the two families handle that differently.
`Lower`/`Upper` use the whole expansion (`ß` → `SS`, `İ` → `i` + U+0307),
because they name the whole thing. `Flip` takes only the first character of an
expansion, because `ß` has no lowercase form to become and a flip that grew
would never return to the original — it would stop being a toggle. That is a
real, stated asymmetry (`Alt-` ` on `ß` gives `SS`; `~` on `ß` gives `S`), not
an oversight, and both are pinned by tests.

**Those six go through `editor_editing_verb`, not `resolve_edit`.** Same
read-only gate, same snapshot, same `EditorEdit`, same `submit_edit` funnel; one
difference, and it is the reason the helper exists at all: they resolved their
target *from* the selection, so they must clear it. `submit_edit` deliberately
leaves `presentation.selection` alone — most verbs have no business touching it
— and a selection left over a replaced range is a highlight painted over text
that is no longer there. Expressing the clipboard payload in `EditorCommand`
instead would have meant a data-carrying verb enum that the motion machine and
the `Alt-.` recorder must also know about, so the verb enum stayed a key
identity.

**Character safety is inherited, not re-derived.** Every transform reads
through `clamp_offset` on both ends and never calls
`read_bytes(offset, offset + 1)`, which refuses inside a multi-byte character —
the defect M005 found in `w`/`b`/`e`. An edit whose inserted text is longer than
the range it replaces is safe precisely because `submit_edit` clamps
`cursor_after` against the **post**-edit snapshot.

One trap is worth naming because it is invisible: `next_char_boundary(0)` on a
document that is a *single character* returns `None`, which reads correctly at
the end of a document and wrongly at the start of one. `char_end` resolves it to
the document length, which is what tells those two cases apart. It is the same
`None`-versus-`Some(len)` distinction M005 hit in `long_word_end`.

**Cursor placement is the same for every verb here: the last byte written.** In
Normal mode the cursor sits *on* a character, so leaving it one past the edit
would put it on whatever followed — and `p p` would step over the character
between the two pastes instead of appending. It is `insert_text`'s `true` that
is the exception, because there the cursor is a bar between characters.

None of these is `stay_insert`: they are Normal-mode verbs, and an accepted
edit must not hand the keyboard to the document.

#### One binding moved: `R`

`R` used to reload from disk and throw the local draft away, as a Normal-only
**guard-block** chord. Helix gives `R` to `replace_with_yanked`, and a
guard-block chord is answered *before* mode dispatch — so leaving it would have
shadowed the new verb while surviving it, which is the one failure this
dispatch chain is arranged to make impossible. The chord and the
`BufferKey::RequestReload` variant are gone; reload is `:e`
(`EditorCommandName::Reload`), which already reached `reload_editor` directly.

The capability survived the rebinding. The *keystroke* did not, which is the
observable cost, and it is why `scripts/check_tui_editor_key_authority.py`
now asserts the two remaining guard-block chords by name instead of three.

### Project-scoped file tree (M006-D)

`Route::Editor` renders a workspace file tree in a pane to the left of the
buffer, shown with `Ctrl-T` and hidden by default so the M006-A surface is
unchanged until the user asks for it. `src/directory_listing.rs` holds the
single shared walk, `src/tui/file_tree.rs` the presentation node model
(delegating to the shared walk), `src/tui/app/state/file_tree.rs` the view
state, and `src/tui/components/file_tree.rs` the widget.

**The tree is a navigator, not a content path.** It enumerates directory
*entries* and never reads file bytes. Opening an entry calls the existing
`open_editor` verbatim, so the tree inherits that command's lexical
validation, its already-open short-circuit, its daemon-authorized
attachment, and its request/generation discipline rather than duplicating
any of them. Document text remains owned by the M005 controller, which is
why `scripts/check_tui_editor_text_authority.py` stays green and
non-vacuous: that guard's `SCANNED_FILES` list is fixed at the five editor
modules, and this feature does not widen it.

**Bounded on every axis.** The walk caps the embedded depth window (3), total
entries (4096), and per-entry name length, and records which bound stopped it
so the pane can render a truncation notice instead of presenting a partial
tree as complete. The depth figure is a *window*, not a navigation limit —
one listing is scoped, and `l` re-walks a selected directory as a new root
against the same budget, so a three-level embed bounds a response rather than
a workspace. See "Depth is a window, not a navigation limit" under the C004
walk budget below for why the window is what keeps every top-level row
inside the 512 KiB wire cap. An unreadable subdirectory is skipped rather
than treated as fatal, so a permission-denied folder cannot blank the tree.
Only the rows that fit the pane are materialized, so per-frame cost is a
function of the viewport rather than of the workspace.

**Symlinks are never followed**, at any depth, including directory
symlinks. The test is on the raw entry, because `canonicalize` resolves a
symlink and would then report the target as an ordinary directory. Paths are
built from validated component names rather than by joining arbitrary
strings, so a tree-derived path is always workspace-relative and cannot
contain `..`.

**Authority (C005/ADR-0022).** Directory enumeration is daemon-owned: the
command layer sends `CoreRequest::WorkspaceDirectoryList` carrying the
tab's canonical project/workspace binding, and the daemon resolves its own
canonical root, enforces `DirectProject + FileRead` containment, and walks
with the single shared walker. The TUI displays only daemon-origin
metadata keyed to the echoed daemon workspace identity plus the existing
generation/root/request fencing — it never walks its own filesystem, in
any mode including standalone. A same-spelled local path can describe a
different machine's directory, so there is deliberately no local `read_dir`
fallback: an unavailable daemon listing is an explicit error, and
`scripts/check_daemon_listing_authority.py` rejects reintroducing TUI-side
enumeration on the listing path. The tab's `workspace_root` remains only
fencing state (which workspace the pane believes it shows). There is no
`current_dir()` read and no path-as-identity, so
`scripts/check_tui_project_authority.py` holds by construction.

**The session's own root survives the restore.** `tab.workspace_root` is
mutable state, and the tab manifest is *user*-scoped
(`default_tui_state_root`), not workspace-scoped — so it can legitimately
name a project from an earlier session. `apply_restore_plan` clears every tab
before installing restored ones, which meant launching `codegg` in a git
worktree could silently activate a tab rooted in whatever checkout the
previous session used, with nothing on screen saying so. Because that root
anchors every relative path in the session (tools, `@` completions, the file
tree), it is the same failure class as a tool reading the daemon's launch
directory: the session gets retargeted into a tree the user never opened.

`restore::LaunchBinding::capture` snapshots the root, identities, model, and
agent off the active tab **before** the plan clears the container — whatever
the session was actually anchored to, including a daemon-canonicalized form
that differs from the frontend's launch directory.
`restore::preserve_launch_binding` then either leaves an agreeing restore
untouched (keeping the plan's heavy session load), focuses an already-restored
tab for that root, or re-establishes the tab — and drops the heavy load in the
latter two cases, because it targets a session the user is no longer looking
at. The disagreement is reported as a restore diagnostic plus a warning toast
naming the root that was preserved. Reporting rather than silently correcting
is deliberate: the user may have meant to resume the older project, and
discarding their restored tabs would be the opposite mistake — what is never
acceptable is the two disagreeing invisibly. A `None` root is compat mode
(the launch directory could not be canonicalized) and preserves nothing. Reporting rather than silently
correcting is deliberate: the user may have meant to resume the older project,
and discarding their restored tabs would be the opposite mistake — what is
never acceptable is the two disagreeing invisibly.

**Focus and layout.** The layout split degrades to the full-width editor on a
narrow terminal rather than producing a degenerate pane. While the tree is
focused it claims a strict subset of keys — `j`/`k` to move, `Ctrl+d`/`Ctrl+u`
to page, `l`/`→` to make the selected directory the pane's root, `h`/`←` to go
back one level, `Enter` to open a file or expand a directory, `r` to re-walk,
`Esc` to leave the tree — and every other key falls through unchanged, so a
focused tree never swallows composer or editor input. `Esc` returns focus to
the editor buffer without changing the route, preserving the non-modal
composer contract.

`Ctrl+d`/`Ctrl+u` carry `Char('d')`/`Char('u')` codes, so the tree's modifier
check runs before its character arms; the editor's own half-page motions are
untouched because the tree only claims keys while focused. A page is two
thirds of the pane, from the viewport the renderer measured and published as
`FileTreeState::viewport_rows` — the widget returns its `PaneLayout` rather
than writing into the state it borrows, so the number stays next to the
layout that produced it. An unmeasured pane (a key arriving before the first
frame) falls back to `FALLBACK_PAGE_ROWS` rather than making the verb inert.

**The pane has a root, and it says which one.** `root_history`'s last entry
is the directory being listed; the title leads with it and names the
workspace after it, because a pane scoped with `l` showing a workspace-root
title would misdescribe every row under it. Rows carry a per-type icon from
the one table in `src/tui/file_icons.rs` — the folder glyph doubles as the
expansion state, so there is no second caret column in a 22-column pane — and
the selected row repaints rather than recolors, so the type colour that
distinguishes a `.rs` from a `.md` survives the cursor landing on it. Icons
are Nerd Font glyphs by default and `CODEGG_FILE_ICONS=plain` selects the
pre-Nerd caret markers for a terminal without a patched font; that is an
environment switch rather than a config key because it answers a
terminal-capability question, and a wrong-shaped key in a layered config
discards more than it selects.

**Async discipline.** The listing is a daemon round-trip on a scoped
`TuiTaskKind::FileTree` task. A completion carries the request id, the
tree generation, the root it was issued for, and the scope it was issued for;
a completion that is stale, cancelled, for another workspace, or answered for
a different subtree is discarded rather than rendered. Failures arrive as an
explicit `FileTreeFailed` completion and are fail-visible in the pane.
Switching projects resets selection, expansion, and the root, because a path
from the previous project is meaningless in the next. Advisory daemon
statuses stay visible: `busy` names the retry key, and bound hits carry their
truncation reason.

### Command mode and agent management (M006-F)

M006-F adds the `:` command line to the editor buffer and turns `/agents`
into a management panel.

#### `/agents` opens agents, and `Enter` opens the editor

Bare `/agents` opens the **agent dialog** (`Dialog::Agent`) — the same surface
`/agent` opens. It previously opened `PanelPickerDialog` ("which panel should
this go in?") instead, which answered a question nobody asked: someone typing
`/agents` wants to see agents. Panel *placement* is a different question and
stays on `/panels`.

`PanelPickerDialog` was **retired** with that route. It had exactly one caller,
and its selection half hard-coded `SidePanelId::Agents`, so it was a one-off
shim for `/agents` rather than a general placement tool; leaving it in place
would have been dead code that clippy's `-D warnings` rejects and that no
surface could reach. Its list mechanics were never duplicated: the manager
shares `PanelListState` (`components/dialogs/panel_list.rs`) directly, which is
where the shared `move_cursor`/`scroll`/`matches`/`mode_badge` behavior now
lives alone.

The picker commits through the **same** `TuiMsg::SelectPanel` the manager
emits; there is no second route into panel selection.
`App::pending_panel_picker_subject` is what gives the selection its second
half: the manager selects a panel and stops, while this box exists to have the
chosen content *loaded* into the chosen panel afterwards. It is spent on
commit, so a picker the user abandoned cannot load its content into the next
panel they pick.

`AgentsPanel::handle_key` had **no `Enter` arm** — it fell through to the
catch-all and returned `Consumed` without emitting a message, so selecting a
row consumed the key and did nothing. `Enter` now means the same as `e`:
open that agent's config. The verbs stayed provenance-checked in the command
layer, so adding a key does not widen what a built-in allows. It consumes the `document.v1` surface the earlier
editor milestones established; the one *additive* protocol change is the
create/delete pair described below.

#### The `:` command line is modal, bounded, and one registry

`:` opens the command line from **Normal mode only** — in Insert a colon is
document text, and swallowing it would make a URL untypable.
`editor::EditorCommandName::ALL` is the single command registry: the parser,
the fuzzy palette, and the help line all read that list, so a command that
cannot be discovered also cannot be run by muscle memory alone.

| Behavior | Rule |
|---|---|
| Palette query | Matches the **command name only**, never the argument, so `:open <TAB>` completes the command while the path is still being typed. |
| Empty query | Offers the whole registry in declared order — that is the discovery path for a user who has never pressed `:`. |
| Stray argument | `:wq now` is **refused**, not silently run as `:wq`. Half of what was asked for is worse than refusing it. |
| Exiting | `Esc` closes without side effects; it is the only exit that can never run anything. |
| Modal capture | The line owns *every* key while open, including the mode-independent `Ctrl+S` chords. A leaked printable would insert into the document behind a line the user believes is capturing it. |
| Bound | `MAX_EDITOR_COMMAND_LINE` (64), checked at the keystroke. The field holds a command *name*, never document text, so it is exempt from the editor text-authority guard for the same reason `pending_command` is. |

`fuzzy_score` returns `0` for both an empty query and a total miss, so a score
of zero cannot separate them. `complete_editor_command` therefore keeps exact
prefix matches explicitly and requires a non-zero score for everything else —
without that, `:qz` would offer commands that do not contain the query.

#### `:wq` closes after the write, never before

`EditorState::close_after_save` defers the close to the save completion.
Closing eagerly would detach the controller and discard the very draft the
user asked to keep; not setting it would strand the editor open after a
successful write. The flag is cleared in `detach()` so a failed save cannot
leave a stale close armed for the *next* document.

A plain `:q` with an unflushed draft is refused with a message naming both
escapes (`:w` first, or `:q!` to discard) — `:q!` is only reachable because
that refusal exists.

#### The editor replaces the main buffer; the composer arms with `:`

While `Route::Editor` is active and the route is in **Normal** mode,
`composer_refuses_bare_text()` is true and `App::on_key` turns the first
character into `:` + that character. The gate sits on the single
`InputAction::Char` arm, so every path that could type a character is
covered by one rule rather than by the keymap alone.

It is scoped narrowly on purpose: a focused side panel is a *different*
destination with its own composer, and chat is a conversation, so both stay
freely typeable. The test is `input_mode == Normal` because the buffer holds
the keyboard exactly when it is `Insert` — the one axis, read once.

#### The directionals stay live in the buffer

With the buffer focused in **Normal mode**, `Shift+H/J/K/L` and the arrow
twins select or dock a panel. This routes through
`panels::handle_normal_only_action` — the *same* entry point `App::on_key`
uses for its five direction arms — so `Shift+L` means the same thing whether
the keyboard is in the composer or in the buffer. Two dispatchers for one
gesture family is exactly how a box and its panel drift.

#### `/agents` actions differ by provenance, and the panel says so

`src/tui/panels/agents.rs` holds presentation only (cursor, viewport, armed
action); every action becomes `TuiMsg::AgentPanelAction` and is resolved in
`commands/agent_editor.rs`, which re-checks provenance so a verb the panel
could not offer is still refused if it arrives another way.

| Origin | Edit | Deactivate | Delete | Restore |
|---|---|---|---|---|
| Project (`<root>/.codegg/agents/<name>.toml`) | yes | yes | yes (`d` then `d` — armed, not immediate) | no |
| Global (`~/.config/codegg/agents/<name>.toml`) | yes | yes | yes (armed, as above) | no |
| Default (`build`, `explore`, …) | via override | yes | **no** | yes (drops the override) |

A built-in agent is compiled into `generated.rs` and has **no file of its
own**. Deleting one would only remove its override and leave the agent
present, which is worse than offering no delete at all. Restore is exact
precisely because the default has no saved copy to go stale: removing the
override falls back to the compiled definition.

#### Create and delete are additive, contained, and never overwrite

`DocumentOpen` requires the target to already exist
(`canonical_target.is_file()`), so "create this file" was not expressible.
`DocumentCreate` and `DocumentDelete` are the additive pair. Both live in
`src/lsp/mutation.rs` next to `checked_workspace_text_write` and borrow
**every** containment check it applies — validated relative path,
symlink-free components, canonical root prefix — because "delete this config"
is as dangerous as "create a file outside the workspace" if the path can
escape. Four additions of their own:

- **No overwrite.** An existing target is `AlreadyExists`; the file is not
  touched. Opening an existing file stays `DocumentOpen`'s job, so there is
  one way to reach an existing file and it never rewrites it.
- **Bounded parent creation.** Missing directories are materialized to
  `MAX_CREATE_DEPTH` (16) components, each canonicalized *after* creation so
  a directory that is a symlink is caught at the level it appears.
- **`create_new(true)`.** A file that appears between the existence check
  and the write is not destroyed by the race.
- **Refuse to delete what is open.** The daemon rejects an unlink for a
  live attachment; a client-side `remove_file` would leave a replica writing
  to a path that no longer exists.

Both carry `Capability::FileModify` in `authorization/policy.rs`. Grading
either as a read would let a read-only attachment create or destroy files.

#### Scope: two authorities, and the UI always says which

An agent config lives in one of two places, and they are not
interchangeable:

| Scope | Location | Who sees the edit |
|---|---|---|
| `DocumentScope::Workspace` | `<root>/.codegg/agents/<name>.toml` | this project |
| `DocumentScope::GlobalAgent` | `~/.config/codegg/agents/<name>.toml` | **every project on this machine** |

`DocumentOpen` is workspace-contained by design, and a global agent is
outside every workspace, so workspace containment can never reach it. The
answer is **not** to relax that containment. `DocumentScope` establishes a
second, separately named authority (`src/core/global_agent_authority.rs`) with
its own root and its own write gate, and the global scope is **stricter** than
the workspace scope, never wider:

| | Workspace | GlobalAgent |
|---|---|---|
| Allowed shape | any workspace-relative path under the root | exactly `<name>.toml`, one level, no separators |
| Name rule | `validate_relative` (no `..`, no absolute) | `[A-Za-z0-9_-]{1,64}`, bounded |
| Write gate | that workspace's repository lock | a process-wide mutex for the agent directory |
| LSP managed document | yes | **no** — it is not a workspace document |
| External-disk refresh | yes | no |

The narrow rule runs in `DocumentScope::validate_global_agent_filename`
*before* any filesystem work, and then the shared checks still run: every
global create/write/delete goes through `src/lsp/mutation.rs` exactly as a
workspace write does. The global path adds a root and a gate; it never
replaces a containment check with a cheaper one.

**These are separate match arms, not one arm with a conditional root.** The
two authorities differ in containment, locking, *and* side effects; folding
them together is how a "just this once" LSP sync or repository lock ends up
applied to a path outside the workspace it was written for. The global
handlers live in their own `impl CoreDaemon` block in
`src/core/daemon_documents.rs`.

`DocumentScope` is part of the attachment key
(`DocumentKey::key_string`), because a global `build.toml` and a workspace
`build.toml` are different documents under different roots — collapsing them
would let one be served from the other's root. It travels on the wire rather
than being inferred server-side, because it is also the discriminator the
future admin authorization check needs (§ *One daemon, many agents* below).

#### The blast radius is always on screen

`build.toml` reads the same at either scope, but `:wq` means "this project"
in one case and "every project" in the other. The scope is therefore shown, not
remembered:

- the editor status row carries a `[project]` / `[global]` badge, warning-
  coloured when global (`src/tui/components/editor.rs`);
- the `/agents` row badge is `default` / `project` / `global`, with global in
  the warning colour (`src/tui/panels/agents.rs`);
- the new-agent template states its scope in a header comment, so the
  distinction is still there after the buffer is closed.

`is_agent_config_path(path, scope)` is **scope-qualified**. Matching on the
path alone would treat a workspace file that happens to be called
`build.toml` as a global agent and refresh the wrong registry after a save.

#### Choosing a scope when creating

`:new <name>` creates a project agent (the historical behaviour).
`:new global <name>` creates a global one. `split_scope_word` consumes a
leading `global`/`project` only when a name follows it, so `:new global` is
still an agent *named* `global` — no agent name is unreachable.

When the name already exists, `:new` opens the file that actually defines the
agent rather than creating a second, shadowed definition: `:new global build`
with a project `build` opens the project file, because that is the definition
in effect. When a default carries overrides at **both** scopes the row
presents the project one, for the same reason.

#### One daemon, many agents (future)

Today one daemon serves one OS user, so "who owns this file" and "who is
asking" are the same question; an explicit authorization tier would be
theatre, and adding one now would be untestable. The forward contract is
recorded rather than assumed:

- the global root is **shared** state — one agent's edit changes every
  project and the user — so the requirement is per-**request**, not
  per-document;
- an admin capability must gate `DocumentOpen`, `DocumentCreate`,
  `DocumentDelete`, and the save/reload path;
- the audit trail must distinguish a global mutation from a workspace one so
  an operator can attribute it;
- every call resolves through `global_agent_authority::ensure_root` /
  `existing_root`, which is the single seam where the check belongs.

`DocumentScope` already travels on the request and the snapshot, so adding
the gate later is additive rather than a rewrite of the containment rules.

#### Saving refreshes the panel

A successful save of a file matching `is_agent_config_path(path, scope)`
triggers the same daemon `AssetRefresh` `/reload` already uses, so `:wq`
makes the new agent appear without a second command, at either scope. A delete does the same. `set_rows`
preserves the cursor **by name**, which is what makes a save-then-refresh
feel stable: the list behind the cursor changes, the row under it does not.

### Agent change review (M006-E)

`/review <preview-id>` opens a modal review of a pending LSP change candidate
and waits. It applies nothing on open. The user accepts with `a` or rejects with
`Esc`. `src/tui/unified_diff.rs` parses the candidate's patches,
`src/tui/app/state/change_review.rs` holds the state machine,
`src/tui/components/change_review.rs` renders, and
`src/tui/commands/change_review.rs` owns the commands.

**Scope: saved documents only.** This milestone reviews changes destined for
*saved* files. The dirty-buffer question — whether an apply may merge into a
buffer with unsaved edits — is deferred to a future ADR, and
`src/lsp/mutation.rs` is deliberately unchanged. The existing rejection
(`"{path} has unsaved editor changes; save and regenerate the preview"`) is
correct as written; the review surface *surfaces* it rather than relaxing it.
A user who hits it is told to save and regenerate, which is the right answer.

**There is one apply path, and it is structural.**
`change_review::build_apply_request` is the only function that turns a preview
id into a `CoreRequest::LspPreviewApply`. Both `/lsp-preview-apply` and the
review's accept call it, so the legacy command was refactored to route through
it rather than keeping a private copy. "One apply semantics" therefore cannot
drift into two — there is no second constructor to drift. The review adds a
*step*, never a *meaning*.

The testable consequence is that **what the user reviews is what gets applied**:
`the_reviewed_change_and_the_applied_request_are_the_same_change` stages a real
candidate in a real `LspTool` registry, resolves the real request, and asserts
the review's hunks are a parse of that request's own patch text, under the same
digest, revision, and base hash. If a future change re-derived the displayed
diff from disk, or opened the review from a different source than the apply,
the user would be approving something other than what lands — and this fails.

**The daemon keeps the authority.** The frontend decides only preconditions —
no session, no workspace, no client, no such candidate. Whether an apply is
*legal* is the daemon's judgement, and its message is recorded **verbatim** in
the review rather than paraphrased or replaced. The review stays open on a
refusal so the message remains readable next to the diff it refused. The
trajectory test quotes the daemon's exact dirty-buffer string, so a change to
that rejection is a visible diff in the test rather than a silent divergence.

**Stale and spent candidates fail closed.** A candidate whose base no longer
matches is refused *before* a review is built — showing a diff that no longer
corresponds to the candidate would invite approving the wrong thing. An
already-applied candidate is refused by the same builder the legacy path uses,
so a spent id cannot be applied twice from either path.

**Bounded parsing.** `unified_diff` caps patches (64), patch bytes (512 KiB),
hunks per patch (256), and lines per hunk (2000), and records which bound
stopped it so the view can say the diff is partial rather than presenting it as
complete. Byte cuts land on UTF-8 character boundaries. An empty change
(`"(no changes)"`) is treated as known-empty, not malformed — a real no-op must
not read as a parse failure.

**Rendering reuses `DiffViewer`.** The view constructs the existing widget and
replaces its public `hunks` rather than forking diff rendering, so review and
the ordinary diff panel cannot disagree about how a change looks.

**The one modal surface in M006.** While open, the review takes over the
viewport, because accept-versus-reject is a decision that should not be made
while typing. It claims a strict subset of keys (`a`, `Esc`, `j`/`k`); every
other key falls through unchanged, so it never swallows composer input. An
async completion carries the request id and review generation, and a
completion for a review already rejected or superseded is discarded rather than
applied.

### InputMode (`src/tui/input.rs`)

```rust
pub enum InputMode {
    Insert,
    Normal,
}
```

`InputMode` stays a text-editing/Vim concern. The M003 prompt
submission mode is the separate `ComposerMode::Session | Task`
(`src/tui/app/state/work_orders.rs`). C001 owns bare Tab at root
prompt focus for `Session <-> Task` in both keymaps; `SwitchAgent`
migrates to `Ctrl+A` (configurable/discoverable, with `Ctrl+G` as a
backward-compatible composer alias) and Shift+Tab stays
permission-mode cycling. Modal-local Tab focus consumes Tab before
root routing.

### InputAction

52 variants (`src/tui/input.rs:102`): `Send`, `Newline`, `Cancel`,
`NavigateUp`, `NavigateDown`, `SwitchAgent`, `SelectModel`, `ClearSession`,
`NewSession`, `ToggleSidebar`, `FocusSidebar`, `ToggleTasksList`, `ToggleSection`,
`CloseSession`, `Help`, `FocusPrompt`, `StashPrompt`, `RestorePrompt`,
`CopyMessage`, `CycleModelForward`, `CycleModelBackward`, `ToggleReasoning`,
`Quit`, `ForceQuit`, `ExternalEditor`, `Char`, `Backspace`, `Delete`,
`Left`, `Right`,
`Home`, `End`, `PageUp`, `PageDown`, `Search`, `SearchNext`, `SearchPrev`,
`ClearSearch`, `Command`, `ToggleTts`, `StopTts`, `ToggleFullscreen`,
`TogglePermissionMode`, `OpenDiff`, `GoToTop`, `GoToBottom`,
`OpenProjectPicker`, `NextProjectTab`, `PreviousProjectTab`,
`CloseProjectTab`, `ToggleComposerMode`, `OpenWorkspaceDashboard`.

### New session (`Ctrl+N`, `/new`)

`NewSession` and `/new` (alias `/clear`) both route through
`App::new_session` (`src/tui/app/mod.rs`), which clears the composer view and
then calls `sessions::start_new_session`. That spawns a registered
`TuiTaskKind::Command` task issuing `CoreRequest::SessionCreate`, guarded by
`dialog_state.new_session_request` and completed by
`TuiCommand::NewSessionCreated` → `apply_new_session_created`, which attaches
the new session via `set_session` and ignores stale request ids.

The session is created eagerly rather than left implicit. Clearing
`session_state.session` without creating a replacement left the TUI with no
session at all: `/models`, `/sessions` and the session tree had nothing to bind
to, and `Ctrl+N` looked like a dead key because pressing it again was a no-op on
already-empty state. Creating the session is what makes those surfaces work
before the first prompt.

With no core client available (daemon unreachable) the action reports an error
toast instead of silently doing nothing; a failed creation never fabricates a
session. Covered by `apply_new_session_created_*` in `src/tui/mod.rs`.

### The launch directory starts its own session (`ensure_launch_session`)

`App::ensure_launch_session` (`src/tui/app/project_session.rs`) runs once from
`main.rs`, after the launch-directory bootstrap and the initial-session load,
and is a no-op whenever a session is already bound. It exists because the
inproc (`--standalone`) startup branch only acted for an explicit
`--fork`/`--session`/`--continue`, so a plain `--standalone` launch reached the
event loop with **no session at all**.

That is the same defect `NewSession` above was written for, reached by a
different door: every session-scoped surface — chat, files, agents, models —
resolves its binding through the active session, so a launch that leaves
`session_state.session` as `None` looks like a broken install rather than a
missing keystroke, and the user's first action is `Ctrl+N`.

It reuses `sessions::start_new_session`, so the request carries the **typed**
`(project_id, workspace_id)` pair the daemon can resolve — not the bare
directory locator the initial-session path uses. It is deliberately not
`App::new_session`: that clears the transcript, resets the route, and can open
a template picker, none of which belong in a launch with nothing to clear.
Gated on `!cli.no_session`.

### Stopping a turn and carrying on (`/stop`, `/start`, `x`)

`CoreRequest::TurnCancel` is addressed by `(session_id, turn_id)` and the
daemon **rejects** a mismatch (`turn_id_mismatch`) rather than ignoring it, so
stopping a turn is impossible without the id. Both wires that carry one —
`CoreEvent::TurnStarted` and the submit receipt — used to be discarded, which
is why nothing in the TUI could interrupt a running agent. `TurnStarted` now
binds `SessionState::active_turn_id`; every terminal turn event clears it.

A cancelled turn arrives as an ordinary `TurnCompleted` with an unrecognised
`stop_reason`, so `SessionStatus` alone cannot distinguish "I stopped this"
from "the agent finished". `SessionState::stopped_turn` carries that, and it
**survives** `TurnCompleted` deliberately: the stopped badge and `/start` are
still what the user should see until they resume or send a new prompt. A new
`TurnStarted` clears it.

`/stop` marks `stopped_turn` optimistically and only undoes it when the
request genuinely fails — the daemon's cancel is cooperative, so waiting for
`TurnCompleted` would leave a visible lag between the keypress and the badge,
and a user who presses twice needs the second press to be a no-op.
`no_active_turn` and `turn_id_mismatch` are absorbed as "it had already
finished" rather than reported as errors; punishing that common race would be
worse than the silence.

There is no protocol-level resume-a-stopped-turn, so `/start` **re-submits**
(`turn_control::start`): the cancelled turn's work is gone and the transcript
carries the context. A `/start` that claimed to revive the dead turn would
silently run something the user did not ask for.

The spelling is `/start`, not `/resume`, because `/sessions` has owned the
`resume` alias for switching sessions all along and the registry rejects a
name/alias collision rather than letting one silently shadow the other.

`x` (`InputAction::ToggleAgentRun`) covers both directions in one key because
the state already says which applies. Stopping is the destructive direction, so
it is gated behind a second press of the same key inside `STOP_ARM_WINDOW`
(3s); an accidental `x` while reaching for a chord must not kill a turn. The
arm is expired by the render pump, not only on the next keypress, so a
confirmation nobody acts on clears itself. Bare-char binding, so Insert mode
types the letter — asserted by
`a_single_x_in_insert_mode_is_typed_rather_than_stopping_the_agent`.

## Directory Structure

```
tui/
├── app/
│   ├── mod.rs              # App composition root, construction, facade, shutdown
│   ├── types.rs            # Dialog, TuiMsg, and stable app-facing values
│   ├── commands.rs         # TuiCommand effect requests/completions
│   ├── render.rs           # Cached render composition and view preparation
│   ├── input.rs            # Synchronous TuiMsg intent routing
│   ├── project_session.rs  # Active project/session and projection lifecycle
│   ├── prompt_turn.rs      # Prompt submit and route-safe turn start
│   ├── modal.rs            # FocusManager-backed modal lifecycle
│   ├── plugin_ui.rs        # Validated plugin UI effect application
│   └── state/              # 31 state domain modules + mod.rs
│       ├── agent.rs        # AgentState (models, agents, snapshot)
│       ├── async_request.rs # AsyncUiRequestState
│       ├── change_review.rs # M006-E review state machine
│       ├── chat.rs         # Project-scoped chat projection
│       ├── diagnostics.rs  # TuiDiagnostics
│       ├── dialog.rs       # DialogState (all dialog instances)
│       ├── editor.rs       # M006-A editor presentation state
│       ├── execution_context.rs # Explicit project execution context
│       ├── file_tree.rs    # M006-D file tree view state
│       ├── manifest.rs     # Manifest handling
│       ├── messages.rs     # MessagesState (messages, toasts, spinner)
│       ├── observe.rs      # Read-only session observation
│       ├── panel_hotkeys.rs # Side-panel Ctrl+digit hotkey bindings
│       ├── persistence.rs  # State persistence
│       ├── plugin_ui.rs    # PluginUiState
│       ├── presence.rs     # Collaborator presence projection
│       ├── project_picker.rs # Project picker (Milestone 2)
│       ├── project_tabs.rs # Multi-project tabs (Milestone 1)
│       ├── projection_client.rs # Projection client state
│       ├── prompt.rs       # PromptState (prompt, completions)
│       ├── restore.rs      # State restore
│       ├── routing.rs      # Route routing helpers
│       ├── session.rs      # SessionState (session, history, git info)
│       ├── snapshot.rs     # Remote snapshot building
│       ├── subagent_addressee.rs # C007: who the composer is addressed to
│       ├── subagent_tabs.rs # Subagent tab set over the main buffer (Shift+N/P/X)
│       ├── subagents.rs    # SubagentRow projection rows + strip/buffer view state
│       ├── tasks.rs        # Ctrl+T tasks overlay rows (todos, subagents, jobs)
│       ├── ui.rs           # UiState (theme, layout, routes, keybindings)
│       ├── view_switch.rs  # Active-view switch coordinator
│       ├── work_orders.rs  # Task/work-order state, ComposerMode
│       └── workspace_dashboard.rs # M005 Workspace primary-view cache
├── commands/               # 37 command handler submodules + mod.rs
│   ├── mod.rs              # Re-exports
│   ├── agents.rs           # Asset refresh, agent operations
│   ├── change_review.rs    # M006-E review commands
│   ├── chat.rs             # Project chat commands
│   ├── control.rs          # Shared-session control (M004)
│   ├── diagnostics.rs      # Doctor, diagnostics, tool contracts
│   ├── editor.rs           # M006-A document open/save/reload
│   ├── file_tree.rs        # M006-D tree walk
│   ├── git_sidebar.rs      # Git sidebar refresh
│   ├── goals.rs            # Goal lifecycle, session state refresh
│   ├── import.rs           # Import preview, confirm
│   ├── interactive_terminal.rs # Interactive terminal attach/resume
│   ├── manifest_restore.rs # Manifest restore operations
│   ├── memory.rs           # Memory summary, search, remember, forget
│   ├── observe.rs          # Read-only session observation
│   ├── plugin_management.rs # Plugin management operations
│   ├── plugins.rs          # Plugin command run, UI effect
│   ├── policy.rs           # Approval mode / sandbox profile
│   ├── presence.rs         # Collaborator presence commands
│   ├── project_catalog.rs  # Project catalog refresh
│   ├── project_picker.rs   # Project picker navigation
│   ├── prompt.rs           # Prompt/session continuation commands
│   ├── provider_connections.rs # Provider connection lifecycle
│   ├── queue.rs           # /queue box: open, refresh, delete (M004); add/edit/reorder
│   │                      # (M005); send-now (M006)
│   ├── research.rs         # Research list runs, load run, load section
│   ├── run_rerun.rs        # Historical run reruns
│   ├── security.rs         # Security review dispatch
│   ├── session_selection.rs # Session selection refresh/load
│   ├── sessions.rs         # Session CRUD, archive, fork, bulk ops, rename, share
│   ├── shell.rs            # Shell list, include, rerun, kill, show, ask
│   ├── tasks.rs            # Task/worktree/template/notification/file-diff
│   ├── team.rs             # Team administration (M003)
│   ├── test.rs             # Test run lifecycle
│   ├── work_orders.rs      # WorkOrder/TaskSheet/TaskView operations
│   └── workspace_dashboard.rs # M005 Workspace primary-view fetches
├── runtime/
│   ├── mod.rs              # Re-exports
│   ├── event_loop.rs       # Main event loop (select loop, render, terminal)
│   ├── command_dispatch.rs # Main TuiCommand dispatch match
│   ├── app_events.rs       # Bus event handling (AppEvent subscription)
│   └── render_recovery.rs  # Render panic recovery (progressive fallback)
├── components/
│   ├── component.rs        # Component trait, DialogType enum
│   ├── component/
│   │   ├── focus.rs        # FocusManager for modal focus stack
│   │   └── context.rs      # AppContext for overlay dialogs
│   ├── dialogs/            # 40 modal dialogs + mod.rs (all implement Component)
│   │   ├── agent.rs        # AgentDialog
│   │   ├── command.rs      # CommandPalette (query/selection model only —
│   │   │                     # it no longer renders; see "One popup" below)
│   │   ├── confirm.rs      # ConfirmDialog
│   │   ├── connect.rs      # ConnectDialog (provider-neutral /connect:
│   │   │                     # setup-catalog selection + typed per-provider
│   │   │                     # credential forms; Eggpool is one proxy preset)
│   │   ├── connection_selection.rs # ConnectionSelectionDialog
│   │   ├── device_secret.rs # DeviceSecretDialog (LocalOwner credential render-once)
│   │   ├── diff.rs         # DiffDialog
│   │   ├── goto.rs         # GotoDialog
│   │   ├── help.rs         # HelpDialog (searchable keybinding list)
│   │   ├── import.rs       # ImportDialog
│   │   ├── info.rs         # InfoDialog (Context/Cost/Usage/Stats/etc.)
│   │   ├── keybind.rs      # KeybindDialog
│   │   ├── mcp.rs          # McpDialog
│   │   ├── model.rs        # ModelDialog
│   │   ├── permission.rs   # PermissionDialog
│   │   ├── plan.rs         # PlanDialog
│   │   ├── plugin.rs       # PluginDialog (generic plugin UI)
│   │   ├── project_picker.rs # ProjectPickerDialog
│   │   ├── question.rs     # QuestionDialog
│   │   ├── queue.rs        # QueueDialog (M004 list/delete; M005 add/edit/reorder;
│   │   │                  # M006 send-now)
│   │   ├── research.rs     # ResearchBrowserDialog
│   │   ├── review.rs       # ReviewDialog
│   │   ├── run_detail.rs   # RunDetailDialog
│   │   ├── security_review.rs # SecurityReviewDialog
│   │   ├── session.rs      # SessionDialog (sessions + Subagents/Jobs sections)
│   │   ├── share.rs        # ShareDialog
│   │   ├── source_preview.rs # SourcePreviewDialog
│   │   ├── task_schedule.rs # TaskScheduleDialog (WorkOrder sheet)
│   │   ├── task_view.rs    # TaskViewDialog
│   │   ├── template.rs     # TemplateDialog
│   │   ├── theme.rs        # ThemePickerDialog
│   │   ├── tree.rs         # TreeDialog
│   │   ├── trigger_secret.rs # TriggerSecretDialog (render-once secret)
│   │   ├── ui_node.rs      # UiNodeDialog (generic, reuses Plugin slot)
│   │   └── workspace_dashboard.rs # WorkspaceDashboardDialog (compat shim)
│   ├── change_review.rs    # M006-E review component
│   ├── completion_overlay.rs # Slash/file/agent completion popups
│   ├── diff.rs             # DiffViewer
│   ├── editor.rs           # M006-A buffer widget
│   ├── file_tree.rs        # M006-D file tree widget
│   ├── help_overlay.rs     # (removed in C001 — help is mode-aware via input.rs)
│   ├── image.rs            # ImageViewer (image rendering via ANSI)
│   ├── messages.rs         # MessagesWidget (message display, streaming)
│   ├── messages/           # MessagesWidget submodules
│   │   └── layout.rs       # Row index: drawn rows → scroll offsets
│   ├── notification.rs     # NotificationManager
│   ├── plugin_renderer.rs  # Compat alias for UiNodeRenderer
│   ├── ui_node_renderer.rs # UiNodeRenderer (UiNode → ratatui/line)
│   ├── prompt.rs           # PromptWidget
│   ├── scroll.rs           # CenteredScroll
│   ├── sidebar.rs          # SidebarWidget
│   ├── spinner.rs          # SpinnerWidget
│   ├── status_bar.rs       # StatusBarWidget
│   ├── toast.rs            # ToastManager
│   └── tool_output.rs      # ToolOutput
├── input.rs                # Key event handling, keybindings, InputMode
├── layout.rs               # Layout calculations, TuiLayout
├── panels/                 # Swappable side-panel host (Shift+S manager)
│   ├── mod.rs              # SidePanelId registry, SidePanel trait, SidePanelHost
│   ├── chat.rs             # Interactive project-chat panel
│   ├── terminal.rs         # Live PTY panel (NOR/INS, starts a shell on `i`)
│   ├── help.rs             # Keybinding-reference panel (hosts HelpDialog)
│   └── session.rs          # Session panel (delegates to SidebarWidget)
├── route.rs                # Route/RouteManager (Home, Session, Workspace, Editor)
├── selection.rs            # Terminal text selection over the rendered frame buffer
├── helix_selection.rs      # Helix selection state machine (coordinate-agnostic)
├── panel_resize.rs         # Outward-only panel resize rules + key→border geometry
├── theme.rs                # TUI-local Theme (ratatui projection)
├── terminal.rs             # TerminalGuard lifecycle, AppTerminal
├── editor.rs               # M006-A editor presentation record/state
├── editor_effect.rs        # M010 editor-level pending effect + clipboard verb
├── document_session.rs     # TuiDocumentSession (DocumentController seam)
├── file_diff.rs            # Async diff stats for sidebar
├── file_tree.rs            # M006-D directory walk + node model
├── interactive_terminal.rs # Interactive terminal projection
├── unified_diff.rs         # Bounded patch/hunk parser
├── task_lifecycle.rs       # TuiTaskRegistry for background task tracking
├── async_cmd.rs            # spawn_tui_task, spawn_registered_tui_task
├── command.rs              # Slash command registry (164 built-in)
├── ui_builders/            # Pure UiNode builder functions
│   ├── mod.rs
│   ├── stats.rs            # stats_node for /tui-stats
│   ├── plugins.rs          # Plugin management builders
│   └── shell.rs            # shell_detail_node for /shell-show
└── mod.rs                  # TUI entry point, module declarations, re-exports
```

### App ownership and message direction

`App` remains the sole TUI composition root. The `app/` modules are
responsibility-oriented implementation families, not alternate coordinators or
state stores:

| Responsibility | Owner | Boundary |
|---|---|---|
| Composition, construction, facade, shutdown | `app/mod.rs` | owns the `App` aggregate |
| Render composition/view preparation | `app/render.rs` | cached state only; no I/O or async work |
| Component/user intent | `app/types.rs`, `app/input.rs` | `TuiMsg` is synchronous UI intent |
| Effect requests/completions | `app/commands.rs` | `TuiCommand` crosses the runtime channel |
| Project/session/projection lifecycle | `app/project_session.rs` | active-tab and reconnect guards |
| Prompt/turn start | `app/prompt_turn.rs` | immutable route capture before continuation |
| Modal lifecycle | `app/modal.rs` | M007 `FocusManager` is the live owner |
| Plugin UI effects | `app/plugin_ui.rs` | bounds/ownership validation and lowering |
| Runtime routing | `runtime/command_dispatch.rs` | one top-level dispatcher |

`TuiMsg` describes an intent already on the synchronous UI side; it may mutate
cached UI state or enqueue an explicit `TuiCommand`. `TuiCommand` carries an
effect request to the runtime or a typed asynchronous completion back to the
`App` state owner. Completions do not enter through `process_msg`, and render
code never starts asynchronous work.

### UiNode Builders (`ui_builders/`)

Pure functions converting domain data into `UiNode` trees. Free of `App`
and ratatui dependencies for testability.

| Module | Responsibility |
|--------|----------------|
| `stats.rs` | `stats_node(diagnostics, task_summary, shell_handles_count) -> UiNode` |
| `plugins.rs` | Re-exports plugin management builders from `crate::plugin::management_ui` |
| `shell.rs` | `shell_detail_node(entry) -> UiNode` for `/shell-show` |

### Shared `UiNode` Surface

The portable `UiNode` schema (`codegg_protocol::ui`) is used for both
plugin UI and selected first-party surfaces:

```
Domain data -> UiNode builder -> UiNodeRenderer -> ratatui / line output
```

- **Builders** live in `src/tui/ui_builders/`.
- **Renderer** (`UiNodeRenderer` in `src/tui/components/ui_node_renderer.rs`)
  is the single lowering adapter.
- **Generic dialog** (`UiNodeDialog` in `src/tui/components/dialogs/ui_node.rs`)
  accepts `UiNode`, supports scroll/page/jump, reuses `DialogType::Plugin`
  slot in FocusManager.

Use `UiNode` for: read-only informational surfaces (tables, key-value
lists, text/code dumps, scrollable summaries, plugin dialogs).

Do NOT use `UiNode` for: interactive components needing focus management
(permission, question, command palette, diff, tree, shell interactive).

## Configuration Surface

No direct TUI config keys in `opencode.jsonc`. TUI state is driven by:
- Theme selection via `[theme]` config
- Keybindings via `[keybindings]` config
- Vim mode via config
- Agent/model selection stored in session

`ActionKey::all()` and `ActionDescriptor` in `src/tui/input.rs` are the single
exhaustive catalog for configurable actions. The keybinding dialog and
keyboard help consume that catalog; prompt characters remain an explicit
non-configurable exception. Exported bindings use the same snake_case names
accepted by config import, and collision checks reject overwriting another
action.

## Invariants & Gotchas

- **`bool::then_some` is eager; `bool::then` is not.** This killed the TUI on
  the first message of a new session. The render throttle read `elapsed` twice
  per loop iteration — once for `should_render`, once to compute how long
  remained — and the work in between (starting a turn, dispatching a submit)
  makes the second read larger. Written as
  `(elapsed < render_interval).then_some(render_interval - elapsed)`, the
  subtraction ran *before* its own guard, and `Duration`'s `Sub` panics on
  underflow rather than wrapping: `overflow when subtracting durations`. It is
  `render_delay_after` in `runtime/event_loop.rs` now, lazy *and*
  `saturating_sub`. Two rules follow: never put arithmetic in `then_some`, and
  remember that the render's `catch_unwind` only covers `render_app` — the
  throttle maths sits outside it, so a panic there takes the process down
  rather than degrading.
- **An empty authoritative load must not delete local rows.**
  `apply_session_messages_loaded` used to `clear()` unconditionally. A
  brand-new session has no persisted rows yet, so the first load after sending
  returned zero and erased the optimistically-rendered prompt. It now clears
  only when the load carries rows, or when there is nothing local to lose.
- **The chat viewport is centred on the selection, not clamped to it.** It
  used to end on the selected message's last row, so scrolling up left a
  screen of blank rows *below* the selection and made every later message
  unreachable. `chat_browser::window` centres and clamps; tail-follow still
  lands on the newest message.
- **The composer names one destination: the surface with the keyboard.**
  `App::composer_destination_title` → `[LLM]` / `[CHAT]` / `[SESSIONS]`, via
  `SidePanelId::title`. The row used to render `[BUILD]` *and* `[CHAT]`
  unconditionally alongside the agent name (`[INS] [build][BUILD] [CHAT]`):
  two badges about one axis, one of them permanently lit. A panel that has no
  title is a destination the user cannot name, so `title()` is mandatory for
  every `SidePanelId`, not decoration.
- **The directionals are one gesture family with two meanings.** `Shift+←/→/↑/↓`
  and the vim `Shift+H/J/K/L` twins all resolve to `InputAction::Direction*`
  and then to a single contextual dispatcher,
  `commands::panels::move_directionally`: **a dockable box is open → the box is
  moved into the side panel and the box goes away**; **no box is open → the
  direction selects a pane** (`→` takes the keyboard into the column, `←`
  returns it to the main buffer, `↑`/`↓` cycle which panel type is loaded).
  The key says *which way*; the surface underneath says *what is there*.
  Resolving this once is what keeps a box and its panel from implementing the
  same rule twice and drifting — and it is why `ChatDialog`, `HelpDialog` and
  the terminal view can all be dockable without any of them knowing what
  "dock" means.
- **A modal short-circuits key routing, so a box must recognise the
  directional itself.** `App::on_key` returns into `handle_dialog_key` before
  the binding map is consulted whenever a dialog is open. A box therefore calls
  the shared matcher `panels::direction_of` in its own Normal-mode handler and
  emits `TuiMsg::MoveDirection`; the main buffer arrives at the same dispatcher
  via the `InputAction` arms. One matcher, two doors.
- **`Shift+Up/Down` and `Shift+J/K` are no longer reserved.** They used to be
  deliberately left unbound for a stacked-pane layout, on the grounds that
  binding an inert key advertises a capability the UI does not have. They are
  bound now because they are no longer inert: with a two-pane layout `↑`/`↓`
  cycle the loaded panel type. `the_vertical_pair_is_now_bound_rather_than_reserved`
  in `tests/modifier_chords.rs` is the test that says so.
- **Uppercase letters only; bare lowercase is never the directional.** A
  terminal reports `Shift+L` as `Char('L')` with or without SHIFT, but never as
  `Char('l')`. Binding lowercase would claim the plain within-surface vim verbs
  for a gesture the user did not make.
- **Boxes open in Insert, and that is why they track a mode.** `/chat` and
  `/help` open ready to type: the box is opened in order to write in it or
  search in it, and a mode switch before the first keystroke taxes the common
  case. Normal exists for the gestures that need a non-typable key — above all
  the directional, which in Insert is the letter `L` and must stay typeable so
  it can be searched for. The mode badge (`INS`/`NOR`) is identical across the
  chat and help boxes on purpose.
- **`/chat` flips to Insert *after* the first snapshot.**
  `ChatDialog::apply_snapshot` adopts the shared project draft only while the
  window is in Normal mode — the guard that stops a live event from clobbering
  a half-typed message. Flipping the mode first would silently drop a draft the
  user wrote and never sent.
- **The box→panel map is one `match` arm in `App::dock_target`.** Thirty-one
  boxes each map to their panel; everything else returns `None` and the
  gesture warns rather than silently navigating. A dockable box with no panel
  counterpart is a bug, not a silent no-op. `DialogType::Control` exists
  separately from `DialogType::Team` for exactly this reason — both are
  `InfoDialog`, but they are different panels, so they cannot share an
  identity.
- **The docking gesture is intercepted once, for every box.**
  `App::handle_dialog_key` resolves `Shift+←/→/↑/↓` before handing a key to
  the mounted modal, gated on `Component::docks_as_viewer()`. A viewer box
  owns no text buffer, so `Shift+L` is free to mean "dock me" — and 28
  dialogs do not each re-implement the same check. Two boxes (`/models`,
  `/sessions`) own a live filter, so they declare `docks_as_viewer() == false`
  and answer the gesture themselves under the same "filter is empty" guard
  that already governs their `j`/`k`. That guard is the dialog's existing
  rule, not a second one.
- **A panel with nothing to show refuses focus.** Chat with no bound project,
  Terminal with no handle: the host routes *every* key to the active panel, so
  a panel that cannot act would swallow the composer's keystrokes. `select`
  activates a panel *before* asking whether it is empty, so a panel that
  adopts its content on activation (Terminal adopts the controller's active
  handle) is not judged empty for exactly one frame.
- **Clearing a terminal is local and must not fake a resync.**
  `InteractiveTerminalController::clear_scrollback` drops the scrollback and
  resets the emulator but deliberately preserves `next_seq`: the daemon keeps
  streaming, and resetting the sequence would make the next honest chunk fail
  the gap check and print a "history expired" notice the user did not cause.
- **The help panel hosts the dialog, it does not reimplement it.**
  `HelpPanel` owns a `HelpDialog` and paints it with `HelpDialog::render_into`,
  a `Buffer`-level method `Component::render` adapts over. That is the whole
  difference between "docking moves the box" and "docking gives you a similar
  panel" — with a shared widget there is nothing to keep in sync, so the box
  and the panel cannot drift the way the previous chat pair did.
- **Docking a hosted box is a move, not a re-open.**
  `App::close_dialog` returns the component it unmounted and
  `SidePanelHost::adopt` installs it in the target panel, so the column starts
  with the same filter, cursor, scroll offset and selection the box was
  showing a frame earlier. A panel with nothing adopted that cannot be
  materialized (no pending plan confirmation, no security receipt) still
  renders a placeholder — an empty bordered box is indistinguishable from a
  rendering bug — but the placeholder describes **state**, never instructions:
  nothing loaded, a load in flight, or the reason it cannot act. It used to
  read "Run `<cmd>` to open it, then Shift+← to move it here", which described
  a workflow the surface does not have: the panel is already in place, so there
  is nothing to move into it, and `Shift+←` names a different edge on every
  placement.
- **Selecting a hosted panel loads it: one step, not two.**
  `commands::panels::select_panel` materializes an empty hosted panel before
  selecting it, through the same seams as its `/` command
  (`materialize_hosted_panel`/`open_panel_box`). A synchronous snapshot box
  is opened and adopted immediately, so `Shift+S` + `Enter` lands on live
  content with focus — the behaviour the bespoke panels always had. A
  live-loading box (Research, Task view) stays open as a modal instead:
  adopting its spinner would freeze it and cancel the load, so it keeps the
  keyboard until its data arrives. Panels with genuinely nothing to show
  (Plan with no pending confirmation, Security with no receipt) are selected
  and show why. `Shift+↑/↓` walking deliberately does not materialize: cycling
  all entries would fire dozens of loads for panels the user only glanced at.
- **`PanelFill` is the contract, not `dock_target`.**
  `open_panel_box` used to return `()` and its caller re-read
  `App::dock_target()` to infer which of three things had happened — with a
  fourth, "the panel did nothing", looking identical to the first. The
  mid-load special case lived in `matches!(id, Research | TaskView)` at the call
  site, so a panel arm could not state its own answer. Every arm now returns a
  [`PanelFill`]: `Filled`, `Loading`, `StillLoading`, `OpenedAs(other)`,
  `Cannot(reason)`, or `Bespoke`. The `StillLoading` variant *is* the old
  literal list, moved next to the work that needs the protection.
- **`render_hosted` is a bridge, not the destination.**
  `SidePanel::render` receives a `&mut Buffer` while every `Component::render`
  takes a `&mut Frame`, and ratatui's `Frame` has no public constructor. The
  bridge renders the widget into an in-memory backend the size of the column
  and copies the cells out, so a dockable box can be adopted without
  rewriting that box's render path. A widget needing real cursor or clipping
  semantics in the column should still grow its own `render_into`, as
  `HelpDialog` did.
- **Dialog::Info doesn't exist**: `Dialog::Info` is NOT in the Dialog
  enum. `components/dialogs/info.rs` exists but `InfoDialog` uses
  `DialogType::Context`, `Cost`, `Usage`, `Stats`, `ShellShow`,
  `TerminalShow`, etc. via `dialog_type_for_info_type()`. One widget, one
  file, and therefore one seam override covering every one of them — but not
  all 17 `InfoType`s are reachable as a hosted panel. `TerminalShow` resolves
  to `DialogType::Terminal`, a bespoke panel that owns its own `SidePanel`
  impl and never renders through `render_hosted`; `Agents` resolves to
  `DialogType::Agent`, bespoke *and* the one panel
  `discards_a_docked_box()` names; and `WorktreeList` resolves to a
  `DialogType` that `dock_target` omits entirely, so the gesture refuses.
  Fourteen remain reachable-hosted, and `ProjectInit` shares `DialogType::Team`
  with `Team` — two content types, one panel, which is why `keymap_hint` is
  written per `InfoType` and no test here may key on `DialogType`.
- **DialogType in component.rs**: `DialogType` lives in
  `src/tui/components/component.rs` (not `types.rs`, and not a
  `component/component.rs` — that directory holds only `context.rs` and
  `focus.rs`).
- **Dialog::Plugin is generic**: A single `Dialog::Plugin` variant
  handles all plugin dialogs. `UiNodeDialog` also reuses this slot.
- **Dispatch is async-capable, not async-free**: `dispatch_tui_command` is
  `pub(crate) async fn` with five `.await` points, all in the durable-editor
  `Edit*` arms. Most arms remain synchronous.
- **Git sidebar is cached, not live**: Render reads from
  `session_state.git_sidebar`; never shells out to git.
- **Remote TUI is event/state-driven**: `RenderFrame` is unsupported.
- **State domains are 27 modules**: Not the 6 listed in older revisions;
  the domain model expanded across multiple milestones.
- **Async command stale-completion tests**: Each guarded handler has a
  stale-completion test in `src/tui/mod.rs::async_cmd_tests`.
- **The focus manager owns the painted dialog, not `dialog_state`**: 
  `open_dialog` pushes a `clone()` of the dialog onto `FocusManager`, and
  `render_dialog` draws whatever the focus manager holds. Filling only the
  App-owned `dialog_state` field therefore leaves the *painted* instance on
  the empty snapshot captured at open time. Any async path that populates
  a dialog must write through `focus_manager.with_dialog_mut::<D, _>(..)`
  (or `dialog_mut`) as well. `/sessions` shipped broken for exactly this
  reason — see `reload_fills_the_painted_focus_manager_instance` in
  `src/tui/commands/sessions.rs`. The C002 subagent/job sections obey the
  same rule on open *and* on every live refresh
  (`App::refresh_sessions_dialog_pending_rows`), and
  `tests/tui_sessions_pending_sections.rs::both_instances_receive_the_sections`
  reads the painted clone rather than the App-owned field.
- **The terminal's own text selection is unavailable**: `EnableMouseCapture`
  (`src/tui/terminal.rs`) makes the terminal stop offering drag-select, so
  the TUI implements its own over the rendered frame buffer
  (`src/tui/selection.rs`). `App::finish_render` retains the frame buffer
  each pass; drag inside `ClickTarget::Viewport` sets
  `selection_anchor`/`selection_focus`, mouse-up copies via `arboard`, and
  `Ctrl+C` copies a live selection (falling through when none exists).
  Selection is deliberately scoped to the viewport so the sidebar,
  scrollbar, dialogs, completion popup, and prompt keep their click
  behaviour.
- **`bottom_border_area` gets its own row, carved off before layout**:
  `render` splits the last row of the main pane out of `main_area_inner`
  *before* `session_layout` runs and records it as `bottom_border_area`;
  `finish_render` paints a `Borders::BOTTOM` block into it. Anchoring the
  edge on the footer's last row instead — as `render_outer_borders`
  originally did — painted over the only row the status bar ever had,
  because `footer_height` is 1 and `StatusBar` takes a border-less early
  path at that height. The status line disappeared entirely.
- **One completion popup, not two**: `CommandPalette::render` used to paint
  a second, narrower slash popup over the prompt whenever `command_mode`
  was set, while `update_completions` deliberately left `show_completions`
  false in that mode. Typing `/logs` cold showed the old box; backspacing
  to a bare `/` cleared command mode and showed `render_completions`
  instead — one interaction, two visually different boxes with two
  independently fuzzy-scored orderings. `render_completions` is now the
  only popup and `handle_command_key` navigates `completion_rows()`, so the
  highlighted row is always the row Enter executes. The popup also scrolls
  (`completion_scroll_offset`) so a selection past the 8-row window stays
  on screen and click hit-testing matches the drawn window.
- **The chat tail carries a live activity line**: `MessagesWidget::activity`
  renders `⠋ thinking 3s` / `⠹ running bash 12s` while the agent works, and
  a finished reasoning block's collapsed header reads `Thought for 3.2s`
  (`reasoning_durations`, click to expand via `message_has_reasoning` ->
  `toggle_reasoning`). Spinner frames are derived from elapsed whole
  seconds, not a per-frame counter, so the animation is frame-rate
  independent and reproducible under a `TestBackend`.
- **Completion popups resolve foreground against the selection fill**:
  `render_completions` picks label/description foregrounds via
  `readable_on`, not by trusting the theme palette. Themes set
  `ui.selection` from a widget slot rather than deriving it from the
  background, so an accent and the selection fill can share a luminance —
  resolving against the popup background renders the highlighted row as an
  unreadable block.

## Testing

```bash
cargo test --test tui_render      # 99 render regression tests
cargo test --test tui              # integration tests
```

### Render Regression Tests (`tests/tui_render.rs`)

Uses `ratatui::backend::TestBackend` across terminal sizes:
tiny (40×12), small (60×20), normal (100×32), wide (160×40),
tall (100×60).

Coverage: empty/home state, active session with messages, streaming,
tool calls, sidebar file changes, all 30+ dialog variants, completion
overlay, toasts, pathological content, component panic injection,
combined states.

Key patterns: `render_app_to_buffer(app, w, h)`,
`assert_render_ok(app, w, h)`, `text_in_buffer(buffer)`,
`buffer_contains(buffer, needle)`.

## Related Docs

The `TuiDocumentSession` M005 seam wraps `codegg-client::DocumentController`
for future editor flows. It owns only presentation cursor/selection/viewport
placeholders; the shared controller is the single local text replica. M005
does not add editor widgets, keybindings, or a second TUI text buffer.

- [agent.md](agent.md) — AgentLoop that processes TUI commands
- [bus.md](bus.md) — GlobalEventBus and event types
- [session.md](session.md) — Session storage

## Source verification

Verified 2026-10-06 against `src/tui/`: `app/mod.rs` line count
(14,358), the 27 `app/state/` modules (28 `.rs` files), `App` at
`app/mod.rs:222`, `Component` at `components/component.rs:183`,
`TuiMsg` (116 variants) at `app/types.rs:123`, `Dialog` (46) at
`app/types.rs:2`, `DialogType` (46) at `components/component.rs:22`,
`TuiCommand` (203 variants) at `app/commands.rs:15`, `InputAction`
(50) at `input.rs:102`, `TuiTaskKind` (13) and `TuiTaskRegistry`
(`task_lifecycle.rs:133`), `GitSidebarState` (`session.rs:134`),
`UiState` (`ui.rs:40`), `SessionState` (`session.rs:71`),
`AgentState` (`agent.rs:6`), `AsyncUiRequestState`
(`async_request.rs:20`), the 22 `DialogState` async-request fields,
`ComposerMode` (`work_orders.rs:25`), `Route` (`route.rs:2`), and the
five `.await` points in `dispatch_tui_command`
(`runtime/command_dispatch.rs:84`). Render-regression test count (99
`#[test]` in `tests/tui_render.rs`) confirmed.

Re-verified during the subagent/job visibility roadmap against `src/tui/`:
`app/mod.rs` line count (18,815), the **31** `app/state/` modules (32 `.rs`
files incl. `mod.rs`), `TuiMsg` (149 variants) at `app/types.rs:123`,
`InputAction` (62) at `input.rs`, and the render-regression test count (111
`#[test]` in `tests/tui_render.rs`). The Directory Structure counts below were
corrected in the same pass: `commands/` (37 submodules), `components/dialogs/`
(40 dialogs), `command.rs` (164 built-ins), and `app/state/` — which had been
missing `panel_hotkeys.rs`, `subagents.rs`, `subagent_tabs.rs`, and `tasks.rs`
entirely, so the map silently understated the module count.

Re-verified 2026-10-06 against `src/tui/` and
`scripts/check_tui_*`. Corrected the Invariants entry that placed
`DialogType` at `src/tui/components/component/component.rs` — that path does
not exist; the enum is at `src/tui/components/component.rs:22`, and
`components/component/` contains only `context.rs` and `focus.rs`. Replaced
`RemoteTuiMessage::PluginUiEffect` with `TuiMessage::PluginUiEffect`;
`RemoteTuiMessage` is only a local import alias
(`src/tui/app/mod.rs:79`) for the `TuiMessage` wire enum, which derives
`Serialize`/`Deserialize`, whereas `TuiCommand` (`app/commands.rs:14`) is the
in-process runtime channel and is never serialized. Corrected the
`app/mod.rs` line count to 14,358. Re-confirmed accurate: the five
`.await` points in `dispatch_tui_command` (`runtime/command_dispatch.rs:84`,
at 2109/2160/2214/2268/2320, all inside the `EditUndoLatest` /
`EditReapplyLatest` / `EditUndo` / `EditReapply` / `EditCheckpointList`
arms), `TuiCommand` (203) and `InputAction` (50) counts, the 46-variant
`Dialog`/`DialogType` pairing, and both static guards
(`check_tui_project_authority.py` scanning `session_state.project_dir` and
`std::env::current_dir()`; `check_tui_editor_text_authority.py` rejecting
second text buffers and direct filesystem reads).
### Editor LSP presentation (M006-C)

The editor consumes the daemon-owned `LspDiagnosticsGet` and `LspReadGet`
surfaces. Diagnostics are pulled when a document opens, after a successful
save, and on F2. F2 opens a bounded list with severity, message, and source
range; gutter markers identify lines with diagnostics. A `Ready` result
replaces the complete current-file set, including clearing it when empty.
`NotReady`, request failure, and transport loss preserve the last complete
set and mark it stale where one exists. The UI does not poll or claim live
push updates.

F3 requests hover, F4 definition, F5 references, F6 document symbols, and F7
workspace symbol search for the word at the cursor. Results are bounded and
rendered as text. A location is navigable only when it includes the optional
daemon-validated `workspace_relative_path`; the TUI passes that candidate to
the existing `DocumentOpen`, which performs final containment and
`file.read` authorization. Older or outside-workspace locations remain
display-only.

The presentation state is bound to session, project, workspace, path, and
editor generation. Semantic-token overlays are accepted only for the same
controller snapshot revision and valid non-overlapping UTF-16 ranges;
unknown token types and malformed/stale results render as plain text. The
TUI retains no document text outside the controller snapshot.

### Editor LSP keymap and its degradation contract (M013)

M013 bound Helix's LSP gestures into the M005 leaf tables
(`src/tui/editor.rs`: `SPACE_LEAVES`, `GOTO_LEAVES`) rather than adding a
second prefix mechanism. `PrefixAction::Effect(EditorPendingEffect)` is a
fourth arm rather than a bool or an `EditorCommand`, because an LSP leaf moves
no cursor and edits no text. (M010's W1 generalized the arm: it was
`PrefixAction::Lsp(EditorLspLeafEffect)` until then, and the effect enum was
LSP-typed because a file picker is not an LSP fact. The arm, the enum and the
pending slot now live in `src/tui/editor_effect.rs`.)

| Key | Effect |
|---|---|
| `space k` | hover |
| `space s` | document symbols |
| `space f` | open the workspace file browser |
| `space ?` | open the editor's own `:` command palette |
| `space y` | copy every selection |
| `space Y` | copy **only** the primary selection |
| `space p` / `space P` / `space R` | paste after / paste before / replace with the clipboard |
| `space S` | workspace symbols, queried with the token at the cursor |
| `space d` | refresh and open the diagnostics panel |
| `space h`, `gr` | references |
| `gd` | definition |
| `gi` | implementation |
| `]d` / `[d` | next / previous diagnostic, cursor moves |
| `gy` | **unavailable**: no language server implements type definition here |
| `space F` | **unavailable**: one workspace root, and `space f` opens it |
| `space b` | **unavailable**: one document is attached at a time |
| `space j` | **unavailable**: no visited-position history |
| `space D` | **unavailable**: no server capability backs a separate workspace-diagnostics request |
| `space c` / `space C` | **unavailable**: no comment syntax is known for the edited buffer |
| `space '` | **unavailable**: the pickers are not fuzzy, so there is no query to recall |
| `space a` | the **code-action read** — the actions the server would offer at the cursor, each with the diff of its own edit; the panel is the picker |
| `=` | the **format preview** — what the formatter would change, and the change review when it can be applied |
| `space r` | the **rename preview** — what a rename would touch, and the change review when it can be applied |

**The two halves of a leaf run at different authority levels.** The prefix
resolver holds `&mut EditorState` and can decide *what* was asked for; only
the App holds a core client, a task registry and toasts, so it owns *how* the
request runs. The leaf parks its effect on `EditorState::pending_effect` and
`handle_editor_key` drains it on the very next statement. It is one hop, not a
queued channel: a leaf can never leave a request armed, and a document change
discards it — the effect is not LSP state, so an LSP scope change no longer
clears it, but a *document* change does, because an effect aimed at a document
the editor no longer shows is stale.

**The effect enum is editor-level, not LSP-level** (M010). It was
`EditorLspLeafEffect` while the only leaves were LSP reads, which left a file
picker and a clipboard verb with nowhere to live: they are not LSP facts, and
`EditorLspState` is reset wholesale on a scope change, which is the wrong
fate for an effect that is not about the language server at all. The type
moved to `src/tui/editor_effect.rs` and now carries `Clipboard`,
`YankPrimary`, `OpenFilePicker` and `OpenCommandPalette` beside the reads.

**The file browser is daemon-owned, so the editor's gesture is a *request* for
a root, not a walk.** The listing is resolved from the active tab's canonical
project/workspace binding (ADR-0022, C005); the TUI never walks its own
filesystem, and the local root is only fencing state. So `space f` checks the
binding **before** it opens anything: with no workspace root, or with a root
that has no canonical project/workspace ids, it says so and opens nothing —
a browser that opens empty is a key that appears to do nothing. When the
binding is there, the browser is seeded from the shared tree state, and its
title is a breadcrumb over that state's own root, so it names the root it is
browsing. `App::open_file_picker` is the one place that knows how to seed it,
so `/files` and `space f` cannot disagree.

**The space menu has no dead keys** (M010). Every Helix space glyph is either
bound, bound to `Unavailable` with a reason, or documented as out of scope
with its reason. `space f` used to be a *second* binding of `space s`, which
left the file browser unreachable from the editor route and gave the symbols
panel two names for one request; the reclaim is a binding change, not a
rename, and `CHANGELOG.md` records it under `### Changed`.

**A preview is a read, and — when it is complete — the editor's preview can
apply** (M015, M016). `=` and `space r` reach `LspReadOperation::FormatPreview`
and `LspReadOperation::RenamePreview` — the two operations M013 delivered with
daemon arms and panel renderings and **no key**. They show what would change,
and since M016 a preview that is complete enough carries the **material the
apply path consumes**, so the chord leads to the change review rather than
only to a picture of it. The difference from Helix is deliberate and recorded:
its `=` formats and its `space r` renames.

The apply path is **ADR-0008's, and it is accepted** —
`CoreRequest::LspPreviewApply`, authorized `via_session` + `file.modify`,
applying a digest-bound candidate through the change-review surface or the
model-facing preview-apply tool. The editor reaches it by **carrying** the
candidate: the daemon mints it into the read result, the editor holds it, and
the review's accept builds the one request through `build_apply_request` — the
single place a `CoreRequest::LspPreviewApply` is constructed, which
`scripts/check_tui_editor_apply_path.py` enforces structurally. Nothing is
registered, and no registry is needed: `apply_preview` recomputes the digest
over the request's own patches, so a candidate altered in transit fails the
check.

`=` is a **Normal-mode** chord with no prefix, so it is attributed by the
key-authority guard as one — the count moves. `space r` is a **prefix leaf**,
and it is not a read: the read follows the prompt, so the leaf names the
*step* (`EditorPendingEffect::RenamePrompt`) and the request is issued when
the `:` line is submitted. The new name is collected through the `:` command
line, M008's precedent, rather than through a second input mode; an empty
name and an over-long one are refused before a request is spent on them.

`space a` is the **code-action read**, and it is bound to a read: the server
capability exists, and `LspReadOperation::CodeActionPreview` now asks for it.
The leaf is a *point* operation, so the request carries the cursor's line and
column and the range is zero-width there.

**A code action is a choice, so the panel is the picker.** The read answers
with a *list*, and the editor does not open a review on the first entry: it
would be choosing for the reader. `j`/`k` move the selection (the panel's row
count is the action count), and `Enter` opens the change review on the
**selected** action's candidate — the same carried review M016 built, reached
through a picker instead of through a single preview. `candidate_of` does
*not* lift a code-action candidate out of the payload, which is what keeps the
review from opening before a choice is made.

**The reason an action cannot be applied is per action, not per result.** One
action in a list can be command-only while the one beside it is perfectly
applicable, so a result-wide flag could only say "something was cut" and the
reader would not know which. The reason is therefore carried on the action
itself, and the panel and the `Enter` handler read it through **one** helper
(`code_action_unavailable_reason`) so the two cannot drift apart: the panel
says why in the list, and pressing `Enter` says the same thing in the place
the key was pressed.

**The window follows the selection.** The panel is at most twelve rows tall,
and a server can offer more actions than that. The code-action arm renders a
window of eight that **ends on the selected action**, and the panel's scroll
offset becomes that window's offset rather than the selection's index. This is
the one payload that overrides the panel's standing scroll, and it does so for
a reason the other payloads do not share: here `Enter` applies a *change*, so
an off-screen selection would offer to apply something the reader could not
read. The other pickers in the panel (diagnostics, locations, symbols,
highlights, signature help, rename files) keep the standing behaviour, which
shows the first rows and the marker on the selected one; a selection beyond
the visible rows is invisible in those panels too. That is a **pre-existing**
panel property, inherited rather than introduced, and changing it for the
other payloads is a separate concern with its own milestone.

**One request, many previews.** The alternative shape — summaries first, then a
second request naming the chosen action — has a hazard this shape does not: an
action's *index* is only meaningful for the list the server returned in that
moment, and a second request may return a different list, so the index would
name a different action than the user picked. One request removes the index
entirely, and the candidate is computed from exactly the action that is
shown. The request count is measured, not assumed: the egglsp integration test
reads the fake server's own transcript and asserts that it saw exactly one
`textDocument/codeAction`.

**The preview/apply transition landed, and it was registered before it did.**
M015 recorded the change of meaning while the preview was what a reader
learned; M016 built the bridge, so `=` and `space r` open the change review
on the candidate. The chords do not move, so the change is invisible to
anyone who did not learn the preview — which is why the roadmap, the closure
record and the `CHANGELOG.md` record it, and why the preview panel says
*ready to apply* rather than *not applied*.

**The apply writes to disk, so the editor reloads what it wrote.** The apply
is the daemon's checked path — its checkpoint, its hash revalidation, its
rollback and its LSP re-synchronization — not an editor edit, so the editor's
buffer is not the thing that changed. A clean buffer is reloaded after a
successful apply; a buffer with a draft is **not**, because a reload discards
the local draft and the daemon's dirty-document check is not the editor's:
the editor is a replica, so the daemon can believe a document is clean while
the replica holds edits it has not yet pushed. Discarding those edits to make
the screen tidier would lose work, so the editor keeps them and says the file
changed on disk.

**Degradation is a status row, not a toast.** `EditorPendingEffect::Unavailable`
carries the reason a key is unbound, so a Helix user pressing `gy` learns the
capability is missing rather than assuming the keypress was dropped. A
`NotReady` read prints "Language server not ready"; an `Ambiguous` one names the
several servers running; an `Unsupported` one names the missing server
capability. None of them falls through to "No results", which would read as a
clean workspace.

**Capital leaves resolve on both terminal conventions.** `PrefixLeaf::glyph`'s
doc always claimed the dispatch reconciled `Char('S')`+`NONE` with
`Char('s')`+`SHIFT`; the lookup gated on `NONE` alone, so `space S` and `space
F` would have been live on one terminal and dead on another. `leaf_lookup_char`
now upper-cases under `SHIFT`, which makes the doc true.

F2–F9 are untouched: the function-key `match` in `handle_editor_key` returns
before `editor_buffer_key` runs, and a leaf is a *second* key after a prefix, so
the two cannot collide.

### Completion popup and transactional acceptance (M004)

The completion popup consumes the M003 `LspCompletionGet` surface and
nothing else. The trigger is `F8` (claimed inside `handle_editor_key` in
**both** modes, so it never falls through to the global bindings) and
`:complete` from Normal mode; terminals that cannot send `F8` always have
the command spelling. `F8` is spoken for here — the syntax-highlight
retry is `F9`, because a highlight refresh that silently took `F8` would
make completion unreachable from a terminal that can send function keys. While open, the popup owns exactly `Up`/`Down`
(`j`/`k` only in Normal mode, where they are navigation rather than text),
`Enter`, and `Esc`; every other key dismisses first and processes normally,
so a keystroke is never both typed and interpreted as popup input. `Esc`
dismisses and preserves the editing mode — it never quits the editor — and
a re-trigger supersedes the in-flight request by request id.

The selection is advisory until acceptance. `Enter` re-validates scope,
generation, attachment identity, and revision against the current state,
resolves the DTO's UTF-16 range against the current snapshot, and submits
the single forward/inverse pair through the shared `submit_edit` path: one
controller edit, one undo unit, redo replays the forward text. Non-applicable
candidates, identity mismatches, stale revisions, lost writer leases, and
unresolvable ranges are refused with a notice and change nothing. The popup
clears on edit, undo/redo, save, reload, resync, close, detach, tab switch,
and reconnect; a late, superseded, or wrong-scope answer drops in the state
guard and never paints. Untrusted labels, details, and error strings render
as sanitized literal spans, and the popup hides (keys still live) on
terminals too narrow to fit it.

C003 status and fencing corrections (`src/tui/editor_completion.rs`).
Identity binding is split from status-specific revision policy: the
document id is verified for every status, but only `Ready` requires the
exact requested revision (plus a consistent sync witness when present).
`NotReady` keeps its constructor placeholder revision and `Stale` its
newer daemon-current echo — both render their typed states instead of the
generic revision Error. Every non-ready answer is disarmed (candidates
cleared), so a hostile or malformed peer can never smuggle suggestions
into a warming/stale/unsynced/unsupported popup, and a malformed Ready
(wrong revision, inconsistent witness) is an Error, never actionable.

Request fencing is a process-lifetime epoch, not just a counter: every
path that resets the request id (`clear`, `reset_for_scope`) bumps
`epoch`, and completions carry the epoch they were issued under
(`TuiCommand::EditorCompletionFinished.completion_epoch`). A queued
pre-reset answer can never satisfy a post-reset request even when scope
and request id coincide — the exact shape a reconnect leaves behind,
since reconnect clears completion without touching generation or scope.
Telemetry exposes counts only; no result contents, paths, or source text
are logged.

C003 trigger budget (`crates/codegg-document`). The F8 cursor resolves
through `utf16_column_bounded`, which walks borrowed rope chunks with a
32 KiB prefix cap matching the C002 line-render budget — no prefix
materialization, exact surrogate-pair counting. Past the budget F8
refuses visibly ("Line too long for completion at this position") rather
than sending a truncated or wrong UTF-16 cursor; ordinary lines resolve
exactly as before.

### Semantic token fidelity (C001)

Structural validation (`src/tui/editor_lsp.rs::validate_semantic_tokens`)
is separated from style mapping (`src/tui/components/editor.rs::token_style`)
and bounded drawing (`line_spans`). The frontend received-token cap is
`MAX_EDITOR_SEMANTIC_TOKENS`, which mirrors the protocol maximum
`MAX_LSP_SEMANTIC_TOKENS` (1000); the 100-item `MAX_EDITOR_READ_ITEMS` cap
applies only to read-panel item lists. Responses with 101/500/1000 valid
tokens highlight normally; only a response exceeding 1000 fails closed as a
whole.

Well-formed unknown token types (including ordinary `variable`,
`parameter`, `property`, and `namespace`) are accepted structurally and
render as plain text while valid neighbors keep their themed styles.
Malformed spans (zero length, out-of-range line, split surrogate,
out-of-range UTF-16, empty/inverted byte range, overlap, oversized line)
are dropped individually without shifting later absolute offsets; only the
over-cap case rejects the whole response. The daemon `truncated` flag is
preserved in `EditorLspState::semantic_truncated`, and any local drop,
whole-response rejection, or daemon truncation sets
`semantic_incomplete` and shows `highlighting truncated` / `highlighting
partial` in the status row. A truncated result is never represented as
complete coverage. A `Ready` with zero tokens is a valid empty highlight
(`revision=Some`, both flags false), never `NotReady`.

Supported classes map through `Theme` slots (keyword→primary bold,
string→success, number→warning, comment→muted italic,
function/method/macro→secondary, type/class/enum/struct/interface/
typeParameter/namespace→primary, variable/parameter/property and other
standard kinds→foreground) so highlighting stays legible on dark and light
themes; patching preserves the current-line background and diagnostic
gutter. Local edit, undo, redo, reload, resync, save, attachment switch,
and reconnect clear stale semantic state via `clear_semantic`; late
completions after any of those are discarded by generation, scope, and
snapshot-revision checks with no server calls on the render path.

### Bounded rendering and tree scheduling (C002)

Per-frame editor work is bounded in input bytes, not output rows.
`MAX_RENDER_LINE_BYTES` (32 KiB) caps source bytes examined for one visible
logical line; `MAX_FRAME_SOURCE_BYTES` (256 KiB) caps the frame total across
`render_text` and `longest_visible_width` together. `line_content_end`
inspects at most the last two bytes for a terminator instead of reading the
line; `line_slice`/`line_spans` read at most the 32 KiB prefix window;
`display_column`/`offset_at_column`/`resolve_viewport` saturate at the window
instead of scanning a multi-megabyte line. `measure_frame_cost` mirrors the
render loop exactly so tests assert deterministic budgets rather than
wall-clock time.

A line past the per-line budget renders its prefix as plain text (no
per-token spans, which would cost the whole line) with a warning-styled
`…[too long]` row suffix; rows past the frame budget render as the marker
alone with zero content bytes read. The banner adds `line too long —
showing prefix` when the cursor line or cursor column is past the window, so
a truncated frame is never presented as complete. Cursor and selection byte
offsets stay exact; horizontal motion past the prefix clamps to the prefix
end. Ordinary short lines render exactly as before.

The tree walk counts *examined* entries independently of retained nodes
(`MAX_TREE_EXAMINED` 100,000 vs `MAX_TREE_NODES` 4096): ignored, symlink,
special, unreadable, and iterator-error entries consume the examined budget
while retaining nothing, and the truncation reason names which budget fired.
Iterator `Err`s are counted via `account_one_entry` (C004): the old
`entries.flatten()` silently dropped errors without counting work or checking
cancellation, letting an error flood bypass both bounds. `TreeListing`
carries the examined count as result metadata.

**Depth is a window, not a navigation limit.** `MAX_LISTING_DEPTH` is 3,
single-sourced into the TUI as `MAX_TREE_DEPTH`. One listing is scoped:
`WorkspaceDirectoryList` carries a `prefix`, and `l` re-walks the selected
directory as a new root, so a three-level embed bounds one response rather
than the workspace. It was 12, and the deep embed made the *wrong* budget
bind. The wire enforcer keeps the longest **top-level prefix** that fits
512 KiB, so one directory with a deep subtree monopolises the budget and
every sibling root is trimmed away — measured on a 75,956-entry repository
at depth 12, that left exactly **one** root node, and `/files` rendered a
single unselectable row with no second row to move to. Raising
`MAX_TREE_NODES` does not fix it (65,536 nodes still yielded 4 roots): the
binding constraint was bytes, not node count. At depth 3 the same repository
retains all 37 roots in 290 KiB. `LISTING_IGNORED_DIRS` also gained
`.codegg` — this application's own per-project runtime state, 21,482 entries
in a CodeGG checkout and, being second alphabetically, a consumer of the
entire retained-node budget before any source directory was walked. The other
walkers here (`python_script::snapshot`, `python_script::executor`,
`research::sources::local_repo`) already skipped it; the list had drifted.

C004 single-flight admission (`src/tui/file_tree_coordinator.rs`): at most
one blocking scan executes (global cap 1, preferred per plan), with at most
one coalesced latest-only pending intent. The permit is held from event-loop
`try_admit` until the blocking work's completion arrives; aborting the async
waiter never frees capacity early, so the old "one live task" registry count
(which counted waiters, not workers) is replaced by instrumentation
(`running`/`peak_running`) proving peak actual workers ≤ 1 across 100
superseding rebuilds. A rebuild arriving while a scan is active supersedes
intent immediately (generation bump + cancel flag) but spawns nothing and
aborts nothing; the active scan drains as one accounted, finite-budget prior
operation, then the pending latest runs. The newest request shows an explicit
`Waiting` state ("waiting for prior scan…") while the prior scan drains; the
event loop stays responsive. Every scan is fenced to
request/generation/project/tab/root and revalidated before the delayed scan
and again before display, so late A after a switch to B never renders and
`/files` shares the one guarded state. A stale completion never cancels the
current request (that would strand the queued intent). Shutdown signals the
flag once, drops the queue, force-clears admission, aborts waiters, and
reports a bounded policy: a worker stuck in a kernel syscall is not joined
and no deadline is claimed; its `RunningGuard` still releases on exit and its
late completion mismatches and is discarded. Diagnostics (`diagnostics()`)
expose counts only, never paths. Walks run on `spawn_blocking` inside a
generation-scoped async task; the blocking scan checks a cooperative flag on
every entry, superseded by `begin_rebuild`, `reset_for_project`, and
`prepare_shutdown`, so a superseded scan exits within one *completed* entry —
not a wall-clock bound, since one syscall cannot be preempted. Stale
completions are still refused by generation/root/request. With no command
channel the UI reports an explicit error instead of requesting synchronously
on the event loop.

C005 daemon-owned listing (ADR-0022, accepted): the TUI no longer walks at
all. `src/tui/commands/file_tree.rs` sends `WorkspaceDirectoryList` with
the tab's project/workspace binding over the attached core client; the
daemon resolves its canonical root, verifies the project/workspace binding,
validates the prefix, and runs the single shared walk
(`src/directory_listing.rs`, extracted verbatim from the TUI walker) on a
`spawn_blocking` worker under a per-daemon single-flight semaphore.
Contended scans answer `busy`; the permit is held from admission until the
blocking walk completes. The wire response echoes `workspace_id`/`prefix`
and carries inert nested entries plus `truncated`/`truncated_reason`/
`examined`/`status`; responses over the 512 KiB serialized cap are trimmed
from trailing top-level nodes with an explicit reason. TUI-side single
flight is unchanged (one round-trip in flight, latest-only pending,
`Waiting`, generation/root/request fencing) with one addition: the echoed
daemon workspace identity must match the request before anything renders,
and failures arrive as `FileTreeFailed` with no local fallback in any mode.
`local_walk_root` remains only as the retired client-side refusal point
(kept with its tests, never called on a listing path); the new invariant
is enforced by `scripts/check_daemon_listing_authority.py`, which rejects
`read_dir`/`build_tree*`/`local_walk_root` on the listing path outside the
two walker homes. The background `@`-mention indexer in `tui/app/mod.rs`
(`index_files_sync`) is a separate client-side index explicitly deferred
by the C005 plan, not covered by that guard.

C005 flush fix: hitting a walk bound previously `return`ed out of the walk
before the retained-entry flush loops, so a node-budget truncation always
presented an empty tree alongside its (correct) reason. Bound hits now
`break` to the flush, keeping everything retained up to the bound with the
explicit reason. Pre-set cancellation still returns empty; mid-walk cancel
keeps retained-so-far as a truncated listing rather than blanking it.

C006 worker-owned admission and full-response cap (post-C005 corrective):
the daemon's single-flight `OwnedSemaphorePermit` is moved into the
`spawn_blocking` closure, so aborting the async waiter cannot free a
scanner that is still alive — the permit releases only when the blocking
closure exits, including error and panic unwind. Contention still answers
`busy`; abnormal exits surface sanitized `unavailable`. Actual worker
entry/exit is instrumented per daemon (`DirectoryListingProbe`:
`active`/`peak` counters plus a test-only blocking gate; production
installs no gate). The 512 KiB cap is enforced on the complete serialized
`CoreResponse::WorkspaceDirectoryList` (discriminator + `workspace_id` /
`prefix` + `status` + `examined` + combined walker/wire truncation reason
+ retained nodes) via the same `serde_json` serializer the socket/stdio
transports use — never a nodes-only estimate. Trimming removes trailing
whole top-level nodes in prefix order, down to zero retained roots if even
the last nested root cannot fit; an all-roots-removed `Ready` carries
`truncated=true` with a bounded wire reason and is visibly distinct from a
genuinely empty `Ready`. If even that cannot fit, the daemon fails closed
to `unavailable` (then a bounded `Error`), never an oversize payload. The
enforcer binary-searches the longest fitting prefix in `O(log n)` full
serializations, never `O(n)` pops. ADR-0022 Option B is unchanged; its
acceptance-date provenance is marked unverified (see its C006 note).

### The connection dialog's model pane is per-connection

`/connections` fetches a catalog for exactly one connection at a time, so
which connection the pane belongs to is part of the contract, not a detail.

`start_selection_refresh` resolved the focus only from an existing
`SessionSelectionDto::Selected`. A session with nothing bound therefore
rendered an empty model pane — and because the pane is the only way to bind
one, that is a closed loop: no models to pick, so nothing to bind, so still
no models. Focus now falls back to an `active` connection, then to the first
row, and `SessionSelectionRefresh { focus_connection_id }` carries the
user's highlight across the fetch.

The highlight arm matters for correctness, not just freshness. `Up`/`Down`
emit `TuiMsg::ConnectionFocusChanged`, which re-drives the refresh for the
newly highlighted row; without it the dialog kept the previous connection's
models on screen while `pending_update` paired them with the newly
highlighted connection, so Enter would have bound one connection's model to
another connection.

**The pane's `catalog_revision` must be the daemon's, never a local
placeholder.** `fetch_models` used to discard the `catalog_revision` the daemon
returned and mint a literal `"0"` for every row. `pending_update` forwards that
value verbatim as `expected_catalog_revision`, and `update_selection` compares
it against `catalog_revision_for(current revision)`, so the bind failed with
`selection_catalog_stale` every single time — the dialog could list a
connection's models and still never bind one. The value was manufactured
locally rather than observed as stale, so no refresh (`r`) and no daemon
restart could change the outcome. Guarded by
`fetched_models_carry_the_daemons_catalog_revision`.

Model navigation is `Tab`/`BackTab` (`move_model`), while `Up`/`Down` move the
connection highlight.
