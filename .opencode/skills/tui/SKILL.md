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
| `src/tui/commands/primary_projection.rs` | Default `/core` session projection subscription, snapshot installation, bounded pre-install events, ack/resync, and teardown |
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

### M004 `/queue` box

`components/dialogs/queue.rs`, mounted by `commands::queue::open_queue_box`.
Rules that are easy to break and hard to notice:

- **The box renders, it does not decide.** It must never sort, filter, or
  reorder `SessionState::user_prompt_queue`; the daemon owns the ordinal.
  Selection follows `entry_id` across a fresh projection, not the index —
  index-keyed selection slides the cursor onto a different prompt when a row
  disappears, and the `d` after that deletes something the user never selected.
- **Advertise only keys `handle_key` implements.** M004 owns `j/k`, `r`, `d`;
  M005 adds `a`, `e`, `[`/`]`. Naming one that does not exist in the hint *or*
  in `EMPTY_STATE` teaches a dead key. `Esc` is handled but deliberately not
  hinted, like `TreeDialog`, because the modal host claims `Esc` first.
- **A `delivering` row is listed and offered to `d`.** The daemon refuses it
  with `user_prompt_queue_entry_delivering` and the box reports it as *being
  delivered*, never as *expired* — a delivering entry has not expired.
- **A mutation applies its own response's projection.** `queue_projection`
  returns the post-write projection in the write's response; `commands::queue`
  must never follow it with a `UserPromptQueueList`.
- **Do not add key arms to `handle_dialog_key_legacy`** for a new `Dialog`
  variant. It is `#[cfg(any())]`-disabled and `FocusManager` is the sole key
  authority, so such an arm compiles and does nothing.
- `close_dialog`'s `Dialog::Queue` arm cancels
  `DialogState::queue_delete_request` (and M005's add/edit/reorder states) so a
  late completion mutates nothing. It must **not** call
  `requeue_open_permission` — the box has a cursor, not a pending human
  decision.
- `/queue` is blocked for observers by **omission** in the fail-closed
  `is_observer_allowed_command` allowlist. Do not add it, and do not add the
  M005 actions either.

### M005 `/queue` authoring

Same file; the box is now also a text field. Three rules carry the milestone:

- **Guard the text step structurally, not per letter.** `handle_key` matches
  `is_text_step()` **first, as a separate match**, and returns from it, so the
  browsing arm is unreachable while a composer is open. A per-arm
  `if !self.is_text_step()` is the shape that eventually ships a sixth action
  with a missing guard — and typing `d` into a prompt then deletes a prompt.
  Copy `ConnectDialog::is_text_input_step`'s shape; do **not** open a shared
  `App`-level `captures_text` predicate.
- **Reorder's permutation is built from `pending` rows only.** The projection
  carries `pending` *and* `delivering`, but `reorder` checks the permutation
  against `status = 'pending'` with exact length equality, so naming a
  delivering row earns `user_prompt_queue_order_mismatch`.
- **`order_mismatch` re-reads once and reports — never a retry loop.** The
  check is a length-equality set compare, so blind retrying against a stale
  projection is a contention source.

Two deliberate asymmetries, both load-bearing:

- M004 lets the daemon refuse a `d` on a `delivering` row. Reorder **cannot**:
  the permutation would omit that row, succeed, and reorder nothing. So
  `reorder_selected` refuses locally and names which end it is at.
- The `unowned` refusal says the repair is a separate explicit operation
  outside this box, and does **not** name a slash command — none is registered.
  Naming one would be this milestone's own version of the dead-key lie.

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
  resolves a directory by catalog lookup only and never registers on a miss.
  Startup and the local directory picker share
  `ensure_bound_project_context`: they register the workspace then the
  directory-named project, and both operations are idempotent for a known
  canonical root. New roots receive one project each even when their Git
  lineage matches another project; historical multi-root bindings stay
  readable until explicitly rebound. The launch pair is written onto the
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
  different questions and merging them renumbers every panel row. 32 panels
  ship: 5 **bespoke** (`Session`, `Agents`, `Chat`, `Terminal`, `Help`) and 27
  **hosted** — one per `/` command that brings up a box. Which one is loaded
  is chosen by the same manager, which renders `SidePanelId::ALL` directly —
  the list it shows *is* the registry. To add a **hosted** panel (the common case): an enum
  variant, an entry in `SidePanelId::ALL`, and `label`/`title`/`hint`/
  `config_key` arms. The host builds its `HostPanel` from
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
  its `/` command), and **`open_panel_box` returns a `PanelFill`** saying which
  of three things it did: `Filled` (adopt it now), `Loading` (the box arrives
  on its own — the panel shows progress), or `Cannot(reason)` (the panel states
  why). Two more variants exist for a box that mounted but is still loading
  (`StillLoading` — adopt it and you freeze its spinner) and for one that
  belongs to another panel (`OpenedAs`). **Never re-read `dock_target()` to
  infer this** — that inference is what the enum replaced, and the mid-load
  special case used to be a `matches!` list of panel ids at the call site.
  A panel with nothing to show keeps a placeholder describing its **state** —
  never an empty bordered box (a rendering bug), and never an instruction to
  run a `/` command or press `Shift+←`, because the panel is already in place.
  `render_hosted` is a deliberate **bridge**:
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
  which render on one contextual line directly above the composer — that is
  the seam for adding the next panel, so do not grow per-panel footers.
  A panel declares its verbs through `SidePanel::composer_hints`. Read
  `architecture/tui.md`'s "A docked panel advertises itself before you go in"
  before changing that seam; two things about it are easy to get backwards:

  - **The unfocused answer is not one answer.** A *hosted* panel (an adopted
    box like `/files`) advertises without the keyboard, because its verbs exist
    nowhere else and it leads with the gesture that makes them live. A
    *bespoke* panel (session, chat, agents, help, **terminal**) answers `None`,
    because the row is meant to describe whoever holds the keyboard and the
    reader can already see the panel. The Terminal panel used to get this
    wrong: it kept an unfocused branch whose every verb — including the
    `Shift+←/→` it advertised — required the keyboard the composer had just
    taken, so the line sat above the LLM buffer describing a terminal nobody
    was in.
  - **The row is a carved row, and the input box does not move with it.**
    `session_layout` gives the prompt chunk a fixed, bottom-anchored height, and
    the working strip, hint line and todo strip come off its **top** in that
    order. So the composer stays on its row whatever the carved region does —
    the opposite of what the arithmetic looks like it should do. Pin it, because
    a hint row that reflowed the input every time a panel took or lost focus
    would be the same defect the working strip's reserved row 0 exists to
    prevent. Nothing to do with that strip, which is reserved unconditionally.
    **The row is not vacated when a panel lets go**: the composer takes it over,
    because the composer's own verbs are what the row is for (M007). The row's
    *content* changes with the keyboard; its *presence* does not, so the working
    strip above it does not move either.
  A **hosted** box gets its verbs from the box itself via
  `Component::keymap_hint()`, so the string sits next to the `handle_key`
  arms that implement it; the host's generic `j/k scroll · i act` is only
  the fallback for widgets that answer `None`, and it named `i`, which no
  adopted box dispatches. **Every dockable box now answers it** (14
  overrides, counted by grepping `src/` for `fn keymap_hint` and excluding
  the trait declaration), and every one also overrides `render_in_panel`
  (15, including `Placeholder`), so `InfoDialog` is no longer the template
  — **`FilesDialog` and `InfoDialog` are the two worked examples**, and
  a third should copy one of them rather than invent an idiom.
  Three rules the twelve adapters established, each of which a new one
  must keep:
  - **A hint names only keys a *docked* box can receive.** The host
    answers, before the widget is consulted, `Esc`, `q`, the four
    arrows, and bare **and** shifted uppercase `H`/`J`/`K`/`L`
    (`insert_panel_direction_bindings`). So spell scrolling `j/k`, never
    `↑/↓`; never advertise `Enter` as a close verb, because docking pops
    the box off the modal stack and `CloseDialog` runs against an empty
    one. `Home`/`End`/`PageUp`/`PageDown` *do* arrive.
  - **One body routine, parameterised by chrome.** `enum
    Chrome { Owned, Hosted }` and a single private routine that `render`
    and `render_in_panel` both call. Two viewport computations is how a
    docked box ends up with an unreachable tail and no error — the
    `ResearchBrowserDialog` report view shipped exactly that, and its
    `Home` key rendered the report blank because nothing clamped the
    offset. Clamp in the shared routine, against the rect actually being
    filled, or do not have a sentinel-keyed scroll at all.
  - **Retire in-panel footers and border-title keymaps into the hint**,
    but check whether the footer is also an *affordance*. `plugin`
    keeps its `12-30/30` scroll indicator (that is content, and the only
    end-of-content signal); `panel_picker` keeps its whole footer,
    because it declares `docks_as_viewer() == true` but has no
    `App::dock_target` arm, so it never docks and the footer is all a
    user ever sees. Do not replay a caller-supplied `set_custom_footer`
    string into the hint: those describe the modal box, where `Esc`
    really does close.
  A docked panel advertises itself **unfocused**
  too, leading with the gesture that enters it (`PanelPlacement::toward`),
  because the alternative is a keymap readable only after you have already
  guessed the keys. Bespoke panels (session, chat, help, agents) decline
  unfocused — a pre-docked column is on screen every session — and an empty
  panel is silent because `focus` refuses it. The host owns the border and
  the `●` focus dot, so a widget with chrome of its own overrides
  `Component::render_in_panel` rather than `render`; `FilesDialog` and
  `InfoDialog` are the two worked examples, and the rule for both is one
  private body routine parameterised by "draw chrome", because the hosted and
  modal paths must reach the same end of the same scroll. `InfoDialog` is the
  one to copy when the box has scroll state: its `Down`/`j` arm is
  deliberately unbounded and `scroll_to_end` stores a `usize::MAX` sentinel
  because a wrapped line occupies several rows, so both hand the clamp to the
  render pass and two clamps reach a different last row with no error to show.
  A `keymap_hint` is a claim about the **hosted** verb set: `Esc` never reaches
  a hosted widget (the host claims `Esc`/`q` as its own exit and blurs first)
  and `Enter`'s `CloseDialog` runs against an empty modal stack, because
  docking popped the box off it. Do not name either, and do not replay a
  `set_custom_footer` string into the hint — those describe the modal box.
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
- **`Enter` expands, `l` re-roots, `h` goes up a level.** `Enter` reveals a
  directory's contents in place and leaves the cursor alone. `l` makes the
  selected directory the listing's **root** — a daemon round-trip scoped by
  `WorkspaceDirectoryListRequestDto::prefix`, not a local filter of the
  already-loaded rows, which would drop everything the first walk hit its
  bounds on and present the remainder as if it were the directory. `h` pops
  `FileTreeState::root_history` and lands the cursor on the directory just
  left behind; at the workspace root, where there is no level to leave, it
  falls back to `leave_selected` (collapse in place, else step to the parent)
  rather than making the key inert at the one depth every listing starts at.
  All of it lives on `FileTreeState`, so the box, the hosted panel, and the
  Ctrl+T pane cannot drift; re-rooting and ascending are **intents**
  (`TuiMsg::FilesDescend` / `FilesAscend`) because the box holds a copy.
  `root_history`'s last entry *is* the current root — one axis, not a prefix
  field and an expansion set that can disagree — and it is re-seeded with the
  workspace root before the first push, or the very first `l` becomes one-way.
- **Depth is display arithmetic, and only one function owns it.** A subtree
  walk counts depth from the subtree, so a listing scoped with `l` arrives at
  the same depth the workspace root's top level does — and rendering `row.depth`
  alone therefore drew the two identically. `FileTreeState::display_depth`
  adds `scope_depth()` back and clamps at `MAX_INDENT_DEPTH`; `indent_for`
  pairs that with last-ness for `└─` over `├─`; `file_icons::tree_indent`
  renders both. The box and the Ctrl+T pane call that last one rather than each
  carrying their own `"  ".repeat(...)` and their own cap. The guide is BMP box
  drawing and deliberately **not** behind `CODEGG_FILE_ICONS` — hierarchy must
  not depend on a patched font — and it is elided before the name, never after.
  The box title is the same fact in words: a `workspace › src › tui`
  breadcrumb. `display_depth` reads state and never writes it; if it starts
  changing expansion, selection or what a walk returns, it has become
  navigation and stopped being a display concern.
- **`Ctrl+d`/`Ctrl+u` page a listing that does not fit.** They carry
  `Char('d')`/`Char('u')` codes, so the tree checks modifiers *before* its
  character arms. A page is two thirds of `FileTreeState::viewport_rows`,
  which the renderer publishes from the widget's returned `PaneLayout` —
  a widget borrows the state it draws and does not write into it. Unmeasured
  (`0`) falls back to `FALLBACK_PAGE_ROWS`; a page verb must never be a no-op.
- **One icon table, `src/tui/file_icons.rs`.** Both navigators call it for
  the glyph and its tone; neither owns the mapping. The folder glyph doubles
  as expansion state, so there is no second caret column in a 22-column pane,
  and a selected row is *repainted* (bg + fg) rather than recolored so the
  type color survives the cursor landing on it. Nerd Font by default;
  `CODEGG_FILE_ICONS=plain` for a terminal without a patched font — an env
  switch, not a config key, because it is a terminal-capability question.
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
- **The editor route has ONE mode axis, and it is `ui_state.input_mode`.**
  This is the invariant that was violated for the whole life of `EditorFocus`.
  The route used to carry `EditorFocus` (`Composer`/`Buffer`) *beside*
  `EditorMode`, the two could disagree, and a fresh open defaulted to
  `Composer` — so `handle_editor_key` claimed nothing except `Ctrl+E` while
  the bare-text gate swallowed every printable. The file rendered and no key
  reached it, which read as "the editor does not work" rather than as two
  toggles. Never reintroduce a second axis. On `Route::Editor`: **`Insert` puts
  the keyboard in the document** (the cursor is drawn in the text box);
  **`Normal` gives it to the Helix verbs and leaves typing to the bottom input
  box**. `handle_normal_key` returning
  `BufferKey::Unhandled` is the *mechanism* for the second half, not a
  fallback — the editor claims navigation and editing and everything else
  stays a composer keystroke. Same contract as `/terminal`: NOR reads, INS
  types.
- **A file opens in `Normal`, not `Insert`.** Same choice `/terminal` and the
  panels make: they are read before they are typed into, and `i` is one
  keystroke away. It used to enter `Insert`, which cost two concrete things —
  the first keystroke of a mistaken open became an *edit*, and `j`/`k` silently
  inserted newlines, so every navigation key was an edit key until `Esc`, a
  chord nothing on screen named. `open_document` and `:new` both call
  `enter_editor_normal`; nothing puts a fresh document in `Insert`.
- **Insert mode has its own chords, and its table is a tuple match.**
  `handle_insert_key` dispatches on `(key.modifiers, key.code)`, because every
  Helix insert-mode chord is a *modified* key: `Ctrl+h`, `Ctrl+d`, `Ctrl+j`,
  `Ctrl+k`, `Ctrl+u`, `Ctrl+w`, `Alt+Backspace`, `Alt+d`, `Alt+Delete`, plus
  `Ctrl+x` for the completion popup. A chord may mean one thing in `Normal` and
  another in `Insert` (`Ctrl+d` pages in one and deletes a character in the
  other) — each still has exactly one owner, and the owner is the stage that
  claims it. **The catch-all character arm is last**, so an unmodified
  character chord added later sits above it rather than under it. Teach
  `scripts/check_tui_editor_key_authority.py` any new dispatch shape *before*
  the shape lands: it reads the tuple table and the legacy `match key.code`
  table, treats an arm it cannot resolve as an **error** and never as a pass,
  and its `--self-test` runs synthetic tables through the same analysis the
  real check uses.
- **A typing burst is one undo step.**
  `TuiDocumentPresentation::insert_session` accumulates the edits of one `INS`
  burst and commits them as a single `EditorUndoEntry` when insert mode ends,
  so one `u` takes back the whole run rather than its last keystroke. A run is
  one *contiguous insertion*: a deletion, a replacement, or an insertion away
  from the end of the run ends it, because those are separate edits. The
  session keeps bounded `TextTransaction`s and never a copy of the text. A
  reconciliation **discards** an open session rather than committing it — the
  text it described is the pre-reload text.
- **The first edit of a run is asked the same question as every later one.**
  `absorb_insert_edit` asks whether an insertion continues the run; the first
  edit must clear the same bar, and `submit_edit` asks it through
  `typing_run_continues`: the cursor must end where the inserted text ends. A
  run whose text does not end where the cursor is cannot be continued. The
  open-line verbs are why — `o` inserts its newline at the **end** of the line
  it opens and leaves the cursor at its **start**, so the text the reader
  types goes in before the newline, and the open-line edit is its own undo step
  rather than the first keystroke of a run.
- **`o` and `O` open a line, through the shared edit funnel.**
  `editor_open_line` shares `editor_editing_command`'s read-only gate, its
  snapshot and its `resolve_edit`; it differs only in what it returns. A verb
  that **writes** claims its key and reports the refusal
  (`BufferKey::Consumed` plus a notice), so `o`/`O` behave like `d`/`D`/`x`; a
  verb that only **moves the cursor** (`i`/`a`/`I`/`A`) leaves the key
  unclaimed and lets the status row's "read-only" be the report. The open-line
  cursor is the **start of the opened line** — the two cases reach it by
  different arithmetic, and getting it wrong puts the reader's typing on the
  line after the one that was opened.
- **The history is one list with a cursor in it.**
  `TuiDocumentPresentation` holds `history`, `history_bytes` and
  `history_cursor` — not an undo stack and a redo stack. `history_cursor` is
  the number of *applied* entries, so undo and redo are the same movement in
  opposite directions and an entry never moves between containers. The cursor
  moves **only after** the apply is accepted: a refused apply leaves the
  document, the history and the cursor exactly where they were.
  `MAX_EDITOR_UNDO_DEPTH` (256) and `MAX_EDITOR_UNDO_BYTES` (4 MiB) are **one
  cap over the history as a whole** — the two-stack model enforced them
  twice, so its worst case was twice the documented bound.
  `Alt+u` and `Alt+U` (Helix's `earlier`/`later`) are the explicit history
  gestures and **report** their boundary ("no earlier change" / "no later
  change" in the status row); `u` and `U` stay silent there, as they always
  have — a CodeGG convention, not a Helix behaviour. The position is
  observable: while the cursor is not at the tip the banner row says
  `history <at>/<of>`, and at the tip it says nothing.
- **A modifier is an exact bitmask, and a capital has two spellings.**
  An arm's modifier predicate is read as the set of **exact** bitmasks it
  admits: `m == alt || m == alt_shift` admits exactly `ALT` and exactly
  `ALT | SHIFT`, not their combination, and the key-authority guard compares
  those bitmasks rather than modifier *names* — `ALT` and `ALT | SHIFT` are
  disjoint, exactly as Rust's `==` on the bitflags says they are. Write a
  multi-modifier arm as a `let alt_shift = KeyModifiers::ALT |
  KeyModifiers::SHIFT;` binding, never as the or-pattern `m == alt | m ==
  shift`, which is not a bitmask at all. And a terminal may report a capital
  as the shifted glyph (`Char('U')`) or as the plain glyph plus `SHIFT`
  (`Char('u')` with `SHIFT`): an arm that claims a capital accepts both
  spellings, because a chord live on one terminal and dead on the next is a
  binding that does not exist. When the two spellings differ in the *key
  code* — `Alt+U` is `Char('U')` with `ALT` **or** `Char('u')` with
  `ALT | SHIFT` — they need two arms, and the guard counts arms.
- **The `m` grammar is two levels, and the sub-state rides in the variant.**
  `EditorPrefix::Match` is the first level and `EditorPrefix::MatchArg(arg)`
  is the second, with `MatchArg` naming the argument being waited for — so
  the presentation still has **one** pending-prefix field and
  `scripts/check_tui_editor_prefix_state.py` stays green. A second
  `pending_*` field is exactly what that guard exists to prevent.
  The leaf table does not carry a **verb**: the two-level commands' leaves
  all carry `PrefixAction::Capture(MatchArg)`, whose payload is a `Copy`
  enum, and the verb is produced when the *capture* resolves — which is what
  keeps `PrefixAction` and `PrefixLeaf` `Copy` and the leaf tables `const`.
  `EditorPrefix::display()` returns a `Cow` so the two halves of `m r` do
  not render alike: the second half shows the character already captured
  (`m r (`).
- **A bracket match that cannot land says which of the two things went
  wrong.** `m m` on no delimiter and `m m` on a delimiter with no mate are
  different answers — one means *move the cursor*, the other means *the code
  is unbalanced* — and both are reported. The disclosure lives on
  `TuiDocumentPresentation::match_note`, beside the find and paragraph
  notes, and `lift_motion_notes` lifts it into the notice immediately after
  the motion, so it describes exactly one keystroke.
- **A surround edits, and then holds a selection.** `submit_edit` clears
  every selection on success, so a verb that means to leave one behind says
  which one: `HelixVerbOutcome::EditAndSelect` carries the members, and the
  command layer applies them **after** the submission. The members are in
  **post-edit** coordinates, and they are **computed** rather than read — a
  post-edit offset past the old end of the document cannot be read from the
  pre-edit snapshot, because `clamp_offset` pulls it back to the old end.
- **A text object follows the motion, not a private rule.** `m i w` uses
  `is_word_byte` and `m i W` uses `is_long_word_byte`, so each object agrees
  with the `w`/`W` the reader just used to get there. A WORD is everything
  up to whitespace, punctuation included — on `a.b` the two objects differ.
- **The space menu is a table, and a leaf names a request, not an action.**
  `SPACE_LEAVES` holds one row per Helix space gesture, and every row names
  what it wants: `PrefixAction::Effect(EditorPendingEffect)`. The effect enum
  is **editor-level** (`src/tui/editor_effect.rs`), not LSP-level — a file
  picker and a clipboard verb are not language-server facts, and
  `EditorLspState` is reset wholesale on a scope change, which is the wrong
  fate for them. The leaf parks the effect on `EditorState::pending_effect`
  and `handle_editor_key` drains it on the very next statement: one hop, not
  a queue, so a leaf can never leave a request armed.
- **`space f` is a request for a root, not a walk.** The listing is
  daemon-owned (ADR-0022, C005): it is resolved from the active tab's
  canonical project/workspace binding, and the TUI never walks its own
  filesystem. So the editor checks that binding **before** it opens anything —
  with no workspace root, or with a root that has no canonical ids, it says
  so and opens nothing, because a browser that opens empty is a key that
  appears to do nothing. `App::open_file_picker` is the one place that knows
  how to seed the browser, so `/files` and `space f` cannot disagree about
  what it shows, and the browser's title names the root it is browsing.
- **A space key that cannot run says why.** `EditorPendingEffect::Unavailable`
  carries the reason, and it renders as a **status row**, not a toast: a
  toast disappears, a status line stays and can be read. Every Helix space
  glyph is bound, bound to a statement, or documented as out of scope — there
  is no dead key in the menu.
- **A preview is a read, and a complete one can become an edit.** `space r` and `=`
  show what a rename or a format *would* change. `space r` collects the new
  name on the `:` command line — the mechanism M008 established, not a second
  input mode — so the leaf arms the prompt and the read follows it. Since M016
  a preview that is complete enough carries the material the apply path
  consumes, and the chord opens the **change review** on it: the diff on
  screen is the change that would be written, `[a]pply` sends it through the
  daemon's checked apply path, and `Esc` rejects it. A preview that cannot be
  applied stays a preview and says which it was. Since M017 `space a` is a
  preview too — the **code-action read** — and it is a *choice*: the LSP panel
  is the picker, `j`/`k` move and `Enter` opens the review on the selected
  action. Do not open the review on the first action: the reader has not
  chosen one. The write path is **not** missing — ADR-0008 owns it, and the
  editor reaches it by carrying the candidate to the one builder,
  `build_apply_request`. There is no registry and no second apply path.
- **A Normal-mode chord that needs the App layer reports what it wants.**
  `=` is a Normal chord, not a prefix leaf, and `pending_effect` is
  documented as *a prefix leaf's* effect — so `=` returns
  `BufferKey::LspRead(LspReadOperation)`, the same shape the other
  Normal-mode verbs use: the buffer decides **what** was asked for, and the
  App layer — which holds the core client — does **how**.
- **A leaf table is policed by its own guard.**
  `scripts/check_tui_editor_leaf_authority.py` rejects a duplicated glyph
  inside one table, a glyph that is not printable ASCII, one glyph claimed by
  two tables of the same prefix, a leaf behind a guard-block chord, and a
  table no prefix binds. It reads the guard-block chords through the
  key-authority guard rather than re-parsing the dispatch chain, because two
  parsers for one shape is two models of it. A leaf is consulted only while
  its prefix is armed, and the resolver disarms before consulting it, so a
  leaf **cannot** shadow a directly-bound chord — the earlier rule that said
  otherwise was demonstrably false on the shipped tree.
- **A panel never claims a key that means text.** In `INS` a printable is
  text. The completion popup and the LSP diagnostics panel are mode-aware:
  their `j`/`k`/`r`/`Enter` claims are Normal-mode claims, and in `INS` the
  popup's `Enter` is a newline, its `↑`/`↓` move the cursor, and its
  `Tab`/`Shift+Tab` move the candidate selection.
- **`/` searches the document, not the command catalog.** On this route the
  composer arms bare text with `:`, so an unclaimed `/` became `:/` and
  resolved to no command — a key that looks like it should do something and
  silently did not. `handle_normal_key` claims it, seeds from the word under
  the cursor, and owns *every* key while open (same modal contract as `:`,
  including `Ctrl` chords). It searches **as you type**; `Enter` accepts and
  keeps the highlight, `Esc` abandons the whole search. `n`/`N` are claimed
  **only while a search exists**, so an `n` with no search still reaches the
  composer. Bounded (`MAX_EDITOR_SEARCH_MATCHES`/`_BYTES`), and a partial
  result says so in the status row rather than reading as complete.
- **The cursor is painted *last*, in `line_spans`, and that ordering is the
  contract.** Four layers resolve into one set of spans — syntax, `/` search,
  selection, cursor — each overriding only the fields it owns. Nothing may
  bury the position.
- **Every background layer pins `fg(theme.foreground)` with it.** "Patch only
  the background" is *not* a safe compromise; it is the same dark-on-dark bug
  one layer down. It was tried: the current line rendered as
  `fg(theme.alternate_bg)` (a *background* colour as a foreground), so on every
  dark theme the cursor's own line was the one line a reader could not read.
  The current line is now `bg(theme.alternate_bg)` — a tint that leaves the
  syntax colours alone.
- **The document area needs its theme background restated.** The block paints
  `theme.background`, then `render_text` renders `Clear` over the text area —
  which resets every cell to the terminal default — and a bare `Paragraph`
  inherits it. Both `Paragraph`s now carry
  `bg(theme.background).fg(theme.foreground)`.
- **The Normal cursor is reverse video, not an accent block.** It must stay
  legible over whatever the highlighter painted, and a fixed accent
  background cannot promise that (dark token foreground over a dark accent is
  the same unreadable pairing). `fg(background)` on `bg(foreground)` guarantees
  contrast for every token color and every theme without the renderer knowing
  which of them is light. Insert draws a thin bar (`fg(primary)` +
  `UNDERLINED`) — a block would cover the character being typed over. A cursor
  past the last character still owns a cell: one space past the end, so "the
  cursor is at the end of the line" is never invisible.
- **Syntax highlighting has a server-free floor.** LSP semantic tokens stay
  authoritative, but a language server is *optional* — with none installed
  every file rendered flat, which made highlighting a property of the machine
  rather than of the editor. `tui::editor_highlight::EditorHighlight` is a
  bounded `syntect` scan underneath, keyed by path/code-theme/revision, living
  on `EditorState::highlight` and passed as a scoped `&mut`. It holds byte
  ranges and colors only, so the text-authority guard is unaffected. Its scan
  is **forward-only and sequential** because `syntect` is a state machine (line
  N depends on line N-1 opening a block comment); backwards scrolls replay from
  the top, bounded by `MAX_HIGHLIGHT_SCAN_LINES`, past which it switches
  **itself off** and says so. An unknown extension renders plain, not wrong.
- **`Ctrl+d` continues to the last line** when a half-page would land past the
  end — half a window from the last page is an ordinary place to be, and a
  motion that quietly refuses there reads as a dropped keypress.
- **`Esc` on the editor route is a mode step, never an exit.** Insert → Normal;
  Normal → a toast naming `:q`. It is answered in `handle_editor_key` *above*
  every other editor key, so it cannot reach anything that closes the view. It
  used to call `close_editor`, which meant the only key that could get you out
  of Insert was also the key that threw the document away. The only exits are
  `:q` and `:wq`, and the frame says so. It closes an open `/` line first, then
  an open `:` line, then steps the mode.
- **The buffer speaks Helix, matching `/terminal`.** `hjkl`/arrows move,
  `Ctrl+d`/`Ctrl+u` page, `e`/`b` jump by word, `v` selects by character, `x`
  selects the whole line and grows one per press, `d` deletes the held
  selection, `space y` yanks it, `0`/`$`/`gg`/`G`/`u`/`Ctrl+r` and `i/a/I/A/
  o/O` as in Helix. vi's `dd`/`dw` are gone (`x` then `d` is the same gesture);
  `D` stays as delete-to-line-end. Bare `d` is **claimed with nothing selected**
  — an unclaimed `d` escapes to the global `OpenDiff`. `space y` is remembered
  prefix state, not a chord, because legacy xterm sends it as two events.
  Uppercase verbs bind with *and* without `SHIFT`.
- **Selection reuses `tui::helix_selection`; `editor.rs` owns only geometry.**
  The editor instantiates `SelectionState<EditorPoint>` where `EditorPoint` is
  `{ byte, line }`, and `SelectionState::restore` lets it drive the shared
  machine while keeping its own byte-offset cursor. Never re-derive `v`/`x`
  locally — `v` is a mode and `x` is an action on both surfaces, and that split
  is the only reason `x` can mean both "select this line" and "again, the
  next". `editor::selection_range` is the single resolver: character mode is
  inclusive of the head, line mode spans whole lines including terminators so a
  delete leaves no trailing blank line. The renderer patches only `bg` for a
  selection, never `fg`, or highlighted text becomes dark-on-dark exactly where
  the user is selecting.
- **Syntax highlighting is ambient, debounced, and never steals a key.**
  `EditorLspState::clear_semantic` runs on every local edit (spans are
  revision-bound), and the re-request is deferred to
  `EditorState::arm_semantic_refresh` + `service_editor_semantic_refresh` from
  the render pump, so a fast typist produces one request, not one per
  keystroke — and the file is never permanently colourless. `F9` is the manual
  retry. **Not F8**: F8 is the completion trigger, and a highlight refresh
  stealing it would make completion unreachable. `request_editor_semantic_tokens`
  deliberately never opens the LSP result panel, so a server that answered
  `NotReady` leaves an unhighlighted file rather than a modal over it.
- **The editor replaces the main buffer, so the composer arms with `:`.**
  `composer_refuses_bare_text()` is true only on `Route::Editor` in **Normal**
  mode, and the gate sits on the single `InputAction::Char` arm
  so every typing path is covered by one rule. Scoped narrowly: a focused
  side panel has its own composer and chat is a conversation, so both stay
  freely typeable.
- **The directionals survive the editor buffer.** On `Route::Editor` in
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
  `[NOR|INS] [agent] [LLM]` — one mode, one agent, **one** destination. The mode
  is `App::composer_input_mode`, never the raw `ui_state.input_mode` (see the
  terminal bullet below for why that field lies). That destination is
  `App::composer_destination_title`: the focused panel's
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
  `render_working_strip` draws one status row plus one row per advertised
  subagent between the transcript and the composer, reserved from the prompt
  allocation and carved back off like the hint line and the todo strip.
  **Row 0 is reserved unconditionally** — it reads `idle` when nothing runs —
  because this height is subtracted from the transcript (`session_layout` splits
  `prompt_height` against `Min(10)`). Gating it on turn state reflowed the
  transcript on every submit and every completion; reserving it permanently
  lets the content change freely with the viewport height fixed. Subagent rows
  stay content-driven, since N subagents need N rows. Row 0 resolves in strict
  precedence — latched `last_error`, then `stopped_turn`, then the live phase,
  then `idle` — and pairs it with the key that changes it (`x to stop`, `press x
  again to stop agent` while a confirmation is armed, `press x or /start` once
  stopped): a long pause with no way out is what reads as a dropped connection.
  Error and stopped both draw a **static** glyph; nothing is running, so an
  animated one would lie about it.
  `ClickTarget::Working` is hit-tested **before** `ClickTarget::Prompt`,
  because the strip is carved out of the prompt chunk — the reverse order
  resolves every subagent click to click-to-focus. Hit rects live in
  `SubagentState::hits` and are rebuilt every frame.
- **One spinner, one cadence, one phase enum.** Frames come from
  `tui::components::spinner::spinner_frame(elapsed)` — a pure function over one
  frame table at 100ms, so there is no counter to drift and renders stay
  reproducible under a `TestBackend`. It once indexed a ten-frame table by
  *whole elapsed seconds*, so a rotation took ten seconds while the 80ms pump
  repainted an unchanged glyph eleven times in twelve, and a second
  `SpinnerWidget` on `App` drew a competing indicator at a different speed.
  Turn state is an `ActivityPhase`, never a free label: a `String` could only
  name the phases somebody remembered to pass in, which is why a turn parked on
  a permission prompt and a provider backing off between retries both read as a
  silent screen. Turn failures are **latched** into `SessionState::last_error`
  via `report_turn_error` (which also toasts) and clear on exactly one event,
  `TurnStarted` — not on a timer, so a user reading the reason is never
  interrupted mid-sentence. Route genuine turn failures through it only; shell,
  clipboard, and `/theme` errors would otherwise claim a turn failure that never
  happened. The full text stays in the toast and `/logs` keeps 50.
- **One approval on screen; the rest queue, and the queued row is the way back.**
  `show_permission_dialog` used to assign straight into `permission_dialog`, so
  the second `PermissionPending` of a batch destroyed the first *along with its
  `perm_id`*. Nothing could answer it; the daemon waiter sat on an unbounded human
  wait and the tool timed out denied for a reason the user never saw. One dialog
  is mounted, the rest live in `DialogState::permission_queue`, and every answer
  path clears the dialog fields **before** `close_dialog()` and then promotes the
  next. `close_dialog`'s `Dialog::Permission` arm requeues whatever was still
  mounted — a dismissal is not an answer — which is also why the clearing order
  matters. Because `CoreEvent::PermissionPending` has no `args`,
  `mount_permission_dialog` recovers them from the transcript rather than showing
  "No additional details". The transcript mirrors the open approvals into
  `pending_permission_tools` so a `Pending` row can offer `queued — waiting on
  your approval · Enter to review` and both the click path and its keyboard twin
  reopen it. Make the marker honest: advertising the action where nothing is
  waiting is its own dead end.
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
  `TuiCommand::TurnSubmissionFinished { outcome }` (daemon refused it or the
  socket is dead), and `dispatch_turn_submit_request`'s no-core-client and
  no-session early returns. Route them through
  `App::apply_turn_submission_finished` / `App::fail_unstarted_turn`, which
  clear `working_since` (so the watchdog stops counting a turn that never
  started), set `Error`, stop the thinking indicator, and **release
  `pending_send` so a retry is possible**. Never record a submission failure in
  a log line and return; the log is not the UI.
- **A turn already running is not an unstarted turn — do not tear it down.**
  `turn_already_active` (both the in-memory collision and the ADR-0007
  controller-lease conflict) means a turn is *running*. Sending it through
  `fail_unstarted_turn` nulled `active_turn_id` and `working_since`, stopped the
  spinner, and set `Error` on a session that was actively working — the UI
  asserting the opposite of the truth while the user waits.
  `App::fail_contended_turn` releases only what the submit owns (the composer
  guard and the text) and reports a warning toast. It deliberately does **not**
  call `report_turn_error`: row 0 gives `last_error` absolute precedence, so
  latching would hide the live turn's spinner and elapsed time behind a stale
  `✗`. Contention is not a turn failure — the turn continues.
- **Classify a refusal before consuming any state.**
  `src/tui/app/turn_submit.rs` is the one seam that reads a `turn.submit`
  result, and it produces a typed `TurnSubmitOutcome` the completion arm applies
  as data. Two rules keep it honest: an **unknown daemon code classifies as
  `Rejected`**, not `Unexpected` — `Rejected` is the only class that keeps the
  user's text, and the daemon grows refusal codes; `CONTENTION_CODES` is an
  exact allowlist, never a default or a resemblance match. `Unexpected` is
  reserved for a response *shape* the frontend cannot read, and does not
  restore the prompt, because that submit may already have been accepted.
  Do not re-derive a class by matching message text at a second site.
- **Give the user's text back on a recoverable refusal.**
  `send_prompt`'s optimistic clear means the composer held the only editable
  copy. Use `PromptState::restore_prompt_with_stash` — one shared shape, already
  used by session-create failure and reconnect — so the newer draft is displaced
  into the bounded stash and `mark_retry_without_message` suppresses the
  duplicate transcript entry on resubmission. The bound applies to how many
  drafts are kept, **never** to the restored text: truncating would change what
  the user wrote. Carry the prompt in `PendingTurnSubmission::prompt`; never
  re-read it from the composer, which the user may have kept typing into.
  **Never resubmit on the user's behalf** — handing the text back is the whole
  recovery, and anything more is an unapproved queue.
- **Keep the daemon idempotency key and the local staleness marker separate.**
  `PendingTurnSubmission::request_id` is the daemon's receipt key and its
  derivation must not change. `App::turn_submit_request`'s generation answers a
  different question — *is this completion still about the submission the user is
  looking at?* — and `apply_turn_submission_finished` calls
  `finish(generation)` **first** and drops the completion on `false`. Without
  that, a late completion from a superseded submission still nulled a live turn's
  identity: the old arm guarded only the `pending_turn_submission` clear and
  then tore down turn state unconditionally. `App::cancel_turn_submit`
  invalidates on tab switch / reconnect / project change.
- **Never attribute a refusal to a component that is not at fault.** A missing
  dependency is not a diagnosis. `app.core_client` being `None` means two
  different things depending on the transport: on the socket-daemon frontend it
  really is a lost connection, but on the `/tui` WebSocket attach frontend the
  daemon is healthy and that frontend was simply never given a `CoreClient`
  (`src/client/attach.rs` speaks `TuiMessage`). Answering the second with
  "Daemon connection unavailable" sends the operator to restart a working
  process. Classify the cause explicitly through one seam that owns the wording
  — `terminal_core_client` → `TerminalRefusal` for terminals — rather than
  retyping a sentence at each site; duplicated text is exactly how the wrong
  cause propagates to nine places. The discriminator is
  `App::is_ws_attach_transport()`, never `AppMode` alone.
  Related: classify a refusal **before** any state is consumed.
  `flush_terminal_input` used to claim the input slot and drain the queued bytes
  and only then discover there was no client, which dropped the keystrokes *and*
  left `input_in_flight` set with nothing in flight to release it — one visible
  error, then permanent silence.
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
- **Selection uses `bg`, not `fg`, and its text is `theme.selection_text`.**
  `theme.selection` is a background; using it as a foreground painted selected
  message authors dark-on-dark. Its text needs a colour that reads on *that
  fill*, which is not necessarily the body foreground — a light theme's
  selection can be pale enough that dark text is required. Use
  `fg(theme.selection_text)`; it is derived per theme, never hardcode a literal
  foreground (a `Color::White` row is white-on-white on every light theme, which
  is what the old selection code did). `Theme::selection_style()` already pairs
  them. Unselected bodies are `foreground`, not `muted` — dimming the whole
  transcript whenever nothing was unread made the common case the unreadable
  one.
- **A render path reads the theme it is handed; it never caches one.**
  `Component::render` / `render_in_panel` receive the live `&Arc<Theme>` every
  frame. A box that stores `Arc<Theme>` and refreshes it only when opened goes
  stale the moment the user changes theme, because the seven sites that
  reassign `UiState::theme` do not re-push into an open box. A box that
  reassigns the field from inside its own render is *also* wrong — it happens to
  work, and it is the shape a source grep ranks as the worst offender.
  Enforced by `tests/tui_theme_source_of_truth.rs`: render with theme A, render
  again with theme B, and no interior foreground may be a colour A defined.
  `Widget` cannot receive a theme, so a widget that must stay one has to be an
  inherent method instead — `DiffViewer::render_in`, `PlanDialog::render_legacy`,
  `DiffDialog::render_legacy`.
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
- **Interactive terminal keys are modal input, one key per meaning.** INS (the
  process owns the keyboard): every printable key plus `Enter`/`Backspace`/`Tab`/
  arrows/`Ctrl-C`/`Ctrl-D` goes to the PTY; `Esc` returns to NOR; `Ctrl+]` sends a
  real `0x1b` for full-screen programs and stays in INS. NOR (the reader owns it):
  helix `hjkl`/`Ctrl+u/d`/`e`/`b`/`v`/`x`/`d`/`space+y`/`C`/`D`, `i` or `Enter`
  back to INS, `Esc` detaches. `/terminal` opens the box **already in INS**.
  Terminal output is interpreted by bounded VT screen state before Ratatui
  rendering; never write raw child escape sequences to the host terminal.
- **A surface with the keyboard is not a viewer.** The dock gesture (`H`/`J`/`K`/
  `L`, `Shift`+arrows) must be gated on "nothing is being typed", and for a
  terminal that question is `TerminalFocus`, **not** `InputMode` — a terminal in
  INS still reports the app's `Normal` mode, so a Normal-only gate let an
  ordinary shell letter be read as "move the panel". The character never reached
  the process and the column moved out from under the user, which is
  indistinguishable from the terminal eating their command. Every other surface
  already gates this on its own text buffer; the two terminal surfaces were the
  ones missing it (`claims_panel_directions` for the panel host, and the
  `DialogType::Terminal` pre-check in `App::on_key` for the box).
- **Output is pulled on the existing cadence, and input is serialized — both are
  correctness, not polish.** `service_terminal_output_pull` rides the same pump
  tick as the editor's semantic refresh (never a second timer) and skips unless a
  mounted surface holds a live attachment with no read outstanding. Flushing
  input per keystroke spawned one concurrent daemon request per character, and
  those raced, so fast typing arrived **scrambled**. `queue_resize` must no-op on
  an unchanged size: a PTY resize flushes tty pending input, so a per-frame
  resize storm eats the half-typed word. And a stale test is a finding, not
  noise — a red assertion left behind by a deliberate contract change reads as
  "pre-existing bug" and ships.
- **"Rides the existing cadence" is a claim about the *arming*, and it was false
  for the idle case.** The pull has exactly one call site: the timed arm of the
  loop's `select!`. That arm parks on `futures_util::future::pending()` when
  `next_wake` is `None`, and `None` does not mean "sleep a while" — the arm never
  runs again until some *other* event wakes the loop. `next_wake` comes from
  `runtime::event_loop::animation_interval`, whose contributors were all
  transient (streaming, working, toast), so a mounted terminal was read only
  while a toast happened to be alive, a keystroke arrived, or a resize was
  debouncing: a shell's output became visible when the user pressed the **next**
  key. When you add a service to that tick, assert the **arming decision**, not
  just the function — a test that calls the service directly stands in for the
  arm and proves nothing about reachability. `animation_interval` is now a pure
  `&App` function precisely so that is testable. Two rules keep
  `terminal_pull_armed` honest: never gate the arm on read state
  (`begin_output_read`, refusals, `TerminalLinkState` — that latches the loop
  off, because a dark arm means nothing wakes it to issue the read that would
  clear the condition), and key it on **mount**, never focus, since focus is what
  changes when nobody is looking.
- **The `[NOR]`/`[INS]` badge reports where the keyboard is, not what the
  composer thinks.** `ui_state.input_mode` is the composer's *own* mode, and it is
  the wrong answer whenever the keyboard is elsewhere: `focus_panel` forces it to
  `Normal` on panel focus, and the terminal box never touches it at all. So a
  terminal in Insert rendered `[NOR]` above the input, and the terminal panel's
  own Insert hint branch — which branches on that same field — was unreachable
  dead code. One field, read twice, both wrong. Resolve it once in
  `App::composer_input_mode` over two named seams that key routing already reads:
  `App::terminal_box_has_keyboard` (a modal box owns the keyboard either way, so
  it reports `Normal` after `Esc` even though the composer's field still claims
  `Insert`) and `PanelHost::claims_insert_keys` for a docked panel. Neither half
  can answer the other — a box is not a side panel, and a panel is not a dialog.
  It is a **pure read**: a badge that forced `input_mode` to match what it
  displays turns the state it reports into the state it causes.
- **The composer's caret is derived, and `PromptWidget` has no `focused` flag.**
  `App::composer_holds_keyboard` answers "does the composer hold the keyboard",
  in the same shape as `composer_input_mode` and over the same seams. Its order
  mirrors `App::on_key`'s priority chain: a mounted modal owns the keyboard, then
  a focused side panel (`App::panel_owns_keyboard` — the extracted `panel_active`
  condition, so the key chain and the caret rules read **one** derivation), then
  the editor's buffer when it is explicitly focused; otherwise the composer owns
  it. The old `focused` field was **dead in production** (`PromptWidget::blur()`'s
  only caller was a test), so "no cursor while another surface has the keyboard"
  was unimplementable as written. Do not reintroduce a stored flag: a copy of
  this fact can drift from the badge, which is the defect class the mode
  resolution exists to kill.
- **Two carets, mutually exclusive, and the blink is the terminal's.**
  Insert mode positions the **terminal's own cursor** at the caret with
  `Frame::set_cursor_position` (never the deprecated `Frame::set_cursor`); Normal
  mode paints a solid block instead. Both read
  `PromptWidget::caret_cell`, the single computation of where the caret is on
  screen, so the two cannot disagree. The app runs **one** animation cadence
  (the 100 ms working strip), so a blinking caret must come from the terminal —
  ratatui hides the terminal cursor whenever a frame did not position it, so
  "position nothing" is how the composer shows no cursor at all. The rule the
  badge and the caret share: **when the composer holds the keyboard, `[INS]`
  means the terminal cursor is positioned and `[NOR]` means the block is
  painted; when it does not, neither caret is present.**
- **The composer owns a closed set of Normal-mode letters, and a claimed
  letter never falls through.** `src/tui/composer_claims.rs` is the single
  table; the app's binding map is consulted only for letters the composer does
  not claim. A claimed letter is consumed **even when its verb is not
  implemented yet** — that is the fallthrough rule, and it is the whole point
  of the table: a reserved claim that leaks becomes an unrelated app action
  (the terminal panel records the failure verbatim: an unclaimed `d` silently
  became `OpenDiff`). The claim step runs **before** the binding map, inside
  the composer branch of the key chain, so a modal, a focused panel, the
  Workspace view and the editor route are all consulted first. The gate is
  the *resolved* mode plus `App::composer_holds_keyboard`, never the raw
  `UiState::input_mode`. A condition on a claim must be written on the entry
  (`j`/`k` are the one conditional pair): an implicit conditional is how a
  leak reappears. A claimed **capital** answers to both encodings
  (`Char('U')` with `SHIFT` and with `NONE`), like the app's
  `insert_panel_direction_bindings`; a lowercase claim is the bare key only.
- **A consumed claim that does nothing is a dead key — never claim a letter
  the app needs to keep working.** `i` is the app's **only** route back to
  Insert mode (it is the `None` arm of the key chain, not a binding), so its
  verb is **live**: it runs the transition the app already had, and the table
  is where that ownership now lives. A reserved `i` would swallow the key,
  do nothing, and leave the user in Normal mode with no way to type. When a
  verb is promoted from reserved to live, the runner in
  `App::run_composer_claim` is where the behaviour goes — not the binding
  map — and the `None` arm keeps the gesture only for the surfaces the
  composer does not own.
- **The live verbs so far are the motions and the selection verbs.** `w`, `b`,
  `e`, `j`, `k` (motions), `v`, `x`, `d`, `p` (selection and clipboard),
  `i`, `a`, `o` (the insert transitions) and `u`, `U` (undo and redo) — every
  claim in the table is live, so the runner's match on the verbs is
  **exhaustive** and a verb added without an arm does not compile. Two rules
  make them work, and
  both are shared with the editor rather than local to the composer: a motion
  moves the caret through **`move_caret`** (the machine's `move_to`), never
  `set_caret`, because that is what makes a held character selection *grow*
  with the motion; and the selection has **one** byte range
  (`ComposerText::selection_range`, the editor's rule term for term) that
  delete, copy and the renderer all read. A second range is how "copy" and
  "delete" stop meaning the same thing. The word and vertical **targets** come
  from `src/tui/text_geometry.rs`, so the composer and the editor land on the
  same offset for the same text; a composer with its own word rule would be
  two answers to one question.
- **An edit that changes the buffer's length drops the held selection, and the
  undo history never evicts its last entry.** The selection's points are byte
  offsets into the text the edit replaces, so the rule lives in
  `ComposerText::replace_range` — the one place every edit passes through —
  rather than in each caller's judgement; `undo` and `redo` change the length
  too and drop it the same way. The history's byte bound drops **older** edits
  and never the newest one: evicting the newest edit because it is large would
  make a large paste unundoable the moment it was made. The undo scope is the
  composer's **own text** (roadmap D4): a submit and a staged projection
  replace the buffer and drop the history, so an undo can never bring back
  text the user has already sent.
- **The hint row and the help are DERIVED from the claim table, and the guard
  polices both.** `composer_verb_entries` (`src/tui/composer_claims.rs`) is a
  read-only view over `COMPOSER_CLAIMS` — the live claims, each with its key,
  its human label, and its condition. Three surfaces read it and none keeps a
  copy: the **hint row** above the composer (`App::composer_hint_row`), the
  **help**'s composer section (`composer_help_entries`, `src/tui/input.rs`),
  and the **guard** (`scripts/check_tui_composer_key_authority.py`). A
  hand-written list anywhere would be a second copy of the table, and a second
  copy is the drift the table exists to prevent.
  - **Focus gates the row; the mode changes its text.** The row is present in
    Normal and in Insert alike, so `Esc` does not reflow the transcript — the
    interactive terminal panel's shape (`src/tui/panels/terminal.rs:416`). The
    row names the composer's **keys**, not their descriptions: the
    descriptions live in the help, and a row spelling out fourteen verbs does
    not fit eighty columns.
  - **A part is never split.** The row is bounded by dropping whole parts from
    the end, because a row cut in the middle of a key names a key that does not
    exist. `fit_parts` measures display columns, not bytes.
  - **Two rules police the help.** **G10** asks "does the help describe the
    composer's verbs?" — the composer section must be derived, every live verb
    must have a label, and no app row may advertise a claimed key for a
    different action (the `a` → "Focus sidebar" class) or a reserved claim. The
    one exception is a *conditional* claim's fall-through row, the `j`/`k` row,
    which earns the exception by carrying its condition. **G11** asks "does the
    help describe the *app's* re-spelled verbs?" — each must be documented
    under a modifier chord and not under its old bare letter. Both are scoped
    to the **Normal-mode** section: the Dialog and Command sections describe
    other surfaces, where the same letters mean something else.
- **The copy gesture belongs to the help box, and the ROUTE decides — never
  the mode.** `space` opens the composer's help and `y` in that box copies the
  selection; the `/help` command's box does not copy, and `y` there is a
  filter character. The mode cannot say which box this is: a `/help` box
  starts in Insert and `Esc` moves it to Normal, and a `space` box can be
  switched to Insert with `Enter`. So the route travels **in the open
  request** (`Dialog::Help(HelpRoute)`), and a call site cannot open the box
  without saying which box it is; the default route is the one that cannot
  copy. `HelpRoute` now lives in `src/tui::input` beside the content it
  selects, and `HelpDialog::with_route` **rebuilds** that content — the route
  selects content as well as behaviour, and relabelling the box without
  rebuilding it would leave the chord route teaching a gesture it does not
  have. The copy is a **`TuiMsg`**, not a dialog-side mutation:
  `open_dialog` pushes a *clone* onto the focus manager, and the clone is what
  receives the keys.
- **A letter the composer claims is not available to the app.** The four
  app-level actions that owned `x`, `d`, `i` and `a` moved to `Ctrl+Alt+X`,
  `Ctrl+Alt+D`, `Ctrl+Alt+E` and `Ctrl+Alt+S`, in **both** maps. The `a`
  re-spelling **deleted** a binding rather than moving one: `FocusSidebar`
  already answered to `Ctrl+Alt+S` in both maps, so the vim map's bare `a`
  was a second spelling of an action that already had one — the composer's
  insert-after key took the letter and nothing was lost with it.
  `Ctrl+Alt+<letter>` is the
  legacy-safe family: the terminal delivers `ESC` + the control byte, which
  crossterm reports as the letter with CONTROL|ALT. `Ctrl+Alt+I` is **not**
  usable — `ESC` + `0x09` arrives as `Tab` with ALT. A chord's `(modifiers,
  key)` pair must not be one the table **claims**: a claim is matched on the
  exact modifier set, so `Ctrl+Alt+X` is not the claim for the bare `x` and the
  composer does not take the chord back — but a chord spelled with a modifier
  set the table claims would be swallowed. When an action's only spelling
  becomes a chord, it must leave `is_normal_only`: a chord cannot be composed
  as text, so gating it out of Insert mode would take the action out of the
  mode the app boots in. **A bare spelling of a letter the composer does *not*
  claim is not a re-spelling violation.** The default map's `/` for  focus-prompt is the live example: `/` is not claimed, so the plan's
  invariant — *a letter the composer does not claim must resolve to the same
  app action it resolves to today* — requires it to keep working, and it is
  the gesture that enters command mode, which is what makes the rest of a
  typed slash command land. Removing it broke the only route back into the
  panel manager from a focused panel.
- **Never make one key answer two questions.** `Esc` used to be both "leave
  insert" and "pass an escape byte through", disambiguated by a hidden
  `pending_escape` latch (one `Esc` went to the program, two left). A keystroke's
  destination then depended on state the user could not see. Give each meaning
  its own key and keep the classifier stateless.
- **A panel that fronts a process must opt into Insert-mode keys.**
  `SidePanel::claims_insert_keys` is what makes `panel_active` consult a focused
  panel in Insert. The default is correct for panels that render their own text
  field (there Insert means "type into that field"), and exactly wrong for a
  panel fronting a PTY — without the opt-in, docking with `Shift+L` and typing
  sends everything to a composer the user is not looking at.
- **Focus must never desync between the panel host and the PTY view.**
  `SidePanelHost::blur` and `TerminalFocus` are separate flags, and
  `panel_active` requires `is_focused()` — so a focused PTY that declines
  `owns_exit_key` looks like it left while still reporting INS, and then takes
  no keys at all. A panel whose view can hold the keyboard claims the host's
  mode key (`owns_exit_key` takes `ctx` for exactly this) and ends in the blur
  that clears its own focus. Claim **`Esc` only** — claiming `q` as well meant
  the shell could never receive the most common letter in most commands.
- **Side panels are mouse surfaces.** Record every occupied column's rect on
  each render (`App::panel_areas`) and resolve `ClickTarget::Panel { placement }`
  from it, so the list can never name a column that is not painted. Click focuses
  the column (`TuiMsg::FocusPanelAt`, via the same composer-mode park/restore as
  `Shift+H/J/K/L`); drag joins the viewport in `on_mouse`'s deferred-press arm so
  release copies; wheel scrolls **that column**. An unhandled target falls
  through to the transcript, which is what made scrolling a terminal column look
  like scrolling the conversation.
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

Every guard in the table below fails `verify.sh quick` and the CI `verify` job.
Read them before adding TUI code - they are the executable form of the
invariants above.

| Script | Enforces |
|---|---|
| `python3 scripts/check_tui_project_authority.py` | Scans `tui/app/state/**`, `tui/app/mod.rs`, `tui/commands/**`, `tui/runtime/**`, `tui/command.rs`, and `tui/components/dialogs/command.rs`; rejects `session_state.project_dir` and `std::env::current_dir()` as project authority. Only clearly marked bootstrap boundaries and test fixtures are exempt. |
| `python3 scripts/check_tui_editor_text_authority.py` | In the editor path, rejects a second text buffer (struct fields named `text`/`content`/`lines`/`body`/`buffer`/`rope`/`source`/`document`, owned `DocumentSnapshot`/`DocumentBuffer`/`Rope`, materialized `Vec<String>`/`Vec<Line>`), any filesystem read API, and any `DocumentBuffer`/`Rope` construction. `&DocumentSnapshot` borrows and `TextTransaction` undo history are deliberately allowed (ADR-0011). |
| `python3 scripts/check_tui_composer_key_authority.py` | Reads the composer's claim table (`src/tui/composer_claims.rs`) and the two binding maps, and fails when a letter is claimed twice, when a claimed letter can still reach an app-level action, when a re-spelled chord is missing from a map, when a re-spelled action has no chord to reach it by or still answers to a claimed key, when a capital claim misses an encoding, and when a claimed key bound under `SHIFT` has no `is_normal_only` entry. Carries a `--self-test` that injects each violation class. |
| `python3 scripts/check_tui_editor_key_authority.py` | Scans the editor's dispatch stages and rejects a chord claimed twice, a chord claimed by a later stage after an earlier one took it, and a shadowed arm. The conflict model is the **exact modifier bitmask**, so `ALT` and `ALT \| SHIFT` are disjoint. `--self-test` runs ten cases, including one the guard cannot resolve — that must be an error, not a pass. |
| `python3 scripts/check_tui_editor_prefix_state.py` | Rejects a second pending-prefix mechanism: the presentation must hold exactly one, typed `EditorPrefix`. A grammar that wants a second level puts it in the **variant** (`EditorPrefix::MatchArg`), not in a second field. |
| `python3 scripts/check_tui_editor_leaf_authority.py` | Polices the editor's **leaf tables**: no duplicated glyph inside one table, every glyph printable ASCII, one glyph per prefix across that prefix's tables, no leaf behind a guard-block chord, and no table no prefix binds. Reads the guard-block chords through the key-authority guard. `--self-test` runs nine synthetic cases through the same analysis the real check uses. |
| `python3 scripts/check_tui_editor_apply_path.py` | Polices the **preview-apply path**: one builder (`build_apply_request`) and one `CoreRequest::LspPreviewApply` construction, and a carried candidate that is never registered. Reads ADR-0008's contract. |
| `python3 scripts/check_tui_editor_completion_authority.py` | Polices the **completion popup's triggers**: `F8` claimed exactly once, `Ctrl-x` claimed exactly once, no trigger shadowing the guard block's `Ctrl+s`/`Ctrl+r`, and `Ctrl-x` in Insert mode only. Tells the triggers from the popup's own keys **structurally** — the arms before the popup-open check — and reads the guard-block chords through the key-authority guard. |
| `python3 scripts/check_tui_editor_doc_drift.py` | Polices the **binding set against `docs/tui.md`**: every leaf gesture and every claimed chord the code owns is documented, every key the document declares unbound is unbound, and the document carries an unbound declaration at all. Reads the chords and the leaf tables through the key- and leaf-authority guards, so there is one parser per dispatch shape. `--self-test` runs eight synthetic cases, one of which is the clean tree. |
| `python3 scripts/check_tui_transport_authority.py` | Polices **transport selection**: `AppMode::RemoteCore` names two different frontends — the WebSocket attach path (which owns `remote_send_tx`) and the socket-daemon path (which does not) — so a remote send may only be gated on `App::is_ws_attach_transport()`. Sending on the mode alone drops the message into an absent channel, which has hung a turn twice. |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py` | Polices **subagent descriptor provenance**: the TUI may not resolve a subagent's descriptor against the curated name pool. The descriptor is drawn once, at spawn, by the only code holding the name lease, and every surface downstream reads what the projection carried. A display name is not a stable key, so a local lookup reintroduces the attribution defect C001 recorded once. |

## Testing

The default socket-connected TUI owns a session-scoped projection separately
from `/observe`. `set_session`, active-tab switch, and reconnect start a
generation-fenced subscription; the completion validates project/session/tab
and stream identity before installing the snapshot. Reconnect takes a fresh
authoritative snapshot. Until it lands, `Ctrl+T`, `/sessions`, and the sidebar
show loading or unavailable state instead of claiming an empty session. Keep
the live-event pre-install queue bounded and drain it only after snapshot
installation. Do not route projection events through `AppMode` or the raw
session event subscription.

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
