# TUI Composer Helix Parity Milestone 001 — Composer Keyboard Ownership and Cursor Presence

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/001-status.md`

Repository baseline: `61cbe9222` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m001--composer-keyboard-ownership-and-cursor-presence`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- None required for this milestone. ADR-0042
  (`plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md`) governs M002 and the verb
  milestones; M001 deliberately lands first because it introduces no key claim.

Primary class: capability (composer cursor honesty), with infrastructure (keyboard-ownership
derivation)

## 1. Objective

Make the composer draw a cursor that is true about the composer's state: the terminal's own
blinking cursor at the caret in Insert mode, a solid block at the caret in Normal mode, and no
cursor at all when the composer does not hold the keyboard — with the cursor always agreeing with
the `[INS]` / `[NOR]` badge.

This milestone also produces the derived "does the composer hold the keyboard" fact that every
later milestone in this workstream depends on, and it repairs the composer's focus flag, which is
currently wired to nothing.

## 2. Why this milestone is ready

- **No hard dependencies.** The milestone reads existing app state only. It introduces no new
  dependency on the claim table, the text model, or the geometry.
- **Stable interface dependencies.** The inputs it builds on are already established and
  documented:
  - `App::composer_input_mode()` (`src/tui/app/mod.rs`) already resolves the composer's mode
    across the terminal box, side panels that claim insert keys, and the raw app mode field.
  - `App::terminal_box_owns_keyboard()` (`src/tui/app/mod.rs`) already answers the terminal box's
    keyboard ownership.
  - The composer badge already renders the resolved mode, so the resolved mode is a settled
    concept with a user-visible surface.
- **It is independently shippable.** Nothing in it depends on a design decision that is still
  open. It closes capability C6 and infrastructure N4 of the roadmap, and it is the prerequisite
  for M002's claim table.
- **The open decision D3 (blink source) is resolved by the constraint, not by the plan.** The
  roadmap's invariant I7 and the app's single-cadence rule in `AGENTS.md` leave exactly one
  acceptable implementation, described in Work Package 4. The milestone's acceptance criteria are
  written so that an implementation which adds a timer fails them.

## 3. Current implementation evidence

- `PromptWidget` (`src/tui/components/prompt.rs`) holds `text: String`, a byte-offset `cursor`, a
  `focused: bool`, and scroll offsets. It supports multi-line text.
- `PromptWidget::new` initialises `focused` to `true`. `PromptWidget::blur()` exists.
- **`PromptWidget::blur()` has exactly one caller in the tree, and it is a test**
  (`tests/tui.rs`). No production path ever clears the flag, so the composer cannot currently
  express "not focused". The flag is dead.
- The composer paints its cursor as a reversed cell: the cell at the caret gets the theme's
  foreground as background, the background as foreground, and the `REVERSED` modifier. The paint
  is guarded by `focused && !text.is_empty()`, so an empty composer draws no cursor at all.
- The composer is rendered from `src/tui/app/render.rs` with `frame.render_widget(&prompt, area)`.
  A `Widget` implementation has no `Frame` handle, so the widget cannot position the terminal's
  cursor itself; that must be done by the app's render layer.
- **No code anywhere in the app positions the terminal's own cursor.** There is no
  `set_cursor_position`, no `show_cursor`, and no `hide_cursor` call in `src/`. The TUI's fake
  cursor is the only cursor the user has ever seen.
- The ratatui version is 0.30.2. In that version, `Frame::set_cursor_position` sets the cursor
  position for the frame, and **if it is not called, the terminal cursor is hidden** after the
  draw. `Frame::set_cursor(x, y)` is deprecated in favour of `set_cursor_position`.
- `App::composer_input_mode()` returns `Insert` when the terminal box owns the keyboard, `Normal`
  when the terminal box has the keyboard but does not own it, `Insert` when a focused side panel
  claims insert keys, and otherwise the raw app mode field.
- The composer badge renders the resolved mode, and its own doc comment records why: a focused
  terminal panel was previously reporting the composer's mode.

## 4. Invariants that must not regress

- **I5 (from the roadmap) — the resolved mode is the gate.** The cursor must be driven by the
  *resolved* mode, never by the raw `UiState::input_mode` field. The raw field is forced to
  `Normal` whenever a side panel takes focus, so gating on it would show a composer cursor while
  the user is typing into a panel.
- **I7 — one redraw cadence.** No new timer, no new animation, no new poll interval.
- **The badge must not change meaning.** The `[INS]` / `[NOR]` badge is the app's statement about
  the composer. The cursor is added to agree with it, not to redefine it.
- **The composer's text, caret, and scroll behaviour must not change.** This milestone touches
  presentation, not editing.
- **The terminal cursor and the fake block must never both be visible.**
- **No authority change.** Nothing here alters what the daemon accepts or how a turn is admitted.
- **The editor's and the terminal panel's cursors are unaffected.** The editor draws its own fake
  cursor and must keep doing so.
- **The app must not call `Terminal::hide_cursor()` or `Terminal::show_cursor()` directly.**
  Ratatui's per-frame cursor management and the app's own calls would fight; the app currently
  makes none, and that must stay true.

## 5. Scope

### In scope

- A derived helper answering "does the composer hold the keyboard", placed alongside
  `App::composer_input_mode()`.
- A caret-cell computation on the composer widget, returning the composer's caret position in
  terminal coordinates, shared by the paint and the app's render layer.
- Cursor rules in the app's render layer: position the terminal cursor in Insert mode; paint a
  solid block in Normal mode; do neither when the composer does not hold the keyboard.
- The composer paint's block rule updated to agree with the above, including the empty-composer
  case in Insert mode.
- The dead `focused` flag either removed or rewired to the derived fact. Removing it is preferred
  if no other consumer needs it.
- Tests for the ownership derivation, the cursor rules, and the badge agreement.
- Documentation updates listed in section 12.

### Out of scope

- Any Helix key, motion, selection, undo, yank, or paste. No key claim of any kind. The claim
  table is M002's deliverable and ADR-0042's decision.
- Any change to the app's binding maps.
- Any change to the badge's rendering or meaning.
- Any change to the composer's text model, editing behaviour, or scroll behaviour.
- Any change to the editor, the terminal panel, the side-panel composers, the chat box, or the
  dialogs.
- A user-configurable cursor style or blink rate.
- The `space` / `x` / `d` / `i` re-spellings. Those are M002's, under ADR-0042.

## 6. Required production changes

**Core.** No domain types change. The composer buffer is unchanged.

**Storage.** None. Nothing is persisted.

**Protocol.** None. No daemon message, session state, or wire format changes.

**Runtime.** The app's render layer gains the responsibility for positioning the terminal cursor
when the composer holds the keyboard in Insert mode, and for not positioning it otherwise. The
app's key-dispatch chain is unchanged.

**Frontend.**

- `src/tui/app/mod.rs`: a new derived helper next to `composer_input_mode()`, expressing
  composer keyboard ownership from the same inputs the mode resolution uses. It must be a
  function over current state, not a stored field — a stored copy can drift from the badge, which
  is the defect the existing resolution was written to fix.
- `src/tui/components/prompt.rs`: a method returning the caret's cell position, computed from the
  caret, the text layout, the vertical scroll, and the horizontal scroll. The same computation must
  serve the block paint and the terminal-cursor positioning, so that the two can never disagree
  about where the caret is.
- `src/tui/app/render.rs`: the cursor rules, applied after the composer widget is rendered.

**Security.** None. No text is copied, transmitted, or logged.

**Docs.** See section 12.

## 7. Ordered work packages

### WP1 — Derive composer keyboard ownership

*Intent.* Produce one derived fact that the badge, the cursor, and (later) the key claim all read.

*Required changes.*

- Add a helper on `App` next to `composer_input_mode()`, named for ownership rather than mode.
  It must answer whether the composer currently holds the keyboard, taking into account at least:
  the terminal box's keyboard ownership, a focused side panel that claims insert keys, an open
  dialog or modal that owns the keyboard, the editor route, and the workspace focus.
- It must read the same inputs the badge's resolution reads. Deriving ownership separately from
  mode resolution, from a second set of conditions, is the drift this work package exists to
  prevent.
- Prefer deriving it *from* the resolved mode and the existing ownership helpers rather than
  re-deriving the panel rules.

*Acceptance evidence.*

- A test that the helper agrees with the badge in every state the badge can display: composer
  focused, terminal box focused, a side panel that claims insert keys focused, a side panel that
  does not, the editor route, and a dialog open.
- A test that the helper is not stale after focus changes: focus a panel, then return focus to
  the composer, and assert the helper reflects the change without any explicit invalidation call.

### WP2 — Compute the caret cell

*Intent.* One computation of "where is the caret on screen", usable by both the block paint and
  the terminal-cursor positioning.

*Required changes.*

- Add a method on the composer widget that returns the caret's cell position, or `None` when the
  caret is not currently on screen (for example, scrolled out of the visible window).
- It must account for the text's line structure, the vertical scroll, the horizontal scroll, and
  the composer's prefix row, so that the returned cell is the cell the user actually sees the
  caret in.
- The existing reversed-cell paint must use this same method rather than recomputing the
  position, so the two cannot drift.

*Acceptance evidence.*

- A test that the computed cell matches the cell the existing paint modifies, for a caret at the
  start, in the middle, at the end of a line, and on a second line.
- A test that the computed cell follows the vertical and horizontal scroll offsets.

### WP3 — Paint rules and the dead focus flag

*Intent.* The composer paints a solid block in Normal mode, and does not paint one in Insert mode
— the terminal's own cursor is used there instead.

*Required changes.*

- The composer's block paint is gated on Normal mode rather than on the `focused` flag, and is
  suppressed when the composer does not hold the keyboard.
- The `focused` flag: if it has no other consumer, remove it. If it does, rewire it to the
  derived ownership fact rather than to any stored value. In either case the composer must not
  retain a flag whose value can disagree with the badge.
- The empty composer in Normal mode paints a block at the caret, which is at column zero of the
  first line. The current guard `!text.is_empty()` must not suppress it.

*Acceptance evidence.*

- A test that the block is painted in Normal mode, at the caret, including for an empty
  composer.
- A test that the block is not painted in Insert mode, and not painted when another surface holds
  the keyboard.

### WP4 — Position the terminal cursor in Insert mode

*Intent.* The blinking cursor, with no new cadence.

*Required changes.*

- In the app's render layer, after the composer widget is rendered, position the terminal cursor
  at the caret cell when the composer holds the keyboard and the resolved mode is Insert.
- Use ratatui's current cursor API, `Frame::set_cursor_position`, with a `Position`. The
  deprecated `Frame::set_cursor(x, y)` must not be used.
- Position nothing in Normal mode and when the composer does not hold the keyboard. Ratatui hides
  the terminal cursor whenever the frame did not position it, so "position nothing" is the correct
  and sufficient way to show no terminal cursor.
- The app must not call `Terminal::show_cursor()` or `Terminal::hide_cursor()` directly, because
  those fight the per-frame mechanism.
- The editor must keep painting its own fake cursor, and the app must not position the terminal
  cursor anywhere the editor owns the buffer.

*Acceptance evidence.*

- A render-level test asserting the frame's cursor position is the caret cell in Insert mode, and
  that it is unset in Normal mode and when another surface holds the keyboard.
- A test asserting the terminal cursor and the painted block are never both present in the same
  frame.

### WP5 — Agreement with the badge

*Intent.* The cursor and the badge never disagree.

*Required changes.*

- The cursor rules and the badge must read the same derived ownership fact and the same resolved
  mode.
- If the badge ever renders a state the cursor rules do not cover, the cursor rules are the
  defect.

*Acceptance evidence.*

- A test that walks the focus states the app can be in and asserts, for each, that the badge's
  mode and the cursor's presence and shape agree.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** The render layer must not panic when the caret cell cannot be computed (an empty
  composer, a caret scrolled out of view, a zero-sized area). A `None` caret cell means "no
  terminal cursor", not a panic.
- **Cancellation.** Not applicable. This milestone adds no cancellable operation.
- **Restart.** Not applicable. Nothing is persisted.
- **Contention.** The composer is single-owner and frontend-local. The only interaction is with
  the app's own key-dispatch chain, and this milestone adds no key handling, so it introduces no
  new contention. The ownership derivation must reflect focus changes immediately, because focus
  changes are the app's own state transitions and there is no external notification to wait for.
- **Resize.** A terminal resize changes the composer's layout and its scroll offsets. The caret
  cell must be recomputed on the next frame, and the composer must not be left positioned at a
  stale cell. Because the computation is per-frame and derived, this is automatic; a test should
  assert it.

## 9. Compatibility and migration

- **No storage migration.** Nothing is persisted.
- **No protocol change.**
- **No configuration change.** The cursor style and the blink source are not user-configurable in
  this milestone.
- **User-visible change.** The composer gains a visible terminal cursor in Insert mode where it
  previously had only a static reversed cell. This is a deliberate improvement and is recorded in
  the roadmap's C6. The user-visible *key* changes are M002's, under ADR-0042, and are not part
  of this milestone.
- **Platform note.** The blink rate is the terminal's, not the app's. Terminals configured not to
  blink will show a steady cursor in Insert mode. This is accepted: the app's single-cadence rule
  forbids an app-driven blink, and the user's requirement is a blinking cursor in Insert mode and
  a solid one in Normal mode. The distinction that matters — Insert versus Normal — is enforced
  by shape, not by blink, so a non-blinking terminal still distinguishes the two modes.

## 10. Required tests

**Focused.**

- Ownership derivation: agreement with the badge across every focus state; no staleness after a
  focus change.
- Caret cell computation: start, middle, end of line, second line, after scroll, on an empty
  composer, and when the area is too small.
- Paint rules: block painted in Normal mode including an empty composer; not painted in Insert
  mode; not painted when another surface holds the keyboard.
- Terminal cursor positioning: positioned at the caret in Insert mode; unset in Normal mode; unset
  when another surface holds the keyboard; never both the terminal cursor and the painted block.
- Resize: the caret cell is recomputed and the cursor does not stay at a stale position.

**Integration.**

- A test that drives the app's key path into Insert mode and asserts the composer owns the
  keyboard and the cursor is positioned, and that focusing a side panel and asserting the
  composer shows no cursor.

**Regression guards.**

- The existing test that a bare character in Insert mode is typed rather than interpreted as an
  app action must continue to pass.
- The existing editor and terminal-panel cursor behaviour must be unchanged.

**Not required here.** Dispatch-reachability tests for Helix verbs, claim-set leak tests, and the
composer key-ownership guard are M002's, because M002 introduces the claim table.

## 11. Required verification commands

```bash
# Fast feedback while iterating
cargo test -p codegg --lib tui::components::prompt
cargo test -p codegg --lib tui::app::state
cargo test --test tui_main_buffer
cargo test --test tui_render

# Canonical gate
scripts/verify.sh quick

# Full gate before closure
scripts/verify.sh full
```

`scripts/verify.sh quick` is the canonical sanity gate and includes the TUI authority checks
(`scripts/check_tui_project_authority.py`, `scripts/check_tui_editor_text_authority.py`,
`scripts/check_tui_editor_key_authority.py`, `scripts/check_tui_editor_prefix_state.py`,
`scripts/check_tui_transport_authority.py`, and
`scripts/check_tui_subagent_name_pool_isolation.py`) that this milestone must not regress. Do not
run `--all-features` sweeps: they drag in the real-server tests. `cargo fmt` is part of `quick`.

## 12. Documentation updates

- `architecture/tui.md`: record the composer's keyboard-ownership derivation, the cursor rules, and
  the fact that the cursor and the badge read the same derivation. Also record that the app
  positions the terminal cursor for the composer and does so nowhere else.
- `docs/tui.md`: a short note, in the section that describes where input goes, describing the
  composer's cursor in Insert and Normal mode and that no cursor is shown when another surface
  holds the keyboard. The full composer keymap arrives with M004–M006 and must not be
  described ahead of its implementation.
- `.opencode/skills/tui/SKILL.md`: the dispatch boundary note for the composer.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md`: update the M001 row of the milestone
  status table when the closure record is written.

## 13. Acceptance criteria

Externally observable, from a user's position at the terminal:

1. With the composer focused and in Insert mode, a cursor blinks at the caret. The caret is
   visible when the composer is empty.
2. With the composer focused and in Normal mode, a solid block is shown at the caret, and no
   blinking cursor is shown.
3. With a side panel focused, the interactive terminal box focused, the editor focused, or a
   dialog open, the composer shows no cursor at all.
4. The cursor's presence and shape always agree with the `[INS]` / `[NOR]` badge.
5. Exactly one cursor is visible at any time: the terminal cursor and the composer's block are
   never both shown.
6. The editor's own cursor is unchanged, and the interactive terminal panel's behaviour is
   unchanged.
7. Typing, the caret's movement, the scroll offsets, and the composer's text are unchanged by
   this milestone.

## 14. Stop conditions

Report rather than improvise if any of the following holds.

- The ownership derivation cannot be expressed from the same inputs the badge uses without
  duplicating the panel rules. That duplication is the drift the milestone exists to prevent;
  report it rather than shipping two derivations.
- The caret cell cannot be computed consistently with the composer's existing paint, because the
  paint's geometry and the render layer's geometry disagree. Report the disagreement rather than
  patching one side.
- The terminal cursor would have to be positioned by the editor or the terminal panel as well to
  avoid two cursors. That is a separate ownership question and belongs to a separate decision.
- A blinking cursor would require an app-driven timer. The single-cadence rule is not negotiable
  inside this milestone; report that the blink is terminal-dependent and let the roadmap record
  it.
- The app already calls `Terminal::show_cursor()` or `Terminal::hide_cursor()` somewhere, which
  would fight ratatui's per-frame mechanism. Report it rather than adding a second mechanism.
- The milestone would need to change the composer's editing behaviour to deliver the cursor
  rules. That is out of scope.

## 15. Closure evidence required

- A closure record at `plans/closure/tui-composer-helix-parity/001-status.md`, following the
  closure template, with a requirement-to-evidence matrix covering C6, N4, I5, and I7.
- Test evidence for every acceptance criterion in section 13, with the exact commands from
  section 11 and their results, labelled truthfully as local or CI.
- The output of `scripts/verify.sh quick` and `scripts/verify.sh full` at the closure commit.
- A statement of the cursor's actual appearance in a real terminal, including whether the
  terminal blinks, since the blink is terminal-provided. A render-level assertion is necessary
  but not sufficient evidence for a requirement phrased in terms of what the user sees.
- Confirmation that the app contains no new timer, poll interval, or animation cadence, and that
  it makes no direct `show_cursor` / `hide_cursor` call.
- Registry updates, including the M001 status change and an audit of blocked work for any plan
  whose blocker this closure resolves.

## 16. Handoff notes

- **Start here.** `App::composer_input_mode()` in `src/tui/app/mod.rs` is the model for the
  ownership derivation: it already encodes the terminal box, the side panels that claim insert
  keys, and the raw mode field, and its doc comment records the badge bug it fixed. Read it
  before designing anything.
- **The composition root.** `src/tui/app/render.rs` renders the composer with
  `frame.render_widget(&prompt, area)`. The terminal-cursor positioning belongs in that render
  layer, because a `Widget` implementation has no `Frame` handle.
- **Ratatui's cursor semantics.** In ratatui 0.30, positioning the cursor for a frame and not
  positioning it are the two halves of one mechanism: not positioning it hides the terminal
  cursor. The app must therefore not call `Terminal::show_cursor()` or `Terminal::hide_cursor()`
  itself, and must not use the deprecated `Frame::set_cursor`.
- **The dead flag.** `PromptWidget::blur()`'s only caller is a test. Do not wire the flag to a
  stored value to make it "work" — that reintroduces exactly the staleness class the mode
  resolution was written to avoid. Prefer removing the flag if nothing else needs it.
- **Do not start the key work here.** M002 owns the claim table and its guard, under ADR-0042.
  M001 introduces no key claim and must not change any binding map.
- **Two other Helix surfaces exist.** `src/tui/editor.rs` and
  `src/tui/commands/interactive_terminal.rs` already implement the requested verbs. They are
  references for intent, not for code: this milestone's ownership derivation and cursor rules are
  the composer's own, and the editor's cursor and dispatch authority must be left alone.
