---
name: tui
description: Operational guide for changing the terminal UI safely - command registration, dispatch boundaries, async spawn-and-complete pattern, dialogs, and background task lifecycle
version: 1.0.0
tags:
  - tui
  - commands
  - async
  - ratatui
---

# TUI Module Guide

Operational guide for making changes to `src/tui/`. The full module contract
lives in `architecture/tui.md`; this skill covers the patterns you must follow
to avoid breaking invariants that are easy to violate across the TUI composition
root and lifecycle modules.

## Layout

| Path | Purpose |
|------|---------|
| `src/tui/app/mod.rs` | `App` composition root, construction, facade, shutdown |
| `src/tui/app/types.rs` | `Dialog`, `TuiMsg`, and stable app-facing types |
| `src/tui/app/commands.rs` | `TuiCommand` effect requests/completions |
| `src/tui/app/render.rs` | Cached render composition; no I/O or async work |
| `src/tui/app/input.rs` | Synchronous `TuiMsg` processing |
| `src/tui/app/project_session.rs` | Active project/session and projection lifecycle |
| `src/tui/app/prompt_turn.rs` | Prompt submission and route-safe turn start |
| `src/tui/app/modal.rs` | FocusManager-backed modal lifecycle |
| `src/tui/app/plugin_ui.rs` | Plugin UI validation and effect application |
| `src/tui/app/state/` | App state helpers; `execution_context.rs` resolves explicit project scope, `async_request.rs` holds the finish/fail guard, `routing.rs` holds the `UiRouteToken` project/tab scope token for prompt/session continuations, `work_orders.rs` owns Task composer/sheet/view pure state (mode, validation, grouping, reorder math), and `workspace_dashboard.rs` owns global dashboard pure state (filter/nav/generation/dirty/revocation) |
| `src/tui/commands/work_orders.rs` | Project Task flows: composer toggle (bare Tab owns Session/Task, `Ctrl+A` owns SwitchAgent, `Ctrl+G` alias), sheet prefetch/confirm with focused queue insertion (`j/k`/Up/Down) and capability-aware external-trigger gate, `WorkOrderCreate` then chained `WorkOrderTriggerCreate` with one-time secret dialog, Task view refresh/reorder/mutate/open plus trigger setup/rotate/revoke/refresh (`t`/`T`/`X`/`e`), `/tasks` migration — all spawn-and-complete with route+generation stale guards; bearer cleared on close/switch/reconnect |
| `src/tui/commands/workspace_dashboard.rs` | Workspace primary-view flows (`Route::Workspace`, non-modal): open/refresh (one `WorkspaceDashboard` aggregate), `Space` expand (one bounded `WorkOrderList`), empty-`Enter` descend via project tabs + Task view, `/workspace` + `/chat` side-panel focus — generation+epoch stale guards, hint-dirty without focus theft; composer/chat routing via selected-project locator with no cwd/hidden-session fallback. Cancellation ownership: dashboard refresh/expand spawn as `TuiTaskKind::Workspace`; `leave_workspace_view` cancels only that kind (never `Command`), and generation/request fencing stays the correctness boundary for races |
| `src/tui/command.rs` | Slash-command registry, scoped catalog, and discovery metadata |
| `src/tui/commands/` | command-handler submodules (see `mod.rs` for the current set) |
| `src/tui/runtime/command_dispatch.rs` | `dispatch_tui_command(app, cmd)` - maps `TuiCommand` variants to handlers |
| `src/tui/runtime/` | Runtime loop and event routing |
| `src/tui/async_cmd.rs` | `spawn_tui_task` / `spawn_registered_tui_task` |
| `src/tui/route.rs` | `Route`/`RouteManager` primary views (Home, Session, Workspace, Editor) |
| `src/tui/ui_builders/` | Typed UI builders (`shell.rs`, `plugins.rs`, `stats.rs`) for complex panels |
| `src/tui/task_lifecycle.rs` | `TuiTaskRegistry` - tracks spawned background tasks on `App` |
| `src/tui/components/` | Widgets; `component.rs` has `DialogType`, `focus.rs` has `FocusManager` |

### Intent/effect direction

- `TuiMsg` is synchronous component/user intent. `App::process_msg` applies
  immediate UI state changes or enqueues an explicit runtime effect.
- `TuiCommand` is the runtime-channel boundary. Request variants start work
  through existing domain handlers; completion variants carry typed results
  back for synchronous `App` mutation.
- Do not add daemon completions to `TuiMsg`, or use `process_msg` as a
  completion router. Keep one top-level `runtime/command_dispatch.rs` entry
  point.
- Render modules may only read cached state and prepare widgets. They must not
  perform filesystem, network, daemon, process, or blocking work.

## Adding a New Command

1. Add the variant to the command list in `CommandRegistry::built_in_commands()`
   (`src/tui/command.rs`). Keep its description and discovery domain searchable;
   the exact built-in count is asserted by the registry tests.
2. Add a `TuiCommand` variant if the command needs backend work.
3. Handle the variant in `src/tui/runtime/command_dispatch.rs`.

## Dispatch Rules

- **`dispatch_tui_command` is `pub(crate) async fn`**
  (`src/tui/runtime/command_dispatch.rs:84`). The match is overwhelmingly
  synchronous, but it contains five `.await` points, all of the same shape:
  `match core_client.request(req).await` inside the durable-editor arms
  (`EditUndoLatest`, `EditReapplyLatest`, `EditUndo`, `EditReapply`,
  `EditCheckpointList`, at lines 2109/2160/2214/2268/2320). Those arms await
  an in-process `CoreClient` round trip and return a completion `TuiCommand`.
  Every other arm is non-async.
- High-latency work uses the **spawn-and-complete** pattern instead:
  1. Spawn with `spawn_registered_tui_task(tx, registry, kind, name, fut)`
     (registers with `TuiTaskRegistry` for lifecycle tracking).
  2. On completion, send a completion `TuiCommand` back through the channel.
  3. The completion handler MUST use the stale-completion guard:
     `state.finish(request_id)` / `state.fail(request_id, err)` from
     `src/tui/app/state/async_request.rs`. These return `bool`; a `false`
     return means the completion is stale (superseded request) and must be
     dropped.
  4. Every new apply handler needs a stale-completion test (duplicate or
     out-of-order completions must not corrupt state).

## Dialogs

- `Dialog::Info` does NOT exist even though `components/dialogs/info.rs` does.
  Use `App::show_short_or_info(info_type, lines)`: toasts when <= 3 lines,
  otherwise opens a scrollable `InfoDialog`.
- `Dialog::Plugin` is generic: one variant handles every plugin dialog.
- `DialogType` lives in `src/tui/components/component.rs`, not `types.rs`.
- Focus management goes through `FocusManager` (`components/component/focus.rs`).
  Only the top mounted modal receives key input; unhandled keys are dropped,
  never bubbled to lower modals or the prompt.
- `FocusManager` owns mounted live modal components. Use its narrow typed
  `with_dialog`, `with_dialog_mut`, or `with_component` helpers for updates;
  do not clone a component into `DialogState` and replace the mounted copy.
  `ui_state.dialog` is a derived compatibility discriminator, while
  `active_dialog_type()` controls modal lifecycle.

## Other Invariants

- **Command discovery metadata**: `CommandRegistry` is the only command
  catalog. Its domain/source/scope/keywords fields are presentation metadata;
  they do not authorize or execute commands. The palette searches those fields
  with a bounded result set and must use the active project catalog. Built-in,
  config, project, and plugin collisions are deterministic, with existing
  entries winning. `ActionKey::all()` is the exhaustive configurable action
  list; `Char` is the explicit non-configurable input exception.

- **Sidebar focus is projection-only**: `SidebarFocusTarget` identities are
  shared by keyboard navigation and mouse hit testing. Selection is cleared on
  project-scope changes, and the bounded `agent_tree` is joined to durable
  runs only by exact canonical task IDs. Enter opens the existing lazy run
  detail surface; it never copies transcripts or logs into sidebar state.
- **Git sidebar is cached**: `GitSidebarState` caches git info; stale
  generations are dropped silently. Do not render git state live per frame.
- **Remote protocol is event/state-driven**: the `/tui` WebSocket speaks the
  `TuiMessage` wire enum (`crates/codegg-protocol/src/tui.rs`) with
  sequence-tagged `EventEnvelope` replay. `TuiCommand` is the in-process
  runtime channel only - it is not `Serialize` and never crosses the socket.
  There is no `RenderFrame` support - do not add pixel/frame-style remote
  messages.
- **Human shell cells** render via `MsgPart::ShellCell`; `/shell-*` commands
  live in `commands/shell.rs` (see the `human-shell` skill).
- **Project scope**: resolve `App::project_execution_context()` before
  spawning project-scoped work. The active tab supplies project/workspace/
  session identities and the workspace root. Never use process cwd or the
  legacy `session_state.project_dir` mirror as project authority; do not
  change process cwd to switch tabs. While `Route::Workspace` is active,
  resolve `workspace_dashboard::composer_execution_context()` (selected
  project through an open tab, fail visibly with no fallback) and never
  send to the hidden prior session when the selection points elsewhere.
- **Prompt/session creation**: `App::send_prompt` captures immutable prompt
  text and a `ProjectExecutionContext`/`UiRouteToken` when no session exists.
  The event loop starts the registered `PromptSessionCreated` continuation;
  it must not await `CoreClient::request`. Completion validates request,
  tab/project/workspace/view/reconnect scope before calling `set_session` and
  submitting the captured text exactly once. Failures restore the prompt and
  explicit retry must not duplicate the already-visible user message. Tab
  switch/close, reconnect, and shutdown invalidate the continuation.

## Static Guards

Both guards fail `verify.sh quick` and the CI `verify` job. Read them before
adding TUI code - they are the executable form of the invariants above.

| Script | Enforces |
|---|---|
| `python3 scripts/check_tui_project_authority.py` | Scans `tui/app/state/**`, `tui/app/mod.rs`, `tui/commands/**`, `tui/runtime/**`, `tui/command.rs`, and `tui/components/dialogs/command.rs`; rejects `session_state.project_dir` and `std::env::current_dir()` as project authority. Only clearly marked bootstrap boundaries and test fixtures are exempt. |
| `python3 scripts/check_tui_editor_text_authority.py` | In the editor path, rejects a second text buffer (struct fields named `text`/`content`/`lines`/`body`/`buffer`/`rope`/`source`/`document`, owned `DocumentSnapshot`/`DocumentBuffer`/`Rope`, materialized `Vec<String>`/`Vec<Line>`), any filesystem read API, and any `DocumentBuffer`/`Rope` construction. `&DocumentSnapshot` borrows and `TextTransaction` undo history are deliberately allowed (ADR-0011). |

## Testing

```bash
cargo test --test tui_render        # rendering integration tests
cargo test --test tui               # behavior tests
```

Rendering tests assert on buffer contents; keep widget output deterministic
(no wall-clock times without injection seams).

## Source verification

Re-verified 2026-10-06 against `architecture/tui.md` and source. Re-confirmed
accurate: `dispatch_tui_command` is `pub(crate) async fn` at
`runtime/command_dispatch.rs:84` with exactly five `.await` points at
2109/2160/2214/2268/2320, all `match core_client.request(req).await` inside
the `EditUndoLatest` / `EditReapplyLatest` / `EditUndo` / `EditReapply` /
`EditCheckpointList` arms (verified by grep, not by reading the match);
`spawn_tui_task` / `spawn_registered_tui_task` in `src/tui/async_cmd.rs`;
`AsyncUiRequestState::finish`/`fail` returning `bool` in
`app/state/async_request.rs`; `UiRouteToken` at `app/state/routing.rs:66`
(`route.rs` holds `Route`/`RouteManager`); `DialogType` at
`components/component.rs:22`; `FocusManager` at `components/component/focus.rs`;
`Dialog::Info` absent while `components/dialogs/info.rs` exists;
`show_short_or_info` toasting at <= 3 lines (`MAX_TOAST_LINES`);
`ActionKey::all()` as the exhaustive configurable list with `InputAction::Char`
the non-configurable exception; `MsgPart::ShellCell` in
`components/messages.rs`; `/shell-*` registered in `command.rs` and handled in
`commands/shell.rs`; both `--test tui_render` and `--test tui` targets.

Corrected this pass: the remote-protocol bullet claimed the `/tui`
WebSocket "speaks the `TuiCommand` enum". `TuiCommand` derives only
`Debug, Clone` (`app/commands.rs:14`) and appears nowhere in `src/server/` or
`crates/codegg-protocol/`; the wire type is
`codegg_protocol::tui::TuiMessage` (`#[serde(tag = "type")]`,
`crates/codegg-protocol/src/tui.rs:16-19`), which `src/server/ws.rs:23`
imports and serializes. Replaced with the wire-enum name.

Added: a `## Static Guards` section naming
`scripts/check_tui_project_authority.py` and
`scripts/check_tui_editor_text_authority.py` with the surfaces and patterns
each one actually rejects (both scripts confirmed present and wired into
`verify.sh quick`).
