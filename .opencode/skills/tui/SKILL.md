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

- **The side column is a panel host**: `SidePanelHost` (`src/tui/panels/`)
  owns which `SidePanelId` is active, whether it holds focus, and the
  `Ctrl+Alt+W` selector. To add a panel: an enum variant, an entry in
  `SidePanelId::ALL`, and one `SidePanel` impl. Do not add a per-panel `if`
  to the render path. A focused panel owns *navigation* — arrows, `j`/`k`,
  paging, its own verbs — and is consulted **only in Normal mode**, because
  the host claims `Esc`/`q` as exits and would otherwise make `q` untypable
  while composing. `PanelKey::PassThrough` exists only for panels that hold
  no draft (the chat panel passes `i`/`a` to the composer); a panel with a
  live buffer must still consume everything. A verb that carries text to
  prefill cannot pass through — the chat panel's `r`/`e` emit
  `TuiMsg::ChatComposerSeed` instead, so the reply target or replaced body
  is not stranded. Panels get named borrows via `PanelCtx` (never `&App`)
  and express every mutation as a `TuiMsg` routed into an existing command
  seam. A panel declares its own keys through `SidePanel::composer_hints`,
  which render on one contextual line directly above the composer — that is
  the seam for adding the next panel, so do not grow per-panel footers.
  Project binding is resolved by `commands/panels.rs`, not by the panel.
  Key precedence in `App::on_key`:
  modals → timeline → command mode → completions → change review → **side
  panel (Normal only)** → Workspace → Editor → composer.
- **Focus moves and the input target are Normal-mode, Shift-qualified, and
  derived.** `Shift+→` focuses the side panel, `Shift+←`/`Shift+H` return to
  the main buffer; bare arrows keep their existing meaning. They are gated by
  `is_normal_only` because the binding map is shared across modes. `H` is bound
  with *and* without `SHIFT` because terminals disagree on the encoding;
  `FocusSidePanel` needs no letter twin because arrows encode reliably.
  `App::input_target()` is derived from panel focus on every read — never
  latched — so leaving the panel re-arms `Build` automatically. `send_prompt`
  reads the same value; when `Chat` is armed but unbound, text is dropped with
  a warning rather than rerouted to the agent.
- **Every surface the composer can target needs a name.** The badge row reads
  `[NOR|INS] [agent] [LLM]` — one mode, one agent, **one** destination. That
  destination is `App::composer_destination_title`: the focused panel's
  `SidePanelId::title`, or `MAIN_BUFFER_TITLE` (`LLM`) when nothing is focused,
  or `CHAT` while a seed is armed. It previously rendered `[BUILD]` *and*
  `[CHAT]` unconditionally, giving `[INS] [build][BUILD] [CHAT]` — two badges
  on one axis and one permanently lit. Adding a panel means adding a `title()`
  arm; a destination the user cannot name is one they cannot trust.
- **`L` docks a box into a panel; it does not move focus.** `L`/`Shift+L`
  (`InputAction::DockPanel`) closes the open box and activates the panel that
  can hold it, so `/chat` then `L` leaves the same browser running in the
  column. The association lives in one `match` arm in `App::dock_target` — a
  `/help` panel later is one arm plus one `SidePanel` impl, with no change to
  `commands::panels::dock_open_box`. With no box open it warns instead of
  silently navigating.
- **Never put arithmetic in `bool::then_some`.** `then_some` takes its value
  eagerly, so `cond.then_some(a - b)` evaluates `a - b` even when `cond` is
  false — and `Duration`'s `Sub` panics on underflow. This crashed the TUI on
  the first message of a new session, in the render throttle's
  `render_delay_after`, because `elapsed` is read twice per loop iteration and
  the work between the reads makes the second one larger. Use `then(|| ..)`
  and `saturating_sub`. Note the render's `catch_unwind` covers only
  `render_app`: the throttle maths is outside it, so a panic there kills the
  process instead of degrading.
- **An empty authoritative load must not delete local rows.**
  `apply_session_messages_loaded` clears only when the load carries rows or
  there is nothing local to lose. A brand-new session returns zero rows until
  its first turn is persisted, and an unconditional `clear()` erased the
  prompt the user had just sent.
- **The chat viewport centres the selection.** `chat_browser::window` used to
  end on the selected message's last row, so scrolling up filled the screen
  with blank rows *below* the selection and made every later message
  unreachable. Centre, then clamp — tail-follow still lands on the newest.
- **`/chat` and the chat panel are one implementation, not two.**
  `src/tui/chat_browser.rs` owns the cursor, the keymap, row rendering, the
  viewport anchor, and the hints; each surface owns only its composer. They
  were peers with a copied keymap and drifted until the panel could neither
  reply nor edit — fix a verb in the browser, never in one surface. The
  browser holds no text: a verb that needs typing returns
  `ChatKeyAction::Compose` and the *host* materializes the editor (the
  dialog's own composer, or `PromptState::chat_seed` for the shared one).
- **The chat panel holds no composer buffer** — one bottom composer, two
  destinations. Its destination row names the armed target and draws a `▸`
  marker only while it holds the keyboard; when unbound it renders
  `No project bound` instead of an armed-looking row.
- **An armed chat seed outranks panel focus.** Pressing `r` seeds the
  composer with `@author ` and a reply target; letting a later focus move
  redirect that into an agent prompt would be the worst possible outcome, so
  `App::input_target()` returns `Chat` while `PromptState::chat_seed` is set,
  the badge shows `[REPLY]`/`[EDIT]`, and Normal-mode `Esc` cancels the
  context without discarding the typed text.
- **Selection uses `bg`, not `fg`.** `theme.selection` is a background; using
  it as a foreground painted selected message authors dark-on-dark. Pair it
  with `fg(theme.foreground)`. Unselected bodies are `foreground`, not
  `muted` — dimming the whole transcript whenever nothing was unread made
  the common case the unreadable one.
- **Chat surfaces open on the newest message and follow the tail.**
  `ChatBrowser::follow_tail` starts true; `k` unpins, `j` past the newest and
  `G` re-pin. `sync` runs on every render and snapshot, not only on
  navigation. A cursor starting at index 0 made a busy channel look empty.
- **`@` is not the same trigger in both destinations.** `@` opens the
  agent/file completion popup for the agent session and Enter accepts it;
  against chat it is a mention, and `update_completions` skips that branch
  when `input_target() == Chat`. Leaving it enabled made Enter insert an
  agent name instead of sending — and `r` seeds `@author `, so it broke
  every reply.
- **Multi-modifier chords are `{CONTROL, ALT}` only.** The TUI never pushes
  keyboard-enhancement flags (`tui/terminal.rs` does raw mode, bracketed
  paste, mouse capture only), so the terminal is in legacy xterm encoding where
  `Ctrl+Shift+X` and `Ctrl+X` are the same byte. A `(CONTROL|SHIFT, …)` binding
  is unreachable and the key silently fires whatever owns the bare-Ctrl twin —
  that is how `Ctrl+Shift+W` ran `CloseSession` instead of opening the panel
  switcher. `Ctrl+Alt+<letter>` arrives as `ESC` + one control byte and is
  unambiguous. `tests/modifier_chords.rs` asserts no map holds a
  CONTROL|SHIFT entry and that each chord stays disjoint from its twins.
- **An input surface must make its mode observable, and must draw the whole
  buffer.** The `/chat` window reserves its composer row in both modes with a
  `NOR`/`INS` badge and `press i to type` in Normal mode. It also renders
  `before + caret + after`: it used to draw only the slice after the caret and
  nothing at all when empty, so text typed left of the cursor never appeared.
  Collapsing the row in Normal mode made the modes pixel-identical on an empty
  buffer, so a user pressing `i` and typing watched characters vanish into
  navigation and concluded the input box was broken. The chat panel has no
  manual refresh key — it renders `ChatState` live and daemon chat events push
  into the reducer.
- **`HelpDialog` is searchable.** Printable keys filter the keybinding rows,
  `Esc` clears the filter before it closes the dialog, and scrolling is on
  arrows/PageUp/PageDown rather than `j`/`k` so letters stay typeable.
- **Sidebar focus is projection-only**: `SidebarFocusTarget` identities are
  shared by keyboard navigation and mouse hit testing. Selection is cleared on
  project-scope changes, and the bounded `agent_tree` is joined to durable
  runs only by exact canonical task IDs. Enter opens the existing lazy run
  detail surface; it never copies transcripts or logs into sidebar state.
  The Session panel (`panels/session.rs`) delegates to `SidebarWidget`
  unchanged, so its binding-resolved behavior is whatever `handle_key` says.
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
- **Interactive terminal keys** are modal input: focused Esc is forwarded to
  the PTY, Ctrl-] leaves focus, and viewing-mode Esc detaches/closes. Terminal
  output is interpreted by bounded VT screen state before Ratatui rendering;
  never write raw child escape sequences to the host terminal.
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

Re-verified 2026-10-10 for C003: focused Esc is forwarded to the PTY, Ctrl-]
leaves focus, and the screen renderer uses bounded VT state with resize and
degraded-history handling. Terminal output remains outside model context.
