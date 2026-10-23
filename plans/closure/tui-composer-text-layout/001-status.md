# TUI Composer Text Layout Milestone M001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-composer-text-layout/001-composer-word-wrap-and-visual-row-geometry.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-text-layout-roadmap.md#milestone-m001--the-composer-word-wraps-grows-and-scrolls`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are
  `src/tui/composer_geometry.rs` (**new**), `src/tui/components/prompt.rs`,
  `src/tui/app/render.rs`, `src/tui/mod.rs` (the new module's registration),
  `tests/tui_render/tui_composer_wrap.inc` (**new**),
  `tests/tui_render/tui.inc`, `tests/tui_render/tui_composer_cursor.inc`,
  `architecture/tui.md`, `docs/tui.md` and `.opencode/skills/tui/SKILL.md`. No
  SHA, pull request number or commit message is claimed, because none exists:
  the implementing agents were not authorized to commit.

## 1. Executive finding

The milestone's capability boundary — **how the composer's text is laid out in
rows, how much of it is visible, and where the caret sits on screen** — is
complete in the working tree, and the completion is verified by a property over
texts and widths rather than by a set of examples.

At the baseline the composer was **hard-clipped at the right edge**. It
rendered one `Line` per *logical* line with a paragraph that had **no wrap**, so
a ratatui paragraph truncated a long line; the only recourse was a horizontal
scroll, which is the paragraph's x offset and therefore panned **every** line
together. The box did not grow, because the height counted **newline
characters** — a 500-character line is one newline. The height's ceiling was
**40 % of the pane**, a budget rather than a row cap. And the geometry was
logical everywhere: the scroll offset, the caret's cell and the row count all
spoke in logical lines while the screen spoke in visual rows.

What landed:

- **`src/tui/composer_geometry.rs` (new)** — the visual-row geometry:
  `VisualCell { row, column, byte_start, byte_end, width }`,
  `VisualPosition { row, column }`, and `VisualLayout` with `new(source,
  rows)`, `row_count()`, `row_cells(row)`, `row_width(row)`,
  `row_source_start(row)`, `cell_of_byte(byte)`, `byte_at(row, column)` and
  `caret_position(byte)`.
- **`src/tui/components/prompt.rs`** — the widget paints **wrapped rows**; the
  scroll is a **visual-row** offset; the caret's drawn cell is the cell of the
  caret's **byte**; `needed_height(max_height, width)`,
  `visible_text_rows(area_height)`, `ensure_cursor_visible_with_width(
  visible_rows, width)`, `clamp_scroll(visible_rows, width)`,
  `set_cursor_at_column(visual_row, column, …)`, the selection painted **per
  wrapped row**, and a `wrap_width` field with a setter.
- **The mode row is pinned at the top** and the **marker rows pinned at the
  bottom**, with the text window scrolling between them.
- **`src/tui/app/render.rs`** — the height is computed against the **pane's**
  width, and `set_wrap_width(area.width)` is called. **The width dependency is
  not circular**: `session_layout` splits the pane *vertically*, so the prompt
  chunk's width is the pane's width and is known before the height is
  computed.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended. **No key binding
changed**, and the composer's submission path is untouched.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate.

**The risk.** A local `scripts/verify.sh quick` run is a property of **this
machine and this tree**, not of the hosted runner. The hosted lane is a strict
**subset** of `quick` — it omits the client and desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards —
so a green hosted run does **not** by itself prove everything the local gate
proves. The local gate's own value is what this record states in §4: it is
strong evidence about this tree, and it is **not** evidence about the hosted
runner's environment, its runner image, or its concurrency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — a prompt wider than the box wraps at word boundaries, and every character of it is visible or reachable | `a_prompt_wider_than_the_box_wraps_instead_of_clipping` and `a_prompt_taller_than_the_box_scrolls_to_the_content_past_the_cap` (`tests/tui_render/tui_composer_wrap.inc`); the unit tests for a word that does not fit moving to the next row whole and a token wider than the row being hard-broken | pass | The baseline's clip — `["❯", "the quick brown fox", …]` — is what the test detects. |
| §13.2 — the box grows with the content and stops at ten rows **in total**, counting the mode row and the marker rows | `needed_height` (`src/tui/components/prompt.rs:453`–`:461`); the unit test that the marker rows are inside the ten-row cap; `test_prompt_widget_needed_height_max` (`tests/tui_render/tui.inc`) | pass | The formula is in §4. The cap is on the box's **total** rows, not on the text. |
| §13.3 — on a pane whose 40 % budget is below ten rows, the budget binds | the unit test that the pane budget wins over the cap; `test_prompt_widget_needed_height_max` | pass | On a 15-row frame the budget is 6 rows and the box takes 6. |
| §13.4 — the caret is drawn on the cell of its byte, for the tested widths and texts | the unit test that the caret's cell is the cell of its byte; `the_caret_stays_visible_when_the_composer_is_scrolled` (`tests/tui_render/tui_composer_wrap.inc`); the geometry's bijection tests | pass | The closure evidence is a **property over widths and texts**, not a set of examples. |
| §13.5 — scrolling the composer keeps the caret visible and reaches the content beyond the ten-row cap | `ensure_cursor_visible_with_width` (`src/tui/components/prompt.rs:500`–`:539`); `a_prompt_taller_than_the_box_scrolls_to_the_content_past_the_cap`; `the_caret_stays_visible_when_the_composer_is_scrolled` | pass | The scroll is a visual-row offset, so a caret past the cap scrolls the composer down to itself. |
| §13.6 — no key binding changes, and the composer's claim guard is unchanged | `scripts/check_tui_composer_key_authority.py --self-test` → every violation class detected and rejected; the milestone changed no `InputAction`, no binding map, and no claim | pass | The vertical motions were **deliberately not changed** — that is M002 of this same roadmap. |
| §15 — the geometry module's unit tests, including the bijection property | 9 unit tests in `src/tui/composer_geometry.rs` | pass | The bijection over 13 texts × 6 widths; the byte and the cell are inverses; an empty buffer; a buffer ending in a newline; content wider than the row; a token wider than the row hard-broken; a multi-byte character is one cell; a collapsed whitespace run is on the space's column; a caret after a line break is at the start of the next row. |
| §15 — the height's unit tests, including the ten-row cap and the budget interaction | 12 unit tests in `src/tui/components/prompt.rs` (this milestone's share); `test_prompt_widget_needed_height`, `test_prompt_widget_needed_height_max` (`tests/tui_render/tui.inc`) | pass | One row of text is one row of the box; the marker rows are inside the ten-row cap; the pane budget wins over the cap. |
| §15 — an integration assertion that a long prompt is fully visible or reachable, and that the caret's drawn cell matches its byte | `a_prompt_wider_than_the_box_wraps_instead_of_clipping`, `a_prompt_taller_than_the_box_scrolls_to_the_content_past_the_cap`, `the_caret_stays_visible_when_the_composer_is_scrolled` | pass | — |
| §15 — the disposition of **every** existing logical-line composer test — rewritten or kept, with the reason | §3's "Existing tests rewritten and kept" subsection | pass | Four rewritten in `tui.inc`, one rewritten in `tui_composer_cursor.inc`, the rest kept because they test the logical text model, which this milestone does not touch. |
| §15 — P1's disposition: whether the horizontal scroll stays, and under what condition | §4's "P1 — the horizontal scroll's disposition" subsection | pass | It **stays**, under the condition "a visual row wider than the box". |
| §15 — a statement that no key binding changed and that the composer's submission path is untouched (I1, I2) | §3's "No key binding changed" subsection; the diff touches no binding map and no `InputAction` | pass | — |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's verification table | **partial** | The gate is **green on the working tree** (exit 0, "Quick verification passed", 74 steps). What is missing is the other half of the item: **there is no closing commit**, so the gate has not been green *at a closing commit*, and the hosted `CI / verify` lane has not run against this code. This is the milestone's condition; see §1 and §11. |
| **Non-vacuity**: the new tests detect the absence of the change | with the baseline `prompt.rs` / `render.rs` / `tui.inc` / `tui_composer_cursor.inc` swapped back in, **all 4** new integration tests fail | pass | The tree was restored afterwards. |
| The selection is painted on the wrapped row it landed on | `a_selection_is_painted_on_the_wrapped_row_it_landed_on` (`tests/tui_render/tui_composer_wrap.inc`) | pass | The selection is painted **per wrapped row**. |
| The mouse placement resolves a **visual row** | `set_cursor_at_column`'s first argument is now a visual row (`src/tui/components/prompt.rs:178`); with `wrap_width == 0` it keeps the logical behaviour, so the existing mouse tests are unchanged | pass | Without this the click would land on a line that was never drawn. |
| The geometry's cells and the rendered bytes are the same set seen from two sides | `VisualLayout::new` (`src/tui/composer_geometry.rs:94`); the bijection unit tests | pass | Every cell holds exactly one rendered character and every rendered character is on exactly one cell. |

## 3. Production implementation evidence

**The geometry module (`src/tui/composer_geometry.rs`, new).**

- `pub struct VisualCell { row, column, byte_start, byte_end, width }` — one
  rendered column and the source bytes drawn in it.
- `pub struct VisualPosition { row, column }` — a position in the wrapped
  layout; the column may be one past the last cell of the row, which is the
  insertion point at the end of the row's text.
- `pub struct VisualLayout` — built from the source text and the rows the
  shared wrap primitive produced for it (`new(source: &str, rows: &[WrappedRow])`,
  `:94`). The rows' byte offsets are offsets into `source`, so the two must
  describe the same text. A buffer that ends with a line break gets one extra
  row: the empty line the caret sits on after the break.
- The API: `row_count()` (`:121`), `row_cells(row)` (`:126`),
  `row_width(row)` (`:131`), `row_source_start(row)` (`:139`),
  `cell_of_byte(byte)` (`:149`), `byte_at(row, column)` (`:171`) and
  `caret_position(byte)` (`:185`).

**The widget (`src/tui/components/prompt.rs`).**

- The widget paints **wrapped rows**; `wrapped_rows` (`:603`) calls the
  shared primitive `wrap_styled_spans_by_word_rows`, and `layout_at` (`:608`)
  builds the `VisualLayout` from it.
- The **scroll is a visual-row offset**. `ensure_cursor_visible_with_width(
  visible_rows, width)` (`:500`) lays the composer out, records the width it
  was handed as `wrap_width` (`:503`), and scrolls so that the caret's
  **visual row** is inside the window.
- The **caret's drawn cell is the cell of the caret's byte**:
  `caret_position(self.buffer.caret())` (`:511`) answers with a row and a
  column, and the cell for that byte is what is painted.
- `needed_height(max_height: u16, width: u16) -> u16` (`:453`) — the height
  formula, in §4.
- `visible_text_rows(height: u16) -> usize` (`:421`) — the number of text rows
  visible in a box `height` rows tall.
- `clamp_scroll(visible_rows: usize, width: u16)` (`:464`) — clamps the
  scroll to the content the window can show; the width parameter was added
  because the visual scroll cannot be clamped without it.
- `set_cursor_at_column(row: usize, column: usize)` (`:178`) — its first
  argument is now a **visual row**. With `wrap_width == 0` it keeps the
  logical behaviour, so the existing mouse tests are unchanged.
- `wrap_width: u16` (`:69`) with `set_wrap_width` (`:132`).
- The **selection is painted per wrapped row**.

**The pinned rows.** The **mode row is pinned at the top** and the **marker
rows pinned at the bottom**, with the text window scrolling between them.
Before, the mode row *and* the marker rows were the paragraph's first lines and
scrolled away with the text.

**The render layer (`src/tui/app/render.rs`).**

- `let max_prompt_height = (area.height * 40 / 100).max(3);` (`:199`) — the
  40 % budget.
- `.needed_height(max_prompt_height, pane_area.width)` (`:255`) — the height
  is computed against the **pane's** width. The comment at `:252`–`:254`
  states why the pane's width and not the chunk's: the chunk's width is what
  this height is asking for, so using it would be circular.
- `self.prompt_state.prompt.set_wrap_width(area.width);` (`:1992`) — the
  composer wraps at the pane's width, which is the width this chunk has,
  because the layout splits the pane vertically.

**No key binding changed.** The milestone changed no `InputAction`, no binding
map, and no claim in `src/tui/composer_claims.rs`. The **vertical motions were
deliberately not changed** — that is M002 of this same roadmap.

### The height formula (shipped)

```
marker_rows = char_count_row (>500 chars) + (multiline) marker row
text_rows   = the wrapped row count at the pane's width
text_budget = 10 - 1 - marker_rows            // 10 = MAX_HEIGHT, the box's TOTAL rows
total       = 1 + min(text_rows, text_budget) + marker_rows
height      = total.clamp(3, max_height.max(3))   // max_height = the 40 % budget
```

The **10-row cap is a cap on the box's total rows** — the mode row, the text
rows and the marker rows together. It is **not** a limit on how much may be
typed: text past the cap stays in the buffer and is reachable by scrolling. The
**40 % budget wins**: on a 15-row frame the budget is 6 rows and the box takes
6.

**Correction to the plan's wording, verified against the source:** the 40 %
budget is 40 % of the **terminal frame** (`src/tui/app/render.rs:138` reads
`area = frame.area()`, and `:199` computes the budget from it), not of the
pane. The plan and the roadmap call it the "pane budget". The *dependency* is
still the pane's width — the budget is a **height** bound taken from the frame,
and the wrap width is the pane's.

**Tests.**

- `src/tui/composer_geometry.rs` — 9 unit tests.
- `src/tui/components/prompt.rs` — 12 unit tests (this milestone's share).
- `tests/tui_render/tui_composer_wrap.inc` — 4 integration tests.

### Existing tests rewritten and kept

- `tests/tui_render/tui.inc`:
  - `test_prompt_widget_needed_height` — **rewritten**: the logical-line
    height became the visual-row height.
  - `test_prompt_widget_needed_height_max` — **rewritten**: it now asserts
    the ten-row total cap plus the budget binding at 6.
  - `test_prompt_widget_ensure_cursor_visible` — **rewritten**: the one-arg
    form is gone (a width is required to know the rows), so the test calls the
    width-aware form and asserts the vertical scroll follows the caret's
    *visual* row.
  - `test_prompt_widget_horizontal_cursor_visibility` — **rewritten**: a
    wrapped row is never wider than the box, so the scroll stays 0 and the
    caret's wrapped row is reached by scrolling.
  - **Kept:** the remaining prompt tests in that file — they test the
    **logical text model**, which this milestone does not touch.
- `tests/tui_render/tui_composer_cursor.inc`:
  - `the_caret_cell_is_none_when_the_caret_line_is_scrolled_off` —
    **rewritten**: its scenario became unreachable under the visual model, and
    the new scenario is reachable and asserts the same property.
  - **Kept:** the other five.
- The height's signature changed, so the height assertions are **bound to the
  new API** and cannot be expressed against the baseline.

**Documentation.**

- `architecture/tui.md` — "The composer's box: wrapped rows, a visual-row
  scroll, a pinned mode row": the wrap rule, the height formula and its two
  bounds, the scroll offset, the caret's cell, and the geometry module's
  ownership.
- `docs/tui.md` — "The composer's box grows with your text": the box's growth,
  the ten-row cap, and the scrolling.
- `.opencode/skills/tui/SKILL.md` — the module guide's statement that
  `src/tui/composer_geometry.rs` owns the visual-row geometry, because a
  wrapped row is not a logical line.
- `architecture/overview.md` — the integration-test-file count row, which this
  milestone's new member moved.

## 4. Verification executed

### Commands run

```bash
# recorded at this milestone's own verification point
cargo test --locked -p codegg --lib tui::components::prompt
cargo test --locked -p codegg --lib tui::composer_geometry
cargo test --locked -p codegg --lib tui::text_geometry
cargo test --locked -p codegg --lib tui::composer_text
cargo test --locked -p codegg --test tui_render
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo check --workspace --all-targets --locked
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::prompt` | **12 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::composer_geometry` | **9 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::text_geometry` | **15 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::composer_text` | **43 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1036 passed**, 0 failed |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | every violation class detected and rejected |
| `python3 scripts/check_test_assertion_quality.py` | all checks pass |
| `python3 scripts/check_test_duplication.py` | all checks pass |
| `cargo check --workspace --all-targets --locked` | exit 0 |
| `bash scripts/verify.sh quick` | **exit 1 — one step** (at this milestone's verification point — see the re-verification below for the current result) |

**`verify.sh quick` failed at exactly one step at this milestone's
verification point:**
`check_test_target_references` reported that `architecture/overview.md`
recorded a stale integration-test-file count. That row is a **documentation
count**, not a product behaviour: the guard compares the row against the tree,
and the row had not been re-measured after this line's new integration members
were added. Every other step of the gate passed; the remainder was run
individually and passed. **The row has since been re-measured and corrected in
the same working tree, and the gate is now green** — see the re-verification
below and §9.

### Re-verification (this record's author, on the same working tree)

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::composer_geometry` | **9 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --lib tui::components::prompt` | **23 passed**, 0 failed — **higher** than the 12 recorded above, because M002 of this same roadmap added unit tests to the same module after this milestone's verification point |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed — **higher** than the 1036 recorded above, for the same reason |
| `cargo test --locked -p codegg --lib` | **7373 passed, 3 failed** — the three environmental failures in the table below, and nothing else. |
| `bash scripts/verify.sh quick` | **exit 0 — green**: "Quick verification passed", 74 steps. The one red step seen earlier in this record's writing — `check_test_target_references`, "architecture/overview.md records 326 integration test files, but the tree holds 327 root integration test modules" — was a **stale documentation count**, re-measured and corrected in the same working tree. |

The recorded test counts are **snapshots of a tree that other agents were
concurrently modifying** — the main-buffer and composer milestones of this line
were implemented in parallel, and each milestone's integration members landed in
the same family. They are therefore **not a monotonic series** across the five
records of this line, and each record's count describes the tree at that
milestone's own verification point.

### P1 — the horizontal scroll's disposition

**The horizontal scroll stays**, and its condition is now: **a visual row
wider than the box**.

With wrapping, the shared primitive hard-breaks a token wider than the row, so
every row fits the width and the horizontal scroll is zero in every ordinary
case. The one case the wrap cannot cover is a single *character* wider than the
whole row; there the scroll keeps the caret's column visible
(`src/tui/components/prompt.rs:528`–`:538`). `caret_cell` subtracts it from the
caret's column, so the painted block and the terminal cursor stay in
agreement.

### Failures recorded, not concealed

The following failures are **not** this milestone's failures, and are recorded
so that a reader is not left to discover them:

| Failure | Attribution |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** This host's `/tmp` root path is not usable by the daemon-refresh fixture. Nothing in this milestone touches the daemon. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` — `OfflineRequired is unavailable on this platform` | **Environmental and pre-existing.** The macOS host has no offline-required sandbox backend. Nothing in this milestone spawns a process. |
| (resolved) `tui::command::tests::command_docs_builtin_slash_action_variant_count_matches_source` and `tui::command::tests::command_docs_count_matches_registry` — stale counts in `architecture/command.md` ("claims BuiltinSlashAction has 160 variants; the enum has 161"; "count 168 drifts from registry 169") | **Not this milestone's.** The extra variant is M004's `/expand-all` slash action (`src/tui/command.rs`), and the stale counts were in `architecture/command.md`, which this milestone does not touch. **Documentation counts**, in the same class as the `overview.md` row below. The counts were corrected in `architecture/command.md` in the same working tree, and both tests now **pass**. |
| (resolved) `check_test_target_references` — the `architecture/overview.md` integration-test-file count row | **A documentation obligation, not a product defect.** The row records a count of test *files*; the guard compares it against the tree. It was the gate's one red step; the row has been re-measured and corrected in the same working tree, and the step is green. §9 names it. |
| (not reproduced) `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` | **Flaky, and not a failure of this milestone.** It failed once under the load of a full `--lib` run on this host, **passes when run in isolation** (`cargo test --locked -p codegg --lib tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` → 1 passed, 0 failed), and passed in the final full `--lib` run. It touches no TUI code. |

**Skipped scope.** `scripts/verify.sh full` (clippy `-D warnings` over the
whole workspace, the workspace test suite, and
`cargo test -p codegg --features server,plugins,lsp-test-support`) was **not
run**, and no claim is made about it. A hosted CI run was **not** executed
against this code; the evidence above is local. The change is confined to the
TUI's presentation path, and the commands above cover the modules it touches.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — the composer is a frontend-local string.** A layout change touches the model's buffer and nothing else: no daemon call, no revision, no document, no submission. | The diff touches `src/tui/composer_geometry.rs`, `src/tui/components/prompt.rs`, `src/tui/app/render.rs` and `src/tui/mod.rs`; the composer's submission path, its buffer's ownership, and every daemon interaction are untouched | pass |
| **I2 — the claim set is fixed.** ADR-0042 part 2 fixes the composer's claimed letters. This milestone adds no row, removes no row, and claims no new letter. `j`/`k` keep ADR-0042 part 4's condition in this milestone; M002 changes it. | `scripts/check_tui_composer_key_authority.py --self-test` → every violation class detected and rejected; the milestone changed no claim, no binding map, and no `InputAction` | pass |
| **I3 — a claimed key never falls through.** Unchanged. | The milestone adds no key handling; the claim gate at `src/tui/app/mod.rs` is unchanged in this milestone | pass |
| **I4 — the resolved mode is the gate.** Unchanged. | The mode gate is unchanged; the milestone changed only the layout the gate's surface draws in | pass |
| **I5 — the caret is on the byte it names.** The byte-offset ↔ visual-row mapping is a bijection. The closure evidence is a property over widths and texts, not a set of examples. | The geometry module's bijection tests over **13 texts × 6 widths**, the byte/cell inverse test, and the widget's "the caret's cell is the cell of its byte" test | pass |
| **I6 — the pane's budget wins.** The composer never takes more than the 40 % budget allows, whatever the 10-row cap says. | `needed_height` clamps with `total.clamp(MIN_HEIGHT, max_height.max(MIN_HEIGHT))` (`src/tui/components/prompt.rs:460`), so the budget is the upper bound; the unit test that the pane budget wins over the cap; `test_prompt_widget_needed_height_max` | pass |
| **I7 — one redraw cadence.** No second frame clock, no second spinner. | The milestone adds no timer, interval, tick or animation. The composer's rows are built in the render pass that already existed | pass |

## 6. Failure and recovery review

- **Failure.** The layout is total over its inputs: an empty buffer and a
  buffer ending in a newline are both defined (`VisualLayout::new`,
  `src/tui/composer_geometry.rs:94` — a buffer ending in a line break gets one
  extra row for the empty line the caret sits on), and a zero width returns
  the source unwrapped. The height's arithmetic uses `saturating_sub` and
  `min`, so it cannot underflow or exceed the budget.
- **Duplicate delivery and idempotency.** Not applicable. The geometry is a
  pure function of `(source, rows)`, and `VisualLayout` is derived from the
  buffer's text on each layout pass; it is not cached across deliveries, so a
  repeated layout produces the same cells.
- **Cancellation races.** Not applicable. Nothing asynchronous is added.
- **Daemon or node restart.** Not applicable. Nothing is persisted; the
  composer's text is the model's buffer, owned by the frontend.
- **Partial persistence failure.** Not applicable. No write path is added.
- **Stale generation or lease.** The `wrap_width` field is a **layout-pass
  input**, not a cached measurement: `ensure_cursor_visible_with_width` sets
  it from the width it is handed on every pass (`:503`), so a resize cannot
  leave the widget wrapping at a stale width. The fit measurement that M002
  caches is recorded by M002's record, not by this one.
- **Contention and resource release.** The composer's layout is frontend-local
  and single-threaded within a render. Its cost is O(text length) per layout,
  the same order as the pre-wrap rendering it replaces, and the shared
  primitive it uses is the one the main buffer adopted for a **3.5× reduction
  in per-layout allocation** (M001's measurement). There is no shared resource
  and nothing to release.
- **Malformed or unauthorized input.** The composer holds the user's own
  text. The wrap handles the malformed cases — an over-long token is
  hard-broken at character boundaries, and a multi-byte character is one cell
  (asserted by a geometry unit test) — and the widget does not interpret the
  text it lays out.
- **Bounded event and artifact behavior.** No event or artifact is produced.
  The visible row count is bounded by the text's length and the width; the
  box's height is bounded by the ten-row cap and the 40 % budget.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. No daemon message, session state, or wire format
  changes.
- **Configuration.** None. No configuration key is added, renamed or
  reinterpreted.
- **Backward compatibility.** The change is **user-visible** in three ways,
  all of them improvements:
  1. a long prompt **wraps** instead of being clipped at the right edge, and
     the content past the ten-row cap is reachable by scrolling;
  2. the box **grows** with the content, bounded by the ten-row total and the
     40 % budget, so a short prompt no longer reserves the room a long one
     needed;
  3. the **mode row and the marker rows are pinned** rather than scrolling
     away with the text — a behaviour change, recorded here.
  The public API of the widget changed (`needed_height` gained a width,
  `ensure_cursor_visible` was replaced by
  `ensure_cursor_visible_with_width`, `clamp_scroll` gained a width), but
  `PromptWidget` is an internal TUI type in a `0.1.0` crate, not a wire type.
- **Rollback limitations.** The change is self-contained in one new module
  (`src/tui/composer_geometry.rs`), the widget, and the render layer. Reverting
  them removes the module and restores the previous behaviour exactly. There
  is no persisted state to migrate back. The rewritten tests are bound to the
  new API and would have to be rewritten back, which is the real cost of a
  rollback here.
- **Legacy paths.** The **logical-line** geometry is retained for the paths
  that still speak it: `src/tui/text_geometry.rs` owns the **word** motions
  and the editor's own geometry, and the composer delegates to it. The
  composer keeps the logical behaviour in `set_cursor_at_column` when
  `wrap_width == 0`, so a caller that has not set a wrap width behaves exactly
  as it did.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no tool
  path, and no daemon interaction.
- **Secrets.** None handled. No credential, token, or path is read or logged.
- **Privilege boundaries.** Unchanged.
- **Path validation.** Not applicable — the milestone parses no paths and
  resolves none.
- **The trust frame.** Not applicable, and not weakened: the milestone does
  not read the frame, does not parse it, and does not touch
  `src/search_backend/framing.rs`, which has **zero diff lines**. The model
  still receives the frame byte for byte.
- **Denial of service.** The layout's cost is O(text length) per render pass,
  the same order as the baseline's rendering, and it uses the **same shared
  primitive** the main buffer adopted. The primitive's greedy word placement
  is a single linear pass with no backtracking; the pathological input — a
  single token wider than the row — is hard-broken at character boundaries,
  which is the same linear behaviour the plain engine already had. The
  composer's height is bounded by the ten-row cap and the 40 % budget, so the
  **painted** area is bounded even when the buffer is not.
- **Redaction.** Unchanged. The milestone adds no redaction and removes none.
- **Audit.** No new audit event. Laying out the user's own text is not an
  auditable action.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/tui.md` — "The composer's box: wrapped rows, a visual-row
    scroll, a pinned mode row": the wrap rule, the height formula and its two
    bounds, the scroll offset, the caret's cell, and the geometry module's
    ownership.
  - `architecture/overview.md` — the integration-test-file count row, which
    this milestone's new member moved.
- **Updated user-facing documentation.** `docs/tui.md` — "The composer's box
  grows with your text": the box's growth, the ten-row cap, and the
  scrolling.
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the module
  guide's statement that `src/tui/composer_geometry.rs` owns the visual-row
  geometry (`VisualLayout::new`, `cell_of_byte`, `byte_at`, `caret_position`,
  `row_count`), because a wrapped row is not a logical line, and that the
  word targets still come from `src/tui/text_geometry.rs`.
- **Static guards.** None added. The milestone adds no new claim, no new key,
  and no new call site for a guard to police. The existing
  `scripts/check_tui_composer_key_authority.py` was run **with its
  `--self-test`**, and every violation class was detected and rejected — the
  guard's coverage is unchanged by this milestone.
- **Operator diagnostics.** None. Nothing to diagnose.
- **Documentation obligation — resolved.** The `architecture/overview.md`
  integration-test-file count row was the gate's one red step
  (`check_test_target_references`) — a **documentation count**, not a
  behaviour. The row has been re-measured against the tree and corrected in
  the same working tree, and the step is green. Nothing is outstanding here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The composer's in-app verb labels still read "Move down one line" / "Move up one line"** (`src/tui/composer_claims.rs:267`–`:268`), while `docs/tui.md` describes the same gesture as "Move down / up **a row of the box**, keeping the column". | The motion is now a **visual-row** motion, so the in-app label is imprecise — the user is told "one line" and gets one visual row — and the in-app help and the user guide describe the same gesture **two different ways**. It is a user-visible string, and it was deliberately left alone rather than changed without the owner's decision. | **Flagged to the owner** as a wording choice: either re-spell the in-app labels to name the visual row (and bring them into agreement with `docs/tui.md`), or record that the Helix wording is kept deliberately. Not to be changed unilaterally: it is a user-visible string, and the label's own verb identity ("LineDown" / "LineUp") is bound into the guard's "every caret motion is bound the same way in both maps" rule. |
| medium | **The plain `wrap_to_strings` engine is still not escape-atomic.** | A tool output that contains an escape sequence can still be split by the plain word wrap. This milestone's composer path uses the **styled** primitive and is not affected; the gap is in the tool-output path. | **Pre-existing** and **outside this milestone's scope**. Recorded here so the line's records carry it once; the finding is owned by the main-buffer roadmap's M001 record. |
| low | **The geometry's bijection is over the rendered bytes, not over every byte of the buffer.** The primitive collapses a whitespace run to one space and drops the whitespace at a row break, so those bytes are on no cell. | The bytes the wrap does not render are on no cell; `caret_position` still answers for them. Nothing that consumes the geometry today needs a byte-exact mapping. | Accepted and recorded. If a future consumer needs a byte-exact mapping, it needs a second helper rather than a change to this one. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the geometry
module is in the tree, the widget paints wrapped rows, the height formula is
shipped, and the closure evidence is a **property over 13 texts × 6 widths**
rather than a set of examples. The **outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA today),
   and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and the
hosted lane is a strict **subset** of `quick`, so a green hosted run does not
by itself prove everything the local gate proves.

M002 of this roadmap depends on M001's interface, and that dependency is
**discharged**: the geometry module's API is fixed and in the tree, and M002
built its visual-row motions on it.

## 12. Registry updates

The parent owns `plans/registry.md` and the roadmaps; the changes below are
**specified** here, and the parent has **already applied them** in the same
working tree. The "as applied" lines record what the files actually say now,
so a reader can check the registry against this record rather than against
this specification.

- `plans/registry.md` — the "TUI composer text layout" row:
  - **Status** must read **conditionally closed**, and the row must name the
    baseline `f2639b5d2`.
  - The **milestone cell** must name this milestone's plan
    (`plans/implementation/tui-composer-text-layout/001-composer-word-wrap-and-visual-row-geometry.md`)
    **and** this closure record
    (`plans/closure/tui-composer-text-layout/001-status.md`), with the
    dependency and the class.
  - The **narrative** must record: that the composer now **wraps** and grows
    to a **ten-row total** (mode row + text + marker rows) with the content
    beyond the cap scrollable; that the 40 % budget **wins** over the cap and
    is **40 % of the terminal frame** — the plan and the roadmap call it the
    "pane budget", and the correction is verified against
    `src/tui/app/render.rs`; that the **mode row and marker rows are
    pinned**; that the horizontal scroll **stays** under the condition "a
    visual row wider than the box"; and that the geometry lives in the new
    `src/tui/composer_geometry.rs`.
  - The **summary paragraph** must record the conditional closure and its
    named outstanding evidence, and the two owner decisions pending — the
    imprecise in-app verb labels and the not-mode-gated page.
- **As applied.** `plans/registry.md` carries a **workstream row** for
  "TUI composer text layout" reading `**conditionally closed**`, and a
  **per-milestone table** whose rows read `**conditionally closed**` and name
  both the implementation plan and the closure record, with the dependency and
  the class and the baseline `f2639b5d2`. The roadmap's §13 "Milestone
  status" table reads `conditionally closed` for every milestone and carries
  the plan path and the closure path for each; the M002 row's dependency cell
  keeps `M001 (hard) — satisfied`; the roadmap also gained an **"Outcome
  (2026-10-23)"** paragraph naming the line as **conditionally closed** and
  naming the outstanding item — the hosted `CI / verify` lane, which has
  not run against the commit that carries this code.

- `plans/subsystems/tui-composer-text-layout-roadmap.md`:
  - §13 "Milestone status" must read `conditionally closed` for the M001 row,
    with the implementation-plan path and the closure-record path filled in.
    The **dependency cell is retained** rather than blanked — it records the
    main-buffer line's M001 as a *contract dependency* rather than a wait.
  - §5.2 "the height is logical, and the ceiling is a pane budget, not a row
    cap" must be corrected: the ceiling is **40 % of the terminal frame**, and
    the box now has a **ten-row total cap** in addition to it.
  - §5.3 "the geometry is logical everywhere" must be updated to record that
    the geometry is now **visual-row**, and that
    `src/tui/composer_geometry.rs` owns it.
  - §5.6 "the horizontal scroll's remaining role" must record P1's
    disposition: the scroll **stays**, and its condition is now "a visual row
    wider than the box".
  - §12 "Completion definition" must record that the milestone's *production*
    completion is in the tree while its *operational* evidence (the closing
    commit and the hosted run) is outstanding.
  - The roadmap must carry an **"Outcome (2026-10-23)"** paragraph naming the
    line as conditionally closed, naming the two outstanding items, and
    recording the delivered capability.
