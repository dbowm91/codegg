# Desktop Frontend and IDE Foundation Milestone 019 — Desktop Terminal Surface over the Daemon's Interactive-Process Service

Status: ready for handoff

Repository baseline: `4aea3df5e` — `origin/main` at the time of writing: the
merged desktop GUI parity line through **M007-E**, M007-D and its corrective
pass (**plan 017**), the projection milestone (**plan 018**), and the
reconciliation pass that closed M007-F under accepted ADR-0056.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`
- The M007-J milestone row: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:571`

Planning basis:

- The M007-J milestone row — *"A PTY surface in the desktop over the
  daemon's interactive-process service — attach, input and resize wiring — so
  `/terminal-show` can focus a named handle and the `show_terminal` frontend
  action becomes performable."* The row records that the milestone needs
  **no daemon work and no ADR**, and that the `show_terminal` action stays
  uncarried, with the desktop's honest notice, until the milestone lands.
- M007-D's closure record,
  `plans/closure/desktop-frontend-ide-foundation/017-status.md` §3.6 — the
  desktop's action table, and the six actions the desktop declares.
- M007-E's closure record,
  `plans/closure/desktop-frontend-ide-foundation/015-status.md` — the
  first-party editor surface, which is this milestone's structural precedent.

Applicable ADRs:

- **None is required for this milestone, and none is created.** The roadmap
  row says so and the code bears it out: the daemon already serves every
  interactive-process request (§3.1), the frontend action `show_terminal` is
  already declared and is already `NonMutating`
  (`crates/codegg-core/src/frontend_action.rs:350-354`), and the catalog form
  `terminal_show` is already a `FrontendEffect`
  (`crates/codegg-core/src/command_catalog.rs:2489-2505`).
- This milestone is bound by **ADR-0010** (the desktop frontend and shared
  client boundary), **ADR-0052** (cross-frontend command execution
  ownership) and **ADR-0053** (the daemon's published projection surfaces),
  and it revisits none of their conclusions.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.4 — a frontend renders
  projections and does not own durable state. The terminal's **bytes** come
  from the daemon; the desktop holds a bounded viewport and a scrollback
  window, exactly as the editor holds a bounded viewport slice.
- `plans/002-long-term-roadmap.md:899` — *"When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap."* **This milestone
  changes no terminology and no canonical ownership boundary.** It
  implements a surface the roadmap already placed inside the line.
- `plans/001-terminology-and-domain-model.md` — the canonical vocabulary is
  unchanged. A **terminal** is the desktop's surface over the daemon's
  **interactive process**; a **handle** names the process, an
  **attachment** names the caller's bound access to it, and the two are not
  the same thing (§3.2).

## 1. Objective

The desktop gains a **terminal surface**: a dockable panel over the daemon's
already-served interactive-process service, with **attach**, **input** and
**resize** wired end to end, so that `/terminal-show <handle>` focuses a
named handle and the declared `show_terminal` frontend action becomes
**performable** rather than an honest notice.

The milestone's deliverable is the **wiring**, and it is complete when the
desktop can attach to a daemon-owned process, send it bounded input, resize
it, read its output back on a bounded cadence, and detach — and when the
`show_terminal` action performs instead of declining.

## 2. Why this milestone is ready

- **The daemon's side is done.** All ten `CoreRequest::InteractiveProcess*`
  variants are served (`src/core/daemon.rs:4834-4996`). No protocol change,
  no daemon change and no ADR is required, which is exactly what the
  roadmap row claims.
- **The desktop's side does not exist.** The host registers **44** bridge
  commands (`apps/desktop/src-tauri/src/lib.rs:1154-1199`) and **none** of
  them names an interactive process. The renderer has no terminal panel, no
  terminal state, and no terminal bridge target.
- **The action is already declared.** `show_terminal` is a declared
  `FrontendAction` with the `NonMutating` disposition, bound to the catalog
  form `terminal_show` (`crates/codegg-core/src/frontend_action.rs:350-354`).
  The only thing missing is a desktop that performs it.
- **The milestone is dependency-ready.** Its only roadmap dependency is
  M007-E (closed 2026-10-25) for the desktop's surface conventions. Its
  behavioural reference — the TUI's terminal — is implemented and stable.

## 3. Current implementation evidence

### 3.1 The daemon serves the whole interactive-process surface

Ten `CoreRequest` variants, ten daemon handlers, no gap
(`src/core/daemon.rs:4808-4822` names the family; `:4834-4996` handles it):

| Variant | Protocol | Daemon handler |
|---|---|---|
| `InteractiveProcessCapabilities` | `crates/codegg-protocol/src/core.rs:3601` | `src/core/daemon.rs:4851` |
| `InteractiveProcessCreate` | `core.rs:3605-3607` | `daemon.rs:4856` |
| `InteractiveProcessList` | `core.rs:3609-3614` | `daemon.rs:4924` |
| `InteractiveProcessAttach` | `core.rs:3617-3623` | `daemon.rs:4930` |
| `InteractiveProcessDetach` | `core.rs:3625-3627` | `daemon.rs:4937` |
| `InteractiveProcessInput` | `core.rs:3631-3634` | `daemon.rs:4940` |
| `InteractiveProcessResize` | `core.rs:3636-3640` | `daemon.rs:4946` |
| `InteractiveProcessResume` | `core.rs:3642-3647` | `daemon.rs:4953` |
| `InteractiveProcessTerminate` | `core.rs:3650-3652` | `daemon.rs:4960` |
| `InteractiveProcessRemove` | `core.rs:3656-3658` | `daemon.rs:4985` |

The response shapes are at `crates/codegg-protocol/src/core.rs:1999-2066`;
the DTOs are at `crates/codegg-protocol/src/interactive_process.rs:234-300`.
The bounds are fixed in the protocol: 64 list items
(`interactive_process.rs:84`), a 64 KiB default and 256 KiB maximum chunk
(`:87`, `:90`), 32 KiB maximum input (`:93`), and 16 attachments per client
(`:75`).

### 3.2 A handle is not an attachment

`InteractiveProcessAttach { handle, from_seq, max_bytes }`
(`core.rs:3617-3623`) returns an `attachment_id`
(`core.rs:2021-2027`). Every later operation — `Input`, `Resize`, `Resume`,
`Detach` — names the **attachment**, never the handle
(`core.rs:3631-3658`), and the daemon resolves the attachment to a process
server-side so an id cannot be replayed across owners
(`core.rs:3628-3630`). The desktop therefore holds an attachment id for the
lifetime of one attachment and never treats it as a handle.

The daemon's ownership is the **transport connection's** client id
(`src/core/daemon.rs:4849`), never a payload field, and the interactive
family bypasses `authorize_request` (`daemon.rs:4569-4597`). A reconnect
changes the client id, which orphans the attachment — see §8.

### 3.3 The catalog serves four of the ten forms, and one of those four is broken

`build_inner_request` (`src/core/daemon_commands.rs:185`) has a row for:

- `terminal_attach` — `daemon_commands.rs:467`
- `terminal_resume` — `daemon_commands.rs:486`
- `terminal_list` — `daemon_commands.rs:592`
- `terminal_send` — `daemon_commands.rs:597`

It has **no** row for `terminal_resize` (`command_catalog.rs:2543`),
`terminal_detach` (`:2579`), `terminal_terminate` (`:2597`) or
`terminal_remove` (`:2615`). `serves_form` (`daemon_commands.rs:778-787`)
publishes the truth to the desktop as `CommandFormView.daemon_served`
(`daemon_commands.rs:881`), so the desktop already knows that resize and
detach are not daemon-served — and refuses them by name rather than
discovering it at runtime.

`terminal_send`'s row is **latently broken**: it forwards the raw text into
`data_b64` verbatim (`daemon_commands.rs:597-601`) while the daemon requires
valid base64 (`src/interactive_process_attach.rs:827-833`). The TUI never
uses that row — it sends input as a raw request
(`src/tui/commands/interactive_terminal.rs:1097-1101`). This is a
pre-existing defect in the daemon's catalog. **This milestone does not fix
it and does not inherit it**: the desktop reaches input through its own
bridge command (§6.2), never through `command.execute`.

### 3.4 The TUI's terminal — the behavioural reference

- The `/terminal-show` dispatcher arm is `src/tui/app/mod.rs:8752-8765`. An
  empty argument toasts *"Usage: /terminal-show <handle>"*; otherwise it
  emits `TuiCommand::TerminalShow { handle }`.
- `handle_terminal_show` (`src/tui/commands/interactive_terminal.rs:1612-1637`)
  refuses an unknown handle with *"Unknown terminal {handle}; use
  /terminal-list"*, sets the active handle, opens `Dialog::Terminal` and
  focuses it.
- Attach goes through the catalog (`interactive_terminal.rs:525-531`) and
  matches `InteractiveProcessAttached` or
  `InteractiveProcessResyncRequired` (`:534-556`).
- Live output is a **100 ms resume cadence**:
  `TERMINAL_PULL_RENDER_INTERVAL` (`interactive_terminal.rs:888-889`) and
  `service_terminal_output_pull` (`:931-…`) ride the TUI's existing pump
  tick — there is no second timer (`:929-930`).
- Input is a raw `CoreRequest::InteractiveProcessInput` (`:1097-1101`),
  queued and flushed one ordered operation per tick (`:1051`, `:1061`).
- Resize is a raw `CoreRequest::InteractiveProcessResize` (`:1205-1209`).
- The resync answer is typed: `InteractiveResyncReason` is
  `HistoryExpired | CursorAhead | HandleGone | VersionMismatch`
  (`crates/codegg-protocol/src/interactive_process.rs:275-287`), and a
  `gap` chunk carries the oldest retained bytes instead of the requested
  range (`interactive_process.rs:255-270`).

### 3.5 The desktop's structure

- **Panels are data, not a switch.** `PANEL_REGISTRY`
  (`apps/desktop/src/panels/registry.ts:61-117`) is the only panel list; the
  frame renders whatever the registry names
  (`apps/desktop/src/regions/SidePanelFrame.tsx:3`). `PanelId` is
  `'tasks' | 'subagents' | 'activity' | 'session' | 'editor'`
  (`registry.ts:29`).
- **A panel holds no authority.** The editor panel is a three-line wrapper
  (`apps/desktop/src/panels/EditorPanel.tsx:16-20`); its state lives above
  it, in `useEditorSession` (`apps/desktop/src/editor/session.ts:57`).
  This milestone follows the same shape: the terminal's state lives in a
  session hook, and the panel renders props.
- **The renderer has no timer.** The projection arrives by subscription
  callback; there is no `setInterval` in the renderer today. The terminal's
  resume cadence is therefore **new**, and §7's work package C places it
  deliberately.
- **The bridge is one reviewed surface.** `apps/desktop/src/bridge.ts:1`
  imports `invoke` and `Channel`; every method is a named
  `invoke<T>('desktop_…', { …, routeGeneration })`. The streaming precedent
  is `desktop_document_diagnostics_subscribe`
  (`bridge.ts:126-128`, host side `apps/desktop/src-tauri/src/document.rs:1337`).
- **The host holds one daemon client.** `route_request()` returns a clone of
  the single `codegg_client::LocalSocketClient`
  (`apps/desktop/src-tauri/src/route.rs:254-270`). Every interactive-process
  operation must therefore travel over **that** connection (§8).
- **The action dispatch drops the argument.** `CommandPalette.tsx:183-190`
  calls `onFrontendEffect?.(command, shape)` without the typed argument,
  and `answerFrontendAction(action, context)` takes no argument
  (`apps/desktop/src/commands/frontendActions.ts:179-182`). The catalog
  form requires one: `argument_hint: "<handle>"`, `argument_required: true`
  (`crates/codegg-core/src/command_catalog.rs:2498-2503`).

### 3.6 The desktop's honest notice — what this milestone retires

`apps/desktop/src/commands/frontendActions.ts:67-68`:

```ts
export const UNCARRIED_ACTIONS: Readonly<Record<string, string | undefined>> = {
  show_terminal: 'the desktop has no terminal surface to focus',
};
```

`show_terminal` is the **only** entry. The other five declared actions are
carried (`frontendActions.ts:183-193`).

## 4. Invariants that must not regress

1. **The desktop holds no Tauri permission.** `apps/desktop/src-tauri/capabilities/main.json`
   carries an empty permission list, and the host's capability set stays as
   it is (`apps/desktop/src-tauri/src/lib.rs:620-643`). The terminal changes
   neither.
2. **The renderer holds no machine authority.** No generic request
   passthrough (`scripts/check-desktop-boundary.sh:23-26`), no bridge call
   from the panel layer (G2 of `scripts/check_desktop_panel_authority.py`),
   no Tauri import, no filesystem or process API (G4), and no durable
   frontend-owned store (G5).
3. **The desktop's bridge surface stays a recorded set.** Every new
   `invoke` target is a deliberate addition to
   `RECORDED_BRIDGE_TARGETS`
   (`scripts/check_desktop_panel_authority.py:128`) in the same change that
   adds it, and the bridge and the host's `generate_handler!` table must
   agree with it exactly.
4. **The daemon owns the process.** The desktop never spawns, signals or
   inspects a process; it asks the daemon for the daemon's processes. The
   `interactive_process` family is already daemon-owned
   (`src/core/daemon.rs:4811-4820`).
5. **The bytes are bounded.** Every chunk the renderer receives is bounded by
   the daemon's `MAX_INTERACTIVE_CHUNK_BYTES`
   (`crates/codegg-protocol/src/interactive_process.rs:90`), and the
   renderer's scrollback is bounded by the plan (§5).
6. **A kind is never inferred.** The desktop dispatches the daemon's
   published `action` on the form (`frontendActions.ts:1-27`), never a
   command spelling.
7. **The desktop's command surface stays the daemon's catalog.** The
   renderer may not spell a `/command` name of its own
   (`scripts/check_command_catalog_ownership.py`, `DESKTOP_RENDERER_DIR`).

## 5. Scope

### In scope

- A **host bridge** for the six terminal operations the desktop needs:
  list, attach, resume, input, resize and detach.
- A **renderer terminal surface**: a dockable panel that renders a bounded
  scrollback and forwards the user's input.
- A **first-party bounded VT renderer** — no new runtime dependency
  (§6.1).
- The **`show_terminal` frontend action** becoming performable, with the
  handle argument reaching it.
- A **keymap action** for focusing the terminal, and the terminal's own
  input scope.
- The honest-notice path for `show_terminal` being **removed** from
  `UNCARRIED_ACTIONS`.

### Explicitly out of scope (non-goals and stop conditions)

- **Creating a process.** `InteractiveProcessCreate` exists and is served,
  but the desktop does not create processes. A "new terminal" affordance is
  a **stop condition** requiring its own milestone: it needs a creation
  capability, a working-directory decision and a writer/launcher
  disposition, and none of that is this milestone's wiring.
- **Terminating or removing a process.** `InteractiveProcessTerminate` and
  `InteractiveProcessRemove` are served but are **process-mutating**; they
  need a capability and a writer disposition this milestone does not
  decide. Detaching is in scope; terminating is not.
- **A third-party terminal emulator.** No `@xterm/xterm`, no bundled engine
  of any kind. See §6.1 for why, and §14 for the stop condition that would
  reopen it.
- **Full-screen alternate-buffer applications.** `vim`, `less` and `htop`
  are **not** supported by the bounded renderer. The desktop must say so
  honestly rather than render them as garbage (§6.1).
- **Mouse reporting, 256-colour palettes, and OSC title-setting.** Outside
  the bounded subset in §6.1.
- **A second daemon connection.** The terminal uses the host's existing
  client (§8).
- **Changing the daemon's command catalog.** No `terminal_*` row is added,
  removed or repaired by this milestone. The `terminal_send` defect
  (§3.3) is recorded, not fixed here.
- **The TUI's terminal.** Untouched. This milestone adds a second frontend
  over an existing daemon surface; it does not move, reshape or deprecate
  the TUI's.

## 6. Required production changes

### 6.1 The renderer (the desktop tree)

**A first-party bounded VT renderer, in `apps/desktop/src/terminal/`.** The
daemon publishes **raw PTY bytes** (`InteractiveOutputChunk.data_b64`,
`crates/codegg-protocol/src/interactive_process.rs:255-270`), so the desktop
must interpret escape sequences. The tree has no ANSI or grid library today
(`apps/desktop/package.json` dependencies: `@tanstack/react-virtual`,
`@tauri-apps/api`, `react`, `react-dom`), and this milestone adds none.

*Why not a bundled engine.* The roadmap's stated reason for refusing a
bundled **editor** engine is that adopting one "would change the renderer's
Content Security Policy, its worker model, and the bundle and signing
boundary" (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:154-160`).
The same reasoning applies verbatim to a bundled **terminal** engine, and
M007-E already set the precedent by delivering a first-party editor surface
over the shared document foundation. A first-party renderer keeps the
renderer's CSP local-only, adds no worker, no wasm, and no new supply-chain
surface, and keeps the milestone free of an ADR.

*The bounded subset.* The renderer implements exactly:

- printable bytes written at the cursor;
- `CR` (carriage return), `LF` (line feed), `BS` (backspace);
- `CSI` **erase** — `ED` (`J`) and `EL` (`K`), including the parameter
  variants that clear to end of line / display;
- `CSI` **cursor movement** — `CUU` (`A`), `CUD` (`B`), `CUF` (`C`),
  `CUB` (`D`), `CUP` (`H`);
- `CSI` **SGR** (`m`) for the eight standard colours and the bold
  attribute, mapped to a small class set; other SGR parameters are
  consumed and ignored;
- `CSI` **scroll region** (`r`) is **not** implemented and is consumed;
- `OSC` sequences (`ESC ] … BEL` or `ESC ] … ST`) are consumed and ignored.

Everything else — including `ESC [ ? … h/l` (private modes, including the
alternate buffer) — is **consumed and ignored**, and the renderer records
which unsupported class it last saw so the panel can show an honest notice.

*The scrollback.* A bounded ring of at most **2000 lines** and at most
**256 KiB** of retained text, whichever binds first. When the bound is
reached the oldest lines are dropped and the panel says so. The bound is a
renderer-side bound; the daemon's own retention is separate and larger.

**Files (new):**

- `apps/desktop/src/terminal/vt.ts` — the parser and the grid. Exports a
  `TerminalGrid` with `write(bytes: Uint8Array): void`, `resize(cols, rows)`,
  `lines(): string[]`, `cursor(): { row, col }`, `lastUnsupported(): string | null`.
- `apps/desktop/src/terminal/vt.test.ts` — the parser's unit tests.
- `apps/desktop/src/terminal/contract.ts` — the terminal's bounded bridge
  contract: the TypeScript mirror of the host's view types (§6.2), exactly
  as `apps/desktop/src/editor/contract.ts` mirrors `document.rs`. The
  terminal's DTOs live in their **own** host module
  (`src-tauri/src/terminal.rs`) rather than in the general bridge, and
  `check-bindings.mjs` gains a third source pair for them (§6.2).
- `apps/desktop/src/terminal/session.ts` — `useTerminalSession(options): TerminalPanelProps`,
  the terminal's state holder: the attachment, the cursor, the grid, the
  input queue, the resize, and the resume cadence.
- `apps/desktop/src/terminal/session.test.ts` — the session's unit tests.
- `apps/desktop/src/panels/TerminalPanel.tsx` — the panel. Renders props and
  calls nothing.
- `apps/desktop/src/panels/TerminalPanel.test.tsx` — the panel's tests.

**Files (modified):**

- `apps/desktop/src/bridge-types.ts` — **unchanged**; the terminal's DTOs
  are not bridge DTOs.
- `apps/desktop/src/bridge.ts` — the six bridge wrappers (§6.2).
- `apps/desktop/src/panels/registry.ts` — `'terminal'` added to `PanelId`
  (`:29`), a `TerminalPanel` entry in `PANEL_REGISTRY` (`:61-117`), and
  `terminal: TerminalPanelProps` on `PanelProps` (`:31-49`).
- `apps/desktop/src/App.tsx` — the composition root owns the terminal state
  and the bridge calls (`useEditorSession` is the precedent at `:200`); the
  panel props are passed at `:1251-1261`; the `show_terminal` action gains a
  `case` in `runAction` (`:831-889`).
- `apps/desktop/src/commands/frontendActions.ts` — `show_terminal` leaves
  `UNCARRIED_ACTIONS` (`:67-68`) and gains a `case` in
  `answerFrontendAction` (`:183-200`); the action's **argument** joins
  `FrontendActionContext`.
- `apps/desktop/src/commands/CommandPalette.tsx` — `onFrontendEffect`
  carries the typed argument (`:43`, `:183-190`).
- `apps/desktop/src/keymap.ts` — the terminal actions and their chords in
  `ACTION_DECLARATIONS` (`:178`), with a terminal scope beside
  `EDITOR_SCOPE` (`:160`).
- `apps/desktop/src/focus.ts` — a `terminal` region beside `REGIONS` (`:61-66`).
- `apps/desktop/src/style.css` — the terminal's styles, in the existing
  named-area system (`:120`, `:187-195`).
- `apps/desktop/scripts/check-bindings.mjs` — the new view types in the
  `expected` map (`:21-…`), and the third `sources` pair for the terminal
  module (`:11-20`).
- `scripts/check_desktop_panel_authority.py` — `RECORDED_BRIDGE_TARGETS`
  (`:128`) extended **deliberately** with the six new targets, and the
  count in the comment (`:104-105`) updated.

### 6.2 The host (the Tauri bridge)

Six named commands, each a bounded operation and **never** a generic
request passthrough (`scripts/check-desktop-boundary.sh:23-26`):

| Bridge target | Daemon request | Response |
|---|---|---|
| `desktop_terminal_list` | `CoreRequest::InteractiveProcessList` | `TerminalListView` |
| `desktop_terminal_attach` | `CoreRequest::InteractiveProcessAttach` | `TerminalAttachView` |
| `desktop_terminal_resume` | `CoreRequest::InteractiveProcessResume` | `TerminalChunkView` |
| `desktop_terminal_input` | `CoreRequest::InteractiveProcessInput` | `TerminalInputView` |
| `desktop_terminal_resize` | `CoreRequest::InteractiveProcessResize` | `TerminalResizeView` |
| `desktop_terminal_detach` | `CoreRequest::InteractiveProcessDetach` | `TerminalDetachView` |

*Why direct bridge commands rather than the catalog.* `terminal_list`,
`terminal_attach` and `terminal_resume` **are** catalog-served, so the
desktop could reach them through the existing `desktop_command_execute`.
But `terminal_resize` and `terminal_detach` are **not**
(`daemon_commands.rs:185` has no row), so new host commands are required
regardless. Mixing the two mechanisms would mean two response-unwrapping
paths (`src/tui/command_route.rs:241`) and two authorization paths, and it
would route input through the **broken** `terminal_send` row (§3.3). Six
direct commands keep one mechanism, one ownership story and one unwrapping
path.

*The view types* — in a **new host module** `apps/desktop/src-tauri/src/terminal.rs`,
mirrored by `apps/desktop/src/terminal/contract.ts`:

- `TerminalProcessView` — `handle`, `workspace_id`, `command`, `state`,
  `cols`, `rows`, `next_seq`, `retained_bytes`, `total_bytes`,
  `truncated`, `exit_code`, `exit_signal`. The daemon's metadata also
  carries `child_pid` (`crates/codegg-protocol/src/interactive_process.rs:242`);
  the view **deliberately omits it**, because a process id is a machine
  detail the renderer has no use for.
- `TerminalListView` — `processes`, `truncated`.
- `TerminalChunkView` — `handle`, `from_seq`, `next_seq`, `gap`, `data_b64`.
- `TerminalAttachView` — `attachment_id`, `handle`, `chunk`,
  `resync_reason`.
- `TerminalInputView` — `attachment_id`, `bytes_accepted`.
- `TerminalResizeView` — `attachment_id`, `cols`, `rows`.
- `TerminalDetachView` — `attachment_id`.

*Files (modified):*

- `apps/desktop/src-tauri/src/terminal.rs` — **new module**: the view structs
  and the `route_terminal_*` functions, each going through
  `HostState::route_request()` (`route.rs:253-270`, `pub(crate)`) and
  mapping the daemon's response to the view type.
- `apps/desktop/src-tauri/src/lib.rs` — the `mod terminal;` declaration, the
  `#[tauri::command]` wrappers and their registration in
  `tauri::generate_handler![]` (`:1154-1199`).
- `apps/desktop/scripts/check-bindings.mjs` — a third source pair,
  `terminal: { rust: readFileSync(resolve('src-tauri/src/terminal.rs'), 'utf8'), ts: readFileSync(resolve('src/terminal/contract.ts'), 'utf8') }`
  (`:11-20`), and the seven view types in the `expected` map with
  `source: 'terminal'` (`:21-…`).

### 6.3 Protocol

**No change.** Every variant this milestone uses already exists
(`crates/codegg-protocol/src/core.rs:3601-3658`). The recorded bridge
baseline grows from **44** to **50** targets; the protocol does not move.

### 6.4 Storage

**No change.** The terminal holds no durable state. The renderer's scrollback
is in-memory and bounded (§6.1); the panel layer holds no durable
frontend-owned store (G5).

### 6.5 Security

- The desktop opens **no** capability and gains **no** Tauri permission.
- The daemon's interactive family bypasses `authorize_request`
  (`src/core/daemon.rs:4569-4597`); the desktop's commands therefore
  inherit the daemon's connection-scoped ownership, which is the same
  authority the TUI's terminal has.
- The renderer must never name a `desktop_*` bridge target
  (`check_desktop_panel_authority.py` G2) and must never import the bridge
  module. The terminal's panel obeys this exactly as the editor's does.
- The renderer must not hold a **quoted `a:b` literal** — the guard's
  permission-string pattern matches any such literal
  (`check_desktop_panel_authority.py:332`). The VT parser's grammar uses
  `ESC` and `CSI` names, not `a:b` pairs.

### 6.6 Documentation

- `architecture/desktop.md` — the terminal surface, its bounded VT subset
  and its honest limits.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:571` — the
  M007-J status cell.
- `plans/registry.md` — the M007-J row.

## 7. Ordered work packages

### Work package A — The host bridge

Add the seven view types to the **new** module
`apps/desktop/src-tauri/src/terminal.rs`; add the six `route_terminal_*`
functions there too; add the six `#[tauri::command]` wrappers in `lib.rs`
and register them in `generate_handler![]`. Each route maps the daemon's
`CoreResponse` to the view type and turns an unexpected response into a
typed error rather than a silent default. Add the six targets to
`RECORDED_BRIDGE_TARGETS` in the same change. Add the `terminal` source
pair and the seven expected entries to `check-bindings.mjs`.

### Work package B — The bounded VT renderer

Write `apps/desktop/src/terminal/vt.ts` to §6.1's subset, with its unit
tests. The parser is pure: bytes in, grid out. It never touches the bridge,
the DOM or the filesystem.

### Work package C — The terminal session

Write `useTerminalSession`. It owns:

- the **attachment** (`attachment_id`, `handle`, `next_seq`);
- the **grid**, fed by the chunks the resume cadence returns;
- the **resume cadence**: a single `setInterval` at **100 ms**, matching the
  TUI's `TERMINAL_PULL_RENDER_INTERVAL`
  (`src/tui/commands/interactive_terminal.rs:888-889`), running **only**
  while an attachment is live, and torn down on detach, on unmount and on
  route change. The renderer has no other timer, so this is the only one;
  it must not be created per panel instance.
- the **input queue**: keystrokes are queued and flushed in order, one
  bounded write per tick, mirroring the TUI's
  `queue_terminal_input`/`flush_terminal_input`
  (`interactive_terminal.rs:1051`, `:1061`).
- the **resize**: on a panel resize, the session issues one
  `InteractiveProcessResize` with the panel's measured cell size.
- the **resync handling**: a `gap` chunk or a
  `InteractiveProcessResyncRequired` answer resets the grid to the daemon's
  `base_seq` and says so.

### Work package D — The panel and the registry

Write `TerminalPanel.tsx` as a thin renderer of props (the editor panel is
the precedent), add `'terminal'` to `PanelId`, add the registry entry, and
add `terminal` to `PanelProps`. Style it in the existing named-area system.

### Work package E — The action

Extend `onFrontendEffect` to carry the typed argument; thread the argument
into `FrontendActionContext`; give `show_terminal` a `case` in
`answerFrontendAction` that performs the action when a handle resolves and
returns an honest reason when it does not; remove `show_terminal` from
`UNCARRIED_ACTIONS`.

### Work package F — The keymap

Declare the terminal's actions — at minimum a **focus** action — with their
chords and a `terminal` scope. A chord that must not be swallowed by
typing sets `captured: true` (the rule is at
`apps/desktop/src/keymap.ts:130-136`). Update the bindings surface's
expected count if it asserts one.

### Work package G — The composition root

Wire the session into `App.tsx`: the panel props, the `show_terminal` case
in `runAction`, and the route-change teardown.

### Work package H — Tests

Per §10.

## 8. Failure, cancellation, restart, contention semantics

- **A reconnect orphans the attachment.** Ownership is the transport
  connection's client id (`src/core/daemon.rs:4849`) and the host holds
  **one** client (`route.rs:254-270`). On a reconnect the desktop **must
  re-attach** to the handle it was showing, and must never open a second
  connection for the terminal. A re-attach that finds the handle gone is
  reported as `HandleGone` (`interactive_process.rs:275-287`), not as an
  empty terminal.
- **A resync resets the view.** A `gap` chunk means the requested cursor
  predated retention: the returned bytes are the oldest retained bytes
  (`interactive_process.rs:255-270`). The renderer must show them as a
  resynchronized view, not as a continuous stream.
- **A lost attachment is not an error state.** `HandleGone` and
  `CursorAhead` are typed answers (`interactive_process.rs:275-287`), and
  the panel reports them in words.
- **Input is bounded.** The daemon caps input at 32 KiB
  (`interactive_process.rs:93`); the renderer must not queue without bound.
  A queue over the cap is refused locally and reported.
- **Resize is best-effort and idempotent.** A resize to the size the
  process already has is a no-op on the daemon's side.
- **Detach releases the attachment, not the process**
  (`core.rs:3624-3627`). The desktop never terminates a process (§5).
- **The cadence must not leak.** The interval is created when an attachment
  becomes live and cleared when it stops being live, on detach, on route
  change and on unmount. A test asserts that the desktop holds **at most
  one** terminal interval.

## 9. Compatibility and migration

- **No protocol change**, so no daemon and no other client needs to move.
  An older daemon answers an unknown variant with `Error`; the desktop
  reports that as a typed failure rather than as an empty terminal.
- **The recorded bridge baseline grows** from 44 to 50 targets. That is a
  deliberate edit to the guard, not a measured baseline.
- **The desktop's action table changes**: `show_terminal` moves from
  uncarried to carried. The desktop's notice text for it disappears.
- **No storage migration**, no configuration change, and no change to the
  TUI.
- **The desktop's dependency set does not change** — no new package.

## 10. Required tests

### Focused unit tests (renderer)

- The VT parser: each supported sequence in §6.1, in isolation and in
  sequence; a sequence that is unsupported is **consumed** (does not leak
  into the visible text) and recorded.
- The scrollback bound: the ring drops the oldest lines at the bound and
  the panel says so.
- `useTerminalSession`: the cadence is created once and cleared; a chunk
  advances the cursor; a gap chunk resets the view; input is queued in
  order; a resize issues one request with the measured size.

### Focused unit tests (host)

- Each `route_terminal_*` maps its daemon response to its view type, and
  turns an unexpected response into a typed error.
- The attach route's resync answer maps to `resync_reason` and never to an
  empty chunk.

### Integration tests

- Attach → resume → input → resize → detach, end to end over a real
  `LocalSocketClient`, asserting that the daemon's process receives the
  input and that the resize is applied.
- The desktop's `show_terminal` action performs and focuses the named
  handle, and declines honestly for an unknown handle.

### Guard tests

- `python3 scripts/check_desktop_panel_authority.py` — the recorded
  baseline matches the bridge and the host's table; the panel layer holds
  no authority.
- `python3 scripts/check_frontend_action_set.py` — the declared set is
  unchanged and closed.
- `python3 scripts/check_command_catalog_ownership.py` — the desktop
  renderer holds no second catalog.
- `python3 scripts/check-desktop-boundary.sh` — the desktop stays a
  separate workspace with no renderer permission.

### Test-quality guards

Every test body must assert a checkable property or delegate to a named
helper that does (`python3 scripts/check_test_assertion_quality.py`), and
no two bodies may assert the same thing twice
(`python3 scripts/check_test_duplication.py`).

## 11. Required verification commands

```bash
# Desktop renderer and bridge contract
cd apps/desktop
npm run typecheck
npm test
npm run bindings:check
npm run build
cd ../..

# Desktop Rust host (its own Cargo workspace, lockfile and toolchain)
rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib
rustup run 1.90.0 cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings

# The one local entry point that runs the hosted lane's checks
apps/desktop/scripts/verify-desktop.sh renderer
apps/desktop/scripts/verify-desktop.sh host

# Guards this milestone touches
python3 scripts/check_desktop_panel_authority.py
python3 scripts/check_frontend_action_set.py
python3 scripts/check_command_catalog_ownership.py
./scripts/check-desktop-boundary.sh

# Root workspace hygiene (this milestone changes no root Rust source)
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
scripts/verify.sh quick
```

The hosted lane is `.github/workflows/desktop-e2e.yml`; its path filters
include `apps/desktop/**`, `crates/codegg-client/**`,
`crates/codegg-protocol/**` and `src/core/daemon.rs`.

## 12. Documentation updates

- `architecture/desktop.md` — the terminal surface, its bounded VT subset,
  its honest limits, and the six bridge commands.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:571` — the
  M007-J status cell becomes **closed**, with the closure record's link.
- `plans/registry.md` — a M007-J row under
  `## Dependency-ready implementation plans`, and the line's status row.
- `plans/closure/desktop-frontend-ide-foundation/019-status.md` — the
  closure record, in the shape of closures 014 and 015.

## 13. Acceptance criteria

1. The desktop attaches to a daemon-owned interactive process, reads its
   output on the 100 ms cadence, sends it bounded input, resizes it, and
   detaches — all through the six named bridge commands.
2. `/terminal-show <handle>` focuses the named handle in the desktop's
   terminal panel, and the `show_terminal` frontend action **performs**
   rather than declining.
3. `UNCARRIED_ACTIONS` is empty, and the desktop's honest notice for
   `show_terminal` is gone.
4. The desktop holds no Tauri permission, and the panel layer holds no
   bridge authority, no filesystem or process API and no durable store.
5. The recorded bridge baseline is **50** targets, and the bridge and the
   host's `generate_handler!` table agree with it exactly.
6. The renderer holds **no new runtime dependency**, and the bounded VT
   subset's limits are documented.
7. The cadence does not leak: at most one terminal interval exists, and it
   is cleared on detach, route change and unmount.
8. A reconnect re-attaches, or reports `HandleGone` honestly.
9. Every verification command in §11 was run and its outcome is recorded in
   the closure, including the ones that were not run.

## 14. Stop conditions

Stop and report rather than continue when:

- **A bundled terminal engine looks necessary.** The bounded subset in
  §6.1 is the decision this plan makes. If the surface cannot be delivered
  honestly without one, stop and report that the decision needs an ADR —
  do not add the dependency.
- **The daemon turns out not to serve a variant this plan needs.** The plan
  asserts all ten are served (§3.1). If that is false at implementation
  time, stop: the milestone's premise changes and the roadmap row must be
  corrected before the work proceeds.
- **A second daemon connection looks necessary.** It must not be. Attach,
  input and resize travel over the host's **one** client (§8).
- **The panel layer would need a bridge call.** It must not. A panel that
  needs fresh data names a prop; if that is impossible, the design is
  wrong and the work stops.
- **A `desktop_*` target would need to be a generic passthrough.** Every
  command is named and bounded
  (`scripts/check-desktop-boundary.sh:23-26`).
- **The recorded bridge baseline would need to grow by something other than
  the six named targets.**
- **The daemon's `terminal_send` defect (§3.3) would need fixing here.**
  It must not: the desktop does not use that row.

## 15. Closure evidence required

The closure record must carry:

1. The implementation commit or pull request.
2. A requirement-to-evidence matrix over §13's nine criteria, each with a
   `path:line` anchor or a named test.
3. Every command in §11, with its **outcome** — passed, failed, or **not
   run, with the reason**. A command that was not run is not evidence.
4. The guard runs, with their exact output.
5. The bridge-baseline change, before and after, with the guard's proof that
   the recorded set matches the bridge and the host's table.
6. The bounded VT subset as implemented, with the tests that prove each
   supported sequence and each consumed-unsupported sequence.
7. The evidence that the cadence does not leak.
8. The evidence that the panel layer holds no authority.
9. **Known limitations, stated as limitations** — no alternate-buffer
   applications, no mouse reporting, no 256-colour palette, no process
   creation, no process termination.
10. Unresolved findings, classified by severity.
11. A recommendation: **closed**, **conditionally closed**, **corrective
    pass required**, or **blocked**.

## 16. Handoff notes

- **Do not edit the guard to make it pass.** If
  `check_desktop_panel_authority.py` reports a bridge target that is not
  recorded, the correct fix is the deliberate edit to
  `RECORDED_BRIDGE_TARGETS` that §6.1 already specifies — and nothing
  else. The guard's baseline is a recorded set on purpose.
- **The desktop is a separate Cargo workspace.** Its host pins **Rust
  1.90.0**; the root package's MSRV is 1.89. Run the host's checks with
  `rustup run 1.90.0`.
- **`apps/desktop/` is outside the comment budget's scope**
  (`scripts/check_comment_budget.py:70` scopes it to `src`, `crates` and
  `tests`). This milestone adds no root Rust source, so the budget is
  untouched.
- **`npm run bindings:check` must run with `cwd = apps/desktop`** — the
  script resolves relative paths (`apps/desktop/scripts/check-bindings.mjs:11-20`).
- **The desktop's renderer holds no timer today.** The 100 ms cadence in
  §6.1 is new and must be the only one.
- **The `show_terminal` action needs the handle.** The catalog form
  requires it (`command_catalog.rs:2498-2503`) and the TUI refuses an
  unknown handle (`interactive_terminal.rs:1612-1637`). The desktop must do
  the same rather than focusing an arbitrary terminal.
