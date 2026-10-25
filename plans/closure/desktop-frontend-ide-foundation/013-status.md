# Desktop Frontend and IDE Foundation Milestone 013 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md`
  (milestone **M007-C**)

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADR (controlling):

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` — accepted.
  M007-C changes **no** ownership contract: the keymap is renderer-local
  preference state, not daemon state.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — accepted
  2026-10-24. It governs the **command** surface (M007-D), and it constrains
  M007-C in one way: the desktop's `/` command palette is M007-D's, so M007-C
  reserves its chord and does not implement command execution.

Repository baseline reviewed: `511bbdb31` — `origin/main` (with the M007-A
closure and the desktop-parity work) plus M007-B's region frame and panel
layer. M007-B's own closure and its merge into `main` were folded in before
this milestone's closure was written, so the tree this record describes is
the one `main` now carries.

Implementation commits on this milestone's branch:

- `d43817708` — the action catalog, the chord machinery, the keymap's
  persistence and conflict rules, the focus model, the bindings surface, and
  the renderer's wiring.
- `58c072e54` — an explicit per-test budget for the dual-reachability test,
  which timed out under load against vitest's 5000 ms default (§6).

## 1. Executive finding

The desktop had **zero** keyboard handlers and twelve `onClick` bindings. It
was a mouse-only application that happened to be a TUI's distant cousin.

M007-C makes it keyboard-first, and it does so **without touching the TUI**.
The TUI's `ActionKey` (`src/tui/input.rs:259`, 61 variants) is the desktop's
**source vocabulary**; the desktop derives its own action catalog from it, and
`plans/000-long-term-specification.md:564` is the licence for concept-parity
rather than key-for-key parity. `git diff --name-only -- src/tui/` is
**empty** — the TUI's keymap, focus model and behaviour are exactly what they
were.

Three decisions shape the result:

- **Direct manipulation, not a modal layer.** The plan's §1.1 decided this and
  the implementation followed it: chords act immediately, there is no
  Insert/Normal mode, and no plain key means two things at once. The one
  exception is *scoped* rather than modal: `enter` is the composer submit in
  the composer and the row activation in the catalog — the **scope** decides
  the owner, not a mode.
- **One entry point, one owner per chord.** A single `keydown` listener,
  installed once at the renderer's root. The order is: the capture field owns
  its key, `Tab` is the region's traversal, then the overlay scope, then the
  region scope, then the global map. Only a **captured** chord calls
  `preventDefault()`, so ordinary typing and native editing are untouched.
- **Every action has exactly one mouse affordance.** The keyboard action and
  its mouse affordance are the same operation, not two — a "keyboard-first"
  application whose keyboard surface is a parallel second copy of a mouse
  surface would drift.

## 2. Requirement-to-evidence matrix

| Plan criterion | Evidence | Verdict |
|---|---|---|
| §13.1 — the desktop has a **keymap** with remappable bindings, a conflict check, and a reset path | §3.2 — `apps/desktop/src/keymap.ts`: the declaration table, the effective map, the conflict check, the reset, the export and import | met |
| §13.2 — the **concept-parity table** records every canonical concept, what it maps to, and what the desktop does not carry, with reasons | §3.1 — `CONCEPT_PARITY` and `NOT_CARRIED_TUI_ACTIONS` | met |
| §13.3 — the chord normalisation is **deterministic and total**, and `mod` resolves per platform | §3.2 — `parseChord`, `resolveChordToken`, `neutralChord`; `mod` → `metaKey` on macOS, `ctrlKey` elsewhere | met |
| §13.4 — an override **replaces** an action's defaults entirely; a conflicting remap is **refused** and leaves the keymap byte-identical | §3.2 — `buildEffectiveMap` and `remapAction`, with the TUI's message shape | met |
| §13.5 — persistence is **one `setItem` per commit**; a malformed or wrong-version document yields the defaults **plus a visible notice**; an unknown action id is dropped individually | §3.2 — `codegg.desktop.keymap.v1`, the loader's four cases | met |
| §13.6 — the focus model binds to **M007-B's regions**, `Tab` **wraps** within a region, and the indicator is visible | §3.3 — the five regions, the roving `tabindex`, `data-focus-region` and the `focus-ring` class | met |
| §13.7 — `Esc` pops **exactly one** overlay layer and restores the prior focus; on the base view it does nothing | §3.3 — the overlay stack's restore-element and fallback | met |
| §13.8 — there is **one** keydown entry point, and only a captured chord calls `preventDefault()` | §3.2 — the single `window` listener in `App.tsx`; the `captured` flag marks the chords that shadow a WebView or OS accelerator | met |
| §13.9 — the bindings surface offers **remap, reset, export and import**, and import is **atomic** | §3.4 — `KeybindingsPanel.tsx`; a malformed *or* conflicting document is rejected whole | met |
| §13.10 — the **existing `data-testid` values survive** and the renderer suite passes unmodified | §4.1 — 232 → 262, **none removed**; `App.test.tsx` byte-identical | met |
| §13.11 — **no `src/tui/` change**, no protocol change, no Rust change, no Tauri capability | §5 — `git diff --name-only -- src/tui/` and `-- apps/desktop/src-tauri/` are both empty | met |
| §13.12 — the milestone's **non-goals** are honoured: no `/` command surface, no editor, no clipboard | §5 — no such capability is in the diff; `mod+q` is reserved and `CopyMessage` is recorded as not carried | met |
| §13.13 — the **terminology amendment** is recorded, not performed **by this milestone** | §6 — the proposed vocabulary is in this record. M007-C does not touch `plans/001-terminology-and-domain-model.md`; the **owner directed** the amendment on 2026-10-25 and it was performed by the follow-up plan at `plans/implementation/desktop-frontend-ide-foundation/016-desktop-input-model-terminology-amendment.md`, whose closure record is `plans/closure/desktop-frontend-ide-foundation/016-status.md` | met |

## 3. Production implementation evidence

### 3.1 The action catalog and the concept-parity table

`ALL_ACTION_DECLARATIONS` carries **19** actions. Each has an identity, a
label, a description, a default binding, a scope (`global`, `region:<id>` or
`overlay`), a mouse affordance, a `captured` flag and the TUI concept it
carries.

The concept-parity table maps the canonical concepts at
`plans/000-long-term-specification.md:550-562` onto the desktop:

| Canonical concept | Carried | Desktop action | Binding | Affordance |
|---|---|---|---|---|
| `Space f` — project picker | yes | `project.switcher.open` | `mod+p` | `project-switcher-button` |
| `Enter` — open/focus the project tab | yes | `item.activate` | `enter` (catalog scope) | `project-select-*` |
| `Space s` — project session picker | yes | `session.switcher.open` | `mod+shift+s` | `session-switcher-button` |
| `Space j` — project jobs | partial — the tasks panel *is* the picker | `tasks.toggle` | `mod+t` | `tasks-toggle-button` |
| `Space a` — agent tree | partial — the same surface | `tasks.toggle` | — | — |
| `gt`/`gT` — next/previous project tab | **no** — the desktop has no project-tab set | — | — | — |
| `Space p` — collaborators/presence | **no** — the projection DTO carries no presence data | — | — | — |
| `Space o` — observe the selected session | **no** — no observe workflow in the desktop | — | — | — |
| `Space c` — focus the project chat | **no** — no project-chat surface | — | — | — |

`NOT_CARRIED_TUI_ACTIONS` records the rest with a reason each: the clipboard
actions (clipboard is forbidden), the search actions (no find surface), the
exit actions (the native close owns exit; `mod+q` is reserved), the
directional and editing actions (M007-E), the modal and router concepts
(`ToggleComposerMode`, `Direction*`, `OpenPanelManager`), the tab-set actions,
and the actions with no desktop surface (`SwitchAgent`, `SelectModel`,
`CloseSession`, `ToggleTts`, …).

### 3.2 The keymap

- **Normalisation.** `parseChord` emits the fixed modifier order plus a
  lowercased key token, so `mod+shift+p` and `shift+mod+p` are the same chord;
  `resolveChordToken` maps `mod` to `meta`/`ctrl` per platform; `neutralChord`
  is the storage form and `normalizeChord` the matching form. An unrecognised
  key normalises to a chord nothing claims.
- **The effective map.** `buildEffectiveMap(declarations, overrides, platform)`
  merges defaults with overrides; an override **replaces** an action's
  defaults entirely. Claims are keyed by *(scope, resolved chord)*, and a
  conflict is resolved deterministically — the later-declared action keeps the
  chord and the earlier falls back to its default — and is reported as a
  `ConflictNotice`.
- **The conflict check.** `remapAction` evaluates the *current* effective map
  at commit time and refuses with the TUI's shape: `"<chord> is already bound
  to <action>"`. A refused remap leaves the overrides byte-identical.
- **Persistence.** One `setItem` per commit into `codegg.desktop.keymap.v1`. On
  load: a missing document yields the defaults silently; a malformed,
  wrong-version or unparseable document yields the defaults **plus a visible
  notice**; an unknown action id is dropped individually; a conflicting
  document resolves deterministically with a notice naming both actions.
- **Reset.** Drops the override document entirely and restores the declaration
  table's defaults.
- **Export/import.** `{"version":1,"bindings":{"<action id>":["<chord>"]}}`,
  exported into a read-only `<textarea>` and imported from an editable one
  with an explicit apply. Import is atomic: malformed **or** conflicting ⇒
  rejected whole, nothing applied.
- **One entry point.** A single `window` `keydown` listener in `App.tsx`,
  installed once, with the handler refreshed through a ref.

### 3.3 The focus model

`apps/desktop/src/focus.ts` holds the region registry, the region resolution,
the roving `tabindex`, the wrapping in-region traversal, the overlay focus
stack's primitives and the focus-indicator application.

- **Regions** (focus order): `catalog` (`catalog-region`), `chat`
  (`chat-region`), `sidePanel` (`side-panel-frame`), `composer`
  (`composer-region`), `status` (`status-row`).
- **Traversal.** `mod+1`…`mod+5` focus the n-th region directly; `Tab` and
  `Shift+Tab` move within the focused region and **wrap**; a roving `tabindex`
  keeps exactly one control tabbable per focused region, so `Tab` never
  escapes it.
- **Indicator.** The focused region container carries `data-focus-region` and
  an outline; the focused control carries the `focus-ring` class, styled from
  the class **and** `:focus` — not only `:focus-visible`, because a
  keyboard-driven focus must be visible even when the browser's heuristic
  would suppress it.
- **Overlays.** The stack traps the focus (`Tab` inside the surface wraps
  within it); `Esc` pops exactly one layer and restores the element that had
  the focus before that overlay opened; `Esc` on the base view does nothing —
  no overlay closes, the composer keeps its draft, nothing exits. While an
  overlay is open the overlay scope and the global map are consulted, but the
  region map behind the overlay does not fire: the overlay holds the focus,
  not a region.
- **Focus loss.** A focused region that unmounts hands the focus to its
  declared fallback (the composer), never to `document.body`.

### 3.4 The bindings surface

`apps/desktop/src/panels/KeybindingsPanel.tsx` — a searchable action list
with a roving tabindex, a remap capture, the conflict message,
reset-to-defaults, export in a read-only `<textarea>` and import from an
editable one with an explicit apply. It renders props and calls callbacks:
**no** `localStorage`, **no** `fetch`, and **no** runtime import of the keymap
module — the capture chord arrives through a prop. That keeps the panel layer
inside the authority boundary M007-B's guard enforces.

### 3.5 The documentation

- `architecture/desktop.md` gained a **new section, "The desktop's input
  model"**: where the keymap lives, the chord normalisation and the `mod`
  token, the declaration table and the concept-parity table, the focus model
  and the visible indicator, the affordance classes and the dual-reachability
  rule, the `Esc`/cancel semantics, and the explicit statement that the
  desktop's shortcuts are **in-window only** and that no OS-global shortcut
  exists.
- `architecture/overview.md`'s desktop entry gained one line saying the same.
- `architecture/tui.md` was **not** changed. The TUI's keymap, focus model and
  behaviour are unchanged, and the document has no "related frontends" slot
  that would mislead a reader into thinking the two keymaps are the same.

## 4. Verification executed

### 4.1 Local

| Command | Outcome |
|---|---|
| `npm run typecheck` | **pass** — `tsc --noEmit` and the e2e project, no output |
| `npm test` | **pass** — **25 files, 176 passed** (was 21 files, 131) |
| `npm run bindings:check` | **pass** — "desktop bridge DTO fields match" |
| `npm run build` | **pass** — 49 modules transformed |
| `rustup run 1.90.0 cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml --locked` | **pass** — "Finished `dev` profile … in 4.88s"; the host's own toolchain, since the host pins Rust 1.90 and the root workspace does not build it |
| `rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings` | **pass** — no warnings, 2.24s |
| `cargo fmt --all -- --check` | **pass** — no output, exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **pass** — no warnings, "Finished `dev` profile … in 4m 00s" |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." (rustfmt, the generated-asset checks, the 72 guard scripts, the workspace `cargo check`) |
| `apps/desktop/scripts/verify-desktop.sh` | **pass** — M007-A's one local entry point, on this head; the phases are `boundary`, `renderer`, `stage-daemon`, `host`, `e2e` |
| `python3 scripts/check_desktop_panel_authority.py` | **pass** |
| `python3 scripts/check_desktop_panel_authority.py --self-test` | **pass** — 18 injected defect classes, each rejected for its expected reason |
| `python3 scripts/check_desktop_gate_wiring.py` | **pass** |
| `python3 scripts/check_desktop_gate_wiring.py --self-test` | **pass** — 7 injected defect classes and 2 positive controls |
| `./scripts/check-desktop-boundary.sh` | **pass** — "desktop dependency and authority boundaries verified" |
| `git diff --name-only -- src/tui/` | **empty** |
| `git diff --name-only -- apps/desktop/src-tauri/` | **empty** |
| `git diff --name-only -- apps/desktop/src/bridge.ts apps/desktop/src/bridge-types.ts` | **empty** |
| `git diff --stat -- apps/desktop/src/App.test.tsx` | **empty** — byte-identical |
| the `data-testid` inventory, measured over the shipped renderer sources | **232 → 262, none removed or renamed** |

The new tests: **16** in `keymap.test.ts` (the plan's 15 plus a `neutralChord`
case), **11** in `focus.test.ts`, **4** in `panels/KeybindingsPanel.test.tsx`,
and **14** in `keyboard.test.tsx` (the plan's 12 §10.3 cases plus the
component-level reload test and the §10.5 contention test — a remap committed
while an overlay is open must not steal the focus).

### 4.2 Hosted

The hosted qualification on the milestone's pull-request head is recorded on
that head. The lane's 14 built-app tests select on the `data-testid` values
this milestone had to preserve, so a structural break in the renderer that the
unit tests missed is caught there.

The closest inherited hosted evidence is **M007-B's own pull-request head**,
which this milestone is stacked on — the same renderer, the same lane, the
same `data-testid` contract, and M007-B's panel frame underneath:

| Run | Head | Workflow | Conclusion |
|---|---|---|---|
| `40145996132` | `ba8aa1bcb` | Desktop E2E | **success** — the three phases green, 5 + 2 + 7 = 14 passing |
| `40145996125` | `ba8aa1bcb` | CI / verify | **success** — all 114 steps green: the 72 guard scripts, formatting, workspace clippy, and the workspace test sweep |

M007-C's own hosted run is recorded on its own pull-request head. Its
renderer change is exercised by the same Desktop E2E lane, and the lane's 14
built-app tests select on the `data-testid` values this milestone had to
preserve.

### 4.3 The named proofs the plan requires

The plan's §15 asks for proofs by **test name**, not by assertion of a
summary. They are here.

**Dual reachability** — `every_declared_action_is_reachable_by_chord_and_by_mouse`
(`apps/desktop/src/keyboard.test.tsx:210`). It enumerates every entry of
`ALL_ACTION_DECLARATIONS` — **19** (14 in `ACTION_DECLARATIONS`, 5 region-focus
actions in `REGION_FOCUS_ACTIONS`) — and asserts, per action, that it holds at
least one chord in the effective map **and** that its declared affordance
exists in the DOM, is visible, and is activatable: a `button`-class affordance
must be a `BUTTON` or `INPUT` and not disabled; an `editing`-class affordance
must be an `INPUT` or `TEXTAREA`; a `region`-class affordance must take the
focus on click. The count is **19 of 19**, and the test fails if an action
loses either route.

**Focus order** — `names_the_frame_regions_in_focus_order`
(`apps/desktop/src/focus.test.ts:55`) fixes the order `catalog`, `chat`,
`sidePanel`, `composer`, `status`;
`moves_the_focus_into_the_region_and_its_first_control` (`:80`) and
`marks_exactly_one_control_for_the_roving_tabindex` (`:88`) fix the roving
`tabindex` to exactly one tabbable control per focused region;
`traverses_within_the_region_and_wraps_at_the_ends` (`:106`) is the wrapping
proof; `falls_back_to_the_declared_region_when_the_focused_one_is_gone` (`:119`)
is the unmount proof.

**Focus indicator** — `marks_the_focused_region_and_the_focused_control`
(`apps/desktop/src/focus.test.ts:145`) and
`clears_the_indicator_when_the_focus_leaves` (`:154`), plus the end-to-end
`focus_is_always_visible` (`apps/desktop/src/keyboard.test.tsx:338`) and
`focus_is_never_left_on_document_body` (`:341`).

**Reserved chords** — the list as shipped is **11** entries
(`apps/desktop/src/keymap.ts:487`):

| Chord | Reserved for | Why it is unbound |
|---|---|---|
| `mod+s` | M007-E (editor save) | no editor in M007-C; binding it would capture the WebView save accelerator with no target |
| `mod+shift+p` | M007-D (command palette) | the palette's command contents are M007-D's, and a chord bound to an empty surface would be a dead chord |
| `mod+f` | a future in-app find | no find surface in M007-C |
| `mod+q` | never — the OS | quitting is OS-owned |
| `mod+w` | never — the OS | closing the window is OS-owned |
| `mod+c` | never — the OS | copy is OS-owned, including for the keymap export surface |
| `mod+x` | never — the OS | cut is OS-owned |
| `mod+v` | never — the OS | paste is OS-owned |
| `mod+a` | never — the OS | select-all is OS-owned |
| `mod+tab` | never — the OS | OS application switching |
| `mod+shift+tab` | never — the OS | OS application switching |

**None of the eleven is bound.** `no_bound_chord_appears_in_the_reserved_list`
(`apps/desktop/src/keymap.test.ts:240`) intersects the reserved set with every
action's default bindings and fails on any overlap. Two further tests hold the
boundary from the other side:
`no_chord_shadows_an_os_accelerator`
(`apps/desktop/src/keyboard.test.tsx:377`) asserts that `mod+q`, `mod+w`,
`mod+c`, `mod+x`, `mod+v` and `mod+a` are **not** captured, and
`a_captured_accelerator_is_prevented` (`:379`) asserts that `mod+p`, which is
deliberately captured, **is**.

## 5. Invariant review

- **The TUI is untouched.** `git diff --name-only -- src/tui/` is empty. The
  TUI's keymap, focus model, dispatch and behaviour are byte-for-byte what
  they were.
- **No protocol, storage or migration change.** `crates/codegg-protocol/**`,
  `crates/codegg-core/**` and `src/**` are untouched; the desktop's Rust
  workspace has no diff at all.
- **No Tauri capability, no new bridge target.** The keymap is renderer-local
  `localStorage`; the panel authority guard's rule 4 (no durable
  frontend-owned store) applies to the **panel** layer, and the keymap module
  is outside it by design — a user's binding preference is the user's, not
  the daemon's.
- **One keydown listener, one owner per chord.** The dispatch order is fixed
  and the overlay scope wins over the region scope, which wins over the
  global map.
- **Ordinary typing is untouched.** Only a captured chord calls
  `preventDefault()`.
- **The existing renderer suite is unmodified.** `App.test.tsx` is
  byte-identical; the new tests are in new files.
- **`Esc` on the base view does nothing.** It does not exit, does not close an
  overlay that is not open, and does not discard the composer's draft.

## 6. Failure and recovery review

- **A malformed override must not brick the desktop.** The loader's four
  cases — missing, malformed, wrong version, unparseable — all yield a usable
  keymap; three of them yield a **visible notice** so the user learns their
  binding was not applied rather than silently losing it.
- **A conflicting document must resolve, not deadlock.** It resolves
  deterministically with a notice naming both actions.
- **A half-applied remap must be unobservable.** One `setItem` per commit.
- **An import must be atomic.** Malformed *or* conflicting ⇒ the whole
  document is rejected.
- **A refused remap must change nothing.** The overrides stay byte-identical.
- **A region that unmounts must not lose the focus.** The focus falls back to
  the composer, never to `document.body`.
- **A remap committed while an overlay is open must not steal the focus.** The
  §10.5 contention test covers it.
- **The dual-reachability test was load-sensitive, and was given an explicit
  budget.** Running the local entry point while the root workspace's clippy was
  saturating the machine, `every_declared_action_is_reachable_by_chord_and_by_mouse`
  **timed out at 5140 ms** against vitest's 5000 ms default — a failure the
  unloaded run does not reproduce (the file passes in 3.88 s on its own, and
  the whole suite passes in 7.32 s). The cause is structural, not incidental:
  the test walks **all 19** declared actions and clicks **every** affordance it
  finds, and each click is an `await act(...)` flush, so its cost is
  proportional to the catalog rather than to one action. The fix is a
  per-test timeout of **20 s** on that one test, not a global
  `testTimeout` change — the default stays tight for every other test, so a
  genuine hang elsewhere still fails fast. It is recorded here because a test
  that fails only under load is a hosted-lane flake waiting to happen, and the
  hosted lane runs on a loaded runner.

## 7. Migration and compatibility review

None required. The renderer is a pure client of the projection it already had;
the keymap's persistence is a new, versioned, renderer-local document whose
missing state is the defaults.

## 8. Security review

- No Tauri capability was added; `scripts/check-desktop-boundary.sh` passes
  unchanged.
- The keymap holds no credential, no provider secret and no path. It is a
  table of chords and labels.
- The bindings surface cannot fetch or write: it renders props and calls
  callbacks, and M007-B's panel-authority guard enforces that.
- `mod+q` is **reserved** rather than bound: the desktop does not register an
  OS-global shortcut, so a chord that a user might expect to quit the
  application is left unbound rather than half-implemented.

## 9. Documentation and operations

| Document | What changed |
|---|---|
| `architecture/desktop.md` | the new **"The desktop's input model"** section |
| `architecture/overview.md` | one line on the desktop's own renderer-owned, remappable input model, and that it is in-window only |
| `architecture/tui.md` | **not** changed — the TUI's behaviour is unchanged and the document has no slot that would mislead |
| `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` | the M007-C row (status and dependencies); the M007-D row's dependency column, because M007-C is closed; the line's status row; and a new **"What M007-C changed"** paragraph that corrects the **zero keyboard handlers** claim the baseline carried |
| `plans/registry.md` | the M007-C row in `## Dependency-ready implementation plans`; the desktop row in `## Active subsystem roadmaps`; the M007 gate paragraph in `## Current execution order and dependency gates`; and a new dated entry in `## Source verification` |

## 10. Unresolved findings

| Finding | Severity | Owner |
|---|---|---|
| **`turn.toggle` and `model.cycleForward`/`model.cycleBackward` are recorded as not carried**, although the plan's WP-2 table lists them as carried with the affordances `turn-toggle-button` and `model-cycle-button` marked "(M007-B)". **Those buttons do not exist** in M007-B's tree, and neither action has a desktop surface to act on: the bridge carries no stop/resume command, and the bridge DTO carries no model catalogue to cycle. Inventing a button that cannot act would be a fake affordance and a dead chord, so both are recorded as not carried with the reason. **This is settled by evidence, not by a preference.** The WP-2 table's "(M007-B)" annotations were a *forward-looking assumption* that M007-B would supply those two buttons, and M007-B's own plan (`plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`) never claimed them — `turn-toggle-button`, `model-cycle-button`, `ToggleAgentRun` and `CycleModel` occur in **no** plan other than 013 — and M007-B's closure delivered a chat region, a dockable side panel and the projection fields it already had, which is what it was scoped to. Plan 013's own acceptance criteria (§13.1–§13.13) do not name either action. The plan file is **left untouched**, because the house pattern at closure is to record the deviation in the closure record rather than to amend an implementation plan after the fact — M007-B's closure (`b1b277505`) touched the closure record, the roadmap and the registry, and not plan 012. **If the owner intends the desktop to carry a turn toggle and a model cycle, that is new scope with no surface behind it today, and it needs its own milestone** — a stop/resume command in the bridge and a model catalogue in the DTO are both protocol and DTO changes, which this milestone's criterion §13.11 forbids. | low | closed by evidence; new scope would need its own milestone |
| The **project and session switchers are focus-and-reveal**, not picker overlays. A quick-open style surface would drift into M007-D's command surface, which the plan's §5.2 forbids. | low | accepted for this milestone |
| The bindings surface's own chords (`mod+alt+shift+r` reset, `mod+alt+shift+e` export, `mod+alt+shift+i` import) were **chosen by the implementer** — the plan requires "their own bindings" without naming them. | low | accepted for this milestone |
| The **terminology amendment is recorded, not performed by this milestone** — `keymap`, `chord`, and the new UI labels ("Toggle side panel", "Open project switcher", "Toggle tasks panel", "Keyboard help", "Key bindings") are UI-label surface under `plans/001-terminology-and-domain-model.md:5`, and adopting canonical vocabulary for them is an owner-directed step that changes a normative document. **Resolved after the fact: the owner directed the amendment on 2026-10-25**, and it was performed by the follow-up plan `016-desktop-input-model-terminology-amendment.md` — `plans/closure/desktop-frontend-ide-foundation/016-status.md`. The canonical set now names `Keymap`, `Chord`, `Action` and `Binding scope`, keeps **action** distinct from ADR-0052's **command**, and adds the UI-label rule. | low | closed |
| The desktop's **Windows native qualification** (M002's condition) is still outstanding, and this milestone makes no Windows claim. | medium | M002's line |

**No medium or high finding keeps M007-C open.** The one finding that began as
a plan-versus-baseline conflict — `turn.toggle` and `model.cycleForward` /
`model.cycleBackward` — was settled by evidence rather than by preference: the
plan's WP-2 table assumed M007-B would ship two affordances that M007-B's plan
never claimed and never delivered, and neither action has a surface behind it
today. It is recorded above, at low severity, with the reason and with what
new scope would cost. The remaining findings are low and accepted.

## 11. Roadmap disposition

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- the milestone-007 decomposition table's M007-C row moves from *ready for
  handoff* to **closed**, and its dependency column records M007-A and M007-B
  as closed;
- the M007-D row's dependency column records M007-C as closed, because the
  palette's chord is now bound;
- the status row records M007-C as closed;
- the desktop's **zero keyboard handlers** claim — which the roadmap still
  carried — is corrected by a new **"What M007-C changed"** paragraph that
  says what the desktop has now, and names the closure record.

## 12. Registry updates

`plans/registry.md`:

- the `## Dependency-ready implementation plans` row for M007-C moves from
  *ready for handoff* to **closed**, and points at this closure record rather
  than the implementation plan;
- the `## Active subsystem roadmaps` desktop row records that the desktop has
  its own renderer-owned, remappable input model with mouse parity, and that
  its shortcuts are in-window only;
- the `## Current execution order and dependency gates` section records that
  M007-C is closed, so M007-D's palette chord is now bound;
- `## Source verification` gains a dated entry for this closure.

## 13. Proposed terminology (owner-directed step — not performed **by this milestone**)

Recorded here, as the plan requires, and **not** applied to
`plans/001-terminology-and-domain-model.md` by this milestone. **The owner
directed the amendment on 2026-10-25**, and it was performed by the follow-up
plan `016-desktop-input-model-terminology-amendment.md`
(`plans/closure/desktop-frontend-ide-foundation/016-status.md`). The proposal
below is what landed, with two refinements the amendment made: the term is
**binding scope** rather than *scope*, because scope is already
provider-connection scope and authorization scope; and **action** was joined
by an "action" bullet in §15's prohibited-usage list, beside the "command"
bullet it is easiest to confuse with.

| Proposed term | Sense | Note |
|---|---|---|
| **keymap** | the desktop's renderer-owned mapping from chords to actions | descriptive; the canonical documents do not use the word today |
| **chord** | a modifier-normalised key combination | descriptive |
| **action** | one entry in the desktop's declaration table — identity, label, description, default bindings, affordance | the canonical documents use "command" for the `/` surface's catalog entry (ADR-0052's `### Command`), so **action** and **command** must not be conflated: a command is a catalog entry the **daemon** owns, an action is a desktop-local binding |
| **scope** | the region, overlay or global context a chord is resolved in | the desktop's own concept |

The "action" versus "command" distinction is the load-bearing one: ADR-0052's
`Command` is a daemon-owned catalog entry, and M007-C's "action" is a
renderer-local binding. Using one word for both would be exactly the
incompatible local meaning `plans/002-long-term-roadmap.md:899` warns about.

## 14. Handoff notes

- **The next milestone is M007-D**, and it is unblocked: the palette's chord
  (`mod+shift+p`) is reserved by M007-C and M007-C states that the palette's
  command contents are M007-D's.
- **M007-E** waits on M007-B's region frame (which exists) and inherits
  M007-C's keymap: the editor's own key bindings should be **declared in the
  keymap**, not hard-coded in the editor, so the two surfaces share one
  dispatch and one conflict rule.
- The reserved-chord list and the `captured` flag are the mechanism M007-D's
  palette and M007-E's editor should extend rather than duplicate.
