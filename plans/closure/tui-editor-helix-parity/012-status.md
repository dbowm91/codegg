# TUI Editor Helix Parity Milestone 012 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/012-undo-history-traversal.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M012), §13 (stage B)
Repository baseline reviewed: `519577633` (M011, wave 2 stage A)

---

## 1. Executive finding

M012 is **closed**. The editor's undo history is **one ordered list with a
cursor in it** rather than two stacks; `Alt+u` and `Alt+U` walk that cursor
and **report** their boundary; the cursor's position is **observable** on the
editor's banner row; the memory bound is **one cap** over the history rather
than one per direction; and the key-authority guard's conflict model is now
the **exact modifier bitmask**, so the two terminal spellings of a capital
can be two arms without the guard calling them a duplicate.

Five things in this milestone are worth recording beyond the checklist, because
each is a decision a later milestone would otherwise have to re-litigate.

1. **The plan's chord count was wrong, and the guard's was right about what it
   counts.** The plan said 68 Normal-mode chords. The guard reports **69**.
   The plan counted the two *gestures*; the guard counts *chords*, and the two
   terminal spellings of `Alt-U` differ in the **key code**, not only in the
   modifier — so they need two arms, and `Alt-u` is the third. See §5.
2. **The plan's §6.9 claim that "the key-authority guard needs no change" was
   wrong, and its own §6.4 snippet is what made it wrong.** The old conflict
   rule keyed a claim by its *modifier names*, so an arm accepting exactly
   `ALT` and one accepting exactly `ALT | SHIFT` looked like a duplicate. A
   guard that cries wolf is a guard that gets switched off. See §4.
3. **A redo now restores the same recorded position an undo restores.** The
   two-stack model gave the redo stack its own copy of the entry's cursor, so
   a redo landed where the reader was when they pressed `u`. One history and
   one entry carry one position, so a redo restores `entry.cursor_before` —
   the same value the undo restores. The change is visible, and it is *more*
   symmetric. See §3.2.
4. **The bound is now the bound the documentation always claimed.** The
   two-stack model enforced the 4 MiB cap **per stack**, so the worst case was
   twice what `architecture/tui.md` promised. One history enforces it once, so
   the code and the documentation now agree. See §3.3.
5. **`EditorCommand::Undo` and `EditorCommand::Redo` were deleted, not
   wired.** They were never constructed, and an undo cannot be built from a
   snapshot the way `resolve_edit` builds an edit — the inverse of an edit is
   a property of the *entry*, and the entry is not an `EditorEdit`. A variant
   with no producer is a claim, not a hook. See §3.4.

---

## 2. The work packages that landed

| W | Package | Landed as |
|---|---|---|
| W1 | The model: one `history`, one `history_cursor`, one `history_bytes` | `src/tui/document_session.rs`, `src/tui/editor.rs` — `push_undo`, `truncate_history_tail`, `undo_entry`, `redo_entry`, `move_history_cursor_back`, `move_history_cursor_forward`, `trim_history`, `clear_history` |
| W2 | The apply side: one traversal, four gestures | `src/tui/commands/editor.rs` — `traverse_history`, `editor_undo`, `editor_redo`, `editor_history_earlier`, `editor_history_later` |
| W3 | The `Alt-u` / `Alt-U` arms | `src/tui/commands/editor.rs` — `handle_normal_key`, three arms over an exact `alt` / `alt_shift` bitmask |
| W4 | The boundary report | `EditorNotice::Note` — "no earlier change" / "no later change", in the editor's notice surface |
| W5 | The observable cursor | `src/tui/components/editor.rs` — `history <at>/<of>` on the banner row, and only while the cursor is not at the tip |
| W6 | The dead variants, and the docs | `EditorCommand::Undo`/`Redo` and `EditorUndoEntry::rebased` deleted; `docs/tui.md`, `architecture/tui.md`, `CHANGELOG.md`, `.opencode/skills/tui/SKILL.md` |

---

## 3. The model

### 3.1 One list, one cursor

`TuiDocumentPresentation` no longer holds `undo`, `redo`, `undo_bytes` and
`redo_bytes`. It holds:

- `history: Vec<EditorUndoEntry>` — every entry, in the order it was made;
- `history_bytes: usize` — the retained payload's size;
- `history_cursor: usize` — the number of **applied** entries.

`history_cursor == 0` is the beginning of the history (everything is
redoable); `history_cursor == history.len()` is the tip (nothing is
redoable). `undo_entry` is the entry at `cursor - 1`; `redo_entry` is the
entry at `cursor`. **An entry never moves.** That is what makes the "both
directions" invariant structural rather than a discipline: there is no
transfer in which an entry can lose the direction it is not currently using.

The four stack operations — `take_undo`, `take_redo`, `restore_undo`,
`record_redo` — are gone. `push_undo` truncates the tail
(`history[cursor..]`) before pushing, which is what a new edit after a
traversal has to do, and it sets the cursor to the new tip.

### 3.2 A redo restores the same position an undo restores

The two-stack model carried **two** values per entry, because
`EditorUndoEntry::rebased` re-recorded the cursor onto the redo stack as the
position the reader was at when they pressed `u`. A redo therefore landed
there — *not* at the pre-edit position.

One history and one entry carry one recorded position. The redo now restores
`entry.cursor_before`, the same value the undo restores, and `rebased` is
deleted because its only caller was the transfer between the two stacks.

**This is a visible change.** After `u` and then `U`, the cursor is at the
entry's recorded position rather than where the reader was when they undid.
It is also more *symmetric*: both directions restore the state the entry
names, which is what a time cursor means. The plan's §6.1 ("the entry is
unchanged") and §3.1 are recorded in §5.3 and §5.4 of the plan.

### 3.3 One cap, not one per direction

`MAX_EDITOR_UNDO_DEPTH` (256) and `MAX_EDITOR_UNDO_BYTES` (4 MiB) are now
enforced **once**, over the history as a whole. The two-stack model enforced
the byte cap per stack, so its worst case was twice the "4 MiB across both
directions" that `architecture/tui.md` documented. The code and the
documentation now agree, and the worst case is **halved**.

| | Before | Now |
|---|---|---|
| Entries | 256 per stack → 512 | **256 total** |
| Payload bytes | 4 MiB per stack → 8 MiB | **4 MiB total** |
| Documented bound | `architecture/tui.md` — "256 entries and 4 MiB across both directions" | the same sentence, now **true** |

The eviction is bounded and accounted for: `trim_history` drops the **oldest**
entry until the history is inside the cap, and shifts the cursor with it, so
the entries the reader still needs are the ones that stay. The plan's stop
condition — that the single cap must not evict entries the reader still needs
*unboundedly* — is not reached: the tightening is the planned one.

### 3.4 The dead variants

`EditorCommand::Undo` and `EditorCommand::Redo` had no producer. They could
not be wired: `resolve_edit` builds an `EditorEdit` from a snapshot, and an
undo is not derivable from the current text — it is a property of the
*entry*, and the entry is not an `EditorEdit`, so `PrefixAction::Edit` could
not carry it either. Both are deleted, and `EditorUndoEntry::rebased` with
them.

### 3.5 The M011 sequencing note

**M011's coalescing had landed before this milestone did.** M011 was closed
on 2026-10-19 in commit `519577633`, and this milestone was implemented
against that baseline, so the insert session was already in the tree and this
milestone's traversal **traversed coalesced entries**, not one entry per
keystroke.

What that means concretely: a typing burst is one `EditorUndoEntry`, and
`Alt-u` moves past the whole burst in one step — the same granularity `u`
already had after M011. The traversal does not care about granularity: it
walks entries, whatever an entry is. M012 was required to tolerate any entry
granularity, and it does, because nothing in the history model knows what an
entry's edits are.

---

## 4. The guard: the conflict model is the exact bitmask

`scripts/check_tui_editor_key_authority.py` compares a claim's **exact
modifier bitmask**, not its modifier *names*.

**The reason.** An arm that accepts exactly `ALT` and one that accepts
exactly `ALT | SHIFT` are disjoint keys, and Rust's `==` on the bitflags says
so. The old rule keyed a claim by the *set of names* it carried, so both
arms carried the name `ALT` and the guard reported a duplicate. That is a
false positive on a correct source, and a guard that reports false positives
is a guard that gets switched off.

**What changed.**

- A predicate `m == alt || m == alt_shift` resolves to the set of **two**
  bitmasks — exactly `ALT` and exactly `ALT | SHIFT` — and not to their
  combination.
- Rule 2 is a pairwise **intersection** on those bitmasks.
- Rules 1 and 3 still compare modifier *names*, because what they ask is
  whether a chord is *shadowed*, not whether it is identical.
- The guard reads a **multi-modifier binding** —
  `let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;` — which is how
  the `alt | shift` of the plan's §6.4 is spelled in Rust. (The plan's own
  snippet used the or-pattern `m == alt | m == shift`, which is not a bitmask
  at all; its prose was right and its snippet was wrong.)

**The tenth self-test case** pins the model: `ALT` and `ALT | SHIFT` are
disjoint bitmasks, so two arms that claim them are not a duplicate. The
self-test is **10 cases**, all passing.

**The reported counts are `2 guard-block chord(s), 69 Normal-mode chord(s),
8 Insert-mode chord(s)`.** The Normal count moved from 66 to 69; the Insert
count is M011's 8 and is unaffected.

---

## 5. The guard-injection proof

Three injections, each restored **byte-identical** (`diff -q` reported no
difference), each proving the guard is not vacuous under the new model. The
command in every case is:

```bash
python3 scripts/check_tui_editor_key_authority.py
```

| # | Injection | Expected | Observed |
|---|---|---|---|
| 1 | A duplicated `Alt-u` arm — the same exact bitmask twice | rule 2 | `editor.rs:3828 … re-declares ALT+'u', already claimed at editor.rs:3809 … — unreachable`, **1 violation**, exit 1 |
| 2 | A `Ctrl+s` arm in the Normal table — a guard-block chord claimed twice | rule 1 | `editor.rs:3809 … claims CONTROL+'s', but the guard block at editor.rs:3309 already claims CONTROL+'s' in Normal — unreachable`, **1 violation**, exit 1 |
| 3 | `Char('u')` under `m == alt \|\| m == alt_shift` beside the existing `m == alt` arm — a **multi-bitmask** overlap | rule 2 | `editor.rs:3832 … re-declares ALT\|SHIFT+'u', already claimed at editor.rs:3809 … — unreachable`, **1 violation**, exit 1 |

Injection 3 is the one that covers the new model specifically: the old
name-keyed rule would also have caught it, but it is the case where the
overlap is real *and* one of the claims admits a set of bitmasks, which is
exactly the shape the new rule has to get right.

The **negative** case is the real tree: the `Char('U')` arm admits
`{ALT, ALT|SHIFT}` and the `Char('u')` arm admits `{ALT|SHIFT}`, and the
guard passes — two claims that share a modifier *name* and share no
*bitmask*, which the old rule would have called a duplicate.

The guard's own `--self-test` grew a tenth case for the same property:
*"ALT and ALT|SHIFT are disjoint bitmasks, so the two arms are not a
duplicate"*. All **10** cases pass.

---

## 6. The verification and test evidence

### 6.1 The sweep

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test -p codegg --lib tui:: --locked` | **2249 passed, 0 failed** (M011's baseline was 2241) |
| `cargo test -p codegg --lib --no-fail-fast --locked` | **not green** — see §6.5 |
| `cargo build --locked --bin codegg-sandbox-helper` | exit 0 |
| `bash scripts/verify.sh quick` | "Quick verification passed.", exit 0 |
| `python3 scripts/check_tui_editor_key_authority.py` | passed — **2 / 69 / 8** |
| `python3 scripts/check_tui_editor_key_authority.py --self-test` | **10 cases**, all passing |
| `python3 scripts/check_tui_editor_prefix_state.py` | passed — one pending-prefix field |
| `python3 scripts/check_tui_editor_text_authority.py` | passed — no retained text buffer |
| the plan/registry/roadmap consistency sweep | **108 checks, all consistent** |

### 6.2 The tests

`cargo test -p codegg --lib tui:: --locked` → **2249 passed, 0 failed**
(M011's baseline was 2241).

**Eight new tests.**

| Test | What it pins |
|---|---|
| `alt_u_moves_the_history_cursor_back_and_the_document_follows` | `Alt-u` and `Alt-U` through the **real dispatch path**; the document follows the cursor |
| `alt_u_accepts_both_terminal_spellings_of_the_capital` | `(ALT, Char('U'))` **and** `(ALT\|SHIFT, Char('u'))` both move the cursor forward — the M013 failure mode |
| `alt_u_at_the_oldest_entry_reports_and_does_not_move` | "no earlier change", and the cursor did not move |
| `alt_u_at_the_tip_reports_and_does_not_move` | "no later change", and the cursor did not move |
| `u_and_u_stay_silent_at_their_boundaries` | `u` and `U` stay silent at their boundaries, as they always have |
| `alt_u_and_alt_u_are_inert_in_insert_mode` | all four `Alt` spellings reach the composer in `INS` and type nothing |
| `a_refused_undo_leaves_the_history_cursor_where_it_was` | a refused apply leaves the cursor, the history and the entry exactly where they were |
| `the_history_position_is_disclosed_only_when_the_cursor_is_not_at_the_tip` | the banner row says `history 1/3` off the tip and says **nothing** at the tip |

**Two count tests updated**, both because a stale count is a test that fails
for the wrong reason:

- `the_eight_alt_chords_are_inert_in_insert_mode` → renamed
  `the_eleven_alt_chords_are_inert_in_insert_mode` and extended with `u` and
  `U`. The old name said *eight* while the list held *nine*; M011 recorded
  that as pre-existing and out of scope, and M012 **discharges** it — the
  list now holds eleven and the name says eleven.
- `all_eighteen_chords_are_claimed_in_normal_mode` — `claimed.len()` 22 → **24**.

**Two model tests replaced**, because they exercised a model that no longer
exists:

- `rejected_undo_entry_returns_to_the_history_unchanged` → renamed
  `the_entry_stays_in_the_history_when_the_cursor_moves_back`. The old test
  asserted the two-stack model's *"take the entry, put it back"*; the
  guarantee it protected is now an **apply-side** fact, and it is pinned by
  `a_refused_undo_leaves_the_history_cursor_where_it_was`.
- `undo_history_pushes_caps_and_records_bytes` and
  `clear_history_drops_undo_redo_and_the_pending_prefix` were rewritten
  against the cursor model, and `the_first_edit_of_a_run_clears_the_redo_tail`
  was **strengthened** — it now plants an entry and undoes it first, so the
  tail it claims to clear actually exists. Before, the assertion held for a
  history that never had anything in it.

---

### 6.3 The existing-test disposition

**Passed unmodified** — the model's behaviour, not its shape, is what these
pin, and it did not change:

`editor_keeps_edit_through_undo_redo_save_and_reopen` ·
`a_multi_selection_edit_is_undone_and_redone_as_one_step` ·
`an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` ·
`a_local_edit_undo_and_redo_clear_stale_semantic_state` ·
`a_divergent_resync_keeps_the_draft_and_clears_undo` ·
`a_local_edit_during_a_destructive_phase_is_refused_and_history_survives` ·
`undo_and_redo_are_reachable_through_dispatch` ·
`undo_history_does_not_survive_reload_resync_or_close` ·
`switching_documents_clears_history_and_drops_the_previous_attachment` ·
`typing_undoing_and_typing_again_leaves_the_redo_stack_empty` ·
`a_typing_burst_is_one_undo_step` ·
`a_second_undo_reverts_the_edit_before_the_burst` ·
`editor_motion_alone_never_changes_text_or_history` ·
`ampersand_aligns_in_display_columns_and_is_undoable_as_one_step` ·
`a_read_only_attachment_renders_but_refuses_every_edit` ·
`reconciliation_operations_clear_undo_history_but_save_does_not` ·
`reset_for_open_clears_history_and_viewport`

**Updated**, and why:

| Test | Change | Reason |
|---|---|---|
| `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` | the assertion reads the new `redo_depth` helper | the accessor changed; the assertion is the same one |
| `escape_abandons_a_pending_regex_without_touching_the_set` | the assertion reads the new `undo_depth` helper | as above |
| `the_eight_alt_chords_are_inert_in_insert_mode` | **renamed** to `the_eleven_alt_chords_are_inert_in_insert_mode`, and extended with `u` and `U` | the list grew from nine to eleven, and the name said *eight* while the list held *nine*. M011 recorded that as pre-existing; M012 **discharges** it. |
| `all_eighteen_chords_are_claimed_in_normal_mode` | `claimed.len()` 22 → **24** | M012's two history gestures are claimed chords |
| `undo_history_pushes_caps_and_records_bytes` | rewritten against the cursor model | it asserted the two-stack shape (`undo.len()`, `redo.len()`, `undo_bytes`) |
| `undo_history_drops_the_oldest_entry_past_the_depth_cap` | `presentation.undo.len()` → `presentation.history.len()` | the field is now `history` |
| `undo_history_drops_the_oldest_entry_past_the_byte_cap` | `undo_bytes` → `history_bytes` | as above |
| `clear_history_drops_undo_redo_and_the_pending_prefix` | asserts `history` / `history_bytes` / `history_cursor` | as above |
| `the_first_edit_of_a_run_clears_the_redo_tail` | **strengthened**: it now plants an entry and undoes it first | the old assertion held for a history that never had anything in it — it could not fail |
| `rejected_undo_entry_returns_to_the_history_unchanged` | **replaced** by `the_entry_stays_in_the_history_when_the_cursor_moves_back` | it exercised the two-stack model's *"take the entry, put it back"*, a transfer that no longer exists. The guarantee it protected is now structural, and the refusal case is pinned on the apply side. |
| `tests/tui_completion_trajectory.rs` — `ready_answer_applies_one_undo_unit_with_redo` and the refused-accept assertions | `.undo` / `.redo` → `.history` and the history cursor | the integration test reached into the presentation's fields |

### 6.4 The capital-encoding evidence

`alt_u_accepts_both_terminal_spellings_of_the_capital` drives **both**
spellings of the capital — `(ALT, Char('U'))` and
`(ALT | SHIFT, Char('u'))` — and asserts, for each, that the chord is
`Consumed` **and** that the document moved forward. A chord that was claimed
but did nothing would fail the second assertion.

### 6.5 The full-library sweep was **not green**, and why

`cargo test -p codegg --lib --no-fail-fast --locked` was run **twice** on the
same tree, and the failing set **changed between the two runs**:

| Run | Result | Failures |
|---|---|---|
| 1 | 6762 passed, **4 failed**, 4 ignored | `a_hidden_terminal_surface_does_not_arm_the_loop`, `a_live_terminal_surface_repaints_with_no_further_user_input`, `the_badge_names_insert_while_the_terminal_box_takes_keystrokes`, `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` |
| 2 | 6763 passed, **3 failed**, 4 ignored | `a_hidden_terminal_surface_does_not_arm_the_loop`, `a_live_terminal_surface_repaints_with_no_further_user_input`, `panel_rendered_buffer_shows_live_pty_output` |

**A deterministic regression cannot change its failing set between two runs of
the same binary.** One test that failed in run 1 passed in run 2, and one that
passed in run 1 failed in run 2.

Each failing test **passes in isolation on the same tree**:
`cargo test -p codegg --lib --locked interactive_terminal` → **94 passed, 0
failed**; `cargo test -p codegg --lib --locked
poolless_standalone_human_shell_uses_registered_workspace` → **1 passed, 0
failed**.

The failures are the **registered load-sensitive residual** the roadmap's
§13.5 names — *"the two load-sensitive `interactive_terminal` library
failures"* — and the count varies with the machine's load, which is what
"load-sensitive" means. They are in `src/tui/commands/interactive_terminal.rs`
and `src/core/daemon.rs`, neither of which this milestone touched and
neither of which depends on the editor's history model. The panic messages
are the fixtures' own ("the box is showing a terminal", "the fixture must
start with a mounted surface", "the panel adopted the new terminal") — setup
assertions, not behaviour assertions.

**The plan's §15 asks the closure to confirm the failing set is identical to
the registered residual. It is not identical — it is the same *class* with a
load-dependent count, and the variation is itself the evidence.** It is
recorded here rather than papered over: this milestone does not claim a green
full-library sweep, and it does not claim to have fixed the residual either.

`verify.sh full` was not run: the pre-existing 69 failing `tests/`
integration binaries are outside this milestone's blast radius, and a full
sweep would take the shared build lock for a long time under the load that
produced these flakes.

## 7. The exit conditions, with their evidence

| Exit condition | Evidence |
|---|---|
| The traversal is bounded and memory-capped | one cap over the history: 256 entries and 4 MiB, enforced in `trim_history` |
| The undo/redo round-trip is still exact | `editor_keeps_edit_through_undo_redo_save_and_reopen`, `editor_delete_and_motion_commands_produce_the_expected_text`, `alt_u_moves_the_history_cursor_back_and_the_document_follows` |
| A new edit after a traversal truncates correctly | `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set`, `the_first_edit_of_a_run_clears_the_redo_tail` |
| The memory bound on retained payload bytes is preserved | `undo_history_drops_the_oldest_entry_past_the_byte_cap`, `undo_history_drops_the_oldest_entry_past_the_depth_cap` |
| The selection set still survives a round-trip | `a_multi_selection_edit_is_undone_and_redone_as_one_step` |
| The model change preserves the existing behaviour | §8's two source injections |
| The guard's counts and the two count tests | §4, §6 |

---

## 8. The source-injection proof

Two injections into the implementation, each restored **byte-identical**
(`diff -q` reported no difference).

| # | Injection | Expected | Observed |
|---|---|---|---|
| 1 | Remove the boundary report from `traverse_history` | the two boundary-report tests fail | **both FAILED** — `alt_u_at_the_oldest_entry_reports_and_does_not_move` and `alt_u_at_the_tip_reports_and_does_not_move`; `test result: FAILED. 0 passed; 2 failed` |
| 2 | Move the history cursor **inside the refusal branch**, so a refused apply moves it anyway | `a_refused_undo_leaves_the_history_cursor_where_it_was` fails | **FAILED** — `assertion 'left == right' failed: a refused apply must not move the cursor · left: 0 · right: 1`; `test result: FAILED. 0 passed; 1 failed` |

Injection 2 is the one that matters most. A cursor that moved without a
matching change of text would leave the history describing a document that
does not exist, and no other test would notice.

The command in both cases is:

```bash
cargo test -p codegg --lib <filter> --locked
```

---

## 9. The recorded divergences

Three, all deliberate, all recorded in the plan's §17 and in the roadmap's
Amendment A2.

1. **The `Alt-u` / `Alt-U` gestures report their boundary; `u` / `U` do
   not.** A CodeGG convention, not a Helix behaviour — in Helix all four are
   the same movement. The plan's §3.5 and the roadmap's Amendment A1 both
   record it.
2. **A redo restores the pre-edit position rather than the position the
   reader was at when they undid.** See §3.2.
3. **The Normal-mode chord count is 69, not 68.** See §4 and the plan's
   §17.2.

---

## 10. Notes carried forward

- **The plan's §6.9 said the key-authority guard "needs no change". It did.**
  Any future milestone that adds a chord with a **multi-modifier** binding
  must write it as a `let` binding of a bitmask — `let alt_shift =
  KeyModifiers::ALT | KeyModifiers::SHIFT;` — and never as the or-pattern
  `m == alt | m == shift`, which is not a bitmask. The guard reads the
  binding form and cannot resolve the or-pattern.
- **The plan's §6.9 said the count goes "66 → 68"; the plan's §13.9 repeated
  it.** Both are corrected to 69. The *final* figure in the roadmap's §13.2
  moves from 70 to **71** as a consequence, and the stage table's deltas move
  with it: stage C is `69 → 70`, stage D is `70`, stage E is `70 → 71`.
- **The plan's §12 named `docs/tui.md`'s "Multi-key gestures" section as the
  home for the new gesture.** That section is about **prefixes** — gestures
  whose first key is held and shown in the indicator — and `Alt+u` is a
  single chord that needs no indicator. The history gestures are documented
  in a new **"History gestures"** section in `docs/tui.md` instead, and the
  editor-language-features introduction links to it.
- **The plan's §6.6 pointed the observable cursor at the string built in
  `src/tui/app/render.rs`.** That string is the *no-snapshot* fallback shell,
  not the status surface a reader looks at. The status surface is the banner
  row the `EditorWidget` paints, and that is where the position went.
- **The `Alt-u` / `Alt-U` chords are Normal-mode only.** `INS`'s catch-all
  covers `NONE` and `SHIFT` only, so an `Alt` chord reaches the composer
  there. Helix binds no `Alt-u` in Insert mode, so inert is the faithful
  answer — and it is asserted, not assumed.

---

## 11. The residual

- **The pre-existing failures are untouched by this milestone.** The 69
  failing `tests/` integration binaries, the load-sensitive
  `interactive_terminal` lib failures (three of them in one full-lib run, two
  in another — see §6.5), the four unrelated registry rows, and the
  `plans/closure/shared-provider-profile-contract/001-status.md` sibling
  pointer are all outside this milestone's blast radius, and none of them is
  claimed as fixed.
- **The `Ctrl+s`/`Ctrl+r` Insert-mode gap, the LSP-write-authorization gap,
  the unimpaired-navigation gap and the `o`/`O` gap** are unchanged. They are
  M009's and M010's business, not this milestone's.
- **`every_pending_prefix_display_fits_the_indicator_bound` still omits
  `ReplaceChar`.** Pre-existing, and outside the history model.
- **M014 remains blocked** on ADR-0034 acceptance and deliberately has no
  plan.
- **The plan's own §17.7 records that the refusal test moved to the apply
  side**, so the *model*-level test that replaced the old one no longer
  asserts what its name says on its own. The guarantee is pinned on the side
  that can actually break it.

---

## 12. Handoff

- The history is `history` / `history_bytes` / `history_cursor` on
  `TuiDocumentPresentation`. A reader that wants to know "where am I in the
  history" asks `presentation.history_position()`; it returns `None` at the
  tip.
- The traversal is `traverse_history(state, step, report)` in
  `src/tui/commands/editor.rs`. `report` is the only difference between
  `editor_undo` and `editor_history_earlier`.
- The chords are the three `Alt` arms in `handle_normal_key`, written against
  the exact `alt` and `alt_shift` bitmask bindings.
- The guard's self-test is `python3 scripts/check_tui_editor_key_authority.py
  --self-test` (10 cases) and its report is
  `python3 scripts/check_tui_editor_key_authority.py`.
