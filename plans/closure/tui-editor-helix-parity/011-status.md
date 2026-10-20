# TUI Editor Helix Parity Milestone 011 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/011-insert-mode-control-chords.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M011), §13 (stage A)
Repository baseline reviewed: `f15bdf522` (the commit that registered this plan and
landed wave 1: the byte match foundation, the pending-effect generalization, and the
key-authority guard's Insert-shape support)

---

## 1. Executive finding

M011 is **closed**. The nine Helix insert-mode chords are bound and each has exactly
one owner; the two panels that used to eat the reader's typing are mode-aware; a
typing burst is one undo step instead of one per keystroke; and the key-authority
guard counts the new table correctly and still fails on an injected duplicate and on
an injected shadow of `Ctrl+s`.

Three things in this milestone are worth recording beyond the checklist, because each
is a decision a later milestone would otherwise have to re-litigate.

1. **The coalesced run is one *contiguous insertion*, and that restriction is what
   makes the merged entry exact.** See §3. The plan's stop condition — that
   coalescing must not require a change to `EditorUndoEntry`'s shape — was **not**
   reached: the constraint was met by narrowing what a run is, not by changing the
   entry. `EditorUndoEntry` still holds a forward and an inverse transaction, and
   M012 can still traverse the entries.

2. **The guard's Insert chord count is 8, not the roadmap's 9.** The roadmap's §13
   stage table said `1 → 9`; the count that actually lands is `1 → 8`. The reason
   is arithmetic, not a defect: the guard counts *character* claims, and of the nine
   chords two are key-code claims (`Alt+Backspace`, `Alt-Delete`, which it checks
   under rule 3 for exactly-once ownership) and one (`Ctrl+x`) lives in
   `handle_completion_key`, a different dispatch stage the guard does not scan. The
   roadmap is amended; see §6. The **Normal-mode** count is unaffected at 66, and
   M012's acceptance criterion pins that one, not this one.

3. **The plan contradicted itself about `Tab`, and the table won.** §6.3's prose
   said the candidate is *accepted* with `Tab`; §6.3's table and acceptance
   criterion 4 said `Tab` *moves the candidate selection*. Helix's own completion
   menu binds `Tab` to *Next entry*, not to accept. The table was implemented and
   the prose was corrected. See §4.

---

## 2. The work packages that landed

| Package | What landed | Files |
|---|---|---|
| W2 | The completion popup and the LSP diagnostics panel became mode-aware. | `src/tui/commands/editor.rs` |
| W3 | Three deletion builders: `kill_to_line_start`, `delete_word_backward`, `delete_word_forward`. | `src/tui/editor.rs` |
| W4 | The insert table became a tuple match on `(modifiers, code)` and gained the nine chords. | `src/tui/commands/editor.rs` |
| W5 | The insert session: a typing burst is one undo entry. | `src/tui/editor.rs`, `src/tui/document_session.rs`, `src/tui/commands/editor.rs` |
| W6 | The docs, the closure record, and this milestone's plan amendment. | `docs/tui.md`, `architecture/tui.md`, `CHANGELOG.md`, `.opencode/skills/tui/SKILL.md` |

W1 (the guard, taught the Insert tuple shape) landed in the previous commit,
`f15bdf522`, and is the baseline for this record.

---

## 3. The undo granularity change: **it landed**

**The undo granularity change landed.** A typing burst is now one undo step.

`TuiDocumentPresentation::insert_session` holds the open run. An edit joins it only
when it is a **pure insertion at the end of the run so far**; a deletion, a
replacement, or an insertion anywhere else ends the run, and the parts become
separate steps. The run is committed as a single `EditorUndoEntry` — one forward
transaction inserting everything the run typed, one inverse transaction deleting
exactly those bytes — when insert mode ends.

**What `u` and `U` do across an insert session:**

- `u` in Normal mode, with a burst open, commits the burst first and then undoes
  it. One `u` takes back the whole run, not its last keystroke.
- `U` in Normal mode is unchanged: it redoes. It does **not** commit an open
  session, because committing one would push an entry and clear the redo stack the
  reader is asking to use. An open session and a non-empty redo stack cannot
  coexist: the session's first edit clears the redo tail.
- A reconciliation — open, reload, resync, reconnect, close — **discards** an open
  session rather than committing it. The text the session described is the
  pre-reload text, and committing it would record a step that restores a document
  the reader is no longer looking at.

**Why the run is restricted to a contiguous insertion.** A `TextTransaction`'s
edits are all expressed in the coordinates of one base text: `DocumentBuffer::apply`
walks them in reverse order against the original. Two insertions at the same base
place therefore cannot both be expressed in one transaction, and the application
order of same-place insertions is not the order they were typed in. A run that is
not one contiguous insertion has no single merged form. Narrowing the run is what
kept the plan's stop condition from being reached — `EditorUndoEntry`'s shape is
unchanged, and its "both directions" invariant still holds.

**Four existing tests changed**, because they asserted the old granularity:

| Test | Was | Now |
|---|---|---|
| `a_rapid_edit_burst_serializes_with_one_change_in_flight` | `undo_depth == 25` — "every keystroke is individually reversible" | `undo_depth == 1` — the whole burst is one step. The daemon still saw 25 changes; only the *history* coalesces. |
| `editor_keeps_edit_through_undo_redo_save_and_reopen` | asserted the depth right after typing | closes the burst first, then asserts the depth |
| `switching_documents_clears_history_and_drops_the_previous_attachment` | asserted the depth right after typing | closes the burst first, then asserts the depth |
| `undo_history_does_not_survive_reload_resync_or_close` | asserted the depth right after typing | closes the burst first, then asserts the depth — there must be a step for the reconciliation to clear |

---

## 4. The `Ctrl+s` / `Ctrl+r` decision

**Decided: declined.** In Insert mode, `Ctrl+s` still saves and `Ctrl+r` still
resyncs. Helix's insert-mode meanings — `Ctrl+s` as *commit undo checkpoint* and
`Ctrl+r` as *insert register* — are **not** adopted.

**The rejected alternative.** Helix's `Ctrl+s` is an undo checkpoint, and the
natural way to match it is to make `Ctrl+s` commit the insert session and do
nothing else. That was rejected for two reasons. The guard block is
*mode-independent* and *outranks* the mode dispatch, so a `Ctrl+s` that meant two
different things in two modes would need a second owner — the M001 defect class,
and the exact class the key-authority guard exists to catch. And the checkpoint is
already covered: the insert session commits when insert mode ends, which is the
same checkpoint at the same granularity, reached without a second owner.

Helix's `Ctrl+r` (*insert register*) is declined because there is no register: the
roadmap defers named registers, and `p`/`P` already paste the OS clipboard. It is
declined for want of a thing to insert, not because the chord was contested.

**The three tests that continue to pin the mode-independent promise:**

- `ctrl_s_and_ctrl_r_still_reach_the_lifecycle_chords_in_insert_mode` (new) —
  `Ctrl+s` in Insert yields `BufferKey::RequestSave` and edits nothing; `Ctrl+r`
  yields `BufferKey::RequestResync` and edits nothing.
- The pre-existing test at the old `:5618` — `Ctrl+s` in Insert still yields
  `BufferKey::RequestSave` — still passes, unedited.
- `scripts/check_tui_editor_key_authority.py` — the guard block is two chords
  (`Ctrl+s`, `Ctrl+r`), and rule 1 fails the build if any mode table re-declares
  either of them. The injection proof in §7 is that rule firing.

---

## 5. The three recorded divergences from Helix

All three are the same decision, and it is the plan's invariant 3: **in Insert
mode a printable is text.** Helix's completion menu takes the keypress; CodeGG
does not, because a printable the reader typed must reach the document.

| Helix's completion menu | CodeGG | Doc reference |
|---|---|---|
| `Enter` — close the menu and accept the completion | `Enter` inserts a newline and dismisses the popup | `docs/tui.md` § Completion; `architecture/tui.md` § *Normal returns `BufferKey::Unhandled`…* |
| `↑` / `↓` — previous / next entry | `↑` / `↓` move the cursor | `docs/tui.md` § Completion; `architecture/tui.md` § *Normal returns `BufferKey::Unhandled`…* |
| "any other keypresses result in the completion being accepted" | a printable **types**, and the popup is dismissed first | `docs/tui.md` § Completion; `architecture/tui.md` § *Normal returns `BufferKey::Unhandled`…* |

`Tab` and `Shift+Tab` **match** Helix (next / previous entry), and the divergence
is recorded in `docs/tui.md` under *Completion* and in `architecture/tui.md` under
*Normal returns `BufferKey::Unhandled` for every key it does not own*.

The full Helix comparison — every chord, matched or not — is in the plan's §17.7.

---

## 6. The chord counts

The guard's printed line, before and after:

```
before:  TUI editor key-authority guard passed — 2 guard-block chord(s), 66 Normal-mode chord(s), 1 Insert-mode chord(s), no shadowed arm
after:   TUI editor key-authority guard passed — 2 guard-block chord(s), 66 Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm
```

**The Normal-mode count is unchanged at 66**, which is what M012's acceptance
criterion pins. The Insert count goes `1 → 8`:

- the wildcard character arm (1) — `NONE`/`SHIFT` plus any printable;
- seven character chords: `Ctrl+h`, `Ctrl+d`, `Ctrl+j`, `Ctrl+k`, `Ctrl+u`,
  `Ctrl+w`, `Alt+d`;
- two **key-code** claims that the guard counts under rule 3 rather than as
  chords: `Alt+Backspace`, `Alt+Delete`;
- `Ctrl+x`, in `handle_completion_key` — a different dispatch stage, and one the
  guard does not scan.

**The roadmap's §13 stage table said `1 → 9`. It is amended to `1 → 8`**; the
amendment is in the roadmap's M011 entry and is dated.

---

## 7. The guard-injection proof

The guard is load-bearing, so it was proved non-vacuous by injection rather than
asserted. Two defects were injected into `src/tui/commands/editor.rs`, the guard
was run against each, and the file was restored byte-identically afterwards.

**Injection 1 — a duplicated Insert chord.** A second
`(KeyModifiers::CONTROL, KeyCode::Char('w'))` arm was added to `handle_insert_key`.

```
$ python3 scripts/check_tui_editor_key_authority.py
TUI editor key-authority violations found.
...
  editor.rs:2960 [handle_insert_key] (KeyModifiers::CONTROL, KeyCode::Char('w')) re-declares CONTROL+'w', already claimed at editor.rs:2956 ((KeyModifiers::CONTROL, KeyCode::Char('w'))) — unreachable

1 violation(s) found.
exit=1
```

**Injection 2 — an Insert chord shadowing `Ctrl+s`.** A
`(KeyModifiers::CONTROL, KeyCode::Char('s'))` arm was added to `handle_insert_key`.

```
$ python3 scripts/check_tui_editor_key_authority.py
TUI editor key-authority violations found.
...
  editor.rs:2960 [handle_insert_key] (KeyModifiers::CONTROL, KeyCode::Char('s')) claims CONTROL+'s', but the guard block at editor.rs:3281 already claims CONTROL+'s' in Insert — unreachable

1 violation(s) found.
exit=1
```

**Restoration.** `diff -q` against the pre-injection copy reported no difference,
and the guard passed again on the restored file. The guard's own `--self-test`
(9 cases) also passes, and it exercises the *analyser* rather than a snapshot of
the count: each case runs a synthetic table through the same analysis the real
check uses.

---

## 8. The test evidence

| Suite | Command | Result |
|---|---|---|
| TUI library | `cargo test -p codegg --lib tui:: --locked` | **2241 passed, 0 failed** (2215 before this milestone; +26 new) |
| Guard | `python3 scripts/check_tui_editor_key_authority.py` | passed — 2 / 66 / 8 |
| Guard self-test | `python3 scripts/check_tui_editor_key_authority.py --self-test` | 9 case(s), passed |
| Formatting | `cargo fmt --check` | clean |
| Lints | `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Sweep | `bash scripts/verify.sh quick` | passed |

**The new tests, all driven through the real dispatch path**
(`handle_editor_key` → `editor_buffer_key` → `handle_insert_key`), in
`src/tui/commands/editor.rs`:

Chords — `ctrl_w_deletes_the_previous_word`, `ctrl_w_stops_at_the_line_start`,
`alt_backspace_is_the_same_gesture_as_ctrl_w`, `alt_d_deletes_the_next_word`,
`alt_d_stops_at_the_line_end`, `ctrl_u_deletes_to_the_line_start_and_ctrl_k_to_the_line_end`,
`ctrl_h_and_ctrl_d_delete_one_character_each`, `ctrl_j_inserts_a_newline`,
`ctrl_x_is_an_insert_mode_chord_only`.

Negative chords — `ctrl_s_and_ctrl_r_still_reach_the_lifecycle_chords_in_insert_mode`.

Mode blindness — `the_completion_popup_does_not_steal_enter_in_insert_mode`,
`the_completion_popup_does_not_steal_the_arrows_in_insert_mode`,
`the_lsp_panel_does_not_steal_characters_in_insert_mode`,
`the_lsp_panel_does_not_steal_enter_in_insert_mode`.

Coalescing — `a_typing_burst_is_one_undo_step`,
`a_second_undo_reverts_the_edit_before_the_burst`,
`a_reconciliation_discards_an_open_burst`,
`typing_undoing_and_typing_again_leaves_the_redo_stack_empty`.

Builders and the session, in `src/tui/editor.rs` —
`kill_to_line_start_deletes_the_line_head_and_nothing_more`,
`delete_word_backward_stops_at_the_line_start`,
`delete_word_forward_stops_at_the_line_end`,
`the_word_deletions_never_split_a_multi_byte_character`,
`a_deletion_ends_the_typing_run`,
`an_insertion_away_from_the_end_of_the_run_ends_it`,
`a_run_is_committed_as_one_insertion_with_one_inverse`,
`the_first_edit_of_a_run_clears_the_redo_tail`.

**The insert-mode sweep is unchanged and still asserts `inspected == 93`.** It was
not edited, and it is the non-vacuity check for the Insert table.

---

## 9. The exit conditions, with their evidence

| Exit condition (§13) | Met | Evidence |
|---|---|---|
| 1. Each of the nine chords does its documented thing in Insert mode, and nothing in Normal mode | yes | the nine chord tests above; the guard's per-mode tables; `Ctrl+x` in Normal mode is not the editor's to take (`ctrl_x_is_an_insert_mode_chord_only`) |
| 2. `Ctrl+s` still saves and `Ctrl+r` still resyncs; the Helix insert-mode meanings are declined and documented | yes | §4; `docs/tui.md` § *Insert-mode chords*; `architecture/tui.md` § *Key dispatch authority* |
| 3. A typing burst is one undo step, and a reload discards an unfinished burst | yes | `a_typing_burst_is_one_undo_step`, `a_reconciliation_discards_an_open_burst` |
| 4. With the completion popup open in Insert mode, `Enter` inserts a newline, `↑`/`↓` move the cursor, and `Tab` moves the candidate selection | yes | the two popup tests; `Tab` is covered by the plan's §17.1 decision and the divergence record |
| 5. With the LSP panel open in Insert mode, `j`/`k`/`r` type and `Enter` inserts a newline | yes | the two panel tests |
| 6. In Insert mode, a printable always reaches the document, whatever panel is open | yes | the two panel tests and the two popup tests |
| 7. `Ctrl+w` at the start of a line deletes nothing | yes | `ctrl_w_stops_at_the_line_start`, `delete_word_backward_stops_at_the_line_start` |
| 8. The insert-mode sweep still asserts `inspected == 93` | yes | the sweep was not edited |
| 9. The guard fails on an injected duplicate Insert chord and on an injected shadow of `Ctrl+s` | yes | §7, with the commands and the observed failures |
| 10. `verify.sh quick` passes, and the closure states the new chord counts | yes | §6 and §8 |

**The stop conditions (§14), none of which was reached:**

- *Coalescing requires a change to `EditorUndoEntry`'s shape* — **not reached.**
  The run was narrowed to a contiguous insertion instead; the shape is unchanged.
- *The guard cannot be taught the new shape without weakening what it catches* —
  **not reached.** The guard already read the tuple shape from wave 1, and it still
  fails on both injections.
- *A chord cannot be given exactly one owner* — **not reached.** Rule 2 and rule 3
  hold, and the injections prove they are live.
- *The insert-session coalescing and M012's history traversal cannot both hold* —
  **not reached.** M012 traverses entries, and an entry is still an entry. M012 must
  not assume one entry per keystroke, which is recorded in its own plan.
- *A panel's key claims cannot be made mode-aware without changing the panel's
  Normal-mode behaviour* — **not reached.** Every Normal-mode claim is unchanged;
  the mode-aware arms are additional guards on the same behaviour.
- *The `Ctrl+w` / `Ctrl+k` route-scoped shadow is judged unacceptable* — **not a
  defect, but a product decision that was not asked for.** `Ctrl+w`, `Ctrl+k` and
  `Ctrl+u` are the editor's in Insert mode, and keep their global meanings
  everywhere else. `Alt+Backspace` and `Alt+d` are offered as the
  modifier-free alternatives, so a reader who objects to the shadow has a path
  that does not use it. **This remains open for the maintainer.**

---

## 10. Notes carried forward

- **`src/tui/components/help_overlay.rs` is not a doc surface** and was not
  edited. It is unreferenced dead code: it is absent from `components/mod.rs`,
  never compiled, and carries none of the shipped help. The live surfaces are
  `docs/tui.md`, `architecture/tui.md`, and the shipped `/help`. The residual is
  carried forward unchanged from the plan's §3.12.
- **The pre-existing test name `the_eight_alt_chords_are_inert_in_insert_mode`
  names eight and lists nine.** It is pre-existing and untouched by this
  milestone; the list contains no chord this milestone binds (`Alt-d` is not in
  it), so the test still holds. It is recorded here rather than fixed, because
  renaming a test is not this milestone's business.
- **The Insert table's plain keys now accept `NONE` and `SHIFT` only.** Before
  this milestone they matched any modifier, so `Ctrl+Backspace` and `Ctrl+Left`
  were the editor's. They are not any more: an unclaimed insert chord is a
  **global** action, which is the plan's §3.3 finding and Helix's arrangement. No
  Helix insert-mode binding is lost. Recorded in the plan's §17.3.
- **The `o` / `O` open-line edit joins the burst.** `o` inserts a newline and
  enters Insert mode, and that newline is the first edit of the next run, so `o`
  followed by typing is one undo step. This is Helix's behaviour — `o` opens the
  line and the typing belongs to the same checkpoint.

---

## 11. The residual

**M011 leaves no defect of its own.** The next milestone is M012 (undo history
traversal), which must not assume one entry per keystroke: an entry is now a
*burst*, and M012's history cursor traverses bursts.
