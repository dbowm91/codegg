# TUI Editor Helix Capability Parity Milestone 004 — Counts

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-004--counts`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Accept a numeric prefix on the motions Helix documents it for — `3w`, `5j`,
`<n>G`, `<n>gg` — without widening the deliberately one-slot prefix state
machine, without stealing `0` from `LineStart`, and with the count visibly
disclosed while it is being typed.

## 2. Why this milestone is ready

No hard dependency. The interface it touches is closed and small:

- `EditorPrefix` is one prefix wide and resolves through a single
  `resolve_pending_prefix` call that runs **before** every bare-key arm. A count
  is not a prefix — it is a modifier that survives prefix arming and is consumed
  by the completed gesture — so it lives beside the machine rather than inside
  it.
- `TuiDocumentPresentation` already holds bounded, lifecycle-cleared
  presentation state (the command line, the search line, the undo history and
  its cursor), so a count field has an established home and an established
  clearing contract.
- `EditorCommand` is the motion vocabulary and `apply_motion` is the single
  resolver, so "apply this motion N times" is a loop over one function rather
  than N special cases.
- The `EditorNotice` path is the established way to disclose a refusal or a
  bound, and the banner row already renders the editor's own state.

## 3. Current implementation evidence

At the baseline:

- `handle_normal_key` matches on the `(modifiers, key.code)` tuple. `0` is bound
  to `EditorCommand::LineStart`, `$` to `LineEnd`, `G` to `BufferEnd`, and `g`
  arms `EditorPrefix::Goto` whose `g` leaf resolves to `BufferStart`.
- **No motion accepts a repeat.** `G` and `gg` take no numeric argument; the
  goto-mode leaf table's own documentation records that `g|` is omitted because
  "this editor's `G` takes no count today".
- No digit other than `0` is bound in Normal mode, so `1`–`9` currently fall
  through `BufferKey::Unhandled` to the composer and **type digits into the
  prompt box**.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm, and its
  model is "a chord is a `(modifiers, char)` claim over a character; `char is
  None` marks the wildcard catch-all".
- `MAX_EDITOR_PENDING_PREFIX` is 5 and bounds the prefix payload, not a count.

## 4. Invariants that must not regress

- **The prefix machine stays one slot wide.** A count may not be stored in
  `EditorPrefix`, and adding counts may not make two prefixes simultaneously
  armable.
- **`0` remains `LineStart`** when no count is pending. A count may only begin
  with `1`–`9`; a leading `0` is never a count.
- **A count cannot survive a mode change, a document switch, a reload, a resync,
  a reconnect, or a close.** It is presentation state and is cleared with it.
- **A count cannot survive a gesture that did not consume it.** A stray digit
  followed by an unrelated key must not leave a count armed for the next verb.
- **Every motion remains bounded.** A count is clamped by the document, not by
  trusting the number.
- **One owner per chord**, no shadowed arm, and the guard must still be able to
  tell a digit claim from the printable wildcard.
- **No second text buffer.**

## 5. Scope

### In scope

- A bounded count field on `TuiDocumentPresentation`.
- Digit accumulation in Normal mode only.
- Counts applied to the **motions**: character moves, vertical moves, word and
  long-word motions, paragraph motions, half/full page, `G`, `gg`, and the
  goto-mode `e`/`h`/`l`/`s` leaves.
- Visible disclosure of the pending count.
- `docs/tui.md` and the keymap box updated in the same change.

### Explicitly out of scope

- **Counts on repeatable verbs** (`3d`, `2y`, `4p`, `3>`). Helix's documented
  `<n>` surface is the motions, and repeating a *change* verb needs a rule for
  what "three deletes" means when the first delete removed the text the second
  would have addressed. That is a separate decision; record it, do not guess.
- Counts on `Alt-.` repeat.
- Counts on the half/full page keys beyond a single multiplier.
- `Ctrl-a`/`Ctrl-x` increment/decrement (roadmap §3).
- A count on a prefix that resolves to a **capture** (`3f x`). The capture's
  target character arrives after the count, and repeating a find is `Alt-.`'s
  job. Record the omission.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Counts are synchronous presentation state.

### Frontend or operator surface

- `src/tui/document_session.rs`: a `pending_count` field with a doc comment
  stating the clearing contract and the `0` rule.
- `src/tui/commands/editor.rs`: the digit arm, placed **before** the `0` arm; the
  consumption point in the motion path; the clearing point for a non-digit key.
- `src/tui/editor.rs`: a `MAX_COUNT` constant and the clamp; the banner
  disclosure string.
- `src/tui/app/render.rs`: the count rendered on the banner row while pending.

### Security and authorization

No change.

### Documentation and static guards

- `docs/tui.md`: the count grammar, the `1`–`9` rule, what honours a count and
  what does not.
- `scripts/check_tui_editor_key_authority.py` must be **checked against** the new
  arm. If the guard classifies a `Char(ch)` binding arm as the wildcard, the
  guard needs a distinction for a digit claim; that is a guard change and must
  carry a `--self-test` case proving it rejects an injected shadowed digit.

## 7. Ordered work packages

### Work package A — The count field and the digit grammar

Intent: accumulate a bounded count without touching the prefix machine.

Required changes: the field, `MAX_COUNT`, the accumulation rule
(`count = count * 10 + digit`, saturating at `MAX_COUNT`), and the **single** arm
that claims digits. One arm, not nine: a guard-style
`(m, KeyCode::Char(ch)) if m == none && ch.is_ascii_digit() && (ch != '0' ||
count.is_some())` placed ahead of the `0` arm is the shape, and the plan requires
the implementer to confirm the guard's classification of it rather than assume.

Acceptance evidence: `1`–`9` no longer reach the composer; `0` with no count is
still `LineStart`; `0` after `1` extends the count to 10; a count saturates at
`MAX_COUNT` and discloses that it did; the guard reports the arm and the
implementer records how it classified it.

### Work package B — Apply the count to motions

Intent: one loop over `apply_motion`, not N special cases.

Required changes: the motion path reads the count once, applies the motion that
many times, and consumes it. The count survives arming a prefix so `3gg` and
`3ge` work; it is consumed by the leaf that completes the gesture. Each motion
must remain bounded, so a count larger than the document clamps at the boundary
rather than looping the full number.

Acceptance evidence: `5j` moves five lines and clamps at the last line; `3w`
moves three words; `3gg` reaches line 3 from anywhere; `3G` behaves as Helix
documents; a count on a motion that cannot move (for example `5h` at the buffer
start) lands at the boundary and reports nothing; the motion is a single history
entry if it is a motion at all (motions are not edits).

### Work package C — Clearing

Intent: a count can never leak into the next gesture.

Required changes: clear on every mode change, on open, reload, resync, reconnect
and close, and on any key that is neither a digit nor a gesture that consumes the
count. The clearing must be structural — one function called from the lifecycle
transitions and from the dispatch tail — rather than a scatter of assignments.

Acceptance evidence: `3` then `Esc` then `j` moves one line; `3` then a document
switch then `j` moves one line; `3` then a prefix that disarms without resolving
(`3` then `g` then an unbound key) leaves no count armed.

### Work package D — Disclosure

Intent: the reader can see the count they are building.

Required changes: render the pending count on the banner row, in the same place
and style the editor already uses for its own state. A count is not a notice and
must not use the notice slot, because a notice is a statement about what just
happened and a count is a statement about what is about to.

Acceptance evidence: typing `1` then `2` shows `12`; consuming the count clears
it in the same frame; a saturated count shows the clamped value.

### Work package E — Guard and documentation

Intent: keep the binding set and the document coherent in one change.

Required changes: update the guard's expected figure or its model; document the
grammar; update the keymap box.

Acceptance evidence: the key authority guard passes and its `--self-test` rejects
an injected shadowed digit if the guard was changed; the doc-drift guard passes in
both directions.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A count that is never consumed is cleared by the clearing
  path, so there is no half-armed state.
- **Duplicate delivery.** None; synchronous.
- **Restart.** No state.
- **Cancellation races.** None; no async work added.
- **Stale generations.** A count is cleared by the same transitions that clear
  the rest of the presentation, so a stale count cannot survive a document
  switch.
- **Bound reached.** A count above `MAX_COUNT` saturates and discloses; a count
  larger than the document clamps at the document boundary.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**The compatibility surface is the digit keys.** `1`–`9` currently reach the
composer on the editor route and type digits into the prompt box. After this
milestone they belong to the buffer in Normal mode. That is the intended effect
of a binding, it is scoped to Normal mode on the editor route, and it is recorded
in `docs/tui.md`.

There is no transitional state and no removal criteria: the old behaviour was
"the key types a digit", which is the fallthrough, not a feature.

## 10. Required tests

### Focused unit tests

- Accumulation: single digit, multi-digit, `0` continuation, saturation.
- The `0` rule: `0` alone is `LineStart`; `10` is a count of ten.
- Clearing: after a mode change, after a document switch, after an unconsumed
  count.

### Integration tests

- One dispatch-path test per honoured motion class with a count: character,
  vertical, word, long-word, paragraph, half page, `G`, `gg`, and one goto leaf.
- A test that `1`–`9` no longer reach the composer.
- A test that a count larger than the document clamps at the boundary.

### Restart and recovery tests

None. No durable state; the closure record must say so rather than omit it.

### Contention and cancellation tests

- A count that survives prefix arming (`3gg`) and is consumed exactly once, with
  a second press of `g` proving the count is gone.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A count above `MAX_COUNT` discloses the clamp rather than silently wrapping.

### Migration and compatibility tests

- The doc-drift guard as the keymap compatibility check.
- A test asserting `0` is still `LineStart`, which is the one binding a careless
  digit arm would break.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_key_authority.py --self-test
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

Run the guard's `--self-test` first if the guard was changed at all: a guard edit
without a self-test proving it still rejects an injected shadowed chord is not
evidence.

## 12. Documentation updates

- `docs/tui.md`: the count grammar; that a count begins with `1`–`9` and `0` is
  `LineStart`; which motions honour a count; that repeatable verbs and captures do
  not, with the reason.
- The `space ?` keymap box follows.
- The `pending_count` field's doc comment states the clearing contract.

## 13. Acceptance criteria

- `5j` moves five lines; `3w` moves three words; `3gg` reaches line 3; `10G`
  behaves as Helix documents.
- `0` with no pending count is still `LineStart`.
- A count is visible on the banner row while it is being typed and disappears
  when it is consumed.
- A count never survives a mode change, a document switch, or an unconsumed
  gesture.
- `1`–`9` no longer reach the composer in Normal mode on the editor route.
- The key authority guard reports no shadowed arm; the doc-drift guard passes in
  both directions.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The key authority guard cannot distinguish a digit claim from the printable
  wildcard without a model change that would weaken its shadowing rule. Report
  the guard shape rather than loosening it.
- A count cannot be made to survive prefix arming without widening the prefix
  machine. That contradicts §4 and is a design change, not a detail.
- Honouring a count on a motion requires an unbounded loop. Clamp or refuse;
  report if neither is possible.
- The work expands into repeatable verbs, `Alt-.` counts, or increment/decrement.
- A reviewer asks for `0` to begin a count. That inverts §4's `0` rule and is an
  owner decision.

## 15. Closure evidence required

- The key authority guard's summary with no shadowed arm, and either its
  unchanged figure or the new figure with the reason it moved.
- If the guard changed, its `--self-test` output rejecting an injected shadowed
  digit.
- One dispatch-path test per honoured motion class, named individually.
- The `0`-is-still-`LineStart` test.
- The clearing tests for mode change, document switch and unconsumed gesture.
- The saturation and clamp evidence.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **The guard's model is the hazard.** It defines a chord as a `(modifiers, char)`
  claim and treats `char is None` as the wildcard catch-all. A binding arm written
  `KeyCode::Char(ch)` may be classified as a wildcard, which would make the guard
  pass for the wrong reason. Confirm the classification and record it; do not
  assume.
- **`0` is the trap.** It is the one existing binding a digit arm can silently
  steal. The arm must be placed before the `0` arm *and* must not match `0` when
  no count is pending.
- **One arm, not nine.** Nine arms would move the guard's count by nine and add
  nine places for a future edit to disagree.
- **A count is not a notice.** Do not reuse the notice slot for disclosure; a
  notice describes what happened and a count describes what is about to.
- **Counts do not repeat edits.** Helix's documented `<n>` surface is the
  motions. Repeating a change verb needs a rule this milestone does not have.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
