# TUI editor Helix parity Milestone 019 — the open-line verbs: `o` and `O`

Status: implemented - closed 2026-10-20
Closure record: `plans/closure/tui-editor-helix-parity/019-status.md`
Repository baseline: `8df5dbb32` (`feat/auto-20261018-12255c45`) — M018's commit, the
last closed milestone of the third wave

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (a new
milestone added by the M019-planning amendment; see the roadmap's Amendment A3) and
§11 (the completion definition this milestone serves)
Long-term requirements: `plans/000-long-term-specification.md` (the editor's keymap is
a contract, not an implementation detail)
Applicable ADRs: none — this milestone repairs a verb that is already bound
Primary class: capability

## 1. Objective

Make `o` and `O` **open a line**.

Today both keys enter Insert mode without opening anything. The code that would
open the line exists, is correct, and is unreachable.

## 2. The defect, in four facts

1. **The keys are bound.** `src/tui/commands/editor.rs:4292-4295` routes `o` to
   `EditorCommand::OpenLineBelow` and `O` to `EditorCommand::OpenLineAbove`.
2. **The route drops the edit.** `enter_insert` (`:4642`) maps the command to a
   cursor position through a match whose only arms are the four *insertion entry*
   commands; `OpenLineBelow` and `OpenLineAbove` fall to `_ => cursor`
   (`:4669`). The function is named for what it does — it moves the cursor and
   hands over the keyboard — and an open-line command has no cursor position of
   its own, so the match has nothing to say about it.
3. **The correct implementation is already written and unit-tested.**
   `editor::resolve_edit` has real arms for both commands
   (`src/tui/editor.rs:4744-4745`), delegating to `open_line_below` /
   `open_line_above` (`:4654`, `:4668`). `open_line_below_splits_the_existing_terminator`
   (`src/tui/editor.rs:7669`) and `open_above_the_first_line_preserves_a_leading_blank`
   (`:7682`) assert the text it produces. Those tests call `resolve_edit`
   **directly**, so they never touch the key path — the same helper-path-only
   shape the roadmap's §9 prohibits for a binding.
4. **The documentation already claims the fixed behaviour.** `docs/tui.md:1198`
   says *"`o` opens the line **and** hands you the keyboard, while `]Space`
   inserts the line and leaves the cursor exactly where it was"*. The code does
   not open the line, so the document and the implementation disagree today.

`EditorEdit::stay_insert` is documented as *"Whether the controller accepting this
edit should keep the editor in insert mode (**open-line commands**, insertion
entry)"* (`src/tui/editor.rs:1602`). The flag was designed for exactly this verb
and has never been used by it.

## 3. Why the repair is not a new mechanism

`resolve_edit`'s own doc comment already names the two halves of this verb
(`src/tui/editor.rs:4746-4749`): *"Helix's `add_newline_*` is the same insertion as
`open_*` with a different consequence, so both verbs share one implementation
rather than two arms that could drift: `stay_insert` is the only thing that
separates them, and that belongs to the caller that changes mode."*

The caller that changes mode is the key handler. It does not today.

## 4. Scope

**In:**

- the `o` and `O` arms, routed through the shared edit funnel;
- a new `editor_open_line` beside `enter_insert`;
- `submit_edit` reporting whether the controller accepted the transaction;
- the `stay_insert` comment in `submit_edit`, which today states an invariant
  this milestone makes false;
- dispatch-path tests;
- the documents that describe the verb.

**Out:**

- **`i`/`a`/`I`/`A`.** They are mode transitions, not writes, and they keep their
  own path. Their silent refusal on a read-only attachment is recorded as an
  unresolved finding (§9), not fixed here.
- **The `]Space`/`[Space` verbs.** They already share `resolve_edit`'s insertion
  and differ only in `stay_insert`; this milestone does not change them.
- **`open_line_below`'s document shape at the last line.** The function's text
  is pre-existing and its own doc comment states it deliberately; this milestone
  makes it reachable and does not redesign it. The function's *cursor* **is** in
  scope, because a cursor on the wrong line is the same defect the milestone
  exists to fix — see §12.1.
- **Any new chord.** The key guard's counts do not move.

## 5. Required production changes

### W1 — `submit_edit` reports acceptance

`fn submit_edit(state: &mut EditorState, edit: editor::EditorEdit)` becomes
`-> bool`: `true` when `controller.apply_local` accepted the transaction, `false`
when it refused.

This is the honest signal for *"the document took the keyboard"*. A mode
transition decided on a rejected transaction would put the editor in Insert mode
over an unchanged document — a worse state than the one the defect creates today.

### W2 — `editor_open_line`

A new function beside `enter_insert`:

```rust
fn editor_open_line(state: &mut EditorState, command: EditorCommand) -> BufferKey
```

It goes through the **shared edit funnel** — the same `editable` gate, the same
snapshot, the same `resolve_edit`, the same `submit_edit` — and returns:

| Outcome | Return | Why |
|---|---|---|
| the attachment cannot be written | `BufferKey::Consumed` | the verb is claimed and the refusal is **reported**, exactly as `d`/`D`/`x` do; `Unhandled` would type `o` into the composer |
| the controller accepted the edit | `BufferKey::EnterInsert` | the document took the keyboard, so the App adopts Insert mode |
| the controller refused the edit | `BufferKey::Consumed` | the error is already reported by `submit_edit`; the key is still claimed |

`editor_open_line` is **not** a second funnel. It shares `editor_editing_command`'s
gate and resolution and differs only in what it returns and in the mode it asks
for.

### W3 — the two arms

```rust
(m, KeyCode::Char('o')) if m == none => editor_open_line(state, EditorCommand::OpenLineBelow),
(m, KeyCode::Char('O')) if m == none || m == shift => {
    editor_open_line(state, EditorCommand::OpenLineAbove)
}
```

### W4 — the `stay_insert` invariant, restated

`submit_edit` writes `presentation.mode = EditorMode::Insert` when `stay_insert`
is true, under the comment *"Safe by construction, not by convention: `stay_insert`
is only ever true for an edit resolved while the model was already inserting, and
the route only enters Insert through `BufferKey::EnterInsert`."*

**That invariant becomes false with this milestone**: the open-line verbs resolve
an edit with `stay_insert` in **Normal** mode. The property the comment protects
— that `presentation.mode` and `App::ui_state.input_mode` agree — is preserved by
a different mechanism: `editor_open_line` returns `BufferKey::EnterInsert` **only
when the controller accepted the edit**, so the App adopts Insert mode in exactly
the case where the presentation already did. The comment is rewritten to say that.

### W5 — a typing run starts where the cursor ends, not where the edit ends

`submit_edit` folds an edit into the typing run when the editor is in Insert
mode, and `absorb_insert_edit` decides whether the run **continues** by asking
whether the next insertion is at the end of the run so far. The *first* edit
of a run had no such question asked of it: it opened the run wherever it
landed.

The open-line verbs are the first edit to need the question. `o` inserts its
newline in `NOR` and *then* takes the keyboard, so the mode afterwards is
Insert and the edit is folded into a run — and because its newline is not at
the end of that run, the first keystroke cannot continue it. The word the
reader typed was split across two entries, and the burst that M011 made one
step was two.

The question is now asked of the first edit too: **an edit starts a typing run
only when the cursor ends where the text it inserted ends.** The next
keystroke continues at the end of the run, so a run whose text does not end
where the cursor is cannot be continued — and the open-line edit, whose cursor
sits at the *start* of the line its newline ends, is not a typing edit at all.

The check is on the cursor, not on the mode. A mode check would also have
excluded the open-line edit, but it would have excluded the *literal*
insertion too, and the literal insertion in Normal mode is how the model's
own burst test drives it. The cursor check asks the question the run model
actually cares about, and leaves the literal insertion alone.

## 6. Required tests

Dispatch-path, per the roadmap's §9 — through `editor_buffer_key`, never through
`resolve_edit` directly:

1. `o` opens a line below the cursor's line and enters Insert mode.
2. `O` opens a line above the cursor's line and enters Insert mode.
3. `o` on a read-only attachment is refused: the document is unchanged, the key
   is claimed, and the refusal is reported.
4. The undo granularity of `o` followed by typing.
5. `]Space` still inserts a line without taking the keyboard — the regression
   guard for the shared `resolve_edit` arm this milestone touches.

## 7. Required verification commands

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib tui::commands::editor::tests::
cargo test -p codegg --lib tui::editor::tests::
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
bash scripts/verify.sh quick
```

## 8. Acceptance criteria

1. `o` in Normal mode opens a line below the cursor's line and enters Insert
   mode; `O` opens a line above. Asserted **through the key path**.
2. The cursor lands **on the line that was opened**, not on the line after it.
3. A read-only attachment refuses `o` and says so; the document is unchanged and
   the key does not reach the composer.
4. `]Space` and `[Space` are unchanged: the line is inserted and the keyboard is
   not taken.
5. The key guard's counts are unchanged: **2 / 71 / 8** and **5 / 44 / 12**.

## 9. Stop conditions

Report rather than improvise if:

- routing `o`/`O` through the edit funnel would move a chord the key guard
  counts;
- the open-line edit cannot be applied without a second document-write path;
- the App's input mode and the presentation's mode would disagree after the
  change — that is the one property the `stay_insert` write is about, and a
  milestone that breaks it has replaced one defect with a worse one.

## 10. Closure evidence required

- the five dispatch-path tests, with their assertions;
- the key guard's and the leaf guard's counts, before and after;
- the whole-lib sweep's result, stated truthfully including any failure that is
  not this milestone's.

## 11. Handoff notes

- This milestone **repairs** a documented behaviour. `docs/tui.md` already says
  `o` opens the line; after this milestone the code agrees with the document.
- The `o`/`O` arms keep their chords, so the key guard's Normal-mode count stays
  at 71.
- The `i`/`a`/`I`/`A` refusal finding is recorded, not fixed. It is a different
  verb family and a different question: a mode transition is not a write.

## 12. Deviations from this plan

Three, all forced by evidence the plan did not have when it was written. Each
is recorded here rather than left implicit, and each was measured by a test
rather than reasoned about.

### 12.1 The open-line edit's cursor was one byte past the opened line

**Planned:** the pre-existing `open_line_below` is reused unchanged.

**Found:** its cursor was `insert_at + 1` in both cases, which is right only
when there is no terminator to split. With a terminator, the insertion is one
newline at the start of the *next* line and that newline is the one that ends
the opened line, so the opened line starts where the insertion did. The
pre-existing cursor therefore landed on the **next** line, and the reader's
typing went there — `o` followed by "xy" produced `one\n\nxytwo\n`.

**Done:** the cursor is now the start of the opened line in both cases —
`insert_at` when a terminator was split, `insert_at + 1` when a full blank line
was appended. `open_line_below`'s doc comment states the geometry.

**Consequence:** `open_and_add_newline_agree_on_text_and_differ_on_mode`, which
pinned the old value, now asserts 4. Its *reason* ("on the line `o` opened")
was already the truth; the number was not.

### 12.2 `o`/`O` claim the key on a read-only attachment; `i`/`a`/`I`/`A` do not

**Planned:** the plan's §5 W2 said a read-only refusal returns
`BufferKey::Consumed`, on the grounds that an unclaimed `o` would fall into the
composer.

**Found:** `every_insertion_entry_is_reachable_through_dispatch` asserted
`Unhandled` for `i`, `o` and `A` alike — the *mode-entry* contract — so the two
verb families were covered by one assertion that could only hold for one of
them.

**Done:** the families are separated. `i`/`a`/`I`/`A` are mode transitions: they
do not write, so they leave the key unclaimed and the status row's "read-only"
is the report. `o`/`O` write, so they claim the key and report the refusal as a
notice, exactly as `d`/`D`/`x` do. Both families keep the property the old test
was really asserting: the keyboard is not handed over.

**Recorded, not fixed:** the `i`/`a`/`I`/`A` refusal is *visible* (the status
row) rather than silent, and `enter_insert`'s doc comment said so before this
milestone. The plan's first draft of that comment called it silent; the
comment was wrong and has been corrected.

### 12.3 `o` and the typing that follows it are **two** undo steps

**Planned:** one undo step, on the expectation that `o` plus the word behaves
like a single burst.

**Found:** two problems, and the second only appeared once the first was fixed.

*The insert session is a typing run.* M012's model is that everything typed in
one go in `INS` is one entry, and the `o` is not typed — it is an edit made in
`NOR` that then hands the keyboard over.

*The geometry forbids merging them.* The open-line edit inserts its newline at
the **end** of the opened line while the cursor sits at the **start** of it, so
the typing goes in before the newline, and a run is one contiguous insertion by
construction.

And then the third problem, which is the one that made the measured behaviour
*three* steps rather than two: `submit_edit` decided whether to fold an edit
into the typing run by looking at the editor's mode **after** the edit. The
open-line edit sets that mode to Insert (it takes the keyboard), so the edit
was folded into a run it does not belong to — and because its newline is not at
the end of the run, the *first keystroke* could not continue the run, so the
burst was split in two.

**Done:** the first edit of a run is now asked the question the run model
already asked of every later edit — *does the cursor end where the inserted
text ends?* — and the open-line edit, whose cursor sits at the start of the
line its own newline ends, is not a typing edit. It is its own undo step; the
typing that follows opens the run, and the word is one step however many
keystrokes it took.

**Not decided here:** whether a reader would rather have one step. Making it
one would need the open-line edit and the typing to merge into a single
contiguous insertion, and they are not contiguous — the newline the `o` opened
the line with sits *after* the text the reader types. That is a property of
the geometry, not of the history model, and it is recorded as a residual rather
than taken.
