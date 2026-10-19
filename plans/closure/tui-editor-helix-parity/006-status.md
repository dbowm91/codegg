# TUI Editor Helix Parity Milestone 006 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/006-unimpaired-navigation-non-ts-subset.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M006, Amendment A2)
Repository baseline reviewed: `84440fe7f` (`feat/auto-20261018-12255c45`)
Implementation commits: **none.** The change is uncommitted in the working tree of
`feat/auto-20261018-12255c45`, alongside the M004–M008 work that was already
uncommitted there. No commit was made because none was requested. The evidence
below is therefore "working tree at baseline + this change", not "commit X".

---

## 1. Executive finding

M006 is **closed**. `[` and `]`, armed but inert since M002, now have a real
grammar: `]p`/`[p` jump between paragraphs, `]Space`/`[Space` add a line, and
`Alt-.` repeats the paragraph motions. `scripts/check_tui_editor_prefix_state.py`
reports the same single pending-prefix field it reported before this milestone.

Six things are worth recording beyond the checklist, because each is a decision a
later milestone would otherwise have to re-derive:

1. **`PrefixAction` needed a third arm, `Edit`.** `]Space` inserts a newline, and
   `editor_motion` is the one function in the dispatch whose contract is that it
   never touches text. Folding a mutation into the `Motion` arm would have been
   a three-line change and a broken promise. The leaf now says what kind of thing
   it is, and `]Space` routes through `editor_editing_command` — the same
   `editable` gate, the same undo entry and the same `submit_edit` transaction as
   `x`, `d` and `y`. This was the single largest design decision in the milestone
   and it was in the plan; the plan's reasoning held.

2. **Helix's `add_newline_*` moves no selection, and that is the interesting
   part.** Read from source (`helix-term/src/commands.rs`,
   `add_newline_impl`) rather than assumed: it inserts a line ending at
   `line_to_char(end + 1)` and attaches no `with_selection` to the transaction.
   So `]Space` **leaves the cursor where it was**. That is what makes `]Space`
   then `]p` a paragraph split. It is also *not* `open_below` — `o` inserts the
   same line and additionally hands over the keyboard — so
   `AddNewlineBelow`/`AddNewlineAbove` are separate variants from
   `OpenLineBelow`/`OpenLineAbove`, and the two pairs share one implementation
   because `stay_insert` is the only thing separating them.

3. **A paragraph scan has *three* answers, and the plan only named two.** The
   plan described the disclosure but not a type for it. `Option<usize>` cannot
   carry "the budget ran out", so `ParagraphOutcome` has three variants:
   `Landed`, `NoParagraph`, `Bounded`. Collapsing the third into `None` would
   report "there is no next paragraph" about a document that has one — the one
   answer a reader has no way to distrust, because it is phrased as a fact.

4. **A pre-existing `Alt-.` defect was found and fixed.** `note_motion` returned
   early for a find without clearing the *other* repeat slot. With two slots and
   `Alt-.` checking the selection one first, that was invisible; the moment a
   third existed it became wrong: `]p`, `f x`, `Alt-.` repeated the **older**
   `]p` backwards instead of `f x` forwards. Every recorder now closes the other
   two doors, which is the single-slot rule `note_motion`'s own comment already
   claimed. M008 shipped this latent bug; M006's tests found it.

5. **A second pre-existing defect was found and deliberately *not* fixed.** `o`
   and `O` do not insert a line: `enter_insert` matches `OpenLineBelow`/
   `OpenLineAbove` against `_ => cursor`, so `resolve_edit`'s open-line arms are
   reachable only from their own unit test. `o` enters Insert mode on the
   **current** line rather than opening one below it. This is not M006's to
   change — it is a pre-existing milestone's behaviour — so it is recorded as a
   finding with a named owner (§10) rather than quietly fixed or quietly
   omitted. `o_and_close_space_stay_different_verbs` asserts only what is true,
   and says so in a comment.

6. **One deliberate divergence from Helix, on the degenerate case.** With no
   blank line anywhere, Helix's `move_next_paragraph` skips every remaining line
   and lands on the end of the buffer —
   `test_behaviour_when_moving_to_next_paragraph_single`,
   `helix-core/src/movement.rs`, verified against the live keymap. CodeGG reports
   "no next paragraph" instead. An end-of-buffer jump looks like the key did
   something and hides that it found nothing, which is the exact failure the
   note exists to prevent. Recorded in `docs/tui.md` and Amendment A2.3 rather
   than matched.

---

## 2. Requirement-to-evidence matrix

| Plan §13 criterion | Evidence | Verdict |
|---|---|---|
| 1. `]p`/`[p` land on the first non-whitespace byte, blank runs are one separator, on char boundaries | `the_paragraph_scanners_resolve_the_documented_offsets` (literal offsets 9/15/0), `unimpaired_close_p_lands_on_each_next_paragraph`, `unimpaired_open_p_lands_on_each_previous_paragraph`, `consecutive_blank_lines_are_one_separator`, `a_whitespace_only_line_is_blank`, `every_paragraph_shape_either_lands_or_reports` (8 document shapes × every offset × both directions) | met |
| 2. At the last (first) paragraph the cursor does not move and a notice is shown | `unimpaired_close_p_lands_on_each_next_paragraph`, `unimpaired_open_p_lands_on_each_previous_paragraph`, `a_document_with_no_blank_lines_reports_no_next_paragraph` | met |
| 3. `]Space`/`[Space` insert through the normal funnel; refused with a notice read-only | `unimpaired_close_space_adds_a_line_below_and_leaves_the_cursor_alone`, `unimpaired_open_space_adds_a_line_above_and_keeps_the_cursor_on_its_byte`, `a_newline_below_the_last_line_appends_rather_than_refusing`, `a_newline_above_the_first_line_prepends`, `the_newline_keys_are_refused_on_a_read_only_buffer` | met |
| 4. `]` then `Space` adds a newline and does not arm space mode; `Space` alone still arms | `a_bracket_then_space_is_a_newline_and_not_a_second_armed_prefix`, `space_alone_still_arms_space_mode` | met |
| 5. An unbound second key still disarms and resolves normally, proven by the M002 test unmodified | `the_unimpaired_prefixes_disarm_without_swallowing_the_next_key` passed **unmodified**; `an_unbound_bracket_leaf_still_reaches_the_composer` | met |
| 6. `Alt-.` repeats the last unimpaired motion and does not repeat an edit | `alt_dot_repeats_the_paragraph_motion_and_an_edit_is_not_one`, `the_repeat_slot_is_one_slot_every_motion_overwrites`, `a_reconciliation_forgets_the_unimpaired_repeat`, `a_find_and_a_paragraph_motion_do_not_both_hold_the_repeat_slot` | met |
| 7. `[`/`]` remain literal text in Insert, proven by a sweep | `printable_characters_reach_the_document_in_insert_mode` (punctuation set, 91 → 93) and `the_bracket_keys_stay_document_text_in_insert_mode` (both NONE and SHIFT encodings) | met |
| 8. No paragraph scan reads an entire line; the bound test fails without it | `a_four_mib_indented_line_is_content_and_the_walk_stays_bounded` (4 MiB), `an_exhausted_budget_is_a_third_answer_not_a_refusal`, `an_unprovably_blank_line_counts_as_content` | met |
| 9. `]d`/`[d`/`]D`/`[D` and `]g`/`[g`/`]G`/`[G` are not bound, asserted | `the_deferred_diagnostic_and_change_quartets_are_asserted_unbound` (leaf-table absence for all eight, plus dispatch: `d` resolves as the delete operator on an empty range, `g` resolves as goto mode) | met |
| 10. All three editor guards and `verify.sh quick` pass | §4 | met |

Two existing tests changed, each per its own documented contract — both are
called out here because a closure that hides a modified test is hiding evidence:

- `each_prefix_declares_its_leaf_and_its_unmatched_key_policy` (M002) asserted
  `OpenBracket`/`CloseBracket` return `vec![]`. That row was a placeholder for
  exactly this milestone. It now reads `vec![' ', 'p']`; **every other asserted
  fact** — the unmatched-key policy, the indicator, the find kind, `is_find`,
  `is_pending` — is unchanged, and the comment records that claiming a key is
  supposed to require editing that row.
- `printable_characters_reach_the_document_in_insert_mode` carries its own
  non-vacuity count (`26 × 3 + 13 = 91`). Adding `[` and `]` to the punctuation
  set makes it 93. The count is the test working as designed.

---

## 3. Production implementation evidence

| Change | File | Anchor |
|---|---|---|
| `PrefixAction::Edit(EditorCommand)` | `src/tui/editor.rs` | `PrefixAction` |
| `OPEN_BRACKET_LEAVES`, `CLOSE_BRACKET_LEAVES` | `src/tui/editor.rs` | beside `SPACE_LEAVES` |
| `EditorPrefix::leaves()` rows for `[`/`]` | `src/tui/editor.rs` | `leaves` |
| `EditorCommand::{NextParagraph, PrevParagraph, AddNewlineBelow, AddNewlineAbove}` | `src/tui/editor.rs` | `EditorCommand` |
| `ParagraphMotion`, `ParagraphOutcome`, `ParagraphNote` | `src/tui/editor.rs` | beside `FindMotion` |
| `line_blankness` / `line_is_blank` | `src/tui/editor.rs` | beside `first_non_whitespace` |
| `next_paragraph_start`, `prev_paragraph_start`, `MAX_PARAGRAPH_SCAN_BYTES` | `src/tui/editor.rs` | after `line_is_blank` |
| `apply_motion` intercepts paragraph motions; `write_back` carries `paragraph_note` off the probe | `src/tui/editor.rs` | `apply_motion`, `write_back` |
| `resolve_edit` → `open_line_below` / `open_line_above` / `add_newline_below` / `add_newline_above` | `src/tui/editor.rs` | `resolve_edit` |
| `TuiDocumentPresentation::{last_unimpaired, paragraph_note}` | `src/tui/document_session.rs` | beside `last_selection_motion` / `find_note` |
| `PrefixAction::Edit` routing | `src/tui/commands/editor.rs` | `resolve_pending_prefix` |
| `lift_paragraph_note`, shared by both motion entry points | `src/tui/commands/editor.rs` | beside `editor_motion` |
| `note_motion` — three doors, all closing | `src/tui/commands/editor.rs` | `note_motion` |
| `unimpaired_repeat`, third `Alt-.` slot | `src/tui/commands/editor.rs` | `Alt-.` arm |
| `clear_history` clears the new fields | `src/tui/editor.rs` | `clear_history` |

`src/tui/interactive_terminal.rs` and `src/tui/commands/interactive_terminal.rs`
are **unmodified**. That is the evidence for the terminal-panel exemption in §8,
not an assertion of it.

---

## 4. Verification executed

All local; no CI run was made and none is claimed.

```bash
# 1. Formatting
cargo fmt --check
#   -> clean

# 2. Editor guard scripts
python3 scripts/check_tui_editor_key_authority.py
#   -> passed — 2 guard-block chord(s), 66 Normal-mode chord(s),
#      1 Insert-mode chord(s), no shadowed arm
python3 scripts/check_tui_editor_prefix_state.py
#   -> passed — one pending-prefix field (pending_prefix), typed as EditorPrefix
python3 scripts/check_tui_editor_text_authority.py
#   -> passed — no retained text buffer and no direct filesystem reads

# 3. Unit suite
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib tui:: --no-fail-fast
#   -> 2168 passed; 0 failed

# 4. Clippy
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
#   -> clean

# 5. TUI integration binaries
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci --no-fail-fast \
  -E 'binary(tui) or binary(tui_render) or binary(tui_main_buffer) or
      binary(tui_presentation_regressions) or binary(tui_usability_fixes) or
      binary(interactive_terminal_tui) or binary(document_client_trajectory) or
      binary(theme_selection_tokens) or binary(modifier_chords)'
#   -> see §4.1

# 6. Canonical gate
scripts/verify.sh quick
#   -> see §4.1
```

### 4.1 Integration binaries and `verify.sh quick`

**New tests: 39** — 27 in the `src/tui/commands/editor.rs` dispatch block and 12
in the `src/tui/editor.rs` model block. Counted by matching **exact test names**
in the two M006 blocks, not by counting added `fn`s (which would sweep in
M004–M008's tests too). The count is corroborated by arithmetic: the `tui::`
total went 2129 → 2168, a difference of exactly 39.

```
Summary [ 28.682s] 403 tests run: 403 passed, 0 skipped
```

and

```
==> Quick verification passed.
```

Both green, both local.

**`verify.sh full` was attempted and did not complete.** Its own transcript is
the honest record:

```
==> scripts/verify.sh quick          (internally)  -> Quick verification passed.
==> cargo clippy --workspace --all-targets --locked -- -D warnings -> Finished
==> CARGO_BUILD_JOBS=2 cargo nextest run --workspace --locked --profile ci
   Finished `test` profile ... in 1m 16s
error: creating test list failed

Caused by:
  for `codegg::collaboration_m001_chat`, command
  `.../target/debug/deps/collaboration_m001_chat-<hash> --list --format terse`
  aborted with signal 9 (SIGKILL)
```

Three things follow, and they are deliberately kept apart:

1. `verify.sh full` **cleared its own quick gate and its clippy stage.**
2. It then died in nextest's **test-enumeration** step. **No workspace test
   ran**, so this closure quotes no workspace-wide test result and claims
   none. Signal 9 is an external kill — resource pressure or the OOM killer —
   not a Rust panic and not an assertion failure.
3. This is **not** the same wall M007 and M008 recorded, and it must not be
   reported as one. Those two got a test list and then specific failing tests
   outside `tui::`. This run never produced a list at all, so it is **not
   evidence about any module's tests** — in either direction. It is also
   **not** evidence that those failures went away.

The box carried 11 concurrent cargo/rustc processes from other sessions while
this ran, so contention is the available explanation for the kill. It is an
explanation, not a proof, and is recorded as such rather than promoted to a
verified cause.

### 4.2 Guard non-vacuity

Both guards were proven to fail on an injected violation, because a guard that
has never been seen red is indistinguishable from a guard that cannot go red.

**`check_tui_editor_prefix_state.py`** — the injection §15 names. Renaming
`pub fn is_capture(self) -> bool` to `pub fn is_capture_M006INJECTED`:

```
TUI editor prefix-state violations found.
...
  expected declaration `fn is_capture(` is absent from src/
1 violation(s) found.
exit=1
```

**`check_tui_editor_key_authority.py`** — a shadow on the `]` arm itself, which
is the arm M006's whole grammar depends on:

```
TUI editor key-authority violations found.
...
  editor.rs:3444 [handle_normal_key] (m, KeyCode::Char(']')) if m == none
  re-declares NONE+']', already claimed at editor.rs:3438 ... — unreachable
1 violation(s) found.
exit=1
```

Both injections were reverted and the files confirmed **byte-identical**
(`cmp -s`) before re-running; both guards returned to `exit=0` at 66 chords and
one prefix field.

### 4.3 The chord count did not move, and that is correct

The guard reports **66** Normal-mode chords — the same number M008 left. M006
adds no bare chords: it fills in two leaf tables that M002 had left empty. That
is the M005 shape (a grammar is a leaf table, not a chord) and it is the reason
this milestone added **no** `TuiDocumentPresentation` field whose name contains
`prefix`, so the prefix-state guard's output is unchanged rather than relaxed.

---

## 5. Invariant review

**"A leaf lookup must never swallow an unbound key."** Preserved by
construction, and proven twice: the M002 test
`the_unimpaired_prefixes_disarm_without_swallowing_the_next_key` passes
**unmodified**, and `an_unbound_bracket_leaf_still_reaches_the_composer`
re-asserts it for both armings after the tables were populated.

**"`resolve_pending_prefix` runs before every bare-key arm."** Not changed and
not needed to change. `handle_normal_key` resolves the pending prefix first, so
`]` then `Space` finds the `' '` leaf before `Space` can arm. That is why
`a_bracket_then_space_is_a_newline_and_not_a_second_armed_prefix` passes without
a special case — the precedence is structural, and §14's "leaf lookup would need
to move after prefix arming" stop condition was **not** triggered.

**"A motion never touches text."** `editor_motion` is unchanged in that respect.
This is precisely why `PrefixAction::Edit` exists: `]Space` does not go through
it.

**"A relative path is relative to `workspace_root`."** Untouched — M006 reads
bytes through `DocumentSnapshot` only. No filesystem read, no git handle, no
App-level request was added, so §14's last stop condition was not triggered.

**"The cursor is a byte offset on a character boundary."** Asserted for every
landing in `every_paragraph_shape_either_lands_or_reports` and in the offset
table.

---

## 6. Failure and recovery semantics

There is nothing new to fail: a paragraph motion and a newline insertion are
synchronous, bounded, and leave no session-scoped state. The two bounded
outcomes are disclosed rather than silent —

- `ParagraphNote::NoNextParagraph` / `NoPreviousParagraph` — the scan reached the
  end of the buffer;
- `ParagraphNote::ScanBounded` — the scan hit `MAX_PARAGRAPH_SCAN_BYTES` first,
  and the notice says the answer is **not known**.

Both are transient: `note_motion` clears the disclosure on the next unrelated
motion, and `clear_history` clears it on a reconciliation, so a note can only
ever describe the keystroke that produced it. That is asserted by
`a_motion_after_a_paragraph_motion_clears_its_disclosure`.

A rejection leaves the buffer **byte-identical** — `the_newline_keys_are_refused_on_a_read_only_buffer`
asserts the text, and the paragraph refusal tests assert the cursor.

---

## 7. Migration and compatibility review

No migration. No stored state, no schema, no wire format. The only
representation change is additive: two new `TuiDocumentPresentation` fields and
one new `PrefixAction` variant, both defaulted, both cleared on reconciliation.

`next_paragraph_start` / `prev_paragraph_start` were written returning
`Result<Option<usize>, ()>` and clippy rejected `Result<_, ()>`
(`clippy::result_unit_err`). They now return `ParagraphOutcome` directly, which
is *smaller* than what they return and removes a conversion — the conversion was
the only thing the `Result` was carrying.

---

## 8. Security review

`docs/security-semantics.md` is unaffected. No new authority, no new input
channel, no new trust boundary. `]Space`/`[Space` are refused on a read-only
buffer through the same `editable` gate every other edit uses, and the refusal
is reported rather than swallowed.

**Terminal-panel parity: explicitly exempt.** The panel has no unimpaired layer,
no leaf table, and `src/tui/commands/interactive_terminal.rs` has no
`helix_selection` import to extend. Both interactive-terminal files are
**unmodified** by this milestone, which is why the exemption is stated rather
than asserted — the roadmap forbids silent divergence, and "nothing changed" is
the strongest available form of that claim.

---

## 9. Documentation and operations

- `docs/tui.md` — new §"Unimpaired navigation": the four bindings, the paragraph
  definition, the `Alt-.` rule and the `]Space`-is-not-`o` distinction, plus the
  three deferred slices with their owners.
- `architecture/tui.md` — new §"The unimpaired grammar (Helix-parity M006)":
  the `Edit` arm and why, the precedence rule, the three outcomes, the
  whole-walk budget, unprovable blankness, the recorded divergence, and the
  repeat-slot rule. The heading carries an explicit naming note because this
  document already has an unrelated `M006-A`…`M006-F` series, and two
  milestone numbers in one file are exactly the kind of collision that produces
  a wrong citation later.
- `scripts/check_tui_editor_prefix_state.py` — **model prose updated, no
  assertion relaxed.** The plan's §14 first stop condition was explicitly "would
  require editing or weakening the guard rather than updating its prose"; it did
  not. The docstring's old claim that every empty `leaves()` row is a capture
  became *true* at this milestone rather than being changed to fit.

---

## 10. Unresolved findings

1. **`o`/`O` never insert a line.** `enter_insert` maps `OpenLineBelow` and
   `OpenLineAbove` to `_ => cursor`, so `resolve_edit`'s open-line arms are
   reachable only from `open_line_below_splits_the_existing_terminator`.
   `o` enters Insert on the **current** line. Pre-existing, found here, **not
   fixed** — it is another milestone's behaviour and changing it is not this
   milestone's to decide. Owner: whoever next touches the insert-entry chords.
   It is the one place where this milestone's "the two verbs share one
   implementation" claim is about a code path no key reaches.
2. **No green `verify.sh full` exists in this worktree**, and this milestone's
   attempt did not improve on that. `verify.sh full` cleared its quick gate and
   its clippy stage, then nextest was **SIGKILLed during test enumeration**, so
   no workspace test ran (§4.1). The failures M007 and M008 recorded outside
   `tui::` — `lsp`, `tool_execution`, `workspace_isolation`,
   `session_family::control_m004_controller_lease`, `tool_surface_minimization`,
   `core::daemon::tests::session_create_through_daemon` — remain un-run here and
   therefore un-adjudicated by this milestone: `core::daemon` was *proven*
   pre-existing by M007 at baseline `84440fe7f` with changed sources stashed,
   and the others are recorded as environment-dependent **by inspection, not by
   proof**. Nothing in this diff touches those modules. A SIGKILL is **not**
   evidence that they now pass, and this closure does not claim it does.
3. **Nothing is committed.** The closure rests on a tree carrying uncommitted
   M004–M008 work as well.
4. **`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`**
   timed out at 120 s during M007's `verify.sh full`. Unrelated; not re-run at
   baseline.
5. **Unimpaired change navigation (`]g`/`[g`/`]G`/`[G`) remains deferred
   product work with no reachable data source** — unchanged by this milestone
   and now *asserted unbound* so it cannot land by accident. `egggit`'s
   `ChangedFile`/`FileDiff` carry no line ranges; `DiffHunk` is behind a private
   widget method. Recorded in `plans/registry.md` §"Milestones eligible for fresh
   planning" with the three data-source options and the constraint that only
   daemon-side ranges keep the editor free of filesystem authority.
6. **The diagnostic quartet remains M013's**, for the reasons in Amendment A1.2 —
   the roadmap claimed it twice and the read path is broken until M013 repairs it.
   M006 asserts it unbound rather than leaving the question open.

---

## 11. Roadmap disposition

| Milestone | Before | After | Note |
|---|---|---|---|
| M006 | `ready` | **closed** | this record; Amendment A2 records five implementation findings |
| M009 | `ready`, blocked on ADR-0034 acceptance alone | unchanged | no edge of M009's was touched here |
| M010 | blocked on M013 | unchanged | |
| M013 | `ready` | unchanged | owns the diagnostic quartet |
| M011 / M012 | eligible for fresh planning | unchanged | M002's edge was discharged in M002 |

The unblock audit for downstream work: M006's only hard dependency was **M002**,
and that edge was discharged in M002, so M006's closure unblocks nothing that
was waiting on it. The roadmap's own "M009 waits on M008 plus ADR-0034" sentence
was already narrowed to ADR-0034 alone by M008's closure; this milestone does not
touch it, and **the ADR edge is not discharged here** — a proposed ADR confers
no authorization (ADR-0030 precedent).

---

## 12. Registry updates

- `plans/registry.md` roadmap row: `active (M001–M005, M007–M008 closed; M006,
  M013 ready; …)` → `active (M001–M008 closed; M013 ready; …)`, plus the M006
  closure record added to its closure list.
- `plans/registry.md` M006 plan row: `ready` → `closed` with the closure link,
  and its prose extended to state what shipped (the `Edit` arm, the
  no-selection-moves finding, the three outcomes, the recorded divergence, the
  `Alt-.` fix and the `o`/`O` gap).
- `plans/subsystems/tui-editor-helix-parity-roadmap.md` §12 status table: M006
  `ready` → `closed` with the closure link.
- `plans/subsystems/tui-editor-helix-parity-roadmap.md` M006 entry: dated
  **Amendment A2** with the five findings above.
- `plans/subsystems/tui-editor-helix-parity-roadmap.md` §12 note: the
  "M006 and M007 are ready" paragraph now states all three are closed, and the
  `Alt-.` composition note now records that M006 discharged it.
- `plans/implementation/tui-editor-helix-parity/006-unimpaired-navigation-non-ts-subset.md`
  header: `Status: ready for handoff` → `Status: implemented — **closed
  2026-10-19**` plus the closure link and a one-line summary, matching the
  wording M007's and M008's plan headers already use.

Cross-surface reconciliation printed and checked: **M006 `closed`, M007 `closed`,
M008 `closed`** on the plan header, the closure record, the roadmap table and the
registry rows.

```bash
cd plans
head -14 implementation/tui-editor-helix-parity/00[678]-*.md | grep -E '^(#|Status)'
# -> M006 "Status: implemented — closed 2026-10-19"
# -> M007 "Status: implemented — closed 2026-10-18"
# -> M008 "Status: implemented — closed 2026-10-19"
for n in 006 007 008; do head -6 closure/tui-editor-helix-parity/$n-status.md \
  | grep -E '^(# |Status)'; done
# -> all three "Status: closed"
grep -E '^\| M00[678] \|' subsystems/tui-editor-helix-parity-roadmap.md
# -> all three **closed**

# Column integrity for the two registry rows this milestone edited.
awk -F'|' 'NR==102 {print NF}' registry.md   # -> 8
git show HEAD:plans/registry.md | awk -F'|' 'NR==102 {print NF}'   # -> 8
awk -F'|' 'NR==145 {print NF}' registry.md   # -> 7
git show HEAD:plans/registry.md | awk -F'|' 'NR==145 {print NF}'   # -> 7
```

The registry's row count is unchanged at **544**, and neither edited row changed
its column count — the edits replaced cell text and added no `|`. (One row in the
registry has ten pipes; it is `| Agent recovery escalation authority corrective |`
at line 97, it is **pre-existing** — `git show HEAD` reports the same ten — and
this milestone did not touch it.)