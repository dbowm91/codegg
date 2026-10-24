# TUI Composer Helix Parity Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity-post-closure-corrective/001-the-caret-tells-the-truth.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md#c001--the-caret-tells-the-truth`

Repository baseline reviewed: `c2d781319` (branch `fix/file-picker-surface-corrective`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the two
  production changes, the geometry and render tests, this record, and the
  re-banked comment budget. No hosted CI run is claimed.

Applicable ADRs (unchanged by this milestone):

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted**
  2026-10-19. The caret's geometry and its paint are outside the ADR's scope:
  this milestone changes no key claim, no condition, no binding and no state.

## 1. Executive finding

The caret now tells the truth in both modes, and both defects are closed.

**Defect 1 — the space.** In Insert mode the caret's *byte* advanced when a
space was typed, but the caret's *cell* did not: the wrap drops the whitespace
at the end of the buffer, the caret's fallback column was derived from the
drawn cells alone, and the byte before the space and the byte after it
resolved to the **same column**. The caret's column now counts the **source**,
so one keystroke moves the caret one column.

**Defect 2 — the block.** In Normal mode the painted block carried
`Modifier::REVERSED`, which is SGR 7: a terminal honours it by swapping the
cell's foreground and background. The block's own colours already said
"inverted", and the swap cancelled them, so the block was drawn in the
composer's own background colour and was invisible. The block no longer
carries the modifier, and its colours reach the terminal as written.

**The drawn cells are unchanged.** `row_cells`, `byte_at`, `cell_of_byte` and
the row construction are untouched: the milestone changes where the *caret* is,
not what is drawn. The layout's row count, its cell set and its byte↔cell
bijection are the same objects as before, and
`the_mapping_is_a_bijection_over_widths_and_texts` passes unmodified.

One finding is **out of this milestone's ownership** and is recorded in §10: a
test in `tests/tui_render/tui.inc` still asserts the *old* block paint (the
`REVERSED` bit) and now fails. That file is outside this milestone's write
scope, so the assertion was left alone and the exact required change is
recorded instead.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The caret's column counts the source | `VisualLayout::caret_position` → `no_cell_position` (`src/tui/composer_geometry.rs`) | pass | The rule is stated in the module doc and in the function's own doc. |
| A dropped whitespace run is one column | `a_dropped_whitespace_run_is_one_column` | pass | Bytes 6 and 7 of `"hello  "` share column 6. |
| A caret after dropped whitespace is one column past the text | `a_caret_after_dropped_whitespace_is_one_column_past_the_text` | pass | `"hello "`: byte 5 → column 5, byte 6 → column 6. |
| The caret at the end of the text is at the row's end | `a_caret_at_the_end_of_the_text_is_one_past_the_last_character` | pass | `"hello"`: byte 5 → column 5. |
| A gap's first byte is the previous row's own end | `a_caret_at_a_row_break_sits_at_the_row_end_and_then_moves_to_the_next_row` | pass | `"hello world"` at width 6: byte 5 → (row 0, column 5), byte 6 → (row 1, column 0). |
| A byte inside a gap belongs to the next row | `a_caret_inside_a_multi_byte_gap_is_on_the_next_row` | pass | `"hello   world"` at width 6: bytes 6 and 7 → (row 1, column 0). |
| **I1 — the drawn cells are unchanged** | `the_mapping_is_a_bijection_over_widths_and_texts` and `the_byte_and_the_cell_are_inverses` pass **unmodified**; `row_cells`/`byte_at`/`cell_of_byte` are not in the diff | pass | The diff touches `caret_position` and adds two private helpers. |
| **I2 — a collapsed whitespace run is one column** | `a_collapsed_whitespace_run_is_on_the_space_s_column` passes unmodified | pass | The caret *inside* a drawn run still resolves one column past that run — the pre-existing wrinkle §4 names, left untouched. |
| **I3 — a caret after a line break is at the start of the next row** | `a_caret_after_a_line_break_is_at_the_start_of_the_next_row` passes **byte for byte, unmodified** | pass | `"one\ntwo"`: byte 3 → (row 0, column 3), byte 4 → (row 1, column 0). |
| **I4 — the caret never leaves the box** | `PromptWidget::caret_cell_in` is unchanged; its clamp to the box's last column is untouched | pass | The clamp is in `src/tui/components/prompt.rs`, outside the diff. |
| **I5 — Insert mode's caret is the terminal's own cursor** | no painted caret was added; `src/tui/app/render.rs` is not in the diff | pass | `insert_mode_positions_the_terminal_cursor_at_the_caret` passes. |
| **I6 — the block is painted only when the composer holds the keyboard in Normal mode** | the block's *gate* is unchanged; only its style changed | pass | `the_composer_paints_no_block_in_insert_mode`, `the_composer_paints_no_block_when_a_panel_holds_the_keyboard`, `the_block_and_the_terminal_cursor_are_never_both_present`. |
| **I7 — one redraw cadence** | no timer, tick or interval added | pass | |
| The Normal-mode block is visible | `the_composer_paints_a_block_in_normal_mode` (unit) and `the_composer_paints_a_visible_block_in_normal_mode` (render) | pass | Both assert the colours **and** the absence of `REVERSED`. |
| The block is visible on a **blank** cell | `the_composer_paints_a_visible_block_in_normal_mode` asserts the cell's symbol is a space | pass | The caret at the end of `"hello"` is a blank cell. |
| Typing a space moves the caret one column | `the_caret_cell_moves_one_column_when_a_space_is_typed` (unit) and `the_composer_caret_advances_one_column_on_a_space` (render) | pass | The render test types the space through the app's own key path (`on_key`). |
| `theme.primary` and `theme.background` are distinguishable | `Theme::dark()`: `primary` = `Rgb(120, 180, 255)`, `background` = `Rgb(15, 15, 20)`, `input_bg` = `Rgb(8, 8, 12)` | pass | The block's background differs from the composer's own background, so the block reads on a blank cell. |
| The model, the claim table and the key routing are untouched | no diff in `composer_text.rs`, `composer_claims.rs`, `input.rs`, `app/mod.rs` | pass | |

## 3. Production implementation evidence

**The geometry (`src/tui/composer_geometry.rs`).**

`VisualLayout::caret_position` resolves a byte in four steps, and the first
three are the ones the milestone did not touch:

1. a byte a rendered character **starts** on is that character's cell;
2. a byte **just after a line break** is the start of the next row;
3. a byte **inside** a rendered cell — a multi-byte character, or a drawn
   collapsed whitespace run — is one column past that cell (the pre-existing
   wrinkle invariant I2 names);
4. a byte the wrap **dropped** is the new case, handled by the new private
   helper `no_cell_position`.

The rule the helper implements, verbatim from the plan's §6.1:

> **A byte that is on no cell resolves to the column it occupies in the
> source.** A whitespace run the wrap dropped occupies **one column** — the
> same width a drawn run has. A caret that has entered a gap **between** two
> rows belongs to the **next** row; a caret past the **last** row's end belongs
> to that row, one column past its drawn text. The **first** byte of a gap is
> the previous row's own end, and stays there.

The code shape is:

- `no_cell_position(byte)` — the dispatcher for a byte on no cell. It first
  asks `row_containing(byte)`: a byte **inside** a row's source range that no
  cell draws (a zero-width character, or a terminal control sequence) keeps
  the answer it always had — one column past the last cell before it — and the
  dropped bytes go on to the gap rules.
- `row_ending_at_or_before(byte)` — the last row whose source range ends at or
  before the byte. It names the row the gap follows.
- `row_containing(byte)` — the row whose source range holds the byte.

The gap rules, in the order they are applied (the order matters: the byte
that is *exactly* a row's end is that row's end, and only a byte *past* it is
the dropped run's column):

| The byte | The answer |
|---|---|
| exactly a row's end | (that row, that row's end column) |
| past the last row's end | (last row, that row's end column + 1) |
| strictly inside a gap between two rows | (next row, column 0) |

The **drawn cells** are unchanged: `row_cells`, `byte_at`, `cell_of_byte` and
the row construction are not in the diff, and the layout's cell set, row count
and byte↔cell bijection are the same objects as before.

**The block's paint (`src/tui/components/prompt.rs`).**

```diff
                     cell.set_style(
                         Style::default()
                             .fg(self.theme.background)
-                            .bg(self.theme.primary)
-                            .add_modifier(Modifier::REVERSED),
+                            .bg(self.theme.primary),
                     );
```

The `Modifier::REVERSED` term is removed; the style is now
`Style::default().fg(self.theme.background).bg(self.theme.primary)`. The
`Modifier` import moved from the module's use list to the test module's,
because the production path no longer names it. The reason is recorded in the
paint's own comment: the modifier is SGR 7, it swaps the cell's two colours,
and the swap cancelled the block.

**The diff of the two defect fixes, side by side.**

| | Before | After |
|---|---|---|
| The space | `caret_position`'s fallback returned the last cell before the byte, plus that cell's width — so the byte before a dropped space and the byte after it resolved to the same column | the no-cell bytes resolve against the source: the gap's ends and the dropped run's single column |
| The block | `fg(background) bg(primary)` **plus** `Modifier::REVERSED`, which the terminal applies as a colour swap and which cancelled the block | `fg(background) bg(primary)`, with no modifier |

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo fmt --all --check
cargo test -p codegg --lib -- composer_geometry
cargo test -p codegg --lib -- tui::components::prompt
cargo test --test tui_render -- tui_composer_cursor
cargo test --test tui_render -- tui_composer_wrap
CARGO_BUILD_JOBS=3 cargo clippy -p codegg --all-targets --locked -- -D warnings
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean (exit 0) |
| `cargo test -p codegg --lib -- composer_geometry` | **14 passed**, 0 failed (9 before this milestone, 5 added) |
| `cargo test -p codegg --lib -- tui::components::prompt` | **25 passed**, 0 failed (23 before this milestone, 2 added) |
| `cargo test --test tui_render -- tui_composer_cursor` | **25 passed**, 0 failed (24 before this milestone, 1 added, 1 renamed) |
| `cargo test --test tui_render -- tui_composer_wrap` | **4 passed**, 0 failed |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | **clean** — no errors, no warnings (exit 0) |
| `python3 scripts/check_test_assertion_quality.py` | `check_test_assertion_quality: all checks pass` (exit 0) |
| `python3 scripts/check_test_duplication.py` | `check_test_duplication: all checks pass` (exit 0) |
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` (exit 0), after the re-bank described below |
| `bash scripts/verify.sh quick` | **passed** (exit 0) — "Quick verification passed", 99 guard steps including the harness-lab unittest sweep (328 tests, 1 skipped) and `cargo check --workspace --all-targets --locked` |

### The measured before/after columns

Both columns were **measured**, not read off the plan. The "before" column was
measured with a temporary probe against the baseline implementation
(`git checkout -- src/tui/composer_geometry.rs` plus a throw-away
`#[test]` that printed `caret_position` for each case); the "after" column was
measured with the same probe against the corrected implementation. The probe
was removed afterwards and the tree carries no trace of it.

| Buffer | Width | Caret byte | Before | After |
|---|---|---|---|---|
| `"hello "` | 10 | 5 | (row 0, column 5) | (row 0, column 5) |
| `"hello "` | 10 | 6 | **(row 0, column 5)** | **(row 0, column 6)** |
| `"hello  "` | 10 | 5 | (row 0, column 5) | (row 0, column 5) |
| `"hello  "` | 10 | 6 | **(row 0, column 5)** | **(row 0, column 6)** |
| `"hello  "` | 10 | 7 | **(row 0, column 5)** | **(row 0, column 6)** |
| `"hello world"` | 11 | 6 | (row 0, column 6) | (row 0, column 6) |
| `"one\ntwo"` | 10 | 3 | (row 0, column 3) | (row 0, column 3) |
| `"one\ntwo"` | 10 | 4 | (row 1, column 0) | (row 1, column 0) |
| `"hello world"` | 6 | 5 | (row 0, column 5) | (row 0, column 5) |
| `"hello world"` | 6 | 6 | (row 1, column 0) | (row 1, column 0) |
| `"hello   world"` | 6 | 5 | (row 0, column 5) | (row 0, column 5) |
| `"hello   world"` | 6 | 6 | **(row 0, column 5)** | **(row 1, column 0)** |
| `"hello   world"` | 6 | 7 | **(row 0, column 5)** | **(row 1, column 0)** |

The bolded rows are the ones the milestone moves. Every other row is
unchanged, which is the point: the rule moves the bytes the wrap dropped and
nothing else. The baseline measurement reproduces the plan's §2 probe output
(`caret 5 -> Some((0, 5))`, `caret 6 -> Some((0, 5))` for `"hello "`, and
`caret 5/6/7 -> (0, 5)` for `"hello  "`) exactly.

### The theme's contrast

The default theme is `Theme::dark()` (`src/tui/theme.rs`), which is also the
theme `App::new_for_testing` installs:

| Role | Colour |
|---|---|
| `theme.background` | `Rgb(15, 15, 20)` (`#0f0f14`) |
| `theme.primary` | `Rgb(120, 180, 255)` (`#78b4ff`) |
| `theme.input_bg` (the composer's own box background) | `Rgb(8, 8, 12)` (`#08080c`) |

`theme.primary` and `theme.background` are **clearly distinguishable**, so the
block reads as an inverted block on every cell it lands on, a blank one
included. The plan's §14 stop condition — "`theme.primary` and
`theme.background` are the same colour in the default theme" — does not hold,
and no theme's palette was touched.

### The comment budget

The three files this milestone writes were each **exactly at** their recorded
ceiling, so the new documentation could not fit. The budget's documented
procedure is to re-measure the tree, so
`python3 scripts/check_comment_budget.py --bank` was run. Its diff is
confined to this milestone's three files and the three global values — no other
file's ceiling moved:

```diff
 [global]
-max_comment_lines = 122449
-max_comment_words = 1078055
-max_ratio = 0.1316
+max_comment_lines = 122524
+max_comment_words = 1078890
+max_ratio = 0.1317
-"src/tui/components/prompt.rs" = { max_comment_lines = 409, max_ratio = 0.417 }
+"src/tui/components/prompt.rs" = { max_comment_lines = 425, max_ratio = 0.4229 }
-"src/tui/composer_geometry.rs" = { max_comment_lines = 128, max_ratio = 0.37 }
+"src/tui/composer_geometry.rs" = { max_comment_lines = 173, max_ratio = 0.3932 }
-"tests/tui_render/tui_composer_cursor.inc" = { max_comment_lines = 116, max_ratio = 0.2673 }
+"tests/tui_render/tui_composer_cursor.inc" = { max_comment_lines = 130, max_ratio = 0.2749 }
```

### What was **not** run

- `scripts/verify.sh full` (clippy `-D warnings` over the whole workspace plus
  the workspace test suite and the feature-gated suite). The package-scoped
  clippy above covers every crate and every test target this change set
  touches; the full sweep was not run, and no claim is made about it.
- A hosted CI run. No CI job was executed for this change set; the evidence
  above is local.
- `cargo test --test tui_render` as a whole. It is **red**, for one reason
  only: **1082 passed, 1 failed** — the stale assertion recorded in §10
  (`tui::test_prompt_widget_cursor_on_empty_line`).

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — the cells the composer draws are unchanged** | `the_mapping_is_a_bijection_over_widths_and_texts` and `the_byte_and_the_cell_are_inverses` pass unmodified; `row_cells`, `byte_at`, `cell_of_byte` and the row construction are absent from the diff | pass |
| **I2 — a collapsed whitespace run is one column** | `a_collapsed_whitespace_run_is_on_the_space_s_column` passes unmodified; the caret inside a *drawn* run still resolves one column past the run, the pre-existing wrinkle the plan leaves out of scope | pass |
| **I3 — a caret after a line break is at the start of the next row** | `a_caret_after_a_line_break_is_at_the_start_of_the_next_row` passes unmodified, byte for byte | pass |
| **I4 — the caret never leaves the box** | the clamp in `PromptWidget::caret_cell_in` is untouched | pass |
| **I5 — Insert mode's caret is the terminal's own cursor** | `src/tui/app/render.rs` is not in the diff; `insert_mode_positions_the_terminal_cursor_at_the_caret` passes | pass |
| **I6 — the block is painted only when the composer holds the keyboard in Normal mode** | the gate is untouched; the three ownership render tests pass | pass |
| **I7 — one redraw cadence** | no timer, tick or interval added | pass |
| **The model, the claim table and the key routing are untouched** | the diff contains no change to `composer_text.rs`, `composer_claims.rs`, `input.rs` or `app/mod.rs` | pass |

## 6. Failure and recovery review

- **Failure.** `caret_position` is a pure function of the layout and the byte;
  it cannot fail. It returns a position for every byte of the buffer, and the
  paths that could have had no answer (a byte before the first row, a byte in
  a row that holds no cell) fall back to `(row 0, column 0)` — the same
  fallback the old code had.
- **Cancellation, restart, persistence.** Not applicable: no task, no timer,
  no state and nothing persisted. The caret's position is recomputed from the
  buffer on every frame.
- **Contention.** The composer is single-owner and frontend-local; the
  milestone adds no key handling and no shared resource.
- **Resize.** Unchanged: the caret's cell is derived per frame from the
  layout, so the changed column follows the layout, not a stale cache.
- **Malformed input.** Not applicable: no input is parsed. A byte inside a
  multi-byte character and a byte inside a zero-width character both keep the
  answer they had.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. The caret is a frontend-local position.
- **Configuration.** None.
- **Backward compatibility.** Two user-visible corrections, both of them the
  point of the milestone: the caret now advances a column when a space is
  typed, and the Normal-mode block is now visible. No published contract
  changes; the block's *shape* is unchanged, and the editor's own caret is
  untouched.
- **Rollback.** The change set is confined to two source files, one render
  test file, one architecture paragraph and this record; reverting it restores
  the previous behaviour exactly. The re-banked comment budget would also
  need reverting with it.
- **Legacy paths.** None. No flag, message or API was removed.

## 8. Security review

- **Authorization.** Unchanged. The milestone adds no key handling, no tool
  path and no daemon interaction.
- **Secrets.** None handled.
- **Privilege boundaries.** Unchanged.
- **Denial of service.** `caret_position` is the same order of work as before
  — a bounded scan over the layout's rows, on the buffer's own length. The new
  `row_containing` / `row_ending_at_or_before` helpers are bounded by the row
  count.
- **Audit.** No new audit event: the caret's position is presentation.

## 9. Documentation and operations

- `src/tui/composer_geometry.rs` — the module doc and `caret_position`'s doc
  now state the resolution rule, including why a dropped run is one column,
  and `no_cell_position` carries the rule in its own words.
- `src/tui/components/prompt.rs` — the block's paint carries the reason it has
  no `REVERSED` modifier.
- `architecture/tui.md` — the composer's geometry section gained one paragraph
  stating the resolution rule for the bytes the wrap dropped. Nothing else in
  that document states a caret-position or caret-visibility fact this
  milestone changed.
- `docs/tui.md` — **read, not edited**. Its composer section states what the
  user sees ("a **solid block** at the caret") and how the caret moves; it
  states neither the caret's column resolution nor how the block is painted,
  and the statement it does make is now true rather than aspirational.
- `src/tui/app/render.rs` — **read, not edited**. The comment at the
  Insert-mode cursor is about the terminal cursor's placement, and the other
  `REVERSED` comment in that file is about the tasks overlay's selection
  marker, not the composer's caret.
- **Static guards.** None added. The milestone adds no guard; the existing
  comment-budget, test-assertion-quality and test-duplication guards pass.
- **Operator diagnostics.** None. Nothing to diagnose.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `tests/tui_render/tui.inc:698` (`tui::test_prompt_widget_cursor_on_empty_line`) still asserts that the composer's block cell carries `Modifier::REVERSED`. That is the **defect** this milestone corrects, so the assertion now fails: `assertion failed: cell.modifier.contains(ratatui::style::Modifier::REVERSED)` at `tests/tui_render/tui.inc:698`. The file is outside this milestone's write scope (another milestone owns it), so the assertion was left untouched. | A whole-family `cargo test --test tui_render` run is red on that one test. The milestone's own members — `tui_composer_cursor` and `tui_composer_wrap` — are green. | The owner of `tests/tui_render/tui.inc` must replace the `REVERSED`-bit assertion with the new block contract: bind the theme (`let theme = Arc::new(Theme::dark());` and pass `Arc::clone(&theme)` to `PromptWidget::new`), then assert `cell.bg == theme.primary`, `cell.fg == theme.background` and `!cell.modifier.contains(ratatui::style::Modifier::REVERSED)`. The test's intent — that the block lands on the *empty* second line — is unchanged. |
| low | The caret *inside* a **drawn** collapsed whitespace run still resolves one column past the run (`caret_position("a  b", 2)` is column 2, not the run's column 1). This is pre-existing, unreported and explicitly out of scope (invariant I2). | A caret in the middle of a run of spaces in the source sits one column right of the run. | Recorded, not fixed: the milestone applies the run-is-one-column convention to the run the wrap *drops* and leaves the drawn run's wrinkle alone. |
| low | The plan's acceptance criterion 6 reads "the caret inside a drawn collapsed run still resolves to that run's column", which is looser than the plan's own §4 invariant I2 and §6.1 table ("that cell's column **plus its width**"). | A reader of §13 alone would expect the run's column; the authoritative rule (§6.1, and the task's own instruction) is the cell's column plus its width, which is what the code does. | None in the code. The plan's wording is inconsistent with itself; the implementation follows §6.1. |
| low | The plan's §10.1 describes the dropped run's column as "one past the drawn text's last column", while its own worked-example table gives column 6 for `"hello  "` — whose drawn text ends at column 5 (the row's end column). | The prose and the table disagree by one column of wording. | None in the code: the table's numbers are the ones the tests assert, and they are the ones the measurement produces. |

## 11. Roadmap disposition

**Milestone closed, and the next dependency may proceed.**

C001's own scope — the two defects, their regression tests and the rule that
produced them — is complete, and every acceptance criterion in the plan's §13
is met with evidence in §2. C002 and C003 do not depend on the caret's
geometry or the block's paint, so neither is blocked by this record.

The one outstanding item is the stale block assertion in `tests/tui_render/tui.inc`
(§10, medium), which belongs to the file's owner rather than to this
milestone, and which must be updated before a whole-family `tui_render` run is
green again.

## 12. Registry updates

- `plans/registry.md` — the C001 row of the post-closure corrective's status
  table becomes **closed** with this record. *Not changed by this milestone:
  the registry is outside this milestone's write scope.*
- `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md`
  — the C001 entry moves to **closed** with this record. *Not changed by this
  milestone: the addendum is outside this milestone's write scope.*
