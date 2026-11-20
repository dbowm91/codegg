# TUI Editor Helix Capability Parity Milestone 002 — Change Verbs and No-Yank Deletion

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-002--change-verbs-and-no-yank-deletion`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Bind `c`, `Alt-c` and `Alt-d` so "delete the selection and start typing" is one
gesture, and so the two explicit no-yank spellings exist, with the
yank-on-change semantics that Helix has **deferred to M007 for a stated reason**
rather than silently approximated.

## 2. Why this milestone is ready

No hard dependency. Everything it needs is closed:

- `HelixVerb::DeleteSelection` and the `delete_ranges` path resolve a multi-member
  delete to one `TextTransaction`, and `submit_edit` is the single funnel that
  applies it.
- `enter_insert` already decides whether a read-only attachment may enter Insert
  and returns `None` when it may not, so a dead Insert mode over an unwritable
  buffer is not possible.
- The insert-session model (one undo step per typing burst, a run starting where
  the cursor ends) is closed and is what makes `c` + typing two undo steps rather
  than one or three.
- The key authority guard polices the three dispatch stages, so adding three
  chords is a counted change rather than an unbounded one.

The interface dependency with roadmap M007 is recorded in §9: M007 replaces the
system clipboard with a register set and must keep this milestone's promises
true while making `c`/`d` yank into the unnamed register.

## 3. Current implementation evidence

At the baseline:

- `d` is bound to `HelixVerb::DeleteSelection`. Its outcome is
  `BufferKey::DeleteRanges(ranges)`, which the App layer turns into one
  `TextTransaction` through `editor::delete_ranges` and then `submit_edit`.
- **`d` does not write the clipboard.** The yank is a separate verb: bare `y` and
  `space y` return `BufferKey::YankRanges(ranges)`, and `yank_editor_ranges`
  performs the clipboard write. This is a deliberate split, not an omission.
- Consequence, and the finding this milestone is built around: **CodeGG's `d`
  already behaves like Helix's `Alt-d`**, because Helix's `d` yanks into a
  register and CodeGG has no register — only the system clipboard.
- `c`, `Alt-c` and `Alt-d` are absent from the chord table. `c` is a bare
  character with no arm, so it falls through to the composer and types a `c`.
  `Alt-c` and `Alt-d` are modified chords with no owner, so they are inert.
- `enter_insert` refuses a read-only attachment and reports it; the editor's own
  comment records that entering Insert over an unwritable buffer is how a user
  ends up in a dead Insert mode with no explanation.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm.

## 4. Invariants that must not regress

- **One undo step for the delete.** `c` submits exactly one transaction; entering
  Insert adds none, because entering Insert edits nothing.
- **A typing run starts where the cursor ends.** The `c` delete and the word
  typed into the gap are two entries, and the word is one entry however many
  keystrokes it took. This is the predecessor M019 rule and it must survive.
- **No dead Insert mode.** On a read-only attachment `c` is refused with a notice
  and the route does not enter Insert.
- **`d`'s current behaviour is not silently changed.** See §5 and §9.
- **No second text buffer** and no mutation outside `apply_local`.
- **One owner per chord**, no shadowed arm.
- **Positions, never text**: `c` computes its transaction from the live snapshot
  and retains nothing.

## 5. Scope

### In scope

- `c` bound in `handle_normal_key`: delete the held selection (or the character
  under a collapsed selection) and enter Insert at the deletion point.
- `Alt-c` bound as the explicit no-yank spelling of the same gesture.
- `Alt-d` bound as the explicit no-yank delete.
- `docs/tui.md` and the keymap box updated in the same change.
- The **D1 decision** below, recorded in the plan and in the code.

### Explicitly out of scope

- **Making `d` and `c` yank into a register.** That is roadmap M007's job. See D1.
- Registers, named or unnamed. M007.
- `c` interacting with counts (`3c`). M004.
- A `c` that re-indents the line it opens. Not a Helix behaviour and not needed.
- Any change to `p`/`P`/`R` or to the clipboard model.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The gesture is synchronous over the current snapshot.

### Frontend or operator surface

- `src/tui/editor.rs`: a `HelixVerb` variant (or a flag on the existing
  `DeleteSelection`) for "delete and report that Insert should follow", plus the
  collapsed-selection rule. Prefer a distinct variant: a flag on
  `DeleteSelection` would make `d` and `c` share an arm and would hide the
  difference the two verbs exist to express.
- `src/tui/commands/editor.rs`: the three chord arms; the `c` arm must delete
  **then** enter Insert, and must not enter Insert when the delete was refused.
- `mutates_buffer` gains the change verb, because it writes.

### Security and authorization

No change. Read-only refusal is the existing gate and must be exercised, not
assumed.

### Documentation and static guards

- `docs/tui.md`: `c`, `Alt-c`, `Alt-d` in the Normal-mode table, with an explicit
  statement that CodeGG's `d` does not touch the clipboard today and that M007 is
  the milestone that changes it.
- The `space ?` keymap box follows.
- Both guard figures move with the chords in the same change.

## 7. Ordered work packages

### Work package A — The change verb's resolution

Intent: resolve "delete and insert" into one transaction plus a mode request,
without a second delete implementation.

Required changes: a verb that resolves the held set to delete ranges exactly as
`DeleteSelection` does, and reports that Insert follows. For a **collapsed**
selection it deletes the character under the cursor; at a line end it deletes the
newline and joins the lines. That edge is **pinned against the reference dialect
and asserted by a test**, not assumed.

Acceptance evidence: a selection deletes and the cursor lands at the deletion
point; a collapsed selection deletes one character; a collapsed selection at a
line end joins the lines; the whole thing is one history entry; a read-only
attachment refuses and does not enter Insert.

### Work package B — The three chord arms

Intent: give each gesture exactly one owner.

Required changes: `c` (`NONE`), `Alt-c` (`ALT`), `Alt-d` (`ALT`). `c` must be
claimed even with nothing selected, for the reason `d` is already claimed: an
unclaimed `c` escapes to the composer and types a `c`.

Order inside the `c` arm: resolve and submit the delete, **then** enter Insert.
Entering Insert first and deleting second would leave the reader in Insert with
the text still there if the delete were refused.

Acceptance evidence: a dispatch-path test per chord through
`editor_buffer_key`; `c` with nothing selected is consumed, not typed; `Alt-c`
and `Alt-d` are consumed; on a read-only attachment `c` produces a notice and the
route stays in Normal.

### Work package C — The no-yank promise, proven

Intent: make "does not touch the clipboard" a checkable property rather than a
comment.

Required changes: a test that seeds the clipboard with a known value, performs
`Alt-d` and `Alt-c` and the current `d`, and asserts the clipboard is unchanged
in all three cases. This test is what M007 will have to invert for `d`/`c` and
keep for `Alt-d`/`Alt-c`, so it must exist before M007 starts.

Acceptance evidence: the clipboard is byte-identical after each gesture; `y`
still writes it (the positive control), so the test cannot pass by the clipboard
write being broken.

### Work package D — Guard and documentation

Intent: keep the binding set coherent in one change.

Required changes: update both guard figures; add the three keys to
`docs/tui.md`; update the keymap box.

Acceptance evidence: the key authority guard passes with the new count and no
shadowed arm; the doc-drift guard passes in both directions.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** The delete is one transaction; if the controller rejects
  it the notice reports the rejection, the document is unchanged, and Insert is
  **not** entered. There is no state in which the text is deleted but the mode is
  Normal, or the mode is Insert but the text survives.
- **Duplicate delivery.** None; synchronous local gesture.
- **Restart.** No state.
- **Cancellation races.** None; no async work added.
- **Stale generations.** The verb reads the current snapshot.
- **Bound reached.** A multi-member change that would exceed `MAX_DOCUMENT_EDITS`
  is refused with the existing notice, and Insert is not entered.

## 9. Compatibility and migration

**No storage, protocol, or configuration migration.**

The compatibility surface is twofold.

1. **The keymap.** `c` stops reaching the composer on the editor route. That is
   the intended effect of a binding and is recorded in `docs/tui.md`.

2. **The yank-on-change divergence, which is the substantive decision of this
   milestone.**

   - **D1 — what `c`/`d` do about yanking, given there are no registers.**
     Helix's `delete_selection` and `change_selection` yank into a register;
     `Alt-d`/`Alt-c` are the no-yank spellings. CodeGG has no register, so its
     only yank target is the **system clipboard**. Three candidates were
     considered.
     *(a)* Make `c`/`d` write the system clipboard, matching Helix's gesture
     names at the cost of clobbering the user's clipboard on every delete.
     *(b)* Leave `c`/`d` not yanking, bind `Alt-d`/`Alt-c` as the explicit
     spellings, and record that the two pairs are identical until M007.
     *(c)* Introduce a single unnamed internal register in this milestone so the
     two pairs differ now.
     **Recommendation: (b).** (a) is a user-hostile regression: a user's
     clipboard is often the reason they are editing. (c) pulls M007's substrate
     into this milestone and would ship a two-tier clipboard (an internal
     register plus the system clipboard) with no way to inspect either — the
     register set is what makes the model legible, and that is M007.
     (b) delivers the three gestures and states the limitation plainly.
   - **The obligation this creates for M007, which is an interface dependency:**
     when registers land, `d` and `c` must yank into the **unnamed** register and
     `Alt-d`/`Alt-c` must not, and `space y`/`y` must keep reaching the system
     clipboard. The clipboard test from work package C is the assertion M007
     inverts for `d`/`c` and preserves for the `Alt` pair.

Removal criteria for the transitional state: M007's closure record must state
that `d`/`c` now yank, and this plan's D1 is the document that says so was
always intended.

## 10. Required tests

### Focused unit tests

- The change verb against a `DocumentSnapshot`: a selection, a collapsed
  selection mid-line, a collapsed selection at a line end, and an empty buffer.
- The no-yank promise for `Alt-d` and `Alt-c`.

### Integration tests

- One dispatch-path test per chord through `editor_buffer_key`.
- A read-only test asserting the notice, unchanged text, and Normal mode
  retained.
- A test asserting `c` followed by typing produces exactly two history entries:
  the delete and the run.

### Restart and recovery tests

None. No durable state; the closure record must say so rather than omit it.

### Contention and cancellation tests

- A multi-member `c` asserts one history entry for the delete.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A change whose transaction would exceed `MAX_DOCUMENT_EDITS` is refused with a
  notice, the document is byte-identical, and Insert is not entered.

### Migration and compatibility tests

- The clipboard-preservation test from work package C, with `y` as the positive
  control.
- The doc-drift guard as the keymap compatibility check.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

`verify.sh quick` runs no tests. The `cargo test` lines are the test evidence;
`quick` is the guard evidence. Do not present one as the other.

## 12. Documentation updates

- `docs/tui.md`: `c`, `Alt-c`, `Alt-d` in the Normal-mode table; the yank
  divergence stated where the delete verbs are described, with M007 named as the
  milestone that changes it.
- The `space ?` keymap box follows.
- The code comment on the change verb states why it does not yank yet, so a later
  reader does not "fix" it in isolation and break D1's sequencing.

## 13. Acceptance criteria

- Pressing `c` with a selection deletes it, leaves the cursor at the deletion
  point, and enters Insert.
- Pressing `c` with a collapsed selection deletes the character under the cursor
  and enters Insert; at a line end it joins the lines.
- `Alt-c` and `Alt-d` are bound and consumed.
- `c` on a read-only attachment reports a notice and leaves the route in Normal.
- None of `c`, `Alt-c`, `Alt-d` changes the system clipboard, and `y` still does.
- `c` plus a typing burst is exactly two history entries.
- The key authority guard reports no shadowed arm; the doc-drift guard passes in
  both directions.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The collapsed-selection behaviour (delete the character, or delete the line, or
  do nothing) cannot be pinned from the reference dialect. Record and ask.
- Making `c` work appears to require changing the clipboard model. That is M007's
  boundary; report rather than extending scope.
- Entering Insert and deleting cannot be ordered so a refusal leaves the mode
  untouched. That is a correctness hazard, not a detail.
- A chord collides with an existing owner and the fix would re-spell an existing
  binding.
- The work expands into registers, counts, or `p`/`P` semantics.

## 15. Closure evidence required

- The key authority guard's summary with the new count and no shadowed arm.
- One dispatch-path test per chord, named individually.
- The clipboard-preservation test with its positive control.
- The read-only refusal evidence, including that Normal mode was retained.
- The two-history-entry assertion for `c` plus typing.
- A statement that no migration, protocol variant, crate, or new authority was
  added, and that D1's sequencing is recorded for M007.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **`c` is a bare character with no arm today**, so it currently types into the
  composer. That is the fallthrough hazard in its most visible form and is the
  strongest reason this milestone is worth landing early.
- **Do not "fix" the missing yank.** D1 is a decision with a reason and a named
  successor milestone. Changing it here would ship a two-tier clipboard with no
  way to inspect it.
- **The delete-then-enter order is load-bearing** and must be asserted, not
  merely written.
- **`Alt-c` and `Alt-d` are modified chords**, so they are inert today rather
  than typed. Binding them is a discoverability gain, not a corruption fix.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
