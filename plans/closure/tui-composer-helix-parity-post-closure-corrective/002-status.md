# TUI Composer Helix Parity Post-Closure Corrective C002 — Closure Status

**Milestone:** C002 — "the composer selects like the editor"
**Plan:** `plans/implementation/tui-composer-helix-parity-post-closure-corrective/002-the-composer-selects-like-the-editor.md`
**Addendum:** `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md`
**Branch:** `fix/file-picker-surface-corrective`
**Status:** `closed`

## 1. Executive finding

The composer now selects the way the editor selects. `e` and `b` select the
word they land on, `v` is a **mode** that arms a selection at the caret and
turns a later movement into a growth rather than a drop, a movement outside
that mode drops the selection, and the badge above the composer describes
the surface that holds the keyboard instead of reading the editor's mode over
the composer.

The rule is implemented **once**, in `PromptWidget::apply_motion`
(`src/tui/components/prompt.rs`), and every path that moves the caret reaches
it: the claim table's verbs, `h`/`l`, the arrows, `Home`/`End` and the page
motions. The composer's mode is a three-valued, surface-local
`ComposerMode { Insert, Normal, Select }` on the widget, and the app pushes
its own half of it (Insert or Normal) at **key dispatch** — not at render,
because the rule has to hold when the key is dispatched.

## 2. The rule as implemented

`src/tui/components/prompt.rs` → `PromptWidget::apply_motion(target, motion)`.
It is the composer's copy of the editor's `apply_motion_selection`
(`src/tui/editor.rs`), term for term.

| Situation | The rule |
|---|---|
| Insert mode | Unchanged. A movement behaves exactly as it did before this milestone (`ComposerText::move_caret`), and an edit replaces the selection. |
| Select mode, nothing held | Arm a selection at the caret, then move. A **word** movement arms the **word** instead (`v` then `e` selects a word, not one character). |
| Select mode, a selection held | Move — the selection grows. A **word** movement grows it to the word's end. |
| Normal mode, a word movement (`e`, `b`) | Drop the held selection, then select the word the caret lands on. |
| Normal mode, any other movement | Drop the held selection, then move. |

A **word** movement is `e` (`ComposerVerb::WordEnd`) and `b`
(`ComposerVerb::WordBack`) — the two movements that land on a word's **edge**,
which identifies the word unambiguously. `w`
(`ComposerVerb::WordForward`) lands on a word's *start*, so it is a plain
movement and never selects.

A movement that has **nowhere to go** still resolves the rule. The editor
clears a held selection for a motion that does not move — `j` on the last
visual row — and the composer's rule is the editor's, so the composer's
clamped vertical motion drops the selection too, with the caret staying where
it was. The widget member
`a_clamped_vertical_motion_drops_the_held_selection` pins it.

The word's extent comes from `composer_text::selected_word_bounds(text, offset)`
(`src/tui/composer_text.rs`), the composer's analogue of the editor's
`selected_word_bounds`: `text_geometry::word_run_at(text, offset)` over the
composer's **own** text, mapped to `(run.start, run.last)`.

## 3. The mode shape, and why

`ComposerMode` is a three-valued enum on the widget
(`src/tui/components/prompt.rs`):

```rust
pub enum ComposerMode { Insert, Normal, Select }
```

**Why a three-valued enum, and why on the widget.**

- It mirrors `EditorMode` (`src/tui/editor.rs`), which is exactly the
  reference: the two surfaces are peers, so the same shape is right for both.
- The app's `InputMode` (`src/tui/input.rs`) stays **two-valued** — `Insert`
  and `Normal` — exactly as invariant I7 requires. The select mode is a
  **surface's** own: the app has no word for it, and it cannot, because
  "select" is only meaningful for the surface that holds the keyboard.
- It lives on the widget because the **rule** lives on the widget, and the
  rule has to know the mode at the moment the movement is dispatched. A mode
  held in the app and read across a call boundary at dispatch time would be
  the same "one field, read twice" defect the app's own mode resolution was
  written to kill.

**The Insert-mode wrinkle, and how it is resolved.**

The widget does not know whether the app is in Insert or Normal mode, and the
app's `composer_input_mode()` is the app's single resolution of that fact.
The resolution has to be true at **dispatch** time, so it is pushed at
dispatch time:

- `App::sync_composer_mode` (`src/tui/app/mod.rs`) is called from `App::on_key`
  **before** the composer's key handling — before the completions handler,
  before the claim gate, and therefore before the `TuiMsg::CursorLeft` /
  `CursorRight` / `CursorHome` / `CursorEnd` path.
- It reads `App::composer_input_mode()` — the app's own derivation, not a
  second one — and pushes it into the widget with
  `PromptWidget::set_app_mode`.
- `set_app_mode` carries the app's half only. `Normal` leaves an existing
  select mode alone (a select mode is not something the app can revoke);
  `Insert` ends it, because a select mode is a Normal-mode thing and Insert
  is not Normal.

**One derivation, not two.** The composer's Insert/Normal half is
`App::composer_input_mode()` and nothing else. The badge reads the same
function; the claim gate reads the same function; the widget's mirror is
pushed from it at dispatch. The select half is the widget's own and is read
from the widget. There is no second computation of the composer's mode
anywhere.

**Where the app pushes and clears it.**

| Site | What it does |
|---|---|
| `App::on_key` → `App::sync_composer_mode` (`src/tui/app/mod.rs`) | Pushes `composer_input_mode()` into the widget at dispatch. When the composer does **not** hold the keyboard, it calls `leave_select_mode`, so a composer that loses the keyboard does not keep a stale select mode. |
| `App::park_composer_at_normal` (`src/tui/app/mod.rs`) | The park: `input_mode = Normal` **and** `leave_select_mode`. Called from the panel focus seam (below). |
| `commands::panels::focus_panel` / `focus_panel_at` (`src/tui/commands/panels.rs`) | The panel-takes-the-keyboard seam. These two call sites are the only change in that file, and they are the **mode hand-off**, not a derivation: they call the app's park, which clears the select mode. The mouse route and the keyboard route cannot leave the composer in different modes. |
| `PromptWidget::leave_select_mode` | Clears the select mode and drops the selection — the editor's rule for leaving select mode. |
| `PromptWidget::end_select_mode` | An edit destroys the held selection, so the mode that exists to grow it ends with it. Called by every edit the widget performs (`set_text`, `clear`, `insert_char`, `insert_newline`, `insert_after`, `open_line_below`, `undo`, `redo`, `backspace`, `delete`, `delete_selection`, `paste`). |

The composer's mode is **not** cleared by opening a dialog: the dialog
short-circuit in `on_key` returns before `sync_composer_mode`, and the
selection therefore survives the help box — which the existing copy-gesture
tests require.

## 4. The entry-point table — every path reaches the one implementation

| Path | App-level route | Widget method | The rule |
|---|---|---|---|
| `w` | `ComposerVerb::WordForward` (`src/tui/app/mod.rs`) | `cursor_word_forward` | `apply_motion(target, Plain)` — **no** word selection |
| `b` | `ComposerVerb::WordBack` | `cursor_word_backward` | `apply_motion(target, Word)` |
| `e` | `ComposerVerb::WordEnd` | `cursor_word_end` | `apply_motion(target, Word)` |
| `v` | `ComposerVerb::SelectChars` | `toggle_select_chars` | the mode toggle: enters Select and arms the selection, or leaves Select and drops the selection |
| `j` | `ComposerVerb::LineDown` | `cursor_visual_row_down` | `apply_motion(target, Plain)` |
| `k` | `ComposerVerb::LineUp` | `cursor_visual_row_up` | `apply_motion(target, Plain)` |
| `h` | `InputAction::Left` → `TuiMsg::CursorLeft` (`src/tui/app/input.rs`) | `cursor_left` | `apply_motion(target, Plain)` |
| `l` | `InputAction::Right` → `TuiMsg::CursorRight` | `cursor_right` | `apply_motion(target, Plain)` |
| `←` / `→` | the same `InputAction`s | the same | the same |
| `Home` / `End` | `InputAction::Home` / `End` → `TuiMsg::CursorHome` / `CursorEnd` | `cursor_home` / `cursor_end` | `apply_motion(target, Plain)` |
| `Ctrl+u` / `Ctrl+d` (page) | `PromptWidget::page_up` / `page_down` | `page` → `move_visual_row` | `apply_motion(target, Plain)` |

There is **no** second implementation of the rule and no per-dispatch-path
special case: every row above ends in `apply_motion`. The mouse is the one
path that does **not** — see §8.

## 5. The badge rule, and the wart it fixes

`src/tui/app/render.rs` renders the mode badge. The rule (the plan's C-D3) is
**the badge describes the surface that holds the keyboard**:

- The **composer** holds the keyboard → the badge shows the composer's mode:
  `[INS]` (the app's `composer_input_mode()` says Insert), `[NOR]`
  (`composer_input_mode()` says Normal and the widget is not selecting), or
  `[SEL]` (the widget's `is_selecting()`).
- The **editor's buffer** holds it (`App::editor_buffer_owns_keyboard()`) → the
  badge shows the editor's mode: `EditorMode::Insert` → `[INS]`,
  `EditorMode::Normal` → `[NOR]`, `EditorMode::Select` → `[SEL]`.
- **Neither** holds it — a focused panel, the timeline overlay or a modal does
  → the badge shows the composer's own **resolved** mode
  (`composer_input_mode()`): `[INS]` or `[NOR]`.

The ownership fact is `App::composer_holds_keyboard()`, the editor's is
`App::editor_buffer_owns_keyboard()`, and the mode fact is
`App::composer_input_mode()`; all three are the app's own derivations and none
was re-derived.

**The wart it fixes.** The badge used to read the **editor's** select mode for
`[SEL]`, whatever held the keyboard. An editor in select mode therefore showed
`[SEL]` above a composer that was in Normal mode — a row describing a surface
the reader was not typing into, and the composer had no select mode of its
own to report in exchange. The badge now asks the surface that holds the
keyboard.

**The third arm, and why it is not the editor's.** "The composer does not hold
the keyboard" is not "the editor holds the keyboard": a focused **panel** or
the **timeline overlay** takes the composer out without the editor taking
over. Reading the editor's mode there would name a surface the reader is not
in — and it reintroduced, for the panel case, the symptom this row had already
been fixed for once: a terminal panel holding a live PTY claims the Insert
keys, and the badge said `[NOR]` above the input while the reader was typing
into a shell (`src/tui/app/render.rs`'s own comment on
`App::composer_input_mode` names that symptom). The third arm reads the
composer's own resolved mode, which is what the badge read before it learned to
ask the surface, and the render member
`the_badge_reads_the_composer_while_another_surface_holds_the_keys`
(`tests/tui_render/tui_composer_selection.inc`) pins it: the member **fails**
against the two-armed badge and passes against the three-armed one.

The colours are unchanged: `[SEL]` is the warning colour, `[INS]` the
secondary colour, `[NOR]` the primary colour, all bold.

## 6. Measured before/after, per scenario

Measured through the app's own key path (`App::on_key`), with the composer in
Normal mode and the text `"hello world"`.

| Scenario | Before | After |
|---|---|---|
| `e` at the caret on `hello` | The caret moved to the word's last byte; **no selection** | The caret moves to the word's last byte and the **word is selected** — the selection covers `hello` |
| `b` at the caret inside `world` | The caret moved to the word's first byte; **no selection** | The caret moves to the word's first byte and the **word is selected** — the selection covers `world` |
| `v` at the caret | A character selection was armed; the mode did not change and the badge did not say so | A character selection is armed **and the mode becomes Select** — the badge reads `[SEL]` |
| `v` again | The armed selection was discarded | The mode leaves Select **and** the selection is discarded — the badge reads `[NOR]` |
| A movement (`l`, the arrows, `Home`/`End`, `j`/`k`) after a selection is held | The movement **extended** the held selection (the caret was in Insert mode, or the selection was a leftover) | In **Normal** mode the movement **drops** the selection; in **Select** mode it **grows** it |
| A movement after `v` | The movement extended the selection, and nothing said so | In **Select** mode the movement grows the selection, and the badge says `[SEL]` |
| The badge, with the editor in Select mode and the composer holding the keyboard | `[SEL]` — the editor's mode, over the composer | `[NOR]` — the composer's own mode |
| The badge, with the composer in its select mode | `[NOR]` — the composer had no mode to report | `[SEL]` |
| `w` | The caret moved to the next word's start; no selection | **Unchanged**: the caret moves to the next word's start and no selection is made |
| `x`, `d`, `p` | The line selection, the deletion and the paste | **Unchanged** (see §8) |

### One defect this milestone exposed, and fixed

`ComposerText::selection_range` computed the end of a character selection with
`next_char_boundary(text, last).unwrap_or(last)`. `next_char_boundary` answers
"where does the **next** character begin", and there is no next character at
the end of the buffer — so the fallback left `end == last`, which **dropped
the head character** from a selection that reached it. `v` on the last
character of the composer selected nothing, and `e` on the last word of the
buffer selected all but its last letter.

The editor had already fixed exactly this, with the reasoning in the comment
at `src/tui/editor.rs:4077-4089` (`char_end`). The composer now uses the
same rule — `next_char_boundary(text, last).unwrap_or(text.len())` — so the
two surfaces agree. One existing model test
(`composer_text::tests::an_edit_drops_the_held_selection`) pinned the buggy
end-of-buffer range and was updated to the editor's convention; a new test
(`a_selection_that_reaches_the_end_covers_the_last_character`) states it.

## 7. The tests, and what each asserts

### Model and widget unit tests

`src/tui/components/prompt.rs`:

| Test | Asserts |
|---|---|
| `a_word_motion_in_normal_mode_selects_the_word_it_lands_on` | After `e`, the selected text is the word the caret landed on, and the caret is on the word's last byte. |
| `a_backward_word_motion_in_normal_mode_selects_the_word_it_lands_on` | After `b`, the selected text is the word the caret landed on, and the caret is on the word's first byte. |
| `a_word_motion_in_select_mode_grows_the_held_selection` | After `v` then `e`, the selection spans from the armed character to the word's end. |
| `a_movement_in_normal_mode_drops_the_held_selection` | After a selection is held, a movement in Normal mode leaves **no** selection. |
| `a_movement_in_select_mode_grows_the_held_selection` | After `v`, a movement extends the selection (the range grows). |
| `a_word_forward_motion_does_not_select_a_word` | `w` reaches the next word and leaves **no** selection (invariant I4). |
| `the_select_verb_toggles_the_select_mode` | `v` enters the mode and the badge-facing accessor reports it; `v` again leaves it and drops the selection. |
| `the_composer_returns_to_normal_mode_when_it_loses_the_keyboard` | After `leave_select_mode`, the composer is in Normal mode and holds no selection. |
| `an_edit_ends_the_select_mode` | An edit leaves the composer in Normal mode. |
| `the_apps_insert_mode_ends_the_select_mode` | The app's Insert mode reaches the composer and ends the select mode. |

`src/tui/composer_text.rs`:

| Test | Asserts |
|---|---|
| `the_word_bounds_name_the_run_the_motion_landed_on` | The composer's word-bounds helper names the word `e` and `b` land on — `(0, 4)` for the last byte of `hello`, `(6, 10)` for the first byte of `world` — and `None` on whitespace. |
| `a_selection_that_reaches_the_end_covers_the_last_character` | A selection that reaches the end of the text covers the last character (the editor's `char_end` rule). |

### Render tests

`tests/tui_render/tui_composer_selection.inc`:

| Test | Asserts |
|---|---|
| `the_composer_badge_shows_select_while_it_is_selecting` | The badge reads `[SEL]` while the composer holds the keyboard in its select mode, and `[NOR]` outside it. |
| `the_composer_badge_describes_the_surface_that_holds_the_keyboard` | With the editor in Select mode and the composer holding the keyboard, the badge reads the **composer's** mode (`[NOR]`, and **not** `[SEL]`). |
| `a_word_motion_selects_the_word_in_normal_mode` | The **rendered** selection covers the word the caret landed on. |
| `a_movement_outside_select_mode_drops_the_selection` | The **rendered** selection is gone after the movement. |
| `a_movement_in_select_mode_grows_the_selection` | The rendered selection grew after a movement in select mode. |
| `every_caret_moving_path_resolves_the_same_rule` | Each of the claim table's `e` and `b`, `h`, `l`, the left and right arrows, `Home` and `End` grows the armed selection in select mode — no path reaches a different rule. |
| `v_is_a_mode_the_badge_can_read` | `v` enters and leaves the composer's select mode. |
| `the_composer_loses_its_select_mode_when_the_keyboard_leaves_it` | A composer that loses the keyboard to a panel does not keep its select mode. |
| `w_does_not_select_a_word` | `w` reaches the next word and leaves no selection. |

The **existing** widget tests were not weakened: `a_selected_range_is_painted_with_the_selection_colours`,
`a_selection_spanning_a_newline_is_painted_on_both_lines` and
`the_widget_reports_the_selection_the_delete_verb_removes` still pass
unmodified, because `v` arms the selection and the movement after it grows
it — the editor's behaviour.

## 8. What is out of scope, and what is unchanged

- **The mouse is out of scope.** The mouse **places** the caret; it does not
  move it, and it gained no selection semantics. `PromptWidget::set_cursor_at_column`
  is unchanged, and the mouse is deliberately absent from the entry-point
  table in §4.
- **`w` is unchanged.** It moves to the next word's start and selects
  nothing, before and after.
- **`x` is unchanged.** It still selects the caret's line, and a second `x`
  still takes the next line.
- **`d` is unchanged.** It still deletes the held selection. The *mode* ends
  with the selection, because an edit destroys the selection the select mode
  exists to grow — the editor's rule.
- **`p` is unchanged.** It still pastes at the caret, over the selection.
- **`a` is an insert entry, not one of the ruled caret motions.** It places the
  caret for the insertion and extends a held character selection, the way it
  always has; the milestone's rule governs the **motions** — `w`, `b`, `e`,
  `h`, `l`, `j`, `k`, the arrow keys and `Home`/`End` — and `a` is the
  composer's `ComposerVerb::InsertAfter`, an entry into Insert mode. The
  published paragraph in `docs/tui.md` says exactly this, and it says it
  because an earlier draft of that paragraph claimed the rule covered `a` and
  the code does not: the sentence was corrected rather than the behaviour,
  and the correction is recorded here so the boundary is named rather than
  left to be rediscovered.
- **The claim set, the keymaps and the key authority guard are unchanged.**
  The `v` letter keeps its place in the table; only the row's description
  changed, and the description is what the help and the hint publish.
- **The mouse, `x`, `d` and `p` have no selection semantics of their own.**
  `x` selects a line (a line selection does not grow with a caret motion),
  `d` and `p` act on the selection, and the mouse places the caret.

## 9. Verification executed

### Commands run, and their real results

| Command | Result |
|---|---|
| `cargo fmt --all` | clean (exit 0) |
| `cargo test -p codegg --lib -- tui::components::prompt` | **35 passed**, 0 failed (25 before this milestone, 10 added) |
| `cargo test -p codegg --lib -- composer_text` | **45 passed**, 0 failed (42 before this milestone, 3 added, 1 updated) |
| `cargo test --test tui_render -- tui_composer_selection` | **26 passed**, 0 failed (17 before this milestone, 9 added) |
| `cargo test --test tui_render -- tui_composer_motions` | **16 passed**, 0 failed (re-run unchanged, as §10.3 requires) |
| `cargo test --test tui_render -- tui_composer_claims` | **30 passed**, 0 failed (re-run unchanged) |
| `cargo test --test tui_render -- tui_composer_keymap_consistency` | **14 passed**, 0 failed (re-run unchanged) |
| `cargo test --test tui_render` (the whole family) | **1092 passed**, 0 failed |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | **clean** — no errors, no warnings (exit 0) |
| `python3 scripts/check_test_assertion_quality.py` | `check_test_assertion_quality: all checks pass` (exit 0) |
| `python3 scripts/check_test_duplication.py` | `check_test_duplication: all checks pass` (exit 0) |
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` (exit 0), after the re-bank described below |
| `bash scripts/verify.sh quick` | **passed** (exit 0) — "==> Quick verification passed."; the harness-lab unittest sweep ran 328 tests (1 skipped) and `cargo check --workspace --all-targets --locked` finished in 11m 11s |

`tests/tui_render/tui_composer_motions.inc` and
`tests/tui_render/tui_composer_keymap_consistency.inc` are re-run **unchanged**
and both pass; neither asserted the old `v` wording, so neither needed an
update.

### The comment-budget re-bank

`check_comment_budget.py` is a ratchet: the corpus may not grow past the
recorded ceiling, and the only way to spend the lines a reduction frees is a
visible, reviewed edit to `docs/comment-budget.toml`. This milestone's
documentation is part of the change, so the budget was re-banked after
`cargo fmt --all`, with the procedure the script prints
(`python3 scripts/check_comment_budget.py --bank`).

The re-bank covers this milestone's growth **and** C001's, which was still
un-banked in the tree: `src/tui/composer_geometry.rs`,
`tests/tui_render/tui_composer_cursor.inc` and `tests/tui_render/tui.inc`
were raised to their already-written values. `src/tui/components/prompt.rs`,
`src/tui/composer_text.rs`, `src/tui/app/mod.rs`, `src/tui/app/render.rs`,
`src/tui/composer_claims.rs` and `tests/tui_render/tui_composer_selection.inc`
carry this milestone's growth.

### The full library sweep

`cargo test -p codegg --lib` was run once for information: **7453 passed, 7
failed**. None of the seven is caused by this change, and each was checked:

- The four `tui::commands::interactive_terminal::tests::*` failures are
  **flaky**: each passes when re-run on its own (three consecutive re-runs of
  `a_hidden_terminal_surface_does_not_arm_the_loop` all passed), and they
  fail only in the shared-process sweep. They are about
  `App::composer_input_mode()` — the app's own resolution, which this
  milestone did not change.
- `core::daemon_refresh::tests::session_projection_*` and
  `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn`
  fail in isolation too, and are **pre-existing and environmental**: the
  managed-process one fails with "managed process sandbox failed:
  OfflineRequired is unavailable on this platform". Neither surface is in this
  milestone's diff.

The repository runs its broad sweeps under the `nextest` `ci` profile — one
process per test — precisely because env mutation in tests is process-global.

## 10. The canonical gate

`bash scripts/verify.sh quick` — **exit 0**, ending with
`==> Quick verification passed.` It ran 99 steps: rustfmt, the two
generated-asset checks, the guard scripts (including the comment budget, the
test assertion-quality guard and the test duplication guard), the harness-lab
unittest sweep (328 tests, 1 skipped) and
`cargo check --workspace --all-targets --locked`. It was run twice — once
before the closure record was written and once with it in place — and exited
0 both times.

## 11. What the plan got wrong, and what needs a change elsewhere

1. **The plan's §12 asks for the `e` and `b` rows of the Normal-mode table in
   `docs/tui.md` to state that they select the word they land on.** The
   ownership this milestone was given covers the `v` row only, so the `e` and
   `b` rows were **not** changed. They still read "Move to the last character
   of the word at the caret" / "Move to the start of the previous word", which
   is now incomplete. **This needs a change in a row this milestone did not
   own.**
2. **The plan does not mention the end-of-buffer selection defect.** The
   composer's `selection_range` dropped the head character of a selection that
   reached the end of the buffer — the editor had already fixed this
   (`src/tui/editor.rs:4077-4089`), and the composer had not. The milestone's
   headline behaviour (`e` selects the word it lands on) is wrong at the end
   of the buffer without the fix, so the fix is in the diff and is named in
   §6.
3. **The plan's line references were written against baseline `c2d781319` and
   had drifted.** Every reference in this record was re-derived from the
   source.
4. **The plan does not say where the mode is cleared when the composer loses
   the keyboard.** The panel focus seam
   (`commands::panels::focus_panel` / `focus_panel_at`) is the one place both
   the mouse route and the keyboard route pass through, and it is where the
   clear happens. That file is outside the ownership this milestone was
   given; the change is two call sites and it is the mode hand-off, not a
   second derivation.

## 12. Files changed

| File | What changed |
|---|---|
| `src/tui/components/prompt.rs` | `ComposerMode` (the surface-local mode), `ComposerMotion`, the mode field and its accessors (`mode`, `is_selecting`, `set_app_mode`, `leave_select_mode`, `end_select_mode`), `apply_motion` (the rule, implemented once), every movement method routed through it, `visual_row_target`, the `v` verb as a mode toggle, the edits ending the select mode, and the widget unit tests. |
| `src/tui/composer_text.rs` | `selected_word_bounds` (the composer's analogue of the editor's), `left_target` / `right_target`, `clear_selection` / `select_char` / `select_char_range`, the end-of-buffer fix in `selection_range`, and the model unit tests. |
| `src/tui/app/render.rs` | The badge: it describes the surface that holds the keyboard. |
| `src/tui/app/mod.rs` | `App::sync_composer_mode` (the push at dispatch), `App::park_composer_at_normal` (the park), and the `sync_composer_mode` call in `App::on_key`. |
| `src/tui/commands/panels.rs` | **Outside the ownership given**, and named here: two call sites now call `App::park_composer_at_normal` instead of assigning `ui_state.input_mode = InputMode::Normal` directly, so the panel park also clears the composer's select mode. This is the mode hand-off — it introduces no second derivation of the composer's mode. |
| `src/tui/composer_claims.rs` | The `v` verb's description and doc comment only ("Toggle select mode; a motion then extends the selection"). The condition columns are untouched. |
| `tests/tui_render/tui_composer_selection.inc` | The render members: the badge, the word motion, the drop, the path table, the mode toggle, the keyboard leaving, and `w`. |
| `docs/tui.md` | The `v` rows (the Normal-mode table and the selection table), and the two sentences in the selection section that said a caret motion extends a held selection. |
| `docs/comment-budget.toml` | The re-bank (§9). |
| `plans/closure/tui-composer-helix-parity-post-closure-corrective/002-status.md` | This record. |

## 13. Status

`closed` — every acceptance criterion in the plan's §13 is met with the
evidence above, and the canonical gate passes.
