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
| `src/tui/helix_selection.rs` | The Helix selection **state machine** over a generic `SelectionPoint`. Coordinate-agnostic on purpose: the terminal instantiates it with viewport cells, the editor would instantiate it with byte offsets. Owns the rules (`v` is a mode, `x` is an action, a movement extends only in character mode); owns no surface. |
| `src/tui/panel_resize.rs` | Outward-only panel resize: which key drags which border, and on which panel. Knows nothing about `App`. |
| `src/tui/runtime/` | Runtime loop and event routing |
| `src/tui/async_cmd.rs` | `spawn_tui_task` / `spawn_registered_tui_task` |
| `src/tui/route.rs` | `Route`/`RouteManager` primary views (Home, Session, Workspace, Editor) |
| `src/tui/commands/agent_editor.rs` | M006-F agent config editing: the agent catalog, the `:new` template, provenance rules, and the panel-action resolver. Handles both scopes — `AgentRow` carries `origin` (what it is) *and* `scope` (where its file is), because a default's override may live at either. |
| `src/tui/panels/agents.rs` | M006-F `/agents` management panel: presentation only (cursor, viewport, armed delete). Every action is a `TuiMsg::AgentPanelAction`; provenance is re-checked in the command layer so a hidden verb is still refused. Badges are `default` / `project` / `global`. |
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

For LSP navigation, preserve the result's display label separately from its
optional `workspace_relative_path`. Missing identity means no navigation
action. Pass a present candidate through the existing `DocumentOpen` request;
the daemon remains responsible for containment and `file.read` authorization.
Do not build a workspace path from a display basename or open an LSP URI.

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

### M006-C editor LSP presentation

The editor's F2–F7 actions consume existing daemon reads: F2 refreshes and
lists diagnostics, F3 hover, F4 definition, F5 references, F6 document
symbols, and F7 searches workspace symbols for the current word. Bind every
completion to `EditorLspScope` and discard results after session/project/
workspace/path/generation changes. Preserve complete diagnostics on
`NotReady` or error and visibly mark the retained set stale. Do not add
polling: the diagnostics publisher has no production driver.

The LSP panel renders untrusted payloads as bounded text. Definition and
symbol navigation requires the DTO's validated `workspace_relative_path`
and must go through `open_editor`/`DocumentOpen`. Semantic spans are
ephemeral, revision-bound, UTF-16 validated, and rejected as a whole on
malformed or overlapping ranges. Do not cache snapshot text in the LSP state.

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

## Connection dialog model pane

The model pane is scoped to one connection. `start_selection_refresh` must
resolve a focus for the fetch — an unbound session falls back to an `active`
connection, then to the first row — or the pane stays empty and the session
can never bind itself. `Up`/`Down` emit `TuiMsg::ConnectionFocusChanged`,
which re-drives `SessionSelectionRefresh { focus_connection_id }`; never let
the highlight move without refetching, because `pending_update` pairs the
rendered models with the highlighted connection and would bind the wrong
model to the wrong connection.

## Other Invariants

- **The launch directory is registered at startup.** `ProjectContextResolver`
  resolves a directory by catalog lookup only and never registers on a miss, so
  a directory CodeGG has not seen before has no executable project context.
  `App::bootstrap_launch_project_context` runs from `main.rs` before any
  session-scoped request: `WorkspaceRegister` then `ProjectRegister` for the
  launch directory (both idempotent), writing the resulting pair onto the
  active tab via `set_active_identities`. New code must not move identity
  creation into the resolver, and must not send a path in an identity field —
  `SessionCreate` takes a `(project_id, workspace_id)` **pair**, so a
  directory-shaped `project_id` with no workspace is rejected by construction.
  `ProjectExecutionContext::project_key()` exists for legacy single-field
  requests only; do not use it for `SessionCreate`. Bootstrap failure is
  non-fatal but must stay visible (stderr warning at startup, toast for the
  startup session create) — a warn-only failure leaves an empty TUI with no
  explanation. Raw-path registration through the project picker stays gated to
  `AppMode::Embedded`; the bootstrap differs because its path is the
  frontend's own cwd, not user-supplied remote text.

- **Command discovery metadata**: `CommandRegistry` is the only command
  catalog. Its domain/source/scope/keywords fields are presentation metadata;
  they do not authorize or execute commands. The palette searches those fields
  with a bounded result set and must use the active project catalog. Built-in,
  config, project, and plugin collisions are deterministic, with existing
  entries winning. `ActionKey::all()` is the exhaustive configurable action
  list; `Char` is the explicit non-configurable input exception.

- **The side column is a panel host**: `SidePanelHost` (`src/tui/panels/`)
  owns which `SidePanelId` is active, whether it holds focus, and which edge of
  the frame it lives on (`PanelPlacement`: `Right` — the default and the
  shipped layout — `Left`, or `Bottom`, a full-width row under the main input
  box). Placement is changed with `p` in the **panel manager box**
  (`Shift+S` in Normal mode, or `/panels` from anywhere) and persisted as
  `ui.panel_placement`; the strip is deliberately **display-only** rather than
  rows in the navigable list, because "which edge" and "which panel" are
  different questions and merging them renumbers every panel row. 33 panels
  ship: 5 **bespoke** (`Session`, `Agents`, `Chat`, `Terminal`, `Help`) and 28
  **hosted** — one per `/` command that brings up a box. Which one is loaded
  is chosen by the same manager, which renders `SidePanelId::ALL` directly —
  the list it shows *is* the registry. To add a **hosted** panel (the common case): an enum
  variant, an entry in `SidePanelId::ALL`, and `label`/`title`/`hint`/
  `load_command`/`config_key` arms. The host builds its `HostPanel` from
  `ALL` filtered on **`SidePanelId::is_bespoke()`**, so there is no host field,
  no `panel()` arm and no render-path branch. To add a **bespoke** panel: add
  it to `is_bespoke()`, write a `SidePanel` impl, and add arms in `panel()`,
  `panel_is_empty()`, `composer_hints()`. That list is load-bearing, not
  documentation — a bespoke panel left out of it keeps a hosted placeholder,
  and `materialize_hosted_panel` reads that placeholder as "an unloaded box"
  and calls `open_panel_box`, which calls `select_panel`, which calls it again.
  That loop killed the process on `/agents`.
  Docking is a **move**: `App::close_dialog` returns the component it
  unmounted and `SidePanelHost::adopt` installs it, so the panel inherits the
  box's filter, cursor and scroll. Selecting an empty hosted panel loads it
  in one step (`commands::panels::materialize_hosted_panel`, same seams as
  its `/` command): a sync box is opened and adopted at once, a live-loading
  box (Research, Task view) stays open with an `L`-to-dock toast, and a panel
  with nothing to show keeps a placeholder naming its `/` loader — never an
  empty bordered box, which reads as a rendering bug. `render_hosted` is a deliberate **bridge**:
  `SidePanel::render` gets a `&mut Buffer`, `Component::render` needs a
  `&mut Frame`, and ratatui's `Frame` has no public constructor, so the
  widget renders into an in-memory backend sized to the column and is copied
  out. Do not add per-panel branches to the render path. A focused panel owns
  *navigation* — arrows, `j`/`k`, paging, its own verbs — and is consulted
  **only in Normal mode**, because the host claims `Esc`/`q` as exits and
  would otherwise make `q` untypable while composing.
  `PanelKey::PassThrough` exists only for panels that hold
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
  **Ctrl+digit hotkey recall** → modals → timeline → command mode →
  completions → change review → **side panel (Normal only)** → Workspace →
  Editor → composer. Hotkeys come before the panel because a panel that
  swallowed the only key that leaves it would trap the keyboard.
- **`/files` is a navigator, not a second document surface.** The box holds a
  copy of the shared `FileTreeState` (a widget cannot reach `App` from
  `render`), is seeded from it on open so a browse resumes where the last one
  ended, and re-syncs from the generation-guarded completion. It holds no file
  text and reads nothing: `r` emits `TuiMsg::FilesRefresh` and the command
  layer owns the async walk. `Enter` on a file emits
  `TuiMsg::FilesOpenSelected`, routed to `open_editor` — the same seam `/open`
  and the Ctrl+T pane use. Only a *file* emits an open intent; a directory has
  no document. `/files` is `DocumentScope::Workspace`, so the editor's
  `[project]` badge is the honest reading.
- **`Enter` and `l` both expand a directory, and that is deliberate.** `Enter`
  reveals contents in place and leaves the cursor alone; `l` expands *and*
  steps onto the first child. `h` collapses an open directory in place and
  only steps to the parent when it is already closed — backing out of a
  directory means closing it. All three verbs live on `FileTreeState`
  (`toggle_selected` / `enter_selected` / `leave_selected`), so the box, the
  hosted panel, and the Ctrl+T pane cannot drift.
- **`:` is one registry, and it is modal.** The editor's command line opens
  from Normal mode only (in Insert a colon is document text) and owns *every*
  key while open, including the mode-independent `Ctrl+S` chords — a leaked
  printable would insert into the document behind a line the user believes is
  capturing it. `EditorCommandName::ALL` is the single list the parser, the
  fuzzy palette, and the help all read. The palette matches the **command
  name only**, never the argument, so `:open <TAB>` completes while the path
  is still being typed. `fuzzy_score` returns `0` for both an empty query and
  a total miss, so zero cannot separate them: exact prefixes are kept
  explicitly and everything else must score above the floor. `:wq now` is
  **refused**, not run as `:wq`. `:wq` sets
  `EditorState::close_after_save` and closes from the save *completion* —
  closing eagerly would discard the draft the user asked to keep.
- **The editor replaces the main buffer, so the composer arms with `:`.**
  `composer_refuses_bare_text()` is true only on `Route::Editor` with the
  buffer unfocused, and the gate sits on the single `InputAction::Char` arm
  so every typing path is covered by one rule. Scoped narrowly: a focused
  side panel has its own composer and chat is a conversation, so both stay
  freely typeable.
- **The directionals survive the editor buffer.** With the buffer focused in
  Normal mode, `Shift+H/J/K/L` still selects or docks a panel, routed through
  `panels::handle_normal_only_action` — the same entry point `App::on_key`
  uses for its five direction arms. Two dispatchers for one gesture family is
  exactly how a box and its panel drift.
- **Agent actions differ by provenance.** A **project** agent
  (`<root>/.codegg/agents/<name>.toml`) and a **global** agent
  (`~/.config/codegg/agents/<name>.toml`) can both be edited, deactivated,
  and deleted; a built-in (`build`, `explore`) can be overridden, deactivated,
  or restored, but **not** deleted — a built-in is compiled into
  `generated.rs` and has no file, so deleting would only drop an override and
  leave the agent present. Delete is armed (`d` then `d`), not immediate.
  Saving or deleting an agent config triggers the daemon `AssetRefresh`, so
  `:wq` refreshes `/agents` without a second command, at either scope.
- **Global agent editing is a second authority, not a relaxed workspace.**
  Never widen workspace containment to reach `~/.config/codegg/agents/`.
  `DocumentScope::GlobalAgent` resolves its own root and write gate in
  `src/core/global_agent_authority.rs` and accepts only a bare
  `<name>.toml`; every global write still goes through `src/lsp/mutation.rs`.
  The daemon arms are separate match blocks on purpose — they differ in
  containment, locking, *and* side effects (no LSP managed document).
- **A document's scope is part of its identity, and is always on screen.**
  `DocumentScope` is in the `DocumentKey` string, so a global `build.toml`
  and a workspace `build.toml` are two documents, never one attachment
  served from the wrong root. It also travels on the wire rather than being
  inferred, because it is the discriminator the future admin authorization
  check needs when one daemon serves many agents. `is_agent_config_path` is
  scope-qualified for the same reason: matching on the path alone would
  refresh the wrong registry after a save.
- **`t` on a default that already has an override opens it, never
  re-creates.** Re-seeding the disable template would be refused as an
  overwrite, which reads as a broken button.
- **`DocumentCreate`/`DocumentDelete` are additive and contained.**
  `DocumentOpen` requires the target to exist, so creating a file needed a new
  request. Both live in `src/lsp/mutation.rs` and borrow every check
  `checked_workspace_text_write` applies, plus: never overwrite an existing
  target, bounded parent creation (16 components, each re-canonicalized),
  `create_new(true)` so a race cannot destroy a file, and refuse to unlink a
  live attachment. Both carry `Capability::FileModify` — grading either as a
  read would let a read-only attachment create or destroy files.
- **`Shift+S` is the only *chord* of the panel manager; `/panels` is the
  other route.** `Ctrl+Alt+W` is retired: the manager is a Normal-mode
  surface exactly like the directionals that drive the same panels, so one
  key per surface is one less chord to decode in a legacy terminal. A chord
  alone, though, would make the manager unreachable in Insert — where
  `Shift+S` is the letter `S` and must stay typeable — so `/panels` (aliases
  `/panel`, `/panel-manager`) opens the same box. The key is answered ahead
  of the focused panel because a panel consumes everything by contract:
  `commands::panels::handle_panel_key` takes `OpenPanelManager` before
  delegating, and `App::on_key` answers it ahead of the modal short-circuit
  for the same reason. Keep the two spellings in lockstep — a third route is
  a third thing to keep working.
- **The manager's fixed rows are budgeted, not free.** Its chunks are
  `[Length(3) title, Length(3) filter, Min(1) list, Length(3) footer]` in a box
  roughly 44 columns wide. Adding a fifth `Length(3)` chunk looks correct at
  40 rows tall and then, at 24, the fixed rows exceed the box and the **footer
  — the only place its keys are written down — is pushed off the bottom
  entirely**. That is why the `PanelPlacement` strip shares the title block's
  inner row instead of taking a chunk. When something new has to go in the
  box, draw it inside an existing block's interior or take it from the list;
  never add a fixed chunk. Relatedly, the list scrolls
  (`PanelManagerDialog::scroll`) because 33 panels do not fit in ~10 rows and
  `j` must never walk the highlight off the bottom — the highlight is the only
  thing saying which panel `Enter` commits to.
- **A toast is its message and a border, nothing else.** The live column is
  a fixed 60×10 overlay at the top right (`App::render`), so every row a
  toast spends is a row of transcript it hides. It previously drew a
  countdown bar under the message, costing one row per toast for something
  the expiry already says — and reading, at the bottom edge of the frame,
  like a progress indicator for whatever sits beside it. Height is
  `wrapped_lines + 2`; the durable record is the `/logs` toast history, so
  nothing that matters is lost by making the live box cheap.
- **A focused panel and the composer mode are one fact, not two.**
  `App::on_key` consults a focused panel **only in Normal mode**, so focus
  and mode must agree or the panel is silently dead: the badge and the hint
  line say the panel has the keyboard while every key — including the
  gesture that leaves it — goes to the composer and is typed as text.
  `focus_panel` therefore parks the composer's mode in
  `App::composer_mode_before_panel_focus` and forces Normal;
  `blur_panel` restores it. The path that proved this was
  `dock_open_box`: `close_dialog()` parks the composer in Insert and *then*
  focuses the panel, so `Shift+H` to leave it used to type a literal `H`.
  Anything that takes focus outside `focus_panel` (`select_panel` via a
  `Ctrl+digit` recall, a route change) must go through the same seam.
- **Every dockable box answers the gesture from one place.**
  `App::handle_dialog_key` intercepts `Shift+←/→/↑/↓` *before* the mounted
  modal sees the key, gated on `Component::docks_as_viewer()`. A viewer box
  owns no text buffer, so the directional is free to mean "dock me" — and 28
  dialogs do not each re-implement the same check. `/models` and `/sessions`
  own a live filter, declare `docks_as_viewer() == false`, and answer the
  gesture themselves under the same "filter is empty" guard that already
  governs their `j`/`k`. Never add a bespoke `direction_of` check to a
  dialog that is a viewer box — the central gate already covers it, and a
  second copy is a second rule that can drift.
- **The directionals are one gesture family, and their meaning is geometry.**
  `Shift+←/→/↑/↓` and the vim `Shift+H/J/K/L` twins all resolve to
  `InputAction::Direction*` and then to one dispatcher,
  `commands::panels::move_directionally`. It asks, in order: a box is open →
  dock it to the edge the direction names; the keyboard is in the main buffer
  and the direction names an edge that is **showing** a panel → take the
  keyboard there; the keyboard is in a panel and the direction leads **away**
  from its edge → return to the main buffer; otherwise `cycle_step(dir)` walks
  the loaded panel type. Never branch on a side per surface, or a box and its
  panel drift. **The main buffer is a stop, not a gap** — it sits *between* the
  columns, so two `Shift+H` from Chat reach Terminal and one lands in the
  buffer; routing past it made the surface the user spends most of their time
  unreachable by the gesture that names it. `PanelPlacement::named_by` returns
  `None` for `Up`, so `Shift+K` names no edge and can never shadow the way out
  of a bottom panel. Normal-mode only via `is_normal_only`.
- **Edges are slots, and a panel moves between them.**
  `SidePanelHost` holds one `PanelSlot {panel, visible, history}` per
  `PanelPlacement`. Panel *state* stays keyed by `SidePanelId`, so
  `host.dock(id, edge)` moves a panel — keeping its cursor, filter and scroll —
  and never loads one panel on two edges at once. The edge being vacated
  restores its remembered state, **collapsed-ness included**, so an edge the
  user had put away comes back put away rather than popping open. If you add a
  new edge, add the slot, the `PanelPlacement` arm, and a geometry arm in
  `TuiLayout::split`; do not add a second "which panel is where" map.
- **A gesture naming an empty edge moves the panel there; it does not refuse.**
  This replaced a deliberate refusal and the difference matters. A dead end
  told the user their key was wrong when the frame had a perfectly good place
  for the panel; swapping keeps the gesture truthful. `ui_state.sidebar_visible`
  is a derived mirror of `host.any_visible()` kept only for the session
  manifest — **never branch behavior on it**, because it cannot say *which*
  edge is up and will silently answer a coarser question than the one asked.
  Uppercase letters only — `Char('l')` is a bare `l`, a within-surface verb,
  never the gesture — and each is bound with *and* without `SHIFT` because
  terminals disagree on the encoding. A box dock gesture pointing at a side
  with no panel is refused **out loud** rather than docked to the wrong one; a
  user whose box lands on the wrong side has learned the gesture lies. That
  refusal is what a second column deletes.
  `TuiLayout::split` returns `(main, panel)` whatever the side, so render
  chunks are indexed by role, never position.
  `App::input_target()` is derived from panel focus on every read — never
  latched — so leaving the panel re-arms `Build` automatically. `send_prompt`
  reads the same value; when `Chat` is armed but unbound, text is dropped with
  a warning rather than rerouted to the agent.
- **The manager opens in Normal, and opening it must not change the frame.**
  Two rules that keep reading as one bug when either is broken. (1) It used to
  force the selected edge visible, on the reasoning that a strip describing a
  collapsed column is confusing — so a panel the user had deliberately put away
  with `Ctrl+B` came back the moment they reached for `Shift+S`. A box that only
  *describes* the layout cannot be the reason a column reappears; the strip now
  names the state (`[Left collapsed]`, live edge only, because three states in a
  44-column box wrap). (2) It carries **NOR/INS** like every other dockable box:
  in Normal the highlight moves and `Shift+H/J/K/L` puts the highlighted panel on
  that edge; in Insert, `Shift+H` is the letter `H` and belongs in the filter.
  It opens in Normal because it is reached with a Normal-mode chord and is read
  before it is typed into — opening in Insert made `j`/`k` filter text, which is
  indistinguishable from a dead pane. The manager's directional is
  `TuiMsg::MovePanelTo`, deliberately **not** `MoveDirection`: the latter
  resolves the keyboard position between edges already in use, which is the wrong
  question while the manager is modal. Moving and loading stay separate acts, for
  the same reason walking the list never materializes an empty panel — a gesture
  that silently starts a daemon load is not a positioning gesture.
- **Every surface the composer can target needs a name.** The badge row reads
  `[NOR|INS] [agent] [LLM]` — one mode, one agent, **one** destination. That
  destination is `App::composer_destination_title`: the focused panel's
  `SidePanelId::title`, or `MAIN_BUFFER_TITLE` (`LLM`) when nothing is focused,
  or `CHAT` while a seed is armed. It previously rendered `[BUILD]` *and*
  `[CHAT]` unconditionally, giving `[INS] [build][BUILD] [CHAT]` — two badges
  on one axis and one permanently lit. Adding a panel means adding a `title()`
  arm; a destination the user cannot name is one they cannot trust.
- **`L` docks a box into a panel.** `Shift+L` (one of the four `Direction*`
  actions — there is no `DockPanel` action) closes the open box and activates
  the panel that can hold it, so `/chat` then `Shift+L` leaves the same browser
  running in the column. The association lives in one `match` arm in
  `App::dock_target` — a `/help` panel later was one arm plus one `SidePanel`
  impl, with no change to `commands::panels::dock_open_box`. With no box open
  the same key is a *pane move*, not a warning; both meanings live in the one
  `move_directionally` dispatcher.
- **A box and its panel may open in different modes, and the caller decides.**
  `HelpDialog::new_with_mode` takes the starting mode: `/help` passes **Insert**
  (opened to type a query), the docked Help panel passes **Normal** (read and
  scroll first; `i` reaches the filter). Never hardcode the mode inside the
  widget. In Insert `j`/`k` are filter text, so a panel hardcoded to Insert
  does not "refuse" to scroll — it types `j` into a filter nobody is watching,
  which is indistinguishable from a dead pane. The panel owns no copy of the
  rule; it hosts the dialog and passes a mode.
- **A panel refuses focus only when it has nothing at all to show.** Chat with
  no bound project leaves the keyboard in the composer. **Terminal is the
  deliberate exception**: `i` either takes the keyboard of a terminal that
  exists or starts a shell, so it always has something to do. Refusing there
  was the dead end — an empty panel takes no keys, so the `i` that would have
  fixed it never arrived. Distinguishing "re-attach a live process" from
  "replace an exited one" belongs to the command layer (`focus_terminal_panel`
  consults `InteractiveTerminalController::is_live`), never the panel.
- **A command that means "open X" should produce X.** `/terminal` with no live
  view behind it starts `default_shell_argv()` (`$SHELL`, falling back to
  `sh`/`cmd.exe` — the same answer `shell::ShellRuntime` gives `!cmd`). Making
  the user first invent an argv for `/terminal-create` and then re-run
  `/terminal` to see it was the whole reason it read as broken. An explicitly
  named handle is still honoured as typed, so `/terminal <handle>` never
  silently substitutes a shell. Where an async create needs to land in a
  *different surface* than the usual one (the side panel), carry the route as
  explicit app state — `App::terminal_panel_spawn` — and clear it on **every**
  failure path, or a failed create diverts the next unrelated terminal.
- **A diagnostic dump is not a header.** `header_lines` leads with one line —
  command, link state, whether the terminal is taking keys. The full dump
  (handle, workspace, attachment id, size, cursors) is `D` away via
  `full_header_lines`, because six lines of bug-report fields pushed the actual
  terminal rows off a short dialog and left the box reading as a handle list
  rather than a shell. Anything that changes what the visible rows *mean* — a
  resync, a degraded VT screen, an exit — stays on screen in **both** forms;
  hiding "exited, input rejected" behind a toggle is how a dead terminal reads
  as a live one. Opening the box is an async create-and-attach round trip, so
  keystrokes before it completes belong to the composer, not the terminal.
- **Viewport click and viewport drag share one press.** Inside the message
  viewport, `Down` arms a selection, `Drag` extends it, and `Up` decides:
  release on the anchor cell is a *click* and reaches `on_click` (whose
  viewport arm expands reasoning, then tool output); release elsewhere is a
  *drag* and copies. Resolving on `Down` — which the text-selection feature
  briefly did — makes every click a selection and leaves click-to-expand
  unreachable, because the viewport click arm is the only thing that expands
  a message. All other surfaces keep immediate press behaviour through the
  shared `dispatch_click`.
- **The working strip owns turn liveness, and it names the way out.**
  `render_working_strip` draws one throbber row plus one row per advertised
  subagent between the transcript and the composer, reserved from the prompt
  allocation and carved back off like the hint line and the todo strip. It
  draws while `Working` **or** while `SessionState::stopped_turn` — a stopped
  turn keeps the strip deliberately, because the user's next question is
  whether it is still stopped and how to carry on, and an indicator that
  vanishes the instant work stops answers neither. Row 0 pairs the phase with
  the key that changes it (`x to stop`, `press x again to stop agent` while a
  confirmation is armed, `■ stopped · press x or /start` once stopped): a
  long pause with no way out is what reads as a dropped connection.
  `ClickTarget::Working` is hit-tested **before** `ClickTarget::Prompt`,
  because the strip is carved out of the prompt chunk — the reverse order
  resolves every subagent click to click-to-focus. Hit rects live in
  `SubagentState::hits` and are rebuilt every frame.
- **A turn must be interruptible, and the id is what makes it so.**
  `TurnCancel` is addressed by `(session_id, turn_id)` and the daemon
  *rejects* a mismatch, so `SessionState::active_turn_id` — bound from
  `TurnStarted`, cleared by every terminal turn event — is the only thing that
  can stop a turn. A cancelled turn arrives as an ordinary `TurnCompleted`,
  so `stopped_turn` is what distinguishes "I stopped this" from "it
  finished", and it deliberately survives `TurnCompleted`. `/stop` marks it
  optimistically and undoes only on a real failure, because the daemon's
  cancel is cooperative and the lag would otherwise be visible. There is no
  protocol-level resume, so `/start` **re-submits**; never imply it revives
  the cancelled turn. `x` is one key for both directions — the state says
  which applies — and the destructive one takes a second press inside
  `STOP_ARM_WINDOW`, expired by the render pump rather than only on the next
  key. Bare-char binding, so Insert types the letter.
- **The launch directory starts its own session.** `App::ensure_launch_session`
  runs once from `main.rs` and is a no-op whenever a session is bound, because
  every session-scoped surface resolves through it and a launch without one
  reads as a broken install rather than a missing `Ctrl+N`. It reuses
  `sessions::start_new_session` so the request carries the **typed**
  `(project_id, workspace_id)` pair, and is deliberately not
  `App::new_session` (that clears the view and can open a template picker).
- **Two panel boxes, one list implementation.** `PanelManagerDialog` and
  `PanelPickerDialog` (opened by bare `/agents`) share query/cursor/scroll/
  mode/keymap through `PanelListState`; two boxes that both list panels must
  agree on what `j` does. The cursor is an index into `SidePanelId::ALL`,
  never into the filtered view — filtering narrows visibility without
  renumbering what `Enter` acts on. The picker commits the **same**
  `TuiMsg::SelectPanel` the manager emits; `App::pending_panel_picker_subject`
  is what gives the selection its second half (load the content into the
  chosen panel), and it is spent on commit so an abandoned picker cannot leak
  into the next pick. A panel row that a user selects must do something:
  `AgentsPanel` had no `Enter` arm at all and consumed the key silently.
- **Subagent rows are projection-derived and last-known-good.**
  `sync_subagents_from_snapshot` reads the same agent tree the sidebar reads.
  A snapshot that arrives replaces the rows; a *missing* snapshot does not,
  because absence is not evidence that no subagents exist and clearing on it
  blanks the strip mid-turn. Never synthesize a row the projection did not
  vouch for, and never synthesize progress text: the projection adapter
  hardcodes `progress: None`.
- **The subagent buffer is a view, not a modal.** Clicking a row replaces the
  transcript in the viewport rect; `Esc` returns. Its `Esc` arm sits below the
  modal short-circuit and above every other handler.
- **Reasoning deltas must reach `add_reasoning`.** `add_assistant_text` is a
  parser for literal `<think>` tags — a raw delta has none, so it falls
  through to `MsgPart::Text` and thinking renders as ordinary prose with no
  `Thought for` header. Both the daemon (`TurnReasoningDelta`) and standalone
  (`ReasoningDelta`) arms must call `add_reasoning`, or the two transports
  silently disagree.
- **A refused submit is not a stall — release it immediately.**
  `send_prompt` is optimistic: it stamps `Working`, appends the message, and
  clears the composer before the daemon acknowledges anything. So every path
  where acceptance fails must undo that explicitly, or the user watches an
  eternal spinner and learns nothing for five minutes. The three carriers are
  `TuiCommand::TurnSubmissionFinished { error }` (daemon refused it or the
  socket is dead), and `dispatch_turn_submit_request`'s no-core-client and
  no-session early returns. Route all of them through
  `App::fail_unstarted_turn`, which clears `working_since` (so the watchdog
  stops counting a turn that never started), sets `Error`, stops the thinking
  indicator, and **releases `pending_send` so a retry is possible**. Never
  record a submission failure in a log line and return; the log is not the UI.
- **The stall watchdog measures silence, and long tools are silent.**
  `bash` delivers its output only on completion, so a multi-minute build emits
  no events at all. `SessionState::running_tool` (set on `ToolStarted`, cleared
  on a matching `ToolCompleted`, in **both** transports) lets
  `check_turn_stall` re-arm instead of failing a healthy turn, bounded by
  `TURN_STALL_TOOL_GRACE`. Keep that grace at or above the slowest legal tool
  timeout, or the watchdog kills work that is merely slow.
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
- **One row producer for the main buffer.** `MessagesWidget::build_message_rows`
  is the only thing that turns a message into rows, and
  `messages/layout.rs` derives every scroll offset from what it emits. A
  cheaper parallel counter (`estimate_msg_lines`) drifted from the emit loops
  on two tool-output paths, and since the disagreement is per message it
  *accumulated* — long transcripts clipped off the edge and most of the history
  was unreachable. Do not add a second counter beside the builder. Rebuilds are
  suffix-scoped via `rows_dirty_from` (streaming appends to the last message),
  so a token costs one message, not the buffer. Any new output path must wrap
  to `width - TOOL_OUTPUT_INDENT`: the widget paints no `Paragraph::wrap` and
  has no horizontal scroll, so an unwrapped line is not merely clipped, it is
  unreachable.
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
- **`HelpDialog` is searchable and mode-aware.** Printable keys filter the
  keybinding rows, `Esc` clears the filter before it closes the dialog, and it
  tracks `NOR`/`INS` with a badge. Arrows/PageUp/PageDown scroll in **both**
  modes; `j`/`k` scroll **only** in Normal, because in Insert they have to stay
  typeable so `help` itself can be searched for. `NOR` exists for the
  gestures that need a non-typable key — above all the directional that docks
  the box — since in Insert `Shift+L` is the letter `L` and must stay typeable.
  The Help *panel* hosts this dialog via `render_into`; it does not
  reimplement it, so box and panel cannot drift.
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
  Finite `!`/`!!` commands go through the daemon's versioned create/read/cancel
  family. The TUI is only an adapter for output and cancellation: never spawn
  locally or send frontend cwd/environment. Insert a run into shell history
  only after daemon admission; preserve the captured session/workspace binding
  so late output cannot be promoted into another tab.
- **Interactive terminal keys** are modal input: a focused lone `Esc` is
  forwarded to the PTY, `Esc Esc` (or `Ctrl-]`) leaves focus, and viewing-mode
  Esc detaches/closes. `Esc Esc` is the reachable exit on keyboards without a
  `]` key, and any intervening key disarms the pair. Terminal
  output is interpreted by bounded VT screen state before Ratatui rendering;
  never write raw child escape sequences to the host terminal.
- **Focus must never desync between the panel host and the PTY view.**
  `SidePanelHost::blur` and `TerminalFocus` are separate flags, and
  `panel_active` requires `is_focused()` — so a focused PTY that declines
  `owns_exit_key` looks like it left while still reporting INS, and then takes
  no keys at all. A panel whose view can hold the keyboard claims the host's
  exit keys (`owns_exit_key` takes `ctx` for exactly this) and ends in the
  blur that clears its own focus. Terminal output is interpreted by bounded VT
  screen state before Ratatui rendering; never write raw child escape sequences
  to the host terminal.
- **The terminal is a reader, not a text editor.** In viewing (NOR) mode it
  speaks Helix: `j/k/h/l` and arrows move the cursor and scroll at the edges,
  `Ctrl+u`/`Ctrl+d` page, `e`/`b` jump by word, `v` selects by character, `x`
  selects the cursor's whole line and grows one line per press, `d` deletes the
  selection, `space+y` yanks to the system clipboard, and `C` clears scrollback
  while the process keeps running. In focused (INS) mode the PTY owns the
  keyboard and every key goes through `classify_key`, so a `v` or `d` typed at
  a running program stays program input. Five things make this work and are
  easy to undo by accident:
  1. The emulator holds a **bounded 5,000-row scrollback**. A zero-length
     history destroys rows as they scroll off, leaving nothing to scroll to or
     select — the reader can only address the screen, which is exactly the text
     about to disappear.
  2. **The selection rules live in `tui::helix_selection`, not on either
     surface.** `SelectionState<P>` is coordinate-agnostic on purpose: the
     terminal instantiates `P = (u16, u16)` cells, the editor would instantiate
     it with byte offsets. `v` is a **mode** (the head follows the cursor, so
     movement extends) and `x` is an **action** (the head is pinned, so a bare
     movement cannot widen it) — that split is the whole reason `x` can mean
     both "select this line" and "again, the next one". Never re-derive those
     rules per surface.
  3. Navigation state lives on the **controller**, so box and panel cannot
     disagree; selection coordinates are **viewport** offsets and shift with
     the content on every scroll.
  4. `sync_viewport` clamps the offset before every read —
     `Grid::visible_rows` subtracts without guarding, so a stale offset is a
     panic, and `clear_scrollback` is how one arises. `clear_scrollback` also
     drops the scrub overlay: it is keyed on text the clear threw away.
  5. Verbs are applied by `apply_nav`, which returns `ClearRequested` rather
     than clearing, because only the command layer can issue the daemon
     operation. `d` and `C` must both stay in `classify_nav_key` — a focused
     panel is consulted before the global bindings, so an unclaimed `d` falls
     through to `OpenDiff`. `space+y` is two events in legacy xterm encoding,
     so the prefix is remembered state (`prefix_pending`), not a chord.
  `d` deletes **locally**: `vt100::Screen` is read-only, so `Redactions`
  remembers each scrubbed range against the **row's text** rather than its
  position — a bounded scrollback has no stable row identity, and a
  position-keyed overlay drifts onto whatever scrolls into the same cells.
  Deleted text is also excluded from the yank path, or `d` would be a visual
  effect rather than a deletion. A docked terminal resizes its PTY to the real
  panel rect and drains it through `flush_terminal_resize`, because the render
  path has no `App` and the program otherwise wraps to columns the narrow
  column cannot show.
- **Panel resize is a modal frame gesture, and `Ctrl+N` is the only chord.**
  `h/j/k/l` drag the focused surface's **own** borders *outward only*: `h` is
  its left border, `l` its right, `j` its bottom, `k` its top. Only shared
  borders move, so a right-docked panel grows with `h` and not with `l`, and a
  border belongs to two surfaces — growing one shrinks the other. That makes
  `l` from the main buffer narrow the chat column while `h` from inside the
  chat column widens it, which is the whole point of doing it from both sides.
  `resolve_resize` (`tui::panel_resize`) answers "which panel changes, and
  which way", so the same key cannot mean two things depending on the subject.
  It must be resolved **ahead of the focused panel** in `App::on_key`: a
  focused panel consumes every key it is shown, so `h/j/k/l` would otherwise
  move a terminal cursor instead of a border — and it must also work with the
  keyboard in the main buffer, which is the other half of the gesture. A resize
  key naming a frame edge is consumed and ignored, never passed through. Leave
  with `Esc`, `Enter`, or `Ctrl+N` again; stay armed across keys, because the
  gesture is "drag several borders in one visit". Normal-mode only, like
  `Shift+S` — a frame mode that could be armed mid-sentence would swallow the
  next four letters. `Ctrl+N` is deliberately **not** an `InputAction`: the
  frame is not a composable action, and a configurable binding would let it be
  pinned to a key a panel already owns. New session is the bare `n`. Column
  widths are per edge (`LayoutConfig::{left,right}_panel_width`, falling back
  to `sidebar_width`), and the bottom panel records an exact `bottom_panel_rows`
  because a percentage cannot express a one-row step — `observe_frame` records
  the last rendered height purely so that conversion has a denominator.
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
