# TUI Composer Text Layout Milestone M001 — Composer Word Wrap and Visual-Row Geometry

Status: implemented — closure record at `plans/closure/tui-composer-text-layout/001-status.md`

Repository baseline: `f2639b5d2` (`origin/main`)

Source roadmap: `plans/subsystems/tui-composer-text-layout-roadmap.md#milestone-m001--the-composer-word-wraps-grows-and-scrolls`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted**
  2026-10-19) — the composer's claim set, the mode gate, the fall-through
  rule and the frontend-local-string boundary. **This milestone changes no
  part of that decision.** It changes the composer's *layout*, and the
  condition on the `j`/`k` claim is M002's business, together with ADR-0042's
  dated amendment.

Primary class: capability (roadmap C1, C2, C3) with an infrastructure
obligation (roadmap IN1)

## 1. Objective

Make a long prompt legible in the box it is typed into: it word-wraps, the box
grows to fit it up to ten rows, and the content beyond that cap is reachable
by scrolling.

## 2. Why this milestone is ready

- **The main-buffer roadmap's M001 (interface)** supplies the styled,
  word-aware, byte-offset-preserving primitive this milestone consumes. That
  milestone's plan **fixes the primitive's signature**, so this milestone is
  written against a settled contract rather than an open shape.
- **The width at height-computation time is available.** The session layout
  splits the pane vertically, so the prompt chunk's width is the pane's width
  and is known **before** the height is computed. The height/width dependency
  is not circular, which was the risk that could have made a wrapped
  composer unimplementable.
- **The composer's text model is settled.** `ComposerText` holds a byte
  offset, a selection and an undo history; the shared word and vertical
  geometry lives in `src/tui/text_geometry.rs`. This milestone adds the
  **visual-row** layer on top and changes neither.

## 3. Current implementation evidence

Verified at baseline `f2639b5d2`.

### 3.1 There is no wrapping, and the paragraph is clipped

The composer's render builds one `Line` per **logical** line and paints them
with a paragraph that has **no wrap**. A ratatui paragraph without a wrap
truncates each line at the area width, so a long prompt is cut at the right
edge.

The only recourse is a horizontal scroll, which is the paragraph's x offset
and therefore pans **every** line together. It is not a per-line view and it
does not wrap.

### 3.2 The height is logical, and its ceiling is a pane budget

The height is one mode row, plus the number of **logical** lines, plus a
character-count row and a multiline-marker row, clamped to a minimum of three
rows and a maximum of **40 % of the pane's height**.

So a 500-character single-line prompt is a three-row box with its tail
invisible, and the maximum is a share of the pane rather than a number of
rows. The 40 % budget is load-bearing — the main content has a minimum — and
the 10-row cap is a separate, new bound.

### 3.3 The geometry is logical everywhere

The height, the scroll clamp, the caret's visibility, the caret's cell and the
vertical motions all count logical lines. The caret's column is derived from
the byte offset within its logical line, and the horizontal scroll is applied
to the whole paragraph.

### 3.4 The vertical motions move by logical line

The composer's `move_vertical` moves the caret by a number of **logical**
lines, preserving the column where the target line allows and clamping at the
last line. It has no desired-column carry across a wrap, because a wrap does
not exist yet.

### 3.5 The existing tests assert the logical-line contract

The composer's render tests assert the logical-line height, the height clamp,
the scroll clamp, the horizontal cursor visibility, and the multi-line
behaviour. Each is a test of the *current* contract, and each must be
reviewed one by one: rewritten to the visual-row contract, or kept as the
logical-line contract it actually tests.

### 3.6 The horizontal scroll's remaining role

With wrapping, a horizontal scroll is needed only for a single token wider
than the row. Whether it stays, and under what condition, is P1 and this
milestone must state it rather than leave a dead field in place.

## 4. Invariants that must not regress

- **I1 — the composer is a frontend-local string.** A layout change touches
  the model's buffer and nothing else: no daemon call, no revision, no
  document, no submission.
- **I2 — the claim set is fixed.** ADR-0042 part 2 fixes the composer's
  claimed letters. This milestone adds no row, removes no row, and claims no
  new letter. **`j`/`k` keep ADR-0042 part 4's condition in this milestone**;
  M002 changes it.
- **I3 — a claimed key never falls through.** Unchanged.
- **I4 — the resolved mode is the gate.** Unchanged.
- **I5 — the caret is on the byte it names.** The byte-offset ↔ visual-row
  mapping is a bijection. The closure evidence is a property over widths and
  texts, not a set of examples.
- **I6 — the pane's budget wins.** The composer never takes more than the
  40 % budget allows, whatever the 10-row cap says.
- **I7 — one redraw cadence.** No second frame clock, no second spinner.

## 5. Scope

### In scope

- The composer renders **wrapped rows**, word-wrapped by the shared primitive.
- The height is computed from the **wrapped row count** at the pane's width,
  plus the mode row and the marker rows, capped at **10 rows in total**, and
  still bounded by the 40 % pane budget.
- The scroll offset is a **visual row** offset.
- The caret's drawn cell is the (row, column) of the caret's **byte**.
- Scrolling keeps the caret visible.
- The byte-offset ↔ visual-row mapping, as a small module with one
  responsibility.
- The disposition of the existing logical-line tests.

### Explicitly out of scope

- The `j`/`k` claim condition and the page motions (M002).
- ADR-0042's amendment (M002).
- The composer's text model, selection, clipboard and undo.
- The main buffer's rendering.
- The editor's soft wrap.
- A configuration key for the row cap (roadmap §4).
- Changing the 40 % pane budget.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

**The visual-row geometry module.** A small module that, given the wrapped
rows and the pane's width, answers:

- which row and column is a given byte on;
- which byte is at a given row and column;
- how many rows the content occupies.

It is the single place where "line" stops meaning "newline-separated
segment". It is **infrastructure**: this milestone does not close on it; it
is what makes C1–C3 possible.

**The height.** Computed from the wrapped row count:

```text
height = 1 (mode row)
       + min(wrapped_rows, 10 - 1 - marker_rows)
       + marker_rows
```

and then clamped to the 40 % pane budget. The 10-row cap is a cap on the
box's **total** rows — the mode row, the text rows and the marker rows
together — because that is what the owner decided. The budget still wins: on
a 15-row pane the budget is 6 rows and the box takes 6.

The wrap width is the **pane's** width, not the not-yet-existing chunk's
width. The prompt chunk spans the pane's width, so the pane's width is the
correct input and the dependency is not circular.

**The scroll offset.** A visual row offset, clamped against the wrapped row
count rather than the logical line count.

**The caret's cell.** The (row, column) of the caret's byte, in the wrapped
layout. The column is the offset within the **wrapped row**, not within the
logical line.

**The caret's visibility.** The existing "ensure the caret is visible" call
becomes a visual-row operation, so a caret on a wrapped row scrolls the
composer to that row.

**The vertical motions.** This milestone does **not** change them (M002 does),
but it must leave them compiling and behaving as they do against the logical
model until then — or, if the visual-row geometry makes the logical model
unimplementable, record that as a finding and stop. The plan's preference is
that the logical motions continue to work against the logical model and M002
moves them to the visual model.

### Security and authorization

None. The composer is a frontend-local string (I1).

### Documentation and static guards

- `architecture/tui.md` — the composer's layout contract: the wrap rule, the
  height formula and its two bounds, the scroll offset, and the caret's cell.
- `docs/tui.md` — the box's growth, the cap, and the scrolling.
- `.opencode/skills/tui/SKILL.md` — the same, if it describes the composer.
- **No new static guard.** The composer key-authority guard is untouched by
  this milestone, because this milestone changes no claim (I2).

## 7. Ordered work packages

### Work package A — The geometry module

Intent: one module owns the byte ↔ (row, column) mapping.

Required changes: add the module; unit-test it against an empty buffer, a
buffer ending in a newline, a buffer whose content is wider than the row, a
buffer with a token wider than the row, and multi-byte characters.

Acceptance evidence: the mapping's tests pass, and the mapping is a bijection
over the tested cases — every byte is on exactly one cell, and every cell
holds exactly one byte.

### Work package B — The height

Intent: the box grows with the content, stops at ten rows, and the pane's
budget still wins.

Required changes: compute the height from the wrapped row count at the pane's
width; apply the 10-row total cap; apply the 40 % budget clamp.

Acceptance evidence: one row of text is one row of box; the box grows with
the content; the box stops at 10 rows; a 15-row pane's box is bounded by its
budget rather than by the cap.

### Work package C — The render and the caret

Intent: the composer paints wrapped rows and the caret is on the right cell.

Required changes: render the wrapped rows; draw the caret at the geometry
module's answer; make the caret's visibility a visual-row operation; replace
the horizontal-scroll behaviour for the wrapped case.

Acceptance evidence: a prompt wider than the box wraps at word boundaries and
every character is reachable; the caret's drawn cell is the cell of its byte
across the tested widths and texts; scrolling keeps the caret visible.

### Work package D — The existing tests and the documents

Intent: no test asserts a contract that is no longer true, and the documents
describe the delivered layout.

Required changes: review each existing logical-line composer test; rewrite it
to the visual-row contract or keep it as the logical-line contract it
actually tests, and record each disposition; update `architecture/tui.md`,
`docs/tui.md` and the TUI skill; verify every count they state against its
listed source; state P1's disposition of the horizontal scroll.

Acceptance evidence: the disposition list is recorded in the closure record;
the documents describe the delivered behaviour.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. The composer is frontend-local; there is no concurrent writer,
no delivery to reconcile and nothing to recover.

A layout that cannot be computed must fall back to the current behaviour
rather than dropping text: a prompt that cannot be wrapped is painted as it
is today, clipped, not blank. The failure mode is "no worse than today",
never "the prompt vanished".

## 9. Compatibility and migration

- **Rendering only.** No stored state, no protocol, no migration.
- **No key changes** (I2). The composer's keymap is untouched.
- **No submission changes.** The prompt text, the newline character and the
  submit path are untouched; only the layout of the box changes.
- **The existing logical-line tests** are the compatibility surface; each
  disposition is recorded.

## 10. Required tests

### Focused unit tests

- The geometry module: byte ↔ (row, column) for an empty buffer, a buffer
  ending in a newline, content wider than the row, a token wider than the
  row, and multi-byte characters; the bijection property over each.
- The height: one row of text is one row of box; the box grows with the
  content; the box stops at 10 rows total; the 40 % budget binds on a short
  pane; the marker rows are inside the 10.
- The wrap: word boundaries; a token wider than the row hard-broken; the
  escape-sequence exclusion inherited from the shared primitive.
- The caret: the drawn cell is the cell of the byte, across widths and texts.

### Integration tests

- A prompt longer than the box's width renders fully visible across more
  than one row, and every character of it appears.
- A prompt longer than ten rows renders ten rows and scrolls.
- The caret stays visible after a scroll.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The escape-sequence exclusion holds in the composer's render (I5).
- The composer remains a frontend-local string: the milestone changes no
  submission path (I1).

### Migration and compatibility tests

None.

### Non-vacuity

Every new test must fail against the baseline tree: against the baseline, a
prompt wider than the box is **clipped**, not wrapped.

## 11. Required verification commands

```bash
# the geometry module and the composer widget, narrowly
cargo test --locked -p codegg --lib tui::components::prompt
cargo test --locked -p codegg --lib tui::text_geometry
cargo test --locked -p codegg --lib tui::composer_text

# the rendered composer
cargo test --locked -p codegg --test tui_render

# the composer claim guard, unchanged by this milestone
python3 scripts/check_tui_composer_key_authority.py --self-test

# static guards that cover the new tests
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and the workspace check
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked

# the routine gate
bash scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the composer's layout: the wrap rule, the height
  formula and its two bounds, the scroll offset, the caret's cell, and the
  geometry module's ownership.
- `docs/tui.md` — the box's growth, the ten-row cap, and the scrolling.
- `.opencode/skills/tui/SKILL.md` — if it describes the composer's layout.
- `plans/registry.md` — this milestone's row, when it closes.

## 13. Acceptance criteria

- A prompt wider than the box wraps at word boundaries, and every character
  of it is visible or reachable.
- The box grows with the content and stops at ten rows **in total**,
  counting the mode row and the marker rows.
- On a pane whose 40 % budget is below ten rows, the budget binds.
- The caret is drawn on the cell of its byte, for the tested widths and
  texts.
- Scrolling the composer keeps the caret visible and reaches the content
  beyond the ten-row cap.
- No key binding changes, and the composer's claim guard is unchanged.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the wrap width cannot be derived from the pane's width, and the
  height/width dependency turns out to be circular after all;
- the byte-offset ↔ visual-row mapping cannot be made a bijection without
  changing `ComposerText`'s caret model, which would break I1 and I5;
- the logical-line vertical motions cannot be left working against the
  logical model, and moving them would pull M002's work into this milestone;
- the 10-row cap cannot be applied without changing the 40 % budget, which
  is not this milestone's to spend;
- scope would expand into the disclosure model or the main buffer.

## 15. Closure evidence required

- The geometry module's unit tests, including the bijection property.
- The height's unit tests, including the ten-row cap and the budget
  interaction.
- An integration assertion that a long prompt is fully visible or reachable,
  and that the caret's drawn cell matches its byte.
- The disposition of **every** existing logical-line composer test — rewritten
  or kept, with the reason.
- P1's disposition: whether the horizontal scroll stays, and under what
  condition.
- A statement that no key binding changed and that the composer's submission
  path is untouched (I1, I2).
- `scripts/verify.sh quick` green at the closing commit.

## 16. Handoff notes

- The wrap width is the **pane's** width. Using the chunk's width is the
  circular mistake this milestone is exposed to.
- The caret's cell is the part most likely to be wrong by one row. The
  property test over widths and texts is the evidence; examples are not.
- M002 changes the `j`/`k` condition and the page motions. Leaving the
  logical-line vertical motions working in this milestone keeps that seam
  clean — do not pull M002's work forward.
- The composer's render runs on the redraw path. Measure the cost of the
  wrap; do not assert it.
