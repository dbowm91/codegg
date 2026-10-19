# TUI Editor Helix Parity Milestone 008 — Closure Status

Status: closed
Milestone: M008 — Multi-selection command surface
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md`
Implementation plan: `plans/implementation/tui-editor-helix-parity/008-multi-selection-command-surface.md`
Closed: 2026-10-19
Baseline commit: `84440fe7f`
Predecessor: [M007](007-status.md), which shipped the selection set itself.

---

## 1. Executive finding

M008 is **closed**. All eighteen verbs are bound, all eleven work packages
shipped, and the guard counts moved from 48 to the 66 Normal-mode chords the
plan pinned.

The milestone's central claim — that eighteen new Normal-mode chords is the
first thing in this roadmap able to break terminal-panel parity — was tested
rather than assumed, and **one chord diverges**: `C`. The terminal panel claims
`C`/`c` for `TerminalNavAction::Clear`; the editor uses it to add a cursor on
the next line. That is recorded as an exemption, per the plan's §12, because
the two surfaces have different subjects and neither choice was available to
both. The other seventeen are unclaimed by the panel and are swallowed by it,
which is the safe failure.

Two real defects surfaced while the milestone's own tests were being written,
both caught before the code was called done:

1. **`K` with no matches refused instead of emptying the set.** Plan §13.11
   requires "the set is empty, and the command reports it". The implementation
   returned a refusal and left the reader holding exactly the selections they
   had asked to discard.
2. **`C` at the end of a buffer reported success while adding nothing.**
   `copy_selection_on_line` bounded the walk by `len_lines()`, which counts a
   trailing line that exists only because the buffer ends in a newline — and
   which **no byte offset resolves into**. `C` therefore resolved "the next
   line" onto the cursor's own byte, added a duplicate member, and returned
   `Handled`. The bound is now the last line a *cursor* can reach, plus an
   explicit check that a target is not the member's own byte.

A third item was a documentation error rather than a behaviour one: this
milestone's `select_all` doc comment claimed `%` stopped short of a trailing
terminator. `buffer_content_end` covers the whole buffer, and that is correct —
`%` then `d` should empty the file. The comment was corrected and a test now
pins the behaviour.

The regex verbs use the **existing `:` command line**, pre-filled. No new input
mode, no new prefix field, nothing new to disarm on reload, resync or buffer
switch. `scripts/check_tui_editor_prefix_state.py` still reports exactly the one
prefix field it reported before this milestone, which is the plan's §6 claim
verified rather than argued.

`verify.sh quick` is green. `verify.sh full` did **not** go green, for the same
environmental reasons M007 recorded; that section distinguishes what was proven
from what was inferred.

---

## 2. Requirement-to-evidence matrix

| §13 | Requirement | Evidence | Verdict |
|---|---|---|---|
| 13.1 | `C`/`Alt-C` on a two-member set make four members | `copy_onto_the_next_line_adds_a_member_rather_than_moving_one`, `capital_c_adds_a_cursor_below_and_alt_c_above` (dispatch) | pass |
| 13.2 | `d` over four members is one transaction | inherited from M007; `a_multi_selection_edit_is_undone_and_redone_as_one_step` | pass |
| 13.3 | `%` selects the whole file, including empty and unterminated | `select_all_covers_the_whole_buffer_terminator_included`, `select_all_on_an_unterminated_document_covers_the_last_character`, `select_all_on_an_empty_document_is_held_but_resolves_to_nothing` | pass |
| 13.4 | `S` on three matches gives three members | `split_makes_one_member_per_match` | pass |
| 13.5 | `;`/`Alt-;`/`Alt-:`/`(`/`)`,% ,`,`Alt-,` all observable on the set | `semicolon_collapses…`, `alt_semicolon_flips…`, `alt_colon_makes_a_backwards_member_forward…`, `rotate_moves_focus…`, `comma_keeps_the_primary_and_alt_comma_removes_it` (all dispatch) | pass |
| 13.6 | `Alt-;` reverses each member's direction without moving it | `flip_swaps_every_direction_without_moving_a_single_member` | pass |
| 13.7 | `(Alt-)` moves bytes and leaves members alone | `rotate_moves_focus_and_alt_rotate_moves_bytes`, `rotating_contents_is_one_transaction_over_the_original_coordinates` | pass |
| 13.8 | `Alt-minus` bridges gaps; `Alt-_` does not | `merge_all_bridges_the_gap_and_merge_consecutive_does_not`, `merge_consecutive_leaves_a_real_gap_alone`, dispatch twin | pass |
| 13.9 | `&` pads to the widest member in **display** columns | `align_measures_display_columns_not_bytes`, `align_pads_every_member_to_the_widest_one` | pass |
| 13.10 | Over-cap / invalid regex refuses visibly | `copy_onto_the_next_line_refuses_past_the_selection_ceiling`, `a_pattern_that_will_not_compile_is_reported_and_the_buffer_is_untouched`, `the_regex_window_is_bounded_per_member` | pass |
| 13.11 | A regex that matches nothing is reported; `S` unchanged, `K` empty | `a_regex_that_matches_nothing_reports_and_leaves_the_buffer_alone` (dispatch), `keeping_a_set_where_nothing_matches_empties_it_and_says_so`, `split_whose_pattern_matches_nothing_leaves_the_set_alone` | pass |
| 13.12 | **All eighteen are claimed** in Normal mode | `all_eighteen_chords_are_claimed_in_normal_mode` drives 22 key/modifier spellings; guard reports 66 Normal-mode chords | pass |
| 13.13 | Read-only buffer refuses to grow | `the_mutating_set_verbs_are_refused_on_a_read_only_document`; M004's `a_read_only_buffer_refuses_selection_growth_but_still_moves` unchanged | pass |
| 13.14 | Insert mode inserts every glyph literally | `none_of_the_eighteen_new_chords_are_captured_in_insert_mode` (12 glyphs × NONE and SHIFT), `the_eight_alt_chords_are_inert_in_insert_mode` | pass |
| 13.15 | `Alt-.` learns `C`, `Alt-C`, `%` | `capital_c_is_recorded_as_a_repeatable_motion_for_alt_dot`, `a_plain_motion_disarms_the_selection_repeat_slot` | pass |
| 13.16 | No new prefix field or input mode | `scripts/check_tui_editor_prefix_state.py` — "one pending-prefix field (pending_prefix), typed as EditorPrefix", identical to the pre-milestone output | pass |
| 13.17 | `check_tui_editor_key_authority.py` passes | exit 0 — 2 guard-block, 66 Normal-mode, 1 Insert-mode | pass |
| 13.18 | One undo restores text **and** the set | `ampersand_aligns_in_display_columns_and_is_undoable_as_one_step`; M007's restore path unchanged | pass |
| 13.19 | Docs updated | `docs/tui.md` §"Selecting more than one region" and §"The regex keys"; `architecture/tui.md` §"The selection-manipulation verbs (M008)" | pass |

---

## 3. Production implementation evidence

### The set algebra (`src/tui/helix_selection.rs`)

Twelve methods, all pure and all answering `bool`:

`selection_point_of`, `set_members`, `collapse_to_primary`, `flip_all`,
`ensure_forward`, `keep_primary`, `remove_primary`, `rotate_primary`,
`merge_all`, `merge_consecutive`, plus the `span_of_set` / `widest_mode`
privates they share.

The split is by **what a verb needs to know**, not by convenience. These eight
operate on points alone, which is exactly why they work unchanged on the
terminal panel's `(row, col)` type. `merge_consecutive` is the interesting one:
it is a set rule that needs a geometry answer, so it takes an `adjacent`
closure and stays pure while the surface supplies the bytes.

Three findings the tests forced, each now documented at the method:

- **`rotate_primary` must move the cursor.** The invariant that `cursor_byte`
  *is* the primary's head is what the viewport, LSP reads and the undo entry
  all address. Rotating focus without moving the cursor would leave the two
  silently disagreeing about which member is focused.
- **`flip_all` and `ensure_forward` never re-sort.** A member's bounds are
  `min`/`max` of its two points, so reversing it cannot move it in document
  order. A `normalize()` call there would be a sort that provably cannot
  change anything.
- **`remove_primary` of a one-member set clears the set.** That is a result,
  not a failure, and `clear()` is the honest answer rather than a panic.

### The geometry and content (`src/tui/editor.rs`)

`select_all`, `copy_selection_on_line`, `rotate_selection_contents`,
`align_selections`, `split_selection`, `filter_selections`,
`compile_selection_regex`, `bounded_regex_window`, `last_char_start`,
`member_ranges`, `set_edit_outcome`, `members_are_adjacent`, `write_back`.

Twelve new `HelixVerb` variants; `HelixVerbOutcome` gains `Edit(EditorEdit,
Option<String>)` and `NothingToDo(String)`; `RangeSetEdit::Edit` gains the
note field. `HelixVerb` **lost `Copy`** — three variants carry a `String`, and a
verb that owns text is not a unit value. Every call site constructed one and
passed it through, so the boundary moved rather than being papered over with a
`Copy` impl that held only for the other fifteen variants.

The note field on `RangeSetEdit::Edit` exists for one reason: `&` builds it only
when a member was too wide to measure exactly, and that fact has to reach the
reader. Carrying it beside the edit is what makes "leave a disclosure behind"
impossible to forget.

### The command layer (`src/tui/commands/editor.rs`)

Eighteen chords in one contiguous block, `apply_helix_edit` as the single funnel
from a verb's transaction to `submit_edit`, `run_selection_regex`, and
`note_selection_motion` / `selection_repeat` for the `Alt-.` slot.

`EditorNotice::Note(String)` is new and is the reason a verb can report its own
outcome. These are **answers, not errors**: "nothing is selected" and "the
pattern matched nothing" describe a healthy buffer, and rendering them with
error wording would tell the reader their file is broken.

The read-only gate widened from `grows_selection` to
`grows_selection || mutates_buffer`. Pure set verbs — `;`, `,`, `(`/`)`,
`Alt-;`, `Alt-:`, `C`, `%` — still work on a read-only buffer, because they
change no byte. `&` and `Alt-(`/`Alt-)` are refused, because they do.

### The prompt surface (deliberately none)

`S`, `K` and `Alt-K` call `open_command_line("split-regex ")`. Three
`EditorCommandName` variants (`SplitRegex`, `KeepRegex`, `RemoveRegex`) carry
the argument through `parse_editor_command`, which already models
argument-taking commands.

`run_selection_regex` was extracted from `run_editor_command` so the whole `S`
path — chord, modal command line, parse, verb — is drivable from a test
without an `App`. The `App` layer remains the caller; only the effect moved.

---

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test -p codegg --lib tui::` | **2128 passed, 1 failed** — see below |
| `cargo test -p codegg --lib -- the_badge_names_insert_while_the_terminal_box_takes_keystrokes` | 1 passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `scripts/check_tui_editor_key_authority.py` | exit 0 — 2 guard-block, 66 Normal-mode, 1 Insert-mode |
| `scripts/check_tui_editor_prefix_state.py` | exit 0 — one prefix field, unchanged |
| `scripts/check_tui_editor_text_authority.py` | exit 0 |
| TUI integration binaries (nextest, 9 binaries) | **403 run, 403 passed, 0 skipped** (29.5 s) |
| `scripts/verify.sh quick` | **"Quick verification passed."** |

New tests: **69** (33 in `helix_selection.rs`, 31 in `editor.rs`, 15 in
`commands/editor.rs` — see the count method in §11).

### The one `tui::` failure is a known load-sensitive class, proven so

`tui::commands::interactive_terminal::tests::the_badge_names_insert_while_the_terminal_box_takes_keystrokes`
failed in the full-suite run and **passed in isolation** (0.32 s). It lives in
`commands/interactive_terminal.rs`, which this milestone does not touch — the
diff for that file is empty. M007 recorded two full-suite runs of the same
command disagreeing on this same test while another worktree held 170 %+ CPU.
It is load sensitivity, not a regression, and it is recorded as such rather
than waved away.

### Guard self-demonstration, run twice

`check_tui_editor_key_authority.py` had to be taught a new shape, and a guard
that stops parsing stops checking. Its `ALT_RE` matched a `KeyCode::Char(...)`
payload with `[^()]*`, which cannot spell `Char('(')` — a glyph M008 claims.
The fix matches the glyph as a **quoted literal** instead.

Two injections, each reverted, each byte-identical afterwards:

1. **A shadow on a paren glyph** — `(m, KeyCode::Char('(')) if m == shift` after
   the real `(` arm. Exit 1, naming `SHIFT+'('` as re-declared and unreachable.
   This is the exact shape the old regex could not parse.
2. **A shadow on an ordinary glyph** — `(m, KeyCode::Char('&')) if m == none`
   after the real `&` arm. Exit 1, naming `NONE+'&'`. This proves the regex
   change did not weaken the general case.

Both restored, guard back to exit 0 at 66 chords.

### `verify.sh full` did not go green, and that is not an M008 failure

Same wall M007 recorded. Guard, fmt, clippy and check stages pass; the
workspace-test stage fails outside `tui::`, in `lsp`, `tool_execution`,
`workspace_isolation`, `session_family`, `tool_surface_minimization` and
`core::daemon::tests::session_create_through_daemon`. M007 **proved** the
daemon failure pre-existing by re-running it at baseline `84440fe7f` with the
changed sources stashed. The others were recorded then as environment-dependent
by inspection, not by proof, and that distinction is preserved here rather than
being upgraded to a proof it never had. Nothing in this milestone's diff touches
any of those modules. Finding 3, carried forward.

---

## 5. Invariant review

| Invariant | Holds? | Why |
|---|---|---|
| The set is sorted by bounds with one valid primary | yes | `normalize_set` is the only writer; `set_members` and every mutator route through it |
| The primary travels **with its member**, not by index | yes | `copy_selection_on_line`'s members stay in order so the primary index still names the primary member; `normalize_set` tracks it through the sort. Pinned by `copy_onto_the_next_line_keeps_the_set_sorted_and_the_primary_intact` |
| `cursor_byte` is the primary's head | yes | `rotate_primary`, `select_all` and `merge_all` each write it explicitly rather than assuming |
| A refused verb changes no buffer and pushes no undo entry | yes | every set verb returns `bool`/`SetVerbOutcome`; `apply_helix_edit` only submits on `Edit` |
| A mutating verb is one transaction | yes | `rotate_selection_contents` and `align_selections` each build one `TextTransaction` |
| Nothing exceeds `MAX_DOCUMENT_EDITS` | yes | `C` refuses above it (not truncates); `rotate` refuses; `S` refuses; `&` refuses |
| Read-only refuses growth and mutation, not navigation | yes | gate widened to `mutates_buffer`, not to "any verb that touches the set" |
| No new prefix field or input mode | yes | the prefix guard's output is byte-identical to the pre-milestone output |

---

## 6. Failure and recovery review

Every set verb has an explicit refusal path, and `apply_helix` reports it once
so no caller can forget:

- nothing selected → "nothing selected"
- `C` with no line below / above → names the direction
- `C` past the selection ceiling → names the count and the limit
- invalid regex → echoes the pattern and the parser's complaint
- `K`/`Alt-K` where the answer is the existing set → "already matched every selection"
- overlapping set for rotate or align → tells the reader to `Alt-minus` first
- a member over the measure budget → `&` discloses that its padding is approximate
- read-only → the existing M004 notice

`apply_helix_edit` sets the post-edit notice **after** `submit_edit`, because
`submit_edit` clears the notice on success and a disclosure set beforehand would
be erased by the very edit it describes.

---

## 7. Migration and compatibility review

No storage layout change, no protocol change, no configuration key, no data
migration. `TuiDocumentPresentation` gains one field,
`last_selection_motion`, which is transient runtime state and is `None` after
load. `EditorNotice` gains one variant; both its `severity()` and `message()`
handle it, so every existing matcher is exhaustive.

`HelixVerb`'s loss of `Copy` is a source-level break for any downstream crate
that copied a verb. No such caller exists in this workspace — the change is
workspace-local.

---

## 8. Security review

**Bounded work per keystroke**, which is the whole of it:

- `&` inherits `MAX_RENDER_LINE_BYTES` (32 KiB) per member through
  `bounded_display_width`, and the truncation is disclosed.
- Regex verbs cap each member's window at the same line budget and the whole
  set at `MAX_SET_VERB_READ_BYTES` (256 KiB, the frame budget).
- `regex` is refused at **compile** time via `size_limit`, because the expensive
  thing about a pathological pattern is running it and a pattern that will not
  compile cannot run.
- `rotate_selection_contents` refuses above its read budget rather than
  allocating the document.
- `member_ranges` refuses an overlapping set, so no verb can write a member's
  text into bytes another member owns.

No secrets are read or logged. No filesystem access. No network. The regex verbs
take a reader-supplied pattern and that is the only new input surface; it is
bounded at compile time and executed against a bounded window.

---

## 9. Documentation and operations

`docs/tui.md` — new §"Selecting more than one region" with the eighteen-key
table and §"The regex keys". The §"Change verbs" preamble no longer promises a
set that cannot be built, and the read-only paragraph now states the widened
gate honestly rather than only naming `v` and `x`.

`architecture/tui.md` — new §"The selection-manipulation verbs (M008)" with the
three-group split table, the rationale for the split, the four findings above,
the regex-boundedness argument, and the **recorded parity exemption** on `C`.

`scripts/check_tui_editor_prefix_state.py` — prose only; its assertion is
unchanged and its output is identical before and after this milestone.

No new user-facing configuration and no operational runbook change.

---

## 10. Unresolved findings

**Finding 1 — `C` diverges between the panel and the editor. Severity: low.
Accepted.** Recorded, not fixed. The panel's `C` clears scrollback; the
editor's adds a cursor. They act on different objects, and the panel has no
document to place a cursor in. The panel also claims lowercase `c`, which the
editor leaves to the composer.

**Finding 2 — the guard's `ALT_RE` could not spell `Char('(')`. Severity: medium.
Fixed.** A guard that stops parsing stops checking, and it stopped on exactly
the glyphs M008 claims. Fixed and proven non-vacuous twice (§4). Any future
chord whose glyph contains a bracket needs the same attention.

**Finding 3 — `verify.sh full` is not green in this worktree. Severity: medium.
Open; inherited.** Every failure is outside `tui::`. The daemon failure is
*proven* pre-existing (M007, at baseline, sources stashed); the `lsp`,
`tool_execution`, `workspace_isolation`, `session_family` and
`tool_surface_minimization` failures are recorded as environment-dependent by
inspection, not by proof, and are not upgraded here. M009 inherits this wall.

**Finding 4 — one `tui::` test is load-sensitive. Severity: low. Not a
defect.** Proven: failed in-suite, passed in isolation, in a module this
milestone does not modify.

**Finding 5 — the five milestones share one commit. Severity: informational.
Resolved by `278737d4e`.** When this record was written nothing was committed and
the working tree also carried uncommitted M004, M005, M006 and M007 work, so this
closure rested on a tree that was not a single milestone. M004–M008 were
subsequently committed together as `278737d4e` on the user's request. The
consequence stands: this closure describes one commit containing five milestones,
not a commit that contains only M008. Nothing here was re-verified against the
commit; the tree it contains is the tree the evidence below was gathered from.

**Finding 6 — `C` can add a cursor onto an existing one. Severity: low.
Accepted.** Pressing `C` twice can resolve a target onto a line that already
has a cursor. Identical members are kept rather than folded — they are
indistinguishable, and `selection_ranges` merges them into one span for every
effect. Documented at `copy_selection_on_line` and pinned by
`capital_c_is_recorded_as_a_repeatable_motion_for_alt_dot`.

---

## 11. Roadmap disposition

M008 is closed. M009 becomes **ready** — it was blocked on M008 **and** on
accepted ADR-0034, which is still `proposed`, so it stays blocked on the ADR
alone. That blocker is unchanged and is not discharged here.

The milestone's §5 set API contract shipped as pinned, with one addition beyond
the plan: `RangeSetEdit::Edit` carries an optional note. M009's input was the
set API, not the outcome types, so the contract is intact.

M010 remains blocked on M013. M011's `Ctrl+s`/`Ctrl+r` decision is unchanged.

Unimpaired change navigation (`]g`/`[g`/`]G`/`[G`) remains deferred,
unregistered product work with no assigned milestone — unchanged by M008.

New test counts were taken by exact-name match against the 69 names introduced
here, not by counting added functions, which would have swept in M004–M007.

---

## 12. Registry updates

- Roadmap status table: M008 `ready` → `closed`, closure cell filled.
- M009 row annotated with the surviving ADR-0034 blocker.
- Dated `Amendment A3` on the M008 entry recording: the `C` parity exemption,
  the `RangeSetEdit::Edit` note field, and `HelixVerb` losing `Copy`.
- Plan header: `ready for handoff — unblocked 2026-10-18` →
  `implemented — closed 2026-10-19`.
- `plans/registry.md`: M008 row → `closed`, closure link added, M009 row
  carries the surviving ADR blocker.