# TUI Composer Helix Keybinding Parity Roadmap

Status: active; ADR-0042 accepted 2026-10-19 with the `space`-chord amendment

Long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19, with the
  `space`-chord amendment — governs M002 and every
  verb milestone)

## 1. Purpose and ownership boundary

The main composer is the app's primary text-entry surface: the field the user types a request to
the model into, and the field the human shell, slash commands, and staged shell input are composed
in. Today it is a plain text buffer with a caret. It has no selection, no undo, and no navigation
beyond the caret, and its Normal mode is not a mode at all — it is the absence of typing, during
which bare letters fall through to app-level actions.

This workstream gives the composer the same Helix-style text manipulation the editor and the
interactive terminal panel already have: motions, character and line selection, yank, paste,
delete, undo, redo, and insert transitions, plus a cursor whose presence and shape follow the
composer's actual mode and keyboard ownership.

**Ownership boundary.** The composer's buffer is a **frontend-local string**. It is not a document,
not daemon-authorized text, and not part of the editor's `Route::Editor` surface. It is delivered
to the daemon only through the ordinary submit path. Every milestone in this workstream must
preserve that boundary:

- The composer owns its own text, selection, and undo state. It does not reuse the editor's
  document, controller, or transaction machinery.
- The composer is a **peer** of the editor and the terminal panel, not a subordinate of the
  editor. The editor's key authority, prefix state, and text authority guards are the editor's
  business and are not extended to cover the composer.
- The composer **does** reuse the app-wide shared selection state machine
  (`src/tui/helix_selection.rs`) and the shared clipboard utility, because those are the assets
  that exist precisely to keep a third surface from drifting.
- The composer's surface selection — "does the composer hold the keyboard right now" — is
  **derived**, never stored, and follows the existing `App::composer_input_mode()` pattern.

**Out of scope surfaces.** The editor buffer, the interactive terminal panel, the side-panel
composers, the chat box, the project picker, the dialogs, and the file tree are all different
buffers with different ownership. This workstream changes none of their behaviour. Where the
composer's work requires a shared asset to change, the change must be additive and behaviour-
preserving for the other surfaces.

## 2. Work classification

### Invariants

- **I1 — One owner per chord per surface.** In the main-buffer surface, every key has exactly one
  owner. A key cannot be simultaneously a composer verb and an app-level action.
- **I2 — A claimed key never falls through.** A letter the composer claims is consumed by the
  composer even when the composer's verb is a no-op (no selection, single-line buffer). It must
  not silently become an unrelated app verb. This is the defect class the terminal panel's
  dispatch comment records and the editor's key-authority guard now prevents.
- **I3 — The composer never reads the process CWD or an ambient path.** Composer text is a
  string; it has no filesystem identity. This is inherited from the app's daemon path rules and
  is recorded so a future "open the file under the cursor" verb does not quietly break it.
- **I4 — The composer buffer is not the document.** No composer milestone may route composer
  text into the editor's document controller, `codegg_document` transactions, or the daemon's
  file-change bus.
- **I5 — The resolved mode is the gate.** The composer claims letters only when the *resolved*
  mode (the one `App::composer_input_mode()` returns) is Normal, never when the raw
  `UiState::input_mode` field says so. The raw field is forced to Normal whenever a side panel
  takes focus, and gating on it would let the composer claim letters while the user is typing
  into a panel.
- **I6 — Undo does not resurrect submitted text.** The composer's undo stack is bound to the
  current buffer and is cleared when the composer is submitted or replaced.
- **I7 — One redraw cadence.** The composer introduces no second timer and no second animation
  cadence. Where a blinking cursor is required, the terminal's own cursor is used rather than an
  app-driven blink.

### Capabilities

- **C1 — Navigable composer.** In Normal mode, with the composer holding the keyboard, the user
  can move the caret horizontally, vertically, and by word without leaving the field.
- **C2 — Selectable composer.** The user can select characters and whole lines in the composer,
  and see the selection.
- **C3 — Editable selection.** The user can delete a selection, yank it to the clipboard, and
  paste it back.
- **C4 — Undoable composer.** The user can undo and redo composer edits, bounded in depth.
- **C5 — Insert transitions.** The user can enter insert mode before or after the caret, and open
  a line below, from Normal mode.
- **C6 — Honest cursor.** The composer shows a blinking cursor in Insert mode, a solid cursor in
  Normal mode, and no cursor at all when it does not hold the keyboard. The cursor and the
  `[INS]` / `[NOR]` badge always agree.
- **C7 — Discoverable keymap.** Every composer verb is reachable from the help surface and is
  described in the user documentation.

### Infrastructure

- **N1 — Shared composer geometry.** Word, line, and caret-boundary geometry over a plain string,
  shared with the editor so the two surfaces compute word boundaries identically.
- **N2 — Composer text model.** A selection state and a bounded undo/redo history for a
  string-backed buffer.
- **N3 — Composer key-ownership guard.** A static guard proving the composer's claim set is
  closed, single-owner, and non-leaking, with a self-test.
- **N4 — Composer keyboard-ownership derivation.** A derived helper answering "does the composer
  hold the keyboard", alongside the existing mode-resolution helper.

### Polish

- **P1 — Composer hint row.** A one-line hint in the composer showing the currently available
  Normal-mode verbs, matching the interactive terminal panel's hint row.
- **P2 — Documentation.** A user-facing section in `docs/tui.md` describing composer Normal-mode
  keys, and the corresponding updates to `architecture/tui.md` and the TUI skill.

## 3. Non-goals

- **Not** a second document editor. The composer buffer is not the editor, is not
  daemon-authorized, and gains no LSP, no tree-sitter, no file backing, and no multi-selection.
- **Not** Helix parity in full. This workstream targets the basic navigability and
  manipulation set — motions, `v`, `x`, `d`, `u`, `U`, `o`, `i`, `a`, and the copy gesture
  `space` then `y`. Helix's
  multi-selection, text objects, match mode, goto mode, and unimpaired navigation are explicitly
  out of scope, and are tracked by the editor roadmap for the editor surface.
- **Not** a change to the editor's or the terminal panel's keymaps. Those surfaces keep their
  current behaviour exactly.
- **Not** a configurable keymap. The composer's claim set is fixed in this workstream. Making it
  user-configurable is deferred and would require its own decision.
- **Not** a change to the daemon protocol, the turn submission path, the prompt queue, or the
  human-shell staging model.
- **Not** a transcript selection or message-level navigation. The transcript is not the
  composer's buffer.
- **Not** a new redraw cadence, and not an app-driven blink timer.
- **Not** a change to how the app resolves the composer badge, except where the cursor must agree
  with it.

## 4. Current state

Repository evidence at baseline `61cbe9222`.

**The composer buffer is a string and a caret.** `PromptWidget`
(`src/tui/components/prompt.rs`) holds `text: String`, a byte-offset `cursor`, a `focused` flag,
and scroll offsets. It supports multi-line text. It has no selection, no undo history, and no
yank register. Its public surface is insert, delete, move, and paste.

**The `focused` flag is not wired to anything.** `PromptWidget::blur()` has exactly one caller in
the tree, and it is a test. No production path clears the flag, so the composer cannot currently
express "I am not focused". Any milestone that must show no cursor when another surface holds the
keyboard has to fix this first.

**One mode axis, and it is a keymap gate.** `InputMode` (`src/tui/input.rs`) is `Insert` or
`Normal` and lives as a single global field on `UiState`. In Insert, bare characters are text. In
Normal, bare characters resolve to app-level actions through the global binding maps. The
composer's badge already renders `[INS]` / `[NOR]`, and `App::composer_input_mode()` already
resolves which surface actually holds the keyboard, covering the terminal box and side panels
that claim insert keys. That resolution exists because the badge was previously wrong.

**"The main buffer is selected" has no representation.** `InputTarget` describes the composer's
*delivery destination* (`Build` or `Chat`), not focus. Focus is a multi-axis fact —
`panel_host.focused`, the focus manager, the editor's buffer focus, the terminal box's keyboard
ownership, the workspace focus, and the active subagent tab — resolved by the app's key-dispatch
priority chain rather than by a single enum. `App::on_key` is that chain.

**The alphabet is partly spoken for.** In the main buffer's Normal mode, `h` and `l` already move
the composer caret (through the app's `Left` / `Right` actions), so those two are not conflicts.
But `x` is the turn stop/resume action, `d` is open-diff in the vim map, `space` is help in both
maps, `j` and `k` are transcript navigation, `g` is go-to-top, and `i` is focus-prompt. `e`, `b`,
`v`, `o`, `u`, `U`, `p`, and `w` are unclaimed.

`space` is the one that changed its standing. Help keeps `space` — ADR-0042's amendment
rejected re-spelling it — and the copy gesture became the **second step** of that chord rather
than a prefix: `space` opens the help menu, and `y` in the menu the `space` chord opened copies
the composer's selection. The composer therefore claims neither `space` nor `y`.

**Capital letters have two encodings.** The app binds some capital letters twice — `Char('H')`
with `SHIFT` and `Char('H')` with `NONE` — because some terminals report a shifted key without
the `SHIFT` modifier. A composer claim on an uppercase letter, notably `U` for redo, must match
both encodings or the verb works on one terminal and not another. This is the app's existing
convention, not a new one.

**Bare capital letters once swallowed typed text.** A capital letter bound under the unshifted
encoding used to be consumed in Insert mode, so typing it opened a panel instead of inserting a
character. The fix was made at the key, in the Insert-mode text-key guard, rather than by
removing the binding, and a sweep over both keymaps asserts the property. A composer claim table
must not reintroduce that defect, and the Insert-mode sweep for the claim set is the same kind of
test.

**Two surfaces already implement the requested verbs.** The editor implements them over a document
under its own dispatch authority. The interactive terminal panel implements them over a scrollback
buffer, and its dispatch records the leak defect that I2 exists to prevent.

**Reusable assets already exist.** The shared selection state machine
(`src/tui/helix_selection.rs`) is generic over a point type and is already exercised against an
independent implementation. The editor's prefix type is coordinate-agnostic. The word-segmentation
policy is a small standalone function. The clipboard utility has both write and read.

**The editor's geometry does not transfer.** The editor's word and line functions are all bound to
its document snapshot type. They cannot be called against a plain string, so either the composer
reimplements them or they are extracted behind a shared source trait.

**The guards do not cover the composer.** The editor's key-authority guard targets the editor's
command module only. The editor's prefix-state guard requires exactly one prefix field on the
editor's presentation type. The editor's text-authority guard scans an explicit file allowlist
that does not include the composer, which is correct — the composer's string is a legitimate
owning buffer, not a document replica.

**No real terminal cursor is positioned anywhere in the app.** The TUI draws a fake cursor as a
reversed cell and never positions the terminal's own cursor. The terminal backend hides the
terminal cursor on every draw, because ratatui hides it whenever the frame did not position it.

**Paste has no precedent in any Helix surface.** The editor and the terminal panel both yank to
the system clipboard; neither has a paste verb.

## 5. Target architecture

**One derived ownership fact.** A helper alongside `App::composer_input_mode()` answers "does the
composer hold the keyboard right now", derived from the same inputs the badge uses. It is a
function, not a field. The cursor, the key claim, and the badge all read the same derivation, so
they cannot disagree.

**One mode gate.** The composer claims letters only when the resolved mode is Normal and the
composer holds the keyboard. In Insert, or when another surface holds the keyboard, the composer
claims nothing.

**One claim table, closed and policed.** A single table maps each claimed letter to exactly one
composer verb. The table is the composer's whole Normal-mode surface. It is enforced by a static
guard that fails when a claimed letter has no owner, when a claimed letter can fall through to an
app-level action, or when a letter has two owners. The guard ships with the table, before the verb
milestones extend it.

**A string-backed text model.** The composer's selection reuses `src/tui/helix_selection.rs`
through a small point type carrying a byte offset and a line index. Its undo history is a bounded
pair of edit logs over string ranges, local to the frontend, cleared on submit.

**Shared geometry.** Word and line boundary computation is expressed against a text-source trait
so the editor and the composer compute word boundaries by the same code. This is the reason the
shared selection module exists, applied to geometry.

**An honest cursor.** In Insert mode the composer positions the terminal's own cursor at the
caret, which gives the terminal's native blink with no new cadence. In Normal mode the composer
paints a solid block at the caret and does not position the terminal cursor. When the composer
does not hold the keyboard, it paints nothing and positions nothing, and the terminal cursor stays
hidden. The fake block and the real terminal cursor are mutually exclusive.

**Per-surface claims, shared model.** The editor, the terminal panel, and the composer each own
their own key table, and all three share the selection semantics and the geometry. A future
generalisation of the editor's key-authority guard to cover a sibling surface is explicitly
deferred; this workstream adds a composer-scoped guard, not a unified one.

## 6. Dependency graph

| From | To | Class | Note |
|---|---|---|---|
| M001 | — | — | No dependencies. Derives from existing app state. |
| M002 | M001 | hard | The claim set needs the ownership derivation to know when a claim is live. |
| M002 | ADR-0042 | hard | The claim set is the ADR's decision; the ADR must be accepted first. |
| M003 | M001 | soft | The text model benefits from the ownership derivation but does not need it. |
| M004 | M002 | hard | Motions are reached through the claim table. |
| M004 | M003 | hard | Motions need the geometry and the caret model. |
| M005 | M004 | hard | Selection verbs operate on the motion-derived caret and the selection model. |
| M005 | M003 | hard | Selection verbs need the selection and undo model. |
| M006 | M005 | hard | Undo/redo and insert transitions share the same edit log. |
| M007 | M004, M005, M006 | hard | The consistency sweep runs after the verbs exist. |

M003 has no hard predecessor and may be executed in parallel with M002 now that M001 has closed and
ADR-0042 has settled M002's contract.

## 7. Milestones

### M001 — Composer keyboard ownership and cursor presence

**Class:** capability (C6), with infrastructure (N4).

**Objective.** Derive whether the composer holds the keyboard, and make the composer draw a
cursor that is honest about it: the terminal's own cursor in Insert mode, a solid block in Normal
mode, and nothing at all when another surface holds the keyboard. Fix the unwired focus flag.

**Dependencies.** None.

**Status.** **Closed** 2026-10-19 — `plans/closure/tui-composer-helix-parity/001-status.md`. The
dead `focused` flag was removed rather than rewired, and `TuiMsg::FocusPrompt` went with it.

**Deliverable boundary.** The ownership derivation; the focus flag wired to a real fact; a caret
cell computation shared by the paint and the terminal-cursor positioning; the cursor painting
rules. No Helix verb of any kind.

**Exit conditions.**

- A derived helper answers "does the composer hold the keyboard", and it agrees with the
  `[INS]` / `[NOR]` badge in every state the badge can show.
- In Insert mode with the composer holding the keyboard, the terminal cursor is positioned at the
  caret, including when the composer is empty.
- In Normal mode with the composer holding the keyboard, a solid block is painted at the caret and
  the terminal cursor is not positioned.
- When a side panel, the terminal box, the editor, or a dialog holds the keyboard, the composer
  paints no cursor and positions no terminal cursor.
- The fake block and the terminal cursor are never both visible.
- No new timer or animation cadence exists.

### M002 — Normal-mode key ownership contract

**Class:** invariant (I1, I2, I5), with infrastructure (N3).

**Objective.** Establish the composer's closed claim table, the rule that a claimed key never
falls through, and the re-spellings for the app-level verbs that conflict with it. Ship the static
guard that polices the table.

**Dependencies.** M001 closed; ADR-0042 accepted.

**Status.** **Closed** — closure record
`plans/closure/tui-composer-helix-parity/002-status.md`. The claim table landed with 13 letters
(`b d e i j k o p u U v w x`), the fallthrough rule is enforced in the key chain ahead of the
binding map, the three re-spellings are `Ctrl+Alt+X` (stop/resume), `Ctrl+Alt+D` (open-diff) and
`Ctrl+Alt+E` (focus-prompt) in both maps, and
`scripts/check_tui_composer_key_authority.py` ships with a `--self-test`.

**Deliverable boundary.** The claim table, the fallthrough rule, the re-spelled app-level chords,
the composer-scoped key-ownership guard with its self-test, and the help-surface updates for the
re-spelled chords. No composer text-manipulation verb beyond the ownership mechanism itself — with
the one exception recorded below.

**Exit conditions.**

- Every letter in the claim table has exactly one owner, and the guard fails when one does not.
- A claimed letter with a no-op verb is consumed by the composer and does not reach the app-level
  action; the guard's self-test injects such a leak and proves the guard rejects it.
- The re-spelled chords for turn stop/resume, open-diff, and focus-prompt are present in
  both binding maps and reach their actions.
- Help keeps `space` in both binding maps, and the composer's claim set contains neither `space`
  nor `y`. The copy verb itself lands with the selection it copies, in M005.
- The re-spellings are reachable by a chord that is not a bare letter, so the composer's claim set
  cannot take them back.
- `h` and `l` still move the composer caret, and `j`/`k` still scroll the transcript while the
  composer buffer is a single line.
- A claimed uppercase letter is reachable in both the shifted and the unshifted encoding.
- Every letter in the claim set is still typed as text in Insert mode, in both keymaps.

### M003 — Composer text model

**Class:** infrastructure (N1, N2).

**Objective.** Give the composer a selection, a bounded undo/redo history, and word/line geometry
that is shared with the editor rather than reimplemented.

**Dependencies.** None hard. Soft on M001.

**Deliverable boundary.** A composer point type satisfying the shared selection trait; the
composer's selection state; a bounded undo/redo edit log; word and line geometry expressed against
a shared text source, with the editor migrated onto it. No key handling, no rendering change.

**Exit conditions.**

- The composer's selection uses the shared selection state machine and satisfies the shared
  selection tests against an independent point type.
- Word boundaries computed by the shared geometry are identical for the editor and the composer,
  and the shared policy function is the only word-segmentation rule in the tree.
- The undo history is bounded in depth and in bytes, and a bounded eviction does not corrupt the
  history.
- Undo and redo are inverse operations over the composer's buffer, including over a selection
  deletion.
- Submitting or replacing the composer clears the undo history, so undo cannot resurrect text that
  was already sent.

### M004 — Composer motions

**Class:** capability (C1).

**Objective.** Make the composer caret navigable in Normal mode: horizontally, vertically, and by
word.

**Dependencies.** M002 closed; M003 closed.

**Deliverable boundary.** The motion verbs and their key claim entries. No selection verbs, no
undo, no insert transitions.

**Exit conditions.**

- Each motion is reachable by pressing its key through the app's real key path, and each is
  verified by a dispatch-reachability test rather than by calling a helper.
- Motions respect the composer's multi-line text and its scroll offsets.
- A motion that has nowhere to go leaves the caret unchanged and consumes the key.
- A key the composer does not claim behaves exactly as it did before this workstream.

### M005 — Composer selection and clipboard verbs

**Class:** capability (C2, C3).

**Objective.** Let the user select characters and lines in the composer, see the selection, delete
it, yank it, and paste it back.

**Dependencies.** M004 closed; M003 closed.

**Deliverable boundary.** Character selection, line selection, delete-selection, the yank
capability, paste from the clipboard, and the selection rendering. The yank's **key** is not a
composer letter: it is `y` in the help box the `space` chord opens, as ADR-0042 part 7 requires,
so the help box's route-carrying and its verb belong here too. Undo/redo verbs are **not** in
this milestone.

**Exit conditions.**

- Character and line selection are entered, extended, and cancelled, and the selection is
  rendered using the theme's selection colours.
- Deleting a selection with nothing selected is a no-op that still consumes the key.
- Yank copies exactly the selected text, and nothing else, to the system clipboard.
- The yank is reached by pressing `space` and then `y` in the help box that opened, and the
  `/help` command's box does not copy.
- Yank with no selection reports that there is nothing to copy and changes nothing.
- Paste inserts the clipboard contents at the caret, and is a no-op with an empty clipboard.
- The composer is the first surface with a paste verb; the editor and the terminal panel are
  unchanged.

### M006 — Composer undo, redo, and insert transitions

**Class:** capability (C4, C5).

**Objective.** Let the user undo and redo composer edits, and enter insert mode before or after
the caret, or on a new line, from Normal mode.

**Dependencies.** M005 closed; M003 closed.

**Deliverable boundary.** Undo, redo, insert-before, insert-after, and open-line-below, plus the
mode transitions they cause. No new text model.

**Carried forward from M002.** The `i` claim's verb is already **live**: M002 had to make it so,
because `i` is the app's only route back to Insert mode and a reserved `i` would have stranded
the user in Normal mode. What it runs is the app's **existing** transition — insert before the
caret, the caret unmoved — moved into the claim table. M006 still owns the rest of this
milestone's boundary: insert-after (`a`), open-line-below (`o`), the caret placement each
transition implies, and undo/redo. M002's closure records the deviation.

**Exit conditions.**

- Undo and redo traverse the composer's edit history in both directions, including deletions
  performed by a selection delete.
- The history is bounded, and the bound is enforced without losing the ability to undo the most
  recent edit.
- A new edit after an undo discards the redo tail, as a user expects.
- Undo cannot resurrect text that was submitted; the history is cleared on submit.
- Entering insert mode places the caret correctly for each of the three transitions, and the
  badge and the cursor both follow.
- Redo is reachable in both the shifted and the unshifted encoding of its capital letter, so it
  works on terminals that report a shifted key without the `SHIFT` modifier.

### M007 — Keymap consistency and guard coverage

**Class:** polish (P1, P2), closing sweep.

**Objective.** Make the composer's keymap discoverable and consistent, and close the remaining
gaps between the composer, the editor, and the terminal panel.

**Dependencies.** M004, M005, M006 closed.

**Deliverable boundary.** The composer hint row, the user documentation, the architecture
documentation, the TUI skill update, and the consistency sweep across the three Helix surfaces.

**Exit conditions.**

- The composer hint row shows the currently available Normal-mode verbs, and it does not claim
  verbs that another surface holds.
- `docs/tui.md` documents the composer Normal-mode keymap, the re-spelled app-level chords, and
  the cursor rules.
- `architecture/tui.md` records the composer's ownership boundary, the claim-table rule, and the
  relationship to the shared selection and geometry assets.
- The three surfaces agree on selection semantics and word boundaries, and any divergence found
  is either fixed or recorded as a known difference.
- The help audit and the modifier-chord audit pass with the complete claim set.

## 8. Cross-cutting requirements

### Storage and migration

None. The composer's text, selection, and undo history are frontend-local and are not persisted by
this workstream. The composer's submitted text is unaffected.

### Protocol and compatibility

No daemon message, session state, or wire format changes. The composer reaches the daemon only
through the existing submit path.

The user-visible key changes are the re-spellings decided by ADR-0042, and the change of meaning
of `x`, `d`, and `i` in the main buffer. `space` is **not** among them: help keeps its chord.
These are recorded in the ADR and must be
stated in the user documentation in the same change that removes the old spellings.

### Security and authorization

The composer is a frontend buffer. No milestone in this workstream changes turn admission, tool
authority, sandboxing, or what the daemon accepts.

Clipboard exposure is the one real surface: the composer's yank copies user-selected composer text
to the system clipboard. It is user-initiated, bounded to the selection, and must not log
clipboard contents. The composer's undo history is local presentation state and must never reach
the daemon.

### Concurrency, cancellation, and recovery

The composer is single-owner and frontend-local; there is no cross-process contention. The
relevant interactions are with the app's own key-dispatch chain: the composer must not claim a key
that a higher-priority surface (an open dialog, the terminal box, a focused panel) is already
handling. This is enforced by the ownership derivation and by the claim table, not by locking.

### Observability and audit

No new audit event. Compositor edits are not auditable actions. The existing turn submission
audit is unchanged.

### Performance and resource use

The undo history is bounded in depth and in bytes, mirroring the editor's bounds. Word and line
geometry must not be quadratic in buffer length on ordinary input; the editor's implementation is
the reference for acceptable cost.

### Documentation and static guards

- `docs/tui.md` gains a composer keymap section. It is updated per milestone as verbs land, not
  at roadmap close.
- `architecture/tui.md` records the composer's ownership boundary and the claim-table rule.
- `.opencode/skills/tui/SKILL.md` is updated with the composer dispatch boundary.
- A composer-scoped key-ownership guard is added to `scripts/` and wired into
  `scripts/verify.sh quick` and CI, with a `--self-test` that injects a leak and proves
  rejection.
- The existing editor guards are not modified.

## 9. Verification strategy

**Static guards.** The composer key-ownership guard is the primary structural check. It proves the
claim table is closed, single-owner, and non-leaking, and that the re-spelled app-level chords
exist in both binding maps. Its `--self-test` injects a leak, an unowned claim, and a
double-ownership and proves each is rejected.

**Dispatch-reachability tests.** Every claimed letter is pressed as a real key event through the
app's key path, and the composer is asserted to own it. This is the discipline the editor
milestones established after a dead binding survived three direct-call tests. A verb that is only
reachable by calling a helper does not count.

**Negative tests.** In the main buffer with the composer holding the keyboard, `d` does not open
the diff, `x` does not stop the turn, and `j`/`k` still scroll the
transcript while the composer buffer is a single line. The re-spelled chords do reach their
actions.

**Help-chord tests.** `space` still opens the help menu. `y` in the menu the `space` chord
opened copies the composer's selection; the `/help` command's box does not carry that verb.

**Cursor tests.** The composer draws a terminal cursor in Insert mode, a solid block in Normal
mode, and neither when another surface holds the keyboard; the fake block and the terminal cursor
are never both visible; an empty composer in Insert mode still shows a cursor.

**Insert-mode regression.** The existing guarantee that a bare character in Insert mode is typed
rather than interpreted as an app action must continue to hold for every letter in the claim set.

**Shared-asset tests.** The shared selection and geometry assets are exercised against both the
editor's and the composer's point types, so a change to one surface's semantics cannot silently
diverge from the other's.

**Command surface.** `scripts/verify.sh quick` for the routine subset, `scripts/verify.sh full`
for clippy and the workspace suite. No `--all-features` sweep: it drags in the real-server tests.

## 10. Risks and decision points

### Decision points

| Point | Status | Owner |
|---|---|---|
| D1 — Does the app's single `InputMode` axis become the composer's navigation mode, or does the composer get its own axis? | Decided in ADR-0042 (Option A), **accepted 2026-10-19** | Maintainers |
| D2 — Which app-level chords are re-spelled, and to what? | ADR-0042 sets the rule and the conflict list; the implementing milestone owns the exact chords. Help is **not** re-spelled — the amendment keeps `space`. **Settled in M002**: `Ctrl+Alt+X` (stop/resume), `Ctrl+Alt+D` (open-diff), `Ctrl+Alt+E` (focus-prompt), in both maps. `Ctrl+Alt+I` is not deliverable — crossterm reports `ESC` + `0x09` as `Tab`+ALT — and the default map's bare `/` for focus-prompt was removed with the rest. | Maintainers, in M002 |
| D3 — Is the blinking cursor the terminal's own cursor or an app-driven cadence? | Open. Constrained by I7 and the single-cadence rule. Recommended: the terminal's own cursor. | Maintainers, in M001 |
| D4 — Is the composer's undo scoped to the prompt text, or to the wider composer state including slash commands and staged shell input? | Open. Recommended: prompt text only, matching the composer's existing buffer boundary. | Maintainers, in M003 |
| D5 — Does the composer introduce a paste verb alone, or do the editor and terminal panel gain it too? | Open. Recommended: composer alone in this workstream, with a recorded follow-up for the other two, because the composer is a different buffer with a different boundary. | Maintainers, in M005 |

### Risks

- **R1 — The re-spellings are a user-visible regression.** Turn stop/resume, open-diff, and
  focus-prompt move to non-letter chords. Users who learned the old ones will hit them and find
  nothing. Help does **not** move, which removes the sharpest instance of this risk.
  Mitigation: the help surface lists the new chords in the same change; the change is recorded in
  ADR-0042 rather than discovered.
- **R1a — The copy gesture is two keys and a menu.** `space` then `y` is more friction than one
  chord, and the menu in between is not where a user looks for a clipboard action. This is the
  recorded price of keeping help on `space`. Mitigation: the help box names the verb, and the
  verb reports its outcome rather than failing silently.
- **R2 — A partially implemented claim set leaks.** The largest correctness risk in this
  workstream. Mitigation: the guard ships with the table, before the verbs, and the guard fails
  when a claimed letter has no owner or can fall through.
- **R3 — The cursor and the badge disagree.** A cursor shown while another surface holds the
  keyboard, or a cursor shape that does not match the badge, teaches the wrong keymap.
  Mitigation: both read the same derivation; the cursor tests assert agreement.
- **R4 — Geometry drift between the editor and the composer.** Reimplementing the editor's word
  functions produces two answers to "where is the next word boundary".
  Mitigation: the shared text-source extraction in M003, with both surfaces on it.
- **R5 — The two-cursor conflict.** Painting the fake block while also positioning the terminal
  cursor shows two cursors. Mitigation: the paint and the positioning are mutually exclusive by
  construction and asserted by a test.
- **R6 — The `focused` flag is rewired wrongly.** The flag is currently dead; a milestone that
  wires it to a stored value rather than the derived ownership fact reintroduces the drift class
  that `composer_input_mode()` was written to kill. Mitigation: the derivation is a function, and
  the flag is not used as the gate.
- **R7 — Scope creep toward Helix parity.** The editor roadmap already tracks multi-selection,
  text objects, match mode, and goto mode. Mitigation: the non-goals section; the composer's
  claim set is fixed and small.

## 11. Completion definition

This workstream is complete when all of the following hold.

- The composer, in Normal mode with the keyboard, supports horizontal, vertical, and word
  motions; character and line selection; delete, yank, and paste of a selection; undo and redo;
  and insert transitions.
- The composer shows a blinking cursor in Insert mode, a solid cursor in Normal mode, and no
  cursor when it does not hold the keyboard, and the cursor always agrees with the badge.
- The composer's claim table is closed, single-owner, non-leaking, and policed by a static guard
  with a self-test.
- The re-spelled app-level chords are present in both binding maps, are reachable, and are
  documented.
- Every claimed letter is verified by a dispatch-reachability test through the real key path.
- The composer's selection and word boundaries are the shared assets' behaviour, not a second
  implementation.
- The user documentation, the architecture documentation, and the TUI skill describe the
  composer's keymap, its cursor rules, and its ownership boundary.
- No new timer or animation cadence was introduced.
- No milestone remains open, blocked, or conditionally closed in `plans/registry.md` without a
  recorded reason.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | **closed** | `plans/implementation/tui-composer-helix-parity/001-composer-keyboard-ownership-and-cursor-presence.md` | `plans/closure/tui-composer-helix-parity/001-status.md` | none |
| M002 | **closed** | `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md` | `plans/closure/tui-composer-helix-parity/002-status.md` | none |
| M003 | **eligible for fresh planning** | — | — | none |
| M004 | **not started** | — | — | M002, M003 |
| M005 | **not started** | — | — | M004, M003 |
| M006 | **not started** | — | — | M005, M003 |
| M007 | **not started** | — | — | M004, M005, M006 |

**Reading this table.** `closed` means a closure record was accepted. `ready` means the
implementation plan is written and its dependencies are discharged. `blocked` means a named
dependency — a decision or an unclosed milestone — prevents progress. `eligible for fresh planning`
means the milestone has no outstanding dependency and needs a planning pass before an implementation
plan can be written. `not started` means the milestone is sequenced behind milestones that have not
closed, and no implementation plan has been written for it; it does **not** mean the milestone is
blocked by an external condition. M003 is eligible for fresh planning and may be taken at any time.

**M002's one scoping deviation, carried forward.** M002's plan put "no insert transition" out of
scope and said "do not add verbs here". The `i` claim's verb is **live** in M002, because `i` is
the app's **only** route back to Insert mode — it is the `None` arm of the key chain, not a
binding — so a reserved `i` would have consumed the key, done nothing, and left the user in Normal
mode with no way to type. The verb that runs is the app's **existing** transition, moved into the
claim table so the table is the one place that says who owns the key. M006 still owns the insert
transitions *beyond* `i` (`a`, `o`) and the caret placement they imply; this is a phasing
deviation, recorded in `plans/closure/tui-composer-helix-parity/002-status.md` §1.1.

Status labels here must match `plans/registry.md`. The registry carries M001 and M002 under
**Dependency-ready implementation plans** (both `closed`, each with its closure record), and M003
under **Milestones eligible for fresh planning**; M004–M007 are registered in the registry only
through this table, because they have no implementation plan and are not yet dependency-ready.
