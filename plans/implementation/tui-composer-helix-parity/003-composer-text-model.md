# TUI Composer Helix Parity Milestone 003 — Composer Text Model

Status: implemented

Repository baseline: `2b44be918` (branch `feat/auto-20261019-8cc543fb`)

Source roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md#m003--composer-text-model`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — the
  composer's claim set. Unchanged by this milestone: it adds no verb and claims no key.

Primary class: infrastructure (the composer's model), with the capability it enables
(the composer's word motions and selection verbs) belonging to M004 and M005.

## 1. Objective

Give the composer a **text model**: a point type that satisfies the shared selection
contract, a selection state held by the shared state machine, a bounded undo/redo history, and
word/line geometry computed by a rule shared with the editor rather than reimplemented.

## 2. Why this milestone is ready

- **No hard dependencies.** M001 and M002 are closed. M003's only hard prerequisite is M002
  (the claim table it will eventually serve), and the corrective C001 has closed on top of it.
- **Stable interface dependencies.** `src/tui/helix_selection.rs` already owns the shared
  selection state machine and its `SelectionPoint` contract, and
  `src/tui/editor.rs` already owns an `EditorPoint` that satisfies it. The composer's point
  type has a shape to copy, not a contract to invent.
- **Independently shippable.** It adds no key handling and no rendering change, so it cannot
  change what the user sees. Everything it adds is exercised by its own tests.
- **The decision is settled.** The roadmap's §5 already names the shape: a composer point type
  satisfying the shared selection trait, the composer's selection state, a bounded undo/redo
  edit log, and word/line geometry over a shared text source with the editor migrated onto it.

## 3. Current implementation evidence

At the baseline:

- `PromptWidget` held `pub text: String` and `pub cursor: usize` and did the buffer arithmetic
  itself — `insert_char`, `backspace`, `delete`, `cursor_left`, `cursor_right`, `paste`, and
  friends. There was no model, no selection and no history.
- The editor owned its own word geometry: `word_forward`, `word_backward`, `word_end`,
  `move_vertical`, `clamp_offset`, `next_char_boundary`, `prev_char_boundary`, `cursor_line`,
  `line_start`, `line_content_end` and `buffer_content_end`, all over a `DocumentSnapshot`, plus
  a private `is_word_byte` policy.
- `src/tui/helix_selection.rs` held the shared selection state machine. Its tests ran against
  a test-only `Cell` point — the rules were checked against **one** point type, so a second
  surface could claim to use the machine without being checked against it.
- `src/tui/interactive_terminal.rs` had its own word rule (`next_word_end`,
  `previous_word_start`), which is **whitespace-delimited** and works in viewport columns.
- `src/tui/editor.rs` already had an `EditorPoint { byte, line }` implementing `SelectionPoint`
  with `Line = usize` — the same shape the composer needs, and the reason the composer's point
  type is a copy of a shape rather than an invention.

## 4. Invariants that must not regress

- **I1 — the claim table is unchanged.** M003 adds no verb, claims no key, and changes no
  claim's status. The table's 13 rows and their encodings stand.
- **I2 — a claimed key never falls through.** Unchanged; no key handling is added.
- **I3 — the composer is a frontend-local string.** The model holds no daemon-authorized
  state, no revision and no document. It is delivered only on submit.
- **I5 — the resolved mode is the gate.** Unchanged; the model has no opinion about modes.
- **I7 — one redraw cadence.** Unchanged; nothing here touches rendering or timing.
- **The editor's behaviour is unchanged.** The migration is a move, not a rewrite: the editor's
  public geometry functions keep their signatures and their results.
- **The composer renders exactly as before.** The widget's rendering and its public API are
  unchanged; the buffer moved behind the model.
- **The terminal panel is unchanged.** Its word rule is a different rule on purpose, and
  migrating it would change what it selects.

## 5. Scope

**In scope.**

- A shared text-geometry module: a `TextSource` trait, one word policy, and the word/line
  motions over it.
- The editor migrated onto that module, with its public API preserved.
- The composer's model: a point type, a selection state, a bounded undo/redo history, and the
  buffer arithmetic that used to live in the widget.
- `PromptWidget` rewired onto the model, with its public API preserved.
- The shared selection rules made generic and run against the composer's point type as well as
  the terminal's.

**Out of scope.**

- Any key handling. `w`, `b`, `e`, `v`, `x`, `d`, `p`, `u`, `U` remain claimed and reserved;
  M004, M005 and M006 own them.
- Any rendering change. The composer draws what it drew.
- The interactive terminal panel's word geometry. Its rule is whitespace-delimited and its
  coordinates are viewport columns; migrating it would change what it selects, which is not
  this milestone's to decide.
- The editor's own selection state, which is a byte-offset cursor in a replica and is
  explicitly documented as not moving wholesale into the shared type.
- The command palette's filter charset, which is a filter rule and not a word rule.
- M004's motions, M005's selection and clipboard verbs, M006's undo/redo and insert
  transitions, and M007's consistency sweep.

## 6. Required production changes

- **`src/tui/text_geometry.rs` (new).** The `TextSource` trait; the word policy
  (`is_word_byte`); the terminal panel's token rule beside it, named for what it is
  (`is_whitespace_token_char`); the word motions (`word_forward`, `word_backward`, `word_end`)
  and line motions (`move_vertical`, `cursor_line`, `line_start`, `line_content_end`,
  `buffer_content_end`, `clamp_offset`, `next_char_boundary`, `prev_char_boundary`) over a
  source; and adapters for `DocumentSnapshot` and `str`.
- **`src/tui/composer_text.rs` (new).** `ComposerPoint`; `ComposerText` (buffer, caret,
  selection state, history); `Edit`; `UndoHistory` (bounded in depth and in bytes).
- **`src/tui/editor.rs`.** The private `is_word_byte` and `byte_at` deleted; the public
  geometry functions delegate to the shared module; `EditorPoint` and the editor's own
  selection handling untouched.
- **`src/tui/components/prompt.rs`.** The `text` and `cursor` fields replaced by the model;
  every public method preserved.
- **`src/tui/helix_selection.rs`.** The test module's rules made generic over the point type
  and run against the composer's point as well as the terminal's.
- **`src/tui/mod.rs`.** The two new modules registered.

## 7. Ordered work packages

**WP1 — The shared text source and the word policy.** The trait, the policy, the motions, the
adapters. The policy is the part that matters: it is the tree's word-segmentation rule, and
the editor's private copy is deleted in the same pass so there is exactly one.

**WP2 — Migrate the editor onto the shared module.** The editor's public functions become
thin delegations. The public API is unchanged, so no caller moves.

**WP3 — The composer's model.** `ComposerPoint` implements `SelectionPoint` with `Line = usize`,
matching `EditorPoint`'s shape. `ComposerText` owns the buffer, the caret (as the shared
machine's cursor, not a second copy of it), the selection state, and the history.

**WP4 — The bounded history.** Two stacks, a depth bound and a byte bound. Eviction removes the
oldest undo entry, and it is only safe because a new edit clears the redo stack first — that
invariant is stated in the code and tested.

**WP5 — Rewire the widget.** The widget's public API is preserved exactly; the buffer moves
behind the model. `set_text` puts the caret at the end of the new text, which is where the
widget put it before, and clearing the buffer drops the history, which is what makes "submitting
the composer clears the undo history" true.

**WP6 — Make the shared rules generic.** The rules move into a `rules` module, generic over a
surface, and the test module runs them against the viewport cell **and** the composer's point.
A macro keeps one test per rule per surface, so a failure still names the rule.

**WP7 — Verify.** The geometry's own tests, the model's own tests, the shared rules against
both surfaces, the editor's suites, the composer's suites, and the workspace gates.

## 8. Failure, cancellation, restart, contention semantics

The model is a frontend-local value with no I/O, no background task and no durable state, so
there is nothing to fail, cancel or contend over. The editor migration changes no timing: the
shared `DocumentSnapshot` adapter calls the same snapshot methods the editor called, and
`line_of` uses the snapshot's indexed `byte_to_position` rather than a scan.

The one behavioural subtlety in the migration is the editor's word scan, which reads one byte
per step through `read_bytes`. The shared module keeps that exact behaviour for a
`DocumentSnapshot` (its `byte_at` default reads a one-byte range), so the migration is a move
rather than a rewrite. The composer, whose buffer is a `String`, gets an allocation-free
`byte_at` instead.

## 9. Compatibility and migration

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** The composer renders and behaves exactly as before: the widget's public
  API is unchanged, and every method keeps its semantics. The editor's public geometry
  functions keep their signatures and their results.
- **The editor's callers.** `clamp_offset`, `next_char_boundary`, `prev_char_boundary`,
  `cursor_line` and `line_start` are used outside the editor module; they keep their names and
  their behaviour.
- **The terminal panel.** Untouched. Its word rule is a different rule and is now named as one
  in the shared module, so the tree has one module that owns word classification.

## 10. Required tests

- `src/tui/text_geometry.rs`: a `String` and a `DocumentSnapshot` of the same text agree on
  every word boundary and every line fact; the word motions find the expected boundaries; an
  underscore is word content; a multi-byte character is never split; vertical movement keeps
  the column where it can.
- `src/tui/composer_text.rs`: a point carries its line; the caret sits where the text was
  typed; undo and redo are inverse; undo walks back through several edits; replacing or
  clearing the text drops the history; the history is bounded in depth and in bytes; an
  eviction does not strand a redo; a selection deletion is undoable; the composer uses the
  shared word geometry and agrees with the editor on it.
- `src/tui/helix_selection.rs`: every shared rule, against the viewport cell and against the
  composer's point.
- `tests/tui_composer_claims.rs` and `tests/tui_composer_cursor.rs`: unchanged and passing —
  they are the composer's existing behaviour, and the model must not have moved it.
- The editor's own suites, unchanged and passing.

## 11. Required verification commands

```bash
cargo test --lib tui::text_geometry
cargo test --lib tui::composer_text
cargo test --lib tui::helix_selection
cargo test --lib tui::editor
cargo test --test tui_composer_claims --test tui_composer_cursor --test tui
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the composer's model: the shared geometry, the point type, the
  bounded history, and the rule that the editor and the composer share one word policy.
- `src/tui/text_geometry.rs` and `src/tui/composer_text.rs` — the module docs, which are the
  authoritative statement of what the model is and is not.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — M003's status and the §4 current
  state, which describes the composer as having no model.
- `plans/registry.md` — M003's row and the dependency audit.

## 13. Acceptance criteria

- **AC-M1** — the composer has a point type that satisfies the shared selection trait, and the
  shared selection rules pass against it, rule for rule, alongside the terminal's.
- **AC-M2** — the composer's selection is held by the shared selection state machine, not by a
  second implementation of the growth rule.
- **AC-M3** — the word boundaries the composer computes are identical to the editor's for the
  same text, and the shared policy function is the only word-segmentation rule the editor and
  the composer use.
- **AC-M4** — the undo history is bounded in depth and in bytes, and a bounded eviction does
  not corrupt the history.
- **AC-M5** — undo and redo are inverse operations over the composer's buffer, including over
  a selection deletion.
- **AC-M6** — submitting or replacing the composer clears the undo history, so an undo cannot
  resurrect text that was already sent.
- **AC-M7** — the composer's rendering and behaviour are unchanged: the widget's public API is
  intact and its existing test suites pass.
- **AC-M8** — the editor's public geometry API is intact, its results are unchanged, and its
  suites pass.
- **AC-M9** — no key handling and no rendering change: the claim table is untouched and the
  composer's drawing is untouched.
- **AC-M10** — the workspace gates are green.

## 14. Stop conditions

Report rather than improvise if:

- The editor's public geometry API cannot be preserved while the implementation moves — the
  callers outside the editor module are the constraint, not the internals.
- Making the shared rules generic would change a rule's meaning for an existing surface. The
  rules are the contract; a surface that needs different rules needs a different contract,
  and that is a decision to record, not to improvise.
- The composer's model cannot preserve the widget's current behaviour exactly — the widget's
  existing suites are the specification here, and "no rendering change" means no behaviour
  change either.
- The terminal panel's word rule turns out to be the same rule after all, in which case it
  belongs in the migration and the milestone's scope changes; report it rather than widening
  the change quietly.
- Any verification command fails for a reason this milestone did not cause: report the failure
  and its evidence rather than widening the change.

## 15. Closure evidence required

- The focused test runs, with their counts.
- `scripts/verify.sh quick` output.
- The clippy gate on both the repository's minimum toolchain and the CI toolchain.
- A requirement-to-evidence matrix mapping AC-M1…AC-M10 to the evidence above.
- The registry update and the dependency audit.

## 16. Handoff notes

- The composer is a frontend-local string. Nothing here touches the daemon, the storage layer
  or the protocol, and the model holds no daemon-authorized state.
- `PromptWidget` keeps its public API on purpose: there are 183 call sites across the app that
  use the accessors, and a milestone that renames them would be a milestone about renaming.
- The model is the substrate M004–M006 stand on. It has no keys of its own, and adding one
  would be two owners for a key.
- The undo history is bounded in **both** depth and bytes on purpose: a depth bound alone
  still lets one enormous paste sit in the history, and a byte bound alone still lets a
  thousand one-character edits sit in it.
- Grouping consecutive typing into one undo step is not done here. It is a behaviour question
  with a UX answer, and M006 owns the undo verbs.
