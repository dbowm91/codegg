# Desktop Frontend and IDE Foundation Milestone M007-C — Keyboard-First Input and Mouse Parity

Status: ready for handoff

Repository baseline: `c5d3f4dc4c35ae702a7bbaf14009526295d6ac8a` — the merged tree carrying
the desktop-parity work (`cd9c2fab4 feat(desktop): project the whole session snapshot and
render it`)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Planning basis:

- The roadmap's §7 and its M007 decomposition table name M007-C as the keyboard/mouse-parity line
  of the desktop GUI parity milestone. This plan implements the M007-C row only and must not
  absorb M007-A (build gate), M007-B (chat region and side panel), M007-D (command surface) or
  M007-E (editor surface) scope.
- The TUI is the reference frontend and already ships keyboard-first input with mouse parity
  (`src/tui/terminal.rs:83`, `src/tui/app/mod.rs:8954-9091`). This milestone ports **concepts** to
  the desktop; it does not re-specify the TUI's keymap and does not change the TUI's behaviour.

Long-term requirements:

- `plans/000-long-term-specification.md:564` — "Exact keys remain configurable. The conceptual
  hierarchy and read-only observation behavior are normative." This is the licence for
  concept-parity rather than key-for-key parity, and this plan exercises it explicitly (§1).
- `plans/000-long-term-specification.md:550-562` — the conceptual interaction hierarchy (`Space f`
  project picker, `Enter` open/focus, `gt`/`gT` tab navigation, `Space s` session picker, `Space p`
  collaborators, `Space o` observe, `Space c` project chat, `Space j` project jobs, `Space a` agent
  tree).
- `plans/000-long-term-specification.md:397` — Normal-mode `Space f` SHOULD open a project picker
  following Helix conventions; `plans/000-long-term-specification.md:430` — while observing, the
  main activity panel is read-only and insert-mode input targets the project-chat side panel.
- `plans/000-long-term-specification.md:97` — frontends render projections and must not own
  durable state.
- `plans/002-long-term-roadmap.md:237` — "keyboard and focus-state regression tests" is a required
  test category.
- `plans/001-terminology-and-domain-model.md:5` — terminology is normative for plans, protocol
  types, schemas, architecture docs, tests and **UI labels**. A keymap is a UI-label surface.

Applicable ADRs and architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted) — controlling.
  The desktop renderer holds presentation state and no daemon authority; the desktop's bridge
  surface is a reviewed DTO set, not a raw `CoreRequest` channel.
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted);
  `plans/adrs/ADR-0012-…` (accepted) — the desktop's bridge DTO contract.
- `architecture/desktop.md` — trust boundary, bridge DTO surface, deferred surfaces, and the
  hosted qualification (un-paused at `:154`, green at `:217`).
  `architecture/tui.md` — the TUI's key dispatch authority, focus model, input modes
  and keybinding panel; **read-only for this milestone**. `architecture/overview.md` — the
  desktop entry in the workspace map.
- `plans/registry.md:131` — "TUI editor Helix keybinding parity | closed (M001–M013 and M015–M020
  closed; M014 blocked on ADR-0034)". The TUI's Helix-parity workstream already owns the TUI's own
  keymap; this milestone must not contradict or duplicate it.

Predecessor closure (hard dependency, **not yet satisfied at the baseline**):

- M001…M006-E are closed; their closure records are in `plans/closure/`.
- **M007-A** — `plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`.
  The plan file **is present** in the baseline tree (the directory holds `001`…`009` and
  `011`…`015`), but it is `Status: ready for handoff` and **unimplemented**. Until M007-A
  lands, the desktop tree is compiled by **no** automated gate: `Cargo.toml:6` excludes
  `apps/desktop/src-tauri` from the root workspace, `scripts/verify.sh` contains no `npm`,
  `node` or `apps/desktop` reference (its only desktop step is
  `./scripts/check-desktop-boundary.sh`), and the hosted `e2e` lane — **green** since
  2026-10-24 (`architecture/desktop.md:217`) — is a restored premise, not the desktop Rust
  host's `clippy -D warnings` gate (`architecture/desktop.md:207-210`).
- **M007-B** — `plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`.
  The plan file **is present** in the baseline tree and is likewise `Status: ready for
  handoff` and **unimplemented**. A keymap binds to regions that do not exist yet, so
  M007-C is **not implementable** until M007-B's region model and `data-testid` conventions are
  in the tree. **The desktop-parity work does not satisfy this dependency**: it delivered
  *panel-level* surfaces under `apps/desktop/src/panels/`, and there is still **no region
  model, no region container and no region-level `data-testid`** anywhere in
  `apps/desktop/src/`.

Primary class: capability

---

## 1. Objective

Deliver a **shared, remappable keymap for the desktop renderer**, the **concept-parity mapping**
from the TUI's interaction hierarchy, **platform-idiomatic chords** alongside the TUI's concepts,
and **full mouse operability** — such that every region and every action in the desktop is
reachable by keyboard alone *and* by mouse, with a visible focus indicator at all times.

One bounded outcome: an input model for the desktop renderer. It adds no command surface, no
editor, no protocol variant, and no durable daemon capability.

### 1.1 The modality decision

The owner's goal is "keyboard first semantics borrowing from the TUI, but fully controllable to
those more used to keyboard and mouse or traditional IDE's". That contains a genuine tension,
resolved here explicitly rather than by omission:

| | TUI (today) | Conventional IDE | **Desktop (this milestone)** |
|---|---|---|---|
| Model | Modal — `Insert` ↔ `Normal` (`src/tui/input.rs:66-71`) | Direct manipulation | **Direct manipulation** |
| A chord | Acts in the current mode | Acts immediately | **Acts immediately** |
| `Esc` | Mode switch (Insert ↔ Normal) | Cancels the current overlay | **Cancels one overlay layer** |
| Plain letter keys | Mode-dependent verbs | Typed text | **Typed text, always** |

**Decision: the desktop's default state is direct manipulation, not modal.** There is no
Insert/Normal mode in the desktop; every declared chord acts immediately, in any surface, without
a mode switch.

**Rationale.** The canonical documents make the *conceptual hierarchy* normative and the *exact
keys* configurable (`plans/000-long-term-specification.md:564`); modality is one implementation of
the TUI's key semantics, not a required concept. A conventional IDE user expects `Cmd/Ctrl+S`,
`Cmd/Ctrl+P` and `Cmd/Ctrl+Shift+P` to act the moment they are pressed, so a modal desktop would
make every one of them mode-dependent — the opposite of the stated goal. Modality also changes
**every** screen: a plain letter key that means "type" in one mode means "verb" in the other, so
the renderer's affordances would have to declare a mode. Finally, the TUI's modal design is
correct for a terminal, where `Esc` as a mode switch is cheap and the mouse is unreliable; a
desktop WebView has a real mouse, real focus rings and real accelerator conventions.

**What the TUI's concepts become.** `Space f`, `Space s` and `gt`/`gT` are Helix *gestures*, not
*concepts*. The concepts they carry — a project picker, a session picker, activating the focused
row, navigating between surfaces — are ported as **named actions with platform-appropriate default
bindings**, not as the same bytes (§7, WP-2). `Space f` becomes "open the project switcher", bound
to `mod+p`, because a bare `Space` in a WebView is a character in the composer.

**Platform-idiomatic chords are added alongside the TUI's conceptual chords, not instead of
them.** Both spellings of one idea coexist in the catalog; where a concept has a TUI-flavoured and
an IDE-flavoured spelling, the IDE-flavoured one is the default and the TUI-flavoured one is a
documented secondary binding. "Toggle the side panel" is the TUI's `ToggleSidebar`
(`src/tui/input.rs:920`, `Ctrl+B` in the TUI) and becomes `mod+b` on the desktop.

**A modal layer is a recorded, deliberate deferral, not a silent omission.** Helix-style modal
behaviour in the desktop is a separate decision with its own ADR-shaped record: it changes every
screen, conflicts with direct manipulation, and needs its own concept-parity table, affordance
rules and mode indicator. This milestone does not take that decision, and §14 makes "the modality
question appears to need deciding beyond what this plan states" a **stop condition** — the
implementing agent reports rather than picks. The TUI's modal behaviour is not the desktop's
model, and this milestone does not change the TUI's (`src/tui/input.rs:66-71`,
`architecture/tui.md:6406-6411`).

### 1.2 What this milestone is *not*

The desktop today has **no keyboard input at all** (§3.1). This milestone creates the input model;
it does not create the surfaces the input model drives beyond what M007-B already delivers. It
does not implement the `/` command surface, the command palette's contents, the editor's own key
handling, or Helix motion (§5, §14).

---

## 2. Why this milestone is ready

**Closed hard dependencies.** M001…M006-E are closed; their closure records are in
`plans/closure/`. `ADR-0010`, `ADR-0011` and `ADR-0012` are accepted and controlling for the
desktop's trust boundary and bridge DTO surface.

**Open hard dependencies — M007-C is not implementable until both land.**

| Dependency | Why it is hard | Evidence |
|---|---|---|
| **M007-A** — desktop build and hosted gate | The desktop tree is compiled by no automated gate today, so "the desktop typechecks and its tests pass" is an unverifiable claim and §15 cannot be satisfied. | `Cargo.toml:6`, `scripts/verify.sh`, `architecture/desktop.md:207-210` |
| **M007-B** — desktop chat region and side panel | A keymap binds to regions. M007-B establishes the region model, the region order, the focus containers and the `data-testid` conventions this milestone's affordance declarations reference. Without it there is nothing to focus and nothing to bind. The desktop-parity work delivered **panel-level** surfaces (`apps/desktop/src/panels/*.tsx`), not regions: there is still **no region model, no region container and no region-level `data-testid`** in the tree. | roadmap §7 M007 decomposition table |

**Both plan files are present in the baseline tree** —
`plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`
and `plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`
both exist, and both are still `Status: ready for handoff` and **unimplemented**. Their
presence is not their closure: M007-C's implementer must confirm **both are closed**
before starting and record the evidence.

**Stable interface dependencies.** The TUI's `ActionKey` set — `pub enum ActionKey` at
`src/tui/input.rs:259`, **61 variants** (§3.2) — is the source vocabulary for the concept-parity
table; and the canonical conceptual hierarchy (`plans/000-long-term-specification.md:550-562`,
with `:564` making exact keys configurable) is the source of the concepts. The TUI's
remap/export/import capability (`src/tui/components/dialogs/keybind.rs`, 782 lines) is the
behavioural model for the desktop's customisation surface; its *terminal UI* is not copied.

**Readiness risks.** The desktop renderer has never had a keyboard handler (§3.1), so there is no
in-tree convention to follow and this milestone establishes one. The hosted `e2e` lane is
**green** (`architecture/desktop.md:217`), but it is a restored premise and **not** a
substitute for M007-A's gate: the desktop Rust host still has no `clippy -D warnings` step and
`scripts/verify.sh quick` still compiles nothing under `apps/desktop/src-tauri`. M007-C
relies on M007-A's gate running locally and in CI.

---

## 3. Current implementation evidence

### 3.1 The desktop renderer has zero keyboard handlers

```
$ grep -rn "onKeyDown\|onKeyUp\|onKeyPress\|addEventListener" apps/desktop/src/
(no output)
```

There is **no** `onKeyDown`, `onKeyUp`, `onKeyPress`, or `addEventListener` for any key event
anywhere under `apps/desktop/src/`, and no `KeyboardEvent` type is named there either. There are
**twelve** `onClick` bindings in `App.tsx` and four more in the panel modules — **16** across
`apps/desktop/src/`. Every interaction in the desktop today is a mouse click on a button.

The whole renderer is still one component: `apps/desktop/src/App.tsx:28` is
`export function App()`, and the entire render is `apps/desktop/src/App.tsx:594-630` — a single
`<main>` of stacked `<section className="panel">` cards (projects at `:599`, workspaces and
sessions at `:601-602`, session creation at `:603`, the composer at `:604`), followed by the
baseline's panel modules composed inline (`:623-628`). There is no region abstraction, no focus
management, no overlay layer, and no keymap.

The renderer's data path is a bridge over the daemon, already structured for M007-B:

- `apps/desktop/src/bridge.ts:22-94` — the whole bridge surface, now **23** distinct
  `desktop_*` invoke targets: `connect`, `snapshot`, `projects`, `projectDetail`,
  `workspaceSelect`, `sessionList`, `sessionOpen`, `sessionCreate`, `promptSubmit`,
  `artifactRead`, `controlRefresh`, `permissionRespond`, `questionRespond`,
  `projectionStart`/`Stop`/`Current`, `projectionSubscribe`, `todoRefresh`, `gitRead`,
  `skillCatalogue`, `subscribeProjection`, `subscribe`, `disconnect`, `unsubscribe`. The
  three the desktop-parity work added are `todoRefresh` (`:53-54`), `gitRead` (`:60-61`) and
  `skillCatalogue` (`:62-63`).
- `apps/desktop/src/bridge-types.ts:284-324` — `SessionPresentationView` already carries `runs`
  (`:298`), `jobs` (`:299`), `subagents` (`:301`), `agentTree` (`:304`), `recentTurns` (`:310`),
  `todos` (`:317`), `pendingPermissions` (`:318`) and `pendingQuestions` (`:319`). **The
  baseline already renders `jobs` and `subagents`** (`App.tsx:626`, `:628`); `runs` and
  `recentTurns` are still received and dropped, and M007-B's tasks panel closes that pair.
- `apps/desktop/src/App.test.tsx:1-4` — vitest with `@testing-library/react`, the `bridge` module
  mocked at `apps/desktop/src/App.test.tsx:6-31`. The renderer's test harness exists and works.

### 3.2 The TUI's bindable action catalog

`pub enum ActionKey` — `src/tui/input.rs:259` — has **61 variants**:

```
Send, Newline, Cancel, NavigateUp, NavigateDown, SwitchAgent, SelectModel, ClearSession,
NewSession, ToggleAgentRun, ToggleSidebar, FocusSidebar, OpenPanelManager, DirectionLeft,
DirectionRight, DirectionUp, DirectionDown, ToggleTasksList, ToggleSection, CloseSession,
Help, FocusPrompt, StashPrompt, RestorePrompt, CopyMessage, CycleModelForward,
CycleModelBackward, ToggleReasoning, ExpandAllDisclosures, Quit, ForceQuit, ExternalEditor,
Backspace, Delete, Left, Right, Home, End, PageUp, PageDown, Search, SearchNext, SearchPrev,
ClearSearch, Command, ToggleTts, StopTts, ToggleFullscreen, TogglePermissionMode, OpenDiff,
GoToTop, GoToBottom, OpenProjectPicker, NextProjectTab, PreviousProjectTab, CloseProjectTab,
NextSubagentTab, PreviousSubagentTab, CloseSubagentTab, ToggleComposerMode,
OpenWorkspaceDashboard
```

The **configurable** subset is `ActionKey::all()` at `src/tui/input.rs:349-407`, which lists
**56** actions; the five it excludes are `OpenPanelManager`, `DirectionLeft`, `DirectionRight`,
`DirectionUp` and `DirectionDown` — routed by the dispatcher rather than remapped.

The desktop's catalog is **derived from** these 61 and is **not** a copy: several TUI actions have
no desktop target (§7, WP-2), and the desktop adds actions the TUI has no equivalent for (region
focus, the keymap surface). One incidental finding the implementer must not "fix" here:
`architecture/tui.md:6424` describes `InputAction` as "61 variants (`src/tui/input.rs:137`)" —
`InputAction` actually begins at `src/tui/input.rs:130` and has 62 variants (it adds `Char`, the
non-remappable text-input action). The 61-variant catalog is `ActionKey`, at
`src/tui/input.rs:259`; the desktop binds against `ActionKey`.

### 3.3 The TUI's keymap: remap, export/import, dispatch, mouse parity, focus, exit

`src/tui/components/dialogs/keybind.rs` — 782 lines — is the behavioural model: `set_bindings`
(`src/tui/components/dialogs/keybind.rs:80`), `get_binding` (`:92`), `start_remap` (`:112`),
`reset_to_defaults` (`:124`), `start_export` (`:131`), `start_import` (`:148`), `apply_import`
(`:153`), `bind_waiting_key` (`:176`), the **conflict rejection** at `:185-195` (a key already
bound to a *different* action is refused with `"<key> is already bound to <action>"`), the
**single-binding replacement** at `:197-207` (rebinding an action removes its previous key), the
conflict state `set_conflict` (`:166`), and the filter hint `HINT` (`:40`).

The TUI persists the map in the shared config file: `KeybindConfig { bindings: HashMap<String,
ActionKey> }` at `src/tui/input.rs:641-644`, surfaced by `codegg-config` as `pub keybinds:
Option<HashMap<String, String>>` at `crates/codegg-config/src/schema.rs:457`. Defaults live in
`DEFAULT_BINDINGS` (`src/tui/input.rs:646-647`), built by `default_bindings_internal()`
(`src/tui/input.rs:782`) and exposed by `pub fn default_bindings()` (`src/tui/input.rs:1160`).
The TUI's **tolerance precedent** matters: a keymap is loaded at `src/tui/app/mod.rs:868-876`,
where a stale or hand-edited entry **drops one slot** — it does not stop the TUI from starting.
The desktop adopts the same posture (§8). The TUI also exposes the keymap as a **panel** —
`SidePanelId::Keybinds` at `src/tui/panels/mod.rs:480` — and its `HelpDialog` is a **searchable**
keybinding list, not a wall of text (`architecture/tui.md:4413-4419`).

**Dispatch.** `src/tui/app/mod.rs:3929` — `pub fn on_key(&mut self, key: KeyEvent)`;
`src/tui/app/mod.rs:5229` — `fn handle_keybind_key(&mut self, key: KeyEvent)`;
`src/tui/app/mod.rs:5322` — `fn save_keybinds(&mut self)`, which copies the dialog's map into
`ui_state.keybinds`. `architecture/tui.md:5207` — "Key dispatch authority: ordered stages, one
owner per chord": the keymap is consulted **after** the modal and dialog stages, so a chord has
exactly one owner at a time.

**The default bindings the desktop's concept-parity table draws on** are enumerated, with their
anchors, in the WP-2 table below — `Send` → `Enter` (`src/tui/input.rs:786`), `Newline` →
`Shift+Enter` (`src/tui/input.rs:785`), `Cancel` → `Esc` (`src/tui/input.rs:793`),
`ToggleSidebar` → `Ctrl+B` (`src/tui/input.rs:920`), `FocusSidebar` → `Ctrl+Alt+S`
(`src/tui/input.rs:946`), `ToggleTasksList` → `Ctrl+T` (`src/tui/input.rs:916`), `ToggleAgentRun` →
`Ctrl+Alt+X` (`src/tui/input.rs:812`), `CycleModelForward` → `Ctrl+P`
(`src/tui/input.rs:997`), `CycleModelBackward` → `Ctrl+Alt+P` (`src/tui/input.rs:1007`),
`TogglePermissionMode` → `Shift+Tab` (`src/tui/input.rs:879`), `Help` → `Space` / `?`
(`src/tui/input.rs:924`, `src/tui/input.rs:986`).

**Mouse parity.** `src/tui/terminal.rs:83` — `execute!(stdout(), EnableMouseCapture)`: mouse
capture is on by default, in the same setup sequence as raw mode and bracketed paste. The mouse
handler is `src/tui/app/mod.rs:8954-9091`, and every `MouseEventKind` is handled:
`Down(_) | Drag(_) | Up(_)` (`src/tui/app/mod.rs:8990`), `Down(MouseButton::Left)` (`:8995`, focus
+ selection anchor), `Drag(MouseButton::Left)` (`:9000`), `Up(MouseButton::Left)` (`:9004`),
`Up(btn)` (`:9030`), `Moved` (`:9041`, hover), `ScrollUp` (`:9082`), `ScrollDown` (`:9085`),
`ScrollLeft` (`:9088`), `ScrollRight` (`:9091`). `architecture/tui.md:785` — "Side panels are mouse
surfaces like the buffer"; `architecture/tui.md:4447`, `architecture/tui.md:4459` — keyboard
navigation and mouse hit-testing resolve the same target. So the desktop's target is **not**
"keyboard only" and **not** "mouse with shortcuts bolted on": it is the TUI's shape, where every
action has a keyboard route *and* a mouse route, and the two are the same action.

**Focus and cancel.** `architecture/tui.md:4871` — `FocusManager`
(`src/tui/components/component/focus.rs`); `architecture/tui.md:4884-4886` — `Tab` and `Shift-Tab`
wrap against the current component count, and the focused element is tracked and restored.
`architecture/tui.md:4912` — the section heading "Esc never quits; exiting is explicit";
`src/tui/input.rs:787-788` — the source comment: "Ctrl+C is the unconditional exit; Esc only
backs out of whatever is on screen and never terminates the app." `architecture/tui.md:4943-4945`
— "`Ctrl+C` and `Esc` are distinct `InputAction`s … so the back-out key and the terminate key
cannot drift into the same behavior again."

**Exit arming.** `architecture/tui.md:4930-4937` — exiting is **two deliberate presses**: `q` and
`Ctrl+C` each need a second press of the same key inside three seconds. `EXIT_ARM_WINDOW` is
`std::time::Duration::from_secs(3)` at `src/tui/app/state/prompt.rs:126`; the arming logic is
`src/tui/commands/exit_control.rs:16`, `src/tui/commands/exit_control.rs:32`,
`src/tui/commands/exit_control.rs:45`. `architecture/tui.md:4932-4933` — the first
`Ctrl+C` clears the field being typed into *and* arms the exit; the row above the composer names
the key to press again.

### 3.4 The desktop's exit path, boundary and toolchain

The desktop has **no renderer-initiated exit**: the window's close and destroy are handled by the
Rust host (`apps/desktop/src-tauri/src/lib.rs:1084`,
`tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed`, and
`apps/desktop/src-tauri/src/lib.rs:1128`, `tauri::RunEvent::ExitRequested { .. } |
tauri::RunEvent::Exit`), and there is no `desktop_quit` command in `apps/desktop/src/bridge.ts`.
Consequence: **the desktop has no exit chord**, and this plan does not add one — `mod+q` would
shadow the OS's own quit accelerator, which §4 forbids without a deliberate override rule and
§14 makes a stop condition. The two-press rule (`EXIT_ARM_WINDOW`) is therefore **carried, not
implemented**: it applies to the native window-close path the OS already owns, and any *later*
milestone that adds a renderer-initiated exit must adopt it. `Esc` — the desktop's cancel key —
never exits.

The boundary and toolchain:

- `architecture/desktop.md` "Trust boundary" — the WebView renders bundled local assets, the only
  Tauri capability is a main capability with **no permissions**, and the renderer does not reach
  the daemon transport directly.
- `scripts/check-desktop-boundary.sh:19` — the main capability must not contain
  `shell|filesystem|fs:|http:default|process:|updater|clipboard|global-shortcut`;
  `:23` — `core_request|read_file|invoke.*shell` must not appear in `apps/desktop/src` or
  `apps/desktop/src-tauri/src`; `:92` — the capability's `permissions` array must be empty.
- `Cargo.toml:6` — `exclude = ["crates/eggwork-test-node", "apps/desktop/src-tauri"]`;
  `Cargo.toml:12` — the root workspace is `rust-version = "1.89"`.
- `apps/desktop/src-tauri/Cargo.toml:6` — `rust-version = "1.90"`; `:8-10` — it declares its own
  `[workspace]`; `:20` — `tauri = { version = "2.12" }`.
- `apps/desktop/package.json` — `typecheck` (`tsc --noEmit` plus the e2e tsconfig), `test`
  (`vitest run`), `bindings:check` (`node scripts/check-bindings.mjs`), `build`, `dev`, `tauri`,
  `stage:daemon`, `test:e2e`. `apps/desktop/scripts/check-bindings.mjs:7-38` compares a
  **fixed** DTO field map between `src-tauri/src/bridge.rs` and `src/bridge-types.ts`, so a
  milestone that adds no bridge DTO leaves this check trivially green.

### 3.5 The canonical documents say nothing about the mouse

`grep -ic` over `plans/000-long-term-specification.md`, `plans/001-terminology-and-domain-model.md`
and `plans/002-long-term-roadmap.md`:

| Term | `000` | `001` | `002` |
|---|---|---|---|
| `mouse` | 0 | 0 | 0 |
| `pointer` | 0 | 0 | 0 |
| `click` | 0 | 0 | 0 |
| `drag` | 0 | 0 | 0 |
| `accessibility` | 0 | 0 | 0 |
| `keymap` | 0 | 0 | 0 |
| `chord` | 0 | 0 | 0 |
| `sidebar` | 0 | 0 | 0 |
| `focus` | 4 | 0 | 4 |
| `panel` | 2 | 0 | 5 |

**That absence is itself the finding.** Mouse semantics are entirely unconstrained canonically, so
this milestone's mouse contract — the dual-reachability rule, the affordance classes, the focus
indicator — is an **interim decision owned by this roadmap**, not a canonical requirement. The same
is true of *keymap* and *chord*: no canonical term exists, so this plan names them descriptively
and claims no canonical status for them.

*Focus* **does** occur canonically in the UI-focus sense — `plans/000-long-term-specification.md:397`
("creates or focuses a project tab"), `:554` ("open/focus project tab"), `:559` ("focus project
chat") and `plans/002-long-term-roadmap.md:386` ("chat-input focus routing") — so this plan uses
*focus* freely and treats the canonical *focus*/*panel* vocabulary as authoritative.

### 3.6 The TUI's keybinding-parity workstream is a dependency, not a duplicate

`plans/registry.md:131` — "TUI editor Helix keybinding parity | closed (M001–M013 and M015–M020
closed; M014 blocked on ADR-0034)"; `plans/registry.md:48` — "TUI editor and files panel Helix UX
improvement | closed (M001–M020)"; and
`plans/subsystems/tui-editor-correctness-usability-roadmap.md` — the TUI editor's own correctness
and usability roadmap (semantic fidelity C001, viewport/tree work C002, with M003 and M004 gated).
That workstream owns the **TUI's** keymap and Helix motion. This milestone ports **concepts** to
the desktop and must not re-specify the TUI's keymap, change the TUI's behaviour, or contradict the
registered TUI rows.

---

## 4. Invariants that must not regress

| # | Invariant | Evidence |
|---|---|---|
| I1 | The renderer holds **no durable state** and no daemon authority. A keymap is presentation state (like focus and selection), not session state. No `CoreRequest` variant, no protocol change, no storage change, no migration. | `plans/000-long-term-specification.md:97`; `ADR-0010`; `architecture/desktop.md` "Trust boundary" |
| I2 | The desktop boundary guard's **meaning** is unchanged: the main capability still has no permissions, and the renderer still holds no machine authority. The keymap introduces no capability. | `scripts/check-desktop-boundary.sh:19`, `:23`, `:92` |
| I3 | The desktop's **workspace stays separate**: `apps/desktop/src-tauri` remains excluded from the root Cargo workspace, and the desktop's Rust host keeps its own toolchain. | `Cargo.toml:6`; `apps/desktop/src-tauri/Cargo.toml:6`, `:8-10` |
| I4 | The **TUI's keymap and behaviour are unchanged**. No `src/tui/input.rs`, `src/tui/components/dialogs/keybind.rs` or `src/tui/app/mod.rs` edit. | §7 WP-1; `plans/registry.md:131` |
| I5 | **Focus is always visible.** A keyboard-first surface that does not show focus is not keyboard-first. The focused region and the focused control must be visibly indicated, for mouse users as well as keyboard users. | §7 WP-4; `plans/002-long-term-roadmap.md:237` |
| I6 | **No chord may shadow a browser or OS accelerator without a deliberate override rule.** Every chord that collides with a WebView or OS accelerator is either captured with an explicit `preventDefault()` and a stated purpose, or explicitly reserved and left unbound. A chord with no purpose is not bound. | §7 WP-2; §14 |
| I7 | The desktop's shortcuts are **in-window only**. No OS-global shortcut is registered, and `global-shortcut` never enters the Tauri capability. | `scripts/check-desktop-boundary.sh:19` |
| I8 | The desktop's **bridge DTO surface** is unchanged. `npm run bindings:check` stays green against the same fixed field map. | `apps/desktop/scripts/check-bindings.mjs:7-38` |
| I9 | The canonical documents are **not edited** by this milestone, and no accepted ADR is edited. | §12 |

---

## 5. Scope

### 5.1 In scope

1. **A keymap data structure owned by the renderer** (TypeScript): a declaration table of actions
   (identity, label, description, default bindings, mouse affordance); a normalised chord
   representation with a platform-neutral `mod` token; persistence across a renderer reload; a
   conflict check that refuses a colliding remap; a reset-to-defaults path; and an export/import
   document, mirroring the TUI's capability without copying its terminal UI.
2. **A concept-parity table**: TUI concept → desktop action identity → default binding(s) → mouse
   affordance. The TUI's `ActionKey` names and the canonical conceptual hierarchy are the source.
   A canonical concept with **no** desktop surface is recorded as *not carried*, with the reason —
   it is not silently dropped.
3. **A bindings/customisation surface** reachable by keyboard **and** by mouse, mirroring the
   TUI's `SidePanelId::Keybinds` (`src/tui/panels/mod.rs:480`).
4. **Focus management and a visible focus indicator**: a region focus order, in-region focus
   traversal, overlay focus trapping, and focus restoration when an overlay closes.
5. **Escape/cancel semantics** for overlays: `Esc` cancels exactly one layer and never exits.
6. **Mouse operability parity**: every declared action has a visible affordance and a keyboard
   route, and every affordance has a keyboard route.
7. **Renderer tests**: keymap resolution, conflict detection, persistence, focus order, and a
   dual-reachability test over the declared action set.

### 5.2 Out of scope — explicit non-goals

These are **stop conditions**, not deferrals to be quietly absorbed later:

| Non-goal | Why | Anchor |
|---|---|---|
| The `/` command surface and the command palette's **command contents** | M007-D's (not in the baseline tree). ADR-0052, the decision that owns the command surface, was **accepted 2026-10-24**, so M007-D is no longer blocked — it is simply not this milestone's scope. M007-C may **reserve** the palette's chord but must not implement command execution. | roadmap §7 M007 decomposition table; `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` (accepted 2026-10-24) |
| The editor's own key handling and Helix motion | M007-E's concern for the editor surface; the TUI's Helix parity workstream already owns the TUI side. | `plans/registry.md:131`; `plans/subsystems/tui-editor-correctness-usability-roadmap.md` |
| Any change to the TUI's keymap or behaviour | I4. | §3.6 |
| Any Tauri global-shortcut capability in the renderer | `scripts/check-desktop-boundary.sh:19` forbids `global-shortcut` in the capability. The desktop's shortcuts are **in-window**, not OS-global. | I7 |
| Any new `CoreRequest` variant, protocol change, storage change or migration | I1. | `plans/000-long-term-specification.md:97` |
| Any new renderer capability widening the renderer trust boundary | No shell, filesystem, process, HTTP, clipboard or updater plugin. | `scripts/check-desktop-boundary.sh:19`, `:23` |
| A clipboard surface (message copy) | Would require the forbidden clipboard permission, or a `navigator.clipboard` write the milestone has no need for. `ActionKey::CopyMessage` is therefore **not carried**. | §7 WP-2 |
| A renderer-initiated exit and its chord | Would shadow the OS quit accelerator and needs a host command. | §3.4 |
| An in-app find surface | No target exists in M007-C; `ActionKey::Search` is therefore **not carried**. | §7 WP-2 |
| A modal (Insert/Normal) layer | A recorded deferral requiring its own decision. | §1.1 |

### 5.3 Terminology note

The canonical vocabulary that applies is used exactly: *session*, *project*, *workspace*, *agent
tree*, *collaborators*, *project chat*, *project jobs*, *turn*, *run*, *job*, *subagent*,
*permission*, *question*. There is **no canonical term** for *keymap*, *chord*, *sidebar* or
*overlay* (§3.5), so this plan names them descriptively and claims no canonical status for them.
If the keymap's UI labels introduce vocabulary `plans/001-terminology-and-domain-model.md` does
not cover, the terminology amendment is a **separate, owner-directed step** and is **not performed**
by this milestone (§12).

---

## 6. Required production changes

### 6.1 Frontend (renderer) — the bulk of the change

New modules under `apps/desktop/src/`:

| Module | Responsibility |
|---|---|
| `keymap.ts` | The action declaration table, the chord normaliser, the platform resolver, the effective-map builder, the conflict check, the reset path, the export/import document shape, and the `localStorage` persistence adapter. |
| `keymap.test.ts` | Focused unit tests (§10.1). |
| `KeybindingsPanel.tsx` | The customisation surface: a searchable action list, a remap capture, a conflict message, reset-to-defaults, export and import. |
| `KeybindingsPanel.test.tsx` | Focused component tests (§10.2). |
| `focus.ts` | The region registry, the focus order, the in-region traversal, the overlay focus stack, and the focus-indicator application. |
| `focus.test.ts` | Focused unit tests for focus resolution. |

Modified under `apps/desktop/src/`: `App.tsx` — install the single global keydown listener (the
renderer's only key entry point), register the regions and overlays with `focus.ts`, render the
keybindings panel, and add the missing mouse affordances (§7 WP-5); `App.test.tsx` — the
integration tests (§10.3). The renderer owns the keymap because it is the only surface that
receives key events, and the keymap is renderer state — consistent with I1.

### 6.2 Runtime (Tauri host) — **no change**

The keymap persists in the renderer's own WebView storage (`localStorage`, namespace
`codegg.desktop.keymap.v1`). The Rust host gains **no** command, **no** capability, **no** module
and **no** filesystem write. Consequences: `apps/desktop/src/bridge.ts` and
`apps/desktop/src/bridge-types.ts` are unchanged, so `npm run bindings:check` compares the same
fixed field map and stays green (I8); and `apps/desktop/src-tauri/src/lib.rs` is unchanged, so
`cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml` has nothing new to compile.

The `localStorage` choice is deliberate. The TUI's analogue is config-file-backed
(`crates/codegg-config/src/schema.rs:457`), but sharing that file would be wrong for three
reasons: the desktop's action identities and defaults differ from the TUI's (direct manipulation
plus platform-idiomatic chords versus the TUI's modal map); one user's TUI remap must not
silently change the desktop; and reading a config file from the renderer would require a
filesystem capability the boundary forbids.

**If `localStorage` proves not to survive a renderer reload**, the persistence surface must move
to the host. That is a scope decision, not an implementation detail: it adds a bridge command and
therefore a DTO change, which §14 makes a **stop condition**. The implementer reports; it does
not add a host command unilaterally.

### 6.3 Protocol — **NONE**

No `CoreRequest` variant, no protocol version bump, no wire DTO change, no `PROTOCOL_VERSION`
change, no `STORAGE_LAYOUT_VERSION` change, no migration. The desktop's bridge already carries
every field the keymap needs — none.

### 6.4 Security

- The keymap document is **data, never code**: parsed as JSON and validated against the declared
  action set; unknown action ids are dropped, unknown chord strings are kept but inert, and no
  value from the document reaches an `invoke`, a URL, a shell, or the DOM as markup.
- The remap capture reads a `KeyboardEvent`'s key and modifiers only; it never reads
  `event.target`'s value into the document.
- Import is **atomic**: a malformed or conflicting document is rejected whole; a partially
  applied import is impossible.
- The renderer gains **no** Tauri permission. `scripts/check-desktop-boundary.sh` must remain
  green, and the guard's `global-shortcut` and `clipboard` patterns must not appear in
  `apps/desktop/src`.
- The renderer performs **no** clipboard write. Export is a selectable `<textarea>` the user
  copies with the OS's own copy command, so `mod+c` stays OS-owned and unbound.

### 6.5 Documentation

See §12.

---

## 7. Ordered work packages

### WP-1 — Freeze the boundary (no code)

**Intent.** Record, before any code, that the TUI's keymap is not touched and that the desktop's
action catalog is derived, not copied.

**Required changes.** A comment header in `keymap.ts` naming `src/tui/input.rs:259` (`ActionKey`,
61 variants) as the source vocabulary and `plans/000-long-term-specification.md:564` as the
licence for concept-parity; and a note that `ActionKey::all()` (`src/tui/input.rs:349-407`, 56
actions) is the *configurable* subset, the five excluded variants (`OpenPanelManager`,
`Direction*`) being desktop *concepts* the desktop maps onto its own regions rather than
remappable TUI actions.

**Acceptance evidence.** `git diff --name-only` shows no `src/tui/` file touched.

### WP-2 — The action catalog and the concept-parity table

**Intent.** The single source of truth for the desktop's actions: identity, label, description,
default bindings, mouse affordance, and the TUI concept each one carries. The catalog is the
union of (a) a selected subset of `ActionKey`, (b) the canonical concepts at
`plans/000-long-term-specification.md:550-562`, and (c) desktop-only actions the TUI has no
equivalent for.

**The concept-parity table.**

| Canonical concept (`plans/000-long-term-specification.md:550-562`) | Carried in M007-C? | Desktop action identity | Default binding | Mouse affordance |
|---|---|---|---|---|
| `Space f` — project picker (`:552`, `:397`) | **yes** — the desktop already renders a project list (`apps/desktop/src/App.tsx:599`) | `project.switcher.open` | `mod+p` | button `project-switcher-button` |
| `Enter` — open/focus project tab (`:553`) | **yes** — as "activate the focused row" | `item.activate` | `enter` | button family `[data-testid^="project-select-"], [data-testid^="session-open-"]` |
| `gt` / `gT` — next/previous project tab (`:554`) | **no** — the desktop has no project-tab set; M007-B's side panel is one region. Recorded, not dropped. | — | — | — |
| `Space s` — project session picker (`:555`) | **yes** — the desktop already renders a session list (`apps/desktop/src/App.tsx:602`) | `session.switcher.open` | `mod+shift+s` | button `session-switcher-button` |
| `Space p` — collaborators/presence (`:556`) | **no** — the desktop's projection DTO carries no presence data (`apps/desktop/src/bridge-types.ts:284-324`) | — | — | — |
| `Space o` — observe selected session (`:557`) | **no** — the desktop has no observe workflow; `plans/000-long-term-specification.md:430` describes the TUI's observer mode | — | — | — |
| `Space c` — focus project chat (`:558`) | **no** — no project-chat surface in the desktop | — | — | — |
| `Space j` — project jobs (`:559`) | **partial** — `jobs` arrives in the projection DTO (`apps/desktop/src/bridge-types.ts:299`) and **the baseline already renders it** in the tasks panel (`App.tsx:628` → `TaskPanel.tsx:42`). The *picker* is the panel itself. | `tasks.toggle` | `mod+t` | button `tasks-toggle-button` |
| `Space a` — agent tree (`:560`) | **partial** — `subagents` arrives (`apps/desktop/src/bridge-types.ts:301`) and **the baseline already renders it** (`App.tsx:626` → `SubagentPanel.tsx:23`) | `tasks.toggle` (same surface) | — | — |

**The TUI actions the desktop carries.**

| TUI `ActionKey` | TUI default | Desktop action identity | Desktop default | Mouse affordance |
|---|---|---|---|---|
| `Send` | `Enter` (`src/tui/input.rs:786`) | `prompt.submit` | `mod+enter` (global), `enter` (composer) | button `prompt-submit-button` (exists) |
| `Newline` | `Shift+Enter` (`src/tui/input.rs:785`) | `prompt.newline` | `shift+enter` (composer) | editing affordance: `prompt-input` textarea |
| `Cancel` | `Esc` (`src/tui/input.rs:793`) | `overlay.cancel` | `escape` | button `overlay-close-button` on every overlay (new) |
| `ToggleSidebar` | `Ctrl+B` (`src/tui/input.rs:920`) | `sidepanel.toggle` | `mod+b` | button `sidepanel-toggle-button` (M007-B) |
| `FocusSidebar` | `Ctrl+Alt+S` (`src/tui/input.rs:946`) | `sidepanel.focus` | `mod+alt+s` | button `sidepanel-focus-button` (M007-B) |
| `ToggleTasksList` | `Ctrl+T` (`src/tui/input.rs:916`) | `tasks.toggle` | `mod+t` | button `tasks-toggle-button` (M007-B) |
| `ToggleAgentRun` | `Ctrl+Alt+X` (`src/tui/input.rs:812`) | `turn.toggle` | `mod+alt+x` | button `turn-toggle-button` (M007-B) |
| `TogglePermissionMode` | `Shift+Tab` (`src/tui/input.rs:879`) | `prompt.planMode.toggle` | `mod+alt+p` | checkbox `plan-mode-checkbox` (exists) |
| `CycleModelForward` / `CycleModelBackward` | `Ctrl+P` / `Ctrl+Alt+P` (`src/tui/input.rs:997`, `:1007`) | `model.cycleForward` / `model.cycleBackward` | `mod+alt+→` / `mod+alt+←` | button `model-cycle-button` (M007-B) |
| `Help` | `Space` / `?` (`src/tui/input.rs:924`, `:986`) | `help.open` | `mod+/` | button `help-button` |
| `OpenProjectPicker` | — | `project.switcher.open` | `mod+p` | button `project-switcher-button` |
| — (desktop-only) | — | `session.create` | `mod+n` | button `session-create-button` (exists) |
| — (desktop-only) | — | `region.focus.1`…`region.focus.9` | `mod+1`…`mod+9` | region affordance: `region-<name>` container |
| — (desktop-only) | — | `keybinds.open` | `mod+k` | button `keybinds-open-button` |
| — (desktop-only) | — | `connection.reconnect` | `mod+alt+r` | button `reconnect-button` (exists) |

**TUI actions deliberately not carried, each with its reason.** `CopyMessage` — no clipboard
surface (§5.2), and the clipboard permission is forbidden by the boundary guard. `Search`,
`SearchNext`, `SearchPrev`, `ClearSearch` — no in-app find surface in M007-C. `Quit`,
`ForceQuit` — the native window close owns exit (§3.4); a chord would shadow `mod+q`.
`StashPrompt`, `RestorePrompt`, `ExternalEditor`, `ExpandAllDisclosures`, `ToggleSection`,
`GoToTop`, `GoToBottom`, `Left`/`Right`/`Home`/`End`/`PageUp`/`PageDown`/`Backspace`/`Delete` —
editor-surface concerns (M007-E). `ToggleComposerMode`, `Direction*`, `OpenPanelManager` —
modal/panel-router concepts with no desktop analogue under direct manipulation (WP-1).
`NextProjectTab`, `PreviousProjectTab`, `CloseProjectTab`, `NextSubagentTab`,
`PreviousSubagentTab`, `CloseSubagentTab` — the desktop has no tab sets. `SwitchAgent`,
`SelectModel` — no desktop surface yet. `CloseSession`, `ClearSession` — no desktop surface
(`NewSession` **is** carried, as the desktop-only `session.create`). `ToggleTts`, `StopTts`,
`ToggleFullscreen`, `ToggleReasoning` — no desktop surface.

**The reserved-chord list.** Recorded, documented, **not bound** in M007-C, so a later milestone
can bind them without a conflict and they are not shadowed by accident:

| Chord | Reserved for | Why not bound now |
|---|---|---|
| `mod+s` | M007-E (editor save) | No editor in M007-C; binding it would capture the WebView's save accelerator with no target (I6). |
| `mod+shift+p` | M007-D (command palette) | The palette's command contents are M007-D's, and ADR-0052's acceptance on 2026-10-24 cleared M007-D's only blocker. Binding the chord to an empty surface would be a dead chord. |
| `mod+f` | a future in-app find | No find surface. |
| `mod+q`, `mod+w` | **never** — the OS | Quitting and closing the window are OS-owned. |
| `mod+c`, `mod+x`, `mod+v`, `mod+a` | **never** — the OS | Copy/cut/paste/select-all are OS-owned and must stay so, including for the keymap export surface. |
| `mod+tab`, `mod+shift+tab` | **never** — the OS | OS application switching. |

**Acceptance evidence.** The declaration table is the only place an action is named; the parity
table and the reserved list are both derived from it. A test asserts that no bound chord appears
in the reserved list and that no two actions share a chord (§10).

### WP-3 — The keymap structure, resolution and persistence

**Intent.** A renderer-owned, remappable, persisted keymap with a conflict check and a reset
path.

**Required changes.**

- **Chord normalisation.** A normalised chord string: modifier tokens in a fixed order (`mod`,
  `ctrl`, `alt`, `shift`) followed by the key token, lowercased (`mod+shift+p`, `shift+enter`,
  `escape`, `enter`, `f1`, `arrowright`). `mod` resolves to `metaKey` on macOS and `ctrlKey`
  elsewhere, so one declaration serves both platforms. Resolution is deterministic and total: an
  unrecognised key event normalises to a chord that matches nothing.
- **The declaration table.** `ActionDeclaration { id, label, description, defaultBindings,
  affordance, tuiConcept? }`. `label` and `description` are **UI labels**, so
  `plans/001-terminology-and-domain-model.md:5` applies to them (§12).
- **The effective map.** `resolveChord(event) → Chord`, then a lookup from chord to action. The
  effective map is defaults merged with the user's overrides; an override for an action
  **replaces** that action's defaults entirely (the TUI's single-binding rule at
  `src/tui/components/dialogs/keybind.rs:197-207`).
- **The conflict check.** The TUI's map is a `HashMap<(modifiers, code), action>`, so a conflict
  is impossible by construction; the desktop's map is action → chords, so it is possible. The
  effective-map builder therefore **must** detect two actions claiming one chord, and the remap
  path **must** refuse it with the TUI's shape of message
  (`src/tui/components/dialogs/keybind.rs:185-195`): `"<chord> is already bound to <action>"`. A
  refused remap leaves the keymap byte-identical.
- **Persistence.** `localStorage`, key `codegg.desktop.keymap.v1`, value
  `{"version":1,"bindings":{"<action id>":["<chord>"]}}`. A single `setItem` per commit, so a
  reload can never observe a half-applied remap. On load: a missing, malformed, wrong-version or
  unparseable document yields the defaults plus a visible notice; an unknown action id is dropped
  individually. The TUI's tolerance posture applies (`src/tui/app/mod.rs:868-876`): a stale entry
  drops one slot and never blocks startup.
- **Reset to defaults.** Drops the override document entirely and restores the declaration
  table's defaults. Mirrors `reset_to_defaults` (`src/tui/components/dialogs/keybind.rs:124`).
- **Export / import.** A document of the same shape, shown in a read-only `<textarea>` for the
  user to copy with the OS's own copy command, and consumed from an editable `<textarea>` with an
  explicit apply action. Import is atomic (§6.4). Mirrors `start_export`/`start_import`/
  `apply_import` (`src/tui/components/dialogs/keybind.rs:131`, `:148`, `:153`).
- **One key entry point.** A single `keydown` listener installed at the renderer's root, so
  there is exactly one owner per chord — the desktop's analogue of `architecture/tui.md:5207`
  ("ordered stages, one owner per chord"). The listener resolves the chord, checks the overlay
  stack first (an open overlay owns `escape` and the chords it binds), then the focused region,
  then the global map.
- **Accelerator capture.** A chord that collides with a WebView or OS accelerator is marked
  `captured` in the declaration and the listener calls `preventDefault()` on it. A chord that is
  not captured must not call `preventDefault()`, so ordinary typing and native editing are
  untouched.

**Acceptance evidence.** §10.1's tests pass; the conflict test shows a refused remap leaves the
keymap unchanged; the persistence test shows a remap survives a renderer reload.

### WP-4 — The focus model and the visible focus indicator

**Intent.** A deterministic focus model with a visible indicator, so the surface is genuinely
keyboard-first rather than keyboard-*capable*.

**Required changes.**

- **A region registry.** Regions are declared with an order and a stable `data-testid`
  (`region-chat`, `region-sidepanel`, `region-composer`, plus overlays). M007-B's region model is
  the source; this milestone adds the focus layer over it. **The `data-testid` values the
  baseline already creates are panel-level, not region-level** — `task-surface`,
  `subagent-surface`, `activity-surface`, `transcript-surface`, `turn-summary-surface`,
  `tool-detail-surface`, `git-surface` and `skills-surface`
  (`apps/desktop/src/panels/*.tsx`) — so they are **not** the region affordances this
  milestone's `mod+1`…`mod+9` bindings target. The region containers are M007-B's to create.
- **Region focus.** `mod+1`…`mod+9` focuses the *n*-th region directly, moving DOM focus to the
  region's current control.
- **In-region traversal.** `Tab` / `Shift+Tab` move within the focused region and **wrap** at the
  ends — the TUI's rule (`architecture/tui.md:4884-4886`). The implementation uses roving
  `tabindex`: exactly one control in the focused region has `tabindex="0"`, the rest `-1`. The
  desktop's `Tab` never escapes the focused region; region movement is `mod+1`…`mod+9`.
- **Overlays.** An overlay pushes a focus layer that traps focus. `escape` pops exactly one layer
  and restores focus to the element that had it before the overlay opened. This mirrors the TUI's
  `Cancel` (`src/tui/input.rs:793`) and its "Esc only backs out of whatever is on screen" rule
  (`architecture/tui.md:4912`). `escape` on the base view does **nothing** — it never exits, and
  the test asserts that.
- **The visible focus indicator.** The focused region container carries `data-focus-region` and a
  visible outline; the focused control is visibly outlined via `:focus` (**not** only
  `:focus-visible`, because a mouse user must also see where focus is). The indicator must be
  visible against the desktop's dark theme, and the test asserts its presence structurally
  (`data-focus-region` plus a focus-ring class) — a screenshot check is a nice-to-have, not the
  gate.
- **Focus loss.** When the focused region unmounts, focus moves to the region's declared fallback
  (the composer), never to `document.body`. Asserted (§10).

**Acceptance evidence.** §10.3's focus-order and focus-indicator tests pass; the focus-order test
is the "focus-state regression test" category `plans/002-long-term-roadmap.md:237` requires.

### WP-5 — The bindings surface and the mouse affordance sweep

**Intent.** The user can customise the keymap with the keyboard *or* the mouse, and every action
has a visible affordance.

**Required changes.**

- **The bindings surface.** A panel mirroring the TUI's `SidePanelId::Keybinds`
  (`src/tui/panels/mod.rs:480`), reachable by mouse (a `keybinds-open-button`) and by chord
  (`mod+k`). Its shape: a searchable action list (the TUI's `HelpDialog` is searchable —
  `architecture/tui.md:4413-4419`), a selected action, its current binding(s), a remap capture, a
  conflict message, a reset-to-defaults action, and an export/import pair.
- **Keyboard operability of the surface itself.** The surface must be fully operable with the
  keyboard: the action list is navigable, the remap capture is entered and left with the
  keyboard, and the reset/export/import actions have their own bindings. A keyboard user must not
  have to reach for the mouse to remap a binding.
- **The affordance sweep.** For every declared action, add the missing visible affordance. Four
  already exist (`prompt-submit-button`, `session-create-button`, `plan-mode-checkbox`,
  `reconnect-button`); the rest are added by this milestone or by M007-B. Every overlay gains an
  `overlay-close-button` so `overlay.cancel` has a mouse route.
- **The affordance classes.** Each declaration names exactly one affordance and its class:
  `button` — an activatable element, mouse route a click; `region` — a region container, mouse
  route a click on the region, which focuses it; `editing` — an editable element, mouse route to
  click into it and use that element's native editing affordance. Only `prompt.newline` uses the
  `editing` class.
- **Discoverability.** The bindings surface lists every action with its current binding, so the
  keymap is discoverable without memorisation. It is the desktop's equivalent of the TUI's
  searchable keybinding list.

**Acceptance evidence.** The dual-reachability test (§10.3) enumerates every declared action and
asserts both routes; the surface is operable with the keyboard alone.

### WP-6 — The tests

**Intent.** The regression net this milestone installs for every later desktop milestone: all of
§10's tests, in the three files named in §6.1.

**Acceptance evidence.** §11's commands pass, and the test output names each test.

---

## 8. Failure, cancellation, restart and contention semantics

| Situation | Required behaviour | Anchor |
|---|---|---|
| **Renderer reload** | The overrides are re-read from `localStorage`; a remap survives, and the declaration table supplies anything missing. | §7 WP-3 |
| **A stale keymap** (older build, or hand-edited) | Unknown action ids are dropped individually; unknown chords are kept but inert; the stored `version` is checked, and a wrong version is ignored wholesale in favour of the defaults, with a visible notice. The renderer always starts. | `src/tui/app/mod.rs:868-876` |
| **A conflicting keymap** (hand-edited so two actions claim one chord) | Resolved **deterministically**: the later-declared action keeps the chord and the earlier falls back to its default binding. A notice names the conflict and both actions. Startup is not blocked. | §7 WP-3; `src/tui/components/dialogs/keybind.rs:185-195` |
| **A remap is refused** | The keymap is unchanged, byte-for-byte, and the surface shows the conflict message. The refused chord is not partially applied. | `src/tui/components/dialogs/keybind.rs:185-195` |
| **Import of a malformed document** | Rejected **whole**. No partial application. The message names the parse or validation failure. | §6.4 |
| **A chord is pressed with no matching action** | Nothing happens, and the keydown is not swallowed: a chord with no action must not call `preventDefault()`, so typing and native editing are unaffected. | I6 |
| **A chord is pressed while an overlay is open** | The overlay owns the chord; the base view's global map does not fire. `escape` is owned by the overlay stack and pops exactly one layer. | `architecture/tui.md:5207` |
| **`escape` on the base view** | Nothing. It never exits and never clears the composer. | `architecture/tui.md:4912`; §3.4 |
| **The focused region is removed** | Focus moves to the region's declared fallback (the composer). Focus is never left on `document.body`, which would strand a keyboard user with no focus at all. | §7 WP-4 |
| **An overlay closes** | Focus returns to the element that had focus when the overlay opened. | §7 WP-4 |
| **No chord reaches the daemon** | Nothing changes. A chord is a **shortcut for an existing renderer action**; every action it can invoke is already reachable by mouse and already goes through the existing bridge. The keymap cannot become a new capability surface and adds no daemon round-trip of its own. | I1, I7 |
| **Two remaps race** | The second is refused by the conflict check and the first stands, because the conflict check is evaluated against the *current effective map* at commit time, not a stale snapshot. | §7 WP-3 |
| **A remap is committed while an overlay is open** | The overlay keeps focus. The commit updates the map; it does not move focus or close the overlay. | §7 WP-3, WP-4 |
| **The renderer crashes mid-remap** | `localStorage` holds the previous committed state, because a commit is one `setItem`. | §7 WP-3 |

---

## 9. Compatibility and migration

**None required.**

- No `CoreRequest` variant, no protocol version change, no wire DTO change.
- No storage schema change, no `STORAGE_LAYOUT_VERSION` change, no migration.
- No bridge DTO change: `apps/desktop/src/bridge-types.ts` and
  `apps/desktop/src-tauri/src/bridge.rs` are untouched, so `npm run bindings:check` compares the
  same fixed field map (`apps/desktop/scripts/check-bindings.mjs:7-38`).
- The existing render path is unchanged: the daemon's projection still reaches the renderer
  through the same bridge calls, and the same fields are rendered. The keymap adds a dispatch
  layer above the renderer; it does not change what the renderer requests or how it renders it.
- A renderer that has never stored a keymap starts on the defaults, with no notice.
- The TUI is unaffected in every respect, including for a user who has remapped the TUI's
  keymap: the desktop does not read the TUI's keymap, and the TUI does not read the desktop's.

---

## 10. Required tests

All renderer tests live under `apps/desktop/src/` and run with `npm test` (vitest).

### 10.1 Focused unit tests — `keymap.test.ts`

| Test | Asserts |
|---|---|
| `chord_normalisation_is_stable_and_ordered` | Modifier tokens are emitted in a fixed order; `mod+shift+p` and `shift+mod+p` are the same chord. |
| `mod_resolves_to_meta_on_macos_and_ctrl_elsewhere` | The same declaration yields `meta` on macOS and `ctrl` elsewhere. |
| `chord_normalisation_distinguishes_shift_enter_from_enter` | `shift+enter` ≠ `enter`; `escape` ≠ `enter`. |
| `unknown_key_event_normalises_to_an_unmatched_chord` | A key with no declaration matches no action and is not swallowed. |
| `remap_onto_an_occupied_chord_is_refused` | The remap is refused with a message naming the chord and the occupying action, and the effective map is **unchanged**. |
| `remap_replaces_the_actions_previous_binding` | The action holds exactly one binding after a remap (the TUI's rule, `src/tui/components/dialogs/keybind.rs:197-207`). |
| `reset_to_defaults_restores_the_declaration_table` | After a remap, reset restores the defaults exactly. |
| `overrides_survive_a_renderer_reload` | Write an override, tear the component down and mount it again, and the override is still in effect. |
| `malformed_stored_keymap_falls_back_to_defaults_with_a_notice` | The renderer starts on the defaults and a notice is shown (the TUI's tolerance, `src/tui/app/mod.rs:868-876`). |
| `unknown_action_id_in_a_stored_keymap_is_dropped_individually` | The other stored overrides still apply. |
| `stored_keymap_with_a_wrong_version_is_ignored_wholesale` | Defaults apply and a notice is shown. |
| `conflicting_stored_keymap_resolves_deterministically` | The later-declared action keeps the chord; the earlier falls back to its default; a notice names both. |
| `import_of_a_malformed_document_is_rejected_atomically` | The keymap is unchanged and the message names the failure. |
| `export_document_shape_is_the_documented_shape` | `{"version":1,"bindings":{...}}` exactly. |
| `no_bound_chord_appears_in_the_reserved_list` | The reserved chords (`mod+s`, `mod+shift+p`, `mod+q`, `mod+c`, …) are not bound. |

### 10.2 Focused component tests — `KeybindingsPanel.test.tsx`

| Test | Asserts |
|---|---|
| `panel_is_reachable_by_mouse_and_by_chord` | Clicking `keybinds-open-button` opens it; `mod+k` opens it. |
| `panel_is_fully_operable_with_the_keyboard` | The action list is navigable, a remap can be captured and committed with the keyboard, and reset/export/import are keyboard-operable. |
| `remap_flow_updates_the_binding_and_shows_the_conflict` | A successful remap is reflected in the list; a colliding remap shows the conflict message and leaves the binding unchanged. |
| `action_list_is_searchable` | Typing a query filters the list (the TUI's `HelpDialog` precedent, `architecture/tui.md:4413-4419`). |

### 10.3 Integration tests — `App.test.tsx`

| Test | Asserts |
|---|---|
| **`every_declared_action_is_reachable_by_chord_and_by_mouse`** | The dual-reachability test. For every action in the declaration table: a chord exists in the effective map, **and** the declared affordance element exists in the DOM, is visible, and is activatable (or, for the `editing` class, is a focusable editable element). This is the mouse-parity gate and it **fails** when an action is added without an affordance. |
| `every_chord_reaches_exactly_one_action` | No two actions share a chord in the effective map. |
| `mod_digit_focuses_the_nth_region` | `mod+2` focuses the second declared region and moves DOM focus into it. |
| `tab_moves_within_the_focused_region_and_wraps` | `Tab` moves to the next control in the region and wraps at the end; `Shift+Tab` wraps backwards. |
| `escape_pops_exactly_one_overlay_and_restores_focus` | With two overlays open, one `escape` closes one and focus returns to the element that had it before. |
| `escape_on_the_base_view_does_nothing` | No overlay closes, the composer is not cleared, and nothing exits. |
| `focus_is_always_visible` | The focused region carries `data-focus-region` and the focused control carries the focus-ring class. |
| `focus_is_never_left_on_document_body` | After the focused region unmounts, `document.activeElement` is the declared fallback, not `document.body`. |
| `the_renderer_registers_no_os_global_shortcut` | No Tauri global-shortcut capability is used; the guard pattern `global-shortcut` does not appear in `apps/desktop/src`. |
| `no_chord_shadows_an_os_accelerator` | `mod+q`, `mod+w`, `mod+c`, `mod+x`, `mod+v`, `mod+a` are not bound, and a `keydown` on one of them is **not** `preventDefault`-ed. |
| `a_captured_accelerator_is_prevented` | `mod+p` **is** `preventDefault`-ed, so the WebView does not also print. |
| `typing_in_the_composer_is_unaffected` | Ordinary character keys reach the textarea; `enter` in the composer submits and `shift+enter` inserts a newline. |

### 10.4 Focus-state regression test

The focus-order test (WP-4) is this milestone's entry in the **"keyboard and focus-state
regression tests"** category that `plans/002-long-term-roadmap.md:237` requires. It is the test
the next desktop milestone inherits.

### 10.5 Restart, contention, security and migration

- **Restart / recovery** — covered in §10.1 (`overrides_survive_a_renderer_reload`, the malformed
  / wrong-version / conflict cases). There is no daemon restart in scope, because nothing here
  crosses the daemon.
- **Contention** — `remap_onto_an_occupied_chord_is_refused` (the two-remaps race) and
  `a_remap_committed_while_an_overlay_is_open_does_not_steal_focus`.
- **Security / negative** — the keymap document is inert data: an imported document carrying
  unknown ids, unknown chords or unexpected types is rejected or dropped, and no value reaches an
  `invoke`, a URL, or the DOM as markup. `apps/desktop/src` contains no `global-shortcut`,
  `clipboard`, `shell`, `filesystem`, `process`, `http:default` or `updater` token — the same
  patterns the boundary guard checks (`scripts/check-desktop-boundary.sh:19`) — and
  `scripts/check-desktop-boundary.sh` passes.
- **Migration** — **none required.** No migration, no schema change, no version bump (§9).

---

## 11. Required verification commands

### 11.1 Desktop renderer (a separate npm workspace)

```bash
cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
```

`npm run typecheck` is `tsc --noEmit` plus the e2e tsconfig (`apps/desktop/package.json`).
`npm test` is `vitest run`. `npm run bindings:check` is `node scripts/check-bindings.mjs` and
compares the **fixed** DTO field map — it must stay green with **no** change to
`src/bridge-types.ts` or `src-tauri/src/bridge.rs` (§9, I8).

### 11.2 Desktop Rust host (a separate Cargo workspace)

```bash
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
```

This milestone changes **no** Rust host file (§6.2), so this is a no-regression check that the host
still builds. Note the host's own toolchain: `apps/desktop/src-tauri/Cargo.toml:6` declares
`rust-version = "1.90"`, one minor above the root workspace's `1.89` (`Cargo.toml:12`). The host is
**not** a member of the root workspace (`Cargo.toml:6`), so the root toolchain does not build it
and the desktop toolchain does not build the root.

### 11.3 Root workspace

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

`scripts/verify.sh quick` runs rustfmt, the two generated-asset checks, the guard scripts and a
workspace `cargo check`. It also runs `./scripts/check-desktop-boundary.sh` (`scripts/verify.sh`),
which is the guard this milestone's I2/I7 rest on.

### 11.4 The gate `scripts/verify.sh quick` does **not** provide — stated plainly

**`scripts/verify.sh quick` does NOT compile the desktop tree and does NOT run the renderer's
tests.** The script contains no `npm`, `node` or `apps/desktop` reference; its only desktop-related
step is `./scripts/check-desktop-boundary.sh`. `Cargo.toml:6` excludes `apps/desktop/src-tauri`, so
the root workspace `cargo check` never sees the host either. The hosted lane is paused and red
(`architecture/desktop.md:154-198`).

**M007-A installs the gate that closes this** —
`plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`. Until
M007-A is closed, §11.1 and §11.2 are run manually by the implementer and their output is
recorded in the closure record; after M007-A is closed, they run automatically on every change to
the desktop tree. M007-C's closure **requires** M007-A's gate to be green.

### 11.5 If a broad sweep is wanted

```bash
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci
```

This milestone changes no Rust, so a broad sweep is optional. Never use `--all-features`: it drags
in the real-server tests. Storage tests use `isolated_pool()`; this milestone adds none.

---

## 12. Documentation updates

| Document | Change |
|---|---|
| `architecture/desktop.md` | **New section: the desktop's input model.** The keymap structure and where it lives (renderer `localStorage`), the chord normalisation and the `mod` token, the declaration table and the concept-parity table, the reserved-chord list, the focus model and the visible indicator, the affordance classes and the dual-reachability rule, the `Esc`/cancel semantics, and the explicit statement that the desktop's shortcuts are **in-window only** and that no OS-global shortcut exists. Also update "Deferred surfaces" if the keybindings surface is listed there. |
| `architecture/tui.md` | **No change.** The TUI's keymap, focus model and behaviour are unchanged (I4). At most a cross-reference sentence pointing at the desktop's own input model — and only if the doc has a "related frontends" slot that would otherwise mislead a reader into thinking the two keymaps are the same. |
| `architecture/overview.md` | One line in the desktop entry noting that the desktop has its own renderer-owned, remappable input model with mouse parity, and that it is in-window only. |
| `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` | **M007-C row only.** Mark M007-C closed and record the closure evidence pointer. Do not touch the M007-A/M007-B/M007-D/M007-E rows. |
| `plans/registry.md` | Register M007-C in the desktop frontend line; update the row's status once the closure record exists. |
| The M007-C closure record | A new file under `plans/closure/` recording the requirement-to-evidence matrix, the command outcomes, and the proposed terminology (below). |

**Documents this milestone must not edit.** `plans/000-long-term-specification.md`,
`plans/001-terminology-and-domain-model.md` and `plans/002-long-term-roadmap.md` — the canonical
documents (I9). Any **accepted** ADR, including
`plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`. ADR-0052 was *accepted* on
2026-10-24, and an accepted ADR is not this milestone's to edit either. The TUI editor roadmap and the registry's TUI
rows (§3.6).

**Terminology amendment — a separate, owner-directed step.**
`plans/001-terminology-and-domain-model.md:5` makes terminology normative for plans, protocol
types, schemas, architecture docs, tests and **UI labels**, and this milestone's action labels
("Toggle side panel", "Open project switcher", "Toggle tasks") and the terms *keymap* / *chord*
are **UI-label surface**. The milestone therefore **records** the proposed vocabulary in its
closure record and **does not** amend `plans/001-terminology-and-domain-model.md`. Adopting
canonical vocabulary for these terms is an owner-directed step with its own review, because it
changes a normative document. The implementer **does not perform it** and does not treat its
absence as a blocker.

---

## 13. Acceptance criteria

1. **A user can reach every region and every declared action with the keyboard alone**, without
   touching the mouse, from any state the application can be in.
2. **A user can reach every declared action with the mouse**, through its declared affordance, and
   the dual-reachability test enumerates every action and proves both routes.
3. **A user can remap any action**, is told when a chord conflicts, and can reset every binding to
   its default.
4. **A user's remapped keymap survives a renderer reload.**
5. **A user can always see which region and which control has focus**, whether they arrived by
   mouse or by keyboard.
6. **`Esc` cancels the current overlay** — exactly one layer — and never exits the application and
   never clears the composer.
7. **The desktop's shortcuts work only while the desktop window has focus.** No OS-global shortcut
   is registered, and the boundary guard's `global-shortcut` pattern does not appear in the
   renderer.
8. **The TUI's keymap, bindings and behaviour are unchanged** — no `src/tui/` file is modified.
9. **No protocol change, no storage change, no migration and no `CoreRequest` variant landed**;
   `npm run bindings:check` compares the same fixed field map.
10. **The desktop tree is compiled and tested by an automated gate** (M007-A's) on every change to
    the desktop tree, and that gate is green.
11. **The renderer holds no durable state and no new daemon authority**: the keymap lives in the
    renderer, no Tauri permission was added, and the main capability's `permissions` array is
    still empty.
12. **A user can discover the keymap**: the bindings surface lists every action with its current
    binding, and it is reachable by mouse and by chord.

---

## 14. Stop conditions

The implementing agent **stops and reports** rather than deciding any of the following:

1. **The modality question appears to need deciding beyond what this plan states.** If
   implementing the keymap appears to require an Insert/Normal mode — or to require that plain
   keys mean different things in different contexts — stop. §1.1 states the decision (direct
   manipulation) and records the modal layer as a deferral needing its own decision. The agent
   does not pick one.
2. **Implementing a chord appears to require a Tauri capability the boundary forbids** — shell,
   filesystem, process, HTTP, clipboard, updater, or global shortcut.
   `scripts/check-desktop-boundary.sh:19` is the authority. The agent does not widen the
   capability, and does not add a shortcut that needs one.
3. **A chord would shadow an OS or browser accelerator and no override rule exists for it.** The
   reserved list (§7 WP-2) is the only sanctioned set of shadowed chords. A chord outside the
   declaration table that would shadow `mod+q`, `mod+w`, `mod+c`, `mod+x`, `mod+v`, `mod+a`,
   `mod+tab` or `mod+shift+tab` stops the milestone for a decision.
4. **Persistence of the keymap appears to need durable daemon state, a `CoreRequest` variant, or a
   host command.** §6.2 places persistence in the renderer's `localStorage`; moving it to the host
   adds a bridge DTO and a filesystem write, which is a scope decision, not a detail.
5. **The work starts absorbing the command surface (M007-D)** — implementing command execution, a
   working palette, or `/`-style commands — **or editor motion (M007-E)** — editor key handling,
   Helix motions, or editor-specific bindings.
6. **The TUI's keymap would have to change for parity to work.** The desktop's catalog is
   *derived* from `ActionKey`; it never edits it.
7. **The desktop's action catalog cannot be derived from `ActionKey` and the canonical hierarchy
   without inventing canonical vocabulary.** The canonical documents have no term for *keymap* or
   *chord* (§3.5); inventing a canonical term is a terminology amendment and is the owner's step
   (§12).
8. **M007-A's gate or M007-B's region model is not present in the tree.** Both plan files are
   present in the baseline tree and both are still unimplemented; their **presence** is not a
   formality — M007-C cannot be verified without M007-A's gate, and it has nothing to bind
   without M007-B's region model. The desktop-parity work's panel modules
   (`apps/desktop/src/panels/*.tsx`) are **panel-level** surfaces: they do not stand in for a
   region model, a region container or a region-level `data-testid`.
9. **A conflict rule, a focus rule or a focus-order rule cannot be made deterministic.** The
   desktop's conflict resolution, region focus order and in-region traversal must each have
   exactly one answer; a rule with two answers is a defect.
10. **The milestone would expand into another subsystem roadmap** — the TUI's Helix parity, the
    editor's keymap, the command surface, or a second frontend.
11. **The M007 decomposition's ordering is contradicted** — if M007-C's scope appears to require
    M007-D or M007-E to have landed first in a way the roadmap does not record, stop and report
    the ordering conflict.

---

## 15. Closure evidence required

A commit message that says "closed" is **not** closure evidence. The closure record must contain:

1. **A requirement-to-evidence matrix**: every acceptance criterion in §13, mapped to the test
   name or the command output that proves it.
2. **The actual command outcomes** for every command in §11, with the real output — the vitest
   summary, the `tsc` result, the `bindings:check` result, the host build, `cargo fmt --check`,
   `cargo clippy -D warnings`, and `scripts/verify.sh quick`.
3. **Proof the dual-reachability test ran**: the vitest output naming
   `every_declared_action_is_reachable_by_chord_and_by_mouse`, and the count of actions it
   enumerated.
4. **Proof the focus-order test ran**: the vitest output naming the focus-order and
   focus-indicator tests, recorded as this milestone's entry in the "keyboard and focus-state
   regression tests" category (`plans/002-long-term-roadmap.md:237`).
5. **Proof no protocol, storage or migration change landed**: `git diff --stat` showing no change
   to `crates/codegg-protocol/`, `crates/codegg-core/src/session/schema.rs`, `src/protocol*` or
   any migration file; `PROTOCOL_VERSION` and `STORAGE_LAYOUT_VERSION` unchanged; no new
   `CoreRequest` variant.
6. **Proof the bridge DTO surface is unchanged**: `npm run bindings:check` green, and
   `git diff --stat` showing no change to `apps/desktop/src/bridge-types.ts` or
   `apps/desktop/src-tauri/src/bridge.rs`.
7. **Proof the boundary guard still passes**: the `scripts/check-desktop-boundary.sh` output from
   `scripts/verify.sh quick`, plus `git diff --stat` showing no change to the Tauri capability
   file and no `global-shortcut` / `clipboard` token in `apps/desktop/src`.
8. **Proof the TUI is untouched**: `git diff --stat -- src/tui/` is empty.
9. **Proof the renderer's durable-state boundary held**: no new Tauri permission, the main
   capability's `permissions` array still empty, and the keymap's persistence location named in
   the record.
10. **The concept-parity table as shipped**, including the concepts **not carried** and their
    reasons — the table must not have been trimmed to only the rows that were easy.
11. **The reserved-chord list as shipped**, and confirmation that none of them is bound.
12. **The proposed terminology** for the owner: the action labels and the terms *keymap*,
    *chord*, *overlay*, *affordance*, with the note that `plans/001-terminology-and-domain-model.md`
    is **not** amended by this milestone (§12).
13. **The M007-A gate evidence**: M007-A's gate green on the M007-C head (or, if M007-A is not
    yet closed, the manual §11.1/§11.2 output, explicitly labelled as such and reported as an
    open dependency).
14. **The stop conditions that were hit**, if any, with the report that was made and the owner's
    answer.

---

## 16. Handoff notes

**Authority order.** When two sources disagree, the higher one wins:
(1) `plans/000-long-term-specification.md` and `plans/001-terminology-and-domain-model.md`;
(2) accepted ADRs — `ADR-0010`, `ADR-0011`, `ADR-0012`; (3) `architecture/` documents;
(4) `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`; (5) this plan; (6) the TUI's
current behaviour — a *reference*, not an authority over the canonical documents.

**What not to edit.** `src/tui/**` — the TUI's keymap, focus model and behaviour are unchanged
(I4). `plans/000-long-term-specification.md`, `plans/001-terminology-and-domain-model.md`,
`plans/002-long-term-roadmap.md` — the canonical documents (I9). Any accepted ADR.
`crates/codegg-protocol/**`, `crates/codegg-core/src/session/schema.rs`, any migration file — no
protocol or storage change lands here. The TUI editor roadmap and the registry's TUI rows. The
roadmap's rows other than M007-C, and `plans/registry.md`'s rows other than this milestone's.

**Environment notes.**

- **Rust toolchains differ.** The root workspace is `rust-version = "1.89"` (`Cargo.toml:12`); the
  desktop's Rust host declares `rust-version = "1.90"` (`apps/desktop/src-tauri/Cargo.toml:6`) and
  is its own `[workspace]` (`apps/desktop/src-tauri/Cargo.toml:8-10`). `apps/desktop/src-tauri` is
  **excluded** from the root workspace (`Cargo.toml:6`). Build the host with `cargo build
  --manifest-path apps/desktop/src-tauri/Cargo.toml --locked`; the root's `cargo check`/`clippy`
  will not see it.
- **The desktop is a separate npm workspace** (`apps/desktop/`). `npm run typecheck`, `npm test` and
  `npm run bindings:check` must be run from `apps/desktop/`.
- **`scripts/verify.sh quick` does not compile the desktop** (§11.4). M007-A installs the gate.
- **Broad sweeps** use nextest's `ci` profile with a private cache root: `XDG_CACHE_HOME="$(mktemp
  -d)" cargo nextest run --workspace --locked --profile ci`. This milestone changes no Rust, so it
  is optional.
- **Never `--all-features`** for a workspace sweep — it drags in the real-server tests.
- **Storage tests** use `isolated_pool()`; this milestone adds none.
- **Preserve unrelated user changes.** The worktree carries other writers' uncommitted work; commit
  only this milestone's files.
- **A commit message saying "closed" is not closure evidence.** §15's list is the evidence.

**Desktop tree notes.** `apps/desktop/package.json` pins React 19.3.0, Tauri 2.12.1, TypeScript
7.0.2, Vite 8.3.2 and vitest 5.0.3. The renderer's harness is vitest + `@testing-library/react`
(`apps/desktop/src/App.test.tsx:1-4`) with the bridge mocked
(`apps/desktop/src/App.test.tsx:6-31`), and the Vitest environment is jsdom, so `localStorage` is
available — the persistence tests need no extra harness, but they must clear the store between
tests. `apps/desktop/scripts/check-bindings.mjs` reads `src-tauri/src/bridge.rs` and
`src/bridge-types.ts` against a **fixed** expected field map: adding a field to either side
without adding it to the map fails the check, and adding nothing keeps it green.
