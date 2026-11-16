# TUI Modularity and Render Invariants Milestone 004 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/004-content-sized-modals.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-004--content-sized-modals`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same tree carries other TUI
  milestones (M001–M003, M005, M008, M010) and the Helix editor line, so no
  statement below is attributable to a SHA.

Two provenance labels are used throughout: **measured** means this record's
author read it out of the tree or ran the command at the stated tree state;
**reported** means the implementing session measured it and this record did not
repeat the measurement (the render corpus is expensive and three other sessions
were compiling in the same target directory).

## 1. Executive finding

The seam is delivered and it is the milestone's own shape rather than an
approximation of it. `Component::preferred_area` exists with the **legacy
percentage as its default** (`src/tui/components/component.rs:262` returns
`centered_rect(60, 50, available)`), one clamp helper owns the floor and the
ceiling (`clamp_area`, `src/tui/components/dialog_size.rs:25`), and it has
exactly **one production caller** — `App::dialog_rect`
(`src/tui/app/render.rs:3136-3143`), which is now described in its own doc
comment as "the one place a dialog's rect is decided: a stated preference,
clamped". The ad-hoc `DialogType::ProjectPicker` branch in the sizing path is
gone; `ProjectPickerDialog` states a preference like every other adopter
(`src/tui/components/dialogs/project_picker.rs:61-64`).

Six dialogs adopted the seam: **Confirm, Goto, Import, Info, Question and
ProjectPicker** (measured — the six `preferred_area` implementations under
`src/tui/components/dialogs/`). Confirm at 24 rows no longer receives three
content rows for two options, and a test asserts the drawn frame contains
`Yes`, `No` and `Enter confirm`
(`both_options_and_the_keymap_fit_the_stated_preference_at_24_rows`).

Status is conditional, not closed, for four reasons, and only one of them is a
gap in the milestone's own work:

1. **`PermissionDialog` did not adopt the seam.** Work package D names it
   explicitly ("Confirm, Permission, Question, Import and Goto derive their
   preference from their section count and row budget"); `permission.rs` has no
   `preferred_area` implementation at all (measured by grep), so the permission
   prompt keeps today's 60% × 50% rect. This is not an acceptance-criterion
   failure — the seven criteria do not name Permission — but it is an in-scope
   work item that is absent, and a reader of the plan deserves to be told
   rather than left to infer adoption from the seam's existence.
2. **The "corpus is byte-identical" control case cannot be re-measured.**
   The control case was a comparison against a pre-change corpus, and that
   before-state does not exist anywhere in this tree: there is no commit, and
   the `.inc` corpus files are already modified in the working tree by later
   milestones. The result is **reported** unchanged by the implementing session
   and is not independently reproducible here.
3. **`tui_info_dialog_panel_chrome` is not "unmodified".** Its corpus file —
   `tests/tui_render/tui_info_dialog_panel_chrome.inc` — is modified in this
   working tree (measured, `git status`), so acceptance criterion 6 ("passes
   unmodified") cannot be confirmed by inspection. The modification is not
   attributable to this milestone from the tree alone; M002 and M003 both touch
   dialog chrome.
4. **`scripts/verify.sh quick` was not run at this pass, and the tree is not
   committed.** The gate is since discharged: `scripts/verify.sh full` passed on
   this tree at 2026-11-13 (17,377 workspace tests, 8,441 feature-pass tests, 0
   failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean; corpus
   banked at 139,245 comment lines). What remains is the commit, not a gate.
   That is outstanding provenance, not a milestone defect.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A `preferred_area(&self, available: Rect) -> Rect` seam on `Component`, defaulting to the legacy popup | `src/tui/components/component.rs:262`; doc comment states the default is "the legacy 60% × 50% popup, so a dialog that does not override this is correct by construction" | pass | |
| The default reproduces `centered_rect(60, 50)` exactly | `the_default_preference_is_the_legacy_percentage_unchanged`: three sizes compare `preferred_area` against `centered_rect`, assert the clamp does not move it, and pin 80×24 to `Rect::new(16, 6, 48, 12)` | pass | 60 % of 80 and 50 % of 24, centred — the arithmetic, not a snapshot. |
| `App::render_dialog` consults the seam and stops special-casing the picker's **area** | `App::dialog_rect` (`src/tui/app/render.rs:3136-3143`) is the only reader; `render_dialog` takes the resulting rect | pass | `render_dialog` still branches on `DialogType::ProjectPicker`, but for the *body* (the picker draws from App-owned state), not the rect. |
| One clamp helper, one place, applied by the host rather than by each dialog | `clamp_area` (`dialog_size.rs:25`); production callers: one (`render.rs:3138`) | pass | All other `clamp_area` occurrences are in `#[cfg(test)]` modules. |
| Clamp floor 20×6 and ceiling 90 %, including a degenerate frame | `the_clamp_keeps_a_stated_preference_inside_the_frame` at 120×40, 80×24, 40×12, 20×6, 10×3 and 1×1, plus an 80×24 → `(4, 1, 72, 21)` assertion that pins the 90 % cap; `a_frame_smaller_than_the_minimum_gets_the_whole_frame` | pass | A 400×400 claim in a 1×1 frame stays inside and is never empty. |
| A zero-height preference gets the minimum, not an empty rect | `a_preference_below_the_minimum_gets_the_minimum_not_an_empty_rect` → `Rect::new(30, 9, MIN_DIALOG_WIDTH, MIN_DIALOG_HEIGHT)` | pass | |
| `preferred_area` does not read the rect it was given | `a_stated_preference_is_idempotent_for_the_same_frame`, which also pins Confirm's height to its three `Section::owned_height`s | pass | The plan's oscillation hazard, asserted. |
| Adoption: Confirm, Question, Import, Goto, the Info family, ProjectPicker | `every_adopting_dialog_renders_inside_its_clamped_preference` iterates exactly those six at 80×24, 120×40 and 40×12 | **partial** | **Permission is missing** (§1, §10). |
| Adoption: **Permission** | no `preferred_area` in `src/tui/components/dialogs/permission.rs` (grep) | **fail** | In-scope per work package D; bounded — the legacy default is correct, just not content-derived. |
| Every adopting dialog renders inside its clamp at 3 sizes, with I1 and I2 green | Same matrix asserts `inside(rect, screen)`, `painted_outside(buf, rect) == None` and `painted_inside(buf, rect) > 0` per surface per size | pass | I1 and non-blank I2 are asserted on the *adopted* set; the whole-dialog I1/I2 grid is M001's harness and was not re-run here. |
| An oversized claim is clamped, not cropped | `an_oversized_preference_is_clamped_rather_than_cropped` asserts the host places `clamp_area(preferred, screen)` | pass | |
| Confirm at 24 rows shows both options without scrolling | `both_options_and_the_keymap_fit_the_stated_preference_at_24_rows` asserts `Yes`, `No`, `Enter confirm` are in the drawn buffer | pass | The criterion the milestone exists for. |
| The corpus is byte-identical after the seam arrives (control case) | **reported** by the implementing session; no pre-change corpus exists in this tree to compare against | **not measured** | §1 point 2, §10. |
| `tui_info_dialog_panel_chrome` passes unmodified | the `.inc` is modified in this working tree (`git status`) | **not verified** | §1 point 3. |
| `ProjectPicker`'s existing centring and clamping tests pass unmodified | `picker_area_is_centered` and `picker_area_clamps_to_available` still present (`project_picker.rs:484`, `:492`) | pass | `picker_area` was retained as the implementation body rather than removed. |
| `src/tui/components/dialog_size.rs` states the floor/ceiling once | `clamp_axis` (`:53`) is the only axis rule; `preferred_rows` and `legacy_height` are helpers over it | pass | Two helpers beyond the plan's list; neither re-implements the clamp. |
| `scripts/check_tui_render_cost.py` passes **unmodified** | ran: "the render path derives each result once", exit 0; `git status` shows the guard unmodified | pass | No second measure of a dialog's content per frame. |
| `scripts/verify.sh quick` green | not run at this pass; `scripts/verify.sh full` 2026-11-13 passed, and `run_full()` calls `run_quick()` | **pass, 2026-11-13** | §10. |
| `architecture/tui.md` documents the sizing contract | "Dialog sizing" section at `architecture/tui.md:5600-5620`: `Component::preferred_area` is the dialog's statement, the host runs it through `dialog_size::clamp_area` once, and `clamp_area` is the only code that applies the bounds | pass | Verified in the tree for this correction. |

## 3. Production implementation evidence

- `src/tui/components/component.rs:255-265` — `preferred_area`, with the default
  as `centered_rect(60, 50, available)`. The doc comment carries the reason the
  default matters: an unconverted dialog is *correct*, not merely tolerated.
- `src/tui/components/dialog_size.rs` — `MIN_DIALOG_WIDTH`/`MIN_DIALOG_HEIGHT`
  (20×6), `MAX_DIALOG_PERCENT` (90), `clamp_area` (`:25`), `clamp_axis` (`:53`),
  `preferred_rows` (`:38`) and `legacy_height` (`:49`). The module doc states
  that `clamp_area` is "the only code that decides the floor and the ceiling".
- `src/tui/app/render.rs:3136-3143` — `dialog_rect`, the single sizing site, and
  `:364-385` where its result is stored on `dialog_area` and cleared.
- `src/tui/components/dialogs/confirm.rs:53-60` — Confirm's preference is its
  three sections' owned heights, with `TITLE_ROWS`/`MESSAGE_ROWS`/`OPTIONS_ROWS`
  counting content rows only because `Section` adds the border.
- `src/tui/components/dialogs/info.rs:356-368` — the Info family's preference
  floors at `legacy_height(available)`, with the reason in its doc comment: the
  content changes while the box is open, and a box that resized on every refresh
  would move under the reader's cursor. That is the plan's §16 instruction
  ("content-sized does not mean as small as possible") implemented rather than
  quoted.
- `src/tui/components/dialogs/project_picker.rs:52-64` — `picker_area` retained
  and `preferred_area` delegating to it, so one sizing path exists.

Nothing in the plan's "explicitly out of scope" list was implemented: no frame
region is resized, there is no user-resizable or remembered dialog size, and no
dialog's internal layout was changed to suit a new rect.

**Not adopted**, and why — the list §12 requires:

| Dialog | Reason |
|---|---|
| `PermissionDialog` | **In scope and absent.** No `preferred_area`; keeps the legacy default. |
| Every other dialog module | Not adopted, and the plan permits it: "Any dialog whose content size is not statically derivable — those keep the default and are listed in the closure record as not adopted." Measured: `src/tui/components/dialogs/` holds **42** `.rs` files (including `mod.rs`) and **6** override `preferred_area` — the six named above — so the other 36 keep the default. The per-module reason is the plan's own content-derivability clause; a reviewed justification for each of the 36 was **not** produced, because a guess at a module-to-fixture mapping is worse than a structural statement. |

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`.

```bash
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_invariant_baseline.py

git status --porcelain
grep -rn 'fn preferred_area|fn clamp_area|preferred_area\(' src/
grep -rn 'clamp_area' src/tui
grep -rn '_area: Option<Rect>' src/tui
grep -n 'fn ' src/tui/components/dialog_size.rs
grep -n 'fn keymap_hint' src/tui -r
```

### Results

| Command | Result |
|---|---|
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once", exit 0 |
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and 0 hint-truncation entries recorded; every one reproduces and none was added unbanked", exit 0 |
| `preferred_area` grep | 6 dialog implementations + the trait default + 2 test doubles (measured) |
| `clamp_area` grep | 1 production caller; 8 test occurrences (measured) |
| `*_area: Option<Rect>` grep | 2 App fields (`dialog_area`, `sidebar_area`) — M008's concern, not this milestone's |
| `keymap_hint` grep | 19 implementations across 15 dialog files (relevant to M005, recorded here only because it was taken in the same pass) |

### Not run

| Command | Why |
|---|---|
| `cargo test --test tui_render` | The corpus is expensive and three other sessions were compiling concurrently, by instruction. The corpus result for this milestone is **reported**, not re-measured here. |
| `cargo test --lib -- components::dialogs::project_picker` | Same reason; the two picker tests are present and unmodified by inspection. |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Out of scope for this review pass, and a formatting or lint failure in a file this milestone did not write would not be evidence about it. |
| `scripts/verify.sh quick` | Forbidden by this session's instructions; also red for an unrelated reason per the briefing. |

**Everything above is a property of the tree at the moment it was read**, not a
standing guarantee: the working tree carried 231 changed or untracked paths
while this record was being written, and later milestones move these files.

## 5. Invariant review

- **No dialog becomes unusable at a small size.** I2's panic-freedom grid is
  M001's; this milestone adds the clamp, and the clamp's six-size test includes
  1×1 and 10×3 where the whole frame is returned. Holds.
- **No dialog paints outside its area (I1) at the three named sizes.** Holds for
  the six adopters, asserted with a sentinel-filled buffer. For non-adopters I1
  is unchanged because their rect is unchanged.
- **The default is today's behaviour.** Holds and is asserted: the default is
  literally `centered_rect(60, 50, available)`, and a test compares the two.
- **`centered_rect` remains the single definition of the percentage shape.**
  Holds — it is the default implementation's body.
- **No dialog gains or loses a key, a hint or a row it used to show.** Holds for
  the adopters: Confirm's row budget is unchanged (`Length(4)/Min(4)/Length(5)`
  equivalents now derive from `Section`), and the 24-row test would fail if a
  row disappeared. The Info family's preference is *floored* at the legacy
  height, so it can only grow — which is what keeps a viewer's rows from
  vanishing.
- **The render-cost shape guard is unchanged.** Holds — the guard passes and is
  unmodified.
- **The comment ratchet holds.** **Verified 2026-11-13**: `check_comment_budget.py`
  passes as part of `scripts/verify.sh full`, with the corpus banked at 139,245
  comment lines in `docs/comment-budget.toml`. The overage this record reported
  belonged to the tree state it read, not to the milestone.

## 6. Failure and recovery review

- **No runtime path.** Sizing is a pure function of the frame rect and the
  dialog's own content count; there is no daemon, persistence or async work.
- **The one hazard is oscillation**, and the plan names it: a preference derived
  from the rect it is given would feed back. `preferred_area` takes `available`
  and nothing else, and
  `a_stated_preference_is_idempotent_for_the_same_frame` asserts two calls with
  the same input are equal. The `available` parameter is the *frame*, not the
  previously assigned rect, so the feedback edge does not exist in the
  signature.
- **A claim larger than the frame** is clamped, and a claim in a 1×1 frame is
  clamped to that frame rather than producing an inverted rect.
- **Failure visibility.** A dialog whose preference is wrong is visible as a
  test failure in `every_adopting_dialog_renders_inside_its_clamped_preference`
  with the surface name and size in the message, which is what makes the matrix
  a detector rather than a snapshot.

## 7. Migration and compatibility review

No schema, protocol, configuration or dependency surface. The change is internal
to one crate, and a dialog that does not adopt the seam renders exactly as
before — which is why the adoption list can be partial without a compatibility
question. Rollback is reverting the seam and the six implementations; there is
no durable state and no user setting.

One compatibility note worth recording: the *visible* rect of six dialogs
changes, and that is the milestone's purpose. The change is not covered by any
configurable or documented size, so there is nothing for a user to migrate.

## 8. Security review

Not applicable in the authorization sense: no path, process, secret, network or
permission surface is touched. Two bounded-input properties do hold and are
worth naming because a sizing seam is where they could be lost:

- The clamp is total over the `u16` domain — `clamp_axis` saturates rather than
  subtracting, so a frame smaller than the floor cannot underflow.
- A dialog cannot claim an unbounded rect: the 90 % ceiling and the frame bound
  are applied by the host, in one place, and the "oversized claim" test asserts
  the clamp rather than the claim.

## 9. Documentation and operations

- `architecture/tui.md` — the plan's §12 asks for the sizing contract (a dialog
  states a preference, the host clamps it, the default is the legacy
  percentage). **Present** at `:5600-5620`; see §10.
- The corpus diff note, one entry per dialog with the resulting rect at each of
  the three sizes (plan §12), **was not produced**. The adoption matrix in
  `dialog_size.rs` is the mechanical substitute: it records the surfaces and the
  three sizes and asserts containment, but it does not record a per-dialog rect
  table. Recorded as an outstanding documentation item rather than claimed.
- No new guard, as the plan requires; I1 and I2 at three sizes are the
  enforcement.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `PermissionDialog` does not implement `preferred_area`, although work package D names it as an adopter. | The permission prompt keeps a 60 % × 50 % rect on every terminal. It is correct (the default is the legacy behaviour) but it is the one dialog the plan named that a user meets most often, so the milestone's visible benefit is missing exactly there. | Implement `preferred_area` from the dialog's section heights, add it to `every_adopting_dialog_renders_inside_its_clamped_preference`, and record it in this matrix. Bounded, mechanical. |
| low | The "corpus is byte-identical after the seam" control case is not reproducible: no pre-change corpus exists in this tree (no commit, and the `.inc` files are already modified by later milestones). | A reviewer cannot mechanically confirm that the seam changed no pixel; they can confirm only that the clamp is a no-op for a default-preference dialog, which the default test does assert. | Land milestones as separate commits, or record the corpus digest in the plan at the seam's introduction. Not fixable retrospectively. |
| low | `tests/tui_render/tui_info_dialog_panel_chrome.inc` is modified in this working tree, so acceptance criterion 6's "unmodified" cannot be evaluated. | One acceptance criterion is unverifiable from the tree; the test itself is not known to be red. | Attribute the modification to its milestone (M002/M003 both touch dialog chrome) and record the attribution, or evaluate criterion 6 per-milestone from separate commits. |
| low | `scripts/verify.sh quick` and `cargo fmt --check` were not run at this pass; the tree is uncommitted. | Acceptance criterion 7 had no evidence at that pass. | **Discharged 2026-11-13**: both ran inside `scripts/verify.sh full` on this tree and passed. The commit is still outstanding. |
| low | No per-dialog rect table at the three sizes was produced (plan §12 / §15). | The size matrix asserts containment rather than recording the resulting rects, so a reviewer cannot see where a dialog landed. | Optionally bank the matrix's rects in the record or a fixture note; the containment assertion is the contractual part and it exists. |
| low | `architecture/tui.md`'s sizing-contract section was not verified in this pass. | A plan-mandated documentation item may have been absent. | **Verified present** in the tree at `architecture/tui.md:5600-5620` ("Dialog sizing"): `preferred_area`, the single host-side `clamp_area`, and the legacy default. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding**,
plus one bounded in-scope item (Permission's adoption) that a small corrective
must close.

The conditional element is precise: the green `scripts/verify.sh quick` /
`cargo fmt --check` half is **met** — `scripts/verify.sh full` passed on this
tree at 2026-11-13 — and what remains is the corpus byte-identity result, which
cannot be re-measured because no committed before-state exists. The corrective
element is the Permission adoption: `PermissionDialog` still has no
`preferred_area` (measured on this tree, six adopters under
`src/tui/components/dialogs/`), so this record keeps that as an open item rather
than calling adoption complete. Neither is a redesign, and neither blocks M005
or M008 from being read on their own terms — M005 consumes `keymap_hint`, which
this milestone does not touch, and M008 owns `App::render`, which this
milestone's single `dialog_rect` call site does not disturb.

## 12. Registry updates

- `plans/registry.md` line 29 (the subsystem row): "M001–M004 implemented,
  M005–M010 in progress or pending" and "M004 in flight" must become a status
  that names M004's record and its condition.
- `plans/registry.md` line 682 (the M004 milestone row): status
  "**Blocked — hard dependency on M001 and M003**" is false; it must become
  conditional with this record's path and the Permission finding.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12, row
  `004`: `blocked` → `conditionally closed`, closure record
  `plans/closure/tui-modularity-and-render-invariants/004-status.md`, blockers
  "green `quick` on the merged tree; the corpus control case re-measured against
  a committed before-state; `PermissionDialog` adoption".
- `plans/implementation/tui-modularity-and-render-invariants/004-content-sized-modals.md`
  header: `Status: blocked` → `Status: conditionally closed`. **This record's
  author made this header edit** because the sibling plans 001–003 in this line
  already carry their records' status and a `blocked` header beside an existing
  closure record is a factual error.

## Correction log (2026-11-13)

- **§1 point 4, and §2's `quick` row.** Was "`scripts/verify.sh quick` was not run" with criterion 7 having "no evidence at all". Met by: `scripts/verify.sh full` on this tree, 2026-11-13 — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures, `cargo fmt --all --check` clean, clippy `-D warnings` clean — and `run_full()` calls `run_quick()`. Only the commit remains outstanding.
- **§5, the comment-ratchet bullet.** Was "Not verified here … the corpus-wide ratios were already over their ceiling". Met by the 139,245-line bank and the green guard inside the same `full`.
- **§9 and §2, `architecture/tui.md`'s sizing contract.** Was "Not verified in this pass" / "not measured". Verified present at `architecture/tui.md:5600-5620`: `preferred_area` as the dialog's statement, one host-side `clamp_area`, the legacy default.
- **§10, the `quick`/`fmt` row.** Met by the 2026-11-13 `full`; the commit is still named as outstanding.
- **§10, the sizing-contract row.** Met by the verified `architecture/tui.md:5600-5620` section.
- **§11, the disposition paragraph.** The gate half of the conditional element is recorded as met; the remaining conditional element is the corpus byte-identity result with no before-state, and the corrective is the still-absent `PermissionDialog` `preferred_area` (re-measured: six adopters, Permission not among them).

**No promotion.** The status stays `conditionally closed`: `PermissionDialog` still does not adopt `preferred_area`, the corpus byte-identity control case has no committed before-state, and the per-dialog rect table was not produced. No condition was deleted without naming what met it.
