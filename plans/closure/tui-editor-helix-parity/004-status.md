# TUI Editor Helix Parity — M004 closure: Core change verbs

Status: **closed**

Repository baseline: `84440fe7f` (branch `feat/auto-20261018-12255c45`)

Implementation plan:
`plans/implementation/tui-editor-helix-parity/004-core-change-verbs.md`
(amended — see §2)

Predecessor closure:
`plans/closure/tui-editor-helix-parity/003-status.md`

Source roadmap:
`plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M004)

---

## 1. Outcome

A user can now move text. Before this milestone `space y` yanked a selection
and **nothing** could put it back, so any cut-and-paste left the editor.
`y` → `p` is now a complete round trip, and `r`, `R`, `~`, `` ` `` and
`` Alt-` `` cover the rest of the basic change loop.

The literal scope landed as `p`, `P`, bare `y`, `r`, `R`, `~`, plus `` ` ``
and `` Alt-` `` — the last two because the roadmap's M004 deliverable boundary
names them and Helix binds them, even though the implementation plan's §5 did
not spell them out. Where the implementation plan and the roadmap disagreed,
the roadmap won; it is the subsystem-level artifact and it was already correct
about `R` (§2).

## 2. The plan was wrong about `R`, and the roadmap was right

The implementation plan says, in §1, §5, §6 and §13 alike, that `R` replaces
"to end of line" with captured text, entering Insert mode with a retained tail
removed. That is vi's `R`. It was an older Helix binding.

The current [Helix keymap](https://docs.helix-editor.com/keymap.html), re-read
at the keystroke — the method M005 established when it corrected `gl` and
`gt` — reads:

> `R` | Replace with yanked text

So `R` takes no argument, enters no Insert mode, and retains no tail. Every
paragraph of plan §8 about a "retained R tail" described a design for a verb
that does not exist, including the milestone's single flagged judgement call
and its §14 stop condition.

**The plan also contradicted itself and its own roadmap.** Its §5 bullet said
"`R` replace from the cursor to end of line with captured text" and then, in
the same bullet's parenthetical, "(Helix: enters Insert mode seeded with the
retained tail removed)" — "with captured text" and "no argument" cannot both be
true. The roadmap's M004 deliverable boundary already said "**`R`
replace-with-yanked**".

Recorded as plan amendment A1 (`004-core-change-verbs.md` §17). The plan above
that section is left unamended so the original reasoning stays auditable.

### What the correction removed

The whole retained-tail design: no presentation field, no `Esc` restore hook,
no cancellation semantics, no "must not survive a buffer switch" requirement.
The text-authority guard is not asked to arbitrate a field that does not exist.

It also moved the second capture prefix. Only `r` takes an argument, so
`EditorPrefix::ReplaceChar` is the **only** new variant M004 adds.

## 3. `R` was already bound, and the guard made that a hard problem

`R` reloaded the buffer from disk, as a Normal-only **guard-block** chord in
`editor_buffer_key` — the stage answered *before* mode dispatch, and therefore
capable of both shadowing a Normal-mode verb and surviving into Insert.

That combination is exactly what `scripts/check_tui_editor_key_authority.py`
exists to reject, and re-adding the chord fails it (proved in §6). So the
chord and the `BufferKey::RequestReload` variant are gone, and the guard's
`REQUIRED_SUBSTRINGS` moved from three guard-block chords to two.

**Reload survives as `:e`.** `EditorCommandName::Reload` already reached
`reload_editor` directly, so no capability was lost. A *keystroke* was, and
that is the observable cost of this milestone: it is called out in
`docs/tui.md`, in `architecture/tui.md`, and in the help overlay.

## 4. There is no editor-local register, and that is not an oversight

`y` and `space y` are the same code path (`yank_range` → `yank_editor_range` →
`crate::util::clipboard`, which is `arboard` behind a `Result`/`Option`).
Nothing in `TuiDocumentPresentation` stores yanked text.

Two things follow, and both are user-visible:

- `p` pastes **whatever is on the OS clipboard**, including text copied from
  another application. This is why Helix's `R` is `replace_with_yanked` and
  not a register operation.
- Three `BufferKey` variants carry a `ClipboardVerb` rather than the bytes,
  because the clipboard is the App layer's, not editor state — the same
  division of labour as `YankRange` carrying a range.

A failed read, an empty clipboard, and a read-only buffer are three different
answers and each is reported. Without the empty-clipboard toast, "you copied
nothing" and "`p` did not work" look identical.

## 5. `r` is a second capture grammar, and the classifier is one call

`r` arms `EditorPrefix::ReplaceChar`, whose leaf table is empty because its
next key is a *character to write*, not a leaf glyph. `resolve_pending_prefix`
now has **one** capture branch rather than two: `EditorPrefix::is_capture`
classifies, `capture_char` decides what a target can be, and the
retry-on-no-target rule is written once.

That was not the first draft. The first draft had a find branch and an `r`
branch, which left `is_capture` as a doc-comment claim with **no production
caller** — the "a variant with no producer is a claim, not a hook" failure this
repository's own `AGENTS.md` calls out. The guard injection in §6 caught it.

## 6. Non-vacuity

Both guards were proved to fire on the violations they claim to prevent, then
reverted. `git diff --stat` after the revert shows no residue.

| Injection | Guard | Result |
|---|---|---|
| Re-add a Normal-only guard-block `R` reload chord | `check_tui_editor_key_authority.py` | **exit 1** — "`handle_normal_key` claims NONE\|SHIFT+`R`, but the guard block at `editor.rs:2840` already claims NONE+`R` in Normal — unreachable" |
| Rename `is_capture` → `is_a_capture` | `check_tui_editor_prefix_state.py` | **exit 1** — "expected declaration `fn is_capture(` is absent" |

Both are clean again: 2 guard-block chords, **48** Normal-mode chords (M003
left 37, M005 left 40, M004 adds eight), 1 Insert-mode chord, no shadowed arm.
Both injections were re-run against the final tree after the case-setter verbs
were added, so the counts above are the ones the closed state ships.

### A gap found and left open

A third injection — giving `ReplaceChar` a **non-empty** leaf row — passes
**both** guards. They are static and cannot see that the capture branch returns
before the leaf lookup, and the runtime behaviour is still correct.

Rather than leave that asserted nowhere, it is covered by a test that sweeps the
whole capture class: every capture must have an empty `leaves()`, must not
swallow an unmatched key, and `is_capture` / `is_replace_char` must be
disjoint. The honest summary is that the invariant is pinned by a **test**, not
by the static guard.

## 7. Design decisions worth knowing

**One funnel, five verbs.** `r`, `R`, `~`, `` ` `` and `` Alt-` `` go through
`editor_editing_verb`, not `resolve_edit`. Same read-only gate, same snapshot,
same `EditorEdit`, same `submit_edit`; one difference — they resolved their
target *from* the selection, so they clear it. `submit_edit` deliberately
leaves `presentation.selection` alone, and a selection left over a replaced
range is a highlight painted over text that is no longer there.

`resolve_edit` was **not** extended. A verb whose payload comes from the
clipboard cannot be expressed as a key identity without turning `EditorCommand`
into a data-carrying enum that the motion machine and the `Alt-.` recorder must
also understand.

**One target rule for all five.** Helix names the target as *the selection*; a
bare cursor here is an offset, not a selection. So: the held selection when
there is one, otherwise the character under the cursor. With nothing selected
the two readings coincide for one character, which is why `~` flips one letter
or a whole phrase by the same code.

**`~` flips; `` ` `` and `` Alt-` `` set.** One `case_range` with a `CaseMode`
drives all three. The difference is observable on the second press: `~ ~`
returns the original, while `` ` `` on already-lower-case text is a no-op and is
*reported* as one rather than submitted as an edit that pushes an undo entry
changing nothing.

**Unicode expansion, handled differently on purpose.** `` Alt-` `` on `ß` gives
`SS` and `` ` `` on `İ` gives `i` + U+0307 — the whole expansion, because those
verbs name a whole case. `~` on `ß` does **nothing**, because a flip asks for
*the other case of this character* and `ß` has no single one. Taking half of
`SS` would also mean `~ ~` never returned to the original, so `~` would stop
being a toggle. Both are pinned by tests.

**Cursor placement is the same for all of them: the last byte written.** In
Normal mode the cursor sits *on* a character, so leaving it one past the edit
would put it on whatever followed — and `p p` would step over the character
between the two pastes instead of appending. None is `stay_insert`.

**`y` reuses `space y` exactly.** Same helper, same toast, same selection
drop. Helix keeps the selection after a yank; this editor drops it, and it
already did for `space y`, which the plan required preserving byte for byte.
Two spellings of one verb that behaved differently would be a worse surprise
than either behaviour alone.

## 8. Two defects found beyond plan scope

**A one-character document had no character under the cursor.**
`next_char_boundary(0)` on `"日"` returns `None`, because that function answers
"where does the *next* character begin" and there isn't one. Read naively that
is "no character under the cursor", so `r`, `~` and `p` were silent no-ops in a
one-character file. `char_end` resolves it to the document length, which is
what tells "the cursor is at the end" apart from "the document is one
character long". This is the same `None`-versus-`Some(len)` distinction M005
hit in `long_word_end`, and it was found here only because a test happened to
use a character whose lowercasing expands.

**`p` on `~` disagreed on where the cursor goes.** The first draft put a paste
at `at + text.len()` — the offset *past* the pasted text, correct for Insert
mode where the cursor is a bar between characters, wrong in Normal mode where
it sits on one. `p p` stepped over the character between the two pastes. Both
families now end on the last byte written.

## 9. The help overlay: inspected, and deliberately left alone

`src/tui/components/help_overlay.rs` is a third keymap surface the plan did not
list, and it has drifted badly — it lists `Ctrl+R (redo)` (redo has been `U`
since M001), lists `dd dw` (removed in M001; Helix deletes a *selection*), and
omits eleven implemented verbs.

**None of it was fixed, because none of it compiles.**
`components/mod.rs` does not declare the module; `architecture/tui.md`'s module
map marks it "Dead code (help is mode-aware via input.rs)", and the M001
closure records that edits to this file during that milestone were **reverted**
on exactly that ground.

This milestone briefly edited it, including adding a test that fails if any
binding mentions `redo`. Reverted on the same ground. The reason is worth
stating plainly, because it is the failure mode the edit invited: the test
**never runs**, so `cargo test` would have stayed green while appearing to
provide regression cover. A test that does not execute is worse than no test,
because the next reader counts it as evidence.

The drift itself is real and remains a carried-forward finding, now recorded in
its fifth consecutive closure (M001 §, M002, M003, and here). The live help
surface is `input.rs`'s mode-aware bindings, and it is covered by
`builtin_shift_letters_are_composed_text_in_insert_mode`.

## 10. Known divergence from Helix

| Divergence | Why |
|---|---|
| `y` and `space y` drop the selection | Helix keeps it. Already true of `space y` before M004; changing it was out of scope and would have made two spellings of one verb behave differently. |
| `p` pastes the OS clipboard, not a register | There is no register. Same as Helix's clipboard integration, so this is a match, not a divergence — recorded because the *absence* is structural. |
| `r :` cannot replace with a colon | `:` outranks a pending target, inherited from the M003 find grammar. Consistency with `f :` was chosen over consistency with Helix. |
| `~` on `ß` is a no-op | Helix's own case conversion is not specified for it. A flip has no single answer; leaving it alone keeps `~ ~` a toggle. |

## 11. Deliberately not added

`space p` / `space P` (Helix `paste_clipboard_after` / `_before`) are the same
three transforms reached through the space-mode leaf table — one row each. The
milestone that owns `SPACE_LEAVES` adds them, so M004 does not quietly widen
the table it was given.

`c` / `Alt-c`, noyank variants, `>` / `<`, `=`, registers, `J`, `.`, `` ` ``-with-
a-register — all deferred as the plan's §5 recorded.

## 12. Verification

All **local**. No hosted CI run is claimed.

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0** |
| `cargo test -p codegg --lib tui::` | **2031 passed, 0 failed** |
| `cargo test -p codegg --lib tui::editor` | included above |
| `cargo test -p codegg --lib tui::commands::editor` | included above |
| `python3 scripts/check_tui_editor_prefix_state.py` | exit 0 |
| `python3 scripts/check_tui_editor_key_authority.py` | exit 0 — 2 guard-block chords, **48** Normal chords, no shadowed arm |
| `python3 scripts/check_tui_editor_text_authority.py` | exit 0 — no retained text buffer |
| `bash scripts/verify.sh quick` | see §12.1 |

### 12.1 `verify.sh quick`

`bash scripts/verify.sh quick` → **"Quick verification passed."**, exit 0,
including the prefix-state, key-authority, text-authority, execution-ownership,
TUI-authority, http-route-disposition, audit-coverage, scheduler-bypass and
config-merge-coverage guards and `cargo check --workspace --all-targets
--locked`.

### 12.2 Flakes seen and not attributed to M004

Three separate runs each surfaced one or two failures in
`tui::commands::interactive_terminal::tests` — including
`a_refused_clear_reports_itself_and_leaves_the_screen_intact`,
`reporting_the_mode_never_moves_the_keyboard`,
`the_badge_returns_to_normal_after_esc_even_though_the_composer_forgot`, and
`a_hidden_terminal_surface_does_not_arm_the_loop`. Each passed in the next run.

`git diff --stat HEAD` is empty for `interactive_terminal.rs` and its sibling,
and the panel references none of the editor helpers this milestone touched — it
has its own column-based `next_word_end`. **This is the same flake family
already recorded in the M005 closure §8.1**; a fourth occurrence is not new
information, but it is recorded here so a later reader does not attribute it to
M004 either.

## 13. Files changed

| File | Change |
|---|---|
| `src/tui/editor.rs` | `EditorPrefix::ReplaceChar` + `is_capture`/`is_replace_char`; `char_end`, `selection_or_char_at_cursor`, `paste_anchor`; `paste_after`/`paste_before`, `replace_char`/`replace_with_text`, `switch_case`/`switch_to_lower`/`switch_to_upper`, `case_range` + `CaseMode`; `resolve_edit` doc on its scope boundary; tests |
| `src/tui/commands/editor.rs` | `editor_editing_verb`, `ClipboardVerb`, `run_clipboard_verb`/`apply_clipboard_text`; `BufferKey::PasteAfter`/`PasteBefore`/`ReplaceWithClipboard`; `RequestReload` and its guard-block chord removed; `find_target_char` → `capture_char`; one capture branch in `resolve_pending_prefix`; `y`/`p`/`P`/`R`/`~`/`` ` ``/`` Alt-` ``/`r` arms; tests |
| `scripts/check_tui_editor_key_authority.py` | `RequestReload` dropped from `REQUIRED_SUBSTRINGS`; stage-1 documentation updated |
| `scripts/check_tui_editor_prefix_state.py` | `ReplaceChar` and `fn is_capture(` added to the required declarations; model documentation extended |
| `docs/tui.md` | "Change verbs" section; `R` and reload updated |
| `src/tui/components/help_overlay.rs` | **inspected and deliberately unchanged** — unreferenced dead code (§9) |
| `architecture/tui.md` | change-verb sections; capture-prefix section renamed and extended; key-dispatch stage 1 corrected |
| `plans/implementation/tui-editor-helix-parity/004-core-change-verbs.md` | §17 amendment A1 |
| `plans/subsystems/tui-editor-helix-parity-roadmap.md` | M004 closed |
| `plans/registry.md` | roadmap row, M004 row, execution order |

## 14. Unblock audit

M004's hard dependency was M003, and it is discharged — M003 closed in an
earlier session and the M003 closure records it.

M004 discharges **no** other edge: no milestone waited on it. M005, M013 and
M010 are unaffected by anything here; M013's LSP read-delivery defect and
M010's dependency on it are unchanged.

What M004 changes for other milestones is small and worth stating: `SPACE_LEAVES`
now has a natural pair of rows waiting (`p`/`P`), and `GOTO_LEAVES`/`r` set the
precedent that a capture grammar resolves *before* the leaf lookup. Any future
prefix that takes data must go through `EditorPrefix::is_capture`, or it will
swallow keystrokes.