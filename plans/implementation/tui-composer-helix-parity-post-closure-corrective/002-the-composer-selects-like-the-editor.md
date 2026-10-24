# TUI Composer Helix Parity Post-Closure Corrective Milestone 002 — The Composer Selects Like the Editor

Status: ready

Repository baseline: `c2d781319` (`origin/main`)

Source addendum: `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md#c002--the-composer-selects-like-the-editor`

Parent roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — governs the
  composer's claim set. **This milestone does not change the claim set.** `v` keeps its letter and
  its spelling; what it *means* is corrected, and the correction is recorded here and in the
  addendum's decision points, not by editing the ADR's decision text.

Corrects: the closed milestone **M005** (composer selection and clipboard verbs), whose closure record
is `plans/closure/tui-composer-helix-parity/005-status.md`. M005 closed on the **verbs** — `v`, `x`,
`d`, `p` — and on the model's selection state; both are true and both stay true. The property M005
never stated, and never tested, is the **mode-dependent** consequence of a motion, which is what
this milestone corrects.

Primary class: capability (the composer's selection behaves as the editor's), with the invariant
that the composer and the editor share one motion rule.

## 1. Objective

The composer's selection becomes the editor's selection:

- **`e` and `b` select the word they land on.** In Normal mode they create a selection over that
  word; in Select mode they grow the held selection.
- **`v` is a mode.** It switches the composer between Normal and Select, and the badge says which
  one it is in.
- **A movement outside Select mode drops the held selection.** In Select mode the same movement
  grows it.

## 2. Why this milestone is ready

- **No hard dependencies.** M001–M007 and the first corrective are closed. The change touches the
  composer's widget, its model, the badge and the published tables. C001 shares the badge row and
  the render path, so landing C001 first keeps the two diffs separable; it is a convenience, not
  a dependency.
- **The reference is closed and specified.** The editor's rule is implemented, documented and
  tested: `EditorMode::Select` (`src/tui/editor.rs:1660-1667`), `apply_motion_selection`
  (`src/tui/editor.rs:1788-1829`), `selected_word_bounds` (`src/tui/editor.rs:1770-1776`). The
  composer copies a **decided** rule; it does not design one.
- **The model's primitives already exist.** `SelectionState` provides `clear`, `select_char`,
  `select_char_range`, `select_line`, `toggle_char`, `grow_to`, `move_to`, `repin_cursor` and
  `deactivate` (`src/tui/helix_selection.rs:284-540`). The composer already holds a
  `SelectionState<ComposerPoint>`. What is missing is the **mode** that decides what a motion
  means — not any machinery.
- **The claim set is unchanged.** `v` keeps its letter, its spelling and its position in the
  table. No verb is added, removed or re-spelled, so ADR-0042's decision table and the key
  authority guard's inputs are untouched.
- **Independently shippable.** It changes what a motion does inside the composer. It does not
  change which surface owns a key, which is C003's work.

## 3. Current implementation evidence

### 3.1 The editor, which is the reference

| Site | Fact |
|---|---|
| `src/tui/editor.rs:1660-1667` | `EditorMode::{Normal, Insert, Select}` — a surface-local third mode, coexisting only with the app's `InputMode::Normal`. |
| `src/tui/editor.rs:1788-1829` | `apply_motion_selection`: in **Select** mode the selection follows the cursor; with nothing held, a word motion selects the word it landed on and any other motion starts a selection at the cursor and grows it. In **Normal** mode a movement **drops** the selection, and the word motions (`WordEnd`, `WordBackward`) are the ones that **create** one. |
| `src/tui/editor.rs:1770-1776` | `selected_word_bounds`: only `WordEnd` and `WordBackward` select a word; `WordForward` does not. |
| `src/tui/editor.rs:1772` | The word bounds come from `text_geometry::word_run_at(snapshot, next)` — the word **the caret lands on**. |
| `src/tui/commands/editor.rs:4010-4018` | `v` is `HelixVerb::SelectChar` — a **mode**; `x` is an action. |
| `src/tui/commands/editor.rs:2286-2292` | Leaving Select mode (`Esc`) drops the selection and returns the buffer to Normal; the document stays open. |

### 3.2 The composer, which is the subject

| Site | Fact |
|---|---|
| `src/tui/composer_text.rs` (`move_caret`, `set_caret`) | Two caret movements: `move_caret` extends the held selection, `set_caret` re-anchors it. Neither knows a mode. |
| `src/tui/composer_text.rs` (`toggle_select_chars`) | `v` reaches the shared machine's `toggle_char` (`src/tui/helix_selection.rs:425-438`): on an empty set it **arms** a character selection at the caret, and on a second press it **clears** it. It does not change a mode. |
| `src/tui/composer_claims.rs:59-88` | The verb list has `SelectChars` ("Helix `v` — select characters"), `SelectLine`, `DeleteSelection`, `Paste`. There is no select **mode**. |
| `src/tui/input.rs:65-70` | `InputMode` is two-valued: `Insert`, `Normal`. |
| `src/tui/app/render.rs:1910-1939` | The badge renders `[SEL]` from the **editor's** mode and `[INS]`/`[NOR]` from the composer's — one row, two surfaces, and no rule about which surface the badge describes. |
| `src/tui/components/prompt.rs` (`row_spans`) | The model's selection **is** painted, so a selection is visible today. |

**The three differences, measured against the editor.**

| Behaviour | Editor | Composer today |
|---|---|---|
| `e` / `b` | Select the word they land on (Normal mode) or grow the selection (Select mode) | Move the caret; grow a selection only if one is already held |
| `v` | Enters `EditorMode::Select` — a mode, visible in the badge | Arms a one-shot character selection; the badge still says `[NOR]` |
| A movement outside select mode | **Drops** the held selection | Grows it (`move_caret`) or leaves it alone (`set_caret`) |

### 3.3 Why the existing verification passed

M005 closed on the **verbs** (`v`, `x`, `d`, `p`), on the selection state the model holds, and on
the model's `move_caret`/`set_caret` pair — all of which are correct as implemented. The
property that was never stated, and never tested, is the **mode-dependent** consequence of a
motion. Nothing in the model is wrong; the model is missing the mode that decides what a motion
means.

## 4. Invariants that must not regress

- **I1 — the claim set is unchanged.** The same thirteen letters, the same spellings, the same
  encodings, the same conditions. `v` changes what it means, not that it exists.
- **I2 — the model's selection state is unchanged.** `SelectionState<ComposerPoint>` remains the
  composer's selection representation, with the same `SelectionMode`. The milestone adds a
  **mode**, not a second selection model.
- **I3 — one place decides what a motion means.** Every path that **moves** the composer's caret
  — the claim table's verbs, the app's `InputAction::Left`/`Right`, the arrow keys, `Home`/`End` —
  must resolve the mode-dependent rule through the **same** code. A rule implemented in the
  dispatch for one path and not for another is the defect this milestone exists to remove.
  The **mouse** is deliberately outside this invariant: a click *places* the caret rather than
  moving it, so it is not a motion, and this milestone does not change it. The consequence is
  recorded rather than absorbed — a click in Select mode places the caret without growing the
  selection — and the closure record names it as the boundary this milestone did not cross.
- **I4 — `w` does not select a word.** The editor's rule is specific: only `e` and `b` select a
  word. `w` moves to the start of the next word and does not create a selection. This milestone
  does not generalise the rule to every word motion.
- **I5 — `x` keeps its meaning.** `x` selects the whole line and grows the selection a line at a
  time. It stays an action, not a mode.
- **I6 — `d` and `p` are unchanged.** Deleting the selection and pasting over it are unchanged.
- **I7 — the app's mode axis is two-valued.** `InputMode` stays `Insert`/`Normal`. The composer's
  select mode is **surface-local**, as `EditorMode::Select` is.
- **I8 — the badge describes the surface that holds the keyboard.** The composer shows its own
  mode when it holds the keyboard; the editor's mode is shown otherwise. (Decision C-D3.)
- **I9 — Insert mode is untouched.** While the composer is in Insert mode the selection behaves
  as it does today: an edit replaces the selection.
- **I10 — the editor is untouched.** `src/tui/editor.rs`, `src/tui/components/editor.rs` and the
  editor's key authority do not change.

## 5. Scope

**In scope.**

- A surface-local select mode on the composer widget, with the badge reporting it.
- The mode-dependent motion semantics, implemented in one place inside the composer widget.
- The word-bounds helper the composer's word motions need, as the composer's analogue of the
  editor's `selected_word_bounds`.
- `v`'s new meaning, and the published wording of its claim row.
- The badge's rule (C-D3) and the tests that pin it from both sides.
- The documentation corrections in §12.

**Out of scope.**

- The editor. It is the reference; nothing in it changes.
- The app's `InputMode` and every binding map's branching on it.
- The claim set, the key authority guard's rules, and the keymaps.
- The vertical ownership condition (C003).
- The caret's geometry and the block's paint (C001).

## 6. Required production changes

### 6.1 The composer has a surface-local select mode

The composer widget carries the mode, exactly as the editor's state carries `EditorMode`. The
implementer chooses the representation (a boolean or a two-valued enum on the widget); the
requirements are:

- It is readable by the badge's renderer (`App::render_composer` reads
  `self.prompt_state.prompt`).
- It is set by the `v` verb's dispatch and cleared when the composer leaves the keyboard — the
  composer must not come back from a panel holding a stale select mode. The implementer decides
  where that reset lives; the requirement is that a composer that loses and regains the keyboard
  is in Normal mode.
- It does not change the app's `InputMode`, and it does not change which keys are live: the
  composer's Normal-mode verbs are the same verbs in both modes. Only what a **motion** means
  changes.

### 6.2 One rule, implemented once

Every composer caret movement resolves through one rule. In the editor's terms
(`src/tui/editor.rs:1788-1829`):

| Situation | The rule |
|---|---|
| **Select mode**, nothing held | Arm a selection at the caret, then move — the motion grows it. For a word motion, arm the **word** instead. |
| **Select mode**, a selection held | Move — the selection grows. For a word motion, the selection grows to the word's end. |
| **Normal mode**, a word motion (`e`, `b`) | Drop any held selection, then select the **word the caret lands on**. |
| **Normal mode**, any other motion | Drop any held selection, then move. |

The word bounds come from the word the caret lands on, computed by a composer-side helper in the
shape of the editor's `selected_word_bounds` — a composer analogue of `text_geometry::word_run_at`
over the composer's own text.

**The rule lives in the widget's movement methods**, not in the dispatch. The composer's caret is
moved from more than one path, and every one of them is a `PromptWidget` method:

| Path | Reaches the composer as | Widget entry point |
|---|---|---|
| `h` / `l` and the arrow keys | `InputAction::Left` / `Right` → `TuiMsg::CursorLeft` / `CursorRight` | `PromptWidget::cursor_left` / `cursor_right` (`src/tui/components/prompt.rs:318-324`) |
| `w` / `b` / `e` | `ComposerVerb::WordForward` / `WordBack` / `WordEnd` (`src/tui/composer_claims.rs:59-88`) | `cursor_word_forward` / `cursor_word_backward` / `cursor_word_end` (`:335-348`) |
| `j` / `k` | `ComposerVerb::LineDown` / `LineUp` | `cursor_visual_row_down` / `cursor_visual_row_up` (`:364-372`) |
| `Home` / `End` | `InputAction::LineStart` / `LineEnd` | `cursor_home` / `cursor_end` (`:326-333`) |

The mouse **places** the caret by column (`PromptWidget::set_cursor_at_column`,
`src/tui/components/prompt.rs:207-233`) rather than moving it, so it is not a motion and
this milestone leaves it alone; the boundary is named in the closure record. A rule
implemented per path is a rule that will differ on the path nobody tested, so the
mode-dependent behaviour goes into the widget's methods and every path above inherits it.

### 6.3 `v` is a mode

`v` enters Select mode and arms the selection the editor arms (`select_char` at the caret), so
that the next motion grows it. The chain is the shared machine's: `select_char` pushes one member
at the cursor (`src/tui/helix_selection.rs:349-360`), a movement in character mode moves the
member's head with the cursor (`repin_cursor`, `:515-527`), and the composer's `toggle_select_chars`
currently reaches the same `toggle_char` (`:425-438`) — which arms the member on an empty set and
**clears** it on a second press, rather than changing a mode. `v` again, or leaving the composer, returns the composer to Normal mode and drops the
selection — the editor's rule (`src/tui/commands/editor.rs:2286-2292`).

The composer's existing one-shot `toggle_select_chars` is **reused**, not deleted: it is what
arms the character selection the mode then grows. What changes is that the verb now also changes
a mode.

### 6.4 The badge describes the surface that holds the keyboard

The badge row is one row for two surfaces, and today it answers "is the **editor** in Select
mode?" even when the composer holds the keyboard. The rule:

- The composer holds the keyboard → the badge shows the **composer's** mode: `[INS]`, `[NOR]` or
  `[SEL]`.
- Otherwise, the editor holds it → the badge shows the **editor's** mode.

The `[SEL]` styling (`theme.warning` + `BOLD`, `src/tui/app/render.rs:1918-1921`) is reused for
the composer's select mode, so the two surfaces' badges look the same when they mean the same
thing.

## 7. Ordered work packages

- **WP1 — the mode.** Add the composer's surface-local select mode to the widget, with the
  accessor the badge reads and the reset that fires when the composer leaves the keyboard.
- **WP2 — the word bounds.** Add the composer's word-bounds helper, in the shape of the editor's
  `selected_word_bounds`, and a unit test for it that states the word's extent for the words the
  editor's rule cares about (`e` at the end of a word, `b` at the start of one).
- **WP3 — the rule.** Implement §6.2 **once**, inside the composer's movement methods, and route
  every path through it. The word motions (`e`, `b`) and the other motions (`w`, `h`, `l`, `j`,
  `k`, the arrows, the mouse) each get a case.
- **WP4 — `v`.** Make the `v` verb enter and leave the mode, arming the character selection on
  the way in.
- **WP5 — the badge.** Implement §6.4, and render `[SEL]` for the composer's select mode.
- **WP6 — the claim row's wording.** The `v` row's description changes ("Helix `v` — select
  characters" → the mode's wording). The hint row is derived from the table, so it follows.
- **WP7 — the tests.** §10.
- **WP8 — the documentation.** §12.
- **WP9 — the verification.** §11, in order, with the results recorded in the closure record.

## 8. Failure, cancellation, restart, contention semantics

None. The mode is frontend-local state on the widget; there is no task, no timer and no
persistence. A composer that loses the keyboard resets to Normal mode (WP1's requirement), so no
stale select mode survives a focus change.

## 9. Compatibility and migration

- **Storage:** none.
- **Protocol:** none.
- **Migration:** none.
- **User-visible change:** `e` and `b` select the word they land on; `v` enters a mode the badge
  reports; a movement outside that mode drops the selection. These are the corrections the
  milestone exists for, and they align the composer with the editor, which is the documented
  reference.
- **The claim set, the keymaps and the key authority guard are unchanged.** The `v` letter keeps
  its place in the table; only the row's description changes, and the description is what the
  help and the hint publish.

## 10. Required tests

### 10.1 Model and widget unit tests — `src/tui/composer_text.rs`, `src/tui/components/prompt.rs`

| Test | Asserts |
|---|---|
| `a_word_motion_in_normal_mode_selects_the_word_it_lands_on` | After `e`, the selection spans the word the caret landed on. |
| `a_word_motion_in_select_mode_grows_the_held_selection` | After `v`, `e`, the selection spans from the armed character to the word's end. |
| `a_movement_in_normal_mode_drops_the_held_selection` | After a selection is held, a horizontal or vertical movement leaves no selection. |
| `a_movement_in_select_mode_grows_the_held_selection` | After `v`, a movement extends the selection. |
| `the_select_verb_toggles_the_select_mode` | `v` enters the mode and the badge-facing accessor reports it; `v` again leaves it. |
| `the_composer_returns_to_normal_mode_when_it_loses_the_keyboard` | A composer that loses and regains the keyboard is in Normal mode. |
| `a_word_forward_motion_does_not_select_a_word` | `w` moves to the next word's start and leaves no selection (invariant I4). |

### 10.2 Render tests — `tests/tui_render/tui_composer_selection.inc`

| Test | Asserts |
|---|---|
| `the_composer_badge_shows_select_while_it_is_selecting` | The badge reads `[SEL]` while the composer holds the keyboard in its select mode, and `[NOR]` outside it. |
| `the_composer_badge_describes_the_surface_that_holds_the_keyboard` | With the editor in Select mode and the composer holding the keyboard, the badge reads the **composer's** mode. |
| `a_word_motion_selects_the_word_in_normal_mode` | The rendered selection covers the word the caret landed on. |
| `a_movement_outside_select_mode_drops_the_selection` | The rendered selection is gone after the movement. |

### 10.3 The other members of the family

`tests/tui_render/tui_composer_motions.inc` and
`tests/tui_render/tui_composer_keymap_consistency.inc` are **re-run unchanged**; if either
asserts the old `v` wording, it is updated to the new wording and the change is named in the
closure record.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- composer_text
cargo test -p codegg --lib -- tui::components::prompt
cargo test --test tui_render -- tui_composer_selection
cargo test --test tui_render -- tui_composer_motions
cargo test --test tui_render -- tui_composer_keymap_consistency
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

`scripts/verify.sh quick` is the canonical gate. This milestone adds no guard, so
`architecture/overview.md`'s verified-counts table does not move.

## 12. Documentation updates

| Document | Change |
|---|---|
| `docs/tui.md:396-402` | The Normal-mode table's `v` row: from "Start selecting characters; a caret motion then extends the selection" to the mode's wording. The `e` and `b` rows state that they select the word they land on. |
| `architecture/tui.md` | The composer's badge/mode section (the one that states how the badge is resolved, near the composer's mode resolution): the badge describes the surface that holds the keyboard, and the composer's select mode is surface-local. |
| `src/tui/composer_claims.rs` | The `SelectChars` row's description, and the comment that explains what the verb now means. |
| `src/tui/components/prompt.rs` | The widget's mode accessors' docs: what the mode is, who sets it, and when it is cleared. |

The parent roadmap and the closed closure records are **not** rewritten. This milestone's closure
record states the correction and points at this plan.

## 13. Acceptance criteria

1. `e` and `b` select the word they land on, in Normal mode, and grow a held selection in Select
   mode. Tests assert both.
2. `w` does not select a word (invariant I4). A test asserts it.
3. `v` enters the composer's select mode, the badge reads `[SEL]`, and a subsequent movement
   grows the selection. `v` again leaves the mode and drops the selection.
4. A movement in Normal mode drops the held selection. A test asserts it.
5. Every path that moves the composer's caret resolves the same rule — the claim table's verbs,
   `h`/`l`, the arrows and the mouse. A test per path, or a test that names the shared entry
   point and asserts each path reaches it.
6. The badge describes the surface that holds the keyboard, from both sides.
7. The claim set, the keymaps and the key authority guard are unchanged, and
   `python3 scripts/check_tui_composer_key_authority.py` passes with its self-test.
8. `scripts/verify.sh quick` passes, and clippy is clean with `-D warnings`.
9. The closure record states the three behaviours before and after, and names the one place the
   rule lives.

## 14. Stop conditions

- The rule cannot be implemented in one place — that is, two of the composer's caret-moving paths
  cannot reach a shared implementation without a larger refactor. The milestone stops with the
  evidence rather than implementing the rule twice.
- Making the composer's select mode correct requires a third `InputMode` variant. That is
  decision C-D4's answer, and the milestone stops rather than putting a surface's state on the
  app's axis.
- The editor's rule turns out to depend on an editor-only facility with no composer analogue
  that can be written without changing the editor. The milestone stops and the finding is
  escalated; the composer does not grow a second, divergent rule.
- Any existing `tui_render` member fails for a reason other than the `v` wording. That is a
  regression, and the milestone stops.

## 15. Closure evidence required

- The closure record `plans/closure/tui-composer-helix-parity-post-closure-corrective/002-status.md`,
  with: the three behaviours before and after; the place the rule lives and the paths that reach
  it; the badge rule and its two sides; the test names and their assertions; the verification
  commands and their outputs; and the explicit statement that the claim set, the keymaps and the
  editor are unchanged.
- The updated unit and render test names.
- The claim row's new wording, as published.

## 16. Handoff notes

- C003 changes the claim table's **condition** and the app-level page routing. It does not touch
  the composer's verbs or the badge. If the two land together, keep the diffs separable.
- C001 changed the caret's geometry and the block's paint. This milestone changes the badge and
  the mode-dependent motion semantics; the two share the render path but not a rule.
- The claim row's description is published in the hint row and in the help. C003's guard reads
  the published tables, so C003's author must use **this** milestone's wording when they write the
  guard's expectations.
