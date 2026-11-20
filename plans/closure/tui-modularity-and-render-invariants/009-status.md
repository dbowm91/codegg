# TUI Modularity and Render Invariants Milestone 009 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/009-list-cursor-and-scroll-follow.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-009--list-cursor-and-scroll-follow`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — **239 changed or untracked paths** at the
re-verification pass's first reading (measured, `git status --porcelain | wc
-l`; 254 by the end of the pass as other lanes kept adding, 238 at the
closeout pass, 235 at the first pass)

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same tree carries M001–M008, M010
  and the Helix editor line, so no statement below is attributable to a SHA.

Provenance labels used throughout: **measured** means this record's author read
it out of the tree or ran the command at the stated tree state; **parent-run**
means the parent session ran it on this same tree and supplied the transcript,
which this record did not repeat; **source plan** means the statement is the
plan's requirement, not a measurement.

## 1. Executive finding

The primitive is delivered and is the shape the plan asked for. `ListCursor`
(`src/tui/components/scroll.rs:9`) owns a clamped index and a
`CenteredScroll` offset, **composes** the existing scroll rule rather than
reimplementing it (`:113-118` calls `CenteredScroll::clamp`), and carries an
explicit `ScrollPolicy` whose default is `Clamp` (`:2-6`, `:21-32`). The index is
clamped **on every read** (`index()` is `self.index.min(len - 1)`, `:38-40`), so a
shrinking list cannot leave it out of range. The six operations the plan names
exist: `up`, `down`, `page_up`, `page_down`, `home`, `end` (`:61-83`), with
`place` (`:55`) as the shared setter.

The `tui::components::scroll` module has **25 tests, all passing** (measured).
They cover the six boundaries the plan names — empty list, single item, zero
height, a window taller than the list, the first and last item, and a shrinking
list — plus the clamp-versus-wrap pair and the placement/reset rules. The test
names are the evidence, not a count alone: `an_empty_list_keeps_the_cursor_at_zero`,
`a_single_item_list_cannot_move`, `a_zero_height_window_pins_the_offset_to_the_top`,
`a_window_taller_than_the_list_never_scrolls`,
`clamp_stops_at_the_first_item_but_wrap_returns_to_the_last`,
`clamp_stops_at_the_last_item_but_wrap_returns_to_the_first`,
`a_shrinking_list_clamps_a_stale_cursor_on_read`,
`a_shrinking_list_re_seats_the_cursor_on_the_next_step`,
`leaving_the_window_scrolls_and_staying_inside_it_does_not`, and
`reset_returns_both_the_index_and_the_window_to_the_top`.

**Five surfaces adopted it, and each has a policy test that names the policy:**

| Surface | Field | Wrapper | Policy test |
|---|---|---|---|
| `keybind.rs` | `cursor: ListCursor` (`:45`) | `select_up/down` delegate (`:98-104`) | `keybind_cursor_clamps_at_both_ends_and_never_wraps` (`:746`) |
| `completion_overlay.rs` | `cursor: ListCursor` (`:81`) | delegate (`:122-128`) | `completion_overlay_cursor_clamps_at_both_ends_and_never_wraps` (`:353`) |
| `agent.rs` | `cursor: ListCursor` (`:22`) | delegate with `visible_height` (`:78-86`) | `agent_cursor_clamps_at_both_ends_and_never_wraps` (`:497`) |
| `template.rs` | `cursor: ListCursor` (`:18`) | delegate with `visible_height` (`:69-77`) | `template_cursor_clamps_at_both_ends_and_never_wraps` (`:399`) |
| `tree.rs` | `cursor: ListCursor` (`:58`) | `select_up/down` delegate with `visible_height` (`:163-169`) | `tree_cursor_clamps_at_both_ends_and_never_wraps` (`:618`) |

`keybind` and `completion_overlay` are two of the plan's three named
identical-clamp control cases; `agent` is the named clamp-plus-scroll-follow
case; `template` and `tree` are additions the plan did not name. `tree` was the
record's one unresolved candidate: its hand-rolled arithmetic was
`if selected > 0 { -= 1 }` / `if selected < len - 1 { += 1 }` followed by
`if selected < scroll || selected >= scroll + visible { CenteredScroll::clamp }`,
which is exactly `ListCursor::step` plus `follow` (`scroll.rs:85-118`), so it was
converted rather than exempted. The conversion also reads the index through
`index(len)` (`tree.rs:108`), which removes the "highlight nothing" state a
shrinking `flat` could previously reach.

**`project_picker` — the third named control case — is a recorded non-adopter,
and the reason is a different window rule, not the App-owned index.** The index
half does match: `select_up`/`select_down` (`app/state/project_picker.rs:294`,
`:304`) are clamp-only with no wrap. The window half does not. The picker stores
**no** offset; it derives one per render from
`visible_window(filtered_count, selected_row)` (`project_picker.rs:145-160`),
whose end-pinning test is `center + visible/2 >= total`, while
`CenteredScroll::clamp` (`scroll.rs:143-161`) pins at
`cursor >= total - visible`. Those are different windows: for `total = 20`,
`visible = 16`, `selected = 10` the picker starts at row 2 and `CenteredScroll`
starts at row 4; at `selected = 4` the picker starts at 0 and `CenteredScroll`
at 4. Adopting `ListCursor` while keeping the derived window would leave the
cursor's offset unread; adopting its offset would move painted rows. The plan's
§5 prescribes this outcome for a genuinely different policy ("recorded as not
adopted, with the reason"), and §12 puts that list in this record. **Deferred,
not converted**, with the window rule as the concrete difference.

The re-verification pass found a second, independent difference that the
deferral does not depend on but which makes the point concrete: the picker's
window does not take its visible height from the frame at all. It uses
`MAX_PICKER_VISIBLE_ROWS` = 16 (`src/tui/app/state/project_picker.rs:25`), while
the bounded 64×18 dialog spends 2 rows on its outer border, 3+3+3 rows on the
title, filter and footer chunks and 2 more on the list box's own border, so the
list paints **5** rows (`src/tui/components/dialogs/project_picker.rs:188-194`,
`:414-421`; `src/tui/app/render.rs:3163-3179`). The body paints the window's
first five rows and never scrolls within them, so **a cursor at index 5 or
beyond is never drawn**, for any list length. That is a defect recorded in §10,
not this milestone's
to fix. Converting would force a choice the type cannot make: keep the 16-row
window (and its invisible cursor) or adopt the real 5-row height (and change
painted rows). Both are behaviour changes, so neither is a mechanical adoption.

**`McpDialog` is unchanged**, as the plan permits and the invariant requires. It
keeps its `browse_mode`/`action_mode` dispatch and its separate selection fields
(`src/tui/components/dialogs/mcp.rs:71-73`, `:117-158`); it does not use
`ListCursor`. Its two panel-host tests pass:
`mcp_leaves_the_panel_chrome_alone` and
`mcp_advertises_only_the_verbs_its_current_mode_delivers` (**2 passed, 0
failed**, measured). This is the plan's stop condition handled correctly: the
mode dispatch is a different concept, not a duplicate of the clamp.

**The "sixteen copies become one" objective is partly realised.** Sixteen files
still define `select_up`/`select_down` (measured), the same file count as the
plan's baseline — but **five** are now two-line delegates to `ListCursor`
(`keybind`, `completion_overlay`, `agent`, `template`, `tree`), so **eleven**
still own their arithmetic. Every one of those eleven has a concrete difference
(§3), so acceptance criterion 5 is satisfied rather than partial.

**The corpus result is the weaker kind of evidence, and the record says so.**
The family is **1228 passed; 0 failed; 0 filtered out** at the re-verification
pass — measured through `cargo test --test tui_render` (§4); the closeout pass
measured **1227** by running the post-conversion binary, and the parent session
reported the same figure. The plan's
"corpus is byte-identical" control case **cannot be re-measured** in this tree:
there is no commit and therefore no before-state, and the `.inc` corpus files are
already modified by later milestones. The family count moved from **1224 to
1227** before this pass and to **1228** at it (one later test, not attributable
here); this record does **not** attribute the added tests to M009 — its policy
tests are library unit tests in the adopter files, not render-family members. No
converted surface's corpus fixture is known to have changed, and every member is
green, but byte-identity is **not measured**.

The corpus figure has one caveat from the re-verification pass: a first run
reported **1228 passed, 0 failed**, and a second run of the same selection
reported **1227 passed, 1 failed** after another lane rewrote
`src/theme/halloy.rs` at 21:48. The failure is
`invariants::baseline::the_recorded_i5_baseline_is_exactly_what_the_harness_measures`
— an I5 contrast-baseline mismatch across many themes and surfaces, in files
that lane owns and was editing. It is not attributable to this milestone (this
pass edited Markdown only), but a reader should treat the green figure as the
state at 21:42, not as the tree's last word.

Status is conditional, not closed, for two reasons, both of them evidence this
tree cannot produce:

1. **Nothing is committed.** HEAD is `7642b9e38` and every figure is a
   working-tree property, so no condition is attributable to a SHA (§10).
2. **The corpus byte-identity control case is retrospective.** There is no
   before-state in this tree and the `.inc` files already carry later
   milestones' edits. The family is measured green (§4), which proves the
   current behaviour but not that it is unchanged from before the type existed
   (§10).

The plan's own documentation requirement — "the list-state contract" in
`architecture/tui.md` — is met (§9). `ListCursor` and `ScrollPolicy` still carry
no doc comment of their own; that was this record's suggestion rather than the
plan's requirement, and it is recorded as a low, optional item whose **stated
blocker no longer exists**: the comment corpus is banked at 139,245 lines and
`check_comment_budget.py` passes, so the 243-line overage this record cited is
gone. What remains is the absent comment itself, not a budget that forbids it.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `ListCursor` exists with `index` and `offset` | `scroll.rs:9-13` | pass | Both private fields with accessors. |
| It composes `CenteredScroll` rather than replacing it | `offset: CenteredScroll` (`:11`); `follow` calls `offset.clamp` (`:113-118`) | pass | `CenteredScroll`'s centring rule is unmodified (`:143-161`). |
| `ScrollPolicy` distinguishes clamp from wrap and defaults to clamp | `ScrollPolicy` (`:2-6`), `#[default] Clamp`; `with_policy` (`:26-32`); `cursor_defaults_to_the_clamp_policy` (`:239`) | pass | `Wrap` has **no production caller** (§10). |
| Six operations: `up`, `down`, `page_up`, `page_down`, `home`, `end` | `:61-83` | pass | |
| Boundary tests: empty, single, cursor 0, last item, window taller than list, zero height | the 25-test module, names in §1 | pass | Measured: 25 passed, 0 failed. |
| `Wrap` wraps and `Clamp` does not | `clamp_stops_at_…` pair (`:267`, `:279`) | pass | |
| A shrinking item count clamps a stale cursor | `index()` clamps on read (`:38-40`); `a_shrinking_list_clamps_a_stale_cursor_on_read` (`:365`), `…_re_seats_the_cursor_on_the_next_step` (`:377`) | pass | The plan's §8 hazard, asserted. |
| The identical-clamp group adopts: `keybind`, `completion_overlay`, `project_picker` | `keybind.rs:45`, `completion_overlay.rs:81`; **no `ListCursor` in `project_picker.rs`** | **partial** | Two of three converted; the third is a recorded non-adopter whose derived window rule differs from `CenteredScroll::clamp` (§1). |
| The scroll-follow group adopts with its existing visible height | `agent.rs:22`, `:78-86`; `template.rs:18`, `:69-77`; `tree.rs:58`, `:163-169` | pass | `template` and `tree` are additions. |
| A test per converted surface names its policy and asserts the boundary | five `*_cursor_clamps_at_both_ends_and_never_wraps` tests | pass | Each asserts first/last behaviour and `cursor.policy() == ScrollPolicy::Clamp`. |
| `McpDialog` behaviour is unchanged and its test passes | `mcp.rs:71-73`, `:117-158`; `cargo test --test tui_render -- tui_panel_host_chrome::mcp_` → **2 passed, 0 failed** | pass | `browse_mode`/`action_mode` intact; no `ListCursor`. |
| Every non-adopting surface is listed with a concrete difference | §3 table | pass | Eleven of eleven are concrete; `tree.rs` converted. |
| The corpus is byte-identical for every converted surface | no before-state exists in this tree; family **1228/0** (measured at the re-verification pass, §4) | **not measured** | The family count is the only mechanical statement available. |
| No surface gains or loses a key | the adopters' `select_up`/`select_down` call sites are unchanged; `check_tui_composer_key_authority.py` **pass**; `check_tui_editor_key_authority.py` **pass** (2 / 92 / 8) | pass | No key mapping was edited by this milestone. |
| I1–I6 stay green | render corpus **1228 passed, 0 failed** at 21:42 (re-verification pass, §4); a 21:52 re-run fails only the I5 baseline test after another lane's `src/theme/halloy.rs` edit | pass at the stated time | The later failure is that lane's in-flight baseline, not a converted surface. |
| The render-cost shape guard is unchanged | `check_tui_render_cost.py` **pass** (measured this pass); `git status` shows it unmodified | pass | |
| The render ratchet does not move for this milestone | `check_tui_render_invariant_baseline.py` **pass**, with **0 hint-truncation** entries; 2495 contrast. Its advertised-key section is another lane's in-flight edit (4 entries at this reading, 61 when this record was written), so this row rests on the hint-truncation figure | pass | M009 adds no advertised key or contrast pair. |
| The comment ratchet holds | `check_comment_budget.py` passes inside `scripts/verify.sh full` on this tree (2026-11-13), with the corpus banked at `max_comment_lines = 139245` | **pass, 2026-11-13** | The 138,866 / 30-violation state this row recorded was other lanes' in-flight work; it is retired by the re-bank. `tree.rs`'s ceiling is unchanged. |
| `architecture/tui.md` records the list-state contract (one cursor type, explicit policy, a different rule keeps its own and says why) | `### List state and scroll-follow (M009)` (`architecture/tui.md:5422-5445`); module map updated at `:7330` | pass | Plan §12. |
| `scripts/verify.sh quick` is green | **pass** (parent-run): `==> Quick verification passed.` / `QUICK_RC=0`; 94 guards with `--self-test`s, 479 harness tests `OK (skipped=4)`, workspace check with no warnings | pass | |
| `cargo fmt --check`, `clippy -D warnings`, `scripts/verify.sh full` | `full` **pass** (parent-run, 2026-11-12: 17,326 + 8,400 tests, 0 failures); `cargo fmt --all --check` measured this pass | pass | Clippy is inside `full`; not re-run here. |

## 3. Production implementation evidence

- `src/tui/components/scroll.rs:2-6` — `ScrollPolicy` (`Clamp` default, `Wrap`).
- `src/tui/components/scroll.rs:9-119` — `ListCursor`: fields, `new`/`with_policy`,
  the clamped `index()`, `offset()`, `set_index`, `reset`, `place`, the six
  operations, `step_index` (clamp versus wrap), `page`, and `follow`.
- `src/tui/components/scroll.rs:121-162` — `CenteredScroll`, unchanged, still the
  one centring rule.
- `src/tui/components/scroll.rs:164-...` — the 25-test module.
- `src/tui/components/dialogs/keybind.rs:45`, `:98-104`, `:746` — adoption,
  delegation, policy test.
- `src/tui/components/completion_overlay.rs:81`, `:122-128`, `:353` — adoption,
  delegation, policy test.
- `src/tui/components/dialogs/agent.rs:22`, `:78-86`, `:497` — adoption with a
  visible height, policy test.
- `src/tui/components/dialogs/template.rs:18`, `:69-77`, `:399` — adoption with a
  visible height, policy test.
- `src/tui/components/dialogs/tree.rs:58`, `:108`, `:163-169`, `:618` — adoption
  (`cursor: ListCursor`), the clamped read `selected()`, the two delegating
  arms, and the policy test. `App`'s two readers (`app/mod.rs:6594`, `:6667`)
  call `selected()` instead of reading the field.
- `src/tui/app/state/project_picker.rs:294`, `:304` — unchanged; the recorded
  non-adopter (§1).
- `architecture/tui.md:5422-5445` — the list-state contract; `:7330` — the
  module map now names `CenteredScroll, ListCursor`.

No core/domain, storage, protocol, runtime or concurrency change: the plan's §6
marks all four as none, and the change is confined to `src/tui/components/`
plus the two `App` read sites.

**Non-adoption record** (the plan's work package D; every entry is a concrete
difference observed in the code, not a preference):

| Surface | Concrete difference | Adopt? |
|---|---|---|
| `src/tui/components/dialogs/mcp.rs` | Mode dispatch over `browse_mode`/`action_mode` and separate selection fields (`:71-73`, `:117-158`). The plan names this as a different concept. | No — plan-named |
| `src/tui/app/state/project_picker.rs` | No stored offset: the window is derived per render by `visible_window` (`:145-160`), whose end-pinning test (`center + visible/2 >= total`) differs from `CenteredScroll::clamp` (`cursor >= total - visible`); §1 works the difference at `total = 20`. Its visible height is the `MAX_PICKER_VISIBLE_ROWS` = 16 constant, not the frame: the body paints 5 rows (§1), so the type's `visible` argument has no truthful value to take. | No — different window rule |
| `src/tui/components/dialogs/model.rs` | Window size is derived from the **grouped** model count (`count_visible_models`, `:314-318`), not a fixed visible height. | No |
| `src/tui/components/dialogs/theme.rs` | Same grouped visible count, plus a live preview side effect on move (`preview_theme`, `:157-166`). | No |
| `src/tui/components/dialogs/question.rs` | Selection is per question and each move rewrites `current_input` and `cursor_pos` (`:163-169`). | No |
| `src/tui/components/dialogs/security_review.rs` | A move resets a second, detail-pane scroll (`detail_scroll = 0`, `:122-127`). | No |
| `src/tui/components/dialogs/queue.rs` | A move calls `clamp_selection()` after the index change (`:303-308`). | No |
| `src/tui/components/dialogs/review.rs` | Edge-pinned scroll (`scroll = selected` when above), not centred (`:92-99`). | No |
| `src/tui/components/dialogs/research.rs` | No scroll field at all and no follow (`:110-119`). | No |
| `src/tui/app/state/workspace_dashboard.rs` | App-owned state, plain clamp, no scroll offset (`:184-190`). | No |
| `src/tui/components/dialogs/session.rs` | Selection walks a computed row projection and normalises a public field (`:549-558`). | No |

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`,
with `CARGO_BUILD_JOBS=2`:

```bash
# first pass (the record that produced the conditional status)
cargo test -p codegg --lib -- tui::components::scroll
cargo test --test tui_render -- tui_panel_host_chrome::mcp_
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_render_invariant_baseline.py

# closeout pass (this session)
cargo test -p codegg --lib -- tui::components::dialogs::tree
cargo test --test tui_render -- tui_project_picker
cargo test --test tui_render -- tui_panel_host_chrome tui_theme_source_of_truth
cargo test --test tui_render -- tui_render::
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
rustfmt --edition 2021 --check src/tui/components/dialogs/tree.rs src/tui/app/mod.rs
```

The whole family was then run from the corpus binary already built **after** the
`tree.rs` conversion (19:08; the only later edit to the file was the removal of
nine comment lines), because the shared target dir's build lock was held by
other lanes for the rest of the pass:

```bash
./target/debug/deps/tui_render-c222fb5313649f59   # 1227 passed, 0 failed, 0 filtered
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib -- tui::components::scroll` | **pass** — **25 passed, 0 failed**, 8157 filtered out, 0.00 s |
| `cargo test -p codegg --lib -- tui::components::dialogs::tree` | **pass** — **7 passed, 0 failed**, 8176 filtered out (six existing + the new policy test) |
| `cargo test --test tui_render -- tui_project_picker` | **pass** — **22 passed, 0 failed**, 1205 filtered out |
| `cargo test --test tui_render -- tui_panel_host_chrome tui_theme_source_of_truth` | **pass** — **33 passed, 0 failed**, 1194 filtered out (baselined before the conversion) |
| `cargo test --test tui_render -- tui_render::` | **pass** — **111 passed, 0 failed**, 1116 filtered out (baselined before the conversion) |
| Whole family (`./target/debug/deps/tui_render-c222…`) | **1227 passed, 0 failed, 0 filtered**, 29.23 s |
| `cargo test --test tui_render -- tui_panel_host_chrome::mcp_` | **pass** — **2 passed, 0 failed** (first pass) |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once"; guard unmodified |
| `check_tui_composer_key_authority.py` | **pass** — closed, single-owner claim table |
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block, 92 Normal-mode [9 digits claimed], 8 Insert-mode, no shadowed arm" |
| `check_tui_render_invariant_baseline.py` | **pass** — 61 advertised-key / 2495 contrast / 0 hint-truncation, none added unbanked |
| `check_test_assertion_quality.py` | **pass** — "all checks pass" |
| `check_test_duplication.py` | **pass** — "all checks pass" |
| `rustfmt --edition 2021 --check` on the two edited files | **pass** |
| `ListCursor` grep | **5 adopting surfaces** + the type itself; `Wrap` constructed only in tests |
| `select_up`/`select_down` grep | **16 files** still define them; 5 are delegates, so **11** still own arithmetic |
| `architecture/tui.md` grep | contract at `:5422-5445`; module map at `:7330` |

### Re-verification pass (this session)

This pass changed no Rust. It re-read every adoption site, re-measured the
corpus through cargo, and re-ran the two selectors the plan names. Local, same
tree, `CARGO_BUILD_JOBS=2`, shared target dir (no lock contention this pass):

```bash
cargo test --test tui_render
cargo test --test tui_render -- invariants
cargo test -p codegg --lib -- tui::components::scroll
cargo test --test tui_render -- tui_project_picker
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_invariant_baseline.py
git diff HEAD -- src/tui/components/dialogs/project_picker.rs
```

| Command | Result |
|---|---|
| `cargo test --test tui_render` | **pass** — **1228 passed, 0 failed, 0 filtered out**, 7.05 s |
| same command, run again after another lane rewrote `src/theme/halloy.rs` (21:48) | **1227 passed, 1 failed** — `invariants::baseline::the_recorded_i5_baseline_is_exactly_what_the_harness_measures`: contrast rows are recorded and no longer reproduce, and one measured row is unrecorded. That file, the manifest and `src/theme/**` belong to other lanes and were being edited; this pass changed Markdown only, and the selection is the same 1228 tests. |
| `cargo test --test tui_render -- invariants` | **pass** — **32 passed, 0 failed**, 1196 filtered out |
| `cargo test -p codegg --lib -- tui::components::scroll` | **pass** — **25 passed, 0 failed**, 8175 filtered out |
| `cargo test --test tui_render -- tui_project_picker` | **pass** — **22 passed, 0 failed**, 1206 filtered out |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once"; guard unmodified |
| `check_tui_render_invariant_baseline.py` | **pass** — **0 hint-truncation**; the advertised-key section read 4 entries because another lane is editing that file (61 when this record was written), so this pass cites only the hint-truncation figure |
| `ListCursor` grep | **5 adopting surfaces** (`keybind`, `completion_overlay`, `agent`, `template`, `tree`) plus the type itself |
| `select_up`/`select_down` grep | **16 files** still define them; 5 are delegates, so **11** still own arithmetic |
| `project_picker` diff against HEAD | the window arithmetic (`visible_window`, `MAX_PICKER_VISIBLE_ROWS`) is **unchanged**; the only edits are the `Section`/`preferred_area` sizing seam |

### Parent-run evidence

| Command | Result |
|---|---|
| `scripts/verify.sh full` | **pass** at 2026-11-12 — 17,326 + 8,400 tests, 0 failures |
| `scripts/verify.sh quick` | **pass** — `==> Quick verification passed.` / `QUICK_RC=0` |
| comment budget | **not green now**: 30 violations, 138,866 lines vs the banked 138,582 (re-measured at the re-verification pass), from other in-flight files (§5, §10) |

### Not run

| Command | Why |
|---|---|
| `cargo test --test tui_render` through cargo (whole family) | Not run at the closeout pass (the shared target dir's lock was held by other lanes); the post-conversion binary was run directly instead. **Run through cargo at the re-verification pass: 1228 passed, 0 failed.** |
| `scripts/verify.sh full` / `quick` | Forbidden by this session's instructions; cited as parent-run. |
| `cargo fmt --all --check` | Not clean in this tree, but only in other lanes' files (`src/managed_process.rs`, `src/security/*`); the two files this pass edited pass `rustfmt --check`. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Inside `full`; not run individually. |

**Every result is a property of this working tree at the time it was read.** The
tree carried 238 changed or untracked paths at the closeout pass.

## 5. Invariant review

- **Every converted surface keeps its exact policy.** The five adopters all use
  `ScrollPolicy::Clamp`, and each has a test that names the policy and asserts
  first/last behaviour. Holds for the converted set; the eleven non-adopters are
  unchanged by construction.
- **`McpDialog` behaviour is unchanged**, including its
  `browse_mode`/`action_mode` dispatch. Holds — no `ListCursor` reference, and
  its two tests pass.
- **The corpus is byte-identical.** **Not verified**: no before-state exists in
  this tree. The family is measured green (§4), and no converted surface's
  fixture is known to have changed. Recorded as not measured rather than
  claimed.
- **No surface gains or loses a key.** Holds: the wrappers keep the same call
  sites, no key mapping is edited, and the composer and editor key-authority
  guards pass.
- **I1–I6 stay green.** The corpus is measured green this pass (§4).
- **The render-cost shape guard is unchanged.** Pass and unmodified.
- **The comment ratchet holds.** **Verified 2026-11-13**: `check_comment_budget.py`
  passes inside `scripts/verify.sh full`, with the corpus banked at 139,245
  comment lines. The 30 violations this record reported belonged to other lanes'
  in-flight files, not to this milestone.

## 6. Failure and recovery review

- **A shrinking item count under a cursor** is the plan's one hazard. `index()`
  clamps on every read, so a stale stored index cannot be observed out of range;
  `a_shrinking_list_clamps_a_stale_cursor_on_read` and
  `…_re_seats_the_cursor_on_the_next_step` assert both the read and the next
  step. No panic and no "highlight nothing" state is reachable through the type.
- **A cursor set beyond the item count** renders a valid selection because every
  consumer reads through `index()`; the same two tests cover it. `tree.rs` now
  reads through `selected()` (`:108`), so a `flat` that shrinks under a collapse
  cannot leave the box highlighting nothing either.
- **Duplicate delivery, restart, cancellation, stale generations, contention.**
  Not applicable: cursor state is synchronous, per-surface and in memory. No
  queue, request, lock or durable state is added. A surface that reconstructs its
  state resets its own cursor exactly as before (`reset`, `:50-53`).
- **A page on a zero-height window** still moves one row
  (`a_page_of_a_zero_height_window_still_moves_one_row`), so a degenerate frame
  cannot trap the cursor.
- **Partial failure.** There is no fallible operation; the type returns the new
  index and updates its offset.

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency migration; no schema, DTO or
manifest change. The change is internal to `src/tui/components/` and the five
adopting surfaces, plus two `App` read sites.

Selection and scrolling behave identically for the converted surfaces (clamp
semantics and the existing centring rule are preserved), and every non-converted
surface keeps its own implementation, so there is nothing for a user to migrate
and no rollback limitation beyond reverting the type and the five delegations.

## 8. Security review

No authorization, path, process, secret or network surface is touched. The one
bounded-input property relevant to a cursor is that an index beyond the item
count cannot panic or produce an invalid selection — it clamps on read — and that
is asserted. `page` caps its loop at `visible.max(1)` iterations, so a reported
visible height cannot drive an unbounded loop. Nothing else applies.

## 9. Documentation and operations

- **`architecture/tui.md` now owns the list-state contract** at
  `### List state and scroll-follow (M009)` (`:5422-5445`): one cursor type,
  the clamped index and the scroll-follow as one rule, an explicit
  `ScrollPolicy`, and the statement that a surface with a different rule keeps
  its own and says why — with the non-adoption list pointed at this record,
  `McpDialog`'s mode dispatch named as the documented exception, and
  `ProjectPickerDialog` named as the plan's third control case that does not
  adopt (derived window; the type's offset would have no reader). The directory
  map's `scroll.rs` line now reads `CenteredScroll, ListCursor (index + follow)`
  (`:7330`).
- **`ListCursor` and `ScrollPolicy` carry no doc comment.** `scroll.rs` opens
  with a `derive` and has no module or type prose. The contract is now in
  `architecture/tui.md`, so this is no longer a correctness gap; it is recorded
  because the earlier condition named it and the type is where a contributor
  looks first. `src/tui/components/scroll.rs` is outside this pass's ownership,
  and the comment ratchet has no headroom (§10).
- No guard change is required or was made; the render-cost guard is unmodified.
- Operator diagnostics: none. There is no new command, flag, log line or
  recovery instruction.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **`ProjectPickerDialog`'s window is larger than the rows it paints.** `MAX_PICKER_VISIBLE_ROWS` is 16 (`app/state/project_picker.rs:25`) while the bounded 64×18 dialog paints 5 list rows (`project_picker.rs:188-194`, `:414-421`; `render.rs:3163-3179`). Pre-existing and unchanged: `git diff HEAD` shows the window arithmetic untouched, and HEAD also reached the same 64×18 rect (`render_project_picker` called `picker_area(area)` directly); the sizing seam moved that computation into `preferred_area` without changing the result. | **A cursor at index 5 or beyond is never drawn**, for any list length — with 10+ projects the highlight disappears after five moves. This is also why a mechanical `ListCursor` adoption is unavailable: its `visible` argument has no truthful value on this surface. | Convert the picker to the real painted height and a stored offset together, with a test that renders its body; that changes painted rows, so it needs its own change, not this documentation pass. |
| low | **The corpus byte-identity control case is not reproducible.** There is no commit and the `.inc` files are already modified by later milestones. | A reviewer can confirm the family is green but cannot confirm that a converted surface's rendered frame is unchanged. | Land milestones as separate commits, or record the corpus digest when the type is introduced. Not fixable retrospectively. |
| low | **Nothing is committed**; HEAD is `7642b9e38`, 239 paths dirty or untracked at the re-verification pass's reading. | Every figure is a working-tree property, not a SHA. | Land the milestone as its own commit. |
| low | **`ListCursor`/`ScrollPolicy` have no doc comment** (`scroll.rs:1-119`). | The contract is in `architecture/tui.md` and tests, not at the type. | **The stated blocker is gone**: the comment corpus is banked at 139,245 lines and `check_comment_budget.py` passes, so there is headroom now. The missing comment is a residue to be written, no longer a blocked item. |
| low | **`ScrollPolicy::Wrap` has no production caller** — it is constructed only in tests. | The policy axis exists and is tested, but no surface exercises wrap in production, so the variant is unproven against a real surface. | Keep it if a wrapping surface is expected; otherwise record it as unused rather than letting a reader infer that wrap is in use. |
| low | **Eleven of the sixteen files still own their arithmetic.** The plan's headline is "sixteen copies of one rule become one"; five are converted. | The milestone's value is the reusable type plus five conversions, not a completed deduplication. All eleven have a concrete difference. | Record the counted state (5 of 16) in the roadmap so a later reader does not read "one `ListCursor`" as "no hand-rolled selection left". |
| — | **Resolved this pass:** the `project_picker` decision (recorded non-adopter, §1), the `architecture/tui.md` contract (§9), `tree.rs` (converted, §3), and green `full` (parent-run, §4). | — | — |
| — | **Re-verified at the re-verification pass:** the five adoption sites and their policy tests, the contract's line references, `tree.rs`'s clamped read, and the corpus through cargo (**1228/0**). No Rust was changed, so the corpus is a re-measurement of the same tree, not a before/after pair. | — | — |
| — | **Discharged 2026-11-13:** `check_comment_budget.py` is green with the corpus banked at 139,245 lines; the 30 violations this row recorded belonged to other lanes' in-flight files. | The ratchet is green on this tree. | — |

No high or critical finding. No migration, protocol variant, crate or new
authority was added.

## 11. Roadmap disposition

**Milestone conditionally closed.** The primitive and its boundary tests are
delivered, five surfaces adopt it with per-surface policy tests, `McpDialog` is
unchanged and tested, and the render-cost guard is unmodified. The three
implementation conditions this record set are discharged:

1. **`project_picker` decision recorded** — a non-adopter, with the derived
   window rule and the worked arithmetic as the concrete difference (§1). The
   plan's §5 already prescribes that outcome for a genuinely different policy.
2. **List-state contract written** in `architecture/tui.md:5422-5445`, and the
   module map updated (`:7330`). The type's own doc comment is still not added,
   and its stated blocker is **gone**: the comment corpus is banked at 139,245
   lines and `check_comment_budget.py` passes, so headroom is no longer the
   reason. It is recorded as a low residue (§10).
3. **`tree.rs` converted** with a policy test and the clamped read (§3), so
   acceptance criterion 5 has no unexplained non-adopter.
4. **`scripts/verify.sh full` green** on this tree (parent-run, 2026-11-12:
   17,326 + 8,400 tests, 0 failures); `quick` green (parent-run). Not re-run
   here — the verdict is cited as parent-run evidence, and this pass's focused
   runs are recorded in §4.

Items 1–3 were re-verified at the re-verification pass against the code and a
fresh corpus run (**1228/0**, §4); item 4 was not re-run, and no pass here can
produce the two remaining conditions.

Two conditions remain, and neither is obtainable in this session:

- **A commit carrying the milestone.** Nothing is committed; every figure is a
  property of HEAD `7642b9e38` plus the working tree.
- **A before-state for the corpus byte-identity claim.** The family is measured
  green, but "byte-identical to before the type" is retrospective evidence this
  tree cannot produce.

- **Risk.** Low on behaviour: the 25 module tests cover every boundary the plan
  names, the five policy tests pin clamp semantics, `McpDialog`'s two tests
  pass, the corpus is measured green, and `quick`/`full` are green (parent-run).
  The residual is provenance (a SHA, a before-state) and one low doc item for
  the converted set. The picker's window defect (§10, medium) is **pre-existing
  and on a surface this milestone deliberately did not convert**; it is recorded
  rather than treated as a regression, but a reader should not read "no
  regression" as "the picker's scrolling works".
- **Exact future evidence.** The `full` transcript labelled with the commit that
  carries M009, and — if byte-identity is to be claimed — a corpus digest taken
  before the type was introduced. The `scroll.rs` doc comment is now unblocked
  and is a plain write. The picker's real-height conversion is separate work with
  its own render test.

M010 (slash dispatch tables) is independent: it concerns command routing, not
list state, and this milestone neither changes nor blocks it.

## 12. Registry updates

- `plans/registry.md` line 687 (the M009 row): status
  `**Blocked — hard dependency on M001**` is false for the shipped work; it must
  become **conditionally closed**, with this record's path and the two remaining
  conditions (a commit; a corpus before-state).
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` line 690
  (the M009 row): `blocked` → `conditionally closed`, closure record
  `plans/closure/tui-modularity-and-render-invariants/009-status.md`, blockers
  "a commit carrying the milestone; a before-state for the corpus
  byte-identity claim".
- `plans/registry.md` line 29 (the subsystem row): "M001–M004 implemented,
  M005–M010 in progress or pending" must name M009's record and its conditions.
- `plans/implementation/tui-modularity-and-render-invariants/009-list-cursor-and-scroll-follow.md`
  header: it already reads `Status: conditionally closed`, matching this record;
  no edit was needed. §11's `tui_render_invariants` selector was corrected to
  `invariants`, the same factual fix as M005's.
- Record the counted state so the roadmap does not overstate the deduplication:
  **`ListCursor` exists; 5 surfaces adopt it (keybind, completion_overlay, agent,
  template, tree); 11 of the 16 files that define `select_up`/`select_down` still
  own their arithmetic; `McpDialog` is a plan-named non-adopter; `project_picker`
  is a recorded non-adopter with a different window rule; `tree.rs` is
  converted.**
- Record the measured figures: scroll module **25 / 0**, dialogs/tree
  **7 / 0** (six existing + the new policy test), project-picker member
  **22 / 0**, panel-host + theme members **33 / 0**, `tui_render` member
  **111 / 0**, vocabulary **3 / 0**, whole render family **1228 / 0** at the
  re-verification pass, render baseline **0 hint-truncation** (its advertised-key
  section is another lane's in-flight edit), render-cost guard pass and
  unmodified, `quick`/`full` green (parent-run).
- Record the picker's own defect (§10, medium) against the picker surface, not
  against this milestone: its window arithmetic is unchanged from HEAD and the
  milestone did not convert it.
- Do **not** mark M009 closed on the strength of this record: the two conditions
  in §11 (a commit; a corpus before-state) stand between conditional closure and
  closure.

## Correction log (2026-11-13)

- **§1, the `ListCursor`/`ScrollPolicy` doc-comment paragraph.** Was "recorded as a low, optional item … because … the comment corpus is over its banked ceiling (243 lines …)". The stated blocker is gone: `docs/comment-budget.toml` records `max_comment_lines = 139245` and `check_comment_budget.py` passes inside `scripts/verify.sh full` at 2026-11-13. The doc comment is still absent — that is the residue — but nothing in the budget forbids it.
- **§2 and §5, the comment-ratchet rows.** Were "fail, not attributable — 138,866 lines against 138,582, 30 violations". Met by the re-bank at 139,245 and the green guard inside the 2026-11-13 `full`.
- **§10, the no-doc-comment finding.** "Blocked by §5's measured overage, not by judgement" is replaced by the truth: there is headroom now, so the missing comment is a residue to be written.
- **§10, the "Not this milestone" comment-budget row.** Discharged by the same `full`.
- **§11 item 2 and the future-evidence bullet.** Item 2 said the doc comment was not added "because … the comment ratchet has no headroom"; that reason is retired, and the comment is now a plain write. The future-evidence bullet no longer says "when the comment budget is re-banked".

**No promotion.** The status stays `conditionally closed`: the tree is uncommitted, the corpus byte-identity control case has no before-state, `ScrollPolicy::Wrap` has no production caller, 11 of 16 files still own their selection arithmetic, the picker's window defect is recorded against that surface, and the type's doc comment is still unwritten. None of those conditions was deleted; the one whose *blocker* disappeared now says so.
