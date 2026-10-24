# TUI Composer Helix Parity Post-Closure Corrective Milestone 001 — The Caret Tells the Truth

Status: ready

Repository baseline: `c2d781319` (`origin/main`)

Source addendum: `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md#c001--the-caret-tells-the-truth`

Parent roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19) — governs the
  composer's claim set and its Normal-mode routing. **This milestone does not change that
  decision.** The caret's geometry and its paint are outside the ADR's scope.

Corrects: the closed milestone **M001** (composer keyboard ownership and cursor presence), whose closure record
is `plans/closure/tui-composer-helix-parity/001-status.md`. M001 closed on the caret's *presence* —
that a cursor is drawn and that the terminal's own cursor is positioned in Insert mode — and both of
those are true. The two properties M001 did not state are the ones this milestone corrects: that the
caret's **cell** advances with the caret's byte, and that the painted block is **visible**.

Primary class: capability (the caret advances one column per keystroke; the Normal-mode caret is
visible), with two invariants (the caret names a source position; a painted caret is visible).

## 1. Objective

Two defects in the same surface, both about the caret:

1. In **Insert** mode the caret's *cell* does not move when a space is typed. The caret's byte
   advances in the model; the caret's column does not, because the wrap drops the whitespace and
   the caret's fallback column is derived from the drawn cells alone.
2. In **Normal** mode the block that *is* the caret is painted with `Modifier::REVERSED`, which
   the terminal applies as a colour swap and thereby cancels the block.

The milestone ends with a composer whose caret advances one column per keystroke — spaces
included — and whose Normal-mode block is visible on every cell it can land on.

## 2. Why this milestone is ready

- **No hard dependencies.** M001–M007 and the first corrective are closed. The change touches one
  geometry module, one widget's paint, the composer's own unit and render tests, and no claim
  table, no binding map and no state.
- **The defect is measured, not inferred.** The behaviour was confirmed against the baseline
  tree with a temporary probe over `VisualLayout` (removed again; the tree was clean before this
  milestone began). The probe printed, for the buffer `"hello "`:
  `row 0 cells=[(0,1,0,1) … (4,5,4,1)]`, `caret 5 -> Some((0, 5))`,
  `caret 6 -> Some((0, 5))` — **the same column**. For `"hello  "`: `caret 5/6/7 -> (0, 5)`,
  all three identical. For `"hello world"`: every byte advances the column by one.
- **The fix's rule is derivable, not invented.** The composer already has a convention for a
  whitespace run — a run is **one column**, whether it is drawn (`src/tui/wrap.rs:659-671`) or
  collapsed onto one cell (`src/tui/composer_geometry.rs:476-488`). The correction applies that
  convention to the run the wrap drops.
- **Independently shippable.** It changes what the caret does, not what any key does. C002 and
  C003 are unaffected by it and do not depend on it.
- **The editor is not involved.** The editor owns its own caret (`src/tui/components/editor.rs:1809-1819`)
  and is not touched.

## 3. Current implementation evidence

### 3.1 The space

| Site | Fact |
|---|---|
| `src/tui/wrap.rs:615-618` | The wrap's contract: trailing whitespace is dropped. |
| `src/tui/wrap.rs:481-488`, `:514-515` | A whitespace run is parked and flushed with no word following it, so a run at the end of the buffer is never attached to a slice. |
| `src/tui/wrap.rs:659-671` | A run is drawn only when a word follows it, and it is drawn **collapsed to one column**. |
| `src/tui/composer_geometry.rs:176-196` | `caret_position`: a byte on a cell is that cell; a byte just after a line break is the next row's column 0; any other byte is "one column past the last character before it". |
| `src/tui/composer_geometry.rs:219-235` | `last_cell_before` returns the last cell that *starts* before the byte; the fallback returns `cell.column + cell.width`. |
| `src/tui/composer_geometry.rs:136-139` | The layout's own contract: "A byte the wrap does not render — a line break, or whitespace dropped at a row break — is on no cell." |
| `src/tui/components/prompt.rs:603-645` | `caret_cell` / `caret_cell_in` read that position and clamp the column to the box's last column. |
| `src/tui/app/render.rs:2041-2044` | The only `set_cursor_position` in the tree: Insert mode puts the terminal's own cursor on that cell, so the cell is the caret. |
| `src/tui/composer_text.rs:487-493` | The **model** is correct: `insert` advances the caret by the inserted byte length. The defect is in the geometry, not in the model. |

**Why the space is invisible until the next word arrives.** The run at the end of the buffer is
dropped, so the drawn row is `"hello"` — five columns — and the caret at byte 6 resolves to
column 5, the same column the caret held at byte 5. When the next word is typed, the run becomes
the separator between two words (`wrap.rs:663-669`) and the caret jumps to the word's last
column: two columns for one keystroke.

### 3.2 The block

| Site | Fact |
|---|---|
| `src/tui/components/prompt.rs:1007-1018` | The block is painted `fg(theme.background)`, `bg(theme.primary)`, `add_modifier(Modifier::REVERSED)`. |
| `src/tui/components/prompt.rs:647-655`, `src/tui/app/render.rs:2027-2033` | The block is painted only when the composer holds the keyboard and the resolved mode is Normal. |
| `src/tui/app/render.rs:2037-2045` | In Normal mode no terminal cursor is positioned, so ratatui hides it: the painted block **is** the caret. |
| `src/tui/components/editor.rs:1809-1819` | The editor's caret, for contrast: `fg(primary)` + `UNDERLINED`, patched onto the base style. |

`Modifier::REVERSED` is SGR 7. A terminal that honours it swaps the cell's foreground and
background, so the cell is drawn as `fg(primary)` on `bg(theme.background)` — the composer's own
background. On the caret's usual cell (a blank one, at the end of the text) the cell's symbol is
a space, so nothing changes on screen: the "block" is a background-coloured rectangle on a
background-coloured box.

### 3.3 Why the existing verification passed

- The geometry's unit tests cover the **cell** bijection over texts that include
  `"trailing spaces   "` (`src/tui/composer_geometry.rs:339`, `:352`) and the **line-break** caret
  (`:492`) — but no test asserts the caret's column for a byte after dropped whitespace.
- The render test `the_composer_paints_a_block_in_normal_mode`
  (`tests/tui_render/tui_composer_cursor.inc`) asserts the `REVERSED` **bit** and never the
  colours, so an invisible block satisfies it.
- No test types a space at the end of the buffer and compares the caret's cell before and after.

## 4. Invariants that must not regress

- **I1 — the cells the composer draws are unchanged.** The correction changes where the **caret**
  is, not what is drawn. The layout's cell set, the row count, the byte↔cell bijection and the
  text a row shows are all unchanged. `the_mapping_is_a_bijection_over_widths_and_texts`
  (`src/tui/composer_geometry.rs:352`) must still hold.
- **I2 — a collapsed whitespace run is one column.** The correction applies the existing
  convention; it does not give a run a column per space.
  `a_collapsed_whitespace_run_is_on_the_space_s_column` (`src/tui/composer_geometry.rs:478`) must
  still hold. **What that test does not say** — and what this milestone does not change — is where
  the caret lands *inside* a drawn run: today `caret_position("a  b", 2)` is column **2**, one
  past the run's single column, because the fallback adds the containing cell's width. That
  wrinkle is pre-existing, unreported and out of scope; the milestone records it as a finding
  rather than silently changing it.
- **I3 — a caret after a line break is at the start of the next row.**
  `a_caret_after_a_line_break_is_at_the_start_of_the_next_row`
  (`src/tui/composer_geometry.rs:492`) must still hold, byte for byte.
- **I4 — the caret never leaves the box.** The clamp to the box's last column
  (`src/tui/components/prompt.rs:637-638`) is unchanged.
- **I5 — Insert mode's caret is the terminal's own cursor.** The correction does not introduce a
  painted caret in Insert mode, and does not change when the terminal cursor is positioned
  (`src/tui/app/render.rs:2041-2044`).
- **I6 — the block is painted only when the composer holds the keyboard in Normal mode.** The
  correction changes the block's colours, not when it is painted.
- **I7 — one redraw cadence.** Nothing here touches timing.
- **The model, the claim table and the key routing are untouched.** This milestone changes no
  verb, no condition, no binding and no state.

## 5. Scope

**In scope.**

- `VisualLayout::caret_position` in `src/tui/composer_geometry.rs` — the resolution of a byte
  that is on no cell.
- The Normal-mode block's style in `src/tui/components/prompt.rs`.
- The regression tests: the geometry unit tests, the composer's own unit tests, and the
  `tui_render` members that cover the caret.
- The documentation corrections in §12.

**Out of scope.**

- The shared wrap primitive `wrap_styled_spans_by_word_rows`. The transcript's
  trailing-whitespace policy is a separate decision, and the composer must not decide it.
- The model's insert path, which is already correct.
- The composer's selection semantics (C002), the vertical ownership condition (C003), the badge
  (C002) and the editor.

## 6. Required production changes

### 6.1 The caret's column counts the source

`VisualLayout::caret_position` resolves every byte of the buffer to a position. The rule is
stated here because it is the milestone's substance, and the implementer may choose the code
shape.

> **A byte that is on no cell resolves to the column it occupies in the source.** A whitespace
> run the wrap dropped occupies **one column** — the same width a drawn run has. A caret that
> has entered a gap **between** two rows belongs to the **next** row (the text continues there);
> a caret past the **last** row's end belongs to that row, one column past its drawn text.
> The **first** byte of a gap is the previous row's own end, and stays there — that is the
> existing behaviour, and the line-break case already depends on it.

The table below is for a byte `b` that is **on no cell** (`cell_of_byte` returns `None`). The two
byte classes that are *on* a cell are listed first, because the milestone does not change them:

| Situation | Resolution | Why |
|---|---|---|
| `b` starts a cell | That cell | Unchanged: the cell lookup already wins. |
| `b` is **inside** a cell — a multi-byte character, or a drawn collapsed run | That cell's column **plus its width** | Unchanged: the fallback adds the containing cell's width. This is the pre-existing wrinkle invariant I2 names, and the milestone does not touch it. |
| `b` is the byte immediately after a line break | (the next row, column 0) | Unchanged: the layout's own newline branch (`src/tui/composer_geometry.rs:183-188`), which invariant I3 protects. |

The remaining cases are the ones the milestone corrects — a byte `b` that is on no cell:

| Situation | Resolution | Why |
|---|---|---|
| `b` is exactly a row's end, and the next row exists | (that row, that row's end column) | The gap's **first** byte is the previous row's last byte: the caret sits at the end of the drawn text. This is the existing behaviour, and `a_caret_after_a_line_break_is_at_the_start_of_the_next_row` depends on it — for `"one\ntwo"`, byte 3 is row 0's end and resolves to (row 0, column 3). |
| `b` is strictly inside a gap between two rows | (next row, column 0) | The caret has entered the gap the row break left, and the text continues on the next row. A multi-byte gap is one column wide, so every byte inside it resolves to the same place. |
| `b` is past the last row's end | (last row, that row's end column + 1) | The trailing run is one column, and the row's own drawn text ends one column earlier. |
| `b` is exactly the last row's end | (last row, that row's end column) | Unchanged: the caret sits at the end of the drawn text. |

**Worked examples** (the buffer's bytes, and the column each caret byte resolves to):

| Buffer | Caret byte | Column before the milestone | Column after |
|---|---|---|---|
| `"hello "` | 5 | 5 | 5 |
| `"hello "` | 6 | **5** | **6** |
| `"hello  "` | 6 | **5** | **6** |
| `"hello  "` | 7 | **5** | **6** (one column for the run, like a drawn run) |
| `"hello world"` at width 11 | 6 | 6 | 6 |
| `"one\ntwo"` | 3 | 3 | 3 |
| `"one\ntwo"` | 4 | 0 (row 1) | 0 (row 1) |
| `"hello world"` at width 6 | 5 | 5 (row 0) | 5 (row 0) — the gap's first byte is row 0's end |
| `"hello world"` at width 6 | 6 | 0 (row 1) | 0 (row 1) |
| `"hello   world"` at width 6 | 6 | 5 (row 0) | 0 (row 1) — inside the three-byte gap the row break left |

The **acceptance property** is one keystroke, one column: for any buffer, the column of the
caret at byte `b + 1` is exactly one column to the right of the column at byte `b`, except where
the wrap broke a row — there the caret moves to the next row — and except inside a single
whitespace run, which is one column by the existing convention.

**Two places where the property does not hold, and why.** The property is about the **layout**,
not about the pixel:

- **The box's right edge.** `caret_cell` clamps the column to the box's last column
  (`src/tui/components/prompt.rs:637-638`) and subtracts the horizontal scroll. A caret one
  column past the drawn text in a box that is already full is therefore drawn on the box's last
  column, and the horizontal scroll absorbs the difference. The property holds in the layout; the
  **cell** is clamped. The milestone does not change the clamp.
- **The selection painting and the mouse.** The mouse's hit-testing reads the **cell map** —
  `PromptWidget::set_cursor_at_column` looks the column up in `layout.row_cells(row)`
  (`src/tui/components/prompt.rs:224-233`) — and `cell_of_byte`
  (`src/tui/composer_geometry.rs:136-156`) is the byte→cell direction of that same map. The
  selection painting reads the row's **byte ranges** (`row_spans`), not the cells. The correction
  changes the caret's **column** only: the cells are untouched, so the mouse keeps the map it has,
  and the existing round-trip test (`the_mapping_is_a_bijection_over_widths_and_texts`) guards it.

### 6.2 The block is a block

The Normal-mode block's style drops `Modifier::REVERSED`:

```rust
Style::default()
    .fg(self.theme.background)
    .bg(self.theme.primary)
```

The explicit foreground/background pair then reaches the terminal as written: the cell's
background is `theme.primary` and its glyph is drawn in `theme.background`, so the block reads
as an inverted block on **every** cell it lands on, a blank one included.

The milestone does **not** adopt the editor's `UNDERLINED` caret. The composer is a Helix
surface whose Normal-mode caret is a block, and `ComposerCursor::Block` is the variant the
widget already models. What is wrong is the paint, not the shape.

**The theme's contrast is the implementer's check.** `theme.primary` must be visibly different
from `theme.background`; if a theme pairs them too closely, the theme is out of this
milestone's scope and the fact is recorded in the closure record rather than worked around.

## 7. Ordered work packages

- **WP1 — the geometry.** Change `VisualLayout::caret_position` (and, if the implementation
  needs it, a small private helper) to resolve the no-cell bytes as §6.1 states. Do not touch
  `row_cells`, `byte_at`, `cell_of_byte` or the row construction: the drawn cells stay as they
  are.
- **WP2 — the geometry's unit tests.** Extend `src/tui/composer_geometry.rs`'s test module with
  the cases in §10.2. Each test names the property, not the implementation.
- **WP3 — the block's paint.** Change the style in `src/tui/components/prompt.rs` and update the
  composer's own unit test for the block.
- **WP4 — the render tests.** Update `tests/tui_render/tui_composer_cursor.inc`: the block test
  must assert the **colours** and the absence of `REVERSED`, and a new member must assert that
  the caret's cell advances by one column when a space is typed.
- **WP5 — the documentation.** §12.
- **WP6 — the verification.** §11, in order, with the results recorded in the closure record.

## 8. Failure, cancellation, restart, contention semantics

None. The change is a pure function of the buffer and the layout: no state, no task, no timer, no
persistence. A failure to compute a position leaves the caret where it was, exactly as today.

## 9. Compatibility and migration

- **Storage:** none.
- **Protocol:** none. The caret is a frontend-local position.
- **Migration:** none.
- **User-visible change:** the caret advances on a space, and the Normal-mode block is visible.
  Both are corrections of a defect that was reported, not a change of a published contract.
- **Downstream surfaces:** the transcript buffer does not use `VisualLayout`; the composer is
  its only consumer. The editor has its own caret and its own layout.

## 10. Required tests

### 10.1 Geometry unit tests — `src/tui/composer_geometry.rs`

| Test | Asserts |
|---|---|
| `a_caret_after_dropped_whitespace_is_one_column_past_the_text` | For `"hello "`, the caret at byte 6 is at column 6 and the caret at byte 5 is at column 5. |
| `a_dropped_whitespace_run_is_one_column` | For `"hello  "`, the caret at bytes 6 and 7 are at the same column, and that column is one past the drawn text's last column. |
| `a_caret_after_a_line_break_is_at_the_start_of_the_next_row` | **Existing** — unchanged, byte for byte. |
| `a_collapsed_whitespace_run_is_on_the_space_s_column` | **Existing** — unchanged; the run is still one cell of one column. The caret *inside* the run is a separate, pre-existing wrinkle this milestone does not touch (see invariant I2). |
| `the_mapping_is_a_bijection_over_widths_and_texts` | **Existing** — unchanged; the drawn cells are untouched. |
| `a_caret_at_the_end_of_the_text_is_one_past_the_last_character` | For `"hello"`, the caret at byte 5 is at column 5 — the row's end, not past it. |
| `a_caret_at_a_row_break_sits_at_the_row_end_and_then_moves_to_the_next_row` | For `"hello world"` at width 6, the caret at byte 5 — the gap's first byte, which is row 0's end — is at row 0 column 5, and the caret at byte 6 is at row 1 column 0. |
| `a_caret_inside_a_multi_byte_gap_is_on_the_next_row` | For `"hello   world"` at width 6, the caret at byte 6 — strictly inside the three-byte gap the row break left — is at row 1 column 0. |

### 10.2 Composer unit tests — `src/tui/components/prompt.rs`

| Test | Asserts |
|---|---|
| `the_composer_paints_a_block_in_normal_mode` | **Updated**: the block cell's background is `theme.primary` and its foreground is `theme.background`, and the cell carries **no** `REVERSED` modifier. The existing assertion on the `REVERSED` bit is replaced, not kept alongside. |
| `the_caret_cell_moves_one_column_when_a_space_is_typed` | Type `hello`, read the caret's cell, type a space, and assert the caret's cell is one column to the right. |

### 10.3 Render tests — `tests/tui_render/tui_composer_cursor.inc`

| Test | Asserts |
|---|---|
| `the_composer_paints_a_visible_block_in_normal_mode` | The caret cell's colours differ from the composer's background, on a **blank** cell at the end of the text. |
| `the_composer_caret_advances_one_column_on_a_space` | The rendered caret's cell after typing a space is one column right of the cell before it. |

Both members belong to the existing `tui_render` family; no new test **file** is created, so the
verified-counts table's *test files* row does not move.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- composer_geometry
cargo test -p codegg --lib -- tui::components::prompt
cargo test --test tui_render -- tui_composer_cursor
cargo test --test tui_render -- tui_composer_wrap
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

`scripts/verify.sh quick` is the canonical gate. The milestone adds no guard, so
`architecture/overview.md`'s verified-counts table does not move, and
`architecture/count_audit_history.md` records no reconciliation for this milestone.

## 12. Documentation updates

| Document | Change |
|---|---|
| `src/tui/composer_geometry.rs` (module and `caret_position` docs) | The resolution rule of §6.1, in the module's own words, including why a dropped run is one column. |
| `src/tui/components/prompt.rs` (the block's paint) | The reason the block carries no `REVERSED`: the modifier swaps the colours and cancels the block. |
| `src/tui/app/render.rs` (the comment at the Insert-mode cursor) | No change needed; the comment is about the terminal cursor's placement. Correct it only if it states a caret-position fact that this milestone changed. |
| `architecture/tui.md` | The composer's caret section, **only** if it states a caret-position or a caret-visibility fact this milestone changed. Read it before editing; if it says nothing about either, this milestone edits nothing here. |
| `docs/tui.md` | The composer's section, **only** for the same two facts. |

The composer-text-layout roadmap's closed records are **not** rewritten: they are historical
evidence of what was decided then, and this milestone's closure record states the correction.

## 13. Acceptance criteria

1. Typing a space at the end of the composer buffer moves the caret one column; typing the next
   character moves it one column again. A test asserts both.
2. The Normal-mode block is visible: its cell's background is `theme.primary` and it carries no
   `REVERSED` modifier. A test asserts the colours **and** the absence of the modifier.
3. The block is visible on a blank cell — the caret at the end of the text — not only on a
   character cell.
4. The layout's drawn cells, its row count and its byte↔cell bijection are unchanged; the
   existing geometry tests pass unmodified.
5. The caret after a line break still resolves to the next row's column 0, byte for byte.
6. The caret inside a drawn collapsed run still resolves to that run's column.
7. `scripts/verify.sh quick` passes, and clippy is clean with `-D warnings`.
8. The closure record names the measured before/after columns and the rule that produced them.

## 14. Stop conditions

- The rule in §6.1 cannot be implemented without changing the drawn cells or the row count. That
  is a different correction, and the milestone stops with the evidence.
- `theme.primary` and `theme.background` are the same colour in the default theme. The block
  would still be invisible, and the correction is incomplete; the fact is recorded and the
  milestone stops rather than widening the theme's palette.
- The change makes any existing geometry, prompt or `tui_render` test fail for a reason other
  than the two defects. That is a regression in the composer's layout, and the milestone stops.
- The correction would have to change the shared wrap primitive to work. The wrap is the
  transcript's engine; the milestone stops and the finding is escalated.

## 15. Closure evidence required

- The closure record `plans/closure/tui-composer-helix-parity-post-closure-corrective/001-status.md`,
  with: the measured before/after caret columns; the rule that produced them; the diff of the
  two defect fixes; the test names and their assertions; the verification commands and their
  outputs; and the explicit statement that the drawn cells are unchanged.
- The updated unit and render test names.
- A note on whether `theme.primary` and `theme.background` are distinguishable in the default
  theme.

## 16. Handoff notes

- C002 changes the badge and the composer's selection semantics; it does not touch the caret's
  geometry or the block's paint. If the two land together, keep the diffs separable.
- C003 changes the claim table's condition and the app-level page routing; it does not touch
  this milestone's files except for the composer's page methods, which it reuses.
- The **measured** baseline for the caret is the probe's output in §2. The implementer may
  reproduce it with a temporary test and must remove it again before committing; the tree must
  be clean apart from the milestone's own changes.
