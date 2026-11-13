# TUI Modularity and Render Invariants Milestone 008 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/008-frame-plan-and-layout-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-008--frame-plan-and-layout-ownership`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. `src/tui/layout.rs` and
  `src/tui/app/render.rs` are modified in the tree; there is no commit to diff
  them against beyond HEAD.

Labels: **measured** is what this record's author read out of the tree;
**reported** is the implementing session's measurement, not repeated here (the
corpus is expensive and three sessions were compiling concurrently).

## 1. Executive finding

The extraction landed and it is the shape the plan asked for. `src/tui/layout.rs`
now owns four values:

| Value | Line | Role |
|---|---|---|
| `FrameClaims` | `:232` | the row **asks** for one frame — todo, hint, tab, tasks, present, prompt; documented as "the asks, not the answers" |
| `FrameOuter` | `:255` | the frame area, the panel placements, the pane and the two borders |
| `FramePlan` | `:289` | the planned rects (pane, borders, header, footer, composer, prompt, viewport, tab bar, hint, todo, tasks, present, scrollbar, completion) |
| `plan_frame` | `:543` | `pub fn plan_frame(&self, outer: FrameOuter, claims: FrameClaims) -> FramePlan` |

`plan_frame` is pure in the sense the plan requires: "Every input is a number, a
placement, or the layout's own configuration; nothing here reads `App`, paints,
or touches the world, and equal inputs return equal plans." Its `&self` is
`TuiLayout`, i.e. `LayoutConfig` plus the recorded `frame_height` that it does
not read here — so the plan's stop condition ("report the dependency rather than
passing `&App`") was not reached. `frame_outer` (`:499`) is the separate half
that decides the panel edges and the pane, so the one function did not become a
grab bag.

**Eight of the ten `Option<Rect>` fields are gone.** Measured: `App` now declares
exactly two — `dialog_area` (`src/tui/app/mod.rs:399`) and `sidebar_area`
(`:454`) — and the other eight (`viewport_area`, `scrollbar_area`,
`prompt_area`, `completion_area`, `todo_area`, `tasks_area`, `left_border_area`,
`bottom_border_area`) have no field declaration and no `self.<name>` reference
anywhere under `src/tui`. Hit-testing reads the plan: `clickable_area_at`
(`:6233`) answers completion, prompt, viewport and scrollbar from
`self.frame_plan`.

So **acceptance criterion 4 — "The ten `Option<Rect>` fields are gone" — is
8 of 10, not 10 of 10.** That is the headline condition on this record. It is
not a regression: the two survivors are read by mouse hit-testing and hover
(`clickable_area_at`, `on_click` at `:6307`, `on_scroll_up` at `:6459`), each of
which already consults `frame_plan` for the other regions, so the plan value is
in place and these two simply have not been moved onto it. The plan and the
roadmap anticipated *some* of this: the plan's §5 defers "the remaining ~105
fields" to a successor, and the roadmap's §10 records "**Splitting `App` beyond
geometry is deferred, not solved.** §3 records it. A successor roadmap must own
it". Neither document records that two `Option<Rect>` fields specifically
survive, so the 8-of-10 figure is this record's measurement.

**The briefing's attribution, re-measured — and the record's own correction of
it corrected again.** The briefing states the two remaining fields are "blocked
by `src/tui/commands/**`". When this record was written a grep for the two names
under that path returned nothing, and §10 recorded the attribution as false. **On
the current tree that no longer holds:**

- `src/tui/commands/panels.rs` reads `App::sidebar_area` in production. It
  destructures the field (`:256`, `:830`, `:1252`, `:1304`), passes
  `*sidebar_area` as the panel context's grant (`:267`, `:841`, `:1263`,
  `:1317`), and `panel_ctx` takes it as a parameter (`:1338`) and unwraps it into
  `PanelCtx.area` (`:1356`).
- `src/tui/commands/interactive_terminal.rs:3262` reads `App::dialog_area` from a
  test helper's panic message.

So the command family modules **do** read both fields today, and retiring either
onto `frame_plan` would have to update them. The original measurement is left
visible here rather than replaced, because the discrepancy is the point: an
attribution has to be re-measured against the tree it is repeated into, and this
record's earlier "grep returns nothing" is itself an instance of the rule it was
asserting. The readers that were listed — `src/tui/app/render.rs` (assignment)
and `src/tui/app/mod.rs` (the mouse paths) — are still readers; they are no
longer the only ones.

**Two further observations about how the acceptance evidence was gathered.**
The plan's work package A says the characterisation matrix must "pass on day
one", written **before** any line moved, and acceptance criterion 2 says it
passes "before and after, unchanged". The matrix exists and is thorough — nine
tests, including a deliberate reordering — but nothing in the tree records its
state at the moment before the extraction, so "before and after, unchanged" is
**asserted by the tests' existence, not demonstrated**. The tests that carry the
weight are listed in §2; the ordering test genuinely bites (it fails a swapped
carve), which is the property that matters most, and it is measured here by
reading the assertion, not by running it.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `FramePlan` with named regions for every rect the frame composes | `layout.rs:289-330`, 16 named fields including `panels`, `pane`, both borders, `header`, `footer`, `composer`, `prompt`, `viewport`, `tab_bar`, `hint`, `todo`, `tasks`, `present`, `scrollbar`, `completion` | pass | Three regions are documented as recorded by their painters (scrollbar, completion) or by `App::dialog_rect` — one derivation each, not a second field. |
| A pure `plan_frame` whose signature carries every input | `layout.rs:543`; doc comment: "Pure. Every input is a number, a placement, or the layout's own configuration; nothing here reads `App`, paints, or touches the world, and equal inputs return equal plans" | pass | Implemented as an inherent method on `TuiLayout`, so config is an input by `&self` rather than a parameter. |
| `plan_frame` is idempotent for equal inputs | `a_plan_is_a_function_of_its_inputs` (`layout.rs:855`) | pass (test present) | Not run by this record. |
| `App::render` paints and does not compute | `render.rs` is 3,790 lines and calls `plan_frame` through `frame_outer`; `dialog_rect` is the remaining derivation and is documented as one place | pass | |
| Retire the ten `Option<Rect>` fields | 8 retired; `dialog_area` and `sidebar_area` survive | **partial (8/10)** | §1. Criterion 4 is not met as written. |
| Hit-testing reads the plan | `clickable_area_at` reads `plan.completion`, `plan.prompt`, `plan.viewport`, `plan.scrollbar`; `on_click` and `on_scroll_up` read the plan plus the two surviving fields | **partial** | The dispatch is on the plan for the frame's own regions; the two survivors are still fields. |
| The carve order is preserved exactly | `the_carve_order_is_the_order_on_screen` (`:880`) and `the_tab_bar_is_charged_to_the_transcript_not_the_composer` (`:980`) | pass (tests present) | The rule is now stated in `plan_frame`'s own doc comment, replacing the two prose statements in `App::render` — the plan's §6 documentation requirement, delivered as a doc comment. |
| A deliberately reordered carve fails the test | `a_swapped_carve_puts_a_strip_on_the_wrong_row` (`:918`) | pass (test present) | This is the plan's work package D acceptance evidence, and the test is written to fail on a swap rather than to describe the right order. |
| A claim that does not fit yields rather than shrinking its neighbour | `carve_top` (`:335`, one implementation) and `a_claim_that_does_not_fit_yields_instead_of_taking_the_last_row` (`:935`) | pass | Yield, never shrink, is implemented once. |
| The transcript keeps its floor when claims exceed the pane | `the_second_column_yields_to_the_transcript_floor` (`:764`) and `claims_larger_than_the_pane_produce_a_valid_plan` (`:1034`) | pass (tests present) | |
| The tab bar is charged to the transcript's top, not the composer's | `the_tab_bar_is_charged_to_the_transcript_not_the_composer` (`:980`); `the_tab_bar_wins_the_last_row_over_the_overlay` (`:1014`) | pass | The plan's explicit double-charge hazard. |
| Regions never overlap and their union tiles the pane | `the_plan_tiles_the_pane_across_the_size_and_claim_matrix` (`:1071`) | pass (test present) | A matrix rather than one size, as the plan's work package D asks. |
| A claim set larger than the pane produces a valid plan, no negative height | `claims_larger_than_the_pane_produce_a_valid_plan` (`:1034`) | pass (test present) | |
| Panel edges and the border narrow the pane the claims are measured against | `a_panel_edge_and_the_border_narrow_the_pane_the_claims_are_measured_against` (`:1136`) | pass (test present) | |
| The characterisation matrix passes **before and after, unchanged** | nine `layout.rs` tests, `cargo test -p codegg --lib -- tui::layout` = **19 passed / 0 failed (reported)** | **partial** | The tests exist and pass; that they were unchanged across the move is not recorded anywhere in the tree. |
| The corpus is byte-identical | **reported**: `cargo test --test tui_render` = **1224 passed / 0 failed**, so the extraction did not move the corpus | pass (reported) | Not re-measured here. |
| `scripts/check_tui_render_cost.py` passes unmodified | ran: "the render path derives each result once", exit 0; guard unmodified in `git status` | pass | The plan's stop condition for "the extraction added a second derivation" did not trigger. |
| The comment ratchet holds, with `app/render.rs` banked if split | `check_comment_budget.py` passes inside `scripts/verify.sh full` on this tree (2026-11-13), with the corpus banked at `max_comment_lines = 139245` | pass | Verified for this correction; the split's ceilings are covered by the green guard. |
| `architecture/tui.md` states the carve order once, as a property of `plan_frame` | `architecture/tui.md:3516-3536` — "The carve-order rule is a property of `plan_frame`, and it is stated once", plus the purity paragraph at `:3536` | pass | Verified for this correction. |
| `architecture/testing.md` names the ordering test | `architecture/testing.md:1428-1450`, "The frame plan's ordering test", naming `the_carve_order_is_the_order_on_screen` and `a_swapped_carve_puts_a_strip_on_the_wrong_row` | pass | Verified for this correction. |
| `scripts/verify.sh quick` green | not run at this pass; `scripts/verify.sh full` 2026-11-13 passed, and `run_full()` calls `run_quick()` | **pass, 2026-11-13** | §10. |

## 3. Production implementation evidence

- `src/tui/layout.rs` (1,153 lines) — `PanelRegions`, `LayoutConfig`,
  `FrameClaims` (`:232`), `FrameOuter` (`:255`), `FramePlan` (`:289`),
  `carve_top` (`:335`), `TuiLayout::{split, session_layout, frame_outer,
  plan_frame}`, plus nine planning tests from `:822` onwards.
- `carve_top` is the single place the yield rule is implemented, with the rule
  in its doc comment: "a strip that took the last one would leave its neighbour
  — the input box or the transcript — zero rows tall. Yield, never shrink the
  neighbour, in the one place it is implemented."
- `src/tui/app/render.rs` — `App::render` computes less: the plan is stored on
  `App` and the painters read named regions. `dialog_rect` (`:3136`) remains the
  one rect the plan does not decide, and `FramePlan`'s own doc comment says so
  ("a dialog's rect is `App::dialog_rect`'s — one derivation that the painted box
  and the clickable box both read").
- `src/tui/app/mod.rs:399`, `:454` — the two surviving `Option<Rect>` fields and
  their readers at `:6118`, `:6234`, `:6259`, `:6348`, `:6378`, `:6465`.

Nothing in the plan's "explicitly out of scope" list was implemented: no layout
constant, percentage or claim rule changed; there is no resizable region and no
remembered split; and no painter was rewritten beyond taking a named region.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_invariant_baseline.py

grep -n 'pub struct FramePlan|pub struct FrameOuter|pub struct FrameClaims|pub fn plan_frame' src/tui/layout.rs
grep -n 'Option<Rect>' src/tui/app/mod.rs
grep -rn '_area: Option<Rect>' src/tui
grep -rn 'self.viewport_area|self.prompt_area|self.scrollbar_area|…' src/tui
grep -n 'fn ' src/tui/layout.rs
grep -n 'pub fn ' src/tui/layout.rs
wc -l src/tui/layout.rs src/tui/app/render.rs
```

### Results

| Command | Result |
|---|---|
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once", exit 0 (measured) |
| `check_tui_render_invariant_baseline.py` | **pass** — 61 / 2495 / 0, exit 0 (measured) |
| `FramePlan`/`FrameOuter`/`FrameClaims`/`plan_frame` grep | present at `:289`, `:255`, `:232`, `:543` (measured) |
| `Option<Rect>` on `App` | exactly 2 — `dialog_area`, `sidebar_area` (measured) |
| Old field references | zero (`self.viewport_area` and the other seven do not occur) (measured) |
| `layout.rs` size | 1,153 lines; `render.rs` 3,790 lines (measured) |
| `layout.rs` test count | 9 planning tests + 10 geometry/pane tests = 19 under `-- tui::layout` (matches the reported 19) (measured by counting `#[test]`s in the module; the run itself is reported) |

### Not run

| Command | Why |
|---|---|
| `cargo test -p codegg --lib -- tui::layout` | 19 passed / 0 failed is **reported**; this record counted the tests instead of compiling, by instruction. |
| `cargo test --test tui_render` | 1224 passed / 0 failed is **reported**; the corpus was not re-run. |
| `cargo test --test tui_render -- tui_main_buffer_row_accounting` (and the three sibling selectors) | Same reason. Note the plan's selectors are family members and must be selected as `--test tui_render -- <member>`. |
| `cargo fmt --check`, clippy, `scripts/verify.sh quick` | Out of scope and forbidden for this session. |

## 5. Invariant review

- **The corpus is byte-identical.** Reported green at 1224/0. Not re-measured.
  Any movement would be a defect in the extraction, so the reported result is
  the milestone's central evidence — and it is reported, not verified here.
- **The carve order is preserved exactly, including the yield rules.** Holds by
  construction: one `carve_top`, one ordering expressed by the sequence of calls
  in `plan_frame`, and a test that fails a swapped carve. The comment-maintained
  rule became an executable one, which is the milestone's whole point.
- **`App::render` still paints.** Holds; the arithmetic moved, and painters take
  named regions.
- **The render-cost shape guard passes unmodified.** Holds — measured.
- **Mouse hit-testing still resolves.** Holds for the plan's regions; the two
  surviving fields are read by the same three handlers, so the invariant is met
  while criterion 4 is not.
- **The comment ratchet holds.** **Verified 2026-11-13**: `check_comment_budget.py`
  passes inside `scripts/verify.sh full`, with the corpus banked at
  `max_comment_lines = 139245`. `layout.rs`'s split out of `render.rs` is covered
  by that green guard.

## 6. Failure and recovery review

- **No runtime path.** Planning is synchronous and pure.
- **The plan's one named hazard** — a plan that depends on paint-time state —
  does not exist: `FrameClaims` and `FrameOuter` carry every input, `plan_frame`
  reads only `self.config`, and `a_plan_is_a_function_of_its_inputs` asserts
  equality for equal inputs.
- **Degenerate frames.** `claims_larger_than_the_pane_produce_a_valid_plan`
  asserts a valid plan with no overlap and no negative height, which is the
  plan's §10 negative test; `saturating_sub` appears throughout the claim
  arithmetic.
- **No cancellation, restart or persistence.** Nothing to review.
- **Failure visibility.** A carving regression names the strip and the row in
  `a_swapped_carve_puts_a_strip_on_the_wrong_row`, so a future layout bug of the
  class the old comments described is a failing test rather than a screenshot.

## 7. Migration and compatibility review

No schema, protocol, configuration or dependency surface, and no visible
geometry change: layout constants, claim rules and the resulting frame are
unchanged by contract, so a user sees an identical frame. The migration is
internal and, in eight cases, complete — no field was left behind a fallback,
which is what the plan required. The two survivors are a partial migration, not
a fallback: they are still the single source for their region's rect.

Rollback is reverting `layout.rs`'s planning half and the `App` field changes.
There is no durable state and no user setting.

One compatibility note: `FramePlan` is `pub` inside the crate's `tui::layout`
module, so the `scrollbar`/`completion` regions are part of an internal contract
with painters. It is not a public API and not a serialized shape.

## 8. Security review

Not applicable in the authorization sense: no path, process, secret, network or
permission surface is touched. The bounded-input property that matters — a claim
set larger than the pane must not underflow a `u16` into a huge rect — is asserted
by the oversized-claim test, and the arithmetic saturates rather than wrapping.

## 9. Documentation and operations

- `plan_frame`'s doc comment now states the carve order and the yield rules in
  one place, which is where the plan's §12 documentation requirement best lives
  and is delivered. `architecture/tui.md:3516-3536` states the same rule as a
  property of `plan_frame`, with the purity contract at `:3536`.
- `architecture/testing.md:1428-1450` names the ordering test and both of its
  assertions, so a later change knows the rule is enforced rather than described.
- `docs/comment-budget.toml` — the plan asks for ceilings if `render.rs` is
  split, and it was split (1,153 lines moved out). The guard is green with the
  corpus banked at 139,245 lines.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Two of the ten `Option<Rect>` fields survive: `dialog_area` (`app/mod.rs:382`) and `sidebar_area` (`:437`). Acceptance criterion 4 reads "The ten `Option<Rect>` fields are gone and hit-testing reads the plan", and it is **8 of 10**. | Frame geometry has two owners instead of one. A reader of the landmark claim is told the cluster was retired when a small part of it was not, and the plan's own justification ("the carve-order invariant becomes a tested property") is met for the frame's regions but not for the dialog box or the sidebar column. | **OPEN OWNER QUESTION — not decided by this record.** `dialog_area` and `sidebar_area` are deferred while eight of the ten fields are retired. So: is **8 of 10 the accepted end state** for criterion 4? What would settle it: an owner yes/no. A yes should amend criterion 4 and the roadmap's §11 item 6 to say eight; a no makes the two the remaining bounded work — `dialog_area` is already a single derivation at `dialog_rect`, and the `sidebar_area` read now includes `src/tui/commands/panels.rs`, so the move touches four readers, not three. |
| low | The attribution question — "blocked by `src/tui/commands/**`" — is **not false on the current tree**: `src/tui/commands/panels.rs` reads `App::sidebar_area` in production (`:256`, `:267`, `:830`, `:841`, `:1252`, `:1263`, `:1304`, `:1317`, `:1338`, `:1356`) and `src/tui/commands/interactive_terminal.rs:3262` reads `App::dialog_area` from a test helper. The record's earlier "grep returns nothing" measured an earlier tree state. | Repeating either the briefing's attribution *or* this record's denial without re-measuring sends the next reader to the wrong module. | Recorded in §1 with the measured readers: the command family modules, the three mouse paths in `app/mod.rs`, and the assignment sites in `render.rs`. |
| low | "The characterisation matrix passes before and after, unchanged" is not demonstrated: no tree artifact records the matrix's state before the extraction. | Criterion 2 is asserted rather than evidenced; a test edited *during* the move would be indistinguishable from one written first. | Accept the structural reading (the tests are written as assertions against a pure function, and the reordering test can only be written by someone who knows both orders), or land the matrix as its own commit next time. |
| low | The comment-budget bank for the `render.rs` → `layout.rs` split was not inspected. | The plan's invariant "the comment ratchet holds; a split requires a bank" was unverified. | **Verified 2026-11-13**: `check_comment_budget.py` passes inside `scripts/verify.sh full`, corpus banked at 139,245 lines. |
| low | `architecture/tui.md` and `architecture/testing.md` were not checked for the carve-order statement and the ordering test. | Two plan-mandated documentation items may have been absent. | **Verified present**: `architecture/tui.md:3516-3536` states the carve order once as a property of `plan_frame`; `architecture/testing.md:1428-1450` names the ordering test and its two assertions. |
| low | `scripts/verify.sh quick` and `cargo fmt --check` were not run; the tree is uncommitted. | Criterion 7 had no evidence at that pass. | **Discharged 2026-11-13**: both ran inside `scripts/verify.sh full` and passed. The commit is still outstanding. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding**,
plus one bounded corrective item on acceptance criterion 4.

The conditional element: the `quick`/`fmt --check` half is **met** —
`scripts/verify.sh full` passed on this tree at 2026-11-13 — and what remains is
the characterisation matrix's before/after claim, which no tree artifact records
and which cannot be reconstructed retrospectively. The corrective element is the
two remaining `Option<Rect>` fields: **8 of 10 is the measured state**, and
whether that is the accepted end state for criterion 4 is an **open owner
question** (§10), not a decision this record takes. The roadmap's §10 already owns
the wider successor ("Splitting `App` beyond geometry is deferred, not solved. §3
records it. A successor roadmap must own it"), so the residual is a named item in
an owner's list rather than an orphan.

What the next milestone may rely on unchanged: `FramePlan` and `plan_frame` as
the single owner of frame geometry, the nine planning tests, and the yield rule
in `carve_top`.

## 12. Registry updates

- `plans/registry.md` line 29 (subsystem row): M008's status among "M005–M010 in
  progress or pending" needs its conditional status named.
- `plans/registry.md` line 686 (the M008 milestone row): status "**Blocked —
  hard dependency on M001; soft on M002 and M003**" → conditionally closed, with
  this record's path and the 8-of-10 finding.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12 row
  `008`: `blocked` → `conditionally closed`, closure record
  `plans/closure/tui-modularity-and-render-invariants/008-status.md`, blockers
  "green `quick` on the merged tree; the two surviving `Option<Rect>` fields
  (`dialog_area`, `sidebar_area`); the characterisation matrix's before/after
  evidence".
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §11 item 6
  ("`FramePlan` owns frame geometry including the ten retired `Option<Rect>`
  fields") — the ten are eight; item 6 should say so until the two move.
- `plans/implementation/tui-modularity-and-render-invariants/008-frame-plan-and-layout-ownership.md`
  header: `Status: blocked` → `Status: conditionally closed`. **This record's
  author made this header edit**, following plans 001–003 in this line.

## Correction log (2026-11-13)

- **§1, the briefing-attribution paragraph.** The record's "neither `dialog_area` nor `sidebar_area` is referenced anywhere under `src/tui/commands/` (grep returns nothing)" does **not hold on the current tree**: `src/tui/commands/panels.rs` reads `App::sidebar_area` in production (`:256`, `:267`, `:830`, `:841`, `:1252`, `:1263`, `:1304`, `:1317`, `:1338`, `:1356`) and `src/tui/commands/interactive_terminal.rs:3262` reads `App::dialog_area` from a test helper. The paragraph now records both measurements — the original and the current — and says the attribution is **not** false here. Note: the plan's §3 truth column states the opposite ("nothing under that path references …; the attribution is false"); that claim did not survive verification against this tree and was not used.
- **§2, the comment-ratchet row.** Was "not measured". Met by: `check_comment_budget.py` green inside `scripts/verify.sh full`, corpus banked at 139,245 lines.
- **§2, the `architecture/tui.md` and `architecture/testing.md` rows.** Were "not measured". Met by the tree: `architecture/tui.md:3516-3536` states the carve order once as a property of `plan_frame`; `architecture/testing.md:1428-1450` names the ordering test.
- **§2, the `quick` row.** Was "not run". Met by the 2026-11-13 `full`.
- **§5, the comment-ratchet bullet.** Met by the same green guard.
- **§9, the three documentation bullets.** Replaced "not checked in this pass" with the verified locations.
- **§10, the two-surviving-fields row.** The required action is now an **open owner question**: `dialog_area` and `sidebar_area` stand retired-or-not at **8 of 10** — is 8 of 10 the accepted end state for criterion 4? The record does not decide it.
- **§10, the attribution row.** Rewritten from "the attribution is false" to the measured truth; the readers now include `src/tui/commands/panels.rs`.
- **§10, the comment-bank, documentation and `quick` rows.** Discharged as above; the commit remains outstanding.
- **§11, the disposition.** The gate half of the conditional element is recorded as met; the remaining conditional element is the characterisation matrix's before/after claim, and criterion 4 is an open owner question.

**No promotion.** The status stays `conditionally closed`: criterion 4 is 8 of 10 and undecided, the characterisation matrix's before/after claim is not evidenced, and the tree is uncommitted. No condition was deleted without naming what met it.
