# TUI Composer Helix Parity Milestone 006 — Composer Undo, Redo, and Insert Transitions

Status: implemented — closed at `plans/closure/tui-composer-helix-parity/006-status.md`
to 14 (`a` is one of the letters roadmap §3 names), and the vim map's bare `a` → `FocusSidebar`
binding is deleted because `FocusSidebar` already answers to `Ctrl+Alt+S` in both maps

Repository baseline: branch `feat/auto-20261019-8cc543fb`, tip `3cad45929`, **plus** the
uncommitted M004 and M005 work. M004 and M005 closed on 2026-10-19 but are not committed; the
implementing agent must confirm the M005 closure tree is in place before starting — the claim
table's `w`, `b`, `e`, `j`, `k`, `v`, `x`, `d` and `p` rows are `Live`, and
`ComposerText::selection_range` exists — and work from that tree. If the M004/M005 work is
committed first, this baseline line is superseded by that commit.

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m006--composer-undo-redo-and-insert-transitions`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — the
  composer's claim set, the one-owner rule, and the re-spelling rule for a conflicting letter.
  **Part 4's conflict table does not list `a`.** That is decision point **D6**, and this
  milestone resolves it *without editing the ADR*: an accepted ADR is an immutable historical
  record, so the resolution is recorded in the roadmap's decision table and in this milestone's
  closure record, and the ADR is left exactly as accepted.

Primary class: capability (roadmap C4 and C5, undoable composer and insert transitions).

## 1. Objective

Let the user undo and redo composer edits, and enter insert mode before or after the caret, or
on a new line, from Normal mode.

- `u` undoes the newest composer edit; `U` redoes it.
- `a` enters insert mode **after** the character under the caret.
- `o` opens a line **below** the caret's line and enters insert mode there.
- `i` — already live since M002 — enters insert mode **before** the caret, and does not move it.

## 2. Why this milestone is ready

- **M005 is closed** (`plans/closure/tui-composer-helix-parity/005-status.md`). The selection
  verbs, the copy gesture and the paste verb landed; the composer's selection model and the
  shared selection range exist.
- **M003 is closed** (`plans/closure/tui-composer-helix-parity/003-status.md`). The composer's
  text model exists, including the bounded undo/redo history
  (`src/tui/composer_text.rs:122`, `:129`, `:143`) and the `undo`/`redo` entry points
  (`src/tui/composer_text.rs:593`, `:604`).
- **M004 is closed** (`plans/closure/tui-composer-helix-parity/004-status.md`). The motions exist
  and move the caret through `move_caret`, the shared machine's `move_to`.
- The roadmap's dependency line for M006 is "M005 closed; M003 closed"
  (`plans/subsystems/tui-composer-helix-parity-roadmap.md:465`). Both are satisfied.

Nothing else is outstanding. **D4 and D6 are owned by this milestone** (roadmap §10, `:613` and
`:615`) and are settled in §5 below.

## 3. Current implementation evidence

**The claim table has the verbs; it does not have the keys.** `COMPOSER_CLAIMS`
(`src/tui/composer_claims.rs:282`) holds **13 rows**. Three of them are the verbs this milestone
implements, and all three are `Reserved` — the key is consumed and nothing happens:

| Key | Verb | Row | Status |
|---|---|---|---|
| `o` | `ComposerVerb::OpenLineBelow` | `src/tui/composer_claims.rs:340` | `Reserved` |
| `u` | `ComposerVerb::Undo` | `src/tui/composer_claims.rs:354` | `Reserved` |
| `U` | `ComposerVerb::Redo` | `src/tui/composer_claims.rs:361` | `Reserved`, `CapitalBoth` |

`ComposerVerb` (`src/tui/composer_claims.rs:70-97`) has **13 variants** and **no `InsertAfter`** —
`a` is a verb this milestone introduces, and it needs a new variant.

**`a` is in the roadmap's target set and is not in the table.** Roadmap §3 (`:118-120`) names the
workstream's verbs: "motions, `v`, `x`, `d`, `u`, `U`, `o`, `i`, `a`, and the copy gesture
`space` then `y`". `a` is one of them. The table holds 13 of those 14 letters; `a` is the one it
is missing, and the reason is a conflict the table does not record.

**The conflict is real and it is one-sided.** The **vim map** binds bare `a` to
`InputAction::FocusSidebar` (`src/tui/input.rs:1199-1202`). The **default map** does not bind
bare `a` at all — it binds `Ctrl+a` to `SwitchAgent` (`src/tui/input.rs:957-960`), and the vim map
binds the same chord (`src/tui/input.rs:1273-1276`).

**The capability is not lost by removing the bare letter.** `FocusSidebar` is already bound to
`Ctrl+Alt+S` in **both** maps: the default map at `src/tui/input.rs:940-946` and the vim map at
`src/tui/input.rs:1219-1225`. The roadmap's D6 row (`:615`) says the resolution "is the one M002
applied to `x`, `d` and `i` — `FocusSidebar` moves to a chord in **both** maps". Measured at this
baseline, the move has **already happened**: the chord exists in both maps, and the bare `a` is
an *extra* spelling of an action that already has one. Removing it loses no capability. D6's
wording is therefore satisfied by **deleting one binding**, not by inventing a chord.

**The help advertises the key this milestone takes.** `src/tui/input.rs:1630-1635` is a Normal-mode
help row: key `a`, action "Focus sidebar (agent tree)", condition "vim mode". Once `a` is
claimed, that row is a lie — it teaches a key the composer now owns. It is the only place in the
tree that documents bare `a` as a sidebar gesture.

**The model already has the history, and the history has two defects.**

- `UndoHistory` is bounded in **both** depth and bytes: `MAX_UNDO_DEPTH = 200`
  (`src/tui/composer_text.rs:122`) and `MAX_UNDO_BYTES = 64 * 1024`
  (`src/tui/composer_text.rs:129`).
- `UndoHistory::push` (`src/tui/composer_text.rs:170-175`) clears the redo stack before recording,
  so **a new edit after an undo discards the redo tail** already. ✓
- `UndoHistory::evict` (`src/tui/composer_text.rs:219-230`) removes the **oldest** entry while
  either bound is exceeded. A single edit larger than `MAX_UNDO_BYTES` — a paste, which is one
  edit — is therefore evicted **immediately**, and the paste becomes **not undoable**. The
  roadmap's exit condition requires "the bound is enforced **without losing the ability to undo
  the most recent edit**". This is a defect in the model as it stands, and it is in this
  milestone's scope because this milestone is what makes undo reachable at all.
- `ComposerText::undo` (`:593-601`) and `redo` (`:604-612`) mutate `self.text` directly and
  restore the caret through `set_caret`. They do **not** touch the held selection — and
  `SelectionState::set_cursor` (`src/tui/helix_selection.rs:188-190`) is documented as
  "Relocate the cursor without touching the selection". So an undo that shortens the buffer
  leaves a selection whose byte coordinates were computed against the *old* text. The range
  clamps (`ComposerText::selection_range`), so nothing panics — but the selection covers the wrong
  text.
- `ComposerText::replace_range` (`src/tui/composer_text.rs:615-627`) — the recording path every
  edit goes through — also leaves the selection alone. `delete_selection` (`:570-577`) is the one
  mutator that deactivates it explicitly, and it does so *before* the edit.

**The two wholesale-replacement paths drop the history, which is what makes D4 decidable.**
`ComposerText::set_text` (`:311-316`) and `ComposerText::clear` (`:319-323`) both clear the
history. The app reaches them at exactly the boundaries where the composer stops being the
user's:

- Submit: `self.prompt_state.prompt.clear()` (`src/tui/app/prompt_turn.rs:320`, and the slash,
  shell, chat and addressed branches in the same file).
- A staged shell projection: `app.prompt_state.prompt.set_text(include_text)`
  (`src/tui/commands/shell.rs:655`).
- `/shell-ask` seeding: `self.prompt_state.prompt.set_text(format!("/shell-ask {}", id))`
  (`src/tui/app/input.rs:1073-1077`).

**The insert transitions that exist.** `i` is live (`src/tui/composer_claims.rs:304-325`) and
runs the app's existing transition — `self.ui_state.input_mode = InputMode::Insert`
(`src/tui/app/mod.rs:12855-12861`), with the caret unmoved. The same gesture also survives in
the `None` arm of the key chain (`src/tui/app/mod.rs:4338-4345`) for the surfaces the composer
does not own.

**The claim dispatch site.** `App::on_key` consults the table before the bindings, at
`src/tui/app/mod.rs:4029-4041`: a claimed key is **consumed** here, and `run_composer_claim`
(`src/tui/app/mod.rs:12850`) runs the verb.

## 4. Invariants that must not regress

- **I1 — one owner per key.** A claimed letter reaches the composer and nothing else. The guard
  `scripts/check_tui_composer_key_authority.py` is the enforcement (rule G2, the fallthrough
  rule).
- **I2 — a consumed claim that does nothing is a dead key.** Every verb this milestone makes
  live must actually do something, in every state it can be reached in.
- **I3 — the claim table is closed and single-owner.** The table is the one place that says who
  owns a key; the app's verb lives in `run_composer_claim`, not in a binding.
- **I4 — Insert mode types bare letters.** A claimed letter is ordinary text in Insert mode
  (`src/tui/input.rs`'s `is_bare_char` rule). Claiming `a`, `o`, `u` and `U` must not make any
  of them untypeable.
- **I5 — no new redraw cadence.** No timer, no poller, no animation tick. The blinking cursor
  remains the terminal's own. A verb that moves the caret is picked up by the next frame.
- **I6 — the undo history is frontend-local.** It must never reach the daemon, and the undo
  stack must never be serialized into a turn.
- **I7 — the badge and the cursor agree.** After any transition the `[INS]`/`[NOR]` badge and the
  painted/terminal cursor must describe the same mode and the same caret position.
- **I8 — a relative path is relative to the session's workspace root.** M006 touches no path
  handling; the invariant is listed so a later milestone does not read this plan's silence as
  permission.

## 5. Scope

### In scope

- The `u` and `U` verbs: undo and redo, reachable by pressing the key.
- The `a` verb: insert-after, **including** the `a` claim row and the removal of the vim map's
  bare `a` → `FocusSidebar` binding and the help row that advertises it.
- The `o` verb: open-line-below.
- The caret placement each transition implies, owned by the **model** rather than the app.
- The selection rule for text-changing edits, and the undo/redo selection invalidation.
- The `UndoHistory` eviction fix, so the newest edit is always undoable.
- The guard update: `FocusSidebar` joins `RESPELLED_ACTIONS`, and the guard's self-test covers the
  new violation class.
- The decision records for **D4** (the undo scope) and **D6** (the `a` conflict), in the
  roadmap's §10 decision table and in this milestone's closure record.

### Out of scope

- **Line start/end motion** — decision **D7** (roadmap `:616`) stays open. `0` and `$` are not
  letters, and claiming them would change ADR-0042 part 2's closed claim set. `Home`/`End` keep
  reaching the **buffer's** start/end.
- **The help surface's composer section and the hint row.** M007 owns discovery (C7, P1, P2).
  M006's only help obligation is to **remove the row that becomes false** (§7, WP1) and to list
  the verbs it adds; M007 then derives the whole section from the claim table and audits it.
- **The editor's and the terminal panel's keymaps.** Roadmap §3 (`:123-124`): "Those surfaces
  keep their current behaviour exactly."
- **A configurable keymap** (roadmap `:125-126`).
- **The daemon protocol, the turn submission path, the prompt queue, the human-shell staging
  model** (roadmap `:127-128`). M006 reads the staging paths; it does not change them.
- **A transcript selection or message-level navigation** (roadmap `:129-130`).
- **Editing ADR-0042.** An accepted ADR is immutable. D6's resolution is recorded elsewhere.

## 6. Required production changes

| Area | File | Change |
|---|---|---|
| Model | `src/tui/composer_text.rs` | `insert_after`, `open_line_below`; the selection rule in `replace_range`; selection invalidation in `undo`/`redo`; the eviction fix |
| Widget | `src/tui/components/prompt.rs` | `insert_after`, `open_line_below`, `undo`, `redo` accessors |
| Claims | `src/tui/composer_claims.rs` | `ComposerVerb::InsertAfter`; the `a` row; `o`/`u`/`U` → `Live` |
| App | `src/tui/app/mod.rs` | `run_composer_claim` arms for `Undo`, `Redo`, `InsertAfter`, `OpenLineBelow` |
| App | `src/tui/app/input.rs` | no change expected — the arms live in `run_composer_claim` (I3) |
| Bindings | `src/tui/input.rs` | delete the vim map's bare `a` → `FocusSidebar`; delete the false help row at `:1630-1635` |
| Guard | `scripts/check_tui_composer_key_authority.py` | `"FocusSidebar": "a"` in `RESPELLED_ACTIONS`; a self-test case for the new class |

## 7. Ordered work packages

### WP1 — Settle D6: take `a`, and keep the sidebar reachable

**Intent.** `a` becomes the composer's insert-after verb, and `FocusSidebar` keeps a reachable
spelling in every map.

**Required changes.**

1. **Delete the vim map's bare `a` binding** (`src/tui/input.rs:1199-1202`). The action keeps
   `Ctrl+Alt+S` in both maps, which is already there (default `:940-946`, vim `:1219-1225`).
   Nothing is lost: the capability was already reachable by a chord in both maps before this
   milestone.
2. **Leave the `Ctrl+Alt` chord comment alone.** The comment above the vim map's chord
   (`src/tui/input.rs:1216-1218`) reads "See the CONTROL|ALT chord contract in
   `default_bindings_internal` for why this is not `Ctrl+Alt+S`", and it reads *oddly* next to a
   binding the house labels `Ctrl+Alt+S` — but it is **substantively accurate**, and correcting
   it here would be wrong. The chord family is deliberately `Ctrl+Alt+<lower>`: the terminal
   delivers `ESC` followed by the single control byte, so crossterm reports the **unshifted**
   letter with `CONTROL|ALT`, and the shifted spelling is unreachable in a legacy terminal
   (`src/tui/input.rs:1186-1191` makes exactly this point for `Ctrl+Alt+E`, and
   `tests/modifier_chords.rs` asserts the whole `Ctrl+Alt+<lower>` set). The comment is
   distinguishing the *shifted* spelling from the deliverable one. **M006 removes the bare `a`
   binding and does not touch this comment** — the chord family's spelling is the chord
   contract's business, and "the comment is confusing" is not a finding this milestone may act
   on.
3. **Delete the false help row** at `src/tui/input.rs:1630-1635` (`a` → "Focus sidebar (agent
   tree)", condition "vim mode"). After this milestone the composer owns `a`, so the row
   advertises a gesture that does not exist. The `Ctrl+Alt+S` → "Focus sidebar" row already
   exists at `src/tui/input.rs:1476-1481`.
4. **Add `"FocusSidebar": "a"` to `RESPELLED_ACTIONS`**
   (`scripts/check_tui_composer_key_authority.py:82-86`). This is what makes the re-spelling
   *enforced* rather than asserted: G4 then requires the action to be bound in both maps, and G5
   requires it to be reachable by a chord in both and to have no spelling that is a claimed key
   or the old bare letter.
5. **Add the `a` row** to `COMPOSER_CLAIMS`: key `'a'`, verb `ComposerVerb::InsertAfter`,
   status `Live`, encodings `Bare`, condition `Always`. The table goes from **13 rows to 14**.
   The row's position in the literal is alphabetical, so `a` goes first.
6. **Add `ComposerVerb::InsertAfter`** to the verb enum (`src/tui/composer_claims.rs:70-97`),
   documented as Helix insert-after.

**Acceptance evidence.**

- `python3 scripts/check_tui_composer_key_authority.py` passes with the new row and the new
  `RESPELLED_ACTIONS` entry.
- `python3 scripts/check_tui_composer_key_authority.py --self-test` passes **and** the self-test
  gains a case for the new class: a claim on `a` with the vim map still binding bare `a` to
  `FocusSidebar` must be **rejected** (G2), and a `FocusSidebar` spelling that is a bare letter
  must be rejected (G5). A guard change without a self-test case proves nothing.
- A test that `FocusSidebar` is still reachable by `Ctrl+Alt+S` in **both** maps, in Normal mode.
  This is the "no capability lost" half of the re-spelling, and it is the half a
  "the binding is gone" test would miss.
- A test that `a` is **not** reachable as `FocusSidebar` in the vim map any more.

**Recorded, not done here.** ADR-0042 part 4's conflict table does not list `a`. The ADR is
accepted and therefore immutable; the gap and its resolution go into the roadmap's D6 row
(status: closed in M006) and into this milestone's closure record. §11's closure evidence lists
this as a required record.

### WP2 — The undo and redo verbs

**Intent.** `u` and `U` traverse the composer's edit history in both directions.

**Required changes.**

1. **The model already has the verbs.** `ComposerText::undo` (`src/tui/composer_text.rs:593`) and
   `redo` (`:604`) exist and restore the caret to `caret_before`/`caret_after`. This milestone
   makes them reachable and fixes what they get wrong.
2. **Invalidate the held selection on undo and redo.** Both change the buffer's length, so the
   selection's byte coordinates — computed against the pre-edit text — no longer describe
   anything. `SelectionState::set_cursor` deliberately leaves the selection alone
   (`src/tui/helix_selection.rs:188-190`), so the invalidation must be explicit. The rule is the
   same one `delete_selection` already follows (`:570-577`): **deactivate before the edit**.
3. **Make the eviction rule keep the newest edit.** `UndoHistory::evict`
   (`src/tui/composer_text.rs:219-230`) must never remove the **last remaining** entry. The
   byte bound is then enforced against everything except the newest edit, and the newest edit may
   exceed the bound on its own. **The trade-off is explicit and is the point of the rule:** the
   composer holds at most `MAX_UNDO_BYTES` **plus one edit's weight**, which is at most one more
   copy of the text the user just pasted. The alternative — evicting the oversized paste — makes
   the user's most recent action unundoable, which is the failure the roadmap's exit condition
   names. The bound must not be enforced by truncating an edit's recorded text either: an undo
   that restored truncated text would be a silent corruption, which is worse than an oversized
   history.
4. **Add the widget accessors** `PromptWidget::undo` and `PromptWidget::redo`
   (`src/tui/components/prompt.rs`), named consistently with the existing `cursor_left` /
   `cursor_right` family. They return `bool` — whether anything was undone or redone — so the
   app can report the no-op case.
5. **Add the `run_composer_claim` arms** (`src/tui/app/mod.rs:12850`).
6. **Flip the claims.** `u` (`:354`) and `U` (`:361`) go `Reserved` → `Live`. `U` keeps
   `ClaimEncodings::CapitalBoth` (`src/tui/composer_claims.rs:164-171`): `Char('U')` with
   `SHIFT` **and** with `NONE`. Some terminals report a shifted key without the `SHIFT`
   modifier, and a redo that only answers to one encoding is a redo that works on some machines.
7. **Report in both branches.** `u` with nothing to undo reports "Nothing to undo"; `U` with
   nothing to redo reports "Nothing to redo". This mirrors M005's paste rule ("a verb's silence
   is never ambiguous") and it is the same reason the copy reports the **size** of the
   selection rather than its text: the toast is retained for `/logs`, and the composer can hold
   a secret. Neither undo nor redo may put the text in a message.

**Acceptance evidence.**

- A dispatch-reachability test: type into the composer, press `u` through `App::on_key`, and
  assert the text is back to its previous value. **Not** a call to `ComposerText::undo` — the
  M001 editor's dead-binding lesson is that a helper call proves nothing about the key path.
- The same for `U`, under **both** encodings (`Char('U')` with `SHIFT` and with `NONE`).
- Undo of a **selection delete** restores the deleted text (the roadmap's exit condition names
  it explicitly).
- A new edit after an undo discards the redo tail: undo, type, and `U` is a no-op.
- Undo **cannot resurrect submitted text**: type, submit, and `u` leaves the composer empty and
  reports that there is nothing to undo.
- A paste **larger than `MAX_UNDO_BYTES`** is still undoable — the eviction regression test.

### WP3 — The insert transitions

**Intent.** `a` and `o` enter insert mode with the caret where Helix puts it.

**Required changes — the caret rules live in the model.**

- **`i` (insert-before) does not change.** It is already live and it does not move the caret:
  the next character is typed at the caret. This milestone must not "improve" it.
- **`a` (insert-after).** The insertion point is the next character boundary after the caret,
  **clamped to the end of the caret's line**, so `a` never carries the caret past a line
  terminator into the next line. At the end of a line there is no character under the caret, so
  the caret does not move and the insertion point is the line's end.
  - Implement as `ComposerText::insert_after(&mut self)`: compute the point, move the caret
    there, and return the offset it landed on.
  - The caret move is **not an edit**: it is a movement, so it is not recorded in the history.
    It must use the machine's deliberate-movement path (`move_to`, i.e. `move_caret`) rather than
    `set_caret`, for the same reason M004's motions do — a deliberate movement in character mode
    extends a held selection, and `set_caret` would silently collapse it.
- **`o` (open-line-below).** A newline is inserted at the **end of the caret's line**, and the
  caret lands on the new line.
  - Implement as `ComposerText::open_line_below(&mut self)`: find the caret's line, find that
    line's content end through the **shared** geometry
    (`text_geometry::line_content_end`), insert `"\n"` there through the recording
    `replace_range`, and leave the caret just after the inserted newline.
  - This **is** an edit, so it is recorded and is undoable. `o` then `u` must remove the line
    again.
  - `o` on a **single-line** buffer works: it creates the second line. It must **not** carry
    `ClaimCondition::MultiLineBuffer` — that condition exists for `j`/`k` because those keys
    fall through to transcript scroll, and `o` has no fall-through.
- **Both** transitions set `InputMode::Insert`. The app does that, and nothing else: the model
  owns the caret, the app owns the mode.

**Acceptance evidence.**

- `a` on "abc" with the caret on 'a' leaves the caret after 'a'; with the caret on 'c' it
  leaves the caret at the line's end; with the caret at the line's end it does not move.
- `a` in a **multi-line** buffer does not carry the caret onto the next line.
- `o` on a single-line buffer produces two lines with the caret on the second.
- `o` on the **last** line of a multi-line buffer produces a new last line with the caret on it.
- `o` then `u` restores the original text (the newline is an undoable edit).
- `a` and `o` are reachable by pressing the key through `App::on_key`, and each ends in
  `InputMode::Insert`.
- The `i` transition still does not move the caret (a regression guard on M002's behaviour).

### WP4 — The selection rule for text-changing edits

**Intent.** One rule, in one place: **an edit that changes the buffer's length drops the held
selection.**

**Why it belongs here.** `ComposerText::replace_range` (`:615-627`) — the recording path behind
`insert`, `backspace`, `delete_forward`, `delete_selection` and `open_line_below` — leaves the
selection alone. So today a user can arm a character selection with `v`, press `i`, type a
character, and keep a selection whose byte coordinates were computed against the **pre-typing**
text. The range clamps, so nothing panics, but the painted range now covers the wrong text.
`delete_selection` already deactivates explicitly (`:574`), which shows the rule was understood
there and simply never generalised.

**Required changes.**

1. `ComposerText::replace_range` deactivates the selection before it edits. That is the single
   enforcement point; every length-changing edit inherits it.
2. `delete_selection`'s now-redundant explicit `deactivate()` is removed, so there is exactly one
   place that enforces the rule. (Its range is computed **before** the edit, so removing the
   call changes nothing about what it deletes.)
3. `ComposerText::undo` and `redo` deactivate as well — they do not go through `replace_range`,
   because they must not record a new edit.
4. The rule is stated in the model's module documentation and in `architecture/tui.md`.

**Why "drop it" and not "adjust it".** Adjusting a selection across an arbitrary edit is a
text-edit-mapping problem, and the composer does not need it: the selection's purpose here is
"the text the copy verb takes and the delete verb removes", and after an edit the user has not
asked for that text any more. A stale selection that silently covers the wrong characters is the
failure mode; a selection that is simply gone is a visible, harmless one.

**Acceptance evidence.**

- Arm a selection with `v`, edit the buffer, and assert no selection is held afterwards.
- Undo and redo both drop a held selection.
- The existing M005 selection tests still pass unchanged — the selection *range* rule is
  untouched; only its lifetime changed.
- A test that the composer's delete-selection still removes exactly the range it removed before
  (term for term).

### WP5 — Settle D4: the undo scope

**Intent.** Decide, and record, whether the composer's undo covers the prompt text alone or the
wider composer state.

**Decision: the undo scope is the composer's text buffer, and the history is reset at the two
wholesale-replacement boundaries.**

**Why this is the decision, on the evidence.**

- The composer has **one** text buffer. A slash command, a `!cmd` human-shell line and a staged
  shell projection are all **typed into that same buffer** — there is no second composer state to
  undo. The wider scope the roadmap asks about does not exist as a separate thing.
- The two paths that *replace* the buffer wholesale — submit (`prompt.clear()`) and a staged
  shell projection (`prompt.set_text(include_text)`) — both go through `ComposerText::clear` /
  `set_text`, which **drop the history** (`src/tui/composer_text.rs:311-323`). So the wider state
  is *already* outside the undo scope, by construction, and does not need a rule: a projection is
  the app speaking, not the user editing, and undoing it would delete evidence the user was
  never asked to type.
- A **pending** slash command is ordinary buffer text, so it is undoable while it is pending.
  Once the command is dispatched, `clear()` drops the history and it is not recoverable — which
  is correct: the command has already run.
- The roadmap's exit condition "Undo cannot resurrect text that was submitted; the history is
  cleared on submit" is therefore **already true by construction** and must be *proved* by a
  test, not assumed.

**Required changes.**

- Record the decision in the roadmap's §10 D4 row (status: closed in M006, with the resolution
  and the owner).
- Record it in this milestone's closure record.
- `architecture/tui.md` and `docs/tui.md` state the scope: **the composer's own text; a submit
  or a staged projection resets it.**

**Acceptance evidence.**

- A test that a submitted prompt cannot be resurrected by `u`.
- A test that a staged shell projection is **not** undoable — the projection replaced the buffer
  and reset the history.
- A test that a pending (unsubmitted) slash command **is** undoable while pending.

### WP6 — The help rows for the verbs this milestone adds

**Intent.** The help must not teach a key that does not exist, and it must teach the keys that
now do.

**Required changes.**

- The composer's verbs get help rows: `a` (insert after), `o` (open line below), `u` (undo),
  `U` (redo), alongside the verbs M004 and M005 already landed.
- The rows belong in the app's help entries (`src/tui/input.rs`, `default_help_entries`), in a
  **Normal-mode** section that names the composer, so the row is honest about which surface owns
  the key. The existing `j`/`k` row (`:1615-1623`) already carries a condition for exactly this
  reason ("single-line composer buffer") and is the pattern.
- M007 then **derives** the whole composer section from the claim table so it cannot drift, and
  audits it. M006's obligation is narrower: the rows must be **true**.

**Acceptance evidence.**

- A test that the help contains a row for every composer verb this milestone adds.
- A test that the help contains **no** row advertising bare `a` as a sidebar gesture.

## 8. Failure, cancellation, restart, contention semantics

- **The composer is single-owner and frontend-local.** There is no cross-process contention and
  nothing to lock. The undo history lives in the frontend process and is dropped when the
  process ends.
- **Cancellation.** `Esc` from Insert mode returns to Normal mode. It does not touch the
  history; the text the user typed stays in the buffer and stays undoable.
- **Restart / reconnect.** The undo history is **not** persisted. A frontend that reconnects
  starts with an empty history, which is the same rule as the composer buffer itself: the
  composer is not durable state.
- **Submit.** The submit path clears the composer, and clearing drops the history. An undo
  therefore cannot resurrect a submitted prompt, and the app never sends text the user retracted.
- **A failed send.** `src/tui/commands/subagent_steer.rs:248` clears the composer on a refused
  send, and `src/tui/commands/work_orders.rs:892` clears it on a confirmed work order. Both go
  through `clear()`, so both drop the history. That is the intended behaviour: the draft is gone
  from the buffer, so it is gone from the undo stack too, and a user who wants it back must
  re-type it. **M006 does not change either path.**
- **No daemon interaction.** Nothing in this milestone reaches the daemon, the scheduler, the
  tool broker, or the turn admission path.

## 9. Compatibility and migration

- **The vim map loses bare `a`.** A user who learned `a` → focus sidebar in vim mode will find
  `Ctrl+Alt+S` instead. This is a **deliberate user-visible break**, the same shape as M002's
  re-spellings of stop/resume, open-diff and focus-prompt, and it is **required** by ADR-0042's
  one-owner rule: `a` is the composer's, so it cannot also be the sidebar's.
- **No capability is lost.** `FocusSidebar` was already bound to `Ctrl+Alt+S` in both maps
  before this milestone, so the action keeps a reachable spelling in every configuration. The
  help already documents `Ctrl+Alt+S` → "Focus sidebar" (`src/tui/input.rs:1476-1481`).
- **No stored data, no schema, no protocol change.** There is nothing to migrate.
- **No configuration change.** The claim set is fixed (roadmap §3) and is not user-configurable.
- **The default map is unaffected.** It never bound bare `a`.
- **Other configurations are unaffected.** The claim table is consulted only when the composer
  holds the keyboard and the resolved mode is Normal (`src/tui/app/mod.rs:4025-4041`), so the
  editor route, the terminal panel and the focused panels keep their `a` handling.

## 10. Required tests

**New integration test file `tests/tui_composer_undo.rs`** — dispatch-reachability, pressing real
key events through `App::on_key`, following the machinery `tests/tui_composer_claims.rs:41-64`
already provides (`test_app`, `press`, `normal_mode`, `prompt_text`).

1. `u` undoes the last typed character.
2. `U` redoes it.
3. `U` is reachable with `SHIFT` **and** with `NONE`.
4. `u` undoes a selection delete.
5. A new edit after an undo discards the redo tail.
6. A submitted prompt cannot be resurrected by `u`.
7. `u` with an empty history is a no-op that still consumes the key, and reports.
8. `U` with an empty redo stack is a no-op that still consumes the key, and reports.
9. `a` enters Insert mode with the caret after the character under it.
10. `a` at the end of a line does not move the caret past the line.
11. `o` opens a line below and enters Insert mode there.
12. `o` on a single-line buffer opens a second line.
13. `o` then `u` removes the line again.
14. `i` still does not move the caret.
15. `a` is no longer reachable as `FocusSidebar` in the vim map, and `Ctrl+Alt+S` still is.

**New integration test file `tests/tui_composer_insert_transitions.rs`** — the three insert
transitions, pressed as real key events.

**New integration test file `tests/tui_composer_undo.rs`** — the undo and redo verbs, pressed as
real key events.

**Model unit tests, in `src/tui/composer_text.rs`:**

- `insert_after` at the start, middle, and end of a line.
- `insert_after` clamped to the line end in a multi-line buffer.
- `open_line_below` on a single line, on a middle line, and on the last line.
- `open_line_below` is recorded in the history and is undoable.
- `undo` and `redo` restore the caret to `caret_before` / `caret_after`.
- `undo` and `redo` drop a held selection.
- An edit that changes the length drops a held selection.
- **The eviction regression:** a paste larger than `MAX_UNDO_BYTES` is still undoable, and the
  history's byte total never exceeds `MAX_UNDO_BYTES` plus one edit's weight.
- The depth bound still evicts the **oldest** entries first.

**Guard self-test cases, in `scripts/check_tui_composer_key_authority.py`:**

- A claim on `a` with the vim map still binding bare `a` to `FocusSidebar` is rejected (G2).
- A `FocusSidebar` spelling that is a bare letter is rejected (G5).
- A `FocusSidebar` missing from one map is rejected (G4).

**No new guard is added.** The existing composer guard is extended; the editor guards
(`check_tui_editor_key_authority.py`, `check_tui_editor_prefix_state.py`,
`check_tui_editor_text_authority.py`) are **not** modified (roadmap `:568`).

## 11. Required verification commands

```bash
# Format and compile
cargo fmt --all --check
cargo check --workspace --all-targets --locked

# Lints (both toolchains: the repo's 1.89 and CI's stable)
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo +stable clippy --workspace --all-targets --locked -- -D warnings

# The guard, both modes
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test

# Focused tests while iterating
cargo test --lib tui::
cargo test --test tui_composer_undo --test tui_composer_insert_transitions \
          --test tui_composer_claims --test tui_composer_motions \
          --test tui_composer_selection --test tui_composer_cursor --test tui_render

# Canonical gate
bash scripts/verify.sh quick

# Full gate before closure
bash scripts/verify.sh full
```

**Baseline comparison.** `verify.sh full` is **not** green on every machine: this repository has a
known set of environment-dependent failures (a macOS `/tmp` symlink, tests that need an installed
language server, credentials, or a git identity). The implementing agent must establish the
baseline at the M005 tree and compare the final sweep against it, as M004 and M005 did, rather
than asserting green. The set of failures must not **grow**.

## 12. Documentation updates

- `architecture/tui.md` — the composer's verb table gains `u`, `U`, `a` and `o`; the **selection
  rule** (an edit that changes the length drops the held selection) and the **undo scope** (the
  composer's own text; a submit or a staged projection resets it) are recorded; the `a`
  re-spelling is recorded.
- `docs/tui.md` — the composer Normal-mode key table gains the four verbs; the undo scope and the
  selection lifetime are described in the user-facing voice.
- `.opencode/skills/tui/SKILL.md` — the live-verb list is updated; the two new rules (the
  selection lifetime, the undo scope) are added as bullets, with the one-owner consequence of
  taking `a`.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — the D4 and D6 rows in §10 are updated
  to **closed in M006** with their resolutions; §7's M006 block is marked closed on acceptance;
  §12's table row for M006 is updated. **These edits happen in the closure commit, not in the
  implementation commit.**

## 13. Acceptance criteria

Externally observable, and each one is checkable:

- **AC-M1** — Pressing `u` in Normal mode with the composer holding the keyboard undoes the
  newest composer edit. Verified through the real key path.
- **AC-M2** — Pressing `U` redoes it, and `U` is reachable under **both** the `SHIFT` and the
  `NONE` encoding.
- **AC-M3** — Undo traverses deletions performed by a selection delete, and restores the deleted
  text.
- **AC-M4** — The history is bounded in depth and in bytes, **and the most recent edit is always
  undoable** — including a paste larger than the byte bound.
- **AC-M5** — A new edit after an undo discards the redo tail.
- **AC-M6** — Undo cannot resurrect submitted text; the history is cleared on submit.
- **AC-M7** — Pressing `a` enters insert mode with the caret after the character under it,
  clamped to the caret's line.
- **AC-M8** — Pressing `o` opens a line below the caret's line and enters insert mode there; the
  new line is an ordinary, undoable edit.
- **AC-M9** — Pressing `i` still enters insert mode **without** moving the caret.
- **AC-M10** — After any of the three transitions, the `[INS]`/`[NOR]` badge and the cursor agree,
  and the caret is where the transition put it.
- **AC-M11** — `a` is the composer's: it no longer reaches `FocusSidebar` in the vim map, and
  `FocusSidebar` is still reachable by `Ctrl+Alt+S` in **both** maps.
- **AC-M12** — The help contains a row for every composer verb this milestone adds, and no row
  advertising bare `a` as a sidebar gesture.
- **AC-M13** — An edit that changes the buffer's length drops the held selection; undo and redo
  do the same.
- **AC-M14** — The undo scope is recorded (the composer's own text; a submit or a staged
  projection resets the history) in the roadmap's D4 row, in `architecture/tui.md`, and in
  `docs/tui.md`.
- **AC-M15** — `python3 scripts/check_tui_composer_key_authority.py` and its `--self-test` pass,
  and the self-test rejects each of the three new violation classes.
- **AC-M16** — No new timer, poller, or animation cadence was introduced.

## 14. Stop conditions

Report rather than improvise when:

- **The `a` re-spelling cannot keep `FocusSidebar` reachable in both maps.** If removing the bare
  `a` would leave an action unreachable in some configuration, stop and report: that is a
  decision, not an implementation detail.
- **ADR-0042 part 4 needs to change.** It does not. If the implementer believes the accepted ADR
  is wrong about `a`, that is a **new ADR** (superseding, not editing), and it is out of this
  milestone's scope. Report it.
- **The selection rule cannot be "drop the selection".** If the product wants a selection to
  *follow* an edit, that is a text-edit-mapping feature with its own milestone. Report it rather
  than half-implementing it.
- **The undo scope cannot be settled as "the composer's own text".** Report it — but note that
  the evidence in WP5 already settles it by construction, so a report here means the evidence
  changed.
- **Any work lands in the daemon, the scheduler, the tool broker, or the turn admission path.**
  Stop. This milestone is frontend-local.
- **A second redraw cadence appears to be needed** (a timer for a caret blink, an animation tick
  for the selection). Stop: invariant I5 forbids it, and the terminal's own cursor and the next
  frame are the answer.
- **The eviction fix turns out to need a second bound** (a separate "max single edit" limit
  that makes the newest edit unundoable). Stop: that is the failure the exit condition names.
- **The help surface turns out to need a structural change** (a new help mode, a new dialog) to
  carry the composer's rows. Report it; M007 owns the help's structure.

## 15. Closure evidence required

A closure record at `plans/closure/tui-composer-helix-parity/006-status.md` with:

1. An executive finding.
2. A requirement-to-evidence matrix mapping **AC-M1 … AC-M16** to a named test or a named
   command's output.
3. Production implementation evidence: the claim table's rows before and after, the model's
   changed functions, the bindings removed, the guard's changed rules.
4. Verification executed: the exact commands and their results, with **local** and **CI** results
   labelled truthfully, and the **baseline comparison** for the workspace sweep.
5. An invariant review against §4, item by item.
6. A failure and recovery review: what happens on a refused send, a staged projection, a
   reconnect.
7. A migration and compatibility review: the `a` re-spelling, and the evidence that no capability
   was lost.
8. A security review: no text in any message; the history stays in the frontend; the clipboard
   is untouched by this milestone.
9. Documentation and operations: the three documents updated, and the roadmap and registry rows.
10. Unresolved findings, each with a severity. **A high-severity finding blocks strict closure.**
11. **Roadmap disposition:** D4 and D6 closed with their resolutions; D7 explicitly unchanged.
12. **Registry updates:** M006's row moves to closed with the closure link, and the blocked-work
    audit is recorded — M007's dependency is discharged by this closure, so M007 moves to
    `ready` in the same commit.

## 16. Handoff notes

- **The claim table grows to 14 rows.** That is the plan, not an accident: `a` is one of the
  letters roadmap §3 names, and the table was one short. The guard's G1 rule (closed and
  single-owner) is unaffected; there is still one owner per key.
- **`u` and `U` are lowercase/uppercase of the same letter.** `U` is `CapitalBoth`; `u` is
  `Bare`. A claim on `u` does not claim `U`, and vice versa. Do not "simplify" `U` to `Bare`.
- **The model owns the caret; the app owns the mode.** `insert_after` and `open_line_below`
  return an offset or move the caret; `run_composer_claim` sets `InputMode::Insert` and does
  nothing else. A transition implemented in the app would be untestable without a full app.
- **`move_caret`, not `set_caret`, for `a`.** `a` is a deliberate movement. M004 established that
  the difference is the whole of the selection-growth rule, and M005 depends on it.
- **`o` is an edit; `a` is not.** `o` inserts a newline and is recorded in the history. `a` only
  moves the caret and is not.
- **The eviction fix is the subtle one.** `evict` currently removes the oldest entry, so a
  single oversized paste is evicted immediately and becomes unundoable. The fix is small; the
  reason it matters is that it is invisible until someone pastes a large file.
- **Do not "fix" the `Ctrl+Alt` chord comment** at `src/tui/input.rs:1216-1218`. It reads oddly
  next to the house label `Ctrl+Alt+S`, but it is accurate: the chord family is deliberately
  `Ctrl+Alt+<lower>`, because the terminal delivers the unshifted letter with `CONTROL|ALT`
  (`tests/modifier_chords.rs` asserts the set). Rewording it would touch the chord contract,
  which this milestone does not own.
- **Do not touch the editor's guards.** Roadmap `:568`: "The existing editor guards are not
  modified."
- **Do not edit ADR-0042.** Record the D6 resolution in the roadmap and the closure record.
- **Do not commit the roadmap or registry changes with the implementation.** The status flips
  happen in the closure commit, so the registry never claims a milestone is closed before its
  closure record exists.
