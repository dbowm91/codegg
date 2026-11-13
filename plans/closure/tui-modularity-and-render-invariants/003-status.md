# TUI Modularity and Render Invariants Milestone 003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/003-one-dialog-frame.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-003--one-dialog-frame`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`.
  `src/tui/components/section.rs` is still **untracked** (`git status` reports
  it as a new path), `tests/tui_render/invariants/docked.rs` is part of the
  untracked `tests/tui_render/invariants/` directory, and the 39 adopting
  dialog files carry the change uncommitted. Nothing below is attributable to a
  SHA.

## 1. Executive finding

The primitive is delivered, adopted across every dialog surface that can use
it, and it changed the contrast bank in the direction the milestone predicted.
`src/tui/components/section.rs` owns the outer edge, the title string and the
bottom hint; it resolves both styles through the **same** function the panel
primitive uses and spells no second colour pair:

```rust
pub fn styles(&self) -> ChromeStyles {
    ChromeStyles::resolve(self.theme, self.focused)
}
```

(`src/tui/components/section.rs:156-158`). That is asserted as an equality
against `ChromeStyles::resolve` in both focus states, so a local literal added
later cannot pass the test
(`the_border_and_title_use_the_shared_focus_policy`).

**The owner's marker decision is implemented exactly as decided, and it is the
reason the contrast bank fell.** A modal passes `focused: true` and therefore
paints a primary edge and a primary title, but it never paints a `●`, because
`Section::title()` builds its title through `chrome_title(&label, false)` —
literally passing `false` for the marker, in one call, rather than trusting
every caller to pass `focused: false` and accept a muted title with it
(`section.rs:166-184`). The `●` stays the panel host's: it names the column
that holds the keyboard, and a modal is a surface, not a column. The consequence
is the **−528** contrast reduction recorded in the manifest: 24 dialog surfaces
× 22 themes whose `border`-on-`background` edge pairs stopped existing when the
edge became `primary`; the primary title clears the bar on every bundled theme
(`docs/tui-render-invariant-baseline.toml:77-88`, `[totals] contrast = 2495`).

**Adoption is 39 of 40 dialog surfaces.** Counting is the one place a reader can
go wrong here, so it is stated precisely: `src/tui/components/dialogs/` holds 42
files; `mod.rs` only declares modules and `panel_list.rs` is filterable-list
state with **no `Component` impl and no frame** (`grep "impl Component for"
panel_list.rs` returns nothing), so neither is a surface. The remaining **40**
are the surfaces, and 39 of them build their outer frame and title through
`Section`. The single exception is `permission.rs`, and the gap is on its title
row rather than its frame: that row carries a second, right-aligned, risk-coloured
badge (`ToolRisk` → `theme.warning` / `error` / `secondary`), which `Section`
cannot express because it owns exactly one title styled from the focus policy.
The exception and its reasoning are recorded in `architecture/tui.md:4146-4153`,
which is the plan's §14 stop condition answered as §14 asks — reported, not
special-cased — and in the code itself
(`src/tui/components/dialogs/permission.rs:125-153`). **An earlier working
figure of "39 of 41" was wrong**: it counted `panel_list.rs` as a surface, which
it is not. 39 of 40 is the number the architecture document states and the
number this record verified.

The harness-wide docked assertion exists and is not vacuous.
`tests/tui_render/invariants/docked.rs` renders **every** fixture whose
`Component::docks_as_viewer` is true through the production hosted seam and
requires the host's top border, side borders, title and `●` to survive
(`every_dockable_dialog_leaves_the_hosts_border_title_and_focus_dot_alone`,
with a `docked >= 15` floor). Its non-vacuity twin is a wrapper around the real
`AgentDialog` that answers `docks_as_viewer` but does **not** override the seam
— the pre-fix shape as a value — and the assertion rejects it with the
overwritten title named
(`the_docked_chrome_assertion_detects_a_box_that_paints_its_own_border`). A
second sweep over **48 fixtures** renders every modal and asserts the outer ring
carries no `●`, with a `swept >= 40` floor so a shrunken fixture table cannot
pass vacuously (`no_modal_paints_a_focus_marker_on_its_own_frame`, and its
`the_no_marker_sweep_detects_a_marked_frame` twin).

**One regression this milestone introduced, and fixed here.** `panel_manager.rs`
was converted to draw its list through `Section`, and the conversion passed
`Section`'s **inner** rect height to `PanelListState::scroll`, which already
subtracts its own two border rows. At 72×24 the list chunk is 3 rows, the inner
rect is 1, so `page = 1 - 2 = 0` and `scroll` returned `0` unconditionally —
**panel-list scrolling was silently disabled**, with nothing failing. It
surfaced as "Files missing" in
`tui_panel_placement::the_highlight_stays_visible_when_walking_the_list`
(`tests/tui_render/tui_panel_placement.inc:581`), which walks the cursor past
one page and asserts the highlighted panel is still on screen. The call site
now passes the box height, not the inner height —
`let list_box_height = chunks[2].height;`
(`src/tui/components/dialogs/panel_manager.rs:340-343`) — and because the helper
had no test of its own for `scroll`, one was added:
`the_last_row_is_reachable_through_a_one_row_viewport`
(`src/tui/components/dialogs/panel_list.rs:293-306`), which asserts that a
three-row box shows one content row and that the offset must be the last row.
That is the test that would have caught the conversion before it shipped.

Status is conditional for two reasons, neither of them the primitive: the plan's
per-dialog corpus diff note (acceptance criterion 5, §15) was never produced as
an artifact, and its pre-change frame no longer exists, so the mechanical
substitute needs the owner's acceptance in writing (§10, open question); and
acceptance criterion 1 — "every dialog's outer frame… comes from `Section`" — is
met for 39 of 40 with one reported §14 exception, so the criterion is not
literally satisfied and cannot be without a primitive change this milestone was
forbidden to make. That exception is a second open owner question: amend the
criterion, or scope a `Section` title adjunct (§10). The gate condition this
record previously carried — `scripts/verify.sh quick` and `cargo fmt --check`
not run, the comment ratchet red at 137,275 against 134,015 — is discharged:
`scripts/verify.sh full` passed on this tree at 2026-11-13 (17,377 workspace
tests, 8,441 feature-pass tests, 0 failures; fmt clean, clippy `-D warnings`
clean), and the corpus is banked at 139,245 lines.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A `Section` primitive: bordered block, optional title, optional hint, styled through the shared policy, returning its inner rect | `src/tui/components/section.rs` — `Section::{new, plain, verbatim, hint, focused, frame, owned_height, styles, title, inner, draw}` | pass | The hint sits on the bottom edge, padded and never marked. |
| It owns exactly the chrome decisions and no content layout | `draw` renders a `Block`, answers `inner`, and returns; `owned_height` is the only height arithmetic | pass | `section.rs:36-44` states the non-goal explicitly. |
| Border and title come from `ChromeStyles::resolve`, with no second colour literal | `styles()` is literally `ChromeStyles::resolve(self.theme, self.focused)`; `the_border_and_title_use_the_shared_focus_policy` asserts equality in both states and that the edge differs | pass | The one outcome that would defeat the milestone. |
| The title string is built by the primitive: padding, Title Case, caller marker stripped, never re-marked | `normalise_title`, `Section::title` (`chrome_title(&label, false)`), `a_title_is_padded_and_cased_here_and_never_marked`, `a_caller_supplied_padding_or_marker_is_stripped` | pass | The ten unpadded `run_detail` titles and `"Job detail"` cannot return through a caller. |
| A title that is **data** (breadcrumb, node name) keeps its casing and still gets the padding | `Section::verbatim`, `a_verbatim_title_keeps_its_data_and_still_owns_the_padding` | pass | An addition the plan did not name; justified in the module header and `architecture/tui.md:4115-4119`. |
| A docked dialog draws no outer border, structurally | `SectionFrame::Hosted` writes no cell and answers `area.inner(Margin::new(1,1))`; below the chrome threshold it answers `area` unchanged; `the_hosted_frame_paints_nothing_and_answers_the_hosts_inner_rect` | pass | "No cell at all" includes no background fill, because the host's border cells are inside `area`. |
| `Component::render_in_panel` is the seam that selects the hosted arm | `src/tui/components/dialogs/agent.rs:311-318`; the same shape on every dockable dialog | pass | The trait default stays "delegate to `render`". |
| `AgentDialog` keeps its modal border and draws no outer one when docked (the live double-chrome defect) | `render_in_panel` overrides to `render_body(..., SectionFrame::Hosted)`; unit tests at `agent.rs:429-490`; harness twin `the_docked_chrome_assertion_detects_a_box_that_paints_its_own_border` | pass | The plan's §15 "AgentDialog before/after with the failing assertion". |
| The `InfoDialog` control case passes and its corpus is byte-identical | `cargo test --test tui_render -- tui_info_dialog_panel_chrome` — 13 passed; `modal_rendering_is_unchanged` pins every row of the 44×16 modal layout | **partial** | The file **is** modified in the tree, by M002's `render_hosted(label, focused)` signature change and by the chrome-threshold test rename — not by M003. `modal_rendering_is_unchanged`'s body is untouched and green, but "byte-identical corpus" is asserted row-by-row rather than as a digest, and no M003-only diff exists to inspect. |
| Title normalisation: the 10 `run_detail` titles get padding, `"Job detail"` becomes Title Case, `files.rs`'s dot moves to trailing | `run_detail.rs` builds all nine sections through `Section::new("…")`; `files.rs` uses `Section::verbatim` for the breadcrumb and `Section::new` for labels; no production dialog carries a `●` literal (the `" ● Files "`, `" Diff ● "`, `" Plan ● "`, `" Tree ● "` strings that remain are `#[cfg(test)]` non-vacuity fixtures) | pass | Verified by grep across `dialogs/`: every remaining marked title is inside a `#[cfg(test)]` module. |
| The sweep: every dialog's outer frame and title from `Section` | 39 of 40 surfaces import and use `Section`/`SectionFrame`; `permission.rs` is the recorded exception | **partial** | The exception is the plan's §14 stop condition, reported rather than improvised. See §10. |
| I1 assertion for dockable dialogs, and it detects the defect it repairs | `every_dockable_dialog_leaves_the_hosts_border_title_and_focus_dot_alone` (15+ dockable fixtures); `the_docked_chrome_assertion_detects_a_box_that_paints_its_own_border` rejects the pre-fix shape and names "Agents"/"overwrote" | pass | The failure is produced by production's own render path through a wrapper, not by a rebuilt fake. |
| A modal never paints its own `●` | `no_modal_paints_a_focus_marker_on_its_own_frame` over **48** fixtures with a `swept >= 40` floor; `the_no_marker_sweep_detects_a_marked_frame` proves the ring reader bites | pass | The owner's marker decision as frame-level evidence. |
| No dialog gains or loses a key | `tests/tui_render/dialog_dispatch_coverage.inc` is **unmodified** (`git status` returns nothing) and its 4 tests pass; its three recorded gaps are unchanged | pass | The plan's "neither closes nor widens them". |
| I1, I2 and I3b stay green for every dialog | `cargo test --test tui_render -- invariants` — 28 passed, including `every_dialog_renders_without_panicking_across_the_size_grid`, `every_dialog_renders_at_the_smallest_size_and_only_recorded_ones_decline`, `every_dockable_dialog_changes_its_column_when_the_column_is_focused` | pass | |
| A dialog with a 60-character title renders at 20 columns without panic or overflow | No such test exists | **not run** | §10 finding: the specific §10 negative test has no counterpart. I2 covers 20×6 for every dialog but with each dialog's own title. |
| `tui_info_dialog_panel_chrome` passes, `dialog_dispatch_coverage` unchanged, and a representative spread of converted dialogs passes | `tui_info_dialog_panel_chrome` 13, `dialog_dispatch_coverage` 4, `tui_main_buffer` 82, `file_picker_surface_corrective` 14, `tui_logs_window` 9 — all 0 failed | pass | Superseded by the whole family (1221) but run individually because the plan names them. |
| The render-cost shape guard passes unmodified | `python3 scripts/check_tui_render_cost.py` — "the render path derives each result once" | pass | No edit. |
| The contrast growth/reduction is banked and attributed | manifest header "The banked reduction: the dialog frame (M003)": 528 rows removed, none added, 24 surfaces × 22 themes; `[totals] contrast = 2495`; the harness equality test passes | pass | 3023 − 528 = 2495, and 24 × 22 = 528. |
| `scripts/verify.sh quick` is green | Not run | **not run** | Explicitly out of scope for this review, and red tree-wide for sibling work (§4). |

## 3. Production implementation evidence

- `src/tui/components/section.rs` — the primitive. Untracked in git; 540 lines
  including tests. Its module header is the contract: the style policy is
  `ChromeStyles::resolve`, the marker is the host's, the title string is built
  here, the hosted arm writes no cell, and content layout is a non-goal.
- `SectionFrame::{Owned, Hosted}`, `Section::verbatim` and
  `Section::owned_height` are the three shapes the plan implied but did not
  spell. `owned_height` exists because a box laid out as a chunk and a box
  laid out as a sum must not disagree about the border — which is exactly the
  arithmetic the panel-manager regression got wrong.
- 39 adopting dialog files under `src/tui/components/dialogs/`. The adoption
  shape is uniform: `render` builds an `Owned` section, `render_in_panel`
  builds a `Hosted` one over the same rect, and the body paints into the rect
  `draw`/`inner` answers.
- `src/tui/components/dialogs/agent.rs` — the live double-chrome instance
  repaired: `docks_as_viewer` stays `true`, `render` keeps its modal border,
  and `render_in_panel` selects `SectionFrame::Hosted`.
- `src/tui/components/dialogs/panel_manager.rs:340-343` — the scroll call site
  passes the box height. This is a **fix to a regression introduced by this
  milestone's own conversion**, not a pre-existing defect, and it is recorded
  here rather than folded into the sweep.
- `src/tui/components/dialogs/panel_list.rs:293-306` — the new `scroll` unit
  test. The helper already existed; the coverage did not.
- `tests/tui_render/invariants/docked.rs` — 279 lines: the hosted-chrome
  checker, the pre-fix wrapper, the 48-fixture marker sweep, and both
  non-vacuity twins.
- `docs/tui-render-invariant-baseline.toml` — the M003 banked reduction in the
  file's own header, with the reason (primary edge replaces border edge), the
  count (528) and the surface/theme fan-out (24 × 22).

No key handling, focus ownership, placement or sizing behaviour changed.
`docs/tui.md` and `architecture/tui.md` were updated in the same tree.

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`,
on 2026-11-11 between 19:37 and 19:58.

```bash
cargo test --test tui_render -- invariants
cargo test --test tui_render
cargo test --test tui_render -- tui_info_dialog_panel_chrome
cargo test --test tui_render -- dialog_dispatch_coverage
cargo test --test tui_render -- tui_main_buffer
cargo test --test tui_render -- file_picker_surface_corrective
cargo test --test tui_render -- tui_logs_window

python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test
python3 scripts/check_tui_render_cost.py
python3 scripts/check_comment_budget.py
```

### Results

| Command | Result |
|---|---|
| `cargo test --test tui_render -- invariants` | **pass** — 28 passed, 0 failed, 0 ignored, 1193 filtered out, finished in 10.82s. Includes `invariants::docked` (both tests), the 48-fixture marker sweep, and the three recorded-baseline equalities |
| `cargo test --test tui_render` (whole family) | **pass** — 1221 passed, 0 failed, 0 ignored, 0 filtered out, finished in 16.65s. The filtered count from the `invariants` run (1193) plus its 28 gives the same 1221, which is the family size this review was given as the baseline |
| `… -- tui_info_dialog_panel_chrome` | **pass** — 13 passed, 0 failed, 1208 filtered out |
| `… -- dialog_dispatch_coverage` | **pass** — 4 passed, 0 failed, 1217 filtered out |
| `… -- tui_main_buffer` | **pass** — 82 passed, 0 failed, 1139 filtered out |
| `… -- file_picker_surface_corrective` | **pass** — 14 passed, 0 failed, 1207 filtered out |
| `… -- tui_logs_window` | **pass** — 9 passed, 0 failed, 1212 filtered out |
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and 1 hint-truncation entries recorded; every one reproduces and none was added unbanked" |
| `check_tui_render_invariant_baseline.py --self-test` | **pass** — "12 cases passed" |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once" |
| `check_comment_budget.py` | **fail** — 48 violations tree-wide. **None of this milestone's artifacts is among them**: `src/tui/components/section.rs` and every `src/tui/components/dialogs/*` file are absent from the per-file list. Three violations are **corpus-wide**: 137,275 comment lines against the ceiling of 134,015, 1,220,763 words against 1,188,348, ratio 0.1354 against 0.1344. |

**One transient build failure, recorded because it describes the tree and not
this milestone.** The first `cargo test --test tui_render -- invariants` at
19:37 could not compile the library: `E0599` at
`src/tui/editor_highlight.rs:572` — `SYNTAX_SET.metadata()` with syntect's
`metadata` feature not enabled. `Cargo.toml` was changed by another session at
**19:43:42** to `syntect = { version = "5", features = ["metadata"] }`; the
retry compiled and every result above comes from that retry. Nothing in this
milestone touches `editor_highlight.rs` or the syntect dependency. Several
`cargo` invocations also queued on the shared build-directory lock held by
sibling sessions in the same worktree; the reported finishes are real runs
after the lock cleared, not cached summaries.

The tree moved during the review: `src/tui/editor.rs` was modified at 19:46:04
by a sibling session, before the test binaries this record cites were built
(`tui_render-c222fb5313649f59` at 19:52, `codegg-7f071e724f49550c` at 19:56).
Every source file on this milestone's path predates those binaries
(`section.rs` 18:51:47, `agent.rs` and the other dialogs earlier), so the
results describe this milestone's artifacts at those times. A later run may
report something different for a reason that is not this milestone.

### Not run

| Command | Why |
|---|---|
| `scripts/verify.sh quick` | Not run, by instruction and because it is red tree-wide: `check_comment_budget.py` alone reports 48 violations in files this milestone did not touch, so a `quick` result would not be attributable to M003. |
| `cargo fmt --check` | Not run. Sibling sessions are editing `src/tui/**`; a formatting failure in a file this milestone did not write would not be evidence about it. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Not run. `verify.sh full` is out of scope for this review. |
| Plan §11's `cargo test --test tui_render -- tui_render_invariants` | Selects **nothing**: the member is `invariants`, not `tui_render_invariants`. The plan's §11 line is a documentation defect in the plan, not a missing deliverable; `-- invariants` is what was run. |
| The per-dialog corpus diff note (§12, §15, acceptance criterion 5) | No such artifact exists in the repository. §10 finding. |

## 5. Invariant review

- **No dialog lost a verb, a hint or a row of content.** The mechanical
  guarantees are: the whole family is green (1221/1221), `I1` containment and
  `I2` size-grid coverage assert every dialog paints inside its rect and does
  not panic, `I3b` requires the focus difference to be on an **edge** cell so a
  body-only change cannot satisfy it, and `dialog_dispatch_coverage.inc` is
  unmodified and green. What is missing is the plan's per-dialog corpus diff —
  without it, a body change that happened *alongside* an edge change would not
  be caught. See §10.
- **No dialog gains or loses a key.** `dialog_dispatch_coverage.inc` is
  byte-identical to HEAD and its three recorded gaps are unchanged.
- **`centered_rect` remains the single definition of the modal shape.**
  `src/tui/app/render.rs` was not edited for sizing; M004 owns the arguments.
- **I1, I2 and I3b stay green.** Asserted in the same run (28/28).
- **The render-cost shape guard is unchanged and green.**
- **The comment ratchet does not hold tree-wide**, at 137,275 lines against
  134,015. No artifact of this milestone appears in the per-file list, so the
  milestone did not break a file budget; `section.rs` and the dialog files are
  under the unlisted floor. The corpus overage is shared across many sessions.
- **Consistency of the two chrome producers.** `Section::styles()` is the same
  call `PanelChrome::styles()` makes, asserted by equality rather than by
  reading; the milestone cannot drift from M002 without failing
  `the_border_and_title_use_the_shared_focus_policy`.
- **Every dockable dialog is covered, checked as a set equality.** Exactly 15
  dialog files answer `docks_as_viewer` with `true`, and exactly the same 15
  files implement `render_in_panel` — and every one of those overrides selects
  `SectionFrame::Hosted`. So the docked-rule fan-out and the seam fan-out are
  the same set, which is why the harness's `docked >= 15` floor is the live set
  rather than a sample.

## 6. Failure and recovery review

- **No runtime path.** Rendering is synchronous and stateless with respect to
  this change; no daemon, persistence, async work or cancellation is involved.
- **The realistic failure mode is partial adoption** — a dialog half-converted
  between two chrome producers. The plan orders the work packages to prevent it
  (primitive → control case → defect fix → sweep), and the residue is visible
  as a file that draws a frame without `Section`. `grep` over `dialogs/` finds
  exactly two such files and one of them (`permission.rs`) is the documented
  exception; the other (`panel_list.rs`) is not a surface.
- **The regression this milestone did introduce was a silent one**, and it is
  the clearest failure-and-recovery evidence in this record: a correct-looking
  conversion of a call site to `Section` double-subtracted the border, page
  size went to zero, and scrolling stopped with nothing failing. It was caught
  by a render assertion about a *different* property (`the highlight stays
  visible`), not by any test of the helper. The recovery added the missing unit
  test. That is why `Section::owned_height` exists and why
  `the_height_helper_counts_the_border_exactly_once` is written as an
  equalities-over-a-range test rather than a single example.
- **Failure visibility.** The docked checker returns the first violation with
  the surface name and the actual top row, so a regression localises to a
  dialog and an overwritten title rather than to a count.

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency surface. The visible change
is dialog chrome: a modal now reads as the focused surface (primary edge and
title), titles are uniformly padded and Title Cased, `files.rs`'s dot is gone
from its own frame, and a docked dialog leaves the host's chrome intact. No
dialog identity, key, focus, placement or sizing behaviour moved.

Rollback is deleting `section.rs`, restoring the 39 inline frame blocks, and
reverting the manifest's 528-row reduction. The digests and corpus assertions
that M002 left in place for Chat/Terminal are unaffected (they are panel
surfaces).

## 8. Security review

Not applicable in the authorization sense: no path, process, secret, network,
permission or authority surface is touched. Two bounded-input properties do
hold and are worth naming: a section's title is trimmed and stripped of a
caller-supplied marker, so a title cannot smuggle a focus claim onto a frame it
does not own; and `Section::draw` paints only `area`, which the I1 containment
assertion and its non-vacuity twin check cell by cell.

## 9. Documentation and operations

- `architecture/tui.md` — "One dialog frame" (`:4084-4158`): the shared style
  policy, the title construction, the marker rule, the `verbatim` arm, the
  docked rule and its seam, the 39-of-40 count with `panel_list.rs` excluded,
  and the `permission.rs` badge exception with its reasoning.
- `src/tui/components/section.rs` module header — the contract a new dialog
  must follow, including why the marker is not `Section`'s and why the hosted
  arm writes no cell.
- `docs/tui-render-invariant-baseline.toml` — the banked reduction with its
  reason and fan-out.
- `docs/tui.md` — the editor's `space ?` catalogue and the general help
  routing were checked and did not move in this milestone.
- No new guard, as the plan specifies: the I1/I2/I3b harness and the docked
  sweep are the enforcement.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **The per-dialog corpus diff note (acceptance criterion 5, §12, §15) does not exist.** No artifact in the repository records, for any dialog, which cells moved when it adopted `Section`, and the pre-adoption frames are no longer renderable. | A reviewer cannot mechanically confirm "no dialog lost a verb, a hint or a row of content". The family is green and dispatch coverage is unchanged, but a body change *alongside* an edge change would pass every current assertion. This is the same gap M002's record carries for panels. | **OPEN OWNER QUESTION — not decided by this record.** The plan's §5 puts the per-dialog corpus diff note out of scope because it "need[s] a pre-change frame that no longer exists", and requires the substitute to be accepted in writing. So: is the mechanical substitute acceptable — the whole `tui_render` family run green, `dialog_dispatch_coverage` unmodified and green, and a hosted content-row assertion — in place of the per-dialog corpus diff note? What would settle it: an owner yes/no recorded against this record. |
| medium | **Acceptance criterion 1 is met for 39 of 40 surfaces.** `permission.rs` keeps a hand-built frame because its title row carries a second, right-aligned, risk-coloured badge that `Section` cannot express; folding it into the title loses the alignment and the only `ToolRisk`-coloured paint, and moving it into the body adds a content row. The exception and its reasoning are stated at `architecture/tui.md:4199-4216` and in `src/tui/components/dialogs/permission.rs:125-153`. | The plan's absolute criterion ("every dialog") is not literally satisfied, and cannot be without changing the primitive, which §14 forbids doing for one caller. The exception is reported rather than improvised, which is what §14 asks. | **OPEN OWNER QUESTION — not decided by this record.** The `permission.rs` title-badge exception: 39 of 40 dialog surfaces adopt `Section`, so the owner must choose between (a) amending acceptance criterion 1 to "every dialog surface whose title row needs no adjunct", and (b) scoping a `Section` title adjunct — a second, independently styled title span — for a follow-up milestone and adopting it in `permission.rs`. What would settle it: the owner naming (a) or (b); this record takes neither. |
| low | `scripts/verify.sh quick`, `cargo fmt --check` and clippy were not run. | Acceptance criterion 7 ("`quick` is green") was unevaluated at that pass, and the comment ratchet was red tree-wide (137,275 vs 134,015) for other sessions' work. | **Discharged 2026-11-13**: all three ran inside `scripts/verify.sh full` on this tree and passed — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures, fmt clean, clippy `-D warnings` clean — with the corpus banked at 139,245 lines. |
| low | The plan's §10 negative test — "a dialog with a 60-character title renders without panicking at 20 columns and without painting outside its area" — has no counterpart. | `Section` trims and pads but does not truncate a title; ratatui clips the title to the block, and the I1 containment assertion would catch a paint outside `area`, but no test names the long-title case and no dialog currently carries a 60-character title. | Add the fixture, or record why `Section`'s clipping plus I1 makes it redundant. |
| low | `tui_info_dialog_panel_chrome.inc` is modified in the tree — by M002's `render_hosted(label, focused)` signature change and a chrome-threshold test rename, not by M003 — so the criterion "passes **unmodified**" cannot be evaluated for an M003-only diff, and "byte-identical corpus" is asserted row-by-row rather than as a digest. | The strongest control-case claim in §15 cannot be made mechanically. `modal_rendering_is_unchanged`'s body is untouched and green. | Land milestones as separate commits, or accept the structural reading: the modal layout assertions are unchanged and pass. |

## 11. Roadmap disposition

**Milestone conditionally closed.** The primitive, its adoption across 39 of the
40 dialog surfaces, the `AgentDialog` repair, the docked-chrome harness and the
banked contrast reduction are implemented and verified on this tree. The
conditions are:

- **Condition.** Before M004 (content-sized modals) is cited as depending on a
  closed M003, three things must be true on the merged tree:
  1. `scripts/verify.sh quick` and `cargo fmt --check` are green, and
     `python3 scripts/check_comment_budget.py` is green or the tree's corpus
     ceilings are re-banked with a recorded reason — **met, 2026-11-13**:
     `scripts/verify.sh full` passed (17,377 workspace tests, 8,441 feature-pass
     tests, 0 failures; fmt and clippy clean), and the corpus is banked at
     139,245 lines;
  2. the per-dialog corpus diff note exists, or the milestone records why the
     family run plus `dialog_dispatch_coverage` replaces it — **open owner
     question**: the plan puts the note out of scope for want of a pre-change
     frame, so the substitute needs written acceptance (§10);
  3. acceptance criterion 1 is either amended for the `permission.rs` title-row
     exception or a title adjunct is scoped for a follow-up — **open owner
     question** (§10).
- **Risk.** Low on the primitive. `section.rs` resolves through the shared
  policy by construction, the 48-fixture marker sweep and both non-vacuity
  twins pass, the 39 adopting files all route the docked path through
  `SectionFrame::Hosted`, and the whole render family is 1221/1221. The one
  behaviour risk this milestone actually realised was the panel-manager scroll
  regression; it is fixed, and the helper now has the test it lacked.
- **Exact future evidence.** The `full` transcript of 2026-11-13, the owner's
  answer to the two open questions, and the commit that carries this plan.

M004 may proceed on the primitive. It must not assume the corpus note exists,
and it should treat `Section::owned_height` — not `Section::inner` — as the
height a chunk-based layout passes to a scroll helper.

## 12. Registry updates

- Mark roadmap M003 **conditionally closed** and link this record.
- Record the acceptance-criterion-1 exception: 39 of 40 surfaces, with
  `permission.rs` named and the badge reason referenced.
- Record that the contrast bank is **2495** at this tree state (was 3023), and
  that the 528-row reduction is M003's, not M007's.
- Do not mark M004 ready on the strength of this record alone; the three
  conditions in §11 are its prerequisites.

This record does not edit `plans/registry.md`, the source plan's `Status:`
line, or the roadmap's status table. Those are owned by a separate sweep; this
record's author was scoped to `plans/closure/**` only, and every status claim
above is a recommendation for that sweep rather than a change it made.

## Correction log (2026-11-13)

- **§1, the three-reasons sentence.** Was "`scripts/verify.sh quick` and `cargo fmt --check` were not run". Met by: `scripts/verify.sh full`, 2026-11-13 — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures; `run_full()` → `run_quick()` → `run_guards()`, where `cargo fmt --check` lives. The comment corpus is banked at 139,245 lines. The sentence now names two remaining reasons, both owner questions.
- **§10, the `quick`/`fmt`/clippy row.** Met by the same `full` run; the "137,275 vs 134,015" overage it cited is retired by the 139,245 bank.
- **§10, the per-dialog corpus diff note row.** The required action is now an **open owner question**: the pre-change frame no longer exists, so the plan requires the substitute to be accepted in writing. The question as recorded: is the mechanical substitute — the family run, `dialog_dispatch_coverage`, and a hosted content-row assertion — acceptable in place of the per-dialog corpus diff note?
- **§10, the 39-of-40 row.** Expanded to a second **open owner question** with the exception's documentation cited (`architecture/tui.md:4199-4216`): amend acceptance criterion 1, or scope a `Section` title adjunct for `permission.rs`? The record does not choose.
- **§11, conditions 1–3 and the future-evidence bullet.** Condition 1 is recorded as met by the 2026-11-13 `full`; conditions 2 and 3 are recorded as open owner questions rather than as unmet work; the future evidence is the `full` transcript plus the owner's answers.

**No promotion.** The status stays `conditionally closed`: the corpus-diff-note substitute and the `permission.rs` criterion are undecided owner questions, so the record's conditions are not all met by a green gate alone. No condition was deleted without naming what met it or what must decide it.
