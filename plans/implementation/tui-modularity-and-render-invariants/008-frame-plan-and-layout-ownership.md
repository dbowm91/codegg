# TUI Modularity and Render Invariants Milestone 008 — Frame plan and layout ownership

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-008--frame-plan-and-layout-ownership`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the TUI's
  regions present the state hierarchy; this milestone changes who computes the
  regions, not what they contain.

Applicable ADRs:

- None required. Internal geometry ownership inside one crate. No protocol,
  storage, authority or compatibility surface.

Primary class: infrastructure

## 1. Objective

Extract the arithmetic half of `App::render` into a pure `FramePlan` that
returns named regions, and retire the ten `Option<Rect>` fields from `App` into
it — so the carve-order invariant becomes a tested property instead of a
comment-maintained one.

This milestone delivers **no user-visible capability**. It is justified by
maintainability alone, and its acceptance evidence is that the corpus did not
move.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate) with soft dependencies on M002 and
M003: `FramePlan` does not need the chrome primitives, but the primitives make
its call sites simpler.

The layout machinery it builds on already exists and is separate:
`src/tui/layout.rs` owns `LayoutConfig`, `TuiLayout::split`, `session_layout` and
`observe_frame`. What is missing is a single value that owns the *result* of the
arithmetic, which is why the result currently lives in ten fields on `App`.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- `App::render` (`src/tui/app/render.rs:137`) is 428 lines. It computes the row
  budget, carves strips off the prompt chunk, assigns the resulting rects to
  fields, and then paints — in one function.
- `App` carries **ten `Option<Rect>` fields** whose only purpose is to carry
  this arithmetic between the painter and the mouse hit-test:
  `viewport_area`, `scrollbar_area`, `prompt_area`, `dialog_area`,
  `completion_area`, `todo_area`, `tasks_area`, `sidebar_area`,
  `left_border_area`, `bottom_border_area`. `panel_areas` holds a collection of
  the same kind.
- The ordering rule is load-bearing and comment-maintained. The function's own
  comments state it twice: rows must be carved top-down "in the same order the
  rows were added to `prompt_height`", and the tasks overlay "renders directly
  beneath" the todo strip so "their order in the allocation has to match their
  order on screen".
- The comments also record the bugs this rule has already caused: the outer
  bottom border painted over the status line because the footer's own last row
  was used as the anchor; the working strip that used to own the exit
  confirmation's expiry; the retired command palette whose hint the completion
  popup inherited.
- `App` has 25 `render_*` helper methods in `render.rs`. The painter set is
  reasonably decomposed; the arithmetic is not.
- `scripts/check_tui_render_cost.py` pins the render path's *shape* — one
  producer per derived result — and is a source check rather than a benchmark.

## 4. Invariants that must not regress

- **The corpus is byte-identical.** This milestone has no intended visual
  change. Any movement is a defect in the extraction.
- **The carve order is preserved exactly**, including the yield rules: a strip
  that does not fit yields rather than shrinking its neighbour, and the
  transcript keeps its floor.
- **`App::render` still paints.** The milestone moves arithmetic, not painting;
  a painter that starts computing is a regression.
- **The render-cost shape guard passes unmodified.** `FramePlan` must not
  introduce a second derivation of any measured result.
- **Mouse hit-testing still resolves.** The ten rects are read after the frame;
  replacing them with one value must keep every hit test working.
- **The comment ratchet holds.** `app/render.rs` and `app/mod.rs` ceilings apply;
  a split requires a bank.

## 5. Scope

### In scope

- A `FramePlan` value with named regions for every rect the frame composes.
- A pure planning function: given the frame area, the occupied panel
  placements, and the row claims (todo, tasks, tabs, hint, prompt height), it
  returns the plan.
- Retiring the ten `Option<Rect>` fields into one `frame_plan: FramePlan` on
  `App`, retained for hit-testing.
- Unit tests for the ordering rule and the yield rules.
- Byte-identical corpus.

### Explicitly out of scope

- Splitting `App` beyond frame geometry. The remaining ~105 fields are a
  successor roadmap (§3 of the roadmap).
- Changing any layout constant, percentage or claim rule.
- User-resizable regions or remembered splits.
- Any painter rewrite. Painters change only to take a named region instead of
  a field.

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

- `FramePlan` in `src/tui/layout.rs` with named fields for each region the
  frame composes, including the panel regions.
- A `plan_frame(...)` function that is pure: no `&mut App`, no painting, no
  I/O. Its inputs are the frame area, the occupied placements, and the row
  claims; its output is the plan.
- `App::render` calls `plan_frame`, stores the plan, and paints from it.
- Mouse hit-testing reads `self.frame_plan.<region>` instead of
  `self.<region>_area`.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the carve-order rule stated once, as a property of
  `plan_frame`, replacing the two prose statements inside `App::render`.
- `architecture/testing.md` — the ordering test, so a later change knows the
  rule is enforced rather than described.

## 7. Ordered work packages

### Work package A — Characterise the current arithmetic

Intent: pin the behaviour before moving it.

Required changes: tests that assert, for a matrix of terminal sizes and claim
combinations, the current rect for each region. These are characterisation
tests against the existing code and they pass on day one.

Acceptance evidence: the matrix covers at least 120×40, 80×24, 40×12, 20×6 and
the combinations of no claims, todo only, tasks only, both, and both plus the
tab strip.

### Work package B — `FramePlan` and `plan_frame`

Intent: one value and one pure function.

Required changes: move the arithmetic into `plan_frame` with no behavioural
change; `App::render` keeps painting, now from the plan.

Acceptance evidence: the characterisation tests from A pass unchanged, and the
corpus is byte-identical.

### Work package C — Retire the fields

Intent: one owner of frame geometry.

Required changes: the ten `Option<Rect>` fields are removed; hit-testing reads
the plan.

Acceptance evidence: every hit test passes; a test asserts that a click on the
prompt area still focuses the prompt and a click on the viewport still scrolls.

### Work package D — Ordering and yield tests

Intent: make the comment-maintained rule executable.

Required changes: tests asserting regions never overlap, that their union
covers the pane, that each claim is honoured or yielded in the documented order,
and that the transcript floor is respected when claims exceed the pane.

Acceptance evidence: a deliberately reordered carve fails the test — proving it
detects the class of bug the comments describe.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Planning is synchronous and pure. The one hazard is a plan that
depends on state read at paint time rather than passed in, which would
reintroduce the coupling the milestone removes. `plan_frame`'s signature must
carry every input, and a test asserts that calling it twice with the same
inputs returns an equal plan.

## 9. Compatibility and migration

No compatibility surface. Layout constants, claim rules and visible geometry are
unchanged, so a user sees an identical frame. The migration is internal: ten
fields become one, and every reader is updated in the same change rather than
left with a fallback.

## 10. Required tests

### Focused unit tests

- `plan_frame` is pure and idempotent for equal inputs.
- Each region's rect matches the characterisation matrix.
- The yield rules: a claim that does not fit yields, and its neighbour is not
  shrunk.

### Integration tests

- The corpus is byte-identical across the matrix.
- Hit-testing resolves for prompt, viewport, todo strip, tasks strip, sidebar
  and panels.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A claim set larger than the pane produces a valid plan with no overlap and no
  negative height, rather than a panic or an inverted rect.

### Migration and compatibility tests

- The characterisation matrix from work package A passes before and after,
  unchanged, which is the proof that the extraction was behaviour-preserving.

## 11. Required verification commands

```bash
# the ordering and characterisation tests
cargo test --lib -- tui::layout
cargo test --test tui_render -- tui_render_invariants

# the frame and row-accounting surfaces
cargo test --test tui_render -- tui_main_buffer_row_accounting
cargo test --test tui_render -- tui_main_buffer_collapsed_layout
cargo test --test tui_render -- tui_frame_cost
cargo test --test tui_render -- tui_panel_manager_reachability

# the render-cost shape guard must pass unmodified
python3 scripts/check_tui_render_cost.py

# the comment ratchet, since render.rs may be split
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the carve-order rule as a property of `plan_frame`.
- `architecture/testing.md` — the ordering test.
- `docs/comment-budget.toml` — ceilings if `render.rs` is split.

## 13. Acceptance criteria

1. `plan_frame` is pure, and its signature carries every input.
2. The characterisation matrix passes before and after, unchanged.
3. The corpus is byte-identical.
4. The ten `Option<Rect>` fields are gone and hit-testing reads the plan.
5. The ordering and yield tests exist and are proved to detect a reordering.
6. `scripts/check_tui_render_cost.py` passes unmodified.
7. `scripts/verify.sh quick` is green.

## 14. Stop conditions

Report rather than improvise when:

- the arithmetic turns out to depend on paint-time state that cannot be passed
  in without widening the signature into something that reads `App` — report the
  dependency rather than passing `&App`;
- a region's rect cannot be characterised because it is set on some frames and
  not others in a way the matrix does not cover;
- the extraction requires changing a layout constant to keep the corpus
  identical — that means the constants and the arithmetic disagree today;
- the render-cost guard fails, which would mean the extraction added a second
  derivation.

## 15. Closure evidence required

- The characterisation matrix, passing before and after.
- The corpus result, explicitly byte-identical.
- The ordering test, with its deliberate-reordering failure recorded.
- Confirmation that the ten fields are removed, with the hit-test results.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/008-status.md`.

## 16. Handoff notes

Write the characterisation tests first and make them pass before moving a line.
The entire value of this milestone is that the frame does not change, and
without a matrix that is an assertion rather than a measurement.

The comments in `App::render` are the specification. They describe four
distinct layout bugs that were fixed by hand, and each one is a case the
ordering test should cover. Read them before designing the test, not after.

Resist the temptation to also fix the ~105 remaining `App` fields. This
milestone is the frame-geometry cluster because `FramePlan` is exactly the
contract that absorbs it. Anything wider needs its own roadmap, and the roadmap
says so.
