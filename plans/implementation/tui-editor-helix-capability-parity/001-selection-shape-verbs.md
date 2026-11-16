# TUI Editor Helix Capability Parity Milestone 001 — Selection Shape Verbs

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-001--selection-shape-verbs`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Bind seven Helix selection-shape verbs — `X`, `Alt-x`, `_`, `J`, `Alt-J`, `>`
and `<` — so a reader can widen a selection to whole lines, narrow it back,
trim it, join the lines it covers, and indent or unindent them, all without
leaving Normal mode and all through the one edit funnel.

## 2. Why this milestone is ready

No hard dependency. Every mechanism it needs already exists and is closed:

- The selection **set** with a distinguished primary, its `normalize_set`
  ordering rule, and the shared `helix_selection` state machine are closed
  (predecessor M007/M008).
- The shared edit funnel — `apply_helix_edit` → `submit_edit` →
  `DocumentController::apply_local` — is the single path every writing verb
  already takes, and it is what makes undo, the retained-bytes bound, LSP
  staleness and selection clearing apply for free.
- The line geometry primitives exist in `src/tui/text_geometry.rs`:
  `line_start`, `line_content_end`, `buffer_content_end`, `clamp_offset`,
  `next_char_boundary`, `prev_char_boundary`.
- The read-only gate is established: `grows_selection` and `mutates_buffer` in
  `src/tui/commands/editor.rs` decide which verbs are refused on a read-only
  attachment, and `apply_helix` reports the refusal through `EditorNotice`.

The interface dependency on the selection set is stable: `HelixVerb` already
carries `SelectAll`, `AlignSelections`, `RotateSelectionContents` and
`MergeConsecutiveSelections`, so adding verbs that reshape members is an
extension of a closed contract, not a change to it.

## 3. Current implementation evidence

At the baseline the following are true and were verified in the tree:

- `EditorMode` has three variants — `Normal`, `Insert`, `Select` — and `Select`
  is a *verb* mode that coexists only with `InputMode::Normal`.
- `ObjectKind` is the parser-free set only: `Word`, `LongWord`, `Paragraph`,
  `Pair(char)`, `ClosestPair`. The syntax-node objects are explicitly not
  present because "a variant nothing constructs is a claim, not a hook".
- `x` (`HelixVerb::SelectLine`) selects the cursor's whole line and grows one
  line per press. It is an *action*, not a mode, so a movement does not
  silently extend it.
- `X`, `Alt-x`, `_`, `J`, `Alt-J`, `>` and `<` are **absent from the chord
  table**. `handle_normal_key` has no arm for any of them, so each falls through
  `BufferKey::Unhandled` to the composer's claim table and ultimately to the
  app-level bindings.
- The key authority guard reports **2 guard-block chords, 74 Normal-mode
  chords, 8 Insert-mode chords** and no shadowed arm.
- `docs/tui.md` carries a checked "deliberately not bound" list, and
  `scripts/check_tui_editor_doc_drift.py` compares the code's claimed chords and
  leaf gestures with the document in both directions.
- `MAX_DOCUMENT_EDITS` bounds one transaction's edit count, and
  `delete_ranges` refuses rather than truncating when a multi-cursor gesture
  would exceed it.

The concrete gap this milestone closes is the fallthrough: pressing `c`, `J` or
`>` in Normal mode currently types that character into the prompt box.

## 4. Invariants that must not regress

- **No second text buffer.** Every read is a scoped `try_snapshot()` borrow
  dropped at the end of the statement; every mutation is `apply_local`.
- **One owner per chord.** A chord added to `handle_normal_key` must not be
  claimed by the guard block or by `handle_insert_key`, and the key authority
  guard must still report no shadowed arm.
- **One undo step per gesture.** A multi-cursor `>` is one entry in the
  history, not one per member.
- **Read-only refusal is visible.** A writing verb on a read-only attachment
  sets a notice; it never appears to work.
- **Positions, never text.** Selections stay byte offsets into the live
  replica; a verb may not retain a copy of the document to compute its result.
- **Bounded work per gesture.** No verb may scan an unbounded amount of the
  document; the existing scan budgets are reused rather than re-invented.
- **The published keymap agrees with the code** in both directions, in the same
  change.

## 5. Scope

### In scope

- `HelixVerb` gains the line-bounds, trim and join verbs, and `EditorCommand`
  gains whatever indent verb the implementation needs.
- `X`, `Alt-x`, `_`, `J`, `Alt-J`, `>`, `<` bound in `handle_normal_key`, each
  with exactly one owner.
- An **indent-unit source** for `>`/`<`, chosen and recorded (§7 work package D).
- `docs/tui.md` and the `space ?` keymap box updated in the same change.
- Tests proving reachability through the real dispatch path, not through a
  helper.

### Explicitly out of scope

- Counts on these verbs (`3>` indenting three levels). That is roadmap M004.
- Syntax-node objects or any tree-sitter behaviour (ADR-0034 Option B; M014).
- Any `>`/`<` interaction with a language formatter. Formatting is `=` and is
  already the LSP preview path.
- An editor configuration key for indent width (§7 D1 records why).
- Reflowing or re-indenting a *block* (a whole `if` body). `>` indents the
  lines the selection covers and nothing else.

## 6. Required production changes

### Core/domain

None. `codegg-core` and `codegg-document` are untouched. The verbs are
frontend presentation over `TextTransaction`s.

### Storage and migrations

None.

### Protocol and DTOs

None. No `CoreRequest` variant, no projection field.

### Runtime and concurrency

None. Every verb is synchronous over the snapshot the caller already holds and
submits one transaction.

### Frontend or operator surface

- `src/tui/editor.rs`: the new `HelixVerb` variants and their resolution
  against a `DocumentSnapshot`; the line-bounds, trim and join geometry; the
  indent-unit source.
- `src/tui/commands/editor.rs`: the seven chord arms, and the read-only
  classification of each verb in `grows_selection` / `mutates_buffer`.
- `src/tui/document_session.rs`: only if the indent source needs a cached field.
  Prefer computing it per gesture from the snapshot so no new retained state
  exists.

### Security and authorization

No change. The verbs write only through `apply_local`, which is already the
authorized path, and the read-only gate is the existing one.

### Documentation and static guards

- `docs/tui.md`: the Normal-mode table gains the seven keys and the "not bound"
  list loses any entry that named them.
- The `space ?` keymap box follows the same table.
- `scripts/check_tui_editor_key_authority.py` needs no logic change; its
  expected Normal-mode count moves and must be updated **with** the chords, not
  after.
- `scripts/check_tui_editor_doc_drift.py` needs no logic change; its expected
  chord and gesture figures move with the documentation.

## 7. Ordered work packages

### Work package A — Line-bounds verbs (`X`, `Alt-x`)

Intent: make a selection line-wise and back again, over the whole set.

Required changes: a `HelixVerb` variant for each; resolution that widens every
member to the bounds of the lines it touches (`X`) and narrows it back
(`Alt-x`). `X` on an already line-wise member is a no-op that reports nothing —
it is not an error. The exact treatment of the trailing newline is **pinned
against the reference dialect** and asserted in a test rather than assumed; the
implementation must not guess and then document the guess.

Acceptance evidence: a dispatch-path test that presses `X` and observes the
selection covering the whole line; a second press is a no-op; `Alt-x` returns a
line-wise selection to the character selection it came from where that is
recoverable, and to the line's own bounds where it is not, with the difference
asserted; a read-only attachment refuses `X` with a notice and permits `Alt-x`
(§4's narrow reading: growth is refused, navigation-like motion is not).

### Work package B — Trim (`_`)

Intent: strip leading and trailing whitespace from each member.

Required changes: one verb, resolved over the set, with the all-whitespace
member collapsing to an empty selection at its own start rather than vanishing
from the set. The set must remain ordered and its primary must remain a member
(`normalize_set` is the single place that establishes this).

Acceptance evidence: a member with leading and trailing whitespace trims to its
content; a whitespace-only member collapses without removing a member; the
result is still normalized and the primary still names a member.

### Work package C — Join (`J`, `Alt-J`)

Intent: join the lines each member covers with a single space, removing the
newline and the following line's indentation.

Required changes: one verb plus a flag for the "select the inserted space"
variant, or two variants if that reads better at the call site. The join is one
transaction per gesture. A member that covers one line, or whose covered lines
contain no newline, is a no-op that reports nothing.

Acceptance evidence: joining two lines produces one line separated by one
space; the indentation of the joined line does not survive; `Alt-J` leaves the
inserted space selected and `J` does not; a multi-member join is one undo step;
a read-only attachment refuses both with a notice.

### Work package D — Indent and unindent (`>`, `<`), and the indent-unit decision

Intent: add or remove one indent unit at the start of every line each member
covers, and make the selection line-wise first, as Helix does.

Required changes: the verb, plus a **decision recorded in this plan and in the
code comment**: the indent unit comes from the document.

- **D1 — the indent-unit source.** Three candidates were considered.
  *(a)* Detect from the document: scan a bounded prefix of the file for the
  smallest positive leading-whitespace run, treat a tab-dominated file as
  tab-indented, and fall back to four spaces. *(b)* A fixed four spaces.
  *(c)* A new editor configuration key.
  **Recommendation: (a).** It needs no configuration-surface change, it matches
  what the file already uses, and the scan is bounded by an existing budget.
  (b) is rejected because it is wrong for every two-space file, which is most
  of this repository. (c) is the honest long-term answer and is **out of scope
  here** because adding an editor config key is a configuration-surface decision
  with its own key, default, discovery and documentation consequences.
  The implementer must record the chosen width and whether it was detected or
  defaulted in the notice path when a gesture produces no visible change.

Acceptance evidence: `>` on a two-space-indented document inserts two spaces and
on a tab-indented document inserts a tab; `<` removes exactly one unit and does
not eat content when a line is indented less than one unit; both make the
selection line-wise first; a multi-line selection indents every covered line in
one transaction; a read-only attachment refuses both with a notice.

### Work package E — Guard, documentation and reachability

Intent: make the binding set coherent in one change.

Required changes: update the key authority guard's expected Normal-mode count
and the doc-drift guard's expected figures; add the seven keys to
`docs/tui.md`; update the `space ?` keymap box; remove any "not bound" entry
that named them.

Acceptance evidence: `check_tui_editor_key_authority.py` passes and reports the
new count with no shadowed arm; `check_tui_editor_doc_drift.py` passes in both
directions; a dispatch-path test per chord proves each is reachable through
`editor_buffer_key` and not only through a helper — the predecessor's M001
finding was exactly that a helper-level test can cover a dead arm.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A verb resolves against one snapshot and submits one
  transaction. If the controller rejects it, the notice reports the rejection
  and the selection is left as it was; no half-applied indent is possible
  because the transaction is all-or-nothing.
- **Duplicate delivery.** None. These are synchronous local gestures.
- **Restart.** No state survives a restart, by construction: nothing is stored.
- **Cancellation races.** None; no async work is added. The existing rule that a
  completion for a closed or switched document is discarded is untouched
  because these verbs issue no requests.
- **Stale generations.** A verb reads the current snapshot; a stale snapshot is
  not possible through `try_snapshot()`.
- **Concurrent callers.** The editor is single-threaded over its presentation.
- **Bound reached.** An indent or join that would exceed `MAX_DOCUMENT_EDITS`
  is refused with the existing notice rather than truncated, matching
  `delete_ranges`' established rule.

## 9. Compatibility and migration

No storage, no protocol, no configuration. The only compatibility surface is the
**keymap itself**: seven keys that previously fell through to the composer now
belong to the buffer. That narrows the composer's keyspace on the editor route
and nowhere else, which is the documented and intended effect of adding a
binding. `docs/tui.md` records it in the same change.

Removal criteria for the old behaviour: none needed — the old behaviour was "the
key does nothing to the buffer".

## 10. Required tests

### Focused unit tests

- Each verb resolved directly against a `DocumentSnapshot`: line bounds, trim
  including the all-whitespace member, join with and without the space
  selection, indent and unindent including the under-indented line.
- The indent-unit detector: two-space file, four-space file, tab file, mixed
  file (asserts the recorded choice), and a file with no indentation at all
  (asserts the fallback).

### Integration tests

- One dispatch-path test per chord, pressing the key through
  `editor_buffer_key`/`handle_normal_key` and asserting the observable effect.
  A test that calls the resolver directly does not satisfy this.
- A read-only attachment test per writing verb asserting the notice and that
  the text is unchanged.

### Restart and recovery tests

None required. No durable state exists. The plan must state that explicitly in
its closure record rather than omitting the section.

### Contention and cancellation tests

- A multi-member indent and a multi-member join each assert **one** history
  entry, which is the observable form of "one undo step per gesture".

### Security and negative tests

- `scripts/check_tui_editor_text_authority.py` passes with no allowlist edit.
- A `>` whose transaction would exceed `MAX_DOCUMENT_EDITS` is refused with a
  notice and leaves the document byte-identical.

### Migration and compatibility tests

None. The doc-drift guard is the compatibility check for the keymap.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

`verify.sh quick` runs no tests; the test commands above are the test evidence
and `quick` is the guard evidence. Do not claim one as the other.

## 12. Documentation updates

- `docs/tui.md`: Normal-mode table gains `X`, `Alt-x`, `_`, `J`, `Alt-J`, `>`,
  `<`; the checked "not bound" list loses any entry naming them; the indent-unit
  behaviour and its detection rule are described where the other verbs are
  described.
- The editor's `space ?` keymap box follows the same table.
- The code comment on the indent-unit source states the choice and the fallback
  so a later reader does not re-derive it.

## 13. Acceptance criteria

- Pressing `X` in Normal mode widens every selection member to the lines it
  touches; pressing it again changes nothing and reports nothing.
- Pressing `Alt-x` narrows a line-wise selection to the line's own bounds.
- Pressing `_` trims leading and trailing whitespace from every member.
- Pressing `J` joins the lines a member covers with one space; `Alt-J` leaves
  the inserted space selected.
- Pressing `>` inserts one indent unit at the start of every covered line and
  makes the selection line-wise first; `<` removes one unit without eating
  content.
- Every one of the seven keys is reachable through the real dispatch path.
- On a read-only attachment, `X`, `J`, `Alt-J`, `>`, `<` and `_` are refused with
  a visible notice and the document is byte-identical afterwards.
- The key authority guard reports no shadowed arm, and the doc-drift guard
  passes in both directions.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The exact line-bounds boundary semantics cannot be pinned from the reference
  dialect, and the choice would change what `X` selects on a multi-line
  selection. Record the ambiguity and ask; do not pick and document.
- The indent-unit decision would require a configuration key. That is a
  configuration-surface decision outside this milestone; report and stop.
- A verb appears to need a retained document copy to compute its result. That is
  a text-authority violation; report rather than adding a cache.
- Any chord collides with an existing owner in a way the key authority guard
  reports as shadowing, and the fix would require re-spelling an existing
  binding. Re-spelling a binding a user already learned is an owner decision.
- The work expands into counts, formatting, or syntax-node objects.

## 15. Closure evidence required

- The key authority guard's summary line with the new Normal-mode count and no
  shadowed arm.
- The doc-drift guard's pass output, and a statement of which figures moved.
- One dispatch-path test per chord, named individually.
- The read-only refusal evidence per writing verb.
- The indent-unit decision as implemented, with the detector's test cases.
- A statement that no migration, protocol variant, crate, or new authority was
  added — or the finding that one was, which stops the milestone.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **The guard counts move.** Two guards hold expected figures and both must move
  in the same commit as the chords. Updating a guard after the fact is how the
  predecessor's M001 dead-arm defect survived.
- **`0` is `LineStart`.** This milestone adds no digits, but the next one (M004)
  will, and a reviewer should confirm no arm added here claims a digit.
- **`>` and `<` arrive as `Char('>')`/`Char('<')` with either `NONE` or
  `SHIFT`**, depending on the terminal. Both spellings must be accepted — the
  existing punctuation arms already follow that rule and cite it.
- **The selection set is shared with `/terminal`.** Any change to what `X` or
  `Alt-x` means must land in `helix_selection` or explicitly record why the
  terminal is exempt, or the two surfaces drift.
- **Serial test constraint.** No environment mutation is introduced, so the
  standard nextest profile `ci` applies; do not add a test that mutates process
  environment.
- **Preserve the user's working tree.** This milestone touches no generated
  asset and runs no generator.
