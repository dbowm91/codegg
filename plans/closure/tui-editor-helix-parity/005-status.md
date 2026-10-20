# TUI Editor Helix Parity — M005 closure: Textual motions and goto mode

Status: **closed**

Repository baseline: `84440fe7f` (branch `feat/auto-20261018-12255c45`)

Implementation plan:
`plans/implementation/tui-editor-helix-parity/005-textual-motions-and-goto-mode.md`

Predecessor closure:
`plans/closure/tui-editor-helix-parity/003-status.md`

Source roadmap:
`plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M005)

---

## 1. Outcome

`gs` reaches first non-whitespace — previously there was **no** key that could,
since `0` is the only line-start binding and every placement in an indented file
went through column 0. `W`/`B`/`E` jump by whitespace-delimited WORD, `gj`/`gk`
move by line, and goto mode is a mode with seven leaves rather than one chord.

Two things beyond the plan's scope turned up and were fixed: the word scanners
were skipping every non-ASCII character (§5), and `long_word_end` could not
terminate (§6).

## 2. Two corrections against the live Helix keymap

Re-read `https://docs.helix-editor.com/keymap.html` at the keystroke. Two leaves
in the plan were wrong, and both errors would have shipped a dead key:

- **`gl` is `goto_line_end`, not "last goto".** Helix binds `gh`/`gl` to line
  start and line end. The plan wanted a one-slot `gl`; there is no jumplist here
  to repeat, so it was both wrong about Helix and unimplementable as described.
- **`gt` is `goto_window_top`** — with `gc`/`gb`, top/middle/bottom **of the
  screen**. The plan read it as "last line". All three are **deferred**: motions
  resolve against `NOMINAL_VISIBLE_LINES`, a fixed guess rather than the real
  viewport, so they would land on a line that is not the top/middle/bottom of
  the reader's screen. Recorded omission, not silence.

Also corrected: `gj`/`gk` are Helix's **textual** line motions against `j`/`k`'s
*visual* ones — not aliases. With no soft-wrap display-line model the two
coincide, so the leaves resolve to the same `EditorCommand` rather than to
near-duplicates that could later drift apart.

## 3. The leaf table

`EditorPrefix::leaf() -> Option<char>` became
`EditorPrefix::leaves() -> &'static [PrefixLeaf]`, each row pairing a glyph with
a `PrefixAction::{Motion(EditorCommand), YankSelection}`. The action is an enum
because `space y` is not a motion — it must hand a byte range back out for the
App layer to copy. A one-glyph accessor only worked while goto mode had exactly
one leaf; Helix's has twenty and space mode thirty.

Every goto leaf resolves through the raw motion path, matching `gg`. It does
**not** clear a held selection, because `apply_motion` never touches
`selection` — a known divergence from Helix recorded in §9.

## 4. Two pre-existing behaviours changed, deliberately

1. **`j`/`k` now keep a desired column.** The existing test
   `vertical_motion_clamps_to_shorter_target_lines` asserted the opposite *on
   purpose* — "Column 2 is preserved relative to the line start, not to byte
   10". That is the deficiency §6 of the plan set out to remove and what
   Helix's `desired_col` prevents, so the test was rewritten and renamed
   `vertical_motion_keeps_the_column_across_a_shorter_line_and_resets_after_a_jump`.
   Because `gj`/`gk` are the same commands as `j`/`k` here, they share one
   column rather than starting a second, disagreeing run.
2. **`w`/`b`/`e` now work on non-ASCII text** — §5.

`gg` and `space y` behaviour tests passed **unmodified**. The one test that
changed asserts the *old accessor shape* (`EditorPrefix::leaf`), which is exactly
what this milestone replaces; its leaf column became the leaf **set** and every
other asserted fact is unchanged.

## 5. A pre-existing defect found and fixed: `w`/`b`/`e` skipped all non-ASCII

`byte_at(offset)` was `read_bytes(offset, offset + 1)`, and `read_bytes` refuses
a range that is not boundary-aligned at **both** ends — so it is `Err` for the
**first** byte of a multi-byte character as well as its interior. Every word
scanner read that as "not a word byte" and skipped forward.

Measured on `日本語 text` before the fix:

| From | `w` landed on | correct | `e` landed on | correct |
|---|---|---|---|---|
| byte 0 (日) | 14 — **end of document** | 10 | 14 — **end of document** | 6 |
| byte 10 (`t`) | 14 | 14 | 14 | 13 |

This is the defect class M003 eliminated in `find_char`; the word scanners were
never given the fix. It was invisible because ASCII single-byte characters are
the only kind that reads back, so the whole surface had no non-ASCII test. A
`word_end` doc comment asserted the **opposite** of the truth — that walking
`offset + 1` "can cross the interior of a multi-byte character and still
classify it correctly" — which was not true of the code it described.

**Fixed** by classifying with `leading_byte`, which reads the character's own
byte range. This was not optional: shipping `W`/`B`/`E` on the same primitive
would have left half the word-motion family skipping entire CJK words, and the
plan's own criterion that `W`/`B`/`E` are *observably distinct* from `w`/`b`/`e`
would have been meaningless.

**Verified before paying for a workspace build**, in a scratch crate depending on
`codegg-document` only: ASCII results are **byte-identical** to the pre-fix
behaviour at every boundary offset of `foo.bar baz`, `éx yó` and `日本語 text`.

## 6. An unbounded loop, found by sampling a stack

`long_word_end` spun forever on any document ending in a multi-byte character.
`next_char_boundary` returns `None` — not `Some(len_bytes())` — when the
remaining bytes are one multi-byte character, so
`cursor = next_char_boundary(..).unwrap_or(cursor)` had nowhere to advance and
the loop condition never changed. The existing `word_end` had the identical shape
but escaped by luck: `is_word_byte(0)` is `false` (NUL is not alphanumeric), so an
unreadable byte stopped it, whereas the inverted `is_long_word_byte(0)` is `true`.

Found by sampling the running test binary's stack after a 14-minute hang. The
reasoning that it must terminate was wrong: `len_bytes() - 1` is the last byte
of the final character, so it is **not** a character boundary for any multi-byte
character. Both `*_end` scanners now step one whole character at a time.

## 7. Non-vacuity

`scripts/check_tui_editor_prefix_state.py` was proven live on two injected
violations, each reverted:

| Injection | Result |
|---|---|
| `pub yank_prefix_pending: bool` re-added to `TuiDocumentPresentation` | exit 1 — *"declares 2 prefix field(s); exactly one is the whole point of M002"* |
| `pub fn leaves(` renamed to `pub fn leafs(` | exit 1 — *"expected declaration `pub fn leaves(` is absent"* |

The second is the newly-added required declaration, so the guard now fails if
the leaf table is removed, not merely if a second grammar reappears.
`check_tui_editor_key_authority.py` reports 40 Normal-mode chords (was 37) with
no shadowed arm.

## 8. Verification

Local only. No hosted CI run is claimed.

```
cargo fmt --check                                     clean
scripts/check_tui_editor_prefix_state.py             exit 0
scripts/check_tui_editor_key_authority.py            exit 0  (40 Normal-mode chords, no shadowed arm)
cargo clippy --workspace --all-targets -- -D warnings exit 0
cargo test -p codegg --lib tui::                      1993 passed, 0 failed
cargo test -p codegg --lib tui::editor                126 passed, 0 failed
cargo test -p codegg --lib tui::commands::editor       77 passed, 0 failed
scripts/verify.sh quick                               passed (exit 0)
```

### 8.1 One flake seen and not attributed to M005

In an intermediate run,
`tui::commands::interactive_terminal::tests::a_refused_clear_reports_itself_and_leaves_the_screen_intact`
failed ("a refused clear must leave the screen intact, got `\"\"`"). It **passed**
in the final full run above, and `src/tui/commands/interactive_terminal.rs` and
`src/tui/interactive_terminal.rs` are unmodified (`git diff --stat HEAD` returns
nothing for them) — the panel does not reference `crate::tui::editor`,
`TuiDocumentPresentation`, or any helper M005 changed, and carries its own
`next_word_end`/`prev_word_start` over `&str` column indices. So it is a flake in
a surface this milestone does not touch, not a regression. Recorded because
`plans/closure/tui-editor-helix-parity/003-status.md` §12 lists pre-existing
terminal-panel failures on an earlier tree, and it would be easy to mis-attribute
the next one.

## 9. Terminal panel: an explicit exemption, and a recorded gap

The panel does **not** gain `W`/`B`/`E` or any goto leaf, and does not need to:
it has no `g` prefix, its `j`/`k` scroll a viewport rather than moving through
document lines, and "first non-whitespace" has no meaning over a cell grid.
`long_word_*` was deliberately not introduced there — a second word-end
definition for a surface with no long-word key is the divergence the roadmap
forbids.

**Correction to an earlier claim in this workstream**: the panel does *not* share
the editor's `word_end`/`word_backward`. The claim appears in
`architecture/tui.md` and was repeated in the M005 plan; both were wrong and are
fixed. The consequence is recorded rather than smoothed over — M005 fixed the
editor's scanners for non-ASCII, which does **not** fix the panel's, so the two
surfaces agree on ASCII and may differ on multi-byte scrollback. Owned by M008's
panel verification.

## 10. Known divergence from Helix

A goto leaf does not collapse a held selection, because `apply_motion` never
touches `selection` and `gg` never did. Changing that is shared with the bare
`G`, `w` and `b` and is out of scope. What M005 owns is parity between the old
leaf and the new ones, pinned by
`every_goto_leaf_leaves_a_held_selection_exactly_as_gg_does`, which establishes a
`gg` baseline and compares every leaf against it.

## 11. Deliberately unbound, each recorded

`gd`/`gy`/`gr`/`gi` (LSP — M010, gated on M013); `gt`/`gc`/`gb` (screen-relative,
need the real viewport); `gw` (no anchor model); `g|` (no count entry point);
`gf`/`ga`/`gm`/`gn`/`gp`/`g.` (multi-document attachment is a predecessor
non-goal); `[`/`]` leaves (M006).

## 12. Files changed

| File | Change |
|---|---|
| `src/tui/editor.rs` | leaf table + `PrefixLeaf`/`PrefixAction`/`GOTO_LEAVES`/`SPACE_LEAVES`; `FirstNonWhitespace` + `long_word_*` commands; `leading_byte` replacing `byte_at`; `word_end`/`long_word_end` whole-character stepping; `desired_column_after`; `first_non_whitespace`; `clear_history` resets the column |
| `src/tui/document_session.rs` | `desired_column` field, reset on open |
| `src/tui/commands/editor.rs` | leaf-table resolver; `W`/`B`/`E` arms; desired-column clears on edit/undo/redo/insert-entry/LSP jump; tests |
| `scripts/check_tui_editor_prefix_state.py` | model documentation; `pub fn leaves(` added to required declarations |
| `docs/tui.md` | goto-mode, long-word and desired-column sections |
| `architecture/tui.md` | leaf table, motion scanners, terminal-panel exemption, corrected selection claim |
| `plans/implementation/tui-editor-helix-parity/005-textual-motions-and-goto-mode.md` | keymap corrections in §1/§5/§6/§13; §16 records the behaviour changes and the two defects |
| `plans/subsystems/tui-editor-helix-parity-roadmap.md` | M005 deliverable boundary and deferrals corrected |
| `plans/registry.md` | M005 registered `closed` |

## 13. Unblock audit

M013's hard edge on M005 is discharged — the leaf table it depends on now
exists. M010 still waits on M005 **and** M013. M004 was already dependency-clean.
M009 remains blocked on ADR-0034 acceptance.